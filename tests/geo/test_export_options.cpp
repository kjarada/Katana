// EXPORT on the shared scope and filter, with its options
// (src/katana_app/geo/export_verb.cpp; docs/interop.md, "Export options"):
// what of the drawing is written, the coordinate system it is written in,
// one file layer or one per drawing layer, adding to a GeoPackage, the
// driver's options checked, and text as points - run through the one
// executor as katana_cli, katana_mcp and the window's dialog run it, and
// checked by reading the file back through GDAL.
//
// The drawing each test starts from, by hand (drawSite):
//   lots   two closed rectangles, (0, 0) - (50, 40) and (50, 0) - (100, 40)
//   roads  one line, (0, 50) - (100, 50)
//   pegs   one point, (10, 60)
// so 4 entities; LAYERS lots,pegs takes 3; WHERE TYPE=polyline takes the 2
// rectangles.

#include <cmath>
#include <filesystem>
#include <fstream>
#include <map>
#include <numbers>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "dxf_verbs.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/gis/reproject.hpp"
#include "vector_fixture.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::geo_test::VectorFixture;

class ExportOptions : public VectorFixture {
  protected:
    void drawSite()
    {
        rect(0.0, 0.0, 50.0, 40.0, "lots");
        rect(50.0, 0.0, 100.0, 40.0, "lots");
        line(0.0, 50.0, 100.0, 50.0, "roads");
        add(katana::entity::PointGeometry{{10.0, 60.0}}, "pegs");
    }

    std::string file(const std::string& name) const { return (scratch_ / name).generic_string(); }
    static std::string quoted(const std::string& path) { return "\"" + path + "\""; }

    // The file's layers and how many features each holds, read by GDAL.
    std::map<std::string, std::uint64_t> layersOf(const std::string& path) const
    {
        std::map<std::string, std::uint64_t> found;
        auto dataset = katana::gis::GdalDataset::open(path);
        EXPECT_TRUE(dataset.ok()) << path;
        if (!dataset.ok()) {
            return found;
        }
        for (const katana::gis::VectorLayerInfo& layer : *(*dataset)->vectorLayers()) {
            found[layer.name] = layer.featureCount;
        }
        return found;
    }

    gp::FeatureTable table(const std::string& path, int layer = 0) const
    {
        auto dataset = katana::gis::GdalDataset::open(path);
        EXPECT_TRUE(dataset.ok()) << path;
        if (!dataset.ok()) {
            return {};
        }
        auto read = (*dataset)->readTable(layer);
        EXPECT_TRUE(read.ok());
        return read.ok() ? *read : gp::FeatureTable{};
    }

    static const gp::FieldValue* field(const gp::FeatureTable& table, const gp::Feature& feature,
                                       const std::string& name)
    {
        for (std::size_t f = 0; f < table.fields.size() && f < feature.values.size(); ++f) {
            if (table.fields[f].name == name) {
                return &feature.values[f];
            }
        }
        return nullptr;
    }
};

// ---- what is written ----------------------------------------------------------------------------

TEST_F(ExportOptions, ExportWritesTheProjectCrs)
{
    drawSite();
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:7856").ok());
    const std::string out = file("site.gpkg");
    ok("EXPORT " + quoted(out));
    auto dataset = katana::gis::GdalDataset::open(out);
    ASSERT_TRUE(dataset.ok());
    const auto layers = (*dataset)->vectorLayers();
    ASSERT_TRUE(layers.ok());
    ASSERT_EQ(layers->size(), 1u);
    EXPECT_EQ(katana::gis::crsEpsgCode(layers->front().projectionWkt), std::optional<int>(7856));
}

TEST_F(ExportOptions, AKmlSaysItIsInLongitudeAndLatitudeNotTheProjects)
{
    // The writer converts a KML to WGS 84 whatever the drawing is in; the
    // reply named the project's system, which the file is not in.
    add(katana::entity::PointGeometry{{330000.0, 6250000.0}}, "pegs");
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    const std::string reply = ok("EXPORT " + quoted(file("pegs.kml")));
    const auto exported = record(reply, "exported");
    ASSERT_TRUE(exported.has_value()) << reply;
    EXPECT_NE(exported->get("crs").value_or("").find("EPSG:4326"), std::string::npos) << reply;
}

TEST_F(ExportOptions, EntitiesImportedMovedGoOutInNoCoordinateSystem)
{
    // Imported LOCAL, the lot is moved to 0,0: its coordinates are in
    // neither its file's system nor the project's, and a GeoPackage that
    // claimed the project's put it off the coast of Africa.
    const std::string lot = file("lot.geojson");
    {
        std::ofstream out(lot);
        out << R"({"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::28356"}},)"
            << R"("features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[330000,6250000]}}]})";
    }
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    ok("IMPORT " + quoted(lot) + " LOCAL");
    const std::string out = file("moved.gpkg");
    const std::string reply = ok("EXPORT " + quoted(out));
    const auto exported = record(reply, "exported");
    ASSERT_TRUE(exported.has_value()) << reply;
    EXPECT_EQ(exported->get("crs").value_or("<none>"), "") << reply;
    EXPECT_NE(reply.find("imported moved from their file's coordinates"), std::string::npos)
        << reply;
    auto dataset = katana::gis::GdalDataset::open(out);
    ASSERT_TRUE(dataset.ok());
    const auto layers = (*dataset)->vectorLayers();
    ASSERT_TRUE(layers.ok());
    ASSERT_EQ(layers->size(), 1u);
    EXPECT_TRUE(layers->front().projectionWkt.empty());
    // Nor can they be converted into another system.
    const auto refused = run("EXPORT " + quoted(file("moved.kml")));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidCRS);
}

TEST_F(ExportOptions, ExportOfLayersOnlyWritesThoseLayers)
{
    drawSite();
    const std::string out = file("some.gpkg");
    const std::string reply = ok("EXPORT " + quoted(out) + " LAYERS lots,pegs");
    EXPECT_EQ(layersOf(out), (std::map<std::string, std::uint64_t>{{"katana", 3}}));
    const auto scope = record(reply, "scope");
    ASSERT_TRUE(scope.has_value()) << reply;
    EXPECT_EQ(scope->get("matched"), std::optional<std::string>("3"));
    // The exported record stays the first, as it always was.
    EXPECT_EQ(katana::app::geo::parseRecords(reply).front().kind, "exported");
}

TEST_F(ExportOptions, WhereFiltersTheExport)
{
    drawSite();
    const std::string out = file("areas.gpkg");
    ok("EXPORT " + quoted(out) + " DRAWING WHERE TYPE=polyline layername=areas");
    EXPECT_EQ(layersOf(out), (std::map<std::string, std::uint64_t>{{"areas", 2}}));
}

TEST_F(ExportOptions, SplitByLayerWritesOneLayerEach)
{
    drawSite();
    const std::string out = file("split.gpkg");
    ok("EXPORT " + quoted(out) + " split=layer");
    EXPECT_EQ(layersOf(out),
              (std::map<std::string, std::uint64_t>{{"lots", 2}, {"pegs", 1}, {"roads", 1}}));
}

TEST_F(ExportOptions, AppendAddsASecondLayerToAGpkg)
{
    drawSite();
    const std::string out = file("both.gpkg");
    ok("EXPORT " + quoted(out) + " LAYERS lots layername=lots");
    ok("EXPORT " + quoted(out) + " LAYERS roads layername=roads append");
    EXPECT_EQ(layersOf(out), (std::map<std::string, std::uint64_t>{{"lots", 2}, {"roads", 1}}));
    // A layer name the file has already is refused, and the file kept.
    const auto again = run("EXPORT " + quoted(out) + " LAYERS pegs layername=roads append");
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(layersOf(out), (std::map<std::string, std::uint64_t>{{"lots", 2}, {"roads", 1}}));
}

TEST_F(ExportOptions, AnUnknownCreationOptionIsRefused)
{
    drawSite();
    const std::string out = file("refused.gpkg");
    const auto refused = run("EXPORT " + quoted(out) + " lco=NO_SUCH_OPTION=1");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("NO_SUCH_OPTION"), std::string::npos)
        << refused.error().message;
    EXPECT_FALSE(std::filesystem::exists(out));
    // One the GeoPackage driver declares is written with.
    ok("EXPORT " + quoted(out) + " lco=SPATIAL_INDEX=NO");
    EXPECT_EQ(layersOf(out).at("katana"), 4u);
}

TEST_F(ExportOptions, TextBecomesPointsWithTextFields)
{
    katana::entity::TextGeometry text;
    text.position = {10.0, 20.0};
    text.text = "MH 12";
    text.height = 2.5;
    text.rotation = std::numbers::pi / 6.0; // 30 degrees
    add(text, "notes");
    const std::string out = file("text.gpkg");
    // Left out, a text is all there is: nothing to export, said.
    EXPECT_FALSE(run("EXPORT " + quoted(out)).ok());

    const std::string reply = ok("EXPORT " + quoted(out) + " text=points");
    EXPECT_EQ(record(reply, "exported")->get("texts"), std::optional<std::string>("1"));
    const gp::FeatureTable written = table(out);
    ASSERT_EQ(written.features.size(), 1u);
    const gp::Feature& point = written.features.front();
    ASSERT_EQ(point.parts.size(), 1u);
    EXPECT_EQ(point.parts.front().kind, katana::gis::GeometryKind::Point);
    EXPECT_DOUBLE_EQ(point.parts.front().parts.front().front().x, 10.0);
    EXPECT_DOUBLE_EQ(point.parts.front().parts.front().front().y, 20.0);
    const auto* words = field(written, point, "text");
    ASSERT_NE(words, nullptr);
    EXPECT_EQ(std::get<std::string>(*words), "MH 12");
    EXPECT_DOUBLE_EQ(std::get<double>(*field(written, point, "text_height")), 2.5);
    // pi / 6 radians is 30 degrees; 1e-12 is far above the rounding of one
    // multiplication and one division.
    EXPECT_NEAR(std::get<double>(*field(written, point, "text_rotation")), 30.0, 1e-12);
    const auto* style = field(written, point, "OGR_STYLE");
    ASSERT_NE(style, nullptr);
    EXPECT_NE(std::get<std::string>(*style).find("LABEL(t:\"MH 12\""), std::string::npos);
}

TEST_F(ExportOptions, PropertiesNoWritesNone)
{
    rect(0.0, 0.0, 10.0, 10.0, "lots", {{"owner", std::string("Smith")}});
    const std::string out = file("bare.gpkg");
    ok("EXPORT " + quoted(out) + " properties=no");
    for (const gp::FieldDef& written : table(out).fields) {
        EXPECT_NE(written.name, "owner");
    }
}

// ---- coordinate systems -------------------------------------------------------------------------

// PROJ's own cs2cs puts 330000,6250000 in EPSG:28356 at 151.161906845856,
// -33.876653622997 (`echo 330000 6250000 | cs2cs -f %.12f EPSG:28356
// EPSG:4326`, as tests/geo/test_vector_fidelity.cpp records it).
constexpr double kLongitude = 151.161906845856;
constexpr double kLatitude = -33.876653622997;

TEST_F(ExportOptions, GeoJsonOfAProjectedDrawingIsLonLat)
{
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    add(katana::entity::PointGeometry{{330000.0, 6250000.0}}, "pegs");
    const std::string out = file("peg.geojson");
    const std::string reply = ok("EXPORT " + quoted(out));
    EXPECT_NE(reply.find("RFC 7946"), std::string::npos) << reply;
    const gp::FeatureTable written = table(out);
    ASSERT_EQ(written.features.size(), 1u);
    const katana::gis::GeoPoint& point = written.features.front().parts.front().parts.front().front();
    // 1e-9 degrees is about 0.1 mm: the cs2cs figures carry 12 decimals.
    EXPECT_NEAR(point.x, kLongitude, 1e-9);
    EXPECT_NEAR(point.y, kLatitude, 1e-9);
}

TEST_F(ExportOptions, CrsNativeKeepsTheProjectedCrsInAGeoJson)
{
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    add(katana::entity::PointGeometry{{330000.0, 6250000.0}}, "pegs");
    const std::string out = file("native.geojson");
    ok("EXPORT " + quoted(out) + " crs=native");
    const gp::FeatureTable written = table(out);
    ASSERT_EQ(written.features.size(), 1u);
    const katana::gis::GeoPoint& point = written.features.front().parts.front().parts.front().front();
    EXPECT_DOUBLE_EQ(point.x, 330000.0);
    EXPECT_DOUBLE_EQ(point.y, 6250000.0);
    EXPECT_EQ(katana::gis::crsEpsgCode(written.crsWkt), std::optional<int>(28356));
}

TEST_F(ExportOptions, CrsCodeMovesTheCoordinatesOnTheWayOut)
{
    ASSERT_TRUE(document.setCoordinateSystem("EPSG:28356").ok());
    add(katana::entity::PointGeometry{{330000.0, 6250000.0}}, "pegs");
    const std::string out = file("moved.gpkg");
    ok("EXPORT " + quoted(out) + " crs=EPSG:4326");
    const gp::FeatureTable written = table(out);
    ASSERT_EQ(written.features.size(), 1u);
    const katana::gis::GeoPoint& point = written.features.front().parts.front().parts.front().front();
    EXPECT_NEAR(point.x, kLongitude, 1e-9);
    EXPECT_NEAR(point.y, kLatitude, 1e-9);
    EXPECT_EQ(katana::gis::crsEpsgCode(written.crsWkt), std::optional<int>(4326));
}

TEST_F(ExportOptions, CrsCodeWithoutAProjectCrsIsRefused)
{
    drawSite();
    const auto refused = run("EXPORT " + quoted(file("x.gpkg")) + " crs=EPSG:4326");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidCRS);
}

// ---- the native formats, PREVIEW and nothing taken ----------------------------------------------

TEST_F(ExportOptions, AScopedDxfExportWritesOnlyTheScope)
{
    drawSite();
    const std::string out = file("lots.dxf");
    ok("EXPORT " + quoted(out) + " LAYERS lots");
    auto read = katana::app::readDxfImport(out);
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(read->entities.size(), 2u);
}

TEST_F(ExportOptions, GdalOptionsForADxfAreRefusedByName)
{
    drawSite();
    const auto refused = run("EXPORT " + quoted(file("x.dxf")) + " split=layer");
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().message.find("split="), std::string::npos);
}

TEST_F(ExportOptions, PreviewWritesNothing)
{
    drawSite();
    const std::string out = file("preview.gpkg");
    const std::string reply = ok("EXPORT " + quoted(out) + " LAYERS lots PREVIEW");
    EXPECT_FALSE(std::filesystem::exists(out));
    const auto preview = record(reply, "export");
    ASSERT_TRUE(preview.has_value()) << reply;
    EXPECT_EQ(preview->get("preview"), std::optional<std::string>("yes"));
    EXPECT_EQ(preview->get("entities"), std::optional<std::string>("2"));
}

TEST_F(ExportOptions, AScopeThatTakesNothingWritesNothingAndSaysSo)
{
    drawSite();
    const std::string out = file("none.gpkg");
    const std::string reply = ok("EXPORT " + quoted(out) + " DRAWING WHERE TYPE=circle");
    EXPECT_FALSE(std::filesystem::exists(out));
    EXPECT_EQ(record(reply, "export")->get("ran"), std::optional<std::string>("no"));
    EXPECT_EQ(record(reply, "scope")->get("matched"), std::optional<std::string>("0"));
}

TEST_F(ExportOptions, APlainExportStillWritesTheWholeDrawing)
{
    drawSite();
    const std::string out = file("all.gpkg");
    const std::string reply = ok("EXPORT " + quoted(out));
    EXPECT_EQ(layersOf(out), (std::map<std::string, std::uint64_t>{{"katana", 4}}));
    EXPECT_FALSE(record(reply, "scope").has_value());
}

} // namespace
