#include "ifc_verbs.hpp"

#include <cctype>
#include <chrono>
#include <filesystem>
#include <format>
#include <initializer_list>
#include <iostream>
#include <memory>
#include <string_view>
#include <utility>

#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/cad/project_crs.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/core/text.hpp"
#include "katana/cad/terrain/surface_verbs.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/front_end.hpp"
#include "katana/ifc/import.hpp"
#include "katana/terrain/surface_store.hpp"

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

// The value of the first of `object`'s properties called any of `names`, in
// any letter case; empty when it has none.
std::string propertyOf(const katana::cad::bim::BimObject& object,
                       std::initializer_list<std::string_view> names)
{
    for (const auto& [name, value] : object.properties) {
        for (const std::string_view wanted : names) {
            if (katana::core::equalsIgnoringCase(name, wanted)) {
                return value;
            }
        }
    }
    return {};
}

std::string capitals(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
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
    // ---- the drawing's surfaces (docs/terrain.md), hidden ones too: hiding
    // is a matter of the picture, and the window's export takes them all -
    // what a road model's cut voids, when it names one. Once: two tasks each
    // wrote this loop, and the merge kept both, so every surface went out
    // twice (cli.ifc_export_writes_each_surface_of_the_drawing_once).
    if (arguments.surfaces) {
        for (const katana::terrain::NamedSurface& named : document.surfaces().all()) {
            if (named.surface != nullptr) {
                input.surfaces.push_back({named.name, named.surface.get()});
            }
        }
    }
    // The model is made once per change of what it is made from and shared
    // (bim_object.hpp), so asking for it here costs nothing the views have
    // not already paid.
    const std::shared_ptr<const katana::cad::bim::BimModel> roadModel =
        arguments.roadModel ? katana::cad::bim::bimModel(document) : nullptr;
    if (roadModel != nullptr) {
        input.road = roadModelInput(*roadModel);
    }

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
    setLeaderNotes(options, document);
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
    // ---- the drawing's surfaces (docs/terrain.md, "Surfaces in the drawing") ----
    // A terrain the file carries is the drawing's, in the import's one step,
    // as the window keeps it.
    std::vector<katana::terrain::NamedSurface> surfaces;
    for (auto& surface : imported->surfaces) {
        katana::terrain::NamedSurface named;
        named.name = surface.name;
        named.surface =
            std::make_shared<const katana::terrain::TinSurface>(std::move(surface.surface));
        named.source = "IFC " + path.filename().string();
        surfaces.push_back(std::move(named));
    }
    auto kept = katana::cad::importSurfaces(
        document, katana::ifc::importCommand(*imported, document.model()), std::move(surfaces),
        "IMPORT " + path.filename().string());
    if (!kept) {
        return fail(kept.error().describe());
    }
    std::string reply =
        katana::ifc::formatImportReply(*imported, path.filename().string(), entities);
    for (const std::string& record : *kept) {
        reply += "\n" + record;
    }
    const auto note = [&](const std::string& text) {
        reply += "\n" + katana::ifc::noteRecord(text);
    };
    if (arguments.local && options.originShift) {
        note(std::format("shifted by {:.3f},{:.3f} to sit at the origin", options.originShift->x,
                         options.originShift->y));
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

katana::ifc::RoadModelInput roadModelInput(const katana::cad::bim::BimModel& model)
{
    katana::ifc::RoadModelInput out;
    out.elements.reserve(model.objects.size());
    for (const katana::cad::bim::BimObject& object : model.objects) {
        katana::ifc::RoadElement element;
        element.id = object.id;
        element.name = object.name;
        element.kind = object.kind;
        element.ifcClass.entity = object.ifcClass;
        element.ifcClass.predefinedType = object.predefinedType;
        if (object.predefinedType == "USERDEFINED") {
            // What a USERDEFINED one is, which the schema requires (each
            // class's CorrectPredefinedType rule): the object's own word for
            // it, else its kind.
            element.ifcClass.objectType = propertyOf(object, {"ObjectType"});
            if (element.ifcClass.objectType.empty()) {
                element.ifcClass.objectType = capitals(object.kind);
            }
        }
        element.facility = object.facility;
        element.part = object.part;
        element.material = object.material;
        element.colour = object.colour;
        element.mesh = object.mesh;
        element.plan = object.plan;
        element.quantities.reserve(object.quantities.size());
        for (const katana::cad::bim::Quantity& quantity : object.quantities) {
            element.quantities.push_back({quantity.name, quantity.value, quantity.unit});
        }
        element.properties = object.properties;
        element.layer = object.layer;
        element.host = propertyOf(object, {"Host"});
        element.ground = propertyOf(object, {"Ground", "Surface", "Target Surface"});
        out.elements.push_back(std::move(element));
    }
    out.problems.reserve(model.problems.size());
    for (const katana::cad::bim::BimProblem& problem : model.problems) {
        out.problems.push_back(problem.source.empty() ? problem.message
                                                      : problem.source + ": " + problem.message);
    }
    return out;
}

void setLeaderNotes(katana::ifc::ExportOptions& options, const katana::cad::Document& document)
{
    options.leaderContext = katana::cad::annotation::leaderContext(document);
    options.codeProperties = katana::cad::codePropertyCandidates();
}

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
           "          [NOALIGNMENTS] [NOSURFACES] [NOROADMODEL] [PREVIEW]  IFC 4.3:\n"
           "          alignments, the drawing by class, the road model as IfcRoad and its\n"
           "          parts, an AS 5488 investigation graded, typed and with its delivery\n"
           "          schema; PREVIEW writes nothing\n"
           "          INFO <file.ifc>  what the file holds, without importing it\n"
           "          IFC RULES <file.csv>  the default classification rules, to edit\n"
           "          (replies are key=value records, docs/ifc.md)\n";
}

} // namespace katana::app
