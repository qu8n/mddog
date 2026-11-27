pub fn io(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        std::io::ErrorKind::IsADirectory => "it is a directory, not a file".into(),
        _ => {
            let s = e.to_string();
            let s = s.split(" (os error").next().unwrap_or(&s);
            let mut c = s.chars();
            c.next().map_or_else(String::new, |f| f.to_lowercase().chain(c).collect())
        }
    }
}
