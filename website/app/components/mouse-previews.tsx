import { useEffect, useRef, useState } from 'react';
import {
  CodeBlock,
  CodeCopiedIcon,
  CopyIcon,
  ExpandIcon,
  type Hint,
  MddogFrame,
  OcticonCheck,
  OcticonCopy,
  Page,
  StatusBar,
} from '@/components/mddog-frame';
import { previewLabels } from '@/components/mddog-preview-labels';
import { PlanDiagram, PlanDiagramSvg } from '@/components/mddog-previews';

/*
 * Mouse previews for the Mouse support page. Each one works as mddog does: the buttons copy to the
 * clipboard, show "Copied!" and a green check for two seconds, and say what they copied in the status bar.
 */

type PreviewProps = { className?: string };

/** How long a copy button shows its check, as COPY_FEEDBACK in src/viewer/preview.rs. */
const COPY_FEEDBACK_MS = 2000;

/** Copies text and remembers which button did it, for two seconds. */
function useCopy<T extends string>() {
  const [copied, setCopied] = useState<T | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = (which: T, text: string) => {
    void navigator.clipboard?.writeText(text).catch(() => {});
    setCopied(which);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(null), COPY_FEEDBACK_MS);
  };
  return [copied, copy] as const;
}

/** The label mddog draws to the left of a button. */
function Tip({ children }: { children: string }) {
  return <span className="mdd-tip">{children}</span>;
}

/** A selection just copied with Cmd-c: the selected words in the selection color, and the notice mddog shows. */
export function SelectPreview({ className }: PreviewProps) {
  return (
    <MddogFrame
      label={previewLabels.SelectPreview}
      className={className}
      rows={9}
      bar={<StatusBar left=" Copied the selection " />}
    >
      <Page>
        <h2>Water</h2>
        <p>
          Carry a litre for every two hours of walking, and more on hot days.{' '}
          <span className="mdd-sel">Refill from streams only above the last farm</span>, and boil or filter what you
          take.
        </p>
      </Page>
    </MddogFrame>
  );
}

const installCode = 'brew install qu8n/tap/mddog';

export function CodeCopyPreview({ className }: PreviewProps) {
  const [copied, copy] = useCopy<'code'>();
  const [hover, setHover] = useState(false);
  return (
    <MddogFrame
      label={previewLabels.CodeCopyPreview}
      className={className}
      rows={11}
      bar={<StatusBar left={copied ? ' Copied the code ' : 'install.md'} />}
    >
      <Page>
        <h2>Install</h2>
        <p>Run this in your terminal:</p>
        <CodeBlock
          button={
            <span
              className="mdd-copy mdd-live"
              data-copied={copied ? true : undefined}
              onMouseEnter={() => setHover(true)}
              onMouseLeave={() => setHover(false)}
              onClick={() => copy('code', installCode)}
            >
              {copied ? <CodeCopiedIcon /> : <CopyIcon />}
              {copied ? <Tip>Copied!</Tip> : hover && <Tip>Copy code to clipboard</Tip>}
            </span>
          }
        >
          <span className="mdd-line">
            {installCode}
          </span>
        </CodeBlock>
      </Page>
    </MddogFrame>
  );
}

const planSource = `flowchart LR
  gpx[GPX files] --> parse(Parse trails)
  api[Weather API] --> forecast(Forecast)
  parse --> score{Score}
  forecast --> score
  score --> top[Top 3 hikes]`;

const expandedHints: Hint[] = [
  ['q', 'quit'],
  ['Esc', 'back to the page'],
];

export function DiagramButtonsPreview({ className }: PreviewProps) {
  const [copied, copy] = useCopy<'diagram'>();
  const [expanded, setExpanded] = useState(false);
  // mddog closes the expanded diagram on a click, and on any key but q and Ctrl-c, which quit.
  useEffect(() => {
    if (!expanded) return;
    const close = (e: KeyboardEvent) => {
      if (e.key === 'q' || e.key === 'Q' || (e.ctrlKey && e.key === 'c')) return;
      setExpanded(false);
    };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [expanded]);
  const bar = expanded ? (
    <StatusBar left="pipeline.md" hints={expandedHints} />
  ) : (
    <StatusBar left={copied ? ' Copied the diagram source ' : 'pipeline.md'} />
  );
  return (
    <MddogFrame label={previewLabels.DiagramButtonsPreview} className={className} rows={16} bar={bar}>
      {expanded ? (
        <div className="mdd-expanded mdd-live" onClick={() => setExpanded(false)}>
          <PlanDiagramSvg />
        </div>
      ) : (
        <Page>
          <h2>How a plan gets made</h2>
          <PlanDiagram
            buttons={
              <div className="mdd-diagram-buttons">
                <span className="mdd-live" onClick={() => setExpanded(true)}>
                  <ExpandIcon />
                </span>
                <span
                  className="mdd-live"
                  data-copied={copied ? true : undefined}
                  onClick={() => copy('diagram', planSource)}
                >
                  {copied ? <OcticonCheck /> : <OcticonCopy />}
                  {copied && <Tip>Copied!</Tip>}
                </span>
              </div>
            }
          />
        </Page>
      )}
    </MddogFrame>
  );
}

export function DetailsPreview({ className }: PreviewProps) {
  return (
    <MddogFrame label={previewLabels.DetailsPreview} className={className} rows={9} bar={<StatusBar left="plan.md" />}>
      <Page>
        <h2>Open questions</h2>
        <details>
          <summary>Should the cache keep starred trails?</summary>
          <p>Yes, until the user removes the star. Starred trails don't count toward the 30 day cleanup.</p>
        </details>
      </Page>
    </MddogFrame>
  );
}
