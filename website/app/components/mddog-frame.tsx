import type { ReactNode } from 'react';
import { cn } from '@/lib/cn';
import './mddog-preview.css';

/**
 * A terminal frame drawn the way mddog looks in its github-dark theme. The
 * children fill the screen above the status bar. It stays dark on light pages.
 */
export function MddogFrame({
  label,
  bar,
  className,
  children,
}: {
  /** Says what the frame shows, for screen readers. */
  label: string;
  bar: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    // One labeled picture for screen readers, like the screenshot it replaces.
    // The drawing inside stays real text for the mouse, but its sample headings
    // and tables would read as part of the page, so it's hidden from them.
    <figure role="img" aria-label={label} className={cn('mdd-wrap not-prose', className)}>
      <div className="mdd" aria-hidden>
        <div className="mdd-screen">{children}</div>
        {bar}
      </div>
    </figure>
  );
}

/** A key and what it does, as the status bar shows them. */
export type Hint = [key: string, label: string];

export const pageHints: Hint[] = [
  ['?', 'help'],
  ['/', 'find'],
  ['t', 'toc'],
  ['e', 'edit'],
];

/** The bottom row: what is open on the left, key hints and the scroll position on the right. */
export function StatusBar({
  left,
  hints = pageHints,
  pos = '0%',
}: {
  left: ReactNode;
  hints?: Hint[];
  pos?: string | null;
}) {
  return (
    <div className="mdd-bar mdd-mono">
      {left !== null && <span>{left}</span>}
      <span className={cn('mdd-bar-hints', left !== null && 'mdd-bar-hints-end')}>
        {hints.map(([key, label]) => (
          <span key={key}>
            <span className="mdd-bright">{key}</span> {label}
          </span>
        ))}
      </span>
      {pos !== null && <span className="mdd-bar-pos">{pos}</span>}
    </div>
  );
}

/** The rendered Markdown, with mddog's page padding. */
export function Page({ children }: { children: ReactNode }) {
  return <div className="mdd-page">{children}</div>;
}

type AlertKind = 'note' | 'tip' | 'warning';

const alerts: Record<AlertKind, { title: string; color: string; icon: ReactNode }> = {
  note: {
    title: 'Note',
    color: '#4493f8',
    icon: (
      <>
        <circle cx="8" cy="8" r="6.75" />
        <path d="M8 7.25v4M8 4.6v.1" />
      </>
    ),
  },
  tip: {
    title: 'Tip',
    color: '#3fb950',
    icon: (
      <>
        <circle cx="8" cy="8" r="6.75" />
        <path d="M8 4.5 9 7l2.5 1L9 9l-1 2.5L7 9 4.5 8 7 7z" fill="currentColor" strokeWidth="0.5" />
      </>
    ),
  },
  warning: {
    title: 'Warning',
    color: '#d29922',
    icon: (
      <>
        <circle cx="8" cy="8" r="6.75" />
        <path d="M8 4.5v4M8 11.2v.1" />
      </>
    ),
  },
};

/** A GitHub-style alert, such as `> [!TIP]`. */
export function Alert({ kind, children }: { kind: AlertKind; children: ReactNode }) {
  const { title, color, icon } = alerts[kind];
  return (
    <div className="mdd-alert" style={{ '--alert': color } as React.CSSProperties}>
      <p className="mdd-alert-title">
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden>
          {icon}
        </svg>
        {title}
      </p>
      {children}
    </div>
  );
}

export function CopyIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" aria-hidden>
      <rect x="5.5" y="1.5" width="9" height="9" rx="1.5" />
      <path d="M3.5 5.5H3A1.5 1.5 0 0 0 1.5 7v6A1.5 1.5 0 0 0 3 14.5h6a1.5 1.5 0 0 0 1.5-1.5v-.5" />
    </svg>
  );
}

/** A code block. Pass one child per line; wide lines scroll sideways. */
export function CodeBlock({ plain, children }: { plain?: boolean; children: ReactNode }) {
  return (
    <div className={cn('mdd-pre', plain && 'mdd-pre-plain')}>
      <pre>
        <code>{children}</code>
      </pre>
      <span className="mdd-copy" aria-hidden>
        <CopyIcon />
      </span>
    </div>
  );
}
