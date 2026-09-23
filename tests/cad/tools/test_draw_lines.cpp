// The Draw > Lines tools (src/katana_cad/tools/draw_lines.cpp): Point, Line,
// Polyline, Rectangle and Polygon, each driven through ToolDriver as a user
// would drive it - clicks, typed points, options, Undo, Enter - and the
// entities it leaves in the document compared with geometry worked out by
// hand in the comments. Inputs are small integers and halves wherever the
// geometry allows, so most results are exact in binary; where a sine or a
// square root is involved the comparison is to 1e-12, a few ulps at these
// magnitudes.

#include <cmath>
#include <optional>
#include <sstream>
#include <string>
#include <utility>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/entity_commands.hpp"
#include "tool_driver.hpp"

namespace {

using katana::cad::Document;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::Entity;
using katana::entity::PointGeometry;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using Outcome = ToolStep::Outcome;

const double kRoot2 = std::sqrt(2.0);
const double kRoot3 = std::sqrt(3.0);

std::vector<Entity> entitiesOf(const Document& document)
{
    std::vector<Entity> out;
    document.model().entities.forEach([&](const Entity& entity) { out.push_back(entity); });
    return out;
}

// The document's geometry of one kind, in creation (id) order.
template <typename Shape>
std::vector<Shape> shapesOf(const Document& document)
{
    std::vector<Shape> out;
    for (const Entity& entity : entitiesOf(document)) {
        if (const auto* shape = std::get_if<Shape>(&entity.geometry)) {
            out.push_back(*shape);
        }
    }
    return out;
}

std::string text(const Point2& p)
{
    std::ostringstream out;
    out.precision(17);
    out << "(" << p.x << ", " << p.y << ")";
    return out.str();
}

::testing::AssertionResult near(const Point2& actual, const Point2& expected,
                                double tolerance = 1e-12)
{
    if (std::abs(actual.x - expected.x) <= tolerance &&
        std::abs(actual.y - expected.y) <= tolerance) {
        return ::testing::AssertionSuccess();
    }
    return ::testing::AssertionFailure()
           << text(actual) << " is not within " << tolerance << " of " << text(expected);
}

::testing::AssertionResult verticesNear(const std::vector<Point2>& actual,
                                        const std::vector<Point2>& expected,
                                        double tolerance = 1e-12)
{
    if (actual.size() != expected.size()) {
        return ::testing::AssertionFailure()
               << actual.size() << " vertices where " << expected.size() << " were expected";
    }
    for (std::size_t i = 0; i < actual.size(); ++i) {
        if (auto result = near(actual[i], expected[i], tolerance); !result) {
            return ::testing::AssertionFailure() << "vertex " << i << ": " << result.message();
        }
    }
    return ::testing::AssertionSuccess();
}

// The document's only polyline; fails the test when there is not exactly one.
std::optional<Polyline2> onlyPolyline(const Document& document)
{
    const auto polylines = shapesOf<Polyline2>(document);
    EXPECT_EQ(entitiesOf(document).size(), 1u);
    EXPECT_EQ(polylines.size(), 1u);
    return polylines.size() == 1 ? std::optional<Polyline2>(polylines.front()) : std::nullopt;
}

void expectRefused(const ToolStep& step)
{
    EXPECT_EQ(step.outcome, Outcome::Rejected);
    EXPECT_FALSE(step.message.empty()) << "a refusal says why";
}

} // namespace

// ---- the catalogue -----------------------------------------------------------------

TEST(DrawLineTools, AreListedUnderDrawLinesInMenuOrder)
{
    std::vector<std::string> ids;
    for (const katana::cad::ToolInfo* tool : katana::cad::toolCatalog().all()) {
        if (tool->category == "Draw" && tool->group == "Lines") {
            ids.push_back(tool->id);
            EXPECT_FALSE(tool->tip.empty()) << tool->id;
            EXPECT_TRUE(tool->shortcut.empty()) << tool->id;
        }
    }
    EXPECT_EQ(ids, (std::vector<std::string>{"draw.point", "draw.line", "draw.polyline",
                                             "draw.rectangle", "draw.polygon"}));
}

TEST(DrawLineTools, StartFromTheirAutoCadVerbsAndTheInterpretersSpellings)
{
    // AutoCAD's command names and aliases, and the interpreter's own RECT,
    // RECTANGLE and POLYLINE (command_interpreter.cpp), which name the same
    // tools there.
    const std::vector<std::pair<std::string, std::string>> verbs = {
        {"POINT", "draw.point"},        {"PO", "draw.point"},         {"LINE", "draw.line"},
        {"L", "draw.line"},             {"PLINE", "draw.polyline"},   {"PL", "draw.polyline"},
        {"POLYLINE", "draw.polyline"},  {"RECTANG", "draw.rectangle"}, {"REC", "draw.rectangle"},
        {"RECT", "draw.rectangle"},     {"RECTANGLE", "draw.rectangle"},
        {"POLYGON", "draw.polygon"},    {"POL", "draw.polygon"},
    };
    for (const auto& [verb, id] : verbs) {
        const katana::cad::ToolInfo* tool = katana::cad::toolCatalog().findByAlias(verb);
        ASSERT_NE(tool, nullptr) << verb;
        EXPECT_EQ(tool->id, id) << verb;
    }
}

TEST(DrawLineTools, EveryOneDrawsOnTheCurrentLayer)
{
    ToolDriver driver;
    const katana::entity::Layer kerb{"Kerb"};
    ASSERT_TRUE(driver.document().execute(katana::commands::createLayer(kerb)).ok());
    ASSERT_TRUE(driver.document().setCurrentLayer("Kerb").ok());

    driver.start("draw.point");
    (void)driver.click(1, 1);
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(1, 0);
    (void)driver.click(1, 1);
    (void)driver.enter();
    driver.start("draw.polyline");
    (void)driver.click(0, 0);
    (void)driver.click(2, 0);
    (void)driver.enter();
    driver.start("draw.rectangle");
    (void)driver.click(0, 0);
    (void)driver.click(2, 1);
    driver.start("draw.polygon");
    (void)driver.type("3");
    (void)driver.click(0, 0);
    (void)driver.click(1, 0);

    // 1 point + 2 lines + 1 polyline + 1 rectangle + 1 polygon.
    const auto entities = entitiesOf(driver.document());
    ASSERT_EQ(entities.size(), 6u);
    for (const Entity& entity : entities) {
        EXPECT_EQ(entity.layer, "Kerb") << entity.id;
    }
}

// ---- Point -------------------------------------------------------------------------

TEST(DrawPointTool, PlacesAPointAtEachClickAndKeepsGoingUntilEsc)
{
    ToolDriver driver;
    driver.start("draw.point");
    EXPECT_EQ(driver.tool().prompt(), "Specify a point");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_EQ(driver.click(1.5, 2.25).outcome, Outcome::Done);
    EXPECT_EQ(driver.click(-3, 4).outcome, Outcome::Done);

    const auto points = shapesOf<PointGeometry>(driver.document());
    ASSERT_EQ(points.size(), 2u);
    EXPECT_EQ(points[0].position, Point2(1.5, 2.25));
    EXPECT_EQ(points[1].position, Point2(-3.0, 4.0));
    EXPECT_EQ(driver.executed(), 2);
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().prompt(), "Specify a point");
}

TEST(DrawPointTool, TakesTypedCoordinates)
{
    ToolDriver driver;
    driver.start("draw.point");
    EXPECT_EQ(driver.type("12.5, -7.25").outcome, Outcome::Done);
    const auto points = shapesOf<PointGeometry>(driver.document());
    ASSERT_EQ(points.size(), 1u);
    EXPECT_EQ(points[0].position, Point2(12.5, -7.25));
}

TEST(DrawPointTool, EachPointIsItsOwnUndoStep)
{
    ToolDriver driver;
    driver.start("draw.point");
    (void)driver.click(1, 1);
    (void)driver.click(2, 2);
    ASSERT_TRUE(driver.document().undo().ok());
    const auto points = shapesOf<PointGeometry>(driver.document());
    ASSERT_EQ(points.size(), 1u);
    EXPECT_EQ(points[0].position, Point2(1.0, 1.0));
}

TEST(DrawPointTool, RefusesANumberOnItsOwnAndRelativeInputWithNothingToMeasureFrom)
{
    ToolDriver driver;
    driver.start("draw.point");
    expectRefused(driver.type("12"));
    // Each point starts the tool afresh, so there is never a last point here.
    expectRefused(driver.type("@1,1"));
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
    EXPECT_EQ(driver.executed(), 0);
}

TEST(DrawPointTool, EnterEndsTheToolWithoutPlacingAnything)
{
    ToolDriver driver;
    driver.start("draw.point");
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(driver.executed(), 0);
}

TEST(DrawPointTool, PreviewsThePointAtTheCursor)
{
    ToolDriver driver;
    driver.start("draw.point");
    const auto feedback = driver.tool().preview({3.0, 4.0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], katana::entity::Geometry(PointGeometry{{3.0, 4.0}}));
}

// ---- Line --------------------------------------------------------------------------

TEST(DrawLineTool, AChainOfPointsMakesSeparateLinesInOneCommand)
{
    ToolDriver driver;
    driver.start("draw.line");
    EXPECT_EQ(driver.click(0, 0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.click(4, 0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.click(4, 3).outcome, Outcome::Continue);
    EXPECT_TRUE(entitiesOf(driver.document()).empty()) << "nothing is drawn before Enter";
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.message, "2 lines");

    const auto lines = shapesOf<Segment2>(driver.document());
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(entitiesOf(driver.document()).size(), 2u) << "separate lines, not a polyline";
    EXPECT_EQ(lines[0], (Segment2{{0, 0}, {4, 0}}));
    EXPECT_EQ(lines[1], (Segment2{{4, 0}, {4, 3}}));
    EXPECT_EQ(driver.executed(), 1);
}

TEST(DrawLineTool, OneUndoOfTheDocumentRemovesTheWholeChain)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(4, 3);
    (void)driver.click(0, 3);
    (void)driver.enter();
    ASSERT_EQ(entitiesOf(driver.document()).size(), 3u);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawLineTool, ASingleSegmentIsOneLine)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(1, 2);
    (void)driver.click(3, 5);
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.message, "1 line");
    const auto lines = shapesOf<Segment2>(driver.document());
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines[0], (Segment2{{1, 2}, {3, 5}}));
}

TEST(DrawLineTool, CloseJoinsTheLastPointBackToTheFirst)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(4, 3);
    const ToolStep step = driver.type("C");
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.message, "3 lines");

    const auto lines = shapesOf<Segment2>(driver.document());
    ASSERT_EQ(lines.size(), 3u);
    EXPECT_EQ(lines[2], (Segment2{{4, 3}, {0, 0}}));
    EXPECT_EQ(driver.executed(), 1);
}

TEST(DrawLineTool, TakesCloseWholeAbbreviatedOrInAnyCase)
{
    for (const char* spelling : {"c", "Cl", "close", "CLOSE", " Close "}) {
        ToolDriver driver;
        driver.start("draw.line");
        (void)driver.click(0, 0);
        (void)driver.click(4, 0);
        (void)driver.click(4, 3);
        EXPECT_EQ(driver.type(spelling).outcome, Outcome::Done) << spelling;
        EXPECT_EQ(shapesOf<Segment2>(driver.document()).size(), 3u) << spelling;
    }
}

TEST(DrawLineTool, UndoTakesBackTheLastPointAndTheChainGoesOnFromTheOneBefore)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(9, 9);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().lastPoint(), Point2(4.0, 0.0));
    (void)driver.click(7, 7);
    // U typed at the prompt is the same as the Undo button.
    EXPECT_EQ(driver.type("u").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().lastPoint(), Point2(4.0, 0.0));
    (void)driver.click(4, 3);
    (void)driver.enter();

    const auto lines = shapesOf<Segment2>(driver.document());
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(lines[0], (Segment2{{0, 0}, {4, 0}}));
    EXPECT_EQ(lines[1], (Segment2{{4, 0}, {4, 3}}));
}

TEST(DrawLineTool, TakesRelativeAndPolarPointsFromTheLastPoint)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.type("1,1");
    // (1, 1) + (3, 0) = (4, 1).
    (void)driver.type("@3,0");
    // 2 at 90 degrees is straight up: (4, 1) + (0, 2) = (4, 3).
    (void)driver.type("@2<90");
    (void)driver.enter();

    const auto lines = shapesOf<Segment2>(driver.document());
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(lines[0], (Segment2{{1, 1}, {4, 1}}));
    EXPECT_TRUE(near(lines[1].start, {4, 1}));
    EXPECT_TRUE(near(lines[1].end, {4, 3}));
}

TEST(DrawLineTool, RefusesAPointOnTopOfTheLastOneAndCarriesOn)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(2, 2);
    expectRefused(driver.click(2, 2));
    // Within the geometric tolerance (1e-7) is the same point.
    expectRefused(driver.click(2.0 + 5e-8, 2));
    EXPECT_EQ(driver.tool().prompt(), "Specify next point or [Undo]");
    (void)driver.click(5, 6);
    (void)driver.enter();
    const auto lines = shapesOf<Segment2>(driver.document());
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_EQ(lines[0], (Segment2{{2, 2}, {5, 6}}));
}

TEST(DrawLineTool, RefusesToCloseBeforeThereAreTwoLines)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    expectRefused(driver.type("C"));
    EXPECT_EQ(driver.tool().lastPoint(), Point2(4.0, 0.0));
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawLineTool, RefusesToCloseAChainThatAlreadyEndsAtItsStart)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(4, 3);
    (void)driver.click(0, 0);
    // Closing would add a line of zero length.
    expectRefused(driver.type("C"));
    EXPECT_EQ(driver.enter().message, "3 lines");
}

TEST(DrawLineTool, RefusesUndoBeforeAnyPointAndInputThatIsNeitherAPointNorAnOption)
{
    ToolDriver driver;
    driver.start("draw.line");
    expectRefused(driver.undo());
    expectRefused(driver.type("C"));
    (void)driver.click(0, 0);
    expectRefused(driver.type("12"));
    expectRefused(driver.type("X"));
    EXPECT_EQ(driver.tool().lastPoint(), Point2(0.0, 0.0));
}

TEST(DrawLineTool, EnterBeforeTheFirstLineEndsTheToolWithNothingDrawn)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawLineTool, StartsANewChainAfterFinishingOne)
{
    ToolDriver driver;
    driver.start("draw.line");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.enter();
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().prompt(), "Specify first point");
    EXPECT_FALSE(driver.tool().lastPoint().has_value());
    (void)driver.click(10, 10);
    (void)driver.click(12, 10);
    (void)driver.enter();
    EXPECT_EQ(shapesOf<Segment2>(driver.document()).size(), 2u);
    EXPECT_EQ(driver.executed(), 2);
    // Enter with nothing started ends the tool.
    (void)driver.enter();
    EXPECT_TRUE(driver.finished());
}

TEST(DrawLineTool, OffersCloseOnlyOnceThereAreTwoLines)
{
    ToolDriver driver;
    driver.start("draw.line");
    EXPECT_EQ(driver.tool().prompt(), "Specify first point");
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.tool().prompt(), "Specify next point or [Undo]");
    (void)driver.click(4, 0);
    EXPECT_EQ(driver.tool().prompt(), "Specify next point or [Undo]");
    (void)driver.click(4, 3);
    EXPECT_EQ(driver.tool().prompt(), "Specify next point or [Close/Undo]");
}

TEST(DrawLineTool, PreviewsTheChainSoFarAndTheRubberBandToTheCursor)
{
    ToolDriver driver;
    driver.start("draw.line");
    EXPECT_TRUE(driver.tool().preview({1, 1}).shapes.empty());
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    const auto feedback = driver.tool().preview({4, 3});
    ASSERT_EQ(feedback.shapes.size(), 2u);
    EXPECT_EQ(feedback.shapes[0], katana::entity::Geometry(Segment2{{0, 0}, {4, 0}}));
    EXPECT_EQ(feedback.shapes[1], katana::entity::Geometry(Segment2{{4, 0}, {4, 3}}));
    EXPECT_EQ(feedback.markers, (std::vector<Point2>{{0, 0}, {4, 0}}));
}

// ---- Polyline ----------------------------------------------------------------------

TEST(DrawPolylineTool, EnterMakesOneOpenPolylineThroughTheVertices)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    EXPECT_EQ(driver.tool().prompt(), "Specify start point");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(4, 3);
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.message, "polyline of 3 vertices");

    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{0, 0}, {4, 0}, {4, 3}}, false}));
    EXPECT_EQ(driver.executed(), 1);
}

TEST(DrawPolylineTool, CloseMakesItClosed)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(4, 3);
    EXPECT_EQ(driver.tool().prompt(), "Specify next point or [Close/Undo]");
    const ToolStep step = driver.type("close");
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.message, "closed polyline of 3 vertices");
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{0, 0}, {4, 0}, {4, 3}}, true}));
}

TEST(DrawPolylineTool, ClosingAfterSnappingBackToTheStartDoesNotDoubleTheFirstVertex)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(4, 3);
    (void)driver.click(0, 0);
    (void)driver.type("C");
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{0, 0}, {4, 0}, {4, 3}}, true}));
}

TEST(DrawPolylineTool, UndoTakesBackTheLastVertex)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(9, 9);
    EXPECT_EQ(driver.type("U").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().lastPoint(), Point2(4.0, 0.0));
    (void)driver.click(4, 3);
    (void)driver.undo();
    (void)driver.undo();
    EXPECT_EQ(driver.tool().lastPoint(), Point2(0.0, 0.0));
    (void)driver.click(0, 5);
    (void)driver.enter();
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{0, 0}, {0, 5}}, false}));
}

TEST(DrawPolylineTool, TakesRelativeAndPolarVertices)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.type("0,0");
    (void)driver.type("@5,0");  // (5, 0)
    (void)driver.type("@5<90"); // (5, 0) + 5 straight up = (5, 5)
    (void)driver.type("@-5,0"); // (0, 5)
    (void)driver.type("C");
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_TRUE(polyline->closed);
    EXPECT_TRUE(verticesNear(polyline->vertices, {{0, 0}, {5, 0}, {5, 5}, {0, 5}}));
}

TEST(DrawPolylineTool, OneUndoOfTheDocumentRemovesThePolyline)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.click(0, 0);
    (void)driver.click(4, 0);
    (void)driver.click(4, 3);
    (void)driver.enter();
    ASSERT_EQ(entitiesOf(driver.document()).size(), 1u);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawPolylineTool, RefusesACoincidentVertexAndCloseWithFewerThanThreeVertices)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    expectRefused(driver.undo());
    (void)driver.click(0, 0);
    expectRefused(driver.click(0, 0));
    (void)driver.click(4, 0);
    expectRefused(driver.type("C"));
    // Back to the start and Close would leave two vertices: a line, not a
    // closed shape.
    (void)driver.click(0, 0);
    expectRefused(driver.type("C"));
    expectRefused(driver.type("12"));
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
    EXPECT_EQ(driver.tool().lastPoint(), Point2(0.0, 0.0));
}

TEST(DrawPolylineTool, EnterWithOneVertexEndsTheToolWithNothingDrawn)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.click(3, 3);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawPolylineTool, StartsANewPolylineAfterFinishingOne)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.click(0, 0);
    (void)driver.click(1, 0);
    (void)driver.enter();
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().prompt(), "Specify start point");
    (void)driver.click(0, 1);
    (void)driver.click(1, 1);
    (void)driver.enter();
    EXPECT_EQ(shapesOf<Polyline2>(driver.document()).size(), 2u);
}

TEST(DrawPolylineTool, PreviewsThePolylineRunningOnToTheCursor)
{
    ToolDriver driver;
    driver.start("draw.polyline");
    (void)driver.click(0, 0);
    auto feedback = driver.tool().preview({3, 4});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], katana::entity::Geometry(Polyline2{{{0, 0}, {3, 4}}, false}));

    (void)driver.click(4, 0);
    feedback = driver.tool().preview({4, 3});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0],
              katana::entity::Geometry(Polyline2{{{0, 0}, {4, 0}, {4, 3}}, false}));
    EXPECT_EQ(feedback.markers, (std::vector<Point2>{{0, 0}, {4, 0}}));

    // The cursor on the last vertex adds nothing to what is already there.
    feedback = driver.tool().preview({4, 0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], katana::entity::Geometry(Polyline2{{{0, 0}, {4, 0}}, false}));
}

// ---- Rectangle ---------------------------------------------------------------------

TEST(DrawRectangleTool, TwoCornersMakeAClosedRectangleCounterClockwiseFromTheLowerLeft)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    EXPECT_EQ(driver.tool().prompt(), "Specify first corner point");
    EXPECT_EQ(driver.click(0, 0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify other corner point or [Dimensions/Undo]");
    const ToolStep step = driver.click(4, 3);
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.message, "rectangle");
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{0, 0}, {4, 0}, {4, 3}, {0, 3}}, true}));
    EXPECT_EQ(driver.executed(), 1);
}

TEST(DrawRectangleTool, TheCornersMayBeEitherPairInEitherOrder)
{
    const Polyline2 expected{{{0, 0}, {4, 0}, {4, 3}, {0, 3}}, true};
    const std::vector<std::pair<Point2, Point2>> corners = {
        {{4, 3}, {0, 0}}, {{0, 3}, {4, 0}}, {{4, 0}, {0, 3}}};
    for (const auto& [first, second] : corners) {
        ToolDriver driver;
        driver.start("draw.rectangle");
        (void)driver.click(first.x, first.y);
        (void)driver.click(second.x, second.y);
        const auto polyline = onlyPolyline(driver.document());
        ASSERT_TRUE(polyline);
        EXPECT_EQ(*polyline, expected) << text(first) << " " << text(second);
    }
}

TEST(DrawRectangleTool, TheOtherCornerCanBeTypedAsWidthAndHeightFromTheFirst)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    (void)driver.type("1,2");
    // (1, 2) + (4, 3) = (5, 5).
    (void)driver.type("@4,3");
    // From (4, 3) back by (4, 3) is the origin; the outline still starts at
    // the lower left corner.
    (void)driver.type("4,3");
    (void)driver.type("@-4,-3");
    const auto polylines = shapesOf<Polyline2>(driver.document());
    ASSERT_EQ(polylines.size(), 2u);
    EXPECT_EQ(polylines[0], (Polyline2{{{1, 2}, {5, 2}, {5, 5}, {1, 5}}, true}));
    EXPECT_EQ(polylines[1], (Polyline2{{{0, 0}, {4, 0}, {4, 3}, {0, 3}}, true}));
}

TEST(DrawRectangleTool, RefusesCornersInLineAndWaitsForAnother)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    (void)driver.click(0, 0);
    expectRefused(driver.click(4, 0)); // no height
    expectRefused(driver.click(0, 3)); // no width
    expectRefused(driver.click(0, 0)); // neither
    expectRefused(driver.type("@5,0"));
    expectRefused(driver.type("12"));
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
    EXPECT_EQ(driver.tool().lastPoint(), Point2(0.0, 0.0));
    EXPECT_EQ(driver.click(4, 3).outcome, Outcome::Done);
}

TEST(DrawRectangleTool, UndoTakesBackTheFirstCorner)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    expectRefused(driver.undo());
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify first corner point");
    EXPECT_FALSE(driver.tool().lastPoint().has_value());
    (void)driver.click(1, 1);
    (void)driver.click(2, 3);
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{1, 1}, {2, 1}, {2, 3}, {1, 3}}, true}));
}

TEST(DrawRectangleTool, DimensionsTakesTypedSizesAndAClickOnTheSideItGoes)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    (void)driver.click(10, 10);
    EXPECT_EQ(driver.type("d").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_EQ(driver.tool().prompt(), "Specify length of the rectangle along x or [Undo]");
    (void)driver.type("4");
    EXPECT_EQ(driver.tool().prompt(), "Specify width of the rectangle along y or [Undo]");
    (void)driver.type("3");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    // Below and to the left of (10, 10): the far corner is (10 - 4, 10 - 3).
    EXPECT_EQ(driver.click(0, 0).outcome, Outcome::Done);
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{6, 7}, {10, 7}, {10, 10}, {6, 10}}, true}));
}

TEST(DrawRectangleTool, DimensionsGoesUpAndRightOnEnter)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    (void)driver.click(10, 10);
    (void)driver.type("Dimensions");
    (void)driver.type("4");
    (void)driver.type("2.5");
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{10, 10}, {14, 10}, {14, 12.5}, {10, 12.5}}, true}));
}

TEST(DrawRectangleTool, DimensionsRefusesASizeThatIsNotAPositiveNumber)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    (void)driver.click(0, 0);
    (void)driver.type("D");
    expectRefused(driver.type("0"));
    expectRefused(driver.type("-2"));
    expectRefused(driver.type("abc"));
    expectRefused(driver.click(5, 5));
    expectRefused(driver.enter());
    (void)driver.type("4");
    expectRefused(driver.type("0"));
    expectRefused(driver.enter());
    EXPECT_EQ(driver.tool().prompt(), "Specify width of the rectangle along y or [Undo]");
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawRectangleTool, DimensionsStepsBackWithUndo)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    (void)driver.click(10, 10);
    (void)driver.type("D");
    (void)driver.type("4");
    (void)driver.type("U");
    EXPECT_EQ(driver.tool().prompt(), "Specify length of the rectangle along x or [Undo]");
    (void)driver.undo();
    EXPECT_EQ(driver.tool().prompt(), "Specify other corner point or [Dimensions/Undo]");
    (void)driver.click(12, 13);
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(*polyline, (Polyline2{{{10, 10}, {12, 10}, {12, 13}, {10, 13}}, true}));
}

TEST(DrawRectangleTool, OneUndoRemovesItAndTheToolStartsAgain)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    (void)driver.click(0, 0);
    (void)driver.click(4, 3);
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().prompt(), "Specify first corner point");
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
    // Enter at the first corner ends the tool.
    (void)driver.enter();
    EXPECT_TRUE(driver.finished());
}

TEST(DrawRectangleTool, PreviewsTheRectangleToTheCursor)
{
    ToolDriver driver;
    driver.start("draw.rectangle");
    EXPECT_TRUE(driver.tool().preview({4, 3}).shapes.empty());
    (void)driver.click(0, 0);
    auto feedback = driver.tool().preview({4, 3});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0],
              katana::entity::Geometry(Polyline2{{{0, 0}, {4, 0}, {4, 3}, {0, 3}}, true}));
    EXPECT_EQ(feedback.markers, (std::vector<Point2>{{0, 0}}));
    // In line with the corner the rectangle is only its one side.
    feedback = driver.tool().preview({4, 0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], katana::entity::Geometry(Segment2{{0, 0}, {4, 0}}));
    EXPECT_TRUE(driver.tool().preview({0, 0}).shapes.empty());

    // With typed sizes, the rectangle on the cursor's side of the corner.
    (void)driver.type("D");
    (void)driver.type("4");
    (void)driver.type("3");
    feedback = driver.tool().preview({-1, -1});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0],
              katana::entity::Geometry(Polyline2{{{-4, -3}, {0, -3}, {0, 0}, {-4, 0}}, true}));
}

// ---- Polygon -----------------------------------------------------------------------

TEST(DrawPolygonTool, AsksForTheNumberOfSidesFirstAndEnterTakesFour)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    EXPECT_EQ(driver.tool().prompt(), "Enter number of sides <4>");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify centre of polygon or [Edge/Undo]");
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.tool().prompt(),
              "Specify a vertex or type the radius, or [Circumscribed/Undo]");
    const ToolStep step = driver.click(1, 0);
    EXPECT_EQ(step.outcome, Outcome::Done);
    EXPECT_EQ(step.message, "polygon of 4 sides");
    // Inscribed: (1, 0) is a vertex; the others are it turned by 90, 180 and
    // 270 degrees about the centre.
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_TRUE(polyline->closed);
    EXPECT_TRUE(verticesNear(polyline->vertices, {{1, 0}, {0, 1}, {-1, 0}, {0, -1}}));
}

TEST(DrawPolygonTool, AnInscribedHexagonHasAVertexAtThePickedPoint)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("6");
    (void)driver.click(0, 0);
    (void)driver.click(2, 0);
    // Radius 2 at 0, 60, ... 300 degrees: (2cos60, 2sin60) = (1, sqrt3).
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_TRUE(polyline->closed);
    EXPECT_EQ(polyline->vertices.front(), Point2(2.0, 0.0)) << "the picked point itself";
    EXPECT_TRUE(verticesNear(polyline->vertices, {{2, 0},
                                                  {1, kRoot3},
                                                  {-1, kRoot3},
                                                  {-2, 0},
                                                  {-1, -kRoot3},
                                                  {1, -kRoot3}}));
}

TEST(DrawPolygonTool, ACircumscribedPolygonHasTheMiddleOfASideAtThePickedPoint)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("4");
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.type("C").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(),
              "Specify the middle of a side or type the radius, or [Inscribed/Undo]");
    (void)driver.click(0, -1);
    // The square about the unit circle: the side through (0, -1) runs from
    // (-1, -1) to (1, -1), and the first vertex is 45 degrees clockwise of
    // the picked direction at radius sqrt2, (-1, -1).
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_TRUE(verticesNear(polyline->vertices, {{-1, -1}, {1, -1}, {1, 1}, {-1, 1}}));
}

TEST(DrawPolygonTool, ATypedRadiusDrawsTheBottomSideLevel)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("4");
    (void)driver.click(0, 0);
    (void)driver.type("2");
    // Inscribed in radius 2 with the bottom side level: vertices at -135,
    // -45, 45 and 135 degrees, 2(cos -135, sin -135) = (-sqrt2, -sqrt2).
    (void)driver.type("3");
    (void)driver.click(0, 0);
    (void)driver.type("2");
    // A triangle the same way: -150, -30 and 90 degrees;
    // 2(cos -150, sin -150) = (-sqrt3, -1).
    (void)driver.type("4");
    (void)driver.click(0, 0);
    (void)driver.type("c");
    (void)driver.type("1");
    // About the unit circle: the 2 x 2 square centred on the origin.
    const auto polylines = shapesOf<Polyline2>(driver.document());
    ASSERT_EQ(polylines.size(), 3u);
    EXPECT_TRUE(verticesNear(polylines[0].vertices, {{-kRoot2, -kRoot2},
                                                     {kRoot2, -kRoot2},
                                                     {kRoot2, kRoot2},
                                                     {-kRoot2, kRoot2}}));
    EXPECT_TRUE(verticesNear(polylines[1].vertices, {{-kRoot3, -1}, {kRoot3, -1}, {0, 2}}));
    EXPECT_TRUE(verticesNear(polylines[2].vertices, {{-1, -1}, {1, -1}, {1, 1}, {-1, 1}}));
}

TEST(DrawPolygonTool, TakesTheRadiusPointAsPolarInputFromTheCentre)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("3");
    (void)driver.type("1,1");
    EXPECT_EQ(driver.tool().lastPoint(), Point2(1.0, 1.0));
    (void)driver.type("@2<90");
    // A vertex 2 straight up from (1, 1) at (1, 3); the others are (0, 2)
    // turned by 120 and 240 degrees, (-sqrt3, -1) and (sqrt3, -1), from (1, 1).
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_TRUE(verticesNear(polyline->vertices, {{1, 3}, {1 - kRoot3, 0}, {1 + kRoot3, 0}}));
}

TEST(DrawPolygonTool, EdgeDrawsThePolygonOnTheLeftOfTheEdgeFromItsFirstEnd)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("4");
    EXPECT_EQ(driver.type("e").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify first endpoint of edge or [Undo]");
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.tool().prompt(), "Specify second endpoint of edge or [Undo]");
    (void)driver.click(2, 0);
    // Reversed, the square falls on the other side.
    (void)driver.type("4");
    (void)driver.type("E");
    (void)driver.click(2, 0);
    (void)driver.click(0, 0);
    // A hexagon on the unit edge: each side turns 60 degrees left of the one
    // before, (1, 0) + (cos60, sin60) = (1.5, sqrt3/2), and so on.
    (void)driver.type("6");
    (void)driver.type("E");
    (void)driver.click(0, 0);
    (void)driver.click(1, 0);
    const auto polylines = shapesOf<Polyline2>(driver.document());
    ASSERT_EQ(polylines.size(), 3u);
    EXPECT_TRUE(verticesNear(polylines[0].vertices, {{0, 0}, {2, 0}, {2, 2}, {0, 2}}));
    EXPECT_TRUE(verticesNear(polylines[1].vertices, {{2, 0}, {0, 0}, {0, -2}, {2, -2}}));
    EXPECT_TRUE(verticesNear(polylines[2].vertices, {{0, 0},
                                                     {1, 0},
                                                     {1.5, kRoot3 / 2},
                                                     {1, kRoot3},
                                                     {0, kRoot3},
                                                     {-0.5, kRoot3 / 2}}));
    for (const Polyline2& polyline : polylines) {
        EXPECT_TRUE(polyline.closed);
    }
}

TEST(DrawPolygonTool, TakesItsOptionsWholeAbbreviatedOrInAnyCase)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("5");
    (void)driver.click(0, 0);
    const std::string inscribed = "Specify a vertex or type the radius, or [Circumscribed/Undo]";
    const std::string circumscribed =
        "Specify the middle of a side or type the radius, or [Inscribed/Undo]";
    (void)driver.type("circ");
    EXPECT_EQ(driver.tool().prompt(), circumscribed);
    (void)driver.type("INSCRIBED");
    EXPECT_EQ(driver.tool().prompt(), inscribed);
    (void)driver.type("Circumscribed");
    EXPECT_EQ(driver.tool().prompt(), circumscribed);
    (void)driver.type("i");
    EXPECT_EQ(driver.tool().prompt(), inscribed);
}

TEST(DrawPolygonTool, RefusesANumberOfSidesOutsideThreeTo1024)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    for (const char* sides : {"2", "1025", "0", "-5", "4.5", "six", "C"}) {
        expectRefused(driver.type(sides));
        EXPECT_EQ(driver.tool().prompt(), "Enter number of sides <4>") << sides;
    }
    expectRefused(driver.click(1, 1));
    EXPECT_EQ(driver.type("3").outcome, Outcome::Continue);
    (void)driver.undo();
    EXPECT_EQ(driver.type("1024").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify centre of polygon or [Edge/Undo]");
}

TEST(DrawPolygonTool, RefusesARadiusThatIsNotGreaterThanZero)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("5");
    expectRefused(driver.type("12")); // a number is not a centre
    (void)driver.click(0, 0);
    expectRefused(driver.click(0, 0));
    expectRefused(driver.click(5e-8, 0)); // within the geometric tolerance
    expectRefused(driver.type("0"));
    expectRefused(driver.type("-3"));
    expectRefused(driver.type("abc"));
    EXPECT_EQ(driver.tool().lastPoint(), Point2(0.0, 0.0));
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawPolygonTool, RefusesAnEdgeWhoseEndsAreOnePoint)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("5");
    (void)driver.type("E");
    (void)driver.click(3, 3);
    expectRefused(driver.click(3, 3));
    expectRefused(driver.type("7"));
    EXPECT_EQ(driver.tool().prompt(), "Specify second endpoint of edge or [Undo]");
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawPolygonTool, UndoStepsBackThroughTheCentreAndTheNumberOfSides)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    expectRefused(driver.undo());
    (void)driver.type("6");
    (void)driver.click(0, 0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify centre of polygon or [Edge/Undo]");
    (void)driver.type("E");
    (void)driver.click(1, 1);
    (void)driver.type("U");
    EXPECT_EQ(driver.tool().prompt(), "Specify first endpoint of edge or [Undo]");
    (void)driver.undo();
    EXPECT_EQ(driver.tool().prompt(), "Specify centre of polygon or [Edge/Undo]");
    (void)driver.undo();
    EXPECT_EQ(driver.tool().prompt(), "Enter number of sides <4>");
    expectRefused(driver.undo());
    // Back at the start: a new count, and a square from it.
    (void)driver.type("4");
    (void)driver.click(0, 0);
    (void)driver.click(0, 1);
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    EXPECT_EQ(polyline->vertices.size(), 4u);
}

TEST(DrawPolygonTool, OneUndoRemovesThePolygonAndTheToolAsksForTheSidesAgain)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("8");
    (void)driver.click(0, 0);
    (void)driver.click(3, 0);
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_FALSE(driver.finished());
    EXPECT_EQ(driver.tool().prompt(), "Enter number of sides <4>");
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_TRUE(entitiesOf(driver.document()).empty());
}

TEST(DrawPolygonTool, PreviewsThePolygonForTheCursor)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("6");
    EXPECT_TRUE(driver.tool().preview({2, 0}).shapes.empty());
    (void)driver.click(0, 0);
    auto feedback = driver.tool().preview({2, 0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    const auto* hexagon = std::get_if<Polyline2>(&feedback.shapes[0]);
    ASSERT_NE(hexagon, nullptr);
    EXPECT_TRUE(hexagon->closed);
    EXPECT_TRUE(verticesNear(hexagon->vertices, {{2, 0},
                                                 {1, kRoot3},
                                                 {-1, kRoot3},
                                                 {-2, 0},
                                                 {-1, -kRoot3},
                                                 {1, -kRoot3}}));
    EXPECT_EQ(feedback.markers, (std::vector<Point2>{{0, 0}}));
    EXPECT_TRUE(driver.tool().preview({0, 0}).shapes.empty());

    (void)driver.undo();
    (void)driver.type("E");
    (void)driver.click(0, 0);
    feedback = driver.tool().preview({1, 0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    const auto* onEdge = std::get_if<Polyline2>(&feedback.shapes[0]);
    ASSERT_NE(onEdge, nullptr);
    EXPECT_TRUE(verticesNear(onEdge->vertices, {{0, 0},
                                                {1, 0},
                                                {1.5, kRoot3 / 2},
                                                {1, kRoot3},
                                                {0, kRoot3},
                                                {-0.5, kRoot3 / 2}}));
}

TEST(DrawPolygonTool, EveryVertexOfA1024GonIsOnTheCircle)
{
    ToolDriver driver;
    driver.start("draw.polygon");
    (void)driver.type("1024");
    (void)driver.click(0, 0);
    (void)driver.click(1000, 0);
    const auto polyline = onlyPolyline(driver.document());
    ASSERT_TRUE(polyline);
    ASSERT_EQ(polyline->vertices.size(), 1024u);
    // Each vertex is the first turned about the centre, so none carries the
    // rounding of the ones before it: all are at radius 1000 to a few ulps,
    // and vertex 512 is half a turn round, (-1000, 0).
    for (const Point2& vertex : polyline->vertices) {
        EXPECT_NEAR(vertex.length(), 1000.0, 1e-9);
    }
    EXPECT_TRUE(near(polyline->vertices[512], {-1000, 0}, 1e-9));
}
