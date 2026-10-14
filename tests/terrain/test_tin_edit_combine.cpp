// Surfaces made of others (tin_edit.hpp: pasteSurface, clipToBoundary as a
// copy, offsetSurface, mergeSurfaces; docs/surface_editing.md, "Mixed
// terrain"). The surfaces are grids of a known size and level, so areas and
// levels are worked out by hand.
#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <set>
#include <utility>
#include <vector>

#include "katana/terrain/super_surface.hpp"
#include "katana/terrain/tin_edit.hpp"

using katana::core::ErrorCode;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::Polyline2;
using katana::terrain::TinSurface;
using katana::terrain::TinTriangle;

namespace {

// A flat grid at `level` over [x0, x0 + nx dx] x [y0, y0 + ny dy], each cell
// split along its rising diagonal.
TinSurface grid(double x0, double y0, int nx, int ny, double dx, double dy, double level)
{
    std::vector<Point3> vertices;
    for (int j = 0; j <= ny; ++j) {
        for (int i = 0; i <= nx; ++i) {
            vertices.emplace_back(x0 + i * dx, y0 + j * dy, level);
        }
    }
    const auto at = [nx](int i, int j) { return static_cast<std::uint32_t>(j * (nx + 1) + i); };
    std::vector<TinTriangle> triangles;
    for (int j = 0; j < ny; ++j) {
        for (int i = 0; i < nx; ++i) {
            triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
            triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
        }
    }
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

// The ends of the surface's constrained edges, each once.
std::vector<Point3> constrainedEnds(const TinSurface& surface)
{
    std::set<std::uint32_t> ends;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        for (std::size_t k = 0; k < 3; ++k) {
            if (surface.isEdgeConstrained(t, k)) {
                ends.insert(surface.triangles()[t][k]);
                ends.insert(surface.triangles()[t][(k + 1) % 3]);
            }
        }
    }
    std::vector<Point3> points;
    for (const std::uint32_t v : ends) {
        points.push_back(surface.vertices()[v]);
    }
    return points;
}

double constrainedLength(const TinSurface& surface)
{
    std::set<std::pair<std::uint32_t, std::uint32_t>> seen;
    double length = 0.0;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        for (std::size_t k = 0; k < 3; ++k) {
            const std::uint32_t a = surface.triangles()[t][k];
            const std::uint32_t b = surface.triangles()[t][(k + 1) % 3];
            if (surface.isEdgeConstrained(t, k) && seen.insert({std::min(a, b), std::max(a, b)}).second) {
                const Point3& p = surface.vertices()[a];
                const Point3& q = surface.vertices()[b];
                length += std::hypot(q.x - p.x, q.y - p.y);
            }
        }
    }
    return length;
}

bool onRectangleRim(const Point3& p, double x0, double y0, double x1, double y1)
{
    constexpr double near = 1e-9;
    const bool alongX = p.x >= x0 - near && p.x <= x1 + near;
    const bool alongY = p.y >= y0 - near && p.y <= y1 + near;
    return (alongY && (std::abs(p.x - x0) <= near || std::abs(p.x - x1) <= near)) ||
           (alongX && (std::abs(p.y - y0) <= near || std::abs(p.y - y1) <= near));
}

} // namespace

TEST(TinEditCombine, APastedPadReplacesTheGroundInsideItsFootprintWithACleanSeam)
{
    // Ground: [0,100]^2 at 10 in 10 m cells. Pad: [33,67] x [37,63] at 15,
    // 2 x 2 cells, its rim off the ground's grid lines so the seam has to cut
    // the ground's triangles.
    const TinSurface ground = grid(0, 0, 10, 10, 10, 10, 10.0);
    const TinSurface pad = grid(33, 37, 2, 2, 17, 13, 15.0);
    const auto pasted = katana::terrain::pasteSurface(ground, pad);
    ASSERT_TRUE(pasted.ok()) << pasted.error().describe();
    const TinSurface& result = pasted->surface;

    // No gap and no overlap: the plan area is the ground's.
    EXPECT_NEAR(result.planArea(), 10000.0, 1e-6);
    // The pad inside its footprint, the ground well away from it.
    for (double x = 34; x < 67; x += 2) {
        for (double y = 38; y < 63; y += 2) {
            EXPECT_DOUBLE_EQ(*result.elevationAt(x, y), 15.0) << "at " << x << ", " << y;
        }
    }
    EXPECT_DOUBLE_EQ(*result.elevationAt(5, 5), 10.0);
    EXPECT_DOUBLE_EQ(*result.elevationAt(95, 95), 10.0);
    // The seam is the pad's perimeter, 2 (34 + 26) = 120 m, made constrained,
    // and every vertex on it lies on the pad's rim at the pad's level.
    EXPECT_NEAR(constrainedLength(result), 120.0, 1e-9);
    const auto seam = constrainedEnds(result);
    ASSERT_FALSE(seam.empty());
    for (const Point3& v : seam) {
        EXPECT_TRUE(onRectangleRim(v, 33, 37, 67, 63)) << "seam vertex at " << v.x << ", " << v.y;
        EXPECT_DOUBLE_EQ(v.z, 15.0);
    }
    // Nothing but the two levels: the ground's vertices keep theirs.
    for (const Point3& v : result.vertices()) {
        EXPECT_TRUE(v.z == 10.0 || v.z == 15.0) << "vertex at " << v.x << ", " << v.y;
    }
    EXPECT_EQ(pasted->report.trianglesBefore, 200u);
}

TEST(TinEditCombine, APastedTopIsKeptWhereItReachesPastTheBase)
{
    // The top [80,120] x [40,60] at 12 hangs 20 m past the base's edge at
    // x = 100: the union is 10000 + 20 x 20 = 10400.
    const TinSurface base = grid(0, 0, 10, 10, 10, 10, 10.0);
    const TinSurface top = grid(80, 40, 2, 2, 20, 10, 12.0);
    const auto pasted = katana::terrain::pasteSurface(base, top);
    ASSERT_TRUE(pasted.ok()) << pasted.error().describe();
    EXPECT_NEAR(pasted->surface.planArea(), 10400.0, 1e-6);
    EXPECT_DOUBLE_EQ(*pasted->surface.elevationAt(110, 50), 12.0);
    EXPECT_DOUBLE_EQ(*pasted->surface.elevationAt(90, 50), 12.0);
    EXPECT_DOUBLE_EQ(*pasted->surface.elevationAt(50, 50), 10.0);
}

TEST(TinEditCombine, PastingAnEmptySurfaceIsRefused)
{
    const TinSurface base = grid(0, 0, 2, 2, 10, 10, 10.0);
    EXPECT_EQ(katana::terrain::pasteSurface(base, TinSurface{}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::pasteSurface(TinSurface{}, base).error().code,
              ErrorCode::InvalidArgument);
}

TEST(TinEditCombine, AClipIsTheExactPartInsideThePolygon)
{
    // The triangle (1,1) (9,1) (5,8) out of a 10 x 10 grid: area 8 x 7 / 2 = 28.
    const TinSurface surface = grid(0, 0, 10, 10, 1, 1, 3.0);
    Polyline2 polygon;
    polygon.vertices = {Point2(1, 1), Point2(9, 1), Point2(5, 8)};
    const auto clipped = katana::terrain::clipToBoundary(surface, polygon);
    ASSERT_TRUE(clipped.ok()) << clipped.error().describe();
    EXPECT_NEAR(clipped->surface.planArea(), 28.0, 1e-9);
    EXPECT_DOUBLE_EQ(*clipped->surface.elevationAt(5, 3), 3.0);
    EXPECT_FALSE(clipped->surface.elevationAt(1, 7).has_value());
}

TEST(TinEditCombine, AnOffsetCopyIsRaisedByExactlyDz)
{
    const TinSurface surface = grid(0, 0, 3, 3, 2, 2, 7.25);
    const auto offset = katana::terrain::offsetSurface(surface, -2.5);
    ASSERT_TRUE(offset.ok()) << offset.error().describe();
    ASSERT_EQ(offset->surface.vertexCount(), surface.vertexCount());
    for (std::size_t v = 0; v < surface.vertexCount(); ++v) {
        EXPECT_EQ(offset->surface.vertices()[v].z, surface.vertices()[v].z - 2.5);
    }
    EXPECT_EQ(offset->surface.triangles(), surface.triangles());
}

TEST(TinEditCombine, AMergeLetsTheLaterSurfaceWin)
{
    // combineSurfaces' rule: where the pad can give a level, the pad's.
    const TinSurface ground = grid(0, 0, 10, 10, 10, 10, 10.0);
    const TinSurface pad = grid(40, 40, 4, 4, 5, 5, 15.0);
    const auto merged = katana::terrain::mergeSurfaces({&ground, &pad});
    ASSERT_TRUE(merged.ok()) << merged.error().describe();
    EXPECT_DOUBLE_EQ(*merged->surface.elevationAt(50, 50), 15.0);
    EXPECT_DOUBLE_EQ(*merged->surface.elevationAt(5, 5), 10.0);
    // The same answer combineSurfaces gives wherever either says anything.
    const auto direct = katana::terrain::combineSurfaces({&ground, &pad});
    ASSERT_TRUE(direct.ok());
    for (double x = 2.5; x < 100; x += 5) {
        for (double y = 2.5; y < 100; y += 5) {
            EXPECT_EQ(merged->surface.elevationAt(x, y), direct->elevationAt(x, y));
        }
    }
    EXPECT_EQ(merged->report.trianglesBefore, ground.triangleCount() + pad.triangleCount());
}

TEST(TinEditCombine, AMergeOfFewerThanTwoSurfacesOrAnEmptyOneIsRefused)
{
    const TinSurface ground = grid(0, 0, 2, 2, 10, 10, 10.0);
    const TinSurface empty;
    EXPECT_EQ(katana::terrain::mergeSurfaces({&ground}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(katana::terrain::mergeSurfaces({&ground, &empty}).error().code,
              ErrorCode::InvalidArgument);
}
