#include "katana/cad/definition_users.hpp"

#include <algorithm>
#include <iterator>

#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

namespace {

// Whether `used` - a name a rule, a style or a layer gives - is drawn with the
// definition `name`, as findDefinition would answer: the name itself, or, for
// a name the library has no definition of, the earlier name of a definition
// renamed to `name`. A library that does define `used` draws that definition,
// and so does not use this one.
bool names(const katana::entity::StyleLibrary& library, std::string_view used,
           std::string_view name, std::span<const RenamedDefinition> renamed)
{
    if (used == name) {
        return true;
    }
    return !used.empty() && !library.contains(used) && definitionNameNow(used, renamed) == name;
}

// `style "Marks" draws it as its symbol`. Built by appending: a chain of
// operator+ on temporaries sets off a false -Wrestrict in GCC's basic_string
// at -O3.
std::string userLine(std::string_view kind, std::string_view name, std::string_view how)
{
    std::string line(kind);
    line += " \"";
    line += name;
    line += "\" ";
    line += how;
    return line;
}

} // namespace

std::vector<std::string> DefinitionUsers::describe() const
{
    std::vector<std::string> lines;
    for (const DefinitionRule& rule : rules) {
        // "rule #2 KT* (feature)", as an explanation and the lint cite a rule.
        std::string line = "rule #";
        line += std::to_string(rule.index);
        line += ' ';
        line += rule.key;
        line += " (";
        line += katana::entity::toString(rule.section);
        line += ") ";
        if (rule.linestyle && rule.symbol) {
            line += "names it as its linestyle and its symbol";
        } else if (rule.linestyle) {
            line += "names it as its linestyle";
        } else {
            line += "draws it as its symbol";
        }
        lines.push_back(std::move(line));
    }
    for (const std::string& style : linetypeStyles) {
        lines.push_back(userLine("style", style, "names it as its linetype"));
    }
    for (const std::string& style : symbolStyles) {
        lines.push_back(userLine("style", style, "draws it as its symbol"));
    }
    for (const std::string& layer : layers) {
        lines.push_back(userLine("layer", layer, "names it as its linetype"));
    }
    return lines;
}

std::vector<std::string> DefinitionUsers::styles() const
{
    std::vector<std::string> named;
    // Both lists are ascending, as the header promises, so this is the union
    // in the same order.
    std::set_union(linetypeStyles.begin(), linetypeStyles.end(), symbolStyles.begin(),
                   symbolStyles.end(), std::back_inserter(named));
    return named;
}

std::vector<std::string> DefinitionUsers::cited(std::string_view name) const
{
    const std::string of = "\"" + std::string(name) + "\": ";
    std::vector<std::string> lines;
    for (const DefinitionRule& rule : rules) {
        lines.push_back(of + "rule #" + std::to_string(rule.index) + " " + rule.key + " (" +
                        std::string(katana::entity::toString(rule.section)) + ") names it");
    }
    for (const std::string& style : styles()) {
        lines.push_back(of + "the drawing's style \"" + style + "\" names it");
    }
    for (const std::string& layer : layers) {
        lines.push_back(of + "the drawing's layer \"" + layer + "\" names it");
    }
    return lines;
}

bool DefinitionUsers::namedAsLinetype() const
{
    return !linetypeStyles.empty() || !layers.empty() ||
           std::ranges::any_of(rules, [](const DefinitionRule& rule) { return rule.linestyle; });
}

bool DefinitionUsers::namedAsSymbol() const
{
    return !symbolStyles.empty() ||
           std::ranges::any_of(rules, [](const DefinitionRule& rule) { return rule.symbol; });
}

DefinitionUsers definitionUsers(const Document& document, std::string_view name)
{
    return definitionUsers(document, name, builtinDefinitionRenames());
}

DefinitionUsers definitionUsers(const Document& document, std::string_view name,
                                std::span<const RenamedDefinition> renamed)
{
    DefinitionUsers users;
    if (name.empty()) {
        return users;
    }
    const katana::entity::StyleLibrary& library = document.styleLibrary();
    const auto uses = [&](std::string_view used) { return names(library, used, name, renamed); };

    const std::vector<katana::entity::SurveyRule>& rules = document.surveyMap().rules();
    for (std::size_t index = 0; index < rules.size(); ++index) {
        const katana::entity::SurveyRule& rule = rules[index];
        const bool linestyle = uses(rule.linestyle);
        const bool symbol = rule.symbol.has_value() && uses(rule.symbol->style);
        if (linestyle || symbol) {
            users.rules.push_back(DefinitionRule{index, rule.key, rule.section, linestyle, symbol});
        }
    }
    // Both tables are walked in name order, which is the order the lists
    // promise.
    const katana::entity::Model& model = document.model();
    users.drawingLinetype = model.linetypes.contains(name);
    users.builtInShape = katana::entity::isBuiltInSymbolName(name);
    model.styles.forEach([&](const katana::entity::Style& style) {
        if (uses(style.linetype)) {
            users.linetypeStyles.push_back(style.name);
        }
        if (uses(style.symbol)) {
            users.symbolStyles.push_back(style.name);
        }
    });
    for (const std::string& layerName : model.layers.names()) {
        const katana::entity::Layer* layer = model.layers.find(layerName);
        if (layer != nullptr && uses(layer->linetype)) {
            users.layers.push_back(layerName);
        }
    }
    return users;
}

} // namespace katana::cad
