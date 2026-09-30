// What the Draw > Vertices tools show before a click and what they act on
// (src/katana_cad/tools/modify_vertex.cpp; docs/drawing.md, "What a vertex
// tool acts on") - the owner's request of 2026-09-30: an inserted vertex
// gave "no visual clue where the vertex is going", and a tool asked for a
// polyline "but what about the already selected vertex".
//
// Every expected coordinate here is worked out by hand from the polyline
// given, never read back from a run. The pick aperture is 0.5, so a vertex
// is reached within 0.75 and a segment within 0.5 (the view's 12 and 8 px).

#include <gtest/gtest.h>

#include <cmath>
#include <cstdio>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "tool_driver.hpp"

using katana::cad::FeedbackRole;
using katana::cad::Grip;
using katana::cad::GripKind;
using katana::cad::readPolyline;
using katana::cad::ToolFeedback;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::EntityId;
using katana::entity::Geometry;
using katana::geometry::Arc2;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

constexpr double kAperture = 0.5;
constexpr double kExact = 1.0e-9;

EntityId addPolyline(ToolDriver& driver, std::vector<Point2> points, bool closed = false,
                     std::vector<std::optional<double>> heights = {})
{
    katana::entity::Entity entity;
    entity.geometry = Polyline2{std::move(points), closed};
    if (!heights.empty()) {
        katana::entity::setHeights(entity.properties, heights);
    }
    return driver.add(katana::commands::createEntities({entity}));
}

EntityId addCurve(ToolDriver& driver, const CurvePolyline2& shape)
{
    katana::entity::Entity entity;
    entity.geometry = shape;
    return driver.add(katana::commands::createEntities({entity}));
}

CurvePolyline2 shapeOf(ToolDriver& driver, EntityId id)
{
    return *readPolyline(*driver.document().model().entities.find(id));
}

std::vector<Point2> positionsOf(ToolDriver& driver, EntityId id)
{
    return shapeOf(driver, id).positions();
}

void selectOnly(ToolDriver& driver, std::vector<EntityId> ids)
{
    driver.document().selection().set(std::move(ids));
}

Grip vertexHandle(ToolDriver& driver, EntityId id, std::size_t index)
{
    return Grip{id, GripKind::Vertex, index, shapeOf(driver, id).vertices[index].position};
}

std::vector<Point2> pointsOf(const ToolFeedback& feedback, FeedbackRole role)
{
    std::vector<Point2> out;
    for (const auto& mark : feedback.marks) {
        if (mark.role != role) {
            continue;
        }
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&mark.geometry)) {
            out.push_back(point->position);
        }
    }
    return out;
}

// The vertices a preview marks as the reason a click is refused
// (FeedbackMark::refused), whatever their role.
std::vector<Point2> refusedPointsOf(const ToolFeedback& feedback)
{
    std::vector<Point2> out;
    for (const auto& mark : feedback.marks) {
        const auto* point = std::get_if<katana::entity::PointGeometry>(&mark.geometry);
        if (mark.refused && point != nullptr) {
            out.push_back(point->position);
        }
    }
    return out;
}

std::vector<std::string> labelsOf(const ToolFeedback& feedback, FeedbackRole role)
{
    std::vector<std::string> out;
    for (const auto& mark : feedback.marks) {
        if (mark.role == role && !mark.label.empty()) {
            out.push_back(mark.label);
        }
    }
    return out;
}

std::vector<Geometry> piecesOf(const ToolFeedback& feedback, FeedbackRole role)
{
    std::vector<Geometry> out;
    for (const auto& mark : feedback.marks) {
        if (mark.role == role &&
            !std::holds_alternative<katana::entity::PointGeometry>(mark.geometry)) {
            out.push_back(mark.geometry);
        }
    }
    return out;
}

bool closeTo(const Point2& a, const Point2& b, double tolerance = kExact)
{
    return a.distanceTo(b) <= tolerance;
}

bool isSegment(const Geometry& geometry, const Point2& a, const Point2& b)
{
    const auto* segment = std::get_if<Segment2>(&geometry);
    return segment != nullptr && closeTo(segment->start, a) && closeTo(segment->end, b);
}

bool hasPoint(const std::vector<Point2>& points, const Point2& p, double tolerance = kExact)
{
    return std::ranges::any_of(points, [&](const Point2& q) { return closeTo(p, q, tolerance); });
}

std::string idText(EntityId id) { return std::to_string(id); }

} // namespace

// ---- Insert Vertex --------------------------------------------------------------------------

TEST(VertexToolFeedback, InsertVertexShowsTheSegmentItSplitsAndTheVertexOnIt)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    const ToolFeedback shown = driver.preview(4, 0.3);
    // (4, 0.3) projects onto segment 0 at (4, 0), 4 from vertex 0.
    const auto targets = piecesOf(shown, FeedbackRole::Target);
    ASSERT_EQ(targets.size(), 1u);
    EXPECT_TRUE(isSegment(targets[0], Point2(0, 0), Point2(10, 0)));
    const auto added = pointsOf(shown, FeedbackRole::Added);
    ASSERT_EQ(added.size(), 1u);
    EXPECT_TRUE(closeTo(added[0], Point2(4, 0)));
    EXPECT_TRUE(shown.shapes.empty()) << "the two halves lie on the old segment: no ghost";
    EXPECT_EQ(shown.focus, p);
    EXPECT_FALSE(shown.refused);
    EXPECT_EQ(shown.caption, "new vertex between 0 and 1 · 4.000 from 0");
    EXPECT_EQ(driver.executed(), 0) << "a preview changes nothing";
}

TEST(VertexToolFeedback, AClickNearTheLinePutsTheVertexOnIt)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    ASSERT_EQ(driver.tool().expects(), ToolInput::Point) << "one step: the point, snapped";
    const ToolStep step = driver.click(4, 0.3);
    ASSERT_EQ(step.outcome, ToolStep::Outcome::Done) << step.message;
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(4, 0), Point2(10, 0), Point2(10, 10)}));
    EXPECT_EQ(driver.document().history().undoName(), "VERTEX_INSERT");
    EXPECT_FALSE(driver.finished()) << "it restarts for the next vertex";
    EXPECT_EQ(driver.document().selection().ids(), std::vector<EntityId>{p});
    EXPECT_EQ(step.message, "vertex 1 added to polyline " + idText(p) + " on segment 0 (4 vertices)");
}

TEST(VertexToolFeedback, OnAnArcTheNewVertexIsOnTheArc)
{
    // Bulge +1 from (0,0) to (10,0) is a counter-clockwise half circle about
    // (5,0) of radius 5: it runs below the chord, through (5,-5).
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    CurvePolyline2 shape = CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0)});
    shape.vertices[0].bulge = 1.0;
    const EntityId p = addCurve(driver, shape);
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    const ToolFeedback shown = driver.preview(5, -5.2);
    const auto added = pointsOf(shown, FeedbackRole::Added);
    ASSERT_EQ(added.size(), 1u);
    EXPECT_TRUE(closeTo(added[0], Point2(5, -5), 1.0e-12)) << added[0].x << "," << added[0].y;
    // A quarter of the circle, 5 pi / 2 along the arc.
    EXPECT_EQ(shown.caption, "new vertex between 0 and 1 · 7.854 from 0");
    // (5,6) is above the chord: the arc's nearest point is an end, sqrt(61)
    // = 7.81 away, far out of reach.
    const ToolFeedback away = driver.preview(5, 6);
    EXPECT_TRUE(away.marks.empty());
    EXPECT_FALSE(away.refused);

    ASSERT_EQ(driver.click(5, -5.2).outcome, ToolStep::Outcome::Done);
    const CurvePolyline2 after = shapeOf(driver, p);
    ASSERT_EQ(after.vertices.size(), 3u);
    // Each half sweeps a quarter turn: bulge tan(pi/8) = sqrt(2) - 1.
    EXPECT_NEAR(after.vertices[0].bulge, std::sqrt(2.0) - 1.0, 1.0e-12);
    EXPECT_NEAR(after.vertices[1].bulge, std::sqrt(2.0) - 1.0, 1.0e-12);
    EXPECT_TRUE(closeTo(after.vertices[1].position, Point2(5, -5), 1.0e-12));
}

TEST(VertexToolFeedback, ASelectedPolylineIsNotAskedFor)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0)});
    driver.start("draw.vertex.insert");
    EXPECT_EQ(driver.tool().prompt(), "Click on a polyline where the new vertex goes");
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    EXPECT_EQ(driver.tool().prompt(), "Click on polyline " + idText(p) + " where the new vertex goes");
}

TEST(VertexToolFeedback, AHotVertexChoosesTheSegmentEitherSideNearestTheCursor)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert", {vertexHandle(driver, p, 1)});
    EXPECT_EQ(driver.tool().prompt(),
              "Press Enter for a vertex at the marked middle of segment 1, or click beside vertex "
              "1 of polyline " +
                  idText(p));
    // (7,1) is 1 from segment 0 and 3 from segment 1.
    const ToolFeedback west = driver.preview(7, 1);
    const auto westTargets = piecesOf(west, FeedbackRole::Target);
    ASSERT_EQ(westTargets.size(), 1u);
    EXPECT_TRUE(isSegment(westTargets[0], Point2(0, 0), Point2(10, 0)));
    EXPECT_TRUE(hasPoint(pointsOf(west, FeedbackRole::Added), Point2(7, 0)));
    EXPECT_TRUE(hasPoint(pointsOf(west, FeedbackRole::Target), Point2(10, 0)))
        << "the hot vertex is marked";
    EXPECT_EQ(labelsOf(west, FeedbackRole::Target), std::vector<std::string>{"1"});
    // (11,4) is 1 from segment 1 and sqrt(17) from segment 0.
    const ToolFeedback north = driver.preview(11, 4);
    const auto northTargets = piecesOf(north, FeedbackRole::Target);
    ASSERT_EQ(northTargets.size(), 1u);
    EXPECT_TRUE(isSegment(northTargets[0], Point2(10, 0), Point2(10, 10)));
    EXPECT_TRUE(hasPoint(pointsOf(north, FeedbackRole::Added), Point2(10, 4)));
}

TEST(VertexToolFeedback, EnterWithAHotVertexAddsTheMiddleOfTheSegmentAfterIt)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p =
        addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0), Point2(20, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert", {vertexHandle(driver, p, 1)});
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(10, 0),
                                                           Point2(15, 0), Point2(20, 0),
                                                           Point2(20, 10)}));

    // At the last vertex of an open polyline there is no segment after it:
    // the middle of the one before, (20,0)-(20,10).
    ToolDriver end;
    end.setPickTolerance(kAperture);
    const EntityId q =
        addPolyline(end, {Point2(0, 0), Point2(10, 0), Point2(20, 0), Point2(20, 10)});
    selectOnly(end, {q});
    end.start("draw.vertex.insert", {vertexHandle(end, q, 3)});
    EXPECT_EQ(end.tool().prompt(),
              "Press Enter for a vertex at the marked middle of segment 2, or click beside vertex "
              "3 of polyline " +
                  idText(q));
    ASSERT_EQ(end.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(end, q), (std::vector<Point2>{Point2(0, 0), Point2(10, 0),
                                                        Point2(20, 0), Point2(20, 5),
                                                        Point2(20, 10)}));
}

TEST(VertexToolFeedback, HandlesAreUsedOnceSoARestartedToolDoesNotReuseThem)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p =
        addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0), Point2(20, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert", {vertexHandle(driver, p, 1)});
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    ASSERT_FALSE(driver.finished());
    // Vertex 1 is still (10,0), but the handle named it before the edit;
    // the restarted tool asks afresh.
    EXPECT_EQ(driver.tool().prompt(), "Click on polyline " + idText(p) + " where the new vertex goes");
    EXPECT_EQ(driver.enter().outcome, ToolStep::Outcome::Done) << "Enter now ends the tool";
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(driver.executed(), 1);
}

TEST(VertexToolFeedback, InsertingNearAVertexIsRefusedBeforeTheClick)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    // (0.2, 0.1) projects to (0.2, 0), 0.2 from vertex 0: within the 0.5.
    const ToolFeedback shown = driver.preview(0.2, 0.1);
    EXPECT_TRUE(shown.refused);
    EXPECT_EQ(shown.caption.rfind("too close to vertex 0", 0), 0u) << shown.caption;
    EXPECT_TRUE(pointsOf(shown, FeedbackRole::Added).empty());
    // The vertex it is too near is marked, as the refusal, with its number;
    // the whole segment in red flashed at every vertex passed and named none.
    EXPECT_TRUE(piecesOf(shown, FeedbackRole::Target).empty());
    EXPECT_EQ(refusedPointsOf(shown), std::vector<Point2>{Point2(0, 0)});
    EXPECT_EQ(labelsOf(shown, FeedbackRole::Target), std::vector<std::string>{"0"});
    const ToolStep step = driver.click(0.2, 0.1);
    EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(step.message, shown.caption) << "the click says what the preview said";
    EXPECT_EQ(driver.executed(), 0);
}

TEST(VertexToolFeedback, AFarClickIsRefusedAndChangesNothing)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    EXPECT_TRUE(driver.preview(4, 3).marks.empty());
    EXPECT_EQ(driver.click(4, 3).outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(shapeOf(driver, p).vertices.size(), 3u);
}

TEST(VertexToolFeedback, ATypedPointGoesOnTheLineAndATypedHeightIsKept)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    ASSERT_EQ(driver.type("4,0.3").outcome, ToolStep::Outcome::Done);
    ASSERT_EQ(driver.type("6,0.2,101.5").outcome, ToolStep::Outcome::Done);
    const CurvePolyline2 after = shapeOf(driver, p);
    ASSERT_EQ(after.vertices.size(), 5u);
    EXPECT_TRUE(closeTo(after.vertices[1].position, Point2(4, 0)));
    EXPECT_TRUE(closeTo(after.vertices[2].position, Point2(6, 0)));
    ASSERT_TRUE(after.vertices[2].height.has_value());
    EXPECT_DOUBLE_EQ(*after.vertices[2].height, 101.5);
    EXPECT_FALSE(after.vertices[1].height.has_value()) << "no ends with heights, none invented";
}

TEST(VertexToolFeedback, AnEditMadeSinceTheChoiceIsRefusedNotReverted)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert", {vertexHandle(driver, p, 1)});
    // Another edit - the Vertices panel, an agent - while the tool waits.
    ASSERT_TRUE(driver.document()
                    .execute(katana::cad::editPolyline(p, "VERTEX_MOVE",
                                                       [](const CurvePolyline2& shape) {
                                                           return katana::geometry::moveVertex(
                                                               shape, 2, Point2(10, 12));
                                                       }))
                    .ok());
    const std::size_t undoCount = driver.document().history().undoCount();
    const ToolStep step = driver.enter();
    EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(step.message, "polyline " + idText(p) + " changed since it was picked; pick it again");
    EXPECT_EQ(driver.document().history().undoCount(), undoCount);
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(10, 12)}))
        << "the other edit stays";
    EXPECT_EQ(driver.tool().prompt(), "Click on polyline " + idText(p) + " where the new vertex goes")
        << "the stale choice is dropped";
}

TEST(VertexToolFeedback, TheInsertPreviewIsWhatTheClickCommits)
{
    // A straight segment (0,0)-(10,0), then an arc of bulge -0.5 to (10,10):
    // clockwise, so it bulges to the left of its way north, with sagitta
    // 0.5 x 5 = 2.5 through (7.5,5), radius (25 + 2.5^2) / (2 x 2.5) = 6.25
    // about (13.75,5). Every point of a 21 x 21 grid over [-2,12]^2 is
    // previewed and then clicked: a refused preview must be a refused click,
    // an empty one too, and an Added point exactly the vertex the click adds.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    CurvePolyline2 shape =
        CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    shape.vertices[1].bulge = -0.5;
    const EntityId p = addCurve(driver, shape);
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    int onStraight = 0;
    int onArc = 0;
    int refused = 0;
    int outOfReach = 0;
    for (int i = 0; i <= 20; ++i) {
        for (int j = 0; j <= 20; ++j) {
            const double x = -2.0 + 14.0 * i / 20.0;
            const double y = -2.0 + 14.0 * j / 20.0;
            const ToolFeedback shown = driver.preview(x, y);
            const auto added = pointsOf(shown, FeedbackRole::Added);
            const std::size_t before = shapeOf(driver, p).vertices.size();
            const ToolStep step = driver.click(x, y);
            if (shown.refused || added.empty()) {
                EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected) << x << "," << y;
                EXPECT_EQ(shapeOf(driver, p).vertices.size(), before);
                ++(shown.refused ? refused : outOfReach);
                continue;
            }
            ASSERT_EQ(step.outcome, ToolStep::Outcome::Done) << x << "," << y << " " << step.message;
            const CurvePolyline2 after = shapeOf(driver, p);
            EXPECT_EQ(after.vertices.size(), before + 1) << x << "," << y;
            EXPECT_TRUE(std::ranges::any_of(after.vertices, [&](const auto& vertex) {
                return vertex.position == added.front();
            })) << "the Added point is exactly the vertex made, at " << x << "," << y;
            const auto targets = piecesOf(shown, FeedbackRole::Target);
            ASSERT_EQ(targets.size(), 1u);
            ++(std::holds_alternative<Arc2>(targets.front()) ? onArc : onStraight);
            ASSERT_TRUE(driver.document().undo().ok());
        }
    }
    std::printf("insert grid: %d on the straight segment, %d on the arc, %d refused near a "
                "vertex, %d out of reach\n",
                onStraight, onArc, refused, outOfReach);
    EXPECT_GT(onStraight, 0);
    EXPECT_GT(onArc, 0);
    EXPECT_GT(refused, 0);
    EXPECT_GT(outOfReach, 0);
}

// ---- what is already chosen ------------------------------------------------------------------

TEST(VertexToolFeedback, APreselectedPolylineIsNeverAskedForAgain)
{
    for (const char* id :
         {"draw.vertex.insert", "draw.vertex.delete", "draw.vertex.move", "draw.vertex.edit",
          "draw.vertex.straighten", "draw.vertex.start", "draw.vertex.height", "draw.vertex.grade",
          "draw.vertex.arc", "draw.vertex.line", "draw.vertex.fillet", "draw.vertex.chamfer"}) {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
        selectOnly(driver, {p});
        driver.start(id);
        const std::string prompt = driver.tool().prompt();
        EXPECT_EQ(prompt.find("Select the polyline"), std::string::npos) << id << ": " << prompt;
        EXPECT_TRUE(prompt.find("olyline " + idText(p)) != std::string::npos)
            << id << " names the polyline it has: " << prompt;
    }
}

TEST(VertexToolFeedback, AHotVertexAnswersDeleteAndEnterDeletesItInOneStep)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 3), Point2(10, 0)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete", {vertexHandle(driver, p, 1)});
    EXPECT_EQ(driver.tool().prompt(),
              "Press Enter to delete vertex 1 of polyline " + idText(p) + ", or click another vertex");
    const std::size_t undoCount = driver.document().history().undoCount();
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(driver.document().history().undoCount(), undoCount + 1);
    EXPECT_EQ(driver.document().history().undoName(), "VERTEX_DELETE");
    EXPECT_EQ(driver.tool().prompt(), "Click the vertex of polyline " + idText(p) + " to delete")
        << "the restarted tool asks for a click";
}

TEST(VertexToolFeedback, SeveralHotVerticesGoInOneStep)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(1, 1), Point2(2, 0), Point2(3, 1), Point2(4, 0)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete", {vertexHandle(driver, p, 1), vertexHandle(driver, p, 2)});
    EXPECT_EQ(driver.tool().prompt(), "Press Enter to delete the 2 chosen vertices, or click another vertex");
    // (8,8) is far from every vertex: a click there takes nothing, so the
    // preview is what Enter would take, and says it is Enter's.
    const ToolFeedback shown = driver.preview(8, 8);
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Removed).size(), 2u) << "what Enter would take";
    EXPECT_EQ(shown.caption, "Enter: delete 2 vertices");
    const std::size_t undoCount = driver.document().history().undoCount();
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(3, 1), Point2(4, 0)}));
    EXPECT_EQ(driver.document().history().undoCount(), undoCount + 1);
}

TEST(VertexToolFeedback, DeleteReachesOnlyAVertexNearTheCursor)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0)});
    driver.start("draw.vertex.delete");
    // (5,0) is ON the polyline, but 5 from the nearest vertex: before, that
    // deleted vertex 0 or 1.
    EXPECT_TRUE(driver.preview(5, 0).marks.empty());
    EXPECT_EQ(driver.pick(p, 5, 0).outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(shapeOf(driver, p).vertices.size(), 3u);
    EXPECT_EQ(driver.executed(), 0);
}

TEST(VertexToolFeedback, ASurveyPointOnAVertexDoesNotStealThePick)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0)});
    katana::entity::Entity mark;
    mark.geometry = katana::entity::PointGeometry{Point2(10, 0)};
    mark.properties["point"] = std::string("7");
    const EntityId survey = driver.add(katana::commands::createEntities({mark}));
    driver.start("draw.vertex.delete");
    // The view hands over whatever it picked - the point, drawn on top.
    const ToolStep step = driver.pick(survey, 10, 0.1);
    ASSERT_EQ(step.outcome, ToolStep::Outcome::Done) << step.message;
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(20, 0)}));
    EXPECT_NE(driver.document().model().entities.find(survey), nullptr);
}

TEST(VertexToolFeedback, ATypedNumberNamesTheVertex)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete");
    EXPECT_EQ(driver.type("7").outcome, ToolStep::Outcome::Rejected) << "no vertex 7";
    ASSERT_EQ(driver.type("1").outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(20, 0)}));
}

TEST(VertexToolFeedback, ATypedPointAnswersAPick)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0)});
    driver.start("draw.vertex.delete");
    ASSERT_EQ(driver.type("10.2,0").outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(20, 0)}));
}

TEST(VertexToolFeedback, TwoHotVerticesAnswerBothPicksOfStraighten)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(1, 1), Point2(2, -1), Point2(3, 1), Point2(4, 0)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.straighten", {vertexHandle(driver, p, 0), vertexHandle(driver, p, 4)});
    EXPECT_EQ(driver.tool().prompt(), "Press Enter to straighten polyline " + idText(p) +
                                          " from vertex 0 to 4, or click another vertex");
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(4, 0)}));
}

TEST(VertexToolFeedback, OneHotVertexAnswersTheFirstPickOfMove)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.move", {vertexHandle(driver, p, 1)});
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_EQ(driver.tool().prompt(), "Specify its new position");
    ASSERT_EQ(driver.type("@0,2").outcome, ToolStep::Outcome::Done) << "@ is from the vertex";
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(5, 2), Point2(10, 0)}));

    // Several hot vertices: which one to move is not known, and the tool says so.
    ToolDriver several;
    several.setPickTolerance(kAperture);
    const EntityId q = addPolyline(several, {Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    selectOnly(several, {q});
    several.start("draw.vertex.move", {vertexHandle(several, q, 0), vertexHandle(several, q, 1)});
    EXPECT_EQ(several.tool().prompt(), "2 vertices are chosen, and this tool moves one. Click the "
                                       "vertex of polyline " +
                                           idText(q) + " to move");
    EXPECT_EQ(several.tool().expects(), ToolInput::Entity);
}

TEST(VertexToolFeedback, AClickAfterTheHandlesAnsweredPicksAnew)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p =
        addPolyline(driver, {Point2(0, 0), Point2(5, 3), Point2(10, 0), Point2(15, 3)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete", {vertexHandle(driver, p, 1)});
    ASSERT_EQ(driver.click(10.1, 0).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(5, 3), Point2(15, 3)}))
        << "the vertex clicked goes, not the hot one";
}

TEST(VertexToolFeedback, EditVerticesWithASelectedPolylineDoesNotAskForOne)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.edit");
    EXPECT_EQ(driver.tool().prompt(), "Polyline " + idText(p) +
                                          " is in the Vertices panel; click another polyline, or "
                                          "press Enter");
    // Over nothing a click takes nothing: the preview is what Enter keeps in
    // the panel, and says it is Enter's, as every tool answered by what was
    // chosen before it does.
    const ToolFeedback shown = driver.preview(50, 50);
    EXPECT_EQ(shown.caption, "Enter: polyline " + idText(p) + " · 3 vertices, open");
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_TRUE(driver.finished());
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(driver.document().selection().ids(), std::vector<EntityId>{p});
}

TEST(VertexToolFeedback, TheSelectionPromptCountsInTheSingular)
{
    ToolDriver driver;
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.close");
    EXPECT_EQ(driver.tool().prompt(), "Press Enter to apply to the 1 selected polyline");
}

// ---- every other one-polyline tool shows what it will do ------------------------------------

TEST(VertexToolFeedback, DeleteShowsTheVertexThatGoesAndTheSegmentThatJoins)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 3), Point2(10, 0)});
    driver.start("draw.vertex.delete");
    const ToolFeedback shown = driver.preview(5.2, 2.9);
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Removed), std::vector<Point2>{Point2(5, 3)});
    const auto removed = piecesOf(shown, FeedbackRole::Removed);
    ASSERT_EQ(removed.size(), 2u);
    EXPECT_TRUE(isSegment(removed[0], Point2(0, 0), Point2(5, 3)));
    EXPECT_TRUE(isSegment(removed[1], Point2(5, 3), Point2(10, 0)));
    ASSERT_EQ(shown.shapes.size(), 1u);
    EXPECT_TRUE(isSegment(shown.shapes[0], Point2(0, 0), Point2(10, 0)));
    EXPECT_EQ(shown.caption, "delete vertex 1 · segments 0 and 1 become one");
    EXPECT_EQ(shown.focus, p);
}

TEST(VertexToolFeedback, MoveShowsTheVertexWhereItWasAndWhereItGoes)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    driver.start("draw.vertex.move");
    // Before the pick: only what the pick would take.
    const ToolFeedback hover = driver.preview(5, 0.2);
    EXPECT_EQ(pointsOf(hover, FeedbackRole::Target), std::vector<Point2>{Point2(5, 0)});
    EXPECT_EQ(labelsOf(hover, FeedbackRole::Target), std::vector<std::string>{"1"});
    ASSERT_EQ(driver.pick(p, 5, 0.2).outcome, ToolStep::Outcome::Continue);
    const ToolFeedback shown = driver.preview(5, 3);
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Target), std::vector<Point2>{Point2(5, 0)});
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Added), std::vector<Point2>{Point2(5, 3)});
    ASSERT_EQ(shown.shapes.size(), 2u);
    EXPECT_TRUE(isSegment(shown.shapes[0], Point2(0, 0), Point2(5, 3)));
    EXPECT_TRUE(isSegment(shown.shapes[1], Point2(5, 3), Point2(10, 0)));
    EXPECT_EQ(shown.caption.rfind("vertex 1 · moves 3.000", 0), 0u) << shown.caption;
    // x,y,z: the height too.
    ASSERT_EQ(driver.type("5,3,7.25").outcome, ToolStep::Outcome::Done);
    const CurvePolyline2 after = shapeOf(driver, p);
    EXPECT_EQ(after.vertices[1].position, Point2(5, 3));
    ASSERT_TRUE(after.vertices[1].height.has_value());
    EXPECT_DOUBLE_EQ(*after.vertices[1].height, 7.25);
}

TEST(VertexToolFeedback, StraightenMarksWhatGoesAndTakesTheShorterSideOfAClosedPolyline)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(1, 1), Point2(2, -1), Point2(3, 1), Point2(4, 0)});
    driver.start("draw.vertex.straighten");
    ASSERT_EQ(driver.pick(p, 0, 0).outcome, ToolStep::Outcome::Continue);
    const ToolFeedback shown = driver.preview(4, 0.1);
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Removed),
              (std::vector<Point2>{Point2(1, 1), Point2(2, -1), Point2(3, 1)}));
    EXPECT_EQ(labelsOf(shown, FeedbackRole::Target), (std::vector<std::string>{"keep", "keep"}));
    ASSERT_EQ(shown.shapes.size(), 1u);
    EXPECT_TRUE(isSegment(shown.shapes[0], Point2(0, 0), Point2(4, 0)));
    EXPECT_EQ(piecesOf(shown, FeedbackRole::Removed).size(), 4u) << "the path that goes";

    // A closed hexagon picked at vertex 3 then vertex 1: forward from 3 is
    // 4, 5 and 0; the other way only 2. The shorter side goes.
    ToolDriver ring;
    ring.setPickTolerance(kAperture);
    const std::vector<Point2> hexagon{Point2(0, 0),   Point2(10, 0),  Point2(20, 0),
                                      Point2(20, 10), Point2(10, 10), Point2(0, 10)};
    const EntityId h = addPolyline(ring, hexagon, true);
    ring.start("draw.vertex.straighten");
    ASSERT_EQ(ring.pick(h, 20, 10).outcome, ToolStep::Outcome::Continue);
    EXPECT_TRUE(ring.tool().prompt().ends_with("[Other side]")) << ring.tool().prompt();
    EXPECT_EQ(pointsOf(ring.preview(10, 0.1), FeedbackRole::Removed),
              std::vector<Point2>{Point2(20, 0)});
    ASSERT_EQ(ring.click(10, 0.1).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(ring, h), (std::vector<Point2>{Point2(0, 0), Point2(10, 0),
                                                        Point2(20, 10), Point2(10, 10),
                                                        Point2(0, 10)}));

    // O takes the other side instead: 4, 5 and 0 go.
    ToolDriver other;
    other.setPickTolerance(kAperture);
    const EntityId o = addPolyline(other, hexagon, true);
    other.start("draw.vertex.straighten");
    ASSERT_EQ(other.pick(o, 20, 10).outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(other.type("O").outcome, ToolStep::Outcome::Continue);
    EXPECT_EQ(pointsOf(other.preview(10, 0.1), FeedbackRole::Removed).size(), 3u);
    ASSERT_EQ(other.click(10, 0.1).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(other, o),
              (std::vector<Point2>{Point2(10, 0), Point2(20, 0), Point2(20, 10)}));
}

TEST(VertexToolFeedback, GradeShowsTheHeightsItWillSet)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false,
                                   {100.0, std::nullopt, 101.0});
    driver.start("draw.vertex.grade");
    // A vertex with no height is refused at the pick.
    const ToolFeedback noHeight = driver.preview(5, 0.1);
    EXPECT_TRUE(noHeight.refused);
    EXPECT_EQ(noHeight.caption, "vertex 1 has no height; grading needs one at both ends");
    EXPECT_EQ(driver.pick(p, 5, 0.1).outcome, ToolStep::Outcome::Rejected);
    ASSERT_EQ(driver.pick(p, 0, 0.1).outcome, ToolStep::Outcome::Continue);
    const ToolFeedback shown = driver.preview(10, 0.1);
    // 1 m over 10 m: vertex 1, halfway, at 100.5; a grade of 10 %.
    EXPECT_EQ(labelsOf(shown, FeedbackRole::Target),
              (std::vector<std::string>{"z 100.000", "z 101.000", "→ 100.500"}));
    EXPECT_EQ(shown.caption, "grade 0 → 2: 1 vertex, 100.000 to 101.000 (10.000 %)");
    ASSERT_EQ(driver.click(10, 0.1).outcome, ToolStep::Outcome::Done);
    EXPECT_NEAR(*shapeOf(driver, p).vertices[1].height, 100.5, 1.0e-12);
}

TEST(VertexToolFeedback, SetHeightDefaultsToTheHeightItHasOrWouldHave)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false,
                                   {0.0, std::nullopt, 10.0});
    katana::entity::Entity spot;
    spot.geometry = katana::entity::PointGeometry{Point2(30, 30)};
    katana::entity::setHeights(spot.properties, {7.5});
    driver.add(katana::commands::createEntities({spot}));
    driver.start("draw.vertex.height");
    // Vertex 1 has none: halfway between 0 and 10 by length is 5.
    ASSERT_EQ(driver.pick(p, 5, 0.1).outcome, ToolStep::Outcome::Continue);
    EXPECT_TRUE(driver.tool().prompt().ends_with("<5>")) << driver.tool().prompt();
    // A click takes the height of what it lands on.
    ASSERT_EQ(driver.click(30.1, 30).outcome, ToolStep::Outcome::Done);
    EXPECT_DOUBLE_EQ(*shapeOf(driver, p).vertices[1].height, 7.5);

    // Enter at a vertex's own height changes nothing: no undo step.
    const std::size_t undoCount = driver.document().history().undoCount();
    ASSERT_EQ(driver.pick(p, 0, 0.1).outcome, ToolStep::Outcome::Continue);
    EXPECT_TRUE(driver.tool().prompt().ends_with("<0>")) << driver.tool().prompt();
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(driver.document().history().undoCount(), undoCount);
}

TEST(VertexToolFeedback, ChangeStartShowsTheNewFirstVertexAndRefusesAnOpenPolyline)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true);
    const EntityId open = addPolyline(driver, {Point2(0, 20), Point2(10, 20)});
    driver.start("draw.vertex.start");
    const ToolFeedback shown = driver.preview(10.1, 10);
    EXPECT_EQ(labelsOf(shown, FeedbackRole::Target), std::vector<std::string>{"new 0"});
    EXPECT_EQ(shown.caption, "vertex 2 becomes vertex 0");
    const ToolFeedback openEnd = driver.preview(0.1, 20);
    EXPECT_TRUE(openEnd.refused) << "an open polyline has no start to change";
    EXPECT_EQ(labelsOf(openEnd, FeedbackRole::Target), std::vector<std::string>{"0"})
        << "the vertex refused is named, not labelled with the start it is refused";
    EXPECT_EQ(driver.pick(open, 0.1, 20).outcome, ToolStep::Outcome::Rejected);
    ASSERT_EQ(driver.pick(p, 10.1, 10).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(shapeOf(driver, p).vertices.front().position, Point2(10, 10));
}

TEST(VertexToolFeedback, SegmentToArcShowsTheSegmentThenTheArc)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(20, 0)});
    driver.start("draw.vertex.arc");
    const ToolFeedback segment = driver.preview(10, 0.1);
    ASSERT_EQ(piecesOf(segment, FeedbackRole::Target).size(), 1u);
    EXPECT_TRUE(isSegment(piecesOf(segment, FeedbackRole::Target)[0], Point2(0, 0), Point2(20, 0)))
        << "the segment, not a vertex";
    ASSERT_EQ(driver.pick(p, 10, 0.1).outcome, ToolStep::Outcome::Continue);
    // Through (10,10): the half circle about (10,0) of radius 10, clockwise
    // from (0,0) over the top, so bulge -1.
    const ToolFeedback arc = driver.preview(10, 10);
    ASSERT_EQ(arc.shapes.size(), 1u);
    const auto* shape = std::get_if<Arc2>(&arc.shapes[0]);
    ASSERT_NE(shape, nullptr);
    EXPECT_TRUE(closeTo(shape->center, Point2(10, 0), 1.0e-9));
    EXPECT_NEAR(shape->radius, 10.0, 1.0e-9);
    EXPECT_EQ(arc.caption, "segment 0 · radius 10.000");
    // In line with the ends: refused, before and at the click.
    EXPECT_TRUE(driver.preview(10, 0).refused);
    EXPECT_EQ(driver.click(10, 0).outcome, ToolStep::Outcome::Rejected);
    ASSERT_EQ(driver.click(10, 10).outcome, ToolStep::Outcome::Done);
    EXPECT_NEAR(shapeOf(driver, p).vertices[0].bulge, -1.0, 1.0e-12);

    // A hot segment answers the pick. Through (5,2) over (0,0)-(10,0): the
    // centre (5,k) with 25 + k^2 = (2 - k)^2, so k = -5.25 and r = 7.25.
    ToolDriver hot;
    hot.setPickTolerance(kAperture);
    const EntityId q = addPolyline(hot, {Point2(0, 0), Point2(10, 0), Point2(10, 5)});
    selectOnly(hot, {q});
    hot.start("draw.vertex.arc", {Grip{q, GripKind::SegmentMid, 0, Point2(5, 0)}});
    EXPECT_EQ(hot.tool().expects(), ToolInput::Point);
    const ToolFeedback through = hot.preview(5, 2);
    ASSERT_EQ(through.shapes.size(), 1u);
    const auto* bent = std::get_if<Arc2>(&through.shapes[0]);
    ASSERT_NE(bent, nullptr);
    EXPECT_TRUE(closeTo(bent->center, Point2(5, -5.25), 1.0e-9));
    EXPECT_NEAR(bent->radius, 7.25, 1.0e-9);
}

TEST(VertexToolFeedback, SegmentToLineShowsTheChordAndRefusesAStraightSegment)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    CurvePolyline2 shape =
        CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0), Point2(20, 0)});
    shape.vertices[0].bulge = 1.0; // below the chord, through (5,-5)
    const EntityId p = addCurve(driver, shape);
    driver.start("draw.vertex.line");
    const ToolFeedback shown = driver.preview(5, -5.1);
    ASSERT_EQ(piecesOf(shown, FeedbackRole::Target).size(), 1u);
    EXPECT_TRUE(std::holds_alternative<Arc2>(piecesOf(shown, FeedbackRole::Target)[0]));
    ASSERT_EQ(shown.shapes.size(), 1u);
    EXPECT_TRUE(isSegment(shown.shapes[0], Point2(0, 0), Point2(10, 0)));
    const ToolFeedback straight = driver.preview(15, 0.1);
    EXPECT_TRUE(straight.refused);
    EXPECT_EQ(straight.caption, "segment 1 is already straight");
    EXPECT_EQ(driver.pick(p, 15, 0.1).outcome, ToolStep::Outcome::Rejected);
}

TEST(VertexToolFeedback, FilletShowsTheArcAtTheLastRadiusAndRefusesAnEndAtThePick)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    // Make 1 the remembered radius, as a previous use would.
    const EntityId scratch = addPolyline(driver, {Point2(100, 0), Point2(110, 0), Point2(110, 10)});
    driver.start("draw.vertex.fillet");
    ASSERT_EQ(driver.pick(scratch, 110, 0.1).outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.type("1").outcome, ToolStep::Outcome::Done);

    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.fillet");
    // The corner (10,0) at radius 1: tangent 1 back along each side, at
    // (9,0) and (10,1), the arc about (9,1).
    const ToolFeedback shown = driver.preview(10, 0.1);
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Removed), std::vector<Point2>{Point2(10, 0)});
    const auto added = pointsOf(shown, FeedbackRole::Added);
    ASSERT_EQ(added.size(), 2u);
    EXPECT_TRUE(hasPoint(added, Point2(9, 0), 1.0e-12));
    EXPECT_TRUE(hasPoint(added, Point2(10, 1), 1.0e-12));
    ASSERT_EQ(shown.shapes.size(), 1u);
    const auto* arc = std::get_if<Arc2>(&shown.shapes[0]);
    ASSERT_NE(arc, nullptr);
    EXPECT_TRUE(closeTo(arc->center, Point2(9, 1), 1.0e-9));
    EXPECT_NEAR(arc->radius, 1.0, 1.0e-9);
    // An end vertex: refused at the pick, and no radius asked for.
    const std::string before = driver.tool().prompt();
    EXPECT_TRUE(driver.preview(0, 0.1).refused);
    EXPECT_EQ(driver.pick(p, 0, 0.1).outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), before);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity);
}

TEST(VertexToolFeedback, AFilletsCornerIsMarkedAsTheVertexThatGoesNotAsATakenOne)
{
    // The corner (10,0) of (0,0) (10,0) (10,10) goes, replaced by the arc:
    // its X, with no ring. What the pick acts on is the two segments the arc
    // is tangent to, (0,0)-(10,0) and (10,0)-(10,10), as Chamfer marks its
    // two. It was a Target ring AND the X on the corner: a circled X, the
    // sign for "cancel", and at the default radius - the arc a pixel or two
    // across - all the preview showed; beside the refusal's struck ring it
    // said "no" of a corner that was taken.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.fillet");
    const ToolFeedback shown = driver.preview(10, 0.1);
    ASSERT_FALSE(shown.refused);
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Removed), std::vector<Point2>{Point2(10, 0)});
    EXPECT_TRUE(pointsOf(shown, FeedbackRole::Target).empty())
        << "no ring on the corner that goes";
    const auto pieces = piecesOf(shown, FeedbackRole::Target);
    ASSERT_EQ(pieces.size(), 2u);
    EXPECT_TRUE(isSegment(pieces[0], Point2(0, 0), Point2(10, 0)));
    EXPECT_TRUE(isSegment(pieces[1], Point2(10, 0), Point2(10, 10)));
    // An end is refused as the vertex itself, named by its number and drawn
    // struck (every Target of a refused preview that flags none).
    const ToolFeedback end = driver.preview(0, 0.1);
    ASSERT_TRUE(end.refused);
    EXPECT_EQ(pointsOf(end, FeedbackRole::Target), std::vector<Point2>{Point2(0, 0)});
    EXPECT_EQ(labelsOf(end, FeedbackRole::Target), std::vector<std::string>{"0"});
    EXPECT_TRUE(piecesOf(end, FeedbackRole::Target).empty());
    // Chamfer's end, the same: the vertex struck with its number, where it
    // struck the one segment an end has, labelled with a distance, "d2",
    // that the refusal never asks for.
    driver.start("draw.vertex.chamfer");
    const ToolFeedback chamferEnd = driver.preview(0, 0.1);
    ASSERT_TRUE(chamferEnd.refused);
    EXPECT_EQ(pointsOf(chamferEnd, FeedbackRole::Target), std::vector<Point2>{Point2(0, 0)});
    EXPECT_EQ(labelsOf(chamferEnd, FeedbackRole::Target), std::vector<std::string>{"0"});
    EXPECT_TRUE(piecesOf(chamferEnd, FeedbackRole::Target).empty());
}

TEST(VertexToolFeedback, ChamferMarksItsFirstAndSecondSegments)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    driver.start("draw.vertex.chamfer");
    const ToolFeedback shown = driver.preview(10, 0.1);
    const auto targets = piecesOf(shown, FeedbackRole::Target);
    ASSERT_EQ(targets.size(), 2u);
    EXPECT_TRUE(isSegment(targets[0], Point2(0, 0), Point2(10, 0)));
    EXPECT_TRUE(isSegment(targets[1], Point2(10, 0), Point2(10, 10)));
    EXPECT_EQ(labelsOf(shown, FeedbackRole::Target), (std::vector<std::string>{"d1", "d2"}))
        << "the distances along them, not numbers that read as segment numbers";
    ASSERT_EQ(driver.pick(p, 10, 0.1).outcome, ToolStep::Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt().rfind("Specify the first distance, along segment 0", 0), 0u)
        << driver.tool().prompt();
    ASSERT_EQ(driver.type("1").outcome, ToolStep::Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt().rfind("Specify the second distance, along segment 1", 0), 0u)
        << driver.tool().prompt();
    // The preview at the second distance's default shows the bevel from
    // (9,0); the commit with 2 is the bevel (9,0)-(10,2).
    ASSERT_EQ(driver.type("2").outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(9, 0), Point2(10, 2), Point2(10, 10)}));
}

namespace {

// A preview's marks as text - role, place, label, refused - with its caption,
// to tell one preview from another.
std::string describe(const ToolFeedback& feedback)
{
    std::string text = feedback.caption + (feedback.refused ? " refused" : "");
    for (const auto& mark : feedback.marks) {
        text += " | " + std::string(katana::cad::toString(mark.role)) + " " + mark.label +
                (mark.refused ? " (refused)" : "");
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&mark.geometry)) {
            text += " at " + katana::core::formatExactReal(point->position.x) + "," +
                    katana::core::formatExactReal(point->position.y);
        } else if (const auto* segment = std::get_if<Segment2>(&mark.geometry)) {
            text += " segment " + katana::core::formatExactReal(segment->midpoint().x) + "," +
                    katana::core::formatExactReal(segment->midpoint().y);
        } else if (const auto* arc = std::get_if<Arc2>(&mark.geometry)) {
            text += " arc " + katana::core::formatExactReal(arc->midpoint().x) + "," +
                    katana::core::formatExactReal(arc->midpoint().y);
        } else {
            text += " whole";
        }
    }
    return text;
}

// Whether `ghost` - the result as the preview drew it - is one of the
// segments of `result`, the polyline the click made: at its ends and middle.
bool isPieceOf(const CurvePolyline2& result, const Geometry& ghost)
{
    const auto same = [](const auto& a, const auto& b) {
        return closeTo(a.pointAt(0.0), b.pointAt(0.0), 1.0e-9) &&
               closeTo(a.pointAt(0.5), b.pointAt(0.5), 1.0e-9) &&
               closeTo(a.pointAt(1.0), b.pointAt(1.0), 1.0e-9);
    };
    for (std::size_t i = 0; i < result.segmentCount(); ++i) {
        const bool match = std::visit(
            [&](const auto& piece) {
                if (const auto* line = std::get_if<Segment2>(&ghost)) {
                    return same(piece, *line);
                }
                if (const auto* arc = std::get_if<Arc2>(&ghost)) {
                    return same(piece, *arc);
                }
                return false;
            },
            result.segment(i));
        if (match) {
            return true;
        }
    }
    return false;
}

// The vertex of `shape` at `at`, if one is.
const katana::geometry::CurveVertex* vertexAt(const CurvePolyline2& shape, const Point2& at)
{
    for (const auto& vertex : shape.vertices) {
        if (closeTo(vertex.position, at)) {
            return &vertex;
        }
    }
    return nullptr;
}

} // namespace

TEST(VertexToolFeedback, APreviewNeverPromisesWhatTheClickDoesNot)
{
    // Every pick tool but Insert - whose own finer grid is
    // TheInsertPreviewIsWhatTheClickCommits - at each step a click answers,
    // over a 9 x 9 grid round a small polyline, with and without grips chosen
    // before the tool:
    //   - a refused preview, one with no mark, or the tool's idle picture (as
    //     far off, where nothing is in reach) is a refused click;
    //   - one captioned "Enter: " is a refused click that leaves the tool
    //     where it was, and Enter then does what was shown;
    //   - ANY other preview is a click the tool takes: it is done, or goes on
    //     to its next step - where that step has a default, Enter takes it.
    // Once done, the edit is the one shown: every Added point a vertex, every
    // Removed vertex gone, every ghost a piece of the result, "new 0" the
    // start, a "→ z" label the height given, Set Height's caption the height
    // set, Edit Vertices' polyline selected. And no vertex is marked both
    // taken and gone: a ring with an X over it is a circled X, "cancel".
    // It asserted only where Added, Removed or a ghost was shown, and so
    // checked nothing of Change Start, Set Height or Edit Vertices.
    struct Case {
        const char* tool;
        bool closed = false;
        double bulge = 0.0;             // of segment 0
        std::vector<const char*> first{}; // typed before the grid's step
        std::vector<int> hot{};           // vertices chosen before the tool
        int hotSegment = -1;            // a segment's middle chosen before the tool
        bool heights = false;           // 100, 101, 102 and 103 at the vertices
    };
    const std::vector<Case> cases{
        {"draw.vertex.delete"},
        {"draw.vertex.delete", true},
        {"draw.vertex.delete", true, 0.0, {}, {2}},
        {"draw.vertex.delete", false, 0.0, {}, {1, 2}},
        {"draw.vertex.move"},
        {"draw.vertex.move", false, 0.0, {"1"}},
        {"draw.vertex.move", true, 0.0, {}, {2}},
        {"draw.vertex.edit"},
        {"draw.vertex.straighten", true},
        {"draw.vertex.straighten", true, 0.0, {"0"}},
        {"draw.vertex.straighten", true, 0.0, {}, {0, 2}},
        {"draw.vertex.start", true},
        {"draw.vertex.start", true, 0.0, {}, {2}},
        {"draw.vertex.start"},
        {"draw.vertex.height", false, 0.0, {}, {}, -1, true},
        {"draw.vertex.height", false, 0.0, {"1"}, {}, -1, true},
        {"draw.vertex.grade", true},
        {"draw.vertex.grade", true, 0.0, {"0"}, {}, -1, true},
        {"draw.vertex.arc"},
        {"draw.vertex.arc", false, 0.0, {"0"}},
        {"draw.vertex.line", false, 0.5},
        {"draw.vertex.line", false, 0.5, {}, {}, 0},
        {"draw.vertex.fillet"},
        {"draw.vertex.fillet", true},
        {"draw.vertex.chamfer", true},
    };
    const std::vector<Point2> corners{Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)};
    const std::vector<double> zs{100.0, 101.0, 102.0, 103.0};
    int refusedEver = 0;
    int enterEver = 0;
    for (const Case& c : cases) {
        int taken = 0;
        int finished = 0;
        int refused = 0;
        int enterShown = 0;
        for (int i = 0; i <= 8; ++i) {
            for (int j = 0; j <= 8; ++j) {
                const double x = -1.0 + 12.0 * i / 8.0;
                const double y = -1.0 + 12.0 * j / 8.0;
                const std::string where = std::string(c.tool) + " at " +
                                          katana::core::formatExactReal(x) + "," +
                                          katana::core::formatExactReal(y);
                ToolDriver driver;
                driver.setPickTolerance(1.0); // a vertex within 1.5, a segment within 1
                CurvePolyline2 shape = CurvePolyline2::fromPoints(corners, c.closed);
                shape.vertices[0].bulge = c.bulge;
                if (c.heights) {
                    for (std::size_t k = 0; k < shape.vertices.size(); ++k) {
                        shape.vertices[k].height = zs[k];
                    }
                }
                const EntityId p = addCurve(driver, shape);
                selectOnly(driver, {p});
                std::vector<Grip> handles;
                for (const int v : c.hot) {
                    handles.push_back(vertexHandle(driver, p, static_cast<std::size_t>(v)));
                }
                if (c.hotSegment >= 0) {
                    const auto piece = shape.segment(static_cast<std::size_t>(c.hotSegment));
                    const Point2 middle =
                        std::visit([](const auto& segment) { return segment.pointAt(0.5); }, piece);
                    handles.push_back(Grip{p, GripKind::SegmentMid,
                                           static_cast<std::size_t>(c.hotSegment), middle});
                }
                driver.start(c.tool, handles);
                for (const char* typed : c.first) {
                    ASSERT_EQ(driver.type(typed).outcome, ToolStep::Outcome::Continue) << where;
                }
                const ToolFeedback shown = driver.preview(x, y);
                const ToolFeedback away = driver.preview(1000, 1000);
                const std::string prompt = driver.tool().prompt();
                ToolStep step = driver.click(x, y);
                const bool promised = shown.caption.rfind("Enter: ", 0) != 0 && !shown.refused &&
                                      !shown.marks.empty() && describe(shown) != describe(away);
                if (shown.caption.rfind("Enter: ", 0) == 0) {
                    // Nothing a click takes is here: refused, the tool where
                    // it was, and Enter does what was shown - or is refused
                    // as it showed.
                    EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected) << where;
                    EXPECT_EQ(driver.tool().prompt(), prompt) << where << ": the tool is kept";
                    step = driver.enter();
                    ++enterShown;
                    if (shown.refused) {
                        EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected) << where;
                        continue;
                    }
                } else if (!promised) {
                    EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected)
                        << where << " shows " << describe(shown);
                    refused += shown.refused ? 1 : 0;
                    continue;
                } else {
                    ASSERT_NE(step.outcome, ToolStep::Outcome::Rejected)
                        << where << " showed " << describe(shown) << " and said " << step.message;
                    ++taken;
                }
                // A next step with a default (a radius, two distances, a
                // height) is Enter's; one that wants a point or a pick ends
                // the case, the click having been taken.
                for (int k = 0; k < 3 && step.outcome == ToolStep::Outcome::Continue &&
                                driver.tool().prompt().ends_with(">");
                     ++k) {
                    step = driver.enter();
                }
                if (step.outcome != ToolStep::Outcome::Done) {
                    EXPECT_EQ(step.outcome, ToolStep::Outcome::Continue) << where << ": "
                                                                         << step.message;
                    continue;
                }
                ++finished;
                const CurvePolyline2 after = shapeOf(driver, p);
                const auto positions = after.positions();
                for (const Point2& added : pointsOf(shown, FeedbackRole::Added)) {
                    EXPECT_TRUE(hasPoint(positions, added)) << where;
                }
                const auto targets = pointsOf(shown, FeedbackRole::Target);
                for (const Point2& gone : pointsOf(shown, FeedbackRole::Removed)) {
                    EXPECT_FALSE(hasPoint(positions, gone)) << where;
                    EXPECT_FALSE(hasPoint(targets, gone))
                        << where << ": a vertex marked taken and gone, a circled X";
                }
                for (const Geometry& ghost : shown.shapes) {
                    EXPECT_TRUE(isPieceOf(after, ghost)) << where << ": a ghost not in the result";
                }
                for (const auto& mark : shown.marks) {
                    const auto* point = std::get_if<katana::entity::PointGeometry>(&mark.geometry);
                    if (point == nullptr || mark.role != FeedbackRole::Target) {
                        continue;
                    }
                    if (mark.label == "new 0") {
                        EXPECT_TRUE(closeTo(after.vertices.front().position, point->position))
                            << where << ": the vertex shown as the new start";
                    }
                    if (mark.label.rfind("→ ", 0) == 0) {
                        const auto* vertex = vertexAt(after, point->position);
                        ASSERT_NE(vertex, nullptr) << where;
                        EXPECT_EQ("→ " + katana::cad::heightText(vertex->height).substr(2),
                                  mark.label)
                            << where << ": the height a grade label promised";
                    }
                }
                if (std::string_view(c.tool) == "draw.vertex.edit") {
                    EXPECT_EQ(driver.document().selection().ids(), std::vector<EntityId>{p})
                        << where;
                }
                if (std::string_view(c.tool) == "draw.vertex.height" && !c.first.empty()) {
                    // The value step's click takes the height nearest within
                    // the vertex reach, 1.5; Enter (nothing in reach) keeps
                    // vertex 1's 101 - worked from the corners, not the tool.
                    std::optional<double> expected = zs[1];
                    double nearest = 1.5;
                    for (std::size_t k = 0; k < corners.size(); ++k) {
                        const double d = corners[k].distanceTo(Point2(x, y));
                        if (d <= nearest) {
                            nearest = d;
                            expected = zs[k];
                        }
                    }
                    EXPECT_EQ(after.vertices[1].height, expected) << where;
                    const auto arrow = shown.caption.find("→ ");
                    if (expected != zs[1]) {
                        ASSERT_NE(arrow, std::string::npos) << where << ": " << shown.caption;
                        EXPECT_EQ(shown.caption.substr(arrow + std::string("→ ").size()),
                                  katana::cad::heightText(expected).substr(2))
                            << where << ": the caption said the height the click set";
                    } else {
                        EXPECT_EQ(arrow, std::string::npos) << where << ": " << shown.caption;
                    }
                }
            }
        }
        std::printf("preview grid: %s%s%s: %d taken (%d finished), %d refused, %d Enter's\n",
                    c.tool, c.first.empty() ? "" : " after a pick",
                    c.hot.empty() && c.hotSegment < 0 ? "" : " with grips chosen", taken, finished,
                    refused, enterShown);
        // Every case reaches a click the tool takes or Enter's, except a tool
        // shown all refusals on purpose: Change Start on an open polyline,
        // Grade with no heights.
        const bool allRefused = (std::string_view(c.tool) == "draw.vertex.start" && !c.closed) ||
                                (std::string_view(c.tool) == "draw.vertex.grade" && !c.heights);
        if (allRefused) {
            EXPECT_EQ(taken + enterShown, 0) << c.tool;
            EXPECT_GT(refused, 0) << c.tool;
        } else {
            EXPECT_GT(taken + enterShown, 0) << c.tool;
        }
        refusedEver += refused;
        enterEver += enterShown;
    }
    EXPECT_GT(refusedEver, 0);
    EXPECT_GT(enterEver, 0);
}

// ---- the review of 2026-09-30: what the preview and the click must agree on ----------------

TEST(VertexToolFeedback, WithAChosenVertexAClickIsPreviewedWhereItTakesSomethingAndEnterElsewhere)
{
    // The square (0,0) (10,0) (10,10) (0,10), closed, with vertex 2 chosen
    // before Delete Vertex. Over vertex 0 a click deletes vertex 0, so that
    // is what the preview marks: it marked vertex 2, and the click deleted
    // a vertex nobody had marked. Over nothing a click takes nothing, and
    // the preview is Enter's deletion of vertex 2, saying so.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true);
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete", {vertexHandle(driver, p, 2)});
    const std::string ready =
        "Press Enter to delete vertex 2 of polyline " + idText(p) + ", or click another vertex";
    ASSERT_EQ(driver.tool().prompt(), ready);

    // (0.2, 0.1) is sqrt(0.05) = 0.22 from vertex 0, within the 0.75.
    const ToolFeedback overVertex0 = driver.preview(0.2, 0.1);
    EXPECT_EQ(pointsOf(overVertex0, FeedbackRole::Removed), std::vector<Point2>{Point2(0, 0)});
    EXPECT_EQ(overVertex0.caption, "delete vertex 0 · segments 3 and 0 become one");
    // (5,5) is sqrt(50) = 7.07 from every vertex.
    const ToolFeedback overNothing = driver.preview(5, 5);
    EXPECT_EQ(pointsOf(overNothing, FeedbackRole::Removed), std::vector<Point2>{Point2(10, 10)});
    EXPECT_EQ(overNothing.caption, "Enter: delete vertex 2 · segments 1 and 2 become one");

    // A click on nothing is refused and keeps the choice: it dropped it.
    EXPECT_EQ(driver.click(5, 5).outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), ready);
    EXPECT_EQ(driver.executed(), 0);
    // The click over vertex 0 deletes vertex 0, as the preview showed.
    ASSERT_EQ(driver.click(0.2, 0.1).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(10, 0), Point2(10, 10), Point2(0, 10)}));
}

TEST(VertexToolFeedback, ChangeStartWithAChosenVertexPreviewsTheVertexAClickWouldTake)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true);
    selectOnly(driver, {p});
    driver.start("draw.vertex.start", {vertexHandle(driver, p, 2)});
    const ToolFeedback over3 = driver.preview(0.1, 10.1);
    EXPECT_EQ(pointsOf(over3, FeedbackRole::Target), std::vector<Point2>{Point2(0, 10)});
    EXPECT_EQ(over3.caption, "vertex 3 becomes vertex 0");
    EXPECT_EQ(driver.preview(5, 5).caption, "Enter: vertex 2 becomes vertex 0");
    ASSERT_EQ(driver.click(0.1, 10.1).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(shapeOf(driver, p).vertices.front().position, Point2(0, 10));
}

TEST(VertexToolFeedback, BesideAChosenVertexThePlaceEnterAddsAtIsMarked)
{
    // (0,0) (10,0) (10,10) with vertex 1 chosen: Enter adds at the middle
    // of the segment after it, (10,5), whichever segment the cursor is by -
    // it added on an edge the preview never marked - so that place is
    // marked while the click's own place follows the cursor.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert", {vertexHandle(driver, p, 1)});
    for (const Point2& cursor : {Point2(7, 1), Point2(11, 4)}) {
        const ToolFeedback shown = driver.preview(cursor.x, cursor.y);
        EXPECT_EQ(pointsOf(shown, FeedbackRole::Enter), std::vector<Point2>{Point2(10, 5)});
        EXPECT_EQ(labelsOf(shown, FeedbackRole::Enter), std::vector<std::string>{"Enter"});
    }
    // By segment 0 the click's place is on segment 0, and Enter's stays.
    EXPECT_TRUE(hasPoint(pointsOf(driver.preview(7, 1), FeedbackRole::Added), Point2(7, 0)));
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(10, 0),
                                                           Point2(10, 5), Point2(10, 10)}));
}

TEST(VertexToolFeedback, BackOnTheVertexJustPickedTheClickIsRefusedAndThePreviewSaysSo)
{
    // A second click on Straighten's first vertex is refused, so the preview
    // there is a refusal (R3): it showed the pick as taken, and a preview
    // that was not refused was a click that was. Straight after the pick the
    // view holds the refusal off (ToolHost.AStraightenPicksKeepRingIsHeld...).
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(1, 1), Point2(2, -1), Point2(3, 1), Point2(4, 0)});
    driver.start("draw.vertex.straighten");
    ASSERT_EQ(driver.pick(p, 0, 0.1).outcome, ToolStep::Outcome::Continue);
    const ToolFeedback same = driver.preview(0, 0.1);
    EXPECT_TRUE(same.refused);
    // The first pick's mark stays, which says what was taken; flagged as no
    // reason of its own, it is drawn as the refusal (ToolFeedback::refused).
    EXPECT_EQ(pointsOf(same, FeedbackRole::Target), std::vector<Point2>{Point2(0, 0)});
    EXPECT_EQ(labelsOf(same, FeedbackRole::Target), std::vector<std::string>{"keep"});
    EXPECT_EQ(same.caption, "vertex 0 is picked already; click the other one");
    const ToolStep again = driver.click(0, 0.1);
    EXPECT_EQ(again.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(again.message, same.caption) << "the click says what the preview said";
    // Off the polyline the first pick stays marked, and nothing is refused.
    const ToolFeedback away = driver.preview(50, 50);
    EXPECT_FALSE(away.refused);
    EXPECT_EQ(labelsOf(away, FeedbackRole::Target), std::vector<std::string>{"keep"});
    EXPECT_EQ(away.focus, p);
}

TEST(VertexToolFeedback, FilletAndChamferRememberWhatWasTypedForTheNextUse)
{
    // 2.5, then 1.5 and 0.75 - none of them the 1 the tools start with, so a
    // tool that forgot would offer and preview 1.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId first = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    const EntityId second = addPolyline(driver, {Point2(20, 0), Point2(30, 0), Point2(30, 10)});
    driver.start("draw.vertex.fillet");
    ASSERT_EQ(driver.pick(first, 10, 0.1).outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.type("2.5").outcome, ToolStep::Outcome::Done);
    // At a right angle the tangent points are the radius back along each
    // side: (27.5,0) and (30,2.5).
    const ToolFeedback round = driver.preview(30, 0.1);
    const auto tangents = pointsOf(round, FeedbackRole::Added);
    ASSERT_EQ(tangents.size(), 2u);
    EXPECT_TRUE(hasPoint(tangents, Point2(27.5, 0), 1.0e-12));
    EXPECT_TRUE(hasPoint(tangents, Point2(30, 2.5), 1.0e-12));
    EXPECT_EQ(round.caption, "fillet vertex 1 · radius 2.500");
    ASSERT_EQ(driver.pick(second, 30, 0.1).outcome, ToolStep::Outcome::Continue);
    EXPECT_TRUE(driver.tool().prompt().ends_with("<2.5>")) << driver.tool().prompt();
    ASSERT_EQ(driver.type("1").outcome, ToolStep::Outcome::Done); // as it started

    const EntityId third = addPolyline(driver, {Point2(0, 20), Point2(10, 20), Point2(10, 30)});
    const EntityId fourth = addPolyline(driver, {Point2(20, 20), Point2(30, 20), Point2(30, 30)});
    driver.start("draw.vertex.chamfer");
    ASSERT_EQ(driver.pick(third, 10, 20.1).outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.type("1.5").outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.type("0.75").outcome, ToolStep::Outcome::Done);
    // The bevel 1.5 back along the incoming side, 0.75 along the outgoing.
    const auto cut = pointsOf(driver.preview(30, 20.1), FeedbackRole::Added);
    ASSERT_EQ(cut.size(), 2u);
    EXPECT_TRUE(hasPoint(cut, Point2(28.5, 20), 1.0e-12));
    EXPECT_TRUE(hasPoint(cut, Point2(30, 20.75), 1.0e-12));
    ASSERT_EQ(driver.pick(fourth, 30, 20.1).outcome, ToolStep::Outcome::Continue);
    EXPECT_TRUE(driver.tool().prompt().ends_with("<1.5>")) << driver.tool().prompt();
    ASSERT_EQ(driver.type("1").outcome, ToolStep::Outcome::Continue);
    EXPECT_TRUE(driver.tool().prompt().ends_with("<0.75>")) << driver.tool().prompt();
    ASSERT_EQ(driver.type("1").outcome, ToolStep::Outcome::Done); // as it started
}

TEST(VertexToolFeedback, MoveVertexTakesARelativeDzAsAChangeOfItsHeight)
{
    // @0,0,1 raises the vertex by 1, as the VERTEX MOVE verb reads it: it
    // set the height to 1, and a surveyed 101.5 was lost.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false,
                                   {100.0, 101.5, 100.0});
    driver.start("draw.vertex.move");
    ASSERT_EQ(driver.pick(p, 5, 0.1).outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.type("@0,0,1").outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(shapeOf(driver, p).vertices[1].position, Point2(5, 0));
    EXPECT_DOUBLE_EQ(*shapeOf(driver, p).vertices[1].height, 102.5);
    ASSERT_EQ(driver.pick(p, 5, 0.1).outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.type("@1,0,-0.5").outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(shapeOf(driver, p).vertices[1].position, Point2(6, 0));
    EXPECT_DOUBLE_EQ(*shapeOf(driver, p).vertices[1].height, 102.0);

    // A vertex with no height has none for a dz to change: refused, never
    // read as a height of dz, and the tool still asks where it goes.
    const EntityId flat = addPolyline(driver, {Point2(0, 5), Point2(5, 5), Point2(10, 5)});
    ASSERT_EQ(driver.pick(flat, 5, 5.1).outcome, ToolStep::Outcome::Continue);
    const ToolStep refused = driver.type("@0,0,1");
    EXPECT_EQ(refused.outcome, ToolStep::Outcome::Rejected);
    EXPECT_NE(refused.message.find("no height"), std::string::npos) << refused.message;
    EXPECT_FALSE(shapeOf(driver, flat).vertices[1].height.has_value());
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    // x,y,z is still the height itself.
    ASSERT_EQ(driver.type("5,5,7").outcome, ToolStep::Outcome::Done);
    EXPECT_DOUBLE_EQ(*shapeOf(driver, flat).vertices[1].height, 7.0);
}

TEST(VertexToolFeedback, AThreeDPolylineReadsARelativeDzAsThePLINE3DVerbDoes)
{
    // One line, one polyline, in the window and through katana_cli and
    // katana_mcp: the PLINE3D verb takes the z of @dx,dy,dz as the vertex's
    // height (docs/drawing.md, "The command line": a z is a vertex's
    // height), and the tool reads it as its verb does. The tool once
    // climbed dz from the last vertex - 101.5 from 100 - while the verb put
    // the same vertex at 1.5. Move Vertex's "raise it by dz" is VERTEX
    // MOVE's reading, and stays (MoveVertexTakesARelativeDzAsAChangeOfItsHeight).
    ToolDriver driver;
    driver.start("draw.polyline3d");
    ASSERT_EQ(driver.type("0,0,100").outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.type("@10,0,1.5").outcome, ToolStep::Outcome::Continue);
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    const auto made = driver.document().lastCreatedEntities();
    ASSERT_EQ(made.size(), 1u);
    const CurvePolyline2 drawn = shapeOf(driver, made.front());
    ASSERT_EQ(drawn.vertices.size(), 2u);
    EXPECT_EQ(drawn.vertices[1].position, Point2(10, 0));
    ASSERT_TRUE(drawn.vertices[1].height.has_value());
    EXPECT_DOUBLE_EQ(*drawn.vertices[1].height, 1.5);

    katana::cad::CommandInterpreter verbs{driver.document()};
    ASSERT_TRUE(verbs.run("PLINE3D 0,0,100 @10,0,1.5").ok());
    const auto typed = driver.document().lastCreatedEntities();
    ASSERT_EQ(typed.size(), 1u);
    const CurvePolyline2 fromVerb = shapeOf(driver, typed.front());
    ASSERT_EQ(fromVerb.vertices.size(), 2u);
    EXPECT_EQ(fromVerb.vertices[1].position, drawn.vertices[1].position);
    EXPECT_EQ(fromVerb.vertices[1].height, drawn.vertices[1].height)
        << "the window's tool and the verb read one line one way";
}

TEST(VertexToolFeedback, APolylineOnAHiddenOrLockedLayerIsNotTakenThoughItIsSelected)
{
    // Selected, then its layer hidden in the drawing, hidden in this view, or
    // locked: the selection is not pruned, and the tools took the polyline
    // through it - marks on ground nobody could see, and an edit; on a locked
    // layer a preview the command then refused.
    for (const std::string_view how : {"hidden", "hidden in the view", "locked"}) {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        katana::cad::CommandInterpreter layers{driver.document()};
        ASSERT_TRUE(layers.run("LAYER NEW B").ok());
        katana::entity::Entity entity;
        entity.geometry = Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10)}, false};
        entity.layer = "B";
        const EntityId p = driver.add(katana::commands::createEntities({entity}));
        selectOnly(driver, {p});
        if (how == "hidden") {
            ASSERT_TRUE(layers.run("LAYER HIDE B").ok());
        } else if (how == "hidden in the view") {
            driver.hideInView("B");
        } else {
            ASSERT_TRUE(layers.run("LAYER LOCK B").ok());
        }
        for (const char* tool : {"draw.vertex.delete", "draw.vertex.insert"}) {
            driver.start(tool);
            // Vertex 1, and the middle of segment 0.
            EXPECT_TRUE(driver.preview(10, 0.1).marks.empty()) << how << ", " << tool;
            EXPECT_TRUE(driver.preview(5, 0.1).marks.empty()) << how << ", " << tool;
            EXPECT_EQ(driver.click(10, 0.1).outcome, ToolStep::Outcome::Rejected)
                << how << ", " << tool;
            EXPECT_EQ(driver.click(5, 0.1).outcome, ToolStep::Outcome::Rejected)
                << how << ", " << tool;
            const std::string prompt = driver.tool().prompt();
            if (how == "locked") {
                EXPECT_EQ(prompt.rfind("polyline " + idText(p) +
                                           " is on the locked layer B; unlock it to edit it. "
                                           "Click",
                                       0),
                          0u)
                    << tool << ": " << prompt;
            } else {
                EXPECT_EQ(prompt.find("olyline " + idText(p)), std::string::npos)
                    << "no prompt names what cannot be seen: " << tool << ": " << prompt;
            }
        }
        EXPECT_EQ(driver.executed(), 0) << how;
        EXPECT_EQ(shapeOf(driver, p).vertices.size(), 3u) << how;
    }
}

TEST(VertexToolFeedback, SetHeightTakesNoHeightFromAPointTheViewHides)
{
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    katana::cad::CommandInterpreter layers{driver.document()};
    ASSERT_TRUE(layers.run("LAYER NEW H").ok());
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    katana::entity::Entity spot;
    spot.geometry = katana::entity::PointGeometry{Point2(30, 30)};
    spot.layer = "H";
    katana::entity::setHeights(spot.properties, {7.5});
    driver.add(katana::commands::createEntities({spot}));
    driver.hideInView("H");
    driver.start("draw.vertex.height");
    ASSERT_EQ(driver.pick(p, 5, 0.1).outcome, ToolStep::Outcome::Continue);
    EXPECT_EQ(driver.click(30.1, 30).outcome, ToolStep::Outcome::Rejected);
    EXPECT_FALSE(shapeOf(driver, p).vertices[1].height.has_value());
}

TEST(VertexToolFeedback, InsertTakesNoSnapOffTheArcItInsertsInto)
{
    // The arc from (0,0) to (10,0) of bulge 1: the half circle about (5,0),
    // radius 5, below the chord. The cursor (1.5,-3.6) is sqrt(25.21) - 5 =
    // 0.021 off it, far from its ends and middle, so the only snap there is
    // its Center, (5,0), which the arc offers while it is hovered - 5 off
    // the line, where no pick reaches: Insert showed nothing along an arc
    // but at its very middle.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    CurvePolyline2 shape = CurvePolyline2::fromPoints({Point2(0, 0), Point2(10, 0)});
    shape.vertices[0].bulge = 1.0;
    const EntityId arc = addCurve(driver, shape);
    selectOnly(driver, {arc});
    driver.start("draw.vertex.insert");
    katana::cad::SnapRequest plain;
    plain.cursor = Point2(1.5, -3.6);
    plain.aperture = 0.5;
    const auto centre = katana::cad::snap(driver.document().model(), plain);
    ASSERT_TRUE(centre.has_value());
    ASSERT_EQ(centre->mode, katana::cad::SnapMode::Center) << "what took the cursor before";
    EXPECT_FALSE(driver.snapAt(1.5, -3.6).has_value());
    const auto added = pointsOf(driver.previewSnapped(1.5, -3.6), FeedbackRole::Added);
    ASSERT_EQ(added.size(), 1u);
    // On the arc, by the cursor: its projection from the centre.
    EXPECT_NEAR(added[0].distanceTo(Point2(5, 0)), 5.0, 1.0e-12);
    EXPECT_NEAR(added[0].distanceTo(Point2(1.5, -3.6)), std::sqrt(25.21) - 5.0, 1.0e-12);
    // The arc's Midpoint (5,-5) is on it: taken, and the vertex goes there.
    const auto middle = driver.snapAt(5, -5.3);
    ASSERT_TRUE(middle.has_value());
    EXPECT_EQ(middle->mode, katana::cad::SnapMode::Midpoint);
    ASSERT_EQ(driver.clickSnapped(5, -5.3).outcome, ToolStep::Outcome::Done);
    EXPECT_TRUE(hasPoint(positionsOf(driver, arc), Point2(5, -5), 1.0e-12));
}

TEST(VertexToolFeedback, InsertTakesNoSnapBesideTheLineNorOnAVertexOfIt)
{
    // (0,0)-(30,0), a pick reach of 0.3 and the snap aperture of 0.5: the
    // view's 8 and 12 px in near enough their proportion. A line's end at
    // (10,0.4) is 0.4 off the polyline: snapped to, the point left the
    // pick's reach, and Insert showed nothing and refused the click. The
    // polyline's own vertex (0,0) is too close to insert at. Both are passed
    // over and the cursor stays; a crossing ON the line is still taken.
    ToolDriver driver;
    driver.setPickTolerance(0.3);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(30, 0)});
    driver.add(katana::commands::createLine(Point2(10, 0.4), Point2(10, 5)));
    driver.add(katana::commands::createLine(Point2(20, -5), Point2(20, 7)));
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert");
    EXPECT_FALSE(driver.snapAt(10.3, 0.05).has_value());
    EXPECT_TRUE(hasPoint(pointsOf(driver.previewSnapped(10.3, 0.05), FeedbackRole::Added),
                         Point2(10.3, 0)));
    EXPECT_FALSE(driver.snapAt(0.4, 0.05).has_value());
    EXPECT_TRUE(hasPoint(pointsOf(driver.previewSnapped(0.4, 0.05), FeedbackRole::Added),
                         Point2(0.4, 0)));
    const auto crossing = driver.snapAt(20.2, 0.1);
    ASSERT_TRUE(crossing.has_value());
    EXPECT_EQ(crossing->mode, katana::cad::SnapMode::Intersection);
    ASSERT_EQ(driver.clickSnapped(20.2, 0.1).outcome, ToolStep::Outcome::Done);
    EXPECT_TRUE(hasPoint(positionsOf(driver, p), Point2(20, 0), 1.0e-12));
}

TEST(VertexToolFeedback, AChosenVertexTheToolCannotUseIsExplainedNotDropped)
{
    // Fillet with an end chosen, Straighten with three, Delete with so many
    // that too few would be left: each says why before asking as usual.
    {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
        selectOnly(driver, {p});
        driver.start("draw.vertex.fillet", {vertexHandle(driver, p, 0)});
        EXPECT_EQ(driver.tool().prompt(),
                  "vertex 0 is an end of the polyline; a corner is where two segments meet. "
                  "Click the corner vertex of polyline " +
                      idText(p) + " to round");
    }
    {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId p = addPolyline(
            driver, {Point2(0, 0), Point2(1, 1), Point2(2, -1), Point2(3, 1), Point2(4, 0)});
        selectOnly(driver, {p});
        driver.start("draw.vertex.straighten", {vertexHandle(driver, p, 0),
                                                vertexHandle(driver, p, 2),
                                                vertexHandle(driver, p, 4)});
        EXPECT_EQ(driver.tool().prompt(),
                  "3 vertices are chosen, and this tool takes two on one polyline. Click the "
                  "first vertex of polyline " +
                      idText(p) + " to keep");
    }
    {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
        selectOnly(driver, {p});
        driver.start("draw.vertex.delete",
                     {vertexHandle(driver, p, 0), vertexHandle(driver, p, 1)});
        const std::string prompt = driver.tool().prompt();
        EXPECT_EQ(prompt.rfind("polyline " + idText(p) + ": deleting 2 vertices would leave 1", 0),
                  0u)
            << prompt;
        EXPECT_TRUE(prompt.ends_with(". Click the vertex of polyline " + idText(p) + " to delete"))
            << prompt;
    }
}

TEST(VertexToolFeedback, TheToolStaysOnThePolylineLastEditedNotTheFirst)
{
    // The first insert selects the polyline it edited (R5); an insert on
    // another then moves the selection to it: kept on the first, the prompt
    // named the first while the user worked on the second.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId first = addPolyline(driver, {Point2(0, 0), Point2(10, 0)});
    const EntityId second = addPolyline(driver, {Point2(0, 5), Point2(10, 5)});
    driver.start("draw.vertex.insert");
    ASSERT_EQ(driver.click(4, 5.1).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(driver.document().selection().ids(), std::vector<EntityId>{second});
    ASSERT_EQ(driver.click(6, 0.1).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(driver.document().selection().ids(), std::vector<EntityId>{first});
    EXPECT_EQ(driver.tool().prompt(),
              "Click on polyline " + idText(first) + " where the new vertex goes");
}

// ---- the review of 2026-09-30, second round ------------------------------------------------

namespace {

// A polyline on layer B, made with the layer, for the tests of what the view
// lets be edited.
EntityId onLayerB(ToolDriver& driver, katana::cad::CommandInterpreter& layers,
                  std::vector<Point2> points, bool closed = false)
{
    EXPECT_TRUE(layers.run("LAYER NEW B").ok());
    katana::entity::Entity entity;
    entity.geometry = Polyline2{std::move(points), closed};
    entity.layer = "B";
    return driver.add(katana::commands::createEntities({entity}));
}

// Hides layer B in the drawing or in the view alone, or locks it.
void takeAwayLayerB(ToolDriver& driver, katana::cad::CommandInterpreter& layers,
                    std::string_view how)
{
    if (how == "hidden") {
        ASSERT_TRUE(layers.run("LAYER HIDE B").ok());
    } else if (how == "hidden in the view") {
        driver.hideInView("B");
    } else {
        ASSERT_TRUE(layers.run("LAYER LOCK B").ok());
    }
}

} // namespace

TEST(VertexToolFeedback, BesideAChosenVertexAPointerPastItsSegmentsShowsWhatEnterDoesNotARefusal)
{
    // (0,0) (10,0) (10,10) with the corner, vertex 1, chosen. Beyond the
    // corner, (13,-3), both its segments are nearest at the corner itself;
    // past segment 0's far end, (-3,0.5), at vertex 0; and on the chosen
    // vertex, (10.1,0.1), is where the pointer rests after choosing it. Each
    // put the new vertex on a vertex: "too close to vertex N; zoom in", in
    // red, however far the pointer was and whatever the zoom - the first
    // thing Insert showed. A click there takes nothing, so the preview is
    // Enter's: the chosen vertex, Enter's place - the middle of segment 1,
    // (10,5), 5 from vertex 1 - and a caption said as Enter's; the click is
    // refused and the choice kept.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert", {vertexHandle(driver, p, 1)});
    const std::string ready = driver.tool().prompt();
    for (const Point2& cursor : {Point2(13, -3), Point2(-3, 0.5), Point2(10.1, 0.1)}) {
        const ToolFeedback shown = driver.preview(cursor.x, cursor.y);
        EXPECT_FALSE(shown.refused) << cursor.x << "," << cursor.y << ": " << shown.caption;
        EXPECT_EQ(shown.caption, "Enter: new vertex between 1 and 2 · 5.000 from 1")
            << cursor.x << "," << cursor.y;
        EXPECT_EQ(pointsOf(shown, FeedbackRole::Enter), std::vector<Point2>{Point2(10, 5)});
        EXPECT_EQ(pointsOf(shown, FeedbackRole::Target), std::vector<Point2>{Point2(10, 0)})
            << "the chosen vertex, and no segment a click would split";
        EXPECT_TRUE(pointsOf(shown, FeedbackRole::Added).empty());
        EXPECT_TRUE(refusedPointsOf(shown).empty());
        EXPECT_EQ(driver.click(cursor.x, cursor.y).outcome, ToolStep::Outcome::Rejected);
        EXPECT_EQ(driver.tool().prompt(), ready) << "the choice is kept";
    }
    EXPECT_EQ(driver.executed(), 0);
    // Along segment 0 near its far end the pointer IS too near vertex 0:
    // that vertex is struck, and the chosen one is still marked as chosen.
    const ToolFeedback near = driver.preview(0.2, 0.1);
    EXPECT_TRUE(near.refused);
    EXPECT_EQ(near.caption.rfind("too close to vertex 0", 0), 0u) << near.caption;
    EXPECT_EQ(refusedPointsOf(near), std::vector<Point2>{Point2(0, 0)});
    EXPECT_TRUE(hasPoint(pointsOf(near, FeedbackRole::Target), Point2(10, 0)));
    // And Enter adds at the place it showed.
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(10, 5), Point2(10, 10)}));
}

TEST(VertexToolFeedback, WithChosenVerticesTheClickPreviewStillMarksWhatEnterTakes)
{
    // The closed square with vertex 2, (10,10), chosen before Delete Vertex
    // and the pointer over vertex 0: the preview is the click's - vertex 0
    // goes - and vertex 2, which the prompt says Enter deletes, is marked as
    // Enter's with its number. It was a plain square, and the prompt's
    // "vertex 2" was nowhere on screen.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(
        driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true);
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete", {vertexHandle(driver, p, 2)});
    const ToolFeedback shown = driver.preview(0.2, 0.1);
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Removed), std::vector<Point2>{Point2(0, 0)});
    EXPECT_EQ(pointsOf(shown, FeedbackRole::Enter), std::vector<Point2>{Point2(10, 10)});
    EXPECT_EQ(labelsOf(shown, FeedbackRole::Enter), std::vector<std::string>{"2 · Enter"});
    // Over the chosen vertex the click and Enter do the same: one mark.
    const ToolFeedback over = driver.preview(10.1, 10.1);
    EXPECT_EQ(pointsOf(over, FeedbackRole::Removed), std::vector<Point2>{Point2(10, 10)});
    EXPECT_TRUE(pointsOf(over, FeedbackRole::Enter).empty());

    // Change Start: the click's "new 0" on vertex 3, and vertex 2 as Enter's.
    ToolDriver start;
    start.setPickTolerance(kAperture);
    const EntityId q = addPolyline(
        start, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true);
    selectOnly(start, {q});
    start.start("draw.vertex.start", {vertexHandle(start, q, 2)});
    const ToolFeedback other = start.preview(0.1, 10.1);
    EXPECT_EQ(pointsOf(other, FeedbackRole::Target), std::vector<Point2>{Point2(0, 10)});
    EXPECT_EQ(pointsOf(other, FeedbackRole::Enter), std::vector<Point2>{Point2(10, 10)});

    // Straighten with two chosen: both are Enter's beside the click's "keep".
    ToolDriver straight;
    straight.setPickTolerance(kAperture);
    const EntityId r = addPolyline(
        straight, {Point2(0, 0), Point2(1, 1), Point2(2, -1), Point2(3, 1), Point2(4, 0)});
    selectOnly(straight, {r});
    straight.start("draw.vertex.straighten",
                   {vertexHandle(straight, r, 0), vertexHandle(straight, r, 2)});
    const ToolFeedback keep = straight.preview(3, 1.1);
    EXPECT_EQ(pointsOf(keep, FeedbackRole::Target), std::vector<Point2>{Point2(3, 1)});
    EXPECT_EQ(labelsOf(keep, FeedbackRole::Enter),
              (std::vector<std::string>{"0 · Enter", "2 · Enter"}));
}

TEST(VertexToolFeedback, EnterAndATypedPlaceAreTakenOnASegmentShorterThanTwoApertures)
{
    // Segment 0 of (0,0) (1,0) (10,0) is 1 long: two pick apertures of 0.5,
    // 16 px at the view's 8. Its middle is 0.5 from each end, "too close" by
    // the rule for a POINTER - the view could not tell a click there from one
    // on the vertex. Enter beside vertex 0, which the prompt offers and the
    // preview marks, a typed segment number and a typed point say exactly
    // where, and all three were refused. A pointed click there still is.
    const std::vector<Point2> shortFirst{Point2(0, 0), Point2(1, 0), Point2(10, 0)};
    {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId p = addPolyline(driver, shortFirst);
        selectOnly(driver, {p});
        driver.start("draw.vertex.insert", {vertexHandle(driver, p, 0)});
        EXPECT_EQ(driver.tool().prompt().rfind(
                      "Press Enter for a vertex at the marked middle of segment 0", 0),
                  0u)
            << driver.tool().prompt();
        // Past the polyline's start the click takes nothing: Enter's place
        // is shown, and not as refused.
        const ToolFeedback shown = driver.preview(-3, 0.2);
        EXPECT_EQ(pointsOf(shown, FeedbackRole::Enter), std::vector<Point2>{Point2(0.5, 0)});
        EXPECT_TRUE(refusedPointsOf(shown).empty());
        EXPECT_EQ(shown.caption, "Enter: new vertex between 0 and 1 · 0.500 from 0");
        const ToolStep step = driver.enter();
        ASSERT_EQ(step.outcome, ToolStep::Outcome::Done) << step.message;
        EXPECT_EQ(positionsOf(driver, p),
                  (std::vector<Point2>{Point2(0, 0), Point2(0.5, 0), Point2(1, 0), Point2(10, 0)}));
    }
    {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId p = addPolyline(driver, shortFirst);
        selectOnly(driver, {p});
        driver.start("draw.vertex.insert");
        const ToolStep step = driver.type("0");
        ASSERT_EQ(step.outcome, ToolStep::Outcome::Done) << step.message;
        EXPECT_EQ(positionsOf(driver, p)[1], Point2(0.5, 0));
    }
    {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId p = addPolyline(driver, shortFirst);
        selectOnly(driver, {p});
        driver.start("draw.vertex.insert");
        // 0.3 from vertex 0: pointed at, too close; typed, exactly there.
        const ToolStep pointed = driver.click(0.3, 0.05);
        EXPECT_EQ(pointed.outcome, ToolStep::Outcome::Rejected);
        EXPECT_EQ(pointed.message.rfind("too close to vertex 0", 0), 0u) << pointed.message;
        const ToolStep typed = driver.type("0.3,0");
        ASSERT_EQ(typed.outcome, ToolStep::Outcome::Done) << typed.message;
        EXPECT_EQ(positionsOf(driver, p)[1], Point2(0.3, 0));
        // On a vertex exactly, geometry itself refuses it.
        EXPECT_EQ(driver.type("1,0").outcome, ToolStep::Outcome::Rejected);
    }
}

TEST(VertexToolFeedback, APickEditedSinceIsShownRefusedAndTheNextInputDropsIt)
{
    // Vertex 1 of (0,0) (10,0) (10,10) chosen for Insert, then another edit
    // - the Vertices panel, an agent, another view - moves vertex 2 to
    // (20,10). The preview was drawn from the copy the choice kept: a green
    // segment and a new vertex on the edge x = 10 that is no longer there,
    // and the click was then refused as stale. Now the preview says so, with
    // no marks, and the click is refused with the same words and drops the
    // choice; the next click picks the polyline as it is.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.insert", {vertexHandle(driver, p, 1)});
    ASSERT_TRUE(driver.document()
                    .execute(katana::cad::editPolyline(p, "VERTEX_MOVE",
                                                       [](const CurvePolyline2& shape) {
                                                           return katana::geometry::moveVertex(
                                                               shape, 2, Point2(20, 10));
                                                       }))
                    .ok());
    const std::string stale =
        "polyline " + idText(p) + " changed since it was picked; pick it again";
    const ToolFeedback shown = driver.preview(10.2, 5);
    EXPECT_TRUE(shown.refused);
    EXPECT_EQ(shown.caption, stale);
    EXPECT_TRUE(shown.marks.empty()) << "nothing drawn where the edge was";
    const ToolStep click = driver.click(10.2, 5);
    EXPECT_EQ(click.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(click.message, stale);
    EXPECT_EQ(driver.tool().prompt(),
              "Click on polyline " + idText(p) + " where the new vertex goes")
        << "the stale choice is dropped";
    // Segment 1 is now (10,0)-(20,10), through (15,5).
    ASSERT_EQ(driver.click(15, 5).outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(15, 5), Point2(20, 10)}));

    // Move Vertex's first pick, then its vertex moved by another edit: the
    // preview at the point step is the refusal, not a move from where it was.
    ToolDriver move;
    move.setPickTolerance(kAperture);
    const EntityId q = addPolyline(move, {Point2(0, 0), Point2(5, 0), Point2(10, 0)});
    move.start("draw.vertex.move");
    ASSERT_EQ(move.pick(q, 5, 0.1).outcome, ToolStep::Outcome::Continue);
    ASSERT_TRUE(move.document()
                    .execute(katana::cad::editPolyline(q, "VERTEX_MOVE",
                                                       [](const CurvePolyline2& shape) {
                                                           return katana::geometry::moveVertex(
                                                               shape, 1, Point2(5, 4));
                                                       }))
                    .ok());
    EXPECT_TRUE(move.preview(5, 8).refused);
    EXPECT_EQ(move.type("5,8").outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(positionsOf(move, q)[1], Point2(5, 4)) << "the other edit stands";
    EXPECT_EQ(move.tool().expects(), ToolInput::Entity) << "back to the pick";
}

TEST(VertexToolFeedback, AChosenVertexOnALayerHiddenOrLockedSinceIsNotEditedByEnter)
{
    // Vertex 2 chosen for Delete Vertex, then its layer hidden - in the
    // drawing or in this view - or locked. Enter deleted the vertex of a
    // polyline the view no longer drew, and on a locked layer the command
    // was refused after the preview had promised it.
    for (const std::string_view how : {"hidden", "hidden in the view", "locked"}) {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        katana::cad::CommandInterpreter layers{driver.document()};
        const EntityId p = onLayerB(
            driver, layers, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true);
        selectOnly(driver, {p});
        driver.start("draw.vertex.delete", {vertexHandle(driver, p, 2)});
        takeAwayLayerB(driver, layers, how);
        const ToolFeedback shown = driver.preview(5, 5);
        EXPECT_TRUE(shown.refused) << how;
        EXPECT_TRUE(shown.marks.empty()) << how;
        EXPECT_EQ(shown.caption,
                  how == "locked"
                      ? "polyline " + idText(p) + " is on the locked layer B; unlock it to edit it"
                      : "polyline " + idText(p) +
                            " is on a layer hidden since it was picked; show it to edit it")
            << how;
        const ToolStep step = driver.enter();
        EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected) << how;
        EXPECT_EQ(step.message, shown.caption) << how;
        EXPECT_EQ(driver.executed(), 0) << how;
        EXPECT_EQ(shapeOf(driver, p).vertices.size(), 4u) << how;
    }
}

TEST(VertexToolFeedback, TheSelectionToolsTakeNoSelectedPolylineOnAHiddenOrLockedLayer)
{
    // Selected, then its layer hidden or locked: Close or Open opened it,
    // and Weed weeded it, where nobody could see it (R1 said it could not
    // happen). Now the selection answers nothing, and Enter says why.
    for (const std::string_view how : {"hidden", "hidden in the view", "locked"}) {
        for (const char* tool : {"draw.vertex.close", "draw.vertex.weed"}) {
            ToolDriver driver;
            katana::cad::CommandInterpreter layers{driver.document()};
            const EntityId p = onLayerB(
                driver, layers, {Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)},
                true);
            selectOnly(driver, {p});
            takeAwayLayerB(driver, layers, how);
            driver.start(tool);
            const std::string prompt = driver.tool().prompt();
            EXPECT_EQ(prompt.find("Press Enter to apply"), std::string::npos)
                << how << ", " << tool << ": " << prompt;
            EXPECT_EQ(driver.tool().expects(), ToolInput::Selection) << how << ", " << tool;
            if (how == "locked") {
                EXPECT_EQ(
                    prompt.rfind("polyline " + idText(p) +
                                     " is on the locked layer B; unlock it to edit it. Select",
                                 0),
                    0u)
                    << tool << ": " << prompt;
            }
            const ToolStep step = driver.enter();
            EXPECT_EQ(step.outcome, ToolStep::Outcome::Rejected) << how << ", " << tool;
            EXPECT_EQ(step.message, "the selected polylines are on hidden or locked layers; "
                                    "select others, then press Enter");
            EXPECT_EQ(driver.executed(), 0) << how << ", " << tool;
            EXPECT_TRUE(shapeOf(driver, p).closed) << how << ", " << tool;
        }
    }
    // Beside one the view shows, only that one is taken.
    ToolDriver driver;
    katana::cad::CommandInterpreter layers{driver.document()};
    const EntityId hidden =
        onLayerB(driver, layers, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    const EntityId shown = addPolyline(driver, {Point2(0, 20), Point2(10, 20), Point2(10, 30)});
    selectOnly(driver, {hidden, shown});
    ASSERT_TRUE(layers.run("LAYER HIDE B").ok());
    driver.start("draw.vertex.close");
    EXPECT_EQ(driver.tool().prompt(), "Press Enter to apply to the 1 selected polyline");
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_FALSE(shapeOf(driver, hidden).closed);
    EXPECT_TRUE(shapeOf(driver, shown).closed);
}

TEST(VertexToolFeedback, EditVerticesTakesNoSelectedPolylineOnAHiddenOrLockedLayer)
{
    // Every other vertex tool passes a selected polyline through the view's
    // rule; Edit Vertices answered with its own look through the selection
    // and said a hidden or locked one was "in the Vertices panel".
    for (const std::string_view how : {"hidden", "hidden in the view", "locked"}) {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        katana::cad::CommandInterpreter layers{driver.document()};
        const EntityId p =
            onLayerB(driver, layers, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
        selectOnly(driver, {p});
        takeAwayLayerB(driver, layers, how);
        driver.start("draw.vertex.edit");
        EXPECT_EQ(driver.tool().prompt(),
                  how == "locked" ? "polyline " + idText(p) +
                                        " is on the locked layer B; unlock it to edit it. Click "
                                        "the polyline whose vertices to edit"
                                  : std::string("Click the polyline whose vertices to edit"))
            << how;
        EXPECT_EQ(driver.tool().expects(), ToolInput::Entity) << how;
    }
}

TEST(VertexToolFeedback, EditVerticesSaysEnterShowsAGripsPolylineThePanelDoesNotShowYet)
{
    // Two polylines selected: the panel shows the first (vertex_panel.cpp).
    // A grip of the second chosen before Edit Vertices is shown by Enter,
    // which selects it; the prompt said it was in the panel already.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId first = addPolyline(driver, {Point2(0, 0), Point2(10, 0)});
    const EntityId second = addPolyline(driver, {Point2(0, 5), Point2(10, 5)});
    selectOnly(driver, {first, second});
    driver.start("draw.vertex.edit", {vertexHandle(driver, second, 0)});
    EXPECT_EQ(driver.tool().prompt(), "Press Enter to show polyline " + idText(second) +
                                          " in the Vertices panel, or click another polyline");
    ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(driver.document().selection().ids(), std::vector<EntityId>{second});
    // Its own polyline's grip: in the panel already.
    selectOnly(driver, {first, second});
    driver.start("draw.vertex.edit", {vertexHandle(driver, first, 1)});
    EXPECT_EQ(driver.tool().prompt(), "Polyline " + idText(first) +
                                          " is in the Vertices panel; click another polyline, or "
                                          "press Enter");
}

TEST(VertexToolFeedback, SegmentToArcRefusesASegmentWithNoLengthAtThePick)
{
    // (0,0) (10,0) (20,10) (30,0) with vertex 1 moved onto vertex 0: segment
    // 0 has no length, so no arc passes through any point from it. It was
    // taken, and then every point was refused and only Esc got out.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p =
        addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 10), Point2(30, 0)});
    ASSERT_TRUE(driver.document()
                    .execute(katana::cad::editPolyline(p, "VERTEX_MOVE",
                                                       [](const CurvePolyline2& shape) {
                                                           return katana::geometry::moveVertex(
                                                               shape, 1, Point2(0, 0));
                                                       }))
                    .ok());
    selectOnly(driver, {p});
    driver.start("draw.vertex.arc");
    const std::string why = "segment 0 has no length; an arc needs its two ends apart";
    // (-0.3,-0.1) is as near segment 0 as segment 1, and the first is taken
    // (geometry::nearestSegment).
    const ToolFeedback shown = driver.preview(-0.3, -0.1);
    EXPECT_TRUE(shown.refused);
    EXPECT_EQ(shown.caption, why);
    const ToolStep typed = driver.type("0");
    EXPECT_EQ(typed.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(typed.message, why);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Entity) << "still the pick";
    // Segment 1 is fine.
    EXPECT_EQ(driver.type("1").outcome, ToolStep::Outcome::Continue);
}

TEST(VertexToolFeedback, ATypedPickNumberIsReadAsTheVerbsReadAnIndex)
{
    // core::parseInteger, as the drawing verbs' indexOf reads one: "1.5" and
    // "-1" are no index, and "+1" is 1.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete");
    EXPECT_EQ(driver.type("1.5").outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(driver.type("-1").outcome, ToolStep::Outcome::Rejected);
    ASSERT_EQ(driver.type("+1").outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(20, 0)}));
}

TEST(VertexToolFeedback, ADzTypedWhereTheStepTakesNoHeightIsDroppedNotRefused)
{
    // Segment to Arc's point has no height to take: "@0,2,0" there is the
    // point 2 above where segment 0 was picked - its middle (5,0), picked by
    // number - with the dz dropped, as the z of x,y,z is. Answered as a step
    // that takes heights for being a point step, it was refused for want of
    // a height to change. Through (5,2) over (0,0)-(10,0) the arc has its
    // centre (5,k) with 25 + k^2 = (2 - k)^2: k = -5.25, radius 7.25.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.arc");
    ASSERT_EQ(driver.type("0").outcome, ToolStep::Outcome::Continue);
    const ToolStep step = driver.type("@0,2,0");
    ASSERT_EQ(step.outcome, ToolStep::Outcome::Done) << step.message;
    const CurvePolyline2 after = shapeOf(driver, p);
    const auto arc = katana::geometry::arcFromBulge(after.vertices[0].position,
                                                    after.vertices[1].position,
                                                    after.vertices[0].bulge);
    ASSERT_TRUE(arc.has_value());
    EXPECT_TRUE(closeTo(arc->center, Point2(5, -5.25), 1.0e-9));
    EXPECT_NEAR(arc->radius, 7.25, 1.0e-9);
}

// ---- the review of 2026-09-30, third round -------------------------------------------------

TEST(VertexToolFeedback, ATypedPointOutOfReachSaysSoRatherThanRepeatingThePrompt)
{
    // A typed x,y at a pick is taken as a click there is (docs/drawing.md,
    // R1): within the vertex reach, here 1.5 x the aperture of 0.5 = 0.75,
    // the view's 12 px. (10.9,0) is 0.9 from vertex 1, (10.6,0) is 0.6. The
    // miss was answered with the prompt alone, though the typist cannot see
    // the reach; it says so now, and how to name the vertex instead.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(20, 0)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.delete");
    const std::string prompt = driver.tool().prompt();
    const ToolStep missed = driver.type("10.9,0");
    ASSERT_EQ(missed.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(missed.message.rfind("10.9,0 is not within 0.750 of a vertex", 0), 0u)
        << missed.message;
    EXPECT_NE(missed.message.find("12 px"), std::string::npos) << missed.message;
    EXPECT_NE(missed.message.find("vertex's number"), std::string::npos) << missed.message;
    EXPECT_TRUE(missed.message.ends_with(prompt)) << missed.message;
    EXPECT_EQ(positionsOf(driver, p).size(), 3u) << "nothing deleted";
    // A click there is refused with the prompt, as a click on nothing is.
    EXPECT_EQ(driver.click(10.9, 0).message, prompt);
    ASSERT_EQ(driver.type("10.6,0").outcome, ToolStep::Outcome::Done);
    EXPECT_EQ(positionsOf(driver, p), (std::vector<Point2>{Point2(0, 0), Point2(20, 0)}));
}

TEST(VertexToolFeedback, StraightenAndGradeSayTheSideEnterTakesAndOtherSideFlipsIt)
{
    // The hexagon (0,0) (10,0) (20,0) (20,10) (10,10) (0,10) with vertices 3
    // and 1 chosen, in that order. From 3 forward to 1 is 4, 5 and 0; the
    // other way, 1 to 3, only 2: the shorter side is walked from 1 to 3, and
    // O walks 3 to 1. The prompt named the picks in the order chosen - "from
    // vertex 3 to 1" - before O and after it, while the caption and the
    // marks said otherwise.
    for (const char* tool : {"draw.vertex.straighten", "draw.vertex.grade"}) {
        ToolDriver driver;
        driver.setPickTolerance(kAperture);
        const EntityId h = addPolyline(driver,
                                       {Point2(0, 0), Point2(10, 0), Point2(20, 0), Point2(20, 10),
                                        Point2(10, 10), Point2(0, 10)},
                                       true, {0.0, 1.0, 2.0, 3.0, 4.0, 5.0});
        selectOnly(driver, {h});
        driver.start(tool, {vertexHandle(driver, h, 3), vertexHandle(driver, h, 1)});
        const std::string verb = std::string_view(tool) == "draw.vertex.grade" ? "grade" : "straighten";
        EXPECT_EQ(driver.tool().prompt(), "Press Enter to " + verb + " polyline " + idText(h) +
                                              " from vertex 1 to 3, or click another vertex "
                                              "[Other side]")
            << tool;
        ASSERT_EQ(driver.type("O").outcome, ToolStep::Outcome::Continue);
        EXPECT_EQ(driver.tool().prompt(), "Press Enter to " + verb + " polyline " + idText(h) +
                                              " from vertex 3 to 1, or click another vertex "
                                              "[Other side]")
            << tool;
        // And Enter walks as the prompt says: Straighten takes out 4, 5 and
        // 0, leaving (10,0) (20,0) (20,10).
        if (verb == "straighten") {
            ASSERT_EQ(driver.enter().outcome, ToolStep::Outcome::Done);
            EXPECT_EQ(positionsOf(driver, h),
                      (std::vector<Point2>{Point2(10, 0), Point2(20, 0), Point2(20, 10)}));
        }
    }
}

TEST(VertexToolFeedback, EditVerticesMarksNoVertexForEnterWhereTheClickTakesTheSamePolyline)
{
    // Edit Vertices with a polyline selected is answered by the selection;
    // hovering that polyline previews the click - the whole polyline - and
    // a mark at its vertex 0, "polyline 2 · Enter", read as a chosen vertex
    // the tool does not act on. Nothing is marked for Enter now; the prompt
    // names the polyline.
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(10, 0), Point2(10, 10)});
    selectOnly(driver, {p});
    driver.start("draw.vertex.edit");
    const ToolFeedback shown = driver.preview(10.1, 5);
    EXPECT_EQ(piecesOf(shown, FeedbackRole::Target).size(), 1u);
    EXPECT_TRUE(pointsOf(shown, FeedbackRole::Enter).empty());
    EXPECT_EQ(shown.caption, "polyline " + idText(p) + " · 3 vertices, open");
}

TEST(VertexToolFeedback, SetHeightsValueStepPreviewsTheHeightAClickTakesAndEntersElsewhere)
{
    // Heights 0, none and 10 at x = 0, 5 and 10, and a point at (30,30) with
    // a height of 7.5. Vertex 1 picked, the height is asked for with the
    // default 5, halfway by length. Over the point a click takes 7.5, and the
    // preview says so; over nothing a click is refused, and the preview is
    // Enter's, the default. It was "vertex 1 · no height" wherever the cursor
    // was: neither the height a click took nor the refusal of a click that
    // was refused (R3).
    ToolDriver driver;
    driver.setPickTolerance(kAperture);
    const EntityId p = addPolyline(driver, {Point2(0, 0), Point2(5, 0), Point2(10, 0)}, false,
                                   {0.0, std::nullopt, 10.0});
    katana::entity::Entity spot;
    spot.geometry = katana::entity::PointGeometry{Point2(30, 30)};
    katana::entity::setHeights(spot.properties, {7.5});
    driver.add(katana::commands::createEntities({spot}));
    driver.start("draw.vertex.height");
    ASSERT_EQ(driver.pick(p, 5, 0.1).outcome, ToolStep::Outcome::Continue);
    const ToolFeedback over = driver.preview(30.1, 30);
    EXPECT_FALSE(over.refused);
    EXPECT_EQ(over.caption, "vertex 1 · no height → 7.500");
    const ToolFeedback nothing = driver.preview(20, 20);
    EXPECT_FALSE(nothing.refused);
    EXPECT_EQ(nothing.caption, "Enter: vertex 1 · no height → 5.000");
    const ToolStep refused = driver.click(20, 20);
    EXPECT_EQ(refused.outcome, ToolStep::Outcome::Rejected);
    EXPECT_EQ(refused.message.rfind("nothing with a height is within reach there", 0), 0u)
        << refused.message;
    ASSERT_EQ(driver.click(30.1, 30).outcome, ToolStep::Outcome::Done);
    EXPECT_DOUBLE_EQ(*shapeOf(driver, p).vertices[1].height, 7.5);
}
