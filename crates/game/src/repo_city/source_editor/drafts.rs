//! Private source drafts are separate from inspected snapshots and review comments.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Identity {
    pub repository: String,
    pub revision: String,
    pub baseline: Option<String>,
    pub path: String,
    pub blob: String,
    pub before: bool,
    pub range: std::ops::Range<usize>,
}
impl Identity {
    pub fn parse(value: &str, path: &str) -> Result<Self, String> {
        let identity: Self =
            serde_json::from_str(value).map_err(|_| "Invalid source draft identity.")?;
        if identity.path != path
            || identity.repository.is_empty()
            || identity.revision.is_empty()
            || identity.blob.is_empty()
        {
            return Err("Source draft identity does not match this file.".into());
        }
        Ok(identity)
    }
    pub fn key(&self) -> String {
        format!(
            "repo-city-source-draft-v1:{}",
            serde_json::to_string(self).expect("string identity")
        )
    }
}
#[derive(Serialize, Deserialize)]
struct Record {
    identity: Identity,
    text: String,
}
pub(super) fn load(identity: &Identity) -> Result<Option<String>, String> {
    let Some(data) = read(&identity.key())? else {
        return Ok(None);
    };
    let record: Record = serde_json::from_str(&data)
        .map_err(|_| "Saved source draft is invalid. It was kept for recovery.")?;
    if &record.identity != identity {
        return Err("Saved source draft belongs to another snapshot.".into());
    }
    super::kernel::validate(&record.text)?;
    Ok(Some(record.text))
}
pub(super) fn save(identity: &Identity, text: &str) -> Result<(), String> {
    super::kernel::validate(text)?;
    write(
        &identity.key(),
        Some(
            &serde_json::to_string(&Record {
                identity: identity.clone(),
                text: text.into(),
            })
            .map_err(|e| e.to_string())?,
        ),
    )
}
pub(super) fn remove(identity: &Identity) -> Result<(), String> {
    write(&identity.key(), None)
}

#[cfg(not(target_arch = "wasm32"))]
fn path(key: &str) -> Result<std::path::PathBuf, String> {
    let settings = crate::repository_picker::settings_file()
        .ok_or("Could not locate source draft storage.")?;
    // The full identity is checked inside each record; a collision never loads another file's draft.
    let hash = key.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
    });
    Ok(settings
        .with_file_name("source-drafts")
        .join(format!("{hash:016x}.json")))
}
#[cfg(not(target_arch = "wasm32"))]
fn read(key: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path(key)?) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read source draft: {error}")),
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn write(key: &str, value: Option<&str>) -> Result<(), String> {
    let path = path(key)?;
    if let Some(value) = value {
        let parent = path.parent().ok_or("Invalid draft storage directory.")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        use std::io::Write;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let temporary = path.with_extension(format!("{}-{nonce}.pending", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|e| format!("Could not save source draft: {e}"))?;
        let result = file
            .write_all(value.as_bytes())
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = result.and_then(|_| std::fs::rename(&temporary, &path)) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!("Could not finish saving source draft: {error}"));
        }
    } else if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error.to_string());
        }
    }
    Ok(())
}
#[cfg(target_arch = "wasm32")]
fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("Browser storage is unavailable.")?
        .local_storage()
        .map_err(|_| "Browser denied access to source drafts.")?
        .ok_or("Browser storage is unavailable.".into())
}
#[cfg(target_arch = "wasm32")]
fn read(key: &str) -> Result<Option<String>, String> {
    storage()?
        .get_item(key)
        .map_err(|_| "Could not read source draft.".into())
}
#[cfg(target_arch = "wasm32")]
fn write(key: &str, value: Option<&str>) -> Result<(), String> {
    let storage = storage()?;
    match value {
        Some(value) => storage.set_item(key, value),
        None => storage.remove_item(key),
    }
    .map_err(|_| {
        "Could not save draft: browser storage may be full. Copy your edits before closing.".into()
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub(crate) struct ApplyTarget {
    pub root: std::path::PathBuf,
    pub path: String,
    pub expected: String,
    pub bytes: std::ops::Range<usize>,
}
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub(crate) struct ApplyTarget;
#[cfg(not(target_arch = "wasm32"))]
impl ApplyTarget {
    pub fn apply(&mut self, text: &str) -> Result<(), String> {
        use std::path::Component;
        let mut full = self.expected.clone();
        if full.get(self.bytes.clone()).is_none() {
            return Err("Chunk no longer matches its original source interval.".into());
        }
        full.replace_range(self.bytes.clone(), text);
        let relative = std::path::Path::new(&self.path);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("Cannot apply an unsafe repository path.".into());
        }
        let root = self.root.canonicalize().map_err(|e| e.to_string())?;
        let mut path = root.clone();
        for part in relative.components() {
            path.push(part);
            if std::fs::symlink_metadata(&path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("Cannot apply edits through a symbolic link.".into());
            }
        }
        if !path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&root)
        {
            return Err("File is outside this repository.".into());
        }
        if !std::fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_file()
        {
            return Err("Cannot apply edits to a non-regular file.".into());
        }
        if std::fs::read_to_string(&path).map_err(|e| e.to_string())? != self.expected {
            return Err("File changed outside VibePad. Copy your draft and reopen the current file before applying.".into());
        }
        let permissions = std::fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions();
        // An explicit Apply replaces only this existing regular file; no shell or Git commands run.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let temporary =
            path.with_file_name(format!(".repo-city-edit-{}-{nonce}", std::process::id()));
        let result = (|| -> Result<(), String> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            file.write_all(full.as_bytes()).map_err(|e| e.to_string())?;
            file.set_permissions(permissions)
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            if std::fs::read_to_string(&path).map_err(|e| e.to_string())? != self.expected {
                return Err("File changed while applying. Your draft is unchanged.".into());
            }
            std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        self.bytes.end = self.bytes.start + text.len();
        self.expected = full;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_identity_separates_repository_revision_side_and_blob() {
        let first = Identity {
            repository: "owner/project".into(),
            revision: "a".into(),
            baseline: None,
            path: "src/main.rs".into(),
            blob: "blob-a".into(),
            before: false,
            range: 0..3,
        };
        for other in [
            Identity {
                repository: "other/project".into(),
                ..first.clone()
            },
            Identity {
                revision: "b".into(),
                ..first.clone()
            },
            Identity {
                path: "src/lib.rs".into(),
                ..first.clone()
            },
            Identity {
                blob: "blob-b".into(),
                ..first.clone()
            },
            Identity {
                before: true,
                ..first.clone()
            },
            Identity {
                range: 3..6,
                ..first.clone()
            },
        ] {
            assert_ne!(first.key(), other.key());
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn explicit_apply_preserves_external_changes_and_rejects_escape() {
        let root = std::env::temp_dir().join(format!("repo-city-editor-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("file.rs"), "prefix\noriginal\nsuffix\n").unwrap();
        let mut target = ApplyTarget {
            root: root.clone(),
            path: "file.rs".into(),
            expected: "prefix\noriginal\nsuffix\n".into(),
            bytes: 7..16,
        };
        target.apply("edited\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("file.rs")).unwrap(),
            "prefix\nedited\nsuffix\n"
        );
        target.apply("again\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("file.rs")).unwrap(),
            "prefix\nagain\nsuffix\n"
        );
        std::fs::write(root.join("file.rs"), "external").unwrap();
        assert!(target.apply("overwrite").is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("file.rs")).unwrap(),
            "external"
        );
        target.path = "../escape.rs".into();
        assert!(target.apply("outside").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
