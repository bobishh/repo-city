use super::*;

pub(in super::super) struct Layout {
    pub(in super::super) viewport: Vec2,
    pub(in super::super) zoom: f32,
    pub(in super::super) pan: Vec2,
    pub(in super::super) top: f32,
    overview: Option<Vec<(Vec2, Vec2)>>,
}

impl Layout {
    pub(in super::super) fn new(size: Vec2, model: &model::Canvas) -> Self {
        Self::for_view(size, model, false)
    }

    pub(in super::super) fn for_view(size: Vec2, model: &model::Canvas, overview: bool) -> Self {
        let top = toolbar_height(size.x);
        let viewport = Vec2::new(size.x, (size.y - top).max(1.));
        if overview {
            return Self::overview(viewport, top, model.tiles.len());
        }
        Self {
            viewport,
            zoom: model.zoom,
            pan: Self::clamp(Vec2::from_array(model.pan)),
            top,
            overview: None,
        }
    }

    fn overview(viewport: Vec2, top: f32, count: usize) -> Self {
        let metrics = |rows: usize| {
            let columns = count.max(1).div_ceil(rows);
            let grid = Vec2::new(columns as f32, rows as f32);
            let gap = (viewport / ((grid + 1.) * 3.)).min_element().min(10.);
            let cell = (viewport - (grid + 1.) * gap) / grid;
            let scale = (cell / Vec2::new(370., 144.)).min_element();
            (scale, gap, cell.y)
        };
        let rows = (1..=count.max(1))
            .max_by(|&left, &right| metrics(left).0.total_cmp(&metrics(right).0))
            .unwrap_or(1);
        let (scale, gap, height) = metrics(rows);
        let mut tiles = Vec::with_capacity(count);
        for row in 0..rows {
            let columns = count / rows + usize::from(row < count % rows);
            if columns == 0 {
                break;
            }
            let width = (viewport.x - gap * (columns + 1) as f32) / columns as f32;
            let y = gap + row as f32 * (height + gap);
            tiles.extend((0..columns).map(|column| {
                let x = gap + column as f32 * (width + gap);
                (Vec2::new(x, y), Vec2::new(width, height))
            }));
        }
        Self {
            viewport,
            zoom: scale.clamp(0.01, 1.25),
            pan: Vec2::ZERO,
            top,
            overview: Some(tiles),
        }
    }

    pub(in super::super) fn clamp(pan: Vec2) -> Vec2 {
        pan.clamp(Vec2::splat(-1_000_000.), Vec2::splat(1_000_000.))
    }

    pub(in super::super) fn rect(&self, tiles: &[model::Tile], index: usize) -> (Vec2, Vec2) {
        if let Some(overview) = &self.overview {
            return overview[index];
        }
        let rect = tiles[index].rect().expect("placed canvas pane");
        (rect.origin * self.zoom - self.pan, rect.size * self.zoom)
    }

    pub(in super::super) fn reveal(&self, tiles: &[model::Tile], index: usize) -> Vec2 {
        let (position, extent) = self.rect(tiles, index);
        let start = position + self.pan;
        let end = start + (extent - self.viewport).min(Vec2::ZERO);
        Self::clamp(self.pan.clamp(end, start))
    }

    fn local(&self, point: Vec2, tiles: &[model::Tile], index: usize) -> (Vec2, Vec2) {
        let (origin, extent) = self.rect(tiles, index);
        (point - origin - Vec2::new(0., self.top), extent)
    }

    pub(in super::super) fn tile_at(&self, point: Vec2, tiles: &[model::Tile]) -> Option<usize> {
        let view_point = point - Vec2::new(0., self.top);
        if !view_point.cmpge(Vec2::ZERO).all() || !view_point.cmplt(self.viewport).all() {
            return None;
        }
        (0..tiles.len()).find(|&index| {
            let (local, extent) = self.local(point, tiles, index);
            local.cmpge(Vec2::ZERO).all() && local.cmplt(extent).all()
        })
    }

    pub(in super::super) fn tile_body_at(
        &self,
        point: Vec2,
        tiles: &[model::Tile],
    ) -> Option<usize> {
        let index = self.tile_at(point, tiles)?;
        let (local, extent) = self.local(point, tiles, index);
        (self.zoom >= 0.42 && local.y >= TILE_HEADER && local.y < extent.y - TILE_FOOTER)
            .then_some(index)
    }

    pub(in super::super) fn row_at(
        &self,
        point: Vec2,
        tiles: &[model::Tile],
        index: usize,
    ) -> usize {
        ((self.local(point, tiles, index).0.y - TILE_HEADER - 1.) / (17. * self.zoom)).max(0.)
            as usize
    }

    pub(in super::super) fn divider_at(
        &self,
        point: Vec2,
        tiles: &[model::Tile],
    ) -> Option<(usize, model::Axis)> {
        if self.zoom < 0.42 || point.y < self.top {
            return None;
        }
        let tolerance = 6.;
        for index in 0..tiles.len() {
            let (local, extent) = self.local(point, tiles, index);
            if (local.x - extent.x).abs() <= tolerance
                && (TILE_HEADER..extent.y - TILE_FOOTER).contains(&local.y)
            {
                return Some((index, model::Axis::Horizontal));
            }
            if (local.y - extent.y).abs() <= tolerance
                && (tolerance..extent.x - tolerance).contains(&local.x)
            {
                return Some((index, model::Axis::Vertical));
            }
        }
        None
    }

    pub(in super::super) fn visible(&self, tiles: &[model::Tile], index: usize) -> bool {
        let (point, extent) = self.rect(tiles, index);
        point.cmplt(self.viewport).all() && (point + extent).cmpgt(Vec2::ZERO).all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_uses_both_dimensions_without_clipping_tiles() {
        let columns = |viewport| {
            let layout = Layout::overview(viewport, 44., 25);
            let tiles = layout.overview.unwrap();
            let first_row = tiles[0].0.y;
            let mut area = 0.;
            for &(position, size) in &tiles {
                area += size.x * size.y;
                assert!(position.cmpge(Vec2::ZERO).all());
                assert!((position + size).cmple(viewport + Vec2::splat(0.01)).all());
            }
            assert!(area / (viewport.x * viewport.y) > 0.84);
            let (last, size) = tiles[24];
            assert!((last + size).abs_diff_eq(viewport - tiles[0].0.x, 0.01));
            tiles
                .iter()
                .take_while(|(origin, _)| origin.y == first_row)
                .count()
        };
        assert!(columns(Vec2::new(1900., 720.)) > columns(Vec2::new(760., 1150.)));
    }
}
