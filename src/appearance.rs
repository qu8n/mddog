use crate::config::Themes;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub struct Shown {
    light: &'static Theme,
    dark: Option<&'static Theme>,
    fixed: Option<&'static Theme>,
    told_dark: bool,
}

impl Shown {
    pub fn new(themes: &Themes) -> Shown {
        Shown { light: themes.light, dark: themes.dark, fixed: None, told_dark: false }
    }

    pub fn theme(&self) -> &'static Theme {
        match (self.fixed, self.dark) {
            (Some(t), _) => t,
            (None, Some(dark)) if self.is_dark() => dark,
            _ => self.light,
        }
    }

    pub fn is_dark(&self) -> bool {
        if cfg!(target_os = "macos") { system_dark() } else { self.told_dark }
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.told_dark = dark;
    }

    pub fn fixed(&self) -> Option<&'static Theme> {
        self.fixed
    }

    pub fn show(&mut self, t: Option<&'static Theme>) {
        self.fixed = t;
    }

    pub fn follows(&self) -> bool {
        self.dark.is_some() && self.fixed.is_none()
    }

    pub fn adopt(&mut self, themes: Themes) {
        (self.light, self.dark, self.fixed) = (themes.light, themes.dark, None);
    }
}

#[cfg(target_os = "macos")]
pub fn desktop_dark() -> Option<bool> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn system_dark() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn desktop_dark() -> Option<bool> {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    let owned = gdbus(&["org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus.NameHasOwner", "org.freedesktop.portal.Desktop"], until)?;
    if owned.trim() != "(true,)" && !portal_activatable(&gdbus(&["org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus.ListActivatableNames"], until)?) {
        return None;
    }
    portal_scheme(&gdbus(&["org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings.Read", "org.freedesktop.appearance", "color-scheme"], until)?)
}

#[cfg(not(target_os = "macos"))]
fn gdbus(call: &[&str], until: std::time::Instant) -> Option<String> {
    use std::process::{Command, Stdio};
    let [dest, path, method, args @ ..] = call else { return None };
    let mut child = Command::new("gdbus")
        .args(["call", "--session", "--timeout", "1", "--dest", dest, "--object-path", path, "--method", method])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    while child.try_wait().ok()?.is_none() {
        if std::time::Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    Some(String::from_utf8_lossy(&child.wait_with_output().ok()?.stdout).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn portal_activatable(reply: &str) -> bool {
    reply.contains("'org.freedesktop.portal.Desktop'")
}

#[cfg(not(target_os = "macos"))]
fn portal_scheme(reply: &str) -> Option<bool> {
    match reply.split("uint32 ").nth(1)?.chars().next()? {
        '1' => Some(true),
        '2' => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub fn system_dark() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    let style = NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str("AppleInterfaceStyle"));
    style.is_some_and(|s| s.to_string().eq_ignore_ascii_case("dark"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn themes(dark: bool) -> Themes {
        let t = |n| crate::theme::builtin(n).unwrap();
        Themes { light: t("github-light"), dark: dark.then(|| t("github-dark")) }
    }

    #[test]
    fn a_fixed_theme_wins_until_it_is_cleared() {
        let mut s = Shown::new(&themes(false));
        let fixed = crate::theme::builtin("github-dark").unwrap();
        assert!(std::ptr::eq(s.theme(), crate::theme::builtin("github-light").unwrap()));
        s.show(Some(fixed));
        assert!(std::ptr::eq(s.theme(), fixed) && s.fixed().is_some());
        s.show(None);
        assert!(!std::ptr::eq(s.theme(), fixed));
    }

    #[test]
    fn follows_only_a_pair_with_nothing_fixed() {
        let mut s = Shown::new(&themes(true));
        assert!(s.follows());
        s.show(Some(crate::theme::builtin("github-light").unwrap()));
        assert!(!s.follows());
        s.adopt(themes(false));
        assert!(!s.follows() && s.fixed().is_none());
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn the_terminal_answer_picks_the_dark_theme() {
        let mut s = Shown::new(&themes(true));
        assert!(std::ptr::eq(s.theme(), crate::theme::builtin("github-light").unwrap()));
        s.set_dark(true);
        assert!(std::ptr::eq(s.theme(), crate::theme::builtin("github-dark").unwrap()));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn the_portal_reply_names_the_scheme() {
        assert_eq!(portal_scheme("(<<uint32 1>>,)\n"), Some(true));
        assert_eq!(portal_scheme("(<<uint32 2>>,)\n"), Some(false));
        assert_eq!(portal_scheme("(<<uint32 0>>,)\n"), None);
        assert_eq!(portal_scheme(""), None);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn a_portal_that_can_start_counts() {
        assert!(portal_activatable("(['org.freedesktop.DBus', 'org.freedesktop.portal.Desktop', 'org.gnome.Shell'],)\n"));
        assert!(!portal_activatable("(['org.freedesktop.DBus', 'org.freedesktop.portal.Desktop.Extra'],)\n"));
        assert!(!portal_activatable(""));
    }
}
