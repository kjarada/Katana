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
