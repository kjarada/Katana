// The Draw > Curves tools: Circle and Arc in each construction
// (src/katana_cad/tools/draw_curves.cpp). Each is driven as a user would drive
// it - clicks, typed values and options, Undo, Enter - and what it draws is
// compared with geometry worked out by hand, the working beside each figure.
// Inputs are chosen so that the arithmetic is exact in binary wherever the
// construction allows it; where it cannot be (a square root, pi) the
// comparison is to within a few ulps or 1e-12.

#include <cmath>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/math/numerics.hpp"
#include "cad/tools/tool_driver.hpp"

namespace {

using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kHalfPi;
using katana::math::kPi;
using Outcome = ToolStep::Outcome;
namespace cmd = katana::commands;

std::vector<katana::entity::Entity> drawn(ToolDriver& driver)
{
    std::vector<katana::entity::Entity> out;
    const auto& entities = driver.document().model().entities;
    for (const EntityId id : entities.ids()) {
        out.push_back(*entities.find(id));
    }
    return out;
}

// The one circle in the drawing, or nullopt when there is not exactly one
// entity or it is not a circle.
std::optional<Circle2> theCircle(ToolDriver& driver)
{
    const auto entities = drawn(driver);
    if (entities.size() != 1) {
        return std::nullopt;
    }
    if (const auto* circle = std::get_if<Circle2>(&entities.front().geometry)) {
        return *circle;
    }
    return std::nullopt;
}

std::optional<Arc2> theArc(ToolDriver& driver)
{
    const auto entities = drawn(driver);
    if (entities.size() != 1) {
        return std::nullopt;
    }
    if (const auto* arc = std::get_if<Arc2>(&entities.front().geometry)) {
        return *arc;
    }
    return std::nullopt;
}

EntityId addLine(ToolDriver& driver, Point2 a, Point2 b)
{
    return driver.add(cmd::createLine(a, b));
}

} // namespace

// ---- the catalogue -----------------------------------------------------------------

TEST(DrawCurves, EveryConstructionIsItsOwnToolInTheDrawMenusCurvesGroup)
{
    const auto& catalog = katana::cad::toolCatalog();
    const std::vector<std::string> ids = {
        "draw.circle",     "draw.circle.diameter", "draw.circle.2p", "draw.circle.3p",
        "draw.circle.ttr", "draw.arc",             "draw.arc.sce",   "draw.arc.cse",
        "draw.arc.ser"};
    int previousOrder = -1;
    for (const std::string& id : ids) {
        const katana::cad::ToolInfo* info = catalog.find(id);
        ASSERT_NE(info, nullptr) << id;
        EXPECT_EQ(info->category, "Draw") << id;
        EXPECT_EQ(info->group, "Curves") << id;
        EXPECT_FALSE(info->tip.empty()) << id;
        // Listed above in menu order: circles first, then arcs.
        EXPECT_GT(info->order, previousOrder) << id;
        previousOrder = info->order;
    }
    EXPECT_EQ(catalog.find("draw.circle.3p")->name, "Circle, 3 Points");
}

TEST(DrawCurves, CircleAndArcAreStartedByTheirAutoCadVerbsInAnyCase)
{
    const auto& catalog = katana::cad::toolCatalog();
    for (const char* verb : {"CIRCLE", "C", "circle", "c"}) {
        ASSERT_NE(catalog.findByAlias(verb), nullptr) << verb;
        EXPECT_EQ(catalog.findByAlias(verb)->id, "draw.circle") << verb;
    }
    for (const char* verb : {"ARC", "A", "arc", "a"}) {
        ASSERT_NE(catalog.findByAlias(verb), nullptr) << verb;
        EXPECT_EQ(catalog.findByAlias(verb)->id, "draw.arc") << verb;
    }
}

// ---- Circle: centre and radius -----------------------------------------------------

TEST(DrawCurves, ACircleFromACentreAndAPointOnItHasTheirDistanceForItsRadius)
{
    ToolDriver driver;
    driver.start("draw.circle");
    EXPECT_EQ(driver.tool().prompt(), "Specify centre point or [3P/2P/Ttr]");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_EQ(driver.click(10.0, 20.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify radius or [Diameter/Undo]");
    EXPECT_EQ(driver.click(13.0, 24.0).outcome, Outcome::Done);
    // (13, 24) - (10, 20) = (3, 4): a 3-4-5 triangle, radius 5 exactly.
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{10.0, 20.0}, 5.0}));
}

TEST(DrawCurves, ANumberTypedAtTheRadiusPromptIsTheRadius)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.type("10,20");
    EXPECT_EQ(driver.type("12.5").outcome, Outcome::Done);
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{10.0, 20.0}, 12.5}));
}

TEST(DrawCurves, RelativeAndPolarPointsAtTheRadiusPromptMeasureFromTheCentre)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.type("10,20");
    // @3,4 from the centre is (13, 24): radius 5, as the clicked case.
    EXPECT_EQ(driver.type("@3,4").outcome, Outcome::Done);
    ASSERT_EQ(drawn(driver).size(), 1u);
    EXPECT_EQ(std::get<Circle2>(drawn(driver)[0].geometry), (Circle2{{10.0, 20.0}, 5.0}));

    ASSERT_TRUE(driver.document().undo().ok());
    (void)driver.type("10,20");
    // 5 at 90 degrees is straight up. cos(90 deg) is ~6e-17, not 0, so the
    // offset is (3e-16, 5) - but 3e-16 is under half an ulp of 10 (1.8e-15),
    // so the point rounds to (10, 25) and the radius is 5.
    EXPECT_EQ(driver.type("@5<90").outcome, Outcome::Done);
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(circle->center, Point2(10.0, 20.0));
    EXPECT_DOUBLE_EQ(circle->radius, 5.0);
}

TEST(DrawCurves, TheDiameterOptionTakesATypedOrPickedDiameter)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.click(10.0, 20.0);
    EXPECT_EQ(driver.type("d").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify diameter or [Radius/Undo]");
    EXPECT_EQ(driver.type("10").outcome, Outcome::Done);
    auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{10.0, 20.0}, 5.0})); // diameter 10 / 2

    ASSERT_TRUE(driver.document().undo().ok());
    (void)driver.click(10.0, 20.0);
    (void)driver.type("DIAMETER");
    // A picked point is the far end of the diameter from the centre: 10 away
    // along x, so the radius is 5.
    EXPECT_EQ(driver.click(20.0, 20.0).outcome, Outcome::Done);
    circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{10.0, 20.0}, 5.0}));
}

TEST(DrawCurves, TheDiameterVariantAsksForADiameterAndCanSwitchToARadius)
{
    ToolDriver driver;
    driver.start("draw.circle.diameter");
    // Not the general CIRCLE: no other constructions are offered.
    EXPECT_EQ(driver.tool().prompt(), "Specify centre point");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify diameter or [Radius/Undo]");
    EXPECT_EQ(driver.type("8").outcome, Outcome::Done);
    auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{0.0, 0.0}, 4.0}));

    ASSERT_TRUE(driver.document().undo().ok());
    (void)driver.click(0.0, 0.0);
    (void)driver.type("R");
    EXPECT_EQ(driver.type("8").outcome, Outcome::Done);
    circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{0.0, 0.0}, 8.0}));
}

TEST(DrawCurves, CircleStartsAgainAfterEachCircleUntilTheUserStopsIt)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.click(0.0, 0.0);
    (void)driver.type("1");
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().prompt(), "Specify centre point or [3P/2P/Ttr]");
    (void)driver.click(10.0, 0.0);
    (void)driver.type("2");
    EXPECT_EQ(driver.executed(), 2);
    EXPECT_EQ(drawn(driver).size(), 2u);
}

TEST(DrawCurves, CircleRefusesANonPositiveOrMalformedRadiusAndKeepsItsCentre)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.click(10.0, 20.0);
    const std::string prompt = driver.tool().prompt();
    for (const char* bad : {"0", "-3", "abc", "1e999"}) {
        const ToolStep step = driver.type(bad);
        EXPECT_EQ(step.outcome, Outcome::Rejected) << bad;
        EXPECT_FALSE(step.message.empty()) << bad;
        EXPECT_EQ(driver.tool().prompt(), prompt) << bad;
    }
    // A point on the centre is a zero radius.
    EXPECT_EQ(driver.click(10.0, 20.0).outcome, Outcome::Rejected);
    EXPECT_TRUE(drawn(driver).empty());
    // Still at the radius prompt with the centre kept.
    EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{10.0, 20.0}, 5.0}));
}

TEST(DrawCurves, UndoInsideCircleStepsBackOverTheCentreAndOverAnOption)
{
    ToolDriver driver;
    driver.start("draw.circle");
    // Nothing to step back over yet.
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected);
    (void)driver.click(1.0, 1.0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify centre point or [3P/2P/Ttr]");
    (void)driver.click(10.0, 20.0);
    (void)driver.type("D");
    // U typed is the same as the Undo button, and takes back the option only.
    EXPECT_EQ(driver.type("u").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify radius or [Diameter/Undo]");
    EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{10.0, 20.0}, 5.0})); // a radius of 5, not a diameter
}

TEST(DrawCurves, EnterPartWayThroughACircleEndsTheToolWithNothingDrawn)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.click(10.0, 20.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_TRUE(drawn(driver).empty());
}

TEST(DrawCurves, OneUndoOfTheDocumentRemovesTheCircle)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.click(10.0, 20.0);
    (void)driver.type("5");
    ASSERT_EQ(drawn(driver).size(), 1u);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(drawn(driver).empty());
}

TEST(DrawCurves, ACircleIsDrawnWithTheDocumentsCurrentLayer)
{
    ToolDriver driver;
    katana::entity::Layer kerb;
    kerb.name = "kerb";
    ASSERT_TRUE(driver.document().execute(cmd::createLayer(kerb)).ok());
    ASSERT_TRUE(driver.document().setCurrentLayer("kerb").ok());
    driver.start("draw.circle");
    (void)driver.click(0.0, 0.0);
    (void)driver.type("2");
    ASSERT_EQ(drawn(driver).size(), 1u);
    EXPECT_EQ(drawn(driver)[0].layer, "kerb");
}

TEST(DrawCurves, TheCirclePreviewIsTheCircleThroughTheCursorWithItsRadius)
{
    ToolDriver driver;
    driver.start("draw.circle");
    EXPECT_TRUE(driver.tool().preview({13.0, 24.0}).shapes.empty());
    (void)driver.click(10.0, 20.0);
    const auto feedback = driver.tool().preview({13.0, 24.0});
    ASSERT_EQ(feedback.shapes.size(), 2u);
    EXPECT_EQ(std::get<Circle2>(feedback.shapes[0]), (Circle2{{10.0, 20.0}, 5.0}));
    EXPECT_EQ(std::get<Segment2>(feedback.shapes[1]), (Segment2{{10.0, 20.0}, {13.0, 24.0}}));
    ASSERT_EQ(feedback.markers.size(), 1u);
    EXPECT_EQ(feedback.markers[0], Point2(10.0, 20.0));
}

TEST(DrawCurves, TheDiameterPreviewIsTheCircleWhoseDiameterReachesTheCursor)
{
    ToolDriver driver;
    driver.start("draw.circle.diameter");
    (void)driver.click(10.0, 20.0);
    // The cursor is 10 from the centre along x. At the diameter prompt that
    // distance is the diameter, so the rubber band is the circle of radius 5 -
    // the one a click there draws (TheDiameterOptionTakesATypedOrPickedDiameter)
    // - and not the circle of radius 10 through the cursor.
    const auto feedback = driver.tool().preview({20.0, 20.0});
    ASSERT_EQ(feedback.shapes.size(), 2u);
    EXPECT_EQ(std::get<Circle2>(feedback.shapes[0]), (Circle2{{10.0, 20.0}, 5.0}));
    EXPECT_EQ(std::get<Segment2>(feedback.shapes[1]), (Segment2{{10.0, 20.0}, {20.0, 20.0}}));
    ASSERT_EQ(feedback.markers.size(), 1u);
    EXPECT_EQ(feedback.markers[0], Point2(10.0, 20.0));
}

// ---- Circle: two points ------------------------------------------------------------

TEST(DrawCurves, ATwoPointCircleHasItsDiameterBetweenThePoints)
{
    ToolDriver driver;
    driver.start("draw.circle.2p");
    EXPECT_EQ(driver.tool().prompt(), "Specify first end of the circle's diameter");
    (void)driver.click(2.0, 4.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify second end of the circle's diameter or [Undo]");
    EXPECT_EQ(driver.click(8.0, 12.0).outcome, Outcome::Done);
    // Centre midway, (5, 8); the ends are (6, 8) apart, 10, so the radius is 5.
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{5.0, 8.0}, 5.0}));
}

TEST(DrawCurves, TheGeneralCircleTakes2PAndPolarInputFromTheFirstEnd)
{
    ToolDriver driver;
    driver.start("draw.circle");
    EXPECT_EQ(driver.type("2p").outcome, Outcome::Continue);
    (void)driver.type("2,4");
    // 10 due east of (2, 4) is (12, 4) exactly (cos 0 and sin 0 are exact):
    // centre (7, 4), radius 5.
    EXPECT_EQ(driver.type("@10<0").outcome, Outcome::Done);
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{7.0, 4.0}, 5.0}));
}

TEST(DrawCurves, ATwoPointCircleRefusesCoincidentEnds)
{
    ToolDriver driver;
    driver.start("draw.circle.2p");
    (void)driver.click(2.0, 4.0);
    const ToolStep step = driver.click(2.0, 4.0);
    EXPECT_EQ(step.outcome, Outcome::Rejected);
    EXPECT_FALSE(step.message.empty());
    EXPECT_TRUE(drawn(driver).empty());
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify first end of the circle's diameter");
}

TEST(DrawCurves, TheTwoPointPreviewIsTheCircleOnTheFirstEndAndTheCursor)
{
    ToolDriver driver;
    driver.start("draw.circle.2p");
    (void)driver.click(2.0, 4.0);
    const auto feedback = driver.tool().preview({8.0, 12.0});
    ASSERT_FALSE(feedback.shapes.empty());
    EXPECT_EQ(std::get<Circle2>(feedback.shapes[0]), (Circle2{{5.0, 8.0}, 5.0}));
}

// ---- Circle: three points ----------------------------------------------------------

TEST(DrawCurves, AThreePointCircleIsTheCircumcircleOfThePoints)
{
    ToolDriver driver;
    driver.start("draw.circle.3p");
    EXPECT_EQ(driver.tool().prompt(), "Specify first point on the circle");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 10.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify third point on the circle or [Undo]");
    EXPECT_EQ(driver.click(-10.0, 0.0).outcome, Outcome::Done);
    // All three are 10 from the origin, which is therefore the centre.
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{0.0, 0.0}, 10.0}));
}

TEST(DrawCurves, AThreePointCircleRefusesPointsInALineAndCoincidentPoints)
{
    ToolDriver driver;
    driver.start("draw.circle");
    (void)driver.type("3P");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(0.0, 0.0).outcome, Outcome::Rejected); // on the first
    (void)driver.click(5.0, 5.0);
    const ToolStep inLine = driver.click(10.0, 10.0);
    EXPECT_EQ(inLine.outcome, Outcome::Rejected);
    EXPECT_NE(inLine.message.find("line"), std::string::npos);
    EXPECT_EQ(driver.click(5.0, 5.0).outcome, Outcome::Rejected); // on the second
    EXPECT_TRUE(drawn(driver).empty());
    // The tool kept both points. Through (0,0), (5,5) and (10,0): the centre is
    // on x = 5, the bisector of the first and last, and (5,y) is as far from
    // (0,0) as from (5,5) when 25 + y^2 = (5 - y)^2, so y = 0 and r = 5.
    EXPECT_EQ(driver.click(10.0, 0.0).outcome, Outcome::Done);
    const auto circle = theCircle(driver);
    ASSERT_TRUE(circle);
    EXPECT_EQ(*circle, (Circle2{{5.0, 0.0}, 5.0}));
}

TEST(DrawCurves, TheThreePointPreviewIsTheCircleThroughTheCursor)
{
    ToolDriver driver;
    driver.start("draw.circle.3p");
    (void)driver.click(10.0, 0.0);
    EXPECT_EQ(std::get<Segment2>(driver.tool().preview({0.0, 10.0}).shapes.at(0)),
              (Segment2{{10.0, 0.0}, {0.0, 10.0}}));
    (void)driver.click(0.0, 10.0);
    const auto feedback = driver.tool().preview({-10.0, 0.0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(std::get<Circle2>(feedback.shapes[0]), (Circle2{{0.0, 0.0}, 10.0}));
    EXPECT_EQ(feedback.markers.size(), 2u);
}

// ---- Circle: tangent, tangent, radius ----------------------------------------------

TEST(DrawCurves, ATangentTangentRadiusCircleSitsInTheCornerOfTwoPerpendicularLines)
{
    ToolDriver driver;
    const EntityId x = addLine(driver, {0.0, 0.0}, {20.0, 0.0});
    const EntityId y = addLine(driver, {0.0, 0.0}, {0.0, 20.0});
    driver.start("draw.circle.ttr");
    EXPECT_EQ(driver.tool().prompt(), "Select the first line to touch");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(driver.pick(x, 10.0, 0.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Select the second line to touch or [Undo]");
    EXPECT_EQ(driver.pick(y, 0.0, 10.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify radius of circle or [Undo]");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
    // Touching y = 0 and x = 0 with radius 5, in the quadrant both picks are
    // in: the centre is 5 from each axis, (5, 5).
    const auto entities = drawn(driver);
    ASSERT_EQ(entities.size(), 3u);
    EXPECT_EQ(std::get<Circle2>(entities[2].geometry), (Circle2{{5.0, 5.0}, 5.0}));
}

TEST(DrawCurves, OfTheFourTangentCirclesTheOneTakenIsInTheCornerWhoseArmsWerePicked)
{
    // Two lines crossing at the origin make four corners, and a circle of
    // radius 5 fits in each, centred at (+-5, +-5). The picks say which: the
    // circle is between the half of each line on the side it was picked.
    struct Case {
        Point2 onX;
        Point2 onY;
        Point2 centre;
    };
    for (const Case& c : {Case{{10.0, 0.0}, {0.0, 10.0}, {5.0, 5.0}},
                          Case{{-10.0, 0.0}, {0.0, 10.0}, {-5.0, 5.0}},
                          Case{{-10.0, 0.0}, {0.0, -10.0}, {-5.0, -5.0}},
                          Case{{10.0, 0.0}, {0.0, -10.0}, {5.0, -5.0}}}) {
        ToolDriver driver;
        const EntityId x = addLine(driver, {-20.0, 0.0}, {20.0, 0.0});
        const EntityId y = addLine(driver, {0.0, -20.0}, {0.0, 20.0});
        driver.start("draw.circle.ttr");
        (void)driver.pick(x, c.onX.x, c.onX.y);
        (void)driver.pick(y, c.onY.x, c.onY.y);
        ASSERT_EQ(driver.type("5").outcome, Outcome::Done);
        const auto entities = drawn(driver);
        ASSERT_EQ(entities.size(), 3u);
        EXPECT_EQ(std::get<Circle2>(entities[2].geometry), (Circle2{c.centre, 5.0}))
            << c.onX.x << "," << c.onY.y;
    }
}

TEST(DrawCurves, ATangentTangentRadiusCircleFitsAnObliqueCorner)
{
    ToolDriver driver;
    const EntityId x = addLine(driver, {0.0, 0.0}, {20.0, 0.0});
    const EntityId slope = addLine(driver, {0.0, 0.0}, {6.0, 8.0});
    driver.start("draw.circle");
    EXPECT_EQ(driver.type("ttr").outcome, Outcome::Continue);
    (void)driver.pick(x, 10.0, 0.0);
    (void)driver.pick(slope, 3.0, 4.0);
    EXPECT_EQ(driver.type("4").outcome, Outcome::Done);
    // Arms u1 = (1, 0) and u2 = (0.6, 0.8); sin of the angle between them is
    // 0.8. The centre c = (u1 + u2) r / sin = (1.6, 0.8) * 4 / 0.8 = (8, 4).
    // Check: 4 above y = 0, and |0.6*4 - 0.8*8| = 4 from the sloping line.
    const auto entities = drawn(driver);
    ASSERT_EQ(entities.size(), 3u);
    const auto circle = std::get<Circle2>(entities[2].geometry);
    EXPECT_NEAR(circle.center.x, 8.0, 1e-12);
    EXPECT_NEAR(circle.center.y, 4.0, 1e-12);
    EXPECT_EQ(circle.radius, 4.0);
}

TEST(DrawCurves, ATangentTangentRadiusCircleTouchesTwoSegmentsOfOnePolyline)
{
    ToolDriver driver;
    Polyline2 corner;
    corner.vertices = {{0.0, 0.0}, {20.0, 0.0}, {20.0, 20.0}};
    const EntityId id = driver.add(cmd::createPolyline(corner));
    driver.start("draw.circle.ttr");
    EXPECT_EQ(driver.pick(id, 10.0, 0.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.pick(id, 20.0, 10.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
    // The corner is (20, 0), its arms running west and north: the centre is
    // 5 west and 5 north of it, (15, 5).
    const auto entities = drawn(driver);
    ASSERT_EQ(entities.size(), 2u);
    EXPECT_EQ(std::get<Circle2>(entities[1].geometry), (Circle2{{15.0, 5.0}, 5.0}));
}

TEST(DrawCurves, APolylinesZeroLengthSegmentIsPassedOverForTheSegmentBesideIt)
{
    // A repeated vertex makes a segment of no length at (0, 0), and a pick at
    // (-1, 0) is 1 from it and 1 from the real segment after it: a tie the
    // zero-length one would win by coming first, and then be refused as
    // having no direction. The real segment, y = 0, is the line meant.
    ToolDriver driver;
    Polyline2 repeated;
    repeated.vertices = {{0.0, 0.0}, {0.0, 0.0}, {20.0, 0.0}};
    const EntityId id = driver.add(cmd::createPolyline(repeated));
    const EntityId y = addLine(driver, {0.0, 0.0}, {0.0, 20.0});
    driver.start("draw.circle.ttr");
    EXPECT_EQ(driver.pick(id, -1.0, 0.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.pick(y, 0.0, 10.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
    // The arms run west (towards the pick at x = -1) and north: centre (-5, 5).
    EXPECT_EQ(std::get<Circle2>(drawn(driver).back().geometry), (Circle2{{-5.0, 5.0}, 5.0}));
}

TEST(DrawCurves, TangentTangentRadiusRefusesWhatHasNoCornerToFitIn)
{
    ToolDriver driver;
    const EntityId bottom = addLine(driver, {0.0, 0.0}, {20.0, 0.0});
    const EntityId top = addLine(driver, {0.0, 10.0}, {20.0, 10.0});
    const EntityId side = addLine(driver, {0.0, -5.0}, {0.0, 20.0});
    const EntityId ring = driver.add(cmd::createCircle({50.0, 50.0}, 3.0));
    driver.start("draw.circle.ttr");

    EXPECT_EQ(driver.pick(ring, 53.0, 50.0).outcome, Outcome::Rejected); // not a line
    EXPECT_EQ(driver.pick(999, 0.0, 0.0).outcome, Outcome::Rejected);    // not in the drawing
    EXPECT_EQ(driver.click(5.0, 5.0).outcome, Outcome::Rejected);        // a point, not a line
    EXPECT_EQ(driver.type("5").outcome, Outcome::Rejected);              // not yet a radius
    EXPECT_EQ(driver.tool().prompt(), "Select the first line to touch");

    ASSERT_EQ(driver.pick(bottom, 10.0, 0.0).outcome, Outcome::Continue);
    const ToolStep parallel = driver.pick(top, 10.0, 10.0);
    EXPECT_EQ(parallel.outcome, Outcome::Rejected);
    EXPECT_NE(parallel.message.find("parallel"), std::string::npos);
    EXPECT_EQ(driver.pick(bottom, 15.0, 0.0).outcome, Outcome::Rejected); // the same line
    // The side crosses the bottom at the origin; picked there, it does not
    // say which corner is meant.
    EXPECT_EQ(driver.pick(side, 0.0, 0.0).outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), "Select the second line to touch or [Undo]");

    ASSERT_EQ(driver.pick(side, 0.0, 10.0).outcome, Outcome::Continue);
    for (const char* bad : {"0", "-1", "abc", "3,4"}) {
        EXPECT_EQ(driver.type(bad).outcome, Outcome::Rejected) << bad;
    }
    EXPECT_EQ(driver.click(5.0, 5.0).outcome, Outcome::Rejected);
    EXPECT_EQ(drawn(driver).size(), 4u);
    EXPECT_EQ(driver.type("2").outcome, Outcome::Done);
    EXPECT_EQ(std::get<Circle2>(drawn(driver).back().geometry), (Circle2{{2.0, 2.0}, 2.0}));
}

TEST(DrawCurves, UndoInsideTangentTangentRadiusForgetsTheLastLine)
{
    ToolDriver driver;
    const EntityId x = addLine(driver, {0.0, 0.0}, {20.0, 0.0});
    const EntityId y = addLine(driver, {0.0, 0.0}, {0.0, 20.0});
    driver.start("draw.circle.ttr");
    (void)driver.pick(x, 10.0, 0.0);
    (void)driver.pick(y, 0.0, 10.0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Select the second line to touch or [Undo]");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Select the first line to touch");
    // Picked the other way round, the same corner and the same circle.
    (void)driver.pick(y, 0.0, 10.0);
    (void)driver.pick(x, 10.0, 0.0);
    (void)driver.type("5");
    EXPECT_EQ(std::get<Circle2>(drawn(driver).back().geometry), (Circle2{{5.0, 5.0}, 5.0}));
    // One undo of the document takes the circle and leaves the lines.
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(drawn(driver).size(), 2u);
}

TEST(DrawCurves, TheTangentTangentRadiusPreviewMarksThePicksAndTheCorner)
{
    ToolDriver driver;
    const EntityId x = addLine(driver, {0.0, 0.0}, {20.0, 0.0});
    const EntityId y = addLine(driver, {0.0, 0.0}, {0.0, 20.0});
    driver.start("draw.circle.ttr");
    (void)driver.pick(x, 10.0, 0.5); // a pick beside the line marks the line
    (void)driver.pick(y, 0.0, 10.0);
    const auto feedback = driver.tool().preview({30.0, 30.0});
    ASSERT_EQ(feedback.markers.size(), 3u);
    EXPECT_EQ(feedback.markers[0], Point2(10.0, 0.0));
    EXPECT_EQ(feedback.markers[1], Point2(0.0, 10.0));
    EXPECT_EQ(feedback.markers[2], Point2(0.0, 0.0));
}

// ---- Arc: three points -------------------------------------------------------------

TEST(DrawCurves, AThreePointArcRunsCounterClockwiseThroughItsPoints)
{
    ToolDriver driver;
    driver.start("draw.arc");
    EXPECT_EQ(driver.tool().prompt(), "Specify start point of arc or [Centre]");
    (void)driver.click(10.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point of arc or [Centre/End/Undo]");
    (void)driver.click(0.0, 10.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify end point of arc or [Undo]");
    EXPECT_EQ(driver.click(-10.0, 0.0).outcome, Outcome::Done);
    // The upper half of the circle of radius 10 about the origin: from 0
    // counter-clockwise through pi/2 to pi, a sweep of pi.
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, 0.0, kPi}));
}

TEST(DrawCurves, AThreePointArcPickedClockwiseIsStoredCounterClockwiseFromItsOtherEnd)
{
    ToolDriver driver;
    driver.start("draw.arc");
    (void)driver.click(-10.0, 0.0);
    (void)driver.click(0.0, 10.0);
    (void)driver.click(10.0, 0.0);
    // The same upper half circle, picked clockwise from (-10, 0): stored from
    // (10, 0) at angle 0, counter-clockwise through pi.
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, 0.0, kPi}));
}

TEST(DrawCurves, AThreePointArcTakesRelativePointsFromThePointBefore)
{
    ToolDriver driver;
    driver.start("draw.arc");
    (void)driver.type("10,0");
    (void)driver.type("@-10,10"); // (0, 10)
    EXPECT_EQ(driver.type("@-10,-10").outcome, Outcome::Done); // (-10, 0)
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, 0.0, kPi}));
}

TEST(DrawCurves, AThreePointArcRefusesPointsInALineAndCoincidentPoints)
{
    ToolDriver driver;
    driver.start("draw.arc");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(0.0, 0.0).outcome, Outcome::Rejected);
    (void)driver.click(5.0, 5.0);
    const ToolStep inLine = driver.click(10.0, 10.0);
    EXPECT_EQ(inLine.outcome, Outcome::Rejected);
    EXPECT_NE(inLine.message.find("line"), std::string::npos);
    EXPECT_EQ(driver.click(0.0, 0.0).outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("12").outcome, Outcome::Rejected); // a number is not a point
    EXPECT_TRUE(drawn(driver).empty());
    EXPECT_EQ(driver.tool().prompt(), "Specify end point of arc or [Undo]");
}

TEST(DrawCurves, UndoInsideArcStepsBackOnePointAndEnterEndsIt)
{
    ToolDriver driver;
    driver.start("draw.arc");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(3.0, 3.0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    (void)driver.click(0.0, 10.0);
    (void)driver.click(-10.0, 0.0);
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, 0.0, kPi}));
    // It started again; Enter now ends it.
    EXPECT_FALSE(driver.finished());
    (void)driver.click(1.0, 1.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(drawn(driver).size(), 1u);
}

TEST(DrawCurves, OneUndoOfTheDocumentRemovesTheArc)
{
    ToolDriver driver;
    driver.start("draw.arc");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 10.0);
    (void)driver.click(-10.0, 0.0);
    ASSERT_EQ(drawn(driver).size(), 1u);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(drawn(driver).empty());
}

TEST(DrawCurves, TheThreePointArcPreviewIsTheArcThroughTheCursor)
{
    ToolDriver driver;
    driver.start("draw.arc");
    (void)driver.click(10.0, 0.0);
    EXPECT_EQ(std::get<Segment2>(driver.tool().preview({0.0, 10.0}).shapes.at(0)),
              (Segment2{{10.0, 0.0}, {0.0, 10.0}}));
    (void)driver.click(0.0, 10.0);
    const auto feedback = driver.tool().preview({-10.0, 0.0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(std::get<Arc2>(feedback.shapes[0]), (Arc2{{0.0, 0.0}, 10.0, 0.0, kPi}));
    EXPECT_EQ(feedback.markers.size(), 2u);
}

// ---- Arc: start, centre, end / centre, start, end ----------------------------------

TEST(DrawCurves, AStartCentreEndArcRunsCounterClockwiseToTheEndsDirection)
{
    ToolDriver driver;
    driver.start("draw.arc.sce");
    EXPECT_EQ(driver.tool().prompt(), "Specify start point of arc");
    (void)driver.click(10.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify centre point of arc or [Undo]");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(),
              "Specify end point of arc, counter-clockwise from the start or [Angle/Undo]");
    // The end only aims: (0, 5) is inside the circle, due north of the
    // centre, so the arc ends at (0, 10). Radius 10 from the start; 0 to pi/2.
    EXPECT_EQ(driver.click(0.0, 5.0).outcome, Outcome::Done);
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, 0.0, kHalfPi}));
}

TEST(DrawCurves, AStartCentreEndArcGoesTheLongWayWhenTheEndIsClockwiseOfTheStart)
{
    ToolDriver driver;
    driver.start("draw.arc.sce");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, -10.0);
    // Due south is -pi/2; counter-clockwise from 0 that is three quarters of
    // a turn, 3 pi / 2 - never the quarter turn clockwise.
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(arc->startAngle, 0.0);
    EXPECT_DOUBLE_EQ(arc->sweep, 3.0 * kHalfPi);
}

TEST(DrawCurves, TheAngleOptionTurnsTheArcThroughATypedAngleEitherWay)
{
    ToolDriver driver;
    driver.start("draw.arc.sce");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.type("a").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_EQ(driver.tool().prompt(),
              "Specify included angle in degrees (negative for clockwise) or [Undo]");
    EXPECT_EQ(driver.type("90").outcome, Outcome::Done);
    auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(arc->startAngle, 0.0);
    EXPECT_DOUBLE_EQ(arc->sweep, kHalfPi);

    ASSERT_TRUE(driver.document().undo().ok());
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 0.0);
    (void)driver.type("ANGLE");
    // A quarter turn clockwise from (10, 0) ends at (0, -10); stored
    // counter-clockwise from there: start 3 pi / 2, sweep pi / 2.
    EXPECT_EQ(driver.type("-90").outcome, Outcome::Done);
    arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_DOUBLE_EQ(arc->startAngle, 3.0 * kHalfPi);
    EXPECT_DOUBLE_EQ(arc->sweep, kHalfPi);
}

TEST(DrawCurves, ACentreStartEndArcTakesTheCentreFirst)
{
    ToolDriver driver;
    driver.start("draw.arc.cse");
    EXPECT_EQ(driver.tool().prompt(), "Specify centre point of arc");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify start point of arc or [Undo]");
    (void)driver.click(0.0, 10.0);
    // Start due north (pi/2), end aimed due west (pi): a quarter turn.
    EXPECT_EQ(driver.click(-3.0, 0.0).outcome, Outcome::Done);
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, kHalfPi, kHalfPi}));
}

TEST(DrawCurves, TheGeneralArcReachesEachConstructionByItsOption)
{
    // C first: centre, start, end.
    {
        ToolDriver driver;
        driver.start("draw.arc");
        EXPECT_EQ(driver.type("C").outcome, Outcome::Continue);
        EXPECT_EQ(driver.tool().prompt(), "Specify centre point of arc or [Undo]");
        (void)driver.click(0.0, 0.0);
        (void)driver.click(0.0, 10.0);
        (void)driver.click(-3.0, 0.0);
        const auto arc = theArc(driver);
        ASSERT_TRUE(arc);
        EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, kHalfPi, kHalfPi}));
    }
    // C after the start: start, centre, end.
    {
        ToolDriver driver;
        driver.start("draw.arc");
        (void)driver.click(10.0, 0.0);
        EXPECT_EQ(driver.type("centre").outcome, Outcome::Continue);
        EXPECT_EQ(driver.tool().prompt(), "Specify centre point of arc or [Undo]");
        (void)driver.click(0.0, 0.0);
        (void)driver.click(0.0, 5.0);
        const auto arc = theArc(driver);
        ASSERT_TRUE(arc);
        EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, 0.0, kHalfPi}));
    }
    // E after the start: start, end, radius (the radius case is worked in
    // AStartEndRadiusArcTakesTheMinorArcForAPositiveRadius).
    {
        ToolDriver driver;
        driver.start("draw.arc");
        (void)driver.click(0.0, 0.0);
        EXPECT_EQ(driver.type("e").outcome, Outcome::Continue);
        EXPECT_EQ(driver.tool().prompt(), "Specify end point of arc or [Undo]");
        (void)driver.click(8.0, 0.0);
        EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
        const auto arc = theArc(driver);
        ASSERT_TRUE(arc);
        EXPECT_EQ(arc->center, Point2(4.0, 3.0));
        EXPECT_EQ(arc->radius, 5.0);
    }
}

TEST(DrawCurves, UndoInsideTheGeneralArcTakesBackAnOption)
{
    ToolDriver driver;
    driver.start("draw.arc");
    (void)driver.click(10.0, 0.0);
    (void)driver.type("C");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point of arc or [Centre/End/Undo]");
    (void)driver.click(0.0, 10.0);
    (void)driver.click(-10.0, 0.0);
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{0.0, 0.0}, 10.0, 0.0, kPi}));
}

TEST(DrawCurves, ACentredArcRefusesAZeroRadiusAnEndWithNoDirectionAndAnEmptySweep)
{
    ToolDriver driver;
    driver.start("draw.arc.sce");
    (void)driver.click(10.0, 0.0);
    EXPECT_EQ(driver.click(10.0, 0.0).outcome, Outcome::Rejected); // centre on the start
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(0.0, 0.0).outcome, Outcome::Rejected); // end on the centre
    const ToolStep along = driver.click(20.0, 0.0);               // the start's own direction
    EXPECT_EQ(along.outcome, Outcome::Rejected);
    EXPECT_NE(along.message.find("empty"), std::string::npos);
    EXPECT_EQ(driver.type("xyz").outcome, Outcome::Rejected);
    (void)driver.type("A");
    for (const char* bad : {"0", "360", "-400", "abc"}) {
        EXPECT_EQ(driver.type(bad).outcome, Outcome::Rejected) << bad;
    }
    EXPECT_EQ(driver.click(0.0, 10.0).outcome, Outcome::Rejected); // an angle is typed
    EXPECT_TRUE(drawn(driver).empty());

    ToolDriver centreFirst;
    centreFirst.start("draw.arc.cse");
    (void)centreFirst.click(0.0, 0.0);
    EXPECT_EQ(centreFirst.click(0.0, 0.0).outcome, Outcome::Rejected); // start on the centre
    EXPECT_TRUE(drawn(centreFirst).empty());
}

TEST(DrawCurves, TheCentredArcPreviewIsTheArcToTheCursorsDirection)
{
    ToolDriver driver;
    driver.start("draw.arc.sce");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(0.0, 0.0);
    const auto feedback = driver.tool().preview({0.0, 5.0});
    ASSERT_EQ(feedback.shapes.size(), 2u);
    EXPECT_EQ(std::get<Arc2>(feedback.shapes[0]), (Arc2{{0.0, 0.0}, 10.0, 0.0, kHalfPi}));
    EXPECT_EQ(std::get<Segment2>(feedback.shapes[1]), (Segment2{{0.0, 0.0}, {0.0, 5.0}}));
}

// ---- Arc: start, end, radius -------------------------------------------------------

TEST(DrawCurves, AStartEndRadiusArcTakesTheMinorArcForAPositiveRadius)
{
    ToolDriver driver;
    driver.start("draw.arc.ser");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(),
              "Specify radius of arc (negative for the major arc) or pick its centre or [Undo]");
    EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
    // Half the chord is 4, so the centre is sqrt(25 - 16) = 3 off its middle
    // (4, 0) - on the LEFT of start->end for a positive radius: (4, 3).
    // Counter-clockwise from (0,0), at atan2(-3,-4) = pi + atan(3/4), to
    // (8,0), at atan2(-3,4): a sweep of 2 atan(4/3) = 106.26 degrees, the
    // minor arc, which dips to (4, 3 - 5) = (4, -2) at its middle.
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(arc->center, Point2(4.0, 3.0));
    EXPECT_EQ(arc->radius, 5.0);
    EXPECT_NEAR(arc->startAngle, kPi + std::atan(0.75), 1e-12);
    EXPECT_NEAR(arc->sweep, 2.0 * std::atan(4.0 / 3.0), 1e-12);
    EXPECT_NEAR(arc->midpoint().x, 4.0, 1e-12);
    EXPECT_NEAR(arc->midpoint().y, -2.0, 1e-12);
    EXPECT_NEAR(arc->endPoint().x, 8.0, 1e-12);
    EXPECT_NEAR(arc->endPoint().y, 0.0, 1e-12);
}

TEST(DrawCurves, AStartEndRadiusArcTakesTheMajorArcForANegativeRadius)
{
    ToolDriver driver;
    driver.start("draw.arc.ser");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 0.0);
    EXPECT_EQ(driver.type("-5").outcome, Outcome::Done);
    // The centre on the RIGHT, (4, -3). Counter-clockwise from (0,0), at
    // atan2(3,-4), round through south to (8,0): 2 pi - 2 atan(4/3) = 253.74
    // degrees, whose middle is the bottom of the circle, (4, -3 - 5).
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(arc->center, Point2(4.0, -3.0));
    EXPECT_EQ(arc->radius, 5.0);
    EXPECT_NEAR(arc->sweep, 2.0 * kPi - 2.0 * std::atan(4.0 / 3.0), 1e-12);
    EXPECT_NEAR(arc->midpoint().x, 4.0, 1e-12);
    EXPECT_NEAR(arc->midpoint().y, -8.0, 1e-12);
}

TEST(DrawCurves, AStartEndRadiusArcOfHalfTheChordIsASemicircle)
{
    ToolDriver driver;
    driver.start("draw.arc.ser");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 0.0);
    (void)driver.type("4");
    // Centre at the middle of the chord, from pi counter-clockwise to 0 (2 pi):
    // the lower half, since counter-clockwise from the west goes south.
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(*arc, (Arc2{{4.0, 0.0}, 4.0, kPi, kPi}));
}

TEST(DrawCurves, APickedCentreForAStartEndRadiusArcIsMovedOntoTheEndsBisector)
{
    ToolDriver driver;
    driver.start("draw.arc.ser");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 0.0);
    // Every centre of an arc through both ends is on x = 4; (7, 3) goes to
    // (4, 3), the centre a radius of +5 gives.
    EXPECT_EQ(driver.click(7.0, 3.0).outcome, Outcome::Done);
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(arc->center, Point2(4.0, 3.0));
    EXPECT_EQ(arc->radius, 5.0);
    EXPECT_NEAR(arc->sweep, 2.0 * std::atan(4.0 / 3.0), 1e-12);
}

TEST(DrawCurves, APickedCentreOnTheRightOfTheChordGivesTheMajorArcAsANegativeRadiusDoes)
{
    ToolDriver driver;
    driver.start("draw.arc.ser");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 0.0);
    // (1, -3) is on the RIGHT of start->end. Its foot on the bisector x = 4 is
    // (4, -3): the centre a radius of -5 gives, so the arc is the major one.
    // The working is that of AStartEndRadiusArcTakesTheMajorArcForANegativeRadius:
    // radius |(4,-3)| = 5; counter-clockwise from (0,0), at atan2(3,-4), round
    // through south to (8,0), a sweep of 2 pi - 2 atan(4/3) = 253.74 degrees
    // whose middle is the bottom of the circle, (4, -3 - 5) = (4, -8).
    const auto feedback = driver.tool().preview({1.0, -3.0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    const auto bent = std::get<Arc2>(feedback.shapes[0]);
    EXPECT_EQ(bent.center, Point2(4.0, -3.0));
    EXPECT_NEAR(bent.sweep, 2.0 * kPi - 2.0 * std::atan(4.0 / 3.0), 1e-12);

    EXPECT_EQ(driver.click(1.0, -3.0).outcome, Outcome::Done);
    const auto arc = theArc(driver);
    ASSERT_TRUE(arc);
    EXPECT_EQ(arc->center, Point2(4.0, -3.0));
    EXPECT_EQ(arc->radius, 5.0);
    EXPECT_NEAR(arc->sweep, 2.0 * kPi - 2.0 * std::atan(4.0 / 3.0), 1e-12);
    EXPECT_NEAR(arc->midpoint().x, 4.0, 1e-12);
    EXPECT_NEAR(arc->midpoint().y, -8.0, 1e-12);
}

TEST(DrawCurves, AStartEndRadiusArcRefusesCoincidentEndsAndATooSmallRadius)
{
    ToolDriver driver;
    driver.start("draw.arc.ser");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(0.0, 0.0).outcome, Outcome::Rejected);
    (void)driver.click(8.0, 0.0);
    const ToolStep small = driver.type("3");
    EXPECT_EQ(small.outcome, Outcome::Rejected);
    // Says what the limit is: half the distance between the ends.
    EXPECT_NE(small.message.find('4'), std::string::npos);
    for (const char* bad : {"0", "-3", "abc"}) {
        EXPECT_EQ(driver.type(bad).outcome, Outcome::Rejected) << bad;
    }
    EXPECT_TRUE(drawn(driver).empty());
    EXPECT_EQ(driver.type("5").outcome, Outcome::Done);
}

TEST(DrawCurves, TheStartEndRadiusPreviewBendsTheArcAboutTheCursorsCentre)
{
    ToolDriver driver;
    driver.start("draw.arc.ser");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(8.0, 0.0);
    const auto feedback = driver.tool().preview({7.0, 3.0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    const auto arc = std::get<Arc2>(feedback.shapes[0]);
    EXPECT_EQ(arc.center, Point2(4.0, 3.0));
    EXPECT_EQ(arc.radius, 5.0);
    ASSERT_EQ(feedback.markers.size(), 3u);
    EXPECT_EQ(feedback.markers[2], Point2(4.0, 3.0));
}

TEST(DrawCurves, AnArcIsDrawnWithTheDocumentsCurrentLayerAndTheToolStartsAgain)
{
    ToolDriver driver;
    katana::entity::Layer kerb;
    kerb.name = "kerb";
    ASSERT_TRUE(driver.document().execute(cmd::createLayer(kerb)).ok());
    ASSERT_TRUE(driver.document().setCurrentLayer("kerb").ok());
    driver.start("draw.arc.cse");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, 10.0);
    (void)driver.click(-3.0, 0.0);
    ASSERT_EQ(drawn(driver).size(), 1u);
    EXPECT_EQ(drawn(driver)[0].layer, "kerb");
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().prompt(), "Specify centre point of arc");
}
