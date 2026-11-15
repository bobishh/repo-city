//! Charged water shots, splash effects and private draft impacts.
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

const MAX_SHOTS: usize = 10;
const SPLASH_SECONDS: f32 = 0.32;

#[derive(Resource)]
pub(super) struct Blaster {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

impl Blaster {
    pub(super) fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        Self {
            mesh: meshes.add(Sphere::new(0.22)),
            material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.48, 0.96, 1.),
                emissive: LinearRgba::new(0.6, 2.8, 4.6, 1.),
                unlit: true,
                ..default()
            }),
        }
    }
}

#[derive(Component)]
pub(super) struct Splash(Vec3);

pub(super) fn splash(commands: &mut Commands, position: Vec3) {
    commands.spawn((Splash(position), Lifetime(SPLASH_SECONDS)));
}

pub fn spawn_projectile(
    commands: &mut Commands,
    blaster: &Blaster,
    origin: Vec3,
    velocity: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(blaster.mesh.clone()),
            MeshMaterial3d(blaster.material.clone()),
            Transform::from_translation(origin),
            RigidBody::Dynamic,
            Collider::sphere(0.22),
            CollisionLayers::from_bits(2, 1),
            LinearVelocity(velocity),
            Restitution::new(0.18),
            SweptCcd::default(),
            CollisionEventsEnabled,
            Projectile { used: false },
            Lifetime(4.5),
        ))
        .id()
}

pub(super) fn splashes(
    mut gizmos: Gizmos,
    shots: Query<(&Projectile, &Transform, &LinearVelocity)>,
    flashes: Query<(&Splash, &Lifetime)>,
) {
    for (shot, pose, velocity) in &shots {
        if !shot.used
            && let Some(direction) = velocity.try_normalize()
        {
            let head = pose.translation;
            gizmos.line_gradient(
                head - direction * 2.2,
                head,
                Color::srgba(0.28, 0.78, 1., 0.),
                Color::srgb(0.60, 0.97, 1.),
            );
        }
    }
    for (flash, life) in &flashes {
        let alpha = (life.0 / SPLASH_SECONDS).clamp(0., 1.);
        let spread = 1. - alpha;
        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::TAU / 8.;
            let ray = Vec3::new(angle.cos(), 0.4 + (i % 3) as f32 * 0.3, angle.sin());
            let tip = flash.0 + ray * (0.18 + 1.65 * spread) - Vec3::Y * spread * spread;
            gizmos.line_gradient(
                tip - ray * 0.32,
                tip,
                Color::srgba(0.18, 0.62, 1., 0.),
                Color::srgba(0.62, 0.95, 1., alpha),
            );
        }
    }
}
pub fn trajectory(
    mut gizmos: Gizmos,
    keys: Res<ButtonInput<KeyCode>>,
    explorer: Res<Explorer>,
    camera: Query<&Transform, PlayerCamera>,
    set: Res<WorldSet>,
    listing: Res<listing::Listing>,
    picker: Res<file_picker::FilePicker>,
) {
    if !keys.pressed(KeyCode::KeyF) || picker.is_open() || listing.turning() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let direction = camera.forward().as_vec3();
    let origin = camera.translation + direction;
    let velocity = direction * shot_speed(explorer.charge);
    let point = |t: f32| origin + velocity * t - Vec3::Y * 4.905 * t * t;
    for i in 0..24 {
        gizmos.line(
            point(i as f32 * 0.075),
            point(i as f32 * 0.075 + 0.04),
            color(set.accent),
        );
    }
}

fn shot_speed(charge: f32) -> f32 {
    30. + charge * 28.
}

pub fn launch(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut explorer: ResMut<Explorer>,
    camera: Query<&Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    blaster: Res<Blaster>,
    navigation: Res<city_navigation::Navigation>,
    active_shots: Query<Entity, With<Projectile>>,
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
    if charge <= 0. || listing.turning() || active_shots.iter().take(MAX_SHOTS).count() == MAX_SHOTS
    {
        return;
    }
    if let Ok(camera) = camera.single() {
        let direction = camera.forward().as_vec3();
        // Close reading views can sit less than one unit from the wall.
        let origin = if cover {
            camera.translation
        } else {
            camera.translation + direction
        };
        let shot = spawn_projectile(
            &mut commands,
            &blaster,
            origin,
            direction * shot_speed(charge),
        );
        if cover {
            commands.entity(shot).insert(listing::CoverShot);
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
        comparison_scene::BASE + 6.
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
            Lifetime(6.),
        ))
        .id();
    commands.entity(block).with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(Rectangle::new(4.5, 1.6))),
            MeshMaterial3d(face),
            Transform::from_xyz(0., 0., 0.205),
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_blaster_releases_share_assets_and_cap_active_bodies() {
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let blaster = Blaster::new(&mut meshes, &mut materials);
        let mesh = blaster.mesh.clone();
        let material = blaster.material.clone();
        let mut app = App::new();
        app.insert_resource(blaster)
            .insert_resource(Explorer::new(0))
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<city_navigation::Navigation>()
            .init_resource::<file_picker::FilePicker>()
            .init_resource::<listing::Listing>()
            .add_systems(Update, launch);
        app.world_mut().spawn(Camera3d::default());
        for _ in 0..64 {
            app.world_mut().resource_mut::<Explorer>().charge = 0.5;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::KeyF);
            keys.release(KeyCode::KeyF);
            app.update();
        }
        let world = app.world_mut();
        let mut shots = world.query_filtered::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &CollisionLayers), With<Projectile>>();
        let shots = shots.iter(world).collect::<Vec<_>>();
        assert_eq!(shots.len(), MAX_SHOTS);
        assert!(
            shots
                .iter()
                .all(|(shot_mesh, shot_material, layers)| shot_mesh.0 == mesh
                    && shot_material.0 == material
                    && !layers.interacts_with(**layers))
        );
    }

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
