// SURVEY IMPORT's reduction settings (src/katana_app/survey_verbs.hpp):
// SETTINGS, a file of the settings' text form, and SET, lines of it on the
// line - what they start from, what each changes, what the reply says of
// them and every refusal, with the drawing unchanged by one. The cli.*
// tests in src/katana_app/CMakeLists.txt run the same lines through
// katana_cli; these read the document the line leaves behind.

#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/survey/reduction_settings.hpp"
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
        ASSERT_FALSE(reply.ok()) << *reply;
        EXPECT_EQ(reply.error().code, refusal.code) << reply.error().describe();
        EXPECT_NE(reply.error().describe().find(refusal.said), std::string::npos)
            << reply.error().describe();
        EXPECT_TRUE(document.surveyJobs().empty());
        EXPECT_EQ(katana::cad::drawingSurveyPoints(document).size(), 2U);
        // Nothing to undo but the two placings.
        EXPECT_EQ(document.history().undoCount(), 2U);
    }
}
