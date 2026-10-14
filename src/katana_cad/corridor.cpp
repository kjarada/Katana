#include "katana/cad/corridor.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "katana/cad/road/road_section.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/primitives3d.hpp"
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

namespace {

// The engine's section in the old shape: the finished line from daylight to
// daylight once both batters met the ground, the ground under it when it was
// found everywhere, and the areas when both were.
CorridorSection oldSection(const road::RoadSection& section)
{
    CorridorSection out;
    out.station = section.station;
    out.centre = section.centre;
    out.designElevation = section.designLevel;
    out.complete = section.complete;
    if (section.leftReached && section.rightReached) {
        for (const road::SectionPoint& point : section.top) {
            out.design.push_back(SectionVertex{point.offset, point.level});
        }
    }
    out.ground = section.ground;
    out.cutArea = section.cutArea;
    out.fillArea = section.fillArea;
    return out;
}

// The sections corridorQuantities and corridorSurface are made from, with
// their refusals: the assembly, the interval, and an alignment and profile
// that do not overlap.
Result<std::vector<road::RoadSection>> assemblySections(const geometry::SolvedAlignment& alignment,
                                                        const geometry::SolvedProfile& profile,
                                                        const Assembly& assembly,
                                                        const terrain::TinSurface& ground,
                                                        double interval, double& start,
                                                        double& end)
{
    if (auto status = validateAssembly(assembly); !status) {
        return status.error();
    }
    if (!(interval > 0.0) || !std::isfinite(interval)) {
        return makeError(ErrorCode::InvalidArgument, "the interval must be positive");
    }
    start = std::max(alignment.startStation(), profile.startStation());
    end = std::min(alignment.endStation(), profile.endStation());
    if (!(end - start > tol::kCoordinate)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the alignment and the design profile do not overlap in station");
    }
    // Every interval station, and every key station of the alignment and
    // the profile inside the range - the quantities change character at a
    // TS, a PVC and a PVT, and a report that stepped over them would be one
    // nobody could check.
    std::vector<double> keys = profile.keyStations();
    for (const double key : alignment.keyStations()) {
        keys.push_back(key);
    }
    const road::RoadTemplate roadTemplate = road::templateFromAssembly(assembly);
    std::vector<road::RoadSection> sections;
    for (const double station : road::sectionStations(start, end, interval, keys)) {
        auto section = road::roadSection(roadTemplate, alignment, profile, &ground, station);
        if (!section) {
            return section.error();
        }
        sections.push_back(std::move(*section));
    }
    return sections;
}

} // namespace

// One engine (cad/road/road_section.hpp): the assembly is a template like any
// other (road::templateFromAssembly), checked first in its own words.
Result<CorridorSection> corridorSection(const geometry::SolvedAlignment& alignment,
                                        const geometry::SolvedProfile& profile,
                                        const Assembly& assembly,
                                        const terrain::TinSurface& ground, double station)
{
    if (auto status = validateAssembly(assembly); !status) {
        return status.error();
    }
    auto section = road::roadSection(road::templateFromAssembly(assembly), alignment, profile,
                                     &ground, station);
    if (!section) {
        return section.error();
    }
    return oldSection(*section);
}

Result<CorridorQuantities> corridorQuantities(const geometry::SolvedAlignment& alignment,
                                              const geometry::SolvedProfile& profile,
                                              const Assembly& assembly,
                                              const terrain::TinSurface& ground, double interval)
{
    CorridorQuantities result;
    auto sections = assemblySections(alignment, profile, assembly, ground, interval,
                                     result.startStation, result.endStation);
    if (!sections) {
        return sections.error();
    }
    result.sections.reserve(sections->size());
    for (const road::RoadSection& section : *sections) {
        if (!section.complete) {
            ++result.incompleteSections;
        }
        result.sections.push_back(oldSection(section));
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
    double start = 0.0;
    double end = 0.0;
    auto sections = assemblySections(alignment, profile, assembly, ground, interval, start, end);
    if (!sections) {
        return sections.error();
    }
    std::vector<const road::RoadSection*> pointers;
    for (const road::RoadSection& section : *sections) {
        pointers.push_back(&section);
    }
    // Complete sections only, the strings broken where one is left out and
    // no boundary then, as this function always built it.
    auto surface = road::sectionsSurface(pointers, alignment, false, true);
    if (!surface) {
        return surface.error();
    }
    CorridorSurface result;
    result.surface = std::move(surface->surface);
    result.sections = surface->sections;
    result.incompleteSections = surface->leftOut;
    return result;
}

} // namespace katana::cad
