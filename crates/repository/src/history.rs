//! Read-only local commit history for the desktop tablet.
use std::{
    io::{self, Read},
    path::Path,
    process::{Command, Stdio},
};

#[derive(Clone, Debug)]
pub struct Commit {
    pub id: String,
    pub date: String,
    pub author: String,
    pub summary: String,
}

pub fn commits(root: &Path, revision: &str, skip: usize) -> Result<Vec<Commit>, String> {
    let revision = if revision.starts_with("worktree:") {
        "HEAD"
    } else {
        revision
    };
    if revision != "HEAD" && !valid_id(revision) {
        return Err("Invalid history revision.".into());
    }
    let data = read(
        root,
        &[
            "log",
            "-n",
            "100",
            &format!("--skip={skip}"),
            "--format=%H%x00%aI%x00%an%x00%s%x00",
            revision,
            "--",
        ],
    )?;
    let fields = data.split('\0').collect::<Vec<_>>();
    Ok(fields
        .chunks_exact(4)
        .filter_map(|row| {
            let id = row[0].trim();
            valid_id(id).then(|| Commit {
                id: id.into(),
                date: row[1].into(),
                author: row[2].into(),
                summary: row[3].into(),
            })
        })
        .collect())
}

pub fn show(root: &Path, id: &str) -> Result<String, String> {
    if !valid_id(id) {
        return Err("Invalid commit ID.".into());
    }
    read(
        root,
        &[
            "show",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--format=fuller",
            "--stat",
            "--patch",
            id,
            "--",
        ],
    )
}

fn valid_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read(root: &Path, args: &[&str]) -> Result<String, String> {
    const LIMIT: usize = 2 * 1024 * 1024;
    let mut child = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(command_error)?;
    let mut bytes = Vec::new();
    let result = child
        .stdout
        .take()
        .expect("piped stdout")
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes);
    let truncated = bytes.len() > LIMIT;
    if result.is_err() || truncated {
        let _ = child.kill();
    }
    let status = child.wait().map_err(command_error)?;
    result.map_err(command_error)?;
    if !status.success() && !truncated {
        return Err(
            "Git could not read this revision. Check the checkout and commit in the terminal."
                .into(),
        );
    }
    bytes.truncate(LIMIT);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str(
            "\n\n[Output truncated at 2 MiB. Use the terminal to inspect the complete commit.]\n",
        );
    }
    Ok(text)
}

pub(crate) fn command_error(error: io::Error) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        "Git is not installed or is not on PATH. Install Git, then reopen this repository. Windows: git-scm.com. Linux: install git with your package manager. macOS: install Git or the command-line developer tools.".into()
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    #[test]
    fn log_and_show_read_commits_without_changing_the_checkout() {
        let root = std::env::temp_dir().join(format!(
            "vibepad-history-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        git(&["init", "-q"]);
        for value in ["first", "second"] {
            fs::write(root.join("notes.txt"), value).unwrap();
            git(&["add", "notes.txt"]);
            git(&[
                "-c",
                "user.name=History Test",
                "-c",
                "user.email=history@example.invalid",
                "commit",
                "-qm",
                value,
            ]);
        }
        let head = git(&["rev-parse", "HEAD"]);
        let history = commits(&root, "HEAD", 0).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].summary, "second");
        assert_eq!(history[1].author, "History Test");
        let detail = show(&root, &history[1].id).unwrap();
        assert!(detail.contains("+first"));
        assert!(!detail.contains("+second"));
        assert_eq!(git(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).unwrap(),
            "second"
        );
        assert!(show(&root, "--output=bad").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn absent_git_reports_an_installation_instruction() {
        assert!(command_error(io::Error::from(io::ErrorKind::NotFound)).contains("Install Git"));
    }
}
