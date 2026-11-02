//! The laid-out page: a display list in device pixels plus the data the viewer needs.
use crate::native::text::FKey;
use crate::native::theme::Rgb;
use std::collections::HashMap;
use std::rc::Rc;

/// Premultiplied RGBA pixels.
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

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
    /// Largest offset.
    pub max: f32,
    /// Bar track rect (relative to page).
    pub bar_y: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BtnKind {
    CopyCode,
    CopyDiagram,
    Expand,
    ZoomIn,
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
}

pub struct Diagram {
    pub bmp: Rc<Bitmap>,
    /// The Mermaid source, to draw the diagram again at the expanded size.
    pub src: String,
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
