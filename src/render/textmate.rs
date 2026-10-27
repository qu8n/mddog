use std::cell::OnceCell;
use std::collections::{BTreeMap, HashMap};

use include_dir::{Dir, include_dir};
use irosashi::{CodeToTokensOptions, Highlighter, HighlighterBuilder};

use super::markdown::escape;

static DATA: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets/textmate");
const THEME: &[u8] = br##"{"name":"none","type":"dark","colors":{"editor.foreground":"#000000","editor.background":"#000000"},"tokenColors":[]}"##;

struct Tm {
    highlighter: Highlighter,
    scopes: HashMap<String, String>,
    classes: HashMap<String, String>,
}

thread_local! {
    static TM: OnceCell<Option<Tm>> = const { OnceCell::new() };
}

fn load() -> Option<Tm> {
    let mut builder = HighlighterBuilder::without_bundled_assets().theme("none", THEME).ok()?;
    let mut scopes = HashMap::new();
    for f in DATA.get_dir("grammars")?.files() {
        let scope = f.path().file_stem()?.to_str()?;
        builder = builder.grammar(scope, f.contents()).ok()?;
        scopes.insert(scope.to_string(), scope.to_string());
    }
    let aliases: BTreeMap<String, Vec<String>> = serde_json::from_slice(DATA.get_file("aliases.json")?.contents()).ok()?;
    for (scope, names) in aliases {
        for n in names {
            scopes.entry(n).or_insert_with(|| scope.clone());
        }
    }
    let classes = serde_json::from_slice(DATA.get_file("scopes.json")?.contents()).ok()?;
    Some(Tm { highlighter: builder.build().ok()?, scopes, classes })
}

fn class_for<'a>(classes: &'a HashMap<String, String>, scope: &str) -> Option<&'a str> {
    let mut s = scope;
    loop {
        if let Some(c) = classes.get(s) {
            return Some(c);
        }
        s = &s[..s.rfind('.')?];
    }
}

pub fn highlight(lang: &str, code: &str) -> Option<(String, String)> {
    TM.with(|cell| {
        let tm = cell.get_or_init(load).as_ref()?;
        let scope = tm.scopes.get(lang)?;
        let text = code.replace("\r\n", "\n").replace('\r', "\n");
        let mut opts = CodeToTokensOptions::new(scope, "none");
        opts.include_scopes = true;
        opts.max_line_length = Some(0);
        let result = tm.highlighter.code_to_tokens(&text, &opts).ok()?;
        if !result.diagnostics.is_empty() {
            return None;
        }
        let mut out = String::with_capacity(code.len() * 2);
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let mut open: Vec<(&str, &str)> = Vec::new();
            for token in result.lines.get(i).map_or(&[][..], |l| &l.tokens[..]) {
                let path: Vec<(&str, &str)> = result.scopes_of(token)?.iter().skip(1).filter_map(|s| class_for(&tm.classes, s).map(|c| (&**s, c))).collect();
                let keep = open.iter().zip(&path).take_while(|(a, b)| a.0 == b.0).count();
                for _ in keep..open.len() {
                    out.push_str("</span>");
                }
                open.truncate(keep);
                for (s, c) in &path[keep..] {
                    out.push_str("<span class=\"");
                    out.push_str(c);
                    out.push_str("\">");
                    open.push((s, c));
                }
                out.push_str(&escape(line.get(token.start..token.end).unwrap_or("")));
            }
            for _ in &open {
                out.push_str("</span>");
            }
        }
        Some((scope.clone(), out))
    })
}

#[cfg(test)]
mod tests {
    use comrak::nodes::NodeValue;
    use comrak::{Arena, parse_document};

    #[test]
    fn matches_golden() {
        let mut out = String::new();
        for f in ["reference/probes/langs.md", "fixtures/gfm.md"] {
            let md = std::fs::read_to_string(f).unwrap();
            let arena = Arena::new();
            for node in parse_document(&arena, &md, &crate::render::markdown::options()).descendants() {
                if let NodeValue::CodeBlock(cb) = &node.data.borrow().value {
                    let lang = cb.info.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                    if let Some((scope, html)) = super::highlight(&lang, cb.literal.strip_suffix('\n').unwrap_or(&cb.literal)) {
                        out.push_str(&format!("--- {f} {lang} {scope}\n{html}\n"));
                    }
                }
            }
        }
        let golden = "tests/textmate.golden";
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(golden, &out).unwrap();
        }
        assert_eq!(out, std::fs::read_to_string(golden).unwrap());
    }
}
