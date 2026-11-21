//! Coordinates shared by the built archive, its map and directory arrivals.
use super::*;

pub(super) const COURT_LENGTH: f32 = 36.;
pub(super) const CABINETS_PER_COURT: usize = 8;
pub(super) const FOLDERS_PER_COURT: usize = 4;
pub(super) const DEPTHS: [f32; 4] = [-11., -4., 4., 11.];
pub(super) const FRONT: f32 = 7.2;
pub(super) const CABINET_X: f32 = 13.;
pub(super) const EYE: f32 = 2.15;
pub(super) const ENTRANCE_Z: f32 = -16.6;

pub(super) fn court(index: usize) -> usize {
    index / FOLDERS_PER_COURT
}

pub(super) fn folder(index: usize) -> Vec2 {
    let slot = index % FOLDERS_PER_COURT;
    let side = if slot.is_multiple_of(2) { -1. } else { 1. };
    let depth = if slot < 2 { -7.5 } else { 7.5 };
    Vec2::new(side * CABINET_X, court(index) as f32 * COURT_LENGTH + depth)
}

pub(super) fn folder_at(court: usize, slot: usize) -> usize {
    court * FOLDERS_PER_COURT + slot / 4 * 2 + slot % 2
}

pub(super) fn rack(slot: usize) -> usize {
    slot / 2 % 2
}

pub(super) fn view(index: usize) -> Transform {
    let cabinet = folder(index);
    let side = cabinet.x.signum();
    Transform::from_xyz(side * 9.2, EYE, cabinet.y)
        .looking_at(Vec3::new(cabinet.x, EYE, cabinet.y), Vec3::Y)
}

pub(super) fn nearest(position: Vec3, total: usize) -> Option<usize> {
    let center = ((position.z + COURT_LENGTH / 2.) / COURT_LENGTH).max(0.) as usize;
    let start = center.saturating_sub(1) * FOLDERS_PER_COURT;
    let end = ((center + 2) * FOLDERS_PER_COURT).min(total);
    let position = Vec2::new(position.x, position.z);
    (start..end)
        .min_by(|&a, &b| {
            folder(a)
                .distance_squared(position)
                .total_cmp(&folder(b).distance_squared(position))
        })
        .or_else(|| total.checked_sub(1))
}

pub(super) fn walkable(position: Vec3) -> bool {
    if position.z < ENTRANCE_Z || position.x.abs() >= 12.0 {
        return false;
    }
    let x = position.x.abs();
    let z = ((position.z + COURT_LENGTH / 2.).rem_euclid(COURT_LENGTH) - COURT_LENGTH / 2.).abs();
    if x < FRONT - 0.40 {
        return true;
    }
    if x < FRONT + 0.40 {
        return (5.0..10.0).contains(&z);
    }
    (0.6..14.2).contains(&z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn court_map_slots_and_cabinet_arrivals_use_the_same_coordinates() {
        for index in 0..70 {
            let view = view(index);
            assert!(walkable(view.translation));
            assert_eq!(nearest(view.translation, 70), Some(index));
            let slot = index % FOLDERS_PER_COURT / 2 * 4 + index % 2;
            assert_eq!(folder_at(court(index), slot), index);
            assert_eq!(folder_at(court(index), slot + 2), index);
            assert_eq!((rack(slot), rack(slot + 2)), (0, 1));
        }
    }

    #[test]
    fn the_courtyard_connects_to_rooms_through_their_doors() {
        for z in [-7.5, 7.5, COURT_LENGTH + 7.5] {
            assert!(walkable(Vec3::new(FRONT, EYE, z)));
        }
        assert!(!walkable(Vec3::new(FRONT, EYE, 0.)));
        assert!(!walkable(Vec3::new(10., EYE, 0.)));
        assert!(walkable(Vec3::new(0., EYE, 30.)));
    }
}
