import type { ReactNode } from 'react';

/** A key name, styled with Kumo's tokens (Kumo has no Kbd component). */
export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded-sm bg-kumo-recessed px-1.5 py-0.5 font-mono text-[0.8em] text-kumo-default ring ring-kumo-line">
      {children}
    </kbd>
  );
}
