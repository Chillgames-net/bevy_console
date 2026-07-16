use crate::config::ConsoleConfig;
use crate::state::ConsoleState;
use crate::{ConsoleBuffer, ConsoleLevel};
use bevy::input_focus::AutoFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle, TextLayoutInfo};
use bevy::ui::widget::TextScroll;
use std::collections::VecDeque;

// ── Assets ────────────────────────────────────────────────────────────────────

#[derive(Resource)]
pub(crate) struct ConsoleAssets {
    pub font: Handle<Font>,
}

fn console_text_font(font: &Handle<Font>, font_size: f32) -> TextFont {
    TextFont::from_font_size(font_size).with_font(font.clone())
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
        .with_children(|parent| {
            parent.spawn((
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
            ));

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
                            Text::new(line.text.clone()),
                            console_text_font(&font, config.history_font_size),
                            TextColor(history_line_color(line.level, &config)),
                        ))
                        .id();
                    rendered_history.lines.push_back((line.id, entity));
                }
            });
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
                        BackgroundColor(if selected {
                            config.dropdown_highlight_bg
                        } else {
                            Color::srgba(0.0, 0.0, 0.0, 0.0)
                        }),
                        BorderColor::all(config.dropdown_item_divider_color),
                        Text::new(label),
                        console_text_font(&assets.font, config.dropdown_font_size),
                        TextColor(if selected {
                            config.dropdown_highlight_text_color
                        } else {
                            config.dropdown_text_color
                        }),
                    ));
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

fn history_line_color(level: ConsoleLevel, config: &ConsoleConfig) -> Color {
    match level {
        ConsoleLevel::Trace | ConsoleLevel::Debug => config.history_debug_color,
        ConsoleLevel::Info => config.history_text_color,
        ConsoleLevel::Warn => config.history_warn_color,
        ConsoleLevel::Error => config.history_error_color,
    }
}
