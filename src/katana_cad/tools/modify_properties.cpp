// Match Properties (see families.hpp): AutoCAD's MATCHPROP. Pick the object
// whose properties are wanted, then the objects to give them to, and Enter.
//
// An entity here carries three presentation properties of its own - its
// layer, its style and its colour - and those are what is copied. A linetype
// and a line weight are not an entity's own: they come from its style, or
// from its layer when it is ByLayer (entity.hpp, tables.hpp), so copying the
// style and the layer copies them, and nothing else could. [Settings] chooses
// which of the three go across, and is remembered for the next use, as
// AutoCAD remembers its settings. Everything matched is ONE command, so one
// undo puts every target back.

#include <algorithm>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "everyday_support.hpp"
#include "families.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/core/text.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;
using everyday::counted;
using everyday::SelectionStep;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using modify_edit::isOption;

struct MatchSettings {
    bool layer = true;
    bool colour = true;
    bool style = true;

    // "layer, colour and style", as the prompt and the report say it.
    [[nodiscard]] std::string words() const
    {
        std::vector<std::string> parts;
        if (layer) {
            parts.emplace_back("layer");
        }
        if (colour) {
            parts.emplace_back("colour");
        }
        if (style) {
            parts.emplace_back("style");
        }
        std::string text;
        for (std::size_t i = 0; i < parts.size(); ++i) {
            text += (i == 0 ? "" : i + 1 == parts.size() ? " and " : ", ") + parts[i];
        }
        return text;
    }
};

class MatchPropertiesTool final : public InteractiveTool {
  public:
    MatchPropertiesTool(const ToolContext& context, std::shared_ptr<MatchSettings> settings)
        : document_(context.document), selection_(context.document),
          settings_(std::move(settings))
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Source:
            return "Select the source object";
        case Step::Targets:
            return "Select destination objects or [Settings], then press Enter";
        case Step::Settings:
            return "Properties to copy: any of [Layer/Colour/Style], or All <" +
                   settings_->words() + ">";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        switch (step_) {
        case Step::Source:
            return ToolInput::Entity;
        case Step::Targets:
            return ToolInput::Selection;
        case Step::Settings:
            return ToolInput::Value;
        }
        return ToolInput::Entity;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ == Step::Targets) {
            return selection_.pick(id);
        }
        if (step_ != Step::Source) {
            return InteractiveTool::entity(id, at);
        }
        const Entity* entity =
            document_ != nullptr ? document_->model().entities.find(id) : nullptr;
        if (entity == nullptr) {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        source_ = *entity;
        step_ = Step::Targets;
        return ToolStep::next("Source: layer " + source_.layer + ", style " +
                              (source_.style.empty() ? std::string("ByLayer") : source_.style) +
                              ", colour " +
                              (source_.color ? source_.color->toHex() : std::string("ByLayer")));
    }

    ToolStep value(std::string_view text) override
    {
        if (step_ == Step::Settings) {
            return chooseSettings(text);
        }
        if (step_ == Step::Targets) {
            if (isOption(text, "Settings", "S")) {
                step_ = Step::Settings;
                return ToolStep::next();
            }
            if (isOption(text, "All", "A")) {
                return selection_.all();
            }
        }
        return ToolStep::rejected("'" + std::string(text) + "' is not an option here");
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Source:
            return ToolStep::done(nullptr);
        case Step::Settings:
            step_ = Step::Targets; // the settings stay as they are
            return ToolStep::next();
        case Step::Targets:
            break;
        }
        return apply();
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Settings:
            step_ = Step::Targets;
            return ToolStep::next();
        case Step::Targets:
            if (selection_.undo()) {
                return ToolStep::next();
            }
            step_ = Step::Source;
            return ToolStep::next();
        case Step::Source:
            break;
        }
        return ToolStep::rejected("nothing to undo in this tool");
    }

  private:
    enum class Step { Source, Targets, Settings };

    ToolStep chooseSettings(std::string_view text)
    {
        MatchSettings chosen{false, false, false};
        std::string words(text);
        std::ranges::replace(words, ',', ' ');
        std::size_t at = 0;
        bool any = false;
        while (at < words.size()) {
            const std::size_t end = std::min(words.find(' ', at), words.size());
            const std::string_view word = std::string_view(words).substr(at, end - at);
            at = end + 1;
            if (word.empty()) {
                continue;
            }
            any = true;
            if (isOption(word, "Layer", "L")) {
                chosen.layer = true;
            } else if (isOption(word, "Colour", "C") || isOption(word, "Color", "C")) {
                chosen.colour = true;
            } else if (isOption(word, "Style", "S")) {
                chosen.style = true;
            } else if (isOption(word, "All", "A")) {
                chosen = MatchSettings{};
            } else {
                return ToolStep::rejected("'" + std::string(word) +
                                          "' is not a property here: Layer, Colour, Style or All");
            }
        }
        if (!any || !(chosen.layer || chosen.colour || chosen.style)) {
            return ToolStep::rejected("name at least one of Layer, Colour and Style");
        }
        *settings_ = chosen;
        step_ = Step::Targets;
        return ToolStep::next("Copying " + settings_->words());
    }

    ToolStep apply()
    {
        std::vector<EntityId> ids = selection_.chosen();
        std::erase(ids, source_.id);
        if (ids.empty()) {
            return ToolStep::rejected(
                "nothing is selected: select the objects to match, then press Enter");
        }
        if (settings_->layer) {
            // Moving entities onto a locked layer is refused by the command;
            // said here, before anything is picked in vain.
            if (auto refusal = modify_edit::refusalToEdit(*document_, source_.id)) {
                return ToolStep::rejected("the source's layer '" + source_.layer +
                                          "' is locked, so nothing can be moved onto it");
            }
        }
        cmd::ChangeSet changes;
        std::size_t locked = 0;
        for (const EntityId id : ids) {
            const Entity* target = document_->model().entities.find(id);
            if (target == nullptr) {
                continue;
            }
            if (modify_edit::refusalToEdit(*document_, id)) {
                ++locked;
                continue;
            }
            Entity matched = *target;
            if (settings_->layer) {
                matched.layer = source_.layer;
            }
            if (settings_->style) {
                matched.style = source_.style;
            }
            if (settings_->colour) {
                matched.color = source_.color;
            }
            if (matched != *target) {
                changes.modify.push_back(std::move(matched));
            }
        }
        const std::string note =
            locked == 0 ? std::string()
                        : "; " + std::to_string(locked) + " on a locked layer left as it is";
        if (changes.modify.empty()) {
            return ToolStep::done(nullptr, "Nothing to change: the selected objects already "
                                           "have the source's " +
                                               settings_->words() + note);
        }
        const std::size_t count = changes.modify.size();
        auto command = std::make_unique<cmd::ChangeSetCommand>(
            "MATCHPROP", [changes = std::move(changes)](const cmd::CommandContext&) {
                return Result<cmd::ChangeSet>(changes);
            });
        return ToolStep::done(std::move(command), "Matched " + settings_->words() + " on " +
                                                      counted(count, "object", "objects") + note);
    }

    const Document* document_ = nullptr;
    SelectionStep selection_;
    std::shared_ptr<MatchSettings> settings_;
    Step step_ = Step::Source;
    Entity source_;
};

} // namespace

void addPropertyTools(ToolCatalog& catalog, const Report& report)
{
    auto settings = std::make_shared<MatchSettings>();
    ToolInfo info;
    info.id = "modify.match_properties";
    info.name = "Match Properties";
    info.category = "Modify";
    info.group = "Properties";
    info.order = 10;
    info.aliases = {"MATCHPROP", "MA", "PAINTER"};
    info.tip = "Gives objects the layer, colour and style of a source object - and with the "
               "style and the layer their linetype and weight; S chooses which are copied.";
    info.make = [settings](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<MatchPropertiesTool>(context, settings);
    };
    report(catalog.add(std::move(info)));
}

} // namespace katana::cad::tools
