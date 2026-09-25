#include "katana/cad/scope_verbs.hpp"

#include <algorithm>
#include <cctype>
#include <limits>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

#include "katana/cad/view_set.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::EntityType;

static_assert(std::is_same_v<ViewId, std::uint32_t>,
              "ScopeWords::view and ScopeView::id hold a cad::ViewId");

namespace {

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
}

// "a,b,,c" -> a b c: an empty item is a stray comma, not a name.
std::vector<std::string> splitList(std::string_view text)
{
    std::vector<std::string> parts;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t comma = std::min(text.find(',', start), text.size());
        if (comma > start) {
            parts.emplace_back(text.substr(start, comma - start));
        }
        start = comma + 1;
    }
    return parts;
}

Result<EntityType> parseTypeName(std::string_view text)
{
    // entityTypeFromString takes the enumerator's spelling: title case.
    std::string name = katana::core::lowered(text);
    if (!name.empty()) {
        name.front() = static_cast<char>(std::toupper(static_cast<unsigned char>(name.front())));
    }
    return katana::entity::entityTypeFromString(name);
}

// A WHERE condition: a word holding '=', or DRAWN.
bool isConditionWord(std::string_view word)
{
    return upper(word) == "DRAWN" || word.find('=') != std::string_view::npos;
}

Status parseFilterWord(const std::string& word, ModifyFilter& filter)
{
    if (upper(word) == "DRAWN") {
        filter.drawnOnly = true;
        return {};
    }
    const std::size_t equals = word.find('=');
    if (equals == std::string::npos) {
        return makeError(ErrorCode::ParseFailure, "a WHERE condition is key=value", word);
    }
    const std::string key = upper(word.substr(0, equals));
    const std::string value = word.substr(equals + 1);
    if (key == "TYPE") {
        for (const std::string& name : splitList(value)) {
            auto type = parseTypeName(name);
            if (!type) {
                return type.error();
            }
            filter.types.insert(*type);
        }
    } else if (key == "LAYER") {
        filter.layers = splitList(value);
    } else if (key == "STYLE") {
        filter.style = value;
    } else if (key == "COLOUR" || key == "COLOR") {
        auto colour = parseColourOrByLayer(value);
        if (!colour) {
            return colour.error();
        }
        filter.colour = *colour;
    } else if (key == "PROP") {
        const std::size_t colon = value.find(':');
        filter.property = value.substr(0, colon);
        if (colon != std::string::npos) {
            filter.propertyValue = value.substr(colon + 1);
        }
    } else if (key == "TEXT") {
        filter.text = value;
    } else {
        return makeError(ErrorCode::ParseFailure,
                         "not a WHERE key: TYPE LAYER STYLE COLOUR PROP TEXT or DRAWN", word);
    }
    return {};
}

// "x0,y0,x1,y1", corners in either order.
Result<katana::geometry::Box2> parseArea(std::string_view text)
{
    const auto refuse = [&text] {
        return makeError(ErrorCode::ParseFailure, "AREA is x0,y0,x1,y1: four numbers",
                         std::string(text));
    };
    std::vector<double> numbers;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t comma = std::min(text.find(',', start), text.size());
        const auto value = katana::core::parseFiniteDouble(text.substr(start, comma - start));
        if (!value) {
            return refuse();
        }
        numbers.push_back(*value);
        start = comma + 1;
    }
    if (numbers.size() != 4) {
        return refuse();
    }
    return katana::geometry::Box2(
        katana::geometry::Point2(std::min(numbers[0], numbers[2]), std::min(numbers[1], numbers[3])),
        katana::geometry::Point2(std::max(numbers[0], numbers[2]),
                                 std::max(numbers[1], numbers[3])));
}

bool allDigits(std::string_view word)
{
    return !word.empty() && std::ranges::all_of(word, [](char c) { return c >= '0' && c <= '9'; });
}

std::string joined(const std::vector<std::string>& items, std::string_view separator)
{
    std::string text;
    for (const std::string& item : items) {
        text += (text.empty() ? "" : std::string(separator)) + item;
    }
    return text;
}

// The filter as the words WHERE takes, unquoted.
Result<std::vector<std::string>> conditionWords(const ModifyFilter& filter)
{
    std::vector<std::string> words;
    if (!filter.types.empty()) {
        std::vector<std::string> names;
        for (const EntityType type : filter.types) {
            names.push_back(katana::core::lowered(katana::entity::toString(type)));
        }
        words.push_back("TYPE=" + joined(names, ","));
    }
    if (!filter.layers.empty()) {
        for (const std::string& pattern : filter.layers) {
            if (pattern.empty() || pattern.find(',') != std::string::npos) {
                return makeError(ErrorCode::InvalidArgument,
                                 "a layer pattern with a comma, or none, cannot be written in a "
                                 "LAYER= list",
                                 pattern);
            }
        }
        words.push_back("LAYER=" + joined(filter.layers, ","));
    }
    if (filter.style) {
        words.push_back("STYLE=" + *filter.style);
    }
    if (filter.colour) {
        words.push_back("COLOUR=" + (*filter.colour ? (*filter.colour)->toHex() : "ByLayer"));
    }
    if (filter.propertyValue && !filter.property) {
        return makeError(ErrorCode::InvalidArgument,
                         "a property value to match needs the property it is the value of");
    }
    if (filter.property) {
        words.push_back("PROP=" + *filter.property +
                        (filter.propertyValue ? ":" + *filter.propertyValue : std::string()));
    }
    if (filter.text) {
        words.push_back("TEXT=" + *filter.text);
    }
    if (filter.drawnOnly) {
        words.push_back("DRAWN");
    }
    return words;
}

std::string_view sourceWord(ScopeSource source)
{
    switch (source) {
    case ScopeSource::Selection:
        return "selection";
    case ScopeSource::Drawing:
        return "drawing";
    case ScopeSource::View:
        return "view";
    case ScopeSource::Area:
        return "area";
    case ScopeSource::Layers:
        return "layers";
    }
    return "selection";
}

std::string areaText(const katana::geometry::Box2& area)
{
    return katana::core::formatExactReal(area.min.x) + "," +
           katana::core::formatExactReal(area.min.y) + "," +
           katana::core::formatExactReal(area.max.x) + "," +
           katana::core::formatExactReal(area.max.y);
}

} // namespace

bool isScopeWord(std::string_view word)
{
    const std::string folded = upper(word);
    for (const char* name :
         {"SELECTION", "SEL", "DRAWING", "ALL", "VIEW", "AREA", "LAYERS", "LAYER", "WHERE"}) {
        if (folded == name) {
            return true;
        }
    }
    return false;
}

Result<std::optional<katana::entity::Color>> parseColourOrByLayer(std::string_view text)
{
    if (upper(text) == "BYLAYER") {
        return std::optional<katana::entity::Color>{};
    }
    auto colour = katana::entity::Color::fromHex(text);
    if (!colour) {
        return colour.error();
    }
    return std::optional<katana::entity::Color>(*colour);
}

Result<ScopeWords> parseScopeWords(const std::vector<std::string>& words, std::size_t& at)
{
    ScopeWords result;
    bool named = false;
    std::size_t i = at;
    const auto oneScope = [&named](const std::string& word) -> Status {
        if (named) {
            return makeError(ErrorCode::InvalidArgument,
                             "give one scope: SELECTION, DRAWING, VIEW, AREA or LAYERS", word);
        }
        named = true;
        return {};
    };
    while (i < words.size()) {
        const std::string& word = words[i];
        const std::string folded = upper(word);
        if (folded == "WHERE") {
            break;
        }
        if (folded == "ONLY") {
            return makeError(ErrorCode::ParseFailure, "ONLY follows a LAYERS list", word);
        }
        if (folded == "EXTENTS") {
            return makeError(ErrorCode::ParseFailure, "EXTENTS follows VIEW [<view id>]", word);
        }
        const bool scope = folded == "SELECTION" || folded == "SEL" || folded == "DRAWING" ||
                           folded == "ALL" || folded == "VIEW" || folded == "AREA" ||
                           folded == "LAYERS" || folded == "LAYER";
        if (!scope) {
            break;
        }
        if (auto status = oneScope(word); !status) {
            return status.error();
        }
        ++i;
        if (folded == "SELECTION" || folded == "SEL") {
            result.source = ScopeSource::Selection;
        } else if (folded == "DRAWING" || folded == "ALL") {
            result.source = ScopeSource::Drawing;
        } else if (folded == "VIEW") {
            result.source = ScopeSource::View;
            if (i < words.size() && allDigits(words[i])) {
                const auto id = katana::core::parseInteger(words[i]);
                if (!id || *id < 1 || *id > std::numeric_limits<std::uint32_t>::max()) {
                    return makeError(ErrorCode::ParseFailure,
                                     "a view id is a whole number from 1, as the window numbers "
                                     "its views",
                                     words[i]);
                }
                result.view = static_cast<std::uint32_t>(*id);
                ++i;
            }
            if (i < words.size() && upper(words[i]) == "EXTENTS") {
                result.extents = true;
                ++i;
            }
        } else if (folded == "AREA") {
            if (i >= words.size()) {
                return makeError(ErrorCode::ParseFailure, "AREA is x0,y0,x1,y1: four numbers",
                                 word);
            }
            auto area = parseArea(words[i]);
            if (!area) {
                return area.error();
            }
            result.source = ScopeSource::Area;
            result.area = *area;
            ++i;
        } else {
            if (i >= words.size() || splitList(words[i]).empty()) {
                return makeError(ErrorCode::ParseFailure,
                                 "LAYERS needs the layers: LAYERS a,b[,c] [ONLY]", word);
            }
            result.source = ScopeSource::Layers;
            result.layers = splitList(words[i]);
            ++i;
            if (i < words.size() && upper(words[i]) == "ONLY") {
                result.sublayers = false;
                ++i;
            }
        }
    }
    // WHERE, and its conditions up to the first word that is not one. A
    // second WHERE reads on, so "WHERE TYPE=point WHERE DRAWN" is one filter.
    if (i < words.size() && upper(words[i]) == "WHERE") {
        ++i;
        while (i < words.size()) {
            if (upper(words[i]) == "WHERE") {
                ++i;
                continue;
            }
            if (!isConditionWord(words[i])) {
                break;
            }
            if (auto status = parseFilterWord(words[i], result.filter); !status) {
                return status.error();
            }
            ++i;
        }
    }
    at = i;
    return result;
}

Result<std::string> formatScopeWords(const ScopeWords& words)
{
    std::vector<std::string> parts;
    switch (words.source) {
    case ScopeSource::Selection:
        parts.emplace_back("SELECTION");
        break;
    case ScopeSource::Drawing:
        parts.emplace_back("DRAWING");
        break;
    case ScopeSource::View:
        parts.emplace_back("VIEW");
        if (words.view) {
            parts.push_back(std::to_string(*words.view));
        }
        if (words.extents) {
            parts.emplace_back("EXTENTS");
        }
        break;
    case ScopeSource::Area:
        if (words.area.empty()) {
            return makeError(ErrorCode::InvalidArgument, "an AREA needs a box");
        }
        parts.emplace_back("AREA");
        parts.push_back(areaText(words.area));
        break;
    case ScopeSource::Layers:
        if (words.layers.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a LAYERS scope needs a layer");
        }
        for (const std::string& layer : words.layers) {
            if (layer.empty() || layer.find(',') != std::string::npos) {
                return makeError(ErrorCode::InvalidArgument,
                                 "a layer name with a comma, or none, cannot be written in a "
                                 "LAYERS list",
                                 layer);
            }
        }
        parts.emplace_back("LAYERS");
        parts.push_back(joined(words.layers, ","));
        if (!words.sublayers) {
            parts.emplace_back("ONLY");
        }
        break;
    }
    auto conditions = conditionWords(words.filter);
    if (!conditions) {
        return conditions.error();
    }
    if (!conditions->empty()) {
        parts.emplace_back("WHERE");
        parts.insert(parts.end(), conditions->begin(), conditions->end());
    }

    std::string line;
    for (const std::string& part : parts) {
        if (part.find_first_of("\"\r\n") != std::string::npos) {
            return makeError(ErrorCode::InvalidArgument,
                             "a double quote or a line break cannot be typed inside a word", part);
        }
        const bool blank = std::ranges::any_of(
            part, [](char c) { return std::isspace(static_cast<unsigned char>(c)) != 0; });
        line += (line.empty() ? "" : " ") + (blank || part.empty() ? "\"" + part + "\"" : part);
    }
    return line;
}

Result<ResolvedScope> resolveScope(const ScopeWords& words, const ScopeViewProvider& views)
{
    ResolvedScope resolved;
    resolved.words = words;
    resolved.filter = words.filter;
    ModifyScope& scope = resolved.scope;
    switch (words.source) {
    case ScopeSource::Selection:
        scope.kind = ScopeKind::Selection;
        break;
    case ScopeSource::Drawing:
        scope.kind = ScopeKind::Drawing;
        break;
    case ScopeSource::Layers:
        scope.kind = ScopeKind::Layers;
        scope.layers = words.layers;
        scope.sublayers = words.sublayers;
        break;
    case ScopeSource::Area:
        // What a view hiding nothing of its own shows in that window.
        scope.kind = ScopeKind::View;
        scope.area = words.area;
        break;
    case ScopeSource::View: {
        if (!views) {
            return makeError(ErrorCode::InvalidState,
                             "VIEW is the window's plan view, and there is none here; give the "
                             "window instead: AREA x0,y0,x1,y1");
        }
        auto view = views(words.view);
        if (!view) {
            return view.error();
        }
        resolved.viewLayers = std::make_shared<const LayerOverrides>(std::move(view->layers));
        resolved.view = view->id;
        scope.kind = ScopeKind::View;
        scope.view = resolved.viewLayers.get();
        if (!words.extents) {
            scope.area = view->area;
        }
        break;
    }
    }
    return resolved;
}

Result<ScopeView> scopeViewOf(ViewSet& views, std::optional<std::uint32_t> id)
{
    ViewState* state = nullptr;
    if (id) {
        state = views.find(*id);
        if (state == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no open view has that id; VIEW alone is the plan view in use, "
                             "and AREA x0,y0,x1,y1 names a window without one",
                             std::to_string(*id));
        }
    } else {
        state = views.mostRecent(ViewKind::Plan);
        if (state == nullptr) {
            return makeError(ErrorCode::InvalidState,
                             "VIEW is the plan view in use, and no plan view is open; open one, "
                             "or give the window instead: AREA x0,y0,x1,y1");
        }
    }
    ScopeView answer;
    answer.id = state->id;
    answer.layers = state->layers;
    if (state->kind == ViewKind::Plan) {
        answer.area = state->plan.visibleWorldBounds();
    }
    return answer;
}

Result<ScopeMatch> matchScope(const Document& document, const ScopeWords& words,
                              const ScopeViewProvider& views)
{
    auto resolved = resolveScope(words, views);
    if (!resolved) {
        return resolved.error();
    }
    auto matched = matchEntities(document, resolved->scope, resolved->filter);
    if (!matched) {
        return matched.error();
    }
    return ScopeMatch{std::move(resolved).value(), std::move(matched).value()};
}

std::string scopeRecord(const ScopeMatch& match)
{
    const ScopeWords& words = match.resolved.words;
    std::string record = "scope=" + std::string(sourceWord(words.source));
    switch (words.source) {
    case ScopeSource::View:
        if (match.resolved.view) {
            record += " view=" + std::to_string(*match.resolved.view);
        }
        // Where "what I see" was when the line ran: the view moves.
        if (match.resolved.scope.area) {
            record += " area=" + areaText(*match.resolved.scope.area);
        }
        if (words.extents) {
            record += " extents=yes";
        }
        break;
    case ScopeSource::Area:
        record += " area=" + areaText(words.area);
        break;
    case ScopeSource::Layers:
        record += " layers=" + recordValue(joined(words.layers, ",")) +
                  " sublayers=" + (words.sublayers ? "yes" : "no");
        break;
    case ScopeSource::Selection:
    case ScopeSource::Drawing:
        break;
    }
    // A filter that cannot be typed back still says what it was.
    if (const auto conditions = conditionWords(words.filter); conditions && !conditions->empty()) {
        record += " where=" + recordValue(joined(*conditions, " "));
    }
    return record + " matched=" + std::to_string(match.matched.size());
}

std::string recordValue(std::string_view value)
{
    const bool plain =
        !value.empty() && value.find_first_of(" \t\"=\\\n\r") == std::string_view::npos;
    if (plain) {
        return std::string(value);
    }
    std::string out = "\"";
    for (const char c : value) {
        if (c == '"' || c == '\\') {
            out += '\\';
        }
        out += c == '\n' || c == '\r' ? ' ' : c;
    }
    return out + "\"";
}

} // namespace katana::cad
