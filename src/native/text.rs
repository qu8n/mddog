//! Fonts, text measurement and drawing through CoreText.
use core_foundation::attributed_string::CFMutableAttributedString;
use core_foundation::base::{CFRange, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::context::CGContext;
use core_graphics::geometry::CGAffineTransform;
use core_text::font::{CTFont, new_from_name, new_ui_font_for_language};
use core_text::font_descriptor::{kCTFontBoldTrait, kCTFontItalicTrait};
use core_text::line::CTLine;
use core_text::string_attributes::{kCTFontAttributeName, kCTForegroundColorFromContextAttributeName, kCTKernAttributeName};
use std::collections::HashMap;

pub const BOLD: u16 = 1;
pub const ITALIC: u16 = 2;
pub const MONO: u16 = 4;
pub const MATH: u16 = 8;
/// Digits all one width, as CSS's `font-variant-numeric: tabular-nums`.
pub const TABULAR: u16 = 16;
/// The style bits above the flags hold a CSS font weight, 0 for the font's own.
const WEIGHT_SHIFT: u16 = 5;
pub const WEIGHT_MASK: u16 = !0 << WEIGHT_SHIFT;
/// Style bits for CSS font weight `w`, 1 to 1000.
pub const fn weight(w: u16) -> u16 {
    w << WEIGHT_SHIFT
}
/// Weight 600, as GitHub draws strong text and table headers.
pub const SEMIBOLD: u16 = weight(600);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FKey {
    /// Size in 1/16 px.
    pub size: u32,
    pub style: u16,
    /// Letter spacing in 1/1000 em, as CSS's `letter-spacing`.
    pub track: i16,
}

impl FKey {
    pub fn new(size: f32, style: u16) -> FKey {
        FKey { size: (size * 16.0).round() as u32, style, track: 0 }
    }
    /// The same font with `em` of letter spacing.
    pub fn tracked(self, em: f32) -> FKey {
        FKey { track: (em * 1000.0).round() as i16, ..self }
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
    /// Output pixels per CSS pixel.
    scale: f32,
}

impl Fonts {
    pub fn new() -> Fonts {
        Fonts { fonts: HashMap::new(), lines: HashMap::new(), body: vec![], code: vec![], scale: 1.0 }
    }

    /// Draws with the theme's fonts from now on.
    pub fn set_families(&mut self, body: Vec<String>, code: Vec<String>) {
        if (&body, &code) != (&self.body, &self.code) {
            (self.body, self.code) = (body, code);
            self.fonts.clear();
            self.lines.clear();
        }
    }

    /// Lays out at `scale` output pixels per CSS pixel from now on.
    pub fn set_scale(&mut self, scale: f32) {
        if scale != self.scale {
            self.scale = scale;
            self.fonts.clear();
            self.lines.clear();
        }
    }

    pub fn font(&mut self, k: FKey) -> &CTFont {
        let k = FKey { track: 0, ..k };
        let (body, code, scale) = (&self.body, &self.code, self.scale);
        self.fonts.entry(k).or_insert_with(|| at_scale(make_font(k.px() / scale, k.style, if k.style & MONO != 0 { code } else { body }), scale))
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
            // SAFETY: both keys are CoreText's own attribute names, and each value has the type its key expects.
            unsafe {
                a.set_attribute(CFRange::init(0, len), kCTFontAttributeName, &font);
                a.set_attribute(CFRange::init(0, len), kCTForegroundColorFromContextAttributeName, &CFBoolean::true_value());
                if k.track != 0 {
                    a.set_attribute(CFRange::init(0, len), kCTKernAttributeName, &CFNumber::from(f64::from(k.track) / 1000.0 * f64::from(k.px())));
                }
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
        let u16idx: usize = s.chars().take(idx).map(char::len_utf16).sum();
        self.line(k, s).0.get_string_offset_for_string_index(u16idx as isize) as f32
    }

    pub fn draw(&mut self, ctx: &CGContext, k: FKey, s: &str, x: f32, y: f32) {
        let l = self.line(k, s).0.clone();
        ctx.set_text_matrix(&CGAffineTransform::new(1.0, 0.0, 0.0, -1.0, x as f64, y as f64));
        l.draw(ctx);
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
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
                if let Ok(f) = new_from_name(n, size)
                    && [f.family_name(), f.postscript_name(), f.display_name()].iter().any(|got| got.eq_ignore_ascii_case(n))
                {
                    return f;
                }
            }
        }
    }
    if mono { new_from_name("Menlo-Regular", size).unwrap_or_else(|_| new_ui_font_for_language(2, size, None)) } else { new_ui_font_for_language(2, size, None) }
}

// `NSFont` lives in AppKit, which nothing else links.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

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

unsafe extern "C" {
    fn CTFontCreateWithFontDescriptor(descriptor: core_text::font_descriptor::CTFontDescriptorRef, size: f64, matrix: *const CGAffineTransform) -> core_text::font::CTFontRef;
}

/// `f` drawn `scale` times larger. CoreText picks the system font's optical size and tracking from the point
/// size, so a 16 px body font made at 32 pt for a Retina screen gets the display cut, about 6% narrower than
/// WebKit's text. Scaling a 16 pt font keeps the text cut, as WebKit does.
fn at_scale(f: CTFont, scale: f32) -> CTFont {
    if scale == 1.0 {
        return f;
    }
    let m = CGAffineTransform::new(scale as f64, 0.0, 0.0, scale as f64, 0.0, 0.0);
    // SAFETY: the descriptor and matrix outlive the call, and the create rule takes the returned font's reference.
    unsafe { CTFont::wrap_under_create_rule(CTFontCreateWithFontDescriptor(f.copy_descriptor().as_concrete_TypeRef(), f.pt_size(), &m)) }
}

/// CSS font weights and the CoreText weight traits macOS gives them, as `NSFontWeight` names them.
const WEIGHTS: [(f64, f64); 9] = [(100.0, -0.8), (200.0, -0.6), (300.0, -0.4), (400.0, 0.0), (500.0, 0.23), (600.0, 0.3), (700.0, 0.4), (800.0, 0.56), (900.0, 0.62)];

/// `x` mapped through the table's straight segments: from the pairs' first value to their second, or back when `back`.
fn through_weights(x: f64, back: bool) -> f64 {
    let pairs = WEIGHTS.map(|(c, t)| if back { (t, c) } else { (c, t) });
    let x = x.clamp(pairs[0].0, pairs[8].0);
    let i = pairs.windows(2).position(|w| x <= w[1].0).unwrap_or(7);
    let ((x0, y0), (x1, y1)) = (pairs[i], pairs[i + 1]);
    y0 + (x - x0) / (x1 - x0) * (y1 - y0)
}

/// `f` at CSS weight `css`. CoreText draws a variable family such as SF at any weight, and a family of fixed
/// cuts at its nearest one, the heavier on a tie from 400 up and the lighter below, as CSS leans.
fn weighted(f: &CTFont, css: u16) -> CTFont {
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_text::font_descriptor::{TraitAccessors, kCTFontTraitsAttribute, kCTFontWeightTrait};
    let css = f64::from(css);
    let want = through_weights(css, false);
    let with_trait = |f: &CTFont, t: f64| {
        // SAFETY: both keys are CoreText's own constant strings.
        let (traits, weight) = unsafe { (CFString::wrap_under_get_rule(kCTFontTraitsAttribute), CFString::wrap_under_get_rule(kCTFontWeightTrait)) };
        let w = CFDictionary::from_CFType_pairs(&[(weight, CFNumber::from(t))]);
        let attrs = CFDictionary::from_CFType_pairs(&[(traits, w.as_CFType())]);
        f.copy_descriptor().create_copy_with_attributes(attrs.to_untyped()).ok().map(|d| core_text::font::new_from_descriptor(&d, f.pt_size()))
    };
    let got = |f: &CTFont| f.all_traits().normalized_weight();
    if let Some(n) = with_trait(f, want)
        && (got(&n) - want).abs() < 0.02
    {
        return n;
    }
    // CoreText hands back the regular cut when a fixed family lacks the weight, so pick from the family's cuts.
    // Upright cuts of the same width only, so a condensed or italic cut never stands in.
    let width = f.all_traits().normalized_width();
    let cuts: Vec<(f64, core_text::font_descriptor::CTFontDescriptor)> = core_text::font_collection::create_for_family(&f.family_name())
        .and_then(|c| c.get_descriptors())
        .map(|ds| {
            ds.iter()
                .filter(|d| d.traits().symbolic_traits() & kCTFontItalicTrait == 0 && (d.traits().normalized_width() - width).abs() < 0.05)
                .map(|d| (d.traits().normalized_weight(), d.clone()))
                .collect()
        })
        .unwrap_or_default();
    let off = |t: f64| {
        let c = through_weights(t, true);
        // CSS looks heavier first from 400 up and lighter first below, so a tie goes that way.
        ((c - css).abs(), if css >= 400.0 { -c } else { c })
    };
    match cuts.into_iter().min_by(|a, b| off(a.0).partial_cmp(&off(b.0)).unwrap_or(std::cmp::Ordering::Equal)) {
        Some((_, d)) => core_text::font::new_from_descriptor(&d, f.pt_size()),
        // No family to choose from: CoreText's bold stands in for any heavy weight.
        None if css >= 600.0 => f.clone_with_symbolic_traits(kCTFontBoldTrait, kCTFontBoldTrait).unwrap_or_else(|| f.clone()),
        None => f.clone(),
    }
}

/// The font for CSS size `px` and `style`.
fn make_font(px: f32, style: u16, names: &[String]) -> CTFont {
    let size = px as f64;
    if style & MATH != 0 {
        let n = match (style & BOLD != 0, style & ITALIC != 0) {
            (false, false) => "STIXGeneral-Regular",
            (false, true) => "STIXGeneral-Italic",
            (true, false) => "STIXGeneral-Bold",
            (true, true) => "STIXGeneral-BoldItalic",
        };
        if let Ok(f) = new_from_name(n, size) {
            return f;
        }
    }
    let mut f = family(names, size, style & MONO != 0);
    let mut tr = 0;
    let w = style >> WEIGHT_SHIFT;
    if w != 0 {
        f = weighted(&f, w);
    } else if style & BOLD != 0 {
        tr |= kCTFontBoldTrait;
    }
    if style & ITALIC != 0 {
        tr |= kCTFontItalicTrait;
    }
    if tr != 0
        && let Some(n) = f.clone_with_symbolic_traits(tr, tr)
    {
        f = n;
    }
    if style & TABULAR != 0 {
        f = tabular(&f);
    }
    f
}

unsafe extern "C" {
    fn CTFontDescriptorCreateCopyWithFeature(
        original: core_text::font_descriptor::CTFontDescriptorRef,
        feature_type: core_foundation::number::CFNumberRef,
        feature_selector: core_foundation::number::CFNumberRef,
    ) -> core_text::font_descriptor::CTFontDescriptorRef;
}

/// `f` with monospaced digits: AAT feature type 6 (number spacing), selector 0.
fn tabular(f: &CTFont) -> CTFont {
    let (kind, sel) = (CFNumber::from(6i32), CFNumber::from(0i32));
    // SAFETY: every argument outlives its call, and the create rule takes each returned reference.
    unsafe {
        let d = CTFontDescriptorCreateCopyWithFeature(f.copy_descriptor().as_concrete_TypeRef(), kind.as_concrete_TypeRef(), sel.as_concrete_TypeRef());
        if d.is_null() {
            return f.clone();
        }
        let d = core_text::font_descriptor::CTFontDescriptor::wrap_under_create_rule(d);
        CTFont::wrap_under_create_rule(CTFontCreateWithFontDescriptor(d.as_concrete_TypeRef(), f.pt_size(), std::ptr::null()))
    }
}

#[cfg(test)]
mod weight_tests {
    use super::*;
    use core_text::font_descriptor::TraitAccessors;

    fn font(style: u16, names: &[&str]) -> CTFont {
        let names: Vec<String> = names.iter().map(std::string::ToString::to_string).collect();
        make_font(24.0, style, &names)
    }

    fn w(style: u16, names: &[&str]) -> f64 {
        font(style, names).all_traits().normalized_weight()
    }

    #[test]
    fn sf_draws_every_weight() {
        let ws: Vec<f64> = (1..=9).map(|n| w(weight(n * 100), &["system-ui"])).collect();
        assert!(ws.windows(2).all(|p| p[0] < p[1]), "SF gets heavier at each step: {ws:?}");
        // The in-between weights Obsidian's headings use land between the named cuts.
        let (six, sixty_eight, seven) = (w(SEMIBOLD, &["system-ui"]), w(weight(680), &["system-ui"]), w(weight(700), &["system-ui"]));
        assert!(six < sixty_eight && sixty_eight < seven, "{six} {sixty_eight} {seven}");
        // A whole hundred takes SF's named cut, as WebKit does.
        assert_eq!(font(SEMIBOLD, &["system-ui"]).postscript_name(), ".SFNS-Semibold");
        assert!(font(SEMIBOLD | ITALIC, &["system-ui"]).all_traits().normalized_weight() > 0.25, "italic keeps the weight");
    }

    #[test]
    fn fixed_families_take_the_nearest_cut() {
        // Menlo has only regular and bold.
        assert_eq!(w(SEMIBOLD | MONO, &["Menlo"]), w(BOLD | MONO, &["Menlo"]));
        assert_eq!(w(weight(500) | MONO, &["Menlo"]), w(MONO, &["Menlo"]));
        assert_eq!(w(weight(100) | MONO, &["Menlo"]), w(MONO, &["Menlo"]));
        // Helvetica Neue has a medium (500) and a bold (700): 600 ties and goes heavier, 550 goes to medium.
        let name = |style| font(style, &["Helvetica Neue"]).postscript_name();
        assert_eq!(name(SEMIBOLD), "HelveticaNeue-Bold");
        assert_eq!(name(weight(550)), "HelveticaNeue-Medium");
        assert_eq!(name(weight(300)), "HelveticaNeue-Light");
        // 450 ties regular and medium, and CSS looks up first.
        assert_eq!(name(weight(450)), "HelveticaNeue-Medium");
    }

    #[test]
    fn weights_map_through_the_table() {
        assert_eq!(through_weights(600.0, false), 0.3);
        assert!((through_weights(650.0, false) - 0.35).abs() < 1e-9);
        assert!((through_weights(through_weights(680.0, false), true) - 680.0).abs() < 1e-9);
        assert_eq!(through_weights(1000.0, false), 0.62);
    }
}
