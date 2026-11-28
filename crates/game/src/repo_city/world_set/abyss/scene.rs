//! The bathyscaphe light and survey stations anchored to the seabed.
use super::*;
#[path = "chart.rs"]
mod chart;
#[path = "terrain.rs"]
mod terrain;
use bevy::light::NotShadowCaster;
use std::f32::consts::TAU;
use terrain::{Geometry, TILE_SIZE};

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
struct ChartLight;
#[derive(Component)]
struct SurveyLamp;

const SURVEY_INTENSITY: f32 = 3_200_000.;
const WATER_LIGHT: f32 = 1050.;

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
        (trim_view, survey_lamps).after(city_navigation::navigate),
    );
}

/// Keep distant station contours visible between pools of light.
pub(crate) fn distant_tint(overview: bool) -> Color {
    if overview {
        Color::WHITE
    } else {
        Color::srgb(0.70, 0.80, 0.93)
    }
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

fn rig(
    mut commands: Commands,
    pilot: Single<Entity, PlayerCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
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
    commands.spawn((
        ChartLight,
        DirectionalLight {
            color: Color::srgb(0.66, 0.67, 1.),
            illuminance: WATER_LIGHT,
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

fn trim_view(
    eye: Single<&Transform, PlayerCamera>,
    mut light: Single<&mut DirectionalLight, With<ChartLight>>,
    mut tiles: Query<(&BedTile, &mut Transform), Without<Camera3d>>,
    navigation: Option<Res<city_navigation::Navigation>>,
) {
    let map = navigation
        .as_deref()
        .is_some_and(city_navigation::Navigation::overview);
    let illuminance = if map { 4400. } else { WATER_LIGHT };
    if light.illuminance != illuminance {
        light.illuminance = illuminance;
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
    let intensity = if !map && !reading {
        SURVEY_INTENSITY
    } else {
        0.
    };
    for mut lamp in &mut lamps {
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
        assert!(world.get::<DistanceFog>(pilot).is_none());

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
