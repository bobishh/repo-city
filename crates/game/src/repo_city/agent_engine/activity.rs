//! Bounded public action history. An item keeps its identity across streaming updates.
use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Command,
    Edit,
    Tool,
    Search,
    Delegate,
    Plan,
    Reasoning,
    Session,
}

impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::Edit => "Edit",
            Self::Tool => "Tool",
            Self::Search => "Search",
            Self::Delegate => "Delegation",
            Self::Plan => "Plan",
            Self::Reasoning => "Summary",
            Self::Session => "Session",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Running,
    Completed,
    Failed,
    Ended,
}

impl Phase {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Completed => "Done",
            Self::Failed => "Failed",
            Self::Ended => "Ended",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub(crate) path: String,
    pub(crate) line: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Details {
    pub(crate) kind: Kind,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) locations: Vec<Location>,
    pub(crate) failed: bool,
}

impl Details {
    pub(crate) fn new(kind: Kind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            body: String::new(),
            locations: Vec::new(),
            failed: false,
        }
    }
}

pub(crate) struct Entry {
    pub(crate) serial: u64,
    pub(crate) thread: String,
    pub(crate) item: String,
    pub(crate) phase: Phase,
    pub(crate) details: Details,
    pub(crate) updated: u64,
}

#[derive(Default)]
pub(crate) struct Timeline {
    pub(crate) entries: VecDeque<Entry>,
    pub(crate) version: u64,
    next_serial: u64,
}

impl Timeline {
    pub(crate) fn record(&mut self, thread: &str, item: &str, mut details: Details, phase: Phase) {
        details.title = bounded(&details.title, 256);
        details.body = bounded(&details.body, 2_048);
        details.locations.truncate(16);
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.thread == thread && entry.item == item)
        {
            if entry.phase != Phase::Running && phase == Phase::Running {
                return;
            }
            if details.title.is_empty() {
                details.title = std::mem::take(&mut entry.details.title);
                if details.kind == Kind::Tool {
                    details.kind = entry.details.kind;
                }
            }
            if details.body.is_empty() {
                details.body = std::mem::take(&mut entry.details.body);
            }
            if details.locations.is_empty() {
                details.locations = std::mem::take(&mut entry.details.locations);
            }
            entry.details = details;
            entry.phase = phase;
            entry.updated = now();
        } else {
            if details.title.is_empty() {
                details.title = details.kind.label().into();
            }
            self.next_serial = self.next_serial.wrapping_add(1);
            self.entries.push_back(Entry {
                serial: self.next_serial,
                thread: thread.into(),
                item: item.into(),
                phase,
                updated: now(),
                details,
            });
            if self.entries.len() > 96 {
                self.entries.pop_front();
            }
        }
        self.version = self.version.wrapping_add(1);
    }

    pub(crate) fn append(&mut self, thread: &str, item: &str, kind: Kind, text: &str) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.thread == thread && entry.item == item)
        {
            if entry.phase != Phase::Running {
                return;
            }
            let available = 2_048_usize.saturating_sub(entry.details.body.chars().count());
            entry.details.body.push_str(&bounded(text, available));
            self.version = self.version.wrapping_add(1);
        } else {
            let mut details = Details::new(kind, kind.label());
            details.body = text.into();
            self.record(thread, item, details, Phase::Running);
        }
    }

    pub(crate) fn end_turn(&mut self, thread: &str) {
        for entry in &mut self.entries {
            if entry.thread == thread && entry.phase == Phase::Running {
                entry.phase = Phase::Ended;
                entry.updated = now();
                self.version = self.version.wrapping_add(1);
            }
        }
    }
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

fn bounded(text: &str, max: usize) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .take(max)
        .collect()
}
