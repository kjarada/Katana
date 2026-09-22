// Regression tests for the defects an adversarial review of the module found
// (recorded in docs/interop.md). Each test failed against the code as first
// written and passes with its fix; the case that exposed each one is kept as
// the review reported it.

#include <gtest/gtest.h>

#include <algorithm>
#include <sstream>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/writer.hpp"
#include "katana/entity/model.hpp"

namespace a12 = katana::archive12d;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

a12::Archive read(const std::string& text)
{
    auto archive = a12::readArchive(text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    return archive.ok() ? std::move(*archive) : a12::Archive{};
}

// These fixtures are hand-written, and a hand-written 12da inherits the
// format's CURRENT BREAKLINE TYPE, whose default is `point` (commands,
// 1.4.4). A fixture that means a line has to say so, exactly as every string
// in a real archive does - 12d writes the flag on all 25,659 strings of a
// production file and leaves nothing to the default. Rather than repeat it in
// every fixture, the helper states it once at file level, which is the
// format's own way of saying it; a fixture that wants POINTS says
// `breakline point` inside the string and overrides this.
constexpr const char* kBreaklineLine = "breakline line\n";

a12::DomainImport import(const std::string& text)
{
    auto domain = a12::toDomain(read(kBreaklineLine + text));
    EXPECT_TRUE(domain.ok());
    return domain.ok() ? std::move(*domain) : a12::DomainImport{};
}

bool anyContains(const std::vector<std::string>& list, const std::string& needle)
{
    return std::any_of(list.begin(), list.end(),
                       [&](const std::string& s) { return s.find(needle) != std::string::npos; });
}

std::string joined(const std::vector<std::string>& list)
{
    std::ostringstream out;
    for (const std::string& s : list) {
        out << "\n  " << s;
    }
    return out.str();
}

} // namespace

// ---- the null value ---------------------------------------------------------------

TEST(ReviewNull, ARealHeightEqualToTheFinalNullValueSurvivesBeingWritten)
{
    // Read under `null -999`, string a's -1 is a height; b's, read under
    // `null -1`, is not. Written with one null value, a real -1 must not
    // become a null on the way back.
    const auto first = read("null -999\nstring super { name a data_3d { 0 0 -1  1 1 -999 } }\n"
                            "null -1\nstring super { name b data_3d { 0 0 -1  1 1 -999 } }");
    ASSERT_EQ(first.elements.size(), 2u);
    const auto& a = std::get<a12::VertexString>(first.elements[0]);
    const auto& b = std::get<a12::VertexString>(first.elements[1]);
    EXPECT_EQ(a.vertices[0].z, -1.0);
    EXPECT_FALSE(a.vertices[1].z.has_value());
    EXPECT_FALSE(b.vertices[0].z.has_value());
    EXPECT_EQ(b.vertices[1].z, -999.0);

    const auto second = read(a12::writeArchive(first));
    ASSERT_EQ(second.elements.size(), 2u);
    EXPECT_TRUE(second.elements[0] == first.elements[0]);
    EXPECT_TRUE(second.elements[1] == first.elements[1]);
    // Neither -1 nor -999 could serve; the writer went further down.
    EXPECT_LT(second.nullValue, -999.0);
}

TEST(ReviewNull, AStringsOwnNullValueAppliesWhereverItStandsAndToEveryStringType)
{
    // Manual 1.4.5 lets a string override the null value; 1.4.6 makes string
    // commands order-free.
    const auto archive = read("null -1\n"
                              "string super { name after data_3d { 0 0 -5  1 1 -1 } null_value -5 }\n"
                              "string text { null_value -5 x 1 y 2 z -5 text \"hi\" }\n"
                              "string circle { null_value -5 radius 2 xcentre 1 ycentre 2 zcentre -5 }\n"
                              "string drainage { null_value -5 data { 0 0 -5 0 0  10 0 -1 0 0 } }\n"
                              "string super { name plain data_3d { 0 0 -5  1 1 -1 } }");
    ASSERT_EQ(archive.elements.size(), 5u);
    const auto& after = std::get<a12::VertexString>(archive.elements[0]);
    EXPECT_FALSE(after.vertices[0].z.has_value()) << "-5 is this string's null";
    EXPECT_EQ(after.vertices[1].z, -1.0) << "and -1 is a height here, whatever the file says";
    EXPECT_FALSE(std::get<a12::TextString>(archive.elements[1]).position.z.has_value());
    EXPECT_FALSE(std::get<a12::CircleString>(archive.elements[2]).centre.z.has_value());
    const auto& drainage = std::get<a12::DrainageString>(archive.elements[3]);
    EXPECT_FALSE(drainage.vertices[0].z.has_value());
    EXPECT_EQ(drainage.vertices[1].z, -1.0);
    const auto& plain = std::get<a12::VertexString>(archive.elements[4]);
    EXPECT_EQ(plain.vertices[0].z, -5.0);
    EXPECT_FALSE(plain.vertices[1].z.has_value());

    // Written back, the per-string value is gone and every null is the file's.
    const std::string text = a12::writeArchive(archive);
    EXPECT_EQ(text.find("null_value"), std::string::npos) << text;
    const auto back = read(text);
    ASSERT_EQ(back.elements.size(), 5u);
    for (std::size_t i = 0; i < 5; ++i) {
        a12::Element expected = archive.elements[i];
        // The only intended difference: the extras no longer hold null_value.
        if (auto* s = std::get_if<a12::VertexString>(&expected)) {
            s->header.extras.take("null_value");
        } else if (auto* t = std::get_if<a12::TextString>(&expected)) {
            t->header.extras.take("null_value");
        } else if (auto* c = std::get_if<a12::CircleString>(&expected)) {
            c->header.extras.take("null_value");
        } else if (auto* d = std::get_if<a12::DrainageString>(&expected)) {
            d->header.extras.take("null_value");
        }
        EXPECT_TRUE(expected == back.elements[i]) << "element " << i;
    }
}

// ---- documented keys always take a value ---------------------------------------------

TEST(ReviewKeywords, ABareWordThatIsAlsoAKeywordIsStillTheValueOfADocumentedKey)
{
    // Manual 1.1 needs quotes only around text that is not alphanumeric, so
    // `text PIT` and `title_1 Scale` are legal - and `PIT`, `Scale`, `model`
    // are all words the reader knows as keys.
    const auto archive = read("string text { name lbl text PIT x 1 y 2 z 3 textstyle 1 }\n"
                              "string text { x 100 y 200 z 3 name t2 text X angle 45 }\n"
                              "string plot_frame { name pf title_1 Scale title_2 Drainage width 10 height 20"
                              " plotter model colour dark grey }");
    ASSERT_EQ(archive.elements.size(), 3u);
    const auto& first = std::get<a12::TextString>(archive.elements[0]);
    EXPECT_EQ(first.text, "PIT");
    EXPECT_EQ(first.position, (a12::Vertex{1.0, 2.0, 3.0}));
    EXPECT_EQ(first.annotation.text("textstyle"), "1");
    const auto& second = std::get<a12::TextString>(archive.elements[1]);
    EXPECT_EQ(second.text, "X");
    EXPECT_EQ(second.position.x, 100.0);
    EXPECT_EQ(second.annotation.real("angle"), 45.0);
    const auto& frame = std::get<a12::PlotFrame>(archive.elements[2]);
    EXPECT_EQ(frame.fields.text("title_1"), "Scale");
    EXPECT_EQ(frame.fields.text("title_2"), "Drainage");
    EXPECT_EQ(frame.fields.real("width"), 10.0);
    EXPECT_EQ(frame.fields.text("plotter"), "model");
    EXPECT_EQ(frame.header.colour, "dark grey");
    EXPECT_FALSE(frame.fields.contains("scale")) << "no scale was written";
    EXPECT_TRUE(archive.warnings.empty()) << joined(archive.warnings);
}

TEST(ReviewKeywords, AColourCalledDarkAtTheEndOfAHeaderDoesNotEatTheNextElement)
{
    const auto archive = read("string super { colour dark data_2d { 0 0 } }\n"
                              "string super { colour dark null_value -5 data_3d { 0 0 -5 } }\n"
                              "colour dark\nstring super { data_2d { 1 1 } }\ntin { name T points { } triangles { } }");
    ASSERT_EQ(archive.elements.size(), 4u);
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[0]).header.colour, "dark");
    EXPECT_FALSE(std::get<a12::VertexString>(archive.elements[1]).vertices[0].z.has_value());
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[2]).header.colour, "dark");
    EXPECT_TRUE(std::holds_alternative<a12::Tin>(archive.elements[3]));
}

TEST(ReviewKeywords, TheDefaultLinestyleIsTheManualsAndAStringMayOverrideIt)
{
    // Manual 1.4.3: the current linestyle defaults to "1".
    const auto archive = read("string super { data_2d { 0 0 } }\nstyle Kerb\n"
                              "string super { data_2d { 0 0 } }\nstring super { style \"Own\" data_2d { 0 0 } }\n"
                              "string super { data_2d { 0 0 } }");
    ASSERT_EQ(archive.elements.size(), 4u);
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[0]).header.style, "1");
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[1]).header.style, "Kerb");
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[2]).header.style, "Own");
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[3]).header.style, "Kerb");
}

// ---- strings that lack what defines them ---------------------------------------------

TEST(ReviewIncomplete, AnArcCircleOrTextWithoutItsPositionIsIgnoredAndSaidToBe)
{
    // Manual 1.4.6: "if there is not enough recognised information to define
    // the string, the string is ignored". Not placed at the origin.
    const auto archive = read("string arc { name a1 radius 5 }\n"
                              "string circle { name c1 radius 2 }\n"
                              "string circle { name c2 xcentre 1 ycentre 2 }\n"
                              "string text { name t1 text \"lost\" }\n"
                              "string arc { name a2 radius 5 xcentre 500000 ycentre 6000000 xstart \"500005\""
                              " ystart 6000000 xend 500000 yend 6000005 }");
    ASSERT_EQ(archive.elements.size(), 1u) << "only the one with every coordinate";
    const auto& kept = std::get<a12::ArcString>(archive.elements[0]);
    EXPECT_EQ(kept.start.x, 500005.0) << "a quoted number is a number";
    ASSERT_EQ(archive.warnings.size(), 4u) << joined(archive.warnings);
    EXPECT_TRUE(anyContains(archive.warnings, "\"a1\"")) << joined(archive.warnings);
    EXPECT_TRUE(anyContains(archive.warnings, "\"c1\"")) << joined(archive.warnings);
    EXPECT_TRUE(anyContains(archive.warnings, "\"c2\" in model \"data\" ignored: its radius"));
    EXPECT_TRUE(anyContains(archive.warnings, "\"t1\" in model \"data\" ignored: its position"));
}

TEST(ReviewIncomplete, MandatoryBlocksThatAreAbsentAreReported)
{
    const auto archive = read("full_tin { name NoNb points { 0 0 0 0 9 0 9 9 0 9 0 0 1 1 1 2 1 1 1 2 1 }"
                              " triangles { 5 7 6 } }\n"
                              "tin { name Empty }\nsuper_tin { name ST }\nsuper_tin { tins { A } }\n"
                              "primitive_3d { name P trimesh_3d { } }\n"
                              "primitive_3d { name F trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 } faces { 1 2 3 }"
                              " face_infos { 0 0 red x } face_flags { 1 0 2 } } }");
    for (const char* expected :
         {"full_tin \"NoNb\" has no neighbours block", "full_tin \"NoNb\" has no nulling block",
          "tin \"Empty\" has no points block", "tin \"Empty\" has no triangles block",
          "super_tin \"ST\" has no tins block", "super_tin has no name",
          "trimesh \"P\" has no vertices block", "trimesh \"P\" has no faces block",
          "trimesh \"F\": face_flags names an info that does not exist"}) {
        EXPECT_TRUE(anyContains(archive.warnings, expected)) << expected << joined(archive.warnings);
    }
}

TEST(ReviewIncomplete, ModesAndDirectionsThatAreNotWhatTheManualAllowsAreNotInvented)
{
    const auto archive = read("string interface { name i1 data { 0 0 1 1e300  5 5 1 0.5  9 9 1 -1 } }\n"
                              "string drainage { name d1 flow_direction 1e300 data { 0 0 1 0 0 } }");
    ASSERT_EQ(archive.elements.size(), 2u);
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[0]).interfaceModes,
              (std::vector<int>{0, 0, -1}));
    EXPECT_FALSE(std::get<a12::DrainageString>(archive.elements[1]).flowDirection.has_value());
    EXPECT_TRUE(anyContains(archive.warnings, "2 interface modes are not -1, 0 or 1"))
        << joined(archive.warnings);
    EXPECT_TRUE(anyContains(archive.warnings, "flow_direction must be 0 or 1"))
        << joined(archive.warnings);
}

// ---- models ---------------------------------------------------------------------------

TEST(ReviewModels, OneModelHasOneSpellingAndIsRegisteredOnlyWhenSomethingIsInIt)
{
    // Manual 1.1: case is ignored and leading/trailing spaces are dropped.
    const auto archive = read("model \" Fred \" string super { data_2d { 0 0 } }\n"
                              "model \"FRED\" string super { data_2d { 0 0 } }\n"
                              "string super { model Fred data_2d { 0 0 } }\n"
                              "string super { model \"Other\" data_2d { 0 0 } }");
    EXPECT_EQ(archive.modelNames, (std::vector<std::string>{"Fred", "Other"}))
        << "no phantom \"data\": every string named its model";
    for (std::size_t i = 0; i < 3; ++i) {
        EXPECT_EQ(std::get<a12::VertexString>(archive.elements[i]).header.model, "Fred") << i;
    }
    const auto domain = a12::toDomain(archive);
    ASSERT_TRUE(domain.ok());
    EXPECT_EQ(domain->layersNeeded.size(), 2u);
    const std::string text = a12::writeArchive(archive);
    EXPECT_EQ(text.find("\"data\""), std::string::npos) << text;
}

// ---- tins -----------------------------------------------------------------------------

TEST(ReviewTin, PerTriangleColoursOfATinAreWrittenBackAndOfADemotedFullTinFiltered)
{
    // Manual 1.4.7.2 gives the tin form the same colours block.
    const auto tin = read("tin { name T points { 0 0 1  10 0 2  10 10 3  0 10 4  5 20 5 }"
                          " triangles { 1 4 2  2 4 3  4 5 3 } colour red colours { blue -1 \"dark green\" } }");
    const auto back = read(a12::writeArchive(tin));
    EXPECT_TRUE(back.elements.at(0) == tin.elements.at(0));

    // A full_tin without neighbours is written as a tin of its surface
    // triangles; the colours must follow the triangles that were kept.
    const auto full = read("full_tin { name F points { 0 0 0 0 9 0 9 9 0 9 0 0  1 1 1 2 1 1 1 2 1 2 2 1 }"
                           " triangles { 5 7 6  6 7 8  1 5 6 } nulling { 2 1 2 } colours { blue green red } }");
    const auto demoted = read(a12::writeArchive(full));
    const auto& written = std::get<a12::Tin>(demoted.elements.at(0));
    ASSERT_EQ(written.triangles.size(), 1u);
    EXPECT_EQ(written.colours, (std::vector<std::string>{"blue"}));
}

// ---- LAS ------------------------------------------------------------------------------

TEST(ReviewLas, AColourIsSixtyFourUnsignedBitsAndBadFieldsAreCounted)
{
    // 2^63 = 9223372036854775808 does not fit a signed 64-bit integer; a LAS
    // colour packs three 16-bit channels and uses the top bit when red is
    // bright. Both spellings of the same bits must read as those bits.
    const auto archive = read(R"(string las_cloud_data { name neg data { format v12_p2 points_v12_p2 {
 p { x 1 y 2 z 3 i 4 rn 1 rc 1 sd 0 fe 0 cl 2 sr 0 ud 0 id 1 c -2 }
 p { x 1 y 2 z 3 c 9223372036854775808 }
 p { x 1 y 2 z 3 c 18446744073709551615 }
 p { x 1 y 2 z 3 i 70000 rn 1.5 c abc } } } })");
    const auto& cloud = std::get<a12::LasCloud>(archive.elements.at(0));
    ASSERT_EQ(cloud.points.size(), 4u);
    EXPECT_EQ(cloud.points[0].colour, 18446744073709551614ull);
    EXPECT_EQ(cloud.points[1].colour, 9223372036854775808ull);
    EXPECT_EQ(cloud.points[2].colour, 18446744073709551615ull);
    EXPECT_EQ(cloud.points[3].intensity, 65535) << "clamped, and counted";
    EXPECT_TRUE(anyContains(archive.warnings, "\"neg\": 3 point fields were not numbers or were outside"))
        << joined(archive.warnings);
    // ... and they survive the writer, which is the promise writer.hpp makes.
    const auto back = read(a12::writeArchive(archive));
    EXPECT_EQ(std::get<a12::LasCloud>(back.elements.at(0)).points, cloud.points);
}

// ---- the domain mapping -----------------------------------------------------------------

TEST(ReviewImport, AHouseConnectionsHeightIsItsLevelNotItsInternalZ)
{
    // Manual 1.5.3 marks z "internal use only" for house connections and
    // property controls; 12d Model writes 0 there, 30 m below the job.
    const auto domain = import("string drainage { data { 0 0 1 0 0  5 5 1 0 0 }"
                               " pit { name P x 0 y 0 z 31 }"
                               " house_connection { name H level 28.9 adopted_level 28.8 x 1 y 1 z 0 }"
                               " house_connection { name L level 28.9 x 2 y 2 z 0 }"
                               " property_control { name C x 3 y 3 z 0 } }");
    ASSERT_EQ(domain.entities.size(), 5u);
    EXPECT_EQ(std::get<double>(domain.entities[1].properties.at("elevation")), 31.0);
    EXPECT_EQ(std::get<double>(domain.entities[2].properties.at("elevation")), 28.8);
    EXPECT_EQ(std::get<double>(domain.entities[3].properties.at("elevation")), 28.9);
    EXPECT_FALSE(domain.entities[4].properties.contains("elevation"));
}

TEST(ReviewImport, PipeAndPropertyControlAttributesArriveAndARecordWithoutAPositionIsReported)
{
    const auto domain = import("string drainage { name D data { 0 0 1 0 0  5 5 1 0 0 }"
                               " pit { name A x 0 y 0 z 2 } pit { name B x 5 y 5 z 2 }"
                               " pipe { name P diameter 0.3 attributes { text \"pipe size\" \"375\" } }"
                               " property_control { name Lot attributes { integer lot 4 }"
                               "   data { 1 1 1 0 0  2 2 1 0 0 } }"
                               " house_connection { name HC2 side right length 4 } }");
    ASSERT_EQ(domain.entities.size(), 4u);
    EXPECT_EQ(std::get<std::string>(domain.entities[0].properties.at("pipe.1.attributes/pipe size")),
              "375");
    EXPECT_EQ(std::get<std::int64_t>(domain.entities[3].properties.at("lot")), 4);
    EXPECT_TRUE(anyContains(domain.warnings, "house_connection \"HC2\" of string drainage \"D\""))
        << joined(domain.warnings);
}

TEST(ReviewImport, AFaceIsClosedAndAnInterfaceKeepsItsModes)
{
    const auto domain = import("string face { name f fill_mode 1 hatch_angle 45 data { 0 0 1  10 0 1  10 10 1 } }\n"
                               "string interface { name i data { 0 0 1 -1  5 0 2 0  9 0 3 1 } }");
    ASSERT_EQ(domain.entities.size(), 2u);
    const auto& face = std::get<Polyline2>(domain.entities[0].geometry);
    EXPECT_TRUE(face.closed);
    EXPECT_EQ(std::get<std::string>(domain.entities[0].metadata.at("12d.x.hatch_angle")), "45");
    EXPECT_EQ(std::get<std::string>(domain.entities[1].metadata.at("12d.interface_modes")), "-1 0 1");

    // And back out as the strings they were.
    katana::entity::Model model;
    for (const auto& layer : domain.layersNeeded) {
        ASSERT_TRUE(model.layers.add(layer).ok());
    }
    for (const auto& entity : domain.entities) {
        ASSERT_TRUE(model.entities.add(entity).ok());
    }
    const auto exported = a12::fromDomain(model, {});
    ASSERT_TRUE(exported.ok());
    const auto back = read(a12::writeArchive(exported->archive));
    ASSERT_EQ(back.elements.size(), 2u);
    const auto& f = std::get<a12::VertexString>(back.elements[0]);
    EXPECT_EQ(f.kind, a12::StringKind::Face);
    EXPECT_EQ(f.header.extras.real("hatch_angle"), 45.0);
    EXPECT_EQ(f.header.extras.integer("fill_mode"), 1);
    const auto& i = std::get<a12::VertexString>(back.elements[1]);
    EXPECT_EQ(i.kind, a12::StringKind::Interface);
    EXPECT_EQ(i.interfaceModes, (std::vector<int>{-1, 0, 1}));
}

TEST(ReviewImport, PerSegmentSizesAndPerVertexDataAreAllAccountedFor)
{
    const auto domain = import(R"(string super { name p data_3d { 0 0 0  10 0 0  20 0 0 }
  pipe_data { properties { diameter 0.225 } properties { diameter 0.225 } } justify bottom }
string super { name v data_3d { 0 0 0  10 0 0  20 0 0 }
  diameter_data { 0.3 0.45 } }
string super { name c data_3d { 0 0 0  10 0 0  20 0 0 }
  culvert_data { properties { width 1 height 2 } properties { width 3 height 4 } } }
string super { name full data_3d { 0 0 0  10 0 0  20 0 0 } closed 1
  segment_text_data { s1 "" "s 3" }
  segment_visible_data { 0 1 1 } vertex_tinable_value 0
  colour_data { red "dark green" -1 }
  vertex_attribute_data { attributes { integer n 1 } attributes { } attributes { text t "x y" } }
  segment_attribute_data { attributes { real grade 0.02 } attributes { } attributes { } }
  symbol_value { style Tree size 2 } }
string super { name one data_3d { 5 5 5 } vertex_attribute_data { attributes { text "QualityLevel" "B" } } })");
    // p: a uniform per-segment pipe IS one diameter; v and c: lists.
    EXPECT_EQ(std::get<double>(domain.entities.at(0).metadata.at("12d.diameter")), 0.225);
    EXPECT_EQ(std::get<std::string>(domain.entities.at(1).metadata.at("12d.diameters")), "0.3 0.45");
    EXPECT_EQ(std::get<std::string>(domain.entities.at(2).metadata.at("12d.culverts")), "1 2 3 4");

    // full: the polyline, then its two non-empty segment texts.
    const auto& full = domain.entities.at(3);
    EXPECT_EQ(std::get<std::string>(full.metadata.at("12d.segment_visible")), "0 1 1");
    EXPECT_EQ(std::get<std::string>(full.metadata.at("12d.vertex_tinable")), "0");
    EXPECT_EQ(std::get<std::string>(full.metadata.at("12d.segment_colours")), "red \"dark green\" -1");
    EXPECT_EQ(std::get<std::int64_t>(full.properties.at("vertex/1/n")), 1);
    EXPECT_EQ(std::get<std::string>(full.properties.at("vertex/3/t")), "x y");
    EXPECT_EQ(std::get<double>(full.properties.at("segment/1/grade")), 0.02);
    const auto& text3 = std::get<katana::entity::TextGeometry>(domain.entities.at(5).geometry);
    EXPECT_EQ(text3.text, "s 3");
    EXPECT_EQ(text3.position, Point2(10.0, 0.0)) << "the middle of the closing segment (20,0)-(0,0)";
    // one: a single vertex's attributes are simply the point's.
    EXPECT_EQ(std::get<std::string>(domain.entities.at(6).properties.at("QualityLevel")), "B");
    EXPECT_TRUE(anyContains(domain.warnings, "1 strings have invisible vertices or segments"))
        << joined(domain.warnings);
    // full is a LINE with one vertex symbol, and that symbol is now what it
    // is drawn with: a 12d vertex symbol goes on every vertex, so the style
    // carries it and there is nothing left over to warn about. It used to be
    // kept as metadata and drawn nowhere.
    EXPECT_FALSE(anyContains(domain.warnings, "strings carry vertex symbols"))
        << joined(domain.warnings);
    EXPECT_EQ(full.style, "Tree");
    const auto tree = std::find_if(domain.stylesNeeded.begin(), domain.stylesNeeded.end(),
                                   [](const katana::entity::Style& style) {
                                       return style.name == "Tree";
                                   });
    ASSERT_NE(tree, domain.stylesNeeded.end());
    EXPECT_EQ(tree->symbol, "Tree") << "the real 12d name, resolved when it is drawn";
    EXPECT_EQ(tree->symbolSize, 2.0);
    EXPECT_FALSE(full.metadata.contains("12d.symbol.style"))
        << "the style carries it now, so it is not also loose in the metadata";

    // The whole lot back out and in again: the same strings.
    katana::entity::Model model;
    for (const auto& layer : domain.layersNeeded) {
        ASSERT_TRUE(model.layers.add(layer).ok());
    }
    // The styles too: an import returns stylesNeeded so a caller can add
    // them, and a string is written with the symbol ITS STYLE carries - a
    // model without them is a drawing that has forgotten what it looks like.
    for (const auto& style : domain.stylesNeeded) {
        ASSERT_TRUE(model.styles.add(style).ok());
    }
    for (const auto& entity : domain.entities) {
        ASSERT_TRUE(model.entities.add(entity).ok());
    }
    const auto exported = a12::fromDomain(model, {});
    ASSERT_TRUE(exported.ok());
    const auto back = read(a12::writeArchive(exported->archive));
    const auto& p = std::get<a12::VertexString>(back.elements.at(0));
    EXPECT_EQ(p.diameter, 0.225);
    EXPECT_EQ(p.justify, "bottom");
    EXPECT_EQ(std::get<a12::VertexString>(back.elements.at(1)).diameters, (std::vector<double>{0.3, 0.45}));
    const auto& culverts = std::get<a12::VertexString>(back.elements.at(2)).culverts;
    ASSERT_EQ(culverts.size(), 2u);
    EXPECT_EQ(culverts[1], (std::array<double, 2>{3.0, 4.0}));
    const auto& f = std::get<a12::VertexString>(back.elements.at(3));
    EXPECT_EQ(f.segmentVisible, (std::vector<bool>{false, true, true}));
    EXPECT_EQ(f.vertexTinableValue, false);
    EXPECT_EQ(f.segmentColours, (std::vector<std::string>{"red", "dark green", "-1"}));
    ASSERT_EQ(f.vertexAttributes.size(), 3u);
    EXPECT_EQ(std::get<std::int64_t>(f.vertexAttributes[0].at(0).value), 1);
    EXPECT_TRUE(f.vertexAttributes[1].empty());
    EXPECT_EQ(std::get<std::string>(f.vertexAttributes[2].at(0).value), "x y");
    ASSERT_EQ(f.segmentAttributes.size(), 3u);
    EXPECT_EQ(std::get<double>(f.segmentAttributes[0].at(0).value), 0.02);
    ASSERT_TRUE(f.symbol.has_value()) << "one symbol for every vertex goes back as symbol_value";
    EXPECT_EQ(f.symbol->text("style"), "Tree");
    EXPECT_EQ(f.symbol->real("size"), 2.0);
}

TEST(ReviewImport, SplitListIsTheInverseOfWhatImportJoins)
{
    EXPECT_EQ(a12::splitList("red \"dark green\" -1"), (std::vector<std::string>{"red", "dark green", "-1"}));
    EXPECT_EQ(a12::splitList("  a   b "), (std::vector<std::string>{"a", "b"}));
    EXPECT_EQ(a12::splitList(R"("say \"hi\"" "a\\b")"), (std::vector<std::string>{"say \"hi\"", "a\\b"}));
    EXPECT_TRUE(a12::splitList("").empty());
    EXPECT_EQ(a12::splitList("\"unterminated"), (std::vector<std::string>{"unterminated"}));
}
