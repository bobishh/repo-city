//! Read-only literal text search of the open native repository.
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use repo_world::Repository;

const PATHS_PER_COMMAND: usize = 128;

pub(crate) type Matches = HashMap<String, FileMatch>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileMatch {
    pub first_line: usize,
    pub count: usize,
}

pub(crate) fn search(repo: &Repository, query: &str) -> Result<Matches, String> {
    if query.trim().is_empty() {
        return Ok(Matches::new());
    }
    if query.contains(['\0', '\r', '\n']) {
        return Err("Enter one line of text to find.".into());
    }
    let mut matches = Matches::new();
    let result = if repo.is_worktree() {
        search_live(repo, query, worktree_search_tools(), &mut matches)
    } else {
        let mut command = Command::new("git");
        command
            .current_dir(&repo.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args([
                "grep",
                "-n",
                "-o",
                "-z",
                "-i",
                "-F",
                "-I",
                "-e",
                query,
                &repo.revision,
                "--",
            ]);
        read_matches(&mut command, Some(&repo.revision), &mut matches)
    };
    result
        .map(|()| matches)
        .map_err(|error| format!("Could not search the repository: {error}"))
}

fn worktree_search_tools() -> [PathBuf; 2] {
    let mut rg = PathBuf::from("rg");
    #[cfg(target_os = "macos")]
    if let Some(installed) = ["/opt/homebrew/bin/rg", "/usr/local/bin/rg"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
    {
        let path = std::env::var_os("PATH").unwrap_or_default();
        if !std::env::split_paths(&path).any(|dir| dir.join("rg").is_file()) {
            rg = installed;
        }
    }
    [rg, PathBuf::from("grep")]
}

fn search_live(
    repo: &Repository,
    query: &str,
    [rg, grep]: [PathBuf; 2],
    matches: &mut Matches,
) -> io::Result<()> {
    let root = repo.root.canonicalize()?;
    let paths = repo
        .files
        .iter()
        .filter_map(|file| {
            file.after
                .as_ref()
                .filter(|blob| blob.mode.starts_with("100"))?;
            let location = root.join(&file.path);
            let safe = location.symlink_metadata().is_ok_and(|meta| meta.is_file())
                && location
                    .canonicalize()
                    .is_ok_and(|location| location.starts_with(&root));
            safe.then(|| Path::new(".").join(&file.path))
        })
        .collect::<Vec<_>>();
    let mut ripgrep = true;
    for chunk in paths.chunks(PATHS_PER_COMMAND) {
        let mut command = Command::new(&rg);
        command
            .current_dir(&root)
            .args([
                "--no-config",
                "--threads",
                "1",
                "-n",
                "-o",
                "-i",
                "-F",
                "--null",
                "--with-filename",
                "--no-heading",
                "--color",
                "never",
                "--",
                query,
            ])
            .args(chunk);
        if ripgrep {
            match read_matches(&mut command, None, matches) {
                Ok(()) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => ripgrep = false,
                Err(error) => return Err(error),
            }
        }
        let mut command = Command::new(&grep);
        command
            .current_dir(&root)
            .args(["-inHo", "-F", "-I", "--null", "--", query])
            .args(chunk);
        match read_matches(&mut command, None, matches) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                *matches = search_loaded_files(repo, query);
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn search_loaded_files(repo: &Repository, query: &str) -> Matches {
    let query = query.to_lowercase();
    let mut matches = Matches::new();
    for (index, file) in repo.files.iter().enumerate() {
        let Ok(source) = repo.source(index, false) else {
            continue;
        };
        for (line, text) in source.lines().enumerate() {
            let count = text.to_lowercase().matches(&query).count();
            if count > 0 {
                record(&mut matches, &file.path, line + 1, count);
            }
        }
    }
    matches
}

fn read_matches(
    command: &mut Command,
    revision: Option<&str>,
    matches: &mut Matches,
) -> io::Result<()> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let output = child.stdout.take().expect("search stdout is piped");
    let result = parse_matches(BufReader::new(output), revision, matches);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    if status.success() || status.code() == Some(1) {
        result
    } else {
        Err(io::Error::other(format!(
            "{} exited with {status}",
            command.get_program().to_string_lossy()
        )))
    }
}

fn parse_matches(
    mut output: impl BufRead,
    revision: Option<&str>,
    matches: &mut Matches,
) -> io::Result<()> {
    let prefix = revision.map_or_else(|| "./".into(), |revision| format!("{revision}:"));
    let line_delimiter = if revision.is_some() { 0 } else { b':' };
    let mut path = Vec::new();
    let mut body = Vec::new();
    while output.read_until(0, &mut path)? > 0 {
        let path_text = String::from_utf8_lossy(path.strip_suffix(&[0]).unwrap_or(&path));
        #[cfg(windows)]
        let path_text = path_text.replace('\\', "/");
        if output.read_until(b'\n', &mut body)? > 0
            && let Some(line) = body.split(|byte| *byte == line_delimiter).next()
            && let Ok(line) = str::from_utf8(line)
            && let Ok(line) = line.parse::<usize>()
        {
            let path = path_text.strip_prefix(&prefix).unwrap_or(&path_text);
            record(matches, path, line, 1);
        }
        path.clear();
        body.clear();
    }
    Ok(())
}

fn record(matches: &mut Matches, path: &str, line: usize, count: usize) {
    if let Some(found) = matches.get_mut(path) {
        found.first_line = found.first_line.min(line);
        found.count += count;
    } else {
        matches.insert(
            path.to_owned(),
            FileMatch {
                first_line: line,
                count,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_search_uses_the_live_files_or_the_open_commit() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let fixture = Fixture(
            std::env::temp_dir().join(format!("repo-city-text-search-{}", std::process::id())),
        );
        std::fs::create_dir_all(fixture.0.join("nested")).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(&fixture.0)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--quiet"]);
        let path = "nested/file with spaces.rs";
        std::fs::write(fixture.0.join(path), "first\n// NEEDLE: commit needle:\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "snapshot"]);
        let committed = Repository::open(&fixture.0, "HEAD").unwrap();
        std::fs::write(fixture.0.join(path), "first\nsecond\n// Needle: live\n").unwrap();
        let live = Repository::open_worktree(&fixture.0).unwrap();
        let expected = FileMatch {
            first_line: 3,
            count: 1,
        };
        assert_eq!(
            search(&committed, "needle:").unwrap()[path],
            FileMatch {
                first_line: 2,
                count: 2
            }
        );
        assert_eq!(search(&live, "needle:").unwrap()[path], expected);
        let tools = || [fixture.0.join("missing-rg"), PathBuf::from("grep")];
        let mut fallback = Matches::new();
        search_live(&live, "needle:", tools(), &mut fallback).unwrap();
        assert_eq!(fallback[path], expected);
        assert!(search(&live, "needle.*").unwrap().is_empty());

        let many = "// needle needle\n".repeat(150);
        std::fs::write(fixture.0.join(path), many).unwrap();
        let live = Repository::open_worktree(&fixture.0).unwrap();
        let expected = Matches::from([(
            path.into(),
            FileMatch {
                first_line: 1,
                count: 300,
            },
        )]);
        assert_eq!(search(&live, "needle").unwrap(), expected);
        let mut fallback = Matches::new();
        search_live(&live, "needle", tools(), &mut fallback).unwrap();
        assert_eq!(fallback, expected);
    }
}
