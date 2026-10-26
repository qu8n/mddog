# Design notes (archive)

The project was called mdmax until 2026-10-21, when it was renamed mddog (binary `mdd`). These notes use the new names. Flags and scripts that were removed later are named as they were, without the binary.

This started as the old `DESIGN.md` at commit `58fc3d1`. It keeps the measurements, spike findings, decisions, and rejected approaches. It is a record, not a description of the code today: [ARCHITECTURE.md](../../ARCHITECTURE.md) describes the current system. To see how a section changed over time, run `git log --follow -p ARCHITECTURE.md`, which reaches back through the rename.

Some files named below were deleted after they served their purpose. Get them back from git:

| File | Deleted in | Restore with |
|---|---|---|
| `spikes/kprobe.py` and the other spikes | `af6d7d8` | `git show af6d7d8^:spikes/kprobe.py` |
| `reference/score.py`, `reference/structure.py`, `reference/spec.py`, `reference/corpus.py`, `reference/structure/`, `reference/spec/`, `reference/results/`, the PNGs in `reference/renders/` | `36012a0` | `git show 36012a0^:reference/score.py` |
| `reference/fetch-readmes.sh`, `reference/probes/math.md`, `reference/probes/sanitize.md` | `f18b8ca` | `git show f18b8ca^:reference/fetch-readmes.sh` |

`reference/corpus/` was never committed. `reference/fetch-readmes.sh` rebuilt its README part.

---

`mdd <file>` previews a `.md` or `.mdx` file in the terminal. It starts from GitHub's dark-mode render and departs from it where that makes the terminal preview better. It renders HTML to pixels and shows them with the Kitty graphics protocol.

This document records the feasibility gates (step 0), the references (step 1), decisions from the first design, and the acceptance results. Numbers come from this machine: MacBook Pro M4 Pro, macOS 27.0.1, display backing scale 2, Ghostty 1.3.2-main+22391ed64, herdr 0.9.5.

## Step 0: feasibility gates

### Display path: herdr pane inside Ghostty

Method: a throwaway probe (`spikes/kprobe.py`, Python) ran in a new herdr pane, sent queries and test images to its own tty, and logged every reply. I captured the Ghostty window with `screencapture -l` after each phase. Two research passes read the herdr v0.9.5 source and the Ghostty source at commit 22391ed64 for the details below.

| Check | Result through herdr | Notes |
|---|---|---|
| Kitty query `a=q` | Answered `OK` in 0.1 ms | herdr answers it itself. Each pane runs its own libghostty-vt. |
| Direct `t=d` (f=32 and f=100) | Works | Chunked `m=1` works. 16 MiB cap per APC sequence. |
| File `t=f` | Works | |
| Temp file `t=t` | Works only under `$TMPDIR` | `/tmp/...` fails with `EINVAL: temporary file not in temp dir` because `/tmp` resolves to `/private/tmp`. The name must contain `tty-graphics-protocol`. The file is deleted after reading. |
| Shared memory `t=s` | Works | herdr unlinks the object after reading. macOS caps shm names at 31 characters. |
| Placement with source rect `x,y,w,h` | Works | Sub-cell offsets work. Re-sending `a=p` with the same `i` and `p` and a new `y` moves the visible window in place. herdr forwards only the small `a=p` to Ghostty, and Ghostty reuses the GPU texture. |
| Unicode placeholders `U=1` | Work | Not used: herdr turns them into one real placement per row, Ghostty ignores the source rect on virtual placements, and they cost the most to scroll. |
| OSC 11 (background) | Answered in 0.1 ms: `rgb:0a0a/0a0a/0f0f` | Matches `background = 0a0a0f` in the Ghostty config. herdr caches Ghostty's real colors. Replies end with ST even when the query ends with BEL. |
| CSI 16 t (cell size) | Answered: `6;38;19` | Device pixels: 19 x 38 per cell. CSI 14 t and TIOCGWINSZ also report device pixels (68 cols x 19 = 1292 px). |
| Other replies | DA1 `?62;22c`, XTVERSION `libghostty`, DECRQM 2026 supported, CSI ? 996 n `997;2` | |

Limits that matter, and where they come from:

- 10000 px per image side, in both Ghostty (`graphics_image.zig:19`) and herdr's libghostty-vt.
- herdr caps decoded image storage at 64 MiB per screen (`crates/ghostty-vt/src/lib.rs:205`). The alternate screen has its own 64 MiB. An 86 MB test image failed with `ENOMEM`. herdr counts decoded pixels, so PNG compression does not help.
- herdr skips any image whose base64 form would exceed its 30 MiB frame budget: about 23.5 MB of RGBA. Such images never show.
- herdr's fast route to Ghostty (a file transfer) needs f=32 and at most 16 MiB. Anything else goes inline as base64.
- herdr scales a placement that has no explicit `c` and `r` up to whole cells. mddog always passes `c` and `r` so that pixels map 1:1.
- Ghostty deletes visible placements and unplaced images on `CSI 2 J`, so mddog never clears the screen after it sends images.
- herdr deletes a pane's images from Ghostty when you switch tabs and re-sends them from its own copy when you come back. mddog does not need to do anything.

Workaround for herdr's quota: the document goes to the terminal as horizontal tiles, f=32, each at most 8 MiB and a whole number of cell rows tall. mddog keeps a resident set of at most 56 MiB under herdr (300 MB under plain Ghostty or kitty, which allow 320 MB). A document that fits is sent once. A larger document keeps a window of tiles around the viewport, prefetches the next tile in the scroll direction, and re-sends a tile only after it was evicted. The 64 MiB quota makes this unavoidable: at 1292 px wide, 56 MiB holds about 5700 CSS px of document.

Color: Ghostty draws image pixels in Display P3, the same space it uses for text colors. A window capture converted from its display profile to Display P3 gives back the sent values within 1 unit per channel (sent `230,25,75`, recovered `230,26,75`). The on-screen check compares in Display P3 for that reason.

### Engine

Three throwaway prototypes rendered the same input page at 898 CSS px wide and 2x scale: WKWebView offscreen snapshot (Rust, objc2), Blitz 0.3.0-beta.4 (Rust, vello_cpu), and headless Chrome 155 (its `--screenshot` mode). The input page is GitHub's own rendered DOM for each fixture, frozen from the gist page with GitHub's CSS and with the Mermaid iframe replaced by a screenshot of itself. So the scores measure the engine, not my Markdown pipeline. For gfm.md the frozen page shows the real relative image, as mddog will, instead of the broken image the gist shows (see step 1).

Fidelity is dssim against the github.com reference (lower is better). WKWebView and Blitz are scored against the WebKit reference and Chrome against the Chromium reference, each engine against its own family. Times are hyperfine, 20 runs, 2 warmups, exec to PNG on disk, on gfm.md. "Pruned" means GitHub's CSS cut to the rules that match the fixtures (1.56 MB to 89 KB). Fidelity is identical either way.

| Engine | p50 full CSS | p95 full CSS | p50 pruned | p95 pruned | dssim gfm.md | dssim MDX fixture | Text-range rects |
|---|---|---|---|---|---|---|---|
| WKWebView | 492 ms | 500 ms | 422 ms | 467 ms | 0.00209 | 0.00816 | Yes: `Range.getClientRects()` through `evaluateJavaScript` (the prototype found all 8 matches of "fixture") |
| Blitz | 282 ms | 291 ms | 198 ms | 214 ms | 0.331 | 0.578 | No API. Parley exposes glyph positions, so it would need custom code. No JS, so Mermaid and math would need native renderers. |
| headless Chrome | 912 ms | 1003 ms | 859 ms | 903 ms | 0.0737 | 0.0191 | Yes through CDP `Runtime.evaluate`, but the CLI screenshot mode cannot return them |

Other findings:

- WKWebView and Playwright's WebKit agree almost exactly. On the frozen gfm.md page with the broken image left in, WKWebView scored 0.00021, and every block except Mermaid and the task list was pixel-identical.
- Blitz lays tables out wrongly (the frontmatter table comes out as one row) and is 330 px short on gfm.md.
- Chrome needs 0.8 to 1.1 s to start, three times the first-paint budget, and its build differs from Playwright's Chromium.
- WKWebView's 422 ms includes a full-page snapshot (69 ms) and a PNG encode (87 ms). mddog needs neither for first paint: it snapshots only the first viewport and sends raw pixels.

Decision: WKWebView, through `objc2-web-kit`. It is the only engine that meets the fidelity bar, it is a system framework (nothing to download or bundle), it runs Mermaid and MathJax as JS, and it gives text geometry through the DOM. /ponytail agreed: it is the native platform feature, and Blitz would need native Mermaid, math, and text-geometry code to get anywhere near.

### Fidelity thresholds

The threshold is 1.5 times the best prototype's dssim on that fixture, rounded up to two significant figures. The 0.5 margin allows for Mermaid, which mddog renders live where the prototype embedded a screenshot. These thresholds were fixed while matching GitHub was the goal. That goal is retired (see "Mermaid with Pele" below), so the thresholds and the scores under Results are a record, not a gate.

| Fixture | Best prototype (WKWebView) | Threshold (max dssim) |
|---|---|---|
| `fixtures/gfm.md` | 0.00209 | 0.0032 |
| `fixtures/component.mdx` (scored against `component.twin.md`) | 0.00816 | 0.013 |

The same threshold applies to each block region when I hunt for differences.

## Step 1: references

### Fixtures and the gist

- `fixtures/gfm.md` covers every GFM feature mddog supports: tables, task lists, footnotes, alerts, strikethrough, autolinks, highlighted code in six languages, relative and remote images, `$` and `$$` math, Mermaid, and raw HTML that GitHub's sanitizer must clean.
- `fixtures/component.mdx` is a blog-style post with frontmatter, imports, an export, a Mermaid diagram, a `<Callout>` with Markdown children, a self-closing `<KeySequence>`, and an MDX comment.
- The secret gist <https://gist.github.com/qu8n/d63c249218e272f2b8d87b414a34d2e8> holds only these synthetic files. Gists render Mermaid and math, so no public repo was needed.

Two gist quirks shaped the fixtures:

1. Gists rewrite relative image URLs to `#file-...` anchors on the gist page, so the reference shows broken images. `fixtures/diagram.svg` is sized 125 x 22, the exact box of a broken image with that alt text, so the layout matches and the difference stays inside the image box.
2. Gists render `.mdx` as plain Markdown, so GitHub shows the import lines as text and strips the component tags. There is no github.com render of mddog's MDX behavior. `fixtures/component.twin.md` is the GitHub-renderable twin: the same post with imports, the export, and the comment removed, and each component written as the HTML that mddog emits for it. The MDX fixture is scored against the twin's render.

### Reference renders

`reference/capture.mjs` opens the gist in Playwright with `colorScheme: 'dark'`, DPR 2, and a viewport that makes `.markdown-body` exactly 898 CSS px wide (viewport 1280). It waits for math, Mermaid, and images, then injects CSS that paints every ancestor of the article `#0a0a0f`, and screenshots `.markdown-body`. It captures WebKit (the engine mddog uses) and Chromium (to score the Chrome prototype).

One deviation from "inject only a background": the script also adds `margin-top` to the article so that its top lands on a whole CSS pixel. On the gist page the article starts at y = 1765.78, and that fraction changes how WebKit rounds every line below it. Without the nudge, two identical DOMs scored 0.06 apart. With it, they match pixel for pixel.

Outputs in `reference/renders/`: `<fixture>.webkit.w898.png` (the reference), `.geometry.json` (top-level block boxes, used for per-region scores), `.dom.html` (GitHub's final DOM after its scripts ran), and `.iframe0.html` (the Mermaid viewer document).

Scoring: `reference/score.py` pads both images to the same size with the background color (a height mismatch counts as a difference), runs dssim with a heat map and odiff with antialiasing ignored, and scores each top-level block on its own.

### Structure reference

`reference/structure/*.api.html` hold `gh api markdown -f mode=gfm -F text=@<file>` output for `gfm.md`, `component.mdx`, and `component.twin.md`. (`-f text=@file` sends the literal string `@file`. `-F` reads the file.) The API output differs from the gist page in three known ways, all of which mddog follows the page on: Mermaid comes back as highlighted code, math stays as TeX inside `<math-renderer>`, and frontmatter becomes an `<hr>` plus a setext heading.

### Parser conformance

`reference/spec/spec.txt` is cmark-gfm's `test/spec.txt` (GFM 0.29, 672 examples). `reference/spec/extensions.txt` adds cmark-gfm's 30 extension tests.

### Corpus

- 8,803 `.md` files under `~/dev`, found with `fd -e md -H --no-ignore` excluding `node_modules`, `.git`, `vendor`, `vendored`, `third_party`, `third-party`, `target`, `.venv`, `venv`, `site-packages`, `Pods`, `bower_components`, `dist`, `build`, `.next`, and `.cache`. Sizes run from 0 bytes (3 empty files) to 349 KB. The median is 4,620 bytes: `firsthand/fh-mono/ui/ui-api/src/refdata/data/meta-prompts/templates/layout_curator_template.md`, the file for the first-paint requirement.
- The 17 `.mdx` files in `~/dev/qu8n/src`.
- READMEs of 20 popular repos, fetched by `reference/fetch-readmes.sh` into `reference/corpus/readmes/` (not committed): react, next.js, vscode, rust, go, deno, neovim, ripgrep, bat, fzf, ohmyzsh, tailwindcss, awesome, system-design-primer, public-apis, ollama, uv, mermaid, tensorflow, TypeScript.

No corpus file is ever sent to GitHub.

### Behavior spec (from leaf)

Moved to [`docs/behavior.md`](../behavior.md), where it is kept up to date.

## Decisions from the first design

The first design's description of the code is gone. ARCHITECTURE.md describes the code today. These are the choices from it whose reasons are not in the code:

- **Two highlighters.** A probe of 46 languages through the markdown API showed that GitHub uses tree-sitter for JS, JSX, TS, TSX, Python, Rust, Go, Ruby, CSS, HTML, C#, PHP, Swift, Elixir, and Nix (the output has `pl-kos` and `pl-s1`), and TextMate grammars for the rest. GitHub nests one span per scope, and starry-night's own theme trick cannot do that (a `pl-c` inside a `pl-c` for a shell `#!`).
- **irosashi over giallo.** giallo needed a patched copy to expose scope stacks. irosashi gave byte-identical HTML on 27,589 fenced blocks in 53 languages from the corpus, in 5.2 s against giallo's 9.0 s.
- **The window is ordered in, with occlusion detection off.** A window that is never shown gets rendering updates about once a second and no animation frames, and the CSS highlights that search uses paint only after a rendering update.
- **The terminal probe waits up to 1 s.** It ends at the DA1 answer, so a local terminal costs a few ms. With a 100 ms wait, a terminal over SSH with no Kitty graphics that answered at 150 ms got images instead of the error.
- **Frees go out before new tiles**, in the same synchronized update, so the terminal never holds more than the storage limit, even for a moment.
- **A warm spare, not a faster start.** A fresh process can't paint much sooner than 200 ms. NSApplication takes about 40 ms, the WKWebView about 20 ms, and the web content process about 100 ms from the first load to a loaded page. Reordering them didn't help, because the web process starts at the first load whatever comes before it. A waiting spare holds about 36 MB, plus a web content process of about 20 MB.
- **The login agent has no `KeepAlive` and no `ProcessType Background`.** A spare exits 0 after its viewer and the viewer starts the next one, so `KeepAlive` would make launchd fight it over the socket. macOS throttled a `Background` spare, and the run it served took 206 ms. Without it, the first run after a kill and a kickstart (standing in for a reboot) painted in 64 to 91 ms.
- **Code scroll bars are custom elements** (`mddog-hbar`), because the page's reset for GitHub's wrapper divs would size a `div` there to zero.

## Results

The fidelity, structure, parser conformance, and corpus numbers below were measured with command-line flags (`--render-png`, `--html`, `--api-html`, `--cmark-batch`, `--bench`) and scripts (`score.py`, `structure.py`, `spec.py`, `corpus.py`) that have since been removed from the repository, along with their committed outputs. They are in git history before the commit "refactor: remove reference-harness flags". The tables stay as the record of what was measured.

### Structure

The `--api-html` flag wrote stage A, the HTML that corresponds to `gh api markdown -f mode=gfm`, and `reference/structure.py` compared it after normalizing whitespace, attribute order, and GitHub's per-render ids.

| Fixture | Tokens | Differing lines | What differs |
|---|---|---|---|
| `gfm.md` | 1138 | 4 | The remote badge: GitHub's camo proxy URL against the original URL, which mddog loads directly |
| `component.mdx` against `component.twin.md` | 256 | 0 | Nothing |
| `reference/probes/math.md` (the math and emoji probe) | 52 | 4 | GitHub's custom `:octocat:` emoji, an image that mddog leaves as text. Inline math, including the `` $`...`$ `` form, matches |
| `reference/probes/sanitize.md` (the sanitizer probe) | 111 | 4 | The same camo URL. Raw images at the document root get GitHub's `<p>` wrapper, and `picture > img` stays unstyled, as on GitHub |

Intended differences between stage A and what mddog renders, all of them things GitHub's page does on top of the API output:

1. `mode=gfm` is GitHub's comment mode, so soft line breaks become `<br>`. Files on github.com use document mode, and so does mddog's render. `--api-html` switches hard breaks on so the comparison is like for like.
2. Frontmatter: the API renders it as an `<hr>` and a setext heading, the page as a two-column table. mddog renders the table.
3. Mermaid: the API returns highlighted code, the page a rendered diagram.
4. Math: the API keeps TeX inside `<math-renderer>`, the page converts it to MathML in the browser.
5. The page adds heading anchors, `dir="auto"`, `data-catalyst`, and a `style=""` on task checkboxes, and drops `notranslate` and the table `role`. mddog does the same.
6. The page autolinks URLs inside raw-HTML text that the API leaves alone (the escaped `<iframe>` line in `gfm.md`). mddog does the same, but ends the link at `"` or `<`. GitHub's link runs to the next space, so on that line it takes in `"></iframe>`.
7. Relative links and images: the gist page rewrites them to gist URLs. mddog keeps them relative to the file.
8. Remote images: GitHub serves them through camo. mddog loads the original URL from the user's machine, because the spec asks for remote images, except on loopback, private, link-local, and `.local` hosts, private IPv4 literals, and any IPv6 literal, which get the placeholder as camo refuses them. The check reads the host name, so a public name that resolves to a private address, or a redirect to one, still gets through. Closing that needs a proxy in mddog.

### Fidelity

`--render-png OUT --width 898 --bg '#0a0a0f' FILE`, scored by `reference/score.py` against the WebKit reference:

| Fixture | Threshold | mddog dssim | odiff | Result |
|---|---|---|---|---|
| `fixtures/gfm.md` | 0.0032 | 0.00195 | 0.12% | Pass |
| `fixtures/component.mdx` | 0.013 | 0.0 (pixel-identical) | 0% | Pass |

Blocks over the threshold, with their causes:

| Fixture | Block | dssim | Cause |
|---|---|---|---|
| `gfm.md` | "A relative image" paragraph | 0.140 | The gist shows a broken image there (it rewrites relative URLs). mddog shows the real `diagram.svg`. Same box, different pixels. |
| `gfm.md` | Centered `<div align="center">` | 0.075 | Same cause, the second relative image. |

Heat maps and per-block scores are in `reference/results/gfm.md/` and `reference/results/component.mdx/` (`heatmap.png-0.png` is the full-resolution dssim map, `-1` to `-4` the coarser scales, plus `odiff.png` and `score.json`).

### Parser conformance

`reference/spec.py` runs cmark-gfm's `test/spec.txt` and `test/extensions.txt` through mddog's cmark-gfm stage (comrak with mddog's options) and normalizes like cmark-gfm's `normalize.py`.

| File | Passed | Rate |
|---|---|---|
| `spec.txt` (GFM 0.29) | 648 / 672 | 96.4% |
| `extensions.txt` | 28 / 30 | 93.3% |

Every failure is listed with expected and actual HTML in `reference/spec/failures.md`. They fall into four groups:

1. Fenced code renders as `<pre lang="ruby">` (6 examples). That is GitHub's own output (`github_pre_lang`), not the spec's `class="language-ruby"`.
2. Raw `<script>`, `<style>`, and `<textarea>` blocks are escaped (5 examples). That is GFM's tag filter, which GitHub applies and the core spec examples do not.
3. The extended autolink extension links URLs inside core examples that assume it is off (6 examples, including 2 in `extensions.txt` where comrak's trailing-punctuation rule differs from cmark-gfm's).
4. Emphasis and entity edge cases where comrak follows CommonMark 0.31 and the GFM spec file is 0.29 (9 examples).

### Startup

Measured in a real herdr pane in Ghostty, hyperfine, 20 runs, 2 warmups, from exec to the terminal's answer to a DA1 query sent right after the first placement (`--bench`), on the corpus median file:

```
p50 = 214.9 ms   p95 = 222.1 ms   min = 192.4 ms   max = 226.1 ms
```

Measured with the screen unlocked and the pane on screen. Two earlier numbers were wrong in opposite ways. A p50 of 210 ms came from a race: when Markdown rendering finished before the warm-up page settled, mddog measured and painted the blank warm-up page, so a small file showed an empty preview (on the median file every time). With that fixed, runs taken while the screen was locked gave 258 ms, and the same build gave 340 ms with the screen unlocked, because WebKit then launches its GPU process on the first-paint path. WebKit draws the DOM in that process by default. mddog turns that off with the `UseGPUProcessForDOMRenderingEnabled` feature flag (a private method, guarded by `respondsToSelector`), so the page draws in the web content process. Snapshots are byte-identical either way. `--bench` also prints the measured block count, and the corpus run checks it.

Per-phase breakdown of one run (ms since main started):

| Phase | At | Takes |
|---|---|---|
| tty open, raw mode, alternate screen | 0.3 | 0.3 |
| NSApplication (about 45 ms), WKWebView (about 40 ms), parked window | 103.7 | 103.5 |
| terminal probe (OSC 11, CSI 16 t, XTVERSION, `a=q`, DA1) | 104.4 | 0.7 |
| read and parse (worker thread, overlapped with the WebView start) | 104.5 | 0.0 |
| page written | 104.7 | 0.2 |
| page load: mostly waiting for WebKit's web content process to start, then about 10 ms of loading | 177.8 | 73.1 |
| layout: fonts, images, measure | 178.4 | 0.6 |
| raster and encode: snapshot of the first viewport, RGBA into shared memory | 198.7 | 20.4 |
| transmit: placements and acknowledgement | 200.3 | 1.5 |

With the warm spare, in a herdr pane in Ghostty at 1292 × 2014 device px, from main to first paint sent (median of 10 warm runs, 6 cold runs with `MDDOG_NO_SPARE=1`):

| File | Cold | Warm |
|---|---|---|
| `README.md` | 219 ms | 60 ms |
| `gfm.md` | 291 ms | 96 ms |

Warm `README.md` by phase: render in the spare done at 2 ms, page loaded at 20 ms, layout measured at 23 ms, then 7 ms for a frame after the resize and about 14 ms for each of the two 8 MiB tiles on the first screen.

### Tiles drawn into shared memory

A spare used to send each tile's RGBA back over the socket, and the viewer then copied it into a new shm object. Per 8 MiB tile on the first screen, a local snapshot took 6 to 11 ms (4 to 8 ms waiting for WebKit, 2 ms for the BGRA to RGBA draw), and the same snapshot through a spare took 11 to 16 ms. Now every snapshot draws straight into the shm object the terminal reads. A spare opens it by name. Taking the snapshot without `afterScreenUpdates` was also tried: the wait stayed at 4 ms, so that change was dropped. Asking WebKit for every visible tile at once, then waiting, saved 0.6 ms on two tiles (6.0 against 5.4 ms, 15 runs), so tiles are still taken one at a time. A steady-state snapshot of one tile costs about 3 ms. The first one after a resize costs more.

Measured on 2026-10-19 in a herdr pane in Ghostty, 70 columns (1292 device px) wide, screen unlocked. Times run from just before exec to the "first paint sent" mark, 20 runs after 2 warmups, through `herdr pane run`. That is why they run about 10 ms above the table above. Key times run from the key in the trace to the next frame, 30 presses of `n` after searching "the" in `gfm.md`.

| Measure | Before | After |
|---|---|---|
| Warm `README.md`, p50 / p95 | 80.4 / 84.0 ms | 59.0 / 73.1 ms |
| Warm `gfm.md`, p50 / p95 | 119.6 / 138.5 ms | 103.1 / 116.7 ms |
| Warm, layout ready to first paint sent, `README.md` | 51.1 ms | 30.2 ms |
| Cold `README.md`, p50 | 224.5 ms | 230.5 ms (noise, sd 7 to 13 ms) |
| Cold `gfm.md`, p50 | 307.6 ms | 303.4 ms |
| `n` through a spare, p50 / max | 22.6 / 53.6 ms | 21.0 / 31.6 ms |
| `n` with no spare, p50 | 21.0 ms | 20.7 ms |

The tiles are byte for byte the same: 26 of 26 from `MDDOG_DUMP_TILES`, cold and warm, on `README.md` and `gfm.md`.

### Keymap

Every row of the behavior tables names its tests, and `reference/keymap-check.py` confirms each one ran and passed: 46 rows, 64 test names, all passing. The key-map state machine lives in `src/viewer/app.rs` with no terminal or WebKit in it, so each row is a plain unit test. The search rows that depend on the DOM run in WebKit through `tests/search.rs`, and the watcher row through `tests/watch.rs`.

### Editor, external change, resize, and tiles

Driven through the herdr CLI in a real herdr pane, with `MDDOG_TRACE` logging every frame's scroll row, phase, and the block at the top with its offset (`reference/e2e/`):

| Check | Script | Result |
|---|---|---|
| Scripted `$EDITOR` (`add40.py`): 40 lines added above the view, saved as write-to-temp then rename | `editor_test.py script` | Pass. Top block "First ordered item…" at offset 108.0 px before and after. Scroll moved from row 40 to row 82 plus a 20 px phase. |
| nvim (`:0put =repeat(['nvim line'], 40)`, `:wq`) | `editor_test.py nvim` | Pass. Same block, same 108.0 px offset. |
| Another process saves with write-to-temp then rename | `external_test.py` | Pass. Re-rendered 138 ms after the rename, same block at the same 119.0 px offset. |
| Pane narrowed, then widened again (herdr split, then close) | by hand with the trace | Same top block at the same offset after each re-render. |
| A new search must re-send the tiles that showed the old highlights, and the resident byte count must equal the bytes of the tiles held, through a search, a zero-match search, a search, and 25 `n` | `tiles_test.py` | Pass. Both visible tiles re-sent after the zero-match search. Count equal in all 30 frames. Fails on the code before the fix (57.7 MB counted against fewer tiles held). Every tile sent after the search and each `n` shows the current match in the current-match color (26 of 26, from `MDDOG_DUMP_TILES`). |

### On-screen and search captures

Both scripts drive the installed binary in a herdr pane, capture the Ghostty window with `screencapture -l`, convert it from the display profile to Display P3, and locate mddog's own viewport PNG in it (`reference/e2e/capture.py`).

| Check | Script | Result |
|---|---|---|
| The window capture must match mddog's own PNG of the same viewport | `onscreen_test.py` | Pass on the final build. Max channel difference 2, no pixel over 3, at 1292 × 2014 device pixels. |
| `fixture` in `gfm.md`: 8 matches over 4 screens. Capture after the search, 9 × `n`, and 3 × `N`, and check the current match is in view and drawn in the current-match color | `search_test.py` | Pass on the final build. Order 0 to 7, then 0 and 1 (wraps forward), then 0, 7, 6 (wraps back). The current-match color covers 0.69 to 0.78 of every match box. |

### Tile size

A selection or search highlight re-sends every tile it touches, so the tile size sets the cost of each drag step and each `n`. Tiles were cut from 8 MiB to 2 MiB on 2026-10-17 and went back to 8 MiB on 2026-10-19. I measured in a herdr pane on `gfm.md` at 1121 px wide with the screen unlocked, from the key in the trace to the finished frame, 30 events per run, two runs per size:

| Tile size | Drag step p50 | `n` p50 | Scroll step p50 | First paint p50, `gfm.md` | First paint p50, a 310 KB README |
|---|---|---|---|---|---|
| 8 MiB | 17.0 to 20.6 ms | 20.4 to 20.6 ms | 0.2 ms | 379 ms | 360 ms |
| 2 MiB | 8.4 to 8.8 ms | 12.5 ms | 0.2 ms | 382 ms | 384 ms |

First paint is 12 runs each, read from the first `view` line in the trace. Scrolling costs herdr and Ghostty the same CPU at 8, 2, and 1 MiB. 1 MiB tiles were faster again (6.3 ms per drag step) but added 25 ms of first paint on the large file.

The other option was to send only the changed rectangle, as the pixel engine in zenbu-labs/terminal-browser does. A probe in the same pane found that an in-place edit (`a=f`) displays correctly but costs herdr and Ghostty 2 to 3 times the CPU of re-sending a whole 8 MiB tile. A band image layered over the tile at `z=1` costs about a quarter of a tile and moves correctly with the tile on scroll. Inside mddog, though, the wait for WebKit's next frame (4 to 15 ms) is the same at any size, and a full-width band snapshot saves only about 4.5 ms against an 8 MiB tile. Smaller tiles got most of the gain without patch ids, re-placement on scroll, or folding patches back into tiles.

`onscreen_test.py`, `tiles_test.py`, and `search_test.py` passed with 2 MiB tiles. The on-screen check had a max channel difference of 1, the same as with 8 MiB.

Why 8 MiB again: the gains above are about one 60 Hz frame (16.7 ms) at most, too small for anyone to notice, and the event loop redraws once per batch of events, so a slow drag step doesn't build a backlog. The first-paint numbers are when mddog sent the frame, not when it showed. A screen recording of the herdr pane (`README.md`, 1121 px wide) found that the first screen of 6 tiles at 2 MiB filled in over 3 or 4 steps in about 700 ms, while the spinner sat on its last frame. Turning off prefetch didn't change it. With 8 MiB tiles the first screen is 2 tiles, and it showed at once or in 2 steps 17 ms apart. Plain Ghostty showed the tiles together at either size.

### Band images under herdr

Added on 2026-10-20. Under herdr, every drag step and every `n` re-sent each 8 MiB tile it touched. herdr copies the image, and its client sends it on to Ghostty as about 10.7 MB of base64. The earlier trial above measured only mddog's own time, which is why it found little to gain. This time I measured herdr's and Ghostty's CPU too. Changed rows now go out as a full-width band at `z=1` over the tile, and only under herdr (ARCHITECTURE.md, Display).

All runs were in a herdr pane 1121 px wide (59 × 46 cells of 19 × 38 px, so a tile is 49 rows) with the pane's workspace on screen. herdr forwards images only for the workspace on screen: in a hidden workspace the client and Ghostty used no CPU at all.

**Before any mddog code.** A script wrote the escape sequences straight to the pane, 40 steps 16 ms apart, 3 runs each. CPU is herdr server + client + Ghostty per step, from `ps -o time=` before and after:

| Each step sends | CPU per step |
|---|---|
| Nothing (idle) | 2 ms |
| A whole 8 MiB tile | 89 ms |
| A 2-row band | 6.5 ms |
| A 6-row band | 13 ms |
| A 12-row band | 23 ms |

herdr does not redraw the tile under a band, so the cost scales with the band's size.

**mddog.** A scripted 40-step drag over the first paragraph of ripgrep's README (16 ms apart), and 20 × `n` for `the` in `gfm.md` (150 ms apart). Runs alternated between the two builds. Median, with the range in brackets:

| | Before | Bands |
|---|---|---|
| Drag, CPU per step | 86.8 ms (76.5 to 87.5) | 18.2 ms (17.0 to 18.8) |
| Search, CPU per step | 91.0 ms (89.5 to 92.0) | 47.5 ms (46.5 to 48.5) |
| Drag, pixels sent per step | 8.8 MB | 2.7 MB |
| Search, pixels sent per step | 11.7 MB | 3.7 MB |

mddog wrote 300 to 650 bytes to the tty per step either way, since the pixels go over shared memory.

mddog's own time per step, from the input to the end of a frame, measured two ways:

| | Before | Bands |
|---|---|---|
| Drag, to the next frame end | 7.6 ms (6.2 to 11.2) | 8.5 ms (6.7 to 10.0) |
| Drag, to the end of the frame that handled it | 22.1 ms (20.2 to 22.3) | 12.8 ms (12.7 to 13.1) |
| Search, to the next frame end | 21.6 ms (20.4 to 23.9) | 8.5 ms (7.2 to 9.5) |
| Search, to the end of the frame that handled it | 21.6 ms (20.4 to 23.9) | 8.5 ms (7.2 to 9.5) |

The first way, the one planned, matches an input to whatever frame ends next. A drag input that arrives while an 8 MiB frame is being built gets that frame's end, though the frame doesn't show it. That makes slow frames look fast, so the second way pairs each input with the frame that handled it. An earlier set of 5 runs per build read 9.4 ms before and 10.1 ms after on the first way, and 20.7 and 13.5 ms on the second. The user chose to keep bands, given the drop in CPU and the second measure.

**Pixels.** `onscreen_test.py`, `search_test.py`, and `tiles_test.py` pass with bands, with a max channel difference of 1, the same as before. A scratch check compared the Ghostty capture with mddog's own viewport PNG while bands were on screen: mid-drag, after release, after scrolling 3 rows, during a drag over most of the screen, and after the fold. A window capture takes over a second, longer than the 1 s idle fold, so that check ran on a build with an 8 s fold. All five matched with a max difference of 1. The first long-drag capture found one wrong row. A line's text rect can stay the same while its line-end highlight changes, so each changed selection rect now takes in one row above and below.

**What didn't work.** Joining every changed rect from the page's selection call made a long drag fall back to whole tiles once the selection passed half a tile, because that call returns the old and the new selection, so every selected line looked changed. Only the rects that differ count now.

### Corpus run

The final binary ran with `--bench` on every corpus file in the herdr pane (`reference/corpus.py`): 8,921 runs, 8,904 `.md` and 17 `.mdx`. The 20 popular READMEs count twice, once from the `~/dev` scan and once from the README list.

| Result | Count |
|---|---|
| Non-zero exit, panic, or timeout | 0 |
| No first paint | 0 |
| First paint with no blocks for a file that has text | 0 |

First paint across the corpus, from exec to the terminal's answer:

```
p50 = 194.3 ms   p95 = 229.0 ms   p99 = 357.0 ms   max = 925.4 ms
```

135 files (1.5%) take longer than 300 ms. Most are heavy documents: Mermaid diagrams (the mermaid README, 740 ms with GitHub's viewer) and long files (44 KB to 55 KB at 660 to 690 ms). The slowest three, 7.7 KB notes at 900 ms, ran back to back and paint in 192 to 195 ms when run again, so a passing system slowdown caused those. The median file took 187 ms in this run.

## Mermaid with Pele

mddog first ran Mermaid through GitHub's own viewer bundle: 65 files, 6.3 MB, with no license, inside an iframe driven by faked postMessage events. I replaced it with Pele after scoring both in Playwright's WebKit at 898 CSS px and 2x against the github.com references.

| Check | GitHub viewer | Pele 1.6.0 |
|---|---|---|
| `gfm.md` full-page dssim (threshold 0.0032) | 0.0024, pass | 0.063, fail |
| Flowchart block height (GitHub 264.5 px) | 264.5 px | 121 px |
| Sequence block height (GitHub 425 px) | 425 px | 276 px |
| mermaid README, 10 diagrams, time until drawn | about 660 ms | about 185 ms |

Pele uses its own layout, so the diagrams no longer match GitHub, and every block below a diagram moves. With that, matching GitHub pixel for pixel stopped being a goal. In return the binary drops 6.3 MB of unlicensed code for one 668 KB MIT file, diagrams draw 3 to 4 times faster, and the page code loses the iframe glue. Pele has no pan buttons and pans by drag, which mddog uses for selection, so pan is gone.

## Viewer speedups (2026-10-21)

Four changes from the `perf/viewer` branch (`afdcb72`, `66ed900`, `c9116d2`, `2d46d1b`), measured with `reference/bench/viewer.py` in a headless herdr pane at 119 × 40 cells, 16 × 32 px. Median ms, before → after, 10 alternating rounds per cell. Warm uses the spare. Cold sets `MDDOG_NO_SPARE=1`.

| File | Mode | First paint | Scroll to end | Search Enter | Search `n` | Resize | Reload |
|---|---|---|---|---|---|---|---|
| `README.md` | warm | 51 → 33 | 0.3 → 0.4 | 24 → 31 | 10 → 18 | 44 → 25 | 43 → 23 |
| `README.md` | cold | 262 → 223 | 0.3 → 0.3 | 36 → 24 | 14 → 17 | 44 → 24 | 43 → 25 |
| `fixtures/gfm.md` | warm | 220 → 206 | 14 → 12 | 16 → 11 | 15 → 14 | 103 → 93 | 102 → 81 |
| `fixtures/gfm.md` | cold | 460 → 452 | 14 → 12 | 8 → 7 | 16 → 18 | 104 → 91 | 103 → 111 |
| `ARCHITECTURE.md` | warm | 65 → 44 | 69 → 59 | 50 → 45 | 5 → 5 | 54 → 38 | 46 → 33 |
| `ARCHITECTURE.md` | cold | 283 → 267 | 68 → 59 | 53 → 46 | 5 → 4 | 50 → 38 | 44 → 33 |
| `docs/behavior.md` | warm | 64 → 42 | 86 → 72 | 17 → 16 | 14 → 14 | 51 → 37 | 43 → 32 |
| `docs/behavior.md` | cold | 278 → 270 | 85 → 71 | 17 → 15 | 14 → 16 | 56 → 37 | 42 → 31 |

- An A/A run of the old build against itself agreed within about 5% on first paint, scroll, resize, and reload. Cold first paint is bound by AppKit and WebKit startup and has a ±5% spread.
- On short files, search Enter and `n` jump between about 4 and 17 ms in both builds, A/A included. Those cells are noise.
- The cold `gfm.md` reload cell is unsettled. Two reruns with the same binaries gave 102 → 87 and 102 → 94. Both builds show occasional page loads over 38 ms.
- Tiles were byte-identical between builds: 1,832 of 1,832 over all four files and scenarios, and 2,970 of 2,970 over 360 resizes and 54 reloads with 10 of 12 cores busy. The height poll's p50 was 2.2 ms and its max 23 ms, so its 100 ms fallback never fired.

Rejected or left out:

- **No frame wait before search snapshots.** The tiles differed in every run, with stale highlights. CSS highlights paint only during a rendering update.
- **Resize without reload.** Done later, see "Resize in place" below. It saved 13 to 21 ms, not the 30 ms estimated here. Pele diagrams lay out again correctly. GitHub-engine diagrams don't, so those pages still reload.
- **Paint before Mermaid finishes.** Done later for the GitHub engine, see "Paint before GitHub diagrams load" below. It saved about 140 ms of first paint on `gfm.md`. Pele diagrams were never waited for.
- **Resize GitHub-engine diagram pages in place.** A prototype matched a fresh load pixel for pixel, but saved no time: about 44 ms both ways. It also relied on GitHub viewer internals. It had to put `#mermaid-view-template` back into each iframe, and the viewer's messenger ignores a second ready message. These pages still reload on resize.
- **Paint before math finishes.** MathJax is already done by the page's `load` event: 3.7 ms after it on `gfm.md`, 1.3 ms on a page with 120 math blocks. The 1 s wait in `render_doc` (`src/viewer/preview.rs`) is only a safety cap. If it runs out, the page paints anyway and the late re-send redraws the math.

### Resize in place (2026-10-21)

From `perf/resize-in-place` (`56b583d`). A resize lays the loaded page out again instead of reloading it (ARCHITECTURE.md, Event loop and keys). Measured with `reference/bench/viewer.py`, 10 rounds per binary in alternating order, median ms of the resize step. The A/A columns are main against itself. The main and new columns ran on a busier machine, so main's own resize there is 2 to 6 ms slower than in A/A.

| File | Mode | A/A main | A/A main | main | new | Change |
|---|---|---|---|---|---|---|
| `ARCHITECTURE.md` | cold | 38.2 | 37.8 | 42.5 | 22.5 | −47% |
| `ARCHITECTURE.md` | warm | 38.5 | 37.9 | 44.2 | 23.0 | −48% |
| `README.md` | cold | 26.1 | 26.4 | 30.6 | 17.9 | −42% |
| `README.md` | warm | 28.2 | 27.6 | 32.0 | 18.2 | −43% |
| `docs/behavior.md` | cold | 36.6 | 37.5 | 40.2 | 23.7 | −41% |
| `docs/behavior.md` | warm | 37.7 | 37.5 | 40.0 | 24.1 | −40% |
| `fixtures/gfm.md` (github) | cold | 89.5 | 90.1 | 94.4 | 93.4 | −1% (reloads) |
| `fixtures/gfm.md` (github) | warm | 91.1 | 90.3 | 98.5 | 101.2 | +3% (reloads) |
| `fixtures/gfm.md` (pele) | cold | | | 47.0 | 30.1 | −36% |
| `fixtures/gfm.md` (pele) | warm | | | 49.2 | 30.2 | −39% |

- First paint, scroll, search, and reload stayed within the A/A spread. Search `n` swings up to ±47% even in A/A, because its median is about 6 ms.
- The remaining 18 to 24 ms is mostly the retile and the new tiles. Those steps were not timed alone.
- Tiles matched a fresh load at each width: 728 of 728 on `gfm.md` with both engines, cold and warm, at the top and 7 pages down, through 119, 80, 140, 100, 60, 119, and 80 columns. A fixture with wide code, wide tables, and display math, scrolled sideways first, matched in 146 of 146. Scroll, search, and reload dumps stayed byte-identical to main.
- **Why GitHub-engine diagrams reload.** With the in-place path forced on, the diagram tiles differed from a fresh load at 80, 100, and 60 columns, and the reading position below the diagram moved (263 against 258 rows at 80 columns). The viewer reads `container.getBoundingClientRect().width` once, when it renders. A fix would re-send `code_rendering_service:data:ready` with the new width to each iframe and wait for it to draw. It was not tried.
- **Why diagram pages wait for a frame.** In 40 resizes between 60 and 100 columns with Pele, the page came out 336 CSS px short 3 times in 20, and the footnotes were cut off, because Pele's refit landed after the measure.
- After about 15 resizes, both builds show the same small drift in the `↩` emoji's pixels, up to 27 color levels. That comes from WebKit and was already in main.

### Paint before GitHub diagrams load (2026-10-21)

From `perf/early-paint` (`3a4f75f`, `2f41c78`, `6386619`, `1b0bd30`). With the GitHub engine the first paint no longer waits for diagrams. A skeleton holds each diagram's 500 px box, the iframes start after the first paint, and the late re-send shows the diagrams (ARCHITECTURE.md, First paint and Mermaid). Medians on `fixtures/gfm.md`, GitHub engine, 12 rounds, ms from start. "Settled" is the last re-send, and on `gfm.md` it also waits for the remote badge image.

| Build | First paint, cold | First paint, warm | Settled, cold | Settled, warm |
|---|---|---|---|---|
| main | 399 | 215 | 463 | 273 |
| main again (A/A) | 396 | 211 | 460 | 259 |
| (a) paint first, frames start at once | 384 | 205 | 438 | 266 |
| (b) frames start after the first paint | 258 | 72 | 424 | 245 |

- A/A noise is under 5 ms for first paint and about 15 ms for settled.
- **Why (a) failed.** Painting first while the iframes loaded saved only 5 to 9 ms (against its own main baseline of 393, 210, 462, and 268). The iframes load in the page's own WebKit content process, so they slowed the first snapshot by about as much as the wait they replaced. The wait for the new size after a resize went from 2 to 40 ms, and the snapshot took 75 ms longer. (b) starts the iframes only after the first paint.
- (b) saves about 140 ms of first paint, cold and warm. Diagrams also settle 20 to 40 ms sooner than on main, because the page's `load` event no longer waits for the iframes.
- Pele didn't change. Its diagrams draw before the page finishes loading, so nothing waited. `gfm.md` was 277 against 285 ms cold and 80 against 79 warm, and `README.md` 210 against 219 cold and 36 against 36 warm, all within noise.
- Settled tiles are byte-identical to main: 16 dump pairs over `README.md`, `gfm.md`, `ARCHITECTURE.md`, and `docs/behavior.md`, cold and warm, both engines. Two scratch files with diagrams on the first screen also matched once settled, in GitHub dark, GitHub light, and Pele.
- The position holds through the re-send. Three `f` presses right after the first paint left `Diagram 4` at the top with the same offset (17.4) before and after. The scroll row moved from 111 to 67, because the three diagrams above shrank by 234 CSS px each.
- **The skeleton fills the whole reserved box** (`inset: 0`). A 16 px top and bottom inset ran its border through the expand and copy buttons, which sit at the section's top-right corner on github.com too.
- **Heights apply at once, when the last diagram is done.** Applying each diagram's height as it drew changed the live layout before the re-send, so a click could hit the wrong element.
- **A start sent before setup is kept.** A page that took over 3 s to reach DOMContentLoaded lost the call, and the skeletons stayed up until the 60 s late limit.
- The 500 px box is GitHub's reserved height. Most diagrams are shorter, so the content below moves up when they arrive (234 CSS px each on `gfm.md`). Predicting the real height would need a render, so on a first open it still moves. A reload or reopen now uses the last draw, see "Skeleton heights from the last draw" below.

### Skeleton heights from the last draw (2026-10-21)

From `perf/diagram-heights` (`78f2813`, `66ec15f`). A GitHub-engine skeleton now opens at the height its diagram drew at last time, so a reload or reopen no longer moves the page when the diagrams arrive (ARCHITECTURE.md, Mermaid).

GitHub's viewer sets a diagram's height to max(180, viewBox height × min(1, page width / viewBox width)). That held exactly on 5 diagram kinds at 4 widths. mddog stores the viewBox and not the height, so the guess stays right after a resize. Sizes came out the same in dark and light, so the key is the source alone.

Page height in CSS px from the trace's `doc=` field, first view → settled, base against new, 2 rounds each:

| File | Build | Open | Reload |
|---|---|---|---|
| 5 diagram kinds, reopen | base | 4192 → 3566 | 4272 → 3646 |
| 5 diagram kinds, reopen | new | 3566 → 3566 | 3646 → 3646 |
| `gfm.md`, reopen | base | 10027 → 9563 | moves |
| `gfm.md`, reopen | new | 9559 → 9563 | moves 4 px |

- The 4 px left on `gfm.md` comes from the remote badge image, which loads late. Pele moves by the same 4 px on a copy with no math, and Pele never uses the guess. A first open matches base exactly.
- Settled tiles were byte-identical to base, by tile position, on `README.md`, `ARCHITECTURE.md`, `docs/behavior.md`, `gfm.md`, the 5-kind file, and `gfm.md` without math. Both engines, first open and reopen.
- `g5_skeleton_takes_the_drawn_height` in `tests/search.rs` checks the guess against the settled height at 400 and 900 px.
- **A 45 s deadline for slow diagrams.** Each diagram may take 4 s to load and 4 s to draw, so 15 or more could outlast the viewer's 60 s `LATE_LIMIT` and change the layout after the last re-send. In a fixture with 16 diagrams that never answer, a 17th that answers late, and a link below them, base settled at 60.25 s. The 17th diagram drew at about 64 s and moved the live page up 300 px with no re-send, so a click on the drawn link at 70 s missed. With the deadline the page settled at 45.3 s and the click reached the link. A re-check in Rust would also work, but the deadline is one line.
- Left as is: at the deadline, diagrams that never started stay blank, and one still drawing can change its own pixels inside its fixed box. The 4 s timeouts already did this, and the layout does not move. Showing a "timed out" message instead would be a product choice.
- Left as is: two `mdd` processes can overwrite each other's sizes file. A lost entry only brings back the jump. The 512-entry cap drops the oldest entry first, used or not, so a settle doesn't rewrite the file each time.

## File list (2026-10-21)

From `feat/file-browser` (`10c6d0a`, `187fdd1`, `2c0e206`). `mdd` with no file, `mdd <folder>`, and Ctrl-p list Markdown files to pick from (ARCHITECTURE.md, File list).

The list uses `ignore` and `nucleo-matcher`, not `fff-search`. Clean release builds on this branch:

| Option | Crates added | Clean build | `mdd` size added |
|---|---|---|---|
| `fff-search` | 75 | +64 s | +3.06 MB |
| `ignore` + `nucleo-matcher` | 10 | +29 s (inflated by a cargo lock wait) | +0.63 MB |

`fff-search` also publishes only nightly versions, always builds LMDB and libgit2 with no feature to turn them off, adds a second `notify` version, has no extension filter or count cap, and reports match positions in bytes, not characters.

First paint of `mdd <file>` did not change. Medians in ms over 12 alternating rounds. The A/A column is main against a copy of itself.

| File | Mode | A/A | main | branch | Change |
|---|---|---|---|---|---|
| `README.md` | cold | +15% | 272.4 | 260.0 | −5% |
| `README.md` | warm | +4% | 36.5 | 38.3 | +5% |
| `fixtures/gfm.md` | cold | +6% | 336.0 | 328.5 | −2% |
| `fixtures/gfm.md` | warm | +5% | 70.6 | 71.6 | +1% |
| `ARCHITECTURE.md` | cold | −5% | 286.4 | 287.0 | 0% |
| `ARCHITECTURE.md` | warm | +2% | 48.0 | 48.3 | +1% |
| `docs/behavior.md` | cold | +1% | 285.1 | 287.0 | +1% |
| `docs/behavior.md` | warm | +5% | 45.5 | 47.4 | +4% |

- Every cell is inside the A/A spread of −5% to +15%. The bench binary was `187fdd1`. After it, the file path only gained a `listing` check in `prefetch_target`.
- Tile dumps matched main on `README.md`, `gfm.md`, and `ARCHITECTURE.md`, cold and warm. On `docs/behavior.md`, main itself sent 155 tiles in one run and 152 in the next, and a rerun of main, a second copy of main, and the branch all sent the same 152.
- A walk of `~/dev` found 8,597 files. The first came in 1 to 3 ms, and the whole walk took 2.4 s cold and 0.66 s warm. The list fills in as batches arrive. In a small repo the list was on screen about 190 ms after the process started.
- Left out: frecency, by the user's choice, and a preview pane, by design.
- Left as is: the 10,000 cap applies in walk order, before the newest-first sort, so a repo with more files can miss some newer ones. Walk and read errors are skipped, so an unreadable folder shows "No Markdown files here". The filter takes fzf syntax, so a literal leading `!` needs a backslash (`\!`). With decomposed Unicode names, a highlight can land one letter off, though the match is right. The viewer's bottom row has no hint for the list. The `?` screen lists Ctrl-p.

## Loading skeleton (2026-10-21)

From `feat/loading-skeleton` (`89996f8`). A page-shaped skeleton in terminal cells replaced the 300 ms spinner (ARCHITECTURE.md, First paint).

The timing was chosen after a side-by-side demo. The brief's default, a 90 ms delay with a 200 ms minimum, added about 100 ms to every cold start. 90/100, immediate/100, and 100/100 were also compared. The user picked 100/100: draw only when the page isn't ready after 100 ms, then hold at least 100 ms.

First paint, median ms, 10 rounds per cell. Main is `2c0e206`.

| File | Mode | A/A main | A/A main copy | main | branch | Change |
|---|---|---|---|---|---|---|
| `README.md` | warm | 37.6 | 34.5 | 35.2 | 37.5 | +7% (A/A −8%) |
| `ARCHITECTURE.md` | warm | 43.6 | 43.7 | 42.7 | 43.2 | +1% |
| `docs/behavior.md` | warm | 44.5 | 42.8 | 42.4 | 45.2 | +7% (A/A −4%) |
| `fixtures/gfm.md` | warm | 69.3 | 69.0 | 68.3 | 71.3 | +4% |
| `README.md` | cold | 182.8 | 184.0 | 181.7 | 207.0 | +14% |
| `ARCHITECTURE.md` | cold | 191.2 | 192.3 | 190.9 | 206.3 | +8% |
| `docs/behavior.md` | cold | 190.1 | 189.7 | 187.7 | 205.1 | +9% |
| `fixtures/gfm.md` | cold | 233.3 | 228.9 | 226.4 | 234.2 | +3% |

- Warm cells are inside the A/A spread of up to 8%. The loader thread does nothing in the first 100 ms.
- Cold starts on files ready by about 185 ms pay 15 to 25 ms. That is the minimum at work: the skeleton draws at about 105 ms, so the page waits until about 205 ms. Slower files such as `gfm.md` pay nothing. This was the user's chosen trade-off.
- Traces of 40 cold and 40 warm runs: no warm run drew the skeleton, and the warm median was 43.3 ms. Every cold run drew it. Cold medians were 105.1 ms to draw, 170.3 ms to layout ready, and 207.2 ms to first paint. The shortest showing was 100.3 ms. On a 1.7 s file it drew at 102 ms and cleared in the same write as the first paint.
- Tiles were byte-identical to main: 146 of 146 over all four files, cold and warm, scrolled to the end.
- Left as is: before the probe answers, the margins assume a 2x screen, so a 1x screen gets double margins for the first frames until a full repaint. A `#RGBA` or `#RRGGBBAA` background loses its alpha, and a page background changed in `style.css` is not used. A slow file opened from the file list sits on the list until it paints.

## Figures moved out of ARCHITECTURE.md (2026-10-25)

ARCHITECTURE.md was rewritten for reading. These measurements were in it and are kept here.

- WebKit fires the load event, then spends about 7 ms on the first full layout before it answers the next call. A rendering update (a frame) costs about 2.6 ms, with outliers from 30 ms to 1.6 s whatever the idle gap before it, so there is no idle penalty to work around.
- A warm first paint takes 35 to 45 ms (medians of 10 rounds, 2026-10-21), against about 200 ms cold.
- Per 8 MiB tile, the WebKit snapshot takes 3 to 5 ms and NSImage to CGImage takes nothing. The BGRA to RGBA swap takes 1.1 ms, against 2.0 ms for a Core Graphics draw.
- Under herdr, re-sending an 8 MiB tile costs herdr and Ghostty about 90 ms of CPU, which is why selection and search use bands.
- After a resize, `retile` polls `innerHeight` until the page has its new size, which takes about 2 ms. Waiting for a frame instead takes 15 to 25 ms, because the rendering update after a resize to the full document height is expensive. The first snapshot then absorbs part of that work, so the net gain is 7 to 15 ms per render.
- A glide frame takes under 0.1 ms of work in `show()`, except the first frame of a long jump, which sends tiles (12 to 16 ms). The event loop's 8 ms wake is about 9 to 10 ms in practice.
- A warm first paint of ARCHITECTURE.md took about 44 ms, and page load (20 ms, all WebKit) was the largest part. The first `config::theme()` call for a theme pair costs 2.1 ms, which is why a side thread reads the appearance at startup.
- Headless: a PNG takes about as long to encode as a snapshot (5 ms for 8 MiB). Encoding on up to 4 threads made a 19-tile page 35% faster.
- The startup sweep takes under 0.5 ms with nothing to remove. Removing 220 leftovers took 22 ms, all after the first paint, which stayed at 211 ms (median of 30 headless runs with and without the sweep, 2026-10-22).
- Pele draws 3 to 4 times faster than mermaid. PNG makes a tile about 30 times smaller for about 1 ms of encoding.
- Resize in place saves 13 to 21 ms per resize (see "Resize in place" above).
