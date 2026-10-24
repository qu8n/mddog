use std::hash::Hasher;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("assets/runtime").flatten() {
        let p = e.path();
        if p.is_dir() { files(&p, out) } else { out.push(p) }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=assets/runtime");
    let mut all = Vec::new();
    files(Path::new("assets/runtime"), &mut all);
    all.sort();
    let mut hash = std::hash::DefaultHasher::new();
    for f in &all {
        hash.write(f.to_string_lossy().as_bytes());
        hash.write(&std::fs::read(f).expect("runtime asset"));
    }
    println!("cargo:rustc-env=MDDOG_RUNTIME_HASH={:016x}", hash.finish());
}
