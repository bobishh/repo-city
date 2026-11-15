//! Street lights and the player portrait shown by district glass.
use super::*;
use bevy::light::NotShadowCaster;

const TINTS: [[f32; 3]; 4] = [
    [0.97, 0.39, 0.67],
    [0.33, 0.85, 0.96],
    [0.97, 0.71, 0.39],
    [0.69, 0.55, 0.99],
];
const GLASS_WIDTH: f32 = 11.;
const GLASS_HEIGHT: f32 = 7.;

#[derive(Component)]
pub(super) struct Glass;

#[derive(Component)]
pub(super) struct Reflection;

pub(super) struct Lights {
    glass: Handle<Mesh>,
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
            glass: meshes.add(Rectangle::new(GLASS_WIDTH, GLASS_HEIGHT)),
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

        let pane = Vec3::new(x + 12., 4.25, max_z);
        commands.spawn((
            CityObject,
            Glass,
            NotShadowCaster,
            Mesh3d(self.glass.clone()),
            MeshMaterial3d(self.pane.clone()),
            Transform::from_translation(pane),
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
                NotShadowCaster,
                Mesh3d(meshes.add(mesh.expect("district has lights"))),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(origin),
            ));
        }
        label(
            commands,
            meshes,
            materials,
            images,
            &district.name,
            pane + Vec3::new(0., GLASS_HEIGHT / 2. + 1.4, 0.12),
            24.,
            TINTS[ink],
        );
    }
}

pub(super) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let robot = images.add(code_texture::embedded_png(include_bytes!(
        "../../../../assets/city/android-reflection.png"
    )));
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

fn portrait(pane: Vec3, eye: &Transform) -> Option<(Vec3, f32, f32)> {
    let offset = eye.translation - pane;
    if !(0.25..18.).contains(&offset.z)
        || offset.x.abs() >= GLASS_WIDTH / 2. + 2.
        || offset.y.abs() >= 7.
        || eye.forward().z >= -0.05
    {
        return None;
    }
    // Place the portrait on the glass at the viewer's reflected position.
    let scale = 0.66;
    let half_width = 3.5 * scale / 2.;
    let half_height = 5.25 * scale / 2.;
    let x = offset.x.clamp(
        -GLASS_WIDTH / 2. + half_width,
        GLASS_WIDTH / 2. - half_width,
    );
    let y = (offset.y - 0.8).clamp(
        -GLASS_HEIGHT / 2. + half_height,
        GLASS_HEIGHT / 2. - half_height,
    );
    let opacity = ((18. - offset.z) / 6.).clamp(0., 1.)
        * ((GLASS_WIDTH / 2. + 2. - offset.x.abs()) / 3.).clamp(0., 1.)
        * ((7. - offset.y.abs()) / 3.).clamp(0., 1.);
    Some((pane + Vec3::new(x, y, 0.035), scale, opacity * 0.62))
}

pub(super) fn reflect(
    camera: Query<&Transform, (With<Camera3d>, Without<Reflection>)>,
    panes: Query<&Transform, (With<Glass>, Without<Reflection>)>,
    mut sprite: Query<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        With<Reflection>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Ok(eye), Ok((mut pose, mut visibility, material))) =
        (camera.single(), sprite.single_mut())
    else {
        return;
    };
    let closest = panes
        .iter()
        .filter_map(|pane| portrait(pane.translation, eye))
        .min_by(|(a, _, _), (b, _, _)| {
            a.distance_squared(eye.translation)
                .total_cmp(&b.distance_squared(eye.translation))
        });
    let Some((position, scale, alpha)) = closest else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    visibility.set_if_neq(Visibility::Visible);
    pose.set_if_neq(Transform::from_translation(position).with_scale(Vec3::new(-scale, scale, 1.)));
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
        let mut eye = Transform::from_xyz(-2., 0.5, 8.);
        let (left, scale, opacity) = portrait(Vec3::ZERO, &eye).unwrap();
        eye.translation.x = 2.;
        let (right, ..) = portrait(Vec3::ZERO, &eye).unwrap();
        assert!(left.x < 0. && right.x > 0. && left.z == right.z);
        assert!(scale < 1. && opacity > 0. && opacity < 1.);
        eye.translation.z = 16.;
        assert!(portrait(Vec3::ZERO, &eye).unwrap().2 < opacity);
        eye.translation.z = -8.;
        assert!(portrait(Vec3::ZERO, &eye).is_none());
        eye.translation.z = 8.;
        eye.rotate_y(std::f32::consts::PI);
        assert!(portrait(Vec3::ZERO, &eye).is_none());
    }
}
