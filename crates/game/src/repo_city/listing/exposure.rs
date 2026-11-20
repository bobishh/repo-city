//! Readable diff and blame surfaces can reveal their original source lines.
use super::*;

struct Viewed {
    rows: Range<usize>,
    lines: Vec<(bool, syntax::Line)>,
}

impl Listing {
    fn advance_seen(
        &mut self,
        explorer: &Explorer,
        file: &repo_world::File,
        view: Viewed,
        delta: f32,
        settings: reading::Settings,
    ) -> Vec<progress::Mark> {
        if !settings.enabled {
            self.observed = None;
            return Vec::new();
        }
        if view.lines.is_empty()
            || view.lines.iter().all(|(before, line)| {
                self.revealed
                    .contains(&(explorer.selected, *before, line.number))
            })
        {
            self.observed = None;
            return Vec::new();
        }
        let seconds = settings.seconds(view.lines.iter().map(|(_, line)| line.text.as_str()));
        let key = reading_key(
            file,
            view.lines
                .iter()
                .map(|(before, line)| (*before, line.number, line.text.as_str())),
        );
        let elapsed = self.timers.advance(key, delta, seconds);
        self.observed = Some(WashPreview {
            file: explorer.selected,
            before: explorer.before,
            rows: view.rows,
            elapsed,
            seconds,
        });
        if elapsed < seconds {
            return Vec::new();
        }
        self.observed = None;
        let mut marks = Vec::new();
        for before in [true, false] {
            let total = explorer
                .comparison
                .as_ref()
                .map_or(explorer.highlighted.len(), |diff| {
                    if before {
                        diff.before.len()
                    } else {
                        diff.after.len()
                    }
                });
            let mut rows = view
                .lines
                .iter()
                .filter(|(side, _)| *side == before)
                .map(|(_, line)| line.number - 1)
                .filter(|&row| row < total)
                .collect::<Vec<_>>();
            rows.sort_unstable();
            rows.dedup();
            let mut first = 0;
            while first < rows.len() {
                let mut end = first + 1;
                while end < rows.len() && rows[end] == rows[end - 1] + 1 {
                    end += 1;
                }
                let range = rows[first]..rows[end - 1] + 1;
                if self.reveal(explorer.selected, before, file, range.clone(), total) {
                    marks.push(progress::Mark::Reveal {
                        path: file.path.clone(),
                        blob: (if before { &file.before } else { &file.after })
                            .as_ref()
                            .map(|blob| blob.id.clone()),
                        before,
                        start: range.start,
                        end: range.end,
                        rows: total,
                    });
                }
                first = end;
            }
        }
        marks
    }
}

fn aligned(explorer: &Explorer, rows: Range<usize>, side: Option<bool>) -> Option<Viewed> {
    let diff = explorer.comparison.as_ref()?;
    let lines = rows
        .clone()
        .filter_map(|index| diff.rows.get(index))
        .flat_map(|row| {
            [true, false]
                .into_iter()
                .filter(move |before| side.is_none_or(|side| side == *before))
                .filter_map(move |before| {
                    let index = if before { row.before } else { row.after }?;
                    Some((
                        before,
                        if before {
                            diff.before[index].clone()
                        } else {
                            diff.after[index].clone()
                        },
                    ))
                })
        })
        .collect();
    Some(Viewed { rows, lines })
}

pub(crate) fn update(
    time: Res<Time>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    mut listing: ResMut<Listing>,
    navigation: Res<city_navigation::Navigation>,
    review: Res<review_notes::Review>,
    picker: Res<file_picker::FilePicker>,
    menu: Res<city_menu::Menu>,
    search: Res<find::Search>,
    settings: Res<reading::Settings>,
    camera: Query<&Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut progress: ResMut<progress::Progress>,
) {
    if !settings.enabled {
        listing.observed = None;
        listing.wash_preview = None;
        return;
    }
    let mut guided_focus = false;
    let view = (|| {
        let (camera, window) = (camera.single().ok()?, windows.single().ok()?);
        if !window.focused
            || review.blocked
            || picker.is_open()
            || menu.is_open()
            || search.blocks_controls()
            || navigation.is_flying()
        {
            return None;
        }
        let file = repo.0.files.get(explorer.selected)?;
        if explorer.comparison_mode > 0 {
            let (before, rows) = comparison_scene::reading_rows(file, &explorer, camera)?;
            return aligned(&explorer, rows, Some(before));
        }
        if !Listing::visible(&explorer, &navigation) || listing.turning() {
            return None;
        }
        let aspect = window.width() / window.height().max(1.);
        let face = listing.close_face(file, camera, &explorer, aspect)?;
        if face == Face::Source {
            if (settings.show_all_source
                || listing.guided == Some((explorer.selected, explorer.before)))
                && !review.tablet_open
            {
                let count = explorer.source_layout.len();
                let side = WallSide::new(file, count, face, block_size(file));
                let row = side.wall.row(side.look_height(camera), count);
                guided_focus = true;
                if let Some(mark) =
                    listing.advance_wash(&explorer, file, row, time.delta_secs(), *settings)
                {
                    progress.mark(mark, true);
                }
            }
            return None;
        }
        let count = if face == Face::Diff {
            explorer.diff_layout.rows.len()
        } else {
            explorer.blame_layout.len()
        };
        let side = WallSide::new(file, count, face, block_size(file));
        let visible = side.visible_rows(camera, count, aspect);
        let focus = side.wall.row(side.look_height(camera), count);
        let mut rows = if face == Face::Blame {
            settings.block_in_layout(&explorer, &explorer.blame_layout, focus)
        } else {
            settings.rows(focus, count)
        };
        if face == Face::Diff {
            if let Some((Some(line), mark)) = explorer.diff_layout.rows.get(focus) {
                let before = *mark == Mark::Removed;
                if let Some(diff) = &explorer.comparison
                    && let Some(hunk) = diff.hunks.iter().find(|hunk| {
                        if before {
                            hunk.before.contains(&(line.number - 1))
                        } else {
                            hunk.after.contains(&(line.number - 1))
                        }
                    })
                {
                    let matching = explorer
                        .diff_layout
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(_, (line, mark))| {
                            line.as_ref().is_some_and(|line| {
                                if *mark == Mark::Removed {
                                    hunk.before.contains(&(line.number - 1))
                                } else {
                                    hunk.after.contains(&(line.number - 1))
                                }
                            })
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    if let (Some(&start), Some(&end)) = (matching.first(), matching.last()) {
                        rows = start..end + 1;
                    }
                }
            }
        }
        rows = rows.start.max(visible.start)..rows.end.min(visible.end);
        if rows.is_empty() {
            return None;
        }
        let mut lines: Vec<(bool, syntax::Line)> = if face == Face::Diff {
            explorer.diff_layout.rows[rows.clone()]
                .iter()
                .filter_map(|(line, mark)| line.clone().map(|line| (*mark == Mark::Removed, line)))
                .collect()
        } else {
            explorer.blame_layout.rows[rows.clone()]
                .iter()
                .cloned()
                .map(|line| (explorer.before, line))
                .collect()
        };
        // Continuations retain one canonical source line for read progress and timing.
        lines.dedup_by_key(|(before, line)| (*before, line.number));
        for (before, line) in &mut lines {
            let original = explorer
                .comparison
                .as_ref()
                .map_or(&explorer.highlighted, |diff| {
                    if *before { &diff.before } else { &diff.after }
                });
            if let Some(original) = original.get(line.number - 1) {
                *line = original.clone();
            }
        }
        Some(Viewed { rows, lines })
    })();
    if (settings.show_all_source || listing.guided.is_some()) && !guided_focus {
        listing.wash_preview = None;
    }
    if let Some(view) = view {
        let file = &repo.0.files[explorer.selected];
        for mark in listing.advance_seen(&explorer, file, view, time.delta_secs(), *settings) {
            progress.mark(mark, true);
        }
    } else {
        listing.observed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> Viewed {
        Viewed {
            rows: 0..1,
            lines: vec![
                (true, syntax::highlight("test.rs", "old value\n").remove(0)),
                (false, syntax::highlight("test.rs", "new value\n").remove(0)),
            ],
        }
    }

    #[test]
    fn disabling_reading_pauses_diff_and_blame_credit_without_resetting_time() {
        let file = super::super::tests::fixture_file();
        let mut explorer = Explorer::new(0);
        explorer.set_highlighted(syntax::highlight("test.rs", "new value\n"));
        let enabled = reading::Settings {
            min_seconds: Some(1.),
            max_seconds: Some(1.),
            ..default()
        };
        let disabled = reading::Settings {
            enabled: false,
            ..enabled
        };
        let mut listing = Listing::default();
        assert!(
            listing
                .advance_seen(&explorer, &file, view(), 0.4, enabled)
                .is_empty()
        );
        assert!(
            listing
                .advance_seen(&explorer, &file, view(), 30., disabled)
                .is_empty()
        );
        assert!(listing.observed.is_none());
        assert!(listing.revealed.is_empty());
        assert_eq!(
            listing
                .advance_seen(&explorer, &file, view(), 0.61, enabled)
                .len(),
            2
        );
    }
    #[test]
    fn reading_a_diff_resumes_then_reveals_only_the_seen_lines_on_each_side() {
        let file = super::super::tests::fixture_file();
        let mut explorer = Explorer::new(0);
        explorer.set_highlighted(syntax::highlight("test.rs", "new value\nunseen\n"));
        explorer.set_comparison(Some(
            Comparison::new("test.rs", "old value\nunseen\n", "new value\nunseen\n").unwrap(),
        ));
        let settings = reading::Settings {
            min_seconds: Some(1.),
            max_seconds: Some(1.),
            ..default()
        };
        let mut listing = Listing::default();
        assert!(
            listing
                .advance_seen(&explorer, &file, view(), 0.5, settings)
                .is_empty()
        );
        assert!(!listing.revealed.contains(&(0, false, 1)));
        listing.observed = None;
        let marks = listing.advance_seen(&explorer, &file, view(), 0.5, settings);
        assert_eq!(marks.len(), 2);
        assert!(listing.revealed.contains(&(0, true, 1)));
        assert!(listing.revealed.contains(&(0, false, 1)));
        assert!(!listing.revealed.contains(&(0, false, 2)));
        assert!(
            listing
                .advance_seen(&explorer, &file, view(), 10., settings)
                .is_empty()
        );
    }
}
