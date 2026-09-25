// CommandStack::undoNames and redoNames: the history the Edit toolbar's Undo
// and Redo lists show, newest first, where the k-th name is the step UNDO k or
// REDO k takes back or puts back last. The names are the commands' own:
// createPoint's is CREATE_POINT, createLine's CREATE_LINE.

#include <gtest/gtest.h>

#include <string_view>
#include <vector>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::commands::CommandStack;
using katana::geometry::Point2;
using Names = std::vector<std::string_view>;

namespace {

TEST(CommandStackNames, AnEmptyHistoryHasNoNames)
{
    katana::entity::Model model;
    CommandStack stack(model);
    EXPECT_TRUE(stack.undoNames().empty());
    EXPECT_TRUE(stack.redoNames().empty());
}

TEST(CommandStackNames, TheUndoHistoryIsNewestFirstAndBeginsWithUndoName)
{
    katana::entity::Model model;
    CommandStack stack(model);
    ASSERT_TRUE(stack.execute(katana::commands::createPoint(Point2(1, 1))).ok());
    ASSERT_TRUE(stack.execute(katana::commands::createLine(Point2(0, 0), Point2(5, 0))).ok());
    ASSERT_TRUE(stack.execute(katana::commands::createCircle(Point2(0, 0), 2.0)).ok());
    EXPECT_EQ(stack.undoNames(), (Names{"CREATE_CIRCLE", "CREATE_LINE", "CREATE_POINT"}));
    EXPECT_EQ(stack.undoNames().front(), stack.undoName());
    EXPECT_TRUE(stack.redoNames().empty());
}

TEST(CommandStackNames, UndoingMovesTheNamesToTheRedoHistoryNextFirst)
{
    katana::entity::Model model;
    CommandStack stack(model);
    ASSERT_TRUE(stack.execute(katana::commands::createPoint(Point2(1, 1))).ok());
    ASSERT_TRUE(stack.execute(katana::commands::createLine(Point2(0, 0), Point2(5, 0))).ok());
    ASSERT_TRUE(stack.execute(katana::commands::createCircle(Point2(0, 0), 2.0)).ok());
    ASSERT_TRUE(stack.undo().ok());
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(stack.undoNames(), (Names{"CREATE_POINT"}));
    // The line comes back first, then the circle.
    EXPECT_EQ(stack.redoNames(), (Names{"CREATE_LINE", "CREATE_CIRCLE"}));
    EXPECT_EQ(stack.redoNames().front(), stack.redoName());
    ASSERT_TRUE(stack.redo().ok());
    EXPECT_EQ(stack.undoNames(), (Names{"CREATE_LINE", "CREATE_POINT"}));
    EXPECT_EQ(stack.redoNames(), (Names{"CREATE_CIRCLE"}));
}

TEST(CommandStackNames, ANewStepClearsTheRedoHistory)
{
    katana::entity::Model model;
    CommandStack stack(model);
    ASSERT_TRUE(stack.execute(katana::commands::createPoint(Point2(1, 1))).ok());
    ASSERT_TRUE(stack.undo().ok());
    ASSERT_EQ(stack.redoNames().size(), 1U);
    ASSERT_TRUE(stack.execute(katana::commands::createLine(Point2(0, 0), Point2(5, 0))).ok());
    EXPECT_TRUE(stack.redoNames().empty());
    EXPECT_EQ(stack.undoNames(), (Names{"CREATE_LINE"}));
}

} // namespace
