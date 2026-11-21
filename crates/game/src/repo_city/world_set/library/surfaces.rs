use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::VertexAttributeValues,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::*;

const METERS_PER_TILE: f32 = 4.8;
const TILE: u32 = 128;

#[derive(Clone, Copy)]
pub(super) enum Stone {
    Limestone,
    Paving,
}

impl Stone {
    pub(super) fn material(self, pigment: Color, images: &mut Assets<Image>) -> StandardMaterial {
        StandardMaterial {
            base_color: pigment,
            base_color_texture: Some(images.add(self.image())),
            perceptual_roughness: 0.95,
            reflectance: 0.27,
            ..default()
        }
    }

    fn image(self) -> Image {
        let mut image = Image::new_uninit(
            Extent3d {
                width: TILE,
                height: TILE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        let mut level = (0..TILE * TILE)
            .map(|i| {
                let uv = Vec2::new((i % TILE) as f32 + 0.5, (i / TILE) as f32 + 0.5) / TILE as f32;
                let shade = 0.90 + 0.11 * noise(uv, 9, 31) + 0.056 * noise(uv, 37, 67);
                let tint = match self {
                    Self::Limestone => shade,
                    Self::Paving => {
                        let row = uv.y * 4.;
                        let column = (uv.x + (row.floor() % 2.) * 0.25) * 2.;
                        let edge = (row.fract().min(1. - row.fract()) * 0.5)
                            .min(column.fract().min(1. - column.fract()));
                        let seam = (1. - edge * 49.).clamp(0., 1.);
                        shade - 0.23 * seam - 0.052 * grain((column as i32) % 2, row as i32, 83)
                    }
                };
                (tint.clamp(0., 1.) * 255.) as u8
            })
            .collect::<Vec<_>>();
        let mut data = Vec::new();
        let mut width = TILE as usize;
        loop {
            data.extend(level.iter().flat_map(|&luma| [luma, luma, luma, 255]));
            if width == 1 {
                break;
            }
            let half = width / 2;
            level = (0..half * half)
                .map(|i| {
                    let top = (i / half * width + i % half) * 2;
                    let value = u16::from(level[top])
                        + u16::from(level[top + 1])
                        + u16::from(level[top + width])
                        + u16::from(level[top + width + 1]);
                    (value / 4) as u8
                })
                .collect();
            width = half;
        }
        image.data = Some(data);
        image.texture_descriptor.mip_level_count = TILE.ilog2() + 1;
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            anisotropy_clamp: 4,
            ..ImageSamplerDescriptor::linear()
        });
        image
    }

    pub(super) fn block(size: Vec3) -> Mesh {
        let mut mesh = Mesh::from(Cuboid::from_size(size));
        let uvs = match (
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        ) {
            (
                Some(VertexAttributeValues::Float32x3(positions)),
                Some(VertexAttributeValues::Float32x3(normals)),
            ) => positions
                .iter()
                .zip(normals)
                .map(|(&[x, y, z], &[nx, ny, _])| {
                    let coordinates = if ny.abs() > 0.5 {
                        [x, z]
                    } else if nx.abs() > 0.5 {
                        [z, y]
                    } else {
                        [x, y]
                    };
                    coordinates.map(|value| value / METERS_PER_TILE)
                })
                .collect::<Vec<_>>(),
            _ => unreachable!("cuboids have positions and normals"),
        };
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh
    }
}

fn grain(x: i32, y: i32, seed: u32) -> f32 {
    let salt = (x as u32).wrapping_mul(0x45d9f3b) ^ (y as u32).wrapping_mul(0x27d4eb2d) ^ seed;
    let bits = (salt ^ (salt >> 16)).wrapping_mul(0x7feb352d);
    ((bits ^ (bits >> 15)) & 255) as f32 / 255.
}

fn noise(uv: Vec2, cells: i32, seed: u32) -> f32 {
    let point = uv * cells as f32;
    let corner = point.floor().as_ivec2();
    let fraction = point - point.floor();
    let weight = fraction * fraction * (Vec2::splat(3.) - fraction * 2.);
    let sample = |x: i32, y: i32| {
        grain(
            (corner.x + x).rem_euclid(cells),
            (corner.y + y).rem_euclid(cells),
            seed,
        )
    };
    sample(0, 0)
        .lerp(sample(1, 0), weight.x)
        .lerp(sample(0, 1).lerp(sample(1, 1), weight.x), weight.y)
}
