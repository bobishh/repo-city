//! Shared styling for the city's native panels.
use bevy::{
    input_focus::{
        InputFocus,
        tab_navigation::{TabGroup, TabIndex},
    },
    picking::Pickable,
    text::{EditableText, TextCursorStyle},
    ui_widgets::{Activate, TextInput, observe},
};

use super::*;

pub(super) fn panel(commands: &mut Commands, title: &str, width: f32) -> Entity {
    let backdrop = commands
        .spawn((
            GlobalZIndex(40),
            BackgroundColor(Color::srgba(0.01, 0.02, 0.04, 0.55)),
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
                width: px(width),
                max_width: percent(94.),
                max_height: percent(94.),
                flex_direction: FlexDirection::Column,
                row_gap: px(10.),
                border: UiRect::all(px(1.)),
                padding: UiRect::all(px(18.)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(10, 24, 32)),
            BorderColor::all(Color::srgb_u8(45, 88, 94)),
        ))
        .id();
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
        TextFont::from_font_size(15.),
        TextColor(ink),
        Pickable::IGNORE,
    ));
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
            TextFont::from_font_size(15.),
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

#[derive(Component, Default)]
pub(super) struct TabletEntrance(f32);

pub(super) fn raise_tablet(
    mut commands: Commands,
    time: Res<Time>,
    mut panels: Query<(Entity, &mut TabletEntrance, &mut UiTransform)>,
) {
    for (entity, mut entrance, mut transform) in &mut panels {
        entrance.0 += time.delta_secs();
        let progress = (entrance.0 / 0.20).min(1.);
        let remaining = (1. - progress).powi(3);
        transform.translation = Val2::px(0., remaining * 220.);
        transform.scale = Vec2::splat(1. - remaining * 0.1);
        if progress == 1. {
            commands.entity(entity).remove::<TabletEntrance>();
        }
    }
}
