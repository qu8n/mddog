# Architecture

This file is a map of mddog's code. It says what each part does, where to find it, and which rules hold between the parts. It covers only the things that change rarely. For how a part works inside, read its module comments and code. To search for a name below, use your editor's symbol search. This file names items but does not link to them, because links go stale.

Related docs:

- `CONTRIBUTING.md`: checks, tests, releases, debug tools, and environment variables.
- `docs/behavior.md`: the contract for keys and events, with a test for each row.
- [Themes](https://mddog.pages.dev/docs/themes) on the docs site: how to write a theme.

## Bird's-eye view

mddog shows a Markdown file in the terminal the way GitHub shows it in a browser. A terminal grid of characters can't do that, so mddog draws the page as pixels and sends them as images.

The input is a Markdown file and a pane size in cells and pixels. The output is a column of image tiles. Each tile is one strip of the page, as wide as the pane, sent to the terminal with the Kitty graphics protocol. Keys and the mouse scroll, search, select, and follow links. The page reloads when the file changes.

On the way, the file goes through four forms:

```mermaid
flowchart LR
  A["Markdown text"] -- "render::doc::parse" --> B["Doc: block tree"]
  B -- "render::layout::layout" --> C["Page: display list"]
  C -- "Engine::snapshot" --> D["RGBA tiles"]
  D --> E["viewer: Kitty images"]
  D --> F["headless: PNG or WebP files"]
```

mddog is one Rust binary, `mdd`, for macOS and Linux. It has no library users. It runs in three modes:

| Mode | Started by | What it does |
|---|---|---|
| Viewer | `mdd FILE`, `mdd -`, or piped input | Draws the page in the terminal and handles keys and the mouse |
| File list | `mdd`, `mdd FOLDER`, or Ctrl-p in the viewer | Lists the Markdown files under a folder to pick from |
| Headless | `mdd --headless COLS [--webp] FILE` | Writes image files and prints JSON, for the Claude Code plugin |

The **theme** decides how the page looks. It is one file of tokens: colors, fonts, and a few sizes. A theme can recolor GitHub's look, or switch to a second look modeled on Obsidian. A theme can't change the layout rules. Those are code.

## Code map

### `src/main.rs`

The command line. It parses the options, checks the file before it takes the terminal (so a typo or a binary file gets a plain error), loads the config, refuses tmux, zellij, and Warp by their environment variables before it sends the terminal anything, and starts one of the three modes. For the viewer, standard input is copied to a private temp file first, so the viewer always reads a real file. Headless mode refuses standard input.

### `src/config.rs`, `src/theme.rs`

`config` reads the optional `config.toml` once, at startup, into a `OnceLock`. Other code reads it with `config::get()` and `config::theme()`. An unknown key or a bad value stops the run with an error that names the file, line, and key. `config.example.toml` lists every key. The theme list (T) changes the theme while mddog runs: `config::show` puts a theme on screen over the config, and `config::save` writes the `theme` value back to `config.toml` and makes it the config's.

`theme` reads theme files. `TOKENS` is the one list of tokens and their kinds. The built-in themes are `assets/themes/*.css`, embedded in the binary and listed in `BUILTIN`. A theme file uses CSS syntax but is not a style sheet: only `:root` rules with known tokens are allowed. A theme can name a base theme and take the tokens it leaves out from that base. `theme::Theme` holds the resolved tokens and the `Palette` for the parts drawn in terminal cells.

**Architecture invariant:** each theme file is read once per run and kept for the rest of it, so a theme keeps one address. The viewer tells themes apart by address (`std::ptr::eq`), and the file list's previews key on it.

**Architecture invariant:** `theme.rs` holds the only SGR color formatters. Every part drawn in cells gets its colors through `Palette`.

**Architecture invariant:** there is no appearance observer. `config::theme()` reads the light or dark setting each time a page is built, and a change shows on the next reload. On macOS it reads the system setting. On Linux it reads the answer the terminal gave at startup, and the terminal pushes later changes through mode 2031. The background color (OSC 11) decides, since Ghostty answers the scheme query with the desktop's scheme; a mode 2031 report only triggers a new OSC 11 query. Headless mode can't ask the terminal, so on Linux it asks the desktop portal once at startup.

### `src/render/`

The renderer. It turns Markdown into pixels and knows nothing about terminals. Its parts, in pipeline order:

- `doc`: Markdown to a `Doc`, with pulldown-cmark plus a small reader for the raw HTML that Markdown files use. A `Doc` is a tree of `Block`s, and each top-level block keeps its source lines. `doc/dollars.rs` finds math with GitHub's rules before the parser runs.
- `highlight`: code to colored token runs, the way GitHub does it. tree-sitter covers a few languages, and TextMate grammars from starry-night (in `assets/textmate/`, deflated by `build.rs`) cover the rest.
- `math`: a TeX parser and box layout, with glyphs from the STIX fonts.
- `mermaid`: Mermaid source to a bitmap, through mermaid-rs-renderer and resvg. No browser and no script.
- `images`, `media`, `remote`: local and remote images. `media` decides which local files a page may show. `remote` fetches web images.
- `svg`: SVG parsing and drawing, for SVG images, diagrams, and icons. `icons`: the alert and button icons.
- `text`: fonts, text measuring, and text drawing. It is the platform seam for text, chosen by `cfg`: `text/coretext.rs` uses CoreText on macOS, and `text/cosmic.rs` uses cosmic-text on Linux. On Linux, generic families such as `system-ui` and `monospace` resolve through `fc-match`, with a fixed list of fonts when fontconfig is missing.
- `layout` and `page`: a `Doc` to a `Page`. A `Page` is a display list of rects, text runs, images, and paths, plus what the viewer asks about: links, anchors, headings, buttons, wide blocks, and the page's text.
- `raster`: draws the part of the display list that touches a tile, then the overlay on top. It draws through the `Canvas` seam, chosen by `cfg`: `canvas_cg.rs` uses CoreGraphics on macOS, and `canvas_skia.rs` uses tiny-skia on Linux.
- `theme`: flattens a `theme::Theme` into the fields that layout reads, and picks the `Look` (GitHub or Obsidian) where the two looks draw a part differently.
- `engine`: `Engine`, the one object that the viewer and headless mode call.

`Engine` is the API boundary of the renderer. It owns the fonts, the theme, the `Doc`, the images, the `Page`, and the overlay. Its main calls are `load`, `resize`, `rescale`, `snapshot`, and then one group each for search, selection, hover and presses, wide blocks, diagrams, and late images. The file also defines the types those calls take and return, such as `Layout`, `Press`, and `Select`.

**Architecture invariant:** `render` does not use `viewer`, `term`, `headless`, or `config`. It reads `theme` for the token types, `tmp` for one file-owner check, and `trace` for timing marks. The caller passes in everything else: the source, the theme, the width, the scale, the font smoothing flag, and the callouts rule. The one exception is the home and cache folders: `remote` and `media` read `HOME`, and `remote` reads `MDDOG_CACHE_DIR` and `XDG_CACHE_HOME`.

**Architecture invariant:** the engine keeps no bitmap of the whole page. It draws each tile from the display list when asked. So pixel memory does not grow with the page, and a tile that was freed can be drawn again.

**Architecture invariant:** the overlay (search marks, selection, hover, wide-block offsets, an expanded diagram) is separate from the display list. A change to it never needs a new layout. Only the tiles it touches are drawn again.

**Architecture invariant:** math, Mermaid diagrams, and local images are ready when layout runs, so they are in the first paint. Only remote images arrive late. They draw as their alt text first, and the page is laid out again when they arrive.

**Architecture invariant:** `render::remote` is the only code that uses the network. An engine set offline (the file list's preview) reads only its cache.

### `src/viewer/`

The interactive viewer and the file list.

- `view.rs`: `Viewer`, the viewer itself. It runs the event loop, owns the `Engine`, writes to the terminal, and carries out the actions that `App` returns. `Viewer::show` is the one path that draws the page.
- `app.rs`: `App`, the state machine that turns a key or mouse event into `Action`s (scroll, glide, search, contents, history).
- `chrome.rs` and `keys.rs`: the status row, the contents sidebar, and the `?` screen, drawn as terminal text from `App`. `keys.rs` is the one list of keys that the `?` screen shows, and a test in `app.rs` fails when a key it lists does nothing.
- `cache.rs`: `Cache`, the tiles the terminal holds: their ids, the bytes they use, which to free, and the bands used under herdr and Rex.
- `position.rs`: keeps the view on the same content when the page is laid out again.
- `loading.rs`: the skeleton that shows while a slow file loads.
- `picker.rs`: the file list, and how it shares the screen with the preview. `picker::listed` is the one check of `file_list.extensions`, used by the list, "Did you mean", the not-Markdown notice, and link following.
- `themes.rs`: the theme list that T shows in the sidebar.
- `preview.rs`: `Preview`, the file list's preview: the top screen of the selected file, drawn on a worker thread with its own `Engine` and kept off the network.
- `link.rs` and `clipboard.rs`: following links, and copying text.

**Architecture invariant:** `app.rs` holds no terminal or engine state. Besides the viewer's own pure modules (`labels`, `themes`), it depends only on `term::Key`. So it is tested row by row against `docs/behavior.md` without a terminal. `chrome.rs` is pure in the same way.

**Architecture invariant:** `cache.rs` does not write to the terminal. `Viewer` passes it a function that draws a tile, so its tests use a fake.

**Architecture invariant:** the viewer never passes a local path to the system `open` command. A cloned repo could link to an app or a `.command` file. A link to a local Markdown file opens inside mddog, and only when the file is inside the repo (else the home folder, else the folder of the file shown).

### `src/term/`

The terminal layer. `term/mod.rs` sets raw mode on `/dev/tty`, probes the terminal at startup (`Tty::probe`), and parses keys and mouse reports. `kitty.rs` sends, places, and frees Kitty images, and holds the tile size limits and the storage limit. `screen.rs` reads the display's backing scale.

**Architecture invariant:** the viewer refuses to start when the terminal can't do what it needs: one that answers the probe without Kitty graphics, or a place it can't show images or scroll them: tmux, zellij, and Warp. Those three are found from the environment by `term::refused_by_env`, which `main.rs` calls before the probe, because tmux takes the probe's graphics query as the pane's title. It never falls back to drawing the page as text.

### `src/headless.rs`

`--headless` mode, for hosts that own the screen and the input. It renders the whole page at a given width, writes the tiles to a private temp folder, prints one JSON line that lists them, and does that again on every file change. With `--webp` it writes small WebP tiles for Claude Code's desktop and mobile apps.

**Architecture invariant:** headless mode does not use `viewer`. It calls the `Engine` directly, as the viewer does, and takes the tile limits from `term::kitty`.

### `src/watch.rs`, `src/editor.rs`, `src/error.rs`, `src/tmp.rs`, `src/sweep.rs`, `src/trace.rs`

Small helpers. `watch` watches a file's folder, so a save done as write-then-rename still counts. `editor` runs `$EDITOR`. `error` turns an I/O error into a short message. `tmp` names and writes private temp files, and reads a temp name's pid back. `sweep` removes temp files that crashed or killed runs left behind, and prunes the remote image cache. `trace` writes the debug log that `MDDOG_TRACE` turns on.

### `src/lib.rs`

The crate root. Every module is `pub` but `#[doc(hidden)]`. The library exists only so that `main.rs` and the tests in `tests/` can reach the modules.

### `plugins/claude-code/`

A Claude Code plugin that shows a page in a pane next to the chat. It is TypeScript (`hooks/register.tsx`), not part of the Rust build. A plugin can't host a program that needs a terminal, so it runs `mdd --headless` and draws the tiles it lists. Claude Code owns the pane, its scrolling, and its input, so mddog's own keys don't work there.

**API boundary:** the `--headless` options and the JSON line it prints, typed in `hooks/headless.ts`. The plugin and `mdd` ship apart, through the Claude Code marketplace and `install.sh`. Each line carries a `protocol` number. When it differs from the plugin's `PROTOCOL`, the pane says which side to update and doesn't draw the page. Bump it on both sides with any change to the options or the line.

### Other folders

- `assets/`: built-in themes, TextMate grammars, and README images.
- `fixtures/`: Markdown files for manual and automated checks, such as `fixtures/gfm.md` and `fixtures/math/`.
- `tests/`: integration tests. See `CONTRIBUTING.md`.
- `reference/`: development tools in Node and Python: GitHub page capture, grammar export, end-to-end checks, and the benchmark.
- `website/`: the docs site, built with Fumadocs.
- `docs/`: the behavior spec and notes on testing in real terminals.
- `website/content/docs/`: the user docs, published at mddog.pages.dev.

## Cross-cutting concerns

### Threads

The main thread owns the `Engine`, the `App` state, the tile cache, and the terminal writes. One other thread writes to the terminal: the loading skeleton (`viewer/loading.rs`) draws its frames on its own thread until the page is ready. Both write under the `term::WRITE` lock, so their bytes never mix. The main thread blocks on one channel of `viewer::Event`s. The input thread, the signal thread, the file watcher, and the file list's walker send to that channel.

`Engine` is neither `Send` nor `Sync`: its state sits in `RefCell`s and holds `Rc` data. So only the main thread calls it. Inside `Engine::load`, highlighting, Mermaid builds, and local image decoding run on worker threads, and `load` waits for them before layout. Remote fetches run on their own pool and hand their results back through a channel that the main thread polls. Remote images are decoded on the main thread when they arrive. The highlight and Mermaid memos are thread-local to the thread that calls the engine.

The file list's preview (`viewer/preview.rs`) is the one exception to "only the main thread". Its worker thread creates its own `Engine` and keeps it, so no engine ever crosses threads. It sends finished tiles in shared memory back to the main thread, which sends them to the terminal.

Headless mode has the same rule: the engine and every snapshot stay on its main thread. Besides the engine's own workers, it has a signal thread, the file watcher, and threads that encode the image files.

The viewer has no helper process, no socket, and no daemon. The only programs it runs are `$EDITOR` (through `sh -c`), `pbcopy` on macOS or `wl-copy`, `xclip` or `xsel` on Linux to copy, and the config's `open_command` (`open` on macOS or `xdg-open` on Linux by default) for web and mail links.

### Coordinates and scale

There are two units. The engine's API takes and returns CSS pixels. The `Page` and the tiles are in device pixels: CSS pixels times the scale. The scale is the display's backing scale on macOS, or the terminal's cell height over 19 on Linux (rounded to 0.25, from 1 to 3), times the terminal's font zoom. The page is laid out at the scale the viewer draws it at, so nothing is resampled. Text takes the same CSS width at every scale. Table borders snap to whole device pixels, as they do in Chrome. Emoji also follow Chrome: on macOS an emoji is as wide as its picture at the device size, divided by the scale (16 CSS px at 2x and 20 at 1x for 16 px text), and on Linux it is its font's width rounded to a whole CSS pixel at every scale (20 CSS px for 16 px text). On Linux, a character followed by U+FE0F or U+20E3, such as ❤️ or 1️⃣, takes the emoji font, as in Chrome. `Engine::snapshot` can also draw a tile at another scale, which headless mode uses for its small WebP copies.

### Keeping the place

Each top-level block keeps the source lines it came from. Before the page is laid out again, mddog records the source line at the top of the view. After, it scrolls back to the same spot. When the source changed, as on a reload or a return from the editor, it first maps that line through a diff of the old and new source. A resize, a font zoom, and a late image keep the same source and skip the diff.

### Caches

All caches are in memory and last for one process, except the remote image cache. Highlights are kept by their source text, so a reload with unchanged code doesn't highlight it again. Mermaid diagrams are kept until the next load, so a resize or a `<details>` toggle doesn't build them again. Fonts and laid-out text lines are kept by `render::text::Fonts`. Remote images are kept in memory and on disk under `~/Library/Caches/mddog/remote/` on macOS, or `$XDG_CACHE_HOME/mddog/remote/` (default `~/.cache/mddog/remote/`) on Linux. `MDDOG_CACHE_DIR` moves that root.

### Files on disk

mddog has no database and never writes the file it shows. It writes:

- a copy of standard input, removed on exit.
- tile files for the terminal, which the terminal deletes.
- headless tiles in a private folder, removed on exit.
- the error output of each custom `open_command` run, to a private file deleted as soon as it is opened, so the command can still write to it after mddog quits. The default opener's error output goes nowhere, since `xdg-open` can run the browser and its log.
- the remote image cache.

Each temp name holds the pid of the run that made it. A SIGKILL skips cleanup, and Claude Code kills the headless process on every pane resize. So `sweep` runs once after the first paint. It removes only the temp files whose pid is gone, and the remote cache files that are too old.

### Security

A Markdown file is untrusted input.

- The engine doesn't run scripts. The raw HTML reader drops the contents of `<object>`, `<embed>`, and `<template>`. It shows `<script>`, `<style>`, `<iframe>`, and the other tags from GitHub's tag filter as plain text. Tags it knows, such as `<img>`, `<details>`, and `<kbd>`, change the page. A raw `<img>` follows the same image rules as a Markdown image. Tags it doesn't know don't change anything, and their text stays.
- Local images load only from inside one root folder: the nearest folder above the file that has `.git`, else the home folder when the file is under it, else the file's own folder. `render::media::confined_media` compares canonical paths, so a symlink or `..` can't leave it.
- Remote images never reach a private or reserved address. `remote::Checked` resolves each host, and each redirect, and refuses the host unless every address is public. The fetch then connects only to the addresses it checked, so a DNS answer that changes in between can't get through.
- Text drawn in terminal cells has its control characters replaced, so a file can't inject escape sequences.
- Text goes to the clipboard only on a copy action, or on a selection when `copy_on_select` is on.

### Observability

`MDDOG_TRACE=FILE` logs keys, renders, scrolling, placed images, and timing marks. Other `MDDOG_*` variables dump frames and tiles. `CONTRIBUTING.md` lists them all.

### Testing

Most behavior is tested below the terminal. `App` and the chrome are tested without a terminal, and the engine is tested through the same calls the viewer makes. Then a smaller set of tests runs `mdd` in a pseudo-terminal and checks what it writes. Math and TextMate output are pinned by golden files. See `CONTRIBUTING.md`.

## Glossary

| Term | Meaning |
|---|---|
| Doc | The block tree that `render::doc::parse` makes from the Markdown. |
| Page | What `render::layout` makes from a Doc: a display list in device pixels, plus links, anchors, buttons, wide blocks, and the page's text. |
| Overlay | What the raster draws on top of the display list: search marks, the selection, hover, wide-block offsets, and an expanded diagram. |
| Tile | A pane-wide strip of the page, sent to the terminal as one image. Headless mode writes it as one PNG file, or with `--webp` as two WebP files: a sharp copy and a small one. |
| Wide block | A code block or table wider than the page. It scrolls sideways on its own. The page itself never does. |
| Band | Under herdr and Rex, an image of a tile's changed rows, placed over the tile so that the whole tile need not be sent again. |
| Glide | An eased scroll to a jump target, drawn at pixel offsets. |
| Storage limit | The most tile bytes mddog lets the terminal hold. Past it, the tiles farthest from the view are freed. Tiles in view are never freed, so a view larger than the limit goes over it. mddog starts at the quota of the terminals it knows: herdr, Rex, and the rest. A terminal with a smaller quota deletes images without telling mddog. mddog learns of it when a placement answers `ENOENT`. It then asks about every other tile it holds, sends the dropped ones again, and lowers the limit to what the terminal still holds. |
| herdr | A terminal multiplexer built on libghostty, with a small image quota. mddog detects it and sends less image data. |
