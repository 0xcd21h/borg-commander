# Borg Mono

Regular and bold monospace fonts combining DejaVu Sans Mono with the private-use icon glyphs
from Symbols Nerd Font Mono. Icon advances match the text cell width.

Install both `.ttf` files, select **Borg Mono** in your terminal preferences, and set
`nerd-font = true` in Borg's config. Without that setting Borg uses portable symbols.
Borg does not install fonts or change terminal preferences automatically.

The font sources and their licences are in `crates/borg-testkit/assets`. Both licences
are included here. To rebuild with Python and FontTools installed:

```sh
python3 scripts/build-font.py
```
