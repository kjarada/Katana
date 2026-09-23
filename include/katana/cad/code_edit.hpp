#pragma once

// What the survey-code manager needs to edit a map that is not a dialog's
// business to decide: the code list a person hands to a field crew, the key a
// new rule for a drawing's code should start from, and the attribute lists a
// form edits as text.
//
// Here, below Qt, so that each is tested on its own and the dialog stays a
// thin form over it (docs/cad.md).

#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

// ---- the code list as CSV -----------------------------------------------------

// One field of a CSV record, quoted as RFC 4180 says: a field holding a
// comma, a double quote, a carriage return or a line feed is enclosed in
// double quotes, with each double quote inside it doubled; any other field
// is written as it is. Leading and trailing blanks are data and are kept.
[[nodiscard]] std::string csvField(std::string_view text);

// The map's codes as a CSV table for a person or a spreadsheet: a header
// record, then one record per distinct key in key order (cad::codeTable),
// each giving what a code caught by that key resolves to - the key's own
// rules and the less specific keys that also catch it, combined as
// SurveyMap::lookup combines them. Columns:
//
//   code, description, group, layer, colour, line/point, linestyle, symbol,
//   size, tinable
//
// description is the rule comment, layer is 12d's model (which becomes the
// layer), line/point is the breakline ("line" or "point"), size is the
// symbol's size and tinable "yes" or "no". A field the rules do not set is
// EMPTY, never a default: absent is not zero, and a symbol size of 0 is the
// map's own way of saying "the definition's size", so it is empty too.
// Records end with CRLF, as RFC 4180 says, including the last.
[[nodiscard]] std::string codeListCsv(const katana::entity::SurveyMap& map);

// ---- a new rule for a code a drawing carries ------------------------------------

// The key a new rule for `code` should start from, for a person to accept or
// change: a code ending in a number - a string number, "WM01", "KB12" - gets
// a prefix key on what comes before the number ("WM*"), because every other
// string of that code will want the same rule; any other code gets itself as
// an exact key. Surrounding blanks are dropped (a key with them matches
// nothing typed); a code that is all digits, or empty after trimming, is kept
// exact, because "*" alone would catch every code.
[[nodiscard]] std::string suggestedKey(std::string_view code);

// ---- attribute lists as text ------------------------------------------------------

// One attribute per line, as "<type> <name> = <value>": "text Source = Survey".
// The type is 12d's own word ("text" or "integer") and the value is kept
// verbatim, "$PipeDiameter" included. Lines end with '\n'.
[[nodiscard]] std::string formatAttributeLines(
    const std::vector<katana::entity::SurveyAttribute>& attributes);

// The inverse of formatAttributeLines, for a form a person types in. Blank
// lines are skipped; the name and the value are trimmed, because a person
// does not mean the blanks around an '='. Fails with InvalidArgument, naming
// the line, on a line with no '=', no name, or a type other than "text" or
// "integer" (the only two the mapfile writer can write). A name cannot hold
// '=': the first '=' ends it.
[[nodiscard]] katana::core::Result<std::vector<katana::entity::SurveyAttribute>>
parseAttributeLines(std::string_view text);

} // namespace katana::cad
