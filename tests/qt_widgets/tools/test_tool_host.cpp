// The tool host on its own (src/katana_qt/tools/tool_host), without a view:
// what it reports through its hooks, and that a hook may start another tool
// while the host is still dealing with the step that raised it - the window
// will do exactly that when it chains one command into the next.

#include <gtest/gtest.h>

#include <string>
#include <variant>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/snapping.hpp"
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

// ---- Esc through the tool's own cancel(), transparent commands, selections ----------

TEST(ToolHost, EscInCopyKeepsTheCopiesAlreadyPlacedAsOneUndoStep)
{
    // Copy was left out of the host's old keep-on-Esc list, because its
    // Enter with no copy placed copies by the base point; the tool's own
    // cancel() now keeps what was placed and applies no default.
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0, 0), Point2(1, 0))).ok());
    document.selection().add(document.lastCreatedEntities().front());
    ToolHost host(document);
    std::vector<std::string> messages;
    host.onMessage = [&](const std::string& message) { messages.push_back(message); };
    ASSERT_TRUE(host.start("modify.copy").ok());
    (void)host.point(Point2(0, 0));
    (void)host.point(Point2(0, 5));
    (void)host.point(Point2(0, 10));
    host.cancel();
    EXPECT_FALSE(host.active());
    // The line, then ONE command for both copies: three lines in all.
    EXPECT_EQ(document.history().undoCount(), 2u);
    EXPECT_EQ(document.model().entities.size(), 3u);
    ASSERT_FALSE(messages.empty());
    EXPECT_EQ(messages.back(), "2 copies of 1 entity");
}

TEST(ToolHost, ZoomTypedWhileAToolRunsGoesToTheViewAndTheToolKeepsItsStep)
{
    Document document;
    ToolHost host(document);
    std::vector<std::string> transparent;
    host.onTransparent = [&](const std::string& command) {
        transparent.push_back(command);
        return true;
    };
    ASSERT_TRUE(host.start("draw.line").ok());
    (void)host.point(Point2(0, 0));
    EXPECT_EQ(host.typed("Z"), ToolHost::Outcome::Continue);
    EXPECT_EQ(host.typed("'zoom e"), ToolHost::Outcome::Continue);
    EXPECT_EQ(host.typed("PAN"), ToolHost::Outcome::Continue);
    EXPECT_EQ(transparent, (std::vector<std::string>{"Z", "'zoom e", "PAN"}));
    // Still at the second point of the same chain.
    EXPECT_EQ(host.lastPoint(), Point2(0, 0));
    EXPECT_EQ(host.typed("10,0"), ToolHost::Outcome::Continue);
    host.cancel();
    EXPECT_EQ(document.model().entities.size(), 1u);
}

TEST(ToolHost, ATransparentCommandWithNoViewToRunItIsRefusedNotSentToTheTool)
{
    Document document;
    ToolHost host(document);
    std::vector<std::string> refusals;
    host.onRejected = [&](const std::string& why) { refusals.push_back(why); };
    ASSERT_TRUE(host.start("draw.line").ok());
    EXPECT_EQ(host.typed("ZOOM"), ToolHost::Outcome::Rejected);
    ASSERT_EQ(refusals.size(), 1u);
    EXPECT_EQ(refusals.front(), "ZOOM cannot run inside Line; press Esc to end the tool first.");
    EXPECT_TRUE(host.active());
}

TEST(ToolHost, ABarePIsAnOptionForTheToolAndOnlyTheApostropheFormIsPan)
{
    using katana::qt::tools::isTransparentCommand;
    EXPECT_TRUE(isTransparentCommand("'P"));
    EXPECT_TRUE(isTransparentCommand(" zoom "));
    EXPECT_FALSE(isTransparentCommand("P")); // Rotate's and Scale's [Points]
    EXPECT_FALSE(isTransparentCommand("ZOOMY"));
    EXPECT_FALSE(isTransparentCommand("10,0"));
    // At a prompt for a value a bare word is the answer; 'Z is still ZOOM.
    EXPECT_FALSE(isTransparentCommand("Z", true));
    EXPECT_TRUE(isTransparentCommand("'Z", true));
}

TEST(ToolHost, AWordTypedAtAValuePromptIsTheToolsAnswerEvenWhenItReadsZoom)
{
    // Quick Select's layer prompt takes any name, "Z" included; only 'Z
    // goes to the view from there.
    Document document;
    ToolHost host(document);
    std::vector<std::string> transparent;
    host.onTransparent = [&](const std::string& command) {
        transparent.push_back(command);
        return true;
    };
    ASSERT_TRUE(host.start("select.quick").ok());
    EXPECT_EQ(host.typed("Any"), ToolHost::Outcome::Continue);
    EXPECT_EQ(host.typed("'Z"), ToolHost::Outcome::Continue);
    EXPECT_EQ(transparent, (std::vector<std::string>{"'Z"}));
    EXPECT_EQ(host.typed("Z"), ToolHost::Outcome::Continue); // the layer, "Z"
    EXPECT_EQ(transparent.size(), 1u);
    EXPECT_NE(host.prompt().find("Condition"), std::string::npos) << host.prompt();
}

TEST(ToolHost, ASelectingToolsAnswerIsLeftInTheDocumentsSelection)
{
    Document document;
    std::vector<katana::entity::EntityId> lines;
    for (const double y : {0.0, 1.0, 2.0}) {
        ASSERT_TRUE(
            document.execute(katana::commands::createLine(Point2(0, y), Point2(1, y))).ok());
        lines.push_back(document.lastCreatedEntities().front());
    }
    ASSERT_TRUE(document.execute(katana::commands::createCircle(Point2(5, 5), 1.0)).ok());
    const auto circle = document.lastCreatedEntities().front();
    document.selection().set({lines.front()});
    int notified = 0;
    auto listening = document.addListener([&] { ++notified; });
    ToolHost host(document);
    ASSERT_TRUE(host.start("select.similar").ok());
    EXPECT_EQ(host.enter(), ToolHost::Outcome::Done);
    // The three lines on layer 0, not the circle.
    EXPECT_EQ(document.selection().ids(), lines);
    EXPECT_FALSE(document.selection().contains(circle));
    EXPECT_GE(notified, 1);
    EXPECT_EQ(document.history().undoCount(), 4u) << "no command for a selection";
}


TEST(ToolHost, ASnapOnAnEntitysPointReachesTheToolAsThatPoint)
{
    // A leader's tip snapped to a line's end follows the line's End. One
    // snapped to somewhere along it (Nearest) names no point of the line, so
    // the snap hands the tool a plain point - and the Leader puts a tip on
    // what it is clicked on (docs/annotation.md, "Smart leaders"): it goes on
    // the line at the place nearest, 4 of the line's 10 along, and follows
    // it there.
    Document document;
    ASSERT_TRUE(
        document.execute(katana::commands::createLine(Point2(0, 0), Point2(10, 0))).ok());
    const auto line = document.lastCreatedEntities().front();
    ToolHost host(document);
    const auto leader = [&](const katana::cad::SnapResult& snap) {
        EXPECT_TRUE(host.start("annotate.leader").ok());
        (void)host.point(snap.point, snap);
        (void)host.point(Point2(15, 5));
        (void)host.enter();
        (void)host.enter();
        host.cancel();
        const auto made = document.lastCreatedEntities();
        EXPECT_EQ(made.size(), 1u);
        return std::get<katana::entity::LeaderGeometry>(
            document.model().entities.find(made.front())->geometry);
    };
    const auto snapped =
        leader(katana::cad::SnapResult{Point2(10, 0), katana::cad::SnapMode::Endpoint, line});
    EXPECT_EQ(snapped.tipRef,
              (katana::entity::AnchorRef{line, katana::entity::AnchorPoint::End, 0}));
    const auto along =
        leader(katana::cad::SnapResult{Point2(4, 0), katana::cad::SnapMode::Nearest, line});
    EXPECT_EQ(along.tipRef.entity, line);
    EXPECT_EQ(along.tipRef.point, katana::entity::AnchorPoint::Along);
    EXPECT_DOUBLE_EQ(along.tipRef.parameter, 0.4);
    EXPECT_EQ(along.vertices.front(), Point2(4, 0));
}

TEST(ToolHost, AClickTheToolTookIsNotAnsweredInRedUntilThePointerLeavesIt)
{
    // Insert Vertex restarts after each vertex with the cursor on the vertex
    // it made, where a second click WOULD be refused as too close - and the
    // view said so in red straight after every success. Until the cursor
    // leaves the click's place, what the click did is shown instead.
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createPolyline(
                        katana::geometry::Polyline2{{Point2(0, 0), Point2(10, 0)}, false}))
                    .ok());
    const auto p = document.lastCreatedEntities().front();
    document.selection().add(p);
    ToolHost host(document);
    host.setPickTolerance(0.5);
    ASSERT_TRUE(host.start("draw.vertex.insert").ok());
    host.setCursor(Point2(4, 0.1));
    ASSERT_EQ(host.point(Point2(4, 0.1)), ToolHost::Outcome::Done);
    const auto held = host.feedback(Point2(4, 0.1));
    EXPECT_FALSE(held.refused);
    EXPECT_TRUE(held.marks.empty());
    EXPECT_EQ(held.caption,
              "vertex 1 added to polyline " + std::to_string(p) + " on segment 0 (3 vertices)");
    EXPECT_EQ(held.focus, p);
    // Away - 4 units, beyond the 0.5 aperture - and back: now the tool's
    // refusal is what a click there would get, and it is shown.
    host.setCursor(Point2(8, 0.1));
    EXPECT_FALSE(host.feedback(Point2(8, 0.1)).refused) << "8 is clear of every vertex";
    host.setCursor(Point2(4, 0.1));
    const auto refused = host.feedback(Point2(4, 0.1));
    EXPECT_TRUE(refused.refused);
    EXPECT_EQ(refused.caption.rfind("too close to vertex 1", 0), 0u) << refused.caption;
    // A click the tool refuses is no success to spare: its refusal shows.
    EXPECT_EQ(host.point(Point2(4, 0.1)), ToolHost::Outcome::Rejected);
    EXPECT_TRUE(host.feedback(Point2(4, 0.1)).refused);
}
