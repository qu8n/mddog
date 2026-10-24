use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::{RecursiveMode, Watcher};

pub fn spawn(file: &Path, tx: Sender<crate::term::Event>) -> Option<notify::RecommendedWatcher> {
    let abs = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let dir = abs.parent()?.to_path_buf();
    let name = abs.file_name()?.to_os_string();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if ev.paths.iter().any(|p: &PathBuf| p.file_name() == Some(name.as_os_str())) {
            let _ = tx.send(crate::term::Event::FileChanged);
        }
    })
    .ok()?;
    w.watch(&dir, RecursiveMode::NonRecursive).ok()?;
    Some(w)
}
