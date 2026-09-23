#pragma once

// Small pieces the survey code manager's tabs share: how a linestyle or
// symbol name a rule gives stands against the loaded library, a colour
// swatch, and the 12d colour-name field. Internal to the manager's sources.

#include <string>
#include <string_view>

#include <QIcon>
#include <QString>

#include "katana/entity/entity.hpp"

class QComboBox;

namespace katana::cad {
class Document;
}

namespace katana::qt {

// What a name in a rule draws as.
enum class DefinitionState {
    Plain,     // a linestyle that is 12d's plain line: "0", "1", "continuous"
    Defined,   // the library defines it (as the right kind, for a linestyle)
    BuiltIn,   // a symbol no library defines that Katana draws itself
    WrongKind, // a linestyle naming a `mode vertex` definition: draws solid (D2)
    Undefined, // nothing defines it
};

// `document` may be null (the drawing has closed): then nothing is defined.
[[nodiscard]] DefinitionState linestyleState(const katana::cad::Document* document,
                                             std::string_view name);
[[nodiscard]] DefinitionState symbolState(const katana::cad::Document* document,
                                          std::string_view name);
// "TEST Water Main", "0 (plain line)", "Old Kerb (not defined)".
[[nodiscard]] QString definitionLabel(const std::string& name, DefinitionState state);

// A filled square of `colour`, `size` pixels a side, with a thin border so
// white and black read on either ground.
[[nodiscard]] QIcon colourSwatch(const katana::entity::Color& colour, int size);

// Makes `box` the 12d colour-name field: editable, listing
// archive12d::standardColourNames() with swatches, and taking any typed name
// as it is.
void makeColourField(QComboBox* box);
// Shows `name` in such a field, listed or not, without changing its case.
void setColourField(QComboBox* box, const std::string& name);

} // namespace katana::qt
