// The Document's current style (the lead's D9): what new work is drawn in,
// refused for a style that does not exist, and CLEARED - never left naming
// nothing - when the style goes by delete, merge, rename or undo.

#include <gtest/gtest.h>

#include "katana/cad/document.hpp"

using katana::cad::Document;
namespace cmd = katana::commands;

namespace {

struct CurrentStyle : ::testing::Test {
    Document document;

    void must(cmd::CommandPtr command)
    {
        const auto status = document.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    void addStyle(const char* name)
    {
        katana::entity::Style style;
        style.name = name;
        must(cmd::createStyle(style));
    }
};

} // namespace

TEST_F(CurrentStyle, NewWorkWearsTheCurrentStyleAndByLayerIsTheDefault)
{
    EXPECT_EQ(document.currentStyle(), "");
    EXPECT_EQ(document.currentAttributes().style, "") << "ByLayer until one is chosen";

    addStyle("Kerb");
    ASSERT_TRUE(document.setCurrentStyle("Kerb").ok());
    EXPECT_EQ(document.currentAttributes().style, "Kerb");
    EXPECT_EQ(document.currentAttributes().layer, "0") << "the current layer is unchanged";

    must(cmd::createPoint(katana::geometry::Point2(1.0, 1.0), document.currentAttributes()));
    const auto created = document.lastCreatedEntities();
    ASSERT_EQ(created.size(), 1u);
    EXPECT_EQ(document.model().entities.find(created.front())->style, "Kerb");

    ASSERT_TRUE(document.setCurrentStyle("").ok());
    EXPECT_EQ(document.currentAttributes().style, "");
}

TEST_F(CurrentStyle, AStyleThatDoesNotExistIsRefusedAndTheCurrentOneKept)
{
    addStyle("Kerb");
    ASSERT_TRUE(document.setCurrentStyle("Kerb").ok());
    const auto refused = document.setCurrentStyle("nosuch");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::NotFound);
    EXPECT_EQ(document.currentStyle(), "Kerb");
    // Case-sensitive, as every name is.
    EXPECT_FALSE(document.setCurrentStyle("KERB").ok());
}

TEST_F(CurrentStyle, DeletingMergingOrRenamingTheCurrentStyleClearsIt)
{
    addStyle("Kerb");
    addStyle("Kerb 2");

    ASSERT_TRUE(document.setCurrentStyle("Kerb 2").ok());
    must(cmd::deleteStyle("Kerb 2"));
    EXPECT_EQ(document.currentStyle(), "") << "deleted";
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.currentStyle(), "")
        << "undo brings the style back, not the choice: nothing records it";

    ASSERT_TRUE(document.setCurrentStyle("Kerb 2").ok());
    must(cmd::mergeStyle("Kerb 2", "Kerb"));
    EXPECT_EQ(document.currentStyle(), "") << "merged away";

    ASSERT_TRUE(document.setCurrentStyle("Kerb").ok());
    must(cmd::renameStyle("Kerb", "Kerb line"));
    EXPECT_EQ(document.currentStyle(), "")
        << "cleared, not followed: no command event says what a rename went to";

    // A style that stays is kept through other commands.
    ASSERT_TRUE(document.setCurrentStyle("Kerb line").ok());
    addStyle("Other");
    EXPECT_EQ(document.currentStyle(), "Kerb line");
}

TEST_F(CurrentStyle, UndoingTheStylesCreationClearsItAndANewDrawingStartsByLayer)
{
    addStyle("Kerb");
    ASSERT_TRUE(document.setCurrentStyle("Kerb").ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.currentStyle(), "");

    ASSERT_TRUE(document.redo().ok());
    ASSERT_TRUE(document.setCurrentStyle("Kerb").ok());
    document.newDocument();
    EXPECT_EQ(document.currentStyle(), "");
}
