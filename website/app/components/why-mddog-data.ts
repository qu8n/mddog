/** The "Why mddog?" comparison, shared by the table component and the Markdown export of the docs. */

export const whyColumns = ['mddog', 'Text-based terminal viewers', 'Code editor preview'];

// Each row holds one cell per column: true is yes, false is no, and a string is shown as is.
export const whyRows: { label: string; cells: (boolean | string)[] }[] = [
  { label: 'Stays in the terminal', cells: [true, true, false] },
  { label: 'Real fonts and heading sizes', cells: [true, false, true] },
  { label: 'Diagrams and math drawn as graphics', cells: [true, false, 'Some'] },
  { label: 'Images', cells: [true, 'Some', true] },
  { label: 'Live reload', cells: [true, 'Some', true] },
  { label: 'Preview pane in Claude Code', cells: [true, false, false] },
];

export const whyNote =
  'Compared in November 2026 to terminal viewers like glow, leaf, mdcat, Frogmouth, and rich-cli, and to the built-in Markdown previews in VS Code, Cursor, Zed, and JetBrains IDEs.';

/** The comparison as a Markdown table, with the note under it in italics. */
export function whyMarkdown() {
  const cell = (c: boolean | string) => (c === true ? 'Yes' : c === false ? 'No' : c);
  return [
    `| | ${whyColumns.join(' | ')} |`,
    `|---|${whyColumns.map(() => ':---:').join('|')}|`,
    ...whyRows.map((r) => `| ${r.label} | ${r.cells.map(cell).join(' | ')} |`),
    '',
    `*${whyNote}*`,
  ].join('\n');
}
