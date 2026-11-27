use super::app::{self, App, Hint};
use super::keys;
use super::themes;
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
        self.bottom().2.into_iter().find(|&(a, b, _)| x >= a && x < b).map(|(.., k)| k)
    }

    fn bottom(&self) -> (String, usize, Vec<(usize, usize, Key)>) {
        let cols = usize::from(self.cols);
        let left = term::printable(&self.app.status(self.name));
        let base = term::width(&term::printable(&self.app.status_base(self.name))).max(NOTICE_ROOM.min(cols / 3));
        let mut right = format!(" {:>3}% ", self.app.percent());
        let mut hints = self.app.hints();
        let hints_width = |h: &[Hint]| h.iter().map(|(k, l, _)| term::width(k) + term::width(l) + 3).sum::<usize>();
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
        for (k, l, key) in hints {
            let w = term::width(k) + term::width(l) + 3;
            spans.push((at, at + w - 2, key));
            at += w;
            line.push_str(&format!("{}{k}{} {l}  ", fg(self.palette.fg_bright), fg(self.palette.fg_muted)));
        }
        line.push_str(&right);
        (line, lw, spans)
    }

    fn sidebar(&self, out: &mut String) {
        if let Some(list) = &self.app.themes {
            self.theme_list(list, out);
            return;
        }
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
        // Only the entries that fit: a long list costs as much to draw as a short one.
        let fit = rows.saturating_sub(lines.len());
        for (i, e) in self.app.entries.iter().enumerate().skip(self.app.toc_offset as usize).take(fit) {
            let title = term::printable(&e.title);
            let label = format!("{}{title}", " ".repeat(1 + 2 * usize::from(e.depth)));
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

    fn theme_list(&self, list: &themes::List, out: &mut String) {
        let content = usize::from(self.img_col).saturating_sub(1);
        let p = &self.palette;
        let reset = p.reset();
        for r in 0..view_rows(self.rows) {
            let i = r.checked_sub(app::TOC_HEADER as usize).map(|n| n + list.offset).filter(|&i| i < list.labels.len());
            let (text, style) = match (r, i) {
                (1, _) => (" THEMES".to_string(), format!("\x1b[1m{}", fg(p.fg_bright))),
                (2, _) => ("─".repeat(content), fg(p.border)),
                (_, Some(i)) => {
                    let mark = if i == list.current { "•" } else { " " };
                    let label = format!(" {mark} {}", term::printable(&list.labels[i]));
                    let max = content.saturating_sub(1);
                    let label = if term::width(&label) > max { term::fit(&label, max.saturating_sub(1)).0 + "…" } else { label };
                    let style = match (i == list.sel, i == list.shown) {
                        (true, shown) => format!("{}{}{}", if shown { "\x1b[1m" } else { "" }, bg(p.selection_bg), fg(p.fg_bright)),
                        (false, true) => format!("\x1b[1m{}", fg(p.fg_bright)),
                        (false, false) => fg(p.fg_muted),
                    };
                    (label, style)
                }
                _ => (String::new(), String::new()),
            };
            let pad = content.saturating_sub(term::width(&text));
            let edge = if r == 2 { "┤" } else { "│" };
            out.push_str(&format!("\x1b[{};1H{reset}{style}{text}{}{reset}{}{edge}{reset}", r + 1, " ".repeat(pad), fg(p.border)));
        }
    }
}

/// The rows above the status row, which the page or a screen over it fills.
pub fn view_rows(rows: u16) -> usize {
    usize::from(rows.saturating_sub(1).max(1))
}

#[derive(Clone, Copy, Debug)]
enum Ink {
    Title,
    Head,
    Rule,
    Key,
    Text,
    Muted,
}

type Line = Vec<(usize, String, Ink)>;

const HELP_GAP: usize = 4;

pub fn help(out: &mut String, cols: u16, rows: u16, top: &mut u32, p: &Palette) {
    let cols = usize::from(cols);
    let rows = view_rows(rows);
    let lines = help_lines(cols);
    let max = lines.len().saturating_sub(rows) as u32;
    *top = (*top).min(max);
    let top = *top as usize;
    let reset = p.reset();
    out.push_str(&format!("{reset}{}", crate::term::CLEAR));
    for r in 0..rows {
        let i = top + r;
        let Some(line) = lines.get(i) else { break };
        let more = r + 1 == rows && i + 1 < lines.len();
        let left = lines.len() - i;
        let more_line = [(lines[1].first().map_or(1, |s| s.0), if cols >= 36 { format!("↓ {left} more lines: j k or Space") } else { "↓ more: j k".into() }, Ink::Title)];
        for (x, text, ink) in if more { &more_line[..] } else { &line[..] } {
            let style = match ink {
                Ink::Title => format!("\x1b[1m{}", fg(p.fg_bright)),
                Ink::Head => format!("\x1b[1m{}", fg(p.fg_bright)),
                Ink::Rule => fg(p.border),
                Ink::Key => fg(p.link),
                Ink::Text => fg(p.fg),
                Ink::Muted => fg(p.fg_muted),
            };
            let (text, _) = term::fit(&term::printable(text), cols.saturating_sub(*x));
            out.push_str(&format!("\x1b[{};{}H{style}{text}{reset}", r + 1, x + 1));
        }
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut w = word.to_string();
        while term::width(&w) > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            let (head, _) = term::fit(&w, width);
            let n = head.chars().count().max(1);
            lines.push(head);
            w = w.chars().skip(n).collect();
        }
        if w.is_empty() {
            continue;
        }
        if !line.is_empty() && term::width(&line) + 1 + term::width(&w) > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&w);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

struct Columns {
    key: usize,
    action: usize,
}

type Cell = Vec<Line>;

impl Columns {
    fn fit(width: usize) -> Columns {
        let inner = width.saturating_sub(3);
        let widest = keys::SECTIONS.iter().flat_map(|s| s.rows).map(|r| term::width(&r.keys.join(" or "))).max().unwrap_or(1);
        let key = widest.min(inner / 3).max(1);
        Columns { key, action: inner.saturating_sub(key).max(1) }
    }

    fn row(&self, key: Cell, action: Cell) -> Vec<Line> {
        let height = key.len().max(action.len());
        (0..height)
            .map(|i| {
                let mut line = Line::new();
                for (x, cell) in [(1, &key), (self.key + 3, &action)] {
                    line.extend(cell.get(i).into_iter().flatten().filter(|(_, t, _)| !t.is_empty()).map(|(at, t, ink)| (x + at, t.clone(), *ink)));
                }
                line
            })
            .collect()
    }
}

fn cell(lines: Vec<String>, ink: Ink) -> Cell {
    lines.into_iter().map(|l| vec![(0, l, ink)]).collect()
}

fn key_cell(keys: &[&str], width: usize) -> Cell {
    let mut lines: Cell = vec![vec![]];
    let mut at = 0;
    for (i, k) in keys.iter().enumerate() {
        let or = if i > 0 { 3 } else { 0 };
        let w = term::width(k);
        if at > 0 && at + 1 + or + w > width {
            lines.push(vec![]);
            at = 0;
        }
        let x = if at > 0 { at + 1 } else { 0 };
        if or + w > width {
            if or > 0 {
                lines.last_mut().into_iter().for_each(|l| l.push((x, "or".into(), Ink::Muted)));
                lines.push(vec![]);
            }
            let parts = wrap(k, width);
            let n = parts.len();
            for (j, part) in parts.into_iter().enumerate() {
                at = term::width(&part);
                if let Some(l) = lines.last_mut() {
                    l.push((0, part, Ink::Key));
                }
                if j + 1 < n {
                    lines.push(vec![]);
                }
            }
            continue;
        }
        let line = lines.last_mut().into_iter();
        for l in line {
            if or > 0 {
                l.push((x, "or".into(), Ink::Muted));
            }
            l.push((x + or, k.to_string(), Ink::Key));
        }
        at = x + or + w;
    }
    lines
}

fn help_block(sec: &keys::Section, width: usize, cols: &Columns) -> Vec<Line> {
    let name = term::width(sec.name);
    let mut lines: Vec<Line> = vec![vec![(0, sec.name.to_string(), Ink::Head), (name + 1, "─".repeat(width.saturating_sub(name + 1)), Ink::Rule)], vec![]];
    for row in sec.rows {
        lines.extend(cols.row(key_cell(row.keys, cols.key), cell(wrap(row.what, cols.action), Ink::Text)));
    }
    lines
}

fn shift(line: Line, by: usize) -> Line {
    line.into_iter().map(|(x, t, i)| (x + by, t, i)).collect()
}

fn help_lines(cols: usize) -> Vec<Line> {
    let margin = if cols >= 60 { 3 } else { 1 };
    let usable = cols.saturating_sub(2 * margin);
    let two = cols >= 120;
    let width = if two { ((usable - HELP_GAP) / 2).min(80) } else { usable.min(80) };
    let x0 = (cols.saturating_sub(if two { 2 * width + HELP_GAP } else { width })) / 2;
    let columns = Columns::fit(width);
    let blocks: Vec<Vec<Line>> = keys::SECTIONS.iter().map(|s| help_block(s, width, &columns)).collect();
    let mut lines: Vec<Line> = vec![vec![], vec![(x0, "Keys".into(), Ink::Title)]];
    lines.extend(wrap("Press ? or Esc to go back to the page", cols.saturating_sub(x0 + 1)).into_iter().map(|l| vec![(x0, l, Ink::Muted)]));
    lines.push(vec![]);
    let stack = |bs: &[Vec<Line>]| -> Vec<Line> {
        let mut v = Vec::new();
        for (i, b) in bs.iter().enumerate() {
            if i > 0 {
                v.push(vec![]);
            }
            v.extend(b.iter().cloned());
        }
        v
    };
    if two {
        let split = (1..blocks.len()).min_by_key(|&k| stack(&blocks[..k]).len().max(stack(&blocks[k..]).len())).unwrap_or(1);
        let (left, right) = (stack(&blocks[..split]), stack(&blocks[split..]));
        for i in 0..left.len().max(right.len()) {
            let mut line = shift(left.get(i).cloned().unwrap_or_default(), x0);
            line.extend(shift(right.get(i).cloned().unwrap_or_default(), x0 + width + HELP_GAP));
            lines.push(line);
        }
    } else {
        lines.extend(stack(&blocks).into_iter().map(|l| shift(l, x0)));
    }
    lines.push(vec![]);
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
        crate::viewer::sgr_colors(s)
    }

    #[test]
    fn a_light_theme_draws_light_chrome() {
        let light = theme::builtin("github-light").unwrap().palette;
        let entries: Vec<app::Entry> = (0..4).map(|i| app::Entry { depth: 0, root: true, title: format!("h{i}"), row: i }).collect();
        let mut app = App::default();
        (app.entries, app.toc_visible, app.toc_list_height) = (entries, true, 4);
        let mut out = String::new();
        Chrome { app: &app, name: "a.md", cols: 80, rows: 8, img_col: 26, palette: light }.draw(&mut out);
        help(&mut out, 80, 8, &mut 0, &light);
        let grounds = [light.bg, light.bar_bg, light.selection_bg];
        let inks = [light.fg_muted, light.fg_bright, light.border, light.fg, light.link];
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
        app.notice = Some(app::Notice { text: "x".repeat(60), click: app::OnClick::Hints });
        assert!(keys(&Chrome { app: &app, name: "a.md", cols: 80, rows: 24, img_col: 0, palette: dark() }).is_empty());
    }

    #[test]
    fn the_sidebar_has_a_gap_a_title_and_a_full_rule_then_plain_entries() {
        let entries = vec![app::Entry { depth: 0, root: true, title: "Intro".into(), row: 0 }, app::Entry { depth: 1, root: false, title: "Setup".into(), row: 5 }];
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
            } else if chars.by_ref().find(char::is_ascii_alphabetic) == Some('H') {
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
        let out = sidebar((0..entries).map(|i| app::Entry { depth: 0, root: true, title: format!("h{i}"), row: i as u32 }).collect(), offset);
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
        let out = sidebar((0..6).map(|i| app::Entry { depth: 0, root: true, title: "x".repeat(40 + i), row: 0 }).collect(), 0);
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
        let out = sidebar((0..12).map(|i| app::Entry { depth: 0, root: true, title: "x".repeat(40 + i), row: 0 }).collect(), 0);
        let line = sidebar_line(&out, 4);
        assert_eq!(term::width(&line), 26, "{line:?}");
        assert!(line.ends_with("… ▕│▏") && line.chars().filter(|&c| c == 'x').count() == 20, "{line:?}");
        let p = dark();
        assert!(out.contains(&format!("{}▕{}│{}▏", fg(p.border), fg(p.fg_muted), fg(p.border))), "{out:?}");
        assert!(out.contains(&format!("{}▕ ▏", fg(p.border))), "{out:?}");
    }

    #[test]
    fn help_fits_any_width_and_clamps_its_top() {
        assert!(help_lines(40).len() > help_lines(120).len());
        let mut top = 10_000;
        help(&mut String::new(), 80, 10, &mut top, &dark());
        assert_eq!(top as usize, help_lines(80).len() - 9);
        for cols in [24, 30, 46, 80, 96, 140, 200] {
            for line in help_lines(cols) {
                let mut end = 0;
                for (x, text, _) in &line {
                    assert!(*x >= end, "{cols} columns: {line:?} overlaps");
                    end = x + term::width(text);
                }
                assert!(end <= cols, "{cols} columns: {line:?} is too wide");
            }
        }
    }

    #[test]
    fn help_colors_the_keys_and_marks_their_alternates() {
        let p = dark();
        let mut out = String::new();
        help(&mut out, 120, 200, &mut 0, &p);
        assert!(out.contains(&format!("{}Ctrl-p", fg(p.link))) && !out.contains(&format!("\x1b[1m{}Ctrl-p", fg(p.link))));
        assert!(out.contains(&format!("{}or{}", fg(p.fg_muted), p.reset())) && out.contains(&format!("{}Ctrl-e", fg(p.link))));
        assert!(out.contains(&format!("{}──", fg(p.border))) && !out.contains("┌") && !out.contains("│"));
        assert!(out.contains(&format!("\x1b[1m{}Scroll", fg(p.fg_bright))));
        assert!(!out.contains(&bg(p.selection_bg)));
        assert!(!out.contains("more lines"));
    }
}
