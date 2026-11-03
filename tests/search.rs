#![expect(clippy::unwrap_used, reason = "a failed step should fail the test")]

//! Search, links, anchors, selection, hover, presses and wide blocks, through the engine calls the viewer makes.
//! Points on the page come from search rects and the layout, so the tests don't depend on exact pixels.
//! `docs/behavior.md` names these tests.

use mddog::engine::{Engine, Pointer, Press, Rect, Select};

fn page(src: &str, width: f64) -> Engine {
    let e = Engine::new(width, 2.0);
    e.load(src, std::path::Path::new("."), false, mddog::theme::builtin("github-dark").unwrap());
    e
}

fn center(r: Rect) -> (f64, f64) {
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}

/// The center of the first match of `text`, with the search cleared again.
fn at(e: &Engine, text: &str) -> (f64, f64) {
    let m = e.search(text, None);
    e.search_clear();
    center(m.first().unwrap_or_else(|| panic!("{text:?} is not on the page"))[0])
}

fn count(src: &str, width: f64, query: &str) -> usize {
    page(src, width).search(query, None).len()
}

fn pixels(e: &Engine) -> Vec<u8> {
    e.snapshot(0.0, 300.0, 1.0).unwrap().rgba.to_vec()
}

#[test]
fn s15_spans_formatting() {
    assert_eq!(count("say hello **world** now\n", 600.0, "hello world"), 1);
}

#[test]
fn s14_case_insensitive() {
    assert_eq!(count("Apple apple APPLE ap.ple\n", 600.0, "apple"), 3);
}

#[test]
fn v6_smart_case() {
    assert_eq!(count("Apple apple APPLE\n", 600.0, "Apple"), 1);
}

#[test]
fn s14_literal_not_regex() {
    assert_eq!(count("a.b axb\n", 600.0, "a.b"), 1);
}

#[test]
fn s14_untrimmed_query() {
    assert_eq!(count("one two\n", 600.0, " two"), 1);
}

#[test]
fn d1_every_occurrence_in_page() {
    assert_eq!(count("# Title\n\nTitle again in text.\n\n| a | b |\n|---|---|\n| title | x |\n", 600.0, "title"), 3);
}

#[test]
fn no_match_across_blocks() {
    assert_eq!(count("first para ends\n\nwith next para\n", 600.0, "ends with"), 0);
    assert_eq!(count("| a | b |\n|---|---|\n| one | two |\n", 600.0, "one two"), 0);
}

#[test]
fn d2_across_soft_break() {
    assert_eq!(count("It contains the compiler,\nstandard library\n", 600.0, "compiler, standard"), 1);
}

#[test]
fn d2_across_wrap() {
    let words = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua";
    let m = page(&format!("{words} {words} {words}\n"), 200.0).search("magna aliqua lorem ipsum", None);
    assert_eq!(m.len(), 2, "{m:?}");
    assert!(m.iter().all(|r| r.first().unwrap()[1] < r.last().unwrap()[1]), "a match that wraps has rects on both lines: {m:?}");
}

#[test]
fn the_current_match_moves_and_clears() {
    let e = page("beta alpha beta\n\nbeta\n", 600.0);
    assert_eq!(e.search("beta", Some(1)).len(), 3);
    // The old current match and the new one.
    assert_eq!(e.search_current(2).len(), 2);
    e.search_clear();
    assert!(e.search("nothing here", None).is_empty());
}

#[test]
fn v2_search_scrolls_a_wide_block() {
    let e = page(&format!("```\n{} needle\n```\n", "x".repeat(400)), 600.0);
    let before = e.code_positions();
    assert_eq!(e.search("needle", None).len(), 1);
    // The viewer marks the current match next, which scrolls its block and redraws it.
    let redraw = e.search_current(0);
    assert_ne!(e.code_positions(), before);
    assert!(redraw.iter().any(|r| r[2] >= 500.0), "the whole block redraws: {redraw:?}");
}

const LINKS: &str = "see [example](https://example.com/a) and https://auto.dev/x and [jump](#target) and [doc](docs/a.md)\n\n## Target\n";

fn link(e: &Engine, text: &str) -> Option<String> {
    let (x, y) = at(e, text);
    e.link(x, y).map(|l| l.1)
}

#[test]
fn m1_link_href() {
    let e = page(LINKS, 600.0);
    assert_eq!(link(&e, "example").as_deref(), Some("https://example.com/a"));
    assert_eq!(link(&e, "auto.dev").as_deref(), Some("https://auto.dev/x"));
    assert_eq!(link(&e, "see"), None);
}

#[test]
fn m1_anchor_link() {
    assert_eq!(link(&page(LINKS, 600.0), "jump").as_deref(), Some("#target"));
}

#[test]
fn m1_relative_link() {
    assert_eq!(link(&page(LINKS, 600.0), "doc").as_deref(), Some("docs/a.md"));
}

#[test]
fn m2_anchor_y() {
    let e = page(&format!("{}\n\n## Target Here\n\nend\n", "line\n\n".repeat(20)), 600.0);
    let want = e.layout().headings.iter().find(|h| h.1 == "Target Here").unwrap().2;
    let y = e.anchor_y("target-here").unwrap();
    assert!(y > 0.0 && (y - want).abs() < 1.0, "{y} vs {want}");
    assert_eq!(e.anchor_y("nope"), None);
}

#[test]
fn s0_half_pixel_tile_matches_whole() {
    // With an odd cell height at scale 2, every other tile starts on a half CSS pixel. Its pixels must be the
    // same rows as a snapshot from the whole pixel above, not a shifted or re-rasterized copy.
    let e = page("# Heading\n\nalpha beta gamma\n", 600.0);
    let whole = e.snapshot(10.0, 41.0, 2.0).unwrap();
    let row = whole.width * 4;
    let half = e.snapshot(10.5, 40.5, 2.0).unwrap();
    assert!(half.height == 81 && half.rgba[..] == whole.rgba[row..]);
    let ends_half = e.snapshot(10.0, 40.5, 2.0).unwrap();
    assert!(ends_half.height == 81 && ends_half.rgba[..] == whole.rgba[..81 * row]);
}

const PARAS: &str = "alpha beta gamma\n\nsecond para\n";

#[test]
fn m3_select_text() {
    let e = page(PARAS, 600.0);
    let (_, y) = at(&e, "beta");
    let before = pixels(&e);
    e.select(Select::Range(0.0, y, 5000.0, y));
    assert_eq!(e.selection_text(), "alpha beta gamma", "a drag past the edge stops at the line's end");
    assert_ne!(pixels(&e), before, "the selection is painted");
    let (n, rects) = e.select(Select::Clear);
    assert!(n > 0 && rects.len() == n && e.selection_text().is_empty());
    assert_eq!(pixels(&e), before, "the clear restores the pixels");
}

#[test]
fn m3_select_backwards() {
    let e = page(PARAS, 600.0);
    let (_, y) = at(&e, "beta");
    e.select(Select::Range(5000.0, y, 0.0, y));
    assert_eq!(e.selection_text(), "alpha beta gamma");
}

#[test]
fn m3_double_click_word() {
    let e = page(PARAS, 600.0);
    let (x, y) = at(&e, "beta");
    e.select(Select::Word(x, y));
    assert_eq!(e.selection_text().trim(), "beta");
}

const DETAILS: &str = "<details>\n<summary>More</summary>\n\nHidden body\n\n</details>\n\nplain\n";

#[test]
fn p1_no_press_on_text() {
    let e = page(DETAILS, 600.0);
    let (x, y) = at(&e, "plain");
    assert_eq!(e.press(x, y), Press::Nothing);
}

#[test]
fn p1_summary_opens() {
    let e = page(DETAILS, 600.0);
    assert!(e.search("hidden", None).is_empty(), "a closed body has nothing to find");
    let closed = e.layout().height;
    let (x, y) = at(&e, "More");
    assert_eq!(e.press(x, y), Press::Other);
    assert!(e.layout().height > closed);
    assert_eq!(e.search("hidden", None).len(), 1);
}

const CODE: &str = "```rust\nfn main() {}\n```\n\n```\nplain\n```\n\nsee [a link](https://example.com) here\n";

/// The code block's copy button, found by hovering along its top right.
fn copy_button(e: &Engine, text: &str) -> (f64, f64) {
    let (_, y) = at(e, text);
    // From the left, so the point lands 8 px inside the button's left edge.
    let found = (0..160).rev().map(|dx| (600.0 - f64::from(dx), y - 4.0)).find(|&(x, y)| e.hover(x, y).0 == Pointer::Hand);
    e.hover(-1.0, -1.0);
    let (x, y) = found.unwrap_or_else(|| panic!("no button by {text:?}"));
    (x + 8.0, y)
}

#[test]
fn p7_copy_code() {
    let e = page(CODE, 600.0);
    let (x, y) = copy_button(&e, "main");
    let Press::Copy { text, rects, label } = e.press(x, y) else { panic!("no copy") };
    assert_eq!((text.trim_end(), label.as_str(), rects.len()), ("fn main() {}", "the code", 1));
    // The check shows until it resets, once.
    assert_eq!(e.reset_copy().len(), 1);
    assert!(e.reset_copy().is_empty());
}

#[test]
fn p7_copy_plain_code() {
    let e = page(CODE, 600.0);
    let (x, y) = copy_button(&e, "plain");
    assert!(matches!(e.press(x, y), Press::Copy { text, .. } if text.trim_end() == "plain"));
}

const DIAGRAM: &str = "```mermaid\ngraph LR\n  A --> B\n```\n\nafter\n";
/// Wider than a 600 px page, so it draws shrunk with a zoom bar.
const WIDE: &str = "```mermaid\nflowchart LR\n  A[Start here] --> B[Parse the input file] --> C[Build the syntax tree] --> D[Lay out every block] --> E[Draw the tiles] --> F[Send frames to the terminal] --> G[Done and waiting]\n```\n\nafter\n";

/// The buttons along row `y`, right to left: the centers of the runs of the hand pointer.
fn buttons_at(e: &Engine, y: f64) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = vec![];
    let mut start = None;
    for x in (0..600).rev().map(f64::from) {
        match (e.hover(x, y).0 == Pointer::Hand, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                out.push((f64::midpoint(s, x), y));
                start = None;
            }
            _ => {}
        }
    }
    e.hover(-1.0, -1.0);
    out
}

/// The diagram's top buttons, right to left: copy, expand.
fn diagram_buttons(e: &Engine) -> Vec<(f64, f64)> {
    buttons_at(e, e.layout().blocks[0].top + 25.0)
}

/// The zoom bar's buttons, right to left, that take a click: 15 px up from its bottom, 16 px above the block's end.
fn zoom_buttons(e: &Engine) -> Vec<(f64, f64)> {
    let b = e.layout().blocks[0];
    buttons_at(e, b.top + b.height - 31.0)
}

#[test]
fn p3_copy_mermaid_source() {
    let e = page(DIAGRAM, 600.0);
    let b = diagram_buttons(&e);
    assert!(!b.is_empty(), "no diagram buttons");
    assert!(matches!(e.press(b[0].0, b[0].1), Press::Copy { text, label, .. } if text == "graph LR\n  A --> B" && label == "the diagram source"));
}

#[test]
fn p6_expand_button_expands() {
    let e = page(DIAGRAM, 600.0);
    let b = diagram_buttons(&e);
    assert_eq!(e.press(b[1].0, b[1].1), Press::Expand(0));
}

#[test]
fn p4_zoom_shows_only_on_a_shrunk_diagram() {
    // A diagram that fits gets copy and expand, as pele drew it. Its block ends 16 px under the drawing.
    let e = page(DIAGRAM, 600.0);
    assert_eq!(diagram_buttons(&e).len(), 2);
    assert!(zoom_buttons(&e).is_empty(), "a diagram that fits has no zoom bar");
    // A shrunk one gets a bar too. Zoom out is off at the fit size, and reset shows only once zoomed.
    let e = page(WIDE, 600.0);
    assert_eq!(diagram_buttons(&e).len(), 2);
    let z = zoom_buttons(&e);
    assert_eq!(z.len(), 1, "only zoom in takes a click at the fit size: {z:?}");
    assert!(matches!(e.press(z[0].0, z[0].1), Press::Redraw(r) if !r.is_empty()));
    assert_eq!(zoom_buttons(&e).len(), 3, "zoom in, zoom out and reset once zoomed");
}

#[test]
fn p4_zoom_keeps_the_block_and_reset_gives_back_the_fit() {
    let e = page(WIDE, 600.0);
    let (before, block) = (pixels(&e), e.layout().blocks[0]);
    let z = zoom_buttons(&e);
    let Press::Redraw(r) = e.press(z[0].0, z[0].1) else { panic!("zoom in redraws in place") };
    // The frame: the block less its 16 px padding above and below and 48 px on the right.
    assert_eq!(r, vec![[24.0, block.top + 16.0, 504.0, block.height - 32.0]]);
    assert_eq!(e.layout().blocks[0], block, "the block keeps its place and height");
    let zoomed = pixels(&e);
    assert_ne!(zoomed, before);
    // Right to left once zoomed: zoom out, zoom in, reset. Zoom out from one step lands on the fit size.
    let z = zoom_buttons(&e);
    assert!(matches!(e.press(z[0].0, z[0].1), Press::Redraw(_)));
    assert_eq!(pixels(&e), before);
    let z = zoom_buttons(&e);
    e.press(z[0].0, z[0].1);
    let z = zoom_buttons(&e);
    e.press(z[1].0, z[1].1);
    let z = zoom_buttons(&e);
    assert!(matches!(e.press(z[2].0, z[2].1), Press::Redraw(_)), "reset");
    assert_eq!(pixels(&e), before);
    assert_eq!(zoom_buttons(&e).len(), 1);
}

#[test]
fn p4_zoom_buttons_stay_put_when_reset_shows() {
    // Reset joins at the bar's left end, so a second click on the same spot zooms in again.
    let e = page(WIDE, 600.0);
    let z = zoom_buttons(&e)[0];
    e.press(z.0, z.1);
    let once = pixels(&e);
    assert!(matches!(e.press(z.0, z.1), Press::Redraw(_)));
    assert_ne!(pixels(&e), once, "the second click zooms in further");
    let now = zoom_buttons(&e);
    assert_eq!(now[1], z, "zoom in keeps its place");
}

#[test]
fn p4_the_first_zoom_keeps_the_top_row() {
    // A short, wide diagram zooms from its top center, so its top margin grows and no node is cut at the
    // frame's top edge.
    let src = WIDE.replace("[Done and waiting]", "[Done and waiting]\n  B --> H[Report an error]\n  B --> I[Log it]\n  B --> J[Retry later]");
    let e = page(&src, 600.0);
    let top = e.layout().blocks[0].top + 16.0;
    let first = |e: &Engine| {
        let s = e.snapshot(top, 40.0, 2.0).unwrap();
        // All but the right 120 px, which hold the copy and expand buttons.
        let w = s.width * 4;
        let half = |r: usize| &s.rgba[r * w..(r + 1) * w - 960];
        (0..s.height).find(|&r| half(r) != half(0)).unwrap_or(s.height)
    };
    let fit = first(&e);
    assert!(fit > 0 && fit < 80, "the diagram starts inside the strip: {fit}");
    let z = zoom_buttons(&e)[0];
    e.press(z.0, z.1);
    // Zoomed from the top, the margin above the top row grows with the zoom.
    assert!(first(&e) > fit, "the top row moved up out of the frame: {} <= {fit}", first(&e));
}

#[test]
fn p4_zoom_stops_at_three_times() {
    let e = page(WIDE, 600.0);
    let mut steps = 0;
    // Zoom in is alone at the fit size, then second from the right until it goes off at the limit.
    while let Some(&z) = zoom_buttons(&e).get(usize::from(steps > 0))
        && (steps == 0 || zoom_buttons(&e).len() == 3)
    {
        assert!(matches!(e.press(z.0, z.1), Press::Redraw(_)));
        steps += 1;
        assert!(steps < 20, "zoom in never stops");
    }
    // From well under 1 to 3 CSS px per SVG unit at 1.5625 a step, then zoom in is off.
    assert!(steps >= 3, "{steps}");
    assert_eq!(zoom_buttons(&e).len(), 2, "zoom out and reset, with zoom in off");
}

#[test]
fn p4_drag_pans_a_zoomed_diagram_and_stops_at_its_edges() {
    let e = page(WIDE, 600.0);
    let block = e.layout().blocks[0];
    let (x, y) = (200.0, block.top + 40.0);
    // Not zoomed: a press there is no grab, so it selects as usual.
    assert!(e.code_grab(x, y, 8.0).is_none());
    let z = zoom_buttons(&e);
    e.press(z[0].0, z[0].1);
    let start = pixels(&e);
    assert_eq!(e.code_grab(x, y, 8.0), Some(vec![]), "a press in the zoomed frame takes hold");
    let moved = e.code_drag(x - 60.0, y);
    assert_eq!(moved.len(), 1, "a drag pans and redraws the frame");
    assert_ne!(pixels(&e), start);
    // The view stops at the diagram's left edge: dragging right past it moves no further.
    assert!(!e.code_drag(x + 2000.0, y).is_empty());
    assert!(e.code_drag(x + 4000.0, y).is_empty(), "panned past the left edge");
    e.code_release();
    assert_eq!(e.layout().blocks[0], block);
    // A sideways wheel pans too, and stops at the right edge.
    assert!(!e.code_scroll(x, y, 40.0).is_empty());
    assert!(!e.code_scroll(x, y, 100_000.0).is_empty());
    assert!(e.code_scroll(x, y, 40.0).is_empty(), "panned past the right edge");
    // A press on the bar's buttons is a click, not a grab.
    let z = zoom_buttons(&e);
    assert!(e.code_grab(z[0].0, z[0].1, 8.0).is_none());
}

#[test]
fn p4_a_narrow_page_shrinks_a_diagram_without_a_zoom_bar() {
    // 120 px leaves 72 for the block: too little for the bar's 48 px gap and its 82 px.
    let e = page(WIDE, 120.0);
    let b = e.layout().blocks[0];
    assert!(b.height > 32.0, "the diagram still draws");
    assert!(zoom_buttons(&e).is_empty());
}

#[test]
fn p4_a_details_toggle_above_drops_the_zoom() {
    let src = format!("<details><summary>More</summary>\n\n{WIDE}\n</details>\n\n{WIDE}");
    let e = page(&src, 600.0);
    let fit = pixels(&e);
    let blocks = e.layout().blocks;
    let b = blocks[blocks.len() - 2];
    let z = buttons_at(&e, b.top + b.height - 31.0);
    assert!(matches!(e.press(z[0].0, z[0].1), Press::Redraw(_)));
    // Opening the summary adds a diagram before the zoomed one, so no view carries over to the wrong one.
    assert_eq!(e.press(40.0, e.layout().blocks[0].top + 10.0), Press::Other);
    assert_eq!(e.press(40.0, e.layout().blocks[0].top + 10.0), Press::Other);
    assert_eq!(pixels(&e), fit, "closed again, the page is at its fit view");
}

#[test]
fn p6_expand_after_zoom_shows_the_zoomed_part() {
    let e = page(WIDE, 600.0);
    e.expand(0, 0.0, 400.0);
    let whole = pixels(&e);
    e.collapse();
    let z = zoom_buttons(&e);
    e.press(z[0].0, z[0].1);
    e.expand(0, 0.0, 400.0);
    assert_ne!(pixels(&e), whole, "the expanded view shows the zoomed part, as pele's view box did");
}

#[test]
fn h1_hover_button_redraws_it() {
    let e = page(CODE, 600.0);
    let (x, y) = copy_button(&e, "main");
    let idle = pixels(&e);
    let (shape, rects) = e.hover(x, y);
    assert_eq!((shape, rects.len()), (Pointer::Hand, 1));
    assert_ne!(pixels(&e), idle, "the button shows its hover look and tooltip");
}

#[test]
fn h4_same_target_no_redraw() {
    let e = page(CODE, 600.0);
    let (x, y) = copy_button(&e, "main");
    e.hover(x, y);
    assert_eq!(e.hover(x - 1.0, y), (Pointer::Hand, vec![]));
}

#[test]
fn h5_leaving_clears_and_redraws() {
    let e = page(CODE, 600.0);
    let (x, y) = copy_button(&e, "main");
    let idle = pixels(&e);
    e.hover(x, y);
    let (cx, cy) = at(&e, "main");
    assert_eq!(e.hover(cx, cy).1.len(), 1);
    assert_eq!(pixels(&e), idle);
}

#[test]
fn h8_copied_wins_after_leaving() {
    let e = page(CODE, 600.0);
    let (x, y) = copy_button(&e, "main");
    let idle = pixels(&e);
    e.hover(x, y);
    e.press(x, y);
    e.hover(-1.0, -1.0);
    assert_ne!(pixels(&e), idle, "the check stays after the pointer leaves");
    e.reset_copy();
    assert_eq!(pixels(&e), idle);
}

#[test]
fn h6_link_is_pointer() {
    let e = page(CODE, 600.0);
    let (x, y) = at(&e, "a link");
    assert_eq!(e.hover(x, y).0, Pointer::Hand);
    let (x, y) = at(&e, "main");
    assert_eq!(e.hover(x, y).0, Pointer::Text);
}

#[test]
fn h7_blank_is_default() {
    let e = page(CODE, 600.0);
    let (_, y) = at(&e, "here");
    assert_eq!(e.hover(590.0, y), (Pointer::Default, vec![]));
}

#[test]
fn h7_off_page_is_default() {
    assert_eq!(page(CODE, 600.0).hover(-1.0, -1.0), (Pointer::Default, vec![]));
}

fn wide(rows: usize) -> String {
    format!("```\n{}\n```\n\nshort\n\n```\nfits\n```\n", vec!["x".repeat(400); rows].join("\n"))
}

/// The first wide block's region, and its scroll bar's middle row.
fn bar(e: &Engine) -> (Rect, f64) {
    let code = e.layout().blocks[0];
    let r = e.code_scroll(300.0, code.top + code.height / 2.0, 1.0);
    e.code_scroll(300.0, code.top + code.height / 2.0, -1.0);
    // The region runs to 8 px under the top of the 6 px bar.
    (r[0], r[0][1] + r[0][3] - 5.0)
}

#[test]
fn c1_wide_block_gets_a_bar() {
    let e = page(&wide(1), 600.0);
    let (_, by) = bar(&e);
    assert!(e.code_grab(300.0, by, 0.0).is_some());
    e.code_release();
    let (x, y) = at(&e, "fits");
    assert!(e.code_grab(x, y, 8.0).is_none(), "a block that fits has no bar");
}

#[test]
fn c2_scroll_moves_code() {
    let e = page(&wide(1), 600.0);
    let before = pixels(&e);
    let code = e.layout().blocks[0];
    assert_eq!(e.code_scroll(300.0, code.top + code.height / 2.0, 50.0).len(), 1);
    assert_eq!(e.code_positions(), [100.0], "50 CSS px at scale 2");
    assert_ne!(pixels(&e), before, "the scroll repaints");
}

#[test]
fn c2_short_block_does_not_scroll() {
    let e = page(&wide(1), 600.0);
    let (x, y) = at(&e, "fits");
    assert!(e.code_scroll(x, y, 50.0).is_empty());
    let (x, y) = at(&e, "short");
    assert!(e.code_scroll(x, y, 50.0).is_empty(), "text doesn't scroll");
}

#[test]
fn c2_scroll_stops_at_end() {
    let e = page(&wide(1), 600.0);
    let code = e.layout().blocks[0];
    let (x, y) = (300.0, code.top + code.height / 2.0);
    assert!(!e.code_scroll(x, y, 100_000.0).is_empty());
    assert!(e.code_scroll(x, y, 50.0).is_empty());
}

#[test]
fn c2_scroll_below_view() {
    let e = page(&format!("{}{}", "para\n\n".repeat(80), wide(1)), 600.0);
    let code = e.layout().blocks[80];
    assert!(code.top > 2000.0);
    assert!(!e.code_scroll(300.0, code.top + code.height / 2.0, 50.0).is_empty());
}

#[test]
fn c3_wrap_removes_bar() {
    let e = page(&wide(1), 600.0);
    let (_, by) = bar(&e);
    e.set_wrap(true);
    assert!(e.code_grab(300.0, by, 0.0).is_none());
    e.set_wrap(false);
    let (_, by) = bar(&e);
    assert!(e.code_grab(300.0, by, 0.0).is_some(), "unwrapping gives the bar back");
}

#[test]
fn c4_grab_misses_code() {
    let e = page(&wide(3), 600.0);
    let code = e.layout().blocks[0];
    assert!(e.code_grab(300.0, code.top + 10.0, 8.0).is_none());
}

#[test]
fn c4_grab_thumb_stays() {
    let e = page(&wide(1), 600.0);
    let (r, by) = bar(&e);
    // The thumb starts at the track's left end. A press off its center doesn't move it.
    assert!(e.code_grab(r[0] + 8.0, by, 0.0).is_some());
    assert_eq!(e.code_positions(), [0.0]);
    // A drag keeps the same point of the thumb under the pointer.
    e.code_drag(r[0] + 18.0, by);
    let moved: f64 = f64::from(e.code_positions()[0]);
    e.code_drag(r[0] + 8.0, by);
    assert!(moved > 0.0);
    assert_eq!(e.code_positions(), [0.0]);
}

#[test]
fn c4_grab_track_jumps_and_drag_follows() {
    let e = page(&wide(1), 600.0);
    let (r, by) = bar(&e);
    let end = r[0] + r[2] - 6.0;
    assert!(e.code_grab(end, by, 0.0).is_some());
    let max: f64 = f64::from(e.code_positions()[0]);
    assert!(max > 0.0, "a press at the track's end goes to the end");
    assert_eq!(e.code_drag(r[0], by).len(), 1);
    assert_eq!(e.code_positions(), [0.0], "a drag to the start goes to the start");
    e.code_release();
    assert!(e.code_drag(end, by).is_empty(), "a release lets go");
    assert_eq!(e.code_positions(), [0.0]);
}

const WIDE_TABLE: &str = "```\nxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n```\n\n| a | b |\n|---|---|\n| yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy | zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz |\n";

#[test]
fn w1_wide_table_scrolls() {
    let e = page(WIDE_TABLE, 400.0);
    let (x, y) = at(&e, "yyyy");
    assert_eq!(e.code_scroll(x, y, 40.0).len(), 1);
}

#[test]
fn w2_scroll_view_moves_both() {
    let e = page(WIDE_TABLE, 400.0);
    assert_eq!(e.code_scroll_view(0.0, e.layout().height, 40.0).len(), 2);
    assert_eq!(e.code_positions(), [80.0, 80.0]);
}

#[test]
fn w2_scroll_view_misses_range() {
    assert!(page(WIDE_TABLE, 400.0).code_scroll_view(100_000.0, 200_000.0, 40.0).is_empty());
}

#[test]
fn w3_positions_round_trip() {
    let e = page(WIDE_TABLE, 400.0);
    e.code_scroll_view(0.0, e.layout().height, 40.0);
    let again = page(WIDE_TABLE, 400.0);
    again.code_restore(&e.code_positions());
    assert_eq!(again.code_positions(), e.code_positions());
}
