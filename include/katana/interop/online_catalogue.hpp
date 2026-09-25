#pragma once

// The catalogue of public web services that GIS > Online Data and the ONLINE
// verbs offer (docs/gis_online.md).
//
// The catalogue is DATA, not code: resources/online/online_sources.json,
// compiled into the program with #embed (as the plot frame is), lists
// providers, their services and each service's layers, with everything a
// request and a record need - the service type, the endpoint, the layer id,
// the native CRS, the WGS 84 coverage, the request and page limits, the
// licence and the attribution to record, whether a key is needed. A provider
// changing an address is an edit to that file, and a person can add a service
// the program does not know - or replace a built-in provider - with a
// catalogue of their own beside the settings, which is merged over the
// built-in one by provider id.
//
// Inheritance keeps the file short and the parsed form flat: a provider sets
// the licence, attribution and coverage its services share, a service the CRS,
// type, endpoint and limits its layers share, and a layer overrides what it
// needs. `parseCatalogue` resolves all of it, so every OnlineLayer carries its
// complete description and nothing downstream walks the tree to find one.

#include <array>
#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::interop {

// How a service is asked for data. Each has its own request builder
// (online_requests.hpp) and path through the fetcher (online_fetch.hpp).
enum class OnlineServiceType {
    ArcgisExport,      // ArcGIS REST MapServer /export - a rendered image of a box
    ArcgisExportImage, // ArcGIS REST ImageServer /exportImage - pixel values (elevation)
    ArcgisQuery,       // ArcGIS REST FeatureServer or MapServer /<layer>/query - features, paged
    Wms,               // OGC WMS GetMap
    Wmts,              // OGC WMTS tiles, by a REST template or capabilities
    Wcs,               // OGC WCS GetCoverage - pixel values
    Wfs,               // OGC WFS GetFeature - features, paged
    OgcApiFeatures,    // OGC API - Features /collections/{id}/items - features, paged by links
    XyzTiles,          // {z}/{x}/{y} tiles on the Web Mercator grid
    Stac,              // a STAC API search, then the matching items' COG assets
    Cog,               // a cloud-optimised GeoTIFF (or a tiled set of them) read over HTTP
    Overpass,          // OpenStreetMap features by tag, through the Overpass API
    File,              // one downloadable vector file (a zipped shapefile, a GeoJSON)
    Ckan,              // a CKAN catalogue search that finds services, not data
};

// What a layer gives the drawing. Imagery and elevation become reference
// rasters (elevation keeps its true values, so Surface From Raster can use
// it); vectors become entities; a catalogue layer lists services.
enum class OnlineLayerKind { Imagery, Elevation, Vector, Catalogue };

[[nodiscard]] const char* toString(OnlineServiceType type);
[[nodiscard]] const char* toString(OnlineLayerKind kind);
[[nodiscard]] std::optional<OnlineServiceType> serviceTypeFromString(std::string_view text);
[[nodiscard]] std::optional<OnlineLayerKind> layerKindFromString(std::string_view text);

// A WGS 84 longitude/latitude box: west, south, east, north.
using LonLatBox = std::array<double, 4>;

// One layer with its service and provider resolved into it.
struct OnlineLayer {
    std::string providerId;
    std::string providerTitle;
    std::string group; // "Australia/NSW", "Global": the tree the dialog shows
    std::string serviceId;
    std::string serviceTitle;
    std::string id;    // unique within its provider; what the ONLINE verbs name
    std::string title;
    OnlineLayerKind kind = OnlineLayerKind::Imagery;
    OnlineServiceType type = OnlineServiceType::XyzTiles;
    // The service's address. For XYZ, WMTS and COG services a template with
    // {z} {x} {y} {layer} {time} {lat} {lon}; for the others the base URL
    // the request builder appends to.
    std::string endpoint;
    std::string version;   // WMS 1.3.0, WFS 2.0.0, WCS 1.0.0
    std::string layerName; // the service's own id: ArcGIS layer, WMS layer, WFS type, collection, tag
    std::string crs;       // the CRS requests are made in; empty: the service's own (EPSG:4326 for queries)
    LonLatBox coverage{-180.0, -90.0, 180.0, 90.0};
    // The largest image one request may ask for, each side, in pixels; a
    // larger area is split into several requests.
    int maxRequestPixels = 2048;
    // Features asked for per request; the paging step.
    int pageSize = 1000;
    // Tiles: the finest zoom the service has, and the most tiles one import
    // may fetch (the OpenStreetMap tile policy's "small areas only").
    int maxZoom = 19;
    int maxTiles = 256;
    // The ground resolution of the data, metres, when it has one; the
    // default for an elevation import.
    std::optional<double> resolution;
    // Days a cached answer stays fresh (the OSM tile policy asks for at least 7).
    int cacheDays = 30;
    // Overpass: the largest area, square kilometres, and the server-side timeout.
    double maxAreaKm2 = 25.0;
    int timeoutSeconds = 90;
    // STAC: the default cloud ceiling (percent) and look-back (days); the asset(s).
    double maxCloud = 20.0;
    int days = 90;
    std::string asset;
    std::string idField; // the attribute that identifies a feature across pages
    std::string time;    // WMTS/WMS time dimension; "default" for the latest
    std::string tiling;  // "degree" for the one-degree COG tiles of Copernicus
    std::string licence;
    std::string attribution;
    std::string homepage;
    bool keyRequired = false;
    // The name the key is stored under (ONLINE KEY <name> <value>); the
    // provider id unless the catalogue says otherwise.
    std::string keyName;
    std::string verified; // the date it answered Katana's own requests; empty: documented only
    std::string evidence; // where the address and the terms were checked
    bool userDefined = false;
};

struct OnlineService {
    std::string id;
    std::string title;
    OnlineServiceType type = OnlineServiceType::XyzTiles;
    std::string endpoint;
    std::vector<OnlineLayer> layers;
};

struct OnlineProvider {
    std::string id;
    std::string title;
    std::string group;
    std::string homepage;
    std::string licence;
    std::string attribution;
    bool userDefined = false;
    std::vector<OnlineService> services;

    [[nodiscard]] std::vector<const OnlineLayer*> layers() const;
    [[nodiscard]] const OnlineLayer* findLayer(std::string_view id) const;
};

struct OnlineCatalogue {
    std::vector<OnlineProvider> providers;

    [[nodiscard]] const OnlineProvider* findProvider(std::string_view id) const;
    // Providers whose id, title or group, or one of whose layers' ids or
    // titles, contains every word of `filter` (case-insensitive). An empty
    // filter matches everything. In catalogue order.
    [[nodiscard]] std::vector<const OnlineProvider*> filter(std::string_view text) const;
};

// Parses a catalogue document. `userDefined` marks every provider in it as
// the person's own (the dialog says so, and ONLINE CUSTOM writes there).
// ParseFailure naming the provider, service and field for a document that is
// not JSON or lacks something a request needs; the validation below is the
// fuller check.
[[nodiscard]] katana::core::Result<OnlineCatalogue> parseCatalogue(std::string_view json,
                                                                    bool userDefined = false);

// The catalogue compiled into the program.
[[nodiscard]] std::string_view builtInCatalogueJson();
[[nodiscard]] katana::core::Result<OnlineCatalogue> builtInCatalogue();

// `overlay` merged over `base`: a provider with an id `base` has replaces
// it in place, a new one is appended.
[[nodiscard]] OnlineCatalogue mergeCatalogues(OnlineCatalogue base, const OnlineCatalogue& overlay);

// Everything wrong with a catalogue, one sentence per problem, each naming
// provider/layer: a missing title, licence or attribution, an endpoint that
// is not an https or http URL (or a template missing a placeholder its type
// needs), a coverage box that is not a valid longitude/latitude box, a
// duplicate id, a limit out of range. Empty when it is sound.
[[nodiscard]] std::vector<std::string> validateCatalogue(const OnlineCatalogue& catalogue);

// One provider written as catalogue JSON, as ONLINE CUSTOM saves a discovered
// service to the user catalogue; `parseCatalogue` reads it back to the same
// layers.
[[nodiscard]] std::string providerToJson(const OnlineProvider& provider);
[[nodiscard]] std::string catalogueToJson(const OnlineCatalogue& catalogue);

} // namespace katana::interop
