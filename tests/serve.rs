#![allow(clippy::unwrap_used, reason = "a failed step should fail the test")]
//! `mdd --serve` end to end: the protocol the Neovim plugin speaks.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use serde_json::{Value, json};

struct Serve {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next: u64,
}

impl Serve {
    fn start() -> Serve {
        let mut child =
            Command::new(env!("CARGO_BIN_EXE_mdd")).env("MDDOG_CACHE_DIR", std::env::temp_dir().join("mddog-test-cache")).arg("--serve").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        // A reader thread, so an answer that never comes fails the test instead of hanging it.
        std::thread::spawn(move || BufReader::new(stdout).lines().map_while(Result::ok).try_for_each(|l| tx.send(serde_json::from_str(&l).unwrap())));
        Serve { child, stdin, rx, next: 0 }
    }

    fn write(&mut self, msgs: &[Value]) {
        let text: String = msgs.iter().map(|m| format!("{m}\n")).collect();
        self.stdin.write_all(text.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&self) -> Value {
        self.rx.recv_timeout(Duration::from_secs(20)).expect("no answer within 20 s")
    }

    /// Sends one request and returns its answer, keeping the events that came first.
    fn ask(&mut self, cmd: &str, fields: Value, events: &mut Vec<Value>) -> Value {
        self.next += 1;
        let mut msg = fields;
        msg["id"] = json!(self.next);
        msg["cmd"] = json!(cmd);
        self.write(&[msg]);
        loop {
            let v = self.recv();
            if v["id"] == json!(self.next) {
                return v;
            }
            events.push(v);
        }
    }

    fn page(&mut self, events: &mut Vec<Value>) -> Value {
        if let Some(i) = events.iter().position(|e| e["event"] == "page") {
            return events.remove(i);
        }
        loop {
            let v = self.recv();
            if v["event"] == "page" {
                return v;
            }
        }
    }

    fn hello(&mut self) {
        let ok = self.ask("hello", json!({ "protocol": 1 }), &mut vec![]);
        assert_eq!(ok["ok"]["protocol"], 1, "{ok}");
    }

    /// Opens `text` at 60 columns and returns its first page.
    fn open(&mut self, text: &str, path: &std::path::Path) -> Value {
        let mut ev = vec![];
        self.ask("size", json!({ "cols": 60 }), &mut ev);
        let ok = self.ask("open", json!({ "text": text, "path": path }), &mut ev);
        assert!(ok["ok"]["rev"].is_u64(), "{ok}");
        self.page(&mut ev)
    }

    /// The first cell from the top left whose link matches `want`.
    fn find_link(&mut self, rows: u64, want: impl Fn(&Value) -> bool) -> Value {
        for row in 0..rows {
            for col in 0..30 {
                let v = self.ask("link", json!({ "row": row, "col": col }), &mut vec![]);
                if want(&v["ok"]["link"]) {
                    return v["ok"]["link"].clone();
                }
            }
        }
        panic!("no such link in the first {rows} rows");
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mddog-serve-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

#[test]
fn hello_refuses_another_protocol() {
    let mut s = Serve::start();
    let v = s.ask("hello", json!({ "protocol": 99 }), &mut vec![]);
    assert!(v["error"].as_str().unwrap().contains("protocol"), "{v}");
}

#[test]
fn tiles_are_raw_pixels_of_whole_rows() {
    let dir = temp_dir("tiles");
    let mut s = Serve::start();
    s.hello();
    let page = s.open(&format!("# Title\n\n{}", "A paragraph.\n\n".repeat(80)), &dir.join("doc.md"));
    let (rows, tile_rows) = (page["rows"].as_u64().unwrap(), page["tile_rows"].as_u64().unwrap());
    assert_eq!(page["cols"], 60);
    assert!(rows > tile_rows, "a long page takes more than one tile: {page}");
    assert!(tile_rows <= 297, "a placeholder image has at most 297 rows");
    assert_eq!(page["headings"][0]["text"], "Title");
    let rev = page["rev"].clone();
    let last = rows.div_ceil(tile_rows) - 1;
    let v = s.ask("tiles", json!({ "rev": rev, "tiles": [0, last] }), &mut vec![]);
    let tiles = v["ok"]["tiles"].as_array().unwrap();
    assert_eq!(tiles.len(), 2, "{v}");
    let cell = page["cell"].as_array().unwrap();
    let (cw, ch) = (cell[0].as_u64().unwrap(), cell[1].as_u64().unwrap());
    for t in tiles {
        let file = t["file"].as_str().unwrap();
        let (w, h) = (t["width"].as_u64().unwrap(), t["height"].as_u64().unwrap());
        assert_eq!(w, 60 * cw);
        assert_eq!(h % ch, 0, "a tile spans whole rows");
        assert_eq!(std::fs::metadata(file).unwrap().len(), w * h * 4);
        assert!(file.contains("tty-graphics-protocol"), "the terminal deletes only files so named: {file}");
        std::fs::remove_file(file).unwrap();
    }
    let rows_last = rows - last * tile_rows;
    assert_eq!(tiles[1]["height"].as_u64().unwrap(), rows_last * ch);
}

#[test]
fn a_tiles_request_for_an_old_page_fails() {
    let dir = temp_dir("stale");
    let mut s = Serve::start();
    s.hello();
    let first = s.open("# One\n", &dir.join("doc.md"));
    let mut ev = vec![];
    s.ask("open", json!({ "text": "# Two\n", "path": dir.join("doc.md") }), &mut ev);
    s.page(&mut ev);
    let v = s.ask("tiles", json!({ "rev": first["rev"], "tiles": [0] }), &mut vec![]);
    assert_eq!(v["error"], "stale");
}

#[test]
fn only_the_last_waiting_open_is_laid_out() {
    let dir = temp_dir("burst");
    let mut s = Serve::start();
    s.hello();
    let path = dir.join("doc.md");
    s.write(&[
        json!({ "id": 10, "cmd": "open", "text": "# First\n", "path": path }),
        json!({ "id": 11, "cmd": "open", "text": "# Second\n", "path": path }),
        json!({ "id": 12, "cmd": "open", "text": "# Third\n", "path": path }),
    ]);
    let mut pages = vec![];
    let mut skipped = 0;
    while pages.is_empty() || skipped + pages.len() < 3 {
        let v = s.recv();
        if v["event"] == "page" {
            pages.push(v);
        } else if v["ok"]["skipped"] == true {
            skipped += 1;
        } else if v["id"] == 12 {
            assert!(v["ok"]["rev"].is_u64());
        }
    }
    // The three may arrive in one read or in several; the last page always shows the last text.
    assert_eq!(pages.last().unwrap()["headings"][0]["text"], "Third");
}

#[test]
fn links_are_classified_and_local_ones_confined() {
    let dir = temp_dir("links");
    std::fs::write(dir.join("other.md"), "# Other\n\n## Deep part\n").unwrap();
    std::fs::write(dir.join("app.command"), "echo hi\n").unwrap();
    let text = "[web](https://example.com/x)\n\n[here](#target)\n\n[other](other.md#deep-part)\n\n[app](app.command)\n\n[gone](missing.md)\n\n## Target\n";
    let mut s = Serve::start();
    s.hello();
    // A file not saved yet still finds its neighbours.
    let page = s.open(text, &dir.join("not-saved-yet.md"));
    let rows = page["rows"].as_u64().unwrap();
    let web = s.find_link(rows, |l| l["kind"] == "web");
    assert_eq!(web["url"], "https://example.com/x");
    let anchor = s.find_link(rows, |l| l["kind"] == "anchor");
    let target = page["headings"].as_array().unwrap().iter().find(|h| h["text"] == "Target").unwrap();
    assert_eq!(anchor["row"], target["row"]);
    let file = s.find_link(rows, |l| l["kind"] == "file");
    assert_eq!(file["path"], json!(dir.join("other.md")));
    assert_eq!(file["heading"], "deep-part");
    let refused = s.find_link(rows, |l| l["kind"] == "refused" && l["why"].as_str().unwrap().contains("app.command"));
    assert!(refused["why"].as_str().unwrap().contains("Not a Markdown file"), "{refused}");
    let gone = s.find_link(rows, |l| l["kind"] == "refused" && l["why"].as_str().unwrap().contains("missing.md"));
    assert!(gone["why"].as_str().unwrap().contains("no such file"), "{gone}");
    let v = s.ask("anchor", json!({ "anchor": "target" }), &mut vec![]);
    assert_eq!(v["ok"]["row"], target["row"]);
    let v = s.ask("anchor", json!({ "anchor": "nowhere" }), &mut vec![]);
    assert!(v["ok"]["row"].is_null());
}

#[test]
fn a_size_change_lays_the_page_out_again() {
    let dir = temp_dir("size");
    let mut s = Serve::start();
    s.hello();
    let wide = s.open(&"Some words that wrap when the page is narrow. ".repeat(20), &dir.join("doc.md"));
    let mut ev = vec![];
    s.ask("size", json!({ "cols": 20 }), &mut ev);
    let narrow = s.page(&mut ev);
    assert_eq!(narrow["cols"], 20);
    assert!(narrow["rev"].as_u64() > wide["rev"].as_u64());
    assert!(narrow["rows"].as_u64() > wide["rows"].as_u64(), "a narrower page is taller");
}

#[test]
fn closing_standard_input_ends_the_run() {
    let mut s = Serve::start();
    s.hello();
    drop(std::mem::replace(&mut s.stdin, Command::new("true").stdin(Stdio::piped()).spawn().unwrap().stdin.take().unwrap()));
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < end {
        if let Some(status) = s.child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("mdd --serve kept running after its input closed");
}
