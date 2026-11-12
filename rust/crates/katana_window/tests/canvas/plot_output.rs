//! The twins of tests/qt_widgets/plotting/test_plot_output.cpp's TiffWriter suite (C's; the
//! PlotOutput suite is SH1's): a TIFF written holds the image's pixels exactly, read back by a
//! reader of the baseline TIFF the writer writes, and what cannot be written is refused.

use std::collections::BTreeMap;

use katana_core::ErrorCode;
use katana_update::inflate::inflate_raw_to_string;
use katana_window::canvas::{Color, Image, ImageFormat};
use katana_window::plotting::tiff_writer::write_tiff;

use crate::window_support::Scratch;

/// A baseline TIFF as tiff_writer writes it, read back: its tags' values (a rational as numerator
/// and denominator) and its pixels, the strips inflated and joined.
struct Tiff {
    tags: BTreeMap<u16, Vec<u32>>,
    software: String,
    pixels: Vec<u8>,
}

impl Tiff {
    fn tag(&self, id: u16) -> u32 {
        self.tags[&id][0]
    }
}

fn read_tiff(file: &[u8]) -> Option<Tiff> {
    if file.len() < 8 || !file.starts_with(b"II*\0") {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([file[at], file[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]);
    let mut tiff = Tiff {
        tags: BTreeMap::new(),
        software: String::new(),
        pixels: Vec::new(),
    };
    let directory = u32_at(4) as usize;
    let entries = u16_at(directory);
    for i in 0..usize::from(entries) {
        let entry = directory + 2 + 12 * i;
        let id = u16_at(entry);
        let kind = u16_at(entry + 2);
        let count = u32_at(entry + 4) as usize;
        let size = match kind {
            3 => 2,
            4 => 4,
            5 => 8,
            _ => 1,
        };
        let at = if size * count <= 4 {
            entry + 8
        } else {
            u32_at(entry + 8) as usize
        };
        let mut values = Vec::new();
        for n in 0..count {
            match kind {
                3 => values.push(u32::from(u16_at(at + 2 * n))),
                4 => values.push(u32_at(at + 4 * n)),
                5 => {
                    values.push(u32_at(at + 8 * n));
                    values.push(u32_at(at + 8 * n + 4));
                }
                _ => {}
            }
        }
        if kind == 2 {
            tiff.software = String::from_utf8_lossy(&file[at..at + count - 1]).into_owned();
        }
        tiff.tags.insert(id, values);
    }
    let width = tiff.tag(256);
    let height = tiff.tag(257);
    let row_bytes = width * tiff.tag(277);
    let rows_per_strip = tiff.tag(278);
    let offsets = tiff.tags[&273].clone();
    let counts = tiff.tags[&279].clone();
    for (strip, (&offset, &count)) in offsets.iter().zip(&counts).enumerate() {
        let rows = rows_per_strip.min(height - strip as u32 * rows_per_strip);
        let expected = rows * row_bytes;
        let packed = &file[offset as usize..(offset + count) as usize];
        // A zlib stream: two bytes of header, the deflate data, and four of its Adler-32.
        if packed.len() < 6 {
            return None;
        }
        let inflated =
            inflate_raw_to_string(&packed[2..packed.len() - 4], u64::from(expected)).ok()?;
        if inflated.len() != expected as usize {
            return None;
        }
        tiff.pixels.extend_from_slice(&inflated);
    }
    Some(tiff)
}

/// The pixels of `image` as a TIFF holds them: rows of RGB, or of grey.
fn tiff_pixels_of(image: &Image) -> Vec<u8> {
    match image.format() {
        ImageFormat::Grey8 => image.bytes().to_vec(),
        ImageFormat::Rgba8Premultiplied => image
            .to_straight_rgba8()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect(),
    }
}

mod tiff_writer {
    use super::*;

    #[test]
    fn writes_grey_and_colour_pixels_exactly() {
        let scratch = Scratch::new("tiff_writer_pixels");
        // Tall enough for several strips: 300 x 1000 of RGB is 900 kB.
        let mut colour = Image::new(300, 1000);
        for y in 0..colour.height() {
            for x in 0..colour.width() {
                colour.set_pixel(
                    x,
                    y,
                    Color::rgb((x % 256) as u8, (y % 256) as u8, ((x * y) % 256) as u8),
                );
            }
        }
        let path = scratch.path("colour.tif");
        write_tiff(&colour, &path, 254.5).expect("the colour raster is written");
        let read =
            read_tiff(&std::fs::read(&path).expect("the file is there")).expect("it reads back");
        assert!(read.tags[&273].len() > 1);
        assert_eq!(read.tags[&282], vec![254_500, 1000]);
        assert_eq!(read.software, "Katana");
        assert!(read.pixels == tiff_pixels_of(&colour));

        let mut grey = Image::new_grey(17, 5);
        let width = grey.width();
        for y in 0..grey.height() {
            for x in 0..width {
                grey.bytes_mut()[(y * width + x) as usize] = (x * 15 + y) as u8;
            }
        }
        let path = scratch.path("grey.tif");
        write_tiff(&grey, &path, 300.0).expect("the grey raster is written");
        let read_grey =
            read_tiff(&std::fs::read(&path).expect("the file is there")).expect("it reads back");
        assert_eq!(read_grey.tag(262), 1);
        assert!(read_grey.pixels == tiff_pixels_of(&grey));
    }

    #[test]
    fn refuses_what_it_cannot_write() {
        let scratch = Scratch::new("tiff_writer_refusals");
        assert_eq!(
            write_tiff(&Image::default(), &scratch.path("a.tif"), 300.0)
                .expect_err("a null image is refused")
                .code,
            ErrorCode::InvalidArgument
        );
        let mut image = Image::new(4, 4);
        image.fill(Color::WHITE);
        assert_eq!(
            write_tiff(&image, &scratch.path("a.tif"), 0.0)
                .expect_err("no resolution is refused")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            write_tiff(&image, &scratch.path("no/such/folder/a.tif"), 300.0)
                .expect_err("a folder that is not there is refused")
                .code,
            ErrorCode::FileExportFailure
        );
    }
}
