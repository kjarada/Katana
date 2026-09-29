#pragma once

// Drives an interactive tool the way the plan view does, so a tool is tested
// by the inputs a user would give it: start it from the catalogue, click,
// pick, type, press Enter - and a finished step's command is executed on the
// document, exactly as the view does. Assert on the document afterwards
// against geometry worked out by hand.

#include <memory>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/cad/snapping.hpp"

namespace katana::cad::testing {

class ToolDriver {
  public:
    ToolDriver() = default;
    ToolDriver(const ToolDriver&) = delete;
    ToolDriver& operator=(const ToolDriver&) = delete;

    // Starts catalogue tool `id` on the current selection (select entities in
    // document() first for a Modify tool). Fails the test if there is no such
    // tool.
    void start(std::string_view id) { start(id, {}); }
    // The same with the grips hot when the tool starts (ToolContext::handles):
    // given to the first make only, as the tool host gives them, so a restart
    // after an edit sees none.
    void start(std::string_view id, std::vector<Grip> handles)
    {
        const ToolInfo* info = toolCatalog().find(id);
        ASSERT_NE(info, nullptr) << "no tool " << id;
        info_ = info;
        handles_ = std::move(handles);
        restartTool();
    }
    // The pick aperture a tool is made with (ToolContext::pickTolerance),
    // 0.01 unless set - before start(). The vertex tools reach a vertex
    // within 1.5 times it and a segment within it, as the view's 12 and 8 px.
    void setPickTolerance(double tolerance) { pickTolerance_ = tolerance; }

    ToolStep click(double x, double y) { return apply(tool_->point({x, y})); }
    // What the view would draw with the cursor at (x, y).
    [[nodiscard]] ToolFeedback preview(double x, double y) const { return tool_->preview({x, y}); }
    // A click the plan view would snap: to the nearest end, middle, centre
    // or intersection (the default modes) within 0.5 of (x, y), handed on
    // as the view hands it (routeSnappedPoint) - a point of an entity
    // reaches the tool as that point. Nothing within 0.5 is a plain click.
    ToolStep clickSnapped(double x, double y)
    {
        SnapRequest request;
        request.cursor = {x, y};
        request.aperture = 0.5;
        const auto found = snap(document_.model(), request);
        const katana::geometry::Point2 at = found ? found->point : katana::geometry::Point2(x, y);
        return apply(routeSnappedPoint(*tool_, document_, at, found));
    }
    ToolStep pick(katana::entity::EntityId id, double x, double y)
    {
        return apply(tool_->entity(id, {x, y}));
    }
    // Typed text, through the same routing the command line uses.
    ToolStep type(std::string_view text) { return apply(routeTypedInput(*tool_, text)); }
    ToolStep enter() { return apply(tool_->enter()); }
    ToolStep undo() { return apply(tool_->undo()); }
    // Esc, as the tool host has it: what the tool's cancel() keeps is
    // executed, and the tool ends whatever it answers.
    ToolStep cancel()
    {
        ToolStep step = tool_->cancel();
        step.restart = false;
        ToolStep applied = apply(std::move(step));
        finished_ = true;
        return applied;
    }

    [[nodiscard]] Document& document() { return document_; }
    [[nodiscard]] InteractiveTool& tool() { return *tool_; }
    // Messages the tool reported, in order.
    [[nodiscard]] const std::vector<std::string>& messages() const { return messages_; }
    // Commands executed since start().
    [[nodiscard]] int executed() const { return executed_; }
    // True once the tool finished and did not ask to restart.
    [[nodiscard]] bool finished() const { return finished_; }

    // Adds an entity by command and returns its id - for setting up a Modify
    // tool's input.
    katana::entity::EntityId add(katana::commands::CommandPtr command)
    {
        EXPECT_TRUE(document_.execute(std::move(command)).ok());
        const auto created = document_.lastCreatedEntities();
        EXPECT_EQ(created.size(), 1u);
        return created.empty() ? katana::entity::kInvalidEntityId : created.front();
    }

  private:
    void restartTool()
    {
        ToolContext context;
        context.document = &document_;
        context.attributes = document_.currentAttributes();
        context.selection = document_.selection().ids();
        context.pickTolerance = pickTolerance_;
        context.handles = std::exchange(handles_, {});
        tool_ = info_->make(context);
        finished_ = false;
    }

    ToolStep apply(ToolStep step)
    {
        if (!step.message.empty()) {
            messages_.push_back(step.message);
        }
        if (step.outcome == ToolStep::Outcome::Done) {
            if (step.command) {
                const auto status = document_.execute(std::move(step.command));
                EXPECT_TRUE(status.ok()) << status.error().describe();
                ++executed_;
            }
            if (step.selection) {
                // As the tool host leaves a selecting tool's answer.
                document_.selection().set(*step.selection);
            }
            if (step.restart) {
                restartTool();
            } else {
                finished_ = true;
            }
        }
        return step;
    }

    Document document_;
    const ToolInfo* info_ = nullptr;
    std::unique_ptr<InteractiveTool> tool_;
    std::vector<Grip> handles_;
    double pickTolerance_ = 0.01;
    std::vector<std::string> messages_;
    int executed_ = 0;
    bool finished_ = false;
};

} // namespace katana::cad::testing
