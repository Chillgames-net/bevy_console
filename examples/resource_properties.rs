//! Expose selected fields on a Bevy resource in the developer console.
//!
//! Try:
//!   res get render.wireframes
//!   res set render.max_fps 144
//!   res add render.max_fps 24
//!   res sub render.max_fps 30
//!   res toggle render.wireframes
//!   res set render.build_label release    # rejected: read-only
//!
//! Run with:
//!   cargo run --example resource_properties --features resource-properties

use bevy::prelude::*;
use chill_bevy_console::{
    BuiltinCommand, ChillConsole, ConsoleAppExt, ConsoleLineMessage, ConsoleResource,
};

#[derive(Resource, ConsoleResource, Debug)]
#[console_resource(prefix = "render")]
struct RenderSettings {
    #[console(help = "Draw scene wireframes")]
    wireframes: bool,

    #[console(help = "Maximum render frame rate")]
    max_fps: u32,

    #[console(readonly, help = "Build identifier")]
    build_label: String,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default().with_builtin_commands([BuiltinCommand::Res]))
        .insert_resource(RenderSettings {
            wireframes: false,
            max_fps: 60,
            build_label: "development".into(),
        })
        .add_console_resource::<RenderSettings>()
        .add_systems(Startup, setup)
        .add_systems(Update, apply_render_settings)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

// A regular Bevy system sees direct console changes through change detection.
fn apply_render_settings(
    settings: Res<RenderSettings>,
    mut console: MessageWriter<ConsoleLineMessage>,
) {
    if settings.is_changed() {
        info!("${settings:?}");
        console.write(ConsoleLineMessage::info(format!("${settings:?}")));
    }
}
