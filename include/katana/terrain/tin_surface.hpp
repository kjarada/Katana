#pragma once

// Triangulated irregular network surface (PLAN.MD Phase 14).
//
// TinSurface is an immutable, self-contained value: plain Katana types in flat
// arrays, no third-party objects. It can be copied, stored, serialised and handed
// to a renderer. Surfaces are produced by buildTin() (tin_builder.hpp) or, for
// triangulations that already exist (imported LandXML, stored projects), by
// TinSurface::create().
//
// Conventions
//   * Plan view is the XY plane; z is the elevation. Model units are metres.
//   * Triangles are counter-clockwise in plan view. Edge k of a triangle joins
//     its vertex k to its vertex (k + 1) % 3.
//   * Indices are 32 bit: a surface holds fewer than 2^32 - 1 vertices and
//     triangles (12 bytes per triangle instead of 24; see docs/terrain.md).
//   * Coincidence decisions use math::tolerance::kGeometric, direction decisions
//     math::tolerance::kAngular. Nothing else.
//   * All const member functions are safe to call concurrently.

#include <array>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <optional>
#include <span>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/primitives3d.hpp"

namespace katana::terrain {

using Point2 = katana::geometry::Point2;
using Point3 = katana::geometry::Point3;
using Box2 = katana::geometry::Box2;

namespace detail {

struct TinSurfaceFactory; // trusted construction path used by the builder (src/ only)

// Point-location index of a TinSurface: a uniform grid over its bounds, each
// cell listing the triangles that reach into it (CSR layout, two flat arrays).
// Implementation detail; not part of the API.
struct LocatorGrid {
    Point2 origin;
    double cellWidth = 1.0;
    double cellHeight = 1.0;
    std::uint32_t columns = 0;
    std::uint32_t rows = 0;
    std::vector<std::uint32_t> cellStart;     // columns * rows + 1 offsets
    std::vector<std::uint32_t> cellTriangles; // ascending within each cell
};

} // namespace detail

inline constexpr std::uint32_t kNoTriangle = std::numeric_limits<std::uint32_t>::max();
inline constexpr std::uint32_t kNoVertex = std::numeric_limits<std::uint32_t>::max();

// Vertex indices of one triangle, counter-clockwise in plan view.
using TinTriangle = std::array<std::uint32_t, 3>;
// Triangle across edge k (vertex k -> vertex k + 1); kNoTriangle on the surface edge.
using TinNeighbors = std::array<std::uint32_t, 3>;

// Where a plan position falls on the surface.
struct SurfaceLocation {
    std::uint32_t triangle = kNoTriangle;
    // Barycentric weights of the triangle's vertices; non-negative, sum to 1.
    std::array<double, 3> weights{};
    // False when the position was matched only through the kGeometric tolerance
    // (it lies on the rim of the surface, or on a sliver triangle); `weights`
    // then describe the closest point of the triangle.
    bool interior = true;
};

struct SlopeAspect {
    // Rise over run (the tangent of slopeAngle). Dimensionless, >= 0.
    double slope = 0.0;
    // Inclination of the triangle against the horizontal. Radians in [0, pi/2).
    double slopeAngle = 0.0;
    // Azimuth of steepest descent: radians clockwise from north (+y) in
    // [0, 2*pi). nullopt when the triangle is flat (slopeAngle <= kAngular).
    std::optional<double> aspect;
};

class TinSurface {
  public:
    TinSurface() = default; // the empty surface

    // Builds a surface from an existing triangulation. `constrainedEdges` is
    // either empty or holds one bit mask per triangle (bit k set when edge k is a
    // breakline/boundary edge). Adjacency is derived. Fails with
    //   InvalidArgument  non-finite coordinates, indices out of range, a triangle
    //                    naming a vertex twice, flags of the wrong size or
    //                    disagreeing across a shared edge;
    //   InvalidGeometry  a clockwise triangle, or an edge used by more than two
    //                    triangles / twice in the same direction (non-manifold).
    // Triangles that are degenerate in plan view (height <= kGeometric) are
    // accepted: exact triangulators legitimately produce them from nearly
    // collinear points, and removing them would tear the adjacency.
    [[nodiscard]] static katana::core::Result<TinSurface>
    create(std::vector<Point3> vertices, std::vector<TinTriangle> triangles,
           std::vector<std::uint8_t> constrainedEdges = {});

    // ---- data -------------------------------------------------------------------

    [[nodiscard]] bool empty() const { return triangles_.empty(); }
    [[nodiscard]] std::size_t vertexCount() const { return vertices_.size(); }
    [[nodiscard]] std::size_t triangleCount() const { return triangles_.size(); }

    [[nodiscard]] const std::vector<Point3>& vertices() const { return vertices_; }
    [[nodiscard]] const std::vector<TinTriangle>& triangles() const { return triangles_; }
    [[nodiscard]] const std::vector<TinNeighbors>& neighbors() const { return neighbors_; }
    // One bit mask per triangle; bit k set when edge k is constrained.
    [[nodiscard]] const std::vector<std::uint8_t>& constrainedEdges() const
    {
        return constrainedEdges_;
    }
    [[nodiscard]] bool isEdgeConstrained(std::size_t triangle, std::size_t edge) const;

    // Plan extent and elevation range of the triangulated vertices. The box is
    // empty and the elevations are 0 for the empty surface.
    [[nodiscard]] const Box2& bounds() const { return bounds_; }
    [[nodiscard]] double minElevation() const { return minElevation_; }
    [[nodiscard]] double maxElevation() const { return maxElevation_; }

    // Sum of the triangle areas projected to the XY plane / measured in 3D.
    [[nodiscard]] double planArea() const { return planArea_; }
    [[nodiscard]] double surfaceArea() const { return surfaceArea_; }

    // ---- per triangle (throw std::out_of_range for a bad index) -------------------

    [[nodiscard]] katana::geometry::Triangle2 planTriangle(std::size_t triangle) const;
    [[nodiscard]] katana::geometry::Triangle3 spaceTriangle(std::size_t triangle) const;
    [[nodiscard]] double trianglePlanArea(std::size_t triangle) const;
    [[nodiscard]] double triangleSurfaceArea(std::size_t triangle) const;
    // nullopt when the triangle is degenerate in plan view (its plane is
    // vertical or undefined).
    [[nodiscard]] std::optional<SlopeAspect> triangleSlopeAspect(std::size_t triangle) const;

    // ---- point queries -------------------------------------------------------------
    //
    // Backed by a uniform bucket grid built with the surface: expected O(1) per
    // query. A position is on the surface when it is inside a triangle or within
    // kGeometric of one, so points exactly on edges, vertices and the rim work.
    // Where triangles share the position the lowest triangle index is reported.
    // nullopt: outside the surface, inside a hole, or non-finite input.

    [[nodiscard]] std::optional<SurfaceLocation> locate(const Point2& position) const;

    // Barycentric (linear) interpolation of the vertex elevations.
    [[nodiscard]] std::optional<double> elevationAt(const Point2& position) const;
    [[nodiscard]] std::optional<double> elevationAt(double x, double y) const
    {
        return elevationAt(Point2(x, y));
    }
    // Spot elevations: elevationAt() for every position, in order.
    [[nodiscard]] std::vector<std::optional<double>>
    elevationsAt(std::span<const Point2> positions) const;

    // Slope (rise over run) and aspect of the located triangle. aspectAt() is
    // also nullopt on flat ground. Both are discontinuous across edges; on a
    // shared edge the lowest-index triangle decides.
    [[nodiscard]] std::optional<double> slopeAt(const Point2& position) const;
    [[nodiscard]] std::optional<double> aspectAt(const Point2& position) const;

    // Exact comparison of the defining arrays (vertices, triangles, adjacency,
    // constraint flags). Used to assert determinism.
    friend bool operator==(const TinSurface& a, const TinSurface& b)
    {
        return a.vertices_ == b.vertices_ && a.triangles_ == b.triangles_ &&
               a.neighbors_ == b.neighbors_ && a.constrainedEdges_ == b.constrainedEdges_;
    }

  private:
    friend struct detail::TinSurfaceFactory;

    // Derives bounds, areas, elevation range and the locator grid from the arrays.
    [[nodiscard]] katana::core::Status finalize();

    std::vector<Point3> vertices_;
    std::vector<TinTriangle> triangles_;
    std::vector<TinNeighbors> neighbors_;
    std::vector<std::uint8_t> constrainedEdges_;
    Box2 bounds_;
    double minElevation_ = 0.0;
    double maxElevation_ = 0.0;
    double planArea_ = 0.0;
    double surfaceArea_ = 0.0;
    detail::LocatorGrid grid_;
};

} // namespace katana::terrain
