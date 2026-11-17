//! Shared styling for the city's native panels.
use bevy::{
    input_focus::{
        InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    picking::Pickable,
    text::{EditableText, FontSource, TextCursorStyle},
    ui_widgets::{Activate, TextInput, observe},
};

use super::*;

pub(super) fn panel(commands: &mut Commands, title: &str, width: f32) -> Entity {
    surface(commands, title, width, false)
}

pub(super) fn tablet(commands: &mut Commands, title: &str) -> Entity {
    surface(commands, title, 0., true)
}

fn surface(commands: &mut Commands, title: &str, width: f32, tablet: bool) -> Entity {
    let backdrop = commands
        .spawn((
            GlobalZIndex(40),
            BackgroundColor(if tablet {
                Color::NONE
            } else {
                Color::srgba(0.01, 0.02, 0.04, 0.55)
            }),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.),
                height: percent(100.),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .id();
    let panel = commands
        .spawn((
            ChildOf(backdrop),
            TabGroup::modal(),
            Node {
                width: if tablet { percent(100.) } else { px(width) },
                height: if tablet { percent(100.) } else { Val::Auto },
                max_width: if tablet { percent(100.) } else { percent(94.) },
                max_height: if tablet { percent(100.) } else { percent(94.) },
                flex_direction: FlexDirection::Column,
                row_gap: px(10.),
                border: UiRect::all(px(if tablet { 16. } else { 1. })),
                padding: UiRect::all(px(if tablet { 26. } else { 18. })),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(10, 24, 32)),
            BorderColor::all(if tablet {
                Color::srgb_u8(28, 42, 53)
            } else {
                Color::srgb_u8(45, 88, 94)
            }),
        ))
        .id();
    if tablet {
        commands.entity(panel).with_children(|frame| {
            frame
                .spawn(Node {
                    column_gap: px(8.),
                    height: px(24.),
                    align_items: AlignItems::Center,
                    flex_shrink: 0.,
                    ..default()
                })
                .with_children(|bar| {
                    bar.spawn((
                        Node {
                            width: px(5.),
                            height: px(5.),
                            ..default()
                        },
                        BackgroundColor(Color::srgb_u8(244, 94, 111)),
                    ));
                    bar.spawn((
                        Text::new("VibePad"),
                        TextFont::from_font_size(13.),
                        TextColor(Color::srgb_u8(161, 184, 193)),
                        Pickable::IGNORE,
                    ));
                });
        });
    }
    commands.entity(panel).with_child((
        Text::new(title),
        TextFont::from_font_size(19.),
        TextColor(Color::srgb_u8(156, 241, 223)),
        Pickable::IGNORE,
    ));
    panel
}

pub(super) fn text(parent: &mut ChildSpawnerCommands, text: &str, ink: Color) {
    parent.spawn((
        Text::new(text),
        panel_font(),
        TextColor(ink),
        Pickable::IGNORE,
    ));
}

fn panel_font() -> TextFont {
    TextFont {
        font: FontSource::list([Handle::<Font>::default(), EMOJI_FONT]),
        ..TextFont::from_font_size(15.)
    }
}

pub(super) fn input(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: &str,
    index: i32,
    multiline: bool,
) -> Entity {
    if !label.is_empty() {
        text(parent, label, Color::srgb_u8(157, 193, 196));
    }
    let mut editable = EditableText {
        allow_newlines: multiline,
        max_characters: Some(if multiline { 6000 } else { 500 }),
        ..default()
    };
    editable.editor_mut().set_text(value);
    parent
        .spawn((
            Name::new(label.to_owned()),
            TextInput,
            editable,
            TextCursorStyle::default(),
            TabIndex(index),
            Node {
                width: percent(100.),
                height: px(if multiline { 92. } else { 34. }),
                padding: UiRect::axes(px(9.), px(6.)),
                border: UiRect::all(px(1.)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(16, 40, 48)),
            BorderColor::all(Color::srgb_u8(57, 103, 108)),
            panel_font(),
            TextColor(Color::srgb_u8(229, 246, 246)),
        ))
        .id()
}

pub(super) fn button<
    A: Component + Copy,
    S: Resource<Mutability = bevy::ecs::component::Mutable> + Actions<A>,
>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    color: Color,
    action: A,
) -> Entity {
    parent
        .spawn((
            file_picker::button(
                label,
                &TextFont::from_font_size(15.),
                color,
                AlignSelf::Auto,
            ),
            action,
            observe(
                |click: On<Activate>, actions: Query<&A>, mut state: ResMut<S>| {
                    if let Ok(action) = actions.get(click.entity) {
                        state.activate(*action);
                    }
                },
            ),
        ))
        .id()
}

pub(super) trait Actions<A> {
    fn activate(&mut self, action: A);
}

pub(super) fn close(
    commands: &mut Commands,
    panel: Entity,
    focus: &mut InputFocus,
    parents: &Query<&ChildOf>,
) {
    if let Ok(parent) = parents.get(panel) {
        commands.entity(parent.parent()).despawn();
    }
    focus.clear();
}

pub(super) const INK: Color = Color::srgb_u8(157, 231, 214);

#[derive(Component, Default, Clone, Copy)]
pub(super) struct TabletEntrance(f32);

impl TabletEntrance {
    const SECONDS: f32 = 0.38;

    pub(super) fn pose(self) -> UiTransform {
        let t = (self.0 / Self::SECONDS).min(1.);
        let remaining = 1. - t * t * (3. - 2. * t);
        UiTransform {
            translation: Val2::new(px(0.), vh(remaining * 100.)),
            scale: Vec2::splat(1. - remaining * 0.28),
            rotation: Rot2::radians(remaining * 0.10),
        }
    }
}

pub(super) fn raise_tablet(
    mut commands: Commands,
    time: Res<Time>,
    mut panels: Query<(Entity, &mut TabletEntrance, &mut UiTransform)>,
) {
    for (entity, mut entrance, mut transform) in &mut panels {
        entrance.0 += time.delta_secs();
        *transform = entrance.pose();
        if entrance.0 >= TabletEntrance::SECONDS {
            commands.entity(entity).remove::<TabletEntrance>();
        }
    }
}
