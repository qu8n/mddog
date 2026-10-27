use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::term::{self, Click, Event, Key};
use crate::theme::{Palette, bg, fg};

pub const CAP: usize = 10_000;
const HEAD_BYTES: u64 = 4096;
const BATCH: Duration = Duration::from_millis(30);
const HEADER_ROWS: usize = 3;

pub struct Item {
    pub path: PathBuf,
    /// The path relative to the root, without control characters.
    pub rel: String,
    pub title: String,
    pub mtime: SystemTime,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    Nothing,
    Redraw,
    Open(PathBuf),
    Close,
    Suspend,
    Quit,
}

struct Row {
    item: usize,
    hits: Vec<u32>,
}

pub struct Picker {
    pub root: PathBuf,
    /// A one-off message shown in the header, such as a file that failed to open.
    pub note: Option<String>,
    items: Vec<Item>,
    rows: Vec<Row>,
    query: String,
    sel: usize,
    top: usize,
    done: bool,
    rx: Option<Receiver<(Vec<Item>, bool)>>,
    stop: Arc<AtomicBool>,
    matcher: Matcher,
}

/// The folder the list starts from: the git repo that holds `start`, else `cwd`.
pub fn scope(start: &Path, cwd: &Path) -> PathBuf {
    start.ancestors().find(|d| d.join(".git").exists()).map_or_else(|| cwd.to_path_buf(), Path::to_path_buf)
}

/// Whether the file list shows `p`, by the config's `file_list.extensions`.
pub fn listed(p: &Path) -> bool {
    crate::config::get().file_list.shows(p)
}

/// The text of the first ATX heading, outside front matter and code fences.
pub fn title(text: &str) -> String {
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim_end() == "---") {
        lines.next();
        for l in lines.by_ref() {
            if matches!(l.trim_end(), "---" | "...") {
                break;
            }
        }
    }
    let run = |t: &str, c: u8| t.bytes().take_while(|&b| b == c).count();
    let mut fence: Option<(u8, usize)> = None;
    for l in lines {
        let t = l.trim_start();
        if let Some((c, n)) = fence {
            if run(t, c) >= n && t.trim_start_matches(c as char).trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if let Some(c) = b"`~".iter().copied().find(|&c| run(t, c) >= 3) {
            fence = Some((c, run(t, c)));
            continue;
        }
        if l.len() - t.len() > 3 {
            continue;
        }
        let hashes = t.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with([' ', '\t']) {
            let h = t[hashes..].trim();
            // A closing run of # counts only after a space: `# C#` keeps its #.
            let bare = h.trim_end_matches('#');
            return clean(if bare.is_empty() || bare.ends_with([' ', '\t']) { bare.trim_end() } else { h });
        }
    }
    String::new()
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

fn read_item(root: &Path, path: PathBuf) -> Option<Item> {
    let meta = std::fs::metadata(&path).ok()?;
    let mut head = Vec::new();
    std::fs::File::open(&path).ok()?.take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    let rel = clean(&path.strip_prefix(root).unwrap_or(&path).to_string_lossy());
    Some(Item { title: title(&String::from_utf8_lossy(&head)), rel, mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), path })
}

/// Lists the Markdown files under `root` on a thread, skipping hidden and gitignored ones, and
/// sends them in batches. Each batch also wakes the event loop. It stops once `stop` is set.
fn walk(root: PathBuf, tx: Sender<(Vec<Item>, bool)>, wake: Sender<Event>, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut batch = Vec::new();
        let mut sent = Instant::now();
        let mut count = 0;
        let walker = ignore::WalkBuilder::new(&root).require_git(false).build();
        for e in walker.flatten() {
            if count >= CAP || stop.load(Ordering::Relaxed) {
                break;
            }
            if !e.file_type().is_some_and(|t| t.is_file()) || !listed(e.path()) {
                continue;
            }
            let Some(item) = read_item(&root, e.into_path()) else { continue };
            batch.push(item);
            count += 1;
            if sent.elapsed() >= BATCH {
                if tx.send((std::mem::take(&mut batch), false)).is_err() || wake.send(Event::Listed).is_err() {
                    return;
                }
                sent = Instant::now();
            }
        }
        let _ = tx.send((batch, true));
        let _ = wake.send(Event::Listed);
    });
}

fn age(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h", s / 3600),
        86_400..1_209_600 => format!("{}d", s / 86_400),
        1_209_600..5_184_000 => format!("{}w", s / 604_800),
        5_184_000..31_536_000 => format!("{}mo", s / 2_592_000),
        _ => format!("{}y", s / 31_536_000),
    }
}

/// `text` cut to `max` cells, with the chars at `hits` (sorted char indices) in `hit`, the rest in `base`.
fn styled(text: &str, hits: &[u32], max: usize, base: &str, hit: &str) -> (String, usize) {
    let mut out = String::from(base);
    let mut w = 0;
    let total = term::width(text);
    let room = if total > max { max.saturating_sub(1) } else { max };
    let mut on = false;
    for (i, c) in text.chars().enumerate() {
        let cw = term::width(c.encode_utf8(&mut [0; 4]));
        if w + cw > room {
            break;
        }
        let want = hits.binary_search(&(i as u32)).is_ok();
        if want != on {
            out.push_str(if want { hit } else { base });
            on = want;
        }
        out.push(c);
        w += cw;
    }
    if total > max && max > 0 {
        out.push_str(base);
        out.push('…');
        w += 1;
    }
    (out, w)
}

/// The end of `s` in `max` cells, after a `…` when cut, so a long path keeps its folder name.
fn tail(s: &str, max: usize) -> (String, usize) {
    if term::width(s) <= max {
        return (s.to_string(), term::width(s));
    }
    let mut w = 1;
    let mut kept: Vec<char> = Vec::new();
    for c in s.chars().rev() {
        let cw = term::width(c.encode_utf8(&mut [0; 4]));
        if w + cw > max {
            break;
        }
        kept.push(c);
        w += cw;
    }
    if max == 0 {
        return (String::new(), 0);
    }
    (std::iter::once('…').chain(kept.into_iter().rev()).collect(), w)
}

fn home_short(p: &Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && (s == h || s.starts_with(&format!("{h}/"))) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

impl Picker {
    pub fn new(root: PathBuf) -> Picker {
        Picker {
            root,
            note: None,
            items: Vec::new(),
            rows: Vec::new(),
            query: String::new(),
            sel: 0,
            top: 0,
            done: false,
            rx: None,
            stop: Arc::default(),
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
        }
    }

    /// Moves the list to the folder `start` belongs in, unless `start` is already under the root.
    /// A new root clears the filter. Returns whether the root changed.
    pub fn follow(&mut self, start: &Path, cwd: &Path) -> bool {
        if start.starts_with(&self.root) {
            return false;
        }
        let root = scope(start, cwd);
        if root == self.root {
            return false;
        }
        self.root = root;
        self.query.clear();
        true
    }

    /// Stops the walk, if one is running. The next `list` starts over.
    pub fn halt(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Starts listing the files again, keeping the filter.
    pub fn list(&mut self, wake: Sender<Event>) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.items.clear();
        self.rows.clear();
        (self.sel, self.top) = (0, 0);
        self.done = false;
        self.stop.store(true, Ordering::Relaxed);
        self.stop = Arc::default();
        walk(self.root.clone(), tx, wake, self.stop.clone());
    }

    /// Waits up to `limit` for the walker's first batch, so a small folder shows whole at once.
    pub fn first_batch(&mut self, limit: Duration) {
        if let Some(Ok((items, done))) = self.rx.as_ref().map(|rx| rx.recv_timeout(limit)) {
            self.items.extend(items);
            self.done |= done;
        }
        self.receive();
        self.add(Vec::new());
    }

    /// Takes the batches the walker sent. Returns whether the list changed.
    pub fn receive(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut got = false;
        while let Ok((items, done)) = rx.try_recv() {
            self.items.extend(items);
            self.done |= done;
            got = true;
        }
        if got {
            self.add(Vec::new());
        }
        got
    }

    fn add(&mut self, items: Vec<Item>) {
        let keep = self.selected().map(Path::to_path_buf);
        self.items.extend(items);
        self.items.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.rel.cmp(&b.rel)));
        self.filter();
        if let Some(i) = keep.and_then(|k| self.rows.iter().position(|r| self.items[r.item].path == k)) {
            self.sel = i;
        }
    }

    fn filter(&mut self) {
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let (mut buf, mut hay) = (Vec::new(), String::new());
        let mut scored: Vec<(u32, Row)> = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            if self.query.is_empty() {
                scored.push((0, Row { item: i, hits: vec![] }));
                continue;
            }
            hay.clear();
            hay.push_str(&it.rel);
            hay.push(' ');
            hay.push_str(&it.title);
            // Utf32Str::new counts graphemes, or bytes, in a name with combining marks (common in
            // macOS file names), but the hits are drawn per char, so it gets the chars.
            buf.clear();
            buf.extend(hay.chars());
            let text = if hay.is_ascii() { Utf32Str::Ascii(hay.as_bytes()) } else { Utf32Str::Unicode(&buf) };
            let mut hits = Vec::new();
            if let Some(score) = pattern.indices(text, &mut self.matcher, &mut hits) {
                hits.sort_unstable();
                hits.dedup();
                scored.push((score, Row { item: i, hits }));
            }
        }
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        self.rows = scored.into_iter().map(|(_, r)| r).collect();
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
    }

    pub fn selected(&self) -> Option<&Path> {
        self.rows.get(self.sel).map(|r| self.items[r.item].path.as_path())
    }

    fn move_by(&mut self, d: isize) -> Pick {
        let last = self.rows.len().saturating_sub(1);
        let to = self.sel.saturating_add_signed(d).min(last);
        if to == self.sel {
            return Pick::Nothing;
        }
        self.sel = to;
        Pick::Redraw
    }

    fn edit(&mut self, f: impl FnOnce(&mut String)) -> Pick {
        let before = self.query.clone();
        f(&mut self.query);
        if self.query == before {
            return Pick::Nothing;
        }
        self.sel = 0;
        self.top = 0;
        self.filter();
        Pick::Redraw
    }

    fn list_rows(rows: u16) -> usize {
        usize::from(rows).saturating_sub(HEADER_ROWS + 1).max(1)
    }

    pub fn handle(&mut self, key: Key, rows: u16) -> Pick {
        let noted = self.note.take().is_some();
        match self.key(key, rows) {
            Pick::Nothing if noted => Pick::Redraw,
            p => p,
        }
    }

    fn key(&mut self, key: Key, rows: u16) -> Pick {
        let page = Self::list_rows(rows) as isize;
        match key {
            Key::Enter => self.selected().map_or(Pick::Nothing, |p| Pick::Open(p.to_path_buf())),
            Key::Esc => Pick::Close,
            Key::Ctrl('c') => Pick::Quit,
            Key::Ctrl('z') => Pick::Suspend,
            Key::Up | Key::Ctrl('p') | Key::WheelUp(..) => self.move_by(-1),
            Key::Down | Key::Ctrl('n') | Key::WheelDown(..) => self.move_by(1),
            Key::PageUp => self.move_by(-page),
            Key::PageDown => self.move_by(page),
            Key::Home => self.move_by(isize::MIN / 2),
            Key::End => self.move_by(isize::MAX / 2),
            Key::Backspace => self.edit(|q| {
                q.pop();
            }),
            Key::Ctrl('u') => self.edit(String::clear),
            Key::Ctrl('w') => self.edit(|q| {
                let keep = q.trim_end().char_indices().rfind(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
                q.truncate(keep);
            }),
            Key::Char(c) if !c.is_control() => self.edit(|q| q.push(c)),
            Key::Mouse(Click::Release, _, row) => {
                let r = usize::from(row).checked_sub(HEADER_ROWS + 1).filter(|&r| r < Self::list_rows(rows));
                match r.map(|r| r + self.top).filter(|&i| i < self.rows.len()) {
                    Some(i) => {
                        self.sel = i;
                        Pick::Open(self.items[self.rows[i].item].path.clone())
                    }
                    None => Pick::Nothing,
                }
            }
            _ => Pick::Nothing,
        }
    }

    /// Draws the list over the whole screen, then puts the cursor at the end of the filter.
    pub fn draw(&mut self, out: &mut String, cols: u16, rows: u16, now: SystemTime, pal: &Palette) {
        let cols = usize::from(cols);
        let shown = Self::list_rows(rows);
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + shown {
            self.top = self.sel + 1 - shown;
        }
        let (dim, text, bright) = (fg(pal.fg_muted), fg(pal.fg), format!("\x1b[1m{}", fg(pal.fg_bright)));
        let reset = pal.reset();
        out.push_str(&format!("{reset}\x1b[2J"));
        let count = if let Some(n) = &self.note {
            term::printable(n)
        } else if !self.done {
            format!("{} files, listing…", self.items.len())
        } else if self.items.len() >= CAP {
            format!("first {CAP} files")
        } else if self.query.is_empty() {
            format!("{} files", self.items.len())
        } else {
            format!("{} of {} files", self.rows.len(), self.items.len())
        };
        let (where_, ww) = tail(&term::printable(&home_short(&self.root)), cols.saturating_sub(term::width(&count) + 4));
        let pad = cols.saturating_sub(ww + term::width(&count) + 2);
        out.push_str(&format!("\x1b[1;1H {bright}{where_}{reset}{}{dim}{count}{reset}", " ".repeat(pad)));
        let (q, qw) = term::fit(&term::printable(&self.query), cols.saturating_sub(4));
        out.push_str(&format!("\x1b[2;1H {bright}>{reset} {text}{q}{reset}"));
        out.push_str(&format!("\x1b[3;1H {}{}{reset}", fg(pal.border), "─".repeat(cols.saturating_sub(2))));
        if self.done && self.rows.is_empty() {
            let msg = if self.items.is_empty() { "No Markdown files here" } else { "No files match" };
            out.push_str(&format!("\x1b[{};3H{dim}{msg}{reset}", HEADER_ROWS + 1));
        }
        self.top = self.top.min(self.rows.len());
        let visible = &self.rows[self.top..self.rows.len().min(self.top + shown)];
        let path_w = visible.iter().map(|r| term::width(&self.items[r.item].rel)).max().unwrap_or(0).min(cols * 3 / 5);
        for (n, r) in visible.iter().enumerate() {
            let it = &self.items[r.item];
            let active = self.top + n == self.sel;
            let bg = if active { bg(pal.selection_bg) } else { String::new() };
            let marker = if active { "▌" } else { " " };
            let ago = age(now.duration_since(it.mtime).unwrap_or_default());
            let split = it.rel.chars().count() as u32 + 1;
            let (path_hits, title_hits): (Vec<u32>, Vec<u32>) = r.hits.iter().partition(|&&h| h < split);
            let title_hits: Vec<u32> = title_hits.iter().map(|h| h - split).collect();
            let base = format!("{bg}{text}");
            let hit = format!("{bg}{bright}");
            let (p, pw) = styled(&it.rel, &path_hits, path_w.min(cols.saturating_sub(4)), &format!("{reset}{base}"), &hit);
            let mut line = format!("{bg}{bright}{marker}{p}{}", " ".repeat(path_w.saturating_sub(pw)));
            let mut w = 2 + path_w;
            let title_room = cols.saturating_sub(w + 2 + ago.len() + 2);
            if title_room >= 8 && !it.title.is_empty() {
                let (t, tw) = styled(&it.title, &title_hits, title_room, &format!("{reset}{bg}{dim}"), &hit);
                line.push_str(&format!("{bg}  {t}"));
                w += 2 + tw;
            }
            let gap = cols.saturating_sub(w + ago.len() + 1);
            line.push_str(&format!("{reset}{bg}{}{dim}{ago} {reset}", " ".repeat(gap)));
            out.push_str(&format!("\x1b[{};1H {line}", HEADER_ROWS + 1 + n));
        }
        let hints = [("↑↓", "move"), ("Enter", "open"), ("Esc", "back"), ("Ctrl-u", "clear")];
        let mut bottom = String::new();
        let mut bw = 1;
        for (k, l) in hints {
            let w = term::width(k) + term::width(l) + 3;
            if bw + w > cols {
                break;
            }
            bottom.push_str(&format!("{}{k}{dim} {l}  ", fg(pal.fg_bright)));
            bw += w;
        }
        out.push_str(&format!("\x1b[{rows};1H{} {bottom}{}{reset}", bg(pal.bar_bg), " ".repeat(cols.saturating_sub(bw))));
        out.push_str(&format!("\x1b[2;{}H\x1b[?25h", 4 + qw));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_is_md_or_markdown_in_any_case() {
        for f in ["a.md", "a.MD", "a.markdown", "dir.d/a.Markdown"] {
            assert!(listed(Path::new(f)), "{f}");
        }
        for f in ["a.mdx", "a.mdown", "a.mkd", "a.txt", "md", "a.md.bak"] {
            assert!(!listed(Path::new(f)), "{f}");
        }
    }

    fn dark() -> Palette {
        crate::theme::builtin("github-dark").unwrap().palette
    }

    #[test]
    fn a_light_theme_paints_the_list_light() {
        let light = crate::theme::builtin("github-light").unwrap().palette;
        let mut p = picker(vec![item("a.md", "A", 60), item("b.md", "B", 120)]);
        p.done = true;
        let mut out = String::new();
        p.draw(&mut out, 60, 10, SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000), &light);
        let drawn = crate::viewer::loading::tests::sgr_colors(&out);
        for c in [light.bg, light.fg, light.fg_muted, light.fg_bright, light.selection_bg, light.bar_bg, light.border] {
            assert!(drawn.contains(&c), "{c:?} not drawn");
        }
        assert!(drawn.iter().all(|c| [light.bg, light.fg, light.fg_muted, light.fg_bright, light.selection_bg, light.bar_bg, light.border].contains(c)), "{drawn:?}");
        assert!(out.starts_with(&format!("\x1b[0m{}\x1b[2J", crate::theme::bg(light.bg))));
    }

    fn item(rel: &str, title: &str, age_s: u64) -> Item {
        Item { path: PathBuf::from("/r").join(rel), rel: rel.into(), title: title.into(), mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age_s) }
    }

    fn picker(items: Vec<Item>) -> Picker {
        let mut p = Picker::new(PathBuf::from("/r"));
        p.done = true;
        p.add(items);
        p
    }

    fn names(p: &Picker) -> Vec<&str> {
        p.rows.iter().map(|r| p.items[r.item].rel.as_str()).collect()
    }

    fn typed(p: &mut Picker, s: &str) {
        for c in s.chars() {
            p.handle(Key::Char(c), 24);
        }
    }

    #[test]
    fn hits_count_chars_in_decomposed_names() {
        let mut p = picker(vec![item("cafe\u{301}.md", "", 1), item("re\u{301}sume\u{301}/x.md", "", 2)]);
        typed(&mut p, "md");
        let hits: Vec<Vec<u32>> = p.rows.iter().map(|r| r.hits.clone()).collect();
        assert_eq!(hits, [vec![6, 7], vec![11, 12]]);
    }

    #[test]
    fn p01_newest_first_before_typing() {
        let p = picker(vec![item("old.md", "", 500), item("new.md", "", 10), item("mid.md", "", 100)]);
        assert_eq!(names(&p), ["new.md", "mid.md", "old.md"]);
    }

    #[test]
    fn p02_typing_filters_by_path_and_title() {
        let mut p = picker(vec![item("docs/behavior.md", "Behavior spec", 1), item("README.md", "mddog", 2), item("notes/x.md", "Readme notes", 3)]);
        typed(&mut p, "readme");
        assert_eq!(names(&p), ["README.md", "notes/x.md"]);
        assert_eq!(p.rows[0].hits, [0, 1, 2, 3, 4, 5]);
        p.handle(Key::Ctrl('u'), 24);
        assert_eq!(p.rows.len(), 3);
        typed(&mut p, "zzz");
        assert!(p.rows.is_empty());
        assert_eq!(p.handle(Key::Enter, 24), Pick::Nothing);
    }

    #[test]
    fn p03_arrows_and_ctrl_n_p_move() {
        let mut p = picker(vec![item("a.md", "", 1), item("b.md", "", 2), item("c.md", "", 3)]);
        assert_eq!(p.handle(Key::Down, 24), Pick::Redraw);
        p.handle(Key::Ctrl('n'), 24);
        assert_eq!(p.selected(), Some(Path::new("/r/c.md")));
        assert_eq!(p.handle(Key::Down, 24), Pick::Nothing);
        p.handle(Key::Ctrl('p'), 24);
        p.handle(Key::Up, 24);
        assert_eq!(p.selected(), Some(Path::new("/r/a.md")));
        p.handle(Key::End, 24);
        assert_eq!(p.selected(), Some(Path::new("/r/c.md")));
        p.handle(Key::Home, 24);
        assert_eq!(p.selected(), Some(Path::new("/r/a.md")));
    }

    #[test]
    fn p04_enter_opens_and_esc_closes() {
        let mut p = picker(vec![item("a.md", "", 1), item("b.md", "", 2)]);
        p.handle(Key::Down, 24);
        assert_eq!(p.handle(Key::Enter, 24), Pick::Open(PathBuf::from("/r/b.md")));
        assert_eq!(p.handle(Key::Esc, 24), Pick::Close);
        assert_eq!(p.handle(Key::Ctrl('c'), 24), Pick::Quit);
        assert_eq!(p.handle(Key::Char('q'), 24), Pick::Redraw);
    }

    #[test]
    fn p05_click_opens_a_row() {
        let mut p = picker(vec![item("a.md", "", 1), item("b.md", "", 2)]);
        assert_eq!(p.handle(Key::Mouse(Click::Release, 5, 5), 24), Pick::Open(PathBuf::from("/r/b.md")));
        assert_eq!(p.handle(Key::Mouse(Click::Release, 5, 9), 24), Pick::Nothing);
        assert_eq!(p.handle(Key::Mouse(Click::Release, 5, 2), 24), Pick::Nothing);
    }

    #[test]
    fn a_relist_after_scrolling_draws() {
        let (wake, _w) = mpsc::channel();
        let mut p = picker((0..50).map(|i| item(&format!("f{i}.md"), "", i)).collect());
        p.handle(Key::End, 10);
        p.draw(&mut String::new(), 80, 10, SystemTime::UNIX_EPOCH, &dark());
        assert!(p.top > 0);
        p.root = std::env::temp_dir().join("mddog-no-such-dir");
        p.list(wake);
        p.draw(&mut String::new(), 80, 10, SystemTime::UNIX_EPOCH, &dark());
    }

    #[test]
    fn ctrl_w_deletes_after_wide_space() {
        let mut p = picker(vec![]);
        typed(&mut p, "a\u{3000}b");
        p.handle(Key::Ctrl('w'), 24);
        assert_eq!(p.query, "a\u{3000}");
    }

    #[test]
    fn a_note_cannot_send_escapes() {
        let mut p = picker(vec![]);
        p.note = Some("Can't open \x1b]0;x\x07a.md".into());
        let mut out = String::new();
        p.draw(&mut out, 80, 10, SystemTime::UNIX_EPOCH, &dark());
        assert!(!out.contains("\x1b]0;"));
    }

    #[test]
    fn a_new_batch_keeps_the_selection() {
        let mut p = picker(vec![item("a.md", "", 5), item("b.md", "", 6)]);
        p.handle(Key::Down, 24);
        p.add(vec![item("new.md", "", 1)]);
        assert_eq!(p.selected(), Some(Path::new("/r/b.md")));
    }

    #[test]
    fn titles_skip_front_matter_and_code() {
        assert_eq!(title("---\ntitle: x\n# no\n---\n\n```\n# code\n```\n## Real one ##\n"), "Real one");
        assert_eq!(title("text\n#nospace\n    # indented code\n# Yes"), "Yes");
        assert_eq!(title("no heading"), "");
        assert_eq!(title("# C#\n"), "C#");
        assert_eq!(title("````\n```\n# in code\n````\n# After"), "After");
    }

    #[test]
    fn scope_is_the_git_root_or_the_folder() {
        let dir = std::env::temp_dir().join(format!("mddog-scope-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
        std::fs::create_dir_all(dir.join("repo/docs")).unwrap();
        std::fs::create_dir_all(dir.join("plain")).unwrap();
        assert_eq!(scope(&dir.join("repo/docs"), &dir), dir.join("repo"));
        assert_eq!(scope(&dir.join("plain"), &dir.join("plain")), dir.join("plain"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_list_follows_the_file_to_another_repo() {
        let dir = std::env::temp_dir().join(format!("mddog-follow-{}", std::process::id()));
        for d in ["a/.git", "a/docs", "a/sub/.git", "b/.git", "b/docs", "notes"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let mut p = Picker::new(dir.join("a"));
        p.query = "x".into();
        assert!(!p.follow(&dir.join("a/docs"), &dir));
        assert!(!p.follow(&dir.join("a/sub"), &dir), "a repo inside the root keeps the root");
        assert_eq!(p.query, "x");
        assert!(p.follow(&dir.join("b/docs"), &dir));
        assert_eq!((p.root.as_path(), p.query.as_str()), (dir.join("b").as_path(), ""));
        // A list of a plain folder keeps it for its own files and follows a file into a repo.
        let mut p = Picker::new(dir.join("notes"));
        assert!(!p.follow(&dir.join("notes"), &dir));
        assert!(p.follow(&dir.join("b/docs"), &dir));
        assert_eq!(p.root, dir.join("b"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn walk_skips_hidden_ignored_and_other_files() {
        let dir = std::env::temp_dir().join(format!("mddog-walk-{}", std::process::id()));
        for f in ["a.md", "b.MD", "c.markdown", "d.txt", "e.mdx", ".hidden/e.md", "ignored/f.md", "sub/g.md"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "# T\n").unwrap();
        }
        std::fs::write(dir.join(".gitignore"), "ignored/\n").unwrap();
        let (wake, _w) = mpsc::channel();
        let mut p = Picker::new(dir.clone());
        p.list(wake);
        let until = Instant::now() + Duration::from_secs(5);
        while !p.done && Instant::now() < until {
            p.receive();
        }
        let mut got: Vec<&str> = p.items.iter().map(|i| i.rel.as_str()).collect();
        got.sort_unstable();
        assert_eq!(got, ["a.md", "b.MD", "c.markdown", "sub/g.md"]);
        assert!(p.items.iter().all(|i| i.title == "T"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rows_fit_the_width_and_mark_matches() {
        let mut p = picker(vec![item("a-very-long-folder-name/and-a-long-file-name.md", "A title that is long", 7200)]);
        typed(&mut p, "long");
        let mut out = String::new();
        p.draw(&mut out, 40, 10, SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000), &dark());
        assert!(out.contains("2h"));
        assert!(out.contains('…'));
        assert_eq!(tail("/a/long/path", 6), ("…/path".to_string(), 6));
        assert_eq!(tail("/a", 6), ("/a".to_string(), 2));
        let (s, w) = styled("abcdef", &[1, 2], 4, "B", "H");
        assert_eq!((s.as_str(), w), ("BaHbcB…", 4));
    }

    #[test]
    fn ages_are_short() {
        assert_eq!(age(Duration::from_secs(5)), "now");
        assert_eq!(age(Duration::from_secs(3 * 3600)), "3h");
        assert_eq!(age(Duration::from_secs(3 * 86_400)), "3d");
        assert_eq!(age(Duration::from_secs(400 * 86_400)), "1y");
    }
}
