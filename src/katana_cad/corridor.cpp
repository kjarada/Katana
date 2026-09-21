#include "katana/cad/corridor.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <string>

#include "katana/geometry/primitives3d.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/math/numerics.hpp"
#include "katana/math/summation.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;

namespace {

Status validateAssembly(const Assembly& assembly)
{
    const bool finite = std::isfinite(assembly.halfWidth) && std::isfinite(assembly.crossfall) &&
                        std::isfinite(assembly.cutBatter) && std::isfinite(assembly.fillBatter) &&
                        std::isfinite(assembly.maximumBatterWidth);
    if (!finite) {
        return makeError(ErrorCode::InvalidArgument, "the assembly has a non-finite value");
    }
    if (!(assembly.halfWidth > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "the assembly half width must be positive");
    }
    if (!(assembly.cutBatter > 0.0) || !(assembly.fillBatter > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "batter slopes must be positive (horizontal run per unit rise)");
    }
    if (!(assembly.maximumBatterWidth > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "the maximum batter width must be positive");
    }
    return {};
}

// The ground under the section at `offset` from `centre`, positive left of
// `direction`.
std::optional<double> groundAt(const terrain::TinSurface& ground, const Point2& centre,
                               double direction, double offset)
{
    const Point2 at(centre.x - std::sin(direction) * offset, centre.y + std::cos(direction) * offset);
    return ground.elevationAt(at);
}

// Carries the template edge to the ground on one side. `side` is +1 for the
// left, -1 for the right; `edge` is the edge vertex. Returns the daylight
// vertex, or nullopt when the batter does not meet the ground within the
// assembly's limit or the ground is missing.
//
// Marched outward in fixed steps until the batter line and the ground have
// changed order, then bisected between the last two samples. The march
// rather than a closed form because the ground is a TIN: piecewise planar,
// with no formula for where an arbitrary line meets it.
std::optional<SectionVertex> daylight(const terrain::TinSurface& ground, const Point2& centre,
                                      double direction, const Assembly& assembly,
                                      const SectionVertex& edge, double side)
{
    const auto groundAtEdge = groundAt(ground, centre, direction, edge.offset);
    if (!groundAtEdge) {
        return std::nullopt;
    }
    const double difference = *groundAtEdge - edge.elevation;
    if (std::abs(difference) <= tol::kCoordinate) {
        return SectionVertex{edge.offset, edge.elevation}; // already at grade
    }
    // In cut the batter rises from the edge; in fill it falls. Either way the
    // batter elevation at distance d out from the edge is edge +/- d / run.
    const bool cut = difference > 0.0;
    const double rise = (cut ? 1.0 : -1.0) / (cut ? assembly.cutBatter : assembly.fillBatter);
    const auto batterAbove = [&](double d) -> std::optional<bool> {
        const auto z = groundAt(ground, centre, direction, edge.offset + side * d);
        if (!z) {
            return std::nullopt;
        }
        // "Above ground" from the batter's point of view: in cut the batter
        // starts below the ground and daylights when it rises above it; in
        // fill it starts above and daylights when it falls below.
        const double batter = edge.elevation + rise * d;
        return cut ? batter >= *z : batter <= *z;
    };

    // Half a metre resolves any batter a machine can build; the bisection
    // below then finds the crossing to a millimetre.
    constexpr double kStep = 0.5;
    double previous = 0.0;
    for (double d = kStep; d <= assembly.maximumBatterWidth + 1e-9; d += kStep) {
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
            return SectionVertex{edge.offset + side * d0, edge.elevation + rise * d0};
        }
        previous = d;
    }
    return std::nullopt;
}

// Linear interpolation on a line whose vertices are ordered by offset,
// either direction; nullopt outside its range.
std::optional<double> elevationOn(const std::vector<SectionVertex>& line, double offset)
{
    if (line.size() < 2) {
        return std::nullopt;
    }
    for (std::size_t i = 0; i + 1 < line.size(); ++i) {
        const SectionVertex& a = line[i];
        const SectionVertex& b = line[i + 1];
        const double lo = std::min(a.offset, b.offset);
        const double hi = std::max(a.offset, b.offset);
        if (offset < lo - 1e-12 || offset > hi + 1e-12) {
            continue;
        }
        if (hi - lo < 1e-12) {
            return a.elevation;
        }
        const double t = (offset - a.offset) / (b.offset - a.offset);
        return a.elevation + (b.elevation - a.elevation) * t;
    }
    return std::nullopt;
}

} // namespace

SplitArea areaBetween(const std::vector<SectionVertex>& upper,
                      const std::vector<SectionVertex>& lower)
{
    SplitArea result;
    if (upper.size() < 2 || lower.size() < 2) {
        return result;
    }
    std::vector<double> offsets;
    offsets.reserve(upper.size() + lower.size());
    for (const SectionVertex& v : upper) {
        offsets.push_back(v.offset);
    }
    for (const SectionVertex& v : lower) {
        offsets.push_back(v.offset);
    }
    std::sort(offsets.begin(), offsets.end());
    offsets.erase(std::unique(offsets.begin(), offsets.end(),
                              [](double a, double b) { return std::abs(a - b) < 1e-12; }),
                  offsets.end());

    katana::math::CompensatedSum above;
    katana::math::CompensatedSum below;
    for (std::size_t i = 0; i + 1 < offsets.size(); ++i) {
        const double a = offsets[i];
        const double b = offsets[i + 1];
        const auto ua = elevationOn(upper, a);
        const auto ub = elevationOn(upper, b);
        const auto la = elevationOn(lower, a);
        const auto lb = elevationOn(lower, b);
        if (!ua || !ub || !la || !lb) {
            continue; // one line does not cover this interval
        }
        const double d0 = *ua - *la;
        const double d1 = *ub - *lb;
        const double width = b - a;
        if (d0 >= 0.0 && d1 >= 0.0) {
            above.add(0.5 * (d0 + d1) * width);
        } else if (d0 <= 0.0 && d1 <= 0.0) {
            below.add(-0.5 * (d0 + d1) * width);
        } else {
            // The lines cross inside the interval: two triangles of opposite
            // sign, split where the difference is zero.
            const double t = d0 / (d0 - d1);
            const double left = std::abs(d0) * t * width * 0.5;
            const double right = std::abs(d1) * (1.0 - t) * width * 0.5;
            (d0 > 0.0 ? above : below).add(left);
            (d1 > 0.0 ? above : below).add(right);
        }
    }
    result.above = above.value();
    result.below = below.value();
    return result;
}

Result<CorridorSection> corridorSection(const geometry::SolvedAlignment& alignment,
                                        const geometry::SolvedProfile& profile,
                                        const Assembly& assembly,
                                        const terrain::TinSurface& ground, double station)
{
    if (auto status = validateAssembly(assembly); !status) {
        return status.error();
    }
    const auto centre = alignment.pointAtStation(station);
    const auto direction = alignment.directionAtStation(station);
    const auto elevation = profile.elevationAt(station);
    if (!centre || !direction) {
        return makeError(ErrorCode::InvalidArgument, "the station is outside the alignment",
                         std::to_string(station));
    }
    if (!elevation) {
        return makeError(ErrorCode::InvalidArgument, "the station is outside the design profile",
                         std::to_string(station));
    }

    CorridorSection section;
    section.station = station;
    section.centre = *centre;
    section.designElevation = *elevation;

    const double edgeDrop = assembly.crossfall * assembly.halfWidth;
    const SectionVertex leftEdge{assembly.halfWidth, *elevation - edgeDrop};
    const SectionVertex rightEdge{-assembly.halfWidth, *elevation - edgeDrop};
    const auto leftDaylight = daylight(ground, *centre, *direction, assembly, leftEdge, 1.0);
    const auto rightDaylight = daylight(ground, *centre, *direction, assembly, rightEdge, -1.0);
    if (!leftDaylight || !rightDaylight) {
        section.complete = false;
        return section;
    }
    section.design = {*leftDaylight, leftEdge, SectionVertex{0.0, *elevation}, rightEdge,
                      *rightDaylight};

    // Ground across the same span, at every design vertex and at a regular
    // spacing between them so an undulation between two vertices is not
    // straightened out. Half a metre, as the daylight march.
    constexpr double kSpacing = 0.5;
    std::vector<double> offsets;
    for (const SectionVertex& v : section.design) {
        offsets.push_back(v.offset);
    }
    for (double o = rightDaylight->offset; o < leftDaylight->offset; o += kSpacing) {
        offsets.push_back(o);
    }
    std::sort(offsets.begin(), offsets.end(), std::greater<>()); // left to right
    offsets.erase(std::unique(offsets.begin(), offsets.end(),
                              [](double a, double b) { return std::abs(a - b) < 1e-9; }),
                  offsets.end());
    section.ground.reserve(offsets.size());
    for (const double offset : offsets) {
        const auto z = groundAt(ground, *centre, *direction, offset);
        if (!z) {
            section.complete = false;
            return section;
        }
        section.ground.push_back(SectionVertex{offset, *z});
    }

    const SplitArea split = areaBetween(section.ground, section.design);
    section.cutArea = split.above;  // ground above design
    section.fillArea = split.below; // design above ground
    return section;
}

Result<CorridorQuantities> corridorQuantities(const geometry::SolvedAlignment& alignment,
                                              const geometry::SolvedProfile& profile,
                                              const Assembly& assembly,
                                              const terrain::TinSurface& ground, double interval)
{
    if (auto status = validateAssembly(assembly); !status) {
        return status.error();
    }
    if (!(interval > 0.0) || !std::isfinite(interval)) {
        return makeError(ErrorCode::InvalidArgument, "the interval must be positive");
    }
    const double start = std::max(alignment.startStation(), profile.startStation());
    const double end = std::min(alignment.endStation(), profile.endStation());
    if (!(end - start > tol::kCoordinate)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the alignment and the design profile do not overlap in station");
    }

    // Every interval station plus every key station of the profile inside
    // the range - the quantities change character at a PVC and a PVT, and a
    // report that stepped over them would be one nobody could check.
    std::vector<double> stations;
    for (double s = start; s < end; s += interval) {
        stations.push_back(s);
    }
    stations.push_back(end);
    for (const double key : profile.keyStations()) {
        if (key > start && key < end) {
            stations.push_back(key);
        }
    }
    std::sort(stations.begin(), stations.end());
    stations.erase(std::unique(stations.begin(), stations.end(),
                               [](double a, double b) { return std::abs(a - b) < 1e-9; }),
                   stations.end());

    CorridorQuantities result;
    result.startStation = start;
    result.endStation = end;
    result.sections.reserve(stations.size());
    for (const double station : stations) {
        auto section = corridorSection(alignment, profile, assembly, ground, station);
        if (!section) {
            return section.error();
        }
        if (!section->complete) {
            ++result.incompleteSections;
        }
        result.sections.push_back(std::move(*section));
    }

    // Average end area between consecutive complete sections. An interval
    // touching an incomplete section contributes nothing; the count of those
    // sections is in the result so the total is never mistaken for whole.
    katana::math::CompensatedSum cut;
    katana::math::CompensatedSum fill;
    for (std::size_t i = 0; i + 1 < result.sections.size(); ++i) {
        const CorridorSection& a = result.sections[i];
        const CorridorSection& b = result.sections[i + 1];
        if (!a.complete || !b.complete) {
            continue;
        }
        const double length = b.station - a.station;
        cut.add(0.5 * (a.cutArea + b.cutArea) * length);
        fill.add(0.5 * (a.fillArea + b.fillArea) * length);
    }
    result.cut = cut.value();
    result.fill = fill.value();
    result.net = result.fill - result.cut;
    return result;
}

Result<CorridorSurface> corridorSurface(const geometry::SolvedAlignment& alignment,
                                        const geometry::SolvedProfile& profile,
                                        const Assembly& assembly,
                                        const terrain::TinSurface& ground, double interval)
{
    auto quantities = corridorQuantities(alignment, profile, assembly, ground, interval);
    if (!quantities) {
        return quantities.error();
    }

    terrain::TinInput input;
    std::array<terrain::Breakline, 5> strings{};
    std::vector<Point2> leftDaylight;
    std::vector<Point2> rightDaylight;
    const auto flush = [&] {
        for (terrain::Breakline& string : strings) {
            if (string.vertices.size() >= 2) {
                input.breaklines.push_back(string);
            }
            string.vertices.clear();
        }
    };

    CorridorSurface result;
    for (const CorridorSection& section : quantities->sections) {
        if (!section.complete) {
            flush();
            ++result.incompleteSections;
            continue;
        }
        for (std::size_t k = 0; k < section.design.size(); ++k) {
            const SectionVertex& vertex = section.design[k];
            const auto plan = alignment.pointAtStationOffset(section.station, vertex.offset);
            if (!plan) {
                continue; // the station came from the alignment; cannot happen
            }
            const geometry::Point3 point(plan->x, plan->y, vertex.elevation);
            input.points.push_back(point);
            strings[k].vertices.push_back(point);
            if (k == 0) {
                leftDaylight.push_back(*plan);
            } else if (k + 1 == section.design.size()) {
                rightDaylight.push_back(*plan);
            }
        }
        ++result.sections;
    }
    flush();
    if (result.sections < 2) {
        return makeError(ErrorCode::InvalidGeometry,
                         "fewer than two sections reach the ground: there is no surface to build",
                         std::to_string(result.incompleteSections) + " incomplete");
    }
    if (result.incompleteSections == 0) {
        input.boundary.vertices = leftDaylight;
        input.boundary.vertices.insert(input.boundary.vertices.end(), rightDaylight.rbegin(),
                                       rightDaylight.rend());
        input.boundary.closed = true;
    }

    terrain::TinBuildOptions options;
    // Points and breakline vertices coincide exactly by construction, with
    // equal elevations, so the default duplicate policy holds them to that;
    // crossing strings take the mean - see the header.
    options.crossingBreaklines = terrain::CrossingBreaklinePolicy::Average;
    auto built = terrain::buildTin(input, options);
    if (!built) {
        return built.error();
    }
    result.surface = std::move(built->surface);
    return result;
}

} // namespace katana::cad
