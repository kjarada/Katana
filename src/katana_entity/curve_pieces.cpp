#include "katana/entity/curve_pieces.hpp"

#include <variant>

#include "katana/geometry/chording.hpp"

namespace katana::entity {

using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Curve2;
using katana::geometry::CurvePolyline2;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Spline2;

namespace {

void appendChords(const std::vector<Point2>& chain, std::vector<Curve2>& out)
{
    for (std::size_t i = 1; i < chain.size(); ++i) {
        if (chain[i - 1].distanceTo(chain[i]) > katana::math::tolerance::kGeometric) {
            out.emplace_back(Segment2{chain[i - 1], chain[i]});
        }
    }
}

} // namespace

std::vector<Curve2> curvePieces(const Geometry& geometry, double tolerance)
{
    std::vector<Curve2> out;
    if (const auto* segment = std::get_if<Segment2>(&geometry)) {
        out.emplace_back(*segment);
    } else if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        out.emplace_back(*arc);
    } else if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        out.emplace_back(*circle);
    } else if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
            out.emplace_back(polyline->segment(i));
        }
    } else if (const auto* curved = std::get_if<CurvePolyline2>(&geometry)) {
        for (std::size_t i = 0; i < curved->segmentCount(); ++i) {
            std::visit([&out](const auto& piece) { out.emplace_back(piece); }, curved->segment(i));
        }
    } else if (const auto* ellipse = std::get_if<Ellipse2>(&geometry)) {
        appendChords(ellipse->tessellate(tolerance), out);
    } else if (const auto* spline = std::get_if<Spline2>(&geometry)) {
        appendChords(spline->tessellate(tolerance), out);
    }
    return out;
}

bool piecesAreExact(const Geometry& geometry)
{
    return !std::holds_alternative<Ellipse2>(geometry) &&
           !std::holds_alternative<Spline2>(geometry);
}

std::vector<Point2> linework(const Geometry& geometry, double tolerance)
{
    if (const auto* segment = std::get_if<Segment2>(&geometry)) {
        return {segment->start, segment->end};
    }
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        std::vector<Point2> out = polyline->vertices;
        if (polyline->closed && !out.empty()) {
            out.push_back(out.front());
        }
        return out;
    }
    if (const auto* curved = std::get_if<CurvePolyline2>(&geometry)) {
        return curved->tessellate(tolerance);
    }
    if (const auto* ellipse = std::get_if<Ellipse2>(&geometry)) {
        return ellipse->tessellate(tolerance);
    }
    if (const auto* spline = std::get_if<Spline2>(&geometry)) {
        return spline->tessellate(tolerance);
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        return katana::geometry::chordArc(*arc, tolerance);
    }
    return {};
}

} // namespace katana::entity
