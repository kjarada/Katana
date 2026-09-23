#pragma once

// What a file holds, read WITHOUT importing it (PLAN.MD Phase 20) - the
// gdalinfo / pdal info a person needs before choosing what to import and how:
// which layer of a GeoPackage, whether a DEM is georeferenced and what its
// no-data value is, how many points a LAS holds and whether it is COPC.
//
// One description and one wording, used by the desktop application's
// GIS > Dataset Information and by the CLI's INFO verb, so the two cannot
// disagree about what a file is.

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/import.hpp"

namespace katana::interop {

struct RasterDescription {
    int width = 0;
    int height = 0;
    int bandCount = 0;
    bool georeferenced = false;
    // World units per pixel along the image's own columns and rows: the
    // lengths of the geotransform's column and row vectors, so a rotated
    // image reports its true pixel size. Meaningful only when georeferenced.
    double pixelWidth = 0.0;
    double pixelHeight = 0.0;
    // All four corners through the full affine. Empty when not georeferenced:
    // GDAL's default transform would put it at the origin, which is a guess.
    katana::geometry::Box2 bounds;
    std::optional<double> noDataValue; // band 1's
};

struct VectorLayerDescription {
    std::string name;
    std::uint64_t featureCount = 0;
    std::string geometryType; // GDAL's spelling, e.g. "Polygon"
    std::string crs;          // gis::describeCrs of the layer's own CRS; empty when none
};

struct PointCloudDescription {
    std::uint64_t pointCount = 0;
    katana::pointcloud::PointCloudBounds bounds;
    bool hasColor = false;
    // A Cloud Optimised Point Cloud, which can answer a resolution query
    // (PointCloudImportOptions::resolution) instead of being decimated.
    bool copc = false;
};

struct SourceDescription {
    std::filesystem::path path;
    SourceKind kind = SourceKind::Unknown;
    // GDAL's driver short name ("GTiff", "GPKG"), or the PDAL reader
    // ("readers.las", "readers.copc") for a point cloud.
    std::string driver;
    // gis::describeCrs of the dataset's CRS; empty when the file declares none.
    std::string crs;
    std::string projectionWkt;
    // A GeoPackage can hold rasters and vector layers at once, so these are
    // filled from what the dataset actually contains, not from its kind.
    std::optional<RasterDescription> raster;
    std::vector<VectorLayerDescription> vectorLayers;
    std::optional<PointCloudDescription> pointCloud;
};

// NotFound for a missing file; Unsupported for an extension no importer
// claims, and for a 12d archive, which is described by importing it (its
// header says nothing its contents do not); FileImportFailure when GDAL or
// PDAL cannot read it.
[[nodiscard]] katana::core::Result<SourceDescription>
describeSource(const std::filesystem::path& path);

// A multi-line summary for a person, in a fixed order: kind and driver, the
// CRS (or that there is none), then the raster, the layers or the cloud.
// Coordinates are printed to 3 decimals - the millimetre, the resolution
// survey coordinates are quoted to - and counts with thousands separators.
[[nodiscard]] std::string formatDescription(const SourceDescription& description);

} // namespace katana::interop
