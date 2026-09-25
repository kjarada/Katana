// Importing from web services (docs/gis_online.md): discovery from committed
// capabilities fixtures, and fetchOnlineLayer end to end - paging, joining,
// the cache, cancellation, every limit, keys, and rasters warped into the
// project's CRS - with NO network. Requests go to a transport the test
// supplies (OnlineEnvironment::transport), which answers from fixtures and
// counts what it was asked; a local file goes through the real fetch path
// as a file:// URL.
//
// The tests that DO reach the network are at the end, named Live*, and skip
// unless KATANA_ONLINE_TESTS=1: a test run on a train must not fail because
// a state government's server is down.

#include <gtest/gtest.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iterator>
#include <map>
#include <string>
#include <vector>

#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/gis/web_access.hpp"
#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_discovery.hpp"
#include "katana/interop/online_fetch.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::gis::CrsBox;
namespace fs = std::filesystem;
namespace gis = katana::gis;

namespace {

const std::string kData = KATANA_ONLINE_TEST_DATA;

std::string readFile(const fs::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}

std::string fixture(const std::string& name)
{
    return readFile(fs::path(kData) / name);
}

std::string fileUrl(const fs::path& path)
{
    std::string text = path.generic_string();
    return text.starts_with("/") ? "file://" + text : "file:///" + text;
}

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(fs::temp_directory_path() / ("katana-online-" + name))
    {
        std::error_code error;
        fs::remove_all(path_, error);
        fs::create_directories(path_, error);
    }
    ~TempDir()
    {
        std::error_code error;
        fs::remove_all(path_, error);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;
    [[nodiscard]] const fs::path& path() const { return path_; }

  private:
    fs::path path_;
};

// Answers requests from a table of (a piece the URL or body must contain ->
// the body to return), first match wins; counts every request and keeps the
// URLs in order. `onRequest`, when set, runs first - a test uses it to press
// Cancel part way through.
struct FakeTransport {
    std::vector<std::pair<std::string, std::string>> answers;
    std::vector<std::string> urls;
    std::vector<std::string> bodies;
    std::function<void(const gis::HttpRequest&)> onRequest;

    Result<gis::HttpResponse> operator()(const gis::HttpRequest& request, const std::stop_token&)
    {
        if (onRequest) {
            onRequest(request);
        }
        urls.push_back(request.url);
        bodies.push_back(request.postBody);
        for (const auto& [piece, body] : answers) {
            if (request.url.find(piece) != std::string::npos ||
                (!request.postBody.empty() && request.postBody.find(piece) != std::string::npos)) {
                gis::HttpResponse response;
                response.status = 200;
                response.body = body;
                return response;
            }
        }
        return katana::core::makeError(ErrorCode::NotFound, "the service has nothing at this address",
                                       request.url);
    }
};

OnlineEnvironment environmentWith(FakeTransport& transport, const fs::path& cache)
{
    OnlineEnvironment environment;
    environment.cacheDirectory = cache;
    environment.userAgent = onlineUserAgent("test");
    environment.transport = [&transport](const gis::HttpRequest& request, const std::stop_token& stop) {
        return transport(request, stop);
    };
    return environment;
}

OnlineLayer builtInLayer(const char* provider, const char* layer)
{
    auto catalogue = builtInCatalogue();
    EXPECT_TRUE(catalogue.ok());
    const OnlineProvider* found = catalogue->findProvider(provider);
    EXPECT_NE(found, nullptr) << provider;
    const OnlineLayer* entry = found != nullptr ? found->findLayer(layer) : nullptr;
    EXPECT_NE(entry, nullptr) << layer;
    return entry != nullptr ? *entry : OnlineLayer{};
}

// The Sydney CBD area of the live checks, in WGS 84.
OnlineRequestOptions sydneyOptions(std::string targetCrs = "EPSG:7856")
{
    OnlineRequestOptions options;
    options.area = CrsBox{151.2, -33.875, 151.215, -33.865};
    options.areaCrs = "EPSG:4326";
    options.targetCrs = std::move(targetCrs);
    return options;
}

// A single-band GeoTIFF whose value at (column, row) is 1000 row + column,
// so any sample says where it was read from.
fs::path writeGrid(const fs::path& path, const std::string& crs, double originX, double originY,
                   double pixel, int columns, int rows)
{
    gis::RasterExportOptions options;
    options.width = columns;
    options.height = rows;
    options.projectionWkt = *gis::crsToWkt(crs);
    options.geotransform = {originX, pixel, 0.0, originY, 0.0, -pixel};
    options.noDataValue = -9999.0;
    std::vector<double> values(static_cast<std::size_t>(columns) * rows);
    for (int row = 0; row < rows; ++row) {
        for (int column = 0; column < columns; ++column) {
            values[static_cast<std::size_t>(row) * columns + column] = 1000.0 * row + column;
        }
    }
    const auto written = gis::GdalDataset::writeRaster(path, options, values);
    EXPECT_TRUE(written.ok()) << (written ? "" : written.error().describe());
    return path;
}

bool mentions(const std::vector<std::string>& lines, const std::string& text)
{
    for (const std::string& line : lines) {
        if (line.find(text) != std::string::npos) {
            return true;
        }
    }
    return false;
}

} // namespace

// ---- discovery from fixtures --------------------------------------------------------------

TEST(OnlineDiscovery, TheUrlSaysWhatKindOfServiceItIs)
{
    EXPECT_EQ(guessServiceType("https://a/arcgis/rest/services/X/MapServer"), OnlineServiceType::ArcgisExport);
    EXPECT_EQ(guessServiceType("https://a/arcgis/rest/services/X/MapServer/9"), OnlineServiceType::ArcgisExport);
    EXPECT_EQ(guessServiceType("https://a/arcgis/rest/services/X/ImageServer"), OnlineServiceType::ArcgisExportImage);
    EXPECT_EQ(guessServiceType("https://a/arcgis/rest/services/X/FeatureServer/0"), OnlineServiceType::ArcgisQuery);
    EXPECT_EQ(guessServiceType("https://a/arcgis/services/X/MapServer/WMSServer?request=GetCapabilities"),
              OnlineServiceType::Wms);
    EXPECT_EQ(guessServiceType("https://a/ows?service=WFS"), OnlineServiceType::Wfs);
    EXPECT_EQ(guessServiceType("https://a/wmts/1.0.0/WMTSCapabilities.xml"), OnlineServiceType::Wmts);
    EXPECT_EQ(guessServiceType("https://a/dem.tif"), OnlineServiceType::Cog);
    EXPECT_EQ(guessServiceType("https://a/stac/v1"), OnlineServiceType::Stac);
    EXPECT_EQ(guessServiceType("https://a/item.json"), OnlineServiceType::Stac);
    EXPECT_FALSE(guessServiceType("https://a/geoserver/ows").has_value());
    EXPECT_EQ(descriptionUrl(OnlineServiceType::Wms, "https://a/wms?x=1"),
              "https://a/wms?SERVICE=WMS&REQUEST=GetCapabilities");
    EXPECT_EQ(descriptionUrl(OnlineServiceType::ArcgisQuery, "https://a/FeatureServer/"),
              "https://a/FeatureServer/?f=json");
    EXPECT_EQ(customProviderId("https://opendata.maps.vic.gov.au/geoserver/ows"),
              "custom-opendata-maps-vic-gov-au");
}

TEST(OnlineDiscovery, Wms130CapabilitiesGiveNamedLayersWithInheritedExtent)
{
    auto provider = parseWmsCapabilities(fixture("wms_capabilities_130.xml"), "https://ows.dea.ga.gov.au/?service=WMS");
    ASSERT_TRUE(provider.ok()) << provider.error().describe();
    EXPECT_EQ(provider->title, "Digital Earth Australia - OGC Web Services");
    EXPECT_EQ(provider->licence, "CC BY 4.0");
    const auto layers = provider->layers();
    ASSERT_EQ(layers.size(), 2u);
    EXPECT_EQ(layers[0]->layerName, "ga_ls8cls9c_gm_cyear_3");
    EXPECT_EQ(layers[0]->title, "DEA GeoMAD (Landsat 8 & 9)");
    EXPECT_EQ(layers[0]->endpoint, "https://ows.dea.ga.gov.au/wms");
    EXPECT_EQ(layers[0]->version, "1.3.0");
    EXPECT_EQ(layers[0]->crs, "EPSG:3857"); // preferred over 4326
    EXPECT_EQ(layers[0]->coverage, (LonLatBox{112, -44, 154, -9})); // from its parent
    EXPECT_EQ(layers[1]->coverage, (LonLatBox{113, -43.5, 153.5, -10}));
    EXPECT_TRUE(layers[0]->userDefined);
    OnlineCatalogue catalogue;
    catalogue.providers.push_back(*provider);
    EXPECT_TRUE(validateCatalogue(catalogue).empty());
}

TEST(OnlineDiscovery, Wms111ListsSrsAndLatLonBox)
{
    auto provider = parseWmsCapabilities(fixture("wms_capabilities_111.xml"), "https://old.example.org/wms");
    ASSERT_TRUE(provider.ok()) << provider.error().describe();
    const OnlineLayer* layer = provider->layers().front();
    EXPECT_EQ(layer->version, "1.1.1");
    EXPECT_EQ(layer->crs, "EPSG:4326");
    EXPECT_EQ(layer->coverage, (LonLatBox{140, -38, 154, -28}));
    // "none" is not a licence.
    EXPECT_NE(provider->licence.find("check the publisher"), std::string::npos);
}

TEST(OnlineDiscovery, WmtsLayersOnTheWebMercatorGridBecomeTileTemplates)
{
    auto provider = parseWmtsCapabilities(fixture("wmts_capabilities.xml"),
                                          "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/1.0.0/WMTSCapabilities.xml");
    ASSERT_TRUE(provider.ok()) << provider.error().describe();
    const auto layers = provider->layers();
    ASSERT_EQ(layers.size(), 1u); // the national-grid-only layer is left out
    const OnlineLayer* layer = layers.front();
    EXPECT_EQ(layer->layerName, "MODIS_Terra_CorrectedReflectance_TrueColor");
    EXPECT_EQ(layer->maxZoom, 9);
    EXPECT_EQ(layer->crs, "EPSG:3857");
    EXPECT_EQ(layer->endpoint,
              "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/MODIS_Terra_CorrectedReflectance_TrueColor/"
              "default/{time}/GoogleMapsCompatible_Level9/EPSG:3857:{z}/{y}/{x}.jpg");
    OnlineCatalogue catalogue;
    catalogue.providers.push_back(*provider);
    EXPECT_TRUE(validateCatalogue(catalogue).empty());
}

TEST(OnlineDiscovery, WfsFeatureTypesKeepTheirDefaultCrs)
{
    auto provider = parseWfsCapabilities(fixture("wfs_capabilities_200.xml"), "https://opendata.maps.vic.gov.au/geoserver/ows");
    ASSERT_TRUE(provider.ok()) << provider.error().describe();
    const auto layers = provider->layers();
    ASSERT_EQ(layers.size(), 2u);
    EXPECT_EQ(layers[0]->layerName, "open-data-platform:vmprop_parcel_mp");
    EXPECT_EQ(layers[0]->crs, "EPSG:7844");
    EXPECT_EQ(layers[0]->kind, OnlineLayerKind::Vector);
    EXPECT_EQ(layers[1]->crs, "EPSG:3111");
    EXPECT_EQ(provider->licence, "Creative Commons Attribution 4.0 International");
}

TEST(OnlineDiscovery, WcsCoveragesAreElevation)
{
    auto provider = parseWcsCapabilities(fixture("wcs_capabilities_100.xml"), "https://services.ga.gov.au/x/WCSServer");
    ASSERT_TRUE(provider.ok()) << provider.error().describe();
    const OnlineLayer* layer = provider->layers().front();
    EXPECT_EQ(layer->layerName, "1");
    EXPECT_EQ(layer->kind, OnlineLayerKind::Elevation);
    EXPECT_EQ(layer->coverage, (LonLatBox{112.99, -44.0, 153.99, -10.0}));
}

TEST(OnlineDiscovery, ArcgisDescriptionsGiveTheMapAndEachFeatureLayer)
{
    const std::string url = "https://maps.six.nsw.gov.au/arcgis/rest/services/public/NSW_Cadastre/MapServer";
    auto map = parseArcgisDescription(fixture("arcgis_mapserver.json"), url);
    ASSERT_TRUE(map.ok()) << map.error().describe();
    EXPECT_EQ(map->attribution, "© Spatial Services");
    std::vector<std::string> names;
    for (const OnlineLayer* layer : map->layers()) {
        names.push_back(layer->layerName.empty() ? "(map)" : layer->layerName);
    }
    // The map image, then the feature layers - not the group, not the annotation.
    EXPECT_EQ(names, (std::vector<std::string>{"(map)", "8", "9"}));
    const OnlineLayer* lots = map->findLayer("lot");
    ASSERT_NE(lots, nullptr);
    EXPECT_EQ(lots->type, OnlineServiceType::ArcgisQuery);
    EXPECT_EQ(lots->pageSize, 1000);
    // The Web Mercator extent back in degrees: 15695000 m east is 140.99 E.
    EXPECT_NEAR(lots->coverage[0], 15695000.0 / 20037508.342789244 * 180.0, 1e-6);

    auto one = parseArcgisDescription(fixture("arcgis_layer.json"), url + "/9");
    ASSERT_TRUE(one.ok()) << one.error().describe();
    ASSERT_EQ(one->layers().size(), 1u);
    EXPECT_EQ(one->layers().front()->endpoint, url);
    EXPECT_EQ(one->layers().front()->layerName, "9");
    EXPECT_EQ(one->layers().front()->pageSize, 2000);

    auto image = parseArcgisDescription(fixture("arcgis_imageserver.json"),
                                        "https://maps.six.nsw.gov.au/arcgis/rest/services/public/NSW_5M_Elevation/ImageServer");
    ASSERT_TRUE(image.ok()) << image.error().describe();
    const OnlineLayer* dem = image->layers().front();
    EXPECT_EQ(dem->kind, OnlineLayerKind::Elevation); // one F32 band
    EXPECT_EQ(dem->type, OnlineServiceType::ArcgisExportImage);
    ASSERT_TRUE(dem->resolution.has_value());
    EXPECT_NEAR(*dem->resolution, 4.5e-05 * 111320.0, 1e-9);
}

TEST(OnlineDiscovery, OgcApiAndStacDescriptionsAreRead)
{
    auto features = parseOapifCollections(fixture("oapif_collections.json"), "https://api.example.org/ogc/collections");
    ASSERT_TRUE(features.ok()) << features.error().describe();
    ASSERT_EQ(features->layers().size(), 2u);
    EXPECT_EQ(features->layers().front()->endpoint, "https://api.example.org/ogc");
    EXPECT_EQ(features->layers().front()->coverage, (LonLatBox{150.5, -34.2, 151.5, -33.4}));

    auto item = parseStacDescription(fixture("stac_item_sentinel2.json"),
                                     "https://sentinel-cogs.s3.us-west-2.amazonaws.com/x/S2B_56HLH_20260103_0_L2A.json");
    ASSERT_TRUE(item.ok()) << item.error().describe();
    const OnlineLayer* layer = item->layers().front();
    EXPECT_EQ(layer->asset, "visual");
    EXPECT_EQ(layer->attribution, "Contains modified Copernicus Sentinel data");
    EXPECT_EQ(layer->type, OnlineServiceType::Stac);
}

TEST(OnlineDiscovery, DiscoveryTriesTheCandidatesWhenTheUrlDoesNotSay)
{
    // A bare address: WMS is asked first, and a WFS answer to the WMS
    // question is not a WMS - so WMTS and then WFS are asked.
    std::vector<std::string> asked;
    const TextFetcher fetch = [&](const std::string& url) -> Result<std::string> {
        asked.push_back(url);
        if (url.find("SERVICE=WFS") != std::string::npos) {
            return fixture("wfs_capabilities_200.xml");
        }
        return katana::core::makeError(ErrorCode::NotFound, "no");
    };
    auto provider = discoverService("https://opendata.example.org/geoserver/ows", fetch);
    ASSERT_TRUE(provider.ok()) << provider.error().describe();
    EXPECT_EQ(provider->services.front().type, OnlineServiceType::Wfs);
    ASSERT_EQ(asked.size(), 3u);
    const auto none = discoverService("https://nothing.example.org/", [](const std::string&) -> Result<std::string> {
        return katana::core::makeError(ErrorCode::NotFound, "no");
    });
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::Unsupported);
    EXPECT_NE(none.error().message.find("tried wms, wmts, wfs"), std::string::npos);
    EXPECT_EQ(discoverService("ftp://x", fetch).error().code, ErrorCode::InvalidArgument);
}

// ---- vectors -----------------------------------------------------------------------------

TEST(OnlineFetch, ArcgisPagesAreJoinedWithoutLossOrRepeatAndMovedIntoTheProjectCrs)
{
    TempDir cache("arcgis-paging");
    FakeTransport transport;
    transport.answers = {{"resultOffset=0&", fixture("arcgis_query_page1.geojson")},
                         {"resultOffset=4&", fixture("arcgis_query_page2.geojson")},
                         {"resultOffset=8&", fixture("arcgis_query_page3.geojson")}};
    OnlineEnvironment environment = environmentWith(transport, cache.path());
    OnlineLayer lots = builtInLayer("nsw-spatial", "lots");
    lots.pageSize = 4;

    auto imported = fetchOnlineLayer(lots, sydneyOptions(), environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_TRUE(imported->vectors.has_value());
    const VectorImportResult& vectors = *imported->vectors;
    // Pages of 4, 4 and 3 where the second repeats one of the first: ten
    // lots, each once.
    EXPECT_EQ(vectors.entities.size(), 10u);
    EXPECT_EQ(imported->stats.duplicates, 1u);
    EXPECT_EQ(imported->stats.pages, 3);
    EXPECT_EQ(imported->stats.requests, 3);
    std::set<std::string> ids;
    for (const auto& entity : vectors.entities) {
        ids.insert(katana::entity::toString(entity.properties.at("cadid")));
        EXPECT_EQ(entity.layer, "online/nsw-spatial/lots");
        EXPECT_EQ(katana::entity::toString(entity.metadata.at("online.licence")), "CC BY 4.0");
        EXPECT_NE(katana::entity::toString(entity.metadata.at("online.attribution")).find("Spatial Services"),
                  std::string::npos);
        EXPECT_EQ(katana::entity::toString(entity.metadata.at("online.source")), lots.endpoint);
    }
    EXPECT_EQ(ids.size(), 10u);
    EXPECT_EQ(vectors.layersNeeded, (std::vector<std::string>{"online/nsw-spatial/lots"}));

    // Lot 1's first vertex is 151.2050, -33.8700; gdaltransform -s_srs
    // EPSG:4326 -t_srs EPSG:7856 puts it at 333973.178351782 6250808.33086044.
    const auto* polygon = std::get_if<katana::geometry::Polyline2>(&vectors.entities.front().geometry);
    ASSERT_NE(polygon, nullptr);
    EXPECT_NEAR(polygon->vertices.front().x, 333973.178351782, 1e-3);
    EXPECT_NEAR(polygon->vertices.front().y, 6250808.33086044, 1e-3);

    // The requests were the builder's: WGS 84 envelope, offsets by the count
    // each page actually held.
    ASSERT_EQ(transport.urls.size(), 3u);
    EXPECT_NE(transport.urls[0].find("/NSW_Cadastre/MapServer/9/query?"), std::string::npos);
    EXPECT_NE(transport.urls[1].find("resultOffset=4&resultRecordCount=4"), std::string::npos);

    // Again: every page from the cache, nothing asked.
    auto again = fetchOnlineLayer(lots, sydneyOptions(), environment);
    ASSERT_TRUE(again.ok()) << again.error().describe();
    EXPECT_EQ(again->stats.requests, 0);
    EXPECT_EQ(again->stats.cacheHits, 3);
    EXPECT_EQ(again->vectors->entities.size(), 10u);
    EXPECT_EQ(transport.urls.size(), 3u);
}

TEST(OnlineFetch, AServerThatIgnoresPagingEndsWithAWarningNotALoop)
{
    TempDir cache("arcgis-no-paging");
    FakeTransport transport;
    transport.answers = {{"/query?", fixture("arcgis_query_page1.geojson")}};
    OnlineLayer lots = builtInLayer("nsw-spatial", "lots");
    lots.pageSize = 4;
    auto imported = fetchOnlineLayer(lots, sydneyOptions(), environmentWith(transport, cache.path()));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_EQ(imported->vectors->entities.size(), 4u);
    EXPECT_EQ(transport.urls.size(), 2u);
    EXPECT_TRUE(mentions(imported->warnings, "may not support paging"));
}

TEST(OnlineFetch, TheFeatureLimitIsARefusalThatSaysWhatToDo)
{
    TempDir cache("arcgis-limit");
    FakeTransport transport;
    transport.answers = {{"resultOffset=0&", fixture("arcgis_query_page1.geojson")},
                         {"resultOffset=4&", fixture("arcgis_query_page2.geojson")}};
    OnlineEnvironment environment = environmentWith(transport, cache.path());
    environment.maxFeatures = 5;
    OnlineLayer lots = builtInLayer("nsw-spatial", "lots");
    lots.pageSize = 4;
    const auto refused = fetchOnlineLayer(lots, sydneyOptions(), environment);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("more than 5 features; choose a smaller area"), std::string::npos);
}

TEST(OnlineFetch, CancellingBetweenPagesStopsWithNothingImported)
{
    TempDir cache("arcgis-cancel");
    std::stop_source cancel;
    FakeTransport transport;
    transport.answers = {{"resultOffset=0&", fixture("arcgis_query_page1.geojson")},
                         {"resultOffset=4&", fixture("arcgis_query_page2.geojson")}};
    // The person presses Cancel while the first page downloads.
    transport.onRequest = [&](const gis::HttpRequest&) { cancel.request_stop(); };
    OnlineLayer lots = builtInLayer("nsw-spatial", "lots");
    lots.pageSize = 4;
    const auto stopped = fetchOnlineLayer(lots, sydneyOptions(), environmentWith(transport, cache.path()),
                                          cancel.get_token());
    ASSERT_FALSE(stopped.ok());
    EXPECT_EQ(stopped.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(stopped.error().message, "cancelled");
    EXPECT_EQ(transport.urls.size(), 1u);
}

TEST(OnlineFetch, AServiceRefusalCarriesTheServicesOwnWords)
{
    TempDir cache("arcgis-refusal");
    FakeTransport transport;
    transport.answers = {{"/query?", fixture("arcgis_error.json")}};
    OnlineEnvironment environment = environmentWith(transport, cache.path());
    const auto refused = fetchOnlineLayer(builtInLayer("nsw-spatial", "lots"), sydneyOptions(), environment);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(refused.error().context.find("'resultOffset' is not supported"), std::string::npos);
    // A refusal is not cached as though it were an answer.
    transport.answers = {{"/query?", fixture("arcgis_query_page3.geojson")}};
    auto retried = fetchOnlineLayer(builtInLayer("nsw-spatial", "lots"), sydneyOptions(), environment);
    ASSERT_TRUE(retried.ok()) << retried.error().describe();
    EXPECT_EQ(retried->vectors->entities.size(), 3u);
}

TEST(OnlineFetch, WfsAndOgcApiPagesAreFollowed)
{
    TempDir cache("wfs-oapif");
    FakeTransport transport;
    // Two GeoJSON pages for the WFS (a server may answer GetFeature with
    // GeoJSON; GDAL reads it whatever the file is called).
    transport.answers = {{"STARTINDEX=0", fixture("arcgis_query_page1.geojson")},
                         {"STARTINDEX=4", fixture("arcgis_query_page3.geojson")},
                         {"items?bbox=", fixture("oapif_items_page1.json")},
                         {"offset=2", fixture("arcgis_query_page3.geojson")}};
    OnlineEnvironment environment = environmentWith(transport, cache.path());
    OnlineLayer wfs = builtInLayer("vic-vicmap", "parcels");
    wfs.pageSize = 4;
    wfs.coverage = {140, -40, 155, -28};
    auto viaWfs = fetchOnlineLayer(wfs, sydneyOptions(), environment);
    ASSERT_TRUE(viaWfs.ok()) << viaWfs.error().describe();
    EXPECT_EQ(viaWfs->vectors->entities.size(), 7u);
    // EPSG:4326 in WFS 2.0 is latitude first.
    EXPECT_NE(transport.urls[0].find("&BBOX=-33.875,151.2,-33.865,151.215,EPSG%3A4326&"), std::string::npos);

    OnlineLayer oapif;
    oapif.providerId = "custom-example-invalid";
    oapif.providerTitle = "Example";
    oapif.id = "buildings";
    oapif.title = "Buildings";
    oapif.kind = OnlineLayerKind::Vector;
    oapif.type = OnlineServiceType::OgcApiFeatures;
    oapif.endpoint = "https://example.invalid/ogc";
    oapif.layerName = "buildings";
    oapif.licence = "L";
    oapif.attribution = "A";
    auto viaApi = fetchOnlineLayer(oapif, sydneyOptions(), environment);
    ASSERT_TRUE(viaApi.ok()) << viaApi.error().describe();
    EXPECT_EQ(viaApi->vectors->entities.size(), 5u); // 2 then 3, by the next link
    EXPECT_EQ(viaApi->stats.pages, 2);
}

TEST(OnlineFetch, OverpassIsAskedByPostAndItsAnswerReadByGdalsOsmDriver)
{
    TempDir cache("overpass");
    FakeTransport transport;
    transport.answers = {{"data=", fixture("overpass_buildings.osm")}};
    OnlineEnvironment environment = environmentWith(transport, cache.path());
    auto imported = fetchOnlineLayer(builtInLayer("osm", "buildings"), sydneyOptions(), environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(transport.bodies.size(), 1u);
    EXPECT_TRUE(transport.bodies[0].starts_with("data="));
    EXPECT_NE(transport.bodies[0].find("%5B%22building%22%5D"), std::string::npos); // ["building"]
    const auto& entities = imported->vectors->entities;
    ASSERT_GE(entities.size(), 2u); // the cafe point and the office outline
    bool office = false;
    for (const auto& entity : entities) {
        if (entity.properties.contains("name") && katana::entity::toString(entity.properties.at("name")) == "Test House") {
            office = true;
            EXPECT_EQ(katana::entity::toString(entity.metadata.at("online.licence")), "ODbL 1.0");
        }
    }
    EXPECT_TRUE(office);
    // A big area is refused before anything is asked.
    OnlineRequestOptions big = sydneyOptions();
    big.area = CrsBox{150.0, -35.0, 152.0, -33.0};
    const auto refused = fetchOnlineLayer(builtInLayer("osm", "buildings"), big, environment);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("limited to 25 km2"), std::string::npos);
    EXPECT_EQ(transport.urls.size(), 1u);
}

TEST(OnlineFetch, ALocalGeoJsonGoesThroughTheSameFetchPathAsAFileUrl)
{
    TempDir cache("file-url");
    OnlineLayer layer = builtInLayer("natural-earth", "countries");
    layer.endpoint = fileUrl(fs::path(kData) / "arcgis_query_page1.geojson");
    OnlineEnvironment environment;
    environment.cacheDirectory = cache.path();
    auto imported = fetchOnlineLayer(layer, sydneyOptions("EPSG:3857"), environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_EQ(imported->vectors->entities.size(), 4u);
    EXPECT_EQ(imported->stats.requests, 1);
    const auto* polygon = std::get_if<katana::geometry::Polyline2>(&imported->vectors->entities.front().geometry);
    ASSERT_NE(polygon, nullptr);
    // Web Mercator from its formula (x = R lambda, y = R ln tan(pi/4 + phi/2)).
    // The file is clipped to the area before it is read, which may start the
    // ring at another corner: the corner is looked for, not assumed first.
    const double x = 6378137.0 * 151.2050 * std::numbers::pi / 180.0;
    const double y = 6378137.0 * std::log(std::tan(std::numbers::pi / 4.0 + -33.8700 * std::numbers::pi / 360.0));
    EXPECT_TRUE(std::any_of(polygon->vertices.begin(), polygon->vertices.end(), [&](const auto& vertex) {
        return std::abs(vertex.x - x) < 1e-4 && std::abs(vertex.y - y) < 1e-4;
    }));
    EXPECT_EQ(polygon->vertices.size(), 4u);
    // The area filter: an area away from the lots imports none of them.
    OnlineRequestOptions elsewhere = sydneyOptions("EPSG:3857");
    elsewhere.area = CrsBox{150.0, -35.0, 150.01, -34.99};
    auto none = fetchOnlineLayer(layer, elsewhere, environment);
    ASSERT_TRUE(none.ok()) << none.error().describe();
    EXPECT_TRUE(none->vectors->entities.empty());
}

TEST(OnlineFetch, AKeyIsSentButNeverRecorded)
{
    TempDir cache("key");
    FakeTransport transport;
    transport.answers = {{"data.geojson", fixture("arcgis_query_page3.geojson")}};
    OnlineEnvironment environment = environmentWith(transport, cache.path());
    OnlineLayer layer = builtInLayer("natural-earth", "countries");
    layer.endpoint = "https://example.invalid/data.geojson?key={key}";
    layer.keyRequired = true;
    layer.keyName = "example";
    const auto missing = fetchOnlineLayer(layer, sydneyOptions(), environment);
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    EXPECT_NE(missing.error().message.find("ONLINE KEY example <key>"), std::string::npos);
    EXPECT_TRUE(transport.urls.empty());

    environment.keys["example"] = "S3CR3T value";
    auto imported = fetchOnlineLayer(layer, sydneyOptions(), environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(transport.urls.size(), 1u);
    EXPECT_NE(transport.urls[0].find("key=S3CR3T%20value"), std::string::npos);
    EXPECT_EQ(imported->sourceUrl, "https://example.invalid/data.geojson?key=***");
    for (const auto& entity : imported->vectors->entities) {
        for (const auto& [name, value] : entity.metadata) {
            EXPECT_EQ(katana::entity::toString(value).find("S3CR3T"), std::string::npos) << name;
        }
    }
    // Nor in the cache's file names.
    for (const auto& entry : fs::recursive_directory_iterator(cache.path())) {
        EXPECT_EQ(entry.path().string().find("S3CR3T"), std::string::npos);
    }
}

TEST(OnlineFetch, AnAreaOutsideTheProvidersCoverageIsRefusedByName)
{
    OnlineEnvironment environment;
    OnlineRequestOptions perth = sydneyOptions("EPSG:7850");
    perth.area = CrsBox{115.8, -32.0, 115.9, -31.9};
    const auto refused = fetchOnlineLayer(builtInLayer("nsw-spatial", "lots"), perth, environment);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("outside what NSW Spatial Services covers"), std::string::npos);
    OnlineRequestOptions local = sydneyOptions("");
    const auto noCrs = fetchOnlineLayer(builtInLayer("nsw-spatial", "lots"), local, environment);
    ASSERT_FALSE(noCrs.ok());
    EXPECT_EQ(noCrs.error().code, ErrorCode::InvalidCRS);
}

// ---- rasters ------------------------------------------------------------------------------

TEST(OnlineFetch, ALocalGeoTiffIsWarpedIntoTheProjectWithItsValuesKept)
{
    TempDir work("cog");
    // A 5 m grid in MGA 56 over the Sydney CBD: 400 x 300 cells from
    // (333000, 6252000).
    const fs::path dem = writeGrid(work.path() / "dem.tif", "EPSG:7856", 333000.0, 6252000.0, 5.0, 400, 300);
    OnlineLayer layer = builtInLayer("copernicus", "dem");
    layer.endpoint = fileUrl(dem);
    layer.tiling.clear();
    OnlineEnvironment environment;
    environment.cacheDirectory = work.path() / "cache";
    OnlineRequestOptions options;
    // An area on the grid's own cell edges, at its own resolution: every
    // warped cell centre is a source cell centre, so bilinear gives the
    // source's value exactly.
    options.area = CrsBox{334000.0, 6251000.0, 334500.0, 6251400.0};
    options.areaCrs = "EPSG:7856";
    options.targetCrs = "EPSG:7856";
    options.resolution = 5.0;
    auto imported = fetchOnlineLayer(layer, options, environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_TRUE(imported->raster.has_value());
    EXPECT_EQ(imported->raster->licence, layer.licence);
    EXPECT_EQ(imported->raster->attribution, layer.attribution);
    EXPECT_TRUE(imported->raster->hasGeotransform);
    EXPECT_DOUBLE_EQ(imported->raster->geotransform[0], 334000.0);
    EXPECT_DOUBLE_EQ(imported->raster->geotransform[3], 6251400.0);

    auto warped = gis::GdalDataset::open(imported->file);
    ASSERT_TRUE(warped.ok());
    auto info = (*warped)->rasterInfo();
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(info->width, 100);
    EXPECT_EQ(info->height, 80);
    auto band = (*warped)->readBand(1);
    ASSERT_TRUE(band.ok());
    // Output cell (0, 0) is source cell (200, 120): 1000 * 120 + 200.
    EXPECT_DOUBLE_EQ((*band)[0], 120200.0);
    // Output cell (99, 79) is source (299, 199).
    EXPECT_DOUBLE_EQ((*band)[79 * 100 + 99], 199299.0);

    // Asked again: the finished raster itself comes from the cache.
    auto again = fetchOnlineLayer(layer, options, environment);
    ASSERT_TRUE(again.ok()) << again.error().describe();
    EXPECT_TRUE(again->stats.productFromCache);
    EXPECT_EQ(again->file, imported->file);
}

TEST(OnlineFetch, TilesAreFetchedMosaickedAndWarpedAndCachedForTheNextTime)
{
    TempDir work("tiles");
    FakeTransport transport;
    // Every tile is the same 256-pixel grey GeoTIFF; where it lies comes
    // from the tile index, not from the file.
    const fs::path tile = writeGrid(work.path() / "tile.tif", "EPSG:3857", 0.0, 256.0, 1.0, 256, 256);
    transport.answers = {{"https://tiles.example.invalid/", readFile(tile)}};
    OnlineEnvironment environment = environmentWith(transport, work.path() / "cache");
    OnlineLayer layer = builtInLayer("osm", "standard");
    layer.endpoint = "https://tiles.example.invalid/{z}/{x}/{y}.tif";
    OnlineRequestOptions options = sydneyOptions();
    options.resolution = 2.0;
    auto imported = fetchOnlineLayer(layer, options, environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_GT(imported->stats.requests, 0);
    EXPECT_LE(imported->stats.requests, layer.maxTiles);
    EXPECT_EQ(imported->raster->attribution, "© OpenStreetMap contributors");
    // The raster lies over the area in MGA 56: its corners are the area's.
    const auto area = gis::transformBox(options.area, "EPSG:4326", "EPSG:7856");
    ASSERT_TRUE(area.ok());
    EXPECT_NEAR(imported->raster->geotransform[0], area->minX, 1e-6);
    EXPECT_NEAR(imported->raster->geotransform[3], area->maxY, 2.0);
    // Every tile asked for is a real tile of the zoom the resolution needs:
    // 2 m on the ground at 33.87 S is 2.41 Web Mercator metres, level 16
    // (2.39 m) the first at or below it.
    for (const std::string& url : transport.urls) {
        EXPECT_TRUE(url.starts_with("https://tiles.example.invalid/16/")) << url;
    }
    const std::size_t asked = transport.urls.size();
    auto again = fetchOnlineLayer(layer, options, environment);
    ASSERT_TRUE(again.ok());
    EXPECT_TRUE(again->stats.productFromCache);
    EXPECT_EQ(transport.urls.size(), asked);
    // A different resolution is a different product, but the same tiles
    // come from the cache.
    options.resolution = 2.2;
    auto third = fetchOnlineLayer(layer, options, environment);
    ASSERT_TRUE(third.ok()) << third.error().describe();
    EXPECT_FALSE(third->stats.productFromCache);
    EXPECT_EQ(third->stats.requests, 0);
    EXPECT_GT(third->stats.cacheHits, 0);
}

TEST(OnlineFetch, RasterLimitsAreRefusedBeforeAnyRequestWithTheResolutionThatFits)
{
    TempDir work("raster-limits");
    FakeTransport transport;
    OnlineEnvironment environment = environmentWith(transport, work.path());
    environment.maxPixels = 1000000;
    OnlineRequestOptions options = sydneyOptions();
    options.resolution = 0.5; // 2773 x 2224 pixels
    const auto refused = fetchOnlineLayer(builtInLayer("nsw-spatial", "imagery"), options, environment);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("use res="), std::string::npos);
    EXPECT_TRUE(transport.urls.empty());
    // A tile service's own budget: the OpenStreetMap standard map allows 64.
    // (The pixel limit is lifted so that it is the tiles that refuse.)
    environment.maxPixels = 1ull << 34;
    options.area = CrsBox{151.0, -34.0, 151.3, -33.7};
    options.resolution = 1.0;
    const auto tooManyTiles = fetchOnlineLayer(builtInLayer("osm", "standard"), options, environment);
    ASSERT_FALSE(tooManyTiles.ok());
    EXPECT_NE(tooManyTiles.error().message.find("more than the 64 tiles"), std::string::npos);
    EXPECT_TRUE(transport.urls.empty());
}

TEST(OnlineFetch, AnImageServiceThatAnswersWithAMessageIsReportedNotWarped)
{
    TempDir work("service-exception");
    FakeTransport transport;
    transport.answers = {{"GetMap", "<?xml version=\"1.0\"?><ServiceExceptionReport><ServiceException code=\"LayerNotDefined\">"
                                    "Layer ga_ls_fc_3 is not defined</ServiceException></ServiceExceptionReport>"}};
    const auto refused = fetchOnlineLayer(builtInLayer("dea", "fractional-cover"), sydneyOptions(),
                                          environmentWith(transport, work.path()));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(refused.error().context.find("Layer ga_ls_fc_3 is not defined"), std::string::npos);
    // The WMS was asked in Web Mercator, x,y.
    ASSERT_FALSE(transport.urls.empty());
    EXPECT_NE(transport.urls[0].find("CRS=EPSG%3A3857&BBOX=16831"), std::string::npos);
}

TEST(OnlineFetch, AStacItemsVisualAssetIsReadAsACog)
{
    TempDir work("stac");
    // A scene in the item's own UTM zone (56 S, EPSG:32756), as Sentinel-2
    // COGs are, covering the Sydney area at 10 m.
    const fs::path visual =
        writeGrid(work.path() / "TCI.tif", "EPSG:32756", 332000.0, 6253000.0, 10.0, 400, 400);
    std::string item = fixture("stac_item_sentinel2.json");
    const std::string original =
        "https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/56/H/LH/2026/1/S2B_56HLH_20260103_0_L2A/TCI.tif";
    ASSERT_NE(item.find(original), std::string::npos);
    item.replace(item.find(original), original.size(), fileUrl(visual));
    FakeTransport transport;
    transport.answers = {{"item.json", item}};
    OnlineEnvironment environment = environmentWith(transport, work.path() / "cache");
    OnlineLayer layer = builtInLayer("sentinel2", "truecolour");
    layer.endpoint = "https://example.invalid/item.json";
    auto imported = fetchOnlineLayer(layer, sydneyOptions(), environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_TRUE(mentions(imported->warnings, "scene date 2026-01-03"));
    EXPECT_EQ(imported->raster->attribution, "Contains modified Copernicus Sentinel data");
    EXPECT_GT(imported->raster->width, 0);
}

TEST(OnlineFetch, TheCacheIsPrunedOldestFirstToItsSize)
{
    TempDir work("prune");
    for (int i = 0; i < 4; ++i) {
        const fs::path file = work.path() / "http" / "ab" / ("f" + std::to_string(i) + ".bin");
        fs::create_directories(file.parent_path());
        std::ofstream(file, std::ios::binary) << std::string(1000, 'x');
        fs::last_write_time(file, fs::file_time_type::clock::now() - std::chrono::hours(4 - i));
    }
    EXPECT_EQ(pruneCache(work.path(), 30, 2500), 2000u);
    EXPECT_FALSE(fs::exists(work.path() / "http" / "ab" / "f0.bin"));
    EXPECT_FALSE(fs::exists(work.path() / "http" / "ab" / "f1.bin"));
    EXPECT_TRUE(fs::exists(work.path() / "http" / "ab" / "f3.bin"));
    EXPECT_EQ(pruneCache(work.path(), 0, 1u << 30), 2000u); // everything is older than 0 days
}

// ---- live: only with KATANA_ONLINE_TESTS=1 -----------------------------------------------------

namespace {

bool liveTestsEnabled()
{
    const char* flag = std::getenv("KATANA_ONLINE_TESTS");
    return flag != nullptr && std::string(flag) == "1";
}

} // namespace

TEST(OnlineLive, CopernicusDemAtSydneyIsAboutSeaLevelToFiftyMetres)
{
    if (!liveTestsEnabled()) {
        GTEST_SKIP() << "set KATANA_ONLINE_TESTS=1 to reach the network";
    }
    TempDir work("live-copernicus");
    OnlineEnvironment environment;
    environment.cacheDirectory = work.path();
    OnlineRequestOptions options = sydneyOptions();
    auto imported = fetchOnlineLayer(builtInLayer("copernicus", "dem"), options, environment);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    auto dataset = gis::GdalDataset::open(imported->file);
    ASSERT_TRUE(dataset.ok());
    auto band = (*dataset)->readBand(1);
    ASSERT_TRUE(band.ok());
    double low = 1e9;
    double high = -1e9;
    for (const double value : *band) {
        if (value > -1000.0) {
            low = std::min(low, value);
            high = std::max(high, value);
        }
    }
    // GLO-30 is a SURFACE model: the ground runs from the harbour (0 m) to
    // about 40 m on the ridges, and the towers of the CBD stand on it - the
    // tallest, Sydney Tower, 309 m above the street. So: nothing below the
    // sea, something well above the ground, nothing above the tower.
    EXPECT_GT(low, -5.0);
    EXPECT_GT(high, 40.0);
    EXPECT_LT(high, 350.0);
}

TEST(OnlineLive, EveryBuiltInEndpointAnswers)
{
    if (!liveTestsEnabled()) {
        GTEST_SKIP() << "set KATANA_ONLINE_TESTS=1 to reach the network";
    }
    // Each service's description, or for templates one tile of the area, so
    // a dead address shows here by name rather than in a person's import.
    auto catalogue = builtInCatalogue();
    ASSERT_TRUE(catalogue.ok());
    OnlineEnvironment environment;
    for (const OnlineProvider& provider : catalogue->providers) {
        for (const OnlineService& service : provider.services) {
            const OnlineLayer& layer = service.layers.front();
            std::string url;
            switch (service.type) {
            case OnlineServiceType::XyzTiles:
            case OnlineServiceType::Wmts:
                url = expandTileTemplate(layer.endpoint, TileIndex{2, 3, 2}, {},
                                         {{"layer", layer.layerName}, {"time", "default"}});
                break;
            case OnlineServiceType::Cog:
                url = degreeTileUrls(layer.endpoint, CrsBox{151.2, -33.9, 151.21, -33.89}).front();
                break;
            case OnlineServiceType::File:
                url = layer.endpoint;
                url.replace(url.find("{layer}"), 7, layer.layerName);
                break;
            case OnlineServiceType::Stac:
            case OnlineServiceType::Overpass:
            case OnlineServiceType::Ckan:
                url = layer.endpoint.substr(0, layer.endpoint.find_last_of('/'));
                break;
            default:
                url = descriptionUrl(service.type, layer.endpoint);
            }
            gis::HttpRequest request;
            request.url = url;
            request.userAgent = environment.userAgent;
            request.maxBytes = 64ull << 20;
            request.retries = 1;
            const auto answer = gis::httpFetch(request);
            EXPECT_TRUE(answer.ok()) << provider.id << "/" << service.id << ": "
                                     << (answer ? "" : answer.error().describe());
        }
    }
}
