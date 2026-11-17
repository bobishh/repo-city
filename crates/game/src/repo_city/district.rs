//! Street lights and the player portrait shown by district glass.
use super::*;
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    light::NotShadowCaster,
    render::render_resource::TextureFormat,
};

const TINTS: [[f32; 3]; 4] = [
    [0.97, 0.39, 0.67],
    [0.33, 0.85, 0.96],
    [0.97, 0.71, 0.39],
    [0.69, 0.55, 0.99],
];
pub(super) fn beacon_ink(index: usize) -> [f32; 3] {
    TINTS[index % TINTS.len()]
}
const GLASS_WIDTH: f32 = 11.;
const GLASS_HEIGHT: f32 = 7.;

#[derive(Component)]
pub(super) struct Glass;

#[derive(Component)]
pub(super) struct Beacon(pub usize);

#[derive(Component)]
pub(super) struct Reflection;

#[derive(Component)]
pub(super) struct MirrorCamera;

#[derive(Component)]
pub(super) struct LogoCamera;

#[derive(Default)]
pub(super) struct MirrorMotion {
    last_eye: Option<Vec3>,
    drift: f32,
}

pub(super) struct Lights {
    glass: Handle<Mesh>,
    beacon: Handle<Mesh>,
    tint: [Handle<StandardMaterial>; 4],
    spill: [Handle<StandardMaterial>; 4],
    pane: Handle<StandardMaterial>,
}

impl Lights {
    pub(super) fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let neon = |[r, g, b]: [f32; 3], materials: &mut Assets<StandardMaterial>| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                unlit: true,
                ..default()
            })
        };
        Self {
            glass: meshes.add(Rectangle::new(1., 1.)),
            beacon: meshes.add(Cuboid::from_size(Vec3::ONE)),
            tint: TINTS.map(|ink| neon(ink, materials)),
            spill: TINTS.map(|ink| neon(ink.map(|value| value * 0.29), materials)),
            pane: neon([0.065, 0.10, 0.14], materials),
        }
    }

    pub(super) fn spawn(
        &self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        district: &repo_world::District,
        index: usize,
    ) {
        let [x, z] = district.position;
        let [w, d] = district.size;
        let ink = index % TINTS.len();
        let origin = Vec3::new(x, 0., z);
        let unit = Mesh::from(Cuboid::from_size(Vec3::ONE));
        let mut light_meshes: [Option<Mesh>; 2] = [None, None];
        let mut glow = |position: Vec3, core: Vec3, spill: Vec3| {
            for (mesh, (scale, height)) in
                light_meshes.iter_mut().zip([(spill, -0.075), (core, 0.)])
            {
                let beam = unit.clone().transformed_by(
                    Transform::from_translation(position - origin + Vec3::Y * height)
                        .with_scale(scale),
                );
                if let Some(mesh) = mesh {
                    mesh.merge(&beam).expect("matching light mesh attributes");
                } else {
                    *mesh = Some(beam);
                }
            }
        };
        let min_x = x - 2.5;
        let max_x = min_x + w - 1.;
        let min_z = z - 0.5;
        let max_z = min_z + d - 1.;
        for edge in [min_z, max_z] {
            glow(
                Vec3::new((min_x + max_x) / 2., 0.065, edge),
                Vec3::new(w - 1., 0.14, 0.29),
                Vec3::new(w - 1., 0.15, 0.92),
            );
        }
        for edge in [min_x, max_x] {
            glow(
                Vec3::new(edge, 0.065, (min_z + max_z) / 2.),
                Vec3::new(0.29, 0.14, d - 1.),
                Vec3::new(0.92, 0.15, d - 1.),
            );
            for z in [min_z, max_z] {
                glow(
                    Vec3::new(edge, 1.2, z),
                    Vec3::new(0.22, 2.4, 0.22),
                    Vec3::new(0.49, 2.4, 0.49),
                );
            }
        }

        let pane = Vec3::new(x + 3., 4.25, max_z);
        commands.spawn((
            CityObject,
            city_stream::DistrictPart(index),
            Glass,
            NotShadowCaster,
            Mesh3d(self.glass.clone()),
            MeshMaterial3d(self.pane.clone()),
            Transform::from_translation(pane).with_scale(Vec3::new(GLASS_WIDTH, GLASS_HEIGHT, 1.)),
        ));
        commands.spawn((
            CityObject,
            city_stream::DistrictPart(index),
            Beacon(index),
            NotShadowCaster,
            Mesh3d(self.beacon.clone()),
            MeshMaterial3d(self.tint[ink].clone()),
            Transform::from_xyz(min_x, 10. + listing::MAX_HEIGHT / 2., max_z)
                .with_scale(Vec3::new(0.34, listing::MAX_HEIGHT, 0.34)),
        ));
        for side in [-1., 1.] {
            glow(
                pane + Vec3::new(side * GLASS_WIDTH / 2., -0.35, 0.02),
                Vec3::new(0.14, GLASS_HEIGHT + 0.7, 0.18),
                Vec3::new(0.42, GLASS_HEIGHT + 0.7, 0.13),
            );
        }
        for side in [-1., 1.] {
            glow(
                pane + Vec3::new(0., side * GLASS_HEIGHT / 2., 0.02),
                Vec3::new(GLASS_WIDTH, 0.14, 0.18),
                Vec3::new(GLASS_WIDTH, 0.36, 0.13),
            );
        }
        for (mesh, material) in light_meshes
            .into_iter()
            .zip([&self.spill[ink], &self.tint[ink]])
        {
            commands.spawn((
                CityObject,
                city_stream::DistrictPart(index),
                NotShadowCaster,
                Mesh3d(meshes.add(mesh.expect("district has lights"))),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(origin),
            ));
        }
        let sign = label(
            commands,
            meshes,
            materials,
            images,
            &district.name,
            pane + Vec3::new(0., GLASS_HEIGHT / 2. + 1.4, 0.12),
            24.,
            TINTS[ink],
        );
        commands
            .entity(sign)
            .insert(city_stream::DistrictPart(index));
    }
}

pub(super) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let robot = images.add(Image::new_target_texture(
        256,
        384,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    commands.spawn((
        MirrorCamera,
        Camera3d::default(),
        Camera {
            order: -1,
            is_active: false,
            clear_color: Color::NONE.into(),
            ..default()
        },
        RenderTarget::Image(robot.clone().into()),
        Tonemapping::None,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                viewport_height: 5.55,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0., -0.04, 8.).looking_at(Vec3::Y * -0.04, Vec3::Y),
        RenderLayers::layer(android::LAYER),
    ));
    #[cfg(target_arch = "wasm32")]
    {
        let face = images.add(Image::new_target_texture(
            96,
            96,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ));
        commands.spawn((
            MirrorCamera,
            LogoCamera,
            Camera3d::default(),
            Camera {
                order: -2,
                clear_color: Color::NONE.into(),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                // Include the antenna and head through the reflection rig's full turn.
                fov: 0.66,
                ..default()
            }),
            RenderTarget::Image(face.clone().into()),
            Tonemapping::None,
            Transform::from_xyz(0.25, 1.86, 2.50).looking_at(Vec3::new(-0.045, 1.88, 0.), Vec3::Y),
            RenderLayers::layer(android::LAYER),
        ));
        commands.spawn((
            ImageNode::new(face),
            Node {
                position_type: PositionType::Absolute,
                left: px(16.),
                top: px(16.),
                width: px(48.),
                height: px(48.),
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ));
    }
    for (location, color, lux) in [
        (Vec3::new(2.5, 5., 6.), Color::srgb(0.89, 0.95, 1.), 10_500.),
        (Vec3::new(-4., 1., 2.), Color::srgb(1., 0.48, 0.73), 4_500.),
    ] {
        commands.spawn((
            DirectionalLight {
                color,
                illuminance: lux,
                ..default()
            },
            Transform::from_translation(location).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(android::LAYER),
        ));
    }
    commands.spawn((
        Reflection,
        NotShadowCaster,
        Mesh3d(meshes.add(Rectangle::new(3.5, 5.25))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(robot),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

fn portrait(pane: &Transform, eye: &Transform) -> Option<(Vec3, f32, f32)> {
    let inverse = pane.rotation.inverse();
    let offset = inverse * (eye.translation - pane.translation);
    let facing = inverse * eye.forward().as_vec3();
    let half = pane.scale.xy() / 2.;
    if !(0.23..18.).contains(&offset.z)
        || offset.x.abs() >= half.x + 2.
        || offset.y.abs() >= half.y + 3.5
        || facing.z >= -0.05
    {
        return None;
    }
    let scale = (pane.scale.x / 4.5).min(pane.scale.y / 6.5).min(0.66);
    let x = offset
        .x
        .clamp(-half.x + 1.75 * scale, half.x - 1.75 * scale);
    let y = (offset.y - 0.8 * scale).clamp(-half.y + 2.625 * scale, half.y - 2.625 * scale);
    let opacity = ((18. - offset.z) / 6.).clamp(0., 1.)
        * ((half.x + 2. - offset.x.abs()) / 3.).clamp(0., 1.)
        * ((half.y + 3.5 - offset.y.abs()) / 3.).clamp(0., 1.);
    Some((
        pane.translation + pane.rotation * Vec3::new(x, y, 0.035),
        scale,
        opacity * 0.62,
    ))
}

pub(super) fn reflect(
    camera: Query<(&Transform, &Pilot), PlayerCamera>,
    panes: Query<
        &Transform,
        (
            With<Glass>,
            Without<Reflection>,
            Without<MirrorCamera>,
            Without<android::Joint>,
        ),
    >,
    mut sprite: Query<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (With<Reflection>, Without<Camera3d>),
    >,
    mut render: Query<
        (&mut Camera, &mut Transform),
        (With<MirrorCamera>, With<Camera3d>, Without<LogoCamera>),
    >,
    mut joints: Query<(&android::Joint, &mut Transform), (Without<Camera3d>, Without<Reflection>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut motion: Local<MirrorMotion>,
    review: Res<review_notes::Review>,
) {
    let (
        Ok((eye, _pilot)),
        Ok((mut pose, mut visibility, material)),
        Ok((mut view, mut mirror_pose)),
    ) = (camera.single(), sprite.single_mut(), render.single_mut())
    else {
        return;
    };
    let closest = panes
        .iter()
        .filter_map(|pane| portrait(pane, eye).map(|portrait| (pane, portrait)))
        .min_by(|(a, _), (b, _)| {
            a.translation
                .distance_squared(eye.translation)
                .total_cmp(&b.translation.distance_squared(eye.translation))
        });
    let Some((pane, (position, scale, alpha))) = closest else {
        visibility.set_if_neq(Visibility::Hidden);
        if view.is_active {
            view.is_active = false;
        }
        motion.last_eye = None;
        motion.drift = 0.;
        #[cfg(target_arch = "wasm32")]
        for (joint, mut local) in &mut joints {
            android::animate(*joint, &mut local, time.elapsed_secs(), 0., 0.24, 0., false);
        }
        return;
    };
    if !view.is_active {
        view.is_active = true;
    }
    let inverse = pane.rotation.inverse();
    let offset = inverse * (eye.translation - pane.translation);
    let orbit = Vec3::new(
        (offset.x / offset.z.max(3.) * 2.1).clamp(-1.5, 1.5),
        (offset.y / offset.z.max(3.) * 0.75).clamp(-0.35, 0.45) - 0.04,
        8.,
    );
    mirror_pose.set_if_neq(Transform::from_translation(orbit).looking_at(Vec3::Y * -0.04, Vec3::Y));
    let dt = time.delta_secs();
    let speed = motion
        .last_eye
        .replace(eye.translation)
        .map_or(0., |last| eye.translation.distance(last) / dt.max(0.001));
    motion.drift = motion
        .drift
        .lerp((speed / 10.).clamp(0., 1.), 1. - (-dt * 7.).exp());
    let yaw = (inverse * eye.forward().as_vec3()).x.clamp(-0.8, 0.8);
    for (joint, mut local) in &mut joints {
        android::animate(
            *joint,
            &mut local,
            time.elapsed_secs(),
            motion.drift,
            yaw,
            review.nozzle,
            review.tablet_open,
        );
    }
    visibility.set_if_neq(Visibility::Visible);
    pose.set_if_neq(
        Transform::from_translation(position)
            .with_rotation(pane.rotation)
            .with_scale(Vec3::new(-scale, scale, 1.)),
    );
    let tint = Color::srgba(0.73, 0.81, 0.97, (alpha * 255.).round() / 255.);
    if materials
        .get(&material.0)
        .is_some_and(|m| m.base_color != tint)
        && let Some(mut material) = materials.get_mut(&material.0)
    {
        material.base_color = tint;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_android_tracks_the_viewer_on_the_front_of_the_glass() {
        let pane = Transform::from_scale(Vec3::new(GLASS_WIDTH, GLASS_HEIGHT, 1.));
        let mut eye = Transform::from_xyz(-2., 0.5, 8.);
        let (left, scale, opacity) = portrait(&pane, &eye).unwrap();
        eye.translation.x = 2.;
        let (right, ..) = portrait(&pane, &eye).unwrap();
        assert!(left.x < 0. && right.x > 0. && left.z == right.z);
        assert!(scale < 1. && opacity > 0. && opacity < 1.);
        eye.translation.z = 16.;
        assert!(portrait(&pane, &eye).unwrap().2 < opacity);
        eye.translation.z = -8.;
        assert!(portrait(&pane, &eye).is_none());
        eye.translation.z = 8.;
        eye.rotate_y(std::f32::consts::PI);
        assert!(portrait(&pane, &eye).is_none());
        let rear = pane.with_rotation(Quat::from_rotation_y(std::f32::consts::PI));
        eye.translation.z = -8.;
        assert!(portrait(&rear, &eye).is_some());
    }
}
