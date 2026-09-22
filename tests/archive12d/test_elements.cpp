#include <gtest/gtest.h>

#include <fstream>
#include <sstream>
#include <string>

#include "katana/archive12d/reader.hpp"

namespace a12 = katana::archive12d;
using katana::core::ErrorCode;

namespace {

a12::Archive read(const std::string& text)
{
    auto archive = a12::readArchive(text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    return archive.ok() ? std::move(*archive) : a12::Archive{};
}

template <typename T> const T& only(const a12::Archive& archive)
{
    EXPECT_EQ(archive.elements.size(), 1u);
    return std::get<T>(archive.elements.at(0));
}

std::string fixture(const std::string& name)
{
    std::ifstream file(std::string(KATANA_ARCHIVE12D_TEST_DATA) + "/" + name, std::ios::binary);
    EXPECT_TRUE(file.good()) << name;
    std::ostringstream bytes;
    bytes << file.rdbuf();
    return bytes.str();
}

} // namespace

// ---- simple strings (manual 1.5.1 - 1.5.7, 1.5.10) -----------------------------

TEST(ReaderStrings, AnArcIsItsCentreItsEndsAndItsRadius)
{
    const auto archive = read("string arc { name \"A\" colour blue chainage 5 interval 1 radius -15\n"
                              " xcentre 600 ycentre 100 zcentre 31 xstart 615 ystart 100 zstart 31\n"
                              " xend 600 yend 115 zend 31.5 }");
    const auto& arc = only<a12::ArcString>(archive);
    EXPECT_EQ(arc.header.name, "A");
    EXPECT_EQ(arc.header.chainage, 5.0);
    EXPECT_EQ(arc.radius, -15.0);
    EXPECT_EQ(arc.centre, (a12::Vertex{600.0, 100.0, 31.0}));
    EXPECT_EQ(arc.start, (a12::Vertex{615.0, 100.0, 31.0}));
    EXPECT_EQ(arc.end, (a12::Vertex{600.0, 115.0, 31.5}));
    EXPECT_EQ(arc.header.extras.real("interval"), 1.0);
    EXPECT_FALSE(arc.header.extras.contains("radius")) << "a modelled field is not also an extra";
}

TEST(ReaderStrings, ACircleAndAFeatureDifferOnlyInName)
{
    const auto archive = read("string circle { radius 8 zcentre 32 xcentre 650 ycentre 200 }\n"
                              "string feature { radius 3 zcentre null xcentre 700 ycentre 250 }");
    ASSERT_EQ(archive.elements.size(), 2u);
    const auto& circle = std::get<a12::CircleString>(archive.elements[0]);
    const auto& feature = std::get<a12::CircleString>(archive.elements[1]);
    EXPECT_FALSE(circle.feature);
    EXPECT_EQ(circle.radius, 8.0);
    EXPECT_EQ(circle.centre, (a12::Vertex{650.0, 200.0, 32.0}));
    EXPECT_TRUE(feature.feature);
    EXPECT_FALSE(feature.centre.z.has_value());
}

TEST(ReaderStrings, AFaceKeepsItsHatchingAndAnInterfaceItsCutFillModes)
{
    const auto archive = read("string face { hatch_angle 45 hatch_distance 2 hatch_colour red"
                              " edge_colour blue fill_mode 1 edge_mode 0 data { 0 0 1  4 0 1  4 3 1 } }\n"
                              "string interface { data { 0 0 1 -1   5 0 2 0   9 0 3 1 } }");
    ASSERT_EQ(archive.elements.size(), 2u);
    const auto& face = std::get<a12::VertexString>(archive.elements[0]);
    EXPECT_EQ(face.kind, a12::StringKind::Face);
    EXPECT_EQ(face.vertices.size(), 3u);
    EXPECT_EQ(face.header.extras.real("hatch_angle"), 45.0);
    EXPECT_EQ(face.header.extras.text("hatch_colour"), "red");
    EXPECT_EQ(face.header.extras.integer("fill_mode"), 1);

    const auto& interface12d = std::get<a12::VertexString>(archive.elements[1]);
    EXPECT_EQ(interface12d.kind, a12::StringKind::Interface);
    ASSERT_EQ(interface12d.vertices.size(), 3u);
    EXPECT_EQ(interface12d.vertices[2], (a12::Vertex{9.0, 0.0, 3.0}));
    EXPECT_EQ(interface12d.interfaceModes, (std::vector<int>{-1, 0, 1}));
}

TEST(ReaderStrings, ATextStringKeepsHowItIsDrawnApartFromWhereAndWhat)
{
    const auto archive = read("string text { name \"\" colour \"pen 025\" worldsize 2.7 textstyle \"Arial Narrow\"\n"
                              " angle 30 offset 3 raise -2.5 justify \"bottom-left\" time_created \"08-Nov-2025 06:05:42\"\n"
                              " x 307416.316 y 6270203.132 z null text \"SS112061\" }");
    const auto& text = only<a12::TextString>(archive);
    EXPECT_EQ(text.text, "SS112061");
    EXPECT_EQ(text.position, (a12::Vertex{307416.316, 6270203.132, std::nullopt}));
    EXPECT_EQ(text.header.colour, "pen 025");
    EXPECT_EQ(text.annotation.real("worldsize"), 2.7);
    EXPECT_EQ(text.annotation.real("angle"), 30.0);
    EXPECT_EQ(text.annotation.text("justify"), "bottom-left");
    EXPECT_FALSE(text.annotation.contains("x"));
    EXPECT_FALSE(text.annotation.contains("text"));
    EXPECT_EQ(text.header.extras.text("time_created"), "08-Nov-2025 06:05:42");
}

TEST(ReaderStrings, TheOneLineTextFormOtherToolsWriteIsRead)
{
    const auto archive = read("model \"Labels\"\ntext \"PM001 RL=32.451\" 502000 6960000 32.5\n"
                              "text \"no height\" 1 2\nstring super { data_2d { 0 0 } }");
    ASSERT_EQ(archive.elements.size(), 3u);
    const auto& first = std::get<a12::TextString>(archive.elements[0]);
    EXPECT_EQ(first.text, "PM001 RL=32.451");
    EXPECT_EQ(first.position, (a12::Vertex{502000.0, 6960000.0, 32.5}));
    EXPECT_EQ(first.header.model, "Labels");
    EXPECT_FALSE(std::get<a12::TextString>(archive.elements[1]).position.z.has_value());
}

TEST(ReaderStrings, APlotFrameIsItsFields)
{
    const auto archive = read("string plot_frame { name \"Sheet 01\" border 1 width 420 height 297 scale 500\n"
                              " rotation 0 xorigin 503600 yorigin 6960000 colour white title_1 \"Plan\" }");
    const auto& frame = only<a12::PlotFrame>(archive);
    EXPECT_EQ(frame.header.name, "Sheet 01");
    EXPECT_EQ(frame.header.colour, "white");
    EXPECT_EQ(frame.fields.real("width"), 420.0);
    EXPECT_EQ(frame.fields.real("scale"), 500.0);
    EXPECT_EQ(frame.fields.text("title_1"), "Plan");
}

// ---- drainage (manual 1.5.3) ------------------------------------------------------

TEST(ReaderDrainage, PitsPipesControlsAndConnectionsStayOutOfTheLinesGeometry)
{
    const auto archive = read(R"(string drainage {
  name "SW Line A" outfall 28.5 flow_direction 1
  attributes { text Tin "DESIGN" integer "_floating" 0 }
  data { 0 0 29 0 0   50 20 29.5 0 0   100 40 30 0 0 }
  pit { name "A1" type "Grated" diameter 1.05 floating no x 0 y 0 z 31 }
  pit { name "A2" x 50 y 20 z 31.5 attributes { text "Lid" "Class D" } }
  pipe { name "A1-A2" type "RCP" diameter 0.375 us_level 29.5 ds_level 29 }
  property_control { name "Lot 4" grade 60 x 25 y 10 z 29.2 data { 25 10 29.2 0 0   30 40 30 0 0 } }
  house_connection { name "HC1" hcb 7 side left x 40 y 16 z 29.4 }
})");
    const auto& d = only<a12::DrainageString>(archive);
    EXPECT_EQ(d.outfall, 28.5);
    EXPECT_EQ(d.flowDirection, 1);
    ASSERT_EQ(d.vertices.size(), 3u) << "coordinates inside pits must not leak into the line";
    EXPECT_EQ(d.vertices[1], (a12::Vertex{50.0, 20.0, 29.5}));
    EXPECT_EQ(std::get<std::string>(d.header.attributes.at(0).value), "DESIGN");
    ASSERT_EQ(d.pits.size(), 2u);
    EXPECT_FALSE(d.pitsAreVersion2);
    EXPECT_EQ(d.pits[0].fields.text("name"), "A1");
    EXPECT_EQ(d.pits[0].fields.real("diameter"), 1.05);
    EXPECT_EQ(d.pits[0].fields.boolean("floating"), false);
    ASSERT_EQ(d.pits[1].attributes.size(), 1u);
    ASSERT_EQ(d.pipes.size(), 1u);
    EXPECT_EQ(d.pipes[0].fields.real("us_level"), 29.5);
    ASSERT_EQ(d.propertyControls.size(), 1u);
    EXPECT_EQ(d.propertyControls[0].fields.real("grade"), 60.0);
    ASSERT_EQ(d.propertyControls[0].vertices.size(), 2u);
    EXPECT_EQ(d.propertyControls[0].vertices[1], (a12::Vertex{30.0, 40.0, 30.0}));
    ASSERT_EQ(d.houseConnections.size(), 1u);
    EXPECT_EQ(d.houseConnections[0].fields.integer("hcb"), 7);
}

TEST(ReaderDrainage, VersionTwoPitsAreRememberedAsSuch)
{
    const auto archive = read("string drainage { data { 0 0 1 0 0 } pit_v2 { name \"P\" riser_enabled true"
                              " riser_colour dark green base_height 0.3 x 0 y 0 z 2 } }");
    const auto& d = only<a12::DrainageString>(archive);
    EXPECT_TRUE(d.pitsAreVersion2);
    EXPECT_EQ(d.pits.at(0).fields.text("riser_colour"), "dark green");
    EXPECT_EQ(d.pits.at(0).fields.real("base_height"), 0.3);
}

// ---- alignments (manual 1.5.9, 1.5.16, 1.5.17) ------------------------------------------

TEST(ReaderAlignment, TheManualsOwnIpExampleIsReadPartByPart)
{
    // Manual 1.5.9.1.2 and 1.5.9.2.2, abbreviated to one of each kind.
    const auto archive = read(R"(string super_alignment {
  name "MC01" spiral_type clothoid valid_horizontal true valid_vertical false closed false
  horizontal_parts {
    ip { id 100 x 42606.66161172 y 37239.28824481 }
    spiral { id 300 r 50 l1 30 l2 40 x 43336.6595 y 37469.2563 }
    arc { id 400 r 75 x 43481.15324268 y 37331.6431906 }
  }
  vertical_parts {
    ip { id 600 x -50.8459652 y 159.79764161 }
    kvalue { id 700 k 1.25 x 38.4627 y 179.2126 }
    length { id 800 l 50 x 172.61694837 y 154.72967932 }
    asymmetric { id 900 l1 25 l2 75 x 270.0182 y 208.1493 }
    arc { id 1000 r 1000 x 424.2402 y 196.5637 }
    radius { id 1100 r 200 x 526.7263 y 201.5302 }
  }
})");
    const auto& a = only<a12::SuperAlignment>(archive);
    EXPECT_EQ(a.source, a12::AlignmentSource::SuperAlignment);
    EXPECT_EQ(a.spiralType, "clothoid");
    EXPECT_EQ(a.validHorizontal, true);
    EXPECT_EQ(a.validVertical, false);
    ASSERT_EQ(a.horizontalParts.size(), 3u);
    EXPECT_EQ(a.horizontalParts[1].kind, "spiral");
    EXPECT_EQ(a.horizontalParts[1].fields.real("l2"), 40.0);
    EXPECT_EQ(a.horizontalParts[2].fields.real("x"), 43481.15324268);
    ASSERT_EQ(a.verticalParts.size(), 6u);
    EXPECT_EQ(a.verticalParts[1].fields.real("k"), 1.25);
    EXPECT_EQ(a.verticalParts[3].kind, "asymmetric");
    EXPECT_TRUE(a.horizontalIsIpOnly());
    EXPECT_TRUE(a.verticalIsIpOnly());
}

TEST(ReaderAlignment, SolvedGeometryIsVerticesAndTheSegmentsBetweenThem)
{
    const auto archive = read(R"(string super_alignment { name "A"
  horizontal_data { name "A" closed 0 weight 1
    data_2d { 0 0  100 0  150 50 }
    geometry_data { straight { } arc { radius -50 major 0 } } }
  vertical_data { closed 0
    data_2d { 0 30  60 33  140 31 }
    geometry_data { straight { } parabola { chainage 100 height 35 } } }
})");
    const auto& a = only<a12::SuperAlignment>(archive);
    ASSERT_TRUE(a.horizontalData.has_value());
    EXPECT_EQ(a.horizontalData->vertices.size(), 3u);
    ASSERT_EQ(a.horizontalData->segments.size(), 2u);
    EXPECT_EQ(a.horizontalData->segments[1].radius, -50.0);
    EXPECT_EQ(a.horizontalData->header.real("weight"), 1.0);
    ASSERT_TRUE(a.verticalData.has_value());
    EXPECT_EQ(a.verticalData->vertices[1], (a12::Vertex{60.0, 33.0, std::nullopt}));
    EXPECT_EQ(a.verticalData->segments[1].kind, a12::SegmentKind::Parabola);
    EXPECT_EQ(a.verticalData->segments[1].parameters.real("height"), 35.0);
    EXPECT_FALSE(a.horizontalIsIpOnly()) << "no parts at all is not an IP definition";
}

TEST(ReaderAlignment, TwelveDsUndocumentedConstructionsAreKeptAndSaidNotToBeIps)
{
    const auto archive = read("string super_alignment { horizontal_parts {\n"
                              " ip { id 100 x 0 y 0 }\n"
                              " floating_arc_end_radius_length { id 200 radius 50 length 30 attach_to previous_part }\n"
                              "} drawables { labels { } } geometry_modifiers { } }");
    const auto& a = only<a12::SuperAlignment>(archive);
    ASSERT_EQ(a.horizontalParts.size(), 2u);
    EXPECT_EQ(a.horizontalParts[1].kind, "floating_arc_end_radius_length");
    EXPECT_EQ(a.horizontalParts[1].fields.real("radius"), 50.0);
    EXPECT_FALSE(a.horizontalIsIpOnly());
    EXPECT_EQ(archive.unrecognised.at("string super_alignment/drawables"), 1u);
    EXPECT_EQ(archive.unrecognised.at("string super_alignment/geometry_modifiers"), 1u);
}

TEST(ReaderAlignment, AnOldAlignmentStringsRowsBecomeTheSameIpParts)
{
    // Manual 1.5.16: "x y radius [spil1 L spil2 L]" and "ch z length [parabola]"
    // or "ch z radius circle".
    const auto archive = read(R"(string alignment { name "AL" spiral_type "clothoid" draw_mode 1
  hipdata { 0 0 0
            100 0 40
            150 100 60 spil1 20 spil2 25
            300 100 0 }
  vipdata { 0 30 0
            100 33 40 parabola
            160 32 35
            220 31 500 circle
            300 30 0 }
})");
    const auto& a = only<a12::SuperAlignment>(archive);
    EXPECT_EQ(a.source, a12::AlignmentSource::Alignment);
    ASSERT_EQ(a.horizontalParts.size(), 4u);
    EXPECT_EQ(a.horizontalParts[0].kind, "ip");
    EXPECT_EQ(a.horizontalParts[1].kind, "arc");
    EXPECT_EQ(a.horizontalParts[1].fields.real("r"), 40.0);
    EXPECT_EQ(a.horizontalParts[2].kind, "spiral");
    EXPECT_EQ(a.horizontalParts[2].fields.real("l1"), 20.0);
    EXPECT_EQ(a.horizontalParts[2].fields.real("l2"), 25.0);
    EXPECT_EQ(a.horizontalParts[2].fields.real("y"), 100.0);
    // "a number that is unique for each horizontal and vertical part ... a
    // multiple of 100" (manual 1.5.9.1.2).
    EXPECT_EQ(a.horizontalParts[3].fields.integer("id"), 400);
    ASSERT_EQ(a.verticalParts.size(), 5u);
    EXPECT_EQ(a.verticalParts[0].fields.integer("id"), 500);
    EXPECT_EQ(a.verticalParts[1].kind, "length");
    EXPECT_EQ(a.verticalParts[2].kind, "length") << "the word parabola is optional";
    EXPECT_EQ(a.verticalParts[2].fields.real("l"), 35.0);
    EXPECT_EQ(a.verticalParts[3].kind, "arc");
    EXPECT_EQ(a.verticalParts[3].fields.real("r"), 500.0);
    EXPECT_EQ(a.header.extras.integer("draw_mode"), 1);
}

TEST(ReaderAlignment, APipelineIsAnAlignmentWithASize)
{
    const auto archive = read("string pipeline { diameter 0.375 length 6 hipdata { 0 0 0  100 50 0 } }");
    const auto& a = only<a12::SuperAlignment>(archive);
    EXPECT_EQ(a.source, a12::AlignmentSource::Pipeline);
    EXPECT_EQ(a.diameter, 0.375);
    EXPECT_EQ(a.pipeLength, 6.0);
    EXPECT_EQ(a.horizontalParts.size(), 2u);
}

TEST(ReaderAlignment, AHipRowThatIsNotThreeNumbersIsAnError)
{
    EXPECT_FALSE(a12::readArchive("string alignment { hipdata { 0 0 0  100 } }").ok());
    EXPECT_FALSE(a12::readArchive("string alignment { hipdata { 0 0 0 spil1 } }").ok());
}

// ---- superseded strings (manual 1.5.11 - 1.5.15) --------------------------------------------

TEST(ReaderSuperseded, EachOldStringTypeArrivesAsTheSuperStringThatReplacedIt)
{
    const auto archive = read(R"(string 2d { z 12.5 data { 0 0  5 5 } }
string 3d { data { 0 0 1  5 5 2 } }
string 4d { angle 15 worldsize 2 textstyle "ISO" justify "middle-centre" data { 0 0 1 "T1"  5 5 null "" } }
string pipe { diameter 0.6 data { 0 0 1  5 5 2 } }
string polyline { closed 1 data { 0 0 1 12 0   20 0 1 0 0   20 20 1 0 0 } })");
    ASSERT_EQ(archive.elements.size(), 5u);
    const auto& twoD = std::get<a12::VertexString>(archive.elements[0]);
    EXPECT_EQ(twoD.kind, a12::StringKind::TwoD);
    EXPECT_EQ(twoD.constantZ, 12.5);
    EXPECT_EQ(twoD.vertices.size(), 2u);

    const auto& threeD = std::get<a12::VertexString>(archive.elements[1]);
    EXPECT_EQ(threeD.vertices[1], (a12::Vertex{5.0, 5.0, 2.0}));

    const auto& fourD = std::get<a12::VertexString>(archive.elements[2]);
    EXPECT_EQ(fourD.vertexText, (std::vector<std::string>{"T1", ""}));
    EXPECT_FALSE(fourD.vertices[1].z.has_value());
    ASSERT_TRUE(fourD.vertexAnnotation.has_value());
    EXPECT_EQ(fourD.vertexAnnotation->real("angle"), 15.0);
    EXPECT_EQ(fourD.vertexAnnotation->text("justify"), "middle-centre");
    EXPECT_TRUE(fourD.justify.empty()) << "a 4d string's justify is its text's, not a pipe's";

    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[3]).diameter, 0.6);

    const auto& polyline = std::get<a12::VertexString>(archive.elements[4]);
    EXPECT_TRUE(polyline.closed);
    ASSERT_EQ(polyline.segments.size(), 3u);
    EXPECT_EQ(polyline.segments[0].radius, 12.0);
}

// ---- LAS clouds (manual 1.5.18) ----------------------------------------------------------------

TEST(ReaderLas, TaggedAndCompactRecordsOfOneFormatGiveTheSamePoint)
{
    // Format 3 is format 1 (format 0 plus gps time) plus colour: x y z i rn rc
    // sd fe cl sr ud id t c.
    const auto archive = read(R"(string las_cloud_data { name "C" data {
  categories { true false } format v12_p3 range { xmin 1 xmax 2 ymin 3 ymax 4 }
  points_v12_p3 { p { x 10.5 y 20.5 z 3.25 i 1200 rn 2 rc 3 sd 1 fe 0 cl 6 sr -12 ud 9 id 77 t 1234.5 c 281474976710655 } }
  compact_points_v12_p3 { p { 10.5 20.5 3.25 1200 2 3 1 0 6 -12 9 77 1234.5 281474976710655 } }
} })");
    const auto& cloud = only<a12::LasCloud>(archive);
    EXPECT_EQ(cloud.format, "v12_p3");
    EXPECT_EQ(cloud.pointFormat, 3);
    EXPECT_EQ(cloud.categories, (std::vector<bool>{true, false}));
    EXPECT_EQ(cloud.range.real("ymax"), 4.0);
    ASSERT_EQ(cloud.points.size(), 2u);
    EXPECT_EQ(cloud.points[0], cloud.points[1]);
    const a12::LasPoint& p = cloud.points[0];
    EXPECT_EQ(p.x, 10.5);
    EXPECT_EQ(p.z, 3.25);
    EXPECT_EQ(p.intensity, 1200);
    EXPECT_EQ(p.returnNumber, 2);
    EXPECT_EQ(p.returnCount, 3);
    EXPECT_EQ(p.scanDirection, 1);
    EXPECT_EQ(p.classification, 6);
    EXPECT_EQ(p.scanAngle, -12);
    EXPECT_EQ(p.userData, 9);
    EXPECT_EQ(p.pointSourceId, 77);
    EXPECT_EQ(p.gpsTime, 1234.5);
    EXPECT_EQ(p.colour, 281474976710655ull) << "2^48 - 1: more than a double's 53 bits would survive, but only as an integer";
}

TEST(ReaderLas, TheLasOnePointFourRecordsPutUserDataBeforeScanAngle)
{
    // Manual 1.5.18 point_p6: x y z i rn rc cf sc sd fe cl ud sr id t; p8 adds
    // c and ir. In formats 0-5 the order is ... cl sr ud id.
    const auto archive = read("string las_cloud_data { data { format v14_p8\n"
                              " compact_points_v14_p8 { p { 1 2 3 100 5 6 7 2 1 0 4 200 -30 9 55.5 42 17 } } } }");
    const a12::LasPoint& p = only<a12::LasCloud>(archive).points.at(0);
    EXPECT_EQ(p.returnNumber, 5);
    EXPECT_EQ(p.returnCount, 6);
    EXPECT_EQ(p.classificationFlags, 7);
    EXPECT_EQ(p.scannerChannel, 2);
    EXPECT_EQ(p.scanDirection, 1);
    EXPECT_EQ(p.classification, 4);
    EXPECT_EQ(p.userData, 200);
    EXPECT_EQ(p.scanAngle, -30);
    EXPECT_EQ(p.pointSourceId, 9);
    EXPECT_EQ(p.gpsTime, 55.5);
    EXPECT_EQ(p.colour, 42u);
    EXPECT_EQ(p.nearInfrared, 17);
}

TEST(ReaderLas, ACloudMayNameALasFileInsteadOfHoldingPoints)
{
    const auto archive = read("string las_cloud_data { name \"R\" ref_data { categories { true }\n"
                              " file_name \"scans/site.las\" range { xmin 0 xmax 9 ymin 0 ymax 9 } } }");
    const auto& cloud = only<a12::LasCloud>(archive);
    EXPECT_EQ(cloud.referenceFile, "scans/site.las");
    EXPECT_TRUE(cloud.points.empty());
    EXPECT_EQ(cloud.range.real("xmax"), 9.0);
}

TEST(ReaderLas, ACompactRecordWithoutEvenAPositionIsAnError)
{
    EXPECT_FALSE(a12::readArchive("string las_cloud_data { data { format v10_p0\n"
                                  " compact_points_v10_p0 { p { 1 2 } } } }")
                     .ok());
}

// ---- tins (manual 1.4.7, 1.4.8) --------------------------------------------------------------------

TEST(ReaderTin, PointNumbersStartAtOneAndBecomeZeroBased)
{
    const auto archive = read("tin { name \"T\" colour green points { 0 0 1  10 0 2  0 10 3  10 10 null }\n"
                              " triangles { 1 3 2   2 3 4 } colours { -1 \"vis grass\" } }");
    const auto& tin = only<a12::Tin>(archive);
    EXPECT_FALSE(tin.full);
    EXPECT_EQ(tin.name, "T");
    EXPECT_EQ(tin.colour, "green");
    ASSERT_EQ(tin.points.size(), 4u);
    EXPECT_FALSE(tin.points[3].z.has_value());
    ASSERT_EQ(tin.triangles.size(), 2u);
    EXPECT_EQ(tin.triangles[0], (std::array<std::uint32_t, 3>{0, 2, 1}));
    EXPECT_EQ(tin.triangles[1], (std::array<std::uint32_t, 3>{1, 2, 3}));
    EXPECT_EQ(tin.colours, (std::vector<std::string>{"-1", "vis grass"}));
    EXPECT_TRUE(tin.isSurfaceTriangle(0)) << "a tin's first four points are data, not construction";
}

TEST(ReaderTin, AFullTinSaysWhichTrianglesAreVisibleAndWhichAreConstruction)
{
    // Manual 1.4.7.1: the first four points are construction points; nulling
    // is 1 for null and 2 for visible; a neighbour of 0 is "none".
    const auto archive = read(R"(full_tin { name "F"
  points { -100 -100 0  -100 100 0  100 100 0  100 -100 0   0 0 5  10 0 6  0 10 7  10 10 8 }
  triangles { 5 7 6   6 7 8   1 5 6 }
  neighbours { 0 2 3   1 0 0   0 1 0 }
  nulling { 2 1 2 }
  input { preserve_strings true weed_tin false models { "SURVEY" "DESIGN" } }
})");
    const auto& tin = only<a12::Tin>(archive);
    EXPECT_TRUE(tin.full);
    ASSERT_EQ(tin.triangles.size(), 3u);
    EXPECT_EQ(tin.visible, (std::vector<bool>{true, false, true}));
    EXPECT_TRUE(tin.isSurfaceTriangle(0));
    EXPECT_FALSE(tin.isSurfaceTriangle(1)) << "nulled";
    EXPECT_FALSE(tin.isSurfaceTriangle(2)) << "visible, but it touches construction point 1";
    ASSERT_EQ(tin.neighbours.size(), 3u);
    EXPECT_EQ(tin.neighbours[0], (std::array<std::uint32_t, 3>{a12::Tin::kNoNeighbour, 1, 2}));
    EXPECT_EQ(tin.input.boolean("preserve_strings"), true);
    EXPECT_EQ(tin.inputModels, (std::vector<std::string>{"SURVEY", "DESIGN"}));
}

TEST(ReaderTin, TrianglesMayBeWrappedInAVerticesBlockAsSomeWritersDo)
{
    const auto archive = read("full_tin { name \"F\" points { 0 0 0 0 1 0 1 1 0 1 0 0  2 2 1 3 2 1 2 3 1 }\n"
                              " triangles { vertices { 5 7 6 } } nulling { 2 } }");
    EXPECT_EQ(only<a12::Tin>(archive).triangles.at(0), (std::array<std::uint32_t, 3>{4, 6, 5}));
}

TEST(ReaderTin, ATriangleNamingAPointThatDoesNotExistIsAnError)
{
    const auto archive = a12::readArchive("tin { name \"T\" points { 0 0 0  1 0 0  0 1 0 } triangles { 1 2 4 } }");
    ASSERT_FALSE(archive.ok());
    EXPECT_NE(archive.error().message.find("point 4"), std::string::npos) << archive.error().describe();
}

TEST(ReaderTin, PointNumbersThatDoNotMakeWholeTrianglesAreAnError)
{
    EXPECT_FALSE(a12::readArchive("tin { name \"T\" points { 0 0 0 1 0 0 0 1 0 } triangles { 1 2 3 1 } }").ok());
}

TEST(ReaderTin, AFileThatCountsFromZeroIsReadAndSaidToBeNonConforming)
{
    const auto archive = read("tin { name \"T\" points { 0 0 0  1 0 0  0 1 0 } triangles { 0 2 1 } }");
    EXPECT_EQ(only<a12::Tin>(archive).triangles.at(0), (std::array<std::uint32_t, 3>{0, 2, 1}));
    ASSERT_FALSE(archive.warnings.empty());
    EXPECT_NE(archive.warnings[0].find("from 0"), std::string::npos);
}

TEST(ReaderTin, ASuperTinIsAnOrderedListOfTinNames)
{
    const auto archive = read("super_tin { name \"COMBINED\" colour green exact true"
                              " attributes { text \"Style\" \"1\" } tins { \"NATURAL\" \"PAD A\" PadB } }");
    const auto& superTin = only<a12::SuperTin>(archive);
    EXPECT_EQ(superTin.name, "COMBINED");
    EXPECT_EQ(superTin.tins, (std::vector<std::string>{"NATURAL", "PAD A", "PadB"}));
    EXPECT_EQ(superTin.extras.boolean("exact"), true);
    EXPECT_EQ(superTin.attributes.size(), 1u);
}

// ---- trimeshes (manual 1.4.9) --------------------------------------------------------------------------

TEST(ReaderTrimesh, VerticesFacesEdgesAndTheirPresentationAreAllRead)
{
    const auto archive = read(R"(model "Site Mesh"
primitive_3d { attributes { text "Material" "Rock" } name Stockpile colour brown
  trimesh_3d {
    info { flag 0 key 1 colour brown name Stockpile }
    vertices { 0 0 0  20 0 0  20 20 0  10 10 3.5 }
    faces { 1 2 4   2 3 4 }
    edges { 1 2  2 3 }
    blend 0.5
    face_infos { 0 0 green "no name"   0 1 blue "side" }
    face_flags { 2 0 }
  }
})");
    const auto& mesh = only<a12::Trimesh>(archive);
    EXPECT_EQ(mesh.name, "Stockpile");
    EXPECT_EQ(mesh.model, "Site Mesh");
    EXPECT_EQ(mesh.colour, "brown");
    EXPECT_EQ(mesh.vertices.size(), 4u);
    ASSERT_EQ(mesh.faces.size(), 2u);
    EXPECT_EQ(mesh.faces[1], (std::array<std::uint32_t, 3>{1, 2, 3}));
    EXPECT_EQ(mesh.edges.at(1), (std::array<std::uint32_t, 2>{1, 2}));
    EXPECT_EQ(mesh.info.integer("key"), 1);
    EXPECT_EQ(mesh.blend, 0.5);
    ASSERT_EQ(mesh.faceInfos.size(), 2u);
    EXPECT_EQ(mesh.faceInfos[1].colour, "blue");
    EXPECT_EQ(mesh.faceInfos[1].name, "side");
    EXPECT_EQ(mesh.faceFlags, (std::vector<std::uint32_t>{2, 0})) << "1-based; 0 is none";
    EXPECT_EQ(mesh.attributes.size(), 1u);
}

TEST(ReaderTrimesh, AFaceNamingAVertexThatDoesNotExistIsAnError)
{
    EXPECT_FALSE(a12::readArchive("primitive_3d { name M trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 }"
                                  " faces { 1 2 9 } } }")
                     .ok());
}

// ---- the project --------------------------------------------------------------------------------------------

TEST(ReaderProject, ProjectAttributesAreRead)
{
    const auto archive = read("project_attributes { group { name \"Meta\" attributes { text \"Zone\" \"MGA 56\" } } }");
    ASSERT_EQ(archive.projectAttributes.size(), 1u);
    const auto& group = std::get<a12::AttributeList>(archive.projectAttributes[0].value);
    EXPECT_EQ(std::get<std::string>(group.at(0).value), "MGA 56");
}

// ---- whole files ------------------------------------------------------------------------------------------------

TEST(ReaderFiles, TheAllGeometriesFixtureHoldsOneOfEverythingAndNothingIsLeftUnread)
{
    const auto archive = a12::readArchiveBytes(fixture("all_geometries.12da"));
    ASSERT_TRUE(archive.ok()) << archive.error().describe();

    // Counted by hand from the file: 8 super strings, 3 one-line texts, and
    // one each of the other sixteen kinds it was written to exercise.
    std::map<std::string, int> counts;
    for (const a12::Element& element : archive->elements) {
        ++counts[a12::elementKeyword(element)];
    }
    EXPECT_EQ(counts["string super"], 8);
    EXPECT_EQ(counts["string text"], 3);
    for (const char* keyword :
         {"string super_alignment", "full_tin", "super_tin", "primitive_3d", "string arc",
          "string circle", "string feature", "string interface", "string polyline", "string 4d",
          "string alignment", "string pipeline", "string drainage", "string plot_frame",
          "string las_cloud_data"}) {
        EXPECT_EQ(counts[keyword], 1) << keyword;
    }
    EXPECT_EQ(archive->elements.size(), 26u);
    EXPECT_EQ(archive->modelNames.size(), 15u);
    EXPECT_EQ(archive->modelNames.front(), "Survey Control");
    EXPECT_TRUE(archive->unrecognised.empty());
    // The fixture's full_tin was written by hand without the neighbours block
    // that manual 1.4.7.1 makes mandatory - and that is the one thing the
    // reader has to say about the file.
    ASSERT_EQ(archive->warnings.size(), 1u) << archive->warnings.front();
    EXPECT_NE(archive->warnings[0].find("no neighbours block"), std::string::npos)
        << archive->warnings[0];
    ASSERT_FALSE(archive->headerSettings.empty());
    EXPECT_EQ(archive->headerSettings[0].value, "15.01.08.60");
}

TEST(ReaderFiles, TheOtherFixturesReadWithoutComplaint)
{
    for (const char* name : {"drainage_strings.12da", "super_tin.12da", "multiple_tins.12da",
                             "las_point_cloud.12da"}) {
        const auto archive = a12::readArchiveBytes(fixture(name));
        ASSERT_TRUE(archive.ok()) << name << ": " << archive.error().describe();
        EXPECT_FALSE(archive->elements.empty()) << name;
        EXPECT_TRUE(archive->unrecognised.empty()) << name;
        EXPECT_TRUE(archive->warnings.empty()) << name << ": " << archive->warnings.front();
    }
}

TEST(ReaderFiles, TheSuperTinFixtureRanksItsThreeTins)
{
    const auto archive = a12::readArchiveBytes(fixture("super_tin.12da"));
    ASSERT_TRUE(archive.ok());
    // The file's own header: one natural surface of 25 points and 32
    // triangles (a 5 x 5 grid is 4 x 4 cells of 2), two 3 x 3 pads of 8.
    ASSERT_EQ(archive->elements.size(), 4u);
    const auto& natural = std::get<a12::Tin>(archive->elements[0]);
    EXPECT_EQ(natural.points.size(), 25u);
    EXPECT_EQ(natural.triangles.size(), 32u);
    EXPECT_EQ(std::get<a12::Tin>(archive->elements[1]).triangles.size(), 8u);
    const auto& superTin = std::get<a12::SuperTin>(archive->elements[3]);
    EXPECT_EQ(superTin.tins,
              (std::vector<std::string>{"NATURAL SURFACE", "PAD A", "PAD B"}));
}
