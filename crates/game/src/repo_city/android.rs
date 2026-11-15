//! Articulated android, rendered by the district mirror's small camera.
use super::*;
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster};

pub(super) const LAYER: usize = 7;

#[derive(Component, Clone, Copy)]
pub(super) enum Joint {
    Body,
    Head,
    Shoulder(f32),
    Elbow(f32),
    Hip(f32),
    Knee(f32),
}

#[derive(Clone, Copy)]
enum Paint {
    Metal,
    Dark,
    Shirt,
    Sole,
    Lamp,
    Blue,
}

struct Workshop {
    block: Handle<Mesh>,
    ball: Handle<Mesh>,
    pin: Handle<Mesh>,
    paint: [Handle<StandardMaterial>; 6],
}

impl Workshop {
    fn piece(
        &self,
        commands: &mut Commands,
        parent: Entity,
        mesh: &Handle<Mesh>,
        paint: Paint,
        position: [f32; 3],
        size: [f32; 3],
        rotation: Quat,
    ) {
        commands.spawn((
            ChildOf(parent),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(self.paint[paint as usize].clone()),
            Transform::from_translation(position.into())
                .with_scale(size.into())
                .with_rotation(rotation),
            RenderLayers::layer(LAYER),
            NotShadowCaster,
        ));
    }

    fn block(
        &self,
        commands: &mut Commands,
        parent: Entity,
        paint: Paint,
        position: [f32; 3],
        size: [f32; 3],
    ) {
        self.piece(
            commands,
            parent,
            &self.block,
            paint,
            position,
            size,
            Quat::IDENTITY,
        );
    }

    fn joint(
        &self,
        commands: &mut Commands,
        parent: Entity,
        place: [f32; 3],
        joint: Joint,
    ) -> Entity {
        commands
            .spawn((
                ChildOf(parent),
                Transform::from_translation(place.into()),
                Visibility::default(),
                joint,
            ))
            .id()
    }
}

pub(super) fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    use Paint::*;
    let colors = [
        [0.71, 0.59, 0.45],
        [0.10, 0.13, 0.19],
        [0.87, 0.23, 0.40],
        [0.91, 0.81, 0.62],
        [1.0, 0.81, 0.27],
        [0.24, 0.62, 0.65],
    ];
    let w = Workshop {
        block: meshes.add(shapes::rounded_box(Vec3::ONE, 0.10)),
        ball: meshes.add(Sphere::new(0.5).mesh().uv(12, 8)),
        pin: meshes.add(Cylinder::new(0.5, 1.).mesh().resolution(12)),
        paint: colors.map(|[r, g, b]| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                metallic: 0.14,
                perceptual_roughness: 0.69,
                emissive: LinearRgba::new(r * 0.13, g * 0.13, b * 0.13, 1.),
                ..default()
            })
        }),
    };
    let body = commands
        .spawn((Joint::Body, Transform::default(), Visibility::default()))
        .id();
    w.block(commands, body, Dark, [0., -0.41, 0.0], [0.68, 0.34, 0.46]);
    w.block(commands, body, Metal, [0., 0.29, 0.0], [0.58, 1.16, 0.43]);
    w.block(commands, body, Dark, [0., -0.21, 0.038], [0.72, 0.14, 0.48]);
    w.block(commands, body, Sole, [0., -0.21, 0.30], [0.16, 0.11, 0.045]);
    w.block(commands, body, Metal, [0., 1.04, 0.02], [0.15, 0.43, 0.16]);
    for side in [-1., 1.] {
        w.piece(
            commands,
            body,
            &w.block,
            Shirt,
            [side * 0.25, 0.33, 0.11],
            [0.36, 1.03, 0.51],
            Quat::from_rotation_z(side * 0.06),
        );
        for (dx, y, angle) in [(0.20, 0.62, 0.58), (0.31, 0.38, -0.80), (0.16, 0.12, 0.46)] {
            w.piece(
                commands,
                body,
                &w.block,
                Dark,
                [side * dx, y, 0.371],
                [0.15, 0.045, 0.03],
                Quat::from_rotation_z(angle * side),
            );
        }
    }
    let head = w.joint(commands, body, [0.02, 1.18, 0.02], Joint::Head);
    w.block(commands, head, Metal, [0., 0.47, 0.], [0.69, 0.88, 0.51]);
    w.block(commands, head, Dark, [0., 0.53, 0.268], [0.64, 0.27, 0.11]);
    w.block(
        commands,
        head,
        Blue,
        [-0.32, 0.54, 0.081],
        [0.12, 0.29, 0.28],
    );
    w.block(
        commands,
        head,
        Lamp,
        [-0.14, 0.55, 0.328],
        [0.16, 0.083, 0.033],
    );
    w.block(
        commands,
        head,
        Lamp,
        [0.13, 0.53, 0.328],
        [0.14, 0.060, 0.033],
    );
    w.piece(
        commands,
        head,
        &w.pin,
        Metal,
        [-0.33, 0.96, 0.],
        [0.045, 0.42, 0.045],
        Quat::from_rotation_z(0.38),
    );
    w.piece(
        commands,
        head,
        &w.ball,
        Lamp,
        [-0.41, 1.16, 0.],
        [0.10; 3],
        Quat::IDENTITY,
    );
    for side in [-1., 1.] {
        let arm = w.joint(
            commands,
            body,
            [side * 0.52, 0.74, 0.],
            Joint::Shoulder(side),
        );
        w.block(
            commands,
            arm,
            Shirt,
            [side * 0.045, -0.13, 0.],
            [0.34, 0.34, 0.42],
        );
        w.block(commands, arm, Metal, [0., -0.49, 0.], [0.13, 0.55, 0.15]);
        let elbow = w.joint(commands, arm, [0., -0.75, 0.], Joint::Elbow(side));
        w.piece(
            commands,
            elbow,
            &w.ball,
            Dark,
            [0.; 3],
            [0.23; 3],
            Quat::IDENTITY,
        );
        w.block(commands, elbow, Metal, [0., -0.31, 0.], [0.11, 0.59, 0.13]);
        w.block(
            commands,
            elbow,
            Metal,
            [0., -0.64, 0.02],
            [0.22, 0.22, 0.14],
        );
        for finger in 0..if side < 0. { 2 } else { 3 } {
            w.block(
                commands,
                elbow,
                Sole,
                [-0.085 + finger as f32 * 0.077, -0.78, 0.041],
                [0.05, 0.13, 0.048],
            );
        }

        let hip = w.joint(commands, body, [side * 0.22, -0.51, 0.], Joint::Hip(side));
        w.block(commands, hip, Metal, [0., -0.36, 0.], [0.15, 0.73, 0.21]);
        let knee = w.joint(commands, hip, [0., -0.74, 0.], Joint::Knee(side));
        w.piece(
            commands,
            knee,
            &w.ball,
            Blue,
            [0.; 3],
            [0.26; 3],
            Quat::IDENTITY,
        );
        w.block(commands, knee, Metal, [0., -0.38, 0.], [0.105, 0.69, 0.13]);
        w.block(commands, knee, Sole, [0., -0.76, 0.12], [0.29, 0.22, 0.62]);
        w.block(commands, knee, Blue, [0., -0.84, 0.12], [0.30, 0.068, 0.63]);
        w.block(
            commands,
            knee,
            Shirt,
            [0., -0.65, 0.22],
            [0.25, 0.035, 0.07],
        );
    }
}

fn pose(joint: Joint, t: f32, drift: f32, turn: f32) -> Quat {
    let sway = (t * 2.1).sin();
    let swing = (t * 5.).sin() * drift;
    let (pitch, yaw, roll) = match joint {
        Joint::Body => (0.14 * drift, turn * 0.62, sway * 0.025),
        Joint::Head => (0.04 * sway, turn * 0.37, -0.18 + sway * 0.055),
        Joint::Shoulder(side) => (
            0.06 * sway * side + swing * side * 0.32,
            0.,
            side * (0.17 + 0.12 * drift),
        ),
        Joint::Elbow(side) => (-0.18 - 0.27 * drift + sway * side * 0.055, 0., side * 0.08),
        Joint::Hip(side) => (0.10 + swing * side * 0.30, 0.04 * side, side * 0.12),
        Joint::Knee(side) => (0.12 + (0.3 + swing * side * 0.25) * drift, 0., 0.),
    };
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll)
}

pub(super) fn animate(joint: Joint, transform: &mut Transform, t: f32, drift: f32, turn: f32) {
    transform.rotation = pose(joint, t, drift, turn);
    if matches!(joint, Joint::Body) {
        transform.translation.y = (t * 2.1).sin() * (0.035 + drift * 0.045);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_joints_animate_at_idle_and_answer_motion() {
        assert_ne!(
            pose(Joint::Head, 0., 0., 0.),
            pose(Joint::Head, 0.6, 0., 0.)
        );
        assert_ne!(
            pose(Joint::Knee(1.), 0.4, 1., 0.),
            pose(Joint::Knee(1.), 0.4, 0., 0.)
        );
        assert_ne!(
            pose(Joint::Body, 0.4, 0., 0.),
            pose(Joint::Body, 0.4, 0., 0.5)
        );
    }
}
