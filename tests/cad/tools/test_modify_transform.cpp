// The Modify > Transform tools (src/katana_cad/tools/modify_transform.cpp):
// Move, Copy, Rotate, Scale, Mirror, Stretch, the two Arrays and Erase, driven
// as a user drives them - clicks, typed values, options, Undo, Enter - and
// checked against geometry worked out by hand in the comments.
//
// Inputs are chosen exact in binary wherever the operation allows (moves,
// scales by powers of two, mirrors about the axes), so those results are
// compared exactly. A rotation goes through cos and sin, which are not exact
// at 90 degrees, so rotated results are compared to within 1e-12.

#include <algorithm>
#include <cmath>
#include <string>
#include <utility>
#include <vector>

#include <gtest/gtest.h>

#include "katana/commands/entity_commands.hpp"
#include "katana/entity/leader_values.hpp"
#include "tool_driver.hpp"

namespace {

namespace cmd = katana::commands;

using katana::cad::ToolInput;
using katana::cad::ToolStep;
using katana::cad::testing::ToolDriver;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using Outcome = ToolStep::Outcome;

constexpr double kNear = 1e-12;

EntityId addLine(ToolDriver& driver, Point2 a, Point2 b)
{
    return driver.add(cmd::createLine(a, b));
}

void select(ToolDriver& driver, std::vector<EntityId> ids)
{
    driver.document().selection().set(std::move(ids));
}

const katana::entity::Entity* entityOf(ToolDriver& driver, EntityId id)
{
    return driver.document().model().entities.find(id);
}

Segment2 lineOf(ToolDriver& driver, EntityId id)
{
    const auto* entity = entityOf(driver, id);
    if (entity == nullptr || !std::holds_alternative<Segment2>(entity->geometry)) {
        ADD_FAILURE() << "entity " << id << " is not a line";
        return {};
    }
    return std::get<Segment2>(entity->geometry);
}

// Every line in the drawing, in id order - the order they were made in.
std::vector<Segment2> allLines(ToolDriver& driver)
{
    std::vector<std::pair<EntityId, Segment2>> found;
    driver.document().model().entities.forEach([&](const katana::entity::Entity& entity) {
        if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
            found.emplace_back(entity.id, *segment);
        }
    });
    std::ranges::sort(found, {}, &std::pair<EntityId, Segment2>::first);
    std::vector<Segment2> out;
    for (const auto& [id, segment] : found) {
        out.push_back(segment);
    }
    return out;
}

std::vector<Point2> allPoints(ToolDriver& driver)
{
    std::vector<std::pair<EntityId, Point2>> found;
    driver.document().model().entities.forEach([&](const katana::entity::Entity& entity) {
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry)) {
            found.emplace_back(entity.id, point->position);
        }
    });
    std::ranges::sort(found, {}, &std::pair<EntityId, Point2>::first);
    std::vector<Point2> out;
    for (const auto& [id, position] : found) {
        out.push_back(position);
    }
    return out;
}

std::size_t entityCount(ToolDriver& driver) { return driver.document().model().entities.size(); }

void expectNear(const Point2& actual, const Point2& expected, double tolerance = kNear)
{
    EXPECT_NEAR(actual.x, expected.x, tolerance) << "actual " << actual << " expected " << expected;
    EXPECT_NEAR(actual.y, expected.y, tolerance) << "actual " << actual << " expected " << expected;
}

// The line shapes of a preview, which is where the transformed selection is.
std::vector<Segment2> previewLines(const katana::cad::ToolFeedback& feedback)
{
    std::vector<Segment2> out;
    for (const auto& shape : feedback.shapes) {
        if (const auto* segment = std::get_if<Segment2>(&shape)) {
            out.push_back(*segment);
        }
    }
    return out;
}

bool contains(const std::vector<Segment2>& lines, const Segment2& wanted)
{
    return std::ranges::find(lines, wanted) != lines.end();
}

// A driver with one line selected, the usual start of a Modify tool.
struct OneLine {
    ToolDriver driver;
    EntityId id = katana::entity::kInvalidEntityId;

    OneLine(Point2 a, Point2 b, const char* tool)
    {
        id = addLine(driver, a, b);
        select(driver, {id});
        driver.start(tool);
    }
};

} // namespace

// ---- the catalogue -----------------------------------------------------------------

TEST(ModifyTransformTools, EveryToolIsInTheModifyMenusTransformGroupUnderItsAutoCadAliases)
{
    const auto& catalog = katana::cad::toolCatalog();
    const std::vector<std::pair<std::string, std::string>> aliases = {
        {"MOVE", "modify.move"},       {"M", "modify.move"},
        {"COPY", "modify.copy"},       {"CO", "modify.copy"},
        {"CP", "modify.copy"},         {"ROTATE", "modify.rotate"},
        {"RO", "modify.rotate"},       {"SCALE", "modify.scale"},
        {"SC", "modify.scale"},        {"MIRROR", "modify.mirror"},
        {"MI", "modify.mirror"},       {"STRETCH", "modify.stretch"},
        {"S", "modify.stretch"},       {"ARRAY", "modify.array_rectangular"},
        {"AR", "modify.array_rectangular"}, {"ARRAYRECT", "modify.array_rectangular"},
        {"ARRAYPOLAR", "modify.array_polar"}, {"ERASE", "modify.erase"},
        {"E", "modify.erase"},         {"DELETE", "modify.erase"},
        {"DEL", "modify.erase"},
    };
    for (const auto& [verb, id] : aliases) {
        const katana::cad::ToolInfo* tool = catalog.findByAlias(verb);
        ASSERT_NE(tool, nullptr) << verb;
        EXPECT_EQ(tool->id, id) << verb;
        EXPECT_EQ(tool->category, "Modify");
        EXPECT_EQ(tool->group, "Transform");
        EXPECT_FALSE(tool->tip.empty()) << id;
    }
}

// ---- asking for a selection ----------------------------------------------------------

TEST(ModifyTransformTools, WithNothingSelectedAToolAsksForASelectionAndRefusesAnEmptyOne)
{
    ToolDriver driver;
    const EntityId id = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    driver.start("modify.move");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection);
    EXPECT_EQ(driver.tool().prompt(), "Select entities to move or [All], then press Enter");

    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection);

    EXPECT_EQ(driver.pick(id, 0.5, 0.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
    EXPECT_EQ(driver.messages().back(), "1 entity selected");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(2.0, 0.0);
    EXPECT_EQ(lineOf(driver, id), (Segment2{{2.0, 0.0}, {3.0, 0.0}}));
}

TEST(ModifyTransformTools, EnterTakesWhatTheViewSelectedInTheDocumentWhileTheToolAsked)
{
    ToolDriver driver;
    const EntityId id = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    driver.start("modify.move");
    // The view selects as the Select tool does, into the document.
    select(driver, {id});
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point);
}

TEST(ModifyTransformTools, AllSelectsEverythingAndUndoTakesThePicksBackOneAtATime)
{
    ToolDriver driver;
    const EntityId a = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    const EntityId b = addLine(driver, {0.0, 1.0}, {1.0, 1.0});
    driver.start("modify.erase");
    EXPECT_EQ(driver.pick(a, 0.5, 0.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.pick(a, 0.5, 0.0).outcome, Outcome::Rejected) << "already selected";
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.undo().outcome, Outcome::Rejected) << "nothing left to undo";
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "the pick was undone";

    EXPECT_EQ(driver.type("all").outcome, Outcome::Continue);
    EXPECT_EQ(driver.messages().back(), "2 entities selected");
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(entityCount(driver), 0u);
    EXPECT_EQ(entityOf(driver, b), nullptr);
}

TEST(ModifyTransformTools, UndoAtTheFirstStepGoesBackToTheSelectionTheToolAskedFor)
{
    ToolDriver driver;
    const EntityId id = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    driver.start("modify.rotate");
    (void)driver.pick(id, 0.5, 0.0);
    (void)driver.enter();
    EXPECT_EQ(driver.tool().prompt(), "Specify base point");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Selection);
    // The pick is kept, so Enter goes straight on.
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify base point");
}

TEST(ModifyTransformTools, UndoAtTheFirstStepOfAGivenSelectionHasNothingToUndo)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.move");
    EXPECT_EQ(fixture.driver.undo().outcome, Outcome::Rejected);
}

// ---- Move ----------------------------------------------------------------------------

TEST(MoveTool, MovesTheSelectionFromTheBasePointToTheSecondPoint)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    EXPECT_EQ(driver.tool().prompt(), "Specify base point or [Displacement] <Displacement>");
    EXPECT_EQ(driver.click(0.0, 0.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point or <use first point as displacement>");
    EXPECT_EQ(driver.click(10.0, 5.0).outcome, Outcome::Done);
    // (1,2)-(3,2) + (10,5) = (11,7)-(13,7).
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{11.0, 7.0}, {13.0, 7.0}}));
    EXPECT_EQ(driver.messages().back(), "1 entity moved");
    EXPECT_TRUE(driver.finished()) << "a move needs a fresh selection, so it does not restart";
}

TEST(MoveTool, TakesATypedDisplacementAfterTheDisplacementOptionOrEnter)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    EXPECT_EQ(driver.type("d").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify displacement as dx,dy");
    EXPECT_EQ(driver.type("10,5").outcome, Outcome::Done);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{11.0, 7.0}, {13.0, 7.0}}));

    // Enter at the base point is the <Displacement> default, and a relative
    // displacement there is relative to nothing, i.e. the same numbers.
    driver.document().selection().set({fixture.id});
    driver.start("modify.move");
    EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "the displacement has no default";
    EXPECT_EQ(driver.type("@-10,-5").outcome, Outcome::Done);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{1.0, 2.0}, {3.0, 2.0}}));
}

TEST(MoveTool, TakesTheSecondPointRelativeOrPolarFromTheBasePoint)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    (void)driver.type("2,3");
    EXPECT_EQ(driver.type("@4,-1").outcome, Outcome::Done);
    // Displacement (4,-1): (5,1)-(7,1).
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{5.0, 1.0}, {7.0, 1.0}}));

    select(driver, {fixture.id});
    driver.start("modify.move");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.type("@5<90").outcome, Outcome::Done);
    // 5 at 90 degrees is (0,5): (5,6)-(7,6), x through cos(90 deg), not exact.
    const Segment2 moved = lineOf(driver, fixture.id);
    expectNear(moved.start, {5.0, 6.0});
    expectNear(moved.end, {7.0, 6.0});
}

TEST(MoveTool, ARelativePointWithADzMovesInPlanAsItAlwaysDid)
{
    // Move's points have no height to change: the dz of @dx,dy,dz is dropped,
    // as the z of x,y,z is. The dz rule of 2026-09-30 (a change of the last
    // point's height, refused where there is none) is for the tools that
    // take heights, and applied to every tool it refused "@0,5,0" here.
    for (const char* typed : {"@0,5,0", "@0,5,1"}) {
        OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
        ToolDriver& driver = fixture.driver;
        ASSERT_EQ(driver.type("0,0").outcome, Outcome::Continue);
        const ToolStep step = driver.type(typed);
        ASSERT_EQ(step.outcome, Outcome::Done) << typed << ": " << step.message;
        EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{1.0, 7.0}, {3.0, 7.0}})) << typed;
    }
}

TEST(MoveTool, EnterAtTheSecondPointUsesTheBasePointAsTheDisplacement)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(4.0, 1.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{5.0, 3.0}, {7.0, 3.0}}));
}

TEST(MoveTool, ATypedDistanceGoesThatFarTowardsTheCursor)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.type("10").outcome, Outcome::Rejected) << "no cursor has been seen yet";
    // The cursor at (3,4) is the direction (0.6, 0.8); 10 along it is (6,8).
    (void)driver.tool().preview({3.0, 4.0});
    EXPECT_EQ(driver.type("10").outcome, Outcome::Done);
    const Segment2 moved = lineOf(driver, fixture.id);
    expectNear(moved.start, {7.0, 10.0});
    expectNear(moved.end, {9.0, 10.0});
}

TEST(MoveTool, RefusesAZeroDisplacementAndStaysAtTheSecondPoint)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(1.0, 1.0);
    const ToolStep refused = driver.click(1.0, 1.0);
    EXPECT_EQ(refused.outcome, Outcome::Rejected);
    EXPECT_FALSE(refused.message.empty());
    EXPECT_EQ(driver.tool().prompt(), "Specify second point or <use first point as displacement>");
    EXPECT_EQ(driver.type("banana").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.click(2.0, 1.0).outcome, Outcome::Done);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{2.0, 2.0}, {4.0, 2.0}}));
}

TEST(MoveTool, UndoStepsBackToTheBasePoint)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(5.0, 5.0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify base point or [Displacement] <Displacement>");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(1.0, 0.0);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{2.0, 2.0}, {4.0, 2.0}}));
}

TEST(MoveTool, OneUndoOfTheDocumentPutsTheWholeSelectionBack)
{
    ToolDriver driver;
    const EntityId a = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    const EntityId b = addLine(driver, {0.0, 1.0}, {1.0, 1.0});
    select(driver, {a, b});
    driver.start("modify.move");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(3.0, 0.0);
    EXPECT_EQ(driver.messages().back(), "2 entities moved");
    EXPECT_EQ(driver.executed(), 1);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(lineOf(driver, a), (Segment2{{0.0, 0.0}, {1.0, 0.0}}));
    EXPECT_EQ(lineOf(driver, b), (Segment2{{0.0, 1.0}, {1.0, 1.0}}));
}

TEST(MoveTool, PreviewsTheSelectionMovedToTheCursorWithTheBandFromTheBasePoint)
{
    OneLine fixture({1.0, 2.0}, {3.0, 2.0}, "modify.move");
    ToolDriver& driver = fixture.driver;
    EXPECT_TRUE(driver.tool().preview({5.0, 0.0}).shapes.empty()) << "no base point yet";
    (void)driver.click(0.0, 0.0);
    const auto feedback = driver.tool().preview({5.0, 0.0});
    const auto lines = previewLines(feedback);
    EXPECT_TRUE(contains(lines, Segment2{{6.0, 2.0}, {8.0, 2.0}}));
    EXPECT_TRUE(contains(lines, Segment2{{0.0, 0.0}, {5.0, 0.0}}));
    ASSERT_EQ(feedback.markers.size(), 1u);
    EXPECT_EQ(feedback.markers[0], Point2(0.0, 0.0));
}

// ---- Copy ----------------------------------------------------------------------------

TEST(CopyTool, CopiesToEverySecondPointUntilEnterAsOneCommand)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.copy");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(0.0, 2.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point or [Exit/Undo] <Exit>");
    // Relative to the base point, as every second point is: (0,0) + (0,4).
    EXPECT_EQ(driver.type("@0,4").outcome, Outcome::Continue);
    EXPECT_EQ(driver.executed(), 0) << "nothing is made until Enter";
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(driver.messages().back(), "2 copies of 1 entity");
    const std::vector<Segment2> expected = {
        {{0.0, 0.0}, {1.0, 0.0}}, {{0.0, 2.0}, {1.0, 2.0}}, {{0.0, 4.0}, {1.0, 4.0}}};
    EXPECT_EQ(allLines(driver), expected);

    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(allLines(driver), std::vector<Segment2>{expected[0]}) << "one undo removes both";
}

TEST(CopyTool, UndoRemovesTheLastCopyPlacedAndTheUndoOptionDoesTheSame)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.copy");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, 2.0);
    (void)driver.click(0.0, 4.0);
    (void)driver.click(0.0, 6.0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("U").outcome, Outcome::Continue);
    EXPECT_EQ(driver.type("exit").outcome, Outcome::Done);
    const std::vector<Segment2> expected = {{{0.0, 0.0}, {1.0, 0.0}}, {{0.0, 2.0}, {1.0, 2.0}}};
    EXPECT_EQ(allLines(driver), expected);
}

TEST(CopyTool, EnterBeforeAnyCopyUsesTheBasePointAsTheDisplacement)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.copy");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(3.0, 0.5);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    const std::vector<Segment2> expected = {{{0.0, 0.0}, {1.0, 0.0}}, {{3.0, 0.5}, {4.0, 0.5}}};
    EXPECT_EQ(allLines(driver), expected);
}

TEST(CopyTool, TakesATypedDisplacementAndATypedDistanceTowardsTheCursor)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.copy");
    ToolDriver& driver = fixture.driver;
    (void)driver.type("D");
    EXPECT_EQ(driver.type("5,0").outcome, Outcome::Done);
    EXPECT_EQ(allLines(driver).back(), (Segment2{{5.0, 0.0}, {6.0, 0.0}}));

    select(driver, {fixture.id});
    driver.start("modify.copy");
    (void)driver.click(0.0, 0.0);
    (void)driver.tool().preview({0.0, -7.0}); // straight down: exact
    EXPECT_EQ(driver.type("2").outcome, Outcome::Continue);
    (void)driver.enter();
    EXPECT_EQ(allLines(driver).back(), (Segment2{{0.0, -2.0}, {1.0, -2.0}}));
}

TEST(CopyTool, RefusesACopyOntoTheOriginalAndExitWithNoCopiesMakesNothing)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.copy");
    ToolDriver& driver = fixture.driver;
    // A typed displacement of nothing puts the copy on the original.
    (void)driver.type("D");
    EXPECT_EQ(driver.type("0,0").outcome, Outcome::Rejected) << "a typed displacement of zero";
    (void)driver.undo();
    // With the base point at the origin, Enter's "use first point as
    // displacement" is a displacement of nothing too.
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "the base point is the origin";
    (void)driver.undo();
    (void)driver.click(1.0, 1.0);
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("E").outcome, Outcome::Done);
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(entityCount(driver), 1u);
}

TEST(CopyTool, PreviewsTheCopiesPlacedSoFarAndTheOneAtTheCursor)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.copy");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, 2.0);
    const auto lines = previewLines(driver.tool().preview({0.0, 5.0}));
    EXPECT_TRUE(contains(lines, Segment2{{0.0, 2.0}, {1.0, 2.0}})) << "the placed copy";
    EXPECT_TRUE(contains(lines, Segment2{{0.0, 5.0}, {1.0, 5.0}})) << "the copy at the cursor";
    EXPECT_TRUE(contains(lines, Segment2{{0.0, 0.0}, {0.0, 5.0}})) << "the band";
}

// ---- Rotate --------------------------------------------------------------------------

TEST(RotateTool, RotatesByATypedAngleCounterClockwiseAboutTheBasePoint)
{
    OneLine fixture({2.0, 0.0}, {4.0, 0.0}, "modify.rotate");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify rotation angle or [Copy/Reference]");
    EXPECT_EQ(driver.type("90").outcome, Outcome::Done);
    // (2,0) and (4,0) turned a quarter counter-clockwise about the origin.
    const Segment2 turned = lineOf(driver, fixture.id);
    expectNear(turned.start, {0.0, 2.0});
    expectNear(turned.end, {0.0, 4.0});
    EXPECT_EQ(driver.messages().back(), "1 entity rotated");
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{2.0, 0.0}, {4.0, 0.0}}));
}

TEST(RotateTool, APickedPointGivesTheAngleOfItsDirectionFromTheBasePoint)
{
    OneLine fixture({2.0, 0.0}, {4.0, 0.0}, "modify.rotate");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(1.0, 0.0);
    // (1,-3) is straight down from the base: -90 degrees. (2,0) is 1 east of
    // the base and goes to 1 south of it, (1,-1); (4,0) goes to (1,-3).
    EXPECT_EQ(driver.type("@0,-3").outcome, Outcome::Done);
    const Segment2 turned = lineOf(driver, fixture.id);
    expectNear(turned.start, {1.0, -1.0});
    expectNear(turned.end, {1.0, -3.0});
}

TEST(RotateTool, ReferenceTurnsFromTheCurrentAngleToTheNewOne)
{
    // A line at 45 degrees made to run due north: a turn of 45 degrees about
    // the origin takes (1,1) to (0, sqrt 2).
    const double root2 = std::sqrt(2.0);
    {
        OneLine fixture({0.0, 0.0}, {1.0, 1.0}, "modify.rotate");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        EXPECT_EQ(driver.type("r").outcome, Outcome::Continue);
        EXPECT_EQ(driver.tool().prompt(), "Specify the reference angle or its first point <0>");
        (void)driver.click(0.0, 0.0);
        (void)driver.click(1.0, 1.0);
        EXPECT_EQ(driver.tool().prompt(), "Specify the new angle or [Points]");
        EXPECT_EQ(driver.type("90").outcome, Outcome::Done);
        expectNear(lineOf(driver, fixture.id).end, {0.0, root2});
    }
    {
        // The same with the reference typed and the new angle given by Points.
        OneLine fixture({0.0, 0.0}, {1.0, 1.0}, "modify.rotate");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        (void)driver.type("REFERENCE");
        (void)driver.type("45");
        EXPECT_EQ(driver.type("p").outcome, Outcome::Continue);
        (void)driver.click(5.0, 5.0);
        EXPECT_EQ(driver.click(5.0, 6.0).outcome, Outcome::Done);
        expectNear(lineOf(driver, fixture.id).end, {0.0, root2});
    }
    {
        // Enter takes the <0> reference, so the new angle is the turn itself.
        OneLine fixture({0.0, 0.0}, {1.0, 1.0}, "modify.rotate");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        (void)driver.type("R");
        EXPECT_EQ(driver.enter().outcome, Outcome::Continue);
        EXPECT_EQ(driver.click(-3.0, 3.0).outcome, Outcome::Done) << "135 degrees from the base";
        // (1,1) turned by 135 degrees: its angle becomes 180, so (-sqrt 2, 0).
        expectNear(lineOf(driver, fixture.id).end, {-root2, 0.0});
    }
}

TEST(RotateTool, TheCopyOptionRotatesACopyAndLeavesTheOriginal)
{
    OneLine fixture({2.0, 0.0}, {4.0, 0.0}, "modify.rotate");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.type("C").outcome, Outcome::Continue);
    EXPECT_EQ(driver.messages().back(), "Rotating a copy of the selection.");
    EXPECT_EQ(driver.type("180").outcome, Outcome::Done);
    const auto lines = allLines(driver);
    ASSERT_EQ(lines.size(), 2u);
    EXPECT_EQ(lines[0], (Segment2{{2.0, 0.0}, {4.0, 0.0}}));
    expectNear(lines[1].start, {-2.0, 0.0});
    expectNear(lines[1].end, {-4.0, 0.0});
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(entityCount(driver), 1u);
}

TEST(RotateTool, RefusesNoTurnAPointWithNoDirectionAndACollapsedReferenceOrNewAngle)
{
    OneLine fixture({2.0, 0.0}, {4.0, 0.0}, "modify.rotate");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(1.0, 1.0);
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("360").outcome, Outcome::Rejected) << "a whole turn changes nothing";
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected) << "on the base point";
    EXPECT_EQ(driver.type("north").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "the angle has no default";
    (void)driver.type("R");
    (void)driver.click(3.0, 3.0);
    EXPECT_EQ(driver.click(3.0, 3.0).outcome, Outcome::Rejected) << "reference points coincide";
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point of the reference angle");

    // A reference of 30 degrees, then a new angle with no direction. Taken as
    // due east, either would turn the selection by -30 degrees unasked.
    (void)driver.undo();
    EXPECT_EQ(driver.type("30").outcome, Outcome::Continue);
    EXPECT_EQ(driver.click(1.0, 1.0).outcome, Outcome::Rejected) << "new angle on the base point";
    EXPECT_EQ(driver.type("P").outcome, Outcome::Continue);
    (void)driver.click(3.0, 3.0);
    EXPECT_EQ(driver.click(3.0, 3.0).outcome, Outcome::Rejected) << "new angle points coincide";
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point of the new angle");
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{2.0, 0.0}, {4.0, 0.0}}));
}

TEST(RotateTool, UndoStepsBackThroughTheReferenceDialogue)
{
    OneLine fixture({2.0, 0.0}, {4.0, 0.0}, "modify.rotate");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    (void)driver.type("R");
    (void)driver.type("30");
    EXPECT_EQ(driver.tool().prompt(), "Specify the new angle or [Points]");
    (void)driver.undo();
    EXPECT_EQ(driver.tool().prompt(), "Specify the reference angle or its first point <0>");
    (void)driver.undo();
    EXPECT_EQ(driver.tool().prompt(), "Specify rotation angle or [Copy/Reference]");
    (void)driver.undo();
    EXPECT_EQ(driver.tool().prompt(), "Specify base point");
}

TEST(RotateTool, PreviewsTheSelectionTurnedToTheCursor)
{
    OneLine fixture({2.0, 0.0}, {4.0, 0.0}, "modify.rotate");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    const auto lines = previewLines(driver.tool().preview({0.0, 5.0}));
    ASSERT_EQ(lines.size(), 2u) << "the turned line and the band";
    expectNear(lines[0].start, {0.0, 2.0});
    expectNear(lines[0].end, {0.0, 4.0});
    EXPECT_EQ(lines[1], (Segment2{{0.0, 0.0}, {0.0, 5.0}}));
}

// ---- Scale ---------------------------------------------------------------------------

TEST(ScaleTool, ScalesByATypedFactorAboutTheBasePoint)
{
    OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.tool().prompt(), "Specify scale factor or [Copy/Reference]");
    EXPECT_EQ(driver.type("2").outcome, Outcome::Done);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{2.0, 2.0}, {4.0, 2.0}}));
    EXPECT_EQ(driver.messages().back(), "1 entity scaled");
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{1.0, 1.0}, {2.0, 1.0}}));
}

TEST(ScaleTool, APickedPointGivesItsDistanceFromTheBasePointAsTheFactor)
{
    OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    // (3,4) is 5 from the origin: the classic 3-4-5 triangle.
    EXPECT_EQ(driver.click(3.0, 4.0).outcome, Outcome::Done);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{5.0, 5.0}, {10.0, 5.0}}));
}

TEST(ScaleTool, ReferenceScalesByTheRatioOfTheNewLengthToTheCurrentOne)
{
    {
        OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        (void)driver.type("R");
        (void)driver.type("4");
        EXPECT_EQ(driver.tool().prompt(), "Specify new length or [Points]");
        EXPECT_EQ(driver.type("10").outcome, Outcome::Done);
        // 10 / 4 = 2.5: (2.5,2.5)-(5,2.5).
        EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{2.5, 2.5}, {5.0, 2.5}}));
    }
    {
        // Both lengths picked: 4 up the y axis now, 8 along x wanted - factor 2.
        OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        (void)driver.type("reference");
        (void)driver.click(0.0, 0.0);
        (void)driver.click(0.0, 4.0);
        (void)driver.type("P");
        (void)driver.click(0.0, 0.0);
        EXPECT_EQ(driver.click(8.0, 0.0).outcome, Outcome::Done);
        EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{2.0, 2.0}, {4.0, 2.0}}));
    }
    {
        // Enter takes the <1> reference; the new length is then the factor.
        OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        (void)driver.type("R");
        (void)driver.enter();
        EXPECT_EQ(driver.type("3").outcome, Outcome::Done);
        EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{3.0, 3.0}, {6.0, 3.0}}));
    }
}

TEST(ScaleTool, TheCopyOptionScalesACopyAndLeavesTheOriginal)
{
    OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    (void)driver.type("copy");
    EXPECT_EQ(driver.type("0.5").outcome, Outcome::Done);
    const std::vector<Segment2> expected = {{{1.0, 1.0}, {2.0, 1.0}}, {{0.5, 0.5}, {1.0, 0.5}}};
    EXPECT_EQ(allLines(driver), expected);
}

// Every tool that copies copies a callout with its pit as a callout of the
// pit's copy - the tip on the copy, the note read off it - not a second
// callout of the original (docs/annotation.md, "Associativity"). The tools
// once built their copies apart from COPY and missed this.
TEST(CopyingTools, ACalloutCopiedWithItsPitReadsThePitsCopy)
{
    struct Case {
        const char* tool;
        std::vector<std::string> steps; // typed; "" is Enter
        std::vector<Point2> pits;       // where each copy of the pit lands
    };
    // The pit at (10,0). Copy by (100,0); rotate a copy half a turn and scale
    // one by 2 about the origin; four polar items a quarter-turn apart, the
    // original counted.
    const std::vector<Case> cases = {
        {"modify.copy", {"0,0", "@100,0", ""}, {{110.0, 0.0}}},
        {"modify.rotate", {"0,0", "C", "180"}, {{-10.0, 0.0}}},
        {"modify.scale", {"0,0", "copy", "2"}, {{20.0, 0.0}}},
        {"modify.array_polar", {"0,0", "4", "", ""}, {{0.0, 10.0}, {-10.0, 0.0}, {0.0, -10.0}}},
    };
    for (const Case& test : cases) {
        SCOPED_TRACE(test.tool);
        ToolDriver driver;
        const EntityId pit = driver.add(cmd::createPoint({10.0, 0.0}));
        katana::entity::Entity callout;
        callout.geometry =
            katana::entity::LeaderGeometry{.vertices = {Point2(10.0, 0.0), Point2(15.0, 5.0)},
                                           .text = "PIT {id}",
                                           .tipRef = katana::entity::AnchorRef{pit},
                                           .fields = true};
        const EntityId leader = driver.add(cmd::createEntities({callout}));
        select(driver, {pit, leader});
        driver.start(test.tool);
        for (const std::string& step : test.steps) {
            (void)(step.empty() ? driver.enter() : driver.type(step));
        }
        const auto& model = driver.document().model();
        const auto made = driver.document().lastCreatedEntities();
        ASSERT_EQ(made.size(), 2 * test.pits.size());
        for (std::size_t k = 0; k < test.pits.size(); ++k) {
            // Copied in selection order, a set per placement: pit, callout.
            const EntityId pitCopy = made[2 * k];
            const auto& copy = std::get<katana::entity::LeaderGeometry>(
                entityOf(driver, made[2 * k + 1])->geometry);
            EXPECT_EQ(copy.tipRef.entity, pitCopy);
            expectNear(copy.vertices.front(), test.pits[k]);
            EXPECT_EQ(katana::entity::leaderNote(model, copy), "PIT " + std::to_string(pitCopy));
        }
        const auto& original =
            std::get<katana::entity::LeaderGeometry>(entityOf(driver, leader)->geometry);
        EXPECT_EQ(original.tipRef.entity, pit);
        EXPECT_EQ(original.vertices.front(), Point2(10.0, 0.0));
    }
}

TEST(ScaleTool, RefusesAZeroNegativeOrUnitFactorAndAZeroReference)
{
    OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(2.0, 2.0);
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("-2").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("1").outcome, Outcome::Rejected) << "a factor of 1 changes nothing";
    EXPECT_EQ(driver.click(2.0, 2.0).outcome, Outcome::Rejected) << "the base point: factor 0";
    (void)driver.type("R");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    (void)driver.click(5.0, 5.0);
    EXPECT_EQ(driver.click(5.0, 5.0).outcome, Outcome::Rejected) << "a reference of length 0";
    (void)driver.undo();
    (void)driver.type("2");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected) << "a new length of 0";
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(lineOf(driver, fixture.id), (Segment2{{1.0, 1.0}, {2.0, 1.0}}));
}

TEST(ScaleTool, PreviewsTheSelectionScaledByTheCursorsDistance)
{
    OneLine fixture({1.0, 1.0}, {2.0, 1.0}, "modify.scale");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    const auto lines = previewLines(driver.tool().preview({0.0, 2.0}));
    EXPECT_TRUE(contains(lines, Segment2{{2.0, 2.0}, {4.0, 2.0}}));
}

// ---- Mirror --------------------------------------------------------------------------

TEST(MirrorTool, MirrorsAboutTheLineThroughTwoPointsKeepingTheSourceByDefault)
{
    OneLine fixture({1.0, 0.0}, {2.0, 1.0}, "modify.mirror");
    ToolDriver& driver = fixture.driver;
    EXPECT_EQ(driver.tool().prompt(), "Specify first point of mirror line");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(0.0, 1.0);
    EXPECT_EQ(driver.tool().prompt(), "Erase source entities? [Yes/No] <No>");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    // About the y axis x changes sign: (1,0)-(2,1) becomes (-1,0)-(-2,1).
    const std::vector<Segment2> expected = {{{1.0, 0.0}, {2.0, 1.0}}, {{-1.0, 0.0}, {-2.0, 1.0}}};
    EXPECT_EQ(allLines(driver), expected);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(entityCount(driver), 1u);
}

TEST(MirrorTool, YesErasesTheSourceAndNoKeepsIt)
{
    {
        OneLine fixture({1.0, 0.0}, {2.0, 1.0}, "modify.mirror");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        (void)driver.click(1.0, 0.0);
        EXPECT_EQ(driver.type("y").outcome, Outcome::Done);
        // About the x axis y changes sign.
        const std::vector<Segment2> expected = {{{1.0, 0.0}, {2.0, -1.0}}};
        EXPECT_EQ(allLines(driver), expected);
    }
    {
        OneLine fixture({1.0, 0.0}, {2.0, 1.0}, "modify.mirror");
        ToolDriver& driver = fixture.driver;
        (void)driver.click(0.0, 0.0);
        (void)driver.click(1.0, 0.0);
        EXPECT_EQ(driver.type("maybe").outcome, Outcome::Rejected);
        EXPECT_EQ(driver.type("NO").outcome, Outcome::Done);
        EXPECT_EQ(entityCount(driver), 2u);
    }
}

TEST(MirrorTool, RefusesAMirrorLineOfOnePointAndUndoStepsBack)
{
    OneLine fixture({1.0, 0.0}, {2.0, 1.0}, "modify.mirror");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(3.0, 3.0);
    EXPECT_EQ(driver.click(3.0, 3.0).outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point of mirror line");
    (void)driver.click(3.0, 4.0);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify second point of mirror line");
    EXPECT_EQ(driver.executed(), 0);
}

TEST(MirrorTool, PreviewsTheMirrorImageForTheCursorAsTheSecondPoint)
{
    OneLine fixture({1.0, 0.0}, {2.0, 1.0}, "modify.mirror");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    const auto lines = previewLines(driver.tool().preview({0.0, 5.0}));
    EXPECT_TRUE(contains(lines, Segment2{{-1.0, 0.0}, {-2.0, 1.0}}));
    EXPECT_TRUE(contains(lines, Segment2{{0.0, 0.0}, {0.0, 5.0}})) << "the mirror line";
}

// ---- Rectangular array ---------------------------------------------------------------

namespace {

std::vector<Point2> sortedStarts(ToolDriver& driver)
{
    std::vector<Point2> starts;
    for (const Segment2& line : allLines(driver)) {
        starts.push_back(line.start);
    }
    std::ranges::sort(starts, [](const Point2& a, const Point2& b) {
        return a.y != b.y ? a.y < b.y : a.x < b.x;
    });
    return starts;
}

} // namespace

TEST(ArrayRectangularTool, MakesRowsAndColumnsAtTheTypedSpacing)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.array_rectangular");
    ToolDriver& driver = fixture.driver;
    EXPECT_EQ(driver.tool().prompt(), "Enter the number of rows <1>");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    (void)driver.type("2");
    (void)driver.type("3");
    EXPECT_EQ(driver.tool().prompt(), "Enter the distance between rows or specify unit cell");
    (void)driver.type("5");
    EXPECT_EQ(driver.type("2").outcome, Outcome::Done);
    EXPECT_EQ(driver.messages().back(), "2 x 3 array of 1 entity");
    // Columns 2 apart along x, rows 5 apart along y; the original is (0,0).
    const std::vector<Point2> expected = {{0.0, 0.0}, {2.0, 0.0}, {4.0, 0.0},
                                          {0.0, 5.0}, {2.0, 5.0}, {4.0, 5.0}};
    EXPECT_EQ(sortedStarts(driver), expected);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(entityCount(driver), 1u);
}

TEST(ArrayRectangularTool, AUnitCellGivesBothSpacingsAtOnce)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.array_rectangular");
    ToolDriver& driver = fixture.driver;
    (void)driver.type("2");
    (void)driver.type("3");
    (void)driver.click(10.0, 10.0);
    // The cell from (10,10) to (12,15): 2 across, 5 up.
    const auto lines = previewLines(driver.tool().preview({12.0, 15.0}));
    EXPECT_EQ(lines.size(), 5u) << "the five copies";
    EXPECT_TRUE(contains(lines, Segment2{{4.0, 5.0}, {5.0, 5.0}}));
    // The opposite corner typed relative to the first: the same cell.
    EXPECT_EQ(driver.type("@2,5").outcome, Outcome::Done);
    const std::vector<Point2> expected = {{0.0, 0.0}, {2.0, 0.0}, {4.0, 0.0},
                                          {0.0, 5.0}, {2.0, 5.0}, {4.0, 5.0}};
    EXPECT_EQ(sortedStarts(driver), expected);
}

TEST(ArrayRectangularTool, OneRowAsksOnlyForTheColumnsAndOneColumnOnlyForTheRows)
{
    {
        OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.array_rectangular");
        ToolDriver& driver = fixture.driver;
        (void)driver.enter(); // <1> row
        (void)driver.type("4");
        EXPECT_EQ(driver.tool().prompt(), "Enter the distance between columns or its first point");
        // Two points 3 apart across the columns; the rise between them is not part of it.
        (void)driver.click(1.0, 1.0);
        EXPECT_EQ(driver.click(4.0, 7.0).outcome, Outcome::Done);
        const std::vector<Point2> expected = {{0.0, 0.0}, {3.0, 0.0}, {6.0, 0.0}, {9.0, 0.0}};
        EXPECT_EQ(sortedStarts(driver), expected);
    }
    {
        OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.array_rectangular");
        ToolDriver& driver = fixture.driver;
        (void)driver.type("3");
        (void)driver.enter(); // <1> column
        EXPECT_EQ(driver.type("-2").outcome, Outcome::Done) << "a negative spacing goes down";
        const std::vector<Point2> expected = {{0.0, -4.0}, {0.0, -2.0}, {0.0, 0.0}};
        EXPECT_EQ(sortedStarts(driver), expected);
    }
}

TEST(ArrayRectangularTool, RefusesBadCountsOneCellAZeroSpacingAFlatUnitCellAndLevelColumnPoints)
{
    {
        // Two points straight above one another are 0 apart across the
        // columns: every column would land on the first.
        OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.array_rectangular");
        ToolDriver& driver = fixture.driver;
        (void)driver.enter(); // <1> row
        (void)driver.type("2");
        (void)driver.click(1.0, 1.0);
        EXPECT_EQ(driver.click(1.0, 5.0).outcome, Outcome::Rejected) << "level across the columns";
        EXPECT_EQ(driver.tool().prompt(), "Specify second point of the distance between columns");
        EXPECT_EQ(driver.executed(), 0);
    }
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.array_rectangular");
    ToolDriver& driver = fixture.driver;
    EXPECT_EQ(driver.type("100001").outcome, Outcome::Rejected) << "100001 rows is over the limit";
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("2.5").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("rows").outcome, Outcome::Rejected);
    (void)driver.enter();
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "1 x 1 is the original alone";
    (void)driver.undo();
    (void)driver.type("1000");
    EXPECT_EQ(driver.type("101").outcome, Outcome::Rejected) << "101000 items is over the limit";
    (void)driver.type("2");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected) << "a spacing of zero";
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(3.0, 0.0).outcome, Outcome::Rejected) << "a cell with no height";
    EXPECT_EQ(driver.click(0.0, 3.0).outcome, Outcome::Rejected) << "a cell with no width";
    EXPECT_EQ(driver.executed(), 0);
}

TEST(ArrayRectangularTool, RefusesAColumnCountSoLargeThatRowsTimesColumnsWouldOverflow)
{
    OneLine fixture({0.0, 0.0}, {1.0, 0.0}, "modify.array_rectangular");
    ToolDriver& driver = fixture.driver;
    // 2^63 = 9223372036854775808, and 3074457345618258603 is 2^63 / 3 rounded
    // up, so 3 rows of it are 2^63 + 1 items - one past the largest int64.
    // Multiplied in int64 that wraps round to a negative count, which is
    // "under" the limit of 100000.
    (void)driver.type("3");
    EXPECT_EQ(driver.type("3074457345618258603").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), "Enter the number of columns <1>");
    // 2 rows of the largest int64, 2^63 - 1, are 2^64 - 2: -2 once wrapped.
    (void)driver.undo();
    (void)driver.type("2");
    EXPECT_EQ(driver.type("9223372036854775807").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.tool().prompt(), "Enter the number of columns <1>");
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(entityCount(driver), 1u);
}

// ---- Polar array ---------------------------------------------------------------------

TEST(ArrayPolarTool, SpacesTheItemsEvenlyRoundAWholeTurnCountingTheOriginal)
{
    ToolDriver driver;
    const EntityId id = driver.add(cmd::createPoint({10.0, 0.0}));
    select(driver, {id});
    driver.start("modify.array_polar");
    EXPECT_EQ(driver.tool().prompt(), "Specify center point of array");
    EXPECT_EQ(driver.type("0,0").outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().expects(), ToolInput::Value);
    (void)driver.type("4");
    EXPECT_EQ(driver.tool().prompt(), "Specify the angle to fill (+=ccw, -=cw) <360>");
    (void)driver.enter();
    EXPECT_EQ(driver.tool().prompt(), "Rotate arrayed entities? [Yes/No] <Yes>");
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    // Four items in 360 degrees are 90 apart: (10,0), (0,10), (-10,0), (0,-10).
    const auto points = allPoints(driver);
    ASSERT_EQ(points.size(), 4u);
    EXPECT_EQ(points[0], Point2(10.0, 0.0));
    expectNear(points[1], {0.0, 10.0});
    expectNear(points[2], {-10.0, 0.0});
    expectNear(points[3], {0.0, -10.0});
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(entityCount(driver), 1u);
}

TEST(ArrayPolarTool, APartTurnPutsTheFirstAndLastItemsAtItsEndsAndNegativeIsClockwise)
{
    {
        ToolDriver driver;
        select(driver, {driver.add(cmd::createPoint({10.0, 0.0}))});
        driver.start("modify.array_polar");
        (void)driver.click(0.0, 0.0);
        (void)driver.type("3");
        (void)driver.type("180");
        (void)driver.type("Y");
        // Three items over 180 degrees are 90 apart: the last at 180.
        const auto points = allPoints(driver);
        ASSERT_EQ(points.size(), 3u);
        expectNear(points[1], {0.0, 10.0});
        expectNear(points[2], {-10.0, 0.0});
    }
    {
        ToolDriver driver;
        select(driver, {driver.add(cmd::createPoint({10.0, 0.0}))});
        driver.start("modify.array_polar");
        (void)driver.click(0.0, 0.0);
        (void)driver.type("2");
        (void)driver.type("-90");
        (void)driver.enter();
        const auto points = allPoints(driver);
        ASSERT_EQ(points.size(), 2u);
        expectNear(points[1], {0.0, -10.0});
    }
}

TEST(ArrayPolarTool, ItemsThatDoNotTurnAreCarriedRoundByTheMiddleOfTheSelection)
{
    // A line from (9,0) to (11,0), its middle at (10,0), a quarter turn round
    // the origin: turned, it stands on end at (0,9)-(0,11); carried without
    // turning, it lies level about (0,10), at (-1,10)-(1,10).
    const auto quarter = [](const char* answer) {
        ToolDriver driver;
        select(driver, {addLine(driver, {9.0, 0.0}, {11.0, 0.0})});
        driver.start("modify.array_polar");
        (void)driver.click(0.0, 0.0);
        (void)driver.type("4");
        (void)driver.enter();
        EXPECT_EQ(driver.type(answer).outcome, Outcome::Done);
        return allLines(driver).at(1);
    };
    const Segment2 turned = quarter("yes");
    expectNear(turned.start, {0.0, 9.0});
    expectNear(turned.end, {0.0, 11.0});
    const Segment2 carried = quarter("n");
    expectNear(carried.start, {-1.0, 10.0});
    expectNear(carried.end, {1.0, 10.0});
}

TEST(ArrayPolarTool, RefusesTooFewItemsTooManyNoAngleTooMuchAngleAndAnUnclearAnswer)
{
    OneLine fixture({9.0, 0.0}, {11.0, 0.0}, "modify.array_polar");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.type("1").outcome, Outcome::Rejected) << "the original alone";
    EXPECT_EQ(driver.type("100001").outcome, Outcome::Rejected) << "over the limit of 100000";
    EXPECT_EQ(driver.type("six").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected) << "the count has no default";
    (void)driver.type("6");
    EXPECT_EQ(driver.type("0").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.type("400").outcome, Outcome::Rejected);
    (void)driver.type("90");
    EXPECT_EQ(driver.type("perhaps").outcome, Outcome::Rejected);
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify the angle to fill (+=ccw, -=cw) <360>");
    EXPECT_EQ(driver.executed(), 0);
}

TEST(ArrayPolarTool, PreviewsTheWholeTurnTheDefaultWouldMake)
{
    OneLine fixture({9.0, 0.0}, {11.0, 0.0}, "modify.array_polar");
    ToolDriver& driver = fixture.driver;
    (void)driver.click(0.0, 0.0);
    (void)driver.type("4");
    const auto feedback = driver.tool().preview({50.0, 50.0});
    EXPECT_EQ(previewLines(feedback).size(), 3u) << "the three copies";
    ASSERT_EQ(feedback.markers.size(), 1u);
    EXPECT_EQ(feedback.markers[0], Point2(0.0, 0.0));
}

// ---- Erase ---------------------------------------------------------------------------

TEST(EraseTool, OneEnterErasesTheSelectionItStartedWithAndOneUndoBringsItBack)
{
    ToolDriver driver;
    const EntityId a = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    const EntityId b = addLine(driver, {0.0, 1.0}, {1.0, 1.0});
    const EntityId kept = addLine(driver, {0.0, 2.0}, {1.0, 2.0});
    select(driver, {a, b});
    driver.start("modify.erase");
    EXPECT_EQ(driver.tool().prompt(), "Select entities to erase or [All], then press Enter");
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(driver.messages().back(), "2 entities erased");
    EXPECT_EQ(driver.executed(), 1);
    EXPECT_EQ(entityCount(driver), 1u);
    EXPECT_NE(entityOf(driver, kept), nullptr);
    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(entityCount(driver), 3u);
}

TEST(EraseTool, ASelectionChangedAtItsPromptIsWhatEnterErases)
{
    ToolDriver driver;
    const EntityId a = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    const EntityId b = addLine(driver, {0.0, 1.0}, {1.0, 1.0});
    const EntityId c = addLine(driver, {0.0, 2.0}, {1.0, 2.0});
    {
        // At "Select entities to erase" the view selects as the Select tool
        // does: a plain click on C replaces A and B with C, and C alone is
        // highlighted - so C alone is erased.
        select(driver, {a, b});
        driver.start("modify.erase");
        select(driver, {c});
        EXPECT_EQ(driver.enter().outcome, Outcome::Done);
        EXPECT_EQ(driver.messages().back(), "1 entity erased");
        EXPECT_NE(entityOf(driver, a), nullptr);
        EXPECT_NE(entityOf(driver, b), nullptr);
        EXPECT_EQ(entityOf(driver, c), nullptr);
        ASSERT_TRUE(driver.document().undo().ok());
    }
    {
        // Cleared there, nothing is highlighted and nothing is erased.
        select(driver, {a, b});
        driver.start("modify.erase");
        select(driver, {});
        const int executed = driver.executed();
        EXPECT_EQ(driver.enter().outcome, Outcome::Rejected);
        EXPECT_EQ(driver.executed(), executed);
        EXPECT_EQ(entityCount(driver), 3u);
    }
}

TEST(EraseTool, WithNothingSelectedEnterIsRefusedUntilSomethingIsPicked)
{
    ToolDriver driver;
    const EntityId id = addLine(driver, {0.0, 0.0}, {1.0, 0.0});
    driver.start("modify.erase");
    EXPECT_EQ(driver.enter().outcome, Outcome::Rejected);
    EXPECT_EQ(driver.click(0.5, 0.0).outcome, Outcome::Rejected) << "a click is not a pick";
    (void)driver.pick(id, 0.5, 0.0);
    EXPECT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(entityCount(driver), 0u);
}

// ---- Stretch -------------------------------------------------------------------------

TEST(StretchTool, MovesThePointsInsideTheWindowAndWhatLiesWhollyWithinIt)
{
    ToolDriver driver;
    const EntityId box = driver.add(cmd::createPolyline(
        Polyline2{{{0.0, 0.0}, {10.0, 0.0}, {10.0, 5.0}, {0.0, 5.0}}, true}));
    const EntityId inside = addLine(driver, {9.0, 2.0}, {11.0, 2.0});
    const EntityId across = addLine(driver, {7.0, 3.0}, {13.0, 3.0});
    const EntityId circle = driver.add(cmd::createCircle({20.0, 20.0}, 1.0));
    driver.start("modify.stretch");
    EXPECT_EQ(driver.tool().expects(), ToolInput::Point) << "the window is the selection";
    EXPECT_EQ(driver.tool().prompt(), "Specify first corner of the crossing window");
    (void)driver.click(12.0, 6.0);
    EXPECT_EQ(driver.click(8.0, -1.0).outcome, Outcome::Continue);
    EXPECT_EQ(driver.messages().back(), "2 entities to stretch");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.type("@3,0").outcome, Outcome::Done);
    EXPECT_EQ(driver.executed(), 1);

    // The window x 8..12, y -1..6 holds the rectangle's right-hand corners
    // and the whole short line; each moves 3 to the right.
    const auto* stretched = entityOf(driver, box);
    ASSERT_NE(stretched, nullptr);
    EXPECT_EQ(std::get<Polyline2>(stretched->geometry),
              (Polyline2{{{0.0, 0.0}, {13.0, 0.0}, {13.0, 5.0}, {0.0, 5.0}}, true}));
    EXPECT_EQ(lineOf(driver, inside), (Segment2{{12.0, 2.0}, {14.0, 2.0}}));
    // Crossing the window with neither end in it, the long line stays.
    EXPECT_EQ(lineOf(driver, across), (Segment2{{7.0, 3.0}, {13.0, 3.0}}));
    EXPECT_EQ(std::get<Circle2>(entityOf(driver, circle)->geometry),
              (Circle2{{20.0, 20.0}, 1.0}));

    ASSERT_TRUE(driver.document().undo().ok());
    EXPECT_EQ(std::get<Polyline2>(entityOf(driver, box)->geometry),
              (Polyline2{{{0.0, 0.0}, {10.0, 0.0}, {10.0, 5.0}, {0.0, 5.0}}, true}));
    EXPECT_EQ(lineOf(driver, inside), (Segment2{{9.0, 2.0}, {11.0, 2.0}}));
}

TEST(StretchTool, AnArcWithOneEndInsideKeepsItsHeightAboveTheChord)
{
    ToolDriver driver;
    // The upper half of the circle of radius 2 about the origin: ends (2,0)
    // and (-2,0), 2 above its chord at the top.
    const EntityId id =
        driver.add(cmd::createArc(Arc2{{0.0, 0.0}, 2.0, 0.0, katana::math::kPi}));
    driver.start("modify.stretch");
    (void)driver.click(1.5, -0.5);
    (void)driver.click(2.5, 0.5);
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(2.0, 0.0).outcome, Outcome::Done);
    // The end at (2,0) goes to (4,0). The new arc runs from (4,0) through
    // (1,2) - the chord's new middle, 2 above - to (-2,0). Its centre (1,k) is
    // as far from (4,0) as from (1,2): 9 + k^2 = (2 - k)^2, so k = -5/4, and
    // the radius is 2 - k = 13/4.
    const auto& arc = std::get<Arc2>(entityOf(driver, id)->geometry);
    expectNear(arc.center, {1.0, -1.25}, 1e-9);
    EXPECT_NEAR(arc.radius, 3.25, 1e-9);
    expectNear(arc.startPoint(), {4.0, 0.0}, 1e-9);
    expectNear(arc.endPoint(), {-2.0, 0.0}, 1e-9);
    EXPECT_GT(arc.sweep, 0.0) << "still counter-clockwise, bulging upwards";
    expectNear(arc.midpoint(), {1.0, 2.0}, 1e-9);
}

TEST(StretchTool, AnArcWhollyInsideMovesWholeAndADimensionMovesOnlyThePointInside)
{
    ToolDriver driver;
    const EntityId arc = driver.add(cmd::createArc(Arc2{{2.0, 2.0}, 1.0, 0.0, 1.5}));
    katana::entity::DimensionGeometry measured;
    measured.start = {-10.0, 2.0};
    measured.end = {2.0, 2.0};
    measured.offset = 1.5;
    const EntityId dimension = driver.add(cmd::createDimension(measured));
    driver.start("modify.stretch");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(4.0, 4.0);
    EXPECT_EQ(driver.messages().back(), "2 entities to stretch");
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(0.5, -1.0).outcome, Outcome::Done);
    // Both ends of the arc are inside, so it moves as it is: centre (2.5,1).
    EXPECT_EQ(std::get<Arc2>(entityOf(driver, arc)->geometry),
              (Arc2{{2.5, 1.0}, 1.0, 0.0, 1.5}));
    // Only the dimension's end at (2,2) is inside; its start stays put, and
    // it goes on measuring from there, to (2.5,1).
    const auto& stretched =
        std::get<katana::entity::DimensionGeometry>(entityOf(driver, dimension)->geometry);
    EXPECT_EQ(stretched.start, Point2(-10.0, 2.0));
    EXPECT_EQ(stretched.end, Point2(2.5, 1.0));
    EXPECT_EQ(stretched.offset, 1.5);
}

TEST(StretchTool, ACircleAPointAndATextMoveWhenTheirCentreOrPositionIsInside)
{
    ToolDriver driver;
    // Small enough to lie inside the window: a crossing window catches what
    // it touches, and a circle round the whole window touches none of it.
    const EntityId circle = driver.add(cmd::createCircle({1.0, 1.0}, 0.5));
    const EntityId point = driver.add(cmd::createPoint({2.0, 1.0}));
    katana::entity::TextGeometry label;
    label.position = {1.0, 2.0};
    label.text = "A";
    const EntityId text = driver.add(cmd::createText(label));
    driver.start("modify.stretch");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(3.0, 3.0);
    (void)driver.type("D");
    EXPECT_EQ(driver.type("0,4").outcome, Outcome::Done);
    EXPECT_EQ(std::get<Circle2>(entityOf(driver, circle)->geometry), (Circle2{{1.0, 5.0}, 0.5}));
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(entityOf(driver, point)->geometry).position,
              Point2(2.0, 5.0));
    EXPECT_EQ(std::get<katana::entity::TextGeometry>(entityOf(driver, text)->geometry).position,
              Point2(1.0, 6.0));
}

TEST(StretchTool, ASelectionMadeBeforehandLimitsWhatTheWindowCatches)
{
    ToolDriver driver;
    const EntityId chosen = addLine(driver, {0.0, 0.0}, {5.0, 0.0});
    const EntityId other = addLine(driver, {0.0, 1.0}, {5.0, 1.0});
    select(driver, {chosen});
    driver.start("modify.stretch");
    (void)driver.click(4.0, -1.0);
    (void)driver.click(6.0, 2.0);
    EXPECT_EQ(driver.messages().back(), "1 entity to stretch");
    (void)driver.click(0.0, 0.0);
    (void)driver.click(1.0, 0.0);
    EXPECT_EQ(lineOf(driver, chosen), (Segment2{{0.0, 0.0}, {6.0, 0.0}}));
    EXPECT_EQ(lineOf(driver, other), (Segment2{{0.0, 1.0}, {5.0, 1.0}}));
}

TEST(StretchTool, RefusesAFlatWindowAnEmptyOneNoDisplacementAndAnArcWhoseEndsWouldMeet)
{
    ToolDriver driver;
    const EntityId line = addLine(driver, {0.0, 0.0}, {10.0, 0.0});
    const EntityId arc = driver.add(cmd::createArc(Arc2{{20.0, 0.0}, 2.0, 0.0, katana::math::kPi}));
    driver.start("modify.stretch");
    (void)driver.click(1.0, 1.0);
    EXPECT_EQ(driver.click(1.0, 5.0).outcome, Outcome::Rejected) << "no width";
    // The line crosses this window, but neither end is in it.
    EXPECT_EQ(driver.click(5.0, -1.0).outcome, Outcome::Rejected) << "nothing to stretch";
    EXPECT_EQ(driver.tool().prompt(), "Specify opposite corner");
    (void)driver.undo();
    // Round the arc's east end (22,0): moving it 4 west puts it on the west end.
    (void)driver.click(21.5, -0.5);
    (void)driver.click(22.5, 0.5);
    (void)driver.click(0.0, 0.0);
    EXPECT_EQ(driver.click(0.0, 0.0).outcome, Outcome::Rejected) << "no displacement";
    const ToolStep collapsed = driver.click(-4.0, 0.0);
    EXPECT_EQ(collapsed.outcome, Outcome::Rejected);
    EXPECT_NE(collapsed.message.find("arc"), std::string::npos) << collapsed.message;
    EXPECT_EQ(driver.executed(), 0);
    EXPECT_EQ(lineOf(driver, line), (Segment2{{0.0, 0.0}, {10.0, 0.0}}));
    EXPECT_EQ(std::get<Arc2>(entityOf(driver, arc)->geometry),
              (Arc2{{20.0, 0.0}, 2.0, 0.0, katana::math::kPi}));
}

TEST(StretchTool, RefusesAStretchThatLeavesALineOrPolylineOfZeroLengthAndStaysPut)
{
    // The window round the east end (10,0) of a line or open polyline from
    // (0,0), and that end taken to (0,0) - the other end, where a snap puts
    // it: what is left has zero length, which the document would refuse.
    const auto collapse = [](katana::commands::CommandPtr make) {
        ToolDriver driver;
        const EntityId id = driver.add(std::move(make));
        const katana::entity::Geometry before = entityOf(driver, id)->geometry;
        driver.start("modify.stretch");
        (void)driver.click(9.0, -1.0);
        (void)driver.click(11.0, 1.0);
        (void)driver.click(10.0, 0.0);
        const ToolStep collapsed = driver.click(0.0, 0.0);
        EXPECT_EQ(collapsed.outcome, Outcome::Rejected);
        EXPECT_NE(collapsed.message.find("zero length"), std::string::npos) << collapsed.message;
        EXPECT_EQ(driver.executed(), 0);
        EXPECT_FALSE(driver.finished());
        EXPECT_EQ(entityOf(driver, id)->geometry, before);
        // The window and base point are kept: the next point finishes it.
        // (10,0) to (4,0) leaves 4 of the 10.
        EXPECT_EQ(driver.tool().prompt(),
                  "Specify second point or <use first point as displacement>");
        EXPECT_EQ(driver.click(4.0, 0.0).outcome, Outcome::Done);
        EXPECT_EQ(driver.executed(), 1);
        return entityOf(driver, id)->geometry;
    };
    EXPECT_EQ(collapse(cmd::createLine({0.0, 0.0}, {10.0, 0.0})),
              katana::entity::Geometry(Segment2{{0.0, 0.0}, {4.0, 0.0}}));
    EXPECT_EQ(collapse(cmd::createPolyline(Polyline2{{{0.0, 0.0}, {10.0, 0.0}}, false})),
              katana::entity::Geometry(Polyline2{{{0.0, 0.0}, {4.0, 0.0}}, false}));
}

TEST(StretchTool, UndoStepsBackAndThePreviewShowsTheStretchForTheCursor)
{
    ToolDriver driver;
    addLine(driver, {0.0, 0.0}, {10.0, 0.0});
    driver.start("modify.stretch");
    (void)driver.click(9.0, -1.0);
    const auto window = driver.tool().preview({11.0, 1.0});
    ASSERT_FALSE(window.shapes.empty());
    EXPECT_EQ(std::get<Polyline2>(window.shapes[0]),
              (Polyline2{{{9.0, -1.0}, {11.0, -1.0}, {11.0, 1.0}, {9.0, 1.0}}, true}));
    (void)driver.click(11.0, 1.0);
    (void)driver.click(10.0, 0.0);
    const auto lines = previewLines(driver.tool().preview({12.0, 0.0}));
    EXPECT_TRUE(contains(lines, Segment2{{0.0, 0.0}, {12.0, 0.0}})) << "the stretched line";
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify base point or [Displacement] <Displacement>");
    EXPECT_EQ(driver.undo().outcome, Outcome::Continue);
    EXPECT_EQ(driver.tool().prompt(), "Specify opposite corner");
}
