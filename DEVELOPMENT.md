# Developing borg-commander

Everything you need to build, test and change borg. The user-facing README is in
[README.md](README.md).

## Quick start

```sh
git clone https://github.com/0xcd21h/borg-commander
cd borg-commander
cargo run -p borg-app            # starts borg in the current folder
cargo test --workspace           # the whole suite, including end-to-end tests
```

Requirements:

- Rust 1.97 or newer (edition 2024). `rustfmt` and `clippy` for the checks below.
- For the end-to-end tests: `bash`, and optionally `zsh`, `fish` and `less` (tests for shells
  that are not installed are skipped).
- macOS or Linux. borg is Unix-only: it relies on PTYs and `termios`.
- On Linux, the process watcher's eBPF programs build with a pinned nightly compiler and
  `bpf-linker`. `scripts/install-ebpf-tools.sh` installs both, and checks `bpf-linker` against its
  SHA-256 before it runs. Build with `BORG_NO_EBPF=1` to leave the programs out: the watcher then
  hears the kernel's process events instead.

Before pushing, run what CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same tests with [cargo-nextest](https://nexte.st), as [On CI](#on-ci) describes.
When you add, bump or update a dependency, also run the checks in
[Dependencies](#dependencies).

Dev builds carry line tables only: panics and backtraces still name the file and line, and
`target/` grows much more slowly. Build with `CARGO_PROFILE_DEV_DEBUG=full` when you need
variables in lldb. Cargo keeps every old build in `target/`, so run `cargo clean` from time to
time.

The regex crates (`regex-automata`, `regex-syntax` and `fancy-regex`) build optimized in dev
builds too. Syntax coloring compiles its patterns the first time a file needs them, while the
screen waits, and unoptimized code makes that wait long enough to stall end-to-end tests on a
busy machine.

## How the code is organised

```
crates/
  borg-app      the `borg` binary: terminal setup, event loop, effects, config, session
  borg-i18n     shared UI labels, message templates and plural forms
  borg-core     pure state and logic: update(state, msg, now) -> effects
  borg-ui       rendering: panels, dialogs, themes, effects, art (ratatui buffers in, nothing out)
  borg-term     the embedded shell: PTY, terminal emulator, shell integration (OSC 7 / 133)
  borg-vfs      file system access: streaming directory listings
  borg-view     the pager behind F3: constant memory on files of any size
  borg-edit     the editor's text buffer: rope, grapheme cursor, selection, grouped undo
  borg-gfx      terminal graphics: protocol detection, Kitty / Sixel / iTerm2, half blocks
  borg-git      bundled Git plugin: status parsing and bounded background Git commands
  borg-docker   bundled Docker plugin: containers, stats, files and shells through the docker CLI
  borg-drives   the kind of drive a path lies on, its typical and measured speeds, and the
                answers that stop Borg asking before long jobs
  borg-jobs     the copy / move / delete engine with progress, conflicts, pause and cancel
  borg-archive  archives as folders: zip, 7z and tar read and changed in process, RAR through
                7-Zip or bsdtar, and unpacking that cannot write outside its destination
  borg-rules    YARA rules through boreal: Borg's own rules and the user's, compiled file by file,
                and what they match with its evidence, severity and caveat
  borg-link     the end-to-end encrypted channel between two borgs through a relay (Noise IK)
  borg-relay    the relay borgs meet through: it forwards traffic it cannot read
  borg-local-auth  Touch ID or the password from LocalAuthentication before a key leaves the
                macOS keychain
  borg-update   release checks and self-update
  borg-testkit  test harness: fixtures, PTY sessions, screenshots, the demo recorder
```

### The loop

borg follows a strict *state → update → effects* design:

1. `borg-app` turns terminal input, shell output, listing batches and timers into `Msg`
   values.
2. `borg_core::update(&mut AppState, Msg, Instant) -> Vec<Effect>` is pure. It never touches
   the file system, the terminal or the clock (time is passed in), so every behaviour can be
   tested in microseconds.
3. `borg-app` executes the effects: write to the shell, list a folder, open the viewer,
   save the config, quit.
4. `borg_ui::draw(frame, &Scene)` renders the state into a ratatui buffer and returns the
   picture placements for the graphics layer. It is also pure: a fixed state and clock always
   give the same pixels.

Panel, theme, and dialog changes apply immediately. The screensaver, progress indicators,
and About artwork request ambient frames while visible.

### File operations

Copy, move and delete are background jobs. `borg_core` emits `Effect::StartJob` with a
`borg_jobs::Spec`; the app runs `borg_jobs::run` on a blocking thread and feeds its `Event`s
(scanning, progress, questions, finished) back as `Msg::Job`. Questions become a dialog once no
other dialog is open; answers, pause and cancel travel back as `Effect::JobControl` to the job's
`Control`. The engine is synchronous and fully tested on temporary folders
(`crates/borg-jobs/tests/engine.rs`).

Before the shell receives input, `sync_cwd` moves it to the active panel's folder. Shells
whose integration announces `OSC 6633;silent-cd` at each prompt (zsh) get
`Effect::ChangeShellDir`: the app writes the folder to a private per-process request file
(`BORG_CD_FILE`) and sends `SILENT_CD_KEY`, which a ZLE widget binds to `builtin cd`, the
prompt hooks and `zle reset-prompt`. The redrawn prompt can be longer or shorter than the old
one, so the widget draws it at once and sends `OSC 133;B` again from where typing now starts.
Nothing is echoed and no other markers are sent. The zsh integration loads `zsh/zle` itself
before it adds its line-init hook, since `add-zle-hook-widget` does nothing until something has
loaded it, and only a `bindkey` in the system's or the user's startup files would have.
Other shells receive a typed ` cd '…'` that skips history. Once a shell announces its prompts,
the emulator conceals that command and its output (`Effect::WriteConcealed`) until the next prompt
marker, which is drawn where the previous prompt began, so nothing flashes or stays in the console.

Mkdir and Compress are built by `borg_core::command::CommandLine` (with shell quoting)
and run in the embedded shell, hidden at first. If a command takes longer than 600 ms the shell
is revealed, and the panels come back when it finishes.

An archive panel's folder is the archive's path followed by the folder inside it, and
`Panel::archive` holds the `ArchiveMount`. `borg-app` lists such a folder through `borg-archive`
and keeps the indexes of the last 4 archives while their size and time stay the same. Copying
out (F5, Extract here, Extract to other panel) runs as `JobKind::Unpack`, and F3 inside an
archive runs `JobKind::View`, which unpacks into `archive-view` in the runtime folder and drops
the file once the viewer opened it. The bomb and free space checks in
`borg-archive/src/unpack/guard.rs` ask through the job's questions before the first write.

Changes to an archive go out as `Effect::StartRewrite` with a `RewriteRequest`: F5 into an
archive runs as `JobKind::Pack`, F6 as `JobKind::Move`, F7 as `JobKind::ArchiveFolder`, F8 as
`JobKind::Delete` and an editor save as `JobKind::Save`. `borg_archive::Rewrite` locks the
archive, plans the change and writes `.name.borg-partial` beside it, either as a copy with the new
entries appended (a cloning filesystem makes the copy at once) or as a new archive with the kept
entries copied as stored. It reads a zip's or 7z's entries back and then renames the partial over
the original. A move deletes its originals from the `kept` list of the job's `Report`, so a file
left out stays. F4 inside an archive unpacks into `archive-view/edit-…`, and the file stays while
the editor has it.

Before a job writes, `borg_jobs::Pace` adds up its `borg_drives::Work`, the bytes read and written
on each drive plus the time spent compressing, and asks `Ask::Slow` when the estimate passes the
job's patience: 2 minutes for copies and unpacks, 20 seconds for archive changes. `borg-drives`
finds the drive kind with statfs, `diskutil` and `hdiutil` on macOS and `/proc/self/mountinfo`
and `/sys` on Linux. It learns measured speeds from jobs of at least 2 seconds and 64 MiB and keeps
them with the Always answers in `drives.json` in the state folder. Every Borg on the machine writes
that file, so each change reads it again first and applies itself to what is there, and each write
stages under a name of its own (the process ID and a count) with mode 0600 before it renames the
file into place.

### Rendering pipeline

- `panel_view` draws panels: breadcrumbs, rows, icons, the cursor, Git status and spinners.
- `fx/` holds the effects engine: gradients, glow, dimming, shadows, the braille canvas and
  cube, the particle galaxy, the plank-by-plank Theseus logo, the glitch engine behind the logo
  and the clock, the starfield, glyph rain, spinners, and 256-colour quantising.
- Themes are TOML files in `crates/borg-ui/themes/`. `ThemeSet` loads them, resolves
  `extends` and merges styles and effects.

### Plugins

The Git plugin is compiled as `borg-git`; `borg-core::plugins` owns its pure state and
effects, and `borg-app::plugins` executes requests. Each panel has one cancellable status
scan, with newer navigation replacing older requests. Disabling the plugin drops active
status futures and kills their child processes. Index operations already requested finish
before their results are handled. Output is capped at 8 MiB and status scans time out after
15 seconds. Git arguments use literal pathspecs and never pass through the shell.

Plugins have a separate checkbox window and persisted `[plugins]` settings. Git is the
first bundled plugin; there is no external plugin loader yet.

### System monitor

`borg-core::activity` owns process selection, bounded memory history, sorting,
inspection requests, search state, and snapshots. The monitor uses the regular
split layout with a list pane and preview pane. Enter opens a process in an inspection
window over the monitor; `AppState::layout` returns the window's two panes, so keys,
scrolling and mouse hits follow it, while the renderer draws the monitor beneath from
`monitor_panels`. Process selection is retained, and search matches and open files use
the same window. Regions start in collapsible purpose groups with a raw-list toggle;
groups and members are ranked by footprint (written plus swapped pages), and libraries
and mapped files collapse to one row per file. The ranking is computed once per memory
map and the visible rows once per expansion change, so moving through maps with tens
of thousands of regions does no regrouping. Each list keeps
a viewport offset, shared by keyboard navigation and mouse hit testing. Process
inspection has Memory and Open files tabs; file metadata comes from bounded,
NUL-delimited lsof output on macOS and procfs descriptors on Linux. File inspection
is on demand, retains tab positions, and uses the same identity checks and explicit
administrator retry as memory inspection. It never reads the contents of descriptors.
Region and match selection schedule a 120 ms debounced bounded preview read, while
Tab switches focus for scrolling. Late replies cannot replace another selection's
bytes; unreadable regions do not request a capture. `borg-app::activity` samples through sysinfo
and executes on-demand memory-map and memory-read requests on background workers. On Linux the
sample leaves out each process's threads: `/proc/PID/stat` and `/proc/PID/io` already count every
thread, and reading each thread's files every second costs more than the rest of the sample.
macOS maps come from vmmap; Linux maps come from smaps. Process identity combines
PID and start time and is checked before and after inspection. Captures read at
most 64 KiB through the safe read-process-memory API. No target is suspended.
A region capture whose first window is all zeros reads on, at most 16 MiB, to the first page
with data. The UI captures windows by address within the selected region and folds zero rows
(`Activity::snapshot_rows`). Unprivileged captures check their bounds against `/proc/PID/maps`
on Linux; on macOS they rely on the read failing, since vmmap takes seconds on large processes.
The administrator helper still validates against the full map.
The page map (`borg-app::memory_census`) reads one page per entry, at most 16,384 entries
spread evenly over the region, and classifies each from its bytes alone (`PageClass`).
Executable mappings count as code; pointers are 8-byte words inside the readable mappings,
which the UI sends with the request and the administrator helper looks up itself. On Linux
`/proc/PID/pagemap` says which pages are present, and absent ones are not read. Each page read
also carries its Shannon entropy (`borg_view::entropy::Entropy`, hundredths of a bit per byte),
and the renderer draws pages from `Entropy::HIGH` on in the high-entropy style whatever their
class. The F3 viewer uses the same measure: once hex rows show, `Effect::ProfileViewer` reads a
`Profile` of the source on a blocking thread, one 4 KiB block per entry and at most 65,536
entries, sampled evenly beyond that, for the strip beside the rows.

Administrator retry launches the same binary's internal inspection command through
macOS authorization or Linux pkexec. It accepts only numeric process identity and
region bounds, or a validated bounded search request; it cannot run file-manager
commands or export files. The UI stays
unprivileged. Subprocesses have bounded output, deadlines, and kill-on-drop; closed
views and replaced requests reject late replies. Snapshot export uses create-new,
owner-only files and never overwrites a destination. No memory snapshots are saved
automatically. `borg-app::memory_search` scans bounded chunks with byte overlap,
read-error page fallback, cancellation, progress, hit and time limits, and explicit
continuations. Text matches cover UTF-8/16/32; Unicode matches use grapheme-aware
NFKC with a byte-address mapping back to the captured data. Strings and indicator searches
(`borg-app::memory_strings`) list each ASCII or UTF-16LE run once across chunk reads: a run
reaching the end of a read waits for the next one unless it is longer than the overlap, and
then the next read lists only its new part. Indicators are pattern heuristics over the first
KiB of each string. Snapshot searches avoid live process access.
Integrity checks (`borg-app::integrity`) look at the memory map and what the system says of the
process (`integrity::Facts`: program, environment, `/etc/ld.so.preload`, tracer), and reach memory
and files through `integrity::Access`, so tests run them against a model process. Code is
compared with its file at the offset Linux gives, or on macOS from the Mach-O header that starts
a `__TEXT` segment, matched to a slice of a universal file; shared-cache images are skipped by
their header flag. Only pages that can differ are read: on Linux those pagemap marks as private
copies or swapped, on macOS regions with dirty pages. Unbacked memory is searched for image
headers at page starts, executable regions first. The report counts compared, uncompared,
scanned and unreadable bytes and says whether a byte, time or finding limit stopped it; findings
carry a severity, the evidence's address and length, and a caveat.
Rule scans (`borg-app::rule_scan`) compile `borg-rules` once per scan from the built-in rules and
the `rules` folder beside the configuration. A memory scan reads the regions the process owns
through the same `integrity::Access`, in 16 MiB reads that take in the page before them again,
skips regions and pages that were never touched, keeps one finding per rule and region, and stops
after 1 GiB, 45 seconds or 2,000 findings, like the integrity report. The admin helper runs it as
`rules <pid> <start> [folder]`. Findings reuse the integrity view with the kinds `RuleMatch` and
`RuleProblem`. The renderer shares a reserved branding area across UI modes.
The entropy scan of every process (`borg-core::activity::entropy`, `borg-app::activity::entropy`,
`borg-app::entropy_scan`) runs in its own `EntropyScanner` beside the monitor's single request
slot, so opening a hit's memory map does not stop it. `Effect::EntropyScan` names the processes of
the last sample. The worker reads four at a time, leaves out the ones `code_signature::protection`
marks and the ones `Memory::open` refuses before it runs vmmap, and sends `Msg::EntropyScan` parts
every 250 ms, which `EntropyReport::merge` ranks (code, then own memory, then mapped files; share
of high pages; highest page) and cuts to 2,000 hits. Per process, `entropy_scan::scan_process`
skips shared caches (vmmap's names, the `MH_DYLIB_IN_CACHE` header flag, Linux kernel pages),
untouched regions and absent pages, and file mappings another process already had read
unchanged, and samples up to 1,024 pages a region and 32,768 a process. A run stops after 90
seconds and reports the processes it did not reach, for N. The admin helper runs it as
`entropy <json>` with the scope and the process keys, for 60 seconds, and answers with
`ActivityData::Entropy`. Enter on a hit leaves a landing that the regions and census answers
use: the region's group and file open, the page map turns on, and the census cursor goes to the
first high page sampled.

### Network screen

`borg-core::network` owns the network screen: pages, cursors, sort orders, the filter, and
`Traffic`, which turns two samples' byte counts into per-second rates for each socket, listening
port, process and interface, with 60 seconds of history and a mean over the last 3 samples. A
count lower than the one before counts as new in full, since the socket took another's place.
`Network::measured` builds the same rows from two samples for the agent's `read` sources, so the
screen and the agent agree on every number. `borg-app::network` samples on a blocking worker:
sockets come from `netstat -anvbW` on macOS, and from `/proc/net`, `/proc/PID/fd` and `ss -tinHe`
on Linux; interfaces come from sysinfo, with kinds from `networksetup` or `/sys/class/net`.
Reverse lookups run in batches of 32 with a 2 second timeout each, and only for addresses the
screen has not asked about before. Closing the screen cancels the sampler.

Captures: `borg-core::capture` reads pcap and pcapng into packets with pure functions. It decodes
Ethernet, loopback, Linux cooked and raw links, IPv4 and IPv6, TCP, UDP and ICMP, and recognizes
DNS, TLS client and server hellos, HTTP/1 and QUIC. `flows` groups packets into conversations by
transport and their two ends in either order, takes the client as the side that sent the SYN or
spoke first, and puts the client's first bytes back together to read a TLS server name split over
segments. `stream` puts a whole conversation back together for the viewer: each side's TCP
segments in sequence order with repeated bytes dropped, each piece counted as delivered once every
byte before it came, and the two sides interleaved by that moment into turns. `Stream::left_out`
counts what the packets' IP lengths say each side sent beyond the bytes the capture kept. The
viewer keeps the
selected conversation's stream and the capture's first time, so a frame formats only the rows it
shows. `network::capture` holds the dialog and the running `Recording`, which keeps every
socket the screen samples while it runs, and `network::viewer` gives each conversation the
process its packets were tagged with, by majority, or else the process of the socket that held
it: the exact ends first, then a socket listening on its port. The tags come from macOS's
`pktap`, whose pcapng carries Apple's process information blocks, each packet pointing at one
by its index in the section. `borg-app::network::capture` runs `tcpdump -U -n -w -` and writes
its output to a 0600 file in the panel's folder. When the capture keeps the start of each
connection, the output goes through `ConnectionBudget` first: it cuts the stream into whole pcap
records or pcapng blocks and, once a conversation carried `CONNECTION_BUDGET` bytes of payload,
rewrites its later packets' captured length to end where their payload starts, with their wire
length and pcapng options kept. `PcapCounter` counts the whole packets written. Every second the
footer gets them with the bytes written and the room left on the disk from `statvfs`, above
`CaptureRequest::keep_free` (zero unless `keep-free-mib` sets it), and `Recording::limit_ahead`
turns the rate so far into the time to the first limit. A write that finds the disk full cuts the
file back to its last whole write and ends the capture like a limit. The app sends SIGINT at the
deadline, on R or at a limit, which it reports with `Msg::CaptureLimited`, and reads the file back
on a blocking worker.

TLS: `capture::keylog` reads `SSLKEYLOGFILE` text into secrets by client random, and
`capture::tls` opens a conversation's stream with them. It splits the stream back into each
side's bytes, reads their records, and takes the client random and the ServerHello's random,
suite and version from the handshake in the clear. TLS 1.3 records open by trial: a side tries
its key in use, then the keys it moves on to (early, handshake, traffic), then the next key
update, so a lost Finished or KeyUpdate message does not matter. TLS 1.2 keys come from the
master secret through the PRF, and records count from each side's ChangeCipherSpec. The AEADs,
HKDF-Expand-Label and P_hash live in `tls::cipher` on RustCrypto's `aes-gcm`,
`chacha20poly1305`, `hkdf` and `hmac`. Records after a hole in a side's bytes stay sealed,
because nothing tells where the next record starts. `files` reads pcapng decryption secrets
blocks, and `with_secrets` writes one after the section header, turning a pcap into pcapng
first. The vectors in `capture/tls/vectors` are RFC 8448's simple handshake and sessions between
two OpenSSL endpoints in one process. F3 adds the key log named after the capture
(`borg-app::network::capture::key_log_beside`).

Recording keys: the dialog's **TLS keys** sets `CaptureRequest::tls_keys` to a `TlsKeys`:
`Shell`, or `Proxy` with the `Authority` from `[network] proxy-ca`. Once tcpdump runs, the app
creates the empty `.keylog` beside the capture and sends `Msg::CaptureKeys`, with the proxy's
`ProxyEndpoint` in proxy mode. `network::shell_variables` types the assignments
(`export A='…' B='…'`, fish: `set -gx A '…'; …`) at the host shell's prompt through the same
visible path as the tcpdump install command: `SSLKEYLOGFILE`, or `HTTPS_PROXY`, `https_proxy` and
the four CA bundle variables. `ShellVariables` keeps the values Borg inherited and which names it
set, and the end of the capture types the line that restores them, or waits in
`sync_shell_variables` until the shell is free. The app then embeds the key log's lines for the
capture's ClientHellos with `with_secrets` (`embed_keys`), turning a pcap into pcapng, and keeps
the key log. The decryption itself only reads key log text, so any source of standard key log
lines works the same way.

The proxy (`borg-app::network::proxy`) listens on `127.0.0.1:0` for the length of the capture. It
reads a `CONNECT` head with httparse, connects to the target, and peeks at the first byte of the
tunnel: anything other than a TLS handshake is copied as it is. For TLS, `LazyConfigAcceptor`
reads the ClientHello, the upstream connection offers the client's ALPN and is checked with
rustls-platform-verifier (with the folder CA as an extra root), and only then does the proxy
answer the client with a leaf for the SNI that `proxy::authority` signs and caches, speaking the
protocol upstream chose. Both legs share one rustls `KeyLog` that appends to the capture's key
log. The temporary CA's key never leaves memory; the trust bundle (the CA, then
rustls-native-certs' system roots) is written 0600 as `trust-<pid>-<capture>.pem` in the runtime
folder and removed when the proxy drops, and `runtime_leftovers` removes the bundles of ended
processes. Each tunnel reports `Event::Opened`; the app finds the client socket's process with the
sampler and sends `Msg::CaptureProxied`, and `SeenSockets::view` gives the upstream conversation
that process in place of Borg's own socket. Leaves last 30 days and are backdated an hour, inside
the 825 days macOS allows for TLS server certificates. `proxy/tests.rs` runs a service signed by
a generated development CA and checks the echo, ALPN, both sessions' key log lines, a client that
refuses the CA, an upstream that fails verification, a plain tunnel and the 502 answer.

A capture as administrator goes through `network::capture::elevated`. Borg listens on a Unix
socket in a 0700 temporary folder and asks osascript (macOS), or pkexec or else `sudo -n`
(Linux), to run `borg --capture-traffic` as root. The helper checks its interface and filter
against what the screen can make, runs tcpdump only from root-owned system folders, and hands
the socket to tcpdump as its standard output. Borg checks that the peer is root, saves the
stream as itself, and stops the capture by shutting down its side of the socket, which the
helper turns into SIGINT, as it does when Borg exits. tcpdump's messages come back as the
helper's JSON result.

An agent's `capture` call asks with `network::capture_ask`, picks its interface with the dialog's
`capture_interfaces`, and once approved runs the same tcpdump runner through
`record_for_agent`, while a blocking worker samples sockets every second into `SeenSockets`.
`network::viewer::report` turns the `TrafficView` into the text the agent gets: the summary by
process, host and connection without packet contents, or one packet's layers and hex bytes when a
`read` of a capture file names it.

Missing programs: `borg-app::tools::find` looks a program up on `PATH`, then in the system folders
and Homebrew's `sbin` folders, on every call, so a program installed while Borg runs is found.
When tcpdump is not there, the capture ends with `CaptureFailure::Missing` and the installer
`borg-app::tools::installer` detects. `borg-core::install` holds the pure part: `Installer::detect`
picks the package manager from the programs it is told exist (apt, dnf, pacman, zypper and apk,
then Homebrew, which refuses root) with `sudo` or `doas` when root is needed, and `Installer::command`
builds the exact command line. The install dialog types it at the host shell's prompt without
moving the shell to the panel's folder first, so the next command end is the install's, and
`install::finished` turns its exit code into a notice. Another program joins as a `Tool` variant
with its package name for each manager and its own dialog text.

### Startup items screen

`borg-core::startup_items` owns the screen and everything about startup items that needs no
disk: `startup_spots` lists the folders and files each system starts programs from, `parse_startup`
turns a property list (XML or binary, read by its own small reader), crontab, anacrontab, systemd
unit, desktop entry, script, shell file, preload list or `authorized_keys` into entries with what
they run, their program, when and as whom, and `newest_first` and `startup_filtered` order and
narrow the items. Its tests read fixtures in `startup_items/fixtures`. `borg-app::startup_items`
does the reading on a blocking worker: each place as the user Borg runs as, units that `.wants`
and `.requires` folders link to, the user's own crontab through `crontab -l` when the crontab
folder is closed, owners and change times, whether each program exists, and the rules each file
and program match through `rule_scan`, each program once. Programs on macOS's sealed system
volume are not scanned. The agent's `read` source `startup` returns the same report as text.

### Process watcher

`borg-core::watch` owns everything about the watcher that needs no system: `StartedProcess` is what
Borg knows of a process the moment it starts (program and whether its file was deleted, command,
user, terminal and the process that started its session, what standard input, output and error lead
to, preload variables, and the chain of parents), `examine` applies the ten rules to one of those
and returns `Finding`s that say in plain words why they fired, `Baseline` holds which parent starts
which program here and the learning period, `decide` says what to do about a finding with the
guardrails that keep PID 1, Borg, terminals somebody logged in to and the never-frozen programs
running (by name only where they lie in a system folder, and `tmux` or `screen` only for root and
people's accounts, numbered from the `UID_MIN` the app reads in `/etc/login.defs`), and `Watcher`
ties them together with the limits on alerts and freezes per minute. The alert limit never drops a
finding Borg freezes for, notices and the findings Borg would have frozen past the freeze limit each
have a limit of their own, and a start that a rule learning cannot quiet fired on stays out of the
baseline. Freezing waits until every alert from the learning week that would have frozen has been
read, or until `REVIEW_DAYS` after learning ends, whichever comes first: `AlertLog::reviewed` says
when the week has been read, and until then `decide` spares the process with a reason that says so.
`Alerts` is the page: the log, the filter, the cursor and the keys. All of it is data in and data
out, tested in `watch/tests.rs`.

`borg-app::watch` does the watching. Events come from Borg's own eBPF programs where it may load
them ([The eBPF programs](#the-ebpf-programs)), else from the kernel's proc connector over netlink
(`watch::netlink`, which needs root or `CAP_NET_ADMIN`; parsing is a pure function tested on byte
fixtures, so its tests run on macOS, and a bounded queue lets the kernel drop events, which the
reader survives, when the watcher falls behind), from the root watcher once it connects, or from
looking at the user's own process list four times a second (every process's when Borg runs as root,
and in the root watcher on a kernel without the proc connector, which says so with
`FromRoot::Polling`). Looking finds forks along with programs that start, so a copy of its parent
(the same program and command) is no start: it teaches the baseline nothing, and only the rules
learning never quiets judge it, since a forced library or a socket can be the copy's own.
`Processes::started` gives nothing for the kernel's
threads and for a process that ended before Borg could read it, and keeps the link in a system
folder a process was started by, such as `/usr/bin/awk` for `mawk`, resolved inside the process's
own root, so the name rule leaves it alone. `watch::facts::Processes` reads `/proc` and takes its
folder as an argument, so its tests run against a folder of fixtures on any system; a process is
identified by its number together with its start time in clock ticks since boot, read again before
Borg acts. `watch::freeze` sends the stop, continue and kill signals to a process and its
descendants, parents first, checking identity before each one. `watch::log` keeps `alerts.json` (the
newest 200 alerts), `watch-baseline.json` and `watch-allowed.json` in the state folder, owner-only,
named by constants in `borg-core::storage` so the Borg's files window and the cleanup rules know
them. The watcher writes the baseline and never the allowances, while the Alerts page writes the
allowances and never the baseline, so neither can lose what the other wrote; the watcher takes in
new allowances on its own tick, once a minute. Both sides change `alerts.json` by reading it fresh,
changing it and writing it back, so what the page marked read or let go on survives the watcher's
next alert, and the watcher rereads the log once a minute to learn whether the week has been
reviewed. `AlertLog::add` keeps each command at most `COMMAND_KEPT` bytes, its start and its end, and
`watch::log::write_log` drops the oldest alerts until the file fits the bytes Borg reads back, so
nothing a process puts on its command line can make the log unreadable. A writer that finds a log it
cannot read renames it to `alerts-unreadable.json` first (`load_log`), and the new log names that
file while it exists; reading for the page never moves anything. A thread of its own hashes the program and runs the rules over it for each alert, so a
large program does not hold up the processes that start meanwhile; the alerts are written with the
connections first and take in the hash and matches when they come. The watcher runs inside
`borg service run` and nowhere else, so one Borg decides and the others read the log; a Borg with
the page closed follows the log every 5 seconds for the count of unread alerts.

`watch::notify` sends alerts off the machine as they happen, to the places `[watch.notify]` names: a
Telegram bot, an ntfy topic, a webhook and a heartbeat URL. Configuration parsing refuses an address
that is neither `https://` nor `http://` to the machine itself, and an ntfy address without a topic.
The watcher hands each event to a sender on a thread of its own, so a slow place never holds up the
watcher. A static lock keeps a sender started with new settings from writing the outbox while the
old one finishes, and a counter of senders started tells the old one to stop sending after the
message in hand. Each place has its own chain: a message's `seq` is one higher than the last one to
that place and `previous` is the SHA-256 of that message's exact body, which is why the outbox
(`watch-outbox.json`, owner-only, at most 500 messages, each tried for 48 hours) keeps bodies as
sent. A message cuts a command line, a program's path and an alert's reason to 1,000 bytes, keeping
the start and the end. The writer gives up on the oldest messages until the file fits the 16 MiB it
reads back, and an outbox the sender cannot read is renamed to `watch-outbox-unreadable.json`
before it starts a new one, so nothing waiting in it is overwritten. The chain shows loss and changes on the way; it lives on the watched machine, so it cannot
show what someone who controls that machine does before a message leaves. A place's messages go
oldest first and stop at its first refusal, so they arrive in order, and a place taken out of the
configuration leaves nothing waiting. The program's hash and rule matches follow an alert to the
webhook alone, as `evidence`. The heartbeat adds `/fail` when the examiner's channel is closed, a
message has waited 30 minutes, Borg gave up on one since the last beat, or it found an outbox it
could not read, and names each refusing
place with its HTTP status only, since the heartbeat goes to a service of its own. Building
messages, the chain and the waits are pure functions; `watch::notify::deliver` does the HTTPS with
`ureq` and takes each place's token and the key part of its address out of every error. Its tests
post to a server on `127.0.0.1`. `borg service watch notify` sends a test message outside the chain.

The watcher is Linux only in this release. macOS needs `eslogger` and is a separate change.

### The eBPF programs

`crates/borg-watch-ebpf` holds the programs the watcher loads into the Linux kernel, and the layout
of what they send. `crates/borg-app/build.rs` builds `src/kernel.rs` for `bpfel-unknown-none`
through `aya-build`, with the nightly compiler it names, and the `borg` binary embeds the result.
Built for anything else, the crate's binary is empty, so the workspace builds and lints on stable
everywhere. `src/lib.rs` is the layout both sides share: `Exec` and `Parent` are `#[repr(C)]`, and
Borg reads records by `offset_of!` on them, so the reader cannot drift from what the kernel writes.
`kernel.rs` holds the eBPF side's `unsafe` code: every read the kernel allows a program goes through a
raw pointer, and the kernel's verifier checks each one before it loads the program. The crate denies
`unsafe_code`, and only `kernel.rs` allows it.

Three programs run at tracepoints. `sched_process_fork` remembers each new task's parent and start
time in an LRU map and `sched_process_exit` forgets it. `sched_process_exec` sends the program start
with the process, its parent, its user, the name it took, the path it was started by, the file the
kernel ran and the first 4 KiB of its command line. It reads the command line from `mm->arg_start`
to `mm->arg_end` of the current task, where the kernel has just copied it into the new program's
memory, so the pages are there to read. Reading the caller's `argv` at `sys_enter_execve` fails
whenever those pages need a fault first, which `bpf_probe_read_user` cannot take, and a forked child
often starts that way. It names the file by climbing from `mm->exe_file` through each `dentry`'s
parent to the root of the mount namespace, crossing from a mount's root to its mount point, as
`d_path` does; it writes the names from the file up and Borg puts them in order. A file whose inode
has no links (a memfd, or a file deleted before the exec) is marked as one. Records go through one 4
MiB ring buffer, a counter on each processor counts the records that found no room, and the reader
says so in the service's log at most once a minute.

The programs take what they need to know about the running kernel as globals, which `watch::ebpf`
fills in before it loads them, so they hold across kernel versions. The offsets of the tracepoint
fields come from the kernel's own `format` files in tracefs. The offsets of the members the programs
follow (`task_struct.mm`, the argument area and `exe_file` of `mm_struct`, and the members of
`file`, `path`, `dentry`, `qstr`, `inode`, `vfsmount` and `mount` the climb needs) come from
`/sys/kernel/btf/vmlinux`, which `watch::ebpf::btf` reads with its own small parser, following
anonymous structs and unions the way C does. Without BTF those offsets stay zero: the command line
comes from `/proc`, and the program is the path it was started by. The climb keeps its byte count in
the record, where the verifier tracks no value, so every way through a step looks the same to it and
it checks each step once; with the count in a register the verifier gives up after a million
instructions. Where Borg runs in a pid namespace of its own, the programs report only that
namespace's processes, by its numbers.

`watch::ebpf` reads records and merges what the programs read with `/proc`, all as pure functions
tested on any system. `/proc` gives the streams, the terminal, the preload variables and the
parents. The command line from the exec replaces one the process rewrote, unless the programs cut it
short. A process whose `/proc` entry runs another file than the one the kernel ran has started a
later program, so the programs' program and command line win; without that file, another name and
another file than the path leads to say the same. A process that runs a deleted file never counts as
one that moved on, so renaming itself cannot clear **Program deleted while it runs**. A `/proc`
entry that started after the exec belongs to a later process with the same number, and a process
gone from `/proc` is reported from the programs alone, its start counted in clock ticks from the
fork. `watch::ebpf::load` (Linux, `cfg(borg_ebpf)`) loads and attaches the programs with `aya` and
reads the ring buffer on a thread of its own, letting each record go before it waits for the
watcher. The watcher tries eBPF first, then the proc connector, then looking at the process list,
and `WatchSource::Ebpf` and `FromRoot::Ebpf` say when it is eBPF.

The programs need Linux 5.8 for the ring buffer, `CAP_BPF` and `CAP_PERFMON`. The kernel lends its
tracing helpers only to programs with a GPL-compatible license string, so the object declares
`Dual MIT/GPL`. CI loads them on the runner's kernel with `sudo` in
`the_programs_load_and_report_a_command_that_ends_at_once`. To try them by hand, run that test
binary as root, or in a container with `--cap-add BPF --cap-add PERFMON` and `/sys/kernel/tracing`
mounted. Run it in the container's own pid namespace, since the programs see every process of the
machine they run on.

### The part that runs as root

The watcher is the first part of Borg that may run as root for a long time, so it is kept as small
as it can be. `borg service watch install` installs `borg-commander-watch.service`, which runs
`borg --watch-root SOCKET UID` (`borg-app::watch::root`). That process loads the eBPF programs, or
hears the kernel's process events where it cannot, reads `/proc`, and stops, continues or ends a
process when Borg asks it to. It does nothing else: no file manager commands, no reading of file
contents, no network, and the unit sets `NoNewPrivileges` and bounds its capabilities to
`CAP_NET_ADMIN`, `CAP_KILL`, `CAP_SYS_PTRACE`, `CAP_DAC_READ_SEARCH`, `CAP_BPF` and `CAP_PERFMON`.
`LimitMEMLOCK=64M` covers kernels before 5.11, which count the programs' memory as locked memory.

The two sides talk in lines of JSON over a Unix socket in the user's state folder, mode 0600.
Borg accepts a connection only from a peer whose user is root, and the root part connects only to
a socket owned by the user it was installed for. Borg's own interface stays unprivileged and
writes the log itself, so nothing root-owned is left in the user's folders. Anything the root part
sends is data: it reports processes and the outcomes of what it was asked, and Borg never runs a
command on its behalf.

When adding to the root part, keep that shape. It should read, report and signal, and the list of
things it may be asked to do stays in one enum (`watch::root::Act`).

### Editor

F4 opens `borg-core::editor`, a full-screen editor over `borg-edit::Buffer`: a rope with a
grapheme-aware cursor, selection, indentation and line-ending detection, and undo grouped by
typed words. Core handles keys and prompts and asks the app to read (`Effect::OpenEditor`) and
write (`Effect::SaveFile`) the file. `borg-app::edit_file` refuses text that is not UTF-8 or larger
than 32 MiB, compares the file with the stamp read before writing, and writes through a temporary
file that keeps the mode and owner; a hard-linked file is written in place. Core colours the
visible lines, with context above them, through `borg_view::highlight_lines` and caches them by
buffer revision, so drawing stays free of parsing.

### Collections and live search

Directory entries retain their source paths. Virtual panels reuse cursor, marks, sorting,
viewer, and jobs while resolving selections by source identity. Named collections store
lossless path bytes in `collections.json` under the state directory. Removing a reference
never starts a delete job. The live recursive search worker streams metadata in bounded
batches, coalesces obsolete requests, and stops at the documented match limit. Query
generations reject late messages; the update loop and renderer perform no search I/O.

`borg-core::query` parses the search language and matches entries; `name_search` resolves a
request's `hidden:` and `excluded:` switches against the window's settings in core, so the
worker only applies them. The worker walks with the `ignore` crate: ignore files, configured
globs as overrides, and a `filter_entry` that drops hidden names and generated folders
(`borg-core::exclude`, plus `CACHEDIR.TAG`). A dropped folder is still tested against the query.
A contents search compiles the find bar's `borg_view::Query` once, and the worker reads each file
whose name matches through `borg-core::contents`, which leaves binary and oversized files unread
and cuts the first matching line to a preview. Those files travel in `Msg::Found`, each with its
line, and `Results::lines` keeps them by path for the panel and for F3 to land on the match.
A rules search (`SearchKind::Rules`) reads each matching file through `rule_scan::scan_file` with
the same size limit, and on Linux with `O_NOATIME`. Its files travel in `Msg::RuleHits` with the
rules they matched, and `Results::rule_hits` keeps them by path; F3 lands on the worst rule's
evidence, as text up to its line break or as a byte regex. A packed-executables search
(`SearchKind::Packed`) asks `borg_view::executable::code_ranges` for the parts of an ELF, Mach-O
(thin or universal) or PE file that the loader maps executable, and `entropy::code_tally` measures
up to 1,024 blocks spread over them. Files whose tally `looks_packed` (at least `PACKED_SHARE` of
the blocks high) travel in `Msg::PackedHits`, `Results::packed` keeps the tallies, and F3 opens hex
rows at the first high block through `viewer::Landing::Bytes`. Searches that read files check each
entry's real kind first, so a named pipe is never opened.
Name-only queries match on what the listing gives and read metadata for matches alone. The
search history is core state saved owner-only as `search-history.json` in the state directory.

`borg-app::index` keeps a name index per folder chosen in the Search menu, in `index/` under the
cache directory. A build walks with the same `Walker` as live search, hidden names included and
flagged, and numbers folders in walking order, so every subfolder's contents are one range of
folder numbers. Rows hold the folder, name bytes, kind, size and modification time. The binary
file is written owner-only and atomically, and decoded with bounds checks. `Indexes::covering`
answers a request when its folder lies in an indexed root built with the same exclusions;
otherwise, or when the folder is not in the index, the live walk answers. Stale and missing
indexes are queued for one background build at a time while the previous one keeps answering.
Matches are read again from disk before they are shown.

Folder totals and expandable size trees share
one postorder metadata scanner. `borg-app::size_cache` writes all completed folder
summaries into a staged SQLite index with bounded page memory, then publishes it
atomically. Root and immediate children seed a tree; generation-tagged requests
load expanded branches from the index. Display limits do not truncate the index.
Direct-file totals explain bytes outside child directories. `borg-core::measure`
tracks one independent background size task with generation-tagged updates. A separate
worker in `borg-app::search` reuses the same scanner, streams completed folder totals
into matching normal-panel paths, and publishes the same complete cache. Navigation
and result searches leave it running; starting a file job cancels it before cache
invalidation. Completion refreshes normal listings without replacing result panes. Core tree state owns
branches and selection. Measurements are
explicitly dated observations, never validated as fresh by directory mtimes. Job start
and completion epochs prevent scans spanning file operations from publishing cached
totals; affected roots are invalidated before completion is delivered. Normal listings
reuse saved immediate-child totals without starting recursive walks.

### Remote connections

`borg-link` carries every remote request over a Noise channel through the relay. Borg and the
relay set `TCP_NODELAY` on their sockets before TLS or the WebSocket handshake. A TLS proxy in
front of the relay owns its own sockets, which Borg cannot configure.

Remote folders stream through a directory cursor held on the host, tied to the requesting
computer and path. The host enumerates forward once, reads each entry's metadata once, and
pushes bounded chunks over one request: at most 128 entries first, then up to 512, each
chunk also yielding after 25 ms of scanning. Disk work runs on blocking workers outside the
UI loop and the cursor map's lock. A process keeps at most 64 idle cursors and drops those
idle for 30 seconds; navigation drops the client connection, and network backpressure bounds
queued output. Hosts answer clients that request pages by offset by skipping raw directory
records before reading metadata.

Mirror display and input use separate tasks and connections. Input bursts are batched in a
bounded queue; frame requests wait on the host for new PTY output, outside the service loop
and child lock. A viewer more than 64 KiB behind receives a snapshot of the visible grid,
cursor, and attributes instead of the backlog. The controlling viewer sends `mirror_resize` with
its window less the two banner rows. The service resizes the child's PTY to it. A regular session
keeps its own terminal and, where the viewer's window is smaller, draws the whole screen into the
top left of that terminal at the viewer's size and publishes that area as the mirror's frame; it
fills its terminal again once control returns to it or the viewer lets go. A copy made while a
viewer controls the session travels with that viewer's next frame as a base64 `clipboard` field,
which the viewer passes to its terminal as OSC 52 after checking it is plain base64; view-only
viewers never receive it.
Copies also stay in `borg-core::clipboard`, so Ctrl-V works in Borg's own fields anywhere.

Every channel opens with a hello: `pair`, `session` (an agent, a mirror or a panel) or `viewer`.
A borg opens a viewer channel for each host session one of its panels mounts (`host_asks`) and
long-polls `asks_wait` with the version it has. The host answers with an `AskFeed` as soon as its
`AskBoard` changes, or after 15 seconds: the asks that wait, the last 32 that ended and how, the
computers Approve all is on for, and the controller when another computer holds the native
mirror. Answers (`asks_answer`) and `asks_stop` go on a viewer channel opened for each, on a task
of their own, so they never wait behind a poll and a dropped poll or a closed panel does not lose
them. The viewer remembers the asks it answered or closed by the host's run and number, so a
host that restarts shows its new asks even when they reuse a number. The host takes the three `asks_*` calls only on a viewer channel and
refuses every other call there. `Caller` marks them with `viewer`, which `Call::external` clears,
so an agent's socket call or session channel cannot claim it. An answer names the host's run, a
random ID for each process, and the ask's number, and counts only while that ask still waits and
no other computer controls the session; `borg-core::agent::forward` holds that check and the
first answer wins logic. `borg mcp` forwards only the tools it lists, so an agent cannot reach
mirror, console, panel or viewer calls. In the background service the asks come from the child's
socket, which trusts the service; the service tracks the mirror controller there, so the view-only
check does not apply. Outside the service, a named session other than the one that holds borg
cloud clears the viewer mark on its socket, so its asks stay on its own screen. These checks keep
Borg's own connections apart. They cannot stop an agent that runs programs under your account on
the viewer's computer: it can read the pairing key in Borg's state folder or send keys to the
terminal Borg runs in, so give such an agent a sandbox when its requests matter.

A borg keeps two kinds of keys in the `link` folder of its state folder. The server key, `key`,
identifies it to the computers paired with it and loads by itself. A borg that reaches servers
proves itself with a computer key of its own, which `link::keys` loads once per process, together
with the keys it retired while some server may still take them. `computer.json` holds the current
key's public half and when it was made, and `computer.lock` serializes making keys and changing
`servers.json`. Each `servers.json` entry records the computer key that server takes (`computer`)
and a key sent in a `rotate` whose answer did not arrive (`offered`). `Remote::open_as` tries the
offered key, then the one the server takes, then the current one. On a session channel to a server
whose answer to the hello sets `rotation`, it sends `rotate` with the new public key and a proof:
an HMAC over the channel's handshake hash and the new key, keyed through HKDF from
DH(new private, server public) with the label `borg-link rotate 1`. The server checks the proof,
swaps the key in that computer's entry, keeps its name, saves `computers.json`, and shows a notice
with both fingerprints; `Caller` ends sessions on the old key at their next message. A computer
that every key fails on gets the `cloud_key_refused` message.

Where the computer keys live depends on the state folder. When it is the usual one
(`StateDir::default_root`), a Mac keeps them in a login keychain item, service `Borg Commander` and
account `computer key` (`link/keychain.rs`), whose access list trusts only the borg binary that
made it. Before the first read in a process, `borg-local-auth` asks LocalAuthentication for Touch ID
or the password, and a refusal or a session without the Mac's screen ends the load with a message.
A `computer-key` file found there moves into the keychain, merged around the key `computer.json`
names, and is deleted. Everywhere else, including every test, the keys are a 0600 `computer-key`
file. `computer_keys` loads them on a blocking thread, one load at a time, so parallel connections
ask once.

`scripts/remote-latency.py` measures a paired host from a source checkout without taking
control or sending keys:

```sh
python3 scripts/remote-latency.py HOST --session SESSION --borg target/debug/borg
```

It prints connection time, status round trips, and full-screen and subsequent frame timings
as JSON lines, with byte counts but no screen contents or identity keys. `--sizes 1x1,20x5,150x60`
compares response sizes; a failed frame is followed by a fresh status connection to tell a
stalled display from an unreachable host.

### Borg's own files

`borg-core::storage::leftovers` decides what is left over from a listing of Borg's folders, given
the time, this version, whether the runtime folder was idle at start, which processes run and
which sockets answer. The rules match file names Borg writes, and a name they do not know is
never left over. `borg-app::storage` lists the folders, answers those questions, measures each
place for the Borg's files window and `borg storage`, and cleans up 30 seconds after start and
then at the interval of the runtime TTL refresh. Saved captures are remembered in `captures.json` in
the state directory, owner-only, newest first.

When you add a file or folder that Borg writes, name it with a constant the rules can share, add
the rule and a test in `borg-core::storage`, and check the place it is measured under. A file
the rules do not know is never cleaned up and never shown as left over.

### Interface text

English interface text lives in `crates/borg-i18n/src/en.rs`. Use stable message keys
through `borg_i18n::tr!` for labels, hints, notices, and dialog titles. Keep complete
sentences and format templates in the catalog so translations can change word order:

```rust
use borg_i18n::tr;

let label = tr!(search_in);
let title = tr!(collection_title, name = collection.name);
```

Pass format arguments explicitly: the template expands from the catalog's scope.
Use `borg_i18n::count` with its `Count` enum for singular and plural forms; renderers
must not append an English `s`. Keep field borders and geometry in rendering code, and measure translated labels in terminal cells. Button drawing and mouse hit
areas must use the same label and geometry.

The English catalog is the source of truth; Chinese (Simplified), Portuguese (Portugal),
Spanish, and Russian catalogs live in `crates/borg-i18n/locales/`. A build-time generator
requires every key and validates format placeholders and padding before compiling the
localized format strings. Use `tr!(key, argument = value)` for messages with arguments.
`tr!(@ key)` provides a stable message identity for constant binding tables; call `.text()`
when rendering it. The language picker is in Options. Configuration and `BORG_LANGUAGE`
can select it at startup, with the system locale used when neither is set.

Core updates, rendering, and hit testing scope language to the state preferences;
background workers use the process language. Tests can use `borg_i18n::with_locale`
without changing another thread's language. Russian counts handle singular, few, and
many forms. New interface text needs entries in all four translated catalogs. File names, shell commands, protocol values, and output
from external programs are not message keys. Keep longer explanations in the guide;
dialogs should contain controls, short labels, and keyboard hints.

### Graphics

`borg-gfx` probes the terminal once at start-up (Kitty query, cell size, DA1) and picks a
protocol. `Graphics::frame` diffs the placements of each frame: Kitty images are sent once and
placed by id; Sixel and iTerm2 images are re-sent when something covers them and trigger a
repaint when removed. Clearing the screen deletes kitty's images, so everything is sent again
after a clear: `App::clear_screen` resets the graphics layer, and a frame of a new size counts as
cleared because ratatui clears the screen on every resize. Without a protocol the UI draws pictures with half blocks, which is also
what tests see.

## Tests

The suite has four layers, from fastest to slowest:

| Layer | Where | What it covers |
| --- | --- | --- |
| Unit | `#[cfg(test)]` modules, `borg-core/src/update_tests.rs` | logic, key handling, motion, find, themes, encoders |
| Property | `*/tests/properties.rs`, `panel_properties.rs` | OSC parsing, pager symmetry, panel invariants |
| Render snapshots | `borg-testkit/tests/ui_snapshots.rs` | every theme and screen, rendered at a fixed clock |
| End to end | `borg-app/tests/e2e.rs` | the real binary in a PTY with a real shell |

### Snapshots

Render snapshots are [insta](https://insta.rs) text snapshots: the characters on screen
followed by the style runs of every row. When a change is intended:

```sh
INSTA_UPDATE=always cargo test -p borg-testkit --test ui_snapshots
git diff crates/borg-testkit/tests/snapshots     # review before committing
```

(`cargo insta review` works too if you have `cargo-insta` installed.)

### End-to-end tests

`borg_testkit::Session` starts the real `borg` binary in a PTY inside a throwaway fixture
(home, config, state and work folders) and runs its output through the same terminal emulator
borg uses. Tests send keys and wait for screen conditions:

```rust
let mut app = start(&fixture, "find")?;
app.send(keys::ctrl('s'))?;
app.send("note")?;
app.wait_until("cursor on notes", |s| highlighted(s, "notes.txt"))?;
```

Rules of thumb:

- Never sleep. Wait for what you expect to see (`wait_text`, `wait_until`, `wait_gone`).
- `settle()` waits for the screen to stop changing. Avoid it while something animates
  forever (the About screen, a screensaver); wait for a condition instead.
- Fixtures disable update checks. Use `fixture.configure("...")` to add settings.
- `BORG_TEST_TIMEOUT_SECS` raises the wait timeout on slow machines (CI uses 30).

### On CI

CI runs `cargo nextest run --workspace --profile ci`. nextest runs every test in a process of
its own, and the `ci` profile in `.config/nextest.toml` retries a failed test up to 2 times. A
test that fails all 3 attempts fails the job and shows as an error on the run. A test that
passes on a retry keeps the job green and shows as a warning, with the output of the attempt
that failed. A warning still means the test depends on timing somewhere, so fix it when you see
one. On Linux, CI installs the eBPF tools with `scripts/install-ebpf-tools.sh` and then runs the
test that loads the watcher's programs into the runner's kernel with `sudo`. To run the suite the way CI does, install nextest with `cargo install cargo-nextest
--locked` and run the same command.

### Screenshots

Any snapshot or end-to-end checkpoint can be written as a PNG:

```sh
BORG_SCREENSHOTS=target/screenshots cargo test --workspace
```

The renderer draws box lines, blocks and braille itself and composites pictures, so the PNGs
look like a real terminal. It uses the bundled DejaVu Sans Mono; point it at any other
monospace font with:

```sh
BORG_SCREENSHOT_FONT=/path/to/Font-Regular.ttf \
BORG_SCREENSHOT_FONT_BOLD=/path/to/Font-Bold.ttf \
BORG_SCREENSHOTS=target/screenshots cargo test -p borg-testkit
```

Icons fall back to the bundled Symbols Nerd Font. CI uploads the screenshots of every run as
an artifact.

### README screenshots

The theme gallery and the editor, folder picker, size, activity, network and Docker showcase are
rendered from fictional files, folders, processes, sockets and containers, so they never show the
machine they were made on:

```sh
export BORG_SCREENSHOT_FONT=fonts/BorgMono-Regular.ttf BORG_SCREENSHOT_FONT_BOLD=fonts/BorgMono-Bold.ttf
cargo run -p borg-testkit --example themes      # writes docs/themes/*.png
cargo run -p borg-testkit --example showcase    # writes docs/showcase/*.png
```

### The demo

`docs/demo.gif` is recorded from a real session, not drawn by hand:

```sh
scripts/demo.sh                    # writes docs/demo.gif
scripts/demo.sh /tmp/demo.gif      # or anywhere else
```

The script lives in `crates/borg-testkit/examples/demo.rs`. It records a 132 × 36 terminal
with fictional project and release folders under `/tmp`. Every recorded frame is checked for
host home-directory paths, and a PNG of the final frame is saved beside the GIF. It runs
the release binary in a PTY, plays a list of key presses and records the screen with
`borg_testkit::Recorder`. Frames that did not change are dropped, and each GIF frame stores
only the pixels that changed, which keeps the file small. The README demo uses the bundled Borg Mono font.

## Versions and releases

borg uses semantic versioning. The version lives once, in `[workspace.package]` of the root
`Cargo.toml`, and every crate inherits it. To release:

1. Bump `version` in `Cargo.toml`, run `cargo check` so `Cargo.lock` follows, and commit.
2. Tag the commit with the same version and push the tag:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

`.github/workflows/release.yml` then:

- checks that the tag matches the crate version (`scripts/check-version.sh`),
- builds static Linux binaries (musl, x86_64 and arm64, with `cargo zigbuild`) and macOS
  binaries (Intel and Apple Silicon),
- signs the macOS binaries ad hoc with the hardened runtime and
  `packaging/macos/borg.entitlements`, then checks that they load only system libraries
  (`scripts/sign-macos.sh`); the debugger entitlement keeps memory reading as administrator
  working, and Borg running as you never asks macOS for another process's task port,
- packages each as `borg-<version>-<target>.tar.gz` with a `.sha256` file
  (`scripts/package.sh`),
- creates the GitHub release with generated notes and attaches everything,
- renders the Homebrew formula from `packaging/homebrew/borg-commander.rb.in`
  (`scripts/homebrew-formula.sh`) and, when the `HOMEBREW_TAP_TOKEN` secret is set, pushes it
  to the tap repository (`vars.HOMEBREW_TAP`, default `0xcd21h/homebrew-tap`).

One-time setup for Homebrew: create the public repository `homebrew-tap` and add a
fine-grained token with *Contents: read and write* on it as the `HOMEBREW_TAP_TOKEN` secret of
this repository. Users then install with `brew install 0xcd21h/tap/borg-commander`.

Without the tap token, GitHub Releases still publishes the binaries and the generated formula
is saved as a workflow artifact. After adding the token, run the **Publish Homebrew formula**
action with the released version (for example `0.2.0`) to publish it without making another tag.

`borg update` and the daily check live in `crates/borg-update`: they read the latest release
from the GitHub API, pick the archive for the target borg was built for (`BORG_TARGET`, set by
`crates/borg-app/build.rs`), verify the checksum and swap the binary in place. GNU Linux builds
can also update to the published static MUSL build for the same architecture when no exact
target archive is available.

Archive downloads have an explicit 128 MiB limit; release metadata and checksum downloads
have smaller limits. Borg 0.8.2 and earlier read downloads with the HTTP library's 10 MiB
default, so they cannot download a larger archive and update by installing again.

## Conventions

- No comments in code: names, types and small functions carry the meaning.
- No magic values: name constants, keep them next to their use.
- Prefer pure functions and data in, data out. Side effects belong in `borg-app`.
- Keep `unsafe` out (`unsafe_code = "forbid"` in the workspace lints). The eBPF programs
  ([The eBPF programs](#the-ebpf-programs)) and `borg-local-auth`, whose Objective-C calls into
  LocalAuthentication are all `unsafe`, are the two exceptions.
- New behaviour comes with a test at the lowest layer that can catch it; anything a user sees
  also gets a render snapshot.

## Dependencies

Every crate and npm package ends up on users' machines or in the release build, so each one
has to earn its place. These rules apply to new dependencies, version bumps and every change to
`Cargo.lock` or `website/package-lock.json`, including the transitive packages a change brings
along. Security and maintenance come first. Popularity counts after them.

### Seven days old

Use only versions published at least 7 days ago. Hijacked accounts and broken releases are
usually caught and yanked within days, and the wait keeps Borg out of that window.

- Cargo has no stable setting for this yet. When `cargo update` picks a version that is too
  young, pin an older one with `cargo update -p <crate> --precise <version>`.
- `website/.npmrc` sets `min-release-age=7`, so npm 11.10 and newer only install versions that
  are at least 7 days old.
- A fix for an advisory that affects Borg may come in sooner. Add it to `exceptions` in
  `.config/dependency-age.toml` with the advisory as the reason, for example
  `{ crate = "rustls-webpki@0.103.16", reason = "RUSTSEC-2026-0382" }` or
  `{ npm = "astro@7.4.5", reason = "GHSA-xxxx-xxxx-xxxx" }`, and remove the entry once the
  version is 7 days old.

`scripts/check-dependency-age.py` reads publish times from the crates.io index and the npm
registry. It also fails on yanked crates:

```sh
python3 scripts/check-dependency-age.py --base origin/main   # what your branch adds or changes
python3 scripts/check-dependency-age.py                      # every locked package
```

### Security checks

Before adding or bumping a dependency, look up the exact version in:

- the [RustSec advisory database](https://rustsec.org/advisories/), through
  `cargo deny check advisories`,
- [OSV.dev](https://osv.dev), which collects RustSec, GitHub and other advisories for crates and
  npm packages,
- [GitHub security advisories](https://github.com/advisories),
- [deps.dev](https://deps.dev), for advisories, the OpenSSF Scorecard and dependents,
- its page on [crates.io](https://crates.io) or [npmjs.com](https://www.npmjs.com): who owns it,
  whether the repository link leads to the real source, and whether recent versions were yanked
  or deprecated.

`npm audit --prefix website` covers the website. A crate that bundles a C library, such as
SQLite in `libsqlite3-sys`, also needs the bundled library's version checked against that
project's own advisories, because RustSec rarely tracks them.

For crates that parse untrusted input, handle cryptography, talk to the network or call C, read
the parts Borg calls. Look at the `unsafe` blocks and at what `build.rs` does, since it runs on
every developer's machine and in CI.

### Maintenance and popularity

A dependency must pass all of these:

- No unfixed advisory for the locked version, including RustSec notices that a crate is
  unmaintained or unsound.
- A public repository that matches the published package and is not archived.
- Signs of life in the last 12 months: a release, or commits and answered issues. A small
  finished crate qualifies when its issues still get answers.

Popularity tells you how many other people would notice a problem first. A crate is too obscure
when it misses any of these:

- 100,000 downloads in total and 10,000 in the last 90 days.
- 10 reverse dependencies on crates.io.
- 6 months since its first release.
- Two or more owners, or 1,000,000 downloads in total.

An npm package is too obscure when it misses any of these:

- 10,000 downloads a week.
- 6 months since its first release.
- Two or more maintainers, or 100,000 downloads a week.

A package that is too obscure comes in only when nothing popular does the job and the security
and maintenance checks pass. Say why in the pull request, read the code Borg calls, and consider
whether a small function in Borg would do the job.

### Checks on CI

`.github/workflows/dependencies.yml` runs on every pull request, on pushes to main and every
Monday:

- the age check for what the change adds to either lock file,
- `cargo deny --locked check advisories bans sources` with `deny.toml`. It fails on RustSec
  vulnerabilities, unmaintained and unsound notices, yanked crates, crates from anywhere but
  crates.io, unused `[workspace.dependencies]` entries and the banned UnRAR crates,
- `npm audit` for the website.

When a new advisory fails the check, update the crate. If no fixed version is 7 days old yet,
or the advisory cannot reach the code Borg runs, add it to `ignore` in `deny.toml` with the
reason, and remove the entry when the fix is locked.

The tools follow the same rules: CI pins cargo-deny to a release that is at least 7 days old.
To run it locally, install the version `dependencies.yml` pins:

```sh
cargo install cargo-deny --locked --version <version>
cargo deny --locked check advisories bans sources
```

## Documentation website

The [public website](https://0xcd21h.github.io/borg-commander/) is built with Astro and
Starlight in `website/`. Use Node 24 LTS:

```sh
npm ci --prefix website
npm run dev --prefix website
npm run build --prefix website
python3 scripts/check-public-content.py --site website/dist
```

Edit `docs/GUIDE.md` for guide content. The generator splits its selected sections into
searchable pages on every build; do not edit the generated `website/src/content/docs/guide/`
files. The landing page lives in `website/src/content/docs/index.mdx`. Only the guide, the
icon and an explicit list of fictional showcase screenshots are copied into the site. Build
metadata comes from `Cargo.toml` and the Git revision. The guide tracks main rather than a
release.

The icon is Borg's corner badge caught mid-glitch. `scripts/render-brand.mjs` draws the site
icon, the favicon, the animated README icon and the GitHub social preview into `docs/brand/`.
Run it again after changing the badge or the glitch, then upload the new social preview in the
repository settings. It needs Node, `resvg` and ImageMagick:

```sh
node scripts/render-brand.mjs
```

The **Publish guide** workflow checks the source, builds the site and search index, checks
local links, and deploys GitHub Pages when relevant files change on main. Pull requests build
without deploying. GitHub Pages must use **GitHub Actions** as its build source.

Public files describe the current behaviour only. Keep plans, measurements, change logs and
machine-specific diagnostics in the ignored `.local-notes/` directory. Environment
files, dependency folders, build output, and generated pages are ignored. Before committing,
run `python3 scripts/check-public-content.py`; it checks public text for personal home paths,
private temporary paths, private keys, and common access-token formats without printing
matched values. It does not inspect binary contents, files above 4 MiB, or Git history and
cannot replace reviewing a diff. Keep copyright and license attributions intact.

### Website translations

Website messages and page metadata live in `website/locales/`. Full guide translations
live in `docs/translations/<locale>/GUIDE.md`; translated routes use `/zh-cn/`, `/pt/`,
`/es/`, and `/ru/`. Starlight supplies its translated search, navigation, and language picker.
Keep code blocks, inline commands, placeholders, and link destinations intact. The build
checks their structure and generates all 50 guide pages from the five guide sources.

The initial translations use machine translation with reviewed interface terminology;
corrections are welcome. `docs/translations/manifest.json` records the source hash used.
When the English guide changes, translated pages show a notice until translations have
been refreshed and the source hash updated. Update the manifest only after reviewing the
translations; do not hide the notice by changing a hash alone. Website builds never call a
translation service, and Borg never needs a network connection to load its language catalogs.
