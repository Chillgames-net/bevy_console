# Usage

## Adding commands

Commands are plain Bevy systems that receive `CommandArgs` (`In<Args>`) and return a `String` or `ConsoleResult`. `String` is converted to an info-level `ConsoleResult` automatically:

```rust
use chill_bevy_console::CommandArgs;

fn say_cmd(In(args): CommandArgs) -> String {
    args.join(" ")
}

app.add_console_command("say", "say <text> - echo text to the console", say_cmd);
```

`Args` dereferences to `[String]` (giving you `join`, `len`, `iter`, …) and adds three helpers:

| Method | Returns | Description |
|---|---|---|
| `args.get(i)` | `Option<&str>` | Argument at index `i` |
| `args.parse::<T>(i)` | `Option<T>` | Argument at index `i` parsed as `T`, `None` if missing or invalid |
| `args.rest(i)` | `String` | All arguments from index `i` joined with spaces |

```rust
fn teleport_cmd(In(args): CommandArgs) -> String {
    let (Some(x), Some(y)) = (args.parse::<f32>(0), args.parse::<f32>(1)) else {
        return "Usage: teleport <x> <y>".to_string();
    };
    format!("Teleporting to ({x}, {y})")
}
```

Because they're normal Bevy systems, commands can take any system params:

```rust
fn goto_level_cmd(In(args): CommandArgs, mut level: ResMut<LevelManager>) -> String {
    let Some(index) = args.parse::<usize>(0) else {
        return "Usage: goto_level <index>".to_string();
    };
    level.set(index);
    format!("Jumped to level {index}")
}
```

## Rich commands and argument completion

Keep `add_console_command` for the common case. When a command needs aliases,
structured help or completion metadata, use
`CommandSpec`; its handler is still a normal Bevy system:

```rust
use chill_bevy_console::{ArgumentSpec, CommandSpec, ConsoleAppExt};

app.add_console_command_spec(
    CommandSpec::new("map")
        .help("map <name> - load a map")
        .summary("Load a map by name")
        .alias("changelevel")
        .args([ArgumentSpec::new("name").help("Map asset name")]),
    load_map,
);
```

For dynamic values—entities, loaded assets, saved games, or map names—attach a
normal Bevy system as a completer. It receives the parsed command and active
argument, and can query game resources as usual:

```rust
use bevy::prelude::*;
use chill_bevy_console::{CompletionItem, CompletionRequest, ConsoleAppExt};

fn complete_maps(
    In(request): In<CompletionRequest>,
    maps: Res<MapCatalog>,
) -> Vec<CompletionItem> {
    maps.names()
        .map(|name| CompletionItem::new(name, request.parsed.replacement_range()))
        .collect()
}

app.add_console_completer("map", 0, complete_maps);
```

Quoted arguments and backslash escapes are parsed before execution, so commands
receive `map "my test map"` as one argument. Tab inserts the selected command
or argument candidate; Up/Down selects candidates or command history.

## Resource properties

Enable the opt-in `resource-properties` feature to expose selected fields on a
Bevy `Resource` directly through the `res` command:

```toml
chill_bevy_console = { version = "0.3", features = ["resource-properties"] }
```

```rust
use bevy::prelude::*;
use chill_bevy_console::{ConsoleAppExt, ConsoleResource};

#[derive(Resource, ConsoleResource)]
#[console_resource(prefix = "debug")]
struct DebugSettings {
    #[console(help = "Draw collider shapes")]
    draw_colliders: bool,

    #[console(readonly)]
    build_label: String,

    #[console(help = "Maximum frame rate")]
    max_fps: u32,
}

app.insert_resource(DebugSettings {
    draw_colliders: false,
    build_label: "development".into(),
    max_fps: 60,
})
.add_console_resource::<DebugSettings>();
```

This creates `debug.draw_colliders`, `debug.build_label`, and `debug.max_fps`.
`res set debug.draw_colliders true` mutates `DebugSettings` itself, so ordinary
Bevy change detection observes it. `readonly` omits the setter.

`res` groups resource operations: `res get <name>`, `res set <name> <value>`,
`res add <name> <amount>`, `res sub <name> <amount>`, and `res toggle <name>`.
`add` and `sub` work with numeric properties; integer overflow is reported as a
console error.

Fields must be explicitly marked with `#[console]`. Built-in property values
support booleans, all primitive integers and floats, and `String`; implement
`ConsolePropertyValue` for an application-specific type.

The derive macro currently expects the dependencies to be available as
`chill_bevy_console` and `bevy`; renamed dependencies are not supported.

## Runtime aliases

The console includes Source-style runtime aliases:

```text
alias set quicksave save slot_1
quicksave
alias remove quicksave
```

`help <command>` shows detailed help for a registered command.

## Programmatic commands and output

Game systems can queue commands without simulating keyboard input:

```rust
fn run_startup_command(mut commands: MessageWriter<ConsoleRequest>) {
    commands.write(ConsoleRequest::new("res set debug.draw_colliders true"));
}
```

For log or game output, write `ConsoleLineMessage` values. Use its convenience
constructor for ordinary output, or fill in the level and source when they
matter:

```rust
fn report_loaded(mut console: MessageWriter<ConsoleLineMessage>) {
    console.write(ConsoleLineMessage::info("Map loaded"));
    console.write(ConsoleLineMessage {
        level: ConsoleLevel::Warn,
        source: ConsoleLineSource::System,
        text: "One optional asset is missing".into(),
    });
}
```

See `cargo run --example system_output` for a runnable version. Advanced
command handlers can return `ConsoleResult` for per-line severity levels.

## Capturing Bevy logs

To mirror `tracing`/Bevy logs into the console, install the supplied layer when
configuring Bevy's `LogPlugin`:

```rust
use bevy::log::LogPlugin;
use chill_bevy_console::console_log_layer;

app.add_plugins(DefaultPlugins.set(LogPlugin {
    custom_layer: console_log_layer,
    ..default()
}));
```

## Custom config

Every visual element is configurable via `ConsoleConfig`:

```rust
.add_plugins(ChillConsole {
    config: ConsoleConfig {
        font_path: Some("fonts/MyFont.ttf".into()),
        input_border_color: Color::srgb(0.2, 0.8, 0.4),
        toggle_key: KeyCode::F1,
        z_index: 1000,
        ..default()
    },
})
```

### Built-in presets

A handful of ready-made themes ship as `ConsoleConfig` constructors. Use one
directly, or as a starting point with struct-update syntax:

```rust
.add_plugins(ChillConsole {
    config: ConsoleConfig {
        toggle_key: KeyCode::F1,
        ..ConsoleConfig::chillgames() // also: matrix(), source(), simple()
    },
})
```

### Selecting built-in commands

All built-in commands are enabled by default. To expose only the commands your
game needs, set `ConsoleConfig::builtin_commands` to a set of
`BuiltinCommand` values:

```rust
use chill_bevy_console::{BuiltinCommand, ConsoleConfig};

let config = ConsoleConfig {
    builtin_commands: [BuiltinCommand::Help, BuiltinCommand::Alias]
        .into_iter()
        .collect(),
    ..default()
};
```

## Embedded font

By default, `ConsoleConfig::font_path = None` falls back to Bevy's built-in
font. If you'd rather not ship a font asset, enable the `embedded-font` cargo
feature — `UbuntuMono-R.ttf` is compiled into the binary and used automatically.

```toml
chill_bevy_console = { version = "0.3", features = ["embedded-font"] }
```

## Persisting command recall between runs

Enable the `persistent-history` cargo feature to load and save submitted
commands for Up/Down recall. Console display output, resources, aliases, and
other runtime console state are never persisted by this crate.

```toml
chill_bevy_console = { version = "0.3", features = ["persistent-history"] }
```

By default command recall is written to `console_history.txt` in the current
working directory. The file is plain text, with one command per line, and is
rewritten synchronously whenever recall history changes. Override the path or
disable persistence per-app via
`ConsoleConfig`:

```rust
.add_plugins(ChillConsole {
    config: ConsoleConfig {
        history_file: Some("/tmp/my_game_history.txt".into()),
        ..default()
    },
})
```

Persistence is disabled on WebAssembly targets.

## Blocking gameplay input

Use the `console_closed` run condition to suppress input systems while the console is open:

```rust
app.add_systems(Update, handle_movement.run_if(console_closed));
```

## Disabling the console at runtime

Set `ConsoleState::enabled = false` to disable the toggle key and force-close
the console if it's currently open. Handy for release builds, cutscenes, or
menu screens:

```rust
fn lock_console_for_release(mut state: ResMut<ConsoleState>) {
    state.enabled = false;
}
```

## Built-in commands

| Command   | Description                     |
|-----------|---------------------------------|
| `help`    | List all registered commands    |
| `clear`   | Clear the console output        |
| `alias` | Manage runtime aliases |
| `bind` | Manage runtime key bindings |
| `res` | Inspect and change resource properties (requires `resource-properties`) |

`alias` and `bind` use `list`, `get`, `set`, and `remove` operations:

```text
alias set quicksave save slot_1
alias get quicksave
alias remove quicksave
bind set F1 say quicksave
bind set shift+w res toggle show_wireframes
bind get shift+w
bind remove ctrl+w
```

Bindings run while the console is closed. Use `ctrl+`, `shift+`, and `alt+`
prefixes in any combination. Use Bevy `KeyCode` names (such as `KeyW` or `F1`);
single letters and digits are accepted as shorthand.

See [`examples/bindings.rs`](examples/bindings.rs) for a runnable setup that
selects built-in commands, registers application commands, and adds bindings
through both console commands and the `ConsoleBinds` resource.
