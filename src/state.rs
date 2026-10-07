//! What both sides looked like after the last sync, so we can tell which one
//! changed since. Lives in `.vortexsync/state.json`, which stays out of git.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::layout::Files;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct State {
    pub files_hash: Option<String>,
    pub vrtx_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    InSync,
    /// Files were edited since the last sync, a build would pick them up.
    FilesChanged,
    /// Studio saved the project since the last sync, an extract would pick it up.
    StudioChanged,
    /// Both sides moved. Somebody has to pick which one wins.
    Conflict,
    /// No sync has happened yet.
    Unknown,
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn hash_files(files: &Files) -> String {
    let mut h = Sha256::new();
    for (path, bytes) in files {
        // lengths keep "ab"+"c" and "a"+"bc" apart
        h.update((path.len() as u64).to_le_bytes());
        h.update(path.as_bytes());
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl State {
    pub fn load(dir: &Path) -> State {
        fs::read_to_string(dir.join("state.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        // keeps the folder out of git even if the user's .gitignore misses it
        let _ = fs::write(dir.join(".gitignore"), "*\n");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(dir.join("state.json"), text).map_err(|e| format!("can't write sync state: {e}"))
    }

    /// Compares current hashes against the last sync. A missing .vrtx counts
    /// as unchanged when we never had one, and as a Studio change otherwise.
    pub fn status(&self, files_hash: &str, vrtx_hash: Option<&str>) -> Status {
        if self.files_hash.is_none() && self.vrtx_hash.is_none() {
            return Status::Unknown;
        }
        let files_moved = self.files_hash.as_deref() != Some(files_hash);
        let vrtx_moved = self.vrtx_hash.as_deref() != vrtx_hash;
        match (files_moved, vrtx_moved) {
            (false, false) => Status::InSync,
            (true, false) => Status::FilesChanged,
            (false, true) => Status::StudioChanged,
            (true, true) => Status::Conflict,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        let s = State {
            files_hash: Some("f".into()),
            vrtx_hash: Some("v".into()),
        };
        assert_eq!(s.status("f", Some("v")), Status::InSync);
        assert_eq!(s.status("f2", Some("v")), Status::FilesChanged);
        assert_eq!(s.status("f", Some("v2")), Status::StudioChanged);
        assert_eq!(s.status("f2", None), Status::Conflict);
        assert_eq!(State::default().status("f", None), Status::Unknown);
    }

    #[test]
    fn file_hash_sees_paths_and_contents() {
        let a: Files = [("a".to_string(), b"bc".to_vec())].into();
        let b: Files = [("ab".to_string(), b"c".to_vec())].into();
        assert_ne!(hash_files(&a), hash_files(&b));
    }

    #[test]
    fn saves_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let s = State {
            files_hash: Some("f".into()),
            vrtx_hash: None,
        };
        s.save(dir.path()).unwrap();
        assert_eq!(State::load(dir.path()), s);
        assert_eq!(State::load(&dir.path().join("missing")), State::default());
    }
}
