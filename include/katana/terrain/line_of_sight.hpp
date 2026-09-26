#pragma once

// Line of sight between two points over the ground (docs/terrain.md,
// "Viewshed and line of sight"): whether a target can be seen from an
// observer, where the ground first hides it, and how far above the ground
// the sight passes at its lowest. What the LOS verb reports.
//
// Native, not GDAL's: no algorithm of GDAL's answers it (`raster viewshed`
// answers every cell from one observer, not one sight line with its
// clearance). The ground is a callback, so the same walk reads a TIN on its
// triangles or a raster through GDAL's interpolation, and this module never
// sees either.
//
// The walk: the observer's eye is `observerHeight` above the ground under
// it, the aim `targetHeight` above the ground under the target, and the
// sight the straight line between them. It is checked at stations `step`
// apart (the last spacing shortened so they end exactly at the target),
// between the ends only. A station's clearance is the sight's height less
// the ground's there; the target is hidden where the clearance first falls
// below zero (a grazing sight, clearance exactly zero, still sees). A
// station off the ground cannot hide anything: it is counted as unknown,
// never read as ground at 0.
//
// Earth curvature and refraction lower the ground by `curvature` x d^2 /
// 12 741 994 m at a distance d from the observer - GDAL's viewshed rule and
// sphere diameter, so a sight line and a viewshed of the same ground agree.
// 0 turns it off; GDAL's default is 0.85714 (refraction of 1/7).

#include <cstddef>
#include <functional>
#include <optional>
#include <stop_token>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::terrain {

// The height of the ground at a plan position; none off it.
using GroundAt = std::function<std::optional<double>(const katana::geometry::Point2&)>;

// The sphere GDAL's viewshed corrects heights over (gdal_viewshed:
// "Sphere_Diameter", 12 741 994 m, the Earth's mean diameter).
inline constexpr double kEarthDiameter = 12'741'994.0;
// GDAL's default curvature-and-refraction coefficient: 1 - 1/7.
inline constexpr double kDefaultCurvature = 0.85714;

struct SightOptions {
    double observerHeight = 1.7; // eye above the ground under the observer
    double targetHeight = 0.0;   // aim above the ground under the target
    double step = 1.0;           // between stations, > 0
    double curvature = 0.0;      // coefficient, >= 0; 0 off
};

struct SightLine {
    bool visible = true;
    double distance = 0.0;  // plan, observer to target
    double observerZ = 0.0; // the eye
    double targetZ = 0.0;   // the aim, curvature applied
    // The first station where the ground hides the target.
    std::optional<katana::geometry::Point2> blockedAt;
    std::optional<double> blockedDistance; // from the observer
    std::optional<double> blockedGround;   // the ground there, curvature applied
    // The least clearance between the ends, where it is, and how far out;
    // none when no station between them had ground.
    std::optional<double> clearance;
    std::optional<katana::geometry::Point2> clearanceAt;
    std::optional<double> clearanceDistance;
    std::size_t stations = 0; // between the ends
    std::size_t unknown = 0;  // of which off the ground
};

// InvalidArgument for coincident ends, a step that is not positive and
// finite (or makes more than 10 million stations), a curvature below 0 or
// heights that are not finite, and - naming which - for an observer or a
// target off the ground. InvalidState "cancelled" when `stop` is asked.
[[nodiscard]] katana::core::Result<SightLine>
lineOfSight(const GroundAt& ground, const katana::geometry::Point2& observer,
            const katana::geometry::Point2& target, const SightOptions& options,
            const std::stop_token& stop = {});

} // namespace katana::terrain
