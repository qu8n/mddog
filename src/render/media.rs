//! Which image sources a page fetches from the web, and which local files it may show.

use std::path::{Component, Path, PathBuf};

/// A web URL, by a browser's reading: tabs and line breaks dropped, the ends trimmed, `http` or `https` in any case.
pub fn is_remote(url: &str) -> bool {
    let url: String = url.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    url.trim_matches(|c: char| c <= ' ').split_once(':').is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
}

/// The file a local media URL opens, once canonical, when it is inside `root`. A `/` path starts at `root`, any
/// other at `dir`. The query and fragment don't name a file, so `?raw=1` is dropped.
fn media_path(t: &str, dir: &Path, root: &Path) -> Option<PathBuf> {
    if t.starts_with("//") || t.contains(|c: char| c == '\\' || c.is_control()) {
        return None;
    }
    let path = t.split(['?', '#']).next().unwrap_or_default();
    // An encoded slash is one URL segment but more than one folder on disk, so the two can't agree.
    if path.to_ascii_lowercase().contains("%2f") {
        return None;
    }
    let path = percent_decode(path)?;
    let joined = match path.strip_prefix('/') {
        Some(rest) => root.join(rest),
        None => dir.join(&path),
    };
    // A browser drops `..` with the segment before it as text, before it follows any symlink.
    let mut full = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                full.pop();
            }
            Component::CurDir => {}
            c => full.push(c),
        }
    }
    std::fs::canonicalize(full).ok().filter(|p| p.starts_with(root))
}

/// The file a local image URL in a Markdown file in `dir` may show, by the rules a browser would use: inside the
/// repo, else the home folder, else `dir`. None for a remote or empty URL, or one that leaves that folder.
pub fn confined_media(url: &str, dir: &Path) -> Option<PathBuf> {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let root = media_root(&dir, std::env::var_os("HOME").map(PathBuf::from).as_deref());
    let t = url.trim();
    if t.is_empty() || t.starts_with('#') || is_remote(t) {
        return None;
    }
    // A `file://` URL names an absolute path, which must still be inside the root.
    if let Some(rest) = t.get(..7).filter(|p| p.eq_ignore_ascii_case("file://")).map(|_| &t[7..]) {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        return rest.starts_with('/').then(|| media_path(&format!("/{}", rest.trim_start_matches('/')), Path::new("/"), Path::new("/"))).flatten().filter(|p| p.starts_with(&root));
    }
    media_path(t, &dir, &root)
}

/// None when the bytes aren't UTF-8, since the path checked must be the one a browser would open.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        // from_str_radix takes a sign, and a browser keeps `%+2` as it is.
        let hex = b.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit)).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn media_root(dir: &Path, home: Option<&Path>) -> PathBuf {
    if let Some(repo) = dir.ancestors().find(|a| a.join(".git").exists()) {
        return repo.to_path_buf();
    }
    match home.and_then(|h| std::fs::canonicalize(h).ok()) {
        Some(home) if home != Path::new("/") && dir.starts_with(&home) => home,
        _ => dir.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_images_follow_the_same_rules() {
        let top = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("mddog-native-media-{}", std::process::id()));
        let repo = top.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("docs/img")).unwrap();
        std::fs::write(repo.join("docs/img/a b.png"), b"x").unwrap();
        std::fs::write(repo.join("logo.png"), b"x").unwrap();
        std::fs::write(top.join("secret.png"), b"x").unwrap();
        let dir = repo.join("docs");
        let at = |u: &str| confined_media(u, &dir);
        let in_repo = |f: &str| Some(repo.join(f));
        assert_eq!(at("img/a%20b.png"), in_repo("docs/img/a b.png"));
        assert_eq!(at("img/a%20b.png?raw=1"), in_repo("docs/img/a b.png"));
        assert_eq!(at("/logo.png"), in_repo("logo.png"));
        assert_eq!(at(&format!("file://{}/docs/img/a%20b.png", repo.display())), in_repo("docs/img/a b.png"));
        for out in ["../../secret.png", "//host/x.png", "https://x.test/r.png", "", "img/a%2Fb.png"] {
            assert_eq!(at(out), None, "{out}");
        }
        assert_eq!(at(&format!("file://{}/secret.png", top.display())), None);
        assert_eq!(at(&format!("file://{}/../secret.png", repo.display())), None);
        let _ = std::fs::remove_dir_all(&top);
    }
    #[test]
    fn a_symlink_then_dots_cannot_leave_the_repo() {
        let top = std::env::temp_dir().join(format!("mddog-media-link-{}", std::process::id()));
        let repo = top.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("docs/img/x/y")).unwrap();
        std::os::unix::fs::symlink("img/x/y", repo.join("docs/l")).unwrap();
        // On disk l/../../.. is docs, where this decoy sits. A browser reads it as the folder above the repo.
        std::fs::write(repo.join("docs/secret.png"), b"x").unwrap();
        std::fs::write(top.join("secret.png"), b"x").unwrap();
        std::fs::write(repo.join("docs/img/x/y/in.png"), b"x").unwrap();
        // Decoys for paths a loose decode would check in place of the ones a browser opens.
        for d in ["docs/l\u{2}", "docs/l\u{fffd}"] {
            std::fs::create_dir_all(repo.join(d)).unwrap();
            std::fs::write(repo.join(d).join("in.png"), b"x").unwrap();
        }
        let dir = std::fs::canonicalize(repo.join("docs")).unwrap();
        let at = |u: &str| confined_media(u, &dir);
        let (out, inside) = (["l/../../../secret.png", "l%2F..%2F..%2F..%2Fsecret.png", "l%+2/in.png", "l%ff/in.png"].map(at), at("l/in.png"));
        let _ = std::fs::remove_dir_all(&top);
        assert_eq!(out, [None, None, None, None]);
        assert_eq!(inside, Some(dir.join("img/x/y/in.png")));
    }
    #[test]
    fn media_root_is_the_repo_then_home_then_the_folder() {
        let top = std::env::temp_dir().join(format!("mddog-root-{}", std::process::id()));
        for d in ["home/repo/.git", "home/repo/docs", "home/notes/a", "home/sibling", "other"] {
            std::fs::create_dir_all(top.join(d)).unwrap();
        }
        std::fs::write(top.join("home/sibling/img.png"), b"x").unwrap();
        let top = std::fs::canonicalize(&top).unwrap();
        let home = top.join("home");
        let at = |d: &str| media_root(&top.join(d), Some(&home));
        let (repo, notes, other) = (at("home/repo/docs"), at("home/notes/a"), at("other"));
        let dir = top.join("home/notes/a");
        let sibling = media_path("../../sibling/img.png", &dir, &notes);
        let rooted = media_path("/sibling/img.png", &dir, &notes);
        let _ = std::fs::remove_dir_all(&top);
        assert_eq!((repo, notes, other), (top.join("home/repo"), home.clone(), top.join("other")));
        assert_eq!(sibling, Some(home.join("sibling/img.png")));
        assert_eq!(rooted, sibling);
    }
    #[test]
    fn a_missing_or_root_home_keeps_the_folder() {
        let dir = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        for home in [None, Some(Path::new("")), Some(Path::new("/"))] {
            assert_eq!(media_root(&dir, home), dir, "{home:?}");
        }
    }
}
