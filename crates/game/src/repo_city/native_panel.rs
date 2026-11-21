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

#[derive(Component)]
pub(super) struct Form;

#[derive(Component)]
pub(super) struct Heading;

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
            Form,
            TabGroup::modal(),
            Node {
                width: if tablet { percent(100.) } else { px(width) },
                height: if tablet { percent(100.) } else { Val::Auto },
                max_width: if tablet { percent(100.) } else { percent(94.) },
                max_height: if tablet { percent(100.) } else { percent(94.) },
                flex_direction: FlexDirection::Column,
                row_gap: px(10.),
                border: UiRect::all(px(if tablet { 0. } else { 1. })),
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
        tablet_display::target(commands, backdrop);
    }
    heading(commands, panel, title);
    panel
}

fn heading(commands: &mut Commands, panel: Entity, title: &str) {
    if title.is_empty() {
        return;
    }
    commands.entity(panel).with_child((
        Heading,
        Text::new(title),
        TextFont::from_font_size(19.),
        TextColor(Color::srgb_u8(156, 241, 223)),
        Pickable::IGNORE,
    ));
}

pub(super) fn refresh_tablet(commands: &mut Commands, panel: Entity, title: &str) {
    commands.entity(panel).despawn_children();
    heading(commands, panel, title);
}

pub(super) fn text(parent: &mut ChildSpawnerCommands, text: &str, ink: Color) {
    parent.spawn((
        Text::new(text),
        panel_font(),
        TextColor(ink),
        Pickable::IGNORE,
    ));
}

pub(super) fn panel_font() -> TextFont {
    TextFont {
        font: FontSource::list([Handle::<Font>::default(), MATH_FONT, EMOJI_FONT]),
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
            Outline::new(px(2.), px(2.), Color::NONE),
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
    button_with_font::<A, S>(parent, label, &panel_font(), color, action)
}

pub(super) fn button_with_font<
    A: Component + Copy,
    S: Resource<Mutability = bevy::ecs::component::Mutable> + Actions<A>,
>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    font: &TextFont,
    color: Color,
    action: A,
) -> Entity {
    parent
        .spawn((
            file_picker::button(label, font, color, AlignSelf::Auto),
            Outline::new(px(2.), px(2.), Color::NONE),
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

pub(super) fn focus_outline(
    focus: Res<InputFocus>,
    visible: Res<bevy::input_focus::InputFocusVisible>,
    world: Option<Res<WorldSet>>,
    mut outlines: Query<(Entity, &mut Outline)>,
) {
    let ink = if world.as_deref().is_some_and(WorldSet::is_library) {
        Color::srgb_u8(242, 195, 130)
    } else {
        INK
    };
    for (entity, mut outline) in &mut outlines {
        let color = if visible.0 && focus.get() == Some(entity) {
            ink
        } else {
            Color::NONE
        };
        if outline.color != color {
            outline.color = color;
        }
    }
}

/// Arrow navigation uses the same modal focus order as Tab, preserving text and shell input.
pub(super) fn keyboard_navigation(
    keys: Res<ButtonInput<KeyCode>>,
    navigation: bevy::input_focus::tab_navigation::TabNavigation,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<bevy::input_focus::InputFocusVisible>,
    fields: Query<Option<&Name>, With<TextInput>>,
    editors: Query<Entity, With<source_editor::View>>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    parents: Query<&ChildOf>,
    nodes: Query<(&UiGlobalTransform, &ComputedNode)>,
    mut scrolls: Query<&mut ScrollPosition>,
    #[cfg(not(target_arch = "wasm32"))] terminal: Option<Res<terminal::Terminal>>,
    #[cfg(not(target_arch = "wasm32"))] terminal_views: Query<Entity, With<terminal::View>>,
) {
    if keys.just_pressed(KeyCode::F6)
        && let Ok(view) = editors.single()
    {
        let next = if focus.get() == Some(view) {
            navigation
                .navigate(&focus, bevy::input_focus::tab_navigation::NavAction::First)
                .unwrap_or(view)
        } else {
            view
        };
        focus.set(next, bevy::input_focus::FocusCause::Navigated);
        visible.0 = true;
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if keys.just_pressed(KeyCode::F6)
        && let Ok(view) = terminal_views.single()
    {
        let next = if focus.get() == Some(view) {
            navigation
                .navigate(&focus, bevy::input_focus::tab_navigation::NavAction::First)
                .unwrap_or(view)
        } else {
            view
        };
        focus.set(next, bevy::input_focus::FocusCause::Navigated);
        visible.0 = true;
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if terminal.is_some_and(|terminal| terminal.active) {
        return;
    }
    if keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ]) {
        return;
    }
    if focus.get().is_some_and(|entity| editors.contains(entity)) {
        return;
    }
    if let Some(entity) = focus.get()
        && let Ok(name) = fields.get(entity)
        && !(name
            .is_some_and(|name| ["File name or path", "Filter options"].contains(&name.as_str()))
            && keys.just_pressed(KeyCode::ArrowDown))
    {
        return;
    }
    use bevy::input_focus::{FocusCause, tab_navigation::NavAction};
    let action = if keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::ArrowRight]) {
        NavAction::Next
    } else if keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::ArrowLeft]) {
        NavAction::Previous
    } else {
        return;
    };
    for _ in 0..128 {
        let Ok(next) = navigation.navigate(&focus, action) else {
            return;
        };
        focus.set(next, FocusCause::Navigated);
        if disabled.contains(next) {
            continue;
        }
        visible.0 = true;
        if let Ok((transform, node)) = nodes.get(next) {
            let target: bevy::math::Affine2 = transform.into();
            let top = target.translation.y - node.size().y / 2.;
            let bottom = top + node.size().y;
            for parent in parents.iter_ancestors(next) {
                if let (Ok(mut scroll), Ok((transform, area))) =
                    (scrolls.get_mut(parent), nodes.get(parent))
                {
                    let area_transform: bevy::math::Affine2 = transform.into();
                    let area_top = area_transform.translation.y - area.size().y / 2.;
                    let area_bottom = area_top + area.size().y;
                    let delta = if top < area_top {
                        top - area_top
                    } else if bottom > area_bottom {
                        bottom - area_bottom
                    } else {
                        0.
                    };
                    scroll.y = (scroll.y + delta * area.inverse_scale_factor()).max(0.);
                    break;
                }
            }
        }
        break;
    }
}
