#pragma once

// The single entry point for coordinate transformations between two CRSs
// (PLAN.MD Phase 11: "All CRS transformations must pass through a centralized
// API"). PROJ is an implementation detail hidden behind a pimpl (Rule 4).
//
// What create() does
//   PROJ is asked for every candidate operation between the two CRSs, ranked by
//   area of use, accuracy and grid availability. Katana selects the FIRST
//   candidate that can actually run on this installation and pins it: the same
//   operation is applied to every point, and it is described by operation().
//   Candidates that were passed over because a grid file is missing are listed
//   in candidates(); missingGrids() says which files would unlock a better one.
//
// Ballpark transformations
//   When PROJ knows no datum transformation between two datums it can fall back
//   to a "ballpark" operation that applies NO datum shift at all (errors of up
//   to hundreds of metres). Surveying cannot tolerate that silently, so create()
//   REFUSES ballpark operations unless TransformerOptions::allowBallpark is set.
//
// Axis order and units: see coordinate.hpp. Named coordinate types are checked
// against the kind of the CRS at run time (a GeographicCoordinate handed to a
// transformer whose source CRS is projected is an InvalidArgument error).
//
// Threading: a transformer owns a private PROJ context and is NOT thread-safe,
// not even for concurrent calls of the same transformer on different data -
// which is why the transform functions are non-const. Use one transformer per
// thread; clone() produces an independent, identically configured instance.
// Distinct transformers never share state.

#include <concepts>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geodesy/coordinate.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"

namespace katana::geodesy {

struct TransformerOptions {
    // Region the data lies in. Strongly recommended for datum transformations:
    // it lets PROJ rank the operation that is valid THERE first (e.g. the right
    // national grid) instead of the one with the widest coverage.
    std::optional<GeographicExtent> areaOfInterest;

    // Accept an operation containing a ballpark (no datum shift) step.
    bool allowBallpark = false;

    // Reject candidates whose declared accuracy is worse than this (metres).
    std::optional<double> requiredAccuracyMetres;

    // Coordinate epoch (decimal year, e.g. 2024.5) assumed for every coordinate
    // that carries none: all named coordinates, and Coordinates whose t is
    // kUnspecifiedEpoch. Time-dependent operations (OperationInfo::requiresEpoch,
    // e.g. ITRF2014 -> ETRF2000) REFUSE coordinates without an epoch: PROJ would
    // otherwise silently assume the reference epoch of the operation, which is
    // wrong by the plate motion accumulated since (about 2.5 cm per year).
    std::optional<double> coordinateEpoch;
};

// A grid (datum shift / geoid) file referenced by an operation.
struct GridFile {
    std::string name; // e.g. "us_noaa_conus.tif"
    std::string url;  // where PROJ says it can be downloaded; may be empty
    bool available = false;

    friend bool operator==(const GridFile&, const GridFile&) = default;
};

// Description of one candidate coordinate operation.
struct OperationInfo {
    std::string name;         // e.g. "Inverse of OSGB36 to WGS 84 (6) + British National Grid"
    std::string projPipeline; // PROJ pipeline text; empty when PROJ cannot express it
    std::string areaOfUse;    // human readable region of validity
    // As declared by the authority; 0 for a conversion (a map projection or unit
    // change is error free by definition); nullopt when genuinely unknown, which
    // is what a ballpark operation reports.
    std::optional<double> accuracyMetres;
    bool isBallpark = false;              // contains a step without datum shift
    bool isUsable = false;                // instantiable here: every required grid is installed
    bool requiresEpoch = false;           // time-dependent: each coordinate needs an epoch
    std::vector<GridFile> grids;          // grids the operation needs
};

template <typename T>
concept NamedCoordinate =
    std::same_as<T, GeographicCoordinate> || std::same_as<T, ProjectedCoordinate>;

class CoordinateTransformer {
  public:
    // Fails with
    //   Unsupported  - a LocalEngineering CRS is involved, no operation exists,
    //                  or the only operations are ballpark / need missing grids
    //                  (Error::context lists the grids);
    //   InvalidCRS   - a CRS definition no longer instantiates;
    //   NotFound     - proj.db is not installed.
    [[nodiscard]] static core::Result<CoordinateTransformer>
    create(const CoordinateReferenceSystem& source, const CoordinateReferenceSystem& target,
           const TransformerOptions& options = {});

    ~CoordinateTransformer();
    CoordinateTransformer(CoordinateTransformer&&) noexcept;
    CoordinateTransformer& operator=(CoordinateTransformer&&) noexcept;
    CoordinateTransformer(const CoordinateTransformer&) = delete;
    CoordinateTransformer& operator=(const CoordinateTransformer&) = delete;

    // Independent transformer with the same CRSs and options (its own PROJ
    // context). This is how a transformer is handed to another thread.
    [[nodiscard]] core::Result<CoordinateTransformer> clone() const;

    [[nodiscard]] const CoordinateReferenceSystem& source() const;
    [[nodiscard]] const CoordinateReferenceSystem& target() const;

    // The operation every transform call applies.
    [[nodiscard]] const OperationInfo& operation() const;
    // All candidates in PROJ's ranking order, usable or not.
    [[nodiscard]] const std::vector<OperationInfo>& candidates() const;
    // Missing grids of candidates that are MORE accurate than operation() (or of
    // any candidate when the accuracy of operation() is unknown). Empty means no
    // better result can be had by installing data.
    [[nodiscard]] const std::vector<GridFile>& missingGrids() const;

    // ---- single point, generic (traditional GIS order, CRS units) -------------
    // Errors: InvalidArgument for non-finite input or a coordinate PROJ rejects
    // (outside the projection domain, outside a grid, latitude beyond the pole);
    // the PROJ message is in Error::context. A failed point never yields numbers.
    [[nodiscard]] core::Result<Coordinate> forward(const Coordinate& coordinate);
    [[nodiscard]] core::Result<Coordinate> inverse(const Coordinate& coordinate);

    // ---- single point, named types ---------------------------------------------
    //   auto grid = transformer.forwardAs<ProjectedCoordinate>(GeographicCoordinate{lat, lon});
    // The source type must match the kind of the CRS the point is expressed in
    // and the target type the kind of the CRS it is converted to, otherwise
    // InvalidArgument. Geographic values are degrees whatever the angular unit
    // of the CRS; latitude must lie within [-90, 90].
    template <NamedCoordinate Target, NamedCoordinate Source>
    [[nodiscard]] core::Result<Target> forwardAs(const Source& coordinate);
    template <NamedCoordinate Target, NamedCoordinate Source>
    [[nodiscard]] core::Result<Target> inverseAs(const Source& coordinate);

    // ---- batch, in place --------------------------------------------------------
    // One PROJ call (proj_trans_generic) for the whole span. Results are
    // bit-identical to calling forward()/inverse() per point. Failed points get
    // x = y = z = NaN, t = kUnspecifiedEpoch and are counted in the report; the
    // Result itself only fails when the transformer is unusable (moved-from).
    [[nodiscard]] core::Result<BatchReport> forward(std::span<Coordinate> coordinates);
    [[nodiscard]] core::Result<BatchReport> inverse(std::span<Coordinate> coordinates);

    // ---- batch, named types -----------------------------------------------------
    // `output` must have the size of `input` (InvalidArgument otherwise). Failed
    // points have every field set to NaN. Name both types when passing
    // containers rather than spans, so that the conversion to span can happen:
    //   transformer.forwardAs<ProjectedCoordinate, GeographicCoordinate>(in, out);
    template <NamedCoordinate Target, NamedCoordinate Source>
    [[nodiscard]] core::Result<BatchReport> forwardAs(std::span<const Source> input,
                                                      std::span<Target> output);
    template <NamedCoordinate Target, NamedCoordinate Source>
    [[nodiscard]] core::Result<BatchReport> inverseAs(std::span<const Source> input,
                                                      std::span<Target> output);

  private:
    struct Impl;
    explicit CoordinateTransformer(std::unique_ptr<Impl> impl);

    std::unique_ptr<Impl> impl_;
};

} // namespace katana::geodesy
