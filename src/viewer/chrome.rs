use super::app::{self, App};
use crate::term::{self, Key};
use crate::theme::{Palette, bg, fg};

const NOTICE_ROOM: usize = 30;

pub struct Chrome<'a> {
    pub app: &'a App,
    pub name: &'a str,
    pub cols: u16,
    pub rows: u16,
    pub img_col: u16,
    pub palette: Palette,
}

impl Chrome<'_> {
    pub fn draw(&self, out: &mut String) {
        let rows = self.rows;
        let (line, lw, _) = self.bottom();
        let p = &self.palette;
        out.push_str(&format!("\x1b[{rows};1H\x1b[0m{}{}{line}{}", bg(p.bar_bg), fg(p.fg_muted), p.reset()));
        if self.img_col > 0 && !self.app.help {
            self.sidebar(out);
        }
        if self.app.prompt.is_some() {
            let col = lw;
            out.push_str(&format!("\x1b[{rows};{col}H\x1b[?25h"));
        } else {
            out.push_str("\x1b[?25l");
        }
    }

    pub fn hint_at(&self, col: u16) -> Option<Key> {
        let x = usize::from(col.saturating_sub(1));
        self.bottom().2.into_iter().find(|&(a, b, _)| x >= a && x < b).and_then(|(.., k)| App::hint_key(k))
    }

    fn bottom(&self) -> (String, usize, Vec<(usize, usize, &'static str)>) {
        let cols = usize::from(self.cols);
        let left = term::printable(&self.app.status(self.name));
        let base = term::width(&term::printable(&self.app.status_base(self.name))).max(NOTICE_ROOM.min(cols / 3));
        let mut right = format!(" {:>3}% ", self.app.percent());
        let mut hints = self.app.hints();
        let hints_width = |h: &[(&str, &str)]| h.iter().map(|(k, l)| term::width(k) + term::width(l) + 3).sum::<usize>();
        while hints.len() > 1 && base + 2 + hints_width(&hints) + right.len() > cols {
            hints.pop();
        }
        if hints_width(&hints) + right.len() > cols {
            right.clear();
        }
        if hints_width(&hints) > cols {
            hints.clear();
        }
        if self.app.notice.is_some() && term::width(&left) + 1 > cols.saturating_sub(hints_width(&hints) + right.len()) {
            hints.clear();
        }
        let hw = hints_width(&hints);
        let room = cols.saturating_sub(hw + right.len());
        let (mut line, lw) = term::fit(&left, if hw > 0 { room.saturating_sub(1) } else { room });
        let fill = cols.saturating_sub(lw + hw + right.len());
        line.push_str(&" ".repeat(fill));
        let mut spans = Vec::new();
        let mut at = lw + fill;
        for (k, l) in hints {
            let w = term::width(k) + term::width(l) + 3;
            spans.push((at, at + w - 2, k));
            at += w;
            line.push_str(&format!("{}{k}{} {l}  ", fg(self.palette.fg_bright), fg(self.palette.fg_muted)));
        }
        line.push_str(&right);
        (line, lw, spans)
    }

    fn sidebar(&self, out: &mut String) {
        let w = usize::from(self.img_col);
        let thumb = self.app.toc_thumb().map(|(top, len)| {
            let top = (app::TOC_HEADER + top) as usize;
            top..top + len as usize
        });
        // An overflowing list gives up two more columns to a track drawn ▕│▏, so its lines stay as thin as the rule.
        let content = w.saturating_sub(if thumb.is_some() { usize::from(app::TOC_BAR) } else { 1 });
        let current = self.app.current_entry();
        let rows = view_rows(self.rows);
        let title = if content > 19 { " TABLE OF CONTENTS" } else { " CONTENTS" };
        let mut lines: Vec<(String, bool)> = vec![(String::new(), false), (title.into(), false), ("─".repeat(content), false)];
        for (i, e) in self.app.entries.iter().enumerate().skip(self.app.toc_offset as usize) {
            let title = term::printable(&e.title);
            let label = if e.root { format!(" {title}") } else { format!("   {title}") };
            let max = content.saturating_sub(1);
            let label = if term::width(&label) > max { term::fit(&label, max.saturating_sub(1)).0 + "…" } else { label };
            lines.push((label, current == Some(i)));
        }
        for r in 0..rows {
            let (text, active) = lines.get(r).cloned().unwrap_or_default();
            let pad = content.saturating_sub(term::width(&text));
            let p = &self.palette;
            let style = if active {
                format!("{}{}", bg(p.selection_bg), fg(p.fg_bright))
            } else if r == 1 {
                format!("\x1b[1m{}", fg(p.fg_bright))
            } else if r == 2 {
                fg(p.border)
            } else {
                fg(p.fg_muted)
            };
            let reset = p.reset();
            let line = fg(p.border);
            let edge = match &thumb {
                None if r == 2 => format!("{line}┤"),
                None => format!("{line}│"),
                Some(_) if r == 2 => format!("{line}──▏"),
                Some(_) if r < 2 => format!("{line}  ▏"),
                Some(t) if t.contains(&r) => format!("{line}▕{}│{line}▏", fg(p.fg_muted)),
                Some(_) => format!("{line}▕ ▏"),
            };
            out.push_str(&format!("\x1b[{};1H{reset}{style}{text}{}{reset}{edge}{reset}", r + 1, " ".repeat(pad)));
        }
    }
}

fn view_rows(rows: u16) -> usize {
    usize::from(rows.saturating_sub(1).max(1))
}

pub fn help(out: &mut String, cols: u16, rows: u16, top: &mut u32, p: &Palette) {
    let cols = usize::from(cols);
    let rows = view_rows(rows);
    let lines = help_lines(cols);
    let max = lines.len().saturating_sub(rows) as u32;
    *top = (*top).min(max);
    let top = *top as usize;
    let reset = p.reset();
    out.push_str(&format!("{reset}\x1b[2J"));
    for r in 0..rows {
        let i = top + r;
        let Some(l) = lines.get(i) else { break };
        let more = r + 1 == rows && i + 1 < lines.len();
        let text = if !more {
            l.clone()
        } else if cols >= 36 {
            format!("↓ {} more lines: j k or Space", lines.len() - i)
        } else {
            "↓ more: j k".into()
        };
        let heading = i == 0 || (i > 0 && lines[i - 1].is_empty() && !l.starts_with(' '));
        let style = if heading || more { format!("\x1b[1m{}", fg(p.fg_bright)) } else { fg(p.fg_muted) };
        let (text, _) = term::fit(&term::printable(&text), cols.saturating_sub(2));
        out.push_str(&format!("\x1b[{};2H{style}{text}{reset}", r + 1));
    }
}

fn help_lines(cols: usize) -> Vec<String> {
    let full = app::KEYS.lines().map(|l| term::width(l.trim_end())).max().unwrap_or(0) + 2;
    let mut lines = vec!["mddog keys. Press ? or Esc to go back to the page".to_string(), String::new()];
    for l in app::KEYS.lines().map(str::trim_end) {
        match l.trim_start().split_once("  ").filter(|_| full > cols && l.starts_with("  ")) {
            Some((k, d)) => {
                lines.push(format!("  {k}"));
                let mut line = String::from("     ");
                for w in d.split_whitespace() {
                    if term::width(&line) + 1 + term::width(w) > cols.saturating_sub(2) && !line.trim().is_empty() {
                        lines.push(std::mem::replace(&mut line, String::from("     ")));
                    }
                    line.push(' ');
                    line.push_str(w);
                }
                lines.push(line);
            }
            None => lines.push(l.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{self, Rgb};

    fn dark() -> Palette {
        theme::builtin("github-dark").unwrap().palette
    }

    fn colors(s: &str) -> Vec<Rgb> {
        crate::viewer::loading::tests::sgr_colors(s)
    }

    #[test]
    fn a_light_theme_draws_light_chrome() {
        let light = theme::builtin("github-light").unwrap().palette;
        let entries: Vec<app::Entry> = (0..4).map(|i| app::Entry { root: true, title: format!("h{i}"), row: i }).collect();
        let mut app = App::default();
        (app.entries, app.toc_visible, app.toc_list_height) = (entries, true, 4);
        let mut out = String::new();
        Chrome { app: &app, name: "a.md", cols: 80, rows: 8, img_col: 26, palette: light }.draw(&mut out);
        help(&mut out, 80, 8, &mut 0, &light);
        let grounds = [light.bg, light.bar_bg, light.selection_bg];
        let inks = [light.fg_muted, light.fg_bright, light.border];
        let drawn = colors(&out);
        assert!(drawn.contains(&light.bar_bg) && drawn.contains(&light.selection_bg) && drawn.contains(&light.border));
        assert!(drawn.iter().all(|c| grounds.contains(c) || inks.contains(c)), "{drawn:?}");
        assert!(out.contains(&format!("\x1b[0m{}", theme::bg(light.bg))));
    }

    fn keys(c: &Chrome) -> Vec<Key> {
        let mut keys: Vec<Key> = (1..=c.cols).filter_map(|col| c.hint_at(col)).collect();
        keys.dedup();
        keys
    }

    #[test]
    fn hints_drop_from_the_right_but_the_first_stays() {
        let app = App::default();
        let at = |cols| Chrome { app: &app, name: "a.md", cols, rows: 24, img_col: 0, palette: dark() };
        assert_eq!(keys(&at(80)), vec![Key::Char('?'), Key::Char('/'), Key::Char('e')]);
        assert_eq!(keys(&at(40)), vec![Key::Char('?'), Key::Char('/')]);
        assert_eq!(keys(&at(20)), vec![Key::Char('?')]);
    }

    #[test]
    fn a_long_notice_hides_the_hints() {
        let mut app = App::default();
        app.notice = Some("x".repeat(60));
        assert!(keys(&Chrome { app: &app, name: "a.md", cols: 80, rows: 24, img_col: 0, palette: dark() }).is_empty());
    }

    #[test]
    fn the_sidebar_has_a_gap_a_title_and_a_full_rule_then_plain_entries() {
        let entries = vec![app::Entry { root: true, title: "Intro".into(), row: 0 }, app::Entry { root: false, title: "Setup".into(), row: 5 }];
        let mut app = App::default();
        (app.entries, app.toc_visible, app.toc_list_height) = (entries, true, 4);
        let mut out = String::new();
        Chrome { app: &app, name: "a.md", cols: 80, rows: 10, img_col: 26, palette: dark() }.draw(&mut out);
        assert!(out.contains("\x1b[2;1H") && out.split("\x1b[2;1H").nth(1).unwrap().contains(" TABLE OF CONTENTS"));
        assert!(out.contains(" Intro") && out.contains("   Setup"));
        assert!(!out.contains("01") && !out.contains('•') && !out.contains('▎') && !out.contains("more"));
        assert!(out.contains(&format!("\x1b[3;1H{}{}{}", dark().reset(), fg(dark().border), "─".repeat(25))));
    }

    /// The text drawn on screen line `r` of the sidebar, without its escape codes.
    fn sidebar_line(out: &str, r: usize) -> String {
        let mut chars = out.split(&format!("\x1b[{r};1H")).nth(1).unwrap().chars();
        let mut text = String::new();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                text.push(c);
            } else if chars.by_ref().find(|c| c.is_ascii_alphabetic()) == Some('H') {
                break;
            }
        }
        text
    }

    fn sidebar(entries: Vec<app::Entry>, offset: u32) -> String {
        let mut app = App::default();
        (app.entries, app.toc_visible, app.toc_list_height, app.toc_offset) = (entries, true, 6, offset);
        let mut out = String::new();
        Chrome { app: &app, name: "a.md", cols: 80, rows: 10, img_col: 26, palette: dark() }.draw(&mut out);
        out
    }

    /// The last three columns of screen lines 1 to 9, one line each.
    fn edges(entries: usize, offset: u32) -> Vec<String> {
        let out = sidebar((0..entries).map(|i| app::Entry { root: true, title: format!("h{i}"), row: i as u32 }).collect(), offset);
        (1..=9)
            .map(|r| {
                let line: Vec<char> = sidebar_line(&out, r).chars().collect();
                line[line.len() - 3..].iter().collect()
            })
            .collect()
    }

    #[test]
    fn a_fitting_list_gets_one_line_and_full_width_entries() {
        assert_eq!(edges(6, 0), ["  │", "  │", "──┤", "  │", "  │", "  │", "  │", "  │", "  │"]);
        let out = sidebar((0..6).map(|i| app::Entry { root: true, title: "x".repeat(40 + i), row: 0 }).collect(), 0);
        let line = sidebar_line(&out, 4);
        assert_eq!(term::width(&line), 26, "{line:?}");
        assert!(line.ends_with("… │") && line.chars().filter(|&c| c == 'x').count() == 22, "{line:?}");
    }

    #[test]
    fn a_long_list_gets_a_thin_track_whose_thumb_follows_the_offset() {
        let track = |thumb: &[usize]| {
            let mut want = vec!["  ▏".to_string(), "  ▏".into(), "──▏".into()];
            want.extend((3..9).map(|r| if thumb.contains(&r) { "▕│▏" } else { "▕ ▏" }.to_string()));
            want
        };
        assert_eq!(edges(12, 0), track(&[3, 4, 5]));
        assert_eq!(edges(12, 2), track(&[4, 5, 6]));
        assert_eq!(edges(12, 6), track(&[6, 7, 8]));
        let out = sidebar((0..12).map(|i| app::Entry { root: true, title: "x".repeat(40 + i), row: 0 }).collect(), 0);
        let line = sidebar_line(&out, 4);
        assert_eq!(term::width(&line), 26, "{line:?}");
        assert!(line.ends_with("… ▕│▏") && line.chars().filter(|&c| c == 'x').count() == 20, "{line:?}");
        let p = dark();
        assert!(out.contains(&format!("{}▕{}│{}▏", fg(p.border), fg(p.fg_muted), fg(p.border))), "{out:?}");
        assert!(out.contains(&format!("{}▕ ▏", fg(p.border))), "{out:?}");
    }

    #[test]
    fn help_wraps_when_narrow_and_clamps_its_top() {
        assert!(help_lines(30).len() > help_lines(200).len());
        let mut top = 10_000;
        help(&mut String::new(), 80, 10, &mut top, &dark());
        assert_eq!(top as usize, help_lines(80).len() - 9);
    }
}
