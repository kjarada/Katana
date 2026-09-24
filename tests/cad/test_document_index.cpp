// The Document's spatial index after BULK changes.
//
// The index is kept in step with the model incrementally, one entity at a
// time, which is right for a click and wrong for an import: an index that
// starts empty keeps its default one-unit cell, so a 12 km survey imported
// into it was filed into 722,715 buckets with 4,889 boxes on the oversized
// list every query scans, the import's execute took over a second, and
// queries stayed 36x slower until the project was reopened - because opening
// is what rebuilt the index and chose a cell size from the data.
//
// The rule these tests pin down (Document::applyToSpatialIndex):
//   * a command that touches at least a tenth of the drawing rebuilds it;
//   * so does a drawing that has grown to twice the size its cell was chosen
//     for - which includes the empty drawing, whose cell was chosen for none;
//   * anything else - the ordinary click - is incremental.
//
// The cell sizes below are worked out by hand from SpatialIndex's rule: the
// cell is twice the mean box side, (sum of width + height) / (2 n) * 2.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <memory>
#include <system_error>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/spatial_query.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"

namespace cmd = katana::commands;
namespace fs = std::filesystem;
using katana::cad::Document;
using katana::entity::Entity;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

// `count` horizontal lines of `length`, one above the other, 1 unit apart.
// Each box is `length` wide and 0 high, so it adds exactly `length` to the
// sum the cell size is taken from.
std::vector<Entity> horizontalLines(int count, double length, double y0 = 0.0)
{
    std::vector<Entity> entities;
    for (int i = 0; i < count; ++i) {
        const double y = y0 + static_cast<double>(i);
        Entity entity;
        entity.geometry = Segment2(Point2(0.0, y), Point2(length, y));
        entities.push_back(std::move(entity));
    }
    return entities;
}

// What a file import executes: one transaction holding one bulk create.
void import(Document& document, std::vector<Entity> entities)
{
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
    transaction->add(cmd::createEntities(std::move(entities)));
    const auto status = document.execute(std::move(transaction));
    ASSERT_TRUE(status.ok()) << status.error().describe();
}

} // namespace

TEST(DocumentIndex, AnImportIntoAnEmptyDrawingChoosesTheCellSizeFromWhatItImported)
{
    Document document;
    // (0,0)-(10,0) has a box 10 x 0 and (0,0)-(0,30) one 0 x 30:
    // sum of sides 10 + 30 = 40 over 2 boxes, mean side 40 / (2 * 2) = 10,
    // cell 2 * 10 = 20. Inserted one at a time, the index kept the default 1.
    std::vector<Entity> entities(2);
    entities[0].geometry = Segment2(Point2(0.0, 0.0), Point2(10.0, 0.0));
    entities[1].geometry = Segment2(Point2(0.0, 0.0), Point2(0.0, 30.0));
    import(document, std::move(entities));

    EXPECT_EQ(document.spatialIndex().cellSize(), 20.0);
    EXPECT_EQ(document.spatialIndex().size(), 2u);
}

TEST(DocumentIndex, AChangeToATenthOfTheDrawingRebuildsTheIndexAndASmallerOneDoesNot)
{
    Document document;
    // 100 lines 10 long: mean side 100 * 10 / (2 * 100) = 5, cell 10.
    import(document, horizontalLines(100, 10.0));
    ASSERT_EQ(document.spatialIndex().cellSize(), 10.0);

    // 5 more, 1000 long: 5 is under a tenth of the 105 after, and 105 is
    // under twice the 100 the cell was chosen for, so they are inserted into
    // the grid as it is and the cell stays 10.
    import(document, horizontalLines(5, 1000.0, 200.0));
    EXPECT_EQ(document.spatialIndex().cellSize(), 10.0);
    EXPECT_EQ(document.spatialIndex().size(), 105u);

    // 20 more: 20 is at least a tenth of the 125 after, so the index is
    // rebuilt. Sum of sides 100 * 10 + 25 * 1000 = 26000 over 125 boxes,
    // mean side 26000 / 250 = 104, cell 208.
    import(document, horizontalLines(20, 1000.0, 300.0));
    EXPECT_EQ(document.spatialIndex().cellSize(), 208.0);
    EXPECT_EQ(document.spatialIndex().size(), 125u);
}

TEST(DocumentIndex, ADrawingThatDoublesOneEntityAtATimeHasItsCellChosenAgain)
{
    Document document;
    // 10 lines 10 long: cell 10, chosen for 10 entities.
    import(document, horizontalLines(10, 10.0));
    ASSERT_EQ(document.spatialIndex().cellSize(), 10.0);

    // Lines 30 long, one command each. The k-th leaves 10 + k entities, and 1
    // is never a tenth of that, so only the doubling rule can fire: at 20.
    for (int k = 1; k <= 9; ++k) {
        const double y = 100.0 + static_cast<double>(k);
        ASSERT_TRUE(document.execute(cmd::createLine(Point2(0.0, y), Point2(30.0, y))).ok());
    }
    EXPECT_EQ(document.spatialIndex().cellSize(), 10.0) << "19 entities: still incremental";

    ASSERT_TRUE(document.execute(cmd::createLine(Point2(0.0, 110.0), Point2(30.0, 110.0))).ok());
    // Sum of sides 10 * 10 + 10 * 30 = 400 over 20 boxes, mean 400 / 40 = 10,
    // cell 20.
    EXPECT_EQ(document.spatialIndex().cellSize(), 20.0) << "20 entities: rebuilt";
    EXPECT_EQ(document.spatialIndex().size(), 20u);
}

TEST(DocumentIndex, OneLineDrawnIntoALargeDrawingIsInsertedNotRebuilt)
{
    Document document;
    import(document, horizontalLines(1000, 10.0));
    ASSERT_EQ(document.spatialIndex().cellSize(), 10.0);

    // A line 1000 long would move the mean if the index were rebuilt:
    // (1000 * 10 + 1000) / (2 * 1001) is about 5.49, cell about 10.99.
    ASSERT_TRUE(
        document.execute(cmd::createLine(Point2(0.0, 5000.0), Point2(1000.0, 5000.0))).ok());
    EXPECT_EQ(document.spatialIndex().cellSize(), 10.0);
    EXPECT_EQ(document.spatialIndex().size(), 1001u);
    // Inserted, and found.
    const auto found =
        document.spatialIndex().query(Box2(Point2(499.0, 4999.0), Point2(501.0, 5001.0)));
    EXPECT_EQ(found.size(), 1u);
}

TEST(DocumentIndex, UndoingAnImportChoosesTheCellAgainForWhatRemains)
{
    Document document;
    import(document, horizontalLines(100, 10.0));
    ASSERT_EQ(document.spatialIndex().cellSize(), 10.0);

    // 100 lines 1000 long: sum 100 * 10 + 100 * 1000 = 101000 over 200 boxes,
    // mean 101000 / 400 = 252.5, cell 505.
    import(document, horizontalLines(100, 1000.0, 200.0));
    EXPECT_EQ(document.spatialIndex().cellSize(), 505.0);

    // The undo removes 100 of 200: back to the first 100, cell 10 again.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.spatialIndex().cellSize(), 10.0);
    EXPECT_EQ(document.spatialIndex().size(), 100u);
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(document.spatialIndex().cellSize(), 505.0);
    EXPECT_EQ(document.spatialIndex().size(), 200u);
}

TEST(DocumentIndex, AnImportedDrawingHasTheSameIndexAsTheSameDrawingReopened)
{
    // A survey-like mix: points, short strings and a few long ones spread
    // over 2 km. Inserted one by one into the default cell, the long strings
    // are what overflowed onto the oversized list.
    std::vector<Entity> entities;
    for (int i = 0; i < 3000; ++i) {
        const double t = static_cast<double>(i);
        const double x = std::fmod(t * 37.0, 2000.0);
        const double y = std::fmod(t * 91.0, 1500.0);
        Entity entity;
        if (i % 2 == 0) {
            entity.geometry = katana::entity::PointGeometry{Point2(x, y)};
        } else {
            const double step = (0.25 + std::fmod(t * 0.731, 4.0)) * (i % 32 == 1 ? 25.0 : 1.0);
            Polyline2 polyline;
            for (int v = 0; v < 12; ++v) {
                polyline.vertices.emplace_back(x + v * step, y + std::sin(t + v) * step * 0.5);
            }
            entity.geometry = std::move(polyline);
        }
        entities.push_back(std::move(entity));
    }

    const fs::path directory =
        fs::temp_directory_path() / "katana-cad-tests-document-index" / "imported.katana";
    fs::remove_all(directory.parent_path());

    // Scoped: an open project holds its database file until its Document
    // goes, and Windows will not remove a file that is open.
    {
        Document imported;
        import(imported, std::move(entities));
        ASSERT_TRUE(imported.saveAs(directory).ok());

        Document reopened;
        const auto opened = reopened.open(directory);
        ASSERT_TRUE(opened.ok()) << opened.error().describe();

        const auto& a = imported.spatialIndex();
        const auto& b = reopened.spatialIndex();
        // Exactly, not nearly: both come from one rebuild over the same boxes in
        // the same (ascending id) order, and the cell size is a sum over them.
        EXPECT_EQ(a.cellSize(), b.cellSize());
        EXPECT_EQ(a.oversizedCount(), b.oversizedCount());
        EXPECT_EQ(a.bucketCount(), b.bucketCount());
        EXPECT_EQ(a.size(), b.size());
        EXPECT_EQ(a.size(), 3000u);
        EXPECT_GT(a.cellSize(), 1.0)
            << "the cell was chosen from the data, not left at its default";

        // And it answers as a scan does.
        const Box2 window(Point2(900.0, 700.0), Point2(1100.0, 800.0));
        std::vector<katana::geometry::SpatialId> scanned;
        imported.model().entities.forEach([&](const Entity& entity) {
            if (katana::cad::detail::queryExtents(imported.model(), entity).intersects(window)) {
                scanned.push_back(entity.id);
            }
        });
        EXPECT_EQ(a.query(window), scanned);
        EXPECT_FALSE(scanned.empty());
    }

    std::error_code ignored;
    fs::remove_all(directory.parent_path(), ignored);
}
