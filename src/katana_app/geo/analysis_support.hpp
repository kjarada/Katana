#pragma once

// What the terrain analysis verbs of T4 and T5 share (docs/terrain.md,
// "Sampling and drape", "Viewshed and line of sight"): RASTER SAMPLE, DRAPE,
// RASTER VIEWSHED and LOS read heights from the ground, and all of them
// write points as x,y and take keyword-and-value words (AT x,y, OBSERVER
// x,y, NAME <n>) out of a line before the rest goes to the ONE scope parser
// through vector::readVerbWords.

#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/cad/geo/drape.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/gis/raster_sampling.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::app::geo::analysis {

// The ground a verb reads heights from: a surface on its own triangles -
// exactly, never through a grid of it - or a raster's file, sampled by GDAL
// at full precision. Copied at prepare; opened on the worker.
struct Ground {
    Source source;
    std::shared_ptr<const katana::terrain::TinSurface> surface; // SURFACE
    std::string path;                                           // RASTER, FILE: the file
    // input arg=input source=surface name=ground read=triangles, or the
    // shared input record of a raster or file.
    std::string record;
};

// SURFACE <name> | RASTER <id|name> | FILE <path> at `at`, leaving `at` after
// it. InvalidArgument naming the word for anything else (a scope of the
// drawing comes after the ground) and for CELL, which only a grid has;
// NotFound for a surface or raster of no such name; Unsupported for a
// reference raster with no file behind it.
[[nodiscard]] katana::core::Result<Ground> bindGround(Context& context, const Tokens& tokens,
                                                      std::size_t& at, std::string_view verb);

// The ground opened on the worker: the height at a plan point, and a
// natural spacing to walk it at - half a raster cell, or none for a
// surface, whose caller decides.
struct OpenGround {
    katana::cad::geo::HeightAt at;
    std::optional<double> spacing;
};

// A surface read on its triangles (TinSurface::elevationAt); a raster
// through gis::RasterSampler with `method`. What RasterSampler::open
// refuses.
[[nodiscard]] katana::core::Result<OpenGround> openGround(const Ground& ground,
                                                          katana::gis::Resampling method);

// "x,y" as a plan point: two finite numbers, locale-independent; none
// otherwise.
[[nodiscard]] std::optional<katana::geometry::Point2> pointOf(std::string_view text);
// A point as a line and a record write it: "12.5,-3".
[[nodiscard]] std::string pointText(const katana::geometry::Point2& point);

// A line's words from `begin`, with every `<keyword> <value>` of `keywords`
// (upper case) taken out wherever it stands, in order - an unquoted keyword
// in any case; a quoted "AT" is a value. What is left, for
// vector::readVerbWords. InvalidArgument naming the keyword when no value
// follows it.
struct KeywordValues {
    Tokens rest;
    std::vector<std::pair<std::string, std::string>> taken; // keyword upper case, value
};
[[nodiscard]] katana::core::Result<KeywordValues>
takeKeywordValues(const Tokens& tokens, std::size_t begin, const std::vector<std::string>& keywords);

} // namespace katana::app::geo::analysis
