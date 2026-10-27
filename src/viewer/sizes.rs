//! Sizes seen so far, so the next load can size each placeholder to what replaces it: the viewBox
//! of each obsidian-engine diagram, keyed by a hash of its source, and the natural size of each
//! remote image, keyed by its URL. Kept in memory and in one small file each under the cache
//! folder. Any read or write error leaves the sizes as they were: a missing size only means the
//! page moves when that diagram draws or that image arrives.

use std::path::PathBuf;

use serde_json::{Map, Value};

const CAP: usize = 512;

pub struct Sizes {
    file: Option<PathBuf>,
    /// Oldest first.
    list: Vec<(String, [f64; 2])>,
}

impl Sizes {
    /// The sizes in `name` under the cache root.
    pub fn open(name: &str) -> Self {
        Self::at(Some(crate::assets::cache_dir().join(name)))
    }

    pub fn at(file: Option<PathBuf>) -> Self {
        let list = file.as_ref().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { file, list }
    }

    /// The sizes as the JSON object `mddog.guess` takes.
    pub fn json(&self) -> String {
        let map: Map<String, Value> = self.list.iter().map(|(k, s)| (k.clone(), Value::from(s.to_vec()))).collect();
        Value::Object(map).to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Adds the sizes from `mddog.state.sizes` and saves them when any changed.
    pub fn merge(&mut self, json: &str) {
        let Ok(new) = serde_json::from_str::<Map<String, Value>>(json) else { return };
        let mut changed = false;
        for (k, v) in new {
            let Ok(size) = serde_json::from_value::<[f64; 2]>(v) else { continue };
            let at = self.list.iter().position(|(key, _)| *key == k);
            if at.is_some_and(|i| self.list[i].1 == size) {
                continue;
            }
            if let Some(i) = at {
                self.list.remove(i);
            }
            self.list.push((k, size));
            changed = true;
        }
        if !changed {
            return;
        }
        let over = self.list.len().saturating_sub(CAP);
        self.list.drain(..over);
        if let Some(file) = &self.file {
            let tmp = file.with_extension(format!("tmp{}", std::process::id()));
            let ok = file.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
                && serde_json::to_vec(&self.list).is_ok_and(|b| std::fs::write(&tmp, b).is_ok())
                && std::fs::rename(&tmp, file).is_ok();
            if !ok {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder, removed when the test ends.
    struct Dir(PathBuf);

    impl std::ops::Deref for Dir {
        type Target = PathBuf;
        fn deref(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("mddog-sizes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        Dir(d)
    }

    #[test]
    fn saves_and_reads_back() {
        let d = dir("round");
        let file = d.join("a/sizes.json");
        let mut s = Sizes::at(Some(file.clone()));
        assert!(s.is_empty());
        s.merge(r#"{"ab": [916.5, 270], "cd": [248, 584]}"#);
        let back = Sizes::at(Some(file));
        assert_eq!(back.json(), r#"{"ab":[916.5,270.0],"cd":[248.0,584.0]}"#);
    }

    #[test]
    fn ignores_a_corrupt_file() {
        let d = dir("corrupt");
        std::fs::create_dir_all(&*d).unwrap();
        let file = d.join("sizes.json");
        std::fs::write(&file, b"{not json").unwrap();
        let mut s = Sizes::at(Some(file.clone()));
        assert!(s.is_empty());
        s.merge(r#"{"ab": [1, 2], "bad": "x"}"#);
        assert_eq!(Sizes::at(Some(file)).json(), r#"{"ab":[1.0,2.0]}"#);
    }

    #[test]
    fn keeps_working_when_unwritable() {
        let d = dir("unwritable");
        std::fs::create_dir_all(&*d).unwrap();
        std::fs::write(d.join("file"), b"").unwrap();
        let mut s = Sizes::at(Some(d.join("file/sizes.json")));
        s.merge(r#"{"ab": [1, 2]}"#);
        assert_eq!(s.json(), r#"{"ab":[1.0,2.0]}"#);
        assert!(!d.join("file/sizes.json").exists());
    }

    #[test]
    fn drops_the_oldest_past_the_cap() {
        let d = dir("cap");
        let file = d.join("sizes.json");
        let mut s = Sizes::at(Some(file.clone()));
        let many: Map<String, Value> = (0..CAP + 3).map(|i| (format!("k{i}"), Value::from(vec![1.0, 2.0]))).collect();
        s.merge(&Value::Object(many).to_string());
        s.merge(r#"{"k0": [3, 4]}"#);
        let back = Sizes::at(Some(file));
        assert_eq!(back.list.len(), CAP);
        assert!(!back.list.iter().any(|(k, _)| k == "k1"));
        assert_eq!(back.list.last().unwrap(), &("k0".to_string(), [3.0, 4.0]));
    }
}
