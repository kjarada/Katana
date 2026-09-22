#include "katana/entity/survey_map.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <limits>
#include <set>

#include "katana/entity/entity.hpp"
#include "validation.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

[[nodiscard]] std::string lowered(std::string_view text)
{
    std::string out(text);
    std::transform(out.begin(), out.end(), out.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return out;
}

// How specific a key is. An exact key beats every prefix; a longer prefix
// beats a shorter one; the bare `*` is last. Returned as a number so that
// sorting is the whole of the rule.
[[nodiscard]] std::size_t specificity(const SurveyRule& rule)
{
    if (rule.key.empty() || rule.key.back() != '*') {
        return std::numeric_limits<std::size_t>::max(); // exact
    }
    return rule.key.size() - 1; // the length of the prefix; `*` alone is 0
}

[[nodiscard]] bool matches(const SurveyRule& rule, std::string_view code)
{
    if (rule.key.empty()) {
        return false;
    }
    if (rule.key.back() != '*') {
        return rule.key == code;
    }
    const std::string_view prefix = std::string_view(rule.key).substr(0, rule.key.size() - 1);
    return code.size() >= prefix.size() && code.substr(0, prefix.size()) == prefix;
}

// A field is taken from the first rule that says anything about it.
template <class T> void takeIfUnset(T& into, const T& from)
{
    if (into == T{} && !(from == T{})) {
        into = from;
    }
}

template <class T> void takeIfUnset(std::optional<T>& into, const std::optional<T>& from)
{
    if (!into.has_value() && from.has_value()) {
        into = from;
    }
}

} // namespace

const char* toString(SurveySection section)
{
    switch (section) {
    case SurveySection::Map:
        return "map_data";
    case SurveySection::VertexSymbol:
        return "vertex_symbol_data";
    case SurveySection::VertexTextStyle:
        return "vertex_textstyle_data";
    case SurveySection::Pipe:
        return "pipe_data";
    case SurveySection::VertexPipe:
        return "vertex_pipe_data";
    case SurveySection::SegmentPipe:
        return "segment_pipe_data";
    case SurveySection::StringAttribute:
        return "string_attribute_data";
    case SurveySection::VertexAttribute:
        return "vertex_attribute_data";
    case SurveySection::Tinable:
        return "tinable_data";
    }
    return "map_data";
}

std::optional<SurveySection> parseSurveySection(std::string_view element)
{
    if (element == "map_data") {
        return SurveySection::Map;
    }
    // 12d writes the symbol section twice, once in a form marked v9. Both say
    // the same thing, so both are read as the same section.
    if (element == "vertex_symbol_data" || element == "vertex_symbol_data_v9") {
        return SurveySection::VertexSymbol;
    }
    if (element == "vertex_textstyle_data") {
        return SurveySection::VertexTextStyle;
    }
    if (element == "pipe_data") {
        return SurveySection::Pipe;
    }
    if (element == "vertex_pipe_data") {
        return SurveySection::VertexPipe;
    }
    if (element == "segment_pipe_data") {
        return SurveySection::SegmentPipe;
    }
    if (element == "string_attribute_data") {
        return SurveySection::StringAttribute;
    }
    if (element == "vertex_attribute_data") {
        return SurveySection::VertexAttribute;
    }
    if (element == "tinable_data") {
        return SurveySection::Tinable;
    }
    return std::nullopt;
}

const char* toString(SurveyBreakline breakline)
{
    return breakline == SurveyBreakline::Line ? "Line" : "Point";
}

std::optional<SurveyBreakline> parseSurveyBreakline(std::string_view text)
{
    const std::string word = lowered(text);
    if (word == "line") {
        return SurveyBreakline::Line;
    }
    if (word == "point") {
        return SurveyBreakline::Point;
    }
    return std::nullopt;
}

Status validate(const SurveyRule& rule)
{
    if (auto status = detail::validateName(rule.key, "survey code"); !status) {
        return status;
    }
    // A wildcard anywhere but at the end is a key this cannot match, and
    // matching it approximately would put the code in the wrong model.
    const std::size_t star = rule.key.find('*');
    if (star != std::string::npos && star + 1 != rule.key.size()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a survey code key may only use `*` as its last character", rule.key);
    }
    if (rule.key.find('?') != std::string::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "`?` is not a wildcard a survey code key may use", rule.key);
    }
    for (const std::string* text : {&rule.model, &rule.colour, &rule.linestyle, &rule.weight,
                                    &rule.group, &rule.comment}) {
        if (!isValidUtf8(*text)) {
            return makeError(ErrorCode::InvalidArgument, "a survey rule field is not valid UTF-8",
                             rule.key);
        }
    }
    if (rule.symbol) {
        if (!isValidUtf8(rule.symbol->style) || !isValidUtf8(rule.symbol->colour)) {
            return makeError(ErrorCode::InvalidArgument, "a symbol name is not valid UTF-8",
                             rule.key);
        }
        if (!(std::isfinite(rule.symbol->size) && rule.symbol->size >= 0.0 &&
              std::isfinite(rule.symbol->rotation) && std::isfinite(rule.symbol->offset) &&
              std::isfinite(rule.symbol->raise))) {
            return makeError(ErrorCode::InvalidArgument, "a symbol's numbers are not finite",
                             rule.key);
        }
    }
    if (rule.textStyle) {
        const SurveyTextStyle& text = *rule.textStyle;
        if (!(std::isfinite(text.size) && text.size >= 0.0 && std::isfinite(text.offset) &&
              std::isfinite(text.raise) && std::isfinite(text.angle) &&
              std::isfinite(text.slant) && std::isfinite(text.widthFactor))) {
            return makeError(ErrorCode::InvalidArgument, "a text style's numbers are not finite",
                             rule.key);
        }
    }
    for (const auto* list :
         {&rule.attributes, &rule.vertexAttributes, &rule.segmentAttributes}) {
        for (const SurveyAttribute& attribute : *list) {
            if (attribute.name.empty()) {
                return makeError(ErrorCode::InvalidArgument, "an attribute of this rule has no name",
                                 rule.key);
            }
            if (!isValidUtf8(attribute.name) || !isValidUtf8(attribute.value) ||
                !isValidUtf8(attribute.type)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "an attribute of this rule is not valid UTF-8", rule.key);
            }
        }
    }
    return {};
}

Status SurveyMap::add(SurveyRule rule)
{
    if (auto status = validate(rule); !status) {
        return status;
    }
    rules_.push_back(std::move(rule));
    return {};
}

std::vector<const SurveyRule*> SurveyMap::match(std::string_view code) const
{
    std::vector<const SurveyRule*> found;
    for (const SurveyRule& rule : rules_) {
        if (matches(rule, code)) {
            found.push_back(&rule);
        }
    }
    // Stable, so that two rules of equal specificity keep the order they were
    // read in - which is what makes an earlier mapfile win over a later one.
    std::stable_sort(found.begin(), found.end(),
                     [](const SurveyRule* a, const SurveyRule* b) {
                         return specificity(*a) > specificity(*b);
                     });
    return found;
}

SurveyMatch SurveyMap::lookup(std::string_view code) const
{
    SurveyMatch match;
    for (const SurveyRule* rule : this->match(code)) {
        if (match.keys.empty()) {
            match.resolved.key = rule->key;
            match.resolved.section = rule->section;
        }
        match.keys.push_back(rule->key);
        takeIfUnset(match.resolved.model, rule->model);
        takeIfUnset(match.resolved.colour, rule->colour);
        takeIfUnset(match.resolved.linestyle, rule->linestyle);
        takeIfUnset(match.resolved.weight, rule->weight);
        takeIfUnset(match.resolved.group, rule->group);
        takeIfUnset(match.resolved.comment, rule->comment);
        takeIfUnset(match.resolved.breakline, rule->breakline);
        takeIfUnset(match.resolved.tinable, rule->tinable);
        takeIfUnset(match.resolved.hide, rule->hide);
        takeIfUnset(match.resolved.symbol, rule->symbol);
        takeIfUnset(match.resolved.textStyle, rule->textStyle);
        takeIfUnset(match.resolved.pipe, rule->pipe);
        takeIfUnset(match.resolved.vertexPipe, rule->vertexPipe);
        takeIfUnset(match.resolved.segmentPipe, rule->segmentPipe);
        // Attributes ACCUMULATE rather than override: a general rule saying
        // every pipe has a DepthLocation and a specific one giving a diameter
        // are both meant, which is the whole reason the mapfiles are written
        // with a `*` rule beside the coded ones.
        const auto append = [](std::vector<SurveyAttribute>& into,
                               const std::vector<SurveyAttribute>& from) {
            for (const SurveyAttribute& attribute : from) {
                const bool already =
                    std::any_of(into.begin(), into.end(), [&attribute](const SurveyAttribute& a) {
                        return a.name == attribute.name;
                    });
                if (!already) {
                    into.push_back(attribute);
                }
            }
        };
        append(match.resolved.attributes, rule->attributes);
        append(match.resolved.vertexAttributes, rule->vertexAttributes);
        append(match.resolved.segmentAttributes, rule->segmentAttributes);
    }
    return match;
}

std::vector<std::string> SurveyMap::keys() const
{
    std::set<std::string> distinct;
    for (const SurveyRule& rule : rules_) {
        distinct.insert(rule.key);
    }
    return {distinct.begin(), distinct.end()};
}

std::vector<std::string> SurveyMap::stylesReferenced() const
{
    std::set<std::string> distinct;
    for (const SurveyRule& rule : rules_) {
        if (!rule.linestyle.empty()) {
            distinct.insert(rule.linestyle);
        }
        if (rule.symbol && !rule.symbol->style.empty()) {
            distinct.insert(rule.symbol->style);
        }
    }
    return {distinct.begin(), distinct.end()};
}

} // namespace katana::entity
