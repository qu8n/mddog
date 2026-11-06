use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::theme::{self, Theme};

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub theme: ThemeSpec,
    pub live_reload: bool,
    /// d/u, f/b, Space, g/G and other jump keys glide instead of jumping.
    pub smooth_scroll: bool,
    /// How long a glide over one screen takes. A shorter glide takes its share.
    pub glide_ms_per_page: u32,
    /// Releasing a drag or double-click selection copies it.
    pub copy_on_select: bool,
    /// Unset, mddog follows the macOS font smoothing setting.
    pub font_smoothing: Option<bool>,
    pub file_list: FileListConfig,
    /// The `theme` key's themes, read.
    #[serde(skip)]
    pub themes: Themes,
}

impl Default for Config {
    fn default() -> Config {
        let theme = ThemeSpec::default();
        let themes = theme.load(Path::new("")).expect("built-in themes");
        Config { theme, live_reload: true, smooth_scroll: true, glide_ms_per_page: 200, copy_on_select: false, font_smoothing: None, file_list: FileListConfig::default(), themes }
    }
}

/// The `theme` key: one theme, or one for each macOS appearance.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(try_from = "String")]
pub enum ThemeSpec {
    One(Source),
    Pair { light: Source, dark: Source },
}

/// A built-in theme's name, or a path to a CSS file.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Builtin(&'static Theme),
    File(String),
}

impl Default for ThemeSpec {
    fn default() -> ThemeSpec {
        ThemeSpec::try_from("light:github-light,dark:github-dark".to_string()).expect("default theme")
    }
}

impl TryFrom<String> for ThemeSpec {
    type Error = String;
    fn try_from(s: String) -> Result<ThemeSpec, String> {
        let source = |t: &str| {
            let t = t.trim();
            if theme::is_file(t) {
                Ok(Source::File(t.to_string()))
            } else {
                theme::builtin(t).map(Source::Builtin).ok_or_else(|| format!("unknown theme {t:?}. Built-in themes: {}. A theme file's path ends in .css", theme::names()))
            }
        };
        let pair = s.split(',').map(|p| p.trim().split_once(':')).collect::<Option<Vec<_>>>();
        match pair.as_deref() {
            Some([("light", l), ("dark", d)] | [("dark", d), ("light", l)]) => Ok(ThemeSpec::Pair { light: source(l)?, dark: source(d)? }),
            _ if s.contains(',') || s.starts_with("light:") || s.starts_with("dark:") => Err(format!("expected one theme, or \"light:THEME,dark:THEME\", but got {s:?}")),
            _ => Ok(ThemeSpec::One(source(&s)?)),
        }
    }
}

impl ThemeSpec {
    /// Reads the theme files. A relative path is relative to `dir`, the config folder.
    fn load(&self, dir: &Path) -> Result<Themes, String> {
        let read = |s: &Source| -> Result<Theme, String> {
            match s {
                Source::Builtin(t) => Ok((*t).clone()),
                Source::File(f) => theme::file(&theme::resolve(dir, f)),
            }
        };
        Ok(match self {
            ThemeSpec::One(s) => Themes { light: read(s)?, dark: None },
            ThemeSpec::Pair { light, dark } => Themes { light: read(light)?, dark: Some(read(dark)?) },
        })
    }
}

/// The themes in use: `dark` is set when the theme follows the macOS appearance.
#[derive(Debug, PartialEq)]
pub struct Themes {
    pub light: Theme,
    pub dark: Option<Theme>,
}

impl Default for Themes {
    fn default() -> Themes {
        Config::default().themes
    }
}

#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct FileListConfig {
    /// The extensions the file list shows, lowercase and without the dot.
    #[serde(deserialize_with = "extensions")]
    pub extensions: Vec<String>,
}

impl Default for FileListConfig {
    fn default() -> Self {
        FileListConfig { extensions: vec!["md".into(), "markdown".into()] }
    }
}

impl FileListConfig {
    pub fn shows(&self, p: &Path) -> bool {
        p.extension().and_then(|e| e.to_str()).is_some_and(|e| self.extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
    }

    /// The extensions for a sentence: ".md or .markdown".
    pub fn names(&self) -> String {
        let e: Vec<String> = self.extensions.iter().map(|x| format!(".{x}")).collect();
        match &e[..] {
            [] => String::new(),
            [one] => one.clone(),
            [rest @ .., last] if rest.len() == 1 => format!("{} or {last}", rest[0]),
            [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
        }
    }
}

fn extensions<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    use serde::de::Error;
    let list = Vec::<String>::deserialize(d)?;
    if list.is_empty() {
        return Err(D::Error::custom("needs at least one extension, such as \"md\""));
    }
    for e in &list {
        if e.is_empty() || e.contains('.') {
            return Err(D::Error::custom(format!("{e:?} is not an extension. Write each one without the dot, such as \"md\"")));
        }
    }
    Ok(list.iter().map(|e| e.to_ascii_lowercase()).collect())
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

pub fn dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    xdg.or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).map(|d| d.join("mddog"))
}

pub fn load() -> Result<(), String> {
    let config = match dir() {
        Some(d) => read(&d)?,
        None => Config::default(),
    };
    let _ = CONFIG.set(config);
    Ok(())
}

pub fn read(dir: &Path) -> Result<Config, String> {
    let path = dir.join("config.toml");
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text, dir).map_err(|e| format!("{}: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(format!("{}: {}", path.display(), crate::term::io_error(&e))),
    };
    Ok(config)
}

/// Parses a config file and reads its theme files. `dir` is the folder relative theme paths start from.
pub fn parse(text: &str, dir: &Path) -> Result<Config, String> {
    let mut config: Config = toml::from_str(text).map_err(|e| {
        let msg = e.message().trim().replace('\n', " ");
        let Some(span) = e.span() else { return msg };
        let line = text[..span.start].matches('\n').count() + 1;
        match key_at(text, span.start) {
            Some(key) => format!("line {line}: {key}: {msg}"),
            None => format!("line {line}: {msg}"),
        }
    })?;
    config.themes = config.theme.load(dir).map_err(|e| {
        let at = text.lines().take_while(|l| !l.trim_start().starts_with('[')).position(|l| l.split('=').next().is_some_and(|k| k.trim() == "theme"));
        at.map_or_else(|| format!("theme: {e}"), |i| format!("line {}: theme: {e}", i + 1))
    })?;
    Ok(config)
}

fn key_at(text: &str, at: usize) -> Option<String> {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = text[start..].lines().next()?.trim();
    let table = text[..start].lines().rev().find_map(|l| l.trim().strip_prefix('[')?.split(']').next()).map(str::trim);
    if let Some(name) = line.strip_prefix('[') {
        return name.split(']').next().map(|n| n.trim().to_string());
    }
    let key = line.split('=').next()?.trim();
    if key.is_empty() {
        return None;
    }
    Some(table.map_or_else(|| key.to_string(), |t| format!("{t}.{key}")))
}

/// The theme to draw with now: the one theme, or the one for the macOS appearance.
pub fn theme() -> &'static Theme {
    let t = &get().themes;
    match &t.dark {
        Some(dark) if system_dark() => dark,
        _ => &t.light,
    }
}

/// Whether text is drawn with font smoothing: the config's choice, else the macOS setting at startup.
pub fn font_smoothing() -> bool {
    static SYSTEM: OnceLock<bool> = OnceLock::new();
    get().font_smoothing.unwrap_or_else(|| *SYSTEM.get_or_init(system_smoothing))
}

/// macOS's `AppleFontSmoothing`: 0 turns smoothing off, and unset or 1 to 3 leave it on. CoreGraphics
/// bitmap contexts ignore it, so mddog reads it itself.
fn system_smoothing() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    let d = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("AppleFontSmoothing");
    d.objectForKey(&key).is_none() || d.integerForKey(&key) != 0
}

fn system_dark() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    let style = NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str("AppleInterfaceStyle"));
    style.is_some_and(|s| s.to_string().eq_ignore_ascii_case("dark"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, String> {
        super::parse(text, Path::new("/nonexistent"))
    }

    #[test]
    fn empty_is_default() {
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn example_is_default() {
        assert_eq!(parse(include_str!("../config.example.toml")).unwrap(), Config::default());
    }

    #[test]
    fn full() {
        let c = parse("theme = \"github-dark\"\nlive_reload = false\nsmooth_scroll = false\nglide_ms_per_page = 100\ncopy_on_select = true\n").unwrap();
        assert_eq!(c.theme, ThemeSpec::One(Source::Builtin(theme::builtin("github-dark").unwrap())));
        assert_eq!((c.themes.light.name.as_str(), c.themes.dark), ("github-dark", None));
        assert!(!c.live_reload);
        assert!(!c.smooth_scroll);
        assert!(c.copy_on_select);
        assert_eq!(c.glide_ms_per_page, 100);
        assert_eq!(parse("font_smoothing = true\n").unwrap().font_smoothing, Some(true));
        assert_eq!(parse("font_smoothing = false\n").unwrap().font_smoothing, Some(false));
    }

    #[test]
    fn file_list_extensions() {
        let f = parse("[file_list]\nextensions = [\"MD\", \"txt\"]\n").unwrap().file_list;
        assert_eq!(f.extensions, ["md", "txt"]);
        assert!(f.shows(Path::new("a/b.Txt")) && f.shows(Path::new("b.md")));
        assert!(!f.shows(Path::new("b.markdown")) && !f.shows(Path::new("txt")) && !f.shows(Path::new("a.txt.bak")));
        assert_eq!(f.names(), ".md or .txt");
        assert_eq!(FileListConfig { extensions: vec!["md".into()] }.names(), ".md");
        assert_eq!(FileListConfig { extensions: vec!["md".into(), "txt".into(), "rst".into()] }.names(), ".md, .txt, or .rst");
    }

    #[test]
    fn theme_pairs_and_files() {
        let dir = std::env::temp_dir().join(format!("mddog-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.css"), ":root{color-scheme:dark;--mddog-bg:#101010}").unwrap();
        let c = super::parse("theme = \"dark:mine.css, light:github-dark\"\n", &dir).unwrap();
        let dark = c.themes.dark.unwrap();
        assert_eq!(dark.name, dir.join("mine.css").display().to_string());
        assert_eq!(dark.palette.bg, (16, 16, 16));
        assert_eq!(dark.palette.fg, theme::builtin("github-dark").unwrap().palette.fg);
        assert!(c.themes.light.dark);
        let abs = dir.join("mine.css").display().to_string();
        assert!(parse(&format!("theme = {abs:?}\n")).unwrap().themes.light.dark);
        // A bad token stops mddog with the config line, the file and the token.
        std::fs::write(dir.join("bad.css"), ":root{color-scheme:dark;--mddog-tint:#101010}").unwrap();
        let e = super::parse("live_reload = true\ntheme = \"bad.css\"\n", &dir).unwrap_err();
        assert_eq!(e, format!("line 2: theme: {}: --mddog-tint: unknown token. A theme sets color-scheme and the --mddog-* tokens that docs/themes.md lists", dir.join("bad.css").display()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    /// The line and the dotted key lead every error. What follows from serde and toml is theirs to word.
    fn errors_name_the_key() {
        let one_line = |t: &str| {
            let e = parse(t).unwrap_err();
            assert!(!e.contains('\n'), "{e}");
            e
        };
        let e = one_line("[background]\ndark = \"terminal\"\n");
        assert!(e.starts_with("line 1: background: "), "{e}");
        let e = one_line("live_reload = \"yes\"\n");
        assert!(e.starts_with("line 1: live_reload: "), "{e}");
        let e = one_line("font_smoothing = 2\n");
        assert!(e.starts_with("line 1: font_smoothing: "), "{e}");
        // Removed keys get the unknown-key error like any other.
        let e = one_line("prewarm = false\n");
        assert!(e.starts_with("line 1: prewarm: "), "{e}");
        let e = one_line("[mermaid]\nengine = \"pele\"\n");
        assert!(e.starts_with("line 1: mermaid: "), "{e}");
        let e = one_line("[file_list]\nextensions = [\".md\"]\n");
        assert!(e.starts_with("line 2: file_list.extensions: \".md\" is not an extension"), "{e}");
        let e = one_line("[file_list]\nextensions = [\"md\", \"\"]\n");
        assert!(e.starts_with("line 2: file_list.extensions: \"\" is not an extension"), "{e}");
        let e = one_line("[file_list]\nextensions = []\n");
        assert!(e.starts_with("line 2: file_list.extensions: needs at least one"), "{e}");
        let e = one_line("[colours]\n");
        assert!(e.starts_with("line 1: colours: "), "{e}");
        let e = one_line("theme = \"dark\n");
        assert!(e.starts_with("line 1: theme: "), "{e}");
        let e = one_line("theme = \"dark\"\n");
        assert!(e.starts_with("line 1: theme: unknown theme \"dark\". Built-in themes: github-light, github-dark"), "{e}");
        let e = one_line("theme = \"light:github-light\"\n");
        assert!(e.starts_with("line 1: theme: expected one theme, or \"light:THEME,dark:THEME\""), "{e}");
        let e = one_line("live_reload = true\ntheme = \"light:github-light,dark:missing.css\"\n");
        assert!(e.starts_with("line 2: theme: /nonexistent/missing.css: "), "{e}");
    }

    #[test]
    fn missing_file_is_default() {
        let dir = std::env::temp_dir().join(format!("mddog-config-{}", std::process::id()));
        assert_eq!(read(&dir).unwrap(), Config::default());
    }
}
