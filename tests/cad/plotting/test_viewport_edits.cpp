// Several viewports at once, and the paper geometry of the sheet editor
// (include/katana/cad/plotting/viewport_edits.hpp): the paper grid and its
// snapping, the rubber band, Tab's order, a plan's paper and world, and the
// group edits - move, remove, copy, paste, duplicate - each ONE undoable step.

#include <gtest/gtest.h>

#include <cmath>
#include <numbers>
#include <stdexcept>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/viewport_edits.hpp"

using katana::cad::Document;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;

namespace {

Viewport panel(std::string id, Box2 rect, ViewportKind kind = ViewportKind::Notes)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    return viewport;
}

Box2 box(double x0, double y0, double x1, double y1)
{
    return Box2(Point2(x0, y0), Point2(x1, y1));
}

// Two sheets: s1 with three panels, vp3 locked; s2 with one.
SheetSet twoSheets()
{
    SheetSet set;
    Sheet first;
    first.id = "s1";
    first.name = "FIRST";
    first.viewports = {panel("vp1", box(100.0, 100.0, 200.0, 180.0)),
                       panel("vp2", box(220.0, 100.0, 300.0, 180.0), ViewportKind::Legend),
                       panel("vp3", box(320.0, 100.0, 380.0, 180.0), ViewportKind::Image)};
    first.viewports[2].locked = true;
    Sheet second;
    second.id = "s2";
    second.name = "SECOND";
    second.viewports = {panel("vp4", box(50.0, 50.0, 90.0, 90.0))};
    set.sheets = {first, second};
    return set;
}

const Viewport& byId(const Document& document, std::size_t sheet, std::string_view id)
{
    for (const Viewport& viewport : document.sheetSet().sheets.at(sheet).viewports) {
        if (viewport.id == id) {
            return viewport;
        }
    }
    throw std::runtime_error("no viewport " + std::string(id));
}

std::vector<std::string> idsOf(const Sheet& sheet)
{
    std::vector<std::string> ids;
    for (const Viewport& viewport : sheet.viewports) {
        ids.push_back(viewport.id);
    }
    return ids;
}

using Ids = std::vector<std::string>;

} // namespace

// ---- the paper grid ----------------------------------------------------------------

TEST(ViewportEdits, AValueSnapsToTheNearestGridLineThroughThePapersOrigin)
{
    EXPECT_DOUBLE_EQ(snapToGrid(12.4, 5.0), 10.0);
    EXPECT_DOUBLE_EQ(snapToGrid(12.6, 5.0), 15.0);
    EXPECT_DOUBLE_EQ(snapToGrid(12.5, 5.0), 15.0); // halfway goes up
    EXPECT_DOUBLE_EQ(snapToGrid(-2.4, 5.0), 0.0);
    EXPECT_DOUBLE_EQ(snapToGrid(-2.6, 5.0), -5.0);
    // No grid: the value as it is.
    EXPECT_DOUBLE_EQ(snapToGrid(12.4, 0.0), 12.4);
    EXPECT_DOUBLE_EQ(snapToGrid(12.4, -5.0), 12.4);
}

TEST(ViewportEdits, AMovingRectangleSnapsAnEdgeToTheGridWhereNoEdgeIsInReach)
{
    const Box2 area = box(23.0, 35.0, 410.0, 287.0);
    // Its left edge is 1.2 past a line, its right edge 2.4 past one; the
    // left is the smaller move.
    const SnapResult left = snapMovingRect(box(111.2, 121.9, 212.4, 171.9), area, {}, 1.0, 5.0);
    EXPECT_DOUBLE_EQ(left.rect.min.x, 110.0);
    EXPECT_DOUBLE_EQ(left.rect.max.x, 211.2);
    // 121.9 is 1.9 from 120, and 171.9 is 1.9 from 170 too: the first edge.
    EXPECT_NEAR(left.rect.min.y, 120.0, 1e-9);
    EXPECT_NEAR(left.rect.height(), 50.0, 1e-9);
    // The grid draws no guide.
    EXPECT_FALSE(left.guideX);
    EXPECT_FALSE(left.guideY);

    // The right edge nearer a line: it goes there, and the width is kept.
    const SnapResult right = snapMovingRect(box(111.2, 120.0, 214.0, 170.0), area, {}, 1.0, 5.0);
    EXPECT_NEAR(right.rect.max.x, 215.0, 1e-9);
    EXPECT_NEAR(right.rect.width(), 102.8, 1e-9);
}

TEST(ViewportEdits, AnEdgeInReachWinsOverTheGrid)
{
    const Box2 area = box(23.0, 35.0, 410.0, 287.0);
    // A neighbour whose right edge, 201.3, is off the grid; the moving panel's
    // left edge is 0.5 from it.
    const std::vector<Box2> others{box(150.0, 40.0, 201.3, 60.0)};
    const SnapResult snapped = snapMovingRect(box(201.8, 121.0, 251.8, 171.0), area, others, 1.0, 5.0);
    EXPECT_DOUBLE_EQ(snapped.rect.min.x, 201.3);
    ASSERT_TRUE(snapped.guideX);
    EXPECT_DOUBLE_EQ(*snapped.guideX, 201.3);
    // Nothing in reach vertically: the grid.
    EXPECT_FALSE(snapped.guideY);
    EXPECT_DOUBLE_EQ(snapped.rect.min.y, 120.0);

    // No grid: only the edge snapping, as snapRect.
    const SnapResult plain = snapMovingRect(box(201.8, 121.0, 251.8, 171.0), area, others, 1.0, 0.0);
    EXPECT_DOUBLE_EQ(plain.rect.min.y, 121.0);
}

TEST(ViewportEdits, AResizedEdgeSnapsToATargetInReachElseToTheGrid)
{
    const std::vector<double> targets{201.3, 250.0};
    const EdgeSnap onTarget = snapEdge(200.9, targets, 1.0, 5.0);
    EXPECT_DOUBLE_EQ(onTarget.value, 201.3);
    ASSERT_TRUE(onTarget.guide);
    EXPECT_DOUBLE_EQ(*onTarget.guide, 201.3);

    const EdgeSnap onGrid = snapEdge(207.4, targets, 1.0, 5.0);
    EXPECT_DOUBLE_EQ(onGrid.value, 205.0);
    EXPECT_FALSE(onGrid.guide);

    const EdgeSnap free = snapEdge(207.4, targets, 1.0, 0.0);
    EXPECT_DOUBLE_EQ(free.value, 207.4);
    EXPECT_FALSE(free.guide);
}

// ---- picking -----------------------------------------------------------------------

TEST(ViewportEdits, AWindowTakesWhatItEnclosesAndACrossingWhatItTouches)
{
    const Sheet sheet = twoSheets().sheets[0];
    // Around vp1 wholly and half of vp2.
    const Box2 band = box(90.0, 90.0, 260.0, 190.0);
    EXPECT_EQ(viewportsInBand(sheet, band, BandMode::Window), (Ids{"vp1"}));
    EXPECT_EQ(viewportsInBand(sheet, band, BandMode::Crossing), (Ids{"vp1", "vp2"}));
    // An empty band picks nothing; an unplaced viewport is never picked.
    EXPECT_TRUE(viewportsInBand(sheet, Box2(), BandMode::Crossing).empty());
    Sheet withUnplaced = sheet;
    withUnplaced.viewports.push_back(panel("vp9", Box2()));
    EXPECT_EQ(viewportsInBand(withUnplaced, box(0.0, 0.0, 500.0, 500.0), BandMode::Window),
              (Ids{"vp1", "vp2", "vp3"}));
}

TEST(ViewportEdits, TheBoundsOfSeveralViewportsSpanThemAll)
{
    const Sheet sheet = twoSheets().sheets[0];
    EXPECT_EQ(viewportBounds(sheet, Ids{"vp1", "vp3"}), box(100.0, 100.0, 380.0, 180.0));
    EXPECT_TRUE(viewportBounds(sheet, Ids{"nothing"}).empty());
}

TEST(ViewportEdits, TabStepsThroughThePlacedViewportsAndWrapsRound)
{
    Sheet sheet = twoSheets().sheets[0];
    sheet.viewports.insert(sheet.viewports.begin() + 1, panel("vp9", Box2()));
    EXPECT_EQ(cycleViewport(sheet, "", true), "vp1");
    EXPECT_EQ(cycleViewport(sheet, "", false), "vp3");
    EXPECT_EQ(cycleViewport(sheet, "vp1", true), "vp2"); // the unplaced vp9 is skipped
    EXPECT_EQ(cycleViewport(sheet, "vp3", true), "vp1");
    EXPECT_EQ(cycleViewport(sheet, "vp1", false), "vp3");
    EXPECT_EQ(cycleViewport(sheet, "gone", true), "vp1");
    EXPECT_EQ(cycleViewport(Sheet{}, "", true), "");
}

// ---- a plan's paper and its world --------------------------------------------------

TEST(ViewportEdits, APaperPointOfAPlanIsTheWorldPointDrawnThere)
{
    // A plan 200 x 150 mm centred at (200, 175) on the paper, at 1:500, with
    // (1000, 2000) at its centre.
    Viewport plan = panel("vp1", box(100.0, 100.0, 300.0, 250.0), ViewportKind::Plan);
    const Point2 centre(1000.0, 2000.0);
    EXPECT_EQ(planPaperToWorld(plan, 500.0, centre, Point2(200.0, 175.0)), centre);
    // 20 mm right at 1:500 is 10 m east; 10 mm up is 5 m north.
    const Point2 east = planPaperToWorld(plan, 500.0, centre, Point2(220.0, 185.0));
    EXPECT_NEAR(east.x, 1010.0, 1e-9);
    EXPECT_NEAR(east.y, 2005.0, 1e-9);

    // Turned so north runs left to right across the paper: right is north,
    // up is west.
    plan.rotation = std::numbers::pi / 2.0;
    const Point2 north = planPaperToWorld(plan, 500.0, centre, Point2(220.0, 185.0));
    EXPECT_NEAR(north.x, 995.0, 1e-9);
    EXPECT_NEAR(north.y, 2010.0, 1e-9);
    // And back again.
    const Point2 paper = planWorldToPaper(plan, 500.0, centre, north);
    EXPECT_NEAR(paper.x, 220.0, 1e-9);
    EXPECT_NEAR(paper.y, 185.0, 1e-9);
}

// ---- the edits ---------------------------------------------------------------------

TEST(ViewportEdits, SeveralViewportsMoveInOneStepAndALockedOneStays)
{
    Document document;
    ASSERT_TRUE(document.setSheetSet(twoSheets()).ok());
    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(moveViewports(document, 0, Ids{"vp1", "vp2", "vp3"}, Point2(10.0, -5.0)).ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(byId(document, 0, "vp1").rect, box(110.0, 95.0, 210.0, 175.0));
    EXPECT_EQ(byId(document, 0, "vp2").rect, box(230.0, 95.0, 310.0, 175.0));
    EXPECT_EQ(byId(document, 0, "vp3").rect, box(320.0, 100.0, 380.0, 180.0)); // locked
    // The other sheet is untouched.
    EXPECT_EQ(byId(document, 1, "vp4").rect, box(50.0, 50.0, 90.0, 90.0));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.sheetSet() == twoSheets());
}

TEST(ViewportEdits, AMoveOfNothingOrOfAStrangerIsRefusedAndChangesNothing)
{
    Document document;
    ASSERT_TRUE(document.setSheetSet(twoSheets()).ok());
    const std::size_t before = document.history().undoCount();
    // vp4 is on the other sheet.
    const auto stranger = moveViewports(document, 0, Ids{"vp1", "vp4"}, Point2(1.0, 0.0));
    ASSERT_FALSE(stranger.ok());
    EXPECT_EQ(stranger.error().code, ErrorCode::NotFound);
    EXPECT_NE(stranger.error().context.find("vp4"), std::string::npos);
    EXPECT_EQ(moveViewports(document, 0, Ids{}, Point2(1.0, 0.0)).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(moveViewports(document, 5, Ids{"vp1"}, Point2(1.0, 0.0)).error().code,
              ErrorCode::NotFound);
    EXPECT_EQ(moveViewports(document, 0, Ids{"vp1"}, Point2(std::nan(""), 0.0)).error().code,
              ErrorCode::InvalidArgument);
    // Only the locked one: nothing moves, so no step.
    ASSERT_TRUE(moveViewports(document, 0, Ids{"vp3"}, Point2(1.0, 0.0)).ok());
    EXPECT_EQ(document.history().undoCount(), before);
    EXPECT_TRUE(document.sheetSet() == twoSheets());
}

TEST(ViewportEdits, SeveralViewportsGoInOneStepLockedOrNot)
{
    Document document;
    ASSERT_TRUE(document.setSheetSet(twoSheets()).ok());
    const std::size_t before = document.history().undoCount();
    // A repeated id counts once.
    ASSERT_TRUE(removeViewports(document, 0, Ids{"vp1", "vp3", "vp1"}).ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(idsOf(document.sheetSet().sheets[0]), (Ids{"vp2"}));
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(idsOf(document.sheetSet().sheets[0]), (Ids{"vp1", "vp2", "vp3"}));
    EXPECT_EQ(removeViewports(document, 0, Ids{"vp4"}).error().code, ErrorCode::NotFound);
}

TEST(ViewportEdits, ACopyIsInTheSheetsOrderWhateverTheOrderAskedFor)
{
    const SheetSet set = twoSheets();
    auto copies = copyViewports(set, 0, Ids{"vp3", "vp1"});
    ASSERT_TRUE(copies.ok());
    ASSERT_EQ(copies->size(), 2u);
    EXPECT_EQ((*copies)[0], set.sheets[0].viewports[0]);
    EXPECT_EQ((*copies)[1], set.sheets[0].viewports[2]);
    EXPECT_EQ(copyViewports(set, 0, Ids{"vp4"}).error().code, ErrorCode::NotFound);
    EXPECT_EQ(copyViewports(set, 2, Ids{"vp1"}).error().code, ErrorCode::NotFound);
}

TEST(ViewportEdits, APasteStepsClearOfTheRectanglesAlreadyThere)
{
    const SheetSet set = twoSheets();
    const std::vector<Viewport> clip{set.sheets[0].viewports[0]};
    // On its own sheet the original is where the copy was: one step on.
    EXPECT_EQ(pasteOffset(set.sheets[0], clip), Point2(5.0, -5.0));
    // On another sheet: where it was.
    EXPECT_EQ(pasteOffset(set.sheets[1], clip), Point2(0.0, 0.0));
    // With a copy one step on already, two steps.
    Sheet crowded = set.sheets[0];
    crowded.viewports.push_back(panel("vp8", box(105.0, 95.0, 205.0, 175.0)));
    EXPECT_EQ(pasteOffset(crowded, clip), Point2(10.0, -10.0));
    EXPECT_EQ(pasteOffset(crowded, clip, 2.0), Point2(2.0, -2.0));
}

TEST(ViewportEdits, PastingOntoAnotherSheetIsOneStepWithNewIds)
{
    Document document;
    ASSERT_TRUE(document.setSheetSet(twoSheets()).ok());
    auto clip = copyViewports(document.sheetSet(), 0, Ids{"vp1", "vp2"});
    ASSERT_TRUE(clip.ok());
    const std::size_t before = document.history().undoCount();
    auto pasted = pasteViewports(document, 1, *clip);
    ASSERT_TRUE(pasted.ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    // New ids after the highest in the set, in the clipboard's order.
    EXPECT_EQ(*pasted, (Ids{"vp5", "vp6"}));
    const Sheet& second = document.sheetSet().sheets[1];
    EXPECT_EQ(idsOf(second), (Ids{"vp4", "vp5", "vp6"}));
    // Where they were, and otherwise the same.
    Viewport expected = (*clip)[1];
    expected.id = "vp6";
    EXPECT_EQ(second.viewports[2], expected);
    EXPECT_EQ(second.viewports[1].rect, box(100.0, 100.0, 200.0, 180.0));
    // The first sheet is as it was.
    EXPECT_TRUE(document.sheetSet().sheets[0] == twoSheets().sheets[0]);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.sheetSet() == twoSheets());
}

TEST(ViewportEdits, RepeatedPastesOntoTheSameSheetStepDownTheSheet)
{
    Document document;
    ASSERT_TRUE(document.setSheetSet(twoSheets()).ok());
    auto clip = copyViewports(document.sheetSet(), 0, Ids{"vp1"});
    ASSERT_TRUE(clip.ok());
    auto first = pasteViewports(document, 0, *clip);
    auto second = pasteViewports(document, 0, *clip);
    ASSERT_TRUE(first.ok() && second.ok());
    EXPECT_EQ(byId(document, 0, first->front()).rect, box(105.0, 95.0, 205.0, 175.0));
    EXPECT_EQ(byId(document, 0, second->front()).rect, box(110.0, 90.0, 210.0, 170.0));
    // An offset given is used as it is.
    auto placed = pasteViewports(document, 0, *clip, Point2(0.0, 0.0));
    ASSERT_TRUE(placed.ok());
    EXPECT_EQ(byId(document, 0, placed->front()).rect, box(100.0, 100.0, 200.0, 180.0));
    EXPECT_EQ(pasteViewports(document, 0, {}).error().code, ErrorCode::InvalidArgument);
}

TEST(ViewportEdits, DuplicatingIsOneStepThatPlacesTheCopiesOneStepOn)
{
    Document document;
    ASSERT_TRUE(document.setSheetSet(twoSheets()).ok());
    const std::size_t before = document.history().undoCount();
    auto copies = duplicateViewports(document, 0, Ids{"vp2", "vp1"});
    ASSERT_TRUE(copies.ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(*copies, (Ids{"vp5", "vp6"})); // vp1's copy first: the sheet's order
    EXPECT_EQ(byId(document, 0, "vp5").rect, box(105.0, 95.0, 205.0, 175.0));
    EXPECT_EQ(byId(document, 0, "vp6").kind, ViewportKind::Legend);
    EXPECT_EQ(idsOf(document.sheetSet().sheets[0]), (Ids{"vp1", "vp2", "vp3", "vp5", "vp6"}));
    EXPECT_EQ(duplicateViewports(document, 0, Ids{"vp4"}).error().code, ErrorCode::NotFound);
}
