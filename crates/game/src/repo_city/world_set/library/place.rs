use serde::{Deserialize, Serialize};

use super::*;

#[derive(Deserialize, Serialize, PartialEq)]
pub(super) struct Place {
    path: String,
    before: bool,
    face: listing::Face,
    line: usize,
    position: [f32; 3],
    rotation: [f32; 4],
}

impl Place {
    pub(super) fn restore(self, repo: &Repository) -> Option<(Destination, Transform)> {
        let (index, file) = repo
            .files
            .iter()
            .enumerate()
            .find(|(_, file)| file.path == self.path)?;
        if self.before {
            file.before.as_ref()
        } else {
            file.after.as_ref()
        }?;
        let position = Vec3::from_array(self.position);
        let rotation = Quat::from_array(self.rotation);
        let pose = if position.is_finite()
            && layout::walkable(position)
            && (position.y - layout::EYE).abs() < 0.01
            && rotation.is_normalized()
        {
            Transform::from_translation(position).with_rotation(rotation)
        } else {
            room::entrance()
        };
        Some((
            Destination {
                index,
                before: self.before,
                line: self.line.max(1),
                end: None,
                face: self.face,
            },
            pose,
        ))
    }
}

pub(super) fn save(
    library: Res<Library>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    review: Res<review_notes::Review>,
    stage: Res<room::Unrolling>,
    world: Res<WorldSet>,
    time: Res<Time>,
    mut progress: ResMut<progress::Progress>,
    mut last: Local<(f64, String, Option<Place>)>,
) {
    if review.blocked
        || review.tablet_open
        || !stage.stationary()
        || time.elapsed_secs_f64() < last.0
    {
        return;
    }
    let (Some(reader), Some(pose), Some(file)) = (
        library.reader.as_ref(),
        library.walk_pose.as_ref(),
        repo.0.files.get(explorer.selected),
    ) else {
        return;
    };
    let rows = paper::Rows::new(&explorer, reader.face);
    let Some(row) = rows.row(reader.page(rows.len()).start) else {
        return;
    };
    let place = Place {
        path: file.path.clone(),
        before: row.before,
        face: reader.face,
        line: row.line.number,
        position: pose.translation.to_array(),
        rotation: pose.rotation.to_array(),
    };
    let scope = progress::comparison_scope(&repo.0);
    if last.1 == scope && last.2.as_ref() == Some(&place) {
        return;
    }
    last.0 = time.elapsed_secs_f64() + 0.5;
    if let Err(error) = progress.save_world_place(&repo.0, &world.id, &place) {
        warn!("Could not save the last library scroll: {error}");
    }
    last.1 = scope;
    last.2 = Some(place);
}
