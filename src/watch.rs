use std::path::{Path, PathBuf};

use notify::{RecursiveMode, Watcher};

/// Calls `changed` on each change to `file`, including a save that renames another file over it.
pub fn spawn(file: &Path, changed: impl Fn() + Send + 'static) -> Option<notify::RecommendedWatcher> {
    let abs = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let dir = abs.parent()?.to_path_buf();
    let name = abs.file_name()?.to_os_string();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if ev.paths.iter().any(|p: &PathBuf| p.file_name() == Some(name.as_os_str())) {
            changed();
        }
    })
    .ok()?;
    w.watch(&dir, RecursiveMode::NonRecursive).ok()?;
    Some(w)
}
