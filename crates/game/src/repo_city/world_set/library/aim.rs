use bevy::{picking::Pickable, ui::BorderColor};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Cabinet(usize),
    File(usize),
    Line {
        file: usize,
        row: usize,
        before: bool,
        number: usize,
    },
}

#[derive(Resource, Default)]
pub(super) struct Aim(pub Option<Target>);

#[derive(Component)]
pub(super) struct Reticle;

#[derive(Component)]
pub(super) struct Caption;

pub(super) fn spawn(commands: &mut Commands) {
    commands.spawn((
        Reticle,
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50.),
            top: percent(50.),
            margin: UiRect {
                left: px(-5),
                top: px(-5),
                ..default()
            },
            width: px(10),
            height: px(10),
            border: UiRect::all(px(1)),
            ..default()
        },
        BorderColor::all(Color::srgb_u8(240, 220, 182)),
        Visibility::Hidden,
    ));
    commands.spawn((
        Caption,
        Pickable::IGNORE,
        Text::new(""),
        native_panel::panel_font(),
        TextColor(Color::srgb_u8(247, 224, 176)),
        BackgroundColor(Color::srgba(0.11, 0.085, 0.07, 0.92)),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(24),
            top: px(94),
            padding: UiRect::axes(px(9), px(5)),
            max_width: percent(58),
            ..default()
        },
    ));
}

#[derive(Clone, Copy)]
struct Hit {
    target: Target,
    center: Vec3,
    size: Vec3,
    distance: f32,
}

fn ray_box(origin: Vec3, direction: Vec3, center: Vec3, size: Vec3) -> Option<f32> {
    let relative = origin - center;
    let half = size / 2.;
    let mut entry = 0.0_f32;
    let mut exit = 64.0_f32;
    for (origin, (direction, half)) in relative
        .to_array()
        .into_iter()
        .zip(direction.to_array().into_iter().zip(half.to_array()))
    {
        if direction.abs() < 0.0001 {
            if origin.abs() > half {
                return None;
            }
            continue;
        }
        let a = (-half - origin) / direction;
        let b = (half - origin) / direction;
        entry = entry.max(a.min(b));
        exit = exit.min(a.max(b));
        if entry > exit {
            return None;
        }
    }
    Some(entry)
}

pub(super) fn update(
    library: Res<Library>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    world: Res<WorldSet>,
    (scroll, map): (Res<room::Unrolling>, Res<map::Map>),
    eye: Query<(&Camera, &GlobalTransform, &Pilot), With<room::Camera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    review: Res<review_notes::Review>,
    mut aim: ResMut<Aim>,
    mut nearest: ResMut<Nearest>,
    (cabinets, rolls): (
        Query<(&room::Cabinet, &GlobalTransform, &Visibility, &Children)>,
        Query<(&room::CabinetFile, &GlobalTransform, &Visibility)>,
    ),
    mut crosshair: Query<
        &mut Visibility,
        (
            With<Reticle>,
            Without<room::Cabinet>,
            Without<room::CabinetFile>,
        ),
    >,
    mut caption: Query<(&mut Text, &mut Node), With<Caption>>,
    mut gizmos: Gizmos,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
    #[cfg(target_arch = "wasm32")] mut announced: Local<Option<Target>>,
    #[cfg(target_arch = "wasm32")] touch: Res<browser_touch::Touch>,
) {
    let blocked = map.open() || review.blocked || review.tablet_open || library.input.is_some();
    #[cfg(not(target_arch = "wasm32"))]
    let blocked = blocked || picker.blocks_controls();
    let view = eye.single().ok().zip(windows.single().ok());
    let active =
        !blocked && view.is_some_and(|((_, _, pilot), window)| window.focused || pilot.captured);
    let centered = view.is_some_and(|((_, _, pilot), _)| pilot.captured);
    #[cfg(target_arch = "wasm32")]
    let centered = centered || touch.enabled;
    if let Ok(mut marker) = crosshair.single_mut() {
        *marker = if active && centered {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    let hit = (|| {
        let ((camera, pose, _), window) = view?;
        if !active {
            return None;
        }
        let point = if centered {
            Vec2::new(window.width(), window.height()) / 2.
        } else {
            window.cursor_position()?
        };
        let ray = camera.viewport_to_world(pose, point).ok()?;
        let direction = *ray.direction;
        if let Some(reader) = &library.reader
            && scroll.reveal() >= 1.
        {
            let content = paper::Rows::new(&explorer, reader.face);
            let distance = (room::PAPER_Y - ray.origin.y) / direction.y;
            let point = ray.origin + distance * direction;
            if distance > 0.
                && (point.x - scroll.x).abs() < room::PAPER_WIDTH_WORLD / 2.
                && let Some((line, z)) = room::line_at(point.z, scroll.origin, content.len())
                && let Some(row) = content.row(line - 1)
            {
                return Some(Hit {
                    target: Target::Line {
                        file: explorer.selected,
                        row: line - 1,
                        before: row.before,
                        number: row.line.number,
                    },
                    center: Vec3::new(scroll.x, room::PAPER_Y + 0.023, z),
                    size: Vec3::new(room::PAPER_WIDTH_WORLD - 0.12, 0.012, room::line_height()),
                    distance,
                });
            }
            return None;
        }

        let mut selected = None::<Hit>;
        for (cabinet, pose, visibility, children) in &cabinets {
            if *visibility == Visibility::Hidden {
                continue;
            }
            let Some(group) = library.catalog.folders.get(cabinet.directory) else {
                continue;
            };
            let (center, size) = cabinet.bounds(pose);
            let Some(distance) = ray_box(ray.origin, direction, center, size) else {
                continue;
            };
            if selected.is_some_and(|hit| hit.distance <= distance) {
                continue;
            }
            let mut cabinet_hit = Hit {
                target: Target::Cabinet(cabinet.directory),
                center,
                size,
                distance,
            };
            let mut file_distance = f32::INFINITY;
            for child in children.iter() {
                let Ok((roll, pose, visibility)) = rolls.get(child) else {
                    continue;
                };
                if *visibility == Visibility::Hidden {
                    continue;
                }
                let Some(&file) = group.files.get(cabinet.page + roll.0) else {
                    continue;
                };
                let center = pose.translation();
                let size = Vec3::new(0.86, 0.23 * size.y / 4.12, 0.23);
                if let Some(hit) = ray_box(ray.origin, direction, center, size)
                    && hit < file_distance
                {
                    file_distance = hit;
                    cabinet_hit = Hit {
                        target: Target::File(file),
                        center,
                        size,
                        distance,
                    };
                }
            }
            selected = Some(cabinet_hit);
        }
        selected
    })();

    aim.0 = hit.map(|hit| hit.target);
    nearest.file = aim.0.and_then(|target| match target {
        Target::File(index) | Target::Line { file: index, .. } => Some(file_facade::Aim {
            index,
            surface: None,
        }),
        _ => None,
    });
    nearest.file_distance = hit.map_or(0., |hit| hit.distance);
    let caption_text = hit.and_then(|hit| match hit.target {
        Target::Cabinet(group) => library
            .catalog
            .folders
            .get(group)
            .map(|group| group.path.clone()),
        Target::File(index) => repo.0.files.get(index).map(|file| file.path.clone()),
        Target::Line { number, .. } => Some(format!("Line {number}")),
    });
    if let Some(hit) = hit {
        gizmos.cube(
            Transform::from_translation(hit.center).with_scale(hit.size + Vec3::splat(0.025)),
            color(world.accent),
        );
    }
    if let Ok((mut text, mut node)) = caption.single_mut() {
        let display = if caption_text.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(text_value) = &caption_text
            && text.0 != *text_value
        {
            text.0.clone_from(text_value);
        }
        if node.display != display {
            node.display = display;
        }
    }
    #[cfg(target_arch = "wasm32")]
    if *announced != aim.0 {
        *announced = aim.0;
        browser_target(caption_text.as_deref().unwrap_or_default());
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryTarget)]
    fn browser_target(target: &str);
}

#[test]
fn a_pointer_ray_hits_the_visible_shelf_and_rejects_a_target_behind_the_eye() {
    let center = Vec3::new(8., 2., 10.);
    let eye = Vec3::new(0., 2., 0.);
    let direction = (center - eye).normalize();
    assert!(ray_box(eye, direction, center, Vec3::ONE).is_some());
    assert!(ray_box(eye, -direction, center, Vec3::ONE).is_none());
    assert!(ray_box(eye, Vec3::Z, center, Vec3::ONE).is_none());
}
