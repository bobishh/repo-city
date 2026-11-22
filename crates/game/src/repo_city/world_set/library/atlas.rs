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
        let mut groups = BTreeMap::<&str, Vec<usize>>::new();
        for (index, folder) in catalog.folders.iter().enumerate() {
            let branch = folder
                .path
                .split('/')
                .next()
                .filter(|part| !part.is_empty())
                .unwrap_or("/");
            groups.entry(branch).or_default().push(index);
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
        let mut districts = Vec::with_capacity(groups.len());
        for (_, mut group) in groups {
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
            let half = (maximum - minimum) * 0.5;
            districts.push((group, (minimum + maximum) * 0.5, half));
        }
        districts.sort_by(|a, b| b.2.length_squared().total_cmp(&a.2.length_squared()));
        let mut occupied = Vec::<(Vec2, Vec2)>::new();
        for (files, origin, half) in districts {
            let padding = 16. + half.min_element() * 0.12;
            let extent = half + Vec2::splat(padding);
            let center = (0..)
                .find_map(|attempt| {
                    let angle = attempt as f32 * TURN;
                    let radius = (attempt as f32).sqrt() * 36.;
                    let candidate = Vec2::new(angle.cos() * 1.15, angle.sin()) * radius;
                    occupied
                        .iter()
                        .all(|(placed, bounds)| {
                            ((candidate - *placed).abs() - extent - *bounds).max_element() >= 0.
                        })
                        .then_some(candidate)
                })
                .expect("unbounded atlas");
            occupied.push((center, extent));
            for index in files {
                let terrace = &mut positions[index];
                terrace.center += center - origin;
                terrace.height =
                    (ground_height(terrace.center) + terrace.radius.max_element() * 0.17).min(39.)
                        + 7.;
            }
        }
        let mut lakes = Vec::<Lake>::new();
        // Place water in the gaps near the largest districts, clear of every terrace.
        for (index, &(center, extent)) in occupied.iter().take(8).enumerate() {
            let radius = (extent.min_element() * 0.29).clamp(10., 32.);
            for step in 0..12 {
                let angle = (index * 3 + step) as f32 * TURN;
                let axis = Vec2::new(angle.cos(), angle.sin());
                let shore = center + axis * (extent + Vec2::splat(radius * 0.72));
                let room = radius * 1.42;
                let clear = positions.iter().all(|terrace| {
                    shore.distance(terrace.center) > terrace.radius.max_element() + room
                }) && lakes.iter().all(|lake| {
                    shore.distance(lake.center) > lake.radius.max_element() + room * 2.
                });
                if clear {
                    lakes.push(Lake::new(shore, Vec2::new(radius * 0.67, radius), angle));
                    break;
                }
            }
            if lakes.len() >= 3 {
                break;
            }
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
                let level = (1.6 + dimensions.y.ln_1p() * 0.42).min(4.7);
                let size = Vec3::new(
                    dimensions.x.clamp(7., spacing),
                    level,
                    dimensions.z.clamp(7., spacing) * 0.55,
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
            mesh.scroll(lot);
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

    fn scroll(&mut self, lot: &Lot) {
        let edge = |slot: usize, side: f32| {
            let angle = slot as f32 * std::f32::consts::TAU / 6.;
            (lot.center
                + Vec3::new(
                    side * lot.size.x * 0.5,
                    angle.cos() * lot.size.y * 0.5,
                    angle.sin() * lot.size.z * 0.5,
                ))
            .to_array()
        };
        for slot in 0..6 {
            let light = [1.0, 0.93, 0.78, 0.66, 0.75, 0.95][slot];
            self.face(
                &[
                    edge(slot, -1.),
                    edge(slot, 1.),
                    edge(slot + 1, 1.),
                    edge(slot + 1, -1.),
                ],
                lot.color.map(|shade| shade * light),
            );
        }
        for side in [-1., 1.] {
            let rim = std::array::from_fn::<_, 6, _>(|slot| edge(slot, side));
            self.face(&rim, [0.88, 0.74, 0.52]);
        }
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
            if slot != 1 {
                let lip = (terrace.radius.min_element() * 0.12).clamp(1.3, 5.3);
                let floor = terrace.height;
                self.face(
                    &[
                        edge(slot, 0.85, floor + lip),
                        edge(slot + 1, 0.85, floor + lip),
                        edge(slot + 1, 0.91, floor + lip),
                        edge(slot, 0.91, floor + lip),
                    ],
                    [0.83, 0.73, 0.57],
                );
                self.face(
                    &[
                        edge(slot, 0.91, floor),
                        edge(slot + 1, 0.91, floor),
                        edge(slot + 1, 0.91, floor + lip),
                        edge(slot, 0.91, floor + lip),
                    ],
                    [0.68, 0.58, 0.45],
                );
                self.face(
                    &[
                        edge(slot, 0.85, floor + lip),
                        edge(slot + 1, 0.85, floor + lip),
                        edge(slot + 1, 0.85, floor),
                        edge(slot, 0.85, floor),
                    ],
                    [0.72, 0.61, 0.46],
                );
            }
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
        assert!(terrain.lakes.len() >= 2);
        for lake in terrain.lakes {
            assert!(lake.basin(lake.center, ground_height(lake.center)) < lake.level);
            for terrace in &terrain.terraces {
                assert!(
                    lake.center.distance(terrace.center)
                        > terrace.radius.max_element() + lake.radius.max_element()
                );
            }
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
