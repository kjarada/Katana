#include "katana/cad/drawing/drafting.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <limits>
#include <string>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;
using katana::geometry::Vec2;
using katana::math::kDegToRad;
using katana::math::kRadToDeg;

namespace {

constexpr std::string_view kDegreeSign = "\xC2\xB0"; // U+00B0 in UTF-8

katana::core::Error notAnAngle(std::string_view text)
{
    return katana::core::Error{
        ErrorCode::ParseFailure,
        "'" + std::string(text) +
            "' is not an angle; type degrees as 45.5 or 45d30'15\", or a bearing as N45d30'15\"E",
        {}};
}

// The text with the degree sign spelt 'd', so one scanner reads both.
std::string normalisedAngle(std::string_view text)
{
    std::string out;
    for (std::size_t i = 0; i < text.size(); ++i) {
        if (text.substr(i, kDegreeSign.size()) == kDegreeSign) {
            out += 'd';
            i += kDegreeSign.size() - 1;
        } else if (text[i] == 'D') {
            out += 'd';
        } else if (text[i] != ' ') {
            out += text[i];
        }
    }
    return out;
}

} // namespace

// ---- angles ------------------------------------------------------------------------------

Result<double> parseDegrees(std::string_view text)
{
    const std::string_view trimmed = katana::core::trimmed(text);
    const std::string s = normalisedAngle(trimmed);
    if (s.empty()) {
        return notAnAngle(text);
    }
    if (s.find_first_of("d'\"") == std::string::npos) {
        const auto value = katana::core::parseFiniteDouble(s);
        if (!value) {
            return notAnAngle(text);
        }
        return *value;
    }
    double sign = 1.0;
    std::string_view rest = s;
    if (rest.front() == '-' || rest.front() == '+') {
        sign = rest.front() == '-' ? -1.0 : 1.0;
        rest.remove_prefix(1);
    }
    double parts[3] = {0.0, 0.0, 0.0};
    const char marks[3] = {'d', '\'', '"'};
    int next = 0;
    while (!rest.empty()) {
        const auto at = rest.find_first_of("d'\"");
        const std::string_view number = rest.substr(0, at);
        int which = -1;
        if (at == std::string_view::npos) {
            // A trailing number with no mark takes the next unit.
            which = next;
        } else {
            which = static_cast<int>(std::find(marks, marks + 3, rest[at]) - marks);
        }
        if (which < next || which > 2) {
            return notAnAngle(text);
        }
        const auto value = katana::core::parseFiniteDouble(number);
        if (!value || *value < 0.0) {
            return notAnAngle(text);
        }
        parts[which] = *value;
        next = which + 1;
        rest = at == std::string_view::npos ? std::string_view{} : rest.substr(at + 1);
    }
    if (parts[1] >= 60.0 || parts[2] >= 60.0) {
        return katana::core::Error{ErrorCode::ParseFailure,
                                   "'" + std::string(text) +
                                       "' has 60 or more minutes or seconds",
                                   {}};
    }
    return sign * (parts[0] + parts[1] / 60.0 + parts[2] / 3600.0);
}

Result<double> parseDirection(std::string_view text, AngleConvention convention)
{
    const std::string_view trimmed = katana::core::trimmed(text);
    if (trimmed.size() >= 3) {
        const char first = katana::core::asciiLower(trimmed.front());
        const char last = katana::core::asciiLower(trimmed.back());
        if ((first == 'n' || first == 's') && (last == 'e' || last == 'w')) {
            const auto inside = parseDegrees(trimmed.substr(1, trimmed.size() - 2));
            if (!inside || *inside < 0.0 || *inside > 90.0) {
                return katana::core::Error{ErrorCode::ParseFailure,
                                           "'" + std::string(text) +
                                               "' is not a quadrant bearing; the angle from "
                                               "north or south is 0 to 90 degrees",
                                           {}};
            }
            double bearing = *inside;
            if (first == 's' && last == 'e') {
                bearing = 180.0 - *inside;
            } else if (first == 's' && last == 'w') {
                bearing = 180.0 + *inside;
            } else if (first == 'n' && last == 'w') {
                bearing = 360.0 - *inside;
            }
            return katana::math::normalizeAngleSigned((90.0 - bearing) * kDegToRad);
        }
    }
    const auto degrees = parseDegrees(trimmed);
    if (!degrees) {
        return degrees.error();
    }
    // Signed, (-pi, pi]: one number for one direction, however it was typed.
    return katana::math::normalizeAngleSigned(
        (convention == AngleConvention::Bearing ? 90.0 - *degrees : *degrees) * kDegToRad);
}

std::string formatDms(double degrees, int decimals)
{
    decimals = std::clamp(decimals, 0, 6);
    const double scale = std::pow(10.0, decimals);
    const bool negative = degrees < 0.0;
    // In whole units of the last place shown, so rounding carries into the
    // minutes and degrees (59.9995" at 3 places is the next minute).
    const double units = std::round(std::abs(degrees) * 3600.0 * scale);
    const double perMinute = 60.0 * scale;
    const double perDegree = 3600.0 * scale;
    const double d = std::floor(units / perDegree);
    const double m = std::floor((units - d * perDegree) / perMinute);
    const double s = (units - d * perDegree - m * perMinute) / scale;
    char buffer[64];
    if (decimals == 0) {
        std::snprintf(buffer, sizeof(buffer), "%s%.0f%s%02.0f'%02.0f\"", negative ? "-" : "", d,
                      std::string(kDegreeSign).c_str(), m, s);
    } else {
        std::snprintf(buffer, sizeof(buffer), "%s%.0f%s%02.0f'%0*.*f\"", negative ? "-" : "", d,
                      std::string(kDegreeSign).c_str(), m, 3 + decimals, decimals, s);
    }
    return buffer;
}

std::string formatBearing(double radians, int decimals)
{
    double bearing = std::fmod(90.0 - radians * kRadToDeg, 360.0);
    if (bearing < 0.0) {
        bearing += 360.0;
    }
    std::string text = formatDms(bearing, decimals);
    // A bearing that rounds up to 360 is north.
    if (text.rfind("360", 0) == 0) {
        text = formatDms(0.0, decimals);
    }
    return text;
}

std::string formatQuadrantBearing(double radians, int decimals)
{
    double bearing = std::fmod(90.0 - radians * kRadToDeg, 360.0);
    if (bearing < 0.0) {
        bearing += 360.0;
    }
    if (bearing <= 90.0) {
        return "N" + formatDms(bearing, decimals) + "E";
    }
    if (bearing <= 180.0) {
        return "S" + formatDms(180.0 - bearing, decimals) + "E";
    }
    if (bearing <= 270.0) {
        return "S" + formatDms(bearing - 180.0, decimals) + "W";
    }
    return "N" + formatDms(360.0 - bearing, decimals) + "W";
}

// ---- typed points -----------------------------------------------------------------------------

bool looksLikePoint(std::string_view text)
{
    const std::string_view input = katana::core::trimmed(text);
    return input.find(',') != std::string_view::npos || (!input.empty() && input.front() == '@');
}

Result<PrecisePoint> parsePrecisePoint(std::string_view text, std::optional<Point2> last,
                                       const DraftingSettings& settings)
{
    const std::string_view input = katana::core::trimmed(text);
    const bool relative = !input.empty() && input.front() == '@';
    const std::string_view body = relative ? input.substr(1) : input;
    if (relative && !last) {
        return makeError(ErrorCode::InvalidState, "a relative point needs a previous point",
                         std::string(text));
    }
    PrecisePoint out;
    if (const auto polar = body.find('<'); polar != std::string_view::npos) {
        if (!relative) {
            return makeError(ErrorCode::ParseFailure, "polar points are relative: @distance<angle",
                             std::string(text));
        }
        const auto distance =
            katana::core::parseFiniteDouble(katana::core::trimmed(body.substr(0, polar)));
        if (!distance) {
            return makeError(ErrorCode::ParseFailure, "expected @distance<angle",
                             std::string(text));
        }
        const auto direction = parseDirection(body.substr(polar + 1), settings.angles);
        if (!direction) {
            return direction.error();
        }
        out.point = *last + Vec2(std::cos(*direction), std::sin(*direction)) * *distance;
        return out;
    }
    double values[3] = {0.0, 0.0, 0.0};
    std::size_t count = 0;
    std::string_view rest = body;
    while (true) {
        const auto comma = rest.find(',');
        const std::string_view token = katana::core::trimmed(rest.substr(0, comma));
        const auto value = katana::core::parseFiniteDouble(token);
        if (!value || count == 3) {
            return makeError(ErrorCode::ParseFailure, "expected a point as x,y or x,y,z",
                             std::string(text));
        }
        values[count++] = *value;
        if (comma == std::string_view::npos) {
            break;
        }
        rest = rest.substr(comma + 1);
    }
    if (count < 2) {
        return makeError(ErrorCode::ParseFailure, "expected a point as x,y or x,y,z",
                         std::string(text));
    }
    const Vec2 value(values[0], values[1]);
    out.point = relative ? *last + value : Point2(value);
    if (count == 3) {
        out.z = values[2];
    }
    return out;
}

Point2 directDistance(const Point2& base, const Point2& cursor, double distance,
                      const DraftingSettings& settings)
{
    Vec2 direction(1.0, 0.0);
    if (settings.angleLock) {
        direction = Vec2(std::cos(*settings.angleLock), std::sin(*settings.angleLock));
    } else if (cursor.distanceTo(base) > katana::math::tolerance::kGeometric) {
        direction = (cursor - base).normalized();
    }
    return base + direction * distance;
}

// ---- constraining the cursor -----------------------------------------------------------------

ConstrainedPoint constrain(std::optional<Point2> base, const Point2& cursor,
                           const DraftingSettings& settings, double aperture)
{
    ConstrainedPoint out{cursor, {}, std::nullopt};
    if (!base) {
        return out;
    }
    const Vec2 offset = cursor - *base;
    std::string label;
    if (settings.angleLock) {
        const Vec2 u(std::cos(*settings.angleLock), std::sin(*settings.angleLock));
        out.point = *base + u * offset.dot(u);
        label = "Angle lock " + formatDms(*settings.angleLock * kRadToDeg);
        out.pathFrom = *base;
    } else if (settings.ortho) {
        out.point = std::abs(offset.x) >= std::abs(offset.y) ? Point2(cursor.x, base->y)
                                                             : Point2(base->x, cursor.y);
        label = "Ortho";
    } else if (settings.polar && settings.polarIncrement > 0.0 && offset.length() > 0.0) {
        const double angle = offset.angle();
        const double snapped =
            std::round(angle / settings.polarIncrement) * settings.polarIncrement;
        const Vec2 u(std::cos(snapped), std::sin(snapped));
        const double along = offset.dot(u);
        const Point2 foot = *base + u * along;
        if (along > 0.0 && foot.distanceTo(cursor) <= aperture) {
            out.point = foot;
            double degrees = std::fmod(snapped * kRadToDeg, 360.0);
            if (degrees < 0.0) {
                degrees += 360.0;
            }
            label = "Polar " + formatDms(degrees);
            out.pathFrom = *base;
        }
    }
    if (settings.lengthLock) {
        Vec2 direction = out.point - *base;
        if (!(direction.length() > katana::math::tolerance::kGeometric)) {
            direction = settings.angleLock
                            ? Vec2(std::cos(*settings.angleLock), std::sin(*settings.angleLock))
                            : Vec2(1.0, 0.0);
        }
        out.point = *base + direction.normalized() * *settings.lengthLock;
        label += (label.empty() ? "" : ", ") + std::string("Length lock ") +
                 katana::core::formatExactReal(*settings.lengthLock);
    }
    out.label = std::move(label);
    return out;
}

std::optional<ConstrainedPoint> trackAcquired(const std::vector<Point2>& acquired,
                                              const Point2& cursor, double aperture)
{
    std::optional<ConstrainedPoint> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    bool bestIsCrossing = false;
    const auto offer = [&](const Point2& p, const Point2& from, bool crossing) {
        const double d = p.distanceTo(cursor);
        if (d > aperture) {
            return;
        }
        // A crossing of two paths is the more particular answer.
        if ((crossing && !bestIsCrossing) || (crossing == bestIsCrossing && d < bestDistance)) {
            best = ConstrainedPoint{p, crossing ? "Tracking crossing" : "Tracking", from};
            bestDistance = d;
            bestIsCrossing = crossing;
        }
    };
    for (std::size_t i = 0; i < acquired.size(); ++i) {
        const Point2& a = acquired[i];
        if (a.distanceTo(cursor) <= aperture) {
            continue; // on the acquired point itself: the snap's, not a path's
        }
        offer(Point2(cursor.x, a.y), a, false);
        offer(Point2(a.x, cursor.y), a, false);
        for (std::size_t j = 0; j < acquired.size(); ++j) {
            if (j != i) {
                offer(Point2(a.x, acquired[j].y), a, true);
            }
        }
    }
    return best;
}

bool TrackingPoints::tracks(SnapMode mode)
{
    switch (mode) {
    case SnapMode::Endpoint:
    case SnapMode::Midpoint:
    case SnapMode::Center:
    case SnapMode::Intersection:
    case SnapMode::Quadrant:
    case SnapMode::Node:
    case SnapMode::ApparentIntersection:
        return true;
    default:
        return false;
    }
}

void TrackingPoints::acquire(const Point2& point, double tolerance)
{
    std::erase_if(points_, [&](const Point2& p) { return p.distanceTo(point) <= tolerance; });
    points_.insert(points_.begin(), point);
    if (points_.size() > kMost) {
        points_.resize(kMost);
    }
}

// ---- one-shot snaps ------------------------------------------------------------------------------

Result<Point2> fromBase(const Point2& base, std::string_view offset,
                        const DraftingSettings& settings)
{
    std::string text(katana::core::trimmed(offset));
    if (text.empty() || text.front() != '@') {
        text.insert(text.begin(), '@'); // an offset is relative to the base
    }
    auto point = parsePrecisePoint(text, base, settings);
    if (!point) {
        return point.error();
    }
    return point->point;
}

Point2 midBetween(const Point2& a, const Point2& b) { return (a + b) * 0.5; }

} // namespace katana::cad
