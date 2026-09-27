// GIS OVERLAY (src/katana_app/geo/overlay_verbs.cpp, docs/geoprocessing.md
// "V2"): polygon booleans between two scopes, or a scope and a file, through
// the one executor. The drawing is tests/geo/data/lots.geojson drawn: two
// 50 x 40 m lots side by side and a 4 m corridor across both, 120 m long.

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>

#include "contract_support.hpp"
#include "katana/gis/processing.hpp"
#include "vector_fixture.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::PropertyMap;

const std::string kData = KATANA_GEO_TEST_DATA;

class GisOverlay : public katana::geo_test::VectorFixture {
  protected:
    EntityId a = 0, b = 0, c = 0;

    void SetUp() override
    {
        a = rect(0, 0, 50, 40, "lots",
                 PropertyMap{{"kind", std::string("lot")}, {"name", std::string("A")},
                             {"owner", std::string("Smith")}});
        b = rect(50, 0, 100, 40, "lots",
                 PropertyMap{{"kind", std::string("lot")}, {"name", std::string("B")},
                             {"owner", std::string("Jones")}});
        c = rect(-10, 18, 110, 22, "corridor",
                 PropertyMap{{"kind", std::string("corridor")}, {"name", std::string("C")}});
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

std::string text(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    return found == entity.properties.end() ? std::string("<none>")
                                            : katana::entity::toString(found->second);
}

TEST_F(GisOverlay, IntersectionOfAFourMetreCorridorWithEachLotIs200SquareMetres)
{
    // By hand: the corridor crosses each 50 m lot 4 m wide: 4 x 50 = 200 m2.
    const std::string reply = ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor");
    const auto pieces = on("gis/overlay");
    ASSERT_EQ(pieces.size(), 2u);
    for (const Entity& piece : pieces) {
        EXPECT_NEAR(std::get<katana::geometry::Polyline2>(piece.geometry).area(), 200.0, 1e-9);
        EXPECT_EQ(text(piece, "gis.with"), std::to_string(c));
        // owner is the lot's alone, so it keeps its name; name is on both
        // sides, so each keeps GDAL's prefix.
        EXPECT_NE(text(piece, "owner"), "<none>");
        EXPECT_EQ(text(piece, "method_name"), "C");
    }
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 2u);
    for (const auto& row : found) {
        EXPECT_EQ(row.get("area"), "200.000");
        EXPECT_EQ(row.get("with"), std::to_string(c));
    }
    EXPECT_EQ(found[0].get("entity"), std::to_string(a));
    EXPECT_EQ(found[1].get("entity"), std::to_string(b));
    EXPECT_EQ(record(reply, "overlay")->get("area"), "400.000");
}

TEST_F(GisOverlay, DifferenceLeaves1800SquareMetresOfEachLot)
{
    // Each lot less the corridor: two 50 x 18 strips, 1800 m2 a lot.
    const std::string reply = ok("GIS OVERLAY difference LAYERS lots WITH LAYERS corridor");
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 2u);
    for (const auto& row : found) {
        EXPECT_EQ(row.get("area"), "1800.000");
    }
    EXPECT_EQ(on("gis/overlay").size(), 4u); // each lot's two strips
    EXPECT_NEAR(areaOn("gis/overlay"), 3600.0, 1e-9);
}

TEST_F(GisOverlay, ARowCarriesTheLotsPropertiesNotTheDrawingsBookkeeping)
{
    // A difference keeps the subject's fields without GDAL's input_ prefix,
    // so the drawing's layer, style, colour and type came back as the row's
    // - words about the entity, not properties of it - where an
    // intersection's rows left them out.
    const std::string reply = ok("GIS OVERLAY difference LAYERS lots WITH LAYERS corridor");
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 2u);
    for (const auto& row : found) {
        for (const char* bookkeeping : {"layer", "style", "colour", "type"}) {
            EXPECT_FALSE(row.get(bookkeeping).has_value()) << bookkeeping << " in " << reply;
        }
        EXPECT_TRUE(row.get("owner").has_value()) << reply;
    }
}

TEST_F(GisOverlay, AnOverlayMadeByAnotherVerbLendsItsIdNotItsProvenance)
{
    // The corridor as another verb's result carries gis.op and gis.source.
    // A row says which overlay piece it met by with=; the overlay's own
    // gis.op and gis.source, unprefixed on a row, read as the piece's own
    // (gis.source=<the pipe> beside entity=<the lot>), and the drawn piece
    // is given the overlay's own anyway.
    const EntityId made = rect(
        -10, 18, 110, 22, "made",
        PropertyMap{{"gis.op", std::string("vector buffer")}, {"gis.source", std::int64_t{99}}});
    const std::string reply = ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS made");
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 2u);
    for (const auto& row : found) {
        EXPECT_EQ(row.get("with"), std::to_string(made));
        EXPECT_FALSE(row.get("gis.op").has_value()) << reply;
        EXPECT_FALSE(row.get("gis.source").has_value()) << reply;
    }
    for (const Entity& piece : on("gis/overlay")) {
        EXPECT_EQ(text(piece, "gis.op"), "vector layer-algebra");
        EXPECT_NE(text(piece, "gis.source"), "99");
    }
}

TEST_F(GisOverlay, UnionAreaIsLotsPlusCorridorMinusOverlap)
{
    // By hand: 2000 + 2000 + 120 x 4 - 100 x 4 = 4080 m2, in pieces.
    const std::string reply = ok("GIS OVERLAY union LAYERS lots WITH LAYERS corridor");
    EXPECT_EQ(record(reply, "overlay")->get("area"), "4080.000");
    EXPECT_NEAR(areaOn("gis/overlay"), 4080.0, 1e-9);
}

TEST_F(GisOverlay, IdentityKeepsTheLotFieldsOnEachPiece)
{
    // Identity splits the lots where the corridor crosses them and keeps
    // their extent - 4000 m2 - each piece with its lot's properties.
    const std::string reply = ok("GIS OVERLAY identity LAYERS lots WITH LAYERS corridor");
    EXPECT_NEAR(areaOn("gis/overlay"), 4000.0, 1e-9);
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 4u);
    for (const auto& row : found) {
        const std::string name = row.get("input_name").value_or("");
        EXPECT_TRUE(name == "A" || name == "B") << name;
        EXPECT_TRUE(row.get("owner").has_value());
    }
}

TEST_F(GisOverlay, ALineAgainstLotsGivesItsLengthPerLot)
{
    // A 100 m pipe along y = 10 crosses both lots: 50 m in each.
    const EntityId pipe = line(0, 10, 100, 10, "pipes", PropertyMap{{"pipe", std::string("W1")}});
    const std::string reply = ok("GIS OVERLAY intersection LAYERS pipes WITH LAYERS lots");
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 2u);
    for (const auto& row : found) {
        EXPECT_EQ(row.get("length"), "50.000");
        EXPECT_EQ(row.get("entity"), std::to_string(pipe));
        EXPECT_EQ(row.get("pipe"), "W1");
    }
    EXPECT_EQ(found[0].get("with"), std::to_string(a));
    EXPECT_EQ(found[1].get("with"), std::to_string(b));
    EXPECT_EQ(on("gis/overlay").size(), 2u);
}

TEST_F(GisOverlay, AFileOverlayWorksLikeADrawingOne)
{
    // The corridor of lots.geojson, read by where=, cuts the drawn lots as
    // the drawn corridor does: 200 m2 each.
    const std::string reply =
        ok("GIS OVERLAY intersection LAYERS lots WITH FILE \"" + kData +
           "/lots.geojson\" where=\"kind='corridor'\" TO LAYER easements");
    const auto found = rows(reply);
    ASSERT_EQ(found.size(), 2u) << reply;
    for (const auto& row : found) {
        EXPECT_EQ(row.get("area"), "200.000");
        EXPECT_EQ(row.get("method_kind"), "corridor");
    }
    EXPECT_NEAR(areaOn("easements"), 400.0, 1e-9);
    const auto input = record(reply, "input");
    ASSERT_TRUE(input);
    EXPECT_EQ(input->get("arg"), "method");
    EXPECT_EQ(input->get("source"), "file");
}

TEST_F(GisOverlay, AnEmptySecondScopeIsReported)
{
    const std::string reply =
        ok("GIS OVERLAY intersection LAYERS lots WITH AREA 1000,1000,1010,1010");
    EXPECT_TRUE(reply.starts_with("gis op=overlay seconds=0.000 cancelled=no ran=no")) << reply;
    bool method = false;
    for (const auto& one : katana::app::geo::parseRecords(reply)) {
        if (one.kind == "scope" && one.get("arg") == "method") {
            method = true;
            EXPECT_EQ(one.get("matched"), "0");
        }
    }
    EXPECT_TRUE(method);
    EXPECT_TRUE(on("gis/overlay").empty());
}

TEST_F(GisOverlay, KeepNoneCarriesOnlyWhereEachPieceCameFrom)
{
    ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor keep=none keepwith=none");
    for (const Entity& piece : on("gis/overlay")) {
        EXPECT_EQ(text(piece, "owner"), "<none>");
        EXPECT_EQ(text(piece, "method_name"), "<none>");
        EXPECT_NE(text(piece, "gis.source"), "<none>");
        EXPECT_EQ(text(piece, "gis.with"), std::to_string(c));
    }
}

TEST_F(GisOverlay, TheRowsAreWrittenToCsvAndAFileIsReplacedOnlyWhenAsked)
{
    const std::string csv = (scratch_ / "rows.csv").generic_string();
    ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor keepwith=none csv=\"" + csv + "\"");
    std::ifstream in(csv);
    std::stringstream written;
    written << in.rdbuf();
    const std::string text = written.str();
    EXPECT_TRUE(text.starts_with("entity,")) << text;
    EXPECT_NE(text.find(",200.000\n"), std::string::npos) << text;
    EXPECT_EQ(std::ranges::count(text, '\n'), 3); // the header and two rows
    auto again = run("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor csv=\"" + csv + "\"");
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, ErrorCode::AlreadyExists);
    ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor csv=\"" + csv + "\" OVERWRITE");
}

TEST_F(GisOverlay, PreviewChangesNothing)
{
    const std::size_t before = entityCount();
    const std::string reply =
        ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor PREVIEW");
    EXPECT_TRUE(reply.starts_with("gis op=overlay preview=yes")) << reply;
    EXPECT_EQ(entityCount(), before);
}

TEST_F(GisOverlay, WhatItCannotDoIsRefusedByName)
{
    const auto refused = [&](const std::string& line) {
        auto reply = run(line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument)
            << line << ": " << reply.error().describe();
    };
    refused("GIS OVERLAY merge LAYERS lots WITH LAYERS corridor");
    refused("GIS OVERLAY intersection LAYERS lots");
    refused("GIS OVERLAY intersection LAYERS lots WITH");
    refused("GIS OVERLAY intersection LAYERS lots WITH FILE x.geojson LAYERS corridor");
    refused("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor where=\"kind='x'\"");
    refused("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor keep=");
    EXPECT_TRUE(on("gis/overlay").empty());
}

TEST(GisOverlayContract, TheArgumentsTheVerbBindsAreGdals)
{
    using katana::geo_test::expectArgument;
    expectArgument({"vector", "layer-algebra"}, "operation", gp::ArgType::String, true);
    expectArgument({"vector", "layer-algebra"}, "input", gp::ArgType::Dataset, true);
    expectArgument({"vector", "layer-algebra"}, "method", gp::ArgType::Dataset, true);
    expectArgument({"vector", "layer-algebra"}, "input-field", gp::ArgType::StringList, false);
    expectArgument({"vector", "layer-algebra"}, "method-field", gp::ArgType::StringList, false);
    expectArgument({"vector", "filter"}, "where", gp::ArgType::String, false);
    const auto spec = gp::describe({"vector", "layer-algebra"});
    ASSERT_TRUE(spec.ok());
    for (const gp::ArgSpec& arg : spec->args) {
        if (arg.name == "operation") {
            for (const char* operation :
                 {"intersection", "erase", "union", "sym-difference", "identity", "update", "clip"}) {
                EXPECT_NE(std::ranges::find(arg.choices, std::string(operation)), arg.choices.end())
                    << operation;
            }
        }
    }
}

} // namespace
