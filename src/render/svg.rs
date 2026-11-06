//! SVG: parsing with the system's fonts, and drawing into bitmaps. Images, diagrams and icons all draw through it.
use std::sync::{Arc, OnceLock};

use resvg::usvg;

use crate::render::page::Bitmap;

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for f in [
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/System/Library/Fonts/Menlo.ttc",
        ] {
            if let Ok(d) = std::fs::read(f) {
                db.load_font_data(d);
            }
        }
        db.set_sans_serif_family("Helvetica");
        db.set_serif_family("Helvetica");
        db.set_monospace_family("Menlo");
        Arc::new(db)
    })
    .clone()
}

/// The SF family's name in `fontdb`: macOS's system font file calls itself this.
pub const SYSTEM_FONT: &str = "System Font";

/// A theme's font list for SVG text: CSS's names for the system font become SF's family, which `fontdb` knows,
/// so diagram labels draw in SF, not in the Helvetica fallback.
/// The names after the system font never draw, so they are left out.
pub fn svg_families(fonts: &[String]) -> String {
    let mut out = Vec::new();
    for f in fonts {
        match f.to_ascii_lowercase().as_str() {
            // CoreText knows SF Pro by name, but `fontdb` only by the system font file's own name.
            n if crate::render::text::system_ui(n) || matches!(n, "sf pro" | "sf pro text" | "sf pro display") => {
                out.push(SYSTEM_FONT.to_string());
                break;
            }
            _ => out.push(f.clone()),
        }
    }
    out.join(", ")
}

pub fn svg_tree(data: &[u8]) -> Option<usvg::Tree> {
    let o = usvg::Options { fontdb: fontdb(), font_family: "Helvetica".into(), ..usvg::Options::default() };
    usvg::Tree::from_data(data, &o).ok()
}

pub fn render_svg(t: &usvg::Tree, w: u32, h: u32) -> Option<Bitmap> {
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let s = t.size();
    let tf = resvg::tiny_skia::Transform::from_scale(w as f32 / s.width(), h as f32 / s.height());
    resvg::render(t, tf, &mut pm.as_mut());
    Some(Bitmap { w: pm.width(), h: pm.height(), px: pm.take() })
}

/// A `w` by `h` window onto `t` drawn at `z` pixels per SVG unit, with SVG point (`x`, `y`) at its top left.
pub fn render_view(t: &usvg::Tree, w: u32, h: u32, z: f32, x: f32, y: f32) -> Option<Bitmap> {
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    resvg::render(t, resvg::tiny_skia::Transform::from_row(z, 0.0, 0.0, z, -x * z, -y * z), &mut pm.as_mut());
    Some(Bitmap { w: pm.width(), h: pm.height(), px: pm.take() })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_system_font_names_reach_sf() {
        let fonts = ["system-ui", "-apple-system", "Avenir Next"].map(String::from);
        assert_eq!(super::svg_families(&fonts), "System Font");
        let fonts = ["Avenir Next", "ui-sans-serif", "Inter"].map(String::from);
        assert_eq!(super::svg_families(&fonts), "Avenir Next, System Font");
        // SF is in the font set under that name, so a label in it doesn't fall back to Helvetica.
        assert!(super::fontdb().faces().any(|f| f.families.iter().any(|(n, _)| n == super::SYSTEM_FONT)));
    }
}
