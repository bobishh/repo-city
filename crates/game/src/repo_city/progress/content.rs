//! Saved read credit belongs to exact bytes, not a worktree's stable revision label.
use super::*;
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
pub(super) struct StoredMark {
    #[serde(flatten)]
    pub(super) mark: Mark,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<ReadSource>,
}

#[derive(Serialize, Deserialize)]
struct ReadSource {
    blob: String,
    // Only the read interval is stored. No unobserved file contents are persisted.
    lines: Vec<String>,
}

#[derive(Default)]
pub(super) struct Sources(HashMap<(usize, bool), Option<Vec<String>>>);

impl Sources {
    fn lines(&mut self, repo: &Repository, index: usize, before: bool) -> Option<&[String]> {
        self.0
            .entry((index, before))
            .or_insert_with(|| {
                repo.source(index, before)
                    .ok()
                    .map(|source| source.lines().map(str::to_owned).collect())
            })
            .as_deref()
    }
}

impl Mark {
    fn interval(&self) -> Option<(&str, bool, std::ops::Range<usize>, usize)> {
        match self {
            Self::Reveal {
                path,
                before,
                start,
                end,
                rows,
                ..
            } => Some((path, *before, *start..*end, *rows)),
            Self::ReadDocument(document) => Some((
                &document.path,
                document.before,
                0..document.rows,
                document.rows,
            )),
            _ => None,
        }
    }

    fn read_blob(&self) -> Option<&str> {
        match self {
            Self::Reveal { blob, .. } => blob.as_deref(),
            Self::ReadDocument(document) => Some(&document.blob),
            _ => None,
        }
    }
}

fn blob(file: &repo_world::File, before: bool) -> Option<&repo_world::Blob> {
    if before {
        file.before.as_ref()
    } else {
        file.after.as_ref()
    }
}

impl StoredMark {
    fn bind(mark: Mark, repo: &Repository, sources: &mut Sources) -> Self {
        let source = mark.interval().and_then(|(path, before, range, rows)| {
            let (index, file) = repo
                .files
                .iter()
                .enumerate()
                .find(|(_, file)| file.path == path)?;
            let blob = blob(file, before)?;
            if mark.read_blob()? != blob.id {
                return None;
            }
            let lines = sources.lines(repo, index, before)?;
            if range.is_empty() || rows != lines.len() {
                return None;
            }
            Some(ReadSource {
                blob: blob.id.clone(),
                lines: lines.get(range)?.to_vec(),
            })
        });
        Self { mark, source }
    }

    pub(super) fn reveals(
        &self,
        repo: &Repository,
        index: usize,
        sources: &mut Sources,
    ) -> Vec<(std::ops::Range<usize>, usize)> {
        let Some((path, before, range, rows)) = self.mark.interval() else {
            return Vec::new();
        };
        let Some(file) = repo.files.get(index).filter(|file| file.path == path) else {
            return Vec::new();
        };
        let Some(blob) = blob(file, before) else {
            return Vec::new();
        };
        if range.is_empty() || range.end > rows || rows > 1024 * 1024 {
            return Vec::new();
        }
        let Some(source) = &self.source else {
            // Old immutable comparisons remain safe. A live label cannot prove old bytes.
            return if !repo.is_worktree() {
                vec![(range, rows)]
            } else {
                Vec::new()
            };
        };
        if source.lines.len() != range.len() {
            return Vec::new();
        }
        if source.blob == blob.id {
            return vec![(range, rows)];
        }
        let Some(lines) = sources.lines(repo, index, before) else {
            return Vec::new();
        };
        unique_matches(&source.lines, lines)
            .into_iter()
            .map(|row| (row..row + 1, lines.len()))
            .collect()
    }
}

fn unique_matches(previous: &[String], current: &[String]) -> Vec<usize> {
    let mut positions = HashMap::new();
    for (row, line) in current.iter().enumerate() {
        positions
            .entry(line.as_str())
            .and_modify(|position| *position = None)
            .or_insert(Some(row));
    }
    let mut matches = previous
        .iter()
        .filter_map(|line| positions.get(line.as_str()).copied().flatten())
        .collect::<Vec<_>>();
    matches.sort_unstable();
    matches.dedup();
    matches
}

pub(super) fn bind_changes(repo: &Repository, changes: Vec<(String, bool)>) -> Vec<(String, bool)> {
    let mut sources = Sources::default();
    changes
        .into_iter()
        .filter_map(|(json, checked)| {
            let mark = serde_json::from_str(&json).ok()?;
            if let Mark::ReadDocument(document) = &mark
                && repo.files.iter().any(|file| {
                    document.matches(file)
                        && blob(file, document.before)
                            .is_some_and(|blob| blob.size > markdown_preview::PAGE_BYTES)
                })
            {
                // The immutable blob identifies a fully read document. Avoid storing a copy of it.
                return Some((
                    serde_json::to_string(&StoredMark { mark, source: None }).ok()?,
                    checked,
                ));
            }
            let stored = StoredMark::bind(mark, repo, &mut sources);
            // Queued credit cannot move to bytes that were not present when it was earned.
            if stored.mark.interval().is_some() && stored.source.is_none() {
                return None;
            }
            Some((serde_json::to_string(&stored).ok()?, checked))
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn can_carry(json: &str) -> bool {
    let Ok(saved) = serde_json::from_str::<StoredMark>(json) else {
        return false;
    };
    if let Mark::ReadDocument(document) = &saved.mark
        && saved.source.is_none()
    {
        return !document.blob.is_empty() && document.rows > 0;
    }
    let Some((_, _, range, rows)) = saved.mark.interval() else {
        return false;
    };
    let Some(source) = saved.source else {
        return false;
    };
    !source.blob.is_empty()
        && !range.is_empty()
        && range.end <= rows
        && rows <= 1024 * 1024
        && source.lines.len() == range.len()
        && source.lines.iter().map(String::len).sum::<usize>() <= 1024 * 1024
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queued_document_cannot_certify_a_newer_blob_with_the_same_line_count() {
        let repo = Repository {
            root: "/unused".into(),
            revision: "worktree:head".into(),
            baseline: Some("head".into()),
            files: vec![repo_world::File {
                path: "guide.md".into(),
                before: None,
                after: Some(repo_world::Blob {
                    id: "new".into(),
                    size: 4,
                    mode: "100644".into(),
                }),
                change: Change::Modified,
                position: [0., 0.],
            }],
            districts: Vec::new(),
        };
        let mut sources = Sources::default();
        sources.0.insert((0, false), Some(vec!["new".into()]));
        let queued = Mark::ReadDocument(markdown_preview::ReadDocument {
            path: "guide.md".into(),
            blob: "already-read-old-blob".into(),
            before: false,
            rows: 1,
        });
        assert!(
            StoredMark::bind(queued, &repo, &mut sources)
                .source
                .is_none()
        );
        for blob in [Some("already-read-old-blob".into()), None] {
            let queued = Mark::Reveal {
                path: "guide.md".into(),
                before: false,
                blob,
                start: 0,
                end: 1,
                rows: 1,
            };
            assert!(
                StoredMark::bind(queued, &repo, &mut sources)
                    .source
                    .is_none()
            );
        }
        let current = Mark::Reveal {
            path: "guide.md".into(),
            before: false,
            blob: Some("new".into()),
            start: 0,
            end: 1,
            rows: 1,
        };
        assert!(
            StoredMark::bind(current, &repo, &mut sources)
                .source
                .is_some()
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn saved_worktree_credit_preserves_only_read_unchanged_bytes_after_an_edit() {
        use sha2::{Digest, Sha256};
        let root =
            std::env::temp_dir().join(format!("repo-city-read-content-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let original = "fn read_before() {}\nfn rewritten() {}\nfn unread() {}\n";
        let updated = "// inserted\nfn read_before() {}\nfn changed() {}\nfn unread() {}\n";
        let blob = |source: &str| repo_world::Blob {
            id: format!("worktree:{:x}", Sha256::digest(source.as_bytes())),
            size: source.len(),
            mode: "100644".into(),
        };
        std::fs::write(root.join("code.rs"), original).unwrap();
        let mut repo = Repository {
            root: root.clone(),
            revision: "worktree:head".into(),
            baseline: Some("head".into()),
            files: vec![repo_world::File {
                path: "code.rs".into(),
                before: None,
                after: Some(blob(original)),
                change: Change::Modified,
                position: [0., 0.],
            }],
            districts: Vec::new(),
        };
        let mark = Mark::Reveal {
            path: "code.rs".into(),
            before: false,
            blob: repo.files[0].after.as_ref().map(|blob| blob.id.clone()),
            start: 0,
            end: 2,
            rows: 3,
        };
        let changes = bind_changes(&repo, vec![(serde_json::to_string(&mark).unwrap(), true)]);
        let saved: StoredMark = serde_json::from_str(&changes[0].0).unwrap();
        assert_eq!(
            saved.source.as_ref().unwrap().lines,
            ["fn read_before() {}", "fn rewritten() {}"]
        );
        assert_eq!(
            saved.reveals(&repo, 0, &mut Sources::default()),
            [(0..2, 3)]
        );
        std::fs::write(root.join("code.rs"), updated).unwrap();
        repo.files[0].after = Some(blob(updated));
        assert_eq!(
            saved.reveals(&repo, 0, &mut Sources::default()),
            [(1..2, 4)]
        );
        std::fs::remove_file(root.join("code.rs")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn unchanged_unique_lines_survive_insertions_but_rewrites_and_duplicates_do_not() {
        let lines = |items: &[&str]| {
            items
                .iter()
                .map(|line| (*line).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            unique_matches(
                &lines(&["fn original() {", "    do_work();", "}"]),
                &lines(&[
                    "// inserted",
                    "fn original() {",
                    "    changed_work();",
                    "}",
                    "}"
                ]),
            ),
            [1]
        );
    }

    #[test]
    fn saved_envelope_round_trips_and_legacy_marks_remain_parseable() {
        let saved = r#"{"reveal":{"path":"code.rs","before":false,"start":0,"end":1,"rows":1},"source":{"blob":"blob","lines":["fn code() {}"]}}"#;
        let stored: StoredMark = serde_json::from_str(saved).unwrap();
        assert_eq!(stored.source.as_ref().unwrap().blob, "blob");
        let restored: StoredMark =
            serde_json::from_str(&serde_json::to_string(&stored).unwrap()).unwrap();
        assert_eq!(restored.source.unwrap().lines, ["fn code() {}"]);
        let legacy: StoredMark = serde_json::from_str(r#"{"reviewed":"code.rs"}"#).unwrap();
        assert!(matches!(legacy.mark, Mark::Reviewed(path) if path == "code.rs"));
    }

    #[test]
    fn stored_reveal_restores_only_its_side_and_exact_blob_without_loading_source() {
        let file = repo_world::File {
            path: "code.rs".into(),
            before: Some(repo_world::Blob {
                id: "old".into(),
                size: 4,
                mode: "100644".into(),
            }),
            after: Some(repo_world::Blob {
                id: "new".into(),
                size: 4,
                mode: "100644".into(),
            }),
            change: Change::Modified,
            position: [0., 0.],
        };
        let repo = Repository {
            root: "/unused".into(),
            revision: "worktree:head".into(),
            baseline: Some("head".into()),
            files: vec![file],
            districts: Vec::new(),
        };
        let mut stored = StoredMark {
            mark: Mark::Reveal {
                path: "code.rs".into(),
                before: false,
                blob: None,
                start: 0,
                end: 1,
                rows: 1,
            },
            source: Some(ReadSource {
                blob: "new".into(),
                lines: vec!["new".into()],
            }),
        };
        let mut sources = Sources::default();
        assert_eq!(stored.reveals(&repo, 0, &mut sources), [(0..1, 1)]);
        sources.0.insert((0, false), Some(vec!["rewritten".into()]));
        stored.source.as_mut().unwrap().blob = "previous".into();
        assert!(stored.reveals(&repo, 0, &mut sources).is_empty());
        sources
            .0
            .insert((0, false), Some(vec!["inserted".into(), "new".into()]));
        assert_eq!(stored.reveals(&repo, 0, &mut sources), [(1..2, 2)]);
        stored.mark = Mark::ReadDocument(markdown_preview::ReadDocument {
            path: "code.rs".into(),
            blob: "previous".into(),
            before: false,
            rows: 1,
        });
        assert_eq!(stored.reveals(&repo, 0, &mut sources), [(1..2, 2)]);
        stored.source.as_mut().unwrap().lines.clear();
        assert!(stored.reveals(&repo, 0, &mut sources).is_empty());
    }
}
