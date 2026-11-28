//! Private canvas membership and viewports for a repository comparison.
use std::collections::{HashMap, HashSet};

use repo_world::{Change, File, Repository};
use serde::{Deserialize, Serialize};

mod placement;
mod storage;

pub(super) use placement::{Axis, GAP};
#[cfg(test)]
mod tests;

const DEFAULT_NAME: &str = "Changes";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Scope(String);

impl Scope {
    pub fn for_repository(repo: &Repository) -> Self {
        if repo.is_worktree() {
            // HEAD changes while the live checkout remains open.
            return Self(format!("worktree:{:?}", repo.root));
        }
        Self(crate::progress::comparison_scope(repo))
    }

    fn key(&self) -> String {
        format!("repo-city-canvas-v1:{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Viewport {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Tile {
    pub path: String,
    pub viewport: Viewport,
    #[serde(default)]
    pub positioned: bool,
    #[serde(default)]
    pub size: [f32; 2],
    #[serde(default)]
    origin: Option<[f32; 2]>,
    explicit: bool,
}

impl Tile {
    fn new(path: &str, explicit: bool) -> Self {
        Self {
            path: path.into(),
            viewport: Viewport::default(),
            positioned: false,
            size: [0.; 2],
            origin: None,
            explicit,
        }
    }

    pub fn file<'a>(&self, repo: &'a Repository) -> Option<&'a File> {
        let index = file_index(repo, &self.path)?;
        Some(&repo.files[index])
    }
}

fn file_index(repo: &Repository, path: &str) -> Option<usize> {
    // Repository inventories are sorted by path when built from the Git trees.
    repo.files
        .binary_search_by(|file| file.path.as_str().cmp(path))
        .ok()
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Canvas {
    pub name: String,
    pub tiles: Vec<Tile>,
    pub pan: [f32; 2],
    pub zoom: f32,
    #[serde(default)]
    pub layout_width: Option<f32>,
}

impl Canvas {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            tiles: Vec::new(),
            pan: [0., 0.],
            zoom: 1.,
            layout_width: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Book {
    scope: Scope,
    active: usize,
    canvases: Vec<Canvas>,
    #[serde(skip)]
    protect_saved: bool,
}

impl Book {
    fn new(scope: Scope, repo: &Repository) -> Self {
        let mut book = Self {
            scope,
            active: 0,
            canvases: vec![Canvas::new(DEFAULT_NAME)],
            protect_saved: false,
        };
        book.refresh(repo);
        book
    }

    pub fn open(scope: Scope, repo: &Repository) -> (Self, Option<String>) {
        match storage::read(&scope.key()) {
            Ok(Some(value)) => match Self::decode(&scope, &value) {
                Ok(mut book) => {
                    book.refresh(repo);
                    (book, None)
                }
                Err(error) => {
                    let mut book = Self::new(scope, repo);
                    book.protect_saved = true;
                    (book, Some(error))
                }
            },
            Ok(None) => (Self::new(scope, repo), None),
            Err(error) => (Self::new(scope, repo), Some(error)),
        }
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn canvases(&self) -> &[Canvas] {
        &self.canvases
    }

    pub fn active(&self) -> &Canvas {
        &self.canvases[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Canvas {
        &mut self.canvases[self.active]
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.canvases.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    pub fn create(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a canvas name.".into());
        }
        if self.canvases.iter().any(|canvas| canvas.name == name) {
            return Err("A canvas already uses this name.".into());
        }
        self.canvases.push(Canvas::new(name));
        self.active = self.canvases.len() - 1;
        Ok(())
    }

    pub fn add_paths<'a>(
        &mut self,
        repo: &Repository,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        let tiles = &mut self.active_mut().tiles;
        let mut existing: HashMap<_, _> = tiles
            .iter_mut()
            .map(|tile| (tile.path.as_str(), &mut tile.explicit))
            .collect();
        let mut added = Vec::new();
        let mut seen = HashSet::new();
        let mut changed = false;
        for path in paths {
            if !seen.insert(path) {
                continue;
            }
            if let Some(explicit) = existing.get_mut(path) {
                changed |= !std::mem::replace(*explicit, true);
            } else if file_index(repo, path).is_some() {
                added.push(Tile::new(path, true));
            }
        }
        changed |= !added.is_empty();
        tiles.extend(added);
        changed
    }

    pub fn refresh(&mut self, repo: &Repository) -> bool {
        let tiles = &mut self.canvases[0].tiles;
        let mut changed = false;
        tiles.retain(|tile| {
            let keep = tile.explicit
                || tile
                    .file(repo)
                    .is_some_and(|file| file.change != Change::Unchanged);
            changed |= !keep;
            keep
        });
        let known: HashSet<_> = tiles.iter().map(|tile| tile.path.as_str()).collect();
        let added: Vec<_> = repo
            .files
            .iter()
            .filter(|file| file.change != Change::Unchanged && !known.contains(file.path.as_str()))
            .map(|file| Tile::new(&file.path, false))
            .collect();
        changed |= !added.is_empty();
        tiles.extend(added);
        changed
    }

    pub fn save(&self) -> Result<(), String> {
        if self.protect_saved {
            return Err("Saved canvas layout could not be read and was kept for recovery.".into());
        }
        storage::write(&self.scope.key(), &self.encode()?)
    }

    fn encode(&self) -> Result<String, String> {
        serde_json::to_string(self)
            .map_err(|error| format!("Could not encode canvas layout: {error}"))
    }

    fn decode(scope: &Scope, value: &str) -> Result<Self, String> {
        let book: Self = serde_json::from_str(value)
            .map_err(|_| "Saved canvas layout is invalid. It was kept for recovery.")?;
        if &book.scope != scope {
            return Err("Saved canvas layout belongs to another comparison.".into());
        }
        if book.active >= book.canvases.len()
            || book.canvases[0].name != DEFAULT_NAME
            || book.canvases.iter().any(|canvas| {
                canvas.zoom < 0.025
                    || canvas.tiles.iter().any(|tile| {
                        !tile
                            .size
                            .iter()
                            .all(|value| value.is_finite() && (0. ..=2400.).contains(value))
                            || tile.origin.is_some_and(|origin| {
                                origin.iter().any(|value| !value.is_finite() || *value < 0.)
                                    || tile.size[0] < placement::MIN_PANE.x
                                    || tile.size[1] < placement::MIN_PANE.y
                            })
                    })
                    || canvas
                        .layout_width
                        .is_some_and(|width| !width.is_finite() || width <= 0.)
                    || !canvas.zoom.is_finite()
                    || !canvas.pan.iter().all(|component| component.is_finite())
            })
        {
            return Err("Saved canvas layout is invalid. It was kept for recovery.".into());
        }
        Ok(book)
    }
}
