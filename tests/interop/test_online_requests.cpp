// Requests for every kind of web service (docs/gis_online.md, "How each
// service is asked"), and the pieces under them: the web access helpers, the
// XML reader, the coordinate transformation with its axis order, the tile
// arithmetic, the STAC choice and the Overpass query. No network: every
// request is compared with what the service's own specification says it
// must be, and every number with one worked out here from the specification's
// formula - never with what the code printed.

#include <gtest/gtest.h>

#include <cmath>
#include <fstream>
#include <iterator>
#include <numbers>
#include <string>

#include "katana/gis/reproject.hpp"
#include "katana/gis/web_access.hpp"
#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_requests.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;
using katana::gis::CrsBox;
namespace gis = katana::gis;

namespace {

OnlineLayer layerOf(OnlineServiceType type, std::string endpoint, std::string name = {})
{
    OnlineLayer layer;
    layer.type = type;
    layer.endpoint = std::move(endpoint);
    layer.layerName = std::move(name);
    return layer;
}

// Web Mercator (EPSG:3857, the spherical form of EPSG guidance note 7-2,
// 1.3.3.2) worked from its formula: x = R lambda, y = R ln tan(pi/4 + phi/2).
gis::HttpRequest requestFor(std::string url)
{
    gis::HttpRequest request;
    request.url = std::move(url);
    return request;
}

double mercatorX(double lon)
{
    return 6378137.0 * lon * std::numbers::pi / 180.0;
}
double mercatorY(double lat)
{
    return 6378137.0 * std::log(std::tan(std::numbers::pi / 4.0 + lat * std::numbers::pi / 360.0));
}

} // namespace

// ---- web access helpers ---------------------------------------------------------------

TEST(OnlineWeb, Sha256MatchesTheStandardsVectors)
{
    // FIPS 180-2, appendix B: "abc", the empty string, and the 448-bit message.
    EXPECT_EQ(gis::sha256Hex("abc"),
              "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    EXPECT_EQ(gis::sha256Hex(""),
              "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    EXPECT_EQ(gis::sha256Hex("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
              "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    // A million 'a's crosses many blocks (B.3).
    EXPECT_EQ(gis::sha256Hex(std::string(1000000, 'a')),
              "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
}

TEST(OnlineWeb, RedactUrlHidesKeysAndCredentialsAndNothingElse)
{
    EXPECT_EQ(gis::redactUrl("https://a.org/x?layer=1&key=SECRET&f=json"),
              "https://a.org/x?layer=1&key=***&f=json");
    EXPECT_EQ(gis::redactUrl("https://a.org/x?apiKey=S&access_token=T&token=U&sig=V"),
              "https://a.org/x?apiKey=***&access_token=***&token=***&sig=***");
    EXPECT_EQ(gis::redactUrl("https://user:pass@a.org/x"), "https://a.org/x");
    EXPECT_EQ(gis::redactUrl("https://a.org/tiles/{z}/{x}/{y}.png"),
              "https://a.org/tiles/{z}/{x}/{y}.png");
    // A parameter whose name merely contains "key" at its start is not a key.
    EXPECT_EQ(gis::redactUrl("https://a.org/x?keyword=roads"), "https://a.org/x?keyword=roads");
}

TEST(OnlineWeb, PercentEncodeKeepsOnlyTheUnreservedSet)
{
    // RFC 3986, 2.3: ALPHA DIGIT - . _ ~ stay; everything else is %XX, and a
    // space is %20.
    EXPECT_EQ(gis::percentEncode("AZaz09-._~"), "AZaz09-._~");
    EXPECT_EQ(gis::percentEncode("1=1 & show:9/é"), "1%3D1%20%26%20show%3A9%2F%C3%A9");
}

TEST(OnlineWeb, AFileUrlIsReadWithoutTheNetworkAndItsLimitHolds)
{
    const auto missing = gis::httpFetch(requestFor("file:///no/such/file.json"));
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    const auto wrong = gis::httpFetch(requestFor("gopher://example.org/x"));
    ASSERT_FALSE(wrong.ok());
    EXPECT_EQ(wrong.error().code, ErrorCode::InvalidArgument);

    gis::HttpRequest request;
    request.url = std::string("file://") + KATANA_ONLINE_TEST_DATA + "/ckan_search.json";
    auto read = gis::httpFetch(request);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_NE(read->body.find("package_search"), std::string::npos);
    EXPECT_EQ(read->contentType, "application/json");
    request.maxBytes = 10;
    const auto tooBig = gis::httpFetch(request);
    ASSERT_FALSE(tooBig.ok());
    EXPECT_EQ(tooBig.error().code, ErrorCode::Unsupported);
    EXPECT_NE(tooBig.error().message.find("smaller area"), std::string::npos);
}

TEST(OnlineWeb, AStoppedFetchEndsAsCancelledBeforeAnyRequest)
{
    std::stop_source source;
    source.request_stop();
    const auto stopped = gis::httpFetch(requestFor("https://example.invalid/never"), source.get_token());
    ASSERT_FALSE(stopped.ok());
    EXPECT_EQ(stopped.error().code, ErrorCode::InvalidState);
    EXPECT_EQ(stopped.error().message, "cancelled");
}

TEST(OnlineWeb, XmlIsReadIntoElementsByLocalName)
{
    auto root = gis::parseXml(
        R"(<?xml version="1.0"?><!-- c --><a:Root xmlns:a="urn:x" a:version="2"><a:Child>one</a:Child>)"
        R"(<Child xlink:href="h"> two </Child><Other/></a:Root>)");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->name, "a:Root");
    EXPECT_EQ(root->localName(), "Root");
    EXPECT_EQ(root->attribute("version"), "2");
    ASSERT_EQ(root->childrenNamed("Child").size(), 2u);
    EXPECT_EQ(root->childText("Child"), "one");
    EXPECT_EQ(root->childrenNamed("Child")[1]->text, "two");
    EXPECT_EQ(root->childrenNamed("Child")[1]->attribute("href"), "h");
    EXPECT_EQ(root->child("Missing"), nullptr);
    EXPECT_EQ(gis::parseXml("not xml at all <").error().code, ErrorCode::ParseFailure);
}

// ---- coordinate systems and axis order -------------------------------------------------

TEST(OnlineCrs, AxisOrderIsTheAuthoritysNotTheTraditionalOne)
{
    // EPSG:4326 and GDA2020 geographic are latitude first by the EPSG
    // registry; CRS84, Web Mercator and MGA are easting first.
    EXPECT_TRUE(*gis::crsAxisIsYX("EPSG:4326"));
    EXPECT_TRUE(*gis::crsAxisIsYX("EPSG:7844"));
    EXPECT_FALSE(*gis::crsAxisIsYX("OGC:CRS84"));
    EXPECT_FALSE(*gis::crsAxisIsYX("EPSG:3857"));
    EXPECT_FALSE(*gis::crsAxisIsYX("EPSG:7856"));
    EXPECT_EQ(gis::crsAxisIsYX("EPSG:999999").error().code, ErrorCode::InvalidCRS);
    EXPECT_EQ(gis::crsEpsgCode("EPSG:28356"), 28356);
}

TEST(OnlineCrs, PointsGoWhereTheFormulaAndAnIndependentToolPutThem)
{
    // Web Mercator from its formula.
    const auto mercator = gis::transformPoint(151.2053, -33.8697, "EPSG:4326", "EPSG:3857");
    ASSERT_TRUE(mercator.ok()) << mercator.error().describe();
    EXPECT_NEAR((*mercator)[0], mercatorX(151.2053), 1e-6);
    EXPECT_NEAR((*mercator)[1], mercatorY(-33.8697), 1e-6);
    // MGA zone 56 (GDA2020): gdaltransform -s_srs EPSG:7844 -t_srs EPSG:7856
    // gives 334000.349071675 6250842.08445121 for this point - the command
    // line tool, outside Katana. Traditional order in, easting first out.
    const auto mga = gis::transformPoint(151.2053, -33.8697, "EPSG:7844", "EPSG:7856");
    ASSERT_TRUE(mga.ok()) << mga.error().describe();
    EXPECT_NEAR((*mga)[0], 334000.349071675, 1e-3);
    EXPECT_NEAR((*mga)[1], 6250842.08445121, 1e-3);
}

TEST(OnlineCrs, ABoxIsDensifiedSoItEnclosesItsCurvedImage)
{
    // One degree around Sydney into MGA 56: the box must hold every edge
    // point's image, which a corners-only transform does not guarantee.
    const CrsBox lonLat{150.5, -34.5, 151.5, -33.5};
    const auto box = gis::transformBox(lonLat, "EPSG:7844", "EPSG:7856");
    ASSERT_TRUE(box.ok()) << box.error().describe();
    for (int i = 0; i <= 20; ++i) {
        const double t = i / 20.0;
        for (const auto& [x, y] : {std::pair{150.5 + t, -34.5}, std::pair{150.5 + t, -33.5},
                                   std::pair{150.5, -34.5 + t}, std::pair{151.5, -34.5 + t}}) {
            const auto p = gis::transformPoint(x, y, "EPSG:7844", "EPSG:7856");
            ASSERT_TRUE(p.ok());
            EXPECT_GE((*p)[0], box->minX - 1e-6);
            EXPECT_LE((*p)[0], box->maxX + 1e-6);
            EXPECT_GE((*p)[1], box->minY - 1e-6);
            EXPECT_LE((*p)[1], box->maxY + 1e-6);
        }
    }
    EXPECT_EQ(gis::transformBox(CrsBox{1, 1, 0, 2}, "EPSG:4326", "EPSG:3857").error().code,
              ErrorCode::InvalidArgument);
}

TEST(OnlineCrs, FeaturesAreMovedVertexByVertexWithHeightsKept)
{
    gis::VectorFeature feature;
    feature.geometry.kind = gis::GeometryKind::LineString;
    feature.geometry.hasZ = true;
    feature.geometry.parts = {{{151.2053, -33.8697, 12.5}, {151.21, -33.87, 13.0}}};
    auto moved = gis::reprojectFeatures({feature}, "EPSG:7844", "EPSG:7856");
    ASSERT_TRUE(moved.ok()) << moved.error().describe();
    const auto& first = moved->front().geometry.parts.front().front();
    EXPECT_NEAR(first.x, 334000.349071675, 1e-3);
    EXPECT_NEAR(first.y, 6250842.08445121, 1e-3);
    EXPECT_DOUBLE_EQ(first.z, 12.5);
    EXPECT_EQ(moved->front().attributes, feature.attributes);
}

// ---- image sizing ------------------------------------------------------------------------

TEST(OnlineRequests, AnImageRequestIsGrownToWholePixelsAndSplitAtTheServiceLimit)
{
    auto one = planImageRequests(CrsBox{0, 0, 1000, 500}, 1.0, 2048);
    ASSERT_TRUE(one.ok());
    ASSERT_EQ(one->size(), 1u);
    EXPECT_EQ(one->front().width, 1000);
    EXPECT_EQ(one->front().height, 500);
    // 1000.5 units at 1 per pixel is 1001 pixels: the box grows to match.
    auto grown = planImageRequests(CrsBox{0, 0, 1000.5, 10}, 1.0, 2048);
    ASSERT_TRUE(grown.ok());
    EXPECT_EQ(grown->front().width, 1001);
    EXPECT_DOUBLE_EQ(grown->front().box.maxX, 1001.0);
    // 5000 x 3000 pixels at a 2048 limit: 3 across, 2 down, north-west first,
    // the pieces tiling the area exactly.
    auto split = planImageRequests(CrsBox{0, 0, 5000, 3000}, 1.0, 2048);
    ASSERT_TRUE(split.ok());
    ASSERT_EQ(split->size(), 6u);
    EXPECT_EQ((*split)[0].width, 2048);
    EXPECT_EQ((*split)[2].width, 5000 - 4096);
    EXPECT_EQ((*split)[3].height, 3000 - 2048);
    EXPECT_DOUBLE_EQ((*split)[0].box.maxY, 3000.0);
    EXPECT_DOUBLE_EQ((*split)[0].box.minX, 0.0);
    EXPECT_DOUBLE_EQ((*split)[5].box.minY, 0.0);
    EXPECT_DOUBLE_EQ((*split)[5].box.maxX, 5000.0);
    double area = 0.0;
    for (const ImageRequest& request : *split) {
        area += request.box.width() * request.box.height();
        EXPECT_DOUBLE_EQ(request.box.width(), request.width * 1.0);
    }
    EXPECT_DOUBLE_EQ(area, 5000.0 * 3000.0);
}

TEST(OnlineRequests, TooManyImageRequestsSayWhichResolutionWouldFit)
{
    const auto refused = planImageRequests(CrsBox{0, 0, 100000, 100000}, 1.0, 1000, 64);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("10000 image requests"), std::string::npos);
    EXPECT_NE(refused.error().message.find("resolution of 12.5"), std::string::npos);
}

// ---- ArcGIS REST ---------------------------------------------------------------------------

TEST(OnlineRequests, ArcgisExportAsksForExactlyTheBoxAndSize)
{
    const OnlineLayer layer = layerOf(OnlineServiceType::ArcgisExport,
                                      "https://maps.example.org/arcgis/rest/services/Topo/MapServer/", "3");
    const ImageRequest request{CrsBox{16830000, -4011000, 16832000, -4010000}, 2000, 1000};
    EXPECT_EQ(arcgisExportUrl(layer, request, 3857),
              "https://maps.example.org/arcgis/rest/services/Topo/MapServer/export?"
              "bbox=16830000,-4011000,16832000,-4010000&bboxSR=3857&imageSR=3857&size=2000,1000"
              "&dpi=96&format=png32&transparent=true&layers=show%3A3&f=image");
}

TEST(OnlineRequests, ArcgisExportImageAsksForValuesForElevationAndAPictureOtherwise)
{
    OnlineLayer layer = layerOf(OnlineServiceType::ArcgisExportImage,
                                "https://maps.example.org/arcgis/rest/services/DEM/ImageServer");
    layer.kind = OnlineLayerKind::Elevation;
    const ImageRequest request{CrsBox{0, 0, 100, 50}, 20, 10};
    EXPECT_EQ(arcgisExportImageUrl(layer, request, 3857),
              "https://maps.example.org/arcgis/rest/services/DEM/ImageServer/exportImage?"
              "bbox=0,0,100,50&bboxSR=3857&imageSR=3857&size=20,10&format=tiff&pixelType=F32"
              "&noData=-32767&interpolation=RSP_BilinearInterpolation&f=image");
    layer.kind = OnlineLayerKind::Imagery;
    EXPECT_NE(arcgisExportImageUrl(layer, request, 3857).find("&format=png&pixelType=U8"),
              std::string::npos);
}

TEST(OnlineRequests, ArcgisQueryPagesByOffsetInWgs84)
{
    const OnlineLayer layer = layerOf(OnlineServiceType::ArcgisQuery,
                                      "https://maps.example.org/arcgis/rest/services/Cadastre/MapServer", "9");
    EXPECT_EQ(arcgisQueryUrl(layer, CrsBox{151.2, -33.875, 151.215, -33.865}, 2000, 1000, "objectid"),
              "https://maps.example.org/arcgis/rest/services/Cadastre/MapServer/9/query?"
              "where=1%3D1&geometry=151.2,-33.875,151.215,-33.865&geometryType=esriGeometryEnvelope"
              "&inSR=4326&spatialRel=esriSpatialRelIntersects&outFields=*&returnGeometry=true"
              "&outSR=4326&resultOffset=2000&resultRecordCount=1000&orderByFields=objectid&f=geojson");
}

TEST(OnlineRequests, AQueryPageSaysWhetherMoreFollow)
{
    const auto page = readQueryPage(
        R"({"type":"FeatureCollection","features":[{},{}],"properties":{"exceededTransferLimit":true}})");
    ASSERT_TRUE(page.ok());
    EXPECT_EQ(page->features, 2u);
    EXPECT_TRUE(page->exceededTransferLimit);
    EXPECT_TRUE(queryHasMore(*page, 1000));
    // Esri JSON puts the flag at the top.
    const auto esri = readQueryPage(R"({"features":[{}],"exceededTransferLimit":true})");
    ASSERT_TRUE(esri.ok());
    EXPECT_TRUE(esri->exceededTransferLimit);
    // No flag, but a full page: a server that omits it is asked again.
    const auto full = readQueryPage(R"({"features":[{},{},{}]})");
    EXPECT_TRUE(queryHasMore(*full, 3));
    EXPECT_FALSE(queryHasMore(*full, 4));
    EXPECT_FALSE(queryHasMore(*readQueryPage(R"({"features":[]})"), 4));
    // The server's refusal, with its own words.
    const auto refused = readQueryPage(
        R"({"error":{"code":400,"message":"Invalid parameters.","details":["'resultOffset' is not supported"]}})");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(refused.error().context.find("resultOffset"), std::string::npos);
}

// ---- OGC ---------------------------------------------------------------------------------

TEST(OnlineRequests, Wms130WritesAGeographicBoxLatitudeFirst)
{
    OnlineLayer layer = layerOf(OnlineServiceType::Wms, "https://ows.example.org/wms?", "a b");
    layer.version = "1.3.0";
    const ImageRequest request{CrsBox{150, -34, 151, -33}, 512, 512};
    // WMS 1.3.0, 6.7.4: BBOX in the CRS's axis order - lat,lon for EPSG:4326.
    EXPECT_EQ(wmsGetMapUrl(layer, request, "EPSG:4326", true),
              "https://ows.example.org/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=a%20b"
              "&STYLES=&CRS=EPSG%3A4326&BBOX=-34,150,-33,151&WIDTH=512&HEIGHT=512"
              "&FORMAT=image%2Fpng&TRANSPARENT=TRUE");
    // In Web Mercator the order is x,y.
    EXPECT_NE(wmsGetMapUrl(layer, ImageRequest{CrsBox{1, 2, 3, 4}, 8, 8}, "EPSG:3857", false)
                  .find("&BBOX=1,2,3,4&"),
              std::string::npos);
    // WMS 1.1.1 is always x,y and says SRS.
    layer.version = "1.1.1";
    const std::string old = wmsGetMapUrl(layer, request, "EPSG:4326", true);
    EXPECT_NE(old.find("&SRS=EPSG%3A4326&BBOX=150,-34,151,-33&"), std::string::npos);
    layer.version = "1.3.0";
    layer.time = "2026-01-01";
    EXPECT_NE(wmsGetMapUrl(layer, request, "EPSG:3857", false).find("&TIME=2026-01-01"),
              std::string::npos);
}

TEST(OnlineRequests, WcsAsksForAGeoTiffOfTheBox)
{
    const OnlineLayer layer = layerOf(OnlineServiceType::Wcs,
                                      "https://services.example.org/DEM/MapServer/WCSServer", "1");
    EXPECT_EQ(wcsGetCoverageUrl(layer, ImageRequest{CrsBox{151, -34, 151.5, -33.5}, 1800, 1800}, "EPSG:4326"),
              "https://services.example.org/DEM/MapServer/WCSServer?SERVICE=WCS&VERSION=1.0.0"
              "&REQUEST=GetCoverage&COVERAGE=1&CRS=EPSG%3A4326&RESPONSE_CRS=EPSG%3A4326"
              "&BBOX=151,-34,151.5,-33.5&WIDTH=1800&HEIGHT=1800&FORMAT=GeoTIFF");
}

TEST(OnlineRequests, Wfs20PagesAndFollowsTheAuthoritysAxisOrder)
{
    OnlineLayer layer = layerOf(OnlineServiceType::Wfs, "https://wfs.example.org/ows",
                                "ns:parcels");
    EXPECT_EQ(wfsGetFeatureUrl(layer, CrsBox{144.9, -37.9, 145.0, -37.8}, "EPSG:4326", true, 1000, 500),
              "https://wfs.example.org/ows?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature"
              "&TYPENAMES=ns%3Aparcels&SRSNAME=EPSG%3A4326&BBOX=-37.9,144.9,-37.8,145,EPSG%3A4326"
              "&COUNT=500&STARTINDEX=1000");
    layer.version = "1.1.0";
    const std::string old = wfsGetFeatureUrl(layer, CrsBox{1, 2, 3, 4}, "EPSG:3111", false, 0, 50);
    EXPECT_NE(old.find("&TYPENAME=ns%3Aparcels&"), std::string::npos);
    EXPECT_NE(old.find("&MAXFEATURES=50"), std::string::npos);
    EXPECT_EQ(old.find("STARTINDEX"), std::string::npos);
}

TEST(OnlineRequests, OgcApiFeaturesAsksInCrs84AndFollowsNextLinks)
{
    const OnlineLayer layer = layerOf(OnlineServiceType::OgcApiFeatures,
                                      "https://api.example.org/ogc/", "buildings");
    EXPECT_EQ(oapifItemsUrl(layer, CrsBox{151.2, -33.9, 151.3, -33.8}, 500),
              "https://api.example.org/ogc/collections/buildings/items?"
              "bbox=151.2,-33.9,151.3,-33.8&limit=500&f=json");
    EXPECT_EQ(nextLink(R"({"links":[{"rel":"self","href":"a"},{"rel":"next","href":"b"}]})"), "b");
    EXPECT_EQ(nextLink(R"({"links":[{"rel":"self","href":"a"}]})"), "");
    // A next link that must be POSTed (a STAC search's) is not followed as a GET.
    EXPECT_EQ(nextLink(R"({"links":[{"rel":"next","href":"b","method":"POST"}]})"), "");
    EXPECT_EQ(*countGeoJsonFeatures(R"({"type":"FeatureCollection","features":[{},{}]})"), 2u);
}

// ---- tiles ------------------------------------------------------------------------------------

TEST(OnlineTiles, TheWebMercatorGridIsTheWellKnownScaleSet)
{
    const TileMatrixSet set = webMercatorTileMatrixSet(19);
    ASSERT_EQ(set.matrices.size(), 20u);
    // WMTS 1.0 annex E.4: level 0 is 156543.0339280410 m per pixel.
    EXPECT_NEAR(set.matrices[0].unitsPerPixel(1.0), 156543.0339280410, 1e-6);
    EXPECT_NEAR(set.matrices[19].unitsPerPixel(1.0), 156543.0339280410 / 524288.0, 1e-9);
    EXPECT_EQ(set.matrices[19].matrixWidth, 524288);
    EXPECT_EQ(chooseTileMatrix(set, 1.0), 18);   // 0.597 m is the first at or below 1 m
    EXPECT_EQ(chooseTileMatrix(set, 0.001), 19); // finer than the service: its finest
    EXPECT_EQ(chooseTileMatrix(set, 156543.0339280410), 0);
}

TEST(OnlineTiles, TheTilesOfAnAreaAreTheSlippyMapTiles)
{
    // The OpenStreetMap wiki's slippy map formula, worked here:
    // x = floor((lon + 180) / 360 * 2^z), y = floor((1 - asinh(tan(lat)) / pi) / 2 * 2^z).
    const auto tileX = [](double lon, int z) {
        return static_cast<std::int64_t>(std::floor((lon + 180.0) / 360.0 * std::ldexp(1.0, z)));
    };
    const auto tileY = [](double lat, int z) {
        const double r = lat * std::numbers::pi / 180.0;
        return static_cast<std::int64_t>(
            std::floor((1.0 - std::asinh(std::tan(r)) / std::numbers::pi) / 2.0 * std::ldexp(1.0, z)));
    };
    const TileMatrixSet set = webMercatorTileMatrixSet(19);
    const CrsBox lonLat{151.20, -33.875, 151.215, -33.865};
    const CrsBox box{mercatorX(lonLat.minX), mercatorY(lonLat.minY), mercatorX(lonLat.maxX),
                     mercatorY(lonLat.maxY)};
    const auto tiles = tilesCovering(set, 16, box);
    const std::int64_t x0 = tileX(151.20, 16), x1 = tileX(151.215, 16);
    const std::int64_t y0 = tileY(-33.865, 16), y1 = tileY(-33.875, 16);
    ASSERT_EQ(tiles.size(), static_cast<std::size_t>((x1 - x0 + 1) * (y1 - y0 + 1)));
    EXPECT_EQ(tiles.front().column, x0);
    EXPECT_EQ(tiles.front().row, y0);
    EXPECT_EQ(tiles.back().column, x1);
    EXPECT_EQ(tiles.back().row, y1);
    // A tile's box is where the formula puts its corner.
    const CrsBox corner = tileBox(set, tiles.front());
    EXPECT_NEAR(corner.minX, -20037508.342789244 + x0 * 40075016.68557849 / 65536.0, 1e-6);
    EXPECT_NEAR(corner.maxY, 20037508.342789244 - y0 * 40075016.68557849 / 65536.0, 1e-6);
    // Exactly one tile's box asks for exactly that tile.
    const auto exact = tilesCovering(set, 16, corner);
    ASSERT_EQ(exact.size(), 1u);
    EXPECT_EQ(exact.front().column, x0);
    EXPECT_EQ(exact.front().row, y0);
}

TEST(OnlineTiles, TemplatesFillEveryPlaceholder)
{
    const TileIndex tile{5, 29, 19};
    EXPECT_EQ(expandTileTemplate("https://t.org/{z}/{x}/{y}.png", tile, {}), "https://t.org/5/29/19.png");
    EXPECT_EQ(expandTileTemplate("https://t.org/tile/{z}/{y}/{x}", tile, {}), "https://t.org/tile/5/19/29");
    EXPECT_EQ(expandTileTemplate("https://g.org/{layer}/default/{time}/Set/{z}/{y}/{x}.jpg", tile, {},
                                 {{"layer", "MODIS_Terra"}, {"time", "2026-09-24"}}),
              "https://g.org/MODIS_Terra/default/2026-09-24/Set/5/19/29.jpg");
    EXPECT_EQ(expandTileTemplate("https://w.org/?M={TileMatrix}&R={TileRow}&C={TileCol}", tile, "L5"),
              "https://w.org/?M=L5&R=19&C=29");
}

TEST(OnlineTiles, CopernicusTilesAreNamedByTheirSouthWestCorner)
{
    const std::string pattern = "https://c.org/Copernicus_DSM_COG_10_{lat}_00_{lon}_00_DEM.tif";
    const auto one = degreeTileUrls(pattern, CrsBox{151.2, -33.875, 151.215, -33.865});
    ASSERT_EQ(one.size(), 1u);
    EXPECT_EQ(one.front(), "https://c.org/Copernicus_DSM_COG_10_S34_00_E151_00_DEM.tif");
    const auto four = degreeTileUrls(pattern, CrsBox{-0.5, -0.5, 0.5, 0.5});
    ASSERT_EQ(four.size(), 4u);
    EXPECT_EQ(four[0], "https://c.org/Copernicus_DSM_COG_10_N00_00_W001_00_DEM.tif");
    EXPECT_EQ(four[3], "https://c.org/Copernicus_DSM_COG_10_S01_00_E000_00_DEM.tif");
}

// ---- STAC ------------------------------------------------------------------------------------

TEST(OnlineStac, TheSearchBodyIsItemSearchWithTheQueryExtension)
{
    EXPECT_EQ(stacSearchBody("sentinel-2-l2a", CrsBox{151.2, -33.9, 151.3, -33.8}, "2026-01-01",
                             "2026-03-31", 20.0, 100),
              R"({"bbox":[151.2,-33.9,151.3,-33.8],"collections":["sentinel-2-l2a"],)"
              R"("datetime":"2026-01-01T00:00:00Z/2026-03-31T23:59:59Z","limit":100,)"
              R"("query":{"eo:cloud_cover":{"lt":20.0}},)"
              R"("sortby":[{"direction":"asc","field":"properties.eo:cloud_cover"}]})");
    EXPECT_NE(stacSearchBody("c", CrsBox{0, 0, 1, 1}, "", "", std::nullopt, 5).find(R"("datetime":"../..")"),
              std::string::npos);
}

TEST(OnlineStac, ARealEarthSearchItemIsRead)
{
    // stac_item_sentinel2.json: the item the sentinel-cogs bucket holds for
    // S2B_56HLH_20260103_0_L2A, trimmed to the fields Katana reads.
    std::ifstream in(std::string(KATANA_ONLINE_TEST_DATA) + "/stac_item_sentinel2.json");
    const std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    auto items = parseStacItems(text);
    ASSERT_TRUE(items.ok()) << items.error().describe();
    ASSERT_EQ(items->size(), 1u);
    const StacItem& item = items->front();
    EXPECT_EQ(item.id, "S2B_56HLH_20260103_0_L2A");
    EXPECT_EQ(item.datetime.substr(0, 10), "2026-01-03");
    ASSERT_TRUE(item.cloudCover.has_value());
    EXPECT_NEAR(*item.cloudCover, 93.501586, 1e-6);
    EXPECT_NEAR(item.bounds.minX, 150.82399, 1e-9);
    EXPECT_NEAR(item.bounds.maxY, -33.420387, 1e-9);
    EXPECT_TRUE(item.assets.at("visual").ends_with("/TCI.tif"));
    EXPECT_TRUE(item.assets.contains("red"));
}

TEST(OnlineStac, TheChosenDayCoversTheAreaWithTheLeastCloud)
{
    std::ifstream in(std::string(KATANA_ONLINE_TEST_DATA) + "/stac_search.json");
    const std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    auto items = parseStacItems(text);
    ASSERT_TRUE(items.ok()) << items.error().describe();
    ASSERT_EQ(items->size(), 5u);
    // 2026-01-03 covers the area in two halves at 12 % at worst; 2026-01-08
    // covers it in one at 5 %; 2026-01-13 is clearer but covers only the east.
    const StacChoice choice = chooseStacItems(*items, CrsBox{151.0, -34.0, 151.4, -33.6});
    EXPECT_EQ(choice.day, "2026-01-08");
    EXPECT_TRUE(choice.coversArea);
    ASSERT_EQ(choice.items.size(), 1u);
    EXPECT_EQ(choice.items.front().id, "S2B_FULL_20260108");
    // A corner the 8th and the 13th both cover: the clearer, later 13th.
    const StacChoice east = chooseStacItems(*items, CrsBox{151.45, -34.05, 151.55, -33.95});
    EXPECT_TRUE(east.coversArea);
    EXPECT_EQ(east.day, "2026-01-13"); // it covers this corner, with 1 %
    EXPECT_TRUE(chooseStacItems(*items, CrsBox{10, 10, 11, 11}).items.empty());
    EXPECT_EQ(httpHref("s3://sentinel-cogs/a/b.tif"), "https://sentinel-cogs.s3.amazonaws.com/a/b.tif");
}

// ---- Overpass --------------------------------------------------------------------------------

TEST(OnlineOverpass, TheQueryIsOneStatementWithItsLimits)
{
    auto query = overpassQuery("building", CrsBox{151.2, -33.875, 151.215, -33.865}, 90, 1000000);
    ASSERT_TRUE(query.ok()) << query.error().describe();
    EXPECT_EQ(*query,
              "[out:xml][timeout:90][maxsize:1000000][bbox:-33.875,151.2,-33.865,151.215];\n"
              "(\n  node[\"building\"];\n  way[\"building\"];\n  relation[\"building\"];\n);\n"
              "(._;>;);\nout body;\n");
    auto two = overpassQuery("highway=primary; name", CrsBox{0, 0, 1, 1}, 25, 5);
    ASSERT_TRUE(two.ok());
    EXPECT_NE(two->find("way[\"highway\"=\"primary\"][\"name\"];"), std::string::npos);
    auto negated = overpassQuery("building!=no", CrsBox{0, 0, 1, 1}, 25, 5);
    ASSERT_TRUE(negated.ok());
    EXPECT_NE(negated->find("[\"building\"!=\"no\"]"), std::string::npos);
}

TEST(OnlineOverpass, NothingTypedCanBecomeASecondStatement)
{
    for (const char* hostile : {"building\"];out;", "a[b]", "x{y}", "name=\"x\"", "a\\b", "", ";"}) {
        const auto refused = overpassQuery(hostile, CrsBox{0, 0, 1, 1}, 25, 5);
        ASSERT_FALSE(refused.ok()) << hostile;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument) << hostile;
    }
}

TEST(OnlineOverpass, AreaIsOnTheSphere)
{
    // One degree square on the equator: R^2 (pi/180) sin(1 deg) = 12 363.7 km2,
    // worked from the spherical zone formula with R = 6371 km.
    EXPECT_NEAR(areaKm2(CrsBox{0, 0, 1, 1}),
                6371.0 * 6371.0 * (std::numbers::pi / 180.0) * std::sin(std::numbers::pi / 180.0), 1e-6);
    // The Sydney CBD box of the live checks: 0.015 degrees of longitude at
    // 33.87 S (1.386 km) by 0.01 degrees of latitude (1.112 km), 1.54 km2.
    EXPECT_NEAR(areaKm2(CrsBox{151.2, -33.875, 151.215, -33.865}), 1.386 * 1.112, 0.01);
}

// ---- CKAN ---------------------------------------------------------------------------------------

TEST(OnlineCkan, TheSearchFindsWebServicesOnly)
{
    EXPECT_EQ(ckanSearchUrl("https://data.gov.au/data/api/3/action/package_search", "nsw cadastre", 50),
              "https://data.gov.au/data/api/3/action/package_search?q=nsw%20cadastre&fq=res_format%3A%28WMS"
              "%20OR%20WFS%20OR%20WMTS%20OR%20WCS%20OR%20%22ESRI%20REST%22%20OR%20%22OGC%20API%22%29&rows=50");
    std::ifstream in(std::string(KATANA_ONLINE_TEST_DATA) + "/ckan_search.json");
    const std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    auto found = parseCkanSearch(text);
    ASSERT_TRUE(found.ok()) << found.error().describe();
    ASSERT_EQ(found->size(), 2u); // the ZIP download is not a service
    EXPECT_EQ((*found)[0].format, "WMS");
    EXPECT_EQ((*found)[1].format, "ESRI REST");
    EXPECT_EQ((*found)[1].licence, "Creative Commons Attribution 4.0");
}

TEST(OnlineRequests, NumbersAreShortestExact)
{
    EXPECT_EQ(formatNumber(151.2), "151.2");
    EXPECT_EQ(formatNumber(-33.875), "-33.875");
    EXPECT_EQ(formatNumber(16830000.0), "16830000");
    EXPECT_EQ(std::stod(formatNumber(0.1 + 0.2)), 0.1 + 0.2);
    EXPECT_EQ(withQuery("https://a/b", "x=1"), "https://a/b?x=1");
    EXPECT_EQ(withQuery("https://a/b?", "x=1"), "https://a/b?x=1");
    EXPECT_EQ(withQuery("https://a/b?k=v", "x=1"), "https://a/b?k=v&x=1");
}
