#pragma once

// UTILITY: the AS 5488 subsurface utility tools (katana/survey/subsurface,
// docs/subsurface_utilities.md) on the command line. Here and not in main.cpp
// so that the command line's one hook is a line, as for dxf_verbs.hpp.
//
//   UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>]
//   UTILITY VERIFY <schedule.csv>
//   UTILITY CLEARANCE <schedule.csv> <design.csv> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
//
// The schedules are read, graded and reported; nothing is added to the
// drawing, so the verbs are safe to run against any open project.

#include <string_view>

namespace katana::app {

// `argument` is the rest of the line after UTILITY. Whether it worked; what
// it produced, or why it failed, has been written to stdout or stderr.
[[nodiscard]] bool runUtilityVerb(std::string_view argument);

// The lines --help prints for UTILITY.
[[nodiscard]] const char* utilityHelpText();

} // namespace katana::app
