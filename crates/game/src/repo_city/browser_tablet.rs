//! Keep browser controls on the same handheld screen as the native presentation.
use super::*;
use bevy::camera::visibility::RenderLayers;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = tabletPose)]
    fn project(corners: &[f32], width: f32, height: f32);
}

pub(super) fn spawn(
    commands: &mut Commands,
    pilot: Entity,
    tablet: Entity,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let layer = RenderLayers::layer(android::TABLET_LAYER);
    commands.spawn((
        ChildOf(pilot),
        android::TabletCamera,
        Camera3d::default(),
        Camera {
            order: 2,
            is_active: false,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        layer.clone(),
        Transform::default(),
        Msaa::Sample4,
    ));
    commands.spawn((
        ChildOf(pilot),
        DirectionalLight {
            illuminance: 8_000.,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.33, 2.72, 0.)),
        layer.clone(),
    ));
    commands.spawn((
        ChildOf(tablet),
        Mesh3d(meshes.add(Rectangle::new(
            android::TABLET_SCREEN.x,
            android::TABLET_SCREEN.y,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb_u8(6, 25, 34),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0., 0.012, 0.054),
        layer.clone(),
        bevy::light::NotShadowCaster,
    ));
    commands.spawn((
        ChildOf(tablet),
        Mesh3d(meshes.add(Rectangle::new(0.17, 0.0425))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(code_texture::vibepad_wordmark())),
            alpha_mode: AlphaMode::Mask(0.35),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.385, -0.349, 0.055),
        layer,
        bevy::light::NotShadowCaster,
    ));
}

pub(super) fn update(
    view: Query<(&Transform, &Visibility), With<android::ViewTablet>>,
    mut camera: Query<&mut Camera, With<android::TabletCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
) {
    let Ok((pose, visibility)) = view.single() else {
        return;
    };
    let visible = *visibility != Visibility::Hidden;
    if let Ok(mut camera) = camera.single_mut() {
        camera.is_active = visible;
    }
    let viewport = window.single().map_or(Vec2::ONE, |window| {
        Vec2::new(window.width(), window.height())
    });
    if let Some(canvas) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("repo-city"))
    {
        let width = canvas.client_width();
        if width > 0 {
            let next = (viewport.x / width as f32).clamp(0.5, 4.);
            if ui_scale.0 != next {
                ui_scale.0 = next;
            }
        }
    }
    let corners = visible
        .then(|| android::tablet_corners(pose, viewport))
        .flatten();
    project(
        corners.as_ref().map_or(&[], |corners| corners.as_slice()),
        viewport.x,
        viewport.y,
    );
}
