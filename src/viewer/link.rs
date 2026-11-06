//! Which links the viewer follows, and opening one outside it.
use std::process::{Command, Stdio};

#[derive(Debug, PartialEq, Eq)]
pub enum Link {
    /// A web or mail link, for the system to open.
    Open(String),
    /// A heading on the page, by its id.
    Anchor(String),
}

impl Link {
    /// None for a link the viewer doesn't follow, such as a local file.
    pub fn of(url: &str) -> Option<Link> {
        if let Some(id) = url.strip_prefix('#') {
            return Some(Link::Anchor(id.to_string()));
        }
        let scheme = url.split(':').next()?.to_ascii_lowercase();
        matches!(scheme.as_str(), "http" | "https" | "mailto").then(|| Link::Open(url.to_string()))
    }
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
    fn only_web_and_mail_links_open() {
        let open = |u: &str| Some(Link::Open(u.to_string()));
        assert_eq!(Link::of("https://a.dev/x"), open("https://a.dev/x"));
        assert_eq!(Link::of("HTTP://a.dev"), open("HTTP://a.dev"));
        assert_eq!(Link::of("mailto:a@b.dev"), open("mailto:a@b.dev"));
        assert_eq!(Link::of("#intro"), Some(Link::Anchor("intro".into())));
        assert_eq!(Link::of("docs/a.md"), None);
        assert_eq!(Link::of("javascript:alert(1)"), None);
        assert_eq!(Link::of("x.html"), None);
        assert_eq!(Link::of("data:text/html,hi"), None);
    }
}
