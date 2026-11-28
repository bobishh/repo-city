//! Tiled sand ripples and the basalt, coral fans and tube sponges around survey sites.
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology, prelude::*};
use std::f32::consts::TAU;

pub(super) const TILE_SIZE: f32 = 96.;

/// The same mesh is instanced across the seabed. Heights and normals repeat at each seam.
fn sand(x: f32, z: f32) -> (f32, f32) {
    let a = x * TAU / TILE_SIZE;
    let b = z * TAU / TILE_SIZE;
    let dune = (a * 2. + b + 1.5 * b.sin()).cos();
    let ripple = (b * 20. + 2. * (a * 2. + b).sin() + (a - b).cos()).cos();
    (-1.45 + 0.64 * dune + 0.12 * ripple, ripple)
}

#[derive(Default)]
pub(super) struct Geometry {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
}

impl Geometry {
    pub(super) fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, tint: Color) {
        self.shaded_triangle(a, b, c, [tint; 3]);
    }

    fn shaded_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, tints: [Color; 3]) {
        self.positions.extend([a, b, c].map(|v| v.to_array()));
        let normal = (b - a).cross(c - a).normalize_or_zero();
        self.normals.extend([normal.to_array(); 3]);
        self.colors
            .extend(tints.map(|tint| tint.to_linear().to_f32_array()));
    }

    pub(super) fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, tint: Color) {
        self.tri(a, b, c, tint);
        self.tri(a, c, d, tint);
    }

    pub(super) fn tube(&mut self, path: &[(Vec3, f32)], sides: usize, tint: Color) {
        let axis = (path[path.len() - 1].0 - path[0].0).normalize_or_zero();
        let u = axis.any_orthonormal_vector();
        let v = axis.cross(u);
        let offset = |index: usize| {
            let (sin, cos) = (TAU * index as f32 / sides as f32).sin_cos();
            cos * u + sin * v
        };
        for pair in path.windows(2) {
            let [(a, ra), (b, rb)] = [pair[0], pair[1]];
            for i in 0..sides {
                let (left, right) = (offset(i), offset(i + 1));
                self.quad(
                    a + left * ra,
                    a + right * ra,
                    b + right * rb,
                    b + left * rb,
                    tint,
                );
            }
        }
    }

    pub(super) fn finish(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
    }
}

fn noise(mut seed: u32) -> f32 {
    seed ^= seed >> 16;
    seed = seed.wrapping_mul(0x7feb352d);
    seed ^= seed >> 15;
    seed = seed.wrapping_mul(0x846ca68b);
    (seed ^ (seed >> 16)) as f32 / u32::MAX as f32
}

pub(super) fn vent() -> Mesh {
    let mut mesh = Geometry::default();
    for i in 0..7 {
        let angle = i as f32 * 2.39996;
        let spread = if i == 0 { 0. } else { 0.8 + noise(i + 1) * 1.7 };
        let foot = Vec3::new(angle.cos() * spread, -1.8, angle.sin() * spread);
        let height = if i == 0 { 11. } else { 4. + noise(i + 11) * 4. };
        let cap = foot + Vec3::new(0.32 * angle.cos(), height, 0.18 * angle.sin());
        let r = 0.42 + height * 0.055;
        mesh.tube(
            &[
                (foot, r * 1.6),
                (foot.lerp(cap, 0.47), r),
                (foot.lerp(cap, 0.74), r * 0.87),
                (cap, r * 0.56),
            ],
            7,
            Color::srgb(0.27, 0.23, 0.57),
        );
        mesh.tube(
            &[
                (cap - Vec3::Y * 0.32, r * 0.59),
                (cap, r * 0.68),
                (cap + Vec3::Y * 0.13, r * 0.43),
            ],
            7,
            Color::srgb(0.45, 0.96, 0.82),
        );
    }
    mesh.finish()
}

pub(super) const WATER_CELL: f32 = 24.;

pub(super) fn marine_snow() -> Mesh {
    let mut mesh = Geometry::default();
    for i in 0..90 {
        let seed = i * 5 + 1;
        let center = Vec3::new(noise(seed), noise(seed + 1), noise(seed + 2)) * WATER_CELL;
        let radius = 0.007 + noise(seed + 3) * 0.016;
        let p = center + Vec3::Y * radius * 1.8;
        let q = center - Vec3::Y * radius;
        let tint = Color::srgb(0.72, 0.82, 0.77);
        for j in 0..3 {
            let point = |j: usize| {
                let angle = TAU * j as f32 / 3.;
                center + Vec3::new(angle.cos(), 0., angle.sin()) * radius
            };
            mesh.tri(p, point(j + 1), point(j), tint);
            mesh.tri(q, point(j), point(j + 1), tint);
        }
    }
    mesh.finish()
}

pub(super) fn seabed() -> Mesh {
    const ROWS: usize = 80;
    let mut mesh = Geometry::default();
    let vertex = |i, j| {
        let x = i as f32 * TILE_SIZE / ROWS as f32;
        let z = j as f32 * TILE_SIZE / ROWS as f32;
        Vec3::new(x, sand(x, z).0, z)
    };
    for i in 0..ROWS {
        for j in 0..ROWS {
            let a = vertex(i, j);
            let variation = sand(a.x, a.z).1 * 0.025 + noise((i * ROWS + j) as u32) * 0.012;
            let tint = Color::srgb(0.36 + variation, 0.57 + variation, 0.66 + variation);
            mesh.quad(
                a,
                vertex(i, j + 1),
                vertex(i + 1, j + 1),
                vertex(i + 1, j),
                tint,
            );
        }
    }
    // Small clusters of fauna leave open sand between stations and clear sightlines to the code.
    for i in 0..22 {
        let x = 5. + noise(i * 19 + 2) * (TILE_SIZE - 10.);
        let z = 5. + noise(i * 19 + 3) * (TILE_SIZE - 10.);
        let root = Vec3::new(x, sand(x, z).0 - 0.18, z);
        let scale = 0.55 + noise(i * 19 + 5) * 1.35;
        for j in 0..6 {
            let seed = i * 713 + j * 7;
            let base =
                root + Vec3::new(noise(seed + 1) * 3.8 - 1.9, 0., noise(seed + 2) * 3.8 - 1.9);
            let radius = (0.42 + noise(seed + 3) * 0.62) * scale;
            let height = (0.18 + noise(seed + 4) * 0.85) * scale;
            mesh.tube(
                &[
                    (base, radius),
                    (base + Vec3::Y * height, radius * 0.74),
                    (base + Vec3::new(0.23, height * 1.22, -0.06), 0.),
                ],
                6,
                Color::srgb(0.34 + noise(seed) * 0.13, 0.32, 0.58),
            );
        }
        if i % 2 == 0 {
            for j in 0..13 {
                let seed = i * 81 + j * 5;
                let base =
                    root + Vec3::new(noise(seed) * 1.7 - 0.85, 0., noise(seed + 1) * 1.7 - 0.85);
                let height = (0.75 + noise(seed + 2) * 2.) * scale;
                let drift = Vec3::new(0.28 + (j as f32).sin() * 0.17, height, -0.11);
                let radius = (0.08 + noise(seed + 3) * 0.13) * scale;
                let lip = base + drift;
                mesh.tube(
                    &[
                        (base, radius * 0.9),
                        (base + drift * 0.65, radius),
                        (lip, radius * 1.2),
                    ],
                    7,
                    Color::srgb(0.84, 0.34, 0.75),
                );
                mesh.tube(
                    &[
                        (lip, radius * 1.23),
                        (lip + Vec3::Y * radius * 0.6, radius * 0.9),
                    ],
                    7,
                    Color::srgb(1., 0.76, 0.90),
                );
            }
        } else {
            for j in 0..11 {
                let spread = j as f32 / 5. - 1.;
                let tip = root + Vec3::new(2.1 * spread, 3.3 - spread.abs(), 0.42 * spread) * scale;
                let fork = root.lerp(tip, 0.53) + Vec3::Y * 0.16;
                let tint = Color::srgb(0.31, 0.93, 0.72);
                mesh.tube(
                    &[(root, 0.10 * scale), (fork, 0.065 * scale), (tip, 0.012)],
                    4,
                    tint,
                );
                for k in [-1., 1.] {
                    let twig = tip + Vec3::new(k * 0.24, -0.15, k * 0.06) * scale;
                    mesh.tube(&[(fork, 0.045 * scale), (twig, 0.013)], 4, tint);
                }
            }
        }
    }
    mesh.finish()
}

/// Shell-plated station with flat, full-width walls for the projected source.
pub(super) fn station() -> Mesh {
    let mut mesh = Geometry::default();
    let footprint = [
        Vec2::new(-0.5, -0.5),
        Vec2::new(-0.5, 0.5),
        Vec2::new(0.5, 0.5),
        Vec2::new(0.5, -0.5),
    ];
    let ring = |point: Vec2, y, scale| Vec3::new(point.x * scale, y, point.y * scale);
    let shell = Color::srgb(0.21, 0.40, 0.45);
    let steel = Color::srgb(0.18, 0.24, 0.39);
    let pearl = Color::srgb(0.73, 0.60, 0.77);
    for i in 0..footprint.len() {
        let (a, b) = (footprint[i], footprint[(i + 1) % footprint.len()]);
        for (bottom, top, tint) in [
            (-0.50, -0.48, steel),
            (-0.48, -0.46, pearl),
            (-0.46, 0.48, if i == 3 { shell } else { steel }),
            (0.48, 0.50, steel),
        ] {
            mesh.quad(
                ring(a, bottom, 1.),
                ring(b, bottom, 1.),
                ring(b, top, 1.),
                ring(a, top, 1.),
                tint,
            );
        }
        mesh.tri(ring(a, 0.50, 1.), ring(b, 0.50, 1.), Vec3::Y * 0.50, steel);
    }
    mesh.finish()
}

/// Scallop valves are corrugated along their growth ribs. The raised ridges catch
/// the pearl coat at a different angle from the lavender folds between them.
fn scallop(mesh: &mut Geometry, center: Vec3, radius: f32, yaw: f32) {
    const RIBS: usize = 17;
    let root_tint = Color::srgb(0.55, 0.41, 0.89);
    let pearl = Color::srgb(0.99, 0.88, 0.98);
    let rotation = Quat::from_rotation_y(yaw);
    let point = |step: usize, t: f32| {
        let u = step as f32 / (RIBS * 2) as f32;
        let angle = (u - 0.5) * 2.88;
        let crest = if step % 2 == 0 { 0. } else { 1. };
        let lip = 1. - 0.035 * (1. - crest) * t;
        let p = Vec3::new(
            angle.sin() * t * lip,
            angle.cos() * t * lip,
            0.20 * (t * std::f32::consts::PI).sin() + (0.065 * crest - 0.11 * u) * t,
        );
        center + rotation * p * radius
    };
    for j in 0..RIBS * 2 {
        let face = if j % 2 == 0 {
            Color::srgb(0.96, 0.60, 0.87)
        } else {
            Color::srgb(0.65, 0.93, 0.93)
        };
        let (mid_a, mid_b) = (point(j, 0.72), point(j + 1, 0.72));
        let (tip_a, tip_b) = (point(j, 1.), point(j + 1, 1.));
        mesh.shaded_triangle(center, mid_b, mid_a, [root_tint, face, face]);
        mesh.shaded_triangle(mid_a, mid_b, tip_b, [face, face, pearl]);
        mesh.shaded_triangle(mid_a, tip_b, tip_a, [face, pearl, pearl]);
    }
}

fn starfish(mesh: &mut Geometry, center: Vec3, radius: f32, yaw: f32) {
    let palm = center + Vec3::Y * radius * 0.21;
    let direction = |angle: f32| Vec3::new(angle.cos(), 0., angle.sin());
    for i in 0..5 {
        let angle = yaw + TAU * i as f32 / 5.;
        let left = center + direction(angle - TAU / 10.) * radius * 0.36;
        let right = center + direction(angle + TAU / 10.) * radius * 0.36;
        let tip = center + direction(angle + 0.09) * radius;
        let ridge = center + direction(angle) * radius * 0.48 + Vec3::Y * radius * 0.12;
        let salmon = Color::srgb(0.99, 0.46, 0.68);
        let pink = Color::srgb(1., 0.70, 0.83);
        mesh.tri(left, palm, ridge, pink);
        mesh.tri(left, ridge, tip, salmon);
        mesh.tri(ridge, palm, right, salmon);
        mesh.tri(ridge, right, tip, pink);
    }
}

pub(super) fn shell_colony() -> Mesh {
    let mut mesh = Geometry::default();
    let reef = Color::srgb(0.40, 0.38, 0.62);
    let foot = Vec3::new(0., -2., 0.);
    mesh.tube(
        &[
            (foot, 2.3),
            (Vec3::new(0., -0.63, 0.), 2.1),
            (Vec3::new(0., -0.42, 0.), 0.),
        ],
        10,
        reef,
    );
    scallop(&mut mesh, Vec3::new(-0.25, -0.63, -0.4), 3.7, 0.65);
    scallop(&mut mesh, Vec3::new(-1.2, -0.55, 0.9), 1.8, -0.52);
    scallop(&mut mesh, Vec3::new(1.15, -0.55, -0.6), 1.9, 1.24);
    starfish(&mut mesh, Vec3::new(0.55, -0.35, 1.55), 1.12, -0.2);
    mesh.finish()
}
