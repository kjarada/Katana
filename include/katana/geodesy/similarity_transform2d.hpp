#pragma once

// Local coordinates: 2D conformal (four-parameter Helmert / similarity)
// transformation and its least-squares determination from control points - the
// "site calibration" that ties a local engineering grid to a projected CRS.
//
//     X = tx + a x - b y          a = s cos(theta)
//     Y = ty + b x + a y          b = s sin(theta)
//
// s is the scale and theta the rotation, COUNTER-CLOCKWISE positive in the
// (x = easting, y = northing) plane, consistent with the rest of Katana. A grid
// bearing (clockwise from north) therefore changes by -theta.
//
// Numerical design (see docs/geodesy.md for the derivation)
//   * The transformation is stored about two origins,
//         target = targetOrigin + M (source - sourceOrigin),   M = [a -b; b a],
//     which fit() places at the centroids of the control. Differences of
//     neighbouring coordinates are formed first, so points at UTM magnitudes
//     (1e6..1e7 m) keep their full precision: the transformed coordinate is
//     accurate to about one unit in its last place (~1e-9 m).
//   * The least-squares solution is closed form; with centred coordinates the
//     translation decouples and the 2x2 normal matrix is diagonal, so there is
//     nothing to invert and no conditioning problem.
//   * Reflections are not modelled. Control with swapped axes (E/N confusion)
//     shows up as residuals of the size of the site.

#include <optional>
#include <span>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/math/vec2.hpp"

namespace katana::geodesy {

// The same ground mark in both systems. x = easting, y = northing.
struct ControlPointPair {
    katana::math::Vec2 source;
    katana::math::Vec2 target;
};

class SimilarityTransform2D {
  public:
    SimilarityTransform2D() = default; // identity

    // Fails with InvalidArgument unless scale is finite and > 0 and the other
    // parameters are finite. `translation` is (tx, ty) of the equations above.
    [[nodiscard]] static core::Result<SimilarityTransform2D>
    fromParameters(double scale, double rotationRadians, const katana::math::Vec2& translation);

    // Exact reconstruction of a stored transformation from a(), b() and the two
    // origins (persisting scale/rotation/translation instead would not round
    // trip bit-exactly). InvalidArgument unless everything is finite and
    // (a, b) != (0, 0).
    [[nodiscard]] static core::Result<SimilarityTransform2D>
    fromCoefficients(double a, double b, const katana::math::Vec2& sourceOrigin,
                     const katana::math::Vec2& targetOrigin);

    [[nodiscard]] katana::math::Vec2 apply(const katana::math::Vec2& source) const;
    [[nodiscard]] katana::math::Vec2 applyInverse(const katana::math::Vec2& target) const;
    [[nodiscard]] SimilarityTransform2D inverse() const;

    [[nodiscard]] double scale() const;
    [[nodiscard]] double rotation() const; // radians in (-pi, pi], counter-clockwise
    [[nodiscard]] double a() const { return a_; }
    [[nodiscard]] double b() const { return b_; }

    // (tx, ty): the image of the source origin (0, 0). For display and export;
    // at UTM magnitudes it absorbs the rounding of M * sourceOrigin (~1e-9 m),
    // which apply() itself avoids.
    [[nodiscard]] katana::math::Vec2 translation() const;

    [[nodiscard]] const katana::math::Vec2& sourceOrigin() const { return sourceOrigin_; }
    [[nodiscard]] const katana::math::Vec2& targetOrigin() const { return targetOrigin_; }

  private:
    katana::math::Vec2 sourceOrigin_;
    katana::math::Vec2 targetOrigin_;
    double a_ = 1.0;
    double b_ = 0.0;
};

struct SimilarityFit2D {
    SimilarityTransform2D transform;

    // residuals[i] = pairs[i].target - transform.apply(pairs[i].source), in
    // target units: what must be added to a transformed point to reach its
    // control value.
    std::vector<katana::math::Vec2> residuals;

    double rms = 0.0;         // sqrt( sum |residual|^2 / n ), horizontal, target units
    double maxResidual = 0.0; // largest |residual|

    // A-posteriori standard deviation of one coordinate,
    // sqrt( sum |residual|^2 / (2n - 4) ). nullopt for n == 2, where the four
    // parameters fit exactly and nothing is left to judge the control by.
    std::optional<double> coordinateStdDev;
};

// Equal-weight least-squares fit. Errors (all InvalidArgument):
//   * fewer than two pairs, or a non-finite coordinate;
//   * source (or target) points that all coincide within
//     math::tolerance::kCoordinate - scale and rotation are then undetermined;
//   * control that fits no proper similarity (best scale collapses to zero,
//     typically mirrored point sets).
[[nodiscard]] core::Result<SimilarityFit2D> fitSimilarity2D(std::span<const ControlPointPair> pairs);

} // namespace katana::geodesy
