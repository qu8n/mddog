use kuchikiki::{ElementData, NodeRef};

use super::dom::unwrap;

const ALLOWED: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "br",
    "b",
    "i",
    "strong",
    "em",
    "a",
    "pre",
    "code",
    "img",
    "tt",
    "div",
    "ins",
    "del",
    "sup",
    "sub",
    "p",
    "picture",
    "source",
    "video",
    "ol",
    "ul",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "blockquote",
    "dl",
    "dt",
    "dd",
    "kbd",
    "q",
    "samp",
    "var",
    "hr",
    "ruby",
    "rt",
    "rp",
    "li",
    "tr",
    "td",
    "th",
    "s",
    "strike",
    "summary",
    "details",
    "caption",
    "mark",
    "span",
    "section",
    "mddog-frontmatter",
];

const DROP_WITH_CONTENT: &[&str] = &["script", "svg", "math", "input", "select", "textarea", "button", "object", "embed", "template", "head", "style"];

const ALL_ATTRS: &[&str] = &[
    "abbr",
    "accept",
    "accept-charset",
    "accesskey",
    "action",
    "align",
    "alt",
    "aria-describedby",
    "aria-hidden",
    "aria-label",
    "aria-labelledby",
    "axis",
    "border",
    "cellpadding",
    "cellspacing",
    "char",
    "charoff",
    "charset",
    "checked",
    "clear",
    "cols",
    "colspan",
    "color",
    "compact",
    "coords",
    "datetime",
    "dir",
    "disabled",
    "enctype",
    "for",
    "frame",
    "headers",
    "height",
    "hreflang",
    "hspace",
    "ismap",
    "label",
    "lang",
    "maxlength",
    "media",
    "method",
    "multiple",
    "name",
    "nohref",
    "noshade",
    "nowrap",
    "open",
    "progress",
    "prompt",
    "readonly",
    "rel",
    "rev",
    "role",
    "rows",
    "rowspan",
    "rules",
    "scope",
    "selected",
    "shape",
    "size",
    "span",
    "start",
    "summary",
    "tabindex",
    "target",
    "title",
    "type",
    "usemap",
    "valign",
    "value",
    "vspace",
    "width",
    "itemprop",
    "id",
    "data-sourcepos",
];

fn tag_attrs(tag: &str) -> &'static [&'static str] {
    match tag {
        "a" => &["href", "data-footnote-ref", "data-footnote-backref", "data-footnote-backref-idx", "class"],
        "img" => &["src", "longdesc"],
        "div" => &["itemscope", "itemtype"],
        "blockquote" | "del" | "ins" | "q" => &["cite"],
        "source" => &["srcset", "media", "type"],
        "video" => &["src", "poster", "controls", "muted", "playsinline"],
        "pre" => &["lang"],
        "section" => &["class", "data-footnotes"],
        "sup" => &["class"],
        "li" => &["class"],
        _ => &[],
    }
}

fn url_ok(v: &str, schemes: &[&str]) -> bool {
    let t = v.trim();
    match t.find(':') {
        Some(i) if !t[..i].contains(['/', '?', '#']) => schemes.iter().any(|s| t[..i].eq_ignore_ascii_case(s)),
        _ => true,
    }
}

fn srcset_ok(v: &str) -> bool {
    v.split(',').all(|c| c.split_whitespace().next().is_none_or(|u| url_ok(u, &["http", "https"])))
}

pub fn clean(root: &NodeRef) {
    for node in root.descendants().collect::<Vec<_>>() {
        if node.parent().is_none() {
            continue;
        }
        if node.as_comment().is_some() || node.as_doctype().is_some() {
            node.detach();
            continue;
        }
        let Some(el) = node.as_element() else { continue };
        let tag: &str = &el.name.local;
        if tag == "input" && is_task_box(&node, el) {
            let mut a = el.attributes.borrow_mut();
            a.map.retain(|k, _| matches!(&*k.local, "type" | "checked" | "disabled"));
            continue;
        }
        if DROP_WITH_CONTENT.contains(&tag) {
            node.detach();
            continue;
        }
        if !ALLOWED.contains(&tag) || from_markdown_only(tag, el) {
            unwrap(&node);
            continue;
        }
        let mut a = el.attributes.borrow_mut();
        let extra = tag_attrs(tag);
        a.map.retain(|k, v| {
            let k = &*k.local;
            if !(ALL_ATTRS.contains(&k) || extra.contains(&k)) {
                return false;
            }
            match (tag, k) {
                ("a", "href") => url_ok(&v.value, &["http", "https", "mailto", "github-windows", "github-mac", "x-github-client"]),
                ("img", "src" | "longdesc") | (_, "cite") => url_ok(&v.value, &["http", "https"]),
                ("video", "src" | "poster") => url_ok(&v.value, &["http", "https"]),
                ("source", "srcset") => srcset_ok(&v.value),
                ("section", "class") => v.value == "footnotes",
                ("sup", "class") => v.value == "footnote-ref",
                ("li", "class") => false,
                ("a", "class") => v.value == "footnote-backref",
                _ => true,
            }
        });
        let in_sup = node.parent().and_then(|p| p.as_element().map(|e| &*e.name.local == "sup")).unwrap_or(false);
        let footnote = (a.contains("data-footnote-ref") && in_sup) || ((a.contains("data-footnote-backref") || tag == "li") && in_footnotes(&node));
        for k in ["id", "name"] {
            if let Some(v) = a.get(k).map(str::to_string)
                && !footnote
            {
                a.insert(k, format!("user-content-{v}"));
            }
        }
        if tag == "a" && !a.contains("href") && !a.contains("name") && !a.contains("id") {
            drop(a);
            unwrap(&node);
        }
    }
}

fn is_task_box(node: &NodeRef, el: &ElementData) -> bool {
    let attrs = el.attributes.borrow();
    attrs.get("type") == Some("checkbox") && attrs.contains("disabled") && node.parent().is_some_and(|p| matches!(p.as_element().map(|e| &*e.name.local), Some("li" | "p")))
}

fn in_footnotes(node: &NodeRef) -> bool {
    node.ancestors().any(|a| a.as_element().is_some_and(|e| &*e.name.local == "section" && e.attributes.borrow().get("class") == Some("footnotes")))
}

fn from_markdown_only(tag: &str, el: &ElementData) -> bool {
    tag == "section" && el.attributes.borrow().get("class") != Some("footnotes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::dom::{inner_html, parse_body};

    fn check(cases: &[(&str, &[&str], &[&str])]) {
        for (input, want, deny) in cases {
            let body = parse_body(input);
            clean(&body);
            let out = inner_html(&body);
            for w in *want {
                assert!(out.contains(w), "{input}: expected {w:?} in {out:?}");
            }
            for d in *deny {
                assert!(!out.contains(d), "{input}: unexpected {d:?} in {out:?}");
            }
        }
    }

    #[test]
    fn keeps_and_drops() {
        check(&[
            (r#"<a href="javascript:alert(1)">x</a>"#, &["x"], &["href", "<a"]),
            (r#"<a href="JaVaScRiPt:x">y</a>"#, &["y"], &["href"]),
            (r#"<a href="java&#9;script:x">z</a>"#, &["z"], &["href"]),
            (r#"<a href="https://e.com/">l</a>"#, &[r#"href="https://e.com/""#], &[]),
            (r#"<a href="docs/a.md">r</a>"#, &[r#"href="docs/a.md""#], &[]),
            (r#"<img src="data:image/png;base64,AA">"#, &[], &["data:"]),
            (r#"<img src="x.png" onerror="f()">"#, &[r#"src="x.png""#], &["onerror"]),
            ("<script>bad()</script>ok", &["ok"], &["bad"]),
            (r##"<svg><a href="#">s</a></svg>t"##, &["t"], &["<svg", "s</a>"]),
            ("<style>p{}</style>u", &["u"], &["<style"]),
            ("<center>c</center>", &["c"], &["<center"]),
            (r#"<div id="a" style="color:red">d</div>"#, &[r#"id="user-content-a""#], &["style"]),
            (r#"<section class="x">q</section>"#, &["q"], &["<section"]),
            (r##"<a data-footnote-ref id="x" name="y" href="#">t</a>"##, &[r#"id="user-content-x""#, r#"name="user-content-y""#], &[]),
            (r#"<ul><li><input type="checkbox" disabled checked onclick="f()"> t</li></ul>"#, &[r#"type="checkbox""#], &["onclick"]),
        ]);
    }

    #[test]
    fn media_urls_need_web_schemes() {
        check(&[
            (r#"<video src="file:///etc/hosts" poster="data:image/png;base64,AA"></video>"#, &["<video"], &["file:", "data:"]),
            (r#"<video src="https://e.com/v.mp4" poster="p.png"></video>"#, &[r#"src="https://e.com/v.mp4""#, r#"poster="p.png""#], &[]),
            (r#"<picture><source srcset="https://e.com/a.png 1x, file:///b.png 2x"><img src="a.png"></picture>"#, &["<source"], &["srcset"]),
            (r#"<picture><source srcset="a.png 1x, https://e.com/b.png 2x"><img src="a.png"></picture>"#, &[r#"srcset="a.png 1x, https://e.com/b.png 2x""#], &[]),
        ]);
    }
}
