use std::path::{Path, PathBuf};

pub mod remote;

/// The page's padding in CSS pixels. The loading skeleton uses it too.
pub const PAD: f64 = 24.0;

pub fn base_dir(file: &Path, stdin: bool) -> PathBuf {
    if stdin { std::env::current_dir().ok() } else { std::fs::canonicalize(file).ok().and_then(|p| p.parent().map(Path::to_path_buf)) }.unwrap_or_else(|| PathBuf::from("."))
}
