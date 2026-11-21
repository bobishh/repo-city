use bevy::{picking::Pickable, ui::UiTargetCamera};

use super::*;

const COUNT: usize = 24;
const HEIGHT: f32 = 23.;
const CABINET_RANGE: f32 = 48.;
const FILE_RANGE: f32 = 16.;

#[derive(Component)]
pub(super) struct Sign(usize);

pub(super) fn spawn(commands: &mut Commands, eye: Entity) {
    for slot in 0..COUNT {
        commands.spawn((
            Sign(slot),
            UiTargetCamera(eye),
            Pickable::IGNORE,
            Text::new(""),
            native_panel::panel_font(),
            TextColor(Color::srgb_u8(247, 230, 202)),
            BackgroundColor(Color::srgb_u8(40, 29, 22)),
            BorderColor::all(Color::srgb_u8(132, 99, 66)),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                height: px(HEIGHT),
                padding: UiRect::horizontal(px(5.)),
                border: UiRect::all(px(1.)),
                overflow: Overflow::clip(),
                ..default()
            },
            GlobalZIndex(3),
        ));
    }
}

struct Candidate<'a> {
    name: &'a str,
    center: Vec2,
    distance: f32,
    active: bool,
    cabinet: bool,
}

pub(super) fn draw(
    library: Res<Library>,
    repo: Res<Repo>,
    aim: Res<aim::Aim>,
    review: Res<review_notes::Review>,
    map: Res<map::Map>,
    eye: Query<(&Camera, &Transform), With<room::Camera>>,
    cabinets: Query<(&room::Cabinet, &GlobalTransform, &Visibility, &Children)>,
    rolls: Query<(&room::CabinetFile, &GlobalTransform, &Visibility)>,
    mut signs: Query<(
        &Sign,
        &mut Node,
        &mut Text,
        &mut TextColor,
        &mut BackgroundColor,
    )>,
) {
    let mut candidates = Vec::new();
    let mut viewport = Rect::EMPTY;
    if !map.open()
        && library.reader.is_none()
        && library.input.is_none()
        && !review.blocked
        && !review.tablet_open
        && let Ok((camera, camera_pose)) = eye.single()
        && let Some(bounds) = camera.logical_viewport_rect()
    {
        viewport = bounds;
        let pose = GlobalTransform::from(*camera_pose);
        let project = |point: Vec3, range: f32| {
            let distance = (point - camera_pose.translation).length();
            if distance > range {
                return None;
            }
            let center = camera.world_to_viewport(&pose, point).ok()?;
            viewport.contains(center).then_some((center, distance))
        };
        for (cabinet, transform, visible, children) in &cabinets {
            if *visible == Visibility::Hidden {
                continue;
            }
            let Some(group) = library.catalog.folders.get(cabinet.directory) else {
                continue;
            };
            let (center, size) = cabinet.bounds(transform);
            let front = Vec3::new(-cabinet.side * (size.x / 2. + 0.05), size.y / 2. + 0.28, 0.);
            let aimed = aim.0 == Some(aim::Target::Cabinet(cabinet.directory))
                || matches!(aim.0, Some(aim::Target::File(file)) if group.files.contains(&file));
            if let Some((center, distance)) = project(center + front, CABINET_RANGE) {
                candidates.push(Candidate {
                    name: &cabinet.label,
                    center,
                    distance,
                    active: aimed,
                    cabinet: true,
                });
            }
            let limit = if aimed { CABINET_RANGE } else { FILE_RANGE };
            for child in children.iter() {
                let Ok((slot, pose, visibility)) = rolls.get(child) else {
                    continue;
                };
                if *visibility == Visibility::Hidden {
                    continue;
                }
                let Some(&index) = group.files.get(cabinet.page + slot.0) else {
                    continue;
                };
                let Some((center, distance)) = project(
                    pose.translation() + Vec3::new(-cabinet.side * 0.37, 0.20, 0.),
                    limit,
                ) else {
                    continue;
                };
                let file = &repo.0.files[index];
                candidates.push(Candidate {
                    name: file.path.rsplit('/').next().unwrap_or(&file.path),
                    center,
                    distance,
                    active: aim.0 == Some(aim::Target::File(index)),
                    cabinet: false,
                });
            }
        }
    }
    candidates.sort_unstable_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.cabinet.cmp(&a.cabinet))
            .then(a.distance.total_cmp(&b.distance))
    });
    let mut placed = Vec::<(Candidate, Rect)>::with_capacity(COUNT);
    for candidate in candidates {
        let width = (candidate.name.chars().count() as f32 * 9.3 + 12.)
            .clamp(29., (viewport.width() * 0.37).max(29.));
        let left = (candidate.center.x - width / 2.).clamp(
            viewport.min.x + 4.,
            (viewport.max.x - width - 4.).max(viewport.min.x + 4.),
        );
        let top = (candidate.center.y - HEIGHT).max(viewport.min.y + 2.);
        let Some(bounds) = [0., HEIGHT, 2. * HEIGHT]
            .into_iter()
            .map(|offset| {
                let start = Vec2::new(left, top - offset);
                Rect::from_corners(start, start + Vec2::new(width, HEIGHT))
            })
            .find(|bounds| {
                bounds.min.y >= viewport.min.y
                    && placed
                        .iter()
                        .all(|(_, previous)| !overlap(*bounds, *previous))
            })
        else {
            continue;
        };
        placed.push((candidate, bounds));
        if placed.len() == COUNT {
            break;
        }
    }
    for (slot, mut node, mut text, mut ink, mut background) in &mut signs {
        let Some((candidate, bounds)) = placed.get(slot.0) else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        draw_one(
            &mut node,
            &mut text,
            &mut ink,
            &mut background,
            candidate,
            *bounds,
        );
    }
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.min.x < b.max.x + 3. && a.max.x > b.min.x - 3. && a.min.y < b.max.y && a.max.y > b.min.y
}

fn draw_one(
    node: &mut Mut<Node>,
    text: &mut Mut<Text>,
    ink: &mut Mut<TextColor>,
    background: &mut Mut<BackgroundColor>,
    candidate: &Candidate,
    bounds: Rect,
) {
    let (left, top, width) = (px(bounds.min.x), px(bounds.min.y), px(bounds.width()));
    if node.display != Display::Flex || node.left != left || node.top != top || node.width != width
    {
        node.display = Display::Flex;
        node.left = left;
        node.top = top;
        node.width = width;
    }
    if text.0 != candidate.name {
        text.0.clear();
        text.0.push_str(candidate.name);
    }
    ink.set_if_neq(TextColor(if candidate.active {
        Color::srgb_u8(34, 22, 13)
    } else if candidate.cabinet {
        Color::srgb_u8(244, 207, 151)
    } else {
        Color::srgb_u8(247, 230, 202)
    }));
    background.set_if_neq(BackgroundColor(if candidate.active {
        Color::srgb_u8(227, 176, 110)
    } else {
        Color::srgb_u8(40, 29, 22)
    }));
}
