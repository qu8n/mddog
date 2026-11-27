//! Markdown to a block tree. Inline content is flat runs with style flags.
use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

mod dollars;

/// Which app's rules a `> [!kind]` block follows.
#[derive(serde::Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Callouts {
    /// github.com's alerts: five kinds, the marker alone on the first line, only at the top level of the page.
    #[default]
    Github,
    /// Obsidian's callouts: any kind, aliases, a title, a fold marker, and nesting.
    Obsidian,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Marks {
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
pub enum Inline {
    Text(String, Marks),
    Soft,
    Hard,
    Img { src: String, alt: String, w: Option<f32>, link: u32 },
    Math(String, Marks),
    FootRef(usize, String),
    Ruby { base: Vec<Inline>, rt: Vec<Inline> },
}

/// An alert's or callout's kind. GitHub's five come first, and their place indexes the theme's alert colors.
/// The rest are Obsidian's callout kinds, read only with `callouts = "obsidian"`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
    Abstract,
    Info,
    Todo,
    Success,
    Question,
    Failure,
    Danger,
    Bug,
    Example,
    Quote,
}

impl Alert {
    /// GitHub's kinds, which have theme tokens.
    pub const GITHUB: [Alert; 5] = [Alert::Note, Alert::Tip, Alert::Important, Alert::Warning, Alert::Caution];

    /// The kind an Obsidian callout word selects, from Obsidian's `app.css`. `important` and `caution` are
    /// tip and warning there, and a word Obsidian doesn't know draws as a note.
    pub fn obsidian(word: &str) -> Alert {
        match word {
            "abstract" | "summary" | "tldr" => Alert::Abstract,
            "info" => Alert::Info,
            "todo" => Alert::Todo,
            "tip" | "hint" | "important" => Alert::Tip,
            "success" | "check" | "done" => Alert::Success,
            "question" | "help" | "faq" => Alert::Question,
            "warning" | "caution" | "attention" => Alert::Warning,
            "failure" | "fail" | "missing" => Alert::Failure,
            "danger" | "error" => Alert::Danger,
            "bug" => Alert::Bug,
            "example" => Alert::Example,
            "quote" | "cite" => Alert::Quote,
            _ => Alert::Note,
        }
    }

    /// The title GitHub draws, which is also the theme token's name in lower case.
    pub fn title(self) -> &'static str {
        match self {
            Alert::Note => "Note",
            Alert::Tip => "Tip",
            Alert::Important => "Important",
            Alert::Warning => "Warning",
            Alert::Caution => "Caution",
            Alert::Abstract => "Abstract",
            Alert::Info => "Info",
            Alert::Todo => "Todo",
            Alert::Success => "Success",
            Alert::Question => "Question",
            Alert::Failure => "Failure",
            Alert::Danger => "Danger",
            Alert::Bug => "Bug",
            Alert::Example => "Example",
            Alert::Quote => "Quote",
        }
    }
}

/// How a block's lines sit across its width: from a table column, or the HTML `align` attribute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
pub struct ListItem {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Para(Vec<Inline>),
    /// Text that sits in a raw HTML block outside any `<p>`. HTML draws it as a line box with no margin, so no gap follows it.
    Text(Vec<Inline>),
    Heading(u8, Vec<Inline>, String),
    Code {
        lang: String,
        text: String,
    },
    Quote(Vec<Block>),
    /// A GitHub alert, or an Obsidian callout.
    Alert {
        kind: Alert,
        /// The callout's title. `None` for a GitHub alert, which draws its kind's name.
        title: Option<Vec<Inline>>,
        /// A callout with a fold marker: whether it starts open, and its toggle id, shared with `<details>`.
        fold: Option<(bool, usize)>,
        body: Vec<Block>,
    },
    List {
        start: Option<u64>,
        items: Vec<ListItem>,
        tight: bool,
    },
    Table {
        aligns: Vec<Option<Align>>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
    Math(String),
    Mermaid(String),
    Details {
        summary: Vec<Inline>,
        open: bool,
        body: Vec<Block>,
        id: usize,
    },
    Align(Align, Vec<Block>),
    Footnotes(Vec<(usize, String, Vec<Block>)>),
}

/// Calls `f` on each block the first layout draws, a parent before its children. A closed `<details>` lays
/// out its body only once opened, so the body is left out.
pub fn walk_shown<'a>(blocks: &'a [Block], f: &mut impl FnMut(&'a Block)) {
    for b in blocks {
        f(b);
        match b {
            Block::Quote(bs) | Block::Alert { body: bs, fold: None | Some((true, _)), .. } | Block::Align(_, bs) | Block::Details { body: bs, open: true, .. } => walk_shown(bs, f),
            Block::List { items, .. } => items.iter().for_each(|i| walk_shown(&i.blocks, f)),
            Block::Footnotes(notes) => notes.iter().for_each(|n| walk_shown(&n.2, f)),
            _ => {}
        }
    }
}

pub struct Doc {
    pub blocks: Vec<Block>,
    pub links: Vec<String>,
    /// The 1-based source lines of each top-level block, where it has them.
    pub lines: Vec<Option<(u32, u32)>>,
}

enum FrameKind {
    Root,
    Quote(Option<BlockQuoteKind>),
    Item,
    List(Option<u64>),
    Details { open: bool, id: usize },
    Align(Option<Align>, String),
    Heading(u8, String),
    Foot(String),
}

struct Frame {
    kind: FrameKind,
    blocks: Vec<Block>,
    summary: Vec<Inline>,
    task: Option<bool>,
    loose: bool,
    items: Vec<ListItem>,
    /// An alert's marker, kept in case the alert turns back into a quote.
    marker: Option<Marker>,
    /// A quote's first source line after `>` and one space, which Obsidian matches its callout marker against.
    head: Option<String>,
}

fn frame(kind: FrameKind) -> Frame {
    Frame { kind, blocks: vec![], summary: vec![], task: None, loose: false, items: vec![], marker: None, head: None }
}

/// An alert's `[!KIND]` marker as the source has it.
struct Marker {
    text: String,
    /// The marker line ends in two or more spaces, so a hard break follows it.
    hard: bool,
    /// The first paragraph starts on the next line, so the marker goes in front of its text.
    joins: bool,
    /// Something follows the marker. It counts even when it draws nothing, like `<span></span>`, except HTML
    /// that is only comments, which GitHub removes before it looks.
    body: bool,
}

impl Marker {
    /// Reads the marker from an alert's source range. Returns the marker and the offset just past it.
    fn read(src: &str) -> Option<(Marker, usize)> {
        let start = src.find("[!")?;
        let end = start + src[start..].find(']')? + 1;
        let tail = src[end..].split(['\n', '\r']).next().unwrap_or("");
        Some((Marker { text: src[start..end].to_string(), hard: tail.ends_with("  "), joins: false, body: false }, end))
    }
}

/// Line breaks in `s`, where `\r\n`, `\n` and a lone `\r` each count as one.
fn line_breaks(s: &str) -> usize {
    s.matches('\n').count() + s.matches('\r').count() - s.matches("\r\n").count()
}

/// The HTML is only comments and white space.
fn only_comments(h: &str) -> bool {
    let mut rest = h;
    while let Some(i) = rest.find("<!--") {
        if !rest[..i].trim().is_empty() {
            return false;
        }
        let Some(e) = rest[i..].find("-->") else { return true };
        rest = &rest[i + e + 3..];
    }
    rest.trim().is_empty()
}

/// GitHub draws an alert only at the top level and with a body. Anywhere else it is a quote that starts with
/// the literal marker.
fn demote(kind: Alert, marker: Option<Marker>, mut blocks: Vec<Block>) -> Block {
    let m = marker.unwrap_or_else(|| Marker { text: format!("[!{}]", kind.title().to_uppercase()), hard: false, joins: false, body: false });
    let text = Inline::Text(m.text, Marks::default());
    match blocks.first_mut() {
        Some(Block::Para(v)) if m.joins => {
            v.splice(0..0, [text, if m.hard { Inline::Hard } else { Inline::Soft }]);
        }
        _ => blocks.insert(0, Block::Para(vec![text])),
    }
    Block::Quote(blocks)
}

struct TableBuilder {
    aligns: Vec<Option<Align>>,
    head: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    row: Vec<Vec<Inline>>,
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

struct Builder {
    stack: Vec<Frame>,
    inl: Vec<Inline>,
    cnt: Counts,
    link: Vec<u32>,
    /// The link depth a paragraph started at: GitHub closes an `<a>` left open inside it at its end.
    para_links: usize,
    links: Vec<String>,
    code: Option<(String, String)>,
    table: Option<TableBuilder>,
    foot_order: Vec<String>,
    foots: HashMap<String, Vec<Block>>,
    skip: Option<String>,
    in_summary: bool,
    anchors: HashMap<String, usize>,
    details_n: usize,
    callouts: Callouts,
    in_image: Option<(String, String, Vec<Inline>)>,
    html_buf: String,
    /// Set while a raw HTML block is read, so its loose text becomes `Block::Text`.
    raw: bool,
    ruby: Vec<RubyAt>,
}

struct RubyAt {
    base: usize,
    rt: Option<usize>,
    rp: Option<usize>,
}

impl Builder {
    fn sty(&self) -> Marks {
        let c = &self.cnt;
        Marks {
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
        self.inl.push(Inline::Text(s.to_string(), st));
    }

    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("the root frame is never popped")
    }

    fn push_block(&mut self, b: Block) {
        self.top().blocks.push(b);
    }

    /// Turn pending inline content into a paragraph (tight list items and html text).
    fn flush(&mut self) {
        if self.in_summary {
            return;
        }
        self.ruby.clear();
        let v = std::mem::take(&mut self.inl);
        let v = finish_inlines(v, &mut self.links);
        if v.iter().any(|i| !matches!(i, Inline::Text(t, _) if t.trim().is_empty()) && !matches!(i, Inline::Soft)) {
            let in_p = matches!(self.stack.last().map(|f| &f.kind), Some(FrameKind::Align(_, n)) if n == "p");
            self.push_block(if self.raw && !in_p { Block::Text(v) } else { Block::Para(v) });
        }
    }

    fn pop_frame(&mut self) -> Frame {
        self.stack.pop().expect("every frame pops after its push")
    }

    fn end_html_container(&mut self, name: &str) {
        let pos = self.stack.iter().rposition(|f| match &f.kind {
            FrameKind::Align(_, n) => *n == name,
            FrameKind::Details { .. } => name == "details",
            FrameKind::Heading(..) => name.len() == 2 && name.starts_with('h'),
            _ => false,
        });
        let Some(pos) = pos else { return };
        if !matches!(self.stack[pos].kind, FrameKind::Heading(..)) {
            self.flush();
        }
        while self.stack.len() > pos + 1 {
            self.close_frame();
        }
        self.close_frame();
        if matches!(self.stack.last().map(|f| &f.kind), Some(FrameKind::Align(_, n)) if is_heading(n) && is_heading(name)) {
            self.close_frame();
        }
    }

    /// Close html frames (details, align) left open inside a markdown container.
    fn close_html_frames(&mut self) {
        while self.stack.len() > 1 && matches!(self.stack.last().map(|f| &f.kind), Some(FrameKind::Align(..) | FrameKind::Details { .. })) {
            self.flush();
            self.close_frame();
        }
    }

    fn end_summary(&mut self) {
        self.in_summary = false;
        let v = std::mem::take(&mut self.inl);
        let v = finish_inlines(v, &mut self.links);
        if let Some(f) = self.stack.iter_mut().rev().find(|f| matches!(f.kind, FrameKind::Details { .. })) {
            f.summary = v;
        }
    }

    fn close_frame(&mut self) {
        // A <summary> its <details> outlived: keep its text, or flush() drops everything after it.
        if self.in_summary && matches!(self.top().kind, FrameKind::Details { .. }) {
            self.end_summary();
        }
        let f = self.pop_frame();
        let blocks = f.blocks;
        let b = match f.kind {
            FrameKind::Align(a, _) => {
                let inherited = self.stack.iter().rev().find_map(|f| if let FrameKind::Align(Some(a), _) = f.kind { Some(a) } else { None }).unwrap_or_default();
                let Some(a) = a.filter(|&a| a != inherited) else {
                    for b in blocks {
                        self.push_block(b);
                    }
                    return;
                };
                Block::Align(a, blocks)
            }
            FrameKind::Details { open, id } => Block::Details { summary: f.summary, open, body: blocks, id },
            FrameKind::Quote(k) => match (k, self.callouts) {
                (_, Callouts::Obsidian) => self.callout(f.head.as_deref(), blocks).unwrap_or_else(Block::Quote),
                (None, Callouts::Github) => Block::Quote(blocks),
                (Some(k), Callouts::Github) => {
                    let k = match k {
                        BlockQuoteKind::Note => Alert::Note,
                        BlockQuoteKind::Tip => Alert::Tip,
                        BlockQuoteKind::Important => Alert::Important,
                        BlockQuoteKind::Warning => Alert::Warning,
                        BlockQuoteKind::Caution => Alert::Caution,
                    };
                    // The popped frame's parent is now on top.
                    let body = f.marker.as_ref().map_or(!blocks.is_empty(), |m| m.body);
                    if matches!(self.top().kind, FrameKind::Root) && body { Block::Alert { kind: k, title: None, fold: None, body: blocks } } else { demote(k, f.marker, blocks) }
                }
            },
            FrameKind::Heading(l, id) => {
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
        if matches!(self.inl.last(), Some(Inline::Text(..))) {
            self.text(&HTML_BREAK.to_string());
        }
        for t in tokenize(&tagfilter(h)) {
            match t {
                HtmlToken::Text(s) => {
                    if self.skip.is_some() {
                        continue;
                    }
                    let s = collapse_ws(&s);
                    if self.in_summary || !s.trim().is_empty() || !self.inl.is_empty() {
                        self.text(&s);
                    }
                }
                HtmlToken::Open(name, attrs) => self.open_tag(&name, &attrs),
                HtmlToken::Close(name) => self.close_tag(&name),
            }
        }
    }

    fn open_tag(&mut self, name: &str, attrs: &[(String, String)]) {
        if self.skip.is_some() {
            return;
        }
        let attr = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        match name {
            "object" | "embed" | "template" => {
                self.skip = Some(name.to_string());
            }
            "details" => {
                self.flush();
                self.details_n += 1;
                let id = self.details_n;
                self.stack.push(frame(FrameKind::Details { open: attr("open").is_some(), id }));
            }
            "summary" => {
                self.flush();
                self.in_summary = true;
                self.inl.clear();
            }
            "div" | "p" | "center" | "section" | "article" | "header" | "footer" | "main" | "nav" | "figure" | "figcaption" | "picture" => {
                self.flush();
                self.stack.push(frame(FrameKind::Align(html_align(name, attr("align").as_deref()), name.to_string())));
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                if let Some(a) = html_align(name, attr("align").as_deref()) {
                    self.stack.push(frame(FrameKind::Align(Some(a), name.to_string())));
                }
                let l = name[1..].parse().unwrap_or(1);
                self.stack.push(frame(FrameKind::Heading(l, String::new())));
            }
            "br" => self.inl.push(Inline::Hard),
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
                    self.inl.push(Inline::Img { src, alt, w, link });
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
            "ruby" => self.ruby.push(RubyAt { base: self.inl.len(), rt: None, rp: None }),
            "rt" | "rp" if !self.ruby.is_empty() => {
                self.end_rp();
                self.end_rt();
                let at = Some(self.inl.len());
                if let Some(r) = self.ruby.last_mut() {
                    if name == "rt" { r.rt = at } else { r.rp = at }
                }
            }
            "tr" | "li" | "ul" | "ol" | "table" | "blockquote" => {
                if !self.inl.is_empty() {
                    self.inl.push(Inline::Hard);
                }
            }
            "td" | "th" if !self.inl.is_empty() => {
                self.text("  ");
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
            "summary" => self.end_summary(),
            "details" | "div" | "p" | "center" | "section" | "article" | "header" | "footer" | "main" | "nav" | "figure" | "figcaption" | "picture" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.end_html_container(name);
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
            "rt" => self.end_rt(),
            "rp" => self.end_rp(),
            "ruby" => {
                self.end_rp();
                self.end_rt();
                self.ruby.pop();
            }
            _ => {}
        }
    }

    fn end_rp(&mut self) {
        if let Some(at) = self.ruby.last_mut().and_then(|r| r.rp.take()) {
            self.inl.truncate(at.min(self.inl.len()));
        }
    }

    fn end_rt(&mut self) {
        let len = self.inl.len();
        let Some(top) = self.ruby.last_mut() else { return };
        let Some(r) = top.rt.take() else { return };
        let b = top.base.min(len);
        let rt = self.inl.split_off(r.clamp(b, len));
        let base = self.inl.split_off(b);
        let (bv, rv) = (ruby_part(base.clone(), &mut self.links), ruby_part(rt, &mut self.links));
        if rv.is_empty() {
            self.inl.extend(base);
        } else if !bv.is_empty() {
            self.inl.push(Inline::Ruby { base: bv, rt: rv });
        }
        let len = self.inl.len();
        if let Some(top) = self.ruby.last_mut() {
            top.base = len;
        }
    }

    fn add_link(&mut self, url: String) -> u32 {
        self.links.push(url);
        self.links.len() as u32
    }
}

fn html_align(name: &str, attr: Option<&str>) -> Option<Align> {
    match (name, attr.map(str::to_ascii_lowercase).as_deref()) {
        ("center", _) | (_, Some("center")) => Some(Align::Center),
        (_, Some("right")) => Some(Align::Right),
        (_, Some("left")) => Some(Align::Left),
        _ => None,
    }
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn ruby_part(v: Vec<Inline>, links: &mut Vec<String>) -> Vec<Inline> {
    let mut st = Marks::default();
    let v: Vec<Inline> = v
        .into_iter()
        .map(|i| match i {
            Inline::Soft | Inline::Hard => Inline::Text(" ".into(), st),
            Inline::Text(_, s) => {
                st = s;
                i
            }
            _ => i,
        })
        .collect();
    let mut v = finish_inlines(v, links);
    if let Some(Inline::Text(t, _)) = v.first_mut() {
        *t = t.trim_start().to_string();
    }
    if let Some(Inline::Text(t, _)) = v.last_mut() {
        *t = t.trim_end().to_string();
    }
    v.retain(|i| !matches!(i, Inline::Text(t, _) if t.is_empty()));
    v
}

/// Marks where inline HTML sat in merged text. Math never spans it, and it is dropped before layout.
const HTML_BREAK: char = '\u{e025}';

pub fn finish_inlines(v: Vec<Inline>, links: &mut Vec<String>) -> Vec<Inline> {
    let mut m: Vec<Inline> = Vec::with_capacity(v.len());
    for i in v {
        if let Inline::Text(s, st) = &i
            && let Some(Inline::Text(ps, pst)) = m.last_mut()
            && pst == st
        {
            ps.push_str(s);
            continue;
        }
        m.push(i);
    }
    // `$`x`$`: math written with a code span.
    let mut i = 0;
    while i + 2 < m.len() {
        let spanned = matches!((&m[i], &m[i + 1], &m[i + 2]), (Inline::Text(a, sa), Inline::Text(_, c), Inline::Text(z, _)) if !sa.code && c.code && a.ends_with('$') && z.starts_with('$'));
        if spanned {
            let Inline::Text(code, cst) = m[i + 1].clone() else { unreachable!() };
            if let Inline::Text(a, _) = &mut m[i] {
                a.pop();
            }
            if let Inline::Text(z, _) = &mut m[i + 2] {
                z.remove(0);
            }
            m[i + 1] = Inline::Math(format!("${code}$"), Marks { code: false, ..cst });
        }
        i += 1;
    }
    let held = dollars::HELD_DOLLAR;
    let mut out = Vec::with_capacity(m.len());
    for i in m {
        match i {
            Inline::Text(s, st) if st.link == 0 && !st.code => {
                let n = out.len();
                for piece in s.split(HTML_BREAK).filter(|p| !p.is_empty()) {
                    split_math(piece, st, &mut out, links);
                }
                for o in &mut out[n..] {
                    if let Inline::Text(t, _) = o
                        && t.contains(held)
                    {
                        *t = t.replace(held, "$");
                    }
                }
            }
            Inline::Text(s, st) if s.contains(held) || s.contains(HTML_BREAK) => {
                let s = s.replace(HTML_BREAK, "").replace(held, if st.code { "\\$" } else { "$" });
                if !s.is_empty() {
                    out.push(Inline::Text(s, st));
                }
            }
            o => out.push(o),
        }
    }
    out
}

fn autolink(s: &str, st: Marks, links: &mut Vec<String>, out: &mut Vec<Inline>) {
    let b = s.as_bytes();
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        let rest = &s[i..];
        let at_word = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'/' || b[i - 1] == b'@');
        let mut end = None;
        let mut url = String::new();
        if at_word && (rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("www.")) {
            // A double quote or `>` ends it too, so `src="https://example.com">` from a filtered tag links only
            // the URL (a trailing single quote is trimmed below).
            let mut e = rest.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(rest.len());
            e = trim_url(&rest[..e]);
            let min = if rest.starts_with("www.") { 5 } else { 9 };
            if e >= min {
                end = Some(i + e);
                url = if rest.starts_with("www.") { format!("http://{}", &rest[..e]) } else { rest[..e].to_string() };
            }
        } else if b[i] == b'@' && i > 0 {
            let mut st_i = i;
            while st_i > last && (b[st_i - 1].is_ascii_alphanumeric() || b"._+-".contains(&b[st_i - 1])) {
                st_i -= 1;
            }
            let dom_len = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')).unwrap_or(rest.len() - 1);
            let dom = rest[1..1 + dom_len].trim_end_matches(['.', '-', '_']);
            if st_i < i && dom.contains('.') && !dom.starts_with('.') {
                let e = i + 1 + dom.len();
                if st_i > last {
                    out.push(Inline::Text(s[last..st_i].to_string(), st));
                }
                let id = {
                    links.push(format!("mailto:{}", &s[st_i..e]));
                    links.len() as u32
                };
                out.push(Inline::Text(s[st_i..e].to_string(), Marks { link: id, ..st }));
                last = e;
                i = e;
                continue;
            }
        }
        if let Some(e) = end {
            if i > last {
                out.push(Inline::Text(s[last..i].to_string(), st));
            }
            links.push(url);
            let id = links.len() as u32;
            out.push(Inline::Text(s[i..e].to_string(), Marks { link: id, ..st }));
            last = e;
            i = e;
        } else {
            i += s[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        }
    }
    if last < s.len() {
        out.push(Inline::Text(s[last..].to_string(), st));
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

/// GFM's tagfilter: escapes the `<` of these nine tags, so they show as literal text, as on GitHub.
fn tagfilter(h: &str) -> std::borrow::Cow<'_, str> {
    const TAGS: [&str; 9] = ["title", "textarea", "style", "xmp", "iframe", "noembed", "noframes", "script", "plaintext"];
    let hit = |rest: &str| {
        let r = rest.strip_prefix('/').unwrap_or(rest);
        TAGS.iter().any(|t| r.get(..t.len()).is_some_and(|n| n.eq_ignore_ascii_case(t)) && r[t.len()..].chars().next().is_none_or(|c| c.is_whitespace() || c == '>' || c == '/'))
    };
    if !h.match_indices('<').any(|(i, _)| hit(&h[i + 1..])) {
        return h.into();
    }
    let mut out = String::with_capacity(h.len() + 16);
    for (i, c) in h.char_indices() {
        if c == '<' && hit(&h[i + 1..]) {
            out.push_str("&lt;");
        } else {
            out.push(c);
        }
    }
    out.into()
}

enum HtmlToken {
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

fn tokenize(h: &str) -> Vec<HtmlToken> {
    let mut out = vec![];
    let mut rest = h;
    while !rest.is_empty() {
        if let Some(p) = rest.find('<') {
            if p > 0 {
                out.push(HtmlToken::Text(decode_entities(&rest[..p])));
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
            let Some(e) = rest.find('>').filter(|_| nc.is_ascii_alphabetic() || nc == '/') else {
                out.push(HtmlToken::Text("<".to_string()));
                rest = &rest[1..];
                continue;
            };
            let inner = &rest[1..e];
            rest = &rest[e + 1..];
            if let Some(name) = inner.strip_prefix('/') {
                out.push(HtmlToken::Close(name.trim().to_ascii_lowercase()));
            } else {
                let inner = inner.trim_end_matches('/');
                let (name, attrs) = match inner.find(char::is_whitespace) {
                    Some(sp) => (&inner[..sp], &inner[sp..]),
                    None => (inner, ""),
                };
                out.push(HtmlToken::Open(name.to_ascii_lowercase(), parse_attrs(attrs)));
            }
        } else {
            out.push(HtmlToken::Text(decode_entities(rest)));
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

pub fn plain(v: &[Inline]) -> String {
    let mut s = String::new();
    for i in v {
        match i {
            Inline::Text(t, _) => s.push_str(t),
            Inline::Soft | Inline::Hard => s.push(' '),
            Inline::Math(t, _) => s.push_str(t),
            Inline::Img { alt, .. } => s.push_str(alt),
            Inline::Ruby { base, .. } => s.push_str(&plain(base)),
            Inline::FootRef(..) => {}
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

/// Parses `src` with GitHub's callouts.
pub fn parse(src: &str) -> Doc {
    parse_with(src, Callouts::default())
}

pub fn parse_with(src: &str, callouts: Callouts) -> Doc {
    // mddog's math pre-pass: escapes Markdown inside math spans and holds `\$`, so the parser keeps math text.
    let protected = dollars::protect_math(src);
    let src = protected.as_str();
    let mut o = Options::empty();
    o.insert(Options::ENABLE_TABLES);
    o.insert(Options::ENABLE_FOOTNOTES);
    o.insert(Options::ENABLE_STRIKETHROUGH);
    o.insert(Options::ENABLE_TASKLISTS);
    o.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    // GitHub's alerts. Obsidian's callouts are read from the quote's text instead, in `Builder::callout`.
    if callouts == Callouts::Github {
        o.insert(Options::ENABLE_GFM);
    }
    // Not ENABLE_MATH: `protect_math` finds math by GitHub's rules, and `split_math` reads it from the text.
    let mut b = Builder {
        stack: vec![frame(FrameKind::Root)],
        inl: vec![],
        cnt: Counts::default(),
        link: vec![],
        para_links: 0,
        links: vec![],
        code: None,
        table: None,
        foot_order: vec![],
        foots: HashMap::new(),
        skip: None,
        in_summary: false,
        anchors: HashMap::new(),
        details_n: 0,
        callouts,
        in_image: None,
        html_buf: String::new(),
        raw: false,
        ruby: vec![],
    };
    let mut meta = None;
    let starts: Vec<usize> = std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line = |off: usize| starts.partition_point(|&s| s <= off) as u32;
    let mut lines: Vec<Option<(u32, u32)>> = vec![];
    let mut cur: Option<(u32, u32)> = None;
    // The frame index of an alert whose first child has not started yet, and the offset past its marker.
    let mut alert: Option<(usize, usize)> = None;
    for (ev, range) in Parser::new_ext(src, o).into_offset_iter() {
        let top = b.stack.len() == 1 && b.code.is_none() && b.table.is_none() && matches!(ev, Event::Start(_) | Event::Rule);
        let next = top.then(|| (line(range.start), line(range.end.saturating_sub(1).max(range.start))));
        if let Some((i, end)) = alert.take()
            && matches!(ev, Event::Start(Tag::Paragraph))
            && line_breaks(&src[end..range.start]) == 1
            && let Some(m) = &mut b.stack[i].marker
        {
            m.joins = true;
        }
        let body = match &ev {
            Event::Start(Tag::HtmlBlock) => false,
            Event::Start(_) | Event::Rule => true,
            Event::Html(h) => !only_comments(h),
            _ => false,
        };
        if body {
            b.stack.iter_mut().filter_map(|f| f.marker.as_mut()).for_each(|m| m.body = true);
        }
        let opens_alert = matches!(ev, Event::Start(Tag::BlockQuote(Some(_))));
        let opens_quote = callouts == Callouts::Obsidian && matches!(ev, Event::Start(Tag::BlockQuote(_)));
        b.event(ev, &mut meta);
        if opens_quote {
            let i = b.stack.len() - 1;
            b.stack[i].head = Some(quote_head(&src[range.start..]).to_string());
        }
        if opens_alert && let Some((m, end)) = Marker::read(&src[range.start..range.end]) {
            let i = b.stack.len() - 1;
            b.stack[i].marker = Some(m);
            alert = Some((i, range.start + end));
        }
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

impl Builder {
    /// Reads a quote as an Obsidian callout, or hands its blocks back. Obsidian's rule, from its `app.js`: the
    /// quote's first source line, `head`, starts `[!kind]`, an optional `+` or `-` follows, then a space or the
    /// line's end. It matches the raw line, so an escaped or entity-encoded marker stays text and Markdown in
    /// the kind is part of it. The rest of that line is the title.
    fn callout(&mut self, head: Option<&str>, mut blocks: Vec<Block>) -> Result<Block, Vec<Block>> {
        let Some((word, fold, _)) = head.and_then(|h| callout_marker(h, true)) else { return Err(blocks) };
        let Some((i, end)) = blocks.first().and_then(|b| if let Block::Para(v) = b { parsed_marker_end(v, fold.is_some()) } else { None }) else { return Err(blocks) };
        let Block::Para(mut inl) = blocks.remove(0) else { unreachable!() };
        let Inline::Text(t, m) = &inl[i] else { unreachable!() };
        let rest = Inline::Text(t[end..].to_string(), *m);
        inl.splice(..=i, [rest]);
        let cut = inl.iter().position(|i| matches!(i, Inline::Soft | Inline::Hard)).unwrap_or(inl.len());
        let after: Vec<Inline> = inl.drain(cut..).skip(1).collect();
        let mut title = trim_inlines(inl);
        if title.is_empty() {
            // Obsidian's default title: the kind, dashes as spaces, first letter capitalized.
            let t = word.replace('-', " ");
            let mut c = t.chars();
            let t = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
            title = vec![Inline::Text(t, Marks::default())];
        }
        if !after.is_empty() {
            blocks.insert(0, Block::Para(after));
        }
        let fold = fold.map(|open| {
            self.details_n += 1;
            (open, self.details_n)
        });
        Ok(Block::Alert { kind: Alert::obsidian(&word), title: Some(title), fold, body: blocks })
    }
}

/// A quote's first line from its `>`, as Obsidian reads it: the `>`, then at most one space, are dropped.
fn quote_head(from: &str) -> &str {
    let line = from.split(['\n', '\r']).next().unwrap_or_default();
    let line = line.trim_start_matches([' ', '\t']);
    let line = line.strip_prefix('>').unwrap_or(line);
    line.strip_prefix(' ').unwrap_or(line)
}

/// Where the marker ends in a callout's parsed first paragraph: the text run holding the first `]` after `[!`,
/// and the byte past it and any fold sign. Markdown in the kind splits the marker over several runs.
fn parsed_marker_end(v: &[Inline], fold: bool) -> Option<(usize, usize)> {
    // A reference definition of `[!kind]` makes the marker a link whose text is `!kind`.
    if let Some(Inline::Text(t, m)) = v.first()
        && m.link != 0
        && t.starts_with('!')
    {
        let last = v.iter().take_while(|x| matches!(x, Inline::Text(_, n) if n.link == m.link)).count() - 1;
        let Inline::Text(t, _) = &v[last] else { unreachable!() };
        return match v.get(last + 1) {
            Some(Inline::Text(n, _)) if fold && n.starts_with(['+', '-']) => Some((last + 1, 1)),
            _ => Some((last, t.len())),
        };
    }
    if !matches!(v.first(), Some(Inline::Text(t, _)) if t.starts_with("[!")) {
        return None;
    }
    for (i, x) in v.iter().enumerate() {
        let Inline::Text(t, _) = x else { return None };
        let from = if i == 0 { 2 } else { 0 };
        if let Some(j) = t[from..].find(']') {
            let mut end = from + j + 1;
            if fold && t[end..].starts_with(['+', '-']) {
                end += 1;
            }
            return Some((i, end));
        }
    }
    None
}

/// Splits a callout's first line into its kind word, fold state and the text after the marker.
/// `next_breaks`: the text ends the line, so the marker may end it.
fn callout_marker(t: &str, next_breaks: bool) -> Option<(String, Option<bool>, &str)> {
    let inner = t.strip_prefix("[!")?;
    let close = inner.find(']')?;
    let (raw, mut rest) = (&inner[..close], &inner[close + 1..]);
    if raw.is_empty() {
        return None;
    }
    let mut fold = None;
    if let Some(r) = rest.strip_prefix('+') {
        (fold, rest) = (Some(true), r);
    } else if let Some(r) = rest.strip_prefix('-') {
        (fold, rest) = (Some(false), r);
    }
    if !(rest.starts_with(char::is_whitespace) || rest.is_empty() && next_breaks) {
        return None;
    }
    // The kind is the text before `|`, trimmed and lowercased, with runs of whitespace as one dash.
    let kind = raw.split('|').next().unwrap_or_default();
    let word = kind.split_whitespace().collect::<Vec<_>>().join("-").to_lowercase();
    Some((word, fold, rest))
}

/// Drops the whitespace at both ends of a run of inlines, and the runs left empty.
fn trim_inlines(mut v: Vec<Inline>) -> Vec<Inline> {
    if let Some(Inline::Text(t, _)) = v.first_mut() {
        *t = t.trim_start().to_string();
    }
    if let Some(Inline::Text(t, _)) = v.last_mut() {
        *t = t.trim_end().to_string();
    }
    v.retain(|i| !matches!(i, Inline::Text(t, _) if t.is_empty()));
    v
}

/// A paragraph of plain text that is `$$…$$` as a whole is display math, as GitHub has it.
fn display_paras(bs: &mut [Block]) {
    for b in bs.iter_mut() {
        match b {
            Block::Para(v) => {
                let plain = v.iter().all(|i| match i {
                    Inline::Text(_, st) | Inline::Math(_, st) => *st == Marks::default(),
                    Inline::Soft | Inline::Hard => true,
                    _ => false,
                });
                if !plain {
                    continue;
                }
                let s: String = v
                    .iter()
                    .map(|i| match i {
                        Inline::Text(t, _) | Inline::Math(t, _) => t.as_str(),
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
            Block::Quote(c) | Block::Alert { body: c, .. } | Block::Align(_, c) | Block::Details { body: c, .. } => display_paras(c),
            Block::List { items, .. } => items.iter_mut().for_each(|i| display_paras(&mut i.blocks)),
            Block::Footnotes(f) => f.iter_mut().for_each(|f| display_paras(&mut f.2)),
            _ => {}
        }
    }
}

/// Splits text into text and math by mddog's `inline_math` (GitHub's rules). Math keeps its delimiters.
fn split_math(s: &str, st: Marks, out: &mut Vec<Inline>, links: &mut Vec<String>) {
    let mut last = 0;
    for (a, e) in dollars::inline_math(s, false).into_iter().filter(|&(a, e)| !s[a..e].contains("://") && !s[a..e].contains("www.")) {
        if a > last {
            autolink(&s[last..a], st, links, out);
        }
        out.push(Inline::Math(s[a..e].to_string(), st));
        last = e;
    }
    if last < s.len() {
        autolink(&s[last..], st, links, out);
    }
}

impl Builder {
    fn event(&mut self, ev: Event, meta: &mut Option<String>) {
        let b = self;
        if let Some(yaml) = meta {
            match ev {
                Event::Text(t) => yaml.push_str(&t),
                // GitHub shows front matter as a table of its top-level keys.
                Event::End(TagEnd::MetadataBlock(_)) => {
                    let rows: Vec<Vec<Vec<Inline>>> =
                        front_matter_rows(yaml).into_iter().map(|(k, v)| vec![vec![Inline::Text(k, Marks { bold: true, ..Marks::default() })], vec![Inline::Text(v, Marks::default())]]).collect();
                    *meta = None;
                    if !rows.is_empty() {
                        // Its keys are `th` cells, which browsers center.
                        b.push_block(Block::Table { aligns: vec![Some(Align::Center), Some(Align::Left)], head: vec![], rows });
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
                    b.para_links = b.link.len();
                    if matches!(b.stack.last().map(|f| &f.kind), Some(FrameKind::Item)) {
                        b.top().loose = true;
                    }
                }
                Tag::Heading { level, .. } => {
                    b.flush();
                    b.stack.push(frame(FrameKind::Heading(heading_level(level), String::new())));
                }
                Tag::BlockQuote(k) => {
                    b.flush();
                    b.stack.push(frame(FrameKind::Quote(k)));
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
                    b.stack.push(frame(FrameKind::List(s)));
                }
                Tag::Item => {
                    b.flush();
                    b.stack.push(frame(FrameKind::Item));
                }
                Tag::FootnoteDefinition(l) => {
                    b.flush();
                    b.stack.push(frame(FrameKind::Foot(l.to_string())));
                }
                Tag::Table(al) => {
                    b.flush();
                    let a = al
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Center => Some(Align::Center),
                            pulldown_cmark::Alignment::Right => Some(Align::Right),
                            pulldown_cmark::Alignment::Left => Some(Align::Left),
                            pulldown_cmark::Alignment::None => None,
                        })
                        .collect();
                    b.table = Some(TableBuilder { aligns: a, head: vec![], rows: vec![], row: vec![] });
                }
                Tag::TableHead | Tag::TableRow => {}
                Tag::TableCell => b.inl.clear(),
                Tag::Emphasis => b.cnt.italic += 1,
                Tag::Strong => b.cnt.bold += 1,
                Tag::Strikethrough => b.cnt.strike += 1,
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
                TagEnd::Paragraph => {
                    b.flush();
                    b.link.truncate(b.para_links);
                }
                TagEnd::Heading(_) => {
                    let base = slug(&plain(&b.inl));
                    let n = b.anchors.entry(base.clone()).or_insert(0);
                    let id = if *n > 0 { format!("{base}-{n}") } else { base };
                    *n += 1;
                    if let Some(Frame { kind: FrameKind::Heading(_, i), .. }) = b.stack.last_mut() {
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
                    b.raw = true;
                    b.html(&h);
                    b.flush();
                    b.raw = false;
                    b.skip = None;
                }
                TagEnd::List(_) => {
                    b.flush();
                    b.close_html_frames();
                    let f = b.pop_frame();
                    let start = if let FrameKind::List(s) = f.kind { s } else { None };
                    b.push_block(Block::List { start, items: f.items, tight: !f.loose });
                }
                TagEnd::Item => {
                    b.flush();
                    b.close_html_frames();
                    let f = b.pop_frame();
                    let loose = f.loose;
                    let item = ListItem { task: f.task, blocks: f.blocks };
                    let l = b.top();
                    l.items.push(item);
                    l.loose |= loose;
                }
                TagEnd::FootnoteDefinition => {
                    b.flush();
                    b.close_html_frames();
                    let f = b.pop_frame();
                    if let FrameKind::Foot(l) = f.kind {
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
                TagEnd::Link => {
                    b.link.pop();
                }
                TagEnd::Image => {
                    if let Some((src, alt, _)) = b.in_image.take() {
                        let link = b.link.last().copied().unwrap_or(0);
                        b.inl.push(Inline::Img { src, alt, w: None, link });
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
                    b.inl.push(Inline::Text(s.to_string(), st));
                }
            }
            // The parser runs without ENABLE_MATH, so it sends neither.
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
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
                b.inl.push(Inline::FootRef(n, l));
            }
            Event::SoftBreak => {
                if b.in_image.is_none() {
                    b.inl.push(Inline::Soft);
                }
            }
            Event::HardBreak => b.inl.push(Inline::Hard),
            Event::Rule => {
                b.flush();
                b.push_block(Block::Rule);
            }
            Event::TaskListMarker(c) => {
                if let Some(f) = b.stack.last_mut() {
                    f.task = Some(c);
                }
            }
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
