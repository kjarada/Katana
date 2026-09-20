#pragma once

// Grid scale factor and grid convergence of a projected CRS at a point - the
// quantities a surveyor needs to move between grid and ground:
//
//     grid distance  = ellipsoidal distance * pointScaleFactor
//     grid bearing   = geodetic azimuth - convergence   (+ arc-to-chord, long lines)
//
// Values come from PROJ's proj_factors(), which differentiates the projection
// numerically (relative accuracy around 1e-9; see docs/geodesy.md).
//
// Threading: like CoordinateTransformer, a calculator owns a private PROJ
// context, is not thread-safe (at() is non-const for that reason) and is given
// to another thread with clone().

#include <memory>

#include "katana/core/error.hpp"
#include "katana/geodesy/coordinate.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"

namespace katana::geodesy {

struct GridFactors {
    double meridionalScale = 1.0; // h: scale along the meridian
    double parallelScale = 1.0;   // k: scale along the parallel
    double arealScale = 1.0;      // s: area distortion
    double angularDistortionDegrees = 0.0; // omega: maximum angular distortion

    // Grid convergence (gamma): the geodetic azimuth of GRID north, i.e. the
    // angle at the point from true north clockwise to grid north. Positive east
    // of the central meridian in the northern hemisphere, approximately
    // (lon - lon0) * sin(lat).
    double convergenceDegrees = 0.0;

    // The projection is conformal (every survey grid is: Transverse Mercator,
    // Lambert Conformal Conic, stereographic, oblique Mercator) when h == k and
    // the angular distortion vanishes; the point scale factor is then the same
    // in every direction and equals h. h is preferred over k because k divides
    // by cos(latitude) and degrades towards the poles.
    [[nodiscard]] double pointScaleFactor() const { return meridionalScale; }
};

class GridFactorCalculator {
  public:
    // `projected` must be of kind Projected (InvalidArgument otherwise).
    [[nodiscard]] static core::Result<GridFactorCalculator>
    create(const CoordinateReferenceSystem& projected);

    ~GridFactorCalculator();
    GridFactorCalculator(GridFactorCalculator&&) noexcept;
    GridFactorCalculator& operator=(GridFactorCalculator&&) noexcept;
    GridFactorCalculator(const GridFactorCalculator&) = delete;
    GridFactorCalculator& operator=(const GridFactorCalculator&) = delete;

    [[nodiscard]] core::Result<GridFactorCalculator> clone() const;

    [[nodiscard]] const CoordinateReferenceSystem& crs() const;

    // Factors at a position given on the BASE geographic CRS of the projected
    // CRS (same datum; longitude relative to that CRS's prime meridian, which is
    // Greenwich for practically every modern system). InvalidArgument when the
    // position is invalid or outside the domain of the projection.
    [[nodiscard]] core::Result<GridFactors> at(const GeographicCoordinate& position);

    // Factors at a grid position: the projection is inverted first.
    [[nodiscard]] core::Result<GridFactors> at(const ProjectedCoordinate& position);

  private:
    struct Impl;
    explicit GridFactorCalculator(std::unique_ptr<Impl> impl);

    std::unique_ptr<Impl> impl_;
};

} // namespace katana::geodesy
