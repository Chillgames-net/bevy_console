# bevy_chillgames_console

A configurable developer console plugin for [Bevy](https://bevyengine.org) games.

Press `` ` `` (backtick) to toggle the console open and closed.

## Version

| `bevy_chillgames_console` | `bevy` |
|---------------------------|--------|
| `0.1`                     | `0.18` |

## Usage

```rust
use bevy_chillgames_console::{ChillgamesConsole, ConsoleCommand, ConsoleAppExt, console_closed};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillgamesConsole::default())
        .add_console_command::<SayCommand>()
        .add_systems(Update, gameplay_input.run_if(console_closed))
        .run();
}
```

### Adding commands

```rust
pub struct SayCommand;

impl ConsoleCommand for SayCommand {
    const NAME: &'static str = "say";
    const USAGE: &'static str = "say <text> — echo text to the console";

    fn run(args: &[&str], _world: &mut World) -> String {
        args.join(" ")
    }
}
```

### Custom config

Every visual element is configurable via `ConsoleConfig`:

```rust
.add_plugins(ChillgamesConsolePlugin {
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
| `version` | Print the plugin version        |

## License

MIT OR Apache-2.0
