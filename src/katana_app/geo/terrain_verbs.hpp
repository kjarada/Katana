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
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/core/error.hpp"
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

// ---- T0: SURFACE (surface_verbs.cpp) ----
[[nodiscard]] katana::core::Result<Prepared> prepareSurface(Context& context, const Tokens& tokens,
                                                            std::string_view line);
[[nodiscard]] std::string surfaceUsage();

} // namespace katana::app::geo
