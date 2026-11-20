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
pub(super) fn ink(kind: Kind) -> [f32; 3] {
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
pub(super) struct Canvas {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    context: ScaleContext,
}

fn emoji_font() -> FontRef<'static> {
    static FONT: std::sync::OnceLock<FontRef<'static>> = std::sync::OnceLock::new();
    *FONT
        .get_or_init(|| FontRef::from_index(super::EMOJI_FONT_DATA, 0).expect("bundled emoji font"))
}

fn math_font() -> FontRef<'static> {
    static FONT: std::sync::OnceLock<FontRef<'static>> = std::sync::OnceLock::new();
    *FONT.get_or_init(|| FontRef::from_index(super::MATH_FONT_DATA, 0).expect("bundled math font"))
}

#[cfg(test)]
#[test]
fn lean_math_symbols_render_as_distinct_glyphs_on_source_walls() {
    // Given Lean identifiers and operators absent from Fira Mono.
    let rendered = |text: &str| {
        let mut canvas = Canvas::new(80, 64);
        canvas.text(text, 5., 40., 30., |_| [1.; 3]);
        canvas.pixels
    };
    let missing = rendered("\u{10ffff}");
    // When the wall rasterizes each symbol, none may become the missing-glyph box.
    for ch in "𝓟ℕℤℚℝℂ∀∃∈∉⊆⊂⟨⟩↦→↔≠≤≥⊢₀₁₂ᵢαβλ".chars()
    {
        let pixels = rendered(&ch.to_string());
        assert!(pixels != missing, "Missing Lean glyph: {ch}");
        assert!(pixels.iter().any(|&value| value > 0));
        assert!(
            (source_advance(ch) - source_advance('A')).abs() < 0.01,
            "Non-monospace Lean glyph: {ch}"
        );
    }
}

#[cfg(test)]
#[test]
fn emoji_fallback_rasterizes_eyes_and_other_glyphs() {
    let rendered = |text| {
        let mut canvas = Canvas {
            pixels: vec![0; 64 * 64 * 4],
            width: 64,
            height: 64,
            context: ScaleContext::new(),
        };
        canvas.text(text, 5., 40., 36., |_| [1.; 3]);
        canvas.pixels
    };
    let eyes = rendered("👀");
    assert!(eyes.iter().any(|&value| value > 0));
    assert_ne!(eyes, rendered("\u{10ffff}"));
    assert_ne!(eyes, rendered("👍"));
}

impl Canvas {
    pub(super) fn new(width: u32, height: u32) -> Self {
        Self {
            pixels: vec![0; (width * height * 4) as usize],
            width,
            height,
            context: ScaleContext::new(),
        }
    }
    pub(super) fn rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: [u8; 3]) {
        for row in y..y.saturating_add(height).min(self.height) {
            for col in x..x.saturating_add(width).min(self.width) {
                let offset = ((row * self.width + col) * 4) as usize;
                self.pixels[offset..offset + 4]
                    .copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }
    pub(super) fn image(self) -> Image {
        Image::new(
            Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }
    pub(super) fn text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: impl Fn(usize) -> [f32; 3],
    ) {
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
        self.scaled_text_to(text, x, y, size, scale, self.width as f32 - 14., color);
    }

    fn scaled_text_to(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        scale: Vec2,
        right: f32,
        color: impl Fn(usize) -> [f32; 3],
    ) {
        let font = FontRef::from_index(
            include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf"),
            0,
        )
        .unwrap();
        let metrics = font.glyph_metrics(&[]).scale(size);
        let mut scaler = self.context.builder(font).size(size).hint(true).build();
        let mut fallback_context = None::<ScaleContext>;
        let mut render = Render::new(&[Source::Outline]);
        if scale != Vec2::ONE {
            render.transform(Some(swash::zeno::Transform::scale(scale.x, scale.y)));
        }
        let mut x = x;
        for (byte, ch) in text.char_indices() {
            if x >= right {
                break;
            }
            if ch == '\t' {
                x += metrics.advance_width(font.charmap().map(' ')) * 4. * scale.x;
                continue;
            }
            if matches!(ch, '\u{fe0e}' | '\u{fe0f}' | '\u{200d}') {
                continue;
            }
            let id = font.charmap().map(ch);
            let ink = color(byte);
            let fallback = if id == 0 && math_font().charmap().map(ch) != 0 {
                Some((math_font(), size))
            } else if id == 0 && emoji_font().charmap().map(ch) != 0 {
                Some((emoji_font(), size * 0.85))
            } else {
                None
            };
            let (glyph, advance) = if let Some((font, size)) = fallback {
                let id = font.charmap().map(ch);
                let mut fallback_scaler = fallback_context
                    .get_or_insert_with(ScaleContext::new)
                    .builder(font)
                    .size(size)
                    .hint(true)
                    .build();
                (
                    render.render(&mut fallback_scaler, id),
                    font.glyph_metrics(&[]).scale(size).advance_width(id),
                )
            } else {
                (render.render(&mut scaler, id), metrics.advance_width(id))
            };
            if x + advance * scale.x > right {
                break;
            }
            if let Some(glyph) = glyph {
                let p = glyph.placement;
                for gy in 0..p.height {
                    for gx in 0..p.width {
                        let coverage = glyph.data[(gy * p.width + gx) as usize] as f32 / 255.;
                        let px = x as i32 + p.left + gx as i32;
                        let py = y as i32 - p.top + gy as i32;
                        if px >= 0
                            && py >= 0
                            && (px as u32) < self.width
                            && (px as f32) < right
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
            x += advance * scale.x;
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

pub(super) fn vibepad_wordmark() -> Image {
    let mut canvas = Canvas {
        pixels: vec![0; 256 * 64 * 4],
        width: 256,
        height: 64,
        context: ScaleContext::new(),
    };
    canvas.text("VıbePad", 8., 51., 52., |_| [1.; 3]);
    for (index, pixel) in canvas.pixels.chunks_exact_mut(4).enumerate() {
        let x = (index % 256) as f32;
        let y = (index / 256) as f32;
        let dot = Vec2::new(x, y).distance_squared(Vec2::new(56., 16.)) <= 4.5 * 4.5;
        if dot {
            pixel.copy_from_slice(&[255, 65, 89, 255]);
        } else {
            let coverage = pixel[0];
            pixel.copy_from_slice(&[209, 220, 228, coverage]);
        }
    }
    Image::new(
        Extent3d {
            width: 256,
            height: 64,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas.pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
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

/// Match the title's physical aspect and glyph scale to its source wall.
pub(super) fn heading(title: &str, tint: [f32; 3], aspect: f32, glyph_scale: f32) -> Image {
    let width = LISTING_WIDTH;
    let height = (width as f32 * aspect).ceil().max(1.) as u32;
    let scale = Vec2::new(
        glyph_scale,
        glyph_scale * height as f32 / (width as f32 * aspect),
    );
    let mut canvas = Canvas {
        pixels: vec![0; (width * height * 4) as usize],
        width,
        height,
        context: ScaleContext::new(),
    };
    canvas.band(0, height, [9, 17, 23]);
    canvas.scaled_text(
        title,
        HEADING_LEFT,
        height as f32 / 2. + 7. * scale.y,
        20.,
        scale,
        |_| tint,
    );
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

/// Explain an empty wall at its reading height, independently of its roof heading.
pub(super) fn wall_notice(lines: [&str; 3], aspect: f32) -> Image {
    let width = LISTING_WIDTH;
    let height = (width as f32 * aspect).round().clamp(96., 768.) as u32;
    let mut canvas = Canvas::new(width, height);
    canvas.rect(0, 0, width, height, [13, 23, 32]);
    let scale = (height as f32 / 260.).min(1.);
    for (index, (text, size, tint)) in [
        (lines[0], 42., [0.82, 0.95, 0.91]),
        (lines[1], 28., [0.65, 0.76, 0.81]),
        (lines[2], 26., [0.48, 0.67, 0.72]),
    ]
    .into_iter()
    .enumerate()
    {
        let advance: f32 = text.chars().map(source_advance).sum();
        let size = (size * scale).min((width as f32 - 96.) * 20. / advance.max(1.));
        let x = (width as f32 - advance * size / 20.) / 2.;
        let y = height as f32 / 2. + (index as f32 - 1.) * 58. * scale + size * 0.35;
        canvas.text(text, x, y, size, |_| tint);
    }
    canvas.image()
}

pub(super) const LISTING_WIDTH: u32 = 1536;
pub(super) const LISTING_ROW_PIXELS: u32 = 27;
pub(super) const HEADING_LEFT: f32 = 32.;

pub(super) fn text_start(face: Face) -> f32 {
    if face == Face::Blame {
        if cfg!(target_arch = "wasm32") {
            470.
        } else {
            405.
        }
    } else {
        106.
    }
}

pub(super) fn text_width(face: Face) -> f32 {
    LISTING_WIDTH as f32 - text_start(face) - 20.
}

/// Unscaled source advance, shared by fixed-width wrapping and the rasterizer's font choices.
pub(super) fn source_advance(ch: char) -> f32 {
    let font = FontRef::from_index(
        include_bytes!("../../../../assets/fonts/FiraMono-Medium.ttf"),
        0,
    )
    .unwrap();
    if matches!(ch, '\u{fe0e}' | '\u{fe0f}' | '\u{200d}') {
        return 0.;
    }
    let id = font.charmap().map(ch);
    let metrics = font.glyph_metrics(&[]).scale(20.);
    if ch == '\t' {
        return metrics.advance_width(font.charmap().map(' ')) * 4.;
    }
    let math = math_font();
    if id == 0 && math.charmap().map(ch) != 0 {
        return math
            .glyph_metrics(&[])
            .scale(20.)
            .advance_width(math.charmap().map(ch));
    }
    let emoji = emoji_font();
    let fallback = emoji.charmap().map(ch);
    if id == 0 && fallback != 0 {
        emoji.glyph_metrics(&[]).scale(17.).advance_width(fallback)
    } else {
        metrics.advance_width(id)
    }
}

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
    paint_at: impl Fn(usize) -> Option<Vec4>,
) -> Image {
    const WIDTH: usize = 256;
    let height = (count * 4).clamp(64, 2048);
    let mut pixels = vec![0; WIDTH * height * 4];
    for (y, pixels) in pixels.chunks_exact_mut(WIDTH * 4).enumerate() {
        let index = y * count / height;
        let (line, mark) = row(index);
        let open = line.is_none() || revealed(index + 1);
        let coverage = if open {
            Coverage::Open
        } else {
            Coverage::Closed
        };
        let mut background = listing_background(face, mark);
        let paint = paint_at(index);
        if let Some(paint) = paint {
            let alpha = paint.w * 0.36;
            for (value, ink) in background.iter_mut().zip(paint.to_array()) {
                *value = (*value as f32 * (1. - alpha) + ink * 255. * alpha) as u8;
            }
        }
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[background[0], background[1], background[2], 255]);
        }
        let gutter = WIDTH * 91 / LISTING_WIDTH as usize;
        if let Some(paint) = paint {
            let color = [paint.x, paint.y, paint.z].map(|value| (value * 255.) as u8);
            for x in gutter.saturating_sub(3)..gutter + 2 {
                pixels[x * 4..x * 4 + 3].copy_from_slice(&color);
            }
        } else {
            pixels[gutter * 4..gutter * 4 + 3].copy_from_slice(&coverage.accent());
        }
        if let Some(ink) = source_change_ink(face, mark) {
            let change = WIDTH * 102 / LISTING_WIDTH as usize * 4;
            pixels[change..change + 3].copy_from_slice(&ink);
        }
    }
    let mut canvas = Canvas {
        pixels,
        width: WIDTH as u32,
        height: height as u32,
        context: ScaleContext::new(),
    };
    let ratio = WIDTH as f32 / LISTING_WIDTH as f32;
    // Sample at most 512 real source rows in the distant preview; nearby tiles retain every glyph.
    let stride = count.div_ceil(512).max(1);
    for index in (0..count).step_by(stride) {
        let (line, _) = row(index);
        let Some(line) = line else { continue };
        let row_height = height as f32 / count.max(1) as f32;
        let scale = Vec2::new(
            ratio * glyph_scale.x,
            (row_height / LISTING_ROW_PIXELS as f32).clamp(0.06, 0.35),
        );
        let baseline = (index as f32 + 0.5) * row_height + 5. * scale.y;
        canvas.scaled_text_to(
            &line.text,
            106. * ratio,
            baseline,
            20.,
            scale,
            WIDTH as f32 - 2.,
            |byte| {
                ink(line
                    .spans
                    .iter()
                    .find(|span| span.bytes.contains(&byte))
                    .map_or(Kind::Plain, |span| span.kind))
            },
        );
    }
    let original = canvas.pixels.clone();
    let code_start = (106. * ratio).ceil() as usize;
    for y in 0..height {
        let index = y * count / height;
        if revealed(index + 1) {
            continue;
        }
        for x in code_start..WIDTH {
            let mut sum = [0u32; 3];
            let mut samples = 0;
            for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                for xx in (x - 1).max(code_start)..=(x + 1).min(WIDTH - 1) {
                    let offset = (yy * WIDTH + xx) * 4;
                    for channel in 0..3 {
                        sum[channel] += original[offset + channel] as u32;
                    }
                    samples += 1;
                }
            }
            let offset = (y * WIDTH + x) * 4;
            for channel in 0..3 {
                canvas.pixels[offset + channel] = (sum[channel] / samples) as u8;
            }
        }
    }
    let pixels = canvas.pixels;
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
    image.sampler = ImageSampler::linear();
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
        let start = x(106.) + (text_start(face) - 106.) * glyph_scale.x;
        if let Some(notes) = notes {
            if let Some(author) = notes.get(line.number.saturating_sub(1)) {
                let (limit, size) = if cfg!(target_arch = "wasm32") {
                    (39, 15.)
                } else {
                    (28, 17.)
                };
                let short = author.chars().take(limit).collect::<String>();
                canvas.scaled_text_to(
                    &short,
                    x(106.),
                    baseline,
                    size,
                    glyph_scale,
                    start - x(8.),
                    |_| [0.53, 0.83, 0.78],
                );
            }
        }
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
    fn long_code_tails_render_on_every_face_and_history_cannot_overlap_source() {
        let text = format!("{} ELM_TAIL", "module SourceArgument ".repeat(25));
        let lines = [Line {
            number: 1,
            spans: vec![repo_world::syntax::Span {
                bytes: text.len() - 8..text.len(),
                kind: Kind::Keyword,
            }],
            text,
        }];
        for face in [Face::Source, Face::Diff, Face::Blame] {
            let layout = super::super::listing::layout::SourceLayout::for_face(&lines, face);
            let rows = layout
                .rows
                .into_iter()
                .map(|line| (Some(line), Mark::Context))
                .collect::<Vec<_>>();
            for scale in [Vec2::ONE, Vec2::splat(0.5), Vec2::new(1., 0.25)] {
                let notes = vec!["👀".repeat(80)];
                let ready = listing_with_notes(
                    &rows,
                    face,
                    scale,
                    (face == Face::Blame).then_some(notes.as_slice()),
                );
                assert!(
                    ready
                        .data
                        .as_ref()
                        .unwrap()
                        .chunks_exact(4)
                        .any(|pixel| pixel[0] > 160 && pixel[1] < 185 && pixel[2] > 235),
                    "The tail keyword was clipped on {face:?}"
                );
                if face == Face::Blame {
                    let pending = listing_with_notes(&rows, face, scale, None);
                    let width = ready.width() as usize;
                    let start = ((106. + (text_start(face) - 106.) * scale.x)
                        * (width as f32 / LISTING_WIDTH as f32))
                        .ceil() as usize;
                    for (ready, pending) in ready
                        .data
                        .as_ref()
                        .unwrap()
                        .chunks_exact(width * 4)
                        .zip(pending.data.as_ref().unwrap().chunks_exact(width * 4))
                    {
                        assert_eq!(
                            &ready[start * 4..],
                            &pending[start * 4..],
                            "Attribution changed the code area"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn heading_ink_clears_the_physical_file_stripe_at_every_glyph_scale() {
        let file = repo_world::File {
            path: "math.mjs".into(),
            before: None,
            after: None,
            change: repo_world::Change::Unchanged,
            position: [0., 0.],
        };
        let size = Vec3::splat(14.);
        let stripe =
            crate::file_facade::transform(&file, size, crate::file_facade::Part::Stripe(1.));
        let stripe_end = stripe.scale.x / size.x * LISTING_WIDTH as f32;
        for (aspect, scale) in [(0.0264, 1.), (0.0145, 0.826), (0.008, 0.455)] {
            let image = heading("math.mjs / 4 rows", [0.5, 0.95, 0.85], aspect, scale);
            let width = image.width() as usize;
            let first_ink = image
                .data
                .as_ref()
                .unwrap()
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, pixel)| pixel[1] > 100)
                .map(|(pixel, _)| pixel % width)
                .min()
                .unwrap();
            assert!(
                first_ink as f32 > stripe_end,
                "heading starts at {first_ink}px behind the stripe ending at {stripe_end:.1}px"
            );
        }
    }

    #[test]
    fn headings_fit_short_and_tall_wall_aspects_without_clipping_glyphs() {
        for (aspect, scale) in [(0.0264, 1.), (0.0145, 0.826), (0.008, 0.455)] {
            let image = heading("math.mjs / 4 rows", [0.5, 0.95, 0.85], aspect, scale);
            let width = image.width() as usize;
            let height = image.height() as usize;
            let rows = image
                .data
                .as_ref()
                .unwrap()
                .chunks_exact(width * 4)
                .enumerate()
                .filter(|(_, pixels)| pixels.chunks_exact(4).any(|pixel| pixel[1] > 100))
                .map(|(row, _)| row)
                .collect::<Vec<_>>();
            assert!(!rows.is_empty(), "header text must be visible");
            assert!(
                *rows.first().unwrap() > 0 && *rows.last().unwrap() + 1 < height,
                "header glyphs must have room above ascenders and below descenders"
            );
        }
    }

    #[test]
    fn distant_comments_color_their_row_and_widen_the_gutter_mark() {
        let plain = listing_preview(
            2,
            |_| (None, Mark::Context),
            |_| true,
            Face::Source,
            Vec2::ONE,
            |_| None,
        );
        let marked = listing_preview(
            2,
            |_| (None, Mark::Context),
            |_| true,
            Face::Source,
            Vec2::ONE,
            |row| (row == 1).then_some(Vec4::new(1., 0.65, 0.15, 1.)),
        );
        let plain = plain.data.unwrap();
        let marked = marked.data.unwrap();
        let row_bytes = 256 * 4;
        assert_eq!(&plain[..32 * row_bytes], &marked[..32 * row_bytes]);
        let edge = 32 * row_bytes + 14 * 4;
        assert_eq!(&marked[edge..edge + 3], &[255, 165, 38]);
        assert_ne!(plain[34 * row_bytes], marked[34 * row_bytes]);
    }

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
