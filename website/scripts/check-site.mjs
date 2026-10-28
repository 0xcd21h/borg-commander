import { readdir, readFile, stat } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { codes, dictionary } from '../locales/index.mjs';

const root = fileURLToPath(new URL('../dist/', import.meta.url));
const base = '/borg-commander/';
async function files(directory) {
  return (await Promise.all((await readdir(directory, { withFileTypes: true })).map(entry => entry.isDirectory() ? files(join(directory, entry.name)) : join(directory, entry.name)))).flat();
}
const htmlFiles = (await files(root)).filter(path => path.endsWith('.html'));
const failures = [];
for (const path of htmlFiles) {
  const html = await readFile(path, 'utf8');
  if (!html.includes('<title>') || !html.includes('name="description"')) failures.push(`${path}: missing page metadata`);
  for (const match of html.matchAll(/\b(?:href|src)="([^"]+)"/g)) {
    const value = match[1].replaceAll('&amp;', '&');
    if (/^(?:https?:|mailto:|data:|javascript:)/.test(value)) continue;
    const url = new URL(value, `https://site.invalid${base}${path.slice(root.length).replace(/^\//, '')}`);
    if (!url.pathname.startsWith(base)) {
      failures.push(`${path}: link outside GitHub Pages base: ${value}`);
      continue;
    }
    let target = resolve(root, decodeURIComponent(url.pathname.slice(base.length)));
    try {
      if ((await stat(target)).isDirectory()) target = join(target, 'index.html');
      await stat(target);
      if (url.hash && target.endsWith('.html')) {
        const fragment = decodeURIComponent(url.hash.slice(1));
        const destination = await readFile(target, 'utf8');
        if (!destination.includes(`id="${fragment}"`)) failures.push(`${path}: missing fragment: ${value}`);
      }
    } catch {
      failures.push(`${path}: missing local asset or page: ${value}`);
    }
  }
}
if (htmlFiles.length !== 12 * codes.length) failures.push(`Expected ${12 * codes.length} HTML pages including 404; found ${htmlFiles.length}`);
const englishKeys = Object.keys(dictionary('en')).sort();
for (const code of codes) {
  const messages = dictionary(code);
  if (JSON.stringify(Object.keys(messages).sort()) !== JSON.stringify(englishKeys)) failures.push(`${code}: website message keys differ`);
  if (Object.values(messages).some(value => typeof value !== 'string' || !value.trim())) failures.push(`${code}: empty website message`);
  const homepage = join(root, code === 'en' ? '' : code, 'index.html');
  const content = await readFile(homepage, 'utf8');
  if (!content.includes('starlight-lang-select')) failures.push(`${code}: language switcher is missing`);
}
await stat(join(root, 'pagefind/pagefind.js'));
if (failures.length) throw new Error(failures.join('\n'));
console.log(`Checked metadata, local links, fragments, and assets on ${htmlFiles.length} pages; search index exists.`);
