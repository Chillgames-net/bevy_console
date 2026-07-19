//! Rich command metadata, dynamic completion, and resource-backed console
//! properties while command handlers remain ordinary Bevy systems.
//!
//! Try:
//!   map forest
//!   res set debug.draw_colliders true
//!   res get debug.draw_colliders
//!
//! Run with: `cargo run --example advanced --features resource-properties`

use bevy::prelude::*;
use chill_bevy_console::{
    ArgumentSpec, BuiltinCommand, ChillConsole, CommandArgs, ConsoleAppExt, ConsoleCommand,
    ConsoleCompletionRequest, ConsoleResource,
};

#[derive(Resource)]
struct MapCatalog(Vec<&'static str>);

#[derive(Resource, ConsoleResource)]
#[console_resource(prefix = "debug")]
struct DebugSettings {
    #[console(help = "Draw collider shapes")]
    draw_colliders: bool,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(MapCatalog(vec!["forest", "fortress", "test_chamber"]))
        .insert_resource(DebugSettings {
            draw_colliders: false,
        })
        .add_plugins(ChillConsole::default().with_builtin_commands([BuiltinCommand::Res]))
        .add_console_resource::<DebugSettings>()
        .add_console_command(
            ConsoleCommand::new("map", "map <name> - load a map", load_map)
                .with_summary("Load a map by name")
                .with_alias("changelevel")
                .with_args([ArgumentSpec::new("name").help("Map asset name")])
                .with_completions(complete_maps),
        )
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn load_map(In(args): CommandArgs, maps: Res<MapCatalog>) -> String {
    let Some(name) = args.get(0) else {
        return "Usage: map <name>".into();
    };
    if maps.0.contains(&name) {
        format!("Loading {name}...")
    } else {
        format!("Unknown map: {name}")
    }
}

fn complete_maps(In(request): ConsoleCompletionRequest, maps: Res<MapCatalog>) -> Vec<String> {
    match request.argument_index() {
        0 => maps.0.iter().copied().map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}
