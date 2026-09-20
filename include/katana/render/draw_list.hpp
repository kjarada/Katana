#pragma once

// Device-independent geometry handed to a renderer (PLAN.MD Phase 15, Rule 3).
//
// This is the whole interface between the application and whatever draws it.
// The renderer never sees an Entity, a TinSurface or a Document: something in
// a higher layer walks the model and appends primitives here, which is what
// makes it impossible for the renderer to become a second source of truth, and
// what lets the Vulkan backend of Phase 15 replace the software one without
// touching a line of application code.
//
// LAYOUT is structure-of-arrays (PLAN.MD section 33). Vertex transformation
// reads positions and nothing else, so positions are stored alone: a
// transform pass walks one dense array instead of striding over interleaved
// colours it will not look at until shading.
//
// LIGHTING IS BAKED. There are no normals and no material here, because
// lighting a survey TIN means one dot product per triangle and the answer does
// not change while the user orbits a static surface. The scene builder shades
// the vertex colours once; the rasteriser only interpolates. That keeps the
// inner loop free of per-pixel lighting and keeps the renderer's contract to
// "interpolate what you are given".

#include <cstdint>
#include <vector>

#include "katana/geometry/primitives3d.hpp"
#include "katana/math/primitives.hpp"
#include "katana/render/framebuffer.hpp"

namespace katana::render {

using katana::geometry::Point3;

// Index into DrawList::positions / DrawList::colors.
using VertexIndex = std::uint32_t;

struct DrawTriangle {
    VertexIndex a = 0;
    VertexIndex b = 0;
    VertexIndex c = 0;
};

struct DrawLine {
    VertexIndex a = 0;
    VertexIndex b = 0;
    float width = 1.0f; // pixels
    // Pulls the line towards the viewer in NDC depth before the test. Wireframe
    // over a shaded surface is the normal case in CAD and the edges lie exactly
    // in the plane of the triangles they bound, so without a bias they z-fight
    // into a dashed mess. Applied in depth units, not world units, so it stays
    // constant on screen whatever the scene scale.
    float depthBias = 0.0f;
};

struct DrawPoint {
    VertexIndex a = 0;
    float size = 1.0f; // pixels; the square drawn is size x size
    float depthBias = 0.0f;
};

struct DrawList {
    std::vector<Point3> positions;
    std::vector<Rgba> colors; // parallel to positions

    std::vector<DrawTriangle> triangles;
    std::vector<DrawLine> lines;
    std::vector<DrawPoint> points;

    // Keeps the capacity, so a viewport that rebuilds its list every frame
    // stops allocating after the first one (PLAN.MD section 33).
    void clear();

    [[nodiscard]] bool empty() const
    {
        return triangles.empty() && lines.empty() && points.empty();
    }
    [[nodiscard]] std::size_t vertexCount() const { return positions.size(); }

    // Appends a vertex and returns its index.
    VertexIndex addVertex(const Point3& position, Rgba color);

    void addTriangle(VertexIndex a, VertexIndex b, VertexIndex c);
    void addLine(VertexIndex a, VertexIndex b, float width = 1.0f, float depthBias = 0.0f);
    void addPoint(VertexIndex a, float size = 1.0f, float depthBias = 0.0f);

    // Convenience for one-off geometry that shares no vertices.
    void addSegment(const Point3& from, const Point3& to, Rgba color, float width = 1.0f,
                    float depthBias = 0.0f);

    // Axis-aligned box of every vertex that any primitive references. Vertices
    // added but never referenced are excluded, so a stale append cannot pull
    // the camera off to nowhere when the caller frames the scene.
    [[nodiscard]] katana::math::AABB bounds() const;

    // True when every position is finite. The rasteriser assumes this and the
    // scene builders guarantee it; asserted by the tests rather than re-checked
    // per frame.
    [[nodiscard]] bool allFinite() const;
};

} // namespace katana::render
