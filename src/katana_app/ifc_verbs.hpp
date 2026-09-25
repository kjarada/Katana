#pragma once

// EXPORT of a .ifc, natively (katana_ifc, docs/ifc.md). Here and not in
// main.cpp so that the command line's one hook is a line, as for
// dxf_verbs.hpp, and outside the interoperability guard because writing IFC
// needs no third-party library.
//
//   EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]
//                     [SPACING <m>] [NODRAWING]
//
// The drawing's entities and alignments are written, with the project's
// coordinate system as the georeferencing; UTILITIES adds an AS 5488
// subsurface utility investigation (the schedule format of
// survey/subsurface/utility_csv.hpp, TfNSW Utility Schema columns included),
// SCHEMA the delivery schema it was written to (delivery_schema.hpp), SPACING
// the longest detected spacing that keeps QL-B (default 10 m), and NODRAWING
// leaves the drawing out, for a file of the investigation alone.

#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"

namespace katana::app {

// `verb` is EXPORT, upper case; `argument` the rest of the line. nullopt when
// the path is not a .ifc, so the caller carries on to its other exporters;
// otherwise whether it worked, which has been reported on stdout or stderr.
[[nodiscard]] std::optional<bool> runIfcVerb(const katana::cad::Document& document,
                                             std::string_view verb, std::string_view argument);

// The lines --help prints for it.
[[nodiscard]] const char* ifcHelpText();

} // namespace katana::app
