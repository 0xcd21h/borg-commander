import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://0xcd21h.github.io',
  base: '/borg-commander',
  trailingSlash: 'always',
  integrations: [starlight({
    title: 'Borg Commander',
    description: 'A two-panel file manager with your shell built in. A practical guide for macOS and Linux.',
    favicon: '/favicon.svg',
    social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/0xcd21h/borg-commander' }],
    customCss: ['./src/styles/custom.css'],
    components: { Footer: './src/components/Footer.astro' },
    sidebar: [
      { label: 'Start here', slug: 'guide/start-here' },
      { label: 'Everyday use', items: [
        { label: 'Keys & console', slug: 'guide/keys' },
        { label: 'Files, jobs & viewers', slug: 'guide/files' },
        { label: 'Search & collections', slug: 'guide/search' },
      ] },
      { label: 'Connect & inspect', items: [
        { label: 'Git', slug: 'guide/git' },
        { label: 'Docker', slug: 'guide/docker' },
        { label: 'Remote access & MCP', slug: 'guide/remote-access' },
        { label: 'Activity & memory', slug: 'guide/activity' },
      ] },
      { label: 'Make it yours', items: [
        { label: 'Themes & icons', slug: 'guide/themes' },
        { label: 'Configuration', slug: 'guide/configuration' },
      ] },
    ],
  })],
});
