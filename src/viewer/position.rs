use similar::{ChangeTag, TextDiff};

use crate::page::script::Block;

pub fn top_text(blocks: &[Block], texts: &[String], y: f64) -> String {
    match capture(blocks, y) {
        Some(a) => {
            let i = blocks.iter().position(|b| b.start == a.line).unwrap_or(0);
            format!("{:?}@{:.1}", texts.get(i).map_or("", String::as_str), a.offset)
        }
        None => String::new(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub line: u32,
    pub offset: f64,
}

pub fn capture(blocks: &[Block], y: f64) -> Option<Anchor> {
    let b = blocks.iter().filter(|b| b.start > 0).find(|b| y < b.top + b.height).or_else(|| blocks.iter().rfind(|b| b.start > 0))?;
    Some(Anchor { line: b.start, offset: y - b.top })
}

pub fn map_line(old: &str, new: &str, line: u32) -> u32 {
    let diff = TextDiff::from_lines(old, new);
    let (mut o, mut n) = (1u32, 1u32);
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                if o == line {
                    return n;
                }
                o += 1;
                n += 1;
            }
            ChangeTag::Delete => {
                if o == line {
                    return n;
                }
                o += 1;
            }
            ChangeTag::Insert => n += 1,
        }
    }
    n
}

pub fn restore(blocks: &[Block], anchor: Anchor) -> Option<f64> {
    let b = blocks
        .iter()
        .filter(|b| b.start > 0)
        .find(|b| anchor.line >= b.start && anchor.line <= b.end)
        .or_else(|| blocks.iter().find(|b| b.start >= anchor.line))
        .or_else(|| blocks.iter().rfind(|b| b.start > 0))?;
    Some((b.top + anchor.offset.min(b.height.max(0.0))).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks() -> Vec<Block> {
        vec![Block { start: 1, end: 1, top: 0.0, height: 40.0 }, Block { start: 3, end: 5, top: 56.0, height: 72.0 }, Block { start: 7, end: 7, top: 144.0, height: 24.0 }]
    }

    #[test]
    fn captures_block_and_offset() {
        assert_eq!(capture(&blocks(), 60.0), Some(Anchor { line: 3, offset: 4.0 }));
        assert_eq!(capture(&blocks(), 50.0), Some(Anchor { line: 3, offset: -6.0 }));
    }

    #[test]
    fn line_moves_with_insertions_above() {
        let old = "a\nb\nc\n";
        let new = "x\ny\na\nb\nc\n";
        assert_eq!(map_line(old, new, 3), 5);
        assert_eq!(map_line(old, "b\nc\n", 3), 2);
    }

    #[test]
    fn d3_keep_position() {
        let old_blocks = blocks();
        let anchor = capture(&old_blocks, 70.0).unwrap();
        let old = "# T\n\npara\npara\npara\n\nend\n";
        let new = format!("{}{}", "added\n\n".repeat(20), old);
        let line = map_line(old, &new, anchor.line);
        let shifted: Vec<Block> = old_blocks.iter().map(|b| Block { start: b.start + 40, end: b.end + 40, top: b.top + 900.0, ..*b }).collect();
        let y = restore(&shifted, Anchor { line, offset: anchor.offset }).unwrap();
        assert_eq!(y, 970.0);
    }

    #[test]
    fn restore_falls_back_to_a_neighbouring_block() {
        let at = |line, offset| restore(&blocks(), Anchor { line, offset });
        assert_eq!(at(3, 4.0), Some(60.0));
        assert_eq!(at(2, 4.0), Some(60.0));
        assert_eq!(at(6, 0.0), Some(144.0));
        assert_eq!(at(9, 5.0), Some(149.0));
    }

    #[test]
    fn restore_clamps_offset_to_the_block() {
        let b = blocks();
        assert_eq!(restore(&b, Anchor { line: 3, offset: 500.0 }), Some(128.0));
        assert_eq!(restore(&b, Anchor { line: 3, offset: -100.0 }), Some(0.0));
    }

    #[test]
    fn restore_needs_a_block_with_a_source_line() {
        let b = vec![Block { start: 0, end: 0, top: 0.0, height: 10.0 }, Block { start: 0, end: 0, top: 10.0, height: 10.0 }];
        assert_eq!(restore(&b, Anchor { line: 3, offset: 1.0 }), None);
        assert_eq!(restore(&[], Anchor { line: 1, offset: 0.0 }), None);
    }

    #[test]
    fn line_in_a_truncated_file_lands_at_the_end() {
        let old = "a\nb\nc\nd\n";
        assert_eq!(map_line(old, "a\nb\n", 1), 1);
        assert_eq!(map_line(old, "a\nb\n", 2), 2);
        assert_eq!(map_line(old, "a\nb\n", 4), 3);
        assert_eq!(map_line(old, "a\nb\n", 40), 3);
        assert_eq!(map_line(old, "", 3), 1);
    }
}
