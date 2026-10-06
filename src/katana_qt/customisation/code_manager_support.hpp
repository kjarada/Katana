#pragma once

// Small pieces the survey code manager's tabs share: how a linestyle or
// symbol name a rule gives stands against the loaded library, a colour
// swatch, and the colour-name field. The last section is shared with the
// symbol library too, since both managers read and write Katana customisation
// files: the filter their file dialogs offer, which of a file's sources an
// import of its definitions takes, the name a customisation written from a
// session goes under, and the writing of a file.

#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include <QIcon>
#include <QString>

#include "katana/core/error.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/entity.hpp"

class QComboBox;

namespace katana::cad {
class Document;
}

namespace katana::qt {

// What a name in a rule draws as.
enum class DefinitionState {
    Plain,     // a linestyle that is the plain continuous line: "0", "1", "continuous"
    Defined,   // the library defines it (as the right kind, for a linestyle)
    BuiltIn,   // a symbol no library defines that Katana draws itself
    WrongKind, // a linestyle naming a definition drawn at vertices: draws solid (D2)
    Undefined, // nothing defines it
};

// `document` may be null (the drawing has closed): then nothing is defined.
[[nodiscard]] DefinitionState linestyleState(const katana::cad::Document* document,
                                             std::string_view name);
[[nodiscard]] DefinitionState symbolState(const katana::cad::Document* document,
                                          std::string_view name);
// "TEST Water Main", "0 (plain line)", "Old Kerb (not defined)".
[[nodiscard]] QString definitionLabel(const std::string& name, DefinitionState state);

// The colour `name` means in `document`: its customisation's own names, then
// the standard ones (cad::resolveColour). With no document (the drawing has
// closed) the standard names, which need none. nullopt: neither knows it.
[[nodiscard]] std::optional<katana::entity::Color>
colourOf(const katana::cad::Document* document, std::string_view name);

// A filled square of `colour`, `size` pixels a side, with a thin border so
// white and black read on either ground.
[[nodiscard]] QIcon colourSwatch(const katana::entity::Color& colour, int size);

// Makes `box` a colour-name field: editable, listing the names the
// document's customisation defines and then entity::standardColourNames(),
// each with its swatch, and taking any typed name as it is. `document` may
// be null: the standard names alone.
void makeColourField(QComboBox* box, const katana::cad::Document* document);
// Lists the names again - the customisation's colours have changed - keeping
// what the field shows and telling no one: the rule in the form is as it was.
void refreshColourField(QComboBox* box, const katana::cad::Document* document);
// Shows `name` in such a field, listed or not, without changing its case.
void setColourField(QComboBox* box, const std::string& name);

// ---- Katana customisation files (both managers) ------------------------------------

// What a file dialog offers for one: "Katana customisation
// (*.customisation.json *.json)".
[[nodiscard]] QString customisationFileFilter();

// The name a customisation written from this session goes under: the
// session's own. A session nothing was loaded into has none, and a file must
// have one (entity::validateCustomisationName), so it is then the file's own
// name without `.customisation.json` or `.json` - "kerbs" for
// kerbs.customisation.json - and "customisation" where even that is no name.
[[nodiscard]] std::string exportedCustomisationName(const katana::cad::Document* document,
                                                    const std::filesystem::path& file);

// What a manager WRITES of a session is not here: Export Codes and Export
// Selected cut the session down by cad::customisationPart
// (katana/cad/customisation_part.hpp), the one rule CUSTOMISE EXPORT writes a
// part by too.

// Of `sources` - the list a FILE gives of what went into it - those an
// import of the file's DEFINITIONS hands the merge, each as what it brought
// to them:
//
//   it brought definitions        kept, and said to have brought those
//                                 ALONE: its rules are not taken
//   it brought rules alone        dropped, its notice with it: nothing of it
//                                 is taken
//   it brought neither (a table   kept only `withColours`, when the import
//   of colours)                   takes colours it may have brought
//
// A source left listed with nothing of it taken passes for LOADED -
// Document::installCustomisation takes every listed name off what the open
// project is missing - so a project's warning that a customisation is missing
// would go away with not one of its rules in the session.
//
// It goes by what the file SAYS each source brought - the import takes every
// definition of the file - where a part being WRITTEN goes by where each
// written definition came from (cad::customisationPart).
[[nodiscard]] std::vector<katana::entity::CustomisationSourceNote>
sourcesOfImportedDefinitions(std::vector<katana::entity::CustomisationSourceNote> sources,
                             bool withColours);

// `bytes` as the whole of the file at `path`. FileExportFailure, with the
// path beside it, when it cannot be opened or written.
[[nodiscard]] katana::core::Status writeFileBytes(const std::filesystem::path& path,
                                                  std::string_view bytes);

} // namespace katana::qt
