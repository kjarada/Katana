#include "katana/entity/label_values.hpp"

#include <algorithm>
#include <cmath>

#include "katana/entity/anchor.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/math/numerics.hpp"
#include "value_parts.hpp"

namespace katana::entity {

using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
namespace tol = katana::math::tolerance;

namespace detail {

double bearingOf(const Vec2& along)
{
    return katana::math::normalizeAngle(std::atan2(along.x, along.y));
}

void addCommonValues(const Entity& entity, std::span<const std::string> codeProperties,
                     LabelValues& values)
{
    values.insert_or_assign("id", LabelValue::of(LabelQuantity::Integer,
                                                 static_cast<double>(entity.id)));
    values.insert_or_assign("layer", LabelValue::ofText(entity.layer));
    // The first code property the entity carries, as text.
    for (const std::string& key : codeProperties) {
        const PropertyValue* found = nullptr;
        for (const PropertyMap* where : {&entity.properties, &entity.metadata}) {
            if (const auto at = where->find(key); found == nullptr && at != where->end()) {
                found = &at->second;
            }
        }
        if (found != nullptr) {
            values.insert_or_assign("code", LabelValue::ofText(toString(*found)));
            break;
        }
    }
    for (const auto& [key, value] : entity.properties) {
        LabelValue converted;
        if (const auto* text = std::get_if<std::string>(&value)) {
            converted = LabelValue::ofText(*text);
        } else if (const auto* real = std::get_if<double>(&value)) {
            converted = LabelValue::of(LabelQuantity::Number, *real);
        } else if (const auto* integer = std::get_if<std::int64_t>(&value)) {
            converted = LabelValue::of(LabelQuantity::Integer, static_cast<double>(*integer));
        } else {
            converted = LabelValue::ofText(toString(value));
        }
        values.insert_or_assign("prop." + key, converted);
        // The two properties survey import writes that a label names
        // directly (cad/survey_import.hpp's defaults), as text whatever
        // their stored type - point 1001 is a name, not a quantity.
        if (key == "point" || key == "description") {
            values.insert_or_assign(key, LabelValue::ofText(toString(value)));
        }
    }
}

void addSegmentValues(LabelValues& values, const Point2& from, const Point2& to, std::size_t index,
                      std::optional<double> zFrom, std::optional<double> zTo)
{
    const Vec2 along = to - from;
    const double length = along.length();
    // A piece of no length has no direction: its bearing is absent rather
    // than the 0 atan2 would give it.
    if (length > tol::kGeometric) {
        values.insert_or_assign("bearing",
                                LabelValue::of(LabelQuantity::Bearing, bearingOf(along)));
    }
    values.insert_or_assign("distance", LabelValue::of(LabelQuantity::Length, length));
    values.insert_or_assign("length", LabelValue::of(LabelQuantity::Length, length));
    values.insert_or_assign("dx", LabelValue::of(LabelQuantity::Length, along.x));
    values.insert_or_assign("dy", LabelValue::of(LabelQuantity::Length, along.y));
    values.insert_or_assign("segment",
                            LabelValue::of(LabelQuantity::Integer, static_cast<double>(index + 1)));
    if (zFrom && zTo) {
        const double dz = *zTo - *zFrom;
        values.insert_or_assign("dz", LabelValue::of(LabelQuantity::Number, dz));
        if (length > tol::kGeometric) {
            // Per cent, as a road or a drain is graded.
            values.insert_or_assign("grade",
                                    LabelValue::of(LabelQuantity::Number, 100.0 * dz / length));
        }
    }
}

void addArcValues(LabelValues& values, const Arc2& arc)
{
    const double delta = std::abs(arc.sweep);
    values.insert_or_assign("radius", LabelValue::of(LabelQuantity::Length, arc.radius));
    values.insert_or_assign("length", LabelValue::of(LabelQuantity::Length, arc.length()));
    values.insert_or_assign("delta", LabelValue::of(LabelQuantity::Angle, delta));
    const bool fullTurn = delta >= katana::math::kTwoPi - tol::kAngular;
    if (!fullTurn) {
        const Vec2 chord = arc.pointAt(1.0) - arc.pointAt(0.0);
        values.insert_or_assign("chord", LabelValue::of(LabelQuantity::Length, chord.length()));
        values.insert_or_assign("bearing",
                                LabelValue::of(LabelQuantity::Bearing, bearingOf(chord)));
        // The tangent length R tan(delta / 2): from each end to where the
        // tangents meet. Past half a turn they meet behind the curve and the
        // figure is not what anyone calls a tangent length, so it is absent.
        if (delta < katana::math::kPi - tol::kAngular) {
            values.insert_or_assign("tangent", LabelValue::of(LabelQuantity::Length,
                                                              arc.radius * std::tan(0.5 * delta)));
        }
    }
}

} // namespace detail

namespace {

using detail::addCommonValues;

LabelPiece pointPiece(const Entity& entity, const Point2& position, std::span<const std::string> codes)
{
    LabelPiece piece;
    piece.shape = LabelPiece::Shape::Point;
    piece.anchor = position;
    addCommonValues(entity, codes, piece.values);
    for (const auto& [name, value] :
         {std::pair{"x", position.x}, std::pair{"easting", position.x}, std::pair{"y", position.y},
          std::pair{"northing", position.y}}) {
        piece.values.insert_or_assign(name, LabelValue::of(LabelQuantity::Length, value));
    }
    if (const auto z = heightsOf(entity.properties, 1).front()) {
        piece.values.insert_or_assign("z", LabelValue::of(LabelQuantity::Number, *z));
        piece.values.insert_or_assign("rl", LabelValue::of(LabelQuantity::Number, *z));
    }
    return piece;
}

LabelPiece segmentPiece(const Entity& entity, const Point2& from, const Point2& to,
                        std::size_t index, std::optional<double> zFrom, std::optional<double> zTo,
                        std::span<const std::string> codes)
{
    LabelPiece piece;
    piece.shape = LabelPiece::Shape::Segment;
    piece.from = from;
    piece.to = to;
    piece.anchor = from + (to - from) * 0.5;
    const Vec2 along = to - from;
    piece.direction = along.angle();
    piece.length = along.length();
    addCommonValues(entity, codes, piece.values);
    detail::addSegmentValues(piece.values, from, to, index, zFrom, zTo);
    return piece;
}

LabelPiece arcPiece(const Entity& entity, const Arc2& arc, std::span<const std::string> codes)
{
    LabelPiece piece;
    piece.shape = LabelPiece::Shape::Arc;
    piece.arc = arc;
    piece.anchor = arc.pointAt(0.5);
    const Vec2 radial = piece.anchor - arc.center;
    // The tangent in the direction of sweep.
    piece.direction = (arc.sweep >= 0.0 ? radial.perpendicular() : -radial.perpendicular()).angle();
    piece.length = arc.length();
    piece.from = arc.pointAt(0.0);
    piece.to = arc.pointAt(1.0);
    addCommonValues(entity, codes, piece.values);
    detail::addArcValues(piece.values, arc);
    return piece;
}

LabelPiece areaPiece(const Entity& entity, const Point2& inside, double area, double perimeter,
                     std::span<const std::string> codes)
{
    LabelPiece piece;
    piece.shape = LabelPiece::Shape::Area;
    piece.anchor = inside;
    addCommonValues(entity, codes, piece.values);
    piece.values.insert_or_assign("area", LabelValue::of(LabelQuantity::Area, area));
    piece.values.insert_or_assign("perimeter", LabelValue::of(LabelQuantity::Length, perimeter));
    piece.values.insert_or_assign("x", LabelValue::of(LabelQuantity::Length, inside.x));
    piece.values.insert_or_assign("y", LabelValue::of(LabelQuantity::Length, inside.y));
    return piece;
}

// A chainage label's pieces: a mark at every tick and every label station
// from the first multiple of the tick interval at or after the start, through
// the end; the label stations carry text.
std::vector<LabelPiece> chainagePieces(const Alignment& alignment, const LabelStyle& style)
{
    std::vector<LabelPiece> pieces;
    const auto solved = katana::geometry::solveAlignment(alignment.horizontal);
    if (!solved) {
        return pieces;
    }
    const double start = solved->startStation();
    const double end = solved->endStation();
    const double interval = style.interval;
    const double step = style.tickInterval > tol::kGeometric ? std::min(style.tickInterval, interval)
                                                             : interval;
    // Stations are counted as whole multiples of the step, never accumulated,
    // so the 500th tick of a 10 km road is exactly at 5000 and not wherever
    // 500 additions of 10.0 land. A station that is a label station within a
    // part in a million of the interval is one - 10 x 0.1 is not exactly 1.
    constexpr std::size_t kMaximumMarks = 100000;
    const double firstIndex = std::ceil(start / step - 1e-9);
    for (double k = firstIndex;; k += 1.0) {
        const double station = k * step;
        if (station > end + tol::kGeometric || pieces.size() >= kMaximumMarks) {
            break;
        }
        const auto point = solved->pointAtStation(std::clamp(station, start, end));
        const auto direction = solved->directionAtStation(std::clamp(station, start, end));
        if (!point || !direction) {
            continue;
        }
        LabelPiece piece;
        piece.shape = LabelPiece::Shape::Station;
        piece.anchor = *point;
        piece.direction = *direction;
        const double ratio = station / interval;
        piece.tickOnly = std::abs(ratio - std::round(ratio)) > 1e-6;
        piece.values.insert_or_assign("chainage", LabelValue::of(LabelQuantity::Chainage, station));
        piece.values.insert_or_assign("x", LabelValue::of(LabelQuantity::Length, point->x));
        piece.values.insert_or_assign("y", LabelValue::of(LabelQuantity::Length, point->y));
        piece.values.insert_or_assign("alignment", LabelValue::ofText(alignment.name));
        pieces.push_back(std::move(piece));
    }
    return pieces;
}

// The segments of a curve polyline a label of `kind` labels: every one for
// a segment label (an arc segment as an arc, a straight one as a segment),
// the arc segments for an arc label; only `part` when it names one.
std::vector<std::size_t> curveSegmentsLabelled(const katana::geometry::CurvePolyline2& curve,
                                               LabelKind kind, std::int32_t part)
{
    std::vector<std::size_t> out;
    for (std::size_t i = 0; i < curve.segmentCount(); ++i) {
        if ((part >= 0 && static_cast<std::size_t>(part) != i) ||
            (kind == LabelKind::Arc && !curve.isArc(i)) ||
            curve.segmentLength(i) <= tol::kGeometric) {
            continue;
        }
        out.push_back(i);
    }
    return out;
}

} // namespace

bool labels(LabelKind kind, const Geometry& geometry)
{
    // A curve polyline (docs/drawing.md) is labelled as a polyline is - its
    // arc segments as arcs - so a lot drawn with a curved frontage, or read
    // from DXF with bulges, labels as a straight one did.
    const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&geometry);
    switch (kind) {
    case LabelKind::Point:
        return std::holds_alternative<PointGeometry>(geometry);
    case LabelKind::Segment:
        return std::holds_alternative<Segment2>(geometry) ||
               std::holds_alternative<Polyline2>(geometry) ||
               (curve != nullptr && curve->segmentCount() > 0);
    case LabelKind::Arc:
        return std::holds_alternative<Arc2>(geometry) || std::holds_alternative<Circle2>(geometry) ||
               (curve != nullptr && curve->hasArcs());
    case LabelKind::Area:
        if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
            return polyline->closed && polyline->vertices.size() >= 3;
        }
        if (curve != nullptr) {
            return curve->closed && curve->vertices.size() >= 3;
        }
        return std::holds_alternative<Circle2>(geometry);
    case LabelKind::Chainage:
        return false;
    }
    return false;
}

std::vector<LabelPiece> labelPiecesFor(const Model& model, const Entity& target, std::int32_t part,
                                       const LabelStyle& style,
                                       std::span<const std::string> codes)
{
    (void)model; // the entity is all a piece needs; the model is for a chainage's alignment
    std::vector<LabelPiece> pieces;
    if (!labels(style.kind, target.geometry)) {
        return pieces;
    }
    switch (style.kind) {
    case LabelKind::Point:
        pieces.push_back(
            pointPiece(target, std::get<PointGeometry>(target.geometry).position, codes));
        break;
    case LabelKind::Segment:
    case LabelKind::Arc:
        if (const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&target.geometry)) {
            const auto& v = curve->vertices;
            for (const std::size_t i : curveSegmentsLabelled(*curve, style.kind, part)) {
                const std::size_t j = curve->segmentEnd(i);
                const auto piece = curve->segment(i);
                if (const auto* arc = std::get_if<Arc2>(&piece)) {
                    pieces.push_back(arcPiece(target, *arc, codes));
                } else {
                    pieces.push_back(segmentPiece(target, v[i].position, v[j].position, i,
                                                  v[i].height, v[j].height, codes));
                }
            }
            break;
        }
        if (style.kind == LabelKind::Arc) {
            if (const auto* arc = std::get_if<Arc2>(&target.geometry)) {
                pieces.push_back(arcPiece(target, *arc, codes));
            } else {
                const auto& circle = std::get<Circle2>(target.geometry);
                // A circle is labelled at its top, as a full-turn arc from north.
                pieces.push_back(arcPiece(
                    target, Arc2{circle.center, circle.radius, -0.5 * katana::math::kPi,
                                 katana::math::kTwoPi},
                    codes));
            }
            break;
        }
        if (const auto* line = std::get_if<Segment2>(&target.geometry)) {
            const auto z = heightsOf(target.properties, 2);
            pieces.push_back(segmentPiece(target, line->start, line->end, 0, z[0], z[1], codes));
        } else {
            const auto& polyline = std::get<Polyline2>(target.geometry);
            const auto& v = polyline.vertices;
            const std::size_t segments = polyline.closed ? v.size() : v.size() - 1;
            const auto z = heightsOf(target.properties, v.size());
            for (std::size_t i = 0; i < segments; ++i) {
                if (part >= 0 && static_cast<std::size_t>(part) != i) {
                    continue;
                }
                const std::size_t j = (i + 1) % v.size();
                if (v[i].distanceTo(v[j]) <= tol::kGeometric) {
                    continue; // a repeated vertex has no direction to label
                }
                pieces.push_back(segmentPiece(target, v[i], v[j], i, z[i], z[j], codes));
            }
        }
        break;
    case LabelKind::Area:
        if (const auto* polyline = std::get_if<Polyline2>(&target.geometry)) {
            pieces.push_back(areaPiece(target, insidePoint(*polyline), polyline->area(),
                                       polyline->length(), codes));
        } else if (const auto* curve =
                       std::get_if<katana::geometry::CurvePolyline2>(&target.geometry)) {
            // The area and perimeter exact, arcs included; only where the
            // label stands comes from the chords.
            pieces.push_back(areaPiece(
                target, insidePoint(curve->toPolyline(katana::geometry::kCurveChordTolerance)),
                curve->area(), curve->length(), codes));
        } else {
            const auto& circle = std::get<Circle2>(target.geometry);
            pieces.push_back(
                areaPiece(target, circle.center, circle.area(), circle.perimeter(), codes));
        }
        break;
    case LabelKind::Chainage:
        break;
    }
    return pieces;
}

std::vector<LabelPiece> labelPieces(const Model& model, const LabelGeometry& label,
                                    const LabelStyle& style, std::span<const std::string> codes)
{
    if (style.kind == LabelKind::Chainage) {
        if (label.alignment.empty()) {
            return {};
        }
        const Alignment* alignment = model.alignments.find(label.alignment);
        return alignment == nullptr ? std::vector<LabelPiece>{} : chainagePieces(*alignment, style);
    }
    const Entity* target = model.entities.find(label.target);
    if (target == nullptr) {
        return {};
    }
    return labelPiecesFor(model, *target, label.part, style, codes);
}

std::optional<Point2> labelAnchor(const Model& model, const LabelGeometry& label,
                                  const LabelStyle& style)
{
    // Worked out directly rather than from labelPieces: the associative
    // update asks this of every label after every command, and a piece
    // carries a map of values it does not need.
    if (style.kind == LabelKind::Chainage) {
        const Alignment* alignment = model.alignments.find(label.alignment);
        if (alignment == nullptr || alignment->horizontal.pis.empty()) {
            return std::nullopt;
        }
        return alignment->horizontal.pis.front().point;
    }
    const Entity* target = model.entities.find(label.target);
    if (target == nullptr || !labels(style.kind, target->geometry)) {
        return std::nullopt;
    }
    const Geometry& geometry = target->geometry;
    if (const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&geometry)) {
        if (style.kind == LabelKind::Area) {
            return insidePoint(curve->toPolyline(katana::geometry::kCurveChordTolerance));
        }
        const auto segments = curveSegmentsLabelled(*curve, style.kind, label.part);
        if (segments.empty()) {
            return std::nullopt;
        }
        return std::visit([](const auto& piece) { return piece.pointAt(0.5); },
                          curve->segment(segments.front()));
    }
    switch (style.kind) {
    case LabelKind::Point:
        return std::get<PointGeometry>(geometry).position;
    case LabelKind::Segment:
        if (const auto* line = std::get_if<Segment2>(&geometry)) {
            return line->start + (line->end - line->start) * 0.5;
        } else {
            const auto& polyline = std::get<Polyline2>(geometry);
            const auto& v = polyline.vertices;
            const std::size_t segments = polyline.closed ? v.size() : v.size() - 1;
            const std::size_t i = label.part >= 0 ? static_cast<std::size_t>(label.part) : 0;
            if (i >= segments) {
                return std::nullopt;
            }
            return v[i] + (v[(i + 1) % v.size()] - v[i]) * 0.5;
        }
    case LabelKind::Arc:
        if (const auto* arc = std::get_if<Arc2>(&geometry)) {
            return arc->pointAt(0.5);
        } else {
            const auto& circle = std::get<Circle2>(geometry);
            return circle.center + Vec2(0.0, circle.radius);
        }
    case LabelKind::Area:
        if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
            return insidePoint(*polyline);
        }
        return std::get<Circle2>(geometry).center;
    case LabelKind::Chainage:
        break;
    }
    return std::nullopt;
}

} // namespace katana::entity
