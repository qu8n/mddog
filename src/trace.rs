use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn start() {
    START.get_or_init(Instant::now);
}

pub fn ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

fn log(line: &str) {
    if let Some(path) = std::env::var_os("MDDOG_TRACE") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            // One write per line, since remote fetch threads log too.
            let _ = f.write_all(format!("{epoch:.1} {:.1} {line}\n", ms()).as_bytes());
        }
    }
}

pub fn log_key(k: crate::term::Key) {
    log(&format!("key {k:?}"));
}

pub fn log_view(line: &str) {
    log(&format!("view {line}"));
}

pub fn log_probe(reply: &[u8]) {
    log(&format!("probe {}", reply.escape_ascii()));
}

pub fn log_match(i: usize, n: usize, r: [f64; 4]) {
    log(&format!("match {i}/{n} rect={:.1},{:.1},{:.1},{:.1}", r[0], r[1], r[2], r[3]));
}

pub fn enabled() -> bool {
    std::env::var_os("MDDOG_TRACE").is_some()
}

pub fn mark(phase: &str) {
    let t = ms();
    log(&format!("mark {phase}"));
    if std::env::var_os("MDDOG_DEBUG").is_some() {
        eprintln!("[{t:8.1} ms] {phase}");
    }
}
