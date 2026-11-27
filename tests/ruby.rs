#![allow(clippy::unwrap_used, reason = "a failed step should fail the test")]

//! `<ruby>` text: the reading sits above its base, and only the base is page text.

use mddog::render::doc::{self, Block, Inline};
use mddog::render::engine::Engine;

/// Inlines as text: each ruby pair as `base(reading)`, linked text in brackets.
fn show(v: &[Inline]) -> String {
    v.iter()
        .map(|i| match i {
            Inline::Text(t, st) if st.link > 0 => format!("[{t}]"),
            Inline::Text(t, _) => t.clone(),
            Inline::Math(t, _) => format!("<{t}>"),
            Inline::Ruby { base, rt } => format!("{}({})", show(base), show(rt)),
            _ => " ".into(),
        })
        .collect()
}

fn para(src: &str) -> String {
    let d = doc::parse(src);
    let Some(Block::Para(v)) = d.blocks.first() else { panic!("{src:?} is not a paragraph") };
    show(v)
}

#[test]
fn ruby_pairs_each_base_with_its_reading() {
    assert_eq!(para("<ruby>日本語<rt>にほんご</rt></ruby>\n"), "日本語(にほんご)");
    assert_eq!(para("a <ruby>漢<rt>かん</rt>字<rt>じ</rt></ruby> b\n"), "a 漢(かん)字(じ) b");
    assert_eq!(para("<ruby>漢<rp>(</rp><rt>かん</rt><rp>)</rp></ruby>\n"), "漢(かん)");
    assert_eq!(para("<ruby>漢字</ruby>\n"), "漢字");
}

#[test]
fn ruby_end_tags_may_be_left_out() {
    assert_eq!(para("<ruby>漢<rp>(<rt>かん<rp>)</ruby>\n"), "漢(かん)");
    assert_eq!(para("<ruby>漢<rt>かん</rt>字<rt>じ</ruby>\n"), "漢(かん)字(じ)");
    assert_eq!(para("<ruby>漢<rp>(</rp><rt>かん<rp>)</rp></ruby>\n"), "漢(かん)");
}

#[test]
fn nested_ruby_keeps_both_readings() {
    assert_eq!(para("<ruby><ruby>漢<rt>かん</rt></ruby><rt>Kan</rt></ruby>\n"), "漢(かん)(Kan)");
}

#[test]
fn ruby_keeps_each_runs_link_and_dollars() {
    assert_eq!(para("<ruby><a href=\"/x\">漢</a>字<rt>かんじ</rt></ruby>\n"), "[漢]字(かんじ)");
    assert_eq!(para("<ruby>\\$5<rt>five dollars</rt></ruby>\n"), "$5(five dollars)");
}

#[test]
fn a_line_break_in_ruby_is_a_space() {
    assert_eq!(para("<ruby>foo\nbar<rt>x</rt></ruby>\n"), "foo bar(x)");
}

fn page(src: &str) -> Engine {
    let e = Engine::new(640.0, 2.0, false);
    e.load(src, std::path::Path::new("."), false, mddog::theme::builtin("github-light").unwrap(), mddog::render::doc::Callouts::Github);
    e
}

#[test]
fn ruby_text_is_the_base_and_its_line_grows() {
    let ruby = page("<ruby>日本語<rt>にほんご</rt></ruby>\n\nnext\n");
    assert_eq!(ruby.search("日本語", None).len(), 1);
    assert!(ruby.search("にほんご", None).is_empty(), "the reading is page text");
    let plain = page("日本語\n\nnext\n");
    let y = |e: &Engine| e.search("next", None)[0][0][1];
    assert!(y(&ruby) > y(&plain), "the reading did not make its line taller");
}

#[test]
fn a_ruby_in_a_table_cell_never_splits() {
    let e = page(
        "| a | b | c |\n|---|---|---|\n| <ruby>日本語日本語<rt>にほんごにほんご</rt></ruby> | x lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor | lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor |\n",
    );
    let ruby = e.search("日本語日本語", None);
    assert_eq!(ruby.len(), 1);
    assert_eq!(ruby[0].len(), 1, "the ruby split across lines");
    assert!(ruby[0][0][0] + ruby[0][0][2] <= e.search("x", None)[0][0][0], "the ruby runs into the next cell");
}

#[test]
fn a_spaced_ruby_heading_keeps_its_slug_and_text() {
    let d = doc::parse("# <ruby>foo bar<rt>x</rt></ruby>\n");
    let Some(Block::Heading(_, _, id)) = d.blocks.first() else { panic!("not a heading") };
    assert_eq!(id, "foo-bar");
    assert_eq!(page("<ruby>foo bar<rt>x</rt></ruby>\n").search("foo bar", None).len(), 1);
}

#[test]
fn a_math_reading_is_not_page_text() {
    assert!(page("<ruby>漢<rt>$xyz$</rt></ruby>\n").search("xyz", None).is_empty(), "the math reading is page text");
}
