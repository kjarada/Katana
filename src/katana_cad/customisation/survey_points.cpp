// The drawing's survey points (include/katana/cad/survey_points.hpp).
//
// In customisation/ only because that directory is globbed into katana_cad and
// this file was written while other work edited the library's source list; it
// belongs with survey_import.cpp and survey_tools.cpp in every other respect.

#include "katana/cad/survey_points.hpp"

#include <algorithm>
#include <format>
#include <map>
#include <set>
#include <utility>

#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geodesy/coordinate.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"
#include "katana/geodesy/coordinate_transformer.hpp"

namespace katana::cad {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
namespace geodesy = katana::geodesy;
namespace survey = katana::survey;

// How many ids an error or a warning names before it says "and N more": enough
// to recognise the file, few enough that a whole drawing's worth does not
// become one line of thousands in the log (the area report's limit).
constexpr std::size_t kNamedIds = 12;

std::string textOf(const katana::entity::PropertyMap& map, std::string_view key)
{
    if (key.empty()) {
        return {};
    }
    const auto found = map.find(std::string(key));
    return found == map.end() ? std::string{} : katana::entity::toString(found->second);
}

// A survey point, or nullopt for anything that is not one (see the header).
std::optional<DrawingSurveyPoint> surveyPointOf(const Entity& entity,
                                                const SurveyImportOptions& options)
{
    const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry);
    if (point == nullptr || options.pointNumberProperty.empty() ||
        !entity.properties.contains(options.pointNumberProperty)) {
        return std::nullopt;
    }
    DrawingSurveyPoint out;
    out.entity = entity.id;
    out.id = textOf(entity.properties, options.pointNumberProperty);
    out.code = textOf(entity.properties, options.codeProperty);
    out.description = textOf(entity.properties, options.descriptionProperty);
    out.easting = point->position.x;
    out.northing = point->position.y;
    // The one reader of heights: a point with no height property has none.
    out.elevation = katana::entity::heightsOf(entity.properties, 1).front();
    out.layer = entity.layer;
    out.sourceFile = textOf(entity.metadata, kSourceFileMeta);
    return out;
}

std::string namedIds(const std::vector<std::string>& ids)
{
    std::string text;
    for (std::size_t i = 0; i < ids.size() && i < kNamedIds; ++i) {
        text += (i == 0 ? "" : ", ") + ids[i];
    }
    if (ids.size() > kNamedIds) {
        text += " and " + std::to_string(ids.size() - kNamedIds) + " more";
    }
    return text;
}

// Fixed notation without the locale (std::format is locale-independent unless
// asked with 'L'), and no "-0.000" for a value that rounds to zero.
std::string fixed(double value, int decimals)
{
    std::string text = std::format("{:.{}f}", value, decimals);
    if (text.front() == '-' && text.find_first_of("123456789") == std::string::npos) {
        text.erase(0, 1);
    }
    return text;
}

int clampedDecimals(int decimals) { return std::clamp(decimals, 0, 12); }

std::string csvField(const std::string& field)
{
    const bool quote = field.find_first_of(",\"\r\n") != std::string::npos ||
                       (!field.empty() && (field.front() == ' ' || field.back() == ' ' ||
                                           field.front() == '\t' || field.back() == '\t'));
    if (!quote) {
        return field;
    }
    std::string out = "\"";
    for (const char c : field) {
        out += c == '"' ? std::string("\"\"") : std::string(1, c);
    }
    return out + "\"";
}

std::string sentence(std::string text)
{
    if (!text.empty() && text.back() != '.') {
        text += '.';
    }
    if (!text.empty() && text.front() >= 'a' && text.front() <= 'z') {
        text.front() = static_cast<char>(text.front() - 'a' + 'A');
    }
    return text;
}

} // namespace

std::vector<DrawingSurveyPoint> drawingSurveyPoints(const Document& document,
                                                    const SurveyImportOptions& options)
{
    std::vector<DrawingSurveyPoint> points;
    document.model().entities.forEach([&](const Entity& entity) {
        if (auto point = surveyPointOf(entity, options)) {
            points.push_back(std::move(*point));
        }
    });
    // forEach promises no order; ids are handed out rising, so this is the
    // order the points were added in.
    std::ranges::sort(points, {}, &DrawingSurveyPoint::entity);
    return points;
}

SurveyPointPick surveyPointsAmong(const Document& document, std::span<const EntityId> ids,
                                  const SurveyImportOptions& options)
{
    SurveyPointPick pick;
    for (const EntityId id : ids) {
        const Entity* entity = document.model().entities.find(id);
        auto point = entity == nullptr ? std::nullopt : surveyPointOf(*entity, options);
        if (!point) {
            ++pick.notSurveyPoints;
            continue;
        }
        pick.points.push_back(std::move(*point));
    }
    return pick;
}

survey::SurveyPoint toSurveyPoint(const DrawingSurveyPoint& point)
{
    survey::SurveyPoint out;
    out.id = point.id;
    // The drawing is (x, y) = (easting, northing); the model is north first.
    out.northing = point.northing;
    out.easting = point.easting;
    out.elevation = point.elevation;
    out.code = point.code;
    out.description = point.description;
    return out;
}

const char* toString(ExistingPointPolicy policy)
{
    switch (policy) {
    case ExistingPointPolicy::Refuse:
        return "refuse the import";
    case ExistingPointPolicy::Skip:
        return "keep the drawing's point";
    case ExistingPointPolicy::Replace:
        return "replace the drawing's point";
    case ExistingPointPolicy::KeepBoth:
        return "keep both";
    }
    return "refuse the import";
}

Result<katana::commands::CommandPtr> importSurveyPoints(const Document& document,
                                                        const survey::SurveyProject& project,
                                                        const SurveyImportOptions& options,
                                                        ExistingPointPolicy policy,
                                                        SurveyPointImportReport* report)
{
    SurveyPointImportReport discarded;
    SurveyPointImportReport& out = report != nullptr ? *report : discarded;
    out = {};

    // Every drawing entity of each id: under Replace all of them go, since a
    // drawing that already had two of an id would otherwise keep one.
    std::map<std::string, std::vector<EntityId>, std::less<>> inDrawing;
    for (const DrawingSurveyPoint& point : drawingSurveyPoints(document, options)) {
        inDrawing[point.id].push_back(point.entity);
    }
    std::set<std::string, std::less<>> clashing;
    for (const survey::SurveyPoint& point : project.points) {
        if (inDrawing.contains(point.id) && clashing.insert(point.id).second) {
            out.existingIds.push_back(point.id);
        }
    }

    const survey::SurveyProject* toImport = &project;
    survey::SurveyProject kept;
    std::vector<EntityId> replaced;
    if (!out.existingIds.empty()) {
        const std::string count = std::to_string(out.existingIds.size());
        switch (policy) {
        case ExistingPointPolicy::Refuse:
            return makeError(ErrorCode::AlreadyExists,
                             count + " point id(s) of the file are already in the drawing; "
                                     "choose to skip, replace or keep both",
                             namedIds(out.existingIds));
        case ExistingPointPolicy::Skip:
            kept = project;
            std::erase_if(kept.points, [&](const survey::SurveyPoint& point) {
                return clashing.contains(point.id);
            });
            out.skipped = project.points.size() - kept.points.size();
            toImport = &kept;
            out.warnings.push_back(sentence(
                std::to_string(out.skipped) + " point(s) of the file are not imported because "
                "the drawing already has their ids; the drawing's points are kept: " +
                namedIds(out.existingIds)));
            break;
        case ExistingPointPolicy::Replace:
            for (const std::string& id : out.existingIds) {
                const auto& entities = inDrawing.find(id)->second;
                replaced.insert(replaced.end(), entities.begin(), entities.end());
            }
            out.replaced = replaced.size();
            out.warnings.push_back(sentence(
                std::to_string(out.replaced) + " point(s) in the drawing are replaced by the "
                "file's points of the same id: " + namedIds(out.existingIds)));
            break;
        case ExistingPointPolicy::KeepBoth:
            out.warnings.push_back(sentence(
                count + " point id(s) will be in the drawing twice, the drawing's point and "
                "the file's: " + namedIds(out.existingIds)));
            break;
        }
    }

    auto command = importSurveyProject(document, *toImport, options, &out.import);
    if (!command) {
        return command.error();
    }
    for (const std::string& warning : out.import.warnings) {
        out.warnings.push_back(sentence(warning));
    }
    if (*command == nullptr) {
        return katana::commands::CommandPtr{};
    }
    if (replaced.empty()) {
        return command;
    }
    // One command, so one undo puts the replaced points back with the new ones
    // gone. The deletion goes first: Transaction validates its first step up
    // front and the rest as they run.
    auto transaction = std::make_unique<katana::commands::Transaction>("IMPORT_SURVEY_POINTS");
    transaction->add(katana::commands::deleteEntities(std::move(replaced)));
    transaction->add(std::move(*command));
    return katana::commands::CommandPtr{std::move(transaction)};
}

Result<survey::DeclaredCoordinateSystem> declaredSystemFromEpsg(int code)
{
    if (code <= 0) {
        return makeError(ErrorCode::InvalidCRS, "an EPSG code is a positive whole number",
                         std::to_string(code));
    }
    auto crs = geodesy::CoordinateReferenceSystem::fromEpsg(code);
    if (!crs) {
        return crs.error();
    }
    return survey::DeclaredCoordinateSystem::epsg(code, crs->name());
}

Result<survey::SurveyProject> transformSurveyProject(survey::SurveyProject project,
                                                     int sourceEpsg, int targetEpsg)
{
    auto source = geodesy::CoordinateReferenceSystem::fromEpsg(sourceEpsg);
    if (!source) {
        return source.error();
    }
    auto target = geodesy::CoordinateReferenceSystem::fromEpsg(targetEpsg);
    if (!target) {
        return target.error();
    }
    for (const auto* crs : {&*source, &*target}) {
        if (!crs->isProjected()) {
            return makeError(ErrorCode::Unsupported,
                             "survey points are grid coordinates: both systems must be "
                             "projected, and this one is " +
                                 std::string(geodesy::toString(crs->kind())),
                             "EPSG:" + crs->code() + " " + crs->name());
        }
    }
    auto transformer = geodesy::CoordinateTransformer::create(*source, *target);
    if (!transformer) {
        return transformer.error();
    }
    // The model is metres; each system's coordinates are in its own unit
    // (geodesy/coordinate.hpp: not silently converted), so a US survey foot
    // grid gets feet in and gives feet out.
    const double sourceUnit = source->horizontalUnitToSi();
    const double targetUnit = target->horizontalUnitToSi();
    for (survey::SurveyPoint& point : project.points) {
        // Traditional GIS order for a projected system: x easting, y northing.
        geodesy::Coordinate in;
        in.x = point.easting / sourceUnit;
        in.y = point.northing / sourceUnit;
        const auto moved = transformer->forward(in);
        if (!moved) {
            return makeError(moved.error().code,
                             "point " + point.id + " cannot be transformed: " +
                                 moved.error().message,
                             moved.error().context);
        }
        point.easting = moved->x * targetUnit;
        point.northing = moved->y * targetUnit;
    }
    project.coordinateSystem = survey::DeclaredCoordinateSystem::epsg(targetEpsg, target->name());
    project.metadata.insert_or_assign("transformed from",
                                      "EPSG:" + std::to_string(sourceEpsg) + " " + source->name());
    project.metadata.insert_or_assign("transformation", transformer->operation().name);
    return project;
}

std::string formatPointReport(std::span<const DrawingSurveyPoint> points, int decimals)
{
    decimals = clampedDecimals(decimals);
    std::string text = std::format("{:<12} {:>16} {:>16} {:>12}  {:<10} {:<24} {}\n", "Point",
                                   "Easting", "Northing", "Elevation", "Code", "Description",
                                   "Source file");
    std::size_t withoutElevation = 0;
    for (const DrawingSurveyPoint& point : points) {
        if (!point.elevation) {
            ++withoutElevation;
        }
        text += std::format("{:<12} {:>16} {:>16} {:>12}  {:<10} {:<24} {}\n", point.id,
                            fixed(point.easting, decimals), fixed(point.northing, decimals),
                            point.elevation ? fixed(*point.elevation, decimals) : std::string(),
                            point.code, point.description, point.sourceFile);
    }
    text += std::to_string(points.size()) + " point(s)";
    if (withoutElevation != 0) {
        text += ", " + std::to_string(withoutElevation) + " without an elevation";
    }
    return text + "\n";
}

std::string pointReportCsv(std::span<const DrawingSurveyPoint> points, int decimals)
{
    decimals = clampedDecimals(decimals);
    std::string text = "Point,Easting,Northing,Elevation,Code,Description,Source file\r\n";
    for (const DrawingSurveyPoint& point : points) {
        text += csvField(point.id) + "," + fixed(point.easting, decimals) + "," +
                fixed(point.northing, decimals) + "," +
                (point.elevation ? fixed(*point.elevation, decimals) : std::string()) + "," +
                csvField(point.code) + "," + csvField(point.description) + "," +
                csvField(point.sourceFile) + "\r\n";
    }
    return text;
}

std::string formatSurveyImportReport(const SurveyImportSummary& summary,
                                     const SurveyPointImportReport& report,
                                     const katana::core::Error* blocking)
{
    std::string text = "File: " + summary.fileName + "\n";
    text += "Format: " + summary.format + "\n";
    if (!summary.layout.empty()) {
        text += "Layout: " + summary.layout + "\n";
    }
    text += "Records read: " + std::to_string(summary.recordsRead) + "\n";
    text += "Units: " + summary.units + "\n";
    text += "Coordinate system: " + summary.coordinateSystem + "\n";
    if (!summary.transformation.empty()) {
        text += "Transformation: " + summary.transformation + "\n";
    }
    text += std::string("Points already in the drawing: ") + toString(summary.policy) + "\n";
    const std::size_t warnings = summary.readerWarnings.size() + report.warnings.size();
    if (warnings != 0) {
        text += "Warnings (" + std::to_string(warnings) + "):\n";
        for (const std::string& warning : summary.readerWarnings) {
            text += "  - " + sentence(warning) + "\n";
        }
        for (const std::string& warning : report.warnings) {
            text += "  - " + warning + "\n";
        }
    }
    if (blocking != nullptr) {
        return text + "Import blocked: " + sentence(blocking->describe()) + "\n";
    }
    text += "Points to create: " + std::to_string(report.import.points) + "\n";
    if (!report.import.layersCreated.empty()) {
        text += "Layers to create: ";
        for (std::size_t i = 0; i < report.import.layersCreated.size(); ++i) {
            text += (i == 0 ? "" : ", ") + report.import.layersCreated[i];
        }
        text += "\n";
    }
    return text;
}

} // namespace katana::cad
