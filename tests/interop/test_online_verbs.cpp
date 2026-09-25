// The ONLINE verbs' grammar and replies (docs/gis_online.md, "Verbs"): what an
// agent types and what it reads back. The replies are for machines, so they
// are tested as records - a word, then key=value fields that split on spaces
// outside quotes - and never as a snapshot of prose.

#include <gtest/gtest.h>

#include <map>
#include <string>
#include <vector>

#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_verbs.hpp"

using namespace katana::interop;
using katana::core::ErrorCode;

namespace {

// One reply line read back as a machine would: the record's word and its
// fields, quotes and escapes undone.
struct Record {
    std::string word;
    std::map<std::string, std::string> fields;
};

Record readRecord(const std::string& line)
{
    Record record;
    std::size_t at = line.find(' ');
    record.word = line.substr(0, at);
    while (at != std::string::npos && at < line.size()) {
        ++at;
        const std::size_t equals = line.find('=', at);
        if (equals == std::string::npos) {
            break;
        }
        const std::string key = line.substr(at, equals - at);
        std::string value;
        at = equals + 1;
        if (at < line.size() && line[at] == '"') {
            ++at;
            while (at < line.size() && line[at] != '"') {
                if (line[at] == '\\' && at + 1 < line.size()) {
                    ++at;
                }
                value.push_back(line[at++]);
            }
            ++at; // the closing quote
        } else {
            const std::size_t end = line.find(' ', at);
            value = line.substr(at, end == std::string::npos ? std::string::npos : end - at);
            at = end == std::string::npos ? line.size() : end;
        }
        record.fields[key] = value;
    }
    return record;
}

std::vector<Record> readRecords(const std::string& text)
{
    std::vector<Record> records;
    std::size_t start = 0;
    while (start < text.size()) {
        std::size_t end = text.find('\n', start);
        if (end == std::string::npos) {
            end = text.size();
        }
        records.push_back(readRecord(text.substr(start, end - start)));
        start = end + 1;
    }
    return records;
}

OnlineCatalogue builtIn()
{
    auto catalogue = builtInCatalogue();
    EXPECT_TRUE(catalogue.ok());
    return *catalogue;
}

} // namespace

TEST(OnlineVerbs, EachVerbParses)
{
    auto providers = parseOnlineCommand("online providers nsw   cadastre");
    ASSERT_TRUE(providers.ok());
    EXPECT_EQ(providers->verb, OnlineVerb::Providers);
    EXPECT_EQ(providers->filter, "nsw cadastre");
    auto layers = parseOnlineCommand("ONLINE LAYERS data-gov-au flood maps");
    ASSERT_TRUE(layers.ok());
    EXPECT_EQ(layers->provider, "data-gov-au");
    EXPECT_EQ(layers->filter, "flood maps");
    auto info = parseOnlineCommand("ONLINE INFO nsw-spatial lots");
    ASSERT_TRUE(info.ok());
    EXPECT_EQ(info->verb, OnlineVerb::Info);
    EXPECT_EQ(info->layer, "lots");
    auto custom = parseOnlineCommand("ONLINE CUSTOM https://example.org/wms?service=WMS");
    ASSERT_TRUE(custom.ok());
    EXPECT_EQ(custom->url, "https://example.org/wms?service=WMS");
    auto key = parseOnlineCommand("ONLINE KEY maptiler \"a b\"");
    ASSERT_TRUE(key.ok());
    EXPECT_EQ(key->keyName, "maptiler");
    EXPECT_EQ(key->keyValue, "a b");
    auto removal = parseOnlineCommand("ONLINE KEY maptiler");
    ASSERT_TRUE(removal.ok());
    EXPECT_TRUE(removal->keyValue.empty());
}

TEST(OnlineVerbs, ImportTakesEveryOption)
{
    auto command = parseOnlineCommand(
        "ONLINE IMPORT sentinel2 truecolour area=lonlat:151.215,-33.865,151.2,-33.875 res=10 "
        "layer=\"imagery/sentinel 2\" from=2026-01-01 to=2026-03-31 cloud=15 time=2026-02-01 "
        "crs=EPSG:7856 timeout=120 tag=building");
    ASSERT_TRUE(command.ok()) << command.error().describe();
    EXPECT_EQ(command->verb, OnlineVerb::Import);
    EXPECT_EQ(command->provider, "sentinel2");
    EXPECT_EQ(command->layer, "truecolour");
    EXPECT_EQ(command->area, OnlineAreaKind::Box);
    EXPECT_TRUE(command->boxIsLonLat);
    EXPECT_DOUBLE_EQ(command->box.minX, 151.2);
    EXPECT_DOUBLE_EQ(command->box.maxY, -33.865);
    EXPECT_EQ(command->resolution, 10.0);
    EXPECT_EQ(command->targetLayer, "imagery/sentinel 2");
    EXPECT_EQ(command->fromDate, "2026-01-01");
    EXPECT_EQ(command->toDate, "2026-03-31");
    EXPECT_EQ(command->maxCloud, 15.0);
    EXPECT_EQ(command->time, "2026-02-01");
    EXPECT_EQ(command->crs, "EPSG:7856");
    EXPECT_EQ(command->timeoutSeconds, 120);
    EXPECT_EQ(command->tag, "building");
    for (const auto& [text, kind] : std::vector<std::pair<std::string, OnlineAreaKind>>{
             {"view", OnlineAreaKind::View},
             {"drawing", OnlineAreaKind::Drawing},
             {"selection", OnlineAreaKind::Selection},
             {"334000,6250000,335000,6251000", OnlineAreaKind::Box}}) {
        auto parsed = parseOnlineCommand("ONLINE IMPORT osm buildings area=" + text);
        ASSERT_TRUE(parsed.ok()) << text;
        EXPECT_EQ(parsed->area, kind) << text;
        EXPECT_FALSE(parsed->boxIsLonLat);
    }
}

TEST(OnlineVerbs, MalformedLinesAreRefusedWithTheUsage)
{
    for (const char* bad :
         {"ONLINE", "ONLINE FETCH x y", "ONLINE LAYERS", "ONLINE INFO nsw-spatial",
          "ONLINE IMPORT nsw-spatial lots", "ONLINE IMPORT nsw-spatial area=view",
          "ONLINE IMPORT a b area=1,2,3", "ONLINE IMPORT a b area=1,1,1,5",
          "ONLINE IMPORT a b area=lonlat:190,0,191,1", "ONLINE IMPORT a b area=view res=0",
          "ONLINE IMPORT a b area=view res=-2", "ONLINE IMPORT a b area=view from=2026-13-01",
          "ONLINE IMPORT a b area=view from=26-01-01", "ONLINE IMPORT a b area=view cloud=101",
          "ONLINE IMPORT a b area=view colour=red", "ONLINE IMPORT a b area=view layer=",
          "ONLINE IMPORT a b area=view from=2026-03-01 to=2026-01-01",
          "ONLINE IMPORT a b area=view timeout=0", "ONLINE KEY", "ONLINE KEY a b c",
          "ONLINE CUSTOM", "ONLINE IMPORT a b area=view layer=\"unclosed"}) {
        const auto refused = parseOnlineCommand(bad);
        ASSERT_FALSE(refused.ok()) << bad;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument) << bad;
    }
    const auto missingArea = parseOnlineCommand("ONLINE IMPORT nsw-spatial lots");
    EXPECT_NE(missingArea.error().context.find("usage: ONLINE IMPORT"), std::string::npos);
}

TEST(OnlineVerbs, RepliesAreRecordsAMachineCanSplit)
{
    const OnlineCatalogue catalogue = builtIn();
    const auto providers = readRecords(formatProviders(catalogue.filter("nsw")));
    ASSERT_EQ(providers.size(), 2u);
    EXPECT_EQ(providers[0].word, "providers");
    EXPECT_EQ(providers[0].fields.at("count"), "1");
    EXPECT_EQ(providers[1].word, "provider");
    EXPECT_EQ(providers[1].fields.at("id"), "nsw-spatial");
    EXPECT_EQ(providers[1].fields.at("group"), "Australia/NSW");
    EXPECT_EQ(providers[1].fields.at("title"), "NSW Spatial Services");

    const auto layers = readRecords(formatLayers(*catalogue.findProvider("osm")));
    ASSERT_GE(layers.size(), 3u);
    EXPECT_EQ(layers[1].word, "layer");
    EXPECT_EQ(layers[1].fields.at("provider"), "osm");
    EXPECT_EQ(layers[1].fields.at("id"), "buildings");
    EXPECT_EQ(layers[1].fields.at("kind"), "vector");
    EXPECT_EQ(layers[1].fields.at("service"), "overpass");
    EXPECT_EQ(layers[1].fields.at("licence"), "ODbL 1.0");

    const OnlineLayer* dem = catalogue.findProvider("copernicus")->findLayer("dem");
    std::map<std::string, std::string> info;
    for (const Record& record : readRecords(formatInfo(*dem))) {
        if (record.word == "field") {
            info[record.fields.at("name")] = record.fields.at("value");
        }
    }
    EXPECT_EQ(info.at("kind"), "elevation");
    EXPECT_EQ(info.at("service"), "cog");
    EXPECT_EQ(info.at("coverage"), "-180,-90,180,90");
    EXPECT_EQ(info.at("verified"), "2026-09-25");
    EXPECT_NE(info.at("attribution").find("Airbus"), std::string::npos);
}

TEST(OnlineVerbs, AKeyInAnEndpointIsNeverInAReply)
{
    OnlineLayer layer;
    layer.providerId = "p";
    layer.id = "l";
    layer.endpoint = "https://tiles.example.org/{z}/{x}/{y}.png?key=SECRET";
    EXPECT_EQ(formatInfo(layer).find("SECRET"), std::string::npos);
}

TEST(OnlineVerbs, AnImportReplySaysWhatLandedAndOnWhatTerms)
{
    OnlineLayer layer;
    layer.providerId = "nsw-spatial";
    layer.id = "lots";
    OnlineImport result;
    result.kind = OnlineLayerKind::Vector;
    result.stats.requests = 3;
    result.stats.duplicates = 1;
    result.licence = "CC BY 4.0";
    result.attribution = "© State of New South Wales";
    result.sourceUrl = "https://x/y";
    result.retrieved = "2026-09-25";
    result.warnings = {"the service repeated a page"};
    const auto records = readRecords(formatImport(layer, result, 10, "online/nsw-spatial/lots"));
    ASSERT_EQ(records.size(), 2u);
    EXPECT_EQ(records[0].word, "imported");
    EXPECT_EQ(records[0].fields.at("entities"), "10");
    EXPECT_EQ(records[0].fields.at("target"), "online/nsw-spatial/lots");
    EXPECT_EQ(records[0].fields.at("requests"), "3");
    EXPECT_EQ(records[0].fields.at("duplicates"), "1");
    EXPECT_EQ(records[0].fields.at("attribution"), "© State of New South Wales");
    EXPECT_EQ(records[1].word, "warning");
    EXPECT_EQ(records[1].fields.at("text"), "the service repeated a page");

    const auto error = readRecords(formatError(
        OnlineVerb::Import, katana::core::Error{ErrorCode::InvalidArgument, "say \"why\"", "ctx"}));
    EXPECT_EQ(error[0].word, "error");
    EXPECT_EQ(error[0].fields.at("verb"), "IMPORT");
    EXPECT_EQ(error[0].fields.at("code"), "InvalidArgument");
    EXPECT_EQ(error[0].fields.at("message"), "say \"why\"");
    EXPECT_EQ(replyValue("plain"), "plain");
    EXPECT_EQ(replyValue(""), "\"\"");
    EXPECT_EQ(replyValue("a=b"), "\"a=b\"");
}
