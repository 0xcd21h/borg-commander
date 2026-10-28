import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = fileURLToPath(new URL('../../', import.meta.url));
const source = process.env.BORG_DOCS_REF
  ? execFileSync('git', ['show', `${process.env.BORG_DOCS_REF}:docs/GUIDE.md`], { cwd: root, encoding: 'utf8' })
  : await readFile(join(root, 'docs/GUIDE.md'), 'utf8');
const fenced = text => [...text.matchAll(/^```[^\n]*\n[\s\S]*?^```/gm)].map(match => match[0]);
const inline = text => [...text.replace(/^```[^\n]*\n[\s\S]*?^```/gm, '').matchAll(/`([^`\n]+)`/g)].map(match => match[1]);
const urls = text => [...text.matchAll(/\]\((https?:[^\s)]+)\)/g)].map(match => match[1]);
const headings = text => [...text.matchAll(/^(#{1,6}) /gm)].map(match => match[1]);
const manifest = JSON.parse(await readFile(join(root, 'docs/translations/manifest.json'), 'utf8'));
const digest = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');
for (const code of ['zh-CN', 'pt', 'es', 'ru']) {
  const translated = await readFile(join(root, 'docs/translations', code, 'GUIDE.md'), 'utf8');
  for (const [name, extract] of [['code blocks', fenced], ['inline code', inline], ['external links', urls], ['heading levels', headings]]) {
    if (digest(extract(translated)) !== manifest.locales[code].protected[name]) throw new Error(`${code}: translated guide changed protected ${name}`);
    if (createHash('sha256').update(source).digest('hex') === manifest.locales[code].source && digest(extract(source)) !== manifest.locales[code].protected[name]) throw new Error(`${code}: source protection manifest differs`);
  }
}
console.log('Four guide translations preserve code blocks, inline commands, external URLs, and heading structure.');
