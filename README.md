# VortexSync

Keep a [Vortex](https://playvortex.io) Studio project in git. VortexSync turns a `.vrtx` project into a folder of readable files (scripts as `.luau`, everything else as small JSON files) and back, and can watch both sides so that what you write in your editor reaches Studio and what you build in Studio reaches your files.

Think [Rojo](https://rojo.space), for Vortex.

> Unofficial community project, not affiliated with or endorsed by Vortex.

## Why

* Write scripts in VS Code, Neovim or anything else, with your own formatter, linter and AI tools.
* Review changes as diffs, branch, merge and roll back like any other code.
* Work on one game with other people without passing `.vrtx` files around.

## How it differs from Rojo

Rojo talks to a plugin inside Roblox Studio and updates the open place live. Vortex Studio has no plugin system, so VortexSync works through the `.vrtx` file itself:

* when you **save in Studio**, your files update within a second
* when you **edit files**, the `.vrtx` is rebuilt right away, and you **reopen the project in Studio** to see it

If both sides change before a sync, VortexSync stops and asks which one wins instead of guessing.

## Install

Download the binary for your system from [Releases](https://github.com/xRookieFight/VortexSync/releases) and put it on your PATH, or build it with Rust 1.89+:

```sh
cargo install --git https://github.com/xRookieFight/VortexSync
```

## Quick start

From an existing Studio project:

```sh
mkdir obby && cd obby
vortexsync init --from ~/Games/obby.vrtx
git init && git add . && git commit -m "Start tracking the obby"
vortexsync serve
```

Or from scratch, which creates `game.vrtx` from Studio's empty template:

```sh
vortexsync init my-game
cd my-game
vortexsync serve
```

Open the `.vrtx` in Vortex Studio and keep `vortexsync serve` running while you work.

## The files

```
my-game/
├─ vortex.project.json
├─ game.vrtx                     built from src/, ignored by git
└─ src/
   ├─ Lighting.json
   ├─ Workspace/
   │  ├─ Baseplate.part.json
   │  ├─ Lava.part.json
   │  └─ Tower/                  a Model with children
   │     ├─ _model.json
   │     ├─ Floor1.part.json
   │     └─ Floor2.part.json
   ├─ ReplicatedStorage/
   │  ├─ Shoot.remoteevent.json
   │  └─ Util.luau               ModuleScript
   ├─ ServerScriptService/
   │  └─ Main.server.luau        Script
   └─ StarterPlayerScripts/
      └─ Input.client.luau       LocalScript
```

| On disk | In Studio |
| --- | --- |
| folder at the top of `src/` | a service |
| `Name.server.luau` | Script |
| `Name.client.luau` | LocalScript |
| `Name.luau` | ModuleScript |
| `Name.meta.json` next to a script | extra settings for it, like `"enabled": false` |
| `Name.<class>.json` | any other instance, e.g. `Lava.part.json`, `Shoot.remoteevent.json` |
| `Name/` folder | an instance with children. Its own data goes in `_part.json`, `_model.json`, ... or `init.server.luau` (and friends) for scripts. A folder with neither becomes a Model |
| `Lighting.json` | lighting settings |

A part file only lists what differs from a freshly inserted part, plus its position and size:

```json
{
  "position": [0, 1, 10],
  "size": [8, 1, 8],
  "rotation": [0, 45, 0],
  "color": "#FF3300",
  "transparency": 0.25,
  "material": "Metal",
  "shape": "Block",
  "anchored": false,
  "attributes": { "Damage": 25, "Team": "red" }
}
```

Other fields: `can_collide`, `cast_shadow`, `spawn_location`, `truss`, `velocity`, `angular_velocity`, `textures` (`[{ "face": "Top", "kind": "Studs" }]`) and `point_light` / `spot_light` (`{ "color": "#FFEE88", "brightness": 3, "range": 16 }`, spot lights add `angle` and `face`). Materials are SmoothPlastic, Plastic, Wood, Metal, Grass, Ice and Paint; shapes are Block, Wedge, CornerWedge, Cylinder and Ball. Rotation is in degrees, applied X then Y then Z.

Files VortexSync doesn't recognize, like a `README.md` inside `src/`, are left alone.

### Names

Names that aren't valid file names are percent encoded (`a/b` becomes `a%2Fb`). Siblings with the same name get `~2`, `~3` suffixes and keep their real name in the file's `"name"` field.

## Commands

| Command | |
| --- | --- |
| `vortexsync init [dir] [--from game.vrtx]` | set up a project, empty or from a Studio project |
| `vortexsync serve [--prefer files\|studio]` | watch and sync both ways until Ctrl+C |
| `vortexsync build [--force] [--strict]` | files to `.vrtx` |
| `vortexsync extract [--force]` | `.vrtx` to files |
| `vortexsync status` | which side changed since the last sync |
| `vortexsync check` | lint every script and check the project without writing anything |

`build` and `extract` refuse to overwrite changes on the other side that haven't been synced yet. `--force` overrides that. `build` lints scripts with the [VortexStudio-MCP](https://github.com/xRookieFight/VortexStudio-MCP) linter and prints what it finds; `--strict` makes warnings fatal, handy in CI.

Every build backs up the previous `.vrtx` into `.vrtx-backups/` (the last 20 are kept).

## Working with Studio

1. Run `vortexsync serve` and open the `.vrtx` in Studio.
2. Build in Studio and save. The files follow.
3. Edit files. The `.vrtx` follows; reopen it in Studio before you continue there.
4. Commit whenever you like.

If you edit files and then save from a Studio window that still shows the old version, VortexSync sees two changes and stops. Pick the side to keep with `vortexsync build --force` (your files) or `vortexsync extract --force` (Studio's save).

## Good to know

* The order of siblings in Studio's explorer isn't kept, a build sorts them by name.
* Rotations are stored in degrees rounded to four decimals, so a rotated part can move by a tiny fraction of a degree on its first round trip. After that the files are stable.
* PointLight, SpotLight, Folder, value objects and body movers survive a round trip, but VortexStudio-MCP can't create them yet. See its [format notes](https://github.com/xRookieFight/VortexStudio-MCP/blob/main/docs/FORMAT.md).

## Related

* [VortexStudio-MCP](https://github.com/xRookieFight/VortexStudio-MCP), which VortexSync uses for the `.vrtx` format and linting, lets AI assistants work on your project.
* [Vortex API docs](https://xrookiefight.github.io/VortexAPI-Docs/)

## License

[MIT](LICENSE). Vortex and Vortex Studio are proprietary software owned by their developers.
