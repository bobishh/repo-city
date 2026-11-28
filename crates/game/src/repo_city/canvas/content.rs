use super::*;

#[derive(Default)]
pub(super) struct Content {
    blobs: Option<(Option<repo_world::Blob>, Option<repo_world::Blob>)>,
    before: Option<String>,
    retry_pending: bool,
    pub(super) rows: Rows,
}

pub(super) struct Line<'a> {
    pub line: usize,
    pub before: bool,
    pub mark: char,
    pub text: &'a str,
}

#[derive(Default)]
pub(super) enum Rows {
    #[default]
    Missing,
    Error(String),
    Source(Vec<String>),
    Diff(Comparison),
}

impl Content {
    pub(super) fn refresh(&mut self, repo: &Repository, path: &str) {
        let Some(index) = repo
            .files
            .binary_search_by(|file| file.path.as_str().cmp(path))
            .ok()
        else {
            self.blobs = None;
            self.before = None;
            self.rows = Rows::Missing;
            return;
        };
        let file = &repo.files[index];
        let same_blobs = self
            .blobs
            .as_ref()
            .is_some_and(|(before, after)| before == &file.before && after == &file.after);
        let retry = std::mem::take(&mut self.retry_pending);
        if same_blobs && !retry {
            return;
        }
        if !same_blobs {
            self.before = None;
        }
        self.blobs = Some((file.before.clone(), file.after.clone()));
        if file
            .before
            .iter()
            .chain(file.after.iter())
            .any(|blob| blob.size > repo_world::ASSET_BYTES_LIMIT)
        {
            self.rows = Rows::Error("Source exceeds the 16 MiB preview limit.".into());
            return;
        }
        let read = |before, present| {
            if present {
                repo.source(index, before)
            } else {
                Ok(String::new())
            }
        };
        let rows = if file.change == Change::Unchanged {
            repo.source(index, file.after.is_none())
                .map(|source| Rows::Source(source.lines().map(str::to_owned).collect()))
        } else {
            if self.before.is_none() {
                match read(true, file.before.is_some()) {
                    Ok(source) => self.before = Some(source),
                    Err(error) => {
                        self.rows = Rows::Error(error);
                        return;
                    }
                }
            }
            read(false, file.after.is_some())
                .and_then(|after| {
                    Comparison::new(path, self.before.as_deref().unwrap_or_default(), &after)
                })
                .map(Rows::Diff)
        };
        self.rows = rows.unwrap_or_else(Rows::Error);
        if !self.pending() {
            self.before = None;
        }
    }

    pub(super) fn lines(&self, start: usize, count: usize) -> Vec<Line<'_>> {
        match &self.rows {
            Rows::Diff(diff) => diff
                .rows
                .iter()
                .skip(start)
                .take(count)
                .flat_map(|row| {
                    [
                        row.before
                            .filter(|_| row.changed || row.after.is_none())
                            .map(|line| Line {
                                line,
                                before: true,
                                mark: if row.changed { '-' } else { ' ' },
                                text: &diff.before[line].text,
                            }),
                        row.after.map(|line| Line {
                            line,
                            before: false,
                            mark: if row.changed { '+' } else { ' ' },
                            text: &diff.after[line].text,
                        }),
                    ]
                })
                .flatten()
                .take(count)
                .collect(),
            Rows::Source(lines) => lines
                .iter()
                .enumerate()
                .skip(start)
                .take(count)
                .map(|(line, text)| Line {
                    line,
                    before: false,
                    mark: ' ',
                    text,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn first_visible(&self, tile: &model::Tile) -> usize {
        if tile.positioned {
            tile.viewport.row.min(self.len().saturating_sub(1))
        } else {
            self.row_at_line(None)
        }
    }

    pub(super) fn pending(&self) -> bool {
        matches!(&self.rows, Rows::Error(error) if error.starts_with("Loading source from GitHub"))
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn retry_if_pending(&mut self) -> bool {
        self.retry_pending = self.pending();
        self.retry_pending
    }

    pub(super) fn row_at_line(&self, line: Option<usize>) -> usize {
        self.row_at_anchor(line, false)
    }

    pub(super) fn row_at_anchor(&self, line: Option<usize>, before: bool) -> usize {
        match &self.rows {
            Rows::Diff(diff) => {
                if let Some(line) = line {
                    let line = line.saturating_sub(1);
                    let in_side = |before| {
                        diff.rows.iter().position(|row| {
                            (if before { row.before } else { row.after }) == Some(line)
                        })
                    };
                    in_side(before)
                        .or_else(|| in_side(!before))
                        .unwrap_or_else(|| diff.hunks.last().map_or(0, |hunk| hunk.row))
                } else {
                    diff.hunks.first().map_or(0, |hunk| hunk.row)
                }
            }
            Rows::Source(lines) => line
                .unwrap_or(1)
                .saturating_sub(1)
                .min(lines.len().saturating_sub(1)),
            _ => 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        match &self.rows {
            Rows::Diff(diff) => diff.rows.len(),
            Rows::Source(lines) => lines.len(),
            _ => 0,
        }
    }

    pub(super) fn hunk(&self, current: usize, next: bool) -> usize {
        let Rows::Diff(diff) = &self.rows else {
            return current;
        };
        if next {
            diff.hunks.iter().find(|hunk| hunk.row > current)
        } else {
            diff.hunks.iter().rev().find(|hunk| hunk.row < current)
        }
        .map_or(current, |hunk| hunk.row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserted_lines_preserve_both_anchor_coordinates() {
        let content = Content {
            rows: Rows::Diff(
                Comparison::new("file.txt", "first\nsecond\n", "inserted\nfirst\nsecond\n")
                    .unwrap(),
            ),
            ..default()
        };
        assert_eq!(content.row_at_anchor(Some(2), true), 2);
        assert_eq!(content.row_at_anchor(Some(2), false), 1);
    }
}
