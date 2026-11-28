//! The bathyscaphe light and survey stations anchored to the seabed.
use super::*;
#[path = "chart.rs"]
mod chart;
#[path = "terrain.rs"]
mod terrain;
use bevy::light::NotShadowCaster;
use std::f32::consts::TAU;
use terrain::{Geometry, TILE_SIZE, WATER_CELL};

pub(super) const PILOT_EYE: Vec3 = Vec3::new(0., 0.43, -0.48);
pub(super) fn lens_center(side: f32) -> Vec3 {
    Vec3::new(side * 1.10, -0.30, -1.08)
}

#[derive(Resource)]
pub(crate) struct Seafloor {
    hull: Handle<Mesh>,
    ribs: Handle<Mesh>,
    sign: Handle<Mesh>,
    vent: Handle<Mesh>,
    shells: Handle<Mesh>,
    pearl: Handle<StandardMaterial>,
    finish: Handle<StandardMaterial>,
}

#[derive(Component)]
pub(crate) struct Station;
#[derive(Component)]
struct BedTile(IVec2);
#[derive(Component)]
struct WaterCell(IVec3);
#[derive(Component)]
struct ChartLight;
#[derive(Component)]
struct Backscatter;
#[derive(Component)]
struct SurveyLamp;

const SURVEY_INTENSITY: f32 = 7_000_000.;

pub(super) fn install(app: &mut App) {
    chart::install(app);
    app.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.56, 0.65, 0.98),
        brightness: 165.,
        ..default()
    })
    .add_systems(PostStartup, rig)
    .add_systems(
        Update,
        (trim_view, drift_snow, survey_lamps).after(city_navigation::navigate),
    );
}

pub(crate) fn rim_material(coverage: code_texture::Coverage) -> StandardMaterial {
    use code_texture::Coverage::*;
    let intensity = match coverage {
        Closed => 0.,
        Partial => 0.28,
        Open => 0.70,
    };
    StandardMaterial {
        base_color: Color::srgb(0.28, 0.61, 0.52),
        emissive: LinearRgba::new(intensity * 0.17, intensity, intensity * 0.72, 1.),
        metallic: 0.16,
        perceptual_roughness: 0.58,
        ..default()
    }
}

/// Narrow phosphor lenses across the station crown. Kept separate from its unpainted hull.
fn lenses() -> Mesh {
    let mut mesh = Geometry::default();
    for k in 0..32 {
        let a = k as f32 * TAU / 32.;
        let b = (k as f32 + 0.71) * TAU / 32.;
        let lens = |t: f32, r| Vec3::new(t.cos() * r, 0.07, t.sin() * r);
        mesh.quad(
            lens(a, 0.355),
            lens(b, 0.355),
            lens(b, 0.382),
            lens(a, 0.382),
            Color::WHITE,
        );
    }
    mesh.finish()
}

/// Thin, depth-tested slices approximate the light scattered back toward the pilot.
/// Nearby hulls occlude the farther water, while an empty trench shows the full beam.
fn scatter_slice() -> Mesh {
    use bevy::{
        asset::RenderAssetUsages,
        mesh::{Indices, PrimitiveTopology},
    };
    const SEGMENTS: u32 = 48;
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for (ring, (angle, opacity)) in [
        (0., 0.054),
        (LAMP_INNER_ANGLE * 0.55, 0.045),
        (LAMP_INNER_ANGLE, 0.032),
        ((LAMP_INNER_ANGLE + LAMP_OUTER_ANGLE) / 2., 0.017),
        (LAMP_OUTER_ANGLE, 0.),
    ]
    .into_iter()
    .enumerate()
    {
        for i in 0..SEGMENTS {
            let (sin, cos) = (TAU * i as f32 / SEGMENTS as f32).sin_cos();
            positions.push([cos * angle.tan(), sin * angle.tan(), 0.]);
            colors.push(
                Color::srgba(0.26, 0.60, 0.68, opacity)
                    .to_linear()
                    .to_f32_array(),
            );
            if ring > 0 {
                let base = (ring as u32 - 1) * SEGMENTS;
                let a = base + i;
                let b = base + (i + 1) % SEGMENTS;
                indices.extend([a, a + SEGMENTS, b + SEGMENTS, a, b + SEGMENTS, b]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; positions.len()])
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

fn rig(
    mut commands: Commands,
    pilot: Single<Entity, PlayerCamera>,
    set: Res<WorldSet>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let fog = color(set.background);
    commands.entity(*pilot).insert(DistanceFog {
        color: fog,
        directional_light_color: Color::NONE,
        falloff: FogFalloff::Exponential { density: 0.019 },
        ..default()
    });
    for side in [-1., 1.] {
        commands.spawn((
            Name::new("Survey lamp"),
            SurveyLamp,
            ChildOf(*pilot),
            SpotLight {
                color: Color::srgb(0.91, 1.0, 0.98),
                intensity: SURVEY_INTENSITY,
                range: LAMP_RANGE,
                inner_angle: LAMP_INNER_ANGLE,
                outer_angle: LAMP_OUTER_ANGLE,
                radius: 0.32,
                shadow_maps_enabled: side < 0. && !mobile_scene(),
                ..default()
            },
            Transform::from_translation(lens_center(side) - PILOT_EYE),
        ));
    }
    let scatter = meshes.add(scatter_slice());
    let haze = materials.add(StandardMaterial {
        unlit: true,
        fog_enabled: false,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    let water = commands
        .spawn((
            ChildOf(*pilot),
            Backscatter,
            Transform::IDENTITY,
            Visibility::Inherited,
        ))
        .id();
    for distance in [6., 18., 39.] {
        commands.spawn((
            ChildOf(water),
            NotShadowCaster,
            Mesh3d(scatter.clone()),
            MeshMaterial3d(haze.clone()),
            Transform::from_xyz(0., 0., -distance).with_scale(Vec3::splat(distance)),
        ));
    }
    commands.spawn((
        ChartLight,
        DirectionalLight {
            color: Color::srgb(0.66, 0.67, 1.),
            illuminance: 720.,
            ..default()
        },
        Transform::from_xyz(-15., 38., -22.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let seabed = meshes.add(terrain::seabed());
    let finish = materials.add(StandardMaterial {
        metallic: 0.11,
        perceptual_roughness: 0.84,
        ..default()
    });
    for i in -1..=1 {
        for j in -1..=1 {
            commands.spawn((
                BedTile(IVec2::new(i, j)),
                NotShadowCaster,
                Mesh3d(seabed.clone()),
                MeshMaterial3d(finish.clone()),
                Transform::from_xyz(i as f32 * TILE_SIZE, 0., j as f32 * TILE_SIZE),
            ));
        }
    }
    let snow = meshes.add(terrain::marine_snow());
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let cell = IVec3::new(x, y, z);
                commands.spawn((
                    WaterCell(cell),
                    NotShadowCaster,
                    Mesh3d(snow.clone()),
                    MeshMaterial3d(finish.clone()),
                    Transform::from_translation(cell.as_vec3() * WATER_CELL),
                ));
            }
        }
    }
    commands.insert_resource(Seafloor {
        hull: meshes.add(terrain::station()),
        ribs: meshes.add(lenses()),
        sign: meshes.add(Rectangle::new(1., file_facade::SIGN_ASPECT)),
        vent: meshes.add(terrain::vent()),
        shells: meshes.add(terrain::shell_colony()),
        pearl: materials.add(StandardMaterial {
            metallic: 0.20,
            perceptual_roughness: 0.40,
            clearcoat: 0.55,
            clearcoat_perceptual_roughness: 0.23,
            specular_tint: Color::srgb(0.94, 0.76, 0.97),
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        finish,
    });
}

fn drift_snow(
    camera: Single<&Transform, PlayerCamera>,
    time: Option<Res<Time>>,
    mut water: Query<(&WaterCell, &mut Transform), Without<Camera3d>>,
) {
    let elapsed = time.map_or(0., |time| time.elapsed_secs());
    let drift = Vec3::new(0.045, -0.12, -0.025) * elapsed;
    let center = ((camera.translation - drift) / WATER_CELL)
        .floor()
        .as_ivec3();
    for (cell, mut pose) in &mut water {
        pose.translation = (center + cell.0).as_vec3() * WATER_CELL + drift;
    }
}

fn trim_view(
    view: Single<(&Transform, &mut DistanceFog), PlayerCamera>,
    mut light: Single<&mut DirectionalLight, With<ChartLight>>,
    mut tiles: Query<(&BedTile, &mut Transform), Without<Camera3d>>,
    navigation: Option<Res<city_navigation::Navigation>>,
    mut was_map: Local<bool>,
) {
    let (eye, mut fog) = view.into_inner();
    let map = navigation
        .as_deref()
        .is_some_and(city_navigation::Navigation::overview);
    if map != *was_map {
        fog.falloff = if map {
            FogFalloff::Exponential { density: 0.00001 }
        } else {
            FogFalloff::Exponential { density: 0.019 }
        };
        light.illuminance = if map { 4400. } else { 720. };
        *was_map = map;
    }
    let center = (eye.translation.xz() / TILE_SIZE).floor().as_ivec2();
    for (tile, mut pose) in &mut tiles {
        let at = (center + tile.0).as_vec2() * TILE_SIZE;
        if pose.translation.x != at.x || pose.translation.z != at.y {
            pose.translation = Vec3::new(at.x, 0., at.y);
        }
    }
}

fn survey_lamps(
    eye: Single<&Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    navigation: Option<Res<city_navigation::Navigation>>,
    repo: Option<Res<Repo>>,
    explorer: Option<Res<Explorer>>,
    listing: Option<Res<listing::Listing>>,
    mut scatter: Single<&mut Visibility, With<Backscatter>>,
    mut lamps: Query<&mut SpotLight, With<SurveyLamp>>,
) {
    let aspect = windows
        .single()
        .map_or(16. / 9., |window| window.width() / window.height().max(1.));
    let map = navigation
        .as_ref()
        .is_some_and(|navigation| navigation.overview());
    let reading =
        repo.zip(explorer)
            .zip(listing)
            .is_some_and(|((repo, explorer), listing)| {
                navigation
                    .as_ref()
                    .is_some_and(|navigation| listing::Listing::visible(&explorer, navigation))
                    && repo.0.files.get(explorer.selected).is_some_and(|file| {
                        listing.close_face(file, &eye, &explorer, aspect).is_some()
                    })
            });
    let surveying = !map && !reading;
    scatter.set_if_neq(if surveying {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    for mut lamp in &mut lamps {
        let intensity = if surveying { SURVEY_INTENSITY } else { 0. };
        if lamp.intensity != intensity {
            lamp.intensity = intensity;
        }
    }
}

impl Seafloor {
    pub(crate) fn spawn_vent(
        &self,
        commands: &mut Commands,
        district: &repo_world::District,
        index: usize,
    ) {
        let [x, z] = district.position;
        commands.spawn((
            CityObject,
            city_stream::DistrictPart(index),
            Mesh3d(self.vent.clone()),
            MeshMaterial3d(self.finish.clone()),
            Transform::from_xyz(x - 2.5, 0., z - 0.5),
        ));
    }

    pub(crate) fn spawn_station(
        &self,
        commands: &mut Commands,
        palette: &Palette,
        index: usize,
        file: &repo_world::File,
        set: &WorldSet,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        use file_facade::{Part, transform};
        let size = listing::block_size(file);
        let hull = commands
            .spawn((
                CityObject,
                FileBuilding(index),
                Facade { index },
                Station,
                Mesh3d(self.hull.clone()),
                MeshMaterial3d(self.pearl.clone()),
                transform(file, size, Part::Body),
                RigidBody::Static,
                Collider::cuboid(1., 1., 1.),
            ))
            .id();
        commands.spawn((
            ChildOf(hull),
            Mesh3d(self.shells.clone()),
            MeshMaterial3d(self.pearl.clone()),
            Transform::from_xyz(0.57, -0.5, 0.36).with_scale(size.recip()),
        ));
        let ribs = commands
            .spawn((
                CityObject,
                FileBuilding(index),
                FacadeRim(index),
                NotShadowCaster,
                Mesh3d(self.ribs.clone()),
                MeshMaterial3d(palette.facade_rims[0].clone()),
                transform(file, size, Part::Roof),
            ))
            .id();
        let name = file.path.rsplit('/').next().unwrap_or(&file.path);
        let color = set.change(&file.change).to_srgba();
        let (sign, text) = code_texture::street_sign(name, [color.red, color.green, color.blue]);
        let sign = commands
            .spawn((
                CityObject,
                FileBuilding(index),
                file_facade::StreetSign { text },
                Mesh3d(self.sign.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color_texture: Some(images.add(sign)),
                    cull_mode: None,
                    perceptual_roughness: 0.9,
                    ..default()
                })),
                transform(file, size, Part::Label),
            ))
            .id();
        if file.change == Change::Unchanged {
            for (entity, part) in [(hull, Part::Body), (ribs, Part::Roof), (sign, Part::Label)] {
                commands
                    .entity(entity)
                    .insert(file_facade::SinkPart::new(file, size, part));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diving_carries_the_searchlight_and_read_stations_keep_their_phosphor() {
        // Given a bathyscaphe looking into the trench.
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .insert_resource(WorldSet::load(Some("abyss")).unwrap());
        install(&mut app);
        let pilot = app
            .world_mut()
            .spawn((Camera3d::default(), Pilot::default()))
            .id();
        app.update();

        // When the pilot dives and turns, the searchlight travels with it.
        let world = app.world_mut();
        let mut lights = world.query::<(&SpotLight, &ChildOf, &Transform)>();
        let mut mounts = Vec::new();
        for (light, mount, pose) in lights.iter(world) {
            assert_eq!(mount.parent(), pilot);
            assert_eq!(pose.forward(), Transform::IDENTITY.forward());
            assert_eq!(light.outer_angle, LAMP_OUTER_ANGLE);
            assert_eq!(light.range, LAMP_RANGE);
            mounts.push(pose.translation);
        }
        assert_eq!(mounts.len(), 2);
        for side in [-1., 1.] {
            assert!(mounts.contains(&(lens_center(side) - PILOT_EYE)));
        }
        assert!(world.get::<DistanceFog>(pilot).is_some());

        // Unread station ribs require incident light. Read ribs keep a cyan emission.
        use code_texture::Coverage;
        let unread = rim_material(Coverage::Closed);
        let partial = rim_material(Coverage::Partial);
        let read = rim_material(Coverage::Open);
        assert!(!unread.unlit && !partial.unlit && !read.unlit);
        assert_eq!(unread.emissive.green, 0.);
        assert!(partial.emissive.green > 0.);
        assert!(read.emissive.green > partial.emissive.green);
        assert!(read.emissive.blue > read.emissive.red);
    }
}
