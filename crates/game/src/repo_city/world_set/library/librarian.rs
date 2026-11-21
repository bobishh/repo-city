//! First-person hands and the stone case for the shared VibePad screen.
use bevy::{camera::visibility::RenderLayers, light::NotShadowCaster};

use super::*;

#[derive(Component)]
pub(super) struct Hands(f32);

pub(super) fn spawn(
    commands: &mut Commands,
    eye: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(158, 143, 121),
        perceptual_roughness: 0.93,
        ..default()
    });
    let trim = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(91, 63, 37),
        metallic: 0.26,
        ..default()
    });
    let sleeve = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(177, 40, 72),
        ..default()
    });
    let hand = materials.add(StandardMaterial {
        base_color: Color::srgb_u8(185, 157, 115),
        ..default()
    });
    let block = meshes.add(shapes::rounded_box(Vec3::ONE, 0.08));
    let tablet = commands
        .spawn((
            ChildOf(eye),
            android::ViewTablet,
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    let size = android::TABLET_CASE;
    commands.entity(tablet).with_children(|case| {
        for (surface, placement, dimensions) in [
            (stone, Vec3::ZERO, Vec3::new(size.x, size.y, 0.103)),
            (
                trim,
                Vec3::new(0., 0., 0.038),
                Vec3::new(1.078, 0.72, 0.025),
            ),
        ] {
            case.spawn((
                Mesh3d(block.clone()),
                MeshMaterial3d(surface),
                Transform::from_translation(placement).with_scale(dimensions),
                RenderLayers::layer(tablet_display::LAYER),
                NotShadowCaster,
            ));
        }
        for side in [-1., 1.] {
            grip(case, &block, &sleeve, &hand, side, tablet_display::LAYER);
        }
    });
    tablet_display::spawn(
        commands,
        eye,
        tablet,
        images,
        meshes,
        materials,
        tablet_display::Case::Stone,
    );
    for side in [-1., 1.] {
        commands
            .spawn((
                ChildOf(eye),
                Hands(side),
                Transform::default(),
                Visibility::Hidden,
            ))
            .with_children(|arm| grip(arm, &block, &sleeve, &hand, side, 0));
    }
}

fn grip(
    parent: &mut ChildSpawnerCommands,
    block: &Handle<Mesh>,
    sleeve: &Handle<StandardMaterial>,
    hand: &Handle<StandardMaterial>,
    side: f32,
    layer: usize,
) {
    for (surface, placement, dimensions) in [
        (sleeve, [side * 0.61, -0.40, 0.035], [0.16, 0.30, 0.125]),
        (hand, [side * 0.55, -0.325, 0.071], [0.17, 0.17, 0.112]),
    ] {
        parent.spawn((
            Mesh3d(block.clone()),
            MeshMaterial3d(surface.clone()),
            Transform::from_translation(placement.into())
                .with_rotation(Quat::from_rotation_z(-side * 0.43))
                .with_scale(dimensions.into()),
            RenderLayers::layer(layer),
            NotShadowCaster,
        ));
    }
}

pub(super) fn animate(
    library: Res<Library>,
    review: Res<review_notes::Review>,
    unrolling: Res<room::Unrolling>,
    mut hands: Query<(&Hands, &mut Transform, &mut Visibility)>,
) {
    let t = unrolling.elapsed;
    let changing =
        library.reader.is_some() && !review.blocked && t >= room::APPROACHED && t < room::UNROLLED;
    let push = (std::f32::consts::PI * unrolling.reveal()).sin().max(0.);
    for (hand, mut pose, mut visibility) in &mut hands {
        *visibility = if changing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if changing {
            pose.translation = Vec3::new(hand.0 * -0.18, -0.20 + push * 0.15, -1.10 - push * 0.35);
            pose.rotation = Quat::from_rotation_x(-0.12 - 0.11 * push);
        }
    }
}
