//! Fonts, text measurement and drawing through CoreText.
use core_foundation::attributed_string::CFMutableAttributedString;
use core_foundation::base::{CFRange, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use core_graphics::context::CGContext;
use core_graphics::geometry::CGAffineTransform;
use core_text::font::{CTFont, new_from_name, new_ui_font_for_language};
use core_text::font_descriptor::{kCTFontBoldTrait, kCTFontItalicTrait};
use core_text::line::CTLine;
use core_text::string_attributes::{kCTFontAttributeName, kCTForegroundColorFromContextAttributeName};
use std::collections::HashMap;

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const MONO: u8 = 4;
pub const MATH: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FKey {
    /// Size in 1/16 px.
    pub size: u32,
    pub style: u8,
}

impl FKey {
    pub fn new(size: f32, style: u8) -> FKey {
        FKey { size: (size * 16.0).round() as u32, style }
    }
    pub fn px(&self) -> f32 {
        self.size as f32 / 16.0
    }
}

pub struct Fonts {
    fonts: HashMap<FKey, CTFont>,
    lines: HashMap<(FKey, String), (CTLine, f32)>,
    /// The theme's body and code font lists, first choice first.
    body: Vec<String>,
    code: Vec<String>,
}

impl Fonts {
    pub fn new() -> Fonts {
        Fonts { fonts: HashMap::new(), lines: HashMap::new(), body: vec![], code: vec![] }
    }

    /// Draws with the theme's fonts from now on.
    pub fn set_families(&mut self, body: Vec<String>, code: Vec<String>) {
        if (&body, &code) != (&self.body, &self.code) {
            (self.body, self.code) = (body, code);
            self.fonts.clear();
            self.lines.clear();
        }
    }

    pub fn font(&mut self, k: FKey) -> &CTFont {
        let (body, code) = (&self.body, &self.code);
        self.fonts.entry(k).or_insert_with(|| make_font(k, if k.style & MONO != 0 { code } else { body }))
    }

    pub fn ascent_descent(&mut self, k: FKey) -> (f32, f32) {
        let f = self.font(k);
        (f.ascent() as f32, f.descent() as f32)
    }

    fn line(&mut self, k: FKey, s: &str) -> &(CTLine, f32) {
        if self.lines.len() > 20000 {
            self.lines.clear();
        }
        if !self.lines.contains_key(&(k, s.to_string())) {
            let font = self.font(k).clone();
            let cf = CFString::new(s);
            let mut a = CFMutableAttributedString::new();
            a.replace_str(&cf, CFRange::init(0, 0));
            let len = a.char_len();
            unsafe {
                a.set_attribute(CFRange::init(0, len), kCTFontAttributeName, &font);
                a.set_attribute(CFRange::init(0, len), kCTForegroundColorFromContextAttributeName, &CFBoolean::true_value());
            }
            let line = CTLine::new_with_attributed_string(a.as_concrete_TypeRef());
            let w = line.get_typographic_bounds().width as f32;
            self.lines.insert((k, s.to_string()), (line, w));
        }
        &self.lines[&(k, s.to_string())]
    }

    pub fn measure(&mut self, k: FKey, s: &str) -> f32 {
        self.line(k, s).1
    }

    /// x offset of the char at `idx` (in chars) from the start of the string.
    pub fn char_x(&mut self, k: FKey, s: &str, idx: usize) -> f32 {
        let total = s.chars().count();
        if idx == 0 {
            return 0.0;
        }
        if idx >= total {
            return self.measure(k, s);
        }
        let u16idx: usize = s.chars().take(idx).map(|c| c.len_utf16()).sum();
        self.line(k, s).0.get_string_offset_for_string_index(u16idx as isize) as f32
    }

    pub fn draw(&mut self, ctx: &CGContext, k: FKey, s: &str, x: f32, y: f32) {
        let l = self.line(k, s).0.clone();
        ctx.set_text_matrix(&CGAffineTransform::new(1.0, 0.0, 0.0, -1.0, x as f64, y as f64));
        l.draw(ctx);
    }
}

/// The first font of `names` that is installed, as CSS's `font-family` picks. `system-ui` and its kin are the system font, and `ui-monospace` and `monospace` its monospaced cut.
fn family(names: &[String], size: f64, mono: bool) -> CTFont {
    for n in names {
        match n.to_ascii_lowercase().as_str() {
            "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "sans-serif" => return new_ui_font_for_language(2, size, None),
            "ui-monospace" | "monospace" => {
                if let Some(f) = system_mono(size) {
                    return f;
                }
            }
            _ => {
                // CoreText answers an unknown name with a stand-in, so check the name it gave.
                if let Ok(f) = new_from_name(n, size) {
                    if [f.family_name(), f.postscript_name(), f.display_name()].iter().any(|got| got.eq_ignore_ascii_case(n)) {
                        return f;
                    }
                }
            }
        }
    }
    if mono { new_from_name("Menlo-Regular", size).unwrap_or_else(|_| new_ui_font_for_language(2, size, None)) } else { new_ui_font_for_language(2, size, None) }
}

/// SF Mono, which only AppKit names.
fn system_mono(size: f64) -> Option<CTFont> {
    objc2::rc::autoreleasepool(|_| {
        let cls = objc2::runtime::AnyClass::get(c"NSFont")?;
        // SAFETY: `monospacedSystemFontOfSize:weight:` takes two CGFloats and returns an autoreleased NSFont, which is toll-free bridged to CTFontRef. The get rule retains it before the pool drains.
        unsafe {
            let f: *const objc2::runtime::AnyObject = objc2::msg_send![cls, monospacedSystemFontOfSize: size, weight: 0.0f64];
            (!f.is_null()).then(|| CTFont::wrap_under_get_rule(f as core_text::font::CTFontRef))
        }
    })
}

fn make_font(k: FKey, names: &[String]) -> CTFont {
    let size = k.px() as f64;
    if k.style & MATH != 0 {
        let n = match (k.style & BOLD != 0, k.style & ITALIC != 0) {
            (false, false) => "STIXGeneral-Regular",
            (false, true) => "STIXGeneral-Italic",
            (true, false) => "STIXGeneral-Bold",
            (true, true) => "STIXGeneral-BoldItalic",
        };
        if let Ok(f) = new_from_name(n, size) {
            return f;
        }
    }
    let mut f = family(names, size, k.style & MONO != 0);
    let mut tr = 0;
    if k.style & BOLD != 0 {
        tr |= kCTFontBoldTrait;
    }
    if k.style & ITALIC != 0 {
        tr |= kCTFontItalicTrait;
    }
    if tr != 0 {
        if let Some(n) = f.clone_with_symbolic_traits(tr, tr) {
            f = n;
        }
    }
    f
}
