#include "katana/geodesy/grid_factors.hpp"

#include <cmath>
#include <stdexcept>
#include <utility>

#include "katana/math/numerics.hpp"
#include "proj_internal.hpp"

namespace katana::geodesy {

namespace {

using detail::PjPtr;
using detail::ProjContext;

} // namespace

struct GridFactorCalculator::Impl {
    explicit Impl(CoordinateReferenceSystem crsIn) : crs(std::move(crsIn)) {}

    CoordinateReferenceSystem crs;

    // Declaration order matters: PJ objects must be destroyed before their context.
    std::unique_ptr<ProjContext> context;
    PjPtr projected;      // the projected CRS (a BoundCRS wrapper removed)
    PjPtr toGeographic;   // projected -> base geographic CRS, normalised lon/lat order
    double radiansPerGeographicUnit = katana::math::kDegToRad;

    core::Result<GridFactors> evaluate(double longitudeRadians, double latitudeRadians);
};

core::Result<GridFactors> GridFactorCalculator::Impl::evaluate(double longitudeRadians,
                                                               double latitudeRadians)
{
    PJ_COORD position = proj_coord(0.0, 0.0, 0.0, 0.0);
    position.lp.lam = longitudeRadians;
    position.lp.phi = latitudeRadians;

    context->clearDiagnostics();
    proj_errno_reset(projected.get());
    const PJ_FACTORS factors = proj_factors(projected.get(), position);
    const int projErrno = proj_errno(projected.get());

    // On failure PROJ returns an all-zero struct and sets the errno.
    if (projErrno != 0 || !std::isfinite(factors.meridional_scale) ||
        !(factors.meridional_scale > 0.0) || !std::isfinite(factors.meridian_convergence)) {
        std::string reason = projErrno != 0 ? detail::describeErrno(*context, projErrno)
                                            : std::string("PROJ returned no factors");
        const std::string log = context->consumeDiagnostics();
        if (!log.empty()) {
            reason += "; " + log;
        }
        proj_errno_reset(projected.get());
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "grid factors cannot be evaluated at this position (outside the "
                               "domain of the projection)",
                               std::move(reason));
    }

    GridFactors result;
    result.meridionalScale = factors.meridional_scale;
    result.parallelScale = factors.parallel_scale;
    result.arealScale = factors.areal_scale;
    result.angularDistortionDegrees = factors.angular_distortion * katana::math::kRadToDeg;
    result.convergenceDegrees = factors.meridian_convergence * katana::math::kRadToDeg;
    return result;
}

GridFactorCalculator::GridFactorCalculator(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
GridFactorCalculator::~GridFactorCalculator() = default;
GridFactorCalculator::GridFactorCalculator(GridFactorCalculator&&) noexcept = default;
GridFactorCalculator& GridFactorCalculator::operator=(GridFactorCalculator&&) noexcept = default;

core::Result<GridFactorCalculator>
GridFactorCalculator::create(const CoordinateReferenceSystem& projected)
{
    if (projected.kind() != CrsKind::Projected) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "grid factors are defined for projected CRSs only",
                               "crs='" + projected.name() +
                                   "' kind=" + std::string(toString(projected.kind())));
    }

    auto impl = std::make_unique<Impl>(projected);
    auto context = ProjContext::create();
    if (!context) {
        return context.error();
    }
    impl->context = std::move(*context);
    ProjContext& projContext = *impl->context;
    PJ_CONTEXT* ctx = projContext.get();

    auto object = detail::createCrs(projContext, projected.definition());
    if (!object) {
        return object.error();
    }
    impl->projected = std::move(*object);
    if (proj_get_type(impl->projected.get()) == PJ_TYPE_BOUND_CRS) {
        impl->projected.reset(proj_get_source_crs(ctx, impl->projected.get()));
    }

    // Inverse of the map projection alone: projected CRS -> its own base
    // geographic CRS. No datum change is involved, so there is exactly one
    // candidate operation.
    const PjPtr geographic(impl->projected
                               ? proj_crs_get_geodetic_crs(ctx, impl->projected.get())
                               : nullptr);
    PjPtr inverseProjection;
    if (geographic) {
        const PjPtr raw(proj_create_crs_to_crs_from_pj(ctx, impl->projected.get(),
                                                       geographic.get(), nullptr, nullptr));
        if (raw) {
            inverseProjection.reset(proj_normalize_for_visualization(ctx, raw.get()));
        }
    }
    if (!inverseProjection) {
        return core::makeError(core::ErrorCode::InvalidCRS,
                               "PROJ could not derive the base geographic CRS of the projected CRS",
                               "crs='" + projected.name() + "'; " +
                                   projContext.consumeDiagnostics());
    }
    impl->toGeographic = std::move(inverseProjection);

    // Angular unit of the base CRS (grads for the French NTF systems).
    const PjPtr system(proj_crs_get_coordinate_system(ctx, geographic.get()));
    double unitToRadians = katana::math::kDegToRad;
    if (!system || proj_cs_get_axis_info(ctx, system.get(), 0, nullptr, nullptr, nullptr,
                                         &unitToRadians, nullptr, nullptr, nullptr) == 0) {
        return core::makeError(core::ErrorCode::InvalidCRS,
                               "PROJ could not report the angular unit of the base geographic CRS",
                               "crs='" + projected.name() + "'; " +
                                   projContext.consumeDiagnostics());
    }
    // Same snapping as the transformer: a degree is exactly math::kDegToRad.
    impl->radiansPerGeographicUnit =
        katana::math::nearlyEqual(unitToRadians, katana::math::kDegToRad)
            ? katana::math::kDegToRad
            : unitToRadians;

    projContext.clearDiagnostics();
    return GridFactorCalculator(std::move(impl));
}

core::Result<GridFactorCalculator> GridFactorCalculator::clone() const
{
    if (!impl_) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "cannot clone a moved-from GridFactorCalculator");
    }
    return create(impl_->crs);
}

const CoordinateReferenceSystem& GridFactorCalculator::crs() const
{
    if (!impl_) {
        throw std::logic_error("katana::geodesy::GridFactorCalculator used after being moved from");
    }
    return impl_->crs;
}

core::Result<GridFactors> GridFactorCalculator::at(const GeographicCoordinate& position)
{
    if (!impl_) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "GridFactorCalculator used after being moved from");
    }
    if (!std::isfinite(position.latitude) || !std::isfinite(position.longitude) ||
        std::abs(position.latitude) > 90.0) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "geographic position must be finite with latitude in [-90, 90]");
    }
    return impl_->evaluate(position.longitude * katana::math::kDegToRad,
                           position.latitude * katana::math::kDegToRad);
}

core::Result<GridFactors> GridFactorCalculator::at(const ProjectedCoordinate& position)
{
    if (!impl_) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "GridFactorCalculator used after being moved from");
    }
    if (!std::isfinite(position.easting) || !std::isfinite(position.northing)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "projected position must be finite");
    }

    PJ* inverse = impl_->toGeographic.get();
    impl_->context->clearDiagnostics();
    proj_errno_reset(inverse);
    const PJ_COORD geographic = proj_trans(
        inverse, PJ_FWD, proj_coord(position.easting, position.northing, 0.0, kUnspecifiedEpoch));
    const int projErrno = proj_errno(inverse);
    if (projErrno != 0 || !std::isfinite(geographic.xy.x) || !std::isfinite(geographic.xy.y)) {
        std::string reason = projErrno != 0 ? detail::describeErrno(*impl_->context, projErrno)
                                            : std::string("PROJ returned no coordinate");
        proj_errno_reset(inverse);
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "grid position cannot be converted to latitude/longitude",
                               std::move(reason));
    }
    return impl_->evaluate(geographic.xy.x * impl_->radiansPerGeographicUnit,
                           geographic.xy.y * impl_->radiansPerGeographicUnit);
}

} // namespace katana::geodesy
