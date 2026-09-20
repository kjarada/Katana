#pragma once

// Reference data: imported material a drawing is worked ON TOP OF rather than
// material the drawing is made of - georeferenced imagery and point clouds.
//
// Why this is not part of the entity model. An entity is something the user
// draws, selects, snaps to, edits and undoes. A 400-megapixel orthophoto and a
// 2-million-point LiDAR scan are none of those things: they are backdrop. Making
// them entities would put a hundred megabytes of pixels through the command
// stack's before-image undo machinery for a visibility toggle, and would give
// the user a "select all" that returns an orthophoto.
//
// So reference data lives beside the model, is not undoable, and is not saved
// into the project database - the project records the SOURCE PATH and the
// display settings, and the pixels are re-read on open. Rule 3 still holds: the
// renderer paints this, it does not own it. The Document owns it, the CLI can
// manipulate it headless, and the viewport merely draws it.

#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "katana/geometry/primitives2d.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

namespace katana::interop {

using ReferenceId = std::uint64_t;

// ---- raster ---------------------------------------------------------------

struct RasterOverlay {
    ReferenceId id = 0;
    std::string name;
    std::filesystem::path source;

    int width = 0;
    int height = 0;
    std::vector<std::uint8_t> rgba; // width * height * 4, top row first

    // Maps this image's pixel grid to world coordinates (GDAL order).
    std::array<double, 6> geotransform{0.0, 1.0, 0.0, 0.0, 0.0, -1.0};
    bool hasGeotransform = false;
    std::string projectionWkt;

    bool visible = true;
    double opacity = 1.0;

    // World-space corners of the image. Computed through the full affine, so a
    // rotated or north-up-negative geotransform is handled rather than assumed
    // away: all four corners are transformed and the bounding box taken.
    [[nodiscard]] katana::geometry::Box2 worldBounds() const;

    // World position of a pixel corner (px, py), pixel units, origin top-left.
    [[nodiscard]] katana::geometry::Point2 pixelToWorld(double px, double py) const;
};

// ---- point cloud ----------------------------------------------------------

enum class PointColorMode {
    Elevation,      // ramp over the cloud's own z range
    Intensity,      // ramp over the cloud's own intensity range
    Classification, // ASPRS class colours
    SourceColor,    // the file's own RGB, where it has any
    Flat,
};

[[nodiscard]] const char* toString(PointColorMode mode);

struct PointCloudLayer {
    ReferenceId id = 0;
    std::string name;
    std::filesystem::path source;

    std::vector<katana::pointcloud::PointCloudPoint> points;
    katana::pointcloud::PointCloudBounds bounds;
    std::string projectionWkt;
    // Points in the file before decimation, so the UI can be honest about
    // showing a sample rather than the whole cloud.
    std::uint64_t sourcePointCount = 0;
    std::uint32_t decimationStep = 1;

    bool visible = true;
    PointColorMode colorMode = PointColorMode::Elevation;
    double pointSize = 1.0;

    [[nodiscard]] katana::geometry::Box2 worldBounds() const;
    [[nodiscard]] bool isDecimated() const { return decimationStep > 1 || points.size() < sourcePointCount; }
};

// Colour for one point under `mode`, given the ranges to normalise against.
// Returned as 0-255 RGB so no rendering type appears in this layer.
struct Rgb {
    std::uint8_t r = 0;
    std::uint8_t g = 0;
    std::uint8_t b = 0;
};

[[nodiscard]] Rgb colorForPoint(const katana::pointcloud::PointCloudPoint& point,
                                PointColorMode mode, double minValue, double maxValue);

// ASPRS standard classification colours (LAS 1.4 table 17); unknown classes get
// a neutral grey rather than an arbitrary colour.
[[nodiscard]] Rgb classificationColor(std::uint8_t classification);

// ---- the collection -------------------------------------------------------

class ReferenceData {
  public:
    [[nodiscard]] const std::vector<RasterOverlay>& rasters() const { return rasters_; }
    [[nodiscard]] const std::vector<PointCloudLayer>& pointClouds() const { return pointClouds_; }

    // Ids are monotonic and never reused, matching the entity database's rule,
    // so a stale reference can never silently resolve to a different layer.
    ReferenceId add(RasterOverlay raster);
    ReferenceId add(PointCloudLayer cloud);

    [[nodiscard]] RasterOverlay* findRaster(ReferenceId id);
    [[nodiscard]] PointCloudLayer* findPointCloud(ReferenceId id);

    bool remove(ReferenceId id);
    void clear();

    [[nodiscard]] bool empty() const { return rasters_.empty() && pointClouds_.empty(); }
    [[nodiscard]] std::size_t size() const { return rasters_.size() + pointClouds_.size(); }

    // Bounds of every VISIBLE layer, for Zoom Extents. Empty when nothing is
    // visible, which the caller must distinguish from "nothing is loaded".
    [[nodiscard]] katana::geometry::Box2 visibleBounds() const;

  private:
    std::vector<RasterOverlay> rasters_;
    std::vector<PointCloudLayer> pointClouds_;
    ReferenceId nextId_ = 1;
};

} // namespace katana::interop
