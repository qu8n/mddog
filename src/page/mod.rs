use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::engine::Engine;
use crate::render::markdown::{self, Doc};
use crate::render::mdx;
use crate::term;
use crate::theme::Theme;
use crate::{assets, config, trace};

pub mod remote;
pub mod script;

const RUNTIME_JS: &str = include_str!("../../assets/mddog.js");

pub struct Opts<'a> {
    pub theme: &'a Theme,
    pub base: &'a Path,
    pub pad_x: f64,
    pub pad_y: f64,
    pub math: bool,
    pub mermaid: bool,
    pub engine: config::Mermaid,
    pub wrap: bool,
    pub runtime: Option<&'a Path>,
}

/// The page's padding in CSS pixels. The loading skeleton uses it too.
pub const PAD: f64 = 24.0;

pub fn doc(file: &Path, bytes: &[u8], base: &Path, spare: impl FnOnce(&str, &Path) -> Option<Doc>) -> (String, Doc) {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let src = if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("mdx")) { mdx::to_markdown(&text) } else { text };
    let shown = if src.trim().is_empty() { "*This file is empty.*" } else { &src };
    let doc = spare(shown, base).unwrap_or_else(|| markdown::render_in(shown, Some(base)));
    (src, doc)
}

pub fn base_dir(file: &Path, stdin: bool) -> PathBuf {
    if stdin { std::env::current_dir().ok() } else { std::fs::canonicalize(file).ok().and_then(|p| p.parent().map(Path::to_path_buf)) }.unwrap_or_else(|| PathBuf::from("."))
}

/// Builds the page in the current theme, and returns it with that theme.
pub fn html(doc: &Doc, base: &Path, wrap: bool) -> (String, &'static Theme) {
    let runtime = (doc.math || doc.mermaid).then(assets::runtime_dir);
    let theme = config::theme();
    let opts = Opts { theme, base, pad_x: PAD, pad_y: PAD, math: doc.math, mermaid: doc.mermaid, engine: config::mermaid(theme), wrap, runtime: runtime.as_deref() };
    (build(&doc.html, &opts), theme)
}

pub(crate) fn file_url(p: &Path) -> String {
    format!("file://{}", percent_path(&p.to_string_lossy()))
}

/// The runtime every style loads, plus MathJax when the page has math.
pub(crate) fn scripts(o: &Opts) -> String {
    let mut out = format!("<script>{RUNTIME_JS}</script>");
    if let (true, Some(rt)) = (o.math, o.runtime) {
        out.push_str(&format!("<script>mddog.mathConfig()</script><script defer src=\"{}\"></script>", file_url(&rt.join("mathjax/tex-mml-chtml.js"))));
    }
    out
}

/// Writes the page to `path`, readable by its owner only, loads it within `wait`, then gives its fonts and images up
/// to 500 ms. The file must stay while the page is open: WebKit won't load the Mermaid frames' `file:` scripts once
/// it is gone.
pub fn load(engine: &Engine, path: &Path, html: &str, dark: bool, wait: Duration) -> Result<(), String> {
    crate::sweep::write_private(path, html.as_bytes()).map_err(|e| format!("cannot write the page: {}", term::io_error(&e)))?;
    trace::mark("page written");
    engine.load_file(path, dark);
    if !engine.wait_loaded(wait) {
        return Err("the page did not load".into());
    }
    engine.wait_js(script::READY, Duration::from_millis(500));
    Ok(())
}

pub fn build(article: &str, o: &Opts) -> String {
    (crate::style::DEFAULT.page)(article, o)
}

pub(crate) fn percent_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_reads_mdx_and_fills_an_empty_file() {
        let (src, empty) = doc(Path::new("a.md"), b"  \n", Path::new("."), |_, _| None);
        assert_eq!(src, "  \n");
        assert!(empty.html.contains("This file is empty"));
        let (src, _) = doc(Path::new("a.MDX"), b"import X from './x'\n\n# Hi\n", Path::new("."), |_, _| None);
        assert!(!src.contains("import"));
        let (_, spare) = doc(Path::new("a.md"), b"# Hi", Path::new("."), |s, _| Some(markdown::render(&format!("{s} from the spare"))));
        assert!(spare.html.contains("from the spare"));
    }
}
