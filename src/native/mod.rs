//! The native renderer taken from the blind rebuild: Markdown to a display list, drawn with CoreText and
//! CoreGraphics. Spike code, kept close to its source.
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction, dead_code, unused)]

pub mod doc;
pub mod highlight;
pub mod images;
pub mod layout;
pub mod math;
pub mod mermaid;
pub mod page;
pub mod raster;
pub mod text;
pub mod theme;
