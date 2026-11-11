//! A DrawList packed the way the GPU reads it (docs/gpu.md, "Architecture";
//! gpu_scene.hpp).
//!
//! This is the CPU half of an upload, and it has no wgpu in it: the packing is
//! plain arithmetic that can be tested without a device, and the renderer then
//! copies these arrays into GPU buffers byte for byte.
//!
//! WHAT CHANGES FROM THE DRAW LIST
//!
//! * Positions become float offsets from `origin`, which stays in double
//!   (scene_origin has the precision rule). 16 bytes a vertex instead of the
//!   draw list's 28 (three doubles and a colour).
//! * Triangles keep sharing vertices through a 32-bit index buffer.
//! * Lines and points carry their own positions and colours (a line its two
//!   ends, a point its centre), and a shader widens each into a screen-space
//!   quad. The software path widens every line into two triangles on the CPU
//!   (katana_render's Rasterizer); here that work - and the antialiasing -
//!   happens on the GPU.
//! * A line's or point's depth_bias is NOT carried. It is a constant in NDC
//!   depth, sized for the software path's standard-Z buffer, and applied to a
//!   reversed-Z float buffer it would pull a line metres towards the eye (the
//!   x-ray defect). The GPU pushes filled triangles back by a slope-scaled
//!   polygon offset instead (gpu_renderer, FILL_SLOPE_BIAS), which is what lets
//!   an edge lying in a surface win without letting a line behind a building
//!   show through it.
//!
//! Indices the draw list holds that point past its vertices are dropped here,
//! as the software path skips them, so a malformed list cannot make the GPU
//! read outside a buffer.

use crate::scene_origin::{choose_scene_origin, relative_position};
use crate::simd::pack_kernels;
use katana_math::{AABB, Vec3, cpp};
use katana_pointcloud::PointCloud;
use katana_render::{DrawList, Rgba, rgba};

/// One vertex: position relative to the scene origin and the draw list's
/// 0xAARRGGBB colour, which on a little-endian machine is the bytes B, G, R, A
/// (the shaders swizzle it back).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub color: u32,
}

/// One end of a line. The width is carried at both ends, so that each end is
/// a self-contained record and the two together one 40-byte INSTANCE of the
/// six-vertex quad the shader builds (shader_library, Expansion::Instanced).
/// The C++'s geometry-shader expansion read the same bytes as two vertices of
/// a line list; wgpu has none, but the layout is kept.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuLineEnd {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub color: u32,
    /// Logical pixels, as DrawLine::width.
    pub width: f32,
}

impl Default for GpuLineEnd {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            color: 0,
            width: 1.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuLine {
    pub a: GpuLineEnd,
    pub b: GpuLineEnd,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuPoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub color: u32,
    /// Logical pixels, as DrawPoint::size: a size x size square.
    pub size: f32,
    pub unused: f32,
}

impl Default for GpuPoint {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            color: 0,
            size: 1.0,
            unused: 0.0,
        }
    }
}

// The records are uploaded as they lie in memory (crate::plain): every field
// is four bytes, so #[repr(C)] leaves no padding, and these are the sizes the
// shaders' vertex layouts read.
const _: () = assert!(std::mem::size_of::<GpuVertex>() == 16);
const _: () = assert!(std::mem::size_of::<GpuLineEnd>() == 20);
const _: () = assert!(std::mem::size_of::<GpuLine>() == 40);
const _: () = assert!(std::mem::size_of::<GpuPoint>() == 24);
const _: () = assert!(std::mem::size_of::<GpuCloudPoint>() == 16);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuSceneData {
    /// Kept in double: every position below is relative to this.
    pub origin: Vec3,
    /// What the draw list's primitives reference, in world coordinates.
    pub bounds: AABB,

    pub vertices: Vec<GpuVertex>,
    /// Three per triangle.
    pub triangle_indices: Vec<u32>,
    pub lines: Vec<GpuLine>,
    pub points: Vec<GpuPoint>,
}

impl GpuSceneData {
    /// Empties every array, keeping its capacity.
    pub fn clear(&mut self) {
        self.origin = Vec3::new(0.0, 0.0, 0.0);
        self.bounds = AABB::default();
        self.vertices.clear();
        self.triangle_indices.clear();
        self.lines.clear();
        self.points.clear();
    }

    pub fn triangle_count(&self) -> usize {
        self.triangle_indices.len() / 3
    }

    pub fn empty(&self) -> bool {
        self.triangle_indices.is_empty() && self.lines.is_empty() && self.points.is_empty()
    }

    /// Bytes the GPU buffers will hold, for the stats line and the docs.
    pub fn byte_size(&self) -> usize {
        self.vertices.len() * std::mem::size_of::<GpuVertex>()
            + self.triangle_indices.len() * std::mem::size_of::<u32>()
            + self.lines.len() * std::mem::size_of::<GpuLine>()
            + self.points.len() * std::mem::size_of::<GpuPoint>()
    }
}

/// Packs `list` into `out` (cleared first, capacity kept), relative to the
/// centre of the list's bounds.
pub fn pack_draw_list(list: &DrawList, out: &mut GpuSceneData) {
    // Bounded once, for the origin and the box both.
    let bounds = list.bounds();
    pack_draw_list_with_bounds(list, choose_scene_origin(bounds), bounds, out);
}

/// The same, relative to a given origin: for the layers of one scene, which
/// share the centre of their joint bounds, and for tests that need to prove the
/// origin is what makes large coordinates safe.
pub fn pack_draw_list_with_origin(list: &DrawList, origin: Vec3, out: &mut GpuSceneData) {
    pack_draw_list_with_bounds(list, origin, list.bounds(), out);
}

// The draw list's positions as the scalar loop takes them: the reference every
// kernel is held to, bit for bit.
fn pack_vertices(list: &DrawList, count: usize, origin: Vec3, out: &mut Vec<GpuVertex>) {
    out.resize(count, GpuVertex::default());
    // Any length but none: the kernel has no set-up to repay, and at four
    // vertices it already packs as fast as the loop (docs/gpu.md, "Packing").
    // On 64-bit ARM the NEON kernel takes the same rule, not measured there.
    if count > 0
        && let Some(kernels) = pack_kernels::kernels()
    {
        pack_kernels::pack_vertices(
            kernels,
            &list.positions[..count],
            &list.colors[..count],
            [origin.x, origin.y, origin.z],
            out,
        );
        return;
    }
    for ((vertex, &position), &color) in out.iter_mut().zip(&list.positions).zip(&list.colors) {
        let p = relative_position(position, origin);
        *vertex = GpuVertex {
            x: p[0],
            y: p[1],
            z: p[2],
            color,
        };
    }
}

/// The same, with the list's bounds (list.bounds()) already worked out: a
/// caller that needed them to choose the origin does not bound the list twice.
pub fn pack_draw_list_with_bounds(
    list: &DrawList,
    origin: Vec3,
    bounds: AABB,
    out: &mut GpuSceneData,
) {
    out.clear();
    out.origin = origin;
    out.bounds = bounds;

    let count = std::cmp::min(list.positions.len(), list.colors.len());
    pack_vertices(list, count, origin, &mut out.vertices);

    // The indices are sized for every triangle, written in place and cut back
    // to those kept: sizing plain integers is one memset, and three pushes a
    // triangle made the C++'s whole pack 14% slower (BM_GpuPack, docs/gpu.md
    // "Packing"). The lines are the other way round and stay appended: a
    // line's width defaults to 1, so sizing the array writes every line once
    // before the loop writes it again, and that made the pack 24% slower. The
    // points, whose size defaults to 1 the same way, are appended too.
    out.triangle_indices.resize(list.triangles.len() * 3, 0);
    let mut kept = 0;
    for triangle in &list.triangles {
        if triangle.a as usize >= count
            || triangle.b as usize >= count
            || triangle.c as usize >= count
        {
            continue;
        }
        out.triangle_indices[kept] = triangle.a;
        out.triangle_indices[kept + 1] = triangle.b;
        out.triangle_indices[kept + 2] = triangle.c;
        kept += 3;
    }
    out.triangle_indices.truncate(kept);

    out.lines.reserve(list.lines.len());
    for source in &list.lines {
        if source.a as usize >= count || source.b as usize >= count {
            continue;
        }
        let a = out.vertices[source.a as usize];
        let b = out.vertices[source.b as usize];
        // std::max(width, 1.0f), whose NaN is the width's own.
        let width = cpp::max(f64::from(source.width), 1.0) as f32;
        out.lines.push(GpuLine {
            a: GpuLineEnd {
                x: a.x,
                y: a.y,
                z: a.z,
                color: a.color,
                width,
            },
            b: GpuLineEnd {
                x: b.x,
                y: b.y,
                z: b.z,
                color: b.color,
                width,
            },
        });
    }

    out.points.reserve(list.points.len());
    for source in &list.points {
        if source.a as usize >= count {
            continue;
        }
        let v = out.vertices[source.a as usize];
        out.points.push(GpuPoint {
            x: v.x,
            y: v.y,
            z: v.z,
            color: v.color,
            size: cpp::max(f64::from(source.size), 1.0) as f32,
            unused: 0.0,
        });
    }
}

// ---- point clouds ---------------------------------------------------------------

/// One cloud point: 16 bytes against the 40 of PointCloudPoint.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuCloudPoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub color: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointCloudData {
    pub origin: Vec3,
    pub points: Vec<GpuCloudPoint>,
    /// How many of the source points were offered; points.len() is what the
    /// budget let through.
    pub source_count: usize,
}

impl PointCloudData {
    pub fn clear(&mut self) {
        self.origin = Vec3::new(0.0, 0.0, 0.0);
        self.points.clear();
        self.source_count = 0;
    }
}

// Every stride-th of `count` points, the stride the smallest whole number that
// keeps the count within the budget: ceil(count / budget).
fn pack_thinned(
    count: usize,
    position_of: impl Fn(usize) -> Vec3,
    color_of: impl Fn(usize) -> Rgba,
    origin: Vec3,
    budget: usize,
    out: &mut PointCloudData,
) {
    out.clear();
    out.origin = origin;
    out.source_count = count;
    let mut stride = 1;
    if budget > 0 && count > budget {
        stride = count.div_ceil(budget);
    }
    out.points.reserve(count / stride + 1);
    for i in (0..count).step_by(stride) {
        let position = position_of(i);
        if !position.is_finite() {
            continue;
        }
        let p = relative_position(position, origin);
        out.points.push(GpuCloudPoint {
            x: p[0],
            y: p[1],
            z: p[2],
            color: color_of(i),
        });
    }
}

/// Packs at most `budget` of the given points, relative to `origin`, taking an
/// evenly spread subset when there are more: every k-th point, k chosen so the
/// budget is not exceeded. Even spacing through the SOURCE ORDER is the right
/// thinning for a file read in acquisition or tile order (a LAS scan line is
/// spatially coherent); a budget of 0 means no limit. `colors` is parallel to
/// `positions`, or empty for a single `fallback_color`.
pub fn pack_point_cloud(
    positions: &[Vec3],
    colors: &[Rgba],
    fallback_color: Rgba,
    origin: Vec3,
    budget: usize,
    out: &mut PointCloudData,
) {
    let colored = colors.len() == positions.len();
    pack_thinned(
        positions.len(),
        |i| positions[i],
        |i| if colored { colors[i] } else { fallback_color },
        origin,
        budget,
        out,
    );
}

/// The same for a cloud as the point-cloud engine reads it, relative to the
/// centre of its bounds (the points' own box when the bounds are empty): each
/// point in its own colour when it has one, `fallback_color` otherwise. Reads
/// the engine's 40-byte points in place - no copy of the positions first,
/// which for two million points would be 48 MB.
pub fn pack_engine_point_cloud(
    cloud: &PointCloud,
    fallback_color: Rgba,
    budget: usize,
    out: &mut PointCloudData,
) {
    let points = &cloud.points;
    let mut bounds = AABB::default();
    if !cloud.bounds.empty() {
        bounds.expand_point(Vec3::new(
            cloud.bounds.min_x,
            cloud.bounds.min_y,
            cloud.bounds.min_z,
        ));
        bounds.expand_point(Vec3::new(
            cloud.bounds.max_x,
            cloud.bounds.max_y,
            cloud.bounds.max_z,
        ));
    } else {
        for point in points {
            let position = Vec3::new(point.x, point.y, point.z);
            if position.is_finite() {
                // A stray infinity would put the origin nowhere.
                bounds.expand_point(position);
            }
        }
    }
    pack_thinned(
        points.len(),
        |i| Vec3::new(points[i].x, points[i].y, points[i].z),
        |i| {
            let point = &points[i];
            if point.has_color {
                rgba(point.red, point.green, point.blue)
            } else {
                fallback_color
            }
        },
        choose_scene_origin(bounds),
        budget,
        out,
    );
}
