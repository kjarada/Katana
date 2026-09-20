#pragma once

// Coordinate value types of the geodesy module.
//
// AXIS-ORDER CONVENTION (the classic PROJ / EPSG pitfall, fixed here once)
//
//   * Prefer the two NAMED types. A latitude can never be mistaken for a
//     longitude, or a northing for an easting, when the field has a name:
//
//       GeographicCoordinate{latitude, longitude, height}   angles in DEGREES
//       ProjectedCoordinate{easting, northing, height}      CRS axis unit
//
//     Degrees are decimal degrees, north and east positive. Eastings and
//     northings are in the linear unit of the CRS (metres, feet, US survey
//     feet ... see CoordinateReferenceSystem::horizontalUnitName()); they are
//     NOT silently converted to metres.
//
//   * `Coordinate{x, y, z, t}` is the generic escape hatch (geocentric XYZ,
//     bulk arrays). Its order is ALWAYS "traditional GIS order":
//
//       geographic CRS : x = longitude, y = latitude   (CRS angular unit)
//       projected  CRS : x = easting,   y = northing   (CRS linear unit)
//       geocentric CRS : x = X, y = Y, z = Z           (metres)
//
//     regardless of the axis order the authority declares (EPSG:4326 is
//     officially latitude-first; Katana still uses x = longitude). This is
//     obtained from PROJ with proj_normalize_for_visualization().
//     The normalisation reorders axes; it never flips their direction: for the
//     rare westing/southing systems (e.g. South African Lo grids) x carries the
//     westing and y the southing, positive as the CRS defines them.
//
//   * Heights travel in `height` / `z` in the unit of the CRS vertical axis and
//     are only transformed when both CRSs are three-dimensional; with 2D CRSs
//     they pass through unchanged.

#include <cstddef>
#include <limits>

namespace katana::geodesy {

// Value of Coordinate::t meaning "no coordinate epoch given". PROJ treats an
// infinite time as unspecified; a literal 0 would be the year 0 and would make a
// time-dependent (plate-motion) transformation drift by two millennia.
inline constexpr double kUnspecifiedEpoch = std::numeric_limits<double>::infinity();

// Position on an ellipsoid. Decimal degrees, north/east positive.
struct GeographicCoordinate {
    double latitude = 0.0;
    double longitude = 0.0;
    double height = 0.0; // above the ellipsoid (3D CRS) or passed through (2D CRS)

    friend constexpr bool operator==(const GeographicCoordinate&,
                                     const GeographicCoordinate&) = default;
};

// Position on a map grid, in the linear unit of the projected CRS.
struct ProjectedCoordinate {
    double easting = 0.0;
    double northing = 0.0;
    double height = 0.0;

    friend constexpr bool operator==(const ProjectedCoordinate&,
                                     const ProjectedCoordinate&) = default;
};

// Generic coordinate tuple in traditional GIS order (see the header comment).
// `t` is the coordinate epoch in decimal years, used only by time-dependent
// datum transformations.
struct Coordinate {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
    double t = kUnspecifiedEpoch;

    friend constexpr bool operator==(const Coordinate&, const Coordinate&) = default;
};

// The batch transformation hands interior pointers to PROJ with
// sizeof(Coordinate) as the stride.
static_assert(sizeof(Coordinate) == 4 * sizeof(double), "Coordinate must be four packed doubles");

// Outcome of a batch transformation. A point that PROJ cannot transform is NOT
// an error of the whole batch: its x, y and z are set to quiet NaN (so that it
// can never be mistaken for a position) and it is counted here. Callers must
// inspect `failed`.
struct BatchReport {
    std::size_t succeeded = 0;
    std::size_t failed = 0;
    std::size_t firstFailedIndex = 0; // meaningful only when failed > 0

    [[nodiscard]] bool allSucceeded() const { return failed == 0; }
};

} // namespace katana::geodesy
