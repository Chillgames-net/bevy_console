use crate::config::ConsoleConfig;
use crate::registry::ConsoleRegistry;
use crate::state::ConsoleState;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;

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
pub(crate) struct ConsoleInputMain;

#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleInputGhost;

#[derive(Component, Default, Clone)]
pub(crate) struct ConsoleDropdown;

// ── Spawn ─────────────────────────────────────────────────────────────────────

pub(crate) fn spawn_console_ui(
    commands: &mut Commands,
    assets: &ConsoleAssets,
    config: &ConsoleConfig,
) {
    let main_font = assets.font.clone();
    let ghost_font = assets.font.clone();
    let input_prefix = config.input_prefix.clone();
    let font_size = config.font_size;
    let history_height_vh = config.history_height_vh;
    let history_padding = config.history_padding;
    let history_bg = config.history_bg;
    let input_padding_h = config.input_padding_h;
    let input_padding_v = config.input_padding_v;
    let input_border_width = config.input_border_width;
    let input_bg = config.input_bg;
    let input_border_color = config.input_border_color;
    let input_text_color = config.input_text_color;
    let input_ghost_color = config.input_ghost_color;
    let dropdown_bg = config.dropdown_bg;
    let dropdown_border_color = config.dropdown_border_color;

    commands.spawn_scene(bsn! {
        DevConsoleOverlay
        Node {
            position_type: PositionType::Absolute,
            top: px(0),
            left: px(0),
            width: percent(100),
            flex_direction: FlexDirection::Column,
        }
        ZIndex(200)
        Children [
            (
                ConsoleHistory
                Node {
                    flex_direction: FlexDirection::Column,
                    height: vh(history_height_vh),
                    max_height: vh(history_height_vh),
                    width: percent(100),
                    overflow: Overflow::scroll_y(),
                    padding: px(history_padding),
                }
                BackgroundColor({history_bg})
                ScrollPosition
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    width: percent(100),
                    padding: {UiRect::axes(
                        Val::Px(input_padding_h),
                        Val::Px(input_padding_v),
                    )},
                    border: {UiRect::top(Val::Px(input_border_width))},
                }
                BackgroundColor({input_bg})
                BorderColor {
                    top: {input_border_color},
                    right: {input_border_color},
                    bottom: {input_border_color},
                    left: {input_border_color},
                }
                Children [
                    (
                        ConsoleInputMain
                        Text({input_prefix})
                        TextFont {
                            font: FontSourceTemplate::Handle({main_font}),
                            font_size: px(font_size),
                        }
                        TextColor({input_text_color})
                    ),
                    (
                        ConsoleInputGhost
                        Text("")
                        TextFont {
                            font: FontSourceTemplate::Handle({ghost_font}),
                            font_size: px(font_size),
                        }
                        TextColor({input_ghost_color})
                    ),
                ]
            ),
            (
                ConsoleDropdown
                Node {
                    flex_direction: FlexDirection::Column,
                    width: percent(100),
                    border: {UiRect::bottom(Val::Px(1.0))},
                }
                BackgroundColor({dropdown_bg})
                BorderColor {
                    top: {dropdown_border_color},
                    right: {dropdown_border_color},
                    bottom: {dropdown_border_color},
                    left: {dropdown_border_color},
                }
            ),
        ]
    });
}

// ── Render ────────────────────────────────────────────────────────────────────

pub(crate) fn update_console_ui(
    mut commands: Commands,
    mut state: ResMut<ConsoleState>,
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
        if state.history_dirty || maybe_children.is_none() {
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
                        console_text_font(&font, config.history_font_size),
                        TextColor(config.history_text_color),
                    ));
                }
            });
            state.bypass_change_detection().history_dirty = false;
        }
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
                        console_text_font(&assets.font, config.dropdown_font_size),
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
