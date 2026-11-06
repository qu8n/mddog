//! The clipboard: through pbcopy, and through the terminal for a viewer over SSH.
use std::io::Write;
use std::process::{Command, Stdio};

/// Puts `text` on the clipboard with pbcopy, and returns the OSC 52 sequence that does it through the terminal.
pub fn copy(text: &str) -> String {
    let text = pasteable(text);
    let seq = osc52(&text);
    pbcopy(text);
    seq
}

fn pasteable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t' | '\r')).collect()
}

/// OSC 52 puts `text` on the clipboard of the terminal the user sits at, also over SSH.
fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x1b\\", crate::term::kitty::base64(text.as_bytes()))
}

fn pbcopy(text: String) {
    std::thread::spawn(move || {
        let Ok(mut child) = Command::new("pbcopy").env("LC_CTYPE", "UTF-8").stdin(Stdio::piped()).spawn() else { return };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_text_drops_control_characters() {
        assert_eq!(pasteable("ls\x1b[201~echo\u{9b}hi\x07\n\tok\r\n"), "ls[201~echohi\n\tok\r\n");
    }

    #[test]
    fn osc52_carries_the_text_in_base64() {
        assert_eq!(osc52("é✓"), "\x1b]52;c;w6ninJM=\x1b\\");
    }
}
