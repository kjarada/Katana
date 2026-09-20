#pragma once

// Reference ellipsoid parameters.

namespace katana::geodesy {

// Ellipsoid of revolution. `inverseFlattening` == 0 denotes a sphere (the usual
// EPSG / PROJ convention, since 1/f is infinite there).
struct Ellipsoid {
    double semiMajorAxis = 0.0;     // a, metres
    double inverseFlattening = 0.0; // 1/f, dimensionless; 0 for a sphere

    [[nodiscard]] constexpr double flattening() const
    {
        return inverseFlattening == 0.0 ? 0.0 : 1.0 / inverseFlattening;
    }
    [[nodiscard]] constexpr double semiMinorAxis() const
    {
        return semiMajorAxis * (1.0 - flattening());
    }
    // First eccentricity squared, e^2 = f (2 - f).
    [[nodiscard]] constexpr double eccentricitySquared() const
    {
        return flattening() * (2.0 - flattening());
    }

    // Defining constants as published by the respective authorities.
    [[nodiscard]] static constexpr Ellipsoid wgs84() { return {6378137.0, 298.257223563}; }
    [[nodiscard]] static constexpr Ellipsoid grs80() { return {6378137.0, 298.257222101}; }

    friend constexpr bool operator==(const Ellipsoid&, const Ellipsoid&) = default;
};

} // namespace katana::geodesy
