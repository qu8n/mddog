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
/// Under `LATE_WAIT`, so a slow image shows as failed in the tiles rather than as a loading box.
const REMOTE_LIMIT: Duration = Duration::from_secs(4);
const ENCODERS: usize = 4;
/// Rows per WebP tile: 160 CSS px at the 16 px row, a fine step for the desktop pane's window of tiles, so
/// little of its 262,144 characters goes to tiles mostly out of view.
const WEBP_ROWS: u32 = 10;
/// The most bytes a WebP tile may take: its 128,000 base64 characters plus the `Svg` markup stay under the
/// desktop's 131,072.
const WEBP_MAX_BYTES: usize = 96_000;
/// The most bytes a small copy should take, so seven fill no more than 190,000 of a drawing's characters.
const WEBP_SMALL_MAX: usize = 20_000;
/// A tile under this is text: no lossy copy can save enough to matter.
const WEBP_LOSSY_FROM: usize = 12_000;

type FirstRender = JoinHandle<Option<(Vec<u8>, Doc)>>;

pub fn run(file: PathBuf, cols: u16, webp: bool) -> i32 {
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
                let (_, doc) = page::doc(&bytes, &page::base_dir(&file, false), |_, _| None);
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
    let mut r = Renderer { engine, file, first, dir, html: PathBuf::new(), scale, cell_w, cell_h, cols: u32::from(cols.max(1)), round: 0, stamp: None, gone: false, webp };
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
    /// Lossless WebP tiles sized for the desktop app's `Svg` element, instead of PNG tiles for a terminal.
    webp: bool,
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
        let doc = first.unwrap_or_else(|| page::doc(&bytes, &base, |s, d| self.engine.render(s, d)).1);
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
        if remote::load(&self.engine, REMOTE_LIMIT) {
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
        let tiles = if self.webp {
            self.webp_tiles(round, rows)?
        } else {
            // PNG encoding takes as long as a snapshot, so it runs on other threads while the next snapshot is taken.
            std::thread::scope(|s| {
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
            })?
        };
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

    /// Cuts the page into lossless WebP tiles of `WEBP_ROWS` rows, halving a tile whose file would not fit one
    /// desktop `Svg` (its base64 under 131,072 characters with the markup around it). Each tile also gets a
    /// `small` copy at 1x, a quarter of the pixels: one drawing holds 262,144 characters, which
    /// dense text or pictures in a tall pane outgrow, so the plugin draws small copies where sharp ones don't fit.
    /// One thread: each size decides the next snapshot.
    fn webp_tiles(&self, round: u64, rows: u32) -> Result<Vec<serde_json::Value>, String> {
        let mut tiles = Vec::new();
        let mut top = 0;
        while top < rows {
            let mut n = WEBP_ROWS.min(rows - top);
            loop {
                let (y, h) = (f64::from(top * self.cell_h) / self.scale, f64::from(n * self.cell_h) / self.scale);
                let mut bmp = self.engine.snapshot(y, h, self.scale).ok_or("the snapshot failed")?;
                bmp.rgba.unlink();
                let path = self.dir.join(format!("{round}-{}.webp", tiles.len()));
                let mut size = bmp.write_webp(&path, None)?;
                // A picture takes a third of its lossless bytes as lossy, text four fifths or more: only a picture
                // goes lossy, so a pane full of one stays 2x within the desktop's 262,144 characters.
                if size > WEBP_LOSSY_FROM {
                    let lossless = size;
                    size = bmp.write_webp(&path, Some(75.0))?;
                    if size * 10 > lossless * 6 {
                        size = bmp.write_webp(&path, None)?;
                    }
                }
                if size <= WEBP_MAX_BYTES || n == 1 {
                    let mut low = self.engine.snapshot(y, h, (self.scale / 2.0).max(1.0)).ok_or("the snapshot failed")?;
                    low.rgba.unlink();
                    let small = self.dir.join(format!("{round}-{}s.webp", tiles.len()));
                    // Lossless suits text and lossy suits pictures, at a third to a tenth of the other's bytes.
                    let lossless = low.write_webp(&small, None)?;
                    let mut small_size = low.write_webp(&small, Some(75.0))?;
                    if small_size > lossless {
                        small_size = low.write_webp(&small, None)?;
                    }
                    // The plugin must fit the view in small copies, about seven tiles to a drawing.
                    if small_size > WEBP_SMALL_MAX {
                        small_size = low.write_webp(&small, Some(40.0))?;
                    }
                    tiles.push(serde_json::json!({
                        "file": path, "rows": n, "width": bmp.width, "height": bmp.height, "bytes": size,
                        "small": { "file": small, "width": low.width, "height": low.height, "bytes": small_size },
                    }));
                    break;
                }
                n = n.div_ceil(2);
            }
            top += n;
        }
        Ok(tiles)
    }
}
