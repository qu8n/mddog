use std::path::PathBuf;
use std::time::SystemTime;

use include_dir::{Dir, include_dir};

static FILES: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets/runtime");

/// This build's runtime folder name. Other builds keep their own, which the sweep removes after a week unused.
pub const RUNTIME: &str = concat!("runtime-", env!("MDDOG_RUNTIME_HASH"));

/// The cache root: `MDDOG_CACHE_DIR` when set, so tests keep out of the real one, else `~/Library/Caches/mddog`.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MDDOG_CACHE_DIR").filter(|d| !d.is_empty()) {
        // Absolute, since the spare runs in `/` and its socket name hashes this path.
        return std::path::absolute(&dir).unwrap_or_else(|_| PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    home.join("Library/Caches/mddog")
}

/// Called when a page needs math or Mermaid, and by the spare. Each call marks the folder as in use for the sweep.
pub fn runtime_dir() -> PathBuf {
    let dir = cache_dir().join(RUNTIME);
    if let Ok(f) = std::fs::File::open(&dir) {
        let _ = f.set_modified(SystemTime::now());
    }
    if dir.join(".complete").exists() {
        return dir;
    }
    let tmp = dir.with_extension(format!("tmp{}", std::process::id()));
    let written = std::fs::create_dir_all(&tmp).is_ok() && FILES.extract(&tmp).is_ok() && std::fs::write(tmp.join(".complete"), b"").is_ok();
    if !written {
        let _ = std::fs::remove_dir_all(&tmp);
        return dir;
    }
    if std::fs::rename(&tmp, &dir).is_err() {
        if !dir.join(".complete").exists() {
            let _ = std::fs::remove_dir_all(&dir);
            if std::fs::rename(&tmp, &dir).is_ok() {
                return dir;
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
    dir
}
