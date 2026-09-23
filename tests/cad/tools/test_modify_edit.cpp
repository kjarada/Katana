// The Modify > Edit tools (src/katana_cad/tools/modify_edit*.cpp), driven as
// the plan view drives them. Expected geometry is worked out by hand in the
// comments; coordinates are chosen so that the arithmetic is exact in binary
// wherever the construction allows it (quarters and halves of lengths that are
// powers of two), and compared to a stated tolerance where a square root or a
// trigonometric function is involved.

#include <cmath>
#include <optional>
#include <string>
#include <tuple>
#include <utility>
#include <vector>

#include <gtest/gtest.h>

#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/math/numerics.hpp"
#include "tool_driver.hpp"

namespace {

namespace cmd = katana::commands;
using katana::cad::ToolFeedback;
using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Geometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kPi;

constexpr auto kContinue = ToolStep::Outcome::Continue;
constexpr auto kDone = ToolStep::Outcome::Done;
constexpr auto kRejected = ToolStep::Outcome::Rejected;

const Entity& entityOf(ToolDriver& driver, EntityId id)
{
    const Entity* entity = driver.document().model().entities.find(id);
    EXPECT_NE(entity, nullptr) << "entity " << id << " is not in the drawing";
    static const Entity missing;
    return entity != nullptr ? *entity : missing;
}

template <typename T>
T geometryOf(ToolDriver& driver, EntityId id)
{
    const Entity& entity = entityOf(driver, id);
    const T* geometry = std::get_if<T>(&entity.geometry);
    EXPECT_NE(geometry, nullptr) << "entity " << id << " is a different kind";
    return geometry != nullptr ? *geometry : T{};
}

std::size_t entityCount(ToolDriver& driver) { return driver.document().model().entities.size(); }

EntityId line(ToolDriver& driver, Point2 a, Point2 b)
{
    return driver.add(cmd::createLine(a, b, driver.document().currentAttributes()));
}

EntityId polyline(ToolDriver& driver, std::vector<Point2> vertices, bool closed = false)
{
    return driver.add(
        cmd::createPolyline(Polyline2{std::move(vertices), closed},
                            driver.document().currentAttributes()));
}

// The only entity the last command created.
EntityId created(ToolDriver& driver)
{
    const auto ids = driver.document().lastCreatedEntities();
    EXPECT_EQ(ids.size(), 1u);
    return ids.empty() ? katana::entity::kInvalidEntityId : ids.front();
}

bool containsShape(const ToolFeedback& feedback, const Geometry& shape)
{
    for (const Geometry& drawn : feedback.shapes) {
        if (drawn == shape) {
            return true;
        }
    }
    return false;
}

std::vector<std::optional<double>> heights(ToolDriver& driver, EntityId id, std::size_t count)
{
    return katana::entity::heightsOf(entityOf(driver, id).properties, count);
}

// An entity carrying a height per vertex, as the 12d import writes a 3D string.
EntityId with3DHeights(ToolDriver& driver, Geometry geometry,
                       std::vector<std::optional<double>> vertexHeights)
{
    Entity entity;
    entity.geometry = std::move(geometry);
    katana::entity::setHeights(entity.properties, vertexHeights);
    return driver.add(cmd::createEntities({entity}));
}

// ============================================================================
// Trim
// ============================================================================

// A line from (0,0) to (8,0) crossed by vertical edges at x = 2 and x = 6.
// Picking at x = 4 removes [2, 6]: the line keeps (0,0)-(2,0) and a new line
// (6,0)-(8,0) is made - parameters 1/4 and 3/4 of 8, exact in binary.
struct TrimScene {
    ToolDriver driver;
    EntityId target = 0;
    EntityId left = 0;
    EntityId right = 0;

    TrimScene()
    {
        target = line(driver, {0, 0}, {8, 0});
        left = line(driver, {2, -2}, {2, 2});
        right = line(driver, {6, -2}, {6, 2});
    }
};

TEST(ModifyEditTrim, PickedCuttingEdgesCutTheSpanUnderThePickOutOfALine)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.trim");
    EXPECT_EQ(d.tool().expects(), ToolInput::Selection);
    EXPECT_EQ(d.pick(scene.left, 2, 1).outcome, kContinue);
    EXPECT_EQ(d.pick(scene.right, 6, 1).outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Select cutting edges or [Undo] <use 2 edges>");
    EXPECT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(d.tool().prompt(), "Select object to trim or [Undo]");
    EXPECT_EQ(d.pick(scene.target, 4, 0).outcome, kContinue);
    // Nothing reaches the document until the tool finishes.
    EXPECT_EQ(geometryOf<Segment2>(d, scene.target), (Segment2{{0, 0}, {8, 0}}));
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Trimmed 1 part.");
    EXPECT_TRUE(d.finished());
    EXPECT_EQ(d.executed(), 1);
    EXPECT_EQ(geometryOf<Segment2>(d, scene.target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{6, 0}, {8, 0}}));
    EXPECT_EQ(entityCount(d), 4u);
}

TEST(ModifyEditTrim, APreselectionIsOfferedAsTheCuttingEdges)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    d.document().selection().set({scene.left, scene.right});
    d.start("modify.trim");
    EXPECT_EQ(d.tool().prompt(), "Select cutting edges <use 2 edges>");
    EXPECT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(scene.target, 4, 0).outcome, kContinue);
    EXPECT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, scene.target), (Segment2{{0, 0}, {2, 0}}));
}

TEST(ModifyEditTrim, EnterAtOnceCutsWithEveryObjectInTheDrawing)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.trim");
    EXPECT_EQ(d.tool().prompt(), "Select cutting edges or <all objects>");
    const ToolStep all = d.enter();
    EXPECT_EQ(all.outcome, kContinue);
    EXPECT_EQ(all.message, "Every object is an edge.");
    EXPECT_EQ(d.pick(scene.target, 4, 0).outcome, kContinue);
    EXPECT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, scene.target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{6, 0}, {8, 0}}));
}

TEST(ModifyEditTrim, EveryPickOfOneTrimIsOneUndoOfTheDocument)
{
    // Edges at x = 2, 4 and 6 across (0,0)-(8,0). The pick at x = 3 removes
    // [2, 4], leaving (0,0)-(2,0) and a new piece (4,0)-(8,0). The pick at
    // x = 5 lands on that NEW piece, which has no id yet: its only crossing is
    // x = 6 (x = 4 is its own end), so [4, 6] goes and (6,0)-(8,0) is left.
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    line(d, {2, -2}, {2, 2});
    line(d, {4, -2}, {4, 2});
    line(d, {6, -2}, {6, 2});
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 3, 0).outcome, kContinue);
    EXPECT_EQ(d.pick(target, 5, 0).outcome, kContinue);
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Trimmed 2 parts.");
    EXPECT_EQ(d.executed(), 1);
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{6, 0}, {8, 0}}));
    EXPECT_EQ(entityCount(d), 5u);

    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {8, 0}}));
    EXPECT_EQ(entityCount(d), 4u);
}

TEST(ModifyEditTrim, UndoInsideTheToolTakesBackTheLastPickOnly)
{
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    line(d, {2, -2}, {2, 2});
    line(d, {4, -2}, {4, 2});
    line(d, {6, -2}, {6, 2});
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 3, 0).outcome, kContinue);
    EXPECT_EQ(d.pick(target, 7, 0).outcome, kContinue);
    const ToolStep undone = d.type("u");
    EXPECT_EQ(undone.outcome, kContinue);
    EXPECT_EQ(undone.message, "Undid the last trim.");
    ASSERT_EQ(d.enter().outcome, kDone);
    // Only the x = 3 pick is left: (0,0)-(2,0) and (4,0)-(8,0).
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{4, 0}, {8, 0}}));
}

TEST(ModifyEditTrim, UndoBeforeAnyTrimGoesBackToChoosingTheEdges)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.trim");
    EXPECT_EQ(d.pick(scene.left, 2, 1).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.undo().outcome, kContinue);
    EXPECT_EQ(d.tool().expects(), ToolInput::Selection);
    // The edge picked before is still picked.
    EXPECT_EQ(d.tool().prompt(), "Select cutting edges or [Undo] <use 1 edge>");
    EXPECT_EQ(d.undo().outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Select cutting edges or <all objects>");
    EXPECT_EQ(d.undo().outcome, kRejected);
}

TEST(ModifyEditTrim, APolylineLosesTheSpanBetweenTheCrossingsEitherSideOfThePick)
{
    // (0,0)-(8,0)-(8,8), 16 long. A vertical edge crosses at station 4, a
    // horizontal one at station 8 + 4 = 12. The pick at (8,2), station 10,
    // takes out [4, 12] round the corner: (0,0)-(4,0) is kept as the polyline
    // and (8,4)-(8,8) becomes a new one.
    ToolDriver d;
    const EntityId target = polyline(d, {{0, 0}, {8, 0}, {8, 8}});
    line(d, {4, -2}, {4, 2});
    line(d, {6, 4}, {10, 4});
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 8, 2).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, target), (Polyline2{{{0, 0}, {4, 0}}, false}));
    EXPECT_EQ(geometryOf<Polyline2>(d, created(d)), (Polyline2{{{8, 4}, {8, 8}}, false}));
}

TEST(ModifyEditTrim, AClosedPolylineBecomesTheOpenPolylineAwayFromThePick)
{
    // The square (0,0)-(8,0)-(8,8)-(0,8), 32 round. The vertical edge x = 4
    // crosses the bottom at station 4 and the top at station 16 + 4 = 20. The
    // pick at (8,4), station 12, removes [4, 20] - the right half - and what
    // is left runs from station 20 on through the first vertex to station 4:
    // (4,8)-(0,8)-(0,0)-(4,0), open.
    ToolDriver d;
    const EntityId target = polyline(d, {{0, 0}, {8, 0}, {8, 8}, {0, 8}}, true);
    line(d, {4, -2}, {4, 10});
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 8, 4).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, target),
              (Polyline2{{{4, 8}, {0, 8}, {0, 0}, {4, 0}}, false}));
    EXPECT_EQ(entityCount(d), 2u);
}

TEST(ModifyEditTrim, APickOnTheSpanThroughTheFirstVertexOfAClosedPolylineRemovesThatSpan)
{
    // The same square and edge, picked at (0,4), station 28: the span from
    // station 20 through the first vertex to station 4 goes, and the right
    // half (4,0)-(8,0)-(8,8)-(4,8) is kept.
    ToolDriver d;
    const EntityId target = polyline(d, {{0, 0}, {8, 0}, {8, 8}, {0, 8}}, true);
    line(d, {4, -2}, {4, 10});
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 0, 4).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, target),
              (Polyline2{{{4, 0}, {8, 0}, {8, 8}, {4, 8}}, false}));
}

TEST(ModifyEditTrim, ACircleCutTwiceKeepsTheArcAwayFromThePick)
{
    // Radius 4 about the origin, cut by the y axis at 90 and 270 degrees. A
    // pick at (4,0) takes the right half; the left half is kept as an arc
    // from 90 degrees sweeping 180 counter-clockwise. The crossings come from
    // a square root, so the angles are compared to 1e-12.
    ToolDriver d;
    const EntityId target =
        d.add(cmd::createCircle({0, 0}, 4, d.document().currentAttributes()));
    line(d, {0, -8}, {0, 8});
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    const auto arc = geometryOf<Arc2>(d, target);
    EXPECT_EQ(arc.center, Point2(0, 0));
    EXPECT_EQ(arc.radius, 4.0);
    EXPECT_NEAR(arc.startAngle, kPi / 2, 1e-12);
    EXPECT_NEAR(arc.sweep, kPi, 1e-12);
}

TEST(ModifyEditTrim, TheHeightsOfA3DLineAreInterpolatedAtTheNewEnds)
{
    // (0,0) at height 10 to (8,0) at height 18: z = 10 + x. Trimmed between
    // x = 2 and x = 6, the kept line ends at height 12 and the new one starts
    // at 16; their far ends keep 10 and 18.
    ToolDriver d;
    const EntityId target = with3DHeights(d, Segment2{{0, 0}, {8, 0}}, {10.0, 18.0});
    line(d, {2, -2}, {2, 2});
    line(d, {6, -2}, {6, 2});
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(heights(d, target, 2), (std::vector<std::optional<double>>{10.0, 12.0}));
    EXPECT_EQ(heights(d, created(d), 2), (std::vector<std::optional<double>>{16.0, 18.0}));
}

TEST(ModifyEditTrim, ALineNoEdgeCrossesIsRefusedAndTheToolStaysPut)
{
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    const EntityId edge = line(d, {0, 4}, {8, 4}); // parallel: never crosses
    d.start("modify.trim");
    EXPECT_EQ(d.pick(edge, 4, 4).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolStep refused = d.pick(target, 4, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "No cutting edge crosses that line.");
    EXPECT_EQ(d.tool().expects(), ToolInput::Entity);
    EXPECT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(d.executed(), 0);
}

TEST(ModifyEditTrim, ACircleCrossedOnceIsRefused)
{
    ToolDriver d;
    const EntityId target = d.add(cmd::createCircle({0, 0}, 4, d.document().currentAttributes()));
    line(d, {0, 0}, {0, 8}); // from the centre out: one crossing, at (0,4)
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolStep refused = d.pick(target, 4, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "A circle needs two crossings with the cutting edges to be trimmed.");
}

TEST(ModifyEditTrim, TextIsRefusedWithWhatTheToolTakes)
{
    ToolDriver d;
    katana::entity::TextGeometry text;
    text.position = {0, 0};
    text.text = "KERB";
    const EntityId label = d.add(cmd::createText(text, d.document().currentAttributes()));
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolStep refused = d.pick(label, 0, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "A text cannot be trimmed; pick a line, arc, circle or polyline.");
}

TEST(ModifyEditTrim, APickOnAPartAlreadyTrimmedAwayIsRefused)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    ASSERT_EQ(d.pick(scene.target, 4, 0).outcome, kContinue);
    // (5,0) was in the span just removed: the document still draws it, but
    // there is nothing of the line left there.
    const ToolStep refused = d.pick(scene.target, 5, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "That part of the line has already been trimmed away.");
}

TEST(ModifyEditTrim, AnObjectOnALockedLayerIsRefusedByName)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    // Made unlocked and locked afterwards: nothing can be moved onto a layer
    // that is already locked.
    katana::entity::Layer kerbs;
    kerbs.name = "kerbs";
    ASSERT_TRUE(d.document().execute(cmd::createLayer(kerbs)).ok());
    ASSERT_TRUE(d.document().execute(cmd::setEntityLayer({scene.target}, "kerbs")).ok());
    kerbs.locked = true;
    ASSERT_TRUE(d.document().execute(cmd::updateLayer(kerbs)).ok());
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolStep refused = d.pick(scene.target, 4, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message,
              "That line is on the locked layer 'kerbs'; unlock the layer to edit it.");
}

TEST(ModifyEditTrim, EnterWithNothingTrimmedFinishesWithoutACommand)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolStep finished = d.enter();
    EXPECT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.command, nullptr);
    EXPECT_EQ(d.executed(), 0);
}

TEST(ModifyEditTrim, ThePreviewShowsThePartTheCursorWouldRemove)
{
    TrimScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.trim");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolFeedback feedback = d.tool().preview({4, 0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], Geometry(Segment2{{2, 0}, {6, 0}}));
    // Once picked it stays marked while the cursor moves on to nothing.
    ASSERT_EQ(d.pick(scene.target, 4, 0).outcome, kContinue);
    EXPECT_TRUE(containsShape(d.tool().preview({100, 100}), Segment2{{2, 0}, {6, 0}}));
}

// ============================================================================
// Extend
// ============================================================================

TEST(ModifyEditExtend, ALineIsLengthenedFromThePickedEndToTheBoundary)
{
    // (0,0)-(4,0) picked near its east end reaches the boundary x = 8.
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {4, 0});
    const EntityId boundary = line(d, {8, -2}, {8, 2});
    d.start("modify.extend");
    EXPECT_EQ(d.tool().prompt(), "Select boundary edges or <all objects>");
    EXPECT_EQ(d.pick(boundary, 8, 0).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Select object to extend or [Undo]");
    EXPECT_EQ(d.pick(target, 3, 0).outcome, kContinue);
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Extended 1 end.");
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {8, 0}}));
}

TEST(ModifyEditExtend, BothEndsOfALineReachTheirBoundariesInOneCommand)
{
    // Every object is a boundary: x = -4 to the west, x = 8 to the east. The
    // pick at (1,0) is nearer the start, (3,0) nearer the end.
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {4, 0});
    line(d, {-4, -2}, {-4, 2});
    line(d, {8, -2}, {8, 2});
    d.start("modify.extend");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 1, 0).outcome, kContinue);
    EXPECT_EQ(d.pick(target, 3, 0).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(d.executed(), 1);
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{-4, 0}, {8, 0}}));
    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {4, 0}}));
}

TEST(ModifyEditExtend, AnArcIsLengthenedAlongItsCircle)
{
    // The quarter arc of radius 4 from 0 to 90 degrees, picked near its end
    // (0,4), goes on round its circle to the boundary (-8,0)-(0,0), which it
    // meets at (-4,0), 180 degrees: the sweep becomes a half turn.
    ToolDriver d;
    const EntityId target =
        d.add(cmd::createArc(Arc2{{0, 0}, 4, 0, kPi / 2}, d.document().currentAttributes()));
    line(d, {-8, 0}, {0, 0});
    d.start("modify.extend");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 0.5, 3.9).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    const auto arc = geometryOf<Arc2>(d, target);
    EXPECT_EQ(arc.startAngle, 0.0);
    EXPECT_NEAR(arc.sweep, kPi, 1e-12);
}

TEST(ModifyEditExtend, AnOpenPolylineExtendsItsEndSegment)
{
    // (0,0)-(4,0)-(4,4), 8 long; the pick at (4,3.5) is at station 7.5, in
    // the half nearer the last vertex, so (4,4) moves up to y = 8.
    ToolDriver d;
    const EntityId target = polyline(d, {{0, 0}, {4, 0}, {4, 4}});
    line(d, {0, 8}, {8, 8});
    d.start("modify.extend");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 4, 3.5).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, target), (Polyline2{{{0, 0}, {4, 0}, {4, 8}}, false}));
}

TEST(ModifyEditExtend, AnExtendedEndHasNoHeightBecauseNoneWasSurveyedThere)
{
    // A 3D line (0,0) at 10 to (4,0) at 14 extended to x = 8: the start keeps
    // 10, and the new end is absent rather than a guessed 18 - absent is not
    // zero, and an extrapolated grade is not a survey.
    ToolDriver d;
    const EntityId target = with3DHeights(d, Segment2{{0, 0}, {4, 0}}, {10.0, 14.0});
    line(d, {8, -2}, {8, 2});
    d.start("modify.extend");
    ASSERT_EQ(d.enter().outcome, kContinue);
    EXPECT_EQ(d.pick(target, 3, 0).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(heights(d, target, 2), (std::vector<std::optional<double>>{10.0, std::nullopt}));
}

TEST(ModifyEditExtend, ACircleAndAClosedPolylineAreRefusedAsHavingNoEnd)
{
    ToolDriver d;
    const EntityId ring = d.add(cmd::createCircle({0, 0}, 4, d.document().currentAttributes()));
    const EntityId square = polyline(d, {{10, 0}, {12, 0}, {12, 2}}, true);
    d.start("modify.extend");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolStep circle = d.pick(ring, 4, 0);
    EXPECT_EQ(circle.outcome, kRejected);
    EXPECT_EQ(circle.message, "A circle has no end to extend.");
    const ToolStep closed = d.pick(square, 11, 0);
    EXPECT_EQ(closed.outcome, kRejected);
    EXPECT_EQ(closed.message, "A closed polyline has no end to extend.");
}

TEST(ModifyEditExtend, AnEndWithNoBoundaryAheadIsRefused)
{
    // The boundary is behind the start; the pick asks for the east end.
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {4, 0});
    line(d, {-4, -2}, {-4, 2});
    d.start("modify.extend");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolStep refused = d.pick(target, 3, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "No boundary lies ahead of that end of the line.");
}

TEST(ModifyEditExtend, ThePreviewShowsTheNewLength)
{
    ToolDriver d;
    line(d, {0, 0}, {4, 0});
    line(d, {8, -2}, {8, 2});
    d.start("modify.extend");
    ASSERT_EQ(d.enter().outcome, kContinue);
    const ToolFeedback feedback = d.tool().preview({3, 0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], Geometry(Segment2{{4, 0}, {8, 0}}));
}

// ============================================================================
// Offset
// ============================================================================

TEST(ModifyEditOffset, ATypedDistanceOffsetsALineToTheClickedSide)
{
    // (0,0)-(8,0) moved 2 towards (4,5), its left: (0,2)-(8,2).
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    EXPECT_EQ(d.tool().expects(), ToolInput::Point);
    EXPECT_EQ(d.type("2").outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Select object to offset or [Exit/Undo]");
    EXPECT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify point on side to offset or [Multiple/Undo]");
    EXPECT_EQ(d.click(4, 5).outcome, kContinue);
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Made 1 offset.");
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{0, 2}, {8, 2}}));
    EXPECT_EQ(geometryOf<Segment2>(d, source), (Segment2{{0, 0}, {8, 0}}));
}

TEST(ModifyEditOffset, EveryOffsetOfOneSessionIsOneUndoOfTheDocument)
{
    // Above and below the same line: (0,2)-(8,2) and (0,-2)-(8,-2).
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    ASSERT_EQ(d.type("2").outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(4, 5).outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(4, -5).outcome, kContinue);
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Made 2 offsets.");
    EXPECT_EQ(d.executed(), 1);
    const auto made = d.document().lastCreatedEntities();
    ASSERT_EQ(made.size(), 2u);
    EXPECT_EQ(geometryOf<Segment2>(d, made[0]), (Segment2{{0, 2}, {8, 2}}));
    EXPECT_EQ(geometryOf<Segment2>(d, made[1]), (Segment2{{0, -2}, {8, -2}}));
    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(entityCount(d), 1u);
}

TEST(ModifyEditOffset, ThroughPutsTheCopyExactlyThroughTheClickedPoint)
{
    // (3,1.5) is 1.5 from the line y = 0: the copy is (0,1.5)-(8,1.5).
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    EXPECT_EQ(d.type("t").outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify through point or [Multiple/Undo]");
    ASSERT_EQ(d.click(3, 1.5).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{0, 1.5}, {8, 1.5}}));
}

TEST(ModifyEditOffset, TwoClickedPointsMeasureTheDistance)
{
    // (0,0) to (3,4) is 5 (a 3-4-5 triangle); the second point typed
    // relative to the first. The copy of (0,0)-(8,0) is (0,5)-(8,5).
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    ASSERT_EQ(d.click(0, 0).outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify second point");
    ASSERT_EQ(d.type("@3,4").outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(4, 9).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{0, 5}, {8, 5}}));
}

TEST(ModifyEditOffset, ACircleGrowsOrShrinksByTheSideClicked)
{
    // Radius 4, distance 1: a click inside makes radius 3, outside radius 5.
    ToolDriver d;
    const EntityId ring = d.add(cmd::createCircle({0, 0}, 4, d.document().currentAttributes()));
    d.start("modify.offset");
    ASSERT_EQ(d.type("1").outcome, kContinue);
    ASSERT_EQ(d.pick(ring, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(1, 0).outcome, kContinue);
    ASSERT_EQ(d.pick(ring, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(8, 0).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    const auto made = d.document().lastCreatedEntities();
    ASSERT_EQ(made.size(), 2u);
    EXPECT_EQ(geometryOf<Circle2>(d, made[0]), (Circle2{{0, 0}, 3}));
    EXPECT_EQ(geometryOf<Circle2>(d, made[1]), (Circle2{{0, 0}, 5}));
}

TEST(ModifyEditOffset, ARectangleOffsetsOutwardWithMitredCorners)
{
    // The counter-clockwise square (0,0)-(4,0)-(4,4)-(0,4). (2,-3) is outside
    // (right of the bottom edge's direction), so every side moves out by 1 and
    // the corners meet at (-1,-1), (5,-1), (5,5), (-1,5).
    ToolDriver d;
    const EntityId square = polyline(d, {{0, 0}, {4, 0}, {4, 4}, {0, 4}}, true);
    d.start("modify.offset");
    ASSERT_EQ(d.type("1").outcome, kContinue);
    ASSERT_EQ(d.pick(square, 2, 0).outcome, kContinue);
    ASSERT_EQ(d.click(2, -3).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, created(d)),
              (Polyline2{{{-1, -1}, {5, -1}, {5, 5}, {-1, 5}}, true}));
}

TEST(ModifyEditOffset, MultipleKeepsOffsettingTheCopyJustMade)
{
    // Distance 2 from (0,0)-(8,0), three clicks above: y = 2, 4 and 6. Undo
    // takes back y = 6, and Enter makes the two left.
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    ASSERT_EQ(d.type("2").outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    EXPECT_EQ(d.type("M").outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify point on side for the next offset or [Undo]");
    ASSERT_EQ(d.click(4, 1).outcome, kContinue);
    ASSERT_EQ(d.click(4, 3).outcome, kContinue);
    ASSERT_EQ(d.click(4, 5).outcome, kContinue);
    EXPECT_EQ(d.undo().outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    const auto made = d.document().lastCreatedEntities();
    ASSERT_EQ(made.size(), 2u);
    EXPECT_EQ(geometryOf<Segment2>(d, made[0]), (Segment2{{0, 2}, {8, 2}}));
    EXPECT_EQ(geometryOf<Segment2>(d, made[1]), (Segment2{{0, 4}, {8, 4}}));
}

TEST(ModifyEditOffset, TheDistanceUsedLastIsOfferedNextTimeAndEnterTakesIt)
{
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    ASSERT_EQ(d.type("2.5").outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone); // nothing offset; the distance is kept
    d.start("modify.offset");
    EXPECT_EQ(d.tool().prompt(), "Specify offset distance or [Through] <2.5>");
    ASSERT_EQ(d.enter().outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(4, 5).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{0, 2.5}, {8, 2.5}}));
}

TEST(ModifyEditOffset, EachVertexOfA3DStringKeepsItsHeight)
{
    ToolDriver d;
    const EntityId source = with3DHeights(d, Segment2{{0, 0}, {8, 0}}, {10.0, 18.0});
    d.start("modify.offset");
    ASSERT_EQ(d.type("2").outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(4, 5).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(heights(d, created(d), 2), (std::vector<std::optional<double>>{10.0, 18.0}));
}

TEST(ModifyEditOffset, AZeroNegativeOrUnreadableDistanceIsRefused)
{
    ToolDriver d;
    d.start("modify.offset");
    for (const char* typed : {"0", "-1"}) {
        const ToolStep refused = d.type(typed);
        EXPECT_EQ(refused.outcome, kRejected) << typed;
        EXPECT_EQ(refused.message, "The offset distance must be greater than zero.") << typed;
    }
    EXPECT_EQ(d.type("two").outcome, kRejected);
    ASSERT_EQ(d.click(1, 1).outcome, kContinue);
    const ToolStep same = d.click(1, 1);
    EXPECT_EQ(same.outcome, kRejected);
    EXPECT_EQ(d.tool().prompt(), "Specify second point");
}

TEST(ModifyEditOffset, AThroughPointOnTheObjectIsRefused)
{
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    ASSERT_EQ(d.type("T").outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    const ToolStep refused = d.click(6, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "The through point lies on the line; pick a point off it.");
}

TEST(ModifyEditOffset, AnInwardOffsetNoSmallerThanTheRadiusIsRefused)
{
    ToolDriver d;
    const EntityId ring = d.add(cmd::createCircle({0, 0}, 4, d.document().currentAttributes()));
    d.start("modify.offset");
    ASSERT_EQ(d.type("5").outcome, kContinue);
    ASSERT_EQ(d.pick(ring, 4, 0).outcome, kContinue);
    const ToolStep refused = d.click(1, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message,
              "The distance is not smaller than the radius, so there is no circle on that side.");
}

TEST(ModifyEditOffset, TextIsRefusedAsASource)
{
    ToolDriver d;
    katana::entity::TextGeometry text;
    text.text = "KERB";
    const EntityId label = d.add(cmd::createText(text, d.document().currentAttributes()));
    d.start("modify.offset");
    ASSERT_EQ(d.type("1").outcome, kContinue);
    const ToolStep refused = d.pick(label, 0, 0);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "A text cannot be offset; pick a line, arc, circle or polyline.");
}

TEST(ModifyEditOffset, ThePreviewShowsTheCopyForTheCursorSide)
{
    ToolDriver d;
    const EntityId source = line(d, {0, 0}, {8, 0});
    d.start("modify.offset");
    ASSERT_EQ(d.type("2").outcome, kContinue);
    ASSERT_EQ(d.pick(source, 4, 0).outcome, kContinue);
    EXPECT_TRUE(containsShape(d.tool().preview({4, -3}), Segment2{{0, -2}, {8, -2}}));
}

// ============================================================================
// Fillet and Chamfer
// ============================================================================

// Two lines meeting at the corner (8,0): along the x axis and up x = 8.
struct CornerScene {
    ToolDriver driver;
    EntityId along = 0;
    EntityId up = 0;

    CornerScene()
    {
        along = line(driver, {0, 0}, {8, 0});
        up = line(driver, {8, 0}, {8, 8});
    }
};

TEST(ModifyEditFillet, ARadiusRoundsARightAngledCornerWithATangentArc)
{
    // Radius 2 at a right angle: the tangent points are 2 back along each line,
    // (6,0) and (8,2), and the arc's centre is (6,2), sweeping 90 degrees
    // counter-clockwise from -90. tan(45 degrees) is not exactly 1 in binary,
    // so positions are compared to 1e-12.
    CornerScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.fillet");
    EXPECT_EQ(d.type("R").outcome, kContinue);
    EXPECT_EQ(d.tool().expects(), ToolInput::Value);
    EXPECT_EQ(d.type("2").outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Select first line or [Radius/Multiple] (radius 2)");
    EXPECT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Select second line or [Undo] (radius 2)");
    const ToolStep finished = d.pick(scene.up, 8, 4);
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Filleted 1 corner with radius 2.");
    EXPECT_TRUE(d.finished());

    const auto along = geometryOf<Segment2>(d, scene.along);
    EXPECT_EQ(along.start, Point2(0, 0));
    EXPECT_NEAR(along.end.x, 6.0, 1e-12);
    EXPECT_NEAR(along.end.y, 0.0, 1e-12);
    const auto up = geometryOf<Segment2>(d, scene.up);
    EXPECT_NEAR(up.start.x, 8.0, 1e-12);
    EXPECT_NEAR(up.start.y, 2.0, 1e-12);
    EXPECT_EQ(up.end, Point2(8, 8));
    const auto arc = geometryOf<Arc2>(d, created(d));
    EXPECT_NEAR(arc.center.x, 6.0, 1e-12);
    EXPECT_NEAR(arc.center.y, 2.0, 1e-12);
    EXPECT_NEAR(arc.radius, 2.0, 1e-12);
    EXPECT_NEAR(arc.startAngle, -kPi / 2, 1e-12);
    EXPECT_NEAR(arc.sweep, kPi / 2, 1e-12);

    // One undo of the document puts both lines back and takes the arc away.
    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(geometryOf<Segment2>(d, scene.along), (Segment2{{0, 0}, {8, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, scene.up), (Segment2{{8, 0}, {8, 8}}));
    EXPECT_EQ(entityCount(d), 2u);
}

TEST(ModifyEditFillet, RadiusZeroExtendsBothLinesToASharpCorner)
{
    // (0,0)-(6,0) and (8,2)-(8,8) stop short of the corner (8,0); radius 0
    // runs both to it and makes no arc.
    ToolDriver d;
    const EntityId along = line(d, {0, 0}, {6, 0});
    const EntityId up = line(d, {8, 2}, {8, 8});
    d.start("modify.fillet");
    ASSERT_EQ(d.type("0").outcome, kContinue); // a number alone is the radius
    ASSERT_EQ(d.pick(along, 3, 0).outcome, kContinue);
    ASSERT_EQ(d.pick(up, 8, 5).outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, along), (Segment2{{0, 0}, {8, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, up), (Segment2{{8, 0}, {8, 8}}));
    EXPECT_EQ(entityCount(d), 2u);
}

TEST(ModifyEditFillet, CrossingLinesKeepTheSidesThatWerePicked)
{
    // (0,4)-(8,4) and (4,0)-(4,8) cross at (4,4). Picked east of the crossing
    // and south of it, radius 0 keeps (4,4)-(8,4) and (4,0)-(4,4). Taking the
    // end farther from the corner cannot decide this - both halves are 4 long
    // - so this is the pick at work (the geometry alone keeps the west and
    // north halves).
    ToolDriver d;
    const EntityId across = line(d, {0, 4}, {8, 4});
    const EntityId down = line(d, {4, 0}, {4, 8});
    d.start("modify.fillet");
    ASSERT_EQ(d.type("0").outcome, kContinue);
    ASSERT_EQ(d.pick(across, 7, 4).outcome, kContinue);
    ASSERT_EQ(d.pick(down, 4, 1).outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, across), (Segment2{{4, 4}, {8, 4}}));
    EXPECT_EQ(geometryOf<Segment2>(d, down), (Segment2{{4, 0}, {4, 4}}));
}

TEST(ModifyEditFillet, ParallelLinesAreRefusedAsHavingNoCorner)
{
    ToolDriver d;
    const EntityId a = line(d, {0, 0}, {8, 0});
    const EntityId b = line(d, {0, 2}, {8, 2});
    const EntityId c = line(d, {10, 0}, {12, 0}); // collinear with a
    d.start("modify.fillet");
    ASSERT_EQ(d.pick(a, 4, 0).outcome, kContinue);
    for (const auto& [id, x, y] : {std::tuple{b, 4.0, 2.0}, std::tuple{c, 11.0, 0.0}}) {
        const ToolStep refused = d.pick(id, x, y);
        EXPECT_EQ(refused.outcome, kRejected);
        EXPECT_EQ(refused.message, "The lines are parallel, so there is no corner to fillet.");
    }
    EXPECT_EQ(d.tool().prompt().rfind("Select second line", 0), 0u);
}

TEST(ModifyEditFillet, ARadiusTooLargeForTheLinesIsRefusedByValue)
{
    // Radius 10 needs 10 of each line back from the corner; they are 8 long.
    CornerScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.fillet");
    ASSERT_EQ(d.type("10").outcome, kContinue);
    ASSERT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    const ToolStep refused = d.pick(scene.up, 8, 4);
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "A radius of 10 is too large for these lines.");
    EXPECT_EQ(d.executed(), 0);
}

TEST(ModifyEditFillet, TheSameLineTwiceAnArcAndAPolylineAreRefused)
{
    CornerScene scene;
    ToolDriver& d = scene.driver;
    const EntityId bend =
        d.add(cmd::createArc(Arc2{{20, 0}, 2, 0, kPi / 2}, d.document().currentAttributes()));
    const EntityId string = polyline(d, {{30, 0}, {34, 0}, {34, 4}});
    d.start("modify.fillet");
    const ToolStep arc = d.pick(bend, 22, 0);
    EXPECT_EQ(arc.outcome, kRejected);
    EXPECT_EQ(arc.message, "Only lines can be filleted, and that is an arc.");
    const ToolStep poly = d.pick(string, 32, 0);
    EXPECT_EQ(poly.outcome, kRejected);
    EXPECT_EQ(poly.message, "Only lines can be filleted; explode the polyline into lines first.");
    ASSERT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    const ToolStep twice = d.pick(scene.along, 5, 0);
    EXPECT_EQ(twice.outcome, kRejected);
    EXPECT_EQ(twice.message, "Pick a different line for the second.");
}

TEST(ModifyEditFillet, ANegativeRadiusIsRefusedAndTheRadiusIsStillAsked)
{
    ToolDriver d;
    d.start("modify.fillet");
    ASSERT_EQ(d.type("r").outcome, kContinue);
    const ToolStep refused = d.type("-1");
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "The fillet radius cannot be negative.");
    EXPECT_EQ(d.tool().expects(), ToolInput::Value);
}

TEST(ModifyEditFillet, UndoAtTheSecondLineGoesBackToTheFirst)
{
    CornerScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.fillet");
    ASSERT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    EXPECT_EQ(d.undo().outcome, kContinue);
    EXPECT_EQ(d.tool().prompt().rfind("Select first line", 0), 0u);
}

TEST(ModifyEditFillet, MultipleFilletsTwoCornersOfOneLineAsOneCommand)
{
    // The right side (8,0)-(8,8) of an open box is filleted at both ends with
    // radius 2: its bottom end moves up to (8,2), then - working on that
    // shortened line - its top end down to (8,6). Two arcs; one command.
    ToolDriver d;
    const EntityId bottom = line(d, {0, 0}, {8, 0});
    const EntityId right = line(d, {8, 0}, {8, 8});
    const EntityId top = line(d, {8, 8}, {0, 8});
    d.start("modify.fillet");
    ASSERT_EQ(d.type("2").outcome, kContinue);
    ASSERT_EQ(d.type("M").outcome, kContinue);
    ASSERT_EQ(d.pick(bottom, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.pick(right, 8, 4).outcome, kContinue);
    ASSERT_EQ(d.pick(right, 8, 4).outcome, kContinue);
    ASSERT_EQ(d.pick(top, 4, 8).outcome, kContinue);
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Filleted 2 corners with radius 2.");
    EXPECT_EQ(d.executed(), 1);
    const auto side = geometryOf<Segment2>(d, right);
    EXPECT_NEAR(side.start.y, 2.0, 1e-12);
    EXPECT_NEAR(side.end.y, 6.0, 1e-12);
    EXPECT_EQ(d.document().lastCreatedEntities().size(), 2u);
}

TEST(ModifyEditFillet, TheRadiusUsedLastIsTheRadiusNextTime)
{
    ToolDriver d;
    d.start("modify.fillet");
    ASSERT_EQ(d.type("3").outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    d.start("modify.fillet");
    EXPECT_EQ(d.tool().prompt(), "Select first line or [Radius/Multiple] (radius 3)");
}

TEST(ModifyEditFillet, TheArcTakesItsEndHeightsFromTheLinesItJoins)
{
    // (0,0) at 10 to (8,0) at 18, then (8,0) at 18 to (8,8) at 26: z rises 1
    // per unit along the path. The radius 2 arc starts 6 along the first line
    // (height 16) and ends 2 up the second (height 20).
    ToolDriver d;
    const EntityId along = with3DHeights(d, Segment2{{0, 0}, {8, 0}}, {10.0, 18.0});
    const EntityId up = with3DHeights(d, Segment2{{8, 0}, {8, 8}}, {18.0, 26.0});
    d.start("modify.fillet");
    ASSERT_EQ(d.type("2").outcome, kContinue);
    ASSERT_EQ(d.pick(along, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.pick(up, 8, 4).outcome, kDone);
    const auto ends = heights(d, created(d), 2);
    ASSERT_TRUE(ends[0].has_value());
    ASSERT_TRUE(ends[1].has_value());
    EXPECT_NEAR(*ends[0], 16.0, 1e-9);
    EXPECT_NEAR(*ends[1], 20.0, 1e-9);
}

TEST(ModifyEditFillet, ThePreviewShowsTheArcForTheLineUnderTheCursor)
{
    CornerScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.fillet");
    ASSERT_EQ(d.type("2").outcome, kContinue);
    ASSERT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    const ToolFeedback feedback = d.tool().preview({8, 4});
    ASSERT_EQ(feedback.shapes.size(), 3u);
    EXPECT_TRUE(std::holds_alternative<Arc2>(feedback.shapes[2]));
}

TEST(ModifyEditChamfer, TwoDistancesCutTheCornerWithABevel)
{
    // Distance 2 along the first line and 3 along the second, back from the
    // corner (8,0): the cut points are (6,0) and (8,3), exactly.
    CornerScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.chamfer");
    EXPECT_EQ(d.type("D").outcome, kContinue);
    ASSERT_EQ(d.type("2").outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify second chamfer distance <2>");
    ASSERT_EQ(d.type("3").outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Select first line or [Distance/Multiple] (distances 2, 3)");
    ASSERT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    const ToolStep finished = d.pick(scene.up, 8, 4);
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Chamfered 1 corner at 2 and 3.");
    EXPECT_EQ(geometryOf<Segment2>(d, scene.along), (Segment2{{0, 0}, {6, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, scene.up), (Segment2{{8, 3}, {8, 8}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{6, 0}, {8, 3}}));
    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(entityCount(d), 2u);
}

TEST(ModifyEditChamfer, EnterMakesTheSecondDistanceTheFirst)
{
    CornerScene scene;
    ToolDriver& d = scene.driver;
    d.start("modify.chamfer");
    ASSERT_EQ(d.type("1").outcome, kContinue); // a number alone is the first distance
    EXPECT_EQ(d.tool().prompt(), "Specify second chamfer distance <1>");
    ASSERT_EQ(d.enter().outcome, kContinue);
    ASSERT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.pick(scene.up, 8, 4).outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{7, 0}, {8, 1}}));
}

TEST(ModifyEditChamfer, ParallelLinesAndDistancesTooLargeAreRefused)
{
    CornerScene scene;
    ToolDriver& d = scene.driver;
    const EntityId parallel = line(d, {0, 2}, {6, 2});
    d.start("modify.chamfer");
    ASSERT_EQ(d.type("9").outcome, kContinue);
    ASSERT_EQ(d.type("9").outcome, kContinue);
    ASSERT_EQ(d.pick(scene.along, 4, 0).outcome, kContinue);
    const ToolStep flat = d.pick(parallel, 3, 2);
    EXPECT_EQ(flat.outcome, kRejected);
    EXPECT_EQ(flat.message, "The lines are parallel, so there is no corner to chamfer.");
    const ToolStep large = d.pick(scene.up, 8, 4);
    EXPECT_EQ(large.outcome, kRejected);
    EXPECT_EQ(large.message, "The chamfer distances are too large for these lines.");
    EXPECT_EQ(d.type("-1").outcome, kRejected);
}

// ============================================================================
// Break and Break at Point
// ============================================================================

TEST(ModifyEditBreak, TwoPointsRemoveTheSpanBetweenThemFromALine)
{
    // (0,0)-(8,0) picked at (2,0), second point (6,0): (0,0)-(2,0) keeps the
    // line's id and (6,0)-(8,0) is new.
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    d.start("modify.break");
    EXPECT_EQ(d.tool().prompt(), "Select object to break");
    ASSERT_EQ(d.pick(target, 2, 0).outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify second break point or [First point]");
    const ToolStep finished = d.click(6, 0);
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Broken in two.");
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{6, 0}, {8, 0}}));
    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {8, 0}}));
    EXPECT_EQ(entityCount(d), 1u);
}

TEST(ModifyEditBreak, AtZeroZeroBreaksAtTheFirstPointWithoutAGap)
{
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 2, 0).outcome, kContinue);
    ASSERT_EQ(d.type("@0,0").outcome, kDone);
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{2, 0}, {8, 0}}));
}

TEST(ModifyEditBreak, FirstPointGivesTheFirstBreakPointExactly)
{
    // The pick at (1,0) only chooses the line; F then gives (2,0).
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 1, 0).outcome, kContinue);
    ASSERT_EQ(d.type("F").outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify first break point");
    ASSERT_EQ(d.click(2, 0).outcome, kContinue);
    ASSERT_EQ(d.type("@4,0").outcome, kDone); // relative to the first break point: (6,0)
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{6, 0}, {8, 0}}));
}

TEST(ModifyEditBreak, ASecondPointPastTheEndRemovesTheWholeEnd)
{
    // (10,0) is beyond (8,0): the nearest point of the line is its end, so
    // (6,0)-(8,0) goes and one line is left.
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 6, 0).outcome, kContinue);
    const ToolStep finished = d.click(10, 0);
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Broken.");
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {6, 0}}));
    EXPECT_EQ(entityCount(d), 1u);
}

TEST(ModifyEditBreak, ACircleLosesTheArcCounterClockwiseFromTheFirstPoint)
{
    // Radius 4: first point (4,0) at 0 degrees, second (0,4) at 90. The
    // quarter from 0 to 90 goes; the arc left starts at 90 and sweeps 270.
    ToolDriver d;
    const EntityId ring = d.add(cmd::createCircle({0, 0}, 4, d.document().currentAttributes()));
    d.start("modify.break");
    ASSERT_EQ(d.pick(ring, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(0, 4).outcome, kDone);
    const auto arc = geometryOf<Arc2>(d, ring);
    EXPECT_NEAR(arc.startAngle, kPi / 2, 1e-12);
    EXPECT_NEAR(arc.sweep, 1.5 * kPi, 1e-12);
    EXPECT_EQ(arc.radius, 4.0);
}

TEST(ModifyEditBreak, AnArcLosesTheSpanBetweenThePoints)
{
    // The half arc of radius 4 from 0 to 180 degrees, broken between 45 and
    // 135 degrees: a quarter is left at either end.
    ToolDriver d;
    const EntityId target =
        d.add(cmd::createArc(Arc2{{0, 0}, 4, 0, kPi}, d.document().currentAttributes()));
    const double r = 4.0 / std::sqrt(2.0);
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, r, r).outcome, kContinue);
    ASSERT_EQ(d.click(-r, r).outcome, kDone);
    const auto first = geometryOf<Arc2>(d, target);
    EXPECT_EQ(first.startAngle, 0.0);
    EXPECT_NEAR(first.sweep, kPi / 4, 1e-12);
    const auto second = geometryOf<Arc2>(d, created(d));
    EXPECT_NEAR(second.startAngle, 0.75 * kPi, 1e-12);
    EXPECT_NEAR(second.sweep, kPi / 4, 1e-12);
}

TEST(ModifyEditBreak, AnOpenPolylineIsBrokenAroundItsCorner)
{
    // (0,0)-(8,0)-(8,8) broken from (4,0) to (8,4): (0,0)-(4,0) and
    // (8,4)-(8,8) are left, the corner between them removed.
    ToolDriver d;
    const EntityId target = polyline(d, {{0, 0}, {8, 0}, {8, 8}});
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(8, 4).outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, target), (Polyline2{{{0, 0}, {4, 0}}, false}));
    EXPECT_EQ(geometryOf<Polyline2>(d, created(d)), (Polyline2{{{8, 4}, {8, 8}}, false}));
}

TEST(ModifyEditBreak, AClosedPolylineOpensWithTheSpanFromFirstToSecondRemoved)
{
    // The square (0,0)-(8,0)-(8,8)-(0,8) in drawing order: from (4,0) to
    // (8,4) goes, and the rest runs from (8,4) round to (4,0).
    ToolDriver d;
    const EntityId target = polyline(d, {{0, 0}, {8, 0}, {8, 8}, {0, 8}}, true);
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(8, 4).outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, target),
              (Polyline2{{{8, 4}, {8, 8}, {0, 8}, {0, 0}, {4, 0}}, false}));
}

TEST(ModifyEditBreak, TheHeightsAtTheBreakPointsAreInterpolatedAlongA3DString)
{
    // Heights 0, 8, 16 along (0,0)-(8,0)-(8,8): one per unit of length. The
    // break points at stations 4 and 12 get 4 and 12.
    ToolDriver d;
    const EntityId target =
        with3DHeights(d, Polyline2{{{0, 0}, {8, 0}, {8, 8}}, false}, {0.0, 8.0, 16.0});
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(8, 4).outcome, kDone);
    EXPECT_EQ(heights(d, target, 2), (std::vector<std::optional<double>>{0.0, 4.0}));
    EXPECT_EQ(heights(d, created(d), 2), (std::vector<std::optional<double>>{12.0, 16.0}));
}

TEST(ModifyEditBreak, BreaksThatWouldDoNothingOrEverythingAreRefused)
{
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    const EntityId ring = d.add(cmd::createCircle({0, 20}, 4, d.document().currentAttributes()));
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 0, 0).outcome, kContinue);
    const ToolStep atEnd = d.type("@0,0");
    EXPECT_EQ(atEnd.outcome, kRejected);
    EXPECT_EQ(atEnd.message, "The break point is at an end of the line, so there is nothing to "
                             "split.");
    const ToolStep whole = d.click(8, 0);
    EXPECT_EQ(whole.outcome, kRejected);
    EXPECT_EQ(whole.message, "The two points take in the whole line; use Erase to remove it.");
    EXPECT_EQ(d.undo().outcome, kContinue); // back to choosing the object
    ASSERT_EQ(d.pick(ring, 4, 20).outcome, kContinue);
    const ToolStep once = d.type("@0,0");
    EXPECT_EQ(once.outcome, kRejected);
    EXPECT_EQ(once.message, "A circle cannot be broken at a single point; pick two points.");
    EXPECT_EQ(d.executed(), 0);
}

TEST(ModifyEditBreak, ThePreviewShowsWhatTheCursorWouldRemove)
{
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    d.start("modify.break");
    ASSERT_EQ(d.pick(target, 2, 0).outcome, kContinue);
    const ToolFeedback feedback = d.tool().preview({6, 1});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], Geometry(Segment2{{2, 0}, {6, 0}}));
    EXPECT_EQ(feedback.markers, (std::vector<Point2>{{2, 0}, {6, 0}}));
}

TEST(ModifyEditBreakAtPoint, ALineIsSplitInTwoAtThePoint)
{
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    d.start("modify.break_at_point");
    ASSERT_EQ(d.pick(target, 5, 0).outcome, kContinue);
    EXPECT_EQ(d.tool().prompt(), "Specify break point");
    const ToolStep finished = d.click(2, 0);
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Broken in two.");
    EXPECT_EQ(geometryOf<Segment2>(d, target), (Segment2{{0, 0}, {2, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, created(d)), (Segment2{{2, 0}, {8, 0}}));
}

TEST(ModifyEditBreakAtPoint, AnOpenPolylineIsSplitAtAVertex)
{
    ToolDriver d;
    const EntityId target = polyline(d, {{0, 0}, {8, 0}, {8, 8}});
    d.start("modify.break_at_point");
    ASSERT_EQ(d.pick(target, 4, 0).outcome, kContinue);
    ASSERT_EQ(d.click(8, 0).outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, target), (Polyline2{{{0, 0}, {8, 0}}, false}));
    EXPECT_EQ(geometryOf<Polyline2>(d, created(d)), (Polyline2{{{8, 0}, {8, 8}}, false}));
}

TEST(ModifyEditBreakAtPoint, AClosedShapeOrAnEndIsRefused)
{
    ToolDriver d;
    const EntityId target = line(d, {0, 0}, {8, 0});
    const EntityId square = polyline(d, {{0, 10}, {4, 10}, {4, 14}}, true);
    d.start("modify.break_at_point");
    ASSERT_EQ(d.pick(square, 2, 10).outcome, kContinue);
    const ToolStep closed = d.click(2, 10);
    EXPECT_EQ(closed.outcome, kRejected);
    EXPECT_EQ(closed.message,
              "A closed polyline cannot be broken at a single point; pick two points.");
    ASSERT_EQ(d.undo().outcome, kContinue);
    ASSERT_EQ(d.pick(target, 4, 0).outcome, kContinue);
    const ToolStep end = d.click(8, 0);
    EXPECT_EQ(end.outcome, kRejected);
    EXPECT_EQ(end.message, "The break point is at an end of the line, so there is nothing to "
                           "split.");
}

// ============================================================================
// Join and Explode
// ============================================================================

TEST(ModifyEditJoin, LinesMeetingEndToEndBecomeOnePolyline)
{
    // Three lines, the middle one drawn backwards, chain (0,0)-(4,0)-(4,4)-(8,4).
    // The lowest id keeps its identity; the other two go.
    ToolDriver d;
    const EntityId first = line(d, {0, 0}, {4, 0});
    const EntityId second = line(d, {4, 4}, {4, 0});
    const EntityId third = line(d, {4, 4}, {8, 4});
    d.document().selection().set({first, second, third});
    d.start("modify.join");
    EXPECT_EQ(d.tool().prompt(),
              "Select lines and polylines to join or [Tolerance] (3 selected, ends within 0.001)");
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Joined 3 objects into 1 polyline.");
    EXPECT_EQ(geometryOf<Polyline2>(d, first),
              (Polyline2{{{0, 0}, {4, 0}, {4, 4}, {8, 4}}, false}));
    EXPECT_EQ(entityCount(d), 1u);
    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(entityCount(d), 3u);
    EXPECT_EQ(geometryOf<Segment2>(d, first), (Segment2{{0, 0}, {4, 0}}));
}

TEST(ModifyEditJoin, EndsWithinTheToleranceJoinAndTheEarlierEndIsKept)
{
    // The second line starts 0.0005 from the first's end, inside the default
    // 0.001: the joint is the first line's (4,0), unmoved.
    ToolDriver d;
    const EntityId first = line(d, {0, 0}, {4, 0});
    const EntityId second = line(d, {4.0005, 0}, {4, 4});
    d.start("modify.join");
    ASSERT_EQ(d.pick(first, 2, 0).outcome, kContinue);
    ASSERT_EQ(d.pick(second, 4, 2).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, first), (Polyline2{{{0, 0}, {4, 0}, {4, 4}}, false}));
}

TEST(ModifyEditJoin, EndsFartherApartThanTheToleranceAreRefused)
{
    ToolDriver d;
    const EntityId first = line(d, {0, 0}, {4, 0});
    const EntityId second = line(d, {4.0005, 0}, {4, 4});
    d.document().selection().set({first, second});
    d.start("modify.join");
    ASSERT_EQ(d.type("T").outcome, kContinue);
    EXPECT_EQ(d.tool().expects(), ToolInput::Value);
    EXPECT_EQ(d.type("-1").outcome, kRejected);
    ASSERT_EQ(d.type("0.0001").outcome, kContinue);
    const ToolStep refused = d.enter();
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message,
              "None of the selected lines and polylines meet end to end within 0.0001.");
    EXPECT_EQ(d.tool().expects(), ToolInput::Selection);
    // Put the default back for the rest of the program's tests.
    ASSERT_EQ(d.type("T").outcome, kContinue);
    ASSERT_EQ(d.type("0.001").outcome, kContinue);
}

TEST(ModifyEditJoin, FourLinesRoundASquareMakeAClosedPolyline)
{
    ToolDriver d;
    const EntityId a = line(d, {0, 0}, {4, 0});
    const EntityId b = line(d, {4, 0}, {4, 4});
    const EntityId c = line(d, {4, 4}, {0, 4});
    const EntityId e = line(d, {0, 4}, {0, 0});
    d.document().selection().set({a, b, c, e});
    d.start("modify.join");
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, a),
              (Polyline2{{{0, 0}, {4, 0}, {4, 4}, {0, 4}}, true}));
}

TEST(ModifyEditJoin, AnArcIsLeftOutAndTheToolSaysWhy)
{
    ToolDriver d;
    const EntityId a = line(d, {0, 0}, {4, 0});
    const EntityId b = line(d, {4, 0}, {4, 4});
    const EntityId bend =
        d.add(cmd::createArc(Arc2{{4, 6}, 2, -kPi / 2, kPi}, d.document().currentAttributes()));
    d.document().selection().set({a, b, bend});
    d.start("modify.join");
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Joined 2 objects into 1 polyline. 1 arc was left out: a "
                                "polyline has straight segments only.");
    EXPECT_TRUE(std::holds_alternative<Arc2>(entityOf(d, bend).geometry));
}

TEST(ModifyEditJoin, OnlyArcsIsRefused)
{
    ToolDriver d;
    const EntityId a =
        d.add(cmd::createArc(Arc2{{0, 0}, 2, 0, kPi}, d.document().currentAttributes()));
    const EntityId b =
        d.add(cmd::createArc(Arc2{{4, 0}, 2, kPi, kPi}, d.document().currentAttributes()));
    d.document().selection().set({a, b});
    d.start("modify.join");
    const ToolStep refused = d.enter();
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "Select at least two lines or open polylines to join. 2 arcs were "
                               "left out: a polyline has straight segments only.");
}

TEST(ModifyEditJoin, ALineJoinsOntoTheStartOfAPolylineWithItsHeights)
{
    // The polyline (4,0)-(8,0)-(8,4) at heights 4, 8, 12 is lowest-id and
    // kept; the line (0,0)-(4,0) at 0, 4 joins before its start.
    ToolDriver d;
    const EntityId string =
        with3DHeights(d, Polyline2{{{4, 0}, {8, 0}, {8, 4}}, false}, {4.0, 8.0, 12.0});
    const EntityId lead = with3DHeights(d, Segment2{{0, 0}, {4, 0}}, {0.0, 4.0});
    d.document().selection().set({string, lead});
    d.start("modify.join");
    ASSERT_EQ(d.enter().outcome, kDone);
    EXPECT_EQ(geometryOf<Polyline2>(d, string),
              (Polyline2{{{0, 0}, {4, 0}, {8, 0}, {8, 4}}, false}));
    EXPECT_EQ(heights(d, string, 4), (std::vector<std::optional<double>>{0.0, 4.0, 8.0, 12.0}));
}

TEST(ModifyEditJoin, EnterWithNothingSelectedIsRefused)
{
    ToolDriver d;
    d.start("modify.join");
    const ToolStep refused = d.enter();
    EXPECT_EQ(refused.outcome, kRejected);
    EXPECT_EQ(refused.message, "Select at least two lines or open polylines to join.");
}

TEST(ModifyEditJoin, ThePreviewShowsThePolylineEnterWouldMake)
{
    ToolDriver d;
    const EntityId a = line(d, {0, 0}, {4, 0});
    const EntityId b = line(d, {4, 0}, {4, 4});
    d.document().selection().set({a, b});
    d.start("modify.join");
    const ToolFeedback feedback = d.tool().preview({0, 0});
    ASSERT_EQ(feedback.shapes.size(), 1u);
    EXPECT_EQ(feedback.shapes[0], Geometry(Polyline2{{{0, 0}, {4, 0}, {4, 4}}, false}));
}

TEST(ModifyEditExplode, ARectangleComesApartIntoItsFourLines)
{
    ToolDriver d;
    const EntityId rectangle = polyline(d, {{0, 0}, {4, 0}, {4, 2}, {0, 2}}, true);
    d.document().selection().set({rectangle});
    d.start("modify.explode");
    EXPECT_EQ(d.tool().prompt(), "Select polylines to explode (1 selected)");
    const ToolStep finished = d.enter();
    ASSERT_EQ(finished.outcome, kDone);
    EXPECT_EQ(finished.message, "Exploded 1 polyline into 4 lines.");
    EXPECT_FALSE(d.document().model().entities.contains(rectangle));
    const auto lines = d.document().lastCreatedEntities();
    ASSERT_EQ(lines.size(), 4u);
    EXPECT_EQ(geometryOf<Segment2>(d, lines[0]), (Segment2{{0, 0}, {4, 0}}));
    EXPECT_EQ(geometryOf<Segment2>(d, lines[1]), (Segment2{{4, 0}, {4, 2}}));
    EXPECT_EQ(geometryOf<Segment2>(d, lines[2]), (Segment2{{4, 2}, {0, 2}}));
    EXPECT_EQ(geometryOf<Segment2>(d, lines[3]), (Segment2{{0, 2}, {0, 0}}));
    ASSERT_TRUE(d.document().undo().ok());
    EXPECT_EQ(entityCount(d), 1u);
    EXPECT_TRUE(d.document().model().entities.contains(rectangle));
}

TEST(ModifyEditExplode, EachLineKeepsTheHeightsOfItsTwoEnds)
{
    ToolDriver d;
    const EntityId string =
        with3DHeights(d, Polyline2{{{0, 0}, {4, 0}, {4, 4}}, false}, {1.0, 2.0, std::nullopt});
    d.start("modify.explode");
    ASSERT_EQ(d.pick(string, 2, 0).outcome, kContinue);
    ASSERT_EQ(d.enter().outcome, kDone);
    const auto lines = d.document().lastCreatedEntities();
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(heights(d, lines[0], 2), (std::vector<std::optional<double>>{1.0, 2.0}));
    EXPECT_EQ(heights(d, lines[1], 2), (std::vector<std::optional<double>>{2.0, std::nullopt}));
}

TEST(ModifyEditExplode, ASelectionWithNoPolylineIsRefusedAndOthersAreLeftAlone)
{
    ToolDriver d;
    const EntityId single = line(d, {0, 0}, {4, 0});
    d.start("modify.explode");
    const ToolStep empty = d.enter();
    EXPECT_EQ(empty.outcome, kRejected);
    EXPECT_EQ(empty.message, "Select the polylines to explode first.");
    ASSERT_EQ(d.pick(single, 2, 0).outcome, kContinue);
    const ToolStep none = d.enter();
    EXPECT_EQ(none.outcome, kRejected);
    EXPECT_EQ(none.message, "Only polylines can be exploded, and the selection has none that can "
                            "be. 1 object that is not a polyline was left as it was.");
    const EntityId string = polyline(d, {{0, 4}, {4, 4}, {4, 8}});
    ASSERT_EQ(d.pick(string, 2, 4).outcome, kContinue);
    const ToolStep mixed = d.enter();
    ASSERT_EQ(mixed.outcome, kDone);
    EXPECT_EQ(mixed.message,
              "Exploded 1 polyline into 2 lines. 1 object that is not a polyline was left as it "
              "was.");
    EXPECT_TRUE(d.document().model().entities.contains(single));
}

// ============================================================================
// The catalogue
// ============================================================================

TEST(ModifyEditCatalogue, EachToolIsFoundByItsAutoCadAliases)
{
    const auto& catalog = katana::cad::toolCatalog();
    const std::vector<std::pair<std::string, std::vector<std::string>>> expected = {
        {"modify.trim", {"TRIM", "TR"}},
        {"modify.extend", {"EXTEND", "EX"}},
        {"modify.offset", {"OFFSET", "O"}},
        {"modify.fillet", {"FILLET", "F"}},
        {"modify.chamfer", {"CHAMFER", "CHA"}},
        {"modify.break", {"BREAK", "BR"}},
        {"modify.break_at_point", {"BREAKATPOINT"}},
        {"modify.join", {"JOIN", "J"}},
        {"modify.explode", {"EXPLODE", "X"}},
    };
    for (const auto& [id, aliases] : expected) {
        const auto* info = catalog.find(id);
        ASSERT_NE(info, nullptr) << id;
        EXPECT_EQ(info->category, "Modify") << id;
        EXPECT_EQ(info->group, "Edit") << id;
        EXPECT_FALSE(info->tip.empty()) << id;
        for (const std::string& alias : aliases) {
            const auto* found = catalog.findByAlias(alias);
            ASSERT_NE(found, nullptr) << alias;
            EXPECT_EQ(found->id, id) << alias;
        }
    }
}

} // namespace
