#include "katana/entity/leader_values.hpp"

#include <algorithm>
#include <cmath>
#include <iterator>

#include "katana/core/text.hpp"
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

// The context's kinds (leader_values.hpp), a surface's first, then an
// alignment's.
constexpr std::string_view kSurfaceKinds[] = {"surface", "dsurface", "cut", "fill"};
constexpr std::string_view kAlignmentKinds[] = {"station", "offset", "profile", "dprofile"};

bool isSurfaceKind(std::string_view kind)
{
    return std::find(std::begin(kSurfaceKinds), std::end(kSurfaceKinds), kind) !=
           std::end(kSurfaceKinds);
}

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
// By the vertex count and closure alone, so a curve polyline's segments are
// found by the same rule.
std::optional<std::pair<std::size_t, double>> segmentAlong(std::size_t n, bool closed,
                                                           const AnchorRef& place)
{
    if (n < 2) {
        return std::nullopt;
    }
    const std::size_t segments = closed ? n : n - 1;
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

std::vector<std::string_view> leaderContextKinds()
{
    std::vector<std::string_view> kinds(std::begin(kSurfaceKinds), std::end(kSurfaceKinds));
    kinds.insert(kinds.end(), std::begin(kAlignmentKinds), std::end(kAlignmentKinds));
    return kinds;
}

bool isSurfaceContextKind(std::string_view kind)
{
    return isSurfaceKind(kind);
}

std::optional<ContextValueName> contextValueName(std::string_view value)
{
    const std::size_t dot = value.find('.');
    if (dot == std::string_view::npos || dot + 1 >= value.size()) {
        return std::nullopt;
    }
    const std::string_view kind = value.substr(0, dot);
    for (const std::string_view known : leaderContextKinds()) {
        if (known == kind) {
            return ContextValueName{known, value.substr(dot + 1)};
        }
    }
    return std::nullopt;
}

std::optional<LabelQuantity> leaderValueQuantity(std::string_view name)
{
    if (const auto context = contextValueName(name)) {
        // A station is a chainage, so {station.MC01:ch} writes 0+120.000;
        // an offset a length; the rest are levels and differences of level.
        if (context->kind == "station") {
            return LabelQuantity::Chainage;
        }
        if (context->kind == "offset") {
            return LabelQuantity::Length;
        }
        return LabelQuantity::Number;
    }
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

namespace {

// The target's own values at the place `ref` names (anchorValues), and in
// `where` that place: what the context is read at.
LabelValues targetValues(const Entity& target, const AnchorRef& ref, const Point2& tip,
                         std::span<const std::string> codeProperties, Point2& where)
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
    where = at.value_or(tip);
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
        if (const auto piece = segmentAlong(v.size(), polyline->closed, place)) {
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
    } else if (const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&geometry)) {
        // As a polyline, its heights its vertices' own; an arc segment gives
        // an arc's values, a straight one a segment's.
        const auto& v = curve->vertices;
        if (const auto piece = segmentAlong(v.size(), curve->closed, place)) {
            const auto [i, t] = *piece;
            const std::size_t j = curve->segmentEnd(i);
            const auto shape = curve->segment(i);
            if (const auto* arcShape = std::get_if<Arc2>(&shape)) {
                detail::addArcValues(values, *arcShape);
                put(values, "segment", LabelQuantity::Integer, static_cast<double>(i + 1));
            } else {
                detail::addSegmentValues(values, v[i].position, v[j].position, i, v[i].height,
                                         v[j].height);
            }
            const double station = curve->stationOfVertex(i) + t * curve->segmentLength(i);
            put(values, "chainage", LabelQuantity::Chainage, station);
            putLevel(values, heightBetween(v[i].height, v[j].height, t));
        } else if (!v.empty() && std::all_of(v.begin(), v.end(), [&](const auto& vertex) {
                       return vertex.height && v.front().height &&
                              *vertex.height == *v.front().height;
                   })) {
            putLevel(values, v.front().height);
        }
        put(values, "length", LabelQuantity::Length, curve->length());
        put(values, "vertices", LabelQuantity::Integer, static_cast<double>(v.size()));
        if (curve->closed && v.size() >= 3) {
            put(values, "area", LabelQuantity::Area, curve->area());
            put(values, "perimeter", LabelQuantity::Length, curve->length());
        }
    } else if (const auto* ellipse = std::get_if<katana::geometry::Ellipse2>(&geometry)) {
        put(values, "length", LabelQuantity::Length, ellipse->length());
        if (ellipse->isFull()) {
            put(values, "perimeter", LabelQuantity::Length, ellipse->length());
            put(values, "area", LabelQuantity::Area,
                katana::math::kPi * ellipse->majorRadius() * ellipse->minorRadius());
        }
        putLevel(values, heightsOf(target.properties, 1).front());
    } else if (const auto* spline = std::get_if<katana::geometry::Spline2>(&geometry)) {
        if (spline->checkStructure()) {
            put(values, "length", LabelQuantity::Length, spline->length());
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

// Every KIND.NAME of each surface and alignment `context` lists.
std::vector<std::string> everyContextValue(const LeaderContext& context)
{
    std::vector<std::string> names;
    for (const std::string& surface : context.surfaces) {
        for (const std::string_view kind : kSurfaceKinds) {
            names.push_back(std::string(kind) + "." + surface);
        }
    }
    for (const std::string& alignment : context.alignments) {
        for (const std::string_view kind : kAlignmentKinds) {
            names.push_back(std::string(kind) + "." + alignment);
        }
    }
    return names;
}

// A leader's target's values at its tip, with the context values `names`
// names (every one the context lists when null), and where they were read.
struct ReadValues {
    LabelValues values;
    Point2 at;
};
std::optional<ReadValues> readLeader(const Model& model, const LeaderGeometry& leader,
                                     std::span<const std::string> codeProperties,
                                     const LeaderContext& context,
                                     const std::vector<std::string>* names)
{
    if (!leader.tipRef.associated() || leader.vertices.empty()) {
        return std::nullopt;
    }
    const Entity* target = model.entities.find(leader.tipRef.entity);
    if (target == nullptr) {
        return std::nullopt;
    }
    ReadValues read;
    read.values =
        targetValues(*target, leader.tipRef, leader.vertices.front(), codeProperties, read.at);
    addContextValues(read.values, read.at, context,
                     names != nullptr ? *names : everyContextValue(context));
    return read;
}

} // namespace

LabelValues anchorValues(const Entity& target, const AnchorRef& ref, const Point2& tip,
                         std::span<const std::string> codeProperties, const LeaderContext& context)
{
    Point2 where;
    LabelValues values = targetValues(target, ref, tip, codeProperties, where);
    addContextValues(values, where, context, everyContextValue(context));
    return values;
}

void addContextValues(LabelValues& values, const Point2& at, const LeaderContext& context,
                      std::span<const std::string> names)
{
    // The target's own level, which a difference is taken from.
    std::optional<double> level;
    if (const auto found = values.find("z"); found != values.end()) {
        level = found->second.number;
    }
    for (const std::string& value : names) {
        const auto parsed = contextValueName(value);
        if (!parsed || values.contains(value)) {
            continue;
        }
        if (isSurfaceKind(parsed->kind)) {
            const auto ground =
                context.surfaceLevel ? context.surfaceLevel(parsed->name, at) : std::nullopt;
            if (!ground) {
                continue;
            }
            if (parsed->kind == "surface") {
                put(values, value, LabelQuantity::Number, *ground);
                continue;
            }
            if (!level) {
                continue;
            }
            // On the surface is a fill of nothing, and "on" is within the
            // geometric tolerance: the surface's level is interpolated, so a
            // point exactly on it differs from it in the last bits, and some
            // of them would read as a cut of 4e-15 (docs/annotation.md).
            const double above =
                katana::math::lengthsEqual(*level, *ground) ? 0.0 : *level - *ground;
            if (parsed->kind == "dsurface") {
                put(values, value, LabelQuantity::Number, above);
            } else if (parsed->kind == "cut" && above < 0.0) {
                put(values, value, LabelQuantity::Number, -above);
            } else if (parsed->kind == "fill" && above >= 0.0) {
                put(values, value, LabelQuantity::Number, above);
            }
            continue;
        }
        const auto place =
            context.alignmentPlace ? context.alignmentPlace(parsed->name, at) : std::nullopt;
        if (!place) {
            continue;
        }
        if (parsed->kind == "station") {
            put(values, value, LabelQuantity::Chainage, place->station);
        } else if (parsed->kind == "offset") {
            put(values, value, LabelQuantity::Length, place->offset);
        } else if (parsed->kind == "profile" && place->profile) {
            put(values, value, LabelQuantity::Number, *place->profile);
        } else if (parsed->kind == "dprofile" && place->profile && level) {
            put(values, value, LabelQuantity::Number, *level - *place->profile);
        }
    }
}

std::string whyAbsent(std::string_view name, const LabelValues& values, const Point2& at,
                      const LeaderContext& context)
{
    const auto parsed = contextValueName(name);
    if (!parsed || values.contains(name)) {
        return {};
    }
    const std::string what(parsed->name);
    const bool hasLevel = values.contains("z");
    if (isSurfaceKind(parsed->kind)) {
        if (!context.surfaceLevel) {
            return "no surfaces were given to read here";
        }
        // The matcher terrain::SurfaceStore::find uses, so a refusal never
        // says there is no "Ground" when the store finds "ground".
        if (std::none_of(context.surfaces.begin(), context.surfaces.end(),
                         [&](const std::string& surface) {
                             return katana::core::equalsIgnoringCase(surface, what);
                         })) {
            return "the drawing has no surface " + what;
        }
        if (!context.surfaceLevel(parsed->name, at)) {
            return "the tip is off surface " + what;
        }
        if (parsed->kind != "surface" && !hasLevel) {
            return "the target has no level";
        }
        if (parsed->kind == "cut") {
            return "the target is not below surface " + what;
        }
        if (parsed->kind == "fill") {
            return "the target is below surface " + what;
        }
        return {};
    }
    if (!context.alignmentPlace) {
        return "no alignments were given to read here";
    }
    if (std::find(context.alignments.begin(), context.alignments.end(), what) ==
        context.alignments.end()) {
        return "the drawing has no alignment " + what;
    }
    const auto place = context.alignmentPlace(parsed->name, at);
    if (!place) {
        return "the tip is beyond the ends of alignment " + what;
    }
    if ((parsed->kind == "profile" || parsed->kind == "dprofile") && !place->profile) {
        return "alignment " + what + " has no design profile at station " +
               formatFixed(place->station, 3);
    }
    if (parsed->kind == "dprofile" && !hasLevel) {
        return "the target has no level";
    }
    return {};
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
                                        std::span<const std::string> codeProperties,
                                        const LeaderContext& context)
{
    auto read = readLeader(model, leader, codeProperties, context, nullptr);
    if (!read) {
        return std::nullopt;
    }
    return std::move(read->values);
}

std::string leaderNote(const Model& model, const LeaderGeometry& leader,
                       std::span<const std::string> codeProperties, const LeaderContext& context)
{
    if (!isSmart(leader)) {
        return leader.text;
    }
    const auto templateText = leaderTemplate(model, leader);
    if (!templateText) {
        return {};
    }
    // Only what the template names is read of the context: a note is made
    // on every paint, and a drawing may hold many surfaces.
    const std::vector<std::string> names = templateFields(*templateText);
    const auto read = readLeader(model, leader, codeProperties, context, &names);
    return formatLabel(*templateText, read ? read->values : LabelValues{});
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
                                              std::span<const std::string> codeProperties,
                                              const LeaderContext& context)
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
    if (!leaderNote(model, leader, codeProperties, context).empty()) {
        return {};
    }
    const std::string templateText = leaderTemplate(model, leader).value_or(std::string());
    const std::vector<std::string> names = templateFields(templateText);
    const auto read = readLeader(model, leader, codeProperties, context, &names);
    const LabelValues values = read ? read->values : LabelValues{};
    std::string missing;
    for (const std::string& name : missingValues(templateText, values)) {
        // A value of the context says why it is not there: a tip off the
        // surface reads very like a misspelt surface otherwise.
        const std::string why = read ? whyAbsent(name, values, read->at, context) : std::string();
        const std::string said = why.empty() ? name : name + " (" + why + ")";
        missing += missing.empty() ? said : ", " + said;
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
