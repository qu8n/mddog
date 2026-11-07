//! Which links the viewer follows, and opening one outside it.
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
        return match readmes(folder).into_iter().find_map(inside) {
            Some(path) => Follow::Show(path, heading),
            None => Follow::Refuse(format!("No README in {name}")),
        };
    }
    if !shown.ends_with('/') && !is_markdown(Path::new(&shown)) {
        return Follow::Refuse(format!("Not a Markdown file: {name}"));
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

/// The folder's `README.md` and `README.markdown` files, in any case, `.md` first.
fn readmes(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("readme")) && is_markdown(p))
        .collect();
    found.sort_by_key(|p| (!p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")), p.clone()));
    found
}

/// Whether the viewer shows `path`.
fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

pub fn open(url: String) {
    std::thread::spawn(move || {
        let _ = Command::new("open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(follow("sh%2Ecommand", &dir), Follow::Refuse("Not a Markdown file: sh.command".into()));
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
            assert_eq!(follow(url, &dir), Follow::Refuse(format!("Not a Markdown file: {name}")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
