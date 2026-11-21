use repo_world::comparison::Mark;

use super::*;

pub(super) struct Rows<'a> {
    explorer: &'a Explorer,
    face: listing::Face,
}

pub(super) struct Row<'a> {
    pub line: &'a syntax::Line,
    pub before: bool,
    pub mark: Mark,
}

impl<'a> Rows<'a> {
    pub(super) fn new(explorer: &'a Explorer, face: listing::Face) -> Self {
        Self { explorer, face }
    }

    pub(super) fn len(&self) -> usize {
        match self.face {
            listing::Face::Source => self.explorer.source_layout.len(),
            listing::Face::Blame => self.explorer.highlighted.len(),
            listing::Face::Diff => self
                .comparison()
                .map_or(0, |comparison| comparison.rows.len()),
        }
    }

    fn comparison(&self) -> Option<&'a Comparison> {
        self.explorer
            .comparison
            .as_ref()
            .filter(|comparison| !comparison.hunks.is_empty() || comparison.final_newline_changed)
    }

    pub(super) fn row(&self, index: usize) -> Option<Row<'a>> {
        if self.face != listing::Face::Diff {
            let lines = if self.face == listing::Face::Source {
                &self.explorer.source_layout.rows
            } else {
                &self.explorer.highlighted
            };
            return lines.get(index).map(|line| Row {
                line,
                before: self.explorer.before,
                mark: Mark::Context,
            });
        }
        let comparison = self.comparison()?;
        let row = comparison.rows.get(index)?;
        let before = row.after.is_none() || (self.explorer.before && row.before.is_some());
        let (line, changed) = if before {
            (comparison.before.get(row.before?), Mark::Removed)
        } else {
            (comparison.after.get(row.after?), Mark::Added)
        };
        line.map(|line| Row {
            line,
            before,
            mark: if row.changed { changed } else { Mark::Context },
        })
    }

    pub(super) fn locate(&self, line: usize) -> usize {
        let index = line.saturating_sub(1);
        if self.face == listing::Face::Source {
            return self
                .explorer
                .source_layout
                .visual_row(index, 0)
                .min(self.len().saturating_sub(1));
        }
        if self.face == listing::Face::Blame {
            return index.min(self.len().saturating_sub(1));
        }
        self.comparison()
            .and_then(|comparison| {
                comparison.rows.iter().position(|row| {
                    let index = if self.explorer.before {
                        row.before
                    } else {
                        row.after
                    };
                    index.is_some_and(|index| index + 1 == line)
                })
            })
            .unwrap_or(0)
    }

    pub(super) fn line_range(
        &self,
        viewport: std::ops::Range<usize>,
    ) -> std::ops::RangeInclusive<usize> {
        let mut numbers = viewport
            .filter_map(|index| self.row(index))
            .filter(|row| row.before == self.explorer.before)
            .map(|row| row.line.number);
        let start = numbers.next().unwrap_or(0);
        start..=numbers.next_back().unwrap_or(start)
    }
}

pub(super) fn guided(reader: &Reader, line: usize, now: f64) -> bool {
    reader
        .guidance
        .as_ref()
        .is_some_and(|(lines, until)| lines.contains(&line) && now < *until)
}

pub(super) fn readable(
    face: listing::Face,
    line: &Row,
    index: usize,
    row: usize,
    guidance: bool,
    settings: reading::Settings,
    listing: &listing::Listing,
) -> bool {
    face != listing::Face::Source
        || settings.source_is_visible()
        || guidance
        || listing.has_read_line(index, line.before, line.line.number)
        || listing.observes_row(index, line.before, row)
}

pub(super) fn face_name(face: listing::Face) -> &'static str {
    match face {
        listing::Face::Source => "Source",
        listing::Face::Diff => "Diff",
        listing::Face::Blame => "Blame",
    }
}

#[test]
fn a_diff_scroll_preserves_the_side_and_line_of_a_deleted_row() {
    let mut explorer = Explorer::new(0);
    explorer.comparison = Comparison::new("fixture.rs", "a\nb\nc\n", "a\nc\n").ok();
    let paper = Rows::new(&explorer, listing::Face::Diff);
    assert_eq!(paper.len(), 3);
    let deleted = paper.row(1).unwrap();
    assert_eq!(deleted.line.number, 2);
    assert_eq!(deleted.line.text, "b");
    assert_eq!(deleted.mark, Mark::Removed);
    assert!(deleted.before);
    assert_eq!(paper.locate(2), 2);
}
