//! Owl mesh from d2974c2, framed against the orthographic atlas.
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster};

use super::*;

#[derive(Component)]
pub(super) struct Owl;

#[derive(Component)]
pub(super) struct Wing(f32);

#[derive(Component)]
pub(super) struct Head;

pub(super) fn spawn(
    commands: &mut Commands,
    eye: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mut surface = |r, g, b| {
        materials.add(StandardMaterial {
            base_color: Color::srgb_u8(r, g, b),
            perceptual_roughness: 0.94,
            fog_enabled: false,
            ..default()
        })
    };
    let brown = surface(108, 70, 51);
    let feather = surface(143, 99, 63);
    let face = surface(220, 182, 127);
    let dark = surface(34, 24, 23);
    let gold = surface(242, 170, 66);
    let paper = surface(248, 212, 153);
    let ball = meshes.add(Sphere::new(0.5).mesh().ico(2).expect("owl body"));
    let roll = meshes.add(Cylinder::new(0.10, 0.92).mesh().resolution(12));
    let part = |root: &mut ChildSpawnerCommands, material: &Handle<StandardMaterial>, pos, size| {
        root.spawn((
            Mesh3d(ball.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(pos).with_scale(size),
            RenderLayers::layer(atlas::LAYER),
            NotShadowCaster,
        ));
    };
    commands
        .spawn((ChildOf(eye), Owl, Transform::default(), Visibility::Hidden))
        .with_children(|root| {
            part(root, &brown, Vec3::ZERO, Vec3::new(0.58, 0.69, 0.67));
            part(
                root,
                &face,
                Vec3::new(0., -0.13, -0.26),
                Vec3::new(0.47, 0.60, 0.21),
            );
            root.spawn((Head, Transform::from_xyz(0., 0.36, -0.16)))
                .with_children(|head| {
                    part(head, &brown, Vec3::ZERO, Vec3::new(0.68, 0.54, 0.55));
                    part(
                        head,
                        &gold,
                        Vec3::new(0., -0.09, -0.30),
                        Vec3::new(0.13, 0.14, 0.19),
                    );
                    for side in [-1., 1.] {
                        part(
                            head,
                            &face,
                            Vec3::new(side * 0.148, 0., -0.20),
                            Vec3::splat(0.31),
                        );
                        part(
                            head,
                            &gold,
                            Vec3::new(side * 0.15, 0.013, -0.324),
                            Vec3::splat(0.125),
                        );
                        part(
                            head,
                            &dark,
                            Vec3::new(side * 0.15, 0.013, -0.377),
                            Vec3::splat(0.066),
                        );
                    }
                });
            for s in [-1., 1.] {
                part(
                    root,
                    &gold,
                    Vec3::new(s * 0.16, -0.37, 0.01),
                    Vec3::new(0.12, 0.17, 0.24),
                );
                root.spawn((Wing(s), Transform::from_xyz(s * 0.23, 0.07, 0.)))
                    .with_children(|wing| {
                        part(
                            wing,
                            &brown,
                            Vec3::new(s * 0.49, 0., 0.07),
                            Vec3::new(1.12, 0.16, 0.55),
                        );
                        for step in 0..4 {
                            let step = step as f32;
                            part(
                                wing,
                                &feather,
                                Vec3::new(s * (0.47 + step * 0.14), -0.02, 0.27 + step * 0.07),
                                Vec3::new(0.39, 0.11, 0.45 - step * 0.03),
                            );
                        }
                    });
            }
            part(
                root,
                &feather,
                Vec3::new(0., -0.11, 0.43),
                Vec3::new(0.48, 0.14, 0.47),
            );
            root.spawn((
                Mesh3d(roll),
                MeshMaterial3d(paper),
                Transform::from_xyz(0., -0.44, 0.03)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                RenderLayers::layer(atlas::LAYER),
                NotShadowCaster,
            ));
            part(
                root,
                &dark,
                Vec3::new(0., -0.44, 0.03),
                Vec3::new(0.073, 0.216, 0.216),
            );
        });
}

pub(super) fn fly(
    map: Res<map::Map>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut owls: Query<(&mut Transform, &mut Visibility), (With<Owl>, Without<Wing>, Without<Head>)>,
    mut wings: Query<(&Wing, &mut Transform), Without<Head>>,
    mut heads: Query<&mut Transform, With<Head>>,
) {
    let Ok((mut pose, mut visible)) = owls.single_mut() else {
        return;
    };
    let aspect = windows
        .single()
        .map_or(1.6, |window| window.width() / window.height().max(1.));
    let frame = map.flight_frame(aspect);
    visible.set_if_neq(if frame.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    let Some(height) = frame else {
        return;
    };
    let t = time.elapsed_secs();
    pose.translation = Vec3::new(
        height * (0.28_f32.min(aspect * 0.31) + (t * 0.7).sin() * 0.018),
        height * (-0.29 + (t * 3.9).sin() * 0.008),
        -height * 0.35,
    );
    pose.scale = Vec3::splat(height * 0.083);
    pose.rotation = Quat::from_euler(EulerRot::YXZ, -0.48, 0.29, (t * 1.3).sin() * 0.09);
    for mut head in &mut heads {
        head.rotation = Quat::from_rotation_y(-2.55 + (t * 1.2).sin() * 0.23);
    }
    for (wing, mut joint) in &mut wings {
        joint.rotation = Quat::from_rotation_z(wing.0 * (0.13 + 0.48 * (t * 5.8).sin()));
    }
}
