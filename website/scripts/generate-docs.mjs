import { readFile, writeFile, mkdir, rm, copyFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import GithubSlugger from 'github-slugger';

const root = fileURLToPath(new URL('../../', import.meta.url));
const site = join(root, 'website');
const base = '/borg-commander/';
const repository = 'https://github.com/0xcd21h/borg-commander';
const source = process.env.BORG_DOCS_REF
  ? execFileSync('git', ['show', `${process.env.BORG_DOCS_REF}:docs/GUIDE.md`], { cwd: root, encoding: 'utf8' })
  : await readFile(join(root, 'docs/GUIDE.md'), 'utf8');
const pages = [
  ['start-here', 'Start here', 'Install Borg on macOS or Linux and open your first workspace.', ['Install']],
  ['keys', 'Keys & console', 'Move between panels, use shortcuts, and work in your embedded shell.', ['Keys', 'Mouse and console']],
  ['files', 'Files, jobs & viewers', 'Find, preview, copy, move, and manage files.', ['Find, jobs, and viewers']],
  ['search', 'Search & collections', 'Search recursively, save collections, and measure folder sizes.', ['Collections and recursive search']],
  ['git', 'Git', 'Inspect changes, stage files, and work with Git inside Borg.', ['Git plugin']],
  ['docker', 'Docker', 'Browse containers, inspect status, and open container files and shells.', ['Docker plugin']],
  ['remote-access', 'Remote access & MCP', 'Connect Borg sessions, browse remote files, and pair an assistant.', ['Claude and other agents (MCP)']],
  ['activity', 'Activity & memory', 'Inspect processes, resource use, and memory maps.', ['Activity and memory']],
  ['themes', 'Themes & icons', 'Choose a theme and configure terminal fonts and file icons.', ['Themes and icons']],
  ['configuration', 'Configuration', 'Find settings, state, and command-line options.', ['Configuration']],
];
const sections = new Map([...source.matchAll(/^## (.+)\n([\s\S]*?)(?=^## |$(?![\s\S]))/gm)].map(match => [match[1], match[2].trim()]));
const anchors = new Map();
const slugger = new GithubSlugger();
for (const match of source.matchAll(/^(#{2,6}) (.+)$/gm)) {
  const anchor = slugger.slug(match[2]);
  const page = pages.find(([, , , titles]) => titles.includes(match[2]));
  if (match[1].length === 2) {
    anchors.set(anchor, page ? `${base}guide/${page[0]}/` : `${repository}/blob/main/docs/GUIDE.md#${anchor}`);
  } else {
    const previous = [...source.slice(0, match.index).matchAll(/^## (.+)$/gm)].at(-1)?.[1];
    const parent = pages.find(([, , , titles]) => titles.includes(previous));
    anchors.set(anchor, parent ? `${base}guide/${parent[0]}/#${anchor}` : `${repository}/blob/main/docs/GUIDE.md#${anchor}`);
  }
}
const rewrite = text => text
  .replace(/\]\(#([^\s)]+)\)/g, (_, anchor) => `](${anchors.get(anchor) ?? `${repository}/blob/main/docs/GUIDE.md#${anchor}`})`)
  .replace(/\]\(\.\.\/README\.md#themes\)/g, `](${base}#themes)`)
  .replace(/\]\(\.\.\/([^\s)]+)\)/g, `](${repository}/blob/main/$1)`);
const destination = join(site, 'src/content/docs/guide');
await rm(destination, { recursive: true, force: true });
await mkdir(destination, { recursive: true });
for (const [slug, title, description, titles] of pages) {
  const bodies = titles.map(heading => {
    if (!sections.has(heading)) throw new Error(`Missing guide section: ${heading}`);
    const content = sections.get(heading);
    return titles.length > 1 ? `## ${heading}\n\n${content}` : content;
  });
  const frontmatter = `---\ntitle: ${JSON.stringify(title)}\ndescription: ${JSON.stringify(description)}\neditUrl: ${repository}/edit/main/docs/GUIDE.md\n---\n\n`;
  await writeFile(join(destination, `${slug}.md`), frontmatter + rewrite(bodies.join('\n\n')) + '\n');
}
const screenshots = [
  ...['paper', 'classic', 'midnight', 'aurora', 'ember', 'neon'].map(name => ['themes', name]),
  ...['docker', 'activity', 'memory-map', 'size-tree', 'folder-totals'].map(name => ['showcase', name]),
];
await mkdir(join(site, 'public/screenshots'), { recursive: true });
for (const [directory, name] of screenshots) {
  await copyFile(join(root, 'docs', directory, `${name}.png`), join(site, 'public/screenshots', `${name}.png`));
}
const version = (await readFile(join(root, 'Cargo.toml'), 'utf8')).match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (!version) throw new Error('Workspace version is missing');
const revision = process.env.GITHUB_SHA ?? execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
await mkdir(join(site, 'src/generated'), { recursive: true });
await writeFile(join(site, 'src/generated/build.json'), JSON.stringify({ version, revision: revision.slice(0, 7) }));
console.log(`Generated ${pages.length} guide pages and ${screenshots.length} public screenshots for Borg ${version}.`);
