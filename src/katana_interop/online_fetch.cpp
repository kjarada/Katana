#include "katana/interop/online_fetch.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <format>
#include <fstream>
#include <set>
#include <sstream>

#include "katana/core/text.hpp"
#include "katana/interop/online_discovery.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::gis::CrsBox;
using katana::gis::HttpRequest;
namespace fs = std::filesystem;

// Imagery with no resolution asked for: the area's longer side over this
// many pixels - a sharp full-screen backdrop, 16 MB of RGBA at most.
constexpr double kDefaultImageryPixels = 2048.0;
// Elevation with neither a resolution asked for nor one in the catalogue.
constexpr double kDefaultElevationPixels = 1024.0;
// How far below the level a resolution asks for a tile service may fall to
// stay within its tile budget before the import is refused instead: three
// levels is an image eight times coarser, about as far as "a little blurry"
// stretches.
constexpr int kMaxZoomReduction = 3;
// The most image requests one import may split into (planImageRequests).
constexpr int kMaxImageRequests = 64;
// A STAC search asks for this many items: a month of Sentinel-2 over one
// area is a few dozen, and the choice is made among them.
constexpr int kStacSearchLimit = 100;

std::string today()
{
    const auto now = std::chrono::floor<std::chrono::days>(std::chrono::system_clock::now());
    return std::format("{:%F}", now);
}

std::string daysBefore(int days)
{
    const auto then = std::chrono::floor<std::chrono::days>(std::chrono::system_clock::now()) -
                      std::chrono::days(days);
    return std::format("{:%F}", then);
}

std::string lower(std::string_view text)
{
    return katana::core::lowered(text);
}

// A file extension for an answer, from its Content-Type, else from the URL's
// path, else ".bin": GDAL chooses its driver partly by extension, so a PNG
// saved as .bin is harder for it to open than one saved as .png.
std::string extensionFor(const std::string& contentType, const std::string& url)
{
    const std::string type = lower(contentType);
    if (type.find("png") != std::string::npos) {
        return ".png";
    }
    if (type.find("jpeg") != std::string::npos || type.find("jpg") != std::string::npos) {
        return ".jpg";
    }
    if (type.find("tiff") != std::string::npos) {
        return ".tif";
    }
    const std::string path = lower(url.substr(0, url.find('?')));
    for (const char* known : {".png", ".jpg", ".jpeg", ".tif", ".tiff", ".json", ".geojson", ".xml"}) {
        if (path.ends_with(known)) {
            return known;
        }
    }
    return ".bin";
}

// The first bytes of a file, to tell an image from an error document a
// service returned with a 200: a WMS ServiceException, an ArcGIS {"error"}.
std::string headOf(const fs::path& path, std::size_t bytes = 512)
{
    std::ifstream in(path, std::ios::binary);
    std::string head(bytes, '\0');
    in.read(head.data(), static_cast<std::streamsize>(bytes));
    head.resize(static_cast<std::size_t>(in.gcount()));
    return head;
}

bool looksLikeImage(const std::string& head)
{
    return head.starts_with("\x89PNG") || head.starts_with("\xFF\xD8") ||
           head.starts_with("II*") || head.starts_with("MM\0*") || head.starts_with("GIF8") ||
           head.starts_with("RIFF") || head.starts_with("II+") || head.starts_with("MM\0+");
}

std::string readWhole(const fs::path& path)
{
    std::ifstream in(path, std::ios::binary);
    std::ostringstream text;
    text << in.rdbuf();
    return text.str();
}

// An error document (XML or JSON) a service sent where data was expected,
// reduced to the words a person can act on.
std::string serviceMessage(const std::string& head)
{
    std::string text;
    bool inTag = false;
    for (const char c : head) {
        if (c == '<') {
            inTag = true;
            continue;
        }
        if (c == '>') {
            inTag = false;
            text.push_back(' ');
            continue;
        }
        if (!inTag) {
            text.push_back(c == '\n' || c == '\r' || c == '\t' ? ' ' : c);
        }
    }
    std::string collapsed;
    for (const char c : text) {
        if (c == ' ' && (collapsed.empty() || collapsed.back() == ' ')) {
            continue;
        }
        collapsed.push_back(c);
    }
    return std::string(katana::core::trimmed(collapsed)).substr(0, 300);
}

HttpRequest makeRequest(const std::string& url, const OnlineEnvironment& environment)
{
    HttpRequest request;
    request.url = url;
    request.userAgent = environment.userAgent;
    request.timeoutSeconds = environment.timeoutSeconds;
    request.retries = environment.retries;
    request.maxBytes = environment.maxResponseBytes;
    return request;
}

std::string cacheKey(const HttpRequest& request)
{
    return katana::gis::sha256Hex((request.postBody.empty() ? "GET\n" : "POST\n") + request.url +
                                  "\n" + request.postBody);
}

fs::path cachePath(const fs::path& root, const std::string& folder, const std::string& key,
                   const std::string& extension)
{
    return root / folder / key.substr(0, 2) / (key + extension);
}

bool fresh(const fs::path& path, int maxAgeDays, bool offline)
{
    std::error_code error;
    if (!fs::is_regular_file(path, error) || fs::file_size(path, error) == 0) {
        return false;
    }
    if (offline) {
        return true;
    }
    const auto written = fs::last_write_time(path, error);
    if (error) {
        return false;
    }
    const auto age = fs::file_time_type::clock::now() - written;
    return age < std::chrono::hours(24) * std::max(0, maxAgeDays);
}

Status writeAtomically(const fs::path& path, const std::string& bytes)
{
    std::error_code error;
    fs::create_directories(path.parent_path(), error);
    const fs::path temporary = path.string() + ".part";
    {
        std::ofstream out(temporary, std::ios::binary | std::ios::trunc);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        if (!out) {
            fs::remove(temporary, error);
            return makeError(ErrorCode::FileExportFailure, "could not write to the online cache",
                             temporary.string());
        }
    }
    fs::rename(temporary, path, error);
    if (error) {
        fs::remove(temporary, error);
        return makeError(ErrorCode::FileExportFailure, "could not write to the online cache",
                         path.string());
    }
    return {};
}

// Where one fetched answer lives when there is no cache: a scratch folder of
// the system's, removed by nobody but small and named by hash like the cache.
fs::path scratchRoot()
{
    std::error_code error;
    const fs::path root = fs::temp_directory_path(error) / "katana-online";
    return root;
}

fs::path cacheRoot(const OnlineEnvironment& environment)
{
    return environment.cacheDirectory.empty() ? scratchRoot() : environment.cacheDirectory;
}

CrsBox lonLatOf(const LonLatBox& box)
{
    return CrsBox{box[0], box[1], box[2], box[3]};
}

bool overlaps(const CrsBox& a, const CrsBox& b)
{
    return a.minX < b.maxX && b.minX < a.maxX && a.minY < b.maxY && b.minY < a.maxY;
}

struct Prepared {
    CrsBox lonLat;     // the area in WGS 84 longitude/latitude
    CrsBox target;     // the area in the project's CRS
    std::string endpoint;
};

Result<Prepared> prepare(const OnlineLayer& layer, const OnlineRequestOptions& options,
                         const OnlineEnvironment& environment)
{
    if (options.targetCrs.empty()) {
        return makeError(ErrorCode::InvalidCRS,
                         "the project has no coordinate system, so web data has nowhere to go; "
                         "set one (the project's metadata) or give crs=EPSG:<code>");
    }
    if (!options.area.valid()) {
        return makeError(ErrorCode::InvalidArgument, "the area is empty or inverted");
    }
    const std::string areaCrs = options.areaCrs.empty() ? options.targetCrs : options.areaCrs;
    Prepared prepared;
    auto lonLat = katana::gis::transformBox(options.area, areaCrs, "EPSG:4326");
    if (!lonLat) {
        return lonLat.error();
    }
    prepared.lonLat = *lonLat;
    if (!overlaps(prepared.lonLat, lonLatOf(layer.coverage))) {
        return makeError(ErrorCode::InvalidArgument,
                         "the area is outside what " + layer.providerTitle + " covers (" +
                             formatNumber(layer.coverage[0]) + "," + formatNumber(layer.coverage[1]) +
                             " to " + formatNumber(layer.coverage[2]) + "," +
                             formatNumber(layer.coverage[3]) + ")",
                         formatNumber(prepared.lonLat.minX) + "," + formatNumber(prepared.lonLat.minY) +
                             " to " + formatNumber(prepared.lonLat.maxX) + "," +
                             formatNumber(prepared.lonLat.maxY));
    }
    if (areaCrs == options.targetCrs) {
        prepared.target = options.area;
    } else {
        auto target = katana::gis::transformBox(options.area, areaCrs, options.targetCrs);
        if (!target) {
            return target.error();
        }
        prepared.target = *target;
    }
    auto endpoint = endpointWithKey(layer, environment);
    if (!endpoint) {
        return endpoint.error();
    }
    prepared.endpoint = *endpoint;
    return prepared;
}

// The layer as the request builders should see it: its endpoint with the
// key filled in.
OnlineLayer withEndpoint(const OnlineLayer& layer, const std::string& endpoint)
{
    OnlineLayer copy = layer;
    copy.endpoint = endpoint;
    return copy;
}

double defaultResolution(const OnlineLayer& layer, const CrsBox& target)
{
    const double longer = std::max(target.width(), target.height());
    if (layer.kind == OnlineLayerKind::Elevation) {
        if (layer.resolution) {
            // The data's own spacing, unless that would be over the pixel
            // budget - then the budget's.
            return std::max(*layer.resolution, longer / 8192.0);
        }
        return longer / kDefaultElevationPixels;
    }
    return longer / kDefaultImageryPixels;
}

// ---- rasters ---------------------------------------------------------------------------

// How many units of `serviceCrs` one unit of the project's CRS is, at the
// area's centre: two short steps, east and north, measured in both. The ratio
// of the two boxes' areas was used first and chose a zoom one level too fine:
// a box in MGA is rotated against Web Mercator by the grid convergence, so
// its bounding box is larger than the area by a few per cent - enough to cross
// a level boundary.
Result<double> localScale(const std::string& targetCrs, const CrsBox& target,
                          const std::string& serviceCrs)
{
    const double cx = 0.5 * (target.minX + target.maxX);
    const double cy = 0.5 * (target.minY + target.maxY);
    const double step = 0.01 * std::max(target.width(), target.height());
    auto centre = katana::gis::transformPoint(cx, cy, targetCrs, serviceCrs);
    auto east = katana::gis::transformPoint(cx + step, cy, targetCrs, serviceCrs);
    auto north = katana::gis::transformPoint(cx, cy + step, targetCrs, serviceCrs);
    if (!centre || !east || !north) {
        return makeError(ErrorCode::InvalidCRS,
                         "the area cannot be expressed in the service's coordinate system");
    }
    const double eastward = std::hypot((*east)[0] - (*centre)[0], (*east)[1] - (*centre)[1]);
    const double northward = std::hypot((*north)[0] - (*centre)[0], (*north)[1] - (*centre)[1]);
    return 0.5 * (eastward + northward) / step;
}

struct RasterPlan {
    std::vector<katana::gis::WarpSource> sources;
    bool skipMissing = false;
};

Result<RasterPlan> tileSources(const OnlineLayer& layer, const Prepared& prepared,
                               double resolution, const OnlineRequestOptions& options,
                               const OnlineEnvironment& environment, OnlineStats& stats,
                               std::vector<std::string>& warnings, const std::stop_token& stop,
                               const OnlineProgress& progress)
{
    const TileMatrixSet set = webMercatorTileMatrixSet(layer.maxZoom);
    auto box = katana::gis::transformBox(prepared.lonLat, "EPSG:4326", "EPSG:3857");
    if (!box) {
        return box.error();
    }
    // Web Mercator metres are not ground metres: at latitude phi one is
    // cos(phi) of a metre. The ratio of the two boxes carries that, and any
    // other scale between the project's CRS and the grid.
    auto ratio = localScale(options.targetCrs, prepared.target, "EPSG:3857");
    if (!ratio) {
        return ratio.error();
    }
    const int wanted = chooseTileMatrix(set, resolution * *ratio);
    int level = wanted;
    std::vector<TileIndex> tiles = tilesCovering(set, level, *box);
    while (static_cast<int>(tiles.size()) > layer.maxTiles && level > 0) {
        --level;
        tiles = tilesCovering(set, level, *box);
    }
    if (static_cast<int>(tiles.size()) > layer.maxTiles || wanted - level > kMaxZoomReduction) {
        return makeError(ErrorCode::InvalidArgument,
                         "the area needs more than the " + std::to_string(layer.maxTiles) +
                             " tiles " + layer.providerTitle +
                             " allows one import at this resolution; choose a smaller area or a "
                             "coarser resolution");
    }
    if (level < wanted) {
        warnings.push_back("tiles fetched at zoom " + std::to_string(level) + " rather than " +
                           std::to_string(wanted) + " to stay within " +
                           std::to_string(layer.maxTiles) + " tiles");
    }
    const std::map<std::string, std::string> extra = {
        {"layer", layer.layerName},
        {"time", options.time.empty() ? (layer.time.empty() ? "default" : layer.time) : options.time}};
    RasterPlan plan;
    int missing = 0;
    for (std::size_t i = 0; i < tiles.size(); ++i) {
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        if (progress) {
            progress(0.6 * static_cast<double>(i) / static_cast<double>(tiles.size()),
                     "Fetching tile " + std::to_string(i + 1) + " of " + std::to_string(tiles.size()));
        }
        const std::string url = expandTileTemplate(prepared.endpoint, tiles[i], {}, extra);
        auto file = fetchToCache(makeRequest(url, environment), extensionFor({}, url),
                                 layer.cacheDays, environment, stats, stop);
        if (!file) {
            // A tile the service has not rendered (the sea at zoom 19) is a
            // hole in the mosaic, not a failed import.
            if (file.error().code == ErrorCode::NotFound) {
                ++missing;
                continue;
            }
            return file.error();
        }
        if (!looksLikeImage(headOf(*file))) {
            return makeError(ErrorCode::FileImportFailure, "the tile service sent something other than an image",
                             serviceMessage(headOf(*file)));
        }
        plan.sources.push_back(katana::gis::WarpSource{file->string(), tileBox(set, tiles[i]), "EPSG:3857"});
    }
    if (missing > 0) {
        warnings.push_back(std::to_string(missing) + " of " + std::to_string(tiles.size()) +
                           " tiles do not exist at the service; those parts are empty");
    }
    if (plan.sources.empty()) {
        return makeError(ErrorCode::NotFound, "the service has no tiles for this area");
    }
    return plan;
}

Result<RasterPlan> imageSources(const OnlineLayer& layer, const Prepared& prepared,
                                double resolution, const std::string& targetCrs,
                                const OnlineEnvironment& environment,
                                OnlineStats& stats, const std::stop_token& stop,
                                const OnlineProgress& progress)
{
    const bool arcgis = layer.type == OnlineServiceType::ArcgisExport ||
                        layer.type == OnlineServiceType::ArcgisExportImage;
    const std::string crs = !layer.crs.empty() ? layer.crs
                            : arcgis             ? "EPSG:3857"
                                                 : "EPSG:4326";
    // The area as the service sees it; from longitude/latitude, which every
    // area has, rather than chaining through the project's CRS.
    auto serviceBox = katana::gis::transformBox(prepared.lonLat, "EPSG:4326", crs);
    if (!serviceBox) {
        return serviceBox.error();
    }
    auto ratio = localScale(targetCrs, prepared.target, crs);
    if (!ratio) {
        return ratio.error();
    }
    auto requests = planImageRequests(*serviceBox, resolution * *ratio, layer.maxRequestPixels,
                                      kMaxImageRequests);
    if (!requests) {
        return requests.error();
    }
    const OnlineLayer resolved = withEndpoint(layer, prepared.endpoint);
    int epsg = 0;
    if (arcgis) {
        const auto code = katana::gis::crsEpsgCode(crs);
        if (!code) {
            return makeError(ErrorCode::InvalidCRS, "ArcGIS requests need an EPSG code", crs);
        }
        epsg = *code;
    }
    bool axisYX = false;
    if (layer.type == OnlineServiceType::Wms) {
        auto swap = katana::gis::crsAxisIsYX(crs);
        if (!swap) {
            return swap.error();
        }
        axisYX = *swap;
    }
    RasterPlan plan;
    for (std::size_t i = 0; i < requests->size(); ++i) {
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        if (progress) {
            progress(0.6 * static_cast<double>(i) / static_cast<double>(requests->size()),
                     "Fetching image " + std::to_string(i + 1) + " of " +
                         std::to_string(requests->size()));
        }
        const ImageRequest& request = (*requests)[i];
        std::string url;
        std::string extension = ".png";
        switch (layer.type) {
        case OnlineServiceType::ArcgisExport:
            url = arcgisExportUrl(resolved, request, epsg);
            break;
        case OnlineServiceType::ArcgisExportImage:
            url = arcgisExportImageUrl(resolved, request, epsg);
            extension = layer.kind == OnlineLayerKind::Elevation ? ".tif" : ".png";
            break;
        case OnlineServiceType::Wms:
            url = wmsGetMapUrl(resolved, request, crs, axisYX);
            break;
        case OnlineServiceType::Wcs:
            url = wcsGetCoverageUrl(resolved, request, crs);
            extension = ".tif";
            break;
        default:
            return makeError(ErrorCode::Internal, "not an image service");
        }
        auto file = fetchToCache(makeRequest(url, environment), extension, layer.cacheDays,
                                 environment, stats, stop);
        if (!file) {
            return file.error();
        }
        const std::string head = headOf(*file);
        if (!looksLikeImage(head)) {
            // Not kept: the next run must ask again rather than read the
            // refusal back from the cache.
            std::error_code ignored;
            fs::remove(*file, ignored);
            return makeError(ErrorCode::FileImportFailure,
                             "the service answered with a message instead of an image",
                             serviceMessage(head));
        }
        plan.sources.push_back(katana::gis::WarpSource{file->string(), request.box, crs});
    }
    return plan;
}

std::string vsicurl(const std::string& url)
{
    if (url.starts_with("file://")) {
        // The path a file URL names, for GDAL to open directly.
        std::string path = url.substr(7);
        if (path.size() > 2 && path[0] == '/' && path[2] == ':') {
            path.erase(path.begin());
        }
        return path;
    }
    return "/vsicurl/" + url;
}

Result<RasterPlan> cogSources(const OnlineLayer& layer, const Prepared& prepared)
{
    RasterPlan plan;
    if (layer.tiling == "degree") {
        for (const std::string& url : degreeTileUrls(prepared.endpoint, prepared.lonLat)) {
            plan.sources.push_back(katana::gis::WarpSource{vsicurl(url), std::nullopt, {}});
        }
        // Over the sea there is no tile: skipped, and the mosaic is empty
        // there.
        plan.skipMissing = true;
    } else {
        plan.sources.push_back(katana::gis::WarpSource{vsicurl(prepared.endpoint), std::nullopt, {}});
    }
    return plan;
}

Result<RasterPlan> stacSources(const OnlineLayer& layer, const Prepared& prepared,
                               const OnlineRequestOptions& options,
                               const OnlineEnvironment& environment, OnlineStats& stats,
                               std::vector<std::string>& warnings, const std::stop_token& stop,
                               const OnlineProgress& progress)
{
    std::vector<StacItem> items;
    const bool search = lower(prepared.endpoint).ends_with("/search");
    if (search) {
        const std::string from = !options.fromDate.empty() ? options.fromDate
                                 : layer.days > 0          ? daysBefore(layer.days)
                                                           : std::string();
        const std::string to = !options.toDate.empty() ? options.toDate : today();
        HttpRequest request = makeRequest(prepared.endpoint, environment);
        request.postBody = stacSearchBody(layer.layerName, prepared.lonLat, from, to,
                                          options.maxCloud ? options.maxCloud : std::optional<double>(layer.maxCloud),
                                          kStacSearchLimit);
        request.postContentType = "application/geo+json";
        if (progress) {
            progress(-1.0, "Searching " + layer.providerTitle);
        }
        // A search is fresh for a day: new scenes arrive daily.
        auto file = fetchToCache(request, ".json", 1, environment, stats, stop);
        if (!file) {
            return file.error();
        }
        auto parsed = parseStacItems(readWhole(*file));
        if (!parsed) {
            return parsed.error();
        }
        items = std::move(*parsed);
    } else {
        auto file = fetchToCache(makeRequest(prepared.endpoint, environment), ".json",
                                 layer.cacheDays, environment, stats, stop);
        if (!file) {
            return file.error();
        }
        auto parsed = parseStacItems(readWhole(*file));
        if (!parsed) {
            return parsed.error();
        }
        items = std::move(*parsed);
    }
    const StacChoice choice = chooseStacItems(items, prepared.lonLat);
    if (choice.items.empty()) {
        return makeError(ErrorCode::NotFound,
                         "no scene meets the area" +
                             std::string(search ? " between those dates under that cloud cover; "
                                                  "widen the dates (from= to=) or raise cloud="
                                                : ""));
    }
    if (!choice.coversArea) {
        warnings.push_back("the scenes of " + choice.day + " cover only part of the area");
    }
    warnings.push_back("scene date " + choice.day + ", cloud cover " +
                       formatNumber(std::round(choice.cloudCover * 10.0) / 10.0) + "%");
    RasterPlan plan;
    const bool composite = layer.asset.find(',') != std::string::npos;
    for (const StacItem& item : choice.items) {
        if (!composite) {
            const auto asset = item.assets.find(layer.asset);
            if (asset == item.assets.end()) {
                return makeError(ErrorCode::NotFound, "the scene has no '" + layer.asset + "' asset", item.id);
            }
            plan.sources.push_back(katana::gis::WarpSource{vsicurl(httpHref(asset->second)), std::nullopt, {}});
            continue;
        }
        std::vector<std::string> bands;
        for (const char* key : {"red", "green", "blue"}) {
            const auto asset = item.assets.find(key);
            if (asset == item.assets.end()) {
                return makeError(ErrorCode::NotFound, std::string("the scene has no '") + key + "' band", item.id);
            }
            bands.push_back(vsicurl(httpHref(asset->second)));
        }
        const fs::path vrt = cachePath(cacheRoot(environment), "products",
                                       katana::gis::sha256Hex("composite\n" + item.id + "\n" + bands[0]), ".vrt");
        // Sentinel-2 L2A reflectance times 10000. Earth Search's COGs have
        // the offset of processing baseline 04.00 already removed (measured:
        // Sydney Harbour reads 35 in red on 2026-09-20), so 0..3000 - the
        // usual true-colour stretch - holds dark water to bright roofs.
        if (auto built = katana::gis::buildTrueColourVrt(bands, 0.0, 3000.0, vrt); !built) {
            return built.error();
        }
        plan.sources.push_back(katana::gis::WarpSource{vrt.string(), std::nullopt, {}});
    }
    return plan;
}

std::string productKey(const OnlineLayer& layer, const Prepared& prepared,
                       const OnlineRequestOptions& options, double resolution)
{
    std::ostringstream key;
    key << "product\n"
        << toString(layer.type) << "\n"
        << prepared.endpoint << "\n"
        << layer.layerName << "\n"
        << layer.asset << "\n"
        << toString(layer.kind) << "\n"
        << options.targetCrs << "\n"
        << formatNumber(prepared.target.minX) << "," << formatNumber(prepared.target.minY) << ","
        << formatNumber(prepared.target.maxX) << "," << formatNumber(prepared.target.maxY) << "\n"
        << formatNumber(resolution) << "\n"
        << options.fromDate << "/" << options.toDate << "/"
        << (options.maxCloud ? formatNumber(*options.maxCloud) : "") << "/" << options.time;
    // A STAC search with no dates means "the last N days": the same words
    // tomorrow ask for a different window, so the day is part of the key.
    if (layer.type == OnlineServiceType::Stac && options.toDate.empty()) {
        key << "\n" << today();
    }
    return katana::gis::sha256Hex(key.str());
}

Result<OnlineImport> fetchRaster(const OnlineLayer& layer, const OnlineRequestOptions& options,
                                 const OnlineEnvironment& environment, const std::stop_token& stop,
                                 const OnlineProgress& progress)
{
    auto prepared = prepare(layer, options, environment);
    if (!prepared) {
        return prepared.error();
    }
    OnlineImport result;
    result.kind = layer.kind;
    const double resolution =
        options.resolution ? *options.resolution : defaultResolution(layer, prepared->target);
    if (!(resolution > 0.0) || !std::isfinite(resolution)) {
        return makeError(ErrorCode::InvalidArgument, "the resolution must be positive");
    }
    result.resolution = resolution;
    // The finished raster's own size, refused before a single request.
    const double pixels = std::ceil(prepared->target.width() / resolution) *
                          std::ceil(prepared->target.height() / resolution);
    if (pixels > static_cast<double>(environment.maxPixels)) {
        const double fits = resolution * std::sqrt(pixels / static_cast<double>(environment.maxPixels));
        return makeError(ErrorCode::InvalidArgument,
                         "the area at this resolution is " + formatNumber(std::round(pixels / 1e6)) +
                             " million pixels, more than the " +
                             formatNumber(std::round(static_cast<double>(environment.maxPixels) / 1e5) / 10.0) +
                             " million allowed; use res=" + formatNumber(std::ceil(fits * 100.0) / 100.0) +
                             " or coarser, or a smaller area");
    }

    const fs::path product = cachePath(cacheRoot(environment), "products",
                                       productKey(layer, *prepared, options, resolution), ".tif");
    if (!fresh(product, layer.cacheDays, environment.offline)) {
        if (environment.offline) {
            return makeError(ErrorCode::NotFound, "working offline, and the cache has no copy of this");
        }
        Result<RasterPlan> plan = makeError(ErrorCode::Internal, "no plan");
        switch (layer.type) {
        case OnlineServiceType::XyzTiles:
        case OnlineServiceType::Wmts:
            plan = tileSources(layer, *prepared, resolution, options, environment, result.stats,
                               result.warnings, stop, progress);
            break;
        case OnlineServiceType::ArcgisExport:
        case OnlineServiceType::ArcgisExportImage:
        case OnlineServiceType::Wms:
        case OnlineServiceType::Wcs:
            plan = imageSources(layer, *prepared, resolution, options.targetCrs, environment,
                                result.stats, stop, progress);
            break;
        case OnlineServiceType::Cog:
            plan = cogSources(layer, *prepared);
            break;
        case OnlineServiceType::Stac:
            plan = stacSources(layer, *prepared, options, environment, result.stats, result.warnings,
                               stop, progress);
            break;
        default:
            return makeError(ErrorCode::Unsupported,
                             std::string("a ") + toString(layer.type) + " service gives no raster");
        }
        if (!plan) {
            return plan.error();
        }
        if (progress) {
            progress(0.6, "Warping into the project's coordinate system");
        }
        katana::gis::WarpOptions warp;
        warp.targetCrs = options.targetCrs;
        warp.targetBounds = prepared->target;
        warp.resolution = resolution;
        warp.elevation = layer.kind == OnlineLayerKind::Elevation;
        warp.maxPixels = environment.maxPixels;
        warp.timeoutSeconds = environment.timeoutSeconds;
        warp.userAgent = environment.userAgent;
        warp.skipMissingSources = plan->skipMissing;
        const fs::path partial = product.string() + ".part.tif";
        auto warped = katana::gis::warpToGeoTiff(plan->sources, warp, partial, stop,
                                                 [&](double fraction) {
                                                     if (progress) {
                                                         progress(0.6 + 0.35 * fraction, "Warping");
                                                     }
                                                 });
        if (!warped) {
            return warped.error();
        }
        for (const std::string& warning : warped->warnings) {
            result.warnings.push_back(warning);
        }
        for (const katana::gis::WarpSource& source : plan->sources) {
            if (source.path.starts_with("/vsicurl/")) {
                ++result.stats.remoteSources;
            }
        }
        std::error_code error;
        fs::rename(partial, product, error);
        if (error) {
            fs::remove(partial, error);
            return makeError(ErrorCode::FileExportFailure, "could not keep the finished raster",
                             product.string());
        }
    } else {
        result.stats.productFromCache = true;
        ++result.stats.cacheHits;
    }
    if (progress) {
        progress(0.97, "Reading the result");
    }
    RasterImportOptions importOptions;
    importOptions.name = layer.providerTitle + " - " + layer.title;
    auto raster = importRaster(product, importOptions);
    if (!raster) {
        return raster.error();
    }
    result.file = product;
    result.sourceUrl = katana::gis::redactUrl(prepared->endpoint);
    result.licence = layer.licence;
    result.attribution = layer.attribution;
    result.retrieved = today();
    raster->sourceUrl = result.sourceUrl;
    raster->licence = result.licence;
    raster->attribution = result.attribution;
    result.raster = std::move(*raster);
    return result;
}

// ---- vectors ---------------------------------------------------------------------------

// Joins the pages of one import, dropping a feature a later page repeats.
// A feature is known by `idField` when the service has one (matched without
// regard to case), else by all its attributes together; within one page
// nothing is dropped, because a multi-part feature arrives as several
// entities with the same attributes on purpose.
class PageJoiner {
  public:
    explicit PageJoiner(std::string idField) : idField_(katana::core::lowered(idField)) {}

    // Adds a page; returns how many of its features were new.
    std::uint64_t add(VectorImportResult page, OnlineStats& stats)
    {
        std::set<std::string> thisPage;
        std::uint64_t added = 0;
        for (katana::entity::Entity& entity : page.entities) {
            const std::string key = keyOf(entity);
            if (seen_.contains(key)) {
                ++stats.duplicates;
                continue;
            }
            if (thisPage.insert(key).second) {
                ++added;
            }
            result_.bounds.expand(page.bounds);
            result_.entities.push_back(std::move(entity));
        }
        seen_.insert(thisPage.begin(), thisPage.end());
        for (const std::string& layer : page.layersNeeded) {
            if (std::find(result_.layersNeeded.begin(), result_.layersNeeded.end(), layer) ==
                result_.layersNeeded.end()) {
                result_.layersNeeded.push_back(layer);
            }
        }
        result_.featuresRead += page.featuresRead;
        result_.featuresSkipped += page.featuresSkipped;
        if (result_.projectionWkt.empty()) {
            result_.projectionWkt = page.projectionWkt;
        }
        for (std::string& warning : page.warnings) {
            if (std::find(result_.warnings.begin(), result_.warnings.end(), warning) ==
                result_.warnings.end()) {
                result_.warnings.push_back(std::move(warning));
            }
        }
        return added;
    }

    [[nodiscard]] std::size_t size() const { return result_.entities.size(); }
    VectorImportResult take() { return std::move(result_); }

  private:
    std::string keyOf(const katana::entity::Entity& entity) const
    {
        if (!idField_.empty()) {
            for (const auto& [name, value] : entity.properties) {
                if (katana::core::lowered(name) == idField_) {
                    return "id:" + katana::entity::toString(value);
                }
            }
        }
        std::string key;
        for (const auto& [name, value] : entity.properties) {
            key += name + "=" + katana::entity::toString(value) + "\x1f";
        }
        return key;
    }

    std::string idField_;
    std::set<std::string> seen_;
    VectorImportResult result_;
};

Result<VectorImportResult> importPage(const fs::path& path, const OnlineLayer& layer,
                                      const OnlineRequestOptions& options,
                                      const std::optional<CrsBox>& filter = std::nullopt)
{
    VectorImportOptions importOptions;
    importOptions.targetLayer = options.targetLayer.empty()
                                    ? "online/" + layer.providerId + "/" + layer.id
                                    : options.targetLayer;
    importOptions.layerAttribute.clear();
    importOptions.targetCrs = options.targetCrs;
    // GeoJSON without a crs member is WGS 84 longitude/latitude (RFC 7946).
    importOptions.assumedSourceCrs = "EPSG:4326";
    importOptions.sourceFilter = filter;
    auto imported = importVector(path, importOptions);
    // A page, or a file filtered to the area, that holds no feature at all
    // is an empty answer - the area has none of these - not a failure. One
    // whose features could not be read still is.
    if (!imported && imported.error().code == ErrorCode::InvalidArgument &&
        imported.error().context.starts_with("0 features read")) {
        return VectorImportResult{};
    }
    return imported;
}

Result<OnlineImport> fetchVectors(const OnlineLayer& layer, const OnlineRequestOptions& options,
                                  const OnlineEnvironment& environment, const std::stop_token& stop,
                                  const OnlineProgress& progress)
{
    auto prepared = prepare(layer, options, environment);
    if (!prepared) {
        return prepared.error();
    }
    OnlineImport result;
    result.kind = OnlineLayerKind::Vector;
    const OnlineLayer resolved = withEndpoint(layer, prepared->endpoint);
    PageJoiner joiner(layer.idField.empty() && layer.type == OnlineServiceType::ArcgisQuery
                          ? std::string("objectid")
                          : layer.idField);
    const auto tooMany = [&] {
        return makeError(ErrorCode::InvalidArgument,
                         "the area holds more than " + std::to_string(environment.maxFeatures) +
                             " features; choose a smaller area");
    };
    const auto report = [&](const std::string& stage) {
        if (progress) {
            progress(-1.0, stage);
        }
    };

    switch (layer.type) {
    case OnlineServiceType::ArcgisQuery: {
        std::uint64_t offset = 0;
        for (int page = 0;; ++page) {
            if (page >= environment.maxPages) {
                return tooMany();
            }
            report("Fetching page " + std::to_string(page + 1) + " (" + std::to_string(joiner.size()) +
                   " features)");
            const std::string url = arcgisQueryUrl(resolved, prepared->lonLat, offset, layer.pageSize);
            auto file = fetchToCache(makeRequest(url, environment), ".geojson", layer.cacheDays,
                                     environment, result.stats, stop);
            if (!file) {
                return file.error();
            }
            auto state = readQueryPage(readWhole(*file));
            if (!state) {
                std::error_code ignored;
                fs::remove(*file, ignored);
                return state.error();
            }
            ++result.stats.pages;
            if (state->features > 0) {
                auto imported = importPage(*file, layer, options);
                if (!imported) {
                    return imported.error();
                }
                if (joiner.add(std::move(*imported), result.stats) == 0 && page > 0) {
                    // Every feature was one already had: the server ignored
                    // resultOffset. Stopping is the only way this ends.
                    result.warnings.push_back(
                        "the service repeated a page instead of moving on; it may not support "
                        "paging, so some features may be missing - try a smaller area");
                    break;
                }
            }
            if (joiner.size() > environment.maxFeatures) {
                return tooMany();
            }
            if (!queryHasMore(*state, layer.pageSize)) {
                break;
            }
            offset += state->features;
        }
        break;
    }
    case OnlineServiceType::Wfs: {
        const std::string crs = layer.crs.empty() ? "EPSG:4326" : layer.crs;
        auto box = katana::gis::transformBox(prepared->lonLat, "EPSG:4326", crs);
        if (!box) {
            return box.error();
        }
        const bool v2 = layer.version.empty() || layer.version.starts_with("2.");
        auto swap = katana::gis::crsAxisIsYX(crs);
        if (!swap) {
            return swap.error();
        }
        std::uint64_t start = 0;
        for (int page = 0;; ++page) {
            if (page >= environment.maxPages) {
                return tooMany();
            }
            report("Fetching page " + std::to_string(page + 1) + " (" + std::to_string(joiner.size()) +
                   " features)");
            // 1.1.0 is always x,y in its BBOX for EPSG:4326 as GDAL and
            // servers read it; 2.0 follows the authority.
            const std::string url =
                wfsGetFeatureUrl(resolved, *box, crs, v2 && *swap, start, layer.pageSize);
            auto file = fetchToCache(makeRequest(url, environment), ".gml", layer.cacheDays,
                                     environment, result.stats, stop);
            if (!file) {
                return file.error();
            }
            const std::string head = headOf(*file);
            if (head.find("ExceptionReport") != std::string::npos ||
                head.find("ServiceException") != std::string::npos) {
                std::error_code ignored;
                fs::remove(*file, ignored);
                return makeError(ErrorCode::FileImportFailure, "the WFS refused the request",
                                 serviceMessage(head));
            }
            ++result.stats.pages;
            auto imported = importPage(*file, layer, options);
            if (!imported) {
                // A page with no feature is a valid, empty FeatureCollection
                // that GDAL may still decline to call a vector file.
                if (imported.error().code == ErrorCode::InvalidArgument) {
                    break;
                }
                return imported.error();
            }
            const std::uint64_t read = imported->featuresRead;
            joiner.add(std::move(*imported), result.stats);
            if (joiner.size() > environment.maxFeatures) {
                return tooMany();
            }
            if (!v2 || read < static_cast<std::uint64_t>(layer.pageSize) || read == 0) {
                break;
            }
            start += read;
        }
        break;
    }
    case OnlineServiceType::OgcApiFeatures: {
        std::string url = oapifItemsUrl(resolved, prepared->lonLat, layer.pageSize);
        for (int page = 0; !url.empty(); ++page) {
            if (page >= environment.maxPages) {
                return tooMany();
            }
            report("Fetching page " + std::to_string(page + 1) + " (" + std::to_string(joiner.size()) +
                   " features)");
            auto file = fetchToCache(makeRequest(url, environment), ".geojson", layer.cacheDays,
                                     environment, result.stats, stop);
            if (!file) {
                return file.error();
            }
            const std::string body = readWhole(*file);
            auto count = countGeoJsonFeatures(body);
            if (!count) {
                std::error_code ignored;
                fs::remove(*file, ignored);
                return count.error();
            }
            ++result.stats.pages;
            if (*count == 0) {
                break;
            }
            auto imported = importPage(*file, layer, options);
            if (!imported) {
                return imported.error();
            }
            if (joiner.add(std::move(*imported), result.stats) == 0 && page > 0) {
                break;
            }
            if (joiner.size() > environment.maxFeatures) {
                return tooMany();
            }
            const std::string next = nextLink(body);
            // Only links to the same service are followed: an answer cannot
            // send the importer somewhere else.
            const auto sameHost = [](const std::string& a, const std::string& b) {
                const auto host = [](const std::string& u) {
                    const std::size_t s = u.find("://");
                    return s == std::string::npos ? std::string() : u.substr(0, u.find('/', s + 3));
                };
                return host(a) == host(b);
            };
            url = !next.empty() && sameHost(next, prepared->endpoint) ? next : std::string();
        }
        break;
    }
    case OnlineServiceType::Overpass: {
        const double area = areaKm2(prepared->lonLat);
        if (area > layer.maxAreaKm2) {
            return makeError(ErrorCode::InvalidArgument,
                             "the area is " + formatNumber(std::round(area * 10.0) / 10.0) +
                                 " km2; OpenStreetMap queries through the public Overpass API are "
                                 "limited to " + formatNumber(layer.maxAreaKm2) +
                                 " km2 - choose a smaller area");
        }
        const std::string filter = !options.filter.empty() ? options.filter : layer.layerName;
        auto query = overpassQuery(filter, prepared->lonLat, layer.timeoutSeconds,
                                   std::min<std::uint64_t>(environment.maxResponseBytes, 512ull << 20));
        if (!query) {
            return query.error();
        }
        HttpRequest request = makeRequest(prepared->endpoint, environment);
        request.postBody = "data=" + katana::gis::percentEncode(*query);
        request.postContentType = "application/x-www-form-urlencoded";
        // The server may take its whole timeout; the transfer is given a
        // little longer so that its own answer, not ours, ends a slow query.
        request.timeoutSeconds = std::max(environment.timeoutSeconds, layer.timeoutSeconds + 30);
        report("Querying OpenStreetMap");
        auto file = fetchToCache(request, ".osm", layer.cacheDays, environment, result.stats, stop);
        if (!file) {
            return file.error();
        }
        const std::string head = headOf(*file);
        if (head.find("<osm") == std::string::npos) {
            std::error_code ignored;
            fs::remove(*file, ignored);
            return makeError(ErrorCode::FileImportFailure, "the Overpass API did not return OpenStreetMap data",
                             serviceMessage(head));
        }
        ++result.stats.pages;
        report("Reading OpenStreetMap data");
        auto imported = importPage(*file, layer, options);
        if (!imported) {
            return imported.error();
        }
        joiner.add(std::move(*imported), result.stats);
        if (joiner.size() > environment.maxFeatures) {
            return tooMany();
        }
        break;
    }
    case OnlineServiceType::File: {
        std::string url = prepared->endpoint;
        for (std::size_t at = url.find("{layer}"); at != std::string::npos; at = url.find("{layer}")) {
            url.replace(at, 7, layer.layerName);
        }
        const bool zipped = lower(url).ends_with(".zip");
        report("Downloading " + layer.title);
        auto file = fetchToCache(makeRequest(url, environment), zipped ? ".shp.zip" : extensionFor({}, url),
                                 layer.cacheDays, environment, result.stats, stop);
        if (!file) {
            return file.error();
        }
        ++result.stats.pages;
        report("Clipping " + layer.title + " to the area");
        // The file covers far more than the area: its features are cut at
        // the area's edge (in the file's own CRS, WGS 84 for every File layer
        // the catalogue has) before they are read.
        const fs::path clipped = cachePath(
            cacheRoot(environment), "products",
            katana::gis::sha256Hex("clip\n" + file->string() + "\n" + formatNumber(prepared->lonLat.minX) +
                                   "," + formatNumber(prepared->lonLat.minY) + "," +
                                   formatNumber(prepared->lonLat.maxX) + "," +
                                   formatNumber(prepared->lonLat.maxY)),
            ".gpkg");
        if (auto cut = katana::gis::clipVectorFile(*file, prepared->lonLat, clipped); !cut) {
            return cut.error();
        }
        report("Reading " + layer.title);
        auto imported = importPage(clipped, layer, options, prepared->lonLat);
        if (!imported) {
            return imported.error();
        }
        joiner.add(std::move(*imported), result.stats);
        break;
    }
    case OnlineServiceType::Ckan:
        return makeError(ErrorCode::Unsupported,
                         "a catalogue finds services rather than holding data: search it with "
                         "ONLINE LAYERS " + layer.providerId + " <words>, then add a result with "
                         "ONLINE CUSTOM <url>");
    default:
        return makeError(ErrorCode::Unsupported,
                         std::string("a ") + toString(layer.type) + " service gives no vectors");
    }

    VectorImportResult vectors = joiner.take();
    result.sourceUrl = katana::gis::redactUrl(prepared->endpoint);
    result.licence = layer.licence;
    result.attribution = layer.attribution;
    result.retrieved = today();
    for (katana::entity::Entity& entity : vectors.entities) {
        entity.metadata["online.source"] = result.sourceUrl;
        entity.metadata["online.licence"] = result.licence;
        entity.metadata["online.attribution"] = result.attribution;
        entity.metadata["online.retrieved"] = result.retrieved;
        // A page's cache file name says nothing to anyone: the service does.
        entity.metadata["source.file"] = layer.providerId + "/" + layer.id;
    }
    if (vectors.entities.empty() && vectors.layersNeeded.empty()) {
        vectors.layersNeeded.push_back(options.targetLayer.empty()
                                           ? "online/" + layer.providerId + "/" + layer.id
                                           : options.targetLayer);
    }
    result.vectors = std::move(vectors);
    return result;
}

} // namespace

std::string onlineUserAgent(std::string_view version)
{
    return "Katana/" + std::string(version) +
           " (survey and civil-engineering CAD; +https://github.com/kjarada/Katana)";
}

Result<std::string> endpointWithKey(const OnlineLayer& layer, const OnlineEnvironment& environment)
{
    const auto key = environment.keys.find(layer.keyName);
    const bool placeholder = layer.endpoint.find("{key}") != std::string::npos;
    if ((layer.keyRequired || placeholder) &&
        (key == environment.keys.end() || key->second.empty())) {
        return makeError(ErrorCode::NotFound,
                         layer.providerTitle + " needs a key; store yours with ONLINE KEY " +
                             layer.keyName + " <key>",
                         layer.providerId + "/" + layer.id);
    }
    std::string endpoint = layer.endpoint;
    if (placeholder) {
        const std::string encoded = katana::gis::percentEncode(key->second);
        for (std::size_t at = endpoint.find("{key}"); at != std::string::npos;
             at = endpoint.find("{key}", at + encoded.size())) {
            endpoint.replace(at, 5, encoded);
        }
    }
    return endpoint;
}

Result<fs::path> fetchToCache(const HttpRequest& request, const std::string& extension,
                              int maxAgeDays, const OnlineEnvironment& environment,
                              OnlineStats& stats, const std::stop_token& stop)
{
    const std::string key = cacheKey(request);
    const fs::path path = cachePath(cacheRoot(environment), "http", key, extension);
    const bool caching = !environment.cacheDirectory.empty();
    if (caching && fresh(path, maxAgeDays, environment.offline)) {
        ++stats.cacheHits;
        return path;
    }
    if (environment.offline) {
        return makeError(ErrorCode::NotFound, "working offline, and the cache has no copy of this",
                         katana::gis::redactUrl(request.url));
    }
    if (stop.stop_requested()) {
        return makeError(ErrorCode::InvalidState, "cancelled", katana::gis::redactUrl(request.url));
    }
    auto response = environment.transport ? environment.transport(request, stop)
                                          : katana::gis::httpFetch(request, stop);
    if (!response) {
        return response.error();
    }
    ++stats.requests;
    stats.bytes += response->body.size();
    // A stop that arrived while the answer came in: the answer is whole and
    // is kept for next time, but nothing more is done with it now.
    if (stop.stop_requested()) {
        (void)writeAtomically(path, response->body);
        return makeError(ErrorCode::InvalidState, "cancelled", katana::gis::redactUrl(request.url));
    }
    if (auto written = writeAtomically(path, response->body); !written) {
        return written.error();
    }
    return path;
}

Result<std::string> fetchText(const std::string& url, const OnlineEnvironment& environment,
                              OnlineStats& stats, const std::stop_token& stop, int maxAgeDays)
{
    auto file = fetchToCache(makeRequest(url, environment), ".txt", maxAgeDays, environment, stats, stop);
    if (!file) {
        return file.error();
    }
    return readWhole(*file);
}

std::uint64_t pruneCache(const fs::path& cacheDirectory, int maxAgeDays, std::uint64_t maxBytes)
{
    std::error_code error;
    if (cacheDirectory.empty() || !fs::is_directory(cacheDirectory, error)) {
        return 0;
    }
    struct Entry {
        fs::file_time_type written;
        std::uintmax_t size = 0;
        fs::path path;
    };
    std::vector<Entry> entries;
    std::uint64_t removed = 0;
    const auto now = fs::file_time_type::clock::now();
    for (const std::string folder : {"http", "products"}) {
        const fs::path root = cacheDirectory / folder;
        if (!fs::is_directory(root, error)) {
            continue;
        }
        for (auto it = fs::recursive_directory_iterator(root, error);
             it != fs::recursive_directory_iterator(); it.increment(error)) {
            if (error) {
                break;
            }
            if (!it->is_regular_file(error)) {
                continue;
            }
            const auto written = it->last_write_time(error);
            const auto size = it->file_size(error);
            if (now - written > std::chrono::hours(24) * maxAgeDays) {
                if (fs::remove(it->path(), error)) {
                    removed += size;
                }
                continue;
            }
            entries.push_back(Entry{written, size, it->path()});
        }
    }
    std::sort(entries.begin(), entries.end(),
              [](const Entry& a, const Entry& b) { return a.written < b.written; });
    std::uint64_t total = 0;
    for (const Entry& entry : entries) {
        total += entry.size;
    }
    for (const Entry& entry : entries) {
        if (total <= maxBytes) {
            break;
        }
        if (fs::remove(entry.path, error)) {
            removed += entry.size;
            total -= entry.size;
        }
    }
    return removed;
}

Result<OnlineImport> fetchOnlineLayer(const OnlineLayer& layer, const OnlineRequestOptions& options,
                                      const OnlineEnvironment& environment,
                                      const std::stop_token& stop, const OnlineProgress& progress)
{
    switch (layer.kind) {
    case OnlineLayerKind::Imagery:
    case OnlineLayerKind::Elevation:
        return fetchRaster(layer, options, environment, stop, progress);
    case OnlineLayerKind::Vector:
    case OnlineLayerKind::Catalogue:
        return fetchVectors(layer, options, environment, stop, progress);
    }
    return makeError(ErrorCode::Internal, "unknown layer kind");
}

Result<OnlineProvider> discoverOnline(const std::string& url, const OnlineEnvironment& environment,
                                      const std::stop_token& stop)
{
    OnlineStats stats;
    const TextFetcher fetch = [&](const std::string& address) {
        return fetchText(address, environment, stats, stop);
    };
    const auto probe = [&](const std::string& address) -> Result<OnlineProvider> {
        auto raster = katana::gis::probeRaster(vsicurl(address), environment.userAgent,
                                               environment.timeoutSeconds);
        if (!raster) {
            return raster.error();
        }
        OnlineProvider provider;
        provider.id = customProviderId(address);
        std::string name = address.substr(address.find_last_of('/') + 1);
        provider.title = name;
        provider.group = "Custom";
        provider.homepage = address.substr(0, address.find_last_of('/') + 1);
        provider.licence = "not stated by the service - check the publisher's terms";
        provider.attribution = provider.title;
        provider.userDefined = true;
        OnlineService service;
        service.id = "cog";
        service.title = name;
        service.type = OnlineServiceType::Cog;
        service.endpoint = address;
        OnlineLayer layer;
        layer.providerId = provider.id;
        layer.providerTitle = provider.title;
        layer.group = provider.group;
        layer.serviceId = service.id;
        layer.serviceTitle = service.title;
        layer.id = "cog";
        layer.title = name + " (" + std::to_string(raster->width) + " x " +
                      std::to_string(raster->height) + ", " + std::to_string(raster->bandCount) +
                      " band " + raster->dataType + ")";
        layer.type = OnlineServiceType::Cog;
        layer.endpoint = address;
        // One band that is not bytes is values: an elevation model.
        layer.kind = raster->bandCount == 1 && raster->dataType != "Byte" ? OnlineLayerKind::Elevation
                                                                         : OnlineLayerKind::Imagery;
        if (raster->lonLatBounds) {
            layer.coverage = {raster->lonLatBounds->minX, raster->lonLatBounds->minY,
                              raster->lonLatBounds->maxX, raster->lonLatBounds->maxY};
        }
        layer.licence = provider.licence;
        layer.attribution = provider.attribution;
        layer.keyName = provider.id;
        layer.userDefined = true;
        layer.evidence = "discovered from the COG's header";
        service.layers.push_back(std::move(layer));
        provider.services.push_back(std::move(service));
        return provider;
    };
    return discoverService(url, fetch, probe);
}

Result<std::vector<CkanResource>> searchCatalogue(const OnlineLayer& layer, const std::string& words,
                                                  const OnlineEnvironment& environment,
                                                  const std::stop_token& stop)
{
    OnlineStats stats;
    auto body = fetchText(ckanSearchUrl(layer.endpoint, words, 50), environment, stats, stop);
    if (!body) {
        return body.error();
    }
    return parseCkanSearch(*body);
}

} // namespace katana::interop
