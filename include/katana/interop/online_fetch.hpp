#pragma once

// Importing a layer from a web service: GIS > Online Data and ONLINE IMPORT
// (docs/gis_online.md).
//
// `fetchOnlineLayer` turns one catalogue layer and an area into what the
// existing import paths already know how to take in, and takes it in through
// them:
//
//   imagery, elevation  the service's images or tiles for the area are
//                       fetched (through the disk cache), warped and mosaicked
//                       by GDAL into ONE GeoTIFF in the project's CRS at the
//                       asked resolution, and that file goes through
//                       importRaster - a reference raster like any other. An
//                       elevation keeps its true Float32 values, so Surface
//                       From Raster builds ground from it.
//   vector              the features are fetched page by page, each page
//                       read by importVector with the page moved into the
//                       project's CRS (VectorImportOptions::targetCrs), and
//                       the pages joined with the features one page repeats
//                       from another dropped - Entity values for the caller
//                       to add as ONE command.
//
// Every result records where it came from: the raster's sourceUrl, licence and
// attribution; each entity's metadata "online.source", "online.licence",
// "online.attribution" and "online.retrieved". The URL recorded is the
// service's address with any key removed (gis::redactUrl) - a key is sent,
// never stored in the project, never logged.
//
// Nothing here touches a Document or a widget: the desktop application runs
// it in a background job and applies the result on the GUI thread
// (src/katana_qt/jobs.hpp). It blocks its thread; `stop` cancels it between
// and during requests, and a cancelled import leaves nothing behind but what
// the cache already held.

#include <cstdint>
#include <filesystem>
#include <functional>
#include <map>
#include <optional>
#include <stop_token>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/gis/web_access.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_requests.hpp"
#include "katana/interop/reference_data.hpp"

namespace katana::interop {

// "Katana/<version> (survey and civil-engineering CAD; +https://github.com/kjarada/Katana)":
// what every request says it is, as the OpenStreetMap tile and Overpass
// policies require - the program by name, and where to find who makes it.
[[nodiscard]] std::string onlineUserAgent(std::string_view version);

// How the fetcher may behave: where it caches, what it says it is, its keys
// and its limits. Each limit is refused with a message saying how to stay
// within it, never truncated silently.
struct OnlineEnvironment {
    // Where answers are kept: <cache>/http for what services returned,
    // <cache>/products for finished rasters. Empty disables the cache.
    std::filesystem::path cacheDirectory;
    std::string userAgent = onlineUserAgent("dev");
    // Keys by name (OnlineLayer::keyName), from the application's settings.
    std::map<std::string, std::string> keys;
    int timeoutSeconds = 120;
    int retries = 3;
    // One answer's size, and the pixels of one finished raster.
    std::uint64_t maxResponseBytes = 256ull * 1024 * 1024;
    std::uint64_t maxPixels = 64ull * 1024 * 1024;
    // Features in one import, and pages of them.
    std::uint64_t maxFeatures = 250'000;
    int maxPages = 500;
    // Answer from the cache only, whatever its age; fail where it has nothing.
    bool offline = false;
    // What sends a request. Empty: gis::httpFetch, the network. The tests
    // put a function here that answers from fixtures and counts what it was
    // asked, so paging, caching, cancellation and every limit are exercised
    // with no network at all.
    std::function<katana::core::Result<katana::gis::HttpResponse>(const katana::gis::HttpRequest&,
                                                                  const std::stop_token&)>
        transport;
};

struct OnlineRequestOptions {
    // The area, in `areaCrs` (traditional order).
    katana::gis::CrsBox area;
    std::string areaCrs;
    // The project's CRS: where everything is brought.
    std::string targetCrs;
    // Rasters: target units per pixel. Absent: the layer's own resolution
    // for elevation, and the area's longer side over 2048 pixels for imagery.
    std::optional<double> resolution;
    // Vectors: the Katana layer the entities go on. Empty:
    // online/<provider>/<layer>.
    std::string targetLayer;
    // STAC: the acquisition dates (YYYY-MM-DD) and the cloud ceiling, percent.
    // Empty dates: the layer's look-back from today.
    std::string fromDate;
    std::string toDate;
    std::optional<double> maxCloud;
    // Overpass: the tag test, when the layer has none of its own or it is
    // to be replaced ("building", "highway=primary").
    std::string filter;
    // WMS/WMTS time dimension; empty: the layer's own.
    std::string time;
};

struct OnlineStats {
    int requests = 0;  // made over the network
    int cacheHits = 0; // answered from the cache
    std::uint64_t bytes = 0;
    int pages = 0;
    std::uint64_t duplicates = 0; // features a later page repeated, dropped
    bool productFromCache = false; // the finished raster itself was cached
};

struct OnlineImport {
    OnlineLayerKind kind = OnlineLayerKind::Imagery;
    std::optional<RasterOverlay> raster;
    std::optional<VectorImportResult> vectors;
    std::filesystem::path file; // the finished GeoTIFF, for a raster
    double resolution = 0.0;    // for a raster: target units per pixel
    std::string sourceUrl;      // redacted
    std::string licence;
    std::string attribution;
    std::string retrieved; // YYYY-MM-DD, UTC
    OnlineStats stats;
    std::vector<std::string> warnings;
};

// Progress in [0, 1] (negative: busy with no measure) and a stage.
using OnlineProgress = std::function<void(double fraction, const std::string& stage)>;

[[nodiscard]] katana::core::Result<OnlineImport>
fetchOnlineLayer(const OnlineLayer& layer, const OnlineRequestOptions& options,
                 const OnlineEnvironment& environment, const std::stop_token& stop = {},
                 const OnlineProgress& progress = {});

// The address a layer is fetched from with its {key} filled in. NotFound,
// naming the key and the verb that stores it, when the layer needs a key the
// environment does not have.
[[nodiscard]] katana::core::Result<std::string> endpointWithKey(const OnlineLayer& layer,
                                                                const OnlineEnvironment& environment);

// ---- the cache ----------------------------------------------------------------------

// A request's answer, from the cache when it holds a fresh one, else from
// the network, kept in the cache for next time. The cache file is named by
// the SHA-256 of the method, URL and body, so no two requests share a file
// and no key appears in a file name; it is written to a temporary name and
// renamed, so a cancelled or failed transfer never leaves a partial answer
// that a later run would take for a whole one. Returns the file's path.
[[nodiscard]] katana::core::Result<std::filesystem::path>
fetchToCache(const katana::gis::HttpRequest& request, const std::string& extension, int maxAgeDays,
             const OnlineEnvironment& environment, OnlineStats& stats,
             const std::stop_token& stop = {});

// The same, for text (a capabilities document, a JSON answer).
[[nodiscard]] katana::core::Result<std::string>
fetchText(const std::string& url, const OnlineEnvironment& environment, OnlineStats& stats,
          const std::stop_token& stop = {}, int maxAgeDays = 1);

// Removes cache files older than `maxAgeDays`, and then the oldest until the
// cache holds at most `maxBytes`. Returns the bytes removed.
std::uint64_t pruneCache(const std::filesystem::path& cacheDirectory, int maxAgeDays,
                         std::uint64_t maxBytes);

// ---- discovery and search -------------------------------------------------------------

// discoverService over the network with the environment's User-Agent,
// timeouts and cache, and a COG's header read through GDAL.
[[nodiscard]] katana::core::Result<OnlineProvider>
discoverOnline(const std::string& url, const OnlineEnvironment& environment,
               const std::stop_token& stop = {});

// A CKAN catalogue search (the data-gov-au provider's layer) for `words`.
[[nodiscard]] katana::core::Result<std::vector<CkanResource>>
searchCatalogue(const OnlineLayer& layer, const std::string& words,
                const OnlineEnvironment& environment, const std::stop_token& stop = {});

} // namespace katana::interop
