//! Which links the viewer follows, and opening one outside it.
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, PartialEq, Eq)]
pub enum Link {
    /// A web or mail link, for the system to open.
    Open(String),
    /// A heading on the page, by its id.
    Anchor(String),
    /// A local file, as the link wrote it: a relative path, a `/` path from the repo, or a `file://` URL.
    File(String),
}

impl Link {
    /// None for a link the viewer doesn't follow, such as `javascript:` or `data:`.
    pub fn of(url: &str) -> Option<Link> {
        if let Some(id) = url.strip_prefix('#') {
            return Some(Link::Anchor(decode(id)));
        }
        // A query alone names the page shown, as an empty path does in a browser.
        if url.starts_with('?') {
            return Some(Link::Anchor(url.split_once('#').map_or_else(String::new, |(_, id)| decode(id))));
        }
        let Some(scheme) = scheme(url) else { return Some(Link::File(url.to_string())) };
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" | "mailto" => Some(Link::Open(url.to_string())),
            "file" => Some(Link::File(url.to_string())),
            _ => None,
        }
    }
}

/// A URL's scheme, by RFC 3986: a letter, then letters, digits, `+`, `-` or `.`, then `:`.
fn scheme(url: &str) -> Option<&str> {
    let (s, _) = url.split_once(':')?;
    let mut c = s.chars();
    (c.next()?.is_ascii_alphabetic() && c.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))).then_some(s)
}

/// What a click on a `File` link does.
#[derive(Debug, PartialEq, Eq)]
pub enum Follow {
    /// Shows the Markdown file in the viewer, at the heading with this id, if any.
    Show(PathBuf, Option<String>),
    /// Opens nothing and says why. Nothing local goes to the system's `open`, which would run a
    /// `.command` or an app that a cloned repo links to.
    Refuse(String),
}

/// Where a `File` link goes from a page in `dir`. The file must be Markdown, and in the repo (else
/// the home folder, else `dir`) as local images must be. A folder opens its README, as on GitHub.
pub fn follow(url: &str, dir: &Path) -> Follow {
    let raw = url.split(['?', '#']).next().unwrap_or(url);
    let shown = decode(raw);
    let name = shown.trim_end_matches('/').rsplit('/').next().unwrap_or(&shown);
    let heading = url.split_once('#').map(|(_, id)| decode(id)).filter(|id| !id.is_empty());
    let found = crate::render::media::confined_media(url, dir);
    if let Some(folder) = found.as_ref().filter(|p| p.is_dir()) {
        let name = folder.file_name().map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().into_owned());
        // Through the same check as a link, so a README that is a symlink can't leave the repo.
        let inside = |p: PathBuf| crate::render::media::confined_media(&format!("{}/{}", raw.trim_end_matches('/'), p.file_name()?.to_str()?), dir);
        return match super::picker::readmes(folder).into_iter().find_map(inside) {
            Some(path) => Follow::Show(path, heading),
            None => Follow::Refuse(format!("No README in {name}")),
        };
    }
    if !shown.ends_with('/') && !super::picker::listed(Path::new(&shown)) {
        return Follow::Refuse(format!("Not a {} file: {name}", crate::config::get().file_list.names()));
    }
    match found {
        Some(path) if !shown.ends_with('/') => Follow::Show(path, heading),
        _ => Follow::Refuse(format!("Can't open {shown}: no such file in this repo")),
    }
}

/// The URL text with `%XX` decoded, or as written when the bytes aren't UTF-8.
fn decode(s: &str) -> String {
    crate::render::media::percent_decode(s).unwrap_or_else(|| s.to_string())
}

pub fn argv(cmd: &[String], url: &str) -> Vec<String> {
    let mut out: Vec<String> = cmd.iter().map(|a| a.replace("{url}", url)).collect();
    if !cmd.iter().any(|a| a.contains("{url}")) {
        out.push(url.to_string());
    }
    out
}

fn on_path(name: &str, path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|d| std::fs::metadata(d.join(name)).is_ok_and(|m| m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0))
}

pub fn open(url: String, failed: impl FnOnce(String) + Send + 'static) -> Result<(), String> {
    let cmd = &crate::config::get().open_command;
    let capture = *cmd != crate::config::default_open_command();
    let argv = argv(cmd, &url);
    let prog = &argv[0];
    if !prog.contains('/') && !std::env::var_os("PATH").is_some_and(|p| on_path(prog, &p)) {
        return Err(format!("{prog} is not installed"));
    }
    std::thread::spawn(move || {
        if let Err(why) = run(&argv, capture) {
            failed(format!("Can't open {url}: {why}"));
        }
    });
    Ok(())
}

const OPEN_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

fn run(argv: &[String], capture: bool) -> Result<(), String> {
    run_for(argv, OPEN_LIMIT, capture)
}

fn run_for(argv: &[String], limit: std::time::Duration, capture: bool) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};
    let prog = &argv[0];
    let io = |e: std::io::Error| format!("{prog}: {}", crate::error::io(&e));
    let mut log = if capture { Some(crate::tmp::unnamed().map_err(io)?) } else { None };
    let err_out = match &log {
        Some(f) => Stdio::from(f.try_clone().map_err(io)?),
        None => Stdio::null(),
    };
    let mut child = Command::new(prog).args(&argv[1..]).process_group(0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(err_out).spawn().map_err(io)?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let status = match rx.recv_timeout(limit) {
        Ok(status) => status.map_err(io)?,
        Err(_) => return Ok(()),
    };
    if status.success() {
        return Ok(());
    }
    let mut tail = vec![];
    if let Some(log) = log.as_mut() {
        let size = log.seek(SeekFrom::End(0)).unwrap_or(0);
        if log.seek(SeekFrom::Start(size.saturating_sub(4096))).is_ok() {
            let _ = log.take(4096).read_to_end(&mut tail);
        }
    }
    match String::from_utf8_lossy(&tail).lines().map(str::trim).rfind(|l| !l.is_empty()) {
        Some(line) => Err(format!("{prog} failed: {line}")),
        None => match (status.code(), status.signal()) {
            (None, Some(sig)) => Err(format!("{prog} was stopped by signal {sig}")),
            (code, _) => Err(format!("{prog} exited with {}", code.unwrap_or(-1))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_open_says_why() {
        assert_eq!(run(&["true".to_string()], true), Ok(()));
        assert_eq!(run(&["false".to_string()], false), Err("false exited with 1".into()));
    }

    #[test]
    fn the_opener_must_be_an_executable_on_path() {
        let dir = std::env::temp_dir().join(format!("mddog-opener-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::env::join_paths([std::path::Path::new("/nonexistent"), &dir]).unwrap();
        assert!(!on_path("opener", &path));
        std::fs::write(dir.join("opener"), "#!/bin/sh\n").unwrap();
        assert!(!on_path("opener", &path), "a file that can't run is no opener");
        std::fs::set_permissions(dir.join("opener"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        assert!(on_path("opener", &path));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_past_the_limit_is_left_running() {
        let dir = std::env::temp_dir().join(format!("mddog-open-left-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("late");
        let script = format!("sleep 0.5; touch '{}'", marker.display());
        let argv = ["sh", "-c", &script].map(String::from);
        let start = std::time::Instant::now();
        assert_eq!(run_for(&argv, std::time::Duration::from_millis(200), true), Ok(()));
        assert!(start.elapsed() < std::time::Duration::from_millis(450), "{:?}", start.elapsed());
        std::thread::sleep(std::time::Duration::from_millis(1000));
        assert!(marker.exists(), "the command was stopped instead of left running");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failure_after_the_limit_is_not_reported() {
        let argv = ["sh", "-c", "sleep 0.4; exit 3"].map(String::from);
        assert_eq!(run_for(&argv, std::time::Duration::from_millis(200), true), Ok(()));
    }

    #[test]
    fn only_a_custom_command_has_its_errors_captured() {
        let argv = ["sh", "-c", "[ -f /dev/fd/2 ] && exit 7; exit 0"].map(String::from);
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), false), Ok(()));
        let argv = ["sh", "-c", "[ -f /dev/fd/2 ] || exit 7; exit 0"].map(String::from);
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), true), Ok(()));
    }

    #[test]
    fn the_open_command_leads_its_own_process_group() {
        let argv = ["sh", "-c", "[ \"$(ps -o pgid= -p $$ | tr -d ' ')\" = \"$$\" ] || exit 7"].map(String::from);
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), true), Ok(()));
    }

    #[test]
    fn a_noisy_failed_open_command_reports_its_last_line() {
        let script = "head -c 300000 /dev/zero | tr '\\0' x >&2; echo >&2; echo 'last line' >&2; exit 1";
        let argv = ["sh", "-c", script].map(String::from);
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), true), Err("sh failed: last line".into()));
    }

    #[test]
    fn a_failure_is_reported_while_a_child_still_runs() {
        let argv = ["sh", "-c", "sleep 3 & exit 3"].map(String::from);
        let start = std::time::Instant::now();
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), true), Err("sh exited with 3".into()));
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
    }

    #[test]
    fn a_failure_keeps_its_error_while_a_child_still_runs() {
        let argv = ["sh", "-c", "echo boom >&2; sleep 3 & exit 3"].map(String::from);
        let start = std::time::Instant::now();
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), true), Err("sh failed: boom".into()));
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
    }

    #[test]
    fn the_open_command_writes_its_errors_where_a_quit_cannot_break_them() {
        let argv = ["sh", "-c", "[ -p /dev/fd/2 ] && exit 7; exit 0"].map(String::from);
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), true), Ok(()));
    }

    #[test]
    fn an_open_command_killed_by_a_signal_says_so() {
        let argv = ["sh", "-c", "kill -TERM $$"].map(String::from);
        assert_eq!(run_for(&argv, std::time::Duration::from_secs(10), true), Err("sh was stopped by signal 15".into()));
    }

    #[test]
    fn a_failed_open_command_says_why() {
        let cmd = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(run(&cmd(&["true"]), true), Ok(()));
        assert_eq!(run(&cmd(&["sh", "-c", "echo first >&2; echo 'no route to host' >&2; exit 3"]), true), Err("sh failed: no route to host".into()));
        assert_eq!(run(&cmd(&["sh", "-c", "exit 4"]), true), Err("sh exited with 4".into()));
        let missing = run(&cmd(&["mddog-no-such-program"]), true).unwrap_err();
        assert!(missing.starts_with("mddog-no-such-program: "), "{missing}");
    }

    #[test]
    fn the_url_goes_last_or_where_the_command_says() {
        let cmd = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(argv(&cmd(&["open"]), "https://a.dev"), cmd(&["open", "https://a.dev"]));
        assert_eq!(argv(&cmd(&["ssh", "mac", "open"]), "https://a.dev"), cmd(&["ssh", "mac", "open", "https://a.dev"]));
        assert_eq!(argv(&cmd(&["rex", "do", "open_url", "url={url}", "x"]), "https://a.dev"), cmd(&["rex", "do", "open_url", "url=https://a.dev", "x"]));
    }

    #[test]
    fn web_mail_anchor_and_local_links_are_followed() {
        let open = |u: &str| Some(Link::Open(u.to_string()));
        let file = |u: &str| Some(Link::File(u.to_string()));
        assert_eq!(Link::of("https://a.dev/x"), open("https://a.dev/x"));
        assert_eq!(Link::of("HTTP://a.dev"), open("HTTP://a.dev"));
        assert_eq!(Link::of("mailto:a@b.dev"), open("mailto:a@b.dev"));
        assert_eq!(Link::of("#intro"), Some(Link::Anchor("intro".into())));
        assert_eq!(Link::of("#caf%C3%A9"), Some(Link::Anchor("café".into())));
        assert_eq!(Link::of("#100%"), Some(Link::Anchor("100%".into())));
        assert_eq!(Link::of("?raw=1#caf%C3%A9"), Some(Link::Anchor("café".into())));
        assert_eq!(Link::of("?raw=1"), Some(Link::Anchor(String::new())));
        assert_eq!(Link::of("docs/a.md"), file("docs/a.md"));
        assert_eq!(Link::of("../a.md#setup"), file("../a.md#setup"));
        assert_eq!(Link::of("x.html"), file("x.html"));
        assert_eq!(Link::of("a:b/c.md"), None);
        assert_eq!(Link::of("./a:b.md"), file("./a:b.md"));
        assert_eq!(Link::of("FILE:///tmp/a.md"), file("FILE:///tmp/a.md"));
        assert_eq!(Link::of("javascript:alert(1)"), None);
        assert_eq!(Link::of("data:text/html,hi"), None);
        assert_eq!(Link::of("vscode://file/a"), None);
    }

    #[test]
    fn a_markdown_target_resolves_from_the_page_folder() {
        let dir = std::env::temp_dir().join(format!("mddog-link-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/a b.md"), "").unwrap();
        std::fs::write(dir.join("top.MARKDOWN"), "").unwrap();
        let dir = std::fs::canonicalize(&dir).unwrap();
        let docs = dir.join("docs");
        assert_eq!(follow("a%20b.md#Set-up", &docs), Follow::Show(docs.join("a b.md"), Some("Set-up".into())));
        assert_eq!(follow("../top.MARKDOWN", &docs), Follow::Show(dir.join("top.MARKDOWN"), None));
        assert_eq!(follow("/top.MARKDOWN#", &docs), Follow::Show(dir.join("top.MARKDOWN"), None));
        assert_eq!(follow("nope.md", &docs), Follow::Refuse("Can't open nope.md: no such file in this repo".into()));
        assert!(matches!(follow("../../outside.md", &docs), Follow::Refuse(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_percent_encoded_path_and_heading_are_decoded() {
        let dir = std::env::temp_dir().join(format!("mddog-pct-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("guide.md"), "").unwrap();
        let dir = std::fs::canonicalize(&dir).unwrap();
        assert_eq!(follow("guide.md#caf%C3%A9", &dir), Follow::Show(dir.join("guide.md"), Some("café".into())));
        assert_eq!(follow("guide%2Emd", &dir), Follow::Show(dir.join("guide.md"), None));
        assert_eq!(follow("guide%2emd#a%20b", &dir), Follow::Show(dir.join("guide.md"), Some("a b".into())));
        assert_eq!(follow("sh%2Ecommand", &dir), Follow::Refuse("Not a .md or .markdown file: sh.command".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_link_opens_its_readme() {
        let dir = std::env::temp_dir().join(format!("mddog-dir-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        std::fs::create_dir_all(dir.join("empty")).unwrap();
        std::fs::write(dir.join("docs/ReadMe.md"), "").unwrap();
        std::fs::write(dir.join("notes/readme.markdown"), "").unwrap();
        std::fs::write(dir.join("empty/README.txt"), "").unwrap();
        let dir = std::fs::canonicalize(&dir).unwrap();
        assert_eq!(follow("docs/", &dir), Follow::Show(dir.join("docs/ReadMe.md"), None));
        assert_eq!(follow("docs#setup", &dir), Follow::Show(dir.join("docs/ReadMe.md"), Some("setup".into())));
        assert_eq!(follow("./notes", &dir), Follow::Show(dir.join("notes/readme.markdown"), None));
        assert_eq!(follow("/", &dir.join("docs")), Follow::Refuse(format!("No README in {}", dir.file_name().unwrap().to_string_lossy())));
        assert_eq!(follow("empty/", &dir), Follow::Refuse("No README in empty".into()));
        assert_eq!(follow("gone/", &dir), Follow::Refuse("Can't open gone/: no such file in this repo".into()));
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", dir.join("out/README.md")).unwrap();
        assert_eq!(follow("out/", &dir), Follow::Refuse("No README in out".into()));
        std::fs::write(dir.join("out/readme.markdown"), "").unwrap();
        assert_eq!(follow("out/", &dir), Follow::Show(dir.join("out/readme.markdown"), None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_local_file_that_is_not_markdown_never_opens() {
        let dir = std::env::temp_dir().join(format!("mddog-refuse-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("x.app")).unwrap();
        std::fs::write(dir.join("setup.command"), "").unwrap();
        std::fs::write(dir.join("doc.pdf"), "").unwrap();
        for (url, name) in [("setup.command", "setup.command"), ("doc.pdf#p", "doc.pdf"), ("file:///bin/sh", "sh")] {
            assert_eq!(follow(url, &dir), Follow::Refuse(format!("Not a .md or .markdown file: {name}")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
