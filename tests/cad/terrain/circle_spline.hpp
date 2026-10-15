#pragma once

// A spline that is exactly a circle, for the tests of the closed outlines the
// drawing's polygons are read from (test_drawing_shapes.cpp, test_volume_verbs.cpp).

#include <cmath>
#include <cstddef>

#include "katana/geometry/curves2d.hpp"

namespace katana::cad::testing {

// A rational quadratic spline that is a circle of radius `radius` about
// `centre`: the nine control points of the square round the circle, weights 1
// and sqrt(1/2) alternately, the knots at the quarters (Piegl and Tiller, The
// NURBS Book, 2nd edition, section 7.5, the full circle). It starts and ends at
// (centre.x + radius, centre.y) and runs counter-clockwise, so it is a closed
// shape (Spline2::isClosedShape) of area pi radius^2.
inline katana::geometry::Spline2 circleSpline(const katana::geometry::Point2& centre, double radius)
{
    katana::geometry::Spline2 spline;
    spline.degree = 2;
    const double corner = std::sqrt(0.5);
    constexpr double offsets[9][2] = {{1, 0},  {1, 1},   {0, 1},  {-1, 1}, {-1, 0},
                                      {-1, -1}, {0, -1}, {1, -1}, {1, 0}};
    for (std::size_t i = 0; i < 9; ++i) {
        spline.controlPoints.emplace_back(centre.x + radius * offsets[i][0],
                                          centre.y + radius * offsets[i][1]);
        spline.weights.push_back(i % 2 == 1 ? corner : 1.0);
    }
    spline.knots = {0, 0, 0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1, 1, 1};
    return spline;
}

} // namespace katana::cad::testing
