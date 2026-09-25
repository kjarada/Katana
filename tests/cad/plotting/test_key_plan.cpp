// The live key plan (key_plan.hpp): every sheet's plan outlined from the
// sheets as they are, numbered as the set numbers them now, the key plan's
// own sheet flagged, and an automatic key plan fitted to all of them.

#include <gtest/gtest.h>

#include <cmath>
#include <string>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/key_plan.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/entity/model.hpp"
#include "katana/math/numerics.hpp"

using katana::cad::Document;
using namespace katana::cad::plotting;

namespace {

Viewport planOf(std::string id, Box2 rect, double scale, Point2 centre, double rotation = 0.0)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.centre = centre;
    viewport.rotation = rotation;
    return viewport;
}

Viewport keyPlanOf(std::string id, Box2 rect)
{
    Viewport viewport = planOf(std::move(id), rect, 5000.0, Point2(1050.0, 2000.0));
    viewport.kind = ViewportKind::KeyPlan;
    return viewport;
}

Sheet sheetOf(std::string id, std::vector<Viewport> viewports)
{
    Sheet sheet;
    sheet.id = std::move(id);
    sheet.name = sheet.id;
    sheet.viewports = std::move(viewports);
    return sheet;
}

const Box2 kStrip(Point2(0.0, 0.0), Point2(100.0, 50.0)); // 100 x 50 m at 1:1000

// A key-plan sheet and two plans side by side: sheet 2 over 950..1050 x
// 1975..2025, sheet 3 over 1050..1150.
SheetSet keyAndTwoPlans()
{
    SheetSet set;
    set.sheets.push_back(
        sheetOf("s1", {keyPlanOf("vp1", Box2(Point2(30.0, 40.0), Point2(130.0, 110.0)))}));
    set.sheets.push_back(sheetOf("s2", {planOf("vp2", kStrip, 1000.0, Point2(1000.0, 2000.0))}));
    set.sheets.push_back(sheetOf("s3", {planOf("vp3", kStrip, 1000.0, Point2(1100.0, 2000.0))}));
    return set;
}

std::vector<std::string> labels(const std::vector<KeyPlanOutline>& outlines)
{
    std::vector<std::string> out;
    for (const KeyPlanOutline& outline : outlines) {
        out.push_back(outline.label);
    }
    return out;
}

} // namespace

TEST(KeyPlan, EveryOtherSheetsPlanIsOutlinedOnTheGroundAndNumbered)
{
    const SheetSet set = keyAndTwoPlans();
    const auto outlines = keyPlanOutlines(set, 0);
    ASSERT_EQ(outlines.size(), 2u);
    EXPECT_EQ(outlines[0].sheetId, "s2");
    EXPECT_EQ(outlines[0].sheetIndex, 1u);
    EXPECT_EQ(outlines[0].viewportId, "vp2");
    EXPECT_EQ(outlines[0].label, "2");
    EXPECT_FALSE(outlines[0].current);
    EXPECT_TRUE(outlines[0].labelled);
    // Counter-clockwise from the ground under the bottom-left corner.
    const std::vector<Point2> expected = {Point2(950.0, 1975.0), Point2(1050.0, 1975.0),
                                          Point2(1050.0, 2025.0), Point2(950.0, 2025.0)};
    ASSERT_EQ(outlines[0].corners.size(), 4u);
    for (std::size_t i = 0; i < 4; ++i) {
        EXPECT_NEAR(outlines[0].corners[i].x, expected[i].x, 1e-9);
        EXPECT_NEAR(outlines[0].corners[i].y, expected[i].y, 1e-9);
    }
    EXPECT_EQ(outlines[1].label, "3");
    EXPECT_NEAR(outlines[1].corners[0].x, 1050.0, 1e-9);
}

TEST(KeyPlan, TheNumbersFollowTheSheetsWhenTheyAreReordered)
{
    Document document;
    ASSERT_TRUE(addSheets(document, keyAndTwoPlans().sheets).ok());
    EXPECT_EQ(labels(keyPlanOutlines(document.sheetSet(), 0)),
              (std::vector<std::string>{"2", "3"}));

    // The tile over 1050..1150 becomes sheet 2: its outline says so, and the
    // outline stays where the sheet's plan is.
    ASSERT_TRUE(moveSheet(document, 2, 1).ok());
    const auto moved = keyPlanOutlines(document.sheetSet(), 0);
    ASSERT_EQ(moved.size(), 2u);
    EXPECT_EQ(moved[0].sheetId, "s3");
    EXPECT_EQ(moved[0].label, "2");
    EXPECT_NEAR(moved[0].corners[0].x, 1050.0, 1e-9);
    EXPECT_EQ(moved[1].sheetId, "s2");
    EXPECT_EQ(moved[1].label, "3");

    // The key plan moved to the end is still not outlined, and the numbers
    // shift with it.
    ASSERT_TRUE(moveSheet(document, 0, 2).ok());
    EXPECT_EQ(labels(keyPlanOutlines(document.sheetSet(), 2)),
              (std::vector<std::string>{"1", "2"}));

    // A removed sheet is gone from the key plan at once.
    ASSERT_TRUE(removeSheet(document, 0).ok());
    const auto after = keyPlanOutlines(document.sheetSet(), 1);
    ASSERT_EQ(after.size(), 1u);
    EXPECT_EQ(after[0].sheetId, "s2");
    EXPECT_EQ(after[0].label, "1");
}

TEST(KeyPlan, TheNumberIsTheOnePrintedOnTheSheet)
{
    SheetSet set = keyAndTwoPlans();
    set.numbering = "{set}-{n:02}";
    set.defaults.setNumber = "DS";
    set.sheets[2].fields["sheet_number"] = "X1"; // typed on the sheet: it wins
    EXPECT_EQ(labels(keyPlanOutlines(set, 0)), (std::vector<std::string>{"DS-02", "X1"}));
    EXPECT_EQ(printedSheetNumber(set, 0), "DS-01");
    EXPECT_EQ(printedSheetNumber(set, 2), "X1");
    EXPECT_EQ(printedSheetNumber(set, 3), "");
}

TEST(KeyPlan, TheKeyPlansOwnSheetIsFlaggedAsHere)
{
    // A plan with a key plan inset beside it, on sheet 2.
    SheetSet set = keyAndTwoPlans();
    set.sheets[1].viewports.push_back(
        keyPlanOf("vp4", Box2(Point2(300.0, 40.0), Point2(400.0, 110.0))));
    const auto outlines = keyPlanOutlines(set, 1);
    // Key plans are not outlined: the two plans only.
    ASSERT_EQ(outlines.size(), 2u);
    EXPECT_TRUE(outlines[0].current);
    EXPECT_EQ(outlines[0].viewportId, "vp2");
    EXPECT_FALSE(outlines[1].current);
    // The same set seen from the key-plan sheet flags neither.
    for (const KeyPlanOutline& outline : keyPlanOutlines(set, 0)) {
        EXPECT_FALSE(outline.current);
    }
}

TEST(KeyPlan, OnlyPlacedPlansAreOutlined)
{
    SheetSet set = keyAndTwoPlans();
    Viewport unplaced = planOf("vp5", Box2{}, 1000.0, Point2());
    Viewport unscaled = planOf("vp6", kStrip, 0.0, Point2());
    Viewport section = planOf("vp7", kStrip, 1000.0, Point2());
    section.kind = ViewportKind::LongSection;
    Viewport legend = planOf("vp8", kStrip, 1000.0, Point2());
    legend.kind = ViewportKind::Legend;
    set.sheets.push_back(sheetOf("s4", {unplaced, unscaled, section, legend}));
    EXPECT_EQ(keyPlanOutlines(set, 0).size(), 2u);
}

TEST(KeyPlan, ARotatedPlansOutlineIsTheGroundUnderItTurned)
{
    SheetSet set;
    // Turned a quarter: the paper's +X runs north.
    set.sheets.push_back(
        sheetOf("s1", {planOf("vp1", kStrip, 1000.0, Point2(0.0, 0.0), katana::math::kPi / 2.0)}));
    const auto outlines = keyPlanOutlines(set, 5);
    ASSERT_EQ(outlines.size(), 1u);
    // The paper's bottom-left, 50 mm left and 25 mm down of the centre, is
    // 50 m south and 25 m east.
    EXPECT_NEAR(outlines[0].corners[0].x, 25.0, 1e-9);
    EXPECT_NEAR(outlines[0].corners[0].y, -50.0, 1e-9);
    EXPECT_NEAR(outlines[0].corners[2].x, -25.0, 1e-9);
    EXPECT_NEAR(outlines[0].corners[2].y, 50.0, 1e-9);
}

TEST(KeyPlan, AnAutomaticPlanIsOutlinedWhereThePainterPutsIt)
{
    SheetSet set = keyAndTwoPlans();
    set.sheets[1].viewports[0].autoScale = true;
    const PlanPlacer place = [](const Viewport& viewport) {
        EXPECT_EQ(viewport.id, "vp2");
        return PlanPlacement{2000.0, Point2(0.0, 0.0)};
    };
    const auto placed = keyPlanOutlines(set, 0, place);
    // 100 x 50 mm at 1:2000 over the origin.
    EXPECT_NEAR(placed[0].corners[0].x, -100.0, 1e-9);
    EXPECT_NEAR(placed[0].corners[2].y, 50.0, 1e-9);
    // Without a placer, the stored scale and centre, as last drawn.
    EXPECT_NEAR(keyPlanOutlines(set, 0)[0].corners[0].x, 950.0, 1e-9);
}

TEST(KeyPlan, AnInsetOfASheetsOwnPlanDoesNotRepeatItsNumber)
{
    SheetSet set = keyAndTwoPlans();
    // A detail of sheet 2 at 1:250, inside its main plan; and a plan of
    // somewhere else on the same sheet.
    set.sheets[1].viewports.push_back(
        planOf("vp5", Box2(Point2(0.0, 60.0), Point2(40.0, 80.0)), 250.0, Point2(1010.0, 2005.0)));
    set.sheets[1].viewports.push_back(planOf("vp6", kStrip, 1000.0, Point2(5000.0, 5000.0)));
    const auto outlines = keyPlanOutlines(set, 0);
    ASSERT_EQ(outlines.size(), 4u);
    EXPECT_EQ(outlines[0].viewportId, "vp2");
    EXPECT_TRUE(outlines[0].labelled);
    EXPECT_EQ(outlines[1].viewportId, "vp5");
    EXPECT_FALSE(outlines[1].labelled);
    EXPECT_EQ(outlines[2].viewportId, "vp6");
    EXPECT_TRUE(outlines[2].labelled);
    // Sheet 3's plan overlaps sheet 2's but is another sheet: numbered.
    EXPECT_TRUE(outlines[3].labelled);
}

TEST(KeyPlan, AnAutomaticPlanIsFittedByThePaintersRule)
{
    Viewport viewport = planOf("vp1", kStrip, 500.0, Point2());
    viewport.autoScale = true;
    viewport.autoCentre = true;
    const std::vector<Point2> points = {Point2(0.0, 0.0), Point2(300.0, 100.0)};
    // 300 m across 100 mm needs 1:3000, 100 m up 50 mm 1:2000; with 4% to
    // spare 1:3120, and the ladder's next is 1:5000.
    const PlanPlacement fitted = fitPlanPlacement(viewport, points);
    EXPECT_DOUBLE_EQ(fitted.scale, 5000.0);
    EXPECT_NEAR(fitted.centre.x, 150.0, 1e-9);
    EXPECT_NEAR(fitted.centre.y, 50.0, 1e-9);

    // Only what is automatic changes.
    viewport.autoCentre = false;
    EXPECT_EQ(fitPlanPlacement(viewport, points).centre, Point2());
    viewport.autoScale = false;
    EXPECT_EQ(fitPlanPlacement(viewport, points), storedPlacement(viewport));
    viewport.autoScale = true;
    EXPECT_EQ(fitPlanPlacement(viewport, {}), storedPlacement(viewport));
}

TEST(KeyPlan, AnAutomaticKeyPlanFramesTheOutlinesAndTheDrawingOnlyWithoutThem)
{
    const SheetSet set = keyAndTwoPlans();
    Viewport key = set.sheets[0].viewports[0]; // 100 x 70 mm
    key.autoScale = true;
    key.autoCentre = true;
    const auto outlines = keyPlanOutlines(set, 0);

    // The outlines span 950..1150 x 1975..2025: 200 m across 100 mm with 4%
    // to spare is 1:2080, so 1:2500, centred on them.
    const PlanPlacement tight = fitKeyPlan(key, outlines, Box2{});
    EXPECT_DOUBLE_EQ(tight.scale, 2500.0);
    EXPECT_NEAR(tight.centre.x, 1050.0, 1e-9);
    EXPECT_NEAR(tight.centre.y, 2000.0, 1e-9);
    for (const KeyPlanOutline& outline : outlines) {
        for (const Point2& corner : outline.corners) {
            EXPECT_TRUE(key.rect.contains(planWorldToPaper(key, tight, corner)));
        }
    }

    // Drawing far off - a stray down at the origin - does not shrink the
    // sheets: the key plan frames them alone.
    const Box2 drawing(Point2(0.0, 0.0), Point2(10.0, 10.0));
    EXPECT_EQ(fitKeyPlan(key, outlines, drawing), tight);

    // With no sheet to frame it shows the drawing.
    const PlanPlacement alone = fitKeyPlan(key, {}, drawing);
    EXPECT_NEAR(alone.centre.x, 5.0, 1e-9);
    EXPECT_NEAR(alone.centre.y, 5.0, 1e-9);
    EXPECT_LT(alone.scale, tight.scale);
    // And with nothing at all it keeps its own.
    EXPECT_EQ(fitKeyPlan(key, {}, Box2{}), storedPlacement(key));
}

TEST(KeyPlan, HeadlessAPlanIsPlacedOverTheModelAndAKeyPlanOverEverySheet)
{
    // A 300 x 100 m drawing: a line, and a road along its bottom edge.
    katana::entity::Model model;
    katana::entity::Entity line;
    line.geometry = katana::geometry::Segment2{Point2(0.0, 0.0), Point2(300.0, 100.0)};
    ASSERT_TRUE(model.entities.add(line).ok());
    katana::entity::Alignment road;
    road.name = "ROAD";
    road.horizontal.pis = {{Point2(0.0, 0.0)}, {Point2(300.0, 0.0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());

    // An automatic plan over the drawing: 300 m across 100 mm needs 1:3120
    // with 4% to spare, so 1:5000, centred on the drawing.
    Viewport plan = planOf("vp2", kStrip, 500.0, Point2());
    plan.autoScale = true;
    plan.autoCentre = true;
    const PlanPlacement placed = placePlan(model, plan);
    EXPECT_DOUBLE_EQ(placed.scale, 5000.0);
    EXPECT_NEAR(placed.centre.x, 150.0, 1e-9);
    EXPECT_NEAR(placed.centre.y, 50.0, 1e-9);
    // Along the road's first 100 m: 100 m in 100 mm is 1:1040, so 1:1250.
    Viewport along = plan;
    along.source.alignment = "ROAD";
    along.source.chainageTo = 100.0;
    const PlanPlacement alongRoad = placePlan(model, along);
    EXPECT_DOUBLE_EQ(alongRoad.scale, 1250.0);
    EXPECT_NEAR(alongRoad.centre.x, 50.0, 1e-9);
    EXPECT_NEAR(alongRoad.centre.y, 0.0, 1e-9);
    // No such alignment: the drawing, as the painter falls back.
    along.source.alignment = "NOWHERE";
    EXPECT_EQ(placePlan(model, along), placed);
    // A fixed plan keeps its own; so does any plan over an empty model.
    const Viewport fixed = planOf("vp3", kStrip, 1000.0, Point2(1100.0, 2000.0));
    EXPECT_EQ(placePlan(model, fixed), storedPlacement(fixed));
    EXPECT_EQ(placePlan(katana::entity::Model{}, plan), storedPlacement(plan));

    // A key plan on sheet 1 over the automatic plan (sheet 2) and the fixed
    // one far off (sheet 3): it frames both outlines.
    SheetSet set;
    Viewport key = keyPlanOf("vp1", Box2(Point2(30.0, 40.0), Point2(130.0, 110.0)));
    key.autoScale = true;
    key.autoCentre = true;
    set.sheets.push_back(sheetOf("s1", {key}));
    set.sheets.push_back(sheetOf("s2", {plan}));
    set.sheets.push_back(sheetOf("s3", {fixed}));
    const auto outlines = keyPlanOutlines(set, 0, modelPlacer(model));
    ASSERT_EQ(outlines.size(), 2u);
    // Sheet 2's is where placePlan put it: 500 x 250 m about (150, 50).
    EXPECT_NEAR(outlines[0].corners[0].x, -100.0, 1e-9);
    EXPECT_NEAR(outlines[0].corners[2].y, 175.0, 1e-9);
    const PlanPlacement keyAt = placePlan(model, set, 0, key);
    EXPECT_EQ(keyAt, fitKeyPlan(key, outlines, Box2{}));
    for (const KeyPlanOutline& outline : outlines) {
        for (const Point2& corner : outline.corners) {
            EXPECT_TRUE(key.rect.contains(planWorldToPaper(key, keyAt, corner)));
        }
    }
    // The key plan on a sheet of its own is placed like any plan elsewhere.
    Viewport plain = key;
    plain.kind = ViewportKind::Plan;
    EXPECT_EQ(placePlan(model, set, 0, plain), placePlan(model, plain));
}
