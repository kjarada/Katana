// GIS SQL and katana_gis_query (src/katana_app/geo/sql_verbs.cpp,
// sql_mcp.cpp; docs/geoprocessing.md "V5"): the drawing queried as tables,
// through the one executor and through the MCP server as an agent calls it.
// Every expected value is worked by hand beside it.

#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "contract_support.hpp"
#include "katana/gis/processing.hpp"
#include "mcp_server.hpp"
#include "session.hpp"
#include "vector_fixture.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::EntityId;
using katana::entity::PropertyMap;
using Json = nlohmann::json;

class GisSql : public katana::geo_test::VectorFixture {
  protected:
    EntityId smithA = 0, smithB = 0, jones = 0;

    // Smith's two 50 x 40 lots (2000 m2 each) and Jones's 30 x 20 (600 m2).
    void lots()
    {
        smithA = rect(0, 0, 50, 40, "lots", PropertyMap{{"owner", std::string("Smith")}});
        smithB = rect(50, 0, 100, 40, "lots", PropertyMap{{"owner", std::string("Smith")}});
        jones = rect(0, 100, 30, 120, "lots", PropertyMap{{"owner", std::string("Jones")}});
    }

    std::vector<katana::app::geo::Record> rows(const std::string& reply) const
    {
        std::vector<katana::app::geo::Record> found;
        for (katana::app::geo::Record& one : katana::app::geo::parseRecords(reply)) {
            if (one.kind == "row") {
                found.push_back(std::move(one));
            }
        }
        return found;
    }
};

TEST_F(GisSql, StAreaOfAFiftyByFortyLotIs2000)
{
    const EntityId lot = rect(0, 0, 50, 40, "lots");
    const std::string reply =
        ok("GIS SQL \"SELECT katana_id, ST_Area(geometry) AS area FROM polygons\" DRAWING");
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 1u) << reply;
    EXPECT_EQ(found.front().get("katana_id"), std::to_string(lot));
    EXPECT_EQ(found.front().get("area"), "2000");
    // The columns say their types, so a reader knows a number from text.
    bool typed = false;
    for (const auto& one : katana::app::geo::parseRecords(reply)) {
        if (one.kind == "column" && one.get("name") == "area") {
            EXPECT_EQ(one.get("type"), "real");
            typed = true;
        }
    }
    EXPECT_TRUE(typed);
    // A query changes nothing.
    EXPECT_EQ(entityCount(), 1u);
}

TEST_F(GisSql, StAreaOfItsMinusOneMetreBufferIs1824)
{
    // One metre in from each side of 50 x 40 leaves 48 x 38 = 1824 m2; the
    // corners of an inward offset stay square.
    rect(0, 0, 50, 40, "lots");
    const auto found =
        rows(ok("GIS SQL \"SELECT ST_Area(ST_Buffer(geometry, -1)) AS inner FROM polygons\" DRAWING"));
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found.front().get("inner"), "1824");
}

TEST_F(GisSql, GroupByOwnerSumsHandComputedAreas)
{
    lots();
    const auto found = rows(ok("GIS SQL \"SELECT owner, SUM(ST_Area(geometry)) AS area, COUNT(*) "
                               "AS lots FROM polygons GROUP BY owner ORDER BY owner\" DRAWING"));
    ASSERT_EQ(found.size(), 2u);
    EXPECT_EQ(found[0].get("owner"), "Jones");
    EXPECT_EQ(found[0].get("area"), "600");
    EXPECT_EQ(found[0].get("lots"), "1");
    EXPECT_EQ(found[1].get("owner"), "Smith");
    EXPECT_EQ(found[1].get("area"), "4000");
    EXPECT_EQ(found[1].get("lots"), "2");
}

TEST_F(GisSql, AsSelectSelectsTheReturnedIds)
{
    lots();
    const std::string reply =
        ok("GIS SQL \"SELECT katana_id FROM polygons WHERE owner = 'Smith'\" DRAWING AS SELECT");
    const auto ids = document.selection().ids();
    ASSERT_EQ(ids.size(), 2u);
    EXPECT_EQ(ids[0], smithA);
    EXPECT_EQ(ids[1], smithB);
    const auto output = record(reply, "output");
    ASSERT_TRUE(output);
    EXPECT_EQ(output->get("target"), "selection");
    EXPECT_EQ(output->get("selected"), "2");
    // Without the katana_id column there is nothing to select by.
    auto refused = run("GIS SQL \"SELECT owner FROM polygons\" DRAWING AS SELECT");
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

TEST_F(GisSql, AsLayerCreatesOneUndoStep)
{
    lots();
    const std::size_t before = entityCount();
    ok("GIS SQL \"SELECT * FROM polygons WHERE owner = 'Jones'\" DRAWING AS LAYER copies");
    const auto made = on("copies");
    ASSERT_EQ(made.size(), 1u);
    EXPECT_NEAR(std::get<katana::geometry::Polyline2>(made.front().geometry).area(), 600.0, 1e-9);
    EXPECT_EQ(katana::entity::toString(made.front().properties.at("gis.source")),
              std::to_string(jones));
    EXPECT_EQ(katana::entity::toString(made.front().properties.at("owner")), "Jones");
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_EQ(entityCount(), before);
}

TEST_F(GisSql, ANonSelectStatementIsRefused)
{
    lots();
    const std::size_t before = entityCount();
    for (const char* line :
         {"GIS SQL \"DELETE FROM polygons\" DRAWING", "GIS SQL \"UPDATE polygons SET owner = 'x'\" DRAWING",
          "GIS SQL \"SELECT 1; DELETE FROM polygons\" DRAWING", "GIS SQL SELECT DRAWING"}) {
        auto refused = run(line);
        ASSERT_FALSE(refused.ok()) << line;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument) << line;
    }
    EXPECT_EQ(entityCount(), before);
}

TEST_F(GisSql, BlobToFileIsRefused)
{
    // Spatialite's file functions stay off (SPATIALITE_SECURITY is never
    // set): BlobToFile writes nothing, whatever the query answers.
    rect(0, 0, 50, 40, "lots");
    const std::filesystem::path leak = scratch_ / "leak.bin";
    const auto reply = run("GIS SQL \"SELECT BlobToFile(X'414243', '" + leak.generic_string() +
                           "') AS written FROM polygons\" DRAWING");
    if (reply.ok()) {
        const auto found = rows(*reply);
        ASSERT_EQ(found.size(), 1u) << *reply;
        EXPECT_NE(found.front().get("written"), "1") << *reply;
    }
    EXPECT_FALSE(std::filesystem::exists(leak));
}

TEST_F(GisSql, TheRowsAreWrittenToCsv)
{
    lots();
    const std::string csv = (scratch_ / "owners.csv").generic_string();
    ok("GIS SQL \"SELECT owner, SUM(ST_Area(geometry)) AS area FROM polygons GROUP BY owner ORDER "
       "BY owner\" DRAWING csv=\"" + csv + "\"");
    std::ifstream in(csv);
    std::stringstream text;
    text << in.rdbuf();
    EXPECT_EQ(text.str(), "owner,area\nJones,600\nSmith,4000\n");
}

TEST_F(GisSql, TheGdalVerbReportsAndSelectsToo)
{
    lots();
    const std::string report =
        ok("GDAL vector sql \"--sql=SELECT katana_id, owner FROM polygons WHERE owner = 'Jones'\" "
           "--dialect=SQLITE FROM DRAWING TO REPORT");
    const auto found = rows(report);
    ASSERT_EQ(found.size(), 1u) << report;
    EXPECT_EQ(found.front().get("owner"), "Jones");
    ok("GDAL vector sql \"--sql=SELECT katana_id FROM polygons WHERE owner = 'Jones'\" "
       "--dialect=SQLITE FROM DRAWING TO SELECTION");
    ASSERT_EQ(document.selection().ids().size(), 1u);
    EXPECT_EQ(document.selection().ids().front(), jones);
}

TEST_F(GisSql, PreviewChangesNothing)
{
    lots();
    const std::string reply = ok("GIS SQL \"SELECT * FROM polygons\" DRAWING AS LAYER copies PREVIEW");
    EXPECT_TRUE(reply.starts_with("gis op=sql preview=yes")) << reply;
    EXPECT_TRUE(on("copies").empty());
}

TEST(GisSqlContract, TheArgumentsTheVerbBindsAreGdals)
{
    using katana::geo_test::expectArgument;
    expectArgument({"vector", "sql"}, "sql", gp::ArgType::StringList, true);
    expectArgument({"vector", "sql"}, "dialect", gp::ArgType::String, false);
}

// The MCP server, driven as a client drives it.
class GisQueryMcp : public ::testing::Test {
  protected:
    katana::app::Session session{nullptr};
    katana::app::mcp::Server server{session, "9.9.9"};
    int nextId = 1;

    Json call(const std::string& tool, Json arguments)
    {
        const Json message{{"jsonrpc", "2.0"},
                           {"id", nextId++},
                           {"method", "tools/call"},
                           {"params", {{"name", tool}, {"arguments", std::move(arguments)}}}};
        const auto reply = server.handle(message.dump());
        EXPECT_TRUE(reply.has_value());
        return reply ? Json::parse(*reply).value("result", Json::object()) : Json();
    }
};

TEST_F(GisQueryMcp, GisQueryReturnsRowsAsJson)
{
    // Two 50 x 40 lots, one owned by Smith: 2000 m2 each, typed as numbers.
    (void)call("katana_run_commands",
               Json{{"commands",
                     {"RECT 0,0 50,40", "RECT 50,0 100,40", "SELECT 1", "MODIFY SET PROP=owner:Smith"}}});
    const Json result = call(
        "katana_gis_query",
        Json{{"sql", "SELECT \"katana_id\", owner, ST_Area(geometry) AS area FROM polygons ORDER "
                     "BY katana_id"}});
    ASSERT_FALSE(result.value("isError", true)) << result.dump();
    const Json& query = result["structuredContent"];
    EXPECT_EQ(query["line"],
              "GIS SQL \"SELECT [katana_id], owner, ST_Area(geometry) AS area FROM polygons ORDER "
              "BY katana_id\" DRAWING dialect=sqlite");
    EXPECT_EQ(query["columns"], (Json{"katana_id", "owner", "area"}));
    EXPECT_EQ(query["column_types"], (Json{"integer", "string", "real"}));
    ASSERT_EQ(query["rows"].size(), 2U);
    EXPECT_EQ(query["rows"][0]["katana_id"], 1);
    EXPECT_EQ(query["rows"][0]["owner"], "Smith");
    EXPECT_EQ(query["rows"][0]["area"], 2000.0);
    EXPECT_TRUE(query["rows"][1]["owner"].is_null()); // no owner: null, not ""
    EXPECT_EQ(query["matched"], 2);
    EXPECT_EQ(query["used"], 2);
}

TEST_F(GisQueryMcp, AStatementThatIsNotASelectIsRefused)
{
    (void)call("katana_run_commands", Json{{"commands", {"RECT 0,0 50,40"}}});
    const Json result = call("katana_gis_query", Json{{"sql", "DELETE FROM polygons"}});
    EXPECT_TRUE(result.value("isError", false)) << result.dump();
    EXPECT_EQ(session.document().model().entities.size(), 1U);
}

} // namespace
