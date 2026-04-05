# chill_bevy_console

[![Crates.io](https://img.shields.io/crates/v/chill_bevy_console)](https://crates.io/crates/chill_bevy_console)

A configurable developer console plugin for [Bevy](https://bevyengine.org) games.

Press `` ` `` (backtick) to toggle the console open and closed.

## Version

| `chill_bevy_console` | `bevy` |
|---------------------------|--------|
| `0.1`                     | `0.18` |

## Usage

```rust
use chill_bevy_console::{ChillConsole, ConsoleAppExt, CommandArgs, console_closed};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default())
        .add_console_command("say", "say <text> — echo text", say_cmd)
        .add_systems(Update, gameplay_input.run_if(console_closed))
        .run();
}
```

### Adding commands

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

### Custom config

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

### Blocking gameplay input

Use the `console_closed` run condition to suppress input systems while the console is open:

```rust
app.add_systems(Update, handle_movement.run_if(console_closed));
```

## Built-in commands

| Command   | Description                     |
|-----------|---------------------------------|
| `help`    | List all registered commands    |
| `clear`   | Clear the console history       |

## License

MIT OR Apache-2.0
