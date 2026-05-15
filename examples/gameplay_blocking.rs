//! Use the `console_closed` run condition to suppress gameplay input while the
//! console is open.
//!
//! Press WASD to move the square. Open the console with `` ` `` and try
//! typing — the square stops moving until you close the console again.
//!
//! Run with: `cargo run --example gameplay_blocking`

use bevy::prelude::*;
use chill_bevy_console::{ChillConsole, console_closed};

#[derive(Component)]
struct Player;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(ChillConsole::default())
        .add_systems(Startup, setup)
        .add_systems(Update, move_player.run_if(console_closed))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Sprite::from_color(Color::srgb(0.4, 0.7, 1.0), Vec2::splat(40.0)),
        Transform::default(),
        Player,
    ));
}

fn move_player(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut query: Query<&mut Transform, With<Player>>,
) {
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir.x += 1.0;
    }

    if let Ok(mut transform) = query.single_mut() {
        transform.translation += dir.normalize_or_zero().extend(0.0) * 200.0 * time.delta_secs();
    }
}
