#pragma once

// Operations on whole arrays of points: transform in place, and bounds.
//
// Each is the loop a caller would write - math::transformPoint on every point,
// Box2::expand or AABB::expand with every point - and gives exactly that loop's
// result, bit for bit, whichever instruction set core::activeSimdLevel() picks.
// On AVX2 the arithmetic is four doubles an instruction; see
// docs/performance.md for what that is measured to be worth.
//
// Short arrays (a few points) go straight to the scalar loop: a string of six
// vertices cannot repay even a well-predicted dispatch.

#include <span>

#include "katana/geometry/primitives2d.hpp"
#include "katana/math/mat3.hpp"
#include "katana/math/mat4.hpp"
#include "katana/math/primitives.hpp"
#include "katana/math/vec3.hpp"

namespace katana::geometry {

// points[i] = transformPoint(m, points[i]) for every i, in place.
void transformPoints(const katana::math::Mat4& m, std::span<katana::math::Vec3> points);

// The 2D affine form: points[i] = transformPoint(m, points[i]).
void transformPoints(const katana::math::Mat3& m, std::span<Point2> points);

// The box that expand() of every point in order would build: empty for no
// points, NaN coordinates passed over, and where the extreme is a zero, the
// sign of the first zero met, as the sequential loop keeps it.
[[nodiscard]] Box2 boundsOf(std::span<const Point2> points);
[[nodiscard]] katana::math::AABB boundsOf(std::span<const katana::math::Vec3> points);

} // namespace katana::geometry
