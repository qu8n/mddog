//! Regressions in the engine's answers to the viewer's calls.

use mddog::render::engine::{Engine, Pointer, Press, Select};

fn engine(src: &str) -> Engine {
    let e = Engine::new(640.0, 2.0, false);
    e.load(src, std::path::Path::new("."), false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
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
    e.rescale(1.0, 640.0, f64::INFINITY);
    assert!(at_left(&e), "a font zoom moved the block sideways");
    let small = Engine::new(640.0, 1.0, false);
    small.load(&wide, std::path::Path::new("."), false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
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
    let table = "| a |\n|---|\n| ![a](assets/logo.png) |\n";
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
fn a_heading_takes_its_align_attribute() {
    use mddog::render::doc::{Align, Block};
    for (attr, want) in [("center", Align::Center), ("right", Align::Right)] {
        let blocks = mddog::render::doc::parse(&format!("<h1 align=\"{attr}\">\n  Title\n</h1>\n\nafter\n")).blocks;
        assert!(matches!(&blocks[..], [Block::Align(a, inner), Block::Para(_)] if *a == want && matches!(&inner[..], [Block::Heading(1, ..)])), "{blocks:?}");
    }
    let blocks = mddog::render::doc::parse("<h1 align=\"center\">\n  Title\n</h2>\n\nafter\n").blocks;
    assert!(matches!(&blocks[..], [Block::Align(Align::Center, _), Block::Para(_)]), "another level's end tag still ends the centering: {blocks:?}");
    for tag in ["h1", "div"] {
        let blocks = mddog::render::doc::parse(&format!("<div align=\"center\">\n<{tag} align=\"left\">\n\nTitle\n\n</{tag}>\n</div>\n")).blocks;
        assert!(matches!(&blocks[..], [Block::Align(Align::Center, inner)] if matches!(&inner[..], [Block::Align(Align::Left, _)])), "{tag}: {blocks:?}");
    }
}

#[test]
fn alignment_keeps_heading_spacing() {
    let height = |src: &str| {
        let e = Engine::new(640.0, 2.0, false);
        e.load(src, std::path::Path::new("."), false, mddog::theme::builtin("obsidian-light").unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
        e.layout().height
    };
    let plain = height("# A\n\n## B\n\n## C\n\ntext\n");
    assert_eq!(height("# A\n\n<h2 align=\"center\">B</h2>\n\n## C\n\ntext\n"), plain);
    assert_eq!(height("# A\n\n<div align=\"center\">\n\n## B\n\n</div>\n\n## C\n\ntext\n"), plain);
    let after_text = height("<div>loose</div>\n\n## B\n\ntext\n");
    assert_eq!(height("<div>loose</div>\n\n<h2 align=\"center\">B</h2>\n\ntext\n"), after_text);
    let in_list = height("- one\n- two\n\n  ## B\n");
    assert_eq!(height("- one\n- two\n\n  <h2 align=\"center\">B</h2>\n"), in_list);
    let tight = height("- one\n  ## B\n- two\n");
    assert_eq!(height("- one\n  <h2 align=\"center\">B</h2>\n- two\n"), tight);
}

#[test]
fn a_table_lays_out_the_same_at_any_scale() {
    let src = "| name | what it does |\n|---|:-:|\n| alpha | a long cell that has to wrap across more than one line in the column |\n| b | ok |\n";
    let at = |scale: f64| {
        let e = Engine::new(640.0, scale, false);
        e.load(src, std::path::Path::new("."), false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
        (e.layout().height, e.search("column", None)[0][0])
    };
    let one = at(1.0);
    for scale in [0.5, 2.0] {
        let other = at(scale);
        assert!((other.0 - one.0).abs() <= 1.0 / scale && other.1.iter().zip(one.1.iter()).all(|(a, b)| (a - b).abs() < 0.5), "{scale}x: within one device pixel of 1x: {other:?} vs {one:?}");
    }
}

#[test]
fn unaligned_table_headers_center_as_in_a_browser() {
    let e = engine("| Name | Left |\n|---|:---|\n| a long body cell here | another long body cell |\n");
    let x = |q: &str| e.search(q, None)[0][0];
    let (name, body, left, other) = (x("Name"), x("a long body"), x("Left"), x("another"));
    assert!(name[0] > body[0] + 10.0, "an unaligned header centers: {name:?} vs {body:?}");
    assert!((left[0] - other[0]).abs() < 1.0, "a left-aligned header stays left: {left:?} vs {other:?}");
}

#[test]
fn table_cells_center_vertically() {
    let e = engine("| a | b |\n|---|---|\n| one<br>two<br>three | solo |\n");
    let y = |q: &str| e.search(q, None)[0][0];
    let (top, bottom, solo) = (y("one"), y("three"), y("solo"));
    let mid = |r: [f64; 4]| r[1] + r[3] / 2.0;
    assert!((mid(solo) - f64::midpoint(mid(top), mid(bottom))).abs() < 1.0, "{solo:?} sits between {top:?} and {bottom:?}");
}

#[test]
fn a_centered_cell_moves_its_footnote_anchor() {
    let e = engine("| a | b |\n|---|---|\n| one<br>two<br>three | see[^n] |\n\n[^n]: note\n");
    let mark = e.search("see", None)[0][0];
    let back = e.anchor_y("fnref-n").unwrap_or_default();
    assert!((back - mark[1]).abs() < mark[3], "the back link lands on the reference: {back} vs {mark:?}");
}

#[test]
fn words_break_after_a_hyphen() {
    let row = |cell: &str| [cell; 8].join(" | ");
    let e = engine(&format!("| {} |\n|{}|\n| {} |\n", row("h"), ["---"; 8].join("|"), row("alphabet-soup")));
    let (first, second) = (e.search("alphabet-", None)[0][0], e.search("soup", None)[0][0]);
    assert!(second[1] > first[1], "a squeezed column breaks after the hyphen: {first:?} {second:?}");
    for cell in ["alphabet-**soup**", "alphabet**-soup**"] {
        let e = engine(&format!("| {} |\n|{}|\n| {} |\n", row("h"), ["---"; 8].join("|"), row(cell)));
        let (first, second) = (e.search("alphabet", None)[0][0], e.search("soup", None)[0][0]);
        assert!(second[1] > first[1], "{cell} breaks after the hyphen: {first:?} {second:?}");
    }
    let soup = |rt: &str| engine(&format!("| {} |\n|{}|\n| {} |\n", row("h"), ["---"; 8].join("|"), row(&format!("<ruby>A<rt>{rt}</rt></ruby>soupy-word")))).search("soupy", None)[0][0];
    assert_eq!(soup("x-"), soup("xy"), "a hyphen in ruby text opens no break after the ruby");
    for word in ["v1-2", "v1-٢", "ab-)"] {
        let e = engine(&format!("| {} |\n|{}|\n| {} |\n", row("h"), ["---"; 8].join("|"), row(word)));
        assert_eq!(e.search(word, None)[0].len(), 1, "no break in {word}");
    }
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
fn a_failed_remote_image_keeps_its_alt_text() {
    // Loading and failed draw the same alt text, so a failure needs no new layout and leaves nothing loading.
    let e = engine("![a cat](https://example.invalid/cat.png)\n");
    assert_eq!(e.pending_images(), 1);
    assert_eq!(e.search("a cat", None).len(), 1);
    e.wait_images(std::time::Duration::from_secs(10));
    assert_eq!(e.pending_images(), 0);
    assert_eq!(e.search("a cat", None).len(), 1);
}

#[test]
fn a_stray_link_ends_with_its_paragraph() {
    // GitHub closes an <a> left open in a paragraph; a stray <b> it carries on, so that stays.
    let text = format!("{:?}", mddog::render::doc::parse("intro <a href=\"https://a.b\">stray\n\nlater text\n").blocks);
    assert!(text.contains("\"later text\"") && text.ends_with("link: 0 })])]"), "{text}");
}

#[test]
fn a_huge_svg_draws_scaled_down() {
    // A declared height of 2,000,000,000 panicked in tiny-skia, and 200,000 drew a page that tall.
    let dir = std::env::temp_dir().join(format!("mddog-huge-svg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    for (name, h) in [("huge", 2_000_000_000u64), ("tall", 200_000)] {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="{h}"><rect width="10" height="{h}"/></svg>"#);
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap_or_else(|e| panic!("{e}"));
        let e = Engine::new(640.0, 2.0, false);
        e.load(&format!("![{name}]({name}.svg)\n"), &dir, false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
        let height = e.layout().height;
        assert!(height < 20_000.0, "{name}: page is {height} px tall");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_svg_whose_rounded_size_is_too_big_still_draws() {
    // 2399.5 by 13983.5 fits under 2^25 pixels, but 2400 by 13984 does not, and the image vanished.
    let dir = std::env::temp_dir().join(format!("mddog-round-svg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="2399.5" height="13983.5"><rect width="10" height="10"/></svg>"#;
    std::fs::write(dir.join("big.svg"), svg).unwrap_or_else(|e| panic!("{e}"));
    let e = Engine::new(3000.0, 1.0, false);
    e.load("![big picture](big.svg)\n", &dir, false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
    let height = e.layout().height;
    assert!(height > 13_000.0, "page is {height} px tall");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[cfg(target_os = "macos")]
fn a_display_p3_page_matches_the_cells_around_it() {
    // Ghostty shows image pixels as Display P3 but cell colors as sRGB, so the page's #0d1117 must go out in P3 to
    // look like the #0d1117 cells beside it.
    let e = Engine::new(640.0, 2.0, false);
    e.load("x\n", std::path::Path::new("."), false, mddog::theme::builtin("github-dark").unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
    let corner = |e: &Engine| e.snapshot(0.0, 10.0, 2.0, |n| Some(vec![0; n])).unwrap().rgba[..4].to_vec();
    assert_eq!(corner(&e), [13, 17, 23, 255]);
    e.set_display_p3(true);
    assert_eq!(corner(&e), [14, 17, 22, 255]);
}

/// The darkest and brightest luminance inside a CSS-pixel rect of a 2x snapshot.
fn lum_range(e: &Engine, r: [f64; 4]) -> (f64, f64) {
    let snap = e.snapshot(0.0, 200.0, 2.0, |n| Some(vec![0u8; n])).unwrap_or_else(|| unreachable!());
    let (mut lo, mut hi) = (f64::MAX, 0.0f64);
    for y in (r[1] * 2.0) as usize..((r[1] + r[3]) * 2.0) as usize {
        for x in (r[0] * 2.0) as usize..((r[0] + r[2]) * 2.0) as usize {
            let p = &snap.rgba[(y * snap.width + x) * 4..][..3];
            let l = 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
            (lo, hi) = (lo.min(l), hi.max(l));
        }
    }
    (lo, hi)
}

#[test]
fn search_marks_are_dark_text_on_a_bright_box() {
    // Chrome's find-in-page look in every theme: a bright box with dark text, so the match stands
    // out and reads the same on a light or dark page.
    for theme in ["github-light", "github-dark", "obsidian-light", "obsidian-dark"] {
        let e = Engine::new(640.0, 2.0, false);
        e.load("ammo [ammo](https://x.y) `ammo`\n", std::path::Path::new("."), false, mddog::theme::builtin(theme).unwrap_or_else(|| unreachable!()), mddog::render::doc::Callouts::Github);
        let found = e.search("ammo", None);
        e.search_current(1);
        for (i, r) in found.iter().enumerate() {
            let (lo, hi) = lum_range(&e, r[0]);
            assert!(lo < 15.0 && hi > 150.0, "{theme} match {i}: darkest {lo:.0}, brightest {hi:.0}");
        }
    }
}

#[test]
fn a_wide_window_centers_the_page_at_its_max_width() {
    let src = format!("{}\n\n| head | {} |\n|---|---|\n| cell | {} |\n\n```\n{}\n```\n", "word ".repeat(300), "wide ".repeat(80), "wide ".repeat(80), "x".repeat(600));
    let e = engine(&src);
    e.resize(2000.0, 800.0);
    let words = e.search("word", None);
    let (left, right) = words.iter().map(|m| m[0]).fold((f64::MAX, 0.0f64), |(l, r), m| (l.min(m[0]), r.max(m[0] + m[2])));
    assert!((left - 624.0).abs() < 1.0, "text starts at {left}");
    assert!(right <= 1376.0 && right > 1300.0, "text ends at {right}");
    for m in e.search("wide", None) {
        assert!(m[0][0] >= 624.0 && m[0][0] + m[0][2] <= 1376.0, "a table cell left the column: {:?}", m[0]);
    }
    e.search_clear();
    let code = e.layout().blocks[2];
    let y = code.top + code.height / 2.0;
    assert!(e.code_scroll(300.0, y, 40.0).is_empty(), "the left margin scrolled the code block");
    assert!(!e.code_scroll(1000.0, y, 40.0).is_empty(), "the code block didn't scroll");
    e.resize(700.0, 800.0);
    let left = e.search("word", None).iter().map(|m| m[0][0]).fold(f64::MAX, f64::min);
    assert!((left - 24.0).abs() < 1.0, "a narrow window starts text at {left}");
}

#[test]
fn wide_display_math_scrolls_inside_the_page_column() {
    let src = format!("$$\n{}\n$$\n", "x + ".repeat(200) + "x");
    let e = engine(&src);
    e.resize(2000.0, 800.0);
    let math = e.layout().blocks[0];
    let y = math.top + math.height / 2.0;
    assert!(e.code_scroll(300.0, y, 40.0).is_empty(), "the left margin scrolled the equation");
    assert!(!e.code_scroll(1000.0, y, 40.0).is_empty(), "the wide equation didn't scroll");
}

fn center(r: [f64; 4]) -> (f64, f64) {
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}

#[test]
fn targets_list_each_clickable_thing_in_view_once() {
    let words = "word ".repeat(60);
    let e = engine(&format!("[{words}](#a) and [b](https://example.com)\n\n```\ncode\n```\n\n<details><summary>More</summary>\n\nhi\n</details>\n"));
    let all = e.targets(0.0, e.layout().height);
    assert_eq!(all.len(), 4, "{all:?}");
    let (x, y) = center(all[0]);
    assert_eq!(e.link(x, y).as_deref(), Some("#a"));
    let (x, y) = center(all[1]);
    assert_eq!(e.link(x, y).as_deref(), Some("https://example.com"));
    let (x, y) = center(all[2]);
    assert!(matches!(e.press(x, y), Press::Copy { .. }));
    let (x, y) = center(all[3]);
    assert_eq!(e.press(x, y), Press::Other);
}

#[test]
fn targets_skip_what_is_out_of_view() {
    let e = engine(&format!("[top](#a)\n\n{}[bottom](#b)\n", "para\n\n".repeat(80)));
    let h = e.layout().height;
    let top = e.targets(0.0, 200.0);
    assert_eq!(top.len(), 1);
    let (x, y) = center(top[0]);
    assert_eq!(e.link(x, y).as_deref(), Some("#a"));
    let bottom = e.targets(h - 200.0, 200.0);
    assert_eq!(bottom.len(), 1);
    let (x, y) = center(bottom[0]);
    assert_eq!(e.link(x, y).as_deref(), Some("#b"));
}

#[test]
fn targets_skip_links_scrolled_out_of_a_wide_table() {
    let e = engine(&format!("| a | b |\n|---|---|\n| {} | [far](#far) |\n", "x".repeat(300)));
    assert!(e.targets(0.0, e.layout().height).is_empty());
    e.code_scroll_view(0.0, 400.0, 10_000.0);
    let all = e.targets(0.0, e.layout().height);
    assert_eq!(all.len(), 1);
    let (x, y) = center(all[0]);
    assert_eq!(e.link(x, y).as_deref(), Some("#far"));
}

#[test]
fn labels_draw_over_the_page_and_go_away() {
    let e = engine("[link](#a)\n");
    let target = e.targets(0.0, 100.0)[0];
    let yellow = |e: &Engine| {
        let snap = e.snapshot(0.0, 100.0, 2.0, |n| Some(vec![0u8; n])).unwrap();
        snap.rgba.chunks(4).any(|p| p[0] > 240 && p[1] > 190 && p[2] < 140)
    };
    assert!(!yellow(&e));
    let drawn = e.set_labels(&[(target, "s".into(), 0)], 0.0, 100.0);
    assert!(!drawn.is_empty());
    assert!(yellow(&e));
    let cleared = e.set_labels(&[], 0.0, 100.0);
    assert_eq!(cleared, drawn);
    assert!(!yellow(&e));
}

#[test]
fn targets_take_the_visible_part_of_a_link() {
    let e = engine(&format!("| a | b |\n|---|---|\n| x | [{}](#far) |\n", "y".repeat(300)));
    let all = e.targets(0.0, e.layout().height);
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(all[0][0] + all[0][2] <= 640.0, "{all:?}");
    let (x, y) = center(all[0]);
    assert_eq!(e.link(x, y).as_deref(), Some("#far"));
    let e = engine("[top](#a)\n");
    let first = e.targets(0.0, 100.0)[0];
    let cut = e.targets(first[1] + 4.0, 100.0);
    assert_eq!(cut.len(), 1);
    assert!(cut[0][1] >= first[1] + 4.0, "{cut:?}");
    let (x, y) = center(cut[0]);
    assert_eq!(e.link(x, y).as_deref(), Some("#a"));
}

#[test]
fn a_summary_label_toggles_even_with_a_link_inside() {
    let e = engine(&format!("<details><summary><a href=\"#x\">{}</a></summary>\n\nhi\n</details>\n", "linked ".repeat(12)));
    let all = e.targets(0.0, e.layout().height);
    assert_eq!(all.len(), 2, "{all:?}");
    let toggle = *all
        .iter()
        .find(|r| {
            let (x, y) = center(**r);
            e.link(x, y).is_none()
        })
        .expect("the summary's own target hits no link");
    let (x, y) = center(toggle);
    assert_eq!(e.press(x, y), Press::Other);
}

#[test]
fn a_label_on_a_target_cut_at_the_bottom_stays_in_view() {
    let e = engine("[link](#a)\n");
    let whole = e.targets(0.0, 100.0)[0];
    let height = whole[1] + 3.0;
    let cut = e.targets(0.0, height)[0];
    let drawn = e.set_labels(&[(cut, "s".into(), 0)], 0.0, height);
    let snap = e.snapshot(0.0, height, 2.0, |n| Some(vec![0u8; n])).unwrap();
    let rows = snap.rgba.chunks(snap.width * 4).filter(|row| row.chunks(4).any(|p| p[0] > 240 && p[1] > 190 && p[2] < 140)).count();
    assert!(rows as f64 >= 20.0, "only {rows} rows of the label show, {drawn:?}");
}

fn label_rows(e: &Engine, top: f64, height: f64) -> usize {
    let snap = e.snapshot(top, height, 2.0, |n| Some(vec![0u8; n])).unwrap();
    snap.rgba.chunks(snap.width * 4).filter(|row| row.chunks(4).any(|p| p[0] > 240 && p[1] > 190 && p[2] < 140)).count()
}

#[test]
fn a_label_on_a_target_cut_at_the_top_stays_in_view() {
    let e = engine("[link](#a)\n\nmore text\n\nand more\n");
    let whole = e.targets(0.0, 100.0)[0];
    let top = whole[1] + whole[3] - 3.0;
    let cut = e.targets(top, 100.0)[0];
    e.set_labels(&[(cut, "s".into(), 0)], top, 100.0);
    let rows = label_rows(&e, top, 100.0);
    assert!(rows >= 20, "only {rows} rows of the label show");
}

#[test]
fn a_wrapped_link_is_labeled_on_its_most_visible_line() {
    let e = engine(&format!("[{}](#a)\n\nafter\n", "wrapped link words ".repeat(12)));
    let lines = e.targets(0.0, 400.0);
    assert_eq!(lines.len(), 1);
    let first = lines[0];
    let top = first[1] + first[3] - 3.0;
    let cut = e.targets(top, 200.0);
    assert_eq!(cut.len(), 1);
    assert!(cut[0][3] > 10.0, "the label went on a {}-pixel sliver", cut[0][3]);
}

#[test]
fn a_wrapped_link_in_full_view_is_labeled_on_its_first_line() {
    let e = engine("# Title\n\nSee [the docs](#d) and [setup](#s), or [a much longer link that wraps onto the next line of the paragraph here](#x).\n");
    let all = e.targets(0.0, 400.0);
    assert_eq!(all.len(), 3);
    let wrapped = all.iter().find(|r| e.link(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0).as_deref() == Some("#x")).unwrap();
    assert!(wrapped[0] > 100.0, "the label went on the second line: {wrapped:?}");
}

#[test]
fn a_folded_callout_opens_on_a_click() {
    let e = Engine::new(640.0, 2.0, false);
    e.load("> [!faq]- Folded\n> hidden body\n", std::path::Path::new("."), false, mddog::theme::builtin("obsidian-light").unwrap(), mddog::render::doc::Callouts::Obsidian);
    assert_eq!(e.search("Folded", None).len(), 1, "the title shows");
    assert!(e.search("hidden body", None).is_empty(), "a folded callout hides its body");
    assert!((0..40).step_by(2).any(|y| e.press(40.0, f64::from(y)) == Press::Other), "no title to click");
    assert_eq!(e.search("hidden body", None).len(), 1, "the click opened it");
}
