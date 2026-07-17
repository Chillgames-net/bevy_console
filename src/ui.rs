use crate::config::ConsoleConfig;
use crate::input::set_editable_text;
use crate::state::ConsoleState;
use crate::{ConsoleBuffer, ConsoleLevel};
use bevy::input_focus::AutoFocus;
use bevy::picking::pointer::PointerId;
use bevy::picking::prelude::{Click, Drag, DragEnd, Pointer, PointerButton};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, LineHeight, TextCursorStyle, TextLayoutInfo};
use bevy::ui::{ComputedNode, ScrollPosition, widget::TextScroll};
use std::collections::{HashSet, VecDeque};

// ── Assets ────────────────────────────────────────────────────────────────────

#[derive(Resource)]
pub(crate) struct ConsoleAssets {
    pub font: Handle<Font>,
}

fn console_text_font(font: &Handle<Font>, font_size: f32) -> TextFont {
    TextFont::from_font_size(font_size).with_font(font.clone())
}

const DROPDOWN_LINE_HEIGHT_MULTIPLIER: f32 = 1.2;
const DROPDOWN_ITEM_DIVIDER_HEIGHT: f32 = 1.0;

fn dropdown_item_max_height(config: &ConsoleConfig) -> Val {
    match config.dropdown_item_max_lines {
        0 => Val::Auto,
        lines => Val::Px(
            config.dropdown_font_size * DROPDOWN_LINE_HEIGHT_MULTIPLIER * lines as f32
                + config.dropdown_padding_v * 2.0
                + DROPDOWN_ITEM_DIVIDER_HEIGHT,
        ),
    }
}

impl FromWorld for ConsoleAssets {
    fn from_world(world: &mut World) -> Self {
        // Clone the path so we can release the borrow before touching AssetServer.
        let font_path = world.resource::<ConsoleConfig>().font_path.clone();
        let font = match font_path {
            Some(path) => world.resource::<AssetServer>().load(path),
            #[cfg(feature = "embedded-font")]
            None => crate::UBUNTU_MONO_FONT_HANDLE.clone(),
            #[cfg(not(feature = "embedded-font"))]
            None => Handle::default(), // Bevy's built-in default font
        };
        Self { font }
    }
}

// ── Marker components ─────────────────────────────────────────────────────────

#[derive(Component, Default, Clone)]
pub(crate) struct DevConsoleOverlay;

/// The history panel. `ColumnReverse` keeps the newest line at the bottom;
/// older lines overflow upward and get clipped.
#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleHistory;

#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleInput;

#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleInputGhost;

#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleDropdown;

/// The index of a completion rendered in the current suggestion page.
#[derive(Component, Clone, Copy)]
struct ConsoleCompletion(usize);

/// Touches that have crossed the upward swipe threshold in the current gesture.
#[derive(Component, Default)]
struct ConsoleSwipeDismiss(HashSet<PointerId>);

#[derive(Default)]
pub(crate) struct RenderedHistory {
    entity: Option<Entity>,
    lines: VecDeque<(u64, Entity)>,
}

#[derive(Default)]
pub(crate) struct RenderedDropdown {
    entity: Option<Entity>,
    items: Vec<crate::CompletionItem>,
    overflow: usize,
    match_index: usize,
}

impl RenderedDropdown {
    fn differs_from(&self, dropdown: Entity, state: &ConsoleState) -> bool {
        self.entity != Some(dropdown)
            || self.items != state.completion_items
            || self.overflow != state.completion_overflow
            || self.match_index != state.match_index
    }

    fn update(&mut self, dropdown: Entity, state: &ConsoleState) {
        self.entity = Some(dropdown);
        self.items.clone_from(&state.completion_items);
        self.overflow = state.completion_overflow;
        self.match_index = state.match_index;
    }
}

// ── Spawn ─────────────────────────────────────────────────────────────────────

pub(crate) fn spawn_console_ui(
    commands: &mut Commands,
    assets: &ConsoleAssets,
    config: &ConsoleConfig,
    initial_input: &str,
) {
    commands
        .spawn((
            DevConsoleOverlay,
            ConsoleSwipeDismiss::default(),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ZIndex(config.z_index),
        ))
        .observe(dismiss_console_on_two_finger_swipe_up)
        .observe(clear_swipe_dismiss_touch)
        .with_children(|parent| {
            parent
                .spawn((
                    ConsoleHistory,
                    Node {
                        flex_direction: FlexDirection::Column,
                        height: Val::Vh(config.history_height_vh),
                        max_height: Val::Vh(config.history_height_vh),
                        width: Val::Percent(100.0),
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::all(Val::Px(config.history_padding)),
                        ..default()
                    },
                    BackgroundColor(config.history_bg),
                    ScrollPosition::default(),
                ))
                .observe(scroll_console_on_touch_drag);

            parent
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(
                            Val::Px(config.input_padding_h),
                            Val::Px(config.input_padding_v),
                        ),
                        border: UiRect::top(Val::Px(config.input_border_width)),
                        overflow: Overflow::clip_x(),
                        ..default()
                    },
                    BackgroundColor(config.input_bg),
                    BorderColor::all(config.input_border_color),
                ))
                .with_children(|input_row| {
                    input_row.spawn((
                        Text::new(config.input_prefix.clone()),
                        console_text_font(&assets.font, config.font_size),
                        TextColor(config.input_text_color),
                    ));
                    input_row
                        .spawn(Node {
                            flex_grow: 1.0,
                            overflow: Overflow::clip_x(),
                            ..default()
                        })
                        .with_children(|input_area| {
                            input_area.spawn((
                                ConsoleInput,
                                EditableText::new(initial_input),
                                // Mobile keyboards can commit their return key through IME
                                // rather than KeyboardInput. Keep the console strictly
                                // single-line so that commit cannot leave a stray newline.
                                EditableTextFilter::new(|c| c != '\n' && c != '\r'),
                                TextCursorStyle {
                                    color: config.input_text_color,
                                    selection_color: config.input_border_color,
                                    unfocused_selection_color: Color::NONE,
                                    selected_text_color: None,
                                },
                                console_text_font(&assets.font, config.font_size),
                                TextColor(config.input_text_color),
                                TextLayout::no_wrap(),
                                Node {
                                    width: Val::Percent(100.0),
                                    ..default()
                                },
                                AutoFocus,
                            ));
                            input_area.spawn((
                                ConsoleInputGhost,
                                Text::new(""),
                                console_text_font(&assets.font, config.font_size),
                                TextColor(config.input_ghost_color),
                                Node {
                                    position_type: PositionType::Absolute,
                                    ..default()
                                },
                            ));
                        });
                });

            parent.spawn((
                ConsoleDropdown,
                Node {
                    flex_direction: FlexDirection::Column,
                    width: Val::Percent(100.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(config.dropdown_bg),
                BorderColor::all(config.dropdown_border_color),
            ));
        });
}

// ── Render ────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)] // Bevy system parameters are dependency injection, not call-site arguments.
pub(crate) fn update_console_ui(
    mut commands: Commands,
    state: Res<ConsoleState>,
    buffer: Res<ConsoleBuffer>,
    assets: Res<ConsoleAssets>,
    config: Res<ConsoleConfig>,
    mut history_q: Query<(Entity, &mut ScrollPosition), With<ConsoleHistory>>,
    mut history_line_q: Query<&mut BackgroundColor>,
    input_q: Query<(&EditableText, &TextLayoutInfo, &TextScroll), With<ConsoleInput>>,
    mut ghost_q: Query<(&mut Text, &mut Node), With<ConsoleInputGhost>>,
    dropdown_q: Query<(Entity, Option<&Children>), With<ConsoleDropdown>>,
    mut rendered_history: Local<RenderedHistory>,
    mut rendered_dropdown: Local<RenderedDropdown>,
) {
    // ── History lines ─────────────────────────────────────────────────────────
    if let Ok((history_entity, mut scroll_pos)) = history_q.single_mut() {
        let ui_recreated = rendered_history.entity != Some(history_entity);
        if ui_recreated {
            rendered_history.entity = Some(history_entity);
            rendered_history.lines.clear();
        }
        if buffer.is_changed() || ui_recreated {
            let first_buffer_id = buffer.lines().front().map(|line| line.id);
            while rendered_history
                .lines
                .front()
                .is_some_and(|(id, _)| Some(*id) != first_buffer_id)
            {
                let (_, entity) = rendered_history.lines.pop_front().unwrap();
                commands.entity(entity).despawn();
            }

            let font = assets.font.clone();
            commands.entity(history_entity).with_children(|parent| {
                for line in buffer.lines().iter().skip(rendered_history.lines.len()) {
                    let entity = parent
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(if Some(line.id) == state.selected_history_line_id() {
                                config.history_highlight_bg
                            } else {
                                Color::NONE
                            }),
                            Text::new(line.text.clone()),
                            console_text_font(&font, config.history_font_size),
                            TextColor(history_line_color(line.level, &config)),
                        ))
                        .id();
                    rendered_history.lines.push_back((line.id, entity));
                }
            });
        }
        if state.is_changed() {
            let selected_line_id = state.selected_history_line_id();
            for (line_id, entity) in &rendered_history.lines {
                if let Ok(mut background) = history_line_q.get_mut(*entity) {
                    *background = BackgroundColor(if Some(*line_id) == selected_line_id {
                        config.history_highlight_bg
                    } else {
                        Color::NONE
                    });
                }
            }
        }
        if state.scroll_follow {
            scroll_pos.y = f32::MAX;
        }
    }

    if let Ok((mut ghost, mut ghost_node)) = ghost_q.single_mut() {
        let input = input_q.single().ok();
        let cursor = input.map(|(input, _, _)| input.editor().raw_selection().focus().index());
        let ghost_str = (cursor == Some(state.input.len()))
            .then(|| {
                state
                    .completion_items
                    .get(state.match_index)
                    .and_then(|item| {
                        state
                            .input
                            .get(item.replace.start..item.replace.end)
                            .and_then(|fragment| item.insert_text.strip_prefix(fragment))
                    })
            })
            .flatten()
            .map(str::to_string)
            .unwrap_or_default();
        *ghost = Text::new(ghost_str);
        ghost_node.left = Val::Px(
            input
                .and_then(|(_, layout, scroll)| {
                    layout
                        .cursor
                        .map(|(_, cursor)| (cursor.min.x - scroll.0.x) / layout.scale_factor)
                })
                .unwrap_or_default(),
        );
    }

    // ── Dropdown ──────────────────────────────────────────────────────────────
    if let Ok((dropdown, maybe_children)) = dropdown_q.single() {
        // Buffer-only updates do not affect completion results. Avoid even
        // comparing the cached candidates in that common path.
        if !state.is_changed() && rendered_dropdown.entity == Some(dropdown) {
            return;
        }
        if !rendered_dropdown.differs_from(dropdown, &state) {
            return;
        }

        if let Some(children) = maybe_children {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }

        let page = state.completion_page_range(config.max_suggestions);
        if !page.is_empty() {
            commands.entity(dropdown).with_children(|parent| {
                for i in page.clone() {
                    let item = &state.completion_items[i];
                    let selected = i == state.match_index;
                    let label = if item.detail.is_empty() {
                        item.label.clone()
                    } else {
                        format!("{} - {}", item.label, item.detail)
                    };
                    parent
                        .spawn((
                            ConsoleCompletion(i),
                            Node {
                                padding: UiRect::axes(
                                    Val::Px(config.dropdown_padding_h),
                                    Val::Px(config.dropdown_padding_v),
                                ),
                                width: Val::Percent(100.0),
                                min_width: Val::Px(0.0),
                                max_height: dropdown_item_max_height(&config),
                                overflow: Overflow::clip(),
                                border: UiRect::top(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(if selected {
                                config.dropdown_highlight_bg
                            } else {
                                Color::srgba(0.0, 0.0, 0.0, 0.0)
                            }),
                            BorderColor::all(config.dropdown_item_divider_color),
                        ))
                        .observe(accept_completion_on_click)
                        .with_children(|row| {
                            row.spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    min_width: Val::Px(0.0),
                                    ..default()
                                },
                                Text::new(label),
                                console_text_font(&assets.font, config.dropdown_font_size),
                                LineHeight::Px(
                                    config.dropdown_font_size * DROPDOWN_LINE_HEIGHT_MULTIPLIER,
                                ),
                                TextColor(if selected {
                                    config.dropdown_highlight_text_color
                                } else {
                                    config.dropdown_text_color
                                }),
                            ));
                        });
                }

                if state.completion_items.len() > config.max_suggestions {
                    parent.spawn((
                        Node {
                            padding: UiRect::axes(
                                Val::Px(config.dropdown_padding_h),
                                Val::Px(config.dropdown_padding_v),
                            ),
                            width: Val::Percent(100.0),
                            border: UiRect::top(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(config.dropdown_item_divider_color),
                        Text::new(format!(
                            "{}-{} of {}",
                            page.start + 1,
                            page.end,
                            state.completion_items.len()
                        )),
                        console_text_font(&assets.font, config.dropdown_font_size),
                        TextColor(config.dropdown_text_color),
                    ));
                }
            });
        }
        rendered_dropdown.update(dropdown, &state);
    }
}

/// Scrolls the history panel in response to a one-finger touch drag.
///
/// Pointer drag coordinates are physical pixels, while `ScrollPosition` uses
/// logical pixels, so account for the UI scale before applying the delta.
fn scroll_console_on_touch_drag(
    drag: On<Pointer<Drag>>,
    mut state: ResMut<ConsoleState>,
    mut history_q: Query<(&mut ScrollPosition, &ComputedNode), With<ConsoleHistory>>,
) {
    if !drag.pointer_id.is_touch() || drag.button != PointerButton::Primary {
        return;
    }

    let Ok((mut scroll_pos, computed)) = history_q.single_mut() else {
        return;
    };

    let max_scroll =
        (computed.content_size().y - computed.size().y).max(0.0) * computed.inverse_scale_factor;
    let current = scroll_pos.y.min(max_scroll);
    let new_y = (current - drag.delta.y * computed.inverse_scale_factor).clamp(0.0, max_scroll);
    scroll_pos.y = new_y;
    state.scroll_follow = new_y >= max_scroll - 1.0;
}

/// Closes the console after two touches make a deliberate upward swipe together.
fn dismiss_console_on_two_finger_swipe_up(
    mut drag: On<Pointer<Drag>>,
    mut swipe_q: Query<&mut ConsoleSwipeDismiss>,
    mut state: ResMut<ConsoleState>,
) {
    const SWIPE_DISMISS_DISTANCE: f32 = 80.0;

    if !drag.pointer_id.is_touch()
        || drag.button != PointerButton::Primary
        || drag.distance.y > -SWIPE_DISMISS_DISTANCE
        || drag.distance.y.abs() < drag.distance.x.abs()
    {
        return;
    }

    let Ok(mut swipe) = swipe_q.get_mut(drag.event_target()) else {
        return;
    };
    swipe.0.insert(drag.pointer_id);
    if swipe.0.len() < 2 {
        return;
    }

    state.open = false;
    drag.propagate(false);
}

/// A completed drag must not count toward a later two-finger gesture.
fn clear_swipe_dismiss_touch(
    drag_end: On<Pointer<DragEnd>>,
    mut swipe_q: Query<&mut ConsoleSwipeDismiss>,
) {
    if let Ok(mut swipe) = swipe_q.get_mut(drag_end.event_target()) {
        swipe.0.remove(&drag_end.pointer_id);
    }
}

/// Accepts a completion for either mouse clicks or touch taps.
fn accept_completion_on_click(
    mut click: On<Pointer<Click>>,
    completions: Query<&ConsoleCompletion>,
    mut state: ResMut<ConsoleState>,
    mut input_q: Query<&mut EditableText, With<ConsoleInput>>,
) {
    let Ok(completion) = completions.get(click.entity) else {
        return;
    };
    let Ok(mut input) = input_q.single_mut() else {
        return;
    };

    // The rows are rebuilt when completion data changes. Ignore a late tap
    // delivered for a stale row rather than accepting a different suggestion.
    if completion.0 >= state.completion_items.len() {
        return;
    }

    state.match_index = completion.0;
    if let Some(cursor) = state.apply_selected_completion() {
        state.cmd_history_index = None;
        state.cmd_history_draft.clear();
        set_editable_text(&mut input, &state.input, cursor);
        click.propagate(false);
    }
}

fn history_line_color(level: ConsoleLevel, config: &ConsoleConfig) -> Color {
    match level {
        ConsoleLevel::Trace | ConsoleLevel::Debug => config.history_debug_color,
        ConsoleLevel::Info => config.history_text_color,
        ConsoleLevel::Warn => config.history_warn_color,
        ConsoleLevel::Error => config.history_error_color,
    }
}

#[cfg(test)]
mod tests {
    use super::{ConsoleAssets, ConsoleHistory, update_console_ui};
    use crate::{ConsoleBuffer, ConsoleConfig, ConsoleLevel, ConsoleLineSource, ConsoleState};
    use bevy::prelude::*;
    use bevy::ui::ScrollPosition;

    #[test]
    fn history_highlight_follows_the_recalled_command() {
        let mut buffer = ConsoleBuffer::default();
        buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, "> first");
        let first_id = buffer.last_line().unwrap().id;
        buffer.push(ConsoleLevel::Info, ConsoleLineSource::System, "> second");
        let second_id = buffer.last_line().unwrap().id;

        let mut app = App::new();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(ConsoleAssets {
                font: Handle::default(),
            })
            .insert_resource(ConsoleState {
                open: true,
                cmd_history: vec!["first".into(), "second".into()],
                cmd_history_line_ids: vec![Some(first_id), Some(second_id)],
                cmd_history_index: Some(1),
                ..default()
            })
            .insert_resource(buffer)
            .add_systems(Update, update_console_ui);
        app.world_mut()
            .spawn((ConsoleHistory, ScrollPosition::default()));

        app.update();
        assert_history_highlight(&mut app, "> second");

        app.world_mut()
            .resource_mut::<ConsoleState>()
            .cmd_history_index = Some(0);
        app.update();
        assert_history_highlight(&mut app, "> first");
    }

    fn assert_history_highlight(app: &mut App, expected: &str) {
        let highlight = app.world().resource::<ConsoleConfig>().history_highlight_bg;
        let mut rows = app.world_mut().query::<(&Text, &BackgroundColor)>();
        for (text, background) in rows.iter(app.world()) {
            assert_eq!(background.0 == highlight, text.0 == expected, "{}", text.0);
        }
    }
}
