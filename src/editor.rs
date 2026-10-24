use std::path::Path;
use std::process::{Command, Stdio};

pub fn command(editor: Option<&str>, file: &Path) -> Command {
    let editor = editor.map(str::trim).filter(|e| !e.is_empty()).unwrap_or("vi");
    let file = if file.is_relative() { Path::new(".").join(file) } else { file.to_path_buf() };
    let mut c = Command::new("/bin/sh");
    c.arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(file);
    c
}

pub fn run(file: &Path) -> Result<(), String> {
    let editor = std::env::var("EDITOR").ok();
    let name = editor.as_deref().map(str::trim).filter(|e| !e.is_empty()).unwrap_or("vi");
    let name = name.split_whitespace().next().and_then(|p| Path::new(p).file_name()).map_or(name.to_string(), |n| n.to_string_lossy().into_owned());
    let mut c = command(editor.as_deref(), file);
    let tty = |w: bool| std::fs::OpenOptions::new().read(!w).write(w).open("/dev/tty").map(Stdio::from).unwrap_or_else(|_| Stdio::inherit());
    c.stdin(tty(false)).stdout(tty(true)).stderr(tty(true));
    match c.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) if s.code() == Some(127) => Err(format!("Editor \"{name}\" not found. Set $EDITOR to your editor")),
        Ok(s) => Err(format!("Editor \"{name}\" failed ({s})")),
        Err(e) => Err(format!("Could not start the editor: {}", crate::term::io_error(&e))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d5_editor_command() {
        let c = command(Some("nvim -u NONE"), Path::new("/tmp/a b.md"));
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(c.get_program(), "/bin/sh");
        assert_eq!(args, vec!["-c", "nvim -u NONE \"$1\"", "sh", "/tmp/a b.md"]);
        let c = command(Some("  "), Path::new("f.md"));
        assert_eq!(c.get_args().nth(1).unwrap(), "vi \"$1\"");
        let c = command(None, Path::new("f.md"));
        assert_eq!(c.get_args().nth(1).unwrap(), "vi \"$1\"");
        let c = command(None, Path::new("+!touch pwned.md"));
        assert_eq!(c.get_args().nth(3).unwrap(), "./+!touch pwned.md");
        let c = command(None, Path::new("-R.md"));
        assert_eq!(c.get_args().nth(3).unwrap(), "./-R.md");
    }
}
