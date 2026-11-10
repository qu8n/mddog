import { whyMarkdown } from '@/components/why-mddog-data';

/**
 * What each live preview shows. The previews use these as their screen-reader
 * labels, and the Markdown export puts them where the preview tags were.
 */
export const previewLabels = {
  TablesPreview: 'mddog showing a tip alert and a table',
  CodePreview: 'A Rust code block with syntax colors, a warning alert, and a diff',
  DiagramPreview: 'A Mermaid flowchart and a typeset formula in mddog',
  SearchPreview: 'Search results for "trail" highlighted on the page',
  TocPreview: 'The table of contents open on the left of a document',
  FileListPreview: 'The file list showing Markdown files with their headings and ages',
  MarkdownPreview: 'A release plan in mddog with front matter, a note, a task list, tables, lists, a quote, a diff, and a footnote',
  WideTablePreview: 'A wide table with a scroll bar along its bottom edge',
  ClaudeCodePreview: 'Claude Code checks off a goal in plan.md, and the mddog pane beside the chat shows the check at once',
  SelectPreview: 'A selected phrase in a paragraph, and the status bar saying Copied the selection',
  CodeCopyPreview: 'A code block with its copy button in the top right corner',
  DiagramButtonsPreview: 'A Mermaid flowchart with expand and copy buttons in its top right corner',
  DetailsPreview: 'A closed details block under an Open questions heading',
  ReloadPreview: 'Vim on the left types a new line into plan.md and saves it, and mddog on the right reloads to show the new line',
  ThemesPreview: 'One page shown in the four built-in themes: Obsidian light and dark, GitHub light and dark',
} as const;

/**
 * Turns the live components in MDX source into text for the Markdown export. Each `<...Preview />` tag
 * becomes a line naming what it shows, `<WhyMddogTable />` becomes its Markdown table, and `<Yes />` and
 * `<No />` become words.
 */
export function previewsToText(markdown: string) {
  return markdown
    .replace(/<WhyMddogTable\b[^>]*\/>/g, whyMarkdown)
    .replace(/<Yes \/>/g, 'Yes')
    .replace(/<No \/>/g, 'No')
    .replace(/<(\w+Preview)\b[^>]*\/>/g, (tag, name: string) =>
    name in previewLabels ? `(Picture: ${previewLabels[name as keyof typeof previewLabels]}.)` : tag,
  );
}
