//! Mermaid diagrams: mermaid-rs-renderer draws SVG, resvg rasterizes it.
use crate::native::images::{render_svg, svg_tree};
use crate::native::page::Bitmap;
use crate::native::theme::Theme;
use mermaid_rs_renderer::{RenderOptions, Theme as MTheme};

fn hex(c: u32) -> String {
    format!("#{:06x}", c)
}

pub fn render(src: &str, th: &Theme, max_css_w: f32, scale: f32) -> Option<Bitmap> {
    let _ = max_css_w;
    let mut mt = if th.dark { MTheme::dark() } else { MTheme::modern() };
    let obs = th.mermaid_obsidian;
    let accent = if obs {
        th.accent
    } else if th.dark {
        0x1f6feb
    } else {
        0x0969da
    };
    mt.background = hex(th.pre_bg);
    mt.text_color = hex(th.text);
    mt.primary_text_color = hex(th.text);
    mt.primary_color = hex(if th.dark { 0x1c2128 } else { 0xffffff });
    mt.primary_border_color = hex(if obs { accent } else { th.fg_muted });
    mt.line_color = hex(th.fg_muted);
    mt.secondary_color = hex(th.code_bg);
    mt.tertiary_color = hex(th.alt_bg);
    mt.cluster_background = hex(th.code_bg);
    mt.cluster_border = hex(th.rule);
    mt.edge_label_background = hex(th.pre_bg);
    mt.font_family = "Helvetica, Arial, sans-serif".to_string();
    let opts = RenderOptions { theme: mt, layout: Default::default() };
    let svg = mermaid_rs_renderer::render_with_options(src, opts).ok()?;
    let tree = svg_tree(svg.as_bytes())?;
    let sz = tree.size();
    let z = scale * 1.0;
    let w = (sz.width() * z).round().max(1.0) as u32;
    let h = (sz.height() * z).round().max(1.0) as u32;
    render_svg(&tree, w, h)
}
