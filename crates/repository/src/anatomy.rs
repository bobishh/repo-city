//! Anatomy records and metrics copied from Sloposcope.
use serde::Serialize;
#[cfg(not(target_arch = "wasm32"))]
#[path = "parser_metrics.rs"]
mod parser_metrics;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnatomyStatus {
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallableVisibility {
    Public,
    Private,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallableAnatomy {
    pub id: String,
    pub match_key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub kind: String,
    pub visibility: CallableVisibility,
    pub arity: Option<usize>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub body_line_span: Option<u32>,
    pub decision_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSlotAnatomy {
    pub name: String,
    pub kind: String,
    pub visibility: CallableVisibility,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAnatomy {
    pub status: AnatomyStatus,
    pub file: String,
    pub language: String,
    pub source_id: String,
    pub profile_id: String,
    pub line_count: Option<u32>,
    pub first_seen_at: Option<String>,
    pub age_as_of: Option<String>,
    pub age_days: Option<u32>,
    pub age_reason: Option<String>,
    pub evidence: String,
    pub callables: Vec<CallableAnatomy>,
    pub state_slots: Option<Vec<StateSlotAnatomy>>,
    pub reason: Option<String>,
}

impl FileAnatomy {
    /// Canonical, zero-based reading regions. Nested callables remain in the
    /// anatomy inventory, but their outer region owns every shared source row.
    pub fn reading_regions(&self, total_lines: usize) -> Vec<std::ops::Range<usize>> {
        let mut ranges = self
            .callables
            .iter()
            .filter_map(|callable| {
                let (start, end) = (callable.start_line? as usize, callable.end_line? as usize);
                (start > 0 && start <= end && end <= total_lines).then(|| start - 1..end)
            })
            .collect::<Vec<_>>();
        ranges.sort_unstable_by_key(|range| (range.start, range.end));
        let mut regions: Vec<std::ops::Range<usize>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            if let Some(previous) = regions.last_mut()
                && range.start < previous.end
            {
                previous.end = previous.end.max(range.end);
            } else {
                regions.push(range);
            }
        }
        regions
    }

    pub fn unavailable(file: &str, language: &str, reason: &str) -> Self {
        Self {
            status: AnatomyStatus::Unavailable,
            file: file.to_string(),
            language: language.to_string(),
            source_id: String::new(),
            profile_id: format!("{language}:unavailable"),
            line_count: None,
            first_seen_at: None,
            age_as_of: None,
            age_days: None,
            age_reason: None,
            evidence: "tree_sitter".to_string(),
            callables: Vec::new(),
            state_slots: None,
            reason: Some(reason.to_string()),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn physical_line_count(content: &[u8]) -> u32 {
    let line_feeds = content.iter().filter(|byte| **byte == b'\n').count() as u64;
    let trailing_line = u64::from(!content.is_empty() && content.last() != Some(&b'\n'));
    line_feeds
        .saturating_add(trailing_line)
        .min(u32::MAX as u64) as u32
}

#[cfg(not(target_arch = "wasm32"))]
pub fn analyze(file: &str, source: &str) -> FileAnatomy {
    let extension = std::path::Path::new(file)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    parser_metrics::analyze(file, extension, tree_sitter_rust::LANGUAGE.into(), source)
}

#[cfg(target_arch = "wasm32")]
pub fn analyze(file: &str, _: &str) -> FileAnatomy {
    FileAnatomy::unavailable(file, "", "Syntax extraction requires the native build")
}

#[cfg(test)]
mod reading_tests {
    use super::*;

    fn callable(start: Option<u32>, end: Option<u32>) -> CallableAnatomy {
        CallableAnatomy {
            id: String::new(),
            match_key: String::new(),
            name: String::new(),
            display_name: None,
            kind: "function".into(),
            visibility: CallableVisibility::Unknown,
            arity: None,
            start_line: start,
            end_line: end,
            start_byte: 0,
            end_byte: 0,
            body_line_span: None,
            decision_count: None,
        }
    }

    #[test]
    fn reading_regions_keep_outer_ownership_without_losing_nested_anatomy() {
        let mut anatomy = FileAnatomy::unavailable("callbacks.mjs", "js", "fixture");
        anatomy.callables = vec![
            callable(Some(5), Some(11)),
            callable(Some(52), Some(54)),
            callable(Some(4), Some(50)),
            callable(Some(55), Some(56)),
            callable(Some(9), Some(9)),
            callable(Some(4), Some(50)),
        ];
        assert_eq!(anatomy.reading_regions(56), vec![3..50, 51..54, 54..56]);
        assert_eq!(anatomy.callables.len(), 6);
        anatomy.callables.reverse();
        assert_eq!(anatomy.reading_regions(56), vec![3..50, 51..54, 54..56]);
    }

    #[test]
    fn malformed_regions_are_ignored_and_shared_physical_rows_have_one_owner() {
        let mut anatomy = FileAnatomy::unavailable("callbacks.mjs", "js", "fixture");
        anatomy.callables = vec![
            callable(Some(0), Some(1)),
            callable(Some(8), Some(7)),
            callable(Some(4), Some(51)),
            callable(None, Some(3)),
            callable(Some(1), None),
        ];
        assert!(anatomy.reading_regions(50).is_empty());
        anatomy.callables.extend([
            callable(Some(1), Some(5)),
            callable(Some(5), Some(8)),
            callable(Some(9), Some(9)),
        ]);
        assert_eq!(anatomy.reading_regions(50), vec![0..8, 8..9]);
    }
}
