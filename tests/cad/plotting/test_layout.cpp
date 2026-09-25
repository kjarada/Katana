// Tiling presets, tiling ranks and snapping (include/katana/cad/plotting/layout.hpp).
//
// The expected rectangles are worked by hand from the A3 frame's drawing area
// (23..410 x 35..287 mm, resources/plot_frames/a3_landscape.json) inset by
// 1 mm - 24..409 x 36..286, 385 x 250 mm - and the owner's app's cell
// fractions with its 3 mm gutter.

#include <gtest/gtest.h>

#include <set>
#include <vector>

#include "katana/cad/plotting/layout.hpp"

using katana::cad::PaperSize;
using namespace katana::cad::plotting;

namespace {

void expectBox(const Box2& box, double x0, double y0, double x1, double y1)
{
    EXPECT_NEAR(box.min.x, x0, 1e-9);
    EXPECT_NEAR(box.min.y, y0, 1e-9);
    EXPECT_NEAR(box.max.x, x1, 1e-9);
    EXPECT_NEAR(box.max.y, y1, 1e-9);
}

Viewport viewportOf(ViewportKind kind, std::string id)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = Box2(Point2(0.0, 0.0), Point2(10.0, 10.0));
    return viewport;
}

const Viewport& byId(const Sheet& sheet, std::string_view id)
{
    for (const Viewport& viewport : sheet.viewports) {
        if (viewport.id == id) {
            return viewport;
        }
    }
    ADD_FAILURE() << "no viewport " << id;
    return sheet.viewports.front();
}

constexpr ViewportKind kEveryKind[] = {
    ViewportKind::Plan,  ViewportKind::LongSection, ViewportKind::CrossSections,
    ViewportKind::Model3D, ViewportKind::Legend,    ViewportKind::Notes,
    ViewportKind::Image, ViewportKind::KeyPlan,     ViewportKind::SheetIndex,
    ViewportKind::Revisions,
};

} // namespace

TEST(SheetLayout, TheTilingAreaIsTheDrawingAreaInsetByOneMillimetre)
{
    const Sheet sheet;
    expectBox(tilingArea(sheet), 24.0, 36.0, 409.0, 286.0);
}

TEST(SheetLayout, TheFullPresetIsTheTilingAreaLessHalfTheGutterAllRound)
{
    const std::vector<Box2> cells = presetCells(TilingPreset::Full, tilingArea(Sheet{}));
    ASSERT_EQ(cells.size(), 1u);
    // 24 + 1.5 .. 409 - 1.5, 36 + 1.5 .. 286 - 1.5.
    expectBox(cells[0], 25.5, 37.5, 407.5, 284.5);
}

TEST(SheetLayout, NeighbouringCellsAreOneGutterApart)
{
    const Box2 area = tilingArea(Sheet{});
    // Two columns: each 385 / 2 - 3 = 189.5 wide; the second starts at
    // 24 + 192.5 + 1.5 = 218, 3 mm after the first ends at 215.
    const std::vector<Box2> columns = presetCells(TilingPreset::Columns2, area);
    ASSERT_EQ(columns.size(), 2u);
    expectBox(columns[0], 25.5, 37.5, 215.0, 284.5);
    expectBox(columns[1], 218.0, 37.5, 407.5, 284.5);
    // Main and panel below: the main 0.64 x 250 - 3 = 157 high from the top
    // (284.5 down to 127.5), the panel 0.36 x 250 - 3 = 87 high under it
    // (124.5 down to 37.5).
    const std::vector<Box2> below = presetCells(TilingPreset::MainBelow, area);
    ASSERT_EQ(below.size(), 2u);
    expectBox(below[0], 25.5, 127.5, 407.5, 284.5);
    expectBox(below[1], 25.5, 37.5, 407.5, 124.5);
    // Quarters: the fourth is bottom-right, from x 218 and up to y
    // 286 - 125 - 1.5 = 159.5.
    const std::vector<Box2> quad = presetCells(TilingPreset::Quad, area);
    ASSERT_EQ(quad.size(), 4u);
    expectBox(quad[3], 218.0, 37.5, 407.5, 159.5);
    // Main and two panels right: the main 0.64 x 385 - 3 = 243.4 wide; the
    // panels 0.36 x 385 - 3 = 135.6 wide from 24 + 246.4 + 1.5 = 271.9, the
    // upper from 284.5 down to 286 - 125 + 1.5 = 162.5.
    const std::vector<Box2> twoRight = presetCells(TilingPreset::MainTwoRight, area);
    ASSERT_EQ(twoRight.size(), 3u);
    expectBox(twoRight[0], 25.5, 37.5, 268.9, 284.5);
    expectBox(twoRight[1], 271.9, 162.5, 407.5, 284.5);
    expectBox(twoRight[2], 271.9, 37.5, 407.5, 159.5);
}

TEST(SheetLayout, TheEightPresetsKeepTheOwnersAppIds)
{
    const std::vector<std::pair<TilingPreset, std::string_view>> ids{
        {TilingPreset::Full, "full"},           {TilingPreset::Columns2, "cols2"},
        {TilingPreset::Rows2, "rows2"},         {TilingPreset::Quad, "quad"},
        {TilingPreset::MainRight, "sectionR"},  {TilingPreset::MainBelow, "sectionB"},
        {TilingPreset::MainCorner, "sectionBR"}, {TilingPreset::MainTwoRight, "sectionMap3d"},
    };
    ASSERT_EQ(ids.size(), kTilingPresetCount);
    for (const auto& [preset, id] : ids) {
        EXPECT_EQ(presetId(preset), id);
        EXPECT_EQ(presetFromId(id), preset);
        EXPECT_FALSE(presetName(preset).empty());
    }
    EXPECT_FALSE(presetFromId("nonsense").has_value());
}

TEST(SheetLayout, EveryViewportKindHasItsOwnRankAndTheMainDrawingComesFirst)
{
    std::set<int> ranks;
    for (const ViewportKind kind : kEveryKind) {
        ranks.insert(tilingRank(kind));
    }
    // Every kind its own rank: no kind falls through a missing table entry
    // the way the owner's app's map panels did.
    EXPECT_EQ(ranks.size(), std::size(kEveryKind));
    EXPECT_LT(tilingRank(ViewportKind::Plan), tilingRank(ViewportKind::LongSection));
    EXPECT_LT(tilingRank(ViewportKind::LongSection), tilingRank(ViewportKind::CrossSections));
    EXPECT_LT(tilingRank(ViewportKind::CrossSections), tilingRank(ViewportKind::Model3D));
    EXPECT_LT(tilingRank(ViewportKind::Model3D), tilingRank(ViewportKind::KeyPlan));
}

TEST(SheetLayout, TilingPutsTheSectionInTheBigCellWhateverOrderTheViewportsWereAdded)
{
    // The owner's app, given [map, section, 3D] and "Section + map + 3D",
    // left the map in the big cell (its sort had no rank for maps). Here a
    // key plan, cross sections and a 3D view added in the worst order still
    // land: sections in the main cell, 3D top-right, key plan bottom-right.
    Sheet sheet;
    sheet.viewports = {viewportOf(ViewportKind::KeyPlan, "key"),
                       viewportOf(ViewportKind::CrossSections, "xs"),
                       viewportOf(ViewportKind::Model3D, "3d")};
    EXPECT_EQ(tileViewports(sheet, TilingPreset::MainTwoRight), 3u);
    expectBox(byId(sheet, "xs").rect, 25.5, 37.5, 268.9, 284.5);
    expectBox(byId(sheet, "3d").rect, 271.9, 162.5, 407.5, 284.5);
    expectBox(byId(sheet, "key").rect, 271.9, 37.5, 407.5, 159.5);
    // The sheet's own order (the drawing order) is left alone.
    EXPECT_EQ(sheet.viewports[0].id, "key");
}

TEST(SheetLayout, LockedViewportsNotesAndViewportsBeyondTheCellsStayWhereTheyWere)
{
    Sheet sheet;
    sheet.viewports = {viewportOf(ViewportKind::Plan, "a"), viewportOf(ViewportKind::Plan, "b"),
                       viewportOf(ViewportKind::Plan, "c"), viewportOf(ViewportKind::Notes, "n"),
                       viewportOf(ViewportKind::Legend, "locked")};
    sheet.viewports[4].locked = true;
    // Two columns for three plans: the first two (in sheet order, as ties
    // keep it) are placed, the third is not.
    EXPECT_EQ(tileViewports(sheet, TilingPreset::Columns2), 2u);
    expectBox(byId(sheet, "a").rect, 25.5, 37.5, 215.0, 284.5);
    expectBox(byId(sheet, "b").rect, 218.0, 37.5, 407.5, 284.5);
    expectBox(byId(sheet, "c").rect, 0.0, 0.0, 10.0, 10.0);
    expectBox(byId(sheet, "n").rect, 0.0, 0.0, 10.0, 10.0);
    expectBox(byId(sheet, "locked").rect, 0.0, 0.0, 10.0, 10.0);
}

TEST(SheetLayout, ATiledViewportIsNeverSmallerThanItsKindsMinimum)
{
    // Portrait A4 has no frame: paper less 10 mm, 10..200 x 10..287, tiled
    // inside 11..199 x 11..286 (188 x 275). In "Main and two panels right" the
    // upper panel is 0.36 x 188 - 3 = 64.68 mm wide - under a long section's
    // 70 mm minimum - so it is 70 wide, from 11 + 120.32 + 1.5 = 132.82, and
    // 0.5 x 275 - 3 = 134.5 high down from 284.5.
    Sheet sheet;
    sheet.paper = PaperSize::A4;
    sheet.landscape = false;
    sheet.frame.clear();
    sheet.viewports = {viewportOf(ViewportKind::Plan, "plan"),
                       viewportOf(ViewportKind::LongSection, "long")};
    tileViewports(sheet, TilingPreset::MainTwoRight);
    expectBox(byId(sheet, "long").rect, 132.82, 150.0, 202.82, 284.5);
    EXPECT_EQ(minimumSize(ViewportKind::LongSection).width, 70.0);
}

TEST(SheetLayout, AMovedViewportSnapsByTheSmallestShiftToAnEdgeOrCentre)
{
    const Box2 area(Point2(23.0, 35.0), Point2(410.0, 287.0));
    // Another viewport: edges x 160 and 260 (centre 210), y 50 and 120
    // (centre 85).
    const std::vector<Box2> others{Box2(Point2(160.0, 50.0), Point2(260.0, 120.0))};
    // The moving one: x 100..158.5, y 100..141 (centres 129.25, 120.5).
    // Its right edge is 1.5 short of the other's left edge; its centre line
    // 0.5 above the other's top edge. Nothing else is within 2.5 mm.
    const SnapResult snapped =
        snapRect(Box2(Point2(100.0, 100.0), Point2(158.5, 141.0)), area, others, 2.5);
    expectBox(snapped.rect, 101.5, 99.5, 160.0, 140.5);
    ASSERT_TRUE(snapped.guideX.has_value());
    EXPECT_EQ(*snapped.guideX, 160.0);
    ASSERT_TRUE(snapped.guideY.has_value());
    EXPECT_EQ(*snapped.guideY, 120.0);
}

TEST(SheetLayout, ASnapPrefersTheNearerTargetAndTheDrawingAreaCounts)
{
    const Box2 area(Point2(23.0, 35.0), Point2(410.0, 287.0));
    // Left edge 24.2: 1.2 from the area's left edge 23. The right edge 70
    // is 2.0 from another viewport's left edge at 72. The nearer wins.
    const std::vector<Box2> others{Box2(Point2(72.0, 200.0), Point2(90.0, 210.0))};
    // Vertically, y 180..196 (centre 188) is 4 mm from the other's bottom
    // edge at 200 and far from the area's centre line at 161: no snap.
    const SnapResult snapped =
        snapRect(Box2(Point2(24.2, 180.0), Point2(70.0, 196.0)), area, others, 2.5);
    EXPECT_NEAR(snapped.rect.min.x, 23.0, 1e-12);
    EXPECT_NEAR(snapped.rect.max.x, 68.8, 1e-12);
    EXPECT_EQ(snapped.guideX, 23.0);
    EXPECT_FALSE(snapped.guideY.has_value());
    EXPECT_EQ(snapped.rect.min.y, 180.0);
}

TEST(SheetLayout, NothingWithinToleranceLeavesTheRectangleAlone)
{
    const Box2 area(Point2(23.0, 35.0), Point2(410.0, 287.0));
    const Box2 moving(Point2(100.0, 100.0), Point2(150.0, 140.0));
    const SnapResult snapped = snapRect(moving, area, {}, 1.0);
    expectBox(snapped.rect, 100.0, 100.0, 150.0, 140.0);
    EXPECT_FALSE(snapped.guideX.has_value());
    EXPECT_FALSE(snapped.guideY.has_value());
    // A zero tolerance is snapping off.
    const SnapResult off = snapRect(Box2(Point2(23.5, 35.5), Point2(50.0, 60.0)), area, {}, 0.0);
    expectBox(off.rect, 23.5, 35.5, 50.0, 60.0);
}
