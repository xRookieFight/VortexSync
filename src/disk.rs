//! Reading the source folder into a [`Files`] map and writing one back,
//! touching only what changed and only files VortexSync owns.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};

use crate::layout::Files;

/// File types VortexSync reads and may delete. Everything else in the source
/// folder (README, images, notes) is left alone.
pub fn is_owned(name: &str) -> bool {
    !name.starts_with('.')
        && [".json", ".luau", ".lua"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

pub fn read(src: &Path) -> Result<Files, String> {
    let mut files = Files::new();
    if !src.exists() {
        return Ok(files);
    }
    walk(src, "", &mut files)?;
    Ok(files)
}

fn walk(dir: &Path, rel: &str, files: &mut Files) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|n| format!("{}: file name isn't valid UTF-8 ({n:?})", dir.display()))?;
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let key = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            walk(&path, &key, files)?;
        } else if kind.is_file() && is_owned(&name) {
            let bytes =
                fs::read(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
            files.insert(key, bytes);
        }
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq)]
pub struct WriteReport {
    pub written: Vec<String>,
    pub deleted: Vec<String>,
}

impl WriteReport {
    pub fn is_empty(&self) -> bool {
        self.written.is_empty() && self.deleted.is_empty()
    }
}

/// Makes the owned files under `src` equal to `wanted`.
pub fn write(src: &Path, wanted: &Files) -> Result<WriteReport, String> {
    for key in wanted.keys() {
        let rel = Path::new(key);
        if key.is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(format!(
                "refusing to write outside the source folder: {key:?}"
            ));
        }
    }
    let current = read(src)?;
    let mut report = WriteReport::default();

    for (key, bytes) in wanted {
        if current.get(key) == Some(bytes) {
            continue;
        }
        let path = src.join(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("can't create {}: {e}", parent.display()))?;
        }
        fs::write(&path, bytes).map_err(|e| format!("can't write {}: {e}", path.display()))?;
        report.written.push(key.clone());
    }

    for key in current.keys().filter(|k| !wanted.contains_key(*k)) {
        let path = src.join(key);
        match fs::remove_file(&path) {
            Ok(()) => report.deleted.push(key.clone()),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(format!("can't delete {}: {e}", path.display())),
        }
        prune_empty_dirs(src, path.parent());
    }
    Ok(report)
}

// walk up from a deleted file and drop folders it left empty, never the source folder itself
fn prune_empty_dirs(src: &Path, mut dir: Option<&Path>) {
    while let Some(d) = dir {
        if d == src || !d.starts_with(src) {
            break;
        }
        if fs::remove_dir(d).is_err() {
            break; // not empty, or not ours to remove
        }
        dir = d.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(pairs: &[(&str, &str)]) -> Files {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn writes_only_changes_and_leaves_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let first = files(&[
            ("Workspace/A.part.json", "{}"),
            ("Workspace/Tower/B.part.json", "{}"),
        ]);
        let r = write(&src, &first).unwrap();
        assert_eq!(r.written.len(), 2);
        fs::write(src.join("Workspace/Tower/notes.md"), "mine").unwrap();
        fs::write(src.join("Workspace/.hidden.json"), "mine").unwrap();

        let second = files(&[
            ("Workspace/A.part.json", "{}"),
            ("Workspace/C.part.json", "{\"x\":1}"),
        ]);
        let r = write(&src, &second).unwrap();
        assert_eq!(r.written, ["Workspace/C.part.json"]);
        assert_eq!(r.deleted, ["Workspace/Tower/B.part.json"]);
        // the folder still holds a file we don't own, so it stays
        assert!(src.join("Workspace/Tower/notes.md").exists());
        assert!(src.join("Workspace/.hidden.json").exists());
        assert_eq!(read(&src).unwrap(), second);

        assert!(write(&src, &second).unwrap().is_empty());
    }

    #[test]
    fn empty_folders_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        write(&src, &files(&[("Workspace/Deep/Er/X.part.json", "{}")])).unwrap();
        write(&src, &Files::new()).unwrap();
        assert!(!src.join("Workspace").exists());
        assert!(src.exists());
    }

    #[test]
    fn refuses_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write(dir.path(), &files(&[("../x.json", "{}")])).is_err());
        assert!(write(dir.path(), &files(&[("/etc/x.json", "{}")])).is_err());
    }
}
