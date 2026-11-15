//! Listening for neighbors: tcpdump keeps the broadcast and multicast packets hosts announce
//! themselves in, as you or as administrator as a capture does, and Borg reads them as they
//! come and reports what they said every second. Nothing is saved and nothing is sent.

use super::{PROGRESS_EVERY, READ_CHUNK, Tcpdump, elevated, explain, own, sleep_until};
use borg_core::{CaptureFailure, ListenEvent, ListenRequest, LiveFrames, Msg, Tool, hear};
use borg_i18n::tr;
use borg_jobs::Control;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::AsyncReadExt,
    sync::{mpsc::UnboundedSender, oneshot},
    time::Instant,
};

/// How often the listener looks whether its job was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(200);

/// Listens as `request` asks, as job `id`, until its time is up or `control` cancels it.
pub(crate) fn listen(id: u64, request: ListenRequest, control: Arc<Control>, messages: UnboundedSender<Msg>) {
    tokio::spawn(async move {
        let report = |event| drop(messages.send(Msg::Listen { id, event }));
        let result = match crate::tools::find(Tool::Tcpdump.program()) {
            Some(program) => run(&program, &request, control, report).await,
            None => Err(CaptureFailure::Missing(crate::tools::installer())),
        };
        report(ListenEvent::Finished(result));
    });
}

/// Fires `stop` once `control` is cancelled.
async fn watch(control: Arc<Control>, stop: oneshot::Sender<()>) {
    while !control.cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
    let _ = stop.send(());
}

/// Runs tcpdump with the listen filter and tells `report` when it started and what the packets
/// it kept announced, every second, until the deadline or a cancel.
async fn run(
    program: &Path,
    request: &ListenRequest,
    control: Arc<Control>,
    report: impl Fn(ListenEvent),
) -> Result<(), CaptureFailure> {
    let (stop, mut stopped) = oneshot::channel();
    let watcher = tokio::spawn(watch(control, stop));
    let tcpdump = Tcpdump::listening(&request.interface);
    let started = if request.administrator {
        let prompt = tr!(listen_admin_prompt, interface = request.interface);
        elevated::start(&tcpdump, &prompt, &mut stopped).await
    } else {
        own(program, &tcpdump)
    };
    let (mut packets, mut tap) = match started {
        Ok(started) => started,
        Err(failure) => {
            watcher.abort();
            return Err(failure);
        }
    };
    report(ListenEvent::Started);
    let deadline = request.seconds.map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let mut frames = LiveFrames::default();
    let mut heard = Vec::new();
    let mut chunk = vec![0; READ_CHUNK];
    let mut ticks = tokio::time::interval(PROGRESS_EVERY);
    let (mut stopping, mut cancelled, mut read_any, mut failure) = (false, false, false, None);
    loop {
        tokio::select! {
            read = packets.read(&mut chunk) => match read {
                Ok(0) => break,
                Ok(count) => {
                    read_any = true;
                    for frame in frames.feed(&chunk[..count]) {
                        heard.extend(hear(frame.link, &frame.data));
                    }
                }
                Err(error) => {
                    failure = Some(tr!(capture_failed, error = error));
                    tap.stop();
                    break;
                }
            },
            _ = ticks.tick() => {
                report(ListenEvent::Heard { heard: std::mem::take(&mut heard), at: crate::network::epoch_seconds() });
            }
            _ = sleep_until(deadline), if !stopping => {
                stopping = true;
                tap.stop();
            }
            _ = &mut stopped, if !stopping => {
                (stopping, cancelled) = (true, true);
                tap.stop();
            }
        }
    }
    watcher.abort();
    if !heard.is_empty() {
        report(ListenEvent::Heard { heard, at: crate::network::epoch_seconds() });
    }
    drop(packets);
    let said = tap.finish().await;
    if let Some(failure) = failure {
        return Err(CaptureFailure::Failed(failure));
    }
    let said = said?;
    if !stopping && !read_any {
        return Err(explain(&said));
    }
    if cancelled { Err(CaptureFailure::Cancelled) } else { Ok(()) }
}

#[cfg(test)]
mod tests;
