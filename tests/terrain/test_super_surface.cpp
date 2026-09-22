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
