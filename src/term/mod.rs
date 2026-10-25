pub mod kitty;

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthStr;

/// Held for each whole write to the terminal, so writes from two threads never mix their bytes.
pub static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub struct Tty {
    out: File,
    saved: Option<libc::termios>,
    pub fd: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub xpix: u16,
    pub ypix: u16,
}

impl Size {
    /// The cell in pixels. Warp fills the window's pixel size in points, so a cell under 7 px wide per unit of
    /// `scale` (no real font is that small) is taken as points and scaled up.
    pub fn cell_px(&self, scale: f64) -> Option<(u32, u32)> {
        let (cols, rows) = (u32::from(self.cols), u32::from(self.rows));
        let (mut x, mut y) = (u32::from(self.xpix), u32::from(self.ypix));
        if cols > 0 && f64::from(x / cols) < 7.0 * scale {
            (x, y) = ((f64::from(x) * scale) as u32, (f64::from(y) * scale) as u32);
        }
        let (w, h) = (x.checked_div(cols)?, y.checked_div(rows)?);
        (w > 0 && h > 0).then_some((w, h))
    }
}

#[derive(Debug, Default, Clone)]
pub struct Probe {
    pub cell: Option<(u16, u16)>,
    pub version: String,
    pub kitty_shm: bool,
    pub kitty_temp: bool,
    pub kitty_direct: bool,
    pub sync: bool,
    pub pixel_mouse: bool,
    pub answered: bool,
}

impl Probe {
    pub fn no_graphics(&self) -> bool {
        self.answered && !(self.kitty_direct || self.kitty_shm || self.kitty_temp)
    }

    pub fn cell_px(&self, size: Size, scale: f64) -> (u32, u32) {
        match self.cell {
            Some((w, h)) if w > 0 && h > 0 => (u32::from(w), u32::from(h)),
            _ => size.cell_px(scale).unwrap_or(((8.0 * scale) as u32, (16.0 * scale) as u32)),
        }
    }

    pub fn medium(&self) -> kitty::Medium {
        if self.kitty_shm {
            kitty::Medium::Shm
        } else if self.kitty_temp {
            kitty::Medium::Temp
        } else {
            kitty::Medium::Direct
        }
    }

    pub fn herdr(&self) -> bool {
        self.version.starts_with("libghostty") || std::env::var("TERM_PROGRAM").as_deref() == Ok("herdr")
    }

    /// The hand pointer's name. iTerm2 knows only X11 names, and Ghostty has no `hand2`.
    pub fn hand(&self) -> &'static str {
        if self.version.starts_with("iTerm2") { "hand2" } else { "pointer" }
    }
}

impl Tty {
    pub fn open() -> io::Result<Tty> {
        let out = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let fd = out.as_raw_fd();
        Ok(Tty { out, saved: None, fd })
    }

    pub fn raw(&mut self) -> io::Result<()> {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(self.fd, &mut t) != 0 {
                return Err(io::Error::last_os_error());
            }
            if self.saved.is_none() {
                self.saved = Some(t);
            }
            libc::cfmakeraw(&mut t);
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(self.fd, libc::TCSANOW, &t) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn restore(&mut self) {
        if let Some(t) = self.saved {
            unsafe {
                libc::tcsetattr(self.fd, libc::TCSANOW, &t);
                libc::tcflush(self.fd, libc::TCIFLUSH);
            }
        }
    }

    pub fn saved(&self) -> Option<libc::termios> {
        self.saved
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _held = WRITE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = self.out.write_all(bytes);
    }

    pub fn flush(&mut self) {
        let _ = self.out.flush();
    }

    pub fn size(&self) -> Size {
        size_of(self.fd)
    }

    pub fn read_timeout(&mut self, buf: &mut Vec<u8>, until: Instant, done: impl Fn(&[u8]) -> bool) {
        let mut chunk = [0u8; 4096];
        while !done(buf) {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            if !readable(self.fd, left) {
                continue;
            }
            match self.out.read(&mut chunk) {
                Ok(n) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                _ => break,
            }
        }
    }

    pub fn probe(&mut self, shm_name: &str, temp_path: &str, timeout: Duration) -> (Probe, Vec<u8>) {
        let b64 = |s: &str| kitty::base64(s.as_bytes());
        let q = format!(
            "\x1b[16t\x1b[>0q\x1b[?2026$p\x1b[?1016$p\x1b_Gi=4001,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b_Gi=4002,s=1,v=1,a=q,t=s,f=24;{}\x1b\\\x1b_Gi=4003,s=1,v=1,a=q,t=t,f=24;{}\x1b\\\x1b[c",
            b64(shm_name),
            b64(temp_path)
        );
        self.write(q.as_bytes());
        self.flush();
        let mut buf = Vec::new();
        self.read_timeout(&mut buf, Instant::now() + timeout, |b| find_da1(b).is_some());
        crate::trace::log_probe(&buf);
        parse_probe(&buf)
    }
}

fn parse_probe(buf: &[u8]) -> (Probe, Vec<u8>) {
    let mut p = Probe::default();
    let s = String::from_utf8_lossy(buf).into_owned();
    if let Some(i) = s.find("[6;") {
        let nums: Vec<u16> = s[i + 3..].split('t').next().unwrap_or("").split(';').filter_map(|n| n.parse().ok()).collect();
        if let [h, w] = nums[..] {
            p.cell = Some((w, h));
        }
    }
    if let Some(i) = s.find("P>|") {
        p.version = s[i + 3..].split('\x1b').next().unwrap_or("").to_string();
    }
    p.sync = s.contains("[?2026;1$y") || s.contains("[?2026;2$y");
    p.pixel_mouse = ["1", "2", "3"].iter().any(|v| s.contains(&format!("[?1016;{v}$y")));
    p.kitty_direct = s.contains("Gi=4001;OK");
    p.kitty_shm = s.contains("Gi=4002;OK");
    p.kitty_temp = s.contains("Gi=4003;OK");
    let end = find_da1(buf);
    p.answered = end.is_some();
    let rest = end.map_or_else(Vec::new, |end| buf[end..].to_vec());
    (p, rest)
}

/// Mode 1003 reports every move, so hover works with no button held.
pub const MOUSE_ON: &str = "\x1b[?1003h\x1b[?1006h";
pub const PIXEL_MOUSE_ON: &str = "\x1b[?1016h";
/// Also puts the pointer shape back (OSC 22), which hover may have changed.
pub const MOUSE_OFF: &str = "\x1b[?1003l\x1b[?1006l\x1b[?1016l\x1b]22;\x1b\\";
/// Kitty keyboard flag 1, so Cmd-C reaches the viewer as `CSI 99;9u`.
/// Pop it before leaving the alternate screen: each screen keeps its own stack.
pub const KEYS_ON: &str = "\x1b[>1u";
pub const KEYS_OFF: &str = "\x1b[<1u";

/// Asks the terminal for a pointer shape (OSC 22). Terminals without OSC 22 ignore it.
pub fn pointer(shape: &str) -> String {
    format!("\x1b]22;{shape}\x1b\\")
}

pub fn size_of(fd: i32) -> Size {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    Size { cols: ws.ws_col, rows: ws.ws_row, xpix: ws.ws_xpixel, ypix: ws.ws_ypixel }
}

pub fn leave() -> String {
    format!("{MOUSE_OFF}{KEYS_OFF}\x1b[0m\x1b[2J\x1b[?25h\x1b[?7h\x1b[?1049l")
}

pub fn on_panic_restore(saved: Option<libc::termios>) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = OpenOptions::new().write(true).open("/dev/tty") {
            let _ = f.write_all(leave().as_bytes());
            let _ = f.flush();
            if let Some(t) = saved {
                unsafe { libc::tcsetattr(f.as_raw_fd(), libc::TCSANOW, &t) };
            }
        }
        prev(info);
        std::process::exit(101);
    }));
}

impl Drop for Tty {
    fn drop(&mut self) {
        self.restore();
    }
}

pub fn readable(fd: i32, timeout: Duration) -> bool {
    if fd < 0 || fd as usize >= libc::FD_SETSIZE {
        return true;
    }
    let micros = timeout.as_micros().min(i32::MAX as u128) as i64;
    let mut tv = libc::timeval { tv_sec: micros / 1_000_000, tv_usec: (micros % 1_000_000) as _ };
    unsafe {
        let mut set: libc::fd_set = std::mem::zeroed();
        libc::FD_ZERO(&mut set);
        libc::FD_SET(fd, &mut set);
        libc::select(fd + 1, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), &mut tv) > 0
    }
}

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn fit(s: &str, max: usize) -> (String, usize) {
    let mut out = String::new();
    for c in s.chars() {
        out.push(c);
        if width(&out) > max {
            out.pop();
            break;
        }
    }
    let w = width(&out);
    (out, w)
}

pub fn wrap(text: &str, cols: usize) -> String {
    let cols = if cols < 20 { 80 } else { cols };
    let mut out = String::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for w in para.split(' ') {
            if !line.is_empty() && width(&line) + 1 + width(w) > cols {
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(w);
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.pop();
    out
}

pub fn io_error(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "no such file".into(),
        io::ErrorKind::PermissionDenied => "permission denied".into(),
        io::ErrorKind::IsADirectory => "it is a directory, not a file".into(),
        _ => {
            let s = e.to_string();
            let s = s.split(" (os error").next().unwrap_or(&s);
            let mut c = s.chars();
            c.next().map_or_else(String::new, |f| f.to_lowercase().chain(c).collect())
        }
    }
}

pub fn printable(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '\u{fffd}' } else { c }).collect()
}

pub fn find_da1(b: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 3 < b.len() {
        if b[i] == 0x1b && b[i + 1] == b'[' && b[i + 2] == b'?' {
            let mut j = i + 3;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b';') {
                j += 1;
            }
            if j < b.len() && b[j] == b'c' {
                return Some(j + 1);
            }
        }
        i += 1;
    }
    None
}

pub enum Event {
    Key(Key),
    Resize,
    FileChanged,
    /// The file list has new entries.
    Listed,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    WheelUp(u16, u16),
    WheelDown(u16, u16),
    WheelLeft(u16, u16),
    WheelRight(u16, u16),
    Mouse(Click, u16, u16),
    /// Cmd-C, sent only by terminals that took the kitty keyboard push.
    Copy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Press,
    Drag,
    Release,
    /// The pointer moved with no button held.
    Move,
}

fn cell(params: &str) -> Option<(u16, u16)> {
    let mut it = params[1..].split(';').skip(1).map(|n| n.parse::<i32>().ok().map(|v| v.clamp(1, i32::from(u16::MAX)) as u16));
    Some((it.next()??, it.next()??))
}

/// The key a control byte stands for.
fn control(c: u8) -> Key {
    match c {
        b'\r' | b'\n' => Key::Enter,
        0x08 | 0x7f => Key::Backspace,
        _ => Key::Ctrl((b'a' + c - 1) as char),
    }
}

/// Reads a kitty keyboard key, `CSI code ; mods u`. Ctrl-letters become the
/// key their control byte gives, so they act as they do without the protocol.
fn csi_u(params: &str) -> Option<Key> {
    let mut fields = params.split(';').map(|f| f.split(':').next().unwrap_or(""));
    let code: u32 = fields.next()?.parse().ok()?;
    let mods = fields.next().map_or(Some(1), |m| m.parse::<u32>().ok())?.saturating_sub(1) & !(64 | 128);
    match (mods, char::from_u32(code)?) {
        (8, 'c') => Some(Key::Copy),
        (4 | 5, c @ 'a'..='z') => Some(control(c as u8 - b'a' + 1)),
        (0, '\x1b') | (4 | 5, '[') => Some(Key::Esc),
        // Kitty sends these as legacy bytes, but a terminal may not.
        (0, c @ ('\r' | '\t' | '\x7f')) => Some(control(c as u8)),
        // The keypad's own keys, which flag 1 also reports in this form.
        _ => match code {
            57399..=57415 => Some(match b"0123456789./*-+\r="[(code - 57399) as usize] {
                b'\r' => Key::Enter,
                c => Key::Char(c.into()),
            }),
            57417 => Some(Key::Left),
            57418 => Some(Key::Right),
            57419 => Some(Key::Up),
            57420 => Some(Key::Down),
            57421 => Some(Key::PageUp),
            57422 => Some(Key::PageDown),
            57423 => Some(Key::Home),
            57424 => Some(Key::End),
            _ => None,
        },
    }
}

pub fn parse_keys(buf: &mut Vec<u8>, flush_esc: bool) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    let b = &buf[..];
    while i < b.len() {
        let c = b[i];
        if c == 0x1b {
            if i + 1 >= b.len() {
                if flush_esc {
                    keys.push(Key::Esc);
                    i += 1;
                }
                break;
            }
            let n = b[i + 1];
            if n == b'[' || n == b'O' {
                let mut j = i + 2;
                while j < b.len() && !(0x40..=0x7e).contains(&b[j]) {
                    j += 1;
                }
                if j >= b.len() {
                    if flush_esc {
                        i = b.len();
                    }
                    break;
                }
                let params = std::str::from_utf8(&b[i + 2..j]).unwrap_or("");
                match (b[j], params) {
                    (b'A', _) => keys.push(Key::Up),
                    (b'B', _) => keys.push(Key::Down),
                    (b'C', _) => keys.push(Key::Right),
                    (b'D', _) => keys.push(Key::Left),
                    (b'H', _) => keys.push(Key::Home),
                    (b'F', _) => keys.push(Key::End),
                    (b'~', "5") => keys.push(Key::PageUp),
                    (b'~', "6") => keys.push(Key::PageDown),
                    (b'~', "1" | "7") => keys.push(Key::Home),
                    (b'~', "4" | "8") => keys.push(Key::End),
                    (b'u', p) => keys.extend(csi_u(p)),
                    (b'M' | b'm', p) if p.starts_with('<') => {
                        let code: u32 = p[1..].split(';').next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        let side = |(c, r)| if code & 1 == 0 { Key::WheelLeft(c, r) } else { Key::WheelRight(c, r) };
                        if b[j] == b'M' {
                            match code & !0x1c {
                                64 | 65 if code & 4 != 0 => keys.extend(cell(p).map(side)),
                                66 | 67 => keys.extend(cell(p).map(side)),
                                64 => keys.extend(cell(p).map(|(c, r)| Key::WheelUp(c, r))),
                                65 => keys.extend(cell(p).map(|(c, r)| Key::WheelDown(c, r))),
                                0 => keys.extend(cell(p).map(|(c, r)| Key::Mouse(Click::Press, c, r))),
                                32 => keys.extend(cell(p).map(|(c, r)| Key::Mouse(Click::Drag, c, r))),
                                35 => keys.extend(cell(p).map(|(c, r)| Key::Mouse(Click::Move, c, r))),
                                _ => {}
                            }
                        } else if code & !0x1c == 0 {
                            keys.extend(cell(p).map(|(c, r)| Key::Mouse(Click::Release, c, r)));
                        }
                    }
                    _ => {}
                }
                i = j + 1;
                continue;
            }
            if matches!(n, b']' | b'P' | b'_' | b'^' | b'X') {
                // A reply holds no ESC but its ST, so another ESC means Alt+P or the like began it:
                // drop the two bytes and read on, or every mouse report after it would wait.
                match (i + 2..b.len()).find(|&j| b[j] == 0x07 || b[j] == 0x1b).map(|j| (j, b.get(j + 1))) {
                    Some((j, _)) if b[j] == 0x07 => i = j + 1,
                    Some((j, Some(b'\\'))) => i = j + 2,
                    Some((j, Some(_))) => i = j,
                    Some(_) | None if flush_esc => i = b.len(),
                    Some(_) | None => break,
                }
                continue;
            }
            keys.push(Key::Esc);
            i += 1;
            continue;
        }
        match c {
            0x01..=0x1a | 0x7f => keys.push(control(c)),
            _ => {
                let len = match c {
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    _ => 1,
                };
                let tail = &b[i + 1..b.len().min(i + len)];
                if !tail.iter().all(|&t| (0x80..=0xbf).contains(&t)) {
                    i += 1;
                    continue;
                }
                if i + len > b.len() {
                    if flush_esc {
                        i = b.len();
                    }
                    break;
                }
                match std::str::from_utf8(&b[i..i + len]) {
                    Ok(s) => {
                        keys.extend(s.chars().map(Key::Char));
                        i += len;
                    }
                    Err(_) => i += 1,
                }
                continue;
            }
        }
        i += 1;
    }
    buf.drain(..i.min(buf.len()));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_size_comes_from_the_probe_then_the_window_then_a_default() {
        let size = Size { cols: 80, rows: 24, xpix: 1600, ypix: 960 };
        let probe = Probe { cell: Some((10, 20)), ..Probe::default() };
        assert_eq!(probe.cell_px(size, 2.0), (10, 20));
        assert_eq!(Probe::default().cell_px(size, 2.0), (20, 40));
        assert_eq!(Probe::default().cell_px(Size { xpix: 0, ..size }, 2.0), (16, 32));
        assert_eq!(Size { cols: 0, ..size }.cell_px(2.0), None);
    }

    #[test]
    fn a_window_size_in_points_is_scaled_to_pixels() {
        let warp = Size { cols: 89, rows: 53, xpix: 749, ypix: 863 };
        assert_eq!(warp.cell_px(2.0), Some((16, 32)));
        assert_eq!(warp.cell_px(1.0), Some((8, 16)));
        let px = Size { cols: 80, rows: 24, xpix: 1600, ypix: 960 };
        assert_eq!(px.cell_px(2.0), Some((20, 40)));
    }

    #[test]
    fn medium_prefers_shared_memory() {
        let p = |kitty_shm, kitty_temp| Probe { kitty_shm, kitty_temp, kitty_direct: true, ..Probe::default() }.medium();
        assert_eq!((p(true, true), p(false, true), p(false, false)), (kitty::Medium::Shm, kitty::Medium::Temp, kitty::Medium::Direct));
    }

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap("aaaa bbbb cccc dddd eeee ffff", 20), "aaaa bbbb cccc dddd\neeee ffff");
        assert_eq!(wrap("short", 80), "short");
    }

    #[test]
    fn parses_keys() {
        let mut b = b"jk\x1b[A\x1b[6~\x04/\x1b[<65;3;4M\r\x7f".to_vec();
        let keys = parse_keys(&mut b, true);
        assert_eq!(keys, vec![Key::Char('j'), Key::Char('k'), Key::Up, Key::PageDown, Key::Ctrl('d'), Key::Char('/'), Key::WheelDown(3, 4), Key::Enter, Key::Backspace]);
        assert!(b.is_empty());
    }

    #[test]
    fn parses_left_button_sgr() {
        let mut b = b"\x1b[<0;12;5M\x1b[<32;13;5M\x1b[<0;14;6m\x1b[<2;1;1M\x1b[<2;1;1m\x1b[<35;1;1M\x1b[<0;;1M\x1b[<39;7;8M\x1b[<34;2;2M".to_vec();
        let keys = parse_keys(&mut b, true);
        assert_eq!(keys, vec![Key::Mouse(Click::Press, 12, 5), Key::Mouse(Click::Drag, 13, 5), Key::Mouse(Click::Release, 14, 6), Key::Mouse(Click::Move, 1, 1), Key::Mouse(Click::Move, 7, 8)]);
        assert!(b.is_empty());
    }

    #[test]
    fn parses_side_wheel() {
        let mut b = b"\x1b[<68;7;3M\x1b[<69;8;4M\x1b[<66;9;5M\x1b[<67;10;6M\x1b[<71;1;2M\x1b[<64;1;1M".to_vec();
        let keys = parse_keys(&mut b, true);
        assert_eq!(keys, vec![Key::WheelLeft(7, 3), Key::WheelRight(8, 4), Key::WheelLeft(9, 5), Key::WheelRight(10, 6), Key::WheelRight(1, 2), Key::WheelUp(1, 1)]);
    }

    #[test]
    fn readable_waits_for_input() {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        assert!(!readable(fds[0], Duration::from_millis(10)));
        assert_eq!(unsafe { libc::write(fds[1], b"x".as_ptr().cast(), 1) }, 1);
        assert!(readable(fds[0], Duration::from_millis(10)));
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        assert!(readable(libc::FD_SETSIZE as i32 + 10, Duration::ZERO));
    }

    #[test]
    fn printable_replaces_control_characters() {
        assert_eq!(printable("a\x1b]52;c;eA==\x07b\u{9b}2J\u{7f}é"), "a\u{fffd}]52;c;eA==\u{fffd}b\u{fffd}2J\u{fffd}é");
    }

    #[test]
    fn fit_counts_terminal_cells() {
        assert_eq!(width("笔记.md"), 7);
        assert_eq!(fit("笔记.md", 3), ("笔".to_string(), 2));
        assert_eq!(fit("ab🚀c", 4), ("ab🚀".to_string(), 4));
        assert_eq!(fit("plain", 10), ("plain".to_string(), 5));
    }

    #[test]
    fn lone_escape_waits_unless_flushed() {
        let mut b = b"\x1b".to_vec();
        assert!(parse_keys(&mut b, false).is_empty());
        assert_eq!(parse_keys(&mut b, true), vec![Key::Esc]);
    }

    #[test]
    fn probe_flags_missing_graphics_only_after_an_answer() {
        let (p, rest) = parse_probe(b"\x1b_Gi=4001;OK\x1b\\\x1b_Gi=4002;EBADF\x1b\\\x1b[?62;22cj");
        assert!(p.kitty_direct && !p.no_graphics());
        assert_eq!(rest, b"j");
        let (p, _) = parse_probe(b"\x1b[?62;22c");
        assert!(p.no_graphics());
        let (p, _) = parse_probe(b"");
        assert!(!p.no_graphics());
    }

    #[test]
    fn probe_reads_pixel_mouse_support() {
        assert!(parse_probe(b"\x1b[?1016;2$y\x1b[?62;22c").0.pixel_mouse);
        assert!(!parse_probe(b"\x1b[?1016;0$y\x1b[?62;22c").0.pixel_mouse);
        assert!(!parse_probe(b"\x1b[?62;22c").0.pixel_mouse);
    }

    #[test]
    fn pixel_reports_in_the_padding_clamp_to_the_edge() {
        let mut b = b"\x1b[<0;-3;250M\x1b[<0;1200;-1m".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Mouse(Click::Press, 1, 250), Key::Mouse(Click::Release, 1200, 1)]);
    }

    #[test]
    fn drops_terminal_replies() {
        let mut b = b"\x1bP>|libghostty 1.3\x1b\\j".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Char('j')]);
        let mut b = b"\x1b]11;rgb:0a0a/0b0b/0f0f\x07k".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Char('k')]);
        let mut b = b"\x1b_Gi=4001;OK\x1b\\".to_vec();
        assert!(parse_keys(&mut b, true).is_empty());
        assert!(b.is_empty());
    }

    #[test]
    fn alt_p_does_not_hold_the_input_after_it() {
        let mut b = b"\x1bP\x1b[<35;5;6Mj".to_vec();
        assert_eq!(parse_keys(&mut b, false), vec![Key::Mouse(Click::Move, 5, 6), Key::Char('j')]);
        assert!(b.is_empty());
    }

    #[test]
    fn partial_reply_waits_then_drops() {
        let mut b = b"\x1b]11;rgb:0a".to_vec();
        assert!(parse_keys(&mut b, false).is_empty());
        assert_eq!(b.len(), 11);
        assert!(parse_keys(&mut b, true).is_empty());
        assert!(b.is_empty());
    }

    #[test]
    fn bad_utf8_keeps_next_key() {
        let mut b = b"\xc3j".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Char('j')]);
        let mut b = b"\xe2j".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Char('j')]);
        let mut b = b"\xe2\x82".to_vec();
        assert!(parse_keys(&mut b, false).is_empty());
        assert_eq!(b.len(), 2);
        assert!(parse_keys(&mut b, true).is_empty());
        assert!(b.is_empty());
        let mut b = "é€".as_bytes().to_vec();
        assert_eq!(parse_keys(&mut b, false), vec![Key::Char('é'), Key::Char('€')]);
    }

    #[test]
    fn probe_reads_cell_size_and_version() {
        let (p, _) = parse_probe(b"\x1b[6;38;19t\x1bP>|libghostty 1.3\x1b\\\x1b[?62;22c");
        assert_eq!(p.cell, Some((19, 38)));
        assert_eq!(p.version, "libghostty 1.3");
        assert_eq!(p.hand(), "pointer");
        let (p, _) = parse_probe(b"\x1b[6;38t\x1b[?62;22c");
        assert_eq!(p.cell, None);
        assert_eq!(p.version, "");
        let (p, _) = parse_probe(b"\x1bP>|iTerm2 3.6.6\x1b\\\x1b[?62;22c");
        assert_eq!(p.hand(), "hand2");
    }

    #[test]
    fn parses_ss3_and_tilde_keys() {
        let mut b = b"\x1bOA\x1bOB\x1bOC\x1bOD\x1b[1~\x1b[7~\x1b[4~\x1b[8~\x1b[H\x1b[F\x1b[5~".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Up, Key::Down, Key::Right, Key::Left, Key::Home, Key::Home, Key::End, Key::End, Key::Home, Key::End, Key::PageUp]);
        assert!(b.is_empty());
    }

    #[test]
    fn kitty_keys_parse_like_legacy_bytes() {
        let cases: &[(&[u8], &[Key])] = &[
            (b"\x1b[99;9u", &[Key::Copy]),
            (b"\x1b[99;73u\x1b[99;137u", &[Key::Copy, Key::Copy]),
            (b"\x1b[99;5u", &[Key::Ctrl('c')]),
            (b"\x1b[99;6u", &[Key::Ctrl('c')]),
            (b"\x1b[100;5u\x1b[117;5u\x1b[122;5u", &[Key::Ctrl('d'), Key::Ctrl('u'), Key::Ctrl('z')]),
            (b"\x1b[105;5u\x1b[104;5u\x1b[109;5u", &[Key::Ctrl('i'), Key::Backspace, Key::Enter]),
            (b"\x1b[100;69u", &[Key::Ctrl('d')]),
            (b"\x1b[27u\x1b[27;1u\x1b[27;65u", &[Key::Esc, Key::Esc, Key::Esc]),
            (b"\x1b[99;5:1u", &[Key::Ctrl('c')]),
            (b"\x1b[99:67;5u", &[Key::Ctrl('c')]),
            (b"\x1b[91;5u", &[Key::Esc]),
            (b"\x1b[13u\x1b[9u\x1b[127u", &[Key::Enter, Key::Ctrl('i'), Key::Backspace]),
            (b"\x1b[57399u\x1b[57408u\x1b[57409u\x1b[57413u\x1b[57415u", &[Key::Char('0'), Key::Char('9'), Key::Char('.'), Key::Char('+'), Key::Char('=')]),
            (b"\x1b[57414u\x1b[57419u\x1b[57420;2u\x1b[57421u\x1b[57424u", &[Key::Enter, Key::Up, Key::Down, Key::PageUp, Key::End]),
        ];
        for (input, want) in cases {
            let mut b = input.to_vec();
            assert_eq!(parse_keys(&mut b, true), *want, "{}", printable(&String::from_utf8_lossy(input)));
            assert!(b.is_empty());
        }
    }

    #[test]
    fn unknown_kitty_keys_drop_whole() {
        let mut b = b"\x1b[106;3uj\x1b[99;13uk\x1b[57441;2u\x1b[?1u\x1b[;5u\x1b[1114112;5u\x1b[99;3u".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Char('j'), Key::Char('k')]);
    }

    #[test]
    fn split_kitty_key_waits() {
        let mut b = b"j\x1b[99;".to_vec();
        assert_eq!(parse_keys(&mut b, false), vec![Key::Char('j')]);
        b.extend_from_slice(b"9u");
        assert_eq!(parse_keys(&mut b, false), vec![Key::Copy]);
    }

    #[test]
    fn tab_is_ctrl_i() {
        let mut b = b"a\tb".to_vec();
        assert_eq!(parse_keys(&mut b, true), vec![Key::Char('a'), Key::Ctrl('i'), Key::Char('b')]);
    }

    #[test]
    fn incomplete_csi_waits_unless_flushed() {
        let mut b = b"j\x1b[1".to_vec();
        assert_eq!(parse_keys(&mut b, false), vec![Key::Char('j')]);
        assert_eq!(b, b"\x1b[1");
        assert!(parse_keys(&mut b, true).is_empty());
        assert!(b.is_empty());
        let mut b = b"\x1bO".to_vec();
        assert!(parse_keys(&mut b, false).is_empty());
        assert_eq!(b, b"\x1bO");
        assert!(parse_keys(&mut b, true).is_empty());
        assert!(b.is_empty());
    }
}
