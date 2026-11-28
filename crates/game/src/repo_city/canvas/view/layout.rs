use super::*;

pub(in super::super) struct Layout {
    pub(in super::super) viewport: Vec2,
    pub(in super::super) zoom: f32,
    pub(in super::super) pan: Vec2,
    pub(in super::super) top: f32,
    tiles: Vec<(Vec2, Vec2)>,
}

impl Layout {
    pub(in super::super) fn new(size: Vec2, model: &model::Canvas) -> Self {
        Self::for_view(size, model, false)
    }

    pub(in super::super) fn for_view(size: Vec2, model: &model::Canvas, overview: bool) -> Self {
        let columns = ((size.x - GAP) / (440. + GAP)).floor().max(1.);
        let default_width = ((size.x - GAP * (columns + 1.)) / columns).max(270.);
        let mut cursor = Vec2::splat(GAP);
        let mut row_height: f32 = 0.;
        let mut bounds = Vec2::ZERO;
        let mut tiles = model
            .tiles
            .iter()
            .map(|tile| {
                let [width, height] = tile.size;
                let extent = Vec2::new(
                    if width > 0. { width } else { default_width },
                    if height > 0. {
                        height
                    } else {
                        BASE_TILE_HEIGHT
                    },
                );
                if cursor.x > GAP && cursor.x + extent.x + GAP > size.x + 0.5 {
                    cursor = Vec2::new(GAP, cursor.y + row_height + GAP);
                    row_height = 0.;
                }
                let position = cursor;
                cursor.x += extent.x + GAP;
                row_height = row_height.max(extent.y);
                bounds = bounds.max(position + extent + GAP);
                (position, extent)
            })
            .collect::<Vec<_>>();
        let top = toolbar_height(size.x);
        let viewport = Vec2::new(size.x, (size.y - top).max(1.));
        let zoom = if overview {
            (viewport / bounds.max(Vec2::ONE)).min_element().min(1.)
        } else {
            model.zoom
        };
        for (position, extent) in &mut tiles {
            *position *= zoom;
            *extent *= zoom;
        }
        let pan = if overview {
            -(viewport - bounds * zoom).max(Vec2::ZERO) / 2.
        } else {
            Self::clamp(Vec2::from_array(model.pan))
        };
        Self {
            viewport,
            zoom,
            pan,
            top,
            tiles,
        }
    }

    pub(in super::super) fn clamp(pan: Vec2) -> Vec2 {
        pan.clamp(Vec2::splat(-1_000_000.), Vec2::splat(1_000_000.))
    }

    pub(in super::super) fn rect(&self, index: usize) -> (Vec2, Vec2) {
        let (position, extent) = self.tiles[index];
        (position - self.pan, extent)
    }

    pub(in super::super) fn reveal(&self, index: usize) -> Vec2 {
        let (position, extent) = self.tiles[index];
        let start = position;
        let end = start + (extent - self.viewport).min(Vec2::ZERO);
        Self::clamp(self.pan.clamp(end, start))
    }

    fn local(&self, point: Vec2, index: usize) -> Vec2 {
        point - self.rect(index).0 - Vec2::new(0., self.top)
    }

    pub(in super::super) fn tile_at(&self, point: Vec2) -> Option<usize> {
        let view_point = point - Vec2::new(0., self.top);
        if !view_point.cmpge(Vec2::ZERO).all() || !view_point.cmplt(self.viewport).all() {
            return None;
        }
        self.tiles
            .iter()
            .enumerate()
            .position(|(index, (_, extent))| {
                let local = self.local(point, index);
                local.cmpge(Vec2::ZERO).all() && local.cmplt(*extent).all()
            })
    }

    pub(in super::super) fn tile_body_at(&self, point: Vec2) -> Option<usize> {
        let index = self.tile_at(point)?;
        let y = self.local(point, index).y;
        (y >= TILE_HEADER && y < self.tiles[index].1.y - TILE_FOOTER).then_some(index)
    }

    pub(in super::super) fn row_at(&self, point: Vec2, index: usize) -> usize {
        ((self.local(point, index).y - TILE_HEADER - 1.) / (17. * self.zoom)).max(0.) as usize
    }

    pub(in super::super) fn grip_at(&self, point: Vec2, index: usize) -> bool {
        (self.tiles[index].1 - self.local(point, index))
            .cmple(Vec2::splat(16.))
            .all()
    }

    pub(in super::super) fn visible(&self, index: usize) -> bool {
        let (point, extent) = self.rect(index);
        point.cmplt(self.viewport).all() && (point + extent).cmpgt(Vec2::ZERO).all()
    }
}
