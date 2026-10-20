# Borg user guide

[Back to the showcase](../README.md)

[Install](#install) · [Themes and icons](#themes-and-icons) · [Keys](#keys) ·
[Mouse and console](#mouse-and-console) · [Git plugin](#git-plugin) ·
[Find, jobs, and viewers](#find-jobs-and-viewers) · [Configuration](#configuration)

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

## Themes and icons

Choose a theme with F9 → Options → Theme. See the [theme gallery](../README.md#themes)
for Midnight, Aurora, Neon, Classic, Paper, and Ember.

Themes use fixed colours so terminal palette customisations do not change their appearance.
Terminals without truecolour use the nearest colours in the 256-colour palette.

File icons are always shown. Ghostty uses the richer Nerd Font icons automatically. Other
terminals default to portable symbols: `/` for folders, `@` for links, and `*` for executables.
Set `icon-style = "nerd"` to use rich icons in another terminal with a suitable font, or
`icon-style = "portable"` to force portable symbols. The default, `"auto"`, adapts to the terminal
without saving that detection as a font preference.

Release archives include **Borg Mono**, a monospace font with Nerd Font icons. Install the
regular and bold fonts from `fonts/`, select Borg Mono in your terminal, and choose the
`"nerd"` icon style. Borg cannot change your terminal’s font.
The installer saves them in `${XDG_DATA_HOME:-~/.local/share}/borg-commander/fonts`;
Homebrew keeps them in `$(brew --prefix)/share/borg-commander/fonts`.

## Collections and recursive search

The Search menu opens collections and recursive filename search. Both use sortable
results with source paths and the existing viewer and quick preview.

- **Ctrl-F:** search recursively below the current folder, using a name fragment or
  glob such as `*.log`. Lowercase queries ignore case; uppercase queries match case.
- **Alt-B:** collect marked files, or the current file, in the other panel. Adding a
  reference does not copy the file's contents.
- **Alt-V:** choose a saved collection from a scrollable list with entry counts and
  size estimates. Press F7 or N to create one. Collections survive restarting Borg.
- **Esc:** hide results, preserving the cursor, scroll, marks, filter, and grouping.
- **Alt-R:** resume the hidden view. A scan continues while hidden.
- **Alt-D:** analyze folder sizes, including hidden files by default. Ctrl-C stops a scan.
- **Alt-W:** maximize or restore the active result pane. Results open in a normal
  pane by default. The title-bar `[+]` (maximize), `[=]` (restore), and `[-]` (fold) controls are clickable.
  Fold hides the view; Alt-R resumes it with its layout and selection preserved.
- **Ctrl-S:** filter results by name or source path. An empty filter shows everything.

Click a column heading to sort or reverse it. The Left/Right menus also offer sorting;
Search adds source-folder sorting and grouping by folder or file type. Enter on a
group heading folds it; Insert marks its matching members. Marks survive sorting and filtering; the footer counts marked
files hidden by the filter as well.

F3 and Ctrl-Q open the real source. F5 copies to a real destination; if the destination
panel is a collection, it adds references instead. F6 requires a real destination.
F8 removes references from a collection. In search results, F8 asks to delete the
originals. Shift-F8 also deletes originals from a collection, after confirmation.
When both a folder and its contents are marked, the folder is sent to the job once.
Reveal original opens its source folder in the other panel. Left or Esc hides
results; Alt-R resumes them in the same place. Tab switches virtual panels even
when the shell has a pending command. The shell keeps a real working directory.
F10 always asks before quitting.

Search reads filenames and metadata, with a cancellable worker and a maximum of
20,000 matches per search. The context line shows completion, errors, skipped entries,
and whether that limit was reached. Hidden folders follow Show hidden files. Search
stays on the starting filesystem and does not recurse through directory symlinks.
The search window can restrict results to files or folders and set a minimum size.
Choose **Folder totals** to measure all descendant files before filtering by name,
size, or minimum file count. It keeps the largest 20,000 matching folders and sorts
by content size; click Files to rank by number of files instead. Use a pattern such
as `node_modules` or `*cache*`, or `*` to inspect everything beneath the chosen root.
Totals count file contents, not guaranteed recoverable disk space. Nested folder
rows overlap. Symlinks are excluded; filesystem boundaries and unreadable or excluded
children make totals incomplete, shown with ≥. Directory scans use metadata and do
not read file contents. Results appear after measurement; the scan remains cancellable.
Indexed and content search are planned separately.

Git and archive actions currently work from real directory panels. Use Reveal original
to access them for a collected file.

## Keys

The two-row shortcut bar sits below the command line. Function keys are on the first
row; search, collections, and other contextual shortcuts are on the second. Both
rows are clickable. Notices temporarily replace the second row.

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
| Shift-Enter, Ctrl-J | Insert a newline at the command-line cursor; Enter runs the command, Ctrl-C clears it |
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
shell’s command history. Shift-Enter (or Ctrl-J) inserts a newline without running the command.
Typed and recalled multiline commands expand the same prompt area, shrinking the panels
and keeping the cursor visible. The shell retains its usual editing and completion keys.

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

## Git plugin

Git is a bundled plugin, enabled by default. F9 → Options → Plugins opens its checkbox;
turning it off hides the Git menu and status columns and stops background status scans.
It requires Git on your PATH.

Inside a repository, panels show the branch and a two-character status beside each file.
The left character describes the index (staged changes); the right describes the working tree.
`M` means modified, `A` added, `D` deleted, `R` renamed, `??` untracked, and `U` a conflict.
Folders summarize changes below them. Clean and ignored files have no marker.
Outside repositories, filenames use the extra space.

The Git menu offers repository status, unstaged and staged diffs, recent commits, and
stage/unstage for marked files (or the current file when nothing is marked).
Diffs and history open in the regular viewer. Unstage preserves working files.
Status refreshes after navigation, commands, and Git actions, and every five seconds while
panels are idle. Refresh status in the Git menu rescans immediately.

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
file information. F3 expands a text preview into the viewer; closing it returns to the
preview with your position, wrapping, and source mode preserved. Both use the same
bounded file reader, so opening a large text file reads near the first page rather than
loading the whole file.

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
icon-style = "auto"         # auto, nerd, or portable; Ghostty gets rich icons automatically
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

[plugins]
git = true
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

For building, testing, or contributing, see [Development](../DEVELOPMENT.md).
