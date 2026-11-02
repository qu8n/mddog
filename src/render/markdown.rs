//! GitHub's math rules for the Markdown source, and the rules for which local images a page may show.

use std::path::{Component, Path, PathBuf};

fn escape_math(s: &str) -> String {
    s.replace('\\', "\\\\").replace('*', "\\*").replace('_', "\\_")
}

pub(crate) const HELD_DOLLAR: char = '\u{e024}';

fn hold_dollars(text: &str, masked: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut slashes = 0;
    for (i, c) in text.char_indices() {
        if c == '$' && slashes % 2 == 1 && masked[i..].starts_with('$') {
            out.pop();
            out.push(HELD_DOLLAR);
        } else {
            out.push(c);
        }
        slashes = if c == '\\' { slashes + 1 } else { 0 };
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Fence {
    Outside,
    Opens,
    Inside,
}

pub(crate) fn protect_math(src: &str) -> String {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let delim = |line: &str| {
        let u = unquote(line).0;
        u.trim() == "$$" && !u.starts_with("    ") && !u.starts_with('\t')
    };
    let mut out = String::with_capacity(src.len() + 16);
    let mut display: Option<usize> = None;
    let mut chunk = String::new();
    let flush = |chunk: &str, out: &mut String| {
        let html = html_block(chunk);
        out.push_str(&chunk[..html]);
        let chunk = &chunk[html..];
        let table = table_start(chunk);
        let escape = |m: &str, at: usize| if at >= table { escape_math(m).replace("\\\\|", "\\|") } else { escape_math(m) };
        let lone = chunk.trim().strip_prefix("$$").and_then(|r| r.strip_suffix("$$")).is_some_and(|m| !m.trim().is_empty() && !m.contains("$$") && !m.contains('`'));
        if chunk.starts_with("    ") || chunk.starts_with('\t') {
            out.push_str(chunk);
        } else if lone {
            out.push_str(&escape(chunk, 0));
        } else {
            let masked = mask_code_spans(chunk);
            let mut last = 0;
            for (a, b) in inline_math(&masked, true).into_iter().filter(|&(a, b)| !chunk[a..b].contains('`') && !chunk[a..b].contains("://") && !chunk[a..b].contains("www.")) {
                out.push_str(&hold_dollars(&chunk[last..a], &masked[last..a]));
                out.push_str(&escape(&chunk[a..b], a));
                last = b;
            }
            out.push_str(&hold_dollars(&chunk[last..], &masked[last..]));
        }
    };
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let fences: Vec<Fence> = lines
        .iter()
        .scan(None, |fence: &mut Option<(char, usize, usize)>, line| {
            let (u, depth) = unquote(line);
            let t = u.trim();
            // A fence in a block quote ends with the quote.
            if fence.is_some_and(|f| depth < f.2) {
                *fence = None;
            }
            let ticks = t.chars().take_while(|c| matches!(c, '`' | '~')).count();
            let open = t.chars().next().filter(|_| ticks >= 3);
            Some(if let Some((c, n, f_depth)) = *fence {
                if depth == f_depth && open == Some(c) && ticks >= n && t[ticks..].trim().is_empty() {
                    *fence = None;
                }
                Fence::Inside
            } else if let Some(c) = open.filter(|c| *c == '~' || !t[ticks..].contains('`')) {
                *fence = Some((c, ticks, depth));
                Fence::Opens
            } else {
                Fence::Outside
            })
        })
        .collect();
    // closes_later[i]: a `$$` line follows line i in the same block quote. A line quoted less
    // deeply ends the quotes in between, so it hides their delimiters from the lines above.
    let mut closes_later = vec![false; lines.len()];
    let depths: Vec<usize> = lines.iter().map(|l| unquote(l).1).collect();
    // below[d]: a `$$` at depth d follows with no shallower line between. It only grows on a
    // `$$` line, by at most that line's depth, so this pass stays linear.
    let mut below: Vec<bool> = Vec::new();
    for i in (0..lines.len()).rev() {
        let d = depths[i];
        closes_later[i] = below.get(d) == Some(&true);
        below.truncate(d + 1);
        if fences[i] == Fence::Outside && delim(lines[i]) {
            below.resize(d + 1, false);
            below[d] = true;
        }
    }
    for (i, line) in lines.iter().enumerate() {
        match fences[i] {
            Fence::Inside => out.push_str(line),
            Fence::Opens => {
                flush(&std::mem::take(&mut chunk), &mut out);
                out.push_str(line);
            }
            Fence::Outside if delim(line) && display.map_or(closes_later[i], |d| d == depths[i]) => {
                flush(&std::mem::take(&mut chunk), &mut out);
                display = if display.is_some() { None } else { Some(depths[i]) };
                out.push_str(line);
            }
            Fence::Outside if display.is_some() => out.push_str(&escape_math(line)),
            Fence::Outside if line.trim().is_empty() => {
                flush(&std::mem::take(&mut chunk), &mut out);
                out.push_str(line);
            }
            Fence::Outside => chunk.push_str(line),
        }
    }
    flush(&std::mem::take(&mut chunk), &mut out);
    out
}

/// Where the GFM table in a chunk of lines starts: its header row, the line before a delimiter
/// row with as many cells. `chunk.len()` when there is none.
fn table_start(chunk: &str) -> usize {
    let cells = |line: &str| line.trim().trim_start_matches('|').trim_end_matches('|').split('|').count();
    let delimiter = |line: &str| {
        let t = line.trim().trim_start_matches('|').trim_end_matches('|');
        line.contains('|')
            && t.split('|').all(|c| {
                let c = c.trim().trim_start_matches(':').trim_end_matches(':');
                !c.is_empty() && c.bytes().all(|b| b == b'-')
            })
    };
    let mut at = 0;
    let mut prev: Option<(usize, &str)> = None;
    for line in chunk.split_inclusive('\n') {
        if let Some((start, _)) = prev.filter(|(_, h)| delimiter(line) && cells(h) == cells(line)) {
            return start;
        }
        prev = Some((at, line));
        at += line.len();
    }
    chunk.len()
}

/// The line without its block quote markers, and how many it had.
fn unquote(line: &str) -> (&str, usize) {
    let (mut s, mut depth) = (line, 0);
    loop {
        let t = s.trim_start_matches(' ');
        match t.strip_prefix('>') {
            Some(r) if s.len() - t.len() <= 3 => (s, depth) = (r.strip_prefix(' ').unwrap_or(r), depth + 1),
            _ => return (s, depth),
        }
    }
}

/// The length of the HTML block a chunk of lines starts with, or 0 (CommonMark kinds 1, 2, 6,
/// and 7). cmark does not unescape inside one, so math there is left as written.
fn html_block(chunk: &str) -> usize {
    const TAGS: [&str; 62] = [
        "address",
        "article",
        "aside",
        "base",
        "basefont",
        "blockquote",
        "body",
        "caption",
        "center",
        "col",
        "colgroup",
        "dd",
        "details",
        "dialog",
        "dir",
        "div",
        "dl",
        "dt",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "frame",
        "frameset",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hr",
        "html",
        "iframe",
        "legend",
        "li",
        "link",
        "main",
        "menu",
        "menuitem",
        "nav",
        "noframes",
        "ol",
        "optgroup",
        "option",
        "p",
        "param",
        "search",
        "section",
        "summary",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "title",
        "tr",
        "track",
        "ul",
    ];
    let line = chunk.lines().next().unwrap_or_default();
    let t = line.trim_start();
    if line.len() - t.len() > 3 || !t.starts_with('<') {
        return 0;
    }
    // Kinds 1 and 2 end on the line with their end marker, and the rest of the chunk is Markdown.
    let through = |marker: &str| chunk.to_ascii_lowercase().find(marker).map_or(chunk.len(), |i| chunk[i..].find('\n').map_or(chunk.len(), |n| i + n + 1));
    if t.starts_with("<!--") {
        return through("-->");
    }
    let closing = t[1..].starts_with('/');
    let rest = if closing { &t[2..] } else { &t[1..] };
    let name = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
    let (name, after) = (rest[..name].to_ascii_lowercase(), &rest[name..]);
    let ends = after.is_empty() || after.starts_with(['>', ' ', '\t', '/']);
    if !closing && ends && ["pre", "script", "style", "textarea"].contains(&name.as_str()) {
        return through(&format!("</{name}>"));
    }
    let lone_tag = rest.starts_with(|c: char| c.is_ascii_alphabetic()) && ends && t.trim_end().ends_with('>') && !t[1..].contains('<');
    if TAGS.contains(&name.as_str()) && ends || lone_tag { chunk.len() } else { 0 }
}

fn mask_code_spans(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('`') {
        let n = rest[i..].bytes().take_while(|b| *b == b'`').count();
        let ticks = &rest[i..i + n];
        out.push_str(&rest[..i + n]);
        rest = &rest[i + n..];
        match rest.find(ticks) {
            Some(j) => {
                out.extend(rest[..j].chars().map(|c| if c == '$' { ' ' } else { c }));
                out.push_str(ticks);
                rest = &rest[j + n..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

fn is_remote(url: &str) -> bool {
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

/// The `$…$` and `$$…$$` spans of `s`, by GitHub's rules. An opener follows whitespace, `(`, or the
/// start, and comes before a character that is not whitespace or `$`. The next `$` (or `$$`) ends
/// the search. It closes the math unless it comes before a word character, sits between two of the
/// same punctuation mark, or (for `$`) follows whitespace and comes before whitespace or the end.
/// Math does not cross lines. `loose`, for the source pass, also takes an opener after the end of
/// other inline markup, so it finds every span the text pass finds.
pub fn inline_math(s: &str, loose: bool) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    if !b.contains(&b'$') {
        return Vec::new();
    }
    let n = b.len();
    let mut escaped = vec![false; n];
    for i in 1..n {
        escaped[i] = b[i - 1] == b'\\' && !escaped[i - 1];
    }
    let dollar = |j: usize| b[j] == b'$' && !escaped[j];
    // next[d - 1][k]: the first `$` (d = 1) or `$$` (d = 2) at or after k. line[k]: the first newline.
    let mut next = [vec![usize::MAX; n + 1], vec![usize::MAX; n + 1]];
    let mut line = vec![usize::MAX; n + 1];
    for j in (0..n).rev() {
        next[0][j] = if dollar(j) { j } else { next[0][j + 1] };
        next[1][j] = if dollar(j) && b.get(j + 1) == Some(&b'$') { j } else { next[1][j + 1] };
        line[j] = if b[j] == b'\n' { j } else { line[j + 1] };
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if !dollar(i) {
            i += 1;
            continue;
        }
        let delim = if b.get(i + 1) == Some(&b'$') { 2 } else { 1 };
        let open_end = i + delim;
        let prev = s[..i].chars().next_back();
        let prev_ok = prev.is_none_or(|c| c.is_whitespace() || c == '(' || loose && "*_`~)]>|".contains(c));
        let next_ok = s[open_end..].chars().next().is_some_and(|c| !c.is_whitespace() && c != '$');
        let j = next[delim - 1][open_end];
        let closes = j != usize::MAX && {
            let (before, after) = (s[..j].chars().next_back(), s[j + delim..].chars().next());
            !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !(before == after && after.is_some_and(|c| c.is_ascii_punctuation()))
                && (delim == 2 || !before.is_some_and(char::is_whitespace) || after.is_some_and(|c| !c.is_whitespace()))
                && line[open_end] > j
        };
        if prev_ok && next_ok && closes {
            out.push((i, j + delim));
            i = j + delim;
        } else {
            i += delim;
        }
    }
    out
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
    #[test]
    fn a_deep_quote_takes_linear_time() {
        let s = format!("{}$$\n{}", "> ".repeat(200_000), "x\n".repeat(200_000));
        let t = std::time::Instant::now();
        protect_math(&s);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }
    #[test]
    fn openers_without_closers_take_linear_time() {
        let s = "($a$a".repeat(100_000);
        let t = std::time::Instant::now();
        assert!(inline_math(&s, true).is_empty() && inline_math(&s, false).is_empty());
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }
}
