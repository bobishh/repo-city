//! Reading a station lights its full wall. Credit follows the focused, resident rows.
use super::*;

pub(super) fn ready_rows(
    rows: Range<usize>,
    focus: usize,
    ready: impl Fn(usize, usize) -> bool,
) -> Range<usize> {
    if !rows.contains(&focus) || !ready(focus, focus) {
        return 0..0;
    }
    let mut first = focus;
    let mut end = focus + 1;
    while first > rows.start && ready(first - 1, first) {
        first -= 1;
    }
    while end < rows.end && ready(end, end - 1) {
        end += 1;
    }
    first..end
}

pub(super) fn source(
    listing: &mut Listing,
    explorer: &Explorer,
    file: &repo_world::File,
    camera: &Transform,
    aspect: f32,
    delta: f32,
    settings: reading::Settings,
) -> Option<progress::Mark> {
    let count = explorer.source_layout.len();
    let side = WallSide::new(file, count, Face::Source, block_size(file));
    let Some(hit) = listing.readable_hit(explorer, &side, camera, count) else {
        listing.stop_water();
        return None;
    };
    let focus = side.wall.row(hit.y, count);
    let visible = side.visible_rows(camera, count, aspect);
    let group = settings.block(explorer, focus);
    let rows = ready_rows(
        group.start.max(visible.start)..group.end.min(visible.end),
        focus,
        |row, previous| {
            listing.source_tiles.ready(row)
                && explorer.source_layout.rows[row]
                    .number
                    .abs_diff(explorer.source_layout.rows[previous].number)
                    <= 1
        },
    );
    listing.advance_wash_rows(explorer, file, rows, delta, settings)
}
