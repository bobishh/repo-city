use super::*;

const PICKUP_DISTANCE: f32 = 4.5;

#[derive(Resource, Default)]
struct Buoys {
    scope: String,
    positions: Vec<[f32; 3]>,
}

#[derive(Component)]
struct Float;

pub(super) fn install(app: &mut App) {
    app.init_resource::<Buoys>().add_systems(
        Update,
        (
            restore,
            deploy.run_if(city_accepts_input).run_if(piloting),
            render,
        )
            .chain()
            .after(flight),
    );
}

fn storage_key(world: &WorldSet) -> String {
    format!("{}|survey-buoys", world.id)
}

fn restore(
    repo: Res<Repo>,
    world: Res<WorldSet>,
    mut progress: ResMut<progress::Progress>,
    mut buoys: ResMut<Buoys>,
) {
    if !repo.is_changed() {
        return;
    }
    let scope = progress::comparison_scope(&repo.0);
    if buoys.scope != scope {
        buoys.positions = progress
            .world_place::<Vec<[f32; 3]>>(&repo.0, &storage_key(&world))
            .unwrap_or_default()
            .into_iter()
            .filter(|&point| Vec3::from_array(point).is_finite())
            .collect();
        buoys.scope = scope;
    }
}

fn toggle(positions: &mut Vec<[f32; 3]>, eye: &Transform) {
    let nearest = positions
        .iter()
        .enumerate()
        .map(|(i, &point)| (i, eye.translation.distance_squared(point.into())))
        .filter(|(_, d)| *d < PICKUP_DISTANCE.powi(2))
        .min_by(|(_, a), (_, b)| a.total_cmp(b));
    if let Some((i, _)) = nearest {
        positions.swap_remove(i);
    } else {
        let mut point = eye.transform_point(Vec3::new(0.78, -0.46, -3.6));
        point.y = point.y.max(0.72);
        positions.push(point.to_array());
    }
}

fn deploy(
    keys: Res<ButtonInput<KeyCode>>,
    camera: Query<(&Transform, &Pilot), PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    navigation: Res<city_navigation::Navigation>,
    review: Res<review_notes::Review>,
    picker: Res<file_picker::FilePicker>,
    #[cfg(not(target_arch = "wasm32"))] repository: Res<repository_picker::RepositoryPicker>,
    menu: Res<city_menu::Menu>,
    search: Res<listing::find::Search>,
    repo: Res<Repo>,
    world: Res<WorldSet>,
    mut progress: ResMut<progress::Progress>,
    mut buoys: ResMut<Buoys>,
    mut explorer: ResMut<Explorer>,
) {
    if !keys.just_pressed(KeyCode::KeyO)
        || keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ])
        || repo.0.files.is_empty()
        || review.blocked
        || review.tablet_open
        || picker.is_open()
        || menu.blocks_controls()
        || search.blocks_controls()
        || navigation.guides_camera()
    {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if repository.blocks_controls() {
        return;
    }
    let Ok((eye, pilot)) = camera.single() else {
        return;
    };
    if !pilot.captured || !windows.single().is_ok_and(|window| window.focused) {
        return;
    }
    let key = storage_key(&world);
    let mut next = progress
        .world_place(&repo.0, &key)
        .unwrap_or_else(|| buoys.positions.clone());
    toggle(&mut next, eye);
    match progress.save_world_place(&repo.0, &key, &next) {
        Ok(()) => buoys.positions = next,
        Err(error) => explorer.notice = format!("Could not save survey buoy: {error}"),
    }
}

struct Hardware {
    float: Handle<Mesh>,
    stem: Handle<Mesh>,
    lens: Handle<StandardMaterial>,
    housing: Handle<StandardMaterial>,
}

fn render(
    mut commands: Commands,
    buoys: Res<Buoys>,
    old: Query<Entity, With<Float>>,
    mut hardware: Local<Option<Hardware>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !buoys.is_changed() {
        return;
    }
    for entity in &old {
        commands.entity(entity).despawn();
    }
    if buoys.positions.is_empty() {
        return;
    }
    let hardware = hardware.get_or_insert_with(|| Hardware {
        float: meshes.add(Sphere::new(1.).mesh().uv(16, 10)),
        stem: meshes.add(Cylinder::new(0.055, 0.54)),
        lens: materials.add(StandardMaterial {
            base_color: Color::srgb(1., 0.54, 0.76),
            emissive: LinearRgba::new(1.55, 0.27, 0.72, 1.),
            ..default()
        }),
        housing: materials.add(StandardMaterial {
            base_color: Color::srgb(0.47, 0.80, 0.73),
            metallic: 0.54,
            ..default()
        }),
    });
    for &point in &buoys.positions {
        commands
            .spawn((
                Float,
                Transform::from_translation(point.into()),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(hardware.float.clone()),
                    MeshMaterial3d(hardware.housing.clone()),
                    Transform::from_scale(Vec3::new(0.22, 0.34, 0.22)),
                ));
                parent.spawn((
                    Mesh3d(hardware.float.clone()),
                    MeshMaterial3d(hardware.lens.clone()),
                    Transform::from_xyz(0., 0.12, 0.).with_scale(Vec3::new(0.235, 0.11, 0.235)),
                ));
                parent.spawn((
                    Mesh3d(hardware.stem.clone()),
                    MeshMaterial3d(hardware.housing.clone()),
                    Transform::from_xyz(0., -0.46, 0.),
                ));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_then_returning_retrieves_the_nearest_buoy_and_preserves_distant_marks() {
        let mut marks = Vec::new();
        let site = Transform::from_xyz(12., 4., -14.);
        let distant = Transform::from_xyz(31., 8., -20.);
        toggle(&mut marks, &site);
        assert_eq!(marks.len(), 1);
        assert!(Vec3::from_array(marks[0]).distance(site.translation) < PICKUP_DISTANCE);
        toggle(&mut marks, &distant);
        assert_eq!(marks.len(), 2);
        let second = marks[1];
        toggle(&mut marks, &site);
        assert_eq!(marks, [second]);
    }
}
