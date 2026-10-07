#pragma once

// Applying a survey code to a drawing (PLAN.MD 20.3, slice 4).
//
// This is what the three files are FOR. An entity carries a field code -
// `WM01` - and the mapfile says a water main goes in model SURVEY SERVICES,
// coloured "sui water potable", drawn with the "WATR Main" linestyle. This
// turns that into the layer, the style and the properties the entity should
// have, as ONE undoable command.
//
// It plans rather than acts: everything it decides comes back as a
// `commands::Transaction`, so it is undoable in one step, and a report says
// what it did and - just as important - which codes the mapfile had no rule
// for. A code silently left alone looks exactly like a code that was handled.
//
// The colour is resolved through a callback because the standard colour
// NAMES are known to `archive12d`, which `cad` may not see. The front ends pass
// `archive12d::standardColour`. A name the callback does not know leaves the
// entity's colour alone rather than guessing at one.
//
// WHICH STYLE A CODE GETS (the lead's decision D4). What a code looks like is
// its APPEARANCE: the linestyle (or a plain line), the symbol, the symbol's
// size and the colour. In the reference mapfile every symbol code also says
// linestyle "0", and "0" comes with 18 different colours, so naming a style
// after the linestyle alone put every one of those codes on ONE style "0",
// with whichever entity came first deciding its symbol and colour for all.
// Instead:
//  1. an existing style with exactly that appearance is REUSED, whatever it is
//     called or says in its description - so renaming a coded style and
//     applying codes again does not make a second one. A colour name the
//     callback does not know has no RGB, so any style with no colour of its
//     own draws it; one described by that name is preferred, and one
//     described by another such name of the map is left to that name's
//     codes;
//  2. otherwise a style is created, named after the linestyle, else the
//     symbol, else "Plain"; a name another appearance already holds gets
//     " (<colour name>)" and then " 2", " 3" - decided over the appearances
//     in a fixed order, so the names follow from the rules and never from
//     which entity came first.

#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

struct SurveyCodingOptions {
    // The entity property holding the field code. EMPTY means find it: the
    // first of `codePropertyCandidates()` that any entity actually carries,
    // which the report names. A drawing from an archive import carries the
    // string name in "12d.name" and a survey file's points carry "code", and
    // asking the caller which is asking them to know how the drawing got here.
    std::string property{};
    // Only these entities, or every entity when empty.
    std::vector<katana::entity::EntityId> ids{};
    // Standard colour name -> RGB. Without one, colours are left alone.
    std::function<std::optional<katana::entity::Color>(std::string_view)> colourOf{};
    // Off, an entity whose rule names a model the drawing has no layer for
    // keeps its layer - its style and attributes are still applied - and is
    // counted in `skippedNoLayer`.
    bool createLayers = true;
    // Off, an entity whose appearance no existing style has keeps its style
    // and is counted in `skippedNoStyle`.
    bool createStyles = true;
    // Attach the attributes the mapfile gives as entity properties. A value
    // that names another attribute ("$PipeDiameter") is skipped: resolving it
    // needs the survey data this drawing was made from, which is not here.
    bool setAttributes = true;
};

// Where a field code is looked for, in order, when none is named.
[[nodiscard]] const std::vector<std::string>& codePropertyCandidates();

// ---- string names --------------------------------------------------------------------
//
// A point's STRING NAME is its code followed by its string number: "KJ01" is
// string 01 of code KJ, and the rules are keyed on that name ("KJ*", and
// "B1*" for the strings 10 to 19 of a one-letter code B). A delimited list
// writes the name whole in its code column. A field file whose records keep
// the two apart (a code column and a string column) gives a point the code
// alone, and the number comes into the drawing beside it, in this property -
// as the file's own metadata key when it has one, else from the feature that
// strings the point (survey_finish.hpp, nameSurveyStrings).
//
// An entity WITHOUT the property is read exactly as before it existed.
inline constexpr std::string_view kSurveyStringProperty = "string";

// The string number `entity` carries: the text under kSurveyStringProperty,
// less blanks at either end; empty when it has none or holds something that
// is not text (a number there is some other program's field, not a string
// number this import wrote).
[[nodiscard]] std::string_view surveyStringOf(const katana::entity::Entity& entity);

// The name `code` is looked up by when its entity carries the string number
// `string`: the string name (`code` followed by `string`) when a rule MORE
// SPECIFIC than the bare "*" answers it, else `code` alone.
//
// The name first, because that is what the keys are written for: code "B" in
// string "12" is "B12", which "B1*" answers and "B" never could. The code
// alone second, so that numbering a string never LOSES a rule the code had -
// an exact key "PTBB" still answers PTBB in string 3, whose name "PABB3" it
// does not match - and so that a code nobody wrote a rule for is reported
// once, as itself, rather than once per string. For the same reason the code
// is also the answer when the name's best rule is a prefix SHORTER than the
// code ("PA*", a rule for every code that begins PA) and the code has an
// exact key of its own: that key is the more specific rule for it, numbered
// or not. With no `string` the answer is `code`; with no `code` there is no
// name and the answer is empty.
[[nodiscard]] std::string surveyLookupName(const katana::entity::SurveyMap& map,
                                           std::string_view code, std::string_view string);

// The code an entity carries under `property`, or nullptr. Only TEXT is a
// code: a number there is a measurement someone named badly, and treating
// "1.5" as a field code would put it in whatever model the rule for `1*`
// names. The entity's properties are asked first and its METADATA after,
// because an archive import records a string's name - its code, in a coded
// survey - as provenance ("12d.name"), not as a property.
[[nodiscard]] const std::string* surveyCodeOf(const katana::entity::Entity& entity,
                                              const std::string& property);

// Which of `codePropertyCandidates()` any of `subject` actually carries as
// text; the first candidate when none does, so a report can still name what
// was looked for.
[[nodiscard]] std::string findCodeProperty(const katana::entity::Model& model,
                                           const std::vector<katana::entity::EntityId>& subject);

// The plain continuous line: "0", "1", "continuous" in any case, or no name
// at all. Such a linestyle names no definition and draws as a plain line.
[[nodiscard]] bool isPlainLinestyle(std::string_view name);

// What happened to the style of one code's entities.
enum class SurveyStyleOutcome {
    None,    // the code's rules give no appearance, so its style is left alone
    Reused,  // an existing style has exactly this appearance
    Created, // a new style was planned for it
    Skipped, // no style has it and createStyles is off
};

[[nodiscard]] const char* toString(SurveyStyleOutcome outcome);

// One distinct code: what the map made of it and what that did to the
// entities carrying it.
struct SurveyCodeRow {
    std::string code{};
    std::size_t entities = 0; // carrying this code
    katana::entity::SurveyMatchKind kind = katana::entity::SurveyMatchKind::None;
    bool matched = false; // SurveyMatch::matched(): decision D5
    // The layers those entities were on, distinct and in name order, and the
    // one the rule gives (empty: it gives none, so they stay where they are).
    std::vector<std::string> layersFrom{};
    std::string layer{};
    // Of `entities`, those that stayed where they were because `layer` does
    // not exist and layers are not being created (they are also counted in
    // SurveyCodingReport::skippedNoLayer). Kept per code so that a row never
    // reads as a move that did not happen.
    std::size_t layerKept = 0;
    std::string style{}; // the style they get; empty with SurveyStyleOutcome::None
    SurveyStyleOutcome styleOutcome = SurveyStyleOutcome::None;
    // Attribute NAMES, in the order the combined rule lists them.
    std::vector<std::string> attributesSet{};
    std::vector<std::string> attributesDeferred{}; // their value names another
    std::size_t changed = 0; // of `entities`, those this alters
};

struct SurveyCodingReport {
    // The property the codes were actually read from - what was asked for, or
    // what was found. Reported because "0 entities carry a code" is a
    // different problem from "they carry it under another name".
    std::string property{};
    std::size_t coded = 0;   // entities carrying a code at all
    std::size_t matched = 0; // ... whose code the map has a rule for (D5)
    // ... whose code only the bare `*` answers, with no model, linestyle or
    // symbol: in practice a code nobody wrote a rule for (audit CAD-05). Its
    // `*` attributes are still applied - that rule does say every code gets
    // them - but it is not counted as matched.
    std::size_t fallbackOnly = 0;
    // Entities this alters in any way - layer, style or a property - each
    // counted once.
    std::size_t changed = 0;
    std::size_t skippedNoLayer = 0; // kept their layer: createLayers is off
    std::size_t skippedNoStyle = 0; // kept their style: createStyles is off
    std::vector<std::string> unmatchedCodes{};    // no rule at all; distinct, in name order
    std::vector<std::string> fallbackOnlyCodes{}; // distinct, in name order
    std::vector<std::string> layersCreated{};     // in name order
    std::vector<std::string> stylesCreated{};     // in name order
    std::vector<std::string> stylesReused{};      // existing styles given to codes, in name order
    // Linestyles and symbols the rules of the codes present name that the
    // loaded library does not define - checked for every code, not only for
    // one that creates a style. They are still set on the style - the name is
    // what the rules record - and they draw as a plain line or mark until a
    // library defines them. Plain lines and Katana's built-in symbol shapes
    // are never listed: they draw correctly without a library.
    std::vector<std::string> missingDefinitions{};
    // Attributes skipped because their value names another attribute, one
    // per entity and attribute.
    std::size_t deferredAttributes = 0;
    // One row per distinct code, matched or not, in code order.
    std::vector<SurveyCodeRow> codes{};
};

// How much of THIS drawing the loaded customisation actually answers for.
//
// It exists because "the linestyles are not showing" has several causes that
// look identical from the outside - no customisation loaded, a customisation
// that does not define what this drawing names, or a drawing whose styles are
// plain continuous lines - and a person cannot tell them apart by looking. This
// says which.
struct CustomisationCoverage {
    std::size_t styles = 0;   // styles the drawing has
    std::size_t named = 0;    // ... that name a linestyle or a symbol
    std::size_t resolved = 0; // ... that the loaded library defines
    // ... whose only name is a symbol Katana draws itself ("cross",
    // "manhole") or a linetype of the drawing's own, and no library defines:
    // drawn correctly, so neither missing nor counted as named (audit CAD-17).
    std::size_t builtIn = 0;
    // The names the styles give whose drawing is a fallback, by
    // cad::linetypeStatus and cad::symbolStatus - the rule cad::missingNames
    // reports by (style_catalogue.hpp) - split by its two reasons, because
    // each asks for a different fix. So never a built-in symbol, ByLayer, a
    // plain line or a style's own symbol as its linetype (D8). Distinct and
    // in name order; the styles' names only - a layer's missing linetype is
    // in missingNames, not here.
    //
    // Undefined: in no loaded library, so the fix is to load one that
    // defines it. The window and CUSTOMISE print these as exactly that.
    std::vector<std::string> unresolved{};
    // NotALinestyle: a linetype naming only a `mode vertex` definition, which
    // the viewport draws solid (D2). The library IS loaded and does define
    // the name, as a symbol, so the fix is to pick a linestyle instead; a
    // name here is never in `unresolved`, since symbolStatus finds any
    // library definition.
    std::vector<std::string> notLinestyles{};
};

[[nodiscard]] CustomisationCoverage customisationCoverage(const Document& document);

// nullptr with no error when there is nothing to do - no entity carries a
// code the map has a rule for, or every coded entity already has what its
// rule gives - so a caller can tell "nothing to do" from "something went
// wrong". Fails only on a rule that cannot be turned into a valid layer or
// style.
//
// A POINT is coded by the string name of its field code, its linework
// controls left out (parseFieldCode in linework.hpp): "PABB ST" is coded as
// PABB, and reported under PABB. Any other entity's code is looked up whole.
// Either is followed by the entity's string number when it carries one
// (kSurveyStringProperty, surveyLookupName): a point coded "B" in string
// "12" is coded, and reported, as B12.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
applySurveyCodes(const Document& document, const SurveyCodingOptions& options,
                 SurveyCodingReport* report = nullptr);

} // namespace katana::cad
