#pragma once

// Plane grid position used by every survey calculation.
//
// Deliberately not math::Vec2: a Vec2 is (x, y) with counter-clockwise angles,
// survey work is (northing, easting) with clockwise azimuths. Naming the fields
// keeps the two conventions from being mixed up silently.

namespace katana::survey {

struct Coordinate2 {
    double northing = 0.0; // metres
    double easting = 0.0;  // metres

    friend constexpr bool operator==(const Coordinate2&, const Coordinate2&) = default;
};

} // namespace katana::survey
