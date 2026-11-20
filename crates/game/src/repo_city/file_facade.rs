//! Physical file blocks and their signs share the listing's dimensions.
use super::*;

pub(super) const BEAM_HEIGHT: f32 = 14.;

#[derive(Resource, Default)]
pub(super) struct ChangesOnly {
    pub(super) available: bool,
    pub(super) enabled: bool,
    lowered: f32,
}

impl ChangesOnly {
    pub(super) fn toggle(&mut self) {
        if self.available {
            self.enabled = !self.enabled;
        }
    }
    pub(super) fn includes(&self, file: &repo_world::File) -> bool {
        !self.enabled || file.change != Change::Unchanged
    }

    pub(super) fn animation_progress(&self) -> f32 {
        self.lowered
    }

    fn eased(&self) -> f32 {
        self.lowered * self.lowered * (3. - 2. * self.lowered)
    }

    pub(super) fn vertical_offset(&self, file: &repo_world::File, size: Vec3) -> f32 {
        if file.change == Change::Unchanged {
            -SinkPart::depth(size) * self.eased()
        } else {
            0.
        }
    }
}

#[derive(Component)]
pub(super) struct SinkPart {
    street_y: f32,
    depth: f32,
}

impl SinkPart {
    pub(super) fn new(file: &repo_world::File, size: Vec3, part: Part) -> Self {
        Self {
            street_y: transform(file, size, part).translation.y,
            depth: Self::depth(size),
        }
    }

    fn depth(size: Vec3) -> f32 {
        size.y + BEAM_HEIGHT + 1.
    }

    fn height(&self, lowered: f32) -> f32 {
        self.street_y - self.depth * lowered
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = canFilterChanges)]
    fn browser_can_filter_changes() -> bool;
}

pub(super) fn lower_unchanged(
    time: Res<Time>,
    repo: Res<Repo>,
    mut focus: ResMut<ChangesOnly>,
    mut explorer: ResMut<Explorer>,
    mut listing: ResMut<listing::Listing>,
    mut navigation: ResMut<city_navigation::Navigation>,
    mut comparison: Local<Option<(PathBuf, String, Option<String>)>>,
    mut parts: ParamSet<(
        Query<(&SinkPart, &mut Transform)>,
        Query<(&SinkPart, &mut Transform), Added<SinkPart>>,
    )>,
) {
    #[cfg(target_arch = "wasm32")]
    let available = browser_can_filter_changes();
    #[cfg(not(target_arch = "wasm32"))]
    let available = repo.0.baseline.is_some();
    focus.available = available;
    if repo.is_changed() {
        let identity = (
            repo.0.root.clone(),
            repo.0.revision.clone(),
            repo.0.baseline.clone(),
        );
        if comparison.as_ref() != Some(&identity) {
            *comparison = Some(identity);
            focus.enabled = false;
        }
    }
    if !available {
        focus.enabled = false;
    }
    if !focus.includes(&repo.0.files[explorer.selected])
        && let Some(index) = repo.0.files.iter().position(|file| focus.includes(file))
    {
        explorer.open(&repo.0, index);
        explorer.anchored = false;
        explorer.comparison_mode = 0;
        listing.reset_view();
        *navigation = city_navigation::Navigation::default();
    }
    let target = f32::from(focus.enabled);
    if focus.lowered == target {
        if target > 0. {
            for (part, mut pose) in &mut parts.p1() {
                pose.translation.y = part.height(target);
            }
        }
        return;
    }
    let step = time.delta_secs() / 0.65;
    focus.lowered += (target - focus.lowered).clamp(-step, step);
    let eased = focus.eased();
    for (part, mut pose) in &mut parts.p0() {
        pose.translation.y = part.height(eased);
    }
}

#[derive(Clone, Copy)]
pub(super) struct Aim {
    pub index: usize,
    pub surface: Option<Surface>,
}

#[derive(Clone, Copy)]
pub(super) struct Surface {
    pub normal: Vec3,
    pub height: f32,
}

impl Aim {
    pub(super) fn within_tool_reach(
        self,
        file: &repo_world::File,
        size: Vec3,
        eye: Vec3,
        hit_distance: f32,
    ) -> bool {
        let Some(surface) = self.surface.filter(|surface| surface.normal.y == 0.) else {
            return hit_distance <= listing::tool_range(size.x);
        };
        let [x, z] = file.position;
        let normal = surface.normal;
        let gap = (eye - Vec3::new(x, 0., z)).dot(normal)
            - normal.abs().dot(size) / 2.
            - listing::PANEL_OFFSET;
        let width = if normal.x == 0. { size.x } else { size.z };
        gap <= listing::tool_range(width)
    }

    pub(super) fn on_side(self, face: listing::Face) -> Self {
        Self {
            surface: Some(Surface {
                normal: face.normal(),
                height: self.surface.map_or(0.9, |surface| surface.height.min(0.9)),
            }),
            ..self
        }
    }

    pub(super) fn view(self, file: &repo_world::File, size: Vec3, eye: Vec3) -> Transform {
        let [x, z] = file.position;
        let center = Vec3::new(x, 0., z);
        let toward_eye = eye - center;
        let side = if toward_eye.x.abs() > toward_eye.z.abs() {
            Vec3::X * toward_eye.x.signum()
        } else if toward_eye.z < 0. {
            Vec3::NEG_Z
        } else {
            Vec3::Z
        };
        let surface = self.surface.unwrap_or(Surface {
            normal: side,
            height: 0.9,
        });
        let normal = if surface.normal == Vec3::ZERO {
            side
        } else {
            surface.normal
        };
        let (target, width, up) = if normal == Vec3::Y {
            (center.with_y(size.y), size.x.max(size.z), Vec3::NEG_Z)
        } else {
            let width = if normal.x == 0. { size.x } else { size.z };
            let target = center.with_y(size.y * surface.height)
                + normal * (normal.abs().dot(size) / 2. + listing::PANEL_OFFSET);
            (target, width, Vec3::Y)
        };
        let position = target + normal * (width * 0.84).max(0.18);
        Transform::from_translation(
            position.with_y(position.y.max(city_navigation::STREET_EYE_HEIGHT)),
        )
        .looking_at(target, up)
    }
}

pub(super) fn beam_material([r, g, b]: [f32; 3]) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(r, g, b),
        unlit: true,
        ..default()
    }
}

pub(super) fn approach(eye: &Transform, min: Vec3, max: Vec3) -> Option<f32> {
    let forward = eye.forward().as_vec3();
    // The front and both sides are readable, the rear remains a mirror.
    [Vec3::Z, Vec3::X, Vec3::NEG_X]
        .into_iter()
        .filter_map(|normal| {
            let facing = forward.dot(normal);
            let plane = if normal.x < 0. { min } else { max };
            let gap = (eye.translation - plane).dot(normal);
            if facing >= -0.12 || gap <= 0. {
                return None;
            }
            let distance = gap / -facing;
            let point = eye.translation + forward * distance;
            let tolerance = Vec3::splat(0.002);
            (point.cmpge(min - tolerance).all() && point.cmple(max + tolerance).all())
                .then_some((distance, gap))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, gap)| gap)
}

#[derive(Clone, Copy)]
pub(super) enum Part {
    Body,
    Roof,
    Stripe(f32),
    Label,
    Beam,
    Mirror,
}

pub(super) fn roof_rim() -> Mesh {
    use bevy::{
        asset::RenderAssetUsages,
        mesh::{Indices, PrimitiveTopology},
    };

    let corners = [[-0.5, -0.5], [-0.5, 0.5], [0.5, 0.5], [0.5, -0.5]];
    let positions: Vec<_> = corners
        .into_iter()
        .map(|[x, z]| [x, 0., z])
        .chain(corners.into_iter().map(|[x, z]| [x * 0.96, 0., z * 0.96]))
        .chain(corners.into_iter().map(|[x, z]| [x, -1., z]))
        .collect();
    let indices = (0..4)
        .flat_map(|i| {
            let next = (i + 1) % 4;
            [
                i,
                next,
                next + 4,
                i,
                next + 4,
                i + 4,
                i,
                i + 8,
                next + 8,
                i,
                next + 8,
                next,
            ]
        })
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; 12])
    .with_inserted_indices(Indices::U32(indices))
}

pub(super) fn transform(file: &repo_world::File, size: Vec3, part: Part) -> Transform {
    let [x, z] = file.position;
    match part {
        Part::Body => Transform::from_xyz(x, size.y / 2., z).with_scale(size),
        Part::Roof => Transform::from_xyz(x, size.y + 0.003, z).with_scale(Vec3::new(
            size.x + 2. * listing::PANEL_OFFSET,
            (size.x * 0.013).clamp(0.035, 0.11),
            size.z + 2. * listing::PANEL_OFFSET,
        )),
        Part::Stripe(side) => {
            let width = size.x * 0.016;
            Transform::from_xyz(
                x - size.x / 2. + width / 2.,
                size.y / 2.,
                z + side * (size.z / 2. + listing::PANEL_OFFSET + 0.015),
            )
            .with_scale(Vec3::new(width, size.y, 0.02))
        }
        Part::Label => {
            let width = size.x.clamp(4., 12.);
            Transform::from_xyz(x, size.y + 0.35 + width / 24., z + size.z / 2.)
                .with_scale(Vec3::new(width, width, 1.))
        }
        Part::Beam => Transform::from_xyz(x, size.y + BEAM_HEIGHT / 2., z).with_scale(Vec3::new(
            0.15,
            BEAM_HEIGHT,
            0.15,
        )),
        Part::Mirror => Transform::from_xyz(x, size.y / 2., z - size.z / 2. - 0.04)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
            .with_scale(Vec3::new(size.x, size.y, 1.)),
    }
}

#[derive(Component)]
pub(super) struct FileBeam;

pub(super) struct ReadingPlane {
    normal: Vec3,
    gap: f32,
    silhouette: f32,
}

pub(super) fn reading_plane(
    repo: &Repository,
    explorer: &Explorer,
    camera: &Transform,
) -> Option<ReadingPlane> {
    let file = repo.files.get(explorer.selected)?;
    let size = listing::block_size(file);
    let face = listing::Face::at_camera(file, size, camera, (size.x * 1.85).max(8.))?;
    let normal = face.normal();
    let [x, z] = file.position;
    let center = Vec3::new(x, 0., z);
    let gap = (camera.translation - center).dot(normal) - normal.abs().dot(size) / 2.;
    let side = Vec3::Y.cross(normal);
    let silhouette = (camera.translation - center).dot(side).abs() + size.dot(side.abs()) / 2.;
    (gap > 0.).then_some(ReadingPlane {
        normal,
        gap,
        silhouette,
    })
}

pub(super) fn beam_visible(
    beam: &Transform,
    camera: &Transform,
    reading: Option<&ReadingPlane>,
) -> bool {
    let delta = camera.translation - beam.translation;
    let half = beam.scale.abs() / 2.;
    if (delta.abs() - half).max(Vec3::ZERO).length_squared() < 16. {
        return false;
    }
    if let Some(reading) = reading {
        let depth = delta.dot(reading.normal);
        let side = Vec3::Y.cross(reading.normal);
        if depth > -half.dot(reading.normal.abs())
            && depth < reading.gap + half.dot(reading.normal.abs())
            && delta.dot(side).abs() < reading.silhouette + 0.75
        {
            return false;
        }
    }
    true
}

pub(super) fn keep_beams_clear(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    camera: Query<&Transform, PlayerCamera>,
    mut beams: Query<(&Transform, &mut Visibility), Or<(With<FileBeam>, With<district::Beacon>)>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let reading = reading_plane(&repo.0, &explorer, camera);
    for (pose, mut visibility) in &mut beams {
        let visible = beam_visible(pose, camera, reading.as_ref());
        visibility.set_if_neq(if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

pub(super) fn kind(path: &str) -> usize {
    match extension(path) {
        "rs" | "go" | "c" | "h" | "cpp" | "swift" => 0,
        "js" | "ts" | "jsx" | "tsx" | "svelte" | "html" | "css" => 1,
        "toml" | "json" | "yml" | "yaml" | "xml" | "lock" => 2,
        "md" | "txt" | "rst" | "adoc" => 3,
        _ => 4,
    }
}

pub(super) fn extension(path: &str) -> &str {
    std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
}

pub(super) const INK: [[f32; 3]; 5] = [
    [1., 0.53, 0.30],
    [0.29, 0.86, 1.],
    [0.84, 0.64, 1.],
    [0.45, 0.94, 0.66],
    [0.80, 0.82, 0.87],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_changes_filter_lowers_every_part_and_restores_its_street_position() {
        let mut file = repo_world::File {
            path: "unchanged.rs".into(),
            before: None,
            after: None,
            change: Change::Unchanged,
            position: [70., 28.],
        };
        let focus = ChangesOnly {
            enabled: true,
            ..default()
        };
        assert!(!focus.includes(&file));
        for size in [Vec3::new(4., 1., 4.), Vec3::new(16., 130., 12.)] {
            for part in [
                Part::Body,
                Part::Roof,
                Part::Stripe(1.),
                Part::Label,
                Part::Beam,
                Part::Mirror,
            ] {
                let pose = transform(&file, size, part);
                let sink = SinkPart::new(&file, size, part);
                assert_eq!(sink.height(0.), pose.translation.y);
                assert!(sink.height(1.) + pose.scale.y / 2. <= -1.);
            }
        }
        for change in [Change::Added, Change::Modified, Change::Removed] {
            file.change = change;
            assert!(focus.includes(&file));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_building_streamed_after_the_filter_settles_starts_below_the_street() {
        let mut repo = repository_picker::empty();
        repo.baseline = Some("base".into());
        repo.files.push(repo_world::File {
            path: "changed.rs".into(),
            before: None,
            after: None,
            change: Change::Added,
            position: [0., 0.],
        });
        let mut app = App::new();
        app.insert_resource(Repo(repo))
            .insert_resource(Explorer::new(0))
            .insert_resource(Time::<()>::default())
            .init_resource::<ChangesOnly>()
            .init_resource::<listing::Listing>()
            .init_resource::<city_navigation::Navigation>()
            .add_systems(Update, lower_unchanged);
        app.update();
        {
            let mut filter = app.world_mut().resource_mut::<ChangesOnly>();
            filter.toggle();
            filter.lowered = 1.;
        }
        let building = app
            .world_mut()
            .spawn((
                SinkPart {
                    street_y: 5.,
                    depth: 30.,
                },
                Transform::from_xyz(4., 5., 6.),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .get::<Transform>(building)
                .unwrap()
                .translation
                .y,
            -25.
        );
    }

    #[test]
    fn the_beacon_is_cleared_from_the_camera_and_reading_wall() {
        let camera = Transform::from_xyz(12., 30., 25.);
        let reading = ReadingPlane {
            normal: Vec3::Z,
            gap: 12.,
            silhouette: 7.,
        };
        let beam = |x, z| Transform::from_xyz(x, 30., z).with_scale(Vec3::new(0.34, 72., 0.34));
        assert!(!beam_visible(&beam(12.1, 24.), &camera, None));
        assert!(!beam_visible(&beam(13., 18.), &camera, Some(&reading)));
        assert!(beam_visible(&beam(24., 18.), &camera, Some(&reading)));
        assert!(beam_visible(&beam(13., 8.), &camera, Some(&reading)));
    }

    #[test]
    fn mirror_covers_the_rear_face_from_ground_to_roof() {
        let mut file = repo_world::File {
            path: "demo.rs".into(),
            before: None,
            after: None,
            change: Change::Unchanged,
            position: [70., 28.],
        };
        for count in [23, 724, 1614] {
            file.after = Some(repo_world::Blob {
                id: String::new(),
                size: count * 40,
                mode: "100644".into(),
            });
            let size = listing::block_size(&file);
            let mirror = transform(&file, size, Part::Mirror);
            assert_eq!(mirror.scale.xy(), size.xy());
            assert_eq!(mirror.translation.y - mirror.scale.y / 2., 0.);
            assert_eq!(mirror.translation.y + mirror.scale.y / 2., size.y);
        }
    }

    #[test]
    fn proximity_opens_reading_walls_but_leaves_the_mirror_free() {
        let min = Vec3::new(-4., 0., -2.);
        let max = Vec3::new(4., 8., 2.);
        let front = Transform::from_xyz(0., 3., 7.);
        assert_eq!(approach(&front, min, max), Some(5.));
        let side = front.with_translation(Vec3::new(4.1, 3., 7.));
        assert_eq!(approach(&side, min, max), None);
        let right = Transform::from_xyz(9., 3., 0.).looking_to(Vec3::NEG_X, Vec3::Y);
        assert!((approach(&right, min, max).unwrap() - 5.).abs() < 0.001);
        let rear = Transform::from_xyz(0., 3., -7.).looking_to(Vec3::Z, Vec3::Y);
        assert_eq!(approach(&rear, min, max), None);
    }
}
