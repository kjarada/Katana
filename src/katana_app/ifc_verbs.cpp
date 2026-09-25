#include "ifc_verbs.hpp"

#include <chrono>
#include <filesystem>
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
    if (arguments.drawing) {
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

    const std::filesystem::path path = katana::ifc::pathFromUtf8(arguments.path);
    const auto written = katana::ifc::writeIfcFile(input, path, options);
    if (!written) {
        return fail(written.error().describe());
    }
    std::cout << "exported " << path.filename().string() << " (IFC4X3_ADD2, " << written->instances
              << " instances, " << written->bytesWritten << " bytes)\n";
    std::cout << "  " << written->alignments << " alignments, " << written->services
              << " services (" << written->serviceSegments << " segments, " << written->segmentsIn3d
              << " in 3D, " << written->locatedPoints << " located points), "
              << written->entitiesWritten << " entities written, " << written->entitiesSkipped
              << " skipped, " << written->surfaces << " surfaces\n";
    std::cout << "  classes:";
    for (const auto& [name, count] : written->classes) {
        std::cout << ' ' << name << ' ' << count << ';';
    }
    std::cout << '\n';
    for (const std::string& warning : written->warnings) {
        std::cout << "  " << warning << '\n';
    }
    return true;
}

bool importIfc(katana::cad::Document& document, const katana::ifc::ImportArguments& arguments)
{
    const std::filesystem::path path = katana::ifc::pathFromUtf8(arguments.path);
    katana::ifc::ImportOptions options;
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
    const std::size_t entities = imported->entities.size();
    const std::size_t alignments = imported->alignments.size();
    if (auto command = katana::ifc::importCommand(*imported, document.model())) {
        if (const auto status = document.execute(std::move(command)); !status) {
            return fail(status.error().describe());
        }
    }
    std::cout << "imported " << entities << " entities, " << alignments << " alignments and "
              << imported->surfaces.size() << " surfaces from " << path.filename().string() << " ("
              << imported->schema << ")\n";
    std::cout << "  " << imported->products << " objects read: " << imported->productsImported
              << " drawn, " << imported->productsAsPoints << " as a point at their placement; "
              << imported->alignmentsAsPolylines << " alignments as polylines\n";
    if (!imported->classes.empty()) {
        std::cout << "  classes:";
        for (const auto& [name, count] : imported->classes) {
            std::cout << ' ' << name << ' ' << count << ';';
        }
        std::cout << '\n';
    }
    if (!imported->surfaces.empty()) {
        std::cout << "  the command line holds no surfaces: the file's are counted, not kept\n";
    }
    const std::string& project = document.metadata().coordinateSystem;
    if (!imported->coordinateSystem.empty() && project.empty() && !arguments.local) {
        std::cout << "  the file is in " << imported->coordinateSystem
                  << " and the project has no coordinate system: CRS SET "
                  << imported->coordinateSystem << " sets it\n";
    } else if (!imported->coordinateSystem.empty() && !project.empty() &&
               project != imported->coordinateSystem) {
        std::cout << "  WARNING: the file is in " << imported->coordinateSystem
                  << " and the project in " << project << "; nothing was reprojected\n";
    }
    for (const std::string& warning : imported->warnings) {
        std::cout << "  " << warning << '\n';
    }
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
    return std::nullopt;
}

const char* ifcHelpText()
{
    return "IFC       IMPORT <file.ifc> [LOCAL]  alignments to PIs and PVIs, elements and\n"
           "          annotations with their property sets, as one undo step\n"
           "          EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]\n"
           "          [RULES <rules.csv>] [SPACING <m>] [NODRAWING]  IFC 4.3: alignments,\n"
           "          the drawing by class, an AS 5488 investigation graded, typed and\n"
           "          with its delivery schema\n";
}

} // namespace katana::app
