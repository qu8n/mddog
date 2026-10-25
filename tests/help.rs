#![expect(clippy::unwrap_used, reason = "a failed step should fail the test")]

use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The terminal's answer to the viewer's probe, for a cell `(width, height)` device pixels big.
fn probe_reply((w, h): (u16, u16)) -> Vec<u8> {
    format!("\x1b]11;rgb:0a0a/0a0a/0f0f\x1b\\\x1b[6;{h};{w}t\x1bP>|libghostty\x1b\\\x1b[?2026;2$y\x1b_Gi=4001;OK\x1b\\\x1b[?62;22c").into_bytes()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn wait_for(out: &Mutex<Vec<u8>>, from: usize, needle: &[u8]) -> usize {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(i) = find(&out.lock().unwrap()[from..], needle) {
            return from + i;
        }
        assert!(Instant::now() < end, "no {:?} in the output", String::from_utf8_lossy(needle));
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Viewer {
    child: std::process::Child,
    tty: std::fs::File,
    out: Arc<Mutex<Vec<u8>>>,
}

const GFM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/gfm.md");

fn open(file: &str, cols: u16, env: &[(&str, &str)]) -> Viewer {
    launch(file, cols, env, false)
}

fn launch(file: &str, cols: u16, env: &[(&str, &str)], job: bool) -> Viewer {
    start(&[file], None, cols, env, job)
}

fn start(args: &[&str], dir: Option<&std::path::Path>, cols: u16, env: &[(&str, &str)], job: bool) -> Viewer {
    start_exe(env!("CARGO_BIN_EXE_mdd").as_ref(), args, dir, cols, (18, 38), env, job)
}

fn start_exe(exe: &std::path::Path, args: &[&str], dir: Option<&std::path::Path>, cols: u16, cell: (u16, u16), env: &[(&str, &str)], job: bool) -> Viewer {
    start_replying(exe, args, dir, cols, cell, probe_reply(cell), env, job)
}

#[expect(clippy::too_many_arguments, reason = "a test helper; each caller sets a different mix")]
fn start_replying(exe: &std::path::Path, args: &[&str], dir: Option<&std::path::Path>, cols: u16, cell: (u16, u16), reply: Vec<u8>, env: &[(&str, &str)], job: bool) -> Viewer {
    let (mut master, mut slave) = (0, 0);
    let mut size = libc::winsize { ws_row: 40, ws_col: cols, ws_xpixel: cols * cell.0, ws_ypixel: 40 * cell.1 };
    assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) }, 0);
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    let mut cmd = Command::new(exe);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.args(args)
        .env("TERM_PROGRAM", "herdr")
        .env("MDDOG_NO_SPARE", "1")
        .envs(env.iter().copied())
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave));
    unsafe {
        cmd.pre_exec(move || {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY.into(), 0);
            if job {
                run_as_job();
            }
            Ok(())
        });
    }
    let child = cmd.spawn().unwrap();
    let tty = unsafe { std::fs::File::from_raw_fd(master) };
    let out = Arc::new(Mutex::new(Vec::new()));
    let (reader, mut writer, seen) = (tty.try_clone().unwrap(), tty.try_clone().unwrap(), out.clone());
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = vec![0; 1 << 16];
        let mut answered = false;
        while let Ok(n @ 1..) = reader.read(&mut buf) {
            let mut all = seen.lock().unwrap();
            all.extend_from_slice(&buf[..n]);
            if !answered && find(&all, b"\x1b[c").is_some() {
                answered = true;
                writer.write_all(&reply).unwrap();
            }
        }
    });

    Viewer { child, tty, out }
}

/// Acts as a small job-control shell. The kernel drops SIGTSTP for an orphaned
/// process group, and a session leader's group is one, so the viewer runs in
/// its own foreground group under this process, as it does under a real shell.
/// This process copies the viewer's stops and exit status, so the test can
/// `waitpid` it as if it were the viewer.
unsafe fn run_as_job() {
    unsafe {
        let pid = libc::fork();
        if pid == 0 {
            libc::setpgid(0, 0);
            let mut ttou: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut ttou);
            libc::sigaddset(&mut ttou, libc::SIGTTOU);
            libc::sigprocmask(libc::SIG_BLOCK, &ttou, std::ptr::null_mut());
            libc::tcsetpgrp(0, libc::getpid());
            libc::sigprocmask(libc::SIG_UNBLOCK, &ttou, std::ptr::null_mut());
            return;
        }
        // Closes spawn's exec-error pipe, or spawn waits for this process to exit.
        for fd in 3..1024 {
            libc::close(fd);
        }
        let mut st = 0;
        loop {
            if libc::waitpid(pid, &mut st, libc::WUNTRACED) < 0 {
                continue;
            }
            if libc::WIFSTOPPED(st) {
                libc::kill(libc::getpid(), libc::SIGSTOP);
                libc::kill(pid, libc::SIGCONT);
            } else if libc::WIFEXITED(st) {
                libc::_exit(libc::WEXITSTATUS(st));
            } else {
                libc::_exit(128 + libc::WTERMSIG(st));
            }
        }
    }
}

fn exits_within(child: &mut std::process::Child, limit: Duration) -> Option<bool> {
    let end = Instant::now() + limit;
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return Some(s.success());
        }
        if Instant::now() > end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn zellij_and_warp_are_refused() {
    for (env, why) in [(("ZELLIJ", "0"), "zellij"), (("TERM_PROGRAM", "WarpTerminal"), "Warp")] {
        let Viewer { mut child, out, .. } = open(GFM, 80, &[env]);
        wait_for(&out, 0, why.as_bytes());
        let exit = exits_within(&mut child, Duration::from_secs(5));
        let _ = child.kill();
        assert_eq!(exit, Some(false), "{why} wasn't refused");
        assert!(find(&out.lock().unwrap(), b"a=p,").is_none(), "{why} got a page");
    }
}

#[test]
fn ctrl_c_in_the_editor_keeps_the_viewer() {
    let Viewer { mut child, mut tty, out } = open(GFM, 80, &[("EDITOR", "sleep 2;:")]);
    wait_for(&out, 0, b"a=p,");
    tty.write_all(b"e").unwrap();
    wait_for(&out, 0, b"\x1b[?1049l");
    std::thread::sleep(Duration::from_millis(300));
    tty.write_all(b"\x03").unwrap();
    let after = wait_for(&out, 0, b"Editor");
    // Checked before writing again: a viewer that quit has closed the pty, so the write would fail
    // with EIO and hide why.
    assert!(exits_within(&mut child, Duration::from_millis(1500)).is_none(), "Ctrl-C in the editor quit the viewer");
    wait_for(&out, after, b"a=p,");
    tty.write_all(b"q").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    assert_eq!(quit, Some(true));
}

#[test]
fn sigint_outside_the_editor_quits() {
    let Viewer { mut child, out, .. } = open(GFM, 80, &[]);
    wait_for(&out, 0, b"a=p,");
    unsafe { libc::kill(child.id().cast_signed(), libc::SIGINT) };
    let exit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    assert!(exit.is_some(), "SIGINT didn't quit the viewer");
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mddog-help-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mddog")).unwrap();
    dir
}

#[test]
fn a_save_during_startup_is_shown() {
    let dir = scratch("startup");
    let file = dir.join("a.md");
    std::fs::write(&file, "# One\n").unwrap();
    let Viewer { mut child, mut tty, out } = open(file.to_str().unwrap(), 80, &[]);
    wait_for(&out, 0, b"\x1b[c");
    std::fs::write(&file, "# Two\n").unwrap();
    wait_for(&out, 0, b"a=p,");
    let end = Instant::now() + Duration::from_secs(3);
    while find(&out.lock().unwrap(), b"Reloaded").is_none() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let reloaded = find(&out.lock().unwrap(), b"Reloaded").is_some();
    tty.write_all(b"q").unwrap();
    let _ = exits_within(&mut child, Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(reloaded, "the save made before the watcher started was never shown");
}

#[test]
fn contents_toggle_keeps_the_loaded_version_without_live_reload() {
    let dir = scratch("nolive");
    std::fs::write(dir.join("mddog/config.toml"), "live_reload = false\n").unwrap();
    let file = dir.join("a.md");
    std::fs::write(&file, "# Alpha\n\ntext\n\n## Alpha two\n").unwrap();
    let Viewer { mut child, mut tty, out } = open(file.to_str().unwrap(), 100, &[("XDG_CONFIG_HOME", dir.to_str().unwrap())]);
    wait_for(&out, 0, b"a=p,");
    std::fs::write(&file, "# Beta\n\ntext\n\n## Beta two\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let before = out.lock().unwrap().len();
    tty.write_all(b"t").unwrap();
    wait_for(&out, before, b"Alpha two");
    let beta = find(&out.lock().unwrap()[before..], b"Beta").is_some();
    tty.write_all(b"q").unwrap();
    let _ = exits_within(&mut child, Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!beta, "t read the file from disk with live_reload off");
}

#[test]
fn a_page_that_does_not_load_keeps_its_source() {
    let dir = scratch("noload");
    std::fs::write(dir.join("mddog/config.toml"), "live_reload = false\n").unwrap();
    // WebKit waits on a FIFO with no writer, so the load event never comes.
    let fifo = std::ffi::CString::new(dir.join("stall.png").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let file = dir.join("a.md");
    std::fs::write(&file, "# Alpha\n\n![](stall.png)\n\n## Alpha two\n").unwrap();
    let Viewer { mut child, mut tty, out } = open(file.to_str().unwrap(), 100, &[("XDG_CONFIG_HOME", dir.to_str().unwrap())]);
    wait_for(&out, 0, b"The page did not load");
    let before = out.lock().unwrap().len();
    tty.write_all(b"t").unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while find(&out.lock().unwrap()[before..], b"Alpha two").is_none() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let kept = find(&out.lock().unwrap()[before..], b"Alpha two").is_some();
    tty.write_all(b"q").unwrap();
    let _ = exits_within(&mut child, Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(kept, "t after a failed load lost the document");
}

/// The last `top=` the viewer traced after the line that contains `after`, once one is there.
fn top_after(trace: &std::path::Path, after: &str) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let log = std::fs::read_to_string(trace).unwrap_or_default();
        if let Some(i) = log.rfind(after)
            && let Some(top) = log[i..].lines().rev().find_map(|l| l.split_once(" top=").map(|t| t.1))
        {
            return top.split('@').next().unwrap_or_default().to_string();
        }
        assert!(Instant::now() < end, "no view after {after:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn backspace_after_a_resize_returns_to_the_same_text() {
    let dir = scratch("backwrap");
    let trace = dir.join("trace.log");
    std::fs::write(dir.join("mddog/config.toml"), "smooth_scroll = false\n").unwrap();
    let Viewer { mut child, mut tty, out } = open(GFM, 100, &[("MDDOG_TRACE", trace.to_str().unwrap()), ("XDG_CONFIG_HOME", dir.to_str().unwrap())]);
    wait_for(&out, 0, b"a=p,");
    tty.write_all(&b"j".repeat(60)).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let before = top_after(&trace, "key Char('j')");
    tty.write_all(b"5").unwrap();
    top_after(&trace, "key Char('5')");
    // A narrower terminal wraps the text onto more rows.
    let size = libc::winsize { ws_row: 40, ws_col: 60, ws_xpixel: 60 * 18, ws_ypixel: 40 * 38 };
    use std::os::fd::AsRawFd;
    assert_eq!(unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCSWINSZ, &size) }, 0);
    top_after(&trace, "mark resize");
    std::thread::sleep(Duration::from_millis(500));
    tty.write_all(b"\x7f").unwrap();
    let back = top_after(&trace, "key Backspace");
    tty.write_all(b"q").unwrap();
    let _ = exits_within(&mut child, Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!before.is_empty());
    assert_eq!(back, before, "Backspace went to another place after a resize re-wrapped the page");
}

#[test]
fn hints_work_while_help_shows() {
    let Viewer { mut child, mut tty, out } = open(GFM, 100, &[]);
    wait_for(&out, 0, b"a=p,");
    let click = |tty: &mut std::fs::File, col: u16| tty.write_all(format!("\x1b[<0;{col};40M\x1b[<0;{col};40m").as_bytes()).unwrap();
    click(&mut tty, 64);
    wait_for(&out, 0, b"mddog keys");
    std::thread::sleep(Duration::from_millis(300));
    let before = out.lock().unwrap().len();
    tty.write_all(b"j").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let placed = find(&out.lock().unwrap()[before..], b"a=p,").is_some();
    click(&mut tty, 55);
    std::thread::sleep(Duration::from_millis(400));
    click(&mut tty, 55);
    let quit = exits_within(&mut child, Duration::from_secs(5)) == Some(true);
    let _ = child.kill();
    assert!(!placed, "j scrolled the page under the help screen");
    assert!(quit, "two clicks on the q hint did not quit");
}

#[test]
fn closing_help_sends_the_page_again() {
    let Viewer { mut child, mut tty, out } = open(GFM, 70, &[]);
    wait_for(&out, 0, b"a=p,");
    tty.write_all(b"?").unwrap();
    wait_for(&out, 0, b"mddog keys");
    std::thread::sleep(Duration::from_millis(1500));
    let closed = out.lock().unwrap().len();
    tty.write_all(b"?").unwrap();
    let place = wait_for(&out, closed, b"a=p,i=");
    let all = out.lock().unwrap().clone();
    let id: String = all[place + 6..].iter().take_while(|b| b.is_ascii_digit()).map(|&b| b as char).collect();
    let sent = find(&all[closed..], format!("t=d,i={id},q=2").as_bytes()).map(|i| closed + i);
    tty.write_all(b"q").unwrap();
    let _ = child.wait();
    assert!(sent.is_some_and(|s| s < place), "image {id} was placed after help closed but not sent again");
}

/// Waits for the page to be placed again after `before`, and says whether the screen was cleared on the way.
fn cleared_before_placing(out: &Mutex<Vec<u8>>, before: usize) -> bool {
    let place = wait_for(out, before, b"a=p,");
    find(&out.lock().unwrap()[before..place], b"\x1b[2J").is_some()
}

#[test]
fn closing_the_contents_clears_its_text() {
    let Viewer { mut child, mut tty, out } = open(GFM, 100, &[]);
    wait_for(&out, 0, b"a=p,");
    tty.write_all(b"t").unwrap();
    wait_for(&out, 0, b"CONTENTS");
    std::thread::sleep(Duration::from_millis(1500));
    let closed = out.lock().unwrap().len();
    tty.write_all(b"t").unwrap();
    let cleared = cleared_before_placing(&out, closed);
    tty.write_all(b"q").unwrap();
    let _ = child.wait();
    assert!(cleared, "the sidebar's text stayed on screen under the wider page");
}

#[test]
fn a_resize_clears_the_old_status_row() {
    let Viewer { mut child, tty, out } = open(GFM, 100, &[]);
    wait_for(&out, 0, b"a=p,");
    std::thread::sleep(Duration::from_millis(1500));
    let before = out.lock().unwrap().len();
    let size = libc::winsize { ws_row: 30, ws_col: 90, ws_xpixel: 90 * 18, ws_ypixel: 30 * 38 };
    use std::os::fd::AsRawFd;
    assert_eq!(unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCSWINSZ, &size) }, 0);
    let cleared = cleared_before_placing(&out, before);
    let mut tty = tty;
    tty.write_all(b"q").unwrap();
    let _ = child.wait();
    assert!(cleared, "the status row stayed at its old row after a resize");
}

/// A copy of the binary with its own defaults domain, named after it, so a test can set the macOS
/// appearance that copy sees without changing the system's.
struct Appearance(std::path::PathBuf);

impl Appearance {
    fn new(dir: &std::path::Path) -> Self {
        let exe = dir.join(format!("mddog-appearance-{}", std::process::id()));
        if std::fs::hard_link(env!("CARGO_BIN_EXE_mdd"), &exe).is_err() {
            std::fs::copy(env!("CARGO_BIN_EXE_mdd"), &exe).unwrap();
        }
        Appearance(exe)
    }

    fn domain(&self) -> &str {
        self.0.file_name().unwrap().to_str().unwrap()
    }

    fn set(&self, style: &str) {
        let ok = Command::new("defaults").args(["write", self.domain(), "AppleInterfaceStyle", style]).status().unwrap().success();
        assert!(ok, "defaults write failed");
    }
}

impl Drop for Appearance {
    fn drop(&mut self) {
        let _ = Command::new("defaults").args(["delete", self.domain()]).status();
        let _ = std::fs::remove_file(std::path::Path::new(&std::env::var("HOME").unwrap()).join(format!("Library/Preferences/{}.plist", self.domain())));
    }
}

#[test]
fn help_follows_an_appearance_change() {
    let dir = scratch("appearance");
    std::fs::write(dir.join("mddog/config.toml"), "theme = \"light:github-light,dark:obsidian-dark\"\n").unwrap();
    let mac = Appearance::new(&dir);
    mac.set("Light");
    let Viewer { mut child, mut tty, out } = start_exe(&mac.0, &[GFM], None, 100, (18, 38), &[("XDG_CONFIG_HOME", dir.to_str().unwrap())], false);
    wait_for(&out, 0, b"a=p,");
    mac.set("Dark");
    std::thread::sleep(Duration::from_millis(500));
    let before = out.lock().unwrap().len();
    tty.write_all(b"?").unwrap();
    let keys = wait_for(&out, before, b"mddog keys");
    let dark = find(&out.lock().unwrap()[before..keys], b"\x1b[0m\x1b[48;2;28;28;28m\x1b[2J").is_some();
    tty.write_all(b"q").unwrap();
    let _ = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    drop(mac);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(dark, "help kept the light theme after the appearance turned dark");
}

#[test]
fn help_flag_points_to_the_key_screen() {
    let out = Command::new(env!("CARGO_BIN_EXE_mdd")).arg("--help").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(text.contains("press ? to list the keys"), "{text}");
    assert!(!text.contains(mddog::viewer::app::KEYS.lines().nth(1).unwrap()), "{text}");
}

#[test]
fn ctrl_z_suspends_and_resumes() {
    suspends_and_resumes(b"\x1a");
}

#[test]
fn kitty_ctrl_z_suspends_and_resumes() {
    suspends_and_resumes(b"\x1b[122;5u");
}

/// Ctrl-Z sent as `key`. The viewer pops its kitty keyboard flags before it
/// leaves, so the shell gets legacy keys, and pushes them again on return.
fn suspends_and_resumes(key: &[u8]) {
    let Viewer { mut child, mut tty, out } = launch(GFM, 80, &[], true);
    let pid = child.id() as libc::pid_t;
    wait_for(&out, 0, b"a=p,");
    let start = out.lock().unwrap().len();
    tty.write_all(key).unwrap();
    let left = wait_for(&out, 0, b"\x1b[?1049l");
    assert!(find(&out.lock().unwrap()[start..left], b"\x1b[<1u").is_some(), "the keyboard flags stayed pushed in the shell");
    let end = Instant::now() + Duration::from_secs(10);
    let mut st = 0;
    while unsafe { libc::waitpid(pid, &mut st, libc::WUNTRACED | libc::WNOHANG) } == 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = libc::WIFSTOPPED(st);
    let early = find(&out.lock().unwrap()[left..], b"\x1b[?1049h").is_some();
    unsafe { libc::kill(pid, libc::SIGCONT) };
    let back = wait_for(&out, left, b"\x1b[?1049h");
    wait_for(&out, back, b"\x1b[>1u");
    wait_for(&out, back, b"a=p,");
    tty.write_all(b"q").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    assert!(stopped, "Ctrl-Z did not stop the viewer");
    assert!(!early, "the viewer came back before SIGCONT");
    assert_eq!(quit, Some(true));
}

#[test]
fn bare_mdd_lists_files_and_opens_one() {
    let dir = scratch("list");
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("alpha.md"), "# Alpha title\n\ntext\n").unwrap();
    std::fs::write(dir.join("sub/beta.md"), "# Beta title\n\ntext\n").unwrap();
    let Viewer { mut child, mut tty, out } = start(&[], Some(&dir.join("sub")), 80, &[], false);
    let list = wait_for(&out, 0, b"Beta title");
    let probe = wait_for(&out, 0, b"\x1b[c");
    wait_for(&out, 0, b"Alpha title");
    tty.write_all(b"bet\r").unwrap();
    let shown = wait_for(&out, probe, b"a=p,");
    wait_for(&out, shown, b"beta.md");
    tty.write_all(b"\x10").unwrap();
    let again = wait_for(&out, shown, b"mbet\x1b[0m");
    tty.write_all(b"\x15").unwrap();
    wait_for(&out, again, b"Alpha title");
    tty.write_all(b"alpha\r").unwrap();
    let alpha = wait_for(&out, again, b" alpha.md ");
    tty.write_all(b"\x7f").unwrap();
    wait_for(&out, alpha, b" beta.md ");
    tty.write_all(b"q").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(list < probe, "the list waited for the terminal probe");
    assert_eq!(quit, Some(true));
}

#[test]
fn a_pixel_click_in_the_list_follows_a_font_size_change() {
    let dir = scratch("listfont");
    let now = std::time::SystemTime::now();
    for i in 0..6u64 {
        let f = dir.join(format!("f{i}.md"));
        std::fs::write(&f, format!("# Title {i}\n")).unwrap();
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(now - Duration::from_secs(60 * i)).unwrap();
    }
    let mut reply = probe_reply((18, 38));
    reply.splice(0..0, *b"\x1b[?1016;2$y");
    let Viewer { mut child, tty, out } = start_replying(env!("CARGO_BIN_EXE_mdd").as_ref(), &[dir.to_str().unwrap()], None, 80, (18, 38), reply, &[], false);
    wait_for(&out, 0, b"\x1b[c");
    wait_for(&out, 0, b"Title 5");
    // A bigger font: the same 80 by 40 cells, 50 pixels tall.
    let before = out.lock().unwrap().len();
    let size = libc::winsize { ws_row: 40, ws_col: 80, ws_xpixel: 80 * 24, ws_ypixel: 40 * 50 };
    use std::os::fd::AsRawFd;
    assert_eq!(unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCSWINSZ, &size) }, 0);
    wait_for(&out, before, b"Title 5");
    // Row 6 holds the third file, f2.md.
    let (x, y) = (10 * 24 + 12, 5 * 50 + 25);
    let mut tty = tty;
    tty.write_all(format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes()).unwrap();
    let opened = wait_for(&out, before, b"a=p,");
    std::thread::sleep(Duration::from_millis(300));
    let f2 = find(&out.lock().unwrap()[opened..], b" f2.md ").is_some();
    tty.write_all(b"q").unwrap();
    let _ = exits_within(&mut child, Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(f2, "the click opened another file: {:?}", String::from_utf8_lossy(&out.lock().unwrap()[opened..]).chars().filter(|c| c.is_ascii_graphic() || *c == ' ').collect::<String>().get(..400));
}

#[test]
fn esc_in_the_startup_list_quits_and_in_a_file_goes_back() {
    let dir = scratch("listesc");
    std::fs::write(dir.join("a.md"), "# A title\n").unwrap();
    let Viewer { mut child, mut tty, out } = start(&[dir.to_str().unwrap()], None, 80, &[], false);
    wait_for(&out, 0, b"A title");
    wait_for(&out, 0, b"\x1b[c");
    std::thread::sleep(Duration::from_millis(300));
    tty.write_all(b"\r").unwrap();
    let shown = wait_for(&out, 0, b"a=p,");
    tty.write_all(b"\x10").unwrap();
    let list = wait_for(&out, shown, b"A title");
    tty.write_all(b"\x1b").unwrap();
    wait_for(&out, list, b"a=p,");
    let alive = exits_within(&mut child, Duration::from_millis(500)).is_none();
    tty.write_all(b"q").unwrap();
    let first = exits_within(&mut child, Duration::from_secs(5));
    let Viewer { mut child, mut tty, out } = start(&[dir.to_str().unwrap()], None, 80, &[], false);
    wait_for(&out, 0, b"A title");
    wait_for(&out, 0, b"\x1b[c");
    std::thread::sleep(Duration::from_millis(300));
    tty.write_all(b"\x1b").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(alive, "Esc in a list opened from a file quit");
    assert_eq!(first, Some(true));
    assert_eq!(quit, Some(true), "Esc in the startup list did not quit");
}

/// The page pixels the viewer sent, stitched from its tile dumps, one row per `Vec`.
fn stitched(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let mut tiles: Vec<(u32, u32, std::path::PathBuf)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            let name = p.file_stem()?.to_str()?.to_string();
            // `{id}-{top}`. Bands and the like carry a tag in front of the id.
            let (id, top) = name.split_once('-')?;
            Some((top.parse().ok()?, id.parse().ok()?, p))
        })
        .collect();
    tiles.sort();
    let mut rows: Vec<Vec<u8>> = Vec::new();
    for (top, _, path) in tiles {
        // A tile still being written doesn't decode yet, so the page stops before it.
        let Ok(mut r) = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).read_info() else { break };
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let Ok(info) = r.next_frame(&mut buf) else { break };
        let line = info.line_size;
        let top = top as usize;
        // A tile is sent with the next tile's first cell row below it, which a view between cells draws. It must
        // be the same pixels the next tile starts with.
        for (y, row) in rows.iter().enumerate().skip(top) {
            assert_eq!(*row, buf[(y - top) * line..(y - top + 1) * line], "the row sent below a tile differs from the next tile at page row {y}");
        }
        rows.truncate(top);
        if rows.len() < top {
            break;
        }
        rows.extend(buf[..info.height as usize * line].chunks(line).map(<[u8]>::to_vec));
    }
    rows
}

#[test]
fn tiles_line_up_at_an_odd_cell_height() {
    // At an odd cell height and scale 2, every other tile starts on a half CSS pixel. WebKit snaps such a rect, so
    // without care those tiles come out a device pixel off, and a heading in one looks cut at the top. The page the
    // tiles make must be the same pixels as at an even cell height.
    let dir = scratch("tiles");
    let md = dir.join("doc.md");
    let body: String = (1..=40).map(|i| format!("## Heading {i}\n\nA paragraph of text under heading {i}, long enough to wrap once.\n\n")).collect();
    std::fs::write(&md, body).unwrap();
    let cfg = dir.to_str().unwrap().to_string();
    let mut pages = Vec::new();
    for h in [43, 42] {
        let dump = dir.join(format!("tiles-{h}"));
        std::fs::create_dir_all(&dump).unwrap();
        let mut v = start_exe(env!("CARGO_BIN_EXE_mdd").as_ref(), &[md.to_str().unwrap()], None, 108, (22, h), &[("XDG_CONFIG_HOME", &cfg), ("MDDOG_DUMP_TILES", dump.to_str().unwrap())], false);
        let end = Instant::now() + Duration::from_secs(10);
        while stitched(&dump).len() < 3000 && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
        }
        v.tty.write_all(b"q").unwrap();
        let _ = exits_within(&mut v.child, Duration::from_secs(5));
        let _ = v.child.kill();
        // 108 columns of 22 px make 19-row tiles, so at 43 px every other tile starts on an odd device pixel.
        let odd = std::fs::read_dir(&dump).unwrap().any(|e| e.unwrap().path().file_stem().and_then(|s| s.to_str()?.rsplit_once('-')?.1.parse::<u32>().ok()).is_some_and(|top| top % 2 == 1));
        assert!(odd || h % 2 == 0, "no tile started on an odd pixel, so the test doesn't cover the case");
        pages.push(stitched(&dump));
    }
    let _ = std::fs::remove_dir_all(&dir);
    let n = pages[0].len().min(pages[1].len());
    assert!(n >= 3000, "only {n} rows of tiles were sent");
    let first = (0..n).find(|&y| pages[0][y] != pages[1][y]);
    assert_eq!(first, None, "the tiles differ from page row {first:?} on");
}

/// Sends `mouse` with `copy_on_select` set to `on`. Returns the text of each
/// OSC 52 copy the viewer wrote, and what it sent to a stand-in pbcopy.
fn mouse_copies(on: bool, mouse: &[u8]) -> (Vec<String>, String) {
    let dir = scratch(&format!("select-{on}-{}", mouse.len()));
    std::fs::write(dir.join("mddog/config.toml"), format!("copy_on_select = {on}\n")).unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(bin.join("pbcopy"), format!("#!/bin/sh\ncat >> '{}'\n", dir.join("clip").display())).unwrap();
    std::fs::set_permissions(bin.join("pbcopy"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let file = dir.join("a.md");
    std::fs::write(&file, "alpha beta gamma\n\ndelta epsilon zeta\n").unwrap();
    let env = [("XDG_CONFIG_HOME", dir.to_str().unwrap()), ("PATH", &path)];
    let Viewer { mut child, mut tty, out } = open(file.to_str().unwrap(), 80, &env);
    wait_for(&out, 0, b"a=p,");
    let before = out.lock().unwrap().len();
    tty.write_all(mouse).unwrap();
    std::thread::sleep(Duration::from_millis(800));
    tty.write_all(b"q").unwrap();
    let _ = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    let clip = std::fs::read_to_string(dir.join("clip")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    let all = out.lock().unwrap()[before..].to_vec();
    let copies = String::from_utf8_lossy(&all)
        .split("\x1b]52;c;")
        .skip(1)
        .map(|s| {
            let b64 = s.split('\x1b').next().unwrap();
            let mut dec = Command::new("base64").arg("-D").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
            dec.stdin.take().unwrap().write_all(b64.as_bytes()).unwrap();
            String::from_utf8(dec.wait_with_output().unwrap().stdout).unwrap()
        })
        .collect();
    (copies, clip)
}

const DRAG: &[u8] = b"\x1b[<0;2;1M\x1b[<32;40;3M\x1b[<32;79;6M\x1b[<0;79;6m";

#[test]
fn copy_on_select_copies_a_drag() {
    let (copies, clip) = mouse_copies(true, DRAG);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(copies[0].contains("alpha beta gamma") && copies[0].contains("delta epsilon zeta"), "{copies:?}");
    assert_eq!(clip, copies[0], "pbcopy and OSC 52 got different text");
    assert_eq!(mouse_copies(false, DRAG), (vec![], String::new()), "a drag copied with copy_on_select off");
}

#[test]
fn copy_on_select_copies_a_double_clicked_word() {
    let (copies, clip) = mouse_copies(true, b"\x1b[<0;10;2M\x1b[<0;10;2m\x1b[<0;10;2M\x1b[<0;10;2m");
    assert_eq!(copies, ["beta"], "{copies:?}");
    assert_eq!(clip, "beta");
}

const CMD_C: &[u8] = b"\x1b[99;9u";

#[test]
fn cmd_c_copies_the_selection() {
    let (copies, clip) = mouse_copies(false, &[DRAG, CMD_C].concat());
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(copies[0].contains("alpha beta gamma") && copies[0].contains("delta epsilon zeta"), "{copies:?}");
    assert_eq!(clip, copies[0], "pbcopy and OSC 52 got different text");
    assert_eq!(mouse_copies(false, CMD_C), (vec![], String::new()), "Cmd-C copied with nothing selected");
}

#[test]
fn kitty_ctrl_c_quits_and_pops_the_keyboard_flags() {
    let Viewer { mut child, mut tty, out } = open(GFM, 80, &[]);
    let pushed = wait_for(&out, 0, b"\x1b[>1u");
    assert!(pushed > wait_for(&out, 0, b"\x1b[?1049h"), "the flags were pushed on the main screen");
    wait_for(&out, 0, b"a=p,");
    let start = out.lock().unwrap().len();
    tty.write_all(b"\x1b[99;5u").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    assert_eq!(quit, Some(true), "Ctrl-C as CSI 99;5u did not quit");
    let tail = out.lock().unwrap()[start..].to_vec();
    let popped = find(&tail, b"\x1b[<1u").expect("the keyboard flags were not popped");
    assert!(popped < find(&tail, b"\x1b[?1049l").unwrap(), "the flags were popped after leaving the alternate screen");
}

#[test]
fn the_pointer_shape_follows_the_mouse() {
    let dir = scratch("pointer");
    let file = dir.join("a.md");
    let words = "lorem ipsum dolor sit amet consectetur adipiscing elit ".repeat(200);
    std::fs::write(&file, format!("{words}\n\n[{words}](https://example.com)\n")).unwrap();
    let Viewer { mut child, mut tty, out } = open(file.to_str().unwrap(), 80, &[]);
    let drawn = wait_for(&out, 0, b"a=p,");
    tty.write_all(b"\x1b[<35;40;4M\x1b[<35;40;4M\x1b[<35;41;4M").unwrap();
    let text = wait_for(&out, drawn, b"\x1b]22;xterm\x1b\\");
    tty.write_all(b"?").unwrap();
    let help = wait_for(&out, text, b"\x1b]22;left_ptr\x1b\\");
    tty.write_all(b"?").unwrap();
    let text = wait_for(&out, help, b"\x1b]22;xterm\x1b\\");
    tty.write_all(b"G").unwrap();
    let link = wait_for(&out, text, b"\x1b]22;pointer\x1b\\");
    tty.write_all(b"g\x1b[<35;40;40M").unwrap();
    let status = wait_for(&out, link, b"\x1b]22;left_ptr\x1b\\");
    tty.write_all(b"\x1b[<35;40;4M").unwrap();
    let back = wait_for(&out, status, b"\x1b]22;xterm\x1b\\");
    tty.write_all(b"\x10").unwrap();
    let list = wait_for(&out, back, b"\x1b]22;left_ptr\x1b\\");
    tty.write_all(b"\x1b[<35;40;4M\x1bq").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
    let first = out.lock().unwrap()[drawn..help].to_vec();
    assert_eq!(first.windows(9).filter(|w| *w == b"\x1b]22;xter").count(), 1, "a move within the same spot or target sent the shape again");
    assert!(find(&out.lock().unwrap()[list..], b"\x1b]22;\x1b\\").is_some(), "quitting did not reset the pointer");
    assert_eq!(quit, Some(true));
}

#[test]
fn a_wheel_before_any_move_sets_the_pointer_shape() {
    let dir = scratch("wheel-pointer");
    let file = dir.join("a.md");
    std::fs::write(&file, "lorem ipsum dolor sit amet ".repeat(400)).unwrap();
    let Viewer { mut child, mut tty, out } = open(file.to_str().unwrap(), 80, &[]);
    let drawn = wait_for(&out, 0, b"a=p,");
    tty.write_all(b"\x1b[<65;40;4M").unwrap();
    wait_for(&out, drawn, b"\x1b]22;xterm\x1b\\");
    tty.write_all(b"q").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(quit, Some(true));
}

#[test]
fn a_file_opened_by_a_click_hit_tests_the_still_pointer() {
    let dir = scratch("click-open");
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(dir.join("a.md"), "lorem ipsum dolor sit amet ".repeat(400)).unwrap();
    let Viewer { mut child, mut tty, out } = start(&[], Some(&dir), 80, &[], false);
    let probe = wait_for(&out, 0, b"\x1b[c");
    let list = wait_for(&out, probe, b"a.md");
    tty.write_all(b"\x1b[<0;40;4M\x1b[<0;40;4m").unwrap();
    let shown = wait_for(&out, list, b"a=p,");
    wait_for(&out, shown, b"\x1b]22;xterm\x1b\\");
    tty.write_all(b"q").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(quit, Some(true));
}

#[test]
fn buttons_show_the_hand() {
    let dir = scratch("buttons");
    let file = dir.join("a.md");
    std::fs::write(&file, "```rust\nfn main() {}\n```\n\n```mermaid\ngraph LR\n  A --> B\n```\n").unwrap();
    std::fs::create_dir_all(dir.join("mddog")).unwrap();
    std::fs::write(dir.join("mddog/config.toml"), "theme = \"github-dark\"\n").unwrap();
    let Viewer { mut child, mut tty, out } = open(file.to_str().unwrap(), 80, &[("XDG_CONFIG_HOME", dir.to_str().unwrap())]);
    let drawn = wait_for(&out, 0, b"a=p,");
    // Text, then the code copy, Mermaid copy and Mermaid expand buttons, with text between.
    let mut at = drawn;
    for button in [&b"\x1b[<35;74;3M"[..], b"\x1b[<35;75;6M", b"\x1b[<35;70;6M"] {
        tty.write_all(b"\x1b[<35;5;3M").unwrap();
        at = wait_for(&out, at, b"\x1b]22;xterm\x1b\\");
        tty.write_all(button).unwrap();
        at = wait_for(&out, at, b"\x1b]22;pointer\x1b\\");
    }
    tty.write_all(b"q").unwrap();
    let quit = exits_within(&mut child, Duration::from_secs(5));
    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(quit, Some(true));
}
