/// The stack size of the render threads: the viewer's file reader, and the highlighter's.
pub const STACK: usize = 64 << 20;

pub(crate) mod highlight;
pub(crate) mod markdown;
pub(crate) mod textmate;
