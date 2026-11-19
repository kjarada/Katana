//! Disjoint parts of one buffer handed to the task pool's parallel tasks.
//!
//! The rasteriser's stages each split one buffer between tasks that never
//! touch the same element: a chunk of primitives owns its bins, a range of
//! vertices its transformed copies, and a tile its pixels (which is the point
//! of tiling: no lock on a pixel, no atomic and no false sharing in the inner
//! loop). The task pool's bodies are `Fn + Sync`, which can reach a buffer
//! only through a shared reference; so the buffer is cut first, with the
//! slice methods (`chunks_mut`, `split_at_mut`), into the parts the tasks
//! will take, and each part is put behind a mutex of its own that only its
//! task ever locks. The lock is uncontended - one task an index, which the
//! pool runs once - and taken once a task, never a pixel. The C++ shared the
//! raw buffers, and so did this module until 2026-11-19, through raw pointers
//! (docs/rust.md, "The workspace").
//!
//! A part is reached at the index the pool hands its task, so the parts are
//! cut exactly as the tasks are numbered: one an item, a grain-sized chunk a
//! range (the pool's ranges are `grain` indices from the start, the last one
//! short), one a tile.

use crate::framebuffer::{Framebuffer, Rgba, TileRect, tile_rect};
use katana_core::task_pool::TaskPool;
use std::sync::{Mutex, MutexGuard};

// A part's own lock. A task that panicked while holding it poisons it, and the
// pool then ends the call with that panic; the part is never locked again.
fn take<T>(part: &Mutex<T>) -> MutexGuard<'_, T> {
    part.lock().unwrap_or_else(|error| error.into_inner())
}

/// `body(index, &mut items[index])` for every index, on the pool.
pub(crate) fn for_each_mut<T, F>(pool: &TaskPool, items: &mut [T], body: F)
where
    T: Send,
    F: Fn(usize, &mut T) + Sync,
{
    let parts: Vec<Mutex<&mut T>> = items.iter_mut().map(Mutex::new).collect();
    pool.parallel_for(0, parts.len(), |index| {
        let mut item = take(&parts[index]);
        body(index, &mut item);
    });
}

// One range's pieces of the three slices of for_each_range3.
type Pieces<'p, A, B, C> = (&'p mut [A], &'p mut [B], &'p mut [C]);

/// `body(low, &mut a[low..high], &mut b[low..high], &mut c[low..high])` for
/// ranges of `grain` indices covering the three slices, which have one length,
/// on the pool.
pub(crate) fn for_each_range3<A, B, C, F>(
    pool: &TaskPool,
    grain: usize,
    a: &mut [A],
    b: &mut [B],
    c: &mut [C],
    body: F,
) where
    A: Send,
    B: Send,
    C: Send,
    F: Fn(usize, &mut [A], &mut [B], &mut [C]) + Sync,
{
    let count = a.len();
    assert!(b.len() == count && c.len() == count);
    // The pool's own rule for a grain of zero.
    let grain = grain.max(1);
    let parts: Vec<Mutex<Pieces<'_, A, B, C>>> = a
        .chunks_mut(grain)
        .zip(b.chunks_mut(grain))
        .zip(c.chunks_mut(grain))
        .map(|((a, b), c)| Mutex::new((a, b, c)))
        .collect();
    pool.parallel_for(0, parts.len(), |index| {
        let mut range = take(&parts[index]);
        let (a, b, c) = &mut *range;
        body(index * grain, a, b, c);
    });
}

/// A framebuffer cut into its tiles for the pool's tasks, one task a tile.
pub(crate) struct TileTarget<'a> {
    tiles: Vec<Mutex<TilePixels<'a>>>,
}

impl<'a> TileTarget<'a> {
    pub(crate) fn new(target: &'a mut Framebuffer) -> Self {
        let width = target.width();
        let height = target.height();
        let tiles_across = target.tiles_across();
        let tile_count = target.tile_count();
        let (color, depth) = target.buffers_mut();
        assert!(color.len() == depth.len() && color.len() == width as usize * height as usize);
        let mut tiles: Vec<TilePixels<'a>> = (0..tile_count)
            .map(|index| {
                let rect = tile_rect(width, height, tiles_across, index);
                let rows = rect.height() as usize;
                TilePixels {
                    rect,
                    color: Vec::with_capacity(rows),
                    depth: Vec::with_capacity(rows),
                }
            })
            .collect();
        // A tile's pixels are a piece of each of its rows, not one run of the
        // buffer (the layout is row-major, framebuffer.rs says why): so each
        // row of the image is cut at the tiles' edges, left to right, and each
        // piece goes to the tile it lies in. Pieces of one row go to the tiles
        // of one band, `tiles_across` of them from the band's first.
        let stride = width as usize;
        let across = tiles_across as usize;
        let band_height = Framebuffer::TILE_SIZE as usize;
        for (y, (mut color_row, mut depth_row)) in color
            .chunks_exact_mut(stride)
            .zip(depth.chunks_exact_mut(stride))
            .enumerate()
        {
            let first = y / band_height * across;
            for tile in &mut tiles[first..first + across] {
                let span = tile.rect.width() as usize;
                let (color_piece, color_rest) = std::mem::take(&mut color_row).split_at_mut(span);
                let (depth_piece, depth_rest) = std::mem::take(&mut depth_row).split_at_mut(span);
                tile.color.push(color_piece);
                tile.depth.push(depth_piece);
                color_row = color_rest;
                depth_row = depth_rest;
            }
            assert!(color_row.is_empty() && depth_row.is_empty());
        }
        Self {
            tiles: tiles.into_iter().map(Mutex::new).collect(),
        }
    }

    /// Fills every tile's colour with `background` and its depth with 0, in
    /// parallel over the tiles.
    pub(crate) fn fill(&mut self, pool: &TaskPool, background: Rgba) {
        let tiles = &self.tiles;
        pool.parallel_for(0, tiles.len(), |index| {
            let mut pixels = take(&tiles[index]);
            let rect = pixels.rect();
            for y in rect.y0..rect.y1 {
                let (color, depth) = pixels.row(y);
                color.fill(background);
                depth.fill(0.0);
            }
        });
    }

    /// `body(index, &mut pixels of tile index, &mut per_tile[index])` for every
    /// tile, on the pool.
    pub(crate) fn for_each<S, F>(&mut self, pool: &TaskPool, per_tile: &mut [S], body: F)
    where
        S: Send,
        F: Fn(usize, &mut TilePixels<'_>, &mut S) + Sync,
    {
        assert_eq!(per_tile.len(), self.tiles.len());
        let tiles = &self.tiles;
        let states: Vec<Mutex<&mut S>> = per_tile.iter_mut().map(Mutex::new).collect();
        pool.parallel_for(0, tiles.len(), |index| {
            let mut pixels = take(&tiles[index]);
            let mut state = take(&states[index]);
            body(index, &mut pixels, &mut state);
        });
    }
}

/// One tile's pixels, reached a row at a time.
pub(crate) struct TilePixels<'a> {
    rect: TileRect,
    // Row y of the tile is element y - rect.y0 of each.
    color: Vec<&'a mut [Rgba]>,
    depth: Vec<&'a mut [f32]>,
}

impl TilePixels<'_> {
    pub(crate) fn rect(&self) -> TileRect {
        self.rect
    }

    /// Row `y` of the tile: its colour and depth from x0 to x1 (exclusive), so
    /// pixel x of the image is element x - x0.
    pub(crate) fn row(&mut self, y: i32) -> (&mut [Rgba], &mut [f32]) {
        assert!((self.rect.y0..self.rect.y1).contains(&y) && self.rect.x0 < self.rect.x1);
        let at = (y - self.rect.y0) as usize;
        (&mut *self.color[at], &mut *self.depth[at])
    }
}
