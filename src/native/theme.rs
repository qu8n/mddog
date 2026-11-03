//! The theme's tokens as the native renderer draws with them.

pub type Rgb = u32;

/// Syntax colors, one per highlight class.
#[derive(Clone, Debug)]
pub struct Pl {
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
    /// Obsidian's reading view, from `--mddog-layout`, rather than GitHub's README.
    pub obsidian: bool,
    pub bg: Rgb,
    pub fg_muted: Rgb,
    pub text: Rgb,
    pub text_muted: Rgb,
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
    /// Note, tip, important, warning and caution.
    pub alerts: [Rgb; 5],
    pub diff_add_bg: Rgb,
    pub diff_del_bg: Rgb,
    pub pl: Pl,
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
    /// Obsidian puts a diagram at the left edge; pele centered it.
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
}

impl Theme {
    pub fn new(t: &crate::theme::Theme) -> Theme {
        let c = |name: &str| {
            let (r, g, b) = t.color(name);
            u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
        };
        let obs = t.choice("layout") == "obsidian";
        let (text, bg, muted) = (c("text"), c("bg"), c("text-muted"));
        // Obsidian's faint gray, guide gray and link-arrow gray, from its text over its page.
        let faint = mix(text, bg, 0.38);
        Theme {
            dark: t.dark,
            obsidian: obs,
            bg: c("bg"),
            fg_muted: c("fg-muted"),
            text: c("text"),
            text_muted: c("text-muted"),
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
            alerts: ["alert-note", "alert-tip", "alert-important", "alert-warning", "alert-caution"].map(c),
            diff_add_bg: c("diff-add-bg"),
            diff_del_bg: c("diff-del-bg"),
            pl: Pl {
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
    fn every_builtin_has_its_tokens() {
        for name in crate::theme::names().split(", ") {
            let t = super::Theme::new(crate::theme::builtin(name).unwrap());
            assert_eq!(t.dark, name.ends_with("dark"), "{name}");
        }
    }
}
