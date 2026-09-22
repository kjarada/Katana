#include <gtest/gtest.h>

#include <limits>
#include <vector>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "support/property.hpp"

using namespace katana::commands;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityType;
using katana::entity::Layer;
using katana::entity::Model;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::nearlyEqual;

namespace {

struct Fixture : ::testing::Test {
    Model model;
    CommandStack stack{model};

    EntityId mustCreate(CommandPtr command)
    {
        const auto status = stack.execute(std::move(command));
        EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
        const auto created = stack.lastCreatedEntities();
        EXPECT_EQ(created.size(), 1u);
        return created.empty() ? 0 : created.front();
    }

    const Segment2& lineOf(EntityId id) const
    {
        return std::get<Segment2>(model.entities.find(id)->geometry);
    }

    // Snapshot of every entity, for exact before/after comparison.
    std::vector<Entity> snapshot() const
    {
        std::vector<Entity> entities;
        model.entities.forEach([&](const Entity& e) { entities.push_back(e); });
        return entities;
    }
};

using CommandCreate = Fixture;
using CommandUndoRedo = Fixture;
using CommandTransform = Fixture;
using CommandEditing = Fixture;
using CommandAttributes = Fixture;
using CommandLayers = Fixture;
using CommandStackState = Fixture;
using CommandTransaction = Fixture;
using CommandFixture = Fixture;

} // namespace

// ---- creation ------------------------------------------------------------------

TEST_F(CommandCreate, EveryCreateCommandAddsOneEntityOfItsType)
{
    EXPECT_EQ(model.entities.find(mustCreate(createPoint(Point2(1, 2))))->type(), EntityType::Point);
    EXPECT_EQ(model.entities.find(mustCreate(createLine(Point2(0, 0), Point2(5, 5))))->type(),
              EntityType::Line);
    EXPECT_EQ(model.entities.find(mustCreate(createCircle(Point2(0, 0), 3.0)))->type(),
              EntityType::Circle);
    EXPECT_EQ(model.entities.find(mustCreate(createArc(Arc2{Point2(0, 0), 2.0, 0.0, kPi})))->type(),
              EntityType::Arc);
    EXPECT_EQ(model.entities
                  .find(mustCreate(createPolyline(
                      Polyline2{{Point2(0, 0), Point2(1, 0), Point2(1, 1)}, false})))
                  ->type(),
              EntityType::Polyline);
    EXPECT_EQ(model.entities.find(mustCreate(createText({Point2(0, 0), "BM1", 2.5, 0.0})))->type(),
              EntityType::Text);
    EXPECT_EQ(model.entities
                  .find(mustCreate(createDimension({Point2(0, 0), Point2(10, 0), 2.0, ""})))
                  ->type(),
              EntityType::Dimension);
    EXPECT_EQ(model.entities.size(), 7u);
    EXPECT_EQ(stack.undoCount(), 7u);
}

TEST_F(CommandCreate, InvalidGeometryIsRejectedBeforeTouchingTheModel)
{
    const auto status = stack.execute(createLine(Point2(3, 3), Point2(3, 3)));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidGeometry);
    EXPECT_TRUE(model.entities.empty());
    EXPECT_FALSE(stack.canUndo()); // rejected commands never enter the history
    EXPECT_EQ(model.entities.nextId(), 1u);

    EXPECT_FALSE(stack.execute(createCircle(Point2(0, 0), -1.0)).ok());
    EXPECT_FALSE(stack.execute(nullptr).ok());
}

TEST_F(CommandCreate, UnknownOrLockedLayerIsRejected)
{
    EXPECT_EQ(stack.execute(createLine(Point2(0, 0), Point2(1, 1), {"Missing", "", {}})).error().code,
              ErrorCode::NotFound);

    ASSERT_TRUE(stack.execute(createLayer(Layer{"Frozen", {}, true, /*locked=*/true})).ok());
    EXPECT_EQ(stack.execute(createLine(Point2(0, 0), Point2(1, 1), {"Frozen", "", {}})).error().code,
              ErrorCode::CommandRejected);
    EXPECT_EQ(stack.execute(createLine(Point2(0, 0), Point2(1, 1), {"0", "NoSuchStyle", {}}))
                  .error()
                  .code,
              ErrorCode::NotFound);
    EXPECT_TRUE(model.entities.empty());
}

// ---- undo / redo -----------------------------------------------------------------

TEST_F(CommandUndoRedo, UndoRemovesAndRedoRestoresTheSameId)
{
    const EntityId id = mustCreate(createLine(Point2(0, 0), Point2(10, 0)));
    const Entity original = *model.entities.find(id);

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_FALSE(model.entities.contains(id));
    EXPECT_TRUE(stack.canRedo());

    ASSERT_TRUE(stack.redo().ok());
    ASSERT_TRUE(model.entities.contains(id)); // not a fresh id: later history may refer to it
    EXPECT_EQ(*model.entities.find(id), original);
    EXPECT_EQ(stack.lastCreatedEntities(), (std::vector<EntityId>{id}));
}

TEST_F(CommandUndoRedo, RedoHistoryReferringToAnEntitySurvivesUndoingItsCreation)
{
    const EntityId id = mustCreate(createLine(Point2(0, 0), Point2(10, 0)));
    ASSERT_TRUE(stack.execute(moveEntities({id}, Vec2(5, 5))).ok());
    ASSERT_TRUE(stack.undo().ok()); // undo MOVE
    ASSERT_TRUE(stack.undo().ok()); // undo CREATE
    EXPECT_TRUE(model.entities.empty());
    ASSERT_TRUE(stack.redo().ok()); // CREATE again, same id
    ASSERT_TRUE(stack.redo().ok()); // MOVE still finds it
    EXPECT_EQ(lineOf(id), (Segment2{Point2(5, 5), Point2(15, 5)}));
}

TEST_F(CommandUndoRedo, NewCommandDiscardsRedoHistory)
{
    mustCreate(createPoint(Point2(0, 0)));
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(stack.canRedo());
    mustCreate(createPoint(Point2(1, 1)));
    EXPECT_FALSE(stack.canRedo());
    EXPECT_EQ(stack.redo().error().code, ErrorCode::InvalidState);
}

TEST_F(CommandUndoRedo, EmptyStacksReportInvalidState)
{
    EXPECT_EQ(stack.undo().error().code, ErrorCode::InvalidState);
    EXPECT_EQ(stack.redo().error().code, ErrorCode::InvalidState);
    EXPECT_TRUE(stack.undoName().empty());
}

TEST_F(CommandUndoRedo, UndoIsBitExactAfterChainsOfTransforms)
{
    // Inverse transforms would accumulate rounding error; before-images do not.
    const EntityId line = mustCreate(createLine(Point2(500000.123, 5000000.456), Point2(500010, 5000010)));
    const EntityId arc = mustCreate(createArc(Arc2{Point2(500005, 5000005), 3.3, 0.7, 1.9}));
    const auto original = snapshot();

    katana::test::Random random;
    const int steps = 50;
    for (int i = 0; i < steps; ++i) {
        ASSERT_TRUE(stack.execute(rotateEntities({line, arc},
                                                 Point2(random.real(4e5, 6e5), random.real(4e6, 6e6)),
                                                 random.real(-kPi, kPi)))
                        .ok());
        ASSERT_TRUE(stack.execute(scaleEntities({line, arc}, Point2(500000, 5000000),
                                                random.real(0.5, 2.0)))
                        .ok());
    }
    const auto transformedState = snapshot();
    for (int i = 0; i < 2 * steps; ++i) {
        ASSERT_TRUE(stack.undo().ok());
    }
    EXPECT_EQ(snapshot(), original); // exact operator==, no tolerance
    for (int i = 0; i < 2 * steps; ++i) {
        ASSERT_TRUE(stack.redo().ok());
    }
    EXPECT_EQ(snapshot(), transformedState);
}

// ---- transforms --------------------------------------------------------------------

TEST_F(CommandTransform, MoveRotateScaleMirror)
{
    const EntityId id = mustCreate(createLine(Point2(0, 0), Point2(10, 0)));

    ASSERT_TRUE(stack.execute(moveEntities({id}, Vec2(1, 2))).ok());
    EXPECT_EQ(lineOf(id), (Segment2{Point2(1, 2), Point2(11, 2)}));

    ASSERT_TRUE(stack.execute(rotateEntities({id}, Point2(1, 2), kHalfPi)).ok());
    EXPECT_TRUE(nearlyEqual(lineOf(id).end, Point2(1, 12)));

    ASSERT_TRUE(stack.execute(scaleEntities({id}, Point2(1, 2), 0.5)).ok());
    EXPECT_TRUE(nearlyEqual(lineOf(id).end, Point2(1, 7)));

    ASSERT_TRUE(stack.execute(mirrorEntities({id}, Point2(0, 0), Point2(0, 1))).ok());
    EXPECT_TRUE(nearlyEqual(lineOf(id).start, Point2(-1, 2)));
    EXPECT_EQ(model.entities.size(), 1u);
}

TEST_F(CommandTransform, CopyMirrorCopyAndArrayAddEntities)
{
    const EntityId id = mustCreate(createCircle(Point2(0, 0), 1.0));

    ASSERT_TRUE(stack.execute(copyEntities({id}, Vec2(5, 0))).ok());
    ASSERT_EQ(stack.lastCreatedEntities().size(), 1u);
    const EntityId copy = stack.lastCreatedEntities().front();
    EXPECT_EQ(std::get<Circle2>(model.entities.find(copy)->geometry).center, Point2(5, 0));
    EXPECT_EQ(std::get<Circle2>(model.entities.find(id)->geometry).center, Point2(0, 0));

    ASSERT_TRUE(stack.execute(mirrorEntities({copy}, Point2(0, 0), Point2(0, 1), true)).ok());
    EXPECT_EQ(model.entities.size(), 3u);

    ASSERT_TRUE(stack.execute(arrayEntities({id}, 2, 3, Vec2(10, 20))).ok());
    EXPECT_EQ(stack.lastCreatedEntities().size(), 5u); // 2x3 cells minus the original
    EXPECT_EQ(model.entities.size(), 8u);
    ASSERT_TRUE(stack.undo().ok()); // one undo removes the whole array
    EXPECT_EQ(model.entities.size(), 3u);
}

TEST_F(CommandTransform, InvalidArgumentsAreRejected)
{
    const EntityId id = mustCreate(createCircle(Point2(0, 0), 1.0));
    EXPECT_FALSE(stack.execute(scaleEntities({id}, Point2(0, 0), 0.0)).ok());
    EXPECT_FALSE(stack.execute(scaleEntities({id}, Point2(0, 0), -2.0)).ok());
    EXPECT_FALSE(stack.execute(mirrorEntities({id}, Point2(1, 1), Point2(1, 1))).ok());
    EXPECT_FALSE(stack.execute(arrayEntities({id}, 1, 1, Vec2(1, 1))).ok());
    EXPECT_FALSE(stack.execute(arrayEntities({id}, 0, 5, Vec2(1, 1))).ok());
    EXPECT_FALSE(stack.execute(moveEntities({}, Vec2(1, 1))).ok());     // empty selection
    EXPECT_FALSE(stack.execute(moveEntities({999}, Vec2(1, 1))).ok()); // unknown entity
    EXPECT_FALSE(stack.execute(moveEntities({id, id}, Vec2(1, 1))).ok()); // duplicate id
    EXPECT_EQ(stack.undoCount(), 1u);
}

TEST_F(CommandTransform, MultiEntityCommandsAreAtomic)
{
    const EntityId good = mustCreate(createLine(Point2(0, 0), Point2(10, 0)));
    const EntityId tiny = mustCreate(createCircle(Point2(0, 0), 1e-6));
    const auto before = snapshot();
    // Scaling by 1e-3 drives the tiny circle below the geometric tolerance. The
    // whole command must fail and the perfectly scalable line must not move.
    const auto status = stack.execute(scaleEntities({good, tiny}, Point2(0, 0), 1e-3));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(snapshot(), before);
}

// ---- delete -----------------------------------------------------------------------

TEST_F(CommandEditing, DeleteIsDestructiveAndUndoable)
{
    const EntityId a = mustCreate(createPoint(Point2(0, 0)));
    const EntityId b = mustCreate(createPoint(Point2(1, 1)));
    const auto before = snapshot();

    auto command = deleteEntities({a, b});
    EXPECT_TRUE(command->isDestructive());
    EXPECT_FALSE(createPoint(Point2(0, 0))->isDestructive());
    ASSERT_TRUE(stack.execute(std::move(command)).ok());
    EXPECT_TRUE(model.entities.empty());

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(snapshot(), before);
    EXPECT_FALSE(stack.execute(deleteEntities({a, 12345})).ok()); // all or nothing
    EXPECT_EQ(model.entities.size(), 2u);
}

// ---- trim / extend / offset / fillet / chamfer ---------------------------------------

TEST_F(CommandEditing, TrimSplitsALineAndKeepsItsAttributes)
{
    ASSERT_TRUE(stack.execute(createLayer(Layer{"Roads"})).ok());
    const EntityId target = mustCreate(createLine(Point2(0, 0), Point2(10, 0), {"Roads", "", {}}));
    const EntityId cutA = mustCreate(createLine(Point2(3, -1), Point2(3, 1)));
    const EntityId cutB = mustCreate(createLine(Point2(7, -1), Point2(7, 1)));

    ASSERT_TRUE(stack.execute(trimEntity(target, {cutA, cutB}, Point2(5, 0))).ok());
    EXPECT_EQ(lineOf(target), (Segment2{Point2(0, 0), Point2(3, 0)})); // identity kept
    ASSERT_EQ(stack.lastCreatedEntities().size(), 1u);
    const Entity& rest = *model.entities.find(stack.lastCreatedEntities().front());
    EXPECT_EQ(std::get<Segment2>(rest.geometry), (Segment2{Point2(7, 0), Point2(10, 0)}));
    EXPECT_EQ(rest.layer, "Roads");

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(lineOf(target), (Segment2{Point2(0, 0), Point2(10, 0)}));
    EXPECT_EQ(model.entities.size(), 3u);
}

TEST_F(CommandEditing, TrimACircleAgainstAPolylineCutter)
{
    const EntityId circle = mustCreate(createCircle(Point2(0, 0), 5.0));
    const EntityId cutter =
        mustCreate(createPolyline(Polyline2{{Point2(-10, 0), Point2(10, 0)}, false}));
    ASSERT_TRUE(stack.execute(trimEntity(circle, {cutter}, Point2(0, 5))).ok());
    const Entity& result = *model.entities.find(circle);
    ASSERT_EQ(result.type(), EntityType::Arc); // same entity, new geometry kind
    EXPECT_TRUE(nearlyEqual(std::get<Arc2>(result.geometry).midpoint(), Point2(0, -5)));
}

TEST_F(CommandEditing, TrimAndExtendFailuresLeaveTheModelAlone)
{
    const EntityId target = mustCreate(createLine(Point2(0, 0), Point2(10, 0)));
    const EntityId far = mustCreate(createLine(Point2(50, 50), Point2(60, 60)));
    const EntityId text = mustCreate(createText({Point2(0, 0), "T", 1.0, 0.0}));
    const auto before = snapshot();

    EXPECT_FALSE(stack.execute(trimEntity(target, {far}, Point2(5, 0))).ok());
    EXPECT_FALSE(stack.execute(extendEntity(target, {far}, Point2(10, 0))).ok());
    EXPECT_EQ(stack.execute(trimEntity(text, {target}, Point2(0, 0))).error().code,
              ErrorCode::Unsupported);
    EXPECT_EQ(snapshot(), before);
}

TEST_F(CommandEditing, ExtendReachesTheBoundary)
{
    const EntityId target = mustCreate(createLine(Point2(0, 0), Point2(4, 0)));
    const EntityId wall = mustCreate(createLine(Point2(9, -5), Point2(9, 5)));
    ASSERT_TRUE(stack.execute(extendEntity(target, {wall}, Point2(4, 0))).ok());
    EXPECT_EQ(lineOf(target), (Segment2{Point2(0, 0), Point2(9, 0)}));
}

TEST_F(CommandEditing, OffsetCreatesANewEntityOnThePickedSide)
{
    const EntityId line = mustCreate(createLine(Point2(0, 0), Point2(10, 0)));
    ASSERT_TRUE(stack.execute(offsetEntity(line, 2.0, Point2(5, -9))).ok());
    EXPECT_EQ(lineOf(stack.lastCreatedEntities().front()),
              (Segment2{Point2(0, -2), Point2(10, -2)}));
    EXPECT_EQ(lineOf(line), (Segment2{Point2(0, 0), Point2(10, 0)})); // source untouched

    const EntityId ring = mustCreate(createPolyline(
        Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10), Point2(0, 10)}, true}));
    ASSERT_TRUE(stack.execute(offsetEntity(ring, 1.0, Point2(5, 5))).ok()); // towards the inside
    const auto& inner =
        std::get<Polyline2>(model.entities.find(stack.lastCreatedEntities().front())->geometry);
    EXPECT_NEAR(inner.area(), 64.0, 1e-9);
    ASSERT_TRUE(stack.execute(offsetEntity(ring, 1.0, Point2(50, 5))).ok()); // towards the outside
    const auto& outer =
        std::get<Polyline2>(model.entities.find(stack.lastCreatedEntities().front())->geometry);
    EXPECT_NEAR(outer.area(), 144.0, 1e-9);
}

TEST_F(CommandEditing, FilletAndChamferModifyBothLinesAndAddTheJoin)
{
    const EntityId a = mustCreate(createLine(Point2(10, 0), Point2(0, 0)));
    const EntityId b = mustCreate(createLine(Point2(0, 0), Point2(0, 10)));

    ASSERT_TRUE(stack.execute(filletEntities(a, b, 2.0)).ok());
    ASSERT_EQ(stack.lastCreatedEntities().size(), 1u);
    EXPECT_EQ(model.entities.find(stack.lastCreatedEntities().front())->type(), EntityType::Arc);
    EXPECT_TRUE(nearlyEqual(lineOf(a).end, Point2(2, 0)));
    EXPECT_TRUE(nearlyEqual(lineOf(b).start, Point2(0, 2)));

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(lineOf(a), (Segment2{Point2(10, 0), Point2(0, 0)}));
    EXPECT_EQ(model.entities.size(), 2u);

    ASSERT_TRUE(stack.execute(chamferEntities(a, b, 1.0, 3.0)).ok());
    EXPECT_EQ(lineOf(stack.lastCreatedEntities().front()), (Segment2{Point2(1, 0), Point2(0, 3)}));

    EXPECT_FALSE(stack.execute(filletEntities(a, a, 1.0)).ok());
    EXPECT_FALSE(stack.execute(filletEntities(a, b, 500.0)).ok());
}

// ---- attributes ---------------------------------------------------------------------

TEST_F(CommandAttributes, LayerColorVisibilityAndProperties)
{
    ASSERT_TRUE(stack.execute(createLayer(Layer{"Survey"})).ok());
    const EntityId id = mustCreate(createPoint(Point2(0, 0)));

    ASSERT_TRUE(stack.execute(setEntityLayer({id}, "Survey")).ok());
    ASSERT_TRUE(stack.execute(setEntityColor({id}, katana::entity::Color{255, 0, 0, 255})).ok());
    ASSERT_TRUE(stack.execute(setEntityVisible({id}, false)).ok());
    ASSERT_TRUE(stack.execute(setEntityProperty({id}, "code", std::string("IP"))).ok());

    const Entity& entity = *model.entities.find(id);
    EXPECT_EQ(entity.layer, "Survey");
    EXPECT_EQ(entity.color->toHex(), "#FF0000");
    EXPECT_FALSE(entity.visible);
    EXPECT_EQ(std::get<std::string>(entity.properties.at("code")), "IP");

    EXPECT_EQ(stack.execute(setEntityLayer({id}, "Nowhere")).error().code, ErrorCode::NotFound);
    ASSERT_TRUE(stack.execute(removeEntityProperty({id}, "code")).ok());
    EXPECT_EQ(stack.execute(removeEntityProperty({id}, "code")).error().code, ErrorCode::NotFound);
    EXPECT_FALSE(stack.execute(setEntityProperty({id}, "", std::string("x"))).ok());
}

TEST_F(CommandAttributes, DefinedPropertiesAreTypeChecked)
{
    ASSERT_TRUE(model.properties
                    .define({"elevation", katana::entity::PropertyType::Real, "Ground level", {}})
                    .ok());
    const EntityId id = mustCreate(createPoint(Point2(0, 0)));
    EXPECT_TRUE(stack.execute(setEntityProperty({id}, "elevation", 101.25)).ok());
    EXPECT_FALSE(stack.execute(setEntityProperty({id}, "elevation", std::string("high"))).ok());
    EXPECT_DOUBLE_EQ(std::get<double>(model.entities.find(id)->properties.at("elevation")), 101.25);
}

TEST_F(CommandAttributes, EntitiesOnLockedLayersCannotBeEdited)
{
    ASSERT_TRUE(stack.execute(createLayer(Layer{"Control"})).ok());
    const EntityId id = mustCreate(createPoint(Point2(0, 0), {"Control", "", {}}));
    Layer locked = *model.layers.find("Control");
    locked.locked = true;
    ASSERT_TRUE(stack.execute(updateLayer(locked)).ok());

    EXPECT_EQ(stack.execute(moveEntities({id}, Vec2(1, 1))).error().code, ErrorCode::CommandRejected);
    EXPECT_EQ(stack.execute(deleteEntities({id})).error().code, ErrorCode::CommandRejected);
    EXPECT_EQ(stack.execute(setEntityLayer({id}, "0")).error().code, ErrorCode::CommandRejected);
    EXPECT_TRUE(model.entities.contains(id));
}

TEST_F(CommandAttributes, SetGeometryReplacesTheShape)
{
    const EntityId id = mustCreate(createCircle(Point2(0, 0), 1.0));
    ASSERT_TRUE(stack.execute(setEntityGeometry(id, Circle2{Point2(2, 2), 4.0})).ok());
    EXPECT_DOUBLE_EQ(std::get<Circle2>(model.entities.find(id)->geometry).radius, 4.0);
    EXPECT_FALSE(stack.execute(setEntityGeometry(id, Circle2{Point2(2, 2), 0.0})).ok());
}

// ---- layers -------------------------------------------------------------------------

TEST_F(CommandLayers, CreateUpdateDeleteWithUndo)
{
    ASSERT_TRUE(stack.execute(createLayer(Layer{"Terrain"})).ok());
    EXPECT_EQ(stack.execute(createLayer(Layer{"Terrain"})).error().code, ErrorCode::AlreadyExists);

    Layer hidden = *model.layers.find("Terrain");
    hidden.visible = false;
    ASSERT_TRUE(stack.execute(updateLayer(hidden)).ok());
    EXPECT_FALSE(model.layers.find("Terrain")->visible);
    EXPECT_EQ(stack.execute(updateLayer(hidden)).error().code, ErrorCode::CommandRejected); // no-op
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.layers.find("Terrain")->visible);

    ASSERT_TRUE(stack.execute(deleteLayer("Terrain")).ok());
    EXPECT_FALSE(model.layers.contains("Terrain"));
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.layers.contains("Terrain"));
}

TEST_F(CommandLayers, LayersInUseAndTheDefaultLayerCannotBeDeleted)
{
    ASSERT_TRUE(stack.execute(createLayer(Layer{"Busy"})).ok());
    mustCreate(createPoint(Point2(0, 0), {"Busy", "", {}}));
    EXPECT_EQ(stack.execute(deleteLayer("Busy")).error().code, ErrorCode::CommandRejected);
    EXPECT_EQ(stack.execute(deleteLayer("0")).error().code, ErrorCode::CommandRejected);
    EXPECT_EQ(stack.execute(deleteLayer("Ghost")).error().code, ErrorCode::NotFound);
    EXPECT_TRUE(deleteLayer("Busy")->isDestructive());
}

// ---- stack state ----------------------------------------------------------------------

TEST_F(CommandStackState, SavePointTracksModificationThroughUndoAndRedo)
{
    EXPECT_FALSE(stack.isModified());
    mustCreate(createPoint(Point2(0, 0)));
    EXPECT_TRUE(stack.isModified());

    stack.markSaved();
    EXPECT_FALSE(stack.isModified());

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(stack.isModified()); // behind the save point
    ASSERT_TRUE(stack.redo().ok());
    EXPECT_FALSE(stack.isModified()); // back at the save point

    ASSERT_TRUE(stack.undo().ok());
    mustCreate(createPoint(Point2(9, 9))); // the saved state is now unreachable
    EXPECT_TRUE(stack.isModified());
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(stack.isModified()); // an empty stack is NOT the saved state any more

    stack.clear();
    EXPECT_FALSE(stack.isModified());
    EXPECT_FALSE(stack.canUndo());
}

TEST_F(CommandStackState, PublishesEventsWithEntityLevelChanges)
{
    std::vector<CommandEvent> events;
    stack.addListener([&](const CommandEvent& event) { events.push_back(event); });

    const EntityId id = mustCreate(createLine(Point2(0, 0), Point2(1, 1)));
    ASSERT_TRUE(stack.execute(moveEntities({id}, Vec2(1, 0))).ok());
    ASSERT_TRUE(stack.undo().ok());
    ASSERT_TRUE(stack.redo().ok());
    EXPECT_FALSE(stack.execute(moveEntities({404}, Vec2(1, 0))).ok()); // no event for failures

    ASSERT_EQ(events.size(), 4u);
    EXPECT_EQ(events[0].kind, CommandEventKind::Executed);
    EXPECT_EQ(events[0].commandName, "CREATE_LINE");
    ASSERT_EQ(events[0].changes.size(), 1u);
    EXPECT_EQ(events[0].changes[0].kind, katana::entity::ChangeKind::EntityAdded);
    EXPECT_EQ(events[1].changes[0].kind, katana::entity::ChangeKind::EntityModified);
    EXPECT_EQ(events[2].kind, CommandEventKind::Undone);
    EXPECT_EQ(events[2].commandName, "MOVE");
    EXPECT_EQ(events[3].kind, CommandEventKind::Redone);
}

TEST_F(CommandStackState, LogsExecutionAndRejection)
{
    katana::core::Logger logger;
    std::vector<std::string> lines;
    logger.addSink([&](const katana::core::LogRecord& r) { lines.push_back(r.message); });
    Model loggedModel;
    CommandStack loggedStack(loggedModel, &logger);

    ASSERT_TRUE(loggedStack.execute(createPoint(Point2(0, 0))).ok());
    EXPECT_FALSE(loggedStack.execute(createCircle(Point2(0, 0), -1)).ok());
    EXPECT_EQ(lines, (std::vector<std::string>{"executed", "rejected"}));
}

// ---- transactions -----------------------------------------------------------------------

TEST_F(CommandTransaction, GroupsCommandsIntoOneUndoStep)
{
    auto transaction = std::make_unique<Transaction>("DRAW_RECTANGLE");
    transaction->add(createLine(Point2(0, 0), Point2(4, 0)));
    transaction->add(createLine(Point2(4, 0), Point2(4, 3)));
    transaction->add(createLine(Point2(4, 3), Point2(0, 3)));
    transaction->add(createLine(Point2(0, 3), Point2(0, 0)));
    ASSERT_TRUE(stack.execute(std::move(transaction)).ok());

    EXPECT_EQ(model.entities.size(), 4u);
    EXPECT_EQ(stack.undoCount(), 1u);
    EXPECT_EQ(stack.lastCreatedEntities().size(), 4u);
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.entities.empty());
    ASSERT_TRUE(stack.redo().ok());
    EXPECT_EQ(model.entities.size(), 4u);
}

TEST_F(CommandTransaction, RollsBackEverythingWhenALaterStepFails)
{
    const EntityId existing = mustCreate(createLine(Point2(0, 0), Point2(10, 0)));
    const auto before = snapshot();

    auto transaction = std::make_unique<Transaction>("BAD_BATCH");
    transaction->add(createCircle(Point2(0, 0), 5.0));
    transaction->add(moveEntities({existing}, Vec2(100, 100)));
    transaction->add(createCircle(Point2(0, 0), -1.0)); // fails
    const auto status = stack.execute(std::move(transaction));

    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().context.find("step=3"), std::string::npos) << status.error().context;
    EXPECT_EQ(snapshot(), before);
    EXPECT_EQ(stack.undoCount(), 1u);
}

TEST_F(CommandTransaction, LaterStepsMayDependOnEarlierOnes)
{
    // The layer does not exist when the transaction is validated, only once its
    // first step has run.
    auto transaction = std::make_unique<Transaction>("IMPORT");
    transaction->add(createLayer(Layer{"Imported"}));
    transaction->add(createPoint(Point2(1, 1), {"Imported", "", {}}));
    ASSERT_TRUE(stack.execute(std::move(transaction)).ok());
    EXPECT_EQ(model.entities.countOnLayer("Imported"), 1u);

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_FALSE(model.layers.contains("Imported"));
    EXPECT_FALSE(stack.execute(std::make_unique<Transaction>("EMPTY")).ok());
}

// ---- metadata is validated, not just properties --------------------------------------------

// Regression. createEntities() exists so importers can add fully specified
// entities, metadata included. Property values were validated but metadata was
// not, and the two are serialised by the SAME writer: a non-finite double is
// written as JSON null, which the loader then refuses. The result was a project
// that saved without complaint and could never be opened again - so the gap had
// to be closed at the command boundary, where the rest of the entity is checked.
TEST_F(CommandFixture, NonFiniteMetadataIsRejectedLikeNonFiniteProperties)
{
    const auto withValue = [](const char* key, double value, bool asMetadata) {
        Entity entity;
        entity.geometry = katana::entity::PointGeometry{Point2(1.0, 2.0)};
        (asMetadata ? entity.metadata : entity.properties).emplace(key, value);
        return entity;
    };

    for (const double bad : {std::numeric_limits<double>::quiet_NaN(),
                             std::numeric_limits<double>::infinity(),
                             -std::numeric_limits<double>::infinity()}) {
        for (const bool asMetadata : {false, true}) {
            const auto status =
                stack.execute(createEntities({withValue("elevation", bad, asMetadata)}));
            ASSERT_FALSE(status.ok())
                << (asMetadata ? "metadata" : "properties") << " accepted " << bad;
            EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
            EXPECT_TRUE(model.entities.empty());
        }
    }

    // A finite value is still accepted on both, so the guard has not simply
    // banned doubles from metadata.
    EXPECT_TRUE(stack.execute(createEntities({withValue("elevation", 42.5, true)})).ok());
    EXPECT_TRUE(stack.execute(createEntities({withValue("elevation", 42.5, false)})).ok());
    EXPECT_EQ(model.entities.size(), 2u);
}

// ---- renaming a table item (PLAN.MD 20.2, slice 3) ----------------------------------

using CommandTableRename = Fixture;

TEST_F(CommandTableRename, RenamingAStyleCarriesEveryEntityThatWoreIt)
{
    katana::entity::Style style;
    style.name = "SEWR Manhole Cover";
    style.symbol = "manhole";
    style.symbolSize = 1.2;
    ASSERT_TRUE(stack.execute(createStyle(style)).ok());
    const EntityId marked = mustCreate(createPoint(katana::geometry::Point2(1.0, 2.0)));
    const EntityId plain = mustCreate(createPoint(katana::geometry::Point2(3.0, 4.0)));
    ASSERT_TRUE(stack.execute(setEntityStyle({marked}, "SEWR Manhole Cover")).ok());

    ASSERT_TRUE(stack.execute(renameStyle("SEWR Manhole Cover", "Manhole")).ok());
    EXPECT_FALSE(model.styles.contains("SEWR Manhole Cover"));
    ASSERT_TRUE(model.styles.contains("Manhole"));
    EXPECT_EQ(model.styles.find("Manhole")->symbol, "manhole") << "the definition travels with the name";
    EXPECT_DOUBLE_EQ(model.styles.find("Manhole")->symbolSize, 1.2);
    EXPECT_EQ(model.entities.find(marked)->style, "Manhole");
    EXPECT_TRUE(model.entities.find(plain)->style.empty()) << "ByLayer is not a style being renamed";

    // One undo step puts back the name AND everything that wore it.
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.styles.contains("SEWR Manhole Cover"));
    EXPECT_FALSE(model.styles.contains("Manhole"));
    EXPECT_EQ(model.entities.find(marked)->style, "SEWR Manhole Cover");
    ASSERT_TRUE(stack.redo().ok());
    EXPECT_EQ(model.entities.find(marked)->style, "Manhole");
}

TEST_F(CommandTableRename, RenamingALinetypeCarriesTheLayersAndStylesThatNamedIt)
{
    katana::entity::Linetype fence;
    fence.name = "fence";
    fence.pattern = {{1.0}, {-0.5}};
    ASSERT_TRUE(stack.execute(createLinetype(fence)).ok());
    Layer layer;
    layer.name = "Boundary";
    layer.linetype = "fence";
    ASSERT_TRUE(stack.execute(createLayer(layer)).ok());
    katana::entity::Style style;
    style.name = "boundary";
    style.linetype = "fence";
    ASSERT_TRUE(stack.execute(createStyle(style)).ok());

    ASSERT_TRUE(stack.execute(renameLinetype("fence", "post and rail")).ok());
    EXPECT_EQ(model.layers.find("Boundary")->linetype, "post and rail");
    EXPECT_EQ(model.styles.find("boundary")->linetype, "post and rail");
    EXPECT_FALSE(model.linetypes.contains("fence"));
    // The delete guard reads the same holders the rename repointed, so it
    // now answers about the new name and no longer knows the old one - which
    // is the proof that the holders really moved rather than the table alone.
    EXPECT_EQ(stack.execute(deleteLinetype("post and rail")).error().code,
              ErrorCode::CommandRejected);
    EXPECT_EQ(stack.execute(deleteLinetype("fence")).error().code, ErrorCode::NotFound);

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(model.layers.find("Boundary")->linetype, "fence");
    EXPECT_EQ(model.styles.find("boundary")->linetype, "fence");
    EXPECT_TRUE(model.linetypes.contains("fence"));
}

TEST_F(CommandTableRename, ARenameOntoAnExistingNameOrOfAProtectedItemIsRefused)
{
    katana::entity::Style a;
    a.name = "a";
    katana::entity::Style b;
    b.name = "b";
    ASSERT_TRUE(stack.execute(createStyle(a)).ok());
    ASSERT_TRUE(stack.execute(createStyle(b)).ok());

    EXPECT_EQ(stack.execute(renameStyle("a", "b")).error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(stack.execute(renameStyle("nope", "c")).error().code, ErrorCode::NotFound);
    EXPECT_EQ(stack.execute(renameStyle("a", "")).error().code, ErrorCode::InvalidArgument);
    // Merging two tables by renaming one onto the other is refused for the
    // same reason a rename in any table is: the two definitions differ and
    // silently picking one would change how everything wearing it draws.
    EXPECT_EQ(stack.execute(renameLinetype("continuous", "solid")).error().code,
              ErrorCode::CommandRejected)
        << "the continuous linetype is what an unset ByLayer chain resolves to";
    EXPECT_TRUE(model.styles.contains("a")) << "a refused rename changes nothing";
    EXPECT_TRUE(model.linetypes.contains("continuous"));
}
