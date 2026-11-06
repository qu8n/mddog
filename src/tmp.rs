//! mddog's private temp files: their names, which hold the pid of the run that made them, and how they are
//! written.
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const TILES: &str = "mddog-tiles-";
const STDIN: (&str, &str) = ("mddog-", "-stdin.md");
const GRAPHICS: &str = "tty-graphics-protocol-mddog-";

/// A folder for headless mode's tiles.
pub fn tiles_dir() -> PathBuf {
    temp_path(TILES, "")
}

/// Where standard input is copied, to be shown as a file.
pub fn stdin_copy() -> PathBuf {
    temp_path(STDIN.0, STDIN.1)
}

/// A file the terminal reads an image from. `tag` sets it apart from the run's others.
pub fn graphics_file(tag: &str) -> PathBuf {
    temp_path(GRAPHICS, &format!("-{tag}"))
}

/// `<prefix><pid>-<nonce><suffix>`, which `temp_owner` reads back.
fn temp_path(prefix: &str, suffix: &str) -> PathBuf {
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
pub fn owner(name: &str) -> Option<i32> {
    if let Some(rest) = name.strip_prefix(TILES) {
        return pid_nonce(rest);
    }
    if let Some(rest) = name.strip_prefix(GRAPHICS) {
        return pid_nonce(rest.rsplit_once('-')?.0);
    }
    name.strip_prefix(STDIN.0)?.strip_suffix(STDIN.1).and_then(pid_nonce)
}

/// The number in `<number>-<lowercase hex>`.
fn pid_nonce(s: &str) -> Option<i32> {
    let (n, nonce) = s.split_once('-')?;
    let hex = !nonce.is_empty() && nonce.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (hex && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| n.parse().ok()).flatten()
}

/// Owned by this process's real user.
pub fn mine(meta: &std::fs::Metadata) -> bool {
    // SAFETY: getuid takes no arguments, cannot fail, and touches no memory.
    meta.uid() == unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_names_mddog_makes() {
        for name in ["mddog-tiles-12-ab3f", "mddog-12-ab3f-stdin.md", "tty-graphics-protocol-mddog-12-ab3f-q"] {
            assert_eq!(owner(name), Some(12), "{name}");
        }
        let me = std::process::id() as i32;
        for path in [stdin_copy(), tiles_dir(), graphics_file("q")] {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(owner(&name), Some(me), "{name}");
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
            assert!(owner(name).is_none(), "{name}");
        }
    }
}
