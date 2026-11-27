use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use serde::Deserialize;

pub use crate::render::doc::Callouts;
use crate::theme::{self, Theme};

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub theme: ThemeSpec,
    /// Releasing a drag or double-click selection copies it.
    pub copy_on_select: bool,
    /// How `> [!kind]` blocks read.
    pub callouts: Callouts,
    pub max_width: MaxWidth,
    #[serde(deserialize_with = "open_command")]
    pub open_command: Vec<String>,
    pub file_list: FileList,
    /// The `theme` key's themes, read.
    #[serde(skip)]
    pub themes: Themes,
}

impl Default for Config {
    fn default() -> Config {
        let theme = ThemeSpec::default();
        let themes = theme.load(Path::new("")).expect("built-in themes");
        Config { theme, copy_on_select: false, callouts: Callouts::Github, max_width: MaxWidth(100), file_list: FileList::default(), open_command: default_open_command(), themes }
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct FileList {
    pub preview: ListPreview,
    #[serde(deserialize_with = "extensions")]
    pub extensions: Vec<String>,
}

impl Default for FileList {
    fn default() -> FileList {
        FileList { preview: ListPreview::default(), extensions: vec!["md".into(), "markdown".into()] }
    }
}

impl FileList {
    pub fn shows(&self, p: &Path) -> bool {
        p.extension().and_then(|e| e.to_str()).is_some_and(|e| self.extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
    }

    pub fn names(&self) -> String {
        let e: Vec<String> = self.extensions.iter().map(|x| format!(".{x}")).collect();
        match &e[..] {
            [] => String::new(),
            [one] => one.clone(),
            [a, b] => format!("{a} or {b}"),
            [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
        }
    }
}

pub fn default_open_command() -> Vec<String> {
    vec![if cfg!(target_os = "macos") { "open" } else { "xdg-open" }.into()]
}

fn open_command<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    use serde::de::Error;
    let cmd = Vec::<String>::deserialize(d)?;
    if cmd.first().is_none_or(String::is_empty) {
        return Err(D::Error::custom("needs a program to run, such as [\"open\"]"));
    }
    Ok(cmd)
}

fn extensions<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    use serde::de::Error;
    let list = Vec::<String>::deserialize(d)?;
    if list.is_empty() {
        return Err(D::Error::custom("needs at least one extension, such as \"md\""));
    }
    if let Some(e) = list.iter().find(|e| e.is_empty() || e.contains('.')) {
        return Err(D::Error::custom(format!("{e:?} is not an extension. Write each one without the dot, such as \"md\"")));
    }
    Ok(list.iter().map(|e| e.to_ascii_lowercase()).collect())
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct ListPreview {
    pub position: Position,
    pub size: Size,
    /// A window narrower than this many columns uses the `narrow_` keys.
    pub narrow_columns: u16,
    pub narrow_position: NarrowPosition,
    /// Unset means `size`.
    pub narrow_size: Option<Size>,
    pub show_by_default: bool,
}

impl Default for ListPreview {
    fn default() -> ListPreview {
        ListPreview { position: Position::Right, size: Size::Percent(50), narrow_columns: 120, narrow_position: NarrowPosition::Bottom, narrow_size: None, show_by_default: true }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Right,
    Left,
    Top,
    Bottom,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NarrowPosition {
    Right,
    Left,
    Top,
    Bottom,
    Hidden,
}

impl NarrowPosition {
    pub fn side(self) -> Option<Position> {
        match self {
            NarrowPosition::Right => Some(Position::Right),
            NarrowPosition::Left => Some(Position::Left),
            NarrowPosition::Top => Some(Position::Top),
            NarrowPosition::Bottom => Some(Position::Bottom),
            NarrowPosition::Hidden => None,
        }
    }
}

/// A number of cells, or a percentage of the window's width beside the list, or of the rows under the filter above
/// or below it.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(try_from = "RawSize")]
pub enum Size {
    Cells(u16),
    Percent(u16),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawSize {
    Cells(i64),
    Text(String),
}

impl TryFrom<RawSize> for Size {
    type Error = String;
    fn try_from(raw: RawSize) -> Result<Size, String> {
        let bad = || "expected a number of cells, such as 40, or a percentage, such as \"50%\"".to_string();
        match raw {
            RawSize::Cells(n) => u16::try_from(n).ok().filter(|&n| n > 0).map(Size::Cells).ok_or_else(bad),
            RawSize::Text(t) => t.trim().strip_suffix('%').and_then(|p| p.trim().parse::<u16>().ok()).filter(|p| (1..=99).contains(p)).map(Size::Percent).ok_or_else(bad),
        }
    }
}

impl Size {
    pub fn of(self, room: u16) -> u16 {
        match self {
            Size::Cells(n) => n,
            Size::Percent(p) => (u32::from(room) * u32::from(p) / 100) as u16,
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(try_from = "i64")]
pub struct MaxWidth(pub u16);

impl MaxWidth {
    pub fn px(self, cell_w: u32) -> u32 {
        u32::from(self.0) * cell_w
    }
}

impl TryFrom<i64> for MaxWidth {
    type Error = String;
    fn try_from(n: i64) -> Result<MaxWidth, String> {
        u16::try_from(n).ok().filter(|&n| n >= 20).map(MaxWidth).ok_or_else(|| "expected a number of cells, 20 or more, such as 100".to_string())
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
        match slots(&s) {
            Some((l, d)) => Ok(ThemeSpec::Pair { light: source(l)?, dark: source(d)? }),
            None if s.contains(',') || s.starts_with("light:") || s.starts_with("dark:") => Err(format!("expected one theme, or \"light:THEME,dark:THEME\", but got {s:?}")),
            None => Ok(ThemeSpec::One(source(&s)?)),
        }
    }
}

fn slots(s: &str) -> Option<(&str, &str)> {
    let pair = s.split(',').map(|p| p.trim().split_once(':')).collect::<Option<Vec<_>>>()?;
    match pair.as_slice() {
        [("light", l), ("dark", d)] | [("dark", d), ("light", l)] => Some((l.trim(), d.trim())),
        _ => None,
    }
}

impl ThemeSpec {
    fn sources(&self) -> Vec<&Source> {
        match self {
            ThemeSpec::One(s) => vec![s],
            ThemeSpec::Pair { light, dark } => vec![light, dark],
        }
    }

    /// Reads the theme files. A relative path is relative to `dir`, the config folder.
    fn load(&self, dir: &Path) -> Result<Themes, String> {
        let read = |s: &Source| -> Result<&'static Theme, String> {
            match s {
                Source::Builtin(t) => Ok(t),
                Source::File(f) => theme_file(&theme::resolve(dir, f)),
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
    pub light: &'static Theme,
    pub dark: Option<&'static Theme>,
}

fn theme_file(path: &Path) -> Result<&'static Theme, String> {
    static FILES: Mutex<Vec<(PathBuf, &'static Theme)>> = Mutex::new(Vec::new());
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some((_, t)) = FILES.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(k, _)| *k == key) {
        return Ok(t);
    }
    let t: &'static Theme = Box::leak(Box::new(theme::file(path)?));
    FILES.lock().unwrap_or_else(PoisonError::into_inner).push((key, t));
    Ok(t)
}

impl Default for Themes {
    fn default() -> Themes {
        Config::default().themes
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();
static DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

pub fn dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    xdg.or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))).map(|d| d.join("mddog"))
}

pub fn load() -> Result<(), String> {
    let config = match dir() {
        Some(d) => {
            let config = read(&d)?;
            let _ = DIR.set(d);
            config
        }
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
        Err(e) => return Err(format!("{}: {}", path.display(), crate::error::io(&e))),
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

#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    Follow { value: String, light: &'static Theme, dark: &'static Theme },
    Theme { value: String, theme: &'static Theme },
}

impl Choice {
    pub fn label(&self) -> &str {
        match self {
            Choice::Follow { .. } => FOLLOW_LABEL,
            Choice::Theme { value, .. } => value,
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Choice::Follow { value, .. } | Choice::Theme { value, .. } => value,
        }
    }

    pub fn theme(&self, dark: bool) -> &'static Theme {
        match self {
            Choice::Follow { dark: t, .. } if dark => t,
            Choice::Follow { light, .. } => light,
            Choice::Theme { theme, .. } => theme,
        }
    }
}

pub fn choices() -> Vec<Choice> {
    let mut out = Vec::new();
    if let (ThemeSpec::Pair { light, dark }, Some(d)) = (&get().theme, get().themes.dark) {
        let name = |s: &Source| match s {
            Source::Builtin(t) => t.name.clone(),
            Source::File(f) => f.clone(),
        };
        out.push(Choice::Follow { value: format!("light:{},dark:{}", name(light), name(dark)), light: get().themes.light, dark: d });
    }
    out.extend(theme::builtin_names().filter_map(|n| Some(Choice::Theme { value: n.to_string(), theme: theme::builtin(n)? })));
    let dir = DIR.get().map_or(Path::new(""), PathBuf::as_path);
    let mut values: Vec<String> = get()
        .theme
        .sources()
        .into_iter()
        .filter_map(|s| match s {
            Source::File(f) => Some(f.clone()),
            Source::Builtin(_) => None,
        })
        .collect();
    if DIR.get().is_some() {
        values.extend(css_in(dir, ""));
        values.extend(css_in(&dir.join("themes"), "themes/"));
    }
    let mut seen = Vec::new();
    for v in values {
        let path = theme::resolve(dir, &v);
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if let Ok(theme) = theme_file(&path) {
            out.push(Choice::Theme { value: v, theme });
        }
    }
    out
}

fn css_in(dir: &Path, prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".css") && n.trim() == n)
        .map(|n| format!("{prefix}{n}"))
        .filter(|v| matches!(ThemeSpec::try_from(v.clone()), Ok(ThemeSpec::One(_))))
        .collect();
    names.sort();
    names.truncate(100);
    names
}

pub fn themes_of(choice: &Choice) -> Result<Themes, String> {
    let spec = ThemeSpec::try_from(choice.value().to_string()).map_err(|e| format!("Can't save the theme: {e}"))?;
    spec.load(DIR.get().map_or(Path::new(""), PathBuf::as_path)).map_err(|e| format!("Can't save the theme: {e}"))
}

pub fn save(choice: &Choice) -> Result<String, String> {
    let value = choice.value();
    let spec = ThemeSpec::try_from(value.to_string()).map_err(|e| format!("Can't save the theme: {e}"))?;
    let dir = DIR.get().ok_or("Can't save the theme: HOME is not set")?;
    for s in spec.sources() {
        if let Source::File(f) = s {
            theme::file(&theme::resolve(dir, f)).map_err(|e| format!("Can't save the theme: {e}"))?;
        }
    }
    let path = dir.join("config.toml");
    let _lock = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::File::open(dir))
        .and_then(|f| f.lock().map(|()| f))
        .map_err(|e| format!("Can't save the theme in {}: {}", tilde(&path), crate::error::io(&e)))?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Can't read {}: {}", tilde(&path), crate::error::io(&e))),
    };
    let new = set_theme(&text, value).map_err(|e| format!("Can't save the theme in {}: {e}", tilde(&path)))?;
    if new != text {
        replace(&path, &text, &new).map_err(|e| format!("Can't write {}: {}", tilde(&path), crate::error::io(&e)))?;
    }
    Ok(match choice {
        Choice::Follow { .. } => format!("Saved: follow the {APPEARANCE} appearance, in {}", tilde(&path)),
        Choice::Theme { .. } => format!("Saved {value} as the theme in {}", tilde(&path)),
    })
}

fn follow(path: &Path) -> PathBuf {
    let mut target = path.to_path_buf();
    for _ in 0..40 {
        let Ok(link) = std::fs::read_link(&target) else { break };
        target = target.parent().map_or_else(|| link.clone(), |p| p.join(&link));
    }
    target
}

fn replace(path: &Path, old: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let target = follow(path);
    let name = target.file_name().map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
    let tmp = target.with_file_name(format!(".{name}.{}", crate::tmp::nonce()));
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    let mut write = || {
        if std::fs::metadata(&target).is_ok() {
            std::fs::copy(&target, &tmp)?;
            f.set_len(0)?;
        }
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        let now = match std::fs::read_to_string(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            r => r?,
        };
        if now != old || follow(path) != target {
            return Err(std::io::Error::other("it changed while saving. Try again"));
        }
        std::fs::rename(&tmp, &target)
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn set_theme(text: &str, value: &str) -> Result<String, String> {
    let table = toml::de::DeTable::parse(text).map_err(|e| e.message().trim().to_string())?;
    let found = table.get_ref().iter().find(|(k, _)| k.get_ref() == "theme").map(|(_, v)| v);
    let Some(v) = found else {
        let (bom, rest) = text.strip_prefix('\u{feff}').map_or(("", text), |r| ("\u{feff}", r));
        return Ok(format!("{bom}theme = {}\n{rest}", quote(value)));
    };
    match v.get_ref() {
        toml::de::DeValue::String(cur) if cur == value || ThemeSpec::try_from(cur.to_string()).is_ok_and(|c| ThemeSpec::try_from(value.to_string()).is_ok_and(|v| c == v)) => {
            return Ok(text.to_string());
        }
        toml::de::DeValue::String(_) => {}
        _ => return Err("theme is not a string".into()),
    }
    let span = v.span();
    Ok(format!("{}{}{}", &text[..span.start], quote(value), &text[span.end..]))
}

fn quote(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Whether text is drawn with font smoothing, from macOS's `AppleFontSmoothing` at startup: 0 turns it off, and
/// unset or 1 to 3 leave it on. CoreGraphics bitmap contexts ignore the setting, so mddog reads it itself.
pub fn font_smoothing() -> bool {
    static SYSTEM: OnceLock<bool> = OnceLock::new();
    *SYSTEM.get_or_init(system_smoothing)
}

#[cfg(target_os = "macos")]
const FOLLOW_LABEL: &str = "Follow macOS";
#[cfg(target_os = "macos")]
const APPEARANCE: &str = "macOS";
#[cfg(not(target_os = "macos"))]
const FOLLOW_LABEL: &str = "Follow terminal";
#[cfg(not(target_os = "macos"))]
const APPEARANCE: &str = "terminal's";

#[cfg(not(target_os = "macos"))]
fn system_smoothing() -> bool {
    true
}

#[cfg(target_os = "macos")]
fn system_smoothing() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    let d = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("AppleFontSmoothing");
    d.objectForKey(&key).is_none() || d.integerForKey(&key) != 0
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
        let c = parse("theme = \"github-dark\"\ncopy_on_select = true\ncallouts = \"obsidian\"\n").unwrap();
        assert_eq!(c.theme, ThemeSpec::One(Source::Builtin(theme::builtin("github-dark").unwrap())));
        assert_eq!((c.themes.light.name.as_str(), c.themes.dark), ("github-dark", None));
        assert!(c.copy_on_select);
        assert_eq!(c.callouts, Callouts::Obsidian);
    }

    #[test]
    fn open_command() {
        assert_eq!(parse("").unwrap().open_command, [if cfg!(target_os = "macos") { "open" } else { "xdg-open" }]);
        assert_eq!(parse("open_command = [\"ssh\", \"mac\", \"open\"]\n").unwrap().open_command, ["ssh", "mac", "open"]);
        for bad in ["open_command = []\n", "open_command = [\"\"]\n"] {
            let e = parse(bad).unwrap_err();
            assert!(e.starts_with("line 1: open_command: "), "{e}");
        }
    }

    #[test]
    fn max_width() {
        assert_eq!(parse("").unwrap().max_width, MaxWidth(100));
        assert_eq!(parse("max_width = 80\n").unwrap().max_width, MaxWidth(80));
        for bad in ["0", "19", "-1", "70000", "\"wide\""] {
            let e = parse(&format!("max_width = {bad}\n")).unwrap_err();
            assert!(e.starts_with("line 1: max_width: "), "{bad}: {e}");
        }
    }

    #[test]
    fn file_list_preview() {
        let c = parse("[file_list.preview]\nposition = \"left\"\nsize = 50\nnarrow_position = \"hidden\"\nnarrow_size = \"30%\"\nshow_by_default = false\n").unwrap();
        let p = c.file_list.preview;
        assert_eq!((p.position, p.size, p.narrow_position, p.narrow_size, p.show_by_default), (Position::Left, Size::Cells(50), NarrowPosition::Hidden, Some(Size::Percent(30)), false));
        assert_eq!(p.narrow_columns, 120);
        let e = parse("[file_list.preview]\nposition = \"hidden\"\n").unwrap_err();
        assert!(e.starts_with("line 2: file_list.preview.position: "), "{e}");
        for bad in ["\"150%\"", "\"0%\"", "0", "-3", "\"big\""] {
            let e = parse(&format!("[file_list.preview]\nsize = {bad}\n")).unwrap_err();
            assert!(e.starts_with("line 2: file_list.preview.size: expected a number of cells"), "{bad}: {e}");
        }
        assert!(parse("[file_list.preview]\nside = \"right\"\n").is_err());
    }

    #[test]
    fn file_list_extensions() {
        let d = Config::default().file_list;
        assert_eq!(d.extensions, ["md", "markdown"]);
        assert_eq!(d.names(), ".md or .markdown");
        let f = parse("[file_list]\nextensions = [\"MD\", \"markdown\", \"Qmd\"]\n").unwrap().file_list;
        assert_eq!(f.extensions, ["md", "markdown", "qmd"]);
        assert_eq!(f.preview, ListPreview::default());
        for yes in ["a.qmd", "a/b.QMD", "c.md", "d.Markdown"] {
            assert!(f.shows(Path::new(yes)), "{yes}");
        }
        for no in ["a.rmd", "qmd", "a.qmd.bak", "a.txt"] {
            assert!(!f.shows(Path::new(no)), "{no}");
        }
        assert_eq!(f.names(), ".md, .markdown, or .qmd");
        assert_eq!(parse("[file_list]\nextensions = [\"qmd\"]\n").unwrap().file_list.names(), ".qmd");
        for (bad, why) in [
            ("[]", "needs at least one extension"),
            ("[\".md\"]", "\".md\" is not an extension"),
            ("[\"md\", \"\"]", "\"\" is not an extension"),
            ("[\"a.b\"]", "\"a.b\" is not an extension"),
            ("\"md\"", ""),
        ] {
            let e = parse(&format!("[file_list]\nextensions = {bad}\n")).unwrap_err();
            assert!(e.starts_with(&format!("line 2: file_list.extensions: {why}")), "{bad}: {e}");
            assert!(!e.contains('\n'), "{e}");
        }
    }

    #[test]
    fn theme_pairs_and_files() {
        let dir = std::env::temp_dir().join(format!("mddog-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.css"), ":root{color-scheme:dark;--mddog-bg:#101010}").unwrap();
        let c = super::parse("theme = \"dark:mine.css, light:github-dark\"\n", &dir).unwrap();
        let dark = c.themes.dark.unwrap();
        assert_eq!(dark.name, dir.join("mine.css").display().to_string());
        assert_eq!(dark.palette.bg, 0x101010);
        assert_eq!(dark.palette.fg, theme::builtin("github-dark").unwrap().palette.fg);
        assert!(c.themes.light.dark);
        let abs = dir.join("mine.css").display().to_string();
        assert!(parse(&format!("theme = {abs:?}\n")).unwrap().themes.light.dark);
        // A bad token stops mddog with the config line, the file and the token.
        std::fs::write(dir.join("bad.css"), ":root{color-scheme:dark;--mddog-tint:#101010}").unwrap();
        let e = super::parse("copy_on_select = false\ntheme = \"bad.css\"\n", &dir).unwrap_err();
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
        let e = one_line("copy_on_select = \"yes\"\n");
        assert!(e.starts_with("line 1: copy_on_select: "), "{e}");
        // Removed keys get the unknown-key error like any other.
        let e = one_line("prewarm = false\n");
        assert!(e.starts_with("line 1: prewarm: "), "{e}");
        let e = one_line("[mermaid]\nengine = \"pele\"\n");
        assert!(e.starts_with("line 1: mermaid: "), "{e}");
        let e = one_line("smooth_scroll = false\n");
        assert!(e.starts_with("line 1: smooth_scroll: "), "{e}");
        let e = one_line("[colours]\n");
        assert!(e.starts_with("line 1: colours: "), "{e}");
        let e = one_line("theme = \"dark\n");
        assert!(e.starts_with("line 1: theme: "), "{e}");
        let e = one_line("callouts = \"gitlab\"\n");
        assert!(e.starts_with("line 1: callouts: "), "{e}");
        let e = one_line("theme = \"dark\"\n");
        assert!(e.starts_with("line 1: theme: unknown theme \"dark\". Built-in themes: github-light, github-dark"), "{e}");
        let e = one_line("theme = \"light:github-light\"\n");
        assert!(e.starts_with("line 1: theme: expected one theme, or \"light:THEME,dark:THEME\""), "{e}");
        let e = one_line("copy_on_select = false\ntheme = \"light:github-light,dark:missing.css\"\n");
        assert!(e.starts_with("line 2: theme: /nonexistent/missing.css: "), "{e}");
    }

    #[test]
    fn set_theme_keeps_the_rest() {
        let text = "# my config\ntheme = \"github-dark\" # night\ncopy_on_select = true\n";
        assert_eq!(set_theme(text, "obsidian-light").unwrap(), "# my config\ntheme = \"obsidian-light\" # night\ncopy_on_select = true\n");
        assert_eq!(set_theme("theme = 'light:github-light, dark:mine.css'\n", "themes/c.css").unwrap(), "theme = \"themes/c.css\"\n");
        let t = set_theme("copy_on_select = true\n\n[file_list.preview]\nsize = 50\n", "github-dark").unwrap();
        assert_eq!(t, "theme = \"github-dark\"\ncopy_on_select = true\n\n[file_list.preview]\nsize = 50\n");
        assert_eq!(set_theme("", "github-dark").unwrap(), "theme = \"github-dark\"\n");
        assert_eq!(set_theme("theme = 'github-dark'\n", "github-dark").unwrap(), "theme = 'github-dark'\n");
        let pair = "theme = \"dark:github-dark, light:github-light\"\n";
        assert_eq!(set_theme(pair, "light:github-light,dark:github-dark").unwrap(), pair);
        assert_eq!(set_theme("\u{feff}copy_on_select = true\n", "github-dark").unwrap(), "\u{feff}theme = \"github-dark\"\ncopy_on_select = true\n");
        assert_eq!(set_theme("\u{feff}theme = \"a.css\"\n", "github-dark").unwrap(), "\u{feff}theme = \"github-dark\"\n");
        assert_eq!(set_theme("theme = 3\n", "github-dark").unwrap_err(), "theme is not a string");
        assert!(set_theme("theme = \"x\n", "github-dark").is_err());
        assert_eq!(quote("C:\\a \"b\".css"), "'C:\\a \"b\".css'");
    }

    #[test]
    fn replace_writes_through_a_symlink() {
        let dir = std::env::temp_dir().join(format!("mddog-replace-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::fs::write(dir.join("real/config.toml"), "old").unwrap();
        std::os::unix::fs::symlink(dir.join("real/config.toml"), dir.join("config.toml")).unwrap();
        replace(&dir.join("config.toml"), "old", "new").unwrap();
        assert!(std::fs::symlink_metadata(dir.join("config.toml")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(dir.join("real/config.toml")).unwrap(), "new");
        assert_eq!(std::fs::read_dir(dir.join("real")).unwrap().count(), 1);
        std::os::unix::fs::symlink(dir.join("real/later.toml"), dir.join("dangling.toml")).unwrap();
        replace(&dir.join("dangling.toml"), "", "made").unwrap();
        assert!(std::fs::symlink_metadata(dir.join("dangling.toml")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(dir.join("real/later.toml")).unwrap(), "made");
        assert!(replace(&dir.join("config.toml"), "stale", "lost").is_err());
        std::fs::set_permissions(dir.join("real/config.toml"), std::os::unix::fs::PermissionsExt::from_mode(0o640)).unwrap();
        #[cfg(target_os = "macos")]
        {
            let path = std::ffi::CString::new(dir.join("real/config.toml").as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: `path` and `c"mddog.test"` are NUL-terminated C strings, and `b"kept"` is valid for 4 bytes.
            assert_eq!(unsafe { libc::setxattr(path.as_ptr(), c"mddog.test".as_ptr(), b"kept".as_ptr().cast(), 4, 0, 0) }, 0);
        }
        replace(&dir.join("config.toml"), "new", "newer").unwrap();
        let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(dir.join("real/config.toml")).unwrap().permissions());
        assert_eq!(mode & 0o777, 0o640);
        #[cfg(target_os = "macos")]
        {
            let path = std::ffi::CString::new(dir.join("real/config.toml").as_os_str().as_encoded_bytes()).unwrap();
            let mut buf = [0u8; 16];
            // SAFETY: `path` and `c"mddog.test"` are NUL-terminated C strings, and `buf` is valid for `buf.len()` bytes.
            let n = unsafe { libc::getxattr(path.as_ptr(), c"mddog.test".as_ptr(), buf.as_mut_ptr().cast(), buf.len(), 0, 0) };
            assert_eq!(n, 4);
            assert_eq!(&buf[..4], b"kept");
        }
        let decoy = dir.join("decoy");
        std::fs::write(&decoy, "keep").unwrap();
        let nonce = crate::tmp::nonce();
        std::os::unix::fs::symlink(&decoy, dir.join(format!("real/.config.toml.{nonce}"))).unwrap();
        assert!(replace(&dir.join("config.toml"), "newer", "evil").is_err());
        assert_eq!(std::fs::read_to_string(&decoy).unwrap(), "keep");
        std::fs::remove_file(dir.join(format!("real/.config.toml.{nonce}"))).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("real/config.toml")).unwrap(), "newer");
        assert_eq!(std::fs::read_dir(dir.join("real")).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn css_in_lists_names_a_theme_can_have() {
        let dir = std::env::temp_dir().join(format!("mddog-css-in-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub.css")).unwrap();
        std::fs::write(dir.join("b.css"), ":root{}").unwrap();
        std::fs::write(dir.join("a.css"), ":root{}").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        std::fs::write(dir.join("light:c.css"), ":root{}").unwrap();
        std::fs::write(dir.join("d,e.css"), ":root{}").unwrap();
        std::fs::write(dir.join(" f.css"), ":root{}").unwrap();
        assert_eq!(css_in(&dir, ""), ["a.css", "b.css", "sub.css"]);
        let held = std::fs::File::open(&dir).unwrap();
        held.lock().unwrap();
        assert!(std::fs::File::open(&dir).unwrap().try_lock().is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_is_default() {
        let dir = std::env::temp_dir().join(format!("mddog-config-{}", std::process::id()));
        assert_eq!(read(&dir).unwrap(), Config::default());
    }
}
