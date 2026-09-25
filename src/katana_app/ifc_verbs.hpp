#pragma once

// IMPORT and EXPORT of a .ifc, natively (katana_ifc, docs/ifc.md). Here and not in
// main.cpp so that the command line's one hook is a line, as for
// dxf_verbs.hpp, and outside the interoperability guard because writing IFC
// needs no third-party library.
//
//   IMPORT <file.ifc> [LOCAL]
//   EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]
//                     [RULES <rules.csv>] [SPACING <m>] [NODRAWING]
//
// IMPORT brings the file's alignments, elements and annotations into the
// drawing as ONE undoable step (ifc/import.hpp); LOCAL moves them to sit at
// the origin, as the other importers' LOCAL does. The command line holds no
// surfaces, so a terrain the file carries is counted and not kept.
//
// The drawing's entities and alignments are written, with the project's
// coordinate system as the georeferencing; UTILITIES adds an AS 5488
// subsurface utility investigation (the schedule format of
// survey/subsurface/utility_csv.hpp, TfNSW Utility Schema columns included),
// SCHEMA the delivery schema it was written to (delivery_schema.hpp), SPACING
// the longest detected spacing that keeps QL-B (default 10 m), RULES a
// project's classification rules (classification.hpp), and NODRAWING leaves
// the drawing out, for a file of the investigation alone. The grammar is
// ifc/front_end.hpp's, which the window's command line and dialogs share.

#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"

namespace katana::app {

// `verb` is IMPORT or EXPORT, upper case; `argument` the rest of the line.
// nullopt when the path is not a .ifc, so the caller carries on to its other
// importers and exporters; otherwise whether it worked, which has been
// reported on stdout or stderr.
[[nodiscard]] std::optional<bool> runIfcVerb(katana::cad::Document& document, std::string_view verb,
                                             std::string_view argument);

// The lines --help prints for it.
[[nodiscard]] const char* ifcHelpText();

} // namespace katana::app
