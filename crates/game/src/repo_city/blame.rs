//! Load attribution only when the user visits the blame wall.
use super::*;

#[derive(Default)]
pub(super) struct Blame {
    #[cfg(not(target_arch = "wasm32"))]
    key: Option<(std::path::PathBuf, String, String)>,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>>,
    #[cfg(not(target_arch = "wasm32"))]
    notes: Option<Result<Vec<String>, String>>,
    #[cfg(target_arch = "wasm32")]
    link: Option<(String, bool, usize)>,
}

impl Blame {
    pub(super) fn notes(&self) -> Option<&[String]> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(Ok(notes)) = &self.notes {
            return Some(notes);
        }
        None
    }

    pub(super) fn caption(&self) -> &'static str {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.notes {
                Some(Ok(_)) => "BLAME / COMMIT + AUTHOR",
                Some(Err(_)) => "BLAME / HISTORY UNAVAILABLE",
                None => "BLAME / LOADING LINE HISTORY",
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            "BLAME / OPEN LINE HISTORY ON GITHUB"
        }
    }

    /// Returns whether the cached line history has changed.
    pub(super) fn update(
        &mut self,
        repo: &Repository,
        explorer: &Explorer,
        looking_at: Option<usize>,
    ) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(_) = looking_at else {
                return false;
            };
            let revision = if explorer.before {
                repo.baseline.as_deref()
            } else {
                Some(repo.revision.as_str())
            };
            let Some(revision) = revision else {
                return false;
            };
            let path = &repo.files[explorer.selected].path;
            let different = !self.key.as_ref().is_some_and(|(root, rev, name)| {
                *root == repo.root && rev == revision && name == path
            });
            // Only one Git process at a time, even when flying past several files.
            if let Some(receiver) = &mut self.pending {
                let result = receiver.get_mut().unwrap().try_recv();
                match result {
                    Ok(notes) => {
                        self.pending = None;
                        self.notes = Some(notes);
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.pending = None;
                        self.notes = Some(Err("Git blame process stopped.".into()));
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                }
                if !different {
                    return true;
                }
            }
            if different {
                let key = (repo.root.clone(), revision.to_owned(), path.clone());
                self.key = Some(key.clone());
                self.notes = None;
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                if std::thread::Builder::new()
                    .name("repo-city-blame".into())
                    .spawn(move || {
                        let (root, revision, path) = key;
                        let _ = sender.send(repo_world::blame::lines(&root, &revision, &path));
                    })
                    .is_ok()
                {
                    self.pending = Some(std::sync::Mutex::new(receiver));
                } else {
                    self.notes = Some(Err("Could not start Git blame.".into()));
                }
                return true;
            }
            false
        }
        #[cfg(target_arch = "wasm32")]
        {
            let next = looking_at.map(|row| {
                (
                    repo.files[explorer.selected].path.as_str(),
                    explorer.before,
                    row + 1,
                )
            });
            if self
                .link
                .as_ref()
                .map(|(path, before, line)| (path.as_str(), *before, *line))
                != next
            {
                self.link = next.map(|(path, before, line)| (path.to_owned(), before, line));
                if let Some((path, before, line)) = next {
                    repo_world::browser_show_blame(path, before, line);
                } else {
                    repo_world::browser_show_blame("", false, 0);
                }
            }
            false
        }
    }

    pub(super) fn clear(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if self.link.take().is_some() {
            repo_world::browser_show_blame("", false, 0);
        }
    }
}
