import { ArrowLeftIcon, ArrowRightIcon } from '@phosphor-icons/react';
import Link from 'fumadocs-core/link';
import { usePathname } from 'fumadocs-core/framework';
import { useFooterItems } from 'fumadocs-ui/utils/use-footer-items';
import { cn } from '@/lib/cn';

// The built-in page footer puts the page description under each title and has no prop to change that, so docs.tsx turns it off and this one takes its place.
export function PageNav() {
  const items = useFooterItems();
  const pathname = usePathname();
  const idx = items.findIndex((item) => trim(item.url) === trim(pathname));
  if (idx === -1) return null;
  const previous = items[idx - 1];
  const next = items[idx + 1];

  return (
    <nav className="grid gap-4 sm:grid-cols-2">
      {previous && <PageNavItem url={previous.url} name={previous.name} dir="previous" />}
      {next && <PageNavItem url={next.url} name={next.name} dir="next" />}
    </nav>
  );
}

const trim = (url: string) => (url.length > 1 ? url.replace(/\/$/, '') : url);

function PageNavItem({ url, name, dir }: { url: string; name: React.ReactNode; dir: 'previous' | 'next' }) {
  const Icon = dir === 'previous' ? ArrowLeftIcon : ArrowRightIcon;
  return (
    <Link
      href={url}
      className={cn(
        'flex flex-col gap-1 rounded-xl border p-4 text-sm transition-colors hover:bg-fd-accent/80 hover:text-fd-accent-foreground',
        dir === 'next' && 'items-end text-end sm:col-start-2',
      )}
    >
      <span className={cn('inline-flex items-center gap-1.5 text-fd-muted-foreground', dir === 'next' && 'flex-row-reverse')}>
        <Icon className="size-4 shrink-0" />
        {dir === 'previous' ? 'Previous Page' : 'Next Page'}
      </span>
      <span className="font-medium">{name}</span>
    </Link>
  );
}
