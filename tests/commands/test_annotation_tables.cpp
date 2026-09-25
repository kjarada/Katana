// The annotation tables' commands (docs/annotation.md): create, update and
// delete through the one table-command implementation, each one undo step,
// and the guards that refuse to delete a style something still names.

#include <gtest/gtest.h>

#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"

using namespace katana::entity;
using katana::commands::CommandStack;
using katana::core::ErrorCode;
using katana::geometry::Point2;
namespace cmd = katana::commands;

namespace {

LabelStyle bearingStyle()
{
    LabelStyle style;
    style.name = "Bearing";
    style.kind = LabelKind::Segment;
    style.text = "{bearing:dms} {distance:.3f}";
    return style;
}

} // namespace

TEST(AnnotationTables, EachEditIsOneUndoStep)
{
    Model model;
    CommandStack stack(model);
    TextStyle road;
    road.name = "Road";
    road.paperHeight = 3.5;
    ASSERT_TRUE(stack.execute(cmd::createTextStyle(road)).ok());
    road.paperHeight = 5.0;
    ASSERT_TRUE(stack.execute(cmd::updateTextStyle(road)).ok());
    ASSERT_TRUE(stack.execute(cmd::createLabelStyle(bearingStyle())).ok());
    LabelRule rule;
    rule.name = "all lines";
    rule.labelStyle = "Bearing";
    ASSERT_TRUE(stack.execute(cmd::createLabelRule(rule)).ok());
    EXPECT_EQ(stack.undoCount(), 4u);
    EXPECT_EQ(stack.undoName(), "CreateLabelRule");

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_FALSE(model.labelRules.contains("all lines"));
    ASSERT_TRUE(stack.undo().ok());
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_DOUBLE_EQ(model.textStyles.find("Road")->paperHeight, 3.5);
    ASSERT_TRUE(stack.redo().ok());
    EXPECT_DOUBLE_EQ(model.textStyles.find("Road")->paperHeight, 5.0);
}

TEST(AnnotationTables, AStyleStillNamedIsNotDeleted)
{
    Model model;
    CommandStack stack(model);
    TextStyle road;
    road.name = "Road";
    ASSERT_TRUE(stack.execute(cmd::createTextStyle(road)).ok());
    TextGeometry text{Point2(0, 0), "A", 2.5, 0.0};
    text.style = "Road";
    ASSERT_TRUE(stack.execute(cmd::createText(text)).ok());
    const auto refused = stack.execute(cmd::deleteTextStyle("Road"));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::CommandRejected);
    EXPECT_NE(refused.error().context.find("id="), std::string::npos) << "names the holder";

    EXPECT_FALSE(stack.execute(cmd::deleteTextStyle(std::string(kDefaultTextStyleName))).ok());

    ASSERT_TRUE(stack.execute(cmd::createLabelStyle(bearingStyle())).ok());
    LabelRule rule;
    rule.name = "lines";
    rule.labelStyle = "Bearing";
    ASSERT_TRUE(stack.execute(cmd::createLabelRule(rule)).ok());
    const auto byRule = stack.execute(cmd::deleteLabelStyle("Bearing"));
    ASSERT_FALSE(byRule.ok());
    EXPECT_NE(byRule.error().context.find("label rule=lines"), std::string::npos);
    ASSERT_TRUE(stack.execute(cmd::deleteLabelRule("lines")).ok());
    EXPECT_TRUE(stack.execute(cmd::deleteLabelStyle("Bearing")).ok());
}

TEST(AnnotationTables, AnAnnotationNamingAMissingStyleIsRefused)
{
    Model model;
    CommandStack stack(model);
    TextGeometry text{Point2(0, 0), "A", 2.5, 0.0};
    text.style = "Nowhere";
    const auto refused = stack.execute(cmd::createText(text));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::NotFound);

    cmd::ChangeSet changes;
    Entity label;
    label.geometry = LabelGeometry{.target = 1, .style = "Nowhere"};
    changes.add.push_back(label);
    EXPECT_FALSE(cmd::validateChangeSet(changes, model).ok());
}

TEST(AnnotationTables, ATemplateThatCannotPrintIsRefusedByTheCommand)
{
    Model model;
    CommandStack stack(model);
    LabelStyle style = bearingStyle();
    style.text = "{area}";
    EXPECT_FALSE(stack.execute(cmd::createLabelStyle(style)).ok());
    EXPECT_EQ(stack.undoCount(), 0u) << "a refused command is no step";
}

// A smart leader in a label style (docs/annotation.md, "Smart leaders"):
// the style is not deleted from under it, and a leader naming a style that
// is not there is refused on the way in.
TEST(AnnotationTables, ALabelStyleALeaderReadsIsNotDeletedAndMustExist)
{
    Model model;
    CommandStack stack(model);
    LabelStyle invert;
    invert.name = "Invert";
    invert.kind = LabelKind::Point;
    invert.text = "IL {prop.invert:.3f}";
    ASSERT_TRUE(stack.execute(cmd::createLabelStyle(invert)).ok());
    Entity pit;
    pit.geometry = PointGeometry{Point2(0, 0)};
    ASSERT_TRUE(stack.execute(cmd::createEntities({pit})).ok());
    Entity leader;
    leader.geometry = LeaderGeometry{.vertices = {Point2(0, 0), Point2(5, 5)},
                                     .tipRef = AnchorRef{1, AnchorPoint::Position},
                                     .labelStyle = "Invert"};
    ASSERT_TRUE(stack.execute(cmd::createEntities({leader})).ok());
    const auto refused = stack.execute(cmd::deleteLabelStyle("Invert"));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::CommandRejected);
    EXPECT_NE(refused.error().context.find("used by 1 leaders, e.g. id=2"), std::string::npos)
        << refused.error().describe();

    std::get<LeaderGeometry>(leader.geometry).labelStyle = "Nowhere";
    cmd::ChangeSet changes;
    changes.add.push_back(leader);
    const auto missing = cmd::validateChangeSet(changes, model);
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
}

// An annotation copied with what it refers to refers to the copy; copied
// alone, to the original (docs/annotation.md, "Associativity"). The ids a
// copy gets are the next ones, in the order the copies are made - ARRAY's
// every cell a set of its own.
TEST(AnnotationTables, CopiesOfAnnotationReferToCopiesOfWhatTheyReferTo)
{
    Model model;
    CommandStack stack(model);
    Entity pit;
    pit.geometry = PointGeometry{Point2(0, 0)};
    Entity line;
    line.geometry = katana::geometry::Segment2{Point2(0, 0), Point2(10, 0)};
    Entity leader;
    leader.geometry = LeaderGeometry{.vertices = {Point2(0, 0), Point2(5, 5)},
                                     .text = "{id}",
                                     .tipRef = AnchorRef{1, AnchorPoint::Position},
                                     .fields = true};
    Entity label;
    label.geometry = LabelGeometry{.target = 2, .style = "S", .anchor = Point2(5, 0)};
    DimensionGeometry dimension{Point2(0, 0), Point2(10, 0), 2.0, ""};
    dimension.startRef = AnchorRef{2, AnchorPoint::Start};
    dimension.endRef = AnchorRef{2, AnchorPoint::End};
    Entity dim;
    dim.geometry = dimension;
    LabelStyle style;
    style.name = "S";
    style.kind = LabelKind::Segment;
    style.text = "{distance}";
    ASSERT_TRUE(stack.execute(cmd::createLabelStyle(style)).ok());
    ASSERT_TRUE(stack.execute(cmd::createEntities({pit, line, leader, label, dim})).ok());

    // Ids 1 to 5; two cells of copies are 6 to 10 and 11 to 15.
    ASSERT_TRUE(stack.execute(cmd::arrayEntities({1, 2, 3, 4, 5}, 1, 3, {100.0, 0.0})).ok());
    for (const EntityId base : {EntityId{5}, EntityId{10}}) {
        const auto& leaderCopy = std::get<LeaderGeometry>(model.entities.find(base + 3)->geometry);
        EXPECT_EQ(leaderCopy.tipRef.entity, base + 1) << "the copied pit";
        const auto& labelCopy = std::get<LabelGeometry>(model.entities.find(base + 4)->geometry);
        EXPECT_EQ(labelCopy.target, base + 2) << "the copied line";
        const auto& dimCopy = std::get<DimensionGeometry>(model.entities.find(base + 5)->geometry);
        EXPECT_EQ(dimCopy.startRef.entity, base + 2);
        EXPECT_EQ(dimCopy.endRef.entity, base + 2);
    }
    ASSERT_TRUE(stack.execute(cmd::copyEntities({3}, {0.0, 50.0})).ok());
    EXPECT_EQ(std::get<LeaderGeometry>(model.entities.find(16)->geometry).tipRef.entity, 1u)
        << "copied alone: another callout of the same pit";
}
