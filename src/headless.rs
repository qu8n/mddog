use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{JoinHandle, ScopedJoinHandle};
use std::time::Duration;

use crate::Doc;
use crate::engine::{self, Engine};
use crate::page::{self, remote, script};
use crate::term;
use crate::term::kitty;
use crate::watch;

const TILE_ROWS: u32 = 100;
const LATE_WAIT: Duration = Duration::from_secs(5);
const ENCODERS: usize = 4;

type FirstRender = JoinHandle<Option<(Vec<u8>, Doc)>>;

pub fn run(file: PathBuf, cols: u16) -> i32 {
    let dir = crate::sweep::temp_path("mddog-tiles-", "");
    if let Err(e) = std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700).create(&dir) {
        eprintln!("mdd: cannot make {}: {}", dir.display(), term::io_error(&e));
        return 1;
    }
    if let Ok(mut signals) = signal_hook::iterator::Signals::new([libc::SIGTERM, libc::SIGINT, libc::SIGHUP]) {
        let dir = dir.clone();
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = std::fs::remove_dir_all(&dir);
                std::process::exit(0);
            }
        });
    }
    let (tx, rx) = mpsc::channel();
    let Some(_watcher) = watch::spawn(&file, tx) else {
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("mdd: cannot watch {} for changes", term::printable(&file.display().to_string()));
        return 1;
    };
    let spare = engine::spare::connect();
    // With no spare, the first render runs while WebKit starts.
    let first = if spare.is_none() {
        let file = file.clone();
        std::thread::Builder::new()
            .stack_size(crate::render::STACK)
            .spawn(move || {
                let bytes = std::fs::read(&file).ok()?;
                let (_, doc) = page::doc(&file, &bytes, &page::base_dir(&file, false), |_, _| None);
                Some((bytes, doc))
            })
            .ok()
    } else {
        None
    };
    let engine = spare.map_or_else(
        || {
            let e = Engine::new(800.0);
            e.load_blank();
            e
        },
        Engine::Remote,
    );
    let scale = engine.scale();
    let (cell_w, cell_h) = cell_size(scale).unwrap_or(((8.0 * scale) as u32, (16.0 * scale) as u32));
    let mut r = Renderer { engine, file, first, dir, html: PathBuf::new(), scale, cell_w, cell_h, cols: u32::from(cols.max(1)), round: 0, stamp: None, gone: false };
    r.html = r.dir.join("page.html");
    engine::spare::spawn();
    let mut swept = false;
    let code = loop {
        if let Err(e) = r.render() {
            eprintln!("mdd: {e}");
            r.drop_rounds(|g| g > r.round);
            if r.engine.stalled() {
                break 1;
            }
        }
        // Claude Code SIGKILLs this process on each width change, so the sweep is what removes the old tile folders.
        if !std::mem::replace(&mut swept, true) {
            crate::sweep::spawn();
        }
        if r.gone {
            break 0;
        }
        if rx.recv().is_err() {
            break 0;
        }
        while rx.recv_timeout(Duration::from_millis(50)).is_ok() {}
    };
    let _ = std::fs::remove_dir_all(&r.dir);
    code
}

fn encoded(h: Option<ScopedJoinHandle<Result<(), String>>>) -> Result<(), String> {
    h.map_or(Ok(()), |h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
}

fn cell_size(scale: f64) -> Option<(u32, u32)> {
    let tty = std::fs::File::open("/dev/tty").ok()?;
    term::size_of(std::os::fd::AsRawFd::as_raw_fd(&tty)).cell_px(scale)
}

struct Renderer {
    engine: Engine,
    file: PathBuf,
    first: Option<FirstRender>,
    dir: PathBuf,
    html: PathBuf,
    scale: f64,
    cell_w: u32,
    cell_h: u32,
    cols: u32,
    round: u64,
    stamp: Option<Vec<u8>>,
    gone: bool,
}

impl Renderer {
    fn drop_rounds(&self, doomed: impl Fn(u64) -> bool) {
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.split_once('-').and_then(|(g, _)| g.parse::<u64>().ok()).is_some_and(&doomed) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    fn render(&mut self) -> Result<(), String> {
        let last = self.stamp.take();
        let (bytes, first) = match self.first.take().and_then(|h| h.join().ok().flatten()) {
            Some((bytes, doc)) => (bytes, Some(doc)),
            None => (std::fs::read(&self.file).map_err(|e| format!("{}: {}", term::printable(&self.file.display().to_string()), term::io_error(&e)))?, None),
        };
        if last.as_ref() == Some(&bytes) {
            self.stamp = last;
            return Ok(());
        }
        let base = page::base_dir(&self.file, false);
        let doc = first.unwrap_or_else(|| page::doc(&self.file, &bytes, &base, |s, d| self.engine.render(s, d)).1);
        let (html, theme) = page::html(&doc, &base, false);
        let width_px = self.cols * self.cell_w;
        self.engine.resize(f64::from(width_px) / self.scale, 600.0);
        page::load(&self.engine, &self.html, &html, theme.dark, Duration::from_secs(3))?;
        let mut late = vec!["true"];
        if doc.math {
            late.push(script::MATH_DONE);
        }
        if doc.mermaid {
            let _ = self.engine.eval(script::START_MERMAID);
            late.push(script::MERMAID_DONE);
        }
        if remote::load(&self.engine) {
            late.push(script::REMOTE_DONE);
        }
        self.engine.wait_js(&late.join(" && "), LATE_WAIT);
        let bottom = script::measure(&self.engine).height;
        let rows = ((bottom * self.scale).ceil() as u32).div_ceil(self.cell_h).max(1);
        self.engine.resize(f64::from(width_px) / self.scale, f64::from(rows * self.cell_h) / self.scale);
        engine::spin(1.0);
        self.engine.wait_frame();
        let round = self.round + 1;
        let tile_rows = TILE_ROWS.min(kitty::TILE_BYTES as u32 / (width_px.max(1) * 4) / self.cell_h).max(1);
        // PNG encoding takes as long as a snapshot, so it runs on other threads while the next snapshot is taken.
        let tiles = std::thread::scope(|s| {
            let mut tiles = Vec::new();
            let mut encoding = VecDeque::new();
            let mut top = 0;
            while top < rows {
                let n = tile_rows.min(rows - top);
                let mut bmp = self.engine.snapshot(f64::from(top * self.cell_h) / self.scale, f64::from(n * self.cell_h) / self.scale, self.scale).ok_or("the snapshot failed")?;
                // Claude Code kills this process on every width change, and a named object outlives it.
                bmp.rgba.unlink();
                let path = self.dir.join(format!("{round}-{}.png", tiles.len()));
                tiles.push(serde_json::json!({ "file": path, "rows": n }));
                if encoding.len() == ENCODERS {
                    encoded(encoding.pop_front())?;
                }
                encoding.push_back(s.spawn(move || bmp.write_png(&path).map_err(|e| format!("cannot write {}: {e}", path.display()))));
                top += n;
            }
            encoding.into_iter().try_for_each(|h| encoded(Some(h)))?;
            Ok::<_, String>(tiles)
        })?;
        self.round = round;
        let keep = kitty::storage_limit(std::env::var("TERM_PROGRAM").as_deref() == Ok("herdr")) / kitty::TILE_BYTES;
        let line = serde_json::json!({ "columns": self.cols, "keep": keep, "tiles": tiles });
        let mut out = std::io::stdout().lock();
        if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
            self.gone = true;
        }
        self.drop_rounds(|g| g + 1 < self.round);
        self.stamp = Some(bytes);
        Ok(())
    }
}
