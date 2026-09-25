#pragma once

// Requests for every kind of web service the online catalogue lists, built as
// plain strings by pure functions (docs/gis_online.md, "How each service is
// asked").
//
// Nothing here touches the network, the disk or a clock, so every URL, query
// and paging decision is tested byte for byte against what the service's
// specification says it should be (tests/interop/test_online_requests.cpp);
// online_fetch.hpp sends what these build.
//
// COORDINATES. Every box handed to a builder is in traditional GIS order - x
// easting or longitude, y northing or latitude (gis/reproject.hpp) - in the
// CRS the builder names. A builder whose protocol follows the CRS authority's
// axis order (WMS 1.3.0, WFS 2.0) takes `axisYX` from gis::crsAxisIsYX and
// swaps the pair itself; nothing else in the program handles a swapped pair.

#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/interop/online_catalogue.hpp"

namespace katana::interop {

using katana::gis::CrsBox;

// ---- sizing an image request ------------------------------------------------

// One image request: a box and the pixels asked for it. The box is the
// requested area grown to a whole number of pixels, so the image's pixels
// are square in the request CRS and the service has no reason to stretch or
// re-fit it - ArcGIS's export, for one, silently widens a box whose aspect
// does not match the image's.
struct ImageRequest {
    CrsBox box;
    int width = 0;
    int height = 0;
};

// Splits `box` into a grid of requests no larger than `maxPixels` on either
// side at `unitsPerPixel`, row by row from the north-west. InvalidArgument
// for an empty box, a non-positive resolution, or a grid over `maxRequests`
// requests (the message gives the resolution that would fit).
[[nodiscard]] katana::core::Result<std::vector<ImageRequest>>
planImageRequests(const CrsBox& box, double unitsPerPixel, int maxPixels, int maxRequests = 64);

// ---- ArcGIS REST ---------------------------------------------------------------

// MapServer /export of `request` in EPSG `epsg`: a PNG with transparency.
// `layers` limits the drawn layers ("show:3") when the catalogue names one.
[[nodiscard]] std::string arcgisExportUrl(const OnlineLayer& layer, const ImageRequest& request,
                                          int epsg);
// ImageServer /exportImage: Float32 GeoTIFF values, bilinear, with the
// no-data value warpToGeoTiff uses.
[[nodiscard]] std::string arcgisExportImageUrl(const OnlineLayer& layer,
                                               const ImageRequest& request, int epsg);
// FeatureServer/MapServer /<layer>/query: every field, geometry in WGS 84
// (outSR=4326, f=geojson), the envelope `lonLat` intersected, one page of
// `count` from `offset` (resultOffset/resultRecordCount), ordered by
// `orderBy` when given so pages cannot overlap or skip.
[[nodiscard]] std::string arcgisQueryUrl(const OnlineLayer& layer, const CrsBox& lonLat,
                                         std::uint64_t offset, int count,
                                         const std::string& orderBy = {});

// What one page of an ArcGIS query answer says about paging: how many
// features it held and whether the server says there are more
// (exceededTransferLimit, at the top level of Esri JSON or in "properties" of
// its GeoJSON). An ArcGIS error document ({"error": {...}}) is
// FileImportFailure with the server's message.
struct QueryPage {
    std::uint64_t features = 0;
    bool exceededTransferLimit = false;
};
[[nodiscard]] katana::core::Result<QueryPage> readQueryPage(std::string_view json);

// Whether another page must be asked for: the server said more exist, or the
// page came back full (a server that ignores the flag).
[[nodiscard]] bool queryHasMore(const QueryPage& page, int pageSize);

// ---- OGC ----------------------------------------------------------------------------

// WMS GetMap. 1.3.0 names the CRS with CRS= and writes BBOX in the CRS's own
// axis order (`axisYX` for EPSG:4326: lat,lon); 1.1.1 names it SRS= and is
// always x,y.
[[nodiscard]] std::string wmsGetMapUrl(const OnlineLayer& layer, const ImageRequest& request,
                                       const std::string& crs, bool axisYX,
                                       const std::string& format = "image/png");
// WCS 1.0.0 GetCoverage as a GeoTIFF: BBOX is always x,y in 1.0.0.
[[nodiscard]] std::string wcsGetCoverageUrl(const OnlineLayer& layer, const ImageRequest& request,
                                            const std::string& crs);
// WFS 2.0.0 GetFeature, one page: TYPENAMES, COUNT, STARTINDEX, and BBOX in
// `crs`'s axis order with the CRS appended (the 2.0 form); SRSNAME asks for
// the answer in the same CRS. 1.1.0 uses TYPENAME and MAXFEATURES.
[[nodiscard]] std::string wfsGetFeatureUrl(const OnlineLayer& layer, const CrsBox& box,
                                           const std::string& crs, bool axisYX,
                                           std::uint64_t startIndex, int count);
// OGC API - Features items: bbox in CRS84 longitude/latitude (the default the
// standard fixes), `limit` per page. Later pages follow the answer's own
// "next" link (nextLink).
[[nodiscard]] std::string oapifItemsUrl(const OnlineLayer& layer, const CrsBox& lonLat, int limit);
// The href of the link with rel "next" in an OGC API or STAC answer; empty
// when there is none (the last page).
[[nodiscard]] std::string nextLink(std::string_view json);
// Features in a GeoJSON FeatureCollection answer (its "features" array).
[[nodiscard]] katana::core::Result<std::uint64_t> countGeoJsonFeatures(std::string_view json);

// ---- tiles ----------------------------------------------------------------------------

// One tile matrix of a tiled service (OGC WMTS 1.0, section 6.1): the scale,
// the top-left corner in the matrix set's CRS (traditional order), the tile
// size in pixels and the matrix size in tiles.
struct TileMatrix {
    std::string id;
    double scaleDenominator = 0.0;
    double topLeftX = 0.0;
    double topLeftY = 0.0;
    int tileWidth = 256;
    int tileHeight = 256;
    std::int64_t matrixWidth = 1;
    std::int64_t matrixHeight = 1;

    // Ground units per pixel: the scale denominator times the 0.28 mm
    // "standardized rendering pixel" of WMTS 1.0 (6.1), over the CRS's
    // metres per unit (1 for metres, 2 pi 6378137 / 360 for degrees).
    [[nodiscard]] double unitsPerPixel(double metresPerUnit) const;
};

struct TileMatrixSet {
    std::string id;
    std::string crs;
    double metresPerUnit = 1.0;
    std::vector<TileMatrix> matrices; // coarsest first
};

// The Web Mercator grid every XYZ service uses - GoogleMapsCompatible in the
// WMTS well-known scale sets (WMTS 1.0, annex E.4) - with levels 0 to
// `maxZoom`: level z is 2^z tiles square and 559082264.0287178 / 2^z the
// scale denominator.
[[nodiscard]] TileMatrixSet webMercatorTileMatrixSet(int maxZoom);

struct TileIndex {
    int matrix = 0; // index into TileMatrixSet::matrices; the zoom for XYZ
    std::int64_t column = 0;
    std::int64_t row = 0;
};

// The coarsest matrix at least as fine as `unitsPerPixel`, or the finest
// there is.
[[nodiscard]] int chooseTileMatrix(const TileMatrixSet& set, double unitsPerPixel);
// Tiles of matrix `level` meeting `box` (in the set's CRS), clamped to the
// matrix, row by row from the north-west.
[[nodiscard]] std::vector<TileIndex> tilesCovering(const TileMatrixSet& set, int level,
                                                   const CrsBox& box);
// The box a tile covers, in the set's CRS.
[[nodiscard]] CrsBox tileBox(const TileMatrixSet& set, const TileIndex& tile);

// A tile URL from a template: {z} {x} {y} (and {TileMatrix} {TileCol}
// {TileRow}) for the tile, {layer} {time} and anything in `extra` by name.
[[nodiscard]] std::string expandTileTemplate(std::string_view pattern, const TileIndex& tile,
                                             const std::string& matrixId,
                                             const std::map<std::string, std::string>& extra = {});

// ---- Copernicus DEM one-degree tiles ------------------------------------------------

// The one-degree tiles meeting `lonLat`, as {lat} and {lon} strings in the
// naming the Copernicus DEM uses ("S34", "E151": the tile's south-west
// corner), filled into `pattern`.
[[nodiscard]] std::vector<std::string> degreeTileUrls(std::string_view pattern, const CrsBox& lonLat);

// ---- STAC ---------------------------------------------------------------------------------

// A STAC API item search body (STAC API - Item Search 1.0): the collection,
// the intersected bbox, the datetime interval, and a cloud ceiling through
// the query extension on eo:cloud_cover, sorted by cloud cover ascending.
[[nodiscard]] std::string stacSearchBody(const std::string& collection, const CrsBox& lonLat,
                                         const std::string& fromDate, const std::string& toDate,
                                         std::optional<double> maxCloud, int limit);

struct StacItem {
    std::string id;
    std::string datetime; // as the item gives it
    std::optional<double> cloudCover;
    CrsBox bounds; // longitude/latitude
    std::map<std::string, std::string> assets; // key -> href
};

// The items of a search answer (a FeatureCollection) or of a single item.
// ParseFailure for anything else.
[[nodiscard]] katana::core::Result<std::vector<StacItem>> parseStacItems(std::string_view json);

// The items to mosaic for `lonLat`: those of the one acquisition day, among
// the days whose items together cover the area, with the least cloud (the
// latest day on a tie). A day that covers only part of the area is used only
// when no day covers all of it, with a warning. Empty when nothing meets the
// area at all.
struct StacChoice {
    std::vector<StacItem> items;
    std::string day;
    double cloudCover = 0.0;
    bool coversArea = false;
};
[[nodiscard]] StacChoice chooseStacItems(const std::vector<StacItem>& items, const CrsBox& lonLat);

// s3://bucket/key hrefs, which some catalogues give, as the bucket's https
// address; anything else unchanged.
[[nodiscard]] std::string httpHref(const std::string& href);

// ---- Overpass -----------------------------------------------------------------------------

// Overpass QL for every node, way and relation matching `filter` in `lonLat`,
// with its referenced nodes, as OSM XML - what GDAL's OSM driver reads. The
// filter is a tag test: `key`, `key=value`, `key!=value` or several joined by
// ';' ("building", "highway=primary", "amenity=school;name"). Anything else
// - quotes, brackets, braces, a second statement - is InvalidArgument, so no
// text a person types can become more than the one query. [timeout:] and
// [maxsize:] bound the server's work, as the Overpass commons policy asks.
[[nodiscard]] katana::core::Result<std::string>
overpassQuery(std::string_view filter, const CrsBox& lonLat, int timeoutSeconds,
              std::uint64_t maxBytes);

// The area of a longitude/latitude box in square kilometres (on the sphere of
// radius 6371 km), for the Overpass and tile area limits.
[[nodiscard]] double areaKm2(const CrsBox& lonLat);

// ---- CKAN -----------------------------------------------------------------------------------

[[nodiscard]] std::string ckanSearchUrl(const std::string& endpoint, const std::string& words,
                                        int rows);

struct CkanResource {
    std::string dataset;
    std::string title;
    std::string format; // WMS, WFS, ESRI REST, ...
    std::string url;
    std::string licence;
};
// The web-service resources of a package_search answer: WMS, WMTS, WFS, WCS,
// OGC API and ArcGIS REST, which ONLINE CUSTOM can open.
[[nodiscard]] katana::core::Result<std::vector<CkanResource>> parseCkanSearch(std::string_view json);

// ---- small helpers ----------------------------------------------------------------------

// `base` with `query` appended after '?' or '&' as the base needs.
[[nodiscard]] std::string withQuery(const std::string& base, const std::string& query);
// %.17g, the shortest text that reads back as the same double - the form
// every coordinate in a URL is written in, so a box survives the round trip.
[[nodiscard]] std::string formatNumber(double value);

} // namespace katana::interop
