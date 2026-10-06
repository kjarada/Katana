#pragma once

// What editing ONE library definition needs below the window: its strokes as
// the text an editor shows and takes back, the name a copy starts under, and
// the command line that removes it.
//
// The window's definition editor (src/katana_qt/customisation/
// definition_editor.hpp) is a form over entity::LineStyle with the strokes in
// a text box. Everything about that text that is not a widget's business is
// here, where it is tested without one: which line a refusal is about, what a
// person is told of it, and what a line may not hold.
//
// THE STROKES AS TEXT are the Katana customisation format's own lines
// (docs/customisation.md, "The members"), one stroke a line:
//     ["move", 0, 0],
//     ["draw", 8, 0],
//     ["text", {"text": "W", "height": 1.5}]
// They are read by the format's reader (entity::definitionFromJson) and by
// nothing else: this joins the lines under the definition's members as a file
// would hold them, hands the result over, and - when it is refused - finds
// the line. Two things are allowed of the text that a file does not allow,
// and neither reaches the reader: the comma that ends a line may be left off,
// and a blank line is passed over.

#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/style_library.hpp"

namespace katana::cad {

// The strokes of `definition` as that text: the lines the format's writer
// gives them, without the indent, a comma ending each but the last. Empty for
// a definition with no strokes. Fails as entity::definitionToJson does, for a
// definition the format cannot write (a stroke carrying a member its kind does
// not use, texts that are not exactly its text strokes in order).
[[nodiscard]] katana::core::Result<std::string>
strokeText(const katana::entity::LineStyle& definition);

// What reading a definition from its members and that text gave.
struct StrokeTextRead {
    // The definition, when it read: `members` with the strokes and texts the
    // text holds.
    std::optional<katana::entity::LineStyle> definition{};
    // Why not, as the format's writer or reader - or entity::validate through
    // it - refused it. Its context may cite a line and column of the joined
    // text, which nobody typed; `problem` is what a person is shown.
    katana::core::Error error{};
    // The same for a person: the refusal with the LINE of `strokes` it is
    // about in front ("Line 3: definition "TEST Valve" strokes[2]: "drow" is
    // not a kind of stroke ..."; "Line 4 is not a stroke: ..." for text that
    // is not JSON), and with no place in the joined text.
    std::string problem{};
    // That line, counted from 1 as a text box counts; 0 when it read, or when
    // what was refused is no line's (a member: a length below 0).
    int line = 0;
};

// Reads the definition `members` describes - its strokes and texts are not
// looked at - with `strokes` as its strokes. `members.symbol` is the list it
// would sit in. Text that reads as MORE than strokes is refused: a line can
// end the list and go on to give the definition members of its own
// (`]], "atVertices": true, ...`), which is sound JSON and would make a
// definition `members` does not describe.
[[nodiscard]] StrokeTextRead readStrokeText(const katana::entity::LineStyle& members,
                                            std::string_view strokes);

// `base`, else "base 2", "base 3" ...: the first name neither the library nor
// the drawing's Linetype table holds, so a copy of a definition does not start
// life over another definition or as a D2 collision (cad/style_resolver.hpp).
// Beside freeStyleName and freeLinetypeName, which do the same for the two
// model tables.
[[nodiscard]] std::string freeDefinitionName(const Document& document, std::string_view base);

// The line that removes the definition `name` from the session's
// customisation: `CUSTOMISE REMOVE "<name>"`, and ` FORCE` after it when
// `force` - the name always in quotes, as the line is documented.
//
// InvalidArgument when no such line can NAME it. The interpreter's words have
// no escape and its quotes do not survive the tokenizer (cad/annotation/
// command_words.hpp), so:
//   * a name holding a double quote or a line break cannot be written at all;
//   * a name that is one of the line's own words - CODE or FORCE, in any case
//     - would be read as that word: `CUSTOMISE REMOVE "CODE" FORCE` is "remove
//     the survey code FORCE", not "remove the definition CODE anyway";
//   * no name is no definition.
// The same refusal is why an editor does not MAKE a definition under such a
// name: it could be saved and never removed.
[[nodiscard]] katana::core::Result<std::string> removeDefinitionLine(std::string_view name,
                                                                     bool force);

} // namespace katana::cad
