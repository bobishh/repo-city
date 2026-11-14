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
