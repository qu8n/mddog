use std::cell::RefCell;
use std::collections::HashMap;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use super::{Runs, Tok};

const CAPTURES: &[(&str, &str)] = &[
    ("comment", "pl-c"),
    ("keyword", "pl-k"),
    ("function", "pl-en"),
    ("function.builtin", "pl-c1"),
    ("function.method", "pl-en"),
    ("function.method.builtin", "pl-en"),
    ("function.macro", "pl-en"),
    ("constructor", "pl-v"),
    ("variable", "pl-s1"),
    ("variable.parameter", "pl-s1"),
    ("variable.builtin", "pl-smi"),
    ("property", "pl-c1"),
    ("punctuation.delimiter", "pl-kos"),
    ("punctuation.bracket", "pl-kos"),
    ("punctuation.special", "pl-kos"),
    ("operator", "pl-c1"),
    ("number", "pl-c1"),
    ("constant", "pl-c1"),
    ("constant.builtin", "pl-c1"),
    ("boolean", "pl-c1"),
    ("string", "pl-s"),
    ("string.special", "pl-s"),
    ("string.special.symbol", "pl-pds"),
    ("string.special.regex", "pl-sr"),
    ("escape", "pl-cce"),
    ("type", "pl-smi"),
    ("type.builtin", "pl-smi"),
    ("tag", "pl-ent"),
    ("attribute", "pl-c1"),
    ("label", "pl-en"),
    ("embedded", "pl-s1"),
];

fn ts_config(name: &str) -> Option<HighlightConfiguration> {
    let (language, highlights, injections, locals): (tree_sitter::Language, String, &str, &str) = match name {
        "js" | "javascript" | "jsx" | "mjs" | "cjs" | "node" => (
            tree_sitter_javascript::LANGUAGE.into(),
            format!("{}\n{}", tree_sitter_javascript::JSX_HIGHLIGHT_QUERY, tree_sitter_javascript::HIGHLIGHT_QUERY),
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ),
        "ts" | "typescript" | "mts" | "cts" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            format!("{}\n{}", tree_sitter_typescript::HIGHLIGHTS_QUERY, tree_sitter_javascript::HIGHLIGHT_QUERY),
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "tsx" => (
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            format!("{}\n{}\n{}", tree_sitter_typescript::HIGHLIGHTS_QUERY, tree_sitter_javascript::JSX_HIGHLIGHT_QUERY, tree_sitter_javascript::HIGHLIGHT_QUERY),
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "python" | "py" | "python3" | "py3" | "gyp" => (tree_sitter_python::LANGUAGE.into(), tree_sitter_python::HIGHLIGHTS_QUERY.replace("(decorator\n  (identifier) @function)", ""), "", ""),
        "rust" | "rs" => (tree_sitter_rust::LANGUAGE.into(), tree_sitter_rust::HIGHLIGHTS_QUERY.to_string(), tree_sitter_rust::INJECTIONS_QUERY, ""),
        "go" | "golang" => (tree_sitter_go::LANGUAGE.into(), tree_sitter_go::HIGHLIGHTS_QUERY.to_string(), "", ""),
        "ruby" | "rb" | "jruby" | "macruby" | "rake" | "rbx" => (tree_sitter_ruby::LANGUAGE.into(), tree_sitter_ruby::HIGHLIGHTS_QUERY.to_string(), "", tree_sitter_ruby::LOCALS_QUERY),
        "css" => (tree_sitter_css::LANGUAGE.into(), tree_sitter_css::HIGHLIGHTS_QUERY.to_string(), "", ""),
        "html" | "htm" | "xhtml" => (tree_sitter_html::LANGUAGE.into(), tree_sitter_html::HIGHLIGHTS_QUERY.to_string(), tree_sitter_html::INJECTIONS_QUERY, ""),
        "php" | "inc" => (tree_sitter_php::LANGUAGE_PHP.into(), tree_sitter_php::HIGHLIGHTS_QUERY.to_string(), tree_sitter_php::INJECTIONS_QUERY, ""),
        "nix" | "nixos" => (tree_sitter_nix::LANGUAGE.into(), nix_highlights(), tree_sitter_nix::INJECTIONS_QUERY, ""),
        _ => return None,
    };
    let mut config = HighlightConfiguration::new(language, name, &highlights, injections, locals).ok()?;
    let names: Vec<&str> = CAPTURES.iter().map(|(n, _)| *n).collect();
    config.configure(&names);
    Some(config)
}

/// Patterns removed from the Nix query. Once the query is reversed, each would color a whole attrpath on
/// top of the name inside it, so `builtins.length` would lose its function color. `(identifier) @property`
/// already colors these names.
const NIX_DROP: [&str; 2] = ["(select_expression\n  attrpath: (attrpath (identifier)) @property)", "(binding\n  attrpath: (attrpath (identifier)) @property)"];

/// The Nix query, changed to color code the way GitHub does: the first pattern that matches wins, and
/// built-in functions get the function color.
fn nix_highlights() -> String {
    let query = NIX_DROP.iter().fold(tree_sitter_nix::HIGHLIGHTS_QUERY.replace("@function.builtin", "@function"), |q, p| q.replace(p, ""));
    reversed(&query)
}

/// `query` with its top-level patterns in reverse order. When several patterns match one node,
/// tree-sitter-highlight uses the last one, but the Nix query is written for the first one to win.
fn reversed(query: &str) -> String {
    let b = query.as_bytes();
    let mut items: Vec<&str> = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b';' => i += query[i..].find('\n').unwrap_or(query.len() - i),
            b'(' | b'[' => {
                let start = i;
                let mut depth = 0;
                while i < b.len() {
                    match b[i] {
                        b'(' | b'[' => depth += 1,
                        b')' | b']' => depth -= 1,
                        b'"' => {
                            i += 1;
                            while i < b.len() && b[i] != b'"' {
                                i += if b[i] == b'\\' { 2 } else { 1 };
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                }
                // Trailing captures and quantifiers belong to the pattern.
                loop {
                    let rest = query[i..].trim_start_matches([' ', '\t']);
                    match rest.bytes().next() {
                        Some(b'@' | b'?' | b'*' | b'+') => {
                            let skip = query.len() - i - rest.len();
                            i += skip + rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
                        }
                        _ => break,
                    }
                }
                items.push(&query[start..i]);
            }
            _ => i += 1,
        }
    }
    items.reverse();
    items.join("\n\n")
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<HighlightConfiguration>>> = RefCell::new(HashMap::new());
}

/// Highlights a fenced block: per line of `code`, its `(start, end, token)` spans, plain text left out.
/// Tree-sitter grammars first, then TextMate ones; a language with neither comes back plain.
pub fn block(info: &str, code: &str) -> Runs {
    let lang = info.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    let body = code.strip_suffix('\n').unwrap_or(code);
    let ts = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.entry(lang.clone()).or_insert_with(|| ts_config(&lang)).as_ref().and_then(|config| ts_spans(config, body))
    });
    let spans = match ts {
        Some(spans) => spans.into_iter().map(|(s, e, class)| (s, e, tok_of(class))).collect(),
        None => {
            let mut spans = vec![];
            let mut line_start = 0;
            let mut line = 0;
            // TextMate reads `\r` as a line end, so a space of the same length keeps the offsets in line.
            let body = &body.replace('\r', " ");
            let found = super::textmate::tokens(&lang, body, |i, s, e, path| {
                while line < i {
                    line_start += body[line_start..].find('\n').map_or(body.len() - line_start, |n| n + 1);
                    line += 1;
                }
                let class = path.last().map_or("", |p| p.1);
                spans.push((line_start + s, line_start + e, tok_of(class)));
            });
            if found.is_some() { spans } else { vec![] }
        }
    };
    by_line(code, spans)
}

/// The token for a GitHub `pl-*` class.
fn tok_of(class: &str) -> Tok {
    match class {
        "pl-c" => Tok::Comment,
        "pl-k" => Tok::Keyword,
        "pl-ent" => Tok::Tag,
        "pl-s" | "pl-pds" | "pl-sr" => Tok::Str,
        "pl-s1" => Tok::Ident,
        "pl-c1" => Tok::Number,
        "pl-cce" => Tok::Escape,
        "pl-en" | "pl-e" => Tok::Func,
        "pl-v" => Tok::Var,
        "pl-smi" | "pl-sc" => Tok::Import,
        "pl-mi1" | "pl-mi2" => Tok::Added,
        "pl-md" => Tok::Removed,
        "pl-mh" | "pl-mdr" | "pl-ms" => Tok::Meta,
        _ => Tok::Plain,
    }
}

/// Splits `code`-wide spans, in order, into per-line spans with offsets from each line's start. Merges
/// neighbours of one token and drops plain text.
fn by_line(code: &str, spans: Vec<(usize, usize, Tok)>) -> Runs {
    let mut merged: Vec<(usize, usize, Tok)> = vec![];
    for (s, e, t) in spans {
        match merged.last_mut() {
            Some(r) if r.1 == s && r.2 == t => r.1 = e,
            _ if s < e => merged.push((s, e, t)),
            _ => {}
        }
    }
    let mut out = vec![];
    let mut line_start = 0;
    let mut runs = merged.into_iter().filter(|r| r.2 != Tok::Plain).peekable();
    for line in code.split('\n') {
        let end = line_start + line.len();
        let mut cur = vec![];
        while let Some(&(s, e, t)) = runs.peek() {
            if s > end {
                break;
            }
            let (s2, e2) = (s.max(line_start), e.min(end));
            if s2 < e2 {
                cur.push((s2 - line_start, e2 - line_start, t));
            }
            if e <= end + 1 {
                runs.next();
            } else {
                break;
            }
        }
        out.push(cur);
        line_start = end + 1;
    }
    out
}

/// Tree-sitter's spans over `code`, each with the class of its innermost capture.
fn ts_spans(config: &HighlightConfiguration, code: &str) -> Option<Vec<(usize, usize, &'static str)>> {
    let mut hl = Highlighter::new();
    let events = hl.highlight(config, code.as_bytes(), None, None, |_| None).ok()?;
    let mut out = vec![];
    let mut open: Vec<&'static str> = vec![];
    for ev in events {
        match ev.ok()? {
            HighlightEvent::HighlightStart(h) => open.push(CAPTURES[h.0].1),
            HighlightEvent::HighlightEnd => {
                open.pop();
            }
            HighlightEvent::Source { start, end } => out.push((start, end, open.last().copied().unwrap_or(""))),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class tree-sitter gives the first `needle` in `code`.
    fn class_of(lang: &str, code: &str, needle: &str) -> &'static str {
        let config = ts_config(lang).unwrap();
        let at = code.find(needle).unwrap();
        let spans = ts_spans(&config, code).unwrap();
        let (s, e, c) = spans.into_iter().find(|&(s, e, _)| s <= at && at < e).unwrap();
        assert_eq!(&code[s..e], needle, "{lang}: {needle:?} is split");
        c
    }

    #[test]
    fn ruby_symbols_get_github_class() {
        assert_eq!(class_of("ruby", "f(:a, b: 1)\n", ":a"), "pl-pds");
        assert_eq!(class_of("ruby", "f(:a, b: 1)\n", "b"), "pl-pds");
    }

    #[test]
    fn ruby_builtins_and_regexes_get_github_class() {
        let code = "require \"a\"\ndefined?(x)\n/b/\n";
        assert_eq!(class_of("ruby", code, "require"), "pl-en");
        assert_eq!(class_of("ruby", code, "defined?"), "pl-en");
        assert_eq!(class_of("ruby", code, "/b/"), "pl-sr");
    }

    #[test]
    fn nix_gets_github_class() {
        let code = "{ a = import b; c = builtins.length d; e = true; f = x: x; }\n";
        assert_eq!(class_of("nix", code, "a"), "pl-c1");
        assert_eq!(class_of("nix", code, "import"), "pl-en");
        assert_eq!(class_of("nix", code, "b"), "pl-s1");
        assert_eq!(class_of("nix", code, "length"), "pl-en");
        assert_eq!(class_of("nix", code, "true"), "pl-smi");
        // A crate update that rewrites these patterns would make the removal do nothing.
        assert!(NIX_DROP.iter().all(|p| tree_sitter_nix::HIGHLIGHTS_QUERY.contains(p)));
    }

    #[test]
    fn reversed_keeps_each_pattern_whole() {
        let q = "; note\n(a) @x\n\n[\n  \"(\"\n  \"]\"\n] @y\n((b) @z (#match? @z \"^c$\"))";
        assert_eq!(reversed(q), "((b) @z (#match? @z \"^c$\"))\n\n[\n  \"(\"\n  \"]\"\n] @y\n\n(a) @x");
    }

    #[test]
    fn runs_are_per_line() {
        let runs = block("rust", "let s = \"<a>\"; // hi");
        assert_eq!(runs.len(), 1);
        assert!(runs[0].iter().any(|r| r.2 == Tok::Comment) && runs[0].iter().any(|r| r.2 == Tok::Str), "{runs:?}");
        // TextMate-only languages too, and a trailing newline gives an empty last line.
        let yaml = block("yaml", "a: 1\n# c\n");
        assert_eq!(yaml.len(), 3);
        assert_eq!(yaml[1], [(0, 3, Tok::Comment)]);
        // CRLF lines keep their offsets.
        let crlf = block("yaml", "a: 1\r\n# c\r\n");
        assert_eq!((crlf[1][0].0, crlf[1][0].2), (0, Tok::Comment), "{crlf:?}");
        assert_eq!(block("nosuchlang", "x\ny"), [vec![], vec![]]);
    }
}
