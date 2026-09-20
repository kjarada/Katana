#include "katana/geodesy/coordinate_transformer.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <stdexcept>
#include <type_traits>
#include <utility>

#include "katana/math/numerics.hpp"
#include "proj_internal.hpp"

namespace katana::geodesy {

namespace {

using detail::PjFactoryContextPtr;
using detail::PjListPtr;
using detail::PjPtr;
using detail::ProjContext;
using detail::toStdString;

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// Number of candidate operations spelled out in an error message.
constexpr std::size_t kMaxCandidatesInError = 4;

OperationInfo describeOperation(ProjContext& context, const PJ* operation)
{
    PJ_CONTEXT* ctx = context.get();
    OperationInfo info;
    info.name = toStdString(proj_get_name(operation));

    const double accuracy = proj_coordoperation_get_accuracy(ctx, operation);
    if (accuracy >= 0.0) { // PROJ reports a negative value for "unknown"
        info.accuracyMetres = accuracy;
    } else if (proj_get_type(operation) == PJ_TYPE_CONVERSION) {
        // A conversion relates two CRSs that share a datum by an exact formula -
        // a map projection, a unit or axis change - so it introduces no
        // positional uncertainty (ISO 19111; EPSG Guidance Note 7-2 calls a
        // conversion error free, which is why the EPSG registry declares no
        // accuracy for one and PROJ's C API answers "unknown"). The declared
        // accuracy is nevertheless zero, and reporting nullopt here would make
        // "exact" indistinguishable from a ballpark operation, whose error
        // genuinely is unknown.
        info.accuracyMetres = 0.0;
    }
    info.isBallpark = proj_coordoperation_has_ballpark_transformation(ctx, operation) != 0;
    info.isUsable = proj_coordoperation_is_instantiable(ctx, operation) != 0;
    info.requiresEpoch =
        proj_coordoperation_requires_per_coordinate_input_time(ctx, operation) != 0;

    const char* areaName = nullptr;
    if (proj_get_area_of_use(ctx, operation, nullptr, nullptr, nullptr, nullptr, &areaName) != 0) {
        info.areaOfUse = toStdString(areaName);
    }

    const int gridCount = proj_coordoperation_get_grid_used_count(ctx, operation);
    for (int index = 0; index < gridCount; ++index) {
        const char* shortName = nullptr;
        const char* url = nullptr;
        int available = 0;
        if (proj_coordoperation_get_grid_used(ctx, operation, index, &shortName, nullptr, nullptr,
                                              &url, nullptr, nullptr, &available) != 0) {
            info.grids.push_back(GridFile{toStdString(shortName), toStdString(url), available != 0});
        }
    }

    info.projPipeline = toStdString(proj_as_proj_string(ctx, operation, PJ_PROJ_5, nullptr));
    context.clearDiagnostics(); // probing unusable candidates is expected to log
    return info;
}

std::string describeCandidates(const std::vector<OperationInfo>& candidates)
{
    std::string text;
    const std::size_t shown = std::min(candidates.size(), kMaxCandidatesInError);
    for (std::size_t index = 0; index < shown; ++index) {
        const OperationInfo& candidate = candidates[index];
        text += "candidate '" + candidate.name + "'";
        for (const GridFile& grid : candidate.grids) {
            if (!grid.available) {
                text += " needs missing grid " + grid.name;
                if (!grid.url.empty()) {
                    text += " (" + grid.url + ")";
                }
            }
        }
        text += "; ";
    }
    if (candidates.size() > shown) {
        text += std::to_string(candidates.size() - shown) + " more candidate(s); ";
    }
    return text;
}

core::Status validateAreaOfInterest(const GeographicExtent& area)
{
    const bool finite = std::isfinite(area.west) && std::isfinite(area.south) &&
                        std::isfinite(area.east) && std::isfinite(area.north);
    if (!finite || area.south > area.north || area.south < -90.0 || area.north > 90.0 ||
        std::abs(area.west) > 180.0 || std::abs(area.east) > 180.0) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "area of interest must be finite degrees with south <= north, "
                               "latitudes in [-90, 90] and longitudes in [-180, 180]");
    }
    return {};
}

// CRS angular units per degree: exactly 1 for the (overwhelmingly common)
// degree-based CRS so that such coordinates pass through bit-exactly; e.g. 10/9
// for the grad-based French NTF systems.
double unitsPerDegree(const CoordinateReferenceSystem& crs)
{
    if (!crs.isGeographic() ||
        katana::math::nearlyEqual(crs.horizontalUnitToSi(), katana::math::kDegToRad)) {
        return 1.0;
    }
    return katana::math::kDegToRad / crs.horizontalUnitToSi();
}

// ---- named coordinate plumbing ----------------------------------------------
// x / y below are PROJ's first and second coordinate in normalised
// (traditional GIS) order: longitude or easting first.

double& xOf(GeographicCoordinate& c)
{
    return c.longitude;
}
double& yOf(GeographicCoordinate& c)
{
    return c.latitude;
}
double& xOf(ProjectedCoordinate& c)
{
    return c.easting;
}
double& yOf(ProjectedCoordinate& c)
{
    return c.northing;
}

template <typename Named> constexpr bool kIsGeographic = std::is_same_v<Named, GeographicCoordinate>;

template <typename Named> bool matchesKind(const CoordinateReferenceSystem& crs)
{
    return kIsGeographic<Named> ? crs.isGeographic() : crs.isProjected();
}

template <typename Named> std::string_view typeName()
{
    return kIsGeographic<Named> ? "GeographicCoordinate" : "ProjectedCoordinate";
}

template <typename Named>
core::Status checkKind(const CoordinateReferenceSystem& crs, std::string_view role)
{
    if (matchesKind<Named>(crs)) {
        return {};
    }
    return core::makeError(core::ErrorCode::InvalidArgument,
                           std::string(typeName<Named>()) + " does not match the " +
                               std::string(role) + " CRS",
                           "crs='" + crs.name() + "' kind=" + std::string(toString(crs.kind())));
}

bool isValidInput(const GeographicCoordinate& c)
{
    return std::isfinite(c.latitude) && std::isfinite(c.longitude) && std::isfinite(c.height) &&
           std::abs(c.latitude) <= 90.0;
}

bool isValidInput(const ProjectedCoordinate& c)
{
    return std::isfinite(c.easting) && std::isfinite(c.northing) && std::isfinite(c.height);
}

template <typename Named> core::Status checkInput(const Named& coordinate)
{
    if (isValidInput(coordinate)) {
        return {};
    }
    return core::makeError(core::ErrorCode::InvalidArgument,
                           kIsGeographic<Named>
                               ? "geographic coordinate must be finite with latitude in [-90, 90]"
                               : "projected coordinate must be finite");
}

template <typename Named> Named failedPoint()
{
    Named failed;
    xOf(failed) = kNaN;
    yOf(failed) = kNaN;
    failed.height = kNaN;
    return failed;
}

} // namespace

// ---- Impl --------------------------------------------------------------------

struct CoordinateTransformer::Impl {
    Impl(CoordinateReferenceSystem sourceIn, CoordinateReferenceSystem targetIn,
         TransformerOptions optionsIn)
        : source(std::move(sourceIn)), target(std::move(targetIn)), options(std::move(optionsIn))
    {
    }

    CoordinateReferenceSystem source;
    CoordinateReferenceSystem target;
    TransformerOptions options;

    // Declaration order matters: the PJ must be destroyed before its context.
    std::unique_ptr<ProjContext> context;
    PjPtr operation; // the selected candidate, normalised to traditional GIS order

    OperationInfo selected;
    std::vector<OperationInfo> candidates;
    std::vector<GridFile> missingGrids;

    double sourceUnitsPerDegree = 1.0;
    double targetUnitsPerDegree = 1.0;

    [[nodiscard]] const CoordinateReferenceSystem& crsFrom(PJ_DIRECTION direction) const
    {
        return direction == PJ_FWD ? source : target;
    }
    [[nodiscard]] const CoordinateReferenceSystem& crsTo(PJ_DIRECTION direction) const
    {
        return direction == PJ_FWD ? target : source;
    }
    [[nodiscard]] double unitsPerDegreeFrom(PJ_DIRECTION direction) const
    {
        return direction == PJ_FWD ? sourceUnitsPerDegree : targetUnitsPerDegree;
    }
    [[nodiscard]] double unitsPerDegreeTo(PJ_DIRECTION direction) const
    {
        return direction == PJ_FWD ? targetUnitsPerDegree : sourceUnitsPerDegree;
    }

    // Epoch handed to PROJ: the coordinate's own, else the configured default.
    [[nodiscard]] double effectiveEpoch(double t) const
    {
        return t == kUnspecifiedEpoch && options.coordinateEpoch ? *options.coordinateEpoch : t;
    }
    // Shared by the single-point and batch paths so that both refuse exactly
    // the same inputs. `epoch` is the effective epoch.
    [[nodiscard]] bool isTransformable(const Coordinate& c, double epoch) const
    {
        // +infinity is kUnspecifiedEpoch; NaN and -infinity are never valid.
        const bool epochValid =
            !std::isnan(epoch) && epoch != -std::numeric_limits<double>::infinity();
        const bool epochPresent = !selected.requiresEpoch || epoch != kUnspecifiedEpoch;
        return std::isfinite(c.x) && std::isfinite(c.y) && std::isfinite(c.z) && epochValid &&
               epochPresent;
    }

    void collectMissingGrids();
    core::Result<Coordinate> transformOne(PJ_DIRECTION direction, const Coordinate& coordinate);
    BatchReport transformMany(PJ_DIRECTION direction, std::span<Coordinate> coordinates);

    template <typename Target, typename Source>
    core::Result<Target> transformNamed(PJ_DIRECTION direction, const Source& coordinate);
    template <typename Target, typename Source>
    core::Result<BatchReport> transformNamedMany(PJ_DIRECTION direction,
                                                 std::span<const Source> input,
                                                 std::span<Target> output);
};

void CoordinateTransformer::Impl::collectMissingGrids()
{
    for (const OperationInfo& candidate : candidates) {
        const bool moreAccurate =
            !selected.accuracyMetres.has_value() ||
            (candidate.accuracyMetres.has_value() &&
             *candidate.accuracyMetres < *selected.accuracyMetres);
        if (candidate.isUsable || !moreAccurate) {
            continue;
        }
        for (const GridFile& grid : candidate.grids) {
            const bool known = std::any_of(missingGrids.begin(), missingGrids.end(),
                                           [&](const GridFile& g) { return g.name == grid.name; });
            if (!grid.available && !known) {
                missingGrids.push_back(grid);
            }
        }
    }
}

core::Result<Coordinate> CoordinateTransformer::Impl::transformOne(PJ_DIRECTION direction,
                                                                   const Coordinate& coordinate)
{
    const double epoch = effectiveEpoch(coordinate.t);
    if (!isTransformable(coordinate, epoch)) {
        if (selected.requiresEpoch && epoch == kUnspecifiedEpoch) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the operation is time-dependent and the coordinate has no "
                                   "epoch; set Coordinate::t or TransformerOptions::coordinateEpoch",
                                   "operation='" + selected.name + "'");
        }
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "coordinate to transform must be finite");
    }

    context->clearDiagnostics();
    proj_errno_reset(operation.get());
    const PJ_COORD result =
        proj_trans(operation.get(), direction,
                   proj_coord(coordinate.x, coordinate.y, coordinate.z, epoch));
    const int projErrno = proj_errno(operation.get());

    // PROJ signals failure with HUGE_VAL (infinity) coordinates and/or an errno.
    // Either one means there is no trustworthy number to hand back.
    if (projErrno != 0 || !std::isfinite(result.xyzt.x) || !std::isfinite(result.xyzt.y) ||
        !std::isfinite(result.xyzt.z)) {
        std::string reason = projErrno != 0 ? detail::describeErrno(*context, projErrno)
                                            : std::string("PROJ returned no coordinate");
        const std::string log = context->consumeDiagnostics();
        if (!log.empty()) {
            reason += "; " + log;
        }
        reason += "; operation='" + selected.name + "'";
        proj_errno_reset(operation.get());
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "coordinate cannot be transformed (outside the domain of the "
                               "operation or of its grids)",
                               std::move(reason));
    }
    return Coordinate{result.xyzt.x, result.xyzt.y, result.xyzt.z, result.xyzt.t};
}

BatchReport CoordinateTransformer::Impl::transformMany(PJ_DIRECTION direction,
                                                       std::span<Coordinate> coordinates)
{
    BatchReport report;
    if (coordinates.empty()) {
        return report;
    }
    constexpr std::size_t kStride = sizeof(Coordinate);
    const std::size_t count = coordinates.size();
    Coordinate* first = coordinates.data();

    // Refuse what transformOne() refuses. PROJ answers an x of HUGE_VAL with its
    // error coordinate without evaluating anything, which the loop below turns
    // into a counted failure.
    for (Coordinate& c : coordinates) {
        c.t = effectiveEpoch(c.t);
        if (!isTransformable(c, c.t)) {
            c.x = std::numeric_limits<double>::infinity();
        }
    }

    proj_errno_reset(operation.get());
    proj_trans_generic(operation.get(), direction, &first->x, kStride, count, &first->y, kStride,
                       count, &first->z, kStride, count, &first->t, kStride, count);

    for (std::size_t index = 0; index < count; ++index) {
        Coordinate& c = coordinates[index];
        if (std::isfinite(c.x) && std::isfinite(c.y) && std::isfinite(c.z)) {
            ++report.succeeded;
            continue;
        }
        c = Coordinate{kNaN, kNaN, kNaN, kUnspecifiedEpoch};
        if (report.failed == 0) {
            report.firstFailedIndex = index;
        }
        ++report.failed;
    }
    proj_errno_reset(operation.get());
    context->clearDiagnostics();
    return report;
}

template <typename Target, typename Source>
core::Result<Target> CoordinateTransformer::Impl::transformNamed(PJ_DIRECTION direction,
                                                                 const Source& coordinate)
{
    if (auto status = checkKind<Source>(crsFrom(direction), "originating"); !status) {
        return status.error();
    }
    if (auto status = checkKind<Target>(crsTo(direction), "destination"); !status) {
        return status.error();
    }
    if (auto status = checkInput(coordinate); !status) {
        return status.error();
    }

    Source in = coordinate;
    const double inScale = unitsPerDegreeFrom(direction);
    auto result =
        transformOne(direction, Coordinate{xOf(in) * inScale, yOf(in) * inScale, in.height});
    if (!result) {
        return result.error();
    }
    const double outScale = unitsPerDegreeTo(direction);
    Target out;
    xOf(out) = result->x / outScale;
    yOf(out) = result->y / outScale;
    out.height = result->z;
    return out;
}

template <typename Target, typename Source>
core::Result<BatchReport>
CoordinateTransformer::Impl::transformNamedMany(PJ_DIRECTION direction,
                                                std::span<const Source> input,
                                                std::span<Target> output)
{
    if (input.size() != output.size()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "input and output spans must have the same size",
                               "input=" + std::to_string(input.size()) +
                                   " output=" + std::to_string(output.size()));
    }
    if (auto status = checkKind<Source>(crsFrom(direction), "originating"); !status) {
        return status.error();
    }
    if (auto status = checkKind<Target>(crsTo(direction), "destination"); !status) {
        return status.error();
    }
    if (selected.requiresEpoch && !options.coordinateEpoch) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the operation is time-dependent and named coordinates carry no "
                               "epoch; set TransformerOptions::coordinateEpoch",
                               "operation='" + selected.name + "'");
    }
    BatchReport report;
    if (input.empty()) {
        return report;
    }

    const double inScale = unitsPerDegreeFrom(direction);
    const double outScale = unitsPerDegreeTo(direction);
    const std::size_t count = input.size();
    for (std::size_t index = 0; index < count; ++index) {
        Source in = input[index];
        Target& out = output[index];
        xOf(out) = xOf(in) * inScale;
        yOf(out) = yOf(in) * inScale;
        out.height = in.height;
    }

    // A single broadcast epoch (length-1 array): without it PROJ would
    // substitute t = 0, i.e. the year 0, into time-dependent operations.
    double epoch = effectiveEpoch(kUnspecifiedEpoch);
    constexpr std::size_t kStride = sizeof(Target);
    Target* first = output.data();
    proj_errno_reset(operation.get());
    proj_trans_generic(operation.get(), direction, &xOf(*first), kStride, count, &yOf(*first),
                       kStride, count, &first->height, kStride, count, &epoch, 0, 1);

    for (std::size_t index = 0; index < count; ++index) {
        Target& out = output[index];
        const bool ok = isValidInput(input[index]) && std::isfinite(xOf(out)) &&
                        std::isfinite(yOf(out)) && std::isfinite(out.height);
        if (ok) {
            xOf(out) /= outScale;
            yOf(out) /= outScale;
            ++report.succeeded;
            continue;
        }
        out = failedPoint<Target>();
        if (report.failed == 0) {
            report.firstFailedIndex = index;
        }
        ++report.failed;
    }
    proj_errno_reset(operation.get());
    context->clearDiagnostics();
    return report;
}

// ---- construction ------------------------------------------------------------

CoordinateTransformer::CoordinateTransformer(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
CoordinateTransformer::~CoordinateTransformer() = default;
CoordinateTransformer::CoordinateTransformer(CoordinateTransformer&&) noexcept = default;
CoordinateTransformer& CoordinateTransformer::operator=(CoordinateTransformer&&) noexcept = default;

core::Result<CoordinateTransformer>
CoordinateTransformer::create(const CoordinateReferenceSystem& source,
                              const CoordinateReferenceSystem& target,
                              const TransformerOptions& options)
{
    const std::string pairText = "source='" + source.name() + "' target='" + target.name() + "'";

    if (source.kind() == CrsKind::LocalEngineering || target.kind() == CrsKind::LocalEngineering) {
        return core::makeError(core::ErrorCode::Unsupported,
                               "a local engineering CRS has no geodetic datum and cannot be "
                               "transformed by PROJ; relate it to a projected CRS with a site "
                               "calibration (fitSimilarity2D)",
                               pairText);
    }
    if (options.areaOfInterest) {
        if (auto status = validateAreaOfInterest(*options.areaOfInterest); !status) {
            return status.error();
        }
    }
    if (options.requiredAccuracyMetres &&
        !(std::isfinite(*options.requiredAccuracyMetres) && *options.requiredAccuracyMetres > 0.0)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "required accuracy must be a positive number of metres");
    }

    if (options.coordinateEpoch && !std::isfinite(*options.coordinateEpoch)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "coordinate epoch must be a finite decimal year");
    }

    auto impl = std::make_unique<Impl>(source, target, options);
    auto context = ProjContext::create();
    if (!context) {
        return context.error();
    }
    impl->context = std::move(*context);
    ProjContext& projContext = *impl->context;
    PJ_CONTEXT* ctx = projContext.get();

    auto sourceCrs = detail::createCrs(projContext, source.definition());
    if (!sourceCrs) {
        return sourceCrs.error();
    }
    auto targetCrs = detail::createCrs(projContext, target.definition());
    if (!targetCrs) {
        return targetCrs.error();
    }

    const PjFactoryContextPtr factory(proj_create_operation_factory_context(ctx, nullptr));
    if (!factory) {
        return core::makeError(core::ErrorCode::Internal,
                               "PROJ could not create an operation factory",
                               projContext.consumeDiagnostics());
    }
    // Partial intersection keeps operations whose area of use covers only part
    // of the CRS extents (most national datum transformations). Operations with
    // missing grids stay in the list - ranked after the usable ones - so that
    // they can be reported instead of silently disappearing.
    proj_operation_factory_context_set_spatial_criterion(
        ctx, factory.get(), PROJ_SPATIAL_CRITERION_PARTIAL_INTERSECTION);
    proj_operation_factory_context_set_grid_availability_use(
        ctx, factory.get(), PROJ_GRID_AVAILABILITY_USED_FOR_SORTING);
    proj_operation_factory_context_set_allow_ballpark_transformations(
        ctx, factory.get(), options.allowBallpark ? 1 : 0);
    if (options.areaOfInterest) {
        const GeographicExtent& area = *options.areaOfInterest;
        proj_operation_factory_context_set_area_of_interest(ctx, factory.get(), area.west,
                                                            area.south, area.east, area.north);
    }
    if (options.requiredAccuracyMetres) {
        proj_operation_factory_context_set_desired_accuracy(ctx, factory.get(),
                                                            *options.requiredAccuracyMetres);
    }

    projContext.clearDiagnostics();
    const PjListPtr operations(
        proj_create_operations(ctx, sourceCrs->get(), targetCrs->get(), factory.get()));
    if (!operations) {
        return core::makeError(core::ErrorCode::Unsupported,
                               "PROJ could not search for coordinate operations",
                               pairText + "; " + projContext.consumeDiagnostics());
    }

    PjPtr chosen;
    const int count = proj_list_get_count(operations.get());
    for (int index = 0; index < count; ++index) {
        PjPtr candidate(proj_list_get(ctx, operations.get(), index));
        if (!candidate) {
            continue;
        }
        impl->candidates.push_back(describeOperation(projContext, candidate.get()));
        if (!chosen && impl->candidates.back().isUsable) {
            impl->selected = impl->candidates.back();
            chosen = std::move(candidate);
        }
    }

    if (!chosen) {
        std::string reason = pairText + "; " + describeCandidates(impl->candidates);
        if (!options.allowBallpark) {
            reason += "ballpark operations (no datum shift) are disabled; set "
                      "TransformerOptions::allowBallpark to accept one";
        }
        return core::makeError(core::ErrorCode::Unsupported,
                               "no usable coordinate operation between the two CRSs",
                               std::move(reason));
    }

    projContext.clearDiagnostics();
    impl->operation.reset(proj_normalize_for_visualization(ctx, chosen.get()));
    if (!impl->operation) {
        return core::makeError(core::ErrorCode::Internal,
                               "PROJ could not normalise the axis order of the operation",
                               pairText + "; " + projContext.consumeDiagnostics());
    }
    // Report the pipeline that really runs, axis swaps and unit conversions included.
    impl->selected.projPipeline =
        toStdString(proj_as_proj_string(ctx, impl->operation.get(), PJ_PROJ_5, nullptr));

    impl->collectMissingGrids();
    impl->sourceUnitsPerDegree = unitsPerDegree(source);
    impl->targetUnitsPerDegree = unitsPerDegree(target);
    projContext.clearDiagnostics();
    return CoordinateTransformer(std::move(impl));
}

core::Result<CoordinateTransformer> CoordinateTransformer::clone() const
{
    if (!impl_) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "cannot clone a moved-from CoordinateTransformer");
    }
    // Re-running the (deterministic) selection in a fresh context is the only
    // way to obtain PROJ objects that share nothing with this instance.
    return create(impl_->source, impl_->target, impl_->options);
}

// ---- accessors ---------------------------------------------------------------

namespace {

[[noreturn]] void throwMovedFrom()
{
    throw std::logic_error("katana::geodesy::CoordinateTransformer used after being moved from");
}

core::Error movedFromError()
{
    return core::makeError(core::ErrorCode::InvalidState,
                           "CoordinateTransformer used after being moved from");
}

} // namespace

const CoordinateReferenceSystem& CoordinateTransformer::source() const
{
    if (!impl_) {
        throwMovedFrom();
    }
    return impl_->source;
}

const CoordinateReferenceSystem& CoordinateTransformer::target() const
{
    if (!impl_) {
        throwMovedFrom();
    }
    return impl_->target;
}

const OperationInfo& CoordinateTransformer::operation() const
{
    if (!impl_) {
        throwMovedFrom();
    }
    return impl_->selected;
}

const std::vector<OperationInfo>& CoordinateTransformer::candidates() const
{
    if (!impl_) {
        throwMovedFrom();
    }
    return impl_->candidates;
}

const std::vector<GridFile>& CoordinateTransformer::missingGrids() const
{
    if (!impl_) {
        throwMovedFrom();
    }
    return impl_->missingGrids;
}

// ---- transformation entry points ----------------------------------------------

core::Result<Coordinate> CoordinateTransformer::forward(const Coordinate& coordinate)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformOne(PJ_FWD, coordinate);
}

core::Result<Coordinate> CoordinateTransformer::inverse(const Coordinate& coordinate)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformOne(PJ_INV, coordinate);
}

core::Result<BatchReport> CoordinateTransformer::forward(std::span<Coordinate> coordinates)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformMany(PJ_FWD, coordinates);
}

core::Result<BatchReport> CoordinateTransformer::inverse(std::span<Coordinate> coordinates)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformMany(PJ_INV, coordinates);
}

template <NamedCoordinate Target, NamedCoordinate Source>
core::Result<Target> CoordinateTransformer::forwardAs(const Source& coordinate)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformNamed<Target>(PJ_FWD, coordinate);
}

template <NamedCoordinate Target, NamedCoordinate Source>
core::Result<Target> CoordinateTransformer::inverseAs(const Source& coordinate)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformNamed<Target>(PJ_INV, coordinate);
}

template <NamedCoordinate Target, NamedCoordinate Source>
core::Result<BatchReport> CoordinateTransformer::forwardAs(std::span<const Source> input,
                                                           std::span<Target> output)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformNamedMany<Target, Source>(PJ_FWD, input, output);
}

template <NamedCoordinate Target, NamedCoordinate Source>
core::Result<BatchReport> CoordinateTransformer::inverseAs(std::span<const Source> input,
                                                           std::span<Target> output)
{
    if (!impl_) {
        return movedFromError();
    }
    return impl_->transformNamedMany<Target, Source>(PJ_INV, input, output);
}

// The member templates are defined here (they need PROJ); instantiate every
// combination the NamedCoordinate concept admits.
#define KATANA_GEODESY_INSTANTIATE(TARGET, SOURCE)                                                 \
    template core::Result<TARGET> CoordinateTransformer::forwardAs<TARGET, SOURCE>(const SOURCE&); \
    template core::Result<TARGET> CoordinateTransformer::inverseAs<TARGET, SOURCE>(const SOURCE&); \
    template core::Result<BatchReport> CoordinateTransformer::forwardAs<TARGET, SOURCE>(           \
        std::span<const SOURCE>, std::span<TARGET>);                                               \
    template core::Result<BatchReport> CoordinateTransformer::inverseAs<TARGET, SOURCE>(           \
        std::span<const SOURCE>, std::span<TARGET>);

KATANA_GEODESY_INSTANTIATE(ProjectedCoordinate, GeographicCoordinate)
KATANA_GEODESY_INSTANTIATE(GeographicCoordinate, ProjectedCoordinate)
KATANA_GEODESY_INSTANTIATE(GeographicCoordinate, GeographicCoordinate)
KATANA_GEODESY_INSTANTIATE(ProjectedCoordinate, ProjectedCoordinate)

#undef KATANA_GEODESY_INSTANTIATE

} // namespace katana::geodesy
