// Contours into the drawing (contour_entities.hpp).

#include "katana/interop/geo/contour_entities.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <variant>
#include <vector>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/intersection.hpp"

namespace katana::interop::geo {

namespace {

namespace gp = katana::gis::processing;
namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::terrain::Contour;

std::optional<double> numberOf(const gp::FieldValue& value)
{
    if (const auto* real = std::get_if<double>(&value)) {
        return *real;
    }
    if (const auto* whole = std::get_if<std::int64_t>(&value)) {
        return static_cast<double>(*whole);
    }
    return std::nullopt;
}

// The ring or string of plan points a part is: a ring GDAL closed by
// repeating its first vertex is closed, and the repeat dropped (Polyline2
// closes a ring itself).
std::optional<Polyline2> lineOf(const std::vector<katana::gis::GeoPoint>& points)
{
    Polyline2 line;
    line.vertices.reserve(points.size());
    for (const katana::gis::GeoPoint& point : points) {
        line.vertices.emplace_back(point.x, point.y);
    }
    if (line.vertices.size() >= 4 && line.vertices.front() == line.vertices.back()) {
        line.vertices.pop_back();
        line.closed = true;
    }
    if (line.vertices.size() < 2) {
        return std::nullopt;
    }
    return line;
}

Box2 boxOf(const Polyline2& line)
{
    Box2 box;
    for (const Point2& vertex : line.vertices) {
        box.expand(vertex);
    }
    return box;
}

// A boundary's rings with their boxes, so a segment far from a ring costs one
// box test and not one per edge.
struct Area {
    std::vector<const Polyline2*> rings;
    std::vector<Box2> boxes;
    Box2 box;
};

std::vector<Area> areasOf(const std::vector<ContourBoundary>& boundaries)
{
    std::vector<Area> areas;
    for (const ContourBoundary& boundary : boundaries) {
        Area area;
        for (const Polyline2& ring : boundary.rings) {
            if (ring.vertices.size() < 3) {
                continue;
            }
            area.rings.push_back(&ring);
            area.boxes.push_back(boxOf(ring));
            area.box.expand(area.boxes.back());
        }
        if (!area.rings.empty()) {
            areas.push_back(std::move(area));
        }
    }
    return areas;
}

// Inside the exterior and in no hole - by the parity of the rings around the
// point, so a hole needs no flag - or on any ring.
bool inside(const std::vector<Area>& areas, const Point2& point)
{
    for (const Area& area : areas) {
        if (!area.box.contains(point)) {
            continue;
        }
        std::size_t around = 0;
        for (std::size_t r = 0; r < area.rings.size(); ++r) {
            if (!area.boxes[r].contains(point)) {
                continue;
            }
            const katana::geometry::Containment where = area.rings[r]->classify(point);
            if (where == katana::geometry::Containment::OnBoundary) {
                return true;
            }
            around += where == katana::geometry::Containment::Inside ? 1u : 0u;
        }
        if (around % 2 == 1) {
            return true;
        }
    }
    return false;
}

// Where along `segment` (0 to 1) it crosses or touches a ring's edge, ends
// excluded, ascending.
std::vector<double> cutsOf(const Segment2& segment, const std::vector<Area>& areas)
{
    const katana::geometry::Vec2 along = segment.delta();
    const double length2 = along.lengthSquared();
    std::vector<double> cuts;
    if (!(length2 > 0.0)) {
        return cuts;
    }
    Box2 box;
    box.expand(segment.start);
    box.expand(segment.end);
    for (const Area& area : areas) {
        if (!area.box.intersects(box)) {
            continue;
        }
        for (std::size_t r = 0; r < area.rings.size(); ++r) {
            if (!area.boxes[r].intersects(box)) {
                continue;
            }
            const Polyline2& ring = *area.rings[r];
            for (std::size_t e = 0; e < ring.segmentCount(); ++e) {
                const Segment2 edge = ring.segment(e);
                const auto meet = katana::geometry::intersect(segment, edge);
                for (std::size_t k = 0; k < meet.count; ++k) {
                    const double t = (meet.points[k] - segment.start).dot(along) / length2;
                    if (t > 0.0 && t < 1.0) {
                        cuts.push_back(t);
                    }
                }
            }
        }
    }
    std::ranges::sort(cuts);
    cuts.erase(std::unique(cuts.begin(), cuts.end()), cuts.end());
    return cuts;
}

// One contour cut into the pieces inside the areas.
void clipOne(const Contour& contour, const std::vector<Area>& areas, std::vector<Contour>& out)
{
    const Polyline2& line = contour.line;
    // Each sub-segment between cuts, in order, with whether it is kept.
    struct Piece {
        Point2 from, to;
        bool kept = false;
    };
    std::vector<Piece> pieces;
    for (std::size_t s = 0; s < line.segmentCount(); ++s) {
        const Segment2 segment = line.segment(s);
        std::vector<double> at = cutsOf(segment, areas);
        at.insert(at.begin(), 0.0);
        at.push_back(1.0);
        for (std::size_t i = 0; i + 1 < at.size(); ++i) {
            const Point2 from = i == 0 ? segment.start : segment.pointAt(at[i]);
            const Point2 to = i + 2 == at.size() ? segment.end : segment.pointAt(at[i + 1]);
            if (from == to) {
                continue;
            }
            pieces.push_back({from, to, inside(areas, (from + to) * 0.5)});
        }
    }
    if (pieces.empty()) {
        return;
    }
    if (std::ranges::all_of(pieces, [](const Piece& piece) { return piece.kept; })) {
        out.push_back(contour);
        return;
    }
    // Runs of kept pieces, each an open polyline.
    std::vector<std::vector<Point2>> runs;
    bool open = false;
    for (const Piece& piece : pieces) {
        if (!piece.kept) {
            open = false;
            continue;
        }
        if (!open) {
            runs.push_back({piece.from});
            open = true;
        }
        runs.back().push_back(piece.to);
    }
    // A ring cut somewhere: the run through its first vertex was cut in two,
    // one at each end of the list.
    if (line.closed && runs.size() > 1 && pieces.front().kept && pieces.back().kept) {
        std::vector<Point2> joined = std::move(runs.back());
        joined.insert(joined.end(), runs.front().begin() + 1, runs.front().end());
        runs.front() = std::move(joined);
        runs.pop_back();
    }
    for (std::vector<Point2>& run : runs) {
        Contour piece;
        piece.elevation = contour.elevation;
        piece.major = contour.major;
        piece.line.vertices = std::move(run);
        out.push_back(std::move(piece));
    }
}

} // namespace

Result<std::vector<Contour>> contoursFromFeatures(const gp::FeatureSet& set,
                                                  std::string_view elevationField,
                                                  double interval, double base,
                                                  std::size_t majorEvery)
{
    if (!(interval > 0.0) || !std::isfinite(interval) || !std::isfinite(base)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a contour interval is positive and finite, and its base finite");
    }
    std::vector<Contour> contours;
    for (const gp::FeatureTable& table : set.tables) {
        if (table.features.empty()) {
            continue;
        }
        const auto field = std::ranges::find_if(
            table.fields, [&](const gp::FieldDef& def) { return def.name == elevationField; });
        if (field == table.fields.end()) {
            return makeError(ErrorCode::InvalidArgument,
                             "GDAL's contours carry no level: the table has no field '" +
                                 std::string(elevationField) + "'",
                             table.name);
        }
        const auto index = static_cast<std::size_t>(field - table.fields.begin());
        for (const gp::Feature& feature : table.features) {
            const std::optional<double> level =
                index < feature.values.size() ? numberOf(feature.values[index]) : std::nullopt;
            if (!level) {
                return makeError(ErrorCode::InvalidArgument,
                                 "a contour GDAL wrote has no level in '" +
                                     std::string(elevationField) + "'",
                                 table.name);
            }
            // The tracer's rule (terrain::contours): the k of base + k *
            // interval, major when a multiple of majorEvery. Rounded, since
            // GDAL computed the level in floating point too.
            const auto k = static_cast<long long>(std::llround((*level - base) / interval));
            const bool major =
                majorEvery != 0 && k % static_cast<long long>(majorEvery) == 0;
            for (const katana::gis::VectorGeometry& part : feature.parts) {
                if (part.kind != katana::gis::GeometryKind::LineString || part.parts.empty()) {
                    continue;
                }
                if (auto line = lineOf(part.parts.front())) {
                    contours.push_back(Contour{*level, major, std::move(*line)});
                }
            }
        }
    }
    std::ranges::stable_sort(contours, {}, &Contour::elevation);
    return contours;
}

std::vector<ContourBoundary> boundariesOf(const gp::FeatureSet& set)
{
    std::vector<ContourBoundary> boundaries;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            for (const katana::gis::VectorGeometry& part : feature.parts) {
                if (part.kind != katana::gis::GeometryKind::Polygon) {
                    continue;
                }
                ContourBoundary boundary;
                for (const std::vector<katana::gis::GeoPoint>& ring : part.parts) {
                    auto line = lineOf(ring);
                    if (line && line->vertices.size() >= 3) {
                        line->closed = true;
                        boundary.rings.push_back(std::move(*line));
                    }
                }
                if (!boundary.rings.empty()) {
                    boundaries.push_back(std::move(boundary));
                }
            }
        }
    }
    return boundaries;
}

std::vector<Contour> clipContours(const std::vector<Contour>& contours,
                                  const std::vector<ContourBoundary>& boundaries)
{
    const std::vector<Area> areas = areasOf(boundaries);
    std::vector<Contour> kept;
    for (const Contour& contour : contours) {
        const Box2 box = boxOf(contour.line);
        const bool near = std::ranges::any_of(areas, [&](const Area& area) {
            return area.box.intersects(box);
        });
        if (near) {
            clipOne(contour, areas, kept);
        }
    }
    return kept;
}

Result<ContourPlan> contourCommand(const katana::entity::Model& model,
                                   const std::vector<Contour>& contours,
                                   const ContourEntityOptions& options)
{
    if (auto valid = katana::entity::validateLayerPath(options.layer); !valid) {
        return makeError(ErrorCode::InvalidArgument,
                         "the contours' layer is not a layer path: " + valid.error().message,
                         options.layer);
    }
    ContourPlan plan;
    if (contours.empty()) {
        return plan;
    }
    const std::string majorLayer = options.layer + "/major";
    const std::string minorLayer = options.layer + "/minor";
    auto transaction = std::make_unique<cmd::Transaction>(
        options.commandName.empty() ? std::string("CONTOUR") : options.commandName);
    const bool anyMajor = std::ranges::any_of(contours, &Contour::major);
    const bool anyMinor = !std::ranges::all_of(contours, &Contour::major);
    for (const auto& [wanted, layer] : {std::pair{anyMajor, majorLayer}, std::pair{anyMinor, minorLayer}}) {
        if (!wanted) {
            continue;
        }
        plan.layers.push_back(layer);
        if (!model.layers.contains(layer)) {
            katana::entity::Layer created;
            created.name = layer;
            transaction->add(cmd::createLayer(created));
        }
    }
    std::vector<katana::entity::Entity> entities;
    entities.reserve(contours.size());
    std::set<double> levels;
    for (const Contour& contour : contours) {
        katana::entity::Entity entity;
        entity.geometry = contour.line;
        entity.layer = contour.major ? majorLayer : minorLayer;
        katana::entity::setHeights(
            entity.properties,
            std::vector<std::optional<double>>(contour.line.vertices.size(), contour.elevation));
        entities.push_back(std::move(entity));
        levels.insert(contour.elevation);
        ++(contour.major ? plan.major : plan.minor);
    }
    plan.created = entities.size();
    plan.levels = levels.size();
    transaction->add(cmd::createEntities(std::move(entities)));
    plan.command = std::move(transaction);
    return plan;
}

} // namespace katana::interop::geo
