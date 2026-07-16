# chill_bevy_console

[![Crates.io](https://img.shields.io/crates/v/chill_bevy_console)](https://crates.io/crates/chill_bevy_console)

A configurable developer console plugin for [Bevy](https://bevyengine.org) games.

Press `` ` `` (backtick) to toggle the console open and closed.

## Version

| `chill_bevy_console` | `bevy` |
|---------------------------|--------|
| `0.3`                     | `0.19` |
| `0.2`                     | `0.19` |
| `0.1`                     | `0.18` |

## Install

```toml
[dependencies]
chill_bevy_console = "0.3"
```

Optional features:

- `embedded-font` — embed `UbuntuMono-R.ttf` so no font asset is required.
- `persistent-history` — save and restore command recall history between
  runs.
- `resource-properties` — expose selected fields on Bevy resources through
  `get` and `res` with `#[derive(ConsoleResource)]`.

## Usage

```rust
use bevy::prelude::*;
use chill_bevy_console::{ChillConsole, ConsoleAppExt, CommandArgs, console_closed};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default())
        .add_console_command("say", "say <text> - echo text", say_cmd)
        .add_systems(Update, gameplay_input.run_if(console_closed))
        .run();
}

fn say_cmd(In(args): CommandArgs) -> String {
    args.join(" ")
}

fn gameplay_input() { /* movement, jump, etc. */ }
```

## `ConsoleAppExt` methods

Import `ConsoleAppExt` to add these methods to Bevy's `App`. Each returns
`&mut App`, so calls can be chained.

| Method | Purpose |
|--------|---------|
| `add_console_command(name, usage, system)` | Register a command whose system receives `CommandArgs` and returns a `String` or `ConsoleResult`. |
| `add_console_command_spec(spec, system)` | Register a `CommandSpec` with aliases, argument metadata, and structured help; its system returns a `String` or `ConsoleResult`. |
| `add_console_completer(command, argument_index, completer)` | Attach a dynamic argument completer to a previously registered command. |
| `add_console_resource::<R>()` | Register `R`'s `ConsoleResource` properties for the built-in `get` and `res` commands. Requires the `resource-properties` feature. |

See [USAGE.md](USAGE.md) for adding commands, custom config, persisting history, blocking gameplay input, and built-in commands. Runnable examples live in [`examples/`](examples) — try `cargo run --example basic`.

For resource-backed properties, rich command metadata, and a dynamic argument
completer, run `cargo run --example advanced --features resource-properties`.

For output written by ordinary game systems, run
`cargo run --example system_output`.

Commands stay simple by default. When a command needs richer help or argument
completion, register a `CommandSpec`; its executor is still an ordinary Bevy
system:

```rust
app.add_console_command_spec(
    CommandSpec::new("map")
        .help("map <name> - load a map")
        .args([ArgumentSpec::new("name")]),
    load_map,
)
.add_console_completer("map", 0, complete_map_names);
```

## License

MIT OR Apache-2.0
