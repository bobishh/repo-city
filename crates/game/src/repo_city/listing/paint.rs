//! Paint follows the line mapping shared with the source and diff walls.
use super::*;

#[derive(Component)]
pub(crate) struct ReticlePart(Option<usize>);

const RING_TICKS: usize = 40;

pub(crate) fn spawn_reticle(commands: &mut Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50.),
                top: percent(50.),
                width: px(40.),
                height: px(40.),
                margin: UiRect {
                    left: px(-20.),
                    top: px(-20.),
                    ..default()
                },
                ..default()
            },
            bevy::picking::Pickable::IGNORE,
        ))
        .with_children(|root| {
            for (width, height) in [(18., 2.), (2., 18.)] {
                root.spawn((
                    ReticlePart(None),
                    BackgroundColor(Color::WHITE),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(20. - width / 2.),
                        top: px(20. - height / 2.),
                        width: px(width),
                        height: px(height),
                        ..default()
                    },
                    bevy::picking::Pickable::IGNORE,
                ));
            }
            for tick in 0..RING_TICKS {
                let angle = tick as f32 * std::f32::consts::TAU / RING_TICKS as f32;
                root.spawn((
                    ReticlePart(Some(tick)),
                    BackgroundColor(Color::WHITE),
                    Node {
                        position_type: PositionType::Absolute,
                        display: Display::None,
                        left: px(19. - angle.sin() * 17.),
                        top: px(18. - angle.cos() * 17.),
                        width: px(2.),
                        height: px(4.),
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::radians(-angle)),
                    bevy::picking::Pickable::IGNORE,
                ));
            }
        });
}

pub(super) fn pigments(
    rows: Range<usize>,
    source_line: impl Fn(usize) -> Option<(bool, usize)>,
    marks: &[(&review_notes::Anchor, Vec4)],
) -> [Vec4; TILE_ROWS] {
    let mut paint = [Vec4::ZERO; TILE_ROWS];
    for (slot, index) in rows.enumerate() {
        let Some((before, line)) = source_line(index) else {
            continue;
        };
        if let Some((_, color)) = marks.iter().rev().find(|(anchor, _)| {
            anchor.before == before && (anchor.start..=anchor.end).contains(&line)
        }) {
            paint[slot] = *color;
        }
    }
    paint
}

// Paint uses the exact same ray, wall geometry, and line mapping as the reader.
pub(crate) fn draw(
    mut gizmos: Gizmos,
    time: Res<Time>,
    repo: Res<Repo>,
    focus: Res<file_facade::ChangesOnly>,
    mut explorer: ResMut<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    mut review: ResMut<review_notes::Review>,
    listing: Res<Listing>,
    nearest: Res<Nearest>,
    camera: Query<(&Transform, &Pilot), PlayerCamera>,
    mut files_by_path: Local<HashMap<String, usize>>,
    mut hud: Query<(&mut Text, &mut TextColor), With<review_notes::ToolHud>>,
    mut reticle: Query<(&ReticlePart, &mut BackgroundColor, &mut Node)>,
) {
    use review_notes::Anchor;
    let Ok((camera, pilot)) = camera.single() else {
        return;
    };
    let Ok((mut text, mut ink)) = hud.single_mut() else {
        return;
    };
    let tool_color = review
        .paint()
        .map_or(Color::srgb_u8(82, 211, 255), |paint| {
            let [r, g, b] = paint.color;
            Color::srgb_u8(r, g, b)
        });
    let remaining = listing.wash_fraction_remaining();
    for (part, mut color, mut node) in &mut reticle {
        let shown = pilot.captured
            && !review.blocked
            && match part.0 {
                None => true,
                Some(tick) => {
                    remaining.is_some_and(|fraction| (tick as f32) < fraction * RING_TICKS as f32)
                }
            };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        color.set_if_neq(BackgroundColor(tool_color));
    }
    let visible = Listing::visible(&explorer, &navigation) && !listing.turning();
    let Some(file) = repo.0.files.get(explorer.selected) else {
        return;
    };
    let size = super::block_size(file);
    let sides = [
        (Face::Source, explorer.source_layout.len()),
        (Face::Diff, listing.diff_lines.len()),
    ];
    if repo.is_changed() || files_by_path.is_empty() {
        *files_by_path = repo
            .0
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.path.clone(), index))
            .collect();
    }
    let mut marked = HashSet::new();
    let outstanding = review.notes.iter().filter(|note| note.attention);
    let other_notes = review.notes.iter().filter(|note| !note.attention);
    for note in outstanding
        .chain(other_notes)
        .filter(|note| review.shows_paint(note, &repo.0))
    {
        let Some(&index) = files_by_path.get(&note.anchor.path) else {
            continue;
        };
        if !marked.insert(index) {
            continue;
        }
        let file = &repo.0.files[index];
        if !focus.includes(file) {
            continue;
        }
        let [x, z] = file.position;
        let bound = super::block_size(file);
        let roof = Vec3::new(x, bound.y + 0.6, z);
        if navigation.overview() {
            if roof.xz().distance_squared(camera.translation.xz()) <= 640. * 640. {
                let gold = Color::srgb_u8(255, 199, 79);
                let pin = roof + Vec3::Y * 12.;
                let radius = (bound.x * 0.35).clamp(3.5, 6.);
                gizmos.line(roof, pin, gold);
                gizmos.linestrip(
                    [
                        pin + Vec3::X * radius,
                        pin + Vec3::Z * radius,
                        pin - Vec3::X * radius,
                        pin - Vec3::Z * radius,
                        pin + Vec3::X * radius,
                    ],
                    gold,
                );
                gizmos.linestrip([pin + Vec3::X * radius, pin, pin - Vec3::X * radius], gold);
                gizmos.linestrip([pin + Vec3::Z * radius, pin, pin - Vec3::Z * radius], gold);
            }
            continue;
        }
        if roof.distance_squared(camera.translation) > 140. * 140. {
            continue;
        }
        let [r, g, b] = note.ink();
        let tint = Color::srgb_u8(r, g, b).with_alpha(if note.resolved == Some(true) {
            0.44
        } else {
            0.92
        });
        let tip = (bound.x * 0.18).clamp(0.6, 2.4);
        for axis in [Vec3::X, Vec3::Z] {
            let span = axis * tip;
            let peak = Vec3::Y * (tip * 0.65);
            gizmos.linestrip(
                [
                    roof - span,
                    roof + peak,
                    roof + span,
                    roof - peak,
                    roof - span,
                ],
                tint,
            );
        }
    }
    let mut target = None;
    if visible && !explorer.dirty {
        for (face, count) in sides {
            if count == 0 {
                continue;
            }
            let side = WallSide::new(file, count, face, size);
            if nearest
                .file
                .is_none_or(|aim| aim.index == explorer.selected)
                && let Some(hit) = listing.readable_hit(&explorer, &side, camera, count)
            {
                let row = side.wall.row(hit.y, count);
                let point = match face {
                    Face::Source => {
                        Some((explorer.before, explorer.source_layout.rows[row].number))
                    }
                    Face::Diff => listing.diff_lines[row]
                        .0
                        .as_ref()
                        .map(|line| (listing.diff_lines[row].1 == Mark::Removed, line.number)),
                    _ => None,
                };
                if let Some((before, line)) = point {
                    target = Some((
                        Anchor {
                            path: file.path.clone(),
                            before,
                            start: line,
                            end: line,
                        },
                        face,
                        hit,
                        side.wall.row_height,
                    ));
                }
            }

            let mark_offset = side.top - Vec3::Y.cross(side.normal()) * (side.width * 0.475)
                + side.normal() * 0.05;
            let mut mark = |anchor: &Anchor, tint: Color| {
                let rows = match face {
                    Face::Source if anchor.before == explorer.before => Some((
                        explorer
                            .source_layout
                            .visual_rows(anchor.start.saturating_sub(1))
                            .start,
                        explorer
                            .source_layout
                            .visual_rows(anchor.end.saturating_sub(1))
                            .end,
                    )),
                    Face::Diff => {
                        let matches = |(line, tag): &(Option<syntax::Line>, Mark)| {
                            (*tag == Mark::Removed) == anchor.before
                                && line.as_ref().is_some_and(|line| {
                                    (anchor.start..=anchor.end).contains(&line.number)
                                })
                        };
                        listing
                            .diff_lines
                            .iter()
                            .position(matches)
                            .zip(listing.diff_lines.iter().rposition(matches))
                            .map(|(start, end)| (start, end + 1))
                    }
                    _ => None,
                };
                if let Some((start, end)) = rows.filter(|&(start, end)| start < end && end <= count)
                {
                    let top = mark_offset.with_y(side.wall.y(start as f32));
                    let bottom = mark_offset.with_y(side.wall.y(end as f32));
                    let tick = Vec3::Y.cross(side.normal()) * (side.width * 0.052);
                    let inside = tick * 0.14;
                    gizmos.linestrip([top + tick, top, bottom, bottom + tick], tint);
                    gizmos.line(top + inside, bottom + inside, tint);
                }
            };
            for note in review
                .notes
                .iter()
                .filter(|note| note.anchor.path == file.path && review.shows_paint(note, &repo.0))
            {
                let [r, g, b] = note.ink();
                let tint = Color::srgb_u8(r, g, b).with_alpha(if note.resolved == Some(true) {
                    0.38
                } else {
                    1.
                });
                mark(&note.anchor, tint);
            }
            if let (Some(preset), Some((_, anchor))) = (review.paint(), &review.drag) {
                let [r, g, b] = preset.color;
                mark(
                    anchor,
                    Color::srgba(r as f32 / 255., g as f32 / 255., b as f32 / 255., 0.6),
                );
            }
        }
    }
    let note = target.as_ref().and_then(|(line, _, _, _)| {
        review
            .notes
            .iter()
            .filter(|note| review.shows_paint(note, &repo.0) && note.anchor.includes(line))
            .max_by_key(|note| note.attention)
    });
    let aimed = note.map(|note| (note.id.clone(), note.preset.label.clone()));
    review.aim_note = aimed.as_ref().map(|(id, _)| id.clone());

    let mut hint = aimed.map_or_else(String::new, |(_, label)| format!("{label} · J opens note"));
    let mut tint = Color::srgb_u8(143, 217, 207);
    if let Some(preset) = review.paint().cloned() {
        let [r, g, b] = preset.color;
        tint = Color::srgb_u8(r, g, b);
        if review.blocked || !pilot.captured || !visible {
            review.drag = None;
            review.phrase = None;
        } else if pilot.firing {
            if let Some((point, _, hit, row_height)) = &target {
                if pilot.shot {
                    review.drag = Some((point.clone(), point.clone()));
                    review.phrase = Some((time.elapsed_secs(), preset.spray.clone()));
                } else if let Some((first, range)) = &mut review.drag
                    && let Some(new_range) = first.extend(point)
                {
                    *range = new_range;
                }
                if review.drag.is_some() && review.nozzle >= 0.85 {
                    physics::spray_stream(
                        &mut gizmos,
                        camera,
                        *hit,
                        *row_height,
                        time.elapsed_secs(),
                        Some(preset.color),
                        review.nozzle,
                    );
                }
            }
        } else if let Some((_, anchor)) = review.drag.take() {
            let readable = target.as_ref().is_some_and(|(point, face, _, _)| {
                anchor.includes(point)
                    && (*face == Face::Diff
                        || (anchor.start..=anchor.end).all(|row| {
                            listing
                                .revealed
                                .contains(&(explorer.selected, anchor.before, row))
                        }))
            });
            if readable {
                match review.save_draft(&repo.0, anchor) {
                    Ok(note) => {
                        review.aim_note = Some(note.id);
                        #[cfg(target_arch = "wasm32")]
                        {
                            explorer.notice =
                                "Draft saved. Press J to reopen an aimed note.".into();
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        if !review.auto_publish {
                            review.notify(
                                time.elapsed_secs(),
                                "Draft saved locally · J opens review",
                            );
                        }
                    }
                    Err(error) => explorer.notice = error,
                }
            } else {
                explorer.notice = "Wash these lines first, or mark them on the diff wall.".into();
            }
        }
        if let Some((start, phrase)) = &review.phrase {
            let age = time.elapsed_secs() - start;
            if age < 1.65 {
                let count = (age * 28.) as usize + 1;
                hint = phrase.chars().take(count).collect();
                tint = tint.with_alpha(((1.65 - age) / 0.45).clamp(0., 1.));
            } else {
                review.phrase = None;
            }
        }
    }
    text.set_if_neq(Text::new(hint));
    ink.set_if_neq(TextColor(tint));
}
