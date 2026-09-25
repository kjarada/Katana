#include "katana/ifc/export.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <fstream>

#include "builder.hpp"
#include "katana/core/text.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "parts.hpp"
#include "product_layout.hpp"

namespace katana::ifc {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// The local origin is rounded down to this in plan, so that it reads as a
// round number in the map conversion and moves by whole steps as a job
// grows rather than with every point added.
constexpr double kOriginStep = 100.0;

bool isEpsgCode(std::string_view name)
{
    if (name.size() < 6 || name.substr(0, 5) != "EPSG:") {
        return false;
    }
    return std::all_of(name.begin() + 5, name.end(),
                       [](char c) { return std::isdigit(static_cast<unsigned char>(c)) != 0; });
}

// The south-west corner of everything the export writes, in plan.
std::optional<math::Vec2> extentCorner(const ExportInput& input, const ExportOptions& options)
{
    std::optional<math::Vec2> corner;
    const auto include = [&](double x, double y) {
        if (!std::isfinite(x) || !std::isfinite(y)) {
            return;
        }
        corner = corner ? math::Vec2(std::min(corner->x, x), std::min(corner->y, y))
                        : math::Vec2(x, y);
    };
    if (input.model != nullptr) {
        if (options.exportEntities && !input.model->entities.empty()) {
            const auto bounds = input.model->entities.bounds();
            if (!bounds.empty()) {
                include(bounds.min.x, bounds.min.y);
            }
        }
        if (options.exportAlignments) {
            input.model->alignments.forEach([&](const entity::Alignment& alignment) {
                for (const auto& pi : alignment.horizontal.pis) {
                    include(pi.point.x, pi.point.y);
                }
            });
        }
    }
    if (input.utilities) {
        for (const auto& line : input.utilities->lines) {
            for (const auto& vertex : line.vertices) {
                include(vertex.position.easting, vertex.position.northing);
            }
        }
    }
    for (const SurfaceInput& surface : input.surfaces) {
        if (surface.surface != nullptr && surface.surface->vertexCount() > 0) {
            const auto& bounds = surface.surface->bounds();
            include(bounds.min.x, bounds.min.y);
        }
    }
    return corner;
}

std::string headerList(std::string_view value)
{
    return "(" + stepString(value) + ")";
}

} // namespace

bool isIfcPath(const std::filesystem::path& path)
{
    return core::equalsIgnoringCase(path.extension().string(), ".ifc");
}

Result<IfcExport> writeIfc(const ExportInput& input, const ExportOptions& options)
{
    const std::vector<ClassificationRule>& rules =
        options.rules.empty() ? defaultClassificationRules() : options.rules;
    for (const ClassificationRule& rule : rules) {
        if (!detail::findProductLayout(rule.target.entity)) {
            return makeError(ErrorCode::InvalidArgument,
                             "the classification rule \"" + rule.name + "\" names " +
                                 rule.target.entity + ", which is not a class this export writes",
                             rule.target.entity);
        }
        if (rule.target.predefinedType == "USERDEFINED" && rule.target.objectType.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "the classification rule \"" + rule.name +
                                 "\" is USERDEFINED and says nothing of what it is (objectType)",
                             rule.name);
        }
    }

    IfcExport report;
    detail::Frame frame;
    const Georeference& crs = options.georeference;
    frame.georeferenced = isEpsgCode(crs.name);
    if (!crs.name.empty() && !frame.georeferenced) {
        report.warnings.push_back("the coordinate system \"" + crs.name +
                                  "\" has no EPSG code, which IFC4X3_ADD2 names a system by; the "
                                  "file is not georeferenced");
    } else if (crs.name.empty()) {
        report.warnings.push_back(
            "the project has no coordinate system: the file is not georeferenced, and its "
            "coordinates are the project's own");
    }
    if (options.localOrigin) {
        frame.origin = *options.localOrigin;
        if (!frame.georeferenced && (frame.origin.x != 0.0 || frame.origin.y != 0.0 ||
                                     frame.origin.z != 0.0)) {
            // A shift that nothing in the file records would move the job.
            return makeError(ErrorCode::InvalidArgument,
                             "a local origin needs a coordinate system (an EPSG code) to be "
                             "recorded in");
        }
    } else if (frame.georeferenced) {
        if (const auto corner = extentCorner(input, options)) {
            frame.origin = math::Vec3(std::floor(corner->x / kOriginStep) * kOriginStep,
                                      std::floor(corner->y / kOriginStep) * kOriginStep, 0.0);
        }
    }

    detail::Builder builder(options, frame, report);
    if (input.model != nullptr && options.exportAlignments) {
        detail::exportAlignments(builder, *input.model);
    }
    if (input.utilities) {
        detail::exportUtilities(builder, *input.utilities);
    }
    if (input.model != nullptr && options.exportEntities) {
        detail::exportEntities(builder, *input.model, rules);
    }
    detail::exportSurfaces(builder, input.surfaces);
    builder.finish();

    if (builder.file().nonFinite()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a number that is not finite reached the IFC writer; nothing was written");
    }

    std::string text;
    text.reserve(builder.file().data().size() + 1024);
    text += "ISO-10303-21;\nHEADER;\n";
    // The Alignment-based view: IFC 4.3's view for alignments and what is
    // positioned along them, whose scope the entities written stay within
    // (ReferenceView leaves out the alignment business logic and the curves
    // IfcClothoid and IfcCircle).
    text += "FILE_DESCRIPTION(('ViewDefinition [Alignment-basedView]'),'2;1');\n";
    const std::string application =
        options.applicationVersion.empty() ? "Katana" : "Katana " + options.applicationVersion;
    text += "FILE_NAME(" + stepString(options.fileName) + "," + stepString(options.timestamp) + "," +
            headerList(options.author) + "," + headerList(options.organisation) + "," +
            stepString(application) + "," + stepString(application) + ",'');\n";
    text += "FILE_SCHEMA(('IFC4X3_ADD2'));\nENDSEC;\nDATA;\n";
    text += builder.file().data();
    text += "ENDSEC;\nEND-ISO-10303-21;\n";

    report.instances = builder.file().size();
    report.bytesWritten = text.size();
    report.text = std::move(text);
    return report;
}

Result<IfcExport> writeIfcFile(const ExportInput& input, const std::filesystem::path& path,
                               const ExportOptions& options)
{
    ExportOptions named = options;
    if (named.fileName.empty()) {
        named.fileName = path.filename().string();
    }
    auto written = writeIfc(input, named);
    if (!written) {
        return written;
    }
    // Beside the target first, then over it: a write that fails part way
    // leaves the previous file, not half of a new one.
    std::filesystem::path temporary = path;
    temporary += ".partial";
    {
        std::ofstream file(temporary, std::ios::binary | std::ios::trunc);
        file.write(written->text.data(), static_cast<std::streamsize>(written->text.size()));
        file.close();
        if (!file) {
            std::error_code ignored;
            std::filesystem::remove(temporary, ignored);
            return makeError(ErrorCode::FileExportFailure, "the file could not be written",
                             path.string());
        }
    }
    std::error_code error;
    std::filesystem::rename(temporary, path, error);
    if (error) {
        std::error_code ignored;
        std::filesystem::remove(temporary, ignored);
        return makeError(ErrorCode::FileExportFailure,
                         "the file could not be replaced: " + error.message(), path.string());
    }
    written->text.clear();
    written->text.shrink_to_fit();
    return written;
}

} // namespace katana::ifc
