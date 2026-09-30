// What every tool's preview can say and where a tool picks
// (include/katana/cad/tool_feedback.hpp, interactive_tool.hpp): the roles'
// names, the pick through the view or the model restricted to the kinds a
// tool asks for, and the reaches that stand in for the view's apertures in a
// test.

#include <gtest/gtest.h>

#include <string>

#include "katana/cad/interactive_tool.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::cad::Document;
using katana::cad::FeedbackRole;
using katana::cad::ToolContext;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

TEST(ToolFeedback, FeedbackRolesHaveStableNames)
{
    // The headless pointer record's keys are these (viewport_widget.cpp,
    // pointerAt; docs/headless.md): a test matching "target=1 added=1"
    // must not break on a rename.
    EXPECT_EQ(std::string(katana::cad::toString(FeedbackRole::Target)), "target");
    EXPECT_EQ(std::string(katana::cad::toString(FeedbackRole::Added)), "added");
    EXPECT_EQ(std::string(katana::cad::toString(FeedbackRole::Removed)), "removed");
    EXPECT_EQ(std::string(katana::cad::toString(FeedbackRole::Enter)), "enter");
}

TEST(ToolFeedback, PickUnderTakesOnlyTheTypesAsked)
{
    Document document;
    ASSERT_TRUE(document
                    .execute(katana::commands::createPolyline(
                        Polyline2{{Point2(0, 0), Point2(10, 0)}, false}))
                    .ok());
    const EntityId polyline = document.lastCreatedEntities().front();
    ASSERT_TRUE(document
                    .execute(katana::commands::createLine(Point2(0, 0), Point2(10, 0),
                                                          document.currentAttributes()))
                    .ok());
    const EntityId line = document.lastCreatedEntities().front();
    ASSERT_GT(line, polyline);

    ToolContext context;
    context.document = &document;
    context.pickTolerance = 0.5;
    // Both lie under (5,0) at distance 0; a tie goes to the higher id, the
    // one drawn on top (selection.hpp) - unless only polylines are asked for.
    EXPECT_EQ(katana::cad::pickUnder(context, Point2(5, 0), {}), line);
    EXPECT_EQ(katana::cad::pickUnder(context, Point2(5, 0),
                                     {EntityType::Polyline, EntityType::CurvePolyline}),
              polyline);
    // Out of reach: 0.6 from both, with a reach of 0.5.
    EXPECT_FALSE(katana::cad::pickUnder(context, Point2(5, 0.6), {}).has_value());
    // A reach given explicitly is used instead of the tool's.
    EXPECT_EQ(katana::cad::pickUnder(context, Point2(5, 0.6),
                                     {EntityType::Polyline, EntityType::CurvePolyline}, 0.75),
              polyline);
}

TEST(ToolFeedback, AViewsPickIsUsedWhenTheContextHasOne)
{
    Document document;
    ToolContext context;
    context.document = &document;
    context.pickTolerance = 0.5;
    double reachAsked = 0.0;
    context.pick = [&reachAsked](const Point2&, double reach,
                                 const std::set<EntityType>&) -> std::optional<EntityId> {
        reachAsked = reach;
        return EntityId{42};
    };
    // The view's answer, even with nothing in the model: it is the view that
    // knows its hidden layers.
    EXPECT_EQ(katana::cad::pickUnder(context, Point2(0, 0), {}), EntityId{42});
    EXPECT_DOUBLE_EQ(reachAsked, 0.5);
}

TEST(ToolFeedback, ReachFallsBackToTheToolsAperture)
{
    ToolContext context;
    context.pickTolerance = 0.5;
    EXPECT_DOUBLE_EQ(katana::cad::pickReach(context), 0.5);
    // 12 px over 8 px, the view's snap aperture over its pick aperture.
    EXPECT_DOUBLE_EQ(katana::cad::vertexReach(context), 0.75);
    // The view's own, read now, win over the tolerance the tool was made with.
    context.pickAperture = [] { return 2.0; };
    context.vertexAperture = [] { return 3.0; };
    EXPECT_DOUBLE_EQ(katana::cad::pickReach(context), 2.0);
    EXPECT_DOUBLE_EQ(katana::cad::vertexReach(context), 3.0);
}
