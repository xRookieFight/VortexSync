//! The commands, minus argument parsing and printing.
//!
//! The rule that keeps work safe: a direction never overwrites the other side
//! when that side changed since the last sync, unless `force` says so.

use std::fs;
use std::path::{Path, PathBuf};

use vortexstudio_mcp::check::{self, Finding};
use vortexstudio_mcp::lint::Severity;
use vortexstudio_mcp::{scene, store, vrtx};

use crate::config::{self, Config, Project};
use crate::disk::{self, WriteReport};
use crate::layout;
use crate::state::{self, State, Status};

type Result<T> = std::result::Result<T, String>;

pub struct Sides {
    pub files: layout::Files,
    pub files_hash: String,
    pub vrtx: Option<Vec<u8>>,
    pub vrtx_hash: Option<String>,
}

pub fn read_sides(p: &Project) -> Result<Sides> {
    let files = disk::read(&p.source_dir())?;
    let files_hash = state::hash_files(&files);
    let vrtx = match fs::read(p.output_path()) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("can't read {}: {e}", p.output_path().display())),
    };
    let vrtx_hash = vrtx.as_deref().map(state::hash_bytes);
    Ok(Sides {
        files,
        files_hash,
        vrtx,
        vrtx_hash,
    })
}

pub fn status(p: &Project) -> Result<Status> {
    let s = read_sides(p)?;
    Ok(State::load(&p.state_dir()).status(&s.files_hash, s.vrtx_hash.as_deref()))
}

fn record(p: &Project) -> Result<()> {
    let s = read_sides(p)?;
    State {
        files_hash: Some(s.files_hash),
        vrtx_hash: s.vrtx_hash,
    }
    .save(&p.state_dir())
}

#[derive(Debug)]
pub struct Built {
    pub instances: usize,
    pub findings: Vec<Finding>,
    pub backup: Option<PathBuf>,
    pub studio_running: bool,
}

/// Files to .vrtx. Refuses when Studio saved changes we haven't extracted yet.
pub fn build(p: &Project, force: bool, strict: bool) -> Result<Built> {
    let sides = read_sides(p)?;
    let st = State::load(&p.state_dir()).status(&sides.files_hash, sides.vrtx_hash.as_deref());
    if !force && matches!(st, Status::StudioChanged | Status::Conflict) {
        return Err(format!(
            "{} was changed in Studio since the last sync. Run `vortexsync extract` to keep those changes, \
             or `vortexsync build --force` to throw them away",
            p.config.output
        ));
    }
    if !force && st == Status::Unknown && sides.vrtx.is_some() {
        return Err(format!(
            "{} exists but was never synced with these files. Run `vortexsync extract` to start from it, \
             or `vortexsync build --force` to overwrite it",
            p.config.output
        ));
    }
    let project = layout::build(&sides.files, p.config.project_id.clone())?;
    let findings: Vec<Finding> = check::check(&project)
        .into_iter()
        // sorted siblings make duplicate names a normal thing here, not worth a line on every build
        .filter(|f| f.rule != "duplicate-name" || f.severity != Severity::Info)
        .collect();
    if strict && findings.iter().any(|f| f.severity >= Severity::Warning) {
        return Err(format!(
            "--strict: {} problems, not building\n{}",
            findings.len(),
            describe(&findings)
        ));
    }
    let backup = store::save(&p.output_path(), &project)?;
    record(p)?;
    Ok(Built {
        instances: project.instances.len(),
        findings,
        backup,
        studio_running: store::studio_running(),
    })
}

/// .vrtx to files. Refuses when files were edited since the last sync.
pub fn extract(p: &Project, force: bool) -> Result<WriteReport> {
    let sides = read_sides(p)?;
    let bytes = sides.vrtx.as_ref().ok_or_else(|| {
        format!(
            "{} doesn't exist yet, run `vortexsync build`",
            p.config.output
        )
    })?;
    let st = State::load(&p.state_dir()).status(&sides.files_hash, sides.vrtx_hash.as_deref());
    if !force && matches!(st, Status::FilesChanged | Status::Conflict) {
        return Err(
            "files were edited since the last sync. Run `vortexsync build` to send them to Studio, \
             or `vortexsync extract --force` to throw them away"
                .into(),
        );
    }
    if !force && st == Status::Unknown && !sides.files.is_empty() {
        return Err(
            "the source folder has files that were never synced. Run `vortexsync extract --force` to replace them".into(),
        );
    }
    let project = vrtx::decode(bytes).map_err(|e| format!("{}: {e}", p.config.output))?;
    let files = layout::extract(&project)?;
    let report = disk::write(&p.source_dir(), &files)?;
    remember_id(p, &project)?;
    record(p)?;
    Ok(report)
}

fn remember_id(p: &Project, project: &vrtx::Project) -> Result<()> {
    if p.config.project_id.is_none() && project.project_id.is_some() {
        let mut updated = p.clone();
        updated.config.project_id = project.project_id.clone();
        updated.save()?;
    }
    Ok(())
}

pub enum Synced {
    Nothing,
    Built(Built),
    Extracted(WriteReport),
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prefer {
    Files,
    Studio,
}

/// One step of `serve`: moves whichever side changed to the other one.
pub fn sync(p: &Project, prefer: Option<Prefer>) -> Result<Synced> {
    let st = status(p)?;
    let out_exists = p.output_path().exists();
    Ok(match (st, prefer) {
        (Status::InSync, _) => Synced::Nothing,
        (Status::FilesChanged, _) => Synced::Built(build(p, false, false)?),
        (Status::StudioChanged, _) => Synced::Extracted(extract(p, false)?),
        (Status::Conflict | Status::Unknown, Some(Prefer::Files)) => {
            Synced::Built(build(p, true, false)?)
        }
        (Status::Conflict | Status::Unknown, Some(Prefer::Studio)) if out_exists => {
            Synced::Extracted(extract(p, true)?)
        }
        (Status::Unknown, _) if !out_exists => Synced::Built(build(p, true, false)?),
        (Status::Conflict | Status::Unknown, _) => Synced::Conflict,
    })
}

/// Sets up a project in `root`. With `from`, the files come from that .vrtx,
/// otherwise from Studio's empty template.
pub fn init(
    root: &Path,
    name: &str,
    from: Option<&Path>,
    output: Option<String>,
) -> Result<Project> {
    if root.join(config::FILE).exists() {
        return Err(format!("{} already has a {}", root.display(), config::FILE));
    }
    let src = root.join("src");
    if disk::read(&src)?.keys().next().is_some() {
        return Err(format!(
            "{} already has files in it, pick an empty folder",
            src.display()
        ));
    }
    let (project, output) = match from {
        Some(path) => {
            let project = store::load(path)?;
            // keep using the file Studio already has open, made relative when it's inside the project
            let out = output.unwrap_or_else(|| {
                path.strip_prefix(root)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| path.display().to_string())
            });
            (project, out)
        }
        None => (
            scene::new_project(new_id()),
            output.unwrap_or_else(|| "game.vrtx".into()),
        ),
    };
    let config = Config {
        name: name.into(),
        source: "src".into(),
        output,
        project_id: project.project_id.clone(),
    };
    if !config.output.ends_with(".vrtx") {
        return Err("the output file should end in .vrtx so Studio can open it".into());
    }
    let p = Project {
        root: root.to_path_buf(),
        config,
    };

    fs::create_dir_all(root).map_err(|e| format!("can't create {}: {e}", root.display()))?;
    disk::write(&p.source_dir(), &layout::extract(&project)?)?;
    // every service gets a folder so there's an obvious place for the first script.
    // git doesn't track empty folders, hence the .gitkeep, which VortexSync ignores
    for service in layout::SERVICES {
        let dir = p.source_dir().join(service.to_string());
        fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        if fs::read_dir(&dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            fs::write(dir.join(".gitkeep"), "").map_err(|e| e.to_string())?;
        }
    }
    if !p.output_path().exists() {
        store::save(&p.output_path(), &project)?;
    }
    p.save()?;
    write_gitignore(root, &p.config.output)?;
    record(&p)?;
    Ok(p)
}

fn write_gitignore(root: &Path, output: &str) -> Result<()> {
    let path = root.join(".gitignore");
    let mut lines = vec![".vortexsync/".to_string(), ".vrtx-backups/".to_string()];
    // the .vrtx is built from the files, keeping it in git would only cause conflicts
    if !Path::new(output).is_absolute() {
        lines.push(format!("/{output}"));
    }
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let missing: Vec<&String> = lines
        .iter()
        .filter(|l| !existing.lines().any(|e| e.trim() == l.as_str()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for l in missing {
        text.push_str(l);
        text.push('\n');
    }
    fs::write(&path, text).map_err(|e| format!("can't write .gitignore: {e}"))
}

/// Builds from the files without writing anything and runs the project check.
pub fn check(p: &Project) -> Result<Vec<Finding>> {
    let files = disk::read(&p.source_dir())?;
    let project = layout::build(&files, p.config.project_id.clone())?;
    Ok(check::check(&project))
}

pub fn describe(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            let place = match (&f.instance, f.line) {
                (Some(i), Some(l)) => format!("{i}:{l}"),
                (Some(i), None) => i.clone(),
                _ => "project".into(),
            };
            let severity = match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            };
            format!("  {severity} {place}: {}", f.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn new_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for salt in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let p = init(dir.path(), "Test", None, None).unwrap();
        (dir, p)
    }

    #[test]
    fn init_makes_a_synced_project() {
        let (dir, p) = fresh();
        assert!(
            dir.path()
                .join("src/Workspace/Baseplate.part.json")
                .exists()
        );
        assert!(dir.path().join("game.vrtx").exists());
        assert!(dir.path().join("src/ServerScriptService/.gitkeep").exists());
        let gi = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gi.contains("/game.vrtx") && gi.contains(".vortexsync/"));
        assert_eq!(status(&p).unwrap(), Status::InSync);
        assert!(init(dir.path(), "Again", None, None).is_err());
    }

    #[test]
    fn edit_files_then_build() {
        let (dir, p) = fresh();
        fs::write(
            dir.path().join("src/Workspace/Lava.part.json"),
            r##"{ "position": [0, 1, 0], "size": [4, 1, 4], "color": "#FF0000" }"##,
        )
        .unwrap();
        assert_eq!(status(&p).unwrap(), Status::FilesChanged);
        // extracting now would throw the new file away, so it refuses
        assert!(extract(&p, false).is_err());
        let built = build(&p, false, false).unwrap();
        assert_eq!(built.instances, 7);
        assert!(built.backup.is_some());
        assert_eq!(status(&p).unwrap(), Status::InSync);
        let project = store::load(&p.output_path()).unwrap();
        assert!(project.instances.iter().any(|i| i.name == "Lava"));
    }

    #[test]
    fn studio_save_then_extract() {
        let (dir, p) = fresh();
        let mut project = store::load(&p.output_path()).unwrap();
        scene::create(
            &mut project,
            3,
            vrtx::Class::Script,
            "Main",
            Some("print('from studio')".into()),
        )
        .unwrap();
        store::save(&p.output_path(), &project).unwrap();
        assert_eq!(status(&p).unwrap(), Status::StudioChanged);
        assert!(build(&p, false, false).is_err());
        let report = extract(&p, false).unwrap();
        assert_eq!(report.written, ["ServerScriptService/Main.server.luau"]);
        assert_eq!(
            fs::read_to_string(dir.path().join("src/ServerScriptService/Main.server.luau"))
                .unwrap(),
            "print('from studio')"
        );
    }

    #[test]
    fn both_sides_changed_is_a_conflict() {
        let (dir, p) = fresh();
        fs::write(dir.path().join("src/Workspace/A.part.json"), "{}").unwrap();
        let mut project = store::load(&p.output_path()).unwrap();
        scene::rename(&mut project, 5, "Floor").unwrap();
        store::save(&p.output_path(), &project).unwrap();
        assert_eq!(status(&p).unwrap(), Status::Conflict);
        assert!(matches!(sync(&p, None).unwrap(), Synced::Conflict));
        // nothing was touched while in conflict
        assert!(dir.path().join("src/Workspace/A.part.json").exists());
        assert!(matches!(
            sync(&p, Some(Prefer::Studio)).unwrap(),
            Synced::Extracted(_)
        ));
        assert!(!dir.path().join("src/Workspace/A.part.json").exists());
        assert!(dir.path().join("src/Workspace/Floor.part.json").exists());
    }

    #[test]
    fn init_from_existing_vrtx() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("mygame.vrtx");
        fs::copy("tests/fixtures/showcase.vrtx", &game).unwrap();
        let p = init(dir.path(), "Showcase", Some(&game), None).unwrap();
        assert_eq!(p.config.output, "mygame.vrtx");
        assert_eq!(
            p.config.project_id.as_deref(),
            Some("8a73ef37b35140fb9a1c60217d3a776c")
        );
        assert!(
            dir.path()
                .join("src/ServerScriptService/Script.server.luau")
                .exists()
        );
        assert_eq!(status(&p).unwrap(), Status::InSync);
        // the original file is untouched until something is built
        assert_eq!(
            fs::read(&game).unwrap(),
            fs::read("tests/fixtures/showcase.vrtx").unwrap()
        );
    }

    #[test]
    fn strict_build_stops_on_warnings() {
        let (dir, p) = fresh();
        fs::write(
            dir.path().join("src/ServerScriptService/Bad.server.luau"),
            "game:GetService(\"DataStoreService\")",
        )
        .unwrap();
        assert!(build(&p, false, true).unwrap_err().contains("--strict"));
        let built = build(&p, false, false).unwrap();
        assert!(built.findings.iter().any(|f| f.rule == "unknown-service"));
    }
}
