//! The file list's preview: the top screen of the selected file, drawn on a worker thread with its own `Engine`.
//! It never uses the network, so browsing a cloned repo tells no image server which files went by.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::Event;
use super::picker::Area;
use crate::render::engine::Engine;
use crate::term::kitty::{self, Medium, Place, Shm};
use crate::theme::Theme;

/// The most files whose previews stay in the terminal.
const KEEP: usize = 6;
/// A screen of text takes about one source line per 7 CSS px of height.
const MIN_LINES: usize = 40;
const PX_PER_LINE: f64 = 7.0;
/// Far more than a screen needs, so a huge file costs at most this.
const READ_CAP: usize = 256 << 10;
/// A move this soon after the last is part of a run, such as a held key, and waits until the selection rests this
/// long. macOS repeats a held key every 30 to 120 ms.
pub const REST: Duration = Duration::from_millis(120);

/// What a preview shows: the file as it was, at one size, in one theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub path: PathBuf,
    pub mtime: SystemTime,
    /// Device pixels.
    pub width: u32,
    pub max_width: u32,
    pub height: u32,
    pub cell_h: u32,
    /// By address: each theme is read once per run.
    pub theme: usize,
}

struct Job {
    key: Key,
    /// A newer round drops the job before it starts.
    round: u64,
    scale: f64,
    theme: &'static Theme,
    smooth: bool,
    display_p3: bool,
}

/// Tiles top to bottom, each a whole number of cells tall. None when the file can't be drawn.
struct Done {
    key: Key,
    tiles: Option<Vec<(u32, Shm)>>,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    /// The worker's job, its round, and whether it is drawing. A job still waiting in an older round gets dropped,
    /// so a new request for it must queue it again.
    busy: Option<(Key, u64, bool)>,
    /// Goes up each time the selection moves to another file.
    round: u64,
    rest_until: Option<Instant>,
    closed: bool,
}

struct Shared {
    q: Mutex<Queue>,
    cv: Condvar,
}

struct Entry {
    key: Key,
    /// Image id and height of each tile. Empty for a file that failed, so it isn't asked for again.
    tiles: Vec<(u32, u32)>,
    bytes: usize,
}

pub struct Preview {
    shared: Arc<Shared>,
    rx: Receiver<Done>,
    /// Oldest first.
    held: Vec<Entry>,
    placed: Vec<u32>,
    medium: Medium,
    limit: usize,
    next_id: u32,
    seq: u64,
    wanted: Option<Key>,
    moved: Option<Instant>,
    /// Stays placed until the selected file's preview is ready, as in fzf.
    shown: Option<Key>,
    /// Dropped once by the terminal. A second drop means it can't hold them.
    dropped: Vec<Key>,
    /// Neighbors that didn't fit beside the selection, not asked for again until it moves.
    unfit: Vec<Key>,
    ahead: bool,
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.shared.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner).closed = true;
        self.shared.cv.notify_all();
    }
}

/// The lines that fill `height` CSS px, plus the link definitions, so links keep their look. None for a short file.
pub fn head(src: &str, height: f64) -> Option<String> {
    let lines = ((height / PX_PER_LINE) as usize).max(MIN_LINES);
    let mut it = src.lines();
    let mut out: String = it.by_ref().take(lines).flat_map(|l| [l, "\n"]).collect();
    let rest: Vec<&str> = it.collect();
    if rest.is_empty() {
        return None;
    }
    out.push('\n');
    for l in rest {
        let t = l.trim_start();
        if l.len() - t.len() <= 3 && t.starts_with('[') && t.contains("]:") {
            out.push_str(l);
            out.push('\n');
        }
    }
    Some(out)
}

/// At most `READ_CAP` bytes. A char the cap splits is dropped, not shown as �.
fn read_start(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(READ_CAP as u64).read_to_end(&mut buf).ok()?;
    if buf.len() == READ_CAP
        && let Err(e) = std::str::from_utf8(&buf)
        && e.error_len().is_none()
    {
        buf.truncate(e.valid_up_to());
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn draw(engine: &mut Option<Engine>, job: &Job) -> Option<Vec<(u32, Shm)>> {
    let k = &job.key;
    let css_w = f64::from(k.width) / job.scale;
    let css_h = f64::from(k.height) / job.scale;
    let css_max = f64::from(k.max_width) / job.scale;
    let e = engine.get_or_insert_with(|| {
        let e = Engine::new(css_w, job.scale, job.smooth);
        e.set_offline(true);
        e
    });
    e.set_display_p3(job.display_p3);
    if (e.scale.get() - job.scale).abs() > f64::EPSILON {
        e.rescale(job.scale, css_w, css_max);
    } else {
        e.resize(css_w, css_max);
    }
    let src = read_start(&k.path)?;
    let base = crate::render::images::base_dir(&k.path, false);
    let callouts = crate::config::get().callouts;
    match head(&src, css_h) {
        Some(cut) => {
            e.load(&cut, &base, false, job.theme, callouts);
            // Tall blocks can push the cut's end above the bottom of the preview.
            if e.layout().height < css_h {
                e.load(&src, &base, false, job.theme, callouts);
            }
        }
        None => e.load(&src, &base, false, job.theme, callouts),
    }
    let step = kitty::tile_rows(k.width, k.cell_h).max(1) * k.cell_h;
    let mut tiles = vec![];
    let mut top = 0;
    while top < k.height {
        let h = step.min(k.height - top);
        let s = e.snapshot(f64::from(top) / job.scale, f64::from(h) / job.scale, job.scale, |n| Shm::create(kitty::shm_name(), n))?;
        tiles.push((s.height as u32, s.rgba));
        top += h;
    }
    Some(tiles)
}

/// Waits out the rest, then marks the job as drawing. False when the selection moved on.
fn rested(shared: &Shared, round: u64) -> bool {
    let mut q = shared.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        if q.closed || q.round != round {
            return false;
        }
        let left = q.rest_until.map_or(Duration::ZERO, |t| t.saturating_duration_since(Instant::now()));
        if left.is_zero() {
            // Under the same lock, so no request can queue the job again from here on.
            if let Some(b) = &mut q.busy {
                b.2 = true;
            }
            return true;
        }
        q = shared.cv.wait_timeout(q, left).unwrap_or_else(std::sync::PoisonError::into_inner).0;
    }
}

fn work(shared: &Shared, tx: &Sender<Done>, wake: &Sender<Event>) {
    let mut engine = None;
    loop {
        let job = {
            let mut q = shared.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                if q.closed {
                    return;
                }
                if !q.jobs.is_empty() {
                    let job = q.jobs.remove(0);
                    q.busy = Some((job.key.clone(), job.round, false));
                    break job;
                }
                q = shared.cv.wait(q).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        if !rested(shared, job.round) {
            crate::trace::log(format_args!("preview dropped file={}", job.key.path.display()));
            shared.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner).busy = None;
            continue;
        }
        crate::trace::mark("preview start");
        let tiles = draw(&mut engine, &job);
        crate::trace::mark("preview drawn");
        shared.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner).busy = None;
        if tx.send(Done { key: job.key, tiles }).is_err() || wake.send(Event::Previewed).is_err() {
            return;
        }
    }
}

impl Preview {
    /// `first_id` keeps the image ids apart from the page's. `ahead` also draws the selection's neighbors, which a
    /// terminal slow with images (iTerm2) can't afford.
    pub fn new(wake: Sender<Event>, medium: Medium, limit: usize, first_id: u32, ahead: bool) -> Preview {
        let shared = Arc::new(Shared { q: Mutex::default(), cv: Condvar::new() });
        let (tx, rx) = mpsc::channel();
        let s = shared.clone();
        let _ = std::thread::Builder::new().name("preview".into()).stack_size(crate::render::STACK).spawn(move || work(&s, &tx, &wake));
        Preview { shared, rx, held: vec![], placed: vec![], medium, limit, next_id: first_id, seq: 1 << 40, wanted: None, moved: None, shown: None, dropped: vec![], unfit: vec![], ahead }
    }

    pub fn owns(&self, id: u32) -> bool {
        self.held.iter().any(|e| e.tiles.iter().any(|&(t, _)| t == id))
    }

    pub fn shown(&self) -> Option<&Key> {
        self.shown.as_ref()
    }

    pub fn failed(&self, key: &Key) -> bool {
        self.held.iter().any(|e| &e.key == key && e.tiles.is_empty())
    }

    pub fn wanted(&self) -> Option<&Key> {
        self.wanted.as_ref()
    }

    fn index(&self, key: &Key) -> Option<usize> {
        self.held.iter().position(|e| &e.key == key)
    }

    fn holds(&self, key: &Key) -> bool {
        self.held.iter().any(|e| &e.key == key)
    }

    /// Asks for `keys`, the selected file first. A move to another file replaces the requests not yet started, and
    /// waits for `REST` during a run. Direct transmission costs tens of ms per image, so it gets only the selected
    /// file. Frees, into `out`, previews of another size or theme.
    pub fn want(&mut self, keys: Vec<Key>, job: (f64, &'static Theme, bool, bool), out: &mut String, now: Instant) {
        let (scale, theme, smooth, display_p3) = job;
        let Some(first) = keys.first().cloned() else { return };
        let same = |e: &Entry| (e.key.width, e.key.height, e.key.cell_h, e.key.theme) == (first.width, first.height, first.cell_h, first.theme);
        let (keep, stale): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.held).into_iter().partition(same);
        self.held = keep;
        for e in &stale {
            self.free_entry(e, out);
        }
        let n = if self.medium == Medium::Direct || !self.ahead { 1 } else { keys.len() };
        let mut q = self.shared.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.wanted.as_ref() != Some(&first) {
            let run = self.moved.is_some_and(|t| now.saturating_duration_since(t) < REST);
            // A failure can be passing, such as a file mid-save, so it gets another try on the next visit.
            self.held.retain(|e| !e.tiles.is_empty() || e.key == first);
            (self.wanted, self.moved) = (Some(first), Some(now));
            q.round += 1;
            q.jobs.clear();
            self.unfit.clear();
            q.rest_until = run.then(|| now + REST);
        }
        let round = q.round;
        // The list filling in can change the neighbors of the same selection.
        q.jobs.retain(|j| keys.contains(&j.key));
        for (i, key) in keys.into_iter().take(n).enumerate() {
            if i > 0 && self.unfit.contains(&key) {
                continue;
            }
            let busy = q.busy.as_ref().is_some_and(|(k, r, drawing)| k == &key && (*drawing || *r == round));
            let asked = busy || q.jobs.iter().any(|j| j.key == key);
            if !self.holds(&key) && !asked {
                q.jobs.push(Job { key, round, scale, theme, smooth, display_p3 });
            }
        }
        drop(q);
        self.shared.cv.notify_all();
    }

    /// Returns whether `current`'s preview arrived.
    pub fn receive(&mut self, current: Option<&Key>, bytes: &mut Vec<u8>, cmds: &mut String) -> bool {
        let mut got = false;
        while let Ok(done) = self.rx.try_recv() {
            let now = current == Some(&done.key);
            if self.holds(&done.key) || (self.medium == Medium::Direct && !now) {
                continue;
            }
            got |= now;
            let Some(tiles) = done.tiles else {
                self.held.push(Entry { key: done.key, tiles: vec![], bytes: 0 });
                continue;
            };
            let need: usize = tiles.iter().map(|(_, s)| s.len()).sum();
            // Oldest first. A neighbor never evicts the preview on screen or the selected file's.
            let keep: Vec<Key> = if now { vec![] } else { self.shown.iter().chain(current).cloned().collect() };
            if !now {
                let kept = self.held.iter().filter(|e| keep.contains(&e.key));
                let (n, bytes) = kept.fold((0, 0), |(n, b), e| (n + 1, b + e.bytes));
                if n >= KEEP || bytes + need > self.limit {
                    self.unfit.push(done.key);
                    continue;
                }
            }
            while self.held.len() >= KEEP || self.bytes() + need > self.limit {
                let Some(i) = self.held.iter().position(|e| !keep.contains(&e.key)) else { break };
                let e = self.held.remove(i);
                self.free_entry(&e, cmds);
            }
            let mut ids = vec![];
            for (h, shm) in tiles {
                let id = self.next_id;
                self.next_id = match self.next_id.wrapping_add(1) {
                    0 | kitty::END_CHECK => 1,
                    n => n,
                };
                self.seq += 1;
                let width = done.key.width as usize;
                self.medium = kitty::transmit(bytes, self.medium, id, width, h as usize, shm, self.seq);
                crate::trace::log(format_args!("preview sent id={id} {width}x{h} medium={:?} file={}", self.medium, done.key.path.display()));
                ids.push((id, h));
            }
            self.held.push(Entry { key: done.key, tiles: ids, bytes: need });
        }
        got
    }

    fn bytes(&self) -> usize {
        self.held.iter().map(|e| e.bytes).sum()
    }

    fn free_entry(&mut self, e: &Entry, out: &mut String) {
        for &(id, _) in &e.tiles {
            kitty::free(out, id);
            self.placed.retain(|&p| p != id);
        }
    }

    /// Until `key`'s preview is ready, the last one stays, so the preview never flashes empty.
    pub fn place(&mut self, key: Option<&Key>, area: Area, out: &mut String) {
        let at = match key.and_then(|k| self.index(k)) {
            Some(i) => Some(i),
            None if key.is_some() => self.shown.as_ref().filter(|s| key.is_some_and(|k| (s.width, s.height) == (k.width, k.height))).and_then(|s| self.index(s)),
            None => None,
        };
        // The file shown moves to the end, the last to go.
        let ids: Vec<(u32, u32)> = match at {
            Some(i) => {
                let e = self.held.remove(i);
                let ids = e.tiles.clone();
                self.shown = (!ids.is_empty()).then(|| e.key.clone());
                self.held.push(e);
                ids
            }
            None => {
                self.shown = None;
                vec![]
            }
        };
        for &p in &self.placed {
            if !ids.iter().any(|&(id, _)| id == p) {
                kitty::unplace(out, p);
            }
        }
        self.placed.clear();
        let Some(k) = key else { return };
        if let Some(sh) = &self.shown {
            crate::trace::log(format_args!("preview placed file={} for={}", sh.path.display(), k.path.display()));
        }
        let mut row = u32::from(area.row);
        for (id, h) in ids {
            let rows = h / k.cell_h.max(1);
            kitty::place(out, &Place { id, row, col: u32::from(area.col), y: 0, width: k.width, height: h, cols: u32::from(area.cols), rows, z: 0 });
            self.placed.push(id);
            row += rows;
        }
    }

    pub fn free_all(&mut self, out: &mut String) {
        for e in std::mem::take(&mut self.held) {
            self.free_entry(&e, out);
        }
        self.placed.clear();
        (self.shown, self.wanted) = (None, None);
        self.dropped.clear();
        self.unfit.clear();
        let mut q = self.shared.q.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        q.jobs.clear();
        q.round += 1;
        q.rest_until = None;
        drop(q);
        self.shared.cv.notify_all();
    }

    /// Drops previews that finished after the list closed, unsent.
    pub fn discard(&mut self) {
        while self.rx.try_recv().is_ok() {}
    }

    /// The terminal dropped image `id`. Dropped once, as a screen clear does, the preview is drawn again. Dropped
    /// twice, the terminal can't hold it, so it shows as a file that can't be drawn rather than looping.
    pub fn lost(&mut self, id: u32, out: &mut String) -> bool {
        let Some(i) = self.held.iter().position(|e| e.tiles.iter().any(|&(t, _)| t == id)) else { return false };
        let e = self.held.remove(i);
        self.limit = self.limit.min(self.bytes().saturating_add(e.bytes).saturating_sub(1)).max(e.bytes);
        crate::trace::log(format_args!("preview gone id={id} file={} limit={}", e.key.path.display(), self.limit));
        self.free_entry(&e, out);
        if self.dropped.contains(&e.key) {
            self.held.push(Entry { key: e.key, tiles: vec![], bytes: 0 });
        } else {
            self.dropped.push(e.key);
        }
        true
    }
}

pub fn key(path: &Path, mtime: SystemTime, area: Area, cell_w: u32, cell_h: u32, theme: &'static Theme) -> Key {
    Key {
        path: path.to_path_buf(),
        mtime,
        width: u32::from(area.cols) * cell_w,
        max_width: u32::from(crate::config::get().max_width.0) * cell_w,
        height: u32::from(area.rows) * cell_h,
        cell_h,
        theme: std::ptr::from_ref(theme) as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_keeps_the_top_and_the_link_definitions() {
        let mut src: String = (0..100).map(|i| format!("line {i} [a][r]\n")).collect();
        src.push_str("[r]: https://example.com\n   [^1]: a note\n    [code]: not a definition\n");
        let cut = head(&src, 280.0).unwrap();
        assert!(cut.starts_with("line 0 "));
        assert!(cut.contains("line 39 ") && !cut.contains("line 40 "));
        assert!(cut.contains("[r]: https://example.com") && cut.contains("[^1]: a note"));
        assert!(!cut.contains("[code]"));
    }

    #[test]
    fn an_offline_engine_requests_no_remote_image() {
        let th = crate::theme::builtin("github-light").unwrap();
        let e = Engine::new(400.0, 2.0, true);
        e.set_offline(true);
        e.load("![badge](https://example.com/never-fetched.svg)\n", Path::new("."), false, th, crate::render::doc::Callouts::Github);
        assert_eq!(e.pending_images(), 0);
        let online = Engine::new(400.0, 2.0, true);
        online.load("![badge](https://example.invalid/b.svg)\n", Path::new("."), false, th, crate::render::doc::Callouts::Github);
        assert_eq!(online.pending_images(), 1);
    }

    #[test]
    fn a_short_file_is_not_cut() {
        assert_eq!(head("# Title\n\nshort\n", 850.0), None);
    }

    fn key(name: &str) -> Key {
        Key { path: PathBuf::from(name), mtime: SystemTime::UNIX_EPOCH, width: 400, max_width: u32::MAX, height: 170, cell_h: 17, theme: 0 }
    }

    #[test]
    fn previews_render_off_the_main_thread_and_stay_until_evicted() {
        let dir = std::env::temp_dir().join(format!("mddog-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 5000, true);
        let th = crate::theme::builtin("github-light").unwrap();
        let keys: Vec<Key> = (0..KEEP + 1)
            .map(|i| {
                let f = dir.join(format!("{i}.md"));
                std::fs::write(&f, format!("# File {i}\n\n![badge](https://example.invalid/b{i}.svg)\n")).unwrap();
                Key { path: f, ..key("") }
            })
            .collect();
        let (mut bytes, mut cmds) = (vec![], String::new());
        for k in &keys {
            p.want(vec![k.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
            events.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
            assert!(p.receive(Some(k), &mut bytes, &mut cmds));
        }
        // 170 px in one tile, and the first file evicted to keep KEEP.
        assert_eq!(p.held.len(), KEEP);
        assert!(!p.holds(&keys[0]) && p.holds(&keys[KEEP]));
        assert!(cmds.contains("a=d,d=I,i=5000,"), "{cmds}");
        let mut out = String::new();
        p.place(Some(&keys[KEEP]), Area { col: 50, row: 3, cols: 20, rows: 10 }, &mut out);
        assert!(out.contains(&format!("a=p,i={},", 5000 + KEEP)) && out.contains("c=20,r=10"), "{out}");
        p.place(None, Area { col: 50, row: 3, cols: 20, rows: 10 }, &mut out);
        assert!(out.contains(&format!("a=d,d=i,i={},p=1", 5000 + KEEP)), "{out}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_asked_for_once() {
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        let k = key("/nonexistent/a.md");
        p.want(vec![k.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        events.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let (mut bytes, mut cmds) = (vec![], String::new());
        assert!(p.receive(Some(&k), &mut bytes, &mut cmds));
        assert!(bytes.is_empty());
        p.want(vec![k.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        assert!(p.shared.q.lock().unwrap().jobs.is_empty());
    }

    /// Markdown files in a new temp folder, and their keys.
    fn files(tag: &str, n: usize) -> (PathBuf, Vec<Key>) {
        let dir = std::env::temp_dir().join(format!("mddog-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let keys = (0..n)
            .map(|i| {
                let f = dir.join(format!("{i}.md"));
                std::fs::write(&f, format!("# File {i}\n")).unwrap();
                Key { path: f, ..key("") }
            })
            .collect();
        (dir, keys)
    }

    /// Takes every preview that arrives within `wait` of the last, and returns the files drawn, in order.
    fn arrivals(p: &mut Preview, events: &Receiver<Event>, wait: Duration) -> Vec<PathBuf> {
        let mut got = vec![];
        // A loaded CI runner can take far longer than `wait` to draw one preview.
        while events.recv_timeout(wait.max(Duration::from_secs(1))).is_ok() {
            let before: Vec<Key> = p.held.iter().map(|e| e.key.clone()).collect();
            p.receive(None, &mut vec![], &mut String::new());
            got.extend(p.held.iter().filter(|e| !before.contains(&e.key)).map(|e| e.key.path.clone()));
        }
        got
    }

    #[test]
    fn over_direct_transmission_only_the_selected_file_draws() {
        let (dir, keys) = files("direct", 3);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Direct, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(keys.clone(), (2.0, th, true, false), &mut String::new(), Instant::now());
        events.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(p.receive(Some(&keys[0]), &mut vec![], &mut String::new()));
        assert!(events.recv_timeout(REST * 2).is_err(), "no neighbor drawn ahead");
        assert_eq!(p.held.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_selected_file_draws_first_even_with_neighbors_queued() {
        let (dir, keys) = files("first", 3);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(keys.clone(), (2.0, th, true, false), &mut String::new(), Instant::now());
        let got = arrivals(&mut p, &events, Duration::from_millis(500));
        assert_eq!(got, keys.iter().map(|k| k.path.clone()).collect::<Vec<_>>());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_lost_preview_is_drawn_again_for_the_same_selection() {
        let (dir, keys) = files("lost", 1);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(keys.clone(), (2.0, th, true, false), &mut String::new(), Instant::now());
        assert_eq!(arrivals(&mut p, &events, Duration::from_millis(500)).len(), 1);
        assert!(p.lost(p.held[0].tiles[0].0, &mut String::new()));
        p.want(keys.clone(), (2.0, th, true, false), &mut String::new(), Instant::now());
        assert_eq!(arrivals(&mut p, &events, Duration::from_millis(500)), vec![keys[0].path.clone()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_last_preview_stays_until_the_next_one_is_ready() {
        let (dir, keys) = files("stays", 2);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(vec![keys[0].clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        arrivals(&mut p, &events, Duration::from_millis(500));
        let area = Area { col: 0, row: 3, cols: 50, rows: 10 };
        let mut out = String::new();
        p.place(Some(&keys[0]), area, &mut out);
        let a = p.held[0].tiles[0].0;
        // keys[1] isn't drawn yet: keys[0]'s preview stays, placed again, and nothing is taken down.
        let mut out = String::new();
        p.place(Some(&keys[1]), area, &mut out);
        assert!(out.contains(&format!("a=p,i={a},")), "{out}");
        assert!(!out.contains("a=d"), "{out}");
        // Hidden, or a file that can't be drawn, takes it down.
        let mut out = String::new();
        p.place(None, area, &mut out);
        assert!(out.contains(&format!("a=d,d=i,i={a},p=1")), "{out}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn while_keys_repeat_only_the_file_where_the_selection_rests_draws() {
        let (dir, keys) = files("rests", 5);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        // A key repeat every 30 ms, as macOS's fastest setting sends. Given times, not sleeps, so a busy machine
        // can't stretch a gap past `REST`.
        let t0 = Instant::now();
        for (i, k) in (0u32..).zip(&keys) {
            p.want(vec![k.clone()], (2.0, th, true, false), &mut String::new(), t0 + Duration::from_millis(30) * i);
        }
        let got = arrivals(&mut p, &events, REST * 3);
        assert!(got.contains(&keys[4].path), "the resting file draws: {got:?}");
        assert!(keys[1..4].iter().all(|k| !got.contains(&k.path)), "the files passed don't: {got:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_move_after_a_pause_draws_at_once() {
        let (dir, keys) = files("pause", 2);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        let t0 = Instant::now();
        p.want(vec![keys[0].clone()], (2.0, th, true, false), &mut String::new(), t0);
        arrivals(&mut p, &events, REST * 2);
        p.want(vec![keys[1].clone()], (2.0, th, true, false), &mut String::new(), t0 + REST * 2);
        assert!(p.shared.q.lock().unwrap().rest_until.is_none(), "no wait after a pause");
        assert_eq!(arrivals(&mut p, &events, REST), vec![keys[1].path.clone()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn after_a_run_of_moves_the_resting_file_draws_then_its_neighbors() {
        let (dir, keys) = files("run", 8);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        // Each move asks for the selection and both neighbors, and the list draws twice per move, as a key and
        // the arrival of a batch of files can both redraw it.
        let t0 = Instant::now();
        for i in 1..7 {
            let around = vec![keys[i].clone(), keys[i + 1].clone(), keys[i - 1].clone()];
            for _ in 0..2 {
                p.want(around.clone(), (2.0, th, true, false), &mut String::new(), t0 + Duration::from_millis(60) * i as u32);
            }
        }
        let got = arrivals(&mut p, &events, REST * 3);
        let tail: Vec<PathBuf> = got.iter().rev().take(3).rev().cloned().collect();
        assert_eq!(tail, vec![keys[6].path.clone(), keys[7].path.clone(), keys[5].path.clone()], "{got:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn closing_the_list_drops_queued_previews_and_sends_none_that_finish_later() {
        let (dir, keys) = files("close", 3);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(keys, (2.0, th, true, false), &mut String::new(), Instant::now());
        p.free_all(&mut String::new());
        assert!(p.shared.q.lock().unwrap().jobs.is_empty(), "queued jobs dropped");
        // A job already drawing still finishes: the closed list drains it without sending anything.
        while events.recv_timeout(REST * 2).is_ok() {}
        let mut bytes = vec![];
        p.discard();
        assert!(p.held.is_empty() && bytes.is_empty());
        assert!(!p.receive(None, &mut bytes, &mut String::new()));
        assert!(bytes.is_empty(), "nothing left to send");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_preview_reads_only_the_start_of_a_big_file() {
        let dir = std::env::temp_dir().join(format!("mddog-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("big.md");
        // One line, far longer than any screen, ending in a 3-byte char split by the cap.
        let mut text = "# Big\n\n".to_string();
        while text.len() < READ_CAP * 3 {
            text.push_str("word € ");
        }
        std::fs::write(&f, &text).unwrap();
        let src = read_start(&f).unwrap();
        assert!(src.len() <= READ_CAP, "{}", src.len());
        assert!(src.starts_with("# Big") && !src.ends_with('\u{FFFD}'));
        assert_eq!(read_start(&dir.join("missing.md")), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dropped_tile_frees_its_siblings_and_a_preview_dropped_twice_stops() {
        let (dir, keys) = files("dropped", 1);
        // Tall enough for two tiles: one tile holds at most `TILE_BYTES` of pixels.
        let k = Key { height: 17 * 400, ..keys[0].clone() };
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(vec![k.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        arrivals(&mut p, &events, REST * 2);
        let ids: Vec<u32> = p.held[0].tiles.iter().map(|t| t.0).collect();
        assert_eq!(ids.len(), 2);
        let mut out = String::new();
        assert!(p.lost(ids[0], &mut out));
        assert!(out.contains(&format!("a=d,d=I,i={},", ids[1])), "the sibling is freed: {out}");
        // Dropped once, as a screen clear does, it is drawn again.
        p.want(vec![k.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        events.recv_timeout(REST * 4).unwrap();
        let mut bytes = vec![];
        assert!(p.receive(Some(&k), &mut bytes, &mut String::new()));
        assert!(!bytes.is_empty() && !p.failed(&k));
        // Dropped again, the terminal can't hold it: it shows as a file that can't be drawn, rather than being
        // sent and dropped over and over.
        let id = p.held[0].tiles[0].0;
        assert!(p.lost(id, &mut String::new()));
        assert!(p.failed(&k));
        p.want(vec![k.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        assert!(events.recv_timeout(REST * 2).is_err(), "not drawn again");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_terminal_slow_with_images_draws_no_neighbors_ahead() {
        let (dir, keys) = files("ahead", 3);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, false);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(keys.clone(), (2.0, th, true, false), &mut String::new(), Instant::now());
        assert_eq!(arrivals(&mut p, &events, REST * 2), vec![keys[0].path.clone()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_neighbors_replace_queued_ones_and_the_shown_preview_is_never_evicted() {
        let (dir, keys) = files("neighbors", KEEP + 4);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        let job = (2.0, th, true, false);
        // A move during a run, so the selected file waits out the rest and its neighbors stay queued.
        p.want(vec![keys[1].clone()], job, &mut String::new(), Instant::now());
        p.want(vec![keys[0].clone(), keys[2].clone(), keys[3].clone()], job, &mut String::new(), Instant::now());
        // The list fills in: same selection, other neighbors.
        p.want(vec![keys[0].clone(), keys[4].clone(), keys[5].clone()], job, &mut String::new(), Instant::now());
        let queued: Vec<PathBuf> = p.shared.q.lock().unwrap().jobs.iter().map(|j| j.key.path.clone()).collect();
        assert!(!queued.contains(&keys[2].path) && !queued.contains(&keys[3].path), "{queued:?}");
        assert!(queued.ends_with(&[keys[4].path.clone(), keys[5].path.clone()]), "{queued:?}");
        while events.recv_timeout(REST * 2).is_ok() {}
        p.receive(None, &mut vec![], &mut String::new());
        let area = Area { col: 0, row: 3, cols: 20, rows: 10 };
        p.place(Some(&keys[0]), area, &mut String::new());
        // Fill the cache past KEEP while keys[0] stays on screen, then place nothing new.
        for k in &keys[6..] {
            p.want(vec![keys[0].clone(), k.clone()], job, &mut String::new(), Instant::now());
            while events.recv_timeout(REST).is_ok() {}
            p.receive(None, &mut vec![], &mut String::new());
        }
        assert!(p.holds(&keys[0]), "the preview on screen stays");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_outgoing_preview_makes_room_for_the_selected_one() {
        let (dir, keys) = files("outgoing", 2);
        let (wake, events) = mpsc::channel();
        // Room for one preview: 400x170 px of RGBA.
        let mut p = Preview::new(wake, Medium::Shm, 400 * 170 * 4, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        let area = Area { col: 0, row: 3, cols: 20, rows: 10 };
        p.want(vec![keys[0].clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        while events.recv_timeout(REST * 2).is_ok() {}
        p.receive(Some(&keys[0]), &mut vec![], &mut String::new());
        p.place(Some(&keys[0]), area, &mut String::new());
        std::thread::sleep(REST);
        p.want(vec![keys[1].clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        while events.recv_timeout(REST * 2).is_ok() {}
        let mut cmds = String::new();
        assert!(p.receive(Some(&keys[1]), &mut vec![], &mut cmds));
        assert!(!p.holds(&keys[0]) && p.holds(&keys[1]), "only one fits");
        assert!(cmds.contains("a=d,d=I,"), "{cmds}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_neighbor_that_does_not_fit_beside_the_shown_preview_is_not_sent() {
        let (dir, keys) = files("nofit", 2);
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 400 * 170 * 4, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(vec![keys[0].clone(), keys[1].clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        while events.recv_timeout(REST * 2).is_ok() {}
        p.receive(Some(&keys[0]), &mut vec![], &mut String::new());
        p.place(Some(&keys[0]), Area { col: 0, row: 3, cols: 20, rows: 10 }, &mut String::new());
        assert!(p.holds(&keys[0]) && !p.holds(&keys[1]) && !p.failed(&keys[1]));
        // Asked again for the same selection, it isn't drawn again just to be dropped again.
        p.want(vec![keys[0].clone(), keys[1].clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        assert!(p.shared.q.lock().unwrap().jobs.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preview_temp_files_are_numbered_apart_from_the_page_s() {
        let (wake, _events) = mpsc::channel();
        assert!(Preview::new(wake, Medium::Temp, 300 << 20, 1, true).seq >= 1 << 40);
    }

    #[test]
    fn a_failure_is_tried_again_after_the_selection_moves_away() {
        let (dir, keys) = files("retry", 2);
        let missing = Key { path: dir.join("later.md"), ..keys[0].clone() };
        let (wake, events) = mpsc::channel();
        let mut p = Preview::new(wake, Medium::Shm, 300 << 20, 1, true);
        let th = crate::theme::builtin("github-light").unwrap();
        p.want(vec![missing.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        while events.recv_timeout(REST * 2).is_ok() {}
        p.receive(Some(&missing), &mut vec![], &mut String::new());
        assert!(p.failed(&missing));
        std::fs::write(&missing.path, "# Now here\n").unwrap();
        std::thread::sleep(REST);
        p.want(vec![keys[1].clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        std::thread::sleep(REST);
        p.want(vec![missing.clone()], (2.0, th, true, false), &mut String::new(), Instant::now());
        while events.recv_timeout(REST * 2).is_ok() {}
        p.receive(Some(&missing), &mut vec![], &mut String::new());
        assert!(!p.failed(&missing) && p.holds(&missing));
        std::fs::remove_dir_all(&dir).ok();
    }
}
