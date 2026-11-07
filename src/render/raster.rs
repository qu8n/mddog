//! Draws display-list items into RGBA tiles with CoreGraphics.
use crate::render::icons;
use crate::render::page::*;
use crate::render::text::Fonts;
use crate::render::theme::{Theme, rgb};
use core_graphics::base::kCGImageAlphaPremultipliedLast;
use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::data_provider::CGDataProvider;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::image::CGImage;
use std::rc::Rc;
use std::sync::Arc;

/// Things drawn over the page that are not part of the display list.
#[derive(Default, Clone)]
pub struct Overlay {
    /// Highlight rects (page coordinates): (x, y, w, h, current)
    pub marks: Vec<(f32, f32, f32, f32, bool)>,
    pub sel: Vec<(f32, f32, f32, f32)>,
    /// Horizontal scroll offsets per hscroll region.
    pub hoff: Vec<f32>,
    /// Hovered button index and the one that shows "Copied!".
    pub hover_btn: Option<usize>,
    pub copied_btn: Option<usize>,
    pub expanded: Option<Expanded>,
}

/// An expanded diagram over the view: the box it covers and the diagram, drawn to fit it (device px).
#[derive(Clone)]
pub struct Expanded {
    pub y: f32,
    pub h: f32,
    pub bmp: Rc<Bitmap>,
    pub x: f32,
    pub top: f32,
}

pub struct Ctx<'a> {
    pub page: &'a Page,
    pub th: &'a Theme,
    pub scale: f32,
    pub ov: &'a Overlay,
    /// Font smoothing, which CoreGraphics bitmap contexts leave off unless asked.
    pub smooth: bool,
}

fn set_fill(c: &CGContext, rgbv: u32, a: f64) {
    let (r, g, b) = rgb(rgbv);
    c.set_rgb_fill_color(r, g, b, a);
}
fn set_stroke(c: &CGContext, rgbv: u32, a: f64) {
    let (r, g, b) = rgb(rgbv);
    c.set_rgb_stroke_color(r, g, b, a);
}
fn argb(c: &CGContext, v: u32, stroke: bool) {
    let a = ((v >> 24) & 255) as f64 / 255.0;
    if stroke { set_stroke(c, v & 0xffffff, a) } else { set_fill(c, v & 0xffffff, a) }
}

fn rrect(c: &CGContext, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    c.begin_path();
    let (x2, y2) = (x + w, y + h);
    if r <= 0.0 {
        c.move_to_point(x, y);
        c.add_line_to_point(x2, y);
        c.add_line_to_point(x2, y2);
        c.add_line_to_point(x, y2);
        c.close_path();
        return;
    }
    let k = r * 0.5522847498;
    c.move_to_point(x + r, y);
    c.add_line_to_point(x2 - r, y);
    c.add_curve_to_point(x2 - r + k, y, x2, y + r - k, x2, y + r);
    c.add_line_to_point(x2, y2 - r);
    c.add_curve_to_point(x2, y2 - r + k, x2 - r + k, y2, x2 - r, y2);
    c.add_line_to_point(x + r, y2);
    c.add_curve_to_point(x + r - k, y2, x, y2 - r + k, x, y2 - r);
    c.add_line_to_point(x, y + r);
    c.add_curve_to_point(x, y + r - k, x + r - k, y, x + r, y);
    c.close_path();
}

pub fn cg_image(b: &Bitmap) -> CGImage {
    let cs = CGColorSpace::create_device_rgb();
    let prov = CGDataProvider::from_buffer(Arc::new(b.px.clone()));
    CGImage::new(b.w as usize, b.h as usize, 8, 32, b.w as usize * 4, &cs, kCGImageAlphaPremultipliedLast, &prov, true, 0)
}

/// Draws page rows from `top` (layout pixels) into `out`, a `w` by `h` RGBA buffer, at `z` output pixels per
/// layout pixel.
#[expect(clippy::too_many_arguments, reason = "the tile's place, size and scale, each a plain number")]
pub fn render_into(fonts: &mut Fonts, cx: &Ctx, top: f32, h: usize, w: usize, items: &[u32], z: f64, out: &mut [u8]) {
    let cs = CGColorSpace::create_device_rgb();
    // CoreGraphics draws into `out`, which holds w * h * 4 bytes and outlives the context: the context drops before this returns.
    let ctx = CGContext::create_bitmap_context(Some(out.as_mut_ptr().cast()), w, h, 8, w * 4, &cs, kCGImageAlphaPremultipliedLast);
    ctx.set_allows_font_smoothing(cx.smooth);
    ctx.set_should_smooth_fonts(cx.smooth);
    ctx.set_allows_antialiasing(true);
    ctx.set_should_antialias(true);
    ctx.set_allows_font_subpixel_positioning(true);
    ctx.set_should_subpixel_position_fonts(true);
    ctx.set_allows_font_subpixel_quantization(false);
    ctx.translate(0.0, h as f64);
    ctx.scale(1.0, -1.0);
    ctx.scale(z, z);
    ctx.translate(0.0, -(top as f64));
    set_fill(&ctx, cx.th.bg, 1.0);
    ctx.fill_rect(CGRect::new(&CGPoint::new(0.0, top as f64), &CGSize::new(w as f64 / z, h as f64 / z)));
    let page = cx.page;
    let s = cx.scale as f64;
    for &i in items {
        let it = &page.items[i as usize];
        let (dx, clip_rect) = if it.clip > 0 {
            let hs = &page.hscroll[it.clip as usize - 1];
            let off = cx.ov.hoff.get(it.clip as usize - 1).copied().unwrap_or(0.0);
            (-off, Some(hs))
        } else {
            (0.0, None)
        };
        ctx.save();
        if let Some(hs) = clip_rect {
            ctx.clip_to_rect(CGRect::new(&CGPoint::new(hs.x as f64, hs.y as f64), &CGSize::new(hs.w as f64, hs.h as f64)));
        }
        let (x, y, iw, ih) = ((it.x + dx) as f64, it.y as f64, it.w as f64, it.h as f64);
        match &it.k {
            Kind::Rect { fill, stroke, radius } => {
                let half = stroke.map(|(w, _)| w as f64 / 2.0).unwrap_or(0.0);
                if (fill >> 24) != 0 {
                    argb(&ctx, *fill, false);
                    rrect(&ctx, x, y, iw, ih, *radius as f64);
                    ctx.fill_path();
                }
                if let Some((sw, sc)) = stroke {
                    argb(&ctx, *sc, true);
                    ctx.set_line_width(*sw as f64);
                    rrect(&ctx, x + half, y + half, iw - 2.0 * half, ih - 2.0 * half, (*radius as f64 - half).max(0.0));
                    ctx.stroke_path();
                }
            }
            Kind::Text { s: t, f, color, under, strike, asc, .. } => {
                set_fill(&ctx, *color, 1.0);
                fonts.draw(&ctx, *f, t, x as f32, it.y + asc);
                if *under || *strike {
                    set_fill(&ctx, *color, 1.0);
                    let (u, st, th) = f.lines();
                    for (on, dy) in [(*under, u), (*strike, st)] {
                        if on {
                            ctx.fill_rect(CGRect::new(&CGPoint::new(x, f64::from(it.y + asc) + f64::from(dy)), &CGSize::new(iw, f64::from(th))));
                        }
                    }
                }
            }
            Kind::Img(b) => {
                let img = cg_image(b);
                ctx.save();
                // flip the image back upright in our y-down space
                ctx.translate(x, y + ih);
                ctx.scale(1.0, -1.0);
                ctx.draw_image(CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(iw, ih)), &img);
                ctx.restore();
            }
            Kind::Poly { pts, color, width, fill } => {
                argb(&ctx, *color, *fill);
                argb(&ctx, *color, !*fill);
                ctx.set_line_width(*width as f64);
                ctx.set_line_cap(core_graphics::context::CGLineCap::CGLineCapRound);
                ctx.set_line_join(core_graphics::context::CGLineJoin::CGLineJoinRound);
                ctx.begin_path();
                for (i, p) in pts.iter().enumerate() {
                    let (px, py) = (x + p.0 as f64, y + p.1 as f64);
                    if i == 0 {
                        ctx.move_to_point(px, py);
                    } else {
                        ctx.add_line_to_point(px, py);
                    }
                }
                if *fill {
                    ctx.close_path();
                    ctx.fill_path();
                } else {
                    ctx.stroke_path();
                }
            }
        }
        ctx.restore();
    }
    for (i, hs) in page.hscroll.iter().enumerate() {
        if hs.max <= 0.0 || hs.y > top + h as f32 || hs.y + hs.h < top {
            continue;
        }
        let off = cx.ov.hoff.get(i).copied().unwrap_or(0.0);
        let (tx, tw) = hs.thumb(off, s as f32);
        set_fill(&ctx, cx.th.fg_muted, 0.55);
        rrect(&ctx, f64::from(tx), f64::from(hs.bar_y), f64::from(tw), f64::from(HScroll::BAR) * s, f64::from(HScroll::BAR) / 2.0 * s);
        ctx.fill_path();
    }
    for (i, b) in page.buttons.iter().enumerate() {
        if b.w <= 0.0 || b.y > top + h as f32 || b.y + b.h < top {
            continue;
        }
        draw_button(&ctx, cx, b, cx.ov.hover_btn == Some(i), cx.ov.copied_btn == Some(i), z);
        let hover = cx.ov.hover_btn == Some(i);
        let copied = cx.ov.copied_btn == Some(i);
        let label = if copied {
            Some("Copied!")
        } else if hover && b.kind == BtnKind::CopyCode {
            Some("Copy code to clipboard")
        } else {
            None
        };
        if let Some(t) = label {
            let sc = cx.scale;
            let f = crate::render::text::FontKey::new(12.0 * sc, 0);
            let tw = fonts.measure(f, t);
            let (a, d) = fonts.ascent_descent(f);
            let (px, py) = (4.0 * sc, 3.0 * sc);
            let bx = b.x - 6.0 * sc - tw - 2.0 * px;
            let by = b.y + (b.h - (a + d + 2.0 * py)) / 2.0;
            set_fill(&ctx, cx.th.tooltip_bg, 1.0);
            rrect(&ctx, bx as f64, by as f64, (tw + 2.0 * px) as f64, (a + d + 2.0 * py) as f64, 6.0 * s);
            ctx.fill_path();
            set_fill(&ctx, 0xffffff, 1.0);
            fonts.draw(&ctx, f, t, bx + px, by + py + a);
        }
    }
    for &(x, y, w, hh, cur) in &cx.ov.marks {
        if y > top + h as f32 || y + hh < top {
            continue;
        }
        set_fill(&ctx, if cur { cx.th.hl_cur_bg } else { cx.th.hl_bg }, if cur { 0.75 } else { 0.55 });
        ctx.fill_rect(CGRect::new(&CGPoint::new(x as f64, y as f64), &CGSize::new(w as f64, hh as f64)));
    }
    for &(x, y, w, hh) in &cx.ov.sel {
        if y > top + h as f32 || y + hh < top {
            continue;
        }
        set_fill(&ctx, cx.th.sel_bg, 0.6);
        ctx.fill_rect(CGRect::new(&CGPoint::new(x as f64, y as f64), &CGSize::new(w as f64, hh as f64)));
    }
    if let Some(e) = &cx.ov.expanded
        && e.y <= top + h as f32
        && e.y + e.h >= top
    {
        set_fill(&ctx, cx.th.bg, 1.0);
        ctx.fill_rect(CGRect::new(&CGPoint::new(0.0, e.y as f64), &CGSize::new(page.w as f64, e.h as f64)));
        let img = cg_image(&e.bmp);
        let (iw, ih) = (e.bmp.w as f64, e.bmp.h as f64);
        ctx.save();
        ctx.clip_to_rect(CGRect::new(&CGPoint::new(0.0, e.y as f64), &CGSize::new(page.w as f64, e.h as f64)));
        ctx.translate(e.x as f64, e.top as f64 + ih);
        ctx.scale(1.0, -1.0);
        ctx.draw_image(CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(iw, ih)), &img);
        ctx.restore();
    }
    drop(ctx);
}

/// Draws a diagram button: GitHub's bordered box for copy and expand, and a bare icon for the zoom bar,
/// faded when off. `z` is output pixels per layout pixel, so the icon is drawn sharp.
fn draw_button(ctx: &CGContext, cx: &Ctx, b: &Button, hover: bool, copied: bool, z: f64) {
    if b.kind == BtnKind::CopyCode {
        return draw_code_button(ctx, cx, b, hover, copied);
    }
    let s = cx.scale as f64;
    let (x, y, w, h) = (b.x as f64, b.y as f64, b.w as f64, b.h as f64);
    let color = match b.kind {
        BtnKind::Expand | BtnKind::CopyDiagram => {
            // The pointer changes only the cursor here.
            set_fill(ctx, cx.th.button_bg, 1.0);
            rrect(ctx, x, y, w, h, 6.0 * s);
            ctx.fill_path();
            set_stroke(ctx, cx.th.border, 1.0);
            ctx.set_line_width(s);
            rrect(ctx, x + s / 2.0, y + s / 2.0, w - s, h - s, 5.5 * s);
            ctx.stroke_path();
            if copied { cx.th.copied } else { cx.th.fg_muted }
        }
        _ => cx.th.text,
    };
    let size = 16.0 * s;
    let Some(img) = icons::bitmap(icons::Icon::of(b.kind, copied), color, (size * z).round() as u32) else { return };
    ctx.save();
    if b.off {
        ctx.set_alpha(0.35);
    }
    ctx.translate(x + ((w - size) / 2.0).round(), y + ((h - size) / 2.0).round() + size);
    ctx.scale(1.0, -1.0);
    ctx.draw_image(CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(size, size)), &cg_image(&img));
    ctx.restore();
}

/// A code block's copy button.
fn draw_code_button(ctx: &CGContext, cx: &Ctx, b: &Button, hover: bool, copied: bool) {
    let s = cx.scale as f64;
    let (x, y, w, h) = (b.x as f64, b.y as f64, b.w as f64, b.h as f64);
    set_fill(ctx, cx.th.pre_bg, 1.0);
    rrect(ctx, x, y, w, h, 6.0 * s);
    ctx.fill_path();
    if hover {
        set_fill(ctx, cx.th.rule, 0.6);
        rrect(ctx, x, y, w, h, 6.0 * s);
        ctx.fill_path();
    }
    set_stroke(ctx, cx.th.fg_muted, 1.0);
    ctx.set_line_width(1.4 * s);
    ctx.set_line_cap(core_graphics::context::CGLineCap::CGLineCapRound);
    ctx.set_line_join(core_graphics::context::CGLineJoin::CGLineJoinRound);
    let (mx, my) = (x + w / 2.0, y + h / 2.0);
    if copied {
        set_stroke(ctx, cx.th.copied, 1.0);
        ctx.set_line_width(2.0 * s);
        ctx.begin_path();
        ctx.move_to_point(mx - 5.0 * s, my);
        ctx.add_line_to_point(mx - 1.5 * s, my + 4.0 * s);
        ctx.add_line_to_point(mx + 5.5 * s, my - 4.0 * s);
        ctx.stroke_path();
    } else {
        rrect(ctx, mx - 3.0 * s, my - 6.0 * s, 9.0 * s, 10.0 * s, 1.5 * s);
        ctx.stroke_path();
        rrect(ctx, mx - 6.0 * s, my - 3.0 * s, 9.0 * s, 10.0 * s, 1.5 * s);
        set_fill(ctx, cx.th.pre_bg, 1.0);
        ctx.draw_path(core_graphics::context::CGPathDrawingMode::CGPathFillStroke);
    }
}
