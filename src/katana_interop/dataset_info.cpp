#include "katana/interop/dataset_info.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <limits>
#include <string>
#include <system_error>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
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

// A dataset's name as GDAL is given it: UTF-8, whatever the code page.
std::string gdalName(const std::filesystem::path& path)
{
    const std::u8string text = path.u8string();
    return std::string(text.begin(), text.end());
}

// GDAL's own `raster info`, `vector info` or `mdim info` of `name`, as its
// JSON text, through the geoprocessing bridge: the bridge opens the dataset
// on this thread, collects GDAL's errors for this run alone, and puts back
// any .aux.xml a --stats would have written beside the file.
Result<std::string> gdalInfo(std::vector<std::string> algorithm, const std::string& name,
                             std::vector<std::string> tokens, const std::string& layer = {})
{
    namespace gp = katana::gis::processing;
    gp::RunRequest request;
    request.path = std::move(algorithm);
    request.tokens = std::move(tokens);
    gp::DatasetPath input{name, {}, layer};
    request.values.emplace_back("input", gp::ArgValue(gp::DatasetValue(std::move(input))));
    auto outputs = gp::run(request);
    if (!outputs) {
        return outputs.error();
    }
    if (!outputs->text) {
        return makeError(ErrorCode::CommandRejected,
                         "GDAL " + gp::pathText(request.path) + " printed nothing", name);
    }
    return *outputs->text;
}

std::optional<double> number(const nlohmann::json& value)
{
    if (value.is_number()) {
        return value.get<double>();
    }
    // GDAL writes a NaN no-data value as the word: a value all the same, and
    // not the absence of one.
    if (value.is_string() && katana::core::lowered(value.get<std::string>()) == "nan") {
        return std::numeric_limits<double>::quiet_NaN();
    }
    return std::nullopt;
}

std::string text(const nlohmann::json& object, const char* key)
{
    const auto found = object.find(key);
    return found != object.end() && found->is_string() ? found->get<std::string>() : std::string();
}

std::string wktOf(const nlohmann::json& object)
{
    const auto crs = object.find("coordinateSystem");
    return crs != object.end() && crs->is_object() ? text(*crs, "wkt") : std::string();
}

// `raster info`'s JSON into the description.
void readRaster(const nlohmann::json& json, SourceDescription& description)
{
    description.driver = text(json, "driverShortName");
    RasterDescription raster;
    if (const auto size = json.find("size"); size != json.end() && size->is_array() &&
                                              size->size() == 2) {
        raster.width = (*size)[0].get<int>();
        raster.height = (*size)[1].get<int>();
    }
    if (const auto transform = json.find("geoTransform");
        transform != json.end() && transform->is_array() && transform->size() == 6) {
        std::array<double, 6> gt{};
        for (std::size_t i = 0; i < gt.size(); ++i) {
            gt[i] = (*transform)[i].get<double>();
        }
        raster.georeferenced = true;
        // The lengths of the column and row vectors, so a rotated grid
        // reports the size of its pixels rather than of their shadow on the
        // x axis.
        raster.pixelWidth = std::hypot(gt[1], gt[4]);
        raster.pixelHeight = std::hypot(gt[2], gt[5]);
        const auto corner = [&gt](double px, double py) {
            return Point2(gt[0] + px * gt[1] + py * gt[2], gt[3] + px * gt[4] + py * gt[5]);
        };
        const double w = raster.width;
        const double h = raster.height;
        for (const Point2& p : {corner(0, 0), corner(w, 0), corner(0, h), corner(w, h)}) {
            raster.bounds.expand(p);
        }
    }
    for (const nlohmann::json& each : json.value("bands", nlohmann::json::array())) {
        BandDescription band;
        band.band = each.value("band", static_cast<int>(raster.bands.size()) + 1);
        band.dataType = text(each, "type");
        band.colour = text(each, "colorInterpretation");
        if (const auto noData = each.find("noDataValue"); noData != each.end()) {
            band.noData = number(*noData);
        }
        // GDAL's JSON prints a band's minimum, maximum, mean and deviation
        // to three decimals; the STATISTICS_ metadata beside them carries 14
        // significant digits (measured: "mean":30.341 against
        // STATISTICS_MEAN 30.341263614231 for the sample terrain). The
        // metadata first, the rounded keys when a driver gives only those.
        const nlohmann::json statistics = [&each] {
            const auto metadata = each.find("metadata");
            if (metadata == each.end() || !metadata->is_object()) {
                return nlohmann::json::object();
            }
            const auto domain = metadata->find("");
            return domain != metadata->end() && domain->is_object() ? *domain
                                                                    : nlohmann::json::object();
        }();
        const auto statistic = [&each, &statistics](const char* key, const char* metadataKey) {
            if (const auto exact = statistics.find(metadataKey);
                exact != statistics.end() && exact->is_string()) {
                if (const auto value = katana::core::parseFiniteDouble(exact->get<std::string>())) {
                    return std::optional<double>(*value);
                }
            }
            const auto found = each.find(key);
            return found != each.end() ? number(*found) : std::nullopt;
        };
        band.min = statistic("minimum", "STATISTICS_MINIMUM");
        band.max = statistic("maximum", "STATISTICS_MAXIMUM");
        band.mean = statistic("mean", "STATISTICS_MEAN");
        band.stdDev = statistic("stdDev", "STATISTICS_STDDEV");
        for (const nlohmann::json& overview : each.value("overviews", nlohmann::json::array())) {
            const auto size = overview.find("size");
            if (size != overview.end() && size->is_array() && size->size() == 2) {
                band.overviews.emplace_back((*size)[0].get<int>(), (*size)[1].get<int>());
            }
        }
        raster.bands.push_back(std::move(band));
    }
    raster.bandCount = static_cast<int>(raster.bands.size());
    raster.noDataValue = raster.bands.empty() ? std::nullopt : raster.bands.front().noData;
    // A container's datasets, SUBDATASET_<n>_NAME and _DESC in the
    // SUBDATASETS metadata domain, in n's order.
    if (const auto metadata = json.find("metadata"); metadata != json.end() && metadata->is_object()) {
        if (const auto domain = metadata->find("SUBDATASETS");
            domain != metadata->end() && domain->is_object()) {
            for (int n = 1;; ++n) {
                const std::string key = "SUBDATASET_" + std::to_string(n) + "_";
                const std::string name = text(*domain, (key + "NAME").c_str());
                if (name.empty()) {
                    break;
                }
                description.subdatasets.push_back({name, text(*domain, (key + "DESC").c_str())});
            }
        }
    }
    description.projectionWkt = wktOf(json);
    description.raster = std::move(raster);
}

// `vector info`'s JSON into the description.
void readVector(const nlohmann::json& json, SourceDescription& description)
{
    if (description.driver.empty()) {
        description.driver = text(json, "driverShortName");
    }
    std::vector<std::string> wkts;
    for (const nlohmann::json& each : json.value("layers", nlohmann::json::array())) {
        VectorLayerDescription layer;
        layer.name = text(each, "name");
        if (const auto count = each.find("featureCount");
            count != each.end() && count->is_number_integer() && count->get<std::int64_t>() >= 0) {
            layer.featureCount = count->get<std::uint64_t>();
        }
        std::string wkt;
        const nlohmann::json geometries = each.value("geometryFields", nlohmann::json::array());
        if (!geometries.empty()) {
            const nlohmann::json& geometry = geometries.front();
            layer.geometryType = text(geometry, "type");
            wkt = wktOf(geometry);
            if (const auto extent = geometry.find("extent");
                extent != geometry.end() && extent->is_array() && extent->size() >= 4) {
                layer.extent.expand(Point2((*extent)[0].get<double>(), (*extent)[1].get<double>()));
                layer.extent.expand(Point2((*extent)[2].get<double>(), (*extent)[3].get<double>()));
            }
        }
        layer.crs = katana::gis::describeCrs(wkt);
        wkts.push_back(std::move(wkt));
        for (const nlohmann::json& field : each.value("fields", nlohmann::json::array())) {
            FieldDescription entry;
            entry.name = text(field, "name");
            entry.type = text(field, "type");
            entry.subtype = text(field, "subType");
            entry.width = field.value("width", 0);
            entry.precision = field.value("precision", 0);
            layer.fields.push_back(std::move(entry));
        }
        description.vectorLayers.push_back(std::move(layer));
    }
    // A vector dataset has no CRS of its own; its layers each have one. When
    // a raster did not already give one, the file's CRS is the one every
    // layer that declares a CRS agrees on. Layers that disagree leave it
    // empty, and formatDescription says they differ rather than that there
    // is none.
    if (description.projectionWkt.empty()) {
        std::string common;
        bool agree = true;
        for (const std::string& wkt : wkts) {
            if (wkt.empty()) {
                continue;
            }
            if (common.empty()) {
                common = wkt;
            } else if (wkt != common) {
                agree = false;
            }
        }
        if (agree) {
            description.projectionWkt = common;
        }
    }
}

// The multidimensional formats GDAL has here (`gdalinfo --formats`, the
// "multidimensional raster" ones that are files): `mdim info` is asked of
// these only, since it fails for every classic raster.
bool multidimensional(const std::string& driver)
{
    for (const char* each : {"netCDF", "HDF4", "HDF5", "GRIB", "BAG", "S102", "S104", "S111",
                             "Zarr", "CPHD"}) {
        if (driver == each) {
            return true;
        }
    }
    return false;
}

// A GDAL dataset described from GDAL's own info JSON: `raster info` and
// `vector info` are both asked, since one file can hold both (a GeoPackage),
// and what neither can read is refused with the reader's error its kind
// suggests.
Result<SourceDescription> describeGdalSource(const std::filesystem::path& path,
                                             SourceDescription description,
                                             const DescribeOptions& options)
{
    const std::string name = gdalName(path);
    std::vector<std::string> rasterTokens{"--format=json"};
    if (options.statistics) {
        rasterTokens.emplace_back("--stats");
    }
    auto raster = gdalInfo({"raster", "info"}, name, std::move(rasterTokens));
    auto vector = gdalInfo({"vector", "info"}, name, {"--format=json"}, options.layer);
    if (!raster && !vector) {
        // A layer asked for that is not there: the vector reader's words.
        const bool vectorFirst = !options.layer.empty() || description.kind == SourceKind::Vector;
        return vectorFirst ? vector.error() : raster.error();
    }
    try {
        if (raster) {
            readRaster(nlohmann::json::parse(*raster), description);
            description.rasterJson = std::move(*raster);
        }
        if (vector) {
            readVector(nlohmann::json::parse(*vector), description);
            description.vectorJson = std::move(*vector);
        }
    } catch (const nlohmann::json::exception& error) {
        return makeError(ErrorCode::CommandRejected,
                         std::string("GDAL's description could not be read: ") + error.what(),
                         name);
    }
    if (options.multidim && multidimensional(description.driver)) {
        // A format that can be multidimensional need not be (a netCDF of
        // one plain grid opens as either): no multidimensional reading is
        // not a failure.
        if (auto multidim = gdalInfo({"mdim", "info"}, name, {})) {
            description.multidimJson = std::move(*multidim);
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

bool isVirtualPath(const std::filesystem::path& path)
{
    const std::string name = gdalName(path);
    return name.starts_with("/vsi") || name.find("://") != std::string::npos;
}

Result<SourceDescription> describeSource(const std::filesystem::path& path,
                                         const DescribeOptions& options)
{
    // A /vsi path or a URL is GDAL's to find: refused here, /vsizip/a.zip/b
    // was "file does not exist" before GDAL was asked.
    std::error_code existsError;
    if (!isVirtualPath(path) && !std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", gdalName(path));
    }

    SourceDescription description;
    description.path = path;
    description.kind = kindForPath(path);
    switch (description.kind) {
    case SourceKind::PointCloud:
        return describePointCloud(path, std::move(description));
    case SourceKind::Archive12d:
        return makeError(ErrorCode::Unsupported,
                         "a 12d archive is described by importing it: its header says nothing "
                         "its contents do not",
                         gdalName(path));
    case SourceKind::Vector:
    case SourceKind::Raster:
        break;
    case SourceKind::Unknown:
        // A /vsi path or a URL is named by what GDAL opens, not by an
        // extension (a URL's may end in a query): GDAL is asked, and so it
        // is when the caller wants whatever GDAL reads. Otherwise a file
        // whose extension no importer claims is refused, as it always was.
        if (!isVirtualPath(path) && !options.anyFormat) {
            return makeError(ErrorCode::Unsupported,
                             "no importer reads files named '" + gdalName(path.extension()) + "'",
                             gdalName(path));
        }
        break;
    }
    return describeGdalSource(path, std::move(description), options);
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
