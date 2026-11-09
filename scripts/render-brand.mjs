import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const brand = join(root, 'docs/brand');
const fonts = join(root, 'fonts');

const LOGO = [
  '██████   ██████  ██████   ██████',
  '██   ██ ██    ██ ██   ██ ██',
  '██████  ██    ██ ██████  ██   ███',
  '██   ██ ██    ██ ██   ██ ██    ██',
  '██████   ██████  ██   ██  ██████',
];
const LOGO_WIDTH = 34;
const SUBTITLE = 'c o m m a n d e r';
const SUBTITLE_GAP = 1;
const SUBTITLE_ROW = 99;
const TAGLINE = 'A two-panel file manager with your shell built in.';
const FULL_BLOCK = '█';
const BLANK = ' ';

const ACCENT = { from: [0x5f, 0xaf, 0xd7], to: [0x87, 0x5f, 0xd7] };
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
const MAX_TEAR = 14;
const BEAM_PERIOD = 1.4;
const BEAM_OVERRUN = 3;
const SETTLE_CURVE = 1.6;
const SPLIT_REACH = 2;
const SPLIT_SHADE = 0.7;
const SMEAR_FLOOR = 0.35;
const SMEAR_RAMP = 0.5;
const TINT_SHARE = 0.75;
const ODDS = { tear: 0.65, invert: 0.12, splitHeat: 0.25, split: 0.55, glyph: 0.4, blank: 0.05, vivid: 0.2, subtitle: 0.3 };

const MASK = (1n << 64n) - 1n;
const MIX = [0x9e3779b97f4a7c15n, 0xbf58476d1ce4e5b9n, 0x94d049bb133111ebn];
const SEED_MIX = [0x9e37n, 0x85ebn, 0xc2b2n];
const UNIT_BITS = 24n;

const ADVANCE_EM = 0.602;
const ASCENT_EM = 0.928;
const HEIGHT_EM = 1.164;
const CELL_ASPECT = HEIGHT_EM / ADVANCE_EM;
const FONT = 'Borg Mono';

const LOOP_SECONDS = 3.6;
const CALM_SECONDS = 1.6;
const RISE_SECONDS = 0.12;
const BURST_SECONDS = 1.1;
const PEAK_HEAT = 0.9;
const FROZEN_SECONDS = 2.25;
const GIF_DELAY_CS = 8;
const CS_PER_SECOND = 100;

const ICON = { size: 512, cols: 38, corner: 0.2, scanline: 4, glow: 5 };
const GIF_SIZE = 256;
const FAVICON_SIZE = 64;
const SOCIAL = { width: 1280, height: 640, cols: 56, scanline: 4, glow: 8, taglineSize: 28, taglineGap: 1.6 };

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
const hex = rgb => `#${rgb.map(channel => channel.toString(16).padStart(2, '0')).join('')}`;
const escape = glyph => glyph.replace('&', '&amp;').replace('<', '&lt;');

const frameAt = t => Math.floor(Math.max(0, t) * FRAMES_PER_SECOND);
const beamRow = (t, heat, height) => (heat > 0.05 ? Math.floor(((t / BEAM_PERIOD) % 1) * (height + BEAM_OVERRUN)) : undefined);

const heatAt = t => {
  const since = (t % LOOP_SECONDS) - CALM_SECONDS;
  if (since < 0 || since > BURST_SECONDS) return 0;
  if (since < RISE_SECONDS) return (PEAK_HEAT * since) / RISE_SECONDS;
  return PEAK_HEAT * (1 - (since - RISE_SECONDS) / (BURST_SECONDS - RISE_SECONDS)) ** SETTLE_CURVE;
};

const grid = () => {
  const cells = new Map();
  const put = (x, y, glyph, fg, bg) => {
    const key = `${x},${y}`;
    cells.set(key, { x, y, glyph, fg, bg: bg ?? cells.get(key)?.bg });
  };
  return { cells, put };
};

const glitchRow = (put, row, heat, frame) => {
  const r = row.id;
  const [left, y] = row.origin;
  const torn = unit(seed(frame, r, 0, 1)) < ODDS.tear * heat;
  const tear = torn ? Math.round((unit(seed(frame, r, 0, 2)) * 2 - 1) * MAX_TEAR * heat) : 0;
  const tint = tear !== 0 ? pick(VIVID, seed(frame, r, 0, 3)) : undefined;
  const inverted = unit(seed(frame, r, 0, 4)) < ODDS.invert * heat;
  const split = heat > ODDS.splitHeat && unit(seed(frame, r, 0, 5)) < ODDS.split * heat;
  const x0 = left + tear;
  if (split) {
    row.cells.forEach(([glyph], c) => {
      if (glyph === BLANK) return;
      put(x0 + c - SPLIT_REACH, y, '▓', mix(SCREEN, SPLIT_RED, SPLIT_SHADE));
      put(x0 + c + SPLIT_REACH, y, '▓', mix(SCREEN, SPLIT_CYAN, SPLIT_SHADE));
    });
  }
  if (tear !== 0) {
    const reach = Math.abs(tear);
    const anchor = tear > 0 ? left : x0 + row.width;
    const color = tint ?? row.cells[0]?.[1] ?? SCREEN;
    for (let k = 0; k < reach; k += 1) {
      const density = SMEAR[Math.floor((k * SMEAR.length) / reach)];
      const x = tear > 0 ? anchor + k : anchor + reach - 1 - k;
      put(x, y, density, mix(SCREEN, color, SMEAR_FLOOR + (SMEAR_RAMP * k) / reach));
    }
  }
  row.cells.forEach(([glyph, base], c) => {
    const corrupt = (salt, odds) => unit(seed(frame, r, c, salt)) < odds;
    const shown = glyph !== BLANK
      ? (corrupt(6, ODDS.glyph * heat) ? pick(NOISE, seed(frame, r, c, 7)) : glyph)
      : (corrupt(8, ODDS.blank * heat) ? pick(NOISE, seed(frame, r, c, 9)) : undefined);
    if (shown === undefined) return;
    const fg = row.lit ? BEAM : corrupt(10, ODDS.vivid * heat) ? pick(VIVID, seed(frame, r, c, 11)) : tint ? mix(base, tint, TINT_SHARE) : base;
    if (inverted) put(x0 + c, y, shown, SCREEN, fg);
    else put(x0 + c, y, shown, fg);
  });
};

const glitchLogo = (put, origin, t) => {
  const heat = heatAt(t);
  const frame = frameAt(t);
  const span = LOGO_WIDTH - 1;
  const beam = beamRow(t, heat, LOGO.length);
  LOGO.forEach((line, index) => glitchRow(put, {
    cells: [...line].map((glyph, c) => [glyph, accentAt(c / span)]),
    width: LOGO_WIDTH,
    origin: [origin[0], origin[1] + index],
    id: index,
    lit: beam === index,
  }, heat, frame));
  const letters = [...SUBTITLE];
  const x = origin[0] + Math.floor((LOGO_WIDTH - letters.length) / 2);
  const y = origin[1] + LOGO.length + SUBTITLE_GAP;
  letters.forEach((glyph, i) => {
    const noisy = glyph !== BLANK && unit(seed(frame, SUBTITLE_ROW, i, 12)) < ODDS.subtitle * heat;
    put(x + i, y, noisy ? pick(NOISE, seed(frame, SUBTITLE_ROW, i, 13)) : glyph, accentAt(i / (letters.length - 1)));
  });
};

const logoOrigin = (cols, rows) => [Math.floor((cols - LOGO_WIDTH) / 2), Math.floor((rows - LOGO.length - SUBTITLE_GAP - 1) / 2)];

const cellMarkup = (cells, cols, rows, cell) => {
  const size = cell / ADVANCE_EM;
  const height = cell * CELL_ASPECT;
  const shapes = [];
  for (const { x, y, glyph, fg, bg } of cells.values()) {
    if (x < 0 || x >= cols || y < 0 || y >= rows) continue;
    const left = x * cell;
    const top = y * height;
    if (bg) shapes.push(`<rect x="${left}" y="${top}" width="${cell}" height="${height}" fill="${hex(bg)}"/>`);
    if (glyph === BLANK) continue;
    if (glyph === FULL_BLOCK) shapes.push(`<rect x="${left}" y="${top}" width="${cell}" height="${height}" fill="${hex(fg)}"/>`);
    else shapes.push(`<text x="${left}" y="${top + ASCENT_EM * size}" fill="${hex(fg)}">${escape(glyph)}</text>`);
  }
  return `<g font-family="${FONT}" font-size="${size}">${shapes.join('')}</g>`;
};

const crt = ({ width, height, scanline, glow }) => `<defs>
<radialGradient id="screen" cx="${width / 2}" cy="${height / 2}" r="${Math.max(width, height) * 0.62}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${SCREEN_GLOW}"/><stop offset="1" stop-color="${SCREEN_EDGE}"/></radialGradient>
<pattern id="scanlines" width="${scanline}" height="${scanline}" patternUnits="userSpaceOnUse"><rect width="${scanline}" height="${scanline / 2}" fill="#000" fill-opacity=".22"/></pattern>
<filter id="glow" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="${glow}"/></filter>
</defs>`;

const screenOf = ({ width, height, cols }, t, extra = '') => {
  const cell = width / cols;
  const rows = Math.floor(height / (cell * CELL_ASPECT));
  const top = (height - rows * cell * CELL_ASPECT) / 2;
  const { cells, put } = grid();
  glitchLogo(put, logoOrigin(cols, rows), t);
  const art = cellMarkup(cells, cols, rows, cell);
  return `<rect width="${width}" height="${height}" fill="url(#screen)"/>
<g transform="translate(0 ${top})"><g filter="url(#glow)" opacity=".7">${art}</g>${art}</g>${extra}
<rect width="${width}" height="${height}" fill="url(#scanlines)"/>`;
};

const iconSvg = t => {
  const { size, corner } = ICON;
  const radius = size * corner;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}">${crt({ width: size, height: size, ...ICON })}
<clipPath id="tile"><rect width="${size}" height="${size}" rx="${radius}"/></clipPath>
<g clip-path="url(#tile)">${screenOf({ width: size, height: size, cols: ICON.cols }, t)}</g>
<rect x="1" y="1" width="${size - 2}" height="${size - 2}" rx="${radius - 1}" fill="none" stroke="${RIM}" stroke-opacity=".22" stroke-width="2"/>
</svg>`;
};

const socialSvg = t => {
  const { width, height, cols, taglineSize, taglineGap } = SOCIAL;
  const cell = width / cols;
  const rows = Math.floor(height / (cell * CELL_ASPECT));
  const [, originRow] = logoOrigin(cols, rows);
  const subtitleBottom = (height - rows * cell * CELL_ASPECT) / 2 + (originRow + LOGO.length + SUBTITLE_GAP + 1) * cell * CELL_ASPECT;
  const tagline = `<text x="${width / 2}" y="${subtitleBottom + taglineGap * cell * CELL_ASPECT}" font-family="${FONT}" font-size="${taglineSize}" fill="${TAGLINE_COLOR}" text-anchor="middle">${TAGLINE}</text>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">${crt(SOCIAL)}
${screenOf(SOCIAL, t, tagline)}
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
  const still = iconSvg(FROZEN_SECONDS);
  execFileSync('magick', [render(still, 'icon', ICON.size), '-strip', join(brand, 'icon.png')]);
  execFileSync('magick', [render(still, 'favicon', FAVICON_SIZE), '-strip', join(brand, 'favicon.png')]);
  execFileSync('magick', [render(socialSvg(FROZEN_SECONDS), 'social'), '-strip', join(brand, 'social-preview.png')]);
  const frames = gifFrames().flatMap(({ svg, delay }, i) => ['-delay', `${delay}`, render(svg, `frame-${i}`, GIF_SIZE)]);
  execFileSync('magick', ['-dispose', 'background', ...frames, '-loop', '0', '-layers', 'Optimize', join(brand, 'icon.gif')]);
} finally {
  rmSync(work, { recursive: true, force: true });
}
