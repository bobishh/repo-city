//! A review balcony over the selected address: solid current, translucent parent.
use super::*;
#[derive(Component)]
pub struct HunkBlock(pub usize);
pub fn focus(camera: &mut Transform, repo: &Repository, explorer: &Explorer) {
    let [x, z] = repo.files[explorer.selected].position;
    *camera = Transform::from_xyz(x, 32., z + 37.).looking_at(Vec3::new(x, 31., z + 3.), Vec3::Y);
}
pub fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
    navigation: Res<city_navigation::Navigation>,
    picker: Res<file_picker::FilePicker>,
) {
    if picker.is_open() {
        return;
    }
    if navigation.guides_camera() {
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
) {
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(size.x, size.y))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(position),
        Detail,
    ));
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
    let base = 24.;
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
            commands.entity(building).insert((
                RigidBody::Static,
                Collider::cuboid(1., 1., 1.),
                physics::BuildingCollider(explorer.selected, true),
            ));
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
    let first = (explorer.hunk / 8) * 8;
    for (i, hunk) in diff.hunks.iter().enumerate().skip(first).take(8) {
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
            RigidBody::Static,
            Collider::cuboid(1., 1., 1.),
            HunkBlock(i),
            Detail,
        ));
        let title = format!("#{}  -{} +{}", i + 1, hunk.before.len(), hunk.after.len());
        panel(
            commands,
            meshes,
            materials,
            images,
            code_texture::render(&title, &[], set.ink, 768, 64),
            Vec3::new(hx, base + height + 0.35, z + 9.7),
            Vec2::new(3.4, 0.35),
        );
    }
    let summary = format!(
        "{} | {} hunks | G ghost/pair | [ ] hunk{}",
        file.path,
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
