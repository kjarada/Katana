#pragma once

// Shared by the katana_interop sources, never installed: how many chords a
// curve is written as, so that EXPORT and the geoprocessing bindings
// (geo/drawing_dataset.cpp) turn an arc into the same polyline.

#include <algorithm>
#include <cmath>

namespace katana::interop::detail {

// Number of chords needed so a circular arc of `radius` spanning `sweep` never
// deviates from the polyline by more than `tolerance`.
//
// The sagitta of a chord subtending an angle phi is r * (1 - cos(phi/2)), so the
// largest admissible phi is 2 * acos(1 - tolerance / r). When the tolerance is
// at or beyond the radius the whole arc is within tolerance of a single chord
// and the formula degenerates, so that case is handled separately rather than
// left to produce a NaN.
[[nodiscard]] inline int chordCount(double radius, double sweep, double tolerance)
{
    const double absSweep = std::abs(sweep);
    if (!(radius > 0.0) || !(absSweep > 0.0)) {
        return 1;
    }
    if (!(tolerance > 0.0)) {
        return 64; // a caller that asked for zero error gets a fine default
    }
    if (tolerance >= radius) {
        return 1;
    }
    const double maxAngle = 2.0 * std::acos(1.0 - tolerance / radius);
    if (!(maxAngle > 0.0) || !std::isfinite(maxAngle)) {
        return 4096;
    }
    const auto count = static_cast<int>(std::ceil(absSweep / maxAngle));
    return std::clamp(count, 1, 4096);
}

} // namespace katana::interop::detail
