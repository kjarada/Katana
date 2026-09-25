// The online provider catalogue (docs/gis_online.md, "The catalogue").
//
// The built-in catalogue is data compiled into the program, so these tests
// are what stops a bad edit to resources/online/online_sources.json from
// shipping: every layer must parse into a complete description - a licence
// and an attribution to record, a well-formed https endpoint whose template
// has the placeholders its type needs, a valid longitude/latitude coverage -
// and the ids the ONLINE verbs name must be unique and typeable. No network.

#include <gtest/gtest.h>

#include <algorithm>
#include <set>
#include <string>

#include "katana/interop/online_catalogue.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;

namespace {

OnlineCatalogue builtIn()
{
    auto catalogue = builtInCatalogue();
    EXPECT_TRUE(catalogue.ok()) << (catalogue ? "" : catalogue.error().describe());
    return catalogue ? *catalogue : OnlineCatalogue{};
}

} // namespace

TEST(OnlineCatalogue, TheBuiltInCatalogueParsesAndPassesItsOwnValidation)
{
    const OnlineCatalogue catalogue = builtIn();
    ASSERT_FALSE(catalogue.providers.empty());
    const std::vector<std::string> problems = validateCatalogue(catalogue);
    for (const std::string& problem : problems) {
        ADD_FAILURE() << problem;
    }
    EXPECT_TRUE(problems.empty());
}

TEST(OnlineCatalogue, EveryLayerCarriesWhatARequestAndARecordNeed)
{
    for (const OnlineProvider& provider : builtIn().providers) {
        for (const OnlineLayer* layer : provider.layers()) {
            const std::string at = provider.id + "/" + layer->id;
            EXPECT_FALSE(layer->title.empty()) << at;
            EXPECT_FALSE(layer->licence.empty()) << at;
            EXPECT_FALSE(layer->attribution.empty()) << at;
            EXPECT_TRUE(layer->endpoint.starts_with("https://")) << at << " " << layer->endpoint;
            EXPECT_FALSE(layer->group.empty()) << at;
            EXPECT_EQ(layer->providerId, provider.id) << at;
            // Something says where the address and terms were checked: the
            // date it answered, or the documentation it was taken from.
            EXPECT_TRUE(!layer->verified.empty() || !layer->evidence.empty()) << at;
            EXPECT_LT(layer->coverage[0], layer->coverage[2]) << at;
            EXPECT_LT(layer->coverage[1], layer->coverage[3]) << at;
        }
    }
}

TEST(OnlineCatalogue, TheGroupsAreAustraliaByStateAndGlobal)
{
    std::set<std::string> groups;
    for (const OnlineProvider& provider : builtIn().providers) {
        groups.insert(provider.group);
        EXPECT_TRUE(provider.group == "Global" || provider.group.starts_with("Australia/"))
            << provider.id << " is in " << provider.group;
    }
    for (const char* wanted : {"Australia/NSW", "Australia/QLD", "Australia/National", "Global"}) {
        EXPECT_TRUE(groups.contains(wanted)) << wanted;
    }
}

TEST(OnlineCatalogue, ItOffersTheLayersTheOwnerAskedFor)
{
    const OnlineCatalogue catalogue = builtIn();
    const auto has = [&](const char* provider, const char* layer, OnlineLayerKind kind,
                         OnlineServiceType type) {
        const OnlineProvider* found = catalogue.findProvider(provider);
        ASSERT_NE(found, nullptr) << provider;
        const OnlineLayer* entry = found->findLayer(layer);
        ASSERT_NE(entry, nullptr) << provider << "/" << layer;
        EXPECT_EQ(entry->kind, kind) << provider << "/" << layer;
        EXPECT_EQ(entry->type, type) << provider << "/" << layer;
    };
    has("nsw-spatial", "imagery", OnlineLayerKind::Imagery, OnlineServiceType::XyzTiles);
    has("nsw-spatial", "lots", OnlineLayerKind::Vector, OnlineServiceType::ArcgisQuery);
    has("nsw-spatial", "roads", OnlineLayerKind::Vector, OnlineServiceType::ArcgisQuery);
    has("nsw-spatial", "dem5m", OnlineLayerKind::Elevation, OnlineServiceType::ArcgisExportImage);
    has("osm", "buildings", OnlineLayerKind::Vector, OnlineServiceType::Overpass);
    has("osm", "standard", OnlineLayerKind::Imagery, OnlineServiceType::XyzTiles);
    has("copernicus", "dem", OnlineLayerKind::Elevation, OnlineServiceType::Cog);
    has("sentinel2", "truecolour", OnlineLayerKind::Imagery, OnlineServiceType::Stac);
    has("nasa-gibs", "modis-terra", OnlineLayerKind::Imagery, OnlineServiceType::Wmts);
    has("natural-earth", "countries", OnlineLayerKind::Vector, OnlineServiceType::File);
    has("ga", "dem-h", OnlineLayerKind::Elevation, OnlineServiceType::Wcs);
    has("dea", "geomad", OnlineLayerKind::Imagery, OnlineServiceType::Wms);
}

TEST(OnlineCatalogue, TheRoadLayerIsCalledRoads)
{
    const OnlineCatalogue catalogue = builtIn();
    const OnlineLayer* roads = catalogue.findProvider("nsw-spatial")->findLayer("roads");
    ASSERT_NE(roads, nullptr);
    EXPECT_EQ(roads->title, "Roads");
}

TEST(OnlineCatalogue, TheOpenStreetMapTilePolicyIsInTheData)
{
    // operations.osmfoundation.org/policies/tiles: small areas only, cache
    // for at least seven days, attribution "© OpenStreetMap contributors".
    const OnlineCatalogue catalogue = builtIn();
    const OnlineLayer* tiles = catalogue.findProvider("osm")->findLayer("standard");
    ASSERT_NE(tiles, nullptr);
    EXPECT_LE(tiles->maxTiles, 64);
    EXPECT_GE(tiles->cacheDays, 7);
    EXPECT_EQ(tiles->attribution, "© OpenStreetMap contributors");
    EXPECT_EQ(tiles->licence, "ODbL 1.0");
}

TEST(OnlineCatalogue, InheritanceFillsALayerFromItsServiceAndProvider)
{
    const char* json = R"({"providers": [{
        "id": "p", "title": "Provider", "group": "Global", "licence": "CC BY 4.0",
        "attribution": "© Provider", "coverage": [100, -40, 160, -10],
        "services": [{"id": "s", "title": "S", "type": "arcgis-query",
                      "endpoint": "https://example.org/arcgis/rest/services/X/MapServer",
                      "pageSize": 500, "crs": "EPSG:3857",
                      "layers": [{"id": "a", "title": "A", "kind": "vector", "layer": "3"},
                                 {"id": "b", "title": "B", "kind": "vector", "layer": "4",
                                  "pageSize": 50, "licence": "CC BY 3.0 AU"}]}]}]})";
    auto catalogue = parseCatalogue(json);
    ASSERT_TRUE(catalogue.ok()) << catalogue.error().describe();
    const OnlineProvider& provider = catalogue->providers.front();
    const OnlineLayer* a = provider.findLayer("a");
    const OnlineLayer* b = provider.findLayer("B"); // ids match without regard to case
    ASSERT_NE(a, nullptr);
    ASSERT_NE(b, nullptr);
    EXPECT_EQ(a->pageSize, 500);
    EXPECT_EQ(a->licence, "CC BY 4.0");
    EXPECT_EQ(a->attribution, "© Provider");
    EXPECT_EQ(a->crs, "EPSG:3857");
    EXPECT_EQ(a->coverage, (LonLatBox{100, -40, 160, -10}));
    EXPECT_EQ(a->endpoint, "https://example.org/arcgis/rest/services/X/MapServer");
    EXPECT_EQ(b->pageSize, 50);
    EXPECT_EQ(b->licence, "CC BY 3.0 AU");
    EXPECT_EQ(b->layerName, "4");
    EXPECT_TRUE(validateCatalogue(*catalogue).empty());
}

TEST(OnlineCatalogue, ValidationNamesEachProblem)
{
    const char* json = R"({"providers": [{
        "id": "bad", "title": "Bad", "group": "Global",
        "services": [
          {"id": "t", "title": "T", "type": "xyz", "endpoint": "ftp://tiles/{z}/{x}.png",
           "layers": [{"id": "one", "title": "One", "kind": "imagery"}]},
          {"id": "q", "title": "Q", "type": "arcgis-query", "endpoint": "https://example.org/x",
           "coverage": [10, 10, 5, 5], "licence": "L", "attribution": "A",
           "layers": [{"id": "one", "title": "Again", "kind": "vector"},
                      {"id": "has space", "title": "S", "kind": "imagery", "layer": "1"}]}]}]})";
    auto catalogue = parseCatalogue(json);
    ASSERT_TRUE(catalogue.ok()) << catalogue.error().describe();
    const std::vector<std::string> problems = validateCatalogue(*catalogue);
    const auto mentions = [&](const std::string& text) {
        return std::any_of(problems.begin(), problems.end(),
                           [&](const std::string& p) { return p.find(text) != std::string::npos; });
    };
    EXPECT_TRUE(mentions("bad/one: no licence"));
    EXPECT_TRUE(mentions("bad/one: no attribution"));
    EXPECT_TRUE(mentions("not an http or https URL"));
    EXPECT_TRUE(mentions("has no {y}"));
    EXPECT_TRUE(mentions("crs must be EPSG:3857"));
    EXPECT_TRUE(mentions("coverage is not a valid"));
    EXPECT_TRUE(mentions("used twice"));
    EXPECT_TRUE(mentions("needs the layer id"));
    EXPECT_TRUE(mentions("may not hold spaces"));
    EXPECT_TRUE(mentions("cannot come from a arcgis-query service"));
}

TEST(OnlineCatalogue, ADocumentThatIsNotACatalogueIsRefusedWithWhere)
{
    EXPECT_EQ(parseCatalogue("{").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseCatalogue("{\"version\": 1}").error().code, ErrorCode::ParseFailure);
    const auto unknown = parseCatalogue(R"({"providers": [{"id": "p", "services": [
        {"id": "s", "type": "gopher", "endpoint": "https://x", "layers": [{"id": "l", "kind": "vector"}]}]}]})");
    ASSERT_FALSE(unknown.ok());
    EXPECT_NE(unknown.error().message.find("gopher"), std::string::npos);
    EXPECT_EQ(unknown.error().context, "p/s");
}

TEST(OnlineCatalogue, AUserCatalogueReplacesAProviderByIdAndAddsNewOnes)
{
    const OnlineCatalogue base = builtIn();
    auto user = parseCatalogue(R"({"providers": [
        {"id": "OSM", "title": "My OSM mirror", "group": "Global", "licence": "ODbL 1.0",
         "attribution": "© OpenStreetMap contributors",
         "services": [{"id": "t", "title": "T", "type": "xyz", "crs": "EPSG:3857",
                       "endpoint": "https://tiles.example.org/{z}/{x}/{y}.png",
                       "layers": [{"id": "standard", "title": "Mirror", "kind": "imagery"}]}]},
        {"id": "mine", "title": "Mine", "group": "Custom", "licence": "L", "attribution": "A",
         "services": [{"id": "c", "title": "C", "type": "cog",
                       "endpoint": "https://example.org/dem.tif",
                       "layers": [{"id": "dem", "title": "DEM", "kind": "elevation"}]}]}]})",
                               true);
    ASSERT_TRUE(user.ok()) << user.error().describe();
    const OnlineCatalogue merged = mergeCatalogues(base, *user);
    EXPECT_EQ(merged.providers.size(), base.providers.size() + 1);
    const OnlineProvider* osm = merged.findProvider("osm");
    ASSERT_NE(osm, nullptr);
    EXPECT_EQ(osm->title, "My OSM mirror");
    EXPECT_TRUE(osm->userDefined);
    // Replaced in place: the tree keeps its order.
    const auto position = [&](const OnlineCatalogue& c, const char* id) {
        for (std::size_t i = 0; i < c.providers.size(); ++i) {
            if (c.providers[i].id == id || c.providers[i].id == "OSM") {
                return i;
            }
        }
        return c.providers.size();
    };
    EXPECT_EQ(position(merged, "osm"), position(base, "osm"));
    EXPECT_NE(merged.findProvider("mine"), nullptr);
}

TEST(OnlineCatalogue, AProviderWrittenAsJsonReadsBackToTheSameLayers)
{
    const OnlineCatalogue base = builtIn();
    for (const char* id : {"nsw-spatial", "sentinel2", "nasa-gibs"}) {
        const OnlineProvider* provider = base.findProvider(id);
        ASSERT_NE(provider, nullptr);
        auto again = parseCatalogue(providerToJson(*provider));
        ASSERT_TRUE(again.ok()) << again.error().describe();
        ASSERT_EQ(again->providers.size(), 1u);
        const auto before = provider->layers();
        const auto after = again->providers.front().layers();
        ASSERT_EQ(before.size(), after.size()) << id;
        for (std::size_t i = 0; i < before.size(); ++i) {
            EXPECT_EQ(before[i]->id, after[i]->id);
            EXPECT_EQ(before[i]->endpoint, after[i]->endpoint);
            EXPECT_EQ(before[i]->type, after[i]->type);
            EXPECT_EQ(before[i]->kind, after[i]->kind);
            EXPECT_EQ(before[i]->layerName, after[i]->layerName);
            EXPECT_EQ(before[i]->licence, after[i]->licence);
            EXPECT_EQ(before[i]->attribution, after[i]->attribution);
            EXPECT_EQ(before[i]->coverage, after[i]->coverage);
            EXPECT_EQ(before[i]->crs, after[i]->crs);
            EXPECT_EQ(before[i]->asset, after[i]->asset);
        }
    }
}

TEST(OnlineCatalogue, FilterMatchesEveryWordAgainstProvidersAndTheirLayers)
{
    const OnlineCatalogue catalogue = builtIn();
    EXPECT_EQ(catalogue.filter("").size(), catalogue.providers.size());
    const auto nsw = catalogue.filter("nsw lots");
    ASSERT_EQ(nsw.size(), 1u);
    EXPECT_EQ(nsw.front()->id, "nsw-spatial");
    const auto elevation = catalogue.filter("ELEVATION");
    EXPECT_GE(elevation.size(), 3u); // NSW, GA, Copernicus at least
    EXPECT_TRUE(catalogue.filter("no-such-thing-anywhere").empty());
}

TEST(OnlineCatalogue, EnumNamesRoundTrip)
{
    for (const OnlineServiceType type :
         {OnlineServiceType::ArcgisExport, OnlineServiceType::ArcgisExportImage,
          OnlineServiceType::ArcgisQuery, OnlineServiceType::Wms, OnlineServiceType::Wmts,
          OnlineServiceType::Wcs, OnlineServiceType::Wfs, OnlineServiceType::OgcApiFeatures,
          OnlineServiceType::XyzTiles, OnlineServiceType::Stac, OnlineServiceType::Cog,
          OnlineServiceType::Overpass, OnlineServiceType::File, OnlineServiceType::Ckan}) {
        EXPECT_EQ(serviceTypeFromString(toString(type)), type) << toString(type);
    }
    for (const OnlineLayerKind kind : {OnlineLayerKind::Imagery, OnlineLayerKind::Elevation,
                                       OnlineLayerKind::Vector, OnlineLayerKind::Catalogue}) {
        EXPECT_EQ(layerKindFromString(toString(kind)), kind);
    }
}
