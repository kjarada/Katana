#include "ifc_verbs.hpp"

#include <chrono>
#include <filesystem>
#include <format>
#include <fstream>
#include <iostream>
#include <iterator>
#include <vector>

#include "katana/cad/project_crs.hpp"
#include "katana/core/text.hpp"
#include "katana/ifc/export.hpp"
#include "katana/survey/subsurface/delivery_schema.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"

#ifndef KATANA_VERSION
#define KATANA_VERSION ""
#endif

namespace katana::app {

namespace {

namespace sub = katana::survey::subsurface;

// Blank-separated words, a double-quoted word kept whole with its blanks.
// nullopt for a quote that is never closed.
std::optional<std::vector<std::string>> words(std::string_view text)
{
    std::vector<std::string> out;
    std::size_t at = 0;
    while (at < text.size()) {
        if (text[at] == ' ' || text[at] == '\t') {
            ++at;
            continue;
        }
        if (text[at] == '"') {
            const std::size_t end = text.find('"', at + 1);
            if (end == std::string_view::npos) {
                return std::nullopt;
            }
            out.emplace_back(text.substr(at + 1, end - at - 1));
            at = end + 1;
            continue;
        }
        const std::size_t end = text.find_first_of(" \t", at);
        out.emplace_back(text.substr(at, end == std::string_view::npos ? end : end - at));
        at = end == std::string_view::npos ? text.size() : end;
    }
    return out;
}

std::optional<std::string> readFile(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

bool fail(const std::string& message)
{
    std::cerr << "error: " << message << '\n';
    return false;
}

// The path, and what follows it: a quoted path, or everything up to the
// first ".ifc" that ends a word - so that an unquoted path with blanks in it
// still reads, as EXPORT's other paths do. nullopt when there is no .ifc.
std::optional<std::pair<std::string, std::string_view>> splitPath(std::string_view argument)
{
    argument = core::trimmed(argument);
    if (argument.starts_with('"')) {
        const std::size_t end = argument.find('"', 1);
        if (end == std::string_view::npos) {
            return std::nullopt;
        }
        std::string path(argument.substr(1, end - 1));
        if (!katana::ifc::isIfcPath(path)) {
            return std::nullopt;
        }
        return std::pair(std::move(path), argument.substr(end + 1));
    }
    const std::string lower = core::lowered(argument);
    for (std::size_t at = lower.find(".ifc"); at != std::string::npos;
         at = lower.find(".ifc", at + 1)) {
        const std::size_t end = at + 4;
        if (end == lower.size() || lower[end] == ' ' || lower[end] == '\t') {
            return std::pair(std::string(argument.substr(0, end)), argument.substr(end));
        }
    }
    return std::nullopt;
}

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
}

bool exportIfc(const katana::cad::Document& document, const std::filesystem::path& path,
               std::string_view rest)
{
    const auto keywords = words(rest);
    if (!keywords) {
        return fail("InvalidArgument: a quoted path is never closed");
    }
    std::optional<std::string> schedulePath;
    std::optional<std::string> schemaPath;
    std::optional<double> spacing;
    bool drawing = true;
    for (std::size_t i = 0; i < keywords->size(); ++i) {
        const std::string keyword = upper((*keywords)[i]);
        const bool hasValue = i + 1 < keywords->size();
        if (keyword == "UTILITIES" && hasValue) {
            schedulePath = (*keywords)[++i];
        } else if (keyword == "SCHEMA" && hasValue) {
            schemaPath = (*keywords)[++i];
        } else if (keyword == "SPACING" && hasValue) {
            spacing = core::parseFiniteDouble((*keywords)[++i]);
            if (!spacing || *spacing <= 0.0) {
                return fail("InvalidArgument: SPACING takes a positive number of metres");
            }
        } else if (keyword == "NODRAWING") {
            drawing = false;
        } else {
            return fail("InvalidArgument: \"" + (*keywords)[i] +
                        "\" is not an option of EXPORT <file.ifc>: UTILITIES <schedule.csv>, "
                        "SCHEMA <schema.csv>, SPACING <m>, NODRAWING");
        }
    }
    if (schemaPath && !schedulePath) {
        return fail("InvalidArgument: SCHEMA describes a schedule: give it with UTILITIES");
    }

    katana::ifc::ExportInput input;
    if (drawing) {
        input.model = &document.model();
    }
    sub::DeliverySchema schema;
    if (schedulePath) {
        const auto text = readFile(*schedulePath);
        if (!text) {
            return fail("NotFound: cannot read " + *schedulePath);
        }
        auto lines = sub::parseUtilityCsv(*text);
        if (!lines) {
            return fail(lines.error().describe() + " in " + *schedulePath);
        }
        katana::ifc::UtilityInput utilities;
        utilities.lines = std::move(*lines);
        utilities.sourceName = std::filesystem::path(*schedulePath).filename().string();
        if (spacing) {
            utilities.grading.maximumDetectedSpacing = *spacing;
        }
        if (schemaPath) {
            const auto schemaText = readFile(*schemaPath);
            if (!schemaText) {
                return fail("NotFound: cannot read " + *schemaPath);
            }
            auto parsed = sub::parseDeliverySchema(*schemaText);
            if (!parsed) {
                return fail(parsed.error().describe() + " in " + *schemaPath);
            }
            schema = std::move(*parsed);
            utilities.schema = &schema;
        }
        input.utilities = std::move(utilities);
    }

    const auto& metadata = document.metadata();
    katana::ifc::ExportOptions options;
    options.projectName = metadata.name;
    options.projectDescription = metadata.description;
    options.applicationVersion = KATANA_VERSION;
    // The export's clock is the caller's, so that the writer stays a
    // function of what it is given.
    options.timestamp = std::format(
        "{:%Y-%m-%dT%H:%M:%S}",
        std::chrono::floor<std::chrono::seconds>(std::chrono::system_clock::now()));
    // A project is the same project across sessions by its name and when it
    // was created, so its objects keep their GlobalIds from one export to
    // the next.
    options.guidNamespace = metadata.name + "/" + metadata.createdUtc;
    options.georeference.name = metadata.coordinateSystem;
    if (!metadata.coordinateSystem.empty()) {
        if (const auto described = katana::cad::describeCoordinateSystem(metadata.coordinateSystem)) {
            options.georeference.description = described->name;
        }
    }

    const auto written = katana::ifc::writeIfcFile(input, path, options);
    if (!written) {
        return fail(written.error().describe());
    }
    std::cout << "exported " << path.filename().string() << " (IFC4X3_ADD2, " << written->instances
              << " instances, " << written->bytesWritten << " bytes)\n";
    std::cout << "  " << written->alignments << " alignments, " << written->services
              << " services (" << written->serviceSegments << " segments, "
              << written->segmentsIn3d << " in 3D, " << written->locatedPoints
              << " located points), " << written->entitiesWritten << " entities written, "
              << written->entitiesSkipped << " skipped, " << written->surfaces << " surfaces\n";
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

} // namespace

std::optional<bool> runIfcVerb(const katana::cad::Document& document, std::string_view verb,
                               std::string_view argument)
{
    if (verb != "EXPORT") {
        return std::nullopt;
    }
    const auto split = splitPath(argument);
    if (!split) {
        return std::nullopt;
    }
    return exportIfc(document, split->first, split->second);
}

const char* ifcHelpText()
{
    return "IFC       EXPORT <file.ifc> [UTILITIES <schedule.csv>] [SCHEMA <schema.csv>]\n"
           "          [SPACING <m>] [NODRAWING]  IFC 4.3: alignments, the drawing by class,\n"
           "          an AS 5488 investigation graded, typed and with its delivery schema\n";
}

} // namespace katana::app
