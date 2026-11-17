//! The working directory as a read-only snapshot against its current HEAD.
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Blob, Repository, git, resolve_revision, tree};

const TEXT_LIMIT: usize = 1024 * 1024;

impl Repository {
    pub fn open_worktree(folder: &Path) -> Result<Self, String> {
        let root = git(folder, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(
            String::from_utf8(root)
                .map_err(|error| error.to_string())?
                .trim_end_matches('\n'),
        );
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let head = resolve_revision(&root, "HEAD")?;
        let before = tree(&root, &head)?;
        let status = git(
            &root,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--no-renames",
            ],
        )?;
        let mut after = before.clone();
        for entry in status
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let path = str::from_utf8(entry.get(3..).ok_or("Malformed Git status entry")?)
                .map_err(|error| error.to_string())?;
            match disk_blob(&root, path, before.get(path))? {
                Some(blob) => {
                    after.insert(path.to_owned(), blob);
                }
                None => {
                    after.remove(path);
                }
            }
        }
        Repository::from_trees(root, format!("worktree:{head}"), Some(head), before, after)
    }
}

fn path_in_root(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Invalid worktree path".into());
    }
    let path = root.join(path);
    let parent = path.parent().ok_or("Invalid worktree parent")?;
    match parent.canonicalize() {
        Ok(parent) if !parent.starts_with(root) => {
            Err("Worktree path leaves the repository".into())
        }
        Ok(_) => Ok(path),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            Ok(path)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn disk_blob(root: &Path, path: &str, baseline: Option<&Blob>) -> Result<Option<Blob>, String> {
    let file = path_in_root(root, path)?;
    let metadata = match fs::symlink_metadata(&file) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(None);
        }
        Err(error) => return Err(error.to_string()),
    };
    let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    let (mode, content) = if metadata.is_symlink() {
        (
            "120000",
            fs::read_link(&file)
                .map_err(|error| error.to_string())?
                .as_os_str()
                .as_encoded_bytes()
                .to_vec(),
        )
    } else if metadata.is_file() {
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 {
                "100755"
            } else {
                "100644"
            }
        };
        #[cfg(not(unix))]
        let mode = baseline.map_or("100644", |blob| blob.mode.as_str());
        if size > TEXT_LIMIT {
            let modified = metadata.modified().ok();
            return Ok(Some(Blob {
                id: format!("worktree:{size}:{modified:?}"),
                size,
                mode: mode.into(),
            }));
        }
        (mode, read_text_bytes(&file)?)
    } else {
        return Ok(None);
    };
    if let Some(before) = baseline.filter(|blob| blob.mode == mode && blob.size == content.len())
        && git(root, &["cat-file", "blob", &before.id])? == content
    {
        return Ok(Some(before.clone()));
    }
    Ok(Some(Blob {
        id: format!("worktree:{:x}", Sha256::digest(&content)),
        size: content.len(),
        mode: mode.into(),
    }))
}

fn read_text_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take((TEXT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

pub(super) fn source(root: &Path, path: &str, blob: &Blob) -> Result<Vec<u8>, String> {
    let file = path_in_root(root, path)?;
    if !fs::symlink_metadata(&file)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Worktree source changed while loading.".into());
    }
    let source = read_text_bytes(&file)?;
    if format!("worktree:{:x}", Sha256::digest(&source)) != blob.id {
        return Err("Worktree source changed while loading.".into());
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Change;

    #[test]
    fn reads_uncommitted_changes_against_head_and_excludes_ignored_files() {
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let root = Scratch(
            std::env::temp_dir().join(format!("repo-city-worktree-test-{}", std::process::id())),
        );
        fs::create_dir(&root.0).unwrap();
        git(&root.0, &["init", "-q"]).unwrap();
        fs::write(root.0.join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(root.0.join("edit.rs"), "fn before() {}\n").unwrap();
        fs::write(root.0.join("gone.rs"), "fn gone() {}\n").unwrap();
        git(&root.0, &["add", "."]).unwrap();
        git(
            &root.0,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "Initial",
            ],
        )
        .unwrap();
        fs::write(root.0.join("edit.rs"), "fn after() {}\n").unwrap();
        fs::write(root.0.join("new file.rs"), "fn added() {}\n").unwrap();
        fs::write(root.0.join("ignored.rs"), "fn ignored() {}\n").unwrap();
        fs::remove_file(root.0.join("gone.rs")).unwrap();

        let snapshot = Repository::open_worktree(&root.0).unwrap();
        let file = |path| snapshot.files.iter().position(|file| file.path == path);
        let edited = file("edit.rs").unwrap();
        assert_eq!(snapshot.files[edited].change, Change::Modified);
        assert_eq!(snapshot.source(edited, true).unwrap(), "fn before() {}\n");
        assert_eq!(snapshot.source(edited, false).unwrap(), "fn after() {}\n");
        assert_eq!(
            snapshot.files[file("new file.rs").unwrap()].change,
            Change::Added
        );
        assert_eq!(
            snapshot.files[file("gone.rs").unwrap()].change,
            Change::Removed
        );
        assert!(file("ignored.rs").is_none());
        fs::write(root.0.join("edit.rs"), "fn other() {}\n").unwrap();
        assert!(snapshot.source(edited, false).is_err());
    }
}
