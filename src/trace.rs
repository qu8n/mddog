//! Test and debug logging. `MDDOG_TRACE=FILE` appends one line per event to FILE, and `MDDOG_DEBUG` prints the
//! timing marks to stderr. The pty tests read the trace lines, so their first words are a format.
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Pins the clock that `ms` counts from.
pub fn start() {
    START.get_or_init(Instant::now);
}

pub fn ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

fn file() -> Option<&'static PathBuf> {
    static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
    FILE.get_or_init(|| std::env::var_os("MDDOG_TRACE").map(PathBuf::from)).as_ref()
}

pub fn enabled() -> bool {
    file().is_some()
}

/// Appends `line` to the trace with the wall clock and `ms`.
pub fn log(line: impl std::fmt::Display) {
    if let Some(path) = file() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            // One write per line, since remote fetch threads log too.
            let _ = f.write_all(format!("{epoch:.1} {:.1} {line}\n", ms()).as_bytes());
        }
    }
}

/// A timing mark: in the trace, and on stderr under `MDDOG_DEBUG`.
pub fn mark(phase: &str) {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    let t = ms();
    log(format_args!("mark {phase}"));
    if *DEBUG.get_or_init(|| std::env::var_os("MDDOG_DEBUG").is_some()) {
        eprintln!("[{t:8.1} ms] {phase}");
    }
}
