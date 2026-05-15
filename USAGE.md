# Usage

## Adding commands

Commands are plain Bevy systems that receive `CommandArgs` (`In<Args>`) and return a `String`:

```rust
use chill_bevy_console::CommandArgs;

fn say_cmd(In(args): CommandArgs) -> String {
    args.join(" ")
}

app.add_console_command("say", "say <text> — echo text to the console", say_cmd);
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

## Custom config

Every visual element is configurable via `ConsoleConfig`:

```rust
.add_plugins(ChillConsole {
    config: ConsoleConfig {
        font_path: Some("fonts/MyFont.ttf".into()),
        input_border_color: Color::srgb(0.2, 0.8, 0.4),
        toggle_key: KeyCode::F1,
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

## Embedded font

By default, `ConsoleConfig::font_path = None` falls back to Bevy's built-in
font. If you'd rather not ship a font asset, enable the `embedded-font` cargo
feature — `UbuntuMono-R.ttf` is compiled into the binary and used automatically.

```toml
chill_bevy_console = { version = "0.1", features = ["embedded-font"] }
```

## Persisting console history between runs

Enable the `persistent-history` cargo feature and the entire console display
(commands and their outputs) is loaded at startup and saved whenever the
history changes — no extra Rust config needed. Up/Down recall is rebuilt from
the loaded `> command` echo lines.

```toml
chill_bevy_console = { version = "0.1", features = ["persistent-history"] }
```

By default the history is written to `console_history.txt` in the current
working directory. Override the path or disable persistence per-app via
`ConsoleConfig`:

```rust
.add_plugins(ChillConsole {
    config: ConsoleConfig {
        history_file: Some("/tmp/my_game_history.txt".into()),
        ..default()
    },
})
```

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
| `clear`   | Clear the console history       |
