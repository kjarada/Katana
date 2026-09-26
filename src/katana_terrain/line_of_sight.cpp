// Line of sight between two points over the ground (line_of_sight.hpp).

#include "katana/terrain/line_of_sight.hpp"

#include <cmath>
#include <string>

namespace katana::terrain {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

namespace {

// More stations than this is a step typed in the wrong unit, not a sight
// line anyone needs: 10 million samples of a raster is minutes of work.
constexpr double kMaxStations = 1e7;

} // namespace

Result<SightLine> lineOfSight(const GroundAt& ground, const Point2& observer, const Point2& target,
                              const SightOptions& options, const std::stop_token& stop)
{
    if (!std::isfinite(options.observerHeight) || !std::isfinite(options.targetHeight)) {
        return makeError(ErrorCode::InvalidArgument, "the heights must be numbers");
    }
    if (!std::isfinite(options.curvature) || options.curvature < 0.0) {
        return makeError(ErrorCode::InvalidArgument, "curvature is a coefficient of 0 or more");
    }
    const double dx = target.x - observer.x;
    const double dy = target.y - observer.y;
    SightLine line;
    line.distance = std::hypot(dx, dy);
    if (!(line.distance > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "the observer and the target are one point");
    }
    if (!std::isfinite(options.step) || !(options.step > 0.0) ||
        line.distance / options.step > kMaxStations) {
        return makeError(ErrorCode::InvalidArgument,
                         "the step is a positive distance, no finer than a 10 millionth of the "
                         "sight line");
    }
    const auto dropAt = [&](double distance) {
        return options.curvature * distance * distance / kEarthDiameter;
    };
    const std::optional<double> atObserver = ground(observer);
    if (!atObserver) {
        return makeError(ErrorCode::InvalidArgument, "the observer is off the ground");
    }
    const std::optional<double> atTarget = ground(target);
    if (!atTarget) {
        return makeError(ErrorCode::InvalidArgument, "the target is off the ground");
    }
    line.observerZ = *atObserver + options.observerHeight;
    line.targetZ = *atTarget - dropAt(line.distance) + options.targetHeight;

    const auto count = static_cast<std::size_t>(std::ceil(line.distance / options.step));
    for (std::size_t i = 1; i < count; ++i) {
        if ((i & 0xFFFU) == 0 && stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        // Measured from the observer each time, never accumulated, so the
        // last station is as exact as the first.
        const double t = static_cast<double>(i) / static_cast<double>(count);
        const Point2 at(observer.x + dx * t, observer.y + dy * t);
        const double distance = line.distance * t;
        ++line.stations;
        const std::optional<double> height = ground(at);
        if (!height) {
            ++line.unknown;
            continue;
        }
        const double groundZ = *height - dropAt(distance);
        const double sight = line.observerZ + (line.targetZ - line.observerZ) * t;
        const double clearance = sight - groundZ;
        if (!line.clearance || clearance < *line.clearance) {
            line.clearance = clearance;
            line.clearanceAt = at;
            line.clearanceDistance = distance;
        }
        if (clearance < 0.0 && line.visible) {
            line.visible = false;
            line.blockedAt = at;
            line.blockedDistance = distance;
            line.blockedGround = groundZ;
        }
    }
    if (stop.stop_requested()) {
        return makeError(ErrorCode::InvalidState, "cancelled");
    }
    return line;
}

} // namespace katana::terrain
