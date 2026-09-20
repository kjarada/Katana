#include "katana/geodesy/similarity_transform2d.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "katana/math/numerics.hpp"

namespace katana::geodesy {

namespace {

using katana::math::Vec2;
namespace tol = katana::math::tolerance;

// M v with M = [a -b; b a].
Vec2 rotateScale(double a, double b, const Vec2& v)
{
    return Vec2(a * v.x - b * v.y, b * v.x + a * v.y);
}

} // namespace

core::Result<SimilarityTransform2D>
SimilarityTransform2D::fromParameters(double scale, double rotationRadians, const Vec2& translation)
{
    if (!std::isfinite(scale) || scale <= 0.0 || !std::isfinite(rotationRadians) ||
        !translation.isFinite()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "similarity parameters must be finite with a positive scale");
    }
    return fromCoefficients(scale * std::cos(rotationRadians), scale * std::sin(rotationRadians),
                            Vec2{}, translation);
}

core::Result<SimilarityTransform2D>
SimilarityTransform2D::fromCoefficients(double a, double b, const Vec2& sourceOrigin,
                                        const Vec2& targetOrigin)
{
    const bool finite =
        std::isfinite(a) && std::isfinite(b) && sourceOrigin.isFinite() && targetOrigin.isFinite();
    if (!finite || (a == 0.0 && b == 0.0)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "similarity coefficients must be finite and not both zero");
    }
    SimilarityTransform2D transform;
    transform.a_ = a;
    transform.b_ = b;
    transform.sourceOrigin_ = sourceOrigin;
    transform.targetOrigin_ = targetOrigin;
    return transform;
}

Vec2 SimilarityTransform2D::apply(const Vec2& source) const
{
    // Difference first: for nearby points it is (nearly) exact, and only the
    // small site-sized vector is scaled and rotated.
    return targetOrigin_ + rotateScale(a_, b_, source - sourceOrigin_);
}

Vec2 SimilarityTransform2D::applyInverse(const Vec2& target) const
{
    // M^-1 = [a b; -b a] / (a^2 + b^2).
    const double determinant = a_ * a_ + b_ * b_;
    return sourceOrigin_ + rotateScale(a_ / determinant, -b_ / determinant, target - targetOrigin_);
}

SimilarityTransform2D SimilarityTransform2D::inverse() const
{
    const double determinant = a_ * a_ + b_ * b_;
    SimilarityTransform2D result;
    result.a_ = a_ / determinant;
    result.b_ = -b_ / determinant;
    result.sourceOrigin_ = targetOrigin_;
    result.targetOrigin_ = sourceOrigin_;
    return result;
}

double SimilarityTransform2D::scale() const
{
    return std::hypot(a_, b_);
}

double SimilarityTransform2D::rotation() const
{
    return std::atan2(b_, a_);
}

Vec2 SimilarityTransform2D::translation() const
{
    return targetOrigin_ - rotateScale(a_, b_, sourceOrigin_);
}

core::Result<SimilarityFit2D> fitSimilarity2D(std::span<const ControlPointPair> pairs)
{
    const std::size_t count = pairs.size();
    if (count < 2) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "a 2D similarity fit needs at least two control point pairs",
                               "pairs=" + std::to_string(count));
    }
    for (std::size_t index = 0; index < count; ++index) {
        if (!pairs[index].source.isFinite() || !pairs[index].target.isFinite()) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "control point coordinates must be finite",
                                   "pair index=" + std::to_string(index));
        }
    }
    const double n = static_cast<double>(count);

    // Origins at the centroids. The centroid is accumulated relative to the
    // first point so that the sums hold site-sized numbers, not 1e7-sized ones.
    Vec2 sourceMean;
    Vec2 targetMean;
    for (const ControlPointPair& pair : pairs) {
        sourceMean += pair.source - pairs.front().source;
        targetMean += pair.target - pairs.front().target;
    }
    const Vec2 sourceOrigin = pairs.front().source + sourceMean / n;
    const Vec2 targetOrigin = pairs.front().target + targetMean / n;

    // The stored origins are rounded to the coordinate grid (~1e-9 m), so the
    // reduced coordinates do not sum to exactly zero. Remove what is left; it is
    // folded back into the target origin below.
    Vec2 sourceResidualMean;
    Vec2 targetResidualMean;
    for (const ControlPointPair& pair : pairs) {
        sourceResidualMean += pair.source - sourceOrigin;
        targetResidualMean += pair.target - targetOrigin;
    }
    sourceResidualMean /= n;
    targetResidualMean /= n;

    // Normal equations in centred coordinates (x, y) -> (X, Y):
    //   a = sum(x X + y Y) / sum(x^2 + y^2),  b = sum(x Y - y X) / sum(x^2 + y^2).
    double sumSquares = 0.0;
    double sumDot = 0.0;
    double sumCross = 0.0;
    double sourceSpread = 0.0;
    double targetSpread = 0.0;
    for (const ControlPointPair& pair : pairs) {
        const Vec2 s = (pair.source - sourceOrigin) - sourceResidualMean;
        const Vec2 t = (pair.target - targetOrigin) - targetResidualMean;
        sumSquares += s.lengthSquared();
        sumDot += s.dot(t);
        sumCross += s.cross(t);
        sourceSpread = std::max(sourceSpread, s.length());
        targetSpread = std::max(targetSpread, t.length());
    }
    if (sourceSpread <= tol::kCoordinate || targetSpread <= tol::kCoordinate) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "control points coincide; scale and rotation are undetermined",
                               "source spread=" + std::to_string(sourceSpread) +
                                   " target spread=" + std::to_string(targetSpread));
    }

    const double a = sumDot / sumSquares;
    const double b = sumCross / sumSquares;
    if (std::hypot(a, b) * sourceSpread <= tol::kCoordinate) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "control points fit no proper similarity (best scale is zero); "
                               "check for mirrored or mismatched point pairs");
    }

    auto transform = SimilarityTransform2D::fromCoefficients(
        a, b, sourceOrigin,
        targetOrigin + (targetResidualMean - rotateScale(a, b, sourceResidualMean)));
    if (!transform) {
        return transform.error();
    }

    SimilarityFit2D fit;
    fit.transform = *transform;
    fit.residuals.reserve(count);
    double sumResidualSquares = 0.0;
    for (const ControlPointPair& pair : pairs) {
        const Vec2 residual = pair.target - fit.transform.apply(pair.source);
        sumResidualSquares += residual.lengthSquared();
        fit.maxResidual = std::max(fit.maxResidual, residual.length());
        fit.residuals.push_back(residual);
    }
    fit.rms = std::sqrt(sumResidualSquares / n);
    if (count > 2) {
        fit.coordinateStdDev = std::sqrt(sumResidualSquares / (2.0 * n - 4.0));
    }
    return fit;
}

} // namespace katana::geodesy
