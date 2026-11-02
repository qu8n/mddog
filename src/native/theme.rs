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
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub mermaid_obsidian: bool,
    pub bg: Rgb,
    pub fg_muted: Rgb,
    pub text: Rgb,
    pub text_muted: Rgb,
    pub heading: Rgb,
    pub link: Rgb,
    pub accent: Rgb,
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
    pub line_height: f32,
    pub code_size: f32,
}

impl Theme {
    pub fn new(t: &crate::theme::Theme) -> Theme {
        let c = |name: &str| {
            let (r, g, b) = t.color(name);
            u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
        };
        Theme {
            dark: t.dark,
            mermaid_obsidian: crate::config::mermaid(t) == crate::config::Mermaid::Obsidian,
            bg: c("bg"),
            fg_muted: c("fg-muted"),
            text: c("text"),
            text_muted: c("text-muted"),
            heading: c("heading"),
            link: c("link"),
            accent: c("accent"),
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
            },
            font_body: t.fonts("font-body").to_vec(),
            font_code: t.fonts("font-code").to_vec(),
            heading_scale: t.heading_scale(),
            line_height: t.number("line-height"),
            code_size: t.number("code-size"),
        }
    }
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
            assert_eq!(t.mermaid_obsidian, name.starts_with("obsidian"), "{name}");
        }
    }
}
