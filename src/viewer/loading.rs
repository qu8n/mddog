use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::term::{self, Size};
use crate::theme::{self, Rgb, Theme};
use crate::{config, trace};

const STEP: Duration = Duration::from_millis(100);
/// A page ready sooner shows without a skeleton. Warm starts are ready in about 40 to 75 ms.
const DELAY: Duration = Duration::from_millis(100);
/// Once drawn, the skeleton stays at least this long, so it never flashes.
const MIN_SHOWN: Duration = Duration::from_millis(100);
/// Frames in one shimmer sweep.
const SWEEP: usize = 14;
/// The page's padding in CSS pixels, as in page.rs.
const PAD_CSS: f64 = 24.0;

/// The colors and margins of a skeleton, taken from the page it stands in for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub bg: Rgb,
    /// The theme's `--mddog-skeleton` minus its `--mddog-bg`.
    pub lift: (f64, f64, f64),
    pub bar_bg: Rgb,
    pub muted: Rgb,
    pub pad_cols: u16,
    pub pad_rows: u16,
}

impl Look {
    /// `cell` is the cell size in device pixels.
    pub fn new(t: &Theme, scale: f64, cell: (u32, u32)) -> Look {
        let bg = t.palette.bg;
        let d = |a: u8, b: u8| f64::from(a) - f64::from(b);
        let s = t.palette.skeleton;
        let pad = |px: u32| (PAD_CSS * scale / f64::from(px.max(1))).round() as u16;
        Look { dark: t.dark, bg, lift: (d(s.0, bg.0), d(s.1, bg.1), d(s.2, bg.2)), bar_bg: t.palette.bar_bg, muted: t.palette.fg_muted, pad_cols: pad(cell.0), pad_rows: pad(cell.1) }
    }

    fn mix(&self, k: f64) -> Rgb {
        let m = |c: u8, l: f64| (f64::from(c) + l * k).round().clamp(0.0, 255.0) as u8;
        (m(self.bg.0, self.lift.0), m(self.bg.1, self.lift.1), m(self.bg.2, self.lift.2))
    }

    fn bar(&self) -> Rgb {
        self.mix(1.0)
    }

    /// The shimmer: past the bars on a dark page, between the bars and the page on a light one.
    fn shine(&self) -> Rgb {
        self.mix(if self.dark { 2.0 } else { 0.25 })
    }

    fn code(&self) -> Rgb {
        self.mix(if self.dark { 0.35 } else { 0.375 })
    }
}

#[derive(Default)]
struct Tune {
    scale: Option<f64>,
    cell: Option<(u32, u32)>,
}

/// How far the first frame has got.
#[derive(Clone, Copy)]
enum Shown {
    No,
    Writing,
    Since(Instant),
}

struct Shared {
    stop: AtomicBool,
    drawn: Mutex<Shown>,
    tune: Mutex<Tune>,
}

/// Draws a skeleton of a page on the terminal until it is dropped, if the page takes longer than
/// `DELAY` to show.
pub struct Loader {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Loader {
    pub fn start(fd: i32, name: &str) -> Loader {
        let shared = Arc::new(Shared { stop: AtomicBool::new(false), drawn: Mutex::new(Shown::No), tune: Mutex::new(Tune::default()) });
        let s = shared.clone();
        let name = term::printable(name);
        let thread = std::thread::spawn(move || {
            let shown = Instant::now() + DELAY;
            while !s.stop.load(Ordering::Acquire) && Instant::now() < shown {
                std::thread::park_timeout(shown.saturating_duration_since(Instant::now()));
            }
            let fd = unsafe { libc::dup(fd) };
            if fd < 0 {
                return;
            }
            let mut out = unsafe { File::from_raw_fd(fd) };
            let theme = config::theme();
            // A terminal too small to draw in waits, so the minimum time starts at a visible frame.
            let mut last = None;
            for i in 0.. {
                let size = term::size_of(fd);
                if size.cols == 0 || size.rows < 2 {
                    if s.stop.load(Ordering::Acquire) {
                        break;
                    }
                    std::thread::park_timeout(Duration::from_millis(5));
                    continue;
                }
                {
                    // Checked under the lock `finish` takes, so a frame is either never drawn or waited for.
                    let mut drawn = s.drawn.lock().unwrap();
                    if s.stop.load(Ordering::Acquire) {
                        break;
                    }
                    if matches!(*drawn, Shown::No) {
                        *drawn = Shown::Writing;
                    }
                }
                let look = {
                    let t = s.tune.lock().unwrap();
                    let scale = t.scale.unwrap_or(2.0);
                    let cell = t.cell.or_else(|| size.cell_px(scale)).unwrap_or(((8.0 * scale) as u32, (16.0 * scale) as u32));
                    Look::new(theme, scale, cell)
                };
                let bytes = frame(i, &name, size, &look, last != Some((size, look)));
                {
                    let _held = term::WRITE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let _ = out.write_all(bytes.as_bytes());
                }
                if last.is_none() {
                    // The minimum time counts from when the frame is out, not when it was started.
                    *s.drawn.lock().unwrap() = Shown::Since(Instant::now());
                    trace::mark("skeleton drawn");
                }
                last = Some((size, look));
                std::thread::park_timeout(STEP);
            }
        });
        Loader { shared, thread: Some(thread) }
    }

    /// Gives the skeleton what the terminal probe found: the screen scale and the cell size in
    /// device pixels.
    pub fn tune(&self, scale: f64, cell: (u32, u32)) {
        *self.shared.tune.lock().unwrap() = Tune { scale: Some(scale), cell: Some(cell) };
    }

    /// Stops the skeleton, after it has been up for `MIN_SHOWN` if it was drawn.
    pub fn finish(self) {
        {
            let d = self.shared.drawn.lock().unwrap();
            if matches!(*d, Shown::No) {
                self.shared.stop.store(true, Ordering::Release);
                return;
            }
        }
        loop {
            let left = match *self.shared.drawn.lock().unwrap() {
                Shown::Since(at) => (at + MIN_SHOWN).saturating_duration_since(Instant::now()),
                _ => Duration::from_millis(1),
            };
            if left.is_zero() || self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
                break;
            }
            std::thread::sleep(left);
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
        if matches!(*self.shared.drawn.lock().unwrap(), Shown::Since(_)) {
            trace::mark("skeleton cleared");
        }
    }
}

/// One row of the skeleton page.
#[derive(Clone, Copy)]
enum Line {
    Gap,
    /// A heading as wide as this percent of the page.
    Head(u16),
    /// A line of text.
    Text(u16),
    /// A line in a code block; 0 is a padding row.
    Code(u16),
}

use Line::*;

const PAGE: &[Line] = &[Head(42), Gap, Text(100), Text(96), Text(58), Gap, Text(100), Text(88), Text(34), Gap];
const REPEAT: &[Line] = &[Code(0), Code(30), Code(54), Code(46), Code(20), Code(0), Gap, Head(26), Gap, Text(100), Text(97), Text(92), Text(66), Gap, Text(100), Text(48), Gap];

/// Draws frame `i` of the skeleton for a terminal of `size`, with the status row at the bottom.
/// `full` clears the screen first; later frames redraw only the bars.
pub fn frame(i: usize, name: &str, size: Size, look: &Look, full: bool) -> String {
    let cols = usize::from(size.cols);
    if cols == 0 || size.rows < 2 {
        return String::new();
    }
    let sgr = |kind: u8, c: Rgb| if kind == 4 { theme::bg(c) } else { theme::fg(c) };
    let mut out = String::from("\x1b[?2026h");
    if full {
        let (line, w) = term::fit(&format!(" {name} "), cols);
        out.push_str(&format!("\x1b[0m{}\x1b[2J\x1b[{};1H{}{}{line}{}\x1b[0m", sgr(4, look.bg), size.rows, sgr(4, look.bar_bg), sgr(3, look.muted), " ".repeat(cols - w)));
    }
    let pad = usize::from(look.pad_cols).min(cols / 4);
    let width = cols - 2 * pad;
    let span = width + 2 * width / 3;
    let lead = (i % SWEEP) * span / SWEEP;
    let (bar, shine, code) = (look.bar(), look.shine(), look.code());
    let first = usize::from(look.pad_rows).min(usize::from(size.rows) / 4) + 1;
    for (row, line) in (first..usize::from(size.rows)).zip(PAGE.iter().chain(REPEAT.iter().cycle())) {
        let (pct, glyph, ground) = match *line {
            Gap => continue,
            Head(p) => (p, '█', look.bg),
            Text(p) => (p, '▀', look.bg),
            Code(p) => (p, '▀', code),
        };
        let inset = if matches!(line, Code(_)) { 2.min(width / 4) } else { 0 };
        let len = (width - 2 * inset) * usize::from(pct) / 100;
        out.push_str(&format!("\x1b[{row};{}H{}", pad + 1, sgr(4, ground)));
        if matches!(line, Code(_)) {
            out.push_str(&" ".repeat(inset));
        }
        let mut level = usize::MAX;
        for x in 0..len {
            let d = (inset + x + row).abs_diff(lead);
            let l = 3usize.saturating_sub(d * 3 / (width / 6).max(1));
            if l != level {
                level = l;
                let k = l as f64 / 3.0;
                let m = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * k).round() as u8;
                out.push_str(&sgr(3, (m(bar.0, shine.0), m(bar.1, shine.1), m(bar.2, shine.2))));
            }
            out.push(glyph);
        }
        if matches!(line, Code(_)) {
            out.push_str(&" ".repeat(width - inset - len));
        }
    }
    out.push_str("\x1b[0m\x1b[?2026l");
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const SIZE: Size = Size { cols: 40, rows: 11, xpix: 0, ypix: 0 };
    fn look(name: &str) -> Look {
        Look::new(theme::builtin(name).unwrap(), 2.0, (16, 32))
    }

    static DARK: std::sync::LazyLock<Look> = std::sync::LazyLock::new(|| Look { pad_rows: 1, ..look("github-dark") });

    #[test]
    fn look_follows_the_theme_and_the_page_padding() {
        let dark = look("github-dark");
        assert_eq!((dark.pad_cols, dark.pad_rows), (3, 2));
        assert_eq!(dark.bar(), (37, 41, 46));
        let light = look("github-light");
        assert_eq!(light.bg, (255, 255, 255));
        assert_eq!(light.bar(), theme::builtin("github-light").unwrap().palette.skeleton);
        assert!(light.bar() < light.bg && light.shine() > light.bar());
        // A darker page keeps the bars as far above it.
        let lower = Look { bg: (3, 7, 13), ..dark };
        assert_eq!(lower.bar(), (27, 31, 36));
    }

    #[test]
    fn a_light_theme_draws_no_dark_cells() {
        let light = theme::builtin("github-light").unwrap().palette;
        let f = frame(3, "a.md", Size { rows: 30, ..SIZE }, &look("github-light"), true);
        assert!(f.contains(&theme::bg(light.bar_bg)) && f.contains(&theme::fg(light.fg_muted)));
        for c in sgr_colors(&f) {
            assert!(c.0 > 80 && c.1 > 80 && c.2 > 80 || c == light.fg_muted, "dark color {c:?} in {f:?}");
        }
    }

    /// Every truecolor in `s`.
    pub fn sgr_colors(s: &str) -> Vec<Rgb> {
        let re = regex::Regex::new(r"\x1b\[[34]8;2;(\d+);(\d+);(\d+)m").unwrap();
        re.captures_iter(s).map(|c| (c[1].parse().unwrap(), c[2].parse().unwrap(), c[3].parse().unwrap())).collect()
    }

    #[test]
    fn full_frame_paints_the_page_and_the_status_row() {
        let f = frame(0, "a.md", SIZE, &DARK, true);
        assert!(f.starts_with("\x1b[?2026h\x1b[0m\x1b[48;2;13;17;23m\x1b[2J\x1b[11;1H"));
        assert!(f.contains(&format!(" a.md {}", " ".repeat(34))));
        assert!(f.ends_with("\x1b[0m\x1b[?2026l"));
        assert!(!frame(0, "a.md", SIZE, &DARK, false).contains("a.md"));
    }

    #[test]
    fn lays_out_a_heading_text_and_a_code_block_inside_the_padding() {
        let f = frame(SWEEP - 1, "a.md", Size { rows: 30, ..SIZE }, &DARK, false);
        // The heading sits below the top padding, 42% of the 34 columns between the margins.
        assert!(f.contains(&format!("\x1b[2;4H\x1b[48;2;13;17;23m\x1b[38;2;37;41;46m{}", "█".repeat(14))));
        assert!(f.contains(&format!("\x1b[4;4H\x1b[48;2;13;17;23m\x1b[38;2;37;41;46m{}", "▀".repeat(34))));
        // Row 3 is a gap, and the code block starts on row 12 with a padding row.
        assert!(!f.contains("\x1b[3;4H"));
        let code = format!("\x1b[12;4H\x1b[48;2;21;25;31m  {}", " ".repeat(32));
        assert!(f.contains(&code), "{f:?}");
        assert!(!f.contains("\x1b[30;"));
    }

    #[test]
    fn the_shimmer_moves() {
        let a = frame(0, "a.md", SIZE, &DARK, false);
        let b = frame(3, "a.md", SIZE, &DARK, false);
        assert_ne!(a, b);
        assert_eq!(a, frame(SWEEP, "a.md", SIZE, &DARK, false));
        assert!(b.contains("\x1b[38;2;61;65;69m"));
    }

    #[test]
    fn tiny_terminal_draws_nothing() {
        assert_eq!(frame(0, "a.md", Size { cols: 0, rows: 0, xpix: 0, ypix: 0 }, &DARK, true), "");
        assert_eq!(frame(0, "a.md", Size { cols: 40, rows: 1, xpix: 0, ypix: 0 }, &DARK, true), "");
        assert!(frame(0, "a.md", Size { cols: 2, rows: 3, xpix: 0, ypix: 0 }, &DARK, true).contains(" a"));
    }
}
