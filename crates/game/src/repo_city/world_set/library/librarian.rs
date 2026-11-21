//! The library's third-person avatar and the walk, step and push sequence.
use super::{room, *};

#[derive(Component)]
pub(super) struct Librarian;

#[derive(Component)]
pub(super) enum Limb {
    Arm(f32),
    Leg(f32),
}

#[derive(Default)]
pub(super) struct Gait {
    previous: Option<Vec3>,
    phase: f32,
}

pub(super) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let block = meshes.add(shapes::rounded_box(Vec3::ONE, 0.13));
    let paint = [
        Color::srgb(0.71, 0.59, 0.45),
        Color::srgb(0.10, 0.13, 0.19),
        Color::srgb(0.87, 0.23, 0.40),
        Color::srgb(1., 0.81, 0.27),
    ]
    .map(|base_color| {
        materials.add(StandardMaterial {
            base_color,
            ..default()
        })
    });
    let piece = |position: Vec3, size: Vec3, material: usize| {
        (
            Mesh3d(block.clone()),
            MeshMaterial3d(paint[material].clone()),
            Transform::from_translation(position).with_scale(size),
        )
    };

    commands
        .spawn((Librarian, Transform::default(), Visibility::Visible))
        .with_children(|body| {
            body.spawn(piece(
                Vec3::new(0., 0.99, 0.),
                Vec3::new(0.51, 0.50, 0.28),
                2,
            ));
            body.spawn(piece(
                Vec3::new(0., 1.54, 0.01),
                Vec3::new(0.43, 0.47, 0.36),
                0,
            ));
            body.spawn(piece(
                Vec3::new(0., 1.58, 0.19),
                Vec3::new(0.38, 0.15, 0.058),
                1,
            ));
            for side in [-1., 1.] {
                body.spawn(piece(
                    Vec3::new(side * 0.085, 1.58, 0.224),
                    Vec3::new(0.072, 0.047, 0.012),
                    3,
                ));
                body.spawn((
                    Limb::Leg(side),
                    Transform::from_xyz(side * 0.13, 0.76, 0.),
                    Visibility::Visible,
                ))
                .with_children(|leg| {
                    leg.spawn(piece(
                        Vec3::new(0., -0.31, 0.),
                        Vec3::new(0.17, 0.62, 0.18),
                        0,
                    ));
                    leg.spawn(piece(
                        Vec3::new(0., -0.66, 0.065),
                        Vec3::new(0.20, 0.17, 0.33),
                        1,
                    ));
                });
                body.spawn((
                    Limb::Arm(side),
                    Transform::from_xyz(side * 0.33, 1.17, 0.),
                    Visibility::Visible,
                ))
                .with_children(|arm| {
                    arm.spawn(piece(
                        Vec3::new(0., -0.20, 0.),
                        Vec3::new(0.13, 0.41, 0.14),
                        0,
                    ));
                    arm.spawn(piece(
                        Vec3::new(0., -0.44, 0.01),
                        Vec3::new(0.16, 0.15, 0.17),
                        1,
                    ));
                });
            }
        });
}

fn ahead(eye: &Transform) -> (Vec3, Vec3) {
    let forward = eye.forward().as_vec3().with_y(0.).normalize_or(Vec3::Z);
    (
        eye.translation.with_y(0.) + forward * 2.05 - Vec3::Y.cross(forward) * 0.56,
        forward,
    )
}

pub(super) fn animate(
    time: Res<Time>,
    library: Res<Library>,
    unrolling: Res<room::Unrolling>,
    camera: Query<&Transform, With<room::Camera>>,
    mut avatar: Query<&mut Transform, (With<Librarian>, Without<room::Camera>, Without<Limb>)>,
    mut limbs: Query<(&Limb, &mut Transform), (Without<Librarian>, Without<room::Camera>)>,
    mut gait: Local<Gait>,
) {
    let (Ok(eye), Ok(mut actor)) = (camera.single(), avatar.single_mut()) else {
        return;
    };
    let elapsed = unrolling.elapsed;
    let (position, direction, walk, climb, push) = if library.reader.is_none() {
        let (position, forward) = ahead(eye);
        let moved = gait
            .previous
            .replace(position)
            .map_or(0., |previous| previous.distance(position));
        (
            position,
            forward,
            (moved / time.delta_secs().max(0.001) / 3.).clamp(0., 1.),
            0.,
            0.,
        )
    } else {
        gait.previous = None;
        let (from, _) = ahead(library.walk_pose.as_ref().unwrap_or(eye));
        let step_in = Vec3::new(-0.83, 0., unrolling.origin - 1.38);
        let on_step = Vec3::new(-0.83, 0.18, unrolling.origin - 0.58);
        let approach = (elapsed / room::APPROACHED).clamp(0., 1.);
        let climb =
            ((elapsed - room::APPROACHED) / (room::CLIMBED - room::APPROACHED)).clamp(0., 1.);
        let push = if elapsed >= room::CLIMBED && elapsed <= room::UNROLLED {
            (std::f32::consts::PI * unrolling.reveal()).sin().max(0.)
        } else {
            0.
        };
        let mut position = from.lerp(step_in, approach).lerp(on_step, climb);
        position.y += (climb * std::f32::consts::PI).sin() * 0.12 - push * 0.13;
        position.z += 0.28 * push;
        let mut direction = if approach < 0.98 {
            (step_in - from).with_y(0.).normalize_or(Vec3::Z)
        } else {
            Vec3::Z
        };
        if elapsed > room::UNROLLED {
            position = Vec3::new(-2.87, 0., eye.translation.z + 1.1);
            direction = Vec3::X;
        }
        (
            position,
            direction,
            if approach < 1. { 1. } else { 0. },
            (climb * std::f32::consts::PI).sin(),
            push,
        )
    };

    gait.phase += 10.5 * time.delta_secs();
    let stride = gait.phase.sin() * walk;
    actor.translation = position + Vec3::Y * stride.abs() * 0.035;
    actor.rotation =
        Quat::from_rotation_y(direction.x.atan2(direction.z)) * Quat::from_rotation_x(0.32 * push);
    for (limb, mut pose) in &mut limbs {
        let tilt = match limb {
            Limb::Arm(side) => -side * stride * 0.52 - 0.94 * push - 0.39 * climb,
            Limb::Leg(side) => side * stride * 0.61 + 0.31 * climb,
        };
        pose.rotation = Quat::from_rotation_x(tilt);
    }
}
