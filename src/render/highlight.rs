use std::cell::RefCell;
use std::collections::HashMap;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use super::markdown::escape;

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

struct Lang {
    scope: &'static str,
    config: HighlightConfiguration,
}

fn ts_config(name: &str) -> Option<(&'static str, HighlightConfiguration)> {
    let (scope, language, highlights, injections, locals): (&str, tree_sitter::Language, String, &str, &str) = match name {
        "js" | "javascript" | "jsx" | "mjs" | "cjs" | "node" => (
            "source.js",
            tree_sitter_javascript::LANGUAGE.into(),
            format!("{}\n{}", tree_sitter_javascript::JSX_HIGHLIGHT_QUERY, tree_sitter_javascript::HIGHLIGHT_QUERY),
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ),
        "ts" | "typescript" | "mts" | "cts" => (
            "source.ts",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            format!("{}\n{}", tree_sitter_typescript::HIGHLIGHTS_QUERY, tree_sitter_javascript::HIGHLIGHT_QUERY),
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "tsx" => (
            "source.tsx",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            format!("{}\n{}\n{}", tree_sitter_typescript::HIGHLIGHTS_QUERY, tree_sitter_javascript::JSX_HIGHLIGHT_QUERY, tree_sitter_javascript::HIGHLIGHT_QUERY),
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "python" | "py" | "python3" | "py3" | "gyp" => {
            ("source.python", tree_sitter_python::LANGUAGE.into(), tree_sitter_python::HIGHLIGHTS_QUERY.replace("(decorator\n  (identifier) @function)", ""), "", "")
        }
        "rust" | "rs" => ("source.rust", tree_sitter_rust::LANGUAGE.into(), tree_sitter_rust::HIGHLIGHTS_QUERY.to_string(), tree_sitter_rust::INJECTIONS_QUERY, ""),
        "go" | "golang" => ("source.go", tree_sitter_go::LANGUAGE.into(), tree_sitter_go::HIGHLIGHTS_QUERY.to_string(), "", ""),
        "ruby" | "rb" | "jruby" | "macruby" | "rake" | "rbx" => ("source.ruby", tree_sitter_ruby::LANGUAGE.into(), tree_sitter_ruby::HIGHLIGHTS_QUERY.to_string(), "", tree_sitter_ruby::LOCALS_QUERY),
        "css" => ("source.css", tree_sitter_css::LANGUAGE.into(), tree_sitter_css::HIGHLIGHTS_QUERY.to_string(), "", ""),
        "html" | "htm" | "xhtml" => ("text.html.basic", tree_sitter_html::LANGUAGE.into(), tree_sitter_html::HIGHLIGHTS_QUERY.to_string(), tree_sitter_html::INJECTIONS_QUERY, ""),
        "php" | "inc" => ("text.html.php", tree_sitter_php::LANGUAGE_PHP.into(), tree_sitter_php::HIGHLIGHTS_QUERY.to_string(), tree_sitter_php::INJECTIONS_QUERY, ""),
        _ => return None,
    };
    let mut config = HighlightConfiguration::new(language, name, &highlights, injections, locals).ok()?;
    let names: Vec<&str> = CAPTURES.iter().map(|(n, _)| *n).collect();
    config.configure(&names);
    Some((scope, config))
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<Lang>>> = RefCell::new(HashMap::new());
}

pub fn block(info: &str, code: &str) -> String {
    let lang = info.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    let code = code.strip_suffix('\n').unwrap_or(code);
    let ts = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let entry = c.entry(lang.clone()).or_insert_with(|| ts_config(&lang).map(|(scope, config)| Lang { scope, config }));
        entry.as_ref().map(|l| (l.scope, tree_sitter_html_spans(&l.config, code)))
    });
    if let Some((scope, Some(body))) = ts {
        return wrap(scope, &body, code);
    }
    if let Some((scope, body)) = super::textmate::highlight(&lang, code) {
        return wrap(&scope, &body, code);
    }
    copy_box("snippet-clipboard-content", &format!("<pre lang=\"{}\" class=\"notranslate\"><code class=\"notranslate\">{}\n</code></pre>", escape(&lang), escape(code)), code)
}

fn wrap(scope: &str, body: &str, code: &str) -> String {
    copy_box(&format!("highlight highlight-{}", scope.replace('.', "-")), &format!("<pre class=\"notranslate\">{body}</pre>"), code)
}

pub fn copy_box(class: &str, pre: &str, code: &str) -> String {
    format!(
        "<div class=\"{class} notranslate position-relative overflow-auto\">{pre}<div class=\"zeroclipboard-container\"><clipboard-copy aria-label=\"Copy code to clipboard\" class=\"ClipboardButton btn btn-invisible js-clipboard-copy m-2 p-0 d-flex flex-justify-center flex-items-center\" data-copy-feedback=\"Copied!\" data-tooltip-direction=\"w\" value=\"{}\" tabindex=\"0\" role=\"button\">{}</clipboard-copy></div></div>",
        escape(code),
        super::octicons::CODE_COPY
    )
}

fn tree_sitter_html_spans(config: &HighlightConfiguration, code: &str) -> Option<String> {
    let mut hl = Highlighter::new();
    let events = hl.highlight(config, code.as_bytes(), None, None, |_| None).ok()?;
    let mut out = String::with_capacity(code.len() * 2);
    let mut open: Vec<&str> = Vec::new();
    for ev in events {
        match ev.ok()? {
            HighlightEvent::HighlightStart(h) => {
                let class = CAPTURES[h.0].1;
                out.push_str("<span class=\"");
                out.push_str(class);
                out.push_str("\">");
                open.push(class);
            }
            HighlightEvent::HighlightEnd => {
                open.pop();
                out.push_str("</span>");
            }
            HighlightEvent::Source { start, end } => push_lines(&mut out, &code[start..end], &open),
        }
    }
    Some(out)
}

pub fn push_lines(out: &mut String, text: &str, open: &[&str]) {
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            for _ in open {
                out.push_str("</span>");
            }
            out.push('\n');
            for class in open {
                out.push_str("<span class=\"");
                out.push_str(class);
                out.push_str("\">");
            }
        }
        first = false;
        out.push_str(&escape(line));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ruby_symbols_get_github_class() {
        let html = super::block("ruby", "f(:a, b: 1)\n");
        assert!(html.contains(r#"<span class="pl-pds">:a</span>"#), "{html}");
        assert!(html.contains(r#"<span class="pl-pds">b</span>"#), "{html}");
    }

    #[test]
    fn ruby_builtins_and_regexes_get_github_class() {
        let html = super::block("ruby", "require \"a\"\ndefined?(x)\n/b/\n");
        assert!(html.contains(r#"<span class="pl-en">require</span>"#), "{html}");
        assert!(html.contains(r#"<span class="pl-en">defined?</span>"#), "{html}");
        assert!(html.contains(r#"<span class="pl-sr">/b/</span>"#), "{html}");
    }
}
