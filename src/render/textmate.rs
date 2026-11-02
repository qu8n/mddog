use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use irosashi::{CodeToTokensOptions, Highlighter, HighlighterBuilder};

use super::escape;

// `GRAMMARS`: each grammar's JSON, deflated by build.rs, sorted by scope.
include!(concat!(env!("OUT_DIR"), "/textmate/textmate.rs"));
const ALIASES: &[u8] = include_bytes!("../../assets/textmate/aliases.json");
const SCOPES: &[u8] = include_bytes!("../../assets/textmate/scopes.json");
const THEME: &[u8] = br##"{"name":"none","type":"dark","colors":{"editor.foreground":"#000000","editor.background":"#000000"},"tokenColors":[]}"##;

/// A page pays only for its own languages: a fence's grammar and the grammars it can include are inflated on
/// first use, and the highlighter is built again over them. Building reads only each grammar's header; a
/// grammar is parsed when tokenizing first reaches it.
struct Tm {
    highlighter: RefCell<Highlighter>,
    json: RefCell<BTreeMap<String, Arc<[u8]>>>,
    aliases: HashMap<String, String>,
    classes: HashMap<String, String>,
}

thread_local! {
    static TM: OnceCell<Option<Tm>> = const { OnceCell::new() };
}

fn load() -> Option<Tm> {
    let by_scope: BTreeMap<String, Vec<String>> = serde_json::from_slice(ALIASES).ok()?;
    let mut aliases = HashMap::new();
    for (scope, names) in by_scope {
        for n in names {
            // A name two grammars claim goes to the first scope in order.
            aliases.entry(n).or_insert_with(|| scope.clone());
        }
    }
    let highlighter = build(&BTreeMap::new())?;
    Some(Tm { highlighter: RefCell::new(highlighter), json: RefCell::default(), aliases, classes: serde_json::from_slice(SCOPES).ok()? })
}

fn build(json: &BTreeMap<String, Arc<[u8]>>) -> Option<Highlighter> {
    let mut builder = HighlighterBuilder::without_bundled_assets().theme("none", THEME).ok()?;
    for (scope, bytes) in json {
        builder = builder.grammar(scope, bytes.clone()).ok()?;
    }
    builder.build().ok()
}

fn deflated(scope: &str) -> Option<&'static [u8]> {
    GRAMMARS.binary_search_by_key(&scope, |g| g.0).ok().map(|i| GRAMMARS[i].1)
}

/// The other grammars `json` includes, as `"include":"source.js#expression"` names them.
fn includes(json: &[u8]) -> Vec<String> {
    const KEY: &str = "\"include\":\"";
    let text = std::str::from_utf8(json).unwrap_or("");
    text.match_indices(KEY)
        .filter_map(|(i, _)| {
            let rest = &text[i + KEY.len()..];
            let name = &rest[..rest.find(['"', '#'])?];
            (!name.is_empty() && !name.starts_with('$')).then(|| name.to_string())
        })
        .collect()
}

impl Tm {
    /// The grammar scope for a fence's language name, made ready to tokenize.
    fn scope(&self, lang: &str) -> Option<String> {
        let scope = if deflated(lang).is_some() { lang } else { self.aliases.get(lang)? };
        let mut json = self.json.borrow_mut();
        if json.contains_key(scope) {
            return Some(scope.to_string());
        }
        let mut todo = vec![scope.to_string()];
        while let Some(s) = todo.pop() {
            if json.contains_key(&s) {
                continue;
            }
            let Some(bytes) = deflated(&s).and_then(|z| miniz_oxide::inflate::decompress_to_vec_zlib(z).ok()) else { continue };
            todo.extend(includes(&bytes));
            json.insert(s, bytes.into());
        }
        *self.highlighter.borrow_mut() = build(&json)?;
        Some(scope.to_string())
    }
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
        let scope = tm.scope(lang)?;
        let text = code.replace("\r\n", "\n").replace('\r', "\n");
        let mut opts = CodeToTokensOptions::new(&scope, "none");
        opts.include_scopes = true;
        opts.max_line_length = Some(0);
        let result = tm.highlighter.borrow().code_to_tokens(&text, &opts).ok()?;
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
        Some((scope, out))
    })
}

#[cfg(test)]
mod tests {
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

    #[test]
    fn grammars_are_sorted_for_binary_search() {
        assert!(super::GRAMMARS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn matches_golden() {
        let mut out = String::new();
        for f in ["reference/probes/langs.md", "fixtures/gfm.md"] {
            let md = std::fs::read_to_string(f).unwrap();
            let mut block: Option<(String, String)> = None;
            for ev in Parser::new(&md) {
                match ev {
                    Event::Start(Tag::CodeBlock(kind)) => {
                        let info = if let CodeBlockKind::Fenced(i) = kind { i.to_string() } else { String::new() };
                        block = Some((info.split_whitespace().next().unwrap_or("").to_ascii_lowercase(), String::new()));
                    }
                    Event::Text(t) => {
                        if let Some(b) = block.as_mut() {
                            b.1.push_str(&t);
                        }
                    }
                    Event::End(TagEnd::CodeBlock) => {
                        let Some((lang, literal)) = block.take() else { continue };
                        // A fresh thread starts with no grammars, so each block proves its language loads what it includes.
                        let code = literal.strip_suffix('\n').unwrap_or(&literal).to_string();
                        let lang2 = lang.clone();
                        if let Some((scope, html)) = std::thread::spawn(move || super::highlight(&lang2, &code)).join().unwrap() {
                            out.push_str(&format!("--- {f} {lang} {scope}\n{html}\n"));
                        }
                    }
                    _ => {}
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
