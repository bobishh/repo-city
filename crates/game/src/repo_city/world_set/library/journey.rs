use super::*;

struct Stop {
    path: String,
    line: usize,
}

pub(super) struct Route {
    stops: Vec<Stop>,
    cursor: usize,
    face: listing::Face,
}

pub(super) fn update(
    mut library: ResMut<Library>,
    mut trace: ResMut<trace_import::Trace>,
    keys: Res<ButtonInput<KeyCode>>,
    explorer: Res<Explorer>,
    repo: Res<Repo>,
    review: Res<review_notes::Review>,
    map: Res<map::Map>,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
) {
    if review.blocked || review.tablet_open || library.input.is_some() || map.consumed {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if picker.blocks_controls() {
        return;
    }
    let selected = repo.0.files.get(explorer.selected);
    if library.reader.is_some()
        && let Some(file) = selected
    {
        library.history.record(&file.path);
    }
    if library.destination.is_some() {
        library.route = None;
        return;
    }
    if let Some(direction) = city_navigation::history_direction(&keys) {
        library.route = None;
        let Some(path) = library.history.step(direction) else {
            return;
        };
        library.destination = destination(&repo.0, path, 1, listing::Face::Source);
        return;
    }
    let mut moved = false;
    if std::mem::take(&mut trace.start) {
        let stops = trace
            .frames()
            .map(|frame| Stop {
                path: frame.path.clone(),
                line: frame.line,
            })
            .collect();
        library.route = Some(Route {
            stops,
            cursor: 0,
            face: listing::Face::Source,
        });
        moved = true;
    } else if keys.just_pressed(KeyCode::KeyC) {
        let stops = repo
            .0
            .files
            .iter()
            .filter(|file| file.change != Change::Unchanged)
            .map(|file| Stop {
                path: file.path.clone(),
                line: 1,
            })
            .collect();
        library.route = Some(Route {
            stops,
            cursor: 0,
            face: listing::Face::Diff,
        });
        moved = true;
    } else if keys.just_pressed(KeyCode::Escape) {
        library.route = None;
    } else if let Some(route) = &mut library.route {
        let previous = route.cursor;
        if keys.just_pressed(KeyCode::KeyW) {
            route.cursor = (route.cursor + 1).min(route.stops.len().saturating_sub(1));
        } else if keys.just_pressed(KeyCode::KeyS) {
            route.cursor = route.cursor.saturating_sub(1);
        }
        moved = previous != route.cursor;
    }
    if moved
        && let Some(route) = &library.route
        && let Some(stop) = route.stops.get(route.cursor)
    {
        library.destination = destination(&repo.0, &stop.path, stop.line, route.face);
    }
}

fn destination(
    repo: &Repository,
    path: &str,
    line: usize,
    face: listing::Face,
) -> Option<Destination> {
    let (index, file) = repo
        .files
        .iter()
        .enumerate()
        .find(|(_, file)| file.path == path)?;
    Some(Destination {
        index,
        before: file.after.is_none(),
        line,
        end: (line > 1).then_some(line),
        face,
    })
}
