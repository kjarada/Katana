#pragma once

// LAS / LAZ point-cloud access through PDAL, behind a Katana interface (Rule 4).
//
// No PDAL type appears here. PDAL stays inside point_cloud_engine.cpp.
//
// PLAN.MD Phase 17 requires this to scale to datasets that do not fit in memory.
// What is implemented is the decimating, filtering, bounded read that makes a
// large file usable for display and analysis: the caller states a point budget
// and PDAL discards the rest during the pipeline rather than after it. A full
// out-of-core spatial hierarchy with level of detail is NOT implemented, and
// readHeader() exists so a caller can size its budget before committing to a
// read.

#include <cstdint>
#include <filesystem>
#include <limits>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::pointcloud {

struct PointCloudPoint {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
    double intensity = 0.0;
    std::uint8_t classification = 0;
    std::uint8_t red = 0;
    std::uint8_t green = 0;
    std::uint8_t blue = 0;
    bool hasColor = false;
};

struct PointCloudBounds {
    // Inverted infinities, matching geometry::Box2: a default-constructed bounds
    // is EMPTY and can be grown by comparison. Defaulting to zeros would instead
    // describe a degenerate box at the origin, which is not empty by the test
    // below - so an empty cloud would drag Zoom Extents out to 0,0.
    double minX = std::numeric_limits<double>::infinity();
    double minY = std::numeric_limits<double>::infinity();
    double minZ = std::numeric_limits<double>::infinity();
    double maxX = -std::numeric_limits<double>::infinity();
    double maxY = -std::numeric_limits<double>::infinity();
    double maxZ = -std::numeric_limits<double>::infinity();

    [[nodiscard]] bool empty() const { return minX > maxX || minY > maxY || minZ > maxZ; }
};

// What a file declares before any points are read. Cheap: the header only.
struct PointCloudHeader {
    std::uint64_t pointCount = 0;
    PointCloudBounds bounds;
    std::string projectionWkt;
    bool hasColor = false;
};

struct PointCloud {
    std::vector<PointCloudPoint> points;
    PointCloudBounds bounds;
    std::string projectionWkt;
    // Points the file held before decimation and filtering, so a caller can say
    // "showing 2 000 000 of 418 000 000" rather than implying it has them all.
    std::uint64_t sourcePointCount = 0;
};

struct PointCloudReadOptions {
    std::optional<std::uint8_t> classification;
    std::optional<PointCloudBounds> clip;
    // Keep one point in every `decimationStep`. Must be >= 1.
    std::uint32_t decimationStep = 1;
    // Hard ceiling on returned points; 0 means no ceiling. Applied after
    // decimation, as a last resort - prefer a decimation step, which samples
    // the whole extent instead of truncating it to whatever PDAL emits first.
    std::uint64_t maxPoints = 0;
    // COPC only: the coarsest point spacing acceptable, in the cloud's units.
    // A Cloud Optimised Point Cloud is a LAZ whose chunks are already an
    // octree, and readers.copc returns only the levels whose spacing is at or
    // above this, so level of detail is a query parameter rather than a
    // structure Katana owns (PLAN.MD Phase 17). Refused with InvalidArgument
    // on a file that is not COPC, rather than silently returning everything.
    std::optional<double> resolution;
};

class PointCloudEngine {
  public:
    [[nodiscard]] katana::core::Result<PointCloudHeader>
    readHeader(const std::filesystem::path& path) const;

    [[nodiscard]] katana::core::Result<PointCloud>
    read(const std::filesystem::path& path, const PointCloudReadOptions& options = {}) const;

    [[nodiscard]] katana::core::Status write(const std::filesystem::path& path,
                                             const PointCloud& cloud) const;

    // True when PDAL reads `path` as a Cloud Optimised Point Cloud - a
    // .copc.laz whose header carries the COPC record - and so can answer a
    // resolution query. False for any other readable file. NotFound when the
    // file does not exist.
    [[nodiscard]] katana::core::Result<bool> isCopc(const std::filesystem::path& path) const;

    // Writes `source` - any format PDAL reads - as COPC at `destination`,
    // which must end in .copc.laz so that every later read infers
    // readers.copc. Done once, on import, so that every later view of the
    // cloud can ask for just the resolution it needs. Fails with
    // InvalidArgument for the wrong extension, NotFound for a missing
    // source, Unsupported for an unreadable format, and FileExportFailure
    // from PDAL.
    [[nodiscard]] katana::core::Status
    convertToCopc(const std::filesystem::path& source,
                  const std::filesystem::path& destination) const;

    // A decimation step that brings `sourceCount` points under `budget`, never
    // less than 1. Exposed because the GUI needs it to size a read before it
    // knows anything else about the file.
    [[nodiscard]] static std::uint32_t decimationForBudget(std::uint64_t sourceCount,
                                                           std::uint64_t budget);
};

[[nodiscard]] std::string pdalVersion();

} // namespace katana::pointcloud
