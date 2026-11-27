//! OSC 7577 resume reports.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

const VAR: &str = "OSC7577_TOKEN";
/// The spec's minimum a terminal accepts, for the whole sequence.
const LIMIT: usize = 2048;
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

static TOKEN: OnceLock<Option<String>> = OnceLock::new();
static PROGRAM: OnceLock<String> = OnceLock::new();

/// Takes `OSC7577_TOKEN` out of the environment. Call before any thread starts.
pub fn take_token() {
    let token = std::env::var(VAR).ok().filter(|t| !t.is_empty());
    if std::env::var_os(VAR).is_some() {
        // SAFETY: called first in `main`, before any other thread exists.
        unsafe { std::env::remove_var(VAR) };
    }
    let _ = TOKEN.set(token);
}

pub fn token() -> Option<&'static str> {
    TOKEN.get().and_then(Option::as_deref)
}

pub fn set_program(arg0: Option<&OsStr>) {
    let _ = PROGRAM.set(program(arg0));
}

pub fn program_name() -> &'static str {
    PROGRAM.get().map_or("mdd", String::as_str)
}

/// `argv[0]` made absolute when it is a path, else `mdd`. Symlinks stay as given.
pub fn program(arg0: Option<&OsStr>) -> String {
    match arg0.and_then(OsStr::to_str) {
        Some(a) if a.contains('/') => std::path::absolute(a).ok().and_then(|p| p.into_os_string().into_string().ok()).unwrap_or_else(|| "mdd".into()),
        _ => "mdd".into(),
    }
}

/// `p` made absolute, with no `.` or `..`. Symlinks stay as given unless `..` follows one.
pub fn path(p: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(p).ok()?;
    let mut out = PathBuf::new();
    for c in std::path::absolute(p).ok()?.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    Some(if std::fs::canonicalize(&out).is_ok_and(|c| c == real) { out } else { real })
}

/// Seconds since the Unix epoch, or `None` for a clock at or before it.
pub fn now() -> Option<u64> {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs()).filter(|&t| t > 0)
}

pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(B64[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out
}

/// The full report sequence, or `None` when the command can't be reported.
pub fn report(pid: u32, time: u64, argv: &[&str], token: &str) -> Option<String> {
    if argv.first().is_none_or(|a| a.is_empty()) || argv.iter().any(|a| a.chars().any(char::is_control)) {
        return None;
    }
    let args: Vec<String> = argv.iter().map(|a| base64(a.as_bytes())).collect();
    let seq = sign(&format!("pid={pid}:time={time}:argv={}", args.join(",")), token);
    (seq.len() <= LIMIT).then_some(seq)
}

pub fn clear(pid: u32, time: u64, token: &str) -> String {
    sign(&format!("pid={pid}:time={time}:clear=1"), token)
}

fn sign(body: &str, token: &str) -> String {
    let mac = Sha256::new().chain_update(body).chain_update(":").chain_update(token).finalize();
    let hex = mac.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    format!("\x1b]7577;{body}:mac={hex}\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "dummy-token-replace-me";

    #[test]
    fn the_spec_neovim_report() {
        let got = report(74877, 1_795_543_200, &["nvim", "-S", "/home/user/.local/state/nvim/sessions/8f2c1d.vim"], TOKEN);
        let want = "\x1b]7577;pid=74877:time=1795543200:argv=bnZpbQ==,LVM=,L2hvbWUvdXNlci8ubG9jYWwvc3RhdGUvbnZpbS9zZXNzaW9ucy84ZjJjMWQudmlt:mac=9dbb99bfb856749a87563839d7ce9062818d78cf25909db93f309e0cd3b1909b\x1b\\";
        assert_eq!(got.as_deref(), Some(want));
    }

    #[test]
    fn the_spec_clear_report() {
        let want = "\x1b]7577;pid=74877:time=1795543200:clear=1:mac=c30603369f98fef581a4098be889175b0c74f0641e49f8b25f4326cb38f97c93\x1b\\";
        assert_eq!(clear(74877, 1_795_543_200, TOKEN), want);
    }

    #[test]
    fn the_spec_wrapper_report() {
        let got = report(90412, 1_795_543_200, &["resume-wrap", "ssh", "devbox"], TOKEN);
        let want = "\x1b]7577;pid=90412:time=1795543200:argv=cmVzdW1lLXdyYXA=,c3No,ZGV2Ym94:mac=e76149d0ba54a83570ffab0583867dacdfa23928ba9d18604f0badea88b62b65\x1b\\";
        assert_eq!(got.as_deref(), Some(want));
    }

    #[test]
    fn base64_pads() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn the_program_is_mdd_or_an_absolute_path() {
        assert_eq!(program(Some(OsStr::new("mdd"))), "mdd");
        assert_eq!(program(None), "mdd");
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(program(Some(OsStr::new("./target/debug/mdd"))), cwd.join("target/debug/mdd").to_str().unwrap());
        assert_eq!(program(Some(OsStr::new("/opt/homebrew/bin/mdd"))), "/opt/homebrew/bin/mdd");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mddog-resume-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_path_drops_dot_and_dot_dot() {
        let dir = scratch("dots");
        std::fs::create_dir_all(dir.join("proj/a")).unwrap();
        std::fs::write(dir.join("notes.md"), "").unwrap();
        std::fs::write(dir.join("proj/a/b.md"), "").unwrap();
        let p = |s: &str| path(&dir.join(s));
        assert_eq!(p("proj/../notes.md"), Some(dir.join("notes.md")));
        assert_eq!(p("./proj/./a/./b.md"), Some(dir.join("proj/a/b.md")));
        assert_eq!(p("proj/a/../../proj"), Some(dir.join("proj")));
        assert_eq!(p("missing/../notes.md"), None);
        assert_eq!(p("gone.md"), None);
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(path(Path::new("src/../Cargo.toml")), Some(cwd.join("Cargo.toml")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dot_dot_after_a_symlink_resolves_it() {
        let dir = scratch("link-dots");
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join("deep/inner")).unwrap();
        std::fs::write(dir.join("docs/a.md"), "").unwrap();
        std::fs::write(dir.join("deep/a.md"), "").unwrap();
        std::os::unix::fs::symlink("../deep/inner", dir.join("docs/link")).unwrap();
        std::os::unix::fs::symlink("deep/a.md", dir.join("named.md")).unwrap();
        let real = std::fs::canonicalize(dir.join("deep/a.md")).unwrap();
        assert_eq!(path(&dir.join("docs/link/../a.md")), Some(real));
        assert_eq!(path(&dir.join("docs/link/../../named.md")), Some(dir.join("named.md")));
        assert_eq!(path(&dir.join("docs/../named.md")), Some(dir.join("named.md")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_program_name_that_is_not_utf8_is_mdd() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(program(Some(OsStr::from_bytes(b"/bin/m\xffd"))), "mdd");
    }

    #[test]
    fn a_command_that_cannot_be_reported_is_none() {
        assert_eq!(report(1, 1, &["mdd", "/tmp/a\nb.md"], TOKEN), None);
        assert_eq!(report(1, 1, &["mdd", "/tmp/a\u{85}b.md"], TOKEN), None);
        assert_eq!(report(1, 1, &["mdd", "/tmp/a\u{7f}b.md"], TOKEN), None);
        assert_eq!(report(1, 1, &[""], TOKEN), None);
        assert_eq!(report(1, 1, &[], TOKEN), None);
        let path = format!("/{}", "a".repeat(1457));
        assert_eq!(report(1000, 1, &["mdd", &path], TOKEN).map(|s| s.len()), Some(2048));
        assert_eq!(report(10000, 1, &["mdd", &path], TOKEN), None);
        assert!(report(1, 1, &["mdd", "/tmp/a\u{202e}b.md"], TOKEN).is_some());
    }
}
