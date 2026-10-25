# Architecture

mddog previews a Markdown or MDX file in the terminal as a browser shows it. It writes HTML, dresses it in a style (GitHub's look is the default and, for now, the only one), lays it out in an off-screen WebKit view, cuts the page into image tiles, and draws them with the Kitty graphics protocol. Keys and the mouse scroll, search, and follow links. The file reloads when it changes. The page and the terminal parts take their colors from a theme, one CSS file. By default it is GitHub's light or dark theme, following the macOS appearance. With no file, or with a folder, it lists the Markdown files there to pick from. A Claude Code plugin shows the preview inside a session through a headless mode.

It is a macOS-only Rust binary with no library users. The behavior contract for keys and events is in [docs/behavior.md](docs/behavior.md). Measurements, spike findings, and rejected approaches are in [docs/archive/](docs/archive/design-notes.md).

## Project structure

```
src/
  lib.rs             crate root: exports render, the other modules are hidden and serve the binary
  main.rs            CLI: options, file and stdin checks, --spare, --start-at-login, --no-start-at-login
  config.rs          optional config file: theme, live reload, smooth scroll, Mermaid engine
  theme.rs           themes: built-in list, mode and --mddog-* chrome colors read from a theme's CSS
  headless.rs        --headless: render to PNG files, for the Claude Code plugin
  watch.rs           file watcher
  editor.rs          runs $EDITOR
  trace.rs           test and debug logging
  assets.rs          extracts embedded runtime files to the cache
  sweep.rs           removes temp files, sockets, and runtime folders that dead runs left behind
  render/            Markdown and MDX to HTML, the crate's only documented API
    markdown.rs      comrak to the HTML GitHub's Markdown API returns, then the style's markup
    mdx.rs           MDX to Markdown
    sanitize.rs, dom.rs  raw HTML allowlist, as a DOM pass
    highlight.rs     tree-sitter highlighting
    textmate.rs      TextMate highlighting (irosashi)
    emoji.rs, octicons.rs  :shortcodes: and inline icons
  viewer/
    preview.rs       the viewer: first paint, event loop, mouse, links, selection, terminal writes
    cache.rs         tile ids, resident bytes, eviction, herdr bands, placements
    app.rs           key-to-action state machine (scroll, search, contents, history)
    chrome.rs        status row, contents sidebar, and the ? screen, drawn from App
    keys.rs          the key list shown on the ? screen
    loading.rs       page skeleton (heading, text, code block, in the page's theme and padding), drawn when the page isn't ready after 100 ms and held at least 100 ms. Warm starts are usually done before then
    picker.rs        the file list: walk, fuzzy filter, drawing, and keys
    position.rs      keeps the view on the same content across re-renders
    remote.rs        resolves remote image hosts and refuses private ones
    sizes.rs         each obsidian-engine diagram's drawn size, so the next skeleton opens at its height
    script.rs        calls into mddog.js and parses its replies (layout, search, press)
    page.rs          reads the source, has the style wrap the article in its page, CSS, and scripts
  style/
    mod.rs           Style: markup, page, CSS, and Mermaid engine; DEFAULT is GITHUB
    github.rs        GitHub's look: repo-view markup, page wrapper, CSS layers, Mermaid viewer
  term/
    mod.rs           /dev/tty raw mode, terminal probe, key and mouse parser
    kitty.rs         Kitty graphics: transmit, place, free
  engine/
    mod.rs           Engine: a local WKWebView or a warm spare, 10 s call limit
    webview.rs       AppKit window and WKWebView
    spare.rs         warm spare process, socket protocol, launchd job
assets/
  github.css         the github style's CSS, built from github-markdown-css and Primer CSS (both MIT), without its colors
  themes/            built-in themes, one CSS file each: github-light.css and github-dark.css (Primer's colors), obsidian-light.css and obsidian-dark.css (Obsidian's reading view)
  mddog.js           in-page code: search, code scroll bars, mouse, selection
  runtime/           MathJax 3.2.0, GitHub's Mermaid viewer, mermaid 11.13.0, and Pele 1.6.0, embedded in the binary
  textmate/          128 grammars from starry-night, scope-to-class map
fixtures/            gfm.md, component.mdx and its GitHub twin, diagram.svg
tests/               integration tests (help over a pty, search, watch, headless, textmate golden)
reference/           Node and Python dev tools: CSS build, grammar export, captures, e2e
docs/                behavior spec and archive
plugins/
  claude-code/       Claude Code plugin: a pane that draws --headless output
```

## System overview

```
config.toml + style.css (read once) -> theme, Mermaid engine, live reload

file or stdin
  -> mdx.rs (MDX only) -> markdown.rs + sanitize + highlight -> article HTML
  -> page.rs + style (default github: page, light and dark CSS, Mermaid engine) + mddog.js, MathJax
  -> engine: WKWebView in this process, or in a warm spare over a Unix socket
  -> layout and tile snapshots (RGBA)
  -> kitty.rs -> terminal (shared memory, temp file, or PNG chunks)

tty input, signals, file watcher -> channel -> main thread -> app.rs -> actions

--headless: same pipeline -> PNG files in a temp folder -> one JSON line per render -> host draws them
```

Threads in the viewer:

- **Main thread.** Owns the tty writes, the engine, the `App` state, and the tiles. It blocks on one channel and spins the WebKit run loop while it waits for a call.
- **Render worker** (64 MiB stack). Reads the file and produces the HTML.
- **Input thread.** Parses keys and mouse reports.
- **Signal thread.** SIGWINCH becomes a resize. SIGTERM, SIGINT, and SIGHUP become a clean quit.
- **Watcher.** notify's own thread.
- **Walker.** Lists files for the file list while it is open.
- **Skeleton.** Held at least 100 ms once drawn. It stops in the same synchronized update as the first tiles.

## Subsystems

Each subsystem has its own section, so a new note goes under the one it belongs to.

### CLI

`main.rs` checks the file before it takes the terminal, so a typo, a binary, or a URL gets a plain error. With no file and a terminal on standard input, or with a folder, it opens the file list (see File list). With `-`, or with no file and piped input, it copies standard input to a private temp file. `--help` is short and points to the `?` screen for keys. It works even when the config file is broken, because `main` loads the config only after it parses the options. `--headless COLS FILE` runs headless mode instead of the viewer (see Headless mode). Like the viewer, it stops on a broken config.

### Config

`src/config.rs` owns the optional config file: `$XDG_CONFIG_HOME/mddog/config.toml`, else `~/.config/mddog/config.toml`, plus an optional `style.css` in the same folder. The keys are `theme` (one theme, or `light:THEME,dark:THEME`; a theme is a built-in name or a `.css` path), `live_reload`, `smooth_scroll` (default true, and false makes every jump instant), `glide_ms_per_page` (default 200, how long a glide over one screen takes, see Event loop and keys), and `mermaid.engine` (pele or obsidian; unset, the theme picks). README.md documents them. `main` loads the file once, before the viewer takes the terminal, into a `OnceLock`. Everything else calls `config::get()`, which gives the defaults when nothing was loaded (tests, the library). Unknown keys and bad values stop the run with one line that names the file, the line, and the dotted key. `parse` also reads the theme files, so a missing file or a bad theme stops the run the same way. The spare loads the config too, for its warm-up page, and ignores errors there.

### Theme

A theme (`src/theme.rs`) is one CSS file. The built-ins are `assets/themes/*.css`, embedded in the binary and listed in `BUILTIN`, one line each, so a new built-in is that line and its file. `theme::parse` (built-ins) and `theme::file` (theme files) read the declarations of the theme's top-level `:root` rules (not those inside `@media`, nor nested rules) for `--mddog-base`, `color-scheme` (light or dark), and the eight `--mddog-*` hex colors into a `Palette`. `masks` blanks comments to spaces and string contents to `x` at the same byte offsets, so a quoted `{`, `}`, `;` or `/*` can't move a rule's edges, and the scan slices values out of the comment-free text. It takes literal hex only, so it doesn't need a CSS parser.

A theme with `--mddog-base` keeps its base, a built-in or a file relative to its own folder, in `Theme::base`. `theme::file` loads the chain and stops on a file it has already seen (by canonical path). The theme takes its mode from the base when it sets none, and a different `color-scheme` is an error. A color the theme leaves out comes from its base's resolved palette, else from `github-light` or `github-dark`, by mode. `theme::relative_urls` rewrites each relative `url()` and `@import` in a theme file to `file://<that file's folder>/<target>`, because the page's `<base>` is the Markdown file's folder. It leaves any `scheme:`, `#` and `/` target alone. The config applies it to `style.css` too, against the config folder. Built-ins have no relative URLs. The two Obsidian files differ only in their `:root` colors. The rules after that block are the same in both, and a test checks that.

`config::theme()` gives the theme to draw with: the one theme, or for a `light:...,dark:...` pair the one for the macOS `AppleInterfaceStyle` setting, read each time a page is built. `Engine::load_file(path, theme.dark)` sets the view's NSAppearance (Aqua or DarkAqua) before each load, and the style's `prefers-color-scheme` rules, `<picture>` sources, and (in the github style) `gh-dark-mode-only` links do the rest. The page keeps `data-color-mode="auto"` so those rules follow the appearance. The viewer keeps the loaded page's theme, and `resize_in_place` reloads when `config::theme()` is another one. There is no observer: a macOS appearance change shows on the next reload, resize, wrap toggle, or return from the editor, or when help opens: the help draws in the page's colors, so `?` reloads the page first when `config::theme()` changed. The spare builds its warm-up page in the config's theme, and its `load` request carries the mode.

In the github style, CSS goes into the page's `<head>` in this order, so each layer can override the one before it:

1. `assets/github.css`: github-markdown-css's Markdown rules and the Primer CSS rules around them (buttons, tooltips, utilities), without their colors, then mddog's `reference/github-overrides.css`. `reference/build-css.mjs` builds it (see Testing).
2. `EXTRA_CSS` in `style/github.rs`: mddog's own rules, written against GitHub's variables, then the Mermaid engine's rules (`SKELETON_CSS` or `MERMAID_CSS`) on pages with diagrams.
3. The theme's layers, one `<style>` each, from `theme_layers` (in the order `Theme::layers` gives): the GitHub theme of the theme's mode (unless the chain starts there), each base from the bottom up, then the theme. So a partial theme keeps its base's or GitHub's colors for what it leaves out, as the `--mddog-*` fallback does for the chrome. A GitHub theme is one layer, so its page is unchanged. The GitHub themes set Primer's color variables on `:root`. No rule in the base CSS sets those variables, so moving them from GitHub's mode selectors to `:root` kept every render byte for byte.
4. The inline background rules, in the theme's `--mddog-bg`, which Pele also draws on.
5. The user's `style.css`. A `:root` rule there beats the theme's by order, with no `!important`.

The page background is the theme's `--mddog-bg`.

The terminal chrome draws with the theme's `Palette`. `Palette::reset()` is `\x1b[0m` plus the background, and every drawer uses it in place of a plain `\x1b[0m`, so the cells between spans keep the theme's background. The picker, `Chrome` (status bar and TOC), `chrome::help`, the screen clears in `Preview::show`, and `Loader` take their colors from it, and `theme.rs` holds the only SGR color formatters. The picker, which has no page, uses `config::theme()` at each draw.

### HTML pipeline

1. `.mdx` goes through `mdx.rs`: markdown-rs parses it, imports, exports, expressions, and comments are dropped, and each component (a JSX element whose name is not lowercase HTML) becomes a `jsx` code block of its source. Lowercase elements such as `<details>` stay as HTML and go through the sanitizer. Nothing is executed. On a parse error the file renders as plain Markdown.
2. `markdown.rs` parses with comrak (GFM on) and writes the HTML GitHub's Markdown API returns: alerts, footnotes, task lists, and math and Mermaid blocks. `sanitize.rs` cleans raw HTML with GitHub's allowlist. It notes whether the page has math or Mermaid, then calls the style's `dress`. The github style adds what github.com's repo view has after its scripts ran: heading anchors, frontmatter tables, and the Mermaid container. Last, `markdown.rs` links raw URLs and keeps source positions on top-level blocks only. cmark drops a backslash before punctuation and reads `*` and `_` in math as emphasis, so math is escaped before parsing. cmark also turns `\$` into `$`, so an escaped dollar outside code is held as `HELD_DOLLAR` until the math pass. Local media is confined to a root folder (see Security).
3. Code is highlighted the way GitHub does it. tree-sitter covers JS, TS, TSX, Python, Rust, Go, Ruby, CSS, HTML, and PHP. irosashi, a Rust port of vscode-textmate, covers the rest with starry-night's grammars. Both map to GitHub's `pl-*` classes. `tests/textmate.golden` pins the output.
4. `page.rs` (`page::doc`, `page::html`) is the one path from file bytes to a page, shared by the viewer and headless mode. It hands the article to the style's `page`, which adds the style's page context, its CSS layers (see Theme), `mddog.js` and MathJax from `page::scripts`, and its Mermaid scripts. The runtime files are extracted once to `~/Library/Caches/mddog/runtime-<hash>/`. `build.rs` computes the hash. WebKit's fallback draws the footnote back-link `↩` from a text font where Chrome uses Apple Color Emoji, so `EXTRA_CSS` forces the emoji font.

### Styles

The goal is to show a Markdown file the way a browser or a Markdown app would. GitHub's look is the default style, not the goal. A `Style` in `src/style/mod.rs` holds what changes from one look to another:

- `dress`: adds the style's markup to the sanitized HTML.
- `page`: wraps the article in a whole page with the style's CSS and scripts.
- `mermaid`: the Mermaid engine when the config names none.

`style::DEFAULT` is `GITHUB`, and nothing picks another one yet. The shared HTML from step 2 still uses GitHub's API markup, and other parts depend on GitHub's class names: `mddog.js` (copy buttons, code scroll bars, Mermaid), the `pl-*` highlight classes, `highlight::copy_box`, and `EXTRA_CSS`. A second style either keeps those names or brings its own versions.

### Engine

A WKWebView in a borderless window parked off-screen, at the pane's CSS width and the display's scale. Each load sets its appearance to the theme. It is ordered in with occlusion detection off, because WebKit gives a hidden window almost no rendering updates. Every call has a 10 s limit. After one stall every later call fails at once, so `q` still restores the terminal. WebKit snaps a snapshot rect that starts or ends between CSS pixels, which shifts its rows by a device pixel and rasterizes text differently. With an odd cell height at scale 2, every other tile starts on a half CSS pixel, so `snapshot_into` takes the whole CSS pixels around such a rect and keeps the rows it was asked for.

WebKit fires the load event, then spends about 7 ms on the first full layout before it answers the next call. `measure` would force that layout anyway, so the time after load is not waste. A rendering update (a frame) costs about 2.6 ms most of the time, with outliers whatever the idle gap before it, from 30 ms to 1.6 s. So there is no idle penalty to work around.

### Warm spare

A cold first paint takes 200 to 215 ms on files without diagrams, most of it AppKit and WebKit startup, and about 260 ms on `fixtures/gfm.md`. A warm one takes 35 to 45 ms, and about 70 ms on `fixtures/gfm.md` (medians of 10 rounds, 2026-10-21). So after the first paint the viewer starts `mdd --spare`. The spare loads a warm page and waits on a Unix socket in the per-user temp dir. The socket name hashes the binary's path, size, and mtime, so a rebuilt binary never reaches an old spare. The viewer connects only to a socket its own user owns, and the spare answers only a peer with its own uid (`getpeereid`). It ignores a connection that never sends hello. The next viewer drives the spare's WebView with six requests (eval, load, resize, wait, snap, render). `load` carries the theme as its third element, and `render` is `["render", src, dir]` so the spare confines media to the same root. A late or missing reply to the first render ends the connection, so it can't answer the main thread's next request. The spare warms a file page, math, and Mermaid before a viewer asks, since the first of each costs more. One thread with the viewer's 64 MiB stack builds every highlighter (they are cached per thread) and does every render. Snapshots go straight into the shared memory the tile is sent in. A spare serves one viewer, then exits. An unused spare exits after 12 hours. It forks once more and the middle process exits, so launchd adopts it: Claude Code kills the plugin's child and all its descendants on each width change, and the next child would otherwise borrow a dying spare. The launchd job has no `KeepAlive`, so after the first spare the viewers start their own. `--start-at-login` adds a launchd job that starts one spare at login, and `--no-start-at-login` stops and removes it. `MDDOG_NO_SPARE` turns it off.

### Display

Tiles are pane-wide, a whole number of cell rows tall, at most 8 MiB and 10,000 px tall each. They go over shared memory, then a temp file under `$TMPDIR`, then PNG chunks (the only route over SSH, where PNG makes a tile about 30 times smaller for about 1 ms), picked by the startup probe. WebKit snapshots are premultiplied sRGB BGRA, so converting them to RGBA is a channel swap straight into a zeroed shm buffer, whose size rounds up to whole pages. The swap takes 1.1 ms per 8 MiB tile, against 2.0 ms for a Core Graphics draw. Any other pixel format falls back to the draw. Per 8 MiB tile, the WebKit snapshot takes 3 to 5 ms and NSImage to CGImage takes nothing. macOS limits shm names to 31 characters. Each visible tile gets one placement with a source rect. Scrolling moves the source rect inside a synchronized update. A tile that scrolls out loses its placement by placement id (`a=d,d=i,i=<id>,p=1`), because iTerm2 frees the image itself on a `d=i` without `p`, and the tile then comes back blank. Each tile is sent with one more cell row below it, a copy of the next tile's first row (the last tile has none), so a tile and its extra row stay under both limits. That row is for glides (see below). The resident set stays under 56 MiB when XTVERSION starts with `libghostty` or `TERM_PROGRAM` is `herdr` (herdr's 64 MiB quota), and under 300 MiB elsewhere (other terminals hold about 320 MB). Past that, the tiles farthest from the view are freed and re-sent on demand. Visible tiles are never freed, so a viewport larger than the storage limit on its own (a full-screen pane on a 5K display under herdr) goes over it, and herdr then drops the oldest images.

Under herdr (the same check as the storage limit), a selection or search change does not re-send the tiles it touches. herdr copies every image and its client sends it on to Ghostty as base64, so an 8 MiB tile costs herdr and Ghostty about 90 ms of CPU. Instead, the changed rows, snapped to whole cells and full pane width, go out as a band image placed over the tile at `z=1`.

A glide (see Event loop and keys) draws the view at any pixel offset, not only on cell rows. A placement can't start or end between cell rows exactly. Ghostty draws a placement `r * cell_h - Y` pixels tall from the cell top plus `Y`, and herdr 0.9.5 rewrites `c` and `r` from its own grid, `ceil((h + Y) / cell_h)`. So under herdr, a piece whose bottom falls between rows gets stretched. Instead, every placement starts and ends on a screen row:

- Screen row `r` shows the page from `scroll_px + r * cell_h`. `cache::rows` gives the rows each image draws.
- A tile draws the rows that start inside it. The last of those can cross into the next tile, and the tile's extra row covers that part.
- A band is sent with one extra row above and below (`Band::src`), and draws every row it touches.
- So each placement's source height is `rows * cell_h`, with no `Y=` and no rescale, and `kitty::place` is the same as for whole-row scrolling. At a whole-row offset the placements are the same as before glides.
- A change in a tile's extra row resends that tile too, or under herdr gives it a band as well.

After a re-tile resizes the WebView, a snapshot taken before the page has its new size comes back blank. `retile` polls `innerHeight` until it shows the new size, which takes about 2 ms, and falls back to waiting for a frame if the poll passes 100 ms. A frame takes 15 to 25 ms here, because the rendering update after a resize to the full document height is expensive. The first snapshot then absorbs part of that work, so the net gain is 7 to 15 ms per render.

- `viewer/cache.rs` owns all of this: tile ids, the resident byte count, eviction, bands, and placements. `Preview` writes what it returns to the terminal and hands it a snapshot closure, so its tests use a fake. Every subtraction from the resident count uses bytes that `Tile::release` or a band added before.
- A tile keeps its bands in `Tile::bands`. A new change that touches a band replaces it with one band over both (`join_bands`). The tile's byte count includes its bands.
- Bands are placed with the tile on every frame, cropped to the view, so they move with it on scroll. `Tile::release` frees a tile and its bands together, so every path that frees a tile frees them too.
- A tile goes out whole again, which folds its bands away, when it would hold more than 4 bands or bands over half its rows, when it changes while out of view, 1 s after the last input, or when it is re-sent anyway (reload, resize, eviction, help).
- For a drag, `Preview::selection_js` counts only the rects that differ between the old and the new selection, so a long drag sends only the rows at its moving end. The status row and contents sidebar strip control characters, so a document cannot send escape sequences.

### First paint

The main thread probes the terminal (CSI 16 t, XTVERSION, DECRQM for modes 2026 and 1016, `a=q`, then DA1, up to 1 s) while the worker renders and WebKit starts. The first snapshot covers only the first viewport. The other tiles follow in idle time. Math gets up to 1 s. Pele draws its diagrams before the page finishes loading, so they are in the first paint. Obsidian-engine diagrams are not waited for. A skeleton in the theme's colors fills each diagram's box (200 px, or the height the diagram drew at last time, see Mermaid), `mddog.startMermaid()` loads mermaid right after the first paint, and the late re-send shows the diagrams once the last one is drawn. Remote images are not waited for. Before they load, `remote::load` resolves each image host (up to 1 s, cached per run) and refuses hosts that resolve to a private or reserved address, or have a local name such as `.local` or no dot. Anything that finishes late is measured again with the position kept.

While a file loads, `Loader` (`loading.rs`) draws a page-shaped skeleton in terminal cells: a heading bar, text lines, and a code block, with a diagonal shimmer that moves every 100 ms. It draws only when the page is not ready after 100 ms (`DELAY`), so a warm start usually finishes before it shows, and once drawn it stays at least 100 ms (`MIN_SHOWN`), so it never flashes. The margins are the page's 24 CSS px padding times the screen scale, over the cell size. The colors come from the theme (see Theme). `Loader::tune` takes the probe's answers, and a resize redraws the frame in full. `show()` calls `finish()` after the tile snapshots, so the minimum wait overlaps that work, and the clear and the first tiles go out in one synchronized update. The `term::WRITE` lock covers each whole write from `Tty::write` and the loader, so a frame can't split the probe query. The file list has no skeleton. Opening a file from the list, or going back to the file before, starts a new `Loader` before the file is parsed, with the same delay and minimum time. Its first frame clears the screen, so it replaces the list, and `show()` takes it down as at startup.

A warm first paint of `ARCHITECTURE.md` takes about 44 ms: render in the spare (2 ms), page build and write (0.3 ms), page load (20 ms), measure and settle (5 ms), then two 8 MiB snapshots (9 ms and 5 ms). Page load is the largest part, and it is all WebKit. A side thread reads the macOS appearance at startup, because the first `config::theme()` call for a theme pair sets up NSUserDefaults and costs 2.1 ms.

### Event loop and keys

`app.rs` is pure: it turns a key or mouse event into actions and holds no terminal or WebKit state, so it is unit tested row by row against [docs/behavior.md](docs/behavior.md). `chrome.rs` draws the status row, the sidebar, and the `?` screen from `App`, so they are tested without a terminal too. `preview.rs` carries out the actions. Resizes are debounced by 120 ms. A resize or contents toggle reuses the rendered HTML (`Preview::doc`) when the file bytes still match.

A resize lays the live page out again instead of loading it (`Preview::resize_in_place`). It resizes the WebView, waits until `innerWidth` shows the new width, measures, and scrolls to the same content anchor a reload uses. A page with diagrams also waits for one rendering update, because Pele refits its SVGs in a ResizeObserver callback that runs after the measure would. The resize falls back to a full reload when the result could differ from a fresh load:

- the file changed (with live reload on), or the macOS appearance changed under a `light:...,dark:...` theme pair
- a click changed the DOM (a copy, a `<details>` toggle, a button), text is selected, or a diagram is expanded
- the page has `<picture>` or `img[srcset]`, which can pick a new source by width
- GitHub's Mermaid viewer drew the diagrams, because it reads the page width once, when it renders
- the page did not reach its new size, or the frame did not come, within 100 ms. The old geometry is restored first, so the reload keeps the position.

This saves 13 to 21 ms per resize. The archive's "Resize in place" section has the numbers.

The jump keys glide: `d`/`u`, `f`/`b`, Space, PageUp/PageDown, `g`/`G`, Home/End, Backspace/Ctrl-o, digits, `J`/`K`, `]]`/`[[`. `App::handle` turns their `Action::Scroll` into `Action::Glide`. Every key in `App::instant` (`j`, `k`, `n`, `N`, Enter, the wheel) is left alone. `App.scroll` takes the target at once, so the status row and sidebar show where the view is going. Only the drawn offset moves.

- `app::Glide` eases out (cubic) from where the view was drawn to the target. It is time-based, so a late frame skips ahead and the glide still ends on time. It covers at most one screen, so `G` from the top glides only the last screen. A jump during a glide starts a new one from the drawn offset, and may cover one screen more than the old glide had left, so quick repeated presses never skip.
- `app::Pace` sets the time. A glide over one screen takes `glide_ms_per_page`, and a shorter one takes its share. A jump key within 100 ms (`REPEAT`) of the last one glides in 50 ms (`FAST_GLIDE`), like snacks.nvim's `animate_repeat`. Holding `f` then keeps the view 1.4 to 1.9 screens behind the target, with no frame moving more than one screen.
- `Preview` holds the running glide with the offset it last drew, `drawn` (the offset `show` last drew, where a new glide starts), `path` (the tiles a new glide passes over), and `last_jump`. `shown_px()` feeds `view()`, `grid()` and `view_css()`, so drawing and mouse hits use the drawn offset, while `scroll_px()` stays the target everywhere else.
- `start_glide` jumps instead when `smooth_scroll` is off, the terminal lacks mode 2026, or the path's tiles don't fit in the storage limit together (`Cache::fits`, bands included). The first `show()` sends the path's missing tiles inside its synchronized update and keeps them from eviction. Then it restarts the glide clock, so a slow send doesn't eat into the glide.
- While a glide runs, the event loop wakes every 8 ms (about 9 to 10 ms in practice) and `tick()` advances it, skipping frames that land on the same pixel. Prefetch pauses.
- `settle()`, `Action::Scroll`, `Action::Search`, a resize, an instant key, a mouse press, help, and the file list end a glide. A press ends it before it is handled, so the press and its release hit the same page point. `show()` uses `path` only while a glide runs, so a glide that ended before its first frame can't leave stale tile indices behind.
- A glide frame takes under 0.1 ms of work in `show()`, except the first frame of a long jump, which sends tiles (12 to 16 ms). The trace's `view` line starts with `px=` (the drawn offset) and `took=` (ms in `show()`).

When the probe finds mode 1016, the terminal reports the mouse in pixels, not cells. `Preview::spot` turns each report into its cell plus where it falls inside that cell, so the status row, the sidebar, and double clicks still work in cells, while links, word selection, and code scroll bars are hit at the exact point. Without 1016 the point is the cell's center.

The viewer pushes kitty keyboard flag 1 (`term::KEYS_ON`, `ESC[>1u`) after it enters the alternate screen, and pops it (`term::KEYS_OFF`) before it leaves, in `term::leave()` and `Preview::leave_screen`, since each screen keeps its own flag stack. Flag 1 is the only flag that makes the terminal send Cmd-c (`CSI 99;9u`). It also turns Ctrl-letters and Esc into CSI-u forms, so `term::csi_u` maps those back to the `Key` their legacy bytes give, and the rest of the viewer never sees the difference. The one new key is `Key::Copy`, which `App` turns into `Action::CopySelection` in every mode. There is no `CSI ? u` probe: a terminal that ignores the push keeps sending legacy bytes, which parse as before.

When the contents list overflows, the sidebar's last `app::TOC_BAR` (3) columns hold its scroll bar: a thin line on each side and a thin thumb between them. A list that fits keeps one separator column. `App::toc_thumb()` gives the thumb's top and length, and both `Chrome::sidebar` and the hit tests (`App::toc_bar_press`, `toc_bar_drag`) use it, so what is drawn and what is hit agree. `App::toc_grab` holds the thumb row the pointer grabbed. `preview.rs` only routes by column: a list-row press within 3 columns of the sidebar's edge goes to the bar (`App::toc_bar_press` takes the distance from the edge), and the wheel at `col <= img_col` scrolls the list. `Key::WheelUp` and `Key::WheelDown` carry their cell for this.

### File list

`picker.rs` holds the list. `Picker` keeps the items, the filter, and the selection, and `draw` paints the list over the whole screen as terminal text, the way the `?` screen does. `preview::browse(root)` starts with the list and no document. It draws the list right after raw mode, before WebKit starts and before the terminal probe, and waits up to 50 ms for the first batch. Ctrl-p (`Action::Browse`) opens the same list over a file and keeps the last filter.

- `scope` picks the root: the nearest folder with `.git` above the start, else the current folder. `mdd` starts from the current folder, and Ctrl-p from the open file's folder (the current folder for standard input). A folder argument is the root itself. Ctrl-p keeps one `Picker`. Each Ctrl-p calls `Picker::follow`: when the open file is outside the current root, the root becomes `scope` of the file's folder, and the filter is cleared. A file under the root keeps it, so a list of a plain folder, or of a folder that holds several repos, stays put.
- A walker thread uses `ignore::WalkBuilder` with `require_git(false)`, so it skips hidden and gitignored files with or without git. It keeps `.md`, `.markdown`, and `.mdx` files and stops at 10,000 (`CAP`), in walk order. It sends a batch every 30 ms with `term::Event::Listed`, which wakes the event loop. An `AtomicBool` stops the walk when the list closes or restarts.
- `title` reads the first ATX heading from each file's first 4 KiB. Setext headings and headings after long front matter don't give a title.
- `nucleo-matcher` scores `path + " " + title` as one string. `Picker` splits the match positions back into the path and the title for highlighting. The filter takes fzf's syntax (`!`, `^`, `'`, `$`).
- `open`, `switch`, and `back_file` change files inside one `Preview`. `open` pushes a `Visit`: the file, the stdin flag, the name, the whole `App`, a position anchor, and the source. The history holds 50 (`HISTORY`). `switch` resets the document state, restarts the watcher, and calls `render_doc`, which maps `restore_at` through a line diff the way a reload does (see Position keeping). The new `App` keeps `wrap` and `toc_visible`.
- Backspace and Ctrl-o first go back through jumps in the current file. With none left, `App` emits `Action::BackFile`, and `back_file` returns to the file before at its old position. A file that can't be read is dropped from the history, so older files stay reachable.
- When the list is up, `show()` frees the tiles and draws it, and `draw_chrome` and `prefetch_target` skip. Opening a file renders it on the main thread, as a reload does.

### Position keeping

Every top-level block carries comrak's `data-sourcepos`. Before a re-render, mddog records the source line at the top of the view and the offset into its block. Afterwards it maps that line through a line diff of the two sources (`similar`) and scrolls back to the same spot.

### Search

`mddog.js` finds every occurrence across inline elements and soft wraps, with smart case (any uppercase letter makes it case-sensitive), and highlights them with the CSS Custom Highlight API, so the layout does not change. Only the tiles whose highlights changed are snapshotted and sent again. Under herdr only the changed rows go out, as a band over the tile (see Display). Each match's client rects are measured once, and measured again only when the current match sits in a sideways-scrollable block that `reveal` may have moved. With 408 matches on `ARCHITECTURE.md` the search takes 11.5 ms, and 9 ms of that is WebKit's `Range.getClientRects`. A search snapshot still waits for a frame: CSS highlights paint only during a rendering update, and `afterScreenUpdates` alone does not include them.

### Mermaid

`page::Opts.engine` picks the engine, from `config::mermaid(theme)`: `mermaid.engine` if set, else the theme's `--mddog-mermaid` (inherited from its base), else pele. Every built-in theme names its engine: pele for the GitHub themes, obsidian for the Obsidian ones. Both engines are always embedded under `assets/runtime/`. The style's `dress` (`style/github.rs`) emits an engine-neutral `mddog-mermaid` element inside GitHub's diagram box, with its copy and expand buttons.

- **pele** (default): [Pele](https://pele.run) 1.6.0 (MIT) draws each diagram to SVG in the page, with no iframe, before the page finishes loading. Its own layout makes diagrams differ from GitHub's, and it draws 3 to 4 times faster than mermaid. `MERMAID_CSS` in `style/github.rs` maps its colors to the theme's Primer variables. It shows zoom buttons only on a diagram it shrank to fit. The expand button shows its SVG over the view.
- **obsidian**: what Obsidian 1.14's reading view does. Obsidian doesn't theme its diagrams. It runs plain mermaid 11.13.0 with the `default` theme in both modes and inlines the SVG, with no frame, zoom panel, or copy button. In dark mode it adds `filter: invert(100%) hue-rotate(180deg) saturate(1.25)` to the SVG. mddog ships the same mermaid from npm (MIT, `assets/runtime/mermaid/`, not Obsidian's copy) and calls it with Obsidian's settings: `securityLevel` strict, `themeVariables.fontFamily` the page's text font, `useMaxWidth` false for flowchart, sequence, git, and ER diagrams and true for gantt, journey, class, state, and pie.
  - `renderObsidian` in `mddog.js` puts a skeleton (`SKELETON_CSS` in `style/github.rs`) over each 200 px box at DOMContentLoaded. `mddog.startMermaid()` (`script::START_MERMAID`) then adds a hidden `srcdoc` frame at the page width that loads mermaid, so its 3 MB stays out of the first paint, and renders each diagram there, with 4 s for each. A call that comes before that setup is kept and runs when setup ends. Every page load with Mermaid sends it before it waits on `MERMAID_DONE`: `Preview::render_doc` after `show()`, on the first paint and every reload, while `MERMAID_DONE` is still pending, headless mode and the spare's warm-up right away, and the `load_page` test helper. Pele ignores the call.
  - A diagram can name images (an image shape, an `<img>` in a label), and Obsidian's in-page render would fetch them past mddog's media checks. So the frame's CSP allows only `data:` images and fonts, and each SVG shows as an `<img>` with a `data:` URL, which never loads anything either. A diagram whose image is blocked shows mermaid's loading error. Diagram text can't be selected or searched. If mermaid fails to load, each box shows an error.
  - All the images and error flashes replace their skeletons at once, the frame goes, and `MERMAID_DONE` turns true once the images are decoded, so a snapshot after it has them. So the live layout changes once, not once per diagram.
  - 45 s after `startMermaid`, whatever hasn't drawn shows an error, so the page settles before the viewer's 60 s `LATE_LIMIT`. Without that, many slow diagrams could change the layout after the last re-send, and the tiles went stale.
  - The skeleton opens at the diagram's drawn height when mddog has seen that diagram before. On reveal, `mddog.js` stores each diagram's viewBox in `state.sizes`, keyed by an FNV-1a hash of the source (`key`). A diagram drawn at its own size is stored with width 0, so its guess isn't scaled down. When the late check settles, `tick` merges those into `Sizes` (`viewer/sizes.rs`). After `READY`, `render_doc` passes them to `mddog.guess`, which sets each known box to that height at the current width. The sizes live in memory for reloads and in a cache file for reopens. The reveal clears the guessed height before it applies the real one, so the settled page doesn't depend on the guess. Only the viewer guesses. Headless mode doesn't.
  - `OBSIDIAN_CSS` in `style/github.rs` left-aligns the diagram and hides GitHub's diagram buttons, and `OBSIDIAN_DARK_CSS` adds the filter when the theme is dark. A width change doesn't need a reload: a diagram at its own size keeps it, and one that fits the width scales with CSS.

The archive's "Mermaid with Pele" section has the comparison that made Pele the only engine for a while.

### Wide code blocks

mddog clips the page root and shows images, so it scrolls each `pre` itself. The page draws its own scroll bars (`mddog-hbar`), because WebKit paints none in a snapshot. Shift + wheel, `h`/`l`, and dragging a bar change `scrollLeft`, and only the affected tiles are sent again. `w` toggles wrapping for all code blocks.

### Mouse, links, and selection

Clicks map from cells to page CSS pixels. On release, mddog tries a link (`link()`), then a button (`press()`). Links open with `open`, or jump to a heading. Drag and double-click select text and paint the highlight. Releasing copies nothing unless `copy_on_select` is on. The viewer is read-only, so a click on a task checkbox does what a click on plain text does.

- Copy buttons. `highlight::copy_box` wraps every code block in the repo view's box, with the copy button beside the `pre`. The github style shows it at all times. A click copies the button's `value`. Mermaid diagrams have their own copy button.
- Copying. Every copy goes through `Preview::copy_text`. It strips control characters (`pasteable`), writes OSC 52 to the tty so the copy reaches the clipboard of the terminal the user sits at (also over SSH, and through herdr, which shows its own notice), then pipes the text to `pbcopy` on a thread. The OSC 52 write runs on the main thread between frames, so it never splits a tile write. It has no size cap: Ghostty under herdr drops one over about 220 KB, and `pbcopy` still covers the local clipboard. `Preview::copy_selection` copies the page's DOM selection, for Cmd-c and for a release when `copy_on_select` is on.
- Hover. Mode 1003 reports moves with no button held (`Click::Move`). `Preview::hover` hit-tests a move only when it reaches a new spot (`hover_at`: the spot plus the drawn offset), through `mddogMouse.hover(x, y)`, which returns the pointer shape and the boxes to redraw. Off-screen WebKit never gets a real pointer, so `:hover` never fires. Only code-block copy buttons (`.zeroclipboard-container clipboard-copy`) get the hover look: the page sets `mddog-hover` and GitHub's `tooltipped` classes itself, and the tooltip text is the `aria-label`, so `Copied!` wins while it shows. The event loop keeps only the last of a run of queued moves. Any other event marks `hover_at` stale, and the loop hit-tests it again once no drag is held, so a scroll, help, a relayout or a new page update hover under a still pointer. Mermaid buttons get only the hand. `hover(x, y, slack)` also gives the hand over code scroll bars, through `mddogCode.bar`, the hit area a press grabs.
- Pointer shape. `set_pointer` sends OSC 22 only when the shape changes: `xterm` over text, the hand over links and controls, `left_ptr` elsewhere. The hand is `Probe::hand()`: `hand2` for iTerm2, which knows only X11 names, and the W3C `pointer` for the rest, since Ghostty has no `hand2`. kitty takes both. `MOUSE_OFF` ends with an empty OSC 22, so every way out of the screen resets it. herdr and WezTerm drop OSC 22. herdr parses it and never forwards it, and its maintainers closed the request to add that (herdrdev/herdr#5279), so mddog doesn't work around it.
- Expand. A diagram's expand button makes `press()` return `expand`. `mddogMouse.expand` then copies the drawn SVG (from the viewer's frame, or Pele's) into an `mddog-expanded` overlay over the visible rows, scaled to fit. `App::expanded` makes the next key or click call `collapse()`. A new page load clears it.

### Task checkboxes

The viewer is read-only. A task box is drawn and does nothing when clicked.

- The box is drawn with CSS (`appearance: none` in `EXTRA_CSS`), in the gray Chrome draws for github.com's disabled boxes, sampled in each theme. WebKit's native box is nearly black in a window that is never key.
- Raw HTML can't write its own source positions: every `data-sourcepos` in it becomes `data-sourc&#101;pos`, which reads the same as text but is an attribute name the sanitizer drops. Scroll keeping trusts the positions, so a forged one could not move the view.

### Headless mode

`mdd --headless COLS FILE` (`src/headless.rs`) is for hosts that own the screen and input. It renders the whole document `COLS` cells wide, through the warm spare when one is running, with the config's theme and Mermaid engine. It cuts the page into PNG tiles of at most 100 rows (a Claude Code image can't exceed 255 rows) and 8 MiB decoded, writes them to a private temp folder, and prints one JSON line `{columns, keep, tiles: [{file, rows}]}`. `keep` is how many tiles the host may mount at once: the image storage limit over 8 MiB, so 7 when `TERM_PROGRAM` is `herdr` and 37 elsewhere. It renders again on every file change, even with `live_reload = false`, because live refresh is why the plugin exists.

- Tiles are written with fast PNG compression. A PNG takes about as long to encode as a snapshot (5 ms for 8 MiB), so up to 4 threads encode while the main thread takes the next snapshot. This made a 19-tile page 35% faster.
- With no spare, the first Markdown render runs on its own thread while WebKit starts.
- The cell size comes from `/dev/tty`, which a plugin's child still has as its controlling terminal. The fallback is 8 x 16 times the backing scale.
- The folder keeps the current and the previous round of tiles, because the host may still be drawing the previous one. A round counts only once all its tiles are written, so a failed render deletes only its own files.
- A WebKit stall exits with code 1, so the host can say the preview stopped. A signal removes the folder and exits at once.
- It shares the viewer's page path (`page::doc`, `page::html`, `script`) and image limits (`kitty::TILE_BYTES`, `kitty::storage_limit`), and does not import `preview.rs`.

### Editor and watching

`e` leaves the alternate screen, runs `$EDITOR` (or `vi`) through `sh -c`, then re-renders with the position kept. The shell exits 127 when it can't find the program, which is how the status row reports a bad `$EDITOR`. The watcher watches the parent directory, so a save done as write-then-rename still counts. It reloads only when the content changed. With `live_reload = false` the watcher does not start. A preview of standard input has no watcher, and `e` shows `Can't edit standard input`.

On Ctrl-z the viewer also leaves the alternate screen. It sends SIGTSTP to its own process group, then draws the page again when the shell resumes it. When mdd leads its own session, as in a terminal that runs it with no shell, its group is orphaned. The kernel drops SIGTSTP sent to an orphaned group, so the viewer comes back at once.

## Claude Code plugin

The plugin is not part of the Rust build. It calls the `mdd` binary like any other caller. README.md has the install steps.

`plugins/claude-code/` is a hooks-module mod (`hooks/register.tsx`). A mod can't host a TTY program: `$.process.spawn` pipes stdout and stderr and closes stdin. It can draw kitty images through the `Image` element. So mddog renders with `--headless`, and Claude Code owns display and input.

- `/mddog <file>`, or the model's `open` tool (`mcp__mddog__open`), runs the same `show()`. A blank or missing path is refused. Claude Code seats a pane the person asked for (a typed command or prompt) at any width. A pane a plugin opens on its own, as from a model tool call, is seated only at 144 or more columns, or 110 or more for a pane id the person opened before and has not closed by hand. Until then `$.ui.open` returns `{isPlaced: false, reason}` and the pane waits undrawn, with no `ui.render`. When the pane is not drawn, `show()` stops the old file's child, and the tool returns the reason and asks the model to have the user run `/mddog`.
- The pane draws the tiles as a column of `Image`s keyed `tile-0`, `tile-1`, and so on. Only the `keep` tiles nearest the scroll position are mounted, and each other tile is a blank `Box` of its height. Mounting a long page whole went past herdr's 64 MiB quota, and herdr then dropped the oldest images, the ones in view, so the pane showed only the hint. Stable keys let an edit swap sources in place, so the pane keeps its scroll position. A width change stops the child and starts one at the new width.
- When the tiles are taller than the pane, a dim key hint sits above them. The pane API has no pinned footer, so the hint scrolls away with the page.
- Navigation is Claude Code's own pane scroll. mddog's keys (search, links, contents, code scroll) are not forwarded, because the spawn API has no stdin channel.
- `session.end` stops the child and clears the pane, because `/clear` ends the session without unloading the module.
- `MDDOG_BIN` overrides which binary it runs.

## Data and files

No database. The binary reads `config.toml` and `style.css` from its config folder (see Config), and writes:

- `~/Library/Caches/mddog/runtime-<hash>/`: MathJax, GitHub's Mermaid viewer, mermaid, and Pele. It is unpacked into `runtime-<hash>.tmp<pid>` and renamed into place. Each page build that uses it sets its mtime, so the sweep can tell which folders are in use.
- `~/Library/Caches/mddog/diagram-sizes.json`: the last 512 obsidian-engine diagram sizes, oldest dropped first. It is written through `diagram-sizes.tmp<pid>` and a rename. Read and write errors are ignored, since a missing size only brings back the jump.
- Private temp files, each named with the pid and a per-run hex nonce: the page (`mddog-<pid>-<hex>.html`, kept until the viewer exits, since the Mermaid frames load `file:` scripts after the page loads and WebKit refuses them once the page file is gone), the spare's warm-up page (`-warm.html`), a stdin copy (`-stdin.md`), and graphics transfers (`tty-graphics-protocol-mddog-<pid>-<hex>-<tag>`, which the terminal deletes).
- The spare's socket in the per-user temp dir, `mddog-<uid>-<hash>.sock`, mode 0700 (owner only).
- `~/Library/LaunchAgents/dev.mddog.spare.plist`, only with `--start-at-login`.
- With `--headless`, a private temp folder `mddog-tiles-<pid>-<hex>` (mode 0700) of PNG tiles. It is removed on a normal exit and on SIGTERM, SIGINT, or SIGHUP.

A SIGKILL or a crash skips all of that cleanup, and Claude Code SIGKILLs the plugin's `mdd --headless` on every width change. So after the first paint (headless: after the first render), `sweep.rs` runs once on a side thread. It matches only the names above, only entries the user owns, and removes:

- temp files and folders whose pid is gone. Only `kill(pid, 0)` failing with ESRCH counts as gone. A pid that a new process reused keeps its entry until that process ends too, so the sweep never removes anything a live process might use.
- spare sockets more than 10 s old that refuse a connection. A spare binds a moment before it listens, so a newer socket may still be live.
- `.tmp<pid>` entries in the cache whose pid is gone.
- `runtime-<hash>` folders other than this build's whose mtime is over 7 days old. Another build that runs it again unpacks a new copy. The sweep renames a folder to `.tmp<its own pid>` before it deletes it, so a run never sees a half-deleted folder with `.complete` still in it.

With nothing to remove the sweep takes under 0.5 ms. Removing 220 leftovers took 22 ms, all after the first paint, which stayed at 211 ms (median of 30 headless runs with and without the sweep, 2026-10-22). `MDDOG_TRACE` logs `sweep started` and `swept`.

## External integrations

- **WebKit** (system framework, through objc2) for layout, JS, MathJax, and Mermaid.
- **macOS appearance**: the `AppleInterfaceStyle` user default, for a `light:...,dark:...` theme pair.
- **The terminal**, through the Kitty graphics protocol.
- **Claude Code**, as host for the plugin.
- **launchd** for the login spare. **`open`** for links. **`pbcopy`** and OSC 52 for every copy. **`$EDITOR`** for edits.

Opening a file fetches only what the document names: remote images, video posters, and video metadata. A link reaches the network only when you click it. Mermaid diagrams can't load images (see Mermaid).

## Security

- Raw HTML goes through GitHub's allowlist, and cmark-gfm's tag filter turns `<script>` and similar tags into text.
- MDX is never executed.
- Local images and videos load only from inside a root folder: the nearest folder above the file that has `.git`, else `$HOME` when the file is under it, else the file's own folder (`/tmp`, `/Volumes`). So `../sibling/img.png` works for notes outside git. `/x` means `<root>/x`. `markdown::render_in` compares canonical paths, so a symlink or `..` can't leave the root, and it drops any `src`, `poster`, or `srcset` entry that would.
- The obsidian engine runs mermaid with `securityLevel` strict (labels go through its DOMPurify pass, and `click` handlers and links are off) in a hidden frame whose CSP allows only `data:` images and fonts, and shows each diagram as an `<img>`. So a diagram can't load a local file or reach a host. Pele draws in the page, which has no CSP, so `renderMermaid` mounts it with `images: false` and it leaves out every image URL a diagram names.
- Text the terminal draws has control characters replaced, so a file cannot inject escape sequences.
- The spare socket is owner-only (mode 0700) and checked for owner before use.
- mddog never writes the previewed file. Selecting text writes the clipboard only with `copy_on_select` on. Otherwise only `c`, Cmd-c and the copy buttons do.

## Development and testing

- `./check.sh` is the gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- `python3 reference/keymap-check.py` checks that every test named in docs/behavior.md ran and passed.
- `reference/e2e/` drives mddog in a herdr pane inside Ghostty for editor, external change, on-screen, search, and tile checks. The on-screen and search checks capture the Ghostty window.
- `tests/help.rs` runs mdd in a pty as its own session leader, so Ctrl-z can't stop it there. `ctrl_z_suspends_and_resumes` passes `job: true` to `launch`, which forks a small job-control parent the way a shell does: mdd runs in its own foreground group, and the parent passes its stops and exit status up to the test.
- `tests/headless.rs` runs `--headless` end to end. `claude plugin test plugins/claude-code` runs the Claude Code plugin's tests.
- The search harness (`tests/search.rs`) keeps each page file until the next load. WebKit fails `file://` loads once the file is gone, and Mermaid's viewer loads its chunks after the page has loaded.
- Every wait in `tests/search.rs` goes through `wait`, `loaded`, or `next_frame`, which allow 30 s and panic on timeout. A check that runs on a page that is not ready fails later and points at the wrong thing. Pele's ResizeObserver resets the zoom on its first callback, so press its buttons only after `next_frame`.
- The github style's reference is github.com's repo file view of `fixtures/gfm.md`, frozen in `reference/baseline/` (dark and light). A capture holds github.com's own page and CSS, so `reference/baseline/` is gitignored, and each clone captures its own. To capture it, push `fixtures/gfm.md` as the README of a temporary public repo, run `reference/capture.mjs --scheme dark|light --url <its README blob URL>`, copy the `.frozen.html` and `.geometry.json` files from `reference/renders/` into `reference/baseline/`, and delete the repo. `component.twin.md` has no repo page, so its baseline still comes from the secret gist. `style/github.rs`'s `CHAIN` copies the repo view's wrappers that github.com's CSS selects on. These captures test the default style. Another style would bring its own check.
- `reference/capture.mjs` steps the viewport by the miss instead of bisecting, because the repo view's sidebar appears near 1012 px, so article width does not grow steadily with the viewport.
- `reference/bench/` is the interactive viewer benchmark. `viewer.py` runs the viewer in a headless herdr pane, drives it with keys, and times first paint, scroll, search, resize, and reload from the trace. `table.py` turns its runs into a table, and `tiles.py` compares two builds' tile dumps. A headless herdr server does not resize the pty when the layout changes, so the harness sets the size with `stty -f`. After each reload the file watcher reports one extra event that changes nothing, and the harness skips it. Alternate old and new builds, and report medians of 10 or more rounds. Don't export `XDG_CONFIG_HOME` into the bench: herdr keeps its socket under it, so every call misses the test session. mdd in the pane runs with `HOME` set to `VP_HOME`, so put a test config in `$VP_HOME/.config/mddog/config.toml`. The bench exits on the first failed herdr call and names a suspect `XDG_CONFIG_HOME` or `HERDR_CONFIG_PATH`.
- macOS limits a Unix socket path to 104 bytes, so `tests/headless.rs` keeps its `TMPDIR` short.
- `reference/compare.mjs REF.frozen.html PAGE.html dark|light` lays out a frozen capture and mddog's page (the `page.html` that `--headless` writes) at the same content width, and prints every block whose position or size differs and every probed element whose computed style differs.
- `reference/build-css.mjs` rebuilds `assets/github.css` and the Primer block of both GitHub themes from MIT npm packages: github-markdown-css, `@primer/css`, and `@primer/primitives`, plus mddog's `reference/github-overrides.css`. It prunes the rules to the ones the pages mdd builds for the fixtures, and its kitchen sink, use, so run it after `cargo build --release`. It keeps the mddog block at the top of each theme file. Check the result against the baselines with `compare.mjs`. `reference/export-grammars.mjs` rebuilds `assets/textmate/`. `UPDATE_GOLDEN=1 cargo test` rewrites `tests/textmate.golden`.

Environment variables:

| Variable | Effect |
|---|---|
| `MDDOG_NO_SPARE` | No warm spare |
| `MDDOG_TRACE=FILE` | Log keys, renders, scroll (with each drawn pixel offset and frame time), placed images, and cell geometry |
| `MDDOG_DUMP_VIEW=FILE` | With TRACE, write a PNG of the viewport after each frame |
| `MDDOG_DUMP_TILES=DIR` | Write each tile as it is sent |
| `MDDOG_DEBUG` | Print timing marks to stderr |
| `TERM_PROGRAM=herdr` | Use the 56 MiB image storage limit, and send selection and search changes as bands |
| `EDITOR` | The editor for `e` |
| `XDG_CONFIG_HOME` | Where to look for `mddog/config.toml` |
| `MDDOG_BIN` | Read by the Claude Code plugin: the binary to run |
| `MDDOG_PANE` | Read by `reference/e2e/`: the herdr pane the checks drive |

## Glossary

- **Tile.** A pane-wide strip of the page sent to the terminal as one image. In headless mode, a PNG file of at most 100 rows and 8 MiB decoded.
- **Band.** Under herdr, a pane-wide image of a tile's changed rows, placed over the tile at `z=1` until the tile is sent whole again.
- **Round.** One render's set of tiles in headless mode.
- **Resident set.** The tile bytes the terminal holds at one time.
- **Storage limit.** The most tile bytes mddog lets the terminal hold: 56 MiB under herdr, 300 MiB elsewhere. The kitty spec calls this a storage quota, and Ghostty calls it `image-storage-limit`.
- **Spare.** A waiting `mdd --spare` process with WebKit already started.
- **herdr.** A terminal multiplexer built on libghostty, with a 64 MiB image quota per screen. Its XTVERSION reply is `libghostty`.
- **leaf.** The terminal Markdown viewer whose keys mddog's behavior spec started from.
