#![expect(clippy::unwrap_used, reason = "a failed step should fail the test")]

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
    let mut child =
        Command::new(env!("CARGO_BIN_EXE_mdd")).env("MDDOG_CACHE_DIR", test_cache()).args(["--headless", "40"]).arg(&file).env("MDDOG_NO_SPARE", "1").stdout(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next = || serde_json::from_str::<serde_json::Value>(&lines.next().unwrap().unwrap()).unwrap();
    let first = next();
    assert_eq!(first["columns"], 40);
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdd"))
        .env("MDDOG_CACHE_DIR", test_cache())
        .args(["--headless", "200"])
        .arg(&file)
        .env("MDDOG_NO_SPARE", "1")
        .env("TERM_PROGRAM", "herdr")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
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

fn pids(args: &[&str]) -> Vec<i32> {
    let out = Command::new("pgrep").args(args).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()
}

/// Stops this binary's spares on drop, so a failed step does not leave one running for hours.
struct StopSpares;

impl Drop for StopSpares {
    fn drop(&mut self) {
        for pid in pids(&["-f", &format!("^{} --spare$", env!("CARGO_BIN_EXE_mdd"))]) {
            term(pid);
        }
    }
}

#[test]
fn the_spare_outlives_a_tree_kill() {
    let _stop = StopSpares;
    let dir = std::env::temp_dir().join(format!("mddog{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.md");
    std::fs::write(&file, "# one\n\ntext\n").unwrap();
    let tiles =
        |cols: &str| Command::new(env!("CARGO_BIN_EXE_mdd")).env("MDDOG_CACHE_DIR", test_cache()).args(["--headless", cols]).arg(&file).env("TMPDIR", &dir).stdout(Stdio::piped()).spawn().unwrap();
    let first_line = |child: &mut std::process::Child| serde_json::from_str::<serde_json::Value>(&BufReader::new(child.stdout.take().unwrap()).lines().next().unwrap().unwrap()).unwrap();
    let mut old = tiles("40");
    assert_eq!(first_line(&mut old)["columns"], 40);
    let doomed = pids(&["-P", &old.id().to_string()]);
    for pid in doomed.iter().copied().chain([old.id() as i32]) {
        term(pid);
    }
    old.wait().unwrap();
    let mut new = tiles("60");
    let line = first_line(&mut new);
    term(new.id() as i32);
    new.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(doomed.is_empty(), "the spare was a child of the headless process: {doomed:?}");
    assert_eq!(line["columns"], 60);
}
