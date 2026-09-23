// The tool host on its own (src/katana_qt/tools/tool_host), without a view:
// what it reports through its hooks, and that a hook may start another tool
// while the host is still dealing with the step that raised it - the window
// will do exactly that when it chains one command into the next.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "tools/tool_host.hpp"

using katana::cad::Document;
using katana::geometry::Point2;
using katana::qt::tools::ToolHost;

TEST(ToolHost, AToolStartedFromAHookOfTheStepThatFinishedAnotherKeepsRunning)
{
    // Move does not restart, so after its command the host ends it - unless
    // a hook has already started another tool, which must not be ended in
    // its place.
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0, 0), Point2(10, 0))).ok());
    document.selection().add(document.lastCreatedEntities().front());
    ToolHost host(document);
    std::vector<std::string> messages;
    host.onMessage = [&](const std::string& message) {
        messages.push_back(message);
        if (message.find("moved") != std::string::npos) {
            ASSERT_TRUE(host.start("draw.circle").ok());
        }
    };
    ASSERT_TRUE(host.start("modify.move").ok());
    (void)host.point(Point2(0, 0));
    (void)host.point(Point2(0, 5));

    EXPECT_EQ(host.activeId(), "draw.circle");
    ASSERT_FALSE(messages.empty());
    EXPECT_EQ(document.history().undoCount(), 2u) << "the line, then the move";
}

TEST(ToolHost, InputWithNoToolRunningIsRefusedAloud)
{
    Document document;
    ToolHost host(document);
    std::vector<std::string> refusals;
    host.onRejected = [&](const std::string& why) { refusals.push_back(why); };
    EXPECT_EQ(host.point(Point2(1, 1)), ToolHost::Outcome::Rejected);
    EXPECT_EQ(host.enter(), ToolHost::Outcome::Rejected);
    EXPECT_EQ(refusals.size(), 2u);
}

TEST(ToolHost, ARestartingToolReportsItsPromptAgainAndEndsOnlyOnEsc)
{
    // Point restarts after every point: two points are two commands, and the
    // tool is still running until Esc.
    Document document;
    ToolHost host(document);
    std::vector<std::string> finished;
    host.onFinished = [&](const std::string& id) { finished.push_back(id); };
    ASSERT_TRUE(host.start("draw.point").ok());
    EXPECT_EQ(host.point(Point2(1, 2)), ToolHost::Outcome::Done);
    EXPECT_EQ(host.point(Point2(3, 4)), ToolHost::Outcome::Done);
    EXPECT_TRUE(host.active());
    EXPECT_EQ(document.history().undoCount(), 2u);
    host.cancel();
    EXPECT_FALSE(host.active());
    ASSERT_EQ(finished.size(), 1u);
    EXPECT_EQ(finished.front(), "draw.point");
}

TEST(ToolHost, AbandonEndsTheToolWithoutCommittingItsPoints)
{
    // For a replaced document: the chain belongs to the drawing that went,
    // and the tool ends, as Esc ends it, so the toolbar hears of it.
    Document document;
    ToolHost host(document);
    std::vector<std::string> finished;
    host.onFinished = [&](const std::string& id) { finished.push_back(id); };
    ASSERT_TRUE(host.start("draw.line").ok());
    (void)host.point(Point2(0, 0));
    (void)host.point(Point2(5, 0));
    host.abandon();
    EXPECT_FALSE(host.active());
    EXPECT_EQ(document.history().undoCount(), 0u) << "Esc would have kept the segment";
    ASSERT_EQ(finished.size(), 1u);
    EXPECT_EQ(finished.front(), "draw.line");
}
