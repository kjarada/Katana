#pragma once

// What the two front ends share about IFC, so that a line typed at katana_cli,
// the same line typed in the window, and the window's IFC dialogs all mean the
// same file (docs/ifc.md, "Using it"):
//
//   * the grammar of IMPORT <file.ifc> and EXPORT <file.ifc> ...;
//   * reading what an export names - the utility schedule, its delivery
//     schema, a project's classification rules;
//   * the project's GlobalId namespace and the header's timestamp, so that
//     both front ends give an object the same GlobalId.
//
// Here and not in either front end because each may see katana_ifc and
// neither may see the other (katana_cad may not see ifc at all,
// tools/check_layering.cmake), and because what is here can be tested
// without a window or a process.

#include <chrono>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/ifc/classification.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/import.hpp"
#include "katana/survey/subsurface/delivery_schema.hpp"

namespace katana::ifc {

// EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]
//                   [RULES <rules.csv>] [SPACING <m>] [NODRAWING]
//                   [NOENTITIES] [SELECTED] [NOALIGNMENTS] [NOSURFACES] [PREVIEW]
//
// Every choice File > Export IFC offers is a word here, so that the dialog
// only writes the line (formatExportLine) and a person, a script and an agent
// can each ask for the same file.
struct ExportArguments {
    std::string path;                    // the file to write, UTF-8, as given
    std::optional<std::string> schedule; // UTILITIES: an AS 5488 schedule
    std::optional<std::string> schema;   // SCHEMA: the delivery schema it was written to
    std::optional<std::string> rules;    // RULES: a project's classification rules
    std::optional<double> spacing;       // SPACING: longest detected spacing keeping QL-B, m
    // NODRAWING is all three false: the investigation alone.
    bool entities = true;   // false: NOENTITIES
    bool alignments = true; // false: NOALIGNMENTS
    bool surfaces = true;   // false: NOSURFACES; the window's surfaces (katana_cli holds none)
    bool selected = false;  // SELECTED: of the entities, the selected ones
    bool preview = false;   // PREVIEW: the reply the file would give, no file written

    friend bool operator==(const ExportArguments&, const ExportArguments&) = default;
};

// IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]
//                   [TOLERANCE <m>] [TAKECRS | KEEPCRS]
struct ImportArguments {
    std::string path;
    bool local = false;              // LOCAL: moved to sit at the origin
    bool alignments = true;          // false: NOALIGNMENTS
    bool elements = true;            // false: NOELEMENTS - elements and annotations
    bool surfaces = true;            // false: NOSURFACES
    std::optional<double> tolerance; // TOLERANCE: ImportOptions::curveTolerance, m
    // TAKECRS: the project takes the file's coordinate system when it has
    // none; KEEPCRS: it does not. Neither: katana_cli and a typed line say how
    // (CRS SET), and File > Import asks.
    std::optional<bool> takeCoordinateSystem;

    friend bool operator==(const ImportArguments&, const ImportArguments&) = default;
};

// The rest of an EXPORT or IMPORT line, after the verb. nullopt when it names
// no .ifc - a path in quotes, or everything up to the first ".ifc" that ends
// a word, so that an unquoted path with blanks still reads - and the caller's
// other exporters and importers take the line. Otherwise the arguments, or
// InvalidArgument saying what is wrong: an unclosed quote, an unknown word, a
// word given twice, SCHEMA without UTILITIES, SELECTED without the entities,
// a SPACING or TOLERANCE that is not a positive number, TAKECRS with KEEPCRS
// or with LOCAL (data moved to the origin is in no system), an import that
// leaves out everything, a quote opened before a .ifc and never closed. Words
// are matched in any letter case; a value may be double-quoted.
[[nodiscard]] std::optional<core::Result<ExportArguments>>
parseExportArguments(std::string_view argument);
[[nodiscard]] std::optional<core::Result<ImportArguments>>
parseImportArguments(std::string_view argument);

// The whole line that says `arguments` - "EXPORT \"site.ifc\" NOSURFACES" -
// every path quoted and every number as it reads back, so that the parse of
// the line is `arguments` again. What a dialog hands the command line.
// InvalidArgument for a path holding a double quote, which a line cannot say.
[[nodiscard]] core::Result<std::string> formatExportLine(const ExportArguments& arguments);
[[nodiscard]] core::Result<std::string> formatImportLine(const ImportArguments& arguments);
// INFO "<file.ifc>": what the import dialog's Describe runs.
[[nodiscard]] core::Result<std::string> formatInfoLine(std::string_view path);

// IFC RULES <file.csv>: the default classification rules written to a file,
// a project's starting point for its own (RULES). The path after IFC RULES,
// nullopt when the line is not one; InvalidArgument for a missing path, an
// unclosed quote or a word after the path.
[[nodiscard]] std::optional<core::Result<std::string>>
parseRulesArguments(std::string_view argument);
[[nodiscard]] core::Result<std::string> formatRulesLine(std::string_view path);
// Writes them, and answers "ifc rules file=... rules=N"; FileExportFailure
// naming the file when it cannot be written.
[[nodiscard]] core::Result<std::string> writeDefaultRules(std::string_view path);

// A path given as text: core::pathFromUtf8 (core/path_text.hpp), under the
// name the IFC verbs and both front ends already call it by. UTF-8 is read as
// UTF-8, and bytes that are not - a name from an ANSI console - as the narrow
// name they are, where building the path from them threw.
[[nodiscard]] std::filesystem::path pathFromUtf8(std::string_view text);

// What an export names, read and checked, ready for writeIfc: the graded
// investigation, the delivery schema it points into, and the rules.
struct ExportFiles {
    std::optional<UtilityInput> utilities;
    // Owned here, so that utilities->schema stays valid however this is moved.
    std::unique_ptr<survey::subsurface::DeliverySchema> schema;
    // A RULES file's rules, then defaultClassificationRules(): a project names
    // the exceptions, and everything else is classified as it would have
    // been. Empty without a RULES file, which writeIfc reads as the defaults.
    std::vector<ClassificationRule> rules;
};

// Reads the schedule, schema and rules `arguments` name. NotFound naming a
// file that cannot be read; the reader's own error, with the file named, for
// one that does not parse; InvalidArgument for SCHEMA without UTILITIES.
[[nodiscard]] core::Result<ExportFiles> readExportFiles(const ExportArguments& arguments);

// What seeds every GlobalId of a project's exports (ExportOptions::
// guidNamespace): its name and the time it was created, so that the same
// project gives each object the same GlobalId from either front end and in
// every session, and two projects of one name do not collide.
[[nodiscard]] std::string guidNamespaceFor(std::string_view projectName,
                                           std::string_view createdUtc);

// ---- replies ------------------------------------------------------------------------
//
// What EXPORT, IMPORT and INFO answer, as key=value records one a line
// (core::replyQuoted, docs/ifc.md "Replies"), the same from katana_cli, the
// window's command line and a dialog:
//
//   ifc exported file="site.ifc" schema=IFC4X3_ADD2 instances=1097 bytes=82914
//     (PREVIEW: "ifc previewed", and no bytes)
//   counts alignments=1 services=4 segments=10 segments_3d=6 located_points=14
//          entities_written=5 entities_skipped=0 surfaces=0
//   class name=IfcKerb count=1                      one a class written
//   object from="layer Survey/Kerb" count=1 class=IfcKerb predefined=NOTDEFINED
//          object_type="" system="" why="rule kerb" one a ClassTally
//   warning text="..."
//
//   ifc imported file="site.ifc" schema=IFC4X3_ADD2 crs=EPSG:7856 entities=29
//          alignments=1 surfaces=0 objects=29 drawn=29 as_points=0
//          alignments_as_polylines=0
//   ifc described ...                               INFO: the same, and
//   alignment name="MC01" pis=4 pvis=4              one a reconstructed alignment
//   surface name="Terrain" triangles=2
//   extent min_x=... min_y=... max_x=... max_y=...  metres, to the millimetre
//
// A front end adds its own records after them: "note text=..." for what it
// did or says to do (the coordinate system, a shift).

[[nodiscard]] std::string formatExportReply(const IfcExport& report, std::string_view fileName,
                                            bool preview);
// An import's reply is written after importCommand, whose renames are among
// the warnings; `entities` is how many it brought, counted before the
// command moved them out.
[[nodiscard]] std::string formatImportReply(const IfcImport& imported, std::string_view fileName,
                                            std::size_t entities);
[[nodiscard]] std::string formatDescription(const IfcImport& read, std::string_view fileName);
// "note text=..." and "warning text=...".
[[nodiscard]] std::string noteRecord(std::string_view text);
[[nodiscard]] std::string warningRecord(std::string_view text);

// The object records of an EXPORT reply as the tally they were written from:
// the one reader of what formatExportReply writes, which File > Export IFC's
// table is filled from. Other records are passed over; ParseFailure naming
// the line for an object record that does not read.
[[nodiscard]] core::Result<std::vector<ClassTally>> readExportObjects(std::string_view reply);

// `when` as the header's time stamp, ISO 8601 in UTC to the second:
// "2026-09-25T16:01:09". The writer reads no clock (Rule 7); the caller
// passes its own.
[[nodiscard]] std::string headerTimestamp(std::chrono::system_clock::time_point when);

} // namespace katana::ifc
