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

On Linux, the build also compiles the process watcher's eBPF programs, which need a nightly
compiler and `bpf-linker`. When they are missing, the build stops and prints the two commands that
install them. To build without the programs, set `BORG_NO_EBPF=1`. The watcher then hears the
kernel's process events instead:

```sh
BORG_NO_EBPF=1 cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

Homebrew support is ready and will work once the tap is published:

```sh
brew install 0xcd21h/tap/borg-commander
```

Run `borg` in the folder you want to browse. F10 quits, and Ctrl-O switches from the console
back to the panels. To update, run `borg update` for a downloaded copy, or use Homebrew or cargo
if you installed it that way. Borg installs an update only when the release carries the project's
signature. If the signature is missing or wrong, the copy you have stays as it is.
`borg --version` prints the installed version. Open sessions move to
the new version and keep their shells (see [Updating with sessions open](#updating-with-sessions-open)).

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
with the find bar open on the query, so Enter goes on to the next one, and Ctrl-Q shows it there in
the quick view with the query marked. Markdown and JSON files open as source in both, since the
search read the file as it is, and `s` switches to the formatted view. F4 opens the file in Borg's
editor at that match, with the match selected and the find bar open on the query, and Alt-E does the
same in the other panel, so the results stay in view. Borg reads the files in the background, and
the context line counts the files read. Binary files (a NUL byte in the first 8 KiB) and files over
64 MiB stay unread and are counted as binary or too large. Ctrl-C stops the search, and it stops at
20,000 files like any search.

### Scanning files with rules

Set **Look for** to **Rule matches** in the Ctrl-F dialog to scan the files below the folder with
YARA rules: those Borg ships and your own. The name pattern, **Minimum size** and **Leave out build
output, caches and ignored files** still apply. The rules decide what to look for, so the dialog
drops **Containing** and **Minimum files**.

Each result shows in the **Match** column the worst rule the file matched, how many more it
matched, and the first bytes that rule found, as quoted text or as hex. F3 opens the file in the
viewer at those bytes with the find bar on them, and F4 and the quick view land there too. The
context line counts the files scanned and the rules that ran. Files over 64 MiB stay unread and
count as too large, and the scan stops at 20,000 files like any search. Ctrl-C stops it.

Borg's own rules look for:

- `reverse_shell`: an interactive shell over `/dev/tcp`, `nc` or `socat` running a shell, or a
  shell spawned on a pseudo-terminal.
- `download_and_run`: a script downloaded and piped straight into a shell.
- `decode_and_run`: base64 decoded and run as a shell command or as Python code.
- `crypto_miner`: a mining pool address, or the settings of a common miner.
- `upx_packed`: an executable packed with UPX, which unpacks itself in memory.
- `execve_shellcode`: the shell path exploit code builds before it starts a shell.
- `ssh_key_planted`: a public key appended to an `authorized_keys` file.
- `reboot_job_fetch`: a cron job that runs at every boot and downloads, decodes or runs something
  from a temporary folder.
- `password_prompt`: an AppleScript dialog that asks for a password with the answer hidden, as
  macOS stealers do.
- `launch_agent_from_temp`: a macOS launch agent or daemon that starts a program from a temporary
  or shared folder.
- `preload_write`: a write to `/etc/ld.so.preload`, which loads a library into every program on
  Linux.
- `history_wiping`: shell history turned off, or system logs emptied.
- `agent_unattended`: an AI coding agent started with its approvals turned off, as the Nx
  supply-chain attack did to search machines for secrets.
- `hidden_prompt`: a run of invisible Unicode tag characters, which can hide instructions for AI
  models in prompts, rule files and web pages.
- `pickle_runs_code`: a pickle, the format Python and PyTorch save models in, that runs a command
  when it is loaded.

Put your own rules in the `rules` folder beside `config.toml`: `~/.config/borg-commander/rules`,
or the same folder under `$XDG_CONFIG_HOME` or `$BORG_CONFIG_DIR`. Borg reads every `.yar` and
`.yara` file there and in the folders below, each in a namespace of its own, so two files can
use the same rule names. Each file compiles by itself. A file with an error is left out whole,
the status line names it with the compiler's message, the context line counts the files left
out, and the other files still run. Since every file loads on its own, a file that uses `include`
is left out too, and the files it includes run by themselves.

Rules can import the `hash`, `math`, `string`, `time`, `elf`, `macho`, `pe`, `dotnet` and `dex`
modules; a file that imports `magic`, `cuckoo` or `console` is left out. The variables `filename`,
`filepath` and `extension` hold the scanned file's name, path and extension, as rule sets written
for THOR and LOKI expect, and `filetype` and `owner` are empty. A rule's `severity` meta sets how
much a match matters: `alert`, `critical` or `high` make an alert, `warning`, `medium` or
`suspicious` a warning, and `notice`, `low` or `info` a notice. Without it, a `score` of 80 or
more makes an alert, 60 or more a warning, and less a notice. A rule with neither counts as a
warning. Its `description` and `false_positives` show with each match.

Borg ships no public rule sets, because their licenses differ. Download the ones you trust into
the rules folder, and check each license before you use it at work:

- [YARA Forge](https://yarahq.github.io/) packages tested rules from many public sources; each
  rule keeps its source's license.
- [signature-base](https://github.com/Neo23x0/signature-base) is free for non-commercial use
  (CC BY-NC 4.0).
- [Elastic's protection artifacts](https://github.com/elastic/protections-artifacts) come under
  the Elastic License 2.0.
- [ReversingLabs' rules](https://github.com/reversinglabs/reversinglabs-yara-rules) come under
  the MIT license.

Rules match patterns of bytes and know nothing of intent. Installers, security tools and their
test data match too, and a file that no rule matches can still be malicious. Treat matches as
leads. [Scanning memory with rules](#scanning-memory-with-rules) says what a scan cannot see.

### Finding packed executables

Set **Look for** to **Packed executables** in the Ctrl-F dialog to list the programs below the
folder whose code is as dense as compressed or encrypted bytes. Packers and protectors leave code
like that, and so does malware that hides its code until it runs. Borg reads the parts of each
ELF, Mach-O and PE file that the loader maps executable, every slice of a universal Mach-O binary
included, and measures the entropy of up to 1,024 blocks of 4 KiB spread evenly over them. A
program is listed when at least 70% of those blocks reach 7.2 bits per byte. Code compiled the
usual way stays well below that. The name pattern, **Minimum size** and **Leave out build output,
caches and ignored files** still apply, and the dialog drops **Containing** and **Minimum files**.

The **Match** column gives the share of high blocks and the highest entropy found, such as
"97% high · max 8". F3 opens the program in the viewer's hex rows at the first high block, with
the entropy strip described under [Viewer and quick view](#viewer-and-quick-view) beside them.
The context line counts the programs read and the other files, which hold no code Borg knows how
to find. Ctrl-C stops the search, and it stops at 20,000 files like any search. A packed program
shows its real code only while it runs, and [Entropy scan of every
process](#entropy-scan-of-every-process) looks at programs that way.

### Working with results

Click a column heading to sort by it, and click again to reverse the order. The Left and Right
menus also offer sorting, and the Search menu adds sorting by source folder and grouping by
folder or file type. Enter on a group heading folds the group; Insert marks the matching files
in it. Marks are kept when you sort or filter, and the footer also counts marked files that the
filter hides.

F3 and Ctrl-Q open the original file, and after a search for text they open it at its first match.
The quick view follows the cursor through the results and shows each file at its match, with the
query marked as the find bar marks it. Alt-E opens the file in Borg's editor in the other panel, so
the results stay in view. Esc or F10 closes that editor and gives the focus back to the results, and
the quick view comes back if it showed before. F5 copies to a real folder; if the other panel is a
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
| Ctrl-PgDn / Ctrl-PgUp | Open the folder or archive under the cursor / go to the parent folder |
| Tab | Switch panels, or move into the quick preview and back to the file list |
| Ctrl-O | Show the shell / return to the panels |
| Alt-I | Count folder sizes in the background / stop counting |
| Ctrl-S | Quick find in the folder |
| Ctrl-\ | Folder hotlist: jump to a pinned or recent folder |
| Ctrl-Q | Quick view: preview the file under the cursor (text, picture or folder) in the other panel |
| Ctrl-L | Info: show size, permissions, owner, dates, free space and the drive in the other panel |
| Insert, Ctrl-T, Shift-Up/Down | Mark files |
| `+` `-` `*` | Select, unselect or invert by pattern (`*.rs`, `build/`) |
| Alt-Enter | Put the marked names (or the current one) on the command line; on `..`, the parent folder's path |
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
| Alt-O | Open this panel's folder in the other panel |
| Alt-J | Jobs: pause or cancel running copies, moves and deletes |
| Alt-M | Activity monitor: processes and memory |
| Alt-C | Docker: Compose projects and containers |
| Alt-N | Network: ports, connections and traffic |
| Alt-U | Startup items: what starts on its own, newest change first |
| Alt-L | Alerts: what the process watcher found |
| Alt-T | Time Machine: what backups leave out, on macOS |
| Alt-E | Edit the selected file in this panel, in place of its files, or in the other panel beside results |
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

Going back to the parent folder puts the cursor on the folder you came from, on the same screen
row, with the list scrolled as it was when you went in. If files came or went in the meantime, or
the panel changed height, the cursor still lands on that folder, as close to its old row as the
list allows.

Alt-Enter types names as the shell sees them from the panel's folder. On `..` it types the full path
of the parent folder, because the parent's name alone would point inside the current folder. In
search results and collections it types full paths, since their files come from different folders.

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

Hotkeys work with non-Latin keyboard layouts too: Russian, Ukrainian, Belarusian, Kazakh, Greek,
Hebrew, Arabic, Georgian, Korean, and Thai. A letter of such a layout held with Ctrl or Alt counts
as the key in the same place on a US keyboard, and so does a letter pressed in a screen or dialog
that is not waiting for text. With the Russian layout on, Alt-M
opens the activity monitor and Ctrl-C reaches the shell. The command line, the editor, and Borg's
fields keep the letters you type. Questions with Yes and No buttons answer to Y and N in every
language and to the first letter of each button in Borg's language, such as S for Sí in Spanish.

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

### Jumping to folders

Type `cd ` on the command line and Borg lists folders to jump to right above it: your hotlist first,
marked ★, then the folders the panels opened, newest first. Each word you add narrows the list and
can match anywhere in the path as the list shows it, so `cd pro bo` finds `~/Projects/borg`. Your
home folder shows as `~`, so a word from its own path, such as `me` in `/home/me`, matches no folder
inside it. A word that starts with `/` matches the full path instead. Folders whose name matches the
last word come first, next to the command line. Press Up to step into the list. While the list
shows, Up and Down move only through it and stop at both ends, so the panel's cursor keeps its
place. Enter opens the chosen folder in the active panel. Tab puts it on the command line after
`cd ` without running it, escaping spaces and other characters the shell would split or expand, and
the list moves on to the folders inside it. A click on a folder opens it too. To run your own `cd`,
keep typing and press Enter.

Type a path and the list starts with the folders inside it. `cd /Volumes` lists the mounted volumes,
and `cd ~/Projects/bo` narrows to the folders in `~/Projects` with `bo` in their names, those that
start with it first. Any other command gets the same list for a path with a `/`, such as
`cp notes.txt ~/Documents/`. There Tab or Enter types the chosen folder's name into the path, escaped
or inside your quotes as the shell needs, and the list moves on to the folders inside it. Esc hides
the list and gives Up and Down back to the panel. The list comes back when you start another word or
erase the one you are typing, and Esc right after a space hides it only until you type. Hidden
folders show once you type a dot, or when the panels show hidden files. Borg reads up to 10,000
subfolders of a folder, and only at this machine's command line; a container's or a remote
machine's console keeps the shell's own completion.

A command line that only changes folder runs while the panels stay on screen, and the active
panel follows the shell to its new folder. When the folder does not exist, a notice says so and
Ctrl-O shows the shell's message. A folder that takes a while to open, such as one on a slow
network share, brings up the console until the command finishes. Anything more than a `cd`, such
as `cd src && make`, shows the console as usual.

Ctrl-\ or **Command → Folder hotlist** opens the hotlist with the active panel's folder chosen.
Type to narrow it, choose with Up and Down, and press Enter to open the folder. Insert pins the
chosen folder to the hotlist, so Ctrl-\ followed by Insert pins the folder you are in. F8 or
Delete takes a folder off the hotlist, or out of the recent folders. Esc clears what you typed,
and a second Esc closes the window. The hotlist holds up to 256 folders.

Alt-O or **Command → Same folder in other panel** opens the active panel's folder in the other
panel, so both show the same place. The folder takes the place of the quick view, Info, or a
container's or remote machine's files there. If the other panel showed search results or a
collection, Alt-Shift-R there brings them back. From search results, Alt-O opens the folder the
search started from. A collection has no folder of its own, and container and remote folders open in
one panel only, so in those Alt-O shows a notice, as it does while the other panel holds an editor.
While you type a command, Alt-O goes to the shell.

The hotlist is saved in `config.toml` under `[folders]`. Borg remembers the last 64 folders the
panels opened in its state folder, in a file only you can read. Set `history = false` under
`[folders]` to stop keeping them.

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

CPU shows as two figures. **Now %** is the latest reading, and **Avg %** is the average of the last few
seconds, as in the activity monitor, so it holds steady while a container works in bursts. A list
too narrow for both shows **Avg %** alone. The details show the same pair as **CPU now** and
**CPU avg**, and a project adds up both over its running containers.

| Key | Action |
| --- | --- |
| ↑ ↓, PgUp, PgDn, Home, End | Move |
| ← → / Enter | Fold or unfold a project; Enter on a container moves to its details |
| S / X / R | Start / stop / restart the container, or the project's containers |
| P | Pause or resume |
| F | Browse the container's files |
| E, Alt-2 | Open a shell in the container, or go back to its open shell |
| Alt-1 | Show the container's overview again, also from inside its shell |
| D | Show disk use, or the containers again |
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

### Disk use and cleanup

**D** in the Docker view, or **F9 → Docker → Disk use**, shows what Docker keeps on disk in place
of the container list, one row per kind: **Build cache**, **Dangling images**, **Unused images**,
**Images in use**, **Stopped containers**, **Unused anonymous volumes**, **Unused named volumes**,
**Volumes in use**, and **Unused networks**. Each row has a count, a size, and what removing it
frees, and the top line adds up everything you can reclaim. Borg measures with
`docker system df --verbose` in the background, which can take a while on a busy engine. **R**
measures again, and Borg does the same when the next command at the prompt ends after **F8**
typed one.

Dangling images are builds nothing names any more, usually because their tag moved to a newer
build. Unused images have a name, and no container was made from them. Images and volumes that
containers use, running or stopped, stay until those containers are removed, so their rows free
nothing. An image frees only the layers no other image shares, which is why its reclaimable size
can be far below its size.

→ or Enter unfolds a kind into its items, biggest first, and ← folds it again. The other panel
shows the selected item: an image's tags, ID, shared size, creation time, and container count; a
build cache record's type, whether it is shared or in use, and when a build last used it; a
container's image and status; a volume's facts, which the next paragraphs describe. Sizes Docker
could not measure show as `?`, and `/` filters every kind at once. It also finds volumes by their
Compose project and by the containers and images that use them.

Volume rows fill the room the Name column has to spare with **Last used**, **Created**,
**Project**, and **Used by**, as many as fit, and Alt-W maximizes the pane to show them all.
Project is the Compose project that made the volume. Used by lists the containers that mount it,
running or stopped. Anonymous volumes go by the first 12 characters of their ID.

**Last used** reads **in use** while a container that mounts the volume runs. Otherwise it gives
the day the last of those containers stopped, which Docker records exactly. Once the containers are
removed, Docker keeps no record of the volume's use. Where Borg reads a volume's files directly, it
also looks at when they last changed, and a later change shows as that day with `~` in front. That
day is an estimate: reading a file leaves no trace, so the volume may have been used since. An
empty cell means Borg knows neither.

The other panel shows the full time of the last use and of the creation, the **Compose** project
with the volume's name in it, and the **Driver** when the volume uses a driver other than `local`
or has options. Each container that mounts the volume gets a line with its state and the path it
mounts the volume at. For an anonymous volume, these are the containers it was **Made for**, with
their images. Docker forgets that container once it is removed. **Folder** and **Holds** show where
the files are on this machine and what is at the volume's top.

Enter on a volume shows its files in the panel, read only. The title marks the panel with ▥ and
**read only**, and paths start at the volume's top. **F3** views files, and **F5** copies files out
to the other panel. Borg refuses anything that would change the volume: editing, moving, deleting,
new folders, packing, and copies into it. `..` at the volume's top returns the panel to the folder
it showed before, and the command line stays in that folder all along. A session saved while a
panel shows a volume opens that folder next time.

Borg reads a volume's files directly when they are on this machine. With OrbStack on macOS, they
are in OrbStack's share in `~/OrbStack/docker/volumes`, which macOS treats as a network volume:
allow your terminal app Network Volumes in System Settings › Privacy & Security › Files & Folders.
On Linux, Borg reads the volume's folder when the engine runs on the same machine and Borg may open
it, as with rootless Docker or when Borg runs as root. Search and sizes then work as in any folder.

Elsewhere, such as with Docker Desktop, a remote context, or Docker on Linux where only root reads
its folders, Borg shows the files through a helper container. The helper runs the `busybox` image,
pinned to one build, with no network, a read-only file system, and no privilege beyond reading
files, and it mounts the volume read only. The first time, Borg asks before Docker downloads the
image, about 2 MB, which then stays on the engine as `borg-commander/volume-helper`. The footer
shows **Downloading busybox…** or **Starting the helper container…** until the panel opens. Borg
removes the helper when the panel leaves the volume or when Borg quits, and a helper left behind
stops by itself after a day. Search and sizes only work on the host there, as in a container's
files, and disk use doesn't count the helper among the containers that use the volume.

Once a measurement ends, Borg reads the volumes it reaches directly in the background, biggest
first, and the footer shows **Reading volume files…** until it is done. **R** reads them again as
it measures.

**F8** types the cleanup command for the selected row at the host shell's prompt and shows the
console. Nothing runs until you press Enter, so you can read the command, change it, or clear it
with Ctrl-U first. On a kind, F8 types its prune, such as `docker image prune -a` for unused
images or `docker volume prune` for unused anonymous volumes, and Docker asks for confirmation
before it deletes anything. On an item, F8 types `docker rmi`, `docker rm`, or `docker volume rm`
with that item, or with every marked image, container, or volume. Space or Insert marks an item,
and `*` inverts the marks of the selected kind. An image goes by all its tags, since Docker
deletes it with its last tag, or by its ID when it has none. Build cache records go together with
`docker builder prune`. Borg never adds `-f` or `--force`. A name with a control character gets
no command, since the prompt would take that character as a key.

Volumes hold the data containers saved, such as a database's files, and a removed volume cannot
be brought back. The other panel repeats this beside every volume command. When the `docker` CLI
has more than one context, the commands name the one Borg measured, as in
`docker --context staging image prune`, so they reach the same engine whatever context the shell
would pick. They always go to the host's shell, also while a panel shows a container's files,
because the Docker view keeps the command line on the host.

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
your other panel. It can also list the open ports, the connections, and the network interfaces
with their traffic, measured over one second and narrowed with the same words as the network
screen's filter (see [Filtering](#filtering)). It can list the startup items too, newest change
first, with their programs and the rules they matched, narrowed with the same words as that
screen's filter (see [Startup items](#startup-items)).

Anything that changes the machine waits for you in a dialog that shows the whole change: a
command and where it will run, a new file, or an edit as a diff. Scroll long ones with the arrow
keys, PgUp and PgDn, or the mouse wheel, and press F3 to see the dialog full screen. No stays
selected until you choose Yes, and the dialog ignores Yes for its first half second, so a key or
click meant for something else cannot approve it. An edit is refused if the file changed while
you were deciding.
A command with tabs, escapes or other control characters is refused too, since the dialog cannot show them.
The dialog shows on top of everything, including the editor, the viewer, another dialog and the
screensaver, and it takes every key until you answer. What it covered comes back afterward. If
the agent stops waiting first, the dialog closes and a notice says so.
For a paired computer, `approval_mode` asks you once before turning on Approve all for that
session. While it is on, a badge stays visible; click it or press Ctrl+Alt+A to turn it off at
once. It also turns off when that paired connection closes. Pairing approvals are separate.

A request also reaches your own computer when a remote panel there shows the session it waits in
(see [Remote panels](#remote-panels)). Your borg shows it in the same dialog, titled with the
server's name, over whatever you are doing, and sends your answer back over the encrypted
connection. The server keeps its own dialog open too. The first answer counts, wherever it comes
from, and the other dialogs close with a notice that says where it was answered. A request the
agent withdraws closes everywhere. When another computer controls the session through a mirror,
your dialog shows the request without Yes and No, and Esc closes it on your screen only. While
that session approves everything, your borg shows a badge with the server's name; click it or
press Ctrl+Alt+A to turn Approve all off there at once. Only your keys and clicks answer these
dialogs: the agent's tools, `type` included, cannot press anything in them.

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
search and scan that process again until borg quits. It can ask to read as administrator, which
opens the system's own password prompt.

The agent can also scan with YARA rules and check code integrity, as **Rule matches** in the
Ctrl-F dialog and **Y** and **I** in the memory map do (see
[Scanning files with rules](#scanning-files-with-rules), [Code integrity check](#code-integrity-check)
and [Scanning memory with rules](#scanning-memory-with-rules)). A file or folder on the machine
borg runs on is scanned without asking, the same as a read. A folder scan reads every file below
the folder on the same file system, hidden files included. It leaves files over 64 MiB unread and
stops after 20,000 files, 2,000 matches or 45 seconds. Files in containers are not scanned.

A process is different, because the scan reads its memory. The first scan of each process waits
for you in **Remote wants to scan process memory**, which names the process, its program and the
checks: the integrity check, the rules, or both. One approval covers scans and memory searches of
that process until borg quits, whichever the agent asked for first. Each check stops after 45
seconds, 1 GiB of memory or 2,000 findings. The agent can ask to read as administrator, which
opens the system's own password prompt once for both checks.

The rules are Borg's and those in your rules folder, unless the agent brings its own rule text of
up to 64 KiB. That way it can draft a rule from what it found, try it on a folder or a process,
and refine it. Its text runs alone, without Borg's rules or yours, cannot use `include`, and is
never saved. Text that does not compile goes back to the agent with the compiler's message, before
any dialog. For a process, the dialog shows the agent's rule text below the checks, so you see
what it looks for.

The agent gets each finding, worst first, with its severity, its kind, the rule and where it comes
from, the file and offset or the address and memory region, up to 64 bytes of what matched as text
or hex, and what else could explain it. Then come how much was read, what could not be read,
whether the scan finished, and the rule files left out. Evidence goes to the agent as found, keys
and tokens included, as with the memory search.

The agent can also capture traffic as **R** does on the network screen (see
[Capturing traffic](#capturing-traffic)). It names what to record: a port, one connection, a
process by its PID, or all traffic on the interface. For a process, Borg records the whole
interface and sums up that process's conversations. A capture lasts 1 to 60 seconds, 5 unless the
agent names a length. When the agent names no interface, Borg measures for a second and picks the
one the **Capture traffic** dialog would. Each capture waits for you in a dialog,
**Remote wants to capture traffic**, that shows the traffic, the interface, the duration, how much
of each packet to keep, and the folder. While it runs, the footer shows it, and a notice then says
where Borg saved it: in the active panel's folder or one the agent names, readable only by you.
An agent's capture always runs as you and never asks for an administrator's password, so without
access it ends with **No permission to capture on** and the interface's name.

The agent gets back a summary: the protocols Borg recognized, then each process, host and
connection with its packets and bytes, with host names from the capture's own DNS answers and TLS
server names. Borg watches the sockets while the capture runs, so conversations belong to their
processes even with the network screen closed. The summary leaves out what the packets carry.
Reading a `.pcap`, `.pcapng` or `.cap` file gives the same summary, narrowed with the words of the
traffic viewer's filter, and the agent can ask for one packet by its number to get its layers and
its bytes as a hex dump. Borg remembers the processes of its last 16 agent captures until it
quits. Other files show their conversations under **Unknown process**, unless their packets
carry their process.

While an agent is connected, **◈ Remote** appears at the start of the shortcut bar. Click it, or
choose F9 → Remote → **Remote log…** at any time, to see a log of what the agent did: what it looked
at, read, scanned, showed, and typed, and every command and change, approved or declined. Each
session keeps its log on disk, readable only by you, so you can come back to a session you left
running and see what happened there, even after Borg quit. When the log grows large, Borg drops the
oldest entries. Before an entry is saved, Borg hides the passwords, tokens and keys it recognizes,
and it leaves out what commands printed. A password passed as a plain word looks like any other
word, so Borg cannot hide it. To keep no log at all, turn off **Remote: keep a log of what it does**
in Options → Plugins, which also deletes the saved log.

### Borg cloud

Borg cloud lets Claude reach a borg it has no SSH route to: a server you open in a browser
terminal, a container, or a machine behind NAT. Both sides connect out to a relay, and the relay
joins them.

1. In borg on the server, press F1 then C. The first option is `borg pair` with a link; Enter
   copies it. The link works once, for ten minutes. Below it is the relay: to use a different
   relay, paste its address there and press Enter to save it and get a link through it.
2. Run the command in a terminal on your computer, where borg is installed too. It prints a
   pairing code: six emoji, each with its name, such as `🐱 Cat · 🦁 Lion · 🐎 Horse`.
3. Borg on the server asks whether to pair, showing your computer's name and the code. The
   question comes up at once, even while F3 shows the command as plain text. Choose Yes if the
   codes match. `borg pair` then offers to add the server to Claude Code, as
   `claude mcp add borg-NAME -- borg mcp remote NAME`. It waits 5 minutes for your answer; when it
   gives up or you stop it, the question closes and the link stops working.

The code comes from the encrypted handshake the two borgs have just completed, so it is the same
on both screens only if your computer is the one talking to the server. Someone who got hold of
the link and used it first would show the server a different code, and cannot choose one that
matches yours. If your terminal draws emoji narrower than borg expects, borg shows only the names;
compare the names instead.

From then on, Claude can reach that borg whenever it is running there. `borg pair` on its own
lists the paired servers, and `borg pair --forget NAME` forgets one; on the server, F8 in the
Remote access dialog forgets every paired computer. Through borg cloud, Claude also gets
`download` and `upload`, which copy files between the server and your computer;
`borg get NAME:PATH [TO]` and `borg put FROM NAME:PATH` do the same from your terminal. Claude's
downloads go into your Downloads folder and never replace a file there. Uploads wait for your
approval in borg, which shows the file being sent, and every approval names the computer that asked.

The connection is encrypted end to end. Every borg keeps a server key, which paired computers know
it by, and a computer that reaches servers also keeps a computer key of its own. Both are readable
only by you. The server key is a file in the `link` folder of the state folder. The computer key is
a file there too. On a Mac it lives in your login keychain instead, and on Linux the file is sealed
with a password (see [Computer keys](#computer-keys)). Pairing exchanges the public keys, and every
connection opens with a Noise IK handshake bound to them, so the relay only ever forwards ciphertext
and cannot answer in either borg's name. Calls from another computer can read without asking, just
like local ones; turn on "ask before another computer reads" in Options → Plugins to make those
reads wait for approval too. Borg keeps no connection to the relay unless it is paired or showing a
pairing link, and none at all when borg cloud is turned off in Options → Plugins.

Borg saves the relay in the config as `relay = "wss://relay.example.com"`. A computer reaches the
server through the relay it paired through, so after the server switches relays, pair the computer
again. All borgs on one machine share their keys, and the
[background service](#named-sessions-and-the-background-service) connects them all to the relay.
With the service turned off, only the first borg opened connects, and the others tell you which one
has borg cloud.

#### Computer keys

The computer key proves to each paired server that a connection comes from your computer. When a
server still knows this computer by another key, such as this machine's server key, Borg moves it
to the computer key the next time it opens a session there. Borg proves to the server that it
holds the new key, and the server swaps the key in that computer's entry and keeps its name.
Connections that still use the old key end at their next call. The server shows a notice and
writes both fingerprints to the agent log, and its **Clients** list shows when the key changed and
what it was. A server running an older Borg keeps the old key until it is updated.

On a Mac, Borg keeps the computer key in your login keychain, as an item named **Borg Commander**
that only Borg may read without asking you. The first time a Borg process needs the key, macOS asks
for Touch ID or your password, so even a program that starts `borg` on its own shows itself. After
each Borg update, macOS also asks once whether the new Borg may use the item; choose **Always
Allow**. A computer key file from an earlier Borg moves into the keychain the first time Borg uses
it, and the file is deleted. A Borg that runs without the Mac's screen, such as over SSH or in the
background service, cannot show the prompt and cannot reach servers. When `$BORG_STATE_DIR` names a
state folder other than the usual one, Borg keeps the computer key as a file in that folder's `link`
folder. Borg's macOS releases are signed with the Hardened Runtime, so other programs cannot read
Borg's memory or load code into it.

On Linux, Borg seals the computer key with a password you choose, in the file `computer-key.sealed`
in the `link` folder. Run `borg unlock` in a terminal to choose it: Borg suggests four random words,
which are easy to type and hard to guess, and asks for the password twice. Borg stretches the
password with Argon2id, which takes a few seconds and 1 GiB of memory, or an eighth of the memory on
a machine with less than 8 GiB, so guessing it from a copy of the file is slow. A computer key file
from an earlier Borg is sealed the first time you run `borg unlock`, and the file is deleted; until
then, Borg asks you to run it. When `$BORG_STATE_DIR` names a state folder other than the usual one,
Borg on Linux also keeps the computer key as a plain file, with no password.

Once unlocked, the key stays in your user's kernel keyring until 12 hours pass without Borg using
it, and every use starts the 12 hours again. A restart locks it, and `borg lock` locks it at once.
Where Borg cannot use the kernel keyring, as in many containers and system services, a small Borg
process holds the key instead, on a socket in the `link` folder that only you can open, and it ends
after 12 hours without use. When the key is locked, Borg asks for the password: the panels open the
**Unlock key** dialog, and `borg pair`, `borg get`, `borg put` and `borg mirror` ask in the terminal.
`borg mcp remote`, which Claude starts without a terminal, says to run `borg unlock`.

`borg unlock --ssh-agent` also lets your ssh-agent unlock the key. Borg asks the agent to sign a
fixed challenge with an Ed25519 key, or with an RSA key when it holds none, and seals the key a
second time with that signature. From then on, Borg unlocks the key without asking while that agent
is reachable, and the password keeps working. Borg checks that the agent signs the same way twice,
and refuses a key whose signatures change. Anyone who can use your agent can make the same
signature: when you forward it with `ssh -A`, root on the far machine can use it while you are
connected, and with a copy of the sealed file could unlock this key. Forward the agent only to
machines you trust, or leave this off. `borg unlock --no-ssh-agent` turns it off again.

`borg pair --new-key` gives this computer a new key, and each server takes it the next time Borg
connects to it. Until every server has, Borg keeps the old key too, and `borg pair` marks each
server that still takes it. A week later it also suggests forgetting servers you no longer use,
with `borg pair --forget NAME`.

When a server refuses every key this computer holds, Borg says so. Either someone forgot this
computer on that server, or someone with a copy of its key moved the server to a key of their own
first. A new key is hygiene and does not undo a leak. If you think someone copied a key, press F8
in Remote access on every server to forget the paired computers, pair again, and check those
servers.

### Named sessions and the background service

With named workspaces, `borg sessions list` shows their IDs and names,
`borg sessions create NAME` adds one, and `borg --session NAME` opens it. Over MCP, the agent can
call `list_sessions`, `list_remote_sessions`, and `use_session` to choose which session that
connection works with. A single tool call can also pass a `session` ID to target another session
just once. The chosen session applies only to that MCP connection.

With more than one session, or when the only one is open in another terminal, `borg` starts with the
**Borg sessions** window. Its first lines say what a session is, and the list shows each session
with when it was last used. A session that is active right now is marked with its status:
**Open in another terminal**, or **Kept running by the service** when the background service runs
it. The session used last is chosen, unless another terminal has it open. Enter opens the chosen
session where you left off; one the service keeps opens attached, as with
`borg service attach NAME`. A click chooses a session and a second click opens it. **F7** names a
new session and opens it, and Esc or F10 quits without opening one. While the window is open, it
checks every 2 seconds which sessions are active.

**F8** or **Delete** removes the chosen session after a confirmation that defaults to **No**. This
deletes its saved panels and terminal history; your files and folders are kept. The Default session
always stays, and so does a session that is active right now: quit Borg in the other terminal first.
To remove a session the service keeps, open it with `borg service attach NAME` and quit with F10.

When you name a session with `borg --session NAME` that another Borg has open in a terminal,
`borg` stops and names that Borg: its process number, its terminal as `ps` shows it, and how long
ago it started. Quit that Borg with F10 where it runs. When you can no longer reach its terminal,
end it with `kill` and the process number.

All your Borgs on a machine share one background service. The first `borg` you open starts it,
with nothing to install, and it stops 30 seconds after the last session closes. To turn this off,
set `start-service = false` in the config.

The service keeps borg cloud connected for all your Borgs, so a paired computer reaches every open
session. It works in the session it names, or in the one you used last, and waits for your
approval there. You can pair from any Borg.

To keep a session running with no terminal, run `borg service keep NAME`. Open it with
`borg service attach NAME` or from the **Borg sessions** window, and press Ctrl-] to detach.
Quitting Borg there with F10 ends the session. When a paired computer asks for a session that is
not running, the service starts it in the background.

To start the service at boot, run `borg service install`, `enable`, and `start`. They use sudo,
which asks for your password, unless you are root. `borg service stop`, `disable`, and `uninstall`
undo them. You can also choose F9 → Remote → **Background service…**: the dialog shows the command
for the next step, and Enter runs it in Borg's shell. In a container without systemd, have the
container's process supervisor run `borg service run`.

### Updating with sessions open

`borg update` installs the new version, then moves the background service to it and tells you once
the service runs it. The service starts again in its own process, so the sessions it keeps hold on
to their shells and the programs running in them. Every open Borg follows when it is idle: it
waits for running jobs to finish, for edited files to be saved or closed, and for an agent's
request to be done. Then it starts again in the same terminal, keeps its shell, and says which
version it runs now.

The update keeps the version it replaced next to the program file, as `.borg.previous`. When the
new version does not take over within 60 seconds, Borg puts the previous one back and the service
starts again on it. The sessions it kept then start with new shells.

A service started by an older Borg cannot move by itself. `borg update` says so, and the service
uses the new version the next time it starts.

To update a paired machine, select it in [Remote hosts](#remote-hosts) and press **U**. After you
confirm, the host gets the latest release the way `borg update` does, and its service moves to it.
**Esc** closes the picker while the update goes on. Borg then tells you which version the host
runs, or that the new version did not start and the host went back to the one it ran. A mirror of
one of its sessions waits for it and shows the host again once it is back (see
[Mirroring a session](#mirroring-a-session)). Run `borg update` on the host instead when its Borg
is older or runs without the background service. Borg installed there with Homebrew or cargo is
updated with that tool on the host, and Borg names the command.

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

While a remote panel shows a session, the agent requests that wait there show on your screen too,
and you can answer them here (see [What the agent can do](#what-the-agent-can-do)).

### Remote hosts

In local Borg, press **Alt-H**, click **Remote hosts** in the shortcut bar, or choose
**F9 → Remote → Browse hosts and sessions…** to see paired machines and their named sessions.
Enter opens the selected session's files in the panel you opened the picker from, replacing what
that panel showed. Press **M** to mirror the session's screen, **U** to update the host (see
[Updating with sessions open](#updating-with-sessions-open)), and **R** to refresh the list.

**Tab** switches between **Hosts** (computers this Borg can reach) and **Clients** (computers
that can reach this Borg). Clients lists the computers paired with this Borg, their public-key
fingerprints, whether they are connected, and how many connections each has. After a computer
changed its key, the fingerprint line also says on which day, and the fingerprint it had before. A computer is listed
once even when a mirror uses several connections. Status updates while the picker is open,
including in a session whose connections are held by the background service. Pairing works in
one direction only: an allowed client does not automatically become a host whose files this Borg
can browse. When the connection owner cannot report connection status, paired clients are still
listed, with **Connection status unavailable**. The same Borg can be a host and a client at once.
The Remote menu also offers **Mirror mounted host**, **Share this Borg as host…**,
**Remote log…**, and **Background service…**.

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
can try again. A running session is stopped first, and the service does not start it again.

### Mirroring a session

`borg mirror MACHINE:SESSION` shows the remote session's live screen in your local terminal;
Alt+Shift+M opens the same mirror from a local Borg with a remote panel. **Ctrl-G** (hold Control
and press G) leaves the mirror and returns to the local panels, as does a click on the banner at
the bottom. The line above it shows who has control; Ctrl-] also exits.

A regular `borg` or `borg --session NAME` can be mirrored while it runs, without the background
service. Its own terminal stays visible, and any keyboard or mouse input there takes control back
from the mirror. While you have control, Borg on the host lays out for your window: when the space
above the banner is smaller than the host's terminal, Borg draws in the top left of that terminal
at your size, and the rest of it stays blank. Once control goes back to the host's terminal or you
leave the mirror, Borg fills that terminal again. A mirror without control follows the host
terminal's layout and is cut off to fit a smaller window; making your window larger shows the
rest. A session the background service keeps stays alive after its terminal closes, and the
computer in control sets its size.

If another terminal has control of the remote session, the mirror opens in view-only mode and
your keys do not reach the session. Press **Ctrl-T** or click the status line to take control and
detach the other terminal, or start with `borg mirror MACHINE:SESSION --take`; `--take` always
takes control from another viewer. Once you have control, the first key wakes a sleeping remote
screen, and later keys and mouse input reach it. A viewer that falls behind skips old animation
frames and jumps straight to the current screen.

When the host stops answering, for example while it updates, the mirror keeps the last screen,
says so on its status line, and tries again every 2 seconds for up to 100 seconds. **Ctrl-G**
leaves it meanwhile. Once the host is back, the mirror shows its screen again, with the version it
runs now at the bottom right. A host that is still away after that ends the mirror with a message.

An agent's request shows in the mirror as the server shows it, and you answer it there once you
have control. If you opened the mirror from a remote panel and the request still waits when you
leave the mirror, the panel's borg shows it again.

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

### Long jobs and drives

Borg knows what kind of drive each folder lies on: a hard drive, an SSD, a RAM disk, a network
share such as NFS, SMB or sshfs, or the machine a remote panel shows. On macOS it asks `diskutil`
and `hdiutil`, and on Linux it reads `/proc/self/mountinfo` and the device's flags in `/sys`.
Ctrl-L shows the drive and its filesystem in the **Drive** row, and the **Speed** row says how fast
Borg expects it to read and write.

Before a job writes anything, Borg works out how long it will take from the bytes it reads and
writes on each drive and the time compressing takes. When a copy, a move to another drive or an
unpack would take more than 2 minutes, or a change to an archive more than 20 seconds, the
**Long job** dialog says about how long and on which kind of drive, and waits:

- **Start** (`s`) runs the job.
- **Always for this file** (`f`) runs it and stops asking about this archive. Unpacking and
  changes to an archive offer it.
- **Always on this kind of drive** (`d`) runs it and stops asking about work on drives of this
  kind.
- **Cancel** (`c`) stops before the first write.

A save from the editor into an archive asks over the editor, which gets the keys back once you
answer.

Until Borg has measured a drive, it expects the usual speed of its kind: about 120 MB/s for a hard
drive, 900 MB/s for an SSD, 4 GB/s for a RAM disk, 60 MB/s for a network share and 10 MB/s for a
remote machine. Every job that runs for at least 2 seconds and moves at least 64 MiB teaches Borg
the drive's real speed, and the **Drive** row then says **measured speed**. A job that spends its
time compressing teaches it nothing about the drive.

Borg keeps the speeds and your answers in `drives.json` in its state folder, with up to 256
archives. The file is readable by you alone, since it names the archives you answered about. Every
Borg you have open shares it: each one reads it when it starts and again right before it writes, so
what one Borg learned or was told stays when another writes. The **Long jobs** row of Ctrl-L says
whether Borg asks first on that drive. **F9 → Options → Ask before long jobs again** forgets every
Always answer, those given in other Borgs too. A Borg that is already open keeps the answers it has
until it next writes the file or you start it again.

### Viewer and quick view

F3 opens the viewer. With the System plugin on, F3 on a `.pcap`, `.pcapng` or `.cap` file opens
the network screen's traffic viewer instead (see [Capturing traffic](#capturing-traffic)). In
text files, the arrow keys and PgUp/PgDn scroll, `w` turns line wrapping on or off, and Esc or
`q` closes it. In images, Left/Right or `p`/`n` show the previous or next image. Borg uses the
terminal's image protocol when there is one; other terminals show the image as colored blocks.

Ctrl-Q shows a quick preview in the other panel. Tab moves into it, where the arrow keys, PgUp/PgDn,
Home/End, and the mouse wheel scroll; Tab or Esc goes back to the file list. Ctrl-L shows file
information. F3 opens a text preview in the full viewer, and closing the viewer returns to the
preview with your position, wrapping, source mode, and hex rows kept. Both read files the same
limited way, so opening a large text file reads only around the first page and stays fast however
big the file is. On a result of a search for text, the preview opens at the file's first match
instead, with the query marked as the find bar marks it, and reads only around that match.

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

Press `h` in the text viewer or in the quick view to see the file's bytes as hex rows, and press it
again to go back to the text. A binary file, which Borg spots by a zero byte in its first 4 KiB,
opens as hex rows in both, and `h` shows its text. Each row starts with its offset in the file, then
shows its bytes in hex and the same bytes as text, with a dot for each byte that does not print.
Offsets and zero bytes are dimmed. A row holds as many bytes as fit the width: 64, 32, 16, 8, 4, 2
or 1, with a wider space after every eighth byte. Borg reads only the rows on the screen, so the
arrow keys, PgUp/PgDn, Space, Home and End move through a file of several gigabytes at once. The
find bar works in hex rows too and marks each match in both columns. In hex, Markdown and JSON files
show their bytes as written.

In the viewer, an entropy strip runs down the right edge of the hex rows. Entropy says how varied
the bytes are, from 0 bits per byte for one byte repeated to 8 for bytes as random as compressed,
encrypted or packed data. Text is usually near 4.5, and code between 5 and 6.5. The strip covers
the whole file from top to bottom. Each cell's bar is as long as the highest entropy in its part of
the file, faint below 4 bits per byte and bold red from 7.2, and the cells of the bytes on the
screen are marked as on a scrollbar. The status line gives the entropy at the page and whether it
is **low**, **typical** or **high**. Press `]` and `[` to jump to the next and previous run of
high-entropy blocks, or click the strip to go to that part of the file.

Borg reads the entropy in the background in blocks of 4 KiB. A file up to 256 MiB is read whole,
and a larger one is sampled at 65,536 evenly spaced blocks, so memory use stays the same however
big the file is. High entropy where you expect code, headers or tables can mean packed or
encrypted data hidden there. Compressed media, archives, encrypted volumes and key stores are high
all the way through, so read the strip against what the file should hold.

### Archives

Press Enter on an archive to open it as a folder. Borg opens files whose names end in `.zip`,
`.7z`, `.rar`, `.tar`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tbz2`, `.tbz`, `.tar.xz`, `.txz`, `.tar.zst`
or `.tzst`, and single compressed files ending in `.gz`, `.bz2`, `.xz` or `.zst`, which open as a
folder holding the one file. Ctrl-PgDn opens an archive too, and F3 on the archive itself still
shows its bytes in the viewer. The panel's title gets a badge such as **▤ zip** that names the
format Borg found in the file's first bytes, and the path goes on inside the archive. Folders
inside show the size of what they hold. Left, Enter on `..` or Ctrl-PgUp at the archive's top
takes you back to the folder that holds it, with the cursor on the archive. When the archive was
deleted or no longer reads, going up from any folder of it takes you back there too, and the
notice says why.

To list an archive, Borg reads what it holds, which for a compressed tar means reading the whole
file. While a large archive is read, the panel's footer shows how far Borg got, and Esc stops
reading and goes back. Borg remembers the listings of the last 4 archives as long as their files
keep the same size and time, so going in and out again is instant. Archives with more than
1,000,000 entries do not open.

F3 or Enter on a file inside unpacks it into a private folder in Borg's runtime folder and opens
it in the viewer. The unpacking runs as a job, so a large file shows its progress, and Esc cancels
it. Borg deletes the unpacked file as soon as the viewer has it open.

F4 on a file inside opens it in the editor, and Alt-E opens it in the panel's editor. Borg unpacks
the file into the same private folder, and the editor's title shows its path inside the archive.
Each save writes the file back into the archive as a job, and the editor says so ("Saved. Writing
it into site.zip."). When the archive does not take it, the notice says why and where your edit
stays.

F5 copies the marked files and folders out of the archive into the other panel's folder, or into
the folder you type in the Copy dialog. **Extract here** and **Extract to other panel** in the
File menu (F2) unpack the whole archive under the cursor into this panel's folder or the other
panel's. Either way the files land in a folder on this machine, and the job shows up in Alt-J with
its progress, rate and time left. It asks about names that already exist as a copy does, writes
each file under a temporary name until it is complete, and stops each file at the size the
archive gives it.

Borg changes zip, 7z and tar archives, compressed or not. F5 or F6 from a folder of this machine
copies or moves the marked files into the archive the other panel shows, or into the folder of it
you type in the dialog. F7 makes a folder in the archive, and F8 deletes the marked entries after
asking. F6 to a name in the same archive renames or moves the entries there, and F6 to a folder of
this machine moves them out: they unpack there and then leave the archive. Names that already exist
in the archive are asked about as a copy asks, and so are names two of the files you copy share,
as files from search results or a collection can. A rename to a name the archive holds stops with
a notice. A move deletes only what arrived whole, so a file you skip stays where it was, and so
does one that a later file of the same name replaced.

Each change runs as a job in Alt-J. Borg writes the changed archive beside the original under a
temporary name, reads it back, and then puts it in the original's place with the same permissions.
The read-back checks a zip's or 7z's list of entries and every entry of a tar. A file goes in as it
is when Borg writes it, so a file you save again while the job waits for your answer goes in with
its latest contents. A change that fails or that you cancel leaves the archive as it was. Entries
the change keeps are copied as they are stored: a zip copies their compressed data, and a 7z copies
every solid block that keeps all its entries and packs again only the blocks that lose one. A
compressed tar is unpacked and packed again whole, then unpacked once more to read it back, so
changing a large `.tar.xz` takes about as long as making it and unpacking it. A zip or plain tar
that only gains entries is copied and the new entries follow the last one, and on APFS, Btrfs, XFS
and bcachefs that copy is instant. Before a change that would take long, Borg asks first (see [Long
jobs and drives](#long-jobs-and-drives)).

In an encrypted zip or 7z archive, new entries are encrypted with AES-256 and the password you
opened the archive with, and a 7z that hides its list of entries keeps it hidden. Entries the change
keeps stay as they were.

RAR archives and single compressed files such as `.gz` stay read only, and changing them says so.
Ctrl-Q and **Compress…** do not work inside an archive. An archive inside an archive opens once you
copy it out. The shell stays in the folder that holds the archive, and a saved session reopens that
folder with the cursor on the archive.

Encrypted zip and 7z archives ask for the password when they open. In the **Password** dialog each
character you type shows as glitch noise that keeps changing and flares with every key. The noise
depends only on how many characters you typed. The dialog asks again when the password is wrong, and
Esc leaves the archive. Borg keeps the password in memory while the panel shows the archive and
never writes it anywhere. **Extract here** has no way to ask for one, so for an encrypted archive
press Enter on it, type the password, and copy the files out with F5.

Before it writes anything, Borg adds up what the chosen entries unpack to and asks first when
that is more than 1 GiB and more than 100 times the archive's size, or more than 100,000 files,
which is how zip bombs look. It also asks when the destination has less free space than the files
need, and a change to an archive asks when its drive lacks room for the new archive beside the old
one. **Unpack anyway** (`u`) goes ahead, and Cancel stops before the first file. Entries with
absolute paths, `..` in their paths, or device files are left out with a notice, and no entry or
link in an archive can make Borg write outside the destination folder.

Borg reads zip, 7z and the tar family itself. RAR archives go through 7-Zip (`7zz` or `7z`) or
`bsdtar`, whichever Borg finds first, and macOS comes with `bsdtar`. When none is installed, Enter
on a RAR archive offers to install bsdtar. The **Install bsdtar** dialog works like the one for
tcpdump in [Capturing traffic](#capturing-traffic): it shows the exact command for your package
manager, and Enter types it into Borg's shell. Borg also looks for `bsdtar` in Homebrew's
`libarchive` folder, which Homebrew leaves off `PATH`. Encrypted RAR archives do not open yet.

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
you can edit two files side by side, or a file next to the folder it is in. In search results and
collections, Alt-E opens the editor in the other panel, so the list stays in view. Alt-E on another
result opens it in place of the file there. When that file has unsaved changes, Alt-E brings its
editor forward and asks whether to save them. F6 moves from a panel's editor to the other panel, and
Tab goes back to the editor from the file list. Text copied with Ctrl-C in one editor pastes with
Ctrl-V in the other. While an editor has focus, the shortcut bar shows the editor's keys, and the
panel keys come back with the file list. Ctrl-O shows the console from an editor too, and F9 opens
the menus. Esc or F10 closes the editor and gives the panel back its files; Quit first asks about
each panel editor with unsaved changes. Quick view and file information need the other panel, so
they wait until its editor is closed. An editor opened beside results gives the focus back to them
when it closes, and brings back the quick view or file information they showed.

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
Borg reads the file setting by setting. A setting it does not have, or a value it cannot use, costs
only that setting: Borg says which line it is when it starts and uses its default there. When Borg
saves a setting it changes that one and keeps the rest of the file, with your comments, as you wrote
it. A setting from an older version is read as what replaced it and leaves the file at the next
save. A file that is not TOML at all starts Borg on its defaults, and the first setting you then
save keeps that file beside the new one as `config-unreadable.toml`.

```toml
theme = "neon"              # midnight, aurora, neon, classic, paper, ember, or your own
icon-style = "auto"         # auto, nerd, or portable; Ghostty gets rich icons automatically
icon-spacing = 1            # spaces after an icon, 1 to 3
status-line = true
menu-bar = false            # keep the menu bar visible
show-hidden = false
return-to-panels = true    # return after commands launched from the panels
restore-terminal = false    # bring back the shell scrollback on the next start
screensaver = true          # one of five animations, picked at random
screensaver-after = 300     # seconds of idle time
update-check = true         # look for a new release once a day
start-service = true        # start the background service when Borg opens
graphics = "auto"           # auto, kitty, sixel, iterm or off
relay = "wss://relay.example.com"  # the relay for borg cloud

[plugins]
git = true
system = true
docker = true
docker-console = true       # the command line follows container panels into the container
agent-log = true            # keep a log of what agents do in each session
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

[folders]
hotlist = ["~/Projects", "/srv/www"]  # pinned with Ctrl-\ and Insert
history = true              # remember the folders the panels open

[network]
proxy-ca = "~/dev-ca"       # the CA folder for captures through Borg's proxy
keep-free-mib = 1024        # stop captures before the disk has less than this free

[watch]
on = false                  # watch what starts, in the background service
freeze = true               # stop a process a rule is sure about
learning-days = 7           # learn what is normal here before freezing anything, 365 at most
alerts-per-minute = 20      # processes written about before Borg only counts them
freezes-per-minute = 5
keep-running = ["backup-agent"]  # programs Borg never freezes

[[watch.allow]]
program = "/bin/sh"         # allowed starts: program, parent, user, or any of them together
parent = "nginx"

[watch.notify.telegram]     # send alerts as they happen, see Alerts off the machine
token = "123456:ABC"
chat = "123456789"
```

In a local terminal with true color, Borg starts with a short glitch animation of its logo; any
key or click skips it. Over SSH, and in terminals without true color, it opens straight to the
panels.

After `screensaver-after` seconds without input, Borg shows one of five screensavers, picked at
random: a cube spinning in a starfield, the glitching logo, a clock in the logo's block letters
that tears apart as each minute turns, the logo swirling out into a spiral galaxy and back, or
the logo rebuilt plank by plank while the old planks gather into a second logo below it. Any key
or click brings the panels back, and that key does nothing else. An agent's approval ends the
screensaver by itself, so it never waits behind one.

Custom themes go in `~/.config/borg-commander/themes/`. A theme can extend a built-in one:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

### Borg's files

Panel folders, cursor positions, the split, listing modes, and sorting are saved in the state
folder, `~/.local/state/borg-commander`, or in the folder set by `$XDG_STATE_HOME` or
`$BORG_STATE_DIR`.

While it runs, Borg keeps its shell integration scripts and files copied out of containers in its
runtime folder, `$XDG_RUNTIME_DIR/borg-commander`, or `borg-commander-<uid>` in the temporary
folder. Only you can open this folder; if one already exists that someone else created, or that
other users can enter, Borg skips it and uses a new name. Its `TTL` file starts with the time
until which the folder is in use: every running borg pushes that time a day ahead each hour, so
once it has passed, the folder can be deleted.

**F9 → Options → Borg's files…** shows every place Borg keeps files, how much each one holds, and
where it is:

- **Temporary files**: shell integration scripts, folder requests from the shell, copies out of
  containers, remote previews, files unpacked from archives for the viewer and the editor, and the
  certificates of a capture's proxy, in the runtime folder.
- **Sockets**: where agents and mirrors reach running sessions, in `/tmp/borg-<uid>-default`, or
  in a `/tmp/borg-<uid>-` folder named after the runtime folder when `$XDG_RUNTIME_DIR` is set.
- **Caches**: folder sizes and name indexes, in the cache folder (see
  [Saved size scans](#saved-size-scans)).
- **Sessions and history**: sessions, shell output, the remote log, search and folder history,
  collections, paired computers with their keys, and what Borg learned about your drives
  (`drives.json`), in the state folder.
- **Settings**: `config.toml` and your themes.
- **Captures**: the traffic captures Borg saved, wherever they are.

Up and Down choose a row, and a short note under the list says what the chosen place holds and
how long it stays. Enter opens the place's folder in the active panel. Esc closes the window.

Borg cleans up after itself 30 seconds after it starts and then every hour. It only removes
names it makes itself, and never anything that changed in the last 10 minutes, so a Borg that is
still starting up keeps what it made. This is what goes:

- Folders of other Borg versions in the runtime folder, once no Borg runs from them, and either
  no Borg was running when this one started or nothing in them changed for 3 days.
- Folder requests, half-written files, remote console folders and capture proxy certificates of
  Borg processes that ended.
- Copies out of containers, remote previews and files unpacked from archives for the viewer and
  the editor, once they are older than a day.
- Sockets nobody answers on, and socket folders where nothing answers any more.
- Size scans in the format of Borg 0.4 and 0.5, scans the cache no longer lists, and name indexes
  of folders that are no longer indexed.
- Runtime folders that older Borgs left open to other users, once nothing in them changed for
  3 days.

Sessions, history, settings and captures stay until you delete them. The line under the note says
how much is left over right now and what the last cleanup removed. F8 removes what is left over
after asking, and the question lists the folders it comes from.

Each capture Borg saves goes on a list of the 1,000 newest. The window shows the ones still on
disk under **Captures Borg saved**, with their size and date, and **with keys** when a `.keylog`
file of the same name sits beside one. Enter on a capture opens its folder with the cursor on the
file, so F8 there deletes it. Captures saved before Borg started keeping this list are not on it.

`borg storage` prints the same places in the terminal and says what is left over, and
`borg storage clean` removes it.

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

CPU is an average of the last few seconds in which older use fades out, as in `top`, so a list
sorted by CPU holds still while a process works in short bursts. The overview and the system CPU
at the top of the monitor are averaged the same way. **H** hides the processes that used no CPU
in that time, and the filter row says how many are hidden. A process that goes quiet stays listed
until its average falls to 0.0. In the tree, the processes a busy one descends from stay listed
with it. Press H again to show them all.

| Key | Action |
| --- | --- |
| Enter | Open the process's memory map / move to the region preview |
| `/` | Filter by name or PID; ↑↓ still move, Enter keeps the filter, Esc clears it |
| C / M / G / N / P | Sort by CPU / RAM / growth / name / PID; press again to reverse |
| I / O | Sort by read / write rate |
| T | Show or hide the process tree |
| H | Hide or show the processes that used no CPU in the last few seconds |
| Left / Right | In the tree, fold the selected process's branch or unfold it |
| K | Send a signal or change the priority |
| E | Scan every process for high entropy |
| Alt-N | Show the selected process's traffic on the network screen |
| Space | Pause or resume sampling |
| R | Refresh the current view |
| Esc | Go back; in the process list, close the monitor or return to the network screen |

Click a column heading to sort, and double-click a process to open its memory map. The overview
shows current resident memory, the highest value seen, the change since monitoring started, CPU,
virtual address space, and a chart of resident memory over the last 60 seconds. Footprint,
swapped, private, and shared totals appear once the memory map has been read, if the system
reports them. **Enter** in the memory pane opens the memory map. The memory map is read when you
ask for it; press R to read it again after the process changes its mappings.

Below the chart, **Process tree** lists the processes the selected one descends from, the process
itself, and everything it started, each indented under the one that started it and shown with its
resident memory. When they do not all fit, the last line says how many are left out.

In the tree that **T** shows, each process is listed under the one that started it, and **▾** marks
a process with others under it. **Left** folds the selected process's branch; on a process with
nothing under it, Left folds the branch it belongs to and selects the process that started it.
**Right** unfolds a folded branch, and on an open one steps to the first process in it. A folded
process shows **▸** and how many processes it hides. A branch stays folded until its process ends.
While a filter applies, the list is flat.

### Signals and priority

K opens **Signal or priority** for the selected process. Press a choice's letter, or move to it
and press Enter. Esc closes the dialog and sends nothing.

| Key | Choice | What it does |
| --- | --- | --- |
| T | **Terminate** | Sends TERM, which asks the process to quit |
| I | **Interrupt** | Sends INT, the same as Ctrl-C in its terminal |
| H | **Hang up** | Sends HUP; many services read their settings again |
| K | **Kill** | Sends KILL, which ends the process at once without letting it clean up |
| S | **Stop** | Sends STOP, which pauses the process |
| C | **Continue** | Sends CONT, which resumes a stopped process |
| L | **Lower priority** | Raises its nice value by 5, up to 19, so it waits longer for the CPU when the system is busy |
| R | **Raise priority** | Lowers its nice value by 5, down to -20 |

Right before sending, Borg checks that the PID still belongs to the process you chose, started at
the same time, so a new process that reused the PID is left alone. The status line confirms what
was sent, and the list samples again at once. The overview says when a process is stopped, and
shows its **Nice** value when it is not 0.

You can signal your own processes and lower their priority. The system refuses the rest: another
user's process, and raising a priority, which only an administrator may do. Borg then offers
**A Administrator**, which sends the same choice through the helper described under
[Administrator access](#administrator-access). Borg sends no signals to itself; F10 quits it.

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

When you select a readable region, Borg captures up to 64 KiB of it in the other pane. If the start
of the region holds only zeros, the capture skips ahead to the first page with data, and the line
above the bytes says where in the region the window is. Each row starts with its offset from the
start of the region, and a wide pane shows more bytes on a row. When you move quickly, Borg waits a
moment before reading; selecting another region clears the old preview and cancels its pending read.
Regions that cannot be read show a message instead. Press **Tab** or **Enter** to scroll the
preview. **Esc** there goes back to the list, and **Esc** in the map goes back to the process list
with its cursor and filter as they were.

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

Pages whose entropy reaches 7.2 bits per byte are drawn in bold red, or bold alone where red would
be hard to read, whatever their kind. Packed or encrypted code stands out that way in an
executable region, where Borg counts every page that is not zeros or filler as code. The legend
gives their share as **high entropy**, the cursor's line adds the entropy of the page under it, and
the same line ends with the highest entropy in the region. A note under the legend says what high and low entropy usually
mean. Compressed caches, images, archives and keys are high too, so look at what the region holds
before you suspect it.

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
**A Administrator**. Treat findings as leads, because debuggers, profilers, updaters, and
security tools leave the same traces.

### Scanning memory with rules

**Y**, in the memory map or among search matches, scans the memory the process filled in itself
with the same rules as a file scan: Borg's and those in your rules folder (see
[Scanning files with rules](#scanning-files-with-rules)). Like **Strings**, it reads memory not
backed by a file and memory the process may write to; on macOS, regions it never wrote to are
skipped too. A region with no page in memory or in swap is not read, and on Linux the page tables
show which pages the process never touched, so those are not read either. Swapped pages are read,
since a payload can wait there. Memory is read in pieces of 16 MiB that
overlap by 4 KiB, so a match that crosses from one piece into the next is still found. Each rule
is listed once per region, at its first match.

Matches take the list pane, with alerts first. Each one names the rule and where it comes from,
**Borg's rules** or your file, then the bytes it matched and what the rule looks for, and the rows
below the list say what else could explain it. Selecting a match previews the bytes around it with
the match highlighted. Rule files left out come last, with the compiler's message. **R** scans
again, **Ctrl-C** stops the scan and keeps what it found, and **Esc** goes back. A scan reads at
most 1 GiB, for up to 45 seconds, lists up to 2,000 matches, and says if it stopped early. Memory
it cannot read is counted, and Borg offers **A Administrator**, which runs the scan in the helper
with your rules folder.

A rule scan is triage: a quick look at one process or folder for patterns someone has described.
It does not watch the system, block anything or clean up, which is what endpoint protection is
for. Some things stay out of its reach:

- macOS keeps the memory of its own programs and of programs built with the Hardened Runtime from
  every other program, administrators included (see [Administrator access](#administrator-access)).
- On Linux, `kernel.yama.ptrace_scope` decides whose memory you may read. At 1, as on Ubuntu, you
  may read only your own child processes, so Borg offers **A Administrator** for the rest. At 3, nobody may read another process, administrators
  included.
- A kernel rootkit can hide memory, files and processes from every program, Borg included, so a
  clean scan proves little.
- A packed or encrypted program shows its real code only while it runs. Scanning its memory then
  sees what a scan of its file cannot.

Borg reads memory without attaching as a debugger, with `process_vm_readv` on Linux and
`mach_vm_read` on macOS, so the process sees no tracer, which is what most anti-debugging checks
look for.
Monitors that run as administrator, such as auditd, eBPF tools and macOS Endpoint Security
clients, can still log the reads. A file scan opens every file it reads, which `inotify` and
`fanotify` report. On Linux, it leaves the access time of the files you own as it was.

### Entropy scan of every process

**E** in the process list opens **Entropy scan**, which reads sampled pages of every process for
bytes as dense as packed code. Press a choice's letter, or move to it and press Enter. Esc closes
the dialog.

| Key | Choice | What it reads |
| --- | --- | --- |
| C | **Code of all processes** | The executable regions, where packed or encrypted code shows |
| M | **All memory of all processes** | Every readable region, so data such as keys and compressed files too |
| L | **Last results** | Nothing new: it shows the hits of the last scan again |

Borg reads up to 1,024 pages of 4 KiB from a region and 32,768 from a process, spread evenly. A
process with many regions gets fewer from each, but never fewer than 16. A region with at least
one page of 7.2 bits per byte or more is a hit. Hits take the list pane: code first, then memory
the process wrote, then files mapped as they are on disk, and within each the region with the
larger share of high pages first. Code with 70% or more of its pages high stands out in bold, by
the same rule as the search for [packed executables](#finding-packed-executables). Each row gives
the kind (**code**, **written** or **file**), that share with the highest entropy, and the process
name, PID and region. The other pane shows the region's addresses, size and access, how many
sampled pages are high, where the first of them starts, and what such bytes usually are. The
2,000 strongest hits are kept, and the summary counts the weaker ones left out.

**Enter** on a hit opens the process's memory map at that region, with the page map on and its
cursor on the first dense page sampled, so you can look at the bytes around it. **Esc** goes back
to the map, and again to the hits.

Borg reads 4 processes at once. A region with no page in memory or in swap is not read, and on
Linux the page tables show which pages are in memory, so the scan neither touches new pages nor
brings swapped ones back. Caches every process maps alike are left out: the dyld shared cache on
macOS, and the pages the kernel maps into every process on Linux. A file mapped unchanged, such
as a shared library, is read in the first process that maps it only.

A scan stops after 90 seconds and leaves the rest for **N**, which goes on with the processes it
did not reach. **Ctrl-C** stops it sooner and keeps the hits, and **R** scans again from the
start. Above the hits, one line counts the processes scanned and the hits, and the next the
processes left out: unreadable, protected and not reached. On macOS, vmmap takes a few
seconds to list a process's regions, so a scan of every process can take a few rounds of **N**.

Processes whose memory macOS protects are counted and skipped, since nobody can read them. For
the processes Borg could not read, it offers **A Administrator**, which scans them in the helper
for at most 60 seconds, so the helper finishes within its 2-minute limit even after a slow
password prompt. Dense bytes are common: compressed caches, images, fonts and keys are high by
nature, so treat hits as leads and read the details before you suspect a process.

### Administrator access

When reading a memory map or a capture is restricted, a search could not read part of the
memory, an entropy scan could not read some processes, or the system refuses a signal or priority
change, Borg offers **A Administrator**. On macOS this opens the system's authorization prompt;
on Linux it uses `pkexec` and needs an authorization agent to be running. Only a short-lived
helper runs with elevated rights. Borg itself keeps running normally, and the helper only returns
map data, a limited number of bytes or the hits of a scan, or sends the one signal or priority
you chose.

macOS shields the memory of its own programs (System Integrity Protection) and of programs built
with the Hardened Runtime from every other program, administrators included. Borg reads this from
the program's signature: the process details say **Memory protected**, a refused read explains
why, and **A Administrator** is not offered, since it cannot help. Their memory maps and open
files are still shown. It is not offered either when Borg already runs as administrator. On
Linux, security policy can still deny access; Borg shows the error and the process monitor keeps
working as usual.

A process is identified together with its start time, so a new process that reuses a PID does
not inherit another process's history. Memory maps, inspection results, captures, and history
all have size limits. The Borg Commander name stays visible in the bottom-right corner
everywhere, including the console, viewer, monitor, dialogs, and screensaver.

### Alerts

Borg can watch what starts on a machine and tell you when something looks wrong. Turn on **Watch:
unusual process starts** in F9 → Options → Plugins. The watcher lives in the background service
that all your Borgs on the machine share, so every Borg reads the same alerts. That service runs
while a Borg is open. To keep watching with no terminal open, start the service at boot as
[Named sessions and the background service](#named-sessions-and-the-background-service) describes.

Alerts stay on that machine until you name a place to send them, and the page says so while they
do (see [Alerts off the machine](#alerts-off-the-machine)).

**Alt-L** or **Command → Alerts** opens the page. The list takes the active pane, newest alert
first, and the other pane shows the selected alert in full. Each row shows the time, the rule that
fired, the program, the parent that started it, the user it ran as, and what has become of the
process. The line above the list counts the alerts and how many nobody has read yet.

The details say in one sentence why the rule fired, then the command, the program, the parent, the
user, what the standard streams led to, and where the process stands now. Under **Process tree**
each parent appears above its child, so you can read the path from a service down to the command
that worries you. **Connections** lists the sockets the process held at that moment, **Program
hash** is the SHA-256 of its program, and **Rules** shows what Borg's rules and yours matched in
that program (see [Scanning files with rules](#scanning-files-with-rules)).

A command longer than 2 KiB keeps its start and its end, 2 KiB together, with `…` between them. The
commands of its parents and the values of its preload variables are cut the same way. That keeps the
newest 200 alerts inside the log however long their commands run, and a long first argument cannot
push the end of a command out of view. Ctrl-C copies the command as the log keeps it.

| Key | Action |
| --- | --- |
| Enter | Show the process in the activity monitor |
| C | Let a frozen process go on running |
| K | End the process, after a question |
| A | Allow starts like this one from now on |
| `/` | Filter the rows; ↑↓ still move, Enter keeps the filter, Esc clears it |
| R | Read the log again |
| Ctrl-C | Copy the command the process ran |
| Alt-W | Maximize the list |
| Esc | Clear the filter, then close the page |

The filter keeps the alerts that contain every word you type, in the rule, its short name, the
reason, the program, the parent, the user, the command and what Borg did. Put `!` before a word to
leave out what it matches: `shell-on-socket !root` keeps the remote shells of every other user.

Alerts live in `alerts.json` in the state folder, owner-only, the newest 200 of them. An alert about
a process that is still frozen stays beyond that, so you can always let it go on or kill it. Borg keeps
the file within 8 MiB, the most it reads back, and leaves out the oldest alerts when they would not
fit. The **Borg's files** window counts them under **State** (see [Borg's files](#borgs-files)).
Opening the page marks what it holds as read, and the count of unread alerts follows the log while
Borg is open, whether or not the page is.

When Borg finds an `alerts.json` it cannot read, such as one an older version let grow past 8 MiB,
it renames the file to `alerts-unreadable.json` before it writes a new log, so the alerts in it
stay on disk. A later one gets a numbered name such as `alerts-unreadable (2).json`. The Alerts page
names the file below the list for as long as it is there, so delete it once you have looked
through it.

### What the watcher looks for

Each rule names one thing that is odd about a process that has just started. The level says how
much it means: **alert** is seldom innocent, **warning** often has an ordinary reason, **notice**
is there so the picture is complete.

| Rule | What fires it | Level |
| --- | --- | --- |
| **Shell on a network socket** | A shell whose input, output or errors go to a network socket, which is what a remote shell looks like. A socket that stays on the machine, such as the one `systemd` gives a service's output to the journal, does not count | alert |
| **Remote shell on a terminal** | A shell on a terminal that a program opened for it, where that program's own input, output or errors go to a network socket, or the program is a relay such as `socat` or `nc`. That is a remote shell turned into a full terminal, as `pty.spawn` in Python makes one | alert |
| **Program deleted while it runs** | The program's file is gone, so nothing on disk says what the process is | alert |
| **Program with no file** | Borg can find no program file for the process at all | alert |
| **Library forced into a program** | `LD_PRELOAD`, `LD_AUDIT` or `DYLD_INSERT_LIBRARIES` is set in its environment | alert |
| **Shell from a service** | A web server, application runtime or database started a shell | warning |
| **Shell without a terminal** | A shell with no terminal on either side, so nobody typed it | warning |
| **Shell on a program's terminal** | A shell on a terminal that a program opened for itself and nobody logged in to, such as one that `expect` or `docker exec` opens | warning |
| **Looking around the machine** | Four different commands such as `whoami`, `id`, `uname` and `ss` from one program within an hour, with nobody logged in to the terminal they run on. Commands it runs through one shell after another count together, so a web shell that starts `sh -c` for each command is caught too | warning |
| **Program in a temporary folder** | The program lies under `/tmp`, `/var/tmp`, `/dev/shm` or `/run/shm` | warning |
| **Name differs from the program** | The process calls itself one thing and runs another | warning |
| **Parent never started it before** | This parent has never started this program on this machine | notice |

For each process Borg reads the program it runs and whether its file was deleted, the command, the
user, the terminal it belongs to and who opened it, what its standard input, output and errors lead
to, the preload variables in its environment, and the chain of parents above it. A socket counts as
a network socket when the process's network lists it as TCP or UDP.

Whether somebody logged in to a terminal is decided the same way as for freezing (see
[Freezing a process](#freezing-a-process)): over SSH, at the console, in `tmux` or `screen`, through
`sudo` or `su`, or in a terminal window. A terminal that a program opened for itself does not count,
whether an editor's remote server, `docker exec` or a reverse shell opened it.

Borg learns the machine before it acts. For the first 7 days it records which parent starts which
program here and freezes nothing, and the Response column says **Would freeze** where it would have.
After that, a pair it has seen before stays quiet for the rules that can be learned: the three shell
rules, the name that differs, and the parent that never started it. The run of commands is learned
by the program it comes from: it stays quiet when that program ran each of its commands during the
learning week, so a monitoring agent that runs `df` and `ps` all day stays quiet, while a web server
that never ran `id` does not. The five alert-level rules fire every time, because a shell on a
socket or a forced library never becomes normal by happening often. Whatever runs during that week counts as normal, so turn the watcher on
while you trust the machine. Starts that a rule learning cannot quiet fired on stay out of what Borg
learns.

**A** on an alert allows starts like it from then on, by its program and its parent. Those
allowances go in `watch-allowed.json` in the state folder, which the watcher reads and takes in
within a minute. The `[watch]` section of the configuration holds the same thing by hand (see
[Configuration](#configuration)). Borg writes down at most 20 processes a minute and freezes at most
5, and the line above the list says how many findings it left out when more fired at once. A process
Borg freezes is always written down, however many alerts came before it, so you can let it go on.
Notices are counted apart, so a run of them never takes the room of a warning, and up to 20
processes a minute that Borg would have frozen past its limit of 5 are written down too.

### Freezing a process

Once the learning period is over, Borg freezes the process an alert-level or warning-level rule
fired on. Freezing sends a stop signal to the process and everything below it. A frozen process has
not exited, so its service keeps running, `systemd` and Docker have nothing to restart, and
whatever it was about to do waits where it is until you look. The Response column says **Frozen**,
and **C** lets it go on while **K** ends it. Killing is final, so Borg asks first.

Borg starts freezing once you have seen every alert from the learning week that says **Would
freeze**, or 3 days after the week ends, whichever comes first. Opening the Alerts page marks them
read, and the watcher notices within a minute. That way you can check what the week caught and allow
a harmless program with **A** before anything stops, and a machine nobody looks at is still
protected. Until then, the page's footer says how long freezing waits, and the details of a new
alert say that it is waiting.

Borg never freezes the system's first process, itself or anything it started, a program on the list
it never freezes (`init`, `systemd`, `sshd`, `sudo`, `su`, `login` and the like, plus any name you
add under `keep-running`), a process on a terminal somebody logged in to, or a start you have
allowed. A notice never stops a program. The details say which of these kept a process running.

A program on the built-in list is spared only where the system keeps its programs, such as `/usr`
and `/bin`, so a copy named `sshd` in another folder is frozen like anything else. Names you add
under `keep-running` match wherever the program lies. A terminal counts when somebody logged in to
it: over SSH, at the console, in `tmux` or `screen`, through `sudo` or `su`, or in a terminal
window. A terminal multiplexer counts only for root and for people's accounts, numbered from
`UID_MIN` in `/etc/login.defs` (1000 on most systems), because a service account can start one too.
A terminal that a program opened for itself does not count, which keeps a remote shell from hiding
behind one. Terminals opened by an editor's remote server or by `docker exec` do not count either,
so if Borg freezes something you ran there, **C** lets it go on and **A** allows it.

Borg checks a process's identity, its number together with the moment it started, right before it
signals it, so a number another process has taken in the meantime is left alone.

### The watcher as root

Without more rights, Borg hears only about its own user's processes, by looking at the process
list four times a second, and the page says so under the list. A short command can be gone before
the next look.

To hear about every process the moment it starts, install the part that runs as root:

```sh
borg service watch install
```

This asks for your password once and installs a separate unit, `borg-commander-watch.service`, which
runs `borg --watch-root`. That part loads Borg's own small programs into the kernel, which read each
program start before the new program runs: the program, the command line exactly as it was passed,
the process that started it and the user. A command that ends at once is still reported, and so is
the command line of a process that rewrites it afterwards. The root part reads `/proc` for the rest,
and stops, continues or ends a process when Borg asks. It does nothing else: the unit drops every
capability it does not need, and Borg's own interface keeps running as you. The two sides talk over
a socket in the state folder that only its owner can reach, Borg accepts a connection only from a
process running as root, and the root part connects only to a socket that belongs to the user it was
installed for. `borg service watch uninstall` removes it, and `borg service watch status` prints
where the events come from and what has been found. The install and the status both warn while alerts
have no place to go (see [Alerts off the machine](#alerts-off-the-machine)).

The programs need Linux 5.8 or later. On an older kernel, or where the root part may not load them,
it hears the kernel's process events and reads everything from `/proc`, and the page says so under
the list. After updating Borg, run `borg service watch install` again, so the unit gets any rights
the new version needs.

Alerts from either side are the same alerts, in the same log. A process another user owns can be
frozen only by the root part, so resuming or killing one from the page goes through the watcher in
the service. When nothing can reach it, the page says only the owner or an administrator may do
that.

The watcher is a Linux feature for now. On macOS the page opens and reads its log, and the source
says nothing is watching.

### Alerts off the machine

The Alerts page lives on the machine it watches, so whoever takes that machine over can delete the
log. Send each alert somewhere else as it happens: a copy that has arrived is out of their reach,
and a heartbeat tells you when the watcher goes quiet. Add `[watch.notify]` to the configuration on
the server, with any of these places or several. Every address uses `https://`, or `http://` to the
server itself:

```toml
[watch.notify]
name = "web-01"             # how messages name this machine, its host name otherwise
level = "warning"           # the least an alert needs to be sent: alert, warning or notice

[watch.notify.telegram]
token = "123456:ABC-your-bot-token"
chat = "123456789"

[watch.notify.ntfy]
url = "https://ntfy.sh/a-long-random-topic"
token = "tk_your_token"     # where the topic needs one

[watch.notify.webhook]
url = "https://example.com/borg"
secret = "a long random string"

[watch.notify.heartbeat]
url = "https://hc-ping.com/your-check-id"
minutes = 5
```

Then run `borg service watch notify`. It sends a test message to each place and prints what each
one answered.

- **Telegram**: create a bot with @BotFather, which gives you the token. Send the bot a message,
  then open `https://api.telegram.org/bot<token>/getUpdates` and take `chat.id` from the answer.
- **ntfy**: subscribe in the ntfy app to a topic with a long name nobody can guess, or to one on
  your own ntfy server.
- **Webhook**: Borg posts every message to the address as JSON. With `secret`, each request
  carries `X-Borg-Signature: t=<time>,v1=<signature>`: the HMAC-SHA256, under the secret, of the
  time, a dot and the body, so your receiver can tell Borg's messages from forged ones.
  `X-Borg-Event` says what the message is: `alert`, `evidence` (the program's SHA-256 and rule
  matches, which follow an alert once Borg has read the program), `started`, `armed` or `test`.
- **Heartbeat**: Borg calls the address every `minutes` while the watcher runs and adds `/fail`
  when something is wrong: the thread that hashes programs stopped, a place has not taken a
  message for 30 minutes, Borg gave up on one, or Borg could not read its outbox. A service such as
  healthchecks.io tells you when the calls stop, which is what happens when someone kills the
  watcher.

While no place takes alerts, they stay in the log on the machine, and Borg says so wherever you meet
the watcher. The Alerts page shows **Alerts stay on this machine.** above the details of the
selected alert, or in the last row of the list when the list is maximized. You get the same line
when you turn the watcher on in Options → Plugins, from `borg service watch install` and
`borg service watch status`, and in the service's output when the watcher starts. A heartbeat alone
does not end the warning. It tells you that the watcher runs and carries no alerts, so add Telegram,
ntfy or a webhook as well.

The warning gets stronger when Borg is close to acting. From a day before the learning period ends
it reads **Borg is about to freeze processes here, and nobody will be told.**, and once freezing has
started, **Borg freezes processes here and nobody is told.** Borg still watches, learns and freezes
without a place to send alerts, so set one up before the learning period ends on a machine you do
not look at every day.

The service sends each alert within seconds of writing it down, and also says when the watcher
starts and when freezing begins. Notices stay on the page unless `level` is `notice`. A message a
place refuses waits in `watch-outbox.json` in the state folder and is tried again, 5 seconds later
at first and every 30 minutes at most. Borg keeps up to 500 waiting messages, fewer when long ones
would take the file past 16 MiB, and tries each for 48 hours. The next message says how many it
gave up on.

A message carries at most 1,000 bytes of the command line, the program's path and the reason for
the alert. A longer one keeps its start and its end with an ellipsis between them, so padding the
start of a command cannot hide what it does.

When Borg finds a `watch-outbox.json` it cannot read, it renames the file to
`watch-outbox-unreadable.json` and starts a new outbox. A later one gets a numbered name such as
`watch-outbox-unreadable (2).json`. The messages that were waiting stay in that file and are never
sent, so the heartbeat fails once and says why, and each place's numbering starts again at 1. Look
through the file, then delete it.

Each place numbers its messages from 1, and every message's JSON carries the SHA-256 of the one
before it in `previous`. A gap in the numbers, or a `previous` that does not match, means a message
went missing or was changed on the way. The numbering happens on the server, so it cannot catch
someone who controls the server.

The tokens sit in the configuration, which anyone running as your user on the server can read. With
them, someone can send messages that look like Borg's and carry the numbering on without a gap, and
a Telegram bot can delete what it sent in the last 48 hours. Whoever takes over the server can also
delete the messages still waiting to leave. What arrived before that stays as it was, and a webhook
to a machine you control keeps it as it arrived. When the watcher goes quiet, the heartbeat is what
tells you.

### What the watcher cannot see

The watcher judges each program as it starts, which leaves some things out of its sight. Know them
before you rely on it.

- Code that runs inside a program that is already running starts nothing new, so no rule looks at
  it. A web application whose own interpreter is taken over keeps the name and parent it always had.
- Processes running before the watcher started are never examined, and whatever runs during the
  learning week counts as normal.
- Without the root watcher, Borg sees only your own processes, and a command that ends within a
  quarter of a second can be gone before Borg looks. Services that run as another user, such as
  `www-data` or `postgres`, need the root watcher. When the root watcher stops, Borg goes back to
  watching your own processes until it runs again.
- Before Linux 5.8, or where the root part cannot load its programs, Borg reads each new process in
  `/proc` after the kernel reports it. A command that ends at once can be gone by then, and a
  process can change its command line first.
- Some kernels send no process events, as on some virtual machines and container hosts. Borg running
  as root and the root watcher then look at every process four times a second, and a short command
  can be gone before they look.
- A burst of new processes can fill the queue between the kernel and Borg, and the kernel then drops
  events, so some starts are never looked at.
- A command that ended before Borg could read `/proc` carries only what the programs read: its
  program, command line, parent and user. Its terminal, standard streams and preload variables ended
  with it, so the rules about them cannot fire. **Program deleted while it runs** fires on it only
  when its file was already gone as it started, as for a program run from memory.
- The programs keep the first 4 KiB of a command line, and Borg takes a longer one from `/proc`
  while the process is still there. They find the command line and the program's file through the
  type information the kernel publishes at `/sys/kernel/btf/vmlinux`. On a kernel built without it,
  the command line comes from `/proc` as well. A command that ended first is then named by the path
  it was started by, which can be relative to its working folder or lead through a symbolic link, so
  **Program in a temporary folder** can miss it.
- Anything running as the user the watcher runs as can stop the watcher, change its files in the
  state folder, or let a frozen process go on with `kill -CONT`. Freezing buys you time to look, and
  the process's owner can still undo it.
- A process on a terminal somebody logged in to is never frozen, even when the login used a stolen
  password or key. The rules still fire and write alerts.
- **Looking around the machine** counts the commands of one program within an hour. Someone who
  waits longer between commands, or runs each one from a different program, stays below it.
- A process that detaches from its parent before Borg freezes the parent keeps running.
- Without `[watch.notify]`, alerts reach you only when you open the page. With it, someone who
  takes over the machine in the seconds after an alert can still stop the message before it
  leaves, and then the heartbeat's silence is what tells you.

## Network

**Alt-N** or **Command → Network** shows what this machine does on the network: the ports that
are open and who can reach them, the processes that are talking and how fast, every connection,
and the network interfaces. It comes from the bundled System plugin, like the activity monitor.
The list takes the active pane, and the other pane shows the selected row in detail with charts
of the last 60 seconds. Borg samples once a second while the screen is open and stops when you
close it. Rates appear from the second sample on, and each one is the mean of the last 3 seconds,
so a list sorted by traffic holds still long enough to read.

Press **1** to **4**, or click a tab, to switch pages:

- **1 Ports** lists every port a process listens on, sorted by port. **Reach** says who can
  connect: **All networks** for a port open on every address, the address for a port bound to
  specific ones, and **This machine** for a port only programs on this machine can reach. Ports
  open to all networks stand out, since other machines can find them. **Conns** counts the
  connections that came in through the port, and the details list them with their traffic.
- **2 Processes** lists the processes that use the network, busiest first, with their download
  and upload rates and connection counts. The details add what the process moved since the
  screen opened and every connection it holds.
- **3 Connections** lists every connection with its process, the other end, and its rates. A
  wide pane adds the protocol and the TCP state. The details show both ends, the host name, the
  service, and the bytes received and sent.
- **4 Interfaces** lists the network interfaces with their kind (**Wi-Fi**, **Ethernet**,
  **Loopback**, **Tunnel**, **Bridge** or **Virtual**), first address, and rates. ● marks an
  interface that is up. The details add every address, the MTU, the bytes and packets the
  interface received and sent so far, and its errors.

The footer shows the machine's total download and upload rate over every interface except
loopback. Borg looks up the names of remote addresses in the background and shows a name in
place of the address once it is known.

| Key | Action |
| --- | --- |
| 1 / 2 / 3 / 4 | Show Ports / Processes / Connections / Interfaces |
| `/` | Filter the rows; ↑↓ still move, Enter keeps the filter, Esc clears it |
| T / N / P / C | Sort by traffic / name / port / connections; press again to reverse |
| Enter | Show the selected row's process in the activity monitor |
| K | Show the process with **Signal or priority** open |
| R | Capture the selected row's traffic, or stop the capture that runs |
| Ctrl-C | Copy the port's address, the PID, the other end, or the interface's address |
| Tab | Move between the list and the details |
| Space | Pause or resume sampling |
| Alt-W | Maximize the focused pane |
| Esc | Go back to the list, then clear the filter, then close the screen or return to the activity monitor |

P sorts the pages that have a port, and C the pages that count connections. Click a column
heading to sort by it, and double-click a row to show its process. Enter on a row without a
process moves into the details. The activity monitor that Enter and K open has the process
selected, so its memory, signals and priority are one key away (see
[Signals and priority](#signals-and-priority)). Esc there brings you back to the network screen
on the row you left, with its charts.

It works the other way too. Alt-N in the activity monitor opens this screen on the selected
process's row of **2 Processes**, with its traffic and connections in the details, and Esc goes
back to the monitor. A process without connections or open ports gets a notice that says so.

### Filtering

The filter keeps the rows that match every word you type. A plain word matches the process, PID,
port, service, address, host name, state, protocol, or a label the row shows, so `postgres`,
`5432`, `https` and `example.com` all work. A word with a field name looks in that field only:

| Word | Keeps |
| --- | --- |
| `port:443`, `port:ssh` | Rows on that port number or service |
| `proc:node`, `process:node` | Rows whose process name contains the text |
| `pid:4100` | Rows of that PID |
| `host:example` | Rows whose address or host name contains the text; a whole address keeps that address alone |
| `state:listen`, `state:time` | TCP rows in a state that contains the text |
| `proto:udp`, `proto:tcp` | Rows of that protocol |
| `if:en0`, `interface:en0` | Interfaces whose name contains the text |

Put `!` before a word to drop the rows it matches: `!state:time_wait` hides closing connections,
and `!proc:browser` hides one program. The filter applies to every page, and while you type it
says how many of the page's rows it keeps.

### Capturing traffic

Press **R** to record the packets behind the selected row with `tcpdump`, which macOS ships and
Linux installs from the `tcpdump` package. The **Capture traffic** dialog starts from that row:

- **Traffic** is what tcpdump keeps. On **1 Ports** that is the port and the connections that
  came in through it, and on **3 Connections** the one connection in both directions. On
  **2 Processes** it is **All traffic, shown for** that process: tcpdump cannot pick packets by
  process, so Borg records the whole interface and opens the result on the process. **All
  traffic on the interface** is always there as the second choice.
- **Interface** starts on the one that carries the row's address, or else the busiest one.
- **Duration** is 5, 10, 30 or 60 seconds, or **Until stopped**.
- **Keep** starts on **First 1 MiB of each connection**: each conversation's packets stay whole
  until they carried 1 MiB of data, and after that Borg saves only their headers. Short requests
  stay complete while a long download or a video call leaves the file small. **Whole packets**
  saves everything, and **Headers, first 128 bytes of each packet** saves the start of every
  packet for the smallest files. Borg trims the packets as it writes the file, which stays a
  normal pcap or pcapng, and each trimmed packet still records its length on the wire.
- **TLS keys** is **Off**, **Set SSLKEYLOGFILE in Borg's shell** to record the TLS keys of the
  programs you start while the capture runs, or **Proxy HTTPS through Borg with a temporary CA**
  for programs that do not write their keys (see [Decrypting TLS](#decrypting-tls)).
- **Run as** shows where Borg can ask the system for an administrator's password. **You** runs
  tcpdump as you, and **Administrator** runs it as root once you give your password in the
  system's dialog. On macOS the choice reads **Administrator, with each packet's process**:
  tcpdump then listens through macOS's `pktap`, which tags every packet with the process that
  sent or received it.

↑↓ pick a field, ←→ or Space change it, Enter starts the capture and Esc cancels. With the mouse,
click a field to pick it and click again to change it. While the capture runs, the footer counts its
packets and bytes and the seconds left, and **R** stops it early. A capture also stops at 200,000
packets or 256 MiB, and when the disk fills up, keeping what it saved so far. To leave room for the
rest of the system, set `keep-free-mib` in the `[network]` section of the config file: a capture
then does not start when the disk that holds the folder has less than that many MiB free, and it
stops once the free space drops below that. After its first 3 seconds, the footer says how soon the
first of these limits stops the capture at its rate so far, such as **about 30 s to 256 MiB** or
**about 2 min to a full disk**, when that comes before the capture's own end and within the hour.
When a limit stopped the capture, a notice says which one. If you close the screen first, the
capture stops, and a notice says where Borg saved it.

Borg saves each capture as `capture-<date>-<time>.pcap` in the folder of the panel the network
screen covers, readable only by you, so Wireshark and other tools can open it later. A capture
that tags each packet with its process is saved as `capture-<date>-<time>.pcapng`, the format
that can carry the tags, and so is a capture that recorded TLS keys once Borg puts them inside. A
capture holds whatever the packets carried, so keep it as safe as that data. Each capture also
goes on the list in [Borg's files](#borgs-files).

An agent connected through `borg mcp` can ask for a capture too, and it waits for your approval
(see [What the agent can do](#what-the-agent-can-do)).

When the capture ends, the traffic viewer takes the place of the live pages, with a page for each
way to look at it:

- **1 Processes** shows each process with the hosts it talked to, its packets and its bytes.
  When the packets carry their process, each packet belongs to the process that sent or
  received it. A process that lived for less than a second still gets its name then, and a
  process's filter keeps exactly its packets. Otherwise a conversation belongs to the process
  whose socket held it while the capture ran, and the rest goes to **Unknown process**.
- **2 Hosts** shows the other machines with the protocols seen. Names come from the capture's
  own DNS answers and the server names TLS connections asked for, so most hosts have one.
- **3 Connections** shows each conversation with its process, other end, protocol, packets and
  bytes. The details show both ends, the server name, the service, the bytes sent and received,
  and the conversation's stream: what each side sent, in order and with retransmitted bytes
  counted once, as a hex dump with the bytes that print as text beside them. The stream comes in
  turns, each with an arrow for its direction, the bytes it holds and when it came. When the
  capture lacks bytes before a turn, the turn says how many are missing. When it kept only the
  start of the connection, the stream's heading says how many bytes it left out after that, as
  the packets' headers count them. **]** and **[** jump to
  the next and previous turn. F3 opens the whole stream in the viewer as hex rows, with its turns
  one after another, so you can page through it and search it there. Enter shows the
  conversation's packets one by one. When Borg has the connection's TLS keys, the stream shows
  what its encrypted records carried (see [Decrypting TLS](#decrypting-tls)).
- **4 Packets** shows every packet in order with its time from the start, protocol, length and a
  summary such as `ClientHello example.com` or `A example.com → 203.0.113.10`. The details show
  its layers (Ethernet, IP, TCP or UDP, and DNS, TLS or HTTP when Borg reads them) and its bytes
  as a hex dump. A dump lays its bytes out as the viewer's hex rows do, with as many on each line as
  fit the pane. F3 opens the packet's bytes in the viewer as hex rows.

| Key | Action |
| --- | --- |
| 1 / 2 / 3 / 4 | Show Processes / Hosts / Connections / Packets |
| Enter | Go down a level: from a process or host to its connections, from a connection to its packets, from a packet into its details |
| ] / [ | In a connection's details, jump to the next or previous turn of its stream |
| `/` | Filter the rows, as on the live pages |
| Ctrl-C | Copy the PID, the address, the other end, or the packet's bytes in hex |
| F3 | Open the connection's stream or the packet's bytes in the viewer as hex rows |
| Tab | Move between the list and the details, where ↑↓ scroll |
| Esc | Go back to the list, then up a level, then clear the filter, then return to the live screen |

The filter takes the same words as the live pages, and `proto:` also matches what Borg recognized
in the packets: `proto:dns`, `proto:tls`, `proto:http` and `proto:quic`. A filter applies to every
page, and Enter sets one to go down a level, which is why Esc can step back out.

F3 on a `.pcap`, `.pcapng` or `.cap` file in a panel opens it in the same viewer, starting on
**2 Hosts**. A file recorded elsewhere has no sockets to match, so its conversations show
**Unknown process**. A `.pcapng` whose packets carry their process, as Borg and
`tcpdump -i pktap` save them on macOS, still names every process. The viewer reads files up to
256 MiB and their first 200,000 packets.

When Borg cannot find tcpdump, the capture offers to install it. The **Install tcpdump** dialog
shows the exact command for the package manager Borg found: `apt-get`, `dnf`, `pacman`, `zypper` or
`apk` on Linux, or Homebrew's `brew` on macOS and on a Linux machine with none of those. The
system's package managers run under `sudo`, or `doas` where there is no `sudo`, unless Borg runs as
root. Enter types the command into Borg's shell, where you see what it does and type your password
yourself, and Esc cancels. When the command ends, a notice says whether it worked, and Ctrl-O shows
its output. Borg looks for tcpdump again each time a capture starts, on `PATH` and in `/usr/sbin`,
`/usr/bin`, `/sbin`, `/bin` and Homebrew's `sbin` folders, so R works as soon as the install
finishes. Without a package manager Borg knows, the capture ends with **Capturing needs tcpdump,
and Borg cannot find it.**

tcpdump needs access to the capture device. Borg runs it as you, so give yourself access once:

- On macOS, only root can open the `/dev/bpf` devices at first. Install Wireshark with its
  **ChmodBPF** component once: it gives the devices to an `access_bpf` group and adds you to it,
  and from then on you can capture on every interface.
- On Linux, give tcpdump the capture capabilities once with
  `sudo setcap cap_net_raw,cap_net_admin=eip /usr/sbin/tcpdump`. Some distributions let the
  members of a `pcap` or `wireshark` group capture instead.

Without access, Borg asks **Capture as administrator** where the system can ask for your
password: on macOS in its own password dialog, and on Linux through `pkexec`, whose dialog comes
from your desktop. Choose Yes and type the password in the system's dialog, which Borg never
sees. While the dialog is up, the footer reads **Waiting for your password to capture on** and
the interface's name. Borg then runs a copy of itself as root, which checks what it was asked to
capture and starts tcpdump from the system's own folders. The packets stream back to Borg, which
saves the file as you. **R** and the duration stop the capture as before, and if Borg exits, the
capture stops with it. Cancel the system's dialog and the capture ends without a notice. To skip
the question, pick **Administrator** under **Run as** before you start.

On Linux without `pkexec`, Borg uses `sudo` when it may run tcpdump without a password. Over SSH
there is no desktop to show the dialog, so the capture ends with **Borg cannot capture as
administrator** and the system's reason. When Borg itself runs as root, it never asks, and on
macOS its captures always carry each packet's process. Where Borg cannot ask at all, the capture
ends with **No permission to capture on** and the interface's name.

### Decrypting TLS

When Borg has the keys of a TLS connection, its stream on **3 Connections** shows what the
encrypted records carried, such as an HTTP request and its answer, and the stream's heading ends
in **decrypted**.

The keys come from a TLS key log, the file that browsers, curl, Python and many other programs
write when the `SSLKEYLOGFILE` environment variable names one. Nothing sits between the program
and the server then, so this works for programs that pin their certificates too. For programs that
never write a key log, Borg can stand in the middle instead (see
[Through Borg's proxy](#through-borgs-proxy)).

To record them with a capture, set **TLS keys** to **Set SSLKEYLOGFILE in Borg's shell** in the
**Capture traffic** dialog. When the capture starts, Borg creates `capture-<date>-<time>.keylog`
next to the capture, readable only by you, and types a line like this at its shell's prompt, where
you see it run and find it later in the shell's history:

```sh
export SSLKEYLOGFILE='/home/me/projects/capture-20260101-120000.keylog'
```

In fish the line starts with `set -gx` instead. Borg then comes back to the network screen, where
the footer says **TLS keys** while the capture runs.

Now start the program from Borg's command line, or press Ctrl-O and start it in the shell. It
inherits the variable and writes its keys to the file. For a program in another terminal, press
**L** on the network screen to copy `SSLKEYLOGFILE='…'` with the key log's path, paste it there and
type the program after it. A program that was already running keeps the setting it started with,
so quit it and start it again. That goes for browsers in particular, which hand a second launch
over to the window already open. On macOS, `open` starts apps without the shell's variables, so
start the app's own program, such as `/Applications/Firefox.app/Contents/MacOS/firefox`.

If Borg's shell is busy, or the command line belongs to a container or another machine, Borg types
nothing and a notice says so. **L** still copies the line.

When the capture ends, Borg types `unset SSLKEYLOGFILE` at the prompt (`set -e SSLKEYLOGFILE` in
fish), or puts back the value Borg itself started with, and a notice names the line. If a program
you started is still running in the shell, the line waits until it ends. Borg then puts the keys
of the capture's connections into the capture as a decryption secrets block, so a `.pcap` becomes
a `.pcapng` with the same name, which Wireshark opens decrypted. The key log stays next to it,
because programs started during the capture keep writing to it until they exit. Captures an agent
asks for do not record keys.

Programs that read `SSLKEYLOGFILE` include Firefox, Chrome and other Chromium browsers, most curl
builds, and Python programs that use the `ssl` module's default context. Others need their own
switch:

- Node.js writes its keys with `--tls-keylog`, so start it as
  `node --tls-keylog="$SSLKEYLOGFILE" app.js`.
- Go programs write them only when the program sets `KeyLogWriter` in its TLS configuration.
- Rust programs on rustls write them only when they use its `KeyLogFile`.
- Java needs an agent that hooks into its TLS code.

Safari and other apps built on Apple's own TLS libraries never write a key log, and their traffic
stays encrypted.

#### Through Borg's proxy

Set **TLS keys** to **Proxy HTTPS through Borg with a temporary CA** to read the HTTPS of programs
that never write a key log, such as Go and Rust programs or a client you cannot change. When the
capture starts, Borg runs a proxy on `127.0.0.1` and makes a certificate authority for this
capture alone. Its key lives in Borg's memory and is gone when the capture ends. Borg types one
line at its shell's prompt that sends HTTPS through the proxy and has programs trust the CA:

```sh
export HTTPS_PROXY='http://127.0.0.1:54321' https_proxy='http://127.0.0.1:54321' SSL_CERT_FILE='…' CURL_CA_BUNDLE='…' REQUESTS_CA_BUNDLE='…' NODE_EXTRA_CA_CERTS='…'
```

The four certificate variables name one file in Borg's runtime folder, readable only by you, that
holds the CA's certificate followed by the system's own, so the programs still trust every other
host. While the capture runs, the footer says **Proxy on** and the proxy's address, and
**L Copy proxy settings** copies the same variables for a program in another terminal. When the
capture ends, Borg puts back what its shell had, as it does for `SSLKEYLOGFILE`, and removes the
file. Borg never adds its CA to the system's trust store.

A program that goes through the proxy asks it for a tunnel to the host. The proxy connects to the
host and checks its certificate as the system does, then shows the program a certificate for the
same host that the CA signed. The program and the host still talk HTTP/2 or HTTP/1.1, whichever
the host picked. The proxy writes the keys of both TLS sessions to the capture's key log, so the
capture's copy of the connection to the host opens decrypted, and the viewer gives it the process
that asked for it. Anything in a tunnel that is not TLS passes through as it is.

Programs follow the proxy when they read both kinds of variables:

- curl, git and Python with requests or urllib follow `HTTPS_PROXY` or `https_proxy`, and trust
  the bundle through `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, or `SSL_CERT_FILE` where they use
  OpenSSL.
- Go follows `HTTPS_PROXY`, except for requests to `localhost`, and reads `SSL_CERT_FILE` on
  Linux. On macOS it trusts the system's certificates only, so its connections fail there.
- Node.js trusts the CA through `NODE_EXTRA_CA_CERTS`. Recent versions send `fetch` and `https`
  through the proxy when you also set `NODE_USE_ENV_PROXY=1`.
- Programs with their own certificate list, those that pin certificates, and apps that take the
  proxy from the system settings, such as Safari, refuse or bypass the proxy. A refusal shows as
  **A program did not trust the proxy's certificate for** and the host.

When the host's certificate does not check out, the proxy closes the connection and a notice says
**Borg could not verify** and the host. That is what happens with your own services when their
certificates come from a development CA the system does not trust, such as the one
[mkcert](https://github.com/FiloSottile/mkcert) makes. Point `proxy-ca` in the `[network]` section
of `config.toml` at that CA's folder:

```toml
[network]
proxy-ca = "~/Library/Application Support/mkcert"  # mkcert -CAROOT prints the folder
```

The folder must hold the certificate and its key as `rootCA.pem` and `rootCA-key.pem`, as mkcert
names them, or as `ca.pem` and `ca-key.pem`. The choice in the dialog then reads **Proxy HTTPS
through Borg with the CA in** and the folder. The proxy signs with your CA, which your programs
already trust, and it trusts the CA too when it checks your services. Use a CA that exists for
development only, because whoever has its key can read the traffic of everything that trusts it.

F3 finds keys in two places:

- A decryption secrets block inside a `.pcapng` file, where Wireshark keeps them too.
- A key log beside the capture with the same name and the `.keylog` extension, such as
  `capture-20260101-120000.keylog` next to `capture-20260101-120000.pcap`.

To decrypt a capture you made another way, run the program with the variable set during the
capture, and once the capture is saved, rename the key log so its name matches the capture's:

```sh
SSLKEYLOGFILE=~/tls.keylog curl https://example.com
```

Borg opens TLS 1.3 and TLS 1.2 records sealed with AES-128-GCM, AES-256-GCM or
ChaCha20-Poly1305, the ciphers browsers and current servers agree on. A record that no key opens
keeps its encrypted bytes, and its turn says **still encrypted**. That happens when the key log
lacks some of the connection's keys, and after bytes the capture lacks, because Borg cannot tell
where the next record starts. QUIC traffic stays encrypted.

A key log decrypts every connection it has keys for, passwords, cookies and tokens included, and
so does a capture with the keys inside. Borg makes both readable only by you. Do the same for a key
log you write yourself with `chmod 600`, keep key logs as safe as the captures, and delete both when
you are done.

### What the system reports

On macOS, Borg reads every socket with its byte counts and process from `netstat`, so traffic
appears for TCP and UDP alike. Sockets of other users' processes are listed too.

On Linux, Borg reads the sockets from `/proc/net` and finds each socket's process through the
processes' open files. Without administrator rights it can only look into your own processes, so
other users' sockets show **Unknown process**. TCP byte counts come from `ss` when it is
installed; UDP sockets and machines without `ss` show **—** for rates, and the details say
**The system reports no byte counts here**. Interface totals and rates always work.

## Startup items

**Alt-U** or **Command → Startup items** lists everything set to start on its own: at boot, at
login, on a schedule, or whenever a shell or an SSH login starts. The newest change comes first,
so whatever was added or edited lately sits at the top. Like the activity monitor, it comes from
the bundled System plugin. The list takes the active pane, and the other pane shows the selected
item in detail.

Each row shows when its file last changed, its place, and what it runs. A wider pane adds who owns
the file and whether its program is there: **found**, or **missing** when the path it names does
not exist. A missing program is worth a look, since whatever puts a file there later runs with
the item's rights. The **Rules** column names the rules an item matched, colored by their level,
and appears once any item matched one. The line above the list counts the items and how many
matched a rule.

The details show the file and its line, what the item runs, the program and where Borg found it,
when it runs in the file's own words (`RunAtLoad`, `@reboot`, a calendar or a systemd target),
the user it runs as, the file's owner and the exact time of the change. Under **Rules**, each
match says whether it was **in its file** or **in its program**, with what the rule looks for, when
it matches harmless things, and where the rule comes from. An item its file turns off is dimmed,
and its details say **Turned off in its own file, so it does not start.**

| Key | Action |
| --- | --- |
| Enter | Close the screen and show the item's file in the panel |
| F3 / F4 | View / edit the item's file |
| `/` | Filter the rows; ↑↓ still move, Enter keeps the filter, Esc clears it |
| R | Read every place again |
| Ctrl-C | Copy what the item runs |
| Alt-W | Maximize the list |
| Esc | Clear the filter, then close the screen |

Double-click a row to show its file in the panel. The filter keeps the items that contain every
word you type, in the place, the name, what it runs, the file's path, the owner, the user it runs
as, when it runs, **found** or **missing**, and the names of the rules it matched. Put `!` before a
word to leave out what it matches: `root !timer` keeps root's items other than timers. Each place
also has a short name for the filter and for agents, such as `launch-daemon`, `systemd-timer` or
`ssh-authorized-key`.

### Where Borg looks

On macOS:

- **User agent**: `~/Library/LaunchAgents`.
- **Launch agent** and **Launch daemon**: `/Library/LaunchAgents` and `/Library/LaunchDaemons`.
- **System agent** and **System daemon**: `/System/Library/LaunchAgents` and
  `/System/Library/LaunchDaemons`, on the sealed system volume.
- **Cron job**: the crontabs in `/usr/lib/cron/tabs`, and `/etc/crontab`.
- **Periodic script**: the daily, weekly and monthly scripts in `/etc/periodic` and
  `/usr/local/etc/periodic`.

On Linux:

- **Service** and **Timer**: systemd units in `/etc/systemd/system`, `/usr/local/lib/systemd/system`,
  `/usr/lib/systemd/system` and `/lib/systemd/system`, and the units their `.wants` and
  `.requires` folders link to from anywhere else.
- **User service** and **User timer**: `/etc/systemd/user`, `/usr/local/lib/systemd/user`,
  `/usr/lib/systemd/user`, `~/.config/systemd/user` and `~/.local/share/systemd/user`.
- **Autostart**: desktop entries in `~/.config/autostart` and `/etc/xdg/autostart`.
- **Cron job**: `/etc/crontab`, `/etc/cron.d`, and the crontabs in `/var/spool/cron/crontabs` or
  `/var/spool/cron`.
- **Cron script**: `/etc/cron.hourly`, `/etc/cron.daily`, `/etc/cron.weekly` and
  `/etc/cron.monthly`.
- **Anacron job**: `/etc/anacrontab`.
- **rc.local**: `/etc/rc.local`.
- **Preload**: `/etc/ld.so.preload`, the libraries loaded into every program that starts.
- **Shell startup**: the scripts in `/etc/profile.d`, besides the files below.

On both, **Shell startup** covers the zsh, bash and fish startup files in your home folder, such
as `~/.zshrc`, `~/.bashrc`, `~/.profile` and `~/.config/fish/config.fish`, and their system-wide
versions in `/etc`. **SSH key** lists each key in `~/.ssh/authorized_keys` and
`~/.ssh/authorized_keys2` with its comment, and with the command it forces when it has one.

Borg reads these places as you. A place that is there but closed to you shows at the bottom of
the list as **Could not read** with its path; run Borg as administrator to see inside. Your own
crontab is the exception: when the folder of crontabs is closed to you, as it is on macOS, Borg
asks `crontab -l` for it.

A program named with a full path or `~/` counts as **missing** when nothing is there. A bare name
is looked up in `/usr/local/sbin`, `/usr/local/bin`, `/usr/sbin`, `/usr/bin`, `/sbin` and `/bin`,
where cron and launchd look. A relative path, or a name found in none of them, shows **—**, since
the item may set its own search path.

### Rules on startup items

Borg runs its own rules and yours (see [Scanning files with rules](#scanning-files-with-rules))
over the first 1 MiB of each startup file and over each program up to 64 MiB. A program many items
run is scanned once. Programs of **System agent** and **System daemon** items are left unscanned,
because nothing can change the sealed system volume while macOS runs, and their details say so.
When a rule file fails to compile, the bottom of the list says how many were left out.
