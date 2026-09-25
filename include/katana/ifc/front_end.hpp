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
#include "katana/survey/subsurface/delivery_schema.hpp"

namespace katana::ifc {

// EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]
//                   [RULES <rules.csv>] [SPACING <m>] [NODRAWING]
struct ExportArguments {
    std::string path;                    // the file to write, UTF-8, as given
    std::optional<std::string> schedule; // UTILITIES: an AS 5488 schedule
    std::optional<std::string> schema;   // SCHEMA: the delivery schema it was written to
    std::optional<std::string> rules;    // RULES: a project's classification rules
    std::optional<double> spacing;       // SPACING: longest detected spacing keeping QL-B, m
    bool drawing = true;                 // false: NODRAWING - the investigation alone

    friend bool operator==(const ExportArguments&, const ExportArguments&) = default;
};

// IMPORT <file.ifc> [LOCAL]
struct ImportArguments {
    std::string path;
    bool local = false; // LOCAL: moved to sit at the origin

    friend bool operator==(const ImportArguments&, const ImportArguments&) = default;
};

// The rest of an EXPORT or IMPORT line, after the verb. nullopt when it names
// no .ifc - a path in quotes, or everything up to the first ".ifc" that ends
// a word, so that an unquoted path with blanks still reads - and the caller's
// other exporters and importers take the line. Otherwise the arguments, or
// InvalidArgument saying what is wrong: an unclosed quote, an unknown word,
// SCHEMA without UTILITIES, a SPACING that is not a positive number, a quote
// opened before a .ifc and never closed. Words are matched in any letter
// case; a value may be double-quoted.
[[nodiscard]] std::optional<core::Result<ExportArguments>>
parseExportArguments(std::string_view argument);
[[nodiscard]] std::optional<core::Result<ImportArguments>>
parseImportArguments(std::string_view argument);

// A path given as UTF-8 text, as the filesystem takes it on every platform
// (on Windows a narrow std::filesystem::path would read it in the ANSI code
// page).
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

// `when` as the header's time stamp, ISO 8601 in UTC to the second:
// "2026-09-25T16:01:09". The writer reads no clock (Rule 7); the caller
// passes its own.
[[nodiscard]] std::string headerTimestamp(std::chrono::system_clock::time_point when);

} // namespace katana::ifc
