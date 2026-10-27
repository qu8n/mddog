pub use markdown::{Doc, render};

/// The stack size of the render threads: the viewer's worker, headless mode's, and the spare's.
pub const STACK: usize = 64 << 20;

pub(crate) mod dom;
pub(crate) mod emoji;
pub(crate) mod highlight;
pub(crate) mod markdown;
pub(crate) mod octicons;
pub(crate) mod sanitize;
pub(crate) mod textmate;
