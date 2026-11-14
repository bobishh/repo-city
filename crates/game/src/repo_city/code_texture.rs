//! Tree-sitter syntax colors and independent diff backgrounds.
use bevy::{
    asset::RenderAssetUsages,
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

pub fn listing(rows: &[(Option<Line>, Mark)], revealed: &impl Fn(usize) -> bool) -> Image {
    let width = 1536;
    let height = rows.len() as u32 * 27;
    let mut canvas = Canvas {
        pixels: vec![0; (width * height * 4) as usize],
        width,
        height,
        context: ScaleContext::new(),
    };
    for (index, (line, mark)) in rows.iter().enumerate() {
        let y = index as u32 * 27;
        let open = line.as_ref().is_none_or(|line| revealed(line.number));
        let background = match (open, mark) {
            (false, _) => [28, 43, 50],
            (_, Mark::Added) => [17, 56, 42],
            (_, Mark::Removed) => [66, 27, 35],
            _ => [9, 17, 23],
        };
        canvas.band(y, y + 27, background);
        if !open && index == 0 {
            canvas.band(y, y + 2, [95, 137, 147]);
            canvas.text("COVERED", 106., y as f32 + 21., 18., |_| [0.58, 0.73, 0.77]);
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
        } else if index > 0 && !line.text.trim().is_empty() {
            let start = 106 + (line.text.len() - line.text.trim_start().len()).min(40) * 12;
            let end = (106 + line.text.len() * 12).min((width - 16) as usize);
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
