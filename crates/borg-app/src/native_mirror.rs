use std::{
    collections::VecDeque,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use crossterm::event::{Event, KeyEventKind};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    buffer::{Buffer, Cell, CellWidth},
    layout::Position,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{mpsc, oneshot, watch},
    task::{JoinHandle, JoinSet},
};
use unicode_width::UnicodeWidthStr;

use crate::agent::{Call, Reply};

const REPLAY_LIMIT: usize = 1024 * 1024;
const REQUEST_LIMIT: usize = 512 * 1024;
const EVENT_LIMIT: usize = 128;
const PASTE_LIMIT: usize = 64 * 1024;
const REQUEST_WAIT: Duration = Duration::from_secs(5);

type Patch = (u16, u16, Cell);

struct Delta {
    sequence: u64,
    cells: Vec<Patch>,
    bytes: usize,
}

#[derive(Default)]
struct Screen {
    buffer: Option<Buffer>,
    cursor: Option<Position>,
    sequence: u64,
    full_at: u64,
    replay: VecDeque<Delta>,
    bytes: usize,
    controller: Option<String>,
    /// The last copy, base64-encoded, and the sequence it came with: only the controlling
    /// viewer receives it, once, to put on the clipboard of the computer it runs on.
    clipboard: Option<(u64, String)>,
}

impl Screen {
    fn advance(&mut self, cells: Vec<Patch>) {
        self.sequence = self.sequence.saturating_add(1);
        let bytes = cells.iter().map(|(_, _, cell)| std::mem::size_of::<Patch>() + cell.symbol().len()).sum();
        self.replay.push_back(Delta { sequence: self.sequence, cells, bytes });
        self.bytes += bytes;
        while self.bytes > REPLAY_LIMIT || self.replay.len() > 128 {
            if let Some(delta) = self.replay.pop_front() {
                self.bytes -= delta.bytes;
            }
        }
    }

    fn publish(&mut self, buffer: &Buffer, cursor: Option<Position>) -> bool {
        let changed_size = self.buffer.as_ref().is_none_or(|old| old.area != buffer.area);
        let empty = Buffer::empty(buffer.area);
        let previous = self.buffer.as_ref().filter(|old| old.area == buffer.area).unwrap_or(&empty);
        let cells = previous.diff(buffer).into_iter().map(|(x, y, cell)| (x, y, cell.clone())).collect::<Vec<_>>();
        if cells.is_empty() && self.cursor == cursor && !changed_size {
            return false;
        }
        if changed_size {
            self.replay.clear();
            self.bytes = 0;
        }
        self.buffer = Some(buffer.clone());
        self.cursor = cursor;
        self.advance(cells);
        if changed_size {
            self.full_at = self.sequence;
        }
        true
    }

    /// Keeps `text` for the controlling viewer's next frame; false when no viewer controls.
    fn copy(&mut self, text: &str) -> bool {
        if self.controller.is_none() {
            return false;
        }
        self.advance(Vec::new());
        self.clipboard = Some((self.sequence, STANDARD.encode(text)));
        true
    }

    fn frame(&self, after: u64, cols: u16, rows: u16, origin: &str) -> Result<Reply> {
        let buffer = self.buffer.as_ref().context("The session screen is not ready yet")?;
        let oldest = self.replay.front().map_or(self.sequence, |delta| delta.sequence.saturating_sub(1));
        let reset = after == 0 || after < oldest || after < self.full_at || after > self.sequence;
        let cells = if reset {
            Buffer::empty(buffer.area).diff(buffer)
        } else {
            let cells: Vec<_> = self
                .replay
                .iter()
                .filter(|delta| delta.sequence > after)
                .flat_map(|delta| delta.cells.iter().map(|(x, y, cell)| (*x, *y, cell)))
                .collect();
            let batches = self.replay.iter().filter(|delta| delta.sequence > after).take(2).count();
            if batches > 1 && cells.iter().all(|(_, _, cell)| cell.cell_width() == 1) {
                let mut latest = vec![None; buffer.content.len()];
                for (x, y, cell) in cells {
                    latest[buffer.index_of(x, y)] = Some(cell);
                }
                buffer
                    .area
                    .positions()
                    .zip(latest)
                    .filter_map(|(position, cell)| cell.map(|cell| (position.x, position.y, cell)))
                    .collect()
            } else {
                cells
            }
        };
        let mut data = b"\x1b[?2026h\x1b[?25l".to_vec();
        if reset {
            data.extend_from_slice(b"\x1b[0m\x1b[2J\x1b[H");
        }
        {
            let mut backend = CrosstermBackend::new(&mut data);
            backend.draw(cells.into_iter().filter(|(x, y, cell)| {
                *x < cols && *y < rows && usize::from(*x) + cell.symbol().width() <= usize::from(cols)
            }))?;
        }
        data.extend_from_slice(b"\x1b[0m");
        if let Some(cursor) = self.cursor.filter(|cursor| cursor.x < cols && cursor.y < rows) {
            write!(data, "\x1b[{};{}H\x1b[?25h", cursor.y + 1, cursor.x + 1)?;
        }
        data.extend_from_slice(b"\x1b[?2026l");
        let controller = self.controller.as_deref() == Some(origin);
        let mut frame = json!({"sequence":self.sequence,"reset":reset,"data":STANDARD.encode(data),"controller":controller,"long_poll":true,"native_input":true,"cols":buffer.area.width,"rows":buffer.area.height});
        if let Some((_, text)) =
            self.clipboard.as_ref().filter(|(sequence, _)| controller && after > 0 && *sequence > after)
        {
            frame["clipboard"] = json!(text);
        }
        Ok(Reply::ok(frame.to_string()))
    }
}

pub struct Input {
    pub origin: String,
    pub events: Vec<Event>,
    pub reply: Option<oneshot::Sender<Reply>>,
}

#[derive(Clone)]
struct Shared {
    session: String,
    screen: Arc<Mutex<Screen>>,
    output: watch::Sender<u64>,
    input: mpsc::UnboundedSender<Input>,
}

impl Shared {
    async fn call(&self, call: Call) -> Reply {
        let Some(origin) = call.origin.filter(|origin| !origin.is_empty()) else {
            return Reply::error("Mirror requires a paired computer");
        };
        if call.args["session"].as_str() != Some(&self.session) {
            return Reply::error("Mirror request names another session");
        }
        if call.tool == "mirror_frame" {
            let after = call.args["after"].as_u64().unwrap_or(0);
            let mut output = self.output.subscribe();
            let waiting = {
                let screen = self.screen.lock().expect("mirror screen");
                screen.buffer.is_none() || screen.sequence == after
            };
            if waiting {
                let wait = call.args["wait_ms"].as_u64().unwrap_or(0).min(1000);
                let _ = tokio::time::timeout(Duration::from_millis(wait), output.changed()).await;
            }
            let cols = call.args["cols"].as_u64().unwrap_or(500).clamp(1, 500) as u16;
            let rows = call.args["rows"].as_u64().unwrap_or(200).clamp(1, 200) as u16;
            return self
                .screen
                .lock()
                .expect("mirror screen")
                .frame(after, cols, rows, &origin)
                .unwrap_or_else(|error| Reply::error(error.to_string()));
        }
        if call.tool == "mirror_input" {
            if self.screen.lock().expect("mirror screen").controller.as_deref() != Some(&origin) {
                return Reply::error("Take control before sending input");
            }
            let events = match call.args["events"]
                .as_array()
                .filter(|events| events.len() <= EVENT_LIMIT)
                .and_then(|_| serde_json::from_value::<Vec<Event>>(call.args["events"].clone()).ok())
            {
                Some(events) if events.iter().all(valid_event) => events,
                _ => return Reply::error("This session uses native mirror input; update and restart the client"),
            };
            let (send, receive) = oneshot::channel();
            if self.input.send(Input { origin, events, reply: Some(send) }).is_err() {
                return Reply::error("Borg session closed");
            }
            return tokio::time::timeout(REQUEST_WAIT, receive)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_else(|| Reply::error("Borg session did not process the mirror input"));
        }
        let mut screen = self.screen.lock().expect("mirror screen");
        match call.tool.as_str() {
            "mirror_view" => Reply::ok(json!({"native_input":true}).to_string()),
            "mirror_take" => {
                if screen.controller.as_ref().is_some_and(|controller| controller != &origin)
                    && call.args["force"] != true
                {
                    return Reply::error("Another viewer controls this session; request Take control explicitly");
                }
                screen.controller = Some(origin.clone());
                screen.advance(Vec::new());
                self.output.send_replace(screen.sequence);
                let _ = self.input.send(Input { origin, events: Vec::new(), reply: None });
                Reply::ok(json!({"native_input":true}).to_string())
            }
            "mirror_release" => {
                if screen.controller.as_deref() == Some(&origin) {
                    screen.controller = None;
                    screen.advance(Vec::new());
                    self.output.send_replace(screen.sequence);
                    let _ = self.input.send(Input { origin, events: Vec::new(), reply: None });
                }
                Reply::ok("Control released")
            }
            "mirror_resize" => {
                if screen.controller.as_deref() != Some(&origin) {
                    return Reply::error("Take control before resizing");
                }
                Reply::ok("This mirror follows the host terminal size")
            }
            _ => Reply::error("Unknown mirror request"),
        }
    }
}

fn valid_event(event: &Event) -> bool {
    match event {
        Event::Paste(text) => text.len() <= PASTE_LIMIT,
        Event::Key(key) => key.kind != KeyEventKind::Release,
        Event::Mouse(mouse) => mouse.column < 500 && mouse.row < 200,
        _ => false,
    }
}

pub struct Host {
    shared: Shared,
    pub incoming: mpsc::UnboundedReceiver<Input>,
    task: JoinHandle<()>,
    socket: PathBuf,
}

fn socket(runtime: &Path, session: &str) -> Result<PathBuf> {
    let hash = Sha256::digest(session.as_bytes());
    Ok(crate::agent::socket_dir(runtime)?.join(format!("mirror-{}.sock", borg_link::hex(&hash[..12]))))
}

impl Host {
    pub fn start(runtime: &Path, session: &str) -> Result<Self> {
        let socket = socket(runtime, session)?;
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        let (input, incoming) = mpsc::unbounded_channel();
        let (output, _) = watch::channel(0);
        let shared = Shared { session: session.into(), screen: Arc::new(Mutex::new(Screen::default())), output, input };
        let endpoint = shared.clone();
        let task = tokio::spawn(async move {
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {
                    next=listener.accept()=>match next {
                        Ok((stream,_))=>{let shared=endpoint.clone(); clients.spawn(async move {let _=serve(stream,shared).await;});}
                        Err(_)=>break,
                    },
                    Some(_)=clients.join_next(),if !clients.is_empty()=>{}
                }
            }
        });
        Ok(Self { shared, incoming, task, socket })
    }

    pub fn publish(&self, buffer: &Buffer, cursor: Option<Position>) {
        let mut screen = self.shared.screen.lock().expect("mirror screen");
        if screen.publish(buffer, cursor) {
            self.shared.output.send_replace(screen.sequence);
        }
    }

    pub fn controller(&self) -> Option<String> {
        self.shared.screen.lock().expect("mirror screen").controller.clone()
    }

    /// Sends `text` to the controlling viewer's clipboard with the next frame.
    pub fn copy(&self, text: &str) {
        let mut screen = self.shared.screen.lock().expect("mirror screen");
        if screen.copy(text) {
            self.shared.output.send_replace(screen.sequence);
        }
    }

    pub fn plain(&self, text: &str, cols: u16, rows: u16) {
        use ratatui::widgets::{Paragraph, Widget, Wrap};
        let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, cols, rows));
        let message =
            format!("{}\n\n{text}\n\n{}", borg_i18n::tr!(agent_plain_intro), borg_i18n::tr!(agent_plain_return));
        Paragraph::new(message).wrap(Wrap { trim: false }).render(buffer.area, &mut buffer);
        self.publish(&buffer, None);
    }

    pub fn controls(&self, origin: &str) -> bool {
        self.shared.screen.lock().expect("mirror screen").controller.as_deref() == Some(origin)
    }

    pub fn local_input(&self) -> bool {
        let mut screen = self.shared.screen.lock().expect("mirror screen");
        if screen.controller.take().is_none() {
            return false;
        }
        screen.advance(Vec::new());
        self.shared.output.send_replace(screen.sequence);
        true
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.socket);
    }
}

async fn serve(stream: UnixStream, shared: Shared) -> Result<()> {
    if stream.peer_cred()?.uid() != rustix::process::getuid().as_raw() {
        bail!("Mirror socket belongs to another user");
    }
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    loop {
        let mut bytes = Vec::new();
        let count = (&mut read).take(REQUEST_LIMIT as u64 + 1).read_until(b'\n', &mut bytes).await?;
        if count == 0 {
            break;
        }
        if count > REQUEST_LIMIT || bytes.last() != Some(&b'\n') {
            bail!("Mirror request is too large");
        }
        let reply = match serde_json::from_slice::<Call>(&bytes) {
            Ok(call) => shared.call(call).await,
            Err(error) => Reply::error(error.to_string()),
        };
        write.write_all(format!("{}\n", serde_json::to_string(&reply)?).as_bytes()).await?;
    }
    Ok(())
}

pub struct Peer {
    read: BufReader<tokio::net::unix::OwnedReadHalf>,
    write: tokio::net::unix::OwnedWriteHalf,
}
impl Peer {
    pub async fn open(runtime: &Path, session: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket(runtime, session)?)
            .await
            .context("This running session cannot be mirrored; update and restart its Borg process")?;
        let (read, write) = stream.into_split();
        Ok(Self { read: BufReader::new(read), write })
    }
    pub async fn call(&mut self, call: &Call) -> Result<Reply> {
        self.write.write_all(format!("{}\n", serde_json::to_string(call)?).as_bytes()).await?;
        let mut bytes = Vec::new();
        if self.read.read_until(b'\n', &mut bytes).await? == 0 {
            bail!("Mirrored Borg session closed");
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(test)]
mod tests;
