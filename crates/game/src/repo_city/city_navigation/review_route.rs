//! A review visits complete files from the lowest building to the highest.
use super::*;

pub(super) fn build(
    repo: &Repository,
    load: impl Fn(usize, bool) -> Result<String, String>,
) -> Result<(Route, String), String> {
    let mut files = repo
        .files
        .iter()
        .enumerate()
        .filter(|(_, file)| file.change != Change::Unchanged)
        .collect::<Vec<_>>();
    files.sort_by(|(_, left), (_, right)| {
        listing::block_size(left)
            .y
            .total_cmp(&listing::block_size(right).y)
            .then_with(|| left.path.cmp(&right.path))
    });
    let mut stops = Vec::new();
    let mut labels = Vec::new();
    let mut skipped = Vec::new();
    for (index, file) in files {
        let comparison = (|| {
            let before = if file.before.is_some() {
                load(index, true)?
            } else {
                String::new()
            };
            let after = if file.after.is_some() {
                load(index, false)?
            } else {
                String::new()
            };
            Comparison::new(&file.path, &before, &after)
        })();
        match comparison {
            Ok(diff) => {
                if let Some(stop) = review_stops(&diff, index).into_iter().next() {
                    labels.push(format!("{} · {} hunks", file.path, diff.hunks.len()));
                    stops.push(stop);
                }
            }
            Err(error) => skipped.push(format!("{}: {error}", file.path)),
        }
    }
    let notice = if skipped.is_empty() {
        String::new()
    } else {
        format!(
            "Skipped {} files without a readable diff. {}",
            skipped.len(),
            skipped[0]
        )
    };
    if stops.is_empty() {
        return Err(if notice.is_empty() {
            "No readable text changes in this comparison.".into()
        } else {
            notice
        });
    }
    Ok((
        Route {
            kind: RouteKind::Review,
            stops,
            current: 0,
            scroll: 0.,
            labels,
        },
        notice,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tour_visits_changed_buildings_in_height_order_and_keeps_all_hunks_on_each_roof() {
        let mut repo = repository_picker::empty();
        repo.files = [
            ("large.rs", 40_000),
            ("small.rs", 400),
            ("binary.png", 20),
            ("unchanged.rs", 20),
        ]
        .into_iter()
        .map(|(path, size)| {
            let blob = repo_world::Blob {
                id: "fixture".into(),
                size,
                mode: "100644".into(),
            };
            repo_world::File {
                path: path.into(),
                before: Some(blob.clone()),
                after: Some(blob),
                change: if path == "unchanged.rs" {
                    Change::Unchanged
                } else {
                    Change::Modified
                },
                position: [0., 0.],
            }
        })
        .collect();
        let before = (0..26)
            .map(|row| format!("old {row}\n"))
            .collect::<String>();
        let after = (0..26)
            .map(|row| {
                if row % 2 == 0 {
                    format!("new {row}\n")
                } else {
                    format!("old {row}\n")
                }
            })
            .collect::<String>();
        let (route, notice) = build(&repo, |file, old| {
            assert_ne!(file, 3, "unchanged files must not be read");
            if file == 2 {
                return Err("Binary asset".into());
            }
            Ok(if old { before.clone() } else { after.clone() })
        })
        .unwrap();
        let addresses = route
            .stops
            .iter()
            .map(|stop| match stop {
                Stop::Review { file, hunk, .. } => (*file, *hunk),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(addresses, vec![(1, 0), (0, 0)]);
        assert!(route.labels.iter().all(|label| label.contains("13 hunks")));
        assert!(notice.contains("binary.png"));
        assert!(stop_position(&route, 0, &repo, 1).y < stop_position(&route, 1, &repo, 0).y);
    }
}
