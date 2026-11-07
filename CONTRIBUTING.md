# Contributing

How to check, test, debug, and release mddog. For a map of the code, read `ARCHITECTURE.md` first.

## Checks

- `./check.sh` is the gate: `cargo fmt --check`, then `cargo clippy --locked --all-targets -- -D warnings`, then `cargo test --locked`.
- `[lints.clippy]` in `Cargo.toml` turns on some pedantic and nursery lints, `unwrap_used` among them. `clippy.toml` exempts unit tests, and each `tests/*.rs` file that needs `unwrap` has its own `#![expect]` or `#![allow]`. Outside tests, an `expect` names the invariant that holds, such as a built-in asset or a constant regex.
- Each `unsafe` block or impl has a `// SAFETY:` comment on the line above that names the invariant it relies on, and each `unsafe fn` has a `# Safety` doc section. Clippy's `undocumented_unsafe_blocks` enforces the comment, and `unnecessary_safety_comment` flags one left on safe code. Before you add a block, check whether the binding or std already has a safe call. When the same call repeats, wrap it once in a small safe function next to its callers, as `tmp::mine` does for `getuid`.
- CI (`.github/workflows/check.yml`) runs `./check.sh` on every push and pull request, except those that change only `website/`. It runs on a macOS runner, because the renderer and the pty tests need macOS.
- `python3 reference/keymap-check.py` checks that every test named in `docs/behavior.md` ran and passed.
- `claude plugin test plugins/claude-code` runs the plugin's tests in `plugins/claude-code/tests/`. `./check.sh` and CI don't run them. `tsc -p plugins/claude-code` type-checks them once a live load has written `.claude-plugin/types/` (gitignored). The tracked `plugins/claude-code/types/` is a different folder: it types the plugin's own `open` tool.
- The docs site has its own gate: `cd website && pnpm install && pnpm types:check && pnpm build`.
- `.github/workflows/website.yml` deploys the site to Cloudflare Pages (https://mddog.pages.dev) on every push to `main` that changes `website/`. It needs the `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` repo secrets. To deploy by hand, run `pnpm run deploy` in `website/`. Plain `pnpm deploy` is a different, built-in pnpm command, and it fails.

## Tests

| File | Covers |
|---|---|
| `tests/help.rs` | `mdd` in a pty: startup and the file list, keys, resize and font zoom, tile alignment, links, copy, the pointer shape, signals, the editor, and Ctrl-z |
| `tests/engine.rs` | the engine's typed answers: wide-block scroll and restore, button indices after a relayout, an empty file, search, select and hover, diagram expand and collapse, and raw HTML |
| `tests/search.rs` | search, links, anchors, selection, hover, presses, copy buttons, Mermaid buttons, and wide blocks, through the engine calls the viewer makes |
| `tests/math.rs` | the parser's math against `tests/math.golden`, and the TeX layout of commands taken from those files |
| `tests/headless.rs` | `--headless` end to end |
| `tests/watch.rs` | the file watcher, including saves done as write-then-rename |

- `tests/help.rs` runs `mdd` in a pty as its own session leader, so Ctrl-z can't stop it there. the Ctrl-z tests call `launch` with `job` set, which forks a small job-control parent as a shell does: `mdd` runs in its own foreground group, and the parent passes its stops and exit status up to the test.
- `tests/search.rs` is an ordinary test file. Each test makes an `Engine`, loads a Markdown string, and calls the same methods the viewer does. Points on the page come from search rects and the layout, not from fixed pixels. `docs/behavior.md` names these tests.
- `tests/math.rs` renders each `fixtures/math/` file's `Doc` into one line per block that holds math or a `$`: inline math as `⟨...⟩`, display math as `⟦...⟧`, and code spans in backticks. It must match `tests/math.golden` exactly. `UPDATE_GOLDEN=1 cargo test --test math` rewrites it. `fixtures/math/SOURCES.md` lists where each fixture came from.
- A unit test in `src/render/highlight/textmate.rs` checks TextMate output against `tests/textmate.golden`. `UPDATE_GOLDEN=1 cargo test` rewrites it.
- Every test that launches `mdd` sets `MDDOG_CACHE_DIR` to `$TMPDIR/mddog-test-cache`, so `./check.sh` leaves `~/Library/Caches/mddog/` alone. Unit tests of `remote` pass no disk folder to `fetch_in`, so they skip the disk cache.
- The pty tests in `tests/help.rs` that click at a cell and expect a given word or button set `MDDOG_SCALE=2`, since GitHub's macOS runners have a 1x screen. The others run at the screen's own scale, since a forced 2x on a 1x screen changes how tiles line up.

## Reference tools

| Tool | Use |
|---|---|
| `reference/e2e/` | Drives mddog in a herdr pane inside Ghostty, for editor, external change, on-screen, search, and tile checks. The on-screen and search checks capture the Ghostty window. |
| `reference/capture.mjs` | Captures github.com's view of a file with Playwright, to compare a page by eye. Don't commit the captures. |
| `reference/export-grammars.mjs` | Rebuilds `assets/textmate/` from starry-night |
| `reference/keymap-check.py` | Checks that every test named in `docs/behavior.md` ran and passed |
| `reference/bench/` | The interactive viewer benchmark |
| `reference/probes/langs.md` | Code samples in many languages, for highlight checks |

**Benchmark.** `reference/bench/viewer.py` runs the viewer in a headless herdr pane, drives it with keys, and times first paint, scroll, search, resize, and reload from the trace. `table.py` turns its runs into a table, and `tiles.py` compares two builds' tile dumps. Alternate old and new builds, and report medians of 10 or more rounds.

- A headless herdr server does not resize the pty when the layout changes, so the harness sets the size with `stty -f`.
- After each reload, the file watcher reports one extra event that doesn't change anything, and the harness skips it.
- Don't export `XDG_CONFIG_HOME` into the bench: herdr keeps its socket under it, so every call misses the test session. `mdd` in the pane runs with `HOME` set to `VP_HOME`, so put a test config in `$VP_HOME/.config/mddog/config.toml`. The bench exits on the first failed herdr call and names a suspect `XDG_CONFIG_HOME` or `HERDR_CONFIG_PATH`.

## Environment variables

| Variable | Read by | Effect |
|---|---|---|
| `MDDOG_TRACE=FILE` | `mdd` | Log keys, renders, scrolling (with each drawn pixel offset and frame time), placed images, cell geometry, and the page height after each measure |
| `MDDOG_SCALE` | `mdd` | Pixels per CSS pixel in place of the screen's, so pty tests click the same page points on a 1x CI runner. For tests |
| `MDDOG_DUMP_VIEW=FILE` | `mdd` | With `MDDOG_TRACE`, write a PNG of the viewport after each frame |
| `MDDOG_DUMP_TILES=DIR` | `mdd` | Write each tile as it is sent |
| `MDDOG_DEBUG` | `mdd` | Print timing marks to stderr |
| `MDDOG_CACHE_DIR=DIR` | `mdd` | Use `DIR` as the cache root in place of `~/Library/Caches/mddog` |
| `TERM_PROGRAM=herdr` | `mdd` | Use herdr's storage limit, and send selection and search changes as bands (an XTVERSION reply that starts with `libghostty` does the same) |
| `EDITOR` | `mdd` | The editor for `e` |
| `XDG_CONFIG_HOME` | `mdd` | Where to look for `mddog/config.toml` |
| `MDDOG_BIN` | the plugin, `reference/e2e/` | The binary to run |
| `MDDOG_INSTALL_DIR`, `MDDOG_RELEASE_URL` | `install.sh` | Where to install, and which release to fetch |
| `MDDOG_PANE` | `reference/e2e/` | The herdr pane the checks drive |

## Releasing

`mdd` ships through the [`qu8n/tap`](https://github.com/qu8n/homebrew-tap) Homebrew tap and `install.sh`, and the plugin through the Claude Code marketplace. Claude Code caches an installed plugin by the `version` in its `plugin.json`, so each release bumps it along with Cargo's.

1. Run `claude plugin test plugins/claude-code`.
2. Set the same `version` in `Cargo.toml` and `plugins/claude-code/.claude-plugin/plugin.json`, then run `cargo build` to update `Cargo.lock`.
3. Commit as `chore: release X.Y.Z`.
4. Push a `vX.Y.Z` tag. `.github/workflows/release.yml` checks that the tag matches both versions, runs `./check.sh`, then builds both architectures and publishes the release. Last, it points `Formula/mddog.rb` in `qu8n/homebrew-tap` at the new tarballs and pushes, using the `HOMEBREW_TAP_TOKEN` repo secret (a fine-grained token with write access to that repo's contents).
