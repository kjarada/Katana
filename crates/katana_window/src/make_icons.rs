//! katana_make_icons <output-directory>
//!
//! A DEVELOPER tool, not part of the product and not built by default. It runs the same painting
//! code the application uses and writes:
//!
//! ```text
//! katana.ico        the application icon, 16 to 256 px, for the Windows resource in katana.rc
//!                   and for an installer
//! katana.png        the 256 px icon, for a README or a store listing
//! icon_sheet.png    every toolbar icon on the theme's own ground, large enough to judge - the
//!                   way to review the set without launching the application
//! ```
//!
//! The .ico is COMMITTED (resources/katana.ico) rather than generated during the build.
//! Generating it would make an ordinary build run a program before it can compile a resource
//! file, which fails wherever that program cannot start - a cross build, a CI image with no
//! display - and buys nothing, since the icon changes about once a year. The program is
//! src/bin/katana_make_icons, which calls `run`.

use std::path::Path;

use katana_app::console::Console;
use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::{ErrorCode, Status};

use crate::canvas::{Brush, Canvas, PointF, RectF, encode_png, write_png};
use crate::icons::{Rasteriser, all_icons, application_icon_image, default_rasteriser, paint_icon};
use crate::theme::palette;

/// The sizes katana.ico holds, 16 to 256 px.
pub const ICO_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

fn shown(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

fn append_little_endian(bytes: &mut Vec<u8>, value: u32, size: usize) {
    for at in 0..size {
        bytes.push(((value >> (8 * at)) & 0xff) as u8);
    }
}

/// An .ico holding one PNG per size. PNG-compressed entries have been valid since Windows Vista
/// and are a fraction of the size of the BMP form; the header is six bytes and each directory
/// entry sixteen (Microsoft, "Icons", ICONDIR and ICONDIRENTRY). `images` are each size with its
/// PNG's bytes.
pub fn ico_bytes(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    append_little_endian(&mut file, 0, 2); // reserved
    append_little_endian(&mut file, 1, 2); // 1 = icon
    append_little_endian(&mut file, images.len() as u32, 2); // image count
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in images {
        // A width or height of 256 is written as 0: the field is one byte.
        let dimension = if *size >= 256 { 0 } else { *size };
        append_little_endian(&mut file, dimension, 1);
        append_little_endian(&mut file, dimension, 1);
        append_little_endian(&mut file, 0, 1); // palette size: none
        append_little_endian(&mut file, 0, 1); // reserved
        append_little_endian(&mut file, 1, 2); // colour planes
        append_little_endian(&mut file, 32, 2); // bits per pixel
        append_little_endian(&mut file, png.len() as u32, 4);
        append_little_endian(&mut file, offset, 4);
        offset += png.len() as u32;
    }
    for (_, png) in images {
        file.extend_from_slice(png);
    }
    file
}

/// katana.ico: the application icon at each of `sizes`, painted by `raster`.
pub fn write_ico(path: &Path, sizes: &[u32], raster: &dyn Rasteriser) -> Status {
    let mut images = Vec::new();
    for &size in sizes {
        images.push((size, encode_png(&application_icon_image(raster, size))?));
    }
    std::fs::write(path, ico_bytes(&images)).map_err(|error| {
        make_error(
            ErrorCode::FileExportFailure,
            format!("could not write the icon file: {error}"),
            shown(path),
        )
    })
}

/// Every icon at 96 px, eight to a row, on the window colour, then the same icons at the 20 px
/// they ship at in one row, then the application icon. Large, because the point is to look at
/// them.
pub fn write_sheet(path: &Path, raster: &dyn Rasteriser) -> Status {
    const CELL: i32 = 120;
    const ICON: i32 = 96;
    const COLUMNS: i32 = 8;
    let icons = all_icons();
    let rows = (icons.len().div_ceil(COLUMNS as usize) + 3) as i32;
    let application: Vec<(u32, crate::canvas::Image)> = [192_u32, 96, 48, 32, 24, 16]
        .into_iter()
        .map(|size| (size, application_icon_image(raster, size)))
        .collect();
    let sheet = raster.paint(
        (COLUMNS * CELL) as u32,
        (rows * CELL) as u32,
        1.0,
        &mut |canvas: &mut dyn Canvas| {
            canvas.fill_rect(
                RectF::new(0.0, 0.0, f64::from(COLUMNS * CELL), f64::from(rows * CELL)),
                &Brush::solid(palette::window()),
            );
            for (index, &icon) in (0_i32..).zip(icons) {
                let column = index % COLUMNS;
                let row = index / COLUMNS;
                let cell = RectF::new(
                    f64::from(column * CELL) + f64::from(CELL - ICON) / 2.0,
                    f64::from(row * CELL) + f64::from(CELL - ICON) / 2.0,
                    f64::from(ICON),
                    f64::from(ICON),
                );
                paint_icon(canvas, icon, cell, palette::text(), palette::accent());
            }
            // The same icons at the size they are actually used, 20 px, in one row: what reads
            // at 96 does not always read at 20, and 20 is what ships.
            let small_row = (rows - 3) * CELL + 30;
            for (index, &icon) in (0_i32..).zip(icons) {
                let cell = RectF::new(
                    16.0 + f64::from(index) * 27.0,
                    f64::from(small_row),
                    20.0,
                    20.0,
                );
                paint_icon(canvas, icon, cell, palette::text(), palette::accent());
            }
            // The application icon, large and at the sizes a taskbar and a title bar use.
            let app_row = (rows - 2) * CELL;
            let mut x = 16;
            for (size, image) in &application {
                let size = *size as i32;
                canvas.draw_image_at(
                    PointF::new(
                        f64::from(x),
                        f64::from(app_row) + f64::from(2 * CELL - size) / 2.0,
                    ),
                    image,
                );
                x += size + 28;
            }
        },
    );
    write_png(&sheet, path)
}

/// The three files into `directory`.
pub fn make_icons(directory: &Path, raster: &dyn Rasteriser) -> Status {
    write_ico(&directory.join("katana.ico"), &ICO_SIZES, raster)?;
    write_png(
        &application_icon_image(raster, 256),
        &directory.join("katana.png"),
    )?;
    write_sheet(&directory.join("icon_sheet.png"), raster)
}

/// The program: `args` as the command line gives them (the program's own name first), its words
/// to `console`, and the exit code - 2 for no directory, 1 for one that cannot be made or written
/// into, 0 when the three files are written.
pub fn run(args: &[String], console: &mut dyn Console) -> i32 {
    let Some(directory) = args.get(1) else {
        console.err("usage: katana_make_icons <output-directory>\n");
        return 2;
    };
    let path = Path::new(directory);
    if !path.exists() && std::fs::create_dir_all(path).is_err() {
        console.err(&format!("cannot create {directory}\n"));
        return 1;
    }
    let Some(raster) = default_rasteriser() else {
        console.err("katana_make_icons: this build has no raster canvas to paint with\n");
        return 1;
    };
    if make_icons(path, raster.as_ref()).is_err() {
        console.err(&format!("failed to write the icons into {directory}\n"));
        return 1;
    }
    console.out(&format!(
        "wrote katana.ico, katana.png and icon_sheet.png into {directory}\n"
    ));
    0
}
