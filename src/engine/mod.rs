use std::path::Path;
use std::time::Duration;

use crate::page::script::{self, Layout, Press, Rect};

pub mod native;
pub mod spare;
mod webview;

pub use webview::{backing_scale, spin};

pub const CALL_LIMIT: Duration = Duration::from_secs(10);

const NO_SCRIPTS: &str = "the native engine runs no scripts";

/// The mouse pointer a spot on the page asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Default,
    Text,
    Hand,
}

/// A selection change, in CSS pixels.
#[derive(Clone, Copy, Debug)]
pub enum Select {
    /// From the anchor point to the focus point.
    Range(f64, f64, f64, f64),
    /// The word at a point.
    Word(f64, f64),
    Clear,
}

/// Content that shows up after the first paint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Late {
    pub math: bool,
    pub mermaid: bool,
    pub remote: bool,
}

impl Late {
    pub fn any(self) -> bool {
        self.math || self.mermaid || self.remote
    }

    /// The WebKit condition that holds once all of it is done.
    fn js(self) -> String {
        let parts: Vec<&str> = [(self.math, script::MATH_DONE), (self.mermaid, script::MERMAID_DONE), (self.remote, script::REMOTE_DONE)].into_iter().filter(|p| p.0).map(|p| p.1).collect();
        if parts.is_empty() { "true".into() } else { parts.join(" && ") }
    }
}

pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub rgba: crate::term::kitty::Shm,
}

impl Bitmap {
    pub fn write_png(&self, path: &Path) -> Result<(), png::EncodingError> {
        let f = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), self.width as u32, self.height as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header()?.write_image_data(&self.rgba)
    }

    /// WebP at libwebp's method 2. Lossless takes about a sixth of `write_png`'s bytes for a page of text, so a
    /// tile fits in the 128 KiB `Svg` source the Claude Code desktop app draws; lossy is for pictures, and for
    /// the small copies it draws where sharp tiles don't fit.
    /// Lossy at `quality` when given, else lossless.
    pub fn webp(&self, quality: Option<f32>) -> Result<Vec<u8>, String> {
        let mut cfg = webp::WebPConfig::new().map_err(|()| "can't set up the WebP encoder".to_string())?;
        cfg.lossless = i32::from(quality.is_none());
        // Lossless reads it as effort.
        cfg.quality = quality.unwrap_or(75.0);
        cfg.method = 2;
        let mem = webp::Encoder::from_rgba(&self.rgba[..], self.width as u32, self.height as u32).encode_advanced(&cfg).map_err(|e| format!("can't encode WebP: {e:?}"))?;
        Ok(mem.to_vec())
    }
}

pub fn snapshot_size(width: f64, h: f64, scale: f64) -> (usize, usize) {
    ((width * scale).round() as usize, (h * scale).round() as usize)
}

pub enum Engine {
    Local(webview::Engine),
    Remote(spare::Client),
    Native(Box<native::Engine>),
}

/// Whether this run uses the native renderer: the default on this branch. `MDDOG_ENGINE=webkit` picks WebKit.
pub fn native() -> bool {
    std::env::var("MDDOG_ENGINE").map_or(true, |v| v != "webkit")
}

impl Engine {
    pub fn new(width: f64) -> Engine {
        Engine::Local(webview::Engine::new(width))
    }

    pub fn new_native(width: f64) -> Engine {
        Engine::Native(Box::new(native::Engine::new(width, backing_scale())))
    }

    pub fn is_native(&self) -> bool {
        matches!(self, Engine::Native(_))
    }

    /// Lays out `src` natively. Returns false on WebKit, which loads a built HTML page instead.
    pub fn load_markdown(&self, src: &str, base: &Path, wrap: bool, theme: &crate::theme::Theme) -> bool {
        match self {
            Engine::Native(n) => {
                n.load(src, base, wrap, theme);
                true
            }
            _ => false,
        }
    }

    pub fn load_blank(&self) {
        if let Engine::Local(e) = self {
            e.load_blank();
        }
    }

    /// Whether WebKit blocks direct http and https image and media loads (see `remote::content_rules`). A spare
    /// serves only once its own rules are in place.
    pub fn content_rules(&self) -> bool {
        match self {
            Engine::Local(e) => e.content_rules(),
            Engine::Remote(_) | Engine::Native(_) => true,
        }
    }

    pub fn load_file(&self, html: &Path, dark: bool) {
        match self {
            Engine::Local(e) => e.load_file(html, dark),
            Engine::Remote(c) => c.load_file(html, dark),
            Engine::Native(_) => {}
        }
    }

    /// Runs `js` in the WebKit page. The native engine runs no scripts.
    pub fn eval(&self, js: &str) -> Result<String, String> {
        match self {
            Engine::Local(e) => e.eval(js),
            Engine::Remote(c) => c.eval(js),
            Engine::Native(_) => Err(NO_SCRIPTS.into()),
        }
    }

    /// Like `eval`, but a reply later than `limit` marks the engine stalled.
    pub fn eval_within(&self, js: &str, limit: Duration) -> Result<String, String> {
        match self {
            Engine::Local(e) => e.eval_within(js, limit),
            Engine::Remote(c) => c.eval_within(js, limit),
            Engine::Native(_) => Err(NO_SCRIPTS.into()),
        }
    }

    /// The reply to a WebKit call, or empty when it failed.
    fn web(&self, js: &str) -> String {
        self.eval(js).unwrap_or_default()
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        match self {
            Engine::Local(e) => e.resize(width, height),
            Engine::Remote(c) => c.resize(width, height),
            Engine::Native(n) => n.resize(width),
        }
    }

    pub fn snapshot(&self, y: f64, h: f64, scale: f64) -> Option<Bitmap> {
        match self {
            Engine::Local(e) => e.snapshot(y, h, scale),
            Engine::Remote(c) => c.snapshot(y, h, scale),
            Engine::Native(n) => n.snapshot(y, h, scale),
        }
    }

    pub fn snapshot_into(&self, y: f64, h: f64, scale: f64, dst: &mut [u8]) -> Option<()> {
        match self {
            Engine::Local(e) => e.snapshot_into(y, h, scale, dst),
            Engine::Remote(_) => None,
            Engine::Native(n) => n.snapshot_into(y, h, scale, dst),
        }
    }

    pub fn render(&self, src: &str, dir: &std::path::Path) -> Option<crate::Doc> {
        match self {
            Engine::Local(_) => None,
            Engine::Remote(c) => c.render(src, dir),
            // The native engine parses the source itself, so the HTML render is skipped.
            Engine::Native(_) => Some(crate::Doc::default()),
        }
    }

    pub fn width(&self) -> f64 {
        match self {
            Engine::Local(e) => e.width,
            Engine::Remote(c) => c.width.get(),
            Engine::Native(n) => n.width.get(),
        }
    }

    pub fn scale(&self) -> f64 {
        match self {
            Engine::Local(_) => backing_scale(),
            Engine::Remote(c) => c.scale,
            Engine::Native(n) => n.scale,
        }
    }

    pub fn stalled(&self) -> bool {
        match self {
            Engine::Local(e) => e.stalled.get(),
            Engine::Remote(c) => c.stalled.get(),
            Engine::Native(_) => false,
        }
    }

    pub fn wait_js(&self, js: &str, timeout: Duration) -> bool {
        match self {
            Engine::Local(e) => e.wait_js(js, timeout),
            Engine::Remote(c) => c.wait_js(js, timeout),
            Engine::Native(_) => true,
        }
    }

    pub fn wait_loaded(&self, timeout: Duration) -> bool {
        self.wait_js("location.protocol === 'file:' && !document.documentElement.dataset.stale && document.readyState === 'complete'", timeout)
    }

    pub fn wait_frame(&self) {
        if self.is_native() {
            return;
        }
        let _ = self.eval("window.mddogFrame = false; requestAnimationFrame(() => { window.mddogFrame = true; }); ''");
        self.wait_js("window.mddogFrame", Duration::from_millis(100));
    }

    pub fn layout(&self) -> Layout {
        match self {
            Engine::Native(n) => n.layout(),
            _ => script::layout(&self.web("mddogLayout()")),
        }
    }

    /// Finds `query` and marks the match at `keep`, or the first. Returns each match's rects.
    pub fn search(&self, query: &str, keep: Option<usize>) -> Vec<Vec<Rect>> {
        match self {
            Engine::Native(n) => n.search(query, keep),
            _ => serde_json::from_str(&self.web(&format!("mddogSearch.run({}, {})", serde_json::Value::from(query), keep.map_or(-1, |k| k as i64)))).unwrap_or_default(),
        }
    }

    /// Marks match `i` as the current one. Returns the old and new current match's rects.
    pub fn search_current(&self, i: usize) -> Vec<Rect> {
        match self {
            Engine::Native(n) => n.search_current(i),
            _ => script::rects(&self.web(&format!("mddogSearch.setCurrent({i})"))),
        }
    }

    pub fn search_clear(&self) {
        match self {
            Engine::Native(n) => n.search_clear(),
            _ => drop(self.eval("mddogSearch.clear()")),
        }
    }

    /// The pointer at a point, and the rects of the buttons whose hover changed. A point off the page is (-1, -1).
    pub fn hover(&self, x: f64, y: f64, slack: f64) -> (Pointer, Vec<Rect>) {
        match self {
            Engine::Native(n) => n.hover(x, y),
            _ => {
                let (shape, rects) = serde_json::from_str::<(String, Vec<Rect>)>(&self.web(&format!("mddogMouse.hover({x},{y},{slack})"))).unwrap_or_default();
                let pointer = match shape.as_str() {
                    "text" => Pointer::Text,
                    "pointer" => Pointer::Hand,
                    _ => Pointer::Default,
                };
                (pointer, rects)
            }
        }
    }

    /// The link at a point: its target as written, and resolved.
    pub fn link(&self, x: f64, y: f64) -> Option<(String, String)> {
        match self {
            Engine::Native(n) => n.link(x, y),
            _ => serde_json::from_str(&self.web(&format!("mddogMouse.link({x},{y})"))).ok(),
        }
    }

    /// Presses the button or toggle at a point.
    pub fn press(&self, x: f64, y: f64) -> Press {
        match self {
            Engine::Native(n) => n.press(x, y),
            _ => script::press(&self.web(&format!("mddogMouse.press({x},{y})"))),
        }
    }

    /// Ends a copy button's "copied" look. Returns the rect to redraw.
    pub fn reset_copy(&self) -> Vec<Rect> {
        match self {
            Engine::Native(n) => n.reset_copy(),
            _ => script::rects(&self.web("mddogMouse.resetCopy()")),
        }
    }

    /// Where the anchor `id` sits, in CSS pixels.
    pub fn anchor_y(&self, id: &str) -> Option<f64> {
        match self {
            Engine::Native(n) => n.anchor_y(id),
            _ => self.web(&format!("mddogMouse.anchorY({})", serde_json::Value::from(id))).parse::<f64>().ok(),
        }
        .filter(|y| *y >= 0.0)
    }

    /// Changes the selection. Returns how many rects the old selection had, then its rects and the new one's.
    pub fn select(&self, sel: Select) -> (usize, Vec<Rect>) {
        if let Engine::Native(n) = self {
            return n.select(sel);
        }
        let call = match sel {
            Select::Range(ax, ay, fx, fy) => format!("mddogMouse.select({ax},{ay},{fx},{fy})"),
            Select::Word(x, y) => format!("mddogMouse.word({x},{y})"),
            Select::Clear => "mddogMouse.clear()".into(),
        };
        let json =
            self.web(&format!("(() => {{ const s = getSelection(); const n = s.rangeCount ? s.getRangeAt(0).getClientRects().length : 0; return JSON.stringify([n, JSON.parse({call})]); }})()"));
        serde_json::from_str(&json).unwrap_or_default()
    }

    pub fn selection_text(&self) -> String {
        match self {
            Engine::Native(n) => n.selection_text(),
            _ => self.web("getSelection().toString()"),
        }
    }

    /// Whether a resize must reload the page: a selection, or an image that picks its source by width.
    pub fn resize_needs_reload(&self) -> bool {
        match self {
            Engine::Native(n) => n.has_selection(),
            _ => self.eval("String(!getSelection().isCollapsed || !!document.querySelector('picture, img[srcset]'))").as_deref() != Ok("false"),
        }
    }

    /// Opens Mermaid diagram `i` over the view. Returns the rects to redraw; none when it didn't open.
    pub fn expand(&self, i: i64, top: f64, height: f64) -> Vec<Rect> {
        match self {
            Engine::Native(n) => usize::try_from(i).map_or_else(|_| vec![], |i| n.expand(i, top, height)),
            _ => script::rects(&self.web(&format!("mddogMouse.expand({i},{top},{height})"))),
        }
    }

    pub fn collapse(&self) -> Vec<Rect> {
        match self {
            Engine::Native(n) => n.collapse(),
            _ => script::rects(&self.web("mddogMouse.collapse()")),
        }
    }

    /// Grabs a wide block's scroll bar near a point. Returns the block's rect, or None off every bar.
    pub fn code_grab(&self, x: f64, y: f64, slack: f64) -> Option<Vec<Rect>> {
        match self {
            Engine::Native(n) => n.code_grab(x, y, slack),
            _ => Some(self.web(&format!("mddogCode.grab({x},{y},{slack})"))).filter(|j| !j.is_empty()).map(|j| script::rects(&j)),
        }
    }

    pub fn code_drag(&self, x: f64) -> Vec<Rect> {
        match self {
            Engine::Native(n) => n.code_drag(x),
            _ => script::rects(&self.web(&format!("mddogCode.drag({x})"))),
        }
    }

    pub fn code_release(&self) {
        match self {
            Engine::Native(n) => n.code_release(),
            _ => drop(self.eval("mddogCode.release()")),
        }
    }

    /// Scrolls the wide block at a point sideways by `dx`.
    pub fn code_scroll(&self, x: f64, y: f64, dx: f64) -> Vec<Rect> {
        match self {
            Engine::Native(n) => n.code_scroll(x, y, dx),
            _ => script::rects(&self.web(&format!("mddogCode.scroll({x},{y},{dx})"))),
        }
    }

    /// Scrolls every wide block between `top` and `bottom` sideways by `dx`.
    pub fn code_scroll_view(&self, top: f64, bottom: f64, dx: f64) -> Vec<Rect> {
        match self {
            Engine::Native(n) => n.code_scroll_view(top, bottom, dx),
            _ => script::rects(&self.web(&format!("mddogCode.scrollView ? mddogCode.scrollView({top},{bottom},{dx}) : '[]'"))),
        }
    }

    /// The wide blocks' sideways offsets, to give `code_restore` after a reload.
    pub fn code_positions(&self) -> String {
        match self {
            Engine::Native(n) => n.code_positions(),
            _ => self.web("mddogCode.positions ? mddogCode.positions() : '[]'"),
        }
    }

    pub fn code_restore(&self, saved: &str) {
        match self {
            Engine::Native(n) => n.code_restore(saved),
            _ => drop(self.eval(&format!("mddogCode.restore && mddogCode.restore({})", serde_json::Value::from(saved)))),
        }
    }

    pub fn set_wrap(&self, wrap: bool) {
        match self {
            Engine::Native(n) => n.set_wrap(wrap),
            _ => drop(self.eval(&format!("mddogCode.wrap({wrap})"))),
        }
    }

    /// Gives WebKit the diagram sizes saved from earlier runs, so the page lays out at them before they draw.
    pub fn guess_diagrams(&self, sizes: &str) {
        if !self.is_native() {
            let _ = self.eval(&format!("mddog.guess({sizes}); ''"));
        }
    }

    /// Gives WebKit the image sizes saved from earlier runs.
    pub fn guess_images(&self, sizes: &str) {
        if !self.is_native() {
            let _ = self.eval(&format!("mddog.guessImages({sizes}); ''"));
        }
    }

    /// The diagram and image sizes the page recorded, as JSON maps. A reply later than `limit` marks WebKit stalled.
    pub fn recorded_sizes(&self, images: bool, limit: Duration) -> Option<String> {
        if self.is_native() {
            return None;
        }
        self.eval_within(if images { "JSON.stringify(mddog.state.images)" } else { "JSON.stringify(mddog.state.sizes)" }, limit).ok()
    }

    pub fn start_mermaid(&self) {
        if !self.is_native() {
            let _ = self.eval(script::START_MERMAID);
        }
    }

    /// Starts loading the page's remote images and media, each given up after `timeout`. Returns false when it has none.
    pub fn load_remote(&self, timeout: Duration) -> bool {
        match self {
            Engine::Native(n) => n.pending_images() > 0,
            _ => crate::page::remote::load(self, timeout),
        }
    }

    /// Whether all of `late` is done, without waiting. The native engine lays out again as images arrive.
    pub fn late_done(&self, late: Late) -> bool {
        match self {
            Engine::Native(n) => n.images_done(),
            _ => self.eval(&format!("String({})", late.js())).as_deref() == Ok("true"),
        }
    }

    /// Waits up to `timeout` for all of `late`. Returns false when it ran out.
    pub fn wait_late(&self, late: Late, timeout: Duration) -> bool {
        match self {
            Engine::Native(n) => {
                if late.remote {
                    n.wait_images(timeout);
                }
                true
            }
            _ => self.wait_js(&late.js(), timeout),
        }
    }

    /// Waits up to 100 ms for the page to take a width or height set by `resize`. False when it didn't.
    pub fn wait_size(&self, width: Option<f64>, height: Option<f64>) -> bool {
        let mut cond = vec![];
        if let Some(w) = width {
            cond.push(format!("Math.abs(innerWidth - {w}) < 1"));
        }
        if let Some(h) = height {
            cond.push(format!("Math.abs(innerHeight - {h}) < 1"));
        }
        cond.is_empty() || self.wait_js(&cond.join(" && "), Duration::from_millis(100))
    }

    /// Waits up to 100 ms for the diagrams to refit after a resize. Pele refits each diagram in a ResizeObserver
    /// callback. Those run in the next rendering update, after its animation frame callbacks, so this waits for
    /// a task after it.
    pub fn wait_refit(&self) -> bool {
        if self.is_native() {
            return true;
        }
        let _ = self.eval("window.mddogFrame = false; requestAnimationFrame(() => setTimeout(() => { window.mddogFrame = true; })); ''");
        self.wait_js("window.mddogFrame", Duration::from_millis(100))
    }
}
