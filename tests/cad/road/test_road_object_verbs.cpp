// ROADOBJECT on the command line (include/katana/cad/road/road_object_verbs.hpp),
// driven through CommandInterpreter::run on a Document as the window's
// command line, katana_cli and katana_mcp drive it: each word, its reply, its
// refusals, and that every edit is one undo step.
//
// The site: alignment Main, straight from (0,0) to (100,0) with a flat
// design profile at level 10, so station s, offset o is the point (s, o).

#include <gtest/gtest.h>

#include <filesystem>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/road_object_verbs.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::DocumentChange;
using katana::core::ErrorCode;
using katana::geometry::Point2;

namespace fs = std::filesystem;

namespace {

struct Site {
    Site()
    {
        katana::entity::Alignment main;
        main.name = "Main";
        main.horizontal.pis = {katana::geometry::AlignmentPI{Point2(0, 0)},
                               katana::geometry::AlignmentPI{Point2(100, 0)}};
        main.vertical = katana::geometry::VerticalAlignment{
            {katana::geometry::ProfilePVI{0.0, 10.0}, katana::geometry::ProfilePVI{100.0, 10.0}}};
        const auto status = document.execute(katana::commands::createAlignment(main));
        EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    }

    std::string ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }

    katana::core::Error fails(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was not refused: " << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }

    [[nodiscard]] std::size_t objects() const { return document.roadObjects().objects.size(); }

    Document document;
    CommandInterpreter interpreter{document};
};

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

std::vector<std::string> lines(const std::string& text)
{
    std::vector<std::string> out;
    for (const std::string_view line : katana::core::splitLines(text)) {
        out.emplace_back(line);
    }
    return out;
}

} // namespace

TEST(RoadObjectVerbs, KindsListsEveryKindAndOneKindsKeysWithTheirDefaults)
{
    Site site;
    const auto kinds = lines(site.ok("ROADOBJECT KINDS"));
    ASSERT_EQ(kinds.size(), katana::cad::road::kRoadObjectKindCount);
    for (const std::string& line : kinds) {
        EXPECT_TRUE(line.starts_with("kind word=")) << line;
    }
    EXPECT_TRUE(contains(kinds.front(), "kind word=barrier title=\"Safety barrier\" shape=linear"));
    const std::string barrier = site.ok("ROADOBJECT KINDS barriers");
    EXPECT_TRUE(contains(barrier, "parameter key=height type=number unit=m default=\"by type\""))
        << barrier;
    EXPECT_TRUE(contains(barrier, "choices=wbeam,thriebeam,fshape,singleslope,wirerope"));
    EXPECT_EQ(site.fails("ROADOBJECT KINDS bench").code, ErrorCode::NotFound);
}

TEST(RoadObjectVerbs, AnAddIsOneUndoStepTogetherWithTheLayerItMade)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    EXPECT_EQ(site.objects(), 1u);
    EXPECT_TRUE(site.document.model().layers.contains("road/objects/barrier"));
    site.ok("UNDO");
    EXPECT_EQ(site.objects(), 0u);
    EXPECT_FALSE(site.document.model().layers.contains("road/objects/barrier"));
    site.ok("REDO");
    EXPECT_EQ(site.objects(), 1u);
    EXPECT_TRUE(site.document.model().layers.contains("road/objects/barrier"));
    // A second object on the layer makes no layer again.
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=40 to=60 offset=4");
    site.ok("UNDO");
    EXPECT_TRUE(site.document.model().layers.contains("road/objects/barrier"));
}

TEST(RoadObjectVerbs, RoadModelChangesAreReportedWithTheStepItsUndoAndItsRedo)
{
    Site site;
    int reported = 0;
    const auto handle = site.document.addListener([&reported](const DocumentChange& change) {
        if (change.has(DocumentChange::RoadModel)) {
            ++reported;
        }
    });
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    EXPECT_EQ(reported, 1);
    site.ok("UNDO");
    EXPECT_EQ(reported, 2);
    site.ok("REDO");
    EXPECT_EQ(reported, 3);
    // A line that changes nothing reports nothing.
    site.ok("ROADOBJECT LIST");
    EXPECT_EQ(reported, 3);
}

TEST(RoadObjectVerbs, APreviewSaysWhatWouldBeAddedAndChangesNothing)
{
    Site site;
    const auto revision = site.document.modelRevision();
    const std::string reply =
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4 PREVIEW");
    EXPECT_TRUE(reply.starts_with("preview id=barrier-1 kind=barrier")) << reply;
    EXPECT_TRUE(contains(reply, "length=20")) << reply;
    EXPECT_EQ(site.objects(), 0u);
    EXPECT_EQ(site.document.modelRevision(), revision);
    EXPECT_FALSE(site.document.model().layers.contains("road/objects/barrier"));
}

TEST(RoadObjectVerbs, AScopeThatTakesNothingIsReportedAndNotRefused)
{
    Site site;
    const std::string reply = site.ok("ROADOBJECT ADD fence ALONG FROM DRAWING");
    EXPECT_TRUE(contains(reply, "scope=drawing matched=0")) << reply;
    EXPECT_TRUE(contains(reply, "added=0")) << reply;
    EXPECT_EQ(site.objects(), 0u);
}

TEST(RoadObjectVerbs, AlongAScopeMakesOneObjectPerLineInOneStepAndCountsWhatItSkipped)
{
    Site site;
    site.ok("LINE 0,20 10,20");
    site.ok("LINE 0,30 10,30");
    site.ok("POINT 5,25");
    site.ok("SELECT ALL");
    site.ok("PROP SET elevation 2.5");
    const auto reply = lines(site.ok("ROADOBJECT ADD fence height=1.5 ALONG FROM DRAWING"));
    ASSERT_EQ(reply.size(), 3u);
    EXPECT_TRUE(contains(reply[0], "scope=drawing matched=3 lines=2 skipped=1 skippedtypes=point"))
        << reply[0];
    EXPECT_TRUE(contains(reply[1], "added id=fence-1 kind=fence placement=line points=2 source=#1"))
        << reply[1];
    EXPECT_TRUE(contains(reply[2], "added id=fence-2")) << reply[2];
    EXPECT_TRUE(contains(reply[2], "source=#2")) << reply[2];
    EXPECT_TRUE(contains(reply[2], "height=1.5")) << reply[2];
    EXPECT_EQ(site.objects(), 2u);
    site.ok("UNDO");
    EXPECT_EQ(site.objects(), 0u);
}

TEST(RoadObjectVerbs, AnAddOfSeveralLinesIsAllOrNothing)
{
    Site site;
    site.ok("LINE 0,20 10,20");
    site.ok("LINE 0,30 10,30");
    site.ok("SELECT 1");
    site.ok("PROP SET elevation 2.5");
    // The second line has no heights, so its fence cannot stand anywhere.
    const auto error = site.fails("ROADOBJECT ADD fence ALONG FROM DRAWING");
    EXPECT_TRUE(contains(error.message, "no height")) << error.describe();
    EXPECT_TRUE(contains(error.context, "#2")) << error.describe();
    EXPECT_EQ(site.objects(), 0u);
    // Given a surface it could stand on, or a line's own heights, it goes.
    site.ok("ROADOBJECT ADD fence ALONG #1");
    EXPECT_EQ(site.objects(), 1u);
}

TEST(RoadObjectVerbs, AlongSomethingThatIsNotALineIsRefused)
{
    Site site;
    site.ok("POINT 1,1");
    const auto error = site.fails("ROADOBJECT ADD barrier ALONG #1");
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(error.message, "#1 is a point, not a line")) << error.describe();
    EXPECT_EQ(site.fails("ROADOBJECT ADD barrier ALONG #99").code, ErrorCode::NotFound);
}

TEST(RoadObjectVerbs, AnAddNeedsOnePlacementAndAKind)
{
    Site site;
    EXPECT_EQ(site.fails("ROADOBJECT ADD barrier").code, ErrorCode::InvalidArgument);
    EXPECT_EQ(site.fails("ROADOBJECT ADD bench ALIGNMENT Main station=1").code,
              ErrorCode::NotFound);
    EXPECT_EQ(site.fails("ROADOBJECT ADD sign ALIGNMENT Main AT 1,1,1").code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(site.fails("ROADOBJECT ADD sign ALIGNMENT Main station=50 plinth").code,
              ErrorCode::ParseFailure);
    // A run of point objects needs both its end and its spacing.
    EXPECT_TRUE(contains(site.fails("ROADOBJECT ADD light ALIGNMENT Main station=0 spacing=30")
                             .message,
                         "spacing= needs to="));
    EXPECT_TRUE(contains(site.fails("ROADOBJECT ADD light ALIGNMENT Main station=0 to=60")
                             .message,
                         "to= needs spacing="));
    // A linear object needs its end; a culvert crosses at one station.
    EXPECT_TRUE(contains(site.fails("ROADOBJECT ADD barrier ALIGNMENT Main station=0").message,
                         "to="));
    EXPECT_TRUE(contains(site.fails("ROADOBJECT ADD culvert ALIGNMENT Main station=0 to=5")
                             .message,
                         "one station"));
    EXPECT_EQ(site.objects(), 0u);
}

TEST(RoadObjectVerbs, SetChangesKeysAsOneStep)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    const std::string reply = site.ok("ROADOBJECT SET barrier-1 type=fshape name=\"Median east\"");
    EXPECT_TRUE(reply.starts_with("set id=barrier-1 kind=barrier name=\"Median east\"")) << reply;
    EXPECT_TRUE(contains(reply, "changed=type,name")) << reply;
    const std::string info = site.ok("ROADOBJECT INFO barrier-1");
    EXPECT_TRUE(contains(info, "parameter key=type value=fshape given=yes")) << info;
    EXPECT_TRUE(contains(info, "parameter key=height value=0.813 given=no unit=m")) << info;
    site.ok("UNDO");
    EXPECT_TRUE(site.document.roadObjects().find("barrier-1")->parameters.empty());
}

TEST(RoadObjectVerbs, SetRefusesWhatTheKindDoesNotTakeOrCannotBeBuiltAndChangesNothing)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    const auto revision = site.document.modelRevision();
    EXPECT_EQ(site.fails("ROADOBJECT SET barrier-1 diameter=1").code, ErrorCode::NotFound);
    EXPECT_TRUE(contains(site.fails("ROADOBJECT SET barrier-1 to=200").message,
                         "off alignment Main"));
    EXPECT_EQ(site.fails("ROADOBJECT SET barrier-9 to=10").code, ErrorCode::NotFound);
    EXPECT_EQ(site.document.modelRevision(), revision);
    EXPECT_EQ(*site.document.roadObjects().find("barrier-1")->placement.to, 20.0);
}

TEST(RoadObjectVerbs, ACopyAtAStationKeepsItsLength)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=10 to=30 offset=4");
    const std::string reply = site.ok("ROADOBJECT COPY barrier-1 station=50 offset=-4");
    EXPECT_TRUE(contains(reply, "copied id=barrier-2")) << reply;
    EXPECT_TRUE(contains(reply, "station=50 to=70 offset=-4")) << reply;
    EXPECT_TRUE(contains(reply, "from=barrier-1")) << reply;
    EXPECT_EQ(site.objects(), 2u);
}

TEST(RoadObjectVerbs, ACopyOfAnObjectAtPointsMovesByDxAndDy)
{
    Site site;
    site.ok("ROADOBJECT ADD sign AT 5,5,1");
    site.ok("ROADOBJECT COPY sign-1 dx=10 dy=-2");
    const auto* copy = site.document.roadObjects().find("sign-2");
    ASSERT_NE(copy, nullptr);
    ASSERT_EQ(copy->placement.vertices.size(), 1u);
    EXPECT_EQ(copy->placement.vertices.front().position, Point2(15, 3));
    EXPECT_EQ(copy->placement.vertices.front().z, 1.0);
    // dx= has no meaning along an alignment.
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    EXPECT_EQ(site.fails("ROADOBJECT COPY sign-3 dx=1").code, ErrorCode::InvalidArgument);
}

TEST(RoadObjectVerbs, DeleteRefusesAnUnknownIdAndThenDeletesNothing)
{
    Site site;
    site.ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
    EXPECT_EQ(site.fails("ROADOBJECT DELETE sign-1 sign-7").code, ErrorCode::NotFound);
    EXPECT_EQ(site.objects(), 1u);
    EXPECT_EQ(site.ok("ROADOBJECT DELETE sign-1 PREVIEW"), "preview delete count=1 ids=sign-1");
    EXPECT_EQ(site.objects(), 1u);
    EXPECT_EQ(site.ok("ROADOBJECT DELETE SIGN-1"), "deleted count=1 ids=sign-1");
    EXPECT_EQ(site.objects(), 0u);
}

TEST(RoadObjectVerbs, InfoSaysACulvertsEndsAndItsSolids)
{
    // The skewed culvert of test_road_objects.cpp: inlet (55, -8.660254)
    // and outlet (45, 8.660254), at invert 8.4.
    Site site;
    site.ok("ROADOBJECT ADD culvert ALIGNMENT Main station=50 skew=30");
    const std::string info = site.ok("ROADOBJECT INFO culvert-1");
    EXPECT_TRUE(contains(info, "inlet=55,-8.660254,8.4 outlet=45,8.660254,8.4")) << info;
    EXPECT_TRUE(contains(info, "part id=object/culvert-1/barrel class=IfcPipeSegment type=CULVERT"))
        << info;
    EXPECT_TRUE(contains(info, "closed=yes")) << info;
    EXPECT_FALSE(contains(info, "closed=no")) << info;
    EXPECT_EQ(site.fails("ROADOBJECT INFO culvert-2").code, ErrorCode::NotFound);
}

TEST(RoadObjectVerbs, ListJsonIsTheStoredSetAndListIsARecordAnObject)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    site.ok("ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6");
    const auto read = katana::cad::road::roadObjectsFromJson(site.ok("ROADOBJECT LIST JSON"));
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(*read, site.document.roadObjects());
    const auto listing = lines(site.ok("ROADOBJECT LIST"));
    ASSERT_EQ(listing.size(), 3u);
    EXPECT_EQ(listing[0], "roadobjects count=2 problems=0");
    EXPECT_TRUE(listing[1].starts_with("object id=barrier-1")) << listing[1];
    // Three poles, each a pole and a lantern.
    EXPECT_TRUE(contains(listing[2], "objects=6")) << listing[2];
    EXPECT_TRUE(contains(listing[2], "count=6")) << listing[2];
}

TEST(RoadObjectVerbs, QuantitiesAreGivenPerSolidAndTotalledByKind)
{
    Site site;
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
    site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=30 to=50 offset=4");
    const std::string reply = site.ok("ROADOBJECT QUANTITIES");
    EXPECT_TRUE(contains(reply, "quantity object=barrier-1 part=object/barrier-1 class=IfcRailing"))
        << reply;
    EXPECT_TRUE(contains(reply, "total kind=barrier objects=2 length=40")) << reply;
    EXPECT_EQ(site.fails("ROADOBJECT QUANTITIES nosuch").code, ErrorCode::NotFound);
}

TEST(RoadObjectVerbs, HelpRoadObjectListsEveryWord)
{
    Site site;
    const std::string help = site.ok("HELP ROADOBJECT");
    for (const char* word : {"KINDS", "LIST", "INFO", "ADD", "SET", "COPY", "DELETE",
                             "QUANTITIES", "PROBLEMS", "ALIGNMENT", "ALONG", "AT FROM"}) {
        EXPECT_TRUE(contains(help, word)) << word;
    }
    EXPECT_TRUE(contains(CommandInterpreter::helpText(), "ROADOBJECT"));
}

TEST(RoadObjectVerbs, TheObjectsAreSavedWithTheProjectAndOpenedAgain)
{
    const fs::path directory =
        fs::temp_directory_path() / "katana-cad-tests-road-objects" / "saved.katana";
    fs::remove_all(directory.parent_path());
    katana::cad::road::RoadObjectSet saved;
    {
        Site site;
        site.ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4 type=thriebeam");
        site.ok("ROADOBJECT ADD sign AT 5,5,1 code=R1-1");
        saved = site.document.roadObjects();
        ASSERT_TRUE(site.document.saveAs(directory).ok());
    }
    Document opened;
    ASSERT_TRUE(opened.open(directory).ok());
    EXPECT_TRUE(opened.roadObjectsStatus().ok());
    EXPECT_EQ(opened.roadObjects(), saved);
    // A new drawing has none.
    opened.newDocument();
    EXPECT_TRUE(opened.roadObjects().objects.empty());
    fs::remove_all(directory.parent_path());
}

TEST(RoadObjectVerbs, TheReportForAnAgentHoldsEachDefinitionAndWhatWasMadeOfIt)
{
    Site site;
    site.ok("ROADOBJECT ADD culvert ALIGNMENT Main station=50");
    const std::string report = katana::cad::road::roadObjectsReport(site.document);
    EXPECT_TRUE(contains(report, "\"format\": \"katana-road-objects-report\"")) << report;
    EXPECT_TRUE(contains(report, "\"id\": \"culvert-1\"")) << report;
    EXPECT_TRUE(contains(report, "\"ifc_class\": \"IfcPipeSegment\"")) << report;
    EXPECT_TRUE(contains(report, "\"closed\": true")) << report;
    EXPECT_FALSE(contains(report, "\"closed\": false")) << report;
}
