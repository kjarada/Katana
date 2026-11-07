//! Contour lines of a TinSurface.
//!
//! Definition. The contour at level L is the boundary of the region { z >= L }
//! inside the surface. A vertex counts as "above" when z >= L (a vertex exactly
//! at the level is treated as infinitesimally above it), which settles every
//! degenerate case without special handling:
//!   * a level through a vertex gives one polyline through it - no duplicate or
//!     zero-length pieces and no gaps;
//!   * an edge lying in the level is traced once, as the rim of the higher side;
//!   * a flat area (plateau) at the level is outlined where the ground falls away
//!     from it and is not filled with lines; if ground rises from it on all sides
//!     (a flat pit floor) or the whole surface is at the level there is no contour;
//!   * a summit exactly at the level would give a single point; it is dropped.
//!
//! Contours are traced triangle to triangle through the surface adjacency, so the
//! pieces join exactly (no endpoint matching). Open contours begin and end on the
//! rim of the surface (outer edge or a hole); the others are closed rings.
//! Higher ground is on the left of the direction of travel, hence a closed
//! contour around a hill is counter-clockwise (positive signed area) and one
//! around a depression clockwise.
//!
//! Order: levels ascending; within a level the open contours first, then the
//! rings, each group by the index of the triangle where tracing starts.

use std::sync::Mutex;

use katana_core::task_pool::TaskPool;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Polyline2;
use katana_math::numerics::tolerance;

use crate::tin_surface::{NO_TRIANGLE, Point2, TinSurface, compute_by_index, std_max3, std_min3};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Contour {
    pub elevation: f64,
    /// index contour (see contours())
    pub major: bool,
    /// closed = ring
    pub line: Polyline2,
}

/// Refuses requests for more levels than this (InvalidArgument): an interval far
/// too small for the relief is a unit mistake, not a wish for millions of lines.
pub const MAX_CONTOUR_LEVELS: usize = 100000;

/// Marching triangles with the "z >= level is above" rule. A triangle is crossed
/// when it has vertices on both sides; it then has exactly one edge running from
/// above to below (in its counter-clockwise order) and one running from below to
/// above. The contour enters through the first and leaves through the second,
/// which puts the higher ground on its left; in the neighbour across the exit
/// edge that same edge runs the other way and is therefore its entry edge.
struct LevelTracer<'a> {
    surface: &'a TinSurface,
    visited_stamp: Vec<u32>,
    level: f64,
    stamp: u32,
}

impl<'a> LevelTracer<'a> {
    fn new(surface: &'a TinSurface) -> Self {
        Self {
            surface,
            visited_stamp: vec![0; surface.triangle_count()],
            level: 0.0,
            stamp: 0,
        }
    }

    /// Traces all contours of `level` through `candidates` (every crossed
    /// triangle, ascending). `stamp` must be unique per call and non-zero.
    fn trace(
        &mut self,
        level: f64,
        major: bool,
        candidates: &[u32],
        stamp: u32,
        output: &mut Vec<Contour>,
    ) {
        self.level = level;
        self.stamp = stamp;
        // Open contours start where the entry edge lies on the rim. Tracing them
        // first leaves only closed rings for the second sweep.
        for &t in candidates {
            if self.visited_stamp[t as usize] != self.stamp
                && self.surface.neighbors()[t as usize][self.entry_edge(t)] == NO_TRIANGLE
            {
                let line = self.follow(t);
                self.emit(line, major, output);
            }
        }
        for &t in candidates {
            if self.visited_stamp[t as usize] != self.stamp {
                let line = self.follow(t);
                self.emit(line, major, output);
            }
        }
    }

    fn is_crossed(&self, t: u32, level: f64) -> bool {
        let tri = self.surface.triangles()[t as usize];
        let vertices = self.surface.vertices();
        let a = vertices[tri[0] as usize].z >= level;
        let b = vertices[tri[1] as usize].z >= level;
        let c = vertices[tri[2] as usize].z >= level;
        a != b || b != c
    }

    fn above(&self, vertex: u32) -> bool {
        self.surface.vertices()[vertex as usize].z >= self.level
    }

    fn entry_edge(&self, t: u32) -> usize {
        let tri = self.surface.triangles()[t as usize];
        for k in 0..3 {
            if self.above(tri[k]) && !self.above(tri[(k + 1) % 3]) {
                return k;
            }
        }
        0 // not reached for a crossed triangle
    }

    fn exit_edge(&self, t: u32) -> usize {
        let tri = self.surface.triangles()[t as usize];
        for k in 0..3 {
            if !self.above(tri[k]) && self.above(tri[(k + 1) % 3]) {
                return k;
            }
        }
        0 // not reached for a crossed triangle
    }

    /// Where the level meets edge k of triangle t. Always evaluated from the low
    /// end to the high end, so both triangles sharing the edge get the identical
    /// point; a high end exactly at the level is returned as is.
    fn crossing(&self, t: u32, k: usize) -> Point2 {
        let tri = self.surface.triangles()[t as usize];
        let first = self.surface.vertices()[tri[k] as usize];
        let second = self.surface.vertices()[tri[(k + 1) % 3] as usize];
        let (low, high) = if first.z >= self.level {
            (second, first)
        } else {
            (first, second)
        };
        if high.z == self.level {
            return Point2::new(high.x, high.y);
        }
        let fraction = (self.level - low.z) / (high.z - low.z);
        Point2::new(
            low.x + fraction * (high.x - low.x),
            low.y + fraction * (high.y - low.y),
        )
    }

    fn follow(&mut self, start: u32) -> Polyline2 {
        let mut line = Polyline2::default();
        line.vertices
            .push(self.crossing(start, self.entry_edge(start)));
        let mut t = start;
        loop {
            self.visited_stamp[t as usize] = self.stamp;
            let exit = self.exit_edge(t);
            let next = self.surface.neighbors()[t as usize][exit];
            if next == start {
                line.closed = true; // the exit point would repeat the first point
                break;
            }
            line.vertices.push(self.crossing(t, exit));
            if next == NO_TRIANGLE || self.visited_stamp[next as usize] == self.stamp {
                break; // reached the rim (the second case cannot occur on a valid surface)
            }
            t = next;
        }
        line
    }

    /// Drops the zero-length pieces that arise where the level passes exactly
    /// through vertices, and contours that have no extent at all.
    fn emit(&self, mut line: Polyline2, major: bool, output: &mut Vec<Contour>) {
        line.vertices.dedup();
        if line.closed {
            while line.vertices.len() > 1 && line.vertices.first() == line.vertices.last() {
                line.vertices.pop();
            }
        }
        if line.vertices.len() < if line.closed { 3 } else { 2 } {
            return;
        }
        output.push(Contour {
            elevation: self.level,
            major,
            line,
        });
    }
}

/// Contours of one level. Fails with InvalidArgument for a non-finite elevation.
/// A level outside the elevation range gives no contours.
pub fn contour_at(surface: &TinSurface, elevation: f64) -> Result<Vec<Contour>> {
    if !elevation.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "contour elevation must be finite",
        ));
    }
    let mut output = Vec::new();
    if surface.empty()
        || !(elevation > surface.min_elevation())
        || elevation > surface.max_elevation()
    {
        return Ok(output); // { z >= level } is everything or nothing: no boundary
    }
    let mut tracer = LevelTracer::new(surface);
    let candidates: Vec<u32> = (0..surface.triangle_count() as u32)
        .filter(|&t| tracer.is_crossed(t, elevation))
        .collect();
    tracer.trace(elevation, false, &candidates, 1, &mut output);
    Ok(output)
}

#[derive(Clone, Copy)]
struct LevelRange {
    first: i64,
    /// empty when last < first
    last: i64,
}

/// Contours at base + k * interval for every integer k with a level inside the
/// elevation range. Levels with k divisible by `major_every` are flagged major
/// (major_every == 0: none). Fails with InvalidArgument when interval is not
/// finite and larger than tolerance::GEOMETRIC, base is not finite, or more than
/// MAX_CONTOUR_LEVELS levels would be needed.
///
/// Levels are traced in parallel on `pool` (None: TaskPool::shared()), each
/// into its own list, and the lists are joined in level order: the result is
/// identical at any thread count, TaskPool::new(0) included.
pub fn contours(
    surface: &TinSurface,
    interval: f64,
    base: f64,
    major_every: usize,
    pool: Option<&TaskPool>,
) -> Result<Vec<Contour>> {
    // Levels closer than GEOMETRIC would be the same geometric feature (and,
    // far below that, no longer distinct doubles at survey elevations).
    if !(interval > tolerance::GEOMETRIC) || !interval.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "contour interval must be finite and larger than tolerance::kGeometric",
        )
        .with_context(format!("interval={}", format_exact_real(interval))));
    }
    if !base.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "contour base elevation must be finite",
        ));
    }
    let mut output = Vec::new();
    if surface.empty() {
        return Ok(output);
    }

    // Level k is base + k * interval, always evaluated by this one expression so
    // that every comparison sees the same double. A triangle (or the surface)
    // with elevation range [lo, hi] is crossed by the levels in (lo, hi].
    let level = |k: i64| base + k as f64 * interval;
    // The estimates from the division can be one off; the loops settle them
    // against the level values actually used.
    let first_level_above = |z: f64| {
        let mut k = ((z - base) / interval).floor() as i64;
        while level(k) > z {
            k -= 1;
        }
        while !(level(k) > z) {
            k += 1;
        }
        k
    };
    let last_level_at_or_below = |z: f64| first_level_above(z) - 1;

    // Beyond 2^52 the level index itself is no longer exact.
    const MAX_LEVEL_INDEX: f64 = 4.0e15;
    if ((surface.min_elevation() - base) / interval).abs() > MAX_LEVEL_INDEX
        || ((surface.max_elevation() - base) / interval).abs() > MAX_LEVEL_INDEX
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "contour interval is too small for the elevations of the surface",
        )
        .with_context(format!("interval={}", format_exact_real(interval))));
    }
    let first_k = first_level_above(surface.min_elevation());
    let last_k = last_level_at_or_below(surface.max_elevation());
    if last_k < first_k {
        return Ok(output);
    }
    let level_count = (last_k - first_k + 1) as usize;
    if level_count > MAX_CONTOUR_LEVELS {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "contour interval is too small for the relief of the surface",
        )
        .with_context(format!("levels={level_count} limit={MAX_CONTOUR_LEVELS}")));
    }

    let workers: &TaskPool = match pool {
        Some(pool) => pool,
        None => TaskPool::shared(),
    };

    // The levels crossing each triangle, found once and kept: both bucketing
    // passes below need them, and the floor and the settling loops are most
    // of the bucketing's cost. Each triangle writes only its own slot, so the
    // parallel pass fills exactly what a serial one would.
    let triangle_count = surface.triangle_count();
    let ranges = compute_by_index(
        workers,
        triangle_count,
        4096,
        LevelRange { first: 0, last: -1 },
        |t| {
            let tri = surface.triangles()[t];
            let z0 = surface.vertices()[tri[0] as usize].z;
            let z1 = surface.vertices()[tri[1] as usize].z;
            let z2 = surface.vertices()[tri[2] as usize].z;
            let low = std_min3(z0, z1, z2);
            let high = std_max3(z0, z1, z2);
            if low < high {
                LevelRange {
                    first: first_level_above(low),
                    last: last_level_at_or_below(high),
                }
            } else {
                LevelRange { first: 0, last: -1 }
            }
        },
    );

    // Bucket the triangles by the levels that cross them (CSR), so that tracing a
    // level touches only its own triangles. Total size = number of contour pieces.
    // Serial, and in triangle order, so every bucket is ascending - the order
    // LevelTracer::trace documents and the output order depends on.
    let mut start: Vec<usize> = vec![0; level_count + 1];
    for range in &ranges {
        for k in range.first..=range.last {
            start[(k - first_k) as usize + 1] += 1;
        }
    }
    for i in 0..level_count {
        start[i + 1] += start[i];
    }
    let mut bucket: Vec<u32> = vec![0; start[level_count]];
    let mut cursor: Vec<usize> = start[..level_count].to_vec();
    for (t, range) in ranges.iter().enumerate() {
        for k in range.first..=range.last {
            let slot = &mut cursor[(k - first_k) as usize];
            bucket[*slot] = t as u32;
            *slot += 1;
        }
    }

    // Levels are independent: tracing one reads the immutable surface and its
    // own bucket, and writes its own list. They are traced in parallel into
    // one list per level, and the lists are joined in level order, so the
    // result is identical to the serial trace whatever the thread count.
    //
    // A tracer's visited stamps must be unique per level WITHIN the array it
    // uses, not globally, so each chunk of levels has an array of its own and
    // numbers its levels from 1. Chunks are sized to give each thread a few
    // (the levels differ greatly in length), which bounds the arrays alive at
    // once by the thread count; the chunking cannot change a result, since no
    // level sees another level's stamps as its own.
    let levels_per_chunk = (level_count / (workers.concurrency() * 4)).max(1);
    let pieces: Mutex<Vec<(usize, Vec<Vec<Contour>>)>> = Mutex::new(Vec::new());
    workers.parallel_ranges(0, level_count, levels_per_chunk, |lo, hi| {
        let mut tracer = LevelTracer::new(surface);
        let mut chunk: Vec<Vec<Contour>> = Vec::with_capacity(hi - lo);
        for i in lo..hi {
            let k = first_k + i as i64;
            let major = major_every != 0 && k % major_every as i64 == 0;
            let mut lines = Vec::new();
            tracer.trace(
                level(k),
                major,
                &bucket[start[i]..start[i + 1]],
                (i - lo + 1) as u32,
                &mut lines,
            );
            chunk.push(lines);
        }
        pieces
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((lo, chunk));
    });
    let mut pieces = pieces
        .into_inner()
        .unwrap_or_else(|error| error.into_inner());
    pieces.sort_unstable_by_key(|piece| piece.0);
    for (_, chunk) in pieces {
        for lines in chunk {
            output.extend(lines);
        }
    }
    Ok(output)
}
