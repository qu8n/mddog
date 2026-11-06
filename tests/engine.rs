//! Regressions in the engine's answers to the viewer's calls.

use mddog::render::engine::{Engine, Pointer, Press, Select};

fn engine(src: &str) -> Engine {
    let e = Engine::new(640.0, 2.0, false);
    e.load(src, std::path::Path::new("."), false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()));
    e
}

#[test]
fn keys_scroll_a_wide_code_block_sideways() {
    let e = engine(&format!("```\n{}\n```\n", "x".repeat(400)));
    assert!(!e.code_scroll_view(0.0, 400.0, 80.0).is_empty(), "the visible wide block didn't move");
    let saved = e.code_positions();
    assert_ne!(saved, [0.0]);
    let back = engine(&format!("```\n{}\n```\n", "x".repeat(400)));
    back.code_restore(&saved);
    assert_eq!(back.code_positions(), saved);
}

#[test]
fn a_rescale_keeps_a_wide_block_on_the_same_text() {
    let wide = format!("```\n{}\n```\n", "x".repeat(400));
    // Scrolled 80 CSS px in, the block must still be 80 CSS px in at any scale: 80 back reaches the left edge, and no further.
    let at_left = |e: &Engine| !e.code_scroll_view(0.0, 400.0, -80.0).is_empty() && e.code_scroll_view(0.0, 400.0, -1.0).is_empty();
    let e = engine(&wide);
    e.code_scroll_view(0.0, 400.0, 80.0);
    let saved = e.code_positions();
    e.rescale(1.0, 640.0);
    assert!(at_left(&e), "a font zoom moved the block sideways");
    let small = Engine::new(640.0, 1.0, false);
    small.load(&wide, std::path::Path::new("."), false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()));
    small.code_restore(&saved);
    assert!(at_left(&small), "a reload at another scale moved the block sideways");
}

#[test]
fn a_relayout_forgets_button_indices() {
    // A copy button inside an open <details>, which a toggle removes.
    let e = engine("<details open><summary>More</summary>\n\n```\ncode\n```\n\n</details>\n");
    let bottom = e.layout().height;
    // Find and press the copy button from the bottom up (the summary spans the width), then the summary.
    let mut copied = false;
    'scan: for y in (0..bottom as i32).rev().step_by(4) {
        for x in (560..640).rev().step_by(4) {
            if let Press::Copy { text, .. } = e.press(f64::from(x), f64::from(y)) {
                assert_eq!(text.trim_end(), "code");
                copied = true;
                break 'scan;
            }
        }
    }
    assert!(copied, "no copy button found");
    let closed = (0..40).step_by(2).any(|y| e.press(40.0, f64::from(y)) == Press::Other);
    assert!(closed, "the summary didn't toggle");
    // The copied button went with the old page, so there is nothing to reset, and a hover there finds no button.
    assert_eq!(e.reset_copy(), Vec::<[f64; 4]>::new());
    assert_eq!(e.hover(600.0, 30.0).1, Vec::<[f64; 4]>::new());
}

#[test]
fn an_empty_file_says_so() {
    let e = engine("  \n");
    assert!(e.layout().texts.iter().any(|t| t.contains("This file is empty")));
}

#[test]
fn search_select_and_hover_answer_typed() {
    let e = engine("# Title\n\nalpha beta gamma beta\n");
    let found = e.search("beta", None);
    assert_eq!(found.len(), 2);
    assert!(!e.search_current(1).is_empty());
    e.search_clear();
    let l = e.layout();
    let para = l.blocks[1];
    let y = para.top + para.height / 2.0;
    let (n, rects) = e.select(Select::Range(0.0, y, 2000.0, y));
    assert_eq!(n, 0);
    assert!(!rects.is_empty());
    assert_eq!(e.selection_text(), "alpha beta gamma beta");
    assert!(e.has_selection());
    let (n, _) = e.select(Select::Clear);
    assert!(n > 0, "the clear reports the old selection's rects");
    assert!(!e.has_selection());
    assert_eq!(e.hover(-1.0, -1.0).0, Pointer::Default);
    assert_eq!(e.hover(40.0, y).0, Pointer::Text);
    assert!(e.anchor_y("title").is_some());
    assert!(e.anchor_y("nope").is_none());
}

#[test]
fn a_diagram_expands_over_the_view_and_collapses() {
    let e = engine("# Top\n\n```mermaid\ngraph LR\n  A --> B\n```\n\nafter\n");
    let shot = |e: &Engine| e.snapshot(0.0, 200.0, 2.0, |n| Some(vec![0; n])).unwrap().rgba;
    let before = shot(&e);
    // The box stops at the page's end.
    assert_eq!(e.expand(0, 100.0, 900.0)[0][3].round(), (e.layout().height - 100.0).round());
    e.collapse();
    assert!(e.expand(5, 0.0, 200.0).is_empty(), "no diagram 5");
    let shown = e.expand(0, 0.0, 200.0);
    assert_eq!(shown, vec![[0.0, 0.0, 640.0, 200.0]]);
    let expanded = shot(&e);
    assert_ne!(expanded, before);
    // Expanding again replaces the box, and collapsing gives back the page.
    assert_eq!(e.expand(0, 0.0, 200.0).len(), 2);
    assert_eq!(e.collapse(), vec![[0.0, 0.0, 640.0, 200.0]]);
    assert_eq!(shot(&e), before);
    assert!(e.collapse().is_empty());
}

#[test]
fn a_selection_across_words_is_one_span_per_line() {
    let e = engine("alpha beta gamma beta\n");
    let b = e.layout().blocks[0];
    let y = b.top + b.height / 2.0;
    let (n, rects) = e.select(Select::Range(0.0, y, 2000.0, y));
    assert_eq!(rects.len() - n, 1, "the spaces between words are not highlighted: {rects:?}");
    let found = e.search("alpha beta gamma", None);
    assert_eq!(found[0].len(), 1, "a multi-word match is one span: {found:?}");
}

#[test]
fn table_cells_stay_separate_spans() {
    let e = engine("| alpha | beta |\n|---|---|\n| gamma | delta |\n");
    let found = e.search("alpha", None);
    assert_eq!(found.len(), 1);
    let b = e.layout().blocks[0];
    let y = b.top + 4.0;
    let (n, rects) = e.select(Select::Range(0.0, y, 2000.0, y));
    assert!(rects.len() - n >= 2, "a selection across cells skips the gap between them: {rects:?}");
}

#[test]
fn tagfilter_tags_show_as_text() {
    let e = engine(
        "<script>alert(\"hi\")</script>\n<p>Para.</p>\n<iframe src=\"https://example.com\"></iframe>\n\n<title>T</title> <textarea>A</textarea> <style>s</style> <xmp>x</xmp> <noembed>n</noembed> <noframes>f</noframes> <plaintext>p\n\nkeep <object>o</object><embed><template>t</template> end\n",
    );
    let all = e.layout().texts.join("\n");
    assert!(all.contains("<script>alert(\"hi\")</script>"), "{all}");
    assert!(all.contains("<iframe src=\"https://example.com\"></iframe>"), "{all}");
    for src in ["\"https://example.com\"", "'https://example.com'", "https://example.com"] {
        let links = mddog::render::doc::parse(&format!("<iframe src={src}></iframe>\n")).links;
        assert_eq!(links, ["https://example.com"], "the link stops at the attribute's end: {src}");
    }
    for t in ["<title>T</title>", "<textarea>A</textarea>", "<style>s</style>", "<xmp>x</xmp>", "<noembed>n</noembed>", "<noframes>f</noframes>", "<plaintext>p"] {
        assert!(all.contains(t), "{t} missing: {all}");
    }
    assert!(all.contains("keep") && !all.contains("<object") && !all.contains("<embed") && !all.contains("<template"), "{all}");
}

#[test]
fn a_selection_spans_inline_code_and_bold() {
    let e = engine("alpha `beta` **gamma** delta\n");
    let b = e.layout().blocks[0];
    let y = b.top + b.height / 2.0;
    let (n, rects) = e.select(Select::Range(0.0, y, 2000.0, y));
    assert_eq!(rects.len() - n, 1, "{rects:?}");
}

/// Text loose in a raw HTML block draws with no margin, as HTML has it. A `<p>` in the same block keeps its own.
#[test]
fn loose_html_text_has_no_margin() {
    use mddog::render::doc::{Block, parse};
    let d = parse("<script>x</script>\n<p>Para.</p>\n<iframe></iframe>\n\nAfter.\n");
    let kinds: Vec<_> = d.blocks.iter().map(|b| matches!(b, Block::Text(_))).collect();
    assert_eq!(kinds, [true, false, true, false], "Text, Para, Text, Para");
    let p = |s: &str| engine(s).layout().height;
    // The loose line and the paragraph under it sit as close as two lines of one paragraph.
    assert_eq!(p("<script>x</script>\n<p>Para.</p>\n"), p("x  \nPara.\n"));
}

#[test]
fn a_table_image_measures_at_the_table_width() {
    // Cell measure once read the width the previous paragraph left behind, 40 px at the page top.
    let table = "| a |\n|---|\n| ![a](assets/spike.png) |\n";
    let first = engine(table).layout().height;
    let after = engine(&format!("hello\n\n{table}")).layout().height - engine("hello\n").layout().height;
    assert!(first > 0.9 * after, "{first} vs {after}");
}

#[test]
fn an_unclosed_summary_keeps_its_text() {
    // The summary ran to the end of the file and was never shown.
    let blocks = mddog::render::doc::parse("<details><summary>Title\n\nbody one\n").blocks;
    let text = format!("{blocks:?}");
    assert!(text.contains("Title") && text.contains("body one"), "{text}");
}

#[test]
fn opening_details_fetches_its_remote_images() {
    // Only the first layout's images were fetched, so these stayed loading for good.
    let e = engine("<details><summary>More</summary>\n\n![a](https://example.invalid/a.png)\n\n</details>\n");
    assert_eq!(e.pending_images(), 0);
    assert!((0..40).step_by(2).any(|y| e.press(40.0, f64::from(y)) == Press::Other), "no summary to open");
    assert_eq!(e.pending_images(), 1);
    e.wait_images(std::time::Duration::from_secs(10));
    assert_eq!(e.pending_images(), 0, "the fetch never ran");
}

#[test]
fn a_stray_link_ends_with_its_paragraph() {
    // GitHub closes an <a> left open in a paragraph; a stray <b> it carries on, so that stays.
    let text = format!("{:?}", mddog::render::doc::parse("intro <a href=\"https://a.b\">stray\n\nlater text\n").blocks);
    assert!(text.contains("\"later text\"") && text.ends_with("link: 0 })])]"), "{text}");
}
