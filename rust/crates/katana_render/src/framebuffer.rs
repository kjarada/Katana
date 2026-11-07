//! Colour and depth target for the software renderer (PLAN.MD Phase 15).
//!
//! LAYOUT. Pixels are row-major and contiguous, not swizzled into tile-local
//! blocks. A swizzled layout gives a rasteriser marginally better locality, but
//! every frame ends up in an image handed to the window, and row-major is the
//! one layout that can be handed over without a copy. A 64x64 tile still
//! touches only 64 rows of a single buffer, so the locality that matters -
//! staying inside L2 while a tile is being shaded - is preserved either way.
//! Measure before changing this.
//!
//! COLOUR is 0xAARRGGBB packed in a `u32`, which is byte-for-byte a 32-bit ARGB
//! image on a little-endian machine.
//!
//! DEPTH is normalised [0, 1] and REVERSED: 1 at the near plane, 0 at the far
//! (camera.rs says why: it keeps a float's precision where the perspective
//! divide needs it). Larger is nearer and the buffer is cleared to 0; the test
//! is strictly-greater, so of two primitives at exactly the same depth the
//! first rasterised wins, which combined with a fixed binning order is what
//! makes a frame reproducible (Rule 7).

use crate::scalar::{clamp, min};
use katana_core::{Error, ErrorCode, Result, Status};

pub type Rgba = u32;

/// An opaque colour: `rgba_with_alpha(r, g, b, 255)`.
pub const fn rgba(r: u8, g: u8, b: u8) -> Rgba {
    rgba_with_alpha(r, g, b, 255)
}

pub const fn rgba_with_alpha(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    ((a as Rgba) << 24) | ((r as Rgba) << 16) | ((g as Rgba) << 8) | (b as Rgba)
}

pub const fn red_of(c: Rgba) -> u8 {
    (c >> 16) as u8
}

pub const fn green_of(c: Rgba) -> u8 {
    (c >> 8) as u8
}

pub const fn blue_of(c: Rgba) -> u8 {
    c as u8
}

pub const fn alpha_of(c: Rgba) -> u8 {
    (c >> 24) as u8
}

/// Scales the three colour channels by `factor` (clamped to [0, 1] by the
/// caller's lighting), leaving alpha alone. Used to bake flat shading into
/// vertex colours before anything reaches the rasteriser.
pub fn shade(color: Rgba, factor: f64) -> Rgba {
    let f = clamp(factor, 0.0, 1.0);
    // +0.5 then truncate: round-to-nearest without a library call, and
    // monotone in `value`, so a brighter input never shades darker.
    let channel = |value: u8| (f64::from(value) * f + 0.5) as u8;
    (color & 0xFF00_0000)
        | (Rgba::from(channel(red_of(color))) << 16)
        | (Rgba::from(channel(green_of(color))) << 8)
        | Rgba::from(channel(blue_of(color)))
}

/// A rectangle of pixels, in pixel coordinates with the origin at the top-left.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TileRect {
    pub x0: i32,
    pub y0: i32,
    /// Exclusive.
    pub x1: i32,
    /// Exclusive.
    pub y1: i32,
}

impl TileRect {
    pub const fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    pub const fn height(&self) -> i32 {
        self.y1 - self.y0
    }

    pub const fn empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
}

// 64 megapixels. Far beyond any real display wall, and small enough that the
// colour plus depth allocation (512 MiB) still fails cleanly rather than
// taking the process out.
const MAX_PIXELS: usize = 64 * 1024 * 1024;

/// Tile `index` of a `width` x `height` target `tiles_across` tiles wide,
/// clipped to the target. The caller has checked that `index` is a tile.
pub(crate) fn tile_rect(width: i32, height: i32, tiles_across: i32, index: usize) -> TileRect {
    let tx = (index % tiles_across as usize) as i32;
    let ty = (index / tiles_across as usize) as i32;
    let x0 = tx * Framebuffer::TILE_SIZE;
    let y0 = ty * Framebuffer::TILE_SIZE;
    TileRect {
        x0,
        y0,
        x1: min(x0 + Framebuffer::TILE_SIZE, width),
        y1: min(y0 + Framebuffer::TILE_SIZE, height),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Framebuffer {
    width: i32,
    height: i32,
    tiles_across: i32,
    tiles_down: i32,
    color: Vec<Rgba>,
    depth: Vec<f32>,
}

impl Framebuffer {
    /// Tiles are 64x64: 4096 pixels of colour (16 KiB) plus depth (16 KiB)
    /// fits in a typical 256 KiB L2 many times over, leaving room for the
    /// primitive list and the vertex data a tile touches.
    pub const TILE_SIZE: i32 = 64;

    /// Fails with InvalidArgument for a non-positive or absurd size. The cap is
    /// deliberately generous (64 megapixels) and exists so a corrupt widget size
    /// cannot ask for an allocation that takes the process down.
    pub fn create(width: i32, height: i32) -> Result<Self> {
        let mut framebuffer = Self::default();
        framebuffer.resize(width, height)?;
        Ok(framebuffer)
    }

    /// Reuses the existing allocation when the size is unchanged.
    pub fn resize(&mut self, width: i32, height: i32) -> Status {
        if width <= 0 || height <= 0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "framebuffer size must be positive",
            ));
        }
        let pixels = width as usize * height as usize;
        if pixels > MAX_PIXELS {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "framebuffer is larger than 64 megapixels",
            ));
        }
        if width == self.width && height == self.height {
            return Ok(()); // the allocation is already the right size
        }

        self.width = width;
        self.height = height;
        // Both positive, so the unsigned ceiling is the C++'s
        // (width + kTileSize - 1) / kTileSize.
        let tile = Self::TILE_SIZE as usize;
        self.tiles_across = (width as usize).div_ceil(tile) as i32;
        self.tiles_down = (height as usize).div_ceil(tile) as i32;
        self.color.clear();
        self.color.resize(pixels, 0);
        self.depth.clear();
        self.depth.resize(pixels, 0.0);
        Ok(())
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    pub fn tiles_across(&self) -> i32 {
        self.tiles_across
    }

    pub fn tiles_down(&self) -> i32 {
        self.tiles_down
    }

    pub fn tile_count(&self) -> usize {
        self.tiles_across as usize * self.tiles_down as usize
    }

    /// Clipped to the framebuffer, so the right and bottom tiles are partial.
    pub fn tile(&self, index: usize) -> TileRect {
        if index >= self.tile_count() {
            return TileRect::default();
        }
        tile_rect(self.width, self.height, self.tiles_across, index)
    }

    pub fn color(&self) -> &[Rgba] {
        &self.color
    }

    pub fn color_mut(&mut self) -> &mut [Rgba] {
        &mut self.color
    }

    pub fn depth(&self) -> &[f32] {
        &self.depth
    }

    pub fn depth_mut(&mut self) -> &mut [f32] {
        &mut self.depth
    }

    /// Colour and depth together, for the rasteriser's tile tasks.
    pub(crate) fn buffers_mut(&mut self) -> (&mut [Rgba], &mut [f32]) {
        (self.color.as_mut_slice(), self.depth.as_mut_slice())
    }

    pub fn color_at(&self, x: i32, y: i32) -> Rgba {
        self.color[self.index(x, y)]
    }

    pub fn depth_at(&self, x: i32, y: i32) -> f32 {
        self.depth[self.index(x, y)]
    }

    /// Fills colour with `background` and depth with 0 (the far plane).
    pub fn clear(&mut self, background: Rgba) {
        self.color.fill(background);
        self.depth.fill(0.0);
    }

    /// Same, but only inside `rect`. Used when a tile is rendered on its own.
    pub fn clear_tile(&mut self, rect: &TileRect, background: Rgba) {
        for y in rect.y0..rect.y1 {
            let row = self.index(0, y);
            let from = row + rect.x0 as usize;
            let to = row + rect.x1 as usize;
            self.color[from..to].fill(background);
            self.depth[from..to].fill(0.0);
        }
    }

    fn index(&self, x: i32, y: i32) -> usize {
        y as usize * self.width as usize + x as usize
    }
}
