//! `vortexsync serve`: watch both sides and sync whichever one changes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::config::Project;
use crate::ops::{self, Prefer, Synced};

// Studio and editors often write a file in a few steps, wait for them to settle
const QUIET: Duration = Duration::from_millis(400);

pub enum Note {
    /// The watchers are registered, changes from now on will be seen.
    Watching,
    Built {
        instances: usize,
        problems: String,
        studio_running: bool,
    },
    Extracted {
        written: usize,
        deleted: usize,
    },
    Conflict,
    Failed(String),
}

fn relevant(event: &Event, src: &Path, output: &Path, ignore: &[PathBuf]) -> bool {
    // reading the folders during a sync fires access events, reacting to those
    // would keep us busy forever
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|p| {
        if ignore.iter().any(|i| p.starts_with(i)) {
            return false;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // our own atomic save writes a dotted temp file next to the output first
        if name.starts_with('.') {
            return false;
        }
        p == output || p.starts_with(src)
    })
}

/// One sync step, turned into something to print. Errors don't stop serving:
/// a half saved file from Studio fixes itself on the next event.
pub fn step(p: &Project, prefer: Option<Prefer>) -> Option<Note> {
    match ops::sync(p, prefer) {
        Ok(Synced::Nothing) => None,
        Ok(Synced::Built(b)) => Some(Note::Built {
            instances: b.instances,
            problems: ops::describe(&b.findings),
            studio_running: b.studio_running,
        }),
        Ok(Synced::Extracted(r)) if r.is_empty() => None,
        Ok(Synced::Extracted(r)) => Some(Note::Extracted {
            written: r.written.len(),
            deleted: r.deleted.len(),
        }),
        Ok(Synced::Conflict) => Some(Note::Conflict),
        Err(e) => Some(Note::Failed(e)),
    }
}

/// Runs until the watcher dies. `report` gets every note worth printing.
pub fn serve(
    p: &Project,
    prefer: Option<Prefer>,
    mut report: impl FnMut(Note),
) -> Result<(), String> {
    // watchers report real paths (macOS turns /var into /private/var), so compare
    // against real paths too or symlinked folders never match
    let real = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let src = p.source_dir();
    fs::create_dir_all(&src).map_err(|e| format!("can't create {}: {e}", src.display()))?;
    let src = real(&src);
    let output = p.output_path();
    let out_dir = output
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| p.root.clone());
    let out_dir = real(&out_dir);
    let output = out_dir.join(output.file_name().unwrap_or_default());
    let ignore = [
        real(&p.state_dir()),
        out_dir.join(vortexstudio_mcp::store::BACKUP_DIR),
    ];

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let _ = tx.send(res);
    })
    .map_err(|e| format!("can't watch files: {e}"))?;
    watcher
        .watch(&src, RecursiveMode::Recursive)
        .map_err(|e| format!("can't watch {}: {e}", src.display()))?;
    // the folder rather than the file, so atomic renames by Studio are seen too
    watcher
        .watch(&out_dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("can't watch {}: {e}", out_dir.display()))?;

    let mut in_conflict = false;
    let mut handle = |note: Option<Note>, report: &mut dyn FnMut(Note)| match note {
        Some(Note::Conflict) if in_conflict => {}
        Some(Note::Conflict) => {
            in_conflict = true;
            report(Note::Conflict);
        }
        Some(n) => {
            in_conflict = false;
            report(n);
        }
        None => in_conflict = false,
    };

    report(Note::Watching);
    handle(step(p, prefer), &mut report);
    loop {
        // wait for something that matters
        loop {
            match rx.recv() {
                Ok(Ok(ev)) if relevant(&ev, &src, &output, &ignore) => break,
                Ok(_) => {}
                Err(_) => return Err("the file watcher stopped".into()),
            }
        }
        // then until things that matter have been quiet for a moment. Unrelated
        // events don't push the deadline, or our own reads would stall us
        let mut deadline = Instant::now() + QUIET;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match rx.recv_timeout(left) {
                Ok(Ok(ev)) if relevant(&ev, &src, &output, &ignore) => {
                    deadline = Instant::now() + QUIET
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("the file watcher stopped".into());
                }
            }
        }
        handle(step(p, prefer), &mut report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    fn wait_until_watching(ready: &mpsc::Receiver<()>) {
        ready
            .recv_timeout(Duration::from_secs(10))
            .expect("watcher never started");
        // FSEvents on macOS only reports changes made after its stream really started
        std::thread::sleep(Duration::from_millis(500));
    }

    // drives the real watcher: edit a file and wait for the .vrtx to follow
    #[test]
    fn file_edits_reach_the_vrtx() {
        let dir = tempfile::tempdir().unwrap();
        let p = ops::init(dir.path(), "Watch", None, None).unwrap();
        let notes = Arc::new(Mutex::new(Vec::new()));
        let sink = notes.clone();
        let project = p.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = serve(&project, None, |n| {
                let label = match n {
                    Note::Watching => {
                        let _ = ready_tx.send(());
                        return;
                    }
                    Note::Built { .. } => "built".to_string(),
                    Note::Extracted { .. } => "extracted".to_string(),
                    Note::Conflict => "conflict".to_string(),
                    Note::Failed(e) => format!("failed: {e}"),
                };
                sink.lock().unwrap().push(label);
            });
            eprintln!("serve returned: {:?}", result.err());
        });
        wait_until_watching(&ready_rx);
        fs::write(dir.path().join("src/Workspace/Watched.part.json"), "{}").unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let project = vortexstudio_mcp::store::load(&p.output_path()).unwrap();
            if project.instances.iter().any(|i| i.name == "Watched") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "vrtx never updated, notes: {:?}",
                notes.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(ops::status(&p).unwrap(), crate::state::Status::InSync);
        assert_eq!(notes.lock().unwrap().as_slice(), ["built"]);
    }

    // the other direction: a save that replaces the .vrtx through a rename, like
    // an atomic save from Studio, shows up in the files
    #[test]
    fn studio_saves_reach_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = ops::init(dir.path(), "Watch", None, None).unwrap();
        let project = p.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = serve(&project, None, |n| {
                if matches!(n, Note::Watching) {
                    let _ = ready_tx.send(());
                }
            });
        });
        wait_until_watching(&ready_rx);

        let mut game = vortexstudio_mcp::store::load(&p.output_path()).unwrap();
        vortexstudio_mcp::scene::create(
            &mut game,
            3,
            vortexstudio_mcp::vrtx::Class::Script,
            "FromStudio",
            None,
        )
        .unwrap();
        vortexstudio_mcp::store::save(&p.output_path(), &game).unwrap();

        let target = dir
            .path()
            .join("src/ServerScriptService/FromStudio.server.luau");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !target.exists() {
            assert!(Instant::now() < deadline, "files never updated");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
