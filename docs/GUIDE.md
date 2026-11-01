# Borg user guide

[Back to the showcase](../README.md)

[Install](#install) · [Themes and icons](#themes-and-icons) · [Keys](#keys) ·
[Mouse and console](#mouse-and-console) · [Git plugin](#git-plugin) ·
[Find, jobs, and viewers](#find-jobs-and-viewers) · [Configuration](#configuration)

## Install

Download a binary from [GitHub Releases](https://github.com/0xcd21h/borg-commander/releases),
or run the installer. It works on macOS and Linux, on both Intel and ARM:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

The installer verifies the archive checksum and puts `borg` in a folder on your PATH, so you
can run it straight away in the same terminal. It picks `~/.local/bin` if that is already on
your PATH, then `/usr/local/bin` if it is writable (for example as root in a container), and
otherwise uses `~/.local/bin` and adds it to your shell's startup file. When it finishes, it
starts borg; set `BORG_NO_RUN=1` to skip that, or `BORG_INSTALL_DIR` to choose the folder.
If the same version is already installed, running the installer again skips the download.

To build from source, you need Rust 1.97 or newer:

```sh
cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

Homebrew support is ready and will work once the tap is published:

```sh
brew install 0xcd21h/tap/borg-commander
```

Run `borg` in the folder you want to browse. F10 quits, and Ctrl-O switches from the console
back to the panels. To update, run `borg update` for a downloaded copy, or use Homebrew or cargo
if you installed it that way. `borg --version` prints the installed version.

## Themes and icons

Choose a theme in F9 → Options → Theme. The [theme gallery](../README.md#themes) shows all six:
Midnight, Aurora, Neon, Classic, Paper, and Ember.

Themes use fixed colors, so a customized terminal palette does not change how they look.
Terminals without true color get the nearest colors from the 256-color palette.

Every file has an icon. In Ghostty, Borg uses the richer Nerd Font icons automatically; other
terminals get portable symbols by default: `/` for folders, `@` for links, and `*` for
executables. Set `icon-style = "nerd"` to use rich icons in another terminal with a suitable font,
or `icon-style = "portable"` to always use the portable symbols. The default, `"auto"`, adapts to
the terminal and does not save what it detects as your font preference.

Release archives include **Borg Mono**, a monospace font with Nerd Font icons. Install its regular
and bold styles from `fonts/`, select Borg Mono in your terminal's settings, and choose the
`"nerd"` icon style. Borg cannot change your terminal's font for you.
The installer puts the fonts in `${XDG_DATA_HOME:-~/.local/share}/borg-commander/fonts`;
Homebrew puts them in `$(brew --prefix)/share/borg-commander/fonts`.

## Collections and recursive search

The Search menu holds collections and recursive search by file name or contents. Both show their
results as a sortable list with each file's source path, and both work with the viewer and quick
preview.

- **Ctrl-F:** search the current folder and everything below it. Every word must match:
  `main rs` finds `main.rs`, and `a | b` finds either word. A lowercase word ignores case; a word
  with an uppercase letter matches case exactly. You can also use globs such as `*.log`, quoted
  phrases, paths with a slash such as `src/lib`, and `-word` to exclude matches, plus the filters
  `ext:rs,toml`, `type:file`, `type:dir`, `type:link`, `size:>10M`, `size:1M..50M`, `mod:<7d`
  (changed in the last week), `mod:>1y` and `in:src` (a folder anywhere in the path). Recent
  searches are listed below the form, with those made in the same folder first; click one, or
  press Ctrl-P and Ctrl-N in the query field, to reuse it. Names match in any alphabet whatever
  the case of a glob, and accented letters match however the file system stores them. Matches
  close to the search folder appear first, while the rest of the tree is still being read.
- **Alt-K:** collect the marked files, or the current file, into a collection in the other panel.
  A collection holds references, so the file's contents are not copied.
- **Alt-V:** choose a saved collection from a scrollable list that shows how many entries each
  has and an estimate of its size. Press F7 or N to create one. Collections are kept when Borg
  restarts.
- **Esc:** hide the results and keep the cursor, scroll position, marks, filter, and grouping.
- **Alt-Shift-R:** show hidden results again. A scan keeps running while its results are hidden.
- **Alt-D:** open Folder sizes to find folders by total size or file count. Hidden files are included
  by default. Alt-D opens the Ctrl-F dialog set to *Folder totals*; change *Look for* to switch
  between the two. Ctrl-C stops a scan.
- **Alt-S:** calculate the sizes of the selected folder. **Alt-Shift-D:** open a size tree of
  the current folder.
- **Alt-W:** maximize or restore the active result pane. Results open in a normal pane by
  default. The `[+]` (maximize), `[=]` (restore), and `[-]` (fold) controls in the title bar can
  be clicked. Folding hides the view; Alt-Shift-R brings it back with its layout and selection.
- **Ctrl-S:** filter the results by name or source path. An empty filter shows everything.

### Searching inside files

Type text in **Containing** in the Ctrl-F dialog to list the files below the folder that contain
it. The name pattern still applies, so `*.rs` with `fn main` in **Containing** finds the Rust files
that have it. The switches beside the field work as in the find bar: Alt-C matches case exactly
(otherwise a lowercase query matches any case), Alt-W matches whole words, and Alt-R reads the
text as a regular expression. You can click the switches too. Ctrl-P and Ctrl-N in the field bring
back recent queries from the find bar and earlier searches. A search for text lists files only,
so it takes no folder totals or minimum file count, and **Folder sizes** has no **Containing**
field.

Each result shows its first matching line, with the line number, in the **Match** column, and the
status line adds the line number to the file's path. F3 opens the file in the viewer at that match
with the find bar open on the query, so Enter goes on to the next one. Markdown and JSON files open
as source there, since the search read the file as it is, and `s` switches to the formatted view. F4
and Alt-E open the file in Borg's editor at that match, with the match selected and the find bar
open on the query. Borg reads the files in the background, and the context line counts the files
read. Binary files (a NUL byte in the first 8 KiB) and files over 64 MiB stay unread and are counted
as binary or too large. Ctrl-C stops the search, and it stops at 20,000 files like any search.

### Working with results

Click a column heading to sort by it, and click again to reverse the order. The Left and Right
menus also offer sorting, and the Search menu adds sorting by source folder and grouping by
folder or file type. Enter on a group heading folds the group; Insert marks the matching files
in it. Marks are kept when you sort or filter, and the footer also counts marked files that the
filter hides.

F3 and Ctrl-Q open the original file. F5 copies to a real folder; if the other panel is a
collection, F5 adds references to it instead. F6 needs a real folder as its destination. In a
collection, F8 removes references and leaves the files alone; in search results, F8 asks to
delete the original files. Shift-F8 deletes the original files from a collection too, after
you confirm. When both a folder and files inside it are marked, the job handles the folder once.
**Reveal original** opens the file's folder in the other panel. Left or Esc hides the results,
and Alt-Shift-R brings them back where you left them. Tab switches between panels even when the
shell has an unfinished command. The shell always stays in a real folder. F10 always asks before
quitting.

Git and archive actions only work in real folder panels for now. To use them on a collected
file, open its folder with **Reveal original** first.

### Search limits

Search reads file names and metadata in a background worker that you can cancel, and stops at
20,000 matches. The context line shows whether the search has finished, any errors and skipped
entries, and whether it hit that limit. Hidden folders are searched only when **Show hidden
files** is on. Search stays on the file system where it started and does not follow symlinks to
folders. The search window can limit results to files or to folders and set a minimum size.

### Folder totals and the size tree

Choose **Folder totals** to measure every file below each folder before filtering by name, size,
or minimum file count. Borg keeps the 20,000 largest matching folders and sorts them by size;
click Files to rank them by file count instead. Use a pattern such as `node_modules` or
`*cache*`, or `*` to see every folder below the starting folder.

Totals add up file contents. Deleting a folder may free a different amount of disk space.
Nested folders are counted in their parents' totals as well. Symlinks are never followed, scans
stay on the file system where they started, and a folder that contains itself (through a bind
mount or a hard-linked folder) is counted only once. When a total is incomplete because of such
a boundary, or because a subfolder could not be read or was left out, it is shown with ≥.
Folder scans read metadata only, never file contents. Results appear once the measurement is
done, and you can cancel the scan at any time.

**Folder size tree** shows the total for the starting folder and its subfolders in the current
panel. Right expands a branch, or enters it if it is already expanded; Left folds a branch or
selects its parent. Enter opens the folder itself. Click a branch arrow to fold or expand it, or
a column heading to sort folders on the same level by name, size, or file count. Esc hides the
tree, and Alt-Shift-R brings back its branches, marks, and cursor. File operations work on the
real paths, and if you mark both a folder and a subfolder, a copy or delete job gets only the
folder.

### What searches leave out

While **Leave out build output, caches and ignored files** is checked, name searches skip
generated folders: `node_modules`, `.git`, Python caches and virtual environments, folders such
as `target` and `dist` that sit next to a build file (`Cargo.toml`, `package.json`), and any
folder tagged with a `CACHEDIR.TAG` file. They also skip files excluded by `.gitignore`,
`.ignore`, `.rgignore`, or `.borgignore`, and the globs in `[search] exclude`.

To change what is left out, press F4 in the search dialog or click **F4 Edit**. You can switch
generated folders and ignore files on or off, and add (Ins), edit (Enter), or remove (Del)
patterns. Changes are saved to the configuration file. A left-out folder still appears if its
own name matches, and the context line counts how many folders were left out. Add `excluded:yes`
to a query to search everything once, or `hidden:yes` to include hidden files. Size scans always
read everything.

On macOS, searches and size scans that start above `~/Library/Containers` and
`~/Library/Group Containers` skip them: reading there makes macOS hold the read until someone
answers whether to allow access to another app's data. A search started inside one of these
folders does read it.

Search history stays on this computer and only you can read it. Set
`history = false` under `[search]` to stop keeping it.

### Instant search with an index

**Search → Index this folder for instant search** keeps an index of every name below the active
folder, so Ctrl-F there, or in any folder below it, answers at once. The context line says when
the index was built. Borg rebuilds indexes in the background after it starts and whenever an
index is more than 15 minutes old, and the old index keeps answering until the new one is ready.
A search that leaves nothing out, or uses different exclusions, reads the folders instead, and so
does a search for text, because the index holds only names. Choose
the menu item again to stop indexing and delete the index. Indexes are kept in the cache folder
next to the size scans, and `[index]` in the configuration lists the indexed folders.

### Counting sizes in the background

To measure sizes while keeping ordinary file panels open, press **Alt-I** or choose
**Search → Count sizes in background**. Borg scans every folder below the current one, including
hidden files, and fills in each folder's total as it finishes. You can keep browsing or use the
console while it runs. Folders still being measured show a growing total marked `≥`, and totals
stay when you leave a folder and come back. The panel footer shows progress; press **Alt-I**
again, or click the footer, to stop the scan and keep the totals measured so far. Starting a
copy, move, or delete job stops a size scan in progress. Finished scans share their cache with
the size tree and Folder totals.

### Saved size scans

Finished scans are saved on this computer. Folder totals and the size tree both reuse a saved
scan of the same folder with the same hidden-file setting. When you reopen a folder that was
measured before, its saved sizes appear at once, marked `~`, with the date of the measurement
above the tree. **Ctrl-R** scans again in either size view, and **Ctrl-C** cancels. The previous
tree stays visible while it refreshes, and stays if the new scan is cancelled or fails. Ordinary
folder listings also show saved totals where they exist. Changes made outside Borg can make them
out of date, and browsing never starts a recursive scan by itself. Copy, move, and delete jobs in
Borg discard saved scans that cover the paths they changed.

The local SQLite cache keeps every measured folder for up to 16 starting folders. Trees load a
branch's subfolders when you expand it. A branch shows up to 20,000 folders, and Folder totals
keeps the 20,000 largest matches; these display limits do not stop the scan or change the
totals. A "Files in this folder" row accounts for the bytes stored directly in an expanded
folder. Errors and skipped subfolders mark totals as incomplete. The cache lives in
`~/.cache/borg-commander`, or under `XDG_CACHE_HOME` if it is set; `BORG_CACHE_DIR` overrides
the location. The cache only holds measurements that can be recreated, and is separate from
collections. A hard-linked file counts once for every path it has. All sizes measure file contents, so the
space a file takes on disk can be larger or smaller.

## Keys

The two-row shortcut bar sits below the command line. The first row has the function keys; the
second has search, collections, and other shortcuts that depend on what you are doing. You can
click both rows. Notices replace the second row for a moment.

| Key | Action |
| --- | --- |
| Up, Down, PgUp, PgDn, Home, End | Move |
| Enter, Right | Open a folder / run or view a file |
| Left | Go to the parent folder |
| Tab | Switch panels, or move into the quick preview and back to the file list |
| Ctrl-O | Show the shell / return to the panels |
| Alt-I | Count folder sizes in the background / stop counting |
| Ctrl-S | Quick find in the folder |
| Ctrl-Q | Quick view: preview the file under the cursor (text, picture or folder) in the other panel |
| Ctrl-L | Info: show size, permissions, owner, dates and free space in the other panel |
| Insert, Ctrl-T, Shift-Up/Down | Mark files |
| `+` `-` `*` | Select, unselect or invert by pattern (`*.rs`, `build/`) |
| Alt-Enter | Put the marked names (or the current one) on the command line |
| Ctrl-Insert / Alt-Shift-Insert | Copy the marked names / their full paths |
| Shift-Enter, Ctrl-J | Insert a line break at the cursor on the command line; Enter runs the command, Ctrl-C clears it |
| Alt-A / Alt-Shift-A | Put the path of this panel's / the other panel's folder on the command line |
| Shift-PgUp / Shift-PgDn | Scroll back and forth through console output |
| Shift-Home / Shift-End | Go to the oldest console output / back to the live prompt |
| Ctrl-P / Ctrl-N, Ctrl-R | Shell history: previous / next command, search (handled by your shell) |
| Alt-. | Show hidden files |
| Alt-R | Reload the active panel: its folder, search results or collection |
| Alt-H | Remote hosts: paired machines and their sessions |
| Alt-Shift-Left/Right | Resize the panels |
| Ctrl-U | Swap panels |
| Alt-J | Jobs: pause or cancel running copies, moves and deletes |
| Alt-M | Activity monitor: processes and memory |
| Alt-C | Docker: Compose projects and containers |
| Alt-T | Time Machine: what backups leave out, on macOS |
| Alt-E | Edit the selected file in this panel, in place of its files |
| F1 | About |
| F2 | File menu |
| F3 | View (text or picture) |
| F4 / Shift-F4 | Edit in Borg's editor / in `$VISUAL` or `$EDITOR` |
| F5 / F6 | Copy / move to the other panel |
| F7 | Create a folder |
| F8 | Delete |
| F9 | Menu bar |
| F10 | Quit |

Tab, Enter, Home, End, Left, and Right control the panels while the command line is empty. Once
you start typing a command, these keys go to the shell. Ctrl-P, Ctrl-N, and Ctrl-R use your
shell's command history. Shift-Enter (or Ctrl-J) inserts a line break without running the
command. Multi-line commands, typed or recalled from history, make the prompt area taller and
the panels shorter, so the cursor always stays visible. All of your shell's usual editing and
completion keys keep working.

Copies reach your computer's clipboard through the terminal, even over SSH and in a mirror you
control. Borg also remembers your last 20 copies: Ctrl-V or Shift-Insert pastes the latest one
into Borg's own fields, such as dialogs and the memory search, even where the terminal cannot
reach your clipboard. On the command line Ctrl-V belongs to the shell, so paste there with your
terminal's paste shortcut. The File menu offers both copy actions for keyboards without an
Insert key. Copies too large for the terminal to pass on stay in Borg.

If your terminal keeps the F-keys for itself, press **Esc, then a digit** within 1.5 seconds:
Esc 1 to Esc 9 act as F1 to F9, and Esc 0 as F10. For example, Esc 9 opens the menus and Esc 5 copies.
While Borg waits for the digit, the function key row shows which digit to press. If something is
open (a notice, results, the quick view or Info), the first Esc closes it, and a second Esc
cancels. Alt-1 to Alt-0 work too when the terminal passes Alt through. On macOS, you may need to
turn on "Use Option as Meta key" in your terminal.

## Mouse and console

Click a row to move the cursor to it, double-click to open it, and right-click to mark it. Click
the menu bar or the shortcut bar to use them. The mouse wheel scrolls through the panel.

Dialogs work with the mouse too. Click a row in a list to choose it; in the theme and language
pickers, Remote hosts and Remote access, a second click confirms it. The wheel scrolls lists such
as themes, settings, and jobs. The keys shown in a dialog's border can be clicked: click
**Esc close** to close the dialog, or a button to answer a question. Clicking outside a list, a
picker or About closes it, while forms and questions stay open until you answer them.

Ctrl-O switches between the panels and the console. Before you type or open the console, the
shell moves to the active panel's folder. In zsh this happens silently and the prompt is redrawn;
in bash and fish you see a short `cd` command in the console instead. If you opened the console
yourself, it stays open after commands and Ctrl-C. "Return to panels after command" only applies
to commands run from the panels. At the shell prompt, Option-Left and Option-Right (Alt on Linux)
move by word.

In the console, the mouse wheel scrolls back through the output without touching the command
you are typing. Shift-PgUp and Shift-PgDn scroll by a page, Shift-Home jumps to the oldest
output, and Shift-End returns to the live prompt, as does typing. To copy visible output, select
it the way your terminal allows, usually by Shift-dragging, then use its normal copy shortcut.

## Git plugin

Git support is a bundled plugin and is on by default. Its checkbox is in F9 → Options → Plugins;
turning it off hides the Git menu and status columns and stops background status checks. Git
must be installed and on your PATH.

Inside a repository, the panels show the current branch and a two-character status next to each
file. The left character is for the index (staged changes) and the right one for the working
tree. `M` means modified, `A` added, `D` deleted, `R` renamed, `??` untracked, and `U` a
conflict. Folders summarize the changes inside them. Clean and ignored files have no marker.
Outside a repository, file names use the extra space.

The Git menu shows repository status, unstaged and staged diffs, and recent commits, and can
stage or unstage the marked files (or the current file if nothing is marked). Diffs and history
open in the regular viewer. Unstaging leaves your working files untouched. Status refreshes after
you move between folders, run commands, or use Git actions, and every five seconds while the
panels are idle. **Refresh status** in the Git menu checks again right away.

## Docker plugin

**Alt-C** or **Command → Docker** lists your containers in the active panel, grouped by Compose
project, with containers that are not part of a project at the end. The other panel shows the
selected container: its status, ports, networks, mounts, command, and restart policy, plus live
CPU, memory, network, and disk use with a chart of the last minute. For a project, it shows the
project's folder, its containers, and their combined use. Esc closes the view.

| Key | Action |
| --- | --- |
| ↑ ↓, PgUp, PgDn, Home, End | Move |
| ← → / Enter | Fold or unfold a project; Enter on a container moves to its details |
| S / X / R | Start / stop / restart the container, or the project's containers |
| P | Pause or resume |
| F | Browse the container's files |
| E, Alt-2 | Open a shell in the container, or go back to its open shell |
| Alt-1 | Show the container's overview again, also from inside its shell |
| `/` | Filter by name, service, image or project |
| Tab | Switch between the list and the details |
| Alt-W | Maximize the focused pane |

Stop and restart ask for confirmation first. On a project, an action only affects the
containers it would change, so Start starts only the stopped ones.

### Container files

**F** on a running container closes the Docker view and shows the container's file system in
that panel, starting in its working directory. The panel title shows the container's name. The
other panel stays on your host folder, so **F5** copies in either direction, while **F3** views,
**F7** creates a folder, and **F8** deletes inside the container. Search, sizes, previews, and
editing only work on the host. **Alt-C** goes back to the Docker view, and `..` at the
container's root returns the panel to the host folder it showed before. Browsing needs `sh` in
the container, so minimal images without a shell cannot be listed.

While a panel shows a container's files, the command line follows it: it becomes that
container's shell, started in the panel's folder, with the container's name at the start of the
prompt. Commands run inside the container, the panel refreshes when they finish, and `cd` there
moves the panel. Ctrl-O shows that container's console, marked **▣ name console**. On a host
panel, the command line and Ctrl-O belong to the host again, and whatever you had typed there is
still there. This is the same shell as the container's Shell tab in the Docker view.

To keep the command line on the host, turn off **Follow** in
Options → Plugins. Then, while a panel shows a container's files, the command line is dimmed
and marked **⌂ host**, and Enter does nothing until a host panel is active, so a command never
runs on the wrong machine.

### Container shells

**E** opens a shell in the container (bash if the image has it, otherwise `sh`) on the Shell tab
of the details panel, and sends your keys to it, function keys included. **Alt-1** shows the
overview, **Alt-C** returns to the list, and the mouse wheel scrolls back through the shell's
output. A shell keeps running while you look at other containers, close the Docker view, or use
the host console; its container is marked `›_` in the list and counted on the Alt-C shortcut. The
shell closes when you exit it, when its container stops, or when Borg quits (Borg asks first
while shells are open).

The plugin uses the `docker` CLI and its current context, so it works with Docker Desktop,
OrbStack, Colima, and remote engines alike. You can turn it off in Options → Plugins.

## Time Machine exclusions

On macOS, **Alt-T** or **Command → Time Machine** shows what Time Machine leaves out of backups
and lets you change it. Panels mark items that are excluded themselves with ⊘, and the footer
counts them, or says that the whole folder is not backed up. **Ctrl-L** Info shows how Time
Machine treats the item under the cursor.

The window has three tabs; press **1**, **2**, **3**, or **Tab**, or click a tab, to switch.
**Selection** lists the marked items, or the one under the cursor. **Caches here** searches the
panel's folder for cache and build folders such as `node_modules`, `target`, `.venv`, and folders
with a `CACHEDIR.TAG`, largest first, and adds up how much of them is still backed up.
**All exclusions** lists every exclusion Borg can find, by kind: marked items that Spotlight knows
about and those in the panel's folder, fixed paths, excluded volumes, and the places macOS always
leaves out.

**Space**, **Insert**, or a right-click marks rows, and `*` inverts the marks. **E** excludes the
marked rows, or the one under the cursor, by marking the item itself: this needs no password and
the exclusion moves with the item, but a tool that deletes and recreates the folder loses it.
**P** excludes the path instead, which stays excluded whatever is there, and asks for your
password in the system dialog. **I**, **Delete**, or **F8** includes items again. To back up an
item inside an excluded folder again, include the folder. What macOS leaves out by itself cannot
be changed.

On All exclusions, **A** excludes a path you type, even one where nothing exists yet. **R**
searches again, **Enter** or a second click on a row shows the item in the active panel, and
**Esc** closes the window. Excluding or including a volume also asks for your password. To report
how it treats some folders, Time Machine may need your terminal to have Full Disk Access in
System Settings › Privacy & Security; until then, those folders show as Unknown. On other systems
the menu item is hidden.

## Claude and other agents (MCP)

`borg mcp` lets an AI agent such as Claude work in the borg you have open, either on your own
machine or on a server you reach over SSH. You do not need to install anything besides borg: the
agent starts `borg mcp` itself (through SSH for a server), and it connects to the borg session
running there. The server needs no open ports and no internet access.

### Connecting Claude

Press F1 and then C (or choose F9 → Remote → Share this Borg as host…). Borg shows two commands
to paste into a terminal on your computer: one starts Claude Code already connected to this borg,
the other adds the connection to Claude Code for every session. Tab chooses one, and Enter copies
it to your computer's clipboard through the terminal. That works over SSH in iTerm2, Ghostty,
kitty, WezTerm, and Alacritty (in tmux, add `set -g set-clipboard on`). Where the terminal does
not accept it, as in many browser terminals, F3 shows the command on its own as plain text so you
can select it with the mouse; any key returns to borg.

Over SSH, borg fills in the address you connected to. If your computer reaches the server under
another name, such as an SSH alias or through a jump host, type that name instead. SSH must log
in without asking for a password, using a key or an SSH agent. `borg mcp setup prod` prints the
same setup for Claude Code and Claude Desktop from the command line.

### What the agent can do

Once connected, the agent sees what you see: the screen itself as text, whether you are in the
panels or the console, both panels, the highlighted and marked files, where the command line runs,
and what is typed on it. It refers to places as locations: an absolute path or `~/…` on the
machine borg runs on, `docker://web/etc/nginx` inside a container, or a path relative to the
active panel. Without asking, it can read folders and files (by line range, or only the lines that
contain some text), see the busiest processes and the containers, and show a file or folder in
your other panel.

Anything that changes the machine waits for you in a dialog that shows the whole change: a
command and where it will run, a new file, or an edit as a diff. Scroll long ones with the arrow
keys, PgUp and PgDn, or the mouse wheel, and press F3 to see the dialog full screen. No stays
selected until you choose Yes, and an edit is refused if the file changed while you were deciding.
For a paired computer, `approval_mode` asks you once before turning on Approve all for that
session. While it is on, a badge stays visible; click it or press Ctrl+Alt+A to turn it off at
once. It also turns off when that paired connection closes. Pairing approvals are separate.

Commands are typed into a shell you can watch, and kept out of its history. Commands for this
machine go into borg's own console, which is shown while they run so you can answer a prompt such
as sudo's; commands for a container go into that container's shell in the Docker view. The shell
keeps its folder and variables between commands.

The agent can also type text for you to review: a command at the prompt, or a script into vim or
nano open in the console. Nothing runs: Enter is never pressed, and text with several lines goes
in as a paste, which shells and editors insert without running it.

The agent can also search a process's memory the same way the memory search does, for strings,
indicators, text, or bytes. Memory can hold passwords, keys, and private messages, and whatever
the search finds goes to the agent, so the first search of each process waits for your approval
in a dialog that names the process, its program, and the search. Once you approve, the agent can
search that process again until borg quits. It can ask to read as administrator, which opens the
system's own password prompt.

While an agent is connected, **◈ Remote** appears at the start of the shortcut bar. Click it to
see a log of what the agent did: what it looked at, read, showed, and typed, and every command and
change, approved or declined. Borg keeps the log in memory only, until it quits; to keep no log
at all, turn it off in Options → Plugins.

### Borg cloud

Borg cloud lets Claude reach a borg it has no SSH route to: a server you open in a browser
terminal, a container, or a machine behind NAT. Both sides connect out to a relay, and the relay
joins them.

1. In borg on the server, press F1 then C. The first option is `borg pair` with a link; Enter
   copies it. The link works once, for ten minutes. Below it is the relay: to use a different
   relay, paste its address there and press Enter to save it and get a link through it.
2. Run the command in a terminal on your computer, where borg is installed too. It prints a
   pairing code: six emoji, each with its name, such as `🐱 Cat · 🦁 Lion · 🐎 Horse`.
3. Borg on the server asks whether to pair, showing your computer's name and the code. Choose
   Yes if the codes match. `borg pair` then offers to add the server to Claude Code, as
   `claude mcp add borg-NAME -- borg mcp remote NAME`.

The code comes from the encrypted handshake the two borgs have just completed, so it is the same
on both screens only if your computer is the one talking to the server. Someone who got hold of
the link and used it first would show the server a different code, and cannot choose one that
matches yours. If your terminal draws emoji narrower than borg expects, borg shows only the names;
compare the names instead.

From then on, Claude can reach that borg whenever it is running there. `borg pair` on its own
lists the paired servers, and `borg pair --forget NAME` forgets one; on the server, F8 in the
Remote access dialog forgets every paired computer. Through borg cloud, Claude also gets
`download` and `upload`, which copy files between the server and your computer;
`borg get NAME:PATH [TO]` and `borg put FROM NAME:PATH` do the same from your terminal. Uploads
wait for your approval in borg, and every approval names the computer that asked.

The connection is encrypted end to end. Each borg keeps a private key, readable only by you, in
its state folder. Pairing exchanges the public keys, and every connection opens with a Noise IK
handshake bound to them, so the relay only ever forwards ciphertext and cannot answer in either
borg's name. Calls from another computer can read without asking, just like local ones; turn on
"ask before another computer reads" in Options → Plugins to make those reads wait for approval
too. Borg keeps no connection to the relay unless it is paired or showing a pairing link, and
none at all when borg cloud is turned off in Options → Plugins.

Borg saves the relay in the config as `relay = "wss://relay.example.com"`. A computer reaches the
server through the relay it paired through, so after the server switches relays, pair the
computer again. All borgs on one machine share a key, so only the first one opened connects to
the relay; the others tell you which one has borg cloud.

### Named sessions and the background service

With named workspaces, `borg sessions list` shows their IDs and names,
`borg sessions create NAME` adds one, and `borg --session NAME` opens it. Over MCP, the agent can
call `list_sessions`, `list_remote_sessions`, and `use_session` to choose which session that
connection works with. A single tool call can also pass a `session` ID to target another session
just once. The chosen session applies only to that MCP connection.

`borg service install`, `enable`, and `start` set up the optional background service. It holds
the machine's pairing connection and keeps named sessions running after their terminals close.
`borg service attach NAME` reconnects to one, and Ctrl-] detaches again without ending its
shell. Use `borg service stop`, `disable`, or `uninstall` to stop or remove the service.
In a container without systemd, have the container's process supervisor run `borg service run`;
the `install` and `enable` commands need a Linux host with systemd.

### Remote panels

After pairing, `borg --remote MACHINE:SESSION` opens that session's files in the left panel.
Enter or F3 previews a remote file by downloading a temporary local copy. **F5** copies marked
files and folders between local and remote panels, or between two paired hosts, and the
destination dialog says which client or host each side is. Copies run as background jobs with
byte progress, pause and cancel, and a choice to overwrite, skip, or keep both. Folders are copied
with their empty subfolders and symlinks, and regular files keep their executable permission and
modification time. A file only appears at its destination once all of its contents have arrived.
F6 (move) and F8 (delete) are not available in remote panels.

The right panel stays on your computer. With **Options → Plugins → Follow** turned on, the
command line and **Ctrl-O** use an interactive shell on the remote host. Its terminal is drawn
locally, while keys, pastes, Ctrl-C, and terminal programs run on the remote host. Each remote
session keeps its own shell and unfinished command line while you switch panels, and selecting a
client panel brings back the local shell.

Full-screen programs, and programs that read raw keyboard input such as `top`, receive every key
while they run, Ctrl-O included, and resizing the terminal resizes the remote PTY. Quit such a
program with its own keys (`q` in `top`). If you started it from the panel command line, Borg
then returns to the panels; if you started it in the remote console, the console stays open. The
remote Borg's own panels and shell are separate from this console. To transfer files from the
command line, use `borg get MACHINE/SESSION:PATH [TO]` and `borg put FROM MACHINE/SESSION:PATH`.

Typing `exit` in Borg's main Bash, Zsh, or Fish shell asks for confirmation. **No** is selected
by default and keeps the same shell, variables, folder, and mirrors running. In a remote console,
`exit` asks whether to close the shell on the host; after Yes, choose the session again with
Alt-H to get a new one.

Remote folders load in chunks, with a spinner and a count of the entries received so far; moving
to another folder cancels the request.

### Remote hosts

In local Borg, press **Alt-H**, click **Remote hosts** in the shortcut bar, or choose
**F9 → Remote → Browse hosts and sessions…** to see paired machines and their named sessions.
Enter opens the selected session's files in the panel you opened the picker from, replacing what
that panel showed. Press **M** to mirror the session's screen, and **R** to refresh the list.

**Tab** switches between **Hosts** (computers this Borg can reach) and **Clients** (computers
that can reach this Borg). Clients lists the computers paired with this Borg, their public-key
fingerprints, whether they are connected, and how many connections each has. A computer is listed
once even when a mirror uses several connections. Status updates while the picker is open,
including in a session whose connections are held by the background service. Pairing works in
one direction only: an allowed client does not automatically become a host whose files this Borg
can browse. When the connection owner cannot report connection status, paired clients are still
listed, with **Connection status unavailable**. The same Borg can be a host and a client at once.
The Remote menu also offers **Mirror mounted host** and **Share this Borg as host…**.

The picker shows each host's Borg version. Selecting a session shows its OS and architecture,
whether it runs as a background service or in a terminal, request latency, and relay address.
Hosts are checked in parallel, so one unavailable host does not hold up the others. A host that
cannot report its version shows **version unavailable**; the relay's version is not the host's.
Keep Borg up to date on both computers: a host that lacks a remote feature shows a message asking
you to upgrade.

While a mirror connects, the picker shows an animated status, and **Esc** cancels right away.
Each connection step has 10 seconds, and the whole attempt has 30. If the attempt fails, the
picker stays open and says which step failed; **M** tries the selected session again.

In the picker, **F2** renames the selected host for every session on it, and **Shift-F2** renames
the selected session. These names are saved on this client and appear in the picker, panel
titles, and console footer. Leave a name empty to go back to the original. The details still show
the original host and session names. Names are tied to the paired host's key and the session ID;
command targets and names on the remote host do not change.

**F8** on an unavailable host forgets its pairing on this client, and **Shift-F8** forgets the
selected host even when it is online. Confirmation defaults to No. Forgetting a host closes this
client's panels and consoles for it; the host's sessions and files are kept, and so are this
client's identity and its other pairings. Pair again to reconnect.

**F8** or **Delete** on a host session removes it, after a confirmation that defaults to **No**.
This closes the session and its remote consoles and deletes its saved Borg panels and terminal
history; files and folders on the host are kept. The Default session cannot be removed. A
standalone session that is currently hosting the connection cannot remove itself; first close it
on the host and run Borg in another session. If removal fails, the session stays listed so you
can try again. Sessions managed by the service are stopped before their saved state is removed,
and the service does not recreate them.

### Mirroring a session

`borg mirror MACHINE:SESSION` shows the remote session's live screen in your local terminal;
Alt+Shift+M opens the same mirror from a local Borg with a remote panel. **Ctrl-G** (hold Control
and press G) leaves the mirror and returns to the local panels, as does a click on the banner at
the bottom. The line above it shows who has control; Ctrl-] also exits.

A regular `borg` or `borg --session NAME` can be mirrored while it runs, without the background
service. Its own terminal stays visible, and any keyboard or mouse input there takes control back
from the mirror. The mirror follows the host terminal's layout and is cut off to fit a smaller
window; making your window larger shows the rest. The optional background service keeps sessions
alive after their original terminal closes and lets the controlling client resize their PTY.

If another terminal has control of the remote session, the mirror opens in view-only mode and
your keys do not reach the session. Press **Ctrl-T** or click the status line to take control and
detach the other terminal, or start with `borg mirror MACHINE:SESSION --take`; `--take` always
takes control from another viewer. Once you have control, the first key wakes a sleeping remote
screen, and later keys and mouse input reach it. A viewer that falls behind skips old animation
frames and jumps straight to the current screen.

### Running a relay

`borg-relay` is the relay that borgs meet through. On a Linux server with a domain pointing at it
and port 443 free, one command installs it as a service, with a TLS certificate from Let's
Encrypt:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install-relay.sh | sudo sh -s -- relay.example.com you@example.com
```

Run the same command again to update it.

On Dokploy, Coolify, or another Docker host whose proxy handles TLS, run the image
`ghcr.io/0xcd21h/borg-relay:latest` and give it a domain on port 8080. In Dokploy, create an
application from that Docker image with a domain on container port 8080 and HTTPS turned on; in
Coolify, create a resource from that Docker image with port 8080 exposed and a domain. Both also
accept [packaging/relay/docker-compose.yml](../packaging/relay/docker-compose.yml) as a Compose
service. The image runs `borg-relay --behind-proxy`: the proxy serves HTTPS and passes WebSockets
through, and the relay applies its limits per client address as reported by the proxy in
`X-Forwarded-For`. It runs as an unprivileged user and has a health check (`GET /health`).

On a Docker host without such a proxy, the same image serves TLS itself:

```sh
docker run -d --restart unless-stopped -p 443:443 -v borg-relay:/data ghcr.io/0xcd21h/borg-relay --domain relay.example.com --cache /data
```

Behind another proxy that handles TLS, run `borg-relay --behind-proxy --listen 127.0.0.1:8080`.
Only run it this way behind a proxy, because the relay trusts the client address the proxy
reports. The relay enforces limits per address on servers and sessions, frame size, bandwidth,
bytes per session, idle time, and session length. It stores nothing except its certificate.

The relay logs only counts: once a minute, if anything changed, it writes a line such as

```text
stats: connections=3 servers=2 sessions=1 joined=14 carried=3.2MiB limited=0
```

This shows the connections it holds, the servers registered, and the sessions open, and, since it
started, the sessions joined, the bytes they carried, and the connections and sessions that a
limit refused or ended. For debugging, `--log`, or the environment variable `BORG_RELAY_LOG=1`
for the image, also logs each server's registration by the start of its key, each session with
the bytes it carried, and each limit reached with the client's address.

## Find, jobs, and viewers

Ctrl-S opens quick find. Type a name or a glob such as `*.rs`; Down and Up move between matches,
Insert marks a match, Ctrl-A marks all matches, and Enter or Esc closes the search. Matching
ignores case unless the query contains uppercase letters.

### Copying, moving, and deleting

F5, F6, and F8 start copy, move, and delete jobs. Alt-J opens the list of jobs, where you can
pause or cancel them.

The Copy dialog has an **Optimize copying for hard drives** checkbox. Press Tab to reach it and
Space to toggle it, or click it. It is unchecked for every new copy. In this mode, Borg reads
small files into memory in batches and then writes each batch, which can cut down on disk seeking
when copying within a single hard drive. A batch holds up to 64 MiB of file data or 4,096 files;
files larger than 1 MiB are copied normally. How much it helps depends on the drive, the file
system, and OS caching, and it may be slower when copying between separate drives.
When a file name already exists: `o` overwrites, `s` skips, `k` keeps both, `a` overwrites all,
`n` skips all, and Esc cancels. Copies are written to temporary files and renamed once they are
complete.

### Viewer and quick view

F3 opens the viewer. In text files, the arrow keys and PgUp/PgDn scroll, `w` turns line wrapping
on or off, and Esc or `q` closes it. In images, Left/Right or `p`/`n` show the previous or next
image. Borg uses the terminal's image protocol when there is one; other terminals show the image
as colored blocks.

Ctrl-Q shows a quick preview in the other panel. Tab moves into it, where the arrow keys,
PgUp/PgDn, Home/End, and the mouse wheel scroll; Tab or Esc goes back to the file list. Ctrl-L
shows file information. F3 opens a text preview in the full viewer, and closing the viewer
returns to the preview with your position, wrapping, and source mode kept. Both read files the
same limited way, so opening a large text file reads only around the first page and stays
fast however big the file is.

Both text viewers highlight common programming languages, markup, and configuration files,
including Rust, Python, JavaScript, TypeScript/TSX, Go, C/C++, Java, C#, Ruby, PHP, Lua, SQL,
HTML/CSS, JSON, YAML, TOML, shell scripts, and Dockerfiles. Markdown opens formatted by default;
press `s` or F4 in either viewer to switch to the source. Headings, emphasis, lists, task lists,
links, tables, and fenced code blocks are supported. Markdown files larger than 2 MiB open as
source to keep memory use down.

JSON opens laid out with one value per line, with its keys in their original order and its numbers
exactly as written, which helps most with files written on a single line. Press `s` or F4 to see
the file as written. JSON Lines files (`.jsonl`, `.ndjson`) are laid out one record at a time.
JSON larger than 16 MiB, or JSON that does not parse, opens as written.

### Editor

F4 opens the selected file in Borg's editor, and Shift-F4 opens it in `$VISUAL` or `$EDITOR`. The
editor keeps the file's line endings and indentation, highlights code like the viewers do, and
shows line numbers. Shift with the arrow keys selects text; Ctrl-C, Ctrl-X, and Ctrl-V copy, cut,
and paste through Borg's clipboard, and text pasted from the terminal goes in exactly as it is.
Ctrl-Z and Ctrl-Y undo and redo, and Tab and Shift-Tab indent and unindent. F2 or Ctrl-S saves
through a temporary file, so a file is never left half written. Files that are not UTF-8 text, or
are larger than 32 MiB, open in the viewer instead. Lines longer than 20,000 characters, such as
minified code, are not highlighted, and only the part of a line that is on screen is drawn, so
even a large file written on a single line edits without delay. Alt-Shift-F lays out a JSON file
with one value per line, using the file's own indentation, in one step that Ctrl-Z undoes; JSON
that does not parse is left as it is, and the status line says where parsing stopped.

The editor asks its questions in dialogs over the text: Tab moves between fields and buttons,
Enter confirms the selected one, Esc cancels, and you can click any of them. Ctrl-F or F7 finds
text with the find bar described below. Ctrl-R or F4 replaces, with the same switches as the find
bar: Enter replaces the match and moves to the next one, **Skip** leaves it, and Alt-A or **All**
replaces every match in one step that a single Ctrl-Z undoes. With a regular expression, `$1` in
the replacement puts back what its first group matched. Ctrl-G goes to a line. Esc closes the
editor, and asks first whether to save or discard unsaved changes; if the file changed on disk
since you opened it, Borg asks whether to overwrite it or reload it. A click places the cursor,
dragging or Shift-clicking selects, a double-click selects a word, and the wheel scrolls; the keys
shown in the status line can be clicked too.

Alt-E opens the selected file in Borg's editor inside the active panel, in place of its files, so
you can edit two files side by side, or a file next to the folder it is in. F6 moves from a
panel's editor to the other panel, and Tab goes back to the editor from the file list. Text copied
with Ctrl-C in one editor pastes with Ctrl-V in the other. While an editor has focus, the shortcut
bar shows the editor's keys, and the panel keys come back with the file list. Ctrl-O shows the
console from an editor too, and F9 opens the menus. Esc or F10 closes the editor and gives the
panel back its files; Quit first asks about each panel editor with unsaved changes. Quick view and
file information need the other panel, so they wait until its editor is closed.

The viewer and the editor find text the same way. In the viewer, `/`, F7, or Ctrl-F opens a find
bar in the status line; in the editor, Ctrl-F or F7 does. Borg searches as you type, starting from
where the bar opened: every match on the screen is marked, the current one stands out, and the bar
counts them (3 of 17). Enter, Down, or F3 goes to the next match, and Shift-Enter, Up, or Shift-F3
to the previous one; after the last match, the search continues from the other end and says so.
Alt-C matches case exactly (otherwise a lowercase query matches any case), Alt-W matches whole
words, and Alt-R reads the query as a regular expression; you can click the switches too. Ctrl-P
and Ctrl-N bring back recent queries. Esc closes the bar; after that, `n` and `N` in the viewer,
or F3 and Shift-F3 in the editor, step through the matches. In a large file, the viewer searches
in the background, shows how far it has got, and stops when you press Esc. It counts matches in
files up to 64 MiB, up to 10,000 of them.

## Configuration

Change the interface language in **F9 → Options → Language**. Choose English, Simplified
Chinese, Portuguese (Portugal), Spanish, or Russian, then press Enter; Esc cancels. The choice is
saved in `config.toml` as `language = "en"`, `"zh-CN"`, `"pt"`, `"es"`, or `"ru"`.
`BORG_LANGUAGE` overrides the saved setting. If neither is set, Borg follows `LC_ALL`,
`LC_MESSAGES`, or `LANG`, and uses English for languages it does not support. Commands, paths,
file names, and keyboard shortcuts are never translated. The language menu on this website
changes the guide's language separately.

Settings are saved in `~/.config/borg-commander/config.toml`, or in the folder set by
`$XDG_CONFIG_HOME` or `$BORG_CONFIG_DIR`. F9 → Options → Settings shows them as checkboxes; Space
or a click toggles a setting and saves it right away. Esc closes the window.

```toml
theme = "neon"              # midnight, aurora, neon, classic, paper, ember, or your own
icon-style = "auto"         # auto, nerd, or portable; Ghostty gets rich icons automatically
icon-spacing = 1            # spaces after an icon, 1 to 3
status-line = true
menu-bar = false            # keep the menu bar visible
show-hidden = false
return-to-panels = true    # return after commands launched from the panels
restore-terminal = false    # bring back the shell scrollback on the next start
screensaver = true          # a spinning cube or a glitching logo, picked at random
screensaver-after = 300     # seconds of idle time
update-check = true         # look for a new release once a day
graphics = "auto"           # auto, kitty, sixel, iterm or off
relay = "wss://relay.example.com"  # the relay for borg cloud

[plugins]
git = true
system = true
docker = true
docker-console = true       # the command line follows container panels into the container
agent-log = true            # log what Claude does while Borg runs
agent-cloud = true          # paired computers reach this borg through borg cloud
agent-ask-reads = false     # reads from another computer wait for approval

[search]
exclude = ["*.min.js", "fixtures/large"]  # paths below the search folder, as in .gitignore
artifacts = true            # leave out node_modules, Cargo's target and cache-tagged folders
ignore-files = true         # follow .gitignore, .ignore, .rgignore and .borgignore
history = true              # remember searches for Ctrl-P and Ctrl-N

[index]
roots = ["~/Projects"]      # folders indexed for instant search
refresh-minutes = 15        # rebuild an index once it is this old
```

In a local terminal with true color, Borg starts with a short glitch animation of its logo; any
key or click skips it. Over SSH, and in terminals without true color, it opens straight to the
panels.

Panel folders, cursor positions, the split, listing modes, and sorting are saved in
`~/.local/state/borg-commander`, or in the folder set by `$XDG_STATE_HOME` or `$BORG_STATE_DIR`.

While it runs, Borg keeps its shell integration scripts and files copied out of containers in
`$XDG_RUNTIME_DIR/borg-commander`, or in `borg-commander-<uid>` in the temporary folder. Only you
can open this folder; if one already exists that someone else created, or that other users can
enter, Borg skips it and uses a new name. Its `TTL` file starts with the time until which the
folder is in use: every running borg pushes that time a day ahead each hour, so once it has
passed, the folder can be deleted.

Custom themes go in `~/.config/borg-commander/themes/`. A theme can extend a built-in one:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

To build, test, or contribute, see [Development](../DEVELOPMENT.md).

## A small distraction

Open About with F1 and type `2048` to play in a window. The arrow keys, or a click toward a side
of the board, move the tiles; U undoes the last move, R starts a new game, and Esc closes it.
When you reach 2048, press Enter to keep playing. The board and your best score are kept between
sessions.

## Activity and memory

**Alt-M** or **Command → Activity monitor** opens the process monitor. It comes from the bundled
System plugin, which is on by default and can be turned off in Options → Plugins. The process list
takes the active pane, and the other pane shows a memory overview of the selected process.
**Enter** or a double-click opens that process in a window over the monitor, with its memory map
on one side and a preview of the selected region's bytes on the other. **Esc** closes it, and
**Tab** switches between the sides. **Alt-W** or the **[+]** button maximizes the focused pane,
**[=]** restores the split, and **[-]** hides the monitor. The monitor samples once a second while
it is open and stops when you close it. Sampling pauses while a memory inspection runs and
resumes when the result arrives.

The list shows CPU, resident memory, memory growth, and disk read and write rates. CPU 100% means
one fully busy core. Resident pages shared between processes count toward each of them, so adding
up the RAM of all processes does not give the system total. CPU and disk rates appear from the
second sample on.

| Key | Action |
| --- | --- |
| Enter | Open the process's memory map / move to the region preview |
| `/` | Filter by name or PID; ↑↓ still move, Enter keeps the filter, Esc clears it |
| C / M / G / N / P | Sort by CPU / RAM / growth / name / PID; press again to reverse |
| I / O | Sort by read / write rate |
| T | Show or hide the process tree |
| Space | Pause or resume sampling |
| R | Refresh the current view |
| Esc | Go back; in the process list, close the monitor |

Click a column heading to sort, and double-click a process to open its memory map. The overview
shows current resident memory, the highest value seen, the change since monitoring started, CPU,
virtual address space, and a chart of resident memory over the last 60 seconds. Footprint,
swapped, private, and shared totals appear once the memory map has been read, if the system
reports them. **Enter** in the memory pane opens the memory map. The memory map is read when you
ask for it; press R to read it again after the process changes its mappings.

### Memory map

The map has three columns:

- **Footprint** is the memory charged to this process alone: pages it has written, plus pages
  swapped out or compressed. It is the number to watch for leaks, and on macOS it matches the
  *Memory* column in Activity Monitor.
- **Resident** is the pages in RAM right now, including pages shared with other processes.
- **Mapped** is address space, including reservations. It says nothing about RAM use.

A process can map gigabytes, mostly system libraries shared with every other process, while its
footprint is only a few megabytes. Regions start out grouped by purpose: heap and allocations,
thread stacks, the program's own code and data, shared libraries, mapped files, shared memory,
system and other allocations, and reserved ranges and guard pages. Groups and the regions in them
are ordered by footprint, with reservations last. Libraries and mapped files get one row per
file; expand a file to see its regions. Group names are worked out from the labels the operating
system gives. Selecting a group or file explains what it is and shows its share of the footprint
and its largest members. Enter or Right expands a group or file; Left collapses it, or the one
that contains the selected row. G switches between the grouped and the raw list.

### Open files

**1 Memory** and **2 Open files** switch between the inspection tabs; you can also click them.
The open files list shows each descriptor, its access mode, its type, and its path or network
endpoint; the other pane shows full details, the file size, and the current offset when they are
available. On macOS it also includes entries such as `cwd` (current directory) and `txt` (mapped
executable). R refreshes the list. Each tab keeps its position in the list when you switch. When
access is restricted, Borg offers to try again as administrator. For a regular file, the details
also preview its contents, as the quick view does: **Tab** moves into the preview so you can
scroll it, and **F3** opens the file in the viewer. Borg opens the file by its path with your own
permissions and leaves the process's descriptor alone.

### Region preview

When you select a readable region, Borg captures up to 64 KiB of it in the other pane. If the
start of the region holds only zeros, the capture skips ahead to the first page with data, and
the line above the bytes says where in the region the window is. When you move quickly, Borg
waits a moment before reading; selecting another region clears the old preview and cancels its
pending read. Regions that cannot be read show a message instead. Press **Tab** or **Enter** to
scroll the preview. **Esc** there goes back to the list, and **Esc** in the map goes back to the
process list with its cursor and filter as they were.

Runs of zero rows collapse into a single row, such as "· 60K zero bytes ·". **]** and **[** jump
to the next and previous block of data, and **>** and **<** capture the next and previous 64 KiB
of the region. **H** switches between hex and text, **R** captures the shown window again, and
**D** highlights what changed since the previous capture of the same process and address. Text
view shows printable ASCII and replaces other bytes with dots. Captures are taken from the live
process without pausing it, so a capture is not a consistent image of the whole process. They
stay in memory unless you export them. **E** saves the raw bytes to a new file in the active file
panel's folder, readable only by you. Existing files are never overwritten.

**M** switches the preview between bytes and a page map of the region: one cell per 4 KiB page,
or per run of pages in a large region, marked by what it holds: pointers into the process's own
memory, text, UTF-16 text, code, compressed or encrypted data, other data, filler, zeros, and
pages not in memory. A legend shows each kind's share of the region. When the map is focused, the
arrow keys, **PgUp**, **PgDn**, **Home**, and **End** move a cursor, and **Enter** opens the page
under it as bytes. Regions larger than 16,384 pages are sampled evenly. On Linux, the map asks the
system which pages are in memory and does not read the rest, so it neither brings swapped pages
back nor touches new ones; on macOS, pages that were never touched read as zeros. Kinds are
guessed from the bytes alone, so treat them as a hint.

### Memory search

Press **/** in the memory pane, or **Ctrl-F** in either pane, to search memory. Choose a mode,
whether case must match, and whether to search the process's readable memory or the current
capture. **Text** searches UTF-8, UTF-16 (both byte orders), and UTF-32 (both byte orders).
**Unicode** also normalizes compatibility characters and combining sequences: for example,
`𝐁𝐨𝐫𝐠` matches `Borg`. It does not treat different letters as equal just because they look
alike. **Hex** takes byte pairs such as `42 6F 72 67`.

**Strings** and **Indicators** only read memory the process filled in itself: memory not backed
by a file and that the process may write to; on macOS, regions it never wrote to are skipped too.
**Strings** lists each run of at least six printable ASCII or UTF-16LE characters once, with the
number of copies in memory, and skips filler such as repeated characters or punctuation; if you
give a query, it keeps only the strings that contain it. **Indicators** keeps the strings that
look like a URL, an IP address, an email address, a file path, a command line, a secret (a
private key, an access token, or a password assignment), or a long run of base64, and labels
each one.

Borg also reads the strings in the files the process has mapped, up to 1 GiB, and marks with •
the strings found only in memory: text the process built, received, or decoded, which none of
its files contain. Indicators come first, then strings found only in memory, then those its
files also hold, and strings that look like words come before the rest. The list keeps the best
2,000. Where other matches show their region, each row shows the kind and the number of copies,
and the preview shows the string in place; **C** steps through its copies. Treat these results as
leads worth a closer look. Ordinary programs hold plenty of URLs, paths, and keys too.

Matches take the list pane and show their address, encoding, and region. Selecting a match
previews the bytes around it in the other pane, with the match highlighted. Enter moves into the
preview, and Esc goes back to the matches. Esc in the matches returns to the memory map or
process list you came from. Searching a saved capture uses its bytes without reading the running
process. Process searches read memory in chunks, report bytes they could not read, and stop
after 2,000 matches or 60 seconds. **N** continues from where the search stopped, and **Ctrl-C**
stops it and keeps the results. The coverage report separates searched bytes from unreadable
ones. A process can change its memory while it is being searched.

### Code integrity check

**I**, in the memory map or among search matches, checks whether the process is running the code
its files contain. It compares the code in memory with the files it was mapped from and reports
pages that differ, searches memory not backed by a file for the headers of executable images
(ELF, Mach-O, and PE), and lists executable memory not backed by a file, memory that is both
writable and executable, and code from deleted, nameless, or temporary files. It also checks the
process as a whole: libraries injected through `LD_PRELOAD`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`,
or `/etc/ld.so.preload`, library search paths, a tracer on Linux, a program whose file has been
deleted or replaced, and a process name that differs from its program's.

Findings take the list pane, with alerts first, then warnings, then notices. Selecting one
previews the bytes it is about, with the evidence highlighted, and the rows below the list say
what else could explain it. Programs that compile code while they run, such as browsers and Java
and JavaScript engines, always hold code that no file backs, so for them those findings are only
notices. **R** checks again, **Ctrl-C** stops a check and keeps what it found so far, and **Esc**
goes back. A check reads at most 512 MiB of code and 1 GiB of other memory, for up to 45 seconds,
and says if it stopped early.

On Linux, the page tables show which code pages a process has changed, so only those are read;
on macOS, regions without modified pages are not read either. Code in the macOS shared cache has
no file of its own and is not compared. Memory a check cannot read is counted, and Borg offers
**A: Administrator**. Treat findings as leads, because debuggers, profilers, updaters, and
security tools leave the same traces.

### Administrator access

When reading a memory map or a capture is restricted, or a search could not read part of the
memory, Borg offers **A: Administrator**. On macOS this opens the system's authorization prompt;
on Linux it uses `pkexec` and needs an authorization agent to be running. Only a short-lived
inspection helper runs with elevated rights. Borg itself keeps running normally, and the helper
only returns map data or a limited number of bytes.

macOS shields the memory of its own programs (System Integrity Protection) and of programs built
with the Hardened Runtime from every other program, administrators included. Borg reads this from
the program's signature: the process details say **Memory protected**, a refused read explains
why, and **A: Administrator** is not offered, since it cannot help. Their memory maps and open
files are still shown. It is not offered either when Borg already runs as administrator. On
Linux, security policy can still deny access; Borg shows the error and the process monitor keeps
working as usual.

A process is identified together with its start time, so a new process that reuses a PID does
not inherit another process's history. Memory maps, inspection results, captures, and history
all have size limits. The Borg Commander name stays visible in the bottom-right corner
everywhere, including the console, viewer, monitor, dialogs, and screensaver.
