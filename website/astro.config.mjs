import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { label } from './locales/index.mjs';

export default defineConfig({
  site: 'https://0xcd21h.github.io',
  base: '/borg-commander',
  trailingSlash: 'always',
  integrations: [starlight({
    title: 'Borg Commander',
    defaultLocale: 'root',
    locales: {
      root: { label: 'English', lang: 'en' },
      'zh-cn': { label: '简体中文', lang: 'zh-CN' },
      pt: { label: 'Português (Portugal)', lang: 'pt-PT' },
      es: { label: 'Español', lang: 'es' },
      ru: { label: 'Русский', lang: 'ru' },
    },
    description: 'A two-panel file manager with your shell built in. A practical guide for macOS and Linux.',
    favicon: '/favicon.svg',
    social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/0xcd21h/borg-commander' }],
    customCss: ['./src/styles/custom.css'],
    components: { Footer: './src/components/Footer.astro', Header: './src/components/Header.astro' },
    sidebar: [
      { ...label('start-here.title'), slug: 'guide/start-here' },
      { ...label('everyday'), items: [
        { ...label('keys.title'), slug: 'guide/keys' },
        { ...label('files.title'), slug: 'guide/files' },
        { ...label('search.title'), slug: 'guide/search' },
      ] },
      { ...label('connect'), items: [
        { ...label('git.title'), slug: 'guide/git' },
        { ...label('docker.title'), slug: 'guide/docker' },
        { ...label('remote-access.title'), slug: 'guide/remote-access' },
        { ...label('activity.title'), slug: 'guide/activity' },
        { ...label('network.title'), slug: 'guide/network' },
      ] },
      { ...label('customize'), items: [
        { ...label('themes.title'), slug: 'guide/themes' },
        { ...label('configuration.title'), slug: 'guide/configuration' },
      ] },
    ],
  })],
});
