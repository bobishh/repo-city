//! Every file occupies a building on its folder’s mountain terrace.
use bevy::{
    asset::RenderAssetUsages,
    camera::ScalingMode,
    mesh::{Indices, PrimitiveTopology},
    shape::{Aabb3d, RayCast3d},
};

use super::*;

pub(super) const LAYER: usize = 8;
const ANGLE: f32 = -1.21;
const TURN: f32 = 0.24;

struct Lot {
    index: usize,
    directory: usize,
    center: Vec3,
    size: Vec3,
    color: [f32; 3],
}

#[derive(Clone, Copy)]
struct Terrace {
    center: Vec2,
    radius: Vec2,
    height: f32,
    phase: f32,
}

#[derive(Clone, Copy)]
struct Lake {
    center: Vec2,
    radius: Vec2,
    level: f32,
    phase: f32,
}

impl Lake {
    fn new(center: Vec2, radius: Vec2, phase: f32) -> Self {
        Self {
            center,
            radius,
            level: ground_height(center) - 1.4,
            phase,
        }
    }

    fn shoreline(self, angle: f32) -> f32 {
        1. + (angle * 3. + self.phase).sin() * 0.10 + (angle * 5. - self.phase).cos() * 0.055
    }

    fn basin(self, point: Vec2, ground: f32) -> f32 {
        let offset = (point - self.center) / self.radius;
        let distance = offset.length();
        if distance >= 1.48 {
            return ground;
        }
        let edge = distance / self.shoreline(offset.y.atan2(offset.x));
        let bank = ((edge - 1.05) / 0.19).clamp(0., 1.);
        let bottom = (self.level - 2.7).min(ground);
        bottom.lerp(ground, bank * bank * (3. - 2. * bank))
    }
}

pub(super) struct Terrain {
    terraces: Vec<Terrace>,
    lakes: Vec<Lake>,
}

pub(super) struct Atlas {
    lots: Vec<Lot>,
    terraces: Vec<(usize, Terrace)>,
    center: Vec3,
    size: Vec2,
    distance: f32,
    terrain_bounds: (Vec2, Vec2),
    lakes: Vec<Lake>,
}

impl Atlas {
    pub(super) fn terrain(catalog: &Catalog) -> Terrain {
        const TURN: f32 = 2.39996;
        const GRID: f32 = 40.;
        let mut groups: [Vec<usize>; 3] = std::array::from_fn(|_| Vec::new());
        for (index, folder) in catalog.folders.iter().enumerate() {
            let branch = folder.path.split('/').next().unwrap_or("");
            let seed = branch.bytes().fold(0_u32, |seed, byte| {
                seed.wrapping_mul(31).wrapping_add(u32::from(byte))
            });
            groups[seed as usize % 3].push(index);
        }
        let mut positions = vec![
            Terrace {
                center: Vec2::ZERO,
                radius: Vec2::ZERO,
                height: 0.,
                phase: 0.,
            };
            catalog.folders.len()
        ];
        let cell = |point: Vec2| (point / GRID).floor().as_ivec2();
        let mut ridge_bounds = [None; 3];
        for (ridge, group) in groups.iter_mut().enumerate() {
            group.sort_by_key(|&index| std::cmp::Reverse(catalog.folders[index].files.len()));
            let mut cells = std::collections::HashMap::<(i32, i32), Vec<usize>>::new();
            let mut minimum = Vec2::splat(f32::INFINITY);
            let mut maximum = Vec2::splat(f32::NEG_INFINITY);
            for &index in group.iter() {
                let phase = index as f32 * TURN;
                let radius = 12.8 * (catalog.folders[index].files.len() as f32).sqrt() + 5.2;
                let dimensions = Vec2::new(radius * (1. + phase.sin() * 0.11), radius);
                let padding = dimensions.max_element() * 1.05 + 3.8;
                for attempt in 0.. {
                    let angle = phase + attempt as f32 * TURN;
                    let step = (attempt as f32).sqrt() * 15.;
                    let center = Vec2::new(angle.cos() * 1.36, angle.sin() * 0.74) * step;
                    let low = cell(center - Vec2::splat(padding));
                    let high = cell(center + Vec2::splat(padding));
                    let overlaps = (low.x..=high.x).any(|x| {
                        (low.y..=high.y).any(|y| {
                            cells.get(&(x, y)).is_some_and(|group| {
                                group.iter().any(|&other| {
                                    let old = positions[other];
                                    let gap = padding + old.radius.max_element() * 1.05 + 3.8;
                                    old.center.distance_squared(center) < gap * gap
                                })
                            })
                        })
                    });
                    if overlaps {
                        continue;
                    }
                    positions[index] = Terrace {
                        center,
                        radius: dimensions,
                        height: 0.,
                        phase,
                    };
                    minimum = minimum.min(center - dimensions * 1.06);
                    maximum = maximum.max(center + dimensions * 1.06);
                    for x in low.x..=high.x {
                        for y in low.y..=high.y {
                            cells.entry((x, y)).or_default().push(index);
                        }
                    }
                    break;
                }
            }
            if !group.is_empty() {
                ridge_bounds[ridge] = Some((minimum, maximum));
            }
        }
        let (width, depth) = ridge_bounds
            .iter()
            .flatten()
            .fold((0_f32, 0_f32), |(width, depth), (min, max)| {
                (width + max.x - min.x, depth.max(max.y - min.y))
            });
        let valley = (depth * 0.19).max(24.);
        let ridge_count = ridge_bounds.iter().flatten().count();
        let gaps = ridge_count.saturating_sub(1);
        let mut left = -(width + valley * gaps as f32) * 0.5;
        let mut lakes = Vec::new();
        let mut placed = 0;
        for ((group, bounds), stagger) in groups.iter().zip(ridge_bounds).zip([-0.12, 0.16, -0.18])
        {
            if let Some((min, max)) = bounds {
                let shift = Vec2::new(left - min.x, -(min.y + max.y) * 0.5 + depth * stagger);
                for &index in group {
                    let terrace = &mut positions[index];
                    terrace.center += shift;
                    terrace.height = (ground_height(terrace.center)
                        + terrace.radius.max_element() * 0.17)
                        .min(39.)
                        + 7.;
                }
                left += max.x - min.x;
                if placed < gaps {
                    let y = depth * if placed == 0 { 0.08 } else { -0.19 };
                    lakes.push(Lake::new(
                        Vec2::new(left + valley * 0.5, y),
                        Vec2::new(
                            valley * 0.34,
                            valley * if placed == 0 { 0.84 } else { 0.74 },
                        ),
                        0.95 + placed as f32 * 1.8,
                    ));
                }
                placed += 1;
                left += valley;
            }
        }
        if ridge_count == 1 {
            lakes.push(Lake::new(
                Vec2::new(left - valley * 0.35, depth * 0.17),
                Vec2::new(valley * 0.34, valley * 0.68),
                2.3,
            ));
        }
        Terrain {
            terraces: positions,
            lakes,
        }
    }

    pub(super) fn new(
        repo: &Repository,
        catalog: &Catalog,
        terrain: &Terrain,
        folder: Option<usize>,
    ) -> Option<Self> {
        let mut lots = Vec::new();
        let mut terraces = Vec::new();
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for (group, members) in catalog.folders.iter().enumerate() {
            if folder.is_some_and(|index| index != group) {
                continue;
            }
            let terrace = terrain.terraces[group];
            let Terrace {
                center,
                radius,
                height,
                phase,
            } = terrace;
            let steps = (members.files.len() as f32).sqrt();
            let spacing = (radius.min_element() * 1.28 / steps).clamp(7., 21.);
            for (slot, &index) in members.files.iter().enumerate() {
                let file = &repo.files[index];
                let dimensions = listing::block_size(file);
                let level = (2.3 + dimensions.y.ln_1p() * 2.15).min(15.);
                let size = Vec3::new(
                    dimensions.x.clamp(7., spacing),
                    level,
                    dimensions.z.clamp(7., spacing),
                ) * Vec3::new(0.83, 1., 0.83);
                let offset = if members.files.len() == 1 {
                    Vec2::ZERO
                } else {
                    let angle = phase + slot as f32 * 2.39996;
                    let toward_edge = ((slot as f32 + 0.5) / members.files.len() as f32).sqrt();
                    Vec2::new(angle.cos(), angle.sin()) * radius * toward_edge * 0.72
                };
                let point = center + offset;
                let change = if repo.baseline.is_some() {
                    file.change.clone()
                } else {
                    Change::Unchanged
                };
                let color = match change {
                    Change::Unchanged => [0.79, 0.48, 0.33],
                    Change::Added => [0.67, 0.72, 0.49],
                    Change::Removed => [0.77, 0.34, 0.37],
                    Change::Modified => [0.95, 0.71, 0.38],
                };
                lots.push(Lot {
                    index,
                    directory: group,
                    center: Vec3::new(point.x, height + level / 2. + 0.08, point.y),
                    size,
                    color,
                });
            }
            min = min.min(center - radius * 1.06);
            max = max.max(center + radius * 1.06);
            terraces.push((group, terrace));
        }
        if lots.is_empty() {
            return None;
        }
        let lakes = if folder.is_none() {
            terrain.lakes.clone()
        } else {
            Vec::new()
        };
        for lake in &lakes {
            min = min.min(lake.center - lake.radius * 1.17);
            max = max.max(lake.center + lake.radius * 1.17);
        }
        lots.sort_unstable_by_key(|lot| lot.index);
        let rotation = rotation();
        let roof = lots
            .iter()
            .map(|lot| lot.center.y + lot.size.y * 0.5)
            .fold(0., f32::max);
        let mid = (min + max) * 0.5;
        let center = Vec3::new(mid.x, roof * 0.5, mid.y);
        let half = (max - min) / 2.;
        let mut size = Vec2::ZERO;
        for x in [-half.x, half.x] {
            for y in [-roof * 0.5, roof * 0.5] {
                for z in [-half.y, half.y] {
                    let projected = rotation.inverse() * Vec3::new(x, y, z);
                    size = size.max(projected.xy().abs() * 2.);
                }
            }
        }
        Some(Self {
            lots,
            terraces,
            center,
            size,
            distance: (max - min).length() + 70.,
            terrain_bounds: (min, max),
            lakes,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.lots.len()
    }

    pub(super) fn file(&self, slot: usize) -> Option<usize> {
        self.lots.get(slot).map(|lot| lot.index)
    }

    pub(super) fn slot(&self, file: usize) -> Option<usize> {
        self.lots.iter().position(|lot| lot.index == file)
    }

    pub(super) fn hit(&self, ray: Ray3d) -> Option<usize> {
        let ray = RayCast3d::from_ray(ray, self.distance * 3.);
        self.lots
            .iter()
            .enumerate()
            .filter_map(|(slot, lot)| {
                ray.aabb_intersection_at(&Aabb3d::new(lot.center, lot.size * 0.5))
                    .map(|distance| (slot, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(slot, _)| slot)
    }

    pub(super) fn directory(&self, ray: Ray3d, lot: Option<usize>) -> Option<usize> {
        if let Some(lot) = lot.and_then(|slot| self.lots.get(slot)) {
            return Some(lot.directory);
        }
        if ray.direction.y.abs() < 0.001 {
            return None;
        }
        self.terraces.iter().find_map(|&(directory, terrace)| {
            let distance = (terrace.height - ray.origin.y) / ray.direction.y;
            let point = ray.get_point(distance);
            let offset = (Vec2::new(point.x, point.z) - terrace.center) / terrace.radius;
            let angle = offset.y.atan2(offset.x);
            let bevel = 0.89 + 0.10 * (angle * 3. + terrace.phase).sin();
            (distance >= 0. && offset.length() <= bevel).then_some(directory)
        })
    }

    pub(super) fn view(&self, shift: Vec2, zoom: f32) -> (Transform, Projection) {
        let rotation = rotation();
        let center = self.center + rotation * shift.extend(0.);
        let position = center + rotation * Vec3::Z * self.distance;
        let size = self.minimum_view();
        (
            Transform::from_translation(position).with_rotation(rotation),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::AutoMin {
                    min_width: size.x,
                    min_height: size.y,
                },
                scale: zoom,
                far: self.distance * 3.,
                ..OrthographicProjection::default_3d()
            }),
        )
    }

    pub(super) fn framing_from(
        &self,
        old: &Self,
        shift: Vec2,
        zoom: f32,
        aspect: f32,
    ) -> (Vec2, f32) {
        let center_offset = (rotation().inverse() * (old.center - self.center)).xy();
        (
            shift + center_offset,
            zoom * old.view_height(aspect) / self.view_height(aspect),
        )
    }

    pub(super) fn view_height(&self, aspect: f32) -> f32 {
        let size = self.minimum_view();
        (size.x / aspect.max(0.1)).max(size.y)
    }

    fn minimum_view(&self) -> Vec2 {
        self.size * Vec2::new(1.13, 1.28)
    }

    pub(super) fn lot_bounds(&self, slot: usize) -> Option<(Vec3, Vec3)> {
        self.lots.get(slot).map(|lot| (lot.center, lot.size))
    }

    pub(super) fn focus_shift(&self, file: usize) -> Option<Vec2> {
        let (center, size) = self.lot_bounds(self.slot(file)?)?;
        Some((rotation().inverse() * (center + Vec3::Y * size.y / 2. - self.center)).xy())
    }

    pub(super) fn mesh(&self) -> Mesh {
        let mut mesh = Blocks::default();
        mesh.ground(self.terrain_bounds, &self.lakes);
        for lake in &self.lakes {
            mesh.lake(*lake);
        }
        for (_, terrace) in &self.terraces {
            mesh.terrace(terrace);
        }
        for lot in &self.lots {
            mesh.building(lot);
        }
        mesh.finish()
    }
}

fn ground_height(point: Vec2) -> f32 {
    let ridge = ((point.x + point.y * 0.55) * 0.0071).sin();
    let valley = (point.y * 0.0052 + 0.4).cos();
    let hills = ((point.x - point.y * 0.8) * 0.0093).cos();
    13. + ridge * valley * 11. + hills * 5.5
}

fn rotation() -> Quat {
    Quat::from_euler(EulerRot::YXZ, TURN, ANGLE, 0.)
}

#[derive(Default)]
struct Blocks {
    vertices: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Blocks {
    fn ground(&mut self, (min, max): (Vec2, Vec2), lakes: &[Lake]) {
        const SAMPLES: usize = 60;
        let margin = Vec2::splat(((max - min).length() * 0.98).max(34.));
        let start = min - margin;
        let end = max + margin;
        let axis = |index: usize| {
            let mut values = (0..=SAMPLES)
                .map(|step| start[index].lerp(end[index], step as f32 / SAMPLES as f32))
                .collect::<Vec<_>>();
            for lake in lakes {
                for sample in -6..=6 {
                    values.push(lake.center[index] + lake.radius[index] * sample as f32 * 0.23);
                }
            }
            values.sort_unstable_by(f32::total_cmp);
            values.dedup();
            values
        };
        let xs = axis(0);
        let zs = axis(1);
        let start = self.vertices.len() as u32;
        for &z in &zs {
            for &x in &xs {
                let point = Vec2::new(x, z);
                let depth = lakes.iter().fold(ground_height(point), |height, lake| {
                    lake.basin(point, height)
                });
                self.vertices.push([x, depth, z]);
                let brightness = depth * 0.0028;
                let pigment = color([
                    0.31 + brightness,
                    0.36 + brightness,
                    0.28 + brightness * 0.7,
                ])
                .to_linear();
                self.colors
                    .push([pigment.red, pigment.green, pigment.blue, 1.]);
            }
        }
        let columns = xs.len() as u32;
        for z in 0..(zs.len() - 1) as u32 {
            for x in 0..columns - 1 {
                let a = start + z * columns + x;
                let b = a + 1;
                let c = b + columns;
                let d = a + columns;
                self.indices.extend([a, b, c, a, c, d]);
            }
        }
    }

    fn lake(&mut self, lake: Lake) {
        const POINTS: usize = 36;
        let edge = |index: usize, ring: f32| {
            let angle = index as f32 / POINTS as f32 * std::f32::consts::TAU;
            let point = lake.center
                + Vec2::new(angle.cos(), angle.sin()) * lake.radius * lake.shoreline(angle) * ring;
            [point.x, lake.level, point.y]
        };
        let center = [lake.center.x, lake.level, lake.center.y];
        for index in 0..POINTS {
            let near = edge(index, 0.80);
            let next = edge(index + 1, 0.80);
            self.face(&[center, near, next], [0.24, 0.47, 0.52]);
            self.face(
                &[near, edge(index, 1.), edge(index + 1, 1.), next],
                [0.37, 0.56, 0.57],
            );
        }
    }

    fn building(&mut self, lot: &Lot) {
        let half = lot.size / 2.;
        let [x, y, z] = lot.center.to_array();
        let [left, right] = [x - half.x, x + half.x];
        let [front, back] = [z - half.z, z + half.z];
        let [low, top] = [y - half.y, y + half.y];
        let wall = top - half.y * 0.53;
        for (corners, shade) in [
            (
                [
                    [left, low, front],
                    [right, low, front],
                    [right, wall, front],
                    [left, wall, front],
                ],
                0.85,
            ),
            (
                [
                    [right, low, front],
                    [right, low, back],
                    [right, wall, back],
                    [right, wall, front],
                ],
                0.77,
            ),
            (
                [
                    [right, low, back],
                    [left, low, back],
                    [left, wall, back],
                    [right, wall, back],
                ],
                0.90,
            ),
            (
                [
                    [left, low, back],
                    [left, low, front],
                    [left, wall, front],
                    [left, wall, back],
                ],
                0.80,
            ),
        ] {
            self.face(&corners, [0.87, 0.78, 0.64].map(|v| v * shade));
        }
        self.face(
            &[
                [left, wall, front],
                [left, wall, back],
                [x, top, back],
                [x, top, front],
            ],
            lot.color,
        );
        self.face(
            &[
                [x, top, front],
                [x, top, back],
                [right, wall, back],
                [right, wall, front],
            ],
            lot.color.map(|v| v * 0.79),
        );
        self.face(
            &[[left, wall, front], [x, top, front], [right, wall, front]],
            [0.67, 0.59, 0.48],
        );
        self.face(
            &[[left, wall, back], [x, top, back], [right, wall, back]],
            [0.71, 0.61, 0.48],
        );
    }

    fn face(&mut self, points: &[[f32; 3]], tint: [f32; 3]) {
        let index = self.vertices.len() as u32;
        self.vertices.extend_from_slice(points);
        let linear = color(tint).to_linear();
        self.colors.extend(std::iter::repeat_n(
            [linear.red, linear.green, linear.blue, 1.],
            points.len(),
        ));
        for vertex in 1..(points.len() as u32 - 1) {
            self.indices
                .extend([index, index + vertex, index + vertex + 1]);
        }
    }

    fn terrace(&mut self, terrace: &Terrace) {
        let rim: [Vec2; 10] = std::array::from_fn(|index| {
            let angle = index as f32 * std::f32::consts::TAU / 10.;
            let bevel = 0.89 + 0.10 * (angle * 3. + terrace.phase).sin();
            Vec2::new(angle.cos(), angle.sin()) * bevel
        });
        let edge = |slot: usize, width: f32, height: f32| {
            let p = terrace.center + rim[slot % rim.len()] * terrace.radius * width;
            [p.x, height, p.y]
        };
        for slot in 0..rim.len() {
            let var = (slot as f32 * 1.9 + terrace.phase).sin() * 0.035;
            self.face(
                &[
                    [terrace.center.x, terrace.height, terrace.center.y],
                    edge(slot, 0.91, terrace.height),
                    edge(slot + 1, 0.91, terrace.height),
                ],
                [0.56 + var, 0.56 + var, 0.41],
            );
            self.face(
                &[
                    edge(slot, 0.91, terrace.height),
                    edge(slot + 1, 0.91, terrace.height),
                    edge(slot + 1, 1.04, -0.8),
                    edge(slot, 1.04, -0.8),
                ],
                [0.45 + var, 0.47 + var, 0.41],
            );
        }
    }

    fn finish(self) -> Mesh {
        let normals = vec![[0., 1., 0.]; self.vertices.len()];
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.vertices)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valleys_separate_the_ridges() {
        let mut catalog = Catalog::default();
        catalog.folders = ["a", "b", "c"]
            .into_iter()
            .map(|path| CatalogFolder {
                path: path.into(),
                files: vec![0],
            })
            .collect();
        let terrain = Atlas::terrain(&catalog);
        assert_eq!(terrain.lakes.len(), 2);
        let mut ridges = terrain.terraces;
        ridges.sort_by(|a, b| a.center.x.total_cmp(&b.center.x));
        for (pair, lake) in ridges.windows(2).zip(terrain.lakes) {
            let left = pair[0].center.x + pair[0].radius.x;
            let right = pair[1].center.x - pair[1].radius.x;
            assert!(right - left >= 24.);
            assert!(lake.center.x - lake.radius.x * 1.17 > left);
            assert!(lake.center.x + lake.radius.x * 1.17 < right);
            assert!(lake.basin(lake.center, ground_height(lake.center)) < lake.level);
        }
    }

    #[test]
    fn a_large_map_keeps_every_lot_as_a_solid_pickable_building() {
        let blob = repo_world::Blob {
            id: "demo".into(),
            size: 1024,
            mode: "100644".into(),
        };
        let repo = Repository {
            root: std::path::PathBuf::from("test"),
            revision: "abc".into(),
            baseline: None,
            files: (0..5029)
                .map(|i| repo_world::File {
                    path: format!("src/file-{i}.rs"),
                    before: None,
                    after: Some(blob.clone()),
                    change: Change::Unchanged,
                    position: [0.; 2],
                })
                .collect(),
            districts: vec![repo_world::District {
                name: "src".into(),
                position: [0.; 2],
                size: [2200.; 2],
            }],
        };
        let mut catalog = Catalog::default();
        catalog.index(&repo);
        let atlas =
            Atlas::new(&repo, &catalog, &Atlas::terrain(&catalog), None).expect("large archive");
        assert_eq!(atlas.len(), repo.files.len());
        let first = atlas.lots.first().unwrap();
        let last = atlas.lots.last().unwrap();
        assert_eq!(
            atlas.hit(Ray3d::new(first.center + Vec3::Y * 50., Dir3::NEG_Y)),
            Some(0)
        );
        assert_eq!(
            atlas.hit(Ray3d::new(last.center + Vec3::Y * 50., Dir3::NEG_Y)),
            Some(5028)
        );
        assert_eq!(atlas.lakes.len(), 1);
        assert!(atlas.mesh().count_vertices() >= atlas.len() * 30 + 61 * 61);
    }
}
