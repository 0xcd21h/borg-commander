from pathlib import Path
from tempfile import TemporaryDirectory
from fontTools import merge, subset
from fontTools.ttLib import TTFont
from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen

root = Path(__file__).resolve().parent.parent
assets = root / 'crates/borg-testkit/assets'
output = root / 'fonts'
output.mkdir(exist_ok=True)
with TemporaryDirectory() as directory:
    symbols = TTFont(assets / 'SymbolsNerdFontMono-Regular.ttf')
    selected = subset.Subsetter()
    selected.populate(unicodes=[c for c in symbols.getBestCmap() if 0xE000 <= c <= 0xF8FF or 0xF0000 <= c <= 0xFFFFD])
    selected.subset(symbols)
    symbols_path = Path(directory) / 'symbols.ttf'
    symbols.save(symbols_path)
    for style in ('Regular', 'Bold'):
        base = assets / ('DejaVuSansMono.ttf' if style == 'Regular' else 'DejaVuSansMono-Bold.ttf')
        font = merge.Merger().merge([str(base), str(symbols_path)])
        names = font['name']
        for record in list(names.names):
            if record.nameID in (1, 2, 3, 4, 6, 16, 17):
                names.names.remove(record)
        for platform, encoding, language in ((3, 1, 0x409), (1, 0, 0)):
            for name_id, text in {1:'Borg Mono', 2:style, 3:f'Borg Mono {style} 1.0', 4:f'Borg Mono {style}', 6:f'BorgMono-{style}', 16:'Borg Mono', 17:style}.items():
                names.setName(text, name_id, platform, encoding, language)
        glyphs = font.getGlyphSet()
        advance = font['hmtx'][font.getBestCmap()[ord('M')]][0]
        private = {glyph for code, glyph in font.getBestCmap().items() if 0xE000 <= code <= 0xF8FF or 0xF0000 <= code <= 0xFFFFD}
        changed = {}
        for name in private:
            old_advance = font['hmtx'][name][0]
            scale = advance / old_advance if old_advance else 1
            recording = DecomposingRecordingPen(glyphs)
            glyphs[name].draw(recording)
            pen = TTGlyphPen(None)
            recording.replay(TransformPen(pen, (scale, 0, 0, scale, 0, 150)))
            changed[name] = pen.glyph()
        for name, glyph in changed.items():
            font['glyf'][name] = glyph
            glyph.recalcBounds(font['glyf'])
            font['hmtx'][name] = (advance, getattr(glyph, 'xMin', 0))
        font['post'].isFixedPitch = 1
        font['OS/2'].xAvgCharWidth = advance
        font.save(output / f'BorgMono-{style}.ttf')
