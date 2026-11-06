//! The renderer: Markdown to a display list, drawn with CoreText and CoreGraphics.

pub mod doc;
pub mod engine;
pub mod highlight;
pub mod icons;
pub mod images;
pub mod layout;
pub mod math;
pub mod media;
pub mod mermaid;
pub mod page;
pub mod raster;
pub mod remote;
pub mod svg;
pub mod text;
pub mod theme;

/// The stack size of the render threads: the viewer's file reader, and the highlighter's.
pub const STACK: usize = 64 << 20;
