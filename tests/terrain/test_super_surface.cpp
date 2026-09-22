// Combining surfaces in rank order (PLAN.MD 20.2, slice 8): a 12d super tin.
//
// The surfaces here are GRIDS rather than a sheet of two huge triangles,
// because that is what a super tin is made of in practice - a survey tin of
// thousands of small triangles with a design pad over it - and the rule
// these tests pin (a triangle of a lower member is dropped whole if a higher
// member covers any of it) only behaves well when the pad is larger than the
// triangles it covers. A two-triangle base would be removed entirely by a
// pad in the middle of it, which is true of the rule and says nothing about
// real data.
#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <vector>

#include "katana/terrain/super_surface.hpp"

using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::terrain::combineSurfaces;
using katana::terrain::TinSurface;
using katana::terrain::TinTriangle;

namespace {

// A flat grid at `level` over [x0,x1] x [y0,y1], `cells` squares each way,
// every square split into two triangles.
TinSurface grid(double x0, double y0, double x1, double y1, double level, int cells)
{
    std::vector<Point3> vertices;
    const double dx = (x1 - x0) / cells;
    const double dy = (y1 - y0) / cells;
    for (int j = 0; j <= cells; ++j) {
        for (int i = 0; i <= cells; ++i) {
            vertices.push_back(Point3{x0 + i * dx, y0 + j * dy, level});
        }
    }
    const auto index = [cells](int i, int j) {
        return static_cast<std::uint32_t>(j * (cells + 1) + i);
    };
    std::vector<TinTriangle> triangles;
    for (int j = 0; j < cells; ++j) {
        for (int i = 0; i < cells; ++i) {
            triangles.push_back({index(i, j), index(i + 1, j), index(i + 1, j + 1)});
            triangles.push_back({index(i, j), index(i + 1, j + 1), index(i, j + 1)});
        }
    }
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

} // namespace

TEST(SuperSurface, APadSubstitutesTheBaseOverItsOwnFootprintAndNowhereElse)
{
    // A 100 x 100 natural surface at 10 in 10 m triangles, with a 20 x 20
    // pad at 15 in the middle of it - the sample archive's "COMBINED
    // SURFACE", which names the base first.
    const TinSurface base = grid(0, 0, 100, 100, 10.0, 10);
    const TinSurface pad = grid(40, 40, 60, 60, 15.0, 2);
    const auto combined = combineSurfaces({&base, &pad});
    ASSERT_TRUE(combined.ok()) << (combined.ok() ? "" : combined.error().describe());

    EXPECT_EQ(combined->elevationAt(Point2(50.0, 50.0)), 15.0) << "inside the pad, the pad";
    EXPECT_EQ(combined->elevationAt(Point2(45.0, 55.0)), 15.0);
    EXPECT_EQ(combined->elevationAt(Point2(5.0, 5.0)), 10.0) << "well away from it, the base";
    EXPECT_EQ(combined->elevationAt(Point2(95.0, 95.0)), 10.0);
    EXPECT_EQ(combined->elevationAt(Point2(50.0, 5.0)), 10.0);
    // Off both of them there is no answer at all, rather than a zero.
    EXPECT_FALSE(combined->elevationAt(Point2(-5.0, -5.0)).has_value());
}

TEST(SuperSurface, NoPointEverHasTwoAnswersWhichIsTheWholePointOfTheRule)
{
    // The defect this rule exists to prevent: two sheets over one point make
    // elevationAt() answer with whichever triangle it meets first, which is
    // the base as often as the pad. Everywhere the pad covers, the combined
    // surface must give the PAD's level and never the base's.
    // The pad does NOT line up with the base's grid: 45 to 55 over 10 m
    // triangles, so a base triangle like (40,40)-(50,40)-(50,50) has its
    // middle outside the pad while part of it lies within. Dropping only
    // the triangles whose CENTRE is covered would leave that one, and the
    // base would answer at (48, 46) - inside the pad.
    const TinSurface base = grid(0, 0, 100, 100, 10.0, 10);
    const TinSurface pad = grid(45, 45, 55, 55, 15.0, 1);
    const auto combined = combineSurfaces({&base, &pad});
    ASSERT_TRUE(combined.ok());

    int insidePad = 0;
    for (double x = 45.5; x < 55.0; x += 0.5) {
        for (double y = 45.5; y < 55.0; y += 0.5) {
            const auto level = combined->elevationAt(Point2(x, y));
            ASSERT_TRUE(level.has_value()) << x << "," << y;
            EXPECT_EQ(*level, 15.0) << "the base answered at " << x << "," << y;
            ++insidePad;
        }
    }
    EXPECT_GT(insidePad, 300) << "the sweep must actually reach inside the pad";
}

TEST(SuperSurface, TheLastMemberWinsWhichIsWhatTheOrderMeans)
{
    // The comparison to reverse if 12d Model turns out to rank the list the
    // other way: the manual does not say, and this follows the sample
    // archive's own comment ("base surface first, then the pads that
    // substitute it over their footprints").
    const TinSurface first = grid(0, 0, 10, 10, 1.0, 2);
    const TinSurface second = grid(0, 0, 10, 10, 2.0, 2);
    const auto later = combineSurfaces({&first, &second});
    ASSERT_TRUE(later.ok());
    EXPECT_EQ(later->elevationAt(Point2(5.0, 5.0)), 2.0);

    const auto reversed = combineSurfaces({&second, &first});
    ASSERT_TRUE(reversed.ok());
    EXPECT_EQ(reversed->elevationAt(Point2(5.0, 5.0)), 1.0);
}

TEST(SuperSurface, TheSeamIsRaggedOutwardByAtMostOneTriangleOfTheLowerMember)
{
    // The documented approximation, pinned so that an exact clip later is a
    // deliberate change. A base triangle that the pad only touches goes with
    // the rest, so a ring of ground just outside the pad has no level -
    // which is honest, where a wrong level would not be.
    const TinSurface base = grid(0, 0, 100, 100, 10.0, 10);
    const TinSurface pad = grid(45, 45, 55, 55, 15.0, 1);
    const auto combined = combineSurfaces({&base, &pad});
    ASSERT_TRUE(combined.ok());

    EXPECT_EQ(combined->elevationAt(Point2(50.0, 50.0)), 15.0);
    // The base's 10 m triangles touching the pad went with it: the hole
    // reaches the grid line at 40, one triangle outside the pad's 45.
    EXPECT_FALSE(combined->elevationAt(Point2(44.0, 50.0)).has_value())
        << "inside the dropped base triangle, outside the pad";
    EXPECT_EQ(combined->elevationAt(Point2(35.0, 50.0)), 10.0)
        << "one triangle further out the base is untouched";
}

TEST(SuperSurface, EmptyAndMissingMembersAreNotAnError)
{
    // A super tin whose member tins the archive did not carry is a fact
    // about the file, not a failure of the reader.
    const auto none = combineSurfaces({});
    ASSERT_TRUE(none.ok());
    EXPECT_TRUE(none->empty());

    const TinSurface base = grid(0, 0, 10, 10, 1.0, 2);
    const TinSurface nothing;
    const auto some = combineSurfaces({nullptr, &base, &nothing});
    ASSERT_TRUE(some.ok());
    EXPECT_EQ(some->triangleCount(), base.triangleCount());
    EXPECT_EQ(some->elevationAt(Point2(5.0, 5.0)), 1.0);
}

TEST(SuperSurface, CombiningIsPureAndLeavesTheMembersAsTheyWere)
{
    const TinSurface base = grid(0, 0, 100, 100, 10.0, 10);
    const TinSurface pad = grid(40, 40, 60, 60, 15.0, 2);
    const std::size_t before = base.triangleCount();
    const auto combined = combineSurfaces({&base, &pad});
    ASSERT_TRUE(combined.ok());
    EXPECT_EQ(base.triangleCount(), before);
    EXPECT_EQ(base.elevationAt(Point2(50.0, 50.0)), 10.0)
        << "the base still answers for its own ground";
}

// ---- the vertex test is indexed, and must answer what the scan answered -----

namespace {

// The "is any vertex of the higher member inside this triangle" predicate,
// stated here the way super_surface.cpp stated it before the bucket grid went
// in: every vertex, no index, no bounding box around the triangle. This is the
// exhaustive run the culled one has to agree with (CLAUDE.md section 4).
bool vertexInsideExhaustively(const TinSurface& higher, const Point3& a, const Point3& b,
                              const Point3& c)
{
    const auto side = [](const Point3& from, const Point3& to, const Point3& p) {
        return (to.x - from.x) * (p.y - from.y) - (to.y - from.y) * (p.x - from.x);
    };
    for (const Point3& vertex : higher.vertices()) {
        const double ab = side(a, b, vertex);
        const double bc = side(b, c, vertex);
        const double ca = side(c, a, vertex);
        if ((ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0)) {
            return true;
        }
    }
    return false;
}

// The other half of the predicate: the triangle's own corners and middle
// against the higher surface.
bool sampleInsideExhaustively(const TinSurface& higher, const Point3& a, const Point3& b,
                              const Point3& c)
{
    const Point2 samples[] = {Point2(a.x, a.y), Point2(b.x, b.y), Point2(c.x, c.y),
                              Point2((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0)};
    for (const Point2& sample : samples) {
        if (higher.elevationAt(sample).has_value()) {
            return true;
        }
    }
    return false;
}

// A band two vertices deep: `columns` + 1 across, 2 up. Coarse across and thin
// up the page, which is what puts a lone vertex of it inside a base triangle,
// hard against that triangle's bounding box, where a query box even slightly
// too small would lose it.
TinSurface band(double x0, double y0, double x1, double y1, double level, int columns)
{
    std::vector<Point3> vertices;
    const double dx = (x1 - x0) / columns;
    for (int j = 0; j < 2; ++j) {
        for (int i = 0; i <= columns; ++i) {
            vertices.push_back(Point3{x0 + i * dx, j == 0 ? y0 : y1, level});
        }
    }
    std::vector<TinTriangle> triangles;
    for (int i = 0; i < columns; ++i) {
        const auto lower = static_cast<std::uint32_t>(i);
        const auto upper = static_cast<std::uint32_t>(columns + 1 + i);
        triangles.push_back({lower, lower + 1, upper + 1});
        triangles.push_back({lower, upper + 1, upper});
    }
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

bool coversExhaustively(const TinSurface& higher, const Point3& a, const Point3& b,
                        const Point3& c)
{
    const katana::geometry::Box2 box = higher.bounds();
    const double minX = std::min({a.x, b.x, c.x});
    const double maxX = std::max({a.x, b.x, c.x});
    const double minY = std::min({a.y, b.y, c.y});
    const double maxY = std::max({a.y, b.y, c.y});
    if (box.empty() || maxX < box.min.x || minX > box.max.x || maxY < box.min.y ||
        minY > box.max.y) {
        return false;
    }
    return sampleInsideExhaustively(higher, a, b, c) || vertexInsideExhaustively(higher, a, b, c);
}

} // namespace

TEST(SuperSurface, TheIndexedVertexTestKeepsExactlyTheTrianglesTheExhaustiveScanKept)
{
    // The pads are FINE grids over a coarse base. That is what makes this test
    // sharp: the bucket grid sizes its cells for about one vertex each, so a
    // 700-vertex pad over 30 m has cells near a metre, and a query box even
    // slightly too small loses vertices the scan would have found. A pad of
    // three cells would hide any such mistake behind cells wider than the base
    // triangles.
    //
    // One pad is placed off the base's 10 m grid lines, so its vertices fall in
    // the middle of base triangles; one is placed exactly ON them, which is the
    // case where the sign tests read zero and the boundary counts as inside.
    const TinSurface base = grid(0, 0, 100, 100, 10.0, 10);
    const TinSurface offGrid = grid(33.3, 41.7, 63.3, 71.7, 15.0, 25);
    const TinSurface onGrid = grid(10, 10, 40, 40, 12.0, 20);
    // The knife edge. This band crosses the base's triangles between y = 47 and
    // y = 48, missing every corner and every centroid of them, and its vertices
    // are 10 m apart across - so a base triangle like (40,40)-(50,50)-(40,50)
    // is decided by ONE pair of band vertices, at x = 40, exactly on that
    // triangle's own bounding box. Nothing but the vertex test answers there,
    // and it answers only if the query box really does reach the box's edge.
    const TinSurface knifeEdge = band(0, 47, 100, 48, 20.0, 10);
    const std::vector<const TinSurface*> members{&base, &offGrid, &onGrid, &knifeEdge};

    const auto combined = combineSurfaces(members);
    ASSERT_TRUE(combined.ok()) << combined.error().describe();

    std::vector<std::array<Point3, 3>> expected;
    std::size_t dropped = 0;
    std::size_t droppedByTheVertexTestAlone = 0;
    for (std::size_t rank = 0; rank < members.size(); ++rank) {
        const TinSurface& member = *members[rank];
        for (std::size_t t = 0; t < member.triangleCount(); ++t) {
            const TinTriangle& triangle = member.triangles()[t];
            const Point3& a = member.vertices()[triangle[0]];
            const Point3& b = member.vertices()[triangle[1]];
            const Point3& c = member.vertices()[triangle[2]];
            bool overridden = false;
            for (std::size_t higher = rank + 1; higher < members.size(); ++higher) {
                if (!coversExhaustively(*members[higher], a, b, c)) {
                    continue;
                }
                overridden = true;
                if (!sampleInsideExhaustively(*members[higher], a, b, c)) {
                    ++droppedByTheVertexTestAlone;
                }
                break;
            }
            if (overridden) {
                ++dropped;
            } else {
                expected.push_back({a, b, c});
            }
        }
    }

    // A cull that never fires proves nothing, and neither does one that only
    // the cheap sample test ever answers: the grid replaces the vertex scan, so
    // triangles must exist that ONLY the vertex scan rejects.
    EXPECT_GT(dropped, 20u);
    EXPECT_GT(droppedByTheVertexTestAlone, 5u);

    ASSERT_EQ(combined->triangleCount(), expected.size());
    for (std::size_t t = 0; t < expected.size(); ++t) {
        const TinTriangle& triangle = combined->triangles()[t];
        for (std::size_t k = 0; k < 3; ++k) {
            const Point3& got = combined->vertices()[triangle[k]];
            const Point3& want = expected[t][k];
            EXPECT_EQ(got.x, want.x) << "triangle " << t << " corner " << k;
            EXPECT_EQ(got.y, want.y) << "triangle " << t << " corner " << k;
            EXPECT_EQ(got.z, want.z) << "triangle " << t << " corner " << k;
        }
    }
}

TEST(SuperSurface, AVertexOnTheFarSideOfThePadFromTheTriangleIsNotMistakenForOneInside)
{
    // The grid query is a box around the triangle; a pad whose vertices are all
    // far outside that box must leave the triangle alone even though the pad's
    // whole-surface bounding box overlaps the base. A pad along one edge of the
    // base tests exactly that: the base triangles at the opposite edge share the
    // pad's bounding box in one axis and nothing else.
    const TinSurface base = grid(0, 0, 100, 100, 10.0, 10);
    const TinSurface strip = grid(0, 0, 100, 10, 15.0, 10);
    const auto combined = combineSurfaces({&base, &strip});
    ASSERT_TRUE(combined.ok()) << combined.error().describe();

    EXPECT_EQ(combined->elevationAt(Point2(50.0, 5.0)), 15.0) << "the strip owns the bottom band";
    EXPECT_EQ(combined->elevationAt(Point2(50.0, 95.0)), 10.0)
        << "and the base still owns the far edge";
}
