//! The native engine: lays out and draws the page with CoreText and CoreGraphics, and answers the viewer's
//! calls (search, selection, hit tests, wide-block scrolling) from the laid-out `Page`.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::json;

use super::{Bitmap, Block, Layout, Pointer, Press, Select, snapshot_size};
use crate::native::doc::{self, Doc};
use crate::native::images::Images;
use crate::native::layout::{self, Opts};
use crate::native::page::{BtnKind, Kind, Page};
use crate::native::raster::{self, Ctx, Expanded, Overlay};
use crate::native::text::Fonts;
use crate::native::theme::Theme;
use crate::term::kitty::{self, Shm};

pub struct Engine {
    pub width: Cell<f64>,
    pub scale: Cell<f64>,
    st: RefCell<St>,
}

struct St {
    fonts: Fonts,
    th: Theme,
    doc: Option<Doc>,
    images: Images,
    wrap: bool,
    toggled: HashSet<usize>,
    zoom: HashMap<usize, f32>,
    page: Page,
    runs: Vec<(usize, usize, u32)>,
    ov: Overlay,
    sel: Option<(usize, usize)>,
    matches: Vec<(usize, usize)>,
    cur: Option<usize>,
    held: Option<usize>,
    /// How far right of the held thumb's center the press was, so a drag keeps the thumb under the pointer.
    grip: f32,
    pending: usize,
}

type R = [f64; 4];

impl Engine {
    pub fn new(width: f64, scale: f64) -> Engine {
        let st = St {
            fonts: Fonts::new(),
            th: Theme::new(crate::theme::builtin("github-light").unwrap_or_else(|| unreachable!())),
            doc: None,
            images: Images::new(".".into()),
            wrap: false,
            toggled: HashSet::new(),
            zoom: HashMap::new(),
            page: Page::default(),
            runs: vec![],
            ov: Overlay::default(),
            sel: None,
            matches: vec![],
            cur: None,
            held: None,
            grip: 0.0,
            pending: 0,
        };
        Engine { width: Cell::new(width), scale: Cell::new(scale), st: RefCell::new(st) }
    }

    /// Parses and lays out `src` in place of the page.
    pub fn load(&self, src: &str, base: &Path, wrap: bool, th: &crate::theme::Theme) {
        let mut st = self.st.borrow_mut();
        crate::trace::mark("native parse start");
        st.th = Theme::new(th);
        let (body, code) = (st.th.font_body.clone(), st.th.font_code.clone());
        st.fonts.set_families(body, code);
        st.doc = Some(doc::parse(if src.trim().is_empty() { "*This file is empty.*" } else { src }));
        crate::trace::mark("native parsed");
        if let Some(doc) = &st.doc {
            crate::native::highlight::prefetch(doc);
            crate::trace::mark("code highlighted");
        }
        st.images = Images::new(base.to_path_buf());
        st.wrap = wrap;
        st.toggled.clear();
        st.zoom.clear();
        st.sel = None;
        st.held = None;
        st.ov = Overlay::default();
        self.relayout(&mut st);
        let wanted = std::mem::take(&mut st.page.wanted);
        st.pending = wanted.len();
        st.images.fetch(wanted);
        crate::trace::mark("native laid out");
    }

    fn relayout(&self, st: &mut St) {
        let Some(doc) = st.doc.take() else { return };
        let o = Opts { width: (self.width.get() * self.scale.get()) as f32, scale: self.scale.get() as f32, wrap_code: st.wrap, toggled: st.toggled.clone(), zoom: st.zoom.clone() };
        let St { fonts, th, images, .. } = st;
        st.page = layout::layout(fonts, th, &o, &doc, images);
        st.doc = Some(doc);
        st.runs = build_runs(&st.page);
        let n = st.page.hscroll.len();
        st.ov.hoff.resize(n, 0.0);
        for (i, h) in st.page.hscroll.iter().enumerate() {
            st.ov.hoff[i] = st.ov.hoff[i].min(h.max);
        }
        st.sel = None;
        // Button and scroll-bar indices point into the old page.
        st.ov.hover_btn = None;
        st.ov.copied_btn = None;
        st.held = None;
        if !st.matches.is_empty() {
            st.matches.clear();
            st.cur = None;
        }
        self.overlay(st);
    }

    /// Waits up to `timeout` for the remote images, then lays out again if any arrived.
    pub fn wait_images(&self, timeout: std::time::Duration) {
        let end = std::time::Instant::now() + timeout;
        let mut st = self.st.borrow_mut();
        let mut changed = false;
        while st.images.loading() > 0 && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(10));
            changed |= st.images.poll();
        }
        if changed {
            st.pending = 0;
            self.relayout(&mut st);
        }
    }

    pub fn rescale(&self, scale: f64, width: f64) {
        self.scale.set(scale);
        self.width.set(width);
        let mut st = self.st.borrow_mut();
        self.relayout(&mut st);
    }

    pub fn resize(&self, width: f64) {
        if (width - self.width.get()).abs() < 0.5 {
            return;
        }
        self.width.set(width);
        let mut st = self.st.borrow_mut();
        self.relayout(&mut st);
    }

    pub fn snapshot(&self, y: f64, h: f64, scale: f64) -> Option<Bitmap> {
        let (width, height) = snapshot_size(self.width.get(), h, scale);
        let mut rgba = Shm::create(kitty::shm_name(), width * height * 4)?;
        self.snapshot_into(y, h, scale, &mut rgba)?;
        Some(Bitmap { width, height, rgba })
    }

    pub fn snapshot_into(&self, y: f64, h: f64, scale: f64, dst: &mut [u8]) -> Option<()> {
        let (width, height) = snapshot_size(self.width.get(), h, scale);
        let dst = dst.get_mut(..width * height * 4)?;
        let mut st = self.st.borrow_mut();
        let s = self.scale.get() as f32;
        let (top, bottom) = (y as f32 * s, (y + h) as f32 * s);
        let page = &st.page;
        let items: Vec<u32> = page.items.iter().enumerate().filter(|(_, it)| it.y <= bottom && it.y + it.h >= top - 2.0).map(|(i, _)| i as u32).collect();
        let St { fonts, page, th, ov, .. } = &mut *st;
        let cx = Ctx { page, th, scale: s, ov };
        raster::render_into(fonts, &cx, top, height, width, &items, scale / self.scale.get(), dst);
        Some(())
    }

    fn overlay(&self, st: &mut St) {
        let mut marks = vec![];
        for (i, &(a, b)) in st.matches.clone().iter().enumerate() {
            for r in match_rects(st, a, b) {
                marks.push((r.0 + dx(st, r.4), r.1, r.2, r.3, Some(i) == st.cur));
            }
        }
        st.ov.marks = marks;
        st.ov.sel = st.sel.map(|(a, b)| match_rects(st, a, b).into_iter().map(|r| (r.0 + dx(st, r.4), r.1, r.2, r.3)).collect()).unwrap_or_default();
    }

    fn css(&self, r: (f32, f32, f32, f32)) -> R {
        let s = self.scale.get();
        [f64::from(r.0) / s, f64::from(r.1) / s, f64::from(r.2) / s, f64::from(r.3) / s]
    }

    fn rects_of(&self, st: &mut St, a: usize, b: usize) -> Vec<R> {
        match_rects(st, a, b).into_iter().map(|r| self.css((r.0 + dx(st, r.4), r.1, r.2, r.3))).collect()
    }

    fn sel_rects(&self, st: &mut St) -> Vec<R> {
        st.sel.map(|(a, b)| self.rects_of(st, a, b)).unwrap_or_default()
    }

    /// Shows diagram `i` over the view from CSS `top`, `height` tall, drawn again to fill it as GitHub's
    /// expanded view does: 16 px padding, the full width, centered. Returns the old box and the new one.
    pub fn expand(&self, i: usize, top: f64, height: f64) -> Vec<R> {
        let mut out = self.collapse();
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let Some(d) = st.page.diagrams.get(i) else { return out };
        let s = self.scale.get() as f32;
        let y = top as f32 * s;
        let h = (height as f32 * s).min(st.page.h - y).max(0.0);
        let pad = 16.0 * s;
        let (aw, ah) = (st.page.w - 2.0 * pad, h - 2.0 * pad);
        // The page's copy is at its zoom, so this undoes that to find the size at scale 1.
        let z = st.zoom.get(&i).copied().unwrap_or(1.0);
        let (w1, h1) = (d.bmp.w as f32 / (s * z), d.bmp.h as f32 / (s * z));
        let fit = (aw / w1).min(ah / h1).max(0.01);
        let Some(bmp) = crate::native::mermaid::render(&d.src, &st.th, aw / s, fit) else { return out };
        let (bw, bh) = (bmp.w as f32, bmp.h as f32);
        st.ov.expanded = Some(Expanded { y, h, x: (st.page.w - bw) / 2.0, top: y + (h - bh) / 2.0, bmp: std::rc::Rc::new(bmp) });
        out.push(self.css((0.0, y, st.page.w, h)));
        out
    }

    /// Closes the expanded diagram. Returns the box it covered.
    pub fn collapse(&self) -> Vec<R> {
        let mut st = self.st.borrow_mut();
        let Some(e) = st.ov.expanded.take() else { return vec![] };
        vec![self.css((0.0, e.y, st.page.w, e.h))]
    }

    fn btn_rect(&self, st: &St, b: usize) -> R {
        let s = self.scale.get() as f32;
        let b = &st.page.buttons[b];
        self.css((b.x - 4.0 * s, b.y - 4.0 * s, b.w + 8.0 * s, b.h + 8.0 * s))
    }

    /// Device pixels from CSS pixels.
    fn px(&self, v: f64) -> f32 {
        (v * self.scale.get()) as f32
    }

    pub fn layout(&self) -> Layout {
        let st = self.st.borrow();
        let p = &st.page;
        let s = self.scale.get();
        Layout {
            blocks: p.blocks.iter().map(|b| Block { start: b.start, end: b.end, top: f64::from(b.top) / s, height: f64::from(b.h) / s }).collect(),
            texts: p.blocks.iter().map(|b| p.text_str(b.tx.0, b.tx.1.min(b.tx.0 + 200))).collect(),
            headings: p.heads.iter().map(|h| (h.level, h.text.clone(), f64::from(h.y) / s)).collect(),
            height: f64::from(p.h) / s,
        }
    }

    pub fn search(&self, q: &str, keep: Option<usize>) -> Vec<Vec<R>> {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        st.matches = find_all(&st.page.text, q);
        st.cur = (!st.matches.is_empty()).then_some(keep.filter(|&k| k < st.matches.len()).unwrap_or(0));
        self.overlay(st);
        st.matches.clone().into_iter().map(|(a, b)| self.rects_of(st, a, b)).collect()
    }

    pub fn search_current(&self, i: usize) -> Vec<R> {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let mut out = st.cur.and_then(|c| st.matches.get(c).copied()).map(|(a, b)| self.rects_of(st, a, b)).unwrap_or_default();
        st.cur = Some(i);
        match self.reveal(st, i) {
            Some(region) => out.push(region),
            None => self.overlay(st),
        }
        out.extend(st.cur.and_then(|c| st.matches.get(c).copied()).map(|(a, b)| self.rects_of(st, a, b)).unwrap_or_default());
        out
    }

    /// Scrolls the wide block that holds match `i` sideways until the match shows. Returns the block's rect
    /// when it moved.
    fn reveal(&self, st: &mut St, i: usize) -> Option<R> {
        let (a, b) = st.matches.get(i).copied()?;
        let r = match_rects(st, a, b).into_iter().next()?;
        let k = usize::from(r.4).checked_sub(1)?;
        let (hs, off) = (&st.page.hscroll[k], st.ov.hoff[k]);
        let pad = 16.0 * self.scale.get() as f32;
        let (x0, x1) = (r.0 - hs.x, r.0 + r.2 - hs.x);
        let want = if x0 < off {
            x0 - pad
        } else if x1 > off + hs.w {
            x1 - hs.w + pad
        } else {
            return None;
        };
        self.shift(st, k, want - off).then(|| self.region(st, k))
    }

    pub fn search_clear(&self) {
        let mut st = self.st.borrow_mut();
        st.matches.clear();
        st.cur = None;
        self.overlay(&mut st);
    }

    pub fn hover(&self, x: f64, y: f64) -> (Pointer, Vec<R>) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let (x, y) = (self.px(x), self.px(y));
        let shape = pointer(st, x, y);
        let hb = st.page.buttons.iter().position(|b| x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h);
        let mut rects = vec![];
        if hb != st.ov.hover_btn {
            for b in [st.ov.hover_btn, hb].into_iter().flatten() {
                rects.push(self.btn_rect(st, b));
            }
            st.ov.hover_btn = hb;
        }
        (shape, rects)
    }

    pub fn link(&self, x: f64, y: f64) -> Option<(String, String)> {
        let st = self.st.borrow();
        let l = link_at(&st, self.px(x), self.px(y))?;
        let url = st.page.urls.get(l as usize - 1).cloned().unwrap_or_default();
        Some((url.clone(), url))
    }

    pub fn reset_copy(&self) -> Vec<R> {
        let mut st = self.st.borrow_mut();
        let Some(b) = st.ov.copied_btn.take() else { return vec![] };
        vec![self.btn_rect(&st, b)]
    }

    pub fn anchor_y(&self, id: &str) -> Option<f64> {
        let st = self.st.borrow();
        let id = id.to_lowercase();
        let y = st.page.anchors.get(&id).copied().or_else(|| st.page.anchors.get(&doc::slug(&id)).copied())?;
        Some(f64::from(y) / self.scale.get()).filter(|y| *y >= 0.0)
    }

    /// Changes the selection. Returns how many rects the old selection had, then its rects and the new one's.
    pub fn select(&self, sel: Select) -> (usize, Vec<R>) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let mut out = self.sel_rects(st);
        let n = out.len();
        st.sel = match sel {
            Select::Range(ax, ay, fx, fy) => {
                let (p, q) = (hit_char(st, self.px(ax), self.px(ay)), hit_char(st, self.px(fx), self.px(fy)));
                p.zip(q).map(|(p, q)| (p.min(q), p.max(q)))
            }
            Select::Word(x, y) => hit_char(st, self.px(x), self.px(y)).map(|i| word_at(&st.page.text, i)),
            Select::Clear => None,
        };
        self.overlay(st);
        out.extend(self.sel_rects(st));
        (n, out)
    }

    pub fn selection_text(&self) -> String {
        let st = self.st.borrow();
        st.sel.map(|(a, b)| st.page.text_str(a, b)).unwrap_or_default()
    }

    pub fn has_selection(&self) -> bool {
        self.st.borrow().sel.is_some_and(|(a, b)| a < b)
    }

    /// How many remote images are still loading.
    pub fn pending_images(&self) -> usize {
        self.st.borrow().pending
    }

    /// Lays out again as remote images arrive. True once none is loading.
    pub fn images_done(&self) -> bool {
        let mut st = self.st.borrow_mut();
        if st.images.poll() {
            self.relayout(&mut st);
        }
        st.pending = st.images.loading();
        st.pending == 0
    }

    pub fn code_grab(&self, x: f64, y: f64, slack: f64) -> Option<Vec<R>> {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let s = self.scale.get() as f32;
        st.held = None;
        let (x, y, slack) = (self.px(x), self.px(y), self.px(slack));
        let i = st.page.hscroll.iter().position(|h| h.max > 0.0 && x >= h.x && x <= h.x + h.w && y + slack >= h.bar_y && y - slack <= h.bar_y + 6.0 * s)?;
        st.held = Some(i);
        // A press on the thumb holds it where it is. A press on the track centers the thumb there first.
        let (left, width) = self.thumb(st, i);
        st.grip = if x >= left && x <= left + width { x - (left + width / 2.0) } else { 0.0 };
        self.bar_set(st, i, x - st.grip);
        Some(vec![self.region(st, i)])
    }

    pub fn code_drag(&self, x: f64) -> Vec<R> {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let Some(i) = st.held else { return vec![] };
        self.bar_set(st, i, self.px(x) - st.grip);
        vec![self.region(st, i)]
    }

    pub fn code_release(&self) {
        self.st.borrow_mut().held = None;
    }

    pub fn code_scroll(&self, x: f64, y: f64, d: f64) -> Vec<R> {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let (x, y, d) = (self.px(x), self.px(y), self.px(d));
        let hit = st.page.hscroll.iter().position(|h| h.max > 0.0 && x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h);
        hit.filter(|&i| self.shift(st, i, d)).map(|i| vec![self.region(st, i)]).unwrap_or_default()
    }

    /// Scrolls every wide block between `top` and `bottom` sideways by `d`.
    pub fn code_scroll_view(&self, top: f64, bottom: f64, d: f64) -> Vec<R> {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let (top, bottom, d) = (self.px(top), self.px(bottom), self.px(d));
        let mut out = vec![];
        for i in 0..st.page.hscroll.len() {
            let h = &st.page.hscroll[i];
            if h.y > bottom || h.y + h.h < top {
                continue;
            }
            if self.shift(st, i, d) {
                out.push(self.region(st, i));
            }
        }
        out
    }

    /// The wide blocks' sideways offsets, as JSON for `code_restore`.
    pub fn code_positions(&self) -> String {
        json!(self.st.borrow().ov.hoff).to_string()
    }

    pub fn code_restore(&self, saved: &str) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let saved: Vec<f32> = serde_json::from_str(saved).unwrap_or_default();
        for (i, v) in saved.into_iter().enumerate() {
            if let (Some(o), Some(h)) = (st.ov.hoff.get_mut(i), st.page.hscroll.get(i)) {
                *o = v.min(h.max);
            }
        }
        self.overlay(st);
    }

    pub fn set_wrap(&self, wrap: bool) {
        let mut st = self.st.borrow_mut();
        st.wrap = wrap;
        self.relayout(&mut st);
    }

    pub fn press(&self, x: f64, y: f64) -> Press {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let (x, y) = (self.px(x), self.px(y));
        if let Some(b) = st.page.buttons.iter().position(|b| x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h) {
            let (kind, data) = (st.page.buttons[b].kind, st.page.buttons[b].data);
            return match kind {
                BtnKind::CopyCode | BtnKind::CopyDiagram => {
                    let mut rects = vec![];
                    if let Some(old) = st.ov.copied_btn.replace(b) {
                        rects.push(self.btn_rect(st, old));
                    }
                    rects.push(self.btn_rect(st, b));
                    let label = if kind == BtnKind::CopyCode { "the code" } else { "the diagram source" };
                    Press::Copy { text: st.page.copies.get(data).cloned().unwrap_or_default(), rects, label: label.into() }
                }
                BtnKind::Expand => Press::Expand(data),
                BtnKind::ZoomIn => {
                    let z = st.zoom.get(&data).copied().unwrap_or(1.0);
                    st.zoom.insert(data, (z * 1.25).min(4.0));
                    self.relayout(st);
                    Press::Other
                }
                BtnKind::ZoomReset => {
                    st.zoom.remove(&data);
                    self.relayout(st);
                    Press::Other
                }
            };
        }
        if let Some(id) = st.page.toggles.iter().find(|t| x >= t.0 && x <= t.0 + t.2 && y >= t.1 && y <= t.1 + t.3).map(|t| t.4) {
            if !st.toggled.remove(&id) {
                st.toggled.insert(id);
            }
            self.relayout(st);
            return Press::Other;
        }
        Press::Nothing
    }

    fn region(&self, st: &St, i: usize) -> R {
        let h = &st.page.hscroll[i];
        self.css((h.x, h.y, h.w, (h.bar_y + 8.0 * self.scale.get() as f32 - h.y).max(h.h)))
    }

    fn shift(&self, st: &mut St, i: usize, d: f32) -> bool {
        let max = st.page.hscroll[i].max;
        let n = (st.ov.hoff[i] + d).clamp(0.0, max);
        let moved = (n - st.ov.hoff[i]).abs() > 0.01;
        st.ov.hoff[i] = n;
        if moved {
            self.overlay(st);
        }
        moved
    }

    /// The track of block `i`'s scroll bar, its left end and width, and the thumb's width, in page pixels.
    fn track(&self, st: &St, i: usize) -> (f32, f32, f32) {
        let s = self.scale.get() as f32;
        let hs = &st.page.hscroll[i];
        let tw = hs.w - 8.0 * s;
        (hs.x + 4.0 * s, tw, (tw * hs.w / (hs.w + hs.max)).max(16.0 * s))
    }

    /// The thumb's left end and width, in page pixels.
    fn thumb(&self, st: &St, i: usize) -> (f32, f32) {
        let (tx, tw, thumb) = self.track(st, i);
        let max = st.page.hscroll[i].max;
        let r = if max > 0.0 { st.ov.hoff[i] / max } else { 0.0 };
        (tx + r * (tw - thumb), thumb)
    }

    /// Puts the thumb's center at `x`.
    fn bar_set(&self, st: &mut St, i: usize, x: f32) {
        let (tx, tw, thumb) = self.track(st, i);
        let r = ((x - tx - thumb / 2.0) / (tw - thumb).max(1.0)).clamp(0.0, 1.0);
        st.ov.hoff[i] = r * st.page.hscroll[i].max;
        self.overlay(st);
    }
}

fn dx(st: &St, clip: u16) -> f32 {
    if clip > 0 { -st.ov.hoff.get(clip as usize - 1).copied().unwrap_or(0.0) } else { 0.0 }
}

fn in_clip(st: &St, clip: u16, x: f32, y: f32) -> bool {
    clip == 0 || {
        let hs = &st.page.hscroll[clip as usize - 1];
        x >= hs.x && x <= hs.x + hs.w && y >= hs.y && y <= hs.y + hs.h
    }
}

fn link_at(st: &St, x: f32, y: f32) -> Option<u32> {
    st.page
        .links
        .iter()
        .find(|l| {
            let d = dx(st, l.clip);
            x >= l.x + d && x <= l.x + d + l.w && y >= l.y && y <= l.y + l.h && in_clip(st, l.clip, x, y)
        })
        .map(|l| l.link)
}

fn pointer(st: &St, x: f32, y: f32) -> Pointer {
    let p = &st.page;
    let on = |bx: f32, by: f32, bw: f32, bh: f32| x >= bx && x <= bx + bw && y >= by && y <= by + bh;
    if p.buttons.iter().any(|b| on(b.x, b.y, b.w, b.h)) || link_at(st, x, y).is_some() || p.toggles.iter().any(|t| on(t.0, t.1, t.2, t.3)) {
        return Pointer::Hand;
    }
    for &(_, _, it) in &st.runs {
        let item = &p.items[it as usize];
        let d = dx(st, item.clip);
        if on(item.x + d, item.y, item.w, item.h) && in_clip(st, item.clip, x, y) {
            return Pointer::Text;
        }
    }
    Pointer::Default
}

fn build_runs(page: &Page) -> Vec<(usize, usize, u32)> {
    let mut v = vec![];
    for (i, it) in page.items.iter().enumerate() {
        if let Kind::Text { s, tx, .. } = &it.k
            && *tx != usize::MAX
        {
            v.push((*tx, s.chars().count(), i as u32));
        }
    }
    v.sort_by_key(|r| r.0);
    v
}

fn match_rects(st: &mut St, a: usize, b: usize) -> Vec<(f32, f32, f32, f32, u16)> {
    let mut out = vec![];
    let start = st.runs.partition_point(|r| r.0 + r.1 <= a);
    for ri in start..st.runs.len() {
        let (tx, len, it) = st.runs[ri];
        if tx >= b {
            break;
        }
        let item = &st.page.items[it as usize];
        let Kind::Text { s, f, .. } = &item.k else { continue };
        let (ls, le) = (a.max(tx) - tx, b.min(tx + len) - tx);
        if ls >= le {
            continue;
        }
        let x0 = st.fonts.char_x(*f, s, ls);
        let x1 = st.fonts.char_x(*f, s, le);
        out.push((item.x + x0, item.y, x1 - x0, item.h, item.clip));
    }
    out
}

fn hit_char(st: &mut St, px: f32, py: f32) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (ri, &(_, _, it)) in st.runs.iter().enumerate() {
        let item = &st.page.items[it as usize];
        let x = item.x + dx(st, item.clip);
        // Rows first, then the nearest run along the row, so a point past a line's end lands at that end.
        let across = if px < x {
            x - px
        } else if px > x + item.w {
            px - x - item.w
        } else {
            0.0
        } * 0.001;
        let d = if py >= item.y && py <= item.y + item.h { across } else { (py - (item.y + item.h / 2.0)).abs() + across };
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, ri));
        }
    }
    let (_, ri) = best?;
    let (tx, len, it) = st.runs[ri];
    let d = dx(st, st.page.items[it as usize].clip);
    let item = &st.page.items[it as usize];
    let Kind::Text { s, f, .. } = &item.k else { return None };
    let (s, f, rel) = (s.clone(), *f, px - item.x - d);
    let mut idx = 0;
    for i in 0..len {
        let a = st.fonts.char_x(f, &s, i);
        let b = st.fonts.char_x(f, &s, i + 1);
        if rel < f32::midpoint(a, b) {
            break;
        }
        idx = i + 1;
    }
    Some(tx + idx)
}

fn word_at(t: &[char], i: usize) -> (usize, usize) {
    if t.is_empty() {
        return (0, 0);
    }
    let i = i.min(t.len() - 1);
    let w = |c: char| c.is_alphanumeric() || c == '_';
    if !w(t[i]) {
        return (i, i + 1);
    }
    let (mut a, mut b) = (i, i);
    while a > 0 && w(t[a - 1]) {
        a -= 1;
    }
    while b < t.len() && w(t[b]) {
        b += 1;
    }
    (a, b)
}

/// Smart case: a query with no capitals ignores case. Whitespace in the query matches any run of whitespace.
fn find_all(t: &[char], q: &str) -> Vec<(usize, usize)> {
    let qc: Vec<char> = q.chars().collect();
    let ci = !q.chars().any(char::is_uppercase);
    let eq = |a: char, b: char| if ci { a == b || a.to_lowercase().eq(b.to_lowercase()) } else { a == b };
    let mut out = vec![];
    if qc.iter().all(|c| c.is_whitespace()) {
        return out;
    }
    let mut i = 0;
    while i < t.len() {
        let (mut j, mut qi, mut ok) = (i, 0, true);
        while qi < qc.len() {
            if qc[qi].is_whitespace() {
                // A space in the query spans spaces and soft breaks, not a block or table cell boundary.
                let gap = |c: char| c.is_whitespace() && c != '\n' && c != '\t';
                if j >= t.len() || !gap(t[j]) {
                    ok = false;
                    break;
                }
                while j < t.len() && gap(t[j]) {
                    j += 1;
                }
                while qi < qc.len() && qc[qi].is_whitespace() {
                    qi += 1;
                }
            } else {
                if j >= t.len() || !eq(t[j], qc[qi]) {
                    ok = false;
                    break;
                }
                j += 1;
                qi += 1;
            }
        }
        if ok && j > i {
            out.push((i, j));
            i = j;
        } else {
            i += 1;
        }
    }
    out
}
