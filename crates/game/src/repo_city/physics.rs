//! Avian rigid bodies: charged projectiles, collisions, tumbling code fragments.
use super::*;
use avian3d::prelude::*;
#[derive(Component)]
pub struct Lifetime(pub f32);
#[derive(Component)]
pub struct Projectile {
    pub used: bool,
}
#[derive(Component)]
pub struct BuildingCollider(pub usize, pub bool);
#[derive(Component)]
pub struct Fragment;

pub fn spawn_projectile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
    origin: Vec3,
    velocity: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(0.32))),
            MeshMaterial3d(material),
            Transform::from_translation(origin),
            RigidBody::Dynamic,
            Collider::sphere(0.32),
            LinearVelocity(velocity),
            AngularVelocity(Vec3::new(3., 1., 0.)),
            Restitution::new(0.55),
            SweptCcd::default(),
            CollisionEventsEnabled,
            Projectile { used: false },
            Lifetime(15.),
        ))
        .id()
}

pub fn trajectory(
    mut gizmos: Gizmos,
    keys: Res<ButtonInput<KeyCode>>,
    explorer: Res<Explorer>,
    camera: Query<&Transform, With<Camera3d>>,
    navigation: Res<city_navigation::Navigation>,
    set: Res<WorldSet>,
    listing: Res<listing::Listing>,
    picker: Res<file_picker::FilePicker>,
) {
    let cover = listing::Listing::visible(&explorer, &navigation);
    if !keys.pressed(KeyCode::KeyF)
        || picker.is_open()
        || (cover && (listing.on_diff() || listing.turning()))
    {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let direction = camera.forward().as_vec3();
    let origin = camera.translation + direction;
    let velocity = direction * (22. + explorer.charge * 22.);
    let point = |t: f32| origin + velocity * t - Vec3::Y * if cover { 0. } else { 4.905 * t * t };
    for i in 0..24 {
        gizmos.line(
            point(i as f32 * 0.075),
            point(i as f32 * 0.075 + 0.04),
            color(set.accent),
        );
    }
}

pub fn launch(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut explorer: ResMut<Explorer>,
    camera: Query<&Transform, With<Camera3d>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    palette: Res<Palette>,
    navigation: Res<city_navigation::Navigation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    picker: Res<file_picker::FilePicker>,
    listing: Res<listing::Listing>,
) {
    if picker.is_open() {
        explorer.charge = 0.;
        return;
    }
    if windows.single().is_ok_and(|w| !w.focused) {
        explorer.charge = 0.;
        return;
    }
    if keys.pressed(KeyCode::KeyF) {
        explorer.charge = (explorer.charge + time.delta_secs()).min(1.5);
    }
    if !keys.just_released(KeyCode::KeyF) {
        return;
    }
    let charge = std::mem::take(&mut explorer.charge);
    let cover = listing::Listing::visible(&explorer, &navigation);
    if charge <= 0. || (cover && (listing.on_diff() || listing.turning())) {
        return;
    }
    if let Ok(camera) = camera.single() {
        let direction = camera.forward().as_vec3();
        let speed = 22. + charge * 22.;
        let shot = spawn_projectile(
            &mut commands,
            &mut meshes,
            palette.edge.clone(),
            camera.translation + direction,
            direction * speed,
        );
        if cover {
            let core = materials.add(StandardMaterial {
                base_color: Color::srgb(0.57, 0.98, 1.),
                emissive: LinearRgba::new(0.7, 3., 4., 1.),
                unlit: true,
                ..default()
            });
            commands.entity(shot).insert(MeshMaterial3d(core.clone()));
            commands.entity(shot).with_children(|children| {
                children.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.12, 0.12, 1.5))),
                    MeshMaterial3d(core),
                    Transform::from_translation(-direction * 0.88)
                        .with_rotation(Quat::from_rotation_arc(Vec3::Z, direction)),
                ));
            });
            commands.entity(shot).insert((
                listing::CoverShot {
                    file: explorer.selected,
                    before: explorer.before,
                },
                GravityScale(0.),
                Lifetime(2.5),
            ));
        } else {
            explorer.notice =
                format!("Launched at {speed:.0} m/s. Aim at a function block to detach it.");
        }
    }
}
pub fn impacts(
    mut events: MessageReader<CollisionStart>,
    mut shots: Query<&mut Projectile, Without<listing::CoverShot>>,
    sections: Query<&Section>,
    buildings: Query<&BuildingCollider>,
    hunks: Query<&comparison_scene::HunkBlock>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    navigation: Option<Res<city_navigation::Navigation>>,
) {
    for event in events.read() {
        let (shot, target) = if shots.contains(event.collider1) {
            (event.collider1, event.collider2)
        } else if shots.contains(event.collider2) {
            (event.collider2, event.collider1)
        } else {
            continue;
        };
        let Ok(mut shot) = shots.get_mut(shot) else {
            continue;
        };
        if shot.used {
            continue;
        }
        if navigation
            .as_ref()
            .is_some_and(|navigation| navigation.guides_camera())
        {
            continue;
        }
        if let Ok(hunk) = hunks.get(target) {
            shot.used = true;
            if let Some(diff) = &explorer.comparison {
                if let Some(h) = diff.hunks.get(hunk.0) {
                    let page = h.row / 23;
                    explorer.hunk = hunk.0;
                    explorer.page = page;
                    explorer.rebuild = true;
                    explorer.notice = format!("Hunk {} opened on both buildings.", hunk.0 + 1);
                }
            }
            break;
        }
        if let Ok(section) = sections.get(target) {
            shot.used = true;
            explorer.function = Some(section.index);
        } else if let Ok(building) = buildings.get(target) {
            shot.used = true;
            if !building.1 {
                explorer.comparison_mode = 0;
            }
            if building.0 != explorer.selected {
                explorer.open(&repo.0, building.0);
            }
            if explorer.function.is_none() {
                explorer.function = explorer
                    .anatomy
                    .as_ref()
                    .filter(|a| !a.callables.is_empty())
                    .map(|_| 0);
            }
        } else {
            continue;
        }
        explorer.cut();
        if explorer.pending_fracture {
            explorer.anatomy = Some(repo_world::anatomy::analyze(
                &repo.0.files[explorer.selected].path,
                &explorer.source,
            ));
            explorer.function = None;
            explorer.function_page = 0;
            explorer.page = 0;
            explorer.refresh(&repo.0);
        }
        // Remaining contacts may refer to sections from the old syntax snapshot.
        break;
    }
}
pub fn fractures(
    mut commands: Commands,
    mut explorer: ResMut<Explorer>,
    repo: Res<Repo>,
    palette: Res<Palette>,
    set: Res<WorldSet>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !explorer.pending_fracture {
        return;
    }
    explorer.pending_fracture = false;
    let Some((name, source, start)) = &explorer.detached else {
        return;
    };
    let [x, z] = repo.0.files[explorer.selected].position;
    let y = if explorer.comparison_mode > 0 {
        30.
    } else {
        6.
    };
    let rows: Vec<_> = syntax::highlight(&repo.0.files[explorer.selected].path, source)
        .into_iter()
        .take(8)
        .map(|mut l| {
            l.number += start - 1;
            (Some(l), repo_world::comparison::Mark::Removed)
        })
        .collect();
    let texture = images.add(code_texture::source(name, &rows, set.removed, 1024, 320, 0));
    let face = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let block = commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(4.5, 1.6, 0.4))),
            MeshMaterial3d(palette.removed.clone()),
            Transform::from_xyz(x + 6., y, z + 6.),
            RigidBody::Dynamic,
            Collider::cuboid(4.5, 1.6, 0.4),
            LinearVelocity(Vec3::new(1., 4.5, 4.)),
            AngularVelocity(Vec3::new(0.7, 0.3, 0.5)),
            Restitution::new(0.35),
            Friction::new(0.6),
            AngularDamping(0.45),
            LinearDamping(0.08),
            Fragment,
            Lifetime(60.),
        ))
        .id();
    commands.entity(block).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(Rectangle::new(4.5, 1.6))),
            MeshMaterial3d(face),
            Transform::from_xyz(0., 0., 0.205),
        ));
    });
    for i in 0..8 {
        commands.spawn((
            Mesh3d(palette.unit.clone()),
            MeshMaterial3d(palette.removed.clone()),
            Transform::from_xyz(x + (i as f32 - 3.5) * 0.4, y + 1., z + 5.)
                .with_scale(Vec3::splat(0.25)),
            RigidBody::Dynamic,
            Collider::cuboid(1., 1., 1.),
            LinearVelocity(Vec3::new(
                (i as f32 * 2.4).sin() * 5.,
                3. + (i % 4) as f32,
                4. + (i as f32).cos() * 3.,
            )),
            AngularVelocity(Vec3::new(i as f32, 2., 3.)),
            Restitution::new(0.5),
            Friction::new(0.6),
            Fragment,
            Lifetime(15.),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_collision_detaches_exact_utf8_declaration_only_once() {
        let source =
            "const label = 'λ'; function hit() { return 1; } function keep() { return 2; }\n";
        let mut explorer = Explorer::new(0);
        explorer.source = source.into();
        explorer.original = source.into();
        explorer.anatomy = Some(repo_world::anatomy::analyze("sample.js", source));
        let repo = Repository {
            root: std::env::temp_dir(),
            revision: "0".repeat(40),
            baseline: None,
            files: vec![repo_world::File {
                path: "sample.js".into(),
                before: None,
                after: Some(repo_world::Blob {
                    id: "0".repeat(40),
                    size: source.len(),
                    mode: "100644".into(),
                }),
                change: Change::Added,
                position: [0., 0.],
            }],
            districts: vec![],
        };
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<CollisionStart>()
            .insert_resource(Repo(repo))
            .insert_resource(explorer)
            .add_systems(Update, impacts);
        let shot = app.world_mut().spawn(Projectile { used: false }).id();
        let section = app.world_mut().spawn(Section { index: 0 }).id();
        let hit = CollisionStart {
            collider1: shot,
            collider2: section,
            body1: Some(shot),
            body2: Some(section),
        };
        app.world_mut().write_message(hit);
        app.update();
        app.world_mut().write_message(hit);
        app.update();
        let explorer = app.world().resource::<Explorer>();
        assert_eq!(
            explorer.source,
            "const label = 'λ';  function keep() { return 2; }\n"
        );
        assert_eq!(explorer.original, source);
        assert_eq!(explorer.anatomy.as_ref().unwrap().callables.len(), 1);
        assert!(explorer.pending_fracture);
        assert!(app.world().get::<Projectile>(shot).unwrap().used);
    }
    #[test]
    fn rigid_fragment_hits_ground_bounces_and_settles() {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            PhysicsPlugins::default(),
        ));
        app.finish();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1. / 60.),
        ));
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::cuboid(100., 1., 100.),
            Transform::from_xyz(0., -0.5, 0.),
        ));
        let body = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::cuboid(1., 1., 1.),
                Transform::from_xyz(0., 5., 0.),
                Restitution::new(0.65),
                Friction::new(0.7),
            ))
            .id();
        let mut bounced = false;
        for _ in 0..300 {
            app.update();
            if app
                .world()
                .get::<LinearVelocity>(body)
                .is_some_and(|v| v.y > 1.)
            {
                bounced = true;
            }
        }
        assert!(bounced, "impact must produce an upward velocity");
        let p = app.world().get::<Position>(body).unwrap().0;
        assert!(
            p.y > 0.3 && p.y < 0.8,
            "fragment must settle above the ground: {p:?}"
        );
    }
}
