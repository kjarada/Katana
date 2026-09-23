#include "katana/entity/survey_map.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <set>

#include "katana/entity/entity.hpp"
#include "katana/entity/layer_path.hpp"
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

[[nodiscard]] bool isPrefixKey(std::string_view key)
{
    return !key.empty() && key.back() == '*';
}

// The kind of match a key makes, by its shape alone.
[[nodiscard]] SurveyMatchKind kindOfKey(std::string_view key)
{
    if (!isPrefixKey(key)) {
        return SurveyMatchKind::Exact;
    }
    return key.size() == 1 ? SurveyMatchKind::FallbackOnly : SurveyMatchKind::Prefix;
}

[[nodiscard]] bool isBlank(char c)
{
    return std::isspace(static_cast<unsigned char>(c)) != 0;
}

[[nodiscard]] katana::core::Error outOfRange(std::size_t index, std::size_t size)
{
    return makeError(ErrorCode::NotFound, "no survey rule at that index",
                     std::to_string(index) + " of " + std::to_string(size));
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

const char* toString(SurveyMatchKind kind)
{
    switch (kind) {
    case SurveyMatchKind::Exact:
        return "exact";
    case SurveyMatchKind::Prefix:
        return "prefix";
    case SurveyMatchKind::FallbackOnly:
        return "fallback only";
    case SurveyMatchKind::None:
        return "none";
    }
    return "none";
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
    // Matching is byte for byte, so " WM*" could only ever match a code that
    // itself starts with a blank - which no surveyor types. Refused here
    // rather than trimmed, because silently changing a key changes which
    // codes it catches.
    if (isBlank(rule.key.front()) || isBlank(rule.key.back())) {
        return makeError(ErrorCode::InvalidArgument,
                         "a survey code key may not begin or end with whitespace",
                         "\"" + rule.key + "\"");
    }
    if (!rule.model.empty()) {
        if (auto status = validateLayerPath(rule.model); !status) {
            return makeError(ErrorCode::InvalidArgument,
                             "the model of rule \"" + rule.key +
                                 "\" is not a valid layer path: " + status.error().message,
                             rule.model);
        }
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

void SurveyMap::indexRule(std::size_t at)
{
    const std::string& key = rules_[at].key;
    if (isPrefixKey(key)) {
        prefixes_[key.substr(0, key.size() - 1)].push_back(at);
    } else {
        exact_[key].push_back(at);
    }
}

void SurveyMap::reindex()
{
    exact_.clear();
    prefixes_.clear();
    for (std::size_t at = 0; at < rules_.size(); ++at) {
        indexRule(at);
    }
}

Status SurveyMap::add(SurveyRule rule)
{
    if (auto status = validate(rule); !status) {
        return status;
    }
    rules_.push_back(std::move(rule));
    // Appending moves no other rule, so only the new one needs indexing -
    // which keeps loading a mapfile linear.
    indexRule(rules_.size() - 1);
    return {};
}

katana::core::Result<SurveyRule> SurveyMap::at(std::size_t index) const
{
    if (index >= rules_.size()) {
        return outOfRange(index, rules_.size());
    }
    return rules_[index];
}

Status SurveyMap::replace(std::size_t index, SurveyRule rule)
{
    if (index >= rules_.size()) {
        return outOfRange(index, rules_.size());
    }
    if (auto status = validate(rule); !status) {
        return status;
    }
    rules_[index] = std::move(rule);
    reindex(); // the key may have changed
    return {};
}

Status SurveyMap::remove(std::size_t index)
{
    if (index >= rules_.size()) {
        return outOfRange(index, rules_.size());
    }
    rules_.erase(rules_.begin() + static_cast<std::ptrdiff_t>(index));
    reindex();
    return {};
}

Status SurveyMap::insert(std::size_t index, SurveyRule rule)
{
    // size() itself is allowed: inserting there is appending.
    if (index > rules_.size()) {
        return outOfRange(index, rules_.size());
    }
    if (auto status = validate(rule); !status) {
        return status;
    }
    rules_.insert(rules_.begin() + static_cast<std::ptrdiff_t>(index), std::move(rule));
    reindex();
    return {};
}

Status SurveyMap::move(std::size_t from, std::size_t to)
{
    if (from >= rules_.size()) {
        return outOfRange(from, rules_.size());
    }
    if (to >= rules_.size()) {
        return outOfRange(to, rules_.size());
    }
    // A rotation of the span between the two, which is exactly "take it out
    // and put it back at `to`" without a second copy of the rule.
    const auto first = rules_.begin();
    if (from < to) {
        std::rotate(first + static_cast<std::ptrdiff_t>(from),
                    first + static_cast<std::ptrdiff_t>(from) + 1,
                    first + static_cast<std::ptrdiff_t>(to) + 1);
    } else if (to < from) {
        std::rotate(first + static_cast<std::ptrdiff_t>(to),
                    first + static_cast<std::ptrdiff_t>(from),
                    first + static_cast<std::ptrdiff_t>(from) + 1);
    }
    reindex();
    return {};
}

std::vector<std::size_t> SurveyMap::matchIndices(std::string_view code) const
{
    // The order the old scan-and-stable-sort gave, built directly: the exact
    // key first, then each prefix of the code from the longest to the empty
    // one (the bare `*`). Two keys of one specificity that both match a code
    // must be the SAME key - an exact key equal to it, or a prefix of that
    // length - so they share one index list, and that list is in read order,
    // which is what lets an earlier mapfile win over a later one.
    std::vector<std::size_t> found;
    if (const auto exact = exact_.find(code); exact != exact_.end()) {
        found = exact->second;
    }
    for (std::size_t length = code.size() + 1; length-- > 0;) {
        if (const auto prefix = prefixes_.find(code.substr(0, length));
            prefix != prefixes_.end()) {
            found.insert(found.end(), prefix->second.begin(), prefix->second.end());
        }
    }
    return found;
}

std::vector<const SurveyRule*> SurveyMap::match(std::string_view code) const
{
    std::vector<const SurveyRule*> found;
    for (const std::size_t index : matchIndices(code)) {
        found.push_back(&rules_[index]);
    }
    return found;
}

SurveyMatch SurveyMap::lookup(std::string_view code) const
{
    SurveyMatch match;
    for (const SurveyRule* rule : this->match(code)) {
        if (match.keys.empty()) {
            match.resolved.key = rule->key;
            match.resolved.section = rule->section;
            match.kind = kindOfKey(rule->key);
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
