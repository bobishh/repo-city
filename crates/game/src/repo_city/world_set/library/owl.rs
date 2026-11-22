use bevy::{
    camera::visibility::RenderLayers, light::NotShadowCaster,
    world_serialization::WorldInstanceReady,
};

use super::*;

#[derive(Component)]
pub(super) struct Owl;

#[derive(Component)]
pub(super) struct Wing {
    side: f32,
    rest: Quat,
}

#[derive(Component)]
pub(super) struct Head(Quat);

pub(super) fn spawn(commands: &mut Commands, eye: Entity, assets: &AssetServer) {
    commands
        .spawn((
            models::scene(assets, "owl.glb"),
            ChildOf(eye),
            Owl,
            Transform::default(),
            Visibility::Hidden,
        ))
        .observe(rig);
}

fn rig(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    parts: Query<(
        Option<&Name>,
        &Transform,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for descendant in children.iter_descendants(ready.entity) {
        let Ok((name, transform, mesh)) = parts.get(descendant) else {
            continue;
        };
        if let Some(mesh) = mesh {
            if let Some(mut material) = materials.get_mut(&mesh.0) {
                material.fog_enabled = false;
            }
            commands.entity(descendant).insert((
                RenderLayers::from_layers(&[0, atlas::LAYER]),
                NotShadowCaster,
            ));
        }
        match name.map(Name::as_str) {
            Some("Wing.L" | "Wing.R") => {
                commands.entity(descendant).insert(Wing {
                    side: transform.translation.x.signum(),
                    rest: transform.rotation,
                });
            }
            Some("Head") => {
                commands.entity(descendant).insert(Head(transform.rotation));
            }
            _ => {}
        }
    }
}

pub(super) fn fly(
    library: Res<Library>,
    map: Res<map::Map>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<&Projection, With<room::Camera>>,
    mut owls: Query<(&mut Transform, &mut Visibility), (With<Owl>, Without<Wing>, Without<Head>)>,
    mut wings: Query<(&Wing, &mut Transform), Without<Head>>,
    mut heads: Query<(&Head, &mut Transform), Without<Wing>>,
    #[cfg(target_arch = "wasm32")] mut announced: Local<&'static str>,
) {
    let Ok((mut pose, mut visible)) = owls.single_mut() else {
        return;
    };
    let aspect = windows
        .single()
        .map_or(1.6, |window| window.width() / window.height().max(1.));
    let overhead = map.flight_frame(aspect);
    let perspective = camera
        .single()
        .ok()
        .and_then(|projection| match projection {
            Projection::Perspective(lens) => Some(8. * (lens.fov * 0.5).tan()),
            _ => None,
        });
    let flight = library.flight.is_some();
    let landing = matches!(library.flight, Some(room::Flight::Landing { .. }));
    #[cfg(target_arch = "wasm32")]
    {
        let state = if map.open() || !flight {
            ""
        } else if landing {
            "landing"
        } else {
            "flying"
        };
        if *announced != state {
            *announced = state;
            browser_flight(state);
        }
    }
    let height = overhead.or(perspective.filter(|_| !map.open() && flight));
    visible.set_if_neq(if height.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    let Some(height) = height else {
        return;
    };
    let t = time.elapsed_secs();
    let depth = if overhead.is_some() {
        height * 0.23
    } else {
        4.
    };
    pose.translation = Vec3::new(
        height * (0.28_f32.min(aspect * 0.31) + (t * 0.92).sin() * 0.025),
        height * (-0.35 + (t * 3.9).sin() * 0.015),
        -depth,
    );
    pose.scale = Vec3::splat(height * 0.245);
    pose.rotation = Quat::from_euler(EulerRot::YXZ, -0.46, 0.56, (t * 1.3).sin() * 0.13);
    for (head, mut joint) in &mut heads {
        joint.rotation = Quat::from_rotation_y((t * 1.1).sin() * 0.29) * head.0;
    }
    for (wing, mut joint) in &mut wings {
        let flap = 0.31 + (t * if landing { 10.5 } else { 7.4 }).sin() * 0.58;
        joint.rotation = Quat::from_rotation_z(wing.side * flap)
            * Quat::from_rotation_y(-wing.side * 1.12)
            * wing.rest;
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = libraryFlight)]
    fn browser_flight(state: &str);
}
