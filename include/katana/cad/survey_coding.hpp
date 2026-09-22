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
// The colour is resolved through a callback because 12d's colour NAMES are
// known to `archive12d`, which `cad` may not see. The front ends pass
// `archive12d::standardColour`. A name the callback does not know leaves the
// entity's colour alone rather than guessing at one.

#include <functional>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad {

struct SurveyCodingOptions {
    // The entity property holding the field code. EMPTY means find it: the
    // first of `codePropertyCandidates()` that any entity actually carries,
    // which the report names. A drawing imported from a 12da carries the
    // string name in "12d.name" and a survey file's points carry "code", and
    // asking the caller which is asking them to know how the drawing got here.
    std::string property{};
    // Only these entities, or every entity when empty.
    std::vector<katana::entity::EntityId> ids{};
    // 12d colour name -> RGB. Without one, colours are left alone.
    std::function<std::optional<katana::entity::Color>(std::string_view)> colourOf{};
    // A rule naming a model or a linestyle the drawing has no layer or style
    // for creates one. Off, such an entity keeps what it has and is counted.
    bool createLayers = true;
    bool createStyles = true;
    // Attach the attributes the mapfile gives as entity properties. A value
    // that names another attribute ("$PipeDiameter") is skipped: resolving it
    // needs the survey data this drawing was made from, which is not here.
    bool setAttributes = true;
};

// Where a field code is looked for, in order, when none is named.
[[nodiscard]] const std::vector<std::string>& codePropertyCandidates();

struct SurveyCodingReport {
    // The property the codes were actually read from - what was asked for, or
    // what was found. Reported because "0 entities carry a code" is a
    // different problem from "they carry it under another name".
    std::string property{};
    std::size_t coded = 0;   // entities carrying a code at all
    std::size_t matched = 0; // ... that the mapfile had a rule for
    std::size_t changed = 0; // ... that this actually alters
    std::vector<std::string> unmatchedCodes{};    // distinct, in name order
    std::vector<std::string> layersCreated{};     // in name order
    std::vector<std::string> stylesCreated{};     // in name order
    // Linestyles and symbols the rules name that the loaded library does not
    // define. They are still set on the style - the name is what 12d records
    // - and they draw as a plain line or mark until a library defines them.
    std::vector<std::string> missingDefinitions{};
    // Attributes skipped because their value names another attribute.
    std::size_t deferredAttributes = 0;
};

// How much of THIS drawing the loaded customisation actually answers for.
//
// It exists because "the linestyles are not showing" has several causes that
// look identical from the outside - no customisation loaded, a customisation
// that does not define what this drawing names, or a drawing whose styles are
// 12d's plain lines - and a person cannot tell them apart by looking. This
// says which.
struct CustomisationCoverage {
    std::size_t styles = 0;   // styles the drawing has
    std::size_t named = 0;    // ... that name a linestyle or a symbol
    std::size_t resolved = 0; // ... that the loaded library defines
    // The names that resolve to nothing, distinct and in name order.
    std::vector<std::string> unresolved{};
};

[[nodiscard]] CustomisationCoverage customisationCoverage(const Document& document);

// nullptr with no error when there is nothing to do - no entity carries a
// code the map has a rule for - so a caller can tell "nothing to do" from
// "something went wrong". Fails only on a rule that cannot be turned into a
// valid layer or style.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
applySurveyCodes(const Document& document, const SurveyCodingOptions& options,
                 SurveyCodingReport* report = nullptr);

} // namespace katana::cad
