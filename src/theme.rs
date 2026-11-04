//! Themes: one file of `:root` tokens. The terminal chrome draws with its `color-scheme` and terminal colors, and the page with its page colors, syntax colors and type.

use std::collections::BTreeMap;
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

/// The terminal colors' token names, in `Palette`'s field order.
pub const VARS: [&str; 8] = ["bg", "fg", "fg-muted", "fg-bright", "selection-bg", "bar-bg", "border", "skeleton"];

impl Palette {
    fn from(c: [Rgb; 8]) -> Palette {
        Palette { bg: c[0], fg: c[1], fg_muted: c[2], fg_bright: c[3], selection_bg: c[4], bar_bg: c[5], border: c[6], skeleton: c[7] }
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

/// What a token holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `#rgb` or `#rrggbb`.
    Color,
    /// Font names, first choice first, as CSS's `font-family` lists them.
    Fonts,
    /// Six font weights from 100 to 900, one per heading level.
    Weights,
    /// A count of positive numbers, and what the error asks for.
    Nums(usize, &'static str),
    /// One of these words.
    Choice(&'static [&'static str]),
}

/// Every token but `--mddog-base`, without the `--mddog-` prefix. GitHub's themes set them all, so any other theme can leave some out.
#[rustfmt::skip]
const TOKENS: &[(&str, Kind)] = &[
    // The terminal chrome (VARS).
    ("bg", Kind::Color), ("fg", Kind::Color), ("fg-muted", Kind::Color), ("fg-bright", Kind::Color),
    ("selection-bg", Kind::Color), ("bar-bg", Kind::Color), ("border", Kind::Color), ("skeleton", Kind::Color),
    // The page.
    ("text", Kind::Color), ("text-muted", Kind::Color), ("heading", Kind::Color), ("link", Kind::Color), ("accent", Kind::Color),
    ("code-bg", Kind::Color), ("pre-bg", Kind::Color), ("alt-bg", Kind::Color), ("rule", Kind::Color), ("mark-bg", Kind::Color),
    ("match-bg", Kind::Color), ("match-current-bg", Kind::Color), ("text-selection", Kind::Color),
    ("alert-note", Kind::Color), ("alert-tip", Kind::Color), ("alert-important", Kind::Color), ("alert-warning", Kind::Color), ("alert-caution", Kind::Color),
    ("diff-add-bg", Kind::Color), ("diff-del-bg", Kind::Color), ("button-bg", Kind::Color),
    // Code.
    ("pl-comment", Kind::Color), ("pl-keyword", Kind::Color), ("pl-string", Kind::Color), ("pl-constant", Kind::Color),
    ("pl-entity", Kind::Color), ("pl-inserted", Kind::Color), ("pl-deleted", Kind::Color), ("pl-variable", Kind::Color), ("pl-tag", Kind::Color),
    // Type.
    ("font-body", Kind::Fonts), ("font-code", Kind::Fonts),
    ("heading-scale", Kind::Nums(6, "six sizes in em, one per heading level, such as 2 1.5 1.25 1 0.875 0.85")),
    ("heading-weight", Kind::Weights),
    ("line-height", Kind::Nums(1, "a positive number such as 1.5")), ("code-size", Kind::Nums(1, "a positive number such as 0.85")),
    ("code-line-height", Kind::Nums(1, "a positive number such as 1.45")),
    // The rules that place blocks: GitHub's README or Obsidian's reading view.
    ("layout", Kind::Choice(&["github", "obsidian"])),
];

/// A token's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Color(Rgb),
    Fonts(Vec<String>),
    Weights([u16; 6]),
    Nums(Vec<f32>),
    Choice(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The built-in name, or the file's path as the config gave it.
    pub name: String,
    pub dark: bool,
    pub palette: Palette,
    /// Every token, with the ones the theme leaves out taken from its base, else from GitHub's theme of its mode.
    tokens: BTreeMap<&'static str, Value>,
}

impl Theme {
    fn token(&self, name: &str) -> &Value {
        self.tokens.get(name).unwrap_or_else(|| panic!("no token {name}"))
    }

    /// A color token, by its name without `--mddog-`.
    pub fn color(&self, name: &str) -> Rgb {
        match self.token(name) {
            Value::Color(c) => *c,
            v => panic!("{name} is {v:?}, not a color"),
        }
    }

    pub fn fonts(&self, name: &str) -> &[String] {
        match self.token(name) {
            Value::Fonts(f) => f,
            v => panic!("{name} is {v:?}, not fonts"),
        }
    }

    /// A token of numbers.
    pub fn nums(&self, name: &str) -> &[f32] {
        match self.token(name) {
            Value::Nums(n) => n,
            v => panic!("{name} is {v:?}, not numbers"),
        }
    }

    /// A token of one number.
    pub fn number(&self, name: &str) -> f32 {
        self.nums(name)[0]
    }

    pub fn choice(&self, name: &str) -> &'static str {
        match self.token(name) {
            Value::Choice(c) => c,
            v => panic!("{name} is {v:?}, not a word"),
        }
    }

    pub fn heading_scale(&self) -> [f32; 6] {
        self.nums("heading-scale").try_into().expect("six sizes")
    }

    pub fn heading_weight(&self) -> [u16; 6] {
        match self.token("heading-weight") {
            Value::Weights(w) => *w,
            v => panic!("heading-weight is {v:?}"),
        }
    }
}

fn github(dark: bool) -> &'static Theme {
    builtin(if dark { "github-dark" } else { "github-light" }).expect("github theme")
}

pub fn builtin(name: &str) -> Option<&'static Theme> {
    static ALL: LazyLock<Vec<Theme>> = LazyLock::new(|| {
        // The GitHub themes come first, so the others can fill in from them.
        let mut all: Vec<Theme> = Vec::new();
        for (n, css) in BUILTIN {
            let under = all.iter().find(|t: &&Theme| t.name.starts_with("github-") && css.contains(if t.dark { "color-scheme: dark" } else { "color-scheme: light" })).cloned();
            all.push(read(n, css, under.as_ref(), &mut |f| Err(format!("{f}: a built-in theme has no file base"))).expect("built-in theme"));
        }
        all
    });
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
    read(&shown, &css, None, &mut |f| load(&resolve(dir, f), seen))
}

/// Reads a theme's tokens. `file` reads a base that is a file. `under` stands in for GitHub's theme while the built-ins load.
fn read(name: &str, css: &str, under: Option<&Theme>, file: &mut dyn FnMut(&str) -> Result<Theme, String>) -> Result<Theme, String> {
    let decls = root_decls(css).map_err(|e| format!("{name}: {e}"))?;
    let last = |prop: &str| decls.iter().rev().find(|(p, _)| p == prop).map(|(_, v)| v.as_str());
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
    let mut tokens = match (&base, under) {
        (Some(b), _) => b.tokens.clone(),
        (None, Some(u)) => u.tokens.clone(),
        // GitHub's own themes start empty, and a test checks they set every token.
        (None, None) if name.starts_with("github-") && BUILTIN.iter().any(|(n, _)| *n == name) => BTreeMap::new(),
        (None, None) => github(dark).tokens.clone(),
    };
    for (p, value) in &decls {
        if matches!(p.as_str(), "color-scheme" | "--mddog-base") {
            continue;
        }
        let Some((key, kind)) = p.strip_prefix("--mddog-").and_then(|t| TOKENS.iter().find(|(k, _)| *k == t)) else {
            return Err(format!("{name}: {p}: unknown token. A theme sets color-scheme and the --mddog-* tokens that docs/themes.md lists"));
        };
        tokens.insert(key, value_of(*kind, value).ok_or_else(|| format!("{name}: {p}: expected {}, but got {value:?}", expected(*kind)))?);
    }
    let colors: Vec<Rgb> = VARS
        .iter()
        .map(|v| match tokens.get(v) {
            Some(Value::Color(c)) => *c,
            _ => (0, 0, 0),
        })
        .collect();
    let palette = Palette::from(std::array::from_fn(|i| colors[i]));
    Ok(Theme { name: name.to_string(), dark, palette, tokens })
}

fn expected(kind: Kind) -> String {
    match kind {
        Kind::Color => "a hex color such as #0d1117".into(),
        Kind::Fonts => "a list of font names such as \"SF Pro Text\", system-ui".into(),
        Kind::Weights => "six font weights from 100 to 900, one per heading level, such as 700 680 660 640 620 600".into(),
        Kind::Nums(_, what) => what.into(),
        Kind::Choice(words) => words.join(" or "),
    }
}

fn value_of(kind: Kind, v: &str) -> Option<Value> {
    match kind {
        Kind::Color => rgb(v).filter(|_| matches!(v.len(), 4 | 7)).map(Value::Color),
        Kind::Fonts => {
            let names: Vec<String> = v.split(',').map(|f| f.trim().trim_matches(['"', '\'']).trim().to_string()).collect();
            // A font name is plain text: letters, digits, spaces and `-_.`.
            names.iter().all(|n| !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || " -_.".contains(c))).then_some(Value::Fonts(names))
        }
        Kind::Weights => {
            let weights: Vec<u16> = v.split_whitespace().map(|w| w.parse().ok().filter(|w| (100..=900).contains(w))).collect::<Option<_>>()?;
            <[u16; 6]>::try_from(weights).ok().map(Value::Weights)
        }
        Kind::Nums(n, _) => {
            let nums: Vec<f32> = v.split_whitespace().map(|s| s.parse::<f32>().ok().filter(|x| x.is_finite() && *x > 0.0)).collect::<Option<_>>()?;
            (nums.len() == n).then_some(Value::Nums(nums))
        }
        Kind::Choice(words) => words.iter().find(|w| **w == v).map(|w| Value::Choice(w)),
    }
}

/// The declarations, as (property, value), of a theme's `:root` rules. Anything else in the file is an error, so a theme written for the old CSS contract says so instead of losing its rules.
fn root_decls(css: &str) -> Result<Vec<(String, String)>, String> {
    let (clean, mask) = masks(css);
    let mut out = Vec::new();
    let mut rest = 0;
    while let Some(open) = mask[rest..].find('{').map(|i| rest + i) {
        let selector = clean[rest..open].trim();
        if selector != ":root" {
            return Err(format!("a theme holds only `:root {{ ... }}` tokens, but it has {:?}", selector.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
        let close = mask[open..].find('}').map(|i| open + i).ok_or("a `:root {` has no closing `}`")?;
        if mask[open + 1..close].contains('{') {
            return Err("a theme holds only `:root { ... }` tokens, but its :root has a nested rule".into());
        }
        for (i, d) in mask[open + 1..close].split(';').scan(open + 1, |at, d| {
            let start = *at;
            *at += d.len() + 1;
            Some((start, d))
        }) {
            if d.trim().is_empty() {
                continue;
            }
            let colon = d.find(':').ok_or_else(|| format!("expected a token such as --mddog-bg: #0d1117, but got {:?}", clean[i..i + d.len()].trim()))?;
            out.push((clean[i..i + colon].trim().to_string(), clean[i + colon + 1..i + d.len()].trim().to_string()));
        }
        rest = close + 1;
    }
    let tail = clean[rest..].trim();
    if !tail.is_empty() {
        return Err(format!("a theme holds only `:root {{ ... }}` tokens, but it has {:?}", tail.split_whitespace().collect::<Vec<_>>().join(" ")));
    }
    Ok(out)
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
                    j += if b[j] == b'\\' { 2 } else { 1 };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads theme text whose base, if any, is a built-in.
    fn parse(name: &str, css: &str) -> Result<Theme, String> {
        read(name, css, None, &mut |f| Err(format!("{f}: only a theme file can have a file as its base")))
    }

    #[test]
    fn github_themes_set_every_token() {
        for name in ["github-light", "github-dark"] {
            let css = BUILTIN.iter().find(|(n, _)| *n == name).unwrap().1;
            let decls = root_decls(css).unwrap();
            for (t, _) in TOKENS {
                assert!(decls.iter().any(|(p, _)| *p == format!("--mddog-{t}")), "{name} lacks --mddog-{t}");
            }
        }
        for (name, _) in BUILTIN {
            assert_eq!(builtin(name).unwrap().tokens.len(), TOKENS.len(), "{name}");
        }
        let dark = builtin("github-dark").unwrap();
        assert!(dark.dark);
        assert_eq!(dark.palette.bg, (13, 17, 23));
        assert_eq!(dark.palette.bar_bg, (22, 27, 34));
        assert_eq!(dark.color("link"), (0x44, 0x93, 0xf8));
        assert!(!builtin("github-light").unwrap().dark);
        assert!(builtin("nope").is_none());
    }

    #[test]
    fn missing_tokens_come_from_github() {
        let t = parse("x.css", ":root { color-scheme: light; --mddog-bg: #fafafa; /* --mddog-fg: #fff; */ --mddog-fg: #123 }").unwrap();
        let light = builtin("github-light").unwrap();
        assert_eq!(t.palette.bg, (250, 250, 250));
        assert_eq!(t.palette.fg, (17, 34, 51));
        assert_eq!(t.palette.fg_muted, light.palette.fg_muted);
        assert_eq!(t.color("pl-keyword"), light.color("pl-keyword"));
        assert_eq!(t.fonts("font-code"), light.fonts("font-code"));
        let t = parse("y.css", ":root{color-scheme:dark}").unwrap();
        assert_eq!(t.palette, builtin("github-dark").unwrap().palette);
    }

    #[test]
    fn type_tokens_read() {
        let t = parse("x.css", ":root{color-scheme:light;--mddog-font-body:\"Avenir Next\", 'Inter',system-ui;--mddog-heading-scale:1.6 1.4 1.2 1 1 .9;--mddog-heading-weight:800 700 650 600 500 400;--mddog-line-height:1.6;--mddog-code-size:0.9}")
            .unwrap();
        assert_eq!(t.fonts("font-body"), ["Avenir Next", "Inter", "system-ui"]);
        assert_eq!(t.heading_scale(), [1.6, 1.4, 1.2, 1.0, 1.0, 0.9]);
        assert_eq!(t.heading_weight(), [800, 700, 650, 600, 500, 400]);
        assert_eq!((t.number("line-height"), t.number("code-size")), (1.6, 0.9));
        let t = parse("x.css", ":root{color-scheme:dark;--mddog-layout:obsidian;--mddog-code-line-height:1.7}").unwrap();
        assert_eq!((t.choice("layout"), t.number("code-line-height")), ("obsidian", 1.7));
        // A theme without the token keeps GitHub's layout.
        assert_eq!(parse("y.css", ":root{color-scheme:dark}").unwrap().choice("layout"), "github");
        assert_eq!(parse("z.css", ":root{--mddog-base:obsidian-light}").unwrap().choice("layout"), "obsidian");
    }

    #[test]
    fn bad_themes_say_why() {
        let e = |css: &str| parse("x.css", css).unwrap_err();
        assert_eq!(e(":root{--mddog-bg:#fff}"), "x.css: set color-scheme: light or dark on :root, or a base theme with --mddog-base");
        assert_eq!(e(":root{color-scheme:light dark}"), "x.css: color-scheme must be light or dark, but got \"light dark\"");
        assert_eq!(e(":root{color-scheme:dark;--mddog-fg:var(--x)}"), "x.css: --mddog-fg: expected a hex color such as #0d1117, but got \"var(--x)\"");
        assert!(e(":root{color-scheme:dark;--mddog-fg:#ffffff80}").starts_with("x.css: --mddog-fg: expected a hex color"));
        assert_eq!(e(":root{color-scheme:dark;--mddog-tint:#fff}"), "x.css: --mddog-tint: unknown token. A theme sets color-scheme and the --mddog-* tokens that docs/themes.md lists");
        assert!(e(":root{color-scheme:dark;--fgColor-default:#fff}").starts_with("x.css: --fgColor-default: unknown token"));
        assert_eq!(e(":root{color-scheme:dark;--mddog-layout:notion}"), "x.css: --mddog-layout: expected github or obsidian, but got \"notion\"");
        assert!(e(":root{color-scheme:dark;--mddog-code-line-height:0}").starts_with("x.css: --mddog-code-line-height: expected a positive number"));
        assert!(e(":root{color-scheme:dark;--mddog-heading-scale:2 1.5}").starts_with("x.css: --mddog-heading-scale: expected six sizes"));
        assert_eq!(
            e(":root{color-scheme:dark;--mddog-heading-weight:600 600 600}"),
            "x.css: --mddog-heading-weight: expected six font weights from 100 to 900, one per heading level, such as 700 680 660 640 620 600, but got \"600 600 600\""
        );
        for bad in ["950 600 600 600 600 600", "bold 600 600 600 600 600", "600.5 600 600 600 600 600", "50 600 600 600 600 600"] {
            assert!(e(&format!(":root{{color-scheme:dark;--mddog-heading-weight:{bad}}}")).starts_with("x.css: --mddog-heading-weight: expected six font weights"), "{bad}");
        }
        assert!(e(":root{color-scheme:dark;--mddog-line-height:-1}").starts_with("x.css: --mddog-line-height: expected a positive number"));
        assert!(e(":root{color-scheme:dark;--mddog-font-body:a,,b}").starts_with("x.css: --mddog-font-body: expected a list of font names"));
        assert!(e(":root{color-scheme:dark;--mddog-font-body:\"a</style>\"}").starts_with("x.css: --mddog-font-body: expected a list of font names"));
        // Rules and at-rules belonged to the old CSS themes.
        assert_eq!(e(":root{color-scheme:dark}\n.markdown-body h1 { color: red }"), "x.css: a theme holds only `:root { ... }` tokens, but it has \".markdown-body h1\"");
        assert_eq!(e("@import url(a.css);:root{color-scheme:dark}"), "x.css: a theme holds only `:root { ... }` tokens, but it has \"@import url(a.css);:root\"");
        assert_eq!(e(":root{color-scheme:dark;& a{--mddog-bg:#fff}}"), "x.css: a theme holds only `:root { ... }` tokens, but its :root has a nested rule");
        assert_eq!(e(":root{color-scheme:dark} junk"), "x.css: a theme holds only `:root { ... }` tokens, but it has \"junk\"");
        assert_eq!(e(":root{color-scheme:dark; nonsense }"), "x.css: expected a token such as --mddog-bg: #0d1117, but got \"nonsense\"");
    }

    #[test]
    fn quotes_hide_braces_and_comments() {
        let bg = |css: &str| parse("x.css", css).map(|t| (t.dark, t.palette.bg));
        assert_eq!(bg(":root{ color-scheme:dark; /* } { ; */ --mddog-bg:#ff0000; }"), Ok((true, (255, 0, 0))));
        // A brace in quotes is the value's, so the error is about the value.
        assert!(bg(":root{ color-scheme:dark; --mddog-font-body:\"}{;\"; }").unwrap_err().starts_with("x.css: --mddog-font-body: expected"));
        assert_eq!(bg("/* :root{color-scheme:light} */:root{color-scheme:dark;--mddog-bg:#ff0000}/* } */"), Ok((true, (255, 0, 0))));
        // The last declaration wins, as in the cascade.
        assert_eq!(bg(":root{color-scheme:dark;--mddog-bg:#fff}:root{--mddog-bg:#ff0000}"), Ok((true, (255, 0, 0))));
    }

    #[test]
    fn a_base_fills_in_the_rest() {
        let obsidian = builtin("obsidian-dark").unwrap();
        let t = parse("x.css", ":root { --mddog-base: obsidian-dark; --mddog-bg: #202a44; }").unwrap();
        assert!(t.dark);
        assert_eq!(t.palette, Palette { bg: (32, 42, 68), ..obsidian.palette });
        assert_eq!(t.heading_scale(), obsidian.heading_scale());
        assert_eq!(t.color("link"), obsidian.color("link"));
        let e = parse("x.css", ":root{--mddog-base:obsidian-dark;color-scheme:light}").unwrap_err();
        assert_eq!(e, "x.css: color-scheme is light, but its base obsidian-dark is dark");
        let e = parse("x.css", ":root{--mddog-base:nope}").unwrap_err();
        assert!(e.starts_with("x.css: --mddog-base: unknown theme \"nope\". Built-in themes: github-light"), "{e}");
    }

    #[test]
    fn base_files_chain_and_loops_fail() {
        let dir = std::env::temp_dir().join(format!("mddog-base-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.css"), ":root{--mddog-base:sub/b.css;--mddog-fg:#010203}").unwrap();
        std::fs::write(dir.join("sub/b.css"), ":root{--mddog-base:obsidian-light;--mddog-bg:#fefefe}").unwrap();
        let a = file(&dir.join("a.css")).unwrap();
        assert!(!a.dark);
        assert_eq!((a.palette.bg, a.palette.fg, a.palette.border), ((254, 254, 254), (1, 2, 3), builtin("obsidian-light").unwrap().palette.border));
        std::fs::write(dir.join("sub/b.css"), ":root{--mddog-base:../a.css}").unwrap();
        let e = file(&dir.join("a.css")).unwrap_err();
        let (a, b) = (dir.join("a.css").display().to_string(), dir.join("sub/b.css").display().to_string());
        assert_eq!(e, format!("{a}: --mddog-base: {b}: --mddog-base: the chain of base themes loops back to {}", dir.join("a.css").canonicalize().unwrap().display()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn obsidian_modes_set_the_same_tokens() {
        let names = |name: &str| {
            let css = BUILTIN.iter().find(|(n, _)| *n == name).unwrap().1;
            root_decls(css).unwrap().into_iter().map(|(p, _)| p).collect::<Vec<_>>()
        };
        assert_eq!(names("obsidian-light"), names("obsidian-dark"));
        let (light, dark) = (builtin("obsidian-light").unwrap(), builtin("obsidian-dark").unwrap());
        assert!(!light.dark && dark.dark);
        assert_eq!(light.heading_scale(), dark.heading_scale());
        assert_eq!(dark.heading_weight(), [700, 680, 660, 640, 620, 600]);
        assert_eq!(light.heading_weight(), dark.heading_weight());
        assert_eq!(builtin("github-dark").unwrap().heading_weight(), [600; 6]);
        assert_eq!(builtin("github-light").unwrap().heading_weight(), [600; 6]);
    }

    #[test]
    fn rgb_reads_hex() {
        assert_eq!(rgb("#0d1117"), Some((13, 17, 23)));
        assert_eq!(rgb("#fff8"), Some((255, 255, 255)));
        assert_eq!(rgb("#+1+1+1"), None);
        assert_eq!(rgb("var(--bgColor-default)"), None);
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
    fn builtin_text_is_readable() {
        for (name, _) in BUILTIN {
            let t = builtin(name).unwrap();
            let p = t.palette;
            let bg = p.bg;
            for (what, fg, ground, min) in [
                ("fg on bg", p.fg, bg, 4.5),
                ("fg-bright on bg", p.fg_bright, bg, 4.5),
                ("fg-bright on selection", p.fg_bright, p.selection_bg, 4.5),
                ("fg-muted on bg", p.fg_muted, bg, 4.5),
                ("fg-muted on bar", p.fg_muted, p.bar_bg, 4.5),
                ("fg-bright on bar", p.fg_bright, p.bar_bg, 4.5),
                ("text on bg", t.color("text"), bg, 4.5),
                ("text on pre-bg", t.color("text"), t.color("pre-bg"), 4.5),
                ("link on bg", t.color("link"), bg, 3.0),
            ] {
                let c = contrast(fg, ground);
                assert!(c >= min, "{name}: {what} is {c:.2}:1");
            }
        }
    }
}
