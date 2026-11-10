import { type ReactNode, useEffect, useState } from 'react';
import { cn } from '@/lib/cn';
import './mddog-preview.css';

/**
 * A terminal window drawn the way mddog looks in its github-dark theme: a macOS title bar, then the
 * children on the screen above the status bar. It turns github-light when the site is in light mode.
 */
export function MddogFrame({
  label,
  bar,
  rows = 24,
  cols = 84,
  className,
  children,
}: {
  /** Says what the frame shows, for screen readers. */
  label: string;
  /** The status bar, or null for a frame without one. */
  bar: ReactNode;
  /** Terminal rows, status bar included. More rows make the frame taller. */
  rows?: number;
  /** Terminal columns. More columns draw everything smaller in the same width. */
  cols?: number;
  className?: string;
  children: ReactNode;
}) {
  return (
    // One labeled picture for screen readers, like the screenshot it replaces.
    // The drawing inside stays real text for the mouse, but its sample headings
    // and tables would read as part of the page, so it's hidden from them.
    <figure role="img" aria-label={label} className={cn('mdd-wrap not-prose', className)}>
      <div className="mdd" aria-hidden style={frameSize(cols, rows)}>
        <TitleBar />
        <div className="mdd-screen">{children}</div>
        {bar}
      </div>
    </figure>
  );
}

/** The title bar's height, in the frame's pixels. */
const TITLE_BAR = 30;

/**
 * A window's size: the title bar, then cells of 9.5 x 19 of its pixels. Each of its lengths is a multiple
 * of --u, so more columns make --u smaller.
 */
function frameSize(cols: number, rows: number) {
  return {
    aspectRatio: `${cols * 9.5} / ${rows * 19 + TITLE_BAR}`,
    '--u': `calc(100cqw / ${cols * 9.5})`,
  } as React.CSSProperties;
}

/** A macOS title bar with only the traffic lights. An inactive window draws them gray. */
export function TitleBar({ inactive }: { inactive?: boolean }) {
  return (
    <div className="mdd-titlebar" data-inactive={inactive || undefined}>
      <span />
      <span />
      <span />
    </div>
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
    <div className="mdd-alert" style={{ '--alert': `var(--alert-${kind}, ${color})` } as React.CSSProperties}>
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

/** A code block. Pass one child per line; wide lines scroll sideways. `button` replaces the still copy button. */
export function CodeBlock({ plain, button, children }: { plain?: boolean; button?: ReactNode; children: ReactNode }) {
  return (
    <div className={cn('mdd-pre', plain && 'mdd-pre-plain')}>
      <pre>
        <code>{children}</code>
      </pre>
      {button ?? (
        <span className="mdd-copy" aria-hidden>
          <CopyIcon />
        </span>
      )}
    </div>
  );
}

/** mddog's diagram button icons: GitHub's octicons, as src/render/icons.rs draws them. */
export function ExpandIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M3.72 3.72a.75.75 0 011.06 1.06L2.56 7h10.88l-2.22-2.22a.75.75 0 011.06-1.06l3.5 3.5a.75.75 0 010 1.06l-3.5 3.5a.75.75 0 11-1.06-1.06l2.22-2.22H2.56l2.22 2.22a.75.75 0 11-1.06 1.06l-3.5-3.5a.75.75 0 010-1.06l3.5-3.5z" />
    </svg>
  );
}

export function OcticonCopy() {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 010 1.5h-1.5a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-1.5a.75.75 0 011.5 0v1.5A1.75 1.75 0 019.25 16h-7.5A1.75 1.75 0 010 14.25v-7.5z" />
      <path d="M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0114.25 11h-7.5A1.75 1.75 0 015 9.25v-7.5zm1.75-.25a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-7.5a.25.25 0 00-.25-.25h-7.5z" />
    </svg>
  );
}

export function OcticonCheck() {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M13.78 4.22a.75.75 0 010 1.06l-7.25 7.25a.75.75 0 01-1.06 0L2.22 9.28a.75.75 0 011.06-1.06L6 10.94l6.72-6.72a.75.75 0 011.06 0z" />
    </svg>
  );
}

/** The check a code block's copy button turns into, drawn as mddog strokes it. */
export function CodeCopiedIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M3 8l3.5 4L13.5 4" />
    </svg>
  );
}

/**
 * Plays a preview's steps in a loop, each for its `ms`, and returns the one on screen. Before the page
 * runs scripts, and when motion is reduced, it holds the last step.
 */
export function useLoop<T extends { ms: number }>(steps: T[]): T {
  const [i, setI] = useState<number | null>(null);
  useEffect(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    let step = 0;
    let timer: ReturnType<typeof setTimeout>;
    const next = () => {
      setI(step);
      timer = setTimeout(next, steps[step].ms);
      step = (step + 1) % steps.length;
    };
    next();
    return () => clearTimeout(timer);
  }, [steps]);
  return steps[i ?? steps.length - 1];
}
