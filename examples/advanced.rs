//! Rich command metadata, dynamic completion, and resource-backed console
//! properties while command handlers remain ordinary Bevy systems.
//!
//! Try:
//!   map forest
//!   res set debug.draw_colliders true
//!   get debug.draw_colliders
//!
//! Run with: `cargo run --example advanced --features resource-properties`

use bevy::prelude::*;
use chill_bevy_console::{
    ArgumentSpec, ChillConsole, CommandArgs, CommandSpec, CompletionItem, CompletionRequest,
    ConsoleAppExt, ConsoleResource,
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
        .init_resource::<MapCatalog>()
        .insert_resource(DebugSettings {
            draw_colliders: false,
        })
        .add_plugins(ChillConsole::default())
        .add_console_resource::<DebugSettings>()
        .add_console_command_spec(
            CommandSpec::new("map")
                .help("map <name> - load a map")
                .summary("Load a map by name")
                .alias("changelevel")
                .args([ArgumentSpec::new("name").help("Map asset name")]),
            load_map,
        )
        .add_console_completer("map", 0, complete_maps)
        .add_systems(Startup, setup)
        .run();
}

impl Default for MapCatalog {
    fn default() -> Self {
        Self(vec!["forest", "fortress", "test_chamber"])
    }
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

fn complete_maps(In(request): In<CompletionRequest>, maps: Res<MapCatalog>) -> Vec<CompletionItem> {
    maps.0
        .iter()
        .map(|name| CompletionItem::new(*name, request.parsed.replacement_range()))
        .collect()
}
