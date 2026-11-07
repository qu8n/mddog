//! Runs the code highlighter on a few threads and keeps the current page's results.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc;

use crate::render::doc::{Block, Doc};

mod classify;
mod textmate;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    Func,
    Added,
    Removed,
    Meta,
    /// A plain identifier (`pl-s1`), which GitHub's highlighter colors like a string.
    Ident,
    /// A tag or JSON key (`pl-ent`).
    Tag,
    /// A variable or constructor (`pl-v`).
    Var,
    /// A type or imported name (`pl-smi`, `pl-sc`).
    Import,
    /// An escape (`pl-cce`).
    Escape,
}

/// Per source line, `(start_byte, end_byte, token)` spans.
pub type Runs = Vec<Vec<(usize, usize, TokenKind)>>;
type Job = (String, String, mpsc::Sender<(String, String, Runs)>);

/// Highlight threads. A page rarely has more languages than this, and a fixed count keeps a file of made-up
/// fence labels from starting a thread for each.
const WORKERS_MAX: usize = 4;

thread_local! {
    /// The current page's code blocks, by language and text, as `prefetch` highlighted them.
    static MEMO: RefCell<HashMap<(String, String), Runs>> = RefCell::default();
    static WORKERS: RefCell<Vec<mpsc::Sender<Job>>> = const { RefCell::new(Vec::new()) };
    /// Which worker each language goes to, in the order languages first showed up.
    static SLOTS: RefCell<HashMap<String, usize>> = RefCell::default();
}

/// Highlights the code blocks of a new page on a few threads, each language always on the same one, so the
/// first use of each language (a tree-sitter query compile or a TextMate grammar load, up to about 12 ms)
/// runs alongside the others. The threads live on, so a compiled grammar serves later pages too. Layout then
/// reads `MEMO`. `meanwhile` runs while the threads work.
pub fn prefetch(doc: &Doc, meanwhile: impl FnOnce()) {
    let mut blocks = vec![];
    code_blocks(&doc.blocks, &mut blocks);
    let old = MEMO.take();
    let mut memo = HashMap::new();
    let (back, results) = mpsc::channel();
    let mut sent = std::collections::HashSet::new();
    WORKERS.with_borrow_mut(|workers| {
        for key in blocks {
            if !sent.insert(key.clone()) {
                continue;
            }
            if let Some(runs) = old.get(&key) {
                memo.insert(key, runs.clone());
                continue;
            }
            let (lang, text) = key;
            // A worker that failed to start drops the job, and layout highlights that block itself.
            let slot = SLOTS.with_borrow_mut(|s| {
                // Made-up labels, page after page, would grow it for good.
                if s.len() >= 256 {
                    s.clear();
                }
                let n = s.len();
                *s.entry(lang.clone()).or_insert(n % WORKERS_MAX)
            });
            if workers.len() <= slot {
                workers.push(worker());
            }
            let _ = workers[slot].send((lang, text, back.clone()));
        }
    });
    drop(back);
    meanwhile();
    for (lang, text, runs) in results {
        memo.insert((lang, text), runs);
    }
    MEMO.set(memo);
}

fn worker() -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    let _ = std::thread::Builder::new().stack_size(crate::render::STACK).spawn(move || {
        for (lang, text, back) in rx {
            let runs = classify::block(&lang, &text);
            let _ = back.send((lang, text, runs));
        }
    });
    tx
}

fn code_blocks(blocks: &[Block], out: &mut Vec<(String, String)>) {
    crate::render::doc::walk_shown(blocks, &mut |b| {
        if let Block::Code { lang, text } = b {
            out.push((lang.clone(), code_text(text)));
        }
    });
}

/// Returns, per source line, `(start_byte, end_byte, token)` spans: `prefetch`'s, else computed here.
pub fn highlight(langname: &str, text: &str) -> Runs {
    MEMO.with_borrow(|m| m.get(&(langname.to_string(), text.to_string())).cloned()).unwrap_or_else(|| classify::block(langname, text))
}

/// Code as layout draws and highlights it: a tab as four spaces.
pub fn code_text(text: &str) -> String {
    text.replace('\t', "    ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn made_up_fence_labels_share_a_few_threads() {
        let md: String = (0..50).map(|i| format!("```lang{i}\nx = {i}\n```\n\n")).collect();
        super::prefetch(&crate::render::doc::parse(&md), || {});
        assert!(super::WORKERS.with_borrow(Vec::len) <= super::WORKERS_MAX);
        assert_eq!(super::MEMO.with_borrow(std::collections::HashMap::len), 50);
    }
}
