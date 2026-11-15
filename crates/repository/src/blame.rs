//! Attribute lines to the commit that last changed them in the immutable snapshot.
use std::path::Path;

pub fn lines(root: &Path, revision: &str, path: &str) -> Result<Vec<String>, String> {
    let output = super::git(
        root,
        &[
            "--literal-pathspecs",
            "blame",
            "--line-porcelain",
            revision,
            "--",
            path,
        ],
    )?;
    let text = String::from_utf8_lossy(&output);
    let mut commit = "";
    let mut author = "";
    let mut result = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("author ") {
            author = name;
        } else if line.starts_with('\t') {
            if commit.is_empty() || author.is_empty() {
                return Err("Git returned incomplete blame data.".into());
            }
            result.push(format!("{commit}  {author}"));
            commit = "";
            author = "";
        } else if let Some((hash, rest)) = line.split_once(' ')
            && matches!(hash.len(), 40 | 64)
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && rest
                .split_whitespace()
                .take(2)
                .all(|field| field.parse::<usize>().is_ok())
        {
            commit = &hash[..9];
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blame_attributes_lines_to_commits_from_the_requested_snapshot() {
        let temp =
            std::env::temp_dir().join(format!("repo-city-blame-test-{}", std::process::id()));
        std::fs::create_dir(&temp).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(temp.clone());
        let git = |args: &[&str]| super::super::git(&temp, args).unwrap();
        git(&["init", "-q"]);
        let path = "report [draft].rs";
        std::fs::write(temp.join(path), "let one = 1;\nlet two = 2;\n").unwrap();
        git(&["add", "--", path]);
        git(&[
            "-c",
            "user.name=Ada",
            "-c",
            "user.email=a@example.org",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "Start",
        ]);
        let first = String::from_utf8(git(&["rev-parse", "HEAD"])).unwrap();
        std::fs::write(temp.join(path), "let one = 1;\nlet two = 3;\n").unwrap();
        git(&["add", "--", path]);
        git(&[
            "-c",
            "user.name=Lin",
            "-c",
            "user.email=l@example.org",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "Change line two",
        ]);
        let second = String::from_utf8(git(&["rev-parse", "HEAD"])).unwrap();
        std::fs::write(temp.join(path), "Uncommitted").unwrap();
        assert_eq!(
            lines(&temp, second.trim(), path).unwrap(),
            [
                format!("{}  Ada", &first[..9]),
                format!("{}  Lin", &second[..9])
            ]
        );
        assert!(
            lines(&temp, first.trim(), path)
                .unwrap()
                .iter()
                .all(|line| line.ends_with("  Ada"))
        );
    }
}
