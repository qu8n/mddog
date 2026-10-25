use serde::Deserialize;

use crate::engine::Engine;

#[derive(Clone, Copy, Debug, PartialEq)]
/// A block of the page: its source lines and where it sits, in CSS pixels.
pub struct Block {
    pub start: u32,
    pub end: u32,
    pub top: f64,
    pub height: f64,
}

pub type Rect = [f64; 4];

pub const READY: &str = "document.fonts.status === 'loaded' && [...document.images].every(i => i.complete || i.dataset.mddogSrc)";
pub const MATH_DONE: &str = "mddog.state.math === true";
pub const MERMAID_DONE: &str = "mddog.state.mermaid === true";
pub const START_MERMAID: &str = "mddog.startMermaid()";
pub const REMOTE_DONE: &str = "mddog.state.remote === true";

#[derive(Default)]
pub struct Layout {
    pub blocks: Vec<Block>,
    pub texts: Vec<String>,
    pub headings: Vec<(u8, String, f64)>,
    pub height: f64,
}

#[derive(Debug, PartialEq)]
pub enum Press {
    Nothing,
    Copy { text: String, rects: Vec<Rect>, label: String },
    Expand(i64),
    Rect(Option<Rect>),
    Other,
}

pub fn search(query: &str, keep: Option<usize>) -> String {
    format!("mddogSearch.run({}, {})", serde_json::Value::from(query), keep.map_or(-1, |k| k as i64))
}

pub fn measure(engine: &Engine) -> Layout {
    layout(&engine.eval("mddogLayout()").unwrap_or_default())
}

pub fn rects(json: &str) -> Vec<Rect> {
    serde_json::from_str(json).unwrap_or_default()
}

fn layout(json: &str) -> Layout {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let blocks = v["blocks"].as_array().cloned().unwrap_or_default();
    Layout {
        blocks: blocks
            .iter()
            .filter_map(|b| {
                let a = b.as_array()?;
                Some(Block { start: a.first()?.as_u64()? as u32, end: a.get(1)?.as_u64()? as u32, top: a.get(2)?.as_f64()?, height: a.get(3)?.as_f64()? })
            })
            .collect(),
        texts: blocks.iter().filter(|b| b.as_array().and_then(|a| a.first()).is_some_and(|v| v.as_u64().is_some())).map(|b| b[4].as_str().unwrap_or("").to_string()).collect(),
        headings: v["headings"].as_array().map(|hs| hs.iter().filter_map(|h| Some((h[0].as_u64()? as u8, h[1].as_str()?.to_string(), h[2].as_f64()?))).collect()).unwrap_or_default(),
        height: v["bottom"].as_f64().unwrap_or(0.0),
    }
}

pub fn press(json: &str) -> Press {
    let hit: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    if hit.is_null() {
        return Press::Nothing;
    }
    match hit[0].as_str() {
        Some("copy") => Press::Copy {
            text: hit[1].as_str().unwrap_or_default().to_string(),
            rects: Deserialize::deserialize(&hit[2]).unwrap_or_default(),
            label: hit[3].as_str().unwrap_or("the code").to_string(),
        },
        Some("expand") => Press::Expand(hit[1].as_i64().unwrap_or(-1)),
        Some("rect") => Press::Rect(Deserialize::deserialize(&hit[1]).ok()),
        _ => Press::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_keeps_source_blocks_and_headings() {
        let l = layout(r#"{"blocks":[[1,3,10.5,20,"Intro"],[null,null,40,5,"x"],[4,4,50,8,"Next"]],"headings":[[2,"Intro",10.5],[9,7]],"bottom":300}"#);
        assert_eq!(l.blocks.len(), 2);
        assert_eq!(l.blocks[1], Block { start: 4, end: 4, top: 50.0, height: 8.0 });
        assert_eq!(l.texts, ["Intro", "Next"]);
        assert_eq!(l.headings, [(2, "Intro".to_string(), 10.5)]);
        assert_eq!(l.height, 300.0);
        assert_eq!(layout("").height, 0.0);
    }

    #[test]
    fn press_replies_become_actions() {
        assert_eq!(press("null"), Press::Nothing);
        assert_eq!(press(""), Press::Nothing);
        assert_eq!(press(r#"["copy","fn x",[[0,1,2,3]]]"#), Press::Copy { text: "fn x".into(), rects: vec![[0.0, 1.0, 2.0, 3.0]], label: "the code".into() });
        assert_eq!(press(r#"["expand",2]"#), Press::Expand(2));
        assert_eq!(press(r#"["rect",[0,1,2,3]]"#), Press::Rect(Some([0.0, 1.0, 2.0, 3.0])));
        assert_eq!(press(r#"["toggle"]"#), Press::Other);
    }
}
