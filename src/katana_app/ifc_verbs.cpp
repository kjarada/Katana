#include "ifc_verbs.hpp"

#include <chrono>
#include <filesystem>
#include <format>
#include <iostream>
#include <utility>

#include "katana/cad/project_crs.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/front_end.hpp"
#include "katana/ifc/import.hpp"

#ifndef KATANA_VERSION
#define KATANA_VERSION ""
#endif

namespace katana::app {

namespace {

// The arguments of a line that names the file and nothing else: what INFO
// takes.
katana::ifc::ImportArguments fileAlone(const std::string& path)
{
    katana::ifc::ImportArguments bare;
    bare.path = path;
    return bare;
}

bool fail(const std::string& message)
{
    std::cerr << "error: " << message << '\n';
    return false;
}

bool exportIfc(const katana::cad::Document& document, const katana::ifc::ExportArguments& arguments)
{
    // The grammar, the files it names and the GlobalId namespace are the
    // window's too (ifc/front_end.hpp), so a line means one file in either.
    auto files = katana::ifc::readExportFiles(arguments);
    if (!files) {
        return fail(files.error().describe());
    }
    katana::ifc::ExportInput input;
    if (arguments.entities || arguments.alignments) {
        input.model = &document.model();
    }
    input.utilities = std::move(files->utilities);

    const auto& metadata = document.metadata();
    katana::ifc::ExportOptions options;
    options.projectName = metadata.name;
    options.projectDescription = metadata.description;
    options.applicationVersion = KATANA_VERSION;
    // The export's clock is the caller's, so that the writer stays a
    // function of what it is given.
    options.timestamp = katana::ifc::headerTimestamp(std::chrono::system_clock::now());
    options.guidNamespace = katana::ifc::guidNamespaceFor(metadata.name, metadata.createdUtc);
    options.rules = std::move(files->rules);
    options.georeference.name = metadata.coordinateSystem;
    if (!metadata.coordinateSystem.empty()) {
        if (const auto described =
                katana::cad::describeCoordinateSystem(metadata.coordinateSystem)) {
            options.georeference.description = described->name;
        }
    }
    options.exportEntities = arguments.entities;
    options.exportAlignments = arguments.alignments;
    if (arguments.selected) {
        options.entities = document.selection().ids();
        // An empty list is every entity (ExportOptions::entities): nothing
        // selected must not become the whole drawing.
        if (options.entities.empty()) {
            return fail("InvalidState: SELECTED, and nothing is selected: select the entities "
                        "to export, or leave SELECTED out");
        }
    }

    const std::filesystem::path path = katana::ifc::pathFromUtf8(arguments.path);
    const auto written = arguments.preview ? katana::ifc::writeIfc(input, options)
                                           : katana::ifc::writeIfcFile(input, path, options);
    if (!written) {
        return fail(written.error().describe());
    }
    std::cout << katana::ifc::formatExportReply(*written, path.filename().string(),
                                                arguments.preview)
              << '\n';
    return true;
}

bool importIfc(katana::cad::Document& document, const katana::ifc::ImportArguments& arguments)
{
    const std::filesystem::path path = katana::ifc::pathFromUtf8(arguments.path);
    katana::ifc::ImportOptions options;
    options.importAlignments = arguments.alignments;
    options.importElements = arguments.elements;
    options.importSurfaces = arguments.surfaces;
    if (arguments.tolerance) {
        options.curveTolerance = *arguments.tolerance;
    }
    auto imported = katana::ifc::readIfcFile(path, options);
    if (imported && arguments.local && !imported->bounds.empty()) {
        // Read again with the shift, so that the one reader applies it to
        // every kind of geometry, alignments included.
        options.originShift = imported->bounds.min;
        imported = katana::ifc::readIfcFile(path, options);
    }
    if (!imported) {
        return fail(imported.error().describe());
    }
    // The count before the command, which takes the entities; the reply
    // after it, which adds its renames to the warnings.
    const std::size_t entities = imported->entities.size();
    if (auto command = katana::ifc::importCommand(*imported, document.model())) {
        if (const auto status = document.execute(std::move(command)); !status) {
            return fail(status.error().describe());
        }
    }
    std::string reply =
        katana::ifc::formatImportReply(*imported, path.filename().string(), entities);
    const auto note = [&](const std::string& text) {
        reply += "\n" + katana::ifc::noteRecord(text);
    };
    if (arguments.local && options.originShift) {
        note(std::format("shifted by {:.3f},{:.3f} to sit at the origin", options.originShift->x,
                         options.originShift->y));
    }
    if (!imported->surfaces.empty()) {
        note("the command line holds no surfaces: the file's are counted, not kept");
    }
    const std::string& project = document.metadata().coordinateSystem;
    const std::string& file = imported->coordinateSystem;
    if (!file.empty() && project.empty() && !arguments.local) {
        if (arguments.takeCoordinateSystem.value_or(false)) {
            // Its own undoable step, after the import's, as the window's.
            if (const auto status = document.setCoordinateSystem(file, "IMPORT_CRS"); !status) {
                return fail(status.error().describe());
            }
            note("the project is now in " + file + ", the file's coordinate system");
        } else {
            note("the file is in " + file + " and the project has no coordinate system: CRS SET " +
                 file + " sets it");
        }
    } else if (!file.empty() && !project.empty() && project != file) {
        reply +=
            "\n" + katana::ifc::warningRecord("the file is in " + file + " and the project in " +
                                              project + "; nothing was reprojected");
    }
    std::cout << reply << '\n';
    return true;
}

bool describeIfc(std::string_view argument)
{
    // INFO takes a path and nothing else: the import's grammar, which reads
    // a quoted or unquoted .ifc, refusing any option word.
    const auto parsed = katana::ifc::parseImportArguments(argument);
    if (!parsed) {
        return false;
    }
    if (!*parsed) {
        return fail(parsed->error().describe());
    }
    if (**parsed != fileAlone((*parsed)->path)) {
        return fail("InvalidArgument: INFO <file.ifc> takes the file and nothing else");
    }
    const std::filesystem::path path = katana::ifc::pathFromUtf8((*parsed)->path);
    const auto read = katana::ifc::readIfcFile(path);
    if (!read) {
        return fail(read.error().describe());
    }
    std::cout << katana::ifc::formatDescription(*read, path.filename().string()) << '\n';
    return true;
}

} // namespace

std::optional<bool> runIfcVerb(katana::cad::Document& document, std::string_view verb,
                               std::string_view argument)
{
    if (verb == "EXPORT") {
        const auto arguments = katana::ifc::parseExportArguments(argument);
        if (!arguments) {
            return std::nullopt;
        }
        return *arguments ? exportIfc(document, **arguments) : fail(arguments->error().describe());
    }
    if (verb == "IMPORT") {
        const auto arguments = katana::ifc::parseImportArguments(argument);
        if (!arguments) {
            return std::nullopt;
        }
        return *arguments ? importIfc(document, **arguments) : fail(arguments->error().describe());
    }
    if (verb == "INFO") {
        if (!katana::ifc::parseImportArguments(argument)) {
            return std::nullopt; // not a .ifc: GIS's INFO
        }
        return describeIfc(argument);
    }
    if (verb == "IFC") {
        const auto path = katana::ifc::parseRulesArguments(argument);
        if (!path) {
            return fail("InvalidArgument: usage: IFC RULES <file.csv>");
        }
        if (!*path) {
            return fail(path->error().describe());
        }
        const auto written = katana::ifc::writeDefaultRules(**path);
        if (!written) {
            return fail(written.error().describe());
        }
        std::cout << *written << '\n';
        return true;
    }
    return std::nullopt;
}

const char* ifcHelpText()
{
    return "IFC       IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS] [NOELEMENTS] [NOSURFACES]\n"
           "          [TOLERANCE <m>] [TAKECRS | KEEPCRS]  alignments to PIs and PVIs,\n"
           "          elements and annotations with their property sets, one undo step\n"
           "          EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]\n"
           "          [RULES <rules.csv>] [SPACING <m>] [NODRAWING] [NOENTITIES] [SELECTED]\n"
           "          [NOALIGNMENTS] [NOSURFACES] [PREVIEW]  IFC 4.3: alignments, the\n"
           "          drawing by class, an AS 5488 investigation graded, typed and with\n"
           "          its delivery schema; PREVIEW writes nothing\n"
           "          INFO <file.ifc>  what the file holds, without importing it\n"
           "          IFC RULES <file.csv>  the default classification rules, to edit\n"
           "          (replies are key=value records, docs/ifc.md)\n";
}

} // namespace katana::app
