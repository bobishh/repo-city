use super::*;
use ::image::{RgbaImage, imageops};

const VIEW_HEIGHT: u32 = HEIGHT - 96;

enum Row {
    Text {
        value: String,
        size: f32,
        ink: [f32; 3],
    },
    Picture(RgbaImage),
}

pub(super) struct Page {
    title: String,
    rows: Vec<(f32, Row)>,
    height: f32,
}

impl Page {
    pub(super) fn new(title: &str) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
            height: 12.,
        }
    }

    pub(super) fn gap(&mut self, height: f32) {
        self.height += height;
    }

    pub(super) fn text(&mut self, value: &str, size: f32, ink: [f32; 3], code: bool) {
        let capacity = (948. / (size * 0.62)) as usize;
        for line in value.split('\n') {
            let mut remaining = line;
            loop {
                let hard_end = remaining
                    .char_indices()
                    .nth(capacity)
                    .map_or(remaining.len(), |(end, _)| end);
                let end = if code || hard_end == remaining.len() {
                    hard_end
                } else {
                    remaining[..hard_end]
                        .rfind(char::is_whitespace)
                        .filter(|end| *end > 0)
                        .unwrap_or(hard_end)
                };
                let (row, rest) = remaining.split_at(end);
                self.rows.push((
                    self.height,
                    Row::Text {
                        value: row.into(),
                        size,
                        ink,
                    },
                ));
                self.height += size + 5.;
                remaining = if code { rest } else { rest.trim_start() };
                if remaining.is_empty() {
                    break;
                }
            }
        }
    }

    pub(super) fn picture(&mut self, image: Image, height_limit: u32) {
        let (width, height) = (image.width(), image.height());
        let Some(image) = image
            .data
            .and_then(|bytes| RgbaImage::from_raw(width, height, bytes))
        else {
            return;
        };
        let scale = (948. / width as f32)
            .min(height_limit as f32 / height as f32)
            .min(1.5);
        let resized = imageops::resize(
            &image,
            (width as f32 * scale).round().max(1.) as u32,
            (height as f32 * scale).round().max(1.) as u32,
            imageops::FilterType::Triangle,
        );
        let height = resized.height();
        self.rows.push((self.height, Row::Picture(resized)));
        self.height += height as f32;
    }

    pub(super) fn scroll_limit(&self) -> f32 {
        (self.height + 10. - VIEW_HEIGHT as f32).max(0.)
    }

    pub(super) fn image(&self, set: &WorldSet, offset: f32) -> Image {
        let mut result = canvas(set, &self.title).image();
        let mut body = code_texture::Canvas::new(WIDTH - 8, VIEW_HEIGHT);
        body.rect(0, 0, WIDTH - 8, VIEW_HEIGHT, background(set));
        let mut pictures = Vec::new();
        for (top, row) in &self.rows {
            let y = top - offset;
            if y >= VIEW_HEIGHT as f32 {
                break;
            }
            match row {
                Row::Text { value, size, ink } if y + size + 5. > 0. => {
                    body.text(value, 30., y + size, *size, |_| *ink);
                }
                Row::Picture(image) if y + image.height() as f32 > 0. => {
                    pictures.push((y.round() as i64, image));
                }
                _ => {}
            }
        }
        let limit = self.scroll_limit();
        if limit > 0. {
            let track = (VIEW_HEIGHT - 20) as f32;
            let height = (track * VIEW_HEIGHT as f32 / (VIEW_HEIGHT as f32 + limit)).max(24.);
            let top = 10. + offset / limit * (track - height);
            body.rect(WIDTH - 25, 10, 5, track as u32, [37, 66, 72]);
            body.rect(
                WIDTH - 25,
                top.round() as u32,
                5,
                height.round() as u32,
                [105, 177, 168],
            );
        }
        let mut body =
            RgbaImage::from_raw(WIDTH - 8, VIEW_HEIGHT, body.image().data.unwrap()).unwrap();
        for (y, picture) in pictures {
            let x = i64::from((body.width() - picture.width()) / 2);
            imageops::overlay(&mut body, picture, x, y);
        }
        let mut frame = RgbaImage::from_raw(WIDTH, HEIGHT, result.data.take().unwrap()).unwrap();
        imageops::replace(&mut frame, &body, 4, 92);
        result.data = Some(frame.into_raw());
        result
    }
}
