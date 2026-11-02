//! Regressions in the native engine's answers to the viewer's calls.

use mddog::engine::{Engine, Pointer, Press, Select};

fn engine(src: &str) -> Engine {
    let e = Engine::new(640.0, 2.0);
    e.load(src, std::path::Path::new("."), false, mddog::theme::builtin("github-light").unwrap_or_else(|| unreachable!()));
    e
}

#[test]
fn keys_scroll_a_wide_code_block_sideways() {
    let e = engine(&format!("```\n{}\n```\n", "x".repeat(400)));
    assert!(!e.code_scroll_view(0.0, 400.0, 80.0).is_empty(), "the visible wide block didn't move");
    let saved = e.code_positions();
    assert_ne!(saved, "[0.0]");
    let back = engine(&format!("```\n{}\n```\n", "x".repeat(400)));
    back.code_restore(&saved);
    assert_eq!(back.code_positions(), saved);
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
    for y in (0..40).step_by(2) {
        if e.press(40.0, f64::from(y)) == Press::Other {
            break;
        }
    }
    // Neither may index the old page's buttons.
    let _ = e.reset_copy();
    let _ = e.hover(600.0, 30.0);
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
    let shot = |e: &Engine| e.snapshot(0.0, 200.0, 2.0).unwrap().rgba.to_vec();
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
