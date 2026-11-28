use super::*;
use std::ops::Range;

#[derive(Default)]
pub(super) struct Content {
    blobs: Option<(Option<repo_world::Blob>, Option<repo_world::Blob>)>,
    before: Option<String>,
    retry_pending: bool,
    excerpts: Vec<Range<usize>>,
    pub(super) revision: u64,
    pub(super) rows: Rows,
}

pub(super) struct Line<'a> {
    pub line: usize,
    pub section: usize,
    pub before: bool,
    pub mark: char,
    pub text: &'a str,
}

pub(super) enum Visible<'a> {
    Code(Line<'a>),
    Gap(Range<usize>),
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
    fn changed(&mut self) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static REVISION: AtomicU64 = AtomicU64::new(1);
        self.revision = REVISION.fetch_add(1, Ordering::Relaxed);
    }

    fn set_rows(&mut self, rows: Rows) {
        if !matches!(
            (&self.rows, &rows),
            (Rows::Missing, Rows::Missing) | (Rows::Error(_), Rows::Error(_))
        ) || matches!((&self.rows, &rows), (Rows::Error(previous), Rows::Error(next)) if previous != next)
        {
            self.changed();
        }
        self.rows = rows;
    }

    pub(super) fn matches(&self, file: Option<&repo_world::File>) -> bool {
        match file {
            Some(file) => self
                .blobs
                .as_ref()
                .is_some_and(|(before, after)| before == &file.before && after == &file.after),
            None => self.blobs.is_none() && matches!(self.rows, Rows::Missing),
        }
    }

    pub(super) fn refresh(&mut self, repo: &Repository, path: &str) {
        let Some(index) = repo
            .files
            .binary_search_by(|file| file.path.as_str().cmp(path))
            .ok()
        else {
            self.blobs = None;
            self.before = None;
            self.set_rows(Rows::Missing);
            return;
        };
        let file = &repo.files[index];
        let same_blobs = self.matches(Some(file));
        let retry = std::mem::take(&mut self.retry_pending);
        if same_blobs && !retry {
            return;
        }
        if !same_blobs {
            self.excerpts.clear();
            self.before = None;
        }
        self.blobs = Some((file.before.clone(), file.after.clone()));
        if file
            .before
            .iter()
            .chain(file.after.iter())
            .any(|blob| blob.size > repo_world::ASSET_BYTES_LIMIT)
        {
            self.set_rows(Rows::Error(
                "Source exceeds the 16 MiB preview limit.".into(),
            ));
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
                        self.set_rows(Rows::Error(error));
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
        self.set_rows(rows.unwrap_or_else(Rows::Error));
        if let Rows::Diff(diff) = &self.rows {
            self.excerpts = excerpt_rows(diff);
        }
        if !self.pending() {
            self.before = None;
        }
    }

    pub(super) fn lines(&self, start: usize, count: usize) -> Vec<Line<'_>> {
        self.display(start, count, false)
            .into_iter()
            .filter_map(|row| {
                if let Visible::Code(line) = row {
                    Some(line)
                } else {
                    None
                }
            })
            .collect()
    }

    pub(super) fn visible_rows(&self, start: usize, count: usize) -> Vec<Visible<'_>> {
        self.display(start, count, true)
    }

    fn display(&self, start: usize, count: usize, gaps: bool) -> Vec<Visible<'_>> {
        match &self.rows {
            Rows::Diff(diff) => {
                let first = self.excerpts.partition_point(|range| range.end <= start);
                let mut cursor = start;
                let mut visible = Vec::with_capacity(count + 1);
                for range in &self.excerpts[first..] {
                    if gaps && cursor < range.start {
                        visible.push(Visible::Gap(cursor..range.start));
                    }
                    for row in &diff.rows[cursor.max(range.start)..range.end] {
                        if visible.len() >= count {
                            return visible;
                        }
                        if let Some(line) =
                            row.before.filter(|_| row.changed || row.after.is_none())
                        {
                            visible.push(Visible::Code(Line {
                                line,
                                section: range.start,
                                before: true,
                                mark: if row.changed { '-' } else { ' ' },
                                text: &diff.before[line].text,
                            }));
                        }
                        if visible.len() < count
                            && let Some(line) = row.after
                        {
                            visible.push(Visible::Code(Line {
                                line,
                                section: range.start,
                                before: false,
                                mark: if row.changed { '+' } else { ' ' },
                                text: &diff.after[line].text,
                            }));
                        }
                    }
                    cursor = range.end;
                    if visible.len() >= count {
                        return visible;
                    }
                }
                if gaps && !self.excerpts.is_empty() && cursor < diff.rows.len() {
                    visible.push(Visible::Gap(cursor..diff.rows.len()));
                }
                visible
            }
            Rows::Source(lines) => lines
                .iter()
                .enumerate()
                .skip(start)
                .take(count)
                .map(|(line, text)| {
                    Visible::Code(Line {
                        line,
                        section: 0,
                        before: false,
                        mark: ' ',
                        text,
                    })
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn first_visible(&self, tile: &model::Tile) -> usize {
        if tile.positioned {
            self.scroll(tile.viewport.row, 0)
        } else {
            0
        }
    }

    pub(super) fn scroll(&self, start: usize, delta: isize) -> usize {
        if self.excerpts.is_empty() {
            return start
                .saturating_add_signed(delta)
                .min(self.len().saturating_sub(1));
        }
        let mut skipped = 0usize;
        for range in &self.excerpts {
            if start < range.end {
                skipped += start.saturating_sub(range.start);
                break;
            }
            skipped += range.len();
        }
        let mut index = skipped.saturating_add_signed(delta);
        for range in &self.excerpts {
            if index < range.len() {
                return range.start + index;
            }
            index -= range.len();
        }
        self.excerpts.last().unwrap().end - 1
    }

    pub(super) fn expand(&mut self, gap: Range<usize>) -> bool {
        if gap.is_empty() || gap.end > self.len() || self.excerpts.is_empty() {
            return false;
        }
        if self
            .excerpts
            .iter()
            .any(|range| range.start <= gap.start && range.end >= gap.end)
        {
            return false;
        }
        merge_range(&mut self.excerpts, gap);
        self.changed();
        true
    }

    pub(super) fn expose(&mut self, row: usize) {
        let first = self.excerpts.partition_point(|range| range.end <= row);
        if self
            .excerpts
            .get(first)
            .is_some_and(|range| range.contains(&row))
        {
            return;
        }
        let before = first
            .checked_sub(1)
            .map_or(0, |index| self.excerpts[index].end);
        let after = self
            .excerpts
            .get(first)
            .map_or(self.len(), |range| range.start);
        // Explicit navigation needs local context, even far from a changed hunk.
        self.expand(row.saturating_sub(2).max(before)..row.saturating_add(3).min(after));
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

fn excerpt_rows(diff: &Comparison) -> Vec<Range<usize>> {
    const CONTEXT: usize = 2;
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (index, _) in diff.rows.iter().enumerate().filter(|(_, row)| row.changed) {
        let start = index.saturating_sub(CONTEXT);
        let end = (index + CONTEXT + 1).min(diff.rows.len());
        if let Some(previous) = ranges.last_mut()
            && previous.end >= start
        {
            previous.end = end;
        } else {
            ranges.push(start..end);
        }
    }
    ranges
}

fn merge_range(ranges: &mut Vec<Range<usize>>, interval: Range<usize>) {
    let first = ranges.partition_point(|range| range.end < interval.start);
    let end = ranges.partition_point(|range| range.start <= interval.end);
    let start = ranges
        .get(first)
        .filter(|_| first < end)
        .map_or(interval.start, |range| range.start.min(interval.start));
    let stop = end
        .checked_sub(1)
        .filter(|&i| i >= first)
        .map_or(interval.end, |i| ranges[i].end.max(interval.end));
    ranges.splice(first..end, std::iter::once(start..stop));
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(super) struct Worker {
    pending: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Batch>>>,
}

#[cfg(not(target_arch = "wasm32"))]
type Batch = (Scope, Vec<(String, Content)>);

#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    pub fn take(&mut self) -> Option<Batch> {
        match self.pending.as_mut()?.get_mut().ok()?.try_recv() {
            Ok(batch) => {
                self.pending = None;
                Some(batch)
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                None
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        }
    }

    pub fn schedule(&mut self, canvas: &Canvas, repo: &Repository, visible: &[(usize, String)]) {
        if self.pending.is_some() {
            return;
        }
        let paths = visible
            .iter()
            .filter(|(index, path)| {
                let file = canvas.model().tiles[*index].file(repo);
                !canvas
                    .contents
                    .get(path)
                    .is_some_and(|content| content.matches(file))
            })
            .take(6)
            .map(|(_, path)| path.clone())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        let repo = repo.clone();
        self.pending = Some(std::sync::Mutex::new(receive));
        std::thread::spawn(move || {
            let scope = Scope::for_repository(&repo);
            let rows = paths
                .into_iter()
                .map(|path| {
                    let mut content = Content::default();
                    content.refresh(&repo, &path);
                    (path, content)
                })
                .collect();
            let _ = send.send((scope, rows));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts_skip_unchanged_regions_and_keep_real_line_numbers() {
        let before = (0..45)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before
            .replace("line 7\n", "new seven\n")
            .replace("line 35\n", "new thirty-five\n");
        let diff = Comparison::new("sample.txt", &before, &after).unwrap();
        let excerpts = excerpt_rows(&diff);
        let content = Content {
            rows: Rows::Diff(diff),
            excerpts,
            ..default()
        };
        assert_eq!(content.lines(0, 1)[0].line, 5);
        assert!(!content.lines(0, 40).iter().any(|row| row.line == 20));
        assert_eq!(content.scroll(9, 1), 33);
        assert_ne!(
            content.lines(0, 1)[0].section,
            content.lines(33, 1)[0].section
        );
        assert!(
            content
                .lines(33, 8)
                .iter()
                .any(|row| row.line == 35 && row.mark == '+')
        );
    }

    #[test]
    fn expanding_a_gap_preserves_source_line_numbers_and_joins_sections() {
        let before = (0..45)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before
            .replace("line 7\n", "new seven\n")
            .replace("line 35\n", "new thirty-five\n");
        let diff = Comparison::new("sample.txt", &before, &after).unwrap();
        let excerpts = excerpt_rows(&diff);
        let mut content = Content {
            rows: Rows::Diff(diff),
            excerpts,
            ..default()
        };
        assert!(
            content
                .visible_rows(0, 30)
                .iter()
                .any(|row| matches!(row, Visible::Gap(gap) if gap == &(10..33)))
        );
        assert!(content.expand(10..33));
        assert!(!content.expand(10..33));
        let lines = content.lines(0, 50);
        assert_eq!(
            lines.iter().find(|row| row.line == 20).unwrap().text,
            "line 20"
        );
        assert_eq!(
            lines.first().unwrap().section,
            lines.last().unwrap().section
        );
    }

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
