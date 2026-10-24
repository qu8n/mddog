use std::path::{Path, PathBuf};

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, format_html, parse_document};
use kuchikiki::NodeRef;

use super::dom::*;
use super::{emoji, highlight, octicons, sanitize};

pub const RUN_ID: &str = "0f23d3dd37596070b592c1adaa9bf6c8";

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

const HELD_DOLLAR: char = '\u{e024}';

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

fn protect_math(src: &str) -> String {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let delim = |line: &str| line.trim() == "$$" && !line.starts_with("    ") && !line.starts_with('\t');
    let mut out = String::with_capacity(src.len() + 16);
    let mut display = false;
    let mut chunk = String::new();
    let flush = |chunk: &mut String, out: &mut String| {
        if chunk.starts_with("    ") || chunk.starts_with('\t') {
            out.push_str(chunk);
        } else {
            let masked = mask_code_spans(chunk);
            let mut last = 0;
            for (a, b) in inline_math(&masked).into_iter().filter(|&(a, b)| !chunk[a..b].contains('`')) {
                out.push_str(&hold_dollars(&chunk[last..a], &masked[last..a]));
                out.push_str(&escape_math(&chunk[a..b]));
                last = b;
            }
            out.push_str(&hold_dollars(&chunk[last..], &masked[last..]));
        }
        chunk.clear();
    };
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let fences: Vec<Fence> = lines
        .iter()
        .scan(None, |fence: &mut Option<(char, usize)>, line| {
            let t = line.trim();
            let ticks = t.chars().take_while(|c| matches!(c, '`' | '~')).count();
            let open = t.chars().next().filter(|_| ticks >= 3);
            Some(if let Some((c, n)) = *fence {
                if open == Some(c) && ticks >= n && t[ticks..].trim().is_empty() {
                    *fence = None;
                }
                Fence::Inside
            } else if let Some(c) = open.filter(|c| *c == '~' || !t[ticks..].contains('`')) {
                *fence = Some((c, ticks));
                Fence::Opens
            } else {
                Fence::Outside
            })
        })
        .collect();
    let mut closes_later = vec![false; lines.len() + 1];
    for i in (0..lines.len()).rev() {
        closes_later[i] = closes_later[i + 1] || (fences[i] == Fence::Outside && delim(lines[i]));
    }
    for (i, line) in lines.iter().enumerate() {
        match fences[i] {
            Fence::Inside => out.push_str(line),
            Fence::Opens => {
                flush(&mut chunk, &mut out);
                out.push_str(line);
            }
            Fence::Outside if delim(line) && (display || closes_later[i + 1]) => {
                flush(&mut chunk, &mut out);
                display = !display;
                out.push_str(line);
            }
            Fence::Outside if display => out.push_str(&escape_math(line)),
            Fence::Outside if line.trim().is_empty() => {
                flush(&mut chunk, &mut out);
                out.push_str(line);
            }
            Fence::Outside => chunk.push_str(line),
        }
    }
    flush(&mut chunk, &mut out);
    out
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
    if t.starts_with("//") || t.contains(|c: char| c == '\\' || c.is_control()) {
        return None;
    }
    let path = t.split(['?', '#']).next().unwrap_or_default();
    let path = percent_decode(path);
    let full = match path.strip_prefix('/') {
        Some(rest) => root.join(rest),
        None => dir.join(&path),
    };
    if !std::fs::canonicalize(full).ok()?.starts_with(root) {
        return None;
    }
    match t.strip_prefix('/') {
        Some(rest) => Some(format!("{}{rest}", "../".repeat(dir.strip_prefix(root).ok()?.components().count()))),
        None => Some(url.to_string()),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())) {
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
    String::from_utf8_lossy(&out).into_owned()
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

pub fn render(src: &str, defer_remote: bool) -> Doc {
    render_in(src, defer_remote, None)
}

pub fn render_in(src: &str, defer_remote: bool, dir: Option<&Path>) -> Doc {
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
    if defer_remote {
        let remote = is_remote;
        for v in elements(&body).into_iter().filter(|n| is(n, "video")) {
            let media = std::iter::once(v.clone()).chain(v.children().filter(|c| is(c, "source")));
            if !attr(&v, "poster").is_some_and(|p| remote(&p)) && !media.into_iter().any(|m| attr(&m, "src").is_some_and(|s| remote(&s))) {
                continue;
            }
            set_attr(&v, "preload", "none");
            remove_attr(&v, "autoplay");
            if let Some(poster) = attr(&v, "poster").filter(|p| remote(p)) {
                set_attr(&v, "data-mddog-poster", &poster);
                remove_attr(&v, "poster");
            }
            for m in std::iter::once(v.clone()).chain(v.children().filter(|c| is(c, "source"))) {
                if let Some(src) = attr(&m, "src").filter(|s| remote(s)) {
                    set_attr(&m, "data-mddog-media", &src);
                    remove_attr(&m, "src");
                }
            }
        }
        for n in elements(&body).into_iter().filter(|n| is(n, "source") || is(n, "img")) {
            if let Some(set) = attr(&n, "srcset").filter(|s| s.split(',').any(remote)) {
                set_attr(&n, "data-mddog-srcset", &set);
                remove_attr(&n, "srcset");
            }
        }
        for img in elements(&body).into_iter().filter(|n| is(n, "img")) {
            let src = attr(&img, "src").unwrap_or_default();
            let sources = attr(&img, "data-mddog-srcset").is_some() || img.parent().is_some_and(|p| is(&p, "picture") && p.children().any(|c| attr(&c, "data-mddog-srcset").is_some()));
            if remote(&src) || sources {
                set_attr(&img, "data-mddog-src", &src);
                set_attr(&img, "src", BLANK_GIF);
            }
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
        if p.first_child().is_some_and(|c| is(&c, "br")) {
            p.first_child().unwrap().detach();
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
    if attr(a, "target").is_some() || attr(a, "data-footnote-ref").is_some() || attr(a, "data-footnote-backref").is_some() || a.ancestors().any(|x| has_class(&x, "footnotes")) {
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
    let checked = attr(&input, "checked").is_some();
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
    for a in fresh.descendants().filter(|n| attr(n, "data-footnote-backref").is_some()).collect::<Vec<_>>() {
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
    let Some(a) = sup.children().find(|c| attr(c, "data-footnote-ref").is_some()) else { return };
    remove_attr(sup, "class");
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
        let (b, a) = (before.borrow().clone(), after.borrow().clone());
        let (Some(b), Some(a)) = (b.strip_suffix('$'), a.strip_prefix('$')) else { continue };
        *before.borrow_mut() = b.to_string();
        *after.borrow_mut() = a.to_string();
        let math = el("math-renderer", &[("class", "js-inline-math"), ("style", "display: inline-block"), ("data-run-id", RUN_ID)]);
        math.append(NodeRef::new_text(format!("${}$", escape(&code.text_contents()))));
        replace(&code, &math);
    }
    for p in body.descendants().filter(|n| is(n, "p")).collect::<Vec<_>>() {
        let kids: Vec<NodeRef> = p.children().collect();
        if kids.is_empty() || !kids.iter().all(|k| k.as_text().is_some() || is(k, "br")) {
            continue;
        }
        let s: String = kids.iter().filter_map(|k| k.as_text().map(|t| t.borrow().clone())).collect();
        let Some(inner) = s.strip_prefix("$$").and_then(|r| r.strip_suffix("$$")) else { continue };
        if inner.starts_with('\n') && inner.ends_with('\n') && !inner.contains("$$") {
            for k in kids {
                k.detach();
            }
            for f in fragment(&format!("<math-renderer class=\"js-display-math\" style=\"display: block\" data-run-id=\"{RUN_ID}\">{}</math-renderer>", escape(&s))) {
                p.append(f);
            }
        }
    }
    for text in body.descendants().filter(|n| n.as_text().is_some()).collect::<Vec<_>>() {
        if in_code(&text) {
            continue;
        }
        let s = text.as_text().unwrap().borrow().clone();
        if let Some(a) = link_of(&text) {
            let replaced = if shows_own_url(&a) { s.clone() } else { emoji::replace(&s) };
            if replaced != s {
                *text.as_text().unwrap().borrow_mut() = replaced;
            }
            continue;
        }
        let mut pieces: Vec<String> = Vec::new();
        let mut last = 0;
        for (a, b) in inline_math(&s) {
            if a > last {
                pieces.push(escape(&emoji::replace(&s[last..a])));
            }
            pieces.push(format!("<math-renderer class=\"js-inline-math\" style=\"display: inline-block\" data-run-id=\"{RUN_ID}\">{}</math-renderer>", escape(&s[a..b])));
            last = b;
        }
        if last == 0 {
            let replaced = emoji::replace(&s);
            if replaced != s {
                *text.as_text().unwrap().borrow_mut() = replaced;
            }
            continue;
        }
        if last < s.len() {
            pieces.push(escape(&emoji::replace(&s[last..])));
        }
        for f in fragment(&pieces.concat()) {
            text.insert_before(f);
        }
        text.detach();
    }
    for node in body.descendants() {
        if let Some(text) = node.as_text().filter(|t| t.borrow().contains(HELD_DOLLAR)) {
            let t = text.borrow().replace(HELD_DOLLAR, if in_code(&node) { "\\$" } else { "$" });
            *text.borrow_mut() = t;
        }
    }
}

pub fn inline_math(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let escaped = |i: usize| b[..i].iter().rev().take_while(|&&c| c == b'\\').count() % 2 == 1;
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' || escaped(i) {
            i += 1;
            continue;
        }
        let delim = if b.get(i + 1) == Some(&b'$') { 2 } else { 1 };
        let open_end = i + delim;
        let prev_ok = !s[..i].chars().next_back().is_some_and(char::is_alphanumeric);
        let next_ok = s[open_end..].chars().next().is_some_and(|c| !c.is_whitespace() && c != '$');
        if !prev_ok || !next_ok {
            i += delim;
            continue;
        }
        let mut found = None;
        let mut j = open_end;
        while j < b.len() {
            if b[j] == b'$' && !escaped(j) && (delim == 1 || b.get(j + 1) == Some(&b'$')) {
                if s[..j].chars().next_back().is_none_or(char::is_whitespace) {
                    break;
                }
                let after = j + delim;
                let after_ok = s[after..].chars().next().is_none_or(|c| !c.is_alphanumeric());
                if after_ok {
                    found = Some(after);
                    break;
                }
            }
            j += 1;
        }
        match found {
            Some(end) => {
                out.push((i, end));
                i = end;
            }
            None => i += delim,
        }
    }
    out
}

fn raw_autolinks(body: &NodeRef) {
    for text in body.descendants().filter(|n| n.as_text().is_some()).collect::<Vec<_>>() {
        if in_code(&text) || link_of(&text).is_some() {
            continue;
        }
        let s = text.as_text().unwrap().borrow().clone();
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
            text.insert_before(f);
        }
        text.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_html_cannot_write_source_positions() {
        let html = render("<p DATA-SOURCEPOS = \"1:1-1:10\">x</p>\n\n<ul><li/data-sourcepos=\"1:1-1:10\">y</li></ul>\n", false).html;
        assert!(!html.contains("data-sourcepos=\"1:1-1:10\""), "{html}");
        let html = render("<p title=\"data-sourcepos\">data-sourcepos = x</p>\n", false).html;
        assert!(html.contains(">data-sourcepos = x<") && html.contains("title=\"data-sourcepos\""), "{html}");
    }

    #[test]
    fn remote_picture_sources_are_deferred() {
        let html = render("<picture>\n<source media=\"(prefers-color-scheme: dark)\" srcset=\"https://x.test/d.png\">\n<img alt=\"a\" src=\"local.png\">\n</picture>\n", true).html;
        assert!(!html.contains(" srcset="), "{html}");
        assert!(html.contains("data-mddog-srcset=\"https://x.test/d.png\""), "{html}");
        assert!(html.contains("data-mddog-src=\"local.png\""), "{html}");
    }

    #[test]
    fn emoji_in_link_text_unless_it_is_the_url() {
        let html = render("[Sponsors :heart:](#x) <https://example.com/:smile:/x> [see $x$ here](#a) <img src=\"HTTPS://x.test/a.png\">\n", true).html;
        assert!(html.contains(">Sponsors \u{2764}"), "{html}");
        assert!(html.contains(">https://example.com/:smile:/x<"), "{html}");
        assert!(html.contains(">see $x$ here<"), "{html}");
        assert!(html.contains("data-mddog-src=\"HTTPS://x.test/a.png\""), "{html}");
    }

    #[test]
    fn remote_video_and_mixed_srcset_are_deferred() {
        let html = render(
            "<video src=\"https://x.test/v.mp4\" poster=\"https://x.test/p.png\"></video>\n\n<picture>\n<source srcset=\"a.png 1x, https://x.test/b.png 2x\">\n<img src=\"a.png\">\n</picture>\n",
            true,
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
        let local = render("<video src=\"clip.mp4\"></video>\n", true).html;
        assert!(!local.contains("preload"), "{local}");
    }

    #[test]
    fn raw_autolink_ends_at_quote_or_angle() {
        let html = render("<iframe src=\"https://example.com\"></iframe>\n\n<iframe>see https://a.test/x&lt;b</iframe>\n", true).html;
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
        let html = render("Price \\$5 and \\$10, math $x_1$.\n", false).html;
        assert!(!html.contains(HELD_DOLLAR) && html.contains("Price $5 and $10"), "{html}");
        let html = api_html("Price \\$k$ here and $l\\$ m$\n");
        assert!(!html.contains(HELD_DOLLAR) && html.contains("Price $k$ here"), "{html}");
        assert!(!html.contains(">$k$</math-renderer>"), "{html}");
        assert!(html.contains("$l\\$ m$</math-renderer>"), "{html}");
    }

    #[test]
    fn unmatched_display_math_leaves_the_rest_alone() {
        let html = render("$$\n\nSome *em* and `a\\b`\n", false).html;
        assert!(html.contains("<em>em</em>") && html.contains("<code>a\\b</code>"), "{html}");
        let html = render("$$\n\n```\n$$\n```\n\nSome *em*\n", false).html;
        assert!(html.contains("<em>em</em>"), "{html}");
        let html = render("    $$\n    a\\b\n    $$\n", false).html;
        assert!(html.contains("a\\b") && !html.contains("a\\\\b"), "{html}");
    }

    #[test]
    fn slugs_keep_what_github_keeps() {
        let html = render("# हिन्दी\n\n# เมื่อ\n\n# x²\n\n# a‿b\n\n# Café\n\n# Café\n", false).html;
        for id in ["हिन्दी", "เมื่อ", "\"user-content-x\"", "a‿b", "\"user-content-café\"", "\"user-content-café-1\""] {
            assert!(html.contains(id), "{id}: {html}");
        }
    }

    #[test]
    fn footnote_links_reach_their_targets() {
        let html = render("See[^n].\n\n[^n]: Note.\n", false).html;
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
        let html = render_in(&src, false, Some(&dir)).html;
        let _ = std::fs::remove_dir_all(&top);
        assert!(html.contains("src=\"img/a%20b.png\"") && html.contains("src=\"../logo.png\"") && html.contains("src=\"https://x.test/r.png\""), "{html}");
        assert!(!html.contains("src=\"..\\") && !html.contains("src=\".\t") && !html.contains(&format!("src=\"{outside}")) && !html.contains("src=\"//host"), "{html}");
        assert!(html.contains("srcset=\"img/a%20b.png 1x\""), "{html}");
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
            let html = render(src, false).html;
            assert!(html.contains("echo \\$HOME"), "{html}");
        }
        let html = render("\u{feff}```\n$a_b$\n```\n", false).html;
        assert!(html.contains("$a_b$"), "{html}");
    }

    #[test]
    fn math_keeps_stars_and_underscores() {
        let html = api_html("Inline $a*b*c$ and $x_1 + y_2$\n\n$$\na_i * b_j\n$$\n");
        assert!(html.contains(">$a*b*c$</math-renderer>") && !html.contains("<em>"), "{html}");
        assert!(html.contains("$x_1 + y_2$") && html.contains("a_i * b_j"), "{html}");
    }

    #[test]
    fn padded_remote_urls_and_autoplay_are_deferred() {
        let html = render("<img src=\" http://x.test/a.png\">\n\n<video autoplay src=\"https://x.test/v.mp4\" poster=\" https://x.test/p.png\"></video>\n", true).html;
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
        let out = std::thread::Builder::new().stack_size(2 << 20).spawn(move || (render(&divs, true).html, render(&quotes, true).html)).unwrap().join().unwrap();
        assert!(out.0.contains('x'));
        assert!(out.1.contains('y'));
    }

    #[test]
    fn first_node_is_sanitized() {
        let img = render("<img src=x onerror=alert(1)>\n", false).html;
        assert!(!img.contains("onerror"), "{img}");
        let center = render("<center>x</center>\n", false).html;
        assert!(!center.contains("<center"), "{center}");
        let p = render("<p style=\"color:red\" id=\"y\">p</p>\n", false).html;
        assert!(!p.contains("style=") && p.contains("user-content-y"), "{p}");
    }

    #[test]
    fn raw_heading_keeps_attributes() {
        let html = render("<h1 align=\"center\">Title</h1>\n\n# Plain\n", false).html;
        assert!(html.contains("<h1 align=\"center\" class=\"heading-element\" dir=\"auto\">Title</h1>"), "{html}");
        assert!(html.contains("<h1 class=\"heading-element\" dir=\"auto\">Plain</h1>"), "{html}");
    }
}
