#include "katana/cad/symbol_assign.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <set>
#include <utility>

#include "katana/cad/style_drawing.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/table_usage.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::EntityId;
using katana::entity::Style;

namespace {

bool isSymbolRule(const katana::entity::SurveyRule& rule)
{
    return rule.section == katana::entity::SurveySection::VertexSymbol && rule.symbol &&
           !rule.symbol->style.empty();
}

// "says nothing else" - see the header comment for why each field is here
// and why the description is not.
bool saysOnlyItsSymbol(const Style& style)
{
    static const Style fresh{};
    return !style.color && katana::entity::isByLayer(style.linetype) &&
           style.hatchPattern.empty() && style.lineWeight == fresh.lineWeight;
}

std::string countOf(std::size_t count, const char* one, const char* many)
{
    return std::to_string(count) + " " + (count == 1 ? one : many);
}

} // namespace

std::vector<SymbolCode> symbolCodes(const katana::entity::SurveyMap& map, std::string_view name)
{
    std::vector<SymbolCode> codes;
    if (name.empty()) {
        return codes;
    }
    for (const katana::entity::SurveyRule& rule : map.rules()) {
        if (isSymbolRule(rule) && rule.symbol->style == name) {
            codes.push_back(
                SymbolCode{rule.key, rule.symbol->size, rule.symbol->colour, rule.comment});
        }
    }
    return codes;
}

SymbolUsers symbolUsers(const Document& document, std::string_view name)
{
    SymbolUsers users;
    if (name.empty()) {
        return users;
    }
    const katana::entity::Model& model = document.model();
    const katana::entity::TableUsage usage =
        katana::entity::tableUsage(model, katana::entity::UsageOptions{.entityIds = true});
    const katana::entity::Users& of = katana::entity::TableUsage::of(usage.symbols, name);
    users.styles = of.styles;
    for (const EntityId id : of.entityIds) {
        const katana::entity::Entity* entity = model.entities.find(id);
        if (entity != nullptr && entity->type() == katana::entity::EntityType::Point) {
            users.points.push_back(id);
        } else {
            ++users.otherEntities;
        }
    }
    return users;
}

std::vector<SymbolLibraryEntry> symbolLibrary(const Document& document)
{
    const katana::entity::SurveyMap& map = document.surveyMap();
    // Every code by the symbol it draws, in one pass over the rules: asking
    // symbolCodes per entry is a pass per entry - 800 definitions over
    // 1,600 rules on every reload.
    std::map<std::string, std::vector<SymbolCode>, std::less<>> codesOf;
    for (const katana::entity::SurveyRule& rule : map.rules()) {
        if (isSymbolRule(rule)) {
            codesOf[rule.symbol->style].push_back(
                SymbolCode{rule.key, rule.symbol->size, rule.symbol->colour, rule.comment});
        }
    }
    const auto codesFor = [&](std::string_view name) {
        const auto found = codesOf.find(name);
        return found == codesOf.end() ? std::vector<SymbolCode>{} : found->second;
    };

    std::vector<SymbolLibraryEntry> entries;
    for (CatalogueEntry& choice : symbolChoices(document)) {
        SymbolLibraryEntry entry;
        entry.codes = codesFor(choice.name);
        entry.entry = std::move(choice);
        entries.push_back(std::move(entry));
    }

    // The names nothing defines: a Style's (missingNames, which already
    // leaves out built-in names and carries the users) and a survey code's,
    // which missingNames does not look at because no Style holds it yet -
    // but the next survey import will draw it as a stand-in all the same.
    std::map<std::string, SymbolLibraryEntry, std::less<>> missing;
    for (MissingName& name : missingNames(document)) {
        if (name.role != NameRole::Symbol) {
            continue;
        }
        SymbolLibraryEntry entry;
        entry.entry.name = name.name;
        entry.entry.source = DefinitionSource::Undefined;
        entry.entry.missing = true;
        entry.entry.users = std::move(name.users);
        entry.fallback = std::move(name.fallback);
        missing.emplace(name.name, std::move(entry));
    }
    for (const katana::entity::SurveyRule& rule : map.rules()) {
        if (!isSymbolRule(rule)) {
            continue;
        }
        const std::string& name = rule.symbol->style;
        if (missing.contains(name) || document.definitionFor(name) != nullptr ||
            katana::entity::isBuiltInSymbolName(name)) {
            continue;
        }
        SymbolLibraryEntry entry;
        entry.entry.name = name;
        entry.entry.source = DefinitionSource::Undefined;
        entry.entry.missing = true;
        entry.fallback = std::string(katana::entity::builtInSymbolFor(name));
        missing.emplace(name, std::move(entry));
    }
    for (auto& [name, entry] : missing) {
        entry.codes = codesFor(name);
        entries.push_back(std::move(entry));
    }

    // symbolChoices' order - case folded, the exact name breaking a tie -
    // over the whole list, so a missing name sits where a person looks for
    // it rather than at the end.
    std::ranges::stable_sort(entries, [](const SymbolLibraryEntry& a, const SymbolLibraryEntry& b) {
        const std::string foldedA = katana::core::lowered(a.entry.name);
        const std::string foldedB = katana::core::lowered(b.entry.name);
        if (foldedA != foldedB) {
            return foldedA < foldedB;
        }
        return a.entry.name < b.entry.name;
    });
    return entries;
}

katana::geometry::Box2 estimatedTextExtent(const StyleTextMark& text)
{
    katana::geometry::Box2 box;
    if (text.text.empty() || !(std::isfinite(text.height) && text.height > 0.0)) {
        return box;
    }
    // Characters, not bytes: "Ø" is one letter on the plot and two bytes here.
    const auto characters = static_cast<double>(std::ranges::count_if(
        text.text, [](char byte) { return (static_cast<unsigned char>(byte) & 0xC0U) != 0x80U; }));
    const double factor =
        std::isfinite(text.widthFactor) && text.widthFactor > 0.0 ? text.widthFactor : 1.0;
    const double width = kEstimatedCharacterWidth * text.height * factor * characters;
    const std::string& justify = text.justify;
    // In the text's own frame: x along it from the anchor, y up from it.
    double left = 0.0;
    if (justify.find("centre") != std::string::npos ||
        justify.find("center") != std::string::npos) {
        left = -0.5 * width;
    } else if (justify.find("right") != std::string::npos) {
        left = -width;
    }
    double bottom = 0.0;
    if (justify.find("middle") != std::string::npos) {
        bottom = -0.5 * text.height;
    } else if (justify.find("top") != std::string::npos) {
        bottom = -text.height;
    }
    const double cosine = std::cos(text.angle);
    const double sine = std::sin(text.angle);
    for (const double x : {left, left + width}) {
        for (const double y : {bottom, bottom + text.height}) {
            box.expand(katana::geometry::Point2(text.at.x + x * cosine - y * sine,
                                                text.at.y + x * sine + y * cosine));
        }
    }
    return box;
}

std::optional<SymbolPrintSize> symbolPrintSize(const katana::entity::LineStyle& definition,
                                               double size, double scaleDenominator,
                                               const TextExtent& textExtent)
{
    if (!(scaleDenominator > 0.0) || !(std::isfinite(size) && size >= 0.0)) {
        return std::nullopt;
    }
    // Model units (metres) per plot millimetre at 1:N.
    const double paperScale = scaleDenominator / 1000.0;
    // Measured on the drawing a point gets, not recomputed: symbolDrawing
    // is where paper units, `factor`, the origin and a size are applied,
    // and a second copy of those rules here would be a second answer.
    const StyleDrawing drawing =
        symbolDrawing(definition, katana::geometry::Point2(0.0, 0.0), size, 0.0, paperScale);
    // Not StyleDrawing::bounds(), which takes a text's anchor for all of it:
    // a letter set 0.5 m high above a valve's box then printed as nothing,
    // and a symbol that is only a letter as 0 x 0 mm.
    katana::geometry::Box2 box;
    for (const StyleStroke& stroke : drawing.strokes) {
        for (const katana::geometry::Point2& point : stroke.path.vertices) {
            box.expand(point);
        }
    }
    for (const StyleTextMark& text : drawing.texts) {
        box.expand(textExtent ? textExtent(text) : estimatedTextExtent(text));
    }
    if (box.empty()) {
        return std::nullopt;
    }
    SymbolPrintSize printed;
    printed.groundWidth = box.width();
    printed.groundHeight = box.height();
    printed.paperWidth = printed.groundWidth / paperScale;
    printed.paperHeight = printed.groundHeight / paperScale;
    // The point the symbol is put on is the drawing's (0, 0).
    printed.insertionInside = box.min.x <= 0.0 && 0.0 <= box.max.x && box.min.y <= 0.0 &&
                              0.0 <= box.max.y;
    return printed;
}

const Style* findSymbolStyle(const katana::entity::Model& model, std::string_view symbolName,
                             double size)
{
    if (symbolName.empty()) {
        return nullptr;
    }
    const auto matches = [&](const Style& style) {
        return style.symbol == symbolName && style.symbolSize == size && saysOnlyItsSymbol(style);
    };
    // The one a person would look for first: named after the symbol.
    if (const Style* named = model.styles.find(symbolName); named != nullptr && matches(*named)) {
        return named;
    }
    const Style* first = nullptr;
    model.styles.forEach([&](const Style& style) {
        if (first == nullptr && matches(style)) {
            first = model.styles.find(style.name);
        }
    });
    return first;
}


katana::core::Result<SymbolAssignment>
assignSymbolToPoints(const Document& document, const std::vector<EntityId>& ids,
                     std::string_view symbolName, double size)
{
    if (symbolName.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no symbol to assign");
    }
    if (!(std::isfinite(size) && size >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a symbol size must be a finite number of at least 0",
                         std::to_string(size));
    }
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no entities to assign the symbol to");
    }

    const katana::entity::Model& model = document.model();
    SymbolAssignment assignment;
    std::vector<EntityId> points;
    std::set<EntityId> seen;
    for (const EntityId id : ids) {
        if (!seen.insert(id).second) {
            continue; // a selection lists an id once; a caller's list might not
        }
        const katana::entity::Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            ++assignment.notFound;
        } else if (entity->type() != katana::entity::EntityType::Point) {
            ++assignment.notPoints;
        } else {
            points.push_back(id);
        }
    }
    if (points.empty()) {
        std::string what = countOf(assignment.notPoints, "entity that is not a point",
                                   "entities that are not points");
        if (assignment.notFound > 0) {
            what += ", " + countOf(assignment.notFound, "id no entity has", "ids no entity has");
        }
        return makeError(ErrorCode::InvalidArgument, "none of the entities is a point", what);
    }

    Style style;
    if (const Style* found = findSymbolStyle(model, symbolName, size); found != nullptr) {
        style = *found;
    } else {
        style.name = freeStyleName(model, symbolName);
        style.linetype = std::string(katana::entity::kByLayerLinetype);
        style.symbol = std::string(symbolName);
        style.symbolSize = size;
        if (auto valid = katana::entity::validate(style); !valid) {
            return valid.error();
        }
        assignment.createsStyle = true;
    }
    assignment.style = style.name;

    std::vector<EntityId> moving;
    for (const EntityId id : points) {
        if (model.entities.find(id)->style == style.name) {
            ++assignment.alreadyInStyle;
        } else {
            moving.push_back(id);
        }
    }
    assignment.points = moving.size();
    if (moving.empty()) {
        // Every point already wears a style that exists: nothing to undo,
        // so no step. (A style that would be created is never worn yet.)
        return assignment;
    }

    auto transaction = std::make_unique<katana::commands::Transaction>("ASSIGN_SYMBOL");
    if (assignment.createsStyle) {
        transaction->add(katana::commands::createStyle(style));
    }
    transaction->add(katana::commands::setEntityStyle(std::move(moving), style.name));
    assignment.command = std::move(transaction);
    return assignment;
}

katana::commands::CommandPtr replaceSymbolInStyles(const katana::entity::Model& model,
                                                   std::string_view from, std::string_view to)
{
    if (from.empty() || from == to) {
        return nullptr;
    }
    auto transaction = std::make_unique<katana::commands::Transaction>("REPLACE_SYMBOL");
    model.styles.forEach([&](const Style& style) {
        if (style.symbol == from) {
            Style changed = style;
            changed.symbol = std::string(to);
            transaction->add(katana::commands::updateStyle(std::move(changed)));
        }
    });
    if (transaction->size() == 0) {
        return nullptr;
    }
    return transaction;
}

} // namespace katana::cad
