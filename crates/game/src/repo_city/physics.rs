//! Tool effects and physical fragments from private drafts.
use super::*;
use avian3d::prelude::*;
#[derive(Component)]
pub struct Lifetime(pub f32);
#[derive(Component)]
pub struct Fragment;

const SPLASH_SECONDS: f32 = 0.32;

#[derive(Component)]
pub(super) struct Splash(Vec3);

pub(super) fn splash(commands: &mut Commands, position: Vec3) {
    commands.spawn((Splash(position), Lifetime(SPLASH_SECONDS)));
}

pub(super) fn splashes(mut gizmos: Gizmos, flashes: Query<(&Splash, &Lifetime)>) {
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
/// A compact warm pulse near the emitter and target; leave the reading area clear.
pub(super) fn heat_pulse(
    gizmos: &mut Gizmos,
    camera: &Transform,
    hit: Vec3,
    row_height: f32,
    time: f32,
    raised: f32,
) {
    let origin = android::spray_origin(camera, raised, false);
    let forward = camera.forward().as_vec3();
    let right = camera.right().as_vec3();
    let up = camera.up().as_vec3();
    for ray in 0..5 {
        let phase = (time * 2. + ray as f32 / 5.).fract();
        let angle = ray as f32 * std::f32::consts::TAU / 5.;
        let spread = (right * angle.cos() + up * angle.sin()) * 0.012;
        gizmos.line_gradient(
            origin + forward * phase * 0.10,
            origin + forward * (phase + 0.3) * 0.10 + spread,
            Color::srgba(1., 0.24, 0.035, 0.65),
            Color::srgba(1., 0.55, 0.12, 0.),
        );
        let radius = row_height.min(0.12) * (0.4 + phase * 0.3);
        let point = hit - forward * 0.02 + (right * angle.cos() + up * angle.sin()) * radius;
        gizmos.line_gradient(
            point,
            point + up * radius * 0.25,
            Color::srgba(1., 0.38, 0.08, (1. - phase) * 0.5),
            Color::srgba(1., 0.55, 0.12, 0.),
        );
    }
}

/// A short fan of cyan streaks. It has no bodies, emitters, or persistent particles.
pub(super) fn spray_stream(
    gizmos: &mut Gizmos,
    camera: &Transform,
    hit: Vec3,
    row_height: f32,
    time: f32,
    pigment: Option<[u8; 3]>,
    raised: f32,
) {
    let [r, g, b] = pigment.map_or([0.32, 0.83, 1.], |rgb| rgb.map(|v| v as f32 / 255.));
    let right = camera.right().as_vec3();
    let up = camera.up().as_vec3();
    let length = camera.translation.distance(hit);
    let spread = (row_height * 2.4).min(length * 0.07);
    let origin = android::spray_origin(camera, raised, pigment.is_some());
    let touch = hit + Vec3::Z * 0.025;
    for lane in 0..11 {
        let phase = (time * 9. + lane as f32 * 0.173).fract();
        let angle = lane as f32 * 2.39996;
        let fan = (right * angle.cos() + up * angle.sin()) * spread;
        let tip = touch + fan;
        let point = |t: f32| origin.lerp(tip, t) + up * (t * (1. - t) * spread);
        gizmos.line_gradient(
            point((phase - 0.26).max(0.)),
            point(phase),
            Color::srgba(r, g, b, 0.18),
            Color::srgba(r, g, b, 0.86),
        );
        if lane % 2 == 0 {
            let splash = (time * 6. + lane as f32 * 0.223).fract();
            gizmos.line_gradient(
                touch + fan * splash,
                touch + fan * (splash + 0.26) + Vec3::Z * spread * splash,
                Color::srgba(r, g, b, (1. - splash) * 0.75),
                Color::srgba(r, g, b, 0.),
            );
        }
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
        comparison_scene::base(&repo.0.files[explorer.selected]) + 6.
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
