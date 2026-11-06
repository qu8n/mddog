//! The laid-out page: a display list in device pixels plus the data the viewer needs.
use crate::render::text::FKey;
use crate::render::theme::Rgb;
use std::collections::HashMap;
use std::rc::Rc;

/// Premultiplied RGBA pixels.
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// A text item's `tx` when it is not part of the page's text, such as a list marker: search and selection skip it.
pub const NO_TEXT: usize = usize::MAX;

pub enum Kind {
    /// `fill` is 0xAARRGGBB.
    Rect {
        fill: u32,
        stroke: Option<(f32, u32)>,
        radius: f32,
    },
    Text {
        s: String,
        f: FKey,
        color: Rgb,
        under: bool,
        strike: bool,
        asc: f32,
        tx: usize,
    },
    Img(Rc<Bitmap>),
    /// Open or closed polyline / polygon in item-relative coordinates.
    Poly {
        pts: Vec<(f32, f32)>,
        color: u32,
        width: f32,
        fill: bool,
    },
}

pub struct Item {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// 0 = fixed, else index into `Page::hscroll` plus one.
    pub clip: u16,
    pub k: Kind,
}

pub struct LinkRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub clip: u16,
    pub link: u32,
}

pub struct Head {
    pub level: u8,
    pub text: String,
    pub y: f32,
}

pub struct HScroll {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub max: f32,
    /// The scroll bar's top, in device px from the page's top.
    pub bar_y: f32,
}

impl HScroll {
    /// The scroll bar's height, in CSS px.
    pub const BAR: f32 = 6.0;

    /// The track's left edge and width, and the thumb's width, at `s` device px per CSS px. The track sits
    /// 4 px in from each side, and the thumb is at least 16 px wide.
    pub fn track(&self, s: f32) -> (f32, f32, f32) {
        let tw = self.w - 8.0 * s;
        (self.x + 4.0 * s, tw, (tw * self.w / (self.w + self.max)).max(16.0 * s))
    }

    /// The thumb's left edge and width at offset `off`.
    pub fn thumb(&self, off: f32, s: f32) -> (f32, f32) {
        let (tx, tw, thumb) = self.track(s);
        let r = if self.max > 0.0 { (off / self.max).clamp(0.0, 1.0) } else { 0.0 };
        (tx + r * (tw - thumb), thumb)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtnKind {
    CopyCode,
    CopyDiagram,
    Expand,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

pub struct Button {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub kind: BtnKind,
    /// Index into `Page::copies` or `Page::diagrams`.
    pub data: usize,
    /// Drawn dimmed, and takes no clicks.
    pub off: bool,
}

impl Button {
    pub fn new(x: f32, y: f32, w: f32, h: f32, kind: BtnKind, data: usize) -> Button {
        Button { x, y, w, h, kind, data, off: false }
    }

    /// Whether a click at the point lands on it. A hidden button has no width.
    pub fn hit(&self, x: f32, y: f32) -> bool {
        self.w > 0.0 && !self.off && x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

pub struct Diagram {
    pub tree: Rc<resvg::usvg::Tree>,
    /// The display item that draws it.
    pub item: usize,
    /// The item's place and picture at the fit size, to put back on reset.
    pub rect: (f32, f32, f32, f32),
    pub bmp: Rc<Bitmap>,
    /// Device pixels per SVG unit at the fit size.
    pub fit: f32,
    /// Set when the diagram was shrunk to fit, so it zooms.
    pub zoom: Option<ZoomBar>,
}

impl Diagram {
    /// The most a zoom goes to, in device px per SVG unit: three CSS px, or the fit size when larger.
    pub fn max_zoom(&self, scale: f32) -> f32 {
        self.fit.max(3.0 * scale)
    }
}

/// A shrunk diagram's frame, which a zoomed view fills, and its zoom bar (device px).
pub struct ZoomBar {
    pub frame: (f32, f32, f32, f32),
    /// The bar's box item, and its right end.
    pub item: usize,
    pub right: f32,
    /// The zoom in, zoom out and reset buttons.
    pub buttons: [usize; 3],
    /// Device px per CSS px, as laid out.
    pub scale: f32,
}

/// A top-level block: its 1-based source lines, where it sits (device px), and its span of `Page::text`.
pub struct SrcBlock {
    pub start: u32,
    pub end: u32,
    pub top: f32,
    pub h: f32,
    pub tx: (usize, usize),
}

#[derive(Default)]
pub struct Page {
    pub blocks: Vec<SrcBlock>,
    pub w: f32,
    pub h: f32,
    pub items: Vec<Item>,
    pub text: Vec<char>,
    pub urls: Vec<String>,
    pub links: Vec<LinkRect>,
    pub anchors: HashMap<String, f32>,
    pub heads: Vec<Head>,
    pub hscroll: Vec<HScroll>,
    pub buttons: Vec<Button>,
    pub copies: Vec<String>,
    pub diagrams: Vec<Diagram>,
    pub toggles: Vec<(f32, f32, f32, f32, usize)>,
    /// Pending remote images: layout asks, the viewer fetches.
    pub wanted: Vec<String>,
}

impl Page {
    pub fn text_str(&self, a: usize, b: usize) -> String {
        self.text[a.min(self.text.len())..b.min(self.text.len())].iter().collect()
    }
}
