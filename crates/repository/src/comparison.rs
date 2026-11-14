//! Git computes hunks; aligned rows preserve original line numbers on both sides.
use crate::syntax::{self, Line};
use std::ops::Range;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mark {
    #[default]
    Context,
    Added,
    Removed,
}
#[derive(Clone, Debug)]
pub struct Row {
    pub before: Option<usize>,
    pub after: Option<usize>,
    pub changed: bool,
}
#[derive(Clone, Debug)]
pub struct Hunk {
    pub before: Range<usize>,
    pub after: Range<usize>,
    pub row: usize,
}
#[derive(Clone, Debug)]
pub struct Comparison {
    pub before: Vec<Line>,
    pub after: Vec<Line>,
    pub rows: Vec<Row>,
    pub hunks: Vec<Hunk>,
    pub final_newline_changed: bool,
}
#[cfg(not(target_arch = "wasm32"))]
struct Scratch(std::path::PathBuf);
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[cfg(not(target_arch = "wasm32"))]
static NEXT: AtomicUsize = AtomicUsize::new(0);
#[cfg(not(target_arch = "wasm32"))]
fn range(text: &str) -> Result<Range<usize>, String> {
    let (start, count) = text[1..].split_once(',').unwrap_or((&text[1..], "1"));
    let start: usize = start.parse().map_err(|_| "Invalid diff start")?;
    let count: usize = count.parse().map_err(|_| "Invalid diff count")?;
    let first = if count == 0 {
        start
    } else {
        start.saturating_sub(1)
    };
    Ok(first..first + count)
}
#[cfg(not(target_arch = "wasm32"))]
fn changes(before: &str, after: &str) -> Result<Vec<(Range<usize>, Range<usize>)>, String> {
    let mut directory = None;
    for _ in 0..100 {
        let candidate = std::env::temp_dir().join(format!(
            "repo-city-diff-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                directory = Some(Scratch(candidate));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    let directory = directory.ok_or("Cannot allocate diff directory")?;
    std::fs::write(directory.0.join("before"), before).map_err(|e| e.to_string())?;
    std::fs::write(directory.0.join("after"), after).map_err(|e| e.to_string())?;
    let output = Command::new("git")
        .current_dir(&directory.0)
        .args([
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--text",
            "--unified=0",
            "--",
            "before",
            "after",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| l.starts_with("@@ "))
        .map(|header| {
            let fields: Vec<_> = header.split_whitespace().collect();
            Ok((
                range(fields.get(1).ok_or("Missing diff range")?)?,
                range(fields.get(2).ok_or("Missing diff range")?)?,
            ))
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
fn changes(before: &str, after: &str) -> Result<Vec<(Range<usize>, Range<usize>)>, String> {
    let diff = similar::TextDiff::configure()
        .algorithm(similar::Algorithm::Patience)
        .diff_lines(before, after);
    Ok(diff
        .ops()
        .iter()
        .filter(|op| op.tag() != similar::DiffTag::Equal)
        .map(|op| (op.old_range(), op.new_range()))
        .collect())
}

impl Comparison {
    pub fn new(path: &str, before: &str, after: &str) -> Result<Self, String> {
        let mut result = Self {
            before: syntax::highlight(path, before),
            after: syntax::highlight(path, after),
            rows: Vec::new(),
            hunks: Vec::new(),
            final_newline_changed: before.ends_with('\n') != after.ends_with('\n'),
        };
        let (mut b, mut a) = (0, 0);
        for (old, new) in changes(before, after)? {
            if old.end > result.before.len()
                || new.end > result.after.len()
                || old.start < b
                || new.start < a
            {
                return Err("Diff ranges do not match source".into());
            }
            while b < old.start || a < new.start {
                result.rows.push(Row {
                    before: (b < old.start).then_some(b),
                    after: (a < new.start).then_some(a),
                    changed: false,
                });
                if b < old.start {
                    b += 1;
                }
                if a < new.start {
                    a += 1;
                }
            }
            result.hunks.push(Hunk {
                before: old.clone(),
                after: new.clone(),
                row: result.rows.len(),
            });
            for i in 0..old.len().max(new.len()) {
                result.rows.push(Row {
                    before: (i < old.len()).then_some(old.start + i),
                    after: (i < new.len()).then_some(new.start + i),
                    changed: true,
                });
            }
            b = old.end;
            a = new.end;
        }
        while b < result.before.len() || a < result.after.len() {
            result.rows.push(Row {
                before: (b < result.before.len()).then_some(b),
                after: (a < result.after.len()).then_some(a),
                changed: false,
            });
            b += 1;
            a += 1;
        }
        Ok(result)
    }
    pub fn page(&self, before: bool, offset: usize, count: usize) -> Vec<(Option<Line>, Mark)> {
        self.rows
            .iter()
            .skip(offset)
            .take(count)
            .map(|row| {
                let (index, lines) = if before {
                    (row.before, &self.before)
                } else {
                    (row.after, &self.after)
                };
                let mark = if !row.changed || index.is_none() {
                    Mark::Context
                } else if before {
                    Mark::Removed
                } else {
                    Mark::Added
                };
                (index.map(|i| lines[i].clone()), mark)
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn check(old: &str, new: &str) -> Comparison {
        let c = Comparison::new("test.rs", old, new).unwrap();
        assert_eq!(
            c.rows.iter().filter_map(|r| r.before).collect::<Vec<_>>(),
            (0..c.before.len()).collect::<Vec<_>>()
        );
        assert_eq!(
            c.rows.iter().filter_map(|r| r.after).collect::<Vec<_>>(),
            (0..c.after.len()).collect::<Vec<_>>()
        );
        for r in &c.rows {
            if !r.changed {
                assert_eq!(
                    r.before.map(|i| &c.before[i].text),
                    r.after.map(|i| &c.after[i].text)
                );
            }
        }
        for side in [true, false] {
            for (line, mark) in c.page(side, 0, c.rows.len()) {
                if line.is_none() {
                    assert_eq!(
                        mark,
                        Mark::Context,
                        "alignment gaps are not deleted/added source"
                    );
                }
            }
        }
        c
    }
    #[test]
    fn insert_delete_replace_and_empty_snapshots_align() {
        for (a, b) in [
            ("", "one\ntwo\n"),
            ("one\n", ""),
            ("same\nold\nend\n", "same\nnew\nextra\nend\n"),
            ("α\nb\nc\n", "start\nα\nc\n"),
            ("x", "x\n"),
            ("", ""),
        ] {
            check(a, b);
        }
    }
    #[test]
    fn large_file_keeps_distant_hunks_separate() {
        let old = (0..9000).map(|i| format!("line {i}\n")).collect::<String>();
        let new = old
            .replace("line 12\n", "changed twelve\n")
            .replace("line 8500\n", "changed far away\n");
        let c = check(&old, &new);
        assert_eq!(c.hunks.len(), 2);
        assert!(c.hunks[1].row > 8000);
    }
}
