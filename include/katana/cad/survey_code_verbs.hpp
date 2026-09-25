#pragma once

// CODE and MAPFILE: the survey codes on the text command line
// (docs/survey_coding.md). The interpreter's, so the window's command line,
// katana_cli, katana_mcp and an AI agent all run the same verbs; until
// 2026-09-26 they were katana_cli's own, and the window refused them.
//
//   CODE [<property>]           apply the loaded survey codes to every entity
//                               carrying a field code, as ONE undo step
//   CODE EXPLAIN <code>         why a code gets what it gets
//   CODE CENSUS [<property>]    the codes this drawing carries
//   MAPFILE LIST [<filter>]     the loaded survey codes, one code per line
//   MAPFILE CHECK               lint them; refused when a rule has an error
//
// A property cannot be called EXPLAIN or CENSUS here, which no survey format
// does. Every reply is code_table.hpp's formatter text, the words the Survey
// Code Manager shows. The standard colour names reach cad only through a
// ColourLookup: cad may not see archive12d, which owns the table, so the
// front end passes it (CommandInterpreter::setColourLookup); without one no
// colour is known, and colours are left alone rather than guessed at.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/code_table.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

// True for CODE and MAPFILE, in any case.
[[nodiscard]] bool isSurveyCodeVerb(std::string_view verb);

// Runs one CODE or MAPFILE line: `tokens` is the whole line split into words
// with the quotes removed (CommandInterpreter::tokenize), the verb first.
// InvalidState when no survey codes are loaded; a refused CODE changes
// nothing.
[[nodiscard]] katana::core::Result<std::string>
runSurveyCodeVerb(Document& document, const std::vector<std::string>& tokens,
                  const ColourLookup& colourOf);

// MAPFILE CHECK's answer for the lint of a map of `rules` rules: the lint's
// text, or - when any issue is an error - an InvalidState refusal whose first
// line counts the errors and whose later lines are the whole lint, so a
// script stops there and still shows why (as UTILITY CHECK refuses). Apart
// from the verb because SurveyMap::add refuses the rules that lint as errors
// today, so no map a test can build reaches the refusal through the verb.
[[nodiscard]] katana::core::Result<std::string>
mapfileCheckReply(const std::vector<LintIssue>& issues, std::size_t rules);

} // namespace katana::cad
