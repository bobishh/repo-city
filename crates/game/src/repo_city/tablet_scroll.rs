//! VibePad scrollbars. Bevy owns dragging/paging; this module owns the case and grip.
use super::*;
use bevy::{
    picking::{Pickable, hover::Hovered},
    ui::{BackgroundGradient, ColorStop, LinearGradient},
    ui_widgets::{
        Activate, Button, ControlOrientation, Scrollbar, ScrollbarDragState, ScrollbarThumb,
    },
};

const FACE: Color = Color::srgb_u8(43, 105, 111);
const LIGHT: Color = Color::srgb_u8(125, 216, 200);
const DARK: Color = Color::srgb_u8(8, 35, 43);

#[derive(Component, Default)]
pub(super) struct Area;

/// Actual screen controls, also exposed by the browser's tablet geometry inspector.
#[derive(Component)]
pub(super) struct Part {
    pub area: Entity,
    pub kind: &'static str,
}

pub(super) fn register(app: &mut App) {
    app.add_systems(PostUpdate, attach.before(bevy::ui::UiSystems::Prepare))
        .add_systems(
            PostUpdate,
            grip.after(bevy::ui::UiSystems::Layout)
                .before(bevy::ui::UiSystems::PostLayout),
        );
}

fn bevel(raised: bool) -> BorderColor {
    let (light, dark) = if raised { (LIGHT, DARK) } else { (DARK, LIGHT) };
    BorderColor {
        top: light,
        left: light,
        bottom: dark,
        right: dark,
    }
}

fn attach(
    mut commands: Commands,
    mut areas: Query<(Entity, &ChildOf, &mut Node), Added<Area>>,
    children: Query<&Children>,
) {
    for (area, parent, mut node) in &mut areas {
        let parent = parent.parent();
        let index = children
            .get(parent)
            .ok()
            .and_then(|children| children.iter().position(|child| child == area))
            .unwrap_or(0);
        let grow = node.flex_grow;
        node.width = px(0.);
        node.flex_basis = px(0.);
        node.flex_grow = 1.;
        node.min_width = px(0.);
        let row = commands
            .spawn((
                Name::new("VibePad scroll viewport"),
                Node {
                    width: percent(100.),
                    min_height: px(0.),
                    flex_grow: grow,
                    column_gap: px(12.),
                    ..default()
                },
            ))
            .id();
        commands.entity(parent).insert_children(index, &[row]);
        commands.entity(row).add_child(area).with_children(|row| {
            row.spawn((
                Name::new("VibePad scrollbar"),
                Node {
                    width: px(26.),
                    min_width: px(26.),
                    flex_shrink: 0.,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3.),
                    padding: UiRect::all(px(2.)),
                    border: UiRect::all(px(1.)),
                    ..default()
                },
                BackgroundColor(DARK),
                BorderColor::all(Color::srgb_u8(61, 112, 116)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Name::new("Scroll track"),
                    Part {
                        area,
                        kind: "track",
                    },
                    Scrollbar::new(area, ControlOrientation::Vertical, 38.),
                    Node {
                        width: percent(100.),
                        flex_grow: 1.,
                        min_height: px(38.),
                        border: UiRect::all(px(2.)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(11, 27, 34)),
                    bevel(false),
                ))
                .with_child((
                    Name::new("Scroll thumb"),
                    Part {
                        area,
                        kind: "thumb",
                    },
                    ScrollbarThumb {
                        border: UiRect::all(px(2.)),
                        ..default()
                    },
                    Hovered::default(),
                    BackgroundColor(FACE),
                    bevel(true),
                    BackgroundGradient::default(),
                ));
                for (kind, label) in [("up", "▲"), ("down", "▼")] {
                    bar.spawn((
                        Button,
                        Name::new(if kind == "up" {
                            "Scroll up"
                        } else {
                            "Scroll down"
                        }),
                        Part { area, kind },
                        Node {
                            width: percent(100.),
                            height: px(26.),
                            flex_shrink: 0.,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(2.)),
                            ..default()
                        },
                        BackgroundColor(FACE),
                        bevel(true),
                    ))
                    .observe(step)
                    .with_child((
                        Text::new(label),
                        native_panel::panel_font(),
                        TextColor(native_panel::INK),
                        Pickable::IGNORE,
                    ));
                }
            });
        });
    }
}

pub(super) fn range(node: &ComputedNode) -> f32 {
    (node.content_size().y - node.size().y).max(0.) * node.inverse_scale_factor()
}

fn step(
    event: On<Activate>,
    parts: Query<&Part>,
    mut areas: Query<(&mut ScrollPosition, &ComputedNode), With<Area>>,
) {
    let Ok(part) = parts.get(event.entity) else {
        return;
    };
    let Ok((mut position, node)) = areas.get_mut(part.area) else {
        return;
    };
    position.y = (position.y + if part.kind == "up" { -34. } else { 34. }).clamp(0., range(node));
}

fn grip(
    mut thumbs: Query<
        (
            &ComputedNode,
            &Hovered,
            &ScrollbarDragState,
            &mut BackgroundColor,
            &mut BackgroundGradient,
        ),
        (
            With<ScrollbarThumb>,
            Or<(
                Changed<ComputedNode>,
                Changed<Hovered>,
                Changed<ScrollbarDragState>,
            )>,
        ),
    >,
) {
    for (node, hovered, drag, mut color, mut gradient) in &mut thumbs {
        let face = if hovered.0 || drag.dragging {
            Color::srgb_u8(57, 137, 137)
        } else {
            FACE
        };
        color.0 = face;
        let middle = node.size().y * node.inverse_scale_factor() / 2.;
        let mut stripes = vec![ColorStop::new(face, px(0.))];
        for y in [-7., -1., 5.].map(|offset| middle + offset) {
            stripes.extend([
                ColorStop::new(face, px(y)),
                ColorStop::new(DARK, px(y)),
                ColorStop::new(DARK, px(y + 2.)),
                ColorStop::new(LIGHT, px(y + 2.)),
                ColorStop::new(LIGHT, px(y + 3.)),
                ColorStop::new(face, px(y + 3.)),
            ]);
        }
        stripes.push(ColorStop::new(face, percent(100.)));
        // The first gradient masks the ends of the three recessed grip lines.
        gradient.0 = vec![
            LinearGradient::to_right(vec![
                ColorStop::new(face, percent(0.)),
                ColorStop::new(face, percent(24.)),
                ColorStop::new(Color::NONE, percent(24.)),
                ColorStop::new(Color::NONE, percent(76.)),
                ColorStop::new(face, percent(76.)),
                ColorStop::new(face, percent(100.)),
            ])
            .into(),
            LinearGradient::to_bottom(stripes).into(),
        ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrows_scroll_and_clamp_without_overflow() {
        let mut app = App::new();
        let area = app
            .world_mut()
            .spawn((
                Area,
                ScrollPosition::default(),
                ComputedNode {
                    size: Vec2::new(400., 200.),
                    content_size: Vec2::new(400., 500.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
            ))
            .id();
        let down = app
            .world_mut()
            .spawn(Part { area, kind: "down" })
            .observe(step)
            .id();
        let up = app
            .world_mut()
            .spawn(Part { area, kind: "up" })
            .observe(step)
            .id();
        app.world_mut().trigger(Activate { entity: down });
        assert_eq!(app.world().get::<ScrollPosition>(area).unwrap().y, 34.);
        for _ in 0..20 {
            app.world_mut().trigger(Activate { entity: down });
        }
        assert_eq!(app.world().get::<ScrollPosition>(area).unwrap().y, 300.);
        app.world_mut().trigger(Activate { entity: up });
        assert_eq!(app.world().get::<ScrollPosition>(area).unwrap().y, 266.);
        app.world_mut()
            .get_mut::<ComputedNode>(area)
            .unwrap()
            .content_size
            .y = 100.;
        app.world_mut().trigger(Activate { entity: down });
        assert_eq!(app.world().get::<ScrollPosition>(area).unwrap().y, 0.);
    }
}
