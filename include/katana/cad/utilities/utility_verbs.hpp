#pragma once

// UTILITY: the AS 5488 subsurface utility tools (katana/survey/subsurface,
// docs/subsurface_utilities.md) on the text command line - the interpreter's,
// so the window, katana_cli, katana_mcp and an AI agent all run the same
// verbs.
//
//   UTILITY REPORT    <schedule.csv> | <scope> [MINCOVER <m>] [SPACING <m>]
//   UTILITY VERIFY    <schedule.csv> | <scope>
//   UTILITY CLEARANCE <schedule.csv> | <scope> [DESIGN] <design.csv> | #<id> [LEVEL <z>] |
//                     ALIGNMENT <name> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
//   UTILITY CHECK     <schedule.csv> | <scope> SCHEMA <schema.csv>
//   UTILITY DRAW      <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
//   UTILITY REGRADE   <scope> [SPACING <m>] [MINCOVER <m>]
//   UTILITY SCHEDULE  <out.csv> <scope> [SCHEMA <schema.csv>]
//
// Words are case-insensitive and a path with blanks is quoted. <scope> is the
// shared scope and filter words (scope_verbs.hpp): a first word that is one,
// or WHERE, takes the services UTILITY DRAW drew (utility_data.hpp) instead
// of a file, each line whole, and the reply leads with what the scope took.
// REPORT, VERIFY, CLEARANCE, CHECK and SCHEDULE read and report; they never
// touch the drawing. CHECK is refused when the schedule has errors against
// the schema, with the whole check in the refusal, so a script stops there
// and still shows why. DRAW adds the graded schedule to the drawing as ONE
// undo step (utility_drawing.hpp), and REGRADE draws what is drawn again from
// its points as ONE step; each replies with its records, the first of which
// carries the bounds= a front end frames.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::utilities {

// True for UTILITY, in any case.
[[nodiscard]] bool isUtilityVerb(std::string_view verb);

// Runs one UTILITY line: `tokens` is the whole line split into words with the
// quotes removed (CommandInterpreter::tokenize), the verb first. `views`
// answers the scope word VIEW (the window's; none headless). Returns the
// reply - a report's text, or DRAW's and REGRADE's records - or an error
// saying what was refused and why. A refused DRAW or REGRADE changes nothing.
[[nodiscard]] katana::core::Result<std::string>
runUtilityVerb(Document& document, const std::vector<std::string>& tokens,
               const ScopeViewProvider& views = {});

// Every UTILITY verb and option, for HELP UTILITY.
[[nodiscard]] std::string utilityVerbHelp();

// The bounds= box of a UTILITY DRAW or REGRADE reply's first record, for a front end to
// frame what arrived: survey data in a real coordinate system usually lands
// far from the view. The window's command line uses it for typed lines and the
// utilities dialog alike, so there is one reading of the record. nullopt for
// any other reply, or a box that is not four finite numbers, min before max;
// a CR LF line end reads as LF.
[[nodiscard]] std::optional<katana::geometry::Box2> drawReplyBounds(std::string_view reply);

} // namespace katana::cad::utilities
