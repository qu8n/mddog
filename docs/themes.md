# Writing a theme

A theme is one file of tokens. It sets the page's colors and type, and it colors mddog's terminal parts (file list, status bar, table of contents, help, loading skeleton) to match. The file keeps the `.css` extension and CSS's `:root { ... }` syntax, but it is not a style sheet. mddog reads only the tokens below, and anything else in the file is an error.

```css
:root {
  color-scheme: dark;
  --mddog-base: github-dark;
  --mddog-bg: #101418;
  --mddog-link: #6cb6ff;
  --mddog-font-body: "Avenir Next", system-ui;
}
```

- `color-scheme` is `light` or `dark`. It is required unless the theme has a base.
- Colors are literal `#rgb` or `#rrggbb`. `var()`, `rgb()`, names and alpha are errors.
- A token you leave out comes from the base, else from the GitHub theme of the same mode. So a theme sets only what it changes.
- An unknown token, a bad value, or any rule other than `:root` stops mddog with an error that names the file and the token.
- mddog reads the theme at startup. Restart it to see a change.

## Tokens

### Terminal parts

| Token | Colors |
|---|---|
| `--mddog-bg` | the page and file list background |
| `--mddog-fg` | normal text |
| `--mddog-fg-muted` | dim text: ages, titles, hint labels |
| `--mddog-fg-bright` | names, keys, headings |
| `--mddog-selection-bg` | the selected row in the list and the table of contents |
| `--mddog-bar-bg` | the bottom status bar |
| `--mddog-border` | separators and the table of contents divider |
| `--mddog-skeleton` | the loading skeleton bars |

### Page

| Token | Colors |
|---|---|
| `--mddog-text` | body text |
| `--mddog-text-muted` | level 6 headings and other quiet text |
| `--mddog-heading` | headings 1 to 5 |
| `--mddog-link` | links |
| `--mddog-accent` | accents, such as diagram borders under the Obsidian Mermaid engine |
| `--mddog-code-bg` | inline code |
| `--mddog-pre-bg` | code blocks, diagrams and keys |
| `--mddog-alt-bg` | every other table row |
| `--mddog-rule` | rules, heading lines, quote bars and table borders |
| `--mddog-mark-bg` | `<mark>` text |
| `--mddog-match-bg` | search matches |
| `--mddog-match-current-bg` | the current search match |
| `--mddog-text-selection` | selected text |
| `--mddog-alert-note`, `-tip`, `-important`, `-warning`, `-caution` | each alert's bar, icon and title |
| `--mddog-diff-add-bg`, `--mddog-diff-del-bg` | added and removed lines in a `diff` block |

### Code

| Token | Colors |
|---|---|
| `--mddog-pl-comment` | comments |
| `--mddog-pl-keyword` | keywords and operators |
| `--mddog-pl-string` | strings |
| `--mddog-pl-constant` | numbers, constants and types |
| `--mddog-pl-entity` | function names |
| `--mddog-pl-inserted`, `--mddog-pl-deleted` | the text of added and removed `diff` lines |

### Type

| Token | Value | GitHub's |
|---|---|---|
| `--mddog-font-body` | font names, first choice first | `system-ui` |
| `--mddog-font-code` | font names, first choice first | `ui-monospace, SFMono-Regular, Menlo` |
| `--mddog-heading-scale` | six sizes in em, for `h1` to `h6` | `2 1.5 1.25 1 0.875 0.85` |
| `--mddog-line-height` | body line height, in em | `1.5` |
| `--mddog-code-size` | code size, in em of the text around it | `0.85` |

A font list works like CSS's `font-family`: mddog takes the first installed font. `system-ui` is the macOS system font, and `ui-monospace` is SF Mono. Quote a name with spaces. When no name matches, body text uses the system font and code uses Menlo.

### Other

| Token | Sets |
|---|---|
| `--mddog-base` | the theme this one builds on (see below) |
| `--mddog-mermaid` | the Mermaid engine, `pele` or `obsidian`. Unset, the base's, else `pele`. The config's `[mermaid] engine` wins. |

## Building on another theme

Name a base with `--mddog-base`, a built-in name or a `.css` path. A relative path starts from the theme file's folder. This gives `obsidian-dark` a navy background:

```css
:root { --mddog-base: obsidian-dark; --mddog-bg: #202a44; }
```

The theme starts from every token of its base, then sets its own, so it keeps the base's updates. It takes the base's `color-scheme` and Mermaid engine. A different `color-scheme`, or a chain of bases that loops, stops mddog with an error.

The four built-in themes in [`assets/themes/`](../assets/themes/) set every token, so they are a full example.
