//! Durable pane rectangles. Window dimensions seed a new layout once.
use bevy::math::Vec2;

use super::{Canvas, Tile};

pub(in super::super) const GAP: f32 = 12.;
pub(in super::super) const MIN_PANE: Vec2 = Vec2::new(280., 175.);
const MAX_PANE: f32 = 2400.;
const DEFAULT_HEIGHT: f32 = 394.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in super::super) struct Rect {
    pub origin: Vec2,
    pub size: Vec2,
}

impl Rect {
    pub fn end(self) -> Vec2 {
        self.origin + self.size
    }
}

#[derive(Clone, Copy)]
pub(in super::super) enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub fn along(self) -> usize {
        match self {
            Self::Horizontal => 0,
            Self::Vertical => 1,
        }
    }

    fn across(self) -> usize {
        1 - self.along()
    }
}

impl Tile {
    pub fn rect(&self) -> Option<Rect> {
        Some(Rect {
            origin: self.origin?.into(),
            size: self.size.into(),
        })
    }
}

impl Canvas {
    pub fn place_tiles(&mut self, window_width: f32) -> bool {
        if self.tiles.iter().all(|tile| tile.origin.is_some()) {
            return false;
        }
        let width = *self
            .layout_width
            .get_or_insert(window_width.max(MIN_PANE.x + 2. * GAP));
        let columns = ((width - GAP) / (440. + GAP)).floor().max(1.);
        let default_width = ((width - (columns + 1.) * GAP) / columns).max(MIN_PANE.x);
        let rects = || self.tiles.iter().filter_map(Tile::rect);
        let mut row = rects().map(|rect| rect.origin.y).fold(GAP, f32::max);
        let mut right = rects()
            .filter(|rect| rect.end().y > row)
            .map(|rect| rect.end().x + GAP)
            .fold(GAP, f32::max);
        let mut bottom = rects().map(|rect| rect.end().y + GAP).fold(GAP, f32::max);
        for tile in self.tiles.iter_mut().filter(|tile| tile.origin.is_none()) {
            let [saved_width, saved_height] = tile.size;
            let size = Vec2::new(
                if saved_width > 0. {
                    saved_width
                } else {
                    default_width
                },
                if saved_height > 0. {
                    saved_height
                } else {
                    DEFAULT_HEIGHT
                },
            )
            .clamp(MIN_PANE, Vec2::splat(MAX_PANE));
            if right > GAP && right + size.x + GAP > width + 0.5 {
                row = bottom;
                right = GAP;
            }
            tile.origin = Some([right, row]);
            tile.size = size.to_array();
            right += size.x + GAP;
            bottom = bottom.max(row + size.y + GAP);
        }
        true
    }

    pub fn resize(&mut self, path: &str, axis: Axis, requested: f32) -> bool {
        let Some(index) = self.tiles.iter().position(|tile| tile.path == path) else {
            return false;
        };
        let Some(rect) = self.tiles[index].rect() else {
            return false;
        };
        let along = axis.along();
        let across = axis.across();
        let seam = rect.end()[along] + GAP;
        let same = |a: f32, b: f32| (a - b).abs() <= 0.01;
        let mut divider = None;
        let mut maximum = MAX_PANE;
        for (other_index, other) in self.tiles.iter().enumerate() {
            let Some(other) = other.rect() else { continue };
            if other_index == index || other.origin[along] < seam - 0.01 {
                continue;
            }
            if same(other.origin[along], seam)
                && same(other.origin[across], rect.origin[across])
                && same(other.end()[across], rect.end()[across])
            {
                divider = Some((other_index, other));
                break;
            }
            if rect.origin[across] < other.end()[across]
                && other.origin[across] < rect.end()[across]
            {
                maximum = maximum.min(other.origin[along] - GAP - rect.origin[along]);
            }
        }
        let current = rect.size[along];
        let min = MIN_PANE[along];
        let size = if let Some((other_index, other)) = divider {
            let total = current + other.size[along];
            let size = requested.clamp(min.max(total - MAX_PANE), MAX_PANE.min(total - min));
            let mut origin = other.origin;
            origin[along] += size - current;
            self.tiles[other_index].origin = Some(origin.to_array());
            self.tiles[other_index].size[along] = total - size;
            size
        } else {
            requested.clamp(min, maximum.max(min))
        };
        self.tiles[index].size[along] = size;
        size != current
    }
}
