// Drawing data to and from GDAL's feature tables (drawing_dataset.hpp,
// docs/geoprocessing.md "Bindings").

#include "katana/interop/geo/drawing_dataset.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <variant>
#include <vector>

#include "../curve_chords.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/math/numerics.hpp"

namespace katana::interop::geo {

namespace {

namespace gp = katana::gis::processing;
namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
using katana::gis::GeoPoint;
using katana::gis::GeometryKind;
using katana::gis::VectorGeometry;

constexpr std::array<const char*, 5> kBookkeeping{"katana_id", "layer", "style", "colour", "type"};

// ---- entities to features ----------------------------------------------------------------

// A property or a metadata value, as text: IMPORT writes source.ring into an
// entity's metadata, a result writes gis.ring into its properties.
std::optional<std::string> tagOf(const Entity& entity, const std::string& key)
{
    if (const auto found = entity.properties.find(key); found != entity.properties.end()) {
        return katana::entity::toString(found->second);
    }
    if (const auto found = entity.metadata.find(key); found != entity.metadata.end()) {
        return katana::entity::toString(found->second);
    }
    return std::nullopt;
}

bool taggedHole(const Entity& entity)
{
    const auto isHole = [](const std::optional<std::string>& tag) {
        return tag && katana::core::lowered(*tag) == "hole";
    };
    return isHole(tagOf(entity, "gis.ring")) || isHole(tagOf(entity, "source.ring"));
}

// One entity's geometry as a ring or a string of points, before it is put in
// a table.
struct Shape {
    const Entity* entity = nullptr;
    GeometryKind kind = GeometryKind::Unknown;
    std::vector<GeoPoint> points; // a polygon's exterior ring, not repeated at the end
    bool hasZ = false;
    std::vector<std::vector<GeoPoint>> holes;
    bool holesHaveZ = true;
    bool isHole = false;
};

std::vector<GeoPoint> planPoints(const std::vector<Point2>& points)
{
    std::vector<GeoPoint> out;
    out.reserve(points.size());
    for (const Point2& point : points) {
        out.push_back(GeoPoint{point.x, point.y, 0.0});
    }
    return out;
}

std::vector<Point2> arcPoints(const katana::geometry::Arc2& arc, double tolerance)
{
    const int chords = detail::chordCount(arc.radius, arc.sweep, tolerance);
    std::vector<Point2> points;
    points.reserve(static_cast<std::size_t>(chords) + 1);
    for (int i = 0; i <= chords; ++i) {
        points.push_back(arc.pointAt(static_cast<double>(i) / static_cast<double>(chords)));
    }
    return points;
}

std::vector<Point2> circlePoints(const katana::geometry::Circle2& circle, double tolerance)
{
    const int chords =
        std::max(3, detail::chordCount(circle.radius, katana::math::kTwoPi, tolerance));
    std::vector<Point2> points;
    points.reserve(static_cast<std::size_t>(chords));
    for (int i = 0; i < chords; ++i) {
        points.push_back(circle.pointAtAngle(katana::math::kTwoPi * static_cast<double>(i) /
                                             static_cast<double>(chords)));
    }
    return points;
}

// Heights onto the points when every one has one: a geometry is 3D only then.
bool applyHeights(std::vector<GeoPoint>& points, const std::vector<std::optional<double>>& heights)
{
    if (points.empty() || heights.size() != points.size() ||
        !std::ranges::all_of(heights, [](const auto& z) { return z.has_value(); })) {
        return false;
    }
    for (std::size_t i = 0; i < points.size(); ++i) {
        points[i].z = *heights[i];
    }
    return true;
}

// Entities that went in plan although they have heights, and why: said, since
// their heights then travel only as the elevation fields.
struct HeightNotes {
    std::size_t partial = 0;     // heights at only some vertices
    std::size_t slopingArcs = 0; // an arc whose two ends differ
};

// The shape of `entity`, or the reason it has none.
std::variant<Shape, std::string> shapeOf(const Entity& entity, const DrawingDatasetOptions& options,
                                         HeightNotes& notes)
{
    Shape shape;
    shape.entity = &entity;
    std::vector<std::optional<double>> heights;
    const auto& props = entity.properties;
    std::optional<std::string> skip;
    std::visit(
        [&](const auto& held) {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, katana::entity::PointGeometry>) {
                shape.kind = GeometryKind::Point;
                shape.points = planPoints({held.position});
                heights = katana::entity::heightsOf(props, 1);
            } else if constexpr (std::is_same_v<Held, katana::geometry::Segment2>) {
                shape.kind = GeometryKind::LineString;
                shape.points = planPoints({held.start, held.end});
                heights = katana::entity::heightsOf(props, 2);
            } else if constexpr (std::is_same_v<Held, katana::geometry::Arc2>) {
                shape.kind = GeometryKind::LineString;
                shape.points = planPoints(arcPoints(held, options.curveTolerance));
                // An arc carries a height at each end and none between: level,
                // every chord point has it; sloping, the heights between would
                // be invented, so it goes in plan - EXPORT's rule.
                const auto ends = katana::entity::heightsOf(props, 2);
                if (ends[0] && ends[1] && *ends[0] == *ends[1]) {
                    heights.assign(shape.points.size(), ends[0]);
                } else if (ends[0] || ends[1]) {
                    ++notes.slopingArcs;
                }
            } else if constexpr (std::is_same_v<Held, katana::geometry::Polyline2>) {
                shape.points = planPoints(held.vertices);
                heights = katana::entity::heightsOf(props, held.vertices.size());
                if (held.closed && held.vertices.size() >= 3 && options.closedAsPolygons) {
                    shape.kind = GeometryKind::Polygon;
                } else {
                    shape.kind = GeometryKind::LineString;
                    if (held.closed && !shape.points.empty()) {
                        shape.points.push_back(shape.points.front());
                        if (!heights.empty()) {
                            heights.push_back(heights.front());
                        }
                    }
                }
            } else if constexpr (std::is_same_v<Held, katana::geometry::Circle2>) {
                shape.points = planPoints(circlePoints(held, options.curveTolerance));
                heights.assign(shape.points.size(), katana::entity::heightsOf(props, 1)[0]);
                if (options.closedAsPolygons) {
                    shape.kind = GeometryKind::Polygon;
                } else {
                    shape.kind = GeometryKind::LineString;
                    shape.points.push_back(shape.points.front());
                    heights.push_back(heights.front());
                }
            } else if constexpr (std::is_same_v<Held, katana::entity::TextGeometry>) {
                skip = "text";
            } else if constexpr (std::is_same_v<Held, katana::entity::DimensionGeometry>) {
                skip = "dimension";
            } else if constexpr (std::is_same_v<Held, katana::entity::LabelGeometry>) {
                skip = "label";
            } else if constexpr (std::is_same_v<Held, katana::entity::LeaderGeometry>) {
                skip = "leader";
            } else {
                // Whoever adds a geometry kind decides what a feature of it is.
                static_assert(false, "drawingDataset has no case for this geometry kind");
            }
        },
        entity.geometry);
    if (skip) {
        return *skip;
    }
    const std::size_t least = shape.kind == GeometryKind::Point        ? 1
                              : shape.kind == GeometryKind::LineString ? 2
                                                                       : 3;
    if (shape.points.size() < least) {
        return std::string("degenerate");
    }
    shape.hasZ = applyHeights(shape.points, heights);
    if (options.requireHeights && !shape.hasZ) {
        return std::string("heightless");
    }
    if (!shape.hasZ && std::ranges::any_of(heights, [](const auto& z) { return z.has_value(); })) {
        ++notes.partial;
    }
    shape.isHole = shape.kind == GeometryKind::Polygon && taggedHole(entity);
    return shape;
}

katana::geometry::Polyline2 ringOf(const std::vector<GeoPoint>& points)
{
    katana::geometry::Polyline2 ring;
    ring.closed = true;
    for (const GeoPoint& point : points) {
        ring.vertices.emplace_back(point.x, point.y);
    }
    return ring;
}

// Which polygon a ring came from: gis.part, which a result writes (the
// exterior's id), or source.part, which IMPORT writes (the file's feature).
std::optional<std::string> partOf(const Entity& entity)
{
    if (auto part = tagOf(entity, "gis.part")) {
        return part;
    }
    return tagOf(entity, "source.part");
}

// Tagged holes join the area they lie in: the one whose part they share
// when there is such an area around them, else the smallest around them, and
// of equals the one made nearest before the hole - the same file imported
// twice gives two exteriors equal in everything but their ids, and each hole
// was made just after its own. A hole with no area around it stays an area
// of its own, and says so.
void joinHoles(std::vector<Shape>& shapes, DrawingDatasetStats& stats)
{
    std::vector<std::size_t> areas;
    for (std::size_t i = 0; i < shapes.size(); ++i) {
        if (shapes[i].kind == GeometryKind::Polygon && !shapes[i].isHole) {
            areas.push_back(i);
        }
    }
    std::vector<katana::geometry::Polyline2> rings;
    rings.reserve(areas.size());
    for (const std::size_t index : areas) {
        rings.push_back(ringOf(shapes[index].points));
    }
    std::size_t lost = 0;
    for (Shape& hole : shapes) {
        if (!hole.isHole) {
            continue;
        }
        const std::optional<std::string> part = partOf(*hole.entity);
        const EntityId holeId = hole.entity->id;
        // Made before the hole, and how long before: the nearer the better.
        const auto before = [holeId](EntityId id) {
            return id < holeId ? holeId - id : std::numeric_limits<EntityId>::max();
        };
        std::optional<std::size_t> best;
        bool bestSharesPart = false;
        double bestArea = 0.0;
        for (std::size_t a = 0; a < areas.size(); ++a) {
            const bool inside = std::ranges::all_of(hole.points, [&](const GeoPoint& point) {
                return rings[a].contains(Point2(point.x, point.y));
            });
            if (!inside) {
                continue;
            }
            const bool sharesPart = part && partOf(*shapes[areas[a]].entity) == part;
            const double area = rings[a].area();
            const bool better =
                !best || (sharesPart && !bestSharesPart) ||
                (sharesPart == bestSharesPart &&
                 (area < bestArea ||
                  (area == bestArea && before(shapes[areas[a]].entity->id) <
                                           before(shapes[areas[*best]].entity->id))));
            if (better) {
                best = a;
                bestSharesPart = sharesPart;
                bestArea = area;
            }
        }
        if (!best) {
            hole.isHole = false;
            ++lost;
            continue;
        }
        Shape& area = shapes[areas[*best]];
        area.holes.push_back(hole.points);
        area.holesHaveZ = area.holesHaveZ && hole.hasZ;
        hole.kind = GeometryKind::Unknown; // taken into its area
        ++stats.used;
    }
    if (lost != 0) {
        stats.warnings.push_back(std::to_string(lost) +
                                 " rings tagged as holes lie in no area in scope and were kept "
                                 "as areas of their own");
    }
}

gp::FieldType fieldTypeOf(const PropertyValue& value)
{
    return std::visit(
        [](const auto& held) {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, bool>) {
                return gp::FieldType::Boolean;
            } else if constexpr (std::is_same_v<Held, std::int64_t>) {
                return gp::FieldType::Integer64;
            } else if constexpr (std::is_same_v<Held, double>) {
                return gp::FieldType::Real;
            } else {
                return gp::FieldType::String;
            }
        },
        value);
}

// A text property's type as its entity's metadata keeps it (a date), else
// text.
gp::FieldType textTypeOf(const Entity& entity, const std::string& key)
{
    for (const std::string_view prefix : {kImportTypePrefix, kResultTypePrefix}) {
        const auto tag = entity.metadata.find(std::string(prefix) + key);
        if (tag == entity.metadata.end()) {
            continue;
        }
        const std::string text = katana::entity::toString(tag->second);
        if (text == "date") {
            return gp::FieldType::Date;
        }
        if (text == "datetime") {
            return gp::FieldType::DateTime;
        }
    }
    return gp::FieldType::String;
}

gp::FieldValue fieldValueOf(const PropertyValue& value, gp::FieldType type)
{
    if (type == gp::FieldType::String || type == gp::FieldType::Date ||
        type == gp::FieldType::DateTime) {
        return katana::entity::toString(value);
    }
    return std::visit([](const auto& held) -> gp::FieldValue { return held; }, value);
}

std::string lowerType(const Entity& entity)
{
    return katana::core::lowered(katana::entity::toString(entity.type()));
}

gp::FeatureTable tableOf(const std::string& name, GeometryKind kind,
                         const std::vector<const Shape*>& shapes, const DrawingDatasetOptions& options,
                         DrawingDatasetStats& stats)
{
    gp::FeatureTable table;
    table.name = name;
    table.kind = kind;
    table.crsWkt = options.crsWkt;
    // The bookkeeping written: katana_id and layer always, style, colour and
    // type unless the caller leaves them out.
    const std::size_t leading = options.styleFields ? kBookkeeping.size() : 2;
    // The properties' fields: typed when every entity agrees, text when not.
    std::map<std::string, gp::FieldType> types;
    std::set<std::string> mixed;
    std::size_t dropped = 0;
    for (const Shape* shape : shapes) {
        if (!options.properties) {
            break;
        }
        for (const auto& [key, value] : shape->entity->properties) {
            if (std::any_of(kBookkeeping.begin(), kBookkeeping.begin() + leading,
                            [&](const char* field) {
                                return katana::core::equalsIgnoringCase(key, field);
                            })) {
                ++dropped;
                continue;
            }
            gp::FieldType type = fieldTypeOf(value);
            if (type == gp::FieldType::String) {
                type = textTypeOf(*shape->entity, key);
            }
            const auto [at, inserted] = types.emplace(key, type);
            if (!inserted && at->second != type) {
                at->second = gp::FieldType::String;
                mixed.insert(key);
            }
        }
    }
    for (const std::string& key : mixed) {
        stats.warnings.push_back("property " + key + " holds values of more than one type in " +
                                 name + "; it is text there");
    }
    if (dropped != 0) {
        std::string fields;
        for (std::size_t f = 0; f < leading; ++f) {
            fields += (f == 0 ? "" : f + 1 == leading ? " or " : ", ") + std::string(kBookkeeping[f]);
        }
        stats.warnings.push_back(std::to_string(dropped) +
                                 " property values named like the fields " + fields +
                                 " were left out of " + name);
    }
    table.fields = {{"katana_id", gp::FieldType::Integer64}, {"layer", gp::FieldType::String}};
    if (options.styleFields) {
        table.fields.push_back({"style", gp::FieldType::String});
        table.fields.push_back({"colour", gp::FieldType::String});
        table.fields.push_back({"type", gp::FieldType::String});
    }
    for (const auto& [key, type] : types) {
        table.fields.push_back({key, type});
    }
    for (const Shape* shape : shapes) {
        const Entity& entity = *shape->entity;
        gp::Feature feature;
        VectorGeometry geometry;
        geometry.kind = shape->kind;
        geometry.parts.push_back(shape->points);
        for (const auto& hole : shape->holes) {
            geometry.parts.push_back(hole);
        }
        geometry.hasZ = shape->hasZ && (shape->holes.empty() || shape->holesHaveZ);
        table.hasZ = table.hasZ || geometry.hasZ;
        feature.parts.push_back(std::move(geometry));
        feature.values.emplace_back(static_cast<std::int64_t>(entity.id));
        feature.values.emplace_back(entity.layer);
        if (options.styleFields) {
            feature.values.emplace_back(entity.style.empty() ? std::string("ByLayer")
                                                             : entity.style);
            feature.values.emplace_back(entity.color ? entity.color->toHex()
                                                     : std::string("ByLayer"));
            feature.values.emplace_back(lowerType(entity));
        }
        for (const auto& [key, type] : types) {
            const auto found = entity.properties.find(key);
            feature.values.push_back(found == entity.properties.end()
                                         ? gp::FieldValue(std::monostate{})
                                         : fieldValueOf(found->second, type));
        }
        table.features.push_back(std::move(feature));
    }
    return table;
}

// ---- features to the drawing ------------------------------------------------------------

std::optional<PropertyValue> propertyOf(const gp::FieldValue& value)
{
    return std::visit(
        [](const auto& held) -> std::optional<PropertyValue> {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, std::monostate>) {
                return std::nullopt;
            } else {
                return PropertyValue(held);
            }
        },
        value);
}

std::optional<EntityId> sourceOf(const gp::FeatureTable& table, const gp::Feature& feature)
{
    for (std::size_t f = 0; f < table.fields.size() && f < feature.values.size(); ++f) {
        if (table.fields[f].name != "katana_id") {
            continue;
        }
        if (const auto* id = std::get_if<std::int64_t>(&feature.values[f]); id != nullptr && *id > 0) {
            return static_cast<EntityId>(*id);
        }
        if (const auto* real = std::get_if<double>(&feature.values[f]);
            real != nullptr && *real >= 1.0 && std::isfinite(*real)) {
            return static_cast<EntityId>(*real);
        }
    }
    return std::nullopt;
}

bool hasKatanaId(const gp::FeatureTable& table)
{
    return std::ranges::any_of(table.fields,
                               [](const gp::FieldDef& field) { return field.name == "katana_id"; });
}

// A vertex on its way to an entity: where it is, and its height if it has one.
struct Vertex {
    Point2 at;
    std::optional<double> z;
};

// A part's points as the vertices of the line it becomes, its arcs made chords
// by the rule EXPORT makes them by (curve_chords.hpp). A level arc's chords
// carry its height; a sloping one's inner chord points carry none, since a
// height between an arc's ends would be invented.
std::vector<Vertex> verticesOf(const std::vector<GeoPoint>& source,
                               const std::vector<std::size_t>* arcs, bool hasZ,
                               const FeaturePieceOptions& options, FeaturePieceCounts& counts)
{
    const auto shifted = [&options](const GeoPoint& raw) {
        return options.originShift ? Point2(raw.x - options.originShift->x,
                                            raw.y - options.originShift->y)
                                   : Point2(raw.x, raw.y);
    };
    const auto heightOf = [hasZ](const GeoPoint& raw) {
        return hasZ && std::isfinite(raw.z) ? std::optional<double>(raw.z) : std::nullopt;
    };
    std::vector<Vertex> out;
    out.reserve(source.size());
    for (std::size_t i = 0; i < source.size(); ++i) {
        const bool arcHere = arcs != nullptr && std::ranges::find(*arcs, i) != arcs->end() &&
                             i + 2 < source.size();
        if (!arcHere) {
            out.push_back({shifted(source[i]), heightOf(source[i])});
            continue;
        }
        const Point2 start = shifted(source[i]);
        const Point2 end = shifted(source[i + 2]);
        const auto arc = katana::geometry::Arc2::throughPoints(start, shifted(source[i + 1]), end);
        const std::optional<double> z0 = heightOf(source[i]);
        const std::optional<double> z1 = heightOf(source[i + 2]);
        out.push_back({start, z0});
        if (arc) {
            ++counts.curvesMadeChords;
            const int chords = detail::chordCount(arc->radius, arc->sweep, options.curveTolerance);
            const std::optional<double> inner = z0 && z1 && *z0 == *z1 ? z0 : std::nullopt;
            for (int k = 1; k < chords; ++k) {
                out.push_back({arc->pointAt(static_cast<double>(k) / static_cast<double>(chords)),
                               inner});
            }
        }
        // The end is the next arc's start or the next straight run's first
        // point, taken as the loop reaches it.
        i += 1;
    }
    return out;
}

// Consecutive repeats dropped, as IMPORT drops them: a zero-length segment is
// one the model refuses. A repeat at another height takes that height with it.
struct Vertices {
    std::vector<Point2> points;
    std::vector<std::optional<double>> heights;
};

Vertices distinct(const std::vector<Vertex>& source, FeaturePieceCounts& counts)
{
    Vertices out;
    out.points.reserve(source.size());
    out.heights.reserve(source.size());
    for (const Vertex& vertex : source) {
        if (!std::isfinite(vertex.at.x) || !std::isfinite(vertex.at.y)) {
            continue;
        }
        if (!out.points.empty() && out.points.back() == vertex.at) {
            counts.heightsLost += vertex.z != out.heights.back() ? 1u : 0u;
            continue;
        }
        out.points.push_back(vertex.at);
        out.heights.push_back(vertex.z);
    }
    return out;
}

const std::vector<std::size_t>* arcsOfPart(const VectorGeometry& geometry, std::size_t part)
{
    return part < geometry.arcs.size() && !geometry.arcs[part].empty() ? &geometry.arcs[part]
                                                                       : nullptr;
}

// A part that is one arc and nothing else: its start, middle and end.
bool isOneArc(const VectorGeometry& geometry, std::size_t part)
{
    const auto* arcs = arcsOfPart(geometry, part);
    return arcs != nullptr && arcs->size() == 1 && arcs->front() == 0 &&
           part < geometry.parts.size() && geometry.parts[part].size() == 3;
}

// The circle a CircularString of three points is when it ends where it began:
// its middle point is the far side, so the centre is halfway to it.
std::optional<katana::geometry::Circle2> wholeCircle(const Point2& start, const Point2& far)
{
    const double radius = std::hypot(far.x - start.x, far.y - start.y) / 2.0;
    if (!(radius > 0.0) || !std::isfinite(radius)) {
        return std::nullopt;
    }
    return katana::geometry::Circle2{Point2((start.x + far.x) / 2.0, (start.y + far.y) / 2.0),
                                     radius};
}

// One arc part as an Arc or a Circle entity; nullopt when its points are in a
// line, which then goes as the straight line it is.
std::optional<FeaturePiece> arcPiece(const VectorGeometry& geometry, std::size_t part,
                                     const FeaturePieceOptions& options)
{
    const auto& points = geometry.parts[part];
    const auto at = [&options](const GeoPoint& raw) {
        return options.originShift ? Point2(raw.x - options.originShift->x,
                                            raw.y - options.originShift->y)
                                   : Point2(raw.x, raw.y);
    };
    const auto z = [&geometry](const GeoPoint& raw) {
        return geometry.hasZ && std::isfinite(raw.z) ? std::optional<double>(raw.z)
                                                     : std::nullopt;
    };
    const Point2 start = at(points[0]);
    const Point2 end = at(points[2]);
    if (start == end) {
        if (const auto circle = wholeCircle(start, at(points[1]))) {
            return FeaturePiece{*circle, {z(points[0])}, std::nullopt};
        }
        return std::nullopt;
    }
    if (const auto arc = katana::geometry::Arc2::throughPoints(start, at(points[1]), end)) {
        return FeaturePiece{*arc, {z(points[0]), z(points[2])}, std::nullopt};
    }
    return std::nullopt;
}

std::vector<FeaturePiece> piecesOf(const VectorGeometry& geometry,
                                   const FeaturePieceOptions& options, FeaturePieceCounts& counts)
{
    std::vector<FeaturePiece> pieces;
    const auto verticesOfPart = [&](std::size_t part) {
        return verticesOf(part < geometry.parts.size() ? geometry.parts[part]
                                                       : std::vector<GeoPoint>{},
                          arcsOfPart(geometry, part), geometry.hasZ, options, counts);
    };
    switch (geometry.kind) {
    case GeometryKind::Point: {
        const Vertices vertices = distinct(verticesOfPart(0), counts);
        if (vertices.points.empty()) {
            ++counts.degenerate;
            break;
        }
        pieces.push_back({katana::entity::PointGeometry{vertices.points.front()},
                          {vertices.heights.front()},
                          std::nullopt});
        break;
    }
    case GeometryKind::LineString: {
        if (isOneArc(geometry, 0)) {
            if (auto piece = arcPiece(geometry, 0, options)) {
                pieces.push_back(std::move(*piece));
                break;
            }
        }
        Vertices vertices = distinct(verticesOfPart(0), counts);
        // A line that ends where it began is a closed one. GDAL reads a DXF
        // CIRCLE, and a closed LWPOLYLINE, as a line string that repeats its
        // first point, and taken as it came a circle was an OPEN polyline with
        // a seam: no area, no hatch, and Explode or Offset treating it as a
        // path with two ends. As a polygon ring does, the repeat becomes the
        // `closed` flag. Four points at least: three with the ends equal are a
        // line there and back, which encloses nothing. The ends must meet in
        // height too (both absent, or equal): a ramp or helix that comes back
        // over its start a level higher is an open string, and closing it
        // would throw away its top height.
        bool closed = false;
        if (vertices.points.size() >= 4 && vertices.points.front() == vertices.points.back() &&
            vertices.heights.front() == vertices.heights.back()) {
            vertices.points.pop_back();
            vertices.heights.pop_back();
            closed = true;
        }
        if (vertices.points.size() < 2) {
            ++counts.degenerate;
            break;
        }
        // Two points is a line in every CAD program; anything longer is a
        // polyline. A two-point polyline would be unfilletable and odd to edit.
        if (vertices.points.size() == 2 && !closed) {
            pieces.push_back({katana::geometry::Segment2{vertices.points[0], vertices.points[1]},
                              vertices.heights,
                              std::nullopt});
        } else {
            katana::geometry::Polyline2 line;
            line.vertices = vertices.points;
            line.closed = closed;
            pieces.push_back({std::move(line), vertices.heights, std::nullopt});
        }
        break;
    }
    case GeometryKind::Polygon: {
        // A circle, whole, and nothing cut out of it: the Circle it was.
        if (geometry.parts.size() == 1 && isOneArc(geometry, 0)) {
            if (auto piece = arcPiece(geometry, 0, options);
                piece && std::holds_alternative<katana::geometry::Circle2>(piece->geometry)) {
                piece->ring = "exterior";
                pieces.push_back(std::move(*piece));
                break;
            }
        }
        for (std::size_t r = 0; r < geometry.parts.size(); ++r) {
            Vertices vertices = distinct(verticesOfPart(r), counts);
            // A closed ring repeats its first point; the Polyline2 `closed`
            // flag expresses that instead.
            if (vertices.points.size() > 1 && vertices.points.front() == vertices.points.back()) {
                vertices.points.pop_back();
                counts.heightsLost += vertices.heights.back() != vertices.heights.front() ? 1u : 0u;
                vertices.heights.pop_back();
            }
            if (vertices.points.size() < 3) {
                if (r == 0) {
                    ++counts.degenerate;
                    return {};
                }
                continue;
            }
            katana::geometry::Polyline2 ring;
            ring.vertices = std::move(vertices.points);
            ring.closed = true;
            pieces.push_back({std::move(ring), std::move(vertices.heights),
                              std::string(r == 0 ? "exterior" : "hole")});
        }
        break;
    }
    case GeometryKind::Unknown:
        ++counts.degenerate;
        break;
    }
    return pieces;
}

std::string sanitizedTableName(const std::string& name)
{
    std::string out;
    for (const char c : name) {
        out += c == katana::entity::kLayerSeparator ? '_' : c;
    }
    out = std::string(katana::core::trimmed(out));
    return out.empty() || out == "." || out == ".." ? std::string("features") : out;
}

Result<ResultPlan> createPlan(const katana::entity::Model& model, const gp::FeatureSet& set,
                              const ResultOptions& options)
{
    if (auto valid = katana::entity::validateLayerPath(options.targetLayer); !valid) {
        return makeError(ErrorCode::InvalidArgument, "the target is not a layer path: " +
                                                         valid.error().message,
                         options.targetLayer);
    }
    ResultPlan plan;
    auto transaction = std::make_unique<cmd::Transaction>(
        options.commandName.empty() ? std::string("GDAL") : options.commandName);
    std::size_t tablesWithFeatures = 0;
    for (const gp::FeatureTable& table : set.tables) {
        tablesWithFeatures += table.features.empty() ? 0u : 1u;
    }
    std::vector<Entity> entities;
    // The id the first entity will be given (ids are monotonic and the
    // transaction creates them in order), so a polygon's rings name their
    // exterior in gis.part. A later change that broke the prediction would
    // cost only the hint: drawingDataset checks that a hole lies inside the
    // area its part names before it joins them.
    const EntityId firstId = model.entities.nextId();
    FeaturePieceCounts counts;
    for (const gp::FeatureTable& table : set.tables) {
        if (table.features.empty()) {
            continue;
        }
        const std::string layer = tablesWithFeatures > 1
                                      ? options.targetLayer + "/" + sanitizedTableName(table.name)
                                      : options.targetLayer;
        if (auto valid = katana::entity::validateLayerPath(layer); !valid) {
            return makeError(ErrorCode::InvalidArgument,
                             "a result table's layer is not a layer path: " + valid.error().message,
                             layer);
        }
        if (std::ranges::find(plan.layers, layer) == plan.layers.end()) {
            plan.layers.push_back(layer);
            if (!model.layers.contains(layer)) {
                katana::entity::Layer created;
                created.name = layer;
                transaction->add(cmd::createLayer(created));
            }
        }
        for (const gp::Feature& feature : table.features) {
            katana::entity::PropertyMap properties;
            for (std::size_t f = 0; f < table.fields.size() && f < feature.values.size(); ++f) {
                if (isBookkeepingField(table.fields[f].name)) {
                    continue;
                }
                if (auto value = propertyOf(feature.values[f])) {
                    properties[options.propertyPrefix + table.fields[f].name] = std::move(*value);
                }
            }
            if (!options.operation.empty()) {
                properties["gis.op"] = options.operation;
            }
            if (const auto source = sourceOf(table, feature)) {
                properties["gis.source"] = static_cast<std::int64_t>(*source);
            }
            for (const VectorGeometry& part : feature.parts) {
                const std::size_t exterior = entities.size();
                // The rings are tagged only where there are holes to join.
                const bool withHoles = part.kind == GeometryKind::Polygon && part.parts.size() > 1;
                std::vector<FeaturePiece> pieces = piecesOf(part, {}, counts);
                for (FeaturePiece& piece : pieces) {
                    Entity entity;
                    entity.geometry = std::move(piece.geometry);
                    entity.layer = layer;
                    entity.properties = properties;
                    for (const gp::FieldDef& field : table.fields) {
                        const std::string key = options.propertyPrefix + field.name;
                        if (const auto tag = dateTypeTag(field.type);
                            tag && entity.properties.contains(key)) {
                            entity.metadata[std::string(kResultTypePrefix) + key] = *tag;
                        }
                    }
                    if (withHoles && piece.ring) {
                        entity.properties["gis.ring"] = *piece.ring;
                        entity.properties["gis.part"] =
                            static_cast<std::int64_t>(firstId + exterior);
                    }
                    katana::entity::setHeights(entity.properties, piece.heights);
                    entities.push_back(std::move(entity));
                }
            }
        }
    }
    if (counts.degenerate != 0) {
        plan.warnings.push_back(std::to_string(counts.degenerate) +
                                " result geometries had too few distinct points to draw and were "
                                "left out");
    }
    plan.skipped = counts.degenerate;
    plan.created = entities.size();
    if (!entities.empty()) {
        transaction->add(cmd::createEntities(std::move(entities)));
    }
    if (transaction->size() != 0) {
        plan.command = std::move(transaction);
    }
    return plan;
}

// The heights of a replaced geometry, as property edits of `entity`.
void heightEdits(const Entity& entity, const std::vector<std::optional<double>>& heights,
                 cmd::Transaction& transaction)
{
    katana::entity::PropertyMap after = entity.properties;
    katana::entity::setHeights(after, heights);
    for (const std::string_view key :
         {katana::entity::kElevationProperty, katana::entity::kElevationsProperty}) {
        const auto before = entity.properties.find(key);
        const auto now = after.find(key);
        if (now != after.end() && (before == entity.properties.end() || before->second != now->second)) {
            transaction.add(cmd::setEntityProperty({entity.id}, std::string(key), now->second));
        } else if (now == after.end() && before != entity.properties.end()) {
            transaction.add(cmd::removeEntityProperty({entity.id}, std::string(key)));
        }
    }
}

Result<ResultPlan> inPlacePlan(const katana::entity::Model& model, const gp::FeatureSet& set,
                               const ResultOptions& options)
{
    ResultPlan plan;
    auto transaction = std::make_unique<cmd::Transaction>(
        options.commandName.empty() ? std::string("GDAL") : options.commandName);
    std::size_t missing = 0, several = 0;
    FeaturePieceCounts counts;
    for (const gp::FeatureTable& table : set.tables) {
        if (table.features.empty()) {
            continue;
        }
        if (!hasKatanaId(table)) {
            return makeError(ErrorCode::InvalidArgument,
                             "the result has no katana_id to find its entities by", table.name);
        }
        for (const gp::Feature& feature : table.features) {
            const auto id = sourceOf(table, feature);
            const Entity* entity = id ? model.entities.find(*id) : nullptr;
            if (entity == nullptr) {
                ++missing;
                continue;
            }
            if (options.mode == ResultMode::UpdateGeometry) {
                if (feature.parts.size() != 1 ||
                    (feature.parts.front().kind == GeometryKind::Polygon &&
                     feature.parts.front().parts.size() > 1)) {
                    ++several;
                    continue;
                }
                std::vector<FeaturePiece> pieces = piecesOf(feature.parts.front(), {}, counts);
                if (pieces.size() != 1) {
                    continue;
                }
                transaction->add(cmd::setEntityGeometry(entity->id, std::move(pieces.front().geometry)));
                heightEdits(*entity, pieces.front().heights, *transaction);
                ++plan.updated;
                continue;
            }
            bool changed = false;
            for (std::size_t f = 0; f < table.fields.size() && f < feature.values.size(); ++f) {
                if (isBookkeepingField(table.fields[f].name)) {
                    continue;
                }
                auto value = propertyOf(feature.values[f]);
                if (!value) {
                    continue;
                }
                const std::string key = options.propertyPrefix + table.fields[f].name;
                const auto current = entity->properties.find(key);
                if (current != entity->properties.end() && current->second == *value) {
                    continue;
                }
                transaction->add(cmd::setEntityProperty({entity->id}, key, std::move(*value)));
                changed = true;
            }
            plan.updated += changed ? 1u : 0u;
        }
    }
    if (missing != 0) {
        plan.warnings.push_back(std::to_string(missing) +
                                " results name an entity the drawing no longer has");
    }
    if (several != 0) {
        plan.warnings.push_back(std::to_string(several) +
                                " results of several parts cannot replace one entity and were "
                                "left out");
    }
    plan.skipped = missing + several + counts.degenerate;
    if (transaction->size() != 0) {
        plan.command = std::move(transaction);
    }
    return plan;
}

} // namespace

std::optional<std::string> dateTypeTag(gp::FieldType type)
{
    if (type == gp::FieldType::Date) {
        return "date";
    }
    if (type == gp::FieldType::DateTime) {
        return "datetime";
    }
    return std::nullopt;
}

bool isBookkeepingField(const std::string& name)
{
    return std::ranges::any_of(kBookkeeping, [&](const char* field) { return name == field; });
}

Result<DrawingDataset> drawingDataset(const katana::entity::Model& model,
                                      const std::vector<EntityId>& ids,
                                      const DrawingDatasetOptions& options)
{
    if (!(options.curveTolerance > 0.0) || !std::isfinite(options.curveTolerance)) {
        return makeError(ErrorCode::InvalidArgument, "the curve tolerance must be positive",
                         katana::core::formatExactReal(options.curveTolerance));
    }
    DrawingDataset result;
    DrawingDatasetStats& stats = result.stats;
    stats.matched = ids.size();
    std::vector<Shape> shapes;
    shapes.reserve(ids.size());
    HeightNotes notes;
    for (const EntityId id : ids) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            ++stats.skipped["missing"];
            continue;
        }
        auto shape = shapeOf(*entity, options, notes);
        if (const auto* reason = std::get_if<std::string>(&shape)) {
            ++stats.skipped[*reason];
            continue;
        }
        shapes.push_back(std::move(std::get<Shape>(shape)));
    }
    joinHoles(shapes, stats);

    std::vector<const Shape*> points, lines, polygons;
    for (const Shape& shape : shapes) {
        switch (shape.kind) {
        case GeometryKind::Point:
            points.push_back(&shape);
            break;
        case GeometryKind::LineString:
            lines.push_back(&shape);
            break;
        case GeometryKind::Polygon:
            polygons.push_back(&shape);
            break;
        case GeometryKind::Unknown:
            break; // a hole, taken into its area
        }
    }
    stats.points = points.size();
    stats.lines = lines.size();
    stats.polygons = polygons.size();
    stats.used += points.size() + lines.size() + polygons.size();
    if (notes.partial > 0) {
        stats.warnings.push_back(
            std::to_string(notes.partial) +
            " entities have heights at only some of their vertices and were written in plan, "
            "their heights kept as attributes: a 3D geometry needs a height at every vertex");
    }
    if (notes.slopingArcs > 0) {
        stats.warnings.push_back(
            std::to_string(notes.slopingArcs) +
            " arcs have different heights at their two ends and were written in plan, their "
            "heights kept as attributes: a height between the ends of an arc is not recorded");
    }
    if (options.oneTable) {
        std::vector<const Shape*> all;
        for (const Shape& shape : shapes) {
            if (shape.kind != GeometryKind::Unknown) {
                all.push_back(&shape);
            }
        }
        if (!all.empty()) {
            const GeometryKind first = all.front()->kind;
            const bool uniform = std::ranges::all_of(
                all, [first](const Shape* shape) { return shape->kind == first; });
            result.set.tables.push_back(tableOf(options.tableName,
                                                uniform ? first : GeometryKind::Unknown, all,
                                                options, stats));
        }
        return result;
    }
    if (!points.empty()) {
        result.set.tables.push_back(tableOf("points", GeometryKind::Point, points, options, stats));
    }
    if (!lines.empty()) {
        result.set.tables.push_back(tableOf("lines", GeometryKind::LineString, lines, options, stats));
    }
    if (!polygons.empty()) {
        result.set.tables.push_back(
            tableOf("polygons", GeometryKind::Polygon, polygons, options, stats));
    }
    return result;
}

std::vector<FeaturePiece> featurePieces(const katana::gis::VectorGeometry& geometry,
                                        const FeaturePieceOptions& options,
                                        FeaturePieceCounts& counts)
{
    return piecesOf(geometry, options, counts);
}

Result<ResultPlan> resultCommand(const katana::entity::Model& model, const gp::FeatureSet& set,
                                 const ResultOptions& options)
{
    auto plan = options.mode == ResultMode::Create ? createPlan(model, set, options)
                                                   : inPlacePlan(model, set, options);
    if (!plan || !options.deleteSources) {
        return plan;
    }
    std::vector<EntityId> sources;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            const auto id = sourceOf(table, feature);
            if (id && model.entities.contains(*id) && std::ranges::find(sources, *id) == sources.end()) {
                sources.push_back(*id);
            }
        }
    }
    if (sources.empty()) {
        return plan;
    }
    auto transaction = std::make_unique<cmd::Transaction>(
        options.commandName.empty() ? std::string("GDAL") : options.commandName);
    if (plan->command) {
        transaction->add(std::move(plan->command));
    }
    plan->deleted = sources.size();
    transaction->add(cmd::deleteEntities(std::move(sources)));
    plan->command = std::move(transaction);
    return plan;
}

} // namespace katana::interop::geo
