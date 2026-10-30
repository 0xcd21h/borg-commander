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

Before pushing, run what CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

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
  borg-jobs     the copy / move / delete engine with progress, conflicts, pause and cancel
  borg-link     the end-to-end encrypted channel between two borgs through a relay (Noise IK)
  borg-relay    the relay borgs meet through: it forwards traffic it cannot read
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
prompt hooks and `zle reset-prompt`. Nothing is echoed and no command markers are emitted.
Other shells receive a typed ` cd '…'` that skips history. Once a shell announces its prompts,
the emulator conceals that command and its output (`Effect::WriteConcealed`) until the next prompt
marker, which is drawn where the previous prompt began, so nothing flashes or stays in the console.

Mkdir and archive actions are built by `borg_core::command::CommandLine` (with shell quoting)
and run in the embedded shell, hidden at first. If a command takes longer than 600 ms the shell
is revealed, and the panels come back when it finishes.

### Rendering pipeline

- `panel_view` draws panels: breadcrumbs, rows, icons, the cursor, Git status and spinners.
- `fx/` holds the effects engine: gradients, glow, dimming, shadows, the braille canvas and
  cube, the starfield, glyph rain, spinners, and 256-colour quantising.
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
and executes on-demand memory-map and memory-read requests on background workers.
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
`/proc/PID/pagemap` says which pages are present, and absent ones are not read.

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
NFKC with a byte-address mapping back to the captured data. Snapshot searches avoid
live process access. The renderer shares a reserved branding area across UI modes.

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
cursor, and attributes instead of the backlog. A copy made while a viewer controls the session
travels with that viewer's next frame as a base64 `clipboard` field, which the viewer passes to
its terminal as OSC 52 after checking it is plain base64; view-only viewers never receive it.
Copies also stay in `borg-core::clipboard`, so Ctrl-V works in Borg's own fields anywhere.

`scripts/remote-latency.py` measures a paired host from a source checkout without taking
control or sending keys:

```sh
python3 scripts/remote-latency.py HOST --session SESSION --borg target/debug/borg
```

It prints connection time, status round trips, and full-screen and subsequent frame timings
as JSON lines, with byte counts but no screen contents or identity keys. `--sizes 1x1,20x5,150x60`
compares response sizes; a failed frame is followed by a fresh status connection to tell a
stalled display from an unreachable host.

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
repaint when removed. Without a protocol the UI draws pictures with half blocks, which is also
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

The theme gallery and the size, activity and Docker showcase are rendered from fictional
folders, processes and containers, so they never show the machine they were made on:

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
have smaller limits. Packaging keeps Borg archives within the 10 MiB limit used by older
updaters, using stronger gzip compression when necessary, and rejects oversized archives
before publication. To package locally, install `zopfli` in a Python environment and set
`BORG_PACKAGE_PYTHON` to its Python executable.

## Conventions

- No comments in code: names, types and small functions carry the meaning.
- No magic values: name constants, keep them next to their use.
- Prefer pure functions and data in, data out. Side effects belong in `borg-app`.
- Keep `unsafe` out (`unsafe_code = "forbid"` in the workspace lints).
- New behaviour comes with a test at the lowest layer that can catch it; anything a user sees
  also gets a render snapshot.

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
files. The landing page lives in `website/src/content/docs/index.mdx`. Only the guide and
an explicit list of fictional showcase screenshots are copied into the site. Build metadata
comes from `Cargo.toml` and the Git revision. The guide tracks main rather than a release.

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
