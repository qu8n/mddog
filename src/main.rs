use mddog::term::kitty;
use mddog::viewer::preview;
use mddog::*;

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: mdd [file.md|file.mdx|folder]
       command | mdd";

fn help() -> String {
    format!(
        "{USAGE}

Render Markdown and MDX in the terminal as a browser shows them, styled like GitHub by default. The preview reloads when the file changes.
It needs a terminal with the Kitty graphics protocol, such as Ghostty, Kitty, WezTerm, iTerm2, or Warp.
With no file, it lists the Markdown files in the current git repo, or else the current folder, to pick from. With a folder, it lists that folder.
With -, or with input piped in, it reads standard input.

Options
  -h, --help              show this help
  -V, --version           show the version
      --start-at-login    have launchd start a prewarmed mdd at login, so the first preview opens fast
      --no-start-at-login undo --start-at-login
      --headless COLS     write the preview as PNG tiles COLS cells wide and print them as JSON, for the Claude Code plugin

In the preview, press ? to list the keys.
"
    )
}

fn main() {
    trace::start();
    if std::env::args().nth(1).as_deref() == Some("--spare") {
        let _ = config::load();
        std::process::exit(engine::spare::serve());
    }
    if std::env::args().nth(1).as_deref() == Some("--start-at-login") {
        match engine::spare::start_at_login() {
            Ok(_) => println!("mdd: launchd now starts a prewarmed mdd at login, so your first preview opens in about 40 ms instead of 250 ms. Undo with: mdd --no-start-at-login"),
            Err(e) => fail(&e),
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--no-start-at-login") {
        match engine::spare::no_start_at_login() {
            Ok(true) => println!("mdd: launchd no longer starts mdd at login"),
            Ok(false) => println!("mdd: nothing to undo, launchd wasn't starting mdd at login"),
            Err(e) => fail(&e),
        }
        return;
    }
    let mut files: Vec<String> = Vec::new();
    let mut options = true;
    let mut headless: Option<u16> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" if options => match args.next().and_then(|n| n.parse().ok()).filter(|&n: &u16| (1..=255).contains(&n)) {
                Some(n) => headless = Some(n),
                None => usage("--headless needs a column count from 1 to 255"),
            },
            "-h" | "--help" if options => {
                print!("{}", help());
                return;
            }
            "-V" | "--version" if options => {
                println!("mdd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--" if options => options = false,
            "-" => files.push(arg),
            _ if options && arg.starts_with('-') => usage(&format!("unknown option {arg}. To open a file whose name starts with -, use ./{arg} or -- {arg}")),
            "" => usage("the file name is empty"),
            _ => files.push(arg),
        }
    }
    let joined = files.join(" ");
    let file = match &files[..] {
        [] if stdin_is_tty() => String::new(),
        [] => "-".to_string(),
        [f] => f.clone(),
        _ if files.len() > 1 && Path::new(&joined).exists() => fail(&format!("got {} files, but \"{joined}\" is one file. Put quotes around a name that has spaces", files.len())),
        _ => fail(&format!("opens one file at a time, but got {}: {}", files.len(), files.join(", "))),
    };
    config::load().unwrap_or_else(|e| fail(&format!("config: {e}")));
    if let Some(cols) = headless {
        if file.is_empty() {
            usage("--headless needs a file");
        }
        if file == "-" {
            usage("--headless needs a file, not standard input");
        }
        check_file(Path::new(&file)).unwrap_or_else(|e| fail(&e));
        std::process::exit(headless::run(PathBuf::from(file), cols));
    }
    if file.is_empty() || (file != "-" && Path::new(&file).is_dir()) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = if file.is_empty() { viewer::picker::scope(&cwd, &cwd) } else { std::fs::canonicalize(&file).unwrap_or_else(|_| PathBuf::from(&file)) };
        std::process::exit(preview::browse(root));
    }
    let code = if file == "-" {
        let path = stdin_copy().unwrap_or_else(|e| fail(&e));
        let code = preview::run(path.clone(), true);
        let _ = std::fs::remove_file(path);
        code
    } else {
        check_file(Path::new(&file)).unwrap_or_else(|e| fail(&e));
        preview::run(PathBuf::from(file), false)
    };
    std::process::exit(code);
}

fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

fn looks_binary(head: &[u8]) -> bool {
    let head = &head[..head.len().min(8000)];
    if head.contains(&0) {
        return true;
    }
    let text = String::from_utf8_lossy(head);
    let bad = text.chars().filter(|&c| c == '\u{fffd}').count();
    bad * 10 > text.chars().count().max(1)
}

fn stdin_copy() -> Result<PathBuf, String> {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes).map_err(|e| format!("cannot read standard input: {}", term::io_error(&e)))?;
    if bytes.is_empty() {
        usage("no file given, and standard input is empty");
    }
    if looks_binary(&bytes) {
        return Err("standard input looks like a binary file, not Markdown".into());
    }
    let path = std::env::temp_dir().join(format!("mddog-{}-{:x}-stdin.md", std::process::id(), kitty::nonce()));
    kitty::write_private(&path, &bytes).map_err(|e| format!("cannot keep standard input: {}", term::io_error(&e)))?;
    Ok(path)
}

fn check_file(path: &Path) -> Result<(), String> {
    let shown = term::printable(&path.display().to_string());
    if let Some(url) = ["http://", "https://"].iter().find(|p| shown.starts_with(*p)).map(|_| &shown) {
        return Err(format!("shows local files only, not URLs. Download it first: curl -sL {url} | mdd"));
    }
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(format!("{shown} is a link to a file that does not exist"));
            }
            return Err(match near_match(path) {
                Some(m) => format!("{shown}: no such file. Did you mean {}?", term::printable(&m.display().to_string())),
                None => format!("{shown}: no such file"),
            });
        }
        Err(e) => return Err(format!("{shown}: {}", term::io_error(&e))),
    };
    if meta.is_dir() {
        let readme = std::fs::read_dir(path).ok().and_then(|d| d.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("readme.md"))));
        return Err(match readme {
            Some(r) => format!("{shown} is a folder. Did you mean {}?", term::printable(&r.display().to_string())),
            None => format!("{shown} is a folder, not a file. Give a .md or .mdx file"),
        });
    }
    let mut head = Vec::new();
    if meta.is_file() {
        std::fs::File::open(path).and_then(|f| f.take(8000).read_to_end(&mut head)).map_err(|e| format!("{shown}: {}", term::io_error(&e)))?;
    }
    if looks_binary(&head) {
        return Err(format!("{shown} looks like a binary file, not Markdown"));
    }
    Ok(())
}

fn near_match(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = |n: &str| n.rsplit_once('.').map_or(n.to_string(), |(s, _)| s.to_string());
    let mut best: Option<(usize, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let cand = e.file_name().to_string_lossy().to_lowercase();
        if !(cand.ends_with(".md") || cand.ends_with(".mdx") || cand.ends_with(".markdown")) {
            continue;
        }
        let d = if stem(&cand) == stem(&name) { 0 } else { distance(&cand, &name) };
        if d <= 1 && best.as_ref().is_none_or(|(b, _)| d < *b) {
            best = Some((d, path.with_file_name(e.file_name())));
        }
    }
    best.map(|(_, p)| p)
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn usage(msg: &str) -> ! {
    eprintln!("{}\n{USAGE}\nRun mdd --help for the options.", fit(&format!("mdd: {}", term::printable(msg))));
    std::process::exit(2);
}

fn fail(msg: &str) -> ! {
    eprintln!("{}", fit(&format!("mdd: {}", term::printable(msg))));
    std::process::exit(2);
}

fn fit(msg: &str) -> String {
    if std::io::stderr().is_terminal() {
        return term::wrap(msg, usize::from(term::size_of(2).cols));
    }
    msg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance() {
        assert_eq!(distance("readme.md", "readme.dm"), 1);
        assert_eq!(distance("nope.md", "noperm.md"), 2);
        assert_eq!(distance("readme.md", "readme.md"), 0);
        assert_eq!(distance("readme.md", "redme.md"), 1);
    }

    #[test]
    fn binary_by_nul() {
        assert!(looks_binary(b"\x89PNG\r\n\x1a\n\0\0"));
        assert!(!looks_binary("# Hi ✓".as_bytes()));
        assert!(looks_binary(&[0xff, 0xfe, 0x41, 0xc3, 0x28, 0xa0, 0xa1, 0x42]));
        assert!(!looks_binary(b"# Title\n\nan \xff odd byte in a long enough line of text"));
    }

    #[test]
    fn near_match_accepts_one_edit_or_the_same_stem() {
        let dir = std::env::temp_dir().join(format!("mddog-near-{}-{:x}", std::process::id(), kitty::nonce()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["readme.md", "notes.mdx", "guide.markdown", "guid.md", "todo.txt", "plan.md"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let near = |n: &str| near_match(&dir.join(n)).map(|p| p.file_name().unwrap().to_string_lossy().into_owned());
        assert_eq!(near("raedme.md").as_deref(), Some("readme.md"));
        assert_eq!(near("README.MD").as_deref(), Some("readme.md"));
        assert_eq!(near("readme.markdown").as_deref(), Some("readme.md"));
        assert_eq!(near("note.mdx").as_deref(), Some("notes.mdx"));
        assert_eq!(near("guide.md").as_deref(), Some("guide.markdown"));
        assert_eq!(near("plans.md").as_deref(), Some("plan.md"));
        assert_eq!(near("rdme.md"), None);
        assert_eq!(near("todo.md"), None);
        assert_eq!(near("zzz.md"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
