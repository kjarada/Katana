//! A sheet raster as a TIFF file (plotting/tiff_writer.cpp; docs/plotting.md, "Plot styles and
//! output").
//!
//! Written here rather than through an image library's encoder: Qt's TIFF support was a plugin
//! (qtiff) that an installation may not have had, and a plot format that works on one machine and
//! fails on the next is not a format. A baseline TIFF 6.0 file is a header, the pixel strips and
//! one directory of tags, so it is written directly:
//!
//! ```text
//! 8-bit grey (BlackIsZero) for a grey image, 24-bit RGB otherwise;
//! strips of about 256 KiB, each compressed with Deflate (compression 8, a zlib stream), which
//!   every TIFF reader in use decodes;
//! the resolution in dots per inch, so the raster prints at the scale it was plotted at;
//! and "Katana" as the software. Little-endian, one image.
//! ```
//!
//! The same image gives the same bytes, so a plot is reproducible. The zlib streams are
//! miniz_oxide's at zlib's default level (6, qCompress's), the workspace's one zlib
//! (katana_update inflates with it); their bytes are not zlib's own, and every reader inflates
//! them to the same pixels.

use std::path::Path;

use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::{ErrorCode, Status};

use crate::canvas::{Image, ImageFormat};

// TIFF 6.0, section 2: the field types used here.
const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;

/// The uncompressed bytes a strip holds, at most: large enough that Deflate has something to work
/// with, small enough that a reader streams the image.
const STRIP_BYTES: usize = 256 * 1024;

/// qCompress's level: zlib's default compression.
const ZLIB_LEVEL: u8 = 6;

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn set32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// One directory entry: its tag, type and count, and its value's bytes in file order - inline in
/// the entry when they fit in four, elsewhere when not.
struct Field {
    tag: u16,
    kind: u16,
    count: u32,
    value: Vec<u8>,
}

fn shorts(tag: u16, values: &[u16]) -> Field {
    let mut value = Vec::with_capacity(values.len() * 2);
    for &each in values {
        put16(&mut value, each);
    }
    Field {
        tag,
        kind: SHORT,
        count: values.len() as u32,
        value,
    }
}

fn longs(tag: u16, values: &[u32]) -> Field {
    let mut value = Vec::with_capacity(values.len() * 4);
    for &each in values {
        put32(&mut value, each);
    }
    Field {
        tag,
        kind: LONG,
        count: values.len() as u32,
        value,
    }
}

/// Dots per inch as a fraction: whole numbers exactly, others to a thousandth.
fn rational(tag: u16, value: f64) -> Field {
    let mut bytes = Vec::with_capacity(8);
    if value == value.floor() && value < 4.0e9 {
        put32(&mut bytes, value as u32);
        put32(&mut bytes, 1);
    } else {
        // std::lround of at most 4e9: within a u32.
        put32(
            &mut bytes,
            (katana_math::cpp::min(value, 4.0e6) * 1000.0).round() as u32,
        );
        put32(&mut bytes, 1000);
    }
    Field {
        tag,
        kind: RATIONAL,
        count: 1,
        value: bytes,
    }
}

fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

/// The image's rows as the TIFF holds them: grey bytes, or red, green and blue - an RGBA image's
/// colour with its alpha divided out and the alpha dropped (QImage's conversion to RGB888; a
/// plotted sheet is opaque, where the two are the same).
fn rows_of(image: &Image) -> Vec<u8> {
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

/// Writes `image` to `path` at `dpi` (writeTiff). InvalidArgument for a null image or a
/// resolution that is not positive; FileExportFailure when the file cannot be written. The file
/// is written beside its place and moved there whole (QSaveFile), so a failure leaves what was
/// there.
pub fn write_tiff(image: &Image, path: &Path, dpi: f64) -> Status {
    if image.is_null() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "there is no image to write",
            String::new(),
        ));
    }
    // A resolution that is not a number is not finite: refused with the rest.
    if dpi <= 0.0 || !dpi.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the resolution must be positive",
            String::new(),
        ));
    }
    let grey = image.format() == ImageFormat::Grey8;
    let pixels = rows_of(image);
    let samples: u16 = if grey { 1 } else { 3 };
    let width = image.width();
    let height = image.height();
    let row_bytes = width as usize * usize::from(samples);
    let rows_per_strip = (STRIP_BYTES / row_bytes).max(1) as u32;

    let mut file = Vec::new();
    file.extend_from_slice(b"II"); // little-endian
    put16(&mut file, 42);
    put32(&mut file, 0); // the directory's offset, once it is known

    // The strips, each a zlib stream.
    let mut offsets = Vec::new();
    let mut counts = Vec::new();
    let mut row = 0;
    while row < height {
        let rows = rows_per_strip.min(height - row);
        let from = row as usize * row_bytes;
        let to = from + rows as usize * row_bytes;
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(&pixels[from..to], ZLIB_LEVEL);
        offsets.push(file.len() as u32);
        counts.push(packed.len() as u32);
        file.extend_from_slice(&packed);
        if file.len() > i32::MAX as usize {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "the raster is too large for a TIFF file (over 2 GB compressed)",
                path_text(path),
            ));
        }
        row += rows_per_strip;
    }

    // The directory, its tags in ascending order as TIFF requires, on a word boundary; values
    // longer than four bytes follow it.
    let bits: &[u16] = if grey { &[8] } else { &[8, 8, 8] };
    // BlackIsZero for grey, RGB for colour.
    let photometric: u16 = if grey { 1 } else { 2 };
    let fields = [
        longs(256, &[width]),          // ImageWidth
        longs(257, &[height]),         // ImageLength
        shorts(258, bits),             // BitsPerSample
        shorts(259, &[8]),             // Compression: Deflate
        shorts(262, &[photometric]),   // PhotometricInterpretation
        longs(273, &offsets),          // StripOffsets
        shorts(277, &[samples]),       // SamplesPerPixel
        longs(278, &[rows_per_strip]), // RowsPerStrip
        longs(279, &counts),           // StripByteCounts
        rational(282, dpi),            // XResolution
        rational(283, dpi),            // YResolution
        shorts(284, &[1]),             // PlanarConfiguration
        shorts(296, &[2]),             // ResolutionUnit: inch
        // Software, with its NUL.
        Field {
            tag: 305,
            kind: ASCII,
            count: 7,
            value: b"Katana\0".to_vec(),
        },
    ];

    if !file.len().is_multiple_of(2) {
        file.push(0);
    }
    let directory = file.len() as u32;
    set32(&mut file, 4, directory);
    let beyond = directory + 2 + 12 * fields.len() as u32 + 4;
    let mut values = Vec::new();
    put16(&mut file, fields.len() as u16);
    for field in &fields {
        put16(&mut file, field.tag);
        put16(&mut file, field.kind);
        put32(&mut file, field.count);
        if field.value.len() <= 4 {
            let mut inline = field.value.clone();
            inline.resize(4, 0);
            file.extend_from_slice(&inline);
        } else {
            put32(&mut file, beyond + values.len() as u32);
            values.extend_from_slice(&field.value);
            if !values.len().is_multiple_of(2) {
                values.push(0);
            }
        }
    }
    put32(&mut file, 0); // no next directory
    file.extend_from_slice(&values);

    save_whole(path, &file)
}

/// `bytes` written to a file beside `path` and moved onto it (QSaveFile's commit).
fn save_whole(path: &Path, bytes: &[u8]) -> Status {
    let failed = || {
        make_error(
            ErrorCode::FileExportFailure,
            "could not write the TIFF file",
            path_text(path),
        )
    };
    let mut writing = path.as_os_str().to_owned();
    writing.push(".writing");
    let writing = std::path::PathBuf::from(writing);
    if std::fs::write(&writing, bytes).is_err() {
        let _ = std::fs::remove_file(&writing);
        return Err(failed());
    }
    if std::fs::rename(&writing, path).is_err() {
        let _ = std::fs::remove_file(&writing);
        return Err(failed());
    }
    Ok(())
}
