//! A small street and building extract around Hadrian's Library.
use bevy::light::NotShadowCaster;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
struct Athens {
    streets: Vec<(u64, Vec<[f32; 2]>)>,
    buildings: Vec<(u64, [f32; 4])>,
}

pub(super) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
) {
    let athens: Athens =
        serde_json::from_str(include_str!("../../../../../../world_sets/athens-osm.json"))
            .expect("bundled Athens street extract");
    let mut surface =
        |red, green, blue| materials.add(StandardMaterial::from(Color::srgb_u8(red, green, blue)));
    let terrain = surface(171, 155, 112);
    let paving = surface(203, 182, 138);
    let roofs = [surface(166, 99, 73), surface(186, 124, 87)];
    let walls = surface(206, 181, 143);
    let hills = [surface(132, 136, 102), surface(157, 148, 113)];
    let cloud = materials.add(StandardMaterial {
        base_color: Color::srgba(0.95, 0.96, 0.94, 0.38),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let block = meshes.add(Cuboid::from_size(Vec3::ONE));
    let mound = meshes.add(Sphere::new(0.5).mesh().ico(2).expect("low-poly hill"));
    let mut piece = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, pose| {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            pose,
            NotShadowCaster,
        ));
    };
    piece(
        &block,
        &terrain,
        Transform::from_xyz(0., -1.34, 25.).with_scale(Vec3::new(280., 2., 318.)),
    );
    for (_, street) in athens.streets {
        for segment in street.windows(2) {
            let a = Vec2::from_array(segment[0]);
            let b = Vec2::from_array(segment[1]);
            let d = b - a;
            let center = (a + b) / 2.;
            if (a.x > 19. && b.x > 19.) || (a.x < -19. && b.x < -19.) {
                piece(
                    &block,
                    &paving,
                    Transform::from_xyz(center.x, -0.32, center.y)
                        .with_rotation(Quat::from_rotation_y(-d.y.atan2(d.x)))
                        .with_scale(Vec3::new(d.length().max(0.1), 0.025, 1.28)),
                );
            }
        }
    }
    for (id, [x, z, width, depth]) in athens.buildings {
        if x.abs() - width / 2. < 20. {
            continue;
        }
        let height = 1.9 + (id % 5) as f32 * 0.45;
        piece(
            &block,
            &walls,
            Transform::from_xyz(x, height / 2. - 0.34, z)
                .with_scale(Vec3::new(width, height, depth)),
        );
        piece(
            &block,
            &roofs[id as usize % roofs.len()],
            Transform::from_xyz(x, height - 0.24, z).with_scale(Vec3::new(
                width + 0.2,
                0.25,
                depth + 0.2,
            )),
        );
    }
    for (index, (x, z, width, height)) in [
        (-112., 67., 113., 37.),
        (-122., -25., 79., 24.),
        (103., 101., 144., 30.),
        (118., 160., 120., 22.),
        (-85., 155., 130., 43.),
    ]
    .into_iter()
    .enumerate()
    {
        piece(
            &mound,
            &hills[index % hills.len()],
            Transform::from_xyz(x, -2., z).with_scale(Vec3::new(width, height, width * 0.72)),
        );
    }
    for (x, y, z, width) in [
        (-53., 44., 22., 21.),
        (68., 42., 31., 26.),
        (30., 57., 97., 28.),
    ] {
        for offset in [-0.4, 0., 0.36] {
            piece(
                &mound,
                &cloud,
                Transform::from_xyz(x + width * offset, y, z).with_scale(Vec3::new(
                    width,
                    3.2 - offset.abs() * 2.,
                    width * 0.38,
                )),
            );
        }
    }
    for (x, z, height) in [
        (-18.4, -12., 6.1),
        (18.5, -10.2, 4.6),
        (-18.6, 10.1, 5.2),
        (19.3, 14., 6.9),
        (25., -22., 5.8),
        (-21.1, 33.1, 6.4),
    ] {
        commands.spawn((
            models::scene(assets, "cypress.glb"),
            Transform::from_xyz(x, -0.08, z).with_scale(Vec3::new(3.1, height / 1.38, 3.1)),
            Visibility::Inherited,
        ));
    }
    commands.spawn((
        models::scene(assets, "temple.glb"),
        Transform::from_xyz(39., 2.7, -13.)
            .with_scale(Vec3::splat(27.))
            .with_rotation(Quat::from_rotation_y(-0.43)),
        Visibility::Inherited,
    ));
}
