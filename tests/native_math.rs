#![allow(clippy::unwrap_used, reason = "a failed step should fail the test")]

//! `tests/math.golden` from the native parser's `Doc`, in the format `tests/math.rs` reads from HTML:
//! one line per block that holds math or a `$`, inline math as `⟨…⟩`, display math as `⟦…⟧`, code spans
//! in backticks. Writes `tmp/native-math.diff` and reports the differing lines.

use mddog::native::doc::{self, Block, Inl};

fn inl(v: &[Inl], out: &mut String, dollar: &mut bool, math: &mut bool) {
    for i in v {
        match i {
            Inl::Text(t, st) if st.code => {
                out.push('`');
                out.push_str(t);
                out.push('`');
            }
            Inl::Text(t, _) => {
                *dollar |= t.contains('$');
                out.push_str(t);
            }
            Inl::Math(t, _) => {
                *math = true;
                out.push('⟨');
                out.push_str(t);
                out.push('⟩');
            }
            Inl::FootRef(n, _) => out.push_str(&n.to_string()),
            Inl::Soft | Inl::Hard => out.push(' '),
            Inl::Img { .. } => {}
        }
    }
}

fn line(v: &[Inl], out: &mut String) {
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
            Block::Para(v) | Block::Heading(_, v, _) => line(v, out),
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
    let want = std::fs::read_to_string("tests/math.golden").unwrap();
    let diff = similar::TextDiff::from_lines(&want, &out).unified_diff().context_radius(0).header("golden", "native").to_string();
    let _ = std::fs::create_dir_all("tmp");
    std::fs::write("tmp/native-math.diff", &diff).unwrap();
    std::fs::write("tmp/native-math.golden", &out).unwrap();
    let n = diff.lines().filter(|l| l.starts_with(['-', '+']) && !l.starts_with("---") && !l.starts_with("+++")).count();
    eprintln!("{n} differing lines of {}, see tmp/native-math.diff", want.lines().count());
    // 9 known: extractor artefacts (footnote back-arrows, display style next to dropped HTML, a `<div>` split).
    assert!(n <= 9, "{n} lines differ from the golden, up from 9, see tmp/native-math.diff");
}

/// GitHub reads `<y>` in `$x<y>z$` as HTML, so the dollars around it are text. `$a<b$` has no tag and stays math.
#[test]
fn inline_html_splits_math() {
    let d = doc::parse("Inline: $a<b$ and $x<y>z$ end.\n");
    let Block::Para(v) = &d.blocks[0] else { panic!("not a paragraph") };
    let math: Vec<_> = v.iter().filter_map(|i| if let Inl::Math(t, _) = i { Some(t.as_str()) } else { None }).collect();
    assert_eq!(math, ["$a<b$"]);
    let text: String = v.iter().filter_map(|i| if let Inl::Text(t, _) = i { Some(t.as_str()) } else { None }).collect();
    assert_eq!(text, "Inline:  and $xz$ end.");
}

fn items(tex: &str) -> Vec<mddog::native::math::MItem> {
    use mddog::native::{math, text::Fonts};
    let _ = math::take_unknown();
    let b = math::layout(&mut Fonts::new(), tex, 32.0, false);
    assert_eq!(math::take_unknown(), Vec::<String>::new(), "unknown commands in {tex}");
    b.items
}

fn texts(v: &[mddog::native::math::MItem]) -> Vec<String> {
    v.iter().filter_map(|i| if let mddog::native::math::MItem::Text { s, .. } = i { Some(s.clone()) } else { None }).collect()
}

#[test]
fn tex_commands_from_the_fixtures_draw() {
    use mddog::native::math::MItem;
    for tex in [r"\underbrace{a+b}_{n}", r"\overrightarrow{AB}", r"\overleftarrow{AB}", r"\underset{x}{\min}", r"\xrightarrow{d}", r"\KaTeX"] {
        let v = items(tex);
        assert!(!texts(&v).iter().any(|t| t.contains('\\')), "{tex} drew its own name");
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

fn exprs(bs: &[Block], out: &mut Vec<(String, bool)>) {
    let inl = |v: &[Inl], out: &mut Vec<(String, bool)>| {
        for i in v {
            if let Inl::Math(t, _) = i {
                match t.strip_prefix("$$").and_then(|t| t.strip_suffix("$$")) {
                    Some(t) => out.push((t.to_string(), true)),
                    None => out.push((t.strip_prefix('$').and_then(|t| t.strip_suffix('$')).unwrap_or(t).to_string(), false)),
                }
            }
        }
    };
    for b in bs {
        match b {
            Block::Para(v) | Block::Heading(_, v, _) => inl(v, out),
            Block::Math(t) => out.push((t.clone(), true)),
            Block::Quote(c) | Block::Alert(_, c) | Block::Align(_, c) => exprs(c, out),
            Block::Details { summary, body, .. } => {
                inl(summary, out);
                exprs(body, out);
            }
            Block::List { items, .. } => items.iter().for_each(|i| exprs(&i.blocks, out)),
            Block::Table { head, rows, .. } => head.iter().chain(rows.iter().flatten()).for_each(|c| inl(c, out)),
            Block::Footnotes(f) => f.iter().for_each(|f| exprs(&f.2, out)),
            _ => {}
        }
    }
}

/// Counts the expressions per fixture that hold a command the native TeX parser doesn't know.
#[test]
#[ignore = "report, writes tmp/native-math-unknown.txt"]
fn unknown_commands() {
    use mddog::native::{math, text::Fonts};
    let mut fonts = Fonts::new();
    let mut fx: Vec<_> = std::fs::read_dir("fixtures/math").unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "md") && !p.ends_with("SOURCES.md")).collect();
    fx.sort();
    let mut report = String::new();
    let (mut all, mut bad_all) = (0, 0);
    let mut cmds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for f in fx {
        let mut v = vec![];
        exprs(&doc::parse(&std::fs::read_to_string(&f).unwrap()).blocks, &mut v);
        let mut bad = 0;
        for (tex, display) in &v {
            let _ = math::take_unknown();
            let _ = math::layout(&mut fonts, tex, 32.0, *display);
            let u = math::take_unknown();
            if !u.is_empty() {
                bad += 1;
                for c in u {
                    *cmds.entry(c).or_default() += 1;
                }
            }
        }
        all += v.len();
        bad_all += bad;
        report.push_str(&format!(
            "{:<28} {:>4} expressions, {:>4} with unknown commands ({:.0}%)\n",
            f.file_stem().unwrap().to_string_lossy(),
            v.len(),
            bad,
            100.0 * bad as f64 / v.len().max(1) as f64
        ));
    }
    report.push_str(&format!("TOTAL {all} expressions, {bad_all} with unknown commands ({:.1}%)\n\nCommands by count:\n", 100.0 * bad_all as f64 / all as f64));
    let mut by: Vec<_> = cmds.into_iter().collect();
    by.sort_by_key(|b| std::cmp::Reverse(b.1));
    for (c, n) in by {
        report.push_str(&format!("{n:>4} \\{c}\n"));
    }
    std::fs::write("tmp/native-math-unknown.txt", &report).unwrap();
    eprintln!("{report}");
}
