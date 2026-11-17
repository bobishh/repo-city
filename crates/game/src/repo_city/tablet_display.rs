//! Render VibePad into the android's handheld screen and project clicks into its UI.
use bevy::{
    asset::uuid::Uuid,
    camera::{NormalizedRenderTarget, RenderTarget, visibility::RenderLayers},
    input::ButtonState,
    picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput},
    render::render_resource::TextureFormat,
    ui::UiTargetCamera,
    window::WindowEvent,
};

use super::*;

pub(super) const LAYER: usize = 8;
pub(super) const CASE: Vec2 = Vec2::new(1.15, 0.77);
const SCREEN: Vec2 = Vec2::new(1.03, 0.65);
const SIZE: UVec2 = UVec2::new(1440, 909);
const POINTER: PointerId = PointerId::Custom(Uuid::from_u128(0x74ab1e7));

#[derive(Resource)]
pub(super) struct Display {
    ui: Entity,
    view: Entity,
    shield: Entity,
    target: NormalizedRenderTarget,
}

#[derive(Component)]
pub(super) struct Screen;

pub(super) fn spawn(
    commands: &mut Commands,
    pilot: Entity,
    tablet: Entity,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let image = images.add(Image::new_target_texture(
        SIZE.x,
        SIZE.y,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    let target = image.clone().into();
    let ui = commands
        .spawn((
            Camera2d,
            Camera {
                order: -2,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::Image(target),
            RenderLayers::layer(LAYER),
        ))
        .id();
    let view = commands
        .spawn((
            ChildOf(pilot),
            android::TabletCamera,
            Camera3d::default(),
            Camera {
                order: 2,
                is_active: false,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderLayers::layer(LAYER),
            Transform::default(),
            Msaa::Sample4,
        ))
        .id();
    commands.spawn((
        ChildOf(pilot),
        DirectionalLight {
            illuminance: 8_000.,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.33, 2.72, 0.)),
        RenderLayers::layer(LAYER),
    ));
    commands.spawn((
        ChildOf(tablet),
        Screen,
        Mesh3d(meshes.add(Rectangle::new(SCREEN.x, SCREEN.y))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(image.clone()),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0., 0.012, 0.054),
        RenderLayers::layer(LAYER),
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
        RenderLayers::layer(LAYER),
        bevy::light::NotShadowCaster,
    ));
    let shield = commands
        .spawn((
            GlobalZIndex(900),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.),
                height: percent(100.),
                display: bevy::ui::Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.02, 0.04, 0.62)),
        ))
        .id();
    commands.spawn(POINTER);
    commands.insert_resource(Display {
        ui,
        view,
        shield,
        target: NormalizedRenderTarget::Image(image.into()),
    });
}

pub(super) fn target(commands: &mut Commands, root: Entity) {
    commands.queue(move |world: &mut World| {
        if let Some(display) = world.get_resource::<Display>() {
            let ui = display.ui;
            world.entity_mut(root).insert(UiTargetCamera(ui));
        }
    });
}

pub(super) fn toggle(
    display: Res<Display>,
    review: Res<review_notes::Review>,
    shown: Query<&Visibility, With<android::ViewTablet>>,
    mut cameras: Query<&mut Camera>,
    mut nodes: Query<&mut Node>,
) {
    let raised = review.tablet_open;
    if let Ok(mut camera) = cameras.get_mut(display.ui)
        && camera.is_active != raised
    {
        camera.is_active = raised;
    }
    let visible = raised
        || shown
            .single()
            .is_ok_and(|visibility| *visibility != Visibility::Hidden);
    if let Ok(mut camera) = cameras.get_mut(display.view)
        && camera.is_active != visible
    {
        camera.is_active = visible;
    }
    let visibility = if raised {
        bevy::ui::Display::Flex
    } else {
        bevy::ui::Display::None
    };
    if let Ok(mut node) = nodes.get_mut(display.shield)
        && node.display != visibility
    {
        node.display = visibility;
    }
}

fn project_pointer(origin: Vec3, direction: Vec3) -> Option<Vec2> {
    if direction.z >= -0.0001 {
        return None;
    }
    let t = -origin.z / direction.z;
    if t < 0. {
        return None;
    }
    let hit = origin + t * direction;
    let uv = Vec2::new(hit.x / SCREEN.x + 0.5, 0.5 - hit.y / SCREEN.y);
    (uv.cmpge(Vec2::ZERO).all() && uv.cmple(Vec2::ONE).all()).then(|| uv * SIZE.as_vec2())
}

pub(super) fn pointer(
    display: Option<Res<Display>>,
    review: Res<review_notes::Review>,
    window: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    screen: Query<&GlobalTransform, With<Screen>>,
    mut events: MessageReader<WindowEvent>,
    mut inputs: MessageWriter<PointerInput>,
    mut last: Local<Option<Vec2>>,
) {
    let Some(display) = display else { return };
    let position = (|| {
        if !review.tablet_open {
            return None;
        }
        let (camera, view) = cameras.get(display.view).ok()?;
        let ray = camera
            .viewport_to_world(view, window.single().ok()?.cursor_position()?)
            .ok()?;
        let screen = screen.single().ok()?.affine().inverse();
        project_pointer(
            screen.transform_point3(ray.origin),
            screen.transform_vector3(*ray.direction),
        )
    })();
    let outside = Vec2::splat(-1.);
    let location = Location {
        target: display.target.clone(),
        position: position.unwrap_or(outside),
    };
    let previous = last.replace(location.position).unwrap_or(outside);
    if previous != location.position {
        inputs.write(PointerInput::new(
            POINTER,
            location.clone(),
            PointerAction::Move {
                delta: location.position - previous,
            },
        ));
    }
    for event in events.read() {
        if let WindowEvent::MouseButtonInput(event) = event
            && event.button == MouseButton::Left
        {
            let action = match event.state {
                ButtonState::Pressed if position.is_some() => {
                    PointerAction::Press(PointerButton::Primary)
                }
                ButtonState::Released if review.tablet_open => {
                    PointerAction::Release(PointerButton::Primary)
                }
                _ => continue,
            };
            inputs.write(PointerInput::new(POINTER, location.clone(), action));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_maps_onto_the_physical_screen_and_excludes_the_bezel() {
        let screen_top_left = Vec3::new(-SCREEN.x / 2., SCREEN.y / 2., 0.5);
        assert_eq!(
            project_pointer(screen_top_left, Vec3::NEG_Z),
            Some(Vec2::ZERO)
        );
        assert_eq!(
            project_pointer(Vec3::Z, Vec3::NEG_Z),
            Some(SIZE.as_vec2() / 2.)
        );
        let bezel = Vec3::new(CASE.x / 2., 0., 0.5);
        assert_eq!(project_pointer(bezel, Vec3::NEG_Z), None);
    }
}
