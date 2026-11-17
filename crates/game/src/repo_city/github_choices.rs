//! Repository and pull request matches for the native GitHub picker.
use std::time::{Duration, Instant};

use super::native_github::{self, Job, Target};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Default)]
pub(super) enum Load {
    #[default]
    Idle,
    Pending(Job<Vec<Choice>>),
    Ready(Vec<Choice>),
    Failed(String),
}

impl Load {
    pub fn advance(&mut self) {
        if let Self::Pending(job) = self
            && let Some(result) = job.take()
        {
            *self = match result {
                Ok(items) => Self::Ready(items),
                Err(error) => Self::Failed(error),
            };
        }
    }

    pub fn results(&self, input: &str, empty: &str) -> (Vec<Choice>, String) {
        match self {
            Self::Idle => (vec![], String::new()),
            Self::Pending(_) => (vec![], "Loading from GitHub…".into()),
            Self::Failed(error) => (vec![], error.clone()),
            Self::Ready(items) => {
                let matches = matching(items, input);
                let message = if matches.is_empty() {
                    empty.into()
                } else {
                    String::new()
                };
                (matches, message)
            }
        }
    }
}

fn matching(items: &[Choice], input: &str) -> Vec<Choice> {
    let input = input.to_ascii_lowercase();
    items
        .iter()
        .filter(|item| {
            let label = item.label.to_ascii_lowercase();
            input.split_whitespace().all(|term| label.contains(term))
        })
        .take(8)
        .cloned()
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Editor {
    Revision,
    Base,
    PullRequests,
}

#[derive(Default)]
pub(super) struct Picker {
    pub target: Option<Target>,
    pub editor: Option<Editor>,
    pub revision: String,
    pub base: String,
    pub rows: Vec<Choice>,
    pub active: Option<usize>,
    input: String,
    library: Load,
    pull_requests: Load,
    revisions: Load,
    history: Load,
    input_since: Option<Instant>,
}

impl Picker {
    pub fn sample_library(&mut self) {
        self.library = Load::Ready(vec![
            Choice {
                value: "example/repo-city".into(),
                label: "example/repo-city".into(),
            },
            Choice {
                value: "example/demo".into(),
                label: "example/demo | Star".into(),
            },
        ]);
    }

    pub fn set_target(&mut self, target: Option<Target>) {
        self.target = target;
        self.editor = None;
        self.revision.clear();
        self.base.clear();
        self.revisions = Load::Idle;
        self.pull_requests = Load::Idle;
        self.reset_rows();
    }

    pub fn reset_rows(&mut self) {
        self.rows.clear();
        self.active = None;
        self.input.clear();
        self.input_since = None;
        self.history = Load::Idle;
    }

    pub fn load_library(&mut self, login: &str, token: String) {
        if matches!(self.library, Load::Idle | Load::Failed(_)) {
            self.library = Load::Pending(native_github::library(login.to_owned(), token));
        }
    }

    pub fn edit(&mut self, editor: Editor, token: Option<String>) {
        if let Some(target) = &self.target {
            if editor == Editor::PullRequests
                && matches!(self.pull_requests, Load::Idle | Load::Failed(_))
            {
                self.pull_requests =
                    Load::Pending(native_github::pull_requests(target.clone(), token));
            } else if editor != Editor::PullRequests
                && matches!(self.revisions, Load::Idle | Load::Failed(_))
            {
                self.revisions = Load::Pending(native_github::revisions(target.clone(), token));
            }
        }
        self.editor = Some(editor);
        self.reset_rows();
    }

    pub fn results(
        &mut self,
        input: &str,
        token: impl FnOnce() -> Option<String>,
    ) -> (Vec<Choice>, String) {
        self.library.advance();
        self.pull_requests.advance();
        self.revisions.advance();
        if self.input != input {
            self.input = input.to_owned();
            self.input_since = Some(Instant::now());
            self.history = Load::Idle;
            self.active = None;
        }
        match self.editor {
            Some(Editor::PullRequests) => self
                .pull_requests
                .results(input, "No matching pull requests."),
            Some(Editor::Revision | Editor::Base) => {
                let (mut rows, message) = self.revisions.results(input, "No matching revisions.");
                if input.trim().len() < 3
                    || input.starts_with("refs/")
                    || rows.len() == 8
                    || matches!(self.revisions, Load::Pending(_))
                {
                    return (rows, message);
                }
                if matches!(self.history, Load::Idle)
                    && self
                        .input_since
                        .is_some_and(|since| since.elapsed() >= Duration::from_millis(360))
                    && let Some(target) = &self.target
                {
                    self.history = Load::Pending(native_github::search_commits(
                        target.clone(),
                        input.into(),
                        token(),
                    ));
                }
                self.history.advance();
                match &self.history {
                    Load::Ready(history) => {
                        for item in history {
                            if rows.len() == 8 {
                                break;
                            }
                            if !rows.iter().any(|row| row.value == item.value) {
                                rows.push(item.clone());
                            }
                        }
                    }
                    Load::Failed(error) if rows.is_empty() => return (rows, error.clone()),
                    Load::Pending(_) if rows.is_empty() => {
                        return (rows, "Searching commits…".into());
                    }
                    _ => {}
                }
                let hint = if rows.is_empty() {
                    message
                } else {
                    String::new()
                };
                (rows, hint)
            }
            None if self.target.is_some() || input.trim_start().starts_with("https://") => {
                (vec![], String::new())
            }
            None => self
                .library
                .results(input, "No matches in your repositories, stars, or forks."),
        }
    }

    pub fn step(&mut self, down: bool) {
        let count = self.rows.len();
        if count == 0 {
            return;
        }
        self.active = Some(match (self.active, down) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(i), true) => (i + 1) % count,
            (Some(i), false) => (i + count - 1) % count,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_matches_each_typed_word_without_leaking_stale_selection() {
        let mut picker = Picker {
            library: Load::Ready(vec![
                Choice {
                    value: "owner/city".into(),
                    label: "owner/city | Private".into(),
                },
                Choice {
                    value: "other/city".into(),
                    label: "other/city | Star".into(),
                },
            ]),
            ..Default::default()
        };
        let (rows, message) = picker.results("", || None);
        assert_eq!(rows.len(), 2);
        assert!(message.is_empty());
        picker.rows = rows;
        picker.step(true);
        assert_eq!(picker.active, Some(0));
        let (rows, _) = picker.results("CITY star", || None);
        assert_eq!(rows[0].value, "other/city");
        assert_eq!(rows.len(), 1);
        assert_eq!(picker.active, None);
        assert!(
            picker
                .results("https://github.com/owner/city/pull/3/files", || None)
                .0
                .is_empty()
        );
    }

    #[test]
    fn pending_and_unavailable_results_have_distinct_feedback() {
        let pending = Load::Pending(Job::spawn(|| Ok(vec![])));
        assert_eq!(pending.results("", "No matches").1, "Loading from GitHub…");
        assert_eq!(
            Load::Failed("GitHub is unavailable.".into())
                .results("", "No matches")
                .1,
            "GitHub is unavailable."
        );
    }
}
