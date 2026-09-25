#include "import_alignment.hpp"

#include <algorithm>
#include <cmath>
#include <functional>

#include "katana/geometry/spiral2.hpp"
#include "katana/math/numerics.hpp"

namespace katana::ifc::detail {

namespace {

using katana::math::Vec2;

// Below this a segment has no length (the closing segments IFC 4.3 ends
// each layout with) and a direction or curvature no difference.
constexpr double kNoLength = 1e-9;
constexpr double kSameDirection = 1e-9;

bool sameCurvature(double a, double b)
{
    return std::abs(a - b) <= 1e-6 * std::max(std::abs(a), std::abs(b)) + 1e-12;
}

Vec2 unit(double direction)
{
    return Vec2(std::cos(direction), std::sin(direction));
}

double perpDot(Vec2 a, Vec2 b)
{
    return a.x * b.y - a.y * b.x;
}

std::vector<HorizontalSegment> nonZero(const std::vector<HorizontalSegment>& segments)
{
    std::vector<HorizontalSegment> out;
    for (const HorizontalSegment& segment : segments) {
        if (segment.length > kNoLength) {
            out.push_back(segment);
        }
    }
    return out;
}

// The curvature of a transition at fraction u of its length, by the
// definitions of the IFC 4.3 transition types. The cubic parabola is not a
// function of arc length; its curvature is close to the clothoid's over the
// short, flat transitions it is used for, and is taken as that. The Viennese
// bend needs the cant it is designed with, which is not read; it is taken as
// the Bloss curve its curvature follows away from the cant.
double transitionCurvature(const std::string& type, double k0, double k1, double u)
{
    const double d = k1 - k0;
    if (type == "BLOSSCURVE" || type == "VIENNESEBEND") {
        return k0 + d * u * u * (3.0 - 2.0 * u);
    }
    if (type == "COSINECURVE") {
        return k0 + d * (1.0 - std::cos(math::kPi * u)) / 2.0;
    }
    if (type == "SINECURVE") {
        return k0 + d * (u - std::sin(math::kTwoPi * u) / math::kTwoPi);
    }
    if (type == "HELMERTCURVE") {
        return k0 + d * (u < 0.5 ? 2.0 * u * u : 1.0 - 2.0 * (1.0 - u) * (1.0 - u));
    }
    return k0 + d * u; // CLOTHOID, CUBIC
}

} // namespace

std::optional<geometry::HorizontalAlignment>
reconstructHorizontal(const std::vector<HorizontalSegment>& all, double startStation,
                      std::string& why)
{
    const std::vector<HorizontalSegment> segments = nonZero(all);
    if (segments.empty()) {
        why = "it has no segment with a length";
        return std::nullopt;
    }
    for (const HorizontalSegment& segment : segments) {
        if (segment.type != "LINE" && segment.type != "CIRCULARARC" && segment.type != "CLOTHOID") {
            why = "it has a " + segment.type +
                  " transition, which Katana does not have (it has the clothoid)";
            return std::nullopt;
        }
    }
    if (segments.front().type != "LINE" || segments.back().type != "LINE") {
        why = std::string("it ") + (segments.front().type != "LINE" ? "starts" : "ends") +
              " on a curve, which has no point of intersection";
        return std::nullopt;
    }

    geometry::HorizontalAlignment definition;
    definition.startStation = startStation;
    definition.pis.push_back({segments.front().start, 0.0, 0.0, 0.0});
    std::size_t i = 0;
    while (i + 1 < segments.size()) {
        std::size_t j = i + 1;
        while (segments[j].type != "LINE") {
            ++j;
        }
        const HorizontalSegment& before = segments[i];
        const HorizontalSegment& after = segments[j];
        const std::vector<HorizontalSegment> curve(segments.begin() + static_cast<long>(i) + 1,
                                                   segments.begin() + static_cast<long>(j));
        const Vec2 end = before.start + unit(before.direction) * before.length;
        geometry::AlignmentPI pi;
        if (curve.empty()) {
            // Two tangents meeting: a kink where they turn, nothing where they
            // run on.
            if (std::abs(std::remainder(after.direction - before.direction, math::kTwoPi)) >
                kSameDirection) {
                pi.point = end;
                definition.pis.push_back(pi);
            }
            i = j;
            continue;
        }
        // [CLOTHOID] CIRCULARARC [CLOTHOID]: a spiral in from the straight to
        // the arc's curvature, the arc, a spiral out back to the straight.
        std::size_t arc = 0;
        while (arc < curve.size() && curve[arc].type != "CIRCULARARC") {
            ++arc;
        }
        const bool shape = arc < curve.size() && arc <= 1 && curve.size() - arc <= 2 &&
                           (arc == 0 || curve[0].type == "CLOTHOID") &&
                           (arc + 1 == curve.size() || curve[arc + 1].type == "CLOTHOID");
        if (!shape) {
            why = "the curve between its tangents " + std::to_string(i + 1) + " and " +
                  std::to_string(j + 1) +
                  " is not a spiral, an arc and a spiral (a compound or reverse curve has no "
                  "single point of intersection)";
            return std::nullopt;
        }
        const double k = curve[arc].startCurvature;
        if (k == 0.0 || !sameCurvature(k, curve[arc].endCurvature)) {
            why = "an arc's radius changes along it";
            return std::nullopt;
        }
        if (arc == 1 && !(sameCurvature(curve[0].startCurvature, 0.0) &&
                          sameCurvature(curve[0].endCurvature, k))) {
            why = "a spiral does not run from the straight to its arc's radius";
            return std::nullopt;
        }
        if (arc + 1 < curve.size() && !(sameCurvature(curve[arc + 1].startCurvature, k) &&
                                        sameCurvature(curve[arc + 1].endCurvature, 0.0))) {
            why = "a spiral does not run from its arc's radius to the straight";
            return std::nullopt;
        }
        const Vec2 u = unit(before.direction);
        const Vec2 v = unit(after.direction);
        const double denominator = perpDot(u, v);
        if (std::abs(denominator) < 1e-12) {
            why = "two tangents either side of a curve are parallel";
            return std::nullopt;
        }
        const double t = perpDot(after.start - end, v) / denominator;
        pi.point = end + u * t;
        pi.radius = 1.0 / std::abs(k);
        pi.spiralIn = arc == 1 ? curve[0].length : 0.0;
        pi.spiralOut = arc + 1 < curve.size() ? curve[arc + 1].length : 0.0;
        definition.pis.push_back(pi);
        i = j;
    }
    const HorizontalSegment& last = segments.back();
    definition.pis.push_back({last.start + unit(last.direction) * last.length, 0.0, 0.0, 0.0});
    if (definition.pis.size() < 2) {
        why = "it has fewer than two points of intersection";
        return std::nullopt;
    }
    return definition;
}

double checkHorizontal(const geometry::SolvedAlignment& solved,
                       const std::vector<HorizontalSegment>& segments)
{
    double worst = 0.0;
    double distance = 0.0;
    const auto compare = [&](Vec2 expected) {
        const double station = std::clamp(solved.startStation() + distance, solved.startStation(),
                                          solved.endStation());
        if (const auto at = solved.pointAtStation(station)) {
            worst = std::max(worst, (*at - expected).length());
        }
    };
    for (const HorizontalSegment& segment : segments) {
        compare(segment.start);
        distance += segment.length;
    }
    worst = std::max(worst, std::abs(solved.length() - distance));
    return worst;
}

std::optional<geometry::VerticalAlignment>
reconstructVertical(const std::vector<VerticalSegment>& all, double startStation, std::string& why)
{
    std::vector<VerticalSegment> segments;
    for (const VerticalSegment& segment : all) {
        if (segment.length > kNoLength) {
            segments.push_back(segment);
        }
    }
    if (segments.empty()) {
        why = "it has no segment with a length";
        return std::nullopt;
    }
    for (const VerticalSegment& segment : segments) {
        if (segment.type != "CONSTANTGRADIENT" && segment.type != "PARABOLICARC") {
            why = "it has a " + segment.type +
                  " vertical curve, and Katana designs vertical curves as parabolas";
            return std::nullopt;
        }
    }
    const auto endHeight = [](const VerticalSegment& s) {
        return s.startHeight + (s.startGrade + s.endGrade) / 2.0 * s.length;
    };
    for (std::size_t k = 0; k + 1 < segments.size(); ++k) {
        const VerticalSegment& a = segments[k];
        const VerticalSegment& b = segments[k + 1];
        if (std::abs(a.startDistance + a.length - b.startDistance) > 1e-3 ||
            std::abs(endHeight(a) - b.startHeight) > 1e-3) {
            why = "its segments " + std::to_string(k + 1) + " and " + std::to_string(k + 2) +
                  " do not meet";
            return std::nullopt;
        }
    }
    geometry::VerticalAlignment definition;
    definition.pvis.push_back(
        {startStation + segments.front().startDistance, segments.front().startHeight, 0.0});
    for (std::size_t k = 0; k < segments.size(); ++k) {
        const VerticalSegment& s = segments[k];
        if (s.type == "PARABOLICARC") {
            definition.pvis.push_back({startStation + s.startDistance + s.length / 2.0,
                                       s.startHeight + s.startGrade * s.length / 2.0, s.length});
        } else if (k + 1 < segments.size() && segments[k + 1].type == "CONSTANTGRADIENT" &&
                   std::abs(segments[k + 1].startGrade - s.startGrade) > 1e-12) {
            definition.pvis.push_back(
                {startStation + s.startDistance + s.length, endHeight(s), 0.0});
        }
    }
    const VerticalSegment& last = segments.back();
    definition.pvis.push_back(
        {startStation + last.startDistance + last.length, endHeight(last), 0.0});
    return definition;
}

double checkVertical(const geometry::SolvedProfile& solved,
                     const std::vector<VerticalSegment>& segments, double startStation)
{
    double worst = 0.0;
    for (const VerticalSegment& segment : segments) {
        for (const double along : {0.0, segment.length / 2.0, segment.length}) {
            const double station = startStation + segment.startDistance + along;
            const auto expected = heightAt(segments, segment.startDistance + along);
            const auto actual =
                solved.elevationAt(std::clamp(station, solved.startStation(), solved.endStation()));
            if (expected && actual) {
                worst = std::max(worst, std::abs(*expected - *actual));
            }
        }
    }
    return worst;
}

std::vector<std::pair<double, Vec2>> sampleHorizontal(const std::vector<HorizontalSegment>& all,
                                                      double tolerance)
{
    std::vector<std::pair<double, Vec2>> out;
    double distance = 0.0;
    const auto add = [&](double along, Vec2 at) {
        if (!out.empty() && (out.back().second - at).length() < 1e-9) {
            return;
        }
        out.emplace_back(along, at);
    };
    for (const HorizontalSegment& segment : nonZero(all)) {
        const double k0 = segment.startCurvature;
        const double k1 = segment.endCurvature;
        const double kMax = std::max(std::abs(k0), std::abs(k1));
        // Chords from the sagitta rule at the tightest curvature (as
        // geometry::chording does): a chord of length c stands off a circle
        // of curvature k by about k c^2 / 8.
        std::size_t count = 1;
        if (kMax > 0.0) {
            const double chord = std::sqrt(8.0 * tolerance / kMax);
            count = std::max<std::size_t>(
                1, static_cast<std::size_t>(std::ceil(segment.length / chord)));
        }
        count = std::min<std::size_t>(count, 100000);
        if (segment.type == "LINE") {
            add(distance, segment.start);
        } else if (segment.type == "CIRCULARARC" || segment.type == "CLOTHOID") {
            geometry::Spiral2 exact{segment.start, segment.direction, k0, k1, segment.length};
            for (std::size_t n = 0; n < count; ++n) {
                const double s =
                    segment.length * static_cast<double>(n) / static_cast<double>(count);
                add(distance + s, exact.pointAt(s));
            }
        } else {
            // Direction is the integral of curvature, position the integral
            // of direction: Simpson's rule on a fine grid, eight steps to
            // each chord written.
            const std::size_t steps = count * 8;
            const double h = segment.length / static_cast<double>(steps);
            Vec2 at = segment.start;
            double direction = segment.direction;
            const auto curvature = [&](double s) {
                return transitionCurvature(segment.type, k0, k1, s / segment.length);
            };
            for (std::size_t n = 0; n < steps; ++n) {
                if (n % 8 == 0) {
                    add(distance + h * static_cast<double>(n), at);
                }
                const double s = h * static_cast<double>(n);
                const double dMid = direction + h / 12.0 *
                                                    (curvature(s) + 4.0 * curvature(s + h / 4.0) +
                                                     curvature(s + h / 2.0));
                const double dEnd =
                    direction +
                    h / 6.0 * (curvature(s) + 4.0 * curvature(s + h / 2.0) + curvature(s + h));
                at = at + (unit(direction) + unit(dMid) * 4.0 + unit(dEnd)) * (h / 6.0);
                direction = dEnd;
            }
        }
        distance += segment.length;
    }
    const auto segments = nonZero(all);
    if (!segments.empty()) {
        const HorizontalSegment& last = segments.back();
        if (last.type == "LINE") {
            add(distance, last.start + unit(last.direction) * last.length);
        } else if (last.type == "CIRCULARARC" || last.type == "CLOTHOID") {
            geometry::Spiral2 exact{last.start, last.direction, last.startCurvature,
                                    last.endCurvature, last.length};
            add(distance, exact.endPoint());
        }
    }
    return out;
}

std::optional<double> heightAt(const std::vector<VerticalSegment>& segments, double distance)
{
    for (const VerticalSegment& s : segments) {
        if (s.length <= kNoLength || distance < s.startDistance - 1e-9 ||
            distance > s.startDistance + s.length + 1e-9) {
            continue;
        }
        const double x = std::clamp(distance - s.startDistance, 0.0, s.length);
        if (s.type == "CIRCULARARC") {
            // Slope angle a from atan g1 to atan g2: sin a changes by x / R'
            // over a horizontal x, with R' = L / (sin a2 - sin a1).
            const double a1 = std::atan(s.startGrade);
            const double a2 = std::atan(s.endGrade);
            if (std::abs(std::sin(a2) - std::sin(a1)) > 1e-12) {
                const double r = s.length / (std::sin(a2) - std::sin(a1));
                const double sa = std::clamp(std::sin(a1) + x / r, -1.0, 1.0);
                return s.startHeight + r * (std::cos(a1) - std::sqrt(1.0 - sa * sa));
            }
        }
        // CONSTANTGRADIENT, PARABOLICARC; a vertical CLOTHOID taken as the
        // parabola with the same grades.
        return s.startHeight + s.startGrade * x +
               (s.endGrade - s.startGrade) / (2.0 * s.length) * x * x;
    }
    return std::nullopt;
}

} // namespace katana::ifc::detail
