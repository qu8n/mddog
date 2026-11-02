use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, format_html, parse_document};
use kuchikiki::NodeRef;
use kuchikiki::iter::NodeIterator;

use super::dom::*;
use super::{emoji, highlight, octicons, sanitize};

pub const RUN_ID: &str = "0f23d3dd37596070b592c1adaa9bf6c8";

#[derive(Default)]
pub struct Doc {
    pub html: String,
    pub math: bool,
    pub mermaid: bool,
}

pub fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.render.r#unsafe = true;
    o.render.github_pre_lang = true;
    o
}

fn escape_math(s: &str) -> String {
    s.replace('\\', "\\\\").replace('*', "\\*").replace('_', "\\_")
}

pub(crate) const HELD_DOLLAR: char = '\u{e024}';

fn hold_dollars(text: &str, masked: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut slashes = 0;
    for (i, c) in text.char_indices() {
        if c == '$' && slashes % 2 == 1 && masked[i..].starts_with('$') {
            out.pop();
            out.push(HELD_DOLLAR);
        } else {
            out.push(c);
        }
        slashes = if c == '\\' { slashes + 1 } else { 0 };
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Fence {
    Outside,
    Opens,
    Inside,
}

pub(crate) fn protect_math(src: &str) -> String {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let delim = |line: &str| {
        let u = unquote(line).0;
        u.trim() == "$$" && !u.starts_with("    ") && !u.starts_with('\t')
    };
    let mut out = String::with_capacity(src.len() + 16);
    let mut display: Option<usize> = None;
    let mut chunk = String::new();
    let flush = |chunk: &str, out: &mut String| {
        let html = html_block(chunk);
        out.push_str(&chunk[..html]);
        let chunk = &chunk[html..];
        let table = table_start(chunk);
        let escape = |m: &str, at: usize| if at >= table { escape_math(m).replace("\\\\|", "\\|") } else { escape_math(m) };
        let lone = chunk.trim().strip_prefix("$$").and_then(|r| r.strip_suffix("$$")).is_some_and(|m| !m.trim().is_empty() && !m.contains("$$") && !m.contains('`'));
        if chunk.starts_with("    ") || chunk.starts_with('\t') {
            out.push_str(chunk);
        } else if lone {
            out.push_str(&escape(chunk, 0));
        } else {
            let masked = mask_code_spans(chunk);
            let mut last = 0;
            for (a, b) in inline_math(&masked, true).into_iter().filter(|&(a, b)| !chunk[a..b].contains('`') && !chunk[a..b].contains("://") && !chunk[a..b].contains("www.")) {
                out.push_str(&hold_dollars(&chunk[last..a], &masked[last..a]));
                out.push_str(&escape(&chunk[a..b], a));
                last = b;
            }
            out.push_str(&hold_dollars(&chunk[last..], &masked[last..]));
        }
    };
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let fences: Vec<Fence> = lines
        .iter()
        .scan(None, |fence: &mut Option<(char, usize, usize)>, line| {
            let (u, depth) = unquote(line);
            let t = u.trim();
            // A fence in a block quote ends with the quote.
            if fence.is_some_and(|f| depth < f.2) {
                *fence = None;
            }
            let ticks = t.chars().take_while(|c| matches!(c, '`' | '~')).count();
            let open = t.chars().next().filter(|_| ticks >= 3);
            Some(if let Some((c, n, f_depth)) = *fence {
                if depth == f_depth && open == Some(c) && ticks >= n && t[ticks..].trim().is_empty() {
                    *fence = None;
                }
                Fence::Inside
            } else if let Some(c) = open.filter(|c| *c == '~' || !t[ticks..].contains('`')) {
                *fence = Some((c, ticks, depth));
                Fence::Opens
            } else {
                Fence::Outside
            })
        })
        .collect();
    // closes_later[i]: a `$$` line follows line i in the same block quote. A line quoted less
    // deeply ends the quotes in between, so it hides their delimiters from the lines above.
    let mut closes_later = vec![false; lines.len()];
    let depths: Vec<usize> = lines.iter().map(|l| unquote(l).1).collect();
    // below[d]: a `$$` at depth d follows with no shallower line between. It only grows on a
    // `$$` line, by at most that line's depth, so this pass stays linear.
    let mut below: Vec<bool> = Vec::new();
    for i in (0..lines.len()).rev() {
        let d = depths[i];
        closes_later[i] = below.get(d) == Some(&true);
        below.truncate(d + 1);
        if fences[i] == Fence::Outside && delim(lines[i]) {
            below.resize(d + 1, false);
            below[d] = true;
        }
    }
    for (i, line) in lines.iter().enumerate() {
        match fences[i] {
            Fence::Inside => out.push_str(line),
            Fence::Opens => {
                flush(&std::mem::take(&mut chunk), &mut out);
                out.push_str(line);
            }
            Fence::Outside if delim(line) && display.map_or(closes_later[i], |d| d == depths[i]) => {
                flush(&std::mem::take(&mut chunk), &mut out);
                display = if display.is_some() { None } else { Some(depths[i]) };
                out.push_str(line);
            }
            Fence::Outside if display.is_some() => out.push_str(&escape_math(line)),
            Fence::Outside if line.trim().is_empty() => {
                flush(&std::mem::take(&mut chunk), &mut out);
                out.push_str(line);
            }
            Fence::Outside => chunk.push_str(line),
        }
    }
    flush(&std::mem::take(&mut chunk), &mut out);
    out
}

/// Where the GFM table in a chunk of lines starts: its header row, the line before a delimiter
/// row with as many cells. `chunk.len()` when there is none.
fn table_start(chunk: &str) -> usize {
    let cells = |line: &str| line.trim().trim_start_matches('|').trim_end_matches('|').split('|').count();
    let delimiter = |line: &str| {
        let t = line.trim().trim_start_matches('|').trim_end_matches('|');
        line.contains('|')
            && t.split('|').all(|c| {
                let c = c.trim().trim_start_matches(':').trim_end_matches(':');
                !c.is_empty() && c.bytes().all(|b| b == b'-')
            })
    };
    let mut at = 0;
    let mut prev: Option<(usize, &str)> = None;
    for line in chunk.split_inclusive('\n') {
        if let Some((start, _)) = prev.filter(|(_, h)| delimiter(line) && cells(h) == cells(line)) {
            return start;
        }
        prev = Some((at, line));
        at += line.len();
    }
    chunk.len()
}

/// The line without its block quote markers, and how many it had.
fn unquote(line: &str) -> (&str, usize) {
    let (mut s, mut depth) = (line, 0);
    loop {
        let t = s.trim_start_matches(' ');
        match t.strip_prefix('>') {
            Some(r) if s.len() - t.len() <= 3 => (s, depth) = (r.strip_prefix(' ').unwrap_or(r), depth + 1),
            _ => return (s, depth),
        }
    }
}

/// The length of the HTML block a chunk of lines starts with, or 0 (CommonMark kinds 1, 2, 6,
/// and 7). cmark does not unescape inside one, so math there is left as written.
fn html_block(chunk: &str) -> usize {
    const TAGS: [&str; 62] = [
        "address",
        "article",
        "aside",
        "base",
        "basefont",
        "blockquote",
        "body",
        "caption",
        "center",
        "col",
        "colgroup",
        "dd",
        "details",
        "dialog",
        "dir",
        "div",
        "dl",
        "dt",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "frame",
        "frameset",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hr",
        "html",
        "iframe",
        "legend",
        "li",
        "link",
        "main",
        "menu",
        "menuitem",
        "nav",
        "noframes",
        "ol",
        "optgroup",
        "option",
        "p",
        "param",
        "search",
        "section",
        "summary",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "title",
        "tr",
        "track",
        "ul",
    ];
    let line = chunk.lines().next().unwrap_or_default();
    let t = line.trim_start();
    if line.len() - t.len() > 3 || !t.starts_with('<') {
        return 0;
    }
    // Kinds 1 and 2 end on the line with their end marker, and the rest of the chunk is Markdown.
    let through = |marker: &str| chunk.to_ascii_lowercase().find(marker).map_or(chunk.len(), |i| chunk[i..].find('\n').map_or(chunk.len(), |n| i + n + 1));
    if t.starts_with("<!--") {
        return through("-->");
    }
    let closing = t[1..].starts_with('/');
    let rest = if closing { &t[2..] } else { &t[1..] };
    let name = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
    let (name, after) = (rest[..name].to_ascii_lowercase(), &rest[name..]);
    let ends = after.is_empty() || after.starts_with(['>', ' ', '\t', '/']);
    if !closing && ends && ["pre", "script", "style", "textarea"].contains(&name.as_str()) {
        return through(&format!("</{name}>"));
    }
    let lone_tag = rest.starts_with(|c: char| c.is_ascii_alphabetic()) && ends && t.trim_end().ends_with('>') && !t[1..].contains('<');
    if TAGS.contains(&name.as_str()) && ends || lone_tag { chunk.len() } else { 0 }
}

fn mask_code_spans(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('`') {
        let n = rest[i..].bytes().take_while(|b| *b == b'`').count();
        let ticks = &rest[i..i + n];
        out.push_str(&rest[..i + n]);
        rest = &rest[i + n..];
        match rest.find(ticks) {
            Some(j) => {
                out.extend(rest[..j].chars().map(|c| if c == '$' { ' ' } else { c }));
                out.push_str(ticks);
                rest = &rest[j + n..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

fn cmark(src: &str, sourcepos: bool, front_matter: bool) -> (String, Option<String>) {
    let src = &protect_math(src);
    let arena = Arena::new();
    let mut o = options();
    o.render.sourcepos = sourcepos;
    o.render.hardbreaks = !front_matter && sourcepos;
    if front_matter {
        o.extension.front_matter_delimiter = Some("---".into());
    }
    let root = parse_document(&arena, src, &o);
    let mut front = None;
    for node in root.descendants() {
        let mut data = node.data_mut();
        match &mut data.value {
            NodeValue::HtmlBlock(b) => b.literal = own_positions(&tag_filter(&b.literal)),
            NodeValue::HtmlInline(s) => *s = own_positions(&tag_filter(s)),
            NodeValue::FrontMatter(s) => front = Some(s.clone()),
            _ => {}
        }
    }
    let mut out = String::new();
    format_html(root, &o, &mut out).expect("format html");
    (out, front)
}

fn own_positions(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (i, _) in lower.match_indices("data-sourcepos") {
        out.push_str(&s[last..i + 10]);
        out.push_str("&#101;");
        last = i + 11;
    }
    out.push_str(&s[last..]);
    out
}

const FILTERED: [&str; 9] = ["title", "textarea", "style", "xmp", "iframe", "noembed", "noframes", "script", "plaintext"];

pub fn tag_filter(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for i in 0..b.len() {
        if b[i] != b'<' {
            continue;
        }
        let start = i + 1 + usize::from(b.get(i + 1) == Some(&b'/'));
        let Some(tag) = FILTERED.iter().find(|t| lower[start..].starts_with(*t)) else { continue };
        if matches!(b.get(start + tag.len()), None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'>')) {
            out.push_str(&s[last..i]);
            out.push_str("&lt;");
            last = i + 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

#[cfg(test)]
fn api_html(src: &str) -> String {
    inner_html(&api_dom(src, false).0)
}

fn api_dom(src: &str, front_matter: bool) -> (NodeRef, Option<String>) {
    let (html, front) = cmark(src, true, front_matter);
    let body = parse_body(&html);
    flatten_deep(&body, MAX_DEPTH);
    sanitize::clean(&body);
    api_filters(&body);
    (body, front)
}

const BLANK_GIF: &str = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

fn is_remote(url: &str) -> bool {
    let url: String = url.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    url.trim_matches(|c: char| c <= ' ').split_once(':').is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
}

fn local_media(url: &str, dir: &Path, root: &Path) -> Option<String> {
    let t = url.trim();
    if t.is_empty() || t.starts_with('#') || is_remote(t) {
        return Some(url.to_string());
    }
    media_path(t, dir, root)?;
    match t.strip_prefix('/') {
        Some(rest) => Some(format!("{}{rest}", "../".repeat(dir.strip_prefix(root).ok()?.components().count()))),
        None => Some(url.to_string()),
    }
}

/// The file a local media URL opens, once canonical, when it is inside `root`. A `/` path starts at `root`, any
/// other at `dir`. The query and fragment don't name a file, so `?raw=1` is dropped.
fn media_path(t: &str, dir: &Path, root: &Path) -> Option<PathBuf> {
    if t.starts_with("//") || t.contains(|c: char| c == '\\' || c.is_control()) {
        return None;
    }
    let path = t.split(['?', '#']).next().unwrap_or_default();
    // An encoded slash is one URL segment but more than one folder on disk, so the two can't agree.
    if path.to_ascii_lowercase().contains("%2f") {
        return None;
    }
    let path = percent_decode(path)?;
    let joined = match path.strip_prefix('/') {
        Some(rest) => root.join(rest),
        None => dir.join(&path),
    };
    // WebKit drops `..` with the segment before it as text, before it follows any symlink.
    let mut full = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                full.pop();
            }
            Component::CurDir => {}
            c => full.push(c),
        }
    }
    std::fs::canonicalize(full).ok().filter(|p| p.starts_with(root))
}

/// The file a local image URL in a Markdown file in `dir` may show, by the same rules as WebKit's page: inside the
/// repo, else the home folder, else `dir`. None for a remote or empty URL, or one that leaves that folder.
pub fn confined_media(url: &str, dir: &Path) -> Option<PathBuf> {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let root = media_root(&dir, std::env::var_os("HOME").map(PathBuf::from).as_deref());
    let t = url.trim();
    if t.is_empty() || t.starts_with('#') || is_remote(t) {
        return None;
    }
    // A `file://` URL names an absolute path, which must still be inside the root.
    if let Some(rest) = t.get(..7).filter(|p| p.eq_ignore_ascii_case("file://")).map(|_| &t[7..]) {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        return rest.starts_with('/').then(|| media_path(&format!("/{}", rest.trim_start_matches('/')), Path::new("/"), Path::new("/"))).flatten().filter(|p| p.starts_with(&root));
    }
    media_path(t, &dir, &root)
}

/// None when the bytes aren't UTF-8, since the path checked must be the one WebKit opens.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        // from_str_radix takes a sign, and WebKit keeps `%+2` as it is.
        let hex = b.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit)).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn media_root(dir: &Path, home: Option<&Path>) -> PathBuf {
    if let Some(repo) = dir.ancestors().find(|a| a.join(".git").exists()) {
        return repo.to_path_buf();
    }
    match home.and_then(|h| std::fs::canonicalize(h).ok()) {
        Some(home) if home != Path::new("/") && dir.starts_with(&home) => home,
        _ => dir.to_path_buf(),
    }
}

fn confine_media(body: &NodeRef, dir: &Path) {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let root = media_root(&dir, std::env::var_os("HOME").map(PathBuf::from).as_deref());
    let (dir, root) = (dir.as_path(), root.as_path());
    for n in elements(body).into_iter().filter(|n| is(n, "img") || is(n, "video") || is(n, "source")) {
        for k in ["src", "poster"] {
            if let Some(v) = attr(&n, k) {
                match local_media(&v, dir, root) {
                    Some(u) => set_attr(&n, k, &u),
                    None => remove_attr(&n, k),
                }
            }
        }
        if let Some(set) = attr(&n, "srcset") {
            let kept: Vec<String> = set
                .split(',')
                .filter_map(|c| {
                    let c = c.trim();
                    let (u, rest) = c.split_once(char::is_whitespace).unwrap_or((c, ""));
                    local_media(u, dir, root).map(|u| if rest.is_empty() { u } else { format!("{u} {}", rest.trim()) })
                })
                .collect();
            if kept.is_empty() {
                remove_attr(&n, "srcset");
            } else {
                set_attr(&n, "srcset", &kept.join(", "));
            }
        }
    }
}

pub fn render(src: &str) -> Doc {
    render_in(src, None)
}

pub fn render_in(src: &str, dir: Option<&Path>) -> Doc {
    let (body, front) = api_dom(src, true);
    let math = elements(&body).iter().any(|n| is(n, "math-renderer"));
    let mermaid = elements(&body).iter().any(|n| is(n, "div") && has_class(n, "highlight-source-mermaid"));
    (crate::style::DEFAULT.dress)(&body, front.as_deref());
    raw_autolinks(&body);
    for node in elements(&body) {
        if node.parent().is_some_and(|p| !same(&p, &body)) {
            remove_attr(&node, "data-sourcepos");
        }
    }
    if let Some(dir) = dir {
        confine_media(&body, dir);
    }
    for v in elements(&body).into_iter().filter(|n| is(n, "video")) {
        let mut media = std::iter::once(v.clone()).chain(v.children().filter(|c| is(c, "source")));
        if !attr(&v, "poster").as_deref().is_some_and(is_remote) && !media.any(|m| attr(&m, "src").as_deref().is_some_and(is_remote)) {
            continue;
        }
        set_attr(&v, "preload", "none");
        remove_attr(&v, "autoplay");
        if let Some(poster) = attr(&v, "poster").filter(|p| is_remote(p)) {
            set_attr(&v, "data-mddog-poster", &poster);
            remove_attr(&v, "poster");
        }
        for m in std::iter::once(v.clone()).chain(v.children().filter(|c| is(c, "source"))) {
            if let Some(src) = attr(&m, "src").filter(|s| is_remote(s)) {
                set_attr(&m, "data-mddog-media", &src);
                remove_attr(&m, "src");
            }
        }
    }
    for n in elements(&body).into_iter().filter(|n| is(n, "source") || is(n, "img")) {
        if let Some(set) = attr(&n, "srcset").filter(|s| s.split(',').any(is_remote)) {
            set_attr(&n, "data-mddog-srcset", &set);
            remove_attr(&n, "srcset");
        }
    }
    for img in elements(&body).into_iter().filter(|n| is(n, "img")) {
        let src = attr(&img, "src").unwrap_or_default();
        let sources = has_attr(&img, "data-mddog-srcset") || img.parent().is_some_and(|p| is(&p, "picture") && p.children().any(|c| has_attr(&c, "data-mddog-srcset")));
        if is_remote(&src) || sources {
            set_attr(&img, "data-mddog-src", &src);
            set_attr(&img, "src", BLANK_GIF);
        }
    }
    Doc { html: inner_html(&body), math, mermaid }
}

fn api_filters(body: &NodeRef) {
    for node in elements(body) {
        if node.parent().is_none() {
            continue;
        }
        match name(&node).as_deref() {
            Some("pre") => code_block(&node),
            Some("code") if !node.ancestors().any(|a| is(&a, "pre")) => set_attr(&node, "class", "notranslate"),
            Some("blockquote") => alert(&node),
            Some("img") => image(&node),
            Some("a") => link(&node),
            Some("table") => {
                set_attr(&node, "role", "table");
                let wrapper = el("markdown-accessiblity-table", &[]);
                if let Some(pos) = attr(&node, "data-sourcepos") {
                    set_attr(&wrapper, "data-sourcepos", &pos);
                }
                wrap(&node, &wrapper);
            }
            Some("section") if has_class(&node, "footnotes") => footnotes(&node),
            Some("sup") => footnote_ref(&node),
            Some("li") => task_item(&node),
            _ => {}
        }
    }
    text_filters(body);
}

const ALERTS: [(&str, &str); 5] = [("NOTE", "info"), ("TIP", "light-bulb"), ("IMPORTANT", "report"), ("WARNING", "alert"), ("CAUTION", "stop")];

fn alert(bq: &NodeRef) {
    let Some(p) = bq.children().find(|c| c.as_element().is_some()) else { return };
    if !is(&p, "p") {
        return;
    }
    let Some(first) = p.first_child() else { return };
    let Some(text) = first.as_text() else { return };
    let t = text.borrow().clone();
    let Some((kind, icon)) = ALERTS.iter().find(|(k, _)| {
        let marker = format!("[!{k}]");
        t.get(..marker.len()).is_some_and(|m| m.eq_ignore_ascii_case(&marker)) && t[marker.len()..].chars().next().is_none_or(|c| c == '\n')
    }) else {
        return;
    };
    let rest = t[kind.len() + 3..].strip_prefix('\n').unwrap_or(&t[kind.len() + 3..]).to_string();
    let has_more = !rest.is_empty() || first.next_sibling().is_some() || bq.children().filter(|c| c.as_element().is_some()).count() > 1;
    if !has_more {
        return;
    }
    let class = kind.to_ascii_lowercase();
    let title = format!("{}{}", &kind[..1], &class[1..]);
    let div = el("div", &[("class", &format!("markdown-alert markdown-alert-{class}"))]);
    if let Some(pos) = attr(bq, "data-sourcepos") {
        set_attr(&div, "data-sourcepos", &pos);
    }
    for f in fragment(&format!("<p class=\"markdown-alert-title\">{}{title}</p>", octicons::svg(icon, " mr-2"))) {
        div.append(f);
    }
    if rest.is_empty() {
        first.detach();
        if let Some(br) = p.first_child().filter(|c| is(c, "br")) {
            br.detach();
            if let Some(nl) = p.first_child().filter(|c| c.as_text().is_some_and(|t| t.borrow().trim().is_empty())) {
                nl.detach();
            }
        }
        if p.children().next().is_none() {
            p.detach();
        }
    } else {
        *text.borrow_mut() = rest;
    }
    let mut skip_ws = true;
    for c in bq.children().collect::<Vec<_>>() {
        if skip_ws && c.as_text().is_some_and(|t| t.borrow().trim().is_empty()) {
            continue;
        }
        skip_ws = false;
        div.append(c);
    }
    replace(bq, &div);
}

fn image(img: &NodeRef) {
    let src = attr(img, "src").unwrap_or_default();
    let style = match attr(img, "height") {
        Some(h) if !h.is_empty() && h.chars().all(|c| c.is_ascii_digit()) => format!("max-width: 100%; height: auto; max-height: {h}px;"),
        _ => "max-width: 100%;".to_string(),
    };
    let remote = is_remote(&src);
    if remote {
        set_attr(img, "data-canonical-src", &src);
    }
    let picture = img.parent().filter(|p| is(p, "picture"));
    if picture.is_none() {
        set_attr(img, "style", &style);
    }
    if img.ancestors().any(|a| is(&a, "a")) {
        return;
    }
    if let Some(picture) = picture {
        if !picture.parent().is_some_and(|p| is(&p, "themed-picture")) {
            wrap(&picture, &el("themed-picture", &[("data-catalyst-inline", "true")]));
        }
        return;
    }
    let rel = if remote { "noopener noreferrer nofollow" } else { "noopener noreferrer" };
    let a = el("a", &[("target", "_blank"), ("rel", rel), ("href", &src)]);
    wrap(img, &a);
    if a.parent().is_some_and(|p| is(&p, "body")) {
        wrap(&a, &el("p", &[]));
    }
}

fn link(a: &NodeRef) {
    if has_attr(a, "target") || has_attr(a, "data-footnote-ref") || has_attr(a, "data-footnote-backref") || a.ancestors().any(|x| has_class(&x, "footnotes")) {
        return;
    }
    let Some(href) = attr(a, "href") else { return };
    let host = href.strip_prefix("https://").or_else(|| href.strip_prefix("http://")).map(|r| r.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase());
    if host.is_some_and(|h| h != "github.com" && !h.ends_with(".github.com")) {
        set_attr(a, "rel", "nofollow");
    }
}

fn task_item(li: &NodeRef) {
    let input = li.children().find(|c| is(c, "input")).or_else(|| li.children().find(|c| is(c, "p")).and_then(|p| p.children().find(|c| is(c, "input"))));
    let Some(input) = input else { return };
    let checked = has_attr(&input, "checked");
    let fresh = el("input", &[("type", "checkbox"), ("id", ""), ("disabled", ""), ("class", "task-list-item-checkbox"), ("aria-label", if checked { "Completed task" } else { "Incomplete task" })]);
    if checked {
        set_attr(&fresh, "checked", "");
    }
    replace(&input, &fresh);
    set_attr(li, "class", "task-list-item");
    if let Some(list) = li.parent() {
        set_attr(&list, "class", "contains-task-list");
    }
}

fn footnotes(section: &NodeRef) {
    let fresh = el("section", &[("data-footnotes", ""), ("class", "footnotes")]);
    for f in fragment("<h2 id=\"footnote-label\" class=\"sr-only\">Footnotes</h2>") {
        fresh.append(f);
    }
    move_children(section, &fresh);
    for li in fresh.descendants().filter(|n| is(n, "li")).collect::<Vec<_>>() {
        if let Some(id) = attr(&li, "id") {
            set_attr(&li, "id", &format!("user-content-{id}-{RUN_ID}"));
        }
    }
    for a in fresh.descendants().filter(|n| has_attr(n, "data-footnote-backref")).collect::<Vec<_>>() {
        let href = attr(&a, "href").unwrap_or_default();
        let label = attr(&a, "aria-label").unwrap_or_default();
        let fresh_a =
            el("a", &[("href", &format!("#user-content-{}-{RUN_ID}", href.trim_start_matches('#'))), ("data-footnote-backref", ""), ("aria-label", &label), ("class", "data-footnote-backref")]);
        move_children(&a, &fresh_a);
        replace(&a, &fresh_a);
    }
    replace(section, &fresh);
}

fn footnote_ref(sup: &NodeRef) {
    // The sanitizer keeps a footnote anchor's id unprefixed, so raw HTML's extra anchors are rebuilt too.
    let refs: Vec<NodeRef> = sup.children().filter(|c| has_attr(c, "data-footnote-ref")).collect();
    if !refs.is_empty() {
        remove_attr(sup, "class");
    }
    for a in refs {
        let href = attr(&a, "href").unwrap_or_default();
        let id = attr(&a, "id").unwrap_or_default();
        let fresh = el(
            "a",
            &[
                ("href", &format!("#user-content-{}-{RUN_ID}", href.trim_start_matches('#'))),
                ("id", &format!("user-content-{id}-{RUN_ID}")),
                ("data-footnote-ref", ""),
                ("aria-describedby", "footnote-label"),
            ],
        );
        move_children(&a, &fresh);
        replace(&a, &fresh);
    }
}

fn code_block(pre: &NodeRef) {
    let lang = attr(pre, "lang").unwrap_or_default();
    if lang.is_empty() && !pre.children().any(|c| is(&c, "code")) {
        set_attr(pre, "class", "notranslate");
        return;
    }
    let code = pre.text_contents();
    let pos = attr(pre, "data-sourcepos");
    let html = if lang.is_empty() {
        highlight::copy_box("snippet-clipboard-content", &format!("<pre class=\"notranslate\"><code class=\"notranslate\">{}</code></pre>", escape(&code)), code.strip_suffix('\n').unwrap_or(&code))
    } else if lang.eq_ignore_ascii_case("math") {
        format!("<math-renderer class=\"js-display-math\" style=\"display: block\" data-run-id=\"{RUN_ID}\">$${}$$</math-renderer>", escape(code.trim_end_matches('\n')))
    } else {
        highlight::block(&lang, &code)
    };
    let nodes = fragment(&html);
    for n in &nodes {
        pre.insert_before(n.clone());
    }
    if let (Some(p), Some(first)) = (pos, nodes.iter().find(|n| n.as_element().is_some())) {
        set_attr(first, "data-sourcepos", &p);
    }
    pre.detach();
}

fn in_code(node: &NodeRef) -> bool {
    node.ancestors().any(|a| matches!(name(&a).as_deref(), Some("code" | "pre" | "math-renderer" | "mddog-mermaid")) || has_class(&a, "highlight"))
}

fn link_of(node: &NodeRef) -> Option<NodeRef> {
    node.ancestors().find(|a| is(a, "a"))
}

fn shows_own_url(a: &NodeRef) -> bool {
    let Some(href) = attr(a, "href") else { return false };
    let text = a.text_contents();
    href == text || href.strip_prefix("http://") == Some(text.as_str()) || href.strip_prefix("mailto:") == Some(text.as_str())
}

fn text_filters(body: &NodeRef) {
    for code in body.descendants().filter(|n| is(n, "code")).collect::<Vec<_>>() {
        if code.ancestors().any(|a| is(&a, "pre")) {
            continue;
        }
        let (Some(prev), Some(next)) = (code.previous_sibling(), code.next_sibling()) else { continue };
        let (Some(before), Some(after)) = (prev.as_text(), next.as_text()) else { continue };
        let (b, a) = {
            let (b, a) = (before.borrow(), after.borrow());
            let (Some(b), Some(a)) = (b.strip_suffix('$'), a.strip_prefix('$')) else { continue };
            (b.to_string(), a.to_string())
        };
        *before.borrow_mut() = b;
        *after.borrow_mut() = a;
        let math = el("math-renderer", &[("class", "js-inline-math"), ("style", "display: inline-block"), ("data-run-id", RUN_ID)]);
        math.append(NodeRef::new_text(format!("${}$", escape(&code.text_contents()))));
        replace(&code, &math);
    }
    for p in body.descendants().filter(|n| is(n, "p")).collect::<Vec<_>>() {
        let mut kids: Vec<NodeRef> = p.children().collect();
        if kids.is_empty() || !kids.iter().all(|k| k.as_text().is_some() || is(k, "br")) {
            continue;
        }
        // GitHub keeps trailing breaks after the math.
        let tail = kids.iter().rev().take_while(|k| is(k, "br") || k.as_text().is_some_and(|t| t.borrow().trim().is_empty())).count();
        let rest = kids.split_off(kids.len() - tail);
        let s: String = kids.iter().filter_map(|k| k.as_text().map(|t| t.borrow().clone())).collect();
        let s = s.trim_end();
        let Some(inner) = s.strip_prefix("$$").and_then(|r| r.strip_suffix("$$")) else { continue };
        if !inner.trim().is_empty() && !inner.contains("$$") {
            for k in kids {
                k.detach();
            }
            for f in fragment(&format!("<math-renderer class=\"js-display-math\" style=\"display: block\" data-run-id=\"{RUN_ID}\">{}</math-renderer>", escape(s))) {
                match rest.first() {
                    Some(r) => r.insert_before(f),
                    None => p.append(f),
                }
            }
        }
    }
    for text in body.descendants().text_nodes().collect::<Vec<_>>() {
        let node = text.as_node();
        if in_code(node) {
            continue;
        }
        let s = text.borrow().clone();
        if let Some(a) = link_of(node) {
            if !shows_own_url(&a)
                && let Cow::Owned(replaced) = emoji::replace(&s)
            {
                *text.borrow_mut() = replaced;
            }
            continue;
        }
        let mut pieces: Vec<String> = Vec::new();
        let mut last = 0;
        for (a, b) in inline_math(&s, false) {
            if a > last {
                pieces.push(escape(&emoji::replace(&s[last..a])));
            }
            pieces.push(format!("<math-renderer class=\"js-inline-math\" style=\"display: inline-block\" data-run-id=\"{RUN_ID}\">{}</math-renderer>", escape(&s[a..b])));
            last = b;
        }
        if last == 0 {
            if let Cow::Owned(replaced) = emoji::replace(&s) {
                *text.borrow_mut() = replaced;
            }
            continue;
        }
        if last < s.len() {
            pieces.push(escape(&emoji::replace(&s[last..])));
        }
        for f in fragment(&pieces.concat()) {
            node.insert_before(f);
        }
        node.detach();
    }
    for node in body.descendants() {
        if let Some(text) = node.as_text().filter(|t| t.borrow().contains(HELD_DOLLAR)) {
            let t = text.borrow().replace(HELD_DOLLAR, if in_code(&node) { "\\$" } else { "$" });
            *text.borrow_mut() = t;
        }
        if let Some(e) = node.as_element() {
            for v in e.attributes.borrow_mut().map.values_mut().filter(|v| v.value.contains(HELD_DOLLAR)) {
                v.value = v.value.replace(HELD_DOLLAR, "$");
            }
        }
    }
}

/// The `$…$` and `$$…$$` spans of `s`, by GitHub's rules. An opener follows whitespace, `(`, or the
/// start, and comes before a character that is not whitespace or `$`. The next `$` (or `$$`) ends
/// the search. It closes the math unless it comes before a word character, sits between two of the
/// same punctuation mark, or (for `$`) follows whitespace and comes before whitespace or the end.
/// Math does not cross lines. `loose`, for the source pass, also takes an opener after the end of
/// other inline markup, so it finds every span the text pass finds.
pub fn inline_math(s: &str, loose: bool) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    if !b.contains(&b'$') {
        return Vec::new();
    }
    let n = b.len();
    let mut escaped = vec![false; n];
    for i in 1..n {
        escaped[i] = b[i - 1] == b'\\' && !escaped[i - 1];
    }
    let dollar = |j: usize| b[j] == b'$' && !escaped[j];
    // next[d - 1][k]: the first `$` (d = 1) or `$$` (d = 2) at or after k. line[k]: the first newline.
    let mut next = [vec![usize::MAX; n + 1], vec![usize::MAX; n + 1]];
    let mut line = vec![usize::MAX; n + 1];
    for j in (0..n).rev() {
        next[0][j] = if dollar(j) { j } else { next[0][j + 1] };
        next[1][j] = if dollar(j) && b.get(j + 1) == Some(&b'$') { j } else { next[1][j + 1] };
        line[j] = if b[j] == b'\n' { j } else { line[j + 1] };
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if !dollar(i) {
            i += 1;
            continue;
        }
        let delim = if b.get(i + 1) == Some(&b'$') { 2 } else { 1 };
        let open_end = i + delim;
        let prev = s[..i].chars().next_back();
        let prev_ok = prev.is_none_or(|c| c.is_whitespace() || c == '(' || loose && "*_`~)]>|".contains(c));
        let next_ok = s[open_end..].chars().next().is_some_and(|c| !c.is_whitespace() && c != '$');
        let j = next[delim - 1][open_end];
        let closes = j != usize::MAX && {
            let (before, after) = (s[..j].chars().next_back(), s[j + delim..].chars().next());
            !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !(before == after && after.is_some_and(|c| c.is_ascii_punctuation()))
                && (delim == 2 || !before.is_some_and(char::is_whitespace) || after.is_some_and(|c| !c.is_whitespace()))
                && line[open_end] > j
        };
        if prev_ok && next_ok && closes {
            out.push((i, j + delim));
            i = j + delim;
        } else {
            i += delim;
        }
    }
    out
}

fn raw_autolinks(body: &NodeRef) {
    for text in body.descendants().text_nodes().collect::<Vec<_>>() {
        let node = text.as_node();
        if in_code(node) || link_of(node).is_some() {
            continue;
        }
        let s = text.borrow().clone();
        let mut pieces = Vec::new();
        let mut last = 0;
        let mut i = 0;
        while i < s.len() {
            let rest = &s[i..];
            let starts = rest.starts_with("https://") || rest.starts_with("http://");
            let boundary = i == 0 || s[..i].chars().next_back().is_some_and(|c| !c.is_alphanumeric());
            if !(starts && boundary) {
                i += rest.chars().next().map_or(1, char::len_utf8);
                continue;
            }
            let end = i + rest.find(|c: char| c.is_whitespace() || c == '"' || c == '<').unwrap_or(rest.len());
            let mut url_end = end;
            while url_end > i && s[..url_end].ends_with(['.', ',', ':', ';', '!', '?', ')', '\'']) {
                url_end -= 1;
            }
            let url = &s[i..url_end];
            if url.len() <= "https://".len() {
                i = end;
                continue;
            }
            pieces.push(escape(&s[last..i]));
            pieces.push(format!("<a href=\"{}\" rel=\"nofollow\">{}</a>", escape(url), escape(url)));
            last = url_end;
            i = url_end;
        }
        if last == 0 {
            continue;
        }
        pieces.push(escape(&s[last..]));
        for f in fragment(&pieces.concat()) {
            node.insert_before(f);
        }
        node.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_html_cannot_write_source_positions() {
        let html = render("<p DATA-SOURCEPOS = \"1:1-1:10\">x</p>\n\n<ul><li/data-sourcepos=\"1:1-1:10\">y</li></ul>\n").html;
        assert!(!html.contains("data-sourcepos=\"1:1-1:10\""), "{html}");
        let html = render("<p title=\"data-sourcepos\">data-sourcepos = x</p>\n").html;
        assert!(html.contains(">data-sourcepos = x<") && html.contains("title=\"data-sourcepos\""), "{html}");
    }

    #[test]
    fn remote_picture_sources_are_deferred() {
        let html = render("<picture>\n<source media=\"(prefers-color-scheme: dark)\" srcset=\"https://x.test/d.png\">\n<img alt=\"a\" src=\"local.png\">\n</picture>\n").html;
        assert!(!html.contains(" srcset="), "{html}");
        assert!(html.contains("data-mddog-srcset=\"https://x.test/d.png\""), "{html}");
        assert!(html.contains("data-mddog-src=\"local.png\""), "{html}");
    }

    #[test]
    fn emoji_in_link_text_unless_it_is_the_url() {
        let html = render("[Sponsors :heart:](#x) <https://example.com/:smile:/x> [see $x$ here](#a) <img src=\"HTTPS://x.test/a.png\">\n").html;
        assert!(html.contains(">Sponsors \u{2764}"), "{html}");
        assert!(html.contains(">https://example.com/:smile:/x<"), "{html}");
        assert!(html.contains(">see $x$ here<"), "{html}");
        assert!(html.contains("data-mddog-src=\"HTTPS://x.test/a.png\""), "{html}");
    }

    #[test]
    fn remote_video_and_mixed_srcset_are_deferred() {
        let html = render(
            "<video src=\"https://x.test/v.mp4\" poster=\"https://x.test/p.png\"></video>\n\n<picture>\n<source srcset=\"a.png 1x, https://x.test/b.png 2x\">\n<img src=\"a.png\">\n</picture>\n",
        )
        .html;
        assert!(html.contains("preload=\"none\""), "{html}");
        assert!(html.contains("data-mddog-poster=\"https://x.test/p.png\"") && !html.contains(" poster="), "{html}");
        assert!(html.contains("data-mddog-srcset=\"a.png 1x, https://x.test/b.png 2x\""), "{html}");
    }

    #[test]
    fn remote_urls_are_normalized_like_webkit() {
        for url in ["http://x.test/a", " HTTPS://x.test/a", "http:\n//127.0.0.1/x", "http:/\\127.0.0.1/x", "\thttp://x"] {
            assert!(is_remote(url), "{url:?}");
        }
        for url in ["a.png", "./http://x", "foo:bar", "//x.test/a.png", ""] {
            assert!(!is_remote(url), "{url:?}");
        }
        let local = render("<video src=\"clip.mp4\"></video>\n").html;
        assert!(!local.contains("preload"), "{local}");
    }

    #[test]
    fn raw_autolink_ends_at_quote_or_angle() {
        let html = render("<iframe src=\"https://example.com\"></iframe>\n\n<iframe>see https://a.test/x&lt;b</iframe>\n").html;
        assert!(html.contains("&lt;iframe src=\"<a href=\"https://example.com\" rel=\"nofollow\">https://example.com</a>\"&gt;&lt;/iframe&gt;"), "{html}");
        assert!(html.contains("<a href=\"https://a.test/x\" rel=\"nofollow\">https://a.test/x</a>&lt;b"), "{html}");
    }

    #[test]
    fn raw_pre_keeps_its_markup() {
        let html = api_html("<pre><b>x</b> <a href=\"https://example.com/y\">z</a><br>w</pre>\n");
        assert!(html.contains("<pre class=\"notranslate\"><b>x</b> <a href=\"https://example.com/y\" rel=\"nofollow\">z</a><br>w</pre>"), "{html}");
    }

    #[test]
    fn code_span_math_and_abandoned_openers() {
        let html = api_html("$`p<q`$ and c $z $ d $$w$$ e\n");
        assert!(html.contains("<math-renderer class=\"js-inline-math\" style=\"display: inline-block\" data-run-id=\"0f23d3dd37596070b592c1adaa9bf6c8\">$p&amp;lt;q$</math-renderer>"), "{html}");
        assert!(html.contains("c $z $ d <math-renderer") && html.contains(">$$w$$</math-renderer> e"), "{html}");
    }

    #[test]
    fn math_keeps_backslash_punctuation() {
        let html = api_html("$$\n\\int e^{-x^2}\\,dx \\\\ y\n$$\n\nInline $a\\,b$ and `$c\\,d$` and\n\n```\n$e\\,f$\n```\n\nplain \\* star\n");
        assert!(html.contains("\\int e^{-x^2}\\,dx \\\\ y"), "{html}");
        assert!(html.contains(">$a\\,b$</math-renderer>"), "{html}");
        assert!(html.contains("$c\\,d$") && html.contains("$e\\,f$") && !html.contains("\\\\,d") && !html.contains("\\\\,f"), "{html}");
        assert!(html.contains("plain * star"), "{html}");
    }

    #[test]
    fn escaped_dollar_is_not_math() {
        let html = render("Price \\$5 and \\$10, math $x_1$.\n").html;
        assert!(!html.contains(HELD_DOLLAR) && html.contains("Price $5 and $10"), "{html}");
        let html = api_html("Price \\$k$ here and $l\\$ m$\n");
        assert!(!html.contains(HELD_DOLLAR) && html.contains("Price $k$ here"), "{html}");
        assert!(!html.contains(">$k$</math-renderer>"), "{html}");
        assert!(html.contains("$l\\$ m$</math-renderer>"), "{html}");
    }

    #[test]
    fn unmatched_display_math_leaves_the_rest_alone() {
        let html = render("$$\n\nSome *em* and `a\\b`\n").html;
        assert!(html.contains("<em>em</em>") && html.contains("<code>a\\b</code>"), "{html}");
        let html = render("$$\n\n```\n$$\n```\n\nSome *em*\n").html;
        assert!(html.contains(">em</em>"), "{html}");
        let html = render("    $$\n    a\\b\n    $$\n").html;
        assert!(html.contains("a\\b") && !html.contains("a\\\\b"), "{html}");
    }

    #[test]
    fn slugs_keep_what_github_keeps() {
        let html = render("# हिन्दी\n\n# เมื่อ\n\n# x²\n\n# a‿b\n\n# Café\n\n# Café\n").html;
        for id in ["हिन्दी", "เมื่อ", "\"user-content-x\"", "a‿b", "\"user-content-café\"", "\"user-content-café-1\""] {
            assert!(html.contains(id), "{id}: {html}");
        }
    }

    #[test]
    fn footnote_links_reach_their_targets() {
        let html = render("See[^n].\n\n[^n]: Note.\n").html;
        let ref_href = html.split("data-footnote-ref").next().unwrap().rsplit("href=\"#").next().unwrap().split('"').next().unwrap().to_string();
        assert!(html.contains(&format!("id=\"{ref_href}\"")), "{ref_href}: {html}");
        assert!(html.contains("data-footnote-backref"), "{html}");
    }

    #[test]
    fn local_media_stays_inside_the_repo() {
        let top = std::env::temp_dir().join(format!("mddog-media-{}", std::process::id()));
        let repo = top.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("docs/img")).unwrap();
        for f in ["docs/img/a b.png", "logo.png"] {
            std::fs::write(repo.join(f), b"x").unwrap();
        }
        std::fs::write(top.join("secret.png"), b"x").unwrap();
        std::fs::write(repo.join("docs/..\\..\\..\\secret.png"), b"x").unwrap();
        std::fs::create_dir_all(repo.join("docs/.\t./.\t.")).unwrap();
        std::fs::write(repo.join("docs/.\t./.\t./secret.png"), b"x").unwrap();
        let dir = std::fs::canonicalize(repo.join("docs")).unwrap();
        let outside = top.join("secret.png").display().to_string();
        let src = format!(
            "<img alt=\"g\" src=\"..\\..\\..\\secret.png\"> <img alt=\"t\" src=\".&#9;./.&#9;./secret.png\"> ![a](img/a%20b.png) ![b](/logo.png) ![c](../../secret.png) ![d]({outside}) ![e](//host/x.png) ![f](https://x.test/r.png)\n\n<picture><source srcset=\"img/a%20b.png 1x, ../../secret.png 2x\"><img src=\"img/a%20b.png\"></picture>\n"
        );
        let html = render_in(&src, Some(&dir)).html;
        let _ = std::fs::remove_dir_all(&top);
        assert!(html.contains("src=\"img/a%20b.png\"") && html.contains("src=\"../logo.png\"") && html.contains("src=\"https://x.test/r.png\""), "{html}");
        assert!(!html.contains("src=\"..\\") && !html.contains("src=\".\t") && !html.contains(&format!("src=\"{outside}")) && !html.contains("src=\"//host"), "{html}");
        assert!(html.contains("srcset=\"img/a%20b.png 1x\""), "{html}");
    }

    #[test]
    fn native_images_follow_the_same_rules() {
        let top = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("mddog-native-media-{}", std::process::id()));
        let repo = top.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("docs/img")).unwrap();
        std::fs::write(repo.join("docs/img/a b.png"), b"x").unwrap();
        std::fs::write(repo.join("logo.png"), b"x").unwrap();
        std::fs::write(top.join("secret.png"), b"x").unwrap();
        let dir = repo.join("docs");
        let at = |u: &str| confined_media(u, &dir);
        let in_repo = |f: &str| Some(repo.join(f));
        assert_eq!(at("img/a%20b.png"), in_repo("docs/img/a b.png"));
        assert_eq!(at("img/a%20b.png?raw=1"), in_repo("docs/img/a b.png"));
        assert_eq!(at("/logo.png"), in_repo("logo.png"));
        assert_eq!(at(&format!("file://{}/docs/img/a%20b.png", repo.display())), in_repo("docs/img/a b.png"));
        for out in ["../../secret.png", "//host/x.png", "https://x.test/r.png", "", "img/a%2Fb.png"] {
            assert_eq!(at(out), None, "{out}");
        }
        assert_eq!(at(&format!("file://{}/secret.png", top.display())), None);
        assert_eq!(at(&format!("file://{}/../secret.png", repo.display())), None);
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn a_symlink_then_dots_cannot_leave_the_repo() {
        let top = std::env::temp_dir().join(format!("mddog-media-link-{}", std::process::id()));
        let repo = top.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("docs/img/x/y")).unwrap();
        std::os::unix::fs::symlink("img/x/y", repo.join("docs/l")).unwrap();
        // On disk l/../../.. is docs, where this decoy sits. WebKit reads it as the folder above the repo.
        std::fs::write(repo.join("docs/secret.png"), b"x").unwrap();
        std::fs::write(top.join("secret.png"), b"x").unwrap();
        std::fs::write(repo.join("docs/img/x/y/in.png"), b"x").unwrap();
        // Decoys for paths a loose decode would check in place of the ones WebKit opens.
        for d in ["docs/l\u{2}", "docs/l\u{fffd}"] {
            std::fs::create_dir_all(repo.join(d)).unwrap();
            std::fs::write(repo.join(d).join("in.png"), b"x").unwrap();
        }
        let dir = std::fs::canonicalize(repo.join("docs")).unwrap();
        let html = render_in("![a](l/../../../secret.png) ![b](l%2F..%2F..%2F..%2Fsecret.png) ![c](l/in.png) ![d](l%+2/in.png) ![e](l%ff/in.png)\n", Some(&dir)).html;
        let _ = std::fs::remove_dir_all(&top);
        assert!(!html.contains("src=\"l/..") && !html.contains("src=\"l%2F") && !html.contains("src=\"l%+2") && !html.contains("src=\"l%ff"), "{html}");
        assert!(html.contains("src=\"l/in.png\""), "{html}");
    }

    #[test]
    fn media_root_is_the_repo_then_home_then_the_folder() {
        let top = std::env::temp_dir().join(format!("mddog-root-{}", std::process::id()));
        for d in ["home/repo/.git", "home/repo/docs", "home/notes/a", "home/sibling", "other"] {
            std::fs::create_dir_all(top.join(d)).unwrap();
        }
        std::fs::write(top.join("home/sibling/img.png"), b"x").unwrap();
        let top = std::fs::canonicalize(&top).unwrap();
        let home = top.join("home");
        let at = |d: &str| media_root(&top.join(d), Some(&home));
        let (repo, notes, other) = (at("home/repo/docs"), at("home/notes/a"), at("other"));
        let dir = top.join("home/notes/a");
        let sibling = local_media("../../sibling/img.png", &dir, &notes);
        let rooted = local_media("/sibling/img.png", &dir, &notes);
        let _ = std::fs::remove_dir_all(&top);
        assert_eq!((repo, notes, other), (top.join("home/repo"), home, top.join("other")));
        assert_eq!(sibling.as_deref(), Some("../../sibling/img.png"));
        assert_eq!(rooted.as_deref(), Some("../../sibling/img.png"));
    }

    #[test]
    fn a_missing_or_root_home_keeps_the_folder() {
        let dir = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        for home in [None, Some(Path::new("")), Some(Path::new("/"))] {
            assert_eq!(media_root(&dir, home), dir, "{home:?}");
        }
    }

    #[test]
    fn escaped_dollar_stays_in_code() {
        for src in ["<pre>\necho \\$HOME\n</pre>\n", ">     echo \\$HOME\n", "> ~~~\n> echo \\$HOME\n> ~~~\n"] {
            let html = render(src).html;
            assert!(html.contains("echo \\$HOME"), "{html}");
        }
        let html = render("\u{feff}```\n$a_b$\n```\n").html;
        assert!(html.contains("$a_b$"), "{html}");
    }

    #[test]
    fn a_deep_quote_takes_linear_time() {
        let s = format!("{}$$\n{}", "> ".repeat(200_000), "x\n".repeat(200_000));
        let t = std::time::Instant::now();
        protect_math(&s);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn openers_without_closers_take_linear_time() {
        let s = "($a$a".repeat(100_000);
        let t = std::time::Instant::now();
        assert!(inline_math(&s, true).is_empty() && inline_math(&s, false).is_empty());
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn raw_footnote_anchors_get_prefixed_ids() {
        let html = render("<sup><a data-footnote-ref href=\"#\">1</a><a data-footnote-ref id=\"pele\" name=\"x\" href=\"#\">2</a></sup>\n").html;
        assert!(!html.contains("id=\"pele\"") && !html.contains("name=\"x\""), "{html}");
        assert_eq!(html.matches("id=\"user-content-").count(), 2, "{html}");
    }

    #[test]
    fn math_keeps_stars_and_underscores() {
        let html = api_html("Inline $a*b*c$ and $x_1 + y_2$\n\n$$\na_i * b_j\n$$\n");
        assert!(html.contains(">$a*b*c$</math-renderer>") && !html.contains("<em>"), "{html}");
        assert!(html.contains("$x_1 + y_2$") && html.contains("a_i * b_j"), "{html}");
    }

    #[test]
    fn math_openers_and_closers_follow_github() {
        let spans = |s: &str| inline_math(s, false).into_iter().map(|(a, b)| &s[a..b]).collect::<Vec<_>>().join(" ");
        assert_eq!(spans("$a$/$b$ \"$c$\" ($d$) x$e$"), "$a$ $d$");
        assert_eq!(spans("$a$b c$ and $f(p)$) and $x$_ and $y$1"), "");
        assert_eq!(spans("costs $5, and is $\\alpha = 1$ off"), "$5, and is $");
        assert_eq!(spans("c $z $ d $w $( e $v $"), "$w $");
        assert_eq!(spans("$a +\nb$ and $$x $$ y"), "$$x $$");
    }

    #[test]
    fn a_paragraph_of_one_line_display_math_is_display() {
        let html = api_html("$$ \\{a_1 * b_1\\} $$\n\n$$x_2$$<br><br>\n\nText $$y$$\n");
        assert!(html.contains("js-display-math\" style=\"display: block\" data-run-id=\"0f23d3dd37596070b592c1adaa9bf6c8\">$$ \\{a_1 * b_1\\} $$</math-renderer>"), "{html}");
        assert!(html.contains(">$$x_2$$</math-renderer><br><br>"), "{html}");
        assert!(html.contains(">$$y$$</math-renderer>") && html.matches("js-display-math").count() == 2, "{html}");
    }

    #[test]
    fn math_in_tables_quotes_and_html_keeps_its_text() {
        let html = api_html("| a |\n|---|\n| $x \\| y$ and $\\|v\\|$ |\n\n> $$\n> a_1 * b_1 \\{c\\}\n> $$\n\n<summary>Sum $x_1$ and $y_2$</summary>\n\n$\\href{https://a.org/}{\\KaTeX}$\n");
        assert!(html.contains(">$x | y$</math-renderer>") && html.contains(">$|v|$</math-renderer>"), "{html}");
        assert!(html.contains("a_1 * b_1 \\{c\\}"), "{html}");
        assert!(html.contains(">$x_1$</math-renderer> and"), "{html}");
        assert!(html.contains("{\\KaTeX}$") && !html.contains("\\\\KaTeX"), "{html}");
    }

    #[test]
    fn the_source_pass_escapes_only_what_becomes_math() {
        let html = api_html("costs $5\nand $\\{x\\}$\n\n$a *em*\nb$\n\n<pre>$x_1$</pre>\n\n<!-- c -->\n$a *b*$\n\n<https://example.com>\n$c *d*$\n");
        assert!(html.contains(">$\\{x\\}$</math-renderer>") && html.contains(">em</em>"), "{html}");
        assert!(html.contains(">$x_1$</pre>"), "{html}");
        assert!(html.contains(">$a *b*$</math-renderer>") && html.contains(">$c *d*$</math-renderer>"), "{html}");
        let html = api_html(">     $$\n>     a_b\n>     $$\n\n> ```\n> $$\n> c_d * e\n> $$\n> ```\n");
        assert!(html.contains("a_b") && html.contains("c_d * e") && !html.contains("\\_"), "{html}");
        let html = api_html("> $$\n\nplain *em*\n$$\n\n> ```\n> x\n\n$a_b * c_d$\n\n$$ `e_f` $$\n");
        assert!(html.contains(">em</em>") && html.contains(">$a_b * c_d$</math-renderer>") && html.contains(">e_f</code>"), "{html}");
        let html = api_html("```\n> ```\n$x_1 * y$\n```\n\n![Cost \\$5](x.png)\n\nBefore $\\|x\\|$\n| a |\n|---|\n| $x \\| y$ |\n\n$\\|z\\|$\n--- | ---\n");
        assert!(html.contains("$x_1 * y$") && !html.contains("x\\_1") && html.contains("alt=\"Cost $5\""), "{html}");
        assert!(html.contains(">$\\|x\\|$</math-renderer>") && html.contains(">$x | y$</math-renderer>") && html.contains(">$\\|z\\|$</math-renderer>"), "{html}");
    }

    #[test]
    fn padded_remote_urls_and_autoplay_are_deferred() {
        let html = render("<img src=\" http://x.test/a.png\">\n\n<video autoplay src=\"https://x.test/v.mp4\" poster=\" https://x.test/p.png\"></video>\n").html;
        assert!(html.contains("data-mddog-src=\" http://x.test/a.png\""), "{html}");
        assert!(html.contains("data-mddog-poster=") && !html.contains("autoplay"), "{html}");
        assert!(html.contains("data-mddog-media=\"https://x.test/v.mp4\"") && !html.contains(" src=\"https://x.test/v.mp4\""), "{html}");
    }

    #[test]
    fn root_image_gets_a_paragraph_and_picture_image_no_style() {
        let root = api_html("<img src=\"a.png\" alt=\"i\">\n\n<figure><img src=\"f.png\"><figcaption>cap</figcaption></figure>\n");
        assert_eq!(root.matches("<p><a target=\"_blank\"").count(), 2, "{root}");
        let picture = api_html("<picture><source srcset=\"d.png\"><img src=\"l.png\" alt=\"x\"></picture>\n");
        assert!(picture.contains("<img src=\"l.png\" alt=\"x\">"), "{picture}");
        let inline = api_html("text <img src=\"b.png\"> more\n");
        assert!(!inline.contains("<p><p>") && inline.matches("<p").count() == 1, "{inline}");
    }

    #[test]
    fn deep_nesting_renders_on_a_small_stack() {
        let divs = format!("{}x{}", "<div>".repeat(10_000), "</div>".repeat(10_000));
        let quotes = format!("{}y", "> ".repeat(3_000));
        let out = std::thread::Builder::new().stack_size(2 << 20).spawn(move || (render(&divs).html, render(&quotes).html)).unwrap().join().unwrap();
        assert!(out.0.contains('x'));
        assert!(out.1.contains('y'));
    }

    #[test]
    fn first_node_is_sanitized() {
        let img = render("<img src=x onerror=alert(1)>\n").html;
        assert!(!img.contains("onerror"), "{img}");
        let center = render("<center>x</center>\n").html;
        assert!(!center.contains("<center"), "{center}");
        let p = render("<p style=\"color:red\" id=\"y\">p</p>\n").html;
        assert!(!p.contains("style=") && p.contains("user-content-y"), "{p}");
    }

    #[test]
    fn raw_heading_keeps_attributes() {
        let html = render("<h1 align=\"center\">Title</h1>\n\n# Plain\n").html;
        assert!(html.contains("<h1 align=\"center\" class=\"heading-element\" dir=\"auto\">Title</h1>"), "{html}");
        assert!(html.contains("<h1 class=\"heading-element\" dir=\"auto\">Plain</h1>"), "{html}");
    }
}
