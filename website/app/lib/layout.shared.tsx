import type { HomeLayoutProps } from 'fumadocs-ui/layouts/home';
import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { Logo } from '@/components/logo';
import { gitConfig } from './shared';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: { title: <Logo /> },
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
  };
}

// Shared by the home page and the 404 page so their navbars match.
export function homeOptions(): Omit<HomeLayoutProps, 'children'> {
  return {
    ...baseOptions(),
    links: [
      { text: 'Docs', url: '/docs' },
      { text: 'Compare', url: '/#why', active: 'none' },
      { text: 'Features', url: '/#features', active: 'none' },
      { text: 'Themes', url: '/#themes', active: 'none' },
      { text: 'Claude Code', url: '/#claude-code', active: 'none' },
    ],
    // Fit the nav content to the page column: max-w-6xl with px-4 sm:px-6.
    className:
      '[--fd-layout-width:var(--container-6xl)] sm:[&_#nd-nav_.h-14]:px-6 lg:[&_#nd-nav_.h-14>.flex-1]:-me-1.5',
  };
}
