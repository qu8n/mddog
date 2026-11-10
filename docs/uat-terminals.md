# Terminal UAT

Manual tests of mddog in each terminal, alone and inside a multiplexer. The results feed the [Compatibility](https://mddog.pages.dev/docs/compatibility) page of the docs.

Build under test: `7699b95`, run as `target/release/mdd` from a worktree, first on `PATH`. Tested on 2026-11-07 on a Retina Mac with macOS 27.2. Agents drove the terminals with AppleScript keys and CoreGraphics mouse events, and checked screenshots.

## Matrix

Status: ✅ works, ⚠️ works with a limitation, ❌ broken, 🚫 mddog refuses with a clear message and exits cleanly, ➖ skipped.

| Terminal | Version | Alone | herdr 0.9.5 | tmux 3.8 | zellij 0.45.1 |
| --- | --- | --- | --- | --- | --- |
| Ghostty | 1.3.2-main+22391ed64 | ✅ | ✅ | 🚫 | 🚫 |
| kitty | 0.49.2 | ✅ | ✅ | 🚫 | 🚫 |
| WezTerm | 20240203-110809-5046fc22 | ⚠️ | ⚠️ | 🚫 | 🚫 |
| iTerm2 | 3.7.20261106-nightly | ⚠️ | ⚠️ | 🚫 | 🚫 |
| Warp | 0.2026.11.04.08.17.01 | 🚫 | ❌ | 🚫 | 🚫 |
| Rex | Beta 0.1.0 (1853) | ✅ | ➖ | ➖ | ➖ |

The last round marked tmux ✅ because the refusal was the expected result. tmux isn't supported, so this round marks it 🚫.

## Checks

Run these in each cell, from the repo root. Note anything odd, even if the check passes. Checks 7, 8 and 11 are new in this round, because the build added local links, zoom with the terminal font, and a clear message for `mdd -`. Check 3 now covers the relative SVG image, and check 6 checks the clipboard with `pbpaste`.

1. **File list.** `mdd`. The list draws, typing filters it, Enter opens a file.
2. **First paint.** `mdd README.md`. Fonts, tables, and code colors look right. The page paints in under a second.
3. **Rich content.** `mdd fixtures/gfm.md`. Math, Mermaid, alerts, and the relative image `diagram.svg` draw.
4. **Scroll.** `j` `k`, `Space` `b`, `g` `G`, and the mouse wheel, many times down and then many times up, paced and in a fast burst. No blank or stale tiles, no tearing, no lag.
5. **Overlays.** `?` help, `t` table of contents (also `t` twice fast), `/` search with `n` `N`. Each opens and closes cleanly.
6. **Mouse.** Click a heading link. Drag to select, then `Cmd-c`. Check the clipboard with `pbpaste`. With `copy_on_select = true`, the drag alone copies.
7. **Local links.** In `README.md`, click the `docs/themes.md` link. The doc opens, and `Backspace` returns to the same place. A link to a non-Markdown file shows "Not a Markdown file".
8. **Zoom.** `Cmd-+` twice, then `Cmd--` twice. The page lays out again at each font size, with no stale tiles.
9. **Resize.** Resize the window (and the pane, in a multiplexer). The page lays out again at the new width, with no leftover TOC or status bar.
10. **Edit and reload.** `e`, change a line, save, quit the editor. The preview shows the change. Append to the file from another shell too, to check live reload.
11. **Standard input.** `cat README.md | mdd` paints the page. `mdd -` says it waits for typed input. `Ctrl-D` on empty input exits 2 with usage, and `Ctrl-C` exits 130.
12. **Quit.** `q`. The shell comes back clean, with no leftover images.
13. **Panes and tabs** (multiplexer only). Split the pane, switch tabs, and zoom. Images stay in mddog's pane and come back when you return.

In a 🚫 cell, run `mdd README.md`, `mdd`, and `cat README.md | mdd`. Each must print the refusal and exit 1, and leave the shell clean. In tmux, also try `set -g allow-passthrough on`.

The refusal messages (reworded after this round, see [Bugs](#bugs)):

- tmux: "mdd: mddog doesn't support tmux yet, because tmux doesn't pass its images through. Run mdd outside tmux."
- zellij: "mdd: mddog doesn't support zellij yet, because zellij 0.45 garbles scrolled images (zellij-org/zellij#5573 and #5648). Run mdd outside zellij."
- Warp: "mdd: mddog doesn't support Warp, because Warp can't crop images, and mddog crops images to scroll them. Use Ghostty, Kitty, WezTerm, iTerm2, or Rex."

In tmux, also check that `tmux display -p '#{pane_title}'` doesn't show `Gi=`.

## Results

### Ghostty

- Alone: all 12 checks pass. README paints in about 0.3 s, and `G` settles in about 0.5 s. The slow first `G` from the last round (2 to 3 s) is gone.
- herdr: all 13 checks pass. README paints in steps: the large image is cut off at 0.5 s and complete by 1.2 s. A tab switch repaints in under 0.4 s.
- tmux and zellij: refuse as expected. `allow-passthrough` doesn't change anything.

### kitty

- Alone: all 12 checks pass. README paints in about 0.4 s, and `G` settles in about 0.4 s. `Backspace` after a local link returns a frame identical to the one before the click.
- herdr: all 13 checks pass. README is complete by 1 s, and `G` settles in about 0.2 s.
- tmux and zellij: refuse as expected.

### WezTerm

- Alone: ⚠️ `Cmd-c` empties the clipboard. WezTerm's default `Cmd-c` binding takes the key and copies its own empty selection, so it overwrites what mddog copied on release. The README's fix (`enable_kitty_keyboard = true`, and remove the `Cmd-c` binding) wasn't tested. Everything else passes, and `G` settles in about 1 s.
- herdr: ⚠️ the same `Cmd-c` problem. Scrolling is clean, `G` settles in about 0.5 s, and one wheel step fills within 0.2 s. The 0.4 s late fill from the last round didn't come back.
- tmux and zellij: refuse as expected. WezTerm no longer crashes, because mddog refuses zellij before it draws.

### iTerm2

- Alone: ⚠️ `Cmd-c` doesn't reach mddog, as the README says, so it copies nothing. Drag with `copy_on_select = true` works. The rest passes, `G` settles in under 0.5 s, and `t` then Esc, `t` twice, and a resize don't leave anything behind. Synthetic wheel events reached iTerm2 in this round, and scrolling with them was clean.
- herdr: ⚠️ slow, and `Cmd-c` shows iTerm2's "mouse reporting has prevented making a selection" banner, which stays up. README takes about 3.5 s to paint, `G` takes 1.5 to 2 s, and returning from a local link takes 3.5 to 5.5 s. This is herdr sending iTerm2 every image as inline base64 ([herdr#5512](https://github.com/herdrdev/herdr/issues/5512)). Panes, tabs, and zoom work.
- tmux and zellij: refuse as expected.

### Warp

- Alone: refuses as expected. No onboarding or sign-in was needed.
- herdr: ❌ herdr hides Warp's name, so mddog runs, and scrolling is broken. `j`, `Space`, and the wheel change the percent but not the picture. `G` draws the end of the file squashed into the top third, with "The fixture ends here." three times. The page is also stretched sideways from the first paint, and a pane split lays it out squashed. The file list, overlays, `q`, and pane zoom work. The other checks weren't run after scrolling failed.
- tmux and zellij: refuse as expected.

### Rex

New in this round. Rex reports `TERM_PROGRAM=rex` and speaks the kitty graphics protocol. Rex has its own panes and session persistence, so herdr, tmux and zellij were skipped there.

- Alone: all 12 checks pass, including `Cmd-c`. `G` settles in under 1 s.
- Rex's own panes and tabs: a split lays mddog out again at the narrower width, with the image inside its pane. A new tab doesn't show any image from mddog's tab. Zoom Pane couldn't be tested. Rex's pane focus moved to the other pane after a tab closed, which is a Rex quirk.

## Bugs

1. **Fixed after this round.** **The graphics probe leaks into the tmux pane title.** In every terminal, after `mdd` refuses in tmux, the status bar shows `"Gi=4003,s=1,v=1,a=q,t...` until something else sets the title. tmux takes the probe's APC sequence (`ESC _ G...`) as a pane title, and mddog sends the probe before it refuses. Repro: `tmux new`, `mdd README.md`, then `tmux display -p '#{pane_title}'`. `allow-passthrough on` doesn't change it.
2. **Warp inside herdr isn't refused.** Inside herdr, `TERM_PROGRAM` is `herdr`, so the Warp check misses it, and the page garbles as described under Warp. Repro: in Warp, `herdr`, then `mdd fixtures/gfm.md` and `G`. Not fixable from the environment: a herdr session keeps the `WARP_*` variables of the terminal it first started in, even in panes opened later from another terminal.
3. **Fixed after this round.** **The tmux message blames the terminal.** It says "this terminal can't show images" in Ghostty and kitty, which can. tmux is named only in the last sentence.

## Known causes

- zellij 0.45 re-encodes kitty images itself. It caches crops by cell size only, so different crops collide ([zellij#5573](https://github.com/zellij-org/zellij/issues/5573), fixed on main by [#5623](https://github.com/zellij-org/zellij/pull/5623), unreleased), and it leaves the old copy on screen when it replaces one ([#5648](https://github.com/zellij-org/zellij/pull/5648), open). Before mddog refused zellij, this garbled every terminal and crashed WezTerm ("Failed to allocate ~11M quads").
- Warp ignores the source rectangle on a kitty `a=p` placement, so it draws the whole tile and never moves it. mddog scrolls by cropping tiles, so the page can't scroll there.
- herdr sends images to the outer terminal as temp files only for Ghostty. Every other terminal gets inline base64, about 11 MB per tile, which iTerm2 takes about 700 ms to decode ([herdr#5512](https://github.com/herdrdev/herdr/issues/5512)).

## Needs a manual check

1. Trackpad inertia scrolling in herdr + iTerm2. In the last round you found it odd there and fine in iTerm2 alone.
2. Lag by feel in herdr + iTerm2 on this build.
3. WezTerm with `enable_kitty_keyboard = true` and its `Cmd-c` binding removed, to confirm the README's fix.
