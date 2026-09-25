// Arranging a project's sheets as undoable steps
// (include/katana/cad/plotting/arrange_commands.hpp): each command is ONE
// step, undone whole; a refused one records nothing; and what a view shows is
// taken from the drawing when the caller gives nothing.
//
// The A3 tiling area is 24..409 x 36..286 mm, 385 x 250; with the painter's
// 4% to spare a view that fills it holds 370.19 x 240.38 mm of drawing.

#include <gtest/gtest.h>

#include <cmath>
#include <numbers>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/arrange_commands.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::cad::Document;
using katana::cad::PaperSize;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;

namespace {

constexpr double kPi = std::numbers::pi;

Viewport viewportAt(std::string id, ViewportKind kind, Box2 rect)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    return viewport;
}

// One sheet holding `viewports`, added as one step. The set is empty, so
// the ids are given out afresh from vp1 in order, as they are written here.
void addSheetWith(Document& document, std::vector<Viewport> viewports)
{
    Sheet sheet;
    sheet.id = "s1";
    sheet.name = "TEST";
    sheet.viewports = std::move(viewports);
    ASSERT_TRUE(addSheet(document, sheet).ok());
}

const Viewport& viewportOf(const Document& document, std::string_view id)
{
    for (const Sheet& sheet : document.sheetSet().sheets) {
        for (const Viewport& viewport : sheet.viewports) {
            if (viewport.id == id) {
                return viewport;
            }
        }
    }
    ADD_FAILURE() << "no viewport " << id;
    return document.sheetSet().sheets.front().viewports.front();
}

void addLine(Document& document, Point2 a, Point2 b)
{
    ASSERT_TRUE(document.execute(katana::commands::createLine(a, b)).ok());
}

void expectBox(const Box2& box, double x0, double y0, double x1, double y1)
{
    EXPECT_NEAR(box.min.x, x0, 1e-9);
    EXPECT_NEAR(box.min.y, y0, 1e-9);
    EXPECT_NEAR(box.max.x, x1, 1e-9);
    EXPECT_NEAR(box.max.y, y1, 1e-9);
}

} // namespace

TEST(SheetArrangeCommands, AutoArrangeIsOneStepAndUndoPutsTheViewsBack)
{
    Document document;
    const Sheet blank;
    const Box2 full = presetCells(TilingPreset::Full, tilingArea(blank))[0];
    const Box2 dropped(Point2(100.0, 100.0), Point2(180.0, 200.0));
    addSheetWith(document, {viewportAt("vp1", ViewportKind::Plan, full),
                            viewportAt("vp2", ViewportKind::Legend, dropped)});
    const std::size_t before = document.history().undoCount();

    const auto result = autoArrangeSheet(document, 0);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(result->moved, (std::vector<std::string>{"vp1", "vp2"}));
    EXPECT_TRUE(result->mainShrunk);
    EXPECT_TRUE(result->overlapping.empty());

    // Arranged again, nothing changes and no step is recorded.
    const auto again = autoArrangeSheet(document, 0);
    ASSERT_TRUE(again.ok());
    EXPECT_TRUE(again->moved.empty());
    EXPECT_EQ(document.history().undoCount(), before + 1);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(viewportOf(document, "vp1").rect, full);
    EXPECT_EQ(viewportOf(document, "vp2").rect, dropped);

    EXPECT_EQ(autoArrangeSheet(document, 3).error().code, ErrorCode::NotFound);
}

TEST(SheetArrangeCommands, AligningAndSpacingAreOneStepEachAndARefusalRecordsNothing)
{
    Document document;
    addSheetWith(document,
                 {viewportAt("vp1", ViewportKind::Notes, Box2(Point2(30.0, 40.0), Point2(80.0, 80.0))),
                  viewportAt("vp2", ViewportKind::Notes, Box2(Point2(100.0, 50.0), Point2(130.0, 120.0))),
                  viewportAt("vp3", ViewportKind::Notes, Box2(Point2(300.0, 45.0), Point2(360.0, 60.0)))});
    const std::vector<std::string> ids{"vp1", "vp2", "vp3"};
    const std::size_t before = document.history().undoCount();

    const auto aligned = alignViewports(document, 0, ids, AlignEdge::Bottom);
    ASSERT_TRUE(aligned.ok()) << aligned.error().describe();
    EXPECT_EQ(*aligned, (std::vector<std::string>{"vp2", "vp3"}));
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(viewportOf(document, "vp2").rect.min.y, 40.0);
    EXPECT_EQ(viewportOf(document, "vp3").rect.min.y, 40.0);

    // The first ends at 80 and the last starts at 300; vp2 is 30 wide: gaps of
    // (220 - 30) / 2 = 95, so vp2 starts at 175.
    const auto spaced = distributeViewports(document, 0, ids, DistributeAxis::Horizontal);
    ASSERT_TRUE(spaced.ok()) << spaced.error().describe();
    EXPECT_EQ(*spaced, std::vector<std::string>{"vp2"});
    EXPECT_EQ(document.history().undoCount(), before + 2);
    EXPECT_NEAR(viewportOf(document, "vp2").rect.min.x, 175.0, 1e-9);

    const std::vector<std::string> unknown{"vp1", "zz"};
    EXPECT_EQ(alignViewports(document, 0, unknown, AlignEdge::Left).error().code,
              ErrorCode::NotFound);
    const std::vector<std::string> two{"vp1", "vp2"};
    EXPECT_EQ(distributeViewports(document, 0, two, DistributeAxis::Vertical).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(alignViewports(document, 1, ids, AlignEdge::Left).error().code, ErrorCode::NotFound);
    EXPECT_EQ(document.history().undoCount(), before + 2);

    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(viewportOf(document, "vp3").rect, Box2(Point2(300.0, 45.0), Point2(360.0, 60.0)));
}

TEST(SheetArrangeCommands, MatchingAnAutomaticPlanTakesTheScaleItIsDrawnAt)
{
    // A 300 x 100 m drawing in a plan filling the sheet, centred on it: with
    // 4% to spare it needs 1 : max(300000 / 370.19, 100000 / 240.38) =
    // 1 : 810.4, so it is drawn at 1 : 1000, whatever it last stored.
    Document document;
    addLine(document, Point2(0.0, 0.0), Point2(300.0, 100.0));
    Viewport plan = viewportAt("vp1", ViewportKind::Plan, tilingArea(Sheet{}));
    plan.autoScale = true;
    plan.autoCentre = true;
    plan.scale = 500.0;
    Viewport section = viewportAt("vp2", ViewportKind::LongSection,
                                  Box2(Point2(30.0, 40.0), Point2(300.0, 120.0)));
    section.scale = 200.0;
    addSheetWith(document, {plan, section});
    const std::vector<Point2> content =
        viewportContent(document.model(), viewportOf(document, "vp1"));
    EXPECT_EQ(drawnScale(viewportOf(document, "vp1"), content), 1000.0);
    // Drawn round a centre of its own at (0, 0), the painter measures the
    // drawing about that point: 600 x 200 m, 1 : 1620.8, so 1 : 2000.
    Viewport offCentre = viewportOf(document, "vp1");
    offCentre.autoCentre = false;
    EXPECT_EQ(drawnScale(offCentre, content), 2000.0);
    // A view at a scale of its own is drawn at it.
    offCentre.autoScale = false;
    EXPECT_EQ(drawnScale(offCentre, content), 500.0);
    const std::size_t before = document.history().undoCount();

    const std::vector<std::string> targets{"vp2"};
    const auto changed = matchScale(document, targets, "vp1");
    ASSERT_TRUE(changed.ok()) << changed.error().describe();
    EXPECT_EQ(*changed, std::vector<std::string>{"vp2"});
    EXPECT_EQ(viewportOf(document, "vp2").scale, 1000.0);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    // A scale given wins.
    ASSERT_TRUE(matchScale(document, targets, "vp1", 750.0).ok());
    EXPECT_EQ(viewportOf(document, "vp2").scale, 750.0);
    EXPECT_EQ(matchScale(document, targets, "vp7").error().code, ErrorCode::NotFound);
}

TEST(SheetArrangeCommands, FittingAViewToTheDrawingIsOneStep)
{
    // A 100 x 50 m line at 1 : 500 is 200 x 100 mm, and 5% more: 210 x 105
    // about the view's centre (150, 150).
    Document document;
    addLine(document, Point2(0.0, 0.0), Point2(100.0, 50.0));
    Viewport plan = viewportAt("vp1", ViewportKind::Plan, Box2(Point2(100.0, 100.0), Point2(200.0, 200.0)));
    plan.scale = 500.0;
    Viewport section = viewportAt("vp2", ViewportKind::LongSection,
                                  Box2(Point2(30.0, 220.0), Point2(300.0, 280.0)));
    addSheetWith(document, {plan, section});
    const std::size_t before = document.history().undoCount();

    ASSERT_TRUE(fitViewportToContent(document, "vp1").ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    expectBox(viewportOf(document, "vp1").rect, 45.0, 97.5, 255.0, 202.5);
    EXPECT_NEAR(viewportOf(document, "vp1").centre.x, 50.0, 1e-9);
    EXPECT_NEAR(viewportOf(document, "vp1").centre.y, 25.0, 1e-9);

    // At a scale given - an automatic view's, as drawn - 1 : 1000 halves it.
    ASSERT_TRUE(fitViewportToContent(document, "vp1", {}, 1000.0).ok());
    EXPECT_NEAR(viewportOf(document, "vp1").rect.width(), 105.0, 1e-9);
    EXPECT_EQ(viewportOf(document, "vp1").scale, 1000.0);

    // A section's ground is not in the drawing: its points must be given.
    const auto refused = fitViewportToContent(document, "vp2");
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    const std::vector<Point2> profile{Point2(0.0, 10.0), Point2(100.0, 15.0)};
    ASSERT_TRUE(fitViewportToContent(document, "vp2", profile).ok());
    EXPECT_NEAR(viewportOf(document, "vp2").rect.width(), 100.0 * 1000.0 / 500.0 * kFitSpare, 1e-9);
    EXPECT_EQ(fitViewportToContent(document, "vp9").error().code, ErrorCode::NotFound);
}

TEST(SheetArrangeCommands, APlanOfADiagonalRoadIsTurnedJustFarEnoughToReachTheNextScale)
{
    // A 424.26 m line at 45 degrees. Square, the view needs 1 : 300000 /
    // 240.38 = 1248, so 1 : 1250; laid along the diagonal it would need
    // 1 : 961, so 1 : 1000 is to be had. The least turn that reaches it
    // leaves the line's height 240.38 m: t = 45 - asin(240.38 / 424.26) =
    // 10.49 degrees.
    Document document;
    addLine(document, Point2(0.0, 0.0), Point2(300.0, 300.0));
    Viewport plan = viewportAt("vp1", ViewportKind::Plan, tilingArea(Sheet{}));
    plan.autoScale = true;
    plan.autoCentre = true;
    addSheetWith(document, {plan});
    const std::size_t before = document.history().undoCount();

    const auto fit = rotateToBestFit(document, "vp1");
    ASSERT_TRUE(fit.ok()) << fit.error().describe();
    EXPECT_EQ(document.history().undoCount(), before + 1);
    const double t = kPi / 4.0 - std::asin(250.0 / kAutoScaleSpare / std::hypot(300.0, 300.0));
    EXPECT_NEAR(fit->rotation, t, 1e-9);
    EXPECT_EQ(fit->standardScale, 1000.0);
    const Viewport& turned = viewportOf(document, "vp1");
    EXPECT_NEAR(turned.rotation, t, 1e-9);
    EXPECT_EQ(turned.scale, 1000.0);
    EXPECT_FALSE(turned.autoScale);
    EXPECT_NEAR(turned.centre.x, 150.0, 1e-9);
    EXPECT_NEAR(turned.centre.y, 150.0, 1e-9);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(viewportOf(document, "vp1").rotation, 0.0);
    EXPECT_TRUE(viewportOf(document, "vp1").autoScale);
    EXPECT_EQ(rotateToBestFit(document, "vp8").error().code, ErrorCode::NotFound);
}

TEST(SheetArrangeCommands, ChoosingPaperForTheMainPlanIsOneStep)
{
    // A 500 x 300 m drawing in a plan at 1 : 1000 filling an A3 sheet needs
    // A2 landscape (520 x 312 mm with 4% to spare, in 545.27 x 354.36).
    Document document;
    const std::vector<Point2> corners{Point2(0.0, 0.0), Point2(500.0, 0.0), Point2(500.0, 300.0),
                                      Point2(0.0, 300.0)};
    for (std::size_t i = 0; i < corners.size(); ++i) {
        addLine(document, corners[i], corners[(i + 1) % corners.size()]);
    }
    Viewport legend = viewportAt("vp1", ViewportKind::Legend, Box2(Point2(300.0, 40.0), Point2(400.0, 100.0)));
    Viewport plan = viewportAt("vp2", ViewportKind::Plan, tilingArea(Sheet{}));
    plan.scale = 1000.0;
    addSheetWith(document, {legend, plan});
    EXPECT_EQ(mainPlanOf(document.sheetSet(), 0), "vp2");
    EXPECT_EQ(mainPlanOf(document.sheetSet(), 5), "");
    const std::size_t before = document.history().undoCount();

    const auto change = choosePaperForScale(document, "vp2");
    ASSERT_TRUE(change.ok()) << change.error().describe();
    EXPECT_EQ(change->advice.paper, PaperSize::A2);
    EXPECT_TRUE(change->advice.landscape);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    const Sheet& sheet = document.sheetSet().sheets[0];
    EXPECT_EQ(sheet.paper, PaperSize::A2);
    EXPECT_EQ(viewportOf(document, "vp2").rect, tilingArea(sheet));
    EXPECT_EQ(viewportOf(document, "vp2").scale, 1000.0);

    // At a scale given: 1 : 2000 fits A4 (260 x 156 mm in 271.64 x 176.18).
    const auto smaller = choosePaperForScale(document, "vp2", {}, 2000.0);
    ASSERT_TRUE(smaller.ok());
    EXPECT_EQ(document.sheetSet().sheets[0].paper, PaperSize::A4);
    EXPECT_EQ(viewportOf(document, "vp2").scale, 2000.0);

    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet().sheets[0].paper, PaperSize::A3);
    EXPECT_EQ(choosePaperForScale(document, "vp1").error().code, ErrorCode::InvalidArgument);
}
