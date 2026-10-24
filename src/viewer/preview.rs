use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::app::{self, Action, App, Heading};
use super::cache::{Batch, Cache, View};
use super::chrome::{self, Chrome};
use super::loading::Loader;
use super::page;
use super::picker::{self, Pick, Picker};
use super::position;
use super::remote;
use super::script::{self, Layout, Press, Rect};
use super::sizes::Sizes;
use crate::engine::{self, Engine};
use crate::render::markdown;
use crate::term::kitty;
use crate::term::{self, Click, Event, Key, Probe, Size, Tty};
use crate::theme::Theme;
use crate::{config, editor, trace, watch};

const LATE_LIMIT: Duration = Duration::from_secs(60);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const COPY_FEEDBACK: Duration = Duration::from_secs(2);
const NOTICE: Duration = Duration::from_secs(3);
const PROBE_WAIT: Duration = Duration::from_secs(1);
const CLICK_QUIT: &str = "Click again to quit";
const CLICK_AGAIN: Duration = Duration::from_millis(300);
/// The time between a glide's frames.
const GLIDE_FRAME: Duration = Duration::from_millis(8);
const GO_BACK: &str = "Backspace or a click here goes back";
const FOLD_IDLE: Duration = Duration::from_secs(1);
/// The X11 name for the arrow, sent with OSC 22 off text, links and buttons.
const DEFAULT_POINTER: &str = "left_ptr";
/// How many files Backspace can go back through, as many as the jumps inside one file.
const HISTORY: usize = 50;
const UNSUPPORTED: &str = "mdd: this terminal can't show images, so it can't show the preview. mddog needs the Kitty graphics protocol: use Ghostty, Kitty, WezTerm, or iTerm2. Terminal.app, tmux, and screen don't support it.";

/// A terminal that shows kitty images but garbles a scrolled page, with the reason.
fn garbles_scrolling() -> Option<&'static str> {
    if std::env::var_os("ZELLIJ").is_some() {
        Some("zellij 0.45 garbles scrolled images (zellij-org/zellij#5573 and #5648)")
    } else if std::env::var("TERM_PROGRAM").as_deref() == Ok("WarpTerminal") {
        Some("Warp ignores the crop that mddog scrolls an image with")
    } else {
        None
    }
}

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

#[derive(Debug, PartialEq)]
enum Link {
    Open(String),
    Anchor(String),
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

fn classify_link(raw: &str, resolved: &str) -> Option<Link> {
    if let Some(id) = raw.strip_prefix('#') {
        return Some(Link::Anchor(id.to_string()));
    }
    let scheme = resolved.split(':').next()?.to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https" | "mailto").then(|| Link::Open(resolved.to_string()))
}

fn open_url(url: String) {
    std::thread::spawn(move || {
        let _ = Command::new("open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

fn pasteable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r')).collect()
}

/// OSC 52 puts `text` on the clipboard of the terminal the user sits at, also over SSH.
fn osc52(text: &str) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::from("\x1b]52;c;");
    for c in text.as_bytes().chunks(3) {
        let n = c.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() { char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out + "\x1b\\"
}

fn pbcopy(text: String) {
    std::thread::spawn(move || {
        let Ok(mut child) = Command::new("pbcopy").env("LC_CTYPE", "UTF-8").stdin(Stdio::piped()).spawn() else { return };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
}

/// A file the viewer left for another one, to go back to.
struct Visit {
    file: PathBuf,
    stdin: bool,
    name: String,
    app: App,
    anchor: Option<position::Anchor>,
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
    scale: f64,
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
    /// Where the last move was hit-tested, with the scroll it was drawn at, so a move within the same spot does nothing.
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
    tmp_html: PathBuf,
    doc: Option<markdown::Doc>,
    /// The loaded page's theme.
    theme: &'static Theme,
    /// A click changed the page (a copy, a disclosure, a button) since it loaded, which a reload undoes.
    touched: bool,
    late: Option<(String, Instant)>,
    /// Read from disk at the first obsidian-engine diagram.
    sizes: Option<Sizes>,
    frame_due: bool,
    resize: app::Debounce,
    quit: bool,
    /// Whether a file is loaded. False while the list that `mdd` started with is up.
    opened: bool,
    picker: Option<Picker>,
    listing: bool,
    history: Vec<Visit>,
    /// Where `render_doc` scrolls to instead of the top, with the source it was taken from.
    restore_at: Option<(position::Anchor, String)>,
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

/// Starts with the list of Markdown files under `root` instead of a file.
/// Whether input typed during the first render asks to quit, in either key encoding.
fn quits(input: &[u8]) -> bool {
    term::parse_keys(&mut input.to_vec(), false).iter().any(|k| matches!(k, Key::Ctrl('c') | Key::Char('q' | 'Q')))
}

pub fn browse(root: PathBuf) -> i32 {
    start(PathBuf::new(), false, Some(root))
}

fn start(file: PathBuf, stdin: bool, list: Option<PathBuf>) -> i32 {
    trace::start();
    let spare = engine::spare::connect();
    let (tx, rx) = mpsc::channel();
    let paused = Arc::new(AtomicBool::new(false));
    if let Ok(mut signals) = signal_hook::iterator::Signals::new([libc::SIGWINCH, libc::SIGTERM, libc::SIGINT, libc::SIGHUP]) {
        let (tx, paused) = (tx.clone(), paused.clone());
        std::thread::spawn(move || {
            for s in signals.forever() {
                if s == libc::SIGINT && paused.load(Ordering::SeqCst) {
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
    let remote = spare.as_ref().and_then(engine::spare::Client::renderer);
    let mut picker = list.map(Picker::new);
    if let Some(p) = picker.as_mut() {
        p.list(tx.clone());
    }
    let _ = picker.is_none().then(|| {
        std::thread::Builder::new().stack_size(64 << 20).spawn(move || {
            let bytes = std::fs::read(&f2);
            let _ = worker_tx.send(bytes.map(|b| {
                let (src, doc) = page::doc(&f2, &b, &page::base_dir(&f2, stdin), |s, d| remote.and_then(|mut r| engine::spare::render(&mut r, s, d)));
                (b, src, doc)
            }));
        })
    });
    // The first read of the macOS appearance takes about 2 ms, so make it while the file renders.
    std::thread::spawn(config::theme);
    let mut tty = match Tty::open() {
        Ok(t) => t,
        Err(e) => {
            engine::spare::spawn();
            eprintln!("mdd: needs a terminal to show the preview ({}). It can't write to a pipe or a file. Run it in a terminal window.", term::io_error(&e));
            return 1;
        }
    };
    let _ = tty.raw();
    term::on_panic_restore(tty.saved());
    tty.write(format!("\x1b[?1049h\x1b[?7l\x1b[?25l\x1b[2J\x1b[H{}{}", term::MOUSE_ON, term::KEYS_ON).as_bytes());
    let name = if stdin { "standard input".to_string() } else { file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned()) };
    let loader = match picker.as_mut() {
        Some(p) => {
            // The list shows before WebKit starts, so it is up at once.
            p.first_batch(Duration::from_millis(50));
            let mut out = String::new();
            p.draw(&mut out, tty.size().cols, tty.size().rows, std::time::SystemTime::now(), &config::theme().palette);
            tty.write(out.as_bytes());
            tty.flush();
            None
        }
        None => Some(Loader::start(tty.fd, &name)),
    };
    let shm_name = kitty::shm_name();
    let temp = kitty::temp_path("q");
    let _ = kitty::shm_write(&shm_name, &[0, 0, 0]);
    let _ = kitty::write_private(Path::new(&temp), &[0, 0, 0]);
    trace::mark("tty ready");
    let engine = spare.map_or_else(
        || {
            let e = Engine::new(800.0);
            e.load_blank();
            e
        },
        Engine::Remote,
    );
    trace::mark("engine created");
    let (probe, leftover) = tty.probe(&shm_name, &temp, PROBE_WAIT);
    if probe.pixel_mouse {
        tty.write(term::PIXEL_MOUSE_ON.as_bytes());
    }
    kitty::shm_unlink(&shm_name);
    let _ = std::fs::remove_file(&temp);
    trace::mark("terminal probed");
    let refusal = if probe.no_graphics() {
        Some(UNSUPPORTED.to_string())
    } else {
        garbles_scrolling().map(|why| format!("mdd: {why}, so it can't show the preview here. Use Ghostty, Kitty, WezTerm, or iTerm2."))
    };
    if let Some(refusal) = refusal {
        drop(loader);
        drop(picker);
        tty.write(term::leave().as_bytes());
        tty.flush();
        tty.restore();
        engine::spare::spawn();
        eprintln!("{}", term::wrap(&refusal, usize::from(tty.size().cols)));
        return 1;
    }
    let size = tty.size();
    let scale = engine.scale();
    let (cell_w, cell_h) = probe.cell_px(size, scale);
    if let Some(l) = &loader {
        l.tune(scale, (cell_w, cell_h));
    }
    let herdr = probe.herdr();
    let cache = Cache::new(kitty::storage_limit(herdr), herdr, probe.medium(), 1 + (std::process::id() % 1000) * 1000);
    let base = page::base_dir(&file, stdin);
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
        tmp_html: std::env::temp_dir().join(format!("mddog-{}-{:x}.html", std::process::id(), kitty::nonce())),
        doc: None,
        theme: &config::get().themes.light,
        touched: false,
        late: None,
        sizes: None,
        glide: None,
        drawn: 0,
        path: vec![],
        last_jump: None,
        frame_due: false,
        resize: app::Debounce::default(),
        quit: false,
        opened: picker.is_none(),
        listing: picker.is_some(),
        picker,
        history: Vec::new(),
        restore_at: None,
    };
    let mut leftover = leftover;
    if p.listing {
        engine::spare::spawn();
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
                engine::spare::spawn();
                eprintln!("mdd: {}: {}", term::printable(&p.file.display().to_string()), term::io_error(&e));
                return 1;
            }
            Err(RecvTimeoutError::Disconnected) => {
                p.shutdown();
                engine::spare::spawn();
                eprintln!("mdd: could not render {}", term::printable(&p.file.display().to_string()));
                return 1;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let quit = std::iter::from_fn(|| p.rx.try_recv().ok()).fold(false, |q, e| q | matches!(e, Event::Quit));
        p.tty.read_timeout(&mut leftover, Instant::now() + Duration::from_millis(1), quits);
        if quit || quits(&leftover) {
            p.shutdown();
            engine::spare::spawn();
            return 0;
        }
    };
    trace::mark("markdown rendered");
    p.render_doc(first.0, first.1, first.2, false);
    trace::mark("first paint sent");
    engine::spare::spawn();
    crate::sweep::spawn();
    let ext = p.file.extension().map(|e| e.to_string_lossy().to_lowercase());
    if !p.probe.answered {
        p.notify("The terminal did not answer. A blank page means it can't show images");
        p.notice_until = Some(Instant::now() + Duration::from_secs(15));
    } else if std::str::from_utf8(&p.stamp).is_err() {
        p.notify("Some bytes are not UTF-8 and show as �");
    } else if !p.stdin && !matches!(ext.as_deref(), Some("md" | "mdx" | "markdown" | "mdown" | "mkd")) {
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
            self.watcher = watch::spawn(&self.file, self.tx.clone());
            if self.watcher.is_some() {
                let _ = self.tx.send(Event::FileChanged);
            }
        }
    }

    fn event_loop(&mut self) {
        while !self.quit && !self.engine.stalled() {
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
            // The page moved or changed under a still pointer, so what it hovers may have changed.
            if let Some((spot, px)) = self.hover_at
                && px != self.shown_px()
                && !self.listing
                && self.drag.is_none()
                && !self.bar_held
                && !self.app.toc_bar_held()
            {
                need_show |= self.hover(spot);
            }
            if self.notice_until.is_some_and(|due| Instant::now() >= due) {
                self.notice_until = None;
                self.app.notice = None;
                self.draw_chrome();
            }
            if need_show {
                self.show();
            }
            self.tick();
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
                trace::log_key(k);
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
                        self.open(path);
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
                trace::log_key(k);
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
                        if col <= self.img_col
                            && row <= self.viewport_rows() as u16
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
                    let on_hint = spot.cell.1 > self.viewport_rows() as u16 && self.chrome().hint_at(spot.cell.0).is_some();
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
                let json = self.search_js(&script::search(&q, None));
                self.set_matches(serde_json::from_str(&json).unwrap_or_default());
                let rows = self.match_rows();
                for b in self.app.set_matches(rows) {
                    self.act(b);
                }
                true
            }
            Action::Current(i) => {
                let json = self.search_js(&format!("mddogSearch.setCurrent({i})"));
                self.invalidate(&script::rects(&json));
                if let Some(r) = self.matches.get(i).and_then(|m| m.first()) {
                    trace::log_match(i, self.matches.len(), *r);
                }
                true
            }
            Action::ClearSearch => {
                self.search_js("mddogSearch.clear()");
                self.set_matches(Vec::new());
                true
            }
            Action::ToggleToc if self.app.toc_visible && app::toc_width(self.size.cols) == 0 => {
                self.app.toc_visible = false;
                self.notify(&format!("Too narrow for the contents. It needs {} columns", app::TOC_MIN_COLS));
                false
            }
            Action::ToggleToc => {
                self.rerender_keep();
                false
            }
            Action::ToggleWrap => {
                self.search_js(&format!("mddogCode.wrap({})", self.app.wrap));
                self.relayout();
                false
            }
            Action::ScrollView(cells) => {
                let (top, height) = self.view_css();
                let bottom = top + height;
                let dx = f64::from(cells) * f64::from(self.cell_w) / self.scale;
                let moved = self.select_js(&format!("mddogCode.scrollView ? mddogCode.scrollView({top},{bottom},{dx}) : '[]'"));
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
            Action::Collapse => self.select_js("mddogMouse.collapse()"),
            Action::CopySelection => {
                self.copy_selection();
                false
            }
            Action::Suspend => {
                self.suspend();
                false
            }
            Action::ScrollCode(col, row, cells) => {
                if col <= self.img_col || u32::from(row) > self.viewport_rows() {
                    return false;
                }
                let (x, y) = self.grid().point(Spot { cell: (col, row), sub: None }, 0.5);
                let dx = f64::from(cells) * f64::from(self.cell_w) / self.scale;
                self.select_js(&format!("mddogCode.scroll({x},{y},{dx})"))
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
                if row > rows {
                    let back = self.app.notice.as_deref() == Some(GO_BACK);
                    let Some(key) = self.chrome().hint_at(col).or(back.then_some(Key::Backspace)) else {
                        if self.app.notice.is_some() {
                            self.app.notice = None;
                            self.notice_until = None;
                            self.draw_chrome();
                        }
                        return false;
                    };
                    if key == Key::Char('q') {
                        let armed = self.app.notice.as_deref() == Some(CLICK_QUIT);
                        match self.quit_armed.filter(|_| armed).map(|t| t.elapsed()) {
                            Some(e) if e < CLICK_AGAIN => return false,
                            Some(_) => {}
                            None => {
                                self.quit_armed = Some(Instant::now());
                                self.notify(CLICK_QUIT);
                                return false;
                            }
                        }
                    }
                    if self.app.notice.as_deref() == Some(CLICK_QUIT) {
                        self.app.notice = None;
                    }
                    let actions = self.app.handle(key);
                    return self.act_all(actions);
                }
                if col <= self.img_col
                    && let Some(actions) = self.app.toc_bar_press(self.img_col - col, u32::from(row) - 1)
                {
                    return self.act_all(actions);
                }
                if col <= self.img_col {
                    let actions = self.app.toc_click(u32::from(row) - 1);
                    return self.act_all(actions);
                }
                let (x, y) = grid.point(spot, 0.5);
                let hit = self.search_js(&format!("mddogCode.grab({x},{y},{})", f64::from(self.cell_h) / self.scale / 2.0));
                if !hit.is_empty() {
                    self.bar_held = true;
                    self.last_press = None;
                    let rects = script::rects(&hit);
                    self.invalidate(&rects);
                    return !rects.is_empty();
                }
                let double = self.is_double(col, row);
                self.last_press = (!double).then(|| (Instant::now(), col, row));
                self.drag = Some(Drag { from: spot, at: spot, moved: double });
                if double {
                    return self.selection_js(&format!("mddogMouse.word({x},{y})"));
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
                let (x, _) = grid.point(spot, 0.5);
                self.select_js(&format!("mddogCode.drag({x})"))
            }
            Click::Release if self.bar_held => {
                let shown = self.mouse_event(Click::Drag, spot);
                self.bar_held = false;
                let _ = self.engine.eval("mddogCode.release()");
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
                self.selection_js(&format!("mddogMouse.select({ax},{ay},{fx},{fy})"))
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
                let cleared = self.selection_js("mddogMouse.clear()");
                let (x, y) = grid.point(d.from, 0.5);
                let hit = self.engine.eval(&format!("mddogMouse.link({x},{y})")).unwrap_or_default();
                let target = serde_json::from_str::<(String, String)>(&hit).ok();
                let link = target.as_ref().and_then(|(raw, resolved)| classify_link(raw, resolved));
                if target.is_some() && link.is_none() {
                    self.notify("Links to local files don't open in the preview");
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
        let text = pasteable(text);
        self.tty.write(osc52(&text).as_bytes());
        self.tty.flush();
        pbcopy(text);
    }

    /// Copies the page's selection, if there is one.
    fn copy_selection(&mut self) {
        let text = self.engine.eval("getSelection().toString()").unwrap_or_default();
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
        let (col, row) = spot.cell;
        let page = !(self.app.help || self.app.expanded) && col > self.img_col && u32::from(row) <= self.viewport_rows();
        if !page && !self.hover_page {
            self.set_pointer(DEFAULT_POINTER);
            return false;
        }
        self.hover_page = page;
        let (x, y) = if page { self.grid().point(spot, 0.5) } else { (-1.0, -1.0) };
        let json = self.engine.eval(&format!("mddogMouse.hover({x},{y},{})", f64::from(self.cell_h) / self.scale / 2.0)).unwrap_or_default();
        let (shape, rects) = serde_json::from_str::<(String, Vec<Rect>)>(&json).unwrap_or_default();
        self.set_pointer(match shape.as_str() {
            "text" => "xterm",
            "pointer" => self.probe.hand(),
            _ => DEFAULT_POINTER,
        });
        if rects.is_empty() {
            return false;
        }
        self.frame_due = true;
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
        let json = self.search_js(&format!("mddogMouse.press({x},{y})"));
        let hit = script::press(&json);
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
                let shown = self.select_js(&format!("mddogMouse.expand({i},{top},{height})"));
                self.app.expanded = shown;
                shown
            }
            Press::Rect(rect) => {
                self.invalidate(rect.as_slice());
                true
            }
            Press::Other => {
                self.relayout();
                false
            }
        }
    }

    fn copy_due(&mut self) -> bool {
        if !self.copy_until.is_some_and(|due| Instant::now() >= due) {
            return false;
        }
        self.copy_until = None;
        self.select_js("mddogMouse.resetCopy()")
    }

    fn open_link(&mut self, link: Link) -> bool {
        match link {
            Link::Open(url) => {
                self.notify(&format!("Opened {url}"));
                open_url(url);
                false
            }
            Link::Anchor(id) => {
                let top = self.engine.eval(&format!("mddogMouse.anchorY({})", serde_json::to_string(&id).unwrap_or_default())).ok().and_then(|s| s.parse::<f64>().ok());
                let Some(y) = top.filter(|y| *y >= 0.0) else { return false };
                let row = self.row_of(y);
                let actions = self.app.jump(row);
                let show = self.act_all(actions);
                self.notify(GO_BACK);
                show
            }
        }
    }

    fn select_js(&mut self, js: &str) -> bool {
        let json = self.search_js(js);
        let rects = script::rects(&json);
        self.invalidate(&rects);
        !rects.is_empty()
    }

    fn selection_js(&mut self, js: &str) -> bool {
        if !self.cache.bands() {
            return self.select_js(js);
        }
        let json =
            self.search_js(&format!("(() => {{ const s = getSelection(); const n = s.rangeCount ? s.getRangeAt(0).getClientRects().length : 0; return JSON.stringify([n, JSON.parse({js})]); }})()"));
        let (n, all) = serde_json::from_str::<(usize, Vec<Rect>)>(&json).unwrap_or_default();
        let (before, after) = all.split_at(n.min(all.len()));
        let row = f64::from(self.cell_h) / self.scale;
        let rects: Vec<Rect> = changed_rects(before, after).into_iter().map(|r| [r[0], r[1] - row, r[2], r[3] + 2.0 * row]).collect();
        self.invalidate(&rects);
        !rects.is_empty()
    }

    fn search_js(&mut self, js: &str) -> String {
        self.frame_due = true;
        self.engine.eval(js).unwrap_or_default()
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
        u32::from(self.size.rows.saturating_sub(1).max(1))
    }

    fn scroll_px(&self) -> u32 {
        self.phase + self.app.scroll * self.cell_h
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
        if bytes == self.stamp {
            if was_missing {
                self.draw_chrome();
            }
            return;
        }
        let base = self.base();
        let (src, doc) = page::doc(&self.file, &bytes, &base, |s, d| self.engine.render(s, d));
        self.render_doc(bytes, src, doc, true);
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
        !self.stdin && config::get().live_reload
    }

    fn rerender_keep(&mut self) {
        self.rerender(self.live());
    }

    fn rerender(&mut self, read: bool) {
        let bytes = read.then(|| std::fs::read(&self.file).ok()).flatten().unwrap_or_else(|| self.stamp.clone());
        let (src, doc) = match self.doc.take() {
            Some(doc) if bytes == self.stamp => (self.src.clone(), doc),
            _ => {
                let base = self.base();
                page::doc(&self.file, &bytes, &base, |s, d| self.engine.render(s, d))
            }
        };
        self.render_doc(bytes, src, doc, true);
    }

    fn notify(&mut self, msg: &str) {
        self.app.notice = Some(msg.to_string());
        self.notice_until = Some(Instant::now() + NOTICE);
        self.draw_chrome();
    }

    fn render_doc(&mut self, bytes: Vec<u8>, src: String, doc: markdown::Doc, keep: bool) {
        let at_px = self.scroll_px();
        let anchor = if keep && at_px > 0 { position::capture(&self.layout.blocks, f64::from(at_px) / self.scale) } else { None };
        let code = if keep { self.engine.eval("mddogCode.positions ? mddogCode.positions() : '[]'").unwrap_or_default() } else { String::new() };
        let mut old_src = std::mem::take(&mut self.src);
        let mut anchor = anchor;
        if let Some((a, src)) = self.restore_at.take() {
            (anchor, old_src) = (Some(a), src);
        }
        let (toc, css_width) = self.fit();
        let base = self.base();
        let (html, theme) = page::html(&doc, &base, self.app.wrap);
        self.theme = theme;
        let vp_css = self.view_css().1;
        self.engine.resize(css_width, vp_css.max(100.0));
        self.app.expanded = false;
        self.touched = false;
        let loaded = page::load(&self.engine, &self.tmp_html, &html, theme.dark, Duration::from_secs(3));
        trace::mark("page loaded");
        self.engine.wait_js(script::READY, Duration::from_millis(500));
        if doc.mermaid && config::mermaid(theme) == config::Mermaid::Obsidian {
            let sizes = self.sizes.get_or_insert_with(Sizes::open);
            if !sizes.is_empty() {
                let _ = self.engine.eval(&format!("mddog.guess({}); ''", sizes.json()));
            }
        }
        let mut late = Vec::new();
        if doc.math && !self.engine.wait_js(script::MATH_DONE, Duration::from_secs(1)) {
            late.push(script::MATH_DONE);
        }
        // Obsidian-engine diagrams start after the first paint, below; the re-send shows them.
        if doc.mermaid && !self.engine.wait_js(script::MERMAID_DONE, Duration::ZERO) {
            late.push(script::MERMAID_DONE);
        }
        trace::mark("layout ready");
        if !code.is_empty() && code != "[]" {
            let _ = self.engine.eval(&format!("mddogCode.restore && mddogCode.restore({})", serde_json::Value::from(code)));
        }
        self.measure();
        if loaded {
            self.stamp = bytes;
            self.doc = Some(doc);
        }
        self.src = src;
        let mut scroll_px = 0u32;
        if let Some(a) = anchor {
            let line = position::map_line(&old_src, &self.src, a.line);
            if let Some(y) = position::restore(&self.layout.blocks, position::Anchor { line, offset: a.offset }) {
                scroll_px = (y * self.scale).round() as u32;
            }
        }
        self.settle(scroll_px);
        let want = if self.app.toc_visible && !self.app.entries.is_empty() { app::toc_width(self.size.cols) } else { 0 };
        if want != toc {
            self.rerender_keep();
            return;
        }
        if remote::load(&self.engine) {
            late.push(script::REMOTE_DONE);
        }
        let start = late.contains(&script::MERMAID_DONE);
        self.late = (!late.is_empty()).then(|| (format!("String({})", late.join(" && ")), Instant::now() + LATE_LIMIT));
        self.stale_hover();
        self.show();
        if start {
            let _ = self.engine.eval(script::START_MERMAID);
        }
    }

    /// Takes the terminal's size and returns the TOC width and the page width in CSS pixels.
    fn fit(&mut self) -> (u16, f64) {
        let before = (self.size, self.img_col);
        self.size = self.tty.size();
        if let Some((w, h)) = self.size.cell_px(self.scale) {
            (self.cell_w, self.cell_h) = (w, h);
        }
        let toc = if self.app.toc_visible && !self.app.entries.is_empty() { app::toc_width(self.size.cols) } else { 0 };
        self.img_col = toc;
        self.img_cols = self.size.cols.saturating_sub(toc).max(1);
        // Some terminals draw the old text over the new tiles, or keep it at rows the status row left.
        self.help_drawn |= before != (self.size, toc);
        (toc, f64::from(u32::from(self.img_cols) * self.cell_w) / self.scale)
    }

    /// Lays the loaded page out again at the new terminal size, without a reload. Returns false
    /// when only a reload gives the right result: the file changed, the macOS appearance changed
    /// under a light and dark theme pair, a diagram is expanded, a click or a selection changed the page, an
    /// image can pick its source by width, or the page did not take its new size in time.
    fn resize_in_place(&mut self) -> bool {
        if self.app.expanded || self.touched || !std::ptr::eq(config::theme(), self.theme) || self.doc.is_none() {
            return false;
        }
        if self.engine.eval("String(!getSelection().isCollapsed || !!document.querySelector('picture, img[srcset]'))").as_deref() != Ok("false") {
            return false;
        }
        if self.live() && std::fs::read(&self.file).ok().is_none_or(|b| b != self.stamp) {
            return false;
        }
        let at_px = self.scroll_px();
        let anchor = if at_px > 0 { position::capture(&self.layout.blocks, f64::from(at_px) / self.scale) } else { None };
        let old = (self.size, self.cell_w, self.cell_h);
        let (toc, css_width) = self.fit();
        self.engine.resize(css_width, self.view_css().1.max(100.0));
        let mut sized = self.engine.wait_js(&format!("Math.abs(innerWidth - {css_width}) < 1"), Duration::from_millis(100));
        if sized && self.doc.as_ref().is_some_and(|d| d.mermaid) {
            // Pele refits each diagram in a ResizeObserver callback. Those run in the next
            // rendering update, after its animation frame callbacks, so wait for a task after it.
            let _ = self.engine.eval("window.mddogFrame = false; requestAnimationFrame(() => setTimeout(() => { window.mddogFrame = true; })); ''");
            sized = self.engine.wait_js("window.mddogFrame", Duration::from_millis(100));
        }
        if !sized {
            // The reload takes the reading position from the old geometry.
            (self.size, self.cell_w, self.cell_h) = old;
            return false;
        }
        self.measure();
        self.stale_hover();
        let scroll_px = anchor.and_then(|a| position::restore(&self.layout.blocks, a)).map_or(0, |y| (y * self.scale).round() as u32);
        self.settle(scroll_px);
        let want = if self.app.toc_visible && !self.app.entries.is_empty() { app::toc_width(self.size.cols) } else { 0 };
        if want != toc {
            self.rerender_keep();
            return true;
        }
        self.show();
        true
    }

    fn settle(&mut self, scroll_px: u32) {
        self.glide = None;
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
        let json = self.search_js(&script::search(&self.app.query, Some(self.app.current)));
        self.matches = serde_json::from_str(&json).unwrap_or_default();
        self.app.matches = self.match_rows();
        if self.app.current >= self.app.matches.len() {
            self.app.current = 0;
        }
    }

    fn measure(&mut self) {
        self.layout = script::measure(&self.engine);
        self.doc_px = (self.layout.height * self.scale).ceil() as u32;
    }

    fn tile_height(&self) -> u32 {
        let width = u32::from(self.img_cols) * self.cell_w;
        // One row less than fits, for the row each tile is sent with below it.
        let rows = (kitty::TILE_BYTES as u32 / (width.max(1) * 4) / self.cell_h).min(10_000 / self.cell_h);
        rows.saturating_sub(1).max(1) * self.cell_h
    }

    fn retile(&mut self) {
        let mut out = String::new();
        let total = self.phase + self.app.doc_rows.max(1) * self.cell_h + app::PADDING_ROWS * self.cell_h;
        let top = self.cache.retile(&mut out, self.phase, total, self.tile_height(), self.cell_h);
        self.tty.write(out.as_bytes());
        let height = (f64::from(top) / self.scale).max(self.layout.height);
        self.engine.resize(self.engine.width(), height);
        engine::spin(1.0);
        // A snapshot taken before the page has its new size comes back blank. Seeing the new
        // innerHeight takes about 2 ms, where waiting for a frame takes 15 to 25 ms.
        let sized = self.engine.wait_js(&format!("Math.abs(innerHeight - {height}) < 1"), Duration::from_millis(100));
        self.frame_due |= !sized;
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
            frees.push_str("\x1b[?2026h");
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
                out.push_str("\x1b[?2026l");
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
                out.push_str("\x1b[?2026l");
            }
            self.tty.write(out.as_bytes());
            self.tty.flush();
            return;
        }
        if std::mem::take(&mut self.help_drawn) {
            frees.push_str(&format!("{}\x1b[2J", self.theme.palette.reset()));
        }
        let (engine, scale, frame_due) = (&self.engine, self.scale, &mut self.frame_due);
        let mut snap = |top: u32, height: u32| {
            if std::mem::take(frame_due) {
                engine.wait_frame();
            }
            engine.snapshot(f64::from(top) / scale, f64::from(height) / scale, scale)
        };
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
        if let Some(l) = loader {
            l.finish();
        }
        self.tty.write(frees.as_bytes());
        self.tty.write(&bytes);
        let mut out = String::new();
        self.cache.place(&vis, &v, &mut out);
        self.chrome().draw(&mut out);
        if self.probe.sync {
            out.push_str("\x1b[?2026l");
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
            trace::log_view(&format!(
                "px={} took={:.2} scroll={} phase={} placed={:?} resident={} held={held} scale={} cell={}x{} rows={} doc={} top={top}",
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
        if let Some(bmp) = self.engine.snapshot(y, h, self.scale) {
            let _ = bmp.write_png(path);
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

    /// Glides from where the view is drawn to `scroll_px`, or jumps there when gliding is off, the terminal can't
    /// draw a frame at once (no mode 2026), or the tiles along the way don't fit in the storage limit together.
    fn start_glide(&mut self) {
        let to = self.scroll_px();
        // A jump during a glide adds to it, so the screen it may cover grows by what that glide had left.
        let left = self.glide.take().map_or(0, |(g, _)| g.to().abs_diff(self.drawn));
        if !config::get().smooth_scroll || !self.probe.sync || self.drawn == to {
            return;
        }
        let now = Instant::now();
        let pace = app::Pace::for_jump(self.last_jump.replace(now), now, Duration::from_millis(config::get().glide_ms_per_page.into()), self.view_px());
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
        let (engine, scale, frame_due) = (&self.engine, self.scale, &mut self.frame_due);
        let mut snap = |top: u32, height: u32| {
            if std::mem::take(frame_due) {
                engine.wait_frame();
            }
            engine.snapshot(f64::from(top) / scale, f64::from(height) / scale, scale)
        };
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
                self.size = self.tty.size();
                self.show();
            } else if !self.resize_in_place() {
                self.rerender_keep();
            }
        }
        if let Some((cond, until)) = &self.late {
            engine::spin(2.0);
            if Instant::now() > *until || self.engine.eval(cond).as_deref() == Ok("true") {
                self.late = None;
                if let (Some(sizes), Ok(json)) = (&mut self.sizes, self.engine.eval("JSON.stringify(mddog.state.sizes)")) {
                    sizes.merge(&json);
                }
                self.relayout();
            }
            return;
        }
        if let Some(i) = self.prefetch_target() {
            let v = self.view();
            let vis = self.cache.visible(&v);
            self.send_tiles(&[i], &vis, v.scroll_px);
        }
    }

    fn prefetch_target(&self) -> Option<usize> {
        if self.app.help || self.listing || self.engine.stalled() || self.glide.is_some() {
            return None;
        }
        self.cache.prefetch(self.scroll_px(), u32::from(self.img_cols) * self.cell_w)
    }

    fn relayout(&mut self) {
        let scroll_px = self.scroll_px();
        let anchor = if scroll_px > 0 { position::capture(&self.layout.blocks, f64::from(scroll_px) / self.scale) } else { None };
        self.measure();
        self.stale_hover();
        let scroll_px = anchor.and_then(|a| position::restore(&self.layout.blocks, a)).map_or(scroll_px, |y| (y * self.scale).round() as u32);
        self.settle(scroll_px);
        self.show();
        trace::mark("late content settled");
    }

    fn leave_screen(&mut self) {
        self.paused.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(60));
        let mut out = String::new();
        self.cache.free_all(&mut out);
        self.tty.write(out.as_bytes());
        self.tty.write(format!("{}{}\x1b[?25h\x1b[?7h\x1b[?1049l", term::MOUSE_OFF, term::KEYS_OFF).as_bytes());
        self.tty.flush();
        self.tty.restore();
        self.pointer = DEFAULT_POINTER;
        self.stale_hover();
    }

    fn enter_screen(&mut self, read: bool) {
        let _ = self.tty.raw();
        let pixels = if self.probe.pixel_mouse { term::PIXEL_MOUSE_ON } else { "" };
        self.tty.write(format!("\x1b[?1049h\x1b[?7l\x1b[?25l\x1b[2J{}{}{pixels}", term::MOUSE_ON, term::KEYS_ON).as_bytes());
        self.tty.flush();
        let pending: Vec<Event> = std::iter::from_fn(|| self.rx.try_recv().ok()).filter(|e| !matches!(e, Event::FileChanged)).collect();
        for e in pending {
            let _ = self.tx.send(e);
        }
        self.paused.store(false, Ordering::SeqCst);
        if self.opened {
            self.rerender(read);
        } else {
            self.size = self.tty.size();
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
            Ok(()) if self.stamp != before => self.notify("Reloaded"),
            Ok(()) => {}
        }
        trace::mark("editor reload");
    }

    fn suspend(&mut self) {
        self.leave_screen();
        unsafe { libc::kill(0, libc::SIGTSTP) };
        let live = self.live();
        self.enter_screen(live);
    }

    /// Opens a file from the list. The file shown before goes on the Back history.
    fn open(&mut self, path: PathBuf) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                if let Some(p) = self.picker.as_mut() {
                    p.note = Some(format!("Can't open {}: {}", path.display(), term::io_error(&e)));
                }
                self.show();
                return;
            }
        };
        let (wrap, toc_visible) = (self.app.wrap, self.app.toc_visible);
        if self.opened {
            let at_px = self.scroll_px();
            let anchor = if at_px > 0 { position::capture(&self.layout.blocks, f64::from(at_px) / self.scale) } else { None };
            let visit = Visit { file: std::mem::take(&mut self.file), stdin: self.stdin, name: std::mem::take(&mut self.name), app: std::mem::take(&mut self.app), anchor, src: self.src.clone() };
            self.history.push(visit);
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
        }
        self.app = App::default();
        (self.app.wrap, self.app.toc_visible, self.app.back_file) = (wrap, toc_visible, !self.history.is_empty());
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.start_loader(&name);
        self.switch(path, false, name, bytes, None);
    }

    fn back_file(&mut self) {
        let Some(v) = self.history.pop() else { return };
        let bytes = match std::fs::read(&v.file) {
            Ok(b) => b,
            Err(_) => {
                // Drop it, so the next Backspace reaches the files before it.
                self.app.back_file = !self.history.is_empty();
                self.notify(&format!("Can't read {}, so it left the Back list", v.name));
                return;
            }
        };
        let (wrap, toc_visible) = (self.app.wrap, self.app.toc_visible);
        self.app = v.app;
        (self.app.wrap, self.app.toc_visible, self.app.back_file) = (wrap, toc_visible, !self.history.is_empty());
        self.start_loader(&v.name);
        self.switch(v.file, v.stdin, v.name, bytes, v.anchor.map(|a| (a, v.src)));
    }

    /// Draws a skeleton over the screen if the next page is slow to show. `show` takes it down.
    fn start_loader(&mut self, name: &str) {
        let l = Loader::start(self.tty.fd, name);
        l.tune(self.scale, (self.cell_w, self.cell_h));
        self.loader = Some(l);
    }

    fn switch(&mut self, file: PathBuf, stdin: bool, name: String, bytes: Vec<u8>, at: Option<(position::Anchor, String)>) {
        self.base = page::base_dir(&file, stdin);
        (self.file, self.stdin, self.name) = (file, stdin, name);
        self.matches.clear();
        self.late = None;
        self.doc = None;
        self.src.clear();
        self.stamp.clear();
        self.restore_at = at;
        if let Some(p) = &self.picker {
            p.halt();
        }
        self.listing = false;
        self.opened = true;
        let base = self.base();
        let (src, doc) = page::doc(&self.file, &bytes, &base, |s, d| self.engine.render(s, d));
        self.render_doc(bytes, src, doc, false);
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
        let _ = std::fs::remove_file(&self.tmp_html);
        self.watcher = None;
        if self.engine.stalled() {
            eprintln!("mdd: WebKit stopped responding");
        }
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
        let wait = Duration::from_millis(if buf.last() == Some(&0x1b) { 30 } else { 50 });
        let ready = term::readable(fd, wait);
        if ready && !paused.load(Ordering::SeqCst) {
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n <= 0 {
                let _ = tx.send(Event::Quit);
                return;
            }
            buf.extend_from_slice(&chunk[..n as usize]);
        }
        for k in term::parse_keys(&mut buf, !ready) {
            if tx.send(Event::Key(k)).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_text_drops_control_characters() {
        assert_eq!(pasteable("ls\x1b[201~echo\u{9b}hi\x07\n\tok\r\n"), "ls[201~echohi\n\tok\r\n");
    }

    #[test]
    fn osc52_encodes_base64() {
        // The RFC 4648 test vectors.
        for (text, b64) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(osc52(text), format!("\x1b]52;c;{b64}\x1b\\"));
        }
        assert_eq!(osc52("é✓"), "\x1b]52;c;w6ninJM=\x1b\\");
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
    fn only_web_and_mail_links_open() {
        let open = |u: &str| Some(Link::Open(u.to_string()));
        assert_eq!(classify_link("https://a.dev/x", "https://a.dev/x"), open("https://a.dev/x"));
        assert_eq!(classify_link("HTTP://a.dev", "http://a.dev/"), open("http://a.dev/"));
        assert_eq!(classify_link("mailto:a@b.dev", "mailto:a@b.dev"), open("mailto:a@b.dev"));
        assert_eq!(classify_link("#intro", "file:///d/#intro"), Some(Link::Anchor("intro".into())));
        assert_eq!(classify_link("docs/a.md", "file:///d/docs/a.md"), None);
        assert_eq!(classify_link("javascript:alert(1)", "javascript:alert(1)"), None);
        assert_eq!(classify_link("x.html", "file:///d/x.html"), None);
        assert_eq!(classify_link("data:text/html,hi", "data:text/html,hi"), None);
    }

    #[test]
    fn only_changed_selection_rects_count() {
        let (a, b, c) = ([0.0, 0.0, 100.0, 19.0], [0.0, 19.0, 100.0, 19.0], [0.0, 38.0, 40.0, 19.0]);
        assert_eq!(changed_rects(&[a, b, [0.0, 38.0, 20.0, 19.0]], &[a, b, c]), vec![[0.0, 38.0, 20.0, 19.0], c]);
        assert_eq!(changed_rects(&[a, a], &[a]), vec![a]);
        assert_eq!(changed_rects(&[], &[a]), vec![a]);
    }
}
