// SURVEY IMPORT's reduction settings (src/katana_app/survey_verbs.hpp):
// SETTINGS, a file of the settings' text form, and SET, lines of it on the
// line - what they start from, what each changes, what the reply says of
// them and every refusal, with the drawing unchanged by one - and what the
// reply says of the control it held and of the adjustment. The cli.* tests
// in src/katana_app/CMakeLists.txt run the same lines through katana_cli;
// these read the document the line leaves behind. Then, as SurveyVerbs, what
// the import does with the survey codes: CODES and LINEWORK, the drawing's
// own switches, and the two records of the reply.

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <optional>
#include <ostream>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/entity.hpp"
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

// ---- SURVEY IMPORT codes and strings what it draws --------------------------------------
//
// The customisation is the hand-written fixture
// tests/data/field_codes/test_field_codes.customisation.json, installed on
// the Document as a loaded one (CUSTOMISE is the front ends' to run; what a
// drawing has loaded is the Document's). It has a folder of its own: the
// headless checks of the window load EVERY customisation file of
// tests/data/customisation (-DCUSTOMISE_DIR), and pin what those three hold.
// Its rules, read by hand:
//   KB*   a LINE on FIELD KERB, linestyle "FIELD Kerb", colour "field kerb" -
//         a name only the fixture's own colour table knows: #C04000
//   EB*   a LINE on FIELD EDGE, a plain line, colour "blue" (a standard name)
//   CTRL  a POINT on FIELD CONTROL, a plain line, colour "red"
//
// The field file of most of these tests is tests/surveyio/data/fld/gnss.fld,
// an RTK job whose positions the file states, so nothing is reduced (the
// same five are worked beside cli.survey_import_gnss_field_file):
//   CM1   entered, no code            E 500000  N 6200000
//   R001  KB, string 1                E 500010  N 6200020  (its first position)
//   R002  KB, string 1                E 500020  N 6200020
//   R003  KB, string 1                E 500020  N 6200030
//   20    closes the string KB 1: R001, R002 and R003 are one closed line
//   R004  KB, string 1, begun again   E 500040  N 6200040  (alone: no line)
// So an import that codes and strings draws five points; codes the four KB
// ones by their string name KB1, which KB* answers, onto the one layer FIELD
// KERB in the one style "FIELD Kerb" (a style is named after its linestyle);
// draws one line, the closed KB 1; and leaves two things in no line - the
// second KB 1, of one point, and CM1, which has no code. The first of those
// is a fault and so a warning of the job's report; the second is not.

namespace {

using katana::entity::Color;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

// The fixture, installed as the session's loaded customisation. Its folder is
// reached from the survey fixtures' own (tests/surveyio/data), two up.
void installFieldCodes(Document& document)
{
    const std::string path = std::string(KATANA_SURVEYIO_DATA) +
                             "/../../data/field_codes/test_field_codes.customisation.json";
    std::ifstream stream(path, std::ios::binary);
    ASSERT_TRUE(stream.good()) << path;
    const std::string bytes{std::istreambuf_iterator<char>(stream),
                            std::istreambuf_iterator<char>()};
    auto customisation = katana::entity::customisationFromJson(bytes);
    ASSERT_TRUE(customisation.ok()) << customisation.error().describe();
    const auto installed = document.installCustomisation(
        std::move(*customisation), katana::cad::CustomisationOrigin::Loaded);
    ASSERT_TRUE(installed.ok()) << installed.error().describe();
}

// On a layer of its own at the top of the tree: an import onto survey/points
// creates "survey" with it, which its undo leaves behind (docs/survey.md, "Not
// done"), and these tests compare whole drawings.
std::string importGnss(const std::string& options = {})
{
    return "SURVEY IMPORT \"" + fixture("fld/gnss.fld") + "\" LAYER fieldwork" +
           (options.empty() ? std::string{} : " " + options);
}

std::string run(Document& document, const std::string& line)
{
    const auto reply = runSurveyLine(document, line);
    EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
    return reply.ok() ? *reply : std::string{};
}

// The reply's records about the survey codes, in the order given: every line
// whose first word is coded, unmatched_code, unmatched_codes_more or linework.
std::vector<std::string> finishRecords(const std::string& reply)
{
    std::vector<std::string> records;
    for (const std::string_view line : katana::core::splitLines(reply)) {
        for (const std::string_view word :
             {"coded ", "unmatched_code ", "unmatched_codes_more=", "linework "}) {
            if (line.starts_with(word)) {
                records.emplace_back(line);
            }
        }
    }
    return records;
}

// The reply with those records taken out: what it was before they existed.
std::string withoutFinishRecords(const std::string& reply)
{
    const std::vector<std::string> records = finishRecords(reply);
    std::string rest;
    for (const std::string_view line : katana::core::splitLines(reply)) {
        if (std::find(records.begin(), records.end(), line) == records.end()) {
            rest += line;
            rest += '\n';
        }
    }
    return rest;
}

// The value of `key` in the reply's `imported` record.
std::string importedValue(const std::string& reply, std::string_view key)
{
    for (const std::string_view line : katana::core::splitLines(reply)) {
        if (line.starts_with("imported ")) {
            const auto record = katana::core::readReplyRecord(line);
            return record ? record->value(key).value_or("<absent>") : "<not a record>";
        }
    }
    return "<no imported record>";
}

std::vector<const Entity*> linesDrawn(const Document& document)
{
    std::vector<const Entity*> lines;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<Polyline2>(entity.geometry)) {
            lines.push_back(&entity);
        }
    });
    return lines;
}

std::string textOf(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    if (found == entity.properties.end()) {
        return "<absent>";
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr ? *text : "<not text>";
}

const Entity* pointNumbered(const Document& document, const std::string& number)
{
    const Entity* found = nullptr;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry) &&
            textOf(entity, "point") == number) {
            found = &entity;
        }
    });
    return found;
}

Point2 positionOf(const Entity& point)
{
    return std::get<katana::entity::PointGeometry>(point.geometry).position;
}

// Everything of a drawing a survey import may change.
struct Drawing {
    std::vector<Entity> entities;
    std::vector<katana::entity::Layer> layers;
    std::vector<katana::entity::Style> styles;

    friend bool operator==(const Drawing&, const Drawing&) = default;
};

Drawing drawingOf(const Document& document)
{
    Drawing drawing;
    document.model().entities.forEach(
        [&](const Entity& entity) { drawing.entities.push_back(entity); });
    drawing.layers = document.model().layers.all();
    drawing.styles = document.model().styles.all();
    return drawing;
}

// What a failed comparison prints: enough to see what is left over.
void PrintTo(const Drawing& drawing, std::ostream* out)
{
    *out << drawing.entities.size() << " entities; layers:";
    for (const auto& layer : drawing.layers) {
        *out << " [" << layer.name << ']';
    }
    *out << " styles:";
    for (const auto& style : drawing.styles) {
        *out << " [" << style.name << ']';
    }
}

// The text an import with every drawing option at its default has always
// kept with its job (cad::writeSurveyJobOptions): what a job that was neither
// coded nor strung holds.
const char* const kPlainJobOptions = "katana-survey-import-options=1\n"
                                     "layer-per-code=false\n"
                                     "create-layers=true\n"
                                     "code-property=code\n"
                                     "point-number-property=point\n"
                                     "description-property=description\n"
                                     "record-source=true\n";

} // namespace

TEST(SurveyVerbs, AnImportCodesItsPointsAndStringsTheFilesLinesAndOneUndoTakesItAllBack)
{
    Document document;
    installFieldCodes(document);
    const Drawing before = drawingOf(document);

    const std::string reply = run(document, importGnss());

    // entities= is the five POINTS, as it always was; the line has its record.
    EXPECT_EQ(importedValue(reply, "job"), "job-1");
    EXPECT_EQ(importedValue(reply, "entities"), "5");
    EXPECT_EQ(importedValue(reply, "layer"), "fieldwork");
    // The line made no layer and no style of its own: it goes on the layer
    // and wears the style the coding had just made for its points.
    EXPECT_EQ(finishRecords(reply),
              (std::vector<std::string>{
                  "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=1",
                  "linework lines=1 unplaced=2 layers=0 styles=0"}));

    // The drawing: five points and the one line.
    EXPECT_EQ(document.model().entities.size(), 6U);
    for (const char* number : {"R001", "R002", "R003", "R004"}) {
        const Entity* point = pointNumbered(document, number);
        ASSERT_NE(point, nullptr) << number;
        EXPECT_EQ(point->layer, "FIELD KERB") << number;
        EXPECT_EQ(point->style, "FIELD Kerb") << number;
        EXPECT_EQ(textOf(*point, "code"), "KB") << number;
        EXPECT_EQ(textOf(*point, "string"), "1") << number;
    }
    const Entity* uncoded = pointNumbered(document, "CM1");
    ASSERT_NE(uncoded, nullptr);
    EXPECT_EQ(uncoded->layer, "fieldwork");
    EXPECT_EQ(uncoded->style, "");

    const std::vector<const Entity*> lines = linesDrawn(document);
    ASSERT_EQ(lines.size(), 1U);
    const Entity& kerb = *lines.front();
    const Polyline2& shape = std::get<Polyline2>(kerb.geometry);
    EXPECT_TRUE(shape.closed) << "the file closed the string";
    EXPECT_EQ(shape.vertices, (std::vector<Point2>{Point2(500010, 6200020), Point2(500020, 6200020),
                                                   Point2(500020, 6200030)}));
    EXPECT_EQ(kerb.layer, "FIELD KERB");
    EXPECT_EQ(kerb.style, "FIELD Kerb");
    EXPECT_EQ(textOf(kerb, "code"), "KB");
    EXPECT_EQ(textOf(kerb, "string"), "1");
    // A line runs through its points where they stand, in height as in plan.
    const std::vector<std::optional<double>> heights =
        katana::entity::heightsOf(kerb.properties, 3);
    const char* const through[] = {"R001", "R002", "R003"};
    for (std::size_t i = 0; i < 3; ++i) {
        const Entity* point = pointNumbered(document, through[i]);
        ASSERT_NE(point, nullptr) << through[i];
        ASSERT_TRUE(heights[i].has_value()) << through[i];
        EXPECT_EQ(heights[i], katana::entity::heightsOf(point->properties, 1).front())
            << through[i];
    }

    // The style: the rule's linestyle, and its colour as the Document's
    // resolver reads the name - the fixture's own table, #C04000.
    const katana::entity::Style* style = document.model().styles.find("FIELD Kerb");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->linetype, "FIELD Kerb");
    ASSERT_TRUE(style->color.has_value());
    EXPECT_EQ(*style->color, (Color{0xC0, 0x40, 0x00, 255}));

    // The job owns the points and the line, and remembers how it was imported.
    ASSERT_EQ(document.surveyJobs().size(), 1U);
    const auto& job = document.surveyJobs().front();
    EXPECT_EQ(job.placedPoints.size(), 5U);
    EXPECT_EQ(job.createdEntities.size(), 6U);
    EXPECT_EQ(job.importOptions,
              std::string(kPlainJobOptions) + "apply-codes=true\ndraw-linework=true\n");

    // ONE undo: the points, their layers and style, the line and the job.
    EXPECT_EQ(document.history().undoCount(), 1U);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_FALSE(document.undo().ok()) << "it was one step";
}

// What the finish left over is a warning of the job's report, and so of the
// reply: here the second string KB 1, which is one point. CM1, with no code,
// is in no line for being what it is, and is not one.
TEST(SurveyVerbs, WhatTheFinishLeavesOverIsCountedAndListedWithTheReductionsWarnings)
{
    Document plain;
    installFieldCodes(plain);
    const std::string bare = run(plain, importGnss("CODES off LINEWORK off"));
    Document document;
    installFieldCodes(document);
    const std::string reply = run(document, importGnss());

    const auto count = [](const std::string& text) {
        return katana::core::parseInteger(importedValue(text, "reduction_warnings"));
    };
    ASSERT_TRUE(count(bare).has_value()) << bare;
    ASSERT_TRUE(count(reply).has_value()) << reply;
    EXPECT_EQ(*count(reply), *count(bare) + 1);
    // The last of the warnings, straight before the coded record.
    EXPECT_NE(reply.find("\nreduction_warning text=\"1 string(s) of the file are in no line "
                         "(fewer than two of its points have a position: 1).\"\n"
                         "coded points=4 "),
              std::string::npos)
        << reply;
    EXPECT_EQ(bare.find("in no line"), std::string::npos) << bare;
}

TEST(SurveyVerbs, CodesOffStringsTheLinesAndLeavesThePointsAsTheImportDrewThem)
{
    Document document;
    installFieldCodes(document);
    const std::string reply = run(document, importGnss("CODES off"));

    // The layer FIELD KERB and the style "FIELD Kerb" are then the LINE's to
    // make, and its record says so: nothing else would account for them.
    EXPECT_EQ(finishRecords(reply),
              (std::vector<std::string>{"coded none reason=off",
                                        "linework lines=1 unplaced=2 layers=1 styles=1"}));
    EXPECT_EQ(importedValue(reply, "entities"), "5");
    for (const char* number : {"CM1", "R001", "R002", "R003", "R004"}) {
        const Entity* point = pointNumbered(document, number);
        ASSERT_NE(point, nullptr) << number;
        EXPECT_EQ(point->layer, "fieldwork") << number;
        EXPECT_EQ(point->style, "") << number;
    }
    // The line is styled all the same: its rule is how it is known to be one.
    const std::vector<const Entity*> lines = linesDrawn(document);
    ASSERT_EQ(lines.size(), 1U);
    EXPECT_EQ(lines.front()->layer, "FIELD KERB");
    EXPECT_EQ(lines.front()->style, "FIELD Kerb");
    EXPECT_EQ(document.surveyJobs().front().importOptions,
              std::string(kPlainJobOptions) + "draw-linework=true\n");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().entities.size(), 0U);
}

TEST(SurveyVerbs, LineworkOffCodesThePointsAndDrawsNoLine)
{
    Document document;
    installFieldCodes(document);
    const std::string reply = run(document, importGnss("LINEWORK off"));

    EXPECT_EQ(finishRecords(reply),
              (std::vector<std::string>{
                  "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=1",
                  "linework none reason=off"}));
    EXPECT_TRUE(linesDrawn(document).empty());
    EXPECT_EQ(document.model().entities.size(), 5U);
    const Entity* coded = pointNumbered(document, "R002");
    ASSERT_NE(coded, nullptr);
    EXPECT_EQ(coded->layer, "FIELD KERB");
    EXPECT_EQ(coded->style, "FIELD Kerb");
    EXPECT_EQ(document.surveyJobs().front().importOptions,
              std::string(kPlainJobOptions) + "apply-codes=true\n");
}

// Both off is the import of before: the points on LAYER, no layer or style of
// a rule, no string number, and a job stored as one that was not finished.
// The words are read in any case.
TEST(SurveyVerbs, BothOffImportsThePointsAloneThoughSurveyCodesAreLoaded)
{
    Document document;
    installFieldCodes(document);
    const Drawing before = drawingOf(document);
    const std::string reply = run(document, importGnss("codes OFF Linework Off"));

    EXPECT_EQ(finishRecords(reply), (std::vector<std::string>{"coded none reason=off",
                                                              "linework none reason=off"}));
    EXPECT_EQ(document.model().entities.size(), 5U);
    for (const char* number : {"CM1", "R001", "R002", "R003", "R004"}) {
        const Entity* point = pointNumbered(document, number);
        ASSERT_NE(point, nullptr) << number;
        EXPECT_EQ(point->layer, "fieldwork") << number;
        EXPECT_EQ(point->style, "") << number;
        EXPECT_EQ(textOf(*point, "string"), "<absent>") << number;
    }
    EXPECT_FALSE(document.model().layers.contains("FIELD KERB"));
    EXPECT_TRUE(document.model().styles.all() == before.styles);
    EXPECT_EQ(document.surveyJobs().front().importOptions, kPlainJobOptions);
}

// CODES and LINEWORK unsaid, the drawing's customisation decides: its two
// automation switches, which a customisation file or CUSTOMISE SET auto.*
// sets and which are both on until one does. A word on the line overrides.
TEST(SurveyVerbs, WithoutTheWordsTheDocumentsAutomationSwitchesDecide)
{
    Document document;
    installFieldCodes(document);
    document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
    EXPECT_EQ(finishRecords(run(document, importGnss())),
              (std::vector<std::string>{"coded none reason=off",
                                        "linework lines=1 unplaced=2 layers=1 styles=1"}));
    const Entity* uncoded = pointNumbered(document, "R001");
    ASSERT_NE(uncoded, nullptr);
    EXPECT_EQ(uncoded->layer, "fieldwork");

    Document neither;
    installFieldCodes(neither);
    neither.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});
    EXPECT_EQ(finishRecords(run(neither, importGnss())),
              (std::vector<std::string>{"coded none reason=off", "linework none reason=off"}));
    EXPECT_EQ(neither.surveyJobs().front().importOptions, kPlainJobOptions);

    // Said on the line, CODES on codes whatever the switch says.
    Document said;
    installFieldCodes(said);
    said.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});
    EXPECT_EQ(finishRecords(run(said, importGnss("CODES on"))),
              (std::vector<std::string>{
                  "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=1",
                  "linework none reason=off"}));
    const Entity* coded = pointNumbered(said, "R001");
    ASSERT_NE(coded, nullptr);
    EXPECT_EQ(coded->layer, "FIELD KERB");

    // And LINEWORK on strings whatever ITS switch says, the codes' switch
    // staying as it is: the points are left where the import drew them, and
    // the file's closed string is drawn on its rule's layer.
    Document strung;
    installFieldCodes(strung);
    strung.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});
    EXPECT_EQ(finishRecords(run(strung, importGnss("LINEWORK on"))),
              (std::vector<std::string>{"coded none reason=off",
                                        "linework lines=1 unplaced=2 layers=1 styles=1"}));
    const Entity* left = pointNumbered(strung, "R001");
    ASSERT_NE(left, nullptr);
    EXPECT_EQ(left->layer, "fieldwork");
    EXPECT_EQ(left->style, "");
    const std::vector<const Entity*> lines = linesDrawn(strung);
    ASSERT_EQ(lines.size(), 1U);
    EXPECT_EQ(lines.front()->layer, "FIELD KERB");
    EXPECT_TRUE(std::get<Polyline2>(lines.front()->geometry).closed);
    EXPECT_EQ(strung.surveyJobs().front().importOptions,
              std::string(kPlainJobOptions) + "draw-linework=true\n");
}

// The promise every earlier test of this verb rests on: a drawing with no
// survey codes is imported EXACTLY as it was before the verb coded anything -
// the same entities, the same job, the same lines of the reply - whatever
// the line asks, and only the two records are added.
TEST(SurveyVerbs, WithNoSurveyCodesLoadedTheImportIsThePlainOneAndTheReplyOnlyGainsItsTwoRecords)
{
    Document unsaid;
    Document asked;
    Document refused;
    const std::string byDefault = run(unsaid, importGnss());
    const std::string on = run(asked, importGnss("CODES on LINEWORK on"));
    const std::string off = run(refused, importGnss("CODES off LINEWORK off"));

    const std::vector<std::string> noCodes = {"coded none reason=no-survey-codes",
                                              "linework none reason=no-survey-codes"};
    EXPECT_EQ(finishRecords(byDefault), noCodes);
    EXPECT_EQ(finishRecords(on), noCodes);
    EXPECT_EQ(finishRecords(off), (std::vector<std::string>{"coded none reason=off",
                                                            "linework none reason=off"}));
    // Every other line of the three replies is the same line.
    EXPECT_EQ(withoutFinishRecords(byDefault), withoutFinishRecords(off));
    EXPECT_EQ(withoutFinishRecords(on), withoutFinishRecords(off));

    for (Document* document : {&unsaid, &asked}) {
        EXPECT_EQ(drawingOf(*document), drawingOf(refused));
        ASSERT_EQ(document->surveyJobs().size(), 1U);
        const auto& job = document->surveyJobs().front();
        const auto& plain = refused.surveyJobs().front();
        // Not stored as a job to finish: a re-adjustment after codes are
        // loaded would code only the points it drew for the first time.
        EXPECT_EQ(job.importOptions, kPlainJobOptions);
        EXPECT_EQ(job.createdEntities, plain.createdEntities);
        EXPECT_EQ(job.placedPoints.size(), plain.placedPoints.size());
        const Entity* point = pointNumbered(*document, "R001");
        ASSERT_NE(point, nullptr);
        EXPECT_EQ(textOf(*point, "string"), "<absent>");
        EXPECT_EQ(job.reportText.find("survey codes"), std::string::npos);
    }
}

// A step with nothing to go on says which nothing: no rule for the file's
// codes, no code in the file, no point drawn.
TEST(SurveyVerbs, AStepWithNothingToGoOnSaysWhyAndIsNoFailure)
{
    // Rules, and none for KB: the one code the file has is named.
    Document otherCodes;
    auto other = katana::entity::customisationFromJson(
        R"({"format": "katana-customisation", "version": 1, "name": "other codes",
            "codes": [{"key": "ZZ*", "sets": "feature", "layer": "ELSEWHERE", "draw": "line"}]})");
    ASSERT_TRUE(other.ok()) << other.error().describe();
    ASSERT_TRUE(otherCodes
                    .installCustomisation(std::move(*other), katana::cad::CustomisationOrigin::Loaded)
                    .ok());
    EXPECT_EQ(finishRecords(run(otherCodes, importGnss())),
              (std::vector<std::string>{"coded none reason=no-rule-matches",
                                        "unmatched_code text=KB",
                                        "linework none reason=no-rule-matches"}));
    EXPECT_EQ(otherCodes.model().entities.size(), 5U);
    const Entity* point = pointNumbered(otherCodes, "R001");
    ASSERT_NE(point, nullptr);
    EXPECT_EQ(point->layer, "fieldwork");
    EXPECT_FALSE(otherCodes.model().layers.contains("ELSEWHERE"));

    // rounded_seconds_gsi16.gsi carries no code at all (its two shots and two
    // marks: cli.survey_import_gsi_radiates_the_shots_whose_angles_wrote_
    // sixty_seconds).
    Document noCodes;
    installFieldCodes(noCodes);
    const std::string uncoded = run(
        noCodes, "SURVEY IMPORT \"" + fixture("leica/rounded_seconds_gsi16.gsi") + "\"");
    EXPECT_EQ(importedValue(uncoded, "entities"), "4");
    EXPECT_EQ(finishRecords(uncoded),
              (std::vector<std::string>{"coded none reason=no-codes-in-file",
                                        "linework none reason=no-codes-in-file"}));

    // The traverse with no coordinates places nothing without control.
    Document noPoints;
    installFieldCodes(noPoints);
    const std::string nothing = run(noPoints, "SURVEY IMPORT \"" + bareTraverse() + "\"");
    EXPECT_EQ(importedValue(nothing, "entities"), "0");
    EXPECT_EQ(finishRecords(nothing), (std::vector<std::string>{"coded none reason=no-points",
                                                                "linework none reason=no-points"}));
}

// traverse.sdr, whose points are coded CTRL (CP1 and CP2, keyed in) and TRAV
// (T1 and T2, observed) and whose format has no string numbers. The fixture
// makes CTRL a point code and has no rule for TRAV: four points carry a code,
// two are matched onto FIELD CONTROL in the one style a plain red line makes
// ("Plain"), one code is unmatched and named, and nothing is a line - two
// points of a point code and two of a code with no rule.
TEST(SurveyVerbs, CodesWithNoRuleAreCountedAndNamedAndAPointCodeStringsNothing)
{
    Document document;
    installFieldCodes(document);
    const std::string reply =
        run(document, "SURVEY IMPORT \"" + fixture("sdr/traverse.sdr") + "\" LAYER traverse");

    EXPECT_EQ(importedValue(reply, "entities"), "4");
    EXPECT_EQ(finishRecords(reply),
              (std::vector<std::string>{
                  "coded points=4 matched=2 unmatched_codes=1 layers=1 styles=1",
                  "unmatched_code text=TRAV", "linework lines=0 unplaced=4 layers=0 styles=0"}));
    for (const char* number : {"CP1", "CP2"}) {
        const Entity* mark = pointNumbered(document, number);
        ASSERT_NE(mark, nullptr) << number;
        EXPECT_EQ(mark->layer, "FIELD CONTROL") << number;
        EXPECT_EQ(mark->style, "Plain") << number;
    }
    for (const char* number : {"T1", "T2"}) {
        const Entity* station = pointNumbered(document, number);
        ASSERT_NE(station, nullptr) << number;
        EXPECT_EQ(station->layer, "traverse") << number;
        EXPECT_EQ(station->style, "") << number;
    }
    EXPECT_TRUE(linesDrawn(document).empty());
    // Both are for a person to act on, and are the job's warnings.
    EXPECT_NE(reply.find("\nreduction_warning text=\"No rule for the code(s): TRAV.\"\n"),
              std::string::npos)
        << reply;
    EXPECT_NE(reply.find("\nreduction_warning text=\"4 point(s) are in no line (no rule for its "
                         "code and no control code: 2; its code is a point code: 2).\"\n"),
              std::string::npos)
        << reply;
}

// setup.fld: CP1 and CP2 entered with no code, 101 and 102 shot as string 01
// of EB (the third shot of it is the record the reader skips). EB* is a plain
// line in "blue", a name the fixture's table does not have: the standard
// names answer after it, and blue is #0000FF.
TEST(SurveyVerbs, AStandardColourNameIsAnsweredAfterTheCustomisationsOwnTable)
{
    Document document;
    installFieldCodes(document);
    const std::string reply =
        run(document, "SURVEY IMPORT \"" + fixture("fld/setup.fld") + "\" LAYER setup");

    EXPECT_EQ(importedValue(reply, "entities"), "4");
    EXPECT_EQ(finishRecords(reply),
              (std::vector<std::string>{
                  "coded points=2 matched=2 unmatched_codes=0 layers=1 styles=1",
                  "linework lines=1 unplaced=2 layers=0 styles=0"}));
    const katana::entity::Style* style = document.model().styles.find("Plain");
    ASSERT_NE(style, nullptr);
    ASSERT_TRUE(style->color.has_value());
    EXPECT_EQ(*style->color, (Color{0, 0, 255, 255}));

    const Entity* first = pointNumbered(document, "101");
    const Entity* second = pointNumbered(document, "102");
    ASSERT_NE(first, nullptr);
    ASSERT_NE(second, nullptr);
    EXPECT_EQ(first->layer, "FIELD EDGE");
    EXPECT_EQ(first->style, "Plain");
    const std::vector<const Entity*> lines = linesDrawn(document);
    ASSERT_EQ(lines.size(), 1U);
    // Through the two shots where they were drawn, in the order shot.
    EXPECT_EQ(std::get<Polyline2>(lines.front()->geometry).vertices,
              (std::vector<Point2>{positionOf(*first), positionOf(*second)}));
    EXPECT_FALSE(std::get<Polyline2>(lines.front()->geometry).closed);
    EXPECT_EQ(lines.front()->layer, "FIELD EDGE");
    EXPECT_EQ(lines.front()->style, "Plain");
}

// Where the two records stand: after the reduction's warnings and before the
// resections', which end the reply as they did before the records existed
// (cli.survey_import_resection_field_file holds the LIST that follows them).
TEST(SurveyVerbs, TheCodedAndLineworkRecordsFollowTheWarningsAndComeBeforeTheResections)
{
    Document document;
    installFieldCodes(document);
    const std::string reply =
        run(document, "SURVEY IMPORT \"" + fixture("fld/resection.fld") + "\"");

    const std::size_t imported = reply.find("\nimported job=");
    const std::size_t reduction = reply.find("\nreduction method=");
    const std::size_t lastWarning = reply.rfind("\nreduction_warning text=");
    const std::size_t coded = reply.find("\ncoded ");
    const std::size_t linework = reply.find("\nlinework ");
    const std::size_t resection = reply.find("\nresection setup=S1 ");
    for (const std::size_t at : {imported, reduction, lastWarning, coded, linework, resection}) {
        ASSERT_NE(at, std::string::npos) << reply;
    }
    EXPECT_LT(imported, reduction);
    EXPECT_LT(reduction, lastWarning);
    EXPECT_LT(lastWarning, coded);
    EXPECT_LT(coded, linework);
    EXPECT_LT(linework, resection);
    // The reply ends with a resection's record.
    EXPECT_EQ(reply.rfind('\n'), reply.rfind("\nresection")) << reply;
}

// Every refusal of the two words, and the drawing as it was.
TEST(SurveyVerbs, CodesAndLineworkTakeOnOrOffOnceEachAndOnlyOnAnImport)
{
    const std::string bare = "SURVEY IMPORT \"" + fixture("fld/gnss.fld") + "\" ";
    const std::string read = "SURVEY READ \"" + fixture("fld/gnss.fld") + "\" ";
    const struct {
        std::string line;
        std::string said;
    } refusals[] = {
        {bare + "CODES", "CODES needs a value"},
        {bare + "LINEWORK", "LINEWORK needs a value"},
        {bare + "CODES maybe", "CODES takes on or off, and 'maybe' is neither"},
        {bare + "LINEWORK yes", "LINEWORK takes on or off, and 'yes' is neither"},
        {bare + "CODES on CODES off", "CODES is given twice"},
        {bare + "LINEWORK off LINEWORK off", "LINEWORK is given twice"},
        {read + "CODES on", "'CODES' is not an option of SURVEY READ"},
        {read + "LINEWORK off", "'LINEWORK' is not an option of SURVEY READ"},
    };
    for (const auto& refusal : refusals) {
        SCOPED_TRACE(refusal.line);
        Document document;
        installFieldCodes(document);
        const auto reply = runSurveyLine(document, refusal.line);
        EXPECT_FALSE(reply.ok()) << *reply;
        if (reply.ok()) {
            continue;
        }
        EXPECT_EQ(reply.error().code, ErrorCode::InvalidArgument) << reply.error().describe();
        EXPECT_NE(reply.error().message.find(refusal.said), std::string::npos)
            << reply.error().describe();
        // The usage names both words.
        EXPECT_NE(reply.error().message.find("[CODES on|off] [LINEWORK on|off]"),
                  std::string::npos)
            << reply.error().describe();
        EXPECT_TRUE(document.surveyJobs().empty());
        EXPECT_EQ(document.model().entities.size(), 0U);
        EXPECT_EQ(document.history().undoCount(), 0U);
    }

    // SET's items end at either word, as at any other option.
    Document document;
    installFieldCodes(document);
    const std::string reply = run(document, bare + "SET atmospheric=none CODES off");
    EXPECT_NE(reply.find("\nsetting key=atmospheric value=none\n"), std::string::npos) << reply;
    ASSERT_FALSE(finishRecords(reply).empty());
    EXPECT_EQ(finishRecords(reply).front(), "coded none reason=off");

    // SURVEY READ is as it was: it says nothing of codes.
    Document reader;
    installFieldCodes(reader);
    EXPECT_TRUE(finishRecords(run(reader, read)).empty());
}

// layers= and styles= are each record's OWN step's, and are two counts. The
// one rule here makes a kerb a line on a layer and says nothing of how it
// looks - no linestyle, no colour, no symbol - so coding it makes a layer and
// no style (survey_coding.hpp: a code whose rules give no appearance keeps
// its style). With the codes on, the layer is the coding's and the line
// makes nothing; with CODES off the line makes the layer itself.
TEST(SurveyVerbs, EachRecordCountsTheLayersAndTheStylesItsOwnStepMadeAndTheTwoAreNotOneNumber)
{
    const auto installBareKerb = [](Document& document) {
        auto bare = katana::entity::customisationFromJson(
            R"({"format": "katana-customisation", "version": 1, "name": "bare kerb",
                "codes": [{"key": "KB*", "sets": "feature", "layer": "BARE KERB",
                           "draw": "line"}]})");
        ASSERT_TRUE(bare.ok()) << bare.error().describe();
        ASSERT_TRUE(document
                        .installCustomisation(std::move(*bare),
                                              katana::cad::CustomisationOrigin::Loaded)
                        .ok());
    };

    Document coded;
    installBareKerb(coded);
    const Drawing before = drawingOf(coded);
    EXPECT_EQ(finishRecords(run(coded, importGnss())),
              (std::vector<std::string>{
                  "coded points=4 matched=4 unmatched_codes=0 layers=1 styles=0",
                  "linework lines=1 unplaced=2 layers=0 styles=0"}));
    EXPECT_TRUE(coded.model().layers.contains("BARE KERB"));
    EXPECT_TRUE(coded.model().styles.all() == before.styles) << "no style was made";
    const Entity* point = pointNumbered(coded, "R002");
    ASSERT_NE(point, nullptr);
    EXPECT_EQ(point->layer, "BARE KERB");
    EXPECT_EQ(point->style, "");
    ASSERT_EQ(linesDrawn(coded).size(), 1U);
    EXPECT_EQ(linesDrawn(coded).front()->layer, "BARE KERB");
    EXPECT_EQ(linesDrawn(coded).front()->style, "");

    Document uncoded;
    installBareKerb(uncoded);
    EXPECT_EQ(finishRecords(run(uncoded, importGnss("CODES off"))),
              (std::vector<std::string>{"coded none reason=off",
                                        "linework lines=1 unplaced=2 layers=1 styles=0"}));
    EXPECT_TRUE(uncoded.model().layers.contains("BARE KERB"));
    EXPECT_TRUE(uncoded.model().styles.all() == before.styles);
    const Entity* left = pointNumbered(uncoded, "R002");
    ASSERT_NE(left, nullptr);
    EXPECT_EQ(left->layer, "fieldwork");
    ASSERT_EQ(linesDrawn(uncoded).size(), 1U);
    EXPECT_EQ(linesDrawn(uncoded).front()->layer, "BARE KERB");
}

// Ten codes with no rule are named and the rest are counted. The file is
// written here: thirteen entered coordinates of the opcode field file (02,
// then the blank column, the feature code, no string number, the point id, no
// name and no comment, then X, Y and Z - docs/survey.md, "The opcode field
// file (.fld)"), coded ZL back to ZA - twelve codes the fixture has no rule
// for - and then CTRL, its point code. The codes are named in NAME order,
// which is the reverse of the file's: ZA to ZJ, and two more.
TEST(SurveyVerbs, TenCodesWithNoRuleAreNamedInNameOrderAndTheRestAreCounted)
{
    std::string text = "{Version 6.0}\r\n";
    for (int i = 0; i < 13; ++i) {
        const std::string code =
            i < 12 ? std::string("Z") + static_cast<char>('L' - i) : std::string("CTRL");
        text += "02\t\t" + code + "\t\tP" + std::to_string(i + 1) + "\t\t\t" +
                std::to_string(1000 + 10 * i) + ".000\t2000.000\t10.000\r\n";
    }
    const ScratchDirectory scratch("many-codes");
    const std::string file = scratch.file("many_codes.fld", text);
    Document document;
    installFieldCodes(document);
    const std::string reply =
        run(document, "SURVEY IMPORT " + file + " FORMAT opcode-field-file LAYER many");

    EXPECT_EQ(importedValue(reply, "entities"), "13");
    // Thirteen points carry a code and one, CTRL, has a rule: its layer and
    // its plain red style. None is a line: twelve have no rule and CTRL is a
    // point code.
    EXPECT_EQ(finishRecords(reply),
              (std::vector<std::string>{
                  "coded points=13 matched=1 unmatched_codes=12 layers=1 styles=1",
                  "unmatched_code text=ZA", "unmatched_code text=ZB", "unmatched_code text=ZC",
                  "unmatched_code text=ZD", "unmatched_code text=ZE", "unmatched_code text=ZF",
                  "unmatched_code text=ZG", "unmatched_code text=ZH", "unmatched_code text=ZI",
                  "unmatched_code text=ZJ", "unmatched_codes_more=2",
                  "linework lines=0 unplaced=13 layers=0 styles=0"}));
    EXPECT_TRUE(linesDrawn(document).empty());
}

// What HELP LINEWORK says of a job imported with LINEWORK off: the verb
// strings its points BY THEIR CODES - the four shots of KB 1 in point order,
// one open line - and not as the file strung them, which closed R001 to R003
// and began the string again at R004. And having drawn that line it does not
// draw it again: the drawing holds it.
TEST(SurveyVerbs, AJobImportedWithLineworkOffIsStrungByTheVerbByCodeAndOnlyOnce)
{
    Document document;
    installFieldCodes(document);
    run(document, importGnss("LINEWORK off"));
    ASSERT_TRUE(linesDrawn(document).empty());
    ASSERT_EQ(document.model().entities.size(), 5U);

    // The job took the first entity id for its number, so the points are
    // entities 2 to 6 and the line is entity 7. CM1 has no code.
    katana::cad::CommandInterpreter interpreter(document);
    const auto first = interpreter.run("LINEWORK");
    ASSERT_TRUE(first.ok()) << first.error().describe();
    EXPECT_EQ(*first,
              "linework scope=drawing matched=5 considered=5 lines=1 unplaced=1 notes=0\n"
              "string name=KB1 key=KB* number=1 points=4 vertices=4 closed=no "
              "layer=\"FIELD KERB\" entity=7\n"
              "unplaced reason=\"no code\" points=1");
    ASSERT_EQ(linesDrawn(document).size(), 1U);
    const Polyline2& shape = std::get<Polyline2>(linesDrawn(document).front()->geometry);
    EXPECT_FALSE(shape.closed);
    EXPECT_EQ(shape.vertices,
              (std::vector<Point2>{Point2(500010, 6200020), Point2(500020, 6200020),
                                   Point2(500020, 6200030), Point2(500040, 6200040)}));

    const auto second = interpreter.run("LINEWORK");
    ASSERT_TRUE(second.ok()) << second.error().describe();
    EXPECT_EQ(*second,
              "linework scope=drawing matched=6 considered=5 lines=0 unplaced=1 notes=0\n"
              "left_out=4 reason=already-drawn lines=1\n"
              "unplaced reason=\"no code\" points=1");
    EXPECT_EQ(linesDrawn(document).size(), 1U);
    EXPECT_EQ(document.model().entities.size(), 6U);
    EXPECT_EQ(document.history().undoCount(), 2U) << "the import, and the one run that drew";
}
