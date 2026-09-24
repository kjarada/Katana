#include "everyday_support.hpp"

#include <algorithm>
#include <cmath>
#include <format>
#include <limits>
#include <variant>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/selection.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/angles.hpp"

namespace katana::cad::tools::everyday {

using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

// ---- words and numbers --------------------------------------------------------------

std::string fixed(double value, int decimals)
{
    std::string text = std::format("{:.{}f}", value, decimals);
    // -0.0004 prints as "-0.000", which reads as a sign that means something.
    if (text.front() == '-' && text.find_first_not_of("-0.") == std::string::npos) {
        text.erase(0, 1);
    }
    return text;
}

std::string degrees(double radians)
{
    // The survey formatter fails only for a non-finite angle, which nothing
    // here makes; if one ever came, the report says so in place rather than
    // printing a plausible angle (survey_tools.cpp does the same).
    const auto text = katana::survey::formatDms(radians, kSurveySecondsDecimals);
    return text ? *text : "(" + text.error().message + ")";
}

std::string bearing(double azimuth)
{
    const auto text = katana::survey::formatBearing(azimuth, kSurveySecondsDecimals);
    return text ? *text : "(" + text.error().message + ")";
}

double azimuthOf(const Vec2& direction)
{
    if (direction.x == 0.0 && direction.y == 0.0) {
        return 0.0;
    }
    // atan2(east, north): clockwise from north, as survey/angles.hpp measures.
    return katana::survey::normalizeAzimuth(std::atan2(direction.x, direction.y));
}

std::string coordinates(const Point2& point)
{
    return "E " + fixed(point.x) + " N " + fixed(point.y);
}

std::string lengthUnit(const Document& document)
{
    const std::string& unit = document.metadata().linearUnit;
    return isMetreUnit(unit) ? std::string("m") : unit;
}

std::string areaText(const Document& document, double area)
{
    // One hectare is 10 000 m^2 by definition (the SI Brochure, 9th edition,
    // table 8), and means nothing in a drawing in any other unit.
    constexpr double kSquareMetresPerHectare = 10'000.0;
    const std::string& unit = document.metadata().linearUnit;
    if (isMetreUnit(unit)) {
        return fixed(area) + " m\xC2\xB2 (" + fixed(area / kSquareMetresPerHectare, 4) + " ha)";
    }
    return fixed(area) + " square " + unit;
}

std::string counted(std::size_t count, std::string_view one, std::string_view many)
{
    return std::to_string(count) + " " + std::string(count == 1 ? one : many);
}

// ---- the drawing -------------------------------------------------------------------

SurveyPosition positionAt(const Document* document, const Point2& at, double tolerance)
{
    SurveyPosition bare;
    bare.point = at;
    if (document == nullptr) {
        return bare;
    }
    // A snap lands exactly on the point; the tolerance is for a pick the
    // view did not snap, and never less than a hair, so a typed coordinate
    // equal to a point's still finds it.
    const double radius = std::max(tolerance, katana::math::tolerance::kGeometric);
    std::vector<katana::geometry::SpatialId> near;
    document->spatialIndex().query(at, radius, near);
    std::optional<EntityId> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    for (const auto id : near) {
        const Entity* entity = document->model().entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        const auto* point = std::get_if<katana::entity::PointGeometry>(&entity->geometry);
        if (point == nullptr) {
            continue;
        }
        const double distance = point->position.distanceTo(at);
        if (distance <= radius &&
            (distance < bestDistance || (distance == bestDistance && best && id < *best))) {
            best = id;
            bestDistance = distance;
        }
    }
    if (!best) {
        return bare;
    }
    auto position = positionOfPoint(*document, *best);
    return position ? *position : bare;
}

ToolStep SelectionStep::pick(EntityId id)
{
    if (document_ == nullptr || !document_->model().entities.contains(id)) {
        return ToolStep::rejected("that entity is not in the drawing");
    }
    if (std::ranges::find(picked_, id) != picked_.end()) {
        return ToolStep::rejected("that entity is already selected");
    }
    history_.push_back(picked_);
    picked_.push_back(id);
    return ToolStep::next();
}

ToolStep SelectionStep::all()
{
    if (document_ == nullptr) {
        return ToolStep::rejected("there is no drawing to select from");
    }
    std::vector<EntityId> every;
    const katana::entity::Model& model = document_->model();
    // The document's rule alone: ToolContext carries no view, so a layer
    // hidden in one view only is not left out (as SelectionTool's All in
    // modify_transform.cpp, and outstanding there too).
    model.entities.forEach([&](const Entity& candidate) {
        if (isSelectable(model, candidate, kNoLayerOverrides)) {
            every.push_back(candidate.id);
        }
    });
    history_.push_back(picked_);
    picked_ = std::move(every);
    return ToolStep::next(counted(picked_.size(), "entity", "entities") + " selected");
}

bool SelectionStep::undo()
{
    if (history_.empty()) {
        return false;
    }
    picked_ = std::move(history_.back());
    history_.pop_back();
    return true;
}

std::vector<EntityId> SelectionStep::chosen() const
{
    std::vector<EntityId> out = picked_;
    if (document_ != nullptr) {
        for (const EntityId id : document_->selection().ids()) {
            out.push_back(id);
        }
        std::erase_if(out, [&](EntityId id) { return !document_->model().entities.contains(id); });
    }
    std::ranges::sort(out);
    out.erase(std::unique(out.begin(), out.end()), out.end());
    return out;
}

// ---- along an object ----------------------------------------------------------------

std::optional<Path> Path::of(const Geometry& geometry)
{
    Path path;
    path.geometry_ = geometry;
    if (const auto* line = std::get_if<Segment2>(&geometry)) {
        path.length_ = line->start.distanceTo(line->end);
    } else if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        path.length_ = arc->radius * std::abs(arc->sweep);
    } else if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        path.length_ = 2.0 * katana::math::kPi * circle->radius;
        path.closed_ = true;
    } else if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        const auto& v = polyline->vertices;
        if (v.size() < 2) {
            return std::nullopt;
        }
        path.closed_ = polyline->closed;
        path.stations_.push_back(0.0);
        const std::size_t count = polyline->closed ? v.size() + 1 : v.size();
        for (std::size_t i = 1; i < count; ++i) {
            path.stations_.push_back(path.stations_.back() +
                                     v[i - 1].distanceTo(v[i % v.size()]));
        }
        path.length_ = path.stations_.back();
    } else {
        return std::nullopt;
    }
    return path;
}

Point2 Path::pointAt(double station) const
{
    const double s = std::clamp(station, 0.0, length_);
    if (const auto* line = std::get_if<Segment2>(&geometry_)) {
        if (length_ <= 0.0) {
            return line->start;
        }
        return line->start + (line->end - line->start) * (s / length_);
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry_)) {
        return length_ <= 0.0 ? arc->startPoint() : arc->pointAt(s / length_);
    }
    if (const auto* circle = std::get_if<Circle2>(&geometry_)) {
        const double angle = circle->radius > 0.0 ? s / circle->radius : 0.0;
        return circle->center + Vec2(std::cos(angle), std::sin(angle)) * circle->radius;
    }
    const auto& v = std::get<Polyline2>(geometry_).vertices;
    // The segment holding s: the last station not after it.
    const auto upper = std::ranges::upper_bound(stations_, s);
    std::size_t i = static_cast<std::size_t>(upper - stations_.begin());
    i = std::clamp<std::size_t>(i, 1, stations_.size() - 1);
    const Point2& a = v[(i - 1) % v.size()];
    const Point2& b = v[i % v.size()];
    const double span = stations_[i] - stations_[i - 1];
    if (span <= 0.0) {
        return a;
    }
    return a + (b - a) * ((s - stations_[i - 1]) / span);
}

} // namespace katana::cad::tools::everyday
