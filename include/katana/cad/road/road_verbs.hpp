#pragma once

// The road model on the text command line (docs/road_design.md, "The
// verbs"): the interpreter's, so the window's command line, katana_cli,
// katana_mcp and an AI agent all run them.
//
//   ROADTEMPLATE [LIST] [JSON] | INFO <t> | DEFAULTS | SECTION <t>
//   ROADTEMPLATE NEW <t> [FROM <t>] [description=<text>] | DELETE <t> | RENAME <t> <new>
//   ROADTEMPLATE SET <t> description=<text>
//   ROADTEMPLATE ADD <t> LEFT|RIGHT|BOTH <kind> [name=<word>] [at=<n>] [key=value ...]
//   ROADTEMPLATE SET <t> <component> key=value ... | REMOVE <t> <component>
//   ROADTEMPLATE COURSE <t> <component> ADD <course> thickness=<m> [key=value ...]
//   ROADTEMPLATE COURSE <t> <component> SET <course> key=value ... | REMOVE <course>
//   ROADTEMPLATE EXPORT <t>|ALL <file.json> [OVERWRITE] | IMPORT <file.json> [OVERWRITE]
//
//   CORRIDOR [LIST] [JSON] | INFO <c> | PROBLEMS [<c>]
//   CORRIDOR NEW <c> ALIGNMENT <a> TEMPLATE <t> [surface=<s>] [interval=<m>] [from=] [to=]
//     [curvetol=<e>]
//   CORRIDOR SET <c> key=value ... | DELETE <c> | RENAME <c> <new>
//   CORRIDOR REGION <c> ADD <from> <to> <t> | REMOVE <from> | CLEAR
//   CORRIDOR SUPERELEVATION <c> ADD <station> <left%> <right%> | REMOVE <station> | CLEAR
//   CORRIDOR BUILD <c>|ALL | SECTION <c> <station>
//   CORRIDOR STRINGS <c> [layer=<layer>] [PREVIEW]
//   CORRIDOR SURFACE <c> [finished=<name>] [subgrade=<name>] [PREVIEW]
//   CORRIDOR QUANTITIES <c>|ALL [csv=<file> [OVERWRITE]]
//
//   ROADMODEL [LIST] | JSON | PROBLEMS | INFO <id>
//   ROADMODEL SHOW [ON|OFF]    the desktop window's alone (it is taken before it
//                              reaches here); a session with no views refuses it
//
// <kind> is lane, shoulder, kerb, footpath, verge, median, drain or batter.
// <component> is side:name (left:lane) or name, which is every side that has
// one of that name. Words are case-insensitive; a name with blanks is quoted.
// Every edit is ONE undo step, and a refused edit changes nothing. A reply
// is records, one a line, key=value: a definition's numbers exactly
// (core::formatExactReal), so a value a reply prints can be typed back; a
// quantity to three places, the precision a schedule is read to.
//
// The template and corridor verbs act on named definitions (and, for EXPORT and
// IMPORT, on a file), not on drawing entities, so they take no scope (CLAUDE.md
// 1.1): what a corridor is made from is named in it. What they make that IS drawing data - STRINGS - goes on a layer of
// its own and replaces only the strings the same corridor made before, found
// by the corridor's name ignoring case; RENAME carries them to the new name in
// its own step, and DELETE leaves them in the drawing.

#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/csv_table.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {
class Document;
}

namespace katana::cad::road {

// True for ROADTEMPLATE, CORRIDOR and ROADMODEL, in any case.
[[nodiscard]] bool isRoadVerb(std::string_view verb);

// Runs one road verb: `tokens` is the whole line split into words with the
// quotes removed (CommandInterpreter::tokenize), the verb first. Returns the
// reply, or an error saying specifically what was refused and why.
[[nodiscard]] katana::core::Result<std::string> runRoadVerb(Document& document,
                                                            const std::vector<std::string>& tokens);

// Every road verb and option, for HELP ROADTEMPLATE, HELP CORRIDOR and
// HELP ROADMODEL.
[[nodiscard]] std::string roadVerbHelp();

// The road model as ROADMODEL JSON gives it - every object without its
// mesh (its triangle count and box instead), every problem - for the MCP
// resource katana://road-model.
[[nodiscard]] std::string roadModelJson(const bim::BimModel& model);

// The quantities of `objects` as CORRIDOR QUANTITIES' csv= writes them (by
// cad::writeCsvTable, the one CSV writer): a header, then a row per object: id,
// name, kind, facility, part, material, then Length, Width, Depth, Height,
// GrossArea and GrossVolume (empty where an object has none), to three places.
[[nodiscard]] CsvTable quantitiesTable(const std::vector<bim::BimObject>& objects);

} // namespace katana::cad::road
