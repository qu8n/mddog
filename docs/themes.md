# Writing a theme

A theme is one CSS file. It styles the page and colors mddog's terminal parts (file list, status bar, table of contents, help, loading skeleton) to match. A theme can only recolor the page, as the GitHub themes do, or restyle all of it, as the Obsidian themes do (fonts, headings, lists, links, code, callouts).

The page loads its CSS in this order: GitHub's Markdown CSS ([github-markdown-css](https://github.com/sindresorhus/github-markdown-css), without colors), the GitHub theme of the same `color-scheme`, your theme's base if it has one (see below), your theme, mddog's fixed page rules (background, margins, width), then `style.css`. So a theme sets only what it changes. The GitHub themes set Primer's color variables, and [`assets/themes/github-dark.css`](../assets/themes/github-dark.css) lists them. The page text is `--fgColor-default`, and `--mddog-fg` colors only the terminal parts.

Declare the mode and the terminal colors on `:root`:

```css
:root {
  color-scheme: dark;              /* light or dark: the page's appearance */
  --mddog-bg: #0d1117;             /* page and file list background */
  --mddog-fg: #c9d1d9;             /* normal text */
  --mddog-fg-muted: #9198a1;       /* dim text: ages, titles, hint labels */
  --mddog-fg-bright: #f0f6fc;      /* names, keys, headings */
  --mddog-selection-bg: #1f2328;   /* selected row in the list and the TOC */
  --mddog-bar-bg: #161b22;         /* bottom status bar */
  --mddog-border: #3d444d;         /* separators, TOC divider */
  --mddog-skeleton: #25292e;       /* loading skeleton bars */
  --mddog-mermaid: pele;           /* optional: pele or obsidian; unset, the base's, else pele; the config's engine wins */
}
```

- mddog reads these from top-level `:root` rules as text, so colors must be literal `#rgb` or `#rrggbb`, not `var()`. A color you leave out comes from the base, else from the GitHub theme of the same mode.
- `color-scheme` is required unless the theme has a base.
- Set the page background with `--mddog-bg`. A theme's own `body` background or `--bgColor-default` doesn't show.
- mddog fixes the page's outer margins and width with `!important`. Style what is inside `article.markdown-body` instead.
- A relative `url()` or `@import` starts from the theme file's folder.
- mddog reads the theme at startup. Restart it to see a change.

## Building on another theme

Name a base with `--mddog-base`, a built-in name or a `.css` path. This gives `obsidian-dark` a navy background:

```css
:root { --mddog-base: obsidian-dark; --mddog-bg: #202a44; }
```

The base loads first (with its own base, if any), then this file, so the file keeps the base's updates. It takes the base's `color-scheme` and Mermaid engine. A different `color-scheme`, or a chain of bases that loops, stops mddog with an error.
