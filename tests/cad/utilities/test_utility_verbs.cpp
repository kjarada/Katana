// The UTILITY verbs (utility_verbs.hpp) through CommandInterpreter::run, as
// the window's command line, katana_cli, katana_mcp and an agent type them.
//
// REPORT, VERIFY, CLEARANCE and CHECK reply with the report and never touch
// the drawing; CHECK with errors is refused with the whole check in the
// refusal. DRAW is ONE undo step that undo takes back entirely - entities,
// layers and linetypes - and a refused DRAW changes nothing.

#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <regex>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/cad/utilities/utility_verbs.hpp"

namespace fs = std::filesystem;
using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::core::ErrorCode;

namespace {

const std::string kSamples = KATANA_UTILITY_SAMPLES;
const std::string kSchedule = "\"" + kSamples + "/schedule.csv\"";
const std::string kDesign = "\"" + kSamples + "/design.csv\"";

// A document and the interpreter that types into it.
struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    // The reply to `line`; the test fails, naming the line, when it is refused.
    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    // The error `line` is refused with; the test fails when it is not.
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    std::size_t steps() const { return document.history().undoCount(); }
    std::size_t entities() const { return document.model().entities.size(); }
    std::vector<std::string> layers() const { return document.model().layers.names(); }
    bool hasLinetype(const std::string& name) const
    {
        return document.model().linetypes.contains(name);
    }
};

// A directory of the test's own, removed by name: ctest runs cases at the
// same time, and a shared one would be removed from under another.
struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-utility-verbs-" + name))
    {
        fs::remove_all(path);
        fs::create_directories(path);
    }
    ~ScratchDirectory() { fs::remove_all(path); }

    // Writes `bytes` to `name` here and returns the path, quoted for a line.
    std::string file(const std::string& name, const std::string& bytes) const
    {
        const fs::path where = path / name;
        std::ofstream out(where, std::ios::binary);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        return "\"" + where.generic_string() + "\"";
    }
};

std::string firstLine(const std::string& text)
{
    return text.substr(0, text.find('\n'));
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// A schema of the tests' own, in the shape tools/utility_schema_domains.py
// writes: the real ones are clients' documents and stay out of the repository.
constexpr std::string_view kSchema = "kind,attribute,value,detail,label\n"
                                      "schema,Example Utility Schema,0.1,,\n"
                                      "identifier,AssetId,,,\n"
                                      "field,AssetId,Alphanumerical,Yes,Asset Id\n"
                                      "field,Status,Domain List: Status,Yes,Status\n"
                                      "domain,Status,Live,,\n"
                                      "domain,Status,Dead,,\n";

} // namespace

TEST(UtilityVerbs, ReportRepliesWithTheReportAndLeavesTheDrawingAlone)
{
    Session session;
    const std::string report = session.ok("UTILITY REPORT " + kSchedule + " MINCOVER 0.6");
    EXPECT_TRUE(contains(report, "AS 5488 subsurface utility investigation: 4 lines, 14 vertices"))
        << report;
    EXPECT_TRUE(contains(report, "Minimum cover: 0.600 m"));
    EXPECT_TRUE(contains(report, "E1 / E1-3: claimed QL-B, the ground penetrating radar evidence "
                                 "supports QL-C"));
    // The front end ends the reply with its own line break.
    EXPECT_NE(report.back(), '\n');
    EXPECT_EQ(session.steps(), 0u);
    EXPECT_EQ(session.entities(), 0u);
    // Words in any case; SPACING grades as DRAW's does.
    const std::string spaced = session.ok("utility report " + kSchedule + " spacing 20");
    EXPECT_TRUE(contains(spaced, "detected spacing <= 20.000 m")) << spaced;
    EXPECT_FALSE(contains(spaced, "W1-4 -> W1-5  12.502 m  QL-C"));
}

TEST(UtilityVerbs, VerifyComparesThePotholeWithItsDetection)
{
    Session session;
    const std::string report = session.ok("UTILITY VERIFY " + kSchedule);
    EXPECT_TRUE(std::regex_search(report, std::regex("W1 +W1-3 +W1-X1 +0\\.100 +ok +-0\\.100 +ok")))
        << report;
    EXPECT_EQ(session.steps(), 0u);
}

TEST(UtilityVerbs, ClearanceShowsTheCrossingWithinTolerance)
{
    Session session;
    const std::string report =
        session.ok("UTILITY CLEARANCE " + kSchedule + " " + kDesign + " WIDTH 0.375");
    EXPECT_TRUE(std::regex_search(
        report, std::regex("W1 W1-2 -> W1-3 +QL-B +within tolerance +-0\\.263 +0\\.300 +0\\.372")))
        << report;
    EXPECT_EQ(session.steps(), 0u);
}

TEST(UtilityVerbs, CheckRepliesWithTheCheckAndRefusesASchemaWithErrorsShowingAllOfIt)
{
    Session session;
    const ScratchDirectory scratch("check");
    const std::string schema = scratch.file("schema.csv", std::string(kSchema));
    const std::string clean = scratch.file("clean.csv", "AssetId,Status,easting\nW-1,Live,1\n");
    const std::string wrong =
        scratch.file("wrong.csv", "AssetId,Status,easting\nW-1,Alive,1\nW-2,,1\n");

    const std::string passed = session.ok("UTILITY CHECK " + clean + " SCHEMA " + schema);
    EXPECT_TRUE(contains(passed, "Delivery schema check against Example Utility Schema v0.1"))
        << passed;
    EXPECT_TRUE(contains(passed, "1 rows, 1 assets (AssetId): 0 errors, 0 warnings"));

    const katana::core::Error failed =
        session.refused("UTILITY CHECK " + wrong + " schema " + schema);
    EXPECT_EQ(failed.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(failed.message, "the schedule does not meet the schema: 2 errors"))
        << failed.message;
    // The whole check, not a count: a script that stops says why.
    EXPECT_TRUE(contains(failed.message, "2 rows, 2 assets (AssetId): 2 errors, 0 warnings"));
    EXPECT_TRUE(contains(failed.message, "\"Alive\" is not in the domain")) << failed.message;
    EXPECT_TRUE(contains(failed.message, "Status is empty"));
    EXPECT_EQ(session.steps(), 0u);
}

TEST(UtilityVerbs, EachVerbRefusesWhatItCannotRun)
{
    Session session;
    const ScratchDirectory scratch("refusals");
    const auto refusal = [&session](const std::string& line) {
        return session.refused(line).describe();
    };
    EXPECT_TRUE(contains(refusal("UTILITY"), "UTILITY REPORT | VERIFY | CLEARANCE | CHECK | DRAW"));
    EXPECT_TRUE(contains(refusal("UTILITY PLOT " + kSchedule), "UTILITY REPORT | VERIFY"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT"), "usage: UTILITY REPORT <schedule.csv>"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT " + kSchedule + " MINCOVR 0.6"),
                         "unknown option MINCOVR"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT " + kSchedule + " MINCOVER 0.6 MINCOVER 1"),
                         "MINCOVER given twice"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT " + kSchedule + " SPACING -1"),
                         "SPACING needs a number of metres, not negative"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT " + kSchedule + " MINCOVER"),
                         "MINCOVER needs a number of metres"));
    EXPECT_TRUE(contains(refusal("UTILITY VERIFY " + kSchedule + " extra"),
                         "usage: UTILITY VERIFY <schedule.csv>"));
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + kSchedule), "usage: UTILITY CLEARANCE"));
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + kSchedule + " " + kDesign + " DEPTH 1"),
                         "unknown option DEPTH"));
    EXPECT_TRUE(contains(refusal("UTILITY CHECK " + kSchedule + " AGAINST " + kSchedule),
                         "usage: UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>"));
    EXPECT_TRUE(contains(refusal("UTILITY DRAW"), "usage: UTILITY DRAW <schedule.csv>"));
    EXPECT_TRUE(
        contains(refusal("UTILITY DRAW " + kSchedule + " LAYER"), "LAYER needs a layer name"));
    EXPECT_TRUE(
        contains(refusal("UTILITY DRAW " + kSchedule + " WIDTH 1"), "unknown option WIDTH"));
    EXPECT_TRUE(contains(refusal("UTILITY DRAW " + kSchedule + " LAYER a//b"), "not a layer path"));
    // A prefix that is a path, but too deep for the two levels under it, is
    // refused as the prefix, before anything is made.
    EXPECT_TRUE(contains(refusal("UTILITY DRAW " + kSchedule + " LAYER a/b/c/d/e/f/g/h/i/j/k/l/m/n/o"),
                         "the layer prefix leaves no room"));

    // A file that is not there is NotFound, by its path.
    const katana::core::Error missing = session.refused("UTILITY VERIFY \"" + kSamples +
                                                        "/no such schedule.csv\"");
    EXPECT_EQ(missing.code, ErrorCode::NotFound);
    EXPECT_TRUE(contains(missing.message, "no such schedule.csv"));
    // A schedule that does not read says where.
    const std::string broken =
        scratch.file("broken.csv", "line,point,easting,northing,method,survace\n"
                                   "W1,W1-1,0,0,EML,20\n");
    const katana::core::Error unread = session.refused("UTILITY REPORT " + broken);
    EXPECT_EQ(unread.code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(unread.describe(), "broken.csv")) << unread.describe();
    // A quote never closed is the tokenizer's refusal, as for every verb.
    EXPECT_TRUE(contains(refusal("UTILITY REPORT \"" + kSamples + "/schedule.csv"),
                         "unterminated quoted string"));

    EXPECT_EQ(session.steps(), 0u);
    EXPECT_EQ(session.entities(), 0u);
}

TEST(UtilityVerbs, APathThatIsNotUtf8IsOpenedAsTheNarrowNameItIs)
{
    // katana_cli on Windows gets its arguments, its console and a script saved
    // from an ANSI editor in the ANSI code page, where the "é" of "café" is the
    // one byte E9 - not UTF-8. Made into a UTF-8 path, that threw, and nothing
    // caught it: the process ended. It is the narrow name it is, read by the C
    // runtime as this system reads narrow names - so a file made under it is
    // found by it, whatever the code page.
    Session session;
    const ScratchDirectory scratch("narrow");
    const std::string narrow = scratch.path.generic_string() + "/caf\xE9.csv";
    const katana::core::Error missing = session.refused("UTILITY VERIFY \"" + narrow + "\"");
    EXPECT_EQ(missing.code, ErrorCode::NotFound) << missing.describe();

    {
        std::ifstream sample(kSamples + "/schedule.csv", std::ios::binary);
        std::ofstream out(narrow, std::ios::binary);
        if (!out) {
            GTEST_SKIP() << "this system makes no file of that narrow name";
        }
        out << sample.rdbuf();
    }
    const std::string reply = session.ok("UTILITY DRAW \"" + narrow + "\"");
    EXPECT_TRUE(contains(reply, "utilities drawn lines=4 vertices=14")) << reply;
}

TEST(UtilityVerbs, TheDrawRepliesWithItsRecordsBoundsFirst)
{
    Session session;
    const std::string reply = session.ok("UTILITY DRAW " + kSchedule);
    EXPECT_EQ(firstLine(reply),
              "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 "
              "bounds=334000.000,6250000.000,334040.000,6250007.200");
    EXPECT_TRUE(contains(reply, "\nline id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 "
                                "ql_c=12.502 ql_d=0.000"))
        << reply;
    EXPECT_TRUE(contains(reply, "\nline id=G1 type=gas length=40.000 ql_a=0.000 ql_b=0.000 "
                                "ql_c=0.000 ql_d=40.000"));
    EXPECT_EQ(std::count(reply.begin(), reply.end(), '\n'), 4);
    EXPECT_EQ(session.entities(), 21u);

    // What a front end frames, read from the reply as it reads any other.
    using katana::cad::utilities::drawReplyBounds;
    const auto bounds = drawReplyBounds(reply);
    ASSERT_TRUE(bounds.has_value());
    EXPECT_DOUBLE_EQ(bounds->min.x, 334000.0);
    EXPECT_DOUBLE_EQ(bounds->min.y, 6250000.0);
    EXPECT_DOUBLE_EQ(bounds->max.x, 334040.0);
    EXPECT_DOUBLE_EQ(bounds->max.y, 6250007.2);
    // Numbers written with as few digits as they need, and a record read back
    // with a Windows line end, read as well.
    const auto terse = drawReplyBounds("utilities drawn lines=1 bounds=1,2,3,4\r\n");
    ASSERT_TRUE(terse.has_value());
    EXPECT_DOUBLE_EQ(terse->min.x, 1.0);
    EXPECT_DOUBLE_EQ(terse->max.y, 4.0);
    // Anything else frames nothing: another verb's reply, a bounds= that is
    // not the draw record's, or a box that is not four numbers, min first.
    EXPECT_FALSE(drawReplyBounds(session.ok("UTILITY VERIFY " + kSchedule)).has_value());
    for (const char* other :
         {"AS 5488 subsurface utility investigation: 4 lines, 14 vertices\nbounds=1,2,3,4",
          "utilities drawn lines=1 bounds=1,2,3", "utilities drawn lines=1 bounds=1,2,3,4,5",
          "utilities drawn lines=1 bounds=1,2,x,4", "utilities drawn lines=1 bounds=3,2,1,4",
          "utilities drawn lines=1 bounds=1,4,3,2", "utilities drawn lines=1",
          "line id=W1 bounds=1,2,3,4", ""}) {
        EXPECT_FALSE(drawReplyBounds(other).has_value()) << other;
    }
}

TEST(UtilityVerbs, ADrawnScheduleIsOneUndoStep)
{
    Session session;
    const std::vector<std::string> before = session.layers();
    session.ok("UTILITY DRAW " + kSchedule);
    ASSERT_EQ(session.steps(), 1u);
    EXPECT_EQ(session.document.history().undoName(), "UTILITY DRAW");
    EXPECT_EQ(session.entities(), 21u);
    EXPECT_EQ(session.layers().size(), before.size() + 16u);
    const katana::entity::Layer* qlb = session.document.model().layers.find("utilities/water/QL-B");
    ASSERT_NE(qlb, nullptr);
    EXPECT_EQ(qlb->linetype, "utility-ql-b");
    EXPECT_EQ(qlb->color, katana::cad::utilities::utilityTypeColour(
                              katana::survey::subsurface::UtilityType::Water));
    for (const char* name : {"utility-ql-b", "utility-ql-c", "utility-ql-d"}) {
        EXPECT_TRUE(session.hasLinetype(name)) << name;
    }

    session.ok("UNDO");
    EXPECT_EQ(session.entities(), 0u);
    EXPECT_EQ(session.layers(), before);
    for (const char* name : {"utility-ql-b", "utility-ql-c", "utility-ql-d"}) {
        EXPECT_FALSE(session.hasLinetype(name)) << name;
    }

    session.ok("REDO");
    EXPECT_EQ(session.entities(), 21u);
    EXPECT_EQ(session.layers().size(), before.size() + 16u);
    EXPECT_TRUE(session.hasLinetype("utility-ql-c"));
}

TEST(UtilityVerbs, ADrawReusesALayerThatExistsAndLeavesItAsItWas)
{
    Session session;
    session.ok("LAYER NEW utilities/water/QL-B #123456");
    const auto& layers = session.document.model().layers;
    const katana::entity::Layer mine = *layers.find("utilities/water/QL-B");
    const std::size_t layersBefore = session.layers().size();
    session.ok("UTILITY DRAW " + kSchedule);
    EXPECT_EQ(session.steps(), 2u);
    const katana::entity::Layer* after = layers.find("utilities/water/QL-B");
    ASSERT_NE(after, nullptr);
    EXPECT_EQ(*after, mine);
    EXPECT_EQ(after->linetype, "continuous");
    // utilities, utilities/water and QL-B were there; 13 more are made.
    EXPECT_EQ(session.layers().size(), layersBefore + 13u);
    std::size_t onMine = 0;
    session.document.model().entities.forEach([&onMine](const katana::entity::Entity& entity) {
        onMine += entity.layer == "utilities/water/QL-B" ? 1 : 0;
    });
    EXPECT_EQ(onMine, 1u);
    // Undo takes the draw back and leaves the person's layer.
    session.ok("UNDO");
    EXPECT_EQ(session.layers().size(), layersBefore);
    EXPECT_EQ(*layers.find("utilities/water/QL-B"), mine);

    // A second draw over the first reuses every layer and linetype.
    session.ok("REDO");
    const std::size_t drawnOnce = session.layers().size();
    session.ok("UTILITY DRAW " + kSchedule);
    EXPECT_EQ(session.layers().size(), drawnOnce);
    EXPECT_EQ(session.entities(), 42u);
    EXPECT_EQ(session.steps(), 3u);
}

TEST(UtilityVerbs, ARefusedDrawChangesNothing)
{
    Session session;
    const ScratchDirectory scratch("refused-draw");
    const std::string oneVertex = scratch.file("one vertex.csv",
                                               "line,point,easting,northing,method,h_unc,type\n"
                                               "W1,W1-1,0,0,EML,0.1,water\n"
                                               "W1,W1-2,10,0,EML,0.1,\n"
                                               "W9,W9-1,5,5,EML,0.1,water\n");
    const std::vector<std::string> before = session.layers();
    const katana::core::Error error = session.refused("UTILITY DRAW " + oneVertex);
    EXPECT_TRUE(contains(error.message, "line W9 cannot be graded")) << error.describe();
    EXPECT_TRUE(contains(error.message, "nothing was drawn"));
    EXPECT_EQ(session.steps(), 0u);
    EXPECT_EQ(session.entities(), 0u);
    EXPECT_EQ(session.layers(), before);
    EXPECT_FALSE(session.hasLinetype("utility-ql-b"));

    // A locked layer the draw would use refuses it when it runs, and the
    // layers and linetypes made before it are taken back.
    session.ok("LAYER NEW utilities/water/QL-A");
    session.ok("LAYER LOCK utilities/water/QL-A");
    const std::vector<std::string> locked = session.layers();
    const katana::core::Error refused = session.refused("UTILITY DRAW " + kSchedule);
    EXPECT_TRUE(contains(refused.describe(), "locked")) << refused.describe();
    EXPECT_EQ(session.steps(), 2u);
    EXPECT_EQ(session.entities(), 0u);
    EXPECT_EQ(session.layers(), locked);
    EXPECT_FALSE(session.hasLinetype("utility-ql-b"));
}

TEST(UtilityVerbs, RecordsARoundingErrorApartDrawAsTheReportGradesThem)
{
    // A pothole recorded twice, 10 nm apart, as a reprojected export leaves
    // it. REPORT grades the 0.000 m between them; DRAW refused the whole
    // schedule over it, with the model's "polyline has zero length".
    Session session;
    const ScratchDirectory scratch("near");
    const std::string near =
        scratch.file("near.csv",
                     "line,point,easting,northing,method,level,level_ref,surface,h_unc,v_unc,"
                     "path,type\n"
                     "S1,S1-1,334100.0,6250200.0,EML,,,,0.10,,,sewer\n"
                     "S1,S1-2,334105.0,6250200.0,pothole,19.0,top,20.0,0.02,0.02,exposed,\n"
                     "S1,S1-3,334105.00000001,6250200.0,pothole,19.0,top,20.0,0.02,0.02,,\n"
                     "S1,S1-4,334110.0,6250200.0,EML,,,,0.10,,,\n");
    EXPECT_TRUE(contains(session.ok("UTILITY REPORT " + near), "S1-2 -> S1-3  0.000 m  QL-A"));
    const std::string reply = session.ok("UTILITY DRAW " + near);
    EXPECT_EQ(firstLine(reply), "utilities drawn lines=1 vertices=4 segments=3 entities=6 layers=2 "
                                "bounds=334100.000,6250200.000,334110.000,6250200.000");
    EXPECT_EQ(session.steps(), 1u);
    EXPECT_EQ(session.entities(), 6u);
}

TEST(UtilityVerbs, LayerPutsTheDrawingUnderAnotherPrefix)
{
    Session session;
    session.ok("UTILITY DRAW " + kSchedule + " LAYER \"Site Services/Located\"");
    const auto& layers = session.document.model().layers;
    EXPECT_TRUE(layers.contains("Site Services"));
    EXPECT_TRUE(layers.contains("Site Services/Located/water/QL-A"));
    EXPECT_TRUE(layers.contains("Site Services/Located/gas/points"));
    EXPECT_FALSE(layers.contains("utilities"));
    session.ok("UNDO");
    EXPECT_FALSE(layers.contains("Site Services"));
}

TEST(UtilityVerbs, SpacingAndMinCoverReachTheDrawing)
{
    Session session;
    const std::string reply =
        session.ok("UTILITY DRAW " + kSchedule + " SPACING 20 MINCOVER 0.85");
    EXPECT_TRUE(contains(reply, "line id=W1 type=water length=30.024 ql_a=1.420 ql_b=28.604 "
                                "ql_c=0.000 ql_d=0.000"))
        << reply;
    EXPECT_FALSE(session.document.model().layers.contains("utilities/water/QL-C"));
    std::size_t below = 0;
    session.document.model().entities.forEach([&below](const katana::entity::Entity& entity) {
        const auto found =
            entity.properties.find(katana::cad::utilities::keys::kCoverBelowMinimum);
        if (found != entity.properties.end() && std::get<bool>(found->second)) {
            ++below;
        }
    });
    // E1-1 0.800, E1-2 0.800, E1-4 0.780 below 0.85; E1-3 0.900 and W1's are not.
    EXPECT_EQ(below, 3u);
}

TEST(UtilityVerbs, HelpUtilityListsEveryOption)
{
    Session session;
    const std::string help = session.ok("HELP UTILITY");
    for (const char* word : {"UTILITY REPORT",
                             "UTILITY VERIFY",
                             "UTILITY CLEARANCE",
                             "UTILITY CHECK",
                             "UTILITY DRAW",
                             "MINCOVER",
                             "SPACING",
                             "WIDTH",
                             "[H <m>]",
                             "[V <m>]",
                             "MARGIN",
                             "SCHEMA",
                             "LAYER <prefix>",
                             "bounds=",
                             "UTILITY DRAW <scope>",
                             "[TYPE <type>]",
                             "[METHOD <method>]",
                             "[H_UNC <m>]",
                             "[V_UNC <m>]",
                             "[HEIGHTS surface|service|none]",
                             "[LEVEL_REF top|centre|invert]",
                             "[PATH detected|exposed|assumed]",
                             "[OWNER <text>]",
                             "[MATERIAL <text>]",
                             "[DIAMETER_MM <mm>]",
                             "[STATUS <status>]",
                             "utility.heights",
                             "points= loose= drawn= ignored=",
                             "[FIELDS column=property,...]"}) {
        EXPECT_TRUE(contains(help, word)) << word;
    }
    EXPECT_EQ(help, katana::cad::utilities::utilityVerbHelp());
    EXPECT_TRUE(contains(CommandInterpreter::helpText(), "HELP UTILITY"));
}
