import { readFile, writeFile, mkdir, rm, copyFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import GithubSlugger from 'github-slugger';
import { createHash } from 'node:crypto';
import { codes, dictionary } from '../locales/index.mjs';

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
const headings = text => [...text.matchAll(/^(#{2,6}) (.+)$/gm)];
const englishHeadings = headings(source);
const englishSections = [...source.matchAll(/^## (.+)\n([\s\S]*?)(?=^## |$(?![\s\S]))/gm)];
const manifest = JSON.parse(await readFile(join(root, 'docs/translations/manifest.json'), 'utf8'));
const sourceHash = createHash('sha256').update(source).digest('hex');
for (const locale of codes) {
  const t = dictionary(locale);
  const translated = locale !== 'en';
  const sourceLocale = locale === 'zh-cn' ? 'zh-CN' : locale;
  const prefix = translated ? `${base}${locale}/` : base;
  const guide = translated ? await readFile(join(root, 'docs/translations', sourceLocale, 'GUIDE.md'), 'utf8') : source;
  const localHeadings = headings(guide);
  const templateHeadings = translated ? manifest.locales[sourceLocale].headings.map(heading => [null, '#'.repeat(heading.level), heading.title]) : englishHeadings;
  const sectionNames = translated ? manifest.locales[sourceLocale].headings.filter(heading => heading.level === 2).map(heading => heading.title) : englishSections.map(section => section[1]);
  if (localHeadings.length !== templateHeadings.length) throw new Error(`${locale}: guide heading structure differs`);
  const localSections = [...guide.matchAll(/^## (.+)\n([\s\S]*?)(?=^## |$(?![\s\S]))/gm)];
  const sections = new Map(sectionNames.map((heading, index) => [heading, { title: localSections[index][1], body: localSections[index][2].trim() }]));
  const anchors = new Map();
  const sourceSlugger = new GithubSlugger();
  const localSlugger = new GithubSlugger();
  let parent;
  for (const [index, heading] of templateHeadings.entries()) {
    if (heading[1] !== localHeadings[index][1]) throw new Error(`${locale}: heading levels differ`);
    const anchor = sourceSlugger.slug(heading[2]);
    const localAnchor = localSlugger.slug(localHeadings[index][2]);
    if (heading[1].length === 2) parent = pages.find(([, , , titles]) => titles.includes(heading[2]));
    anchors.set(anchor, parent ? `${prefix}guide/${parent[0]}/${heading[1].length === 2 ? '' : `#${localAnchor}`}` : `${repository}/blob/main/docs/GUIDE.md#${anchor}`);
  }
  const rewrite = text => text
    .replace(/\]\(#([^\s)]+)\)/g, (_, anchor) => `](${anchors.get(anchor) ?? `${repository}/blob/main/docs/GUIDE.md#${anchor}`})`)
    .replace(/\]\((?:\.\.\/)+README\.md#themes\)/g, `](${prefix}#themes)`)
    .replace(/\]\((?:\.\.\/)+([^\s)]+)\)/g, `](${repository}/blob/main/$1)`);
  const destination = join(site, 'src/content/docs', translated ? `${locale}/guide` : 'guide');
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  for (const [slug, , , titles] of pages) {
    const bodies = titles.map(heading => {
      const section = sections.get(heading);
      if (!section) throw new Error(`${locale}: missing guide section: ${heading}`);
      return titles.length > 1 ? `## ${section.title}\n\n${section.body}` : section.body;
    });
    const editPath = translated ? `docs/translations/${sourceLocale}/GUIDE.md` : 'docs/GUIDE.md';
    const frontmatter = `---\ntitle: ${JSON.stringify(t[`${slug}.title`])}\ndescription: ${JSON.stringify(t[`${slug}.description`])}\neditUrl: ${repository}/edit/main/${editPath}\n---\n\n`;
    const outdated = translated && sourceHash !== manifest.locales[sourceLocale].source;
    const notice = outdated ? `:::caution\n${t.staleTranslation} [English](${base}guide/${slug}/).\n:::\n\n` : '';
    await writeFile(join(destination, `${slug}.md`), frontmatter + notice + rewrite(bodies.join('\n\n')) + '\n');
  }
  if (translated) {
    const pageRoot = join(site, 'src/content/docs', locale);
    const hero = { tagline: t.tagline, actions: [
      { text: t.download, link: `${repository}/releases/latest`, icon: 'download' },
      { text: t.start, link: `${prefix}guide/start-here/`, icon: 'right-arrow', variant: 'secondary' },
      { text: 'GitHub', link: repository, icon: 'external', variant: 'minimal' },
    ] };
    const frontmatter = `---\ntitle: Borg Commander\ndescription: ${JSON.stringify(t.description)}\ntemplate: splash\nhero: ${JSON.stringify(hero)}\neditUrl: false\n---\n\n`;
    await writeFile(join(pageRoot, 'index.mdx'), frontmatter + "import HomeContent from '../../../components/HomeContent.astro';\n\n<HomeContent />\n");
    await writeFile(join(pageRoot, '404.md'), `---\ntitle: ${JSON.stringify(t.notFound)}\ndescription: ${JSON.stringify(t.notFoundText)}\neditUrl: false\n---\n\n${t.notFoundText}\n\n[${t.start}](${prefix}guide/start-here/)\n`);
  }
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
console.log(`Generated ${pages.length * codes.length} guide pages across ${codes.length} languages and ${screenshots.length} public screenshots for Borg ${version}.`);
