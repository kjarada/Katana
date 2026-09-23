#include "katana/cad/survey_import.hpp"

#include <cstdint>
#include <map>
#include <set>
#include <string>
#include <string_view>
#include <utility>

#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/layer_path.hpp"

namespace katana::cad {

namespace {

using katana::commands::CommandPtr;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::Entity;
using katana::entity::PropertyValue;
using katana::survey::SurveyPoint;

// A field code is user data; a layer name is a PATH. So a code carrying a
// separator would silently build a subtree - "KERB/TOP" would put its points on
// a layer "TOP" underneath "KERB", which nobody asked for and which is invisible
// until someone wonders where their points went. Every character a layer path
// gives meaning to is replaced with '_'.
//
// A code made of nothing but those characters becomes "unnamed" rather than an
// empty segment, because an empty segment is not a valid layer path at all.
[[nodiscard]] std::string sanitizedCode(std::string_view code)
{
    std::string out;
    out.reserve(code.size());
    for (const char c : code) {
        out.push_back(c == katana::entity::kLayerSeparator ? '_' : c);
    }
    // Leading and trailing spaces would make two codes that read the same
    // produce two layers.
    const auto first = out.find_first_not_of(" \t");
    const auto last = out.find_last_not_of(" \t");
    out = first == std::string::npos ? std::string{} : out.substr(first, last - first + 1);
    return out.empty() ? std::string("unnamed") : out;
}

} // namespace

std::string layerForPoint(const SurveyPoint& point, const SurveyImportOptions& options)
{
    if (!options.layerPerCode || point.code.empty()) {
        return options.layer;
    }
    return katana::entity::joinLayerPath(options.layer, sanitizedCode(point.code));
}

katana::core::Result<CommandPtr> importSurveyProject(const Document& document,
                                                     const katana::survey::SurveyProject& project,
                                                     const SurveyImportOptions& options,
                                                     SurveyImportReport* report)
{
    // Asked of the survey layer rather than re-checked here, so that a parser
    // and this bridge cannot disagree about what a valid project is. It also
    // guarantees what follows: ids are non-empty and unique and every
    // coordinate is finite, so there is no point below that has to be skipped.
    if (auto status = katana::survey::validateProject(project); !status) {
        return status.error();
    }
    if (auto status = katana::entity::validateLayerPath(options.layer); !status) {
        return status.error();
    }

    SurveyImportReport discarded;
    SurveyImportReport& out = report != nullptr ? *report : discarded;
    out = {};

    // Said before the empty check, because a raw observation file whose every
    // target is unreduced imports NOTHING, and "nothing" needs its reason.
    out.pointsWithoutPosition = project.unpositionedPoints.size();
    if (out.pointsWithoutPosition != 0) {
        out.warnings.push_back(std::to_string(out.pointsWithoutPosition) +
                               " point(s) are named in the source with no coordinates - the "
                               "targets of raw observations - and are not drawn; reducing the "
                               "observations is what gives them a position");
    }

    // Nothing to do, and NOT an error: a caller has to be able to tell those
    // apart, the same contract applySurveyCodes() follows.
    if (project.points.empty()) {
        return CommandPtr{};
    }

    const katana::entity::Model& model = document.model();
    std::set<std::string> layersNeeded; // ordered: the layers are created in name order
    // Counted per key rather than warned per point: a metadata key that clashes
    // clashes on every one of ten thousand points, and ten thousand identical
    // warnings is the same as none.
    std::map<std::string, std::size_t> metadataClashes;
    std::size_t withoutElevation = 0;

    std::vector<Entity> entities;
    entities.reserve(project.points.size());

    for (const SurveyPoint& point : project.points) {
        const std::string layer = layerForPoint(point, options);
        if (auto status = katana::entity::validateLayerPath(layer); !status) {
            return makeError(ErrorCode::InvalidArgument,
                             "the field code of a point makes an invalid layer name; import "
                             "without a layer per code, or correct the code",
                             "point=" + point.id + " code=" + point.code +
                                 " layer=" + layer + " " + status.error().context);
        }
        if (model.layers.find(layer) == nullptr) {
            if (!options.createLayers) {
                return makeError(ErrorCode::NotFound,
                                 "the drawing has no layer for these points and this import "
                                 "was told not to create one",
                                 "point=" + point.id + " layer=" + layer);
            }
            layersNeeded.insert(layer);
        }

        Entity entity;
        // THE transposition point of the whole module, and the reason this line
        // has a comment on it. The survey model is (northing, easting) - north
        // first, as a field book and LandXML write it - and a CAD Point2 is
        // (x, y), which is EASTING first. Getting this backwards produces a
        // drawing that is internally consistent, plots beautifully, and is in
        // the wrong place. ImportedPointsAreNotTransposed asserts it with
        // coordinates whose magnitudes cannot be confused.
        entity.geometry = katana::entity::PointGeometry{
            katana::geometry::Point2(point.easting, point.northing)};
        entity.layer = layer;

        katana::entity::PropertyMap& properties = entity.properties;
        if (!options.pointNumberProperty.empty()) {
            properties.insert_or_assign(options.pointNumberProperty, PropertyValue(point.id));
        }
        if (!options.codeProperty.empty() && !point.code.empty()) {
            properties.insert_or_assign(options.codeProperty, PropertyValue(point.code));
        }
        if (!options.descriptionProperty.empty() && !point.description.empty()) {
            properties.insert_or_assign(options.descriptionProperty,
                                        PropertyValue(point.description));
        }
        // A 2D drawing has no Z, so the height is a property - the same one the
        // 12d import writes and the surface builder reads. A point the source
        // gave no height for gets NO property rather than 0.0: the surface
        // builder leaves a point without one out, where a zero would be a real
        // height at the datum and pull the surface down to it (PLAN.MD 45.3b).
        if (point.elevation) {
            properties.insert_or_assign(std::string(katana::entity::kElevationProperty),
                                        PropertyValue(*point.elevation));
        } else {
            ++withoutElevation;
        }
        if (point.coordinateSource != katana::survey::CoordinateSource::Unknown) {
            properties.insert_or_assign(
                std::string(kCoordinateSourceProperty),
                PropertyValue(std::string(toString(point.coordinateSource))));
        }
        // Whatever else the file carried. The brief is explicit that metadata is
        // preserved rather than thrown away on import - but never on top of a
        // field this import has just written, because that would let a column
        // in a CSV overwrite the code the same row's code column set.
        for (const auto& [key, value] : point.metadata) {
            if (properties.contains(key)) {
                ++metadataClashes[key];
                continue;
            }
            properties.insert_or_assign(key, PropertyValue(value));
        }

        if (options.recordSource && point.source.known()) {
            katana::entity::PropertyMap& metadata = entity.metadata;
            const auto& source = point.source;
            if (!source.manufacturer.empty()) {
                metadata.insert_or_assign(std::string(kSourceManufacturerMeta),
                                          PropertyValue(source.manufacturer));
            }
            if (!source.format.empty()) {
                metadata.insert_or_assign(std::string(kSourceFormatMeta),
                                          PropertyValue(source.format));
            }
            if (!source.formatVersion.empty()) {
                metadata.insert_or_assign(std::string(kSourceFormatVersionMeta),
                                          PropertyValue(source.formatVersion));
            }
            if (!source.fileName.empty()) {
                metadata.insert_or_assign(std::string(kSourceFileMeta),
                                          PropertyValue(source.fileName));
            }
            if (source.recordNumber != 0) {
                metadata.insert_or_assign(
                    std::string(kSourceRecordMeta),
                    PropertyValue(static_cast<std::int64_t>(source.recordNumber)));
            }
        }

        entities.push_back(std::move(entity));
    }

    out.points = entities.size();
    out.pointsWithoutElevation = withoutElevation;
    if (withoutElevation != 0) {
        out.warnings.push_back(std::to_string(withoutElevation) +
                               " point(s) have no height in the source; they are drawn "
                               "without an elevation and a surface will leave them out");
    }
    for (const auto& [key, count] : metadataClashes) {
        out.warnings.push_back("metadata field \"" + key + "\" was not imported on " +
                               std::to_string(count) +
                               " point(s): this import already writes that property");
    }

    // One command, so one undo. The layers come first and the points second, and
    // Transaction validates only its first step up front precisely because a
    // later step may depend on an earlier one's effect - here, the points need
    // the layers to exist.
    auto transaction =
        std::make_unique<katana::commands::Transaction>("IMPORT_SURVEY_POINTS");
    for (const std::string& name : layersNeeded) {
        katana::entity::Layer layer;
        layer.name = name;
        transaction->add(katana::commands::createLayer(std::move(layer)));
        out.layersCreated.push_back(name);
    }
    transaction->add(katana::commands::createEntities(std::move(entities)));
    return CommandPtr{std::move(transaction)};
}

} // namespace katana::cad
