use super::*;
use bevy::input::mouse::MouseScrollUnit;

pub(super) fn route(
    mut pending: ResMut<Scroll>,
    mut wheel: ResMut<AccumulatedMouseScroll>,
    camera: Query<(&Camera, &Transform, &Pilot), PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    boards: Query<(&Board, &Transform)>,
    navigation: Option<Res<city_navigation::Navigation>>,
    picker: Res<file_picker::FilePicker>,
    menu: Res<city_menu::Menu>,
    review: Res<review_notes::Review>,
    search: Res<listing::find::Search>,
    #[cfg(not(target_arch = "wasm32"))] (repository, session): (
        Option<Res<repository_picker::RepositoryPicker>>,
        Option<Res<agent_session::Session>>,
    ),
    #[cfg(target_arch = "wasm32")] (mut touch, mut previous): (
        Option<ResMut<browser_touch::Touch>>,
        Local<String>,
    ),
) {
    let active = !picker.is_open()
        && !menu.is_open()
        && !review.blocked
        && !search.blocks_controls()
        && !navigation.is_some_and(|navigation| navigation.overview() || navigation.is_flying());
    #[cfg(not(target_arch = "wasm32"))]
    let active = active
        && !repository.is_some_and(|picker| picker.blocks_controls())
        && super::super::city_accepts_input(session);
    let view = camera
        .single()
        .ok()
        .zip(windows.single().ok())
        .filter(|((_, _, pilot), window)| active && (window.focused || pilot.captured));
    #[cfg(target_arch = "wasm32")]
    if let Some(touch) = touch.as_deref_mut() {
        publish_touch_areas(
            view.map(|((camera, pose, _), _)| (camera, pose)),
            &boards,
            &mut previous,
        );
        if let Some((index, distance)) = touch.board.take()
            && view.is_some()
            && distance.is_finite()
            && let Some(delta) = pending.0.get_mut(index)
        {
            *delta += distance;
        }
    }
    let Some(((camera, pose, pilot), window)) = view else {
        return;
    };
    if wheel.delta.y == 0. {
        return;
    }
    let ray = if pilot.captured {
        Some((pose.translation, pose.forward().as_vec3()))
    } else {
        window
            .cursor_position()
            .and_then(|point| {
                camera
                    .viewport_to_world(&GlobalTransform::from(*pose), point)
                    .ok()
            })
            .map(|ray| (ray.origin, ray.direction.as_vec3()))
    };
    if let Some((origin, direction)) = ray
        && let Some((_, index)) = boards
            .iter()
            .filter_map(|(board, transform)| hit(board, transform, origin, direction))
            .min_by(|(a, _), (b, _)| a.total_cmp(b))
    {
        let scale = if wheel.unit == MouseScrollUnit::Line {
            46.
        } else {
            1.
        };
        pending.0[index] -= wheel.delta.y * scale;
        wheel.delta.y = 0.;
    }
}

fn hit(
    board: &Board,
    transform: &Transform,
    origin: Vec3,
    direction: Vec3,
) -> Option<(f32, usize)> {
    let normal = transform.local_z().as_vec3();
    let facing = direction.dot(normal);
    if !board.scrollable || facing >= -0.0001 {
        return None;
    }
    let distance = (transform.translation - origin).dot(normal) / facing;
    let local = transform
        .compute_affine()
        .inverse()
        .transform_point3(origin + distance * direction);
    (distance > 0. && local.xy().abs().cmple(board.size / 2.).all())
        .then_some((distance, board.index))
}

#[cfg(target_arch = "wasm32")]
fn publish_touch_areas(
    view: Option<(&Camera, &Transform)>,
    boards: &Query<(&Board, &Transform)>,
    previous: &mut String,
) {
    let areas = if let Some((camera, pose)) = view {
        let view = GlobalTransform::from(*pose);
        let mut areas: Vec<_> = boards
            .iter()
            .filter_map(|(board, transform)| {
                let offset = pose.translation - transform.translation;
                if !board.scrollable || transform.local_z().dot(offset) <= 0. {
                    return None;
                }
                let affine = transform.compute_affine();
                let mut corners = [[0.; 2]; 4];
                for (corner, (x, y)) in
                    corners
                        .iter_mut()
                        .zip([(-1., 1.), (1., 1.), (1., -1.), (-1., -1.)])
                {
                    let local = Vec3::new(x * board.size.x / 2., y * board.size.y / 2., 0.);
                    let point = camera
                        .world_to_viewport(&view, affine.transform_point3(local))
                        .ok()?;
                    *corner = point.round().to_array();
                }
                Some((offset.length_squared(), board.index, corners))
            })
            .collect();
        areas.sort_by(|(a, _, _), (b, _, _)| a.total_cmp(b));
        areas
            .into_iter()
            .map(|(_, index, corners)| (index, corners))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let value = serde_json::to_string(&areas).expect("board screen coordinates");
    if *previous != value
        && let Some(canvas) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("repo-city"))
    {
        let _ = canvas.set_attribute("data-board-areas", &value);
        *previous = value;
    }
}
