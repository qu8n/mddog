use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn d4_watch_rename() {
    let dir = std::env::temp_dir().join(format!("mddog-watch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.md");
    std::fs::write(&file, "# one\n").unwrap();
    let (tx, rx) = mpsc::channel();
    let _w = mddog::watch::spawn(&file, move || {
        let _ = tx.send(());
    })
    .expect("watcher");
    std::thread::sleep(Duration::from_millis(300));
    while rx.try_recv().is_ok() {}
    let tmp = dir.join(".doc.md.swp");
    std::fs::write(&tmp, "# two\n").unwrap();
    let start = Instant::now();
    std::fs::rename(&tmp, &file).unwrap();
    let got = rx.recv_timeout(Duration::from_millis(500)).ok().map(|()| start.elapsed());
    let _ = std::fs::remove_dir_all(&dir);
    let elapsed = got.expect("no event for a write-then-rename save");
    assert!(elapsed < Duration::from_millis(200), "event after {elapsed:?}");
}
