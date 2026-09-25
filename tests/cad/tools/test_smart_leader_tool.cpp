// The Leader tool's smart leaders (docs/annotation.md, "Smart leaders";
// docs/tools.md): a tip clicked on an entity is put on it and follows it,
// and a note typed with fields in braces is read off it - driven through
// ToolDriver as the plan view drives the tool.

#include <string>
#include <variant>

#include <gtest/gtest.h>

#include "katana/cad/annotation/leader_draw.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/leader_values.hpp"
#include "tool_driver.hpp"

namespace {

using katana::cad::testing::ToolDriver;
using katana::entity::AnchorPoint;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using Outcome = katana::cad::ToolStep::Outcome;
namespace cmd = katana::commands;

// The one leader in the drawing.
LeaderGeometry theLeader(ToolDriver& driver)
{
    LeaderGeometry found;
    int count = 0;
    driver.document().model().entities.forEach([&](const Entity& entity) {
        if (const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry)) {
            found = *leader;
            ++count;
        }
    });
    EXPECT_EQ(count, 1);
    return found;
}

EntityId addLine(ToolDriver& driver)
{
    Entity line;
    line.geometry = Segment2{Point2(0, 0), Point2(40, 0)};
    line.properties["size"] = std::string("150");
    return driver.add(cmd::createEntities({line}));
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(SmartLeaderTool, ATipClickedOnALineIsPutOnItAndANoteWithFieldsIsReadOffIt)
{
    ToolDriver driver;
    const EntityId line = addLine(driver);
    driver.start("annotate.leader");
    // Within the driver's 0.01 pick aperture of the line, a quarter along.
    const auto tip = driver.click(10.0, 0.005);
    EXPECT_EQ(tip.outcome, Outcome::Continue);
    EXPECT_TRUE(contains(tip.message, "leader on Line " + std::to_string(line))) << tip.message;
    (void)driver.click(20.0, 10.0);
    (void)driver.enter();
    (void)driver.type("{prop.size} PVC");
    (void)driver.type("CH {chainage:.1f}");
    const auto done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done) << done.message;
    EXPECT_TRUE(contains(done.message, "read off entity " + std::to_string(line))) << done.message;

    const LeaderGeometry leader = theLeader(driver);
    EXPECT_EQ(leader.vertices.front(), Point2(10.0, 0.0)) << "put on the line";
    EXPECT_EQ(leader.tipRef.entity, line);
    EXPECT_EQ(leader.tipRef.point, AnchorPoint::Along);
    EXPECT_DOUBLE_EQ(leader.tipRef.parameter, 0.25);
    EXPECT_TRUE(leader.fields);
    EXPECT_EQ(katana::entity::leaderNote(driver.document().model(), leader), "150 PVC\nCH 10.0");

    // It follows the line.
    ASSERT_TRUE(driver.document()
                    .execute(cmd::setEntityGeometry(line, Segment2{Point2(0, 0), Point2(80, 0)}))
                    .ok());
    EXPECT_EQ(theLeader(driver).vertices.front(), Point2(20.0, 0.0));
}

TEST(SmartLeaderTool, AFieldTypedWrongIsRefusedAsItIsTyped)
{
    ToolDriver driver;
    addLine(driver);
    driver.start("annotate.leader");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(20.0, 10.0);
    (void)driver.enter();
    const auto wrong = driver.type("{sise}");
    EXPECT_EQ(wrong.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(wrong.message, "no value 'sise'")) << wrong.message;
    const auto unbalanced = driver.type("{size");
    EXPECT_EQ(unbalanced.outcome, Outcome::Rejected);
    // A literal brace is doubled.
    EXPECT_EQ(driver.type("{{150}} {prop.size}").outcome, Outcome::Continue);
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(katana::entity::leaderNote(driver.document().model(), theLeader(driver)),
              "{150} 150");
}

TEST(SmartLeaderTool, ANoteThatWouldSayNothingIsRefusedUntilALineIsTakenBack)
{
    ToolDriver driver;
    addLine(driver);
    driver.start("annotate.leader");
    (void)driver.click(10.0, 0.0);
    (void)driver.click(20.0, 10.0);
    (void)driver.enter();
    (void)driver.type("IL {prop.invert}");
    const auto refused = driver.enter();
    EXPECT_EQ(refused.outcome, Outcome::Rejected);
    EXPECT_TRUE(contains(refused.message, "no prop.invert")) << refused.message;
    EXPECT_EQ(driver.executed(), 0);
    (void)driver.undo();
    (void)driver.type("{length:.1f} m");
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    EXPECT_EQ(katana::entity::leaderNote(driver.document().model(), theLeader(driver)), "40.0 m");
}

TEST(SmartLeaderTool, ATipOnNothingKeepsItsNoteAsTyped)
{
    ToolDriver driver;
    addLine(driver);
    driver.start("annotate.leader");
    (void)driver.click(10.0, 5.0); // 5 from the line: outside the aperture
    (void)driver.click(20.0, 10.0);
    (void)driver.enter();
    (void)driver.type("{not a field}");
    const auto done = driver.enter();
    ASSERT_EQ(done.outcome, Outcome::Done);
    EXPECT_EQ(done.message, "leader with 1 line of text");
    const LeaderGeometry leader = theLeader(driver);
    EXPECT_FALSE(leader.tipRef.associated());
    EXPECT_FALSE(leader.fields);
    EXPECT_EQ(leader.text, "{not a field}");
}

TEST(SmartLeaderTool, ATipOnAnEntityWithAPlainNoteFollowsIt)
{
    ToolDriver driver;
    const EntityId line = addLine(driver);
    driver.start("annotate.leader");
    (void)driver.click(40.0, 0.0);
    (void)driver.click(50.0, 10.0);
    (void)driver.enter();
    (void)driver.type("KERB");
    const auto done = driver.enter();
    EXPECT_TRUE(contains(done.message, "on entity " + std::to_string(line))) << done.message;
    const LeaderGeometry leader = theLeader(driver);
    EXPECT_EQ(leader.tipRef.entity, line);
    EXPECT_FALSE(leader.fields);
    ASSERT_TRUE(driver.document().execute(cmd::moveEntities({line}, {0.0, 3.0})).ok());
    EXPECT_EQ(theLeader(driver).vertices.front(), Point2(40.0, 3.0));

    // Undo in the tip step lets the entity go as well as the point.
    driver.start("annotate.leader");
    (void)driver.click(40.0, 3.0);
    (void)driver.undo();
    (void)driver.click(100.0, 100.0);
    (void)driver.click(110.0, 110.0);
    (void)driver.enter();
    (void)driver.type("FREE");
    ASSERT_EQ(driver.enter().outcome, Outcome::Done);
    int free = 0;
    driver.document().model().entities.forEach([&](const Entity& entity) {
        const auto* other = std::get_if<LeaderGeometry>(&entity.geometry);
        free += other != nullptr && !other->tipRef.associated() ? 1 : 0;
    });
    EXPECT_EQ(free, 1);
}
