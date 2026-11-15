//! PNG, written and read through the png crate: the screenshot every headless run writes, the
//! plotted sheets and snapshots, and the images a drawing holds (QImage's PNG plugin's place).
//!
//! An RGBA image is written as eight-bit RGBA with its alpha divided out, as QImage::save writes
//! a premultiplied image; a grey image as eight-bit grey. Read, any PNG becomes eight bits a
//! channel (palettes and low depths expanded, sixteen bits cut to eight), grey without alpha
//! staying grey.

use std::io::Cursor;
use std::path::Path;

use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::{ErrorCode, Result, Status};

use super::image::{Image, ImageFormat};

fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

/// The image as the bytes of a PNG file.
pub fn encode_png(image: &Image) -> Result<Vec<u8>> {
    let failed = |error: png::EncodingError| {
        make_error(
            ErrorCode::FileExportFailure,
            "the image could not be written as PNG",
            format!("{error}"),
        )
    };
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, image.width(), image.height());
        let (color, data) = match image.format() {
            ImageFormat::Rgba8Premultiplied => (png::ColorType::Rgba, image.to_straight_rgba8()),
            ImageFormat::Grey8 => (png::ColorType::Grayscale, image.bytes().to_vec()),
        };
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(failed)?;
        writer.write_image_data(&data).map_err(failed)?;
        writer.finish().map_err(failed)?;
    }
    Ok(bytes)
}

/// The image a PNG file's bytes hold.
pub fn decode_png(bytes: &[u8]) -> Result<Image> {
    let failed = |error: png::DecodingError| {
        make_error(
            ErrorCode::FileImportFailure,
            "the bytes are not a PNG image this program reads",
            format!("{error}"),
        )
    };
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(failed)?;
    let size = reader.output_buffer_size().ok_or_else(|| {
        make_error(
            ErrorCode::FileImportFailure,
            "the PNG image is too large to read",
            String::new(),
        )
    })?;
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer).map_err(failed)?;
    buffer.truncate(frame.buffer_size());
    let (width, height) = (frame.width, frame.height);
    let pixels = width as usize * height as usize;
    match frame.color_type {
        png::ColorType::Grayscale => Image::from_bytes(width, height, ImageFormat::Grey8, buffer),
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity(pixels * 4);
            for pair in buffer.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
            }
            Image::from_straight_rgba8(width, height, rgba)
        }
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity(pixels * 4);
            for triple in buffer.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[triple[0], triple[1], triple[2], 255]);
            }
            Image::from_straight_rgba8(width, height, rgba)
        }
        png::ColorType::Rgba => Image::from_straight_rgba8(width, height, buffer),
        png::ColorType::Indexed => Err(make_error(
            ErrorCode::FileImportFailure,
            "the PNG image's palette was not expanded",
            String::new(),
        )),
    }
}

/// The image written to `path` as PNG (QImage::save(path, "PNG")).
pub fn write_png(image: &Image, path: &Path) -> Status {
    let bytes = encode_png(image).map_err(|error| error.with_context(path_text(path)))?;
    std::fs::write(path, bytes).map_err(|error| {
        make_error(
            ErrorCode::FileExportFailure,
            format!("could not write the PNG file: {error}"),
            path_text(path),
        )
    })
}

/// The image of the PNG file at `path`.
pub fn read_png(path: &Path) -> Result<Image> {
    let bytes = std::fs::read(path).map_err(|error| {
        make_error(
            ErrorCode::FileImportFailure,
            format!("could not read the PNG file: {error}"),
            path_text(path),
        )
    })?;
    decode_png(&bytes).map_err(|error| error.with_context(path_text(path)))
}

#[cfg(test)]
mod tests {
    use super::{decode_png, encode_png};
    use crate::canvas::{Color, Image};

    // A pixel written and read back is the pixel, through PNG's straight alpha.
    #[test]
    fn an_image_written_as_png_reads_back_the_same() {
        let mut image = Image::new(3, 2);
        image.set_pixel(0, 0, Color::rgb(30, 35, 41));
        image.set_pixel(2, 1, Color::rgba(76, 159, 254, 255));
        let bytes = encode_png(&image).expect("an image encodes");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let read = decode_png(&bytes).expect("its own bytes decode");
        assert_eq!(read.width(), 3);
        assert_eq!(read.height(), 2);
        assert_eq!(read.pixel(0, 0), Color::rgb(30, 35, 41));
        assert_eq!(read.pixel(2, 1), Color::rgb(76, 159, 254));
        assert_eq!(read.pixel(1, 0), Color::TRANSPARENT);
    }

    #[test]
    fn bytes_that_are_no_png_are_refused() {
        assert!(decode_png(b"not a picture").is_err());
    }
}
