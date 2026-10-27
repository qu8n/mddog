//! Removes what mddog processes left behind when they died without cleaning up, as a SIGKILL
//! does: temp files and folders whose name holds a pid that is gone, spare sockets nobody
//! listens on, and runtime folders no build has used for a week.

use std::io::{self, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// A `runtime-<hash>` folder that no run has touched for this long goes.
const RUNTIME_KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// A spare binds its socket a moment before it listens, so a younger socket may still be live.
const SOCKET_GRACE: Duration = Duration::from_secs(10);

/// Sweeps on a side thread, so it never holds up a paint.
pub fn spawn() {
    let _ = std::thread::Builder::new().name("sweep".into()).spawn(|| {
        crate::trace::mark("sweep started");
        temp(&std::env::temp_dir());
        cache(&crate::assets::cache_dir(), crate::assets::RUNTIME, SystemTime::now());
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

enum Owner {
    Pid(i32),
    Socket,
}

/// Who owns a temp entry, from its name. Only names mddog itself makes match.
fn temp_owner(name: &str) -> Option<Owner> {
    if let Some(rest) = name.strip_prefix("mddog-tiles-") {
        return pid_nonce(rest).map(Owner::Pid);
    }
    if let Some(rest) = name.strip_prefix("tty-graphics-protocol-mddog-") {
        return pid_nonce(rest.rsplit_once('-')?.0).map(Owner::Pid);
    }
    let rest = name.strip_prefix("mddog-")?;
    if let Some(s) = rest.strip_suffix(".sock") {
        return (pid_nonce(s)? as u32 == uid()).then_some(Owner::Socket);
    }
    ["-warm.html", "-stdin.md", ".html"].iter().find_map(|suffix| rest.strip_suffix(suffix)).and_then(pid_nonce).map(Owner::Pid)
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
        let Some(owner) = e.file_name().to_str().and_then(temp_owner) else { continue };
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !mine(&meta) {
            continue;
        }
        let stale = match owner {
            Owner::Pid(pid) => dead(pid),
            Owner::Socket => {
                meta.file_type().is_socket()
                    && meta.modified().is_ok_and(|m| m.elapsed().is_ok_and(|age| age > SOCKET_GRACE))
                    && UnixStream::connect(&path).is_err_and(|e| e.kind() == io::ErrorKind::ConnectionRefused)
                    // A new spare may have replaced the dead socket since the stat.
                    && std::fs::symlink_metadata(&path).is_ok_and(|m| (m.dev(), m.ino()) == (meta.dev(), meta.ino()))
            }
        };
        if stale {
            remove(&path, &meta);
        }
    }
}

fn runtime_name(name: &str) -> bool {
    name.strip_prefix("runtime-").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `current` is this build's runtime folder name, which always stays.
fn cache(dir: &Path, current: &str, now: SystemTime) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(name) = e.file_name().to_str().map(str::to_owned) else { continue };
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !mine(&meta) {
            continue;
        }
        // A half-written runtime folder or sizes file from a run that died.
        if let Some((stem, pid)) = name.rsplit_once(".tmp") {
            if (stem == "diagram-sizes" || stem == "image-sizes" || runtime_name(stem)) && pid.bytes().all(|b| b.is_ascii_digit()) && pid.parse().is_ok_and(dead) {
                remove(&path, &meta);
            }
            continue;
        }
        let unused = |m: &std::fs::Metadata| m.modified().is_ok_and(|m| now.duration_since(m).is_ok_and(|age| age > RUNTIME_KEEP));
        if name != current && runtime_name(&name) && meta.is_dir() && unused(&meta) {
            // A run that finds the folder gone mid-sweep unpacks it again, rather than using half of it.
            let doomed = path.with_extension(format!("tmp{}", std::process::id()));
            if std::fs::rename(&path, &doomed).is_ok() {
                // A run that touched it between the stat and the rename is using it, so it goes back.
                if std::fs::metadata(&doomed).is_ok_and(|m| unused(&m)) {
                    let _ = std::fs::remove_dir_all(&doomed);
                } else {
                    let _ = std::fs::rename(&doomed, &path);
                }
            }
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
    use std::os::unix::net::UnixListener;

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
        let uid = uid();
        for name in ["mddog-tiles-12-ab3f", "mddog-12-ab3f.html", "mddog-12-ab3f-warm.html", "mddog-12-ab3f-stdin.md", "tty-graphics-protocol-mddog-12-ab3f-q"] {
            assert!(matches!(temp_owner(name), Some(Owner::Pid(12))), "{name}");
        }
        assert!(matches!(temp_owner(&format!("mddog-{uid}-00ff.sock")), Some(Owner::Socket)));
        let me = std::process::id() as i32;
        let made = [("mddog-", ".html"), ("mddog-", "-stdin.md"), ("mddog-", "-warm.html"), ("mddog-tiles-", ""), ("tty-graphics-protocol-mddog-", "-q")];
        for path in made.map(|(prefix, suffix)| temp_path(prefix, suffix)) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(matches!(temp_owner(&name), Some(Owner::Pid(p)) if p == me), "{name}");
        }
        for name in [
            &format!("mddog-{}-00ff.sock", uid + 1),
            "mddog-tiles-test-12",
            "mddog-tiles-storage-limit-12",
            "mddog-search-test-12.html",
            "mddog-sizes-round-12",
            "mddog-probe.html",
            "mddog12",
            "mddog-12-AB.html",
            "mddog-12-ab3f.html.bak",
            "other-12-ab3f.html",
            "xmddog-12-ab3f.html",
        ] {
            assert!(temp_owner(name).is_none(), "{name}");
        }
    }

    #[test]
    fn removes_what_dead_processes_left_and_nothing_else() {
        let d = dir("temp");
        let (gone, live) = (gone_pid(), std::process::id());
        let uid = uid();
        let tiles_gone = d.join(format!("mddog-tiles-{gone}-ab"));
        let tiles_live = d.join(format!("mddog-tiles-{live}-ab"));
        for t in [&tiles_gone, &tiles_live] {
            std::fs::create_dir(t).unwrap();
            std::fs::write(t.join("1-0.png"), b"x").unwrap();
        }
        let files_gone: Vec<_> = [format!("mddog-{gone}-ab.html"), format!("mddog-{gone}-ab-warm.html"), format!("mddog-{gone}-ab-stdin.md"), format!("tty-graphics-protocol-mddog-{gone}-ab-q")]
            .iter()
            .map(|n| d.join(n))
            .collect();
        let html_live = d.join(format!("mddog-{live}-ab.html"));
        let foreign = d.join(format!("other-{gone}-ab.html"));
        for f in files_gone.iter().chain([&html_live, &foreign]) {
            std::fs::write(f, b"x").unwrap();
        }
        // A socket nobody listens on, one somebody does, and a dead one too new to judge.
        let sock_dead = d.join(format!("mddog-{uid}-01.sock"));
        let sock_live = d.join(format!("mddog-{uid}-02.sock"));
        let sock_new = d.join(format!("mddog-{uid}-03.sock"));
        drop(UnixListener::bind(&sock_dead).unwrap());
        drop(UnixListener::bind(&sock_new).unwrap());
        let _listener = UnixListener::bind(&sock_live).unwrap();
        // set_modified on a socket path needs utimes, not an open file.
        for s in [&sock_dead, &sock_live] {
            let old = libc::timeval { tv_sec: (SystemTime::now() - Duration::from_secs(60)).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i64, tv_usec: 0 };
            let c = std::ffi::CString::new(s.as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: c is a NUL-terminated path, and the array holds the two timevals utimes reads.
            assert_eq!(unsafe { libc::utimes(c.as_ptr(), [old, old].as_ptr()) }, 0);
        }

        temp(&d);

        assert!(!tiles_gone.exists());
        for f in &files_gone {
            assert!(!f.exists(), "{}", f.display());
        }
        assert!(!sock_dead.exists());
        for kept in [&tiles_live, &html_live, &foreign, &sock_live, &sock_new] {
            assert!(kept.exists(), "{}", kept.display());
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn keeps_runtime_folders_in_use_this_week() {
        let d = dir("cache");
        let gone = gone_pid();
        let week = Duration::from_secs(7 * 24 * 60 * 60);
        let old = d.join("runtime-00aa");
        let recent = d.join("runtime-00bb");
        let current = d.join("runtime-00cc");
        let half = d.join(format!("runtime-00dd.tmp{gone}"));
        let half_live = d.join(format!("runtime-00ee.tmp{}", std::process::id()));
        let sizes = d.join(format!("diagram-sizes.tmp{gone}"));
        let images = d.join(format!("image-sizes.tmp{gone}"));
        let other = d.join("runtime-notes");
        for f in [&old, &recent, &current, &half, &half_live, &other] {
            std::fs::create_dir(f).unwrap();
            std::fs::write(f.join(".complete"), b"").unwrap();
        }
        std::fs::write(&sizes, b"{}").unwrap();
        std::fs::write(&images, b"{}").unwrap();
        std::fs::write(d.join("diagram-sizes.json"), b"{}").unwrap();
        for f in [&old, &current, &other] {
            set_age(f, week + Duration::from_secs(60));
        }
        set_age(&recent, week - Duration::from_secs(60));

        cache(&d, "runtime-00cc", SystemTime::now());

        for gone in [&old, &half, &sizes, &images] {
            assert!(!gone.exists(), "{}", gone.display());
        }
        for kept in [&recent, &current, &half_live, &other, &d.join("diagram-sizes.json")] {
            assert!(kept.exists(), "{}", kept.display());
        }
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
