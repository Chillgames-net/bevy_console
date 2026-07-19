//! Console history scrolling.

use crate::ConsoleState;
use crate::ui::ConsoleHistory;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};

const CONSOLE_SCROLL_SPEED: f32 = 1.25;

pub(crate) fn scroll_console(
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut state: ResMut<ConsoleState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut history_q: Query<(&mut ScrollPosition, &ComputedNode), With<ConsoleHistory>>,
) {
    let wheel_pixels: f32 = mouse_wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y * MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
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
