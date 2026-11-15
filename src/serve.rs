//! `--serve`: mddog for a host that owns the screen and the keys, such as the Neovim plugin in `lua/mddog/`.
//!
//! The host and `mdd` talk in JSON lines on standard input and output. The host sends requests with an `id`, and
//! `mdd` answers each with the same `id`, plus `page` events when the page changes. `mdd` never writes to the
//! terminal: it renders tiles into files, and the host sends them to the terminal and decides where they show.
//! So the host also decides which tiles the terminal holds, and drops the ones far from the view.
//!
//! Requests, each `{"id": N, "cmd": NAME, ...}`:
//!
//! | `cmd` | Fields | Answer |
//! |---|---|---|
//! | `hello` | `protocol`, `png` (send PNG files, for a terminal on another machine), `p3` (Display P3 colors) | `protocol`, `version`, `budget` (image bytes the terminal keeps) |
//! | `size` | `cols` | `{}`, then a `page` event once a page is open |
//! | `open` | `text`, `path` (the file it came from, which may not exist yet) | `rev`, then a `page` event |
//! | `tiles` | `rev`, `tiles` (indices) | `tiles`: `index`, `file`, `width`, `height`, `format` (`rgba` or `png`) |
//! | `link` | `row`, `col` | `link`: `kind` (`web`, `anchor`, `file` or `refused`) and `url`, `row`, `path` + `heading`, or `why` |
//! | `anchor` | `anchor` (a heading id) | `row`, or null |
//! | `press` | `row`, `col` | `copy` (text a copy button gave), or `{}` |
//!
//! Only the last of several `open` or `size` requests waiting at once counts; the others answer `skipped`, so
//! typing or a font zoom never queues stale renders. A `tiles` request for an old `rev` answers an error, since its rows moved.
//!
//! **Architecture invariant:** serve does not use `viewer`. It calls the `Engine` directly, as headless mode does,
//! and follows links through `crate::link`.
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::link::{self, Link};
use crate::render::engine::{Engine, Press};
use crate::term::{self, kitty, screen};

/// The protocol this `mdd` speaks. The host checks it in `hello`, since the plugin and `mdd` ship apart.
pub const PROTOCOL: u64 = 1;
/// Kitty's Unicode placeholders mark a cell's row and column with one of 297 diacritics each, so neither a tile's
/// rows nor the page's columns may pass it.
pub const MAX_CELLS: u32 = 297;
/// Rows per tile at most: small, so the host can keep only the tiles near the view and send the others again.
const TILE_ROWS: u32 = 48;
/// How long to wait for more input before polling late remote images again.
const POLL: Duration = Duration::from_millis(100);

pub fn run() -> i32 {
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(Some(line)).is_err() {
                return;
            }
        }
        let _ = tx.send(None);
    });
    let display = screen::display_scale();
    let mut s = Server {
        engine: Engine::new(800.0, display, crate::config::font_smoothing()),
        display,
        base_cell_h: 0,
        cell: term::fallback_cell(display),
        cols: 80,
        base: PathBuf::new(),
        rev: 0,
        loaded: false,
        png: false,
        out: std::io::stdout(),
        swept: false,
    };
    loop {
        let first = if s.engine.pending_images() > 0 {
            match rx.recv_timeout(POLL) {
                Ok(m) => Some(m),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            }
        };
        let Some(first) = first else {
            if s.engine.take_images() {
                s.changed();
            }
            continue;
        };
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        let end = batch.iter().any(Option::is_none);
        let reqs: Vec<Value> = batch.into_iter().flatten().filter_map(|l| serde_json::from_str(&l).ok()).collect();
        // Only the last waiting `open` or `size` counts: a font zoom sends sizes in a burst, and typing sends text.
        let last = |cmd: &str| reqs.iter().rposition(|r| r["cmd"] == cmd);
        let (last_open, last_size) = (last("open"), last("size"));
        for (i, req) in reqs.iter().enumerate() {
            if (req["cmd"] == "open" && Some(i) != last_open) || (req["cmd"] == "size" && Some(i) != last_size) {
                s.reply(req, Ok(json!({ "skipped": true })));
                continue;
            }
            let answer = s.handle(req);
            s.reply(req, answer);
            if s.loaded && req["cmd"] == "open" {
                s.page();
            }
        }
        if end {
            break;
        }
    }
    0
}

struct Server {
    engine: Engine,
    display: f64,
    /// The cell height of the first measure with pixels: a taller cell later is a font zoom.
    base_cell_h: u32,
    /// The cell in device pixels.
    cell: (u32, u32),
    cols: u32,
    /// The folder of the file shown, which links and images start from.
    base: PathBuf,
    rev: u64,
    loaded: bool,
    png: bool,
    out: std::io::Stdout,
    swept: bool,
}

impl Server {
    fn handle(&mut self, req: &Value) -> Result<Value, String> {
        match req["cmd"].as_str().unwrap_or_default() {
            "hello" => {
                let theirs = req["protocol"].as_u64().unwrap_or(0);
                if theirs != PROTOCOL {
                    return Err(format!("mdd {} speaks protocol {PROTOCOL}, but the plugin speaks {theirs}. Update the older one", env!("CARGO_PKG_VERSION")));
                }
                self.png = req["png"].as_bool().unwrap_or(false);
                self.engine.set_display_p3(req["p3"].as_bool().unwrap_or(false));
                Ok(json!({ "protocol": PROTOCOL, "version": env!("CARGO_PKG_VERSION"), "budget": kitty::storage_limit(term::herdr_by_env()) }))
            }
            "size" => {
                self.cols = req["cols"].as_u64().map_or(80, |c| c.clamp(1, u64::from(MAX_CELLS)) as u32);
                if self.loaded {
                    self.fit();
                    self.changed();
                }
                Ok(json!({}))
            }
            "open" => {
                let text = req["text"].as_str().unwrap_or_default();
                self.base = base_dir(req["path"].as_str().map(Path::new));
                self.fit();
                self.engine.load(text, &self.base, false, crate::config::theme());
                self.loaded = true;
                self.rev += 1;
                if !std::mem::replace(&mut self.swept, true) {
                    crate::sweep::spawn();
                }
                Ok(json!({ "rev": self.rev }))
            }
            "tiles" => {
                if req["rev"].as_u64() != Some(self.rev) {
                    return Err("stale".into());
                }
                let rows = self.rows();
                let tr = self.tile_rows();
                let mut out = vec![];
                for i in req["tiles"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
                    let top = i as u32 * tr;
                    if top >= rows {
                        continue;
                    }
                    out.push(self.tile(i, top, tr.min(rows - top))?);
                }
                Ok(json!({ "tiles": out }))
            }
            "link" => Ok(json!({ "link": self.link(req) })),
            "anchor" => Ok(json!({ "row": req["anchor"].as_str().and_then(|id| self.engine.anchor_y(id)).map(|y| self.row_of(y)) })),
            "press" => {
                let (x, y) = self.point(req);
                Ok(match self.engine.press(x, y) {
                    Press::Copy { text, .. } => {
                        self.changed();
                        json!({ "copy": text })
                    }
                    Press::Other | Press::Redraw(_) => {
                        self.changed();
                        json!({})
                    }
                    Press::Nothing | Press::Expand(_) => json!({}),
                })
            }
            other => Err(format!("unknown cmd {other:?}")),
        }
    }

    fn reply(&mut self, req: &Value, answer: Result<Value, String>) {
        let line = match answer {
            Ok(v) => json!({ "id": req["id"], "ok": v }),
            Err(e) => json!({ "id": req["id"], "error": e }),
        };
        self.send(&line);
    }

    fn send(&mut self, v: &Value) {
        let mut out = self.out.lock();
        // A host that went away ends the run at the next read of standard input.
        let _ = writeln!(out, "{v}").and_then(|()| out.flush());
    }

    /// Measures the cell and gives the engine the page width, and a new scale after a font zoom.
    fn fit(&mut self) {
        if let Some(cell) = measured_cell(self.display) {
            if self.base_cell_h == 0 {
                self.base_cell_h = cell.1;
            }
            self.cell = cell;
        }
        let scale = if self.base_cell_h == 0 { self.display } else { self.display * f64::from(self.cell.1) / f64::from(self.base_cell_h) };
        let css_width = f64::from(self.cols * self.cell.0) / scale;
        if (self.engine.scale.get() - scale).abs() > f64::EPSILON {
            self.engine.rescale(scale, css_width);
        } else {
            self.engine.resize(css_width);
        }
    }

    /// A new revision of the page, after a layout that kept the source.
    fn changed(&mut self) {
        self.rev += 1;
        self.page();
    }

    fn page(&mut self) {
        let l = self.engine.layout();
        let headings: Vec<Value> = l.headings.iter().map(|(level, text, y)| json!({ "level": level, "text": text, "row": self.row_of(*y) })).collect();
        let blocks: Vec<Value> = l
            .blocks
            .iter()
            .filter(|b| b.start > 0)
            .map(|b| {
                let row = self.row_of(b.top);
                json!({ "start": b.start, "end": b.end, "row": row, "rows": self.row_of(b.top + b.height).saturating_sub(row).max(1) })
            })
            .collect();
        let v = json!({
            "event": "page",
            "rev": self.rev,
            "cols": self.cols,
            "rows": self.rows(),
            "tile_rows": self.tile_rows(),
            "cell": [self.cell.0, self.cell.1],
            "headings": headings,
            "blocks": blocks,
        });
        self.send(&v);
    }

    fn scale(&self) -> f64 {
        self.engine.scale.get()
    }

    /// The page height in rows.
    fn rows(&self) -> u32 {
        ((self.engine.layout().height * self.scale()).ceil() as u32).div_ceil(self.cell.1).max(1)
    }

    fn tile_rows(&self) -> u32 {
        TILE_ROWS.min(kitty::tile_rows(self.cols * self.cell.0, self.cell.1)).clamp(1, MAX_CELLS)
    }

    /// The row that CSS pixel `y` falls in.
    fn row_of(&self, y: f64) -> u32 {
        ((y * self.scale()).max(0.0) / f64::from(self.cell.1)) as u32
    }

    /// The centre of a cell, in CSS pixels.
    fn point(&self, req: &Value) -> (f64, f64) {
        let (row, col) = (req["row"].as_f64().unwrap_or(0.0), req["col"].as_f64().unwrap_or(0.0));
        let s = self.scale();
        ((col + 0.5) * f64::from(self.cell.0) / s, (row + 0.5) * f64::from(self.cell.1) / s)
    }

    fn tile(&self, index: u64, top: u32, rows: u32) -> Result<Value, String> {
        let s = self.scale();
        let (y, h) = (f64::from(top * self.cell.1) / s, f64::from(rows * self.cell.1) / s);
        let t = self.engine.snapshot(y, h, s, |n| Some(vec![0; n])).ok_or("can't draw the page")?;
        let path = crate::tmp::graphics_file(&format!("{}-{index}", self.rev));
        let bytes = if self.png { kitty::png(t.width, t.height, &t.rgba).ok_or("can't encode PNG")? } else { t.rgba };
        crate::tmp::write_private(&path, &bytes).map_err(|e| format!("can't write {}: {}", path.display(), term::io_error(&e)))?;
        Ok(json!({ "index": index, "file": path, "width": t.width, "height": t.height, "format": if self.png { "png" } else { "rgba" } }))
    }

    fn link(&self, req: &Value) -> Value {
        let (x, y) = self.point(req);
        let Some(url) = self.engine.link(x, y) else { return Value::Null };
        match Link::of(&url) {
            Some(Link::Open(url)) => json!({ "kind": "web", "url": url }),
            Some(Link::Anchor(id)) => match self.engine.anchor_y(&id) {
                Some(y) => json!({ "kind": "anchor", "row": self.row_of(y) }),
                None => json!({ "kind": "refused", "why": format!("No heading #{id} on this page") }),
            },
            Some(Link::File(_)) => match link::follow(&url, &self.base) {
                link::Follow::Show(path, heading) => json!({ "kind": "file", "path": path, "heading": heading }),
                link::Follow::Refuse(why) => json!({ "kind": "refused", "why": why }),
            },
            None => json!({ "kind": "refused", "why": format!("mddog doesn't follow {url}") }),
        }
    }
}

/// The folder relative links and images start from: the file's, also for a file not saved yet, else the working
/// folder.
fn base_dir(path: Option<&Path>) -> PathBuf {
    let cwd = || std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(path) = path else { return cwd() };
    std::fs::canonicalize(path)
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .or_else(|| path.parent().filter(|d| !d.as_os_str().is_empty()).and_then(|d| std::fs::canonicalize(d).ok()))
        .unwrap_or_else(cwd)
}

/// The cell in device pixels, from the controlling terminal's window size. Reading the size doesn't send anything to the
/// terminal, so it can't mix with the host's own output.
fn measured_cell(scale: f64) -> Option<(u32, u32)> {
    let tty = std::fs::File::open("/dev/tty").ok()?;
    term::size_of(std::os::fd::AsRawFd::as_raw_fd(&tty)).cell_px(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_dir_takes_the_folder_of_a_file_not_saved_yet() {
        let dir = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(base_dir(Some(&dir.join("not-saved-yet.md"))), dir);
        assert_eq!(base_dir(None), std::env::current_dir().unwrap());
    }
}
