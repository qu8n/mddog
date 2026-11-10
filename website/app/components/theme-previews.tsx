import { useState } from 'react';
import { Alert, CodeBlock, ExpandIcon, OcticonCopy, StatusBar, TitleBar } from '@/components/mddog-frame';
import { previewLabels } from '@/components/mddog-preview-labels';
import { cn } from '@/lib/cn';
import './mddog-preview.css';

/*
 * The built-in themes, each drawing the same sample page as live text. On a
 * wide container the four windows overlap in a cascade, as macOS stacks them.
 * Click a window to bring it to the front. A narrow one stacks them in a grid.
 */

const themes = ['obsidian-light', 'obsidian-dark', 'github-light', 'github-dark'] as const;
type Theme = (typeof themes)[number];

/** E = mc², inline, and Σ i = n(n + 1) / 2, as a block. React's types have no MathML, so they go in as markup. */
const energy =
  '<math><mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup></math>';
const sum = [
  '<math display="block">',
  '<mrow><munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover><mi>i</mi></mrow>',
  '<mo>=</mo><mfrac><mrow><mi>n</mi><mo stretchy="false">(</mo><mi>n</mi><mo>+</mo><mn>1</mn><mo stretchy="false">)</mo></mrow><mn>2</mn></mfrac>',
  '</math>',
].join('');

function Check({ done }: { done?: boolean }) {
  return (
    <span className="mdd-check" data-checked={done || undefined}>
      {done && (
        <svg viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
          <path d="M2 5.2 4.2 7.4 8 3" />
        </svg>
      )}
    </span>
  );
}

/** Markdown → mddog → Themed page, in each theme's diagram colors. */
function DemoDiagram() {
  const node = { className: 'mdd-node' };
  const edge = { className: 'mdd-edge', strokeWidth: 1.5, markerEnd: 'url(#mdd-theme-arrow)' };
  const label = { className: 'mdd-node-label', fontSize: 18, textAnchor: 'middle', dominantBaseline: 'central' } as const;
  return (
    <div className="mdd-diagram mdd-theme-diagram">
      <svg viewBox="0 0 620 120">
        <defs>
          <marker id="mdd-theme-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
            <path d="M0 0 10 5 0 10z" className="mdd-arrow" />
          </marker>
        </defs>
        <path d="M150 60H210" {...edge} />
        <path d="M368 60H428" {...edge} />
        <rect x="2" y="32" width="146" height="56" {...node} />
        <path d="M289 2 366 60 289 118 212 60z" {...node} />
        <rect x="430" y="32" width="178" height="56" {...node} />
        <text x="75" y="60" {...label}>Markdown</text>
        <text x="289" y="60" {...label}>mddog</text>
        <text x="519" y="60" {...label}>Themed page</text>
      </svg>
      <div className="mdd-diagram-buttons" aria-hidden>
        <span>
          <ExpandIcon />
        </span>
        <span>
          <OcticonCopy />
        </span>
      </div>
    </div>
  );
}

/** One macOS window showing the sample page in a theme. */
function ThemeWindow({ theme, front }: { theme: Theme; front: boolean }) {
  return (
    <div className="mdd mdd-window" data-theme={theme} aria-hidden>
      <TitleBar inactive={!front} />
      <div className="mdd-screen">
        <div className="mdd-page">
          <h1>Theme demo</h1>
          <p>
            Plain text with <strong>bold</strong>, <em>italic</em>,{' '}
            <strong>
              <em>bold italic</em>
            </strong>
            , <del>strikethrough</del>, <code>inline code</code>, and <kbd>Ctrl</kbd> + <kbd>c</kbd>. Water is H
            <sub>2</sub>O, energy is <span dangerouslySetInnerHTML={{ __html: energy }} />, and a link to{' '}
            <a className="mdd-ext">GitHub Docs</a>.
          </p>
          <Alert kind="note">
            <p>Alerts and &gt; quotes render too.</p>
          </Alert>
          <ul className="mdd-tasks mdd-theme-tasks">
            <li>
              <Check />
              <span>
                Task with a <a className="mdd-ext">link</a>
              </span>
            </li>
            <li className="mdd-nested">
              <Check done />
              <span className="mdd-done">Nested completed task</span>
            </li>
          </ul>
          <table>
            <thead>
              <tr>
                <th>Feature</th>
                <th className="mdd-center">Supported</th>
                <th className="mdd-num">Notes</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Escaped pipe</td>
                <td className="mdd-center">
                  <code>a | b</code>
                </td>
                <td className="mdd-num">A pipe inside code</td>
              </tr>
              <tr>
                <td>Formatting</td>
                <td className="mdd-center">
                  <em>yes</em>
                </td>
                <td className="mdd-num">
                  <del>old</del> new
                </td>
              </tr>
            </tbody>
          </table>
          <div className="mdd-math" dangerouslySetInnerHTML={{ __html: sum }} />
          <CodeBlock>
            <span className="mdd-line">
              <span className="pl-k">fn</span> <span className="pl-en">double</span>(n: i32) {'->'} i32 {'{'}
            </span>
            <span className="mdd-line">
              {'    '}n <span className="pl-c1">*</span> <span className="pl-c1">2</span>
            </span>
            <span className="mdd-line">{'}'}</span>
          </CodeBlock>
          <DemoDiagram />
        </div>
      </div>
      <StatusBar left="theme-demo.md" />
    </div>
  );
}

/** The four built-in themes, labeled, in the cascade from the old screenshot. */
export function ThemesPreview({ className }: { className?: string }) {
  // Which window is in front. The rest keep their stacking order behind it.
  const [front, setFront] = useState<Theme>('github-dark');
  return (
    <figure
      role="img"
      aria-label={previewLabels.ThemesPreview}
      className={cn('@container not-prose', className)}
    >
      <div className="mdd-themes grid gap-8 @xl:block">
        {themes.map((theme, i) => (
          <div
            key={theme}
            className="mdd-theme-slot"
            style={{ '--i': i, zIndex: theme === front ? themes.length : i } as React.CSSProperties}
            onClick={() => setFront(theme)}
          >
            <div className="mdd-theme-name">{theme}</div>
            <div className="mdd-wrap">
              <ThemeWindow theme={theme} front={theme === front} />
            </div>
          </div>
        ))}
      </div>
    </figure>
  );
}
