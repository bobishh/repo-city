//! Physical file blocks and their signs share the listing's dimensions.
use super::*;

pub(super) const BEAM_HEIGHT: f32 = 14.;

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
        Transform::from_translation(position.with_y(position.y.max(1.))).looking_at(target, up)
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
                .then_some(distance)
        })
        .min_by(f32::total_cmp)
}

#[derive(Clone, Copy)]
pub(super) enum Part {
    Body,
    Stripe(f32),
    Label,
    Beam,
    Mirror,
}

#[derive(Component)]
pub(super) struct Attachment(pub usize, pub Part);

pub(super) fn transform(file: &repo_world::File, size: Vec3, part: Part) -> Transform {
    let [x, z] = file.position;
    match part {
        Part::Body => Transform::from_xyz(x, size.y / 2., z).with_scale(size),
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

pub(super) fn sync(
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    mut last: Local<Option<(usize, Vec3)>>,
    mut parts: Query<(&Attachment, &mut Transform)>,
) {
    let index = explorer.selected;
    let Some(file) = repo.0.files.get(index) else {
        return;
    };
    let size = explorer.facade_size(file, index);
    if !repo.is_changed() && *last == Some((index, size)) {
        return;
    }
    *last = Some((index, size));
    for (attachment, mut pose) in &mut parts {
        if attachment.0 == index {
            pose.set_if_neq(transform(file, size, attachment.1));
        }
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
    fn mirror_covers_the_rear_face_from_ground_to_roof() {
        let file = repo_world::File {
            path: "demo.rs".into(),
            before: None,
            after: None,
            change: Change::Unchanged,
            position: [70., 28.],
        };
        for count in [23, 724, 1614] {
            let size = listing::block_size(&file, Some(count));
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
