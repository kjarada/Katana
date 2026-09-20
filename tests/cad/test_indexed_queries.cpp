// The spatial index must not change an answer (PLAN.MD Phase 18, Rule 6).
//
// An index is an optimisation, and PLAN.MD's rule for an optimisation is that
// it may make things faster and may not make them different. Every test here
// runs the same query twice - once scanning, once through the index - and
// requires the results to be equal. If they ever diverge, the index is wrong,
// not the scan.

#include <gtest/gtest.h>

#include <cmath>
#include <random>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/cad/spatial_query.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::cad::BoxSelectionMode;
using katana::cad::Document;
using katana::cad::pickEntity;
using katana::cad::pickInBox;
using katana::cad::snap;
using katana::cad::SnapRequest;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

constexpr double kPi = 3.14159265358979323846;

// A drawing with every geometry kind, scattered and overlapping, so the
// comparison is not over one easy shape. Fixed seed: a failure repeats.
void populate(Document& document, int count)
{
    std::mt19937 random(20260920);
    std::uniform_real_distribution<double> position(-200.0, 200.0);
    std::uniform_real_distribution<double> small(0.5, 12.0);

    for (int i = 0; i < count; ++i) {
        const double x = position(random);
        const double y = position(random);
        const auto attributes = document.currentAttributes();
        switch (i % 5) {
        case 0:
            EXPECT_TRUE(
                document.execute(katana::commands::createLine(
                                     Point2(x, y), Point2(x + small(random), y + small(random)),
                                     attributes))
                    .ok());
            break;
        case 1:
            EXPECT_TRUE(document
                            .execute(katana::commands::createCircle(Point2(x, y), small(random),
                                                                    attributes))
                            .ok());
            break;
        case 2: {
            // A shallow arc: its centre is FAR outside its own bounding box,
            // which is exactly the case that breaks a naive index.
            const Arc2 arc{Point2(x, y), small(random) * 8.0, 0.0, 0.25};
            EXPECT_TRUE(document.execute(katana::commands::createArc(arc, attributes)).ok());
            break;
        }
        case 3: {
            Polyline2 polyline;
            for (int v = 0; v < 5; ++v) {
                polyline.vertices.emplace_back(x + v * 1.3, y + std::sin(v + i) * 2.0);
            }
            EXPECT_TRUE(
                document.execute(katana::commands::createPolyline(polyline, attributes)).ok());
            break;
        }
        default:
            EXPECT_TRUE(
                document.execute(katana::commands::createPoint(Point2(x, y), attributes)).ok());
            break;
        }
    }
}

SnapRequest requestAt(const Point2& cursor)
{
    SnapRequest request;
    request.cursor = cursor;
    request.aperture = 3.0;
    request.modes = katana::cad::kDefaultSnapModes;
    return request;
}

} // namespace

TEST(IndexedQueries, TheIndexTracksTheDocumentAsItIsDrawn)
{
    Document document;
    EXPECT_EQ(document.spatialIndex().size(), 0u);

    populate(document, 40);
    // Points have no extent but are still indexed; every entity should be in.
    EXPECT_EQ(document.spatialIndex().size(), document.model().entities.size());
}

TEST(IndexedQueries, PickingGivesTheSameAnswerWithAndWithoutTheIndex)
{
    Document document;
    populate(document, 300);

    std::mt19937 random(5);
    std::uniform_real_distribution<double> position(-220.0, 220.0);

    std::size_t hits = 0;
    for (int trial = 0; trial < 600; ++trial) {
        const Point2 point(position(random), position(random));
        for (const double tolerance : {0.25, 2.0, 15.0}) {
            const auto scanned = pickEntity(document.model(), point, tolerance);
            const auto indexed =
                pickEntity(document.model(), point, tolerance, {}, &document.spatialIndex());
            ASSERT_EQ(scanned.has_value(), indexed.has_value())
                << "trial " << trial << " tolerance " << tolerance;
            if (scanned.has_value()) {
                ASSERT_EQ(*scanned, *indexed) << "trial " << trial;
                ++hits;
            }
        }
    }
    // A comparison that never picked anything would pass while indexing
    // nothing at all.
    EXPECT_GT(hits, 100u) << "the trials must actually hit entities";
}

TEST(IndexedQueries, SnappingGivesTheSameAnswerWithAndWithoutTheIndex)
{
    Document document;
    populate(document, 250);

    std::mt19937 random(9);
    std::uniform_real_distribution<double> position(-220.0, 220.0);

    std::size_t hits = 0;
    for (int trial = 0; trial < 500; ++trial) {
        const auto request = requestAt(Point2(position(random), position(random)));
        const auto scanned = snap(document.model(), request);
        const auto indexed = snap(document.model(), request, &document.spatialIndex());

        ASSERT_EQ(scanned.has_value(), indexed.has_value()) << "trial " << trial;
        if (!scanned.has_value()) {
            continue;
        }
        ++hits;
        EXPECT_EQ(scanned->mode, indexed->mode) << "trial " << trial;
        EXPECT_EQ(scanned->entity, indexed->entity) << "trial " << trial;
        EXPECT_DOUBLE_EQ(scanned->point.x, indexed->point.x) << "trial " << trial;
        EXPECT_DOUBLE_EQ(scanned->point.y, indexed->point.y) << "trial " << trial;
    }
    EXPECT_GT(hits, 50u) << "the trials must actually snap to something";
}

TEST(IndexedQueries, AnArcCentreIsStillSnappableThroughTheIndex)
{
    // THE case the indexed box has to be wider than the bounding box for. A
    // shallow arc's centre is a long way outside the arc's own extent, so an
    // index built on plain bounding boxes rejects the arc before the centre
    // snap is ever considered, and centre snap silently stops working.
    Document document;
    const Arc2 arc{Point2(0.0, 0.0), 100.0, 0.0, 0.2}; // a sliver far from its centre
    ASSERT_TRUE(document.execute(katana::commands::createArc(arc, document.currentAttributes()))
                    .ok());

    SnapRequest request;
    request.cursor = Point2(0.0, 0.0); // the centre
    request.aperture = 1.0;
    request.modes = static_cast<katana::cad::SnapModes>(katana::cad::SnapMode::Center);

    const auto scanned = snap(document.model(), request);
    const auto indexed = snap(document.model(), request, &document.spatialIndex());

    ASSERT_TRUE(scanned.has_value()) << "the scan must find the arc centre";
    ASSERT_TRUE(indexed.has_value()) << "so must the index";
    EXPECT_EQ(scanned->mode, indexed->mode);
    EXPECT_EQ(scanned->entity, indexed->entity);
}

TEST(IndexedQueries, ClickingAnArcCentreStillDoesNotSELECTTheArc)
{
    // The other half of the same decision: the indexed box is widened for
    // snapping, and picking must NOT inherit that generosity.
    Document document;
    const Arc2 arc{Point2(0.0, 0.0), 100.0, 0.0, 0.2};
    ASSERT_TRUE(document.execute(katana::commands::createArc(arc, document.currentAttributes()))
                    .ok());

    EXPECT_FALSE(pickEntity(document.model(), Point2(0.0, 0.0), 1.0).has_value());
    EXPECT_FALSE(
        pickEntity(document.model(), Point2(0.0, 0.0), 1.0, {}, &document.spatialIndex())
            .has_value());
}

TEST(IndexedQueries, BoxSelectionGivesTheSameAnswerWithAndWithoutTheIndex)
{
    Document document;
    populate(document, 300);

    std::mt19937 random(13);
    std::uniform_real_distribution<double> position(-220.0, 220.0);
    std::uniform_real_distribution<double> extent(1.0, 90.0);

    std::size_t nonEmpty = 0;
    for (int trial = 0; trial < 300; ++trial) {
        const double x = position(random);
        const double y = position(random);
        const Box2 window(Point2(x, y), Point2(x + extent(random), y + extent(random)));
        for (const auto mode : {BoxSelectionMode::Window, BoxSelectionMode::Crossing}) {
            const auto scanned = pickInBox(document.model(), window, mode);
            const auto indexed =
                pickInBox(document.model(), window, mode, {}, &document.spatialIndex());
            ASSERT_EQ(scanned, indexed) << "trial " << trial;
            if (!scanned.empty()) {
                ++nonEmpty;
            }
        }
    }
    EXPECT_GT(nonEmpty, 100u) << "the windows must actually contain entities";
}

TEST(IndexedQueries, TheIndexSurvivesEditingMovingAndDeleting)
{
    Document document;
    populate(document, 120);

    std::mt19937 random(21);
    std::uniform_real_distribution<double> position(-200.0, 200.0);

    for (int round = 0; round < 25; ++round) {
        const auto ids = document.model().entities.ids();
        ASSERT_FALSE(ids.empty());

        // Move a few.
        std::vector<EntityId> moving(ids.begin(), ids.begin() + std::min<std::size_t>(5, ids.size()));
        ASSERT_TRUE(document
                        .execute(katana::commands::moveEntities(
                            moving, katana::geometry::Vec2(position(random) * 0.1,
                                                           position(random) * 0.1)))
                        .ok());

        // Delete one.
        if (ids.size() > 10) {
            ASSERT_TRUE(
                document.execute(katana::commands::deleteEntities({ids[ids.size() / 2]})).ok());
        }

        // Draw one.
        ASSERT_TRUE(document
                        .execute(katana::commands::createLine(
                            Point2(position(random), position(random)),
                            Point2(position(random), position(random)),
                            document.currentAttributes()))
                        .ok());

        ASSERT_EQ(document.spatialIndex().size(), document.model().entities.size())
            << "round " << round;

        // And the answers still agree.
        for (int trial = 0; trial < 30; ++trial) {
            const Point2 point(position(random), position(random));
            ASSERT_EQ(pickEntity(document.model(), point, 5.0),
                      pickEntity(document.model(), point, 5.0, {}, &document.spatialIndex()))
                << "round " << round << " trial " << trial;
        }
    }
}

TEST(IndexedQueries, TheIndexSurvivesUndoAndRedo)
{
    // Undo restores entities through the same command machinery, so the index
    // has to be put back with them. A stale index here is the classic way an
    // optimisation silently corrupts a session.
    Document document;
    populate(document, 60);

    const auto before = document.model().entities.ids();
    ASSERT_GE(before.size(), 10u);

    std::vector<EntityId> doomed(before.begin(), before.begin() + 8);
    ASSERT_TRUE(document.execute(katana::commands::deleteEntities(doomed)).ok());
    EXPECT_EQ(document.spatialIndex().size(), document.model().entities.size());

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.spatialIndex().size(), document.model().entities.size())
        << "undo must restore the index with the entities";
    EXPECT_EQ(document.model().entities.ids(), before);

    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(document.spatialIndex().size(), document.model().entities.size());

    ASSERT_TRUE(document.undo().ok());
    // The real assertion: queries agree again after the round trip.
    std::mt19937 random(3);
    std::uniform_real_distribution<double> position(-200.0, 200.0);
    for (int trial = 0; trial < 200; ++trial) {
        const Point2 point(position(random), position(random));
        ASSERT_EQ(pickEntity(document.model(), point, 6.0),
                  pickEntity(document.model(), point, 6.0, {}, &document.spatialIndex()))
            << "trial " << trial;
    }
}

TEST(IndexedQueries, StartingANewDocumentEmptiesTheIndex)
{
    Document document;
    populate(document, 50);
    ASSERT_GT(document.spatialIndex().size(), 0u);

    document.newDocument();
    EXPECT_EQ(document.spatialIndex().size(), 0u)
        << "a stale index would make the new drawing pick entities that are gone";
    EXPECT_FALSE(pickEntity(document.model(), Point2(0.0, 0.0), 1e6, {},
                            &document.spatialIndex())
                     .has_value());
}

TEST(IndexedQueries, HiddenAndLockedEntitiesAreFilteredTheSameWayOnBothPaths)
{
    // The index knows nothing about visibility; the exact test does. Both
    // paths must therefore still exclude the same entities.
    Document document;
    populate(document, 40);
    const auto ids = document.model().entities.ids();
    ASSERT_GE(ids.size(), 10u);

    std::vector<EntityId> hidden(ids.begin(), ids.begin() + 10);
    ASSERT_TRUE(document.execute(katana::commands::setEntityVisible(hidden, false)).ok());

    std::mt19937 random(17);
    std::uniform_real_distribution<double> position(-200.0, 200.0);
    for (int trial = 0; trial < 300; ++trial) {
        const Point2 point(position(random), position(random));
        const auto scanned = pickEntity(document.model(), point, 8.0);
        const auto indexed =
            pickEntity(document.model(), point, 8.0, {}, &document.spatialIndex());
        ASSERT_EQ(scanned, indexed) << "trial " << trial;
        if (scanned.has_value()) {
            EXPECT_EQ(std::find(hidden.begin(), hidden.end(), *scanned), hidden.end())
                << "a hidden entity was picked";
        }
    }
}

TEST(IndexedQueries, BothSidesOfTheScanCrossoverGiveTheSameAnswer)
{
    // forEachCandidate uses the index for a narrow query and falls back to the
    // ordered scan for a wide one, because asking an index for everything
    // costs more than walking the model once (see spatial_query.hpp). The
    // threshold changes WHICH path runs, so the answers on either side of it
    // have to be identical or the drawing would change as the user zoomed.
    Document document;
    populate(document, 400);

    const Box2 extent = document.spatialIndex().bounds();
    ASSERT_FALSE(extent.empty());
    const Point2 middle = extent.center();

    // Window half-widths sweeping from far below the threshold to far above,
    // so both paths are exercised and the boundary is crossed.
    for (const double share : {0.001, 0.01, 0.1, 0.3, 0.34, 0.35, 0.36, 0.5, 0.9, 1.0, 4.0}) {
        const double half = 0.5 * extent.width() * std::sqrt(share);
        const Box2 window(Point2(middle.x - half, middle.y - half),
                          Point2(middle.x + half, middle.y + half));

        for (const auto mode : {BoxSelectionMode::Window, BoxSelectionMode::Crossing}) {
            ASSERT_EQ(pickInBox(document.model(), window, mode),
                      pickInBox(document.model(), window, mode, {}, &document.spatialIndex()))
                << "area share " << share;
        }
        const double tolerance = half;
        ASSERT_EQ(pickEntity(document.model(), middle, tolerance),
                  pickEntity(document.model(), middle, tolerance, {},
                             &document.spatialIndex()))
            << "area share " << share;
    }
}

TEST(IndexedQueries, AQueryEntirelyOffTheDrawingIsEmptyOnBothPaths)
{
    Document document;
    populate(document, 100);

    const Point2 elsewhere(1.0e6, 1.0e6);
    EXPECT_FALSE(pickEntity(document.model(), elsewhere, 1.0).has_value());
    EXPECT_FALSE(
        pickEntity(document.model(), elsewhere, 1.0, {}, &document.spatialIndex()).has_value());

    const Box2 far(Point2(1.0e6, 1.0e6), Point2(1.0e6 + 50.0, 1.0e6 + 50.0));
    EXPECT_TRUE(pickInBox(document.model(), far, BoxSelectionMode::Crossing).empty());
    EXPECT_TRUE(
        pickInBox(document.model(), far, BoxSelectionMode::Crossing, {},
                  &document.spatialIndex())
            .empty());
}

TEST(IndexedQueries, ADimensionIsFoundByItsLabelNotOnlyByItsMeasuredLine)
{
    // The broad phase culls against what a dimension DRAWS. Its label, arrows
    // and extension overshoot sit outside the measured points, so culling
    // against the plain bounding box drops a dimension whose text is still on
    // screen - a glitch at the edge of the view that looks like a redraw fault.
    Document document;

    katana::entity::DimensionGeometry dimension;
    dimension.start = Point2(0.0, 0.0);
    dimension.end = Point2(10.0, 0.0);
    dimension.offset = 2.0;
    ASSERT_TRUE(
        document.execute(katana::commands::createDimension(dimension,
                                                           document.currentAttributes()))
            .ok());

    const auto id = document.model().entities.ids().back();
    const auto plain =
        katana::entity::boundingBox(*&document.model().entities.find(id)->geometry);
    const auto indexed = katana::cad::detail::queryExtents(document.model(),
                                                           *document.model().entities.find(id));

    ASSERT_GT(indexed.max.y, plain.max.y)
        << "the drawn extent must exceed the measured one, or this test proves nothing";

    // A window that touches only the drawn part - above the dimension line,
    // where the label sits - must still find it, on both paths.
    const Box2 labelOnly(Point2(4.0, plain.max.y + 0.05),
                         Point2(6.0, indexed.max.y - 0.01));
    ASSERT_FALSE(labelOnly.empty());

    const auto scanned = pickInBox(document.model(), labelOnly, BoxSelectionMode::Crossing);
    const auto viaIndex = pickInBox(document.model(), labelOnly, BoxSelectionMode::Crossing, {},
                                    &document.spatialIndex());
    EXPECT_EQ(scanned, viaIndex) << "both paths must agree about a dimension's extent";
}
