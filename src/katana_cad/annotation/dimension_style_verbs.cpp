#include "katana/cad/annotation/dimension_style_verbs.hpp"

#include <limits>
#include <sstream>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/dimension_text.hpp"

namespace katana::cad::annotation {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::DimensionStyle;

namespace {

// Where each field lives in the style. A field is a number, a whole number, a
// head, a text or a switch; the table below is the one list of them.
enum class Kind { Number, Whole, Head, Text, Switch };

struct Field {
    std::string_view name;
    Kind kind;
    double DimensionStyle::* number = nullptr;
    int DimensionStyle::* whole = nullptr;
    std::string DimensionStyle::* text = nullptr;
    bool DimensionStyle::* flag = nullptr;
};

const std::vector<Field>& fields()
{
    static const std::vector<Field> kFields = {
        {"TEXT", Kind::Number, &DimensionStyle::textHeight},
        {"GAP", Kind::Number, &DimensionStyle::textGap},
        {"EXTOFF", Kind::Number, &DimensionStyle::extensionOffset},
        {"EXTBEYOND", Kind::Number, &DimensionStyle::extensionBeyond},
        {"ARROW", Kind::Number, &DimensionStyle::arrowSize},
        {"HEAD", Kind::Head},
        {"SCALE", Kind::Number, &DimensionStyle::unitScale},
        {"DECIMALS", Kind::Whole, nullptr, &DimensionStyle::decimals},
        {"ROUND", Kind::Number, &DimensionStyle::roundTo},
        {"PREFIX", Kind::Text, nullptr, nullptr, &DimensionStyle::prefix},
        {"SUFFIX", Kind::Text, nullptr, nullptr, &DimensionStyle::suffix},
        {"TRIM", Kind::Switch, nullptr, nullptr, nullptr, &DimensionStyle::suppressTrailingZeros},
        {"PAPER", Kind::Switch, nullptr, nullptr, nullptr, &DimensionStyle::paperSized},
    };
    return kFields;
}

const Field* findField(std::string_view name)
{
    for (const Field& field : fields()) {
        if (katana::core::equalsIgnoringCase(field.name, name)) {
            return &field;
        }
    }
    return nullptr;
}

std::string fieldList()
{
    std::string list;
    for (const Field& field : fields()) {
        list += list.empty() ? "" : " ";
        list += field.name;
    }
    return list;
}

// The words a switch takes, as the annotation verbs' switches do. Anything
// else is refused: "PAPER of" once turned paper sizing off without a word.
Result<bool> switchOf(std::string_view value, std::string_view field)
{
    const std::string word = katana::core::lowered(value);
    if (word == "on" || word == "yes" || word == "true" || word == "1") {
        return true;
    }
    if (word == "off" || word == "no" || word == "false" || word == "0") {
        return false;
    }
    return makeError(ErrorCode::ParseFailure, "expected on or off for " + std::string(field),
                     std::string(value));
}

std::string valueOf(const DimensionStyle& style, const Field& field)
{
    switch (field.kind) {
    case Kind::Number:
        return katana::core::formatExactReal(style.*field.number);
    case Kind::Whole:
        return std::to_string(style.*field.whole);
    case Kind::Head:
        return std::string(katana::entity::toString(style.arrowHead));
    case Kind::Text:
        return style.*field.text;
    case Kind::Switch:
        return style.*field.flag ? "on" : "off";
    }
    return {};
}

} // namespace

const std::vector<std::string_view>& dimensionStyleFields()
{
    static const std::vector<std::string_view> kNames = [] {
        std::vector<std::string_view> names;
        for (const Field& field : fields()) {
            names.push_back(field.name);
        }
        return names;
    }();
    return kNames;
}

Status setDimensionStyleField(DimensionStyle& style, std::string_view name, std::string_view value)
{
    const Field* field = findField(name);
    if (field == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "unknown dimension style field",
                         std::string(name) + " (fields: " + fieldList() + ")");
    }
    switch (field->kind) {
    case Kind::Number: {
        const auto number = katana::core::parseFiniteDouble(value);
        if (!number) {
            return makeError(ErrorCode::ParseFailure,
                             "expected a number for " + std::string(field->name),
                             std::string(value));
        }
        style.*field->number = *number;
        return {};
    }
    case Kind::Whole: {
        // A whole number: "2.7" places would be quietly two. Its range is
        // validate()'s; here only that it is an int at all.
        const auto whole = katana::core::parseInteger(value);
        if (!whole || *whole < std::numeric_limits<int>::min() ||
            *whole > std::numeric_limits<int>::max()) {
            return makeError(ErrorCode::ParseFailure,
                             "expected a whole number for " + std::string(field->name),
                             std::string(value));
        }
        style.*field->whole = static_cast<int>(*whole);
        return {};
    }
    case Kind::Head: {
        auto head = katana::entity::arrowHeadFromString(value);
        if (!head) {
            return head.error();
        }
        style.arrowHead = *head;
        return {};
    }
    case Kind::Text:
        style.*field->text = std::string(value);
        return {};
    case Kind::Switch: {
        auto on = switchOf(value, field->name);
        if (!on) {
            return on.error();
        }
        style.*field->flag = *on;
        return {};
    }
    }
    return {};
}

Status setDimensionStyleFields(DimensionStyle& style, std::span<const std::string> words)
{
    if (words.empty() || words.size() % 2 != 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "fields come in pairs: FIELD value [FIELD value ...]",
                         "fields: " + fieldList());
    }
    DimensionStyle changed = style;
    for (std::size_t i = 0; i < words.size(); i += 2) {
        if (auto status = setDimensionStyleField(changed, words[i], words[i + 1]); !status) {
            return status;
        }
    }
    style = std::move(changed);
    return {};
}

std::string dimensionStyleFieldValue(const DimensionStyle& style, std::string_view name)
{
    const Field* field = findField(name);
    return field == nullptr ? std::string() : valueOf(style, *field);
}

Result<std::string> dimensionStyleChanges(const DimensionStyle& from, const DimensionStyle& to)
{
    std::string line;
    for (const Field& field : fields()) {
        const std::string before = valueOf(from, field);
        const std::string after = valueOf(to, field);
        if (before == after) {
            continue;
        }
        auto word = commandWord(after);
        if (!word) {
            return makeError(ErrorCode::InvalidArgument,
                             "the " + katana::core::lowered(field.name) +
                                 " holds a double quote, which a command line cannot carry",
                             after);
        }
        line += line.empty() ? "" : " ";
        line += std::string(field.name) + " " + *word;
    }
    return line;
}

std::vector<std::string> layersUsingDimensionStyle(const katana::entity::Model& model,
                                                   std::string_view name)
{
    std::vector<std::string> users;
    for (const katana::entity::Layer& layer : model.layers.all()) {
        if (layer.dimensionStyle == name) {
            users.push_back(layer.name);
        }
    }
    return users;
}

std::string describeDimensionStyle(const katana::entity::Model& model, const DimensionStyle& style)
{
    std::ostringstream out;
    out << "name=" << recordValue(style.name);
    for (const Field& field : fields()) {
        out << ' ' << katana::core::lowered(field.name) << '='
            << recordValue(valueOf(style, field));
    }
    out << " layers=" << layersUsingDimensionStyle(model, style.name).size()
        << " reads=" << recordValue(katana::entity::formatMeasurement(10.0, style));
    return out.str();
}

} // namespace katana::cad::annotation
