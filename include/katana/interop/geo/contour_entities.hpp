#pragma once

// Contours into the drawing (docs/terrain.md, "Contours"): the ONE place the
// two engines of CONTOUR meet. A surface's contours come from the exact TIN
// tracer (terrain::contours); a raster's from GDAL's `raster contour`, whose
// features are turned into the same terrain::Contour here - so which contour
// is major, which layer it goes to and what height it carries are decided
// once, whichever engine traced it.
//
//   contoursFromFeatures  GDAL's contour lines (written with --elevation-name
//                         and --3d) as terrain::Contour, major by the tracer's
//                         own rule: level = base + k * interval, major when k
//                         is a multiple of majorEvery.
//   boundariesOf          the closed shapes a scope took, as the areas a
//                         contour is kept inside.
//   clipContours          the contours cut where they leave those areas.
//   contourCommand        ONE command: polylines on <layer>/major and
//                         <layer>/minor, rings closed, each carrying its level
//                         as its height (entity::setHeights: the `elevation`
//                         property).

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/gis/processing.hpp"
#include "katana/terrain/contours.hpp"

namespace katana::interop::geo {

// The field CONTOUR asks GDAL to write each line's level in: without
// --elevation-name GDAL writes no level at all, only an ID.
inline constexpr std::string_view kContourElevationField = "elevation";

// The contours of `set` - the lines of GDAL's `raster contour` - each at the
// level its `elevationField` holds. A ring GDAL closed by repeating its first
// vertex is a closed polyline. Major when (level - base) / interval is a
// multiple of `majorEvery` (0: none), as terrain::contours flags them. Sorted
// by level, GDAL's order kept within one. InvalidArgument for an interval
// that is not positive and finite, or a table without the field (the contract
// test pins the argument that writes it).
[[nodiscard]] katana::core::Result<std::vector<katana::terrain::Contour>>
contoursFromFeatures(const katana::gis::processing::FeatureSet& set,
                     std::string_view elevationField, double interval, double base,
                     std::size_t majorEvery);

// One area a contour is kept inside: its exterior ring and its holes, each
// closed. A point is in it when it is inside the exterior and in no hole.
struct ContourBoundary {
    std::vector<katana::geometry::Polyline2> rings;
};

// The polygons of `set` (drawingDataset's `polygons` table: closed polylines
// and circles, with the holes tagged to join them) as boundaries. Points and
// open lines are not areas; the caller counts and reports them.
[[nodiscard]] std::vector<ContourBoundary>
boundariesOf(const katana::gis::processing::FeatureSet& set);

// `contours` cut where they cross a boundary, keeping what lies inside any
// of them (on a boundary counts as inside). A ring that never leaves stays a
// ring; one that is cut becomes the open pieces inside, the piece through its
// first vertex joined across it. Each piece keeps its level and whether it is
// major. Deterministic: the order of `contours`, then along each.
[[nodiscard]] std::vector<katana::terrain::Contour>
clipContours(const std::vector<katana::terrain::Contour>& contours,
             const std::vector<ContourBoundary>& boundaries);

struct ContourEntityOptions {
    // Major contours go to <layer>/major, the others to <layer>/minor.
    std::string layer = "terrain/contours";
    // The transaction's name: the verb line, so the history says what ran.
    std::string commandName;
};

struct ContourPlan {
    // nullptr when there are no contours: an empty transaction is refused by
    // the command stack, and nothing to draw changes nothing.
    katana::commands::CommandPtr command;
    std::size_t created = 0, major = 0, minor = 0;
    // Distinct levels with at least one contour.
    std::size_t levels = 0;
    // The layers written to, major first.
    std::vector<std::string> layers;
};

// The one command that draws `contours`: layers made where missing, then the
// polylines, every vertex at the contour's level. It reads the model and
// changes nothing; the caller executes it, which is how the contours become
// one undo step. InvalidArgument for a layer that is not a layer path.
[[nodiscard]] katana::core::Result<ContourPlan>
contourCommand(const katana::entity::Model& model,
               const std::vector<katana::terrain::Contour>& contours,
               const ContourEntityOptions& options);

} // namespace katana::interop::geo
