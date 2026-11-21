use super::*;

pub(super) fn update(
    library: Res<Library>,
    aim: Res<aim::Aim>,
    opened: Res<room::Unrolling>,
    map: Res<map::Map>,
    settings: Res<reading::Settings>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    repo: Res<Repo>,
    mut explorer: ResMut<Explorer>,
    mut listing: ResMut<listing::Listing>,
    mut review: ResMut<review_notes::Review>,
    mut progress: ResMut<progress::Progress>,
    mut heat: Local<Option<(usize, bool)>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let focus = library
        .reader
        .as_ref()
        .zip(repo.0.files.get(explorer.selected));
    let frozen = map.open()
        || review.blocked
        || review.tablet_open
        || !windows.single().is_ok_and(|window| window.focused);
    if frozen || focus.is_none() {
        listing.clear_observation();
        review.drag = None;
        if focus.is_none() {
            *heat = None;
        }
        return;
    }
    if library.input.is_some() {
        review.drag = None;
        return;
    }
    let (reader, file) = focus.expect("visible reader");
    let viewport = reader.page(paper::Rows::new(&explorer, reader.face).len());
    let hit = match aim.0 {
        Some(aim::Target::Line {
            file,
            row,
            before,
            number,
        }) if file == explorer.selected && viewport.contains(&row) => Some((row, before, number)),
        _ => None,
    };
    let key = (explorer.selected, explorer.before);
    if *heat != Some(key) || review.slot != 0 {
        *heat = None;
    }
    let warming = keys.just_pressed(KeyCode::KeyK);
    if warming && review.slot == 0 && reader.face == listing::Face::Source {
        *heat = heat.is_none().then_some(key);
    }
    review.washing = heat.is_some() && reader.face == listing::Face::Source;
    if !opened.stationary() {
        listing.clear_observation();
        return;
    }
    if review.paint().is_some() {
        let point = hit.map(|(_, before, number)| review_notes::Anchor {
            path: file.path.clone(),
            before,
            start: number,
            end: number,
        });
        if let Some(point) = &point {
            review.aim_note = review
                .notes
                .iter()
                .rev()
                .find(|note| review.shows_paint(note, &repo.0) && note.anchor.includes(point))
                .map(|note| note.id.clone());
        }
        if keys.pressed(KeyCode::KeyK) {
            if let Some(point) = point {
                if warming {
                    review.drag = Some((point.clone(), point));
                } else if let Some((first, range)) = &mut review.drag
                    && let Some(extended) = first.extend(&point)
                {
                    *range = extended;
                }
            }
        } else if let Some((_, anchor)) = review.drag.take()
            && point.as_ref().is_some_and(|point| anchor.includes(point))
        {
            let readable = reader.face != listing::Face::Source
                || (anchor.start..=anchor.end).all(|line| {
                    settings.source_is_visible()
                        || listing.has_read_line(explorer.selected, anchor.before, line)
                });
            if readable {
                match review.save_draft(&repo.0, anchor) {
                    Ok(note) => review.aim_note = Some(note.id),
                    Err(error) => explorer.notice = error,
                }
            } else {
                explorer.notice = "Thaw these lines first, or mark them on the diff.".to_owned();
            }
        }
    }

    if (!settings.source_is_visible() && heat.is_none() && reader.face == listing::Face::Source)
        || !settings.enabled
        || reader
            .guidance
            .as_ref()
            .is_some_and(|(_, until)| *until > time.elapsed_secs_f64())
    {
        listing.clear_observation();
        return;
    }
    let rows = paper::Rows::new(&explorer, reader.face);
    let center = hit.map_or(viewport.start + viewport.len() / 2, |(index, _, _)| index);
    let group = if reader.face == listing::Face::Source {
        settings.block(&explorer, center)
    } else {
        settings.rows(center, rows.len())
    };
    let block = group.start.max(viewport.start)..group.end.min(viewport.end);
    let mut seen = HashSet::new();
    let source = block
        .clone()
        .filter_map(|index| rows.row(index))
        .filter_map(|row| {
            if !seen.insert((row.before, row.line.number)) {
                return None;
            }
            let index = row.line.number.saturating_sub(1);
            if reader.face == listing::Face::Source {
                let fragments = explorer.source_layout.visual_rows(index);
                if fragments.start < viewport.start || fragments.end > viewport.end {
                    return None;
                }
                return explorer
                    .highlighted
                    .get(index)
                    .map(|line| (row.before, line.clone()));
            }
            Some((row.before, row.line.clone()))
        });
    for mark in listing.observe_rows(
        &explorer,
        file,
        block.clone(),
        source,
        time.delta_secs(),
        *settings,
    ) {
        progress.mark(mark, true);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn publish(
    time: Res<Time>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    library: Res<Library>,
    listing: Res<listing::Listing>,
    settings: Res<reading::Settings>,
    opening: Res<room::Unrolling>,
    near: Res<Nearest>,
    review: Res<review_notes::Review>,
    tablet: Res<review_tablet::Tablet>,
    map: Res<map::Map>,
    cameras: Query<(&Camera, &Transform), With<room::Camera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut publisher: ResMut<focus_context::Publisher>,
) {
    let (Ok((camera, pose)), Ok(window)) = (cameras.single(), windows.single()) else {
        return;
    };
    if !publisher.due(time.elapsed_secs_f64()) || repo.0.files.is_empty() {
        return;
    }
    let reader = library.reader.as_ref();
    let face = reader.map(|reader| reader.face);
    let visible = reader.filter(|_| {
        !map.open()
            && !review.blocked
            && !review.tablet_open
            && library.input.is_none()
            && opening.stationary()
    });
    let mut printed = Vec::new();
    if let Some(reader) = visible {
        let paper = paper::Rows::new(&explorer, reader.face);
        let pose = GlobalTransform::from(*pose);
        for index in reader.page(paper.len()) {
            let Some(row) = paper.row(index) else {
                continue;
            };
            let Some(columns) = room::visible_columns(camera, &pose, reader.face, index, &opening)
            else {
                continue;
            };
            let text = row
                .line
                .text
                .chars()
                .skip(reader.column)
                .take(columns)
                .collect();
            let guided = paper::guided(reader, row.line.number, time.elapsed_secs_f64());
            printed.push((
                row.before,
                syntax::Line {
                    text,
                    number: row.line.number,
                    spans: vec![],
                },
                paper::readable(
                    reader.face,
                    &row,
                    explorer.selected,
                    index,
                    guided,
                    *settings,
                    &listing,
                ),
            ));
        }
    }
    let lines = visible.map(|reader| {
        (
            reader.face,
            printed
                .iter()
                .map(|(before, line, readable)| listing::VisibleLine {
                    before: *before,
                    line,
                    readable: *readable,
                })
                .collect(),
        )
    });
    publisher.submit(
        &repo.0,
        &explorer,
        pose,
        focus_context::View {
            active: window.focused,
            aimed_path: near
                .file
                .and_then(|aim| repo.0.files.get(aim.index))
                .map(|file| file.path.as_str()),
            wall_face: face,
            visible: lines,
            tablet_page: if map.open() {
                Some("map")
            } else {
                tablet.page_name()
            },
            source_page: reader.map_or(1, |reader| reader.first / SCROLL_ROWS + 1),
        },
    );
}

pub(super) fn share(
    mut scene: ResMut<review_party::Scene>,
    library: Res<Library>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    review: Res<review_notes::Review>,
    stage: Res<room::Unrolling>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    scene.focus = if stage.stationary()
        && !review.blocked
        && !review.tablet_open
        && library.input.is_none()
        && windows.single().is_ok_and(|window| window.focused)
        && let Some(reader) = &library.reader
        && let Some(file) = repo.0.files.get(explorer.selected)
    {
        let rows = paper::Rows::new(&explorer, reader.face);
        let range = rows.line_range(reader.page(rows.len()));
        (*range.start() > 0).then(|| review_party::Focus {
            path: file.path.clone(),
            before: explorer.before,
            face: paper::face_name(reader.face).to_ascii_lowercase(),
            line: *range.start(),
            end_line: *range.end(),
        })
    } else {
        None
    };
}
