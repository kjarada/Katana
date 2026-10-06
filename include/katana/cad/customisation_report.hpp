#pragma once

// What customisation a session has: the counts, where it came from, the
// sources that went into it, what the open project was drawn with that is not
// loaded, and what it covers in THIS drawing. One result and its formatters,
// as code_table.hpp has them, so every front end says the same words.
//
// There are two texts of it while the front ends move to the shared CUSTOMISE
// verb (customisation_verbs.hpp):
//
//   formatCustomisationReply    what the verb's bare CUSTOMISE replies: the
//                               two count lines, then key=value records, then
//                               the coverage. customisationJson is the same as
//                               one JSON object, for CUSTOMISE JSON and for a
//                               front end that hands a client structured data.
//   formatCustomisationSummary  the older prose ("Loaded files, in load
//                               order: ..."), which names a source by the kind
//                               of file it once was. The two front ends' own
//                               CUSTOMISE and File > Drawing Summary still
//                               print it; it goes with the last of them.
//
// Both are made from ONE summary. The form of customisationSummary that takes
// the sources and the missing names as arguments is the older text's, so that
// it can be tested without loading anything; the form that takes the Document
// alone reads them from its state (customisation_state.hpp), where they live.

#include <cstddef>
#include <string>
#include <vector>

#include "katana/cad/customisation_record.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_coding.hpp"

namespace katana::cad {

struct CustomisationSummary {
    std::size_t definitions = 0; // in the style library
    std::size_t groups = 0;      // entity::styleGroups
    // Library definitions the pickers offer as each (decision D3,
    // symbolChoices and linetypeChoices); one definition can be both.
    std::size_t symbols = 0;
    std::size_t linestyles = 0;
    std::size_t rules = 0; // survey code rules
    std::size_t codes = 0; // distinct keys among them
    std::vector<CustomisationSource> loaded{};
    std::vector<std::string> notLoaded{};
    CustomisationCoverage coverage{};
    // The session's own state. The form of customisationSummary that is
    // handed the two lists above leaves these as they are here: it describes
    // lists, not a session.
    std::string name{};
    CustomisationOrigin origin = CustomisationOrigin::None;
    bool kept = false;
    std::size_t colours = 0; // names in the customisation's own colour table
    katana::entity::CustomisationAutomation automation{};
    katana::entity::LineworkCodes linework{};
};

[[nodiscard]] CustomisationSummary
customisationSummary(const Document& document, const std::vector<CustomisationSource>& loaded,
                     const std::vector<std::string>& missingAtOpen);
// The session as the Document holds it: its sources, what the open project is
// missing, and its name, origin, kept flag, colours, automation and linework
// codes.
[[nodiscard]] CustomisationSummary customisationSummary(const Document& document);

// Plain text, lines ending in '\n'. With nothing loaded it says so and how to
// load, and nothing more but the project's missing files: there is no coverage
// to report of nothing.
[[nodiscard]] std::string formatCustomisationSummary(const CustomisationSummary& summary);

// The two above in one call: what the front ends' own CUSTOMISE prints.
[[nodiscard]] std::string customisationReport(const Document& document,
                                              const std::vector<CustomisationSource>& loaded,
                                              const std::vector<std::string>& missingAtOpen);

// ---- the verb's reply -------------------------------------------------------------------
//
// Lines ending in '\n': the two count lines formatCustomisationSummary begins
// with (or that nothing is loaded and how to load; of a customisation that
// brought neither kind - colours, settings - that no definitions and no rules
// are loaded, never that no customisation is), then one record a line -
//
//   customisation name=NSW origin=builtIn kept=yes definitions=792 codes=632 rules=1624 colours=12
//   source name=NSW definitions=yes rules=yes               one a source, in load order
//   automation auto.codes=on auto.linework=on
//   linework linework.start=ST linework.end=END ... linework.rectangle=RECT
//   missing name="old symbols"                              one a name the project
//                                                           recorded and the session lacks
//
// - then the coverage lines, when anything is loaded. The automation and
// linework records carry the very keys CUSTOMISE SET takes, so either can be
// typed back after it. A record's values are written by recordValue
// (scope_verbs.hpp).
[[nodiscard]] std::string formatCustomisationReply(const CustomisationSummary& summary);

// The first of those records alone, with no line break: what a verb that
// changed the session ends its reply with, so the reply says what is loaded
// now.
[[nodiscard]] std::string customisationStateRecord(const CustomisationSummary& summary);

// The linestyle and symbol names the survey code rules ask for that nothing
// defines: not a definition of the library, not a plain line, not a symbol
// Katana draws itself. Distinct, in name order. A customisation need not be
// self-contained, and saying which names are missing is the difference
// between a symbol that is plainly absent and one silently drawn as a dot.
[[nodiscard]] std::vector<std::string> undefinedRuleNames(const Document& document);

// The session's customisation as ONE JSON object - CUSTOMISE JSON, and what a
// front end gives a client that wants structure (katana_mcp's resource). Its
// keys are in alphabetical order, two blanks a level, with no line break at
// the end:
//
//   name, origin, kept, description, notice, basedOn (null when none), builtIn
//   (the name of the host's built-in, "" when it has none)
//   sources      [{name, definitions, rules, notice}], in load order
//   counts       {definitions, groups, symbols, linestyles, rules, codes, colours}
//   automation   {codesOnSurveyImport, lineworkOnSurveyImport}
//   linework     {start, end, close, arcStart, arcEnd, join, rectangle}
//   colours      {name: "#RRGGBB"}, the customisation's own table
//   problems     {rules, cannotApply, warnings, byKind: {kind: count},
//                 undefined: [name]} - the lint CODE CHECK prints, counted
//                (cannotApply is its count of rules that cannot be applied as
//                written), and undefinedRuleNames
//   coverage     {styles, named, resolved, builtIn, unresolved, notLinestyles}
//   missing      [name], what the open project recorded and the session lacks
//
// automation, linework and basedOn use the member names of the Katana
// customisation format, so the report reads against a file. It does not hold
// the definitions or the rules themselves: a file does (CUSTOMISE EXPORT).
// Text that is not UTF-8 is written with U+FFFD rather than thrown.
[[nodiscard]] std::string customisationJson(const Document& document);

} // namespace katana::cad
