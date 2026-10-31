//! The console: the local shell, a remote one, a container's, and what their output says.

use super::{App, Flow};
use anyhow::Result;
use borg_core::{Msg, ShellView};
use borg_i18n::tr;
use borg_term::{Emulator, GridSize, Pty, PtyEvent, Scroll};
use std::time::Instant;

impl App {
    /// The terminal and process of the console: the host's shell, or the container shell
    /// whose panel is active.
    pub(super) fn local_console(&mut self) -> (&mut Emulator, &mut Pty) {
        if let borg_core::Console::Container(id) = &self.console
            && self.shells.screen(id).is_some()
        {
            let id = id.clone();
            if let Some(parts) = self.shells.parts(&id) {
                return parts;
            }
        }
        (&mut self.emulator, &mut self.pty)
    }

    pub(super) fn console_emulator(&mut self) -> &mut Emulator {
        if let borg_core::Console::Remote { machine, session } = &self.console {
            return self
                .remote_consoles
                .emulator(&(machine.clone(), session.clone()))
                .expect("remote console emulator");
        }
        self.local_console().0
    }

    pub(super) fn write_console(&mut self, bytes: Vec<u8>) -> Result<()> {
        if let borg_core::Console::Remote { machine, session } = &self.console {
            if let Err(error) = self.remote_consoles.write(&(machine.clone(), session.clone()), bytes) {
                self.state.notice = Some(tr!(remote_console_error, machine = machine, error = error));
            }
        } else {
            let (emulator, pty) = self.local_console();
            emulator.scroll(Scroll::Bottom);
            pty.write(&bytes)?;
        }
        Ok(())
    }

    pub(super) fn on_remote_console(
        &mut self,
        key: (String, String),
        event: crate::remote_console::Event,
    ) -> Result<Flow> {
        match event {
            crate::remote_console::Event::Output { bytes, foreground, raw_input } => {
                self.remote_consoles.set_input_mode(&key, foreground, raw_input);
                let Some(emulator) = self.remote_consoles.emulator(&key) else { return Ok(Flow::Continue) };
                let signals = emulator.feed(&bytes);
                // What a command Borg typed writes is held back and echoes nothing typed.
                let echoed = !bytes.is_empty() && !emulator.concealing();
                let replies = emulator.drain_replies();
                for reply in replies {
                    let _ = self.remote_consoles.write(&key, reply.into_bytes());
                }
                let emulator = self.remote_consoles.emulator(&key).expect("remote console emulator");
                let view = ShellView {
                    prompt: emulator.prompt_state(),
                    command_rows: emulator.command_rows(),
                    foreground,
                    raw_input,
                    mode: emulator.mode(),
                };
                self.dispatch(Msg::RemoteConsoleOutput { echoed, machine: key.0, session: key.1, view, signals })
            }
            crate::remote_console::Event::Closed(error) => {
                if self.remote_consoles.screen(&key).is_none() {
                    return Ok(Flow::Continue);
                }
                let error = tr!(remote_console_error, machine = key.0, error = error);
                self.dispatch(Msg::RemoteConsoleClosed { machine: key.0.clone(), session: key.1.clone(), error })?;
                self.remote_consoles.close(&key);
                Ok(Flow::Continue)
            }
        }
    }

    pub(super) fn on_pty(&mut self, event: PtyEvent) -> Result<Flow> {
        let mut flow = self.consume(event)?;
        while let (Flow::Continue, Ok(next)) = (&flow, self.pty_events.try_recv()) {
            flow = self.consume(next)?;
        }
        match flow {
            Flow::Continue if self.console == borg_core::Console::Host => self.observe_output(),
            flow => Ok(flow),
        }
    }

    pub(super) fn consume(&mut self, event: PtyEvent) -> Result<Flow> {
        match event {
            PtyEvent::Output(bytes) => {
                if let Some((_, captured)) = self.host_capture.as_mut() {
                    captured.extend_from_slice(&bytes);
                }
                let signals = self.emulator.feed(&bytes);
                for reply in self.emulator.drain_replies() {
                    self.pty.write(reply.as_bytes())?;
                }
                let ended = signals.iter().find_map(|signal| match signal {
                    borg_term::ShellSignal::CommandEnd { exit } => Some(*exit),
                    _ => None,
                });
                let console = self.console == borg_core::Console::Host;
                for signal in signals {
                    let msg = if console { Msg::Signal(signal) } else { Msg::ParkedSignal { container: None, signal } };
                    if let Flow::Quit = self.dispatch(msg)? {
                        return Ok(Flow::Quit);
                    }
                }
                if let (Some(exit), Some((command, captured))) = (ended, self.host_capture.take_if(|_| ended.is_some()))
                {
                    let output = crate::agent::shell_output(&captured, &command.line);
                    command.finish(exit, &output, &self.messages);
                }
                Ok(Flow::Continue)
            }
            PtyEvent::Closed => {
                if let Some((command, _)) = self.host_capture.take() {
                    command.fail("The shell closed before the command finished.", &self.messages);
                }
                self.dispatch(Msg::ShellExited)
            }
        }
    }

    pub(super) fn on_container_shell(&mut self, container: String, event: PtyEvent) -> Result<Flow> {
        match event {
            PtyEvent::Output(bytes) => {
                let Some((mode, signals, captured)) = self.shells.feed(&container, &bytes) else {
                    return Ok(Flow::Continue);
                };
                if let Some(crate::docker::Captured { command, exit, output }) = captured {
                    command.finish(exit, &output, &self.messages);
                }
                let console = self.console == borg_core::Console::Container(container.clone());
                for signal in signals {
                    let msg = if console {
                        Msg::Signal(signal)
                    } else {
                        Msg::ParkedSignal { container: Some(container.clone()), signal }
                    };
                    if let Flow::Quit = self.dispatch(msg)? {
                        return Ok(Flow::Quit);
                    }
                }
                if console { self.observe_output() } else { self.dispatch(Msg::DockerShellOutput { container, mode }) }
            }
            PtyEvent::Closed => {
                if let Some(typed) = self.shells.close(&container) {
                    typed.fail("The shell closed before the command finished.", &self.messages);
                }
                self.dispatch(Msg::DockerShellClosed { container })
            }
        }
    }

    /// Keeps container shells the size they show at: the Docker view's panel, or the whole
    /// screen while one is the console.
    pub(super) fn fit_container_shell(&mut self) {
        self.remote_consoles.fit(GridSize::console(self.state.cols, self.state.rows));
        if let borg_core::Console::Container(id) = &self.console {
            let id = id.clone();
            self.shells.fit(&id, GridSize::console(self.state.cols, self.state.rows));
        }
        let docker = &self.state.docker;
        let (Some(shell), Some(side)) = (docker.visible_shell(), docker.side) else { return };
        let area = borg_core::shell_rect(self.state.layout(Instant::now()).panels[side.other().index()]);
        if area.width > 0 && area.height > 0 {
            let container = shell.container.clone();
            self.shells.fit(&container, GridSize { cols: area.width, rows: area.height });
        }
    }

    /// Reports the console after its output. Output held back while a command Borg typed runs
    /// shows nothing the user typed, so the command line still counts as holding it.
    pub(super) fn observe_output(&mut self) -> Result<Flow> {
        if self.console_emulator().concealing() { self.observe(Msg::Observe) } else { self.observe(Msg::ShellOutput) }
    }

    pub(super) fn observe(&mut self, message: fn(ShellView) -> Msg) -> Result<Flow> {
        let (foreground, raw_input) = if let borg_core::Console::Remote { machine, session } = &self.console {
            self.remote_consoles.input_mode(&(machine.clone(), session.clone()))
        } else {
            let pty = self.local_console().1;
            (pty.foreground(), pty.raw_input())
        };
        let emulator = self.console_emulator();
        let view = ShellView {
            prompt: emulator.prompt_state(),
            command_rows: emulator.command_rows(),
            foreground,
            raw_input,
            mode: emulator.mode(),
        };
        self.dispatch(message(view))
    }

    pub(super) fn container_shell(&mut self, request: borg_core::ShellRequest) {
        use borg_core::ShellRequest;
        match request {
            ShellRequest::Open { container, prompt, size, workdir } => {
                if let Err(error) = self.shells.open(container.clone(), &prompt, size, workdir.as_deref()) {
                    let _ = self.messages.send(Msg::Notice(format!("{error:#}")));
                    let _ = self.messages.send(Msg::DockerShellClosed { container });
                }
            }
            ShellRequest::Input { container, bytes } => self.shells.input(&container, &bytes),
            ShellRequest::Scroll { container, scroll } => self.shells.scroll(&container, scroll),
        }
    }
}
