//! Block and inline layout into a display list. All sizes are device pixels.
use crate::native::doc::*;
use crate::native::highlight::{Tok, highlight};
use crate::native::images::{Images, Img, Status, render_svg};
use crate::native::math::{self, MItem};
use crate::native::page::*;
use crate::native::text::{BOLD, FKey, Fonts, ITALIC, MONO};
use crate::native::theme::{Rgb, Theme};
use std::collections::HashSet;
use std::rc::Rc;

pub struct Opts {
    /// Page width in device pixels.
    pub width: f32,
    /// Device pixels per CSS pixel.
    pub scale: f32,
    pub wrap_code: bool,
    /// Details blocks the user toggled (by id).
    pub toggled: HashSet<usize>,
    pub zoom: std::collections::HashMap<usize, f32>,
}

#[derive(Clone)]
struct Tc {
    size: f32,
    color: Rgb,
    bits: u8,
    lh: f32,
    align: u8,
}

#[derive(Clone)]
enum AK {
    Word(String),
    Space,
    Hard,
    Img(Rc<Bitmap>),
    Math(Rc<math::MBox>),
}

#[derive(Clone)]
struct Atom {
    k: AK,
    w: f32,
    f: FKey,
    color: Rgb,
    under: bool,
    strike: bool,
    link: u32,
    bg: u8,
    ox: f32,
    shift: f32,
    cjk: bool,
    /// Height above and below the baseline for images and math.
    up: f32,
    down: f32,
}

pub struct Layouter<'a> {
    pub fonts: &'a mut Fonts,
    pub th: &'a Theme,
    pub o: &'a Opts,
    pub doc: &'a Doc,
    pub images: &'a mut Images,
    pub page: Page,
    s: f32,
    clip: u16,
    pend_space: bool,
    align: u8,
    list_depth: usize,
    cur_w: f32,
    /// Block quotes around this point, whose text is muted.
    quote: usize,
    /// The body text size in CSS px: 16, or 12 in the footnotes.
    fsize: f32,
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x3000..=0x303F)
}

pub fn layout(fonts: &mut Fonts, th: &Theme, o: &Opts, doc: &Doc, images: &mut Images) -> Page {
    let mut l = Layouter { fonts, th, o, doc, images, page: Page::default(), s: o.scale, clip: 0, pend_space: false, align: 0, list_depth: 0, cur_w: 0.0, quote: 0, fsize: 16.0 };
    l.page.urls = doc.links.clone();
    l.page.w = o.width;
    let s = l.s;
    let mx = 24.0 * s;
    let mut y = 24.0 * s;
    let w = (o.width - 2.0 * mx).max(40.0 * s);
    for (i, b) in doc.blocks.iter().enumerate() {
        if i > 0 {
            y += l.gap(&doc.blocks[i - 1], b, false);
        }
        let (top, tx) = (y, l.page.text.len());
        l.block(b, mx, w, &mut y, false);
        l.nl();
        if let Some(Some((a, e))) = doc.lines.get(i) {
            l.page.blocks.push(SrcBlock { start: *a, end: *e, top, h: y - top, tx: (tx, l.page.text.len()) });
        }
    }
    l.page.h = y + 16.0 * s;
    l.page
}

impl<'a> Layouter<'a> {
    fn push(&mut self, x: f32, y: f32, w: f32, h: f32, k: Kind) -> usize {
        self.page.items.push(Item { x, y, w, h, clip: self.clip, k });
        self.page.items.len() - 1
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgb, radius: f32) -> usize {
        self.push(x, y, w, h, Kind::Rect { fill: 0xff000000 | fill, stroke: None, radius })
    }

    fn note_text(&mut self, s: &str) -> usize {
        if self.pend_space {
            if let Some(&c) = self.page.text.last() {
                if c != ' ' && c != '\n' && c != '\t' {
                    self.page.text.push(' ');
                }
            }
            self.pend_space = false;
        }
        let tx = self.page.text.len();
        self.page.text.extend(s.chars());
        tx
    }

    fn nl(&mut self) {
        if let Some(&c) = self.page.text.last() {
            if c != '\n' {
                self.page.text.push('\n');
            }
        }
        self.pend_space = false;
    }

    // ---------- blocks ----------

    fn gap(&self, prev: &Block, next: &Block, tight: bool) -> f32 {
        let s = self.s;
        if tight && matches!(prev, Block::Para(_)) && matches!(next, Block::List { .. }) {
            return 0.0;
        }
        if tight && matches!(prev, Block::List { .. }) && matches!(next, Block::Para(_)) {
            return 0.0;
        }
        16.0 * s
    }

    fn blocks(&mut self, bs: &[Block], x: f32, w: f32, y: &mut f32, tight: bool) {
        for (i, b) in bs.iter().enumerate() {
            if i > 0 {
                *y += self.gap(&bs[i - 1], b, tight);
            }
            self.block(b, x, w, y, tight);
            self.nl();
        }
    }

    fn base_tc(&self) -> Tc {
        let color = if self.quote > 0 { self.th.text_muted } else { self.th.text };
        Tc { size: self.fsize * self.s, color, bits: 0, lh: self.th.line_height, align: self.align }
    }

    fn block(&mut self, b: &Block, x: f32, w: f32, y: &mut f32, tight: bool) {
        let s = self.s;
        match b {
            Block::Para(v) => {
                let tc = self.base_tc();
                self.inlines(v, x, w, y, &tc);
            }
            Block::Heading(l, v, id) => {
                let first = *y <= 24.0 * s + 0.5;
                if !first {
                    *y += 8.0 * s; // 24 margin = 16 gap + 8
                }
                let em = self.th.heading_scale[(*l as usize - 1).min(5)];
                let tc = Tc { size: 16.0 * s * em, color: if *l == 6 { self.th.text_muted } else { self.th.heading }, bits: BOLD, lh: 1.25, align: self.align };
                self.page.anchors.entry(id.clone()).or_insert(*y);
                let y0 = *y;
                self.inlines(v, x, w, y, &tc);
                if *l <= 2 {
                    *y += 0.3 * tc.size;
                    self.rect(x, *y, w, s.max(1.0), self.th.rule, 0.0);
                    *y += s.max(1.0);
                }
                self.page.heads.push(Head { level: *l, text: plain(v), y: y0 });
            }
            Block::Rule => {
                *y += 8.0 * s;
                self.rect(x, *y, w, 4.0 * s, self.th.rule, 0.0);
                *y += 4.0 * s + 8.0 * s;
                self.page.text.push('\n');
            }
            Block::Code { lang, text } => self.code(lang, text, x, w, y),
            Block::Quote(bs) => {
                let bar = self.rect(x, *y, 4.0 * s, 0.0, self.th.rule, 0.0);
                let y0 = *y;
                let ind = 20.0 * s;
                self.quote_inner(bs, x + ind, w - ind, y);
                if let Kind::Rect { .. } = self.page.items[bar].k {
                    self.page.items[bar].h = *y - y0;
                }
            }
            Block::Alert(k, bs) => self.alert(*k, bs, x, w, y),
            Block::List { start, items, tight } => self.list(*start, items, *tight, x, w, y),
            Block::Table { aligns, head, rows } => self.table(aligns, head, rows, x, w, y),
            Block::Math(tex) => {
                let size = 16.0 * s * 1.15;
                let mb = math::layout(self.fonts, tex, size, true);
                let h = mb.asc + mb.desc;
                let pad = 8.0 * s;
                let bx = x + ((w - mb.w) / 2.0).max(0.0);
                let by = *y + pad;
                self.emit_math(&mb, bx, by + mb.asc, self.th.text, true);
                *y += h + 2.0 * pad;
            }
            Block::Mermaid(src) => self.mermaid(src, x, w, y),
            Block::Details { summary, open, body, id } => {
                let is_open = *open != self.o.toggled.contains(id);
                let tc = Tc { size: 16.0 * s, color: self.th.text, bits: BOLD, lh: 1.5, align: 0 };
                let y0 = *y;
                let ind = 20.0 * s;
                // triangle
                let c = (0xff000000u32) | self.th.text;
                let (tx, ty) = (x + 4.0 * s, y0 + 12.0 * s);
                let pts = if is_open { vec![(0.0, 0.0), (8.0 * s, 0.0), (4.0 * s, 7.0 * s)] } else { vec![(0.0, 0.0), (7.0 * s, 4.0 * s), (0.0, 8.0 * s)] };
                let ty = if is_open { ty - 2.0 * s } else { ty - 4.0 * s };
                self.push(tx, ty, 8.0 * s, 8.0 * s, Kind::Poly { pts, color: c, width: 1.0, fill: true });
                self.inlines(summary, x + ind, w - ind, y, &tc);
                if summary.is_empty() {
                    let tc2 = tc.clone();
                    self.inlines(&[Inl::Text("Details".into(), Sty::default())], x + ind, w - ind, y, &tc2);
                }
                self.page.toggles.push((x, y0, w, *y - y0, *id));
                if is_open && !body.is_empty() {
                    *y += 8.0 * s;
                    self.blocks(body, x, w, y, false);
                }
            }
            Block::Align(a, bs) => {
                let old = self.align;
                self.align = *a;
                self.blocks(bs, x, w, y, tight);
                self.align = old;
            }
            Block::Footnotes(v) => {
                *y += 8.0 * s;
                self.rect(x, *y, w, 1.0f32.max(s), self.th.rule, 0.0);
                *y += 16.0 * s;
                // GitHub's footnotes are 12 px muted text.
                let (old_size, old_quote) = (self.fsize, self.quote);
                (self.fsize, self.quote) = (12.0, 1);
                for (n, label, bl) in v {
                    let mut bl = bl.clone();
                    // back link on the last paragraph
                    let back = format!("#fnref-{}", label);
                    let id = {
                        self.page.urls.push(back);
                        self.page.urls.len() as u32
                    };
                    if let Some(Block::Para(p)) = bl.last_mut() {
                        p.push(Inl::Text(" ↩".into(), Sty { link: id, ..Sty::default() }));
                    }
                    self.page.anchors.entry(format!("fn-{}", label)).or_insert(*y);
                    let item = LItem { task: None, blocks: bl };
                    self.list(Some(*n as u64), &[item], true, x, w, y);
                    *y += 4.0 * s;
                }
                (self.fsize, self.quote) = (old_size, old_quote);
            }
        }
    }

    /// A block quote's blocks, in muted text as GitHub draws them.
    fn quote_inner(&mut self, bs: &[Block], x: f32, w: f32, y: &mut f32) {
        self.quote += 1;
        self.blocks(bs, x, w, y, false);
        self.quote -= 1;
    }

    fn alert(&mut self, k: Alert, bs: &[Block], x: f32, w: f32, y: &mut f32) {
        let s = self.s;
        let (i, title, glyph) = match k {
            Alert::Note => (0, "Note", "i"),
            Alert::Tip => (1, "Tip", "✦"),
            Alert::Important => (2, "Important", "!"),
            Alert::Warning => (3, "Warning", "!"),
            Alert::Caution => (4, "Caution", "×"),
        };
        let col = self.th.alerts[i];
        // GitHub's alert: 8 px padding above and below, a title line 16 px high, then 16 px to the body.
        let bar = self.rect(x, *y, 4.0 * s, 0.0, col, 0.0);
        let y0 = *y;
        *y += 8.0 * s;
        let ind = 20.0 * s;
        // icon
        let r = 8.0 * s;
        let cy = *y + 8.0 * s;
        self.push(x + ind, cy - r, 2.0 * r, 2.0 * r, Kind::Rect { fill: 0, stroke: Some((1.5 * s, 0xff000000 | col)), radius: r });
        let gf = FKey::new(11.0 * s, BOLD);
        let gw = self.fonts.measure(gf, glyph);
        let (ga, gd) = self.fonts.ascent_descent(gf);
        self.push(x + ind + r - gw / 2.0, cy - (ga + gd) / 2.0, gw, ga + gd, Kind::Text { s: glyph.into(), f: gf, color: col, under: false, strike: false, asc: ga, tx: usize::MAX });
        let tc = Tc { size: 16.0 * s, color: col, bits: BOLD, lh: 1.0, align: 0 };
        let tx = x + ind + 2.0 * r + 8.0 * s;
        self.inlines(&[Inl::Text(title.into(), Sty::default())], tx, w - ind - 2.0 * r - 8.0 * s, y, &tc);
        *y += 16.0 * s;
        self.nl();
        self.blocks(bs, x + ind, w - ind, y, false);
        *y += 8.0 * s;
        self.page.items[bar].h = *y - y0;
    }

    fn list(&mut self, start: Option<u64>, items: &[LItem], tight: bool, x: f32, w: f32, y: &mut f32) {
        let s = self.s;
        let ind = 32.0 * s;
        self.list_depth += 1;
        let depth = self.list_depth;
        let n0 = start.unwrap_or(1);
        // widest ordinal decides nothing: GitHub uses fixed 2em padding
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                *y += if tight { 4.0 * s } else { 16.0 * s };
            }
            let y0 = *y;
            let tc = self.base_tc();
            let (asc, desc) = self.fonts.ascent_descent(FKey::new(tc.size, 0));
            let lh = tc.size * tc.lh;
            let base = y0 + (lh - (asc + desc)) / 2.0 + asc;
            let cx = x + ind;
            match (it.task, start) {
                (Some(c), _) => {
                    // GitHub's disabled checkbox: 13 px, gray, set 1.4em left of the text's 2em indent.
                    let bs = 13.0 * s;
                    let bx = x + 9.6 * s;
                    let by = y0 + (lh - bs) / 2.0 + s;
                    let (border, fill, tick) = match (self.th.dark, c) {
                        (false, false) => (0xd1d1d1, 0xf8f8f8, 0),
                        (false, true) => (0xd1d1d1, 0xd1d1d1, 0xededed),
                        (true, false) => (0x626262, 0x3b3b3b, 0),
                        (true, true) => (0x757575, 0x757575, 0x3b3b3b),
                    };
                    self.push(bx, by, bs, bs, Kind::Rect { fill: 0xff000000 | fill, stroke: Some((s, 0xff000000 | border)), radius: 2.0 * s });
                    if c {
                        self.push(bx, by, bs, bs, Kind::Poly { pts: vec![(3.0 * s, 6.6 * s), (5.4 * s, 9.0 * s), (10.0 * s, 4.2 * s)], color: 0xff000000 | tick, width: 1.9 * s, fill: false });
                    }
                }
                (None, Some(_)) => {
                    // GitHub numbers a list inside one other list `i.`, and inside two or more `a.`.
                    let n = n0 + i as u64;
                    let label = match depth {
                        1 => format!("{n}."),
                        2 => format!("{}.", roman(n)),
                        _ => format!("{}.", alpha(n)),
                    };
                    let f = FKey::new(tc.size, 0);
                    let lw = self.fonts.measure(f, &label);
                    // The marker is the label and a space, outside the item.
                    let gap = self.fonts.measure(f, " ");
                    self.push(cx - gap - lw, base - asc, lw, asc + desc, Kind::Text { s: label, f, color: self.th.text, under: false, strike: false, asc, tx: usize::MAX });
                }
                (None, None) => {
                    let r = 2.8 * s;
                    let bxc = cx - 14.0 * s;
                    let byc = base - 0.33 * tc.size;
                    match (depth - 1).min(2) {
                        0 => {
                            self.push(bxc - r, byc - r, 2.0 * r, 2.0 * r, Kind::Rect { fill: 0xff000000 | self.th.text, stroke: None, radius: r });
                        }
                        1 => {
                            self.push(bxc - r, byc - r, 2.0 * r, 2.0 * r, Kind::Rect { fill: 0, stroke: Some((1.0 * s, 0xff000000 | self.th.text)), radius: r });
                        }
                        _ => {
                            self.push(bxc - r, byc - r, 2.0 * r, 2.0 * r, Kind::Rect { fill: 0xff000000 | self.th.text, stroke: None, radius: 0.0 });
                        }
                    }
                }
            }
            if it.blocks.is_empty() {
                *y += lh;
            } else {
                self.blocks(&it.blocks, cx, w - ind, y, tight);
            }
        }
        self.list_depth -= 1;
    }

    fn code(&mut self, lang: &str, text: &str, x: f32, w: f32, y: &mut f32) {
        let s = self.s;
        let pad = 16.0 * s;
        let size = 16.0 * self.th.code_size * s;
        let f = FKey::new(size, MONO);
        let (asc, desc) = self.fonts.ascent_descent(f);
        let adv = self.fonts.measure(f, "MMMMMMMMMM") / 10.0;
        let lh = size * 1.45;
        let orig = text;
        let text = text.replace('\t', "    ");
        let lines: Vec<&str> = text.split('\n').collect();
        let hl = highlight(lang, &text);
        let inner_w = w - 2.0 * pad;
        let maxc = if self.o.wrap_code { ((inner_w / adv).floor() as usize).max(1) } else { usize::MAX };
        // visual rows: (line idx, byte start, byte end)
        let mut rows: Vec<(usize, usize, usize)> = vec![];
        let mut widest = 0usize;
        for (li, l) in lines.iter().enumerate() {
            let chars: Vec<(usize, char)> = l.char_indices().collect();
            if chars.len() <= maxc {
                rows.push((li, 0, l.len()));
                widest = widest.max(chars.len());
            } else {
                let mut i = 0;
                while i < chars.len() {
                    let j = (i + maxc).min(chars.len());
                    let bs = chars[i].0;
                    let be = if j >= chars.len() { l.len() } else { chars[j].0 };
                    rows.push((li, bs, be));
                    i = j;
                }
                widest = widest.max(maxc);
            }
        }
        let content_w = widest as f32 * adv;
        let box_h = rows.len() as f32 * lh + 2.0 * pad;
        let y0 = *y;
        self.rect(x, y0, w, box_h, self.th.pre_bg, 6.0 * s);
        let scroll = content_w > inner_w + 0.5;
        let old_clip = self.clip;
        if scroll {
            self.page.hscroll.push(HScroll { x, y: y0, w, h: box_h, max: content_w + 2.0 * pad - w, bar_y: y0 + box_h - 10.0 * s });
            self.clip = self.page.hscroll.len() as u16;
        }
        // diff line backgrounds first
        let is_diff = matches!(lang.to_ascii_lowercase().as_str(), "diff" | "patch");
        for (ri, (li, _, _)) in rows.iter().enumerate() {
            if is_diff {
                let t = hl[*li].first().map(|t| t.2).unwrap_or(Tok::Plain);
                let bg = match t {
                    Tok::Added => Some(self.th.diff_add_bg),
                    Tok::Removed => Some(self.th.diff_del_bg),
                    _ => None,
                };
                if let Some(c) = bg {
                    self.rect(x, y0 + pad + ri as f32 * lh, content_w.max(inner_w) + 2.0 * pad, lh, c, 0.0);
                }
            }
        }
        let ty0 = y0 + pad;
        for (ri, (li, bs, be)) in rows.iter().enumerate() {
            let line = lines[*li];
            let top = ty0 + ri as f32 * lh;
            let base = top + (lh - (asc + desc)) / 2.0;
            // tokens clipped to [bs, be)
            let mut col = 0usize;
            let mut cursor = *bs;
            let mut spans: Vec<(usize, usize, Tok)> = vec![];
            for &(a, b, t) in &hl[*li] {
                let (a, b) = (a.max(*bs), b.min(*be));
                if a < b {
                    if a > cursor {
                        spans.push((cursor, a, Tok::Plain));
                    }
                    spans.push((a, b, t));
                    cursor = b;
                }
            }
            if cursor < *be {
                spans.push((cursor, *be, Tok::Plain));
            }
            let tx0 = self.page.text.len();
            let _ = tx0;
            for (a, b, t) in spans {
                let seg = &line[a..b];
                let n = seg.chars().count();
                let tx = self.note_text(seg);
                let color = self.tok_color(t);
                self.push(x + pad + col as f32 * adv, base, n as f32 * adv, asc + desc, Kind::Text { s: seg.to_string(), f, color, under: false, strike: false, asc, tx });
                col += n;
            }
            if ri + 1 < rows.len() && rows[ri + 1].0 != *li {
                self.page.text.push('\n');
            } else if ri + 1 < rows.len() {
                // wrapped line: no separator
            }
        }
        self.clip = old_clip;
        // copy button
        let bw = 28.0 * s;
        self.page.copies.push(orig.to_string());
        let ci = self.page.copies.len() - 1;
        self.page.buttons.push(Button { x: x + w - bw - 8.0 * s, y: y0 + 8.0 * s, w: bw, h: bw, kind: BtnKind::CopyCode, data: ci });
        *y = y0 + box_h;
    }

    fn tok_color(&self, t: Tok) -> Rgb {
        let pl = &self.th.pl;
        match t {
            Tok::Plain => self.th.text,
            Tok::Keyword | Tok::Punct => pl.keyword,
            Tok::Str => pl.string,
            Tok::Comment => pl.comment,
            Tok::Number | Tok::Type | Tok::Meta => pl.constant,
            Tok::Func => pl.entity,
            Tok::Added => pl.inserted,
            Tok::Removed => pl.deleted,
        }
    }

    fn mermaid(&mut self, src: &str, x: f32, w: f32, y: &mut f32) {
        let s = self.s;
        let z = self.o.zoom.get(&self.page.diagrams.len()).copied().unwrap_or(1.0);
        let bmp = crate::native::mermaid::render(src, self.th, (w / s).max(100.0), s * z);
        let y0 = *y;
        match bmp {
            Some(b) => {
                let (bw, bh) = (b.w as f32, b.h as f32);
                let base_w = bw / z;
                let sc0 = if base_w > w { w / base_w } else { 1.0 };
                let (dw, dh) = (base_w * sc0 * z, bh / z * sc0 * z);
                let pad = 16.0 * s;
                let scroll = dw > w - 2.0 * pad + 0.5;
                self.rect(x, y0, w, dh + 2.0 * pad, self.th.pre_bg, 6.0 * s);
                let old_clip = self.clip;
                let bx = if scroll { x + pad } else { x + (w - dw) / 2.0 };
                if scroll {
                    self.page.hscroll.push(HScroll { x, y: y0, w, h: dh + 2.0 * pad, max: dw + 2.0 * pad - w, bar_y: y0 + dh + 2.0 * pad - 10.0 * s });
                    self.clip = self.page.hscroll.len() as u16;
                }
                let b = Rc::new(b);
                self.push(bx, y0 + pad, dw, dh, Kind::Img(b.clone()));
                self.clip = old_clip;
                self.page.diagrams.push(Diagram { bmp: b, src: src.to_string() });
                let di = self.page.diagrams.len() - 1;
                self.page.copies.push(src.to_string());
                let ci = self.page.copies.len() - 1;
                // GitHub's diagram buttons: 34 by 32 px, 8 px apart and from the edge, 10 px down.
                let (bw, bh, gap) = (34.0 * s, 32.0 * s, 8.0 * s);
                let bx0 = x + w - gap - bw;
                let by0 = y0 + 10.0 * s;
                for (i, kind, data) in [(0.0, BtnKind::CopyDiagram, ci), (1.0, BtnKind::Expand, di), (2.0, BtnKind::ZoomIn, di), (3.0, BtnKind::ZoomReset, di)] {
                    self.page.buttons.push(Button { x: bx0 - i * (bw + gap), y: by0, w: bw, h: bh, kind, data });
                }
                *y = y0 + dh + 2.0 * pad;
            }
            None => {
                // show the source as a code block
                self.code("", src, x, w, y);
            }
        }
    }

    fn emit_math(&mut self, mb: &math::MBox, x: f32, base: f32, color: Rgb, _display: bool) {
        for it in &mb.items {
            match it {
                MItem::Text { s, f, x: ix, y: iy } => {
                    let (asc, desc) = self.fonts.ascent_descent(*f);
                    let w = self.fonts.measure(*f, s);
                    let tx = self.note_text(s);
                    self.push(x + ix, base + iy - asc, w, asc + desc, Kind::Text { s: s.clone(), f: *f, color, under: false, strike: false, asc, tx });
                }
                MItem::Rule { x: ix, y: iy, w, h } => {
                    self.rect(x + ix, base + iy, *w, *h, color, 0.0);
                }
                MItem::Path { pts, w } => {
                    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                    for p in pts {
                        x0 = x0.min(p.0);
                        y0 = y0.min(p.1);
                        x1 = x1.max(p.0);
                        y1 = y1.max(p.1);
                    }
                    let rel: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 - x0, p.1 - y0)).collect();
                    self.push(x + x0, base + y0, x1 - x0, y1 - y0, Kind::Poly { pts: rel, color: 0xff000000 | color, width: *w, fill: false });
                }
            }
        }
    }

    // ---------- tables ----------

    fn table(&mut self, aligns: &[u8], head: &[Vec<Inl>], rows: &[Vec<Vec<Inl>>], x: f32, w: f32, y: &mut f32) {
        let s = self.s;
        let ncol = head.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
        if ncol == 0 {
            return;
        }
        let padx = 13.0 * s;
        let pady = 6.0 * s;
        let mut tcn = self.base_tc();
        tcn.align = 0;
        let mut tch = tcn.clone();
        tch.bits = BOLD;
        let mut minw = vec![0f32; ncol];
        let mut maxw = vec![0f32; ncol];
        let mut measure_row = |me: &mut Self, r: &[Vec<Inl>], tc: &Tc| {
            for (c, cell) in r.iter().enumerate().take(ncol) {
                let atoms = me.atoms(cell, tc);
                let (mn, mx) = me.atom_extent(&atoms);
                minw[c] = minw[c].max(mn);
                maxw[c] = maxw[c].max(mx);
            }
        };
        measure_row(self, head, &tch);
        for r in rows {
            measure_row(self, r, &tcn);
        }
        let sum_min: f32 = minw.iter().sum::<f32>() + ncol as f32 * 2.0 * padx;
        let sum_max: f32 = maxw.iter().sum::<f32>() + ncol as f32 * 2.0 * padx;
        let colw: Vec<f32> = if sum_max <= w {
            (0..ncol).map(|c| maxw[c] + 2.0 * padx).collect()
        } else if sum_min >= w {
            (0..ncol).map(|c| minw[c] + 2.0 * padx).collect()
        } else {
            let t = (w - sum_min) / (sum_max - sum_min);
            (0..ncol).map(|c| minw[c] + (maxw[c] - minw[c]) * t + 2.0 * padx).collect()
        };
        let tw: f32 = colw.iter().sum();
        let scroll = tw > w + 0.5;
        let y0 = *y;
        let old_clip = self.clip;
        let hs_idx = self.page.hscroll.len();
        if scroll {
            self.page.hscroll.push(HScroll { x, y: y0, w, h: 0.0, max: tw - w, bar_y: 0.0 });
            self.clip = self.page.hscroll.len() as u16;
        }
        let bc = self.th.rule;
        let line = s.max(1.0);
        let mut cy = y0;
        let nrows = rows.len() + 1;
        let mut row_idx_for_bg: Vec<usize> = vec![];
        // Front matter's table has no head row.
        for ri in usize::from(head.is_empty())..nrows {
            let (cells, tc, hdr) = if ri == 0 { (head, &tch, true) } else { (&rows[ri - 1][..], &tcn, false) };
            // bg placeholder
            let bgi = if !hdr && ri % 2 == 0 { Some(self.rect(x, cy, tw, 0.0, self.th.alt_bg, 0.0)) } else { None };
            // lay out cells to find the row height
            let mut rh = 0f32;
            let save_len = self.page.items.len();
            let mut cx = x;
            for c in 0..ncol {
                let cell: &[Inl] = cells.get(c).map(|v| &v[..]).unwrap_or(&[]);
                let mut ty = cy + pady;
                let mut t2 = tc.clone();
                t2.align = aligns.get(c).copied().unwrap_or(0);
                self.pend_space = false;
                let before = self.page.text.len();
                let _ = before;
                self.inlines(cell, cx + padx, colw[c] - 2.0 * padx, &mut ty, &t2);
                self.page.text.push(if c + 1 < ncol { '\t' } else { '\n' });
                rh = rh.max(ty + pady - cy);
                cx += colw[c];
            }
            let _ = save_len;
            if let Some(i) = bgi {
                self.page.items[i].h = rh;
                row_idx_for_bg.push(i);
            }
            cy += rh;
            // horizontal line below row (and above first)
            self.rect(x, cy - line * 0.0, tw, line, bc, 0.0);
            cy += line;
        }
        // top line and verticals
        self.rect(x, y0 - 0.0, tw, line, bc, 0.0);
        let mut vx = x;
        for c in 0..=ncol {
            self.rect(vx - if c == ncol { line } else { 0.0 }, y0, line, cy - y0, bc, 0.0);
            if c < ncol {
                vx += colw[c];
            }
        }
        self.clip = old_clip;
        let mut bottom = cy;
        if scroll {
            bottom += 12.0 * s;
            let h = &mut self.page.hscroll[hs_idx];
            h.h = bottom - y0;
            h.bar_y = bottom - 8.0 * s;
        }
        *y = bottom;
    }

    fn atom_extent(&mut self, atoms: &[Atom]) -> (f32, f32) {
        let (mut mn, mut mx, mut cur) = (0f32, 0f32, 0f32);
        for a in atoms {
            match a.k {
                AK::Hard => {
                    mx = mx.max(cur);
                    cur = 0.0;
                }
                _ => {
                    cur += a.w;
                    if !matches!(a.k, AK::Space) {
                        mn = mn.max(a.w);
                    }
                }
            }
        }
        (mn, mx.max(cur))
    }

    // ---------- inlines ----------

    fn atoms(&mut self, inl: &[Inl], tc: &Tc) -> Vec<Atom> {
        let mut out: Vec<Atom> = vec![];
        let th = self.th;
        for i in inl {
            match i {
                Inl::Text(t, st) => self.text_atoms(t, st, tc, &mut out),
                Inl::Soft => {
                    let f = FKey::new(tc.size, tc.bits);
                    let w = self.fonts.measure(f, " ");
                    out.push(Atom { k: AK::Space, w, f, color: tc.color, under: false, strike: false, link: 0, bg: 0, ox: 0.0, shift: 0.0, cjk: false, up: 0.0, down: 0.0 });
                }
                Inl::Hard => {
                    let f = FKey::new(tc.size, tc.bits);
                    out.push(Atom { k: AK::Hard, w: 0.0, f, color: tc.color, under: false, strike: false, link: 0, bg: 0, ox: 0.0, shift: 0.0, cjk: false, up: 0.0, down: 0.0 });
                }
                Inl::FootRef(n, label) => {
                    self.page.urls.push(format!("#fn-{}", label));
                    let link = self.page.urls.len() as u32;
                    let sty = Sty { sup: true, link, ..Sty::default() };
                    // GitHub brackets the number.
                    self.text_atoms(&format!("[{n}]"), &sty, tc, &mut out);
                }
                Inl::Math(tex, st) => {
                    let size = tc.size * 1.0;
                    let (tex, display) = match tex.strip_prefix("$$").and_then(|t| t.strip_suffix("$$")) {
                        Some(t) => (t, true),
                        None => (tex.strip_prefix('$').and_then(|t| t.strip_suffix('$')).unwrap_or(tex), false),
                    };
                    let mb = math::layout(self.fonts, tex, size, display);
                    let f = FKey::new(tc.size, tc.bits);
                    out.push(Atom {
                        k: AK::Math(Rc::new(math::MBox { w: mb.w, asc: mb.asc, desc: mb.desc, items: mb.items })),
                        w: 0.0,
                        f,
                        color: if st.link > 0 { th.link } else { tc.color },
                        under: false,
                        strike: false,
                        link: st.link,
                        bg: 0,
                        ox: 0.0,
                        shift: 0.0,
                        cjk: false,
                        up: 0.0,
                        down: 0.0,
                    });
                    let last = out.last_mut().unwrap();
                    if let AK::Math(m) = &last.k {
                        last.w = m.w;
                        last.up = m.asc;
                        last.down = m.desc;
                    }
                }
                Inl::Img { src, alt, w, link } => {
                    let wanted = &mut self.page.wanted;
                    match self.images.get(src, wanted) {
                        Status::Ready(img) => {
                            let (nw, nh) = img.size();
                            let s = self.s;
                            let mut dw = w.map(|w| w * s).unwrap_or(nw * s);
                            let mut dh = if let Some(_) = w { dw * nh / nw.max(1.0) } else { nh * s };
                            let maxw = self.cur_w.max(40.0);
                            if dw > maxw {
                                dh *= maxw / dw;
                                dw = maxw;
                            }
                            let bmp = match &img {
                                Img::Raster(b) => b.clone(),
                                Img::Svg(t) => match render_svg(t, dw.round() as u32, dh.round() as u32) {
                                    Some(b) => Rc::new(b),
                                    None => continue,
                                },
                            };
                            let f = FKey::new(tc.size, tc.bits);
                            out.push(Atom { k: AK::Img(bmp), w: dw, f, color: tc.color, under: false, strike: false, link: *link, bg: 0, ox: 0.0, shift: 0.0, cjk: false, up: dh, down: 0.0 });
                        }
                        Status::Pending | Status::Failed => {
                            let t = if alt.is_empty() { "[image]".to_string() } else { alt.clone() };
                            let sty = Sty { italic: true, link: *link, ..Sty::default() };
                            let mut t2 = tc.clone();
                            t2.color = th.fg_muted;
                            self.text_atoms(&t, &sty, &t2, &mut out);
                        }
                    }
                }
            }
        }
        out
    }

    fn text_atoms(&mut self, t: &str, st: &Sty, tc: &Tc, out: &mut Vec<Atom>) {
        let th = self.th;
        let mut bits = tc.bits;
        if st.bold {
            bits |= BOLD;
        }
        if st.italic {
            bits |= ITALIC;
        }
        let mut size = tc.size;
        let mono = st.code || st.kbd;
        if mono {
            bits |= MONO;
            size *= self.th.code_size;
        }
        if st.sub || st.sup {
            size *= 0.75;
        }
        let f = FKey::new(size, bits);
        let shift = if st.sup {
            -0.4 * tc.size
        } else if st.sub {
            0.2 * tc.size
        } else {
            0.0
        };
        let color = if st.link > 0 { th.link } else { tc.color };
        // Links are underlined, as GitHub does for readers who ask for it and mddog always does.
        let under = st.under || st.link > 0;
        let bg = if st.code {
            1
        } else if st.kbd {
            2
        } else if st.mark {
            3
        } else {
            0
        };
        let first_idx = out.len();
        let mut word = String::new();
        let mut push_word = |me: &mut Self, word: &mut String, out: &mut Vec<Atom>, cjk: bool| {
            if word.is_empty() {
                return;
            }
            let w = me.fonts.measure(f, word);
            out.push(Atom { k: AK::Word(std::mem::take(word)), w, f, color, under, strike: st.strike, link: st.link, bg, ox: 0.0, shift, cjk, up: 0.0, down: 0.0 });
        };
        let sp_w = self.fonts.measure(f, " ");
        for c in t.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                push_word(self, &mut word, out, false);
                if !matches!(out.last().map(|a| &a.k), Some(AK::Space)) || out.len() == first_idx {
                    out.push(Atom { k: AK::Space, w: sp_w, f, color, under, strike: st.strike, link: st.link, bg, ox: 0.0, shift, cjk: false, up: 0.0, down: 0.0 });
                }
            } else if is_cjk(c) {
                push_word(self, &mut word, out, false);
                word.push(c);
                push_word(self, &mut word, out, true);
            } else {
                word.push(c);
            }
        }
        push_word(self, &mut word, out, false);
        if bg == 1 || bg == 2 {
            let pad_l = if bg == 1 { 0.4 * size } else { 0.35 * size };
            let pad_r = pad_l;
            if let Some(a) = out[first_idx..].iter_mut().find(|a| matches!(a.k, AK::Word(_))) {
                a.ox = pad_l;
                a.w += pad_l;
            }
            if let Some(a) = out[first_idx..].iter_mut().rev().find(|a| matches!(a.k, AK::Word(_))) {
                a.w += pad_r;
            }
        }
    }

    fn inlines(&mut self, inl: &[Inl], x: f32, w: f32, y: &mut f32, tc: &Tc) {
        self.cur_w = w;
        let atoms = self.atoms(inl, tc);
        self.flow(atoms, x, w, y, tc);
    }

    fn flow(&mut self, atoms: Vec<Atom>, x: f32, w: f32, y: &mut f32, tc: &Tc) {
        let mut line: Vec<Atom> = vec![];
        let mut lw = 0f32;
        let mut brk: Option<usize> = None; // break after this index
        let mut wrapped = false;
        let mut any_line = false;
        let mut queue: std::collections::VecDeque<Atom> = atoms.into();
        while let Some(a) = queue.pop_front() {
            match a.k {
                AK::Hard => {
                    self.emit_line(&mut line, x, w, y, tc, true, &mut wrapped);
                    any_line = true;
                    lw = 0.0;
                    brk = None;
                    continue;
                }
                AK::Space => {
                    if line.is_empty() {
                        continue;
                    }
                    lw += a.w;
                    line.push(a);
                    brk = Some(line.len());
                    continue;
                }
                _ => {}
            }
            if a.cjk && !line.is_empty() {
                brk = Some(line.len());
            }
            if lw + a.w > w + 0.01 && !line.is_empty() {
                if let Some(b) = brk {
                    // carry the tail
                    let tail: Vec<Atom> = line.split_off(b);
                    self.emit_line(&mut line, x, w, y, tc, false, &mut wrapped);
                    any_line = true;
                    line = tail;
                    lw = line.iter().map(|a| a.w).sum();
                    brk = None;
                    queue.push_front(a);
                    continue;
                } else if lw + a.w > w * 1.0 && matches!(a.k, AK::Img(_)) {
                    self.emit_line(&mut line, x, w, y, tc, false, &mut wrapped);
                    any_line = true;
                    line = vec![];
                    lw = 0.0;
                    queue.push_front(a);
                    continue;
                }
            }
            // a single word longer than the line: split it by chars
            if a.w > w + 0.01 && line.is_empty() {
                if let AK::Word(ref s) = a.k {
                    let chars: Vec<char> = s.chars().collect();
                    if chars.len() > 1 {
                        // largest prefix that fits, by binary search (few measures)
                        let (mut lo, mut hi) = (1usize, chars.len());
                        while lo < hi {
                            let mid = (lo + hi + 1) / 2;
                            let p: String = chars[..mid].iter().collect();
                            if self.fonts.measure(a.f, &p) <= w {
                                lo = mid;
                            } else {
                                hi = mid - 1;
                            }
                        }
                        let n = lo;
                        let piece_w = {
                            let p: String = chars[..n].iter().collect();
                            self.fonts.measure(a.f, &p)
                        };
                        let head: String = chars[..n].iter().collect();
                        let rest: String = chars[n..].iter().collect();
                        let mut h = a.clone();
                        h.k = AK::Word(head);
                        h.w = piece_w;
                        let mut r = a.clone();
                        let rw = self.fonts.measure(a.f, &rest);
                        r.k = AK::Word(rest);
                        r.w = rw;
                        r.ox = 0.0;
                        line.push(h);
                        self.emit_line(&mut line, x, w, y, tc, false, &mut wrapped);
                        self.pend_space = false;
                        any_line = true;
                        queue.push_front(r);
                        lw = 0.0;
                        brk = None;
                        continue;
                    }
                }
            }
            lw += a.w;
            line.push(a);
        }
        if !line.is_empty() || !any_line {
            self.emit_line(&mut line, x, w, y, tc, false, &mut wrapped);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_line(&mut self, line: &mut Vec<Atom>, x: f32, w: f32, y: &mut f32, tc: &Tc, hard: bool, wrapped: &mut bool) {
        // strip trailing spaces
        while matches!(line.last().map(|a| &a.k), Some(AK::Space)) {
            line.pop();
        }
        let lh = tc.size * tc.lh;
        let base_f = FKey::new(tc.size, tc.bits);
        let (asc, desc) = self.fonts.ascent_descent(base_f);
        let half = (lh - (asc + desc)) / 2.0;
        let mut up = asc + half;
        let mut down = desc + half;
        for a in line.iter() {
            if a.up > 0.0 || a.down > 0.0 {
                up = up.max(a.up + half.min(2.0));
                down = down.max(a.down + half.min(2.0));
            }
        }
        let width: f32 = line.iter().map(|a| a.w).sum();
        let mut cx = match (tc.align.max(0), self.align_of(tc)) {
            (_, 1) => x + ((w - width) / 2.0).max(0.0),
            (_, 2) => x + (w - width).max(0.0),
            _ => x,
        };
        let base = *y + up;
        let mut bg_run: Option<(u8, f32, f32, FKey, usize)> = None;
        let mut link_run: Option<(u32, f32, f32)> = None;
        let n = line.len();
        for (i, a) in line.iter().enumerate() {
            let x0 = cx;
            // background run management
            match (&mut bg_run, a.bg) {
                (Some((k, _, e, _, _)), b) if *k == b && b != 0 => *e = x0 + a.w,
                _ => {
                    if let Some((k, s0, e0, f, idx)) = bg_run.take() {
                        self.fill_bg(idx, k, s0, e0, base, f);
                    }
                    if a.bg != 0 {
                        let idx = self.push(x0, base, 0.0, 0.0, Kind::Rect { fill: 0, stroke: None, radius: 0.0 });
                        bg_run = Some((a.bg, x0, x0 + a.w, a.f, idx));
                    }
                }
            }
            // link run
            match (&mut link_run, a.link) {
                (Some((l, _, e)), k) if *l == k && k != 0 => *e = x0 + a.w,
                _ => {
                    if let Some((l, s0, e0)) = link_run.take() {
                        self.emit_link(l, s0, e0, *y, up + down);
                    }
                    if a.link != 0 {
                        link_run = Some((a.link, x0, x0 + a.w));
                    }
                }
            }
            match &a.k {
                AK::Word(t) => {
                    if a.link > 0 {
                        if let Some(l) = self.page.urls.get(a.link as usize - 1).and_then(|u| u.strip_prefix("#fn-")) {
                            let key = format!("fnref-{}", l);
                            self.page.anchors.entry(key).or_insert(*y);
                        }
                    }
                    let (fa, fd) = self.fonts.ascent_descent(a.f);
                    let tx = self.note_text(t);
                    let tw = a.w - a.ox - if a.bg == 1 || a.bg == 2 { if is_last_word(line, i) { a.f.px() * if a.bg == 1 { 0.4 } else { 0.35 } } else { 0.0 } } else { 0.0 };
                    self.push(x0 + a.ox, base + a.shift - fa, tw, fa + fd, Kind::Text { s: t.clone(), f: a.f, color: a.color, under: a.under, strike: a.strike, asc: fa, tx });
                }
                AK::Space => {
                    self.pend_space = true;
                    // An underline or strike carries across a space between two words that have it.
                    if let Some(next) = line.get(i + 1).filter(|b| i > 0 && matches!(b.k, AK::Word(_))) {
                        let (px, th) = (a.f.px(), (a.f.px() / 16.0).max(1.0));
                        for (on, dy) in [(a.under && next.under, 0.12), (a.strike && next.strike, -0.3)] {
                            if on {
                                self.push(x0, base + a.shift + dy * px, a.w, th, Kind::Rect { fill: 0xff000000 | a.color, stroke: None, radius: 0.0 });
                            }
                        }
                    }
                }
                AK::Img(b) => {
                    let dh = a.up;
                    self.push(x0, base - dh, a.w, dh, Kind::Img(b.clone()));
                }
                AK::Math(m) => {
                    let mb = m.clone();
                    self.emit_math(&mb, x0, base + a.shift, a.color, false);
                }
                AK::Hard => {}
            }
            cx += a.w;
            let _ = n;
        }
        if let Some((k, s0, e0, f, idx)) = bg_run.take() {
            self.fill_bg(idx, k, s0, e0, base, f);
        }
        if let Some((l, s0, e0)) = link_run.take() {
            self.emit_link(l, s0, e0, *y, up + down);
        }
        *y += up + down;
        line.clear();
        if hard {
            self.page.text.push('\n');
            self.pend_space = false;
        } else {
            self.pend_space = true;
        }
        *wrapped = !hard;
    }

    fn align_of(&self, tc: &Tc) -> u8 {
        if tc.align != 0 { tc.align } else { 0 }
    }

    fn fill_bg(&mut self, idx: usize, kind: u8, x0: f32, x1: f32, base: f32, f: FKey) {
        let s = self.s;
        let (a, d) = self.fonts.ascent_descent(f);
        let (fill, stroke, r) = match kind {
            1 => (0xff000000 | self.th.code_bg, None, 6.0 * s),
            2 => (0xff000000 | self.th.pre_bg, Some((s.max(1.0), 0xff000000 | self.th.rule)), 6.0 * s),
            _ => (0xff000000 | self.th.mark_bg, None, 2.0 * s),
        };
        let pad = if kind == 3 { 0.0 } else { 0.1 * f.px() };
        self.page.items[idx] = Item { x: x0, y: base - a - pad, w: x1 - x0, h: a + d + 2.0 * pad, clip: self.clip, k: Kind::Rect { fill, stroke, radius: r } };
    }

    fn emit_link(&mut self, link: u32, x0: f32, x1: f32, y: f32, h: f32) {
        self.page.links.push(LinkRect { x: x0, y, w: x1 - x0, h, clip: self.clip, link });
    }
}

fn is_last_word(line: &[Atom], i: usize) -> bool {
    !line[i + 1..].iter().any(|a| matches!(a.k, AK::Word(_)) && a.bg == line[i].bg)
}

/// `n` in lower-case Roman numerals, as CSS's `lower-roman` counts. Outside 1 to 3999 CSS falls back to decimal,
/// which also keeps a huge list start from building a huge marker.
fn roman(mut n: u64) -> String {
    if !(1..=3999).contains(&n) {
        return n.to_string();
    }
    let mut out = String::new();
    for (v, s) in [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")] {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// `n` in lower-case letters, as CSS's `lower-alpha` counts: a to z, then aa.
fn alpha(n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let (mut n, mut out) = (n, Vec::new());
    while n > 0 {
        n -= 1;
        out.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn list_markers_count_like_css() {
        assert_eq!([1, 4, 9, 14, 40, 3999].map(super::roman), ["i", "iv", "ix", "xiv", "xl", "mmmcmxcix"]);
        assert_eq!([0, 4000, 999_999_999].map(super::roman), ["0", "4000", "999999999"]);
        assert_eq!([1, 26, 27, 52, 703].map(super::alpha), ["a", "z", "aa", "az", "aaa"]);
    }
}
