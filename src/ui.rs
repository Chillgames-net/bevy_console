use crate::config::ConsoleConfig;
use crate::editor::set_editable_text;
use crate::state::ConsoleState;
use crate::{ConsoleBuffer, ConsoleLevel};
use bevy::input_focus::{AutoFocus, tab_navigation::TabIndex};
use bevy::picking::pointer::PointerId;
use bevy::picking::prelude::{PointerButton, PointerClick, PointerDrag, PointerDragEnd};
use bevy::prelude::*;
use bevy::text::{
    EditableText, EditableTextFilter, LineHeight, TextCursorStyle, TextLayoutInfo,
    TextReadWriteMode,
};
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy::ui_widgets::TextInput;
use std::collections::{HashSet, VecDeque};

/// Updates the console view only while visible and after its state or output changes.
pub(crate) fn console_open_and_changed(
    state: Option<Res<ConsoleState>>,
    buffer: Option<Res<ConsoleBuffer>>,
) -> bool {
    state.is_some_and(|state| {
        state.open && (state.is_changed() || buffer.is_some_and(|buffer| buffer.is_changed()))
    })
}

/// Spawns or despawns the console UI whenever `state.open` changes.
/// Reacts to changes from any source (key press, external code, etc.).
pub(crate) fn sync_console_ui(
    mut commands: Commands,
    mut state: ResMut<ConsoleState>,
    overlay_q: Query<Entity, With<DevConsoleOverlay>>,
    assets: Res<ConsoleAssets>,
    config: Res<ConsoleConfig>,
    mut prev_open: Local<bool>,
) {
    if *prev_open == state.open {
        return;
    }
    *prev_open = state.open;

    if state.open {
        spawn_console_ui(&mut commands, &assets, &config, &state.input);
        state.mark_input_changed();
    } else {
        for entity in &overlay_q {
            commands.entity(entity).despawn();
        }
    }
}

// ── Assets ────────────────────────────────────────────────────────────────────

#[derive(Resource)]
pub(crate) struct ConsoleAssets {
    pub(crate) font: Handle<Font>,
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

/// The scrollable history viewport.
#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleHistory;

/// The history content column. It fills an empty viewport so its lines can be
/// bottom-aligned, then grows normally once there is more output than fits.
#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleHistoryContent;

/// A rendered row in the console history panel.
#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleHistoryLine;

/// Read-only selection overlay; the parent Text keeps native intrinsic sizing.
#[derive(Component)]
pub(crate) struct ConsoleHistoryText;

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
    scroll_follow: bool,
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
                .observe(scroll_console_on_touch_drag)
                .with_children(|history| {
                    history.spawn((
                        ConsoleHistoryContent,
                        Node {
                            flex_direction: FlexDirection::Column,
                            // Keep a short history on the terminal baseline,
                            // without positioning overflowing lines above the
                            // scroll viewport.
                            justify_content: JustifyContent::FlexEnd,
                            width: Val::Percent(100.0),
                            min_height: Val::Percent(100.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                });

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
                                TextInput,
                                TabIndex(0),
                                // Mobile keyboards can commit their return key through IME
                                // rather than KeyboardInput. Keep the console strictly
                                // single-line so that commit cannot leave a stray newline.
                                EditableTextFilter::new(|c| c != '\n' && c != '\r'),
                                TextCursorStyle {
                                    color: config.input_text_color,
                                    selection_color: config.input_border_color,
                                    unfocused_selection_color: Color::NONE,
                                    selected_text_color: None,
                                    ..default()
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
    history_content_q: Query<Entity, With<ConsoleHistoryContent>>,
    mut history_line_q: Query<&mut BackgroundColor, With<ConsoleHistoryLine>>,
    input_q: Query<(&EditableText, &TextLayoutInfo), With<ConsoleInput>>,
    mut ghost_q: Query<(&mut Text, &mut Node), With<ConsoleInputGhost>>,
    dropdown_q: Query<(Entity, Option<&Children>), With<ConsoleDropdown>>,
    mut rendered_history: Local<RenderedHistory>,
    mut rendered_dropdown: Local<RenderedDropdown>,
) {
    // ── History lines ─────────────────────────────────────────────────────────
    if let (Ok((_, scroll_pos)), Ok(history_content)) =
        (history_q.single_mut(), history_content_q.single())
    {
        let ui_recreated = rendered_history.entity != Some(history_content);
        if ui_recreated {
            rendered_history.entity = Some(history_content);
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
            commands.entity(history_content).with_children(|parent| {
                for line in buffer.lines().iter().skip(rendered_history.lines.len()) {
                    let entity = parent
                        .spawn((
                            ConsoleHistoryLine,
                            Node {
                                width: Val::Percent(100.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(if Some(line.id) == state.selected_history_line_id() {
                                config.history_highlight_bg
                            } else {
                                Color::NONE
                            }),
                        ))
                        .with_children(|row| {
                            // Text must remain a leaf for intrinsic height measurement.
                            row.spawn((
                                Text::new(&line.text),
                                Node {
                                    width: Val::Percent(100.0),
                                    ..default()
                                },
                                console_text_font(&font, config.history_font_size),
                                TextColor(history_line_color(line.level, &config)),
                            ));
                            // ponytail: selection stays within one output row; use a transcript widget for cross-row selection.
                            row.spawn((
                                ConsoleHistoryText,
                                EditableText {
                                    visible_lines: None,
                                    ..EditableText::new(&line.text)
                                },
                                TextInput,
                                TabIndex(0),
                                TextReadWriteMode::ReadOnly,
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(0.0),
                                    top: Val::Px(0.0),
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                console_text_font(&font, config.history_font_size),
                                TextColor(Color::NONE),
                                TextCursorStyle {
                                    color: Color::NONE,
                                    selection_color: config.input_border_color,
                                    selected_text_color: Some(history_line_color(
                                        line.level, &config,
                                    )),
                                    ..default()
                                },
                            ))
                            // TextInput consumes drags; retain touch scrolling and swipe dismissal.
                            .observe(scroll_console_on_touch_drag)
                            .observe(dismiss_console_on_two_finger_swipe_up)
                            .observe(clear_swipe_dismiss_touch);
                        })
                        .id();
                    rendered_history.lines.push_back((line.id, entity));
                }
            });
        }
        if state.is_changed() {
            let selected_line_id = state.selected_history_line_id();
            for (line_id, entity) in &rendered_history.lines {
                if let Ok(mut background) = history_line_q.get_mut(*entity) {
                    background.set_if_neq(BackgroundColor(if Some(*line_id) == selected_line_id {
                        config.history_highlight_bg
                    } else {
                        Color::NONE
                    }));
                }
            }
        }
        if state.scroll_follow
            && (buffer.is_changed() || ui_recreated || !rendered_history.scroll_follow)
        {
            scroll_pos
                .map_unchanged(|position| &mut position.y)
                .set_if_neq(f32::MAX);
        }
        rendered_history.scroll_follow = state.scroll_follow;
    }

    if let Ok((mut ghost, ghost_node)) = ghost_q.single_mut() {
        let input = input_q.single().ok();
        let cursor = input.map(|(input, _)| input.editor().raw_selection().focus().index());
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
        ghost.set_if_neq(Text::new(ghost_str));
        ghost_node
            .map_unchanged(|node| &mut node.left)
            .set_if_neq(Val::Px(
                input
                    .and_then(|(input, layout)| {
                        layout.cursor.map(|(_, cursor)| {
                            (cursor.min.x - input.viewport.offset.x) / layout.scale_factor
                        })
                    })
                    .unwrap_or_default(),
            ));
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
    drag: On<PointerDrag>,
    mut state: ResMut<ConsoleState>,
    mut history_q: Query<(&mut ScrollPosition, &ComputedNode), With<ConsoleHistory>>,
) {
    if !drag.pointer.id.is_touch() || drag.button != PointerButton::Primary {
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
    mut drag: On<PointerDrag>,
    mut swipe_q: Query<&mut ConsoleSwipeDismiss>,
    mut state: ResMut<ConsoleState>,
) {
    const SWIPE_DISMISS_DISTANCE: f32 = 80.0;

    if !drag.pointer.id.is_touch()
        || drag.button != PointerButton::Primary
        || drag.distance.y > -SWIPE_DISMISS_DISTANCE
        || drag.distance.y.abs() < drag.distance.x.abs()
    {
        return;
    }

    let Ok(mut swipe) = swipe_q.single_mut() else {
        return;
    };
    swipe.0.insert(drag.pointer.id);
    if swipe.0.len() < 2 {
        return;
    }

    state.open = false;
    drag.propagate(false);
}

/// A completed drag must not count toward a later two-finger gesture.
fn clear_swipe_dismiss_touch(
    drag_end: On<PointerDragEnd>,
    mut swipe_q: Query<&mut ConsoleSwipeDismiss>,
) {
    if let Ok(mut swipe) = swipe_q.single_mut() {
        swipe.0.remove(&drag_end.pointer.id);
    }
}

/// Accepts a completion for either mouse clicks or touch taps.
fn accept_completion_on_click(
    mut click: On<PointerClick>,
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
    use super::{ConsoleAssets, ConsoleHistory, ConsoleHistoryContent, update_console_ui};
    use crate::{ConsoleBuffer, ConsoleConfig, ConsoleLevel, ConsoleLineSource, ConsoleState};
    use bevy::prelude::*;
    use bevy::ui::ScrollPosition;

    #[test]
    fn history_rows_have_height_and_scroll_into_view() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::transform::TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            bevy::input::InputPlugin,
            bevy::picking::DefaultPickingPlugins,
            bevy::window::WindowPlugin::default(),
            bevy::a11y::AccessibilityPlugin,
            bevy::camera::CameraPlugin,
            bevy::image::ImagePlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::text::TextPlugin,
            bevy::ui::UiPlugin,
            bevy::input_focus::InputFocusPlugin,
            bevy::ui_widgets::TextInputPlugin,
        ))
        .init_asset::<bevy::image::TextureAtlasLayout>();
        app.insert_resource(ConsoleConfig::default())
            .insert_resource(ConsoleAssets {
                font: Handle::default(),
            })
            .insert_resource(ConsoleState {
                open: true,
                ..default()
            })
            .init_resource::<ConsoleBuffer>()
            .add_systems(
                Startup,
                |mut commands: Commands, assets: Res<ConsoleAssets>, config: Res<ConsoleConfig>| {
                    commands.spawn(Camera2d);
                    super::spawn_console_ui(&mut commands, &assets, &config, "");
                },
            )
            .add_systems(Update, update_console_ui);
        for _ in 0..80 {
            app.world_mut().resource_mut::<ConsoleBuffer>().push(
                ConsoleLevel::Info,
                ConsoleLineSource::System,
                "visible history output",
            );
        }
        app.finish();
        app.cleanup();
        for _ in 0..5 {
            app.update();
        }
        let world = app.world_mut();
        let mut rows = world.query_filtered::<&ComputedNode, With<super::ConsoleHistoryLine>>();
        assert_eq!(rows.iter(world).count(), 80);
        assert!(
            rows.iter(world).all(|row| row.size.y > 0.0),
            "history rows collapsed"
        );
        let mut history = world.query_filtered::<&ComputedNode, With<ConsoleHistory>>();
        let history = history.single(world).unwrap();
        assert!(
            history.scroll_position.y > 0.0,
            "history should scroll to its overflowing output"
        );
        let mut content = world.query_filtered::<&Children, With<ConsoleHistoryContent>>();
        let last_row = *content.single(world).unwrap().last().unwrap();
        let children = world.get::<Children>(last_row).unwrap();
        assert!(
            !world
                .get::<bevy::text::TextLayoutInfo>(children[0])
                .unwrap()
                .glyphs
                .is_empty()
        );
        assert_eq!(
            world.get::<ComputedNode>(last_row).unwrap().size,
            world.get::<ComputedNode>(children[1]).unwrap().size,
            "selection overlay must cover the visible row"
        );
    }

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
        let history = app
            .world_mut()
            .spawn((ConsoleHistory, ScrollPosition::default()))
            .id();
        app.world_mut()
            .entity_mut(history)
            .with_child((ConsoleHistoryContent, Node::default()));
        let ghost = app
            .world_mut()
            .spawn((
                super::ConsoleInputGhost,
                Text::new(""),
                Node {
                    left: Val::Px(0.0),
                    ..default()
                },
            ))
            .id();

        app.update();
        assert_history_highlight(&mut app, "> second");

        app.world_mut().clear_trackers();
        app.world_mut().resource_mut::<ConsoleState>().enabled = false;
        app.update();
        assert!(
            app.world_mut()
                .query::<Ref<BackgroundColor>>()
                .iter(app.world())
                .all(|color| !color.is_changed())
        );
        let (text, node) = app
            .world_mut()
            .query::<(Ref<Text>, Ref<Node>)>()
            .get(app.world(), ghost)
            .unwrap();
        assert!(!text.is_changed());
        assert!(!node.is_changed());
        assert!(
            !app.world_mut()
                .query::<Ref<ScrollPosition>>()
                .get(app.world(), history)
                .unwrap()
                .is_changed()
        );

        app.world_mut().resource_mut::<ConsoleState>().scroll_follow = false;
        app.world_mut()
            .get_mut::<ScrollPosition>(history)
            .unwrap()
            .y = 80.0;
        app.update();
        app.world_mut().resource_mut::<ConsoleState>().scroll_follow = true;
        app.update();
        assert_eq!(
            app.world().get::<ScrollPosition>(history).unwrap().y,
            f32::MAX
        );

        app.world_mut()
            .resource_mut::<ConsoleState>()
            .cmd_history_index = Some(0);
        app.update();
        assert_history_highlight(&mut app, "> first");
    }

    fn assert_history_highlight(app: &mut App, expected: &str) {
        let highlight = app.world().resource::<ConsoleConfig>().history_highlight_bg;
        let mut rows = app
            .world_mut()
            .query_filtered::<(&BackgroundColor, &Children), With<super::ConsoleHistoryLine>>();
        for (background, children) in rows.iter(app.world()) {
            let text = app.world().get::<Text>(children[0]).unwrap();
            assert_eq!(background.0 == highlight, text.0 == expected, "{}", text.0);
            let overlay = children[1];
            assert_eq!(
                app.world().get::<bevy::text::TextReadWriteMode>(overlay),
                Some(&bevy::text::TextReadWriteMode::ReadOnly)
            );
            assert_eq!(
                app.world()
                    .get::<bevy::text::EditableText>(overlay)
                    .unwrap()
                    .value()
                    .to_string(),
                text.0
            );
        }
    }
}
