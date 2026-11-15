//! What this target's painted twins paint with, and read their pixels by: the window's own raster
//! canvas (canvas::RasterCanvas over tiny-skia), through the icons' rasteriser, so that the icons
//! and the tool buttons are painted and read back as the C++ tests read a QImage's.
//!
//! Until the raster canvas was built (step 7's wave 1, beside this target), a stand-in canvas lived
//! here; it went when the raster canvas joined, so there is one raster canvas, not two.

use katana_window::canvas::{Color, Image};
use katana_window::icons::CanvasRasteriser;

/// The rasteriser the twins paint with: the window's own.
pub fn raster() -> CanvasRasteriser {
    CanvasRasteriser
}

/// The 8-connected pieces of the pixels of `image` that are `ink` exactly (piecesOf).
pub fn pieces_of(image: &Image, ink: Color) -> usize {
    let width = image.width() as i64;
    let height = image.height() as i64;
    let is_ink = |x: i64, y: i64| {
        x >= 0 && y >= 0 && x < width && y < height && image.pixel(x as u32, y as u32) == ink
    };
    let mut seen = vec![false; (width * height) as usize];
    let mut pieces = 0;
    for y in 0..height {
        for x in 0..width {
            if !is_ink(x, y) || seen[(y * width + x) as usize] {
                continue;
            }
            pieces += 1;
            let mut todo = vec![(x, y)];
            seen[(y * width + x) as usize] = true;
            while let Some((at_x, at_y)) = todo.pop() {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (at_x + dx, at_y + dy);
                        if is_ink(nx, ny) && !seen[(ny * width + nx) as usize] {
                            seen[(ny * width + nx) as usize] = true;
                            todo.push((nx, ny));
                        }
                    }
                }
            }
        }
    }
    pieces
}
