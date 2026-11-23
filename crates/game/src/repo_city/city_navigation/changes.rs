//! A directory-ordered route through the changes in the current comparison.
use super::*;

const HUD_SECONDS: f32 = 4.;
const VISIBLE_STOPS: usize = 36;

#[derive(Clone, Copy)]
pub(super) enum FileView {
    Wall(listing::Face),
    SourcePanel,
    Comparison(u8),
}

impl FileView {
    pub(super) fn current(
        explorer: &Explorer,
        file: &repo_world::File,
        camera: &Transform,
        listing: &listing::Listing,
    ) -> Self {
        if explorer.comparison_mode > 0 {
            Self::Comparison(explorer.comparison_mode)
        } else if explorer.anchored {
            Self::SourcePanel
        } else {
            let size = listing::block_size(file);
            let reach = listing.frame.detail_reach(size.x) * 1.18;
            Self::Wall(
                listing::Face::at_camera(file, size, camera, reach)
                    .or(listing.visible_face())
                    .unwrap_or(listing::Face::Source),
            )
        }
    }

    pub(super) fn is_diff(self) -> bool {
        matches!(self, Self::Wall(listing::Face::Diff) | Self::Comparison(_))
    }

    pub(super) fn wall_rows(self, explorer: &Explorer) -> usize {
        match self {
            Self::Wall(listing::Face::Source) => explorer.source_layout.len(),
            Self::Wall(listing::Face::Diff) => explorer.diff_layout.rows.len(),
            Self::Wall(listing::Face::Blame) => explorer.blame_layout.len(),
            Self::SourcePanel | Self::Comparison(_) => 0,
        }
    }

    pub(super) fn destination(
        self,
        repo: &Repository,
        explorer: &mut Explorer,
        listing: &listing::Listing,
        eye: Vec3,
    ) -> Transform {
        let file = &repo.files[explorer.selected];
        let hunk = explorer
            .comparison
            .as_ref()
            .and_then(|diff| diff.hunks.first())
            .map_or(0, |hunk| hunk.row);
        match self {
            Self::Comparison(mode) => {
                explorer.comparison_mode = mode;
                explorer.anchored = true;
                explorer.page = hunk / PAGE;
                let mut target = Transform::default();
                comparison_scene::focus(&mut target, repo, explorer);
                target
            }
            Self::SourcePanel => {
                explorer.anchored = true;
                source_panel_pose(file)
            }
            Self::Wall(listing::Face::Diff) => listing.diff_view(explorer, file, hunk),
            Self::Wall(face) => {
                let aim = file_facade::Aim {
                    index: explorer.selected,
                    surface: None,
                }
                .on_side(face);
                listing.arrival_view(explorer, file, listing::block_size(file), Some(aim), eye)
            }
        }
    }
}

pub(crate) fn source_panel_pose(file: &repo_world::File) -> Transform {
    let [x, z] = file.position;
    Transform::from_xyz(x, 6.2, z + 14.5).looking_at(Vec3::new(x, 6.2, z + 3.2), Vec3::Y)
}

#[derive(Default)]
pub(super) struct ReviewRoute {
    order: Vec<usize>,
    position: Option<usize>,
    diff: bool,
    finished: bool,
    expires_at: f32,
}

impl ReviewRoute {
    pub(super) fn step(
        &mut self,
        repo: &Repository,
        progress: &progress::Progress,
        selected: usize,
        backwards: bool,
        diff: bool,
        now: f32,
    ) -> Option<usize> {
        if self.order.is_empty() {
            self.order = repo
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| file.change != Change::Unchanged)
                .map(|(index, _)| index)
                .collect();
            self.order.sort_unstable_by_key(|&index| {
                let file = &repo.files[index];
                (file.directory(), file.path.as_str())
            });
        }
        self.diff = diff;
        self.show(now);
        let cursor = progress.file_cursor().and_then(|path| {
            self.order
                .iter()
                .position(|&index| repo.files[index].path == path)
        });
        let folder = repo.files[selected].directory();
        let len = self.order.len();
        let start = cursor.map_or_else(
            || {
                self.order.partition_point(|&index| {
                    let directory = repo.files[index].directory();
                    directory < folder || backwards && directory == folder
                })
            },
            |position| position + usize::from(!backwards),
        );
        let next = (0..len)
            .map(|offset| {
                if backwards {
                    (start + len - 1 - offset) % len
                } else {
                    (start + offset) % len
                }
            })
            .find(|&position| !progress.has_visited_view(self.order[position], diff));
        self.finished = next.is_none();
        self.position = next.or(cursor);
        next.map(|position| self.order[position])
    }

    pub(super) fn show(&mut self, now: f32) {
        self.expires_at = now + HUD_SECONDS;
    }

    pub(super) fn hide(&mut self) {
        self.expires_at = 0.;
    }

    pub(super) fn hud(
        &self,
        repo: &Repository,
        progress: &progress::Progress,
        now: f32,
    ) -> Option<RouteHud> {
        if now >= self.expires_at {
            return None;
        }
        let total = self.order.len();
        let label = if self.diff {
            "Diff review"
        } else {
            "Changed files"
        };
        let title = if total == 0 {
            "No changed files in this comparison".into()
        } else if self.finished {
            if self.diff {
                "All diffs opened".into()
            } else {
                "All changed files visited".into()
            }
        } else {
            format!(
                "{label} · {} / {total}",
                self.position.unwrap_or_default() + 1
            )
        };
        let start = self
            .position
            .unwrap_or_default()
            .saturating_sub(VISIBLE_STOPS / 2)
            .min(total.saturating_sub(VISIBLE_STOPS));
        let stops = self
            .order
            .iter()
            .enumerate()
            .skip(start)
            .take(VISIBLE_STOPS)
            .map(|(position, &index)| {
                if self.position == Some(position) {
                    StopState::Current
                } else if progress.has_visited_view(index, self.diff) {
                    StopState::Visited
                } else {
                    StopState::Pending
                }
            })
            .collect();
        let path = self
            .position
            .map(|position| repo.files[self.order[position]].path.clone())
            .unwrap_or_default();
        Some(RouteHud { title, path, stops })
    }
}

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RouteHud {
    pub title: String,
    pub path: String,
    pub stops: Vec<StopState>,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopState {
    Pending,
    Visited,
    Current,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
