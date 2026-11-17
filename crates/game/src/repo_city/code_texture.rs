//! Tree-sitter syntax colors and independent diff backgrounds.
use super::listing::Face;

use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use repo_world::{
    comparison::Mark,
    syntax::{Kind, Line},
};
use swash::{
    FontRef,
    scale::{Render, ScaleContext, Source},
};

pub fn render(
    title: &str,
    lines: &[(usize, String)],
    tint: [f32; 3],
    width: u32,
    height: u32,
) -> Image {
    let rows: Vec<_> = lines
        .iter()
        .map(|(n, text)| {
            (
                Some(Line {
                    number: *n,
                    text: text.clone(),
                    spans: vec![],
                }),
                Mark::Context,
            )
        })
        .collect();
    source(title, &rows, tint, width, height, 0)
}
fn ink(kind: Kind) -> [f32; 3] {
    match kind {
        Kind::Plain => [0.82, 0.87, 0.92],
        Kind::Keyword => [0.80, 0.57, 1.],
        Kind::String => [0.72, 0.86, 0.50],
        Kind::Comment => [0.48, 0.59, 0.66],
        Kind::Number => [1., 0.72, 0.40],
        Kind::Type => [0.45, 0.85, 0.91],
        Kind::Function => [0.53, 0.73, 1.],
    }
}
struct Canvas {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    context: ScaleContext,
}
impl Canvas {
    fn text(&mut self, text: &str, x: f32, y: f32, size: f32, color: impl Fn(usize) -> [f32; 3]) {
        self.scaled_text(text, x, y, size, Vec2::ONE, color);
    }

    fn scaled_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        scale: Vec2,
        color: impl Fn(usize) -> [f32; 3],
    ) {
        let font = FontRef::from_index(
            include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf"),
            0,
        )
        .unwrap();
        let metrics = font.glyph_metrics(&[]).scale(size);
        let mut scaler = self.context.builder(font).size(size).hint(true).build();
        let mut render = Render::new(&[Source::Outline]);
        if scale != Vec2::ONE {
            render.transform(Some(swash::zeno::Transform::scale(scale.x, scale.y)));
        }
        let mut x = x;
        for (byte, ch) in text.char_indices() {
            if x >= self.width as f32 - 14. {
                break;
            }
            if ch == '\t' {
                x += metrics.advance_width(font.charmap().map(' ')) * 4. * scale.x;
                continue;
            }
            let id = font.charmap().map(ch);
            let ink = color(byte);
            if let Some(glyph) = render.render(&mut scaler, id) {
                let p = glyph.placement;
                for gy in 0..p.height {
                    for gx in 0..p.width {
                        let coverage = glyph.data[(gy * p.width + gx) as usize] as f32 / 255.;
                        let px = x as i32 + p.left + gx as i32;
                        let py = y as i32 - p.top + gy as i32;
                        if px >= 0
                            && py >= 0
                            && (px as u32) < self.width
                            && (py as u32) < self.height
                        {
                            let offset = ((py as u32 * self.width + px as u32) * 4) as usize;
                            for c in 0..3 {
                                self.pixels[offset + c] = (self.pixels[offset + c] as f32
                                    * (1. - coverage)
                                    + ink[c] * 255. * coverage)
                                    as u8;
                            }
                        }
                    }
                }
            }
            x += metrics.advance_width(id) * scale.x;
        }
    }
    fn band(&mut self, start: u32, end: u32, rgb: [u8; 3]) {
        for y in start..end.min(self.height) {
            for x in 0..self.width {
                let p = ((y * self.width + x) * 4) as usize;
                self.pixels[p..p + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
}
pub fn source(
    title: &str,
    rows: &[(Option<Line>, Mark)],
    tint: [f32; 3],
    width: u32,
    height: u32,
    column: usize,
) -> Image {
    let mut canvas = Canvas {
        pixels: vec![0; (width * height * 4) as usize],
        width,
        height,
        context: ScaleContext::new(),
    };
    canvas.band(0, height, [9, 17, 23]);
    canvas.text(title, 22., 34., 24., |_| tint);
    for (i, (line, mark)) in rows
        .iter()
        .take((height.saturating_sub(65) / 27) as usize)
        .enumerate()
    {
        let y = 73. + i as f32 * 27.;
        if *mark != Mark::Context {
            canvas.band(
                (y - 22.) as u32,
                (y + 5.) as u32,
                if *mark == Mark::Added {
                    [17, 49, 37]
                } else {
                    [58, 24, 32]
                },
            );
        }
        if let Some(line) = line {
            let sign = match mark {
                Mark::Added => "+",
                Mark::Removed => "-",
                _ => " ",
            };
            canvas.text(&format!("{sign}{:>5}", line.number), 12., y, 21., |_| {
                [0.48, 0.60, 0.65]
            });
            let start = line
                .text
                .char_indices()
                .nth(column)
                .map_or(line.text.len(), |(i, _)| i);
            canvas.text(&line.text[start..], 110., y, 21., |byte| {
                ink(line
                    .spans
                    .iter()
                    .find(|s| s.bytes.contains(&(start + byte)))
                    .map_or(Kind::Plain, |s| s.kind))
            });
        }
    }
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas.pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

pub(super) const LISTING_WIDTH: u32 = 1536;
pub(super) const LISTING_ROW_PIXELS: u32 = 27;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Coverage {
    Closed,
    Partial,
    Open,
}

pub(super) const FACADE_GLASS: [u8; 3] = [23, 29, 40];

impl Coverage {
    pub(super) const ALL: [Self; 3] = [Self::Closed, Self::Partial, Self::Open];

    pub(super) fn accent(self) -> [u8; 3] {
        match self {
            Self::Closed => [130, 149, 184],
            Self::Partial => [197, 160, 240],
            Self::Open => [135, 237, 208],
        }
    }

    pub(super) fn stroke(self) -> [u8; 3] {
        match self {
            Self::Closed => [85, 103, 129],
            Self::Partial => [137, 121, 160],
            Self::Open => [155, 196, 193],
        }
    }
}

pub(super) fn embedded_png(png: &[u8]) -> Image {
    Image::from_buffer(
        png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::default(),
    )
    .expect("valid bundled city texture")
}

pub(super) fn facade_overview(bands: impl ExactSizeIterator<Item = Coverage>) -> Image {
    static PRINT: std::sync::OnceLock<Image> = std::sync::OnceLock::new();
    let print = PRINT
        .get_or_init(|| embedded_png(include_bytes!("../../../../assets/city/facade-print.png")));
    let mut image = print.clone();
    let width = image.width() as usize;
    let height = image.height() as usize;
    let count = bands.len();
    let bands: Vec<_> = bands.collect();
    let pixels = image.data.as_mut().expect("packed facade print");
    let gutter = width * 91 / LISTING_WIDTH as usize;
    for (y, row) in pixels.chunks_exact_mut(width * 4).enumerate() {
        let state = bands[y * count / height];
        let accent = state.accent();
        let stroke = state.stroke();
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let rgb = if (gutter..gutter + 2).contains(&x) {
                accent
            } else if pixel[0] > 0 {
                stroke
            } else {
                FACADE_GLASS
            };
            pixel[..3].copy_from_slice(&rgb);
        }
    }
    image
}

pub(super) fn listing_preview<'a>(
    count: usize,
    row: impl Fn(usize) -> (Option<&'a Line>, Mark),
    revealed: impl Fn(usize) -> bool,
    face: Face,
    glyph_scale: Vec2,
) -> Image {
    const WIDTH: usize = 256;
    let height = (count * 4).clamp(64, 2048);
    let mut pixels = vec![0; WIDTH * height * 4];
    for (y, pixels) in pixels.chunks_exact_mut(WIDTH * 4).enumerate() {
        let index = y * count / height;
        let (line, mark) = row(index);
        let open = line.is_none_or(|line| revealed(line.number));
        let coverage = if open {
            Coverage::Open
        } else {
            Coverage::Closed
        };
        let background = listing_background(face, mark);
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[background[0], background[1], background[2], 255]);
        }
        let gutter = WIDTH * 91 / LISTING_WIDTH as usize;
        pixels[gutter * 4..gutter * 4 + 3].copy_from_slice(&coverage.accent());
        if let Some(ink) = source_change_ink(face, mark) {
            let change = WIDTH * 102 / LISTING_WIDTH as usize * 4;
            pixels[change..change + 3].copy_from_slice(&ink);
        }
        if let Some(line) = line.filter(|line| !line.text.trim().is_empty()) {
            let start_y = (index * height).div_ceil(count);
            let end_y = ((index + 1) * height).div_ceil(count);
            if y != (start_y + end_y - 1) / 2 {
                continue;
            }
            let stroke = coverage.stroke();
            let (start, end) = covered_span(&line.text, glyph_scale.x);
            let start = start * WIDTH / LISTING_WIDTH as usize;
            let end = (end * WIDTH / LISTING_WIDTH as usize).max(start + 1);
            for x in start..end {
                pixels[x * 4..x * 4 + 3].copy_from_slice(&stroke);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: WIDTH as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

fn listing_background(face: Face, mark: Mark) -> [u8; 3] {
    match (face, mark) {
        (Face::Diff, Mark::Added) => [17, 56, 42],
        (Face::Diff, Mark::Removed) => [66, 27, 35],
        _ => FACADE_GLASS,
    }
}

pub(super) fn source_change_ink(face: Face, mark: Mark) -> Option<[u8; 3]> {
    match (face, mark) {
        (Face::Source, Mark::Added) => Some([74, 177, 129]),
        (Face::Source, Mark::Removed) => Some([208, 107, 128]),
        _ => None,
    }
}

pub(super) fn covered_span(text: &str, x_scale: f32) -> (usize, usize) {
    let indent = (text.len() - text.trim_start().len()).min(40);
    let start = 106 + (indent as f32 * 12. * x_scale) as usize;
    let end = (106 + (text.len().min(128) as f32 * 12. * x_scale) as usize)
        .min((LISTING_WIDTH - 16) as usize);
    (start, end)
}

pub fn listing(rows: &[(Option<Line>, Mark)], face: Face, glyph_scale: Vec2) -> Image {
    listing_with_notes(rows, face, glyph_scale, None)
}

pub(super) fn listing_with_notes(
    rows: &[(Option<Line>, Mark)],
    face: Face,
    glyph_scale: Vec2,
    notes: Option<&[String]>,
) -> Image {
    let pixel_density = |scale: f32, limit: f32| (1. / scale).round().clamp(1., limit) as u32;
    let x_density = pixel_density(glyph_scale.x, 2.);
    let y_density = pixel_density(glyph_scale.y, 4.);
    let width = LISTING_WIDTH * x_density;
    let row_pixels = LISTING_ROW_PIXELS * y_density;
    let height = rows.len() as u32 * row_pixels;
    let x = |position: f32| position * x_density as f32;
    let glyph_scale = glyph_scale * Vec2::new(x_density as f32, y_density as f32);
    let mut canvas = Canvas {
        pixels: vec![0; (width * height * 4) as usize],
        width,
        height,
        context: ScaleContext::new(),
    };
    for (index, (line, mark)) in rows.iter().enumerate() {
        let y = index as u32 * row_pixels;
        canvas.band(y, y + row_pixels, listing_background(face, *mark));
        let accent = Coverage::Open.accent();
        let change = source_change_ink(face, *mark);
        for py in y..y + row_pixels {
            for x in 91 * x_density..96 * x_density {
                let pixel = ((py * width + x) * 4) as usize;
                canvas.pixels[pixel..pixel + 3].copy_from_slice(&accent);
            }
            if let Some(ink) = change {
                for x in 101 * x_density..105 * x_density {
                    let pixel = ((py * width + x) * 4) as usize;
                    canvas.pixels[pixel..pixel + 3].copy_from_slice(&ink);
                }
            }
        }
        let Some(line) = line else { continue };
        let row_center = row_pixels as f32 / 2.;
        let baseline =
            y as f32 + row_center + (21. - LISTING_ROW_PIXELS as f32 / 2.) * glyph_scale.y;
        canvas.scaled_text(
            &format!("{:>6}", line.number),
            x(84.) - 72. * glyph_scale.x,
            baseline,
            20.,
            glyph_scale,
            |_| [0.50, 0.66, 0.71],
        );
        let text_start = if let Some(notes) = notes {
            if let Some(author) = notes.get(line.number.saturating_sub(1)) {
                let (limit, size) = if cfg!(target_arch = "wasm32") {
                    (39, 15.)
                } else {
                    (28, 17.)
                };
                let short = author.chars().take(limit).collect::<String>();
                canvas.scaled_text(&short, x(106.), baseline, size, glyph_scale, |_| {
                    [0.53, 0.83, 0.78]
                });
            }
            if cfg!(target_arch = "wasm32") {
                470
            } else {
                405
            }
        } else {
            106
        };
        let start = x(106.) + (text_start as f32 - 106.) * glyph_scale.x;
        canvas.scaled_text(&line.text, start, baseline, 20., glyph_scale, |byte| {
            ink(line
                .spans
                .iter()
                .find(|span| span.bytes.contains(&byte))
                .map_or(Kind::Plain, |span| span.kind))
        });
    }
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas.pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_wall_glyphs_keep_enough_texels_and_the_same_normalized_gutter() {
        let rows = [(
            Some(Line {
                number: 1,
                text: "HELLO".into(),
                spans: Vec::new(),
            }),
            Mark::Context,
        )];
        for (scale, density) in [
            (Vec2::ONE, UVec2::ONE),
            (Vec2::new(0.5, 1.), UVec2::new(2, 1)),
            (Vec2::new(1., 0.25), UVec2::new(1, 4)),
        ] {
            let image = listing(&rows, Face::Source, scale);
            assert_eq!(image.width(), LISTING_WIDTH * density.x);
            assert_eq!(image.height(), LISTING_ROW_PIXELS * density.y);
            let data = image.data.unwrap();
            let gutter = (91 * density.x * 4) as usize;
            assert_eq!(&data[gutter..gutter + 3], &Coverage::Open.accent());
            let width = image.texture_descriptor.size.width as usize;
            let bright_rows = data
                .chunks_exact(width * 4)
                .filter(|row| {
                    row[106 * density.x as usize * 4..]
                        .chunks_exact(4)
                        .any(|p| p[0] > 160)
                })
                .count();
            assert!(
                bright_rows >= 12,
                "{scale:?} only used {bright_rows} texels"
            );
        }
    }
}
