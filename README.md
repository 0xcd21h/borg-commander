<pre align="center">
██████╗  ██████╗ ██████╗  ██████╗
██╔══██╗██╔═══██╗██╔══██╗██╔════╝
██████╔╝██║   ██║██████╔╝██║  ███╗
██╔══██╗██║   ██║██╔══██╗██║   ██║
██████╔╝╚██████╔╝██║  ██║╚██████╔╝
╚═════╝  ╚═════╝ ╚═╝  ╚═╝ ╚═════╝
c o m m a n d e r
</pre>

borg is a two-panel file manager for macOS and Linux. Bash, zsh, or fish runs behind the panels.
Type commands at the bottom prompt; press Ctrl-O to switch between the panels and console.

- Copy, move, and delete files in background jobs with progress and conflict prompts.
- Find files by name or pattern, mark groups, and preview files in the other panel.
- Preview source code with syntax highlighting, formatted Markdown, and images.
- Use the keyboard or mouse. Esc followed by a digit works when F-keys are unavailable.
- Save panel folders, sorting, and theme preferences between sessions.

<p align="center">
  <img src="docs/demo.gif" alt="Borg: file navigation, quick preview, menus, and console history" width="1100">
</p>

## Install

Download a binary from [GitHub Releases](https://github.com/0xcd21h/borg-commander/releases),
or run the installer for macOS or Linux, on Intel or ARM:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

The installer verifies the archive checksum and installs `borg` in `~/.local/bin`.
To build from source, use Rust 1.97 or newer:

```sh
cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

Homebrew support is prepared. Once the tap is published:

```sh
brew install 0xcd21h/tap/borg-commander
```

Run `borg` in the folder you want to browse. F10 quits; Ctrl-O returns from the console to
the panels. Run `borg update` to update a downloaded installation, or use your package manager
if you installed through Homebrew or cargo. `borg --version` prints the installed version.

## Themes

Choose a theme with F9 → Options → Theme. These screenshots use fictional files and folders.
Panels and dialogs use terminal cells.

| Midnight | Aurora |
| --- | --- |
| <img src="docs/themes/midnight.png" alt="Midnight theme" width="400"> | <img src="docs/themes/aurora.png" alt="Aurora theme" width="400"> |
| **Neon** | **Classic** |
| <img src="docs/themes/neon.png" alt="Neon theme" width="400"> | <img src="docs/themes/classic.png" alt="Classic theme" width="400"> |
| **Paper** | **Ember** |
| <img src="docs/themes/paper.png" alt="Paper theme" width="400"> | <img src="docs/themes/ember.png" alt="Ember theme" width="400"> |

Themes use fixed colours so terminal palette customisations do not change their appearance.
Terminals without truecolour use the nearest colours in the 256-colour palette.

File icons are always shown. Portable symbols are the default: `/` for folders, `@` for links,
and `*` for executables. Release archives include **Borg Mono**, a monospace font with Nerd Font
icons. Install the regular and bold fonts from `fonts/`, select Borg Mono in your terminal,
and set `nerd-font = true` for the richer icons. Borg cannot change your terminal’s font.
The installer saves them in `${XDG_DATA_HOME:-~/.local/share}/borg-commander/fonts`;
Homebrew keeps them in `$(brew --prefix)/share/borg-commander/fonts`.

## Keys

| Key | Action |
| --- | --- |
| Up, Down, PgUp, PgDn, Home, End | Move |
| Enter, Right | Open folder / run or view file |
| Left | Go to the parent folder |
| Tab | Switch panel, or focus the quick preview / return to the file list |
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

Tab, Enter, Home, End, Left, and Right act on the panels while the command line is empty.
Once you type a command, those keys go to the shell. Ctrl-P, Ctrl-N, and Ctrl-R use your
shell’s command history. Recalled multiline commands expand the prompt area and keep the cursor visible.

If your terminal reserves F-keys, press **Esc, then a digit** within 1.5 seconds:
Esc 1–9 are F1–F9; Esc 0 is F10. For example, Esc 9 opens the menus and Esc 5 copies.
Alt-1–0 also works when Alt reaches the terminal. On macOS, enable Option as Meta in your
terminal if needed.

## Mouse and console

Click a row to focus it, double-click to open it, and right-click to mark it. Click the menu
bar or bottom actions to use them. The wheel moves through panel entries.

In the console, the wheel scrolls output history without changing the command being typed.
Shift-PgUp/PgDn scroll by a page; Shift-Home goes to the oldest output; Shift-End returns to
the live prompt. Typing returns to the live prompt as well. Use your terminal’s selection
gesture, usually Shift-drag, then its normal copy shortcut to copy visible output.

The small bars after filenames show file sizes relative to the largest file in that panel.

## Find, jobs, and viewers

Ctrl-S opens quick find. Type a name or glob such as `*.rs`; Down and Up move between matches,
Insert marks a match, Ctrl-A marks all matches, and Enter or Esc closes the search.
Matching is case-insensitive unless the query contains uppercase letters.

F5, F6, and F8 start copy, move, and delete jobs. Alt-J opens the jobs list to pause or cancel.
For name conflicts: `o` overwrites, `s` skips, `k` keeps both, `a` overwrites all, `n` skips all,
and Esc cancels. Copies are written to temporary files and renamed after completion.

F3 opens the viewer. In text files, arrows and PgUp/PgDn scroll, `w` changes wrapping, and
Esc or `q` closes. In images, Left/Right or `p`/`n` select the previous or next image.
Image protocols are used when available; other terminals display colour blocks.

Ctrl-Q shows quick preview in the other panel. Tab focuses it; arrows, PgUp/PgDn, Home/End,
and the mouse wheel scroll the document. Tab or Esc returns to the file list. Ctrl-L shows
file information.

Both text viewers highlight common programming languages, markup, and configuration files,
including Rust, Python, JavaScript, TypeScript/TSX, Go, C/C++, Java, C#, Ruby, PHP, Lua,
SQL, HTML/CSS, JSON, YAML, TOML, shell scripts, and Dockerfiles.
Markdown opens formatted by default. Press `s` or F4 in either viewer to toggle source view.
Headings, emphasis, lists, tasks, links, tables, and fenced code blocks are supported.
Markdown files larger than 2 MiB open as source to keep memory bounded.
F4 opens the selected file with `$VISUAL` or `$EDITOR`.

## Configuration

Settings are saved in `~/.config/borg-commander/config.toml`, or the directory selected by
`$XDG_CONFIG_HOME` or `$BORG_CONFIG_DIR`. F9 → Options → Settings opens checkboxes;
Space or a click toggles a setting and saves it immediately. Esc closes the window.

```toml
theme = "neon"              # midnight, aurora, neon, classic, paper, ember, or your own
nerd-font = false           # set true after selecting Borg Mono or another Nerd Font
icon-spacing = 1            # spaces after an icon, 1 to 3
status-line = true
menu-bar = false            # keep the menu bar visible
show-hidden = false
return-to-panels = true    # come back to the panels after a command finishes
restore-terminal = false    # bring back the shell scrollback on the next start
screensaver = true
screensaver-after = 300     # seconds of idle time
update-check = true         # look for a new release once a day
graphics = "auto"           # auto, kitty, sixel, iterm or off
```

Panel folders, cursor position, split, listing mode, and sorting are saved in
`~/.local/state/borg-commander`, or the location selected by `$XDG_STATE_HOME` or `$BORG_STATE_DIR`.

Custom themes go in `~/.config/borg-commander/themes/`. A theme can extend a built-in theme:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

## Development

See [DEVELOPMENT.md](DEVELOPMENT.md) for builds, tests, screenshots, and demo recording.

## License

[MIT](LICENSE). Screenshot rendering uses bundled DejaVu Sans Mono and Symbols Nerd Font.
The icon table comes from [nvim-web-devicons](https://github.com/nvim-tree/nvim-web-devicons)
through [yazi](https://github.com/sxyazi/yazi). Font and icon licenses are included in the repository.

Syntax definitions and highlighting themes come from [two-face](https://github.com/CosmicHorrorDev/two-face)
and [bat](https://github.com/sharkdp/bat); their licences are in [SYNTAX-LICENSES.md](docs/SYNTAX-LICENSES.md).
