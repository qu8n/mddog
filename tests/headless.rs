#![allow(clippy::unwrap_used, reason = "a failed step should fail the test")]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

/// A cache root for the app under test, so tests never write to the real one.
fn test_cache() -> std::path::PathBuf {
    std::env::temp_dir().join("mddog-test-cache")
}

fn png_size(path: &str) -> (u32, u32) {
    let b = std::fs::read(path).unwrap();
    (u32::from_be_bytes(b[16..20].try_into().unwrap()), u32::from_be_bytes(b[20..24].try_into().unwrap()))
}

fn term(pid: i32) {
    // SAFETY: kill takes plain integers and touches no memory.
    unsafe { libc::kill(pid, libc::SIGTERM) };
}

#[test]
fn tiles_follow_the_file() {
    let dir = std::env::temp_dir().join(format!("mddog-tiles-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.md");
    std::fs::write(&file, "# one\n\ntext\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdd")).env("MDDOG_CACHE_DIR", test_cache()).args(["--headless", "40"]).arg(&file).stdout(Stdio::piped()).spawn().unwrap();
    // A reader thread, so a render that never comes fails the test instead of hanging it.
    let (tx, rx) = std::sync::mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || BufReader::new(stdout).lines().map_while(Result::ok).try_for_each(|l| tx.send(l)));
    let next = || serde_json::from_str::<serde_json::Value>(&rx.recv_timeout(std::time::Duration::from_secs(20)).expect("no render within 20 s")).unwrap();
    let first = next();
    let tile = &first["tiles"][0];
    let (w, h) = png_size(tile["file"].as_str().unwrap());
    assert_eq!(w % 40, 0);
    assert_eq!(h % tile["rows"].as_u64().unwrap() as u32, 0);
    std::fs::write(&file, format!("# two\n\n{}", "line\n\n".repeat(200))).unwrap();
    let second = next();
    assert_ne!(first["tiles"][0]["file"], second["tiles"][0]["file"]);
    let rows: u64 = second["tiles"].as_array().unwrap().iter().map(|t| t["rows"].as_u64().unwrap()).sum();
    let first_rows: u64 = first["tiles"].as_array().unwrap().iter().map(|t| t["rows"].as_u64().unwrap()).sum();
    assert!(rows > first_rows, "{rows} rows after the edit, {first_rows} before");
    assert!(second["tiles"].as_array().unwrap().iter().all(|t| t["rows"].as_u64().unwrap() <= 255));
    let body = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    std::fs::write(&file, body).unwrap();
    let third = next();
    assert_ne!(second["tiles"][0]["file"], third["tiles"][0]["file"]);
    let tiles_dir = std::path::Path::new(third["tiles"][0]["file"].as_str().unwrap()).parent().unwrap().to_path_buf();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&tiles_dir).unwrap().permissions()) & 0o777, 0o700);
    term(child.id() as i32);
    child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!tiles_dir.exists(), "the tiles folder outlived the process");
}

#[test]
fn tiles_fit_the_herdr_storage_limit() {
    let dir = std::env::temp_dir().join(format!("mddog-tiles-storage-limit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.md");
    std::fs::write(&file, format!("# long\n\n{}", "line\n\n".repeat(300))).unwrap();
    let mut child =
        Command::new(env!("CARGO_BIN_EXE_mdd")).env("MDDOG_CACHE_DIR", test_cache()).args(["--headless", "200"]).arg(&file).env("TERM_PROGRAM", "herdr").stdout(Stdio::piped()).spawn().unwrap();
    let line = serde_json::from_str::<serde_json::Value>(&BufReader::new(child.stdout.take().unwrap()).lines().next().unwrap().unwrap()).unwrap();
    let sizes: Vec<_> = line["tiles"].as_array().unwrap().iter().map(|t| png_size(t["file"].as_str().unwrap())).collect();
    term(child.id() as i32);
    child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(line["keep"], 7);
    assert!(sizes.len() > 1);
    for (w, h) in sizes {
        assert!(w as usize * h as usize * 4 <= 8 << 20, "a {w}x{h} tile is over 8 MiB");
    }
}

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/plugins/claude-code/tests/headless.fixture.ts");

fn plugin_line(webp: bool) -> serde_json::Value {
    let dir = std::env::temp_dir().join(format!("mddog-contract-{}-{webp}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.md");
    std::fs::write(&file, "# one\n\ntext\n").unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_mdd"));
    c.env("MDDOG_CACHE_DIR", test_cache()).args(["--headless", "40"]);
    if webp {
        c.arg("--webp");
    }
    let mut child = c.arg("--").arg(&file).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = serde_json::from_str::<serde_json::Value>(&BufReader::new(child.stdout.take().unwrap()).lines().next().unwrap().unwrap()).unwrap();
    term(child.id() as i32);
    child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    for t in line["tiles"].as_array_mut().unwrap() {
        let name = |v: &serde_json::Value| format!("/tmp/mddog/{}", std::path::Path::new(v.as_str().unwrap()).file_name().unwrap().to_string_lossy());
        t["file"] = name(&t["file"]).into();
        if t.get("small").is_some() {
            t["small"]["file"] = name(&t["small"]["file"]).into();
        }
    }
    line
}

fn shape(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), shape(v))).collect()),
        Value::Array(a) => {
            let mut s: Vec<Value> = a.iter().map(shape).collect();
            s.sort_by_key(ToString::to_string);
            s.dedup();
            Value::Array(s)
        }
        Value::String(_) => "string".into(),
        Value::Number(_) => "number".into(),
        Value::Bool(_) => "bool".into(),
        Value::Null => Value::Null,
    }
}

fn fixture(name: &str) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(FIXTURE).ok()?;
    let prefix = format!("export const {name}: Out = ");
    text.lines().find_map(|l| l.strip_prefix(&prefix)).map(|j| serde_json::from_str(j).unwrap())
}

#[test]
fn the_plugin_fixture_matches_what_mdd_prints() {
    let real = [("png", plugin_line(false)), ("webp", plugin_line(true))];
    let ts = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/plugins/claude-code/hooks/headless.ts")).unwrap();
    for (_, line) in &real {
        let protocol = &line["protocol"];
        assert!(ts.contains(&format!("export const PROTOCOL = {protocol}\n")), "hooks/headless.ts doesn't say PROTOCOL = {protocol}, which mdd prints");
    }
    let drifted: Vec<_> = real.iter().filter(|(name, line)| fixture(name).is_none_or(|f| shape(&f) != shape(line) || f["protocol"] != line["protocol"])).collect();
    if drifted.is_empty() {
        return;
    }
    if std::env::var_os("MDDOG_BLESS").is_none() {
        let names: Vec<_> = drifted.iter().map(|(n, _)| *n).collect();
        panic!(
            "mdd --headless now prints a different line than {FIXTURE} holds for {names:?}. Run `MDDOG_BLESS=1 cargo test --test headless` to rewrite it, and bump PROTOCOL in src/headless.rs and hooks/headless.ts if the line's fields changed"
        );
    }
    for (name, line) in &drifted {
        if let Some(f) = fixture(name) {
            assert!(shape(&f) == shape(line) || f["protocol"] != line["protocol"], "the {name} line's fields changed, so bump PROTOCOL in src/headless.rs and hooks/headless.ts before blessing");
        }
    }
    let mut text = String::from("import type { Out } from '../hooks/headless'\n\n");
    for (name, line) in &real {
        text += &format!("export const {name}: Out = {line}\n");
    }
    std::fs::write(FIXTURE, text).unwrap();
}
