// Label Objects: labels placed by hand, as the LABEL verb places them.
//
//   Select objects to label or [Style/Part/Text], then press Enter
//                                  picks, or the selection the tool started
//                                  with; Enter with nothing chosen ends it
//   Specify text location or [Style/Part/Text/Undo] <automatic>
//                                  a point pins the text there (LABEL's at=);
//                                  Enter leaves it to the placer
//
// Style names the label style, Part a polyline's segment (from 0; Enter for
// every segment) and Text the label's own words in place of its style's.
// The style offered first is the one the drawing's newest hand-placed label
// wears - the last one used, remembered by the drawing rather than by the
// program, so it is the same after a reopen, as the Text tool offers the
// newest text's height - when it can label what was chosen, and otherwise
// the first style by name that can.
//
// The labels are made by annotation::createLabels, the LABEL verb's own door,
// as ONE command: "LABEL 12 14 style=... part=... at=x,y text=..." typed and
// this tool given the same answers make the same entities. Several objects
// cannot share one place, so with more than one the location prompt takes
// only Enter. When the labels are made the selection is cleared, so the tool
// starts again asking for the next objects rather than labelling the same
// ones twice at the next Enter.

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/auto_label.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/label_values.hpp"

namespace katana::cad::tools::annotate {

namespace {

namespace ann = katana::cad::annotation;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::LabelGeometry;
using katana::entity::LabelStyle;
using katana::entity::Model;
using katana::geometry::Point2;

// The style of the newest label placed by hand whose style still exists;
// empty when there is none. The entities are visited in id order, which is
// the order they were made.
std::string newestHandLabelStyle(const Model& model)
{
    std::string style;
    model.entities.forEach([&](const Entity& entity) {
        const auto* label = std::get_if<LabelGeometry>(&entity.geometry);
        if (label != nullptr && label->rule.empty() && model.labelStyles.contains(label->style)) {
            style = label->style;
        }
    });
    return style;
}

bool styleLabels(const Model& model, const std::string& name, const Entity& target)
{
    const LabelStyle* style = model.labelStyles.find(name);
    return style != nullptr && katana::entity::labels(style->kind, target.geometry);
}

class LabelTool final : public InteractiveTool {
  public:
    explicit LabelTool(const ToolContext& context) : document_(context.document)
    {
        if (context.selection.empty()) {
            step_ = Step::Select;
            asked_ = true;
        } else {
            targets_ = context.selection;
            step_ = Step::Location;
        }
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Select:
            return "Select objects to label or [Style/Part/Text], then press Enter";
        case Step::Location:
            if (targets_.size() == 1) {
                return "Specify text location or [Style/Part/Text/Undo] <automatic>";
            }
            return "Press Enter to label the " + std::to_string(targets_.size()) +
                   " objects, each placed automatically, or [Style/Part/Text/Undo]";
        case Step::Style: {
            const std::string style = currentStyle();
            return style.empty() ? "Enter label style name (the drawing has none that fits: "
                                   "LABELSTYLE DEFAULTS adds the standard set)"
                                 : "Enter label style name <" + style + ">";
        }
        case Step::Part:
            return "Enter the polyline segment to label, 0 for the first, or [All] <" +
                   (settings_.part < 0 ? std::string("all") : std::to_string(settings_.part)) +
                   ">";
        case Step::Text:
            return settings_.text.empty()
                       ? "Enter the label's own text, or press Enter for its style's"
                       : "Enter the label's own text, or press Enter for its style's <" +
                             settings_.text + ">";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        switch (step_) {
        case Step::Select:
            return ToolInput::Selection;
        case Step::Location:
            return ToolInput::Point;
        case Step::Style:
        case Step::Part:
        case Step::Text:
            break;
        }
        return ToolInput::Value;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Select:
            return ToolStep::rejected("select the objects to label first, then press Enter");
        case Step::Location:
            if (targets_.size() != 1) {
                return ToolStep::rejected(std::to_string(targets_.size()) +
                                          " labels cannot share one place; press Enter to place "
                                          "each automatically");
            }
            return finish(at);
        case Step::Style:
        case Step::Part:
        case Step::Text:
            break;
        }
        return InteractiveTool::point(at);
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Select) {
            return InteractiveTool::entity(id, at);
        }
        if (document_ == nullptr || !document_->model().entities.contains(id)) {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        if (std::ranges::find(picked_, id) != picked_.end()) {
            return ToolStep::rejected("that entity is already chosen");
        }
        pickHistory_.push_back(picked_);
        picked_.push_back(id);
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        switch (step_) {
        case Step::Select:
        case Step::Location:
            if (step_ == Step::Location && isOption(text, "Undo")) {
                return undo();
            }
            if (isOption(text, "Style")) {
                return ask(Step::Style);
            }
            if (isOption(text, "Part")) {
                return ask(Step::Part);
            }
            if (isOption(text, "Text")) {
                return ask(Step::Text);
            }
            return ToolStep::rejected(step_ == Step::Select
                                          ? "pick the objects to label, or type S, P or T"
                                          : "click where the text goes, press Enter to place it "
                                            "automatically, or type S, P or T");
        case Step::Style:
            return styleValue(text);
        case Step::Part:
            return partValue(text);
        case Step::Text:
            if (text.empty()) {
                return enter();
            }
            if (!katana::entity::isValidUtf8(text)) {
                return ToolStep::rejected("the text is not valid UTF-8");
            }
            remember();
            settings_.text = std::string(text);
            step_ = returnTo_;
            return ToolStep::next();
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Select:
            return chooseTargets();
        case Step::Location:
            return finish(std::nullopt);
        case Step::Style:
        case Step::Part:
            step_ = returnTo_;
            return ToolStep::next();
        case Step::Text:
            // Back to the style's own words, as the dimension tools' text
            // prompt goes back to the measured value.
            if (!settings_.text.empty()) {
                remember();
                settings_.text.clear();
            }
            step_ = returnTo_;
            return ToolStep::next();
        }
        return InteractiveTool::enter();
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Select:
            if (pickHistory_.empty()) {
                return ToolStep::rejected("nothing to undo: nothing has been picked");
            }
            picked_ = std::move(pickHistory_.back());
            pickHistory_.pop_back();
            return ToolStep::next();
        case Step::Location:
            if (!history_.empty()) {
                settings_ = history_.back();
                history_.pop_back();
                return ToolStep::next();
            }
            if (asked_) {
                // Back to choosing, with the picks kept.
                targets_.clear();
                step_ = Step::Select;
                return ToolStep::next();
            }
            return ToolStep::rejected("nothing to undo in this tool");
        case Step::Style:
        case Step::Part:
        case Step::Text:
            step_ = returnTo_;
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (step_ == Step::Location && targets_.size() == 1) {
            feedback.markers.push_back(cursor);
        }
        return feedback;
    }

  private:
    enum class Step { Select, Location, Style, Part, Text };

    // What the options have been told. Each option pushes the settings it
    // replaces, so Undo at the location takes back the last option first.
    struct Settings {
        std::string style; // empty: the one offered (currentStyle)
        std::int32_t part = -1;
        std::string text;
    };

    ToolStep ask(Step option)
    {
        returnTo_ = step_;
        step_ = option;
        return ToolStep::next();
    }

    void remember() { history_.push_back(settings_); }

    // The style the labels will be made in: the one typed, else the newest
    // hand-placed label's when it can label the first object, else the first
    // by name that can. Empty when the drawing has none.
    [[nodiscard]] std::string currentStyle() const
    {
        if (!settings_.style.empty() || document_ == nullptr) {
            return settings_.style;
        }
        const Model& model = document_->model();
        const Entity* first = targets_.empty() ? nullptr : model.entities.find(targets_.front());
        const std::string newest = newestHandLabelStyle(model);
        if (first == nullptr) {
            if (!newest.empty()) {
                return newest;
            }
            const auto names = model.labelStyles.names();
            return names.empty() ? std::string() : names.front();
        }
        if (!newest.empty() && styleLabels(model, newest, *first)) {
            return newest;
        }
        for (const std::string& name : model.labelStyles.names()) {
            if (styleLabels(model, name, *first)) {
                return name;
            }
        }
        return {};
    }

    ToolStep styleValue(std::string_view typed)
    {
        const std::string_view text = katana::core::trimmed(typed);
        if (text.empty()) {
            return enter();
        }
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing to take the style from");
        }
        const Model& model = document_->model();
        // As typed, else the one style of that name in another case.
        std::string found;
        if (model.labelStyles.contains(text)) {
            found = std::string(text);
        } else {
            for (const std::string& name : model.labelStyles.names()) {
                if (katana::core::equalsIgnoringCase(name, text)) {
                    found = name;
                    break;
                }
            }
        }
        if (found.empty()) {
            std::string names;
            for (const std::string& name : model.labelStyles.names()) {
                names += names.empty() ? "" : ", ";
                names += name;
            }
            return ToolStep::rejected("there is no label style \"" + std::string(text) + "\"" +
                                      (names.empty() ? "; LABELSTYLE DEFAULTS adds the standard set"
                                                     : "; the drawing has " + names));
        }
        // A style that cannot label what was chosen is refused now, in the
        // words LABEL would use, rather than at the end.
        const LabelStyle& style = *model.labelStyles.find(found);
        for (const EntityId id : targets_) {
            const Entity* target = model.entities.find(id);
            if (target != nullptr && !katana::entity::labels(style.kind, target->geometry)) {
                return ToolStep::rejected(
                    "a " + std::string(katana::entity::toString(style.kind)) +
                    " label style cannot label a " +
                    std::string(katana::entity::toString(target->type())) + " (id=" +
                    std::to_string(id) + ")");
            }
        }
        remember();
        settings_.style = found;
        step_ = returnTo_;
        return ToolStep::next();
    }

    ToolStep partValue(std::string_view typed)
    {
        const std::string_view text = katana::core::trimmed(typed);
        if (text.empty()) {
            return enter();
        }
        std::int32_t part = -1;
        if (!isOption(text, "All")) {
            const auto number = typedNumber(text);
            if (!number || *number < 0.0 || *number != std::floor(*number) ||
                *number > 1'000'000'000.0) {
                return ToolStep::rejected("type the segment's number, 0 for the first, or A for "
                                          "every segment");
            }
            part = static_cast<std::int32_t>(*number);
        }
        remember();
        settings_.part = part;
        step_ = returnTo_;
        return ToolStep::next();
    }

    ToolStep chooseTargets()
    {
        std::vector<EntityId> chosen = picked_;
        if (document_ != nullptr) {
            for (const EntityId id : document_->selection().ids()) {
                chosen.push_back(id);
            }
            std::erase_if(chosen,
                          [&](EntityId id) { return !document_->model().entities.contains(id); });
        }
        std::ranges::sort(chosen);
        chosen.erase(std::unique(chosen.begin(), chosen.end()), chosen.end());
        if (chosen.empty()) {
            // Nothing chosen: the tool ends, as Text ends at an empty first
            // line - the way out of a tool that starts itself again.
            return ToolStep::done(nullptr);
        }
        targets_ = std::move(chosen);
        step_ = Step::Location;
        const std::string style = currentStyle();
        return ToolStep::next(std::to_string(targets_.size()) +
                              (targets_.size() == 1 ? " object" : " objects") + " to label" +
                              (style.empty() ? std::string() : " in style " + style));
    }

    ToolStep finish(const std::optional<Point2>& position)
    {
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing to label");
        }
        const std::string style = currentStyle();
        if (style.empty()) {
            return ToolStep::rejected("the drawing has no label style that can label this; add "
                                      "one in Format > Label Styles and Rules (LABELSTYLE "
                                      "DEFAULTS adds the standard set)");
        }
        std::vector<ann::LabelRequest> requests;
        for (const EntityId id : targets_) {
            ann::LabelRequest request;
            request.target = id;
            request.style = style;
            request.part = settings_.part;
            request.position = position;
            request.textOverride = settings_.text;
            requests.push_back(std::move(request));
        }
        auto built = ann::createLabels(document_->model(), requests);
        if (!built) {
            const std::string context = std::string(katana::core::trimmed(built.error().context));
            return ToolStep::rejected(built.error().message +
                                      (context.empty() ? std::string() : " (" + context + ")"));
        }
        const std::size_t count = requests.size();
        ToolStep step =
            ToolStep::done(std::move(*built),
                           (count == 1 ? std::string("1 label") : std::to_string(count) + " labels") +
                               " in style " + style,
                           /*restart=*/true);
        // Labelled: not asked for again at the next Enter.
        step.selection = std::vector<EntityId>{};
        return step;
    }

    const Document* document_ = nullptr;
    Step step_ = Step::Select;
    Step returnTo_ = Step::Select;
    // The tool asked for the objects (there was no selection when it
    // started), so Undo at the location goes back to asking.
    bool asked_ = false;
    std::vector<EntityId> picked_;
    std::vector<std::vector<EntityId>> pickHistory_;
    std::vector<EntityId> targets_;
    Settings settings_;
    std::vector<Settings> history_;
};

} // namespace

std::unique_ptr<InteractiveTool> makeLabelTool(const ToolContext& context)
{
    return std::make_unique<LabelTool>(context);
}

} // namespace katana::cad::tools::annotate
