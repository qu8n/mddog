use std::path::Path;

pub mod native;

pub use native::Engine;

/// The scale the first native page lays out at while `display_scale` reads the real one: every Mac
/// screen since 2016 but an external 1x display.
pub const SCALE_GUESS: f64 = 2.0;

static SCALE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

/// The backing scale of the screen the terminal is on, from CoreGraphics. It needs a window-server
/// connection, about 25 ms, so the viewer starts it on a thread at launch and lays out at `SCALE_GUESS`
/// meanwhile. Unlike `NSScreen`, it runs off the main thread and starts no AppKit. `MDDOG_SCALE` overrides it, so
/// tests that click at terminal cells hit the same page points on a 1x CI runner as on a Retina Mac.
pub fn display_scale() -> f64 {
    *SCALE.get_or_init(|| {
        if let Some(s) = std::env::var("MDDOG_SCALE").ok().and_then(|s| s.parse().ok()).filter(|&s: &f64| s > 0.0) {
            return s;
        }
        use core_graphics::display::CGDisplay;
        let scale = |id| CGDisplay::new(id).display_mode().filter(|m| m.width() > 0).map(|m| m.pixel_width() as f64 / m.width() as f64);
        let scales: Vec<f64> = CGDisplay::active_displays().unwrap_or_default().into_iter().filter_map(scale).collect();
        if let Some(&first) = scales.first()
            && scales.iter().all(|&s| s == first)
        {
            return first;
        }
        // Screens of mixed scales: `NSScreen.mainScreen` meant the one with the focused window, which is the
        // terminal mddog was just typed into.
        front_window_center()
            .and_then(|p| CGDisplay::displays_with_point(p, 1).ok())
            .and_then(|(ids, n)| if n > 0 { ids.first().copied() } else { None })
            .or(Some(CGDisplay::main().id))
            .and_then(scale)
            .unwrap_or(SCALE_GUESS)
    })
}

/// The centre of the frontmost ordinary window, in global display coordinates. Bounds and layer need no
/// screen-recording permission.
fn front_window_center() -> Option<core_graphics::geometry::CGPoint> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly};
    type Dict = CFDictionary<CFString, CFType>;
    let dict = |v: &CFType| -> Option<Dict> {
        // SAFETY: the type ID check makes it a CFDictionary; the get rule retains it for the wrapper.
        (v.type_of() == Dict::type_id()).then(|| unsafe { Dict::wrap_under_get_rule(v.as_CFTypeRef().cast()) })
    };
    let num = |d: &Dict, k: &str| d.find(CFString::new(k)).and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_f64());
    let list = copy_window_info(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID)?;
    list.iter().find_map(|item| {
        // SAFETY: the window list holds CFDictionaries, alive while `list` is.
        let w = unsafe { Dict::wrap_under_get_rule((*item).cast()) };
        if num(&w, "kCGWindowLayer") != Some(0.0) {
            return None;
        }
        let b = dict(&*w.find(CFString::new("kCGWindowBounds"))?)?;
        Some(core_graphics::geometry::CGPoint::new(num(&b, "X")? + num(&b, "Width")? / 2.0, num(&b, "Y")? + num(&b, "Height")? / 2.0))
    })
}

/// A block of the page: its source lines and where it sits, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub start: u32,
    pub end: u32,
    pub top: f64,
    pub height: f64,
}

/// `[x, y, w, h]` in CSS pixels.
pub type Rect = [f64; 4];

/// The laid-out page: its source blocks, their text, the headings and the height, in CSS pixels.
#[derive(Default)]
pub struct Layout {
    pub blocks: Vec<Block>,
    pub texts: Vec<String>,
    pub headings: Vec<(u8, String, f64)>,
    pub height: f64,
}

/// What a click on the page did.
#[derive(Debug, PartialEq)]
pub enum Press {
    Nothing,
    Copy {
        text: String,
        rects: Vec<Rect>,
        label: String,
    },
    /// Opens Mermaid diagram `i` over the view.
    Expand(usize),
    /// Something on the page changed in place: draw these boxes again.
    Redraw(Vec<Rect>),
    /// The page changed, so lay it out again.
    Other,
}

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
