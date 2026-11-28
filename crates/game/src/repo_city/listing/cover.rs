use bevy::{
    asset::{load_internal_asset, uuid_handle},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::{Shader, ShaderRef},
};

use super::{TILE_ROWS, code_texture};

const SHADER: Handle<Shader> = uuid_handle!("8d39533e-ce71-4a9a-b51f-634d1cfe262d");
pub(super) type Material = ExtendedMaterial<StandardMaterial, Cover>;

pub(crate) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "cover.wesl", Shader::from_wesl);
        app.add_plugins(MaterialPlugin::<Material>::default());
        super::overview::install(app);
    }
}

#[derive(Asset, AsBindGroup, Reflect, Clone)]
pub(crate) struct Cover {
    #[uniform(100)]
    pub(super) data: CoverRows,
}

impl MaterialExtension for Cover {
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn enable_oit() -> bool {
        false
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Reflect, ShaderType)]
pub(super) struct CoverRows {
    info: UVec4,
    glass: Vec4,
    glow: [Vec4; TILE_ROWS / 4],
    paint: [Vec4; TILE_ROWS],
    clarity: [Vec4; TILE_ROWS / 4],
}

impl CoverRows {
    pub fn new(rows: usize, mask: u32) -> Self {
        let [r, g, b] = code_texture::FACADE_GLASS;
        Self {
            glass: Vec4::new(f32::from(r), f32::from(g), f32::from(b), 0.),
            glow: [Vec4::ZERO; TILE_ROWS / 4],
            info: UVec4::new(mask, rows as u32, code_texture::LISTING_WIDTH, 0),
            paint: [Vec4::ZERO; TILE_ROWS],
            clarity: std::array::from_fn(|group| {
                Vec4::from_array(std::array::from_fn(|offset| {
                    f32::from((mask >> (group * 4 + offset)) & 1 != 0)
                }))
            }),
        }
    }

    pub fn mask(&self) -> u32 {
        self.info.x
    }

    pub fn set_mask(&mut self, mask: u32) {
        self.info.x = mask;
    }

    #[cfg(test)]
    fn clarity(&self, row: usize) -> f32 {
        self.clarity[row / 4][row % 4]
    }

    pub fn advance(&mut self, seconds: f32) {
        let step = (seconds.max(0.) / 0.18).min(1.);
        for row in 0..TILE_ROWS {
            let target = f32::from((self.mask() >> row) & 1 != 0);
            let value = &mut self.clarity[row / 4][row % 4];
            *value += (target - *value).clamp(-step, step);
        }
    }

    pub fn set_glow(&mut self, strength: [f32; TILE_ROWS]) {
        for (row, value) in strength.into_iter().enumerate() {
            self.glow[row / 4][row % 4] = value;
        }
    }

    pub fn set_paint(&mut self, paint: [Vec4; TILE_ROWS]) {
        self.paint = paint;
    }

    pub fn set_station_light(&mut self, phosphor: u32, close: bool) {
        self.info.w = phosphor;
        self.glass.w = if close { 1. } else { -1. };
    }
}

pub(super) fn mask(rows: std::ops::Range<usize>, revealed: &impl Fn(usize) -> bool) -> u32 {
    rows.enumerate().fold(0, |bits, (tile_row, file_row)| {
        bits | ((revealed(file_row + 1) as u32) << tile_row)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_row_melts_its_cover_instead_of_switching_its_pixels_at_once() {
        let mut data = CoverRows::new(2, 0);
        data.set_mask(1);
        assert_eq!(
            data.mask(),
            1,
            "readability changes independently of presentation"
        );
        assert_eq!(
            data.clarity(0),
            0.,
            "the old cover starts the short melt animation"
        );
        assert_eq!(data.clarity(1), 0., "neighboring unread row stays frosted");
        data.advance(0.09);
        assert!((data.clarity(0) - 0.5).abs() < 0.001);
        data.advance(0.09);
        assert_eq!(data.clarity(0), 1.);
        assert_eq!(data.clarity(1), 0.);
        data.set_mask(0);
        data.advance(0.18);
        assert_eq!(
            data.clarity(0),
            0.,
            "an unfinished preview freezes again on leaving"
        );
    }

    #[test]
    fn cover_mask_uses_one_based_file_rows_and_local_tile_bits() {
        assert_eq!(mask(16..20, &|line| [17, 20, 21].contains(&line)), 0b1001);
        assert_eq!(mask(32..33, &|line| line == 33), 1);
    }
}
