#pragma once

// IMPORT, EXPORT and INFO of a .ifc, natively (katana_ifc, docs/ifc.md). Here
// and not in main.cpp so that the command line's one hook is a line, as for
// dxf_verbs.hpp, and outside the interoperability guard because writing IFC
// needs no third-party library.
//
//   IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]
//                     [TOLERANCE <m>] [TAKECRS | KEEPCRS]
//   EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]
//                     [RULES <rules.csv>] [SPACING <m>] [NODRAWING]
//                     [NOENTITIES] [SELECTED] [NOALIGNMENTS] [NOSURFACES] [PREVIEW]
//   INFO <file.ifc>
//   IFC RULES <file.csv>
//
// IMPORT brings the file's alignments, elements and annotations into the
// drawing as ONE undoable step (ifc/import.hpp); LOCAL moves them to sit at
// the origin, as the other importers' LOCAL does; TAKECRS gives a project
// with no coordinate system the file's. The command line holds no surfaces,
// so a terrain the file carries is counted and not kept.
//
// EXPORT writes the drawing's entities (or the selected ones) and alignments,
// with the project's coordinate system as the georeferencing; UTILITIES adds
// an AS 5488 subsurface utility investigation (the schedule format of
// survey/subsurface/utility_csv.hpp, a delivery schema's columns included),
// SCHEMA the delivery schema it was written to (delivery_schema.hpp), SPACING
// the longest detected spacing that keeps QL-B (default 10 m), RULES a
// project's classification rules (classification.hpp). PREVIEW answers what
// the file would hold and writes nothing. INFO describes a file without
// importing it, and IFC RULES writes the default classification rules for a
// project to start its own from.
//
// Every reply is key=value records (ifc/front_end.hpp, "replies"), and the
// grammar is front_end.hpp's, which the window's command line and its
// dialogs share: a dialog writes the line, and this is what a line means.

#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"

namespace katana::app {

// `verb` is IMPORT, EXPORT, INFO or IFC, upper case; `argument` the rest of the
// line. nullopt when the path is not a .ifc, so the caller carries on to its
// other importers and exporters; otherwise whether it worked, which has been
// reported on stdout or stderr.
[[nodiscard]] std::optional<bool> runIfcVerb(katana::cad::Document& document, std::string_view verb,
                                             std::string_view argument);

// The lines --help prints for it.
[[nodiscard]] const char* ifcHelpText();

} // namespace katana::app
