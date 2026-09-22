// katana_cli: the Katana engine without a GUI.
//
//   katana_cli                      interactive session on stdin
//   katana_cli script.kcs           run a script, one command per line
//   katana_cli -c "LINE 0,0 5,5" -c LIST
//
// Scripts and -c commands stop at the first failing command and exit with
// status 1, so the tool can be used in automated pipelines. Lines starting with
// '#' are comments. It drives exactly the same Document, commands and storage
// as the desktop application.

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"

#if defined(KATANA_WITH_INTEROP)
#include "katana/commands/entity_commands.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"
#endif

namespace {

bool isQuit(const std::string& line)
{
    std::string verb;
    for (const char ch : line) {
        if (ch == ' ' || ch == '\t') {
            if (!verb.empty()) {
                break;
            }
            continue;
        }
        verb += static_cast<char>(std::toupper(static_cast<unsigned char>(ch)));
    }
    return verb == "QUIT" || verb == "EXIT";
}

#if defined(KATANA_WITH_INTEROP)

// IMPORT, EXPORT and REFS live here rather than in CommandInterpreter because
// the interpreter belongs to katana_cad, which must not see GDAL or PDAL - that
// separation is what lets katana_cad build with -DKATANA_BUILD_IO=OFF. The
// front ends own the interoperability, and both front ends offer the same verbs.
struct InteropState {
    katana::interop::ReferenceData reference;
};

// The rest of the line after the verb: leading and trailing blanks removed, and
// one layer of surrounding quotes stripped, so a path with spaces works.
std::string argumentOf(const std::string& line, std::size_t verbLength)
{
    std::string rest = line.substr(verbLength);
    const auto first = rest.find_first_not_of(" \t");
    if (first == std::string::npos) {
        return {};
    }
    const auto last = rest.find_last_not_of(" \t");
    rest = rest.substr(first, last - first + 1);
    if (rest.size() >= 2 && rest.front() == '"' && rest.back() == '"') {
        rest = rest.substr(1, rest.size() - 2);
    }
    return rest;
}

std::string upperVerb(const std::string& line)
{
    std::string verb;
    for (const char ch : line) {
        if (ch == ' ' || ch == '\t') {
            if (!verb.empty()) {
                break;
            }
            continue;
        }
        verb += static_cast<char>(std::toupper(static_cast<unsigned char>(ch)));
    }
    return verb;
}

bool importPath(katana::cad::Document& document, InteropState& state, const std::string& text)
{
    namespace interop = katana::interop;
    namespace cmd = katana::commands;
    const std::filesystem::path path(text);

    // IMPORT <file> LOCAL shifts the data to sit alongside the drawing instead
    // of at its own survey coordinates.
    std::filesystem::path file = path;
    bool shiftToLocal = false;
    {
        const std::string raw = path.string();
        const std::size_t space = raw.find_last_of(" 	");
        if (space != std::string::npos) {
            std::string tail = raw.substr(space + 1);
            std::transform(tail.begin(), tail.end(), tail.begin(),
                           [](unsigned char c) { return static_cast<char>(std::toupper(c)); });
            if (tail == "LOCAL") {
                shiftToLocal = true;
                file = std::filesystem::path(raw.substr(0, space));
            }
        }
    }

    switch (interop::kindForPath(file)) {
    case interop::SourceKind::Vector: {
        interop::VectorImportOptions options;
        auto probe = interop::importVector(file, options);
        if (probe && shiftToLocal && !probe->bounds.empty()) {
            options.originShift = katana::geometry::Vec2(probe->bounds.min.x, probe->bounds.min.y);
        }
        auto imported = shiftToLocal ? interop::importVector(file, options) : std::move(probe);
        if (!imported) {
            std::cerr << "error: " << imported.error().describe() << '\n';
            return false;
        }
        auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
        for (const std::string& name : imported->layersNeeded) {
            if (!document.model().layers.contains(name)) {
                katana::entity::Layer layer;
                layer.name = name;
                transaction->add(cmd::createLayer(layer));
            }
        }
        const std::size_t count = imported->entities.size();
        const auto bounds = imported->bounds;
        const auto existingBounds = document.model().entities.bounds();
        transaction->add(cmd::createEntities(std::move(imported->entities)));
        const auto status = document.execute(std::move(transaction));
        if (!status) {
            std::cerr << "error: " << status.error().describe() << '\n';
            return false;
        }
        std::cout << "imported " << count << " entities from " << file.filename().string()
                  << '\n';
        for (const std::string& warning : imported->warnings) {
            std::cout << "  " << warning << '\n';
        }
        if (!bounds.empty()) {
            std::cout << "  extent " << bounds.min.x << "," << bounds.min.y << " to "
                      << bounds.max.x << "," << bounds.max.y << '\n';
        }
        // Said out loud rather than left for the user to discover by zooming to
        // extents and finding their drawing has become a dot.
        const auto advice = interop::advisePlacement(existingBounds, bounds);
        if (advice.farApart) {
            std::cout << "  WARNING: " << advice.message << '\n'
                      << "  undo, then re-import with  IMPORT <file> LOCAL  to shift it "
                         "alongside the drawing\n";
        }
        return true;
    }
    case interop::SourceKind::Archive12d: {
        interop::Archive12dImportOptions options;
        auto probe = interop::importArchive12d(file, options);
        if (probe && shiftToLocal && !probe->bounds.empty()) {
            options.originShift = katana::geometry::Vec2(probe->bounds.min.x, probe->bounds.min.y);
        }
        auto imported = shiftToLocal && probe ? interop::importArchive12d(file, options)
                                             : std::move(probe);
        if (!imported) {
            std::cerr << "error: " << imported.error().describe() << '\n';
            return false;
        }
        // Layers, entities and alignments are ONE undo step, like any import.
        auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
        for (const katana::entity::Layer& layer : imported->layersNeeded) {
            if (!document.model().layers.contains(layer.name)) {
                transaction->add(cmd::createLayer(layer));
            }
        }
        const std::size_t count = imported->entities.size();
        const auto existingBounds = document.model().entities.bounds();
        if (count != 0) {
            transaction->add(cmd::createEntities(std::move(imported->entities)));
        }
        std::size_t alignments = 0;
        for (katana::entity::Alignment& alignment : imported->alignments) {
            // A name the drawing already has would fail the whole transaction.
            const std::string base = alignment.name;
            for (int copy = 2; document.model().alignments.contains(alignment.name); ++copy) {
                alignment.name = base + " (" + std::to_string(copy) + ")";
            }
            transaction->add(cmd::createAlignment(alignment));
            ++alignments;
        }
        const auto status = document.execute(std::move(transaction));
        if (!status) {
            std::cerr << "error: " << status.error().describe() << '\n';
            return false;
        }
        std::cout << "imported " << count << " entities and " << alignments
                  << " alignments from " << file.filename().string() << " ("
                  << imported->encoding;
        if (!imported->archiveVersion.empty()) {
            std::cout << ", 12d archive " << imported->archiveVersion;
        }
        std::cout << ")\n";
        for (const auto& tally : imported->tally) {
            std::cout << "  " << tally.keyword << ": " << tally.read << " read, " << tally.imported
                      << " imported\n";
        }
        for (const auto& surface : imported->surfaces) {
            // The CLI has nowhere to keep a surface; the desktop application
            // does. Said, so that "12 tins read" is not taken for "12 kept".
            std::cout << "  surface \"" << surface.name << "\": "
                      << surface.surface.triangleCount() << " triangles (read and checked; the "
                      << "CLI holds no surfaces - import in the desktop application to use it)\n";
        }
        for (auto& cloud : imported->clouds) {
            std::cout << "  point cloud \"" << cloud.name << "\": " << cloud.points.size()
                      << " points\n";
            state.reference.add(std::move(cloud));
        }
        for (const std::string& warning : imported->warnings) {
            std::cout << "  " << warning << '\n';
        }
        if (!imported->bounds.empty()) {
            std::cout << "  extent " << imported->bounds.min.x << "," << imported->bounds.min.y
                      << " to " << imported->bounds.max.x << "," << imported->bounds.max.y << '\n';
        }
        const auto advice = interop::advisePlacement(existingBounds, imported->bounds);
        if (advice.farApart) {
            std::cout << "  WARNING: " << advice.message << '\n'
                      << "  undo, then re-import with  IMPORT <file> LOCAL  to shift it "
                         "alongside the drawing\n";
        }
        return true;
    }
    case interop::SourceKind::Raster: {
        auto raster = interop::importRaster(path);
        if (!raster) {
            std::cerr << "error: " << raster.error().describe() << '\n';
            return false;
        }
        const int w = raster->width;
        const int h = raster->height;
        const bool georeferenced = raster->hasGeotransform;
        const auto bounds = raster->worldBounds();
        state.reference.add(std::move(*raster));
        std::cout << "imported raster " << path.filename().string() << " (" << w << "x" << h
                  << " px)";
        if (georeferenced) {
            std::cout << " covering " << bounds.min.x << ',' << bounds.min.y << " to "
                      << bounds.max.x << ',' << bounds.max.y;
        } else {
            std::cout << " (not georeferenced; placed at the origin)";
        }
        std::cout << '\n';
        return true;
    }
    case interop::SourceKind::PointCloud: {
        auto cloud = interop::importPointCloud(path);
        if (!cloud) {
            std::cerr << "error: " << cloud.error().describe() << '\n';
            return false;
        }
        const std::size_t shown = cloud->points.size();
        const std::uint64_t total = cloud->sourcePointCount;
        const bool decimated = cloud->isDecimated();
        state.reference.add(std::move(*cloud));
        std::cout << "imported point cloud " << path.filename().string() << " (" << shown
                  << " points";
        if (decimated) {
            std::cout << " sampled from " << total;
        }
        std::cout << ")\n";
        return true;
    }
    case interop::SourceKind::Unknown:
        break;
    }
    std::cerr << "error: Unsupported: no importer for '" << path.extension().string() << "'\n";
    return false;
}

bool exportPath(katana::cad::Document& document, const std::string& text)
{
    namespace interop = katana::interop;
    const std::filesystem::path target(text);
    if (interop::kindForPath(target) == interop::SourceKind::Archive12d) {
        // No surfaces: the CLI holds none.
        auto archive = interop::exportArchive12d(document.model(), {}, target);
        if (!archive) {
            std::cerr << "error: " << archive.error().describe() << '\n';
            return false;
        }
        std::cout << "exported " << archive->entitiesWritten << " entities and "
                  << archive->alignmentsWritten << " alignments (12d archive, "
                  << archive->bytesWritten << " bytes)\n";
        for (const std::string& warning : archive->warnings) {
            std::cout << "  " << warning << '\n';
        }
        return true;
    }
    auto result = interop::exportVector(document.model(), target);
    if (!result) {
        std::cerr << "error: " << result.error().describe() << '\n';
        return false;
    }
    std::cout << "exported " << result->featuresWritten << " features (" << result->driver
              << ")\n";
    for (const std::string& warning : result->warnings) {
        std::cout << "  " << warning << '\n';
    }
    return true;
}

void listReferences(const InteropState& state)
{
    if (state.reference.empty()) {
        std::cout << "no reference layers\n";
        return;
    }
    for (const auto& raster : state.reference.rasters()) {
        std::cout << raster.id << "  Raster  " << raster.name << "  " << raster.width << "x"
                  << raster.height << " px\n";
    }
    for (const auto& cloud : state.reference.pointClouds()) {
        std::cout << cloud.id << "  PointCloud  " << cloud.name << "  " << cloud.points.size()
                  << " pts\n";
    }
}

// nullopt when the line is not one of the interoperability verbs.
std::optional<bool> runInterop(katana::cad::Document& document, InteropState& state,
                               const std::string& line)
{
    const std::string verb = upperVerb(line);
    if (verb == "IMPORT") {
        const std::string argument = argumentOf(line, line.find_first_of(" \t") == std::string::npos
                                                          ? line.size()
                                                          : line.find_first_of(" \t"));
        if (argument.empty()) {
            std::cerr << "error: InvalidArgument: usage: IMPORT <file>\n";
            return false;
        }
        return importPath(document, state, argument);
    }
    if (verb == "EXPORT") {
        const std::string argument = argumentOf(line, line.find_first_of(" \t") == std::string::npos
                                                          ? line.size()
                                                          : line.find_first_of(" \t"));
        if (argument.empty()) {
            std::cerr << "error: InvalidArgument: usage: EXPORT <file>\n";
            return false;
        }
        return exportPath(document, argument);
    }
    if (verb == "REFS") {
        listReferences(state);
        return true;
    }
    return std::nullopt;
}

#endif // KATANA_WITH_INTEROP

// One session's mutable state. Grouped so that adding a verb that needs more
// context does not mean changing every signature again.
struct Session {
    katana::cad::Document& document;
    katana::cad::CommandInterpreter& interpreter;
#if defined(KATANA_WITH_INTEROP)
    InteropState interop;
#endif
};

// Returns false when the command failed.
bool runLine(Session& session, const std::string& line)
{
    if (!line.empty() && line.front() == '#') {
        return true;
    }
#if defined(KATANA_WITH_INTEROP)
    // Interoperability verbs are handled before the interpreter sees the line,
    // because they live above katana_cad rather than inside it.
    if (const std::optional<bool> handled =
            runInterop(session.document, session.interop, line)) {
        return *handled;
    }
#endif
    const auto reply = session.interpreter.run(line);
    if (!reply) {
        std::cerr << "error: " << reply.error().describe() << '\n';
        return false;
    }
    if (!reply->empty()) {
        std::cout << *reply << '\n';
    }
    return true;
}

int runBatch(Session& session, const std::vector<std::string>& lines)
{
    for (const std::string& line : lines) {
        if (isQuit(line)) {
            break;
        }
        if (!runLine(session, line)) {
            return 1;
        }
    }
    return 0;
}

} // namespace

int main(int argc, char* argv[])
{
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter(document);
#if defined(KATANA_WITH_INTEROP)
    Session session{document, interpreter, {}};
#else
    Session session{document, interpreter};
#endif

    std::vector<std::string> batch;
    for (int i = 1; i < argc; ++i) {
        const std::string argument = argv[i];
        if (argument == "-h" || argument == "--help") {
            std::cout << "usage: katana_cli [script-file] [-c \"command\"]...\n\n"
                      << katana::cad::CommandInterpreter::helpText() << '\n';
#if defined(KATANA_WITH_INTEROP)
            std::cout << "Interop   IMPORT <file> | EXPORT <file> | REFS\n"
                      << "          vector -> entities; raster and point cloud -> "
                         "reference layers\n";
#endif
            return 0;
        }
        if (argument == "-c") {
            if (i + 1 >= argc) {
                std::cerr << "error: -c needs a command\n";
                return 2;
            }
            batch.emplace_back(argv[++i]);
            continue;
        }
        std::ifstream script(argument);
        if (!script) {
            std::cerr << "error: cannot read script " << argument << '\n';
            return 2;
        }
        for (std::string line; std::getline(script, line);) {
            if (!line.empty() && line.back() == '\r') {
                line.pop_back(); // scripts saved with Windows line endings
            }
            batch.push_back(std::move(line));
        }
    }
    if (!batch.empty()) {
        return runBatch(session, batch);
    }

    std::cout << "Katana command line. HELP lists commands, QUIT leaves.\n";
    for (std::string line; std::cout << "> " << std::flush, std::getline(std::cin, line);) {
        if (isQuit(line)) {
            break;
        }
        (void)runLine(session, line); // interactive: report and carry on
    }
    return 0;
}
