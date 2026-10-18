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
  borg-core     pure state and logic: update(state, msg, now) -> effects
  borg-ui       rendering: panels, dialogs, themes, effects, art (ratatui buffers in, nothing out)
  borg-term     the embedded shell: PTY, terminal emulator, shell integration (OSC 7 / 133)
  borg-vfs      file system access: streaming directory listings
  borg-view     the pager behind F3: constant memory on files of any size
  borg-gfx      terminal graphics: protocol detection, Kitty / Sixel / iTerm2, half blocks
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

Animations are values, not threads. A `Fade` knows its start, target and duration, and
`AppState::pace(now)` tells the loop whether it needs smooth frames (16 ms locally, 40 ms over
SSH), ambient frames (80 ms for spinners and art) or none at all.

### File operations

Copy, move, delete, mkdir and archive actions are built by `borg_core::command::CommandLine`
(with shell quoting) and run in the embedded shell, hidden at first. If a command takes longer
than 600 ms the shell is revealed so prompts like `cp -i` can be answered, and the panels come
back when it finishes.

### Rendering pipeline

- `panel_view` draws panels: breadcrumbs, rows, icons, the cursor, size bars and spinners.
- `fx/` holds the effects engine: gradients, glow, dimming, shadows, the braille canvas and
  cube, the starfield, glyph rain, spinners, slides and unfolds, and 256-colour quantising.
- `blend::crossfade` mixes two buffers per cell. It powers the shell fade, theme previews and
  overlay fades.
- Themes are TOML files in `crates/borg-ui/themes/`. `ThemeSet` loads them, resolves
  `extends` and merges styles and effects.

### Graphics

`borg-gfx` probes the terminal once at start-up (Kitty query, cell size, DA1) and picks a
protocol. `Graphics::frame` diffs the placements of each frame: Kitty images are sent once and
placed by id; Sixel and iTerm2 images are re-sent when something covers them and trigger a
repaint when removed. Without a protocol the UI draws pictures with half blocks, which is also
what tests see.

### Pixel-drawn chrome

With the Kitty protocol, `borg_ui::draw` also returns *underlay* placements: shape descriptions
(`borg_gfx::Shape`: rounded cards, gradient strokes, inner glow, blurred shadows) rendered by
tiny-skia and cached by value, placed at negative z so they sit under the text. The text layer
drops its box-drawing borders in this mode, and any cell background that would be hidden under
a card (buttons, menu selections, input fields) is redrawn as a small rounded highlight. The
cursor pill follows `PanelMotion::cursor`, a fade over the cursor row, so it glides between rows
using Kitty's sub-cell pixel offsets. Theme transitions and the shell fade fall back to the cell
renderer for their few frames.

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
- Fixtures write `startup-animation = false` so tests start straight in the panels. Use
  `fixture.configure("...")` to add settings.
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

### The demo

`docs/demo.gif` is recorded from a real session, not drawn by hand:

```sh
scripts/demo.sh                    # writes docs/demo.gif
scripts/demo.sh /tmp/demo.gif      # or anywhere else
```

The script lives in `crates/borg-testkit/examples/demo.rs`. It builds a small project, runs
the release binary in a PTY, plays a list of key presses and records the screen with
`borg_testkit::Recorder`. Frames that did not change are dropped, and each GIF frame stores
only the pixels that changed, which keeps the file small. The README demo uses JetBrains Mono
through `BORG_SCREENSHOT_FONT`.

## Conventions

- No comments in code: names, types and small functions carry the meaning.
- No magic values: name constants, keep them next to their use.
- Prefer pure functions and data in, data out. Side effects belong in `borg-app`.
- Keep `unsafe` out (`unsafe_code = "forbid"` in the workspace lints).
- New behaviour comes with a test at the lowest layer that can catch it; anything a user sees
  also gets a render snapshot.
