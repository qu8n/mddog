import type { Config } from '@react-router/dev/config';
import { glob } from 'node:fs/promises';
import { createGetUrl, getSlugs } from 'fumadocs-core/source';
import { getPageImageUrl, getPageMarkdownUrl } from './app/lib/shared';

const getUrl = createGetUrl('/docs');

export default {
  // Static HTML only: Cloudflare Pages serves build/client with no server.
  ssr: false,
  async prerender({ getStaticPaths }) {
    const paths = getStaticPaths();

    for await (const entry of glob('**/*.mdx', { cwd: 'content/docs' })) {
      const slugs = getSlugs(entry);

      paths.push(getUrl(slugs));
      paths.push(getPageImageUrl({ slugs }).url);
      paths.push(getPageMarkdownUrl({ slugs }).url);
    }

    return paths;
  },
} satisfies Config;
