#include <gtest/gtest.h>

#include <algorithm>
#include <fstream>
#include <sstream>
#include <string>

#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/writer.hpp"

namespace a12 = katana::archive12d;

namespace {

a12::Archive reread(const a12::Archive& archive, const a12::WriteOptions& options = {})
{
    const std::string text = a12::writeArchive(archive, options);
    auto back = a12::readArchive(text);
    EXPECT_TRUE(back.ok()) << (back.ok() ? "" : back.error().describe()) << "\n" << text;
    return back.ok() ? std::move(*back) : a12::Archive{};
}

a12::StringHeader header(const std::string& name, const std::string& model)
{
    a12::StringHeader head;
    head.name = name;
    head.model = model;
    head.colour = "dark green";
    head.style = "Kerb & Channel";
    head.chainage = 12.5;
    head.breakline = a12::Breakline::Line;
    return head;
}

a12::Attribute text(const std::string& name, const std::string& value)
{
    return a12::Attribute{name, value, {}};
}

std::string fixture(const std::string& name)
{
    std::ifstream file(std::string(KATANA_ARCHIVE12D_TEST_DATA) + "/" + name, std::ios::binary);
    std::ostringstream bytes;
    bytes << file.rdbuf();
    return bytes.str();
}

// What the writer is documented to change: the superseded kinds are written
// as super strings, and a full_tin without neighbours as a tin.
void expectSameElements(const a12::Archive& written, const a12::Archive& back)
{
    ASSERT_EQ(written.elements.size(), back.elements.size());
    for (std::size_t i = 0; i < written.elements.size(); ++i) {
        EXPECT_TRUE(written.elements[i] == back.elements[i])
            << "element " << i << " (" << a12::elementKeyword(written.elements[i])
            << ") did not survive being written and read";
    }
}

} // namespace

TEST(Quoting, ADoubleQuoteAndABackslashAreEscapedAndNothingElse)
{
    // Manual 1.1.
    EXPECT_EQ(a12::quoted("plain"), "\"plain\"");
    EXPECT_EQ(a12::quoted(""), "\"\"");
    EXPECT_EQ(a12::quoted("say \"hi\""), R"("say \"hi\"")");
    EXPECT_EQ(a12::quoted(R"($LIB\title.tbf)"), R"("$LIB\\title.tbf")");
    EXPECT_EQ(a12::quoted("a // b"), "\"a // b\"");
}

TEST(Writer, EveryDocumentedElementReadsBackAsItWasWritten)
{
    a12::Archive archive;
    archive.projectAttributes.push_back(text("Zone", "MGA 56"));
    archive.models.push_back(a12::ModelRecord{"Survey/Control", {text("Owner", "QLD")}, {}});

    // A super string using every optional block.
    a12::VertexString super;
    super.header = header("Kerb \"north\"", "Survey/Control");
    super.header.attributes.push_back(a12::Attribute{"Height", 5.25, {}});
    super.header.attributes.push_back(a12::Attribute{"Wires", std::int64_t{3}, {}});
    super.header.attributes.push_back(a12::Attribute{"this id", std::int64_t{5292}, "uid"});
    super.header.attributes.push_back(
        a12::Attribute{"Asset", a12::AttributeList{text("Type", "Kerb"), a12::Attribute{"Size", 0.125, {}}}, {}});
    super.header.extras.add("weight", "2");
    super.header.extras.add("time_created", "07-Mar-2009 03:53:17", true);
    super.closed = true;
    super.vertices = {{0.0, 0.0, 1.5}, {10.0, 0.0, std::nullopt}, {10.0, 10.0, 2.25}};
    a12::Segment arc;
    arc.kind = a12::SegmentKind::Arc;
    arc.radius = -7.5;
    arc.major = true;
    super.segments = {a12::Segment{}, arc, a12::Segment{}};
    super.pointIds = {"101", "A 7", "103"};
    super.vertexText = {"a", "", "c"};
    super.segmentColours = {"red", "dark blue", "-1"};
    super.diameters = {0.25, 0.5, 0.75};
    super.justify = "invert";
    super.vertexTinable = {true, false, true};
    super.segmentVisibleValue = false;
    super.vertexAnnotation.emplace();
    super.vertexAnnotation->add("worldsize", "2.5");
    super.vertexAnnotation->add("justify", "top-left", true);
    super.symbols.resize(3);
    super.symbols[0].add("style", "Tree", true);
    super.vertexAttributes = {{text("n", "1")}, {}, {text("n", "3")}};
    super.interval.emplace();
    super.interval->add("chord_arc", "0.01");
    archive.elements.emplace_back(super);

    // Transitions need geometry_data.
    a12::VertexString transition;
    transition.header = header("Transition", "Design");
    transition.constantZ = 31.25;
    transition.vertices = {{0.0, 0.0, std::nullopt}, {80.0, 4.0, std::nullopt}};
    a12::Segment spiral;
    spiral.kind = a12::SegmentKind::Spiral;
    spiral.parameters.add("type", "cubic parabola", true);
    spiral.parameters.add("leading", "1");
    spiral.parameters.add("l2", "80");
    spiral.parameters.add("r2", "-210.5");
    transition.segments = {spiral};
    archive.elements.emplace_back(transition);

    a12::VertexString interface12d;
    interface12d.kind = a12::StringKind::Interface;
    interface12d.header = header("Daylight", "Design");
    interface12d.vertices = {{0.0, 0.0, 1.0}, {5.0, 0.0, 2.0}};
    interface12d.interfaceModes = {-1, 1};
    archive.elements.emplace_back(interface12d);

    a12::VertexString face;
    face.kind = a12::StringKind::Face;
    face.header = header("Hatch", "Design");
    face.header.extras.add("hatch_angle", "45");
    face.vertices = {{0.0, 0.0, 1.0}, {5.0, 0.0, 1.0}, {5.0, 5.0, 1.0}};
    archive.elements.emplace_back(face);

    a12::ArcString arcString;
    arcString.header = header("Arc", "Design");
    arcString.radius = -15.0;
    arcString.centre = {600.0, 100.0, 31.0};
    arcString.start = {615.0, 100.0, 31.0};
    arcString.end = {600.0, 115.0, std::nullopt};
    archive.elements.emplace_back(arcString);

    a12::CircleString circle;
    circle.header = header("Tank", "Design");
    circle.radius = 8.0;
    circle.centre = {650.0, 200.0, 32.0};
    archive.elements.emplace_back(circle);
    circle.feature = true;
    circle.header.name = "Tree";
    archive.elements.emplace_back(circle);

    a12::TextString label;
    label.header = header("", "Labels");
    label.text = "RL \"32.451\"";
    label.position = {1.0, 2.0, std::nullopt};
    label.annotation.add("worldsize", "2.5");
    label.annotation.add("textstyle", "Arial Narrow", true);
    archive.elements.emplace_back(label);

    a12::PlotFrame frame;
    frame.header = header("Sheet 01", "Plot Frames");
    frame.fields.add("width", "420");
    frame.fields.add("title_file", R"($LIB\title.tbf)", true);
    archive.elements.emplace_back(frame);

    a12::DrainageString drainage;
    drainage.header = header("SW Line A", "Drainage");
    drainage.outfall = 28.5;
    drainage.flowDirection = 1;
    drainage.vertices = {{0.0, 0.0, 29.0}, {50.0, 20.0, 29.5}};
    a12::DrainageRecord pit;
    pit.fields.add("name", "A1", true);
    pit.fields.add("diameter", "1.05");
    pit.attributes.push_back(text("Lid", "Class D"));
    drainage.pits = {pit, pit};
    drainage.pitsAreVersion2 = true;
    a12::DrainageRecord pipe;
    pipe.fields.add("us_level", "29.5");
    drainage.pipes = {pipe};
    a12::DrainageRecord control;
    control.fields.add("name", "Lot 4", true);
    control.vertices = {{25.0, 10.0, 29.25}, {30.0, 40.0, 30.0}};
    drainage.propertyControls = {control};
    drainage.houseConnections = {pipe};
    archive.elements.emplace_back(drainage);

    a12::SuperAlignment alignment;
    alignment.header = header("MC01", "Alignments");
    alignment.spiralType = "clothoid";
    alignment.validHorizontal = true;
    alignment.validVertical = false;
    a12::AlignmentPart ip;
    ip.kind = "spiral";
    ip.fields.add("id", "300");
    ip.fields.add("r", "50");
    ip.attributes.push_back(text("note", "x"));
    alignment.horizontalParts = {ip};
    alignment.horizontalData.emplace();
    alignment.horizontalData->header.add("name", "MC01", true);
    alignment.horizontalData->vertices = {{0.0, 0.0, std::nullopt}, {100.0, 0.0, std::nullopt},
                                          {150.0, 50.0, std::nullopt}};
    alignment.horizontalData->segments = {a12::Segment{}, arc};
    alignment.verticalData.emplace();
    alignment.verticalData->vertices = {{0.0, 30.0, std::nullopt}, {100.0, 33.0, std::nullopt}};
    a12::Segment parabola;
    parabola.kind = a12::SegmentKind::Parabola;
    parabola.parameters.add("chainage", "50");
    parabola.parameters.add("height", "35");
    alignment.verticalData->segments = {parabola};
    alignment.diameter = 0.375;
    alignment.pipeLength = 6.0;
    archive.elements.emplace_back(alignment);

    a12::LasCloud cloud;
    cloud.header = header("Scan", "Clouds");
    cloud.format = "v14_p8";
    cloud.pointFormat = 8;
    cloud.categories = {true, false};
    cloud.range.add("xmin", "1");
    a12::LasPoint point;
    point.x = 10.5;
    point.y = 20.5;
    point.z = 3.25;
    point.intensity = 1200;
    point.returnNumber = 5;
    point.returnCount = 6;
    point.classificationFlags = 7;
    point.scannerChannel = 2;
    point.scanDirection = 1;
    point.classification = 4;
    point.userData = 200;
    point.scanAngle = -30;
    point.pointSourceId = 9;
    point.gpsTime = 55.5;
    point.colour = 281474976710655ull;
    point.nearInfrared = 17;
    cloud.points = {point, point};
    archive.elements.emplace_back(cloud);
    a12::LasCloud reference;
    reference.header = header("Ref", "Clouds");
    reference.referenceFile = "scans/site.las";
    archive.elements.emplace_back(reference);

    a12::Tin tin;
    tin.full = true;
    tin.name = "NATURAL SURFACE";
    tin.colour = "green";
    tin.attributes.push_back(text("Style", "1"));
    tin.points = {{-100.0, -100.0, 0.0}, {-100.0, 100.0, 0.0}, {100.0, 100.0, 0.0},
                  {100.0, -100.0, 0.0}, {0.0, 0.0, 5.0},       {10.0, 0.0, std::nullopt},
                  {0.0, 10.0, 7.5}};
    tin.triangles = {{4, 6, 5}, {0, 4, 5}};
    tin.neighbours = {{a12::Tin::kNoNeighbour, a12::Tin::kNoNeighbour, 1},
                      {a12::Tin::kNoNeighbour, 0, a12::Tin::kNoNeighbour}};
    tin.visible = {true, false};
    tin.colours = {"-1", "vis grass"};
    tin.input.add("weed_tin", "false");
    tin.inputModels = {"SURVEY DETAILS"};
    archive.elements.emplace_back(tin);

    a12::SuperTin superTin;
    superTin.name = "COMBINED";
    superTin.colour = "green";
    superTin.tins = {"NATURAL SURFACE", "PAD A"};
    archive.elements.emplace_back(superTin);

    a12::Trimesh mesh;
    mesh.name = "Stockpile SP-02";
    mesh.model = "Meshes";
    mesh.colour = "brown";
    mesh.info.add("key", "255");
    mesh.vertices = {{0.0, 0.0, 0.0}, {20.0, 0.0, 0.0}, {20.0, 20.0, 0.0}, {10.0, 10.0, 3.5}};
    mesh.faces = {{0, 1, 3}, {1, 2, 3}};
    mesh.edges = {{0, 1}};
    mesh.blend = 0.5;
    mesh.faceInfos = {{0, 1, "blue", "side face"}};
    mesh.faceFlags = {0, 0};
    archive.elements.emplace_back(mesh);

    const a12::Archive back = reread(archive);
    expectSameElements(archive, back);
    EXPECT_EQ(back.projectAttributes, archive.projectAttributes);
    EXPECT_EQ(back.models, archive.models);
    EXPECT_TRUE(back.unrecognised.empty());
    EXPECT_TRUE(back.warnings.empty()) << back.warnings.front();
}

TEST(Writer, SupersededStringsAreWrittenAsTheSuperStringsThatReplacedThem)
{
    auto read = a12::readArchive(R"(model "Legacy"
string 2d { name "two" z 12.5 data { 0 0  5 5 } }
string 4d { name "four" angle 15 worldsize 2 data { 0 0 1 "T1"  5 5 2 "T2" } }
string pipe { name "pipe" diameter 0.5 data { 0 0 1  5 5 2 } }
string polyline { name "poly" closed 1 data { 0 0 1 12 1   20 0 1 0 0   20 20 1 0 0 } })");
    ASSERT_TRUE(read.ok());
    const std::string text = a12::writeArchive(*read);
    for (const char* old : {"string 2d", "string 4d", "string pipe", "string polyline"}) {
        EXPECT_EQ(text.find(old), std::string::npos) << old << " is superseded and is not written";
    }
    const auto back = a12::readArchive(text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    ASSERT_EQ(back->elements.size(), 4u);
    for (std::size_t i = 0; i < 4; ++i) {
        a12::VertexString expected = std::get<a12::VertexString>(read->elements[i]);
        expected.kind = a12::StringKind::Super; // the one documented difference
        EXPECT_TRUE(expected == std::get<a12::VertexString>(back->elements[i])) << "string " << i;
    }
}

TEST(Writer, AnOldAlignmentIsWrittenAsASuperAlignmentWithTheSameIps)
{
    auto read = a12::readArchive("string alignment { name \"AL\" hipdata { 0 0 0  100 0 40  150 100 0 }\n"
                                 " vipdata { 0 30 0  100 33 40 parabola  220 31 0 } }");
    ASSERT_TRUE(read.ok());
    const auto back = a12::readArchive(a12::writeArchive(*read));
    ASSERT_TRUE(back.ok()) << back.error().describe();
    a12::SuperAlignment expected = std::get<a12::SuperAlignment>(read->elements[0]);
    expected.source = a12::AlignmentSource::SuperAlignment;
    EXPECT_TRUE(expected == std::get<a12::SuperAlignment>(back->elements[0]));
}

TEST(Writer, ATinWithoutNeighboursIsWrittenInTheFormThatDoesNotNeedThem)
{
    // Manual 1.4.7.1 makes neighbours MANDATORY in a full_tin; 1.4.7.2's tin
    // lists visible triangles only.
    a12::Archive archive;
    a12::Tin tin;
    tin.full = true;
    tin.name = "T";
    tin.points = {{0.0, 0.0, 0.0}, {0.0, 9.0, 0.0}, {9.0, 9.0, 0.0}, {9.0, 0.0, 0.0},
                  {1.0, 1.0, 5.0}, {2.0, 1.0, 5.0}, {1.0, 2.0, 5.0}, {2.0, 2.0, 5.0}};
    tin.triangles = {{4, 6, 5}, {5, 6, 7}, {0, 4, 5}};
    tin.visible = {true, false, true};
    archive.elements.emplace_back(tin);

    const std::string text = a12::writeArchive(archive);
    EXPECT_EQ(text.find("full_tin"), std::string::npos);
    EXPECT_EQ(text.find("neighbours"), std::string::npos);
    const auto back = a12::readArchive(text);
    ASSERT_TRUE(back.ok());
    const auto& written = std::get<a12::Tin>(back->elements.at(0));
    EXPECT_FALSE(written.full);
    // Of the three, one is nulled and one touches a construction point.
    ASSERT_EQ(written.triangles.size(), 1u);
    EXPECT_EQ(written.triangles[0], (std::array<std::uint32_t, 3>{4, 6, 5}));
}

TEST(Writer, ANullHeightIsWrittenAsTheArchivesNullValue)
{
    a12::Archive archive;
    archive.nullValue = -1.0;
    a12::VertexString string;
    string.header = header("S", "M");
    string.vertices = {{0.0, 0.0, 5.0}, {1.0, 1.0, std::nullopt}};
    archive.elements.emplace_back(string);
    const std::string text = a12::writeArchive(archive);
    EXPECT_NE(text.find("null -1\n"), std::string::npos) << text;
    EXPECT_NE(text.find("1 1 -1\n"), std::string::npos) << text;
    const auto back = a12::readArchive(text);
    ASSERT_TRUE(back.ok());
    EXPECT_FALSE(std::get<a12::VertexString>(back->elements[0]).vertices[1].z.has_value());
}

TEST(Writer, HexadecimalTinPointsReadBackBitForBit)
{
    a12::Archive archive;
    a12::Tin tin;
    tin.name = "T";
    // Thirds and sevenths: no finite decimal, so eight places cannot hold them.
    tin.points = {{187008.0 + 1.0 / 3.0, 6184863.0 + 1.0 / 7.0, 8.0 + 2.0 / 3.0},
                  {187009.5, 6184864.25, -0.1},
                  {187010.0, 6184865.0, 1e-9}};
    tin.triangles = {{0, 2, 1}};
    archive.elements.emplace_back(tin);

    a12::WriteOptions options;
    options.hexFloatTins = true;
    const a12::Archive back = reread(archive, options);
    EXPECT_EQ(std::get<a12::Tin>(back.elements.at(0)).points, tin.points);

    // ... which decimal at the default eight places does not quite manage.
    const a12::Archive decimal = reread(archive);
    EXPECT_NE(std::get<a12::Tin>(decimal.elements.at(0)).points, tin.points);
}

TEST(Writer, CoordinatesAreWrittenWithoutExponentsOrTrailingZeros)
{
    a12::Archive archive;
    a12::VertexString string;
    string.header = header("S", "M");
    string.vertices = {{502000.0, 6960000.5, 0.000125}, {-0.00000001, 1e7, -30.25}};
    archive.elements.emplace_back(string);
    const std::string text = a12::writeArchive(archive);
    EXPECT_NE(text.find("502000 6960000.5 0.000125\n"), std::string::npos) << text;
    // -0.00000001 is one unit in the eighth place; 1e7 must not become 1e+07.
    EXPECT_NE(text.find("-0.00000001 10000000 -30.25\n"), std::string::npos) << text;

    a12::WriteOptions coarse;
    coarse.decimalPlaces = 3;
    const std::string rounded = a12::writeArchive(archive, coarse);
    EXPECT_NE(rounded.find("502000 6960000.5 0\n"), std::string::npos) << rounded;
    EXPECT_NE(rounded.find("\n    0 10000000 -30.25\n"), std::string::npos)
        << "a value that rounds to zero is 0, never -0\n"
        << rounded;
}

TEST(Writer, AModelIsNamedOnceForARunOfItsElements)
{
    a12::Archive archive;
    for (const char* model : {"A", "A", "B", "A"}) {
        a12::VertexString string;
        string.header = header("S", model);
        string.vertices = {{0.0, 0.0, 0.0}};
        archive.elements.emplace_back(string);
    }
    const std::string text = a12::writeArchive(archive);
    std::size_t commands = 0;
    for (std::size_t at = text.find("\nmodel "); at != std::string::npos;
         at = text.find("\nmodel ", at + 1)) {
        ++commands;
    }
    EXPECT_EQ(commands, 3u) << text; // A, B, A
    const auto back = a12::readArchive(text);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(std::get<a12::VertexString>(back->elements[3]).header.model, "A");
}

TEST(Writer, TheAllGeometriesFixtureSurvivesBeingWrittenAndReadAgain)
{
    auto first = a12::readArchiveBytes(fixture("all_geometries.12da"));
    ASSERT_TRUE(first.ok());
    auto second = a12::readArchive(a12::writeArchive(*first));
    ASSERT_TRUE(second.ok()) << second.error().describe();
    // A second trip must change nothing at all: whatever the writer normalises
    // it normalises once.
    auto third = a12::readArchive(a12::writeArchive(*second));
    ASSERT_TRUE(third.ok());
    expectSameElements(*second, *third);
    EXPECT_EQ(second->elements.size(), first->elements.size());
    // Including "Ground TIN", which holds no string at all: a model that is
    // empty is still a model.
    auto before = first->modelNames;
    auto after = second->modelNames;
    std::sort(before.begin(), before.end());
    std::sort(after.begin(), after.end());
    EXPECT_EQ(after, before);
}
