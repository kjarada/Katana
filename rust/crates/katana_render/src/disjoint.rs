//! Disjoint parts of one buffer handed to the task pool's parallel tasks.
//!
//! The rasteriser's stages each split one buffer between tasks that never
//! touch the same element: a chunk of primitives owns its bins, a range of
//! vertices its transformed copies, and a tile its pixels (which is the point
//! of tiling: no lock, no atomic and no false sharing in the inner loop). The
//! C++ shares the raw buffers; the task pool's bodies here are `Fn + Sync`, so
//! the split is made with raw pointers in this module alone, behind functions
//! that hand each task only its own part.
//!
//! Every function here rests on the pool's contract (katana_core::task_pool,
//! the TaskPool cases): `parallel_for` runs each index of its range exactly
//! once, `parallel_ranges` hands out ranges that do not overlap and lie inside
//! the range asked for, and neither returns before every task has finished,
//! a panicking one included.

#![allow(
    unsafe_code,
    reason = "Parallel tasks borrow disjoint parts of one buffer, which the borrow checker cannot see through the pool's Fn + Sync bodies."
)]

use crate::framebuffer::{Framebuffer, Rgba, TileRect, tile_rect};
use katana_core::task_pool::TaskPool;
use std::marker::PhantomData;

/// A pointer the tasks share. Each task dereferences it only at the indices
/// that are its own.
struct SharedPointer<T>(*mut T);

impl<T> Clone for SharedPointer<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SharedPointer<T> {}

// SAFETY: a task reaches through the pointer only the elements no other task
// reaches (the module comment), so sharing it amounts to sending each task
// exclusive access to its own elements: sound exactly when T may be sent.
unsafe impl<T: Send> Send for SharedPointer<T> {}
// SAFETY: as for Send.
unsafe impl<T: Send> Sync for SharedPointer<T> {}

impl<T> SharedPointer<T> {
    fn get(self) -> *mut T {
        self.0
    }
}

/// `body(index, &mut items[index])` for every index, on the pool.
pub(crate) fn for_each_mut<T, F>(pool: &TaskPool, items: &mut [T], body: F)
where
    T: Send,
    F: Fn(usize, &mut T) + Sync,
{
    let base = SharedPointer(items.as_mut_ptr());
    let count = items.len();
    pool.parallel_for(0, count, |index| {
        assert!(index < count);
        // SAFETY: index is inside `items`, which this call borrows mutably until
        // every task has finished, and the pool runs each index once: this is
        // the only reference to items[index].
        let item = unsafe { &mut *base.get().add(index) };
        body(index, item);
    });
}

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
    let (pa, pb, pc) = (
        SharedPointer(a.as_mut_ptr()),
        SharedPointer(b.as_mut_ptr()),
        SharedPointer(c.as_mut_ptr()),
    );
    pool.parallel_ranges(0, count, grain, |low, high| {
        assert!((low..=count).contains(&high));
        // SAFETY: [low, high) is inside the three slices, which this call
        // borrows mutably until every task has finished, and no other task is
        // given an index of it.
        let (sa, sb, sc) = unsafe {
            (
                std::slice::from_raw_parts_mut(pa.get().add(low), high - low),
                std::slice::from_raw_parts_mut(pb.get().add(low), high - low),
                std::slice::from_raw_parts_mut(pc.get().add(low), high - low),
            )
        };
        body(low, sa, sb, sc);
    });
}

/// A framebuffer split into its tiles for the pool's tasks, one task a tile.
pub(crate) struct TileTarget<'a> {
    color: SharedPointer<Rgba>,
    depth: SharedPointer<f32>,
    width: i32,
    height: i32,
    tiles_across: i32,
    tile_count: usize,
    _target: PhantomData<&'a mut Framebuffer>,
}

impl<'a> TileTarget<'a> {
    pub(crate) fn new(target: &'a mut Framebuffer) -> Self {
        let width = target.width();
        let height = target.height();
        let tiles_across = target.tiles_across();
        let tile_count = target.tile_count();
        let (color, depth) = target.buffers_mut();
        assert!(color.len() == depth.len() && color.len() == width as usize * height as usize);
        Self {
            color: SharedPointer(color.as_mut_ptr()),
            depth: SharedPointer(depth.as_mut_ptr()),
            width,
            height,
            tiles_across,
            tile_count,
            _target: PhantomData,
        }
    }

    /// Fills every tile's colour with `background` and its depth with 0, in
    /// parallel over the tiles.
    pub(crate) fn fill(&mut self, pool: &TaskPool, background: Rgba) {
        let this = &*self;
        pool.parallel_for(0, this.tile_count, |index| {
            let mut pixels = this.pixels(index);
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
        assert_eq!(per_tile.len(), self.tile_count);
        let this = &*self;
        let states = SharedPointer(per_tile.as_mut_ptr());
        pool.parallel_for(0, this.tile_count, |index| {
            let mut pixels = this.pixels(index);
            assert!(index < this.tile_count);
            // SAFETY: index is inside `per_tile`, which this call borrows
            // mutably until every task has finished, and the pool runs each
            // index once.
            let state = unsafe { &mut *states.get().add(index) };
            body(index, &mut pixels, state);
        });
    }

    /// Tile `index`'s pixels. Private, and made only by fill and for_each, one
    /// for each task: the tiles do not overlap, so neither do the pixels any
    /// two tasks reach.
    fn pixels(&self, index: usize) -> TilePixels<'_> {
        assert!(index < self.tile_count);
        TilePixels {
            color: self.color,
            depth: self.depth,
            stride: self.width as usize,
            rect: tile_rect(self.width, self.height, self.tiles_across, index),
            _target: PhantomData,
        }
    }
}

/// One tile's pixels, reached a row at a time.
pub(crate) struct TilePixels<'a> {
    color: SharedPointer<Rgba>,
    depth: SharedPointer<f32>,
    stride: usize,
    rect: TileRect,
    _target: PhantomData<&'a mut Framebuffer>,
}

impl TilePixels<'_> {
    pub(crate) fn rect(&self) -> TileRect {
        self.rect
    }

    /// Row `y` of the tile: its colour and depth from x0 to x1 (exclusive), so
    /// pixel x of the image is element x - x0.
    pub(crate) fn row(&mut self, y: i32) -> (&mut [Rgba], &mut [f32]) {
        assert!((self.rect.y0..self.rect.y1).contains(&y) && self.rect.x0 < self.rect.x1);
        let start = y as usize * self.stride + self.rect.x0 as usize;
        let length = (self.rect.x1 - self.rect.x0) as usize;
        // SAFETY: the tile is a rectangle of the framebuffer (tile_rect clips it
        // to the image), so the row lies inside both buffers; no other task
        // reaches a pixel of this tile, and &mut self keeps one row borrowed
        // at a time.
        unsafe {
            (
                std::slice::from_raw_parts_mut(self.color.get().add(start), length),
                std::slice::from_raw_parts_mut(self.depth.get().add(start), length),
            )
        }
    }
}
