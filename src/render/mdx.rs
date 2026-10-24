use markdown::mdast::Node;
use markdown::{MdxSignal, ParseOptions, to_mdast};

pub fn to_markdown(src: &str) -> String {
    rewrite(src).unwrap_or_else(|| src.to_string())
}

fn options() -> ParseOptions {
    let mut o = ParseOptions::mdx();
    o.constructs.frontmatter = true;
    o.constructs.gfm_autolink_literal = true;
    o.constructs.gfm_footnote_definition = true;
    o.constructs.gfm_label_start_footnote = true;
    o.constructs.gfm_strikethrough = true;
    o.constructs.gfm_table = true;
    o.constructs.gfm_task_list_item = true;
    o.constructs.math_flow = true;
    o.constructs.math_text = true;
    o.mdx_esm_parse = Some(Box::new(|_| MdxSignal::Ok));
    o
}

fn rewrite(src: &str) -> Option<String> {
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for (start, end, text) in edits(src)? {
        out.push_str(&src[last..start]);
        out.push_str(&text);
        last = end;
    }
    out.push_str(&src[last..]);
    Some(out)
}

fn edits(src: &str) -> Option<Vec<(usize, usize, String)>> {
    let tree = to_mdast(src, &options()).ok()?;
    let mut edits = Vec::new();
    collect(&tree, src, &mut edits, 0);
    edits.sort_by_key(|e: &(usize, usize, String)| e.0);
    let mut last = 0;
    edits.retain(|(start, end, _)| {
        let keep = *start >= last;
        if keep {
            last = *end;
        }
        keep
    });
    Some(edits)
}

fn span(node: &Node) -> Option<(usize, usize)> {
    node.position().map(|p| (p.start.offset, p.end.offset))
}

fn blank(src: &str) -> String {
    src.chars().filter(|c| *c == '\n').collect()
}

fn intrinsic(name: Option<&String>) -> bool {
    name.is_some_and(|n| n.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && !n.contains(['.', ':']))
}

fn collect(node: &Node, src: &str, edits: &mut Vec<(usize, usize, String)>, depth: usize) {
    match node {
        Node::MdxjsEsm(_) | Node::MdxFlowExpression(_) => {
            if let Some((s, e)) = span(node) {
                edits.push((s, e, blank(&src[s..e])));
            }
        }
        Node::MdxTextExpression(_) => {
            if let Some((s, e)) = span(node) {
                edits.push((s, e, String::new()));
            }
        }
        Node::MdxJsxFlowElement(el) if !intrinsic(el.name.as_ref()) => {
            if let Some((s, e)) = span(node) {
                edits.push((s, e, fence(src, s, e)));
            }
        }
        Node::MdxJsxTextElement(el) if !intrinsic(el.name.as_ref()) => {
            if let Some((s, e)) = span(node) {
                edits.push((s, e, format!("<code>{}</code>", super::markdown::escape(&collapse(&src[s..e])))));
            }
        }
        _ if depth < super::dom::MAX_DEPTH => {
            if let Some(children) = node.children() {
                for c in children {
                    collect(c, src, edits, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fence(src: &str, s: usize, e: usize) -> String {
    let el = &src[s..e];
    let ticks = el.split(|c| c != '`').map(str::len).max().unwrap_or(0).max(2) + 1;
    let fence = "`".repeat(ticks);
    let prefix: String = src[src[..s].rfind('\n').map_or(0, |i| i + 1)..s].chars().map(|c| if c == '>' { '>' } else { ' ' }).collect();
    format!("{fence}jsx\n{prefix}{el}\n{prefix}{fence}")
}

#[cfg(test)]
mod tests {
    use super::to_markdown;

    #[test]
    fn hides_esm_and_comments_and_fences_components() {
        let src = "import { A } from 'a';\n\n# Hi\n\n{/* note */}\n\n<Callout type=\"warn\">\n\nBody **bold**.\n\n</Callout>\n\n<Key combo={[['Ctrl', 'e']]} />\n\nPress <Kbd>q</Kbd> now.\n";
        let out = to_markdown(src);
        assert!(!out.contains("import"));
        assert!(!out.contains("note"));
        assert!(out.contains("```jsx\n<Callout type=\"warn\">\n\nBody **bold**.\n\n</Callout>\n```"));
        assert!(out.contains("```jsx\n<Key combo={[['Ctrl', 'e']]} />\n```"));
        assert!(out.contains("Press <code>&lt;Kbd&gt;q&lt;/Kbd&gt;</code> now."));
    }

    #[test]
    fn fence_stays_inside_lists_and_quotes_and_outlasts_backticks() {
        let out = to_markdown("- item\n\n  <Note>\n    hi\n  </Note>\n\n> <Note>\n> ```\n> x\n> ```\n> </Note>\n");
        assert!(out.contains("  ```jsx\n  <Note>\n    hi\n  </Note>\n  ```"), "{out}");
        assert!(out.contains("> ````jsx\n> <Note>\n> ```\n> x\n> ```\n> </Note>\n> ````"), "{out}");
    }

    #[test]
    fn esm_with_regex_and_comments_keeps_the_markdown() {
        for esm in ["export const quote = /\"/", "export const x = /* { */ 1", "export function f(s) { return /'/.test(s) }", "export const half = 1 / 2"] {
            let out = to_markdown(&format!("{esm}\n\n# Title\n\nText with \"quotes\" and 'apostrophes'.\n"));
            assert!(!out.contains("export"), "{esm}: {out}");
            assert!(out.contains("# Title") && out.contains("Text with \"quotes\" and 'apostrophes'."), "{esm}: {out}");
        }
    }

    #[test]
    fn invalid_mdx_falls_back_to_source() {
        let src = "a < b and <Unclosed\n";
        assert_eq!(to_markdown(src), src);
    }

    #[test]
    fn deep_nesting_converts_on_a_small_stack() {
        let src = format!("{}x\n", "> ".repeat(10_000));
        let out = std::thread::Builder::new().stack_size(8 << 20).spawn(move || to_markdown(&src)).unwrap().join().unwrap();
        assert!(out.ends_with("x\n"));
    }
}
