//! Pasted traces resolve against the active snapshot; unresolved locations stay visible.
use super::*;
mod json;

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Span {
    pub group: String,
    pub name: String,
    pub id: String,
    pub parent: String,
    pub duration_ms: Option<f64>,
    pub error: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Group {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Frame {
    pub path: String,
    pub line: usize,
    pub suffix: bool,
    pub span: Option<Span>,
}

impl Frame {
    fn resolve(repo: &Repository, prefix: &str, line: usize) -> Result<Self, &'static str> {
        let normalized = prefix.trim().replace('\\', "/");
        let prefix = normalized.strip_prefix("at ").unwrap_or(&normalized);
        let exact = repo
            .files
            .iter()
            .filter(|file| file.after.is_some())
            .filter(|file| {
                prefix.strip_suffix(&file.path).is_some_and(|start| {
                    start.is_empty()
                        || start
                            .chars()
                            .last()
                            .is_some_and(|ch| matches!(ch, '/' | '(' | '@' | ' ' | '"' | '\''))
                })
            })
            .max_by_key(|file| file.path.len());
        let token = prefix
            .rsplit([' ', '(', '"', '\'', '\t', '@'])
            .next()
            .unwrap_or(prefix);
        let candidates = if let Some(file) = exact {
            vec![file]
        } else if !token.contains('/') {
            repo.files
                .iter()
                .filter(|file| file.after.is_some() && file.path.rsplit('/').next() == Some(token))
                .collect()
        } else {
            Vec::new()
        };
        match candidates.as_slice() {
            [] => Err("File not found in the selected snapshot."),
            [file] if line > 0 => Ok(Self {
                path: file.path.clone(),
                line,
                suffix: prefix != file.path,
                span: None,
            }),
            [_] => Err("Line must be a positive number."),
            _ => Err("Ambiguous file name; use a repository-relative path."),
        }
    }

    pub fn group(&self) -> &str {
        self.span
            .as_ref()
            .map_or("stack", |span| span.group.as_str())
    }

    pub fn description(&self) -> String {
        let mut label = format!(
            "{}:{}{}",
            self.path,
            self.line,
            if self.suffix {
                " (path suffix match)"
            } else {
                ""
            }
        );
        if let Some(span) = &self.span {
            label = format!("{} · {label}", span.name);
            if let Some(duration) = span.duration_ms {
                label.push_str(&format!(" · {duration:.3} ms"));
            }
            if span.error {
                label.push_str(" · error");
            }
            if !span.parent.is_empty() {
                label.push_str(&format!(" · parent {}", span.parent));
            }
        }
        label
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub(super) struct Report {
    pub frames: Vec<Frame>,
    pub issues: Vec<String>,
    pub ignored: usize,
    pub error: String,
    pub format: String,
    pub groups: Vec<Group>,
}

impl Report {
    fn parse(repo: &Repository, source: &str) -> Self {
        let mut report = Self::default();
        if source.len() > 64 * 1024 {
            report.error = "Trace exceeds the 64 KiB limit.".into();
            return report;
        }
        if source.trim_start().starts_with(['{', '[']) {
            return json::parse(repo, source);
        }
        for (index, text) in source
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
        {
            let python = text
                .split_once("File \"")
                .and_then(|(_, tail)| tail.split_once("\", line "));
            let locations = if let Some((path, tail)) = python {
                vec![(path, tail)]
            } else {
                text.char_indices()
                    .filter_map(|(offset, ch)| {
                        (ch == ':'
                            && text
                                .as_bytes()
                                .get(offset + 1)
                                .is_some_and(u8::is_ascii_digit))
                        .then_some((&text[..offset], &text[offset + 1..]))
                    })
                    .collect()
            };
            let mut located = None;
            for (prefix, tail) in locations {
                if !prefix.contains(['/', '\\', '.'])
                    && !repo.files.iter().any(|file| prefix.ends_with(&file.path))
                {
                    continue;
                }
                let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
                let line = tail[..digits]
                    .parse::<usize>()
                    .ok()
                    .filter(|&line| line > 0);
                located = Some(
                    line.ok_or("Line must be a positive number.")
                        .and_then(|line| Frame::resolve(repo, prefix, line)),
                );
                break;
            }
            if located.is_some() && report.frames.len() + report.issues.len() == 128 {
                report.error =
                    "Trace exceeds the 128 frame limit; shorten it and build again.".into();
                report.frames.clear();
                break;
            }
            match located {
                Some(Ok(frame)) => report.frames.push(frame),
                Some(Err(reason)) => report.issues.push(format!(
                    "Line {}: {reason} {}",
                    index + 1,
                    text.chars().take(240).collect::<String>()
                )),
                None => report.ignored += 1,
            }
        }
        report.format = "Text stack trace".into();
        if !report.frames.is_empty() {
            report.groups.push(Group {
                id: "stack".into(),
                label: "Stack trace".into(),
            });
        }
        if report.frames.is_empty() && report.error.is_empty() {
            report.error =
                "No matching frames. Paste file:line locations or a stack trace for this snapshot."
                    .into();
        }
        report
    }
}

#[derive(Resource, Default)]
pub(super) struct Trace {
    pub source: String,
    pub report: Report,
    pub version: u64,
    pub start: bool,
    pub selected: Option<String>,
    built_source: String,
    scope: Option<(PathBuf, String, Option<String>)>,
}

impl Trace {
    pub fn import(&mut self, repo: &Repository, source: String) {
        self.report = Report::parse(repo, &source);
        self.selected = (self.report.groups.len() == 1).then(|| self.report.groups[0].id.clone());
        self.built_source = source.clone();
        self.source = source;
        self.start = false;
        self.scope = Some((
            repo.root.clone(),
            repo.revision.clone(),
            repo.baseline.clone(),
        ));
        self.version = self.version.wrapping_add(1);
    }

    pub fn follow(&mut self, repo: &Repository) -> bool {
        if !self.matches(repo) {
            self.report = Report {
                error: "Repository changed; build the trace route again.".into(),
                ..default()
            };
            self.start = false;
        } else if self.source != self.built_source {
            self.report.error = "Build the route again after editing the trace.".into();
            self.start = false;
        } else {
            self.report = Report::parse(repo, &self.source);
            self.start = self.can_follow();
        }
        self.version = self.version.wrapping_add(1);
        self.start
    }

    pub fn select(&mut self, group: &str) {
        if self
            .report
            .groups
            .iter()
            .any(|candidate| candidate.id == group)
        {
            self.selected = Some(group.into());
            self.version = self.version.wrapping_add(1);
        }
    }

    pub fn frames(&self) -> impl Iterator<Item = &Frame> {
        self.report
            .frames
            .iter()
            .filter(|frame| self.selected.as_deref() == Some(frame.group()))
    }

    pub fn can_follow(&self) -> bool {
        self.source == self.built_source
            && self.report.error.is_empty()
            && self.frames().next().is_some()
    }

    fn matches(&self, repo: &Repository) -> bool {
        self.scope
            .as_ref()
            .is_some_and(|(root, revision, baseline)| {
                root == &repo.root && revision == &repo.revision && baseline == &repo.baseline
            })
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takeTraceRequest)]
    fn take_request() -> String;
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = traceReply)]
    fn reply(json: &str);
}

#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
enum Request {
    Import { text: String },
    Select { group: String },
    Follow,
}

pub(super) fn update(
    repo: Res<Repo>,
    mut trace: ResMut<Trace>,
    mut navigation: ResMut<city_navigation::Navigation>,
) {
    if repo.is_changed() {
        navigation.clear_trace_route();
        if !trace.matches(&repo.0) {
            *trace = Trace {
                version: trace.version.wrapping_add(1),
                ..default()
            };
        } else if !trace.source.is_empty() {
            let source = trace.source.clone();
            trace.import(&repo.0, source);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let input = take_request();
        if input.is_empty() {
            return;
        }
        let following = match serde_json::from_str::<Request>(&input) {
            Ok(Request::Import { text }) => {
                trace.import(&repo.0, text);
                false
            }
            Ok(Request::Follow) => trace.follow(&repo.0),
            Ok(Request::Select { group }) => {
                trace.select(&group);
                false
            }
            Err(_) => {
                trace.report = Report {
                    error: "Trace request is invalid.".into(),
                    ..default()
                };
                false
            }
        };
        reply(&serde_json::json!({ "report": trace.report, "selected": trace.selected, "following": following }).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn repo() -> Repository {
        let mut repo = repository_picker::empty();
        repo.files = [
            "lib/demo.rb",
            "src/main.rs",
            "src/app.js",
            "app/main.py",
            "a/Same.java",
            "b/Same.java",
        ]
        .map(|path| repo_world::File {
            path: path.into(),
            before: None,
            after: Some(repo_world::Blob {
                id: "blob".into(),
                size: 100,
                mode: "100644".into(),
            }),
            change: Change::Added,
            position: [0., 0.],
        })
        .into();
        repo
    }

    #[test]
    fn pasted_frames_keep_order_duplicates_and_original_line_in_common_trace_formats() {
        let report = Report::parse(
            &repo(),
            "RuntimeError: example\nlib/demo.rb:12:in `call'\n    at run (file:///project/src/app.js:42:9)\n  File \"/project/app/main.py\", line 17, in run\n   at src/main.rs:8:4\nlib/demo.rb:12\n",
        );
        assert_eq!(
            report
                .frames
                .iter()
                .map(|frame| (frame.path.as_str(), frame.line))
                .collect::<Vec<_>>(),
            [
                ("lib/demo.rb", 12),
                ("src/app.js", 42),
                ("app/main.py", 17),
                ("src/main.rs", 8),
                ("lib/demo.rb", 12)
            ]
        );
        assert_eq!(report.ignored, 1);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn missing_ambiguous_and_invalid_frames_are_reported_without_inventing_destinations() {
        let report = Report::parse(
            &repo(),
            "vendor/missing.rb:8\n at call(Same.java:20)\nsrc/main.rs:0\nsrc/main.rs:9\n",
        );
        assert_eq!(report.frames.len(), 1);
        assert_eq!(report.frames[0].line, 9);
        assert_eq!(report.issues.len(), 3);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("Ambiguous"))
        );
        assert!(report.issues.iter().any(|issue| issue.contains("positive")));
    }

    #[test]
    fn trace_limits_reject_partial_routes_and_follow_cannot_cross_snapshot_scope() {
        let mut repository = repo();
        assert!(
            !Report::parse(&repository, &"x".repeat(65537))
                .error
                .is_empty()
        );
        let too_many = Report::parse(&repository, &"src/main.rs:1\n".repeat(129));
        assert!(too_many.frames.is_empty());
        assert!(too_many.error.contains("128"));
        let mut trace = Trace::default();
        trace.import(&repository, "src/main.rs:1".into());
        assert!(trace.follow(&repository));
        repository.revision.push_str("-new");
        assert!(!trace.follow(&repository));
        assert!(trace.report.error.contains("Repository changed"));
    }

    #[test]
    fn windows_paths_and_javascript_columns_keep_the_line_instead_of_the_column() {
        let report = Report::parse(
            &repo(),
            "at run (C:\\project\\src\\app.js:42:9)\nfn@https://example.test/project/src/app.js:17:2\n",
        );
        assert_eq!(
            report
                .frames
                .iter()
                .map(|frame| frame.line)
                .collect::<Vec<_>>(),
            [42, 17]
        );
        assert!(report.frames.iter().all(|frame| frame.suffix));
    }
}
