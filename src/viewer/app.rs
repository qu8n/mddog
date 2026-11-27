use super::labels::{self, Labels};
use crate::term::Key;

/// How many places Backspace goes back through: jumps in a file, and files.
pub const HISTORY: usize = 50;
pub const WHEEL: u32 = 3;
pub const CODE_WHEEL: i32 = 6;
pub const CODE_KEY: i32 = 8;
pub const MATCH_CONTEXT: u32 = 3;
pub const PADDING_ROWS: u32 = 5;
pub const TOC_CONTENT: u16 = 30;
pub const TOC_MIN_COLS: u16 = 40;
/// Rows above the first contents entry: a gap, the title, and the rule.
pub const TOC_HEADER: u32 = 3;
/// Columns at the sidebar's right edge that hold the contents scroll bar when the list overflows.
pub const TOC_BAR: u16 = 3;

pub const RESIZE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);
/// A jump key this soon after the last one glides in `FAST_GLIDE`, so a held key keeps up.
pub const REPEAT: std::time::Duration = std::time::Duration::from_millis(100);
pub const FAST_GLIDE: std::time::Duration = std::time::Duration::from_millis(50);
/// How long a glide over one screen takes. A shorter glide takes its share.
pub const GLIDE_PER_PAGE: std::time::Duration = std::time::Duration::from_millis(200);

#[derive(Default)]
pub struct Debounce {
    deadline: Option<std::time::Instant>,
}

impl Debounce {
    pub fn poke(&mut self, now: std::time::Instant) {
        self.deadline = Some(now + RESIZE_DEBOUNCE);
    }

    pub fn pending(&self) -> bool {
        self.deadline.is_some()
    }

    pub fn due(&mut self, now: std::time::Instant) -> bool {
        match self.deadline {
            Some(d) if now >= d => {
                self.deadline = None;
                true
            }
            _ => false,
        }
    }
}

/// How long a glide lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// `per_page` for each `page` pixels it covers.
    Page {
        per_page: std::time::Duration,
        page: u32,
    },
    Fixed(std::time::Duration),
}

impl Pace {
    /// A jump key within `REPEAT` of the last one glides fast, others at `per_page` for each `page` pixels.
    pub fn for_jump(last: Option<std::time::Instant>, now: std::time::Instant, per_page: std::time::Duration, page: u32) -> Pace {
        if last.is_some_and(|t| now.saturating_duration_since(t) < REPEAT) { Pace::Fixed(FAST_GLIDE) } else { Pace::Page { per_page, page } }
    }
}

/// A pixel scroll from one position to another, easing out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glide {
    from: u32,
    to: u32,
    start: std::time::Instant,
    time: std::time::Duration,
}

impl Glide {
    /// Starts at `from`, but no more than `span` pixels away from `to`, so a long jump glides only its last stretch.
    pub fn new(from: u32, to: u32, span: u32, pace: Pace, now: std::time::Instant) -> Glide {
        let from = from.clamp(to.saturating_sub(span), to.saturating_add(span));
        let time = match pace {
            Pace::Page { per_page, page } => per_page.mul_f64(f64::from(from.abs_diff(to)) / f64::from(page.max(1))),
            Pace::Fixed(t) => t,
        };
        Glide { from, to, start: now, time }
    }

    pub fn restart(&mut self, now: std::time::Instant) {
        self.start = now;
    }

    pub fn from(&self) -> u32 {
        self.from
    }

    pub fn to(&self) -> u32 {
        self.to
    }

    /// The position at `now`, and whether the glide is over.
    pub fn at(&self, now: std::time::Instant) -> (u32, bool) {
        let t = if self.time.is_zero() { 1.0 } else { (now.saturating_duration_since(self.start).as_secs_f64() / self.time.as_secs_f64()).min(1.0) };
        let eased = 1.0 - (1.0 - t).powi(3);
        let px = f64::from(self.from) + (f64::from(self.to) - f64::from(self.from)) * eased;
        (px.round() as u32, t >= 1.0)
    }
}

/// A message on the status row, and what a click on the status row does while it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub click: OnClick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnClick {
    /// Only what the hints under the pointer do.
    Hints,
    /// Goes back, as Backspace does.
    GoBack,
    /// A second click on `q` quits.
    Quit,
}

/// A bottom-row hint: its label, what it does, and the key a click sends.
pub type Hint = (&'static str, &'static str, Key);

/// The quit hint, which a click runs only when it comes twice.
pub const QUIT: Hint = ("q", "quit", Key::Char('q'));

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub row: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// How many headings above it, still open, have a smaller level.
    pub depth: u8,
    /// Whether digits, `J`, and `K` jump to it.
    pub root: bool,
    pub title: String,
    pub row: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Scroll,
    /// A scroll that may glide: a jump of several rows from a key.
    Glide,
    Status,
    Search(String),
    ClearSearch,
    Current(usize),
    ToggleToc,
    ToggleWrap,
    ScrollCode(u16, u16, i32),
    ScrollView(i32),
    Notice(String),
    Help,
    Collapse,
    Editor,
    Copy,
    CopySelection,
    Suspend,
    Browse,
    BackFile,
    OpenThemes,
    PreviewTheme,
    PickTheme,
    CloseThemes,
    DropThemes,
    Quit,
    StartLabels,
    Labels,
    EndLabels,
    Follow(usize),
}

#[derive(Default)]
pub struct App {
    pub labels: Option<Labels>,
    pub scroll: u32,
    pub doc_rows: u32,
    pub viewport: u32,
    pub prompt: Option<String>,
    pub query: String,
    pub matches: Vec<u32>,
    pub current: usize,
    pub toc_visible: bool,
    pub toc_offset: u32,
    pub toc_list_height: u32,
    /// The thumb row the pointer holds while dragging the contents scroll bar.
    toc_grab: Option<u32>,
    pub entries: Vec<Entry>,
    pub wrap: bool,
    pub help: bool,
    pub help_top: u32,
    pub expanded: bool,
    pub notice: Option<Notice>,
    pub missing: bool,
    /// Backspace with no jump left goes back to the file opened before this one.
    pub back_file: bool,
    pub themes: Option<super::themes::List>,
    last_query: String,
    back: Vec<u32>,
    pinned: Option<usize>,
    prefix: Option<char>,
    numkey: Option<(char, usize)>,
    reverse: bool,
}

impl App {
    pub fn max_scroll(&self) -> u32 {
        (self.doc_rows + PADDING_ROWS).saturating_sub(self.viewport)
    }

    pub fn percent(&self) -> u32 {
        let max = self.max_scroll();
        (self.scroll * 100).checked_div(max).map_or(100, |p| p.min(100))
    }

    fn page(&self) -> u32 {
        self.viewport.saturating_sub(2).max(1)
    }

    fn half(&self) -> u32 {
        (self.viewport / 2).max(1)
    }

    pub fn search_active(&self) -> bool {
        !self.query.is_empty() || !self.matches.is_empty()
    }

    pub fn set_headings(&mut self, headings: &[Heading]) {
        self.entries = toc_entries(headings);
        self.pinned = None;
    }

    pub fn layout(&mut self, viewport: u32, doc_rows: u32, scroll: u32, headings: &[Heading]) {
        self.viewport = viewport;
        self.doc_rows = doc_rows;
        self.scroll = scroll.min(self.max_scroll());
        self.set_headings(headings);
        self.toc_list_height = viewport.saturating_sub(TOC_HEADER).max(1);
        self.clamp_toc();
        if let Some(l) = self.themes.as_mut() {
            l.reveal(self.toc_list_height as usize);
        }
    }

    pub fn click(&mut self) -> Vec<Action> {
        self.prefix = None;
        match self.labels.take() {
            Some(_) => vec![Action::EndLabels],
            None => vec![],
        }
    }

    pub fn modal_click(&mut self, press: bool, on_hint: bool) -> Option<Vec<Action>> {
        if !(self.help || self.expanded) || (press && on_hint) {
            return None;
        }
        // Help has no close button, so only its "?" hint or a key closes it.
        if !press || self.help {
            return Some(vec![]);
        }
        self.expanded = false;
        Some(vec![Action::Collapse])
    }

    pub fn set_expanded(&mut self, shown: bool) {
        self.expanded = shown;
    }

    pub fn collapse(&mut self) -> bool {
        std::mem::take(&mut self.expanded)
    }

    pub fn show_notice(&mut self, text: &str, click: OnClick) {
        self.notice = Some(Notice { text: text.to_string(), click });
    }

    pub fn clear_notice(&mut self) -> bool {
        self.notice.take().is_some()
    }

    pub fn hide_toc(&mut self) {
        self.toc_visible = false;
    }

    pub fn start_labels(&mut self, targets: usize) {
        self.labels = Some(Labels::new(targets));
    }

    pub fn void_labels(&mut self) -> bool {
        let up = self.labels.is_some();
        if up {
            self.labels = Some(Labels::new(0));
        }
        up
    }

    pub fn open_themes(&mut self, labels: Vec<String>, current: usize) {
        self.themes = Some(super::themes::List::new(labels, current, self.toc_list_height as usize));
    }

    pub fn close_themes(&mut self) -> Option<super::themes::List> {
        self.themes.take()
    }

    pub fn file_lost(&mut self) -> bool {
        !std::mem::replace(&mut self.missing, true)
    }

    pub fn file_found(&mut self) -> bool {
        std::mem::take(&mut self.missing)
    }

    pub fn fit_help(&mut self, max: u32) {
        self.help_top = self.help_top.min(max);
    }

    pub fn set_back_file(&mut self, back_file: bool) {
        self.back_file = back_file;
    }

    pub fn relocate_matches(&mut self, rows: Vec<u32>) {
        self.matches = rows;
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }

    pub fn replace(&mut self, mut next: App) -> App {
        (next.wrap, next.toc_visible, next.themes) = (self.wrap, self.toc_visible, self.themes.take());
        std::mem::replace(self, next)
    }

    fn reset_nav(&mut self) {
        self.numkey = None;
        self.reverse = false;
        self.pinned = None;
    }

    pub fn scroll_to(&mut self, row: u32) -> Vec<Action> {
        let s = row.min(self.max_scroll());
        self.reset_nav();
        if s == self.scroll {
            return vec![Action::Status];
        }
        self.scroll = s;
        self.follow_toc();
        vec![Action::Scroll]
    }

    pub fn jump(&mut self, row: u32) -> Vec<Action> {
        let from = self.scroll;
        let a = self.scroll_to(row);
        if self.scroll != from && self.back.last() != Some(&from) {
            self.back.push(from);
            if self.back.len() > HISTORY {
                self.back.remove(0);
            }
        }
        a
    }

    /// The rows Backspace returns to, oldest first.
    pub fn back_rows(&self) -> &[u32] {
        &self.back
    }

    /// Moves the Backspace history to a new layout, one row for each in `back_rows`.
    pub fn set_back_rows(&mut self, rows: Vec<u32>) {
        self.back = rows;
    }

    fn go_back(&mut self) -> Vec<Action> {
        let Some(row) = self.back.pop() else {
            return vec![if self.back_file { Action::BackFile } else { Action::Notice("Nothing to go back to".into()) }];
        };
        self.scroll_to(row)
    }

    pub fn follow_toc(&mut self) {
        let Some(c) = self.current_entry() else { return };
        let (c, h) = (c as u32, self.toc_list_height.max(1));
        if c < self.toc_offset || c >= self.toc_offset + h {
            self.toc_offset = c.saturating_sub(h / 3);
            self.clamp_toc();
        }
    }

    fn down(&mut self, n: u32) -> Vec<Action> {
        self.scroll_to(self.scroll.saturating_add(n))
    }

    fn up(&mut self, n: u32) -> Vec<Action> {
        self.scroll_to(self.scroll.saturating_sub(n))
    }

    pub fn set_matches(&mut self, rows: Vec<u32>) -> Vec<Action> {
        self.matches = rows;
        let below = self.matches.iter().position(|&r| r >= self.scroll);
        self.current = below.unwrap_or(0);
        if self.matches.is_empty() {
            return vec![Action::Status];
        }
        let mut a = if below.is_none() { vec![Action::Notice("Search wrapped to the top".into())] } else { vec![] };
        a.extend(self.jump_current());
        a
    }

    fn visible_row(&self, row: u32) -> bool {
        row >= self.scroll && row < self.scroll + self.viewport
    }

    fn jump_current(&mut self) -> Vec<Action> {
        let row = self.matches[self.current];
        let mut a =
            if self.visible_row(row) && row >= self.scroll + MATCH_CONTEXT.min(self.viewport / 2) { vec![Action::Status] } else { self.jump(row.saturating_sub(MATCH_CONTEXT.min(self.viewport / 2))) };
        a.push(Action::Current(self.current));
        a
    }

    fn clear_search(&mut self) -> Vec<Action> {
        if !self.query.is_empty() {
            self.last_query = std::mem::take(&mut self.query);
        }
        self.query.clear();
        self.matches.clear();
        self.current = 0;
        self.notice = None;
        vec![Action::ClearSearch, Action::Status]
    }

    fn again(&mut self) -> Vec<Action> {
        if !self.query.is_empty() {
            return vec![Action::Status];
        }
        if self.last_query.is_empty() {
            return vec![Action::Notice("No search yet. Press / to search".into())];
        }
        self.query = self.last_query.clone();
        vec![Action::Search(self.query.clone())]
    }

    fn next(&mut self) -> Vec<Action> {
        if self.matches.is_empty() {
            return self.again();
        }
        self.notice = None;
        let n = self.matches.len();
        let cur = self.matches[self.current];
        let next = if self.visible_row(cur) { self.current + 1 } else { self.matches.iter().position(|&r| r >= self.scroll).unwrap_or(n) };
        let mut a = vec![];
        if next >= n {
            a.push(Action::Notice("Search wrapped to the top".into()));
        }
        self.current = next % n;
        a.extend(self.jump_current());
        a
    }

    fn prev(&mut self) -> Vec<Action> {
        if self.matches.is_empty() {
            return self.again();
        }
        self.notice = None;
        let n = self.matches.len();
        let cur = self.matches[self.current];
        let prev = if self.visible_row(cur) { self.current.checked_sub(1) } else { self.matches.iter().rposition(|&r| r < self.scroll) };
        let mut a = vec![];
        if prev.is_none() {
            a.push(Action::Notice("Search wrapped to the bottom".into()));
        }
        self.current = prev.unwrap_or(n - 1);
        a.extend(self.jump_current());
        a
    }

    /// Whether a key moves the view at once, ending a glide even when it doesn't scroll: a row or wheel step,
    /// or a step between matches.
    pub fn instant(&self, key: Key) -> bool {
        self.prompt.is_none() && (matches!(scroll_step(key), Some((Step::Row | Step::Wheel, _))) || matches!(key, Key::Char('n' | 'N') | Key::Enter))
    }

    fn rows(&self, step: Step) -> u32 {
        match step {
            Step::Row => 1,
            Step::Wheel => WHEEL,
            Step::Half => self.half(),
            Step::Page => self.page(),
        }
    }

    pub fn handle(&mut self, key: Key) -> Vec<Action> {
        let key = if key == Key::Ctrl('t') && self.prompt.is_none() { Key::Char('t') } else { key };
        let instant = self.instant(key) || self.prompt.is_some();
        let leave =
            self.themes.is_some() && self.labels.is_none() && self.prompt.is_none() && !self.help && !self.expanded && (key == Key::Ctrl('p') || (key == Key::Char('t') && !self.entries.is_empty()));
        let mut a = self.handle_key(key);
        if leave {
            a.insert(0, Action::DropThemes);
        }
        if !instant {
            for x in &mut a {
                if *x == Action::Scroll {
                    *x = Action::Glide;
                }
            }
        }
        a
    }

    fn handle_key(&mut self, key: Key) -> Vec<Action> {
        let held = self.prefix.take();
        if key == Key::Copy {
            if self.labels.take().is_some() {
                return vec![Action::EndLabels, Action::CopySelection];
            }
            return vec![Action::CopySelection];
        }
        if let Some(l) = self.labels.as_mut() {
            return match key {
                Key::Ctrl('c') => vec![Action::Quit],
                Key::Char(c) => match l.push(c.to_ascii_lowercase()) {
                    labels::Step::Pick(i) => {
                        self.labels = None;
                        vec![Action::EndLabels, Action::Follow(i)]
                    }
                    labels::Step::Wait => vec![Action::Labels],
                    labels::Step::Miss => {
                        self.labels = None;
                        vec![Action::EndLabels]
                    }
                },
                Key::Backspace if l.typed.is_empty() => {
                    self.labels = None;
                    vec![Action::EndLabels]
                }
                Key::Backspace => {
                    l.pop();
                    vec![Action::Labels]
                }
                _ => {
                    self.labels = None;
                    vec![Action::EndLabels]
                }
            };
        }
        if self.expanded {
            return match key {
                Key::Char('q') | Key::Ctrl('c') => vec![Action::Quit],
                Key::WheelUp(..) | Key::WheelDown(..) | Key::WheelLeft(..) | Key::WheelRight(..) => vec![],
                _ => {
                    self.expanded = false;
                    vec![Action::Collapse]
                }
            };
        }
        if let Some(draft) = self.prompt.as_mut() {
            return match key {
                Key::Enter => {
                    let q = self.prompt.take().unwrap_or_default();
                    if q.is_empty() {
                        return self.clear_search();
                    }
                    self.query = q.clone();
                    vec![Action::Search(q)]
                }
                Key::Esc | Key::Ctrl('c') => {
                    self.prompt = None;
                    self.clear_search()
                }
                Key::Backspace if draft.is_empty() => {
                    self.prompt = None;
                    self.clear_search()
                }
                Key::Backspace => {
                    draft.pop();
                    vec![Action::Status]
                }
                Key::Ctrl('u') => {
                    draft.clear();
                    vec![Action::Status]
                }
                Key::Ctrl('w') => {
                    let keep = draft.trim_end().rfind(char::is_whitespace).map_or(0, |i| i + 1);
                    draft.truncate(keep);
                    vec![Action::Status]
                }
                Key::Char(c) if !c.is_control() => {
                    draft.push(c);
                    vec![Action::Status]
                }
                Key::Ctrl('z') => vec![Action::Suspend],
                Key::WheelUp(..) => self.up(WHEEL),
                Key::WheelDown(..) => self.down(WHEEL),
                Key::WheelLeft(..) | Key::WheelRight(..) => self.side(key),
                _ => vec![],
            };
        }
        if self.help {
            // Trackpads send sideways wheel events while scrolling up and down, so they must not close help.
            if let Key::WheelLeft(..) | Key::WheelRight(..) = key {
                return vec![];
            }
            let t = self.help_top;
            self.help_top = match (key, scroll_step(key)) {
                (_, Some((step, true))) => t + self.rows(step),
                (_, Some((step, false))) => t.saturating_sub(self.rows(step)),
                (Key::Char('q') | Key::Ctrl('c'), _) => return vec![Action::Quit],
                (Key::Ctrl('z'), _) => return vec![Action::Suspend],
                (Key::Char('?') | Key::Esc, _) => {
                    self.help = false;
                    0
                }
                // Only the keys help names close it, so a stray key is not lost.
                _ => return vec![],
            };
            return vec![Action::Help];
        }
        if self.themes.is_some() {
            return match key {
                Key::Char('j') | Key::Down => self.theme_step(true),
                Key::Char('k') | Key::Up => self.theme_step(false),
                Key::Char(' ') => self.theme_preview(),
                Key::Enter => vec![Action::PickTheme],
                Key::Esc | Key::Char('T') => vec![Action::CloseThemes],
                Key::Char('t') if !self.entries.is_empty() => {
                    self.toc_visible = false;
                    self.page_key(key, held)
                }
                _ => self.page_key(key, held),
            };
        }
        self.page_key(key, held)
    }

    fn page_key(&mut self, key: Key, held: Option<char>) -> Vec<Action> {
        if let Key::Char(c @ (']' | '[' | 'g')) = key {
            if held == Some(c) {
                if c == 'g' {
                    return self.scroll_to(0);
                }
                if !self.entries.is_empty() {
                    return self.cycle_root(c == ']');
                }
            }
            self.prefix = Some(c);
            return vec![];
        }
        match scroll_step(key) {
            Some((step, true)) => return self.down(self.rows(step)),
            Some((step, false)) => return self.up(self.rows(step)),
            None => {}
        }
        match key {
            Key::Home => self.scroll_to(0),
            Key::Char('G') | Key::End => self.scroll_to(u32::MAX),
            Key::Char('h') | Key::Left => vec![Action::ScrollView(-CODE_KEY)],
            Key::Char('l') | Key::Right => vec![Action::ScrollView(CODE_KEY)],
            Key::Char('?') => {
                self.help = true;
                vec![Action::Help]
            }
            Key::Backspace | Key::Ctrl('o') => self.go_back(),
            Key::WheelLeft(..) | Key::WheelRight(..) => self.side(key),
            Key::Char('/') | Key::Ctrl('f') => {
                self.prompt = Some(String::new());
                self.numkey = None;
                self.reverse = false;
                vec![Action::Status]
            }
            Key::Char('n') => self.next(),
            Key::Char('N') => self.prev(),
            Key::Enter if self.search_active() => self.next(),
            Key::Esc if self.search_active() => self.clear_search(),
            Key::Esc if self.toc_visible => {
                self.toc_visible = false;
                vec![Action::ToggleToc]
            }
            Key::Char('q') | Key::Ctrl('c') => vec![Action::Quit],
            Key::Char('e') => vec![Action::Editor],
            Key::Char('f') => vec![Action::StartLabels],
            Key::Char('c') => vec![Action::Copy],
            Key::Ctrl('z') => vec![Action::Suspend],
            Key::Ctrl('p') => vec![Action::Browse],
            Key::Char('T') => vec![Action::OpenThemes],
            Key::Char('t') if self.entries.is_empty() => vec![Action::Notice("This file has no headings".into())],
            Key::Char('t') => {
                self.toc_visible = !self.toc_visible;
                self.toc_offset = 0;
                self.follow_toc();
                vec![Action::ToggleToc]
            }
            Key::Char('w') => {
                self.wrap = !self.wrap;
                let note = if self.wrap { "Long code lines now wrap" } else { "Long code lines now scroll sideways: h l or Shift+wheel" };
                vec![Action::ToggleWrap, Action::Notice(note.into())]
            }
            Key::Char('0') => {
                self.reverse = !self.reverse;
                vec![]
            }
            Key::Char(c @ '1'..='9') => self.digit(c),
            Key::Char('J') if !self.entries.is_empty() => self.cycle_root(true),
            Key::Char('K') if !self.entries.is_empty() => self.cycle_root(false),
            Key::Char('D') if self.toc_visible && !self.entries.is_empty() => self.toc_scroll(true),
            Key::Char('U') if self.toc_visible && !self.entries.is_empty() => self.toc_scroll(false),
            _ => vec![],
        }
    }

    fn side(&self, key: Key) -> Vec<Action> {
        match key {
            Key::WheelLeft(col, row) => vec![Action::ScrollCode(col, row, -CODE_WHEEL)],
            Key::WheelRight(col, row) => vec![Action::ScrollCode(col, row, CODE_WHEEL)],
            _ => vec![],
        }
    }

    fn group(&self, n: usize) -> Vec<usize> {
        let roots: Vec<usize> = self.entries.iter().enumerate().filter(|(_, e)| e.root).map(|(i, _)| i).collect();
        let Some(&start) = roots.get(n) else { return vec![] };
        let mut g = vec![start];
        g.extend((start + 1..self.entries.len()).take_while(|&i| !self.entries[i].root));
        g
    }

    fn digit(&mut self, c: char) -> Vec<Action> {
        let n = c as usize - '1' as usize;
        let g = self.group(n);
        if g.is_empty() {
            return vec![];
        }
        let pos = match self.numkey {
            Some((last, p)) if last == c => {
                if self.reverse {
                    (p + g.len() - 1) % g.len()
                } else {
                    (p + 1) % g.len()
                }
            }
            _ => {
                self.reverse = false;
                0
            }
        };
        let mut pos = pos;
        if self.numkey.is_some_and(|(last, _)| last == c) {
            for _ in 1..g.len() {
                if self.entries[g[pos]].row.min(self.max_scroll()) != self.scroll {
                    break;
                }
                pos = if self.reverse { (pos + g.len() - 1) % g.len() } else { (pos + 1) % g.len() };
            }
        }
        let entry = g[pos];
        let reverse = self.reverse;
        let mut a = self.jump(self.entries[entry].row);
        self.numkey = Some((c, pos));
        self.reverse = reverse;
        self.pinned = Some(entry);
        self.follow_toc();
        a.push(Action::Status);
        a
    }

    fn cycle_root(&mut self, forward: bool) -> Vec<Action> {
        let roots: Vec<usize> = self.entries.iter().enumerate().filter(|(_, e)| e.root).map(|(i, _)| i).collect();
        if roots.is_empty() {
            return vec![];
        }
        let at = self.current_entry().and_then(|c| roots.iter().rposition(|&r| r <= c));
        let next = match (at, forward) {
            (Some(i), true) if self.entries[roots[i]].row > self.scroll && self.pinned != Some(roots[i]) => roots[i],
            (Some(i), true) => roots[(i + 1) % roots.len()],
            (Some(i), false) if self.entries[roots[i]].row < self.scroll && self.pinned != Some(roots[i]) => roots[i],
            (Some(i), false) => roots[(i + roots.len() - 1) % roots.len()],
            (None, true) => roots[0],
            (None, false) => roots[roots.len() - 1],
        };
        let mut a = self.jump(self.entries[next].row);
        self.pinned = Some(next);
        self.follow_toc();
        a.push(Action::Status);
        a
    }

    fn theme_step(&mut self, down: bool) -> Vec<Action> {
        let height = self.toc_list_height as usize;
        let moved = self.themes.as_mut().is_some_and(|l| l.step(down, height));
        if moved { vec![Action::Status] } else { vec![] }
    }

    pub fn theme_hover(&mut self, line: u32) -> Vec<Action> {
        let Some(row) = line.checked_sub(TOC_HEADER) else { return vec![] };
        let moved = self.themes.as_mut().is_some_and(|l| l.point(row as usize));
        if moved { vec![Action::Status] } else { vec![] }
    }

    pub fn theme_click(&mut self, line: u32) -> Vec<Action> {
        let Some(l) = self.themes.as_mut() else { return vec![] };
        let Some(i) = line.checked_sub(TOC_HEADER).and_then(|r| l.at(r as usize)) else { return vec![] };
        l.sel = i;
        self.theme_preview()
    }

    fn theme_preview(&mut self) -> Vec<Action> {
        let changed = self.themes.as_mut().is_some_and(super::themes::List::preview);
        if changed { vec![Action::PreviewTheme] } else { vec![Action::Status] }
    }

    pub fn theme_wheel(&mut self, down: bool) -> Vec<Action> {
        self.theme_step(down)
    }

    pub fn toc_click(&mut self, line: u32) -> Vec<Action> {
        let Some(i) = line.checked_sub(TOC_HEADER).filter(|&n| n < self.toc_list_height).map(|n| n as usize + self.toc_offset as usize).filter(|&i| i < self.entries.len()) else { return vec![] };
        let mut a = self.jump(self.entries[i].row);
        self.pinned = Some(i);
        a.push(Action::Status);
        a
    }

    fn clamp_toc(&mut self) {
        self.toc_offset = self.toc_offset.min((self.entries.len() as u32).saturating_sub(self.toc_list_height));
    }

    fn toc_scroll(&mut self, down: bool) -> Vec<Action> {
        self.toc_move(down, (self.toc_list_height / 2).max(1))
    }

    /// None when every entry fits, so the wheel scrolls the page instead.
    pub fn toc_wheel(&mut self, down: bool) -> Option<Vec<Action>> {
        self.toc_thumb()?;
        Some(self.toc_move(down, WHEEL))
    }

    fn toc_move(&mut self, down: bool, step: u32) -> Vec<Action> {
        let max = (self.entries.len() as u32).saturating_sub(self.toc_list_height);
        self.toc_offset = if down { (self.toc_offset + step).min(max) } else { self.toc_offset.saturating_sub(step) };
        vec![Action::Status]
    }

    /// The contents scroll bar's thumb as (top, length) in list rows, or None when every entry fits.
    pub fn toc_thumb(&self) -> Option<(u32, u32)> {
        let (n, h) = (self.entries.len() as u32, self.toc_list_height.max(1));
        if n <= h {
            return None;
        }
        let len = ((h * h + n / 2) / n).clamp(1, h.saturating_sub(2).max(1));
        let (max, space) = (n - h, h - len);
        let off = self.toc_offset.min(max);
        // Off either end, the thumb leaves that end, so the bar never looks done while entries are hidden.
        let top = match off {
            0 => 0,
            o if o == max => space,
            o if space < 2 => (o * space + max / 2) / max,
            o => ((o * space + max / 2) / max).clamp(1, space - 1),
        };
        Some((top, len))
    }

    /// A press on screen line `line` (0-based), `from_edge` columns left of the sidebar's last one. On the thumb
    /// it holds the thumb where it was pressed; on the track it centers the thumb there first. None off the bar.
    pub fn toc_bar_press(&mut self, from_edge: u16, line: u32) -> Option<Vec<Action>> {
        if from_edge >= TOC_BAR {
            return None;
        }
        let (top, len) = self.toc_thumb()?;
        let row = line.checked_sub(TOC_HEADER).filter(|&r| r < self.toc_list_height)?;
        if (top..top + len).contains(&row) {
            self.toc_grab = Some(row - top);
            return Some(vec![]);
        }
        self.toc_grab = Some(len / 2);
        Some(self.toc_bar_drag(line))
    }

    /// Moves the held thumb so its grabbed row sits on screen line `line`.
    pub fn toc_bar_drag(&mut self, line: u32) -> Vec<Action> {
        let (Some(grab), Some((now, len))) = (self.toc_grab, self.toc_thumb()) else { return vec![] };
        let (n, h) = (self.entries.len() as u32, self.toc_list_height.max(1));
        let (max, space) = (n - h, h - len);
        let top = i64::from(line) - i64::from(TOC_HEADER) - i64::from(grab);
        let top = top.clamp(0, i64::from(space)) as u32;
        if top == now {
            return vec![];
        }
        self.toc_offset = (top * max + space / 2).checked_div(space).unwrap_or(0);
        vec![Action::Status]
    }

    pub fn toc_bar_held(&self) -> bool {
        self.toc_grab.is_some()
    }

    pub fn toc_bar_release(&mut self) {
        self.toc_grab = None;
    }

    /// The contents sidebar's width at `cols` columns: 0 while it is hidden or has no entries.
    pub fn toc_cols(&self, cols: u16) -> u16 {
        if (self.toc_visible && !self.entries.is_empty()) || self.themes.is_some() { toc_width(cols) } else { 0 }
    }

    pub fn current_entry(&self) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        if let Some(p) = self.pinned {
            return Some(p);
        }
        let max = self.max_scroll();
        if max > 0 && self.scroll >= max {
            return Some(self.entries.len() - 1);
        }
        Some(self.entries.iter().rposition(|e| e.row <= self.scroll).unwrap_or(0))
    }

    pub fn status(&self, name: &str) -> String {
        if let Some(d) = &self.prompt {
            return format!(" /{d} ");
        }
        match self.notice.as_ref().map(|n| &n.text) {
            Some(n) if !self.matches.is_empty() => format!("{}· {n} ", self.status_base(name)),
            Some(n) => format!(" {n} "),
            None => self.status_base(name),
        }
    }

    pub fn status_base(&self, name: &str) -> String {
        if let Some(d) = &self.prompt {
            return format!(" /{d} ");
        }
        let mut s = format!(" {name} ");
        if self.missing {
            s.push_str("(file gone, showing the last version) ");
        }
        if !self.query.is_empty() {
            if self.matches.is_empty() {
                s.push_str(&format!(" no matches for \"{}\" ", self.query));
            } else {
                s.push_str(&format!(" \"{}\" {}/{} ", self.query, self.current + 1, self.matches.len()));
            }
        } else if let Some(e) = self.current_entry().filter(|&e| self.notice.is_none() && !self.toc_visible && self.themes.is_none() && self.entries[e].row <= self.scroll && self.scroll > 0) {
            s.push_str(&format!("› {} ", self.entries[e].title));
        }
        s
    }

    /// The bottom row's hints: the label, what it does, and the key a click on it sends.
    pub fn hints(&self) -> Vec<Hint> {
        const HELP: Hint = ("?", "help", Key::Char('?'));
        if self.expanded {
            return vec![QUIT, ("Esc", "back to the page", Key::Esc)];
        }
        if self.prompt.is_some() {
            return vec![("Enter", "search", Key::Enter), ("Esc", "cancel", Key::Esc)];
        }
        if self.help {
            return vec![QUIT, ("?", "back to the page", Key::Char('?')), ("j/k", "scroll", Key::Char(' '))];
        }
        if self.themes.is_some() {
            return vec![("Space", "preview", Key::Char(' ')), ("Enter", "save", Key::Enter), ("Esc", "cancel", Key::Esc)];
        }
        if !self.query.is_empty() && self.matches.is_empty() {
            return vec![QUIT, ("/", "search again", Key::Char('/')), ("Esc", "clear", Key::Esc)];
        }
        if self.search_active() {
            return vec![QUIT, ("n/N", "next/prev", Key::Char('n')), ("Esc", "clear", Key::Esc), HELP];
        }
        let mut h = vec![HELP, ("/", "find", Key::Char('/'))];
        if !self.entries.is_empty() {
            h.push(("t", "toc", Key::Char('t')));
        }
        h.push(("e", "edit", Key::Char('e')));
        h
    }
}

/// How far a scroll key moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Row,
    Wheel,
    Half,
    Page,
}

/// The scroll keys, for the page and the help screen alike: the step, and whether it goes down.
fn scroll_step(key: Key) -> Option<(Step, bool)> {
    Some(match key {
        Key::Char('j') | Key::Down | Key::Ctrl('e') => (Step::Row, true),
        Key::Char('k') | Key::Up | Key::Ctrl('y') => (Step::Row, false),
        Key::WheelDown(..) => (Step::Wheel, true),
        Key::WheelUp(..) => (Step::Wheel, false),
        Key::Char('d') | Key::Ctrl('d') => (Step::Half, true),
        Key::Char('u') | Key::Ctrl('u') => (Step::Half, false),
        Key::Char(' ') | Key::PageDown => (Step::Page, true),
        Key::PageUp | Key::Ctrl('b') => (Step::Page, false),
        _ => return None,
    })
}

/// Every heading in document order. A heading nests under the open headings above it with a smaller
/// level, whatever the jump between levels. Roots sit at depth 0, or at depth 1 when one heading
/// alone sits at depth 0 and others nest under it, as a title does.
pub fn toc_entries(headings: &[Heading]) -> Vec<Entry> {
    let mut open: Vec<u8> = Vec::new();
    let depths: Vec<u8> = headings
        .iter()
        .map(|h| {
            while open.last().is_some_and(|&l| l >= h.level) {
                open.pop();
            }
            open.push(h.level);
            u8::try_from(open.len() - 1).unwrap_or(u8::MAX)
        })
        .collect();
    let titled = depths.len() > 1 && depths.iter().filter(|&&d| d == 0).count() == 1;
    let root = u8::from(titled);
    headings.iter().zip(depths).map(|(h, depth)| Entry { depth, root: depth == root, title: h.title.clone(), row: h.row }).collect()
}

pub fn toc_width(cols: u16) -> u16 {
    if cols < TOC_MIN_COLS { 0 } else { (TOC_CONTENT + 1).min(cols / 3) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(doc_rows: u32, viewport: u32) -> App {
        App { doc_rows, viewport, toc_list_height: 10, ..Default::default() }
    }

    fn keys(a: &mut App, ks: &[Key]) -> Vec<Action> {
        ks.iter().flat_map(|k| a.handle(*k)).collect()
    }

    fn h(level: u8, title: &str, row: u32) -> Heading {
        Heading { level, title: title.into(), row }
    }

    #[test]
    fn k01_j_down() {
        let mut a = app(100, 20);
        keys(&mut a, &[Key::Char('j'), Key::Down]);
        assert_eq!(a.scroll, 2);
        a.scroll = a.max_scroll();
        keys(&mut a, &[Key::Char('j')]);
        assert_eq!(a.scroll, a.max_scroll());
    }

    #[test]
    fn k02_k_up() {
        let mut a = app(100, 20);
        a.scroll = 5;
        keys(&mut a, &[Key::Char('k'), Key::Up]);
        assert_eq!(a.scroll, 3);
        a.scroll = 0;
        keys(&mut a, &[Key::Char('k')]);
        assert_eq!(a.scroll, 0);
    }

    #[test]
    fn k03_page_down() {
        for (k, n) in [(Key::Char('d'), 10), (Key::Ctrl('d'), 10), (Key::PageDown, 18), (Key::Char(' '), 18)] {
            let mut a = app(100, 20);
            a.handle(k);
            assert_eq!(a.scroll, n, "{k:?}");
        }
        let mut a = app(30, 20);
        keys(&mut a, &[Key::Char(' '), Key::Char(' ')]);
        assert_eq!(a.scroll, a.max_scroll());
        assert_eq!(a.scroll, 15);
        let mut a = app(100, 4);
        a.handle(Key::Char(' '));
        assert_eq!(a.scroll, 2);
    }

    #[test]
    fn k04_page_up() {
        for (k, n) in [(Key::Char('u'), 40), (Key::Ctrl('u'), 40), (Key::PageUp, 32), (Key::Ctrl('b'), 32)] {
            let mut a = app(100, 20);
            a.scroll = 50;
            a.handle(k);
            assert_eq!(a.scroll, n, "{k:?}");
        }
        let mut a = app(100, 20);
        a.scroll = 7;
        a.handle(Key::Char('u'));
        assert_eq!(a.scroll, 0);
    }

    #[test]
    fn less_only_keys_do_nothing() {
        for k in [Key::Char('b'), Key::Char('v'), Key::Char('Q')] {
            let mut a = app(100, 20);
            a.scroll = 40;
            assert_eq!(a.handle(k), vec![], "{k:?}");
            assert_eq!(a.scroll, 40, "{k:?}");
        }
    }

    #[test]
    fn f_asks_for_labels() {
        assert_eq!(app(100, 20).handle(Key::Char('f')), vec![Action::StartLabels]);
    }

    #[test]
    fn typing_a_label_follows_its_target() {
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(20));
        let two = a.labels.as_ref().unwrap().all.iter().position(|l| l.len() == 2).unwrap();
        let label = a.labels.as_ref().unwrap().all[two].clone();
        let mut cs = label.chars();
        assert_eq!(a.handle(Key::Char(cs.next().unwrap())), vec![Action::Labels]);
        assert_eq!(a.handle(Key::Char(cs.next().unwrap())), vec![Action::EndLabels, Action::Follow(two)]);
        assert!(a.labels.is_none());
    }

    #[test]
    fn labels_take_uppercase_letters_too() {
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(3));
        assert_eq!(a.handle(Key::Char('A')), vec![Action::EndLabels, Action::Follow(1)]);
    }

    #[test]
    fn a_key_that_continues_no_label_ends_labels_and_does_nothing_else() {
        for k in ['q', '/', 'x', '1', 'z'] {
            let mut a = app(100, 20);
            a.labels = Some(Labels::new(20));
            assert_eq!(a.handle(Key::Char(k)), vec![Action::EndLabels], "{k:?}");
            assert!(a.labels.is_none());
        }
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(20));
        let first = a.labels.as_ref().unwrap().all.iter().find(|l| l.len() == 2).unwrap().chars().next().unwrap();
        a.handle(Key::Char(first));
        assert_eq!(a.handle(Key::Char('h')), vec![Action::EndLabels]);
    }

    #[test]
    fn cmd_c_copies_and_ends_labels() {
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(3));
        assert_eq!(a.handle(Key::Copy), vec![Action::EndLabels, Action::CopySelection]);
        assert!(a.labels.is_none());
    }

    #[test]
    fn a_key_that_ends_labels_leaves_the_theme_list_alone() {
        let mut a = app(100, 20);
        a.set_headings(&[h(1, "One", 0)]);
        a.themes = Some(super::super::themes::List::new(vec!["github-dark".into()], 0, 10));
        for k in [Key::Ctrl('p'), Key::Char('t')] {
            a.labels = Some(Labels::new(3));
            assert_eq!(a.handle(k), vec![Action::EndLabels], "{k:?}");
            assert!(a.themes.is_some());
        }
    }

    #[test]
    fn with_no_labels_left_the_next_key_only_ends_them() {
        for k in [Key::Char('e'), Key::Char('c'), Key::Char('q')] {
            let mut a = app(100, 20);
            a.labels = Some(Labels::new(0));
            assert_eq!(a.handle(k), vec![Action::EndLabels], "{k:?}");
        }
    }

    #[test]
    fn backspace_with_nothing_typed_ends_labels() {
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(3));
        assert_eq!(a.handle(Key::Backspace), vec![Action::EndLabels]);
        assert!(a.labels.is_none());
    }

    #[test]
    fn backspace_in_labels_takes_back_a_letter() {
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(20));
        a.handle(Key::Char('s'));
        assert_eq!(a.handle(Key::Backspace), vec![Action::Labels]);
        assert_eq!(a.labels.as_ref().unwrap().typed, "");
    }

    #[test]
    fn esc_or_another_key_ends_labels_and_does_nothing_else() {
        for k in [Key::Esc, Key::Down, Key::PageDown, Key::Enter] {
            let mut a = app(100, 20);
            a.labels = Some(Labels::new(3));
            assert_eq!(a.handle(k), vec![Action::EndLabels], "{k:?}");
            assert!(a.labels.is_none());
            assert_eq!(a.scroll, 0);
        }
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(3));
        assert_eq!(a.handle(Key::Ctrl('c')), vec![Action::Quit]);
    }

    #[test]
    fn a_click_ends_labels() {
        let mut a = app(100, 20);
        a.labels = Some(Labels::new(3));
        assert_eq!(a.click(), vec![Action::EndLabels]);
        assert!(a.labels.is_none());
        assert_eq!(a.click(), vec![]);
    }

    #[test]
    fn a_lone_g_is_dropped_by_anything_between() {
        let between: [&dyn Fn(&mut App); 3] = [
            &|a| {
                keys(a, &[Key::Char('/'), Key::Esc]);
            },
            &|a| {
                keys(a, &[Key::Char('?'), Key::Char('?')]);
            },
            &|a| {
                a.click();
            },
        ];
        for step in between {
            let mut a = app(100, 20);
            a.scroll = 40;
            a.handle(Key::Char('g'));
            step(&mut a);
            a.handle(Key::Char('g'));
            assert_eq!(a.scroll, 40);
        }
    }

    #[test]
    fn k05_top() {
        for k in [&[Key::Char('g'), Key::Char('g')][..], &[Key::Home]] {
            let mut a = app(100, 20);
            a.scroll = 40;
            keys(&mut a, k);
            assert_eq!(a.scroll, 0);
        }
        let mut a = app(100, 20);
        a.scroll = 40;
        keys(&mut a, &[Key::Char('g')]);
        assert_eq!(a.scroll, 40);
        keys(&mut a, &[Key::Char('j'), Key::Char('g')]);
        assert_eq!(a.scroll, 41);
        let mut a = app(100, 20);
        a.scroll = 40;
        a.handle(Key::Ctrl('g'));
        assert_eq!(a.scroll, 40);
    }

    #[test]
    fn k06_bottom() {
        for k in [Key::Char('G'), Key::End] {
            let mut a = app(100, 20);
            a.handle(k);
            assert_eq!(a.scroll, 85);
        }
    }

    #[test]
    fn k07_bottom_padding() {
        let mut a = app(100, 20);
        a.handle(Key::Char('G'));
        let last_content_row_in_view = a.doc_rows - 1 - a.scroll;
        assert_eq!(last_content_row_in_view, a.viewport - 6);
    }

    #[test]
    fn k08_short_doc_no_scroll() {
        let mut a = app(15, 20);
        assert_eq!(a.max_scroll(), 0);
        for k in [Key::Char('j'), Key::Char('d'), Key::Char('G'), Key::WheelDown(1, 1)] {
            a.handle(k);
            assert_eq!(a.scroll, 0);
        }
    }

    #[test]
    fn k09_short_doc_overscroll() {
        let mut a = app(18, 20);
        assert_eq!(a.max_scroll(), 3);
        a.handle(Key::Char('G'));
        assert_eq!(a.scroll, 3);
    }

    #[test]
    fn k10_percent() {
        let a = app(15, 20);
        assert_eq!(a.percent(), 100);
        let mut a = app(105, 20);
        a.scroll = 33;
        assert_eq!(a.max_scroll(), 90);
        assert_eq!(a.percent(), 36);
        a.handle(Key::Char('G'));
        assert_eq!(a.percent(), 100);
    }

    #[test]
    fn k11_wheel() {
        let mut a = app(100, 20);
        a.handle(Key::WheelDown(1, 1));
        assert_eq!(a.scroll, 3);
        a.handle(Key::WheelUp(1, 1));
        a.handle(Key::WheelUp(1, 1));
        assert_eq!(a.scroll, 0);
    }

    #[test]
    fn s01_open_prompt() {
        let mut a = app(100, 20);
        a.query = "old".into();
        a.handle(Key::Char('/'));
        assert_eq!(a.prompt.as_deref(), Some(""));
        let mut a = app(100, 20);
        a.handle(Key::Ctrl('f'));
        assert_eq!(a.prompt.as_deref(), Some(""));
    }

    #[test]
    fn s02_prompt_typing() {
        let mut a = app(100, 20);
        let acts = keys(&mut a, &[Key::Char('/'), Key::Char('q'), Key::Char('/'), Key::Char('n'), Key::Char('j')]);
        assert_eq!(a.prompt.as_deref(), Some("q/nj"));
        assert_eq!(a.scroll, 0);
        assert!(!acts.iter().any(|x| matches!(x, Action::Search(_) | Action::Quit)));
    }

    #[test]
    fn s02_prompt_ignores_control_keys() {
        let mut a = app(100, 20);
        keys(&mut a, &[Key::Char('/'), Key::Ctrl('i'), Key::Ctrl('w'), Key::Char('a')]);
        assert_eq!(a.prompt.as_deref(), Some("a"));
        assert_eq!(a.handle(Key::Enter), vec![Action::Search("a".into())]);
    }

    #[test]
    fn s03_prompt_backspace() {
        let mut a = app(100, 20);
        keys(&mut a, &[Key::Char('/'), Key::Char('a'), Key::Backspace]);
        assert_eq!(a.prompt.as_deref(), Some(""));
        a.handle(Key::Backspace);
        assert!(a.prompt.is_none());
    }

    #[test]
    fn s03_prompt_editing() {
        let mut a = app(100, 20);
        keys(&mut a, &[Key::Char('/'), Key::Char('a'), Key::Char(' '), Key::Char('b'), Key::Char('c'), Key::Ctrl('w')]);
        assert_eq!(a.prompt.as_deref(), Some("a "));
        a.handle(Key::Ctrl('u'));
        assert_eq!(a.prompt.as_deref(), Some(""));
    }

    #[test]
    fn s04_enter_empty_clears() {
        let mut a = app(100, 20);
        a.query = "x".into();
        a.matches = vec![3];
        keys(&mut a, &[Key::Char('/'), Key::Enter]);
        assert!(a.prompt.is_none());
        assert!(a.query.is_empty() && a.matches.is_empty());
    }

    #[test]
    fn s05_enter_jumps_first() {
        let mut a = app(100, 20);
        a.scroll = 50;
        let acts = keys(&mut a, &[Key::Char('/'), Key::Char('x'), Key::Enter]);
        assert_eq!(acts.last(), Some(&Action::Search("x".into())));
        a.set_matches(vec![10, 60, 70]);
        assert_eq!((a.current, a.scroll), (1, 50));
        a.scroll = 0;
        a.set_matches(vec![40, 60]);
        assert_eq!((a.current, a.scroll), (0, 37));
    }

    #[test]
    fn s06_prompt_cancel_clears() {
        for k in [Key::Esc, Key::Ctrl('c')] {
            let mut a = app(100, 20);
            a.query = "x".into();
            a.matches = vec![3, 4];
            keys(&mut a, &[Key::Char('/'), Key::Char('y'), k]);
            assert!(a.prompt.is_none());
            assert!(a.query.is_empty() && a.matches.is_empty());
        }
    }

    #[test]
    fn s07_next_wraps() {
        let mut a = app(100, 20);
        a.query = "x".into();
        a.set_matches(vec![10, 30, 50]);
        let seq: Vec<u32> = (0..4)
            .map(|_| {
                a.handle(Key::Char('n'));
                a.scroll
            })
            .collect();
        assert_eq!(seq, vec![27, 47, 7, 27]);
    }

    #[test]
    fn s08_prev_wraps() {
        let mut a = app(100, 20);
        a.query = "x".into();
        a.set_matches(vec![10, 30, 50]);
        let seq: Vec<u32> = (0..4)
            .map(|_| {
                a.handle(Key::Char('N'));
                a.scroll
            })
            .collect();
        assert_eq!(seq, vec![47, 27, 7, 47]);
    }

    #[test]
    fn s09_landing_top() {
        let mut a = app(100, 20);
        a.query = "x".into();
        a.set_matches(vec![5, 8, 99]);
        assert_eq!(a.scroll, 0);
        a.handle(Key::Char('n'));
        assert_eq!(a.scroll, 0);
        a.handle(Key::Char('n'));
        assert_eq!(a.scroll, a.max_scroll());
    }

    #[test]
    fn s09_n_continues_from_the_screen() {
        let mut a = app(200, 20);
        a.query = "x".into();
        a.set_matches(vec![10, 50, 90, 130]);
        a.scroll = 80;
        a.handle(Key::Char('n'));
        assert_eq!((a.current, a.scroll), (2, 80));
        a.scroll = 120;
        a.handle(Key::Char('N'));
        assert_eq!(a.current, 2);
        assert!(a.handle(Key::Char('n')).iter().all(|x| !matches!(x, Action::Notice(_))));
        assert!(a.handle(Key::Char('n')).contains(&Action::Notice("Search wrapped to the top".into())));
    }

    #[test]
    fn s10_no_matches() {
        let mut a = app(100, 20);
        a.scroll = 12;
        keys(&mut a, &[Key::Char('/'), Key::Char('z'), Key::Enter]);
        a.set_matches(vec![]);
        assert_eq!(a.query, "z");
        assert!(a.status("f.md").contains("no matches for \"z\""));
        assert!(!a.hints().iter().any(|h| h.0 == "n/N"));
        keys(&mut a, &[Key::Char('n'), Key::Char('N')]);
        assert_eq!(a.scroll, 12);
    }

    #[test]
    fn s10_n_after_esc_brings_search_back() {
        let mut a = app(100, 20);
        assert_eq!(a.handle(Key::Char('n')), vec![Action::Notice("No search yet. Press / to search".into())]);
        keys(&mut a, &[Key::Char('/'), Key::Char('x'), Key::Enter]);
        a.set_matches(vec![30]);
        a.handle(Key::Esc);
        assert_eq!(a.handle(Key::Char('n')), vec![Action::Search("x".into())]);
    }

    #[test]
    fn f01_ctrl_p_lists_files() {
        let mut a = app(100, 20);
        assert_eq!(a.handle(Key::Ctrl('p')), vec![Action::Browse]);
        a.prompt = Some(String::new());
        assert_eq!(a.handle(Key::Ctrl('p')), vec![]);
    }

    #[test]
    fn f02_back_goes_to_the_file_before_after_the_jumps() {
        let mut a = app(100, 20);
        assert_eq!(a.handle(Key::Backspace), vec![Action::Notice("Nothing to go back to".into())]);
        a.back_file = true;
        a.jump(40);
        assert_eq!(a.handle(Key::Backspace), vec![Action::Glide]);
        assert_eq!(a.scroll, 0);
        assert_eq!(a.handle(Key::Ctrl('o')), vec![Action::BackFile]);
    }

    #[test]
    fn s11_esc_clears_never_quits() {
        let mut a = app(100, 20);
        a.query = "x".into();
        a.matches = vec![1];
        assert!(!a.handle(Key::Esc).contains(&Action::Quit));
        assert!(!a.search_active());
        assert!(!a.handle(Key::Esc).contains(&Action::Quit));
    }

    #[test]
    fn s12_ctrl_c() {
        let mut a = app(100, 20);
        a.query = "x".into();
        assert_eq!(a.handle(Key::Ctrl('c')), vec![Action::Quit]);
    }

    #[test]
    fn s13_enter_next() {
        let mut a = app(100, 20);
        assert!(a.handle(Key::Enter).is_empty());
        a.query = "x".into();
        a.set_matches(vec![10, 30]);
        a.handle(Key::Enter);
        assert_eq!(a.scroll, 27);
    }

    #[test]
    fn s14_case_insensitive_literal() {
        let mut a = app(100, 20);
        let acts = keys(&mut a, &[Key::Char('/'), Key::Char(' '), Key::Char('A'), Key::Char('.'), Key::Enter]);
        assert_eq!(acts.last(), Some(&Action::Search(" A.".into())));
    }

    #[test]
    fn t01_toggle() {
        let mut a = app(100, 20);
        assert!(matches!(a.handle(Key::Char('t'))[..], [Action::Notice(_)]));
        a.set_headings(&[h(2, "A", 10)]);
        assert!(!a.toc_visible);
        assert_eq!(a.handle(Key::Char('t')), vec![Action::ToggleToc]);
        assert!(a.toc_visible);
        a.handle(Key::Char('t'));
        assert!(!a.toc_visible);
        a.handle(Key::Char('t'));
        a.handle(Key::Esc);
        assert!(!a.toc_visible);
    }

    #[test]
    fn t02_width() {
        assert_eq!(toc_width(140), 31);
        assert_eq!(toc_width(60), 20);
        assert_eq!(toc_width(45), 15);
        assert_eq!(toc_width(30), 0);
    }

    /// Each entry as its depth, whether it is a root, and its title, from headings given as (level, title).
    fn outline(heads: &[(u8, &str)]) -> Vec<(u8, bool, String)> {
        let heads: Vec<Heading> = heads.iter().zip(0..).map(|(&(l, t), row)| h(l, t, row)).collect();
        toc_entries(&heads).into_iter().map(|e| (e.depth, e.root, e.title)).collect()
    }

    fn ents(want: &[(u8, bool, &str)]) -> Vec<(u8, bool, String)> {
        want.iter().map(|&(d, r, t)| (d, r, t.to_string())).collect()
    }

    #[test]
    fn t03_levels() {
        // A lone first heading at the top level is the title: it shows, and the level under it gives the roots.
        assert_eq!(outline(&[(1, "T"), (2, "A"), (3, "a"), (4, "x"), (2, "B")]), ents(&[(0, false, "T"), (1, true, "A"), (2, false, "a"), (3, false, "x"), (1, true, "B")]));
        assert_eq!(outline(&[(1, "A"), (1, "B"), (2, "b"), (3, "c")]), ents(&[(0, true, "A"), (0, true, "B"), (1, false, "b"), (2, false, "c")]));
        assert_eq!(outline(&[(2, "only")]), ents(&[(0, true, "only")]));
        assert!(outline(&[]).is_empty());
    }

    #[test]
    fn every_heading_shows_in_order_when_an_h1_comes_late() {
        // The Markdown from issue #7.
        let e = outline(&[(2, "Title 2"), (3, "subsection"), (3, "subsection"), (1, "Big Title"), (2, "section"), (3, "subsection")]);
        assert_eq!(e, ents(&[(0, true, "Title 2"), (1, false, "subsection"), (1, false, "subsection"), (0, true, "Big Title"), (1, false, "section"), (2, false, "subsection")]));
    }

    #[test]
    fn odd_level_orders_keep_every_heading() {
        assert_eq!(outline(&[(3, "a"), (4, "b"), (3, "c")]), ents(&[(0, true, "a"), (1, false, "b"), (0, true, "c")]));
        assert_eq!(outline(&[(1, "A"), (4, "deep"), (1, "B")]), ents(&[(0, true, "A"), (1, false, "deep"), (0, true, "B")]));
        assert_eq!(outline(&[(2, "a"), (4, "b"), (1, "C"), (6, "d")]), ents(&[(0, true, "a"), (1, false, "b"), (0, true, "C"), (1, false, "d")]));
        assert_eq!(outline(&[(1, "A"), (1, "B"), (1, "C")]), ents(&[(0, true, "A"), (0, true, "B"), (0, true, "C")]));
        assert_eq!(outline(&[(2, "a"), (4, "b"), (3, "c")]), ents(&[(0, false, "a"), (1, true, "b"), (1, true, "c")]));
    }

    #[test]
    fn a_digit_on_a_titled_file_jumps_past_the_title() {
        let mut a = app(200, 20);
        a.set_headings(&[h(1, "T", 0), h(2, "A", 10), h(3, "a1", 20), h(2, "B", 40)]);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 10);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 20);
        a.handle(Key::Char('2'));
        assert_eq!(a.scroll, 40);
    }

    #[test]
    fn t04_digits() {
        let mut a = app(200, 20);
        a.set_headings(&[h(2, "A", 10), h(3, "a1", 20), h(3, "a2", 30), h(2, "B", 40)]);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 10);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 20);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 30);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 10);
        a.handle(Key::Char('2'));
        assert_eq!(a.scroll, 40);
        a.handle(Key::Char('9'));
        assert_eq!(a.scroll, 40);
    }

    #[test]
    fn t05_zero_reverse() {
        let mut a = app(200, 20);
        a.set_headings(&[h(2, "A", 10), h(3, "a1", 20), h(3, "a2", 30), h(2, "B", 40)]);
        a.handle(Key::Char('1'));
        a.handle(Key::Char('0'));
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 30);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 20);
    }

    #[test]
    fn t06_shift_j_k() {
        let mut a = app(200, 20);
        a.set_headings(&[h(2, "A", 10), h(3, "a1", 20), h(2, "B", 40), h(2, "C", 60)]);
        a.handle(Key::Char('J'));
        assert_eq!(a.scroll, 10);
        a.handle(Key::Char('J'));
        assert_eq!(a.scroll, 40);
        a.handle(Key::Char('J'));
        assert_eq!(a.scroll, 60);
        a.handle(Key::Char('J'));
        assert_eq!(a.scroll, 10);
        a.handle(Key::Char('K'));
        assert_eq!(a.scroll, 60);
        a.scroll = 25;
        a.pinned = None;
        a.handle(Key::Char('K'));
        assert_eq!(a.scroll, 10);
        a.scroll = 25;
        a.pinned = None;
        a.handle(Key::Char('J'));
        assert_eq!(a.scroll, 40);
    }

    #[test]
    fn t07_shift_d_u() {
        let mut a = app(200, 20);
        a.toc_list_height = 4;
        a.set_headings(&(0..10).map(|i| h(2, "x", i * 10)).collect::<Vec<_>>());
        a.toc_visible = true;
        a.handle(Key::Char('D'));
        assert_eq!((a.toc_offset, a.scroll), (2, 0));
        for _ in 0..5 {
            a.handle(Key::Char('D'));
        }
        assert_eq!(a.toc_offset, 6);
        a.handle(Key::Char('U'));
        assert_eq!(a.toc_offset, 4);
    }

    #[test]
    fn t08_current_entry() {
        let mut a = app(200, 20);
        a.set_headings(&[h(2, "A", 10), h(2, "B", 40), h(2, "C", 60)]);
        assert_eq!(a.current_entry(), Some(0));
        a.scroll = 45;
        assert_eq!(a.current_entry(), Some(1));
        a.scroll = a.max_scroll();
        assert_eq!(a.current_entry(), Some(2));
    }

    #[test]
    fn t09_jump_target() {
        let mut a = app(50, 20);
        a.set_headings(&[h(2, "A", 10), h(2, "B", 48)]);
        a.handle(Key::Char('2'));
        assert_eq!(a.scroll, a.max_scroll());
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 10);
    }

    #[test]
    fn q01_quit() {
        let mut a = app(100, 20);
        a.toc_visible = true;
        a.query = "x".into();
        assert_eq!(a.handle(Key::Char('q')), vec![Action::Quit]);
        let mut a = app(100, 20);
        a.handle(Key::Char('/'));
        assert!(!a.handle(Key::Char('q')).contains(&Action::Quit));
    }

    #[test]
    fn d1_every_occurrence() {
        let mut a = app(100, 20);
        a.query = "x".into();
        a.set_matches(vec![10, 10, 10, 30]);
        a.handle(Key::Char('n'));
        assert_eq!((a.current, a.scroll), (1, 0));
        a.handle(Key::Char('n'));
        a.handle(Key::Char('n'));
        assert_eq!((a.current, a.scroll), (3, 27));
        assert!(a.status("f").contains("4/4"));
    }

    #[test]
    fn d6_resize_debounce() {
        let t = std::time::Instant::now();
        let ms = std::time::Duration::from_millis;
        let mut d = Debounce::default();
        assert!(!d.due(t));
        d.poke(t);
        d.poke(t + ms(100));
        assert!(!d.due(t + ms(150)));
        assert!(d.due(t + ms(220)));
        assert!(!d.due(t + ms(400)));
    }

    #[test]
    fn ctrl_t_toggles_the_contents_and_ctrl_z_suspends_from_the_prompt_and_help() {
        let mut a = app(100, 20);
        a.set_headings(&[h(1, "One", 0)]);
        assert_eq!(a.handle(Key::Ctrl('t')), vec![Action::ToggleToc]);
        assert!(a.toc_visible);
        a.handle(Key::Char('/'));
        assert_eq!(a.handle(Key::Ctrl('t')), vec![]);
        assert_eq!(a.handle(Key::Ctrl('z')), vec![Action::Suspend]);
        a.handle(Key::Esc);
        a.handle(Key::Char('?'));
        assert_eq!(a.handle(Key::Ctrl('z')), vec![Action::Suspend]);
    }

    #[test]
    fn theme_list_keys() {
        let mut a = app(100, 20);
        assert_eq!(a.handle(Key::Char('T')), vec![Action::OpenThemes]);
        let two = || vec!["follow".to_string(), "github-dark".to_string()];
        a.themes = Some(super::super::themes::List::new(two(), 0, 10));
        assert_eq!(a.toc_cols(120), toc_width(120));
        assert_eq!(a.handle(Key::Char('k')), vec![]);
        assert_eq!(a.handle(Key::Char('j')), vec![Action::Status]);
        assert_eq!(a.handle(Key::Char(' ')), vec![Action::PreviewTheme]);
        assert_eq!(a.handle(Key::Char(' ')), vec![Action::Status]);
        assert_eq!(a.handle(Key::Char('t')), vec![Action::Notice("This file has no headings".into())]);
        assert_eq!(a.handle(Key::Ctrl('t')), vec![Action::Notice("This file has no headings".into())]);
        assert!(!a.handle(Key::Char('G')).is_empty() && a.themes.is_some());
        assert_eq!(a.handle(Key::Ctrl('p')), vec![Action::DropThemes, Action::Browse]);
        assert_eq!(a.handle(Key::Char('q')), vec![Action::Quit]);
        a.help = true;
        assert_eq!(a.hints()[0], QUIT);
        a.help = false;
        a.set_headings(&[h(1, "One", 0), h(2, "Two", 5)]);
        assert_eq!(keys(&mut a, &[Key::Char('['), Key::Char(' '), Key::Char('[')]), vec![Action::Status]);
        assert_eq!(a.handle(Key::Char('t')), vec![Action::DropThemes, Action::ToggleToc]);
        assert!(a.toc_visible);
        a.themes = Some(super::super::themes::List::new(two(), 1, 10));
        assert_eq!(a.handle(Key::Char('t')), vec![Action::DropThemes, Action::ToggleToc]);
        assert!(a.toc_visible, "t from the theme list hid the contents it was covering");
        a.themes = Some(super::super::themes::List::new(two(), 1, 10));
        assert_eq!(a.handle(Key::Ctrl('t')), vec![Action::DropThemes, Action::ToggleToc]);
        assert!(a.toc_visible, "Ctrl-t from the theme list hid the contents it was covering");
        a.themes = Some(super::super::themes::List::new(two(), 1, 10));
        assert_eq!(a.hints().iter().map(|h| h.2).collect::<Vec<_>>(), vec![Key::Char(' '), Key::Enter, Key::Esc]);
        assert_eq!(a.handle(Key::Enter), vec![Action::PickTheme]);
        assert_eq!(a.handle(Key::Char('T')), vec![Action::CloseThemes]);
        assert_eq!(a.handle(Key::Esc), vec![Action::CloseThemes]);
        assert_eq!(a.theme_hover(TOC_HEADER), vec![Action::Status]);
        assert_eq!(a.themes.as_ref().unwrap().sel, 0);
        assert_eq!(a.theme_click(TOC_HEADER + 5), vec![]);
        assert_eq!(a.theme_click(TOC_HEADER + 1), vec![Action::Status]);
        assert_eq!(a.theme_click(TOC_HEADER), vec![Action::PreviewTheme]);
        assert_eq!(a.themes.as_ref().unwrap().shown, 0);
    }

    #[test]
    fn w01_wrap_toggle() {
        let mut a = app(100, 20);
        a.scroll = 7;
        assert!(!a.wrap);
        assert_eq!(a.handle(Key::Char('w'))[0], Action::ToggleWrap);
        assert!(a.wrap);
        assert_eq!(a.scroll, 7);
        a.handle(Key::Char('w'));
        assert!(!a.wrap);
        a.handle(Key::Char('/'));
        a.handle(Key::Char('w'));
        assert!(!a.wrap);
        assert_eq!(a.prompt.as_deref(), Some("w"));
    }

    #[test]
    fn w02_side_wheel() {
        let mut a = app(100, 20);
        a.scroll = 7;
        assert_eq!(a.handle(Key::WheelRight(12, 5)), vec![Action::ScrollCode(12, 5, CODE_WHEEL)]);
        assert_eq!(a.handle(Key::WheelLeft(3, 9)), vec![Action::ScrollCode(3, 9, -CODE_WHEEL)]);
        assert_eq!(a.scroll, 7);
        a.handle(Key::Char('/'));
        assert_eq!(a.handle(Key::WheelRight(1, 1)), vec![Action::ScrollCode(1, 1, CODE_WHEEL)]);
        assert_eq!(a.handle(Key::WheelDown(1, 1)), vec![Action::Scroll]);
        assert_eq!(a.scroll, 10);
    }

    #[test]
    fn toc_click_jumps_to_entry() {
        let mut a = app(100, 20);
        a.set_headings(&[h(1, "A", 0), h(2, "B", 30), h(1, "C", 60)]);
        assert!(a.toc_click(2).is_empty());
        assert!(a.toc_click(6).is_empty());
        assert_eq!(a.toc_click(4), vec![Action::Scroll, Action::Status]);
        assert_eq!(a.scroll, 30);
        assert_eq!(a.current_entry(), Some(1));
        a.toc_offset = 2;
        a.toc_click(3);
        assert_eq!(a.scroll, 60);
    }

    #[test]
    fn e00_edit_key() {
        let mut a = app(100, 20);
        assert_eq!(a.handle(Key::Char('e')), vec![Action::Editor]);
        assert_eq!(a.handle(Key::Char('c')), vec![Action::Copy]);
        a.handle(Key::Ctrl('e'));
        assert_eq!(a.scroll, 1);
        a.handle(Key::Ctrl('y'));
        assert_eq!(a.scroll, 0);
    }

    #[test]
    fn cmd_c_copies_the_selection_in_any_mode() {
        let mut a = app(100, 20);
        assert_eq!(a.handle(Key::Copy), vec![Action::CopySelection]);
        a.help = true;
        assert_eq!(a.handle(Key::Copy), vec![Action::CopySelection]);
        assert!(a.help);
        a.help = false;
        a.prompt = Some("ab".into());
        assert_eq!(a.handle(Key::Copy), vec![Action::CopySelection]);
        assert_eq!(a.prompt.as_deref(), Some("ab"));
    }

    #[test]
    fn vim_aliases() {
        let mut a = app(200, 20);
        a.set_headings(&[h(2, "A", 10), h(2, "B", 40), h(2, "C", 60)]);
        assert!(a.handle(Key::Char(']')).is_empty());
        a.handle(Key::Char(']'));
        assert_eq!(a.scroll, 10);
        keys(&mut a, &[Key::Char(']'), Key::Char(']')]);
        assert_eq!(a.scroll, 40);
        keys(&mut a, &[Key::Char('['), Key::Char('[')]);
        assert_eq!(a.scroll, 10);
        keys(&mut a, &[Key::Char(']'), Key::Char('j')]);
        assert_eq!(a.scroll, 11);
        a.handle(Key::Ctrl('o'));
        assert_eq!(a.scroll, 40);
        keys(&mut a, &[Key::Char('g'), Key::Char('g')]);
        assert_eq!(a.scroll, 0);
        assert_eq!(a.handle(Key::Char('e')), vec![Action::Editor]);
    }

    #[test]
    fn help_scrolls_by_the_page_s_steps() {
        let mut a = app(100, 20);
        a.handle(Key::Char('?'));
        keys(&mut a, &[Key::Char('j'), Key::WheelDown(1, 1), Key::Char('d'), Key::Char(' ')]);
        assert_eq!(a.help_top, 1 + WHEEL + 10 + 18);
        keys(&mut a, &[Key::Ctrl('b'), Key::Char('u')]);
        assert!(a.help);
        assert_eq!(a.help_top, 1 + WHEEL);
    }

    #[test]
    fn help_stays_open_on_a_sideways_wheel() {
        let mut a = app(100, 20);
        a.handle(Key::Char('?'));
        assert_eq!(a.handle(Key::WheelLeft(1, 1)), vec![]);
        assert_eq!(a.handle(Key::WheelRight(1, 1)), vec![]);
        assert!(a.help && a.help_top == 0);
    }

    #[test]
    fn expanded_diagram_closes_on_any_key() {
        let mut a = app(100, 20);
        a.expanded = true;
        assert_eq!(a.handle(Key::WheelDown(1, 1)), vec![]);
        assert!(a.expanded && a.scroll == 0);
        assert_eq!(a.handle(Key::Char('j')), vec![Action::Collapse]);
        assert!(!a.expanded && a.scroll == 0);
        a.expanded = true;
        assert_eq!(a.handle(Key::Char('q')), vec![Action::Quit]);
        a.prompt = Some(String::new());
        assert_eq!(a.handle(Key::Esc), vec![Action::Collapse]);
        assert!(!a.expanded && a.prompt.as_deref() == Some(""));
    }

    #[test]
    fn help_closes_only_on_its_hinted_keys() {
        let mut a = app(100, 20);
        assert_eq!(a.handle(Key::Char('?')), vec![Action::Help]);
        assert!(a.help);
        assert_eq!(a.handle(Key::Char('j')), vec![Action::Help]);
        assert!(a.help && a.help_top == 1);
        for k in [Key::Char('x'), Key::Char('/'), Key::Enter] {
            assert_eq!(a.handle(k), vec![], "{k:?}");
            assert!(a.help && a.help_top == 1 && a.prompt.is_none(), "{k:?}");
        }
        assert_eq!(a.handle(Key::Char('?')), vec![Action::Help]);
        assert!(!a.help && a.scroll == 0);
        a.handle(Key::Char('?'));
        assert_eq!(a.handle(Key::Esc), vec![Action::Help]);
        assert!(!a.help);
        a.handle(Key::Char('?'));
        assert_eq!(a.handle(Key::Char('q')), vec![Action::Quit]);
    }

    #[test]
    fn backspace_returns_from_a_jump() {
        let mut a = app(200, 20);
        a.set_headings(&[h(2, "A", 10), h(2, "B", 80)]);
        a.scroll = 5;
        a.handle(Key::Char('2'));
        assert_eq!(a.scroll, 80);
        a.handle(Key::Char('1'));
        assert_eq!(a.scroll, 10);
        a.handle(Key::Backspace);
        assert_eq!(a.scroll, 80);
        a.handle(Key::Backspace);
        assert_eq!(a.scroll, 5);
        assert!(matches!(a.handle(Key::Backspace)[..], [Action::Notice(_)]));
    }

    #[test]
    fn contents_follow_the_page() {
        let mut a = app(400, 20);
        a.toc_list_height = 4;
        a.set_headings(&(0..12).map(|i| h(2, "x", i * 30)).collect::<Vec<_>>());
        a.handle(Key::Char('G'));
        assert!(a.toc_offset + a.toc_list_height > 11);
        keys(&mut a, &[Key::Char('g'), Key::Char('g')]);
        assert_eq!(a.toc_offset, 0);
        for _ in 0..7 {
            a.handle(Key::Char('J'));
        }
        assert_eq!(a.scroll, 210);
        assert!((a.toc_offset..a.toc_offset + 4).contains(&7));
    }

    fn long_toc(n: u32, height: u32) -> App {
        let mut a = app(n * 30 + 100, 20);
        a.set_headings(&(0..n).map(|i| h(2, "x", i * 30)).collect::<Vec<_>>());
        (a.toc_visible, a.toc_list_height) = (true, height);
        a
    }

    #[test]
    fn toc_thumb_spans_the_track_and_hides_when_all_fits() {
        assert_eq!(long_toc(4, 4).toc_thumb(), None);
        for (n, height) in [(5, 4), (6, 4), (7, 5), (8, 6), (12, 4), (60, 10), (200, 7), (3, 1)] {
            let mut a = long_toc(n, height);
            let max = n - height;
            let mut last = 0;
            for off in 0..=max {
                a.toc_offset = off;
                let (top, len) = a.toc_thumb().unwrap();
                assert!(len >= 1 && top + len <= height && top >= last, "{n} {height} {off}: {top} {len}");
                let space = height - len;
                assert!(height < 3 || space >= 2, "{n} {height}: {len}");
                match off {
                    0 => assert_eq!(top, 0),
                    o if o == max => assert_eq!(top, space),
                    _ if space >= 2 => assert!(top > 0 && top < space, "{n} {height} {off}: {top}"),
                    _ => {}
                }
                last = top;
            }
        }
        assert_eq!(long_toc(40, 10).toc_thumb(), Some((0, 3)));
    }

    #[test]
    fn toc_wheel_scrolls_the_list_within_bounds() {
        assert_eq!(long_toc(4, 4).toc_wheel(true), None);
        let mut a = long_toc(12, 4);
        assert_eq!(a.toc_wheel(true), Some(vec![Action::Status]));
        assert_eq!((a.toc_offset, a.scroll), (WHEEL, 0));
        a.toc_wheel(true);
        a.toc_wheel(true);
        assert_eq!(a.toc_offset, 8);
        a.toc_wheel(false);
        assert_eq!(a.toc_offset, 8 - WHEEL);
        for _ in 0..5 {
            a.toc_wheel(false);
        }
        assert_eq!(a.toc_offset, 0);
    }

    #[test]
    fn toc_bar_drag_moves_the_list_and_track_press_centers_the_thumb() {
        let mut a = long_toc(40, 10);
        let line = |row: u32| TOC_HEADER + row;
        assert_eq!(a.toc_bar_press(0, line(10)), None);
        assert_eq!(a.toc_bar_press(0, 1), None);
        assert!(!a.toc_bar_held());
        assert_eq!(a.toc_bar_press(0, line(1)), Some(vec![]));
        assert_eq!(a.toc_offset, 0);
        a.toc_bar_drag(line(4));
        assert_eq!(a.toc_thumb(), Some((3, 3)));
        a.toc_bar_drag(line(40));
        assert_eq!((a.toc_offset, a.toc_thumb()), (30, Some((7, 3))));
        a.toc_bar_drag(0);
        assert_eq!(a.toc_offset, 0);
        a.toc_offset = 3;
        a.toc_bar_press(0, line(1));
        assert_eq!((a.toc_bar_drag(line(1)), a.toc_offset), (vec![], 3));
        a.toc_bar_release();
        assert!(!a.toc_bar_held());
        assert_eq!(a.toc_bar_drag(line(5)), vec![]);
        assert_eq!(a.toc_bar_press(0, line(8)), Some(vec![Action::Status]));
        assert_eq!(a.toc_thumb(), Some((7, 3)));
        assert_eq!(a.toc_bar_press(0, line(0)).map(|_| a.toc_offset), Some(0));
        assert_eq!(a.scroll, 0);
    }

    #[test]
    fn toc_bar_presses_hit_its_three_columns_only_while_the_list_overflows() {
        let mut a = long_toc(40, 10);
        let line = TOC_HEADER + 5;
        for from_edge in 0..TOC_BAR {
            a.toc_offset = 0;
            assert_eq!(a.toc_bar_press(from_edge, line), Some(vec![Action::Status]), "{from_edge}");
            a.toc_bar_release();
        }
        assert_eq!(a.toc_bar_press(TOC_BAR, line), None);
        let mut a = long_toc(4, 4);
        assert!((0..=TOC_BAR).all(|c| a.toc_bar_press(c, TOC_HEADER).is_none()));
    }

    #[test]
    fn toc_thumb_follows_every_way_the_list_moves() {
        let mut a = long_toc(40, 10);
        a.handle(Key::Char('D'));
        assert_eq!(a.toc_thumb(), Some((1, 3)));
        a.handle(Key::Char('U'));
        assert_eq!(a.toc_thumb(), Some((0, 3)));
        a.handle(Key::Char('G'));
        assert_eq!(a.toc_thumb(), Some((7, 3)));
        a.layout(a.viewport, a.doc_rows, a.scroll, &(0..40).map(|i| h(2, "x", i * 30)).collect::<Vec<_>>());
        assert_eq!(a.toc_list_height, 17);
        assert_eq!(a.toc_thumb(), Some((10, 7)));
    }

    #[test]
    fn h01_hints_follow_context() {
        let mut a = app(100, 20);
        let keys_of = |a: &App| a.hints().iter().map(|h| h.0).collect::<Vec<_>>();
        assert_eq!(keys_of(&a), ["?", "/", "e"]);
        a.set_headings(&[h(1, "A", 0)]);
        assert_eq!(keys_of(&a), ["?", "/", "t", "e"]);
        a.handle(Key::Char('/'));
        assert_eq!(keys_of(&a), ["Enter", "Esc"]);
        a.handle(Key::Char('x'));
        a.handle(Key::Enter);
        a.set_matches(vec![3]);
        assert_eq!(keys_of(&a), ["q", "n/N", "Esc", "?"]);
    }

    #[test]
    fn layout_clamps_the_scroll_and_the_contents() {
        let mut a = app(0, 0);
        a.toc_offset = 50;
        a.layout(10, 20, 99, &(0..12).map(|i| h(2, "x", i * 2)).collect::<Vec<_>>());
        assert_eq!((a.scroll, a.toc_list_height, a.toc_offset, a.entries.len()), (15, 7, 5, 12));
    }

    #[test]
    fn a_click_keeps_help_open_and_closes_the_expanded_diagram() {
        let mut a = app(100, 20);
        assert_eq!(a.modal_click(true, false), None);
        a.help = true;
        assert_eq!(a.modal_click(false, false), Some(vec![]));
        assert_eq!(a.modal_click(true, false), Some(vec![]));
        assert!(a.help);
        assert_eq!(a.modal_click(true, true), None);
        a.help = false;
        a.expanded = true;
        assert_eq!(a.modal_click(false, false), Some(vec![]));
        assert!(a.expanded);
        assert_eq!(a.modal_click(true, false), Some(vec![Action::Collapse]));
        assert!(!a.expanded);
    }

    #[test]
    fn jump_keys_glide_and_row_keys_do_not() {
        for k in [Key::Char('d'), Key::Ctrl('d'), Key::Char(' '), Key::PageDown, Key::Char('G'), Key::End] {
            assert_eq!(app(100, 20).handle(k), vec![Action::Glide], "{k:?}");
        }
        for k in [Key::Char('j'), Key::Down, Key::Ctrl('e'), Key::WheelDown(1, 1)] {
            assert_eq!(app(100, 20).handle(k), vec![Action::Scroll], "{k:?}");
        }
        let mut a = app(100, 20);
        keys(&mut a, &[Key::Char('/'), Key::Char('x'), Key::Enter]);
        a.set_matches(vec![30, 60]);
        assert!(a.handle(Key::Char('n')).contains(&Action::Scroll));
    }

    const GLIDE: std::time::Duration = std::time::Duration::from_millis(150);

    fn glide(from: u32, to: u32, span: u32, now: std::time::Instant) -> Glide {
        Glide::new(from, to, span, Pace::Fixed(GLIDE), now)
    }

    #[test]
    fn a_glide_eases_out_and_ends_on_time() {
        let t0 = std::time::Instant::now();
        let g = glide(0, 1000, 5000, t0);
        assert_eq!(g.at(t0), (0, false));
        let (mid, done) = g.at(t0 + GLIDE / 2);
        // Eased out: past halfway at half the time.
        assert!(!done && mid > 500 && mid < 1000, "{mid}");
        assert_eq!(g.at(t0 + GLIDE), (1000, true));
        assert_eq!(g.at(t0 + GLIDE * 3), (1000, true));
        let (back, done) = glide(1000, 0, 5000, t0).at(t0 + GLIDE / 2);
        assert!(!done && back == 1000 - mid, "{back}");
        assert_eq!(Glide::new(0, 1000, 5000, Pace::Fixed(std::time::Duration::ZERO), t0).at(t0), (1000, true));
    }

    #[test]
    fn a_glide_takes_its_share_of_the_page_time() {
        let t0 = std::time::Instant::now();
        let per_page = std::time::Duration::from_millis(200);
        let pace = Pace::Page { per_page, page: 800 };
        assert_eq!(Glide::new(0, 400, 800, pace, t0).time, per_page / 2);
        assert_eq!(Glide::new(0, 800, 800, pace, t0).time, per_page);
        assert_eq!(Glide::new(0, 9000, 800, pace, t0).time, per_page, "a long jump glides one page");
    }

    #[test]
    fn a_jump_soon_after_another_glides_fast() {
        let t0 = std::time::Instant::now();
        let per_page = std::time::Duration::from_millis(200);
        let page = Pace::Page { per_page, page: 800 };
        assert_eq!(Pace::for_jump(None, t0, per_page, 800), page);
        assert_eq!(Pace::for_jump(Some(t0), t0 + REPEAT / 2, per_page, 800), Pace::Fixed(FAST_GLIDE));
        assert_eq!(Pace::for_jump(Some(t0), t0 + REPEAT, per_page, 800), page);
    }

    #[test]
    fn a_long_glide_covers_only_its_last_span() {
        let t0 = std::time::Instant::now();
        assert_eq!(glide(0, 9000, 800, t0).from(), 8200);
        assert_eq!(glide(9000, 100, 800, t0).from(), 900);
        assert_eq!(glide(500, 100, 800, t0).from(), 500);
    }

    #[test]
    fn a_new_jump_mid_glide_starts_from_where_the_view_is() {
        let t0 = std::time::Instant::now();
        let g = glide(0, 1000, 5000, t0);
        let now = t0 + GLIDE / 2;
        let g2 = glide(g.at(now).0, 2000, 5000, now);
        assert_eq!(g2.at(now), (g.at(now).0, false));
        assert_eq!(g2.at(now + GLIDE), (2000, true));
    }

    const HELP_SKIPPED: &[&str] = &["wheel", "Shift-wheel", "drag its bar", "click a link", "click a button", "drag", "double-click", "click a hint", "click an entry"];

    fn help_keys(name: &str) -> Option<Vec<Key>> {
        if HELP_SKIPPED.contains(&name) {
            return None;
        }
        let one = |k| Some(vec![k]);
        match name {
            "↓" => one(Key::Down),
            "↑" => one(Key::Up),
            "←" => one(Key::Left),
            "→" => one(Key::Right),
            "Space" => one(Key::Char(' ')),
            "PgDn" => one(Key::PageDown),
            "PgUp" => one(Key::PageUp),
            "Home" => one(Key::Home),
            "End" => one(Key::End),
            "Backspace" => one(Key::Backspace),
            "Enter" => one(Key::Enter),
            "Esc" => one(Key::Esc),
            "Cmd-c" | "Super-c" => one(Key::Copy),
            "gg" | "]]" | "[[" => {
                let c = name.chars().next().unwrap();
                Some(vec![Key::Char(c), Key::Char(c)])
            }
            _ => {
                if let Some(c) = name.strip_prefix("Ctrl-") {
                    return one(Key::Ctrl(c.chars().next().unwrap()));
                }
                let mut cs = name.chars();
                match (cs.next(), cs.next()) {
                    (Some(c), None) => one(Key::Char(c)),
                    _ => panic!("help names {name:?}, which this test neither maps nor skips"),
                }
            }
        }
    }

    fn help_app() -> App {
        let mut a = app(200, 20);
        a.scroll = 50;
        a.toc_visible = true;
        a.query = "x".into();
        a.matches = vec![10, 60, 100];
        a.back = vec![3];
        let heads: Vec<Heading> = (0..12).map(|i| h(1, "H", i * 15)).collect();
        a.set_headings(&heads);
        a
    }

    fn fingerprint(a: &App) -> String {
        format!(
            "{} {} {} {} {:?} {} {} {} {:?} {:?} {} {:?} {}",
            a.scroll,
            a.wrap,
            a.toc_visible,
            a.toc_offset,
            a.prompt,
            a.query,
            a.help,
            a.current,
            a.prefix,
            a.numkey,
            a.reverse,
            a.pinned,
            a.back.len()
        )
    }

    #[test]
    fn every_key_the_help_screen_lists_does_something() {
        let mut unbound = vec![];
        for name in super::super::keys::SECTIONS.iter().flat_map(|s| s.rows).flat_map(|r| r.keys.iter().copied()) {
            if name == "1-9" {
                for c in '1'..='9' {
                    let mut a = help_app();
                    let before = fingerprint(&a);
                    if a.handle(Key::Char(c)).is_empty() && fingerprint(&a) == before {
                        unbound.push(c.to_string());
                    }
                }
                continue;
            }
            let Some(ks) = help_keys(name) else { continue };
            let mut contexts = vec![help_app()];
            if matches!(name, "Ctrl-u" | "Ctrl-w") {
                let mut p = help_app();
                p.prompt = Some("ab cd".into());
                if name == "Ctrl-w" {
                    contexts.clear();
                }
                contexts.push(p);
            }
            for mut a in contexts {
                let before = fingerprint(&a);
                let acted = ks.iter().flat_map(|k| a.handle(*k)).count() > 0 || fingerprint(&a) != before;
                if !acted {
                    unbound.push(name.to_string());
                }
            }
        }
        assert!(unbound.is_empty(), "help lists keys that do nothing: {unbound:?}");
    }

    #[test]
    fn viewer_events_move_app_state() {
        let mut a = app(100, 20);
        assert!(a.file_lost());
        assert!(!a.file_lost());
        assert!(a.file_found() && !a.file_found());
        a.set_expanded(true);
        assert!(a.collapse() && !a.collapse() && !a.expanded);
        a.show_notice("hi", OnClick::Quit);
        assert!(a.clear_notice() && !a.clear_notice());
        assert!(!a.void_labels());
        a.start_labels(3);
        assert!(a.void_labels() && a.labels.is_some());
        a.query = "x".into();
        a.set_matches(vec![10, 30, 50]);
        a.current = 2;
        a.relocate_matches(vec![12, 32, 52]);
        assert_eq!(a.current, 2);
        a.relocate_matches(vec![12]);
        assert_eq!(a.current, 0);
    }

    #[test]
    fn replace_keeps_settings_across_files() {
        let mut a = app(100, 20);
        a.wrap = true;
        a.toc_visible = true;
        a.open_themes(vec!["a".into(), "b".into()], 1);
        a.show_notice("old", OnClick::Hints);
        let old = a.replace(App::default());
        assert!(a.wrap && a.toc_visible && a.themes.is_some() && a.notice.is_none());
        assert!(old.notice.is_some() && old.themes.is_none());
        a.set_back_file(true);
        assert_eq!(a.handle(Key::Backspace), vec![Action::BackFile]);
        a.hide_toc();
        assert!(!a.toc_visible);
        assert!(a.close_themes().is_some() && a.themes.is_none());
    }
}
