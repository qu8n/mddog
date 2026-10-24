# Terminal UAT

Manual tests of mddog in each terminal, alone and inside a multiplexer. The results feed the Limitations section of the README.

Build under test: `3cbe73e`. Fixes from this round: `33d2779` (stale TOC and status text, iTerm2) and `7773209` (Warp cell size). Run `cargo install --path .` before each session so `mdd` is current.

## Matrix

Status: ⬜ not run, ✅ works, ⚠️ works with a limitation, ❌ broken, ➖ skipped.

| Terminal | Version | Alone | herdr 0.9.5 | tmux 3.8 | zellij 0.45.1 |
| --- | --- | --- | --- | --- | --- |
| Ghostty | 22391ed64 | ✅ | ✅ | ✅ | ❌ |
| kitty | 0.49.2 | ✅ | ✅ | ✅ | ❌ |
| WezTerm | 0.1.0 | ⚠️ | ⚠️ | ✅ | ❌ |
| iTerm2 | 3.7 nightly 2026-10-22 | ⚠️ | ⚠️ | ✅ | ❌ |
| Warp | 0.2026.10.21 | ❌ | ⚠️ | ✅ | ⚠️ |

mddog expects tmux to fail with a clear message ("this terminal can't show images"), so for tmux the first check is that message, and whether `set -g allow-passthrough on` changes anything.

## Checks

Run these in each cell, from the repo root. Note anything odd, even if the check passes.

1. **File list.** `mdd`. The list draws, typing filters it, Enter opens a file.
2. **First paint.** `mdd README.md`. Fonts, tables, and code colors look right. The page paints in under a second.
3. **Rich content.** `mdd fixtures/gfm.md`. Math, Mermaid, and alerts draw.
4. **Scroll.** `j` `k`, `Space` `b`, `g` `G`, and the mouse wheel, many times down and then many times up. Scrolling up matters: in herdr on iTerm2 it once turned the page blank. No blank or stale tiles, no tearing, no lag.
5. **Overlays.** `?` help, `t` table of contents, `/` search with `n` `N`. Each opens and closes cleanly.
6. **Mouse.** Click a link and a heading anchor. Drag to select, then `Cmd-c`. Paste somewhere to check the clipboard.
7. **Resize.** Resize the window (and the pane, in a multiplexer). The page lays out again at the new width.
8. **Edit and reload.** `e`, change a line, save, quit the editor. The preview shows the change. Edit the file from another pane too, to check live reload.
9. **Quit.** `q`. The shell comes back clean, with no leftover images.
10. **Panes and tabs** (multiplexer only). Split the pane, switch tabs, and zoom. Images stay in mddog's pane and come back when you return.

## Results

<!-- One section per terminal, filled in as we go. -->

### Ghostty

- Alone: all 9 checks work, including scrolling up and the mouse wheel.
- herdr: scrolling up is clean with no lag. Split, zoom, and tabs work.
- tmux: mddog exits 1 with the "can't show images" message, as expected. `allow-passthrough on` or `all` makes no difference.
- zellij: ❌ scrolling garbles the page. Code blocks are drawn twice and offset, and rows smear after scrolling back to the top. Repro: in `zellij`, `mdd fixtures/gfm.md`, then press Space 3 times.
- Every cell: the first `G` on a 628-line file takes 2 to 3 s to settle.

### kitty

- Alone and herdr: all checks work. `G` settles in about 0.4 s, so the slow `G` seen in Ghostty doesn't happen here.
- tmux: refuses with the clear message. `allow-passthrough` makes no difference.
- zellij: ❌ the same garbling as in Ghostty. A code block is drawn about 5 times after Space 3 times in `fixtures/gfm.md`, and it stays. `smooth_scroll = false` fixes that short repro, but heavy scrolling still garbles.
- The relative link to the MDX fixture shows "Links to local files don't open in the preview", as designed. The Ghostty "missed click" was probably this notice.

### WezTerm

- Alone: ⚠️ only for `Cmd-c`, which copies nothing with the default config, as the README says. `c` (copy source) works. The wheel works, scrolling up is clean, and `G` settles in about 0.3 s.
- herdr: ⚠️ for the same `Cmd-c` reason. Heavy scrolling up, by keys, wheel, and fast bursts, never went blank or stale. A pixel timeline puts the page full about 0.4 s after each step, and the lag wasn't visible by eye. That is milder than the README's herdr note suggests.
- tmux: refuses with the clear message.
- zellij: ❌ the known garbling, and then WezTerm itself crashes, 5 times out of 5. Repro: in `zellij`, `mdd fixtures/gfm.md`, then Space 1 to 3 times. WezTerm logs "Failed to allocate ~11M quads" and panics at `draw.rs:258`. The zellij session survives. Checks 6 to 10 weren't run.

### iTerm2

- Alone: ⚠️ `Cmd-c` doesn't reach mddog, as the README says. iTerm2 shows its "mouse reporting has prevented making a selection" banner over the top of the page. The wheel works. `G` settles in about 0.4 s.
- herdr: ⚠️ for the same `Cmd-c` reason. Over 3 rounds of scrolling down and up, with paced keys, bursts, and the wheel, checked on 60 fps video, the page never went blank and always recovered. One step fills in 0.1 to 0.3 s. `G` and `g` take 1.2 to 1.5 s. A burst of 25 Space then 25 `b` took about 20 s to settle.
- tmux: refuses with the clear message.
- zellij: ❌ the known garbling. iTerm2 didn't crash.
- Possible mddog bug: after `t` then Esc, "TABLE OF CONTENTS" and its entries stay drawn over the page in herdr and zellij. Alone it closed cleanly, except once on a fast `t` `t`.
- Possible mddog bug: after a resize, zoom, tab switch, or split, copies of the status bar stay at old row positions over the page until the next key. Seen alone and in herdr.

### Warp

- Alone: ❌ the page is squashed, and scrolling is broken. Space, `j`, `b`, and the wheel change the percent but not the picture, and `k` leaves it garbled until `g` or `G`. Likely cause, not confirmed: Warp ignores `CSI 16t`, and its `TIOCGWINSZ` pixel size is in points (8x16 per cell, where the real cell is 16x32).
- herdr: ⚠️ only for `Cmd-c`, which doesn't reach mddog. Warp shows an OSC 52 "Allow clipboard writes" banner. `c` works. Each step fills in within 0.25 s, and `G` takes 0.2 s.
- tmux: refuses with the clear message.
- zellij: ⚠️ short run. It didn't garble or crash, unlike every other terminal. Splitting a pane drew the page offset and cut off.
- The TOC and status-bar leftovers seen in iTerm2 don't happen in Warp.

Cause of the zellij bug: zellij 0.45 re-encodes kitty images itself. It caches crops by cell size only, so different crops collide ([zellij#5573](https://github.com/zellij-org/zellij/issues/5573), fixed on main by [#5623](https://github.com/zellij-org/zellij/pull/5623), unreleased), and it leaves the old copy on screen when it replaces one ([#5648](https://github.com/zellij-org/zellij/pull/5648), open).

## Fixes after the first pass

- `33d2779` clears the screen when the size or the sidebar width changes. In iTerm2, alone and in herdr, the TOC text after Esc and the status-bar copies after a resize are gone. Ghostty and kitty show no extra blank frames.
- `7773209` scales the cell size when the terminal reports its window in points. Warp now lays the page out at the right size. Scrolling in Warp is still broken: a hand-written kitty test shows Warp ignores the source rect on `a=p`, so it draws the whole tile and doesn't move it. That's Warp's bug.
- A recheck of Warp + herdr garbled when scrolling with both the old and new binary, which contradicts the first Warp + herdr run.

- `a94e824` makes mddog refuse zellij (the `ZELLIJ` variable) and Warp (`TERM_PROGRAM=WarpTerminal`) with a message, as it does for tmux. A multiplexer inside Warp hides Warp's name, so mddog still runs there and garbles.

## Needs a manual check

1. Trackpad inertia scrolling in herdr + iTerm2.
2. Lag by feel in herdr + WezTerm and herdr + iTerm2. Agents measured it, but only you can say if it bothers you.
3. The TOC and status-bar fix with your own iTerm2 profile, since the agents used the default one.

## Manual check results (2026-10-24)

1. herdr + iTerm2: trackpad scrolling feels odd. Outside herdr, iTerm2 scrolling feels good.
2. herdr + WezTerm feels less laggy than herdr + iTerm2.
3. `t` pressed twice left the TOC on screen. That run used `mdd` from `3cbe73e`, before the fix in `33d2779`. A retest with the current build passed: `t` twice, `t` then Esc, and a resize all leave the page clean.

