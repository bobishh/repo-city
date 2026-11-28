#[cfg(not(target_arch = "wasm32"))]
fn path(key: &str) -> Result<std::path::PathBuf, String> {
    use sha2::{Digest, Sha256};
    Ok(crate::repository_picker::settings_file()
        .ok_or("Could not locate canvas layout storage.")?
        .with_file_name("canvas-layouts")
        .join(format!("{:x}.json", Sha256::digest(key.as_bytes()))))
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn read(key: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path(key)?) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read canvas layout: {error}")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write(key: &str, value: &str) -> Result<(), String> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = path(key)?;
    let directory = path.parent().ok_or("Invalid canvas layout directory.")?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create canvas layout storage: {error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}-{stamp}-{serial}.pending", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("Could not save canvas layout: {error}"))?;
    let result = file
        .write_all(value.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result.and_then(|_| fs::rename(&temporary, &path)) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not finish saving canvas layout: {error}"));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("Browser storage is unavailable.")?
        .local_storage()
        .map_err(|_| "Browser denied access to canvas layouts.")?
        .ok_or("Browser storage is unavailable.".into())
}

#[cfg(target_arch = "wasm32")]
pub(super) fn read(key: &str) -> Result<Option<String>, String> {
    storage()?
        .get_item(key)
        .map_err(|_| "Could not read canvas layout.".into())
}

#[cfg(target_arch = "wasm32")]
pub(super) fn write(key: &str, value: &str) -> Result<(), String> {
    storage()?
        .set_item(key, value)
        .map_err(|_| "Could not save canvas layout.".into())
}
