#pragma once

// UTILITY: the AS 5488 subsurface utility tools (katana/survey/subsurface,
// docs/subsurface_utilities.md) on the text command line - the interpreter's,
// so the window, katana_cli, katana_mcp and an AI agent all run the same
// verbs.
//
//   UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>]
//   UTILITY VERIFY <schedule.csv>
//   UTILITY CLEARANCE <schedule.csv> <design.csv> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
//   UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>
//   UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
//
// Words are case-insensitive and a path with blanks is quoted. REPORT,
// VERIFY, CLEARANCE and CHECK read, grade and report; they never touch the
// drawing, so they are safe against any open project. CHECK is refused when
// the schedule has errors against the schema, with the whole check in the
// refusal, so a script stops there and still shows why. DRAW adds the graded
// schedule to the drawing as ONE undo step (utility_drawing.hpp) and replies
// with its records, the first of which carries the bounds= a front end frames.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::utilities {

// True for UTILITY, in any case.
[[nodiscard]] bool isUtilityVerb(std::string_view verb);

// Runs one UTILITY line: `tokens` is the whole line split into words with the
// quotes removed (CommandInterpreter::tokenize), the verb first. Returns the
// reply - a report's text, or DRAW's records - or an error saying what was
// refused and why. A refused DRAW changes nothing.
[[nodiscard]] katana::core::Result<std::string>
runUtilityVerb(Document& document, const std::vector<std::string>& tokens);

// Every UTILITY verb and option, for HELP UTILITY.
[[nodiscard]] std::string utilityVerbHelp();

// The bounds= box of a UTILITY DRAW reply's first record, for a front end to
// frame what arrived: survey data in a real coordinate system usually lands
// far from the view. nullopt for any other reply, or a box that is not four
// finite numbers, min before max.
[[nodiscard]] std::optional<katana::geometry::Box2> drawReplyBounds(std::string_view reply);

} // namespace katana::cad::utilities
