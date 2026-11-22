use bevy::{
    asset::{load_internal_asset, uuid_handle},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::{Shader, ShaderRef},
};

use super::code_texture::{Coverage, FacadePrint, LISTING_WIDTH};

const SHADER: Handle<Shader> = uuid_handle!("fc8dfe74-5620-4270-aacb-b7eb078d8400");
pub(super) type Material = ExtendedMaterial<StandardMaterial, ReadingBands>;

pub(super) fn install(app: &mut App) {
    load_internal_asset!(app, SHADER, "overview.wesl", Shader::from_wesl);
    app.add_plugins(MaterialPlugin::<Material>::default());
}

#[derive(Asset, AsBindGroup, Reflect, Clone)]
pub(crate) struct ReadingBands {
    #[uniform(100)]
    states: UVec4,
    #[uniform(100)]
    geometry: Vec4,
    #[uniform(100)]
    partial_ink: Vec4,
    #[texture(101)]
    #[sampler(102)]
    open: Handle<Image>,
}

impl ReadingBands {
    pub(super) fn new(
        open: Handle<Image>,
        print: FacadePrint,
        rows: usize,
        bands: impl ExactSizeIterator<Item = Coverage>,
    ) -> Self {
        let count = bands.len();
        let mut states = UVec4::ZERO;
        for (index, state) in bands.enumerate() {
            states[index / 16] |= (state as u32) << (index % 16 * 2);
        }
        let gutter = 256 * 91 / LISTING_WIDTH;
        let [r, g, b] = Coverage::Partial.accent();
        Self {
            states,
            geometry: Vec4::new(
                rows as f32 / print.atlas_height() as f32,
                count as f32,
                gutter as f32 / 256.,
                (gutter + 2) as f32 / 256.,
            ),
            partial_ink: LinearRgba::from(Color::srgb_u8(r, g, b)).to_vec4(),
            open,
        }
    }

    #[cfg(test)]
    pub(super) fn state(&self, index: usize) -> u32 {
        self.states[index / 16] >> (index % 16 * 2) & 3
    }

    #[cfg(test)]
    pub(super) fn repeats(&self) -> f32 {
        self.geometry.x
    }
}

impl MaterialExtension for ReadingBands {
    fn enable_prepass() -> bool {
        false
    }
    fn enable_oit() -> bool {
        false
    }
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}
