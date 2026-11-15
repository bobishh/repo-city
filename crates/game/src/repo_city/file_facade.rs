//! Physical file blocks and their signs share the listing's dimensions.
use super::*;

pub(super) const BEAM_HEIGHT: f32 = 14.;

pub(super) fn beam_material([r, g, b]: [f32; 3]) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(r, g, b),
        unlit: true,
        ..default()
    }
}

pub(super) fn approach(eye: &Transform, min: Vec3, max: Vec3) -> Option<f32> {
    let direction = eye.forward().as_vec3();
    if eye.translation.z <= max.z || direction.z >= -0.12 {
        return None;
    }
    let distance = (max.z - eye.translation.z) / direction.z;
    let point = eye.translation + direction * distance;
    (point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y)
        .then_some(distance)
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
        Part::Stripe(side) => Transform::from_xyz(
            x - size.x / 2. + 0.14,
            size.y / 2.,
            z + side * (size.z / 2. + 0.02),
        )
        .with_scale(Vec3::new(0.24, size.y, 0.04)),
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
            .with_scale(Vec3::new(size.x * 0.88, size.y * 0.75, 1.)),
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
    fn proximity_hits_the_front_but_leaves_the_mirror_free() {
        let min = Vec3::new(-4., 0., -2.);
        let max = Vec3::new(4., 8., 2.);
        let front = Transform::from_xyz(0., 3., 7.);
        assert_eq!(approach(&front, min, max), Some(5.));
        let side = front.with_translation(Vec3::new(4.1, 3., 7.));
        assert_eq!(approach(&side, min, max), None);
        let rear = Transform::from_xyz(0., 3., -7.).looking_to(Vec3::Z, Vec3::Y);
        assert_eq!(approach(&rear, min, max), None);
    }
}
