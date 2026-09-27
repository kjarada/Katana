#pragma once

// Shared between the katana_io sources, never installed: the conversions
// between Katana's plain geometry and OGR's, and the one way a coordinate
// system is read, for the adapter (gdal_adapter.cpp) and the algorithm bridge
// (geo/processing.cpp). Defined once, in gdal_adapter.cpp, so a file read by
// IMPORT and a dataset handed back by an algorithm become the same geometry by
// the same code.

#include <string>
#include <vector>

#include <ogr_geometry.h>
#include <ogr_spatialref.h>

#include "katana/gis/gdal_adapter.hpp"

namespace katana::gis::detail {

// A new OGR geometry the caller owns, in the geometry's own dimension (2D
// stays 2D); nullptr for a geometry with too few points for its kind.
[[nodiscard]] OGRGeometry* makeOgrGeometry(const VectorGeometry& geometry);

// Appends one VectorGeometry per simple member of `geometry`: a
// multi-geometry or collection is recursed into. What cannot be represented
// is skipped and said in `warnings`.
void flattenOgrGeometry(const OGRGeometry* geometry, std::vector<VectorGeometry>& out,
                        std::vector<std::string>& warnings);

// WKT1, WKT2 or PROJJSON, as GDAL's SetProjection reads them, with the
// traditional GIS axis order and nothing fetched from a file or a URL.
[[nodiscard]] bool parseCrs(const std::string& text, OGRSpatialReference& reference);

} // namespace katana::gis::detail
