import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: process.env.SITE_URL,
  base: process.env.SITE_BASE || '/',
  output: 'static',
  trailingSlash: 'always',
  integrations: [
    starlight({
      title: 'Keryx',
      description: 'Self-hosted publishing for agents. Publish HTML documents, keep every version, and export PDFs.',
      favicon: '/favicon.svg',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/SimCubeLtd/keryx' }],
      customCss: ['./src/styles/theme.css', './src/styles/docs.css'],
      components: {
        ThemeProvider: './src/components/ThemeProvider.astro',
        ThemeSelect: './src/components/ThemeSelect.astro',
        SiteTitle: './src/components/DocsTitle.astro',
      },
      sidebar: [
        { label: 'Keryx', link: '/' },
        { label: 'Start here', items: ['docs', 'docs/installation', 'docs/quickstart', 'docs/skills', 'docs/agents'] },
        { label: 'Publish and manage', items: ['docs/versions', 'docs/pdf', 'docs/availability', 'docs/tagging', 'docs/sharing'] },
        { label: 'Run Keryx', items: ['docs/configuration', 'docs/storage', 'docs/html-policy'] },
        { label: 'Reference', items: ['docs/cli'] },
      ],
    }),
  ],
});
