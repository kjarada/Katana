#pragma once

// Shared by the katana_interop sources, never installed: a feature table
// moved from one coordinate system to another, for IMPORT's crs=project and
// EXPORT's crs=<code> alike (docs/interop.md, "Import options", "Export
// options"), so the two directions move a table the one way.

#include <string>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/gis/reproject.hpp"

namespace katana::interop::detail {

// Every part of every feature of `table` from `from` to `to`, by the one
// reprojection (gis::reprojectFeatures), heights untouched. An arc's three
// points move with the rest; a conformal projection keeps an arc that small
// round to far below a millimetre. The table's own CRS is left for the
// caller to set: it may want the text it was given or its WKT.
[[nodiscard]] inline katana::core::Status reprojectTable(katana::gis::processing::FeatureTable& table,
                                                         const std::string& from,
                                                         const std::string& to)
{
    std::vector<katana::gis::VectorFeature> geometries;
    for (const katana::gis::processing::Feature& feature : table.features) {
        for (const katana::gis::VectorGeometry& part : feature.parts) {
            geometries.push_back(katana::gis::VectorFeature{part, {}});
        }
    }
    if (geometries.empty()) {
        return {};
    }
    auto moved = katana::gis::reprojectFeatures(std::move(geometries), from, to);
    if (!moved) {
        return moved.error();
    }
    std::size_t next = 0;
    for (katana::gis::processing::Feature& feature : table.features) {
        for (katana::gis::VectorGeometry& part : feature.parts) {
            part = std::move((*moved)[next++].geometry);
        }
    }
    return {};
}

} // namespace katana::interop::detail
