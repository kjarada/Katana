#pragma once

// What the terrain verbs share (docs/terrain.md, "Surfaces on every front
// end"; docs/geoprocessing.md, T0 to T3): the surface records, the one way a
// surface is triangulated from a raster and kept in the store (which TO
// SURFACE uses too), and the reading of their options.
//
//   SURFACE LIST [JSON] | INFO <name> | REMOVE <name>
//   SURFACE FROM <source> [NAME <n>] [max=<points>] [AREA x0,y0,x1,y1] [classes=2,...]
//   SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] [co=K=V]...
//   CONTOUR, RASTER SHADE, RASTER SLOPE, RASTER ASPECT (contour_verbs.cpp,
//   shade_verbs.cpp, slope_verbs.cpp)
//
// Surfaces are session data, as reference rasters are: not entities, not
// undoable, not saved in the project (terrain/surface_store.hpp).

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/geo/contour_entities.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_builder.hpp"

namespace katana::app::geo {

// The most points a surface is triangulated from when nothing else is said:
// the triangulation of 400 000 points took 1.7 s on the window's job
// (docs/terrain.md, "Jobs"), and a ground surface does not carry two million
// points of information. SURFACE FROM ... max= changes it.
inline constexpr std::size_t kSurfacePointCap = 400'000;

// surface name=ground triangles=2 points=4 bounds=0.000,0.000,10.000,10.000
//         zmin=100.000 zmax=101.000 source="drawing (4 entities)"
[[nodiscard]] std::string surfaceRecord(const katana::terrain::NamedSurface& surface);

// A surface triangulated and not yet kept: made by a job's work, kept by its
// apply. `records` say how it was made - how a raster was sampled, how many
// coincident points were merged - and come before the surface's own record.
struct SurfaceBuild {
    std::shared_ptr<const katana::terrain::TinSurface> surface;
    std::vector<std::string> records;
};

// Triangulated, coincident points averaged (a scan lands on one mark again
// and again, millimetres apart in height). InvalidArgument for fewer than
// three points; what the builder refused otherwise.
[[nodiscard]] katana::core::Result<SurfaceBuild>
triangulate(katana::terrain::TinInput input, const katana::terrain::TinBuildOptions& options);

// The heights of the raster at `path`, sampled on the stride that keeps them
// under `maxPoints` (interop::readRasterElevations, the true band values),
// then triangulated. `area` cuts the raster first.
[[nodiscard]] katana::core::Result<SurfaceBuild>
surfaceFromRasterFile(const std::filesystem::path& path, std::size_t maxPoints,
                      const std::optional<katana::geometry::Box2>& area);

// `build` kept in the context's store as `name`, the front end told. When
// `unique` the name is made unique ("name (2)"), else a name in use is
// AlreadyExists. The records of the build, then the surface's.
[[nodiscard]] katana::core::Result<std::string> keepSurface(Context& context, std::string name,
                                                            std::string source, bool unique,
                                                            SurfaceBuild&& build);

// ---- the words the terrain verbs read ------------------------------------------------------

// `key=value` at `i`, unquoted, the key lower-cased; nothing for any other
// word.
[[nodiscard]] std::optional<std::pair<std::string, std::string>> keyValue(const Tokens& tokens,
                                                                          std::size_t i);

// A finite number, or InvalidArgument naming the option: "interval is a
// number".
[[nodiscard]] katana::core::Result<double> numberOption(std::string_view key, std::string_view value);
// The same, greater than zero.
[[nodiscard]] katana::core::Result<double> positiveOption(std::string_view key,
                                                          std::string_view value);

// The records joined a line each.
[[nodiscard]] std::string joinedRecords(const std::vector<std::string>& records);

// A Prepared with nothing to run: the reply is the answer.
[[nodiscard]] Prepared answeredWith(std::string title, std::string reply);

// ---- the raster steps and areas the analysis verbs share (terrain_steps.cpp) ----------------

// A raster handed from one GDAL step to the next - clip, smooth, contour;
// hillshade, colour, blend; slope, reclassify, polygonize. Each step's raster
// is written to a tiled GeoTIFF in the chain's own folder, never held whole
// in memory, its bands, colours and no-data as GDAL wrote them; the folder is
// removed with the chain, so a failed or cancelled run leaves nothing. Made
// and used on one thread, the job's: it shares no dataset with any other run.
class RasterChain {
  public:
    // `input` is the first step's; `scratch` the folder the chain's own is
    // made in.
    RasterChain(katana::gis::processing::DatasetValue input, const std::filesystem::path& scratch);
    ~RasterChain();
    RasterChain(const RasterChain&) = delete;
    RasterChain& operator=(const RasterChain&) = delete;

    // The algorithm at `path` on the current raster (its `input`), `values`
    // bound before GDAL reads `tokens`; the raster it writes is the current
    // one after. What gis::processing::run refuses, including "cancelled".
    [[nodiscard]] katana::core::Status
    step(const std::vector<std::string>& path, std::vector<std::string> tokens,
         const std::stop_token& stop, const Progress& progress = {},
         std::vector<std::pair<std::string, katana::gis::processing::ArgValue>> values = {});

    // The algorithm at `path` on the current raster for the features it
    // writes (raster contour, raster polygonize); the current raster stays.
    [[nodiscard]] katana::core::Result<katana::gis::processing::FeatureSet>
    features(const std::vector<std::string>& path, std::vector<std::string> tokens,
             const std::stop_token& stop, const Progress& progress = {});

    [[nodiscard]] const katana::gis::processing::DatasetValue& current() const { return current_; }
    // The current raster's facts, read from its file or its grid.
    [[nodiscard]] katana::core::Result<katana::gis::RasterInfo> info() const;
    // The file the last step wrote, moved out of the chain's folder to
    // `destination` (made unique if a file is there): kept when the chain
    // goes. InvalidState before any step.
    [[nodiscard]] katana::core::Result<std::filesystem::path>
    keep(const std::filesystem::path& destination);
    // `text` written to a file `name` of the chain's own, removed with it:
    // a colour map a step reads by name.
    [[nodiscard]] katana::core::Result<std::filesystem::path> write(const std::string& name,
                                                                    const std::string& text);

    // GDAL's warnings of every step, in order.
    std::vector<katana::gis::processing::Diagnostic> warnings;
    double seconds = 0.0;

  private:
    katana::gis::processing::DatasetValue current_;
    std::filesystem::path folder_;
    bool stepped_ = false;
};

// A raster dataset's facts: a file's read through GDAL, a grid's as it is.
[[nodiscard]] katana::core::Result<katana::gis::RasterInfo>
rasterInfoOf(const katana::gis::processing::DatasetValue& dataset);

// The side of a square, unrotated cell; nothing for any other.
[[nodiscard]] std::optional<double> squareCellOf(const katana::gis::RasterInfo& info);

// The ground a raster covers: the box of its four corners.
[[nodiscard]] katana::geometry::Box2 rasterExtent(const katana::gis::RasterInfo& info);

// The least and greatest values of band 1, no-data and non-finite cells left
// out; nothing when no cell has a value. Every cell is read when
// `maxSamples` is 0 (one row at a time from a file); else a stride keeps the
// read to about that many, and the range is the sample's - within the
// whole's.
[[nodiscard]] katana::core::Result<std::optional<std::pair<double, double>>>
valueRange(const katana::gis::processing::DatasetValue& dataset, std::size_t maxSamples = 0);

// The raster an analysis verb reads (CONTOUR, RASTER SHADE, RASTER SLOPE),
// read by the one source parser: RASTER <id|name> or FILE <path> as FROM
// binds them, or SURFACE <name> [CELL <m>] - sampled at CELL, or at the cell
// suggested for its extent (the cell chosen now, so the reply can say it).
struct TerrainSource {
    Source source;
    // The dataset GDAL reads, made on the job's thread.
    DeferredDataset dataset;
    // SURFACE: the surface itself, shared and immutable, and whether the
    // line gave its CELL.
    std::shared_ptr<const katana::terrain::TinSurface> surface;
    bool cellGiven = false;
    // input arg=input source=... : what was read.
    std::string record;
};

// Reads the source at `at`, leaving `at` after it. InvalidArgument naming the
// word for a scope of the drawing (`verb` has no drawing source); NotFound for
// a raster or surface of no such id or name; Unsupported for a reference
// raster with no file behind it.
[[nodiscard]] katana::core::Result<TerrainSource>
bindTerrainSource(Context& context, const Tokens& tokens, std::size_t& at, std::string_view verb);

// The closed shapes a scope took, as the areas an analysis is kept inside
// (CONTOUR's boundaries, RASTER SLOPE's clip): closed polylines and circles
// (drawingDataset's polygons, a tagged hole joining its area). Open lines
// and points are no area: counted and said, never an error, as a scope that
// takes nothing is not.
struct ScopeAreas {
    std::vector<katana::interop::geo::ContourBoundary> areas;
    katana::geometry::Box2 box; // of every area, empty for none
    // scope arg=<arg> ..., then areas used=<n> skipped.open=<n> skipped.points=<n>,
    // then a warning for what was left out.
    std::vector<std::string> records;
};

[[nodiscard]] katana::core::Result<ScopeAreas>
bindAreas(Context& context, const katana::cad::ScopeWords& scope, std::string_view arg);

// The areas as one WKT MULTIPOLYGON, for GDAL's --geometry.
[[nodiscard]] std::string areasWkt(const std::vector<katana::interop::geo::ContourBoundary>& areas);

// A number as GDAL's command line takes it, exactly: "0.5", "-12.25".
[[nodiscard]] std::string gdalNumber(double value);

// ---- T0: SURFACE (surface_verbs.cpp) ----
[[nodiscard]] katana::core::Result<Prepared> prepareSurface(Context& context, const Tokens& tokens,
                                                            std::string_view line);
[[nodiscard]] std::string surfaceUsage();

// ---- T1: CONTOUR (contour_verbs.cpp) ----
[[nodiscard]] katana::core::Result<Prepared> prepareContour(Context& context, const Tokens& tokens,
                                                            std::string_view line);
[[nodiscard]] std::string contourUsage();

// ---- T2: RASTER SHADE (shade_verbs.cpp) ----
[[nodiscard]] katana::core::Result<Prepared> prepareShade(Context& context, const Tokens& tokens,
                                                          std::string_view line);
[[nodiscard]] std::string shadeUsage();

} // namespace katana::app::geo
