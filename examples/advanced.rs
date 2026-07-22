//! Rich command metadata, dynamic completion, and resource-backed console
//! properties while command handlers remain ordinary Bevy systems.
//!
//! Try:
//!   map <Tab>
//!   map "test chamber"
//!   map missing
//!   res set debug.draw_colliders true
//!   res get debug.draw_colliders
//!
//! Run with: `cargo run --example advanced`

use bevy::prelude::*;
use chill_bevy_console::{
    ArgumentSpec, BuiltinCommand, ChillConsole, CommandArgs, CompletionItem, ConsoleAppExt,
    ConsoleCommand, ConsoleCompletionRequest, ConsoleLevel, ConsoleResult,
};

#[derive(Resource)]
struct MapCatalog(Vec<MapInfo>);

struct MapInfo {
    name: &'static str,
    description: &'static str,
    experimental: bool,
}

#[derive(Resource, Reflect)]
#[reflect(Resource)]
struct DebugSettings {
    /// Draw collider shapes
    draw_colliders: bool,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(MapCatalog(vec![
            MapInfo {
                name: "forest",
                description: "Outdoor tutorial map",
                experimental: false,
            },
            MapInfo {
                name: "fortress",
                description: "Large combat sandbox",
                experimental: false,
            },
            MapInfo {
                name: "test chamber",
                description: "Experimental mechanics lab",
                experimental: true,
            },
        ]))
        .insert_resource(DebugSettings {
            draw_colliders: false,
        })
        .add_plugins(ChillConsole::default().with_builtin_commands([BuiltinCommand::Res]))
        .add_console_resource::<DebugSettings>("debug")
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

fn load_map(In(args): CommandArgs, maps: Res<MapCatalog>) -> ConsoleResult {
    let Some(name) = args.get(0) else {
        return ConsoleResult::error("Usage: map <name>");
    };

    let Some(map) = maps.0.iter().find(|map| map.name == name) else {
        return ConsoleResult::error(format!("Unknown map: {name}"));
    };

    let result = ConsoleResult::info(format!("Loading {}...", map.name));
    if map.experimental {
        result.line(
            ConsoleLevel::Warn,
            "This map is experimental and may be unstable",
        )
    } else {
        result
    }
}

fn complete_maps(
    In(request): ConsoleCompletionRequest,
    maps: Res<MapCatalog>,
) -> Vec<CompletionItem> {
    match request.argument_index() {
        0 => maps
            .0
            .iter()
            .map(|map| CompletionItem::new(map.name, map.description).append_space(false))
            .collect(),
        _ => Vec::new(),
    }
}
