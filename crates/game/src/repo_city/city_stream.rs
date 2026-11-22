//! Keep the comparison intact while materializing the city near the camera.
use std::collections::HashMap;

use super::*;

const CELL: f32 = 160.;
pub(super) const RADIUS: f32 = 640.;
const EAGER_FILES: usize = 160;
const FILE_LIMIT: usize = 5000;
const DISTRICT_LIMIT: usize = 256;
const SPAWNS_PER_FRAME: usize = 24;
const REMOVALS_PER_FRAME: usize = 48;
const DISTRICT_VERTICES: usize = 4;
const BUILDING_VERTICES: usize = 12;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum Item {
    File(usize),
    District(usize),
}

#[derive(Component)]
pub(super) struct DistrictPart(pub usize);

#[derive(Default)]
pub(super) struct Stream {
    cells: HashMap<IVec2, Vec<Item>>,
    resident: HashSet<Item>,
    desired: Vec<Item>,
    retiring: Vec<Item>,
    checked: usize,
    eye_cell: Option<IVec2>,
}

fn cell(point: Vec2) -> IVec2 {
    (point / CELL).floor().as_ivec2()
}

impl Stream {
    pub(super) fn reset(&mut self, repo: &Repository) {
        *self = Self::default();
        if repo.files.len() <= EAGER_FILES {
            self.desired
                .extend((0..repo.districts.len()).map(Item::District));
            self.desired.extend((0..repo.files.len()).map(Item::File));
            return;
        }
        for (index, file) in repo.files.iter().enumerate() {
            self.cells
                .entry(cell(file.position.into()))
                .or_default()
                .push(Item::File(index));
        }
        for (index, district) in repo.districts.iter().enumerate() {
            let start = Vec2::from_array(district.position);
            let min = cell(start);
            let max = cell(start + Vec2::from_array(district.size));
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    self.cells
                        .entry(IVec2::new(x, y))
                        .or_default()
                        .push(Item::District(index));
                }
            }
        }
    }

    pub(super) fn refresh_files(&mut self, files: &HashSet<usize>) {
        for &file in files {
            self.resident.remove(&Item::File(file));
        }
        self.checked = 0;
    }

    pub(super) fn next(
        &mut self,
        repo: &Repository,
        eye: Vec2,
        selected: usize,
    ) -> (HashSet<Item>, Vec<Item>) {
        let large = repo.files.len() > EAGER_FILES;
        let eye_cell = cell(eye);
        if large && self.eye_cell != Some(eye_cell) {
            self.eye_cell = Some(eye_cell);
            let center = (eye_cell.as_vec2() + Vec2::splat(0.5)) * CELL;
            let min = cell(center - Vec2::splat(RADIUS));
            let max = cell(center + Vec2::splat(RADIUS));
            let mut candidates = HashSet::new();
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    if let Some(items) = self.cells.get(&IVec2::new(x, y)) {
                        candidates.extend(items.iter().copied());
                    }
                }
            }
            let mut nearby = candidates
                .into_iter()
                .filter_map(|item| {
                    let point = match item {
                        Item::File(index) => Vec2::from_array(repo.files[index].position),
                        Item::District(index) => {
                            let district = &repo.districts[index];
                            let min = Vec2::from_array(district.position);
                            center.clamp(min, min + Vec2::from_array(district.size))
                        }
                    };
                    let distance = center.distance_squared(point);
                    (distance <= RADIUS * RADIUS).then_some((item, distance))
                })
                .collect::<Vec<_>>();
            nearby.sort_unstable_by(|(a, ad), (b, bd)| ad.total_cmp(bd).then_with(|| a.cmp(b)));
            let (mut files, mut districts) = (0, 0);
            self.desired = nearby
                .into_iter()
                .filter_map(|(item, _)| {
                    let (count, limit) = match item {
                        Item::File(_) => (&mut files, FILE_LIMIT),
                        Item::District(_) => (&mut districts, DISTRICT_LIMIT),
                    };
                    *count += 1;
                    (*count <= limit).then_some(item)
                })
                .collect();
            let desired: HashSet<_> = self.desired.iter().copied().collect();
            self.retiring = self.resident.difference(&desired).copied().collect();
            self.retiring.sort_unstable();
            self.checked = 0;
        }
        let mut evicted = HashSet::new();
        for _ in 0..REMOVALS_PER_FRAME {
            let Some(item) = self.retiring.pop() else {
                break;
            };
            if self.resident.remove(&item) {
                evicted.insert(item);
            }
        }
        let limit = if large {
            SPAWNS_PER_FRAME.min(FILE_LIMIT.saturating_sub(self.resident.len()))
        } else {
            self.desired.len()
        };
        let first = Item::File(selected);
        let mut spawned = Vec::new();
        if large && limit > 0 && !self.resident.contains(&first) && self.desired.contains(&first) {
            self.resident.insert(first);
            spawned.push(first);
        }
        while spawned.len() < limit
            && let Some(&item) = self.desired.get(self.checked)
        {
            self.checked += 1;
            if self.resident.insert(item) {
                spawned.push(item);
            }
        }
        (evicted, spawned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_streams_nearby_buildings_in_bounded_batches_without_pruning_search_paths() {
        let mut repo = repository_picker::empty();
        repo.files = (0..6000)
            .map(|index| repo_world::File {
                path: format!("src/{index:04}.rs"),
                before: None,
                after: None,
                change: Change::Added,
                position: [(index % 100) as f32 * 27., (index / 100) as f32 * 30.],
            })
            .collect();
        let mut stream = Stream::default();
        stream.reset(&repo);
        let (_, first) = stream.next(&repo, Vec2::ZERO, 0);
        assert_eq!(first.len(), SPAWNS_PER_FRAME);
        assert_eq!(first[0], Item::File(0));
        assert!(!first.contains(&Item::File(5999)));
        let (_, second) = stream.next(&repo, Vec2::ZERO, 0);
        assert_eq!(second.len(), SPAWNS_PER_FRAME);
        assert!(second.iter().all(|file| !first.contains(file)));
        let (left, arrived) = stream.next(&repo, repo.files[5999].position.into(), 5999);
        assert!(first.iter().all(|item| left.contains(item)));
        assert_eq!(arrived.len(), SPAWNS_PER_FRAME);
        assert_eq!(arrived[0], Item::File(5999));
        assert_eq!(repo.files.len(), 6000);
    }

    #[test]
    fn free_flight_draws_distant_districts_and_buildings_beyond_the_stream_limit() {
        let mut repo = repository_picker::empty();
        repo.districts.push(repo_world::District {
            name: "remote".into(),
            position: [4000., 20.],
            size: [120., 80.],
        });
        repo.files = (0..=FILE_LIMIT)
            .map(|index| repo_world::File {
                path: format!("remote/{index}.bin"),
                before: None,
                after: None,
                change: Change::Unchanged,
                position: [4000. + index as f32 * 20., 40.],
            })
            .collect();
        let last = repo.files.last().unwrap().position[0];
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(WorldSet::load(None).unwrap())
            .init_resource::<file_facade::ChangesOnly>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, draw_distant_city);
        app.update();
        let world = app.world_mut();
        let (_, mesh) = world
            .query_filtered::<(Entity, &Mesh3d), With<DistantCity>>()
            .single(world)
            .expect("distant city is visible while flying freely");
        let handle = mesh.0.clone();
        let meshes = world.resource::<Assets<Mesh>>();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) = meshes
            .get(&handle)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("distant city must contain districts and buildings");
        };
        assert!(vertices[0][0] > 3999.);
        assert!(
            vertices
                .iter()
                .any(|[x, y, _]| (*x - last).abs() < 10. && *y > 10.)
        );
        assert!(
            vertices
                .iter()
                .any(|[x, y, _]| (*x - last).abs() < 10. && y.abs() < 0.1)
        );
    }

    // Given a project map and detailed buildings, filtering and restoring at
    // intermediate frames must keep each map roof attached to its moving body.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn distant_buildings_follow_the_same_filter_motion_as_building_parts() {
        use file_facade::{Part, SinkPart, transform};
        use std::time::Duration;

        let mut repo = repository_picker::empty();
        repo.baseline = Some("base".into());
        repo.files = [Change::Modified, Change::Unchanged]
            .into_iter()
            .enumerate()
            .map(|(index, change)| repo_world::File {
                path: format!("src/file_{index}.rs"),
                before: None,
                after: Some(repo_world::Blob {
                    id: format!("blob-{index}"),
                    size: 4000,
                    mode: "100644".into(),
                }),
                change,
                position: [index as f32 * 20., 0.],
            })
            .collect();
        let set = WorldSet::load(None).unwrap();
        let mut app = App::new();
        app.insert_resource(Repo(repo.clone()))
            .insert_resource(Explorer::new(0))
            .insert_resource(Time::<()>::default())
            .init_resource::<file_facade::ChangesOnly>()
            .init_resource::<listing::Listing>()
            .init_resource::<city_navigation::Navigation>()
            .add_systems(Update, file_facade::lower_unchanged);
        let parts: Vec<_> = repo
            .files
            .iter()
            .map(|file| {
                let size = listing::block_size(file);
                let mut body = app.world_mut().spawn(transform(file, size, Part::Body));
                if file.change == Change::Unchanged {
                    body.insert(SinkPart::new(file, size, Part::Body));
                }
                body.id()
            })
            .collect();
        app.update();
        let mut mesh = distant_city(
            &repo,
            app.world().resource::<file_facade::ChangesOnly>(),
            &set,
        );
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
            bevy::mesh::VertexAttributeValues::Float32x4(colors) => colors.clone(),
            _ => panic!("project map uses linear RGBA colors"),
        };
        let indices = mesh.indices().unwrap().iter().collect::<Vec<_>>();
        for enabled in [true, false, true, false] {
            if app.world().resource::<file_facade::ChangesOnly>().enabled != enabled {
                app.world_mut()
                    .resource_mut::<file_facade::ChangesOnly>()
                    .toggle();
            }
            for _ in 0..10 {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(Duration::from_millis(65));
                app.update();
                let filter = app.world().resource::<file_facade::ChangesOnly>();
                assert!(move_distant_buildings(&mut mesh, &repo, filter));
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("project map must contain roof positions");
                };
                assert_eq!(
                    vertices.len(),
                    BUILDING_VERTICES * 2,
                    "keep moving roofs until they sink below the street"
                );
                for (index, body) in parts.iter().enumerate() {
                    let body = app.world().get::<Transform>(*body).unwrap();
                    let top = body.translation.y + body.scale.y / 2.;
                    assert!(
                        (vertices[index * BUILDING_VERTICES][1] - (top - 0.05)).abs() < 0.0001,
                        "map roof {index} must move with the body while filter is {enabled}"
                    );
                }
                let rebuilt = distant_city(&repo, filter, &set);
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(rebuilt_vertices)) =
                    rebuilt.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("rebuilt project map must contain roof positions");
                };
                assert_eq!(
                    vertices, rebuilt_vertices,
                    "streaming during a transition uses the same heights"
                );
                let Some(bevy::mesh::VertexAttributeValues::Float32x4(current_colors)) =
                    mesh.attribute(Mesh::ATTRIBUTE_COLOR)
                else {
                    panic!("project map must retain its colors");
                };
                assert_eq!(&colors, current_colors);
                assert_eq!(indices, mesh.indices().unwrap().iter().collect::<Vec<_>>());
            }
        }
    }
}

/// The city silhouette stays visible while detailed buildings enter and leave the stream.
#[derive(Component)]
pub(super) struct DistantCity;

pub(super) fn district_at_ray(repo: &Repository, origin: Vec3, direction: Vec3) -> Option<usize> {
    district_at_point(repo, district_ground_point(origin, direction)?)
}

pub(super) fn district_ground_point(origin: Vec3, direction: Vec3) -> Option<Vec2> {
    if direction.y.abs() < 0.00001 {
        return None;
    }
    let distance = (-0.15 - origin.y) / direction.y;
    if distance < 0. || !distance.is_finite() {
        return None;
    }
    Some((origin + direction * distance).xz())
}

pub(super) fn district_at_point(repo: &Repository, point: Vec2) -> Option<usize> {
    repo.districts.iter().position(|district| {
        let min = Vec2::from_array(district.position);
        let max = min + Vec2::from_array(district.size);
        point.cmpge(min).all() && point.cmplt(max).all()
    })
}

pub(super) fn draw_distant_city(
    mut commands: Commands,
    repo: Res<Repo>,
    focus: Res<file_facade::ChangesOnly>,
    set: Res<WorldSet>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    maps: Query<(Entity, &Mesh3d), With<DistantCity>>,
    mut previous_progress: Local<Option<f32>>,
) {
    let enabled = repo.0.files.len() > EAGER_FILES;
    let progress = focus.animation_progress();
    if enabled
        && !repo.is_changed()
        && let Ok((_, handle)) = maps.single()
    {
        if *previous_progress == Some(progress) {
            return;
        }
        if let Some(mut mesh) = meshes.get_mut(&handle.0)
            && move_distant_buildings(&mut mesh, &repo.0, &focus)
        {
            *previous_progress = Some(progress);
            return;
        }
    }
    if !enabled && maps.is_empty() {
        return;
    }
    for (entity, _) in &maps {
        commands.entity(entity).despawn();
    }
    if enabled {
        *previous_progress = Some(progress);
        commands.spawn((
            DistantCity,
            bevy::light::NotShadowCaster,
            Mesh3d(meshes.add(distant_city(&repo.0, &focus, &set))),
            MeshMaterial3d(materials.add(StandardMaterial {
                unlit: true,
                cull_mode: None,
                ..default()
            })),
        ));
    }
}

fn distant_building_heights(
    file: &repo_world::File,
    focus: &file_facade::ChangesOnly,
    size: Vec3,
) -> [f32; 2] {
    let floor = focus.vertical_offset(file, size);
    // Leave the stand-in inside the detailed facade to avoid z-fighting while it streams.
    [floor + (size.y - 0.05).max(0.05), floor]
}

fn move_distant_buildings(
    mesh: &mut Mesh,
    repo: &Repository,
    focus: &file_facade::ChangesOnly,
) -> bool {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    else {
        return false;
    };
    let first = repo.districts.len() * DISTRICT_VERTICES;
    if vertices.len() != first + repo.files.len() * BUILDING_VERTICES {
        return false;
    }
    for (file, building) in repo
        .files
        .iter()
        .zip(vertices[first..].chunks_exact_mut(BUILDING_VERTICES))
    {
        let [roof, floor] = distant_building_heights(file, focus, listing::block_size(file));
        for (corners, height) in building
            .chunks_exact_mut(DISTRICT_VERTICES)
            .zip([roof, roof, floor])
        {
            for corner in corners {
                corner[1] = height;
            }
        }
    }
    true
}

fn distant_city(repo: &Repository, focus: &file_facade::ChangesOnly, set: &WorldSet) -> Mesh {
    use bevy::{
        asset::RenderAssetUsages,
        mesh::{Indices, PrimitiveTopology},
    };

    let corners = [
        Vec2::new(-1., -1.),
        Vec2::new(-1., 1.),
        Vec2::new(1., 1.),
        Vec2::new(1., -1.),
    ];
    let roof = [0, 1, 2, 0, 2, 3];
    let roof_trim = [
        0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7,
    ];
    let walls = [
        0, 8, 9, 0, 9, 1, 1, 9, 10, 1, 10, 2, 2, 10, 11, 2, 11, 3, 3, 11, 8, 3, 8, 0,
    ];
    let vertices = repo.files.len() * BUILDING_VERTICES + repo.districts.len() * DISTRICT_VERTICES;
    let mut positions = Vec::with_capacity(vertices);
    let mut colors = Vec::with_capacity(vertices);
    let mut indices = Vec::with_capacity(
        repo.files.len() * (roof.len() + roof_trim.len() + walls.len())
            + repo.districts.len() * roof.len(),
    );
    let ink = |rgb: [f32; 3]| {
        let linear = color(rgb).to_linear();
        [linear.red, linear.green, linear.blue, 1.]
    };
    let glass = code_texture::FACADE_GLASS.map(|value| f32::from(value) / 255.);
    for district in &repo.districts {
        let size = Vec2::from_array(district.size);
        let min = Vec2::from_array(district.position);
        let center = min + size * 0.5;
        let first = positions.len() as u32;
        for corner in corners {
            let point = center + corner * (size - Vec2::splat(0.5)) * 0.5;
            positions.push([point.x, -0.15, point.y]);
            colors.push(ink(set.ground));
        }
        indices.extend(roof.map(|index| first + index));
    }
    for file in &repo.files {
        let size = listing::block_size(file);
        let tint = match file.change {
            Change::Unchanged => code_texture::Coverage::Closed
                .accent()
                .map(|value| f32::from(value) / 255.),
            Change::Added => set.added,
            Change::Removed => set.removed,
            Change::Modified => set.accent,
        };
        let first = positions.len() as u32;
        let center = Vec2::from_array(file.position);
        let [top, floor] = distant_building_heights(file, focus, size);
        let trim = Vec3::from_array(glass).lerp(Vec3::from_array(tint), 0.52);
        for (height, scale, tone) in [
            (top, 0.48, ink(trim.to_array())),
            (top, 0.456, ink(glass)),
            (floor, 0.48, ink(set.shell)),
        ] {
            for corner in corners {
                let point = center + corner * size.xz() * scale;
                positions.push([point.x, height, point.y]);
                colors.push(tone);
            }
        }
        indices.extend(roof.map(|index| first + 4 + index));
        indices.extend(roof_trim.map(|index| first + index));
        indices.extend(walls.map(|index| first + index));
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}
