#pragma once

// Drives an interactive tool the way the plan view does, so a tool is tested
// by the inputs a user would give it: start it from the catalogue, click,
// pick, type, press Enter - and a finished step's command is executed on the
// document, exactly as the view does. Assert on the document afterwards
// against geometry worked out by hand.

#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"

namespace katana::cad::testing {

class ToolDriver {
  public:
    ToolDriver() = default;
    ToolDriver(const ToolDriver&) = delete;
    ToolDriver& operator=(const ToolDriver&) = delete;

    // Starts catalogue tool `id` on the current selection (select entities in
    // document() first for a Modify tool). Fails the test if there is no such
    // tool.
    void start(std::string_view id)
    {
        const ToolInfo* info = toolCatalog().find(id);
        ASSERT_NE(info, nullptr) << "no tool " << id;
        info_ = info;
        restartTool();
    }

    ToolStep click(double x, double y) { return apply(tool_->point({x, y})); }
    ToolStep pick(katana::entity::EntityId id, double x, double y)
    {
        return apply(tool_->entity(id, {x, y}));
    }
    // Typed text, through the same routing the command line uses.
    ToolStep type(std::string_view text) { return apply(routeTypedInput(*tool_, text)); }
    ToolStep enter() { return apply(tool_->enter()); }
    ToolStep undo() { return apply(tool_->undo()); }

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
        context.pickTolerance = 0.01;
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
    std::vector<std::string> messages_;
    int executed_ = 0;
    bool finished_ = false;
};

} // namespace katana::cad::testing
