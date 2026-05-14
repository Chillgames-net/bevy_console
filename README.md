# chill_bevy_console

[![Crates.io](https://img.shields.io/crates/v/chill_bevy_console)](https://crates.io/crates/chill_bevy_console)

A configurable developer console plugin for [Bevy](https://bevyengine.org) games.

Press `` ` `` (backtick) to toggle the console open and closed.

## Version

| `chill_bevy_console` | `bevy` |
|---------------------------|--------|
| `0.1`                     | `0.18` |

## Install

```toml
[dependencies]
chill_bevy_console = "0.1"
```

Optional features:

- `embedded-font` — embed `UbuntuMono-R.ttf` so no font asset is required.
- `persistent-history` — save and restore console history between runs.

## Usage

```rust
use bevy::prelude::*;
use chill_bevy_console::{ChillConsole, ConsoleAppExt, CommandArgs, console_closed};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default())
        .add_console_command("say", "say <text> — echo text", say_cmd)
        .add_systems(Update, gameplay_input.run_if(console_closed))
        .run();
}

fn say_cmd(In(args): CommandArgs) -> String {
    args.join(" ")
}

fn gameplay_input() { /* movement, jump, etc. */ }
```

See [USAGE.md](USAGE.md) for adding commands, custom config, persisting history, blocking gameplay input, and built-in commands. Runnable examples live in [`examples/`](examples) — try `cargo run --example basic`.

## License

MIT OR Apache-2.0
