// The road verbs (include/katana/cad/road/road_verbs.hpp) through the
// interpreter on a Document, as every front end runs them: each edit one
// undo step, the replies key=value, the refusals specific, and the road
// model made again exactly when what it is made from changes.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/bim/bim_view.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/cad/road/road_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "road_fixtures.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::DocumentChange;
using katana::core::ErrorCode;

namespace {

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    std::size_t steps() const { return document.history().undoCount(); }

    // A 100 m straight along +x with a level design at 10.
    void road()
    {
        ok("ALIGN NEW road 0,0 100,0");
        ok("ALIGN DESIGN road 0,10 100,10");
    }
};

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// A flat surface named `name` at `level` over the test road, put in the
// drawing as SURFACE puts one.
void addGround(Session& session, const std::string& name, double level)
{
    katana::terrain::SurfaceStore store = session.document.surfaces();
    auto shared = std::make_shared<const katana::terrain::TinSurface>(
        katana::cad::road::testing::planeGround(0.0, 0.0, level));
    ASSERT_TRUE(store.add(katana::terrain::NamedSurface{name, shared, "test", {}, true}).ok());
    ASSERT_TRUE(session.document.setSurfaces(std::move(store), "TEST_SURFACE").ok());
}

} // namespace

TEST(RoadVerbs, DefaultsAddTheThreeStandardTemplatesAsOneStepAndThenNothing)
{
    Session session;
    EXPECT_TRUE(contains(session.ok("ROADTEMPLATE"), "templates=0"));
    const std::string added = session.ok("ROADTEMPLATE DEFAULTS");
    EXPECT_TRUE(contains(added, "added=3 skipped=0")) << added;
    EXPECT_EQ(session.steps(), 1u);
    EXPECT_EQ(session.document.roadTemplates().templates.size(), 3u);
    const std::string again = session.ok("ROADTEMPLATE DEFAULTS");
    EXPECT_TRUE(contains(again, "added=0 skipped=3")) << again;
    EXPECT_EQ(session.steps(), 1u) << "adding nothing is no step";
    const std::string list = session.ok("ROADTEMPLATE LIST");
    EXPECT_TRUE(contains(list, "template name=\"rural two-lane\" left=4 right=4")) << list;
}

TEST(RoadVerbs, ATemplateIsMadeAndEditedComponentByComponentEachEditOneStep)
{
    Session session;
    session.ok("ROADTEMPLATE NEW street description=\"a test street\"");
    const std::string lane =
        session.ok("ROADTEMPLATE ADD street both lane width=3.25 crossfall=-2.5");
    EXPECT_TRUE(contains(lane, "side=left name=lane kind=lane width=3.25 crossfall=-2.5")) << lane;
    EXPECT_TRUE(contains(lane, "side=right name=lane")) << lane;
    session.ok("ROADTEMPLATE ADD street both batter cut=1.5 fill=3");
    // A kerb goes before the batter, as the side's last but one.
    session.ok("ROADTEMPLATE ADD street left kerb shape=mountable height=0.1");
    // A second lane is named after its place.
    const std::string second = session.ok("ROADTEMPLATE ADD street right lane");
    EXPECT_TRUE(contains(second, "name=lane2")) << second;
    EXPECT_EQ(session.steps(), 5u);

    const auto* street = session.document.roadTemplates().find("STREET");
    ASSERT_NE(street, nullptr);
    EXPECT_EQ(street->description, "a test street");
    const auto left = street->side(katana::cad::road::Side::Left);
    ASSERT_EQ(left.size(), 3u);
    EXPECT_EQ(left[0]->name, "lane");
    EXPECT_EQ(left[1]->kind, katana::cad::road::ComponentKind::Kerb);
    EXPECT_EQ(left[1]->shape, katana::cad::road::KerbShape::Mountable);
    EXPECT_EQ(left[2]->kind, katana::cad::road::ComponentKind::Batter);
    EXPECT_EQ(left[2]->cutSlope, 1.5);

    // SET by a bare name edits both sides; side:name one.
    session.ok("ROADTEMPLATE SET street lane width=3.5");
    session.ok("ROADTEMPLATE SET street left:lane crossfall=-3");
    street = session.document.roadTemplates().find("street");
    const auto* leftLane =
        &street->components[*street->find(katana::cad::road::Side::Left, "lane")];
    const auto* rightLane =
        &street->components[*street->find(katana::cad::road::Side::Right, "lane")];
    EXPECT_EQ(leftLane->width, 3.5);
    EXPECT_EQ(rightLane->width, 3.5);
    EXPECT_EQ(leftLane->crossfall, -3.0);
    EXPECT_EQ(rightLane->crossfall, -2.5);

    // Courses, top down. A lane comes with a flexible pavement's three
    // (AC14, base, subbase); a seal goes on top of them.
    session.ok("ROADTEMPLATE COURSE street lane ADD seal thickness=0.01 at=1 ifc=protection");
    const std::string info = session.ok("ROADTEMPLATE INFO street");
    EXPECT_TRUE(contains(info, "course template=street component=left:lane name=seal "
                               "thickness=0.01 material=\"\" ifc=PROTECTION"))
        << info;
    session.ok("ROADTEMPLATE COURSE street right:lane SET base thickness=0.25");
    session.ok("ROADTEMPLATE COURSE street lane REMOVE subbase");
    street = session.document.roadTemplates().find("street");
    leftLane = &street->components[*street->find(katana::cad::road::Side::Left, "lane")];
    rightLane = &street->components[*street->find(katana::cad::road::Side::Right, "lane")];
    ASSERT_EQ(leftLane->courses.size(), 3u);
    EXPECT_EQ(leftLane->courses[0].name, "seal");
    EXPECT_EQ(leftLane->courses[1].name, "AC14");
    EXPECT_EQ(leftLane->courses[2].material, "crushed rock");
    EXPECT_EQ(leftLane->courses[2].thickness, 0.2);
    EXPECT_EQ(rightLane->courses[2].thickness, 0.25);
    session.ok("ROADTEMPLATE REMOVE street lane2");
    street = session.document.roadTemplates().find("street");
    EXPECT_EQ(street->side(katana::cad::road::Side::Right).size(), 2u);

    // Every one of those was a step, and undoing them all leaves no key.
    const std::size_t steps = session.steps();
    EXPECT_EQ(steps, 11u);
    session.ok("UNDO " + std::to_string(steps));
    EXPECT_TRUE(session.document.roadTemplates().templates.empty());
    EXPECT_FALSE(session.document.metadata().unknownKeys.contains("road_templates"));
}

TEST(RoadVerbs, ABareNameIsRemovedFromBothSidesWhicheverSideWasMadeFirst)
{
    using katana::cad::road::Side;
    // A side's first component is appended at the end of the vector, so a
    // template whose right side was made first is stored
    //   [right:lane 0, right:shoulder 1, left:lane 2, left:shoulder 3]
    // and the left lane's index (2) is the larger one: REMOVE lane must erase
    // 2 and then 0, and never the 0 first (which slid left:shoulder into 2).
    Session session;
    session.ok("ROADTEMPLATE NEW four");
    session.ok("ROADTEMPLATE ADD four right lane");
    session.ok("ROADTEMPLATE ADD four right shoulder");
    session.ok("ROADTEMPLATE ADD four left lane");
    session.ok("ROADTEMPLATE ADD four left shoulder");
    const auto* four = session.document.roadTemplates().find("four");
    ASSERT_NE(four, nullptr);
    ASSERT_EQ(four->components.size(), 4u);
    ASSERT_EQ(four->components[0].side, Side::Right) << "the premise: right is stored first";
    ASSERT_EQ(four->components[2].side, Side::Left);
    const std::size_t before = session.steps();
    const std::string reply = session.ok("ROADTEMPLATE REMOVE four lane");
    EXPECT_TRUE(contains(reply, "side=left name=lane removed")) << reply;
    EXPECT_TRUE(contains(reply, "side=right name=lane removed")) << reply;
    EXPECT_EQ(session.steps(), before + 1) << "both sides, one undo step";
    four = session.document.roadTemplates().find("four");
    ASSERT_EQ(four->components.size(), 2u);
    ASSERT_TRUE(four->find(Side::Left, "shoulder").has_value()) << "the left shoulder stays";
    ASSERT_TRUE(four->find(Side::Right, "shoulder").has_value()) << "the right shoulder stays";
    EXPECT_FALSE(four->find(Side::Left, "lane").has_value());
    EXPECT_FALSE(four->find(Side::Right, "lane").has_value());
    session.ok("UNDO");
    EXPECT_EQ(session.document.roadTemplates().find("four")->components.size(), 4u);

    // Two components only, right made before left: the indices are [1, 0],
    // and walking them with the 0 erased first read past the end.
    session.ok("ROADTEMPLATE NEW two");
    session.ok("ROADTEMPLATE ADD two right lane");
    session.ok("ROADTEMPLATE ADD two left lane");
    session.ok("ROADTEMPLATE REMOVE two lane");
    EXPECT_TRUE(session.document.roadTemplates().find("two")->components.empty());

    // ADD both after a right-only component stores [right 0, right 1, left 2].
    session.ok("ROADTEMPLATE NEW mixed");
    session.ok("ROADTEMPLATE ADD mixed right lane");
    session.ok("ROADTEMPLATE ADD mixed both shoulder");
    session.ok("ROADTEMPLATE REMOVE mixed shoulder");
    const auto* mixed = session.document.roadTemplates().find("mixed");
    ASSERT_EQ(mixed->components.size(), 1u);
    EXPECT_EQ(mixed->components[0].name, "lane");

    // A name that is on no side is refused and changes nothing.
    const std::size_t steps = session.steps();
    EXPECT_EQ(session.refused("ROADTEMPLATE REMOVE mixed nothing").code, ErrorCode::NotFound);
    EXPECT_EQ(session.steps(), steps);
    EXPECT_EQ(session.document.roadTemplates().find("mixed")->components.size(), 1u);
}

TEST(RoadVerbs, AKerbFacesInwardOrOutwardAndTheDividedRoadsMedianKerbFallsOutward)
{
    Session session;
    session.ok("ROADTEMPLATE DEFAULTS");
    // The divided road's lanes sit 100 mm under the median's top (the profile):
    // hand-worked in RoadSection.TheDividedRoadsLanesSit...
    const std::string section = session.ok("ROADTEMPLATE SECTION \"divided road\"");
    EXPECT_TRUE(contains(section, "point code=left/kerb/lip offset=2.300 level=-0.100")) << section;
    EXPECT_TRUE(contains(section, "point code=left/lane1/outer offset=5.800 level=-0.205"))
        << section;
    EXPECT_TRUE(contains(section, "point code=right/shoulder/outer offset=-11.800 level=-0.385"))
        << section;
    const std::string info = session.ok("ROADTEMPLATE INFO \"divided road\"");
    EXPECT_TRUE(contains(info, "name=kerb kind=kerb shape=mountable faces=outward")) << info;

    // The facing is a key, a word, and refused when it is neither.
    const std::size_t steps = session.steps();
    const auto sideways = session.refused("ROADTEMPLATE SET \"divided road\" kerb faces=sideways");
    EXPECT_EQ(sideways.code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(sideways.message, "inward")) << sideways.message;
    EXPECT_EQ(session.steps(), steps);
    const std::string turned = session.ok("ROADTEMPLATE SET \"divided road\" kerb faces=inward");
    EXPECT_TRUE(contains(turned, "faces=inward")) << turned;
    EXPECT_EQ(session.steps(), steps + 1);
    EXPECT_TRUE(contains(session.ok("ROADTEMPLATE SECTION \"divided road\""),
                         "point code=left/kerb/back offset=2.300 level=0.100"));
    // A flush strip takes no facing; turning a kerb to one needs it inward.
    session.ok("ROADTEMPLATE SET \"divided road\" kerb faces=outward");
    const auto strip = session.refused("ROADTEMPLATE SET \"divided road\" kerb shape=edgestrip");
    EXPECT_EQ(strip.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(strip.message, "no face")) << strip.message;
    session.ok("ROADTEMPLATE SET \"divided road\" kerb faces=inward shape=edgestrip");
    EXPECT_TRUE(contains(session.ok("ROADTEMPLATE INFO \"divided road\""),
                         "name=kerb kind=kerb shape=edgestrip width="));
}

TEST(RoadVerbs, RefusalsNameWhatIsWrongAndChangeNothing)
{
    Session session;
    session.ok("ROADTEMPLATE DEFAULTS");
    const std::size_t steps = session.steps();
    EXPECT_EQ(session.refused("ROADTEMPLATE INFO nowhere").code, ErrorCode::NotFound);
    EXPECT_EQ(session.refused("ROADTEMPLATE NEW \"urban street\"").code, ErrorCode::AlreadyExists);
    EXPECT_EQ(session.refused("ROADTEMPLATE ADD \"urban street\" up lane").code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(session.refused("ROADTEMPLATE ADD \"urban street\" left tunnel").code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(session.refused("ROADTEMPLATE SET \"urban street\" lane width=wide").code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(session.refused("ROADTEMPLATE SET \"urban street\" lane colour=#zz").code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(session.refused("ROADTEMPLATE SET \"urban street\" lane flavour=sweet").code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(session.refused("ROADTEMPLATE SET \"urban street\" lane width=0").code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(session.refused("ROADTEMPLATE SET \"urban street\" nothing width=1").code,
              ErrorCode::NotFound);
    EXPECT_EQ(session.refused("ROADTEMPLATE COURSE \"urban street\" kerb ADD x thickness=0.1").code,
              ErrorCode::InvalidArgument); // a kerb has no courses
    EXPECT_EQ(
        session.refused("ROADTEMPLATE COURSE \"urban street\" lane ADD AC14 thickness=0.1").code,
        ErrorCode::AlreadyExists);
    EXPECT_EQ(session.refused("ROADTEMPLATE COURSE \"urban street\" lane SET x ifc=BRICK").code,
              ErrorCode::NotFound);
    EXPECT_EQ(session.refused("ROADTEMPLATE COURSE \"urban street\" lane SET AC14 ifc=BRICK").code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(session.steps(), steps);
}

TEST(RoadVerbs, ACorridorIsMadeWithItsLayersTheCutHiddenAndBuiltAtOnce)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    const std::size_t before = session.steps();
    const std::string made =
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" interval=20");
    EXPECT_EQ(session.steps(), before + 1);
    EXPECT_TRUE(contains(made, "corridor name=Main alignment=road template=\"urban street\" "
                               "surface=\"\" interval=20"))
        << made;
    // No surface: the pavement alone, and a problem saying why.
    EXPECT_TRUE(contains(made, "built corridor=Main from=0.000 to=100.000 runs=1 sections=6"))
        << made;
    EXPECT_TRUE(contains(made, "no target surface")) << made;
    const auto& layers = session.document.model().layers;
    ASSERT_TRUE(layers.contains("road/earthworks/cut"));
    EXPECT_FALSE(layers.find("road/earthworks/cut")->visible);
    EXPECT_TRUE(layers.find("road/earthworks/fill")->visible);
    EXPECT_TRUE(layers.contains("road/pavement"));
    EXPECT_TRUE(layers.contains("road/kerb"));
    // One undo takes the corridor and its layers.
    session.ok("UNDO");
    EXPECT_TRUE(session.document.corridors().corridors.empty());
    EXPECT_FALSE(session.document.model().layers.contains("road/earthworks/cut"));
}

TEST(RoadVerbs, ACorridorNeedsWhatItNames)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("ALIGN NEW plain 0,0 50,0");
    EXPECT_EQ(session.refused("CORRIDOR NEW A ALIGNMENT nowhere TEMPLATE \"urban street\"").code,
              ErrorCode::NotFound);
    EXPECT_EQ(session.refused("CORRIDOR NEW A ALIGNMENT plain TEMPLATE \"urban street\"").code,
              ErrorCode::InvalidArgument); // no design profile
    EXPECT_EQ(session.refused("CORRIDOR NEW A ALIGNMENT road TEMPLATE nowhere").code,
              ErrorCode::NotFound);
    EXPECT_EQ(
        session.refused("CORRIDOR NEW A ALIGNMENT road TEMPLATE \"urban street\" surface=none")
            .code,
        ErrorCode::NotFound);
    EXPECT_EQ(
        session.refused("CORRIDOR NEW A ALIGNMENT road TEMPLATE \"urban street\" interval=0").code,
        ErrorCode::InvalidArgument);
    EXPECT_EQ(session.refused("CORRIDOR NEW A ALIGNMENT road").code, ErrorCode::ParseFailure);
    session.ok("CORRIDOR NEW A ALIGNMENT road TEMPLATE \"urban street\"");
    // A template a corridor uses is not deleted from under it.
    const auto used = session.refused("ROADTEMPLATE DELETE \"urban street\"");
    EXPECT_TRUE(contains(used.message, "corridor A")) << used.message;
    EXPECT_EQ(session.refused("CORRIDOR REGION A ADD 60 40 \"divided road\"").code,
              ErrorCode::InvalidArgument);
}

TEST(RoadVerbs, RenamingATemplateRenamesItInItsCorridorsInTheSameStep)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    session.ok("CORRIDOR REGION Main ADD 40 60 \"urban street\"");
    const std::size_t before = session.steps();
    const std::string renamed = session.ok("ROADTEMPLATE RENAME \"urban street\" town");
    EXPECT_TRUE(contains(renamed, "corridors=1")) << renamed;
    EXPECT_EQ(session.steps(), before + 1);
    const auto* corridor = session.document.corridors().find("main");
    ASSERT_NE(corridor, nullptr);
    EXPECT_EQ(corridor->templateName, "town");
    EXPECT_EQ(corridor->regions.front().templateName, "town");
    session.ok("UNDO");
    EXPECT_EQ(session.document.corridors().find("Main")->templateName, "urban street");
    EXPECT_NE(session.document.roadTemplates().find("urban street"), nullptr);
}

TEST(RoadVerbs, RegionsAndSuperelevationAreKeptInStationOrder)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"rural two-lane\"");
    session.ok("CORRIDOR REGION Main ADD 60 80 \"divided road\"");
    session.ok("CORRIDOR REGION Main ADD 10 20 \"urban street\"");
    EXPECT_EQ(session.refused("CORRIDOR REGION Main ADD 15 30 \"urban street\"").code,
              ErrorCode::InvalidArgument); // overlaps 10 to 20
    session.ok("CORRIDOR SUPERELEVATION Main ADD 70 6 -6");
    session.ok("CORRIDOR SUPERELEVATION Main ADD 30 -3 -3");
    // The same station again replaces its entry.
    session.ok("CORRIDOR SUPERELEVATION Main ADD 70 5 -5");
    const auto* corridor = session.document.corridors().find("Main");
    ASSERT_NE(corridor, nullptr);
    ASSERT_EQ(corridor->regions.size(), 2u);
    EXPECT_EQ(corridor->regions[0].start, 10.0);
    EXPECT_EQ(corridor->regions[1].templateName, "divided road");
    ASSERT_EQ(corridor->superelevation.size(), 2u);
    EXPECT_EQ(corridor->superelevation[0].station, 30.0);
    EXPECT_EQ(corridor->superelevation[1].left, 5.0);
    const std::string info = session.ok("CORRIDOR INFO Main");
    EXPECT_TRUE(contains(info, "region from=10 to=20 template=\"urban street\"")) << info;
    EXPECT_TRUE(contains(info, "superelevation station=70 left=5 right=-5")) << info;
    session.ok("CORRIDOR REGION Main REMOVE 10");
    EXPECT_EQ(session.refused("CORRIDOR REGION Main REMOVE 10").code, ErrorCode::NotFound);
    session.ok("CORRIDOR SUPERELEVATION Main CLEAR");
    EXPECT_TRUE(session.document.corridors().find("Main")->superelevation.empty());
}

TEST(RoadVerbs, QuantitiesListEachObjectWithTotalsAndWriteTheCsv)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE NEW lanes");
    session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
    session.ok("ROADTEMPLATE COURSE lanes lane REMOVE subbase");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
    const std::string quantities = session.ok("CORRIDOR QUANTITIES Main");
    // 100 m of AC14 50 mm and base 200 mm, 3.5 m wide, each side: by hand
    // 17.5 and 70 m3 a side; totals 35 and 140 m3 over 700 m2 each.
    // No target surface: no earthworks were measured, so cut, fill and net are
    // empty - not the 0.000 of a road with none (CLAUDE.md: absent is not zero).
    EXPECT_TRUE(contains(quantities, "quantities corridor=Main objects=4 cut= fill= net=\n"))
        << quantities;
    EXPECT_TRUE(contains(quantities, "object id=corridor/Main/left/lane/AC14/1 kind=pavement "
                                     "name=\"Main left lane AC14\" Length=100.000 Width=3.500 "
                                     "Depth=0.050 GrossArea=350.000 GrossVolume=17.500"))
        << quantities;
    EXPECT_TRUE(contains(quantities, "total kind=pavement material=asphalt count=2 "
                                     "length=200.000 area=700.000 volume=35.000"))
        << quantities;
    EXPECT_TRUE(contains(quantities, "material=\"crushed rock\" count=2 length=200.000 "
                                     "area=700.000 volume=140.000"))
        << quantities;

    const auto file = std::filesystem::temp_directory_path() / "katana_road_quantities.csv";
    std::filesystem::remove(file);
    const std::string written =
        session.ok("CORRIDOR QUANTITIES ALL csv=\"" + file.generic_string() + "\"");
    EXPECT_TRUE(contains(written, "rows=4")) << written;
    // RFC 4180 records end in CRLF, as the VOLUME verbs' do: read as bytes.
    std::ifstream in(file, std::ios::binary);
    std::stringstream text;
    text << in.rdbuf();
    EXPECT_TRUE(text.str().starts_with(
        "id,name,kind,facility,part,material,Length,Width,Depth,Height,GrossArea,GrossVolume\r\n"))
        << text.str();
    EXPECT_TRUE(contains(text.str(), "corridor/Main/right/lane/base/1,Main right lane base,"
                                     "pavement,Main,TRAFFICLANE,crushed rock,100.000,3.500,"
                                     "0.200,,350.000,70.000"))
        << text.str();
    in.close();
    // A file already there is not replaced without OVERWRITE, as VOLUME's is
    // not; with it, it is, and the same rows are written.
    {
        std::ofstream keep(file, std::ios::binary | std::ios::trunc);
        keep << "keep me";
    }
    const auto refused = session.refused("CORRIDOR QUANTITIES Main csv=\"" + file.generic_string() + "\"");
    EXPECT_EQ(refused.code, ErrorCode::AlreadyExists);
    EXPECT_TRUE(contains(refused.message, "OVERWRITE")) << refused.message;
    {
        std::ifstream check(file, std::ios::binary);
        std::stringstream kept;
        kept << check.rdbuf();
        EXPECT_EQ(kept.str(), "keep me");
    }
    const std::string replaced = session.ok("CORRIDOR QUANTITIES Main csv=\"" +
                                            file.generic_string() + "\" OVERWRITE");
    EXPECT_TRUE(contains(replaced, "rows=4")) << replaced;
    {
        std::ifstream check(file, std::ios::binary);
        std::stringstream rewritten;
        rewritten << check.rdbuf();
        EXPECT_TRUE(rewritten.str().starts_with("id,name,kind,")) << rewritten.str();
    }
    // OVERWRITE belongs to a file.
    EXPECT_EQ(session.refused("CORRIDOR QUANTITIES Main OVERWRITE").code, ErrorCode::ParseFailure);
    std::filesystem::remove(file);
    EXPECT_EQ(session.refused("CORRIDOR QUANTITIES nowhere").code, ErrorCode::NotFound);
}

TEST(RoadVerbs, AnObjectIsListedAndDescribedByItsId)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    const std::string list = session.ok("ROADMODEL");
    EXPECT_TRUE(contains(list, "object id=corridor/Main/left/kerb/1 kind=kerb facility=Main "
                               "part=ROADSIDE ifc=IfcKerb layer=road/kerb"))
        << list;
    EXPECT_TRUE(contains(list, "problem source=\"corridor Main\"")) << list;
    const std::string info = session.ok("ROADMODEL INFO corridor/Main/left/kerb/1");
    EXPECT_TRUE(contains(info, "property name=shape value=kerbgutter")) << info;
    EXPECT_TRUE(contains(info, "property name=mountable value=false")) << info;
    EXPECT_EQ(session.refused("ROADMODEL INFO corridor/Main/left/nothing/1").code,
              ErrorCode::NotFound);
    const std::string json = session.ok("ROADMODEL JSON");
    EXPECT_TRUE(contains(json, "\"id\": \"corridor/Main/left/kerb/1\"")) << json.substr(0, 400);
    EXPECT_TRUE(contains(json, "\"ifcClass\": \"IfcKerb\""));
}

TEST(RoadVerbs, StringsAreThreeDimensionalPolylinesThatARerunReplaces)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE NEW lanes");
    session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
    const std::string preview = session.ok("CORRIDOR STRINGS Main PREVIEW");
    EXPECT_TRUE(contains(preview, "strings created=3 replaced=0")) << preview;
    EXPECT_EQ(session.document.model().entities.size(), 0u);
    const std::size_t before = session.steps();
    const std::string first = session.ok("CORRIDOR STRINGS Main");
    EXPECT_TRUE(contains(first, "strings created=3 replaced=0 corridor=Main layer=road/strings "
                                "points=33"))
        << first;
    EXPECT_EQ(session.steps(), before + 1);
    ASSERT_EQ(session.document.model().entities.size(), 3u);
    // Each a polyline along x with its heights: the lane edge 0.105 under
    // the design level of 10.
    bool sawEdge = false;
    session.document.model().entities.forEach([&](const katana::entity::Entity& entity) {
        EXPECT_EQ(entity.layer, "road/strings");
        const auto heights = katana::entity::heightsOf(
            entity.properties,
            std::get<katana::geometry::Polyline2>(entity.geometry).vertices.size());
        ASSERT_EQ(heights.size(), 11u);
        const auto code = entity.properties.find("road.string");
        ASSERT_NE(code, entity.properties.end());
        if (std::get<std::string>(code->second) == "left/lane/outer") {
            sawEdge = true;
            ASSERT_TRUE(heights.front().has_value());
            EXPECT_NEAR(*heights.front(), 9.895, 1e-12);
        }
    });
    EXPECT_TRUE(sawEdge);
    const std::string second = session.ok("CORRIDOR STRINGS Main");
    EXPECT_TRUE(contains(second, "created=3 replaced=3")) << second;
    EXPECT_EQ(session.document.model().entities.size(), 3u);
}

TEST(RoadVerbs, ARenamedCorridorKeepsItsStringsAndItsNextStringsReplacesThemWhateverTheCase)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE NEW lanes");
    session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
    const auto tagged = [&](const std::string& name) {
        std::size_t count = 0;
        session.document.model().entities.forEach([&](const katana::entity::Entity& entity) {
            const auto found = entity.properties.find("road.corridor");
            if (found != entity.properties.end() &&
                std::get<std::string>(found->second) == name) {
                ++count;
            }
        });
        return count;
    };
    EXPECT_TRUE(contains(session.ok("CORRIDOR STRINGS Main"), "strings created=3 replaced=0"));
    ASSERT_EQ(tagged("Main"), 3u);

    // Renaming carries the strings with it, in the same undo step.
    const std::size_t before = session.steps();
    const std::string renamed = session.ok("CORRIDOR RENAME Main Road1");
    EXPECT_TRUE(contains(renamed, "corridor renamed from=Main to=Road1 strings=3")) << renamed;
    EXPECT_EQ(session.steps(), before + 1);
    EXPECT_EQ(tagged("Main"), 0u);
    EXPECT_EQ(tagged("Road1"), 3u);
    // One undo takes the name and the tags back together.
    session.ok("UNDO");
    EXPECT_EQ(tagged("Main"), 3u);
    EXPECT_EQ(tagged("Road1"), 0u);
    EXPECT_NE(session.document.corridors().find("Main"), nullptr);
    EXPECT_EQ(session.document.corridors().find("Road1"), nullptr);
    session.ok("CORRIDOR RENAME Main Road1");
    EXPECT_EQ(tagged("Road1"), 3u);
    // So the next STRINGS replaces them: it does not add a second set.
    const std::string again = session.ok("CORRIDOR STRINGS Road1");
    EXPECT_TRUE(contains(again, "strings created=3 replaced=3")) << again;
    EXPECT_EQ(session.document.model().entities.size(), 3u);

    // A case-only rename the same; and the name read ignoring case, as
    // corridors are found.
    session.ok("CORRIDOR RENAME Road1 ROAD1");
    EXPECT_EQ(tagged("ROAD1"), 3u);
    EXPECT_TRUE(contains(session.ok("CORRIDOR STRINGS road1"), "created=3 replaced=3"));
    EXPECT_EQ(session.document.model().entities.size(), 3u);

    // A corridor later given the old name, on another alignment, does not take
    // them over: its first STRINGS replaces nothing and the renamed corridor's
    // are still there.
    session.ok("ALIGN NEW other 0,200 60,200");
    session.ok("ALIGN DESIGN other 0,12 60,12");
    session.ok("CORRIDOR NEW Main ALIGNMENT other TEMPLATE lanes");
    EXPECT_TRUE(contains(session.ok("CORRIDOR STRINGS Main"), "created=3 replaced=0"));
    EXPECT_EQ(session.document.model().entities.size(), 6u);
    EXPECT_EQ(tagged("ROAD1"), 3u);
}

TEST(RoadVerbs, WhatWasNotBuiltOrMeasuredIsEmptyAndAMeasuredZeroIsZero)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    // No target surface: the earthworks are not measured. The range was built.
    const std::string made =
        session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    EXPECT_TRUE(contains(made, "built corridor=Main from=0.000 to=100.000 runs=1 sections=11"))
        << made;
    EXPECT_TRUE(contains(made, " cut= fill= net= problems=1")) << made;
    // A kerb has no area: its total says nothing of one rather than 0.000, and
    // the courses, which do have one, say it.
    const std::string quantities = session.ok("CORRIDOR QUANTITIES Main");
    EXPECT_TRUE(contains(quantities, "total kind=kerb material=concrete count=2 length=200.000 "
                                     "area= volume=19.125"))
        << quantities;
    // 3.3 m of lane and 2.3 m of parking, 100 m, both sides: 1120 m2, 56 m3.
    EXPECT_TRUE(contains(quantities, "total kind=pavement material=asphalt count=4 length=400.000 "
                                     "area=1120.000 volume=56.000"))
        << quantities;

    // A corridor that could not be built has no range: from= and to= empty.
    session.ok("ALIGN DELETE road");
    const std::string gone = session.ok("CORRIDOR BUILD Main");
    EXPECT_TRUE(contains(gone, "built corridor=Main from= to= runs=0 sections=0")) << gone;
    EXPECT_TRUE(contains(gone, " cut= fill= net= problems=1")) << gone;
    session.ok("CORRIDOR DELETE Main");

    // A range off the alignment is not built either.
    session.road();
    session.ok("CORRIDOR NEW Far ALIGNMENT road TEMPLATE \"urban street\" from=200 to=300");
    const std::string off = session.ok("CORRIDOR BUILD Far");
    EXPECT_TRUE(contains(off, "built corridor=Far from= to= runs=0")) << off;

    // Measured against a surface, a corridor with no cut says 0.000 of it: the
    // two are different. Ground at 9 under a design of 10, a 4 m lane with no
    // courses and 1 in 4 fill batters: the lane's 4 x 1 and each batter's
    // 0.5 x 4 x 1 = 2 make 6 m2 a side, 1200 m3 in 100 m.
    session.ok("ROADTEMPLATE NEW fillonly");
    session.ok("ROADTEMPLATE ADD fillonly both lane width=4 crossfall=0");
    session.ok("ROADTEMPLATE COURSE fillonly lane REMOVE AC14");
    session.ok("ROADTEMPLATE COURSE fillonly lane REMOVE base");
    session.ok("ROADTEMPLATE COURSE fillonly lane REMOVE subbase");
    session.ok("ROADTEMPLATE ADD fillonly both batter");
    addGround(session, "ground", 9.0);
    const std::string measured =
        session.ok("CORRIDOR NEW Fill ALIGNMENT road TEMPLATE fillonly surface=ground");
    EXPECT_TRUE(contains(measured, " cut=0.000 fill=1200.000 net=1200.000 problems=0")) << measured;
    EXPECT_TRUE(contains(session.ok("CORRIDOR QUANTITIES Fill"),
                         "quantities corridor=Fill objects=2 cut=0.000 fill=1200.000 net=1200.000"));
    // Another corridor with a surface does not make the unmeasured one measured.
    EXPECT_TRUE(contains(session.ok("CORRIDOR QUANTITIES Far"), " cut= fill= net="));
}

TEST(RoadVerbs, AKeyAComponentsKindDoesNotKeepIsRefusedAndNotAnsweredUpdated)
{
    Session session;
    session.ok("ROADTEMPLATE NEW t");
    session.ok("ROADTEMPLATE ADD t both lane");
    session.ok("ROADTEMPLATE ADD t both batter");
    const std::size_t steps = session.steps();
    // A lane keeps no material or batter slope, a batter no width or crossfall;
    // each was answered "updated" and made no step.
    for (const char* line :
         {"ROADTEMPLATE SET t left:lane material=concrete", "ROADTEMPLATE SET t left:lane cut=3",
          "ROADTEMPLATE SET t left:lane depth=0.4", "ROADTEMPLATE SET t left:lane shape=barrier",
          "ROADTEMPLATE SET t left:batter width=5", "ROADTEMPLATE SET t left:batter crossfall=-10",
          "ROADTEMPLATE SET t left:batter superelevated=on",
          "ROADTEMPLATE ADD t left lane height=0.3", "ROADTEMPLATE ADD t left kerb benchheight=1",
          "ROADTEMPLATE SET t lane flavour=sweet"}) {
        const auto error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << line;
        EXPECT_EQ(session.steps(), steps) << line << " made a step";
    }
    const auto lane = session.refused("ROADTEMPLATE SET t left:lane material=concrete");
    EXPECT_TRUE(contains(lane.message, "a lane has no material=; its keys are name layer colour "
                                       "width crossfall superelevated"))
        << lane.message;
    EXPECT_TRUE(contains(session.refused("ROADTEMPLATE SET t left:batter width=5").message,
                         "its keys are name layer colour cut fill maxwidth benchheight benchwidth"));
    // What a kind keeps is taken, `color` and `thickness` as `colour` and `depth`.
    session.ok("ROADTEMPLATE SET t left:lane color=#00FF00 width=3.6");
    session.ok("ROADTEMPLATE SET t left:batter cut=3 benchheight=2 benchwidth=3");
    const std::string drain = session.ok("ROADTEMPLATE ADD t left drain thickness=0.4");
    EXPECT_TRUE(contains(drain, "kind=drain")) << drain;
    EXPECT_TRUE(contains(drain, "depth=0.4")) << drain;
    EXPECT_EQ(session.steps(), steps + 3);
}

TEST(RoadVerbs, AnEdgeStripIsReportedWithoutAFaceAndKeepsItsSuperelevation)
{
    Session session;
    session.ok("ROADTEMPLATE NEW s");
    session.ok("ROADTEMPLATE ADD s both lane");
    const std::string strip = session.ok("ROADTEMPLATE ADD s left kerb shape=edgestrip");
    // A flush strip has no face to give a height to or to turn; it has a
    // crossfall, and says whether a superelevation table sets it.
    EXPECT_FALSE(contains(strip, "height=")) << strip;
    EXPECT_FALSE(contains(strip, "faces=")) << strip;
    EXPECT_TRUE(contains(strip, "shape=edgestrip width=0.15 depth=0.15 crossfall=0 superelevated=no"))
        << strip;
    // Another kerb's superelevation means nothing and is refused.
    const auto refused = session.refused("ROADTEMPLATE ADD s right kerb superelevated=on");
    EXPECT_EQ(refused.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(refused.message, "only an edge strip has")) << refused.message;
    const std::string drain = session.ok("ROADTEMPLATE ADD s right drain superelevated=on");
    EXPECT_TRUE(contains(drain, "superelevated=yes")) << drain;
}

TEST(RoadVerbs, ACorridorOffItsAlignmentSaysSoAndASectionOutsideItsRangeIsRefused)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" from=20 to=60");
    // Sections only over the corridor's stations, the ends included.
    for (const char* station : {"20", "40", "60"}) {
        EXPECT_TRUE(contains(session.ok(std::string("CORRIDOR SECTION Main ") + station),
                             "section corridor=Main station="))
            << station;
    }
    for (const char* station : {"19.9", "60.1", "90", "-5"}) {
        const auto error = session.refused(std::string("CORRIDOR SECTION Main ") + station);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << station;
        EXPECT_TRUE(contains(error.message, "is outside corridor Main's stations 20 to 60"))
            << error.message;
    }

    // A corridor whose whole range is off the alignment has no stations at all.
    session.ok("CORRIDOR SET Main from=200 to=300");
    const auto none = session.refused("CORRIDOR SECTION Main 250");
    EXPECT_EQ(none.code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(none.message, "after its alignment and profile end")) << none.message;
    // SURFACE names that, as STRINGS does, and not "the ground".
    const auto surface = session.refused("CORRIDOR SURFACE Main");
    EXPECT_EQ(surface.code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(surface.message, "after its alignment and profile end")) << surface.message;
    EXPECT_FALSE(contains(surface.message, "ground")) << surface.message;
    EXPECT_TRUE(contains(session.refused("CORRIDOR STRINGS Main").message,
                         "after its alignment and profile end"));
}

TEST(RoadVerbs, ACorridorSetsEmptyOnlyWhatItCanDoWithout)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    addGround(session, "ground", 9.0);
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" surface=ground "
               "from=10 to=90");
    const std::size_t steps = session.steps();
    // Empty clears a surface and each end of the range.
    const std::string cleared = session.ok("CORRIDOR SET Main surface= from= to=");
    EXPECT_TRUE(contains(cleared, "surface=\"\" interval=10 from= to=")) << cleared;
    // A corridor always has an alignment and a template: said so, naming the
    // key, not "no road template : ..." with the name left blank.
    for (const char* line : {"CORRIDOR SET Main alignment=", "CORRIDOR SET Main template="}) {
        const auto error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << line;
        EXPECT_TRUE(contains(error.message, "needs a name")) << error.message;
    }
    EXPECT_EQ(session.refused("CORRIDOR SET Main interval=").code, ErrorCode::ParseFailure);
    EXPECT_EQ(session.steps(), steps + 1);
    // The help says which keys clear.
    const std::string help = session.ok("HELP CORRIDOR");
    EXPECT_TRUE(contains(help, "surface= from= to=")) << help;
    EXPECT_FALSE(contains(help, "(empty clears)")) << help;
}

TEST(RoadVerbs, ATemplateIsNotDeletedWhileTheCorridorsThatMayUseItCannotBeRead)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    for (const char* damage : {"{not json", R"({"format":"katana-corridors","version":9,"corridors":[]})"}) {
        auto metadata = session.document.metadata();
        metadata.unknownKeys["corridors"] = damage;
        session.document.setMetadata(metadata);
        // The corridors are empty to the guard, which is why it must ask.
        const auto deleted = session.refused("ROADTEMPLATE DELETE \"urban street\"");
        EXPECT_EQ(deleted.code, ErrorCode::InvalidState) << damage;
        EXPECT_TRUE(contains(deleted.message, "corridors cannot be read")) << deleted.message;
        EXPECT_NE(session.document.roadTemplates().find("urban street"), nullptr);
        // And BUILD ALL does not say there are none to build.
        EXPECT_EQ(session.refused("CORRIDOR BUILD ALL").code, ErrorCode::InvalidState) << damage;
    }
}

TEST(RoadVerbs, CorridorProblemsAndQuantitiesAllListTheCorridorsAndNotThePlacedObjects)
{
    Session session;
    session.road();
    session.ok("ALIGN NEW other 0,20 100,20");
    session.ok("ALIGN DESIGN other 0,10 100,10");
    session.ok("ROADOBJECT ADD barrier ALIGNMENT other station=10 to=50");
    session.ok("ALIGN DELETE other");
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    // The placed barrier's alignment is gone: that is a problem of the road
    // model, and ROADMODEL PROBLEMS lists it among the corridor's.
    const std::string model = session.ok("ROADMODEL PROBLEMS");
    ASSERT_TRUE(contains(model, "source=\"object ")) << "the premise: the barrier has a problem\n"
                                                      << model;
    const std::string problems = session.ok("CORRIDOR PROBLEMS");
    EXPECT_TRUE(contains(problems, "problems=1\n")) << problems;
    EXPECT_TRUE(contains(problems, "source=\"corridor Main\"")) << problems;
    EXPECT_FALSE(contains(problems, "source=\"object ")) << problems;
    EXPECT_TRUE(contains(session.ok("CORRIDOR PROBLEMS Main"), "problems=1\n"));
    const std::string all = session.ok("CORRIDOR QUANTITIES ALL");
    EXPECT_TRUE(contains(all, "source=\"corridor Main\"")) << all;
    EXPECT_FALSE(contains(all, "source=\"object ")) << all;
}

TEST(RoadVerbs, ACorridorsEarthworksNameTheSurfaceAsTheDrawingSpellsItWhateverWasTyped)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE NEW fillonly");
    session.ok("ROADTEMPLATE ADD fillonly both lane width=4 crossfall=0");
    session.ok("ROADTEMPLATE ADD fillonly both batter");
    addGround(session, "Ground", 11.0);
    // Typed as GROUND, built against "Ground" (the lookup ignores case): the cut
    // carries the drawing's spelling, which the IFC export finds the ground by,
    // exactly.
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE fillonly surface=GROUND");
    const std::string info = session.ok("ROADMODEL INFO corridor/Main/left/earthworks/cut/1");
    EXPECT_TRUE(contains(info, "property name=\"Target Surface\" value=Ground")) << info;
    EXPECT_FALSE(contains(info, "value=GROUND")) << info;
}

TEST(RoadVerbs, AComponentCannotTakeTheNameOfTheEarthworksObjects)
{
    Session session;
    session.ok("ROADTEMPLATE NEW t");
    session.ok("ROADTEMPLATE ADD t both lane");
    const std::size_t steps = session.steps();
    // corridor/<c>/<side>/earthworks/cut/<run> is the cut's id: a component of
    // that name with a course called cut would have made the same id.
    for (const char* line : {"ROADTEMPLATE ADD t both verge name=earthworks",
                             "ROADTEMPLATE ADD t left verge name=Earthworks",
                             "ROADTEMPLATE SET t lane name=EARTHWORKS"}) {
        const auto error = session.refused(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(contains(error.message, "earthworks")) << error.message;
    }
    EXPECT_EQ(session.steps(), steps);
    session.ok("ROADTEMPLATE SET t lane name=earthwork");
}

TEST(RoadVerbs, TheFinishedAndSubgradeSurfacesGoIntoTheDrawingAsOneStep)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE NEW lanes");
    session.ok("ROADTEMPLATE ADD lanes both lane width=3.5 crossfall=-3");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE lanes");
    const std::size_t before = session.steps();
    const std::string reply = session.ok("CORRIDOR SURFACE Main");
    EXPECT_TRUE(contains(reply, "surface name=\"Main finished\" kind=finished")) << reply;
    EXPECT_TRUE(contains(reply, "surface name=\"Main subgrade\" kind=subgrade")) << reply;
    EXPECT_EQ(session.steps(), before + 1);
    const auto* finished = session.document.surfaces().find("Main finished");
    ASSERT_NE(finished, nullptr);
    EXPECT_NEAR(*finished->surface->elevationAt(50.0, 0.0), 10.0, 1e-9);
    const auto* subgrade = session.document.surfaces().find("Main subgrade");
    ASSERT_NE(subgrade, nullptr);
    // The lane's courses as they come: 50 + 200 + 250 mm.
    EXPECT_NEAR(*subgrade->surface->elevationAt(50.0, 0.0), 9.5, 1e-9);
    // Again: replaced in place, not added.
    session.ok("CORRIDOR SURFACE Main");
    EXPECT_EQ(session.document.surfaces().all().size(), 2u);
}

TEST(RoadVerbs, ARegionKeepsItsOwnStringsAndSectionsEvenWhenItIsOneIntervalLong)
{
    // The rural two-lane road (the centreline and four codes a side: 9
    // strings) with the urban street (the centreline and seven a side: 15)
    // in a region. Each run has its own strings from its first section to its
    // last, so there are 9 + 15 + 9 whatever the region's length, and the
    // surfaces take every section built - 13 in both cases (5 + 3 + 5, and
    // 5 + 2 + 6) - not the 11 the join's second sections were dropped to.
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"rural two-lane\"");
    for (const char* region : {"40 60", "40 50"}) {
        session.ok("CORRIDOR REGION Main CLEAR");
        session.ok(std::string("CORRIDOR REGION Main ADD ") + region + " \"urban street\"");
        const std::string strings = session.ok("CORRIDOR STRINGS Main PREVIEW");
        EXPECT_TRUE(contains(strings, "strings created=33 replaced=0")) << region << "\n" << strings;
        const std::string surface = session.ok("CORRIDOR SURFACE Main PREVIEW");
        EXPECT_TRUE(contains(surface, "kind=finished")) << surface;
        std::size_t thirteen = 0;
        for (std::size_t at = surface.find("sections=13"); at != std::string::npos;
             at = surface.find("sections=13", at + 1)) {
            ++thirteen;
        }
        EXPECT_EQ(thirteen, 2u) << region << " finished and subgrade: " << surface;
    }
}

TEST(RoadVerbs, TheRoadModelIsMadeAgainWhenATemplateChangesAndNotOtherwise)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    const auto first = katana::cad::bim::bimModel(session.document);
    EXPECT_EQ(katana::cad::bim::bimModel(session.document), first) << "asked twice";
    session.ok("LINE 0,50 10,50");
    EXPECT_EQ(katana::cad::bim::bimModel(session.document), first)
        << "a line is not what the road model is made from";
    session.ok("ROADTEMPLATE SET \"urban street\" lane width=3.6");
    const auto second = katana::cad::bim::bimModel(session.document);
    EXPECT_NE(second, first);
    EXPECT_NE(*second, *first);
    session.ok("UNDO");
    EXPECT_EQ(*katana::cad::bim::bimModel(session.document), *first)
        << "undo makes the model it took back";
    session.ok("ALIGN DESIGN road 0,11 100,11");
    EXPECT_NE(*katana::cad::bim::bimModel(session.document), *first) << "the profile moved";
}

// The corridors are made again when the alignments' revision or the surfaces'
// generation moves - a new alignment, a hidden surface - and the model is the
// same object if it came out the same, so that a view holding it, and the 3D
// views the road workbench hands it to, have nothing to do.
TEST(RoadVerbs, TheRoadModelIsTheSameObjectAfterAnEditThatLeftItAsItWas)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    const auto first = katana::cad::bim::bimModel(session.document);
    ASSERT_FALSE(first->objects.empty()) << "an empty model would be the same object anyway";

    // A new alignment no corridor uses: the alignments' revision moves.
    const std::uint64_t alignments = session.document.model().alignments.revision();
    session.ok("ALIGN NEW other 0,50 100,50");
    ASSERT_NE(session.document.model().alignments.revision(), alignments)
        << "the key the corridors are made again on did not move";
    EXPECT_EQ(katana::cad::bim::bimModel(session.document), first);

    // A surface no corridor uses, added and then hidden: the surfaces'
    // generation moves each time.
    auto tin = katana::terrain::TinSurface::create({{0, 100, 1}, {10, 100, 2}, {10, 110, 3}},
                                                   {{0, 1, 2}});
    ASSERT_TRUE(tin.ok());
    katana::terrain::SurfaceStore store = session.document.surfaces();
    ASSERT_TRUE(
        store.add({"unused", std::make_shared<const katana::terrain::TinSurface>(*tin), "test"})
            .ok());
    const std::uint64_t generation = session.document.surfacesGeneration();
    ASSERT_TRUE(session.document.setSurfaces(store, "TEST").ok());
    ASSERT_NE(session.document.surfacesGeneration(), generation);
    EXPECT_EQ(katana::cad::bim::bimModel(session.document), first) << "a surface was added";
    ASSERT_TRUE(store.setVisible("unused", false).ok());
    const std::uint64_t shown = session.document.surfacesGeneration();
    ASSERT_TRUE(session.document.setSurfaces(store, "TEST").ok());
    ASSERT_NE(session.document.surfacesGeneration(), shown);
    EXPECT_EQ(katana::cad::bim::bimModel(session.document), first) << "a surface was hidden";

    // What the model is made from still makes a new one.
    session.ok("ROADTEMPLATE SET \"urban street\" lane width=3.6");
    EXPECT_NE(katana::cad::bim::bimModel(session.document), first);
}

// A 3D view applies the layers it hides itself to the meshes it is handed
// (RenderViewWidget::shownMeshes), so each mesh says which layer it is on.
TEST(RoadVerbs, EachSceneMeshOfTheRoadModelNamesTheLayerItsObjectIsOn)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"");
    const auto model = katana::cad::bim::bimModel(session.document);
    const auto meshes = katana::cad::bim::sceneMeshes(*model, session.document.model(),
                                                      katana::cad::kNoLayerOverrides);
    ASSERT_FALSE(meshes.empty());
    std::size_t at = 0;
    for (const auto& object : model->objects) {
        if (object.mesh.empty()) {
            continue;
        }
        ASSERT_LT(at, meshes.size());
        EXPECT_EQ(meshes[at].layer, object.layer) << object.id;
        EXPECT_FALSE(object.layer.empty()) << object.id << " is on no layer";
        ++at;
    }
    EXPECT_EQ(at, meshes.size());
    // The layer hidden for the whole drawing takes its objects out of the list,
    // which is the drawing's rule; a view's own is the view's to apply.
    session.ok("LAYER HIDE road/kerb");
    const auto without = katana::cad::bim::sceneMeshes(*model, session.document.model(),
                                                       katana::cad::kNoLayerOverrides);
    EXPECT_LT(without.size(), meshes.size());
    for (const auto& mesh : without) {
        EXPECT_NE(mesh.layer, "road/kerb");
    }
}

TEST(RoadVerbs, SeveralComponentsAreSetByOneLineAsOneStepAndARefusedValueChangesNone)
{
    using katana::cad::road::Side;
    Session session;
    session.ok("ROADTEMPLATE DEFAULTS");
    const auto widthOf = [&](Side side, const std::string& name) {
        const auto* roadTemplate = session.document.roadTemplates().find("rural two-lane");
        return roadTemplate->components[*roadTemplate->find(side, name)].width;
    };
    // The standard rural two-lane: a 3.5 m lane (Austroads) and a 2 m shoulder
    // each side (road_template.cpp, defaultComponent and the template itself).
    ASSERT_EQ(widthOf(Side::Left, "lane"), 3.5);
    ASSERT_EQ(widthOf(Side::Right, "lane"), 3.5);
    ASSERT_EQ(widthOf(Side::Left, "shoulder"), 2.0);
    ASSERT_EQ(widthOf(Side::Right, "shoulder"), 2.0);
    const std::size_t before = session.steps();

    const std::string reply = session.ok(
        "ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane width=3.75 "
        "left:shoulder width=2.5 crossfall=-4");
    EXPECT_EQ(session.steps(), before + 1) << "three components, one step";
    EXPECT_EQ(widthOf(Side::Left, "lane"), 3.25);
    EXPECT_EQ(widthOf(Side::Right, "lane"), 3.75);
    EXPECT_EQ(widthOf(Side::Left, "shoulder"), 2.5);
    EXPECT_EQ(widthOf(Side::Right, "shoulder"), 2.0) << "the right shoulder was not named";
    EXPECT_TRUE(contains(reply, "side=left name=lane")) << reply;
    EXPECT_TRUE(contains(reply, "side=right name=lane")) << reply;
    EXPECT_TRUE(contains(reply, "side=left name=shoulder")) << reply;
    session.ok("UNDO");
    EXPECT_EQ(widthOf(Side::Left, "lane"), 3.5);
    EXPECT_EQ(widthOf(Side::Right, "lane"), 3.5);
    EXPECT_EQ(widthOf(Side::Left, "shoulder"), 2.0);
    EXPECT_EQ(session.steps(), before);

    // A value the template refuses in the LAST group leaves the first ones
    // undone: nothing is committed, so there is no step to take back.
    const auto zero = session.refused(
        "ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane width=0");
    EXPECT_TRUE(contains(zero.describe(), "positive")) << zero.describe();
    EXPECT_EQ(widthOf(Side::Left, "lane"), 3.5);
    EXPECT_EQ(session.steps(), before);
    // An unknown component and a group with no key are refused as a whole too.
    session.refused("ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 left:nothing width=1");
    EXPECT_EQ(widthOf(Side::Left, "lane"), 3.5);
    const auto bare =
        session.refused("ROADTEMPLATE SET \"rural two-lane\" left:lane width=3.25 right:lane");
    EXPECT_TRUE(contains(bare.describe(), "needs key=value")) << bare.describe();
    EXPECT_EQ(widthOf(Side::Left, "lane"), 3.5);
    EXPECT_EQ(session.steps(), before);

    // One group is the line it always was.
    session.ok("ROADTEMPLATE SET \"rural two-lane\" lane width=3.4");
    EXPECT_EQ(widthOf(Side::Left, "lane"), 3.4);
    EXPECT_EQ(widthOf(Side::Right, "lane"), 3.4);
    EXPECT_EQ(session.steps(), before + 1);

    // A group names its component as the groups before it left it, as the
    // same two lines typed one after the other would: the second finds the
    // lane the first renamed.
    session.ok("ROADTEMPLATE SET \"rural two-lane\" left:lane name=carriageway "
               "left:carriageway width=3.1");
    const auto* renamed = session.document.roadTemplates().find("rural two-lane");
    ASSERT_TRUE(renamed->find(Side::Left, "carriageway").has_value());
    EXPECT_EQ(renamed->components[*renamed->find(Side::Left, "carriageway")].width, 3.1);
    EXPECT_FALSE(renamed->find(Side::Left, "lane").has_value());
    EXPECT_EQ(session.steps(), before + 2);
}

TEST(RoadVerbs, SeveralCoursesAreSetByOneLineAsOneStepAndARefusedValueChangesNone)
{
    using katana::cad::road::Side;
    Session session;
    session.ok("ROADTEMPLATE DEFAULTS");
    const auto thicknessOf = [&](const std::string& course) {
        const auto* roadTemplate = session.document.roadTemplates().find("rural two-lane");
        const auto& lane = roadTemplate->components[*roadTemplate->find(Side::Left, "lane")];
        for (const auto& each : lane.courses) {
            if (each.name == course) {
                return each.thickness;
            }
        }
        return -1.0;
    };
    // A lane's flexible pavement: 50 mm of AC14, 200 mm of base, 250 mm of
    // subbase (road_template.cpp, defaultComponent).
    ASSERT_EQ(thicknessOf("AC14"), 0.05);
    ASSERT_EQ(thicknessOf("base"), 0.2);
    ASSERT_EQ(thicknessOf("subbase"), 0.25);
    const std::size_t before = session.steps();

    const std::string reply = session.ok(
        "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 base "
        "thickness=0.3 material=gravel");
    EXPECT_EQ(session.steps(), before + 1) << "two courses, one step";
    EXPECT_EQ(thicknessOf("AC14"), 0.06);
    EXPECT_EQ(thicknessOf("base"), 0.3);
    EXPECT_EQ(thicknessOf("subbase"), 0.25) << "not named";
    EXPECT_TRUE(contains(reply, "name=AC14")) << reply;
    EXPECT_TRUE(contains(reply, "name=base")) << reply;
    session.ok("UNDO");
    EXPECT_EQ(thicknessOf("AC14"), 0.05);
    EXPECT_EQ(thicknessOf("base"), 0.2);
    EXPECT_EQ(session.steps(), before);

    const auto zero = session.refused(
        "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 base thickness=0");
    EXPECT_TRUE(contains(zero.describe(), "positive")) << zero.describe();
    EXPECT_EQ(thicknessOf("AC14"), 0.05) << "the first course was left as it was";
    session.refused("ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 "
                    "nothing thickness=1");
    EXPECT_EQ(thicknessOf("AC14"), 0.05);
    const auto bare = session.refused(
        "ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET AC14 thickness=0.06 base");
    EXPECT_TRUE(contains(bare.describe(), "needs key=value")) << bare.describe();
    EXPECT_EQ(thicknessOf("AC14"), 0.05);
    EXPECT_EQ(session.steps(), before);

    // One course is the line it always was.
    session.ok("ROADTEMPLATE COURSE \"rural two-lane\" left:lane SET subbase thickness=0.3");
    EXPECT_EQ(thicknessOf("subbase"), 0.3);
    EXPECT_EQ(session.steps(), before + 1);
}

TEST(RoadVerbs, ARoadModelEditIsReportedAsARoadModelChangeAndALineIsNot)
{
    Session session;
    std::vector<std::uint32_t> parts;
    const auto handle = session.document.addListener(
        [&parts](const DocumentChange& change) { parts.push_back(change.parts); });
    session.ok("ROADTEMPLATE NEW t");
    ASSERT_FALSE(parts.empty());
    EXPECT_TRUE((parts.back() & DocumentChange::RoadModel) != 0);
    EXPECT_TRUE((parts.back() & DocumentChange::History) != 0);
    session.ok("LINE 0,0 1,1");
    EXPECT_FALSE((parts.back() & DocumentChange::RoadModel) != 0);
    session.ok("UNDO");
    EXPECT_FALSE((parts.back() & DocumentChange::RoadModel) != 0);
    session.ok("UNDO");
    EXPECT_TRUE((parts.back() & DocumentChange::RoadModel) != 0) << "undoing the template";
}

TEST(RoadVerbs, TemplatesAndCorridorsSurviveTheJsonTheProjectKeeps)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"divided road\" from=10 to=90");
    const auto& keys = session.document.metadata().unknownKeys;
    ASSERT_TRUE(keys.contains("road_templates"));
    ASSERT_TRUE(keys.contains("corridors"));
    const auto templates = katana::cad::road::roadTemplatesFromJson(keys.at("road_templates"));
    ASSERT_TRUE(templates.ok());
    EXPECT_EQ(*templates, session.document.roadTemplates());
    const auto corridors = katana::cad::road::corridorsFromJson(keys.at("corridors"));
    ASSERT_TRUE(corridors.ok());
    EXPECT_EQ(*corridors, session.document.corridors());
    EXPECT_EQ(corridors->corridors.front().start, 10.0);
    // A project whose corridors a newer Katana wrote: kept, not overwritten.
    auto metadata = session.document.metadata();
    metadata.unknownKeys["corridors"] =
        R"({"format":"katana-corridors","version":9,"corridors":[]})";
    session.document.setMetadata(metadata);
    EXPECT_EQ(session.document.corridorsStatus().error().code, ErrorCode::Unsupported);
    EXPECT_EQ(session.refused("CORRIDOR DELETE Main").code, ErrorCode::InvalidState);
}

TEST(RoadVerbs, HelpListsEveryRoadVerb)
{
    Session session;
    const std::string help = session.ok("HELP CORRIDOR");
    for (const char* word : {"ROADTEMPLATE ADD", "CORRIDOR NEW", "CORRIDOR SUPERELEVATION",
                             "CORRIDOR STRINGS", "CORRIDOR QUANTITIES", "ROADMODEL"}) {
        EXPECT_TRUE(contains(help, word)) << word;
    }
    EXPECT_TRUE(contains(CommandInterpreter::helpText(), "ROADTEMPLATE"));
}

TEST(RoadVerbs, ACorridorsCurveToleranceIsKeptSetAndRefusedOutsideItsRange)
{
    Session session;
    session.road();
    session.ok("ROADTEMPLATE DEFAULTS");
    const std::string made = session.ok(
        "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" curvetol=0.002");
    EXPECT_TRUE(contains(made, "interval=10 from= to= regions=0 superelevation=0 curvetol=0.002"))
        << made;
    EXPECT_EQ(session.document.corridors().corridors.front().curveTolerance, 0.002);
    // A corridor that does not say has the default, written as it is.
    session.ok("CORRIDOR NEW Plain ALIGNMENT road TEMPLATE \"urban street\"");
    EXPECT_TRUE(contains(session.ok("CORRIDOR INFO Plain"), "curvetol=0.001"));
    EXPECT_EQ(katana::cad::road::kDefaultCurveTolerance, 0.001);
    // 0 and off are the same: no section is added in a curve.
    const std::size_t steps = session.steps();
    EXPECT_TRUE(contains(session.ok("CORRIDOR SET Main curvetol=off"), "curvetol=0 "))
        << "off is written as the 0 it is";
    EXPECT_TRUE(contains(session.ok("CORRIDOR SET Main curvetol=0.05"), "curvetol=0.05"));
    EXPECT_TRUE(contains(session.ok("CORRIDOR SET Main curvetol=0"), "curvetol=0"));
    EXPECT_EQ(session.steps(), steps + 3) << "each a step of its own";
    session.ok("CORRIDOR SET Main curvetol=0.1");
    EXPECT_EQ(session.document.corridors().corridors.front().curveTolerance, 0.1);

    // The range is 0 to a tenth; a word that is not a number is a parse error.
    const std::size_t before = session.steps();
    for (const char* line :
         {"CORRIDOR SET Main curvetol=-0.001", "CORRIDOR SET Main curvetol=0.1001",
          "CORRIDOR SET Main curvetol=1", "CORRIDOR NEW Bad ALIGNMENT road TEMPLATE \"urban street\" "
                                          "curvetol=2"}) {
        EXPECT_EQ(session.refused(line).code, ErrorCode::InvalidArgument) << line;
    }
    for (const char* line : {"CORRIDOR SET Main curvetol=abc", "CORRIDOR SET Main curvetol=",
                             "CORRIDOR SET Main curvetol=nan", "CORRIDOR SET Main curvetol=inf"}) {
        EXPECT_EQ(session.refused(line).code, ErrorCode::ParseFailure) << line;
    }
    EXPECT_EQ(session.steps(), before) << "a refused value changes nothing";
    EXPECT_EQ(session.document.corridors().corridors.front().curveTolerance, 0.1);
    EXPECT_TRUE(contains(session.ok("HELP CORRIDOR"), "curvetol="));
}

TEST(RoadVerbs, ACorridorsCurveToleranceSurvivesTheProjectsJsonAndAnOldCorridorReadsAsTheDefault)
{
    using katana::cad::road::Corridor;
    using katana::cad::road::CorridorSet;
    CorridorSet set;
    Corridor corridor;
    corridor.name = "Main";
    corridor.alignment = "road";
    corridor.templateName = "t";
    corridor.curveTolerance = 0.0023;
    set.corridors = {corridor};
    corridor.name = "Other";
    corridor.curveTolerance = 0.0;
    set.corridors.push_back(corridor);
    const auto text = katana::cad::road::corridorsToJson(set);
    ASSERT_TRUE(text.ok());
    const auto back = katana::cad::road::corridorsFromJson(*text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(*back, set);
    EXPECT_EQ(back->corridors[0].curveTolerance, 0.0023);
    EXPECT_EQ(back->corridors[1].curveTolerance, 0.0) << "0 is kept, not read as absent";

    // A corridor saved before the tolerance existed has no member: the default.
    const auto old = katana::cad::road::corridorsFromJson(
        R"({"format":"katana-corridors","version":1,"corridors":[)"
        R"({"name":"Old","alignment":"road","template":"t","interval":10}]})");
    ASSERT_TRUE(old.ok()) << old.error().describe();
    EXPECT_EQ(old->corridors.front().curveTolerance, katana::cad::road::kDefaultCurveTolerance);
    // A member that is not a sensible tolerance is refused as a corridor is.
    const auto wrong = [](const char* member) {
        return katana::cad::road::corridorsFromJson(
            std::string(R"({"format":"katana-corridors","version":1,"corridors":[)"
                        R"({"name":"Bad","alignment":"road","template":"t","interval":10,)") +
            member + "}]}");
    };
    EXPECT_EQ(wrong(R"("curveTolerance":5)").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(wrong(R"("curveTolerance":-1)").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(wrong(R"("curveTolerance":"fine")").error().code, ErrorCode::ParseFailure);
}

TEST(RoadVerbs, ACurvedCorridorHasTheSectionsItsToleranceNeedsAndAStraightOneHasNoMore)
{
    // The quarter turn at 50 m (alignment A, 178.5398 m long): 20 stations of
    // the interval and the keys, and two more in each of the arc's eight gaps
    // (10 m is 0.2 radians, over the 0.0775 a tolerance of 0.001 allows, and
    // three parts do), 36. Off, the 20 alone. The straight road is 11 either way.
    Session session;
    session.ok("ROADTEMPLATE DEFAULTS");
    session.ok("ALIGN NEW A 0,0 100,0 100,100");
    session.ok("ALIGN SET A 1 50");
    session.ok("ALIGN DESIGN A 0,10 200,10");
    const std::string curved =
        session.ok("CORRIDOR NEW Bend ALIGNMENT A TEMPLATE \"urban street\"");
    EXPECT_TRUE(contains(curved, "runs=1 sections=36 ")) << curved;
    EXPECT_TRUE(contains(session.ok("CORRIDOR BUILD Bend"), "runs=1 sections=36 "));
    EXPECT_TRUE(contains(session.ok("CORRIDOR SET Bend curvetol=off"), "curvetol=0"));
    EXPECT_TRUE(contains(session.ok("CORRIDOR BUILD Bend"), "runs=1 sections=20 "));
    session.road();
    int which = 0;
    for (const char* tolerance : {"curvetol=0.001", "curvetol=0"}) {
        const std::string line = "CORRIDOR NEW Road" + std::to_string(++which) +
                                 " ALIGNMENT road TEMPLATE \"urban street\" " + tolerance;
        EXPECT_TRUE(contains(session.ok(line), "runs=1 sections=11 ")) << tolerance;
    }
}

// ---- the road model's cache keys (docs/road_design.md, "The cache") --------------------------

namespace {

// The surface `name` replaced by a different one, as an edit of a surface
// replaces it (SurfaceStore::replace): the same name, another object.
void replaceGround(Session& session, const std::string& name, double level)
{
    katana::terrain::SurfaceStore store = session.document.surfaces();
    auto shared = std::make_shared<const katana::terrain::TinSurface>(
        katana::cad::road::testing::planeGround(0.0, 0.0, level));
    ASSERT_TRUE(store.replace(name, std::move(shared)).ok());
    ASSERT_TRUE(session.document.setSurfaces(std::move(store), "TEST_SURFACE_EDIT").ok());
}

// How many times each half of the road model was made since `before`, after
// asking for the model as a view does.
katana::cad::bim::BimBuildCounts madeSince(Session& session,
                                           const katana::cad::bim::BimBuildCounts& before)
{
    (void)katana::cad::bim::bimModel(session.document);
    const auto now = katana::cad::bim::bimBuildCounts(session.document);
    return {now.corridors - before.corridors, now.placed - before.placed};
}

// A road with a corridor targeting "ground" and a sign naming "dem", with
// "other" in the drawing for no one; the model asked for once, so that its two
// halves are made.
struct CachedRoad : Session {
    CachedRoad()
    {
        road();
        ok("ROADTEMPLATE DEFAULTS");
        for (const char* name : {"ground", "dem", "other"}) {
            addGround(*this, name, 9.0);
        }
        ok("CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\" surface=ground");
        ok("ROADOBJECT ADD sign ALIGNMENT road station=50 offset=4 surface=dem");
        first = katana::cad::bim::bimModel(document);
        counts = katana::cad::bim::bimBuildCounts(document);
    }

    std::shared_ptr<const katana::cad::bim::BimModel> first;
    katana::cad::bim::BimBuildCounts counts;
};

} // namespace

TEST(RoadVerbs, AnEditOfASurfaceNeitherACorridorTargetsNorAnObjectNamesMakesNeitherHalfAgain)
{
    CachedRoad s;
    ASSERT_FALSE(s.first->objects.empty()) << "an empty model would not tell";
    // "other" replaced by another surface (an edit of it), then hidden and
    // removed, and a new one added: the surfaces' generation moves each time, and
    // neither half reads any of them.
    replaceGround(s, "other", 8.0);
    EXPECT_EQ(madeSince(s, s.counts), (katana::cad::bim::BimBuildCounts{0, 0}))
        << "an edit of a surface nothing names";
    katana::terrain::SurfaceStore store = s.document.surfaces();
    ASSERT_TRUE(store.setVisible("other", false).ok());
    ASSERT_TRUE(s.document.setSurfaces(store, "TEST").ok());
    ASSERT_TRUE(store.remove("other"));
    ASSERT_TRUE(s.document.setSurfaces(store, "TEST").ok());
    addGround(s, "another", 7.0);
    EXPECT_EQ(madeSince(s, s.counts), (katana::cad::bim::BimBuildCounts{0, 0}))
        << "hidden, removed and added";
    // And nothing else in the drawing: a line, a layer, an undo of one.
    s.ok("LINE 0,50 10,50");
    s.ok("LAYER HIDE road/kerb");
    s.ok("UNDO");
    EXPECT_EQ(madeSince(s, s.counts), (katana::cad::bim::BimBuildCounts{0, 0}))
        << "a line, a layer, an undo";
    EXPECT_EQ(katana::cad::bim::bimModel(s.document), s.first);
}

TEST(RoadVerbs, AnEditOfTheSurfaceACorridorTargetsMakesTheCorridorsAgainAndNotThePlacedObjects)
{
    CachedRoad s;
    replaceGround(s, "ground", 8.0);
    EXPECT_EQ(madeSince(s, s.counts), (katana::cad::bim::BimBuildCounts{1, 0}));
    // The earthworks are the new ground's: the model is not the one it was.
    EXPECT_NE(*katana::cad::bim::bimModel(s.document), *s.first);
    // An undo puts the old surface object back, which is another change.
    const auto counts = katana::cad::bim::bimBuildCounts(s.document);
    s.ok("UNDO");
    EXPECT_EQ(madeSince(s, counts), (katana::cad::bim::BimBuildCounts{1, 0}));
    EXPECT_EQ(*katana::cad::bim::bimModel(s.document), *s.first);
    // Removed, the corridor loses its ground (and says so): the corridors again.
    const auto beforeRemoval = katana::cad::bim::bimBuildCounts(s.document);
    katana::terrain::SurfaceStore store = s.document.surfaces();
    ASSERT_TRUE(store.remove("ground"));
    ASSERT_TRUE(s.document.setSurfaces(std::move(store), "TEST").ok());
    EXPECT_EQ(madeSince(s, beforeRemoval), (katana::cad::bim::BimBuildCounts{1, 0}));
}

TEST(RoadVerbs, AnEditOfTheSurfaceAPlacedObjectNamesMakesThePlacedObjectsAgainAndNotTheCorridors)
{
    CachedRoad s;
    replaceGround(s, "dem", 8.0);
    EXPECT_EQ(madeSince(s, s.counts), (katana::cad::bim::BimBuildCounts{0, 1}));
    // The sign stands on the new surface: its model is not the one it was.
    EXPECT_NE(*katana::cad::bim::bimModel(s.document), *s.first);
}

TEST(RoadVerbs, EachDefinitionMakesOnlyTheHalfItIsPartOfAndAnAlignmentBoth)
{
    CachedRoad s;
    // A template: the corridors', not the placed objects'.
    s.ok("ROADTEMPLATE SET \"urban street\" lane width=3.6");
    EXPECT_EQ(madeSince(s, s.counts), (katana::cad::bim::BimBuildCounts{1, 0}));
    // A placed object: the placed objects', not the corridors'.
    auto counts = katana::cad::bim::bimBuildCounts(s.document);
    s.ok("ROADOBJECT ADD sign ALIGNMENT road station=60 offset=4");
    EXPECT_EQ(madeSince(s, counts), (katana::cad::bim::BimBuildCounts{0, 1}));
    // An alignment: both read the alignments, and it moves their revision.
    counts = katana::cad::bim::bimBuildCounts(s.document);
    s.ok("ALIGN NEW elsewhere 0,500 100,500");
    EXPECT_EQ(madeSince(s, counts), (katana::cad::bim::BimBuildCounts{1, 1}));
    // The corridor's own definition: the corridors'.
    counts = katana::cad::bim::bimBuildCounts(s.document);
    s.ok("CORRIDOR SET Main interval=20");
    EXPECT_EQ(madeSince(s, counts), (katana::cad::bim::BimBuildCounts{1, 0}));
    // Asking again when nothing moved makes nothing.
    counts = katana::cad::bim::bimBuildCounts(s.document);
    EXPECT_EQ(madeSince(s, counts), (katana::cad::bim::BimBuildCounts{0, 0}));
}
