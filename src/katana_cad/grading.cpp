#include "katana/cad/grading.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "katana/geometry/polygon.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/terrain/volume.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::Vec2;

namespace {

// Half a metre resolves any batter a machine can build; the bisection then
// finds the crossing to a millimetre. The same step as the corridor's.
constexpr double kStep = 0.5;

// A corner sharper than this - the bisector at more than about 78 degrees
// to the edge normal - would send the mitre out by more than five times the
// batter width. Real pads do not have corners like that; a feature line
// that does gets its corner graded as if it were this sharp, which keeps
// the daylight line finite. cos(78 deg) = 0.2.
constexpr double kMinimumCorner = 0.2;

Status validate(const FeatureLine& line, const GradingSlopes& slopes)
{
    for (const Point3& vertex : line.vertices) {
        if (!vertex.isFinite()) {
            return makeError(ErrorCode::InvalidArgument, "the feature line has a non-finite vertex");
        }
    }
    const std::size_t least = line.closed ? 3 : 2;
    if (line.vertices.size() < least) {
        return makeError(ErrorCode::InvalidArgument,
                         line.closed ? "a closed feature line needs at least three vertices"
                                     : "a feature line needs at least two vertices");
    }
    if (!(slopes.cutBatter > 0.0) || !(slopes.fillBatter > 0.0) ||
        !std::isfinite(slopes.cutBatter) || !std::isfinite(slopes.fillBatter)) {
        return makeError(ErrorCode::InvalidArgument,
                         "batter slopes must be positive (horizontal run per unit rise)");
    }
    if (!(slopes.maximumWidth > 0.0) || !std::isfinite(slopes.maximumWidth)) {
        return makeError(ErrorCode::InvalidArgument, "the maximum batter width must be positive");
    }
    if (!(slopes.interval > 0.0) || !std::isfinite(slopes.interval)) {
        return makeError(ErrorCode::InvalidArgument, "the sample interval must be positive");
    }
    return {};
}

// Where a batter runs from, which way, and how much flatter it is in that
// direction than the edge's own batter.
struct Sample {
    Point3 origin;
    Vec2 direction; // unit, outward
    // The edge normal's projection on `direction`: 1 along an edge, cos of
    // half the corner at a vertex. The batter's run per rise in the march
    // direction is the design run divided by this.
    double cosine = 1.0;
};

// The outward side of a closed feature line: to the right of travel when
// the ring is counter-clockwise, to the left when it is clockwise.
double outwardSign(const FeatureLine& line, GradingSide side)
{
    if (!line.closed) {
        return side == GradingSide::Left ? 1.0 : -1.0;
    }
    geometry::Polyline2 plan;
    for (const Point3& vertex : line.vertices) {
        plan.vertices.emplace_back(vertex.x, vertex.y);
    }
    plan.closed = true;
    return plan.signedArea() > 0.0 ? -1.0 : 1.0;
}

// `sign` * perpendicular of the direction of travel: +1 is the left.
Vec2 sideNormal(const Vec2& travel, double sign)
{
    return travel.perpendicular() * sign;
}

std::vector<Sample> sampleFeatureLine(const FeatureLine& line, const GradingSlopes& slopes)
{
    std::vector<Sample> samples;
    const std::size_t count = line.vertices.size();
    const std::size_t edges = line.closed ? count : count - 1;
    const double sign = outwardSign(line, slopes.side);

    const auto travelOf = [&](std::size_t edge) -> std::optional<Vec2> {
        const Point3& a = line.vertices[edge % count];
        const Point3& b = line.vertices[(edge + 1) % count];
        const Vec2 d(b.x - a.x, b.y - a.y);
        const double length = d.length();
        if (!(length > tol::kGeometric)) {
            return std::nullopt; // a doubled vertex has no direction
        }
        return d / length;
    };

    for (std::size_t i = 0; i < count; ++i) {
        // The vertex: along the bisector of its two edge normals. An open
        // line's ends have one edge, and its normal.
        const bool hasBefore = line.closed || i > 0;
        const bool hasAfter = line.closed || i + 1 < count;
        const auto before = hasBefore ? travelOf((i + count - 1) % count) : std::nullopt;
        const auto after = hasAfter ? travelOf(i) : std::nullopt;
        Sample vertex;
        vertex.origin = line.vertices[i];
        if (before && after) {
            const Vec2 n1 = sideNormal(*before, sign);
            const Vec2 n2 = sideNormal(*after, sign);
            const Vec2 sum = n1 + n2;
            if (sum.length() > tol::kGeometric) {
                vertex.direction = sum.normalized();
                vertex.cosine = std::max(kMinimumCorner, vertex.direction.dot(n2));
            } else {
                // A hairpin: the normals cancel. March straight on.
                vertex.direction = *after;
                vertex.cosine = kMinimumCorner;
            }
        } else if (before || after) {
            vertex.direction = sideNormal(before ? *before : *after, sign);
        } else {
            continue; // both edges degenerate: nothing to grade from here
        }
        samples.push_back(vertex);

        // Along the edge that follows, at the interval, ends excluded.
        if (i < edges && after) {
            const Point3& a = line.vertices[i];
            const Point3& b = line.vertices[(i + 1) % count];
            const double length = std::hypot(b.x - a.x, b.y - a.y);
            const auto steps = static_cast<std::size_t>(std::floor(length / slopes.interval));
            const Vec2 normal = sideNormal(*after, sign);
            for (std::size_t k = 1; k < steps; ++k) {
                const double t = static_cast<double>(k) * slopes.interval / length;
                Sample along;
                along.origin = a + (b - a) * t;
                along.direction = normal;
                samples.push_back(along);
            }
            // The interval does not usually divide the edge; the last stretch
            // is shorter rather than a sample landing on the next vertex.
        }
    }
    return samples;
}

// The corridor's daylight search, from an arbitrary origin in an arbitrary
// direction. See corridor.cpp for why it marches rather than solving.
std::optional<DaylightPoint> daylight(const terrain::TinSurface& ground, const Sample& sample,
                                      const GradingSlopes& slopes)
{
    const auto groundAt = [&](double d) {
        return ground.elevationAt(Point2(sample.origin.x + sample.direction.x * d,
                                         sample.origin.y + sample.direction.y * d));
    };
    const auto groundAtOrigin = groundAt(0.0);
    if (!groundAtOrigin) {
        return std::nullopt;
    }
    const double difference = *groundAtOrigin - sample.origin.z;
    DaylightPoint point;
    point.origin = sample.origin;
    point.cut = difference > 0.0;
    if (std::abs(difference) <= tol::kCoordinate) {
        point.daylight = sample.origin; // already at grade
        return point;
    }
    const double run = (point.cut ? slopes.cutBatter : slopes.fillBatter) / sample.cosine;
    const double rise = (point.cut ? 1.0 : -1.0) / run;
    const auto batterAbove = [&](double d) -> std::optional<bool> {
        const auto z = groundAt(d);
        if (!z) {
            return std::nullopt;
        }
        const double batter = sample.origin.z + rise * d;
        return point.cut ? batter >= *z : batter <= *z;
    };

    double previous = 0.0;
    // The width limit is a plan distance perpendicular to the edge; along a
    // mitre the same batter runs further.
    const double limit = slopes.maximumWidth / sample.cosine;
    for (double d = kStep; d <= limit + 1e-9; d += kStep) {
        const auto crossed = batterAbove(d);
        if (!crossed) {
            return std::nullopt;
        }
        if (*crossed) {
            double low = previous;
            double high = d;
            for (int i = 0; i < 40; ++i) {
                const double mid = 0.5 * (low + high);
                const auto at = batterAbove(mid);
                if (!at) {
                    return std::nullopt;
                }
                (*at ? high : low) = mid;
            }
            const double d0 = 0.5 * (low + high);
            point.daylight = Point3(sample.origin.x + sample.direction.x * d0,
                                    sample.origin.y + sample.direction.y * d0,
                                    sample.origin.z + rise * d0);
            return point;
        }
        previous = d;
    }
    return std::nullopt;
}

} // namespace

FeatureLine FeatureLine::pad(const geometry::Polyline2& footprint, double elevation)
{
    FeatureLine line;
    for (const Point2& vertex : footprint.vertices) {
        line.vertices.emplace_back(vertex.x, vertex.y, elevation);
    }
    line.closed = true;
    return line;
}

Result<Grading> gradeToSurface(const FeatureLine& line, const GradingSlopes& slopes,
                               const terrain::TinSurface& ground)
{
    if (auto status = validate(line, slopes); !status) {
        return status.error();
    }
    Grading result;
    const std::vector<Sample> samples = sampleFeatureLine(line, slopes);
    result.samples = samples.size();
    for (const Sample& sample : samples) {
        if (auto point = daylight(ground, sample, slopes)) {
            result.daylights.push_back(*point);
        } else {
            ++result.missed;
        }
    }
    if (result.daylights.size() < 2) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the batters reach the ground at fewer than two points: is the "
                         "feature line over the surface?",
                         std::to_string(result.missed) + " of " + std::to_string(result.samples) +
                             " samples found no ground");
    }

    // The surface: the feature line and the daylight line as breaklines, and
    // every batter as one - each is the edge of a plane and the triangles must
    // not cut across it. The daylight line is the boundary when it is whole
    // and simple; a concave corner whose batters overrun its neighbours' can
    // make it cross itself, and then the hull bounds the surface instead.
    terrain::TinInput input;
    terrain::Breakline feature;
    feature.vertices = line.vertices;
    feature.closed = line.closed;
    input.breaklines.push_back(feature);
    terrain::Breakline daylightLine;
    for (const DaylightPoint& point : result.daylights) {
        input.points.push_back(point.origin);
        input.points.push_back(point.daylight);
        daylightLine.vertices.push_back(point.daylight);
        // At grade there is no batter, and a breakline of no length is
        // refused by the builder rather than quietly accepted.
        if (point.daylight.distanceTo(point.origin) > tol::kGeometric) {
            terrain::Breakline batter;
            batter.vertices = {point.origin, point.daylight};
            input.breaklines.push_back(batter);
        }
        result.daylightLine.vertices.emplace_back(point.daylight.x, point.daylight.y);
    }
    for (const Point3& vertex : line.vertices) {
        input.points.push_back(vertex);
    }
    daylightLine.closed = line.closed;
    result.daylightLine.closed = line.closed;
    input.breaklines.push_back(daylightLine);

    if (result.missed == 0) {
        geometry::Polyline2 boundary = result.daylightLine;
        if (!line.closed) {
            // An open line's grading is the strip between it and its daylight:
            // close the boundary back along the feature line.
            for (auto it = line.vertices.rbegin(); it != line.vertices.rend(); ++it) {
                boundary.vertices.emplace_back(it->x, it->y);
            }
            boundary.closed = true;
        }
        const geometry::Polyline2 cleaned = boundary.withoutDuplicateVertices();
        if (cleaned.vertices.size() >= 3 && geometry::isSimple(cleaned)) {
            input.boundary = cleaned;
            result.boundedByDaylight = true;
        }
    }

    terrain::TinBuildOptions options;
    // Batter lines meet the feature and daylight lines at their own ends;
    // where two mitred batters cross at a concave corner the mean is right.
    options.duplicatePoints = terrain::DuplicatePointPolicy::Average;
    options.crossingBreaklines = terrain::CrossingBreaklinePolicy::Average;
    auto built = terrain::buildTin(input, options);
    if (!built) {
        return built.error();
    }
    result.surface = std::move(built->surface);

    auto volumes = terrain::compareSurfaces(ground, result.surface);
    if (!volumes) {
        return volumes.error();
    }
    result.cut = volumes->cut;
    result.fill = volumes->fill;
    result.planArea = volumes->planArea;
    return result;
}

} // namespace katana::cad
