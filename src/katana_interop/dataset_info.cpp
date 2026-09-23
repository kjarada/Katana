#include "katana/interop/dataset_info.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <system_error>

#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

// The PDAL reader for a point-cloud extension, named as PDAL names it. Written
// out here because interop may not include PDAL (Rule 4) and the engine does
// not publish the reader it infers; the extensions are exactly
// pointCloudExtensions(), so a new one there without a line here says
// "unknown reader" rather than something untrue.
std::string pdalReaderFor(const std::filesystem::path& path)
{
    std::string extension = katana::core::lowered(path.extension().string());
    if (!extension.empty() && extension.front() == '.') {
        extension.erase(extension.begin());
    }
    struct ReaderForExtension {
        const char* extension;
        const char* reader;
    };
    constexpr std::array<ReaderForExtension, 7> kReaders{{
        {"las", "readers.las"},
        {"laz", "readers.las"},
        {"copc", "readers.copc"},
        {"bpf", "readers.bpf"},
        {"ply", "readers.ply"},
        {"pcd", "readers.pcd"},
        {"e57", "readers.e57"},
    }};
    for (const ReaderForExtension& entry : kReaders) {
        if (extension == entry.extension) {
            return entry.reader;
        }
    }
    return "unknown reader";
}

Result<SourceDescription> describeGdalSource(const std::filesystem::path& path,
                                             SourceDescription description)
{
    auto opened = katana::gis::GdalDataset::open(path);
    if (!opened.ok()) {
        return opened.error();
    }
    const katana::gis::GdalDataset& dataset = **opened;
    description.driver = dataset.driverName();

    if (dataset.hasRaster()) {
        auto info = dataset.rasterInfo();
        if (!info.ok()) {
            return info.error();
        }
        RasterDescription raster;
        raster.width = info->width;
        raster.height = info->height;
        raster.bandCount = info->bandCount;
        raster.georeferenced = info->hasGeotransform;
        raster.noDataValue = info->noDataValue;
        if (raster.georeferenced) {
            const std::array<double, 6>& gt = info->geotransform;
            // The lengths of the column and row vectors, so a rotated grid
            // reports the size of its pixels rather than of their shadow on
            // the x axis.
            raster.pixelWidth = std::hypot(gt[1], gt[4]);
            raster.pixelHeight = std::hypot(gt[2], gt[5]);
            const auto corner = [&gt](double px, double py) {
                return Point2(gt[0] + px * gt[1] + py * gt[2], gt[3] + px * gt[4] + py * gt[5]);
            };
            const double w = info->width;
            const double h = info->height;
            for (const Point2& p : {corner(0, 0), corner(w, 0), corner(0, h), corner(w, h)}) {
                raster.bounds.expand(p);
            }
        }
        description.raster = raster;
        description.projectionWkt = info->projectionWkt;
    }

    if (dataset.hasVector()) {
        auto layers = dataset.vectorLayers();
        if (!layers.ok()) {
            return layers.error();
        }
        for (const katana::gis::VectorLayerInfo& layer : *layers) {
            VectorLayerDescription entry;
            entry.name = layer.name;
            entry.featureCount = layer.featureCount;
            entry.geometryType = layer.geometryType;
            entry.crs = katana::gis::describeCrs(layer.projectionWkt);
            description.vectorLayers.push_back(std::move(entry));
        }
        // A vector dataset has no CRS of its own; its layers each have one.
        // When a raster did not already give one, the file's CRS is the one
        // every layer that declares a CRS agrees on. Layers that disagree
        // leave it empty, and formatDescription says they differ rather than
        // that there is none.
        if (description.projectionWkt.empty()) {
            std::string common;
            bool agree = true;
            for (const katana::gis::VectorLayerInfo& layer : *layers) {
                if (layer.projectionWkt.empty()) {
                    continue;
                }
                if (common.empty()) {
                    common = layer.projectionWkt;
                } else if (layer.projectionWkt != common) {
                    agree = false;
                }
            }
            if (agree) {
                description.projectionWkt = common;
            }
        }
    }

    description.crs = katana::gis::describeCrs(description.projectionWkt);
    return description;
}

Result<SourceDescription> describePointCloud(const std::filesystem::path& path,
                                             SourceDescription description)
{
    const katana::pointcloud::PointCloudEngine engine;
    auto header = engine.readHeader(path);
    if (!header.ok()) {
        return header.error();
    }
    auto copc = engine.isCopc(path);
    if (!copc.ok()) {
        return copc.error();
    }
    PointCloudDescription cloud;
    cloud.pointCount = header->pointCount;
    cloud.bounds = header->bounds;
    cloud.hasColor = header->hasColor;
    cloud.copc = *copc;
    description.pointCloud = cloud;
    description.driver = *copc ? std::string("readers.copc") : pdalReaderFor(path);
    description.projectionWkt = header->projectionWkt;
    description.crs = katana::gis::describeCrs(header->projectionWkt);
    return description;
}

// ---- text ---------------------------------------------------------------------

// 1234567 -> "1,234,567". By hand rather than through a locale: the grouping
// character must not depend on where the program happens to run, any more
// than the decimal point may (core/text.hpp).
std::string grouped(std::uint64_t value)
{
    const std::string digits = std::to_string(value);
    std::string out;
    out.reserve(digits.size() + digits.size() / 3);
    for (std::size_t i = 0; i < digits.size(); ++i) {
        if (i > 0 && (digits.size() - i) % 3 == 0) {
            out += ',';
        }
        out += digits[i];
    }
    return out;
}

// Three decimals, a millimetre in metres. std::to_chars is locale-independent
// by specification, which an ostream is not.
std::string millimetres(double value)
{
    std::array<char, 64> buffer{};
    const auto [end, error] =
        std::to_chars(buffer.data(), buffer.data() + buffer.size(), value,
                      std::chars_format::fixed, 3);
    if (error != std::errc{}) {
        // Only a magnitude beyond 10^60 fails to fit, which no coordinate is.
        return katana::core::formatExactReal(value);
    }
    return std::string(buffer.data(), end);
}

std::string coordinatePair(double x, double y)
{
    return "(" + millimetres(x) + ", " + millimetres(y) + ")";
}

std::string plural(std::uint64_t count, const char* one, const char* many)
{
    return grouped(count) + " " + (count == 1 ? one : many);
}

} // namespace

Result<SourceDescription> describeSource(const std::filesystem::path& path)
{
    std::error_code existsError;
    if (!std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", path.string());
    }

    SourceDescription description;
    description.path = path;
    description.kind = kindForPath(path);
    switch (description.kind) {
    case SourceKind::Vector:
    case SourceKind::Raster:
        return describeGdalSource(path, std::move(description));
    case SourceKind::PointCloud:
        return describePointCloud(path, std::move(description));
    case SourceKind::Archive12d:
        return makeError(ErrorCode::Unsupported,
                         "a 12d archive is described by importing it: its header says nothing "
                         "its contents do not",
                         path.string());
    case SourceKind::Unknown:
        break;
    }
    return makeError(ErrorCode::Unsupported,
                     "no importer reads files named '" + path.extension().string() + "'",
                     path.string());
}

std::string formatDescription(const SourceDescription& description)
{
    std::string text;
    const auto line = [&text](const std::string& content) {
        text += content;
        text += '\n';
    };

    line(description.path.filename().string());
    line(std::string("Kind: ") + toString(description.kind) +
         (description.driver.empty() ? std::string() : " (" + description.driver + ")"));

    if (!description.crs.empty()) {
        line("Coordinate system: " + description.crs);
    } else if (std::any_of(description.vectorLayers.begin(), description.vectorLayers.end(),
                           [](const VectorLayerDescription& layer) {
                               return !layer.crs.empty();
                           })) {
        line("Coordinate system: differs between layers (see each layer)");
    } else {
        line("Coordinate system: no coordinate system declared");
    }

    if (description.raster.has_value()) {
        const RasterDescription& raster = *description.raster;
        line("Raster: " + grouped(static_cast<std::uint64_t>(std::max(0, raster.width))) + " x " +
             grouped(static_cast<std::uint64_t>(std::max(0, raster.height))) + " pixels, " +
             plural(static_cast<std::uint64_t>(std::max(0, raster.bandCount)), "band", "bands"));
        if (raster.georeferenced) {
            // Exact rather than to the millimetre: a geographic raster's
            // pixel is a few ten-thousandths of a degree, which three
            // decimals would print as nothing.
            line("Pixel size: " + katana::core::formatExactReal(raster.pixelWidth) + " x " +
                 katana::core::formatExactReal(raster.pixelHeight));
            if (!raster.bounds.empty()) {
                line("Bounds: " + coordinatePair(raster.bounds.min.x, raster.bounds.min.y) +
                     " to " + coordinatePair(raster.bounds.max.x, raster.bounds.max.y));
            }
        } else {
            line("Georeferencing: not georeferenced - its pixels have no ground position");
        }
        // Exact: a no-data value is a sentinel compared for equality, so a
        // rounded one would be a different value.
        line("No-data value: " + (raster.noDataValue.has_value()
                                      ? katana::core::formatExactReal(*raster.noDataValue)
                                      : std::string("none declared")));
    }

    if (!description.vectorLayers.empty()) {
        line("Vector layers: " + grouped(description.vectorLayers.size()));
        for (const VectorLayerDescription& layer : description.vectorLayers) {
            line("  " + layer.name + ": " + plural(layer.featureCount, "feature", "features") +
                 ", " + (layer.geometryType.empty() ? std::string("unknown geometry")
                                                    : layer.geometryType) +
                 ", " + (layer.crs.empty() ? std::string("no coordinate system declared")
                                           : layer.crs));
        }
    }

    if (description.pointCloud.has_value()) {
        const PointCloudDescription& cloud = *description.pointCloud;
        line("Point cloud: " + plural(cloud.pointCount, "point", "points"));
        if (!cloud.bounds.empty()) {
            line("Bounds: x " + millimetres(cloud.bounds.minX) + " to " +
                 millimetres(cloud.bounds.maxX) + ", y " + millimetres(cloud.bounds.minY) +
                 " to " + millimetres(cloud.bounds.maxY) + ", z " +
                 millimetres(cloud.bounds.minZ) + " to " + millimetres(cloud.bounds.maxZ));
        } else {
            line("Bounds: none declared");
        }
        line(std::string("Colour: ") + (cloud.hasColor ? "yes" : "no"));
        line(std::string("COPC: ") +
             (cloud.copc ? "yes - can be read by resolution" : "no - read by decimation"));
    }
    return text;
}

} // namespace katana::interop
