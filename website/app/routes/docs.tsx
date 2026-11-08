import type { Route } from './+types/docs';
import { DocsLayout } from 'fumadocs-ui/layouts/notebook';
import { DocsBody, DocsPage, DocsTitle } from 'fumadocs-ui/layouts/notebook/page';
import { MarkdownCopyButton, OpenMenu } from '@/components/page-actions';
import { docs, source } from '@/lib/source';
import { baseOptions } from '@/lib/layout.shared';
import { useFumadocsLoader } from 'fumadocs-core/source/client';
import { useMDXComponents } from '@/components/mdx';
import { PageNav } from '@/components/page-nav';
import { use } from 'react';
import { getPageImageUrl, getPageMarkdownUrl, gitConfig } from '@/lib/shared';

export async function loader({ params }: Route.LoaderArgs) {
  const slugs = params['*'].split('/').filter((v) => v.length > 0);
  const page = source.getPage(slugs);
  if (!page) throw new Response('Not found', { status: 404 });

  return {
    path: page.path,
    markdownUrl: getPageMarkdownUrl(page).url,
    pageTree: await source.serializePageTree(source.getPageTree()),
    imagePath: getPageImageUrl(page).url,
  };
}

function Content({
  path,
  markdownUrl,
  imagePath,
}: {
  path: string;
  markdownUrl: string;
  imagePath: string;
}) {
  const page = docs.getPage(path);
  if (!page) throw new Error(`unknown page: ${path}`);

  // content is loaded lazily, call `page.preload()` in your loader to avoid suspending
  const { toc } = use(page.load());
  const Mdx = page.body;

  return (
    <DocsPage toc={toc} footer={{ enabled: false }}>
      <title>{page.title}</title>
      <meta name="description" content={page.description} />
      <meta property="og:image" content={imagePath} />
      {/* The title grows to push the buttons right. On a narrow row they wrap under it, left-aligned. */}
      <div className="flex flex-row flex-wrap items-center gap-x-4 gap-y-3 border-b pb-6 mb-6">
        <DocsTitle className="grow">{page.title}</DocsTitle>
        <div className="flex flex-row gap-2 items-center">
          <MarkdownCopyButton markdownUrl={markdownUrl} />
          <OpenMenu
            markdownUrl={markdownUrl}
            githubUrl={`https://github.com/${gitConfig.user}/${gitConfig.repo}/blob/${gitConfig.branch}/content/docs/${path}`}
          />
        </div>
      </div>
      {/* Code blocks are not-prose and keep their own top margin, so the rule-to-body gap would grow. */}
      <DocsBody className="[&>:first-child]:mt-0 mb-12">
        <Mdx components={useMDXComponents()} />
      </DocsBody>
      <PageNav />
    </DocsPage>
  );
}

export default function Page({ loaderData }: Route.ComponentProps) {
  const { path, pageTree, imagePath, markdownUrl } = useFumadocsLoader(loaderData);

  return (
    <DocsLayout {...baseOptions()} tree={pageTree}>
      <Content path={path} markdownUrl={markdownUrl} imagePath={imagePath} />
    </DocsLayout>
  );
}
