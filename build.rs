use std::fmt::Write as _;
use std::path::PathBuf;

/// Deflates each TextMate grammar into `OUT_DIR` (2.9 MB to 0.54 MB) and lists them, sorted by scope,
/// in `textmate.rs`.
fn grammars() {
    println!("cargo:rerun-if-changed=assets/textmate/grammars");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("textmate");
    std::fs::create_dir_all(&out).expect("OUT_DIR/textmate");
    let mut all: Vec<(String, PathBuf)> = std::fs::read_dir("assets/textmate/grammars")
        .expect("assets/textmate/grammars")
        .flatten()
        .map(|e| (e.path().file_stem().and_then(|s| s.to_str()).expect("grammar name").to_string(), e.path()))
        .collect();
    // By scope, not file name: `source.c` sorts before `source.c++`, but `source.c++.json` before `source.c.json`.
    all.sort();
    let mut table = String::from("static GRAMMARS: &[(&str, &[u8])] = &[\n");
    for (scope, f) in &all {
        let z = out.join(format!("{scope}.z"));
        std::fs::write(&z, miniz_oxide::deflate::compress_to_vec_zlib(&std::fs::read(f).expect("grammar"), 9)).expect("write grammar");
        let _ = writeln!(table, "    ({scope:?}, include_bytes!({:?})),", z.display().to_string());
    }
    table.push_str("];\n");
    std::fs::write(out.join("textmate.rs"), table).expect("textmate.rs");
}

fn main() {
    grammars();
}
