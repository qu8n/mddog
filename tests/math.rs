#![allow(clippy::unwrap_used, reason = "a failed step should fail the test")]

//! Checks the parser's `Doc` for `fixtures/math` against `tests/math.golden`: one line per block that
//! holds math or a `$`, inline math as `⟨…⟩`, display math as `⟦…⟧`, code spans in backticks. Run with
//! `UPDATE_GOLDEN=1` to write the golden after a parser change, then read its diff.

use mddog::render::doc::{self, Block, Inline};

fn inl(v: &[Inline], out: &mut String, dollar: &mut bool, math: &mut bool) {
    for i in v {
        match i {
            Inline::Text(t, st) if st.code => {
                out.push('`');
                out.push_str(t);
                out.push('`');
            }
            Inline::Text(t, _) => {
                *dollar |= t.contains('$');
                out.push_str(t);
            }
            Inline::Math(t, _) => {
                *math = true;
                out.push('⟨');
                out.push_str(t);
                out.push('⟩');
            }
            Inline::FootRef(n, _) => out.push_str(&n.to_string()),
            Inline::Soft | Inline::Hard => out.push(' '),
            Inline::Img { .. } => {}
        }
    }
}

fn line(v: &[Inline], out: &mut String) {
    let (mut s, mut dollar, mut math) = (String::new(), false, false);
    inl(v, &mut s, &mut dollar, &mut math);
    if dollar || math {
        out.push_str(&s.split_whitespace().collect::<Vec<_>>().join(" "));
        out.push('\n');
    }
}

fn blocks(bs: &[Block], out: &mut String) {
    for b in bs {
        match b {
            Block::Para(v) | Block::Text(v) | Block::Heading(_, v, _) => line(v, out),
            Block::Math(t) => {
                let s = format!("⟦$${t}$$⟧");
                out.push_str(&s.split_whitespace().collect::<Vec<_>>().join(" "));
                out.push('\n');
            }
            Block::Quote(c) | Block::Alert(_, c) | Block::Align(_, c) => blocks(c, out),
            Block::Details { summary, body, .. } => {
                line(summary, out);
                blocks(body, out);
            }
            Block::List { items, .. } => items.iter().for_each(|i| blocks(&i.blocks, out)),
            Block::Table { head, rows, .. } => {
                for c in head.iter().chain(rows.iter().flatten()) {
                    line(c, out);
                }
            }
            Block::Footnotes(f) => f.iter().for_each(|f| blocks(&f.2, out)),
            _ => {}
        }
    }
}

#[test]
fn native_math_against_golden() {
    let mut fx: Vec<_> = std::fs::read_dir("fixtures/math").unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "md") && !p.ends_with("SOURCES.md")).collect();
    fx.sort();
    let mut out = String::new();
    for f in fx {
        out.push_str(&format!("=== {}\n", f.file_stem().unwrap().to_string_lossy()));
        blocks(&doc::parse(&std::fs::read_to_string(&f).unwrap()).blocks, &mut out);
    }
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write("tests/math.golden", &out).unwrap();
    }
    let want = std::fs::read_to_string("tests/math.golden").unwrap();
    if let Some((i, (w, o))) = want.lines().zip(out.lines()).enumerate().find(|(_, (w, o))| w != o) {
        panic!("line {} differs from tests/math.golden\ngolden: {w}\nnative: {o}", i + 1);
    }
    assert_eq!(want.lines().count(), out.lines().count(), "tests/math.golden has another number of lines");
}

/// GitHub reads `<y>` in `$x<y>z$` as HTML, so the dollars around it are text. `$a<b$` has no tag and stays math.
#[test]
fn inline_html_splits_math() {
    let d = doc::parse("Inline: $a<b$ and $x<y>z$ end.\n");
    let Block::Para(v) = &d.blocks[0] else { panic!("not a paragraph") };
    let math: Vec<_> = v.iter().filter_map(|i| if let Inline::Math(t, _) = i { Some(t.as_str()) } else { None }).collect();
    assert_eq!(math, ["$a<b$"]);
    let text: String = v.iter().filter_map(|i| if let Inline::Text(t, _) = i { Some(t.as_str()) } else { None }).collect();
    assert_eq!(text, "Inline:  and $xz$ end.");
}

fn items(tex: &str) -> Vec<mddog::render::math::MItem> {
    use mddog::render::{math, text::Fonts};
    let b = math::layout(&mut Fonts::new(), tex, 32.0, false);
    // An unknown command shows as its own source text.
    assert!(!texts(&b.items).iter().any(|t| t.starts_with('\\')), "unknown commands in {tex}");
    b.items
}

fn texts(v: &[mddog::render::math::MItem]) -> Vec<String> {
    v.iter().filter_map(|i| if let mddog::render::math::MItem::Text { s, .. } = i { Some(s.clone()) } else { None }).collect()
}

#[test]
fn tex_commands_from_the_fixtures_draw() {
    use mddog::render::math::MItem;
    for tex in [r"\underbrace{a+b}_{n}", r"\overrightarrow{AB}", r"\overleftarrow{AB}", r"\underset{x}{\min}", r"\xrightarrow{d}", r"\KaTeX"] {
        items(tex);
    }
    // \binom has no bar; \tfrac13 is 1 over 3
    assert!(!items(r"\binom{n}{k}").iter().any(|i| matches!(i, MItem::Rule { .. })));
    let v = items(r"\tfrac13");
    assert!(v.iter().any(|i| matches!(i, MItem::Rule { .. })));
    assert_eq!(texts(&v), ["1", "3"]);
    assert_eq!(texts(&items(r"a \not\in B")), ["a", "∉", "B"]);
    // an escaped brace in a text argument is a character, and the argument ends at its own brace
    assert_eq!(texts(&items(r"\textbf{\{}+x")), ["{", "+", "x"]);
    // the column spec sets alignment and is not drawn
    assert_eq!(texts(&items(r"\begin{array}{rl} a & b \end{array}")), ["a", "b"]);
}
