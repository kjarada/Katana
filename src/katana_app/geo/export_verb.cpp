// EXPORT (docs/interop.md, "IMPORT, EXPORT, INFO, REFS and COPC on every front
// end", "Export options"): the drawing - or what the shared scope and filter
// take of it - to a file by its extension: a DXF natively, a 12d archive with
// the session's surfaces, any other through GDAL - run as the one executor
// runs every verb.
//
//   EXPORT <file> [<scope>] [layername=<n> | split=layer] [append]
//          [crs=project|native|<code>] [co=K=V]... [lco=K=V]...
//          [text=points|skip] [curve=<m>] [properties=yes|no] [PREVIEW]
//
// The scope is the one grammar (cad::parseScopeWords, resolved by
// cad::matchScope): no scope words is the whole drawing, as EXPORT always
// was. The reply says what the scope took. The options after it are GDAL's
// formats' own; a .dxf or a .12da takes the scope alone.
//
// The drawing is copied when the line is prepared, so the work writes on a
// worker while the window stays live; the file is written beside its place
// and moved there by the apply (staged_files.hpp), so a cancelled EXPORT
// writes nothing - an append included, which adds to a copy of the file.

#include <algorithm>
#include <filesystem>
#include <memory>
#include <set>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "../dxf_verbs.hpp"
#include "../import_records.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/gis/formats.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "replies.hpp"
#include "staged_files.hpp"
#include "vector_support.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace interop = katana::interop;
namespace vec = katana::app::geo::vector;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage =
    "usage: EXPORT <file> [<scope>] [layername=<n> | split=layer] [append] "
    "[crs=project|native|<code>] [co=K=V]... [lco=K=V]... [text=points|skip] [curve=<m>] "
    "[properties=yes|no] [PREVIEW]";

// The option keys, lower case. co= and lco= are read apart: each may be given
// more than once. layername= and not layer=: an option key is never a WHERE
// key, and WHERE's LAYER= would be taken for it (vector_support.hpp).
constexpr const char* kOptions[] = {"layername", "split", "crs", "text", "curve", "properties"};

enum class Kind { Dxf, Archive, Vector };

std::string kindWord(Kind kind)
{
    return kind == Kind::Dxf ? "dxf" : kind == Kind::Archive ? "archive" : "vector";
}

// Whether word `i` begins the words after the path: an option, a flag or a
// scope word. A path runs to the first of them, so one with a blank still
// reads without quotes.
bool beginsOptions(const Tokens& tokens, std::size_t i)
{
    const std::string& word = tokens[i];
    const std::size_t equals = word.find('=');
    if (equals != std::string::npos && equals > 0) {
        const std::string key = katana::core::lowered(word.substr(0, equals));
        if (key == "co" || key == "lco" ||
            std::ranges::any_of(kOptions, [&key](const char* each) { return key == each; })) {
            return true;
        }
    }
    if (tokens.quoted[i]) {
        return false;
    }
    return tokens.is(i, "APPEND") || tokens.is(i, "PREVIEW") || katana::cad::isScopeWord(word);
}

// What the work writes from: copies, never the Document.
struct Snapshot {
    katana::entity::Model model;
    double annotationScale = 1000.0;
    std::vector<katana::terrain::NamedSurface> surfaces;
    // The project's coordinate system: it goes into the file, and is what a
    // KML or GPX is converted to longitude and latitude from (docs/interop.md,
    // "Fidelity").
    std::string projectionWkt;
    // The GDAL formats' options, with the entities the scope took.
    interop::VectorExportOptions vector;
    // Records said after the exported one: what the scope took, a conversion.
    std::vector<std::string> extra;
};

Result<std::string> write(Kind kind, const Snapshot& snapshot, const std::filesystem::path& to,
                          const std::filesystem::path& named)
{
    std::string reply;
    switch (kind) {
    case Kind::Dxf: {
        auto written = writeDxfExport(snapshot.model, snapshot.annotationScale, to);
        if (!written) {
            return written.error();
        }
        reply = dxfExportRecords(*written, named);
        break;
    }
    case Kind::Archive: {
        // A 12d archive carries what the other formats cannot: the session's
        // surfaces go with the drawing.
        std::vector<katana::archive12d::ExportSurface> surfaces;
        for (const katana::terrain::NamedSurface& surface : snapshot.surfaces) {
            surfaces.push_back({surface.name, surface.surface.get()});
        }
        auto written = interop::exportArchive12d(snapshot.model, surfaces, to);
        if (!written) {
            return written.error();
        }
        reply = "exported file=" + recordText(pathText(named)) +
                " kind=archive driver=12da entities=" + std::to_string(written->entitiesWritten) +
                " skipped=" + std::to_string(written->entitiesSkipped) +
                " alignments=" + std::to_string(written->alignmentsWritten) +
                " surfaces=" + std::to_string(written->surfacesWritten) +
                " bytes=" + std::to_string(written->bytesWritten);
        for (const std::string& warning : written->warnings) {
            reply += "\n" + warningText(warning);
        }
        break;
    }
    case Kind::Vector: {
        interop::VectorExportOptions options = snapshot.vector;
        options.projectionWkt = snapshot.projectionWkt;
        auto written = interop::exportVector(snapshot.model, to, options);
        if (!written) {
            return written.error();
        }
        std::string layers;
        for (const std::string& layer : written->layers) {
            layers += (layers.empty() ? "" : ",") + layer;
        }
        std::string crs = snapshot.projectionWkt;
        if (!options.targetCrs.empty()) {
            crs = katana::gis::crsToWkt(options.targetCrs).valueOr(options.targetCrs);
        }
        reply = "exported file=" + recordText(pathText(named)) +
                " kind=vector driver=" + recordText(written->driver) +
                " features=" + std::to_string(written->featuresWritten) +
                " skipped=" + std::to_string(written->entitiesSkipped) +
                " layers=" + recordText(layers) +
                " crs=" + recordText(crs.empty() ? std::string() : katana::gis::describeCrs(crs));
        if (options.textAsPoints) {
            reply += " texts=" + std::to_string(written->textsWritten);
        }
        if (options.append) {
            reply += " append=yes";
        }
        for (const std::string& warning : written->warnings) {
            reply += "\n" + warningText(warning);
        }
        break;
    }
    }
    // The exported record first, as it always was: a reader of the first
    // record keeps finding it there.
    std::string extra;
    for (const std::string& record : snapshot.extra) {
        extra += "\n" + record;
    }
    const std::size_t end = reply.find('\n');
    reply.insert(end == std::string::npos ? reply.size() : end, extra);
    return reply;
}

} // namespace

Result<Prepared> prepareExport(Context& context, const Tokens& tokens, std::string_view line)
{
    // The path, then the words after it; with nothing after it the line reads
    // as it always did (restOfLine), a path with blanks included.
    if (tokens.size() < 2) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    std::size_t first = 2;
    std::string path;
    if (tokens.quoted[1]) {
        path = tokens[1];
    } else {
        while (first < tokens.size() && !beginsOptions(tokens, first)) {
            ++first;
        }
        for (std::size_t i = 1; i < first; ++i) {
            path += (i > 1 ? " " : "") + tokens[i];
        }
    }
    if (first >= tokens.size()) {
        path = restOfLine(line);
    }
    if (katana::core::trimmed(path).empty() || beginsOptions(tokens, 1)) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    const std::filesystem::path file = pathFromText(path);
    const Kind kind = katana::dxf::isDxfPath(file) ? Kind::Dxf
                      : interop::kindForPath(file) == interop::SourceKind::Archive12d
                          ? Kind::Archive
                          : Kind::Vector;

    // co= and lco= may be given more than once: taken out before the rest.
    Tokens rest;
    std::vector<std::string> creation, layerCreation;
    for (std::size_t i = first; i < tokens.size(); ++i) {
        const std::string& word = tokens[i];
        const std::string folded = katana::core::lowered(word.substr(0, 4));
        if (folded.starts_with("co=")) {
            creation.push_back(word.substr(3));
            continue;
        }
        if (folded.starts_with("lco=")) {
            layerCreation.push_back(word.substr(4));
            continue;
        }
        rest.words.push_back(word);
        rest.quoted.push_back(tokens.quoted[i]);
    }
    vec::WordRules rules;
    for (const char* key : kOptions) {
        rules.options.emplace_back(key);
    }
    rules.flags = {"APPEND", "PREVIEW"};
    rules.usage = kUsage;
    auto words = vec::readVerbWords(rest, 0, rest.size(), rules);
    if (!words) {
        return words.error();
    }
    const bool preview = words->has("PREVIEW");

    // A DXF and a 12d archive take the scope alone: GDAL's options are for
    // GDAL's formats. Refused by name rather than ignored.
    if (kind != Kind::Vector) {
        std::string given;
        if (!words->options.empty()) {
            given = words->options.begin()->first + "=";
        } else if (!creation.empty()) {
            given = "co=";
        } else if (!layerCreation.empty()) {
            given = "lco=";
        } else if (words->has("APPEND")) {
            given = "append";
        }
        if (!given.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             given + " is an option of a GDAL format; a " + kindWord(kind) +
                                 " export takes a scope and PREVIEW",
                             pathText(file));
        }
    }

    interop::VectorExportOptions vector;
    std::vector<std::string> extra;
    const std::string project = context.document.metadata().coordinateSystem;
    std::string driver;
    if (kind == Kind::Vector) {
        auto found = katana::gis::GdalDataset::vectorDriverForPath(file);
        if (!found) {
            return found.error();
        }
        driver = *found;
        if (const std::string* name = words->option("layername")) {
            if (katana::core::trimmed(*name).empty()) {
                return makeError(ErrorCode::InvalidArgument, "layername= names the file's layer");
            }
            vector.layerName = std::string(katana::core::trimmed(*name));
        }
        if (const std::string* split = words->option("split")) {
            if (katana::core::lowered(*split) != "layer") {
                return makeError(ErrorCode::InvalidArgument,
                                 "split=layer writes one file layer per drawing layer",
                                 "split=" + *split);
            }
            if (words->option("layername") != nullptr) {
                return makeError(ErrorCode::InvalidArgument,
                                 "split=layer names each file layer after its drawing layer: "
                                 "give it without layername=");
            }
            vector.splitByLayer = true;
        }
        vector.append = words->has("APPEND");
        if (vector.append && !creation.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "co= are the options of a new file, and append adds to one that is "
                             "there: give lco= for the new layer's",
                             "co=" + creation.front());
        }
        if (auto checked = katana::gis::checkOptions(driver, katana::gis::OptionList::Creation,
                                                     creation);
            !checked) {
            return checked.error();
        }
        if (auto checked = katana::gis::checkOptions(
                driver, katana::gis::OptionList::LayerCreation, layerCreation);
            !checked) {
            return checked.error();
        }
        vector.creationOptions = std::move(creation);
        vector.layerCreationOptions = std::move(layerCreation);
        auto text = vec::choiceOption(*words, "text", {"points", "skip"}, "skip");
        if (!text) {
            return text.error();
        }
        vector.textAsPoints = *text == "points";
        auto properties = vec::choiceOption(*words, "properties", {"yes", "no"}, "yes");
        if (!properties) {
            return properties.error();
        }
        vector.propertiesAsAttributes = *properties == "yes";
        auto curve = vec::numberOption(*words, "curve");
        if (!curve) {
            return curve.error();
        }
        if (*curve) {
            if (!(**curve > 0.0)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "curve= is the largest distance a chord may stray from its arc, "
                                 "above 0");
            }
            vector.curveTolerance = **curve;
        }

        // crs=: the project's by default; native keeps it where the format
        // allows; a code moves the coordinates on the way out. A GeoJSON by
        // default is written as RFC 7946 (section 4) says it is - longitude
        // and latitude on WGS 84 - converted from the project's, where GDAL's
        // writer would write projected coordinates in a file most readers
        // take for degrees.
        const std::string* crs = words->option("crs");
        const std::string asked =
            crs != nullptr ? std::string(katana::core::trimmed(*crs)) : std::string();
        const std::string folded = katana::core::lowered(asked);
        if (folded == "native") {
            // The project's, unconverted.
        } else if (!asked.empty() && folded != "project") {
            auto readable = katana::gis::crsToWkt(asked);
            if (!readable) {
                return readable.error();
            }
            if (project.empty()) {
                return makeError(ErrorCode::InvalidCRS,
                                 "crs=" + asked +
                                     " moves the drawing's coordinates, and the project has no "
                                     "coordinate system to move them from: CRS SET <code> first");
            }
            vector.targetCrs = asked;
        } else if (katana::gis::driverAssumesWgs84(driver) && !project.empty() &&
                   katana::gis::crsEpsgCode(project) != std::optional<int>(4326)) {
            vector.targetCrs = "EPSG:4326";
            extra.push_back(warningText(
                "GeoJSON is longitude and latitude on WGS 84 (RFC 7946): the coordinates were "
                "converted from the project's; crs=native keeps the project's"));
        }
    }

    // The drawing as it is now, copied for the worker; the copy's entity
    // observer is the Document's, and a copy must never report to it.
    auto snapshot = std::make_shared<Snapshot>();
    snapshot->model = context.document.model();
    snapshot->model.entities.setObserver({});
    snapshot->annotationScale = context.document.annotationScale();
    snapshot->projectionWkt = project;

    // What the scope took; no scope is the whole drawing, as EXPORT was.
    bool wholeDrawing = true;
    std::size_t taking = snapshot->model.entities.size();
    if (words->scopeGiven) {
        auto match = katana::cad::matchScope(context.document, words->scope,
                                             context.interpreter.scopeContext());
        if (!match) {
            return match.error();
        }
        const std::string record = "scope " + katana::cad::scopeRecord(*match);
        taking = match->matched.size();
        if (match->matched.empty()) {
            // Taking nothing is an answer, not a failure: said, nothing written.
            Prepared prepared;
            prepared.title = "EXPORT " + pathText(file.filename());
            prepared.reply = record + "\nexport file=" + value(pathText(file)) + " kind=" +
                             kindWord(kind) + " ran=no reason=" + value("the scope took nothing");
            return prepared;
        }
        extra.insert(extra.begin(), record);
        wholeDrawing = match->resolved.words.source == katana::cad::ScopeSource::Drawing &&
                       match->resolved.filter.empty();
        if (kind == Kind::Vector) {
            vector.entities = match->matched;
        } else if (!wholeDrawing) {
            // The DXF and archive writers take a model: the copy keeps what
            // the scope took and nothing else.
            const std::set<katana::entity::EntityId> kept(match->matched.begin(),
                                                          match->matched.end());
            for (const katana::entity::EntityId id : snapshot->model.entities.ids()) {
                if (!kept.contains(id)) {
                    (void)snapshot->model.entities.remove(id);
                }
            }
        }
    }
    // An archive carries the session's surfaces with the whole drawing; a
    // part of it is only that part, as the window's export always did.
    if (kind == Kind::Archive && wholeDrawing) {
        snapshot->surfaces = context.surfaces.all();
    }
    snapshot->vector = std::move(vector);
    snapshot->extra = std::move(extra);

    Prepared prepared;
    prepared.title = "EXPORT " + pathText(file.filename());
    if (preview) {
        // Nothing written: what would be, and what the scope took.
        std::string reply = "export file=" + value(pathText(file)) + " kind=" + kindWord(kind) +
                            (driver.empty() ? std::string() : " driver=" + value(driver)) +
                            " preview=yes entities=" + std::to_string(taking);
        for (const std::string& record : snapshot->extra) {
            reply += "\n" + record;
        }
        prepared.reply = reply;
        return prepared;
    }
    prepared.work = [kind, file, snapshot](const std::stop_token& stop,
                                           const Progress&) -> Result<Apply> {
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        auto staged = StagedFiles::beside(file);
        if (!staged) {
            return staged.error();
        }
        // append adds to a copy of the file, which the apply puts in place
        // of it: a cancelled append leaves the file as it was.
        std::error_code missing;
        if (kind == Kind::Vector && snapshot->vector.append &&
            std::filesystem::exists(file, missing)) {
            std::error_code copied;
            std::filesystem::copy_file(file, (*staged)->writeTo(),
                                       std::filesystem::copy_options::overwrite_existing, copied);
            if (copied) {
                return makeError(ErrorCode::FileExportFailure,
                                 "could not copy the file to add to it: " + copied.message(),
                                 pathText(file));
            }
        }
        auto records = write(kind, *snapshot, (*staged)->writeTo(), file);
        if (!records) {
            return records.error();
        }
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        const std::shared_ptr<StagedFiles> files = *staged;
        const std::string reply = *records;
        return Apply([files, reply](Context&) -> Result<std::string> {
            if (auto placed = files->place(); !placed) {
                return placed.error();
            }
            return reply;
        });
    };
    return prepared;
}

} // namespace katana::app::geo
