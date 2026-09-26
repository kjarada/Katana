// EXPORT (docs/interop.md, "IMPORT, EXPORT, INFO, REFS and COPC on every front
// end"): the drawing to a file by its extension - a DXF natively, a 12d
// archive with the session's surfaces, any other through GDAL - run as the one
// executor runs every verb.
//
//   EXPORT <file>
//
// The drawing is copied when the line is prepared, so the work writes on a
// worker while the window stays live; the file is written beside its place
// and moved there by the apply (staged_files.hpp), so a cancelled EXPORT
// writes nothing.

#include <filesystem>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "../dxf_verbs.hpp"
#include "../import_records.hpp"
#include "katana/core/text.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "staged_files.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage = "usage: EXPORT <file>";

enum class Kind { Dxf, Archive, Vector };

// What the work writes from: copies, never the Document.
struct Snapshot {
    katana::entity::Model model;
    double annotationScale = 1000.0;
    std::vector<katana::terrain::NamedSurface> surfaces;
};

Result<std::string> write(Kind kind, const Snapshot& snapshot, const std::filesystem::path& to,
                          const std::filesystem::path& named)
{
    switch (kind) {
    case Kind::Dxf: {
        auto written = writeDxfExport(snapshot.model, snapshot.annotationScale, to);
        if (!written) {
            return written.error();
        }
        return dxfExportRecords(*written, named);
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
        std::string reply = "exported file=" + recordText(pathText(named)) +
                            " kind=archive driver=12da entities=" +
                            std::to_string(written->entitiesWritten) +
                            " skipped=" + std::to_string(written->entitiesSkipped) +
                            " alignments=" + std::to_string(written->alignmentsWritten) +
                            " surfaces=" + std::to_string(written->surfacesWritten) +
                            " bytes=" + std::to_string(written->bytesWritten);
        for (const std::string& warning : written->warnings) {
            reply += "\n" + warningText(warning);
        }
        return reply;
    }
    case Kind::Vector: {
        auto written = interop::exportVector(snapshot.model, to);
        if (!written) {
            return written.error();
        }
        std::string reply = "exported file=" + recordText(pathText(named)) +
                            " kind=vector driver=" + recordText(written->driver) +
                            " features=" + std::to_string(written->featuresWritten) +
                            " skipped=" + std::to_string(written->entitiesSkipped);
        for (const std::string& warning : written->warnings) {
            reply += "\n" + warningText(warning);
        }
        return reply;
    }
    }
    return makeError(ErrorCode::Unsupported, "no exporter", pathText(named));
}

} // namespace

Result<Prepared> prepareExport(Context& context, const Tokens& tokens, std::string_view line)
{
    (void)tokens;
    const std::string path = restOfLine(line);
    if (path.empty()) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    const std::filesystem::path file = pathFromText(path);
    const Kind kind = katana::dxf::isDxfPath(file) ? Kind::Dxf
                      : interop::kindForPath(file) == interop::SourceKind::Archive12d
                          ? Kind::Archive
                          : Kind::Vector;

    // The drawing as it is now, copied for the worker; the copy's entity
    // observer is the Document's, and a copy must never report to it.
    auto snapshot = std::make_shared<Snapshot>();
    snapshot->model = context.document.model();
    snapshot->model.entities.setObserver({});
    snapshot->annotationScale = context.document.annotationScale();
    if (kind == Kind::Archive) {
        snapshot->surfaces = context.surfaces.all();
    }

    Prepared prepared;
    prepared.title = "EXPORT " + pathText(file.filename());
    prepared.work = [kind, file, snapshot](const std::stop_token& stop,
                                           const Progress&) -> Result<Apply> {
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        auto staged = StagedFiles::beside(file);
        if (!staged) {
            return staged.error();
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
