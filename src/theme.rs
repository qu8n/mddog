//! Themes: one CSS file that the page loads after the style's CSS, and whose `color-scheme` and `--mddog-*` colors the terminal chrome draws with. A theme can recolor the page, as the GitHub themes do, or restyle all of it, as the Obsidian themes do.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The built-in themes, one line each.
#[rustfmt::skip]
const BUILTIN: &[(&str, &str)] = &[
    ("github-light", include_str!("../assets/themes/github-light.css")),
    ("github-dark", include_str!("../assets/themes/github-dark.css")),
    ("obsidian-light", include_str!("../assets/themes/obsidian-light.css")),
    ("obsidian-dark", include_str!("../assets/themes/obsidian-dark.css")),
];

pub type Rgb = (u8, u8, u8);

/// The colors the terminal chrome draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub fg_muted: Rgb,
    pub fg_bright: Rgb,
    pub selection_bg: Rgb,
    pub bar_bg: Rgb,
    pub border: Rgb,
    pub skeleton: Rgb,
}

/// The `--mddog-*` names, in `Palette`'s field order.
pub const VARS: [&str; 8] = ["bg", "fg", "fg-muted", "fg-bright", "selection-bg", "bar-bg", "border", "skeleton"];

impl Palette {
    fn from(c: [Rgb; 8]) -> Palette {
        Palette { bg: c[0], fg: c[1], fg_muted: c[2], fg_bright: c[3], selection_bg: c[4], bar_bg: c[5], border: c[6], skeleton: c[7] }
    }

    fn colors(&self) -> [Rgb; 8] {
        [self.bg, self.fg, self.fg_muted, self.fg_bright, self.selection_bg, self.bar_bg, self.border, self.skeleton]
    }

    /// Resets the SGR attributes and sets the background back to the theme's.
    pub fn reset(&self) -> String {
        format!("\x1b[0m{}", bg(self.bg))
    }
}

pub fn fg((r, g, b): Rgb) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

pub fn bg((r, g, b): Rgb) -> String {
    format!("\x1b[48;2;{r};{g};{b}m")
}

pub fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Reads `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, dropping the alpha.
pub fn rgb(s: &str) -> Option<Rgb> {
    let h = s.strip_prefix('#')?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v: Vec<u8> = match h.len() {
        3 | 4 => h.chars().take(3).map(|c| c.to_digit(16).map(|d| d as u8 * 17)).collect::<Option<_>>()?,
        6 | 8 => (0..3).map(|i| u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()).collect::<Option<_>>()?,
        _ => return None,
    };
    Some((v[0], v[1], v[2]))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The built-in name, or the file's path as the config gave it.
    pub name: String,
    pub dark: bool,
    pub css: Cow<'static, str>,
    pub palette: Palette,
    /// The theme its `--mddog-base` names, which the page loads first.
    pub base: Option<Box<Theme>>,
    /// The Mermaid engine its `--mddog-mermaid` names, or its base's. The config's engine wins.
    pub mermaid: Option<crate::config::Mermaid>,
}

impl Theme {
    /// The themes the page loads, first to last: GitHub's theme of this mode (unless the chain starts there), each base, then this one.
    pub fn layers(&self) -> Vec<&Theme> {
        let mut out = vec![self];
        while let Some(b) = out[out.len() - 1].base.as_deref() {
            out.push(b);
        }
        let github = github(self.dark);
        if out[out.len() - 1].name != github.name {
            out.push(github);
        }
        out.reverse();
        out
    }
}

fn github(dark: bool) -> &'static Theme {
    builtin(if dark { "github-dark" } else { "github-light" }).expect("github theme")
}

pub fn builtin(name: &str) -> Option<&'static Theme> {
    static ALL: LazyLock<Vec<Theme>> = LazyLock::new(|| BUILTIN.iter().map(|(n, css)| parse(n, Cow::Borrowed(css)).expect("built-in theme")).collect());
    ALL.iter().find(|t| t.name == name)
}

pub fn names() -> String {
    BUILTIN.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

/// Whether a theme name is a file's path rather than a built-in's name.
pub fn is_file(name: &str) -> bool {
    name.ends_with(".css") || name.contains('/')
}

/// A theme file's path: `~/` is the home folder, and a relative path starts from `dir`.
pub fn resolve(dir: &Path, f: &str) -> PathBuf {
    match f.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map_or_else(|| PathBuf::from(f), |h| PathBuf::from(h).join(rest)),
        None => dir.join(f),
    }
}

/// Reads a theme file and the chain of bases it names.
pub fn file(path: &Path) -> Result<Theme, String> {
    load(path, &mut Vec::new())
}

fn load(path: &Path, seen: &mut Vec<PathBuf>) -> Result<Theme, String> {
    let shown = path.display().to_string();
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.contains(&key) {
        return Err(format!("the chain of base themes loops back to {}", key.display()));
    }
    let css = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {}", crate::term::io_error(&e)))?;
    let dir = path.parent().unwrap_or(Path::new(""));
    seen.push(key);
    read(&shown, relative_urls(&css, dir).into(), &mut |f| load(&resolve(dir, f), seen))
}

/// Reads a built-in theme, or theme text whose base, if any, is a built-in.
pub fn parse(name: &str, css: Cow<'static, str>) -> Result<Theme, String> {
    read(name, css, &mut |f| Err(format!("{f}: only a theme file can have a file as its base")))
}

/// Reads a theme's base, mode and chrome colors from its `:root` rules. `file` reads a base that is a file. A color the theme leaves out comes from its base, else from GitHub's theme of the same mode.
fn read(name: &str, css: Cow<'static, str>, file: &mut dyn FnMut(&str) -> Result<Theme, String>) -> Result<Theme, String> {
    // Only `:root` rules count, so a component's own color-scheme or --mddog-* value can't stand in for the theme's.
    let decls = root_decls(&css);
    // A custom property's name is case-sensitive, a standard property's isn't.
    let last = |prop: &str| decls.iter().rev().find(|(p, _)| if prop.starts_with("--") { p == prop } else { p.eq_ignore_ascii_case(prop) }).map(|(_, v)| v.as_str());
    let base = match last("--mddog-base").map(|v| v.trim_matches(['"', '\''])) {
        None => None,
        Some(b) if is_file(b) => Some(file(b).map_err(|e| format!("{name}: --mddog-base: {e}"))?),
        Some(b) => Some(builtin(b).cloned().ok_or_else(|| format!("{name}: --mddog-base: unknown theme {b:?}. Built-in themes: {}. A theme file's path ends in .css", names()))?),
    };
    let own = match last("color-scheme") {
        Some("dark") => Some(true),
        Some("light") => Some(false),
        Some(v) => return Err(format!("{name}: color-scheme must be light or dark, but got {v:?}")),
        None => None,
    };
    let mode = |dark| if dark { "dark" } else { "light" };
    let dark = match (own, &base) {
        (Some(d), Some(b)) if d != b.dark => return Err(format!("{name}: color-scheme is {}, but its base {} is {}", mode(d), b.name, mode(b.dark))),
        (Some(d), _) => d,
        (None, Some(b)) => b.dark,
        (None, None) => return Err(format!("{name}: set color-scheme: light or dark on :root, or a base theme with --mddog-base")),
    };
    let mermaid = match last("--mddog-mermaid").map(|v| v.trim_matches(['"', '\''])) {
        None => base.as_ref().and_then(|b| b.mermaid),
        Some("pele") => Some(crate::config::Mermaid::Pele),
        Some("obsidian") => Some(crate::config::Mermaid::Obsidian),
        Some(v) => return Err(format!("{name}: --mddog-mermaid must be obsidian or pele, but got {v:?}")),
    };
    let mut set: [Option<Rgb>; 8] = [None; 8];
    for (p, value) in &decls {
        let Some(i) = p.strip_prefix("--mddog-").and_then(|v| VARS.iter().position(|x| x == &v)) else { continue };
        set[i] = Some(rgb(value).filter(|_| matches!(value.len(), 4 | 7)).ok_or_else(|| format!("{name}: {p}: expected a hex color such as #0d1117, but got {value:?}"))?);
    }
    let colors = if set.iter().all(Option::is_some) {
        set.map(Option::unwrap_or_default)
    } else {
        let under = base.as_ref().map_or_else(|| github(dark).palette, |b| b.palette).colors();
        std::array::from_fn(|i| set[i].unwrap_or(under[i]))
    };
    Ok(Theme { name: name.to_string(), dark, css, palette: Palette::from(colors), base: base.map(Box::new), mermaid })
}

/// The declarations, as (property, value), of the top-level rules whose selector list has `:root`. Rules inside `@media` and the like may not apply, so they don't count, and neither do nested rules.
fn root_decls(css: &str) -> Vec<(String, String)> {
    let (clean, mask) = masks(css);
    let mut out = Vec::new();
    let mut push = |from: usize, to: usize| {
        if let Some(colon) = mask[from..to].find(':') {
            out.push((clean[from..from + colon].trim().to_string(), clean[from + colon + 1..to].trim().to_string()));
        }
    };
    let (mut depth, mut start, mut decl, mut root) = (0usize, 0, 0, false);
    for (i, c) in mask.bytes().enumerate() {
        match c {
            b'{' => {
                if depth == 0 {
                    root = mask[start..i].split(',').any(|s| s.trim() == ":root");
                    decl = i + 1;
                }
                depth += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if root {
                        push(decl, i);
                    }
                    root = false;
                    start = i + 1;
                } else if depth == 1 {
                    decl = i + 1;
                }
            }
            b';' if depth == 0 => start = i + 1,
            b';' if depth == 1 && root => {
                push(decl, i);
                decl = i + 1;
            }
            _ => {}
        }
    }
    out
}

/// The CSS with its comments blanked to spaces, and that again with the contents of its strings blanked to `x`. Both keep every byte offset, so a scan of the second can slice values out of the first.
fn masks(css: &str) -> (String, String) {
    let b = css.as_bytes();
    let (mut clean, mut mask) = (b.to_vec(), b.to_vec());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = css[i + 2..].find("*/").map_or(b.len(), |e| i + e + 4);
                clean[i..end].fill(b' ');
                mask[i..end].fill(b' ');
                i = end;
            }
            q @ (b'"' | b'\'') => {
                let mut j = i + 1;
                while j < b.len() && b[j] != q && !matches!(b[j], b'\n' | b'\r' | b'\x0c') {
                    j += match (b[j], b.get(j + 1..j + 3)) {
                        (b'\\', Some(b"\r\n")) => 3,
                        (b'\\', _) => 2,
                        _ => 1,
                    };
                }
                let j = j.min(b.len());
                mask[i + 1..j].fill(b'x');
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    let text = |v| String::from_utf8(v).expect("only whole characters are blanked");
    (text(clean), text(mask))
}

/// Points each relative `url()` and `@import` at `dir`, the CSS file's folder. Otherwise they would start from the page's `<base>`, the Markdown file's folder.
pub fn relative_urls(css: &str, dir: &Path) -> String {
    static URL: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r#"(?i)(?:^|[^\w-])url\(\s*(?:"(x*)"|'(x*)'|([^\s"')]+))|@import\s+(?:"(x*)"|'(x*)')"#).expect("valid regex"));
    static SCHEME: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").expect("valid regex"));
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let (_, mask) = masks(css);
    let (mut out, mut last) = (String::with_capacity(css.len()), 0);
    for c in URL.captures_iter(&mask) {
        let Some(m) = c.iter().skip(1).flatten().next() else { continue };
        let target = &css[m.range()];
        if target.is_empty() || target.starts_with(['#', '/']) || SCHEME.is_match(target) {
            continue;
        }
        out.push_str(&css[last..m.start()]);
        out.push_str(&format!("{}/{target}", crate::page::file_url(&dir)));
        last = m.end();
    }
    out.push_str(&css[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_set_every_color() {
        for (name, css) in BUILTIN {
            let text = masks(css).0;
            for v in VARS {
                assert!(text.contains(&format!("--mddog-{v}:")), "{name} lacks --mddog-{v}");
            }
        }
        let dark = builtin("github-dark").unwrap();
        assert!(dark.dark);
        assert_eq!(dark.palette.bg, (13, 17, 23));
        assert_eq!(dark.palette.bar_bg, (22, 27, 34));
        assert!(!builtin("github-light").unwrap().dark);
        assert!(builtin("nope").is_none());
    }

    #[test]
    fn missing_colors_come_from_github() {
        let t = parse("x.css", ":root { color-scheme: light; --mddog-bg: #fafafa; /* --mddog-fg: #fff; */ --mddog-fg: #123 }".into()).unwrap();
        let light = builtin("github-light").unwrap().palette;
        assert_eq!(t.palette.bg, (250, 250, 250));
        assert_eq!(t.palette.fg, (17, 34, 51));
        assert_eq!(t.palette.fg_muted, light.fg_muted);
        assert_eq!(t.palette.skeleton, light.skeleton);
        let t = parse("y.css", "@media (prefers-color-scheme: dark) { a { color: red } }\n:root{color-scheme:dark}".into()).unwrap();
        assert_eq!(t.palette, builtin("github-dark").unwrap().palette);
    }

    #[test]
    fn bad_themes_say_why() {
        assert_eq!(parse("x.css", ":root{--mddog-bg:#fff}".into()).unwrap_err(), "x.css: set color-scheme: light or dark on :root, or a base theme with --mddog-base");
        assert_eq!(parse("x.css", ":root{color-scheme:light dark}".into()).unwrap_err(), "x.css: color-scheme must be light or dark, but got \"light dark\"");
        let scoped = parse("x.css", ".a{color-scheme:light;--mddog-bg:#fff}\n:root { color-scheme: dark }\n.b{--mddog-bg:#fff}".into()).unwrap();
        assert!(scoped.dark && scoped.palette == builtin("github-dark").unwrap().palette);
        // The last top-level declaration wins, as in the cascade. A :root inside @media may not apply, so it doesn't count.
        assert!(parse("x.css", ":root{color-scheme:dark}@media print{:root{color-scheme:light}}".into()).unwrap().dark);
        assert!(!parse("x.css", "@import url(a.css);:root,html{color-scheme:dark}:root{color-scheme:light}".into()).unwrap().dark);
        let e = parse("x.css", ":root{color-scheme:dark;--mddog-fg:var(--x)}".into()).unwrap_err();
        assert_eq!(e, "x.css: --mddog-fg: expected a hex color such as #0d1117, but got \"var(--x)\"");
        assert!(parse("x.css", ":root{color-scheme:dark;--mddog-fg:#ffffff80}".into()).is_err());
    }

    #[test]
    fn quotes_hide_braces_and_comments() {
        let red = |css: &str| parse("x.css", css.to_string().into()).map(|t| (t.dark, t.palette.bg));
        assert_eq!(red(":root{ --icon:\"}\"; color-scheme:dark; --mddog-bg:#ff0000; }"), Ok((true, (255, 0, 0))));
        assert_eq!(red(":root{ --icon:\"{\"; color-scheme:dark; --mddog-bg:#ff0000; }"), Ok((true, (255, 0, 0))));
        assert_eq!(red(":root{ color-scheme:dark; --icon:\"}\"; --mddog-bg:#ff0000; }"), Ok((true, (255, 0, 0))));
        assert_eq!(red(":root{ --a:'\\';}'; color-scheme:dark; --mddog-bg:#ff0000 }"), Ok((true, (255, 0, 0))));
        assert_eq!(red(":root{ --a:\"\\\r\n}\"; color-scheme:dark; --mddog-bg:#ff0000; --MDDOG-BASE:nope }"), Ok((true, (255, 0, 0))));
        assert_eq!(red("a::before{content:\"/*\"}:root{color-scheme:dark;--mddog-bg:#ff0000}/* */"), Ok((true, (255, 0, 0))));
        assert_eq!(red(":root{color-scheme:dark;--x:\"color-scheme: light; --mddog-bg: #fff\";--mddog-bg:#ff0000}"), Ok((true, (255, 0, 0))));
        // A nested rule's declarations aren't the root's.
        assert_eq!(red(":root{color-scheme:dark;& a{--mddog-bg:#fff}--mddog-bg:#ff0000}"), Ok((true, (255, 0, 0))));
    }

    #[test]
    fn a_base_fills_in_the_rest() {
        let obsidian = builtin("obsidian-dark").unwrap();
        let t = parse("x.css", ":root { --mddog-base: obsidian-dark; --mddog-bg: #202a44; }".into()).unwrap();
        assert!(t.dark);
        assert_eq!(t.palette, Palette { bg: (32, 42, 68), ..obsidian.palette });
        let names: Vec<_> = t.layers().iter().map(|l| l.name.clone()).collect();
        assert_eq!(names, ["github-dark", "obsidian-dark", "x.css"]);
        assert_eq!(builtin("github-dark").unwrap().layers().len(), 1);
        let on_github = parse("x.css", ":root{--mddog-base:\"github-light\"}".into()).unwrap();
        assert_eq!(on_github.layers().len(), 2);
        assert!(!on_github.dark);
        let e = parse("x.css", ":root{--mddog-base:obsidian-dark;color-scheme:light}".into()).unwrap_err();
        assert_eq!(e, "x.css: color-scheme is light, but its base obsidian-dark is dark");
        let e = parse("x.css", ":root{--mddog-base:nope}".into()).unwrap_err();
        assert!(e.starts_with("x.css: --mddog-base: unknown theme \"nope\". Built-in themes: github-light"), "{e}");
    }

    #[test]
    fn a_theme_picks_its_mermaid_engine() {
        use crate::config::Mermaid;
        assert_eq!(builtin("obsidian-dark").unwrap().mermaid, Some(Mermaid::Obsidian));
        assert_eq!(builtin("obsidian-light").unwrap().mermaid, Some(Mermaid::Obsidian));
        assert_eq!(builtin("github-dark").unwrap().mermaid, Some(Mermaid::Pele));
        assert_eq!(builtin("github-light").unwrap().mermaid, Some(Mermaid::Pele));
        assert_eq!(parse("x.css", ":root{--mddog-base:obsidian-dark}".into()).unwrap().mermaid, Some(Mermaid::Obsidian));
        assert_eq!(parse("x.css", ":root{--mddog-base:obsidian-dark;--mddog-mermaid:'pele'}".into()).unwrap().mermaid, Some(Mermaid::Pele));
        let e = parse("x.css", ":root{color-scheme:dark;--mddog-mermaid:dot}".into()).unwrap_err();
        assert_eq!(e, "x.css: --mddog-mermaid must be obsidian or pele, but got \"dot\"");
        // No config loaded in tests, so the theme's choice stands.
        assert_eq!(crate::config::mermaid(builtin("obsidian-dark").unwrap()), Mermaid::Obsidian);
        assert_eq!(crate::config::mermaid(builtin("github-dark").unwrap()), Mermaid::Pele);
    }

    #[test]
    fn base_files_chain_and_loops_fail() {
        let dir = std::env::temp_dir().join(format!("mddog-base-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.css"), ":root{--mddog-base:sub/b.css;--mddog-fg:#010203}").unwrap();
        std::fs::write(dir.join("sub/b.css"), ":root{--mddog-base:obsidian-light;--mddog-bg:#fefefe}\nh1{background:url(img.png)}").unwrap();
        let a = file(&dir.join("a.css")).unwrap();
        assert!(!a.dark);
        assert_eq!((a.palette.bg, a.palette.fg, a.palette.border), ((254, 254, 254), (1, 2, 3), builtin("obsidian-light").unwrap().palette.border));
        let names: Vec<_> = a.layers().iter().map(|l| l.name.clone()).collect();
        assert_eq!(names[..2], ["github-light", "obsidian-light"]);
        // A base's own URLs start from its own folder.
        assert!(a.layers()[2].css.contains(&format!("url({}/img.png)", crate::page::file_url(&dir.join("sub")))));
        std::fs::write(dir.join("sub/b.css"), ":root{--mddog-base:../a.css}").unwrap();
        let e = file(&dir.join("a.css")).unwrap_err();
        let (a, b) = (dir.join("a.css").display().to_string(), dir.join("sub/b.css").display().to_string());
        assert_eq!(e, format!("{a}: --mddog-base: {b}: --mddog-base: the chain of base themes loops back to {}", dir.join("a.css").canonicalize().unwrap().display()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn relative_urls_start_at_the_file() {
        let dir = Path::new("/t/my theme");
        let at = "file:///t/my%20theme";
        let css = "@font-face{src:url( \"f.woff2\" ) format('woff2'),url(../f.ttf)}@import 'more.css';@import url(x.css);a{b:url(data:image/png;base64,AA),url(#i),url(/abs.png),url(https://x/y.png),url(file:///z),url('')}/* url(c.png) */i::after{content:\"url(d.png)\"}";
        let want = format!(
            "@font-face{{src:url( \"{at}/f.woff2\" ) format('woff2'),url({at}/../f.ttf)}}@import '{at}/more.css';@import url({at}/x.css);a{{b:url(data:image/png;base64,AA),url(#i),url(/abs.png),url(https://x/y.png),url(file:///z),url('')}}/* url(c.png) */i::after{{content:\"url(d.png)\"}}"
        );
        assert_eq!(relative_urls(css, dir), want);
    }

    #[test]
    fn obsidian_modes_share_their_rules() {
        // Each file is its :root block of colors, then the same rules.
        let rules = |name: &str| {
            let css = BUILTIN.iter().find(|(n, _)| *n == name).unwrap().1;
            css.split_once("\n}\n").unwrap().1
        };
        assert_eq!(rules("obsidian-light"), rules("obsidian-dark"));
        assert!(!builtin("obsidian-light").unwrap().dark && builtin("obsidian-dark").unwrap().dark);
    }

    #[test]
    fn rgb_reads_hex() {
        assert_eq!(rgb("#0d1117"), Some((13, 17, 23)));
        assert_eq!(rgb("#fff8"), Some((255, 255, 255)));
        assert_eq!(rgb("#+1+1+1"), None);
        assert_eq!(rgb("var(--bgColor-default)"), None);
        assert_eq!(hex((13, 17, 23)), "#0d1117");
    }

    fn luminance((r, g, b): Rgb) -> f64 {
        let l = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b)
    }

    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn builtin_chrome_text_is_readable() {
        for (name, _) in BUILTIN {
            let p = builtin(name).unwrap().palette;
            let bg = p.bg;
            for (what, fg, ground, min) in [
                ("fg on bg", p.fg, bg, 4.5),
                ("fg-bright on bg", p.fg_bright, bg, 4.5),
                ("fg-bright on selection", p.fg_bright, p.selection_bg, 4.5),
                ("fg-muted on bg", p.fg_muted, bg, 4.5),
                ("fg-muted on bar", p.fg_muted, p.bar_bg, 4.5),
                ("fg-bright on bar", p.fg_bright, p.bar_bg, 4.5),
            ] {
                let c = contrast(fg, ground);
                assert!(c >= min, "{name}: {what} is {c:.2}:1");
            }
        }
    }
}
