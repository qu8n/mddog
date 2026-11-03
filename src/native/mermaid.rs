//! Mermaid diagrams: mermaid-rs-renderer draws SVG, resvg rasterizes it.
use crate::native::images::svg_tree;
use crate::native::theme::Theme;
use mermaid_rs_renderer::{RenderOptions, Theme as MTheme};

fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

/// Pele's series colors for pie slices, the same in every theme.
const SERIES: [&str; 8] = ["#4c78a8", "#f58518", "#54a24b", "#e45756", "#72b7b2", "#eeca3b", "#b279a2", "#9d755d"];

/// The diagram as an SVG tree, in pele's colors: nodes on `pre-bg` with a `border` outline, on the page's own
/// background.
pub fn tree(src: &str, th: &Theme) -> Option<resvg::usvg::Tree> {
    svg_tree(svg(src, th)?.as_bytes())
}

fn svg(src: &str, th: &Theme) -> Option<String> {
    use mermaid_rs_renderer::DiagramKind;
    // The renderer's own parser skips comments, directives and front matter before the header.
    let kind = mermaid_rs_renderer::parse_mermaid(src).ok()?.graph.kind;
    let mut mt = if th.dark { MTheme::dark() } else { MTheme::modern() };
    mt.background = hex(th.bg);
    mt.text_color = hex(th.text);
    mt.primary_text_color = hex(th.text);
    mt.primary_color = hex(th.diagram_fill);
    mt.primary_border_color = hex(th.diagram_border);
    mt.line_color = hex(th.diagram_line);
    mt.secondary_color = hex(th.code_bg);
    mt.tertiary_color = hex(th.alt_bg);
    mt.cluster_background = hex(th.diagram_fill);
    mt.cluster_border = hex(th.diagram_border);
    mt.edge_label_background = hex(th.diagram_label_bg);
    mt.font_family = crate::native::images::svg_families(&th.font_body);
    // Sequence and pie diagrams have their own colors, which otherwise keep mermaid.js's palette.
    mt.sequence_actor_fill = hex(th.diagram_fill);
    mt.sequence_actor_border = hex(th.diagram_border);
    mt.sequence_actor_line = hex(th.diagram_line);
    mt.sequence_note_fill = hex(th.code_bg);
    mt.sequence_note_border = hex(th.diagram_border);
    mt.sequence_activation_fill = hex(th.diagram_fill);
    mt.sequence_activation_border = hex(th.diagram_border);
    mt.pie_colors = std::array::from_fn(|i| SERIES[i % SERIES.len()].to_string());
    mt.pie_opacity = 1.0;
    mt.pie_stroke_color = hex(th.bg);
    mt.pie_stroke_width = 1.0;
    mt.pie_outer_stroke_width = 0.0;
    // Slice labels are dark on pele's mid-tone series in every theme: the page color on a dark page, the text
    // color on a light one.
    let ink = if th.dark { th.bg } else { th.text };
    mt.pie_section_text_color = hex(ink);
    mt.pie_title_text_color = hex(th.text);
    mt.pie_title_text_size = 16.0;
    mt.pie_legend_text_color = hex(th.text);
    mt.pie_legend_text_size = 16.0;
    // Pele's percentages are smaller than its 16 px text: "40%" is 29 px wide, SF at 14 px.
    mt.pie_section_text_size = 14.0;
    // The renderer draws state labels at 0.85 of the font size, and pele at 16 px like the rest.
    mt.font_size = if kind == DiagramKind::State { 16.0 / 0.85 } else { 16.0 };
    // Pele's flowchart metrics, measured against the WebKit build: 16 px labels in the body font, 12 px inside
    // each node, 48 px between nodes, and labels wrapped near pele's 200 px (20 of the renderer's average
    // characters at 16 px). The renderer's own spacing, scaled by graph size, gave 25 to 41 px.
    let mut layout = mermaid_rs_renderer::LayoutConfig { node_padding_x: 12.0, node_padding_y: 12.0, max_label_width_chars: 20, node_spacing: 48.0, rank_spacing: 48.0, ..Default::default() };
    layout.flowchart.auto_spacing.enabled = false;
    // Pele's pie has a 120 px radius: the renderer's radius is half this height less its 28 px margin.
    layout.pie.height = 296.0;
    let opts = RenderOptions { theme: mt, layout };
    let svg = mermaid_rs_renderer::render_with_options(src, opts).ok()?;
    // CoreText tightens SF at 16 px by 0.31 px a letter from the font's tracking table, which resvg doesn't read.
    let svg = svg.replace(r#"font-family="System Font""#, r#"font-family="System Font" letter-spacing="-0.31""#);
    // Gantt grid lines come in a fixed light slate, which glares on a dark page.
    let svg = if kind == DiagramKind::Gantt { svg.replace(r##"stroke="#E2E8F0""##, &format!(r#"stroke="{}""#, hex(th.border))) } else { svg };
    // The renderer keeps the quotes around an ER relationship's label, which mermaid's syntax uses only to allow spaces.
    let svg = if kind == DiagramKind::Er { unquote_labels(&svg) } else { svg };
    // A small slice's label sits outside the pie on a box of the page color, so it takes the text color there.
    let svg = if kind == DiagramKind::Pie {
        let mark = format!(r#"fill="{}" stroke="none"/><text "#, hex(th.bg));
        let mut parts = svg.split(&mark);
        let mut out = parts.next().unwrap_or_default().to_string();
        for p in parts {
            out += &mark;
            out += &p.replacen(&format!(r#"fill="{}""#, hex(ink)), &format!(r#"fill="{}""#, hex(th.text)), 1);
        }
        out
    } else {
        svg
    };
    Some(svg)
}

/// `svg` with the quotes taken off each `tspan` whose whole text is in quotes. A quote inside a label, as in an
/// attribute's comment, stays.
fn unquote_labels(svg: &str) -> String {
    const Q: &str = "&quot;";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find("<tspan") {
        let Some(open) = rest[i..].find('>').map(|o| i + o + 1) else { break };
        let Some(close) = rest[open..].find("</tspan>").map(|c| open + c) else { break };
        let text = &rest[open..close];
        out += &rest[..open];
        match text.strip_prefix(Q).and_then(|t| t.strip_suffix(Q)) {
            Some(inner) if !inner.contains(Q) => out += inner,
            _ => out += text,
        }
        rest = &rest[close..];
    }
    out + rest
}

#[cfg(test)]
mod tests {
    fn svg(src: &str) -> String {
        super::svg(src, &super::Theme::new(crate::theme::builtin("github-dark").unwrap())).unwrap()
    }

    #[test]
    fn er_labels_lose_their_syntax_quotes() {
        let s = svg("erDiagram\n    CUSTOMER ||--o{ ORDER : \"places order\"\n");
        assert!(s.contains(">places order</tspan>"), "{s}");
        assert!(!s.contains("&quot;"));
    }

    #[test]
    fn er_attribute_comments_keep_their_quotes() {
        let s = svg("erDiagram\n    CUSTOMER {\n        string name \"customer name\"\n    }\n    CUSTOMER ||--o{ ORDER : \"places order\"\n");
        assert!(s.contains(">places order</tspan>"), "{s}");
        let quotes = |s: &str| s.matches("&quot;").count();
        assert_eq!(quotes(&s) % 2, 0, "no label loses only one quote: {s}");
        assert_eq!(super::unquote_labels(r#"<tspan x="1">&quot;a b&quot;</tspan><tspan>c &quot;d&quot;</tspan>"#), r#"<tspan x="1">a b</tspan><tspan>c &quot;d&quot;</tspan>"#);
    }

    #[test]
    fn a_preamble_keeps_the_kind_fixes() {
        let s = svg("---\ntitle: Orders\n---\n%% a comment\nerDiagram\n    CUSTOMER ||--o{ ORDER : \"places order\"\n");
        assert!(s.contains(">places order</tspan>"), "{s}");
        // Labels come out at 16 px, give or take float rounding, rather than the renderer's 13.6.
        let state = |src: &str| {
            let s = svg(src);
            let at = s.find(r#"font-size=""#).unwrap() + 11;
            (s[at..].split('"').next().unwrap().parse::<f32>().unwrap() - 16.0).abs() < 0.01
        };
        assert!(state("stateDiagram-v2\n    [*] --> Idle\n"));
        assert!(state("%% a comment\nstateDiagram-v2\n    [*] --> Idle\n"));
    }

    #[test]
    fn a_small_slice_label_shows_outside_the_pie() {
        let s = svg("pie\n    \"Most\" : 99\n    \"Few\" : 1\n");
        assert!(s.contains("stroke=\"none\"/><text "), "the renderer put a label outside: {s}");
        // Each outside label is drawn in github-dark's text color, not its page color.
        assert!(s.split("stroke=\"none\"/><text ").skip(1).all(|p| p.split('>').next().unwrap().contains("fill=\"#f0f6fc\"")), "{s}");
    }

    #[test]
    fn slice_labels_are_dark_in_every_theme() {
        for name in crate::theme::names().split(", ") {
            let th = super::Theme::new(crate::theme::builtin(name).unwrap());
            let s = super::svg("pie\n    \"A\" : 50\n    \"B\" : 50\n", &th).unwrap();
            let ink = if th.dark { th.bg } else { th.text };
            assert!(s.contains(&format!("fill=\"#{ink:06x}\">50")), "{name}: {s}");
            // Relative luminance well under the series colors', so the labels read on every slice.
            let (r, g, b) = super::super::theme::rgb(ink);
            assert!(0.2126 * r + 0.7152 * g + 0.0722 * b < 0.15, "{name}");
        }
    }

    #[test]
    fn pie_and_gantt_take_the_theme() {
        let s = svg("pie title Pets\n    \"Dogs\" : 40\n    \"Cats\" : 60\n");
        assert!(s.contains("#4c78a8") && s.contains("#f58518"), "pele's series colors");
        let s = svg("gantt\n    dateFormat YYYY-MM-DD\n    section A\n    Task :a1, 2026-01-01, 7d\n");
        assert!(!s.contains("#E2E8F0") && s.contains("#3d444d"), "grid lines take the border color");
    }
}
