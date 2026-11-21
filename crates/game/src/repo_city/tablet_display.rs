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

pub(super) const LAYER: usize = android::TABLET_LAYER;
pub(super) const CASE: Vec2 = android::TABLET_CASE;
const SCREEN: Vec2 = android::TABLET_SCREEN;
const SIZE: UVec2 = android::TABLET_SIZE;
const POINTER: PointerId = PointerId::Custom(Uuid::from_u128(0x74ab1e7));

#[derive(Resource)]
pub(super) struct Display {
    ui: Entity,
    view: Entity,
    shield: Entity,
    target: NormalizedRenderTarget,
    #[cfg(target_arch = "wasm32")]
    screen_material: Handle<StandardMaterial>,
    pub(super) size: UVec2,
}

#[derive(Component)]
pub(super) struct Screen;

#[derive(Resource, Default)]
pub(super) struct TouchScroll(pub f32);

fn screen_texture(images: &mut Assets<Image>, size: UVec2) -> Handle<Image> {
    images.add(Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ))
}

pub(super) fn register(app: &mut App) {
    tablet_scroll::register(app);
    app.add_systems(First, pointer.in_set(bevy::picking::PickingSystems::Input))
        .add_systems(Update, toggle.after(android::show_tablet))
        .add_systems(
            Update,
            native_panel::focus_outline.after(native_panel::keyboard_navigation),
        );
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(
        Update,
        native_panel::keyboard_navigation.after(review_tablet::update),
    );
    #[cfg(target_arch = "wasm32")]
    app.init_resource::<browser_tablet::Tablet>()
        .add_systems(
            Update,
            native_panel::keyboard_navigation.after(browser_tablet::update),
        )
        .add_systems(Update, resize.before(browser_tablet::update))
        .add_systems(Update, browser_tablet::update.after(android::show_tablet))
        .add_systems(
            PostUpdate,
            browser_tablet::geometry.after(bevy::ui::UiSystems::PostLayout),
        );
}

#[derive(Clone, Copy)]
pub(super) enum Case {
    Metal,
    Stone,
}

pub(super) fn spawn(
    commands: &mut Commands,
    pilot: Entity,
    tablet: Entity,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    case: Case,
) {
    let image = screen_texture(images, SIZE);
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
    let screen_material = materials.add(StandardMaterial {
        base_color_texture: Some(image.clone()),
        unlit: true,
        ..default()
    });
    commands.spawn((
        ChildOf(tablet),
        Screen,
        Mesh3d(meshes.add(Rectangle::new(SCREEN.x, SCREEN.y))),
        MeshMaterial3d(screen_material.clone()),
        Transform::from_xyz(0., 0.012, 0.054),
        RenderLayers::layer(LAYER),
        bevy::light::NotShadowCaster,
    ));
    if matches!(case, Case::Metal) {
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
    }
    let shield = spawn_shield(commands, pilot);
    commands.spawn(POINTER);
    commands.insert_resource(TouchScroll::default());
    commands.insert_resource(Display {
        ui,
        view,
        shield,
        target: NormalizedRenderTarget::Image(image.into()),
        #[cfg(target_arch = "wasm32")]
        screen_material,
        size: SIZE,
    });
}

fn spawn_shield(commands: &mut Commands, pilot: Entity) -> Entity {
    commands
        .spawn((
            GlobalZIndex(900),
            UiTargetCamera(pilot),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.),
                height: percent(100.),
                display: bevy::ui::Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.02, 0.04, 0.62)),
        ))
        // The projected pointer owns tablet focus. The window pointer only blocks the HUD.
        .observe(|mut focus: On<bevy::input_focus::AcquireFocus>| focus.propagate(false))
        .id()
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
    mut touch: Local<Option<(u64, Vec2, Vec2, bool, bool)>>,
    mut tap_release: Local<Option<Location>>,
    mut scrolling: Option<ResMut<TouchScroll>>,
    scroll_parts: Query<(&UiGlobalTransform, &ComputedNode), With<tablet_scroll::Part>>,
) {
    let Some(display) = display else { return };
    let project = |point: Vec2| {
        let (camera, view) = cameras.get(display.view).ok()?;
        let ray = camera.viewport_to_world(view, point).ok()?;
        let screen = screen.single().ok()?.affine().inverse();
        project_pointer(
            screen.transform_point3(ray.origin),
            screen.transform_vector3(*ray.direction),
        )
        .map(|point| point * display.size.as_vec2() / SIZE.as_vec2())
    };
    if !review.tablet_open && touch.take().is_some() {
        *tap_release = None;
        let location = Location {
            target: display.target.clone(),
            position: Vec2::splat(-1.),
        };
        inputs.write(PointerInput::new(
            POINTER,
            location.clone(),
            PointerAction::Cancel,
        ));
        inputs.write(PointerInput::new(
            POINTER,
            location,
            PointerAction::Release(PointerButton::Primary),
        ));
    }
    let position = (|| {
        if !review.tablet_open {
            return None;
        }
        #[cfg(target_arch = "wasm32")]
        if let Some((_, _, point, _, _)) = *touch {
            return Some(point);
        }
        project(window.single().ok()?.cursor_position()?)
    })();
    let outside = Vec2::splat(-1.);
    let mut location = Location {
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
    if let Some(location) = tap_release.take() {
        inputs.write(PointerInput::new(
            POINTER,
            location,
            PointerAction::Release(PointerButton::Primary),
        ));
        *touch = None;
    }
    let mut started_this_frame = false;
    for event in events.read() {
        // Window resources are updated after First. Consume ordered motion events here so
        // a move and press delivered in one frame hit the current control, not the old one.
        if let WindowEvent::CursorMoved(event) = event
            && review.tablet_open
            && touch.is_none()
        {
            location.position = project(event.position).unwrap_or(outside);
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
        }
        if let WindowEvent::TouchInput(event) = event
            && review.tablet_open
        {
            use bevy::input::touch::TouchPhase;
            let point = project(event.position);
            let location = Location {
                target: display.target.clone(),
                position: point.unwrap_or(outside),
            };
            match event.phase {
                TouchPhase::Started if touch.is_none() && point.is_some() => {
                    let point = point.unwrap();
                    let scrollbar = scroll_parts.iter().any(|(transform, node)| {
                        let transform: bevy::math::Affine2 = transform.into();
                        let local = transform.inverse().transform_point2(point);
                        local.abs().cmple(node.size() / 2.).all()
                    });
                    *touch = Some((event.id, point, point, false, scrollbar));
                    started_this_frame = true;
                    inputs.write(PointerInput::new(
                        POINTER,
                        location.clone(),
                        PointerAction::Move { delta: Vec2::ZERO },
                    ));
                    inputs.write(PointerInput::new(
                        POINTER,
                        location,
                        PointerAction::Press(PointerButton::Primary),
                    ));
                }
                TouchPhase::Moved => {
                    if let Some((id, start, previous, dragged, scrollbar)) = touch.as_mut()
                        && *id == event.id
                        && let Some(point) = point
                    {
                        let delta = point - *previous;
                        if !*scrollbar && !*dragged && point.distance(*start) > 8. {
                            *dragged = true;
                            inputs.write(PointerInput::new(
                                POINTER,
                                location.clone(),
                                PointerAction::Cancel,
                            ));
                        }
                        if *dragged {
                            if let Some(scroll) = scrolling.as_mut() {
                                scroll.0 -= delta.y;
                            }
                        } else {
                            inputs.write(PointerInput::new(
                                POINTER,
                                location,
                                PointerAction::Move { delta },
                            ));
                        }
                        *previous = point;
                    }
                }
                TouchPhase::Ended | TouchPhase::Canceled => {
                    if let Some((id, _, _, dragged, _)) = *touch
                        && id == event.id
                    {
                        // Headless buttons need a frame to apply their deferred Pressed state.
                        if started_this_frame && event.phase == TouchPhase::Ended && !dragged {
                            *tap_release = Some(location);
                            continue;
                        }
                        if event.phase == TouchPhase::Canceled && !dragged {
                            inputs.write(PointerInput::new(
                                POINTER,
                                location.clone(),
                                PointerAction::Cancel,
                            ));
                        }
                        inputs.write(PointerInput::new(
                            POINTER,
                            location.clone(),
                            PointerAction::Release(PointerButton::Primary),
                        ));
                        *touch = None;
                    }
                }
                _ => {}
            }
        }
        if let WindowEvent::MouseButtonInput(event) = event
            && event.button == MouseButton::Left
        {
            let action = match event.state {
                ButtonState::Pressed if location.position != outside => {
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

#[cfg(target_arch = "wasm32")]
pub(super) fn resize(
    display: Option<ResMut<Display>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut targets: Query<&mut RenderTarget>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut display) = display else {
        return;
    };
    let viewport = window.single().map_or(Vec2::new(1440., 900.), |window| {
        Vec2::new(window.width(), window.height().max(1.))
    });
    let pose = android::tablet_pose(1., viewport.x / viewport.y);
    let width = android::tablet_corners(&pose, viewport).map_or(SIZE.x as f32, |corners| {
        (corners[2] - corners[0]).clamp(320., SIZE.x as f32)
    });
    // Give small screens more layout space on the same mesh. Rendering, picking
    // and scrolling all use the resulting texture size.
    let compact = ((560. - width) / (560. - 320.)).clamp(0., 1.);
    let width = (width * (1. + 0.25 * compact)).round() as u32;
    let size = UVec2::new(width, width * SIZE.y / SIZE.x);
    if display.size != size
        && let Ok(mut camera_target) = targets.get_mut(display.ui)
        && let Some(mut material) = materials.get_mut(&display.screen_material)
    {
        // A new image handle keeps the camera and material off the old GPU texture.
        let image = screen_texture(&mut images, size);
        *camera_target = RenderTarget::Image(image.clone().into());
        material.base_color_texture = Some(image.clone());
        display.target = NormalizedRenderTarget::Image(image.into());
        display.size = size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_input_keeps_focus_when_the_window_pointer_hits_the_shield() {
        use bevy::input_focus::{AcquireFocus, InputFocus, InputFocusPlugin};
        let mut app = App::new();
        app.add_plugins((bevy::input::InputPlugin, InputFocusPlugin))
            .add_systems(Startup, |mut commands: Commands| {
                let camera = commands.spawn(Camera3d::default()).id();
                spawn_shield(&mut commands, camera);
            });
        app.world_mut().register_component::<ChildOf>();
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        let input = app.world_mut().spawn_empty().id();
        app.world_mut()
            .insert_resource(InputFocus::from_entity(input));
        let shield = app
            .world_mut()
            .query_filtered::<Entity, With<BackgroundColor>>()
            .single(app.world())
            .unwrap();
        app.world_mut().trigger(AcquireFocus {
            focused_entity: shield,
            window,
        });
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(input));
    }

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
