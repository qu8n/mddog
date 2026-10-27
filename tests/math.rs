#![expect(clippy::unwrap_used, reason = "a failed step should fail the test")]

//! Pins the math in `fixtures/math/*.md`. Each block that holds math, or a `$` outside code, becomes
//! one line: its text, with inline math as `⟨…⟩`, display math as `⟦…⟧`, and code spans in backticks.
//! `UPDATE_GOLDEN=1` rewrites `tests/math.golden`. `cargo test --test math -- --ignored` writes
//! the diff of mddog against `reference/math-github/` (built by `reference/math-github.sh`).

use std::path::{Path, PathBuf};

use kuchikiki::NodeRef;
use kuchikiki::traits::TendrilSink;

const BLOCKS: [&str; 13] = ["p", "li", "td", "th", "h1", "h2", "h3", "h4", "h5", "h6", "dt", "dd", "summary"];

fn name(n: &NodeRef) -> String {
    n.as_element().map(|e| e.name.local.to_string()).unwrap_or_default()
}

fn class(n: &NodeRef) -> String {
    n.as_element().and_then(|e| e.attributes.borrow().get("class").map(str::to_owned)).unwrap_or_default()
}

/// What `mddog.js` (and GitHub's page) hands MathJax: math text with its entities decoded.
fn decode(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// The math as MathJax gets it. `lossy` drops each backslash before ASCII punctuation. GitHub
/// drops most of them and mddog keeps them, so this keeps that difference out of the GitHub diff.
fn tex(s: &str, lossy: bool) -> String {
    let s = decode(s);
    if !lossy {
        return s;
    }
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' || !it.peek().is_some_and(char::is_ascii_punctuation) {
            out.push(c);
        }
    }
    out
}

/// The block's own text: nested blocks, `pre`, and display math outside a block get their own line.
fn text(n: &NodeRef, out: &mut String, dollar: &mut bool, math: &mut bool, lossy: bool) {
    for c in n.children() {
        if let Some(t) = c.as_text() {
            *dollar |= t.borrow().contains('$');
            out.push_str(&t.borrow());
            continue;
        }
        let tag = name(&c);
        if BLOCKS.contains(&tag.as_str()) || tag == "pre" || class(&c).contains("highlight") {
            out.push(' ');
        } else if tag == "math-renderer" {
            *math = true;
            let (a, b) = if class(&c).contains("display") { ('⟦', '⟧') } else { ('⟨', '⟩') };
            out.push(a);
            out.push_str(&tex(&c.text_contents(), lossy));
            out.push(b);
        } else if tag == "code" {
            out.push('`');
            out.push_str(&c.text_contents());
            out.push('`');
        } else {
            text(&c, out, dollar, math, lossy);
        }
    }
}

fn extract(html: &str, lossy: bool) -> String {
    let doc = kuchikiki::parse_html().one(html).document_node;
    let mut out = String::new();
    for n in doc.descendants() {
        let tag = name(&n);
        let lone = tag == "math-renderer" && !n.ancestors().any(|a| BLOCKS.contains(&name(&a).as_str()));
        if !lone && !BLOCKS.contains(&tag.as_str()) {
            continue;
        }
        let (mut s, mut dollar, mut math) = (String::new(), false, lone);
        if lone {
            s = format!("⟦{}⟧", tex(&n.text_contents(), lossy));
        } else {
            text(&n, &mut s, &mut dollar, &mut math, lossy);
        }
        if dollar || math {
            out.push_str(&s.split_whitespace().collect::<Vec<_>>().join(" "));
            out.push('\n');
        }
    }
    out
}

fn fixtures() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir("fixtures/math").unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "md") && !p.ends_with("SOURCES.md")).collect();
    v.sort();
    v
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

#[test]
fn matches_golden() {
    let mut out = String::new();
    for f in fixtures() {
        out.push_str(&format!("=== {}\n{}", stem(&f), extract(&mddog::render(&std::fs::read_to_string(&f).unwrap()).html, false)));
    }
    let golden = "tests/math.golden";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(golden, &out).unwrap();
    }
    let want = std::fs::read_to_string(golden).unwrap();
    assert!(out == want, "tests/math.golden is stale (UPDATE_GOLDEN=1 refreshes it):\n{}", similar::TextDiff::from_lines(&want, &out).unified_diff().header("golden", "now"));
}

#[test]
#[ignore = "needs reference/math-github/, built from GitHub's API"]
fn matches_github() {
    let mut report = String::new();
    for f in fixtures() {
        let gh = Path::new("reference/math-github").join(format!("{}.html", stem(&f)));
        let gh = extract(&std::fs::read_to_string(&gh).unwrap(), true);
        let ours = extract(&mddog::render(&std::fs::read_to_string(&f).unwrap()).html, true);
        if gh != ours {
            report.push_str(&format!("{}", similar::TextDiff::from_lines(&gh, &ours).unified_diff().context_radius(0).header(&format!("github/{}", stem(&f)), "mddog")));
        }
    }
    // Every difference left is one of the intended ones, so this reports rather than fails.
    std::fs::write("reference/math-github/diff.txt", &report).unwrap();
    eprintln!("{} differing lines, see reference/math-github/diff.txt", report.lines().filter(|l| l.starts_with(['-', '+']) && !l.starts_with("---") && !l.starts_with("+++")).count());
}
