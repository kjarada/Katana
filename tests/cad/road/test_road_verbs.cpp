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
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/cad/road/road_verbs.hpp"
#include "katana/core/text.hpp"

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
    EXPECT_TRUE(contains(quantities, "quantities corridor=Main objects=4 cut=0.000 fill=0.000"))
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
    std::ifstream in(file);
    std::stringstream text;
    text << in.rdbuf();
    EXPECT_TRUE(text.str().starts_with(
        "id,name,kind,facility,part,material,Length,Width,Depth,Height,GrossArea,GrossVolume\n"))
        << text.str();
    EXPECT_TRUE(contains(text.str(), "corridor/Main/right/lane/base/1,Main right lane base,"
                                     "pavement,Main,TRAFFICLANE,crushed rock,100.000,3.500,"
                                     "0.200,,350.000,70.000"))
        << text.str();
    in.close();
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
