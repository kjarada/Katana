// Broad-phase spatial index (PLAN.MD Phase 18).
//
// An index is only useful if it returns EXACTLY what the scan it replaces
// would have returned. The central test here is that equivalence, checked
// against brute force over awkward data: clustered, wildly varying in size,
// and edited between queries.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <random>
#include <vector>

#include "katana/geometry/spatial_index.hpp"

using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::SpatialEntry;
using katana::geometry::SpatialId;
using katana::geometry::SpatialIndex;

namespace {

Box2 boxAt(double x, double y, double w, double h)
{
    return Box2(Point2(x, y), Point2(x + w, y + h));
}

// What the index replaces: the linear scan, kept here as the oracle.
std::vector<SpatialId> bruteForce(const std::vector<SpatialEntry>& entries, const Box2& query)
{
    std::vector<SpatialId> out;
    for (const SpatialEntry& entry : entries) {
        if (!entry.box.empty() && entry.box.intersects(query)) {
            out.push_back(entry.id);
        }
    }
    std::sort(out.begin(), out.end());
    return out;
}

// Deliberately nasty: two dense clusters, a sparse scatter, a handful of huge
// boxes spanning everything, and some degenerate points. Fixed seed, so a
// failure is reproducible.
std::vector<SpatialEntry> awkwardData(std::size_t count)
{
    std::mt19937 random(20260920);
    std::uniform_real_distribution<double> spread(-500.0, 500.0);
    std::uniform_real_distribution<double> tight(0.0, 5.0);
    std::uniform_real_distribution<double> small(0.01, 2.0);

    std::vector<SpatialEntry> entries;
    entries.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        SpatialEntry entry;
        entry.id = static_cast<SpatialId>(i + 1);
        const std::size_t kind = i % 10;
        if (kind < 4) {
            entry.box = boxAt(tight(random), tight(random), small(random), small(random));
        } else if (kind < 7) {
            entry.box = boxAt(300.0 + tight(random), -200.0 + tight(random), small(random),
                              small(random));
        } else if (kind < 9) {
            entry.box = boxAt(spread(random), spread(random), small(random), small(random));
        } else if (i % 97 == 0) {
            entry.box = boxAt(spread(random), spread(random), 800.0, 800.0); // huge
        } else {
            const double x = spread(random);
            const double y = spread(random);
            entry.box = Box2(Point2(x, y), Point2(x, y)); // a point: zero extent
        }
        entries.push_back(entry);
    }
    return entries;
}

} // namespace

TEST(SpatialIndex, AnEmptyIndexAnswersNothing)
{
    const SpatialIndex index;
    EXPECT_TRUE(index.empty());
    EXPECT_EQ(index.size(), 0u);
    EXPECT_TRUE(index.query(boxAt(0, 0, 10, 10)).empty());
    EXPECT_TRUE(index.bounds().empty());
}

TEST(SpatialIndex, FindsWhatOverlapsAndOnlyThat)
{
    SpatialIndex index;
    index.insert(1, boxAt(0.0, 0.0, 10.0, 10.0));
    index.insert(2, boxAt(20.0, 20.0, 10.0, 10.0));
    index.insert(3, boxAt(5.0, 5.0, 10.0, 10.0));

    EXPECT_EQ(index.query(boxAt(1.0, 1.0, 1.0, 1.0)), (std::vector<SpatialId>{1}));
    EXPECT_EQ(index.query(boxAt(6.0, 6.0, 1.0, 1.0)), (std::vector<SpatialId>{1, 3}));
    EXPECT_EQ(index.query(boxAt(21.0, 21.0, 1.0, 1.0)), (std::vector<SpatialId>{2}));
    EXPECT_TRUE(index.query(boxAt(100.0, 100.0, 1.0, 1.0)).empty());
    // Touching counts as overlapping, matching Box2::intersects, because a
    // snap to a shared endpoint depends on it.
    EXPECT_EQ(index.query(boxAt(10.0, 10.0, 0.0, 0.0)), (std::vector<SpatialId>{1, 3}));
}

TEST(SpatialIndex, ResultsAreSortedSoTieBreaksAreMeaningful)
{
    // pickEntity prefers the higher id on a tie. That rule is only meaningful
    // if the candidate order does not depend on hash iteration order.
    SpatialIndex index;
    for (SpatialId id = 50; id >= 1; --id) {
        index.insert(id, boxAt(0.0, 0.0, 100.0, 100.0));
    }
    const auto found = index.query(boxAt(50.0, 50.0, 1.0, 1.0));
    ASSERT_EQ(found.size(), 50u);
    EXPECT_TRUE(std::is_sorted(found.begin(), found.end()));
    EXPECT_EQ(found.front(), 1u);
    EXPECT_EQ(found.back(), 50u);
}

TEST(SpatialIndex, AnEntitySpanningManyCellsIsReturnedOnceNotOncePerCell)
{
    SpatialIndex index;
    index.rebuild(std::vector<SpatialEntry>{{1, boxAt(0.0, 0.0, 1.0, 1.0)}});
    // One large box on a grid sized for small ones lands in many cells.
    index.insert(2, boxAt(-50.0, -50.0, 100.0, 100.0));

    const auto found = index.query(boxAt(0.0, 0.0, 30.0, 30.0));
    EXPECT_EQ(std::count(found.begin(), found.end(), 2u), 1)
        << "a box listed in several cells must not be reported several times";
}

TEST(SpatialIndex, AgreesWithBruteForceOnAwkwardData)
{
    // THE test. Clustered, mixed-size, with degenerate points and huge boxes.
    const auto entries = awkwardData(4000);
    SpatialIndex index;
    index.rebuild(entries);

    std::mt19937 random(7);
    std::uniform_real_distribution<double> position(-600.0, 600.0);
    std::uniform_real_distribution<double> extent(0.0, 120.0);

    std::size_t nonEmpty = 0;
    for (int trial = 0; trial < 400; ++trial) {
        const double x = position(random);
        const double y = position(random);
        const Box2 query = boxAt(x, y, extent(random), extent(random));

        const auto expected = bruteForce(entries, query);
        const auto actual = index.query(query);
        ASSERT_EQ(actual, expected) << "trial " << trial << " at " << x << "," << y;
        if (!expected.empty()) {
            ++nonEmpty;
        }
    }
    // A test that only ever queried empty space would pass while indexing
    // nothing, so assert the queries actually hit something.
    EXPECT_GT(nonEmpty, 200u) << "the queries must exercise real overlaps";
}

TEST(SpatialIndex, StillAgreesWithBruteForceAfterEdits)
{
    // A CAD document mutates constantly; an index that is only correct when
    // freshly built is no use.
    auto entries = awkwardData(1500);
    SpatialIndex index;
    index.rebuild(entries);

    std::mt19937 random(11);
    std::uniform_real_distribution<double> position(-500.0, 500.0);

    for (int round = 0; round < 40; ++round) {
        // Move some, delete some, add some.
        for (std::size_t i = 0; i < entries.size(); i += 37) {
            entries[i].box = boxAt(position(random), position(random), 3.0, 3.0);
            index.insert(entries[i].id, entries[i].box);
        }
        for (std::size_t i = 5; i < entries.size(); i += 211) {
            if (entries[i].id != 0) {
                EXPECT_TRUE(index.remove(entries[i].id) || entries[i].box.empty());
                entries[i].box = Box2{}; // brute force then skips it too
            }
        }
        const SpatialId fresh = static_cast<SpatialId>(100000 + round);
        const Box2 box = boxAt(position(random), position(random), 8.0, 8.0);
        index.insert(fresh, box);
        entries.push_back(SpatialEntry{fresh, box});

        const Box2 query = boxAt(position(random), position(random), 60.0, 60.0);
        ASSERT_EQ(index.query(query), bruteForce(entries, query)) << "round " << round;
    }
}

TEST(SpatialIndex, ReinsertingAnIdReplacesItRatherThanDuplicatingIt)
{
    SpatialIndex index;
    index.insert(1, boxAt(0.0, 0.0, 5.0, 5.0));
    index.insert(1, boxAt(100.0, 100.0, 5.0, 5.0));

    EXPECT_EQ(index.size(), 1u);
    EXPECT_TRUE(index.query(boxAt(1.0, 1.0, 1.0, 1.0)).empty())
        << "the old position must not linger";
    EXPECT_EQ(index.query(boxAt(101.0, 101.0, 1.0, 1.0)), (std::vector<SpatialId>{1}));
}

TEST(SpatialIndex, RemovingReportsWhetherAnythingWasThere)
{
    SpatialIndex index;
    index.insert(1, boxAt(0.0, 0.0, 5.0, 5.0));
    EXPECT_TRUE(index.contains(1));
    EXPECT_TRUE(index.remove(1));
    EXPECT_FALSE(index.contains(1));
    EXPECT_FALSE(index.remove(1)) << "removing twice must say so, not pretend";
    EXPECT_FALSE(index.remove(999));
    EXPECT_TRUE(index.empty());
}

TEST(SpatialIndex, DegenerateAndNonFiniteBoxesAreDroppedNotFiledSomewhereArbitrary)
{
    SpatialIndex index;
    index.insert(1, boxAt(0.0, 0.0, 5.0, 5.0));

    // A default Box2 is empty (inverted infinities), and a NaN corner has no
    // cell. Either filed under some arbitrary cell would surface in unrelated
    // queries.
    index.insert(2, Box2{});
    index.insert(3, Box2(Point2(std::nan(""), 0.0), Point2(1.0, 1.0)));
    index.insert(4, Box2(Point2(0.0, 0.0),
                         Point2(std::numeric_limits<double>::infinity(), 1.0)));

    EXPECT_EQ(index.size(), 1u);
    const auto everything = index.query(boxAt(-1e6, -1e6, 2e6, 2e6));
    EXPECT_EQ(everything, (std::vector<SpatialId>{1}));
}

TEST(SpatialIndex, ReplacingAGoodBoxWithABadOneRemovesTheEntry)
{
    SpatialIndex index;
    index.insert(1, boxAt(0.0, 0.0, 5.0, 5.0));
    index.insert(1, Box2{}); // the entity lost its geometry
    EXPECT_FALSE(index.contains(1));
    EXPECT_TRUE(index.query(boxAt(1.0, 1.0, 1.0, 1.0)).empty())
        << "the old cell entries must have gone with it";
}

TEST(SpatialIndex, AZeroExtentPointIsFoundAtItsPosition)
{
    // Survey point entities have no extent at all, and must still be pickable.
    SpatialIndex index;
    index.insert(1, Box2(Point2(10.0, 20.0), Point2(10.0, 20.0)));
    EXPECT_EQ(index.size(), 1u);
    EXPECT_EQ(index.query(boxAt(9.5, 19.5, 1.0, 1.0)), (std::vector<SpatialId>{1}));
    EXPECT_TRUE(index.query(boxAt(50.0, 50.0, 1.0, 1.0)).empty());
}

TEST(SpatialIndex, TheRadiusQueryIsTheBoxQueryOfThatRadius)
{
    SpatialIndex index;
    index.insert(1, boxAt(0.0, 0.0, 1.0, 1.0));
    index.insert(2, boxAt(10.0, 0.0, 1.0, 1.0));

    std::vector<SpatialId> out;
    index.query(Point2(0.5, 0.5), 0.5, out);
    EXPECT_EQ(out, (std::vector<SpatialId>{1}));

    index.query(Point2(0.5, 0.5), 20.0, out);
    EXPECT_EQ(out, (std::vector<SpatialId>{1, 2}));

    // A nonsense radius answers nothing rather than everything.
    index.query(Point2(0.5, 0.5), -1.0, out);
    EXPECT_TRUE(out.empty());
    index.query(Point2(0.5, 0.5), std::nan(""), out);
    EXPECT_TRUE(out.empty());
}

TEST(SpatialIndex, AQueryCoveringTheWholeDrawingStillReturnsEverythingExactly)
{
    // Above a cell budget the query falls back to scanning every box. That
    // path has to give the same answer as the cell walk, or a zoom-extents
    // selection would differ from a small one.
    const auto entries = awkwardData(2000);
    SpatialIndex index;
    index.rebuild(entries);

    const Box2 everything = boxAt(-1.0e6, -1.0e6, 2.0e6, 2.0e6);
    EXPECT_EQ(index.query(everything), bruteForce(entries, everything));
}

TEST(SpatialIndex, RebuildingIsEquivalentToInsertingOneAtATime)
{
    const auto entries = awkwardData(800);

    SpatialIndex rebuilt;
    rebuilt.rebuild(entries);

    SpatialIndex incremental;
    for (const SpatialEntry& entry : entries) {
        incremental.insert(entry.id, entry.box);
    }

    EXPECT_EQ(rebuilt.size(), incremental.size());
    // The cell sizes differ - rebuild() sees all the data and picks one, while
    // incremental insertion cannot - so the BUCKETS differ. The ANSWERS must
    // not, and that is what callers depend on.
    std::mt19937 random(3);
    std::uniform_real_distribution<double> position(-600.0, 600.0);
    for (int trial = 0; trial < 120; ++trial) {
        const Box2 query = boxAt(position(random), position(random), 40.0, 40.0);
        ASSERT_EQ(rebuilt.query(query), incremental.query(query)) << "trial " << trial;
    }
}

TEST(SpatialIndex, ClearLeavesItEmptyAndReusable)
{
    SpatialIndex index;
    index.rebuild(awkwardData(500));
    ASSERT_FALSE(index.empty());

    index.clear();
    EXPECT_TRUE(index.empty());
    EXPECT_EQ(index.size(), 0u);
    EXPECT_TRUE(index.query(boxAt(-1e6, -1e6, 2e6, 2e6)).empty())
        << "a cleared index must not answer from stale buckets";

    index.insert(1, boxAt(0.0, 0.0, 1.0, 1.0));
    EXPECT_EQ(index.query(boxAt(0.0, 0.0, 1.0, 1.0)), (std::vector<SpatialId>{1}));
}

TEST(SpatialIndex, BoundsCoverEverythingIndexed)
{
    SpatialIndex index;
    index.insert(1, boxAt(0.0, 0.0, 10.0, 10.0));
    index.insert(2, boxAt(-50.0, 20.0, 5.0, 5.0));

    const Box2 bounds = index.bounds();
    ASSERT_FALSE(bounds.empty());
    EXPECT_DOUBLE_EQ(bounds.min.x, -50.0);
    EXPECT_DOUBLE_EQ(bounds.min.y, 0.0);
    EXPECT_DOUBLE_EQ(bounds.max.x, 10.0);
    EXPECT_DOUBLE_EQ(bounds.max.y, 25.0);
}
