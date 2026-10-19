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
  <a href="docs/GUIDE.md">User guide</a> ·
  <a href="DEVELOPMENT.md">Development</a>
</p>

<p align="center">
  <img src="docs/demo.gif" alt="Borg Commander browsing files, previewing code, inspecting a Git diff, and switching to the shell" width="1100">
</p>

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

## Install

Download a binary for **macOS** (Apple Silicon or Intel) or **Linux** (ARM64 or x86-64)
from [Releases](https://github.com/0xcd21h/borg-commander/releases/latest), or use the installer:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

Run `borg` in any folder. The installer verifies the checksum and puts the binary in
`~/.local/bin`. Update with `borg update`.

[Controls, settings, and source installation →](docs/GUIDE.md) ·
[Build, test, and contribute →](DEVELOPMENT.md)

## License

[MIT](LICENSE). Screenshot rendering uses bundled DejaVu Sans Mono and Symbols Nerd Font.
The icon table comes from [nvim-web-devicons](https://github.com/nvim-tree/nvim-web-devicons)
through [yazi](https://github.com/sxyazi/yazi). Font and icon licenses are included in the repository.

Syntax definitions and highlighting themes come from [two-face](https://github.com/CosmicHorrorDev/two-face)
and [bat](https://github.com/sharkdp/bat); their licences are in [SYNTAX-LICENSES.md](docs/SYNTAX-LICENSES.md).
