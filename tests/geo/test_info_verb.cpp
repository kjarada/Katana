// INFO as data (src/katana_app/geo/info_verb.cpp; docs/interop.md, "Dataset
// information"): what a file, a folder or a /vsi path holds, as records read
// from GDAL's own info JSON, the JSON itself, statistics computed without a
// sidecar, and GDAL's check - through the executor, as every front end runs
// it.
//
// The facts, by hand:
//   samples/gis/terrain.asc  ncols 120, nrows 90, cellsize 1.5, lower-left
//     corner (-5, -5), NODATA_value -9999 (its header); its values are read
//     here, straight from the text, for the statistics.
//   tests/geo/data/lots.geojson  one layer "lots": three polygons, fields
//     "kind" and "name" (strings), extent (-10, 0) to (110, 40), EPSG:28356.
//   samples/gis holds parcels.geojson, terrain.asc (with terrain.prj) and
//     survey_scan.las.

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <limits>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/zip_container.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

namespace geo = katana::app::geo;
using katana::core::ErrorCode;

const std::string kSamples = KATANA_GIS_SAMPLES;
const std::string kData = KATANA_GEO_TEST_DATA;

std::string quoted(const std::string& path)
{
    return "\"" + path + "\"";
}

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana info verb " + name))
    {
        std::error_code error;
        std::filesystem::remove_all(path_, error);
        std::filesystem::create_directories(path_, error);
    }
    ~TempDir()
    {
        std::error_code error;
        std::filesystem::remove_all(path_, error);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;
    [[nodiscard]] const std::filesystem::path& path() const { return path_; }
    [[nodiscard]] std::string file(const std::string& name) const
    {
        return (path_ / name).generic_string();
    }

  private:
    std::filesystem::path path_;
};

std::vector<geo::Record> recordsOf(const std::string& reply, const std::string& kind)
{
    std::vector<geo::Record> found;
    for (geo::Record& record : geo::parseRecords(reply)) {
        if (record.kind == kind) {
            found.push_back(std::move(record));
        }
    }
    return found;
}

std::string field(const geo::Record& record, const std::string& key)
{
    return record.get(key).value_or("<absent>");
}

double numberIn(const geo::Record& record, const std::string& key)
{
    const auto value = katana::core::parseFiniteDouble(record.get(key).value_or(""));
    return value.value_or(std::numeric_limits<double>::quiet_NaN());
}

class InfoVerb : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch.path(), {}, {}};

    katana::core::Result<std::string> run(const std::string& line)
    {
        return geo::runNow(context, line);
    }
};

// The sample terrain's values, read straight from the text grid - an
// independent reader - leaving out the no-data value its header declares.
std::vector<double> terrainValues()
{
    std::ifstream in(kSamples + "/terrain.asc");
    std::string word;
    double noData = 0.0;
    for (int header = 0; header < 6; ++header) {
        std::string key;
        in >> key >> word;
        if (katana::core::lowered(key) == "nodata_value") {
            noData = *katana::core::parseFiniteDouble(word);
        }
    }
    std::vector<double> values;
    while (in >> word) {
        const double value = *katana::core::parseFiniteDouble(word);
        if (value != noData) {
            values.push_back(value);
        }
    }
    return values;
}

TEST_F(InfoVerb, InfoOfLotsGeojsonListsItsLayerAndFields)
{
    auto reply = run("INFO " + quoted(kData + "/lots.geojson"));
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto dataset = recordsOf(*reply, "dataset");
    ASSERT_EQ(dataset.size(), 1u) << *reply;
    EXPECT_EQ(field(dataset[0], "kind"), "vector");
    EXPECT_EQ(field(dataset[0], "driver"), "GeoJSON");
    EXPECT_NE(field(dataset[0], "crs").find("EPSG:28356"), std::string::npos);
    const auto layers = recordsOf(*reply, "layer");
    ASSERT_EQ(layers.size(), 1u);
    EXPECT_EQ(field(layers[0], "name"), "lots");
    EXPECT_EQ(field(layers[0], "features"), "3");
    EXPECT_EQ(field(layers[0], "geometry"), "Polygon");
    EXPECT_EQ(field(layers[0], "bounds"), "-10,0,110,40");
    EXPECT_EQ(field(layers[0], "fields"), "2");
    const auto fields = recordsOf(*reply, "field");
    ASSERT_EQ(fields.size(), 2u);
    EXPECT_EQ(field(fields[0], "layer"), "lots");
    EXPECT_EQ(field(fields[0], "name"), "kind");
    EXPECT_EQ(field(fields[0], "type"), "String");
    EXPECT_EQ(field(fields[1], "name"), "name");
    EXPECT_TRUE(recordsOf(*reply, "band").empty());
}

TEST_F(InfoVerb, InfoOfTypedGeojsonListsEachFieldAsGdalTypesIt)
{
    // GDAL's GeoJSON driver types a field from its values: a whole number
    // Integer, a fraction Real, true and false Integer of subtype Boolean,
    // and an ISO 8601 date Date (the driver's documentation, "Field
    // types"; DATE_AS_STRING is not set).
    const std::string file = scratch.file("typed.geojson");
    std::ofstream(file) << R"({"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"name":"P1","count":3,"area":12.5,"surveyed":"2026-09-26","checked":true},
         "geometry":{"type":"Point","coordinates":[1,2]}}]})";
    auto reply = run("INFO " + quoted(file));
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    std::map<std::string, std::string> types;
    for (const geo::Record& record : recordsOf(*reply, "field")) {
        const std::string subtype = field(record, "subtype");
        types[field(record, "name")] = field(record, "type") + (subtype.empty() ? "" : "/" + subtype);
    }
    EXPECT_EQ(types["name"], "String");
    EXPECT_EQ(types["count"], "Integer");
    EXPECT_EQ(types["area"], "Real");
    EXPECT_EQ(types["surveyed"], "Date");
    EXPECT_EQ(types["checked"], "Integer/Boolean");
}

TEST_F(InfoVerb, InfoOfTheSampleTerrainDescribesItsRasterAndBand)
{
    auto reply = run("INFO " + quoted(kSamples + "/terrain.asc"));
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto dataset = recordsOf(*reply, "dataset");
    ASSERT_EQ(dataset.size(), 1u) << *reply;
    EXPECT_EQ(field(dataset[0], "kind"), "raster");
    EXPECT_EQ(field(dataset[0], "driver"), "AAIGrid");
    const auto raster = recordsOf(*reply, "raster");
    ASSERT_EQ(raster.size(), 1u);
    EXPECT_EQ(field(raster[0], "width"), "120");
    EXPECT_EQ(field(raster[0], "height"), "90");
    EXPECT_EQ(field(raster[0], "bands"), "1");
    EXPECT_EQ(field(raster[0], "cell"), "1.5,1.5");
    EXPECT_EQ(field(raster[0], "bounds"), "-5,-5,175,130");
    const auto bands = recordsOf(*reply, "band");
    ASSERT_EQ(bands.size(), 1u);
    EXPECT_EQ(field(bands[0], "band"), "1");
    EXPECT_EQ(field(bands[0], "nodata"), "-9999");
    // Not computed, so not given: absent, not zero.
    EXPECT_EQ(field(bands[0], "min"), "");
    EXPECT_EQ(field(bands[0], "max"), "");
}

TEST_F(InfoVerb, InfoStatsOfTheSampleTerrainGivesTheMinAndMaxOfItsValues)
{
    // A copy, so that a sidecar written beside it would be seen - and none
    // may be: the bridge puts back whatever --stats would have left.
    std::filesystem::copy_file(kSamples + "/terrain.asc", scratch.path() / "terrain.asc");
    std::filesystem::copy_file(kSamples + "/terrain.prj", scratch.path() / "terrain.prj");
    const std::string copy = scratch.file("terrain.asc");
    auto reply = run("INFO " + quoted(copy) + " STATS");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto bands = recordsOf(*reply, "band");
    ASSERT_EQ(bands.size(), 1u) << *reply;

    const std::vector<double> values = terrainValues();
    ASSERT_EQ(values.size(), 120u * 90u);
    const auto [low, high] = std::ranges::minmax(values);
    double sum = 0.0;
    for (const double value : values) {
        sum += value;
    }
    const double mean = sum / static_cast<double>(values.size());
    double squares = 0.0;
    for (const double value : values) {
        squares += (value - mean) * (value - mean);
    }
    const double stdDev = std::sqrt(squares / static_cast<double>(values.size()));
    // GDAL reads an ASCII grid as Float32: every value below 64 is rounded
    // by at most half an ulp, 2^-19 = 1.9e-6, and so are the minimum and
    // maximum. The mean of values each off by at most 1.9e-6 is off by at
    // most that, and a standard deviation moves by no more than the largest
    // change of any value. GDAL's is the population deviation (N, not N - 1),
    // which for 10 800 values differs from the other by 4.6e-5 relative -
    // ten times this tolerance at a deviation of 3.
    constexpr double kFloat32 = 2e-6;
    EXPECT_NEAR(numberIn(bands[0], "min"), low, kFloat32);
    EXPECT_NEAR(numberIn(bands[0], "max"), high, kFloat32);
    EXPECT_NEAR(numberIn(bands[0], "mean"), mean, kFloat32);
    EXPECT_NEAR(numberIn(bands[0], "stddev"), stdDev, kFloat32);
    EXPECT_NEAR(low, 24.892, 1e-12);
    EXPECT_NEAR(high, 38.819, 1e-12);

    EXPECT_FALSE(std::filesystem::exists(scratch.path() / "terrain.asc.aux.xml"));
}

TEST_F(InfoVerb, InfoJsonIsGdalsJson)
{
    auto reply = run("INFO " + quoted(kSamples + "/terrain.asc") + " JSON");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const nlohmann::json json = nlohmann::json::parse(*reply);
    EXPECT_EQ(json["path"], kSamples + "/terrain.asc");
    // GDAL's own raster info, verbatim: its keys, its driver.
    EXPECT_EQ(json["raster"]["driverShortName"], "AAIGrid");
    EXPECT_EQ(json["raster"]["size"], nlohmann::json({120, 90}));
    EXPECT_TRUE(json["vector"].is_null());

    auto vector = run("INFO " + quoted(kData + "/lots.geojson") + " JSON");
    ASSERT_TRUE(vector.ok()) << vector.error().describe();
    const nlohmann::json lots = nlohmann::json::parse(*vector);
    EXPECT_EQ(lots["vector"]["driverShortName"], "GeoJSON");
    EXPECT_EQ(lots["vector"]["layers"][0]["featureCount"], 3);
    EXPECT_TRUE(lots["raster"].is_null());
}

TEST_F(InfoVerb, InfoOfAFolderIdentifiesEachFile)
{
    auto reply = run("INFO " + quoted(kSamples));
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto dataset = recordsOf(*reply, "dataset");
    ASSERT_EQ(dataset.size(), 1u) << *reply;
    EXPECT_EQ(field(dataset[0], "kind"), "folder");
    EXPECT_EQ(field(dataset[0], "datasets"), "3");
    std::map<std::string, std::string> drivers;
    for (const geo::Record& found : recordsOf(*reply, "found")) {
        drivers[std::filesystem::path(field(found, "file")).filename().string()] =
            field(found, "driver");
    }
    EXPECT_EQ(drivers["parcels.geojson"], "GeoJSON");
    EXPECT_EQ(drivers["terrain.asc"], "AAIGrid");
    EXPECT_EQ(drivers["survey_scan.las"], "readers.las");

    auto json = run("INFO " + quoted(kSamples) + " JSON");
    ASSERT_TRUE(json.ok()) << json.error().describe();
    const nlohmann::json parsed = nlohmann::json::parse(*json);
    EXPECT_EQ(parsed["identify"].size(), 2u); // GDAL's, which reads no LAS
    EXPECT_EQ(parsed["pointclouds"].size(), 1u);
}

TEST_F(InfoVerb, InfoCheckOfAValidFileReturnsZero)
{
    auto reply = run("INFO " + quoted(kSamples + "/terrain.asc") + " CHECK");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto check = recordsOf(*reply, "check");
    ASSERT_EQ(check.size(), 1u) << *reply;
    EXPECT_EQ(field(check[0], "code"), "0");
    EXPECT_EQ(field(check[0], "problems"), "0");
    EXPECT_TRUE(recordsOf(*reply, "problem").empty());
}

TEST_F(InfoVerb, InfoOfAVsizipPathWorks)
{
    std::ifstream in(kData + "/lots.geojson", std::ios::binary);
    std::ostringstream bytes;
    bytes << in.rdbuf();
    const std::filesystem::path archive = scratch.path() / "lots.zip";
    ASSERT_TRUE(katana::gis::writeZip(archive, "lots.geojson", bytes.str()).ok());
    const std::string inside = "/vsizip/" + archive.generic_string() + "/lots.geojson";
    auto reply = run("INFO " + quoted(inside));
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const auto layers = recordsOf(*reply, "layer");
    ASSERT_EQ(layers.size(), 1u) << *reply;
    EXPECT_EQ(field(layers[0], "name"), "lots");
    EXPECT_EQ(field(layers[0], "features"), "3");
}

TEST_F(InfoVerb, InfoLayerNamesOneLayerAndAnUnknownOneIsRefused)
{
    auto one = run("INFO " + quoted(kData + "/lots.geojson") + " LAYER lots");
    ASSERT_TRUE(one.ok()) << one.error().describe();
    EXPECT_EQ(recordsOf(*one, "layer").size(), 1u);
    auto none = run("INFO " + quoted(kData + "/lots.geojson") + " LAYER roads");
    EXPECT_FALSE(none.ok());
}

TEST_F(InfoVerb, InfoOfANetcdfGivesItsMultidimensionalJson)
{
    // A netCDF made here from the plane fixture, by GDAL itself; a GDAL
    // without the driver cannot make one, and the test says so.
    const std::string nc = scratch.file("plane.nc");
    auto made = run("GDAL raster convert " + quoted(kData + "/plane.asc") + " " + quoted(nc));
    if (!made.ok()) {
        GTEST_SKIP() << "no netCDF driver: " << made.error().describe();
    }
    auto reply = run("INFO " + quoted(nc) + " JSON");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    const nlohmann::json json = nlohmann::json::parse(*reply);
    EXPECT_EQ(json["raster"]["driverShortName"], "netCDF");
    ASSERT_TRUE(json["multidim"].is_object()) << json.dump();
    EXPECT_EQ(json["multidim"]["type"], "group");
}

TEST_F(InfoVerb, InfoRefusesWhatItCannotRead)
{
    auto usage = run("INFO");
    ASSERT_FALSE(usage.ok());
    EXPECT_EQ(usage.error().code, ErrorCode::InvalidArgument);
    auto word = run("INFO " + quoted(kSamples + "/terrain.asc") + " SHUFFLE");
    ASSERT_FALSE(word.ok());
    EXPECT_EQ(word.error().code, ErrorCode::InvalidArgument);
    auto missing = run("INFO " + quoted(scratch.file("absent.tif")));
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    // A file with blanks in its name, unquoted, as INFO has always read it.
    std::filesystem::copy_file(kData + "/lots.geojson", scratch.path() / "site lots.geojson");
    auto blanks = run("INFO " + scratch.file("site lots.geojson"));
    EXPECT_TRUE(blanks.ok()) << blanks.error().describe();
}

TEST_F(InfoVerb, InfoOfAnEntityIdStillDescribesTheEntity)
{
    // The interpreter's INFO: the executor leaves it alone.
    ASSERT_TRUE(interpreter.run("RECT 0,0 10,5").ok());
    EXPECT_FALSE(geo::handles("INFO 1"));
    EXPECT_FALSE(geo::handles("INFO #1"));
    auto entity = interpreter.run("INFO 1");
    ASSERT_TRUE(entity.ok()) << entity.error().describe();
    EXPECT_EQ(*entity, "1  Polyline  layer=0  vertices=4  closed  length=30  area=50");
}

} // namespace
