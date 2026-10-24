use html5ever::{LocalName, QualName, ns};
use kuchikiki::iter::NodeEdge;
use kuchikiki::traits::*;
use kuchikiki::{Attribute, ExpandedName, NodeRef};

pub const MAX_DEPTH: usize = 512;

pub fn qn(name: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(name))
}

pub fn el(name: &str, attrs: &[(&str, &str)]) -> NodeRef {
    NodeRef::new_element(qn(name), attrs.iter().map(|(k, v)| (ExpandedName::new(ns!(), LocalName::from(*k)), Attribute { prefix: None, value: (*v).to_string() })))
}

pub fn parse_body(html: &str) -> NodeRef {
    let doc = kuchikiki::parse_html().one(format!("<!DOCTYPE html><html><body>{html}</body></html>")).document_node;
    doc.select_first("body").expect("body").as_node().clone()
}

pub fn fragment(html: &str) -> Vec<NodeRef> {
    let root = kuchikiki::parse_fragment(qn("div"), vec![]).one(html).document_node;
    let container = root.first_child().expect("fragment root");
    container.children().collect()
}

pub fn inner_html(node: &NodeRef) -> String {
    node.children().map(|c| c.to_string()).collect()
}

pub fn name(n: &NodeRef) -> Option<String> {
    n.as_element().map(|e| e.name.local.to_string())
}

pub fn is(n: &NodeRef, tag: &str) -> bool {
    n.as_element().is_some_and(|e| &*e.name.local == tag)
}

pub fn attr(n: &NodeRef, k: &str) -> Option<String> {
    n.as_element().and_then(|e| e.attributes.borrow().get(k).map(str::to_string))
}

pub fn has_class(n: &NodeRef, class: &str) -> bool {
    attr(n, "class").is_some_and(|c| c.split_whitespace().any(|x| x == class))
}

pub fn set_attr(n: &NodeRef, k: &str, v: &str) {
    if let Some(e) = n.as_element() {
        e.attributes.borrow_mut().insert(k, v.to_string());
    }
}

pub fn remove_attr(n: &NodeRef, k: &str) {
    if let Some(e) = n.as_element() {
        e.attributes.borrow_mut().remove(k);
    }
}

pub fn same(a: &NodeRef, b: &NodeRef) -> bool {
    std::rc::Rc::ptr_eq(&a.0, &b.0)
}

pub fn move_children(from: &NodeRef, to: &NodeRef) {
    for c in from.children().collect::<Vec<_>>() {
        to.append(c);
    }
}

pub fn replace(old: &NodeRef, new: &NodeRef) {
    old.insert_before(new.clone());
    old.detach();
}

pub fn wrap(node: &NodeRef, wrapper: &NodeRef) {
    node.insert_before(wrapper.clone());
    wrapper.append(node.clone());
}

pub fn elements(root: &NodeRef) -> Vec<NodeRef> {
    root.descendants().filter(|n| n.as_element().is_some()).collect()
}

pub fn flatten_deep(root: &NodeRef, max: usize) {
    let mut depth = 0;
    let mut deep = Vec::new();
    for edge in root.traverse() {
        match edge {
            NodeEdge::Start(n) if n.as_element().is_some() => {
                depth += 1;
                if depth > max {
                    deep.push(n);
                }
            }
            NodeEdge::End(n) if n.as_element().is_some() => depth -= 1,
            _ => {}
        }
    }
    for n in deep {
        for c in n.children().collect::<Vec<_>>() {
            n.insert_before(c);
        }
        n.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_deep_keeps_text() {
        let body = parse_body("<div><div><div><p>t</p></div></div></div>");
        flatten_deep(&body, 2);
        let html = inner_html(&body);
        assert!(html.contains('t'));
        assert_eq!(html.matches("<div>").count(), 2);
    }
}
