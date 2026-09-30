// SURVEY IMPORT's reduction settings (src/katana_app/survey_verbs.hpp):
// SETTINGS, a file of the settings' text form, and SET, lines of it on the
// line - what they start from, what each changes, what the reply says of
// them and every refusal, with the drawing unchanged by one - and what the
// reply says of the control it held and of the adjustment. The cli.* tests
// in src/katana_app/CMakeLists.txt run the same lines through katana_cli;
// these read the document the line leaves behind.

#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <string_view>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/core/text.hpp"
#include "katana/survey/reduction_report.hpp"
#include "katana/survey/reduction_settings.hpp"
#include "katana/survey/report_summary.hpp"
#include "katana/surveyio/reader.hpp"
#include "survey_verbs.hpp"

namespace {

using katana::app::runSurveyLine;
using katana::cad::Document;
using katana::cad::DrawingSurveyPoint;
using katana::core::ErrorCode;
namespace survey = katana::survey;

std::string fixture(const std::string& relative)
{
    return std::string(KATANA_SURVEYIO_DATA) + "/" + relative;
}

// traverse.sdr less its two 08 coordinate records: nothing in it has a
// position until something holds a point.
const std::string& bareTraverse()
{
    static const std::string path = fixture("sdr/traverse_without_coordinates.sdr");
    return path;
}

// A directory of the test's own, removed afterwards.
struct ScratchDirectory {
    std::filesystem::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(std::filesystem::temp_directory_path() / ("katana-survey-verbs-" + name))
    {
        std::error_code error;
        std::filesystem::remove_all(path, error);
        std::filesystem::create_directories(path, error);
    }
    ~ScratchDirectory()
    {
        std::error_code error;
        std::filesystem::remove_all(path, error);
    }

    // Writes `text` to `name` here and returns its path as a line quotes it.
    [[nodiscard]] std::string file(const std::string& name, const std::string& text) const
    {
        const std::filesystem::path where = path / name;
        std::ofstream(where, std::ios::binary) << text;
        return "\"" + where.generic_string() + "\"";
    }
};

// A survey point on the drawing, put there as katana_cli would: FORWARD
// from a coordinate one metre south of it, due north - level, or with no
// height ("-") when the start has none.
void placePoint(Document& document, const std::string& name, const std::string& southOf,
                const std::string& heightDifference = "0")
{
    katana::cad::CommandInterpreter interpreter(document);
    const auto placed =
        interpreter.run("FORWARD " + southOf + " 0 1 " + heightDifference + " " + name);
    ASSERT_TRUE(placed.ok()) << placed.error().describe();
}

const DrawingSurveyPoint* pointNamed(const std::vector<DrawingSurveyPoint>& points,
                                     const std::string& id)
{
    for (const DrawingSurveyPoint& point : points) {
        if (point.id == id) {
            return &point;
        }
    }
    return nullptr;
}

survey::ControlSelection held(std::string id, survey::ControlOrigin origin,
                              survey::ControlComponent horizontal,
                              survey::ControlComponent vertical)
{
    survey::ControlSelection selection;
    selection.point.pointId = std::move(id);
    selection.point.northing = horizontal;
    selection.point.easting = horizontal;
    selection.point.elevation = vertical;
    selection.origin = origin;
    return selection;
}

constexpr survey::ControlComponent kFixed{survey::ControlConstraint::Fixed, 0.0};
constexpr survey::ControlComponent kFree{survey::ControlConstraint::Free, 0.0};

} // namespace

// The owner's case, on a file of our own: a traverse that gives no
// coordinates, its first station held where the drawing has it. Worked by
// hand from CP1 (500000 E, 5000000 N, 100 Z) with the textbook forms -
// H = S sin z, V = S cos z, less (1 - k/2) V H / R from H and plus
// (1 - k) H^2 / 2R on V, k = 0.13 and R = 6 371 000 m, the reduction's own
// constants - and Z = Z0 + instrument height + V - target height:
//   CP2, the backsight, due north (the 07's azimuth 0 less the reading 0 on
//   it): four pointings of 100.006 m at zenith 89.36977, H 99.9999502,
//   V 1.1000027: N 5000099.9999340, Z 100 + 1.5 + 1.1006855 - 1.6 =
//   101.0006855.
//   T1 due east (circle 90): 150.0015 m, the mean of four, at zenith 87,
//   H 149.7959282, V 7.8504756: E 500149.7957556, Z 100 + 1.5 + 7.8520040
//   - 1.65 = 107.7020040.
//   T2 from T1, oriented on CP1 at 270, circle 90, so due north: 80.001 m at
//   zenith 91, H 79.9888155, V -1.3962069: N 5000079.9888319, Z 107.7020040
//   + 1.55 - 1.3957731 - 1.7 = 106.1562309.
// The working and the reduction evaluate the same closed forms in doubles,
// differing only in the order of the means - well under a nanometre - so a
// micrometre holds the result apart from every correction in it (the
// smallest, CP2's curvature and refraction, is 16 um).
TEST(SurveyImportSettings, AFileWithNoCoordinatesIsReducedFromAPointOfTheDrawingHeldBySet)
{
    Document document;
    placePoint(document, "CP1", "500000,4999999,100");
    const auto reply = runSurveyLine(
        document, "SURVEY IMPORT \"" + bareTraverse() +
                      "\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("\nsettings file= set=1 differ=1\n"
                          "setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0\n"
                          "imported job=job-2 entities=3 layer=survey/points reduction_warnings=3"),
              std::string::npos)
        << *reply;

    ASSERT_EQ(document.surveyJobs().size(), 1U);
    const auto& job = document.surveyJobs().front();
    survey::ReductionSettings expected;
    expected.control = {held("CP1", survey::ControlOrigin::Drawing, kFixed, kFixed)};
    EXPECT_TRUE(job.settings == expected) << survey::serialiseReductionSettings(job.settings);
    // CP1 is the drawing's: held, not drawn again.
    ASSERT_EQ(job.placedPoints.size(), 3U);

    const std::vector<DrawingSurveyPoint> points = katana::cad::drawingSurveyPoints(document);
    ASSERT_EQ(points.size(), 4U);
    constexpr double kMicrometre = 1e-6;
    const struct {
        const char* id;
        double easting;
        double northing;
        double elevation;
    } worked[] = {{"CP1", 500000.0, 5000000.0, 100.0},
                  {"CP2", 500000.0, 5000099.9999340, 101.0006855},
                  {"T1", 500149.7957556, 5000000.0, 107.7020040},
                  {"T2", 500149.7957556, 5000079.9888319, 106.1562309}};
    for (const auto& point : worked) {
        SCOPED_TRACE(point.id);
        const DrawingSurveyPoint* drawn = pointNamed(points, point.id);
        ASSERT_NE(drawn, nullptr);
        EXPECT_NEAR(drawn->easting, point.easting, kMicrometre);
        EXPECT_NEAR(drawn->northing, point.northing, kMicrometre);
        ASSERT_TRUE(drawn->elevation.has_value());
        EXPECT_NEAR(*drawn->elevation, point.elevation, kMicrometre);
    }

    // One undo step: the job and its three points go, the drawing's CP1 stays.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.surveyJobs().empty());
    const std::vector<DrawingSurveyPoint> left = katana::cad::drawingSurveyPoints(document);
    ASSERT_EQ(left.size(), 1U);
    EXPECT_EQ(left.front().id, "CP1");
}

// Without SETTINGS the import starts where the wizard does: the defaults,
// holding what the file declares as control - one_setup.jxl declares A,
// with a height, so fixed in all three.
TEST(SurveyImportSettings, WithoutSettingsTheImportHoldsTheControlTheFileDeclares)
{
    Document document;
    const auto reply =
        runSurveyLine(document, "SURVEY IMPORT \"" + fixture("trimble_jxl/one_setup.jxl") + "\"");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("\nsettings file= set=0 differ=1\n"
                          "setting key=control value=A;file;fixed;0;fixed;0;fixed;0\n"
                          "imported job="),
              std::string::npos)
        << *reply;
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    survey::ReductionSettings expected;
    expected.control = {held("A", survey::ControlOrigin::File, kFixed, kFixed)};
    EXPECT_TRUE(document.surveyJobs().front().settings == expected)
        << survey::serialiseReductionSettings(document.surveyJobs().front().settings);
}

// A SETTINGS file is the whole of the settings, as a job's stored settings
// are: a file with no control line holds nothing, the control the survey
// file declares included.
TEST(SurveyImportSettings, ASettingsFileIsTakenWholeSoControlItDoesNotListIsNotHeld)
{
    ScratchDirectory scratch("whole");
    const std::string settings =
        scratch.file("atmosphere.txt", "katana-reduction-settings=1\natmospheric=none\n");
    Document document;
    const auto reply = runSurveyLine(document, "SURVEY IMPORT \"" +
                                                   fixture("trimble_jxl/one_setup.jxl") +
                                                   "\" SETTINGS " + settings);
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("\nsettings file=atmosphere.txt set=0 differ=1\n"
                          "setting key=atmospheric value=none\nimported job="),
              std::string::npos)
        << *reply;
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    survey::ReductionSettings expected;
    expected.atmospheric = survey::AtmosphericCorrection::None;
    EXPECT_TRUE(document.surveyJobs().front().settings == expected)
        << survey::serialiseReductionSettings(document.surveyJobs().front().settings);
}

// SET's control item holds its point as the wizard's Hold does: the entry of
// the same id is replaced where it stands, and a point not yet held is added
// after the others.
TEST(SurveyImportSettings, ASetControlItemReplacesThePointOfItsIdAndAddsANewOneAfter)
{
    Document document;
    placePoint(document, "Q", "30,39,5");
    const auto reply = runSurveyLine(
        document, "SURVEY IMPORT \"" + fixture("trimble_jxl/one_setup.jxl") +
                      "\" SET control=Q;drawing;fixed;0;fixed;0;free;0 "
                      "control=A;file;weighted;0.01;weighted;0.01;free;0");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    survey::ReductionSettings expected;
    expected.control = {
        held("A", survey::ControlOrigin::File, {survey::ControlConstraint::Weighted, 0.01}, kFree),
        held("Q", survey::ControlOrigin::Drawing, kFixed, kFree)};
    EXPECT_TRUE(document.surveyJobs().front().settings == expected)
        << survey::serialiseReductionSettings(document.surveyJobs().front().settings);
    EXPECT_NE(reply->find("\nsettings file= set=2 differ=2\n"
                          "setting key=control value=A;file;weighted;0.01;weighted;0.01;free;0\n"
                          "setting key=control value=Q;drawing;fixed;0;fixed;0;free;0\n"),
              std::string::npos)
        << *reply;
}

// SET over a SETTINGS file changes what it names and keeps the rest of the
// file; the reply lists the lines that are not the defaults', in the text
// form's order (the enumerations, then the numbers, then the switches, then
// control) and its words, whatever order the items came in. A SET that
// states a default is counted and differs from nothing.
TEST(SurveyImportSettings, SetChangesTheKeysItNamesAndTheReplySaysWhatIsNotTheDefault)
{
    ScratchDirectory scratch("overlay");
    const std::string settings = scratch.file(
        "held.txt", "katana-reduction-settings=1\r\nrefraction.k=0.2\r\nfaces=separate\r\n"
                    "control=CP1;drawing;fixed;0;fixed;0;fixed;0\r\n");
    Document document;
    placePoint(document, "CP1", "500000,4999999,100");
    const auto reply = runSurveyLine(
        document, "SURVEY IMPORT \"" + bareTraverse() + "\" SETTINGS " + settings +
                      " SET curvature_refraction=false refraction.k=0.13 atmospheric=auto "
                      "faces=face-left-only");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("\nsettings file=held.txt set=4 differ=3\n"
                          "setting key=faces value=face-left-only\n"
                          "setting key=curvature_refraction value=false\n"
                          "setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0\n"
                          "imported job="),
              std::string::npos)
        << *reply;
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    survey::ReductionSettings expected;
    expected.faces = survey::FaceHandling::FaceLeftOnly;
    expected.curvatureAndRefraction = false;
    expected.control = {held("CP1", survey::ControlOrigin::Drawing, kFixed, kFixed)};
    EXPECT_TRUE(document.surveyJobs().front().settings == expected)
        << survey::serialiseReductionSettings(document.surveyJobs().front().settings);
}

// A job's own settings, written in their text form, are a SETTINGS file:
// importing with it again makes a job with the very same settings.
TEST(SurveyImportSettings, AJobsSettingsWrittenOutImportTheSameWayAgain)
{
    Document first;
    placePoint(first, "CP1", "500000,4999999,100");
    ASSERT_TRUE(runSurveyLine(first, "SURVEY IMPORT \"" + bareTraverse() +
                                         "\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0 "
                                         "atmospheric=none faces=separate")
                    .ok());
    ASSERT_EQ(first.surveyJobs().size(), 1U);
    const survey::ReductionSettings& used = first.surveyJobs().front().settings;
    ScratchDirectory scratch("round-trip");
    const std::string settings =
        scratch.file("job.txt", survey::serialiseReductionSettings(used));
    Document second;
    placePoint(second, "CP1", "500000,4999999,100");
    const auto reply =
        runSurveyLine(second, "SURVEY IMPORT \"" + bareTraverse() + "\" SETTINGS " + settings);
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(second.surveyJobs().size(), 1U);
    EXPECT_TRUE(second.surveyJobs().front().settings == used);
    EXPECT_EQ(katana::cad::drawingSurveyPoints(second), katana::cad::drawingSurveyPoints(first));
}

// SET's items run to the next option word, so LAYER after them is LAYER.
TEST(SurveyImportSettings, AnOptionWordEndsSetsItems)
{
    Document document;
    placePoint(document, "CP1", "500000,4999999,100");
    const auto reply =
        runSurveyLine(document, "SURVEY IMPORT \"" + bareTraverse() +
                                    "\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0 "
                                    "atmospheric=none LAYER survey/held");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    EXPECT_EQ(document.surveyJobs().front().layer, "survey/held");
    EXPECT_EQ(document.surveyJobs().front().settings.atmospheric,
              survey::AtmosphericCorrection::None);
}

// Every refusal, and the drawing as it was: no job, no point but the one
// placed, nothing to undo but the placing.
TEST(SurveyImportSettings, EveryRefusalSaysWhyAndLeavesTheDrawingAsItWas)
{
    ScratchDirectory scratch("refusals");
    const std::string newer =
        scratch.file("newer.txt", "katana-reduction-settings=2\natmospheric=none\n");
    const std::string notSettings = scratch.file("notes.txt", "atmospheric=none\n");
    const std::string malformed =
        scratch.file("malformed.txt", "katana-reduction-settings=1\nfaces=sideways\n");
    const std::string twice = scratch.file(
        "twice.txt", "katana-reduction-settings=1\natmospheric=none\natmospheric=auto\n");
    const std::string bare = "SURVEY IMPORT \"" + bareTraverse() + "\" ";
    const struct {
        std::string line;
        ErrorCode code;
        std::string said;
    } refusals[] = {
        // The line.
        {bare + "SET", ErrorCode::InvalidArgument, "SET needs at least one <key>=<value>"},
        {bare + "SET atmospheric=none SET faces=separate", ErrorCode::InvalidArgument,
         "SET is given twice"},
        {bare + "SETTINGS a.txt SETTINGS b.txt", ErrorCode::InvalidArgument,
         "SETTINGS is given twice"},
        {bare + "FORMAT sokkia-sdr FORMAT sokkia-sdr", ErrorCode::InvalidArgument,
         "FORMAT is given twice"},
        {bare + "LAYER a LAYER b", ErrorCode::InvalidArgument, "LAYER is given twice"},
        {bare + "SETTINGS", ErrorCode::InvalidArgument, "SETTINGS needs a value"},
        {"SURVEY READ \"" + bareTraverse() + "\" SET atmospheric=none",
         ErrorCode::InvalidArgument, "'SET' is not an option of SURVEY READ"},
        {"SURVEY READ \"" + bareTraverse() + "\" SETTINGS x.txt", ErrorCode::InvalidArgument,
         "'SETTINGS' is not an option of SURVEY READ"},
        // SET's items.
        {bare + "SET bogus=1", ErrorCode::InvalidArgument,
         "'bogus' is not a reduction setting; the settings are atmospheric, "},
        {bare + "SET katana-reduction-settings=1", ErrorCode::InvalidArgument,
         "'katana-reduction-settings' is not a reduction setting"},
        {bare + "SET refraction.k 0.2", ErrorCode::InvalidArgument,
         "SET takes <key>=<value> items, and 'refraction.k' is not one"},
        {bare + "SET #atmospheric=none", ErrorCode::InvalidArgument,
         "SET takes <key>=<value> items, and '#atmospheric=none' is not one"},
        {bare + "SET \"atmospheric=none\nfaces=separate\"", ErrorCode::InvalidArgument,
         "SET item 1 holds a line break"},
        {bare + "SET refraction.k=abc", ErrorCode::ParseFailure,
         "'abc' is not a number (refraction.k)"},
        {bare + "SET faces=sideways", ErrorCode::ParseFailure,
         "'sideways' is not a choice for faces"},
        {bare + "SET refraction.k=2", ErrorCode::InvalidArgument,
         "the coefficient of refraction must lie between -1 and 1"},
        {bare + "SET iterations.max=0", ErrorCode::InvalidArgument,
         "the adjustment needs at least one iteration"},
        {bare + "SET control=CP1;drawing", ErrorCode::ParseFailure,
         "a control line needs id;origin;then constraint;sigma"},
        {bare + "SET control=CP1;drawing;weighted;0;weighted;0;free;0",
         ErrorCode::InvalidArgument, "a weighted northing needs a positive standard deviation"},
        {bare + "SET atmospheric=none atmospheric=auto", ErrorCode::InvalidArgument,
         "'atmospheric' is given twice"},
        {bare + "SET control=CP1;drawing;fixed;0;fixed;0;free;0 "
                "control=CP1;file;fixed;0;fixed;0;free;0",
         ErrorCode::InvalidArgument, "control point 'CP1' is listed twice"},
        // A SETTINGS file.
        {bare + "SETTINGS " + newer, ErrorCode::Unsupported,
         "these reduction settings were written by a newer Katana (settings version 2)"},
        {bare + "SETTINGS " + notSettings, ErrorCode::ParseFailure,
         "this text is not a set of Katana reduction settings"},
        {bare + "SETTINGS " + malformed, ErrorCode::ParseFailure,
         "line 2 of the reduction settings: 'sideways' is not a choice for faces"},
        {bare + "SETTINGS " + twice, ErrorCode::ParseFailure,
         "line 3 of the reduction settings: 'atmospheric' is given twice"},
        {bare + "SETTINGS \"" + (scratch.path / "absent.txt").generic_string() + "\"",
         ErrorCode::FileImportFailure, "the settings file cannot be read"},
        // The settings before the survey file: a mistyped key costs no read
        // of the file, so one that is not there is not what is refused.
        {"SURVEY IMPORT \"" + (scratch.path / "no survey.sdr").generic_string() +
             "\" SET bogus=1",
         ErrorCode::InvalidArgument, "'bogus' is not a reduction setting"},
        {"SURVEY IMPORT \"" + (scratch.path / "no survey.sdr").generic_string() +
             "\" SETTINGS " + newer,
         ErrorCode::Unsupported, "written by a newer Katana (settings version 2)"},
        // The reduction, on control neither the file nor the drawing places.
        {bare + "SET control=CP9;drawing;fixed;0;fixed;0;free;0", ErrorCode::NotFound,
         "Control point CP9 is not on the drawing, so it cannot be held."},
        {bare + "SET control=CP1;file;fixed;0;fixed;0;free;0", ErrorCode::NotFound,
         "Control point CP1 is not a point with coordinates in the file, so it cannot be held."},
        {bare + "SET control=CP0;drawing;fixed;0;fixed;0;fixed;0", ErrorCode::InvalidArgument,
         "Control point CP0 is held in height but has no height; give it one or free its "
         "height."},
    };
    for (const auto& refusal : refusals) {
        SCOPED_TRACE(refusal.line);
        Document document;
        placePoint(document, "CP1", "500000,4999999,100");
        // A point with no height: FORWARD from a start with none.
        placePoint(document, "CP0", "500010,4999999", "-");
        const auto reply = runSurveyLine(document, refusal.line);
        // Every row reported, not the first alone.
        EXPECT_FALSE(reply.ok()) << *reply;
        if (reply.ok()) {
            continue;
        }
        EXPECT_EQ(reply.error().code, refusal.code) << reply.error().describe();
        EXPECT_NE(reply.error().describe().find(refusal.said), std::string::npos)
            << reply.error().describe();
        EXPECT_TRUE(document.surveyJobs().empty());
        EXPECT_EQ(katana::cad::drawingSurveyPoints(document).size(), 2U);
        // Nothing to undo but the two placings.
        EXPECT_EQ(document.history().undoCount(), 2U);
    }
}

// ---- Held control, and what the adjustment made of the data ------------------------

// The reduction holds the first point of an id, and which is first is only the
// order they were drawn in: CP1 drawn at two places 100 km apart, either
// order, the line refuses to hold it and names both, in the drawing's order,
// with the drawing as it was. Before, the job sat on whichever came first.
TEST(SurveyImportSettings, AnIdTheDrawingHasAtTwoPlacesIsNotHeldAndBothAreNamed)
{
    const struct {
        const char* first;
        const char* second;
        const char* said;
    } orders[] = {
        {"500000,4999999,100", "600000,4999999,100",
         "Control point CP1 is on the drawing 2 times, at E 500000.0000 N 5000000.0000 "
         "Z 100.0000 and E 600000.0000 N 5000000.0000 Z 100.0000, so which one to hold is not "
         "known; rename or delete all but one."},
        {"600000,4999999,100", "500000,4999999,100",
         "Control point CP1 is on the drawing 2 times, at E 600000.0000 N 5000000.0000 "
         "Z 100.0000 and E 500000.0000 N 5000000.0000 Z 100.0000, so which one to hold is not "
         "known; rename or delete all but one."},
    };
    for (const auto& order : orders) {
        SCOPED_TRACE(order.first);
        Document document;
        placePoint(document, "CP1", order.first);
        placePoint(document, "CP1", order.second);
        const auto reply = runSurveyLine(document, "SURVEY IMPORT \"" + bareTraverse() +
                                                       "\" SET control=CP1;drawing;fixed;0;"
                                                       "fixed;0;fixed;0");
        ASSERT_FALSE(reply.ok()) << *reply;
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument);
        EXPECT_EQ(reply.error().message, order.said);
        EXPECT_TRUE(document.surveyJobs().empty());
        EXPECT_EQ(katana::cad::drawingSurveyPoints(document).size(), 2U);
        EXPECT_EQ(document.history().undoCount(), 2U);
    }
}

// Two points of one id at one mark - FORWARD twice from the same start - give
// the same job whichever is held, so they are held: the positions worked by
// hand at the top of this file, and the reply naming the first, as the
// reduction takes it.
TEST(SurveyImportSettings, PointsOfOneIdAtOneMarkAreHeldAsOne)
{
    Document document;
    placePoint(document, "CP1", "500000,4999999,100");
    placePoint(document, "CP1", "500000,4999999,100");
    const auto reply = runSurveyLine(
        document, "SURVEY IMPORT \"" + bareTraverse() +
                      "\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("\nheld id=CP1 from=drawing entity=1 "), std::string::npos) << *reply;
    const DrawingSurveyPoint* t1 = pointNamed(katana::cad::drawingSurveyPoints(document), "T1");
    ASSERT_NE(t1, nullptr);
    EXPECT_NEAR(t1->easting, 500149.7957556, 1e-6);
    EXPECT_NEAR(t1->northing, 5000000.0, 1e-6);
}

// The reply says where each held point was held, as the reduction took it:
// the drawing's first point of the id - FORWARD made CP1 the drawing's first
// entity, 1 m north of 500000,4999999 and level at 100 - or the file's own
// coordinates, which one_setup.jxl gives A as North 10, East 20, Elevation 3
// (read from the fixture). A number is written as every record writes one,
// the shortest text that reads back exactly: 5000000 is "5e+06".
TEST(SurveyImportSettings, TheReplySaysWhereEachHeldPointWasHeld)
{
    Document document;
    placePoint(document, "CP1", "500000,4999999,100");
    const auto held = runSurveyLine(
        document, "SURVEY IMPORT \"" + bareTraverse() +
                      "\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0");
    ASSERT_TRUE(held.ok()) << held.error().describe();
    EXPECT_NE(held->find("\nimported job=job-2 entities=3 layer=survey/points "
                         "reduction_warnings=3\n"
                         "held id=CP1 from=drawing entity=1 northing=5e+06 easting=5e+05 "
                         "height=100\n"
                         "reduction method=radiation adjustments=0 rejected=0\n"),
              std::string::npos)
        << *held;

    Document other;
    const auto declared =
        runSurveyLine(other, "SURVEY IMPORT \"" + fixture("trimble_jxl/one_setup.jxl") + "\"");
    ASSERT_TRUE(declared.ok()) << declared.error().describe();
    EXPECT_NE(declared->find("\nheld id=A from=file northing=10 easting=20 height=3\n"),
              std::string::npos)
        << *declared;
}

// Held from the drawing, a point the file never names changes nothing - the
// file's own coordinates place its points, where cli.survey_import_sokkia_sdr
// has them - and the reply's first reduction warning says so.
TEST(SurveyImportSettings, AHeldPointTheFileNeverNamesIsSaidToChangeNothing)
{
    Document document;
    placePoint(document, "BM1", "700000,6999999,10");
    const auto reply = runSurveyLine(document, "SURVEY IMPORT \"" + fixture("sdr/traverse.sdr") +
                                                   "\" SET control=BM1;drawing;fixed;0;fixed;0;"
                                                   "fixed;0");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("\nreduction method=radiation adjustments=0 rejected=0\n"
                          "reduction_warning text=\"Control point BM1 is held from the drawing, "
                          "but the file names no point BM1, so holding it changes nothing.\"\n"),
              std::string::npos)
        << *reply;
    const DrawingSurveyPoint* t1 = pointNamed(katana::cad::drawingSurveyPoints(document), "T1");
    ASSERT_NE(t1, nullptr);
    EXPECT_NEAR(t1->easting, 500149.7957556, 1e-6);
}

// Held from the drawing, a point the file gives other coordinates - CP1, which
// traverse.sdr keys in at 500000 E 5000000 N 100 Z, drawn 1 000 km away - is
// held where the drawing has it, and the first reduction warning gives both.
TEST(SurveyImportSettings, AHeldPointTheFileGivesOtherCoordinatesIsSaidWithBoth)
{
    Document document;
    placePoint(document, "CP1", "600000,5999999,200");
    const auto reply = runSurveyLine(document, "SURVEY IMPORT \"" + fixture("sdr/traverse.sdr") +
                                                   "\" SET control=CP1;drawing;fixed;0;fixed;0;"
                                                   "fixed;0");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("\nheld id=CP1 from=drawing entity=1 northing=6e+06 easting=6e+05 "
                          "height=200\n"),
              std::string::npos)
        << *reply;
    EXPECT_NE(reply->find("\nreduction_warning text=\"Control point CP1 is held where the "
                          "drawing has it, E 600000.0000 N 6000000.0000 Z 200.0000, not where "
                          "the file gives it, E 500000.0000 N 5000000.0000 Z 100.0000.\"\n"),
              std::string::npos)
        << *reply;
}

// The adjustment's outcome, record by record, from a report whose every value
// is set here: the rows rejected before the adjustment count with the
// outliers it rejected, a variance factor and a global test a run has not got
// are "none", and the method is the settings' in words.
TEST(SurveyImportSettings, EachAdjustmentIsARecordOfWhatItsReportHolds)
{
    survey::ReductionReport report;
    report.settings.method = survey::AdjustmentMethod::Network;
    report.settings.networkDimension = survey::NetworkDimension::HorizontalAndLevels;
    survey::ReportObservation kept;
    survey::ReportObservation dropped;
    dropped.rejected = true;
    report.observations = {kept, dropped, dropped};
    survey::AdjustmentReport horizontal;
    horizontal.method = "network least squares (horizontal)";
    horizontal.observations = 12;
    horizontal.unknowns = 6;
    horizontal.redundancy = 6;
    horizontal.varianceFactor = 0.25;
    horizontal.globalTest = survey::ReportGlobalTest{1.5, 1.24, 14.45, 0.05, false};
    horizontal.flaggedOutliers = {"direction A -> P", "distance A -> P"};
    horizontal.rejectedOutliers = {"distance B -> P"};
    survey::AdjustmentReport levels;
    levels.method = "network least squares (levels)";
    levels.observations = 4;
    levels.unknowns = 2;
    levels.redundancy = 2;
    levels.varianceFactor = 1.5;
    levels.globalTest = survey::ReportGlobalTest{3.0, 0.05, 7.38, 0.05, true};
    survey::AdjustmentReport once;
    once.method = "network least squares (levels)";
    once.observations = 2;
    once.unknowns = 2;
    report.adjustments = {horizontal, levels, once};
    EXPECT_EQ(katana::app::reductionRecords(report),
              "\nreduction method=\"network, horizontal and levels\" adjustments=3 rejected=3"
              "\nadjustment method=\"network least squares (horizontal)\" observations=12 "
              "unknowns=6 redundancy=6 variance_factor=0.25 global_test=failed flagged=2 "
              "rejected=1"
              "\nadjustment method=\"network least squares (levels)\" observations=4 unknowns=2 "
              "redundancy=2 variance_factor=1.5 global_test=passed flagged=0 rejected=0"
              "\nadjustment method=\"network least squares (levels)\" observations=2 unknowns=2 "
              "redundancy=0 variance_factor=none global_test=none flagged=0 rejected=0");

    EXPECT_EQ(katana::app::reductionRecords(survey::ReductionReport{}),
              "\nreduction method=radiation adjustments=0 rejected=0");
    survey::ReductionReport unrun;
    unrun.settings.method = survey::AdjustmentMethod::Traverse;
    EXPECT_EQ(katana::app::reductionRecords(unrun),
              "\nreduction method=\"traverse, Bowditch\" adjustments=0 rejected=0");
}

// A network through the line says what the adjustment made of it, as the
// wizard's message does. network_gsi8.gsi (the wizard's own fixture) has two
// setups, on A and on B, each observing the other and P and Q with a
// direction and a distance. The reduction's network takes a setup's
// directions as angles from its backsight (reduction_adjust.cpp), not as
// directions with an orientation unknown each: at A, B to P and B to Q; at
// B, A to P and A to Q - 4 angles - and the 6 distances: 10 observations.
// A and B held in plan, the unknowns are P and Q, 2 each: 4. The redundancy
// is 6, as the directions' form counts it too (12 directions and distances
// less 4 coordinates and 2 orientations): a setup's n directions with an
// orientation unknown are its n - 1 angles, one adjustment either way. The
// variance factor and the global test are the adjustment's: the reply must
// carry them as the reduction run on the same file and settings reports
// them, to the last bit.
TEST(SurveyImportSettings, ANetworkThroughTheLineSaysWhatItsAdjustmentMadeOfTheData)
{
    const std::string file = std::string(KATANA_SURVEY_UI_DATA) + "/network_gsi8.gsi";
    Document document;
    const auto reply = runSurveyLine(
        document, "SURVEY IMPORT \"" + file +
                      "\" SET adjustment.method=network control=A;file;fixed;0;fixed;0;free;0 "
                      "control=B;file;fixed;0;fixed;0;free;0");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(document.surveyJobs().size(), 1U);

    survey::ReductionSettings settings;
    settings.method = survey::AdjustmentMethod::Network;
    settings.control = {
        held("A", survey::ControlOrigin::File, kFixed, kFree),
        held("B", survey::ControlOrigin::File, kFixed, kFree),
    };
    ASSERT_TRUE(document.surveyJobs().front().settings == settings)
        << survey::serialiseReductionSettings(document.surveyJobs().front().settings);
    std::ifstream stream(file, std::ios::binary);
    const std::string bytes{std::istreambuf_iterator<char>(stream),
                            std::istreambuf_iterator<char>()};
    auto read = katana::surveyio::readSurvey(katana::surveyio::formatRegistry(), "leica-gsi",
                                             bytes, "network_gsi8.gsi", {});
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const auto direct = katana::cad::reduceForDrawing(read->project, settings, {});
    ASSERT_TRUE(direct.ok()) << direct.error().describe();
    ASSERT_EQ(direct->report.adjustments.size(), 1U);
    const survey::AdjustmentReport& adjustment = direct->report.adjustments.front();
    ASSERT_TRUE(adjustment.varianceFactor.has_value());
    ASSERT_TRUE(adjustment.globalTest.has_value());

    const std::size_t at = reply->find("\nadjustment ");
    ASSERT_NE(at, std::string::npos) << *reply;
    const auto record = katana::core::readReplyRecord(
        std::string_view(*reply).substr(at + 1, reply->find('\n', at + 1) - at - 1));
    ASSERT_TRUE(record.has_value()) << *reply;
    EXPECT_EQ(record->value("method"), "network least squares (horizontal)");
    EXPECT_EQ(record->value("observations"), "10");
    EXPECT_EQ(record->value("unknowns"), "4");
    EXPECT_EQ(record->value("redundancy"), "6");
    const auto variance = katana::core::parseFiniteDouble(record->value("variance_factor").value_or(""));
    ASSERT_TRUE(variance.has_value()) << *reply;
    EXPECT_EQ(*variance, *adjustment.varianceFactor);
    EXPECT_EQ(record->value("global_test"), adjustment.globalTest->passed ? "passed" : "failed");
    EXPECT_EQ(record->value("flagged"), std::to_string(adjustment.flaggedOutliers.size()));
    EXPECT_NE(reply->find("\nreduction method=\"network, horizontal\" adjustments=1 rejected=" +
                          std::to_string(katana::survey::rejectedObservations(direct->report)) +
                          "\n"),
              std::string::npos)
        << *reply;
}

// A SETTINGS file is read as every text file is: saved with a byte order mark,
// or as UTF-16, it is the same settings, where the parser alone called it
// not settings at all. One that is not UTF-8 - a degree sign in Windows-1252
// in a note - is read, and the guess is said.
TEST(SurveyImportSettings, ASettingsFileIsDecodedAsAnyTextFileIs)
{
    ScratchDirectory scratch("encodings");
    const std::string lines =
        "katana-reduction-settings=1\r\ncontrol=CP1;drawing;fixed;0;fixed;0;fixed;0\r\n";
    std::string utf16 = "\xFF\xFE";
    for (const char c : lines) {
        utf16 += c;
        utf16 += '\0';
    }
    const struct {
        const char* name;
        std::string bytes;
        const char* warning;
    } files[] = {
        {"bom.txt", "\xEF\xBB\xBF" + lines, nullptr},
        {"utf16.txt", utf16, nullptr},
        {"ansi.txt", "# measured at 20\xB0" "C\r\n" + lines,
         "\nsettings_warning text=\"the settings file is not UTF-8; it was read as "},
    };
    for (const auto& entry : files) {
        SCOPED_TRACE(entry.name);
        Document document;
        placePoint(document, "CP1", "500000,4999999,100");
        const auto reply =
            runSurveyLine(document, "SURVEY IMPORT \"" + bareTraverse() + "\" SETTINGS " +
                                        scratch.file(entry.name, entry.bytes));
        ASSERT_TRUE(reply.ok()) << reply.error().describe();
        EXPECT_NE(reply->find("\nsettings file=" + std::string(entry.name) +
                              " set=0 differ=1\n"
                              "setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0\n"),
                  std::string::npos)
            << *reply;
        EXPECT_EQ(reply->find("settings_warning") != std::string::npos, entry.warning != nullptr)
            << *reply;
        if (entry.warning != nullptr) {
            EXPECT_NE(reply->find(entry.warning), std::string::npos) << *reply;
        }
        ASSERT_EQ(document.surveyJobs().size(), 1U);
        EXPECT_EQ(document.surveyJobs().front().placedPoints.size(), 3U);
    }
}
