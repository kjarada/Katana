#pragma once

// Small pieces the survey code manager's tabs share: how a linestyle or
// symbol name a rule gives stands against the loaded library, a colour
// swatch, and the colour-name field. The last section is shared with the
// symbol library too, since both managers read and write Katana customisation
// files, through CUSTOMISE lines: the filter their file dialogs offer, the name
// a customisation written from a session goes under, and a file as a word of
// such a line.

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
// Selected hand the line CUSTOMISE EXPORT to the window's executor, which cuts
// the session down by cad::customisationPart and writes the file by one
// writer (temp + rename); the Symbol Library's Import Definitions hands it
// CUSTOMISE DEFINITIONS (cad::definitionsOfFile says what that takes).

// `path` as one word of a CUSTOMISE line, in quotes and with '/'. A name with
// no directory is given one ("./keep"): the family reads a line's whole FIRST
// word as a keyword, quotes or none - the tokenizer takes them off - so a file
// called `keep` would run KEEP, and EXPORT refuses a file called as one of its
// own words. Every bare name is, so that no list of the family's words is kept
// in a dialog to go stale. What no line can carry - a double quote, a line
// break: the tokenizer's quoted words have no escape - is refused by the one
// rule every dialog writes a word by (command_word.hpp).
[[nodiscard]] katana::core::Result<QString> customisationFileWord(const QString& path);
[[nodiscard]] katana::core::Result<QString>
customisationFileWord(const std::filesystem::path& path);

// `bytes` as the whole of the file at `path`, for a file that is NOT a
// customisation (the code list's CSV): a customisation file is written by
// CUSTOMISE EXPORT, which writes beside the file and renames, so that a write
// that fails part way leaves what was there. FileExportFailure, with the path
// beside it, when it cannot be opened or written.
[[nodiscard]] katana::core::Status writeFileBytes(const std::filesystem::path& path,
                                                  std::string_view bytes);

} // namespace katana::qt
