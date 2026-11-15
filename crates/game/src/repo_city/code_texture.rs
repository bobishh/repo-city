//! Tree-sitter syntax colors and independent diff backgrounds.
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
        let font = FontRef::from_index(
            include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf"),
            0,
        )
        .unwrap();
        let metrics = font.glyph_metrics(&[]).scale(size);
        let mut scaler = self.context.builder(font).size(size).hint(true).build();
        let mut x = x;
        for (byte, ch) in text.char_indices() {
            if x >= self.width as f32 - 14. {
                break;
            }
            if ch == '\t' {
                x += metrics.advance_width(font.charmap().map(' ')) * 4.;
                continue;
            }
            let id = font.charmap().map(ch);
            let ink = color(byte);
            if let Some(glyph) = Render::new(&[Source::Outline]).render(&mut scaler, id) {
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
            x += metrics.advance_width(id);
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
    for (y, row) in pixels.chunks_exact_mut(width * 4).enumerate() {
        let [r, g, b]: [u8; 3] = match bands[y * count / height] {
            Coverage::Closed => [0, 0, 0],
            Coverage::Partial => [17, 8, 0],
            Coverage::Open => [0, 16, 12],
        };
        if r | g | b != 0 {
            for pixel in row.chunks_exact_mut(4) {
                pixel[0] = pixel[0].saturating_add(r);
                pixel[1] = pixel[1].saturating_add(g);
                pixel[2] = pixel[2].saturating_add(b);
            }
        }
    }
    image
}

pub(super) fn listing_preview<'a>(
    count: usize,
    row: impl Fn(usize) -> (Option<&'a Line>, Mark),
    revealed: impl Fn(usize) -> bool,
) -> Image {
    const WIDTH: usize = 128;
    let height = (count * 2).clamp(64, 2048);
    let mut pixels = vec![0; WIDTH * height * 4];
    for (y, pixels) in pixels.chunks_exact_mut(WIDTH * 4).enumerate() {
        let index = y * count / height;
        let (line, mark) = row(index);
        let open = line.is_none_or(|line| revealed(line.number));
        let background = listing_background(open, mark);
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[background[0], background[1], background[2], 255]);
        }
        if let Some(line) = line.filter(|line| !line.text.trim().is_empty()) {
            let start_y = (index * height).div_ceil(count);
            let end_y = ((index + 1) * height).div_ceil(count);
            if y != (start_y + end_y - 1) / 2 {
                continue;
            }
            let mark = if open {
                [113, 135, 146, 255]
            } else {
                [68, 91, 101, 255]
            };
            let (start, end) = covered_span(&line.text);
            let start = start * WIDTH / LISTING_WIDTH as usize;
            let end = (end * WIDTH / LISTING_WIDTH as usize).max(start + 1);
            for x in start..end {
                pixels[x * 4..x * 4 + 4].copy_from_slice(&mark);
            }
        }
    }
    Image::new(
        Extent3d {
            width: WIDTH as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn listing_background(open: bool, mark: Mark) -> [u8; 3] {
    match (open, mark) {
        (false, _) => [28, 43, 50],
        (_, Mark::Added) => [17, 56, 42],
        (_, Mark::Removed) => [66, 27, 35],
        _ => [9, 17, 23],
    }
}

fn covered_span(text: &str) -> (usize, usize) {
    let indent = (text.len() - text.trim_start().len()).min(40);
    let start = 106 + indent * 12;
    let end = (106 + text.len().min(128) * 12).min((LISTING_WIDTH - 16) as usize);
    (start, end)
}

pub fn listing(rows: &[(Option<Line>, Mark)], revealed: &impl Fn(usize) -> bool) -> Image {
    let width = LISTING_WIDTH;
    let height = rows.len() as u32 * LISTING_ROW_PIXELS;
    let mut canvas = Canvas {
        pixels: vec![0; (width * height * 4) as usize],
        width,
        height,
        context: ScaleContext::new(),
    };
    for (index, (line, mark)) in rows.iter().enumerate() {
        let y = index as u32 * LISTING_ROW_PIXELS;
        let open = line.as_ref().is_none_or(|line| revealed(line.number));
        let background = listing_background(open, *mark);
        canvas.band(y, y + LISTING_ROW_PIXELS, background);
        if !open {
            for py in y..y + LISTING_ROW_PIXELS {
                for x in 91..96 {
                    let pixel = ((py * width + x) * 4) as usize;
                    let stripe = if (py + x) % 12 < 5 {
                        [83, 148, 155, 255]
                    } else {
                        [35, 69, 77, 255]
                    };
                    canvas.pixels[pixel..pixel + 4].copy_from_slice(&stripe);
                }
            }
        }
        let Some(line) = line else { continue };
        let baseline = y as f32 + 21.;
        canvas.text(&format!("{:>6}", line.number), 12., baseline, 20., |_| {
            [0.50, 0.66, 0.71]
        });
        if open {
            canvas.text(&line.text, 106., baseline, 20., |byte| {
                ink(line
                    .spans
                    .iter()
                    .find(|span| span.bytes.contains(&byte))
                    .map_or(Kind::Plain, |span| span.kind))
            });
        } else if !line.text.trim().is_empty() {
            let (start, end) = covered_span(&line.text);
            for py in y + 12..y + 15 {
                for px in start..end {
                    let pixel = ((py * width + px as u32) * 4) as usize;
                    canvas.pixels[pixel..pixel + 4].copy_from_slice(&[68, 91, 101, 255]);
                }
            }
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
