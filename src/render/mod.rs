/// The stack size of the render threads: the viewer's file reader, and the highlighter's.
pub const STACK: usize = 64 << 20;

pub(crate) mod highlight;
pub(crate) mod markdown;
pub(crate) mod textmate;

/// Escapes text for the highlighter's HTML spans, which `native::highlight` reads back.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
