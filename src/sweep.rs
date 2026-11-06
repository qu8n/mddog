//! Removes what mddog runs left behind when they died without cleaning up, as a SIGKILL does: temp
//! files and folders whose run is gone, and remote images past their lifetime.

use std::io;
use std::path::Path;
use std::time::SystemTime;

/// Sweeps on a side thread, so it never holds up a paint.
pub fn spawn() {
    let _ = std::thread::Builder::new().name("sweep".into()).spawn(|| {
        crate::trace::mark("sweep started");
        temp(&std::env::temp_dir());
        crate::render::remote::prune_disk(SystemTime::now());
        crate::trace::mark("swept");
    });
}

/// Only ESRCH proves a pid is gone. A reused pid keeps the entry until that process ends too.
fn dead(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the pid exists; it sends nothing.
    pid > 0 && unsafe { libc::kill(pid, 0) } == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn remove(path: &Path, meta: &std::fs::Metadata) {
    let _ = if meta.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
}

fn temp(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(crate::tmp::owner) else { continue };
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if crate::tmp::mine(&meta) && dead(pid) {
            remove(&path, &meta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
