#include "katana/interop/online_requests.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <numbers>
#include <set>

#include <nlohmann/json.hpp>

#include "katana/core/text.hpp"
#include "katana/gis/web_access.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::gis::percentEncode;
using Json = nlohmann::json;

// WMTS 1.0 (6.1): the "standardized rendering pixel" is 0.28 mm.
constexpr double kRenderingPixelMetres = 0.28e-3;
// Web Mercator's scale denominator at level 0 for 256-pixel tiles: the
// equator's length (2 pi 6378137 m) over 256 pixels of 0.28 mm (WMTS 1.0,
// annex E.4, GoogleMapsCompatible).
constexpr double kWebMercatorScale0 = 559082264.0287178;
constexpr double kWebMercatorHalf = 20037508.342789244; // pi * 6378137
constexpr double kEarthRadiusKm = 6371.0;               // mean radius, IUGG

std::string box4(const CrsBox& box, bool swap)
{
    return swap ? formatNumber(box.minY) + "," + formatNumber(box.minX) + "," +
                      formatNumber(box.maxY) + "," + formatNumber(box.maxX)
                : formatNumber(box.minX) + "," + formatNumber(box.minY) + "," +
                      formatNumber(box.maxX) + "," + formatNumber(box.maxY);
}

std::string trimSlash(std::string url)
{
    while (!url.empty() && url.back() == '/') {
        url.pop_back();
    }
    return url;
}

std::string replaceAll(std::string text, std::string_view from, std::string_view to)
{
    for (std::size_t at = text.find(from); at != std::string::npos;
         at = text.find(from, at + to.size())) {
        text.replace(at, from.size(), to);
    }
    return text;
}

// A tag key or value an Overpass filter may hold: letters, digits and the
// punctuation OpenStreetMap keys use (addr:street, name:en, building:levels,
// a value like "1-2"). No quote, backslash, bracket or brace can appear, so a
// term can only ever be quoted text inside the one statement.
bool plainTagText(std::string_view text)
{
    return !text.empty() && std::all_of(text.begin(), text.end(), [](char c) {
        const auto byte = static_cast<unsigned char>(c);
        return (byte >= 'a' && byte <= 'z') || (byte >= 'A' && byte <= 'Z') ||
               (byte >= '0' && byte <= '9') || byte == '_' || byte == ':' || byte == '-' ||
               byte == '.' || byte == ' ' || byte >= 0x80;
    });
}

std::optional<CrsBox> bboxOf(const Json& value)
{
    if (!value.is_array() || value.size() < 4) {
        return std::nullopt;
    }
    // 2D [w, s, e, n] or 3D [w, s, zmin, e, n, zmax] (GeoJSON 5).
    const bool threeD = value.size() >= 6;
    CrsBox box;
    try {
        box.minX = value[0].get<double>();
        box.minY = value[1].get<double>();
        box.maxX = value[threeD ? 3 : 2].get<double>();
        box.maxY = value[threeD ? 4 : 3].get<double>();
    } catch (const Json::exception&) {
        return std::nullopt;
    }
    return box;
}

bool intersects(const CrsBox& a, const CrsBox& b)
{
    return a.minX <= b.maxX && b.minX <= a.maxX && a.minY <= b.maxY && b.minY <= a.maxY;
}

} // namespace

std::string formatNumber(double value)
{
    char buffer[40];
    std::snprintf(buffer, sizeof buffer, "%.17g", value);
    // Shorter when shorter reads back the same: 151.2 rather than
    // 151.19999999999999, which is the same double and an easier URL to read.
    // Never in exponent form, which not every service parses: 16830000, not
    // 1.683e+07.
    for (int precision = 1; precision <= 17; ++precision) {
        char shorter[40];
        std::snprintf(shorter, sizeof shorter, "%.*g", precision, value);
        if (std::strchr(shorter, 'e') == nullptr && std::strtod(shorter, nullptr) == value) {
            return shorter;
        }
    }
    // A value %g will only write with an exponent: fixed notation, trimmed.
    std::snprintf(buffer, sizeof buffer, "%.10f", value);
    std::string fixed = buffer;
    if (std::strtod(fixed.c_str(), nullptr) == value) {
        while (!fixed.empty() && fixed.back() == '0') {
            fixed.pop_back();
        }
        if (!fixed.empty() && fixed.back() == '.') {
            fixed.pop_back();
        }
        return fixed;
    }
    std::snprintf(buffer, sizeof buffer, "%.17g", value);
    return buffer;
}

std::string withQuery(const std::string& base, const std::string& query)
{
    if (base.find('?') == std::string::npos) {
        return base + "?" + query;
    }
    return base + (base.back() == '?' || base.back() == '&' ? "" : "&") + query;
}

// ---- sizing ---------------------------------------------------------------------

Result<std::vector<ImageRequest>> planImageRequests(const CrsBox& box, double unitsPerPixel,
                                                    int maxPixels, int maxRequests)
{
    if (!box.valid()) {
        return makeError(ErrorCode::InvalidArgument, "the area is empty or inverted");
    }
    if (!(unitsPerPixel > 0.0) || !std::isfinite(unitsPerPixel)) {
        return makeError(ErrorCode::InvalidArgument, "the resolution must be positive");
    }
    if (maxPixels < 1) {
        return makeError(ErrorCode::InvalidArgument, "the request size limit must be positive");
    }
    const double totalColumns = std::ceil(box.width() / unitsPerPixel - 1e-9);
    const double totalRows = std::ceil(box.height() / unitsPerPixel - 1e-9);
    const double across = std::ceil(totalColumns / maxPixels);
    const double down = std::ceil(totalRows / maxPixels);
    if (across * down > maxRequests) {
        const double fits = unitsPerPixel * std::sqrt(across * down / maxRequests);
        return makeError(ErrorCode::InvalidArgument,
                         "the area needs " + formatNumber(across * down) +
                             " image requests at this resolution, more than the " +
                             std::to_string(maxRequests) +
                             " allowed; use a resolution of " + formatNumber(std::ceil(fits * 100.0) / 100.0) +
                             " or coarser, or a smaller area");
    }
    std::vector<ImageRequest> requests;
    const auto columns = static_cast<long long>(totalColumns);
    const auto rows = static_cast<long long>(totalRows);
    for (long long rowStart = 0; rowStart < rows; rowStart += maxPixels) {
        const long long rowCount = std::min<long long>(maxPixels, rows - rowStart);
        for (long long columnStart = 0; columnStart < columns; columnStart += maxPixels) {
            const long long columnCount = std::min<long long>(maxPixels, columns - columnStart);
            ImageRequest request;
            request.width = static_cast<int>(columnCount);
            request.height = static_cast<int>(rowCount);
            request.box.minX = box.minX + static_cast<double>(columnStart) * unitsPerPixel;
            request.box.maxX = request.box.minX + static_cast<double>(columnCount) * unitsPerPixel;
            request.box.maxY = box.maxY - static_cast<double>(rowStart) * unitsPerPixel;
            request.box.minY = request.box.maxY - static_cast<double>(rowCount) * unitsPerPixel;
            requests.push_back(request);
        }
    }
    return requests;
}

// ---- ArcGIS REST ---------------------------------------------------------------

std::string arcgisExportUrl(const OnlineLayer& layer, const ImageRequest& request, int epsg)
{
    std::string query = "bbox=" + box4(request.box, false) + "&bboxSR=" + std::to_string(epsg) +
                        "&imageSR=" + std::to_string(epsg) + "&size=" +
                        std::to_string(request.width) + "," + std::to_string(request.height) +
                        "&dpi=96&format=png32&transparent=true";
    if (!layer.layerName.empty()) {
        query += "&layers=" + percentEncode("show:" + layer.layerName);
    }
    return withQuery(trimSlash(layer.endpoint) + "/export", query + "&f=image");
}

std::string arcgisExportImageUrl(const OnlineLayer& layer, const ImageRequest& request, int epsg)
{
    // Elevation as Float32 values in a GeoTIFF; imagery as the picture, a PNG.
    const std::string format =
        layer.kind == OnlineLayerKind::Elevation
            ? "&format=tiff&pixelType=F32&noData=-32767&interpolation=RSP_BilinearInterpolation"
            : "&format=png&pixelType=U8&interpolation=RSP_BilinearInterpolation";
    const std::string query = "bbox=" + box4(request.box, false) + "&bboxSR=" +
                              std::to_string(epsg) + "&imageSR=" + std::to_string(epsg) +
                              "&size=" + std::to_string(request.width) + "," +
                              std::to_string(request.height) + format + "&f=image";
    return withQuery(trimSlash(layer.endpoint) + "/exportImage", query);
}

std::string arcgisQueryUrl(const OnlineLayer& layer, const CrsBox& lonLat, std::uint64_t offset,
                           int count, const std::string& orderBy)
{
    std::string query = "where=" + percentEncode("1=1") + "&geometry=" + box4(lonLat, false) +
                        "&geometryType=esriGeometryEnvelope&inSR=4326"
                        "&spatialRel=esriSpatialRelIntersects&outFields=*&returnGeometry=true"
                        "&outSR=4326&resultOffset=" +
                        std::to_string(offset) + "&resultRecordCount=" + std::to_string(count);
    if (!orderBy.empty()) {
        query += "&orderByFields=" + percentEncode(orderBy);
    }
    return withQuery(trimSlash(layer.endpoint) + "/" + percentEncode(layer.layerName) + "/query",
                     query + "&f=geojson");
}

Result<QueryPage> readQueryPage(std::string_view json)
{
    Json document;
    try {
        document = Json::parse(json.begin(), json.end());
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the service's answer is not JSON", error.what());
    }
    if (document.is_object() && document.contains("error")) {
        const Json& error = document["error"];
        std::string message = error.is_object() ? error.value("message", std::string()) : "";
        if (error.is_object() && error.contains("details") && error["details"].is_array()) {
            for (const Json& detail : error["details"]) {
                if (detail.is_string()) {
                    message += " " + detail.get<std::string>();
                }
            }
        }
        return makeError(ErrorCode::FileImportFailure, "the service refused the query",
                         message.empty() ? error.dump() : message);
    }
    if (!document.is_object() || !document.contains("features") ||
        !document["features"].is_array()) {
        return makeError(ErrorCode::ParseFailure, "the answer holds no features list");
    }
    QueryPage page;
    page.features = document["features"].size();
    const auto flag = [](const Json& object) {
        return object.is_object() && object.contains("exceededTransferLimit") &&
               object["exceededTransferLimit"].is_boolean() &&
               object["exceededTransferLimit"].get<bool>();
    };
    page.exceededTransferLimit =
        flag(document) || (document.contains("properties") && flag(document["properties"]));
    return page;
}

bool queryHasMore(const QueryPage& page, int pageSize)
{
    if (page.features == 0) {
        return false;
    }
    return page.exceededTransferLimit || page.features >= static_cast<std::uint64_t>(pageSize);
}

// ---- OGC ----------------------------------------------------------------------------

std::string wmsGetMapUrl(const OnlineLayer& layer, const ImageRequest& request,
                         const std::string& crs, bool axisYX, const std::string& format)
{
    const std::string version = layer.version.empty() ? "1.3.0" : layer.version;
    const bool v13 = version.starts_with("1.3");
    std::string query = "SERVICE=WMS&VERSION=" + version + "&REQUEST=GetMap&LAYERS=" +
                        percentEncode(layer.layerName) + "&STYLES=&" + (v13 ? "CRS=" : "SRS=") +
                        percentEncode(crs) + "&BBOX=" + box4(request.box, v13 && axisYX) +
                        "&WIDTH=" + std::to_string(request.width) +
                        "&HEIGHT=" + std::to_string(request.height) +
                        "&FORMAT=" + percentEncode(format) + "&TRANSPARENT=TRUE";
    if (!layer.time.empty() && layer.time != "default") {
        query += "&TIME=" + percentEncode(layer.time);
    }
    return withQuery(layer.endpoint, query);
}

std::string wcsGetCoverageUrl(const OnlineLayer& layer, const ImageRequest& request,
                              const std::string& crs)
{
    const std::string query = "SERVICE=WCS&VERSION=1.0.0&REQUEST=GetCoverage&COVERAGE=" +
                              percentEncode(layer.layerName) + "&CRS=" + percentEncode(crs) +
                              "&RESPONSE_CRS=" + percentEncode(crs) + "&BBOX=" +
                              box4(request.box, false) + "&WIDTH=" +
                              std::to_string(request.width) + "&HEIGHT=" +
                              std::to_string(request.height) + "&FORMAT=GeoTIFF";
    return withQuery(layer.endpoint, query);
}

std::string wfsGetFeatureUrl(const OnlineLayer& layer, const CrsBox& box, const std::string& crs,
                             bool axisYX, std::uint64_t startIndex, int count)
{
    const std::string version = layer.version.empty() ? "2.0.0" : layer.version;
    const bool v2 = version.starts_with("2.");
    std::string query = "SERVICE=WFS&VERSION=" + version + "&REQUEST=GetFeature&" +
                        (v2 ? "TYPENAMES=" : "TYPENAME=") + percentEncode(layer.layerName) +
                        "&SRSNAME=" + percentEncode(crs) + "&BBOX=" + box4(box, axisYX) + "," +
                        percentEncode(crs);
    if (v2) {
        query += "&COUNT=" + std::to_string(count) + "&STARTINDEX=" + std::to_string(startIndex);
    } else {
        query += "&MAXFEATURES=" + std::to_string(count);
    }
    return withQuery(layer.endpoint, query);
}

std::string oapifItemsUrl(const OnlineLayer& layer, const CrsBox& lonLat, int limit)
{
    return withQuery(trimSlash(layer.endpoint) + "/collections/" + percentEncode(layer.layerName) +
                         "/items",
                     "bbox=" + box4(lonLat, false) + "&limit=" + std::to_string(limit) + "&f=json");
}

std::string nextLink(std::string_view json)
{
    try {
        const Json document = Json::parse(json.begin(), json.end());
        if (!document.is_object() || !document.contains("links") || !document["links"].is_array()) {
            return {};
        }
        for (const Json& link : document["links"]) {
            if (link.is_object() && link.value("rel", std::string()) == "next" &&
                link.value("method", std::string("GET")) == "GET") {
                return link.value("href", std::string());
            }
        }
    } catch (const Json::exception&) {
        return {};
    }
    return {};
}

Result<std::uint64_t> countGeoJsonFeatures(std::string_view json)
{
    try {
        const Json document = Json::parse(json.begin(), json.end());
        if (document.is_object() && document.contains("features") &&
            document["features"].is_array()) {
            return static_cast<std::uint64_t>(document["features"].size());
        }
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the answer is not JSON", error.what());
    }
    return makeError(ErrorCode::ParseFailure, "the answer is not a GeoJSON FeatureCollection");
}

// ---- tiles ----------------------------------------------------------------------------

double TileMatrix::unitsPerPixel(double metresPerUnit) const
{
    return scaleDenominator * kRenderingPixelMetres / metresPerUnit;
}

TileMatrixSet webMercatorTileMatrixSet(int maxZoom)
{
    TileMatrixSet set;
    set.id = "GoogleMapsCompatible";
    set.crs = "EPSG:3857";
    set.metresPerUnit = 1.0;
    for (int z = 0; z <= std::clamp(maxZoom, 0, 30); ++z) {
        TileMatrix matrix;
        matrix.id = std::to_string(z);
        matrix.scaleDenominator = kWebMercatorScale0 / std::ldexp(1.0, z);
        matrix.topLeftX = -kWebMercatorHalf;
        matrix.topLeftY = kWebMercatorHalf;
        matrix.tileWidth = 256;
        matrix.tileHeight = 256;
        matrix.matrixWidth = std::int64_t{1} << z;
        matrix.matrixHeight = std::int64_t{1} << z;
        set.matrices.push_back(matrix);
    }
    return set;
}

int chooseTileMatrix(const TileMatrixSet& set, double unitsPerPixel)
{
    if (set.matrices.empty()) {
        return 0;
    }
    for (std::size_t i = 0; i < set.matrices.size(); ++i) {
        // A tolerance of one part in a million, so a request for exactly a
        // level's resolution gets that level and not the next finer one.
        if (set.matrices[i].unitsPerPixel(set.metresPerUnit) <= unitsPerPixel * (1.0 + 1e-6)) {
            return static_cast<int>(i);
        }
    }
    return static_cast<int>(set.matrices.size()) - 1;
}

std::vector<TileIndex> tilesCovering(const TileMatrixSet& set, int level, const CrsBox& box)
{
    std::vector<TileIndex> tiles;
    if (level < 0 || level >= static_cast<int>(set.matrices.size()) || !box.valid()) {
        return tiles;
    }
    const TileMatrix& matrix = set.matrices[static_cast<std::size_t>(level)];
    const double pixel = matrix.unitsPerPixel(set.metresPerUnit);
    const double spanX = pixel * matrix.tileWidth;
    const double spanY = pixel * matrix.tileHeight;
    const auto clampColumn = [&](double value) {
        return std::clamp<std::int64_t>(static_cast<std::int64_t>(std::floor(value)), 0,
                                        matrix.matrixWidth - 1);
    };
    const auto clampRow = [&](double value) {
        return std::clamp<std::int64_t>(static_cast<std::int64_t>(std::floor(value)), 0,
                                        matrix.matrixHeight - 1);
    };
    // An edge exactly on a tile boundary belongs to the tile it opens, not
    // the one it closes: the max side is nudged inwards by a millionth of a
    // tile, so a box of exactly one tile asks for one tile.
    const std::int64_t firstColumn = clampColumn((box.minX - matrix.topLeftX) / spanX);
    const std::int64_t lastColumn = clampColumn((box.maxX - matrix.topLeftX) / spanX - 1e-6);
    const std::int64_t firstRow = clampRow((matrix.topLeftY - box.maxY) / spanY);
    const std::int64_t lastRow = clampRow((matrix.topLeftY - box.minY) / spanY - 1e-6);
    for (std::int64_t row = firstRow; row <= lastRow; ++row) {
        for (std::int64_t column = firstColumn; column <= lastColumn; ++column) {
            tiles.push_back(TileIndex{level, column, row});
        }
    }
    return tiles;
}

CrsBox tileBox(const TileMatrixSet& set, const TileIndex& tile)
{
    const TileMatrix& matrix = set.matrices[static_cast<std::size_t>(tile.matrix)];
    const double pixel = matrix.unitsPerPixel(set.metresPerUnit);
    const double spanX = pixel * matrix.tileWidth;
    const double spanY = pixel * matrix.tileHeight;
    CrsBox box;
    box.minX = matrix.topLeftX + static_cast<double>(tile.column) * spanX;
    box.maxX = box.minX + spanX;
    box.maxY = matrix.topLeftY - static_cast<double>(tile.row) * spanY;
    box.minY = box.maxY - spanY;
    return box;
}

std::string expandTileTemplate(std::string_view pattern, const TileIndex& tile,
                               const std::string& matrixId,
                               const std::map<std::string, std::string>& extra)
{
    std::string url(pattern);
    const std::string z = matrixId.empty() ? std::to_string(tile.matrix) : matrixId;
    url = replaceAll(url, "{z}", z);
    url = replaceAll(url, "{TileMatrix}", z);
    url = replaceAll(url, "{x}", std::to_string(tile.column));
    url = replaceAll(url, "{TileCol}", std::to_string(tile.column));
    url = replaceAll(url, "{y}", std::to_string(tile.row));
    url = replaceAll(url, "{TileRow}", std::to_string(tile.row));
    for (const auto& [name, value] : extra) {
        url = replaceAll(url, "{" + name + "}", percentEncode(value));
    }
    return url;
}

// ---- Copernicus -----------------------------------------------------------------------

std::vector<std::string> degreeTileUrls(std::string_view pattern, const CrsBox& lonLat)
{
    std::vector<std::string> urls;
    const int south = static_cast<int>(std::floor(lonLat.minY));
    const int north = static_cast<int>(std::ceil(lonLat.maxY)) - 1;
    const int west = static_cast<int>(std::floor(lonLat.minX));
    const int east = static_cast<int>(std::ceil(lonLat.maxX)) - 1;
    for (int lat = std::max(north, south); lat >= south; --lat) {
        for (int lon = west; lon <= std::max(east, west); ++lon) {
            char latText[16];
            char lonText[16];
            std::snprintf(latText, sizeof latText, "%c%02d", lat < 0 ? 'S' : 'N', std::abs(lat));
            std::snprintf(lonText, sizeof lonText, "%c%03d", lon < 0 ? 'W' : 'E', std::abs(lon));
            std::string url(pattern);
            url = replaceAll(url, "{lat}", latText);
            url = replaceAll(url, "{lon}", lonText);
            urls.push_back(url);
        }
    }
    return urls;
}

// ---- STAC ---------------------------------------------------------------------------

std::string stacSearchBody(const std::string& collection, const CrsBox& lonLat,
                           const std::string& fromDate, const std::string& toDate,
                           std::optional<double> maxCloud, int limit)
{
    Json body = Json::object();
    body["collections"] = Json::array({collection});
    body["bbox"] = Json::array({lonLat.minX, lonLat.minY, lonLat.maxX, lonLat.maxY});
    const std::string from = fromDate.empty() ? ".." : fromDate + "T00:00:00Z";
    const std::string to = toDate.empty() ? ".." : toDate + "T23:59:59Z";
    body["datetime"] = from + "/" + to;
    body["limit"] = limit;
    if (maxCloud) {
        body["query"] = {{"eo:cloud_cover", {{"lt", *maxCloud}}}};
    }
    body["sortby"] = Json::array({{{"field", "properties.eo:cloud_cover"}, {"direction", "asc"}}});
    return body.dump();
}

Result<std::vector<StacItem>> parseStacItems(std::string_view json)
{
    Json document;
    try {
        document = Json::parse(json.begin(), json.end());
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the STAC answer is not JSON", error.what());
    }
    std::vector<const Json*> features;
    if (document.is_object() && document.value("type", std::string()) == "FeatureCollection" &&
        document.contains("features") && document["features"].is_array()) {
        for (const Json& feature : document["features"]) {
            features.push_back(&feature);
        }
    } else if (document.is_object() && document.value("type", std::string()) == "Feature") {
        features.push_back(&document);
    } else {
        return makeError(ErrorCode::ParseFailure,
                         "the STAC answer is neither an item nor an item collection");
    }
    std::vector<StacItem> items;
    for (const Json* feature : features) {
        StacItem item;
        item.id = feature->value("id", std::string());
        if (feature->contains("properties") && (*feature)["properties"].is_object()) {
            const Json& properties = (*feature)["properties"];
            item.datetime = properties.value("datetime", std::string());
            if (properties.contains("eo:cloud_cover") && properties["eo:cloud_cover"].is_number()) {
                item.cloudCover = properties["eo:cloud_cover"].get<double>();
            }
        }
        if (feature->contains("bbox")) {
            if (const auto box = bboxOf((*feature)["bbox"])) {
                item.bounds = *box;
            }
        }
        if (feature->contains("assets") && (*feature)["assets"].is_object()) {
            for (const auto& [key, asset] : (*feature)["assets"].items()) {
                if (asset.is_object() && asset.contains("href") && asset["href"].is_string()) {
                    item.assets[key] = asset["href"].get<std::string>();
                }
            }
        }
        items.push_back(std::move(item));
    }
    return items;
}

StacChoice chooseStacItems(const std::vector<StacItem>& items, const CrsBox& lonLat)
{
    // Grouped by acquisition day: the tiles of one pass mosaic without seams
    // of season or light, which tiles of different days do not.
    std::map<std::string, std::vector<const StacItem*>> days;
    for (const StacItem& item : items) {
        if (item.bounds.valid() && intersects(item.bounds, lonLat)) {
            days[item.datetime.substr(0, 10)].push_back(&item);
        }
    }
    // Whether the day's footprints cover the area, tested on a 5 x 5 grid of
    // points across it: the bounding boxes are what a search returns, and a
    // grid finds the gap between two of them that a corner test would miss.
    const auto covers = [&](const std::vector<const StacItem*>& dayItems) {
        for (int i = 0; i <= 4; ++i) {
            for (int j = 0; j <= 4; ++j) {
                const double x = lonLat.minX + lonLat.width() * i / 4.0;
                const double y = lonLat.minY + lonLat.height() * j / 4.0;
                const bool inside =
                    std::any_of(dayItems.begin(), dayItems.end(), [&](const StacItem* item) {
                        return x >= item->bounds.minX && x <= item->bounds.maxX &&
                               y >= item->bounds.minY && y <= item->bounds.maxY;
                    });
                if (!inside) {
                    return false;
                }
            }
        }
        return true;
    };
    StacChoice best;
    bool haveBest = false;
    for (const auto& [day, dayItems] : days) {
        double cloud = 0.0;
        for (const StacItem* item : dayItems) {
            cloud = std::max(cloud, item->cloudCover.value_or(100.0));
        }
        const bool full = covers(dayItems);
        // Covering beats not covering; then less cloud; then the later day
        // (the map iterates days in ascending order, so <= prefers later).
        const bool better = !haveBest || (full && !best.coversArea) ||
                            (full == best.coversArea && cloud <= best.cloudCover);
        if (better) {
            best.items.clear();
            for (const StacItem* item : dayItems) {
                best.items.push_back(*item);
            }
            best.day = day;
            best.cloudCover = cloud;
            best.coversArea = full;
            haveBest = true;
        }
    }
    return best;
}

std::string httpHref(const std::string& href)
{
    if (!href.starts_with("s3://")) {
        return href;
    }
    const std::string rest = href.substr(5);
    const std::size_t slash = rest.find('/');
    if (slash == std::string::npos) {
        return href;
    }
    return "https://" + rest.substr(0, slash) + ".s3.amazonaws.com" + rest.substr(slash);
}

// ---- Overpass -------------------------------------------------------------------------

Result<std::string> overpassQuery(std::string_view filter, const CrsBox& lonLat,
                                  int timeoutSeconds, std::uint64_t maxBytes)
{
    if (!lonLat.valid()) {
        return makeError(ErrorCode::InvalidArgument, "the area is empty or inverted");
    }
    std::string tests;
    std::string term;
    const std::string text(katana::core::trimmed(filter));
    if (text.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "an OpenStreetMap query needs a tag: tag=building, tag=highway=primary");
    }
    for (std::size_t start = 0; start <= text.size();) {
        std::size_t end = text.find(';', start);
        if (end == std::string::npos) {
            end = text.size();
        }
        term = std::string(katana::core::trimmed(std::string_view(text).substr(start, end - start)));
        start = end + 1;
        if (term.empty()) {
            continue;
        }
        const std::size_t notEqual = term.find("!=");
        const std::size_t equal = term.find('=');
        std::string key;
        std::string value;
        std::string op;
        if (notEqual != std::string::npos) {
            key = std::string(katana::core::trimmed(std::string_view(term).substr(0, notEqual)));
            value = std::string(katana::core::trimmed(std::string_view(term).substr(notEqual + 2)));
            op = "!=";
        } else if (equal != std::string::npos) {
            key = std::string(katana::core::trimmed(std::string_view(term).substr(0, equal)));
            value = std::string(katana::core::trimmed(std::string_view(term).substr(equal + 1)));
            op = "=";
        } else {
            key = term;
        }
        if (!plainTagText(key) || (!op.empty() && !plainTagText(value))) {
            return makeError(ErrorCode::InvalidArgument,
                             "an OpenStreetMap tag test is key, key=value or key!=value, of "
                             "letters, digits and _ : - . only",
                             term);
        }
        tests += "[\"" + key + "\"" + (op.empty() ? "" : op + "\"" + value + "\"") + "]";
    }
    if (tests.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the OpenStreetMap filter holds no tag");
    }
    // Overpass's bbox is (south, west, north, east).
    const std::string bbox = formatNumber(lonLat.minY) + "," + formatNumber(lonLat.minX) + "," +
                             formatNumber(lonLat.maxY) + "," + formatNumber(lonLat.maxX);
    return "[out:xml][timeout:" + std::to_string(std::max(1, timeoutSeconds)) + "][maxsize:" +
           std::to_string(maxBytes) + "][bbox:" + bbox + "];\n(\n  node" + tests + ";\n  way" +
           tests + ";\n  relation" + tests + ";\n);\n(._;>;);\nout body;\n";
}

double areaKm2(const CrsBox& lonLat)
{
    constexpr double kDegree = std::numbers::pi / 180.0;
    // The area of a latitude band's slice on the sphere: R^2 (lon2 - lon1)
    // (sin lat2 - sin lat1).
    return kEarthRadiusKm * kEarthRadiusKm * (lonLat.maxX - lonLat.minX) * kDegree *
           std::abs(std::sin(lonLat.maxY * kDegree) - std::sin(lonLat.minY * kDegree));
}

// ---- CKAN -----------------------------------------------------------------------------------

std::string ckanSearchUrl(const std::string& endpoint, const std::string& words, int rows)
{
    // Datasets with at least one web-service resource, as CKAN's facet query
    // res_format spells them.
    const std::string formats =
        "res_format:(WMS OR WFS OR WMTS OR WCS OR \"ESRI REST\" OR \"OGC API\")";
    return withQuery(endpoint, "q=" + percentEncode(words) + "&fq=" + percentEncode(formats) +
                                   "&rows=" + std::to_string(rows));
}

Result<std::vector<CkanResource>> parseCkanSearch(std::string_view json)
{
    Json document;
    try {
        document = Json::parse(json.begin(), json.end());
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the catalogue's answer is not JSON", error.what());
    }
    if (!document.is_object() || !document.value("success", false) ||
        !document.contains("result") || !document["result"].contains("results")) {
        return makeError(ErrorCode::ParseFailure, "the catalogue's answer holds no results");
    }
    static const std::set<std::string> kServiceFormats = {"WMS", "WFS", "WMTS", "WCS",
                                                          "ESRI REST", "OGC API", "OGC API - FEATURES"};
    std::vector<CkanResource> resources;
    for (const Json& dataset : document["result"]["results"]) {
        if (!dataset.is_object() || !dataset.contains("resources")) {
            continue;
        }
        const std::string datasetTitle = dataset.value("title", std::string());
        const std::string licence = dataset.value("license_title", std::string());
        for (const Json& resource : dataset["resources"]) {
            if (!resource.is_object()) {
                continue;
            }
            std::string format = resource.value("format", std::string());
            std::transform(format.begin(), format.end(), format.begin(), [](char c) {
                return c >= 'a' && c <= 'z' ? static_cast<char>(c - 'a' + 'A') : c;
            });
            const std::string url = resource.value("url", std::string());
            if (!kServiceFormats.contains(format) || url.empty()) {
                continue;
            }
            resources.push_back(CkanResource{datasetTitle, resource.value("name", datasetTitle),
                                             format, url, licence});
        }
    }
    return resources;
}

} // namespace katana::interop
