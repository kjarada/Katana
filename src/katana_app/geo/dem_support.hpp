#pragma once

// What the DEM verbs share (docs/terrain.md, "DEMs from GDAL";
// docs/geoprocessing.md, "T6", "T7"): RASTER GRID (grid_verbs.cpp) and RASTER
// MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT and DIFFERENCE (dem_verbs.cpp).
// They are curated verbs over the one executor: each reads its own words with
// these helpers, its sources and targets with the ONE source and target
// parsers (bindings.hpp) and its scope with the ONE scope parser, and builds
// RunRequests; nothing here reads a line another way.

#include <array>
#include <cstddef>
#include <filesystem>
#include <map>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/gis/processing.hpp"

namespace katana::app::geo {

// A curated verb's words, parted: its own key=value options and the rest.
struct VerbWords {
    // By the key as the verb names it (lower case), the value as typed.
    std::map<std::string, std::string> options;
    // Everything else, in order and with its quoting: the scope, the
    // sources, NAME, TO, the flags.
    Tokens rest;
};

// Takes the words from `from` on that are `key=value` with a key of `keys`
// (any case) as options, wherever they stand, and leaves the rest in order.
// The keys a verb takes are never a WHERE key (TYPE, LAYER, STYLE, COLOUR,
// PROP, TEXT), so an option after a WHERE filter is the verb's and not a
// condition the scope parser would refuse; the scope parser still reads every
// scope word. InvalidArgument naming it for an option given twice or with no
// value. A quoted word is never an option.
[[nodiscard]] katana::core::Result<VerbWords> splitOptions(const Tokens& tokens, std::size_t from,
                                                           std::span<const std::string_view> keys,
                                                           std::string_view verb);

// A value of an option that must be a positive, finite number:
// InvalidArgument naming the option otherwise.
[[nodiscard]] katana::core::Result<double> positiveOption(std::string_view key,
                                                          const std::string& text);
// x0,y0,x1,y1 with x0 < x1 and y0 < y1, min before max whichever corners came
// first: InvalidArgument naming the option otherwise.
[[nodiscard]] katana::core::Result<std::array<double, 4>> boxOption(std::string_view key,
                                                                    const std::string& text);

// The records of a reply, one a line.
[[nodiscard]] std::string joinRecords(const std::vector<std::string>& records);

// Band 1 of a raster at full precision, read whole on the thread that asks:
// a grid handed over in memory as it is, a file opened for this read alone
// (never a dataset another run holds, plan.risks). Its no-data value when it
// declares one: absent is not a value.
struct BandValues {
    int width = 0;
    int height = 0;
    std::array<double, 6> geotransform{0.0, 1.0, 0.0, 0.0, 0.0, -1.0};
    std::optional<double> noData;
    std::vector<double> values; // width * height, row major, top row first
};
[[nodiscard]] katana::core::Result<BandValues>
readBandOne(const katana::gis::processing::DatasetValue& dataset);
[[nodiscard]] katana::core::Result<BandValues> readBandOne(const std::filesystem::path& file);

// Whether a cell of `band` holds a value: finite and not the no-data value.
[[nodiscard]] bool holdsValue(const BandValues& band, double cell);

// The area of one cell of a north-up or rotated grid: |gt1 gt5 - gt2 gt4|.
[[nodiscard]] double cellArea(const std::array<double, 6>& geotransform);

// A raster file kept as a reference raster read from where it is, under
// `name` or the first of name-2, name-3 ... no raster has: one a line wrote
// where it said (save=<file>), which is the person's and so neither moved nor
// copied, or one already in the derived folder (a mosaic's VRT). The record
// says so as a derived raster's does, persisted as the caller says:
//   output arg=output kind=raster target=reference id=2 name=dem raster=40x30
//     file="C:/work/dem.tif" persisted=yes
[[nodiscard]] katana::core::Result<std::string>
keepInPlace(Context& context, const std::filesystem::path& file, const std::string& name,
            const std::string& derivation, bool persisted, std::string_view arg = "output");

// rasterInfoOf, the facts of a raster dataset, is terrain_verbs.hpp's.

// `from` renamed to `to`, or copied and removed when they are on different
// volumes; the folder made. FileExportFailure naming `to`.
[[nodiscard]] katana::core::Status moveFile(const std::filesystem::path& from,
                                            const std::filesystem::path& to);

// A UTF-8 path as the file system's, and back.
[[nodiscard]] std::filesystem::path pathOfUtf8(const std::string& utf8);
[[nodiscard]] std::string utf8OfPath(const std::filesystem::path& path);

} // namespace katana::app::geo
