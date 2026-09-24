#include "katana/cad/interactive_tool.hpp"

#include <algorithm>
#include <tuple>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "tools/families.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;

const char* toString(ToolInput input)
{
    switch (input) {
    case ToolInput::Point:
        return "point";
    case ToolInput::Entity:
        return "entity";
    case ToolInput::Selection:
        return "selection";
    case ToolInput::Value:
        return "value";
    }
    return "unknown";
}

ToolStep ToolStep::next(std::string message)
{
    ToolStep step;
    step.outcome = Outcome::Continue;
    step.message = std::move(message);
    return step;
}

ToolStep ToolStep::rejected(std::string why)
{
    ToolStep step;
    step.outcome = Outcome::Rejected;
    step.message = std::move(why);
    return step;
}

ToolStep ToolStep::done(katana::commands::CommandPtr command, std::string message, bool restart)
{
    ToolStep step;
    step.outcome = Outcome::Done;
    step.command = std::move(command);
    step.message = std::move(message);
    step.restart = restart;
    return step;
}

// ---- the defaults ------------------------------------------------------------------
//
// Each names what the tool wants instead, from expects(), so a refused click
// tells the user what to do rather than only that it was wrong.

ToolStep InteractiveTool::point(const Point2& /*at*/)
{
    return ToolStep::rejected(std::string("a point is not expected here; the tool wants ") +
                              toString(expects()));
}

ToolStep InteractiveTool::entity(katana::entity::EntityId /*id*/, const Point2& /*at*/)
{
    return ToolStep::rejected(std::string("an entity is not expected here; the tool wants ") +
                              toString(expects()));
}

ToolStep InteractiveTool::value(std::string_view text)
{
    return ToolStep::rejected("'" + std::string(text) + "' is not an input this tool takes now");
}

ToolStep InteractiveTool::enter() { return ToolStep::done(nullptr); }

ToolStep InteractiveTool::undo() { return ToolStep::rejected("nothing to undo in this tool"); }

ToolStep InteractiveTool::cancel() { return ToolStep::done(nullptr); }

ToolFeedback InteractiveTool::preview(const Point2& /*cursor*/) const { return {}; }

std::optional<Point2> InteractiveTool::lastPoint() const { return std::nullopt; }

namespace tools {

ToolStep keepWorkOnEscape(InteractiveTool& tool)
{
    // Out of a value prompt first. Enter there takes the prompt's default -
    // at Chamfer's second distance it stores both distances for every later
    // Chamfer - or only returns to the picks (Fillet's radius), and then the
    // corners a Multiple session made would go with the tool. Undo at those
    // prompts only steps back towards the picks (CornerTool::undo); a value
    // prompt still showing after that is left rather than defaulted.
    constexpr int kMostStepsBack = 4; // SecondDistance -> FirstDistance -> First is 2
    for (int stepped = 0; stepped < kMostStepsBack && tool.expects() == ToolInput::Value;
         ++stepped) {
        if (tool.undo().outcome != ToolStep::Outcome::Continue) {
            break;
        }
    }
    if (tool.expects() == ToolInput::Value) {
        return ToolStep::done(nullptr);
    }
    ToolStep step = tool.enter();
    // Only a finished step's command is work to keep. A Continue is Enter
    // moving the tool on a step (Trim's edges, Offset's distance) and a
    // Rejected changed nothing; either way there is nothing to keep.
    if (step.outcome != ToolStep::Outcome::Done) {
        return ToolStep::done(nullptr);
    }
    step.restart = false;
    return step;
}

} // namespace tools

// ---- typed input -------------------------------------------------------------------

Result<Point2> parsePointInput(std::string_view text, std::optional<Point2> last)
{
    const std::string_view input = katana::core::trimmed(text);
    const bool relative = !input.empty() && input.front() == '@';
    const std::string_view body = relative ? input.substr(1) : input;
    if (relative && !last) {
        return makeError(ErrorCode::InvalidState, "a relative point needs a previous point",
                         std::string(text));
    }

    katana::math::Vec2 value;
    if (const auto polar = body.find('<'); polar != std::string_view::npos) {
        if (!relative) {
            return makeError(ErrorCode::ParseFailure, "polar points are relative: @distance<angle",
                             std::string(text));
        }
        const auto distance = katana::core::parseFiniteDouble(
            katana::core::trimmed(body.substr(0, polar)));
        const auto degrees = katana::core::parseFiniteDouble(
            katana::core::trimmed(body.substr(polar + 1)));
        if (!distance || !degrees) {
            return makeError(ErrorCode::ParseFailure, "expected @distance<angle",
                             std::string(text));
        }
        value = katana::math::Vec2(*distance, 0.0).rotated(*degrees * katana::math::kDegToRad);
    } else {
        const auto comma = body.find(',');
        if (comma == std::string_view::npos) {
            return makeError(ErrorCode::ParseFailure, "expected a point as x,y", std::string(text));
        }
        const auto x = katana::core::parseFiniteDouble(katana::core::trimmed(body.substr(0, comma)));
        const auto y =
            katana::core::parseFiniteDouble(katana::core::trimmed(body.substr(comma + 1)));
        if (!x || !y) {
            return makeError(ErrorCode::ParseFailure, "expected a point as x,y", std::string(text));
        }
        value = katana::math::Vec2(*x, *y);
    }
    return relative ? *last + value : Point2(value);
}

ToolStep routeTypedInput(InteractiveTool& tool, std::string_view text)
{
    const std::string_view input = katana::core::trimmed(text);
    // A point only when it LOOKS like one - a comma, or the @ of relative
    // input. "12.5" is a distance or a radius, and must reach value() rather
    // than being refused as a malformed point.
    const bool looksLikePoint =
        tool.expects() != ToolInput::Value &&
        (input.find(',') != std::string_view::npos || (!input.empty() && input.front() == '@'));
    if (looksLikePoint) {
        auto point = parsePointInput(input, tool.lastPoint());
        if (!point) {
            return ToolStep::rejected(point.error().describe());
        }
        return tool.point(*point);
    }
    return tool.value(input);
}

// ---- the catalogue -----------------------------------------------------------------

Status ToolCatalog::add(ToolInfo info)
{
    if (info.id.empty() || info.name.empty() || info.category.empty() || info.group.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a tool needs an id, a name, a category and a group", info.id);
    }
    if (!info.make) {
        return makeError(ErrorCode::InvalidArgument, "a tool needs a factory", info.id);
    }
    // One visible character is a letter typed into the view, which goes to the
    // command line - a shortcut there would steal it.
    if (info.shortcut.size() == 1) {
        return makeError(ErrorCode::InvalidArgument,
                         "a single-key shortcut would steal a letter typed into the view",
                         info.id + " " + info.shortcut);
    }
    for (const std::string& alias : info.aliases) {
        const bool hasLower =
            std::ranges::any_of(alias, [](char c) { return c >= 'a' && c <= 'z'; });
        if (alias.empty() || hasLower) {
            return makeError(ErrorCode::InvalidArgument, "an alias is a non-empty upper-case verb",
                             info.id + " '" + alias + "'");
        }
        if (findByAlias(alias) != nullptr) {
            return makeError(ErrorCode::AlreadyExists, "the alias is taken by another tool",
                             info.id + " " + alias + " (" + findByAlias(alias)->id + ")");
        }
    }
    if (find(info.id) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "a tool with this id already exists", info.id);
    }
    tools_.push_back(std::move(info));
    return {};
}

const ToolInfo* ToolCatalog::find(std::string_view id) const
{
    const auto found =
        std::ranges::find_if(tools_, [&](const ToolInfo& tool) { return tool.id == id; });
    return found == tools_.end() ? nullptr : &*found;
}

const ToolInfo* ToolCatalog::findByAlias(std::string_view verb) const
{
    for (const ToolInfo& tool : tools_) {
        for (const std::string& alias : tool.aliases) {
            if (katana::core::equalsIgnoringCase(alias, verb)) {
                return &tool;
            }
        }
    }
    return nullptr;
}

std::vector<const ToolInfo*> ToolCatalog::all() const
{
    std::vector<const ToolInfo*> out;
    out.reserve(tools_.size());
    for (const ToolInfo& tool : tools_) {
        out.push_back(&tool);
    }
    std::ranges::sort(out, [](const ToolInfo* a, const ToolInfo* b) {
        return std::tie(a->category, a->group, a->order, a->id) <
               std::tie(b->category, b->group, b->order, b->id);
    });
    return out;
}

namespace {

struct BuiltCatalog {
    ToolCatalog catalog;
    std::vector<std::string> problems;
};

const BuiltCatalog& builtCatalog()
{
    // Function-local, so it is built on first use and not raced against other
    // static initialisers. Each family adds its own tools; a refusal is kept,
    // not thrown, and a test asserts there are none.
    static const BuiltCatalog built = [] {
        BuiltCatalog out;
        const auto report = [&](Status status) {
            if (!status) {
                out.problems.push_back(status.error().describe());
            }
        };
        tools::addDrawLineTools(out.catalog, report);
        tools::addDrawCurveTools(out.catalog, report);
        tools::addModifyTransformTools(out.catalog, report);
        tools::addModifyEditTools(out.catalog, report);
        tools::addAnnotateTools(out.catalog, report);
        tools::addInquiryTools(out.catalog, report);
        tools::addDrawDivideTools(out.catalog, report);
        tools::addModifyLengthTools(out.catalog, report);
        tools::addPropertyTools(out.catalog, report);
        tools::addSelectTools(out.catalog, report);
        return out;
    }();
    return built;
}

} // namespace

const ToolCatalog& toolCatalog() { return builtCatalog().catalog; }

const std::vector<std::string>& toolCatalogProblems() { return builtCatalog().problems; }

} // namespace katana::cad
