# Borg Mono

Regular and bold monospace fonts combining DejaVu Sans Mono with the private-use icon glyphs
from Symbols Nerd Font Mono. Icon advances match the text cell width.

Install both `.ttf` files, select **Borg Mono** in your terminal preferences, and set
`icon-style = "nerd"` in Borg's config. Automatic mode uses rich icons in Ghostty and portable symbols elsewhere.
Borg does not install fonts or change terminal preferences automatically.

The font sources and their licences are in `crates/borg-testkit/assets`. Both licences
are included here. To rebuild with Python and FontTools installed:

```sh
python3 scripts/build-font.py
```
