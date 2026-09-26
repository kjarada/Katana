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
// So reference data lives beside the model, is not undoable, and its pixels
// and points are not saved into the project database. The project records
// each layer's SOURCE and its display settings (ReferenceSource, below, kept
// in storage::ProjectMetadata::referenceLayers) and the front ends read them
// again when it opens (REFS RESTORE, docs/interop.md "Reference layers").
// Rule 3 still holds: the renderer paints this, it does not own it. A front
// end owns it, the CLI manipulates it headless, and the viewport merely draws
// it.

#include <array>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

namespace katana::interop {

using ReferenceId = std::uint64_t;

// ---- raster ---------------------------------------------------------------

// What a raster is to the drawing: a picture to work over, heights, or a
// product made from other data by a geoprocessing run (docs/geoprocessing.md).
// Derived rasters are kept in the project's cache and can be made again from
// their `derivation`.
enum class RasterRole { Imagery, Elevation, Derived };

// How a raster's values are drawn: as they are, or shaded from its heights.
// Only Plain is drawn so far; the shaded styles are the terrain shading's.
enum class RasterDisplayStyle { Plain, Hillshade, Relief, ReliefHillshade, Slope };

// The facts of band 1 a reply or a legend needs, read at full precision -
// never from the RGBA display copy. Absent is not zero: a band without a
// no-data value, or whose range was never read, says so.
struct RasterBandFacts {
    std::string dataType;
    std::optional<double> noData;
    std::optional<double> min, max;
    std::string unit;
    std::string verticalCrs;
};

[[nodiscard]] const char* toString(RasterRole role);

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

    // Where a raster fetched from a web service came from, and the terms it
    // came under (docs/gis_online.md, "Licences and attribution"): the
    // service's address with any key removed, its licence, and the
    // attribution the publisher asks for. Empty for a file.
    std::string sourceUrl;
    std::string licence;
    std::string attribution;

    RasterRole role = RasterRole::Imagery;
    RasterBandFacts facts;
    // The line that made a Derived raster ("GDAL raster hillshade ... FROM
    // SURFACE ground CELL 1"); empty for any other.
    std::string derivation;
    RasterDisplayStyle displayStyle = RasterDisplayStyle::Plain;

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
    // Every layer, and every record kept as missing: a drawing closed.
    void clear();

    // The records (toRecord) of layers a project names whose source could
    // not be read when it opened: kept, so that saving does not drop a layer
    // because its drive was not connected that day, and listed as missing.
    void keepMissing(std::string record) { missing_.push_back(std::move(record)); }
    [[nodiscard]] const std::vector<std::string>& missing() const { return missing_; }
    // Drops the missing record of that layer name, case-insensitively; false
    // when none has it.
    bool forgetMissing(std::string_view name);

    [[nodiscard]] bool empty() const { return rasters_.empty() && pointClouds_.empty(); }
    [[nodiscard]] std::size_t size() const { return rasters_.size() + pointClouds_.size(); }

    // Bounds of every VISIBLE layer, for Zoom Extents. Empty when nothing is
    // visible, which the caller must distinguish from "nothing is loaded".
    [[nodiscard]] katana::geometry::Box2 visibleBounds() const;

  private:
    std::vector<RasterOverlay> rasters_;
    std::vector<PointCloudLayer> pointClouds_;
    std::vector<std::string> missing_;
    ReferenceId nextId_ = 1;
};

// ---- the words a line and a record use ------------------------------------

// elevation, intensity, classification, rgb, flat: one word each, unlike the
// names the window's menus show ("Source colour").
[[nodiscard]] const char* toWord(PointColorMode mode);
[[nodiscard]] std::optional<PointColorMode> pointColorModeFromWord(std::string_view word);
// plain, hillshade, relief, relief+hillshade, slope.
[[nodiscard]] const char* toWord(RasterDisplayStyle style);
[[nodiscard]] std::optional<RasterDisplayStyle> displayStyleFromWord(std::string_view word);
// imagery, elevation, derived: toString's words, read back.
[[nodiscard]] std::optional<RasterRole> rasterRoleFromWord(std::string_view word);

// ---- what a project keeps -------------------------------------------------

// A reference layer as the project records it: where it came from and how it
// is shown, never its pixels or points. One line of versioned JSON a layer
// (toRecord): a field a newer build adds is skipped by an older one, and a
// record of a newer version is refused by name rather than half read.
struct ReferenceSource {
    enum class Kind { Raster, PointCloud };
    Kind kind = Kind::Raster;
    std::string name;
    // The file read again: a raster, a point cloud, or the 12d archive a
    // cloud came in with (the cloud of this name is taken from it).
    std::filesystem::path source;
    bool visible = true;
    // A raster's.
    double opacity = 1.0;
    RasterRole role = RasterRole::Imagery;
    RasterDisplayStyle displayStyle = RasterDisplayStyle::Plain;
    std::string derivation;
    std::string sourceUrl, licence, attribution;
    // The display copy's larger side, so it is read again at the size it had.
    int maxPixels = 4096;
    // A point cloud's.
    PointColorMode colorMode = PointColorMode::Elevation;
    double pointSize = 1.0;
    // The points it held, as the import's budget, so it is read again at the
    // density it had.
    std::uint64_t budget = 2'000'000;
};

[[nodiscard]] ReferenceSource sourceOf(const RasterOverlay& raster);
[[nodiscard]] ReferenceSource sourceOf(const PointCloudLayer& cloud);

// One line of JSON: {"version":1,"kind":"raster","name":...}.
[[nodiscard]] std::string toRecord(const ReferenceSource& source);
// InvalidArgument for a record that is not one, and for one of a version
// newer than this build reads, naming it.
[[nodiscard]] katana::core::Result<ReferenceSource> parseReferenceRecord(std::string_view record);

// Every layer's record, the rasters then the clouds, each in the order added,
// then the records kept as missing: what a save puts in
// storage::ProjectMetadata::referenceLayers.
[[nodiscard]] std::vector<std::string> referenceRecords(const ReferenceData& reference);

// A layer read again from its source, with its recorded name and display
// settings: what a project's opening restores. NotFound when the source is
// gone - which a caller reports and carries on from - and the reader's own
// failure otherwise. Pure: reads the file, touches no ReferenceData.
using ReferenceLayer = std::variant<RasterOverlay, PointCloudLayer>;
[[nodiscard]] katana::core::Result<ReferenceLayer> readReference(const ReferenceSource& source);

} // namespace katana::interop
