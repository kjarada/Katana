#include "katana/entity/leader_values.hpp"

#include <algorithm>
#include <cmath>
#include <iterator>

#include "katana/entity/anchor.hpp"
#include "katana/math/numerics.hpp"
#include "value_parts.hpp"

namespace katana::entity {

using katana::core::Status;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

// In the order HELP and LEADER VALUES list them: what every target has, then
// the tip's level and a text's words, then the linework's, then the areas',
// then a dimension's.
constexpr std::string_view kLeaderValues[] = {
    "id",       "layer",    "type",  "code",    "point",   "description", "x",           "y",
    "easting",  "northing", "z",     "rl",      "text",    "bearing",     "distance",    "length",
    "dx",       "dy",       "dz",    "grade",   "segment", "chainage",    "vertices",    "radius",
    "diameter", "chord",    "delta", "tangent", "area",    "perimeter",   "measurement", "angle"};

void put(LabelValues& values, std::string_view name, LabelQuantity quantity, double number)
{
    values.insert_or_assign(std::string(name), LabelValue::of(quantity, number));
}

void putLevel(LabelValues& values, std::optional<double> z)
{
    if (z) {
        put(values, "z", LabelQuantity::Number, *z);
        put(values, "rl", LabelQuantity::Number, *z);
    }
}

// The height `t` of the way from a vertex of height `a` to one of height `b`:
// either end's own height AT that end, linear between two known heights, and
// absent otherwise - a level is never made up from one end alone.
std::optional<double> heightBetween(std::optional<double> a, std::optional<double> b, double t)
{
    if (t <= 0.0) {
        return a;
    }
    if (t >= 1.0) {
        return b;
    }
    if (a && b) {
        return *a + (*b - *a) * t;
    }
    return std::nullopt;
}

// How far along a line or an arc `place` is, as a fraction from its start;
// nullopt for a place that is not on it (an arc's centre).
std::optional<double> fractionAlong(const AnchorRef& place)
{
    switch (place.point) {
    case AnchorPoint::Start:
        return 0.0;
    case AnchorPoint::End:
        return 1.0;
    case AnchorPoint::Mid:
        return 0.5;
    case AnchorPoint::Along:
        return std::isfinite(place.parameter) ? std::clamp(place.parameter, 0.0, 1.0) : 0.0;
    default:
        return std::nullopt;
    }
}

// The segment of a polyline `place` is on and how far along it, nullopt for
// a place not on the line (Inside). The last vertex of an open polyline is
// the END of its last segment; every other vertex starts the segment after it.
std::optional<std::pair<std::size_t, double>> segmentAlong(const Polyline2& polyline,
                                                           const AnchorRef& place)
{
    const std::size_t n = polyline.vertices.size();
    if (n < 2) {
        return std::nullopt;
    }
    const std::size_t segments = polyline.closed ? n : n - 1;
    const auto atVertex = [&](std::size_t i) -> std::optional<std::pair<std::size_t, double>> {
        if (i >= n) {
            return std::nullopt;
        }
        return i < segments ? std::pair{i, 0.0} : std::pair{segments - 1, 1.0};
    };
    switch (place.point) {
    case AnchorPoint::Start:
        return atVertex(0);
    case AnchorPoint::End:
        return atVertex(n - 1);
    case AnchorPoint::Vertex:
        return atVertex(place.index);
    case AnchorPoint::SegmentMid:
    case AnchorPoint::Along:
        if (place.index >= segments) {
            return std::nullopt;
        }
        return std::pair{
            static_cast<std::size_t>(place.index),
            place.point == AnchorPoint::SegmentMid
                ? 0.5
                : (std::isfinite(place.parameter) ? std::clamp(place.parameter, 0.0, 1.0) : 0.0)};
    default:
        return std::nullopt;
    }
}

} // namespace

std::vector<std::string_view> leaderValueNames()
{
    return {std::begin(kLeaderValues), std::end(kLeaderValues)};
}

std::optional<LabelQuantity> leaderValueQuantity(std::string_view name)
{
    if (std::find(std::begin(kLeaderValues), std::end(kLeaderValues), name) ==
        std::end(kLeaderValues)) {
        return std::nullopt;
    }
    if (name == "id" || name == "segment" || name == "vertices") {
        return LabelQuantity::Integer;
    }
    if (name == "layer" || name == "type" || name == "code" || name == "point" ||
        name == "description" || name == "text") {
        return LabelQuantity::Text;
    }
    if (name == "z" || name == "rl" || name == "dz" || name == "grade") {
        return LabelQuantity::Number;
    }
    if (name == "bearing") {
        return LabelQuantity::Bearing;
    }
    if (name == "delta" || name == "angle") {
        return LabelQuantity::Angle;
    }
    if (name == "area") {
        return LabelQuantity::Area;
    }
    if (name == "chainage") {
        return LabelQuantity::Chainage;
    }
    return LabelQuantity::Length;
}

Status checkLeaderTemplate(std::string_view templateText)
{
    return checkTemplate(templateText, leaderValueQuantity, [](std::string_view name) {
        return "a leader has no value '" + std::string(name) + "' (LEADER VALUES lists them)";
    });
}

LabelValues anchorValues(const Entity& target, const AnchorRef& ref, const Point2& tip,
                         std::span<const std::string> codeProperties)
{
    LabelValues values;
    detail::addCommonValues(target, codeProperties, values);
    values.insert_or_assign("type", LabelValue::ofText(std::string(toString(target.type()))));

    // The place the tip is at: the one `ref` names, or - when the target no
    // longer has it - the place on the target nearest the tip.
    AnchorRef place = ref;
    std::optional<Point2> at = resolveAnchor(target, ref);
    if (!at) {
        if (const auto nearest = nearestAnchor(target, tip)) {
            place = *nearest;
            at = resolveAnchor(target, place);
        }
    }
    const Point2 where = at.value_or(tip);
    put(values, "x", LabelQuantity::Length, where.x);
    put(values, "easting", LabelQuantity::Length, where.x);
    put(values, "y", LabelQuantity::Length, where.y);
    put(values, "northing", LabelQuantity::Length, where.y);

    const Geometry& geometry = target.geometry;
    if (std::holds_alternative<PointGeometry>(geometry)) {
        putLevel(values, heightsOf(target.properties, 1).front());
    } else if (const auto* text = std::get_if<TextGeometry>(&geometry)) {
        values.insert_or_assign("text", LabelValue::ofText(text->text));
        putLevel(values, heightsOf(target.properties, 1).front());
    } else if (const auto* line = std::get_if<Segment2>(&geometry)) {
        const auto z = heightsOf(target.properties, 2);
        detail::addSegmentValues(values, line->start, line->end, 0, z[0], z[1]);
        if (const auto t = fractionAlong(place)) {
            put(values, "chainage", LabelQuantity::Chainage, *t * line->length());
            putLevel(values, heightBetween(z[0], z[1], *t));
        }
    } else if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        detail::addArcValues(values, *arc);
        put(values, "diameter", LabelQuantity::Length, 2.0 * arc->radius);
        const auto z = heightsOf(target.properties, 2);
        if (const auto t = fractionAlong(place)) {
            put(values, "chainage", LabelQuantity::Chainage, *t * arc->length());
            putLevel(values, heightBetween(z[0], z[1], *t));
        }
    } else if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        put(values, "radius", LabelQuantity::Length, circle->radius);
        put(values, "diameter", LabelQuantity::Length, 2.0 * circle->radius);
        put(values, "length", LabelQuantity::Length, circle->perimeter());
        put(values, "perimeter", LabelQuantity::Length, circle->perimeter());
        put(values, "area", LabelQuantity::Area, circle->area());
        putLevel(values, heightsOf(target.properties, 1).front());
    } else if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        const auto& v = polyline->vertices;
        const auto z = heightsOf(target.properties, v.size());
        if (const auto piece = segmentAlong(*polyline, place)) {
            const auto [i, t] = *piece;
            const std::size_t j = (i + 1) % v.size();
            detail::addSegmentValues(values, v[i], v[j], i, z[i], z[j]);
            double before = 0.0;
            for (std::size_t k = 0; k < i; ++k) {
                before += v[k].distanceTo(v[k + 1]);
            }
            put(values, "chainage", LabelQuantity::Chainage, before + t * v[i].distanceTo(v[j]));
            putLevel(values, heightBetween(z[i], z[j], t));
        } else if (!z.empty() && std::all_of(z.begin(), z.end(), [&](const auto& h) {
                       return h && z.front() && *h == *z.front();
                   })) {
            // Inside a figure every vertex of which is at one level - a
            // building pad - the tip is at that level too.
            putLevel(values, z.front());
        }
        // The whole line's, over the segment's that addSegmentValues set.
        put(values, "length", LabelQuantity::Length, polyline->length());
        put(values, "vertices", LabelQuantity::Integer, static_cast<double>(v.size()));
        if (polyline->closed && v.size() >= 3) {
            put(values, "area", LabelQuantity::Area, polyline->area());
            put(values, "perimeter", LabelQuantity::Length, polyline->perimeter());
        }
    } else if (const auto* dimension = std::get_if<DimensionGeometry>(&geometry)) {
        if (dimension->kind == DimensionKind::Angular) {
            put(values, "angle", LabelQuantity::Angle, dimension->measurement());
        } else {
            put(values, "measurement", LabelQuantity::Length, dimension->measurement());
        }
    }
    return values;
}

std::optional<std::string> leaderTemplate(const Model& model, const LeaderGeometry& leader)
{
    if (!leader.labelStyle.empty()) {
        const LabelStyle* style = model.labelStyles.find(leader.labelStyle);
        return style != nullptr ? std::optional(style->text) : std::nullopt;
    }
    if (leader.fields) {
        return leader.text;
    }
    return std::nullopt;
}

std::optional<LabelValues> leaderValues(const Model& model, const LeaderGeometry& leader,
                                        std::span<const std::string> codeProperties)
{
    if (!leader.tipRef.associated() || leader.vertices.empty()) {
        return std::nullopt;
    }
    const Entity* target = model.entities.find(leader.tipRef.entity);
    if (target == nullptr) {
        return std::nullopt;
    }
    return anchorValues(*target, leader.tipRef, leader.vertices.front(), codeProperties);
}

std::string leaderNote(const Model& model, const LeaderGeometry& leader,
                       std::span<const std::string> codeProperties)
{
    if (!isSmart(leader)) {
        return leader.text;
    }
    const auto templateText = leaderTemplate(model, leader);
    if (!templateText) {
        return {};
    }
    const auto values = leaderValues(model, leader, codeProperties);
    return formatLabel(*templateText, values ? *values : LabelValues{});
}

std::vector<std::string> missingValues(std::string_view templateText, const LabelValues& values)
{
    std::vector<std::string> missing;
    for (std::string& name : templateFields(templateText)) {
        if (!values.contains(name)) {
            missing.push_back(std::move(name));
        }
    }
    return missing;
}

katana::core::Status checkLeaderSaysSomething(const Model& model, const LeaderGeometry& leader,
                                              std::span<const std::string> codeProperties)
{
    using katana::core::ErrorCode;
    using katana::core::makeError;
    if (!isSmart(leader)) {
        return {};
    }
    if (!leader.tipRef.associated()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a smart leader reads the entity its tip is on: put the tip on one "
                         "(#id, #id.end, #id.inside or #id@x,y on the command line)");
    }
    const Entity* target = model.entities.find(leader.tipRef.entity);
    if (target == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist",
                         "id=" + std::to_string(leader.tipRef.entity));
    }
    if (!leaderTemplate(model, leader)) {
        return makeError(ErrorCode::NotFound, "label style does not exist", leader.labelStyle);
    }
    if (!leaderNote(model, leader, codeProperties).empty()) {
        return {};
    }
    const auto values = leaderValues(model, leader, codeProperties).value_or(LabelValues{});
    std::string missing;
    for (const std::string& name :
         missingValues(leaderTemplate(model, leader).value_or(std::string()), values)) {
        missing += missing.empty() ? name : ", " + name;
    }
    return makeError(ErrorCode::InvalidArgument,
                     "the leader would say nothing: its target has " +
                         (missing.empty() ? std::string("none of the values its template names")
                                          : "no " + missing) +
                         " (LEADER VALUES lists what it has)",
                     "id=" + std::to_string(target->id) +
                         " type=" + std::string(toString(target->type())));
}

} // namespace katana::entity
