<pre align="center">
██████╗  ██████╗ ██████╗  ██████╗
██╔══██╗██╔═══██╗██╔══██╗██╔════╝
██████╔╝██║   ██║██████╔╝██║  ███╗
██╔══██╗██║   ██║██╔══██╗██║   ██║
██████╔╝╚██████╔╝██║  ██║╚██████╔╝
╚═════╝  ╚═════╝ ╚═╝  ╚═╝ ╚═════╝
c o m m a n d e r
</pre>

<p align="center">
  <b>A two-panel terminal file commander with a real shell living behind it.</b><br>
  Midnight Commander's muscle memory, a modern terminal's good looks. Resistance is futile.
</p>

<p align="center">
  <img src="docs/demo.gif" alt="borg-commander in action: splash, icons, quick find, themes, pictures" width="1100">
</p>

---

## What it is

borg is a file manager for people who live in the terminal. Two panels sit on top of your
actual shell (bash, zsh or fish) running in a pseudo-terminal. The bottom line of the panel
view *is* your shell prompt: start typing and you are typing into the shell, press Enter
and the command runs right there. `Ctrl-O` fades the panels away so you can see the whole
shell, and back again. A small **Borg console** footer keeps the shell identifiable.

- **A real shell, not a fake prompt.** Commands, history, completion and your aliases all
  work. `cd` in the shell moves the panel; moving in the panel moves the shell.
- **Full-screen programs just work.** vim, less, htop: while they run they get every key.
- **mc-style keys.** F-keys, a menu bar on F9, Insert to mark, `+` `-` `*` to select by
  pattern, Tab to switch panels.
- **Quick find.** `Ctrl-S` and type: the cursor jumps to matches, matches light up, Ctrl-A
  marks them all.
- **Copying done properly.** Copy, move and delete run in the background with live progress,
  speed and time left, ask what to do when a file already exists (overwrite, skip, keep both,
  or the same for all), keep permissions, timestamps and symlinks, and never leave half-copied
  files behind. Pause or cancel from the Jobs list.
- **Big folders, big files.** Folders with hundreds of thousands of entries stream in; the
  viewer opens a 5 GB log instantly and keeps memory flat.
- **Pictures.** F3 on an image shows it in Kitty, Ghostty, WezTerm, iTerm2 and Sixel
  terminals, and as colour blocks everywhere else.
- **Good looking.** Rounded borders, gradients and a Neon theme that breathes, Nerd Font icons
  coloured per file type, smooth fades, and themes you can switch live.
- **Works over SSH.** Animations adapt to the connection, colours adapt to the terminal.
- **Remembers.** Folders, cursor, split and (optionally) your shell scrollback come back next
  time.

## Getting started

### Install

With Homebrew (macOS and Linux):

```sh
brew install 0xcd21h/tap/borg-commander
```

With the install script (macOS and Linux, Intel and ARM; installs to `~/.local/bin`):

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

Or from source with Rust 1.97 or newer:

```sh
cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

Every option installs a single binary called `borg`. Prebuilt binaries are also attached to each
[GitHub release](https://github.com/0xcd21h/borg-commander/releases).

### Update

```sh
borg update
```

borg checks for a new release once a day in the background and tells you when one is out
(turn that off with `update-check = false`). `borg update` downloads the new version, checks
its SHA-256 and replaces itself. If borg came from Homebrew or cargo it tells you the
matching upgrade command instead. `borg --version` prints the version.

### Run

```sh
borg
```

borg opens in the current folder, with your `$SHELL` running underneath. Quit with **F10**.

Running `borg` from inside borg's own shell is refused on purpose. Press **Ctrl-O** to get
back to the panels instead.

### Make it pretty

- Use a terminal with **24-bit colour** (Ghostty, kitty, WezTerm, iTerm2, Alacritty, Windows
  Terminal, recent GNOME Terminal). borg detects it through `COLORTERM`.
- Install a **[Nerd Font](https://www.nerdfonts.com)** (JetBrains Mono, Fira Code, Iosevka
  and Hack are all great), set it as your terminal font, then switch icons on with
  **F9 › Options › File icons** or `icons = true` in the config.
- Try the themes: **F9 › Options › Theme…** previews each one live.

Fonts belong to your terminal, not to borg, so the font is set in your terminal's settings.
Icons are plain characters, so they also work over SSH as long as your *local* terminal uses
a Nerd Font.

## Keys

| Key | Action |
| --- | --- |
| Up, Down, PgUp, PgDn, Home, End | Move |
| Enter, Right | Open folder / run or view file |
| Left | Go to the parent folder |
| Tab | Switch panel |
| Ctrl-O | Show the shell / back to the panels |
| Ctrl-S | Quick find in the folder |
| Ctrl-Q | Quick view: the other panel previews the file under the cursor (text, picture or folder) |
| Ctrl-L | Info: the other panel shows size, mode, owner, dates and free space |
| Insert, Ctrl-T, Shift-Up/Down | Mark files |
| `+` `-` `*` | Select, unselect or invert by pattern (`*.rs`, `build/`) |
| Alt-Enter | Put the marked names (or the current one) on the command line |
| Shift-Enter, Ctrl-J | Multi-line command: edit in a box above the prompt; Enter adds a line, Alt-Enter (or Ctrl-Enter) runs, Esc puts it back on the prompt |
| Alt-A / Alt-Shift-A | Put this panel's / the other panel's folder path on the command line |
| Shift-PgUp / Shift-PgDn | Scroll console output history |
| Shift-Home / Shift-End | Oldest console output / back to the live prompt |
| Ctrl-P / Ctrl-N, Ctrl-R | Shell history: previous / next command, search (handled by your shell) |
| Alt-. | Show hidden files |
| Alt-Shift-Left/Right | Resize the panels |
| Ctrl-U | Swap panels |
| Alt-J | Jobs: pause or cancel running copies, moves and deletes |
| F1 | About |
| F2 | File menu |
| F3 | View (text or picture) |
| F4 | Edit with `$VISUAL` / `$EDITOR` |
| F5 / F6 | Copy / move to the other panel |
| F7 | Make folder |
| F8 | Delete |
| F9 | Menu bar |
| F10 | Quit |

Tab, Enter, Home, End, Left, Right and the selection keys act on the panels only while the
command line is empty. Once you type something they belong to the shell again.

Commands you run from the prompt show their output in the shell view; press Ctrl-O to go back
to the panels. Full-screen programs (vim, less, htop…) return you to the panels as soon as
they exit.

**No F-keys?** Some terminals keep them for themselves (VS Code's terminal uses F1, F5 and
F11, for example). Press Esc, then the digit: Esc 1 … Esc 9 are F1 … F9 and Esc 0 is F10.
For example, **Esc, then 9** opens the menus; **Esc, then 5** copies; **Esc, then 0** quits.
Press the digit within 1.5 seconds. The bottom bar labels these actions F1–F10 and shows the
alternate keys on wide terminals. Alt-1 … Alt-0 do the same where Alt reaches the terminal; on
macOS that needs Option as Meta (`"terminal.integrated.macOptionIsMeta": true` in VS Code).
In VS Code you can also set `"terminal.integrated.sendKeybindingsToShell": true` to let the
F-keys through.

### Mouse and console history

Click a panel row to focus it, double-click to open it, or right-click to mark it. The wheel
moves through panel entries. Click the bottom **F9 Menu** action to open the menus, then click
a menu title and an item. The other bottom actions are clickable too.

In the console, the wheel scrolls **output history** without changing the command you are
typing. Shift-PgUp/PgDn scroll by a page; Shift-Home goes to the oldest output and Shift-End
returns to the live prompt. Typing also returns to the live prompt. Use your terminal's native
selection gesture (usually **Shift-drag**) to select visible output, then its normal copy
shortcut. Full-screen programs retain their own mouse controls.

### Quick find

`Ctrl-S`, then type. Matching is smart-case and globs such as `*.png` work too.

| Key | Action |
| --- | --- |
| Down, Tab, Ctrl-S | Next match |
| Up, Shift-Tab, Ctrl-R | Previous match |
| Insert | Mark this match and go to the next |
| Ctrl-A | Mark every match |
| Enter, Esc | Close and stay on the match |
| Ctrl-S on an empty bar | Bring back the last search |

### Copy, move and delete

F5, F6 and F8 start a background job; you can keep working (or use the shell) while it runs.
Progress, speed and the time left show in the footer of the panel being written to. When a
file already exists borg asks:

| Key | Answer |
| --- | --- |
| o | Overwrite |
| s | Skip |
| k | Keep both (the copy gets a numbered name) |
| a / n | Overwrite all / skip all for the rest of this job |
| c, Esc | Cancel the job |

Files are written under a temporary name and renamed into place when complete, so a cancelled
or failed copy never leaves a half-written file. Moves within one disk are instant renames.

### Viewer

Text: arrows and PgUp/PgDn scroll, `w` toggles wrapping, Left/Right scroll sideways, Esc or q
closes. Pictures: Left/Right (or `n`/`p`) step through the pictures in the folder.

## Configuration

Settings live in `~/.config/borg-commander/config.toml` (or `$XDG_CONFIG_HOME/borg-commander`,
or `$BORG_CONFIG_DIR`). Most of them can also be changed from **F9 › Options**, which saves
the file for you.

```toml
theme = "neon"              # midnight, aurora, neon, classic, paper, ember, or your own
icons = true                # Nerd Font file icons
icon-spacing = 1            # spaces after an icon, 1 to 3
status-line = true
menu-bar = false            # keep the menu bar visible
animations = true
show-hidden = false
return-to-panels = false    # come back to the panels after a command finishes
restore-terminal = false    # bring back the shell scrollback on the next start
startup-animation = true
screensaver = false
screensaver-after = 300     # seconds of idle time
delight = true              # little surprises
update-check = true         # look for a new release once a day
graphics = "auto"           # auto, kitty, sixel, iterm or off
```

The session (folders, cursor, split, listing mode, sort) is kept in
`~/.local/state/borg-commander/` (or `$XDG_STATE_HOME`, or `$BORG_STATE_DIR`).

### Your own theme

Drop a TOML file into `~/.config/borg-commander/themes/`. Start from a built-in theme and
change only what you want:

```toml
name = "Deep Space"
extends = "neon"

[effects]
border-gradient = ["#7a5cff", "#00e5ff"]
glow = 0.3

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

It appears in the theme picker the next time borg starts.

## Terminals, SSH and tmux

| | Local | Over SSH | In tmux / screen |
| --- | --- | --- | --- |
| Colours and gradients | 24-bit, or mapped to 256 colours | same | same |
| Animations | smooth | lighter frame rate | as local |
| Icons | with a Nerd Font | with a local Nerd Font | yes |
| Pictures | Kitty, iTerm2, Sixel or colour blocks | same | colour blocks |

Picture support is detected at start-up. Set `graphics = "off"` (or a specific protocol) if
detection guesses wrong.

## Hidden things

There are a few. **F1** is a good place to start looking.

## Contributing

See [DEVELOPMENT.md](DEVELOPMENT.md) for how the code is organised, how to run the tests and
how to record screenshots and the demo.

## License

MIT, see [LICENSE](LICENSE). The test kit bundles DejaVu Sans Mono and Symbols Nerd Font
(MIT) for rendering screenshots. The icon table comes from
[nvim-web-devicons](https://github.com/nvim-tree/nvim-web-devicons) by way of
[yazi](https://github.com/sxyazi/yazi), both MIT; their licenses are in `crates/borg-ui/icons/`.
