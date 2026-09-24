// Select Similar and Quick Select (see families.hpp): tools whose answer is a
// selection, left in the document's selection set (ToolStep::selection) for
// the next Modify tool, the Properties panel or Erase to act on. Neither
// changes the drawing, so neither makes a command, and neither is undone with
// the drawing - as selecting by hand is not.
//
// Both select by the document's own rule (isSelectable): an entity that is
// hidden, or on a hidden or locked layer, is never selected, however well it
// matches. A layer hidden in one view only is not yet left out, because a
// tool is not told which view it runs in (as Move's All; outstanding).
//
// Quick Select asks its questions one at a time at the command line, each
// with a default Enter takes - kind, layer, one condition, and whether to
// look in the selection or the whole drawing - so it runs the same from a
// menu, from typed words and from a headless script, with no dialog to wait
// on.

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <tuple>
#include <utility>
#include <variant>
#include <vector>

#include "everyday_support.hpp"
#include "families.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/selection.hpp"
#include "katana/core/text.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools {

namespace {

using everyday::counted;
using everyday::SelectionStep;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::geometry::Point2;
using modify_edit::isOption;

// Every entity `keep` accepts that the document lets be selected, ascending.
template <typename Keep>
std::vector<EntityId> selectableWhere(const Document& document, Keep keep)
{
    std::vector<EntityId> out;
    const katana::entity::Model& model = document.model();
    model.entities.forEach([&](const Entity& entity) {
        if (isSelectable(model, entity, kNoLayerOverrides) && keep(entity)) {
            out.push_back(entity.id);
        }
    });
    std::ranges::sort(out);
    return out;
}

std::string styleName(const Entity& entity)
{
    return entity.style.empty() ? std::string("ByLayer") : entity.style;
}

std::string colourName(const Entity& entity)
{
    return entity.color ? entity.color->toHex() : std::string("ByLayer");
}

// ---- Select Similar ----------------------------------------------------------------

struct SimilarSettings {
    bool kind = true;
    bool layer = true;
    bool style = true;
    bool colour = false;

    [[nodiscard]] std::string words() const
    {
        std::vector<std::string> parts;
        if (kind) {
            parts.emplace_back("kind");
        }
        if (layer) {
            parts.emplace_back("layer");
        }
        if (style) {
            parts.emplace_back("style");
        }
        if (colour) {
            parts.emplace_back("colour");
        }
        std::string text;
        for (std::size_t i = 0; i < parts.size(); ++i) {
            text += (i == 0 ? "" : i + 1 == parts.size() ? " and " : ", ") + parts[i];
        }
        return text;
    }
};

class SelectSimilarTool final : public InteractiveTool {
  public:
    SelectSimilarTool(const ToolContext& context, std::shared_ptr<SimilarSettings> settings)
        : document_(context.document), selection_(context.document),
          settings_(std::move(settings))
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (settingsStep_) {
            return "Match on any of [Kind/Layer/Style/Colour] <" + settings_->words() + ">";
        }
        return "Select objects to find the like of or [SEttings], then press Enter";
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return settingsStep_ ? ToolInput::Value : ToolInput::Selection;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        return settingsStep_ ? InteractiveTool::entity(id, at) : selection_.pick(id);
    }

    ToolStep value(std::string_view text) override
    {
        if (settingsStep_) {
            return chooseSettings(text);
        }
        if (isOption(text, "SEttings", "SE")) {
            settingsStep_ = true;
            return ToolStep::next();
        }
        return ToolStep::rejected("'" + std::string(text) +
                                  "' is not an option here: pick objects, or type SE");
    }

    ToolStep enter() override
    {
        if (settingsStep_) {
            settingsStep_ = false;
            return ToolStep::next();
        }
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing");
        }
        const std::vector<EntityId> seeds = selection_.chosen();
        if (seeds.empty()) {
            return ToolStep::rejected(
                "nothing is selected: select the objects to find the like of, then press Enter");
        }
        // One key per seed: what an entity must share with it, the fields
        // not matched on left empty so they compare equal.
        using Key = std::tuple<int, std::string, std::string, std::string>;
        const auto keyOf = [&](const Entity& entity) {
            return Key{settings_->kind ? static_cast<int>(entity.type()) : -1,
                       settings_->layer ? entity.layer : std::string(),
                       settings_->style ? styleName(entity) : std::string(),
                       settings_->colour ? colourName(entity) : std::string()};
        };
        std::vector<Key> keys;
        for (const EntityId id : seeds) {
            if (const Entity* entity = document_->model().entities.find(id)) {
                keys.push_back(keyOf(*entity));
            }
        }
        std::ranges::sort(keys);
        keys.erase(std::unique(keys.begin(), keys.end()), keys.end());
        std::vector<EntityId> found = selectableWhere(*document_, [&](const Entity& entity) {
            return std::ranges::binary_search(keys, keyOf(entity));
        });
        ToolStep step = ToolStep::done(
            nullptr, counted(found.size(), "entity", "entities") + " selected: the same " +
                         settings_->words() + " as the " + std::to_string(seeds.size()) +
                         " picked");
        step.selection = std::move(found);
        return step;
    }

    ToolStep undo() override
    {
        if (settingsStep_) {
            settingsStep_ = false;
            return ToolStep::next();
        }
        return selection_.undo() ? ToolStep::next()
                                 : ToolStep::rejected("nothing to undo in this tool");
    }

  private:
    ToolStep chooseSettings(std::string_view text)
    {
        SimilarSettings chosen{false, false, false, false};
        std::string words(text);
        std::ranges::replace(words, ',', ' ');
        std::size_t at = 0;
        while (at < words.size()) {
            const std::size_t end = std::min(words.find(' ', at), words.size());
            const std::string_view word = std::string_view(words).substr(at, end - at);
            at = end + 1;
            if (word.empty()) {
                continue;
            }
            if (isOption(word, "Kind", "K")) {
                chosen.kind = true;
            } else if (isOption(word, "Layer", "L")) {
                chosen.layer = true;
            } else if (isOption(word, "Style", "S")) {
                chosen.style = true;
            } else if (isOption(word, "Colour", "C") || isOption(word, "Color", "C")) {
                chosen.colour = true;
            } else {
                return ToolStep::rejected("'" + std::string(word) +
                                          "' is not one of Kind, Layer, Style and Colour");
            }
        }
        if (!(chosen.kind || chosen.layer || chosen.style || chosen.colour)) {
            return ToolStep::rejected("name at least one of Kind, Layer, Style and Colour");
        }
        *settings_ = chosen;
        settingsStep_ = false;
        return ToolStep::next("Matching on " + settings_->words());
    }

    const Document* document_ = nullptr;
    SelectionStep selection_;
    std::shared_ptr<SimilarSettings> settings_;
    bool settingsStep_ = false;
};

// ---- Quick Select ------------------------------------------------------------------

// `text` against `pattern`, * for any run and ? for any one character, in any
// case: layer names and codes are typed by hand, in whatever case.
bool matchesWildcard(std::string_view text, std::string_view pattern)
{
    // The classic two-pointer match, backtracking to the last *: linear in
    // practice, and no regex to compile per entity.
    std::size_t t = 0;
    std::size_t p = 0;
    std::optional<std::size_t> star;
    std::size_t resume = 0;
    while (t < text.size()) {
        if (p < pattern.size() &&
            (pattern[p] == '?' ||
             katana::core::asciiLower(pattern[p]) == katana::core::asciiLower(text[t]))) {
            ++t;
            ++p;
        } else if (p < pattern.size() && pattern[p] == '*') {
            star = p++;
            resume = t;
        } else if (star) {
            p = *star + 1;
            t = ++resume;
        } else {
            return false;
        }
    }
    while (p < pattern.size() && pattern[p] == '*') {
        ++p;
    }
    return p == pattern.size();
}

enum class Compare { Equal, NotEqual, Less, Greater, LessOrEqual, GreaterOrEqual };

struct Condition {
    std::string key;
    Compare compare = Compare::Equal;
    std::string operand;
    std::optional<double> number; // the operand as a number, when it is one
    std::string text;             // as typed, for the report
};

std::optional<Condition> parseCondition(std::string_view typed)
{
    // Two-character operators first, so "<=" is not read as "<".
    static constexpr std::array<std::pair<std::string_view, Compare>, 6> kOperators = {{
        {"<>", Compare::NotEqual},
        {"<=", Compare::LessOrEqual},
        {">=", Compare::GreaterOrEqual},
        {"=", Compare::Equal},
        {"<", Compare::Less},
        {">", Compare::Greater},
    }};
    for (const auto& [symbol, compare] : kOperators) {
        const std::size_t at = typed.find(symbol);
        if (at == std::string_view::npos) {
            continue;
        }
        Condition condition;
        condition.key = std::string(katana::core::trimmed(typed.substr(0, at)));
        std::string_view operand = katana::core::trimmed(typed.substr(at + symbol.size()));
        if (operand.size() >= 2 && operand.front() == '"' && operand.back() == '"') {
            operand = operand.substr(1, operand.size() - 2);
        }
        condition.operand = std::string(operand);
        condition.compare = compare;
        condition.number = katana::core::parseFiniteDouble(operand);
        condition.text = std::string(katana::core::trimmed(typed));
        if (condition.key.empty()) {
            return std::nullopt;
        }
        return condition;
    }
    return std::nullopt;
}

bool ordered(Compare compare)
{
    return compare != Compare::Equal && compare != Compare::NotEqual;
}

// Whether `entity` meets `condition`. The keys Layer, Style and Colour are
// the entity's own; any other key is a property, and an entity without that
// property never meets the condition, <> included - "code <> TB" means the
// coded points whose code is not TB, not everything that has no code.
bool meets(const Entity& entity, const Condition& condition)
{
    std::optional<std::string> text;
    std::optional<double> number;
    if (katana::core::equalsIgnoringCase(condition.key, "layer")) {
        text = entity.layer;
    } else if (katana::core::equalsIgnoringCase(condition.key, "style")) {
        text = styleName(entity);
    } else if (katana::core::equalsIgnoringCase(condition.key, "colour") ||
               katana::core::equalsIgnoringCase(condition.key, "color")) {
        text = colourName(entity);
    } else {
        auto found = entity.properties.find(condition.key);
        if (found == entity.properties.end()) {
            // Keys are typed by hand; "Code" finds "code".
            found = std::ranges::find_if(entity.properties, [&](const auto& property) {
                return katana::core::equalsIgnoringCase(property.first, condition.key);
            });
        }
        if (found == entity.properties.end()) {
            return false;
        }
        const auto& value = found->second;
        if (const auto* integer = std::get_if<std::int64_t>(&value)) {
            number = static_cast<double>(*integer);
        } else if (const auto* real = std::get_if<double>(&value)) {
            number = *real;
        } else if (const auto* string = std::get_if<std::string>(&value)) {
            number = katana::core::parseFiniteDouble(katana::core::trimmed(*string));
        }
        text = katana::entity::toString(value);
    }
    if (condition.number && number) {
        const double a = *number;
        const double b = *condition.number;
        switch (condition.compare) {
        case Compare::Equal:
            return a == b;
        case Compare::NotEqual:
            return a != b;
        case Compare::Less:
            return a < b;
        case Compare::Greater:
            return a > b;
        case Compare::LessOrEqual:
            return a <= b;
        case Compare::GreaterOrEqual:
            return a >= b;
        }
    }
    if (ordered(condition.compare)) {
        return false; // "code > TB" orders nothing; refused when typed
    }
    const bool equal = matchesWildcard(*text, condition.operand);
    return condition.compare == Compare::Equal ? equal : !equal;
}

class QuickSelectTool final : public InteractiveTool {
  public:
    explicit QuickSelectTool(const ToolContext& context)
        : document_(context.document), given_(context.selection)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Kind:
            return "Object kind [Any/Point/Line/Arc/Polyline/Circle/Text/Dimension] <Any>";
        case Step::Layer:
            return "Layer name, with * and ? as wildcards <*>";
        case Step::Condition:
            return "Condition such as code = TB or elevation > 100 (= <> < > <= >=; Layer, "
                   "Style, Colour or a property), Enter for none";
        case Step::Scope:
            return "Look in [Drawing/Selection] <Selection>";
        }
        return {};
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Value; }

    ToolStep value(std::string_view typed) override
    {
        const std::string_view text = katana::core::trimmed(typed);
        switch (step_) {
        case Step::Kind:
            if (katana::core::equalsIgnoringCase(text, "Any")) {
                kind_.reset();
            } else if (const auto kind = kindNamed(text)) {
                kind_ = *kind;
            } else {
                return ToolStep::rejected(
                    "'" + std::string(text) +
                    "' is not a kind: Any, Point, Line, Arc, Polyline, Circle, Text or Dimension");
            }
            step_ = Step::Layer;
            return ToolStep::next();
        case Step::Layer:
            layer_ = text.empty() ? "*" : std::string(text);
            step_ = Step::Condition;
            return ToolStep::next();
        case Step::Condition: {
            auto condition = parseCondition(text);
            if (!condition) {
                return ToolStep::rejected("a condition is a name, an operator (= <> < > <= >=) "
                                          "and a value, such as code = TB");
            }
            if (ordered(condition->compare) && !condition->number) {
                return ToolStep::rejected("< and > compare numbers; compare text with = or <>");
            }
            condition_ = std::move(condition);
            return afterCondition();
        }
        case Step::Scope:
            if (isOption(text, "Drawing", "D")) {
                return select(false);
            }
            if (isOption(text, "Selection", "S")) {
                return select(true);
            }
            return ToolStep::rejected("'" + std::string(text) +
                                      "' is not an option here: Drawing or Selection");
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Kind:
            kind_.reset();
            step_ = Step::Layer;
            return ToolStep::next();
        case Step::Layer:
            layer_ = "*";
            step_ = Step::Condition;
            return ToolStep::next();
        case Step::Condition:
            condition_.reset();
            return afterCondition();
        case Step::Scope:
            return select(true);
        }
        return ToolStep::done(nullptr);
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Kind:
            return ToolStep::rejected("nothing to undo in this tool");
        case Step::Layer:
            step_ = Step::Kind;
            break;
        case Step::Condition:
            step_ = Step::Layer;
            break;
        case Step::Scope:
            step_ = Step::Condition;
            break;
        }
        return ToolStep::next();
    }

  private:
    enum class Step { Kind, Layer, Condition, Scope };

    static std::optional<EntityType> kindNamed(std::string_view text)
    {
        for (int i = 0; i < 7; ++i) {
            const auto kind = static_cast<EntityType>(i);
            if (katana::core::equalsIgnoringCase(text, katana::entity::toString(kind))) {
                return kind;
            }
        }
        return std::nullopt;
    }

    ToolStep afterCondition()
    {
        // Asked only when there is a selection to look in.
        if (!given_.empty()) {
            step_ = Step::Scope;
            return ToolStep::next();
        }
        return select(false);
    }

    ToolStep select(bool withinSelection)
    {
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing");
        }
        std::vector<EntityId> found = selectableWhere(*document_, [&](const Entity& entity) {
            if (withinSelection && !std::ranges::binary_search(given_, entity.id)) {
                return false;
            }
            if (kind_ && entity.type() != *kind_) {
                return false;
            }
            if (layer_ != "*" && !matchesWildcard(entity.layer, layer_)) {
                return false;
            }
            return !condition_ || meets(entity, *condition_);
        });
        std::string text = counted(found.size(), "entity", "entities") + " selected: ";
        text += kind_ ? std::string(katana::entity::toString(*kind_)) + "s" : "any kind";
        text += layer_ == "*" ? " on any layer" : " on layer " + layer_;
        if (condition_) {
            text += " where " + condition_->text;
        }
        text += withinSelection ? ", within the selection" : ", in the drawing";
        ToolStep step = ToolStep::done(nullptr, std::move(text));
        step.selection = std::move(found);
        return step;
    }

    const Document* document_ = nullptr;
    std::vector<EntityId> given_; // ascending, as ToolContext gives it
    Step step_ = Step::Kind;
    std::optional<EntityType> kind_;
    std::string layer_ = "*";
    std::optional<Condition> condition_;
};

} // namespace

void addSelectTools(ToolCatalog& catalog, const Report& report)
{
    auto settings = std::make_shared<SimilarSettings>();
    ToolInfo similar;
    similar.id = "select.similar";
    similar.name = "Select Similar";
    similar.category = "Tools";
    similar.group = "Select";
    similar.order = 10;
    similar.aliases = {"SELECTSIMILAR"};
    similar.tip = "Selects every object of the same kind, layer and style as those picked (SE "
                  "to match on kind, layer, style or colour).";
    similar.make = [settings](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<SelectSimilarTool>(context, settings);
    };
    report(catalog.add(std::move(similar)));

    ToolInfo quick;
    quick.id = "select.quick";
    quick.name = "Quick Select";
    quick.category = "Tools";
    quick.group = "Select";
    quick.order = 20;
    quick.aliases = {"QSELECT"};
    quick.tip = "Selects by kind, layer and one condition on a property, such as code = TB or "
                "elevation > 100, in the drawing or within the selection.";
    quick.make = [](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<QuickSelectTool>(context);
    };
    report(catalog.add(std::move(quick)));
}

} // namespace katana::cad::tools
