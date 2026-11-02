//! Removes what mddog processes left behind when they died without cleaning up, as a SIGKILL
//! does: temp files and folders whose name holds a pid that is gone, and remote images past
//! their lifetime.

use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

/// Sweeps on a side thread, so it never holds up a paint.
pub fn spawn() {
    let _ = std::thread::Builder::new().name("sweep".into()).spawn(|| {
        crate::trace::mark("sweep started");
        temp(&std::env::temp_dir());
        remote(&crate::page::remote::disk_dir(), SystemTime::now());
        crate::trace::mark("swept");
    });
}

/// A temp path this run owns: `<prefix><pid>-<nonce><suffix>`. `temp_owner` must match each prefix and suffix, or
/// what a killed run leaves never goes.
pub fn temp_path(prefix: &str, suffix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}{}-{:x}{suffix}", std::process::id(), nonce()))
}

pub fn nonce() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static N: OnceLock<u64> = OnceLock::new();
    *N.get_or_init(|| std::collections::hash_map::RandomState::new().build_hasher().finish())
}

pub fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    let _ = std::fs::remove_file(path);
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?.write_all(data)
}

/// The pid that owns a temp entry, from its name. Only names mddog itself makes match.
fn temp_owner(name: &str) -> Option<i32> {
    if let Some(rest) = name.strip_prefix("mddog-tiles-") {
        return pid_nonce(rest);
    }
    if let Some(rest) = name.strip_prefix("tty-graphics-protocol-mddog-") {
        return pid_nonce(rest.rsplit_once('-')?.0);
    }
    name.strip_prefix("mddog-")?.strip_suffix("-stdin.md").and_then(pid_nonce)
}

/// The number in `<number>-<lowercase hex>`.
fn pid_nonce(s: &str) -> Option<i32> {
    let (n, nonce) = s.split_once('-')?;
    let hex = !nonce.is_empty() && nonce.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (hex && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| n.parse().ok()).flatten()
}

/// Only ESRCH proves a pid is gone. A reused pid keeps the entry until that process ends too.
fn dead(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the pid exists; it sends nothing.
    pid > 0 && unsafe { libc::kill(pid, 0) } == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn remove(path: &Path, meta: &std::fs::Metadata) {
    let _ = if meta.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
}

/// This process's real user id.
pub fn uid() -> u32 {
    // SAFETY: getuid takes no arguments, cannot fail, and touches no memory.
    unsafe { libc::getuid() }
}

pub fn mine(meta: &std::fs::Metadata) -> bool {
    meta.uid() == uid()
}

fn temp(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(temp_owner) else { continue };
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if mine(&meta) && dead(pid) {
            remove(&path, &meta);
        }
    }
}

/// Removes remote cache files past the longest lifetime, so expired and half-written ones, then the oldest until
/// the folder fits in `DISK_BYTES`.
fn remote(dir: &Path, now: SystemTime) {
    use crate::page::remote::{DISK_BYTES, KEEP_MAX};
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let meta = std::fs::symlink_metadata(e.path()).ok().filter(|m| m.is_file() && mine(m))?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    for (modified, len, path) in files {
        let old = now.duration_since(modified).is_ok_and(|age| age > KEEP_MAX);
        if !old && total <= DISK_BYTES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A pid that is not running: spawn a child, wait for it, and keep its pid.
    fn gone_pid() -> i32 {
        let mut c = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = c.id() as i32;
        c.wait().unwrap();
        pid
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mddog-sweep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn set_age(path: &Path, age: Duration) {
        std::fs::File::open(path).unwrap().set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn remote_cache_drops_expired_then_oldest() {
        let d = dir("remote");
        let big = vec![0u8; (crate::page::remote::DISK_BYTES / 2) as usize];
        let files = [("expired", 2 * 3600, &b"x"[..]), ("oldest", 120, &big[..]), ("older", 60, &big[..]), ("newest", 0, &big[..])];
        for (name, age, body) in files {
            std::fs::write(d.join(name), body).unwrap();
            set_age(&d.join(name), Duration::from_secs(age));
        }
        remote(&d, SystemTime::now());
        let left: Vec<_> = files.iter().map(|f| f.0).filter(|n| d.join(n).exists()).collect();
        assert_eq!(left, ["older", "newest"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn matches_only_names_mddog_makes() {
        for name in ["mddog-tiles-12-ab3f", "mddog-12-ab3f-stdin.md", "tty-graphics-protocol-mddog-12-ab3f-q"] {
            assert_eq!(temp_owner(name), Some(12), "{name}");
        }
        let me = std::process::id() as i32;
        let made = [("mddog-", "-stdin.md"), ("mddog-tiles-", ""), ("tty-graphics-protocol-mddog-", "-q")];
        for path in made.map(|(prefix, suffix)| temp_path(prefix, suffix)) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(temp_owner(&name), Some(me), "{name}");
        }
        for name in [
            "mddog-12-ab3f.html",
            "mddog-tiles-test-12",
            "mddog-tiles-storage-limit-12",
            "mddog-search-test-12.html",
            "mddog-sizes-round-12",
            "mddog-probe.html",
            "mddog12",
            "mddog-12-AB-stdin.md",
            "mddog-12-ab3f-stdin.md.bak",
            "other-12-ab3f-stdin.md",
            "xmddog-12-ab3f-stdin.md",
        ] {
            assert!(temp_owner(name).is_none(), "{name}");
        }
    }

    #[test]
    fn removes_what_dead_processes_left_and_nothing_else() {
        let d = dir("temp");
        let (gone, live) = (gone_pid(), std::process::id());
        let tiles_gone = d.join(format!("mddog-tiles-{gone}-ab"));
        let tiles_live = d.join(format!("mddog-tiles-{live}-ab"));
        for t in [&tiles_gone, &tiles_live] {
            std::fs::create_dir(t).unwrap();
            std::fs::write(t.join("1-0.png"), b"x").unwrap();
        }
        let files_gone: Vec<_> = [format!("mddog-{gone}-ab-stdin.md"), format!("tty-graphics-protocol-mddog-{gone}-ab-q")].iter().map(|n| d.join(n)).collect();
        let stdin_live = d.join(format!("mddog-{live}-ab-stdin.md"));
        let foreign = d.join(format!("other-{gone}-ab-stdin.md"));
        for f in files_gone.iter().chain([&stdin_live, &foreign]) {
            std::fs::write(f, b"x").unwrap();
        }

        temp(&d);

        assert!(!tiles_gone.exists());
        for f in &files_gone {
            assert!(!f.exists(), "{}", f.display());
        }
        for kept in [&tiles_live, &stdin_live, &foreign] {
            assert!(kept.exists(), "{}", kept.display());
        }
        std::fs::remove_dir_all(&d).unwrap();
    }
}
