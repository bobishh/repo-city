//! A review balcony over the selected address: solid current, translucent parent.
use super::*;
#[derive(Component)]
pub struct HunkBlock {
    pub index: usize,
    pub hit_size: Vec3,
}
const HUNKS_PER_PAGE: usize = 8;

fn hunk_page(selected: usize, total: usize) -> std::ops::Range<usize> {
    let first = selected.min(total.saturating_sub(1)) / HUNKS_PER_PAGE * HUNKS_PER_PAGE;
    first..(first + HUNKS_PER_PAGE).min(total)
}
pub(super) fn base(file: &repo_world::File) -> f32 {
    listing::block_size(file).y + 4.
}

pub fn focus(camera: &mut Transform, repo: &Repository, explorer: &Explorer) {
    let [x, z] = repo.files[explorer.selected].position;
    let base = base(&repo.files[explorer.selected]);
    *camera = Transform::from_xyz(x, base + 8., z + 37.)
        .looking_at(Vec3::new(x, base + 7., z + 3.), Vec3::Y);
}
pub(super) fn reading_rows(
    file: &repo_world::File,
    explorer: &Explorer,
    camera: &Transform,
) -> Option<(bool, std::ops::Range<usize>)> {
    let [x, z] = file.position;
    [true, false].into_iter().find_map(|before| {
        let pair_x = x + if before { -7.4 } else { 7.4 };
        let center = Vec3::new(pair_x, base(file) + 6.2, z + 4.);
        reading::panel_in_focus(camera, center, Vec2::new(13.6, 7.2), 24.)
            .then_some((before, explorer.page * 23..explorer.page * 23 + 23))
    })
}

pub fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    mut camera: Query<&mut Transform, PlayerCamera>,
    mut navigation: ResMut<city_navigation::Navigation>,
    picker: Res<file_picker::FilePicker>,
) {
    if picker.is_open() {
        return;
    }
    if navigation.is_flying() || (navigation.guides_camera() && !navigation.reviewing()) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyG) && explorer.comparison_mode > 0 {
        explorer.comparison_mode = if explorer.comparison_mode == 2 { 1 } else { 2 };
        explorer.anchored = true;
        explorer.rebuild = true;
        explorer.page = explorer
            .comparison
            .as_ref()
            .and_then(|c| c.hunks.get(explorer.hunk))
            .map_or(0, |h| h.row / 23);
        if let Ok(mut camera) = camera.single_mut() {
            focus(&mut camera, &repo.0, &explorer);
        }
        explorer.notice =
            "G: ghost / two buildings. [ ] jumps between real Git hunks; pages stay aligned."
                .into();
    }
    if keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::BracketLeft) {
        if let Some(diff) = &explorer.comparison {
            let count = diff.hunks.len();
            if count > 0 {
                let hunk = if keys.just_pressed(KeyCode::BracketRight) {
                    (explorer.hunk + 1) % count
                } else {
                    (explorer.hunk + count - 1) % count
                };
                let page = diff.hunks[hunk].row / 23;
                let change = &diff.hunks[hunk];
                navigation.select_hunk(
                    explorer.selected,
                    hunk,
                    change.row,
                    change.before.len().max(change.after.len()),
                );
                explorer.hunk = hunk;
                explorer.page = page;
                explorer.rebuild = true;
                if explorer.comparison_mode == 0 {
                    explorer.comparison_mode = 1;
                }
                explorer.anchored = true;
                if let Ok(mut camera) = camera.single_mut() {
                    focus(&mut camera, &repo.0, &explorer);
                }
            }
        }
    }
}
fn panel(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    image: Image,
    position: Vec3,
    size: Vec2,
) -> Entity {
    commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(size.x, size.y))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image)),
                unlit: true,
                cull_mode: None,
                ..default()
            })),
            Transform::from_translation(position),
            Detail,
        ))
        .id()
}
pub fn draw(
    commands: &mut Commands,
    repo: &Repository,
    set: &WorldSet,
    explorer: &Explorer,
    palette: &Palette,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    if explorer.comparison_mode == 0 {
        return;
    }
    let Some(diff) = &explorer.comparison else {
        return;
    };
    let file = &repo.files[explorer.selected];
    let [x, z] = file.position;
    let base = base(file);
    commands.spawn((
        Mesh3d(palette.unit.clone()),
        MeshMaterial3d(palette.plate.clone()),
        Transform::from_xyz(x, base - 0.25, z + 3.).with_scale(Vec3::new(33., 0.5, 19.)),
        RigidBody::Static,
        Collider::cuboid(1., 1., 1.),
        Detail,
    ));
    for before in [true, false] {
        let lines = if before { &diff.before } else { &diff.after };
        let exists = if before {
            file.before.is_some()
        } else {
            file.after.is_some()
        };
        let height = if exists {
            (3. + (lines.len().max(1) as f32).ln_1p() * 1.4).min(18.)
        } else {
            0.15
        };
        let pair_x = x + if before { -7.4 } else { 7.4 };
        let body_x = if explorer.comparison_mode == 2 {
            x + if before { -0.6 } else { 0.6 }
        } else {
            pair_x
        };
        let body_z = z + if explorer.comparison_mode == 2 && before {
            -1.0
        } else {
            0.
        };
        let tint = if before { set.removed } else { set.added };
        let shell = materials.add(StandardMaterial {
            base_color: Color::srgba(tint[0], tint[1], tint[2], if before { 0.16 } else { 0.88 }),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.6,
            cull_mode: None,
            ..default()
        });
        let building = commands
            .spawn((
                Mesh3d(palette.unit.clone()),
                MeshMaterial3d(shell),
                Transform::from_xyz(body_x, base + height / 2., body_z)
                    .with_scale(Vec3::new(12., height, 6.)),
                Detail,
            ))
            .id();
        if !before && exists {
            commands
                .entity(building)
                .insert((RigidBody::Static, Collider::cuboid(1., 1., 1.)));
        }
        // Ghost edges remain legible even where transparent faces overlap.
        let edge = materials.add(StandardMaterial {
            base_color: color(tint),
            unlit: true,
            ..default()
        });
        for dx in [-6., 6.] {
            for dz in [-3., 3.] {
                commands.spawn((
                    Mesh3d(palette.unit.clone()),
                    MeshMaterial3d(edge.clone()),
                    Transform::from_xyz(body_x + dx, base + height / 2., body_z + dz)
                        .with_scale(Vec3::new(0.045, height, 0.045)),
                    Detail,
                ));
            }
        }
        for y in [base, base + height] {
            for dz in [-3., 3.] {
                commands.spawn((
                    Mesh3d(palette.unit.clone()),
                    MeshMaterial3d(edge.clone()),
                    Transform::from_xyz(body_x, y, body_z + dz)
                        .with_scale(Vec3::new(12., 0.045, 0.045)),
                    Detail,
                ));
            }
        }
        let revision = if before {
            repo.baseline.as_deref().map_or("empty", |s| &s[..8])
        } else if explorer.dirty {
            "DRAFT"
        } else {
            &repo.revision[..8]
        };
        let title = format!(
            "{} {} / {} lines{}",
            if before { "BEFORE" } else { "AFTER" },
            revision,
            lines.len(),
            if exists { "" } else { " / FILE ABSENT" }
        );
        let rows = diff.page(before, explorer.page * 23, 23);
        panel(
            commands,
            meshes,
            materials,
            images,
            code_texture::source(&title, &rows, tint, 1280, 768, explorer.column),
            Vec3::new(pair_x, base + 6.2, z + 4.),
            Vec2::new(13.6, 8.16),
        );
    }
    let page = hunk_page(explorer.hunk, diff.hunks.len());
    for (i, hunk) in diff
        .hunks
        .iter()
        .enumerate()
        .skip(page.start)
        .take(page.len())
    {
        let hx = x - 12.6 + (i % 8) as f32 * 3.6;
        let height = 0.4 + (hunk.before.len().max(hunk.after.len()) as f32).ln_1p() * 0.23;
        let material = if i == explorer.hunk {
            palette.edge.clone()
        } else {
            materials.add(StandardMaterial {
                base_color: color(if hunk.before.is_empty() {
                    set.added
                } else if hunk.after.is_empty() {
                    set.removed
                } else {
                    set.modified
                }),
                unlit: true,
                ..default()
            })
        };
        commands.spawn((
            Mesh3d(palette.unit.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(hx, base + height / 2., z + 9.)
                .with_scale(Vec3::new(3., height, 1.3)),
            HunkBlock {
                index: i,
                hit_size: Vec3::new(3., height, 1.3),
            },
            Detail,
        ));
        let title = format!("#{}  -{} +{}", i + 1, hunk.before.len(), hunk.after.len());
        let label = panel(
            commands,
            meshes,
            materials,
            images,
            code_texture::render(&title, &[], set.ink, 768, 64),
            Vec3::new(hx, base + height + 0.35, z + 9.7),
            Vec2::new(3.4, 0.35),
        );
        commands.entity(label).insert(HunkBlock {
            index: i,
            hit_size: Vec3::new(3.4, 0.35, 0.15),
        });
    }
    for (target, title, offset) in [
        (
            page.start.checked_sub(HUNKS_PER_PAGE),
            "Previous hunks",
            -7.4,
        ),
        (
            (page.end < diff.hunks.len()).then_some(page.end),
            "Next hunks",
            7.4,
        ),
    ] {
        let Some(target) = target else { continue };
        commands.spawn((
            Mesh3d(palette.unit.clone()),
            MeshMaterial3d(palette.edge.clone()),
            Transform::from_xyz(x + offset, base + 0.4, z + 12.)
                .with_scale(Vec3::new(6., 0.8, 1.3)),
            HunkBlock {
                index: target,
                hit_size: Vec3::new(6., 0.8, 1.3),
            },
            Detail,
        ));
        let label = panel(
            commands,
            meshes,
            materials,
            images,
            code_texture::render(title, &[], set.ink, 768, 64),
            Vec3::new(x + offset, base + 1.15, z + 12.7),
            Vec2::new(6., 0.5),
        );
        commands.entity(label).insert(HunkBlock {
            index: target,
            hit_size: Vec3::new(6., 0.5, 0.15),
        });
    }
    let summary = format!(
        "{} | hunks {}–{} of {} | G ghost/pair | [ ] hunk | C review all changes{}",
        file.path,
        if page.is_empty() { 0 } else { page.start + 1 },
        page.end,
        diff.hunks.len(),
        if diff.final_newline_changed {
            " | final newline changed"
        } else {
            ""
        }
    );
    panel(
        commands,
        meshes,
        materials,
        images,
        code_texture::render(&summary, &[], set.ink, 1536, 64),
        Vec3::new(x, base + 12., z + 4.1),
        Vec2::new(28., 1.),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eleven_hunks_have_a_reachable_second_page_and_a_return_to_the_first() {
        let first = hunk_page(7, 11);
        assert_eq!(first, 0..8);
        let second = hunk_page(first.end, 11);
        assert_eq!(second, 8..11);
        assert_eq!(hunk_page(second.start - HUNKS_PER_PAGE, 11), first);
        assert_eq!(hunk_page(0, 0), 0..0);
    }
}
