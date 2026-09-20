#pragma once

// Shared fixtures for the terrain tests. Everything here produces INPUT; expected
// values are derived analytically inside the tests, never from the module.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <utility>
#include <vector>

#include "katana/terrain/tin_builder.hpp"
#include "support/property.hpp"

namespace katana::terrain::testing {

struct Plane {
    double a = 0.0; // dz/dx
    double b = 0.0; // dz/dy
    double c = 0.0;
    [[nodiscard]] double at(double x, double y) const { return a * x + b * y + c; }
};

// The four corners of [x0, x0 + size] x [y0, y0 + size] plus `interior` random
// points strictly inside, all lifted onto `plane`. The corners make the convex
// hull the square itself, so the plan area is known exactly.
inline std::vector<Point3> planePoints(const Plane& plane, double size, std::size_t interior,
                                       katana::test::Random& random, double x0 = 0.0,
                                       double y0 = 0.0)
{
    std::vector<Point3> points;
    const double xs[4] = {x0, x0 + size, x0 + size, x0};
    const double ys[4] = {y0, y0, y0 + size, y0 + size};
    for (int i = 0; i < 4; ++i) {
        points.emplace_back(xs[i], ys[i], plane.at(xs[i], ys[i]));
    }
    for (std::size_t i = 0; i < interior; ++i) {
        const double x = random.real(x0 + 0.01 * size, x0 + 0.99 * size);
        const double y = random.real(y0 + 0.01 * size, y0 + 0.99 * size);
        points.emplace_back(x, y, plane.at(x, y));
    }
    return points;
}

// Regular grid of (count x count) points with unit spacing starting at the
// origin, lifted onto `plane`. Grid points are cocircular in fours: the
// degenerate case for Delaunay, deliberately.
inline std::vector<Point3> gridPoints(const Plane& plane, int count)
{
    std::vector<Point3> points;
    for (int j = 0; j < count; ++j) {
        for (int i = 0; i < count; ++i) {
            points.emplace_back(i, j, plane.at(i, j));
        }
    }
    return points;
}

// Builds or fails the calling test; returns the empty surface on failure.
inline TinBuildResult build(const TinInput& input, const TinBuildOptions& options = {})
{
    auto result = buildTin(input, options);
    if (!result.ok()) {
        ADD_FAILURE() << "buildTin failed: " << result.error().describe();
        return TinBuildResult{};
    }
    return std::move(result).value();
}

inline TinSurface buildFromPoints(std::vector<Point3> points)
{
    TinInput input;
    input.points = std::move(points);
    return build(input).surface;
}

// Square pyramid: base [0, base]^2 at z = 0, apex above the centre at z = height.
// The four ridges are breaklines, so extra points on the faces cannot make a
// triangle straddle a ridge and the TIN is the pyramid exactly.
inline TinInput pyramidInput(double base, double height, std::size_t extraFacePoints,
                             katana::test::Random& random)
{
    const double half = 0.5 * base;
    TinInput input;
    input.points = {Point3(0, 0, 0), Point3(base, 0, 0), Point3(base, base, 0),
                    Point3(0, base, 0), Point3(half, half, height)};
    for (int corner = 0; corner < 4; ++corner) {
        input.breaklines.push_back(Breakline{{input.points[static_cast<std::size_t>(corner)],
                                              input.points[4]},
                                             false});
    }
    for (std::size_t i = 0; i < extraFacePoints; ++i) {
        const double x = random.real(0.0, base);
        const double y = random.real(0.0, base);
        // Height of the pyramid: falls linearly with the Chebyshev distance from the centre.
        const double distance = std::max(std::abs(x - half), std::abs(y - half));
        input.points.emplace_back(x, y, height * (1.0 - distance / half));
    }
    return input;
}

} // namespace katana::terrain::testing
