//! Tiled, multithreaded software rasteriser (PLAN.MD Phases 15 and 19).
//!
//! WHY A SOFTWARE RASTERISER AT ALL, beside a GPU renderer. Two reasons, both
//! of which outlive it:
//!
//! * It is testable. Rule 5 says test before optimisation, and a GPU frame
//!   cannot be asserted on in CI - there is no dependable headless GPU device
//!   on the build machines. Every pixel here is a unit test, so the clipping,
//!   the fill rule and the depth test are pinned down BEFORE any of it is
//!   reimplemented against a driver.
//! * It fixes the architecture. Everything above it talks to DrawList and
//!   Camera, never to this type, so a GPU backend is a second implementation
//!   rather than a rewrite of the application.
//!
//! It is not a placeholder that does the obvious slow thing. The structure is
//! the one a tile-based GPU uses:
//!
//! 1. TRANSFORM every vertex once, in parallel, into clip space, and classify
//!    it against the clip planes. Primitives index vertices, so a vertex
//!    shared by twenty triangles is transformed and classified once - which is
//!    the entire reason DrawList is indexed.
//! 2. SETUP+BIN: clip against the near plane and a guard band (only what
//!    crosses one - the clip codes say which), project to pixels, widen lines
//!    into quads, then record each primitive in the list of every 64x64 screen
//!    tile its bounding box touches. Done in parallel over a FIXED number of
//!    primitive chunks.
//! 3. RASTERISE: one task per tile. A tile owns its pixels exclusively, so
//!    there is no lock, no atomic and no false sharing in the inner loop, and
//!    the colour and depth a tile touches stay in L2 for the whole tile.
//!
//! DETERMINISM (Rule 7). The chunk count in stage 2 is a function of the
//! primitive count alone, never of the number of cores, so the order in which
//! a tile visits its primitives is identical on every machine. With a strict
//! depth test (reversed Z: strictly greater) that makes the frame reproducible
//! bit for bit - asserted by a test that renders the same scene with 0, 1, 3
//! and 7 worker threads and compares the buffers.
//!
//! SCRATCH is owned by the Rasterizer and reused, so a viewport redrawing at
//! 60 Hz allocates on the first frame and never again (PLAN.MD section 33).
//! That is also why this is a type and not a free function.

use crate::camera::{Camera, Projection};
use crate::disjoint::{self, TilePixels, TileTarget};
use crate::draw_list::{DrawList, VertexIndex};
use crate::framebuffer::{Framebuffer, Rgba, TileRect, alpha_of, blue_of, green_of, red_of, rgba};
use crate::scalar::{clamp, max, max3, min, min3};
use crate::simd::{self, Kernels};
use katana_core::task_pool::TaskPool;
use katana_core::{Error, ErrorCode, Result};
use katana_math::{Mat4, Vec4};
use std::fmt;

#[derive(Clone, Copy)]
pub struct RenderOptions<'a> {
    pub background: Rgba,
    /// Discards clockwise-in-screen-space triangles. Off by default: a survey
    /// surface is legitimately inspected from below, and a TIN whose triangles
    /// are all counter-clockwise in plan is clockwise the moment you look up at
    /// it from underneath.
    pub backface_cull: bool,
    /// Clears colour and depth first. Off lets a caller compose several passes.
    pub clear: bool,
    /// Off draws colour wherever the depth test passes but leaves the depth
    /// buffer as it was, so nothing drawn after is tested against this pass
    /// and, within it, what is drawn later covers what was drawn earlier. For
    /// what must never hide the model: the grid under it, and a surface's
    /// edges, which linework draped on that surface has to cross unbroken
    /// (the cad layer's renderLayers).
    pub depth_write: bool,
    /// None uses TaskPool::shared(). A single-threaded pool renders the same
    /// pixels; pass one to make a test independent of the machine.
    pub pool: Option<&'a TaskPool>,
}

impl Default for RenderOptions<'_> {
    fn default() -> Self {
        Self {
            background: rgba(30, 30, 34),
            backface_cull: false,
            clear: true,
            depth_write: true,
            pool: None,
        }
    }
}

impl fmt::Debug for RenderOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RenderOptions")
            .field("background", &self.background)
            .field("backface_cull", &self.backface_cull)
            .field("clear", &self.clear)
            .field("depth_write", &self.depth_write)
            .field("pool_concurrency", &self.pool.map(TaskPool::concurrency))
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderStats {
    pub vertices: usize,
    pub triangles_submitted: usize,
    /// Survived near-clip, cull and the viewport.
    pub triangles_rasterised: usize,
    pub lines_submitted: usize,
    pub points_submitted: usize,
    /// Pixels that passed the depth test and were written.
    pub fragments: usize,
    /// Primitive-in-tile records; > 1 per primitive when it spans.
    pub bin_entries: usize,
    pub tiles: usize,
}

/// A vertex after the model-view-projection, before the perspective divide.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClipVertex {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
    pub(crate) w: f32,
    pub(crate) color: Rgba,
}

/// A vertex after the divide: pixel coordinates, reversed depth and 1/w.
/// Worked out once per vertex in stage 1 wherever the vertex needs no
/// clipping, rather than once per triangle corner in stage 2: a TIN vertex is
/// a corner of about six triangles, and the divide is the dearest step.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScreenVertex {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
    pub(crate) inv_w: f32,
    pub(crate) color: Rgba,
}

/// A vertex projected in stage 2 has the same fields as one projected in
/// stage 1.
type ProjectedVertex = ScreenVertex;

/// A triangle ready to rasterise: pixel coordinates, NDC depth, and 1/w for
/// perspective-correct colour. Floats throughout - screen space needs nothing
/// more, and halving the working set is what keeps a tile in cache.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScreenTriangle {
    pub(crate) x: [f32; 3],
    pub(crate) y: [f32; 3],
    /// Reversed NDC depth, already in [0, 1] and screen-linear.
    pub(crate) z: [f32; 3],
    pub(crate) inv_w: [f32; 3],
    pub(crate) color: [Rgba; 3],
    /// Added to the depth (reversed: positive is nearer). The slope-scaled
    /// push of a filled triangle, negative; 0 for a widened line, whose pull
    /// is already in its vertices' depths.
    pub(crate) depth_bias: f32,
}

#[derive(Clone, Copy, Debug)]
struct ScreenPoint {
    x: f32,
    y: f32,
    z: f32,
    /// Half the square's side, in pixels.
    half: f32,
    color: Rgba,
    /// Decided once, at the centre, after the pass's filled triangles and
    /// before its lines (decide_points): drawn whole or not at all.
    visible: bool,
}

/// How a line's or a point's depth bias (pixel footprints towards the eye)
/// becomes a change of reversed depth: pixels * (scale * depth + offset).
/// Worked out once per frame from the camera (depth_pull_for).
#[derive(Clone, Copy, Debug, Default)]
struct DepthPull {
    scale: f32,
    offset: f32,
}

/// Stage 2 output, one bucket per primitive chunk so the bucket a primitive
/// lands in - and therefore the order a tile sees it in - depends only on its
/// index.
#[derive(Clone, Debug, Default)]
struct Chunk {
    triangles: Vec<ScreenTriangle>,
    /// triangles[0, line_start) are filled triangles, the rest line quads.
    line_start: usize,
    points: Vec<ScreenPoint>,
    /// tile_bins[t] holds indices into `triangles` (below POINT_TAG) and into
    /// `points` (with POINT_TAG set), in submission order. A triangle spanning
    /// several tiles is binned only in those it can reach.
    tile_bins: Vec<Vec<u32>>,
    stats: RenderStats,
}

const POINT_TAG: u32 = 0x8000_0000;

#[derive(Clone, Debug, Default)]
pub struct Rasterizer {
    depth_pull: DepthPull,

    // Stage 1 output.
    clip: Vec<ClipVertex>,
    // Parallel to clip: bit p set when the vertex is outside clip plane p (the
    // planes are described at GUARD_BAND). Worked out once per vertex, in the
    // parallel transform, rather than once per triangle corner: a TIN vertex is
    // a corner of about six triangles. Codes that OR to zero need no clipping;
    // codes that AND to non-zero are outside one plane together, so the
    // primitive cannot be seen.
    clip_codes: Vec<u8>,
    // Parallel to clip: the projected vertex, meaningful where its code is 0.
    screen: Vec<ScreenVertex>,

    // chunks[0, active_chunks) hold this frame. The vector never shrinks: the
    // 3D view draws several lists a frame through one Rasterizer (the cad
    // layer's renderLayers), each with its own chunk count, and letting it
    // shrink freed every bin and triangle buffer of the larger passes only to
    // allocate them again in the next frame.
    active_chunks: usize,
    chunks: Vec<Chunk>,
    tile_stats: Vec<RenderStats>,
}

// Chunking is deliberately a function of the workload and NOT of the core
// count: the bucket a primitive lands in decides the order a tile visits it,
// and a frame that depends on how many cores the machine has is not
// reproducible (Rule 7). 4096 primitives is large enough that the per-chunk
// bookkeeping disappears and small enough to keep every core fed on the sizes
// that matter.
const PRIMITIVES_PER_CHUNK: usize = 4096;
const MAX_CHUNKS: usize = 64;

fn chunk_count_for(primitives: usize) -> usize {
    if primitives == 0 {
        return 0;
    }
    min(primitives.div_ceil(PRIMITIVES_PER_CHUNK), MAX_CHUNKS)
}

// Interpolates a clip-space vertex's colour towards `b` by t. Colour is
// interpolated in clip space, which is where the near-plane split happens; the
// perspective correction that matters happens later, per pixel.
fn lerp_color(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let mix = |lo: u8, hi: u8| (f32::from(lo) + (f32::from(hi) - f32::from(lo)) * t + 0.5) as u8;
    (Rgba::from(mix(alpha_of(a), alpha_of(b))) << 24)
        | (Rgba::from(mix(red_of(a), red_of(b))) << 16)
        | (Rgba::from(mix(green_of(a), green_of(b))) << 8)
        | Rgba::from(mix(blue_of(a), blue_of(b)))
}

// Clipping happens in clip space, before the divide, against five planes. A
// vertex is inside where every plane_distance() below is >= 0.
//
// The NEAR plane is clip z <= w. With the REVERSED depth range camera.rs builds
// (z/w = 1 at the near plane, 0 at the far), that is exactly w >= near for a
// perspective projection and z_eye <= -near for an orthographic one, and
// set_depth_range() refuses near <= 0, so every kept vertex has w > 0 and the
// divide is safe.
// This used to clip at w > 1e-6 instead, which kept the reciprocal finite and
// nothing else: a vertex cut there projects with 1/w = 1e6 to a screen
// coordinate beyond INT_MAX, and the float-to-int conversion in the binning was
// then undefined in the C++. On x86-64 it yielded INT_MIN, so a triangle or line
// crossing the eye plane lost its VISIBLE part as well, in any view taller than
// about 213 px - the ground below a perspective eye (audit of 2026-09-23).
//
// The four GUARD-BAND planes |x| <= GUARD_BAND * w and |y| <= GUARD_BAND * w
// bound every projected coordinate to within half a viewport of the image, so
// no screen coordinate can overflow whatever the input, and the edge functions
// keep the precision of a viewport-sized triangle. The band is wider than the
// image (1) for two reasons: a point or a wide line centred just outside the
// image still reaches into it and must not be rejected, and a triangle that
// only just overhangs the image takes the pass-through path rather than being
// cut. Wider buys nothing and costs precision: an edge function's error is
// about the triangle's extent in pixels times 2^-24, a few ten-thousandths of a
// pixel at this band on a 2000 px image and over a pixel at ten thousand
// viewports.
//
// Where a triangle or line does cross a plane, the cut is computed in DOUBLE.
// The inputs are floats, so every plane distance is exact in double; in float,
// 2 + (-3.1e8) loses the 2, the parameter of the cut comes out at exactly 0.5,
// and the cut vertex lands at the image centre instead of on the band. A cut
// vertex is small even when its endpoints are enormous, so rounding it back to
// float at the end costs nothing.
//
// The far plane is not clipped: nothing numerical goes wrong beyond it, and the
// per-pixel depth test already rejects depth <= 0 (the cleared value).
const GUARD_BAND: f32 = 2.0;

// How many pixels of its own depth slope a filled triangle is pushed back by
// (the slope-scaled offset in emit_triangle). One covers a line a pixel either
// side of its centre; each more would let linework behind a wall show through
// one more pixel of the wall's silhouette.
const SLOPE_OFFSET_PIXELS: f32 = 1.0;
const CLIP_PLANES: usize = 5;

// Signed distance of a clip-space vertex from `plane`, >= 0 inside. Double for
// the reason just given; the inputs are the float clip coordinates, so the
// stage-1 classification and a stage-2 cut agree on which side a vertex is.
fn plane_distance(x: f32, y: f32, z: f32, w: f32, plane: usize) -> f64 {
    let band = f64::from(GUARD_BAND) * f64::from(w);
    match plane {
        0 => f64::from(w) - f64::from(z), // reversed Z: near is z = w
        1 => band + f64::from(x),
        2 => band - f64::from(x),
        3 => band + f64::from(y),
        _ => band - f64::from(y),
    }
}

// Bit p set when the vertex is outside plane p. A NaN distance counts as
// outside, so a vertex with a NaN in it is never passed through unclipped.
fn clip_code_of(x: f32, y: f32, z: f32, w: f32) -> u8 {
    let mut code = 0u32;
    for plane in 0..CLIP_PLANES {
        if !(plane_distance(x, y, z, w, plane) >= 0.0) {
            code |= 1 << plane;
        }
    }
    code as u8
}

fn distance(v: &ClipVertex, plane: usize) -> f64 {
    plane_distance(v.x, v.y, v.z, v.w, plane)
}

fn is_finite(v: &ClipVertex) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && v.w.is_finite()
}

// ---- stage 1: transform ---------------------------------------------------------

// Below this many vertices a range is transformed one vertex at a time: the
// AVX2 kernel takes four a step and hands the rest back, and for a handful the
// call is the whole cost. Ranges are 4096 vertices but for a list's last, so
// this only decides small lists. Measured, the kernel took 12% off the
// 'Test 4' archive frame and stayed inside the A/A spread elsewhere
// (docs/performance.md, "SIMD: the software rasteriser"). The NEON kernel also
// takes four a step, so the same minimum is four of its steps; it was kept on
// that reasoning, not measured on ARM.
const TRANSFORM_BATCH_MINIMUM: usize = 16;

// One vertex: the matrix in double, clip coordinates rounded to float, its
// clip code and - where the code is 0, so that no primitive using it will be
// cut - its projection, exactly as project() in stage 2 would make it. The
// reference the transform kernels reproduce bit for bit.
fn transform_one(
    mvp: &Mat4,
    position: katana_geometry::Point3,
    color: Rgba,
    width: f32,
    height: f32,
) -> (ClipVertex, u8, Option<ScreenVertex>) {
    let c = *mvp * Vec4::new(position.x, position.y, position.z, 1.0);
    let out = ClipVertex {
        x: c.x as f32,
        y: c.y as f32,
        z: c.z as f32,
        w: c.w as f32,
        color,
    };
    let code = clip_code_of(out.x, out.y, out.z, out.w);
    if code != 0 {
        return (out, code, None);
    }
    let inv_w = 1.0 / out.w;
    let screen = ScreenVertex {
        x: (out.x * inv_w * 0.5 + 0.5) * width,
        y: (0.5 - out.y * inv_w * 0.5) * height, // screen +y is down
        z: out.z * inv_w,
        inv_w,
        color: out.color,
    };
    (out, code, Some(screen))
}

// ---- stage 2: clip, project, widen, bin ----------------------------------------

#[derive(Clone, Copy, Debug)]
struct Viewport {
    width: f32,
    height: f32,
}

// Floor of a screen coordinate, clamped to [lo, hi] BEFORE the conversion to
// int, because converting a float outside int's range is undefined behaviour
// in the C++ (and saturates in Rust). NaN clamps to lo. After clipping every
// coordinate is inside the guard band, so this is the last line of defence
// rather than the mechanism - except for a point's half-size, which comes
// straight from the draw list.
fn pixel_floor(value: f32, lo: i32, hi: i32) -> i32 {
    let f = value.floor();
    if !(f >= lo as f32) {
        return lo;
    }
    if f >= hi as f32 {
        return hi;
    }
    f as i32
}

// The pixels whose CENTRES lie in [c - h, c + h): exactly size x size for a
// whole size wherever the point falls. Pixel i's centre is i + 0.5, so i runs
// from ceil(c - h - 0.5) to ceil(c + h - 0.5) - 1; ceil(v) is -floor(-v), which
// keeps the conversion clamped. It drew floor(c - h)..floor(c + h), one pixel
// too many on each axis (audit REN-10).
fn ceil_in(value: f32, lo: i32, hi: i32) -> i32 {
    -pixel_floor(-value, -hi, -lo)
}

// Clip space is where interpolation is linear, so a cut vertex is a lerp.
fn lerp_clip(a: &ClipVertex, b: &ClipVertex, t: f64) -> ClipVertex {
    let mix = |from: f32, to: f32| {
        let lo = f64::from(from);
        (lo + (f64::from(to) - lo) * t) as f32
    };
    ClipVertex {
        x: mix(a.x, b.x),
        y: mix(a.y, b.y),
        z: mix(a.z, b.z),
        w: mix(a.w, b.w),
        color: lerp_color(a.color, b.color, t as f32),
    }
}

fn project(viewport: &Viewport, v: &ClipVertex) -> ProjectedVertex {
    // Only ever given a vertex that survived clipping, so w >= near > 0.
    let inv_w = 1.0 / v.w;
    ProjectedVertex {
        x: (v.x * inv_w * 0.5 + 0.5) * viewport.width,
        y: (0.5 - v.y * inv_w * 0.5) * viewport.height, // screen +y is down
        z: v.z * inv_w,
        inv_w,
        color: v.color,
    }
}

// depth -> depth pulled `pixels` footprints towards the eye, clamped to the
// near plane so a biased line touching it still draws.
fn pull_towards_eye(pull: DepthPull, depth: f32, pixels: f32) -> f32 {
    if pixels == 0.0 {
        return depth;
    }
    let pulled = depth + pixels * (pull.scale * depth + pull.offset);
    min(pulled, 1.0)
}

// One projected triangle, all of it inside the five planes.
fn emit_triangle(
    chunk: &mut Chunk,
    cull: bool,
    p0: &ProjectedVertex,
    p1: &ProjectedVertex,
    p2: &ProjectedVertex,
    depth_bias: f32,
) {
    let area = (p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y);
    if !(area.abs() > 0.0) {
        return; // zero area or NaN: nothing to fill
    }
    if cull && area >= 0.0 {
        // Screen +y points down, so a counter-clockwise world triangle has a
        // NEGATIVE screen-space area. Cull the other sign.
        return;
    }
    // SLOPE-SCALED OFFSET (what a GPU calls polygon offset): a filled triangle
    // is pushed AWAY by its own depth change over SLOPE_OFFSET_PIXELS. Linework
    // lying on a surface is drawn a pixel or two wide, so its pixels sample the
    // surface up to a pixel off the line; seen at a grazing angle the surface
    // there is nearer than the line by a whole pixel's worth of depth slope,
    // and no constant bias can cover that without also showing lines through
    // buildings. Pushing the surface by its OWN slope covers exactly that and
    // nothing more. Screen-space constant per triangle, so the fill loop only
    // adds it.
    let bias = if depth_bias == 0.0 {
        let dzdx = ((p1.z - p0.z) * (p2.y - p0.y) - (p2.z - p0.z) * (p1.y - p0.y)) / area;
        let dzdy = ((p1.x - p0.x) * (p2.z - p0.z) - (p2.x - p0.x) * (p1.z - p0.z)) / area;
        -SLOPE_OFFSET_PIXELS * max(dzdx.abs(), dzdy.abs())
    } else {
        depth_bias
    };
    chunk.triangles.push(screen_triangle(p0, p1, p2, bias));
    chunk.stats.triangles_rasterised += 1;
}

fn screen_triangle(
    p0: &ProjectedVertex,
    p1: &ProjectedVertex,
    p2: &ProjectedVertex,
    depth_bias: f32,
) -> ScreenTriangle {
    ScreenTriangle {
        x: [p0.x, p1.x, p2.x],
        y: [p0.y, p1.y, p2.y],
        z: [p0.z, p1.z, p2.z],
        inv_w: [p0.inv_w, p1.inv_w, p2.inv_w],
        color: [p0.color, p1.color, p2.color],
        depth_bias,
    }
}

// Fans a convex polygon whose every vertex is inside all five planes into
// screen triangles. Each vertex is projected once; the fan used to project its
// hub again for every triangle, to the same bits.
fn emit_polygon(
    chunk: &mut Chunk,
    cull: bool,
    viewport: &Viewport,
    polygon: &[ClipVertex],
    depth_bias: f32,
) {
    let mut projected = [ProjectedVertex::default(); 3 + CLIP_PLANES];
    for (out, vertex) in projected.iter_mut().zip(polygon) {
        *out = project(viewport, vertex);
    }
    for i in 1..polygon.len() - 1 {
        emit_triangle(
            chunk,
            cull,
            &projected[0],
            &projected[i],
            &projected[i + 1],
            depth_bias,
        );
    }
}

// A line quad's triangle. Never culled: a widened line has no meaningful
// facing.
fn push_quad_triangle(
    chunk: &mut Chunk,
    v0: &ProjectedVertex,
    v1: &ProjectedVertex,
    v2: &ProjectedVertex,
) {
    let area = (v1.x - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (v1.y - v0.y);
    if !(area.abs() > 0.0) {
        return;
    }
    // Already pulled per vertex; a line has no slope of its own across its
    // width to offset.
    chunk.triangles.push(screen_triangle(v0, v1, v2, 0.0));
    chunk.stats.triangles_rasterised += 1;
}

// Depth and 1/w come from the endpoint the corner belongs to, so a widened
// line keeps the depth of the line itself.
fn corner(p: &ProjectedVertex, sx: f32, sy: f32) -> ProjectedVertex {
    ProjectedVertex {
        x: p.x + sx,
        y: p.y + sy,
        ..*p
    }
}

/// What stage 2 reads, shared by every chunk's task.
struct StageTwo<'a> {
    list: &'a DrawList,
    clip: &'a [ClipVertex],
    codes: &'a [u8],
    screen: &'a [ScreenVertex],
    viewport: Viewport,
    cull: bool,
    pull: DepthPull,
    per_chunk: usize,
    primitive_count: usize,
}

impl StageTwo<'_> {
    // Stage 1 projected every vertex that needs no clipping, to the bits
    // project() gives.
    fn projected_at(&self, index: VertexIndex) -> ProjectedVertex {
        self.screen[index as usize]
    }

    // A triangle that crosses at least one plane: Sutherland-Hodgman, one plane
    // at a time, into a convex polygon of up to eight vertices (each plane adds
    // at most one), then fanned.
    fn emit_clipped_triangle(&self, chunk: &mut Chunk, v: [ClipVertex; 3], depth_bias: f32) {
        // A non-finite coordinate has no position to clip, and a cut
        // interpolated from one is NaN: draw nothing rather than part.
        if !is_finite(&v[0]) || !is_finite(&v[1]) || !is_finite(&v[2]) {
            return;
        }
        let mut polygon = [ClipVertex::default(); 3 + CLIP_PLANES];
        polygon[..3].copy_from_slice(&v);
        let mut scratch = [ClipVertex::default(); 3 + CLIP_PLANES];
        let mut count = 3;
        // NEAR FIRST: once every vertex has z >= 0 every w is positive, so each
        // later cut interpolates between vertices the divide survives.
        for plane in 0..CLIP_PLANES {
            if count < 3 {
                break;
            }
            let mut kept = 0;
            for i in 0..count {
                let current = polygon[i];
                let next = polygon[(i + 1) % count];
                let dc = distance(&current, plane);
                let dn = distance(&next, plane);
                let current_in = dc >= 0.0;
                let next_in = dn >= 0.0;
                if current_in {
                    scratch[kept] = current;
                    kept += 1;
                }
                if current_in != next_in {
                    // dc and dn have opposite signs, so the denominator is never
                    // zero and t is in [0, 1].
                    let mut cut_vertex = lerp_clip(&current, &next, dc / (dc - dn));
                    if plane == 0 {
                        cut_vertex.z = cut_vertex.w; // on the near plane exactly: depth 1
                    }
                    scratch[kept] = cut_vertex;
                    kept += 1;
                }
            }
            polygon[..kept].copy_from_slice(&scratch[..kept]);
            count = kept;
        }
        if count >= 3 {
            emit_polygon(
                chunk,
                self.cull,
                &self.viewport,
                &polygon[..count],
                depth_bias,
            );
        }
    }

    // The primitives of chunk `chunk_index`: a contiguous slice of the combined
    // stream, triangles first, then lines, then points.
    fn build(&self, chunk_index: usize, chunk: &mut Chunk) {
        let first = chunk_index * self.per_chunk;
        let last = min(first + self.per_chunk, self.primitive_count);
        if first >= last {
            return;
        }
        let list = self.list;
        let vertex_count = self.clip.len();
        for index in first..last {
            if index < list.triangles.len() {
                let t = list.triangles[index];
                if t.a as usize >= vertex_count
                    || t.b as usize >= vertex_count
                    || t.c as usize >= vertex_count
                {
                    continue;
                }
                chunk.stats.triangles_submitted += 1;
                let ca = self.codes[t.a as usize];
                let cb = self.codes[t.b as usize];
                let cc = self.codes[t.c as usize];
                if (ca & cb & cc) != 0 {
                    continue; // all three outside one plane: nothing of it can show
                }
                if (ca | cb | cc) == 0 {
                    // The common case: nothing to cut.
                    emit_triangle(
                        chunk,
                        self.cull,
                        &self.projected_at(t.a),
                        &self.projected_at(t.b),
                        &self.projected_at(t.c),
                        0.0,
                    );
                } else {
                    self.emit_clipped_triangle(
                        chunk,
                        [
                            self.clip[t.a as usize],
                            self.clip[t.b as usize],
                            self.clip[t.c as usize],
                        ],
                        0.0,
                    );
                }
                // Triangles come first in the stream, so the line quads that
                // follow start here (sweep_tile sweeps them apart).
                chunk.line_start = chunk.triangles.len();
                continue;
            }
            let line_index = index - list.triangles.len();
            if line_index < list.lines.len() {
                let line = list.lines[line_index];
                if line.a as usize >= vertex_count || line.b as usize >= vertex_count {
                    continue;
                }
                chunk.stats.lines_submitted += 1;

                // Clip the segment against the same five planes (Liang-Barsky:
                // the kept part is the parameter interval [t0, t1] every plane
                // agrees on), then widen in SCREEN space so a line keeps its
                // pixel thickness at any depth, and reuse the triangle path:
                // one tested rasteriser, correct depth, correct clipping.
                let ca = self.codes[line.a as usize];
                let cb = self.codes[line.b as usize];
                if (ca & cb) != 0 {
                    continue; // both ends outside one plane
                }
                let (mut p0, mut p1) = if (ca | cb) == 0 {
                    (self.projected_at(line.a), self.projected_at(line.b))
                } else {
                    let a0 = self.clip[line.a as usize];
                    let b0 = self.clip[line.b as usize];
                    if !is_finite(&a0) || !is_finite(&b0) {
                        continue;
                    }
                    let mut t0 = 0.0;
                    let mut t1 = 1.0;
                    let mut visible = true;
                    let mut near_cuts_a = false;
                    let mut near_cuts_b = false;
                    for plane in 0..CLIP_PLANES {
                        if !visible {
                            break;
                        }
                        let da = distance(&a0, plane);
                        let db = distance(&b0, plane);
                        if da < 0.0 && db < 0.0 {
                            visible = false;
                        } else if da < 0.0 {
                            let t = da / (da - db); // entering
                            if t > t0 {
                                t0 = t;
                                near_cuts_a = plane == 0;
                            }
                        } else if db < 0.0 {
                            let t = da / (da - db); // leaving
                            if t < t1 {
                                t1 = t;
                                near_cuts_b = plane == 0;
                            }
                        }
                    }
                    if !visible || !(t0 <= t1) {
                        continue;
                    }
                    let mut a = lerp_clip(&a0, &b0, t0);
                    let mut b = lerp_clip(&a0, &b0, t1);
                    // As for a triangle: an end cut by the near plane is ON it,
                    // depth exactly 1, not a rounding error either side.
                    if near_cuts_a {
                        a.z = a.w;
                    }
                    if near_cuts_b {
                        b.z = b.w;
                    }
                    (project(&self.viewport, &a), project(&self.viewport, &b))
                };
                // The line's depth bias is a VIEW-SPACE distance, depth_bias
                // pixel footprints towards the eye (draw_list.rs). Under the
                // reversed projections that is an affine map of the depth
                // itself (pull_towards_eye), so it is applied to the two ends
                // and interpolates exactly along the line.
                p0.z = pull_towards_eye(self.pull, p0.z, line.depth_bias);
                p1.z = pull_towards_eye(self.pull, p1.z, line.depth_bias);
                let mut dx = p1.x - p0.x;
                let mut dy = p1.y - p0.y;
                let length = (dx * dx + dy * dy).sqrt();
                let half = max(line.width, 1.0) * 0.5;
                if !(length > 0.0) {
                    // Seen exactly end-on there is no direction to widen along;
                    // any one will do, because the square cap below turns it
                    // into a square centred on the point.
                    dx = 1.0;
                    dy = 0.0;
                } else {
                    dx /= length;
                    dy /= length;
                }
                let nx = -dy * half;
                let ny = dx * half;
                // SQUARE CAPS, extending the quad by half a width past each
                // end. Not cosmetic: without them a segment shorter than a
                // pixel encloses no pixel CENTRE and draws nothing at all, so a
                // densely surveyed string - where every segment is sub-pixel
                // once you zoom out - disappears entirely instead of drawing as
                // a line. With them the shortest possible segment still covers
                // its own width, and a vertical pipe viewed in plan reads as a
                // dot rather than vanishing.
                let ex = dx * half;
                let ey = dy * half;

                let q0 = corner(&p0, nx - ex, ny - ey);
                let q1 = corner(&p0, -nx - ex, -ny - ey);
                let q2 = corner(&p1, -nx + ex, -ny + ey);
                let q3 = corner(&p1, nx + ex, ny + ey);
                push_quad_triangle(chunk, &q0, &q1, &q2);
                push_quad_triangle(chunk, &q0, &q2, &q3);
                continue;
            }

            let point_index = line_index - list.lines.len();
            if point_index >= list.points.len() {
                continue;
            }
            let point = list.points[point_index];
            if point.a as usize >= vertex_count {
                continue;
            }
            chunk.stats.points_submitted += 1;
            if self.codes[point.a as usize] != 0 {
                continue;
            }
            let p = self.projected_at(point.a);
            chunk.points.push(ScreenPoint {
                x: p.x,
                y: p.y,
                z: pull_towards_eye(self.pull, p.z, point.depth_bias),
                half: max(point.size, 1.0) * 0.5,
                color: p.color,
                visible: false,
            });
        }
    }
}

// ---- conservative coverage --------------------------------------------------------
//
// The fill decides coverage per pixel in float (shade_pixel): w0 and w1 from
// edge functions times 1/area, w2 = 1 - w0 - w1, and a pixel is in when none is
// negative. Whatever skips pixels ahead of that - a tile the triangle cannot
// reach, the part of a row outside it, the edge tests where every pixel is
// inside - must skip only pixels the float test would have decided the same
// way, or the frame changes. So the decisions below are made on the EXACT edge
// functions, in double, with a margin M that bounds how far the float ones can
// stray from them.
//
// With R the largest distance, on either axis, between a vertex and a pixel
// centre of the rectangle, every factor in an edge function is at most R in
// size, each edge function at most 2 R^2 and the doubled area at most 8 R^2.
// Rounding each float operation once (u = 2^-24) moves w0 * area and
// w1 * area by at most about 10 u R^2 each, the float area by 24 u R^2, and
// w2 * area - which is the third edge function plus those errors, because the
// three edge functions add up to the area exactly - by about 55 u R^2 in all.
// M = 256 u (R^2 + 1) is over four times that, and the double arithmetic
// here is exact to parts in 2^53, far inside it. Where s E_k(p) < -M the float
// test rejects p, where every s E_k(p) > M it accepts p, and between the two
// the pixel goes to the float test as before. s is the sign of the float area
// the fill divides by, which on a sliver need not be that of the exact one.

// Edge k runs between vertices EDGE_FROM[k] and EDGE_TO[k]: E_0 is the
// numerator of the fill's w0, E_1 of its w1, and E_2 the exact third.
const EDGE_FROM: [usize; 3] = [1, 2, 0];
const EDGE_TO: [usize; 3] = [2, 0, 1];

// 256 u with u = 2^-24: 2^-16, exactly.
const MARGIN_SCALE: f64 = 1.0 / 65536.0;

#[derive(Clone, Copy, Debug)]
struct EdgeSetup {
    x: [f64; 3],
    y: [f64; 3],
    sign: f64,
    margin: f64,
    usable: bool, // false: a coordinate is not finite; decide nothing
}

impl EdgeSetup {
    // s E_k at the pixel centre (px, py).
    fn at(&self, k: usize, px: f64, py: f64) -> f64 {
        let i = EDGE_FROM[k];
        let j = EDGE_TO[k];
        self.sign * ((self.x[i] - px) * (self.y[j] - py) - (self.x[j] - px) * (self.y[i] - py))
    }
}

// The setup for a triangle with float corners (tx, ty) and float doubled area
// `area` over the pixels [min_x, max_x] x [min_y, max_y].
fn edge_setup(
    tx: [f32; 3],
    ty: [f32; 3],
    area: f32,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
) -> EdgeSetup {
    let x = tx.map(f64::from);
    let y = ty.map(f64::from);
    let px_lo = f64::from(min_x) + 0.5;
    let px_hi = f64::from(max_x) + 0.5;
    let py_lo = f64::from(min_y) + 0.5;
    let py_hi = f64::from(max_y) + 0.5;
    let reach = max(
        max(max(max3(x) - px_lo, px_hi - min3(x)), max3(y) - py_lo),
        py_hi - min3(y),
    );
    let margin = MARGIN_SCALE * (reach * reach + 1.0); // 256 u (R^2 + 1), u = 2^-24
    EdgeSetup {
        x,
        y,
        sign: if area > 0.0 { 1.0 } else { -1.0 },
        margin,
        usable: margin.is_finite(),
    }
}

// True when no pixel centre of the rectangle can pass the float test: one edge
// function is below -M at all four corners, and so, being affine, all over it.
fn cannot_reach(e: &EdgeSetup, min_x: i32, max_x: i32, min_y: i32, max_y: i32) -> bool {
    if !e.usable {
        return false;
    }
    let px_lo = f64::from(min_x) + 0.5;
    let px_hi = f64::from(max_x) + 0.5;
    let py_lo = f64::from(min_y) + 0.5;
    let py_hi = f64::from(max_y) + 0.5;
    for k in 0..3 {
        if e.at(k, px_lo, py_lo) < -e.margin
            && e.at(k, px_hi, py_lo) < -e.margin
            && e.at(k, px_lo, py_hi) < -e.margin
            && e.at(k, px_hi, py_hi) < -e.margin
        {
            return true;
        }
    }
    false
}

// True when every pixel centre of the rectangle passes the float test: every
// edge function is above M at all four corners. Used only by the kernel paths.
fn covers_all(e: &EdgeSetup, min_x: i32, max_x: i32, min_y: i32, max_y: i32) -> bool {
    if !e.usable {
        return false;
    }
    let px_lo = f64::from(min_x) + 0.5;
    let px_hi = f64::from(max_x) + 0.5;
    let py_lo = f64::from(min_y) + 0.5;
    let py_hi = f64::from(max_y) + 0.5;
    for k in 0..3 {
        if !(e.at(k, px_lo, py_lo) > e.margin
            && e.at(k, px_hi, py_lo) > e.margin
            && e.at(k, px_lo, py_hi) > e.margin
            && e.at(k, px_hi, py_hi) > e.margin)
        {
            return false;
        }
    }
    true
}

// For each row min_y + r, the pixels spans[2 r] .. spans[2 r + 1] (first > last
// when none) that can pass the float test - the rest of [min_x, max_x] cannot.
// Along a row E_k is affine in the pixel centre, E_k = K - px g, so each edge
// bounds px from one side (or rules the whole row in or out when g is 0).
// Returns false when no row has a pixel.
fn row_spans(
    e: &EdgeSetup,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    spans: &mut [i32],
) -> bool {
    let mut any = false;
    for y in min_y..=max_y {
        let r = (y - min_y) as usize;
        let mut lo = f64::from(min_x);
        let mut hi = f64::from(max_x);
        if e.usable {
            let py = f64::from(y) + 0.5;
            for k in 0..3 {
                if !(lo <= hi) {
                    break;
                }
                let i = EDGE_FROM[k];
                let j = EDGE_TO[k];
                // s E_k >= -M  <=>  s g px <= s K + M.
                let bound = e.sign * (e.x[i] * (e.y[j] - py) - e.x[j] * (e.y[i] - py)) + e.margin;
                let slope = e.sign * (e.y[j] - e.y[i]);
                if slope > 0.0 {
                    hi = min(hi, (bound / slope - 0.5).floor());
                } else if slope < 0.0 {
                    lo = max(lo, (bound / slope - 0.5).ceil());
                } else if bound < 0.0 {
                    hi = lo - 1.0;
                }
            }
        }
        if lo <= hi {
            spans[2 * r] = lo as i32;
            spans[2 * r + 1] = hi as i32;
            any = true;
        } else {
            spans[2 * r] = 0;
            spans[2 * r + 1] = -1;
        }
    }
    any
}

// The bins of one chunk: each triangle and point recorded in every tile it
// can reach.
fn bin_chunk(chunk: &mut Chunk, target: &Framebuffer, tiles: usize) {
    // Each chunk owns its bins, so resizing and clearing them here keeps that
    // cost parallel instead of serial in the caller.
    chunk.tile_bins.resize_with(tiles, Vec::new);
    for bin in &mut chunk.tile_bins {
        bin.clear();
    }
    let width = target.width();
    let height = target.height();
    let tiles_across = target.tiles_across() as usize;
    let tile = Framebuffer::TILE_SIZE;

    for (i, t) in chunk.triangles.iter().enumerate() {
        let min_x = min3(t.x);
        let max_x = max3(t.x);
        let min_y = min3(t.y);
        let max_y = max3(t.y);
        if !(max_x >= 0.0) || !(max_y >= 0.0) || min_x >= width as f32 || min_y >= height as f32 {
            continue; // entirely off screen (the !(>=) form also rejects NaN)
        }
        let x0 = pixel_floor(min_x, 0, width - 1) / tile;
        let y0 = pixel_floor(min_y, 0, height - 1) / tile;
        let x1 = pixel_floor(max_x, 0, width - 1) / tile;
        let y1 = pixel_floor(max_y, 0, height - 1) / tile;
        if x0 == x1 && y0 == y1 {
            // Recorded here directly rather than by the tile walk below: on a
            // dense TIN nearly every triangle is in one tile, and flooring its
            // box again for each tile cost the framed 1.05M-triangle grid 10%.
            chunk.tile_bins[y0 as usize * tiles_across + x0 as usize].push(i as u32);
            chunk.stats.bin_entries += 1;
            continue;
        }
        // Spanning tiles: a long line or a large or thin triangle, whose box
        // holds tiles it never reaches - most of them, for a line across the
        // view. Each tile is tested over the pixels the fill would visit there
        // (fill_triangle), and left out only when the fill would find nothing
        // in it.
        let area = (t.x[1] - t.x[0]) * (t.y[2] - t.y[0]) - (t.x[2] - t.x[0]) * (t.y[1] - t.y[0]);
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let tile_index = ty as usize * tiles_across + tx as usize;
                let rect = target.tile(tile_index);
                let px0 = pixel_floor(min_x, rect.x0, rect.x1);
                let px1 = pixel_floor(max_x, rect.x0 - 1, rect.x1 - 1);
                let py0 = pixel_floor(min_y, rect.y0, rect.y1);
                let py1 = pixel_floor(max_y, rect.y0 - 1, rect.y1 - 1);
                if px0 <= px1
                    && py0 <= py1
                    && cannot_reach(
                        &edge_setup(t.x, t.y, area, px0, px1, py0, py1),
                        px0,
                        px1,
                        py0,
                        py1,
                    )
                {
                    continue;
                }
                chunk.tile_bins[tile_index].push(i as u32);
                chunk.stats.bin_entries += 1;
            }
        }
    }
    for (i, p) in chunk.points.iter().enumerate() {
        let min_x = p.x - p.half;
        let min_y = p.y - p.half;
        let max_x = p.x + p.half;
        let max_y = p.y + p.half;
        if !(max_x >= 0.0) || !(max_y >= 0.0) || min_x >= width as f32 || min_y >= height as f32 {
            continue; // entirely off screen (the !(>=) form also rejects NaN)
        }
        let x0 = pixel_floor(min_x, 0, width - 1) / tile;
        let y0 = pixel_floor(min_y, 0, height - 1) / tile;
        let x1 = pixel_floor(max_x, 0, width - 1) / tile;
        let y1 = pixel_floor(max_y, 0, height - 1) / tile;
        let tag = (i as u32) | POINT_TAG;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                chunk.tile_bins[ty as usize * tiles_across + tx as usize].push(tag);
                chunk.stats.bin_entries += 1;
            }
        }
    }
}

// ---- stage 3: rasterise ---------------------------------------------------------

// Where a triangle's box in a tile holds fewer pixels than this, the fill
// visits every pixel of the box, as it always did. From here it first bounds
// each row to the pixels that can be inside (row_spans) and, on AVX2, shades
// them eight at a time (four on NEON). A dense TIN framed whole is fractions
// of a pixel a triangle, where the double setup is pure cost: with no cutoff
// the framed 1.05M-triangle grid took 23% longer, and at 16 the 131k one, whose
// triangles are a few pixels, 4% longer. At 64 neither moved outside the A/A
// spread, and a surface seen from inside it, or a line across the view - both
// hundreds of pixels a box - kept nearly all of the gain (docs/performance.md,
// "SIMD: the software rasteriser"). The cutoff also applies at the scalar level
// and on NEON, where it was not measured: the setup it avoids is the same
// double arithmetic whichever level shades.
const BOUNDED_FILL_MINIMUM_PIXELS: i64 = 64;

const TILE_PIXELS: usize = (Framebuffer::TILE_SIZE * Framebuffer::TILE_SIZE) as usize;

// One pixel of the fill, (x + 0.5, py), of triangle t, in the tile row `row`
// and `depth_row` whose first element is pixel `origin`: true when it was
// written. The reference every fast path reproduces bit for bit - the AVX2 and
// NEON kernels lane by lane (simd/avx2.rs, simd/neon.rs).
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ reference's arguments, and the row's origin that a slice of the tile adds."
)]
#[inline(always)]
fn shade_pixel(
    t: &ScreenTriangle,
    inv_area: f32,
    x: i32,
    py: f32,
    row: &mut [Rgba],
    depth_row: &mut [f32],
    origin: i32,
    depth_write: bool,
) -> bool {
    let px = x as f32 + 0.5;

    // No top-left rule: a pixel exactly on a shared edge is covered by both
    // triangles rather than by exactly one. With an opaque, strictly-greater
    // depth test that costs a redundant write and changes no pixel, so the
    // extra branches are not paid for. It would have to be added before any
    // blended pass.
    //
    // Barycentrics from edge functions. Normalising by the signed area makes
    // the sign test independent of winding, so a triangle is filled whichever
    // way round it is - the caller opted in or out of culling long before this
    // point.
    let w0 = ((t.x[1] - px) * (t.y[2] - py) - (t.x[2] - px) * (t.y[1] - py)) * inv_area;
    let w1 = ((t.x[2] - px) * (t.y[0] - py) - (t.x[0] - px) * (t.y[2] - py)) * inv_area;
    let w2 = 1.0 - w0 - w1;
    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
        return false;
    }

    // Reversed Z: larger is nearer, the buffer is cleared to 0 (the far plane)
    // and the test is strictly greater, so the first of two equal depths still
    // wins (Rule 7). Clamped at the near plane, which a pull towards the eye
    // can overshoot; NaN fails.
    let index = (x - origin) as usize;
    let depth = min(w0 * t.z[0] + w1 * t.z[1] + w2 * t.z[2] + t.depth_bias, 1.0);
    if !(depth > depth_row[index]) {
        return false;
    }

    // Perspective-correct colour: interpolate c/w and 1/w and divide. Under an
    // orthographic projection every inv_w is equal and this reduces to the
    // linear case.
    let inv_w = w0 * t.inv_w[0] + w1 * t.inv_w[1] + w2 * t.inv_w[2];
    let color = if inv_w > 0.0 {
        let s = 1.0 / inv_w;
        let channel = |shift: u32| -> Rgba {
            let c = (w0 * ((t.color[0] >> shift) & 0xFF) as f32 * t.inv_w[0]
                + w1 * ((t.color[1] >> shift) & 0xFF) as f32 * t.inv_w[1]
                + w2 * ((t.color[2] >> shift) & 0xFF) as f32 * t.inv_w[2])
                * s;
            // In range after the clamp, so the conversion truncates as the
            // C++'s does; a NaN gives 0 in both.
            Rgba::from((clamp(c, 0.0, 255.0) + 0.5) as u8)
        };
        (channel(24) << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
    } else {
        t.color[0]
    };

    if depth_write {
        depth_row[index] = depth;
    }
    row[index] = color;
    true
}

// A box of BOUNDED_FILL_MINIMUM_PIXELS or more: each row bounded to the pixels
// that can be inside first (row_spans, which every pixel shade_pixel would
// accept passes), then those shaded - eight at a time with the AVX2 kernel,
// four with the NEON one, which skip the edge tests where the whole box is
// inside, or one at a time. Out of line so that none of this weighs on the
// loop for small boxes, which is what a dense TIN is made of. Returns the
// pixels written.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++'s arguments, with the tile's pixels in place of the buffers and stride."
)]
#[inline(never)]
fn fill_bounded(
    t: &ScreenTriangle,
    area: f32,
    inv_area: f32,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    depth_write: bool,
    pixels: &mut TilePixels<'_>,
    kernel: Option<Kernels>,
) -> usize {
    let mut spans = [0i32; 2 * Framebuffer::TILE_SIZE as usize];
    let edges = edge_setup(t.x, t.y, area, min_x, max_x, min_y, max_y);
    if !row_spans(&edges, min_x, max_x, min_y, max_y, &mut spans) {
        return 0;
    }
    if let Some(kernels) = kernel {
        return simd::shade_rows(
            kernels,
            t,
            inv_area,
            &spans,
            min_y,
            max_y - min_y + 1,
            covers_all(&edges, min_x, max_x, min_y, max_y),
            depth_write,
            pixels,
        );
    }
    let origin = pixels.rect().x0;
    let mut written = 0;
    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        let r = (y - min_y) as usize;
        let (row, depth_row) = pixels.row(y);
        for x in spans[2 * r]..=spans[2 * r + 1] {
            if shade_pixel(t, inv_area, x, py, row, depth_row, origin, depth_write) {
                written += 1;
            }
        }
    }
    written
}

// One triangle of a tile's bin over the part of the tile its box covers.
fn fill_triangle(
    t: &ScreenTriangle,
    rect: TileRect,
    pixels: &mut TilePixels<'_>,
    depth_write: bool,
    kernel: Option<Kernels>,
    stats: &mut RenderStats,
) {
    let min_x = pixel_floor(min3(t.x), rect.x0, rect.x1);
    let max_x = pixel_floor(max3(t.x), rect.x0 - 1, rect.x1 - 1);
    let min_y = pixel_floor(min3(t.y), rect.y0, rect.y1);
    let max_y = pixel_floor(max3(t.y), rect.y0 - 1, rect.y1 - 1);
    if min_x > max_x || min_y > max_y {
        return;
    }

    let area = (t.x[1] - t.x[0]) * (t.y[2] - t.y[0]) - (t.x[2] - t.x[0]) * (t.y[1] - t.y[0]);
    if !(area.abs() > 0.0) {
        return;
    }
    let inv_area = 1.0 / area;

    // A box of a few pixels is filled whole, as it always was. A larger one is
    // bounded row by row first (fill_bounded).
    let box_pixels = i64::from(max_x - min_x + 1) * i64::from(max_y - min_y + 1);
    if box_pixels >= BOUNDED_FILL_MINIMUM_PIXELS {
        stats.fragments += fill_bounded(
            t,
            area,
            inv_area,
            min_x,
            max_x,
            min_y,
            max_y,
            depth_write,
            pixels,
            kernel,
        );
        return;
    }
    let origin = rect.x0;
    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        let (row, depth_row) = pixels.row(y);
        for x in min_x..=max_x {
            if shade_pixel(t, inv_area, x, py, row, depth_row, origin, depth_write) {
                stats.fragments += 1;
            }
        }
    }
}

// A visible point's square, in one tile: there the nearer point wins over the
// pass's other points (`drawn` marks the pixels a point of this pass has
// drawn); anywhere else its centre has decided (decide_points).
fn draw_point(
    p: &ScreenPoint,
    rect: TileRect,
    pixels: &mut TilePixels<'_>,
    drawn: &mut [u8; TILE_PIXELS],
    depth_write: bool,
    stats: &mut RenderStats,
) {
    let x0 = ceil_in(p.x - p.half - 0.5, rect.x0, rect.x1);
    let x1 = ceil_in(p.x + p.half - 0.5, rect.x0, rect.x1) - 1;
    let y0 = ceil_in(p.y - p.half - 0.5, rect.y0, rect.y1);
    let y1 = ceil_in(p.y + p.half - 0.5, rect.y0, rect.y1) - 1;
    let tile = Framebuffer::TILE_SIZE as usize;
    for y in y0..=y1 {
        let (row, depth_row) = pixels.row(y);
        let drawn_row = &mut drawn[(y - rect.y0) as usize * tile..];
        for x in x0..=x1 {
            let k = (x - rect.x0) as usize;
            // Without depth writes the pass is painted in order, as its
            // triangles are.
            if depth_write && drawn_row[k] != 0 && !(p.z > depth_row[k]) {
                continue;
            }
            drawn_row[k] = 1;
            if depth_write {
                depth_row[k] = p.z;
            }
            row[k] = p.color;
            stats.fragments += 1;
        }
    }
}

// What one sweep over the tiles draws. A list without points is drawn in one.
// With points, its filled triangles are drawn first, then every point is
// decided (decide_points, which says why there), then the lines and the
// points. Primitives keep their stream order either way: filled triangles come
// before line quads, and line quads before points, in every tile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Sweep {
    Everything,
    Filled,
    LinesThenPoints,
}

// One tile of one sweep.
fn sweep_tile(
    what: Sweep,
    chunks: &[Chunk],
    tile_index: usize,
    pixels: &mut TilePixels<'_>,
    stats: &mut RenderStats,
    depth_write: bool,
    kernel: Option<Kernels>,
) {
    let rect = pixels.rect();
    if rect.empty() {
        return;
    }
    // Which pixels of this tile a point of this pass has drawn; made, cleared,
    // when the tile meets its first point.
    let mut drawn: Option<[u8; TILE_PIXELS]> = None;

    // Chunks in index order, primitives in index order within a chunk: the
    // visit order is a pure function of the draw list, so ties at equal depth
    // always resolve the same way (Rule 7).
    for chunk in chunks {
        let first = if what == Sweep::LinesThenPoints {
            chunk.line_start
        } else {
            0
        };
        let last = if what == Sweep::Filled {
            chunk.line_start
        } else {
            chunk.triangles.len()
        };
        let points = what == Sweep::LinesThenPoints && !chunk.points.is_empty();
        if first >= last && !points {
            continue; // nothing of this chunk in this sweep
        }
        for &tag in &chunk.tile_bins[tile_index] {
            if (tag & POINT_TAG) != 0 {
                if !points {
                    continue;
                }
                let p = &chunk.points[(tag & !POINT_TAG) as usize];
                if !p.visible {
                    continue;
                }
                #[allow(
                    clippy::unnecessary_lazy_evaluations,
                    reason = "The tile's mask is made once, for its first point; built eagerly it would be zeroed for every point."
                )]
                let drawn = drawn.get_or_insert_with(|| [0; TILE_PIXELS]);
                draw_point(p, rect, pixels, drawn, depth_write, stats);
                continue;
            }
            let tag = tag as usize;
            if !(first..last).contains(&tag) {
                continue;
            }
            fill_triangle(
                &chunk.triangles[tag],
                rect,
                pixels,
                depth_write,
                kernel,
                stats,
            );
        }
    }
}

// A dispatch wakes every worker and waits for each to report back, which costs
// more than a few thousand of these reads: an extra two dispatches made a
// 1600 x 1000 frame of a survey with 400 points (BM_SceneFrame) 5-9% slower on
// the median. Up to POINTS_INLINE points they are decided on this thread; each
// decision reads only the depth buffer, so the result is the same either way.
const POINTS_INLINE: usize = 4096;

impl Rasterizer {
    /// Draws `list` through `camera` into `target`.
    ///
    /// Fails with InvalidArgument when the target is empty or its size does
    /// not match the camera's viewport - a mismatch silently renders the wrong
    /// framing, which is exactly the kind of quiet wrongness section 36
    /// forbids.
    pub fn render(
        &mut self,
        list: &DrawList,
        camera: &Camera,
        target: &mut Framebuffer,
        options: &RenderOptions<'_>,
    ) -> Result<RenderStats> {
        if target.empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "render target is empty",
            ));
        }
        if camera.viewport_width() != target.width() || camera.viewport_height() != target.height()
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "camera viewport does not match the render target",
            )
            .with_context(format!(
                "{}x{} vs {}x{}",
                camera.viewport_width(),
                camera.viewport_height(),
                target.width(),
                target.height()
            )));
        }

        let pool: &TaskPool = match options.pool {
            Some(pool) => pool,
            None => TaskPool::shared(),
        };

        if options.clear {
            // Parallel over tiles: on a 4K target this is 33 MB of writes, which
            // is worth splitting even though it is only a memset.
            TileTarget::new(target).fill(pool, options.background);
        }

        self.depth_pull = Self::depth_pull_for(camera);
        self.transform_vertices(list, camera, target, pool);
        self.build_screen_primitives(list, target, options, pool);
        self.rasterise_tiles(target, options, pool);

        let mut total = RenderStats {
            vertices: list.positions.len(),
            tiles: target.tile_count(),
            ..RenderStats::default()
        };
        for chunk in &self.chunks[..self.active_chunks] {
            total.triangles_submitted += chunk.stats.triangles_submitted;
            total.triangles_rasterised += chunk.stats.triangles_rasterised;
            total.lines_submitted += chunk.stats.lines_submitted;
            total.points_submitted += chunk.stats.points_submitted;
            total.bin_entries += chunk.stats.bin_entries;
        }
        for tile in &self.tile_stats {
            total.fragments += tile.fragments;
        }
        Ok(total)
    }

    // A line's bias is `pixels` footprints of view distance towards the eye,
    // where a footprint is the world size of one pixel at the line's own depth
    // (Camera::world_per_pixel_at). As a change of reversed depth d:
    //
    //   perspective   d = n (f - z) / ((f - n) z) for eye distance z, so
    //                 dd/dz = -n f / ((f - n) z^2); a footprint is z * 2 tan(fov/2)
    //                 / H, and the pull is pixels * (2 tan(fov/2) / H) * (d + n / (f - n))
    //   orthographic  d = (f - z) / (f - n); a footprint is orthoHeight / H, so the
    //                 pull is pixels * (orthoHeight / H) / (f - n), a constant
    //
    // both of the form pixels * (scale * d + offset). It used to be a constant in
    // NDC depth, which after a depth-range fit was anything from 1 to 25 times the
    // depth span of a whole scene: a biased line drawn on the ground behind a
    // 25 m building showed through all of it.
    fn depth_pull_for(camera: &Camera) -> DepthPull {
        let height = f64::from(camera.viewport_height());
        let range = camera.far_plane() - camera.near_plane();
        if !(height > 0.0) || !(range > 0.0) {
            return DepthPull::default();
        }
        if camera.projection() == Projection::Orthographic {
            return DepthPull {
                scale: 0.0,
                offset: (camera.orthographic_height() / height / range) as f32,
            };
        }
        let angle = 2.0 * (camera.field_of_view() * 0.5).tan() / height;
        DepthPull {
            scale: angle as f32,
            offset: (angle * camera.near_plane() / range) as f32,
        }
    }

    fn transform_vertices(
        &mut self,
        list: &DrawList,
        camera: &Camera,
        target: &Framebuffer,
        pool: &TaskPool,
    ) {
        let mvp = camera.view_projection();
        let count = list.positions.len();
        self.clip.resize(count, ClipVertex::default());
        self.clip_codes.resize(count, 0);
        self.screen.resize(count, ScreenVertex::default());
        let width = target.width() as f32;
        let height = target.height() as f32;
        // Read once a frame, as the C++ reads the level once a transform.
        let kernels = simd::kernels();
        let matrix = mvp.data;

        // Read straight out of the SoA position array. Nothing else is touched,
        // so this streams at memory bandwidth rather than striding over colours.
        disjoint::for_each_range3(
            pool,
            4096,
            &mut self.clip,
            &mut self.clip_codes,
            &mut self.screen,
            |lo, clip, codes, screen| {
                let hi = lo + clip.len();
                let mut done = 0;
                if let Some(kernels) = kernels
                    && hi - lo >= TRANSFORM_BATCH_MINIMUM
                {
                    let whole = (hi - lo) / 4 * 4;
                    let colors = list.colors.get(lo..).unwrap_or(&[]);
                    simd::transform_vertices(
                        kernels,
                        &matrix,
                        &list.positions[lo..lo + whole],
                        colors,
                        width,
                        height,
                        &mut clip[..whole],
                        &mut codes[..whole],
                        &mut screen[..whole],
                    );
                    done = whole;
                }
                for k in done..clip.len() {
                    let i = lo + k;
                    let color = list.colors.get(i).copied().unwrap_or(rgba(255, 255, 255));
                    let (out, code, projected) =
                        transform_one(&mvp, list.positions[i], color, width, height);
                    clip[k] = out;
                    codes[k] = code;
                    if let Some(projected) = projected {
                        screen[k] = projected;
                    }
                }
            },
        );
    }

    fn build_screen_primitives(
        &mut self,
        list: &DrawList,
        target: &Framebuffer,
        options: &RenderOptions<'_>,
        pool: &TaskPool,
    ) {
        let primitive_count = list.triangles.len() + list.lines.len() + list.points.len();
        let chunk_count = chunk_count_for(primitive_count);

        // Grown, never shrunk (active_chunks says why).
        if self.chunks.len() < chunk_count {
            self.chunks.resize_with(chunk_count, Chunk::default);
        }
        self.active_chunks = chunk_count;
        for chunk in &mut self.chunks[..chunk_count] {
            chunk.triangles.clear();
            chunk.points.clear();
            chunk.line_start = 0;
            chunk.stats = RenderStats::default();
        }
        if chunk_count == 0 {
            return;
        }

        // The bin step needs each chunk's own triangle/point index, so the chunk
        // boundaries are over the combined primitive stream: triangles first,
        // then lines, then points, each chunk taking a contiguous slice.
        let stage = StageTwo {
            list,
            clip: &self.clip,
            codes: &self.clip_codes,
            screen: &self.screen,
            viewport: Viewport {
                width: target.width() as f32,
                height: target.height() as f32,
            },
            cull: options.backface_cull,
            pull: self.depth_pull,
            per_chunk: primitive_count.div_ceil(chunk_count),
            primitive_count,
        };
        disjoint::for_each_mut(
            pool,
            &mut self.chunks[..chunk_count],
            |chunk_index, chunk| {
                stage.build(chunk_index, chunk);
            },
        );

        self.bin_primitives(target, pool);
    }

    fn bin_primitives(&mut self, target: &Framebuffer, pool: &TaskPool) {
        if target.tiles_across() <= 0 || target.tiles_down() <= 0 {
            return;
        }
        // Per chunk, so no two threads ever append to the same bin.
        let tiles = target.tile_count();
        disjoint::for_each_mut(pool, &mut self.chunks[..self.active_chunks], |_, chunk| {
            bin_chunk(chunk, target, tiles)
        });
    }

    fn rasterise_tiles(
        &mut self,
        target: &mut Framebuffer,
        options: &RenderOptions<'_>,
        pool: &TaskPool,
    ) {
        let tiles = target.tile_count();
        self.tile_stats.clear();
        self.tile_stats.resize(tiles, RenderStats::default());
        let depth_write = options.depth_write;
        // Read once a frame, as the C++ reads the level once a rasterisation.
        let kernel = simd::kernels();

        let mut any_points = false;
        let mut any_filled = false;
        for chunk in &self.chunks[..self.active_chunks] {
            any_points = any_points || !chunk.points.is_empty();
            any_filled = any_filled || chunk.line_start > 0;
        }

        if !any_points {
            self.sweep(Sweep::Everything, target, depth_write, kernel, pool);
            return;
        }
        if any_filled {
            self.sweep(Sweep::Filled, target, depth_write, kernel, pool);
        }
        self.decide_points(target, pool);
        self.sweep(Sweep::LinesThenPoints, target, depth_write, kernel, pool);
    }

    fn sweep(
        &mut self,
        what: Sweep,
        target: &mut Framebuffer,
        depth_write: bool,
        kernel: Option<Kernels>,
        pool: &TaskPool,
    ) {
        let chunks = &self.chunks[..self.active_chunks];
        TileTarget::new(target).for_each(
            pool,
            &mut self.tile_stats,
            |tile_index, pixels, stats| {
                sweep_tile(what, chunks, tile_index, pixels, stats, depth_write, kernel);
            },
        );
    }

    // ---- points -------------------------------------------------------------------
    //
    // A point is a size x size square at ONE depth, its centre's. Tested pixel by
    // pixel, a point lying on a surface lost its lower rows to it: seen at
    // elevation e, the surface under a row k pixels below the centre is nearer
    // than the centre by about k / tan(e) pixel footprints, which neither the
    // surface's one-pixel slope push nor the point's 1.5-footprint pull covers
    // once k > 1 - 6% of the pixels of draped survey points at the iso view, 20%
    // at 0.25 rad. Pulling points further would show them through the walls in
    // front of them instead. So a point is decided ONCE, at the pixel of its
    // square nearest its centre, and then drawn whole (draw_point).
    //
    // Decided against the earlier passes and this pass's FILLED triangles, and
    // not its lines: solids hide a point, the linework it is drawn with does not.
    // A survey point usually sits on the strings through it, in the same pass
    // with the same pull, and decided after them it tied with a string crossing
    // its centre pixel and vanished whole - the survey points along the kerbs of
    // a real archive did. The depth buffer read here is final for what it holds,
    // the fill that follows goes tile by tile in index order, and the frame is
    // the same on any number of threads (Rule 7).
    fn decide_points(&mut self, target: &Framebuffer, pool: &TaskPool) {
        let chunks = &mut self.chunks[..self.active_chunks];
        let point_count: usize = chunks.iter().map(|chunk| chunk.points.len()).sum();
        let width = target.width();
        let height = target.height();
        let depth = target.depth();
        let decide = |chunk: &mut Chunk| {
            for p in &mut chunk.points {
                let x0 = ceil_in(p.x - p.half - 0.5, 0, width);
                let x1 = ceil_in(p.x + p.half - 0.5, 0, width) - 1;
                let y0 = ceil_in(p.y - p.half - 0.5, 0, height);
                let y1 = ceil_in(p.y + p.half - 0.5, 0, height) - 1;
                if x0 > x1 || y0 > y1 {
                    p.visible = false; // no pixel of it on the image
                    continue;
                }
                // floor(c) is always one of the square's pixels (its centre is
                // within half a pixel of c, and h >= 0.5); clamped into the part
                // on the image when the centre is off it.
                let cx = pixel_floor(p.x, x0, x1);
                let cy = pixel_floor(p.y, y0, y1);
                let under = depth[cy as usize * width as usize + cx as usize];
                // Reversed Z: strictly nearer, and not past the near plane; NaN
                // fails both.
                p.visible = p.z > under && p.z <= 1.0;
            }
        };
        if point_count <= POINTS_INLINE {
            for chunk in chunks.iter_mut() {
                decide(chunk);
            }
        } else {
            disjoint::for_each_mut(pool, chunks, |_, chunk| decide(chunk));
        }
    }
}
