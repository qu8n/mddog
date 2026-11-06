//! GitHub's math rules for the Markdown source.

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
