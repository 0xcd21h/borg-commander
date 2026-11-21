import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const brand = join(root, 'docs/brand');
const fonts = join(root, 'fonts');

const ICON_LABEL = '◈ Borg';
const SOCIAL_LABEL = '◈ Borg Commander';
const TAGLINE = 'A two-panel file manager with your shell built in.';
const BLANK = ' ';

const ACCENT = { from: [0x5f, 0xaf, 0xd7], to: [0x87, 0x5f, 0xd7] };
const CONTRAST = 140_000;
const LUMA = [299, 587, 114];
const DARK_INK = [0, 0, 0];
const LIGHT_INK = [255, 255, 255];
const NOISE = [...'▓▒░▚▞▙▟▛▜▀▄▌▐■▪◢◣◤◥╳╱╲┼╪╫¦≡∆¤ΞΨЖλØ§∑∂√#%'];
const SMEAR = [...'░░▒▓'];
const VIVID = [[255, 0, 170], [190, 255, 0], [0, 140, 255], [255, 120, 0], [150, 60, 255], [255, 64, 110], [64, 220, 255]];
const SPLIT_RED = [255, 40, 90];
const SPLIT_CYAN = [40, 230, 255];
const BEAM = [255, 255, 255];
const SCREEN = [11, 12, 22];
const SCREEN_GLOW = '#181a36';
const SCREEN_EDGE = '#06070d';
const TAGLINE_COLOR = '#c3c7e0';
const RIM = '#9aa0e0';

const FRAMES_PER_SECOND = 24;
const GLITCH_WIDTH = 34;
const MAX_TEAR = 14;
const SPLIT_REACH = 2;
const BEAM_PERIOD = 1.4;
const BEAM_OVERRUN = 3;
const SETTLE_CURVE = 1.6;
const SMEAR_FLOOR = 0.35;
const SMEAR_RAMP = 0.5;
const TINT_SHARE = 0.75;
const BEAM_SHARE = 0.45;
const SPLIT_OPACITY = 0.7;
const ODDS = { tear: 0.65, invert: 0.12, splitHeat: 0.25, split: 0.55, glyph: 0.4, blank: 0.05, vivid: 0.2, beamHeat: 0.05 };

const MASK = (1n << 64n) - 1n;
const MIX = [0x9e3779b97f4a7c15n, 0xbf58476d1ce4e5b9n, 0x94d049bb133111ebn];
const SEED_MIX = [0x9e37n, 0x85ebn, 0xc2b2n];
const UNIT_BITS = 24n;

const ADVANCE_EM = 0.602;
const ASCENT_EM = 0.928;
const HEIGHT_EM = 1.164;
const CELL_ASPECT = HEIGHT_EM / ADVANCE_EM;
const FONT = 'Borg Mono';
const SEAM = 0.5;

const LOOP_SECONDS = 3.6;
const CALM_SECONDS = 1.6;
const RISE_SECONDS = 0.12;
const BURST_SECONDS = 1.1;
const PEAK_HEAT = 0.9;
const FROZEN_FRAME = 52;
const MID_FRAME = 0.5;
const GIF_DELAY_CS = 8;
const CS_PER_SECOND = 100;

const ICON = { size: 512, corner: 0.2, slices: 20, scanline: 4 };
const GIF_SIZE = 256;
const FAVICON_SIZE = 64;
const SOCIAL = { width: 1280, height: 640, cell: 56, slices: 8, frozenFrame: 51, scanline: 4, glow: 18, glowOpacity: 0.5, taglineSize: 28, taglineGap: 1.1 };

const hash = value => {
  let z = (value + MIX[0]) & MASK;
  z = ((z ^ (z >> 30n)) * MIX[1]) & MASK;
  z = ((z ^ (z >> 27n)) * MIX[2]) & MASK;
  return z ^ (z >> 31n);
};
const unit = value => Number(hash(value) >> (64n - UNIT_BITS)) / 2 ** Number(UNIT_BITS);
const seed = (frame, row, column, salt) =>
  hash(((BigInt(frame) * SEED_MIX[0]) & MASK) ^ ((BigInt(row) * SEED_MIX[1]) & MASK) ^ ((BigInt(column) * SEED_MIX[2]) & MASK) ^ BigInt(salt));
const pick = (items, value) => items[Number(hash(value) % BigInt(items.length))];
const mix = (from, to, share) => from.map((channel, i) => Math.round(channel + (to[i] - channel) * share));
const accentAt = share => mix(ACCENT.from, ACCENT.to, Math.min(1, Math.max(0, share)));
const inkOn = bg => (bg.reduce((sum, channel, i) => sum + channel * LUMA[i], 0) > CONTRAST ? DARK_INK : LIGHT_INK);
const hex = rgb => `#${rgb.map(channel => channel.toString(16).padStart(2, '0')).join('')}`;
const escape = glyph => glyph.replace('&', '&amp;').replace('<', '&lt;');

const frameAt = t => Math.floor(Math.max(0, t) * FRAMES_PER_SECOND);
const frameTime = frame => (frame + MID_FRAME) / FRAMES_PER_SECOND;
const beamRow = (t, heat, height) => (heat > ODDS.beamHeat ? Math.floor(((t / BEAM_PERIOD) % 1) * (height + BEAM_OVERRUN)) : undefined);

const heatAt = t => {
  const since = (t % LOOP_SECONDS) - CALM_SECONDS;
  if (since < 0 || since > BURST_SECONDS) return 0;
  if (since < RISE_SECONDS) return (PEAK_HEAT * since) / RISE_SECONDS;
  return PEAK_HEAT * (1 - (since - RISE_SECONDS) / (BURST_SECONDS - RISE_SECONDS)) ** SETTLE_CURVE;
};

const sliceGlitch = (frame, heat, s, cols, beam) => {
  const torn = unit(seed(frame, s, 0, 1)) < ODDS.tear * heat;
  const tear = torn ? (unit(seed(frame, s, 0, 2)) * 2 - 1) * (MAX_TEAR / GLITCH_WIDTH) * cols * heat : 0;
  return {
    tear,
    tint: tear !== 0 ? pick(VIVID, seed(frame, s, 0, 3)) : undefined,
    inverted: unit(seed(frame, s, 0, 4)) < ODDS.invert * heat,
    split: heat > ODDS.splitHeat && unit(seed(frame, s, 0, 5)) < ODDS.split * heat,
    lit: beam === s,
  };
};

const glyph = (x, y, size, fill, shown) => `<text x="${x}" y="${y}" font-size="${size}" fill="${fill}">${escape(shown)}</text>`;

const CALM = { tear: 0, tint: undefined, inverted: false, split: false, lit: false };

const badge = ({ label, cell, left, top, rows, textRow, slices, width, backdrop }, t) => {
  const heat = heatAt(t);
  const frame = frameAt(t);
  const glyphs = [...`${BLANK}${label}${BLANK}`];
  const cols = glyphs.length;
  const span = cols * cell;
  const height = cell * CELL_ASPECT;
  const size = cell / ADVANCE_EM;
  const baseline = row => top + row * height + ASCENT_EM * size;
  const colors = glyphs.map((_, c) => accentAt(c / (cols - 1)));
  const sliceHeight = (rows * height) / slices;
  const beam = beamRow(t, heat, slices);
  const slice = (s, y, band, { tear, tint, inverted, split, lit }, level) => {
    const first = Math.floor((y - top) / height);
    const rowsHere = Array.from({ length: Math.floor((y - top + band - SEAM) / height) - first + 1 }, (_, i) => first + i);
    const middle = Math.floor((y - top + band / 2) / height);
    const shift = tear * cell;
    const out = [`<rect x="${left}" y="${y}" width="${span}" height="${band}" fill="${backdrop}"/>`];
    const reach = Math.ceil(Math.abs(tear));
    for (let k = 0; k < reach; k += 1) {
      const density = SMEAR[Math.floor((k * SMEAR.length) / reach)];
      const x = tear > 0 ? left + k * cell : left + span - (k + 1) * cell;
      const fill = hex(mix(SCREEN, tint ?? colors[tear > 0 ? 0 : cols - 1], SMEAR_FLOOR + (SMEAR_RAMP * k) / reach));
      rowsHere.forEach(row => out.push(glyph(x, baseline(row), size, fill, density)));
    }
    const cells = glyphs.map((char, c) => {
      const tinted = tint ? mix(colors[c], tint, TINT_SHARE) : colors[c];
      const lifted = lit ? mix(tinted, BEAM, BEAM_SHARE) : tinted;
      const [bg, fg] = inverted ? [inkOn(lifted), lifted] : [lifted, inkOn(lifted)];
      const corrupt = (salt, odds) => unit(seed(frame, s, c, salt)) < odds * level;
      const onText = rowsHere.includes(textRow);
      const shown = char !== BLANK && onText
        ? (corrupt(6, ODDS.glyph) ? pick(NOISE, seed(frame, s, c, 7)) : char)
        : (corrupt(8, ODDS.blank) ? pick(NOISE, seed(frame, s, c, 9)) : undefined);
      const ink = corrupt(10, ODDS.vivid) ? pick(VIVID, seed(frame, s, c, 11)) : fg;
      return { x: left + c * cell + shift, bg, ink, shown, row: shown === undefined ? undefined : char !== BLANK && onText ? textRow : middle };
    });
    cells.forEach(({ x, bg }) => out.push(`<rect x="${x}" y="${y}" width="${cell + SEAM}" height="${band}" fill="${hex(bg)}"/>`));
    if (split) {
      const offset = (SPLIT_REACH / GLITCH_WIDTH) * span;
      const copies = cells.filter(({ shown, row }) => shown !== undefined && row === textRow);
      out.push(`<g opacity="${SPLIT_OPACITY}">`);
      copies.forEach(({ x, shown }) => out.push(glyph(x - offset, baseline(textRow), size, hex(SPLIT_RED), shown)));
      copies.forEach(({ x, shown }) => out.push(glyph(x + offset, baseline(textRow), size, hex(SPLIT_CYAN), shown)));
      out.push('</g>');
    }
    cells.forEach(({ x, ink, shown, row }) => {
      if (shown !== undefined) out.push(glyph(x, baseline(row), size, hex(ink), shown));
    });
    return out.join('');
  };
  const clips = [`<clipPath id="whole"><rect x="0" y="${top}" width="${width}" height="${rows * height}"/></clipPath>`];
  const parts = [`<g clip-path="url(#whole)">${slice(-1, top, rows * height, CALM, 0)}</g>`];
  for (let s = 0; s < slices; s += 1) {
    const y = top + s * sliceHeight;
    const glitched = slice(s, y, sliceHeight, sliceGlitch(frame, heat, s, cols, beam), heat);
    if (glitched === slice(s, y, sliceHeight, CALM, 0)) continue;
    clips.push(`<clipPath id="slice-${s}"><rect x="0" y="${y}" width="${width}" height="${sliceHeight}"/></clipPath>`);
    parts.push(`<g clip-path="url(#slice-${s})">${glitched}</g>`);
  }
  return `<defs>${clips.join('')}</defs><g font-family="${FONT}" font-weight="bold">${parts.join('')}</g>`;
};

const scanlines = size => `<pattern id="scanlines" width="${size}" height="${size}" patternUnits="userSpaceOnUse"><rect width="${size}" height="${size / 2}" fill="#000" fill-opacity=".18"/></pattern>`;

const iconSvg = t => {
  const { size, corner, slices } = ICON;
  const cols = [...ICON_LABEL].length + 2;
  const cell = size / cols;
  const height = cell * CELL_ASPECT;
  const rows = Math.ceil(size / height) + 1 - (Math.ceil(size / height) % 2);
  const radius = size * corner;
  const art = badge({ label: ICON_LABEL, cell, left: 0, top: (size - rows * height) / 2, rows, textRow: Math.floor(rows / 2), slices, width: size, backdrop: hex(SCREEN) }, t);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">
<defs><clipPath id="tile"><rect width="${size}" height="${size}" rx="${radius}"/></clipPath>${scanlines(ICON.scanline)}</defs>
<g clip-path="url(#tile)"><rect width="${size}" height="${size}" fill="${hex(SCREEN)}"/>${art}<rect width="${size}" height="${size}" fill="url(#scanlines)"/></g>
<rect x="1" y="1" width="${size - 2}" height="${size - 2}" rx="${radius - 1}" fill="none" stroke="${RIM}" stroke-opacity=".22" stroke-width="2"/>
</svg>`;
};

const socialSvg = t => {
  const { width, height, cell, slices, glow, glowOpacity, taglineSize, taglineGap } = SOCIAL;
  const span = ([...SOCIAL_LABEL].length + 2) * cell;
  const rowHeight = cell * CELL_ASPECT;
  const left = (width - span) / 2;
  const top = (height - rowHeight) / 2 - rowHeight * taglineGap / 2;
  const art = badge({ label: SOCIAL_LABEL, cell, left, top, rows: 1, textRow: 0, slices, width, backdrop: 'url(#screen)' }, t);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
<defs>
<radialGradient id="screen" cx="${width / 2}" cy="${height / 2}" r="${width * 0.6}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${SCREEN_GLOW}"/><stop offset="1" stop-color="${SCREEN_EDGE}"/></radialGradient>
<linearGradient id="accent" x1="${left}" y1="0" x2="${left + span}" y2="0" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${hex(ACCENT.from)}"/><stop offset="1" stop-color="${hex(ACCENT.to)}"/></linearGradient>
<filter id="glow" x="-20%" y="-100%" width="140%" height="300%"><feGaussianBlur stdDeviation="${glow}"/></filter>
${scanlines(SOCIAL.scanline)}
</defs>
<rect width="${width}" height="${height}" fill="url(#screen)"/>
<rect x="${left}" y="${top}" width="${span}" height="${rowHeight}" fill="url(#accent)" opacity="${glowOpacity}" filter="url(#glow)"/>
${art}
<text x="${width / 2}" y="${top + rowHeight * (1 + taglineGap)}" font-family="${FONT}" font-size="${taglineSize}" fill="${TAGLINE_COLOR}" text-anchor="middle">${TAGLINE}</text>
<rect width="${width}" height="${height}" fill="url(#scanlines)"/>
</svg>`;
};

const work = mkdtempSync(join(tmpdir(), 'borg-brand-'));
const render = (svg, name, size) => {
  const source = join(work, `${name}.svg`);
  writeFileSync(source, svg);
  const target = join(work, `${name}.png`);
  execFileSync('resvg', ['--skip-system-fonts', '--use-fonts-dir', fonts, ...(size ? ['-w', `${size}`, '-h', `${size}`] : []), source, target]);
  return target;
};

const gifFrames = () => {
  const frames = [];
  for (let step = 0; step * GIF_DELAY_CS < LOOP_SECONDS * CS_PER_SECOND; step += 1) {
    const svg = iconSvg((step * GIF_DELAY_CS) / CS_PER_SECOND);
    const last = frames.at(-1);
    if (last?.svg === svg) last.delay += GIF_DELAY_CS;
    else frames.push({ svg, delay: GIF_DELAY_CS });
  }
  return frames;
};

try {
  const still = iconSvg(frameTime(FROZEN_FRAME));
  execFileSync('magick', [render(still, 'icon', ICON.size), '-strip', join(brand, 'icon.png')]);
  execFileSync('magick', [render(still, 'favicon', FAVICON_SIZE), '-strip', join(brand, 'favicon.png')]);
  execFileSync('magick', [render(socialSvg(frameTime(SOCIAL.frozenFrame)), 'social'), '-strip', join(brand, 'social-preview.png')]);
  const frames = gifFrames().flatMap(({ svg, delay }, i) => ['-delay', `${delay}`, render(svg, `frame-${i}`, GIF_SIZE)]);
  execFileSync('magick', ['-dispose', 'background', ...frames, '-loop', '0', '-layers', 'Optimize', join(brand, 'icon.gif')]);
} finally {
  rmSync(work, { recursive: true, force: true });
}
