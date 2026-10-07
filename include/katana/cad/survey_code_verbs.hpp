#pragma once

// CODE: the survey codes on the text command line (docs/survey_coding.md).
// The interpreter's, so the window's command line, katana_cli, katana_mcp and
// an AI agent all run the same verb; until 2026-09-26 it was katana_cli's own,
// and the window refused it.
//
//   CODE [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW]
//                               apply the loaded survey codes to the entities
//                               the scope takes that carry a field code, as
//                               ONE undo step; PREVIEW changes nothing
//   CODE CENSUS [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW]
//                               the codes those entities carry (it changes
//                               nothing, so PREVIEW asks nothing more of it:
//                               the word is taken so that the two forms are
//                               one grammar)
//   CODE EXPLAIN <code>         why a code gets what it gets
//   CODE LIST [<filter>]        the loaded survey codes, one code per line
//   CODE CHECK                  lint them; refused when a rule has an error
//
// LIST and CHECK were a verb of their own, named after the survey code file
// of another program; that file no longer loads, and its verb went with it.
//
// THE SCOPE is the one every verb on drawing data takes (scope_verbs.hpp),
// read by the one parser. What differs here is the DEFAULT: with no scope
// word CODE takes the whole drawing, and under a bare WHERE too, where MODIFY
// and the rest take the selection. CODE was the whole drawing before it took
// a scope, and a script that says CODE must not come to mean "whatever
// happens to be selected".
//
// THE FIRST WORD decides how the rest is read:
//
//   EXPLAIN, CENSUS, LIST, CHECK    that subcommand
//   a scope word, WHERE, PROPERTY   the grammar above
//   or PREVIEW
//   anything else                   the property, as the whole rest of the
//                                   line: CODE feature_code, as it always was
//
// A scope word that cannot begin a scope where it stands is not one: LAYER,
// LAYERS and AREA need the word after them, so CODE Layer - a property many
// drawings from GIS data carry - is still that property, and CODE LAYERS a is
// the layer a. A property called as a scope word that CAN stand alone (ALL,
// VIEW, SEL), or as a subcommand, is given as PROPERTY <name>.
//
// So is a property of SEVERAL words whose first is any of those words: with
// a word after it even LAYER, LAYERS and AREA begin the scope form, and
// nothing can tell CODE Layer a, the layer, from a property called "Layer a".
// CODE Area m2 is refused (a window is four numbers) and CODE Layer a codes
// layer a; that property is PROPERTY "Area m2". A property of several words
// whose first is none of them is still the whole rest of the line.
//
// Every reply begins with what the scope took (scopeRecord); the rest is
// code_table.hpp's formatter text, the words the Survey Code Manager shows.
// Colour names are resolved through the Document and nothing else
// (colour_lookup.hpp: the customisation's own table, then the standard names):
// a name neither knows leaves the colour alone.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/code_table.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

// True for CODE, in any case.
[[nodiscard]] bool isSurveyCodeVerb(std::string_view verb);

// Runs one CODE line: `args` are the words AFTER the verb, the quotes removed
// (CommandInterpreter::tokenize). `views` answers the scope word VIEW, and is
// empty headless. InvalidState when no survey codes are loaded; a refused CODE
// changes nothing.
//
// (It took a colour lookup of a caller's own until 2026-10-07, asked after
// the Document for a name it did not know. The front ends passed their table
// of standard names through it while cad could not see that table; since the
// Document resolves those itself no program passed one, and a parameter only
// its tests reach is a second way to resolve a colour that nothing uses.)
[[nodiscard]] katana::core::Result<std::string>
runSurveyCodeVerb(Document& document, const std::vector<std::string>& args,
                  const ScopeViewProvider& views);

// CODE CHECK's answer for the lint of a map of `rules` rules: the lint's
// text, or - when any issue is an error - an InvalidState refusal whose first
// line counts the errors and whose later lines are the whole lint, so a
// script stops there and still shows why (as UTILITY CHECK refuses). Apart
// from the verb because SurveyMap::add refuses the rules that lint as errors
// today, so no map a test can build reaches the refusal through the verb.
[[nodiscard]] katana::core::Result<std::string>
codeCheckReply(const std::vector<LintIssue>& issues, std::size_t rules);

} // namespace katana::cad
