//! Read-only literal text search of the open native repository.
use std::{
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use repo_world::Repository;

const LIMIT: usize = 100;
const PATHS_PER_COMMAND: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Match {
    pub path: String,
    pub line: usize,
    pub text: String,
}

pub(crate) fn search(repo: &Repository, query: &str) -> Result<Vec<Match>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    if query.contains(['\0', '\r', '\n']) {
        return Err("Enter one line of text to find.".into());
    }
    let result = if repo.is_worktree() {
        search_live(repo, query, worktree_search_tools())
    } else {
        let mut command = Command::new("git");
        command
            .current_dir(&repo.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args([
                "grep",
                "-n",
                "-z",
                "-i",
                "-F",
                "-I",
                "-e",
                query,
                &repo.revision,
                "--",
            ]);
        read_matches(&mut command, Some(&repo.revision), LIMIT)
    };
    result.map_err(|error| format!("Could not search the repository: {error}"))
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

fn search_live(repo: &Repository, query: &str, [rg, grep]: [PathBuf; 2]) -> io::Result<Vec<Match>> {
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
    let mut matches = Vec::new();
    let mut ripgrep = true;
    for chunk in paths.chunks(PATHS_PER_COMMAND) {
        let remaining = LIMIT - matches.len();
        let mut command = Command::new(&rg);
        command
            .current_dir(&root)
            .args([
                "--no-config",
                "--threads",
                "1",
                "-n",
                "-i",
                "-F",
                "--null",
                "--with-filename",
                "--no-heading",
                "--color",
                "never",
                "--max-columns",
                "240",
                "--max-columns-preview",
                "--",
                query,
            ])
            .args(chunk);
        if ripgrep {
            match read_matches(&mut command, None, remaining) {
                Ok(found) => matches.extend(found),
                Err(error) if error.kind() == io::ErrorKind::NotFound => ripgrep = false,
                Err(error) => return Err(error),
            }
        }
        if !ripgrep {
            let mut command = Command::new(&grep);
            command
                .current_dir(&root)
                .args(["-inH", "-F", "-I", "--null", "--", query])
                .args(chunk);
            match read_matches(&mut command, None, remaining) {
                Ok(found) => matches.extend(found),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(search_loaded_files(repo, query));
                }
                Err(error) => return Err(error),
            }
        }
        if matches.len() == LIMIT {
            break;
        }
    }
    Ok(matches)
}

fn search_loaded_files(repo: &Repository, query: &str) -> Vec<Match> {
    let query = query.to_lowercase();
    let mut matches = Vec::new();
    for (index, file) in repo.files.iter().enumerate() {
        let Ok(source) = repo.source(index, false) else {
            continue;
        };
        for (line, text) in source.lines().enumerate() {
            if text.to_lowercase().contains(&query) {
                matches.push(Match {
                    path: file.path.clone(),
                    line: line + 1,
                    text: preview(text),
                });
                if matches.len() == LIMIT {
                    return matches;
                }
            }
        }
    }
    matches
}

fn read_matches(
    command: &mut Command,
    revision: Option<&str>,
    limit: usize,
) -> io::Result<Vec<Match>> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let output = child.stdout.take().expect("search stdout is piped");
    let result = parse_matches(BufReader::new(output), revision, limit);
    let limited = result.as_ref().is_ok_and(|matches| matches.len() == limit);
    if limited || result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    if limited || status.success() || status.code() == Some(1) {
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
    limit: usize,
) -> io::Result<Vec<Match>> {
    let prefix = revision.map_or_else(|| "./".into(), |revision| format!("{revision}:"));
    let line_delimiter = if revision.is_some() { '\0' } else { ':' };
    let mut path = Vec::new();
    let mut body = Vec::new();
    let mut matches = Vec::new();
    while matches.len() < limit && output.read_until(0, &mut path)? > 0 {
        let path_text = String::from_utf8_lossy(path.strip_suffix(&[0]).unwrap_or(&path));
        if output.read_until(b'\n', &mut body)? > 0
            && let Some((line, text)) = String::from_utf8_lossy(&body).split_once(line_delimiter)
            && let Ok(line) = line.parse::<usize>()
        {
            let path = path_text.strip_prefix(&prefix).unwrap_or(&path_text);
            matches.push(Match {
                path: if cfg!(windows) {
                    path.replace('\\', "/")
                } else {
                    path.to_owned()
                },
                line,
                text: preview(text),
            });
        }
        path.clear();
        body.clear();
    }
    Ok(matches)
}

fn preview(text: &str) -> String {
    text.trim().chars().take(240).collect()
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
        std::fs::write(fixture.0.join(path), "first\n// NEEDLE: commit\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "snapshot"]);
        let committed = Repository::open(&fixture.0, "HEAD").unwrap();
        std::fs::write(fixture.0.join(path), "first\nsecond\n// Needle: live\n").unwrap();
        let live = Repository::open_worktree(&fixture.0).unwrap();
        let expected = Match {
            path: path.into(),
            line: 3,
            text: "// Needle: live".into(),
        };
        assert_eq!(
            search(&committed, "needle:").unwrap(),
            [Match {
                path: path.into(),
                line: 2,
                text: "// NEEDLE: commit".into()
            }]
        );
        assert_eq!(
            search(&live, "needle:").unwrap(),
            [Match {
                path: path.into(),
                line: 3,
                text: "// Needle: live".into()
            }]
        );
        assert_eq!(
            search_live(
                &live,
                "needle:",
                [fixture.0.join("missing-rg"), PathBuf::from("grep")]
            )
            .unwrap(),
            [expected]
        );
        assert!(search(&live, "needle.*").unwrap().is_empty());
    }
}
