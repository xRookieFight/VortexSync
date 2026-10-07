use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use vortexsync::config::Project;
use vortexsync::ops::{self, Prefer};
use vortexsync::serve::{self, Note};
use vortexsync::state::Status;

/// Keep a Vortex Studio project in sync with a folder of files you can edit and commit.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Run as if started in this folder.
    #[arg(short = 'C', global = true, value_name = "DIR")]
    dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Side {
    /// Keep the files, overwrite the .vrtx.
    Files,
    /// Keep what Studio saved, overwrite the files.
    Studio,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start a project here, empty or from an existing .vrtx.
    Init {
        /// Folder to set up, defaults to the current one.
        path: Option<PathBuf>,
        /// Start from this Studio project instead of an empty one.
        #[arg(long)]
        from: Option<PathBuf>,
        /// Project name, defaults to the folder name.
        #[arg(long)]
        name: Option<String>,
        /// The .vrtx to build, defaults to game.vrtx (or the --from file).
        #[arg(long)]
        output: Option<String>,
    },
    /// Turn the files into the .vrtx Studio opens.
    Build {
        /// Overwrite Studio changes that weren't extracted.
        #[arg(long)]
        force: bool,
        /// Fail on lint warnings instead of printing them.
        #[arg(long)]
        strict: bool,
    },
    /// Turn the .vrtx into files.
    Extract {
        /// Overwrite file edits that weren't built.
        #[arg(long)]
        force: bool,
    },
    /// Watch both sides and sync whichever changes.
    Serve {
        /// How to settle it when both sides changed.
        #[arg(long, value_enum)]
        prefer: Option<Side>,
    },
    /// Show which side changed since the last sync.
    Status,
    /// Lint every script and check the project, without writing anything.
    Check,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    let cwd = match cli.dir {
        Some(d) => d,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let project = || Project::find(&cwd);

    match cli.command {
        Cmd::Init {
            path,
            from,
            name,
            output,
        } => {
            let root = path.map(|p| cwd.join(p)).unwrap_or_else(|| cwd.clone());
            let name = name.unwrap_or_else(|| {
                root.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Vortex game")
                    .to_string()
            });
            let from = from.map(|f| cwd.join(f));
            let p = ops::init(&root, &name, from.as_deref(), output)?;
            println!("set up {} in {}", p.config.name, p.root.display());
            println!("  files:   {}/", p.config.source);
            println!("  project: {}", p.config.output);
            println!(
                "next: open {} in Vortex Studio and run `vortexsync serve`",
                p.config.output
            );
        }
        Cmd::Build { force, strict } => {
            let p = project()?;
            let built = ops::build(&p, force, strict)?;
            println!("built {} ({} instances)", p.config.output, built.instances);
            print_problems(&ops::describe(&built.findings));
            if built.studio_running {
                println!("Studio is running: reopen the project there to see the changes");
            }
        }
        Cmd::Extract { force } => {
            let p = project()?;
            let r = ops::extract(&p, force)?;
            if r.is_empty() {
                println!("files already match {}", p.config.output);
            } else {
                for f in &r.written {
                    println!("  wrote   {f}");
                }
                for f in &r.deleted {
                    println!("  deleted {f}");
                }
                println!(
                    "extracted {} ({} written, {} deleted)",
                    p.config.output,
                    r.written.len(),
                    r.deleted.len()
                );
            }
        }
        Cmd::Serve { prefer } => {
            let p = project()?;
            let prefer = prefer.map(|s| match s {
                Side::Files => Prefer::Files,
                Side::Studio => Prefer::Studio,
            });
            serve::serve(&p, prefer, |note| match note {
                Note::Watching => println!(
                    "watching {}/ and {}, Ctrl+C to stop",
                    p.config.source, p.config.output
                ),
                Note::Built {
                    instances,
                    problems,
                    studio_running,
                } => {
                    println!(
                        "> files changed, built {} ({instances} instances)",
                        p.config.output
                    );
                    print_problems(&problems);
                    if studio_running {
                        println!("  reopen the project in Studio to see it");
                    }
                }
                Note::Extracted { written, deleted } => {
                    println!(
                        "> Studio saved, updated the files ({written} written, {deleted} deleted)"
                    );
                }
                Note::Conflict => {
                    println!(
                        "> both the files and {} changed, leaving both alone.\n  \
                         Keep the files with `vortexsync build --force`, keep Studio's save with `vortexsync extract --force`,\n  \
                         or restart with --prefer files or --prefer studio",
                        p.config.output
                    );
                }
                Note::Failed(e) => println!("> {e}"),
            })?;
        }
        Cmd::Status => {
            let p = project()?;
            let msg = match ops::status(&p)? {
                Status::InSync => "in sync",
                Status::FilesChanged => "files changed, run `vortexsync build`",
                Status::StudioChanged => "Studio saved changes, run `vortexsync extract`",
                Status::Conflict => "both sides changed, pick one with --force on build or extract",
                Status::Unknown => "never synced, run `vortexsync build` or `vortexsync extract`",
            };
            println!("{msg}");
        }
        Cmd::Check => {
            let p = project()?;
            let findings = ops::check(&p)?;
            if findings.is_empty() {
                println!("no problems found");
            } else {
                println!("{}", ops::describe(&findings));
                let errors = findings
                    .iter()
                    .filter(|f| f.severity == vortexstudio_mcp::lint::Severity::Error)
                    .count();
                if errors > 0 {
                    return Ok(ExitCode::FAILURE);
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_problems(text: &str) {
    if !text.is_empty() {
        println!("{text}");
    }
}
