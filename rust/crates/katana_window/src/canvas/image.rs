//! The image the canvas paints into and the painter tests read: QImage's place, as plain pixels.
//!
//! Two formats, the two the window's painters make: RGBA, eight bits a channel with the colour
//! premultiplied by its alpha (QImage::Format_ARGB32_Premultiplied, which tiny-skia paints and
//! egui's textures take), and eight-bit grey (Format_Grayscale8). A pixel is read back as QImage
//! reads one: its colour with the alpha divided out.

use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

use super::painter::{Color, IntRect};

/// The layout of an image's bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// Four bytes a pixel, red, green, blue, alpha, the colour premultiplied.
    #[default]
    Rgba8Premultiplied,
    /// One byte a pixel.
    Grey8,
}

impl ImageFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8Premultiplied => 4,
            Self::Grey8 => 1,
        }
    }
}

/// A raster image, row after row from the top, with no padding between rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    width: u32,
    height: u32,
    format: ImageFormat,
    data: Vec<u8>,
    device_pixel_ratio: f64,
}

/// qPremultiply's channel: c * a / 255, rounded exactly (Qt's `(t + (t >> 8)) >> 8` with the
/// half added first).
pub fn premultiply(channel: u8, alpha: u8) -> u8 {
    let t = u32::from(channel) * u32::from(alpha) + 128;
    ((t + (t >> 8)) >> 8) as u8
}

/// qUnpremultiply's channel: c * 255 / a, rounded to nearest, at most 255; zero when the alpha is.
pub fn unpremultiply(channel: u8, alpha: u8) -> u8 {
    match alpha {
        0 => 0,
        255 => channel,
        _ => {
            let a = u32::from(alpha);
            ((u32::from(channel) * 255 + a / 2) / a).min(255) as u8
        }
    }
}

impl Image {
    /// A transparent image of `width` by `height` pixels, RGBA premultiplied, at a ratio of one.
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_format(width, height, ImageFormat::Rgba8Premultiplied)
    }

    /// A black grey image.
    pub fn new_grey(width: u32, height: u32) -> Self {
        Self::with_format(width, height, ImageFormat::Grey8)
    }

    fn with_format(width: u32, height: u32, format: ImageFormat) -> Self {
        let size = width as usize * height as usize * format.bytes_per_pixel();
        Self {
            width,
            height,
            format,
            data: vec![0; size],
            device_pixel_ratio: 1.0,
        }
    }

    /// An image over `data` already in `format`; refused when the bytes are not the size.
    pub fn from_bytes(width: u32, height: u32, format: ImageFormat, data: Vec<u8>) -> Result<Self> {
        let size = width as usize * height as usize * format.bytes_per_pixel();
        if data.len() != size {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the image's bytes are not its width times its height times its pixel's size",
                format!("{width} x {height}, {} bytes", data.len()),
            ));
        }
        Ok(Self {
            width,
            height,
            format,
            data,
            device_pixel_ratio: 1.0,
        })
    }

    /// An RGBA image from straight (not premultiplied) bytes: PNG's and the clipboard's.
    pub fn from_straight_rgba8(width: u32, height: u32, mut data: Vec<u8>) -> Result<Self> {
        for pixel in data.as_chunks_mut::<4>().0.iter_mut() {
            let alpha = pixel[3];
            pixel[0] = premultiply(pixel[0], alpha);
            pixel[1] = premultiply(pixel[1], alpha);
            pixel[2] = premultiply(pixel[2], alpha);
        }
        Self::from_bytes(width, height, ImageFormat::Rgba8Premultiplied, data)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    /// QImage::isNull: no pixels.
    pub fn is_null(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// How many device pixels make a logical one (QImage::devicePixelRatio): an icon painted for
    /// a screen at 125 % is 1.25.
    pub fn device_pixel_ratio(&self) -> f64 {
        self.device_pixel_ratio
    }

    pub fn set_device_pixel_ratio(&mut self, ratio: f64) {
        self.device_pixel_ratio = ratio;
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    fn offset(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| {
            (y as usize * self.width as usize + x as usize) * self.format.bytes_per_pixel()
        })
    }

    /// QImage::pixel: the colour at (x, y), its alpha divided out; a grey pixel is opaque.
    /// Transparent black outside the image (QImage warns and answers nothing useful there).
    pub fn pixel(&self, x: u32, y: u32) -> Color {
        let Some(at) = self.offset(x, y) else {
            return Color::TRANSPARENT;
        };
        match self.format {
            ImageFormat::Rgba8Premultiplied => {
                let alpha = self.data[at + 3];
                Color::rgba(
                    unpremultiply(self.data[at], alpha),
                    unpremultiply(self.data[at + 1], alpha),
                    unpremultiply(self.data[at + 2], alpha),
                    alpha,
                )
            }
            ImageFormat::Grey8 => {
                let grey = self.data[at];
                Color::rgb(grey, grey, grey)
            }
        }
    }

    /// QImage::setPixel(x, y, rgba): the colour premultiplied; a grey image keeps Qt's grey of
    /// it (qGray: (11 r + 16 g + 5 b) / 32). Nothing outside the image.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        let Some(at) = self.offset(x, y) else {
            return;
        };
        match self.format {
            ImageFormat::Rgba8Premultiplied => {
                self.data[at] = premultiply(color.r, color.a);
                self.data[at + 1] = premultiply(color.g, color.a);
                self.data[at + 2] = premultiply(color.b, color.a);
                self.data[at + 3] = color.a;
            }
            ImageFormat::Grey8 => self.data[at] = grey_of(color),
        }
    }

    /// QImage::fill: every pixel `color`.
    pub fn fill(&mut self, color: Color) {
        match self.format {
            ImageFormat::Rgba8Premultiplied => {
                let pixel = [
                    premultiply(color.r, color.a),
                    premultiply(color.g, color.a),
                    premultiply(color.b, color.a),
                    color.a,
                ];
                for each in self.data.as_chunks_mut::<4>().0.iter_mut() {
                    *each = pixel;
                }
            }
            ImageFormat::Grey8 => self.data.fill(grey_of(color)),
        }
    }

    /// QImage::copy(rect): the pixels of `rect`, with what lies outside the image transparent (or
    /// black, for grey), at the same ratio.
    pub fn copy(&self, rect: IntRect) -> Self {
        let width = rect.width.max(0) as u32;
        let height = rect.height.max(0) as u32;
        let mut out = Self::with_format(width, height, self.format);
        out.device_pixel_ratio = self.device_pixel_ratio;
        let size = self.format.bytes_per_pixel();
        for row in 0..height {
            let source_y = i64::from(rect.y) + i64::from(row);
            if source_y < 0 || source_y >= i64::from(self.height) {
                continue;
            }
            for column in 0..width {
                let source_x = i64::from(rect.x) + i64::from(column);
                if source_x < 0 || source_x >= i64::from(self.width) {
                    continue;
                }
                let from = (source_y as usize * self.width as usize + source_x as usize) * size;
                let to = (row as usize * width as usize + column as usize) * size;
                out.data[to..to + size].copy_from_slice(&self.data[from..from + size]);
            }
        }
        out
    }

    /// The pixels as straight RGBA, four bytes each: what PNG and the clipboard take.
    pub fn to_straight_rgba8(&self) -> Vec<u8> {
        match self.format {
            ImageFormat::Rgba8Premultiplied => {
                let mut out = self.data.clone();
                for pixel in out.as_chunks_mut::<4>().0.iter_mut() {
                    let alpha = pixel[3];
                    pixel[0] = unpremultiply(pixel[0], alpha);
                    pixel[1] = unpremultiply(pixel[1], alpha);
                    pixel[2] = unpremultiply(pixel[2], alpha);
                }
                out
            }
            ImageFormat::Grey8 => self
                .data
                .iter()
                .flat_map(|&grey| [grey, grey, grey, 255])
                .collect(),
        }
    }

    /// The image as an egui texture's pixels (premultiplied, as egui keeps them).
    pub fn to_color_image(&self) -> egui::ColorImage {
        let size = [self.width as usize, self.height as usize];
        let pixels = match self.format {
            ImageFormat::Rgba8Premultiplied => self
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| egui::Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
                .collect(),
            ImageFormat::Grey8 => self
                .data
                .iter()
                .map(|&grey| egui::Color32::from_gray(grey))
                .collect(),
        };
        egui::ColorImage::new(size, pixels)
    }

    /// The image from egui's pixels (a frame the soft renderer drew).
    pub fn from_color_image(image: &egui::ColorImage) -> Self {
        let mut data = Vec::with_capacity(image.pixels.len() * 4);
        for pixel in &image.pixels {
            data.extend_from_slice(&pixel.to_array());
        }
        Self {
            width: image.size[0] as u32,
            height: image.size[1] as u32,
            format: ImageFormat::Rgba8Premultiplied,
            data,
            device_pixel_ratio: 1.0,
        }
    }
}

/// qGray(r, g, b): (11 r + 16 g + 5 b) / 32, the alpha not counted.
fn grey_of(color: Color) -> u8 {
    ((u32::from(color.r) * 11 + u32::from(color.g) * 16 + u32::from(color.b) * 5) / 32) as u8
}

#[cfg(test)]
mod tests {
    use super::{Image, premultiply, unpremultiply};
    use crate::canvas::{Color, IntRect};

    // Worked by hand: 200 at half alpha (128) is 200 * 128 / 255 = 100.39, so 100; back,
    // 100 * 255 / 128 = 199.2, so 199 - a round trip through half alpha loses a step, as Qt's does.
    #[test]
    fn a_pixel_is_kept_premultiplied_and_read_back_straight() {
        assert_eq!(premultiply(200, 128), 100);
        assert_eq!(unpremultiply(100, 128), 199);
        let mut image = Image::new(2, 1);
        image.set_pixel(1, 0, Color::rgba(200, 0, 255, 128));
        assert_eq!(image.bytes()[4..8], [100, 0, 128, 128]);
        assert_eq!(image.pixel(1, 0), Color::rgba(199, 0, 255, 128));
        assert_eq!(image.pixel(0, 0), Color::TRANSPARENT);
        assert_eq!(image.pixel(5, 0), Color::TRANSPARENT);
    }

    #[test]
    fn a_copy_beyond_the_edge_is_transparent_there() {
        let mut image = Image::new(2, 2);
        image.fill(Color::WHITE);
        let part = image.copy(IntRect::new(1, 1, 2, 2));
        assert_eq!(part.pixel(0, 0), Color::WHITE);
        assert_eq!(part.pixel(1, 1), Color::TRANSPARENT);
    }
}
