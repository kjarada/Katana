// The layout presets and how they become docked views (PLAN.MD Phase 08, 47).
//
// Since the dock workspace of 2026-09-23 a preset is no longer a tiling that
// Katana computes pixels for - Qt's QMainWindow owns the pixels - but an
// ARRANGEMENT: layoutRects says what "Three: Left" looks like, and dockSplits
// says how to build it out of docks by halving one view at a time. The test
// that matters is that the two agree exactly, so the picture in the menu and
// the docks it produces cannot drift apart. It simulates each split on
// rectangles the way QMainWindow::splitDockWidget splits a dock: the existing
// view keeps the left or top half, the added view gets the right or bottom.
//
// The ViewportLayout class and its ViewportCell were retired with their last
// user, the tiled ViewportContainer. Where each of their tests went:
//
//   StartsAsASinglePlanView
//       ViewSet.TheFirstViewAddedBecomesActiveAndLaterOnesDoNot, and slot 0 of
//       AnArrangementOpensPlanThreeDSectionAndElevationInThatOrder below.
//       Opening the first view is now the workspace's act, not the model's.
//   EveryLayoutTilesTheAreaExactlyOnce                  kept below.
//   EveryPointBelongsToExactlyOneCell
//       EveryPointOfTheAreaBelongsToExactlyOneRect below, on layoutRects.
//       Its cellAt half went with the class: Qt hit-tests the docks.
//   PointsOutsideTheAreaHitNothing
//       PointsOnTheFarEdgeOrOutsideTheAreaBelongToNoRect below.
//   QuadPutsPlanThreeDSectionAndElevationInThatOrder
//       AnArrangementOpensPlanThreeDSectionAndElevationInThatOrder below.
//   GrowingTheLayoutKeepsTheViewTheUserAlreadyHad
//       ViewSet.ChangingKindKeepsThePlanZoomTheLayersAndTheSection and
//       ViewSet.AViewsStateStaysAtOneAddressWhateverElseOpensOrCloses: a view
//       now lives until it is closed, whatever arrangement it is in.
//   ShrinkingMovesTheActiveCellInsteadOfLeavingItDangling
//       ViewSet.RemovingTheActiveViewActivatesTheMostRecentlyActiveOfTheRest.
//   ActivatingAnIndexOutsideTheLayoutDoesNothing
//       ViewSet.ActivatingAnUnknownViewIsNotFoundAndChangesNothing.
//   SettingTheKindOfAMissingCellIsReportedNotIgnored
//       ViewSet.SettingTheKindOfAnUnknownViewIsNotFoundAndChangesNothing.
//   ChangingAKindGivesTheCellAnAppropriateProjection
//       ViewSet.ChangingKindGivesTheCameraThatKindsStartingView, which checks
//       the direction as well as the projection.
//   EveryLayoutAndViewKindHasAName
//       EveryLayoutAndViewKindHasADistinctName below.
//   PixelRectanglesTileTheWindowWithNoSeamAndNoOverlap,
//   CellsNeverReportAZeroSizedFramebuffer,
//   ResizingUpdatesEveryCellsCameraViewport
//       No longer cad's to test. QMainWindow gives each dock its pixels, and
//       each view widget sizes its own framebuffer and camera, never below
//       1 x 1, in its own resizeEvent (RenderViewWidget::resizeEvent).

#include <gtest/gtest.h>

#include <cmath>
#include <cstddef>
#include <limits>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/viewport_layout.hpp"

using katana::cad::CellRect;
using katana::cad::DockSplit;
using katana::cad::LayoutKind;
using katana::cad::ViewKind;

namespace {

constexpr LayoutKind kAllLayouts[] = {LayoutKind::Single,     LayoutKind::SplitVertical,
                                      LayoutKind::SplitHorizontal, LayoutKind::ThreeLeft,
                                      LayoutKind::ThreeTop,   LayoutKind::Quad};

// What each preset's name promises, counted independently of layoutRects.
std::size_t viewsPromised(LayoutKind kind)
{
    switch (kind) {
    case LayoutKind::Single:
        return 1;
    case LayoutKind::SplitVertical:
    case LayoutKind::SplitHorizontal:
        return 2;
    case LayoutKind::ThreeLeft:
    case LayoutKind::ThreeTop:
        return 3;
    case LayoutKind::Quad:
        return 4;
    }
    return 0;
}

// Applies the steps to rectangles. Every value involved is a sum of powers of
// two (1, 1/2, 1/4), so the halving is exact and the comparison below can be
// ==, not "near".
std::vector<CellRect> simulateDocking(const std::vector<DockSplit>& steps, std::size_t views)
{
    std::vector<CellRect> placed(views);
    std::vector<bool> isPlaced(views, false);
    placed[0] = CellRect{0.0, 0.0, 1.0, 1.0};
    isPlaced[0] = true;
    for (const DockSplit& step : steps) {
        EXPECT_LT(step.existing, views);
        EXPECT_LT(step.added, views);
        if (step.existing >= views || step.added >= views) {
            return {};
        }
        EXPECT_TRUE(isPlaced[step.existing]) << "a split can only halve a view already placed";
        EXPECT_FALSE(isPlaced[step.added]) << "each view is placed exactly once";
        CellRect& existing = placed[step.existing];
        CellRect added = existing;
        if (step.sideBySide) {
            existing.width /= 2.0;
            added.x = existing.x + existing.width;
            added.width = existing.width;
        } else {
            existing.height /= 2.0;
            added.y = existing.y + existing.height;
            added.height = existing.height;
        }
        placed[step.added] = added;
        isPlaced[step.added] = true;
    }
    for (std::size_t i = 0; i < views; ++i) {
        EXPECT_TRUE(isPlaced[i]) << "view " << i << " was never placed";
    }
    return placed;
}

} // namespace

// ---- the presets ----------------------------------------------------------------------------

TEST(ViewportLayout, EachPresetHasAsManyViewsAsItsNameSays)
{
    for (LayoutKind kind : kAllLayouts) {
        EXPECT_EQ(katana::cad::cellCount(kind), viewsPromised(kind)) << katana::cad::toString(kind);
        EXPECT_EQ(katana::cad::layoutRects(kind).size(), viewsPromised(kind));
    }
}

TEST(ViewportLayout, EveryLayoutTilesTheAreaExactlyOnce)
{
    for (LayoutKind kind : kAllLayouts) {
        const auto rects = katana::cad::layoutRects(kind);
        ASSERT_FALSE(rects.empty());

        double area = 0.0;
        for (const CellRect& rect : rects) {
            EXPECT_GT(rect.width, 0.0);
            EXPECT_GT(rect.height, 0.0);
            EXPECT_GE(rect.x, 0.0);
            EXPECT_GE(rect.y, 0.0);
            EXPECT_LE(rect.x + rect.width, 1.0 + 1e-12);
            EXPECT_LE(rect.y + rect.height, 1.0 + 1e-12);
            area += rect.width * rect.height;
        }
        // Tiled, not floating: the cells cover the area and do not overlap, so
        // the areas must add to exactly one.
        EXPECT_NEAR(area, 1.0, 1e-12) << katana::cad::toString(kind);
    }
}

TEST(ViewportLayout, EveryPointOfTheAreaBelongsToExactlyOneRect)
{
    // A fine sweep including the shared borders at 0.5, where a closed
    // rectangle test would match two. Asked of layoutRects directly: the menu
    // draws its pictures from them.
    for (LayoutKind kind : kAllLayouts) {
        const auto rects = katana::cad::layoutRects(kind);
        for (int i = 0; i < 40; ++i) {
            for (int j = 0; j < 40; ++j) {
                const double u = static_cast<double>(i) / 40.0;
                const double v = static_cast<double>(j) / 40.0;
                int matches = 0;
                for (const CellRect& rect : rects) {
                    if (rect.contains(u, v)) {
                        ++matches;
                    }
                }
                ASSERT_EQ(matches, 1) << katana::cad::toString(kind) << " at " << u << "," << v;
            }
        }
    }
}

TEST(ViewportLayout, PointsOnTheFarEdgeOrOutsideTheAreaBelongToNoRect)
{
    const auto rects = katana::cad::layoutRects(LayoutKind::Quad);
    const auto inAny = [&](double u, double v) {
        for (const CellRect& rect : rects) {
            if (rect.contains(u, v)) {
                return true;
            }
        }
        return false;
    };
    EXPECT_FALSE(inAny(-0.01, 0.5));
    EXPECT_FALSE(inAny(0.5, 1.0)) << "the far edge is exclusive";
    EXPECT_FALSE(inAny(1.0, 0.5));
    EXPECT_FALSE(inAny(std::numeric_limits<double>::quiet_NaN(), 0.5));
    EXPECT_TRUE(inAny(0.0, 0.0)) << "the near edge is inclusive";
}

TEST(ViewportLayout, EveryLayoutAndViewKindHasADistinctName)
{
    std::set<std::string> names;
    for (LayoutKind kind : kAllLayouts) {
        const std::string name = katana::cad::toString(kind);
        EXPECT_NE(name, "Unknown");
        EXPECT_TRUE(names.insert(name).second) << "duplicate layout name " << name;
    }
    names.clear();
    for (ViewKind kind : {ViewKind::Plan, ViewKind::Model3D, ViewKind::Section,
                          ViewKind::Elevation}) {
        const std::string name = katana::cad::toString(kind);
        EXPECT_NE(name, "Unknown");
        EXPECT_TRUE(names.insert(name).second) << "duplicate view name " << name;
    }
}

// ---- the kinds a preset opens -----------------------------------------------------------------

TEST(ViewportLayout, AnArrangementOpensPlanThreeDSectionAndElevationInThatOrder)
{
    // The four-up arrangement a civil engineer expects, and the first slot is
    // always the plan the user was already drawing in.
    EXPECT_EQ(katana::cad::defaultViewKind(0), ViewKind::Plan);
    EXPECT_EQ(katana::cad::defaultViewKind(1), ViewKind::Model3D);
    EXPECT_EQ(katana::cad::defaultViewKind(2), ViewKind::Section);
    EXPECT_EQ(katana::cad::defaultViewKind(3), ViewKind::Elevation);
}

TEST(ViewportLayout, SlotsPastTheFourthRepeatTheLastKind)
{
    for (const std::size_t slot : {std::size_t{4}, std::size_t{5}, std::size_t{100},
                                   std::numeric_limits<std::size_t>::max()}) {
        EXPECT_EQ(katana::cad::defaultViewKind(slot), ViewKind::Elevation) << slot;
    }
}

// ---- dockSplits -------------------------------------------------------------------------------

TEST(ViewportLayout, DockingTheSplitsReproducesEveryPresetsRectanglesExactly)
{
    for (LayoutKind kind : kAllLayouts) {
        const auto expected = katana::cad::layoutRects(kind);
        const auto steps = katana::cad::dockSplits(kind);
        EXPECT_EQ(steps.size(), expected.size() - 1) << "one split per view after the first";

        const auto docked = simulateDocking(steps, expected.size());
        ASSERT_EQ(docked.size(), expected.size()) << katana::cad::toString(kind);
        for (std::size_t i = 0; i < expected.size(); ++i) {
            EXPECT_EQ(docked[i], expected[i])
                << katana::cad::toString(kind) << " view " << i << ": docked at (" << docked[i].x
                << ", " << docked[i].y << ") " << docked[i].width << " x " << docked[i].height;
        }
    }
}

TEST(ViewportLayout, ASingleViewNeedsNoSplit)
{
    EXPECT_TRUE(katana::cad::dockSplits(LayoutKind::Single).empty());
}

TEST(ViewportLayout, QuadHalvesTheTopRowBeforeSplittingEachHalfDownwards)
{
    // Worked by hand: splitting 0 side by side gives two half-width columns;
    // splitting each column downwards gives four quarters. Splitting 0
    // downwards first and then 0 again side by side would leave view 1 a
    // full-width half - the order is the contract, so it is pinned.
    const std::vector<DockSplit> expected = {
        {0, 1, true}, {0, 2, false}, {1, 3, false}};
    EXPECT_EQ(katana::cad::dockSplits(LayoutKind::Quad), expected);
}
