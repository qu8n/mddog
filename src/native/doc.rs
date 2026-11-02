//! Markdown to a block tree. Inline content is flat runs with style flags.
use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Sty {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub kbd: bool,
    pub sub: bool,
    pub sup: bool,
    pub under: bool,
    pub mark: bool,
    /// 0 = no link, else index into `Doc::links` plus one.
    pub link: u32,
}

#[derive(Clone, Debug)]
pub enum Inl {
    Text(String, Sty),
    Soft,
    Hard,
    Img { src: String, alt: String, w: Option<f32>, link: u32 },
    Math(String, Sty),
    FootRef(usize, String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Clone, Debug)]
pub struct LItem {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Para(Vec<Inl>),
    Heading(u8, Vec<Inl>, String),
    Code { lang: String, text: String },
    Quote(Vec<Block>),
    Alert(Alert, Vec<Block>),
    List { start: Option<u64>, items: Vec<LItem>, tight: bool },
    Table { aligns: Vec<u8>, head: Vec<Vec<Inl>>, rows: Vec<Vec<Vec<Inl>>> },
    Rule,
    Math(String),
    Mermaid(String),
    Details { summary: Vec<Inl>, open: bool, body: Vec<Block>, id: usize },
    Align(u8, Vec<Block>),
    Footnotes(Vec<(usize, String, Vec<Block>)>),
}

pub struct Doc {
    pub blocks: Vec<Block>,
    pub links: Vec<String>,
    /// The 1-based source lines of each top-level block, where it has them.
    pub lines: Vec<Option<(u32, u32)>>,
}

enum Fk {
    Root,
    Quote(Option<BlockQuoteKind>),
    Item,
    List(Option<u64>),
    Details { open: bool, id: usize },
    Align(u8, &'static str),
    Html(&'static str),
    Heading(u8, String),
    Foot(String),
}

struct Frame {
    kind: Fk,
    blocks: Vec<Block>,
    summary: Vec<Inl>,
    task: Option<bool>,
    loose: bool,
    items: Vec<LItem>,
}

fn frame(kind: Fk) -> Frame {
    Frame { kind, blocks: vec![], summary: vec![], task: None, loose: false, items: vec![] }
}

struct TableB {
    aligns: Vec<u8>,
    head: Vec<Vec<Inl>>,
    rows: Vec<Vec<Vec<Inl>>>,
    row: Vec<Vec<Inl>>,
}

#[derive(Default)]
struct Counts {
    bold: u32,
    italic: u32,
    strike: u32,
    code: u32,
    kbd: u32,
    sub: u32,
    sup: u32,
    under: u32,
    mark: u32,
}

struct B {
    stack: Vec<Frame>,
    inl: Vec<Inl>,
    cnt: Counts,
    link: Vec<u32>,
    links: Vec<String>,
    code: Option<(String, String)>,
    table: Option<TableB>,
    foot_order: Vec<String>,
    foots: HashMap<String, Vec<Block>>,
    skip: Option<String>,
    in_summary: bool,
    anchors: HashMap<String, usize>,
    details_n: usize,
    in_image: Option<(String, String, Vec<Inl>)>,
    html_buf: String,
}

impl B {
    fn sty(&self) -> Sty {
        let c = &self.cnt;
        Sty {
            bold: c.bold > 0,
            italic: c.italic > 0,
            strike: c.strike > 0,
            code: c.code > 0,
            kbd: c.kbd > 0,
            sub: c.sub > 0,
            sup: c.sup > 0,
            under: c.under > 0,
            mark: c.mark > 0,
            link: self.link.last().copied().unwrap_or(0),
        }
    }

    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let st = self.sty();
        if self.in_image.is_some() {
            return;
        }
        self.inl.push(Inl::Text(s.to_string(), st));
    }

    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().unwrap()
    }

    fn push_block(&mut self, b: Block) {
        self.top().blocks.push(b);
    }

    /// Turn pending inline content into a paragraph (tight list items and html text).
    fn flush(&mut self) {
        if self.in_summary {
            return;
        }
        let v = std::mem::take(&mut self.inl);
        let v = finish_inlines(v, &mut self.links);
        if v.iter().any(|i| !matches!(i, Inl::Text(t, _) if t.trim().is_empty()) && !matches!(i, Inl::Soft)) {
            self.push_block(Block::Para(v));
        }
    }

    fn pop_frame(&mut self) -> Frame {
        self.stack.pop().unwrap()
    }

    fn end_html_container(&mut self, name: &str) {
        // find the nearest matching html frame
        let pos = self.stack.iter().rposition(|f| match &f.kind {
            Fk::Html(n) => *n == name,
            Fk::Align(_, n) => *n == name,
            Fk::Details { .. } => name == "details",
            Fk::Heading(..) => name.len() == 2 && name.starts_with('h'),
            _ => false,
        });
        let Some(pos) = pos else { return };
        if !matches!(self.stack[pos].kind, Fk::Heading(..)) {
            self.flush();
        }
        while self.stack.len() > pos + 1 {
            self.close_frame();
        }
        self.close_frame();
    }

    /// Close html frames (details, align) left open inside a markdown container.
    fn close_html_frames(&mut self) {
        while self.stack.len() > 1 && matches!(self.stack.last().map(|f| &f.kind), Some(Fk::Html(_) | Fk::Align(..) | Fk::Details { .. })) {
            self.flush();
            self.close_frame();
        }
    }

    fn close_frame(&mut self) {
        let f = self.pop_frame();
        let blocks = f.blocks;
        let b = match f.kind {
            Fk::Html(_) => {
                for b in blocks {
                    self.push_block(b);
                }
                return;
            }
            Fk::Align(a, _) => {
                if a == 0 {
                    for b in blocks {
                        self.push_block(b);
                    }
                    return;
                }
                Block::Align(a, blocks)
            }
            Fk::Details { open, id } => Block::Details { summary: f.summary, open, body: blocks, id },
            Fk::Quote(k) => match k {
                None => Block::Quote(blocks),
                Some(k) => Block::Alert(
                    match k {
                        BlockQuoteKind::Note => Alert::Note,
                        BlockQuoteKind::Tip => Alert::Tip,
                        BlockQuoteKind::Important => Alert::Important,
                        BlockQuoteKind::Warning => Alert::Warning,
                        BlockQuoteKind::Caution => Alert::Caution,
                    },
                    blocks,
                ),
            },
            Fk::Heading(l, id) => {
                let v = std::mem::take(&mut self.inl);
                let v = finish_inlines(v, &mut self.links);
                let id = if id.is_empty() { slug(&plain(&v)) } else { id };
                Block::Heading(l, v, id)
            }
            _ => {
                for b in blocks {
                    self.push_block(b);
                }
                return;
            }
        };
        self.push_block(b);
    }

    fn html(&mut self, h: &str) {
        // Inline HTML splits the text around it, so `$x<y>z$` is not math (GitHub drops `<y>`).
        if matches!(self.inl.last(), Some(Inl::Text(..))) {
            self.text(&HTML_BREAK.to_string());
        }
        for t in tokenize(h) {
            match t {
                Tok::Text(s) => {
                    if self.skip.is_some() {
                        continue;
                    }
                    let s = collapse_ws(&s);
                    if self.in_summary || !s.trim().is_empty() || !self.inl.is_empty() {
                        self.text(&s);
                    }
                }
                Tok::Open(name, attrs) => self.open_tag(&name, &attrs),
                Tok::Close(name) => self.close_tag(&name),
            }
        }
    }

    fn open_tag(&mut self, name: &str, attrs: &[(String, String)]) {
        if let Some(s) = &self.skip {
            let _ = s;
            return;
        }
        let attr = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        match name {
            "script" | "iframe" | "style" | "object" | "embed" | "template" => {
                self.skip = Some(name.to_string());
            }
            "details" => {
                self.flush();
                self.details_n += 1;
                let id = self.details_n;
                self.stack.push(frame(Fk::Details { open: attr("open").is_some(), id }));
            }
            "summary" => {
                self.flush();
                self.in_summary = true;
                self.inl.clear();
            }
            "div" | "p" | "center" | "section" | "article" | "header" | "footer" | "main" | "nav" | "figure" | "figcaption" | "picture" => {
                self.flush();
                let a = match (name, attr("align").as_deref()) {
                    ("center", _) | (_, Some("center")) => 1,
                    (_, Some("right")) => 2,
                    _ => 0,
                };
                let n: &'static str = match name {
                    "div" => "div",
                    "p" => "p",
                    "center" => "center",
                    "section" => "section",
                    "figure" => "figure",
                    _ => "div",
                };
                let n = if matches!(name, "article" | "header" | "footer" | "main" | "nav" | "figcaption" | "picture") {
                    // closing tag name differs; keep a unique static per name
                    match name {
                        "article" => "article",
                        "header" => "header",
                        "footer" => "footer",
                        "main" => "main",
                        "nav" => "nav",
                        "figcaption" => "figcaption",
                        _ => "picture",
                    }
                } else {
                    n
                };
                self.stack.push(frame(Fk::Align(a, n)));
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                let l = name[1..].parse().unwrap();
                self.stack.push(frame(Fk::Heading(l, String::new())));
            }
            "br" => self.inl.push(Inl::Hard),
            "hr" => {
                self.flush();
                self.push_block(Block::Rule);
            }
            "img" => {
                let src = attr("src").unwrap_or_default();
                let alt = attr("alt").unwrap_or_default();
                let w = attr("width").and_then(|w| w.trim_end_matches("px").parse::<f32>().ok());
                let link = self.link.last().copied().unwrap_or(0);
                if self.in_image.is_none() {
                    self.inl.push(Inl::Img { src, alt, w, link });
                }
            }
            "a" => {
                let href = attr("href").unwrap_or_default();
                let id = if href.is_empty() { 0 } else { self.add_link(href) };
                self.link.push(id);
            }
            "b" | "strong" => self.cnt.bold += 1,
            "i" | "em" | "cite" | "var" => self.cnt.italic += 1,
            "del" | "s" | "strike" => self.cnt.strike += 1,
            "code" | "tt" | "samp" => self.cnt.code += 1,
            "kbd" => self.cnt.kbd += 1,
            "sub" => self.cnt.sub += 1,
            "sup" => self.cnt.sup += 1,
            "u" | "ins" => self.cnt.under += 1,
            "mark" => self.cnt.mark += 1,
            "tr" | "li" | "ul" | "ol" | "table" | "blockquote" => {
                if !self.inl.is_empty() {
                    self.inl.push(Inl::Hard);
                }
            }
            "td" | "th" => {
                if !self.inl.is_empty() {
                    self.text("  ");
                }
            }
            _ => {}
        }
    }

    fn close_tag(&mut self, name: &str) {
        if let Some(s) = &self.skip {
            if s == name {
                self.skip = None;
            }
            return;
        }
        let dec = |c: &mut u32| *c = c.saturating_sub(1);
        match name {
            "summary" => {
                self.in_summary = false;
                let v = std::mem::take(&mut self.inl);
                let v = finish_inlines(v, &mut self.links);
                if let Some(f) = self.stack.iter_mut().rev().find(|f| matches!(f.kind, Fk::Details { .. })) {
                    f.summary = v;
                }
            }
            "details" | "div" | "p" | "center" | "section" | "article" | "header" | "footer" | "main" | "nav" | "figure" | "figcaption" | "picture" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.end_html_container(name)
            }
            "a" => {
                self.link.pop();
            }
            "b" | "strong" => dec(&mut self.cnt.bold),
            "i" | "em" | "cite" | "var" => dec(&mut self.cnt.italic),
            "del" | "s" | "strike" => dec(&mut self.cnt.strike),
            "code" | "tt" | "samp" => dec(&mut self.cnt.code),
            "kbd" => dec(&mut self.cnt.kbd),
            "sub" => dec(&mut self.cnt.sub),
            "sup" => dec(&mut self.cnt.sup),
            "u" | "ins" => dec(&mut self.cnt.under),
            "mark" => dec(&mut self.cnt.mark),
            _ => {}
        }
    }

    fn add_link(&mut self, url: String) -> u32 {
        self.links.push(url);
        self.links.len() as u32
    }
}

/// Marks where inline HTML sat in merged text. Math never spans it, and it is dropped before layout.
const HTML_BREAK: char = '\u{e025}';

pub fn finish_inlines(v: Vec<Inl>, links: &mut Vec<String>) -> Vec<Inl> {
    // merge adjacent text with equal style
    let mut m: Vec<Inl> = Vec::with_capacity(v.len());
    for i in v {
        if let Inl::Text(s, st) = &i {
            if let Some(Inl::Text(ps, pst)) = m.last_mut() {
                if pst == st {
                    ps.push_str(s);
                    continue;
                }
            }
        }
        m.push(i);
    }
    // `$`x`$`: math written with a code span.
    let mut i = 0;
    while i + 2 < m.len() {
        let spanned = matches!((&m[i], &m[i + 1], &m[i + 2]), (Inl::Text(a, sa), Inl::Text(_, c), Inl::Text(z, _)) if !sa.code && c.code && a.ends_with('$') && z.starts_with('$'));
        if spanned {
            let Inl::Text(code, cst) = m[i + 1].clone() else { unreachable!() };
            if let Inl::Text(a, _) = &mut m[i] {
                a.pop();
            }
            if let Inl::Text(z, _) = &mut m[i + 2] {
                z.remove(0);
            }
            m[i + 1] = Inl::Math(format!("${code}$"), Sty { code: false, ..cst });
        }
        i += 1;
    }
    // math, then extended autolinks
    let held = crate::render::markdown::HELD_DOLLAR;
    let mut out = Vec::with_capacity(m.len());
    for i in m {
        match i {
            Inl::Text(s, st) if st.link == 0 && !st.code => {
                let n = out.len();
                for piece in s.split(HTML_BREAK).filter(|p| !p.is_empty()) {
                    split_math(piece, st, &mut out, links);
                }
                for o in &mut out[n..] {
                    if let Inl::Text(t, _) = o
                        && t.contains(held)
                    {
                        *t = t.replace(held, "$");
                    }
                }
            }
            Inl::Text(s, st) if s.contains(held) || s.contains(HTML_BREAK) => {
                let s = s.replace(HTML_BREAK, "").replace(held, if st.code { "\\$" } else { "$" });
                if !s.is_empty() {
                    out.push(Inl::Text(s, st));
                }
            }
            o => out.push(o),
        }
    }
    out
}

fn autolink(s: &str, st: Sty, links: &mut Vec<String>, out: &mut Vec<Inl>) {
    let b = s.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        let rest = &s[i..];
        let at_word = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'/' || b[i - 1] == b'@');
        let mut end = None;
        let mut url = String::new();
        if at_word && (rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("www.")) {
            let mut e = rest.find(|c: char| c.is_whitespace() || c == '<').unwrap_or(rest.len());
            e = trim_url(&rest[..e]);
            let min = if rest.starts_with("www.") { 5 } else { 9 };
            if e >= min {
                end = Some(i + e);
                url = if rest.starts_with("www.") { format!("http://{}", &rest[..e]) } else { rest[..e].to_string() };
            }
        } else if b[i] == b'@' && i > 0 {
            // email: local part backwards
            let mut st_i = i;
            while st_i > last && (b[st_i - 1].is_ascii_alphanumeric() || b"._+-".contains(&b[st_i - 1])) {
                st_i -= 1;
            }
            let dom_len = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')).unwrap_or(rest.len() - 1);
            let dom = rest[1..1 + dom_len].trim_end_matches(['.', '-', '_']);
            if st_i < i && dom.contains('.') && !dom.starts_with('.') {
                let e = i + 1 + dom.len();
                if st_i > last {
                    out.push(Inl::Text(s[last..st_i].to_string(), st));
                }
                let id = {
                    links.push(format!("mailto:{}", &s[st_i..e]));
                    links.len() as u32
                };
                out.push(Inl::Text(s[st_i..e].to_string(), Sty { link: id, ..st }));
                last = e;
                i = e;
                continue;
            }
        }
        if let Some(e) = end {
            if i > last {
                out.push(Inl::Text(s[last..i].to_string(), st));
            }
            links.push(url);
            let id = links.len() as u32;
            out.push(Inl::Text(s[i..e].to_string(), Sty { link: id, ..st }));
            last = e;
            i = e;
        } else {
            i += s[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
    }
    if last < s.len() {
        out.push(Inl::Text(s[last..].to_string(), st));
    }
}

/// Length of the URL after trimming trailing punctuation (GFM rules).
fn trim_url(u: &str) -> usize {
    let mut e = u.len();
    loop {
        let t = &u[..e];
        let Some(c) = t.chars().last() else { break };
        if "?!.,:*_~'\";<".contains(c) {
            e -= c.len_utf8();
        } else if c == ')' {
            let o = t.matches('(').count();
            let cl = t.matches(')').count();
            if cl > o {
                e -= 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    e
}

enum Tok {
    Text(String),
    Open(String, Vec<(String, String)>),
    Close(String),
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        if let Some(e) = rest.find(';').filter(|&e| e <= 10) {
            let name = &rest[1..e];
            let r = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                "rarr" => Some('→'),
                "larr" => Some('←'),
                "hellip" => Some('…'),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32),
                _ if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(c) = r {
                out.push(c);
                rest = &rest[e + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

fn tokenize(h: &str) -> Vec<Tok> {
    let mut out = vec![];
    let mut rest = h;
    while !rest.is_empty() {
        if let Some(p) = rest.find('<') {
            if p > 0 {
                out.push(Tok::Text(decode_entities(&rest[..p])));
            }
            rest = &rest[p..];
            if rest.starts_with("<!--") {
                match rest.find("-->") {
                    Some(e) => rest = &rest[e + 3..],
                    None => rest = "",
                }
                continue;
            }
            let nc = rest[1..].chars().next().unwrap_or(' ');
            let e = rest.find('>');
            if !(nc.is_ascii_alphabetic() || nc == '/') || e.is_none() {
                out.push(Tok::Text("<".to_string()));
                rest = &rest[1..];
                continue;
            }
            let e = e.unwrap();
            let inner = &rest[1..e];
            rest = &rest[e + 1..];
            if let Some(name) = inner.strip_prefix('/') {
                out.push(Tok::Close(name.trim().to_ascii_lowercase()));
            } else {
                let inner = inner.trim_end_matches('/');
                let (name, attrs) = match inner.find(char::is_whitespace) {
                    Some(sp) => (&inner[..sp], &inner[sp..]),
                    None => (inner, ""),
                };
                out.push(Tok::Open(name.to_ascii_lowercase(), parse_attrs(attrs)));
            }
        } else {
            out.push(Tok::Text(decode_entities(rest)));
            break;
        }
    }
    out
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        let st = i;
        while i < b.len() && !b[i].is_whitespace() && b[i] != '=' {
            i += 1;
        }
        if st == i {
            break;
        }
        let name: String = b[st..i].iter().collect::<String>().to_ascii_lowercase();
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < b.len() && b[i] == '=' {
            i += 1;
            while i < b.len() && b[i].is_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == '"' || b[i] == '\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                val = b[vs..i].iter().collect();
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_whitespace() {
                    i += 1;
                }
                val = b[vs..i].iter().collect();
            }
        }
        out.push((name, decode_entities(&val)));
    }
    out
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut sp = false;
    for c in s.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !sp {
                out.push(' ');
            }
            sp = true;
        } else {
            out.push(c);
            sp = false;
        }
    }
    out
}

pub fn plain(v: &[Inl]) -> String {
    let mut s = String::new();
    for i in v {
        match i {
            Inl::Text(t, _) => s.push_str(t),
            Inl::Soft | Inl::Hard => s.push(' '),
            Inl::Math(t, _) => s.push_str(t),
            Inl::Img { alt, .. } => s.push_str(alt),
            Inl::FootRef(..) => {}
        }
    }
    s.trim().to_string()
}

/// GitHub-style heading slug.
pub fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            o.push(c);
        } else if c == ' ' {
            o.push('-');
        }
    }
    o
}

pub fn parse(src: &str) -> Doc {
    // mddog's math pre-pass: escapes Markdown inside math spans and holds `\$`, so the parser keeps math text.
    let protected = crate::render::markdown::protect_math(src);
    let src = protected.as_str();
    let mut o = Options::empty();
    o.insert(Options::ENABLE_TABLES);
    o.insert(Options::ENABLE_FOOTNOTES);
    o.insert(Options::ENABLE_STRIKETHROUGH);
    o.insert(Options::ENABLE_TASKLISTS);
    o.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    o.insert(Options::ENABLE_GFM);
    let mut b = B {
        stack: vec![frame(Fk::Root)],
        inl: vec![],
        cnt: Counts::default(),
        link: vec![],
        links: vec![],
        code: None,
        table: None,
        foot_order: vec![],
        foots: HashMap::new(),
        skip: None,
        in_summary: false,
        anchors: HashMap::new(),
        details_n: 0,
        in_image: None,
        html_buf: String::new(),
    };
    let mut meta = None;
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line = |off: usize| starts.partition_point(|&s| s <= off) as u32;
    let mut lines: Vec<Option<(u32, u32)>> = vec![];
    let mut cur: Option<(u32, u32)> = None;
    for (ev, range) in Parser::new_ext(src, o).into_offset_iter() {
        let top = b.stack.len() == 1 && b.code.is_none() && b.table.is_none() && matches!(ev, Event::Start(_) | Event::Rule);
        let next = top.then(|| (line(range.start), line(range.end.saturating_sub(1).max(range.start))));
        b.event(ev, &mut meta);
        while b.stack[0].blocks.len() > lines.len() {
            lines.push(cur);
        }
        if next.is_some() {
            cur = next;
        }
    }
    b.flush();
    while b.stack[0].blocks.len() > lines.len() {
        lines.push(cur);
    }
    while b.stack.len() > 1 {
        b.close_frame();
    }
    let mut root = b.pop_frame();
    lines.truncate(root.blocks.len());
    if !b.foot_order.is_empty() {
        let mut v = vec![];
        for (i, l) in b.foot_order.iter().enumerate() {
            if let Some(bl) = b.foots.remove(l) {
                v.push((i + 1, l.clone(), bl));
            }
        }
        if !v.is_empty() {
            root.blocks.push(Block::Footnotes(v));
        }
    }
    while lines.len() < root.blocks.len() {
        lines.push(None);
    }
    display_paras(&mut root.blocks);
    Doc { blocks: root.blocks, links: b.links, lines }
}

/// A paragraph of plain text that is `$$…$$` as a whole is display math, as GitHub has it.
fn display_paras(bs: &mut [Block]) {
    for b in bs.iter_mut() {
        match b {
            Block::Para(v) => {
                let plain = v.iter().all(|i| match i {
                    Inl::Text(_, st) | Inl::Math(_, st) => *st == Sty::default(),
                    Inl::Soft | Inl::Hard => true,
                    _ => false,
                });
                if !plain {
                    continue;
                }
                let s: String = v
                    .iter()
                    .map(|i| match i {
                        Inl::Text(t, _) | Inl::Math(t, _) => t.as_str(),
                        _ => "\n",
                    })
                    .collect();
                let s = s.trim_end();
                if let Some(inner) = s.strip_prefix("$$").and_then(|r| r.strip_suffix("$$"))
                    && !inner.trim().is_empty()
                    && !inner.contains("$$")
                {
                    *b = Block::Math(inner.to_string());
                }
            }
            Block::Quote(c) | Block::Alert(_, c) | Block::Align(_, c) | Block::Details { body: c, .. } => display_paras(c),
            Block::List { items, .. } => items.iter_mut().for_each(|i| display_paras(&mut i.blocks)),
            Block::Footnotes(f) => f.iter_mut().for_each(|f| display_paras(&mut f.2)),
            _ => {}
        }
    }
}

/// Splits text into text and math by mddog's `inline_math` (GitHub's rules). Math keeps its delimiters.
fn split_math(s: &str, st: Sty, out: &mut Vec<Inl>, links: &mut Vec<String>) {
    let mut last = 0;
    for (a, e) in crate::render::markdown::inline_math(s, false).into_iter().filter(|&(a, e)| !s[a..e].contains("://") && !s[a..e].contains("www.")) {
        if a > last {
            autolink(&s[last..a], st, links, out);
        }
        out.push(Inl::Math(s[a..e].to_string(), st));
        last = e;
    }
    if last < s.len() {
        autolink(&s[last..], st, links, out);
    }
}

impl B {
    fn event(&mut self, ev: Event, meta: &mut Option<String>) {
        let b = self;
        if let Some(yaml) = meta {
            match ev {
                Event::Text(t) => yaml.push_str(&t),
                // GitHub shows front matter as a table of its top-level keys.
                Event::End(TagEnd::MetadataBlock(_)) => {
                    let rows: Vec<Vec<Vec<Inl>>> =
                        front_matter_rows(yaml).into_iter().map(|(k, v)| vec![vec![Inl::Text(k, Sty { bold: true, ..Sty::default() })], vec![Inl::Text(v, Sty::default())]]).collect();
                    *meta = None;
                    if !rows.is_empty() {
                        // Its keys are `th` cells, which browsers center.
                        b.push_block(Block::Table { aligns: vec![1, 0], head: vec![], rows });
                    }
                }
                _ => {}
            }
            return;
        }
        match ev {
            Event::Start(t) => match t {
                Tag::MetadataBlock(_) => *meta = Some(String::new()),
                Tag::Paragraph => {
                    b.flush();
                    if matches!(b.stack.last().map(|f| &f.kind), Some(Fk::Item)) {
                        b.top().loose = true;
                    }
                }
                Tag::Heading { level, .. } => {
                    b.flush();
                    b.stack.push(frame(Fk::Heading(heading_level(level), String::new())));
                }
                Tag::BlockQuote(k) => {
                    b.flush();
                    b.stack.push(frame(Fk::Quote(k)));
                }
                Tag::CodeBlock(k) => {
                    b.flush();
                    let lang = match k {
                        CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                        _ => String::new(),
                    };
                    b.code = Some((lang, String::new()));
                }
                Tag::HtmlBlock => {
                    b.flush();
                }
                Tag::List(s) => {
                    b.flush();
                    b.stack.push(frame(Fk::List(s)));
                }
                Tag::Item => {
                    b.flush();
                    b.stack.push(frame(Fk::Item));
                }
                Tag::FootnoteDefinition(l) => {
                    b.flush();
                    b.stack.push(frame(Fk::Foot(l.to_string())));
                }
                Tag::Table(al) => {
                    b.flush();
                    let a = al
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Center => 1,
                            pulldown_cmark::Alignment::Right => 2,
                            _ => 0,
                        })
                        .collect();
                    b.table = Some(TableB { aligns: a, head: vec![], rows: vec![], row: vec![] });
                }
                Tag::TableHead | Tag::TableRow => {}
                Tag::TableCell => b.inl.clear(),
                Tag::Emphasis => b.cnt.italic += 1,
                Tag::Strong => b.cnt.bold += 1,
                Tag::Strikethrough => b.cnt.strike += 1,
                Tag::Superscript => b.cnt.sup += 1,
                Tag::Subscript => b.cnt.sub += 1,
                Tag::Link { dest_url, .. } => {
                    let id = b.add_link(dest_url.to_string());
                    b.link.push(id);
                }
                Tag::Image { dest_url, .. } => {
                    b.in_image = Some((dest_url.to_string(), String::new(), vec![]));
                }
                _ => {}
            },
            Event::End(t) => match t {
                TagEnd::Paragraph => b.flush(),
                TagEnd::Heading(_) => {
                    let base = slug(&plain(&b.inl));
                    let n = b.anchors.entry(base.clone()).or_insert(0);
                    let id = if *n > 0 { format!("{}-{}", base, n) } else { base };
                    *n += 1;
                    if let Some(Frame { kind: Fk::Heading(_, i), .. }) = b.stack.last_mut() {
                        *i = id;
                    }
                    b.close_frame();
                }
                TagEnd::BlockQuote(_) => {
                    b.flush();
                    b.close_html_frames();
                    b.close_frame();
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, mut text)) = b.code.take() {
                        if text.ends_with('\n') {
                            text.pop();
                        }
                        let blk = match lang.as_str() {
                            "math" => Block::Math(text.trim_end_matches('\n').to_string()),
                            "mermaid" => Block::Mermaid(text),
                            _ => Block::Code { lang, text },
                        };
                        b.push_block(blk);
                    }
                }
                TagEnd::HtmlBlock => {
                    let h = std::mem::take(&mut b.html_buf);
                    b.html(&h);
                    b.flush();
                    b.skip = None;
                }
                TagEnd::List(_) => {
                    b.flush();
                    b.close_html_frames();
                    let f = b.pop_frame();
                    let start = if let Fk::List(s) = f.kind { s } else { None };
                    b.push_block(Block::List { start, items: f.items, tight: !f.loose });
                }
                TagEnd::Item => {
                    b.flush();
                    b.close_html_frames();
                    let f = b.pop_frame();
                    let loose = f.loose;
                    let item = LItem { task: f.task, blocks: f.blocks };
                    let l = b.top();
                    l.items.push(item);
                    l.loose |= loose;
                }
                TagEnd::FootnoteDefinition => {
                    b.flush();
                    b.close_html_frames();
                    let f = b.pop_frame();
                    if let Fk::Foot(l) = f.kind {
                        b.foots.insert(l, f.blocks);
                    }
                }
                TagEnd::Table => {
                    if let Some(t) = b.table.take() {
                        b.push_block(Block::Table { aligns: t.aligns, head: t.head, rows: t.rows });
                    }
                }
                TagEnd::TableHead => {
                    if let Some(t) = &mut b.table {
                        t.head = std::mem::take(&mut t.row);
                    }
                }
                TagEnd::TableRow => {
                    if let Some(t) = &mut b.table {
                        let r = std::mem::take(&mut t.row);
                        t.rows.push(r);
                    }
                }
                TagEnd::TableCell => {
                    let v = std::mem::take(&mut b.inl);
                    let v = finish_inlines(v, &mut b.links);
                    if let Some(t) = &mut b.table {
                        t.row.push(v);
                    }
                }
                TagEnd::Emphasis => b.cnt.italic = b.cnt.italic.saturating_sub(1),
                TagEnd::Strong => b.cnt.bold = b.cnt.bold.saturating_sub(1),
                TagEnd::Strikethrough => b.cnt.strike = b.cnt.strike.saturating_sub(1),
                TagEnd::Superscript => b.cnt.sup = b.cnt.sup.saturating_sub(1),
                TagEnd::Subscript => b.cnt.sub = b.cnt.sub.saturating_sub(1),
                TagEnd::Link => {
                    b.link.pop();
                }
                TagEnd::Image => {
                    if let Some((src, alt, _)) = b.in_image.take() {
                        let link = b.link.last().copied().unwrap_or(0);
                        b.inl.push(Inl::Img { src, alt, w: None, link });
                    }
                }
                _ => {}
            },
            Event::Text(s) => {
                if let Some((_, t)) = &mut b.code {
                    t.push_str(&s);
                } else if let Some((_, alt, _)) = &mut b.in_image {
                    alt.push_str(&s);
                } else {
                    b.text(&s);
                }
            }
            Event::Code(s) => {
                if let Some((_, alt, _)) = &mut b.in_image {
                    alt.push_str(&s);
                } else {
                    let mut st = b.sty();
                    st.code = true;
                    b.inl.push(Inl::Text(s.to_string(), st));
                }
            }
            Event::InlineMath(s) => {
                let st = b.sty();
                // $`x`$ is math written with a code span
                let s: std::borrow::Cow<str> = if s.len() >= 2 && s.starts_with('`') && s.ends_with('`') { s[1..s.len() - 1].trim().to_string().into() } else { s.to_string().into() };
                // `$20, 3$` is money, not math: a digit first and a space inside
                if s.starts_with(|c: char| c.is_ascii_digit()) && s.contains(char::is_whitespace) {
                    b.inl.push(Inl::Text(format!("${}$", s), st));
                } else {
                    b.inl.push(Inl::Math(s.to_string(), st));
                }
            }
            Event::DisplayMath(s) => {
                if b.table.is_some() {
                    let st = b.sty();
                    b.inl.push(Inl::Math(s.to_string(), st));
                } else {
                    b.flush();
                    b.push_block(Block::Math(s.to_string()));
                }
            }
            Event::Html(h) => b.html_buf.push_str(&h),
            Event::InlineHtml(h) => b.html(&h),
            Event::FootnoteReference(l) => {
                let l = l.to_string();
                let n = match b.foot_order.iter().position(|x| *x == l) {
                    Some(p) => p + 1,
                    None => {
                        b.foot_order.push(l.clone());
                        b.foot_order.len()
                    }
                };
                b.inl.push(Inl::FootRef(n, l));
            }
            Event::SoftBreak => {
                if b.in_image.is_none() {
                    b.inl.push(Inl::Soft)
                }
            }
            Event::HardBreak => b.inl.push(Inl::Hard),
            Event::Rule => {
                b.flush();
                b.push_block(Block::Rule);
            }
            Event::TaskListMarker(c) => {
                if let Some(f) = b.stack.last_mut() {
                    f.task = Some(c);
                }
            }
            _ => {}
        }
    }
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[allow(dead_code)]
fn _unused(_: LinkType) {}

/// The top-level `key: value` lines of YAML front matter, without its `---` lines, as GitHub's table shows them.
pub fn front_matter_rows(yaml: &str) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for line in yaml.lines().take_while(|l| l.trim_end() != "---") {
        let Some((k, v)) = line.split_once(':') else { continue };
        if k.starts_with([' ', '\t', '-', '#']) || k.trim().is_empty() {
            continue;
        }
        let v = v.trim();
        let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))).unwrap_or(v);
        rows.push((k.trim().to_string(), v.to_string()));
    }
    rows
}
