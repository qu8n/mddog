import { HomeLayout } from 'fumadocs-ui/layouts/home';
import { Link } from 'react-router';
import { baseOptions } from '@/lib/layout.shared';
import { gitConfig } from '@/lib/shared';

const tagline = 'Preview Markdown in your terminal the way a browser renders it.';
const install = `curl -fsSL https://raw.githubusercontent.com/${gitConfig.user}/${gitConfig.repo}/main/install.sh | sh`;

export function meta() {
  return [
    { title: 'mddog' },
    { name: 'description', content: tagline },
  ];
}

export default function Home() {
  return (
    <HomeLayout {...baseOptions()}>
      <main className="mx-auto flex w-full max-w-4xl flex-1 flex-col items-center px-4 py-16 text-center">
        <img src="/logo.png" alt="" width={96} height={96} />
        <h1 className="mt-4 text-4xl font-bold">mddog</h1>
        <p className="mt-3 max-w-xl text-lg text-fd-muted-foreground">{tagline}</p>

        <pre className="mt-8 max-w-full overflow-x-auto rounded-lg border bg-fd-card px-4 py-3 text-left text-sm">
          <code>{install}</code>
        </pre>
        <p className="mt-2 text-sm text-fd-muted-foreground">
          macOS, in Ghostty, kitty, WezTerm, or iTerm2.
        </p>

        <div className="mt-6 flex gap-3">
          <Link
            to="/docs"
            className="rounded-full bg-fd-primary px-4 py-2.5 text-sm font-medium text-fd-primary-foreground"
          >
            Get started
          </Link>
          <a
            href={`https://github.com/${gitConfig.user}/${gitConfig.repo}`}
            className="rounded-full border px-4 py-2.5 text-sm font-medium"
          >
            GitHub
          </a>
        </div>

        <img
          src="/mddog-themes.webp"
          alt="mddog showing a page in its GitHub and Obsidian themes, light and dark"
          className="mt-12 w-full rounded-lg border"
        />
      </main>
    </HomeLayout>
  );
}
