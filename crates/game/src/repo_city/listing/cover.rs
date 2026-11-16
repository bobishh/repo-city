use bevy::{
    asset::{load_internal_asset, uuid_handle},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::{Shader, ShaderRef},
};

use super::{
    Face, Mark, TILE_ROWS,
    code_texture::{self, Coverage},
    syntax,
};

const SHADER: Handle<Shader> = uuid_handle!("8d39533e-ce71-4a9a-b51f-634d1cfe262d");
pub(super) type Material = ExtendedMaterial<StandardMaterial, Cover>;

pub(crate) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "cover.wesl", Shader::from_wesl);
        app.add_plugins(MaterialPlugin::<Material>::default());
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
    source_changes: [Vec4; 3],
    closed_accent: Vec4,
    closed_stroke: Vec4,
    spans: [Vec4; TILE_ROWS],
    paint: [Vec4; TILE_ROWS],
}

impl CoverRows {
    pub fn new(rows: &[(Option<syntax::Line>, Mark)], mask: u32) -> Self {
        let mut spans = [Vec4::ZERO; TILE_ROWS];
        for (slot, (line, mark)) in spans.iter_mut().zip(rows) {
            let (left, right) = line
                .as_ref()
                .filter(|line| !line.text.trim().is_empty())
                .map_or((0, 0), |line| code_texture::covered_span(&line.text));
            let kind = match mark {
                Mark::Context => 0.,
                Mark::Added => 1.,
                Mark::Removed => 2.,
            };
            *slot = Vec4::new(left as f32, right as f32, kind, 0.);
        }
        let rgb = |[r, g, b]: [u8; 3]| Vec4::new(f32::from(r), f32::from(g), f32::from(b), 0.);
        Self {
            glass: rgb(code_texture::FACADE_GLASS),
            source_changes: [Mark::Context, Mark::Added, Mark::Removed].map(|mark| {
                rgb(code_texture::source_change_ink(Face::Source, mark)
                    .unwrap_or(code_texture::FACADE_GLASS))
            }),
            closed_accent: rgb(Coverage::Closed.accent()),
            closed_stroke: rgb(Coverage::Closed.stroke()),
            info: UVec4::new(
                mask,
                rows.len() as u32,
                code_texture::LISTING_WIDTH,
                code_texture::LISTING_ROW_PIXELS,
            ),
            spans,
            paint: [Vec4::ZERO; TILE_ROWS],
        }
    }

    pub fn mask(&self) -> u32 {
        self.info.x
    }

    pub fn set_mask(&mut self, mask: u32) {
        self.info.x = mask;
    }

    pub fn set_paint(&mut self, paint: [Vec4; TILE_ROWS]) {
        self.paint = paint;
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
    fn cover_mask_uses_one_based_file_rows_and_local_tile_bits() {
        assert_eq!(mask(16..20, &|line| [17, 20, 21].contains(&line)), 0b1001);
        assert_eq!(mask(32..33, &|line| line == 33), 1);
    }
}
