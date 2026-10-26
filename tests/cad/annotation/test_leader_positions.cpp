#include <gtest/gtest.h>

#include <limits>
#include <numbers>

#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/drawing/grips.hpp"

namespace ann = katana::cad::annotation;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;

TEST(LeaderPositions, APointAndItsFrameChangeTogetherInOneUndoStep)
{
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("LEADER 0,0 5,5 10,5 text=Note").ok());
    const auto original = document.model().entities.find(1)->geometry;
    const auto before = document.history().undoCount();
    const auto edited = interpreter.run(
        "LEADER SET 1 vertex=1 at=7,8 callout=box landingangle=180 paper=3");
    ASSERT_TRUE(edited.ok()) << edited.error().describe();
    const auto& leader = std::get<LeaderGeometry>(document.model().entities.find(1)->geometry);
    EXPECT_EQ(leader.vertices, (std::vector<Point2>{{0, 0}, {7, 8}, {10, 5}}));
    EXPECT_EQ(leader.callout, katana::entity::CalloutShape::Box);
    EXPECT_EQ(leader.landingAngle, std::numbers::pi);
    EXPECT_EQ(leader.text, "Note");
    EXPECT_DOUBLE_EQ(leader.paperHeight, 3);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.find(1)->geometry, original);
    ASSERT_TRUE(document.redo().ok());
    ASSERT_TRUE(interpreter.run("LEADER SET 1 landingangle=auto").ok());
    EXPECT_FALSE(std::get<LeaderGeometry>(document.model().entities.find(1)->geometry).landingAngle);
}

TEST(LeaderPositions, NoteAndTipCoordinatesAreEditableAndAnUnchangedPointAddsNoStep)
{
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("LEADER 0,0 5,5 10,5 text=Note").ok());
    ASSERT_TRUE(interpreter.run("LEADER SET 1 at=12,7").ok());
    ASSERT_TRUE(interpreter.run("LEADER SET 1 vertex=0 at=2,3").ok());
    const auto& leader = std::get<LeaderGeometry>(document.model().entities.find(1)->geometry);
    EXPECT_EQ(leader.vertices, (std::vector<Point2>{{2, 3}, {5, 5}, {12, 7}}));
    const auto before = document.history().undoCount();
    ASSERT_TRUE(interpreter.run("LEADER SET 1 vertex=1 at=5,5").ok());
    EXPECT_EQ(document.history().undoCount(), before);
}

TEST(LeaderPositions, InvalidPointEditsAndDirectionsLeaveTheWholeLeaderUnchanged)
{
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("LEADER 0,0 10,5 text=Note").ok());
    const auto before = *document.model().entities.find(1);
    const auto steps = document.history().undoCount();
    for (const char* options : {"vertex=-1 at=2,3", "vertex=2 at=2,3", "vertex=nope at=2,3",
                                "vertex=0", "vertex=0 at=nan,3", "vertex=1 at=0,0",
                                "vertex=0 at=2,3 tip=4,5", "landingangle=nan",
                                "landingangle=inf", "landingangle=wrong"}) {
        const auto reply = interpreter.run(std::string("LEADER SET 1 callout=box ") + options);
        EXPECT_FALSE(reply.ok()) << options;
        EXPECT_EQ(*document.model().entities.find(1), before);
        EXPECT_EQ(document.history().undoCount(), steps);
    }
}

TEST(LeaderPositions, FrameEditsTakeTheSharedScopeButAbsolutePointEditsRequireOneLeader)
{
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("LEADER 0,0 10,5 text=A").ok());
    ASSERT_TRUE(interpreter.run("LEADER 0,10 10,15 text=B").ok());
    ASSERT_TRUE(interpreter.run("LINE 0,0 10,0").ok());
    const auto before = document.history().undoCount();
    const auto frames = interpreter.run("LEADER SET DRAWING callout=box WHERE TYPE=leader");
    ASSERT_TRUE(frames.ok()) << frames.error().describe();
    EXPECT_NE(frames->find("matched=2"), std::string::npos);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(std::get<LeaderGeometry>(document.model().entities.find(1)->geometry).callout,
              katana::entity::CalloutShape::Box);
    EXPECT_EQ(std::get<LeaderGeometry>(document.model().entities.find(2)->geometry).callout,
              katana::entity::CalloutShape::Box);
    EXPECT_FALSE(interpreter.run("LEADER SET DRAWING vertex=1 at=20,20").ok());
    const auto empty = interpreter.run(
        "LEADER SET DRAWING vertex=1 at=20,20 WHERE LAYER=missing");
    ASSERT_TRUE(empty.ok()) << empty.error().describe();
    EXPECT_NE(empty->find("matched=0"), std::string::npos);
    EXPECT_EQ(document.history().undoCount(), before + 1);
}

TEST(LeaderPositions, GripsAndTheVerbUseTheSameVertexEditAndKeepAnAttachedTipAttached)
{
    LeaderGeometry leader{.vertices = {{0, 0}, {5, 5}, {10, 5}}, .text = "Note",
                          .callout = katana::entity::CalloutShape::Box};
    leader.landingAngle = std::numbers::pi;
    katana::entity::Entity entity;
    entity.id = 7;
    entity.geometry = leader;
    const auto grips = katana::cad::gripsOf(entity);
    ASSERT_EQ(grips.size(), 3u);
    EXPECT_EQ(grips[0].kind, katana::cad::GripKind::LeaderVertex);
    const auto moved = katana::cad::applyGripDrag(entity, {grips[1], {6, 9}, {}, false});
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    const auto direct = ann::moveLeaderVertices(leader, {{1, {6, 9}}});
    ASSERT_TRUE(direct.ok());
    EXPECT_EQ(std::get<LeaderGeometry>(moved->geometry), *direct);
    EXPECT_EQ(direct->landingAngle, leader.landingAngle);
    EXPECT_EQ(direct->callout, leader.callout);

    leader.tipRef = katana::entity::AnchorRef{42};
    entity.geometry = leader;
    const auto attached = katana::cad::gripsOf(entity);
    ASSERT_EQ(attached.size(), 2u);
    EXPECT_EQ(attached.front().index, 1u);
    EXPECT_FALSE(ann::moveLeaderVertices(leader, {{0, {1, 1}}}).ok());
    const auto note = ann::moveLeaderVertices(leader, {{2, {15, 8}}});
    ASSERT_TRUE(note.ok());
    EXPECT_EQ(note->tipRef, leader.tipRef);
}

TEST(LeaderPositions, MultipleHotPointsMoveAtomicallyAndInvalidCoordinatesAreRefused)
{
    const LeaderGeometry leader{.vertices = {{0, 0}, {1, 0}, {2, 0}}, .text = "Note"};
    const auto moved = ann::moveLeaderVertices(leader, {{0, {1, 0}}, {1, {2, 0}}, {2, {3, 0}}});
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    EXPECT_EQ(moved->vertices, (std::vector<Point2>{{1, 0}, {2, 0}, {3, 0}}));
    EXPECT_FALSE(ann::moveLeaderVertices(leader, {{3, {2, 3}}}).ok());
    EXPECT_FALSE(ann::moveLeaderVertices(leader, {{1, {2, 3}}, {1, {4, 5}}}).ok());
    EXPECT_FALSE(ann::moveLeaderVertices(
        leader, {{1, {std::numeric_limits<double>::infinity(), 0}}}).ok());
    EXPECT_FALSE(ann::moveLeaderVertices(leader, {{0, {-1e308, 0}}, {2, {1e308, 0}}}).ok());
}
