//! The working directory as a read-only snapshot against its current HEAD.
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Blob, Repository, git, resolve_revision, tree};

// Avoid hashing large changed files as part of inventory. Opening one still reads the snapshot.
const EAGER_HASH_BYTES: usize = 1024 * 1024;

fn metadata_identity(metadata: &fs::Metadata) -> String {
    format!("worktree:{}:{:?}", metadata.len(), metadata.modified().ok())
}

fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    if !left.is_file()
        || !right.is_file()
        || left.len() != right.len()
        || left.modified().ok() != right.modified().ok()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if left.dev() != right.dev()
            || left.ino() != right.ino()
            || left.ctime() != right.ctime()
            || left.ctime_nsec() != right.ctime_nsec()
        {
            return false;
        }
    }
    true
}

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
        if size > EAGER_HASH_BYTES {
            return Ok(Some(Blob {
                id: metadata_identity(&metadata),
                size,
                mode: mode.into(),
            }));
        }
        let mut bytes = Vec::with_capacity(size);
        fs::File::open(&file)
            .map_err(|error| error.to_string())?
            .take((EAGER_HASH_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        (mode, bytes)
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

pub(super) fn snapshot_bytes(root: &Path, path: &str, blob: &Blob) -> Result<Vec<u8>, String> {
    let changed = || "Worktree file changed while loading.".to_owned();
    let file = path_in_root(root, path)?;
    let before = fs::symlink_metadata(&file).map_err(|error| error.to_string())?;
    if !before.is_file() || before.len() != blob.size as u64 {
        return Err(changed());
    }
    let opened = fs::File::open(&file).map_err(|error| error.to_string())?;
    if !same_file(
        &before,
        &opened.metadata().map_err(|error| error.to_string())?,
    ) {
        return Err(changed());
    }
    path_in_root(root, path)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(blob.size)
        .map_err(|error| error.to_string())?;
    (&opened)
        .take(blob.size as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() != blob.size {
        return Err(changed());
    }
    let after = fs::symlink_metadata(&file).map_err(|error| error.to_string())?;
    if !same_file(&before, &after)
        || !same_file(
            &before,
            &opened.metadata().map_err(|error| error.to_string())?,
        )
    {
        return Err(changed());
    }
    path_in_root(root, path)?;
    if metadata_identity(&before) != blob.id
        && format!("worktree:{:x}", Sha256::digest(&bytes)) != blob.id
    {
        return Err(changed());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Change;

    struct AssetFixture(PathBuf);

    impl AssetFixture {
        fn new(name: &str) -> Self {
            let root = Self(
                std::env::temp_dir()
                    .join(format!("vibe-city-assets-{name}-{}", std::process::id())),
            );
            fs::create_dir_all(&root.0).unwrap();
            git(&root.0, &["init", "-q"]).unwrap();
            fs::write(
                root.0.join("image.png"),
                include_bytes!("../../../web/assets/apple-touch-icon.png"),
            )
            .unwrap();
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
                    "Image",
                ],
            )
            .unwrap();
            root
        }
    }

    impl Drop for AssetFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn binary_preview_reads_the_selected_commit_and_worktree_snapshot() {
        // Given a tracked PNG, preview reads the commit even after disk contents change.
        let fixture = AssetFixture::new("snapshots");
        let committed = Repository::open(&fixture.0, "HEAD").unwrap();
        let original = committed.asset_bytes(0, false).unwrap();
        assert!(original.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(committed.source(0, false).is_err());
        fs::write(fixture.0.join("image.png"), b"invalid image bytes").unwrap();
        let worktree = Repository::open_worktree(&fixture.0).unwrap();
        assert_eq!(committed.asset_bytes(0, false).unwrap(), original);
        assert_eq!(worktree.asset_bytes(0, true).unwrap(), original);
        assert_eq!(
            worktree.asset_bytes(0, false).unwrap(),
            b"invalid image bytes"
        );
        fs::write(fixture.0.join("image.png"), b"different image bytes").unwrap();
        assert!(
            worktree
                .asset_bytes(0, false)
                .unwrap_err()
                .contains("changed while loading")
        );
    }

    #[test]
    fn reads_a_large_document_from_git_and_its_changed_worktree_snapshot() {
        let fixture = AssetFixture::new("large-document");
        let path = "CHANGELOG.md";
        let previous = format!(
            "# Changelog\n\n{}Tail from HEAD.\n",
            "A release entry.\n".repeat(75_000)
        );
        fs::write(fixture.0.join(path), &previous).unwrap();
        git(&fixture.0, &["add", path]).unwrap();
        git(
            &fixture.0,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "Changelog",
            ],
        )
        .unwrap();
        let next = previous.replace("Tail from HEAD.", "Tail from disk.");
        fs::write(fixture.0.join(path), &next).unwrap();
        let snapshot = Repository::open_worktree(&fixture.0).unwrap();
        let index = snapshot
            .files
            .iter()
            .position(|file| file.path == path)
            .unwrap();
        assert_eq!(snapshot.source(index, true).unwrap(), previous);
        assert_eq!(snapshot.source(index, false).unwrap(), next);
        fs::write(fixture.0.join(path), b"# Replaced\n").unwrap();
        assert!(
            snapshot
                .source(index, false)
                .unwrap_err()
                .contains("changed while loading")
        );
    }

    #[test]
    fn large_binary_preview_uses_metadata_snapshot_and_still_rejects_binary_as_text() {
        let fixture = AssetFixture::new("large");
        let bytes = vec![0; EAGER_HASH_BYTES + 17];
        fs::write(fixture.0.join("image.png"), &bytes).unwrap();
        let worktree = Repository::open_worktree(&fixture.0).unwrap();
        assert!(
            worktree
                .source(0, false)
                .unwrap_err()
                .contains("Binary asset")
        );
        assert_eq!(worktree.asset_bytes(0, false).unwrap(), bytes);
        fs::write(fixture.0.join("image.png"), vec![43; EAGER_HASH_BYTES + 18]).unwrap();
        assert!(
            worktree
                .asset_bytes(0, false)
                .unwrap_err()
                .contains("changed while loading")
        );
    }

    #[cfg(unix)]
    #[test]
    fn binary_preview_never_follows_a_replaced_symlink() {
        let fixture = AssetFixture::new("symlink");
        fs::write(fixture.0.join("image.png"), b"worktree image").unwrap();
        let snapshot = Repository::open_worktree(&fixture.0).unwrap();
        fs::remove_file(fixture.0.join("image.png")).unwrap();
        fs::write(fixture.0.join("replacement.png"), b"worktree image").unwrap();
        std::os::unix::fs::symlink("replacement.png", fixture.0.join("image.png")).unwrap();
        assert!(snapshot.asset_bytes(0, false).is_err());
    }

    #[test]
    fn preview_size_limit_rejects_large_assets_before_reading() {
        let fixture = AssetFixture::new("oversized");
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.0.join("image.png"))
            .unwrap()
            .set_len((crate::ASSET_BYTES_LIMIT + 1) as u64)
            .unwrap();
        let snapshot = Repository::open_worktree(&fixture.0).unwrap();
        assert!(
            snapshot
                .asset_bytes(0, false)
                .unwrap_err()
                .contains("16 MiB")
        );
    }

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
