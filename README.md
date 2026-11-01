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
  <strong>A two-panel file manager with your shell built in.</strong><br>
  macOS &amp; Linux · Bash, zsh &amp; fish
</p>

<p align="center">
  <a href="https://github.com/0xcd21h/borg-commander/releases/latest">Download</a> ·
  <a href="#install">Install</a> ·
  <a href="https://0xcd21h.github.io/borg-commander/">Website &amp; guide</a> ·
  <a href="DEVELOPMENT.md">Development</a>
</p>

<p align="center">
  <img src="docs/demo.gif" alt="Borg Commander starting with its glitching logo, then browsing files, previewing code, inspecting a Git diff, and switching to the shell" width="1100">
</p>

<p align="center"><em>Resistance is futile.</em></p>

## Why Borg

There have always been good terminals and good file managers. Programs that are both are rare:
two file panels with your shell running underneath, one Ctrl-O away. Norton Commander made that
layout famous on DOS. Volkov Commander, DOS Navigator, FAR Manager and Midnight Commander kept it
going, but most of them stayed in the DOS and Windows console era.

Borg Commander is a direct continuation of those projects for current macOS and Linux. It keeps
the keys you remember: F3 views, F4 edits, F5 copies, F6 moves, and Ctrl-O switches between the
panels and your bash, zsh or fish. On top of that it brings Git, Docker, remote sessions and
access for Claude and other agents into the same two panels.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

The installer verifies the checksum, puts `borg` in `~/.local/bin` (or `/usr/local/bin` as
root), adds it to your PATH if needed, and starts it. Update with `borg update`. Binaries for
**macOS** (Apple Silicon or Intel) and **Linux** (ARM64 or x86-64) are on
[Releases](https://github.com/0xcd21h/borg-commander/releases/latest).

[Controls, settings, and source installation →](docs/GUIDE.md) ·
[Build, test, and contribute →](DEVELOPMENT.md)

## Claude in any terminal

Open borg on a server, in a container, or in a terminal in your browser, and Claude on your
computer works there with you. Borg connects out to borg cloud, a relay, so the machine needs
no open port, no SSH key and no public address. Firewalls and NAT stay as they are.

- **Pair once.** F1 then C shows a link. Run `borg pair` with it on your computer and approve
  the computer in borg. Claude Code then reaches that borg whenever it is open.
- **Claude sees what you see.** The screen, both panels, the console and what is typed at the
  prompt. It reads files, folders, processes and containers, and shows you files in a panel.
- **You approve every change.** Commands, new files and edits wait in a dialog that shows the
  whole command or the diff. For a paired computer, Approve all can temporarily accept its
  changes for one session until you turn it off or disconnect. Commands run in your console, where you watch them and answer
  prompts such as sudo's.
- **Files both ways.** `borg get` and `borg put`, or Claude's download and upload tools, copy
  files between the server and your computer. Uploads wait for your approval.
- **Help at the prompt.** Claude can type a command at your prompt, or a script into vim or
  nano, for you to check and run yourself.

`borg sessions` manages named workspaces. `borg service install` prepares a background service;
`borg service enable` starts it at boot, and `borg service start` starts it now. The service keeps
session shells and the pairing connection alive when no terminal is attached. Open a workspace
with `borg --session NAME`; Ctrl-] detaches. One `borg mcp` connection can list and select local
or paired targets. `borg --remote MACHINE:SESSION` opens a paired session's filesystem in a local
panel with F5 copies to and from local folders, and `borg mirror MACHINE:SESSION` shows its live screen in the local terminal.
Regular sessions can be mirrored while Borg is running, without a background service.
The mirror displays its control status; Ctrl-T takes control from a view-only mirror, and Ctrl-G exits.

**Why the connection cannot be taken over.** Each borg holds its own key. Pairing exchanges
the keys through a link that only your screen shows, and you confirm in borg that both
screens show the same pairing code, six emoji with their names. Every connection then opens with a Noise handshake bound to those keys,
and everything after it is encrypted from one borg to the other. The relay forwards bytes it
cannot read. Whoever runs the relay, or breaks into it, can neither read nor change the
traffic, nor answer in either borg's name: the private keys never leave the two machines.

**Fair use.** borg cloud is a free relay shared by everyone who uses borg. It carries borg's
traffic only, within limits on bandwidth and connections, and it may change or stop without
notice. Use it for machines you own or are allowed to work on. To keep the relay under your
own control, [run your own](docs/GUIDE.md#running-a-relay).

## Files and a real shell

Keep two folders side by side and type commands at the prompt below. Borg runs your
Bash, zsh, or fish session, with its history, completion, and multiline editing.
Ctrl-O switches between the panels and the console.

- **Move files while you keep browsing.** Copy, move, and delete in background jobs,
  with progress, pause, and conflict handling. Mark a few files or select a whole pattern.
- **Look inside without leaving the folder.** Preview highlighted code, formatted Markdown,
  and images. Scroll a document in quick preview, expand it into the viewer, and keep your place.
  Large text files load around the visible page.
- **See Git beside your files.** Branch names and file status in the panels; diffs, recent
  commits, staging, and unstaging in the Git menu. The bundled plugin can be switched off.
- **Use the mouse or stay on the keys.** Click menus, select files, and scroll previews.
  Familiar function-key actions also work with Esc followed by a digit.

## Find what takes space

<p align="center">
  <img src="docs/showcase/size-tree.png" alt="A size tree of a project, largest folders first, beside a panel showing measured folder sizes" width="1100">
</p>

- **Sizes while you browse.** Alt-I measures every folder below the current one in the
  background. Totals fill in as they finish, and you can keep navigating or typing commands.
- **A size tree.** Alt-Shift-D opens the folder as a tree, largest branches first, with a
  row for the bytes stored directly in each folder.
- **Find the heavy folders.** Alt-D searches for folders over a size or file count, such as
  every `node_modules`, and lists them with their source paths. Mark them and delete with F8.
- **Measured once, shown again.** Finished scans are saved locally and reused by the panels,
  the tree, and the search, marked with the date they were measured. Ctrl-R rescans.

<p align="center">
  <img src="docs/showcase/folder-totals.png" alt="Folder size search listing every folder over 200 MB across several projects" width="1100">
</p>

## Processes and memory

<p align="center">
  <img src="docs/showcase/activity.png" alt="Activity monitor with a process list and a chart of the selected process's resident memory over the last minute" width="1100">
</p>

- **A live process list.** Alt-M shows CPU, RAM, memory growth, and disk rates, sorted by
  any column or as a tree. The other pane charts the selected process's memory over the last minute.
- **Where the memory goes.** Enter opens the process's memory map, grouped by purpose and
  ranked by **footprint**: the memory charged to that process alone, separate from shared
  libraries that only make it look large.
- **Look inside.** Preview any readable region as hex or text, compare two captures, search a
  process's memory for text or bytes, and list its open files. Administrator access is only
  requested when you ask for it.

<p align="center">
  <img src="docs/showcase/memory-map.png" alt="Memory map grouped by purpose with footprint, resident and mapped columns, explaining that shared libraries are not the process's own cost" width="1100">
</p>

## Containers

<p align="center">
  <img src="docs/showcase/docker.png" alt="Docker view listing Compose projects and their containers with live CPU and memory, beside the selected container's details and charts" width="1100">
</p>

- **Your Compose projects at a glance.** Alt-C lists containers by project with live CPU and
  memory. S, X, R and P start, stop, restart and pause a container or a whole project.
- **Step inside.** F opens a container's files in a panel beside a host folder, and F5 copies
  either way. E opens a shell, and the command line follows you into the container.

## Themes

Six built-in themes, from Classic blue to Paper. Pick one from the menu or make your own.

| Midnight | Aurora |
| --- | --- |
| <img src="docs/themes/midnight.png" alt="Midnight: dark panels with cyan borders and a blue selection" width="440"> | <img src="docs/themes/aurora.png" alt="Aurora: dark panels with blue and purple accents" width="440"> |
| **Neon** | **Classic** |
| <img src="docs/themes/neon.png" alt="Neon: dark panels with bright cyan and pink accents" width="440"> | <img src="docs/themes/classic.png" alt="Classic: blue panels with cyan highlights" width="440"> |
| **Paper** | **Ember** |
| <img src="docs/themes/paper.png" alt="Paper: light panels with muted accents" width="440"> | <img src="docs/themes/ember.png" alt="Ember: dark panels with warm orange accents" width="440"> |

Every download includes **Borg Mono**, a font with file icons. Ghostty uses rich icons
automatically; other terminals start with portable symbols. [Font setup](fonts/README.md).

## License

Borg Commander is available under either the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option. Unless you say otherwise,
any contribution you submit is licensed the same way, without additional terms.

Screenshot rendering uses bundled DejaVu Sans Mono and Symbols Nerd Font.
The icon table comes from [nvim-web-devicons](https://github.com/nvim-tree/nvim-web-devicons)
through [yazi](https://github.com/sxyazi/yazi). Font and icon licenses are included in the repository.

Syntax definitions and highlighting themes come from [two-face](https://github.com/CosmicHorrorDev/two-face)
and [bat](https://github.com/sharkdp/bat); their licences are in [SYNTAX-LICENSES.md](docs/SYNTAX-LICENSES.md).
