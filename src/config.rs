//! `vortex.project.json`, the file that marks a VortexSync project.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE: &str = "vortex.project.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub name: String,
    /// Folder with the instance files, relative to the project root.
    #[serde(default = "default_source")]
    pub source: String,
    /// The .vrtx Studio opens, relative to the project root or absolute.
    #[serde(default = "default_output")]
    pub output: String,
    /// Studio's project id, kept so rebuilt files stay the same project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

fn default_source() -> String {
    "src".into()
}

fn default_output() -> String {
    "game.vrtx".into()
}

/// A loaded config plus where it lives.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
}

impl Project {
    pub fn source_dir(&self) -> PathBuf {
        self.root.join(&self.config.source)
    }

    pub fn output_path(&self) -> PathBuf {
        self.root.join(&self.config.output)
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join(".vortexsync")
    }

    /// Looks for `vortex.project.json` in `start` and its parents, like git does.
    pub fn find(start: &Path) -> Result<Self, String> {
        let mut dir = Some(start);
        while let Some(d) = dir {
            let candidate = d.join(FILE);
            if candidate.is_file() {
                return Self::load(d);
            }
            dir = d.parent();
        }
        Err(format!(
            "no {FILE} here or in any parent folder. Run `vortexsync init` first"
        ))
    }

    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(FILE);
        let text =
            fs::read_to_string(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let config: Config =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        for (what, value) in [("source", &config.source), ("output", &config.output)] {
            if value.trim().is_empty() {
                return Err(format!("{FILE}: {what} can't be empty"));
            }
        }
        if !config.output.ends_with(".vrtx") {
            return Err(format!(
                "{FILE}: output should end in .vrtx so Studio can open it"
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            config,
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let path = self.root.join(FILE);
        let mut text = serde_json::to_string_pretty(&self.config).map_err(|e| e.to_string())?;
        text.push('\n');
        fs::write(&path, text).map_err(|e| format!("can't write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_from_a_subfolder_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(FILE), r#"{ "name": "Obby" }"#).unwrap();
        let deep = dir.path().join("src/Workspace");
        fs::create_dir_all(&deep).unwrap();
        let p = Project::find(&deep).unwrap();
        assert_eq!(p.root, dir.path());
        assert_eq!(p.source_dir(), dir.path().join("src"));
        assert_eq!(p.output_path(), dir.path().join("game.vrtx"));
    }

    #[test]
    fn bad_configs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Project::find(dir.path()).unwrap_err().contains("init"));
        fs::write(
            dir.path().join(FILE),
            r#"{ "name": "x", "output": "game.rbxl" }"#,
        )
        .unwrap();
        assert!(Project::load(dir.path()).unwrap_err().contains(".vrtx"));
        fs::write(
            dir.path().join(FILE),
            r#"{ "name": "x", "outptu": "a.vrtx" }"#,
        )
        .unwrap();
        assert!(
            Project::load(dir.path())
                .unwrap_err()
                .contains("unknown field")
        );
    }
}
