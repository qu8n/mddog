pub fn replace(s: &str) -> String {
    if !s.contains(':') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(':') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let len = after.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))).unwrap_or(after.len());
        match emojis::get_by_shortcode(&after[..len]) {
            Some(e) if len > 0 && after[len..].starts_with(':') => {
                out.push_str(e.as_str());
                rest = &after[len + 1..];
            }
            _ => {
                out.push(':');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn shortcodes() {
        assert_eq!(super::replace(":smile: :+1: :not_an_emoji: x:smile:y"), "😄 👍 :not_an_emoji: x😄y");
    }
}
