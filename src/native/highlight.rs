//! A small token highlighter: keywords, strings, comments, numbers, calls. No regex.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    Func,
    Type,
    Punct,
    Added,
    Removed,
    Meta,
}

struct Lang {
    kw: &'static [&'static str],
    line_comment: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static str,
    triple: bool,
}

const JS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "async",
    "await",
    "of",
    "true",
    "false",
    "null",
    "undefined",
    "interface",
    "type",
    "enum",
    "implements",
    "public",
    "private",
    "protected",
    "readonly",
    "from",
    "as",
];
const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
];
const PY: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
    "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "self",
];
const SH: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "in", "function", "return", "export", "local", "set", "echo", "cd", "exit", "source", "unset", "readonly",
    "shift", "trap", "select",
];
const C: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "class",
    "namespace",
    "new",
    "delete",
    "public",
    "private",
    "protected",
    "template",
    "typename",
    "using",
    "bool",
    "true",
    "false",
    "nullptr",
    "this",
    "virtual",
    "override",
    "final",
    "try",
    "catch",
    "throw",
];
const GO: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "true",
    "false",
    "nil",
];
const JSON: &[&str] = &["true", "false", "null"];
const SQL: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "drop", "alter", "join", "left", "right", "inner", "outer", "on", "group", "by", "order",
    "having", "limit", "and", "or", "not", "null", "as", "distinct", "union", "index", "primary", "key", "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE",
    "TABLE", "DROP", "ALTER", "JOIN", "ON", "GROUP", "BY", "ORDER", "HAVING", "LIMIT", "AND", "OR", "NOT", "NULL", "AS",
];
const NONE: &[&str] = &[];

fn lang(name: &str) -> Option<Lang> {
    let l = |kw, lc: &'static [&'static str], block, quotes, triple| Lang { kw, line_comment: lc, block, quotes, triple };
    Some(match name.to_ascii_lowercase().as_str() {
        "js" | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "mjs" | "cjs" => l(JS, &["//"], Some(("/*", "*/")), "\"'`", false),
        "rust" | "rs" => l(RUST, &["//"], Some(("/*", "*/")), "\"", false),
        "python" | "py" | "python3" => l(PY, &["#"], None, "\"'", true),
        "bash" | "sh" | "shell" | "zsh" | "console" | "shellsession" => l(SH, &["#"], None, "\"'", false),
        "c" | "cpp" | "c++" | "h" | "hpp" | "java" | "cs" | "csharp" | "swift" | "kotlin" | "objc" => l(C, &["//"], Some(("/*", "*/")), "\"'", false),
        "go" | "golang" => l(GO, &["//"], Some(("/*", "*/")), "\"'`", false),
        "json" | "jsonc" | "json5" => l(JSON, &[], None, "\"", false),
        "sql" => l(SQL, &["--"], Some(("/*", "*/")), "\"'", false),
        "yaml" | "yml" | "toml" | "ini" => l(JSON, &["#"], None, "\"'", false),
        "ruby" | "rb" => l(PY, &["#"], None, "\"'", false),
        "html" | "xml" | "svg" => l(NONE, &[], Some(("<!--", "-->")), "\"'", false),
        "css" | "scss" => l(NONE, &[], Some(("/*", "*/")), "\"'", false),
        _ => return None,
    })
}

/// Returns, per source line, `(start_byte, end_byte, token)` spans, from mddog's highlighter (tree-sitter and
/// TextMate grammars), read back from its `pl-*` spans. `MDDOG_NATIVE_HL=b` uses the rebuild's small highlighter.
pub fn highlight(langname: &str, text: &str) -> Vec<Vec<(usize, usize, Tok)>> {
    if std::env::var("MDDOG_NATIVE_HL").as_deref() == Ok("b") || langname.is_empty() {
        return highlight_b(langname, text);
    }
    from_html(&crate::render::highlight::block(langname, text), text).unwrap_or_else(|| highlight_b(langname, text))
}

fn tok_of(class: &str) -> Tok {
    match class.split_whitespace().next().unwrap_or("") {
        "pl-c" => Tok::Comment,
        "pl-k" | "pl-ent" => Tok::Keyword,
        "pl-s" | "pl-pds" | "pl-sr" | "pl-s1" => Tok::Str,
        "pl-c1" | "pl-cce" => Tok::Number,
        "pl-en" | "pl-e" => Tok::Func,
        "pl-smi" | "pl-v" | "pl-sc" => Tok::Type,
        "pl-mi1" | "pl-mi2" => Tok::Added,
        "pl-md" => Tok::Removed,
        "pl-mh" | "pl-mdr" | "pl-ms" => Tok::Meta,
        _ => Tok::Plain,
    }
}

/// Reads `<pre>`'s spans back into per-line runs of `text`. None when the text doesn't line up.
fn from_html(html: &str, text: &str) -> Option<Vec<Vec<(usize, usize, Tok)>>> {
    let a = html.find("<pre")?;
    let body = &html[a + html[a..].find('>')? + 1..];
    let body = &body[..body.find("</pre>")?];
    let mut stack: Vec<Tok> = vec![];
    let mut runs: Vec<(usize, usize, Tok)> = vec![];
    let mut pos = 0usize;
    let tb = text.as_bytes();
    let mut i = 0;
    let b = body.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let end = i + body[i..].find('>')?;
            let tag = &body[i + 1..end];
            if let Some(rest) = tag.strip_prefix("span class=\"") {
                stack.push(tok_of(rest.trim_end_matches('"')));
            } else if tag.starts_with("/span") {
                stack.pop();
            } else if tag.starts_with("code") || tag.starts_with("/code") {
            } else {
                return None;
            }
            i = end + 1;
            continue;
        }
        let (ch, len) = if b[i] == b'&' {
            let semi = i + body[i..].find(';')?;
            let c = match &body[i + 1..semi] {
                "lt" => b'<',
                "gt" => b'>',
                "amp" => b'&',
                "quot" => b'"',
                "#39" | "#x27" => b'\'',
                _ => return None,
            };
            (c, semi + 1 - i)
        } else {
            (b[i], 1)
        };
        if pos >= tb.len() {
            break;
        }
        if tb[pos] != ch {
            return None;
        }
        let t = stack.last().copied().unwrap_or(Tok::Plain);
        match runs.last_mut() {
            Some(r) if r.1 == pos && r.2 == t => r.1 += 1,
            _ => runs.push((pos, pos + 1, t)),
        }
        pos += 1;
        i += len;
    }
    // Split the runs at newlines into per-line spans with byte offsets relative to the line.
    let mut out: Vec<Vec<(usize, usize, Tok)>> = vec![];
    let mut line_start = 0;
    let mut cur: Vec<(usize, usize, Tok)> = vec![];
    let mut ri = 0;
    for (k, c) in tb.iter().enumerate().chain(std::iter::once((tb.len(), &b'\n'))) {
        if *c == b'\n' {
            while ri < runs.len() && runs[ri].0 < k {
                let (s, e, t) = runs[ri];
                let (s2, e2) = (s.max(line_start), e.min(k));
                if s2 < e2 && t != Tok::Plain {
                    cur.push((s2 - line_start, e2 - line_start, t));
                }
                if e <= k {
                    ri += 1;
                } else {
                    break;
                }
            }
            out.push(std::mem::take(&mut cur));
            line_start = k + 1;
        }
    }
    Some(out)
}

fn highlight_b(langname: &str, text: &str) -> Vec<Vec<(usize, usize, Tok)>> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<Vec<(usize, usize, Tok)>> = Vec::with_capacity(lines.len());
    if matches!(langname.to_ascii_lowercase().as_str(), "diff" | "patch") {
        for l in &lines {
            let t = if l.starts_with('+') && !l.starts_with("+++") {
                Tok::Added
            } else if l.starts_with('-') && !l.starts_with("---") {
                Tok::Removed
            } else if l.starts_with("@@") || l.starts_with("+++") || l.starts_with("---") || l.starts_with("diff ") {
                Tok::Meta
            } else {
                Tok::Plain
            };
            out.push(vec![(0, l.len(), t)]);
        }
        return out;
    }
    let Some(lg) = lang(langname) else {
        return lines.iter().map(|l| vec![(0, l.len(), Tok::Plain)]).collect();
    };
    let json = matches!(langname.to_ascii_lowercase().as_str(), "json" | "jsonc" | "json5");
    // 0 none, 1 in block comment, 2 in triple string(quote byte stored)
    let mut state: u8 = 0;
    let mut tq: u8 = b'"';
    for line in lines {
        let b = line.as_bytes();
        let mut spans: Vec<(usize, usize, Tok)> = vec![];
        let mut i = 0;
        let mut push = |s: usize, e: usize, t: Tok, spans: &mut Vec<(usize, usize, Tok)>| {
            if e > s {
                if let Some(l) = spans.last_mut() {
                    if l.2 == t && l.1 == s {
                        l.1 = e;
                        return;
                    }
                }
                spans.push((s, e, t));
            }
        };
        while i < b.len() {
            if state == 1 {
                let end = lg.block.unwrap().1;
                match line[i..].find(end) {
                    Some(p) => {
                        push(i, i + p + end.len(), Tok::Comment, &mut spans);
                        i += p + end.len();
                        state = 0;
                    }
                    None => {
                        push(i, b.len(), Tok::Comment, &mut spans);
                        i = b.len();
                    }
                }
                continue;
            }
            if state == 2 {
                let pat = [tq; 3];
                match line[i..].as_bytes().windows(3).position(|w| w == pat) {
                    Some(p) => {
                        push(i, i + p + 3, Tok::Str, &mut spans);
                        i += p + 3;
                        state = 0;
                    }
                    None => {
                        push(i, b.len(), Tok::Str, &mut spans);
                        i = b.len();
                    }
                }
                continue;
            }
            let rest = &line[i..];
            if lg.line_comment.iter().any(|c| rest.starts_with(c)) {
                push(i, b.len(), Tok::Comment, &mut spans);
                break;
            }
            if let Some((s, _)) = lg.block {
                if rest.starts_with(s) {
                    state = 1;
                    push(i, i + s.len(), Tok::Comment, &mut spans);
                    i += s.len();
                    continue;
                }
            }
            let c = b[i];
            if lg.quotes.as_bytes().contains(&c) {
                if lg.triple && rest.as_bytes().starts_with(&[c, c, c]) {
                    state = 2;
                    tq = c;
                    push(i, i + 3, Tok::Str, &mut spans);
                    i += 3;
                    continue;
                }
                let mut j = i + 1;
                while j < b.len() && b[j] != c {
                    if b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let j = (j + 1).min(b.len());
                // json keys
                let is_key = json && line[j..].trim_start().starts_with(':');
                push(i, j, if is_key { Tok::Type } else { Tok::Str }, &mut spans);
                i = j;
                continue;
            }
            if c.is_ascii_digit() {
                let mut j = i;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'.' || b[j] == b'_') {
                    j += 1;
                }
                push(i, j, Tok::Number, &mut spans);
                i = j;
                continue;
            }
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let mut j = i;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'$') {
                    j += 1;
                }
                let w = &line[i..j];
                let t = if lg.kw.contains(&w) {
                    Tok::Keyword
                } else if line[j..].starts_with('(') {
                    Tok::Func
                } else if w.chars().next().unwrap().is_ascii_uppercase() && w.len() > 1 {
                    Tok::Type
                } else {
                    Tok::Plain
                };
                push(i, j, t, &mut spans);
                i = j;
                continue;
            }
            let n = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            push(i, i + n, if "=<>!+-*/%&|^~?:".contains(c as char) { Tok::Punct } else { Tok::Plain }, &mut spans);
            i += n;
        }
        out.push(spans);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mddogs_spans() {
        let html = crate::render::highlight::block("rust", "let s = \"<a>\"; // hi");
        let runs = from_html(&html, "let s = \"<a>\"; // hi").expect("lines up");
        assert_eq!(runs.len(), 1);
        assert!(runs[0].iter().any(|r| r.2 == Tok::Comment) && runs[0].iter().any(|r| r.2 == Tok::Str), "{runs:?}");
        // TextMate-only languages read back too.
        let yaml = "a: 1\n# c\n";
        assert!(from_html(&crate::render::highlight::block("yaml", yaml), yaml.trim_end()).is_some());
    }
}
