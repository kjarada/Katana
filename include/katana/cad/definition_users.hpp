#pragma once

// Who uses a library definition, by name: what is left naming nothing once
// the definition is removed.
//
// A linestyle or symbol definition lives in the session's customisation
// (Document::styleLibrary), and three things name it from outside:
//
//   * a survey code rule, as its linestyle or as the symbol it draws;
//   * a style of the drawing, as its linetype or as its symbol;
//   * a layer of the drawing, as its linetype.
//
// A name is resolved when it is DRAWN (cad/style_resolver.hpp), so removing a
// definition breaks none of them: the rule and the style go on naming it, and
// the line is drawn solid, or the point as a stand-in shape, with nothing
// said. That is why a removal asks here first. CUSTOMISE REMOVE refuses a
// definition this finds a user of until it is given FORCE, and the window's
// definition editor shows the same list beside its Delete - one function, so
// what a person reads in the window is what an agent reads in a reply.
//
// What counts as naming it is what the resolver draws with it
// (findDefinition): the name itself, and - only for a name the library does
// not hold - the earlier name of a definition that was renamed, so a drawing
// saved before the rename is not passed over.

#include <cstddef>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/customisation_record.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

// One survey code rule that names a definition. A rule has no identity but
// its place (entity/survey_map.hpp), so it is cited as everything else cites
// one: by its index, with its key and what it is about.
struct DefinitionRule {
    std::size_t index = 0; // into SurveyMap::rules()
    std::string key{};     // as written: "WM*"
    katana::entity::SurveySection section = katana::entity::SurveySection::Map;
    bool linestyle = false; // SurveyRule::linestyle names it
    bool symbol = false;    // SurveyRule::symbol names it

    friend bool operator==(const DefinitionRule&, const DefinitionRule&) = default;
};

struct DefinitionUsers {
    std::vector<DefinitionRule> rules{};       // in the map's order
    std::vector<std::string> linetypeStyles{}; // styles whose linetype names it, ascending
    std::vector<std::string> symbolStyles{};   // styles whose symbol names it, ascending
    std::vector<std::string> layers{};         // layers whose linetype names it, ascending

    // What ELSE answers to the name, and so what its users are drawn with
    // while no library definition does (cad/style_resolver.hpp):
    //   * the drawing's own Linetype table holds it (decision D2): a line
    //     naming it is then dashed by that linetype, not drawn plain;
    //   * it is the name of a built-in symbol shape: a point naming it is
    //     then drawn as that shape, not as a stand-in for a missing one.
    // Neither is a user - empty() does not count them and describe() lists
    // neither. They are what a removal hands the users to, and what a
    // definition made under the name takes them from: a refusal that said
    // "then drawn plain" of a name the drawing also holds would be untrue.
    bool drawingLinetype = false;
    bool builtInShape = false;

    [[nodiscard]] bool empty() const
    {
        return rules.empty() && linetypeStyles.empty() && symbolStyles.empty() && layers.empty();
    }
    // A line a user, in the order above, for a refusal a person can act on:
    //   rule #0 WM* (feature) names it as its linestyle
    //   rule #7 AC* (symbol) draws it as its symbol
    //   style "Water" names it as its linetype
    //   style "Marks" draws it as its symbol
    //   layer "survey/services" names it as its linetype
    // A rule naming it both ways says "as its linestyle and its symbol".
    [[nodiscard]] std::vector<std::string> describe() const;
    // Something names it as a linetype - a rule's linestyle, a style's or a
    // layer's linetype - or as a symbol: which of the two notes above matter.
    [[nodiscard]] bool namedAsLinetype() const;
    [[nodiscard]] bool namedAsSymbol() const;

    friend bool operator==(const DefinitionUsers&, const DefinitionUsers&) = default;
};

// Everything in `document` that names the definition `name`. Any rule may
// carry a linestyle or a symbol, whatever it is about (the model allows it,
// and SurveyMap::stylesReferenced reads them so), so every rule is asked.
// Names compare with regard to case, as they do wherever one is resolved.
// Nothing for "": no name is not a name. The two-argument form uses
// builtinDefinitionRenames.
[[nodiscard]] DefinitionUsers definitionUsers(const Document& document, std::string_view name);
[[nodiscard]] DefinitionUsers definitionUsers(const Document& document, std::string_view name,
                                              std::span<const RenamedDefinition> renamed);

} // namespace katana::cad
