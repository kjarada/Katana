// The Modify > Edit tools (src/katana_cad/tools/modify_edit*.cpp), driven as
// the plan view drives them. Expected geometry is worked out by hand in the
// comments; coordinates are chosen so that the arithmetic is exact in binary
// wherever the construction allows it (quarters and halves of lengths that are
// powers of two), and compared to a stated tolerance where a square root or a
// trigonometric function is involved.

#include <cmath>
#include <optional>
#include <string>
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
// The catalogue
// ============================================================================

TEST(ModifyEditCatalogue, EachToolIsFoundByItsAutoCadAliases)
{
    const auto& catalog = katana::cad::toolCatalog();
    const std::vector<std::pair<std::string, std::vector<std::string>>> expected = {
        {"modify.trim", {"TRIM", "TR"}},
        {"modify.extend", {"EXTEND", "EX"}},
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
