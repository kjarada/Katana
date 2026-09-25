#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <numbers>
#include <string>
#include <vector>

#include "katana/dxf/reader.hpp"
#include "katana/dxf/writer.hpp"
#include "katana/entity/model.hpp"

namespace dxf = katana::dxf;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::PointGeometry;
using katana::entity::TextGeometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

constexpr double kPi = std::numbers::pi;

Color rgb(int r, int g, int b)
{
    return Color{static_cast<std::uint8_t>(r), static_cast<std::uint8_t>(g),
                 static_cast<std::uint8_t>(b), 255};
}

katana::entity::EntityId add(katana::entity::Model& model, katana::entity::Geometry geometry,
                             const std::string& layer = "0",
                             std::vector<std::optional<double>> heights = {})
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = layer;
    if (!heights.empty()) {
        katana::entity::setHeights(entity.properties, heights);
    }
    auto id = model.entities.add(std::move(entity));
    EXPECT_TRUE(id.ok()) << (id.ok() ? "" : id.error().describe());
    return id.ok() ? *id : katana::entity::kInvalidEntityId;
}

std::string written(const katana::entity::Model& model, const dxf::ExportOptions& options = {})
{
    auto out = dxf::writeDxf(model, options);
    EXPECT_TRUE(out.ok()) << (out.ok() ? "" : out.error().describe());
    return out.ok() ? out->text : std::string();
}

dxf::DxfImport roundTrip(const katana::entity::Model& model)
{
    const std::string text = written(model);
    auto back = dxf::readDxf(text);
    EXPECT_TRUE(back.ok()) << (back.ok() ? "" : back.error().describe());
    return back.ok() ? std::move(*back) : dxf::DxfImport{};
}

// How many times `record` (a "0" pair's value) starts a record.
std::size_t records(const std::string& text, const std::string& record)
{
    std::size_t count = 0;
    const std::string needle = "  0\n" + record + "\n";
    for (std::size_t at = text.find(needle); at != std::string::npos;
         at = text.find(needle, at + 1)) {
        ++count;
    }
    return count;
}

template <typename T> std::vector<const Entity*> ofKind(const dxf::DxfImport& imported)
{
    std::vector<const Entity*> out;
    for (const Entity& entity : imported.entities) {
        if (std::holds_alternative<T>(entity.geometry)) {
            out.push_back(&entity);
        }
    }
    return out;
}

} // namespace

TEST(DxfWriter, AClosedPolylineIsWrittenAsAClosedLwPolylineNeverAHatch)
{
    katana::entity::Model model;
    add(model, Polyline2{{Point2(0.0, 0.0), Point2(30.0, 0.0), Point2(30.0, 20.0),
                          Point2(0.0, 20.0)},
                         true});
    const std::string text = written(model);
    EXPECT_EQ(records(text, "LWPOLYLINE"), 1u);
    EXPECT_EQ(records(text, "HATCH"), 0u);
    // Flag 70 = 1: closed. The four vertices and nothing else.
    EXPECT_NE(text.find("AcDbPolyline\n 90\n4\n 70\n1\n"), std::string::npos);
    const auto back = dxf::readDxf(text);
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->entities.size(), 1u);
    const auto& polyline = std::get<Polyline2>(back->entities[0].geometry);
    EXPECT_TRUE(polyline.closed);
    ASSERT_EQ(polyline.vertices.size(), 4u);
    EXPECT_EQ(polyline.area(), 600.0);
}

TEST(DxfWriter, CirclesAndArcsAreWrittenAsTrueCurves)
{
    katana::entity::Model model;
    add(model, Circle2{Point2(15.0, 10.0), 5.0});
    add(model, Arc2{Point2(100.0, 200.0), 1000.0, 0.0, kPi / 6.0});
    const std::string text = written(model);
    EXPECT_EQ(records(text, "CIRCLE"), 1u);
    EXPECT_EQ(records(text, "ARC"), 1u);
    EXPECT_EQ(records(text, "LWPOLYLINE"), 0u);
    // Read back: the same curves, not chords.
    const auto back = dxf::readDxf(text);
    ASSERT_TRUE(back.ok());
    const auto circles = ofKind<Circle2>(*back);
    const auto arcs = ofKind<Arc2>(*back);
    ASSERT_EQ(circles.size(), 1u);
    ASSERT_EQ(arcs.size(), 1u);
    EXPECT_EQ(std::get<Circle2>(circles[0]->geometry), (Circle2{Point2(15.0, 10.0), 5.0}));
    const Arc2& arc = std::get<Arc2>(arcs[0]->geometry);
    EXPECT_EQ(arc.startAngle, 0.0);
    // 30 degrees, through degrees and back: within a unit in the last place.
    EXPECT_NEAR(arc.sweep, kPi / 6.0, 1e-15);
}

TEST(DxfWriter, AClockwiseArcIsWrittenCounterClockwiseFromItsOtherEnd)
{
    katana::entity::Model model;
    // From 90 degrees clockwise by 90: the curve from 0 to 90 counter-clockwise.
    add(model, Arc2{Point2(0.0, 0.0), 2.0, kPi / 2.0, -kPi / 2.0});
    const auto back = roundTrip(model);
    const auto arcs = ofKind<Arc2>(back);
    ASSERT_EQ(arcs.size(), 1u);
    const Arc2& arc = std::get<Arc2>(arcs[0]->geometry);
    EXPECT_NEAR(arc.startAngle, 0.0, 1e-15);
    EXPECT_NEAR(arc.sweep, kPi / 2.0, 1e-15);
    EXPECT_EQ(arc.radius, 2.0);
}

TEST(DxfWriter, EveryKindComesBackAsTheGeometryItWas)
{
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "Survey", .color = rgb(255, 0, 0)}).ok());
    add(model, PointGeometry{Point2(502000.125, 6250000.1)}, "Survey", {31.245});
    add(model, Segment2{Point2(0.1, 0.2), Point2(3.0, 4.0)});
    add(model, Arc2{Point2(650.0, 200.0), 8.0, 0.25, 1.5});
    add(model, Circle2{Point2(-7.5, 1.0 / 3.0), 0.6});
    add(model, Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 5.0)}, false});
    add(model, TextGeometry{Point2(5.0, 6.0), "RL 32.45° Böschung", 2.75, kPi / 2.0});
    const auto back = roundTrip(model);
    ASSERT_EQ(back.entities.size(), 6u);

    const auto& point = back.entities[0];
    EXPECT_EQ(std::get<PointGeometry>(point.geometry).position, Point2(502000.125, 6250000.1));
    EXPECT_EQ(katana::entity::heightsOf(point.properties, 1).front(), 31.245);
    EXPECT_EQ(point.layer, "Survey");
    // Coordinates are written with the shortest digits that read back the
    // same double: nothing is lost to formatting.
    EXPECT_EQ(std::get<Segment2>(back.entities[1].geometry), (Segment2{Point2(0.1, 0.2), Point2(3.0, 4.0)}));
    const Arc2& arc = std::get<Arc2>(back.entities[2].geometry);
    EXPECT_EQ(arc.center, Point2(650.0, 200.0));
    EXPECT_EQ(arc.radius, 8.0);
    // Through degrees and back: equal to a few units in the last place.
    EXPECT_NEAR(arc.startAngle, 0.25, 1e-14);
    EXPECT_NEAR(arc.sweep, 1.5, 1e-14);
    EXPECT_EQ(std::get<Circle2>(back.entities[3].geometry), (Circle2{Point2(-7.5, 1.0 / 3.0), 0.6}));
    const Polyline2& polyline = std::get<Polyline2>(back.entities[4].geometry);
    EXPECT_FALSE(polyline.closed);
    EXPECT_EQ(polyline.vertices.size(), 3u);
    const auto& text = std::get<TextGeometry>(back.entities[5].geometry);
    EXPECT_EQ(text.text, "RL 32.45° Böschung");
    EXPECT_EQ(text.position, Point2(5.0, 6.0));
    EXPECT_EQ(text.height, 2.75);
    EXPECT_NEAR(text.rotation, kPi / 2.0, 1e-15);
}

TEST(DxfWriter, APolylineWhoseHeightsDifferIsAThreeDimensionalPolyline)
{
    katana::entity::Model model;
    add(model, Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 10.0)}, true}, "0",
        {10.0, 11.0, 12.5});
    add(model, Polyline2{{Point2(0.0, 0.0), Point2(5.0, 5.0)}, false}, "0", {3.5, 3.5});
    const std::string text = written(model);
    EXPECT_EQ(records(text, "POLYLINE"), 1u);
    EXPECT_EQ(records(text, "VERTEX"), 3u);
    EXPECT_EQ(records(text, "SEQEND"), 1u);
    EXPECT_EQ(records(text, "LWPOLYLINE"), 1u); // one height is an elevation
    const auto back = dxf::readDxf(text);
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->entities.size(), 2u);
    const auto& threeD = std::get<Polyline2>(back->entities[0].geometry);
    EXPECT_TRUE(threeD.closed);
    const auto heights = katana::entity::heightsOf(back->entities[0].properties, 3);
    EXPECT_EQ(heights[0], 10.0);
    EXPECT_EQ(heights[1], 11.0);
    EXPECT_EQ(heights[2], 12.5);
    EXPECT_EQ(katana::entity::heightsOf(back->entities[1].properties, 2)[1], 3.5);
}

TEST(DxfWriter, LayersAreWrittenWithTheirColoursVisibilityLockAndWeight)
{
    katana::entity::Model model;
    katana::entity::Layer kerb{.name = "Kerb", .color = rgb(0, 0, 255)};
    kerb.visible = false;
    kerb.locked = true;
    kerb.lineWeight = 0.5;
    ASSERT_TRUE(model.layers.add(kerb).ok());
    add(model, Segment2{Point2(0.0, 0.0), Point2(1.0, 0.0)}, "Kerb");
    const auto back = roundTrip(model);
    const auto found = std::find_if(back.layers.begin(), back.layers.end(),
                                    [](const auto& layer) { return layer.name == "Kerb"; });
    ASSERT_NE(found, back.layers.end());
    EXPECT_EQ(found->color, rgb(0, 0, 255));
    EXPECT_FALSE(found->visible);
    EXPECT_TRUE(found->locked);
    EXPECT_EQ(found->lineWeight, 0.5);
    EXPECT_EQ(back.entities.front().layer, "Kerb");
}

TEST(DxfWriter, ANestedLayerPathIsALegalNameAndComesBackThroughTheExtendedData)
{
    EXPECT_EQ(dxf::layerNameFor("survey/kerb/top"), "survey$kerb$top");
    EXPECT_EQ(dxf::layerNameFor("a<b>c:d"), "a_b_c_d");
    EXPECT_EQ(dxf::layerNameFor(""), "_");
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "survey/kerb"}).ok());
    add(model, Segment2{Point2(0.0, 0.0), Point2(1.0, 0.0)}, "survey/kerb");
    const std::string text = written(model);
    EXPECT_NE(text.find("  2\nsurvey$kerb\n"), std::string::npos);
    EXPECT_EQ(text.find("  2\nsurvey/kerb\n"), std::string::npos);
    const auto back = dxf::readDxf(text);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(back->entities.front().layer, "survey/kerb");
}

TEST(DxfWriter, LinetypesAreWrittenAndReadBack)
{
    katana::entity::Model model;
    katana::entity::Linetype dashed;
    dashed.name = "kerb dash";
    dashed.description = "Kerb __ . __";
    dashed.pattern = {{2.0}, {-0.5}, {0.0}, {-0.5}};
    ASSERT_TRUE(model.linetypes.add(dashed).ok());
    katana::entity::Layer kerb{.name = "Kerb"};
    kerb.linetype = "kerb dash";
    ASSERT_TRUE(model.layers.add(kerb).ok());
    add(model, Segment2{Point2(0.0, 0.0), Point2(1.0, 0.0)}, "Kerb");
    const auto back = roundTrip(model);
    ASSERT_EQ(back.linetypes.size(), 1u);
    EXPECT_EQ(back.linetypes[0].name, "kerb dash");
    EXPECT_EQ(back.linetypes[0].pattern, dashed.pattern);
    const auto found = std::find_if(back.layers.begin(), back.layers.end(),
                                    [](const auto& layer) { return layer.name == "Kerb"; });
    ASSERT_NE(found, back.layers.end());
    EXPECT_EQ(found->linetype, "kerb dash");
}

TEST(DxfWriter, AColourNoIndexIsGoesOutAsTheNearestAndComesBackExact)
{
    // (250,5,5) is no indexed colour; the nearest is 1, red, which is what
    // every other program draws. The colour itself rides along as extended
    // data, so it comes back to Katana as it went out - for an entity and for
    // a layer alike.
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "Kerb", .color = rgb(10, 200, 30)}).ok());
    Entity entity;
    entity.geometry = Segment2{Point2(0.0, 0.0), Point2(1.0, 0.0)};
    entity.layer = "Kerb";
    entity.color = rgb(250, 5, 5);
    ASSERT_TRUE(model.entities.add(entity).ok());
    const std::string text = written(model);
    EXPECT_NE(text.find(" 62\n1\n"), std::string::npos);
    EXPECT_NE(text.find("1000\ncolour\n1000\n#FA0505\n"), std::string::npos);
    const auto back = dxf::readDxf(text);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(back->entities.front().color, rgb(250, 5, 5));
    const auto kerb = std::find_if(back->layers.begin(), back->layers.end(),
                                   [](const auto& layer) { return layer.name == "Kerb"; });
    ASSERT_NE(kerb, back->layers.end());
    EXPECT_EQ(kerb->color, rgb(10, 200, 30));
}

TEST(DxfWriter, AnIndexedColourNeedsNoExtendedData)
{
    katana::entity::Model model;
    Entity entity;
    entity.geometry = Segment2{Point2(0.0, 0.0), Point2(1.0, 0.0)};
    entity.color = rgb(255, 0, 0); // index 1 exactly
    ASSERT_TRUE(model.entities.add(entity).ok());
    const std::string text = written(model);
    EXPECT_EQ(text.find("1000\ncolour\n"), std::string::npos);
    EXPECT_EQ(dxf::readDxf(text)->entities.front().color, rgb(255, 0, 0));
}

TEST(DxfWriter, TextWithALineBreakIsOneTextALine)
{
    katana::entity::Model model;
    add(model, TextGeometry{Point2(0.0, 0.0), "LOT 42\nDP 1234", 3.0, 0.0});
    const auto back = roundTrip(model);
    ASSERT_EQ(back.entities.size(), 2u);
    // The second line five thirds of a height (5) lower.
    EXPECT_EQ(std::get<TextGeometry>(back.entities[1].geometry).text, "DP 1234");
    EXPECT_NEAR(std::get<TextGeometry>(back.entities[1].geometry).position.y, -5.0, 1e-12);
}

TEST(DxfWriter, ADimensionIsWrittenAsTheLinesAndTextItDraws)
{
    katana::entity::Model model;
    add(model, katana::entity::DimensionGeometry{Point2(0.0, 0.0), Point2(10.0, 0.0), 5.0, ""});
    auto out = dxf::writeDxf(model);
    ASSERT_TRUE(out.ok());
    // Two extension lines, the dimension line, two ticks, and the text.
    EXPECT_EQ(out->dxfEntitiesWritten, 6u);
    EXPECT_EQ(records(out->text, "LINE"), 5u);
    EXPECT_EQ(records(out->text, "TEXT"), 1u);
    // The standard style's three decimals.
    EXPECT_NE(out->text.find("  1\n10.000\n"), std::string::npos);
}

TEST(DxfWriter, TheOriginShiftIsAddedBack)
{
    katana::entity::Model model;
    add(model, PointGeometry{Point2(1.0, 2.0)});
    dxf::ExportOptions options;
    options.originShift = katana::geometry::Vec2(500000.0, 6000000.0);
    auto out = dxf::writeDxf(model, options);
    ASSERT_TRUE(out.ok());
    const auto back = dxf::readDxf(out->text);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(std::get<PointGeometry>(back->entities[0].geometry).position,
              Point2(500001.0, 6000002.0));
}

TEST(DxfWriter, OnlyTheChosenEntitiesAndLayersAreWritten)
{
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "A"}).ok());
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "B"}).ok());
    const auto first = add(model, PointGeometry{Point2(1.0, 1.0)}, "A");
    add(model, PointGeometry{Point2(2.0, 2.0)}, "A");
    add(model, PointGeometry{Point2(3.0, 3.0)}, "B");
    dxf::ExportOptions byId;
    byId.entities = {first};
    EXPECT_EQ(dxf::writeDxf(model, byId)->entitiesWritten, 1u);
    dxf::ExportOptions byLayer;
    byLayer.layers = {"B"};
    const auto out = dxf::writeDxf(model, byLayer);
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(out->entitiesWritten, 1u);
    EXPECT_EQ(out->layersWritten, 2u); // B, and 0 which every file has
}

TEST(DxfWriter, TheFileHasTheStructureOfAnR2000Drawing)
{
    katana::entity::Model model;
    add(model, Segment2{Point2(0.0, 0.0), Point2(1.0, 0.0)});
    const std::string text = written(model);
    EXPECT_TRUE(text.starts_with("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1015\n"));
    EXPECT_TRUE(text.ends_with("  0\nEOF\n"));
    for (const char* table : {"VPORT", "LTYPE", "LAYER", "STYLE", "VIEW", "UCS", "APPID",
                              "DIMSTYLE", "BLOCK_RECORD"}) {
        EXPECT_NE(text.find(std::string("  0\nTABLE\n  2\n") + table + "\n"), std::string::npos)
            << table;
    }
    for (const char* section : {"HEADER", "CLASSES", "TABLES", "BLOCKS", "ENTITIES", "OBJECTS"}) {
        EXPECT_NE(text.find(std::string("  0\nSECTION\n  2\n") + section + "\n"),
                  std::string::npos)
            << section;
    }
    // $HANDSEED is above every handle used: the line is handle 0x30 + 1
    // layer (0) = 0x31, so the seed is 0x32.
    EXPECT_NE(text.find("$HANDSEED\n  5\n32\n"), std::string::npos);
}

TEST(DxfWriter, WritingToAFileReplacesItWhole)
{
    katana::entity::Model model;
    add(model, Circle2{Point2(0.0, 0.0), 1.0});
    const auto path = std::filesystem::temp_directory_path() / "katana_dxf_writer_test.dxf";
    {
        std::ofstream stale(path, std::ios::binary);
        stale << std::string(100000, 'x');
    }
    auto out = dxf::writeDxfFile(model, path);
    ASSERT_TRUE(out.ok()) << out.error().describe();
    EXPECT_EQ(std::filesystem::file_size(path), out->bytesWritten);
    const auto back = dxf::readDxfFile(path);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(back->entities.size(), 1u);
    std::filesystem::remove(path);
}

TEST(DxfWriter, AClosedPolylineThatRepeatsItsFirstVertexComesBackAsItWas)
{
    // The real drawing this module was measured on holds one: 19 vertices,
    // closed, the last the first again. Dropped as redundant, it was the only
    // one of 27 886 entities not to come back as it went out.
    katana::entity::Model model;
    const Polyline2 ring{{Point2(0.0, 0.0), Point2(4.0, 0.0), Point2(4.0, 3.0), Point2(0.0, 0.0)},
                         true};
    add(model, ring);
    const auto back = roundTrip(model);
    ASSERT_EQ(back.entities.size(), 1u);
    EXPECT_EQ(std::get<Polyline2>(back.entities[0].geometry), ring);
}

TEST(DxfWriter, HeightsKnownAtOnlySomeVerticesGoOutInPlanAndComeBackWithTheirGaps)
{
    // No height is not zero: written at Z 0, the gap would be a false level
    // the moment a surface is built from the file. So the polyline and the
    // line go out flat, their heights beside them, and come back as they were.
    katana::entity::Model model;
    add(model, Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 10.0)}, false}, "0",
        {31.25, std::nullopt, 32.5});
    add(model, Segment2{Point2(0.0, 0.0), Point2(5.0, 0.0)}, "0", {std::nullopt, 12.0});
    auto out = dxf::writeDxf(model);
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(records(out->text, "POLYLINE"), 0u); // no 3D polyline: it would need every Z
    EXPECT_EQ(records(out->text, "LWPOLYLINE"), 1u);
    EXPECT_NE(out->text.find("1000\n31.25 null 32.5\n"), std::string::npos);
    ASSERT_FALSE(out->warnings.empty());
    const auto back = dxf::readDxf(out->text);
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->entities.size(), 2u);
    const auto polyline = katana::entity::heightsOf(back->entities[0].properties, 3);
    EXPECT_EQ(polyline[0], 31.25);
    EXPECT_FALSE(polyline[1].has_value());
    EXPECT_EQ(polyline[2], 32.5);
    const auto line = katana::entity::heightsOf(back->entities[1].properties, 2);
    EXPECT_FALSE(line[0].has_value());
    EXPECT_EQ(line[1], 12.0);
}

TEST(DxfWriter, AnArcThatClimbsGoesOutWithBothEndHeightsBesideItAndComesBackWithThem)
{
    // An arc on a grade keeps a height at each end; ARC has one Z. So it
    // goes out in plan with both beside it, and is counted in the warning.
    // The clockwise arc goes out from its other end (counter-clockwise from
    // 0 to 90 degrees about (50,0)), so its heights go out, and come back,
    // in that order: 12 at the start, 10 at the end. An arc at one height
    // needs nothing beside it.
    katana::entity::Model model;
    add(model, Arc2{Point2(0.0, 0.0), 10.0, 0.0, kPi / 2.0}, "0", {31.5, 32.25});
    add(model, Arc2{Point2(50.0, 0.0), 5.0, kPi / 2.0, -kPi / 2.0}, "0", {10.0, 12.0});
    add(model, Arc2{Point2(100.0, 0.0), 5.0, 0.0, kPi / 2.0}, "0", {5.0, 5.0});
    auto out = dxf::writeDxf(model);
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(records(out->text, "ARC"), 3u);
    EXPECT_NE(out->text.find("1000\n31.5 32.25\n"), std::string::npos);
    EXPECT_NE(out->text.find("1000\n12 10\n"), std::string::npos);
    ASSERT_EQ(out->warnings.size(), 1u);
    EXPECT_EQ(out->warnings[0].rfind("2 entities with heights the format cannot hold", 0), 0u)
        << out->warnings[0];
    const auto back = dxf::readDxf(out->text);
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->entities.size(), 3u);
    const auto climbing = katana::entity::heightsOf(back->entities[0].properties, 2);
    EXPECT_EQ(climbing[0], 31.5);
    EXPECT_EQ(climbing[1], 32.25);
    const Arc2& turned = std::get<Arc2>(back->entities[1].geometry);
    EXPECT_NEAR(turned.startAngle, 0.0, 1e-15);
    const auto reversed = katana::entity::heightsOf(back->entities[1].properties, 2);
    EXPECT_EQ(reversed[0], 12.0);
    EXPECT_EQ(reversed[1], 10.0);
    const auto level = katana::entity::heightsOf(back->entities[2].properties, 2);
    EXPECT_EQ(level[0], 5.0);
    EXPECT_EQ(level[1], 5.0);
}

TEST(DxfWriter, ALevelOfExactlyZeroComesBackAsZeroNotAsNoLevel)
{
    // Z 0 is how every program writes "in plan", so the reader takes it as
    // no level; a surveyed level of 0.000 must be said beside the entity or
    // it comes back unsurveyed. The one height "0" stands for every vertex.
    // Level 5 and no level at all are the controls.
    katana::entity::Model model;
    add(model, PointGeometry{Point2(10.0, 10.0)}, "0", {0.0});
    add(model, PointGeometry{Point2(20.0, 20.0)}, "0", {5.0});
    add(model, PointGeometry{Point2(30.0, 30.0)});
    add(model, Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 10.0)}, true}, "0",
        {0.0, 0.0, 0.0});
    add(model, Segment2{Point2(0.0, 0.0), Point2(5.0, 0.0)}, "0", {0.0, 0.0});
    add(model, Circle2{Point2(40.0, 0.0), 2.0}, "0", {0.0});
    add(model, Arc2{Point2(60.0, 0.0), 2.0, 0.0, kPi / 2.0}, "0", {0.0, 0.0});
    add(model, TextGeometry{Point2(70.0, 0.0), "RL 0.000", 2.5, 0.0}, "0", {0.0});
    auto out = dxf::writeDxf(model);
    ASSERT_TRUE(out.ok());
    // Nothing is lost to another program - its Z 0 is the level - so there
    // is nothing to warn of.
    EXPECT_TRUE(out->warnings.empty());
    const auto back = dxf::readDxf(out->text);
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->entities.size(), 8u);
    const auto& e = back->entities;
    EXPECT_EQ(katana::entity::heightsOf(e[0].properties, 1)[0], 0.0);
    EXPECT_EQ(katana::entity::heightsOf(e[1].properties, 1)[0], 5.0);
    EXPECT_FALSE(katana::entity::heightsOf(e[2].properties, 1)[0].has_value());
    for (const auto& height : katana::entity::heightsOf(e[3].properties, 3)) {
        EXPECT_EQ(height, 0.0);
    }
    for (const auto& height : katana::entity::heightsOf(e[4].properties, 2)) {
        EXPECT_EQ(height, 0.0);
    }
    EXPECT_EQ(katana::entity::heightsOf(e[5].properties, 1)[0], 0.0);
    for (const auto& height : katana::entity::heightsOf(e[6].properties, 2)) {
        EXPECT_EQ(height, 0.0);
    }
    EXPECT_EQ(katana::entity::heightsOf(e[7].properties, 1)[0], 0.0);
}

// ---- annotation (docs/annotation.md, "Exchange") ---------------------------------------

namespace {

// A model with a point and a label of it in a point-number style.
katana::entity::Model labelledPoint(katana::entity::EntityId& label)
{
    katana::entity::Model model;
    katana::entity::LabelStyle style;
    style.name = "pt";
    style.kind = katana::entity::LabelKind::Point;
    style.text = "{point}";
    EXPECT_TRUE(model.labelStyles.add(style).ok());
    const auto point = add(model, PointGeometry{Point2(1.0, 1.0)});
    label = add(model, katana::entity::LabelGeometry{.target = point, .style = "pt",
                                                     .anchor = Point2(1.0, 1.0)});
    return model;
}

} // namespace

TEST(DxfWriter, PaperSizedTextIsWrittenAtTheAnnotationScale)
{
    katana::entity::Model model;
    TextGeometry note{Point2(0.0, 0.0), "NOTE", 2.5, 0.0};
    note.paperHeight = 3.5;
    add(model, note);
    dxf::ExportOptions options;
    options.annotationScale = 500.0;
    const auto back = dxf::readDxf(written(model, options));
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->entities.size(), 1u);
    // 3.5 mm at 1:500 is 1.75 m; the model height is not what the sheet shows.
    EXPECT_DOUBLE_EQ(std::get<TextGeometry>(back->entities[0].geometry).height, 1.75);
}

TEST(DxfWriter, WithNothingDrawnALabelIsSkippedAndSaidSo)
{
    katana::entity::EntityId label = 0;
    const auto model = labelledPoint(label);
    const auto out = dxf::writeDxf(model);
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(out->entitiesWritten, 1u) << "the point; the label is not counted written";
    EXPECT_EQ(out->entitiesSkipped, 1u);
    ASSERT_FALSE(out->warnings.empty());
    EXPECT_NE(out->warnings.front().find("1 labels were not written"), std::string::npos);
    EXPECT_EQ(records(out->text, "TEXT"), 0u);
}

TEST(DxfWriter, DrawnAnnotationIsWrittenAsItsShapesOnItsLayer)
{
    katana::entity::EntityId label = 0;
    auto model = labelledPoint(label);
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "Labels"}).ok());
    Entity moved = *model.entities.find(label);
    moved.layer = "Labels";
    ASSERT_TRUE(model.entities.replace(moved).ok());
    // What cad::annotation::drawAnnotationForExport would hand over: the
    // label's text and a leader stroke.
    const std::map<katana::entity::EntityId, std::vector<katana::entity::Geometry>> drawn = {
        {label,
         {TextGeometry{Point2(2.0, 2.0), "P1", 1.25, 0.0}, Segment2{Point2(1.0, 1.0), Point2(2.0, 2.0)}}},
    };
    dxf::ExportOptions options;
    options.drawn = &drawn;
    const auto out = dxf::writeDxf(model, options);
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(out->entitiesWritten, 2u);
    EXPECT_EQ(out->entitiesSkipped, 0u);
    EXPECT_TRUE(out->warnings.empty());
    const auto back = dxf::readDxf(out->text);
    ASSERT_TRUE(back.ok());
    const auto texts = ofKind<TextGeometry>(*back);
    ASSERT_EQ(texts.size(), 1u);
    EXPECT_EQ(std::get<TextGeometry>(texts[0]->geometry).text, "P1");
    EXPECT_EQ(texts[0]->layer, "Labels");
    EXPECT_EQ(ofKind<Segment2>(*back).size(), 1u);
}

TEST(DxfWriter, ALabelDrawnAsNothingIsLeftOutAndSaidSo)
{
    katana::entity::EntityId label = 0;
    const auto model = labelledPoint(label);
    const std::map<katana::entity::EntityId, std::vector<katana::entity::Geometry>> drawn = {
        {label, {}}};
    dxf::ExportOptions options;
    options.drawn = &drawn;
    options.annotationScale = 2000.0;
    const auto out = dxf::writeDxf(model, options);
    ASSERT_TRUE(out.ok());
    ASSERT_EQ(out->warnings.size(), 1u);
    EXPECT_NE(out->warnings.front().find("1 labels had no room at 1:2000"), std::string::npos);
    EXPECT_EQ(records(out->text, "TEXT"), 0u);
}

TEST(DxfWriter, WithNothingDrawnALeaderIsItsLineLandingAndNote)
{
    katana::entity::Model model;
    katana::entity::LeaderGeometry leader;
    leader.vertices = {Point2(0.0, 0.0), Point2(10.0, 10.0)};
    leader.text = "PIT 12";
    add(model, leader);
    const auto out = dxf::writeDxf(model);
    ASSERT_TRUE(out.ok());
    // The segment and the landing, and the note.
    EXPECT_EQ(records(out->text, "LINE"), 2u);
    EXPECT_EQ(records(out->text, "TEXT"), 1u);
}

// A smart leader's note is read off the entity its tip is on, here too
// (docs/annotation.md, "Smart leaders"): the file says what the drawing
// says, not the template.
TEST(DxfWriter, WithNothingDrawnASmartLeaderWritesTheNoteItReads)
{
    katana::entity::Model model;
    katana::entity::Entity pit;
    pit.geometry = katana::entity::PointGeometry{Point2(0.0, 0.0)};
    pit.properties["invert"] = 10.5;
    ASSERT_TRUE(model.entities.add(pit).ok());
    katana::entity::LeaderGeometry leader;
    leader.vertices = {Point2(0.0, 0.0), Point2(10.0, 10.0)};
    leader.text = "IL {prop.invert:.2f}\nRL {rl:.3f}";
    leader.fields = true;
    leader.tipRef = katana::entity::AnchorRef{1, katana::entity::AnchorPoint::Position};
    add(model, leader);
    const auto out = dxf::writeDxf(model);
    ASSERT_TRUE(out.ok());
    EXPECT_NE(out->text.find("IL 10.50"), std::string::npos);
    EXPECT_EQ(out->text.find("{prop.invert"), std::string::npos) << "never the template";
    EXPECT_EQ(records(out->text, "TEXT"), 1u) << "no line for a level the pit has not got";
}
