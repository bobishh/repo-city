//! A single offscreen portrait follows the pilot along the rear glass of the nearest station.
use super::*;
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    light::NotShadowCaster,
    render::render_resource::TextureFormat,
};

const LAYER: usize = 9;

#[derive(Component)]
pub(crate) struct RearMirrorCamera;
#[derive(Component)]
struct MirrorCraft;
#[derive(Component)]
struct Pane;
#[derive(Component)]
struct Portrait;

pub(super) fn install(app: &mut App) {
    app.add_systems(PostStartup, setup)
        .add_systems(Update, (reflect.after(targeting), pilot::animate));
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let image = images.add(Image::new_target_texture(
        512,
        352,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    let craft = pilot::spawn(&mut commands, &mut meshes, &mut materials);
    commands.entity(craft).insert(MirrorCraft);
    commands.queue(move |world: &mut World| {
        let mut stack = vec![craft];
        while let Some(entity) = stack.pop() {
            if let Some(children) = world.get::<Children>(entity) {
                stack.extend(children.iter());
            }
            world.entity_mut(entity).insert(RenderLayers::layer(LAYER));
        }
    });
    commands.spawn((
        RearMirrorCamera,
        district::MirrorCamera,
        Camera3d::default(),
        Camera {
            order: -3,
            is_active: false,
            clear_color: Color::NONE.into(),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        RenderLayers::layer(LAYER),
        Tonemapping::None,
        Msaa::Off,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                viewport_height: 2.72,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0., 0.15, -5.).looking_at(Vec3::Y * 0.10, Vec3::Y),
    ));
    let mesh = meshes.add(Rectangle::new(1., 1.));
    commands.spawn((
        Pane,
        NotShadowCaster,
        Mesh3d(mesh.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.26, 0.43, 0.46),
            metallic: 0.52,
            perceptual_roughness: 0.22,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        Portrait,
        NotShadowCaster,
        Mesh3d(mesh),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.89, 0.99, 1., 0.91),
            base_color_texture: Some(image),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

struct Reflection {
    pane: Transform,
    portrait: Transform,
    camera: Transform,
}

fn reflection(file: &repo_world::File, eye: &Transform) -> Option<Reflection> {
    let size = listing::block_size(file);
    let pane = file_facade::transform(file, size, file_facade::Part::Mirror);
    let normal = pane.rotation * Vec3::Z;
    let offset = pane.rotation.inverse() * (eye.translation - pane.translation);
    let facing = eye.forward().dot(normal);
    if !(0.22..18.).contains(&offset.z) || facing >= -0.12 {
        return None;
    }
    let contact = eye.translation + eye.forward() * (-offset.z / facing);
    let local = pane.rotation.inverse() * (contact - pane.translation);
    let half = size.xy() * 0.5;
    if local.x.abs() > half.x || local.y.abs() > half.y {
        return None;
    }
    let height = (size.y * 0.92).min(size.x * 0.63).min(2.8);
    let width = height * (512. / 352.);
    let center = Vec3::new(
        offset.x.clamp(-half.x + width / 2., half.x - width / 2.),
        offset.y.clamp(-half.y + height / 2., half.y - height / 2.),
        0.045,
    );
    let portrait = Transform::from_translation(pane.translation + pane.rotation * center)
        .with_rotation(pane.rotation)
        .with_scale(Vec3::new(-width, height, 1.));
    let target = Vec3::Y * 0.10;
    let toward_glass = eye.rotation.inverse() * -normal;
    let camera = Transform::from_translation(target + toward_glass * 5. + Vec3::Y * 0.18)
        .looking_at(target, Vec3::Y);
    Some(Reflection {
        pane,
        portrait,
        camera,
    })
}

fn reflect(
    repo: Res<Repo>,
    nearest: Res<Nearest>,
    navigation: Res<city_navigation::Navigation>,
    eye: Single<(&Camera, &Transform), PlayerCamera>,
    mut offscreen: Single<
        (&mut Camera, &mut Transform),
        (
            With<RearMirrorCamera>,
            With<district::MirrorCamera>,
            With<Camera3d>,
        ),
    >,
    mut craft: Single<&mut Visibility, (With<MirrorCraft>, Without<Pane>, Without<Portrait>)>,
    mut pane: Single<
        (&mut Transform, &mut Visibility),
        (With<Pane>, Without<Camera3d>, Without<Portrait>),
    >,
    mut portrait: Single<
        (&mut Transform, &mut Visibility),
        (With<Portrait>, Without<Camera3d>, Without<Pane>),
    >,
) {
    let (main, transform) = *eye;
    let projection = main
        .is_active
        .then_some(nearest.lamp_file)
        .flatten()
        .filter(|_| !navigation.overview())
        .and_then(|index| repo.0.files.get(index))
        .and_then(|file| reflection(file, transform));
    let (camera, position) = &mut *offscreen;
    let visible = if projection.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if camera.is_active != projection.is_some() {
        camera.is_active = projection.is_some();
    }
    craft.set_if_neq(visible);
    pane.1.set_if_neq(visible);
    portrait.1.set_if_neq(visible);
    if let Some(view) = projection {
        position.set_if_neq(view.camera);
        pane.0.set_if_neq(view.pane);
        portrait.0.set_if_neq(view.portrait);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_engineer_appears_only_on_a_nearby_rear_wall() {
        let file = repo_world::File {
            path: "mirror.rs".into(),
            before: None,
            after: None,
            change: Change::Unchanged,
            position: [0., 0.],
        };
        let face =
            file_facade::transform(&file, listing::block_size(&file), file_facade::Part::Mirror);
        let normal = face.rotation * Vec3::Z;
        let eye = Transform::from_translation(face.translation + normal * 6.)
            .looking_at(face.translation, Vec3::Y);
        let image = reflection(&file, &eye).unwrap();
        assert!(image.portrait.scale.x < 0.);
        assert!(image.camera.translation.z < 0.);
        assert!(
            reflection(
                &file,
                &Transform::from_translation(eye.translation).looking_to(normal, Vec3::Y)
            )
            .is_none()
        );
        assert!(
            reflection(
                &file,
                &Transform::from_translation(face.translation + normal * 25.)
                    .looking_at(face.translation, Vec3::Y)
            )
            .is_none()
        );
    }
}
