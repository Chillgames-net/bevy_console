//! Console history scrolling.

use crate::ConsoleState;
use crate::ui::ConsoleHistory;
use bevy::input::mouse::{MouseScrollPixelsPerLine, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};

const CONSOLE_SCROLL_SPEED: f32 = 1.25;

pub(crate) fn scroll_console(
    mut mouse_wheel: MessageReader<MouseWheel>,
    pixels_per_line: Res<MouseScrollPixelsPerLine>,
    mut state: ResMut<ConsoleState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut history_q: Query<(&mut ScrollPosition, &ComputedNode), With<ConsoleHistory>>,
) {
    let wheel_pixels: f32 = mouse_wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y * *pixels_per_line,
            MouseScrollUnit::Pixel => event.y,
        })
        .sum();

    // Drain wheel messages while closed so a pre-open scroll cannot apply to
    // the newly spawned history panel.
    if !state.open {
        return;
    }

    let key_pixels = if keys.just_pressed(KeyCode::PageUp) {
        240.0
    } else if keys.just_pressed(KeyCode::PageDown) {
        -240.0
    } else {
        0.0
    };
    if wheel_pixels == 0.0 && key_pixels == 0.0 {
        return;
    }

    let Ok((mut scroll_pos, computed)) = history_q.single_mut() else {
        return;
    };

    // `MouseWheel` and computed UI sizes are physical pixels, while
    // `ScrollPosition` is logical pixels. Convert both so scrolling feels
    // consistent on high-DPI displays.
    let pixels = (wheel_pixels * computed.inverse_scale_factor + key_pixels) * CONSOLE_SCROLL_SPEED;
    let max_scroll =
        (computed.content_size().y - computed.size().y).max(0.0) * computed.inverse_scale_factor;
    let current = scroll_pos.y.min(max_scroll);
    let new_y = (current - pixels).clamp(0.0, max_scroll);
    scroll_pos.y = new_y;

    if pixels > 0.0 {
        if state.scroll_follow {
            state.scroll_follow = false;
        }
    } else if new_y >= max_scroll - 1.0 && !state.scroll_follow {
        state.scroll_follow = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::touch::TouchPhase;

    #[test]
    fn wheel_units_use_platform_calibration_and_ui_scale() {
        let mut app = App::new();
        let mut pixels_per_line = MouseScrollPixelsPerLine::default();
        *pixels_per_line = 80.0;
        app.insert_resource(pixels_per_line)
            .insert_resource(ConsoleState {
                open: true,
                ..default()
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<MouseWheel>()
            .add_systems(Update, scroll_console);
        let history = app
            .world_mut()
            .spawn((
                ConsoleHistory,
                ScrollPosition(Vec2::new(0.0, 300.0)),
                ComputedNode {
                    size: Vec2::splat(200.0),
                    content_size: Vec2::new(200.0, 1200.0),
                    inverse_scale_factor: 0.5,
                    ..default()
                },
            ))
            .id();
        for (unit, y, expected) in [
            (MouseScrollUnit::Line, 1.0, 250.0),
            (MouseScrollUnit::Pixel, 80.0, 200.0),
        ] {
            app.world_mut().write_message(MouseWheel {
                unit,
                x: 0.0,
                y,
                window: Entity::PLACEHOLDER,
                phase: TouchPhase::Moved,
            });
            app.update();
            assert_eq!(
                app.world().get::<ScrollPosition>(history).unwrap().y,
                expected
            );
            assert!(!app.world().resource::<ConsoleState>().scroll_follow);
        }
    }
}
