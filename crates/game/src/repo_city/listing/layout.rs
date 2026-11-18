//! Soft-wrap is a presentation projection; repository line identities never change.
use super::*;

#[derive(Default)]
pub(crate) struct SourceLayout {
    pub rows: Vec<syntax::Line>,
    pub bytes: Vec<Range<usize>>,
    starts: Vec<usize>,
    revision: u64,
    folds: Vec<Range<usize>>,
}

impl SourceLayout {
    pub fn new(lines: &[syntax::Line]) -> Self {
        Self::project(lines, Vec::new())
    }

    fn project(lines: &[syntax::Line], folds: Vec<Range<usize>>) -> Self {
        let mut result = Self {
            folds,
            ..Self::default()
        };
        let mut advances = HashMap::new();
        let limit = code_texture::LISTING_WIDTH as f32 - 106. - 20.;
        for (index, line) in lines.iter().enumerate() {
            result.starts.push(result.rows.len());
            if result
                .folds
                .iter()
                .any(|range| index > range.start && index < range.end)
            {
                continue;
            }
            let folded = result
                .folds
                .iter()
                .filter(|range| range.start == index)
                .max_by_key(|range| range.len());
            let placeholder;
            let line = if let Some(range) = folded {
                placeholder = syntax::Line {
                    number: line.number,
                    text: format!("{}  … [{} lines]", line.text, range.len()),
                    spans: line.spans.clone(),
                };
                &placeholder
            } else {
                line
            };
            let mut start = 0;
            let mut first_span = 0;
            loop {
                let mut used = 0.;
                let mut end = start;
                let mut word = None;
                for (offset, ch) in line.text[start..].char_indices() {
                    let advance = *advances
                        .entry(ch)
                        .or_insert_with(|| code_texture::source_advance(ch));
                    if used + advance > limit && end > start {
                        break;
                    }
                    used += advance;
                    end = start + offset + ch.len_utf8();
                    if ch.is_whitespace() {
                        word = Some(end);
                    }
                }
                if end < line.text.len()
                    && let Some(boundary) =
                        word.filter(|&boundary| !line.text[start..boundary].trim().is_empty())
                {
                    end = boundary;
                }
                let bytes = start..end;
                while first_span < line.spans.len() && line.spans[first_span].bytes.end <= start {
                    first_span += 1;
                }
                result.rows.push(syntax::Line {
                    number: line.number,
                    text: line.text[bytes.clone()].into(),
                    spans: line.spans[first_span..]
                        .iter()
                        .take_while(|span| span.bytes.start < end)
                        .filter_map(|span| {
                            let a = span.bytes.start.max(start);
                            let b = span.bytes.end.min(end);
                            (a < b).then(|| syntax::Span {
                                bytes: a - start..b - start,
                                kind: span.kind,
                            })
                        })
                        .collect(),
                });
                result.bytes.push(bytes);
                if end == line.text.len() {
                    break;
                }
                start = end;
            }
        }
        result.starts.push(result.rows.len());
        result
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn replace(&mut self, lines: &[syntax::Line]) {
        let revision = self.revision.wrapping_add(1);
        *self = Self::new(lines);
        self.revision = revision;
    }

    pub fn fold(&mut self, lines: &[syntax::Line], range: Range<usize>) -> bool {
        if range.len() < 2 || range.end > lines.len() || self.folds.contains(&range) {
            return false;
        }
        let mut folds = self.folds.clone();
        folds.push(range);
        self.reproject(lines, folds);
        true
    }
    pub fn unfold(&mut self, lines: &[syntax::Line], row: usize) -> bool {
        let folds = self
            .folds
            .iter()
            .filter(|range| !range.contains(&row))
            .cloned()
            .collect::<Vec<_>>();
        if folds.len() == self.folds.len() {
            return false;
        }
        self.reproject(lines, folds);
        true
    }
    pub fn unfold_all(&mut self, lines: &[syntax::Line]) -> bool {
        if self.folds.is_empty() {
            return false;
        }
        self.reproject(lines, Vec::new());
        true
    }
    fn reproject(&mut self, lines: &[syntax::Line], folds: Vec<Range<usize>>) {
        let revision = self.revision.wrapping_add(1);
        *self = Self::project(lines, folds);
        self.revision = revision;
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn visual_rows(&self, line: usize) -> Range<usize> {
        self.starts.get(line).copied().unwrap_or(self.len())
            ..self.starts.get(line + 1).copied().unwrap_or(self.len())
    }

    pub fn visual_row(&self, line: usize, byte: usize) -> usize {
        let range = self.visual_rows(line);
        range
            .clone()
            .find(|&row| self.bytes[row].contains(&byte))
            .unwrap_or(range.start)
    }

    pub fn canonical_range(&self, rows: Range<usize>) -> Range<usize> {
        if rows.is_empty() || rows.start >= self.len() {
            return 0..0;
        }
        self.rows[rows.start].number.saturating_sub(1)
            ..self.rows[rows.end.min(self.len()) - 1].number
    }

    pub fn find_rows(&self, lines: &[syntax::Line], needle: &str) -> Vec<usize> {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let offset = line.text.to_lowercase().find(&needle)?;
                let mut lower_bytes = 0;
                let byte = line
                    .text
                    .char_indices()
                    .find_map(|(byte, ch)| {
                        let start = lower_bytes;
                        lower_bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
                        (offset >= start && offset < lower_bytes).then_some(byte)
                    })
                    .unwrap_or(0);
                Some(self.visual_row(index, byte))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_preserves_source_line_identity_and_expanding_restores_all_bytes() {
        let lines = syntax::highlight(
            "test.rs",
            "fn first() {\n  let a = 1;\n  work(a);\n}\nfn next() {}\n",
        );
        let mut layout = SourceLayout::new(&lines);
        assert!(layout.fold(&lines, 0..4));
        assert_eq!(layout.len(), 2);
        assert!(layout.rows[0].text.contains("[4 lines]"));
        assert!(layout.visual_rows(2).is_empty());
        assert_eq!(layout.rows[1].number, 5);
        assert_eq!(layout.canonical_range(1..2), 4..5);
        assert!(layout.unfold(&lines, 0));
        assert_eq!(
            layout
                .rows
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn syntax_spans_and_tail_search_keep_original_utf8_coordinates() {
        let source = format!("const text = \"{} TAIL\";\n", "unicode🦀 text ".repeat(80));
        let lines = syntax::highlight("test.mjs", &source);
        let layout = SourceLayout::new(&lines);
        for (row, bytes) in layout.rows.iter().zip(&layout.bytes) {
            for span in &row.spans {
                assert!(
                    lines[0]
                        .spans
                        .iter()
                        .any(|original| original.kind == span.kind
                            && original.bytes.start <= bytes.start + span.bytes.start
                            && original.bytes.end >= bytes.start + span.bytes.end)
                );
            }
        }
        let hits = layout.find_rows(&lines, "tail");
        assert_eq!(hits.len(), 1);
        assert!(hits[0] > 0 && layout.rows[hits[0]].text.contains("TAIL"));
        let mut refreshed = layout;
        let before = refreshed.revision();
        refreshed.replace(&syntax::highlight("test.mjs", "short\n"));
        assert_ne!(refreshed.revision(), before);
        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed.rows[0].text, "short");
    }

    #[test]
    fn long_markdown_and_unbroken_identifiers_wrap_without_losing_source_bytes() {
        let source = format!(
            "# Heading\n\t{} TAIL\n{}\n",
            "Read this code 🦀 и проверь. ".repeat(30),
            "unbroken_identifier_".repeat(80)
        );
        let lines = syntax::highlight("notes.md", &source);
        let layout = SourceLayout::new(&lines);
        assert!(
            layout.len() > lines.len(),
            "long source must have continuation rows"
        );
        for (index, line) in lines.iter().enumerate() {
            let range = layout.visual_rows(index);
            assert_eq!(
                layout.rows[range.clone()]
                    .iter()
                    .map(|row| row.text.as_str())
                    .collect::<String>(),
                line.text
            );
            assert!(
                layout.rows[range.clone()]
                    .iter()
                    .all(|row| row.number == line.number)
            );
            for row in range {
                assert_eq!(&line.text[layout.bytes[row].clone()], layout.rows[row].text);
                assert!(
                    layout.rows[row]
                        .spans
                        .iter()
                        .all(|span| span.bytes.end <= layout.rows[row].text.len())
                );
            }
        }
        let tail = lines[1].text.find("TAIL").unwrap();
        let row = layout.visual_row(1, tail);
        assert!(layout.rows[row].text.contains("TAIL"));
        assert_eq!(layout.canonical_range(row..row + 1), 1..2);
    }
}
