# Contributing

Thanks for helping. Bug reports, ideas for the file layout and pull requests are all welcome.

## Reporting problems

Open an issue with the command you ran, its full output and your versions (`vortexsync --version`, Studio version, OS). If a project fails to build or extract, include the file that broke or the `.vrtx` if you can share it.

Anything that loses data is the most important kind of bug, please say so in the title.

## Development

You need Rust 1.89 or newer.

```sh
cargo build
cargo test
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

CI runs the last three on Linux, Windows and macOS. The `serve` tests drive the real file watcher, so they take a moment.

The `.vrtx` format, the scene model and the linter come from [VortexStudio-MCP](https://github.com/xRookieFight/VortexStudio-MCP), pinned to a tag in `Cargo.toml`. Format fixes belong there.

## Layout

| File | Purpose |
| --- | --- |
| `src/layout.rs` | Project to files and back, the heart of it |
| `src/names.rs` | Instance names to file names and back |
| `src/disk.rs` | Reading and writing the source folder |
| `src/state.rs` | What both sides looked like at the last sync |
| `src/ops.rs` | The commands and their safety rules |
| `src/serve.rs` | Watching both sides |
| `src/config.rs` | `vortex.project.json` |
| `src/main.rs` | Command line |

## Rules that keep people's work safe

* `layout::extract` followed by `layout::build` must keep the scene, and extracting again must give byte identical files. There are tests for both, extend them when you add fields.
* Never overwrite a side that changed since the last sync without `--force`.
* Only delete files VortexSync owns (`.luau`, `.lua`, `.json`), never anything else in the source folder.
* Error messages say what to do next, including which command to run.

## Commits and releases

Short imperative titles in sentence case, no trailing period, for example `Keep sibling order in a meta file`.

To release, bump the version in `Cargo.toml` and push a matching tag like `v1.1.0`. The release workflow builds Linux, Windows and macOS binaries.
