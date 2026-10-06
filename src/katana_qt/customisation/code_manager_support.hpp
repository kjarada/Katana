#pragma once

// Small pieces the survey code manager's tabs share: how a linestyle or
// symbol name a rule gives stands against the loaded library, a colour
// swatch, and the colour-name field. The last section is shared with the
// symbol library too, since both managers read and write Katana customisation
// files: the filter their file dialogs offer, the ONE rule for what a manager
// writes of a session and takes of a file's sources, the name a
// customisation written from a session goes under, and the writing of a file.

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

// The two kinds of thing a customisation holds that a manager deals in: the
// Survey Code Manager its survey code rules, the Symbol Library its linestyle
// and symbol definitions.
enum class CustomisationPart { Codes, Definitions };

// Of `sources` - a customisation's own list of what went into it - those that
// stand beside ONE part of it, each as what it brought to that part:
//
//   it brought the part's kind    kept, and said to have brought that kind
//                                 ALONE: its other kind is not there
//   it brought the other kind     dropped, its notice with it: nothing of it
//   alone                         is there
//   it brought neither (a table   kept only `withColours`, when the part
//   of colours)                   carries colours it may have brought
//
// Both directions ask this, which is why it is one function: what Export
// writes beside the rules or the definitions, and what Import Definitions
// hands the merge beside a file's definitions. A source left listed with
// nothing of it taken passes for LOADED - Document::installCustomisation
// takes every listed name off what the open project is missing - so a
// project's warning that a customisation is missing would go away with not
// one of its rules in the session.
[[nodiscard]] std::vector<katana::entity::CustomisationSourceNote>
sourcesOfPart(std::vector<katana::entity::CustomisationSourceNote> sources,
              CustomisationPart part, bool withColours);

// What a manager writes of `session` (Document::customisation(), with the
// name it is to go under and, for Codes, the rules being edited in place of
// its map): the ONE rule for Export Codes and Export Selected.
//
//   name, description,  the session's, as they are. A notice is "carried
//   notice, basedOn     with the data and shown to whoever uses it"
//                       (entity/customisation.hpp): a part of a customisation
//                       is its author's data too
//   sources             sourcesOfPart: those that brought this kind, each
//                       with its own notice
//   colours             those of the session's that what is written NAMES - a
//                       rule's colour, its symbol's and its text's; a
//                       definition's pens - compared as colour names are
//                       (entity::foldColourName). Without them the part would
//                       draw elsewhere in other colours than here; the rest
//                       would overwrite colours of whoever loads it that
//                       nothing in the file uses
//   linework,           absent: the file says nothing of them, so loading it
//   automation          leaves the session's own alone. "A file of symbols
//                       for a colleague must not reset their control codes"
//                       (entity/customisation.hpp), nor a file of codes
//   the other kind      absent
//
// For Definitions, `only` names the definitions wanted - every one when it is
// empty; a name the library lacks is simply not there, and the caller that
// must not lose one silently passes the same names to the writer
// (CustomisationWriteOptions::only), which refuses.
//
// It reads nothing but the value, so it can sit beside a CUSTOMISE EXPORT
// verb unchanged the day the buttons run that line instead.
[[nodiscard]] katana::entity::Customisation
exportedPart(katana::entity::Customisation session, CustomisationPart part,
             const std::vector<std::string>& only = {});

// `bytes` as the whole of the file at `path`. FileExportFailure, with the
// path beside it, when it cannot be opened or written.
[[nodiscard]] katana::core::Status writeFileBytes(const std::filesystem::path& path,
                                                  std::string_view bytes);

} // namespace katana::qt
