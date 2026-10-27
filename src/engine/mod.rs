use std::path::Path;
use std::time::Duration;

pub mod spare;
mod webview;

pub use webview::{backing_scale, spin};

pub const CALL_LIMIT: Duration = Duration::from_secs(10);

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
}

pub fn snapshot_size(width: f64, h: f64, scale: f64) -> (usize, usize) {
    ((width * scale).round() as usize, (h * scale).round() as usize)
}

pub enum Engine {
    Local(webview::Engine),
    Remote(spare::Client),
}

impl Engine {
    pub fn new(width: f64) -> Engine {
        Engine::Local(webview::Engine::new(width))
    }

    pub fn load_blank(&self) {
        if let Engine::Local(e) = self {
            e.load_blank();
        }
    }

    /// Whether WebKit blocks loads to private hosts (see `remote::content_rules`). A spare serves only once its
    /// own rules are in place.
    pub fn content_rules(&self) -> bool {
        match self {
            Engine::Local(e) => e.content_rules(),
            Engine::Remote(_) => true,
        }
    }

    pub fn load_file(&self, html: &Path, dark: bool) {
        match self {
            Engine::Local(e) => e.load_file(html, dark),
            Engine::Remote(c) => c.load_file(html, dark),
        }
    }

    pub fn eval(&self, js: &str) -> Result<String, String> {
        match self {
            Engine::Local(e) => e.eval(js),
            Engine::Remote(c) => c.eval(js),
        }
    }

    /// Like `eval`, but a reply later than `limit` marks the engine stalled.
    pub fn eval_within(&self, js: &str, limit: Duration) -> Result<String, String> {
        match self {
            Engine::Local(e) => e.eval_within(js, limit),
            Engine::Remote(c) => c.eval_within(js, limit),
        }
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        match self {
            Engine::Local(e) => e.resize(width, height),
            Engine::Remote(c) => c.resize(width, height),
        }
    }

    pub fn snapshot(&self, y: f64, h: f64, scale: f64) -> Option<Bitmap> {
        match self {
            Engine::Local(e) => e.snapshot(y, h, scale),
            Engine::Remote(c) => c.snapshot(y, h, scale),
        }
    }

    pub fn snapshot_into(&self, y: f64, h: f64, scale: f64, dst: &mut [u8]) -> Option<()> {
        match self {
            Engine::Local(e) => e.snapshot_into(y, h, scale, dst),
            Engine::Remote(_) => None,
        }
    }

    pub fn render(&self, src: &str, dir: &std::path::Path) -> Option<crate::Doc> {
        match self {
            Engine::Local(_) => None,
            Engine::Remote(c) => c.render(src, dir),
        }
    }

    pub fn width(&self) -> f64 {
        match self {
            Engine::Local(e) => e.width,
            Engine::Remote(c) => c.width.get(),
        }
    }

    pub fn scale(&self) -> f64 {
        match self {
            Engine::Local(_) => backing_scale(),
            Engine::Remote(c) => c.scale,
        }
    }

    pub fn stalled(&self) -> bool {
        match self {
            Engine::Local(e) => e.stalled.get(),
            Engine::Remote(c) => c.stalled.get(),
        }
    }

    pub fn wait_js(&self, js: &str, timeout: Duration) -> bool {
        match self {
            Engine::Local(e) => e.wait_js(js, timeout),
            Engine::Remote(c) => c.wait_js(js, timeout),
        }
    }

    pub fn wait_loaded(&self, timeout: Duration) -> bool {
        self.wait_js("location.protocol === 'file:' && !document.documentElement.dataset.stale && document.readyState === 'complete'", timeout)
    }

    pub fn wait_frame(&self) {
        let _ = self.eval("window.mddogFrame = false; requestAnimationFrame(() => { window.mddogFrame = true; }); ''");
        self.wait_js("window.mddogFrame", Duration::from_millis(100));
    }
}
