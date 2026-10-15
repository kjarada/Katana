#pragma once

// ROADOBJECT: road objects on the text command line (docs/road_objects.md,
// "The verbs") - the interpreter's, so the window's command line, its road
// object dialogs, katana_cli and katana_mcp place them alike.
//
//   ROADOBJECT KINDS [<kind>]
//   ROADOBJECT LIST [JSON | <scope> [WHERE k=v ...]]
//   ROADOBJECT INFO <id>
//   ROADOBJECT ADD <kind> [key=value ...] <placement> [key=value ...] [PREVIEW]
//     <placement> is one of
//       ALIGNMENT <name> station=<s> [to=<s>] [offset=<o>] [spacing=<m>]
//       ALONG #<id> [#<id> ...]            one object along each line
//       ALONG FROM <scope> [WHERE k=v ...] one along each line the scope takes
//       AT x,y[,z] [x,y[,z] ...]           at the points (a linear kind runs
//                                          through them, a culvert from the
//                                          first to the second)
//       AT FROM <scope> [WHERE k=v ...]    at the points the scope takes
//   ROADOBJECT SET <id> key=value ... [PREVIEW]
//   ROADOBJECT COPY <id> [station=<s>] [dx=<m>] [dy=<m>] [key=value ...] [PREVIEW]
//   ROADOBJECT DELETE <id> [<id> ...] | ALL | <scope> [WHERE k=v ...] [PREVIEW]
//   ROADOBJECT QUANTITIES [<id> ... | <scope> [WHERE k=v ...]]
//   ROADOBJECT PROBLEMS
//
// <kind> is barrier, guidepost, sign, light, culvert, wall, fence or marking.
// The keys are those every object takes - name, layer, colour, material,
// facility, station, to, offset, spacing, heights=profile|surface|line,
// surface, crossfall, dz - and the kind's own (KINDS <kind> lists them with
// their units, ranges and defaults). spacing= is for the point kinds (guide
// posts, signs, light poles) along an alignment or a line - a barrier's and a
// fence's posts take postspacing= - and crossfall= for an object that stands
// on the profile; each is refused where nothing would read it. <scope> is the
// shared scope and filter grammar (cad/scope_verbs.hpp); after its WHERE every
// word with '=' is a condition, so an object's own keys stand before ALONG or
// AT there. LIST, QUANTITIES and DELETE act on ids, or on all of them, or on
// the objects a <scope> takes (road_object_scope.hpp says what each choice
// means for a road object, which is a definition and not an entity): ids and a
// scope are not mixed, a scope word is never read as an id, and LIST JSON is
// the whole stored set and takes no scope. The scope's record leads the reply
// (scope=layers layers=road/objects/sign sublayers=yes matched=1), and a scope
// that takes nothing is reported, not refused: DELETE then says count=0 and
// leaves no undo step.
//
// Words are case-insensitive. Every edit is ONE undo step and a refused one
// changes nothing; an ADD of several lines is all or nothing. PREVIEW says
// what would be done and does nothing. A reply is records, one per line, in
// the key=value form the options take; a scope's record (scopeRecord) leads
// the reply of a line that took one, and a scope that takes nothing is
// reported, not refused.

#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::road {

// True for ROADOBJECT and ROADOBJECTS, in any case.
[[nodiscard]] bool isRoadObjectVerb(std::string_view verb);

// Runs one ROADOBJECT line: `tokens` is the whole line split into words with
// the quotes removed (CommandInterpreter::tokenize), the verb first. `views`
// answers the scope word VIEW (the window's; none headless).
[[nodiscard]] katana::core::Result<std::string>
runRoadObjectVerb(Document& document, const std::vector<std::string>& tokens,
                  const ScopeViewProvider& views = {});

// Every ROADOBJECT word and key, for HELP ROADOBJECT.
[[nodiscard]] std::string roadObjectVerbHelp();

// The drawing's road objects for an agent, as JSON: each definition as the
// drawing keeps it, with the 3D objects made from it - id, IFC class and
// predefined type, layer, triangles, quantities - and the problems. What
// katana_mcp's katana_road_objects tool returns (a tool, not a resource:
// docs/mcp.md).
[[nodiscard]] std::string roadObjectsReport(const Document& document);

// The same for the objects a scope and filter take, with a "scope" member:
// `record` (the scope's record, as a reply leads with it), `matched`, and
// `unmade` (objects the scope would take but for not being made). The
// problems are those of the objects it took. Fails as matchRoadObjects does.
[[nodiscard]] katana::core::Result<std::string>
roadObjectsReport(const Document& document, const ScopeWords& words,
                  const ScopeViewProvider& views = {});

} // namespace katana::cad::road
