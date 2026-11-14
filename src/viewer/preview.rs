use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::Event;
use super::app::{self, Action, App, Heading, Notice, OnClick};
use super::cache::{Batch, Cache, Pixels, View};
use super::chrome::{self, Chrome};
use super::clipboard;
use super::link::{self, Link};
use super::loading::Loader;
use super::picker::{self, Pick, Picker};
use super::position;
use crate::render::engine::{Engine, Layout, Pointer, Press, Rect, Select};
use crate::term::kitty::{self, Shm};
use crate::term::screen;
use crate::term::{self, Click, Input, Key, Probe, Size, Tty};
use crate::theme::Theme;
use crate::{config, editor, trace, watch};

/// How long the page waits on its remote images before it ticks slowly again and prefetches.
const LATE_LIMIT: Duration = Duration::from_secs(60);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const COPY_FEEDBACK: Duration = Duration::from_secs(2);
const NOTICE: Duration = Duration::from_secs(3);
const PROBE_WAIT: Duration = Duration::from_secs(1);
const CLICK_AGAIN: Duration = Duration::from_millis(300);
/// The time between a glide's frames.
const GLIDE_FRAME: Duration = Duration::from_millis(8);
const FOLD_IDLE: Duration = Duration::from_secs(1);
/// The X11 name for the arrow, sent with OSC 22 off text, links and buttons.
const DEFAULT_POINTER: &str = "left_ptr";
const SUPPORTED: &str = "Ghostty, iTerm2, Kitty, WezTerm, or Rex";

/// Why mddog won't run here, when the environment alone tells. Checked before anything goes to the terminal.
pub fn refusal() -> Option<String> {
    Some(match term::refused_by_env(|k| std::env::var_os(k))? {
        term::Refused::Tmux => "mdd: mddog doesn't support tmux yet because tmux doesn't pass its images through. Run mdd outside tmux.".to_string(),
        term::Refused::Zellij => "mdd: mddog doesn't support zellij yet because zellij 0.45 garbles scrolled images (zellij-org/zellij#5573 and #5648). Run mdd outside zellij.".to_string(),
        term::Refused::Warp => format!("mdd: mddog doesn't support Warp because Warp can't crop images, and mddog crops images to scroll them. Use {SUPPORTED}."),
    })
}
/// The longest the input thread waits before it checks `paused` again.
const INPUT_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy)]
struct Drag {
    from: Spot,
    at: Spot,
    moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    cell: (u16, u16),
    sub: Option<(f64, f64)>,
}

/// The parts of the screen: the contents sidebar on the left, the page, and the status row under both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Sidebar,
    Page,
    Status,
}

struct Grid {
    col0: u16,
    cell_w: u32,
    cell_h: u32,
    top: u32,
    scale: f64,
}

impl Grid {
    fn point(&self, spot: Spot, fx: f64) -> (f64, f64) {
        let (col, row) = spot.cell;
        let (fx, fy) = spot.sub.unwrap_or((fx, 0.5));
        let x = f64::from(col.saturating_sub(1).saturating_sub(self.col0)) + fx;
        let y = f64::from(row.saturating_sub(1)) + fy;
        (x * f64::from(self.cell_w) / self.scale, (f64::from(self.top) + y * f64::from(self.cell_h)) / self.scale)
    }
}

fn pixel_spot((x, y): (u16, u16), (cell_w, cell_h): (u32, u32), (cols, rows): (u16, u16)) -> Spot {
    let (cx, cy) = (f64::from(x) / f64::from(cell_w), f64::from(y) / f64::from(cell_h));
    let cell = (cx as u16 + 1, cy as u16 + 1);
    if cell.0 > cols || cell.1 > rows {
        return Spot { cell: (cell.0.min(cols.max(1)), cell.1.min(rows.max(1))), sub: None };
    }
    Spot { cell, sub: Some((cx.fract(), cy.fract())) }
}

/// Device pixels per CSS pixel on the page: the display's scale, zoomed by how much the cell grew since
/// startup (`base`), so the page follows the terminal's font size. The page then still spans the pane's
/// pixels, so its tiles need no more memory at any zoom.
fn page_scale(display: f64, cell_h: u32, base: u32) -> f64 {
    if base == 0 { display } else { display * f64::from(cell_h) / f64::from(base) }
}

/// Snapshots `height` device pixels from `top`.
fn snapper(engine: &Engine, scale: f64) -> impl FnMut(u32, u32) -> Option<Pixels> + '_ {
    move |top, height| engine.snapshot(f64::from(top) / scale, f64::from(height) / scale, scale, |n| Shm::create(kitty::shm_name(), n))
}

/// A file the viewer left for another one, to go back to.
struct Visit {
    file: PathBuf,
    stdin: bool,
    name: String,
    app: App,
    place: Place,
}

/// Where a page was, in its content rather than its rows, so a new layout can come back to it.
struct Place {
    /// The view, None at the top.
    view: Option<position::Anchor>,
    /// Each Backspace row, as `back_anchors` takes them.
    back: Vec<Option<position::Anchor>>,
    /// The source the anchors were taken from.
    src: String,
}

pub struct Preview {
    watcher: Option<notify::RecommendedWatcher>,
    loader: Option<Loader>,
    tty: Tty,
    engine: Engine,
    file: PathBuf,
    name: String,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    paused: Arc<AtomicBool>,
    probe: Probe,
    /// Device pixels per CSS pixel on the page: the display's scale times the font zoom (see `page_scale`).
    scale: f64,
    /// The display's backing scale, which the cell size guesses need.
    display: f64,
    /// The terminal's size at startup, whose cell height is the page's zoom of 1.
    start: Size,
    cell_w: u32,
    cell_h: u32,
    size: Size,
    last_event: Instant,
    src: String,
    stamp: Vec<u8>,
    app: App,
    layout: Layout,
    matches: Vec<Vec<Rect>>,
    drag: Option<Drag>,
    bar_held: bool,
    last_press: Option<(Instant, u16, u16)>,
    /// The pointer shape last sent with OSC 22.
    pointer: &'static str,
    /// Where the last move was hit-tested, with the scroll it was drawn at, so a repeat does nothing.
    hover_at: Option<(Spot, u32)>,
    /// The last move was over the page, so leaving it must clear the page's hover.
    hover_page: bool,
    copy_until: Option<Instant>,
    notice_until: Option<Instant>,
    quit_armed: Option<Instant>,
    /// Text cells may hold something the page doesn't cover: help or the file list, or a sidebar or status row
    /// from before a new geometry. The next page frame clears the screen first.
    help_drawn: bool,
    stdin: bool,
    base: PathBuf,
    doc_px: u32,
    phase: u32,
    cache: Cache,
    img_col: u16,
    img_cols: u16,
    /// Whether the page loaded. A missing page makes the next re-render or file event try again.
    loaded: bool,
    /// The loaded page's theme.
    theme: &'static Theme,
    /// A click changed the page (a copy, a disclosure, a button) since it loaded, which a reload undoes.
    touched: bool,
    /// Until when the page waits on its remote images: a fast tick, and no prefetch. They still arrive after it.
    late: Option<Instant>,
    resize: app::Debounce,
    quit: bool,
    /// Whether a file is loaded. False while the list that `mdd` started with is up.
    opened: bool,
    picker: Option<Picker>,
    listing: bool,
    history: Vec<Visit>,
    /// Where `render_doc` puts the page instead of the top.
    restore_at: Option<Place>,
    /// The heading id `render_doc` scrolls to on a page opened by a link.
    land_on: Option<String>,
    /// The running glide and the position it last drew.
    glide: Option<(app::Glide, u32)>,
    /// The scroll position `show` last drew, where a new glide starts.
    drawn: u32,
    /// The tiles a new glide passes over, which the next `show` sends with its first frame.
    path: Vec<usize>,
    /// When the last jump key came, for `app::REPEAT`.
    last_jump: Option<Instant>,
}

pub fn run(file: PathBuf, stdin: bool) -> i32 {
    start(file, stdin, None)
}

/// Whether input typed during the first render asks to quit, in either key encoding.
fn quits(input: &[u8]) -> bool {
    term::parse_keys(&mut input.to_vec(), false).iter().any(|k| matches!(k, Key::Ctrl('c') | Key::Char('q' | 'Q')))
}

/// Starts with the list of Markdown files under `root` instead of a file.
pub fn browse(root: PathBuf) -> i32 {
    start(PathBuf::new(), false, Some(root))
}

fn start(file: PathBuf, stdin: bool, list: Option<PathBuf>) -> i32 {
    let (tx, rx) = mpsc::channel();
    let paused = Arc::new(AtomicBool::new(false));
    // Whether a Ctrl-C came while the screen was ours. Checked at delivery, not when the thread
    // runs: the same Ctrl-C ends the editor, and the viewer may be back by then.
    // Registered before `Signals`, so it runs first.
    let interrupt = Arc::new(AtomicBool::new(false));
    let (int_paused, int) = (paused.clone(), interrupt.clone());
    // SAFETY: the action only loads and stores atomics, which is async-signal-safe.
    // Only ever set here, so a paused Ctrl-C can't erase an earlier one the thread hasn't seen yet.
    let _ = unsafe {
        signal_hook::low_level::register(libc::SIGINT, move || {
            if !int_paused.load(Ordering::SeqCst) {
                int.store(true, Ordering::SeqCst);
            }
        })
    };
    if let Ok(mut signals) = signal_hook::iterator::Signals::new([libc::SIGWINCH, libc::SIGTERM, libc::SIGINT, libc::SIGHUP]) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for s in signals.forever() {
                if s == libc::SIGINT && !interrupt.swap(false, Ordering::SeqCst) {
                    continue;
                }
                let ev = if s == libc::SIGWINCH { Event::Resize } else { Event::Quit };
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });
    }
    let (worker_tx, worker_rx) = mpsc::channel();
    let f2 = file.clone();
    let mut picker = list.map(Picker::new);
    if let Some(p) = picker.as_mut() {
        p.list(tx.clone());
    }
    let _ = picker.is_none().then(|| {
        std::thread::Builder::new().stack_size(crate::render::STACK).spawn(move || {
            let bytes = std::fs::read(&f2);
            let _ = worker_tx.send(bytes.map(|b| {
                let src = String::from_utf8_lossy(&b).into_owned();
                (b, src)
            }));
        })
    });
    // The first read of the macOS appearance takes about 2 ms, so make it while the file renders.
    std::thread::spawn(config::theme);
    std::thread::spawn(screen::display_scale);
    let mut tty = match Tty::open() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("mdd: needs a terminal to show the preview ({}). It can't write to a pipe or a file. Run it in a terminal window.", term::io_error(&e));
            return 1;
        }
    };
    if let Err(e) = tty.raw() {
        eprintln!("mdd: can't take keys from the terminal: {}", term::io_error(&e));
        return 1;
    }
    term::on_panic_restore(tty.saved());
    // Pixel mouse reports wait for the probe to say the terminal has them.
    tty.write(term::enter(false).as_bytes());
    let name = if stdin { "standard input".to_string() } else { file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned()) };
    let loader = match picker.as_mut() {
        Some(p) => {
            // The list shows before the engine starts, so it is up at once.
            p.first_batch(Duration::from_millis(50));
            let mut out = String::new();
            p.draw(&mut out, tty.size().cols, tty.size().rows, std::time::SystemTime::now(), &config::theme().palette);
            tty.write(out.as_bytes());
            tty.flush();
            None
        }
        None => Some(Loader::start(tty.fd, &name)),
    };
    trace::mark("tty ready");
    let engine = Engine::new(800.0, screen::SCALE_GUESS, config::font_smoothing());
    trace::mark("engine created");
    let (probe, leftover) = tty.probe(PROBE_WAIT);
    if probe.pixel_mouse {
        tty.write(term::PIXEL_MOUSE_ON.as_bytes());
    }
    trace::mark("terminal probed");
    engine.set_display_p3(probe.ghostty());
    if probe.no_graphics() {
        let refusal = format!("mdd: this terminal can't show images so it can't show the preview. mddog needs the Kitty graphics protocol: use {SUPPORTED}.");
        drop(loader);
        drop(picker);
        tty.write(term::leave().as_bytes());
        tty.flush();
        tty.restore();
        eprintln!("{}", term::wrap(&refusal, usize::from(tty.size().cols)));
        return 1;
    }
    let size = tty.size();
    let display = engine.scale.get();
    let scale = display;
    let (cell_w, cell_h) = probe.cell_px(size, display);
    if let Some(l) = &loader {
        l.tune(scale, (cell_w, cell_h));
    }
    let cache = Cache::new(probe.storage_limit(), probe.herdr(), probe.medium(), 1 + (std::process::id() % 1000) * 1000);
    let base = crate::render::images::base_dir(&file, stdin);
    let mut p = Preview {
        watcher: None,
        loader,
        tty,
        engine,
        file,
        name,
        rx,
        tx,
        paused,
        probe,
        scale,
        display,
        start: size,
        cell_w,
        cell_h,
        size,
        last_event: Instant::now(),
        src: String::new(),
        stamp: Vec::new(),
        app: App::default(),
        layout: Layout::default(),
        matches: vec![],
        drag: None,
        bar_held: false,
        last_press: None,
        pointer: DEFAULT_POINTER,
        hover_at: None,
        hover_page: false,
        copy_until: None,
        notice_until: None,
        quit_armed: None,
        help_drawn: false,
        stdin,
        base,
        doc_px: 0,
        phase: 0,
        cache,
        img_col: 0,
        img_cols: size.cols,
        loaded: false,
        theme: &config::get().themes.light,
        touched: false,
        late: None,
        glide: None,
        drawn: 0,
        path: vec![],
        last_jump: None,
        resize: app::Debounce::default(),
        quit: false,
        opened: picker.is_none(),
        listing: picker.is_some(),
        picker,
        history: Vec::new(),
        restore_at: None,
        land_on: None,
    };
    let mut leftover = leftover;
    if p.listing {
        p.show();
        crate::sweep::spawn();
        p.start_threads(leftover);
        p.event_loop();
        p.shutdown();
        std::mem::forget(p);
        return 0;
    }
    let first = loop {
        match worker_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(v)) => break v,
            Ok(Err(e)) => {
                p.shutdown();
                eprintln!("mdd: {}: {}", term::printable(&p.file.display().to_string()), term::io_error(&e));
                return 1;
            }
            Err(RecvTimeoutError::Disconnected) => {
                p.shutdown();
                eprintln!("mdd: can't render {}", term::printable(&p.file.display().to_string()));
                return 1;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let quit = std::iter::from_fn(|| p.rx.try_recv().ok()).fold(false, |q, e| q | matches!(e, Event::Quit));
        p.tty.read_timeout(&mut leftover, Instant::now() + Duration::from_millis(1), quits);
        if quit || quits(&leftover) {
            p.shutdown();
            return 0;
        }
    };
    trace::mark("markdown rendered");
    p.render_doc(first.0, first.1, false);
    trace::mark("first paint sent");
    crate::sweep::spawn();
    if !p.probe.answered {
        p.notify_with("The terminal did not answer. A blank page means it can't show images", OnClick::Hints, Duration::from_secs(15));
    } else if std::str::from_utf8(&p.stamp).is_err() {
        p.notify("Some bytes are not UTF-8 and show as �");
    } else if !p.stdin && !picker::listed(&p.file) {
        p.notify("Not a .md file, shown as Markdown");
    }
    p.start_threads(leftover);
    p.event_loop();
    p.shutdown();
    std::mem::forget(p);
    0
}

impl Preview {
    fn start_threads(&mut self, leftover: Vec<u8>) {
        let tx = self.tx.clone();
        let paused = self.paused.clone();
        let fd = self.tty.fd;
        let first = leftover;
        std::thread::spawn(move || input_loop(fd, tx, paused, first));
        self.watch();
    }

    fn watch(&mut self) {
        self.watcher = None;
        if self.opened && self.live() {
            let tx = self.tx.clone();
            self.watcher = watch::spawn(&self.file, move || {
                let _ = tx.send(Event::FileChanged);
            });
            if self.watcher.is_some() {
                let _ = self.tx.send(Event::FileChanged);
            }
        }
    }

    fn event_loop(&mut self) {
        while !self.quit {
            let busy = self.late.is_some() || self.resize.pending() || self.prefetch_target().is_some();
            let mut timeout = if self.glide.is_some() {
                GLIDE_FRAME
            } else if busy {
                Duration::from_millis(15)
            } else {
                Duration::from_millis(400)
            };
            for due in self.copy_until.iter().chain(&self.notice_until) {
                timeout = timeout.min(due.saturating_duration_since(Instant::now()));
            }
            let mut events = Vec::new();
            match self.rx.recv_timeout(timeout) {
                Ok(e) => events.push(e),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(e) = self.rx.try_recv() {
                events.push(e);
            }
            if !events.is_empty() {
                self.last_event = Instant::now();
            }
            // Only the last of a run of moves matters, so a fast pointer costs one hit-test per batch.
            let is_move = |e: &Event| matches!(e, Event::Key(Key::Mouse(Click::Move, ..)));
            let stale: Vec<bool> = events.windows(2).map(|w| is_move(&w[0]) && is_move(&w[1])).chain([false]).collect();
            let mut need_show = false;
            for (e, _) in events.into_iter().zip(stale).filter(|(_, stale)| !stale) {
                // Anything but a move can change what lies under a still pointer.
                if !is_move(&e) {
                    self.stale_hover();
                }
                need_show |= self.handle(e);
                if self.quit {
                    return;
                }
            }
            need_show |= self.copy_due();
            if self.notice_until.is_some_and(|due| Instant::now() >= due) {
                self.notice_until = None;
                self.app.notice = None;
                self.draw_chrome();
            }
            if need_show {
                self.show();
            }
            self.tick();
            // The page moved or changed under a still pointer, so what it hovers may have changed.
            // The hit test runs after the frame went out, and not at all during a glide. It comes after `tick`, which draws a glide's last frame.
            if let Some((spot, px)) = self.hover_at
                && px != self.shown_px()
                && self.glide.is_none()
                && !self.listing
                && self.drag.is_none()
                && !self.bar_held
                && !self.app.toc_bar_held()
                && self.hover(spot)
            {
                self.show();
            }
        }
    }

    fn handle(&mut self, e: Event) -> bool {
        match e {
            Event::Quit => {
                self.quit = true;
                false
            }
            Event::Resize => {
                self.resize.poke(Instant::now());
                self.glide.take().is_some()
            }
            Event::Gone(kitty::END_CHECK) => {
                self.cache.end_check();
                false
            }
            Event::Gone(id) => {
                // A terminal that drops one image without telling mddog has likely dropped others. Only inside a
                // synchronized update, so the one-cell placements never show.
                let mut out = term::SYNC_BEGIN.to_string();
                if self.probe.sync && self.cache.holds(id) && self.cache.check(&mut out) {
                    out.push_str(term::SYNC_END);
                    self.tty.write(out.as_bytes());
                    self.tty.flush();
                }
                let v = self.view();
                let again = self.cache.lost(id, &self.cache.visible(&v), v.width);
                trace::log(format_args!("gone id={id} again={again} resident={}", self.cache.resident()));
                again
            }
            Event::FileChanged if !self.opened => false,
            Event::FileChanged => {
                trace::mark("file event");
                self.reload_if_changed();
                false
            }
            Event::Listed => self.picker.as_mut().is_some_and(Picker::receive) && self.listing,
            Event::Key(Key::Mouse(click, x, y)) if self.listing && click == Click::Move => {
                self.hover_at = Some((self.spot(x, y), u32::MAX));
                false
            }
            Event::Key(k) if self.listing => {
                trace::log(format_args!("key {k:?}"));
                let k = match k {
                    Key::Mouse(click, x, y) => {
                        let spot = self.spot(x, y);
                        self.hover_at = Some((spot, u32::MAX));
                        Key::Mouse(click, spot.cell.0, spot.cell.1)
                    }
                    k => k,
                };
                let Some(p) = self.picker.as_mut() else { return false };
                let pick = p.handle(k, self.size.rows);
                if pick == Pick::Close {
                    p.halt();
                }
                match pick {
                    Pick::Nothing => false,
                    Pick::Redraw => true,
                    Pick::Open(path) => {
                        self.pick(path);
                        false
                    }
                    Pick::Close if self.opened => {
                        self.listing = false;
                        true
                    }
                    Pick::Close | Pick::Quit => {
                        self.quit = true;
                        false
                    }
                    Pick::Suspend => {
                        self.suspend();
                        false
                    }
                }
            }
            Event::Key(k) => {
                trace::log(format_args!("key {k:?}"));
                // A wheel scroll can be the first thing the pointer does, so it records the spot too.
                if let Key::WheelUp(x, y) | Key::WheelDown(x, y) | Key::WheelLeft(x, y) | Key::WheelRight(x, y) = k {
                    self.hover_at = Some((self.spot(x, y), u32::MAX));
                }
                let k = match k {
                    Key::WheelLeft(x, y) => {
                        let (col, row) = self.spot(x, y).cell;
                        Key::WheelLeft(col, row)
                    }
                    Key::WheelRight(x, y) => {
                        let (col, row) = self.spot(x, y).cell;
                        Key::WheelRight(col, row)
                    }
                    Key::WheelUp(x, y) | Key::WheelDown(x, y) => {
                        let (col, row) = self.spot(x, y).cell;
                        if self.region((col, row)) == Region::Sidebar
                            && !self.app.help
                            && !self.app.expanded
                            && let Some(actions) = self.app.toc_wheel(matches!(k, Key::WheelDown(..)))
                        {
                            return self.act_all(actions);
                        }
                        if matches!(k, Key::WheelUp(..)) { Key::WheelUp(col, row) } else { Key::WheelDown(col, row) }
                    }
                    k => k,
                };
                if let Key::Mouse(click, x, y) = k {
                    // A press ends a glide first, so the press and its release hit the same page.
                    if click == Click::Press && self.glide.take().is_some() {
                        self.show();
                    }
                    let spot = self.spot(x, y);
                    if click != Click::Move {
                        self.hover_at = Some((spot, u32::MAX));
                    }
                    let on_hint = self.region(spot.cell) == Region::Status && self.chrome().hint_at(spot.cell.0).is_some();
                    // A move is not a click, so it never closes help or the expanded diagram.
                    if click != Click::Move
                        && let Some(actions) = self.app.modal_click(click == Click::Press, on_hint)
                    {
                        return self.act_all(actions);
                    }
                    return self.mouse_event(click, spot);
                }
                let stopped = self.app.instant(k) && self.glide.take().is_some();
                let actions = self.app.handle(k);
                self.act_all(actions) || stopped
            }
        }
    }

    fn act_all(&mut self, actions: Vec<Action>) -> bool {
        let mut show = false;
        for a in actions {
            show |= self.act(a);
        }
        show
    }

    fn act(&mut self, a: Action) -> bool {
        match a {
            Action::Scroll => {
                self.glide = None;
                true
            }
            Action::Glide => {
                self.start_glide();
                true
            }
            Action::Status => {
                self.draw_chrome();
                false
            }
            Action::Quit => {
                self.quit = true;
                false
            }
            Action::Search(q) => {
                self.glide = None;
                let m = self.engine.search(&q, None);
                self.set_matches(m);
                let rows = self.match_rows();
                for b in self.app.set_matches(rows) {
                    self.act(b);
                }
                true
            }
            Action::Current(i) => {
                let rects = self.engine.search_current(i);
                self.invalidate(&rects);
                if let Some(r) = self.matches.get(i).and_then(|m| m.first()) {
                    trace::log(format_args!("match {i}/{} rect={:.1},{:.1},{:.1},{:.1}", self.matches.len(), r[0], r[1], r[2], r[3]));
                }
                true
            }
            Action::ClearSearch => {
                self.engine.search_clear();
                self.set_matches(Vec::new());
                true
            }
            Action::ToggleToc if self.app.toc_visible && app::toc_width(self.size.cols) == 0 => {
                self.app.toc_visible = false;
                self.notify(&format!("Too narrow for the table of contents. It needs {} columns", app::TOC_MIN_COLS));
                false
            }
            Action::ToggleToc => {
                self.rerender_keep();
                false
            }
            Action::ToggleWrap => {
                self.engine.set_wrap(self.app.wrap);
                self.relayout();
                false
            }
            Action::ScrollView(cells) => {
                let (top, height) = self.view_css();
                let bottom = top + height;
                let dx = f64::from(cells) * f64::from(self.cell_w) / self.scale;
                let rects = self.engine.code_scroll_view(top, bottom, dx);
                let moved = self.redraw(&rects);
                if !moved {
                    self.notify("Nothing on screen scrolls further sideways");
                }
                moved
            }
            Action::Notice(n) => {
                self.notify(&n);
                false
            }
            // The help takes the page's colors. When the macOS appearance changed since the page was
            // drawn, reload it first; the reload draws the help.
            Action::Help if !std::ptr::eq(config::theme(), self.theme) => {
                self.rerender_keep();
                false
            }
            Action::Help => true,
            Action::Collapse => {
                let rects = self.engine.collapse();
                self.redraw(&rects)
            }
            Action::CopySelection => {
                self.copy_selection();
                false
            }
            Action::Suspend => {
                self.suspend();
                false
            }
            Action::ScrollCode(col, row, cells) => {
                if self.region((col, row)) != Region::Page {
                    return false;
                }
                let (x, y) = self.grid().point(Spot { cell: (col, row), sub: None }, 0.5);
                let dx = f64::from(cells) * f64::from(self.cell_w) / self.scale;
                let rects = self.engine.code_scroll(x, y, dx);
                self.redraw(&rects)
            }
            Action::Editor if self.stdin => {
                self.notify("Can't edit standard input");
                false
            }
            Action::Editor => {
                self.edit();
                false
            }
            Action::Browse => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let from = if self.stdin { cwd.clone() } else { self.base() };
                let tx = self.tx.clone();
                let p = self.picker.get_or_insert_with(|| Picker::new(picker::scope(&from, &cwd)));
                p.follow(&from, &cwd);
                p.list(tx);
                self.listing = true;
                // A drag's release would reach the list and open the file under the pointer.
                self.drag = None;
                self.bar_held = false;
                self.app.toc_bar_release();
                self.set_pointer(DEFAULT_POINTER);
                true
            }
            Action::BackFile => {
                self.back_file();
                false
            }
            Action::Copy => {
                match std::fs::read(&self.file) {
                    Ok(bytes) => {
                        self.copy_text(&String::from_utf8_lossy(&bytes));
                        self.notify("Copied the Markdown source");
                    }
                    Err(_) => self.notify("Can't read the file"),
                }
                false
            }
        }
    }

    fn grid(&self) -> Grid {
        Grid { col0: self.img_col, cell_w: self.cell_w, cell_h: self.cell_h, top: self.shown_px(), scale: self.scale }
    }

    fn spot(&self, x: u16, y: u16) -> Spot {
        if !self.probe.pixel_mouse {
            return Spot { cell: (x, y), sub: None };
        }
        pixel_spot((x, y), (self.cell_w, self.cell_h), (self.size.cols, self.size.rows))
    }

    fn is_double(&self, col: u16, row: u16) -> bool {
        self.last_press.is_some_and(|(t, c, r)| t.elapsed() < DOUBLE_CLICK && (c, r) == (col, row))
    }

    fn mouse_event(&mut self, click: Click, spot: Spot) -> bool {
        let (col, row) = spot.cell;
        let rows = self.viewport_rows() as u16;
        let grid = self.grid();
        match click {
            Click::Move => self.hover(spot),
            Click::Press => {
                self.drag = None;
                self.bar_held = false;
                self.app.toc_bar_release();
                let region = self.region(spot.cell);
                if region == Region::Status {
                    let on_click = self.app.notice.as_ref().map(|n| n.click);
                    let back = on_click == Some(OnClick::GoBack);
                    let Some(key) = self.chrome().hint_at(col).or(back.then_some(Key::Backspace)) else {
                        if self.app.notice.is_some() {
                            self.app.notice = None;
                            self.notice_until = None;
                            self.draw_chrome();
                        }
                        return false;
                    };
                    if key == app::QUIT.2 {
                        let armed = on_click == Some(OnClick::Quit);
                        match self.quit_armed.filter(|_| armed).map(|t| t.elapsed()) {
                            Some(e) if e < CLICK_AGAIN => return false,
                            Some(_) => {}
                            None => {
                                self.quit_armed = Some(Instant::now());
                                self.notify_with("Click again to quit", OnClick::Quit, NOTICE);
                                return false;
                            }
                        }
                    }
                    if on_click == Some(OnClick::Quit) {
                        self.app.notice = None;
                    }
                    let actions = self.app.handle(key);
                    return self.act_all(actions);
                }
                if region == Region::Sidebar
                    && let Some(actions) = self.app.toc_bar_press(self.img_col - col, u32::from(row) - 1)
                {
                    return self.act_all(actions);
                }
                if region == Region::Sidebar {
                    let actions = self.app.toc_click(u32::from(row) - 1);
                    return self.act_all(actions);
                }
                let (x, y) = grid.point(spot, 0.5);
                if let Some(rects) = self.engine.code_grab(x, y, f64::from(self.cell_h) / self.scale / 2.0) {
                    self.bar_held = true;
                    self.last_press = None;
                    self.invalidate(&rects);
                    return !rects.is_empty();
                }
                let double = self.is_double(col, row);
                self.last_press = (!double).then(|| (Instant::now(), col, row));
                self.drag = Some(Drag { from: spot, at: spot, moved: double });
                if double {
                    return self.select(Select::Word(x, y));
                }
                false
            }
            Click::Drag if self.app.toc_bar_held() => {
                let actions = self.app.toc_bar_drag(u32::from(row).saturating_sub(1));
                self.act_all(actions)
            }
            Click::Release if self.app.toc_bar_held() => {
                let actions = self.app.toc_bar_drag(u32::from(row).saturating_sub(1));
                self.app.toc_bar_release();
                self.act_all(actions)
            }
            Click::Drag if self.bar_held => {
                let (x, y) = grid.point(spot, 0.5);
                let rects = self.engine.code_drag(x, y);
                self.redraw(&rects)
            }
            Click::Release if self.bar_held => {
                let shown = self.mouse_event(Click::Drag, spot);
                self.bar_held = false;
                self.engine.code_release();
                shown
            }
            Click::Drag => {
                let Some(d) = self.drag.as_mut() else { return false };
                let cell = (col.max(self.img_col + 1), row.clamp(1, rows));
                let at = Spot { cell, sub: spot.sub.filter(|_| cell == spot.cell) };
                if at == d.at {
                    return false;
                }
                d.at = at;
                d.moved = true;
                let order = |s: Spot| (s.cell.1, s.cell.0, s.sub.map_or(0.5, |(x, _)| x));
                let forward = order(at) >= order(d.from);
                let (ax, ay) = grid.point(d.from, if forward { 0.0 } else { 1.0 });
                let (fx, fy) = grid.point(at, if forward { 1.0 } else { 0.0 });
                self.select(Select::Range(ax, ay, fx, fy))
            }
            Click::Release => {
                if self.drag.is_none() {
                    return false;
                }
                let shown = self.mouse_event(Click::Drag, spot);
                let Some(d) = self.drag.take() else { return false };
                if d.moved {
                    if config::get().copy_on_select {
                        self.copy_selection();
                    }
                    return shown;
                }
                let cleared = self.select(Select::Clear);
                let (x, y) = grid.point(d.from, 0.5);
                let target = self.engine.link(x, y);
                let link = target.as_deref().and_then(Link::of);
                if target.is_some() && link.is_none() {
                    self.notify("Only web, mail and file links open here");
                    return cleared;
                }
                if let Some(link) = link {
                    return self.open_link(link) | cleared;
                }
                self.press(x, y) | cleared
            }
        }
    }

    /// Sends `text` to the clipboard through pbcopy and through the terminal.
    /// The OSC 52 write sits between frames, so it never splits a tile.
    fn copy_text(&mut self, text: &str) {
        self.tty.write(clipboard::copy(text).as_bytes());
        self.tty.flush();
    }

    /// Copies the page's selection, if there is one.
    fn copy_selection(&mut self) {
        let text = self.engine.selection_text();
        if !text.is_empty() {
            self.copy_text(&text);
            self.notify("Copied the selection");
        }
    }

    /// Hit-tests a move only when it reaches a new spot, and redraws only the buttons whose hover changed.
    fn hover(&mut self, spot: Spot) -> bool {
        if self.hover_at == Some((spot, self.shown_px())) {
            return false;
        }
        self.hover_at = Some((spot, self.shown_px()));
        let page = !(self.app.help || self.app.expanded) && self.region(spot.cell) == Region::Page;
        if !page && !self.hover_page {
            self.set_pointer(DEFAULT_POINTER);
            return false;
        }
        self.hover_page = page;
        let (x, y) = if page { self.grid().point(spot, 0.5) } else { (-1.0, -1.0) };
        let (pointer, rects) = self.engine.hover(x, y);
        self.set_pointer(match pointer {
            Pointer::Text => "xterm",
            Pointer::Hand => self.probe.hand(),
            Pointer::Default => DEFAULT_POINTER,
        });
        if rects.is_empty() {
            return false;
        }
        self.invalidate(&rects);
        true
    }

    /// The page may have changed under the pointer, so the event loop hit-tests its last spot again.
    fn stale_hover(&mut self) {
        if let Some(h) = self.hover_at.as_mut() {
            h.1 = u32::MAX;
        }
    }

    fn set_pointer(&mut self, shape: &'static str) {
        if shape != self.pointer {
            self.pointer = shape;
            self.tty.write(term::pointer(shape).as_bytes());
        }
    }

    fn press(&mut self, x: f64, y: f64) -> bool {
        let hit = self.engine.press(x, y);
        if hit != Press::Nothing {
            self.last_press = None;
        }
        self.touched |= !matches!(hit, Press::Nothing | Press::Expand(_));
        match hit {
            Press::Nothing => false,
            Press::Copy { text, rects, label } => {
                self.copy_text(&text);
                self.notify(&format!("Copied {label}"));
                self.copy_until = Some(Instant::now() + COPY_FEEDBACK);
                self.invalidate(&rects);
                true
            }
            Press::Expand(i) => {
                let (top, height) = self.view_css();
                let rects = self.engine.expand(i, top, height);
                let shown = self.redraw(&rects);
                self.app.expanded = shown;
                shown
            }
            Press::Redraw(rects) => self.redraw(&rects),
            Press::Other => {
                self.relayout();
                false
            }
        }
    }

    fn copy_due(&mut self) -> bool {
        if self.copy_until.is_none_or(|due| Instant::now() < due) {
            return false;
        }
        self.copy_until = None;
        let rects = self.engine.reset_copy();
        self.redraw(&rects)
    }

    fn open_link(&mut self, link: Link) -> bool {
        match link {
            Link::Open(url) => {
                self.notify(&format!("Opened {url}"));
                link::open(url);
                false
            }
            Link::File(url) => self.follow(&url),
            // An empty `#` is the top, as in a browser.
            Link::Anchor(id) if id.is_empty() => self.jump_to(0),
            Link::Anchor(id) => {
                let Some(y) = self.engine.anchor_y(&id) else { return false };
                self.jump_to(self.row_of(y))
            }
        }
    }

    /// Scrolls to `row` as a link does, saving the place for Backspace.
    fn jump_to(&mut self, row: u32) -> bool {
        let from = self.app.scroll;
        let actions = self.app.jump(row);
        // A jump that didn't move saved no place to go back to.
        let moved = self.app.scroll != from;
        let show = self.act_all(actions);
        if moved {
            self.notify_with("Backspace or a click here goes back", OnClick::GoBack, NOTICE);
        }
        show
    }

    /// Redraws `rects` after a page change. Returns whether there were any.
    fn redraw(&mut self, rects: &[Rect]) -> bool {
        self.invalidate(rects);
        !rects.is_empty()
    }

    fn select(&mut self, sel: Select) -> bool {
        let (n, all) = self.engine.select(sel);
        if !self.cache.bands() {
            return self.redraw(&all);
        }
        let (before, after) = all.split_at(n.min(all.len()));
        let row = f64::from(self.cell_h) / self.scale;
        let rects: Vec<Rect> = changed_rects(before, after).into_iter().map(|r| [r[0], r[1] - row, r[2], r[3] + 2.0 * row]).collect();
        self.invalidate(&rects);
        !rects.is_empty()
    }

    fn set_matches(&mut self, m: Vec<Vec<Rect>>) {
        let all: Vec<Rect> = self.matches.iter().chain(&m).flatten().copied().collect();
        self.invalidate(&all);
        self.matches = m;
    }

    fn match_rows(&self) -> Vec<u32> {
        self.matches.iter().filter_map(|r| r.first()).map(|r| self.row_of(r[1])).collect()
    }

    fn row_of(&self, css_y: f64) -> u32 {
        let px = (css_y * self.scale).max(0.0) as u32;
        px.saturating_sub(self.phase) / self.cell_h
    }

    fn viewport_rows(&self) -> u32 {
        chrome::view_rows(self.size.rows) as u32
    }

    fn scroll_px(&self) -> u32 {
        self.phase + self.app.scroll * self.cell_h
    }

    /// Where the view is in the content, so a new layout can come back to it. None at the top.
    fn anchor(&self) -> Option<position::Anchor> {
        let at_px = self.scroll_px();
        if at_px > 0 { position::capture(&self.layout.blocks, f64::from(at_px) / self.scale) } else { None }
    }

    /// Where each Backspace row is in the content, as `anchor` takes the view's.
    fn back_anchors(&self) -> Vec<Option<position::Anchor>> {
        self.app.back_rows().iter().map(|&r| if r > 0 { position::capture(&self.layout.blocks, f64::from(self.phase + r * self.cell_h) / self.scale) } else { None }).collect()
    }

    /// Which part of the screen a cell is in.
    fn region(&self, (col, row): (u16, u16)) -> Region {
        if u32::from(row) > self.viewport_rows() {
            Region::Status
        } else if col <= self.img_col {
            Region::Sidebar
        } else {
            Region::Page
        }
    }

    /// Device px from the page's CSS px.
    fn px_of(&self, css: f64) -> u32 {
        (css * self.scale).round() as u32
    }

    /// Scrolls the page just measured to `anchor`, else to `fallback` device px.
    fn settle_at(&mut self, anchor: Option<position::Anchor>, fallback: u32) {
        let px = anchor.and_then(|a| position::restore(&self.layout.blocks, a)).map_or(fallback, |y| self.px_of(y));
        self.settle(px);
    }

    /// Puts the Backspace rows back on the same content after a new layout, with `line`
    /// mapping an old source line to the new source.
    fn restore_back(&mut self, anchors: &[Option<position::Anchor>], line: impl Fn(u32) -> u32) {
        let rows = self
            .app
            .back_rows()
            .iter()
            .zip(anchors)
            .map(|(&r, a)| {
                a.and_then(|a| position::restore(&self.layout.blocks, position::Anchor { line: line(a.line), offset: a.offset })).map_or(r, |y| self.px_of(y).saturating_sub(self.phase) / self.cell_h)
            })
            .collect();
        self.app.set_back_rows(rows);
    }

    /// Where the view is drawn: the glide's position while one runs, else `scroll_px`.
    fn shown_px(&self) -> u32 {
        self.glide.map_or_else(|| self.scroll_px(), |g| g.1)
    }

    fn view_px(&self) -> u32 {
        self.viewport_rows() * self.cell_h
    }

    fn view_css(&self) -> (f64, f64) {
        (f64::from(self.shown_px()) / self.scale, f64::from(self.view_px()) / self.scale)
    }

    fn reload_if_changed(&mut self) {
        let Ok(bytes) = std::fs::read(&self.file) else {
            if !self.app.missing {
                self.app.missing = true;
                self.draw_chrome();
            }
            return;
        };
        let was_missing = std::mem::take(&mut self.app.missing);
        if bytes == self.stamp && self.loaded {
            if was_missing {
                self.draw_chrome();
            }
            return;
        }
        let src = String::from_utf8_lossy(&bytes).into_owned();
        self.render_doc(bytes, src, true);
        self.notify("Reloaded");
        trace::mark("reloaded");
    }

    fn base(&mut self) -> PathBuf {
        if !self.stdin
            && let Some(dir) = std::fs::canonicalize(&self.file).ok().and_then(|p| p.parent().map(Path::to_path_buf))
        {
            self.base = dir;
        }
        self.base.clone()
    }

    fn live(&self) -> bool {
        !self.stdin
    }

    fn rerender_keep(&mut self) {
        self.rerender(self.live());
    }

    fn rerender(&mut self, read: bool) {
        let bytes = read.then(|| std::fs::read(&self.file).ok()).flatten().unwrap_or_else(|| self.stamp.clone());
        let src = if bytes == self.stamp { self.src.clone() } else { String::from_utf8_lossy(&bytes).into_owned() };
        self.render_doc(bytes, src, true);
    }

    fn notify(&mut self, msg: &str) {
        self.notify_with(msg, OnClick::Hints, NOTICE);
    }

    fn notify_with(&mut self, msg: &str, click: OnClick, shown: Duration) {
        self.app.notice = Some(Notice { text: msg.to_string(), click });
        self.notice_until = Some(Instant::now() + shown);
        self.draw_chrome();
    }

    /// Lays out `src` in place of the page, keeping the reading position when `keep` is set.
    fn render_doc(&mut self, bytes: Vec<u8>, src: String, keep: bool) {
        let code = if keep { self.engine.code_positions() } else { Vec::new() };
        let old = std::mem::take(&mut self.src);
        let Place { view: anchor, back, src: old_src } = match self.restore_at.take() {
            Some(place) => place,
            None if keep => Place { view: self.anchor(), back: self.back_anchors(), src: old },
            None => Place { view: None, back: Vec::new(), src: old },
        };
        let (toc, css_width) = self.fit();
        let base = self.base();
        let theme = config::theme();
        self.theme = theme;
        self.size_engine(css_width);
        self.app.expanded = false;
        self.touched = false;
        self.engine.load(&src, &base, self.app.wrap, theme);
        self.settle_scale();
        trace::mark("page loaded");
        if !code.is_empty() {
            self.engine.code_restore(&code);
        }
        self.measure();
        self.stamp = bytes;
        self.loaded = true;
        self.src = src;
        let line = position::line_map(&old_src, &self.src);
        let landing = self.land_on.take().and_then(|id| self.engine.anchor_y(&id)).map_or(0, |y| self.px_of(y));
        self.settle_at(anchor.map(|a| position::Anchor { line: line(a.line), offset: a.offset }), landing);
        self.restore_back(&back, line);
        if self.app.toc_cols(self.size.cols) != toc {
            self.rerender_keep();
            return;
        }
        self.late = (self.engine.pending_images() > 0).then(|| Instant::now() + LATE_LIMIT);
        self.stale_hover();
        self.show();
    }

    /// The first page lays out at a guessed scale while a thread reads the real one. A wrong
    /// guess lays the page out again here, before anything of it is drawn.
    fn settle_scale(&mut self) {
        let real = screen::display_scale();
        if real == self.display {
            return;
        }
        trace::mark("scale guess was wrong");
        self.display = real;
        let (_, css_width) = self.fit();
        self.engine.rescale(self.scale, css_width);
        if let Some(l) = &self.loader {
            l.tune(self.scale, (self.cell_w, self.cell_h));
        }
    }

    /// Takes the terminal's size in cells and, when it tells, in pixels: a font size change keeps
    /// the cells but not their pixels, which the pixel mouse divides by. The page scale follows the
    /// cell height, so the page zooms with the terminal's font.
    fn take_size(&mut self) {
        self.size = self.tty.size();
        // Rex starts a block opened from its CLI with no pixel size and a stand-in cell until a window shows it.
        // The font zoom counts from the first size with pixels, or else the first real cell reads as a zoom.
        if (self.start.xpix == 0 || self.start.ypix == 0) && self.size.xpix > 0 && self.size.ypix > 0 {
            self.start = self.size;
        }
        (self.cell_w, self.cell_h) = self.probe.cell_px(self.size, self.display);
        let base = self.probe.cell_px(self.start, self.display).1;
        self.scale = page_scale(self.display, self.cell_h, base);
        if trace::enabled() {
            let s = self.size;
            trace::mark(&format!("size {}x{} cells, {}x{} px, cell {}x{} px, probe cell {:?}, scale {}", s.cols, s.rows, s.xpix, s.ypix, self.cell_w, self.cell_h, self.probe.cell, self.scale));
        }
    }

    /// Gives the engine the page width, and the page scale when a font zoom changed it.
    fn size_engine(&self, css_width: f64) {
        if self.engine.scale.get() == self.scale {
            self.engine.resize(css_width);
        } else {
            self.engine.rescale(self.scale, css_width);
        }
    }

    /// Takes the terminal's size and returns the TOC width and the page width in CSS pixels.
    fn fit(&mut self) -> (u16, f64) {
        let before = (self.size, self.img_col);
        self.take_size();
        let toc = self.app.toc_cols(self.size.cols);
        self.img_col = toc;
        self.img_cols = self.size.cols.saturating_sub(toc).max(1);
        // Some terminals draw the old text over the new tiles, or keep it at rows the status row left.
        self.help_drawn |= before != (self.size, toc);
        (toc, f64::from(u32::from(self.img_cols) * self.cell_w) / self.scale)
    }

    /// Lays the loaded page out again at the new terminal size, without a reload. Returns false
    /// when only a reload gives the right result: the file changed, the macOS appearance changed
    /// under a light and dark theme pair, a diagram is expanded, or a click or a selection changed the page.
    fn resize_in_place(&mut self) -> bool {
        if self.app.expanded || self.touched || !std::ptr::eq(config::theme(), self.theme) || !self.loaded || self.engine.has_selection() {
            return false;
        }
        if self.live() && std::fs::read(&self.file).ok().is_none_or(|b| b != self.stamp) {
            return false;
        }
        let anchor = self.anchor();
        let back = self.back_anchors();
        let (toc, css_width) = self.fit();
        self.size_engine(css_width);
        self.measure();
        self.stale_hover();
        self.settle_at(anchor, 0);
        self.restore_back(&back, |l| l);
        if self.app.toc_cols(self.size.cols) != toc {
            self.rerender_keep();
            return true;
        }
        self.show();
        true
    }

    fn settle(&mut self, scroll_px: u32) {
        self.glide = None;
        // A layout after the first (a <details> opened, a resize) can start fetches of its own.
        if self.late.is_none() && self.engine.pending_images() > 0 {
            self.late = Some(Instant::now() + LATE_LIMIT);
        }
        self.phase = scroll_px % self.cell_h;
        let headings: Vec<Heading> = self.layout.headings.iter().map(|(l, t, y)| Heading { level: *l, title: t.clone(), row: self.row_of(*y) }).collect();
        self.app.layout(self.viewport_rows(), self.doc_px.saturating_sub(self.phase).div_ceil(self.cell_h), scroll_px / self.cell_h, &headings);
        self.rerun_search();
        self.retile();
    }

    fn rerun_search(&mut self) {
        if self.app.query.is_empty() {
            return;
        }
        self.matches = self.engine.search(&self.app.query, Some(self.app.current));
        self.app.matches = self.match_rows();
        if self.app.current >= self.app.matches.len() {
            self.app.current = 0;
        }
    }

    fn measure(&mut self) {
        self.layout = self.engine.layout();
        self.doc_px = (self.layout.height * self.scale).ceil() as u32;
        if trace::enabled() {
            trace::mark(&format!("page height {} px, width {} px", self.layout.height, f64::from(u32::from(self.img_cols) * self.cell_w) / self.scale));
        }
    }

    fn tile_height(&self) -> u32 {
        let width = u32::from(self.img_cols) * self.cell_w;
        // One row less than fits, for the row each tile is sent with below it.
        let rows = kitty::tile_rows(width, self.cell_h);
        rows.saturating_sub(1).max(1) * self.cell_h
    }

    fn retile(&mut self) {
        let mut out = String::new();
        let total = self.phase + self.app.doc_rows.max(1) * self.cell_h + app::PADDING_ROWS * self.cell_h;
        self.cache.retile(&mut out, self.phase, total, self.tile_height(), self.cell_h);
        self.tty.write(out.as_bytes());
    }

    fn invalidate(&mut self, rects: &[Rect]) {
        for r in rects {
            self.cache.invalidate((r[1] * self.scale).floor() as u32, ((r[1] + r[3]) * self.scale).ceil() as u32);
        }
    }

    fn view(&self) -> View {
        let scroll_px = self.shown_px();
        View { scroll_px, bottom: scroll_px + self.view_px(), cell_h: self.cell_h, col: self.img_col, cols: self.img_cols, width: u32::from(self.img_cols) * self.cell_w }
    }

    fn show(&mut self) {
        let t0 = Instant::now();
        // A glide that ended before its first frame (a later key, a resize, a re-layout) leaves its path unused.
        let mut keep = std::mem::take(&mut self.path);
        if self.glide.is_none() {
            keep.clear();
        }
        let v = self.view();
        self.drawn = v.scroll_px;
        let vis = self.cache.visible(&v);
        let mut frees = String::new();
        if self.probe.sync {
            frees.push_str(term::SYNC_BEGIN);
        }
        let loader = self.loader.take();
        if loader.is_some() {
            frees.push_str(&format!("{}\x1b[2J", self.theme.palette.reset()));
        }
        let todo = if self.app.help { vec![] } else { self.cache.plan_bands(&vis, self.last_event.elapsed() >= FOLD_IDLE, self.phase, self.cell_h, &mut frees) };
        self.cache.free_stale(&mut frees);
        if self.app.help {
            drop(loader);
            let mut out = frees;
            self.cache.free_all(&mut out);
            chrome::help(&mut out, self.size.cols, self.size.rows, &mut self.app.help_top, &self.theme.palette);
            self.help_drawn = true;
            self.chrome().draw(&mut out);
            if self.probe.sync {
                out.push_str(term::SYNC_END);
            }
            self.tty.write(out.as_bytes());
            self.tty.flush();
            return;
        }
        if self.listing
            && let Some(p) = self.picker.as_mut()
        {
            let mut out = frees;
            self.cache.free_all(&mut out);
            p.draw(&mut out, self.size.cols, self.size.rows, std::time::SystemTime::now(), &config::theme().palette);
            self.help_drawn = true;
            if self.probe.sync {
                out.push_str(term::SYNC_END);
            }
            self.tty.write(out.as_bytes());
            self.tty.flush();
            return;
        }
        if std::mem::take(&mut self.help_drawn) {
            frees.push_str(&format!("{}\x1b[2J", self.theme.palette.reset()));
        }
        let mut snap = snapper(&self.engine, self.scale);
        let first = !keep.is_empty();
        keep.extend(&vis);
        let mut b = Batch { keep: &keep, scroll_px: v.scroll_px, snap: &mut snap, bytes: Vec::new(), cmds: frees };
        for &i in &keep {
            if !self.cache.is_resident(i) {
                self.cache.upload(i, &mut b);
            }
        }
        for (i, top, height) in todo {
            if self.cache.is_resident(i) {
                self.cache.upload_band(i, top, height, &mut b);
            }
        }
        let (frees, bytes) = (b.cmds, b.bytes);
        drop(snap);
        if let Some(l) = loader {
            l.finish();
        }
        self.tty.write(frees.as_bytes());
        self.tty.write(&bytes);
        let mut out = String::new();
        self.cache.place(&vis, &v, &mut out);
        self.chrome().draw(&mut out);
        if self.probe.sync {
            out.push_str(term::SYNC_END);
        }
        self.tty.write(out.as_bytes());
        self.tty.flush();
        // The glide's clock starts once the tiles it passes over are sent, so a slow send doesn't eat into it.
        if first && let Some((g, _)) = &mut self.glide {
            g.restart(Instant::now());
        }
        if trace::enabled() {
            let y = self.view_css().0;
            let held = self.cache.held();
            let top = position::top_text(&self.layout.blocks, &self.layout.texts, y);
            trace::log(format_args!(
                "view px={} took={:.2} scroll={} phase={} placed={:?} resident={} held={held} scale={} cell={}x{} rows={} doc={} top={top}",
                v.scroll_px,
                t0.elapsed().as_secs_f64() * 1000.0,
                self.app.scroll,
                self.phase,
                self.cache.placed(),
                self.cache.resident(),
                self.scale,
                self.cell_w,
                self.cell_h,
                self.viewport_rows(),
                self.doc_px
            ));
            if let Some(path) = std::env::var_os("MDDOG_DUMP_VIEW") {
                self.dump_view(Path::new(&path));
            }
        }
    }

    fn dump_view(&self, path: &Path) {
        let (y, h) = self.view_css();
        if let Some(png) = self.engine.snapshot(y, h, self.scale, |n| Some(vec![0; n])).and_then(|b| kitty::png(b.width, b.height, &b.rgba)) {
            let _ = std::fs::write(path, png);
        }
    }

    fn chrome(&self) -> Chrome<'_> {
        Chrome { app: &self.app, name: &self.name, cols: self.size.cols, rows: self.size.rows, img_col: self.img_col, palette: self.theme.palette }
    }

    fn draw_chrome(&mut self) {
        if self.listing {
            return;
        }
        let mut out = String::new();
        self.chrome().draw(&mut out);
        self.tty.write(out.as_bytes());
        self.tty.flush();
    }

    /// Glides from where the view is drawn to `scroll_px`, or jumps there when the terminal can't
    /// draw a frame at once (no mode 2026), or the tiles along the way don't fit in the storage limit together.
    fn start_glide(&mut self) {
        let to = self.scroll_px();
        // A jump during a glide adds to it, so the screen it may cover grows by what that glide had left.
        let left = self.glide.take().map_or(0, |(g, _)| g.to().abs_diff(self.drawn));
        if !self.probe.sync || self.drawn == to {
            return;
        }
        let now = Instant::now();
        let pace = app::Pace::for_jump(self.last_jump.replace(now), now, app::GLIDE_PER_PAGE, self.view_px());
        let g = app::Glide::new(self.drawn, to, self.view_px() + left, pace, now);
        let (lo, hi) = (g.from().min(to), g.from().max(to));
        let path = View { scroll_px: lo, bottom: hi + self.view_px() + self.cell_h, ..self.view() };
        let need = self.cache.visible(&path);
        if !self.cache.fits(&need, path.width) {
            return;
        }
        self.path = need;
        self.glide = Some((g, g.from()));
    }

    /// Sends the tiles `todo` now, keeping `keep` from eviction.
    fn send_tiles(&mut self, todo: &[usize], keep: &[usize], scroll_px: u32) {
        let mut snap = snapper(&self.engine, self.scale);
        let mut b = Batch { keep, scroll_px, snap: &mut snap, bytes: Vec::new(), cmds: String::new() };
        let mut sent = false;
        for &i in todo {
            sent |= self.cache.upload(i, &mut b);
        }
        if sent {
            self.tty.write(b.cmds.as_bytes());
            self.tty.write(&b.bytes);
            self.tty.flush();
        }
    }

    fn tick(&mut self) {
        if let Some((g, last)) = self.glide {
            if self.app.help || self.listing {
                self.glide = None;
            } else {
                let (px, done) = g.at(Instant::now());
                // The eased position reaches `to` a frame or two before the clock runs out, so the glide ends there.
                let done = done || px == g.to();
                self.glide = (!done).then_some((g, px));
                // Near the end the eased position rounds to the same pixel for a few frames.
                if px != last || done {
                    self.show();
                }
            }
        }
        if self.last_event.elapsed() >= FOLD_IDLE && self.cache.has_bands() {
            self.cache.fold();
            self.show();
        }
        if self.resize.due(Instant::now()) && self.tty.size() != self.size {
            trace::mark("resize");
            if !self.opened {
                self.take_size();
                self.show();
            } else if !self.resize_in_place() {
                self.rerender_keep();
            }
        }
        let (poll, late) = image_wait(self.late, self.engine.pending_images(), Instant::now());
        if poll {
            // Each engine relayout gets a measure here before the next tile goes out, so tiles and offsets agree.
            if self.engine.take_images() {
                self.relayout();
            }
            self.late = late;
            if late.is_some() {
                return;
            }
        }
        if let Some(i) = self.prefetch_target() {
            let v = self.view();
            let vis = self.cache.visible(&v);
            self.send_tiles(&[i], &vis, v.scroll_px);
        }
    }

    fn prefetch_target(&self) -> Option<usize> {
        if self.app.help || self.listing || self.glide.is_some() {
            return None;
        }
        self.cache.prefetch(self.scroll_px(), u32::from(self.img_cols) * self.cell_w)
    }

    fn relayout(&mut self) {
        let scroll_px = self.scroll_px();
        let anchor = self.anchor();
        let back = self.back_anchors();
        // The expanded diagram's box was measured on the old page.
        if self.app.expanded {
            self.app.expanded = false;
            self.engine.collapse();
        }
        self.measure();
        self.stale_hover();
        self.settle_at(anchor, scroll_px);
        self.restore_back(&back, |l| l);
        self.show();
        trace::mark("late content settled");
    }

    fn leave_screen(&mut self) {
        self.paused.store(true, Ordering::SeqCst);
        // Past one input poll, the input thread has seen `paused` and leaves the editor's keys alone.
        std::thread::sleep(INPUT_POLL + Duration::from_millis(10));
        let mut out = String::new();
        self.cache.free_all(&mut out);
        self.tty.write(out.as_bytes());
        self.tty.write(term::leave().as_bytes());
        self.tty.flush();
        self.tty.restore();
        self.pointer = DEFAULT_POINTER;
        self.stale_hover();
    }

    fn enter_screen(&mut self, read: bool) {
        let _ = self.tty.raw();
        self.tty.write(term::enter(self.probe.pixel_mouse).as_bytes());
        self.tty.flush();
        let pending: Vec<Event> = std::iter::from_fn(|| self.rx.try_recv().ok()).filter(|e| !matches!(e, Event::FileChanged)).collect();
        for e in pending {
            let _ = self.tx.send(e);
        }
        self.paused.store(false, Ordering::SeqCst);
        if self.opened {
            self.rerender(read);
        } else {
            self.take_size();
            self.show();
        }
    }

    fn edit(&mut self) {
        let before = self.stamp.clone();
        self.leave_screen();
        trace::mark("editor start");
        let result = editor::run(&self.file);
        trace::mark("editor exit");
        self.enter_screen(true);
        match result {
            Err(e) => self.notify(&e),
            Ok(()) if self.stamp != before && self.loaded => self.notify("Reloaded"),
            Ok(()) => {}
        }
        trace::mark("editor reload");
    }

    fn suspend(&mut self) {
        self.leave_screen();
        // SAFETY: kill takes plain integers; pid 0 sends SIGTSTP to this process group.
        unsafe { libc::kill(0, libc::SIGTSTP) };
        let live = self.live();
        self.enter_screen(live);
    }

    /// Opens a file from the list.
    fn pick(&mut self, path: PathBuf) {
        match std::fs::read(&path) {
            Ok(bytes) => self.open(path, bytes, None),
            Err(e) => {
                if let Some(p) = self.picker.as_mut() {
                    p.note = Some(format!("Can't open {}: {}", path.display(), term::io_error(&e)));
                }
                self.show();
            }
        }
    }

    /// Follows a link to a local file. Only Markdown opens, here; nothing local goes to the system's `open`.
    /// A link to the page shown jumps within it, as a `#` link does. Returns whether to show.
    fn follow(&mut self, url: &str) -> bool {
        match link::follow(url, &self.base) {
            link::Follow::Refuse(why) => self.notify(&why),
            link::Follow::Show(path, heading) if !self.stdin && std::fs::canonicalize(&self.file).is_ok_and(|f| f == path) => {
                // At the heading, else the top, as opening the file would land.
                let y = heading.and_then(|id| self.engine.anchor_y(&id)).unwrap_or(0.0);
                return self.jump_to(self.row_of(y));
            }
            link::Follow::Show(path, heading) => match std::fs::read(&path) {
                Ok(bytes) => self.open(path, bytes, heading),
                Err(e) => self.notify(&format!("Can't open {}: {}", path.display(), term::io_error(&e))),
            },
        }
        false
    }

    /// Shows `bytes`, read from `path`, at the top or at the heading with id `heading`. The file shown before goes on
    /// the Back history.
    fn open(&mut self, path: PathBuf, bytes: Vec<u8>, heading: Option<String>) {
        let place = Place { view: self.anchor(), back: self.back_anchors(), src: self.src.clone() };
        let mut old = self.replace_app(App::default());
        // Its timer runs on, so coming back would show it with none.
        old.notice = None;
        if self.opened {
            let visit = Visit { file: std::mem::take(&mut self.file), stdin: self.stdin, name: std::mem::take(&mut self.name), app: old, place };
            self.history.push(visit);
            if self.history.len() > app::HISTORY {
                self.history.remove(0);
            }
        }
        self.app.back_file = !self.history.is_empty();
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.start_loader(&name);
        self.land_on = heading;
        self.switch(path, false, name, bytes, None);
    }

    fn back_file(&mut self) {
        let Some(v) = self.history.pop() else { return };
        let Ok(bytes) = std::fs::read(&v.file) else {
            // Drop it, so the next Backspace reaches the files before it.
            self.app.back_file = !self.history.is_empty();
            self.notify(&format!("Can't read {}, so it was removed from the Back list", v.name));
            return;
        };
        self.replace_app(v.app);
        self.app.back_file = !self.history.is_empty();
        self.start_loader(&v.name);
        self.switch(v.file, v.stdin, v.name, bytes, Some(v.place));
    }

    /// Puts `next` in place of the page's state, keeping the settings that hold across files. Returns the old state.
    fn replace_app(&mut self, mut next: App) -> App {
        (next.wrap, next.toc_visible) = (self.app.wrap, self.app.toc_visible);
        std::mem::replace(&mut self.app, next)
    }

    /// Draws a skeleton over the screen if the next page is slow to show. `show` takes it down.
    fn start_loader(&mut self, name: &str) {
        let l = Loader::start(self.tty.fd, name);
        l.tune(self.scale, (self.cell_w, self.cell_h));
        self.loader = Some(l);
    }

    fn switch(&mut self, file: PathBuf, stdin: bool, name: String, bytes: Vec<u8>, at: Option<Place>) {
        self.base = crate::render::images::base_dir(&file, stdin);
        (self.file, self.stdin, self.name) = (file, stdin, name);
        self.matches.clear();
        self.late = None;
        self.loaded = false;
        self.src.clear();
        self.stamp.clear();
        self.restore_at = at;
        if let Some(p) = &self.picker {
            p.halt();
        }
        self.listing = false;
        self.opened = true;
        let src = String::from_utf8_lossy(&bytes).into_owned();
        self.render_doc(bytes, src, false);
        self.watch();
        trace::mark("file opened");
    }

    fn shutdown(&mut self) {
        self.loader = None;
        let mut out = String::new();
        self.cache.free_all(&mut out);
        self.tty.write(out.as_bytes());
        self.tty.write(term::leave().as_bytes());
        self.tty.flush();
        self.tty.restore();
        self.watcher = None;
    }
}

fn changed_rects(before: &[Rect], after: &[Rect]) -> Vec<Rect> {
    let mut rest = after.to_vec();
    let mut out = Vec::new();
    for r in before {
        match rest.iter().position(|x| x == r) {
            Some(k) => {
                rest.remove(k);
            }
            None => out.push(*r),
        }
    }
    out.extend(rest);
    out
}

fn input_loop(fd: i32, tx: Sender<Event>, paused: Arc<AtomicBool>, first: Vec<u8>) {
    let mut buf = first;
    let mut chunk = [0u8; 1024];
    loop {
        if paused.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        // A lone Esc is a key once no sequence follows it for 30 ms.
        let wait = if buf.last() == Some(&0x1b) { Duration::from_millis(30) } else { INPUT_POLL };
        let ready = term::readable(fd, wait);
        if ready && !paused.load(Ordering::SeqCst) {
            // SAFETY: chunk is writable for chunk.len() bytes, and read writes no more than that.
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n <= 0 {
                let _ = tx.send(Event::Quit);
                return;
            }
            buf.extend_from_slice(&chunk[..n as usize]);
        }
        for input in term::parse_input(&mut buf, !ready) {
            let e = match input {
                Input::Key(k) => Event::Key(k),
                Input::Gone(id) => Event::Gone(id),
            };
            if tx.send(e).is_err() {
                return;
            }
        }
    }
}

/// Whether `tick` takes arrived remote images, and until when the page still waits on them. Taking goes on while
/// any fetch is pending, since a slow host can answer after `LATE_LIMIT`. The wait only keeps the tick fast and
/// holds back prefetch.
fn image_wait(late: Option<Instant>, pending: usize, now: Instant) -> (bool, Option<Instant>) {
    (late.is_some() || pending > 0, late.filter(|&until| pending > 0 && now <= until))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_after_the_late_limit_still_arrive() {
        let now = Instant::now();
        let past = now - Duration::from_secs(1);
        // An image still loading past the limit is still taken, though the page no longer waits on it.
        assert_eq!(image_wait(Some(past), 1, now), (true, None));
        assert_eq!(image_wait(None, 1, now), (true, None));
        assert_eq!(image_wait(Some(now + LATE_LIMIT), 2, now), (true, Some(now + LATE_LIMIT)));
        assert_eq!(image_wait(Some(now + LATE_LIMIT), 0, now), (true, None));
        assert_eq!(image_wait(None, 0, now), (false, None));
    }

    #[test]
    fn the_page_scale_follows_the_cell_height() {
        assert_eq!(page_scale(2.0, 38, 38), 2.0);
        assert_eq!(page_scale(2.0, 57, 38), 3.0);
        assert_eq!(page_scale(2.0, 19, 38), 1.0);
        assert_eq!(page_scale(1.0, 40, 32), 1.25);
        assert_eq!(page_scale(2.0, 38, 0), 2.0);
    }

    #[test]
    fn maps_cells_to_page_points() {
        let g = Grid { col0: 30, cell_w: 20, cell_h: 40, top: 100, scale: 2.0 };
        let at = |col, row| Spot { cell: (col, row), sub: None };
        assert_eq!(g.point(at(31, 1), 0.0), (0.0, 60.0));
        assert_eq!(g.point(at(33, 3), 0.5), (25.0, 100.0));
        assert_eq!(g.point(at(33, 3), 1.0), (30.0, 100.0));
        assert_eq!(g.point(at(5, 1), 0.0), (0.0, 60.0));
    }

    #[test]
    fn pixel_reports_map_to_cells() {
        assert_eq!(pixel_spot((50, 95), (20, 40), (80, 24)), Spot { cell: (3, 3), sub: Some((0.5, 0.375)) });
        assert_eq!(pixel_spot((1600, 10), (20, 40), (80, 24)), Spot { cell: (80, 1), sub: None });
        assert_eq!(pixel_spot((10, 960), (20, 40), (80, 24)), Spot { cell: (1, 24), sub: None });
    }

    #[test]
    fn a_pixel_report_overrides_the_cell_center() {
        let g = Grid { col0: 30, cell_w: 20, cell_h: 40, top: 100, scale: 2.0 };
        let spot = Spot { cell: (33, 3), sub: Some((0.25, 0.75)) };
        assert_eq!(g.point(spot, 0.5), (22.5, 105.0));
        assert_eq!(g.point(spot, 1.0), (22.5, 105.0));
    }

    #[test]
    fn only_changed_selection_rects_count() {
        let (a, b, c) = ([0.0, 0.0, 100.0, 19.0], [0.0, 19.0, 100.0, 19.0], [0.0, 38.0, 40.0, 19.0]);
        assert_eq!(changed_rects(&[a, b, [0.0, 38.0, 20.0, 19.0]], &[a, b, c]), vec![[0.0, 38.0, 20.0, 19.0], c]);
        assert_eq!(changed_rects(&[a, a], &[a]), vec![a]);
        assert_eq!(changed_rects(&[], &[a]), vec![a]);
    }
}
