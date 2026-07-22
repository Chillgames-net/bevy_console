# Usage

## Adding commands

Commands are plain Bevy systems that receive `CommandArgs` (`In<Args>`) and return a `String` or `ConsoleResult`. `String` is converted to an info-level `ConsoleResult` automatically:

```rust
use bevy::prelude::*;
use chill_bevy_console::{CommandArgs, ConsoleAppExt, ConsoleCommand};

fn say_cmd(In(args): CommandArgs) -> String {
    args.join(" ")
}

app.add_console_command(
    ConsoleCommand::new("say", "say <text> - echo text to the console", say_cmd),
);
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

Use `ConsoleCommand` for every command; it scales from the simplest command to
aliases, structured help, and dynamic completion:

```rust
use chill_bevy_console::{ArgumentSpec, ConsoleAppExt, ConsoleCommand};

app.add_console_command(
    ConsoleCommand::new("map", "map <name> - load a map", load_map)
        .with_summary("Load a map by name")
        .with_alias("changelevel")
        .with_args([ArgumentSpec::new("name").help("Map asset name")])
        .with_completions(complete_maps),
);
```

For dynamic values—entities, loaded assets, saved games, or map names—attach a
normal Bevy system as a completer. It runs for every argument of that command;
use `request.argument_index()` to select behavior and `request.argument(i)` to
inspect preceding values. It can query game resources as usual:

```rust
use bevy::prelude::*;
use chill_bevy_console::ConsoleCompletionRequest;

fn complete_maps(
    In(request): ConsoleCompletionRequest,
    maps: Res<MapCatalog>,
) -> Vec<String> {
    match request.argument_index() {
        0 => maps.names().map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}
```

When a completer returns no candidates, completion falls back to the boolean or
choice suggestions declared with `ArgumentSpec` for that argument position.

Quoted arguments and backslash escapes are parsed before execution, so commands
receive `map "my test map"` as one argument. Tab inserts the selected command
or argument candidate; Up/Down selects candidates or command history. When
there are more candidates than `ConsoleConfig::max_suggestions`, navigation
continues through additional suggestion pages.

## Console controls

Press Enter to run the current command. Tab accepts the selected completion,
and Up/Down navigate completions or command history. On touch devices, tap a
completion to accept it; swipe up with two fingers to dismiss the console.

## Resource properties

Register a reflected Bevy `Resource` to expose its supported fields directly
through the `res` command:

```rust
use bevy::prelude::*;
use chill_bevy_console::{ConsoleAppExt, ConsoleProperty};

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct DebugSettings {
    /// Draw collider shapes
    draw_colliders: bool,

    /// Build identifier
    #[reflect(@ConsoleProperty::readonly())]
    build_label: String,

    /// Maximum frame rate
    max_fps: u32,
}

app.insert_resource(DebugSettings {
    draw_colliders: false,
    build_label: "development".into(),
    max_fps: 60,
})
.add_console_resource::<DebugSettings>();
```

This creates `DebugSettings.draw_colliders`, `DebugSettings.build_label`, and
`DebugSettings.max_fps`. `res set DebugSettings.draw_colliders true` mutates
`DebugSettings` itself, so ordinary Bevy change detection observes it.
`readonly` rejects mutating operations. If two registered resources have the
same short type path, use their full reflected type paths to disambiguate them.

`res` groups resource operations: `res get <name>`, `res set <name> <value>`,
`res add <name> <amount>`, `res sub <name> <amount>`, and `res toggle <name>`.
`add` and `sub` work with numeric properties; integer overflow is reported as a
console error.

All reflected fields with a registered value adapter are exposed automatically.
Field documentation comments provide completion and `res get` help. Use a
reflected `ConsoleProperty` attribute only for read-only, name, or help
overrides. Built-in adapters support booleans, all primitive integers and
floats, and `String`.

For an application-specific reflected field type, implement
`ConsolePropertyValue` and call
`register_console_property_value::<T>()` before `add_console_resource`.

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
    commands.write(ConsoleRequest::new(
        "res set DebugSettings.draw_colliders true",
    ));
}
```

To prefill the editable input without executing it, use `ConsoleState::set_input`;
`ConsoleState::input` returns the current value:

```rust
use bevy::prelude::*;
use chill_bevy_console::ConsoleState;

fn prefill_console(mut state: ResMut<ConsoleState>) {
    state.set_input("help");
    assert_eq!(state.input(), "help");
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
    ..default()
})
```

### Built-in presets

A handful of ready-made themes ship as `ConsoleConfig` constructors. Use one
directly, or as a starting point with struct-update syntax:

```rust
.add_plugins(ChillConsole {
    config: ConsoleConfig {
        toggle_key: KeyCode::F1,
        ..ConsoleConfig::chillgames() // also: matrix(), source()
    },
    ..default()
})
```

### Selecting built-in commands

By default, only `help` and `clear` are enabled. Use
`BuiltinCommand::all()` to enable every built-in command. To choose a custom
set, pass it to `with_builtin_commands`:

```rust
use chill_bevy_console::{BuiltinCommand, ChillConsole, ConsoleConfig};

let plugin = ChillConsole::default()
    .with_builtin_commands(BuiltinCommand::all());

let plugin = ChillConsole::default()
    .with_builtin_commands([BuiltinCommand::Help, BuiltinCommand::Alias]);

// Empty-submit closing is behavior rather than command registration.
let plugin = ChillConsole {
    config: ConsoleConfig {
        close_on_empty_submit: true,
        ..default()
    },
    ..default()
};
```

## Embedded font

By default, `ConsoleConfig::font_path = None` falls back to Bevy's built-in
font. Enable `embedded-font` to use the bundled Ubuntu Mono font instead;
`UbuntuMono-R.ttf` is compiled into the binary and selected automatically.

```toml
chill_bevy_console = { version = "0.4", features = ["embedded-font"] }
```

## Persisting console transcripts between runs

Enable the `persistent-history` cargo feature to load and save a console
transcript for Up/Down recall. The transcript restores submitted command prompts
and console output in their original order; commands are not executed again.

```toml
chill_bevy_console = { version = "0.4", features = ["persistent-history"] }
```

By default, console history is saved to `console_history.txt` in the current
working directory. Override the save path and amount of history to keep with
`ConsolePersistence`. The in-game output and command-recall limits remain core
console settings on `ConsoleConfig`:

```rust
.add_plugins(ChillConsole {
    persistence: ConsolePersistence {
        history_file: "/tmp/my_game_history.txt".into(),
        max_saved_lines: 1_000,
        max_line_length: Some(2_000),
        // recall_only: true,
        ..default()
    },
    config: ConsoleConfig {
        max_history_lines: 5_000,
        max_command_history: 1_000,
        ..default()
    },
    ..default()
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
| `state` | Inspect or change registered Bevy states |
| `res` | Inspect and change registered reflected resource properties |

### Reflected states

The `state` command discovers states registered with Bevy reflection. Derive
`Reflect` for each freely mutable state, then register it after initialization:

```rust
#[derive(States, Reflect, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum GameState {
    #[default]
    Menu,
    Playing,
}

app.init_state::<GameState>()
    .add_console_state::<GameState>();
```

Use the enum name and a unit enum variant. If two registered states share an
enum name, completion uses their full reflected type paths to disambiguate:

```text
state get GameState
state set GameState Playing
```

Type and variant completion are provided automatically. Variants with fields
are intentionally not settable through the command.

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
