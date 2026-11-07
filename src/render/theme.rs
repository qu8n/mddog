//! The theme's tokens as the renderer draws with them.

use crate::render::doc::Alert;
use crate::render::highlight::TokenKind;

pub use crate::theme::Rgb;

/// Which reading view the page copies. Most of the difference is in the fields of `Theme`; layout asks this
/// only where the two draw a part with different shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// GitHub's README.
    Github,
    /// Obsidian's reading view.
    Obsidian,
}

/// Syntax colors, one per highlight class.
#[derive(Clone, Debug)]
pub struct SyntaxColors {
    pub comment: Rgb,
    pub keyword: Rgb,
    pub string: Rgb,
    pub constant: Rgb,
    pub entity: Rgb,
    pub inserted: Rgb,
    pub deleted: Rgb,
    /// Variables and tags, in the obsidian layout.
    pub variable: Rgb,
    pub tag: Rgb,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    /// From `--mddog-layout`.
    pub look: Look,
    pub bg: Rgb,
    pub fg_muted: Rgb,
    pub text: Rgb,
    pub heading: Rgb,
    pub link: Rgb,
    pub border: Rgb,
    /// A diagram's copy and expand buttons.
    pub button_bg: Rgb,
    pub code_bg: Rgb,
    pub pre_bg: Rgb,
    pub alt_bg: Rgb,
    pub rule: Rgb,
    pub mark_bg: Rgb,
    pub hl_bg: Rgb,
    pub hl_cur_bg: Rgb,
    pub sel_bg: Rgb,
    /// By `Alert as usize`.
    pub alerts: [Rgb; 5],
    /// A copy button's check: the tip alert's green, as on GitHub.
    pub copied: Rgb,
    pub diff_add_bg: Rgb,
    pub diff_del_bg: Rgb,
    pub pl: SyntaxColors,
    pub font_body: Vec<String>,
    pub font_code: Vec<String>,
    pub heading_scale: [f32; 6],
    pub heading_weight: [u16; 6],
    pub line_height: f32,
    pub code_size: f32,
    pub heading_6: Rgb,
    /// The arrow after a link out of the page, or none.
    pub link_external: Option<Rgb>,
    pub quote_bar: Rgb,
    pub quote_text: Rgb,
    pub footnote_text: Rgb,
    pub list_marker: Rgb,
    pub list_guide: Option<Rgb>,
    /// A done task's box, Obsidian's way, or none for GitHub's gray box.
    pub checkbox: Option<Rgb>,
    /// A done task's text, struck through, or none to leave it be.
    pub task_done: Option<Rgb>,
    pub diagram_fill: Rgb,
    pub diagram_border: Rgb,
    pub diagram_line: Rgb,
    pub diagram_label_bg: Rgb,
    pub heading_line_height: [f32; 6],
    /// Letter spacing in em.
    pub heading_tracking: [f32; 6],
    pub code_line_height: f32,
    /// In em of the body text.
    pub footnote_size: f32,
    /// In CSS px: above a heading after other blocks, and after a heading or rule.
    pub heading_space: [f32; 2],
    /// The last heading level with a line under it.
    pub heading_rule: f32,
    pub radius: f32,
    /// Inline code's side padding, in em of its text.
    pub code_padding: f32,
    /// Above and beside, in CSS px.
    pub pre_padding: [f32; 2],
    pub table_padding: [f32; 2],
    pub table_line_height: f32,
    /// GitHub shades every other row; Obsidian doesn't.
    pub table_stripes: bool,
    /// In em.
    pub list_indent: f32,
    /// Between the items of a tight list and of a loose one, in em.
    pub list_gap: [f32; 2],
    /// Space above and below each list item, in em.
    pub item_pad: f32,
    /// Small dots and plain numbers at every depth, as Obsidian draws them, instead of GitHub's markers by depth.
    pub flat_lists: bool,
    /// Obsidian puts a diagram at the left edge; GitHub centers it.
    pub diagram_left: bool,
    /// The bar's width and the space after it, in CSS px.
    pub quote_indent: [f32; 2],
    pub alert_bar: f32,
    /// Above, below and beside.
    pub alert_padding: [f32; 3],
    /// How much of the alert's color fills its box, from 0 to 1.
    pub alert_tint: f32,
    pub rule_width: f32,
    pub rule_margin: f32,
    /// Below the last block, in CSS px.
    pub page_bottom: f32,
    /// A list item's paragraphs get no margin, loose list or not.
    pub tight_items: bool,
    pub summary_bold: bool,
    /// Above the footnotes' rule, its width, and below it, in CSS px.
    pub footnote_rule: [f32; 3],
    /// Below each footnote, in CSS px.
    pub footnote_gap: f32,
    /// A footnote's indent in em of its own size, or none for the list indent.
    pub footnote_indent: Option<f32>,
    /// Footnote numbers have no underline, and the way back is in the list marker color.
    pub plain_footnote_links: bool,
    pub tabular_numbers: bool,
    /// GitHub's disabled checkbox, unticked then ticked: border, fill and tick.
    pub task_box: [[Rgb; 3]; 2],
    /// The code copy button's tooltip.
    pub tooltip_bg: Rgb,
}

impl Theme {
    pub fn new(t: &crate::theme::Theme) -> Theme {
        let c = |name: &str| t.color(name);
        let look = if t.choice("layout") == "obsidian" { Look::Obsidian } else { Look::Github };
        let obs = look == Look::Obsidian;
        let (text, bg, muted) = (c("text"), c("bg"), c("text-muted"));
        // Obsidian's faint gray, guide gray and link-arrow gray, from its text over its page.
        let faint = mix(text, bg, 0.38);
        Theme {
            dark: t.dark,
            look,
            bg: c("bg"),
            fg_muted: c("fg-muted"),
            text: c("text"),
            heading: c("heading"),
            link: c("link"),
            border: c("border"),
            button_bg: c("button-bg"),
            code_bg: c("code-bg"),
            pre_bg: c("pre-bg"),
            alt_bg: c("alt-bg"),
            rule: c("rule"),
            mark_bg: c("mark-bg"),
            hl_bg: c("match-bg"),
            hl_cur_bg: c("match-current-bg"),
            sel_bg: c("text-selection"),
            copied: c("alert-tip"),
            alerts: Alert::ALL.map(|a| c(&format!("alert-{}", a.title().to_lowercase()))),
            diff_add_bg: c("diff-add-bg"),
            diff_del_bg: c("diff-del-bg"),
            pl: SyntaxColors {
                comment: c("pl-comment"),
                keyword: c("pl-keyword"),
                string: c("pl-string"),
                constant: c("pl-constant"),
                entity: c("pl-entity"),
                inserted: c("pl-inserted"),
                deleted: c("pl-deleted"),
                variable: c("pl-variable"),
                tag: c("pl-tag"),
            },
            font_body: t.fonts("font-body").to_vec(),
            font_code: t.fonts("font-code").to_vec(),
            heading_scale: t.heading_scale(),
            heading_weight: t.heading_weight(),
            line_height: t.number("line-height"),
            code_size: t.number("code-size"),
            heading_6: if obs { c("heading") } else { muted },
            link_external: obs.then(|| mix(text, bg, 0.6)),
            quote_bar: if obs { c("accent") } else { c("rule") },
            quote_text: if obs { text } else { muted },
            footnote_text: if obs { text } else { muted },
            list_marker: if obs { faint } else { text },
            list_guide: obs.then(|| mix(text, bg, 0.14)),
            checkbox: obs.then(|| c("accent")),
            task_done: obs.then_some(muted),
            // Obsidian draws diagrams in mermaid's default theme, and its dark mode inverts that theme's colors.
            diagram_fill: match (obs, t.dark) {
                (false, _) => c("pre-bg"),
                (true, false) => 0xececff,
                (true, true) => 0x0f0f27,
            },
            diagram_border: match (obs, t.dark) {
                (false, _) => c("border"),
                (true, false) => 0x9370db,
                (true, true) => 0x986cf2,
            },
            diagram_line: match (obs, t.dark) {
                (false, _) => c("fg-muted"),
                (true, false) => 0x333333,
                (true, true) => 0xcccccc,
            },
            diagram_label_bg: match (obs, t.dark) {
                (false, _) => bg,
                (true, false) => 0xe8e8e8,
                (true, true) => 0x181818,
            },
            heading_line_height: if obs { [1.2, 1.2, 1.3, 1.4, 1.5, 1.5] } else { [1.25; 6] },
            heading_tracking: if obs { [-0.015, -0.011, -0.008, -0.005, -0.002, 0.0] } else { [0.0; 6] },
            code_line_height: t.number("code-line-height"),
            footnote_size: if obs { 0.875 } else { 0.75 },
            heading_space: if obs { [40.0, 16.0] } else { [24.0, 24.0] },
            heading_rule: if obs { 0.0 } else { 2.0 },
            radius: if obs { 4.0 } else { 6.0 },
            code_padding: if obs { 0.3 } else { 0.4 },
            pre_padding: if obs { [12.0, 16.0] } else { [16.0, 16.0] },
            table_padding: if obs { [4.0, 8.0] } else { [6.0, 13.0] },
            table_line_height: if obs { 1.3 } else { 1.5 },
            table_stripes: !obs,
            list_indent: if obs { 1.89 } else { 2.0 },
            list_gap: if obs { [0.0, 0.0] } else { [0.25, 1.0] },
            item_pad: if obs { 0.075 } else { 0.0 },
            flat_lists: obs,
            diagram_left: obs,
            quote_indent: if obs { [2.0, 24.0] } else { [4.0, 16.0] },
            alert_bar: if obs { 0.0 } else { 4.0 },
            alert_padding: if obs { [12.0, 28.0, 24.0] } else { [8.0, 8.0, 16.0] },
            alert_tint: if obs { 0.1 } else { 0.0 },
            rule_width: if obs { 2.0 } else { 4.0 },
            rule_margin: if obs { 32.0 } else { 24.0 },
            // Obsidian's page keeps its last block's bottom margin.
            page_bottom: if obs { 32.0 } else { 16.0 },
            tight_items: obs,
            // Obsidian's summary is regular text after its disclosure triangle.
            summary_bold: !obs,
            // GitHub: a 1 px rule 24 px below the text and 16 px above the notes. Obsidian: 2 px, 32 and 32.
            footnote_rule: if obs { [16.0, 2.0, 32.0] } else { [8.0, 1.0, 16.0] },
            footnote_gap: if obs { 0.0 } else { 4.0 },
            footnote_indent: obs.then_some(1.8),
            plain_footnote_links: obs,
            tabular_numbers: obs,
            task_box: if t.dark { [[0x626262, 0x3b3b3b, 0], [0x757575, 0x757575, 0x3b3b3b]] } else { [[0xd1d1d1, 0xf8f8f8, 0], [0xd1d1d1, 0xd1d1d1, 0xededed]] },
            tooltip_bg: if t.dark { 0x30363d } else { 0x25292e },
        }
    }
}

impl Theme {
    pub fn tok_color(&self, t: TokenKind) -> Rgb {
        let pl = &self.pl;
        let obs = self.look == Look::Obsidian;
        match t {
            TokenKind::Plain => self.text,
            // Obsidian's theme colored GitHub's classes its own way: plain names, types and operators in the
            // text color, variables and tags in their own colors, escapes as the string around them.
            TokenKind::Ident | TokenKind::Import if obs => self.text,
            TokenKind::Var if obs => pl.variable,
            TokenKind::Tag if obs => pl.tag,
            TokenKind::Escape if obs => pl.string,
            TokenKind::Keyword | TokenKind::Tag => pl.keyword,
            TokenKind::Str | TokenKind::Ident => pl.string,
            TokenKind::Escape | TokenKind::Var | TokenKind::Import | TokenKind::Number | TokenKind::Meta => pl.constant,
            TokenKind::Comment => pl.comment,
            TokenKind::Func => pl.entity,
            TokenKind::Added => pl.inserted,
            TokenKind::Removed => pl.deleted,
        }
    }
}

/// `fg` laid over `bg` at `alpha`, as CSS's `color-mix` with transparent draws it.
pub fn mix(fg: Rgb, bg: Rgb, alpha: f32) -> Rgb {
    let ch = |c: Rgb, i: u32| ((c >> i) & 255) as f32;
    [16, 8, 0].into_iter().fold(0, |out, i| out | ((ch(fg, i) * alpha + ch(bg, i) * (1.0 - alpha)).round() as Rgb) << i)
}

pub fn rgb(c: Rgb) -> (f64, f64, f64) {
    (((c >> 16) & 255) as f64 / 255.0, ((c >> 8) & 255) as f64 / 255.0, (c & 255) as f64 / 255.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_builtin_builds_a_render_theme() {
        for name in crate::theme::names().split(", ") {
            super::Theme::new(crate::theme::builtin(name).unwrap());
        }
    }
}
