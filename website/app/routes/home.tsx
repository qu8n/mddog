import { DynamicCodeBlock } from 'fumadocs-ui/components/dynamic-codeblock';
import { HomeLayout } from 'fumadocs-ui/layouts/home';
import type { LinkItemType } from 'fumadocs-ui/layouts/shared';
import { LinkButton } from '@cloudflare/kumo/components/button';
import { ArrowRightIcon, GithubLogoIcon } from '@phosphor-icons/react';
import type { ReactNode } from 'react';
import { Link } from 'react-router';
import { DemoVideo } from '@/components/demo-video';
import { Kbd } from '@/components/kbd';
import {
  CodePreview,
  DiagramPreview,
  FileListPreview,
  SearchPreview,
  TablesPreview,
  TocPreview,
} from '@/components/mddog-previews';
import { ClaudeMark, GitHubMark, ObsidianMark } from '@/components/brand-icons';
import { cn } from '@/lib/cn';
import { baseOptions } from '@/lib/layout.shared';
import { gitConfig } from '@/lib/shared';

const tagline = 'Preview Markdown in your terminal, with real fonts, tables, diagrams, math, and images.';
const repoUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;
const install = `curl -fsSL https://raw.githubusercontent.com/${gitConfig.user}/${gitConfig.repo}/main/install.sh | sh`;
const plugin = `claude plugin marketplace add ${gitConfig.user}/${gitConfig.repo}
claude plugin install mddog@mddog`;
const usage = `mdd README.md       # preview a file
mdd                 # pick a Markdown file nearby
cat notes.md | mdd  # preview standard input`;

const sectionLinks: LinkItemType[] = [
  { text: 'Docs', url: '/docs' },
  { text: 'Features', url: '/#features', active: 'none' },
  { text: 'Themes', url: '/#themes', active: 'none' },
  { text: 'Claude Code', url: '/#claude-code', active: 'none' },
];

const terminals = [
  { name: 'Ghostty', logo: '/terminals/ghostty.png', url: 'https://ghostty.org' },
  { name: 'iTerm2', logo: '/terminals/iterm2.png', url: 'https://iterm2.com' },
  { name: 'kitty', logo: '/terminals/kitty.svg', url: 'https://sw.kovidgoyal.net/kitty/' },
  { name: 'WezTerm', logo: '/terminals/wezterm.svg', url: 'https://wezterm.org' },
  { name: 'Rex', logo: '/terminals/rex.png', url: 'https://www.superlogical.com/rex' },
];

// `wide` features take a full row, with the preview on the given side. The rest sit in pairs.
const features: {
  title: string;
  body: ReactNode;
  Preview: (props: { className?: string }) => ReactNode;
  wide?: 'right' | 'left';
}[] = [
  {
    title: 'GitHub Flavored Markdown',
    body: 'Tables, task lists, alerts, and footnotes look the same as they do on GitHub',
    Preview: TablesPreview,
    wide: 'right',
  },
  {
    title: 'Syntax highlighting',
    body: 'Colored code blocks for 50+ programming languages',
    Preview: CodePreview,
  },
  {
    title: 'Diagrams and math',
    body: 'Mermaid blocks render as interactive diagrams, and LaTeX renders as typeset math',
    Preview: DiagramPreview,
  },
  {
    title: 'Search',
    body: (
      <>
        Press <Kbd>/</Kbd> and type to search the file, then press <Kbd>n</Kbd> and{' '}
        <Kbd>N</Kbd> to move between matches
      </>
    ),
    Preview: SearchPreview,
  },
  {
    title: 'Table of contents',
    body: (
      <>
        Press <Kbd>t</Kbd> to open the contents panel, then <Kbd>J</Kbd> or <Kbd>K</Kbd> go to the next or previous section
      </>
    ),
    Preview: TocPreview,
  },
  {
    title: 'Fuzzy file picker',
    body: (
      <>
        Run <code className="font-mono text-[0.9em]">mdd</code> with no input to list and search Markdown
        files in your repo, newest first
      </>
    ),
    Preview: FileListPreview,
    wide: 'left',
  },
];

export function meta() {
  return [{ title: 'mddog' }, { name: 'description', content: tagline }];
}

export default function Home() {
  return (
    <>
      {/* Fit the nav content to the page column: max-w-6xl with px-4 sm:px-6. */}
      <HomeLayout
        {...baseOptions()}
        links={sectionLinks}
        className="[--fd-layout-width:var(--container-6xl)] sm:[&_#nd-nav_.h-14]:px-6 lg:[&_#nd-nav_.h-14>.flex-1]:-me-1.5"
      >
        <Hero />
        <Terminals />
        <Demo />
        <Features />
        <Themes />
        <ClaudeCode />
      </HomeLayout>
      <Footer />
    </>
  );
}

function Hero() {
  return (
    <section className="mx-auto w-full max-w-6xl px-4 pt-16 sm:px-6 sm:pt-24">
      <div className="flex flex-col gap-8 lg:flex-row lg:items-center lg:justify-between">
        <div className="min-w-0 max-w-3xl">
          <h1 className="text-4xl font-medium tracking-tight text-balance sm:text-6xl">
            Real Markdown rendering, right in your terminal
          </h1>
          <p className="mt-5 max-w-2xl text-xl text-fd-muted-foreground text-pretty">
            View Markdown with real fonts, tables, diagrams, math, and images
          </p>
          <div className="mt-8 max-w-xs">
            <DynamicCodeBlock lang="sh" code={install} />
          </div>
          <div className="mt-4 flex flex-wrap items-center gap-3">
            <LinkButton href="/docs" variant="primary" size="lg">
              Get started
            </LinkButton>
            <LinkButton href={repoUrl} variant="secondary" size="lg" icon={GithubLogoIcon}>
              View on GitHub
            </LinkButton>
          </div>
        </div>
        <img
          src="/logo.png"
          alt="The mddog dog, reading a book"
          width={384}
          height={255}
          className="order-first w-52 sm:w-64 lg:order-none lg:mr-4 lg:w-96"
        />
      </div>
    </section>
  );
}

function Demo() {
  return (
    <section className="mx-auto w-full max-w-6xl px-4 pb-16 sm:px-6">
      <div className="overflow-hidden rounded-xl border bg-[#0d1117] shadow-md shadow-black/5">
        <DemoVideo />
      </div>
    </section>
  );
}

function Terminals() {
  return (
    <section className="mx-auto w-full max-w-6xl px-4 pt-10 pb-12 sm:px-6">
      <h2 className="text-sm font-medium text-fd-muted-foreground">
        Works in any terminal that supports the{' '}
        <a
          href="https://sw.kovidgoyal.net/kitty/graphics-protocol/"
          className="rounded-sm underline underline-offset-4 hover:text-fd-foreground outline-none focus-visible:ring-2 focus-visible:ring-fd-ring focus-visible:ring-offset-2 focus-visible:ring-offset-fd-background"
        >
          kitty graphics protocol
        </a>
      </h2>
      <ul className="mt-6 flex flex-wrap gap-x-10 gap-y-6">
        {terminals.map((t) => (
          <li key={t.name}>
            <a href={t.url} className="group flex items-center gap-3 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-fd-ring focus-visible:ring-offset-2 focus-visible:ring-offset-fd-background">
              <img
                src={t.logo}
                alt=""
                width={40}
                height={40}
                loading="lazy"
                className="size-10 rounded-[9px] object-contain"
              />
              <span className="text-sm font-medium text-fd-muted-foreground transition-colors group-hover:text-fd-foreground">
                {t.name}
              </span>
            </a>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Features() {
  return (
    <section id="features" className="scroll-mt-16 border-t">
      <div className="mx-auto w-full max-w-6xl px-4 py-20 sm:px-6 sm:py-28">
        <SectionHeading
          title="Features"
          body="Read, search, and move around Markdown files without leaving the terminal"
        />
        <div className="mt-16 grid gap-16 sm:mt-20 sm:gap-24 lg:grid-cols-2 lg:gap-x-12">
          {features.map((f) =>
            f.wide ? (
              <div
                key={f.title}
                className={cn(
                  'grid items-center gap-8 lg:col-span-2 lg:gap-16',
                  f.wide === 'right' ? 'lg:grid-cols-[2fr_3fr]' : 'lg:grid-cols-[3fr_2fr]',
                )}
              >
                <FeatureText title={f.title} body={f.body} />
                <f.Preview className={f.wide === 'left' ? 'lg:order-first' : undefined} />
              </div>
            ) : (
              <div key={f.title} className="flex min-w-0 flex-col gap-8 lg:row-span-2 lg:grid lg:grid-rows-subgrid lg:gap-y-8">
                <FeatureText title={f.title} body={f.body} />
                <f.Preview />
              </div>
            ),
          )}
        </div>
        <LinkButton href="/docs/markdown" variant="secondary" className="mt-16">
          View all features <ArrowRightIcon />
        </LinkButton>
      </div>
    </section>
  );
}

function Themes() {
  return (
    <section id="themes" className="scroll-mt-16 border-t">
      <div className="mx-auto w-full max-w-6xl px-4 py-20 sm:px-6 sm:py-28">
        <SectionHeading
          title={
            <>
              <GitHubMark /> GitHub and <ObsidianMark /> Obsidian themes
            </>
          }
          body="Light and dark variants, with colors and fonts you can change"
        />
        <img
          src="/mddog-themes.webp"
          alt="One page shown in the GitHub and Obsidian themes, light and dark"
          width={2560}
          height={1383}
          loading="lazy"
          className="mt-12 w-full rounded-xl"
        />
        <LinkButton href="/docs/themes" variant="secondary" className="mt-8">
          Configure your theme <ArrowRightIcon />
        </LinkButton>
      </div>
    </section>
  );
}

function ClaudeCode() {
  return (
    <section id="claude-code" className="scroll-mt-16 border-t">
      <div className="mx-auto grid w-full max-w-6xl gap-10 px-4 py-20 sm:px-6 sm:py-28 lg:grid-cols-2 lg:gap-16">
        <SectionHeading
          title={
            <>
              <ClaudeMark /> Claude Code plugin
            </>
          }
          body="Open a Markdown file in a pane right in the chat"
        />
        <div className="flex min-w-0 flex-col justify-center gap-3">
          <p className="text-sm text-fd-muted-foreground">After you install mddog, run:</p>
          <DynamicCodeBlock lang="sh" code={plugin} />
          <LinkButton href="/docs/claude-code" variant="secondary" className="mt-3 self-start">
            Set it up <ArrowRightIcon />
          </LinkButton>
        </div>
      </div>
    </section>
  );
}

function Footer() {
  return (
    <footer className="border-t">
      <div className="mx-auto flex w-full max-w-6xl flex-wrap items-center justify-between gap-4 px-4 py-10 text-sm text-fd-muted-foreground sm:px-6">
        <span>MIT licensed</span>
        <nav className="flex gap-6">
          <Link to="/docs" className="rounded-sm hover:text-fd-foreground outline-none focus-visible:ring-2 focus-visible:ring-fd-ring focus-visible:ring-offset-2 focus-visible:ring-offset-fd-background">
            Docs
          </Link>
          <a href={repoUrl} className="rounded-sm hover:text-fd-foreground outline-none focus-visible:ring-2 focus-visible:ring-fd-ring focus-visible:ring-offset-2 focus-visible:ring-offset-fd-background">
            GitHub
          </a>
          <a href={`${repoUrl}/releases`} className="rounded-sm hover:text-fd-foreground outline-none focus-visible:ring-2 focus-visible:ring-fd-ring focus-visible:ring-offset-2 focus-visible:ring-offset-fd-background">
            Releases
          </a>
        </nav>
      </div>
    </footer>
  );
}

function FeatureText({ title, body }: { title: string; body: ReactNode }) {
  return (
    <div className="max-w-xl">
      <h3 className="text-2xl font-medium tracking-tight">{title}</h3>
      <p className="mt-3 text-fd-muted-foreground leading-relaxed text-pretty">{body}</p>
    </div>
  );
}

function SectionHeading({ title, body }: { title: ReactNode; body: string }) {
  return (
    <div className="max-w-2xl">
      <h2 className="text-3xl font-medium tracking-tight text-balance sm:text-4xl">{title}</h2>
      <p className="mt-4 text-lg text-fd-muted-foreground leading-relaxed text-pretty">{body}</p>
    </div>
  );
}
