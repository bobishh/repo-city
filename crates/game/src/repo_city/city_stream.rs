//! Keep the comparison intact while materializing the city near the camera.
use std::collections::HashMap;

use super::*;

const CELL: f32 = 160.;
const RADIUS: f32 = 640.;
const EAGER_FILES: usize = 160;
const FILE_LIMIT: usize = 5000;
const DISTRICT_LIMIT: usize = 256;
const SPAWNS_PER_FRAME: usize = 24;
const REMOVALS_PER_FRAME: usize = 48;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum Item {
    File(usize),
    District(usize),
}

#[derive(Component)]
pub(super) struct DistrictPart(pub usize);

#[derive(Default)]
pub(super) struct Stream {
    cells: HashMap<IVec2, Vec<Item>>,
    resident: HashSet<Item>,
    desired: Vec<Item>,
    retiring: Vec<Item>,
    checked: usize,
    eye_cell: Option<IVec2>,
}

fn cell(point: Vec2) -> IVec2 {
    (point / CELL).floor().as_ivec2()
}

impl Stream {
    pub(super) fn reset(&mut self, repo: &Repository) {
        *self = Self::default();
        if repo.files.len() <= EAGER_FILES {
            self.desired
                .extend((0..repo.districts.len()).map(Item::District));
            self.desired.extend((0..repo.files.len()).map(Item::File));
            return;
        }
        for (index, file) in repo.files.iter().enumerate() {
            self.cells
                .entry(cell(file.position.into()))
                .or_default()
                .push(Item::File(index));
        }
        for (index, district) in repo.districts.iter().enumerate() {
            let start = Vec2::from_array(district.position);
            let min = cell(start);
            let max = cell(start + Vec2::from_array(district.size));
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    self.cells
                        .entry(IVec2::new(x, y))
                        .or_default()
                        .push(Item::District(index));
                }
            }
        }
    }

    pub(super) fn refresh_files(&mut self, files: &HashSet<usize>) {
        for &file in files {
            self.resident.remove(&Item::File(file));
        }
        self.checked = 0;
    }

    pub(super) fn next(
        &mut self,
        repo: &Repository,
        eye: Vec2,
        selected: usize,
    ) -> (HashSet<Item>, Vec<Item>) {
        let large = repo.files.len() > EAGER_FILES;
        let eye_cell = cell(eye);
        if large && self.eye_cell != Some(eye_cell) {
            self.eye_cell = Some(eye_cell);
            let center = (eye_cell.as_vec2() + Vec2::splat(0.5)) * CELL;
            let min = cell(center - Vec2::splat(RADIUS));
            let max = cell(center + Vec2::splat(RADIUS));
            let mut candidates = HashSet::new();
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    if let Some(items) = self.cells.get(&IVec2::new(x, y)) {
                        candidates.extend(items.iter().copied());
                    }
                }
            }
            let mut nearby = candidates
                .into_iter()
                .filter_map(|item| {
                    let point = match item {
                        Item::File(index) => Vec2::from_array(repo.files[index].position),
                        Item::District(index) => {
                            let district = &repo.districts[index];
                            let min = Vec2::from_array(district.position);
                            center.clamp(min, min + Vec2::from_array(district.size))
                        }
                    };
                    let distance = center.distance_squared(point);
                    (distance <= RADIUS * RADIUS).then_some((item, distance))
                })
                .collect::<Vec<_>>();
            nearby.sort_unstable_by(|(a, ad), (b, bd)| ad.total_cmp(bd).then_with(|| a.cmp(b)));
            let (mut files, mut districts) = (0, 0);
            self.desired = nearby
                .into_iter()
                .filter_map(|(item, _)| {
                    let (count, limit) = match item {
                        Item::File(_) => (&mut files, FILE_LIMIT),
                        Item::District(_) => (&mut districts, DISTRICT_LIMIT),
                    };
                    *count += 1;
                    (*count <= limit).then_some(item)
                })
                .collect();
            let desired: HashSet<_> = self.desired.iter().copied().collect();
            self.retiring = self.resident.difference(&desired).copied().collect();
            self.retiring.sort_unstable();
            self.checked = 0;
        }
        let mut evicted = HashSet::new();
        for _ in 0..REMOVALS_PER_FRAME {
            let Some(item) = self.retiring.pop() else {
                break;
            };
            if self.resident.remove(&item) {
                evicted.insert(item);
            }
        }
        let limit = if large {
            SPAWNS_PER_FRAME.min(FILE_LIMIT.saturating_sub(self.resident.len()))
        } else {
            self.desired.len()
        };
        let first = Item::File(selected);
        let mut spawned = Vec::new();
        if large && limit > 0 && !self.resident.contains(&first) && self.desired.contains(&first) {
            self.resident.insert(first);
            spawned.push(first);
        }
        while spawned.len() < limit
            && let Some(&item) = self.desired.get(self.checked)
        {
            self.checked += 1;
            if self.resident.insert(item) {
                spawned.push(item);
            }
        }
        (evicted, spawned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_streams_nearby_buildings_in_bounded_batches_without_pruning_search_paths() {
        let mut repo = repository_picker::empty();
        repo.files = (0..6000)
            .map(|index| repo_world::File {
                path: format!("src/{index:04}.rs"),
                before: None,
                after: None,
                change: Change::Added,
                position: [(index % 100) as f32 * 27., (index / 100) as f32 * 30.],
            })
            .collect();
        let mut stream = Stream::default();
        stream.reset(&repo);
        let (_, first) = stream.next(&repo, Vec2::ZERO, 0);
        assert_eq!(first.len(), SPAWNS_PER_FRAME);
        assert_eq!(first[0], Item::File(0));
        assert!(!first.contains(&Item::File(5999)));
        let (_, second) = stream.next(&repo, Vec2::ZERO, 0);
        assert_eq!(second.len(), SPAWNS_PER_FRAME);
        assert!(second.iter().all(|file| !first.contains(file)));
        let (left, arrived) = stream.next(&repo, repo.files[5999].position.into(), 5999);
        assert!(first.iter().all(|item| left.contains(item)));
        assert_eq!(arrived.len(), SPAWNS_PER_FRAME);
        assert_eq!(arrived[0], Item::File(5999));
        assert_eq!(repo.files.len(), 6000);
    }
}
