use crate::config::ConsoleConfig;
use crate::registry::ConsoleRegistry;
use crate::state::ConsoleState;
use bevy::prelude::*;

// ── Assets ────────────────────────────────────────────────────────────────────

#[derive(Resource)]
pub(crate) struct ConsoleAssets {
    pub font: Handle<Font>,
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

#[derive(Component)]
pub(crate) struct DevConsoleOverlay;

/// The history panel. `ColumnReverse` keeps the newest line at the bottom;
/// older lines overflow upward and get clipped.
#[derive(Component)]
pub(crate) struct ConsoleHistory;

#[derive(Component)]
pub(crate) struct ConsoleInputMain;

#[derive(Component)]
pub(crate) struct ConsoleInputGhost;

#[derive(Component)]
pub(crate) struct ConsoleDropdown;

// ── Spawn ─────────────────────────────────────────────────────────────────────

pub(crate) fn spawn_console_ui(
    commands: &mut Commands,
    assets: &ConsoleAssets,
    config: &ConsoleConfig,
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
            ZIndex(200),
        ))
        .with_children(|parent| {
            // ── History panel ─────────────────────────────────────────────────
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

            // ── Input bar ─────────────────────────────────────────────────────
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
                        ..default()
                    },
                    BackgroundColor(config.input_bg),
                    BorderColor::all(config.input_border_color),
                ))
                .with_children(|row| {
                    row.spawn((
                        ConsoleInputMain,
                        Text::new(config.input_prefix.clone()),
                        TextFont {
                            font: assets.font.clone(),
                            font_size: config.font_size,
                            ..default()
                        },
                        TextColor(config.input_text_color),
                    ));
                    row.spawn((
                        ConsoleInputGhost,
                        Text::new(""),
                        TextFont {
                            font: assets.font.clone(),
                            font_size: config.font_size,
                            ..default()
                        },
                        TextColor(config.input_ghost_color),
                    ));
                });

            // ── Dropdown suggestions ──────────────────────────────────────────
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

pub(crate) fn update_console_ui(
    mut commands: Commands,
    state: Res<ConsoleState>,
    assets: Res<ConsoleAssets>,
    config: Res<ConsoleConfig>,
    registry: Res<ConsoleRegistry>,
    mut history_q: Query<(Entity, Option<&Children>, &mut ScrollPosition), With<ConsoleHistory>>,
    mut main_q: Query<&mut Text, (With<ConsoleInputMain>, Without<ConsoleInputGhost>)>,
    mut ghost_q: Query<&mut Text, (With<ConsoleInputGhost>, Without<ConsoleInputMain>)>,
    dropdown_q: Query<(Entity, Option<&Children>), With<ConsoleDropdown>>,
) {
    // ── History lines ─────────────────────────────────────────────────────────
    if let Ok((history_entity, maybe_children, mut scroll_pos)) = history_q.single_mut() {
        if let Some(children) = maybe_children {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }
        let font = assets.font.clone();
        commands.entity(history_entity).with_children(|parent| {
            for line in state.history.iter() {
                parent.spawn((
                    Text::new(line.clone()),
                    TextFont {
                        font: font.clone(),
                        font_size: config.history_font_size,
                        ..default()
                    },
                    TextColor(config.history_text_color),
                ));
            }
        });
        if state.scroll_follow {
            scroll_pos.y = f32::MAX;
        }
    }

    // ── Input ─────────────────────────────────────────────────────────────────
    if let Ok(mut main) = main_q.single_mut() {
        *main = Text::new(format!("{}{}", config.input_prefix, state.input));
    }

    if let Ok(mut ghost) = ghost_q.single_mut() {
        let ghost_str = state
            .matches
            .get(state.match_index)
            .filter(|m| m.starts_with(state.input.trim()))
            .map(|m| m[state.input.len()..].to_string())
            .unwrap_or_default();
        *ghost = Text::new(format!("{ghost_str}_"));
    }

    // ── Dropdown ──────────────────────────────────────────────────────────────
    if let Ok((dropdown, maybe_children)) = dropdown_q.single() {
        if let Some(children) = maybe_children {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }

        if !state.matches.is_empty() {
            commands.entity(dropdown).with_children(|parent| {
                for (i, name) in state.matches.iter().enumerate() {
                    let selected = i == state.match_index;
                    let label = registry
                        .commands
                        .get(name)
                        .map(|def| def.usage)
                        .unwrap_or(name.as_str())
                        .to_string();
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
                        TextFont {
                            font: assets.font.clone(),
                            font_size: config.dropdown_font_size,
                            ..default()
                        },
                        TextColor(if selected {
                            config.dropdown_highlight_text_color
                        } else {
                            config.dropdown_text_color
                        }),
                    ));
                }
            });
        }
    }
}
