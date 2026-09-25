#include "katana/survey/subsurface/clearance.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <tuple>
#include <utility>

namespace katana::survey::subsurface {

namespace {

using core::ErrorCode;
using core::makeError;

struct Closest {
    double distance = 0.0;
    double s = 0.0; // parameter along the first segment, [0, 1]
    double t = 0.0; // parameter along the second segment, [0, 1]
};

double cross(double an, double ae, double bn, double be)
{
    return an * be - ae * bn;
}

// Distance from p to segment a-b, and the parameter of the foot along a-b.
std::pair<double, double> toSegment(const Coordinate2& p, const Coordinate2& a,
                                    const Coordinate2& b)
{
    const double dn = b.northing - a.northing;
    const double de = b.easting - a.easting;
    const double lengthSquared = dn * dn + de * de;
    double t = 0.0;
    if (lengthSquared > 0.0) {
        t = ((p.northing - a.northing) * dn + (p.easting - a.easting) * de) / lengthSquared;
        t = std::clamp(t, 0.0, 1.0);
    }
    return {std::hypot(p.northing - (a.northing + t * dn), p.easting - (a.easting + t * de)), t};
}

// Closest approach of two plan segments a0-a1 and b0-b1.
Closest closest(const Coordinate2& a0, const Coordinate2& a1, const Coordinate2& b0,
                const Coordinate2& b1)
{
    const double rn = a1.northing - a0.northing;
    const double re = a1.easting - a0.easting;
    const double sn = b1.northing - b0.northing;
    const double se = b1.easting - b0.easting;
    const double denominator = cross(rn, re, sn, se);
    if (denominator != 0.0) {
        const double qn = b0.northing - a0.northing;
        const double qe = b0.easting - a0.easting;
        const double s = cross(qn, qe, sn, se) / denominator;
        const double t = cross(qn, qe, rn, re) / denominator;
        if (s >= 0.0 && s <= 1.0 && t >= 0.0 && t <= 1.0) {
            return {0.0, s, t};
        }
    }
    // Apart (or parallel): the closest approach involves an end of one of them.
    Closest best;
    best.distance = std::numeric_limits<double>::infinity();
    const auto consider = [&best](double distance, double s, double t) {
        if (distance < best.distance) {
            best = {distance, s, t};
        }
    };
    auto [d, t] = toSegment(a0, b0, b1);
    consider(d, 0.0, t);
    std::tie(d, t) = toSegment(a1, b0, b1);
    consider(d, 1.0, t);
    std::tie(d, t) = toSegment(b0, a0, a1);
    consider(d, t, 0.0);
    std::tie(d, t) = toSegment(b1, a0, a1);
    consider(d, t, 1.0);
    return best;
}

Coordinate2 along(const Coordinate2& a, const Coordinate2& b, double t)
{
    return {a.northing + t * (b.northing - a.northing), a.easting + t * (b.easting - a.easting)};
}

ClearanceStatus against(double gap, double tolerance, double required)
{
    if (gap - tolerance >= required) {
        return ClearanceStatus::Clear;
    }
    return gap >= required ? ClearanceStatus::WithinTolerance : ClearanceStatus::Conflict;
}

// True when `candidate` should replace `current` as the governing result:
// worse status first, then the smaller gap.
bool governs(const ClearanceResult& candidate, const ClearanceResult& current)
{
    if (candidate.status != current.status) {
        return candidate.status < current.status;
    }
    return candidate.horizontalGap < current.horizontalGap;
}

} // namespace

const char* toString(ClearanceStatus status)
{
    switch (status) {
    case ClearanceStatus::Conflict:
        return "conflict";
    case ClearanceStatus::Unconfirmed:
        return "unconfirmed";
    case ClearanceStatus::WithinTolerance:
        return "within tolerance";
    case ClearanceStatus::Clear:
        return "clear";
    }
    return "conflict";
}

core::Result<std::vector<ClearanceResult>> checkClearance(const DesignAlignment& design,
                                                          const std::vector<UtilityLine>& utilities,
                                                          const ClearanceRequirement& requirement,
                                                          const GradingSettings& settings)
{
    if (design.vertices.size() < 2) {
        return makeError(ErrorCode::InvalidArgument, "the design needs at least two vertices",
                         design.id);
    }
    for (const DesignVertex& vertex : design.vertices) {
        if (!std::isfinite(vertex.position.northing) || !std::isfinite(vertex.position.easting)) {
            return makeError(ErrorCode::InvalidArgument,
                             "design vertex has a non-finite coordinate", design.id);
        }
    }
    const auto nonNegative = [](double value) { return std::isfinite(value) && value >= 0.0; };
    if (!nonNegative(design.halfWidth) || !nonNegative(requirement.horizontal) ||
        !nonNegative(requirement.vertical) || !nonNegative(requirement.unverifiedMargin)) {
        return makeError(ErrorCode::InvalidArgument,
                         "half-width and clearances must be finite and not negative", design.id);
    }

    std::vector<ClearanceResult> results;
    for (const UtilityLine& line : utilities) {
        auto graded = gradeLine(line, settings);
        if (!graded) {
            return graded.error();
        }
        const double diameter = line.attributes.diameter;
        const double radius = std::isfinite(diameter) && diameter > 0.0 ? diameter / 2.0 : 0.0;

        for (const GradedSegment& segment : graded->segments) {
            const UtilityVertex& u0 = line.vertices[segment.from];
            const UtilityVertex& u1 = line.vertices[segment.from + 1];
            const Tolerance& tolerance = settings.tolerances.of(segment.level);

            // Centre-line level of the service at both ends, only where both
            // are levels the evidence qualifies.
            std::optional<double> z0;
            std::optional<double> z1;
            if (graded->vertices[segment.from].classification.levelQualified &&
                graded->vertices[segment.from + 1].classification.levelQualified) {
                const auto top0 = topLevel(u0, diameter);
                const auto top1 = topLevel(u1, diameter);
                if (top0 && top1) {
                    z0 = *top0 - radius;
                    z1 = *top1 - radius;
                }
            }

            std::optional<ClearanceResult> governing;
            for (std::size_t j = 0; j + 1 < design.vertices.size(); ++j) {
                const DesignVertex& d0 = design.vertices[j];
                const DesignVertex& d1 = design.vertices[j + 1];
                const Closest approach =
                    closest(u0.position, u1.position, d0.position, d1.position);

                ClearanceResult result;
                result.utilityId = line.id;
                result.segment = segment.from;
                result.fromId = u0.id;
                result.toId = u1.id;
                result.level = segment.level;
                result.nearest = along(u0.position, u1.position, approach.s);
                result.planDistance = approach.distance;
                result.horizontalGap = approach.distance - radius - design.halfWidth;
                result.horizontalTolerance = tolerance.horizontal;

                if (tolerance.horizontal) {
                    result.status = against(result.horizontalGap, *tolerance.horizontal,
                                            requirement.horizontal);
                } else if (result.horizontalGap >=
                           requirement.horizontal + requirement.unverifiedMargin) {
                    result.status = ClearanceStatus::Clear;
                } else {
                    result.status = ClearanceStatus::Unconfirmed;
                    result.note = std::string(toString(segment.level)) +
                                  " position is not a measurement; locate the service to QL-B "
                                  "or QL-A before relying on this clearance";
                }

                if (z0 && z1 && d0.level && d1.level && tolerance.vertical) {
                    const double zu = *z0 + approach.s * (*z1 - *z0);
                    const double zd = *d0.level + approach.t * (*d1.level - *d0.level);
                    result.verticalGap = std::abs(zd - zu) - radius - design.halfWidth;
                    result.verticalTolerance = tolerance.vertical;
                    if (result.status != ClearanceStatus::Clear) {
                        const ClearanceStatus vertical =
                            against(*result.verticalGap, *tolerance.vertical, requirement.vertical);
                        if (vertical > result.status) {
                            result.status = vertical;
                            result.note = vertical == ClearanceStatus::Clear
                                              ? "cleared by vertical separation"
                                              : "vertical separation clears it at the drawn "
                                                "levels, not across the tolerance";
                        }
                    }
                }
                if (radius == 0.0 && result.note.empty()) {
                    result.note = "no diameter recorded; the gap is to the centre line";
                }

                if (!governing || governs(result, *governing)) {
                    governing = std::move(result);
                }
            }
            results.push_back(std::move(*governing));
        }
    }
    return results;
}

} // namespace katana::survey::subsurface
