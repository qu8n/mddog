use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::ScopedJoinHandle;
use std::time::Duration;

use crate::engine::{self, Engine};
use crate::page;
use crate::term;
use crate::term::kitty;
use crate::watch;

const TILE_ROWS: u32 = 100;
/// How long remote images may take. A slower one shows as failed in the tiles.
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

pub fn run(file: PathBuf, cols: u16, webp: bool) -> i32 {
    let dir = crate::sweep::temp_path("mddog-tiles-", "");
    if let Err(e) = std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700).create(&dir) {
        eprintln!("mdd: can't make {}: {}", dir.display(), term::io_error(&e));
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
        eprintln!("mdd: can't watch {} for changes", term::printable(&file.display().to_string()));
        return 1;
    };
    let engine = Engine::new(800.0, engine::display_scale());
    let scale = engine.scale.get();
    let (cell_w, cell_h) = cell_size(scale).unwrap_or(((8.0 * scale) as u32, (16.0 * scale) as u32));
    let mut r = Renderer { engine, file, dir, scale, cell_w, cell_h, cols: u32::from(cols.max(1)), round: 0, stamp: None, gone: false, webp };
    let mut swept = false;
    let code = loop {
        if let Err(e) = r.render() {
            eprintln!("mdd: {e}");
            r.drop_rounds(|g| g > r.round);
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
    dir: PathBuf,
    scale: f64,
    cell_w: u32,
    cell_h: u32,
    cols: u32,
    round: u64,
    stamp: Option<Vec<u8>>,
    gone: bool,
    /// WebP tiles sized for the desktop app's `Svg` element, instead of PNG tiles for a terminal. Text tiles are
    /// lossless; pictures and small copies can be lossy (see `webp_tiles`).
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
        let bytes = std::fs::read(&self.file).map_err(|e| format!("{}: {}", term::printable(&self.file.display().to_string()), term::io_error(&e)))?;
        if last.as_ref() == Some(&bytes) {
            self.stamp = last;
            return Ok(());
        }
        let width_px = self.cols * self.cell_w;
        self.engine.resize(f64::from(width_px) / self.scale);
        self.engine.load(&String::from_utf8_lossy(&bytes), &page::base_dir(&self.file, false), false, crate::config::theme());
        if self.engine.pending_images() > 0 {
            self.engine.wait_images(REMOTE_LIMIT);
        }
        let bottom = self.engine.layout().height;
        let rows = ((bottom * self.scale).ceil() as u32).div_ceil(self.cell_h).max(1);
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
                    let mut bmp = self.engine.snapshot(f64::from(top * self.cell_h) / self.scale, f64::from(n * self.cell_h) / self.scale, self.scale).ok_or("can't draw the page")?;
                    // Claude Code kills this process on every width change, and a named object outlives it.
                    bmp.rgba.unlink();
                    let path = self.dir.join(format!("{round}-{}.png", tiles.len()));
                    tiles.push(serde_json::json!({ "file": path, "rows": n }));
                    if encoding.len() == ENCODERS {
                        encoded(encoding.pop_front())?;
                    }
                    encoding.push_back(s.spawn(move || bmp.write_png(&path).map_err(|e| format!("can't write {}: {e}", path.display()))));
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

    /// Cuts the page into WebP tiles of `WEBP_ROWS` rows, halving a tile whose file would not fit one
    /// desktop `Svg` (its base64 under 131,072 characters with the markup around it). Each tile also gets a
    /// `small` copy at 1x, a quarter of the pixels: one drawing holds 262,144 characters, which
    /// dense text or pictures in a tall pane outgrow, so the plugin draws small copies where sharp ones don't fit.
    /// A snapshot takes a few milliseconds and encoding up to a hundred times that, so tiles encode on other
    /// threads while the next are taken, each taken as if the tiles above it fit. A tile that doesn't drops the
    /// tiles after it and is taken again at half the rows, and the tiles after it go one at a time until one fits
    /// whole: a big picture would drop every guess.
    fn webp_tiles(&self, round: u64, rows: u32) -> Result<Vec<serde_json::Value>, String> {
        let workers = std::thread::available_parallelism().map_or(ENCODERS, usize::from);
        let mut ahead = workers;
        std::thread::scope(|s| {
            let mut tiles = Vec::new();
            let mut encoding = VecDeque::new();
            let (mut top, mut n) = (0, WEBP_ROWS);
            loop {
                if top < rows && encoding.len() < ahead {
                    let at = n.min(rows - top);
                    let (y, h) = (f64::from(top * self.cell_h) / self.scale, f64::from(at * self.cell_h) / self.scale);
                    let mut bmp = self.engine.snapshot(y, h, self.scale).ok_or("can't draw the page")?;
                    bmp.rgba.unlink();
                    let mut low = self.engine.snapshot(y, h, (self.scale / 2.0).max(1.0)).ok_or("can't draw the page")?;
                    low.rgba.unlink();
                    encoding.push_back((top, at, s.spawn(move || webp_tile(&bmp, &low, at == 1))));
                    (top, n) = (top + at, WEBP_ROWS);
                    continue;
                }
                let Some((at, got, h)) = encoding.pop_front() else { break };
                let Some(t) = h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))? else {
                    // Wait out the dropped tiles, or each halving adds threads that starve the snapshots.
                    for (_, _, h) in encoding.drain(..) {
                        let _ = h.join();
                    }
                    (top, n, ahead) = (at, got.div_ceil(2), 1);
                    continue;
                };
                if got == WEBP_ROWS {
                    ahead = workers;
                }
                let path = self.dir.join(format!("{round}-{}.webp", tiles.len()));
                let small = self.dir.join(format!("{round}-{}s.webp", tiles.len()));
                for (p, b) in [(&path, &t.sharp), (&small, &t.small)] {
                    std::fs::write(p, b).map_err(|e| format!("can't write {}: {}", p.display(), term::io_error(&e)))?;
                }
                tiles.push(serde_json::json!({
                    "file": path, "rows": got, "width": t.width, "height": t.height, "bytes": t.sharp.len(),
                    "small": { "file": small, "width": t.small_width, "height": t.small_height, "bytes": t.small.len() },
                }));
            }
            Ok(tiles)
        })
    }
}

struct WebpTile {
    sharp: Vec<u8>,
    width: usize,
    height: usize,
    small: Vec<u8>,
    small_width: usize,
    small_height: usize,
}

/// Encodes a tile and its small copy, or returns `None` when the tile is too big for one `Svg` and `last` (one
/// row, so it can't be halved) is false.
fn webp_tile(bmp: &engine::Bitmap, low: &engine::Bitmap, last: bool) -> Result<Option<WebpTile>, String> {
    let mut sharp = bmp.webp(None)?;
    // A picture takes a third of its lossless bytes as lossy, text four fifths or more: only a picture
    // goes lossy, so a pane full of one stays 2x within the desktop's 262,144 characters.
    if sharp.len() > WEBP_LOSSY_FROM {
        let lossy = bmp.webp(Some(75.0))?;
        if lossy.len() * 10 <= sharp.len() * 6 {
            sharp = lossy;
        }
    }
    if sharp.len() > WEBP_MAX_BYTES && !last {
        return Ok(None);
    }
    // Lossless suits text and lossy suits pictures, at a third to a tenth of the other's bytes.
    let lossless = low.webp(None)?;
    let mut small = low.webp(Some(75.0))?;
    if small.len() > lossless.len() {
        small = lossless;
    }
    // The plugin must fit the view in small copies, about seven tiles to a drawing.
    if small.len() > WEBP_SMALL_MAX {
        small = low.webp(Some(40.0))?;
    }
    Ok(Some(WebpTile { sharp, width: bmp.width, height: bmp.height, small, small_width: low.width, small_height: low.height }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(width: usize, height: usize, px: impl Fn(usize, usize) -> [u8; 4]) -> engine::Bitmap {
        let mut rgba = kitty::Shm::create(kitty::shm_name(), width * height * 4).unwrap();
        for (i, p) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            p.copy_from_slice(&px(i % width, i / width));
        }
        engine::Bitmap { width, height, rgba }
    }

    /// A fixed xorshift, so the noise and its encoded size are the same on every run.
    fn noise(seed: usize) -> u8 {
        let mut x = (seed as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 32) as u8
    }

    #[test]
    fn flat_tiles_stay_lossless_and_keep_the_small_size() {
        let (bmp, low) = (bitmap(800, 200, |_, _| [250, 250, 250, 255]), bitmap(400, 100, |_, _| [250, 250, 250, 255]));
        let tile = webp_tile(&bmp, &low, false).unwrap().unwrap();
        assert_eq!(tile.sharp, bmp.webp(None).unwrap());
        assert!(tile.sharp.len() < WEBP_LOSSY_FROM);
        assert_eq!((tile.width, tile.height, tile.small_width, tile.small_height), (800, 200, 400, 100));
    }

    #[test]
    fn pictures_go_lossy() {
        let px = |x: usize, y: usize| [(x / 4) as u8, (y + x / 8) as u8, 128u8.wrapping_add(noise(x * 7 + y * 13) % 24), 255];
        let (bmp, low) = (bitmap(600, 200, px), bitmap(300, 100, px));
        let lossless = bmp.webp(None).unwrap();
        assert!(lossless.len() > WEBP_LOSSY_FROM, "the fixture must be big enough to try lossy");
        let tile = webp_tile(&bmp, &low, false).unwrap().unwrap();
        assert!(tile.sharp.len() * 10 <= lossless.len() * 6, "{} lossy against {} lossless", tile.sharp.len(), lossless.len());
    }

    #[test]
    fn noise_too_big_for_one_svg_is_halved_unless_last() {
        let px = |x: usize, y: usize| [noise(x + y * 1200), noise(x + y * 1200 + 1), noise(x + y * 1200 + 2), 255];
        let (bmp, low) = (bitmap(1200, 400, px), bitmap(600, 200, px));
        assert!(webp_tile(&bmp, &low, false).unwrap().is_none());
        let last = webp_tile(&bmp, &low, true).unwrap().unwrap();
        assert!(last.sharp.len() > WEBP_MAX_BYTES);
    }
}
