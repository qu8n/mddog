import type { ReactNode } from 'react';
import { Alert, CodeBlock, CopyIcon, type Hint, MddogFrame, Page, StatusBar } from '@/components/mddog-frame';
import { previewLabels } from '@/components/mddog-preview-labels';
import { cn } from '@/lib/cn';

/*
 * Live copies of mddog screenshots. Each one draws the same sample page the
 * screenshot showed, as real text, so visitors can select and copy it.
 */

type PreviewProps = { className?: string };

function Line({ children }: { children?: ReactNode }) {
  return <span className="mdd-line">{children ?? ' '}</span>;
}

export function TablesPreview({ className }: PreviewProps) {
  return (
    <MddogFrame label={previewLabels.TablesPreview} className={className} bar={<StatusBar left="markdown.md" />}>
      <Page>
        <h1>Trailmix</h1>
        <Alert kind="tip">
          <p>
            Run <code>trailmix plan --near home</code> to get three hikes for this weekend.
          </p>
        </Alert>
        <h2>Options</h2>
        <table>
          <thead>
            <tr>
              <th>Flag</th>
              <th>What it does</th>
              <th>Default</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td><code>--near</code></td>
              <td>Start from a saved place</td>
              <td><code>home</code></td>
            </tr>
            <tr>
              <td><code>--max-km</code></td>
              <td>Skip trails longer than this</td>
              <td><code>15</code></td>
            </tr>
            <tr>
              <td><code>--rain</code></td>
              <td>Allow trails when rain is likely</td>
              <td>off</td>
            </tr>
          </tbody>
        </table>
      </Page>
    </MddogFrame>
  );
}

const k = (s: string) => <span className="pl-k">{s}</span>;
const c = (s: string) => <span className="pl-c1">{s}</span>;
const e = (s: string) => <span className="pl-en">{s}</span>;

export function CodePreview({ className }: PreviewProps) {
  return (
    <MddogFrame
      label={previewLabels.CodePreview}
      className={className}
      bar={<StatusBar left="code.md" />}
    >
      <Page>
        <h2>How it scores a trail</h2>
        <CodeBlock plain>
          <Line>
            {k('fn')} {e('score')}(t: {c('&Trail')}, {c('w')}: {c('&Forecast')}) {'->'} {c('f32')} {'{'}
          </Line>
          <Line>
            {'    '}{k('let')} dry = {c('1.0')} - w.{c('rain_chance')};
          </Line>
          <Line>
            {'    '}{k('let')} fit = (t.{c('km')} / {c('MAX_KM')}).{e('min')}({c('1.0')});
          </Line>
          <Line>
            {'    '}{c('0.6')} {c('*')} dry + {c('0.4')} {c('*')} ({c('1.0')} - fit)
          </Line>
          <Line>{'}'}</Line>
        </CodeBlock>
        <Alert kind="warning">
          <p>Scores are only as good as the forecast. Check before you go.</p>
        </Alert>
        <CodeBlock>
          <span className="mdd-line pl-del">- let fit = t.km / MAX_KM;</span>
          <span className="mdd-line pl-ins">+ let fit = (t.km / MAX_KM).min(1.0);</span>
        </CodeBlock>
      </Page>
    </MddogFrame>
  );
}

/** The Mermaid flowchart from the screenshot, drawn by hand in its page coordinates. */
function PlanDiagram() {
  const node = { fill: '#161b22', stroke: '#3d444d', strokeWidth: 1.5 };
  const edge = { fill: 'none', stroke: '#9198a1', strokeWidth: 2, markerEnd: 'url(#mdd-arrow)' };
  const label = { fill: '#f0f6fc', fontSize: 16, textAnchor: 'middle', dominantBaseline: 'central' } as const;
  return (
    <div className="mdd-diagram">
      <svg viewBox="24 78 750 206">
        <defs>
          <marker id="mdd-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto">
            <path d="M0 0 10 5 0 10z" fill="#9198a1" />
          </marker>
        </defs>
        <rect x="91.5" y="96.5" width="617.5" height="168.5" fill="#0f141b" />
        <path d="M202 127H265" {...edge} />
        <path d="M219 225.5H265" {...edge} />
        <path d="M410 149.5C440 149.5 445 166 465 166" {...edge} />
        <path d="M371 201C410 201 420 176 456 176" {...edge} />
        <path d="M522 176H568" {...edge} />
        <rect x="98" y="103" width="103.5" height="48.5" rx="2" {...node} />
        <rect x="267" y="102" width="143" height="50.5" rx="10" {...node} />
        <rect x="98" y="201.5" width="121" height="48.5" rx="2" {...node} />
        <rect x="267" y="200.5" width="104" height="50.5" rx="10" {...node} />
        <path d="M490 143.5 522.25 176 490 208.5 457.75 176z" {...node} />
        <rect x="570.5" y="152.5" width="121.5" height="48" rx="2" {...node} />
        <text x="149.75" y="127.25" {...label}>GPX files</text>
        <text x="338.5" y="127.25" {...label}>Parse trails</text>
        <text x="158.5" y="225.75" {...label}>Weather API</text>
        <text x="319" y="225.75" {...label}>Forecast</text>
        <text x="490" y="176" {...label}>Score</text>
        <text x="631.25" y="176.5" {...label}>Top 3 hikes</text>
      </svg>
      <div className="mdd-diagram-buttons" aria-hidden>
        <span>
          <svg viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
            <path d="M2 9h14M5 6 2 9l3 3M13 6l3 3-3 3" />
          </svg>
        </span>
        <span>
          <CopyIcon />
        </span>
      </div>
    </div>
  );
}

/**
 * s = 0.6 (1 − p_rain) + 0.4 (1 − min(d / d_max, 1)), as MathML so the browser
 * typesets it. React's types have no MathML elements, so it goes in as markup.
 */
const scoreFormula = [
  '<math display="block">',
  '<mi>s</mi><mo>=</mo><mn>0.6</mn><mo stretchy="false">(</mo><mn>1</mn><mo>−</mo>',
  '<msub><mi>p</mi><mtext>rain</mtext></msub><mo stretchy="false">)</mo><mo>+</mo><mn>0.4</mn>',
  '<mrow><mo>(</mo><mn>1</mn><mo>−</mo><mi>min</mi>',
  '<mrow><mo>(</mo><mfrac><mi>d</mi><msub><mi>d</mi><mtext>max</mtext></msub></mfrac>',
  '<mo separator="true">,</mo><mn>1</mn><mo>)</mo></mrow>',
  '<mo>)</mo></mrow>',
  '</math>',
].join('');

function ScoreFormula() {
  return <div className="mdd-math" dangerouslySetInnerHTML={{ __html: scoreFormula }} />;
}

export function DiagramPreview({ className }: PreviewProps) {
  return (
    <MddogFrame
      label={previewLabels.DiagramPreview}
      className={className}
      bar={<StatusBar left="pipeline.md" />}
    >
      <Page>
        <h2>How a plan gets made</h2>
        <PlanDiagram />
        <p>Each trail scores between 0 and 1:</p>
        <ScoreFormula />
      </Page>
    </MddogFrame>
  );
}

const fieldGuide = {
  start: 'Pack water, a map, and a light jacket. Tell someone which ',
  route: ' that fits the daylight you have. A good pace on flat ground is about 4 km an hour. Add an hour for every 600 m of climbing.',
  trail: 'Stay on the marked path. Step aside for people going uphill. Keep dogs on a lead near livestock.',
};

/** The sample "Field guide" page. `match` highlights every "trail", the first one as current. */
function FieldGuide({ match }: { match?: boolean }) {
  const t = (current?: boolean) =>
    match ? <mark className={cn(current && 'mdd-current')}>trail</mark> : 'trail';
  return (
    <Page>
      <h1>Field guide</h1>
      <h2>Getting started</h2>
      <p>
        {fieldGuide.start}
        {t(true)} you are taking.
      </p>
      <h2>Planning a route</h2>
      <p>
        Pick a {t()}
        {fieldGuide.route}
      </p>
      <h2>On the {t()}</h2>
      <p>{fieldGuide.trail}</p>
      <h2>Water</h2>
      <p>Carry a litre for every two hours of walking, and more on hot days.</p>
    </Page>
  );
}

export function SearchPreview({ className }: PreviewProps) {
  const hints: Hint[] = [
    ['q', 'quit'],
    ['n/N', 'next/prev'],
    ['Esc', 'clear'],
    ['?', 'help'],
  ];
  return (
    <MddogFrame
      label={previewLabels.SearchPreview}
      className={className}
      bar={<StatusBar left={'guide.md  "trail" 1/4'} hints={hints} />}
    >
      <FieldGuide match />
    </MddogFrame>
  );
}

const tocEntries = [
  'Getting started',
  'Planning a route',
  'On the trail',
  '  Water',
  '  Weather',
  'Leave no trace',
  'Getting home',
];

export function TocPreview({ className }: PreviewProps) {
  return (
    <MddogFrame
      label={previewLabels.TocPreview}
      className={className}
      bar={<StatusBar left="guide.md" />}
    >
      <div className="mdd-toc mdd-mono">
        <div className="mdd-toc-title">TABLE OF CONTENTS</div>
        <div className="mdd-toc-rule" />
        {tocEntries.map((entry, i) => (
          <span key={entry} className={cn('mdd-row', i === 0 && 'mdd-row-selected')}>
            {entry}
          </span>
        ))}
      </div>
      <FieldGuide />
    </MddogFrame>
  );
}

const files: [path: string, title: string, age: string][] = [
  ['notes/gear-review.md', 'Gear review: rain jackets', '2m'],
  ['notes/trail-log.md', 'Trail log for October', '40m'],
  ['notes/recipes.md', 'Camp stove recipes', '3h'],
  ['notes/packing-list.md', 'Packing list', '5h'],
  ['README.md', 'Trailmix', '9h'],
  ['trailmix.md', 'Trailmix', '12h'],
  ['notes/trips/dolomites.md', 'Dolomites, five days', '20h'],
  ['notes/trips/lake-district.md', 'Lake District weekend', '1d'],
  ['guide.md', 'Field guide', '2d'],
  ['pipeline.md', 'Data pipeline', '3d'],
  ['CHANGELOG.md', 'Changelog', '2w'],
];

export function FileListPreview({ className }: PreviewProps) {
  const hints: Hint[] = [
    ['↑↓', 'move'],
    ['Enter', 'open'],
    ['Esc', 'back'],
    ['Ctrl-u', 'clear'],
  ];
  return (
    <MddogFrame
      label={previewLabels.FileListPreview}
      className={className}
      bar={<StatusBar left={null} hints={hints} pos={null} />}
    >
      <div className="mdd-term mdd-mono mdd-files">
        <div className="mdd-files-head">
          <span className="mdd-bright mdd-b">~/trailmix</span>
          <span className="mdd-dim">11 files</span>
        </div>
        <div className="mdd-bright mdd-b">
          {'> '}
          <span className="mdd-cursor" />
        </div>
        <div className="mdd-files-rule" />
        {files.map(([path, title, age], i) => (
          <div key={path} className={cn('mdd-file', i === 0 && 'mdd-file-selected')}>
            <span />
            <span>{path}</span>
            <span>{title}</span>
            <span>{age}</span>
          </div>
        ))}
      </div>
    </MddogFrame>
  );
}

function Check({ checked }: { checked?: boolean }) {
  return (
    <span className="mdd-check" data-checked={checked || undefined}>
      {checked && (
        <svg viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
          <path d="M2 5.2 4.2 7.4 8 3" />
        </svg>
      )}
    </span>
  );
}

export function MarkdownPreview({ className }: PreviewProps) {
  return (
    <MddogFrame
      label={previewLabels.MarkdownPreview}
      className={className}
      bar={<StatusBar left="plan.md" />}
    >
      <Page>
        <h1>Trail cache: release plan</h1>
        <p>Trail cache keeps offline map tiles for hikers. This plan covers the 2.0 release.</p>
        <Alert kind="note">
          <p>The cache now stores tiles by trail, not by region, so a trip downloads only what it needs.</p>
        </Alert>
        <h2>Goals</h2>
        <ul className="mdd-tasks">
          <li>
            <Check checked />
            Download a trail's tiles in one tap
          </li>
          <li>
            <Check checked />
            Show the cache size per trail
          </li>
          <li>
            <Check />
            Remove old trails on their own after 30 days
          </li>
        </ul>
        <h2>Sizes</h2>
      </Page>
    </MddogFrame>
  );
}

// Enough columns that the table runs well past the page edge, so the scroll bar has room to move.
const regionColumns = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun', 'Week total', 'Avg / day', 'vs last week', 'Cache hits', '5xx errors', 'p95 latency'];

const regions: [region: string, numbers: string[], peak: string, zoom: string][] = [
  ['North coast', ['12,410', '11,982', '13,005', '12,770', '14,220', '21,904', '23,118', '109,409', '15,630', '+8.2%', '94.1%', '212', '184 ms'], 'Sat 09:00', 'z14'],
  ['Lake district', ['8,204', '7,955', '8,390', '8,112', '9,870', '15,402', '16,033', '73,966', '10,567', '+3.5%', '92.7%', '148', '201 ms'], 'Sun 08:00', 'z15'],
  ['High plateau', ['3,120', '2,998', '3,240', '3,305', '4,011', '7,880', '8,204', '32,758', '4,680', '-1.9%', '89.3%', '67', '236 ms'], 'Sat 07:00', 'z13'],
];

export function WideTablePreview({ className }: PreviewProps) {
  return (
    <MddogFrame
      label={previewLabels.WideTablePreview}
      className={className}
      bar={<StatusBar left="server-logs.md" />}
    >
      <Page>
        <h1>Tile server logs</h1>
        <h2>Requests per region</h2>
        <div className="mdd-scroll-x">
          <table className="mdd-wide">
            <thead>
              <tr>
                <th>Region</th>
                {regionColumns.map((d) => (
                  <th key={d} className="mdd-num">
                    {d}
                  </th>
                ))}
                <th>Peak hour</th>
                <th>Top zoom</th>
              </tr>
            </thead>
            <tbody>
              {regions.map(([region, numbers, peak, zoom]) => (
                <tr key={region}>
                  <td>{region}</td>
                  {numbers.map((n, i) => (
                    <td key={i} className="mdd-num">
                      {n}
                    </td>
                  ))}
                  <td>{peak}</td>
                  <td>{zoom}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Page>
    </MddogFrame>
  );
}
