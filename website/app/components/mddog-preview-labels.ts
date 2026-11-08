/**
 * What each live preview shows. The previews use these as their screen-reader
 * labels, and the Markdown export puts them where the preview tags were.
 */
export const previewLabels = {
  TablesPreview: 'mddog showing a tip alert and a table',
  CodePreview: 'A Rust code block with syntax colors, a warning alert, and a diff',
  DiagramPreview: 'A Mermaid flowchart and a typeset formula in mddog',
  SearchPreview: 'Search results for "trail" highlighted on the page',
  TocPreview: 'The contents panel open on the left of a document',
  FileListPreview: 'The file list showing Markdown files with their headings and ages',
  MarkdownPreview: 'A release plan in mddog, with a note and a task list',
  WideTablePreview: 'A wide table with a scroll bar along its bottom edge',
} as const;

/** Replaces `<FileListPreview ... />` tags in MDX source with a line of text naming what they show. */
export function previewsToText(markdown: string) {
  return markdown.replace(/<(\w+Preview)\b[^>]*\/>/g, (tag, name: string) =>
    name in previewLabels ? `(Picture: ${previewLabels[name as keyof typeof previewLabels]}.)` : tag,
  );
}
