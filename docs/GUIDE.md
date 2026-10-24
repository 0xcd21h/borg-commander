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

The installer verifies the archive checksum and installs `borg` in a folder on your PATH,
so it runs in the same terminal: `~/.local/bin` when it is on your PATH, `/usr/local/bin`
when that is writable (as for root, in a container), and otherwise `~/.local/bin`, which
it adds to your shell's startup file. Then it starts borg; `BORG_NO_RUN=1` skips that, and
`BORG_INSTALL_DIR` picks the folder. Run again, it skips the download when that version
is installed. To build from source, use Rust 1.97 or newer:

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
- **Alt-K:** collect marked files, or the current file, in the other panel. Adding a
  reference does not copy the file's contents.
- **Alt-V:** choose a saved collection from a scrollable list with entry counts and
  size estimates. Press F7 or N to create one. Collections survive restarting Borg.
- **Esc:** hide results, preserving the cursor, scroll, marks, filter, and grouping.
- **Alt-R:** resume the hidden view. A scan continues while hidden.
- **Alt-D — Folder sizes:** find folders by their total size or file count, including
  hidden files by default. It opens the Ctrl-F dialog set to *Folder totals*; switching
  *Look for* moves between the two. Ctrl-C stops a scan.
- **Alt-S:** calculate sizes for the selected folder. **Alt-Shift-D:** open a size tree for the current folder.
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
rows overlap. Symlinks are never followed, scans stay on the starting filesystem, and a
folder that contains itself (through a bind mount or a hard-linked folder) is entered only
once; such boundaries and unreadable or excluded children make totals incomplete, shown
with ≥. Directory scans use metadata and do
not read file contents. Results appear after measurement; the scan remains cancellable.
**Folder size tree** shows the root total and its subfolders in the current panel.
Right expands a branch or enters an expanded branch; Left folds it or selects its
parent. Enter opens the real folder. Click a branch arrow to fold or expand, or a
column heading to sort siblings by name, size, or file count. Esc hides the tree;
Alt-R restores its branches, marks, and cursor. File operations use real paths,
and marking a parent and child sends only the parent to a copy or delete job.

To measure sizes while keeping ordinary file panels open, press **Alt-I** or
choose **Search → Count sizes in background**. Borg scans every folder beneath the
current directory, includes hidden files, and fills in folder totals as they finish.
You can navigate or use the console while it runs. Folders still being measured show
a growing running total marked `≥`, and totals stay when you leave a folder and come
back. The panel footer shows progress; **Alt-I** again, or a click on the footer, stops
the scan and keeps the totals measured so far. Starting a copy, move,
or delete job stops an ongoing size scan. Finished scans share their cache with the
size tree and Space analysis.

Completed scans are saved locally. Both Space analysis and the folder tree reuse
a saved scan for the same root and hidden-file policy. Reopening a measured root uses that snapshot
immediately: `~` marks cached sizes, with the measurement date above the tree.
**Ctrl-R** rescans in either size view; **Ctrl-C** cancels. The previous tree stays visible during a
refresh and survives cancellation or failure. Ordinary folder listings also show
available cached totals. Changes outside Borg can make them outdated; navigation
does not trigger recursive scans. Borg copy, move, and delete jobs invalidate
snapshots that overlap their changed paths.

The local SQLite index retains every measured folder for up to 16 roots. Trees
load children as branches expand; a branch displays up to 20,000 folders, and flat
Space analysis keeps the largest 20,000 matches. These display limits do not stop
the scan or reduce measured totals. A “Files in this folder” row accounts for bytes
stored directly in an expanded folder. Errors and skipped subtrees mark totals as
incomplete. Cache files live in `~/.cache/borg-commander`, respecting `XDG_CACHE_HOME`; `BORG_CACHE_DIR`
overrides the location. They are disposable measurements, separate from collections.
Hard-linked files count once per path. These are logical content sizes, not physical
or recoverable disk usage. Indexed and content search are planned separately.

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
| Alt-I | Count folder sizes in the background / stop counting |
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
| Alt-M | Activity monitor: processes and memory |
| Alt-C | Docker: Compose projects and containers |
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
While Borg waits for the digit, the function key row shows the digits to press. Esc first
closes what is open (a notice, results, the quick view or Info); a second Esc cancels.
Alt-1–0 also works when Alt reaches the terminal. On macOS, enable Option as Meta in your
terminal if needed.

## Mouse and console

Click a row to focus it, double-click to open it, and right-click to mark it. Click the menu
bar or bottom actions to use them. The wheel moves through panel entries.

Ctrl-O switches between panels and console. The shell follows the active panel's folder
before you type or open the console. In zsh it changes folder silently and redraws the
prompt; in bash and fish a short `cd` command appears in the console instead. Commands and
Ctrl-C keep an explicitly opened console visible. “Return to panels after command” applies to commands launched
from the panels. At the shell prompt, Option/Alt-Left and Option/Alt-Right move by words.

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

## Docker plugin

**Alt-C** or **Command → Docker** lists your containers in the active panel, grouped by
Compose project, with containers outside Compose last. The other panel shows the selected
container: its status, ports, networks, mounts, command, and restart policy, with live CPU,
memory, network and disk use and a chart of the last minute. On a project it shows the
project's folder, its containers, and their combined use. Esc closes the view.

| Key | Action |
| --- | --- |
| ↑ ↓, PgUp, PgDn, Home, End | Move |
| ← → / Enter | Fold or unfold a project; Enter on a container focuses its details |
| S / X / R | Start / stop / restart the container, or the project's containers |
| P | Pause or resume |
| F | Browse the container's files |
| E, Alt-2 | Open a shell in the container, or return to its open shell |
| Alt-1 | Show the container's overview again, also from inside its shell |
| `/` | Filter by name, service, image or project |
| Tab | Switch between the list and the details |
| Alt-W | Maximize the focused pane |

Stopping and restarting ask first. On a project, an action only touches the containers it
changes, so Start starts the stopped ones.

**F** on a running container closes the Docker view and shows the container's filesystem in
that panel, starting in its working directory. The panel title names the container. The
other panel keeps its host folder, so **F5** copies either way, and **F3** views, **F7** makes
a folder and **F8** deletes inside the container. Searches, sizes, previews and editing stay
on the host. **Alt-C** returns to the Docker view; `..` at the container's root returns the
panel to the host folder it showed before. Browsing needs `sh` in the container, so minimal
images without a shell cannot be listed.

Under a container's files, the command line follows the panel: it becomes that container's
shell, started in the panel's folder, with the container's name at the start of its prompt.
Commands run inside the container, the panel refreshes when they finish, and a `cd` there
moves the panel. Ctrl-O shows that container's console, marked **▣ name console**; on a host
panel, the command line and Ctrl-O are the host's again, with whatever you typed there kept.
The container's shell is the same one as its Shell tab in the Docker view.

Turn off **Docker — command line follows container panels** in Options → Plugins to keep the
command line on the host. Under a container's files it is then dimmed and marked **⌂ host**,
and Enter runs nothing until a host panel is active, so a command never runs on the wrong
machine.

**E** opens a shell in the container (bash when the image has it, otherwise `sh`) on the
details panel's Shell tab, and sends your keys to it, function keys included. **Alt-1**
shows the overview and **Alt-C** returns to the list; the mouse wheel scrolls its history. A shell keeps running while you look at other containers,
close the Docker view or use the host console, and its container is marked `›_` in the list
and counted on the Alt-C shortcut. It closes when you exit it, when its container stops, or
when Borg quits, which asks first while shells are open. The plugin uses the `docker` CLI and its current
context, so it works with Docker Desktop, OrbStack, Colima and remote engines alike. It can
be turned off in Options → Plugins.

## Claude and other agents (MCP)

`borg mcp` lets an AI agent such as Claude work in the borg you have open, on your machine or
on a server you reach over SSH. Nothing needs installing beside borg: the agent starts
`borg mcp` itself, through SSH for a server, and it connects to the borg session open there.
The server needs no open port and no internet access.

To connect, press F1 and then C (or F9 → Command → Remote access…). Borg shows two
commands to paste into a terminal on your computer: one starts Claude Code already connected
to this borg, the other keeps the connection in Claude Code for every session. Tab picks one,
Enter copies it to your computer's clipboard through the terminal, which works over SSH in
iTerm2, Ghostty, kitty, WezTerm and Alacritty (in tmux, with `set -g set-clipboard on`).
Over SSH, borg fills in the address you came in on; if your computer reaches the server by
another name, such as an SSH alias or through a jump host, type that name instead. SSH must
log in without a password prompt: a key, or an SSH agent. `borg mcp setup prod` prints the
same for Claude Code and Claude Desktop from the command line.

Connected, the agent sees what you see: the screen itself as text, whether you are in the
panels or the console, both panels, the highlighted and marked files, where the command line
runs and what is typed there. It names places as locations: an absolute path or `~/…` on the
machine borg runs on, `docker://web/etc/nginx` inside a container, or a path relative to the
active panel. Without asking, it reads folders, files (by line ranges, or the lines that
contain some text), the busiest processes and the containers, and shows a file or folder in
your other panel.

Anything that changes the machine waits for you in a dialog that shows all of it: a
command and where it runs, a new file, or an edit as a diff. Long ones scroll with the arrow
keys, PgUp and PgDn or the mouse wheel, and F3 shows the dialog full screen. No is selected
until you choose Yes; an edit is refused if the file changed while you were deciding.
Commands are typed into a shell you watch, and kept out of its history: commands on this
machine into borg's own console, which shows while they run, so you can answer a prompt
such as sudo's; commands in a container into that container's shell in the Docker view.
The shell keeps its folder and variables between commands.

The agent can also type text for you to review: a command at the prompt, or a script into
vim or nano open in the console. Nothing runs: no Enter is pressed, and text with several
lines goes in as a paste, which shells and editors insert without running.

### Borg cloud

Borg cloud lets Claude reach a borg it has no SSH route to: a server you open in a browser
terminal, a container, a machine behind NAT. Both sides connect out to a relay, and the relay
joins them.

1. In borg on the server, press F1 then C. The first choice is `borg pair` with a link; Enter
   copies it. The link works once, for ten minutes. Under it is the relay: paste another
   relay's address there and press Enter to save it and get a link through it.
2. Run it in a terminal on your computer, where borg is installed too. It prints a pairing
   code: six emoji, each with its name, such as `🐱 Cat · 🦁 Lion · 🐎 Horse`.
3. Borg on the server asks whether to pair, showing the computer's name and the code. Choose
   Yes if the codes match. `borg pair` then offers to add the server to Claude Code, as
   `claude mcp add borg-NAME -- borg mcp remote NAME`.

The code comes from the encrypted handshake the two borgs just finished, so it is the same on
both screens only when your computer is the one talking to the server. Someone who got hold
of the link and used it first would show the server a different code, and cannot pick one
that matches yours. Compare the names where a terminal draws no emoji.

From then on, Claude reaches that borg whenever it is open there. `borg pair` alone lists the
paired servers, and `borg pair --forget NAME` forgets one; on the server, F8 in the Remote
access dialog forgets every paired computer. Through borg cloud Claude also gets `download` and
`upload`, which copy files between the server and your computer; `borg get NAME:PATH [TO]` and
`borg put FROM NAME:PATH` do the same from your terminal. Uploads wait for your approval in
borg. Every approval names the computer that asked.

The connection is encrypted end to end. Each borg keeps a private key, readable only by you,
in its state folder. Pairing exchanges the public keys, and every connection opens with a
Noise IK handshake bound to them, so the relay forwards only ciphertext and cannot answer in
either borg's name. Calls from another computer can read without asking, like local ones;
Options → Plugins → "ask before another computer reads" makes those reads wait for approval
too. Borg keeps no connection to the relay unless it is paired or showing a pairing link, and
none at all when borg cloud is off in Options → Plugins.

Borg saves the relay in the config, as `relay = "wss://relay.example.com"`. A computer
reaches the server through the relay it paired through; after the server changes relay,
pair the computer again.

While an agent is connected, **◈ Remote** sits at the start of the shortcut row; click it
for the log of what it did: what it looked at, read, showed and typed, and every command
and change, approved or declined. Borg keeps the log in memory only, until it exits; to
keep no log at all, turn it off in Options → Plugins.

### Running a relay

`borg-relay` is the relay borgs meet through. On a Linux server whose domain points at it,
with port 443 free, one command installs it as a service with a TLS certificate from Let's
Encrypt:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install-relay.sh | sudo sh -s -- relay.example.com you@example.com
```

Run it again to update.

On Dokploy, Coolify or another Docker host whose proxy adds TLS, run the image
`ghcr.io/0xcd21h/borg-relay:latest` and give it a domain on port 8080: in Dokploy, an
application from that Docker image with a domain on container port 8080 and HTTPS on; in
Coolify, a resource from that Docker image with port 8080 exposed and a domain. Either also
takes [packaging/relay/docker-compose.yml](../packaging/relay/docker-compose.yml) as a compose
service. The image runs `borg-relay --behind-proxy`: the proxy serves HTTPS and passes
WebSockets through, and the relay counts its limits by the client address the proxy reports
in `X-Forwarded-For`. It runs as an unprivileged user and has a health check (`GET /health`).

On a Docker host without such a proxy, the same image serves TLS itself:

```sh
docker run -d --restart unless-stopped -p 443:443 -v borg-relay:/data ghcr.io/0xcd21h/borg-relay --domain relay.example.com --cache /data
```

Behind another proxy that adds TLS, run `borg-relay --behind-proxy --listen 127.0.0.1:8080`;
only use it behind a proxy, since the relay trusts the client address the proxy reports. The relay keeps limits per address, on servers and sessions, on
frame size, bandwidth, bytes per session, idle time and session length. It stores nothing
but its certificate.

The relay logs counts only: once a minute, when they changed, a line such as

```text
stats: connections=3 servers=2 sessions=1 joined=14 carried=3.2MiB limited=0
```

with the connections it holds, the servers registered and the sessions open, and since it
started, the sessions joined, the bytes they carried, and the connections and sessions a
limit refused or ended. For debugging, `--log`, or the environment variable
`BORG_RELAY_LOG=1` for the image, also logs each server's registration by the start of its
key, each session with the bytes it carried, and each limit reached with the client's
address.

## Find, jobs, and viewers

Ctrl-S opens quick find. Type a name or glob such as `*.rs`; Down and Up move between matches,
Insert marks a match, Ctrl-A marks all matches, and Enter or Esc closes the search.
Matching is case-insensitive unless the query contains uppercase letters.

F5, F6, and F8 start copy, move, and delete jobs. Alt-J opens the jobs list to pause or cancel.

The Copy dialog has an optional **Optimize copying for hard drives** checkbox.
Use Tab to focus it and Space to toggle, or click it. It starts unchecked for each copy.
This mode reads small files into memory in batches, then writes each batch, which may
reduce source/destination seeking when copying within one HDD. Batches hold up to
64 MiB of file data or 4,096 files; files larger than 1 MiB stream normally. Performance
depends on the drive, filesystem, and OS caching; it may be slower between separate drives.
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
```

On a local terminal with true colour, Borg opens with a short glitch intro of its logo;
any key or click skips it. Over SSH and on terminals without true colour it starts straight
in the panels.

Panel folders, cursor position, split, listing mode, and sorting are saved in
`~/.local/state/borg-commander`, or the location selected by `$XDG_STATE_HOME` or `$BORG_STATE_DIR`.

While it runs, Borg keeps shell integration scripts and files copied out of containers in
`$XDG_RUNTIME_DIR/borg-commander`, or `borg-commander-<uid>` in the temporary
folder. Only you can open it; one that someone else made, or that others can enter, is
passed over for a fresh name. Its `TTL` file starts with the time until which it is in use:
every running borg moves that time a day ahead each hour, so once it has passed, the folder
can be removed.

Custom themes go in `~/.config/borg-commander/themes/`. A theme can extend a built-in theme:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

For building, testing, or contributing, see [Development](../DEVELOPMENT.md).

## A small distraction

Open About with F1 and type `2048` to play in a modal. Arrow keys move, U undoes the
last move, R starts a new game, and Esc closes it. Reach 2048, then press Enter to
keep playing. The board and best score are saved between sessions.


## Activity and memory

**Alt-M** or **Command → Activity monitor** opens the process monitor. The bundled
System plugin is enabled by default and can be disabled in Options → Plugins.
The process list occupies the active pane, with the selected process's memory
overview opposite it. **Enter** or a double-click opens that process in a window over
the monitor: its memory map on one side and a preview of the selected region's bytes on
the other. **Esc** closes it. **Tab** switches focus. **Alt-W** or the **[+]** button maximizes
the focused pane; **[=]** restores the split, and **[-]** hides the monitor.
Sampling runs once a second while the monitor is open; closing it stops sampling.
Memory inspections temporarily hold sampling until their result arrives.

The list shows CPU, resident memory, memory growth, and disk read/write rates.
CPU 100% means one busy core. Shared resident pages can appear in more than one
process, so process RAM values must not be added as a unique system total.
The first CPU and I/O rate sample is unavailable until a second observation arrives.

| Key | Action |
| --- | --- |
| Enter | Open the process memory map / focus the region preview |
| `/` | Filter by name or PID; ↑↓ still move, Enter keeps the filter, Esc clears it |
| C / M / G / N / P | Sort by CPU / RAM / growth / name / PID; repeat to reverse |
| I / O | Sort by read / write rate |
| T | Toggle process tree |
| Space | Pause or resume sampling |
| R | Refresh the current view |
| Esc | Back; close from the process list |

Click process headings to sort; double-click a process to open its memory map.
The overview shows current resident memory, peak observed memory, the change since
monitoring started, CPU, virtual address space, and a chart of the last 60 seconds of
resident memory. Footprint, swapped, private and shared totals appear once the memory
map has been read and the platform reports them. **Enter** in the memory pane opens its
memory map. The memory map is a measurement taken on request. Refresh it with R after
the process changes its mappings.

The map has three columns:

- **Footprint** is the memory charged to this process alone: pages it has written plus
  pages swapped out or compressed. It is the figure to watch for leaks, and on macOS
  it matches Activity Monitor's *Memory* column.
- **Resident** is pages in RAM now, including pages shared with other processes.
- **Mapped** is address space, including reservations. It is not RAM.

A process can map gigabytes, mostly system libraries shared with every other
process, while its footprint is a few megabytes. Regions start grouped by purpose:
heap and allocations, thread stacks, the program's own code and data, shared
libraries, mapped files, shared memory, system and other allocations, and reserved
ranges and guards. Groups and their members are ordered by footprint, with
reservations last. Libraries and mapped files list one row per file; expand a file to
see its regions. Group names are inferred from the operating system's labels.
Selecting a group or file explains it and shows its share of the footprint and its
largest members. Enter or Right expands a group or file; Left collapses it, or the one
around the selected row. G switches between grouped and raw lists.

**1 Memory** and **2 Open files** switch inspection tabs, or click their labels.
The open-files list includes descriptor, access mode, type, and path or endpoint;
the other pane shows full details, file size, and current offset when available.
On macOS it also includes entries such as `cwd` (current directory) and `txt`
(mapped executable). R refreshes the list. Switching tabs retains list positions.
Access restrictions offer the existing explicit administrator retry. File contents
are not read through process descriptors.

Selecting a readable region automatically captures up to its first 64 KiB in the
opposite pane. Quick movements are coalesced with a short delay; selecting another
region clears the old preview and cancels its pending read. Unreadable regions show
a message without attempting a read. Press **Tab** or **Enter** to scroll the preview.
**Esc** there focuses the list; **Esc** from the map returns to the process list
with its cursor and filter preserved.

**H** switches hex/text presentation, **R** captures the region again, and **D**
highlights differences from the previous capture of the same process and address.
Text presentation shows printable ASCII and substitutes dots for other bytes.
Snapshots are live observations; the process is not paused, so a capture is not
an atomic image of its entire state. They stay in memory unless explicitly exported.
**E** exports raw bytes into the active file panel's directory, using a new filename
and owner-only file permissions. Existing files are never overwritten.

Press **/** in the memory pane, or **Ctrl-F** from either pane, to search memory.
Choose a mode, case matching, and either readable process memory or the current
snapshot. **Text** searches UTF-8, UTF-16 (both byte orders), and UTF-32 (both byte
orders). **Unicode** also normalizes compatibility characters and combining
sequences: for example, `𝐁𝐨𝐫𝐠` matches `Borg`. This does not equate unrelated letters
that merely look similar. **Hex** accepts byte pairs such as `42 6F 72 67`.

Matches occupy the list pane and show their original address, encoding, and region.
Selecting a match previews its surrounding bytes in the opposite pane, with matched
bytes highlighted. Enter focuses the preview; Esc returns focus to the matches.
Esc from matches restores the previous map or process list. Searching
a stored snapshot uses its captured bytes without reading the running process.
Process searches read in chunks, report inaccessible bytes, and stop after 2,000
matches or 60 seconds. **N** continues from that position; **Ctrl-C** stops while
preserving the results. Coverage reports distinguish searched and inaccessible
bytes. A process can change its memory while it is being searched.

Restricted memory-map or snapshot reads, and partially inaccessible searches,
offer **A: Administrator**. On macOS this
opens the system authorization prompt; on Linux it uses `pkexec` and needs an
available authorization agent. Only a short-lived inspection helper is elevated.
Borg keeps running normally, and the helper only returns map data or bounded bytes.
macOS process protections and Linux security policy can still deny access; Borg
shows the failure and leaves the ordinary process monitor usable.

Process identity includes its start time, so a reused PID does not inherit another
process's history. Memory maps, inspection output, snapshots, and history are bounded.
Borg Commander stays visible in the bottom-right corner, including the console,
viewer, monitor, dialogs, and screensaver.
