//! Opens a bounded copy of a snapshot video in the system player.
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{Repository, file_preview, native_github};

pub(super) struct Preview {
    pub path: String,
    pub pending: Option<native_github::Job<Video>>,
    pub status: Result<(), String>,
}

impl Preview {
    pub(super) fn open(repository: &Repository, index: usize, before: bool) -> Option<Self> {
        let file = repository.files.get(index)?.clone();
        let extension = file_preview::video_extension(&file.path)?;
        let path = file.path.clone();
        let snapshot = Repository {
            root: repository.root.clone(),
            revision: repository.revision.clone(),
            baseline: repository.baseline.clone(),
            files: vec![file],
            districts: Vec::new(),
        };
        Some(Self {
            path,
            pending: Some(native_github::Job::spawn(move || {
                let bytes = snapshot.asset_bytes(0, before)?;
                let video = Video::prepare(extension, &bytes)?;
                native_github::open_associated_path(&video.file)?;
                Ok(video)
            })),
            status: Ok(()),
        })
    }
}

pub(super) struct Video {
    directory: PathBuf,
    file: PathBuf,
}

impl Video {
    fn prepare(extension: &'static str, bytes: &[u8]) -> Result<Self, String> {
        let valid = match extension {
            "webm" => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
            "ogv" => bytes.starts_with(b"OggS"),
            _ => matches!(bytes.get(4..8), Some(b"ftyp" | b"moov" | b"mdat" | b"wide")),
        };
        if !valid {
            return Err("This video could not be played.".into());
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "repo-city-video-{}-{now}-{sequence}",
            std::process::id()
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .map_err(|error| error.to_string())?;
        let video = Self {
            file: directory.join(format!("clip.{extension}")),
            directory,
        };
        fs::write(&video.file, bytes).map_err(|error| error.to_string())?;
        Ok(video)
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file);
        let _ = fs::remove_dir(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_only_video_data_and_removes_the_copy_when_released() {
        assert!(Video::prepare("webm", b"invalid video").is_err());
        let bytes = include_bytes!("../../../../web/tests/fixtures/video-preview.webm");
        let video = Video::prepare("webm", bytes).unwrap();
        let path = video.file.clone();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(video);
        assert!(!path.exists());
    }
}
