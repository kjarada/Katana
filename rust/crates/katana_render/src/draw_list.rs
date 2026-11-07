//! Device-independent geometry handed to a renderer (PLAN.MD Phase 15, Rule 3).
//!
//! This is the whole interface between the application and whatever draws it.
//! The renderer never sees an entity, a TIN surface or a document: something
//! in a higher layer walks the model and appends primitives here, which is
//! what makes it impossible for the renderer to become a second source of
//! truth, and what lets a GPU backend replace the software one without
//! touching a line of application code.
//!
//! LAYOUT is structure-of-arrays (PLAN.MD section 33). Vertex transformation
//! reads positions and nothing else, so positions are stored alone: a
//! transform pass walks one dense array instead of striding over interleaved
//! colours it will not look at until shading.
//!
//! LIGHTING IS BAKED. There are no normals and no material here, because
//! lighting a survey TIN means one dot product per triangle and the answer
//! does not change while the user orbits a static surface. The scene builder
//! shades the vertex colours once; the rasteriser only interpolates. That
//! keeps the inner loop free of per-pixel lighting and keeps the renderer's
//! contract to "interpolate what you are given".

use crate::framebuffer::Rgba;
use katana_geometry::Point3;
use katana_math::AABB;

/// Index into `DrawList::positions` / `DrawList::colors`.
pub type VertexIndex = u32;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DrawTriangle {
    pub a: VertexIndex,
    pub b: VertexIndex,
    pub c: VertexIndex,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawLine {
    pub a: VertexIndex,
    pub b: VertexIndex,
    /// Pixels.
    pub width: f32,
    /// Pulls the line towards the viewer before the depth test, in pixel
    /// footprints of view distance (the rasteriser's depth pull). Wireframe
    /// over a shaded surface is the normal case in CAD and the edges lie
    /// exactly in the plane of the triangles they bound, so without a bias
    /// they z-fight into a dashed mess. Measured in footprints, not world
    /// units, so it stays constant on screen whatever the scene scale.
    pub depth_bias: f32,
}

impl Default for DrawLine {
    fn default() -> Self {
        Self {
            a: 0,
            b: 0,
            width: 1.0,
            depth_bias: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawPoint {
    pub a: VertexIndex,
    /// Pixels; the square drawn is size x size.
    pub size: f32,
    pub depth_bias: f32,
}

impl Default for DrawPoint {
    fn default() -> Self {
        Self {
            a: 0,
            size: 1.0,
            depth_bias: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    pub positions: Vec<Point3>,
    /// Parallel to positions.
    pub colors: Vec<Rgba>,

    pub triangles: Vec<DrawTriangle>,
    pub lines: Vec<DrawLine>,
    pub points: Vec<DrawPoint>,
}

impl DrawList {
    /// Keeps the capacity, so a viewport that rebuilds its list every frame
    /// stops allocating after the first one (PLAN.MD section 33).
    pub fn clear(&mut self) {
        self.positions.clear();
        self.colors.clear();
        self.triangles.clear();
        self.lines.clear();
        self.points.clear();
    }

    pub fn empty(&self) -> bool {
        self.triangles.is_empty() && self.lines.is_empty() && self.points.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Appends a vertex and returns its index.
    pub fn add_vertex(&mut self, position: Point3, color: Rgba) -> VertexIndex {
        // Truncated as the C++'s static_cast<VertexIndex> is.
        let index = self.positions.len() as VertexIndex;
        self.positions.push(position);
        self.colors.push(color);
        index
    }

    pub fn add_triangle(&mut self, a: VertexIndex, b: VertexIndex, c: VertexIndex) {
        self.triangles.push(DrawTriangle { a, b, c });
    }

    /// The C++'s defaults are a width of 1 and no bias (`DrawLine::default`).
    pub fn add_line(&mut self, a: VertexIndex, b: VertexIndex, width: f32, depth_bias: f32) {
        self.lines.push(DrawLine {
            a,
            b,
            width,
            depth_bias,
        });
    }

    /// The C++'s defaults are a size of 1 and no bias (`DrawPoint::default`).
    pub fn add_point(&mut self, a: VertexIndex, size: f32, depth_bias: f32) {
        self.points.push(DrawPoint {
            a,
            size,
            depth_bias,
        });
    }

    /// Convenience for one-off geometry that shares no vertices.
    pub fn add_segment(
        &mut self,
        from: Point3,
        to: Point3,
        color: Rgba,
        width: f32,
        depth_bias: f32,
    ) {
        let a = self.add_vertex(from, color);
        let b = self.add_vertex(to, color);
        self.add_line(a, b, width, depth_bias);
    }

    /// Axis-aligned box of every vertex that any primitive references. Vertices
    /// added but never referenced are excluded, so a stale append cannot pull
    /// the camera off to nowhere when the caller frames the scene.
    ///
    /// Each vertex is visited once, however many primitives use it: the
    /// primitives mark the vertices they use, and the marked ones are bounded
    /// in index order. On a TIN a vertex is a corner of about six triangles
    /// and an end of as many edges, and bounding it once per use was most of
    /// what packing a scene for the GPU cost. A box of the same values in
    /// another order is the same box, except for which zero an extreme is
    /// (-0 < +0 is false, so the first zero met is kept); so when an extreme
    /// is a zero the box is bounded again in primitive order, and the result
    /// is the same to the bit as that order's.
    pub fn bounds(&self) -> AABB {
        let mut used = Vec::new();
        self.bounds_with_scratch(&mut used)
    }

    /// The same, with `used` as the marks' scratch - one byte a vertex, kept
    /// between calls by a caller that bounds large lists often.
    pub fn bounds_with_scratch(&self, used: &mut Vec<u8>) -> AABB {
        let count = self.positions.len();
        used.clear();
        used.resize(count, 0);
        {
            let mut mark = |index: VertexIndex| {
                if (index as usize) < count {
                    used[index as usize] = 1;
                }
            };
            for triangle in &self.triangles {
                mark(triangle.a);
                mark(triangle.b);
                mark(triangle.c);
            }
            for line in &self.lines {
                mark(line.a);
                mark(line.b);
            }
            for point in &self.points {
                mark(point.a);
            }
        }
        let mut bounds = AABB::default();
        for (position, &marked) in self.positions.iter().zip(used.iter()) {
            if marked != 0 && position.is_finite() {
                bounds.expand_point(*position);
            }
        }
        if bounds.min.x == 0.0
            || bounds.min.y == 0.0
            || bounds.min.z == 0.0
            || bounds.max.x == 0.0
            || bounds.max.y == 0.0
            || bounds.max.z == 0.0
        {
            return bounds_in_primitive_order(self);
        }
        bounds
    }

    /// True when every position is finite. The rasteriser assumes this and the
    /// scene builders guarantee it; asserted by the tests rather than
    /// re-checked per frame.
    pub fn all_finite(&self) -> bool {
        self.positions.iter().all(|position| position.is_finite())
    }
}

/// The reference order: every use of every vertex, primitive by primitive.
/// What bounds() falls back to when the order decides the result.
fn bounds_in_primitive_order(list: &DrawList) -> AABB {
    let mut bounds = AABB::default();
    let mut include = |index: VertexIndex| {
        if let Some(position) = list.positions.get(index as usize)
            && position.is_finite()
        {
            bounds.expand_point(*position);
        }
    };
    for triangle in &list.triangles {
        include(triangle.a);
        include(triangle.b);
        include(triangle.c);
    }
    for line in &list.lines {
        include(line.a);
        include(line.b);
    }
    for point in &list.points {
        include(point.a);
    }
    bounds
}
