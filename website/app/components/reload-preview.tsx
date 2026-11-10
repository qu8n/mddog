import { MddogFrame, Page, StatusBar, useLoop } from '@/components/mddog-frame';
import { previewLabels } from '@/components/mddog-preview-labels';
import { cn } from '@/lib/cn';

/*
 * Live reload, as two terminal windows side by side: Vim on the left types a new open question and
 * saves it with :w, and mddog on the right shows the question as soon as the file is written. Then it
 * waits and starts again.
 */

const COLS = 44;
const ROWS = 20;
/** Text columns: one fewer than the window, for the terminal's padding. */
const TEXT_COLS = COLS - 1;

const fileLines = [
  '## Rollout',
  '',
  '1. Ship to the beta group on Monday.',
  '2. Watch the trail cache size for a week.',
  '3. Ship to everyone. Then post the release notes.',
  '',
  '## Open questions',
  '',
  '- How long should a trail stay cached after the last trip?',
];

const newLine = '- Should the trail cache sync between phones?';

/** What Vim's command line says once the file is saved, with the real line and byte counts. */
const written = (() => {
  const lines = [...fileLines, newLine];
  const bytes = lines.reduce((n, line) => n + line.length + 1, 0);
  return `"plan.md" ${lines.length}L, ${bytes}B written`;
})();

/** One moment of the loop: how much of the new line is typed, Vim's command line, and whether it's saved. */
type Frame = { typed: number | null; cmd: string; saved: boolean };

type Step = Frame & { ms: number };

/** The loop, one step per change on screen. */
const steps: Step[] = [
  { typed: null, cmd: '', saved: false, ms: 900 },
  // `o` opens a new line in insert mode.
  { typed: 0, cmd: '-- INSERT --', saved: false, ms: 400 },
  ...Array.from({ length: newLine.length }, (_, i) => ({
    typed: i + 1,
    cmd: '-- INSERT --',
    saved: false,
    ms: newLine[i] === ' ' ? 110 : 55,
  })),
  { typed: newLine.length, cmd: '', saved: false, ms: 500 },
  { typed: newLine.length, cmd: ':', saved: false, ms: 180 },
  { typed: newLine.length, cmd: ':w', saved: false, ms: 350 },
  { typed: newLine.length, cmd: written, saved: true, ms: 3500 },
];

/** Splits a line into screen rows, as Vim wraps it: at the last column, even inside a word. */
function wrap(line: string) {
  const rows = [];
  for (let at = 0; at < line.length; at += TEXT_COLS) rows.push(line.slice(at, at + TEXT_COLS));
  return rows.length ? rows : [''];
}

function Cursor({ children = ' ' }: { children?: string }) {
  return <span className="mdd-vim-cursor">{children}</span>;
}

function Vim({ typed, cmd }: Frame) {
  const insert = cmd === '-- INSERT --';
  const onCmdLine = cmd.startsWith(':');
  const rows = fileLines.flatMap(wrap).map((text) => <>{text}</>);
  if (typed !== null) {
    const text = newLine.slice(0, typed);
    const lineRows = wrap(text);
    // In insert mode the cursor sits after the text; in normal mode it's on the last character.
    if (insert && text.length > 0 && text.length % TEXT_COLS === 0) lineRows.push('');
    lineRows.forEach((part, j) => {
      const last = j === lineRows.length - 1;
      if (!last || onCmdLine) return rows.push(<>{part}</>);
      if (insert || part === '') return rows.push(<>{part}<Cursor /></>);
      rows.push(<>{part.slice(0, -1)}<Cursor>{part.slice(-1)}</Cursor></>);
    });
  }
  const tildes = ROWS - 1 - rows.length;
  return (
    <div className="mdd-vim mdd-mono">
      {rows.map((row, j) => (
        <div key={j}>{row}</div>
      ))}
      {Array.from({ length: Math.max(tildes, 0) }, (_, j) => (
        <div key={`~${j}`} className="mdd-vim-tilde">~</div>
      ))}
      <div className={cn(insert && 'mdd-b')}>
        {cmd}
        {onCmdLine && <Cursor />}
      </div>
    </div>
  );
}

function Preview({ saved }: Frame) {
  return (
    <Page>
      <h2>Rollout</h2>
      <ol>
        <li>Ship to the beta group on Monday.</li>
        <li>Watch the trail cache size for a week.</li>
        <li>Ship to everyone. Then post the release notes.</li>
      </ol>
      <h2>Open questions</h2>
      <ul>
        <li>How long should a trail stay cached after the last trip?</li>
        {saved && <li>Should the trail cache sync between phones?</li>}
      </ul>
    </Page>
  );
}

export function ReloadPreview({ className }: { className?: string }) {
  const frame = useLoop(steps);
  return (
    <div role="group" aria-label={previewLabels.ReloadPreview} className={cn('grid gap-4 sm:grid-cols-2', className)}>
      <MddogFrame label="Vim typing a new open question into plan.md and saving it" cols={COLS} rows={ROWS} bar={null}>
        <Vim {...frame} />
      </MddogFrame>
      <MddogFrame
        label="mddog showing plan.md, which reloads when Vim saves it"
        cols={COLS}
        rows={ROWS}
        bar={<StatusBar left="plan.md › Rollout" hints={[['?', 'help']]} pos="100%" />}
      >
        <Preview {...frame} />
      </MddogFrame>
    </div>
  );
}
