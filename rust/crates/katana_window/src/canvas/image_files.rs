//! Raster images read from files and bytes through the `image` crate: the pictures a drawing or a
//! sheet holds (a logo, an aerial), in the formats Qt's image plugins read - PNG, JPEG, BMP, GIF
//! and ICO (QImageReader's place). The format is found from the bytes, not the name, as
//! QImageReader finds it. A grey picture stays grey (QImage's Format_Grayscale8); every other one
//! becomes RGBA with its colour premultiplied, as the canvas paints it. A GIF or an icon of several
//! images reads its first.
//!
//! PNG is written by canvas::png_file, which reads PNG too; this reads every format, PNG
//! included, for the code that takes whatever picture a person chose.

use std::path::Path;

use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::{ErrorCode, Result};

use super::image::{Image, ImageFormat};

fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

/// The image a file's bytes hold. FileImportFailure for bytes no reader takes.
pub fn decode_image(bytes: &[u8]) -> Result<Image> {
    let picture = ::image::load_from_memory(bytes).map_err(|error| {
        make_error(
            ErrorCode::FileImportFailure,
            "the bytes are not an image this program reads",
            format!("{error}"),
        )
    })?;
    match picture {
        ::image::DynamicImage::ImageLuma8(grey) => {
            let (width, height) = (grey.width(), grey.height());
            Image::from_bytes(width, height, ImageFormat::Grey8, grey.into_raw())
        }
        other => {
            let rgba = other.into_rgba8();
            let (width, height) = (rgba.width(), rgba.height());
            Image::from_straight_rgba8(width, height, rgba.into_raw())
        }
    }
}

/// The image of the file at `path`. FileImportFailure for a file that cannot be read or is no
/// image this program reads, naming the file.
pub fn read_image(path: &Path) -> Result<Image> {
    let bytes = std::fs::read(path).map_err(|error| {
        make_error(
            ErrorCode::FileImportFailure,
            format!("could not read the image file: {error}"),
            path_text(path),
        )
    })?;
    decode_image(&bytes).map_err(|error| {
        let context = format!("{}: {}", path_text(path), error.context);
        error.with_context(context)
    })
}
