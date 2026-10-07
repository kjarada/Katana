// The session katana_cli and katana_mcp share (src/katana_app/session.hpp), as
// katana_cli runs a line: what goes to stdout, where a reply or a report goes,
// and what to stderr, where a refusal goes. The verbs' own text is pinned by
// the cli.* tests; this pins the streams, which a merged ctest log cannot.
// IMPORT replies in records (docs/interop.md), so its checks read fields.

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <string>

#include <gtest/gtest.h>

#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/core/path_text.hpp"
#include "session.hpp"
#include "start_environment.hpp"

namespace {

using katana::app::Session;
using katana::app::tests::kTwoScripts;
using katana::app::tests::ScopedVariable;
using katana::app::tests::StartEnvironment;

// A directory of the test's own, removed afterwards.
struct ScratchDirectory {
    std::filesystem::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(std::filesystem::temp_directory_path() / ("katana-session-" + name))
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

    // Writes `text` to `name` here and returns its path, quoted for a line.
    [[nodiscard]] std::string file(const std::string& name, const std::string& text) const
    {
        const std::filesystem::path where = path / name;
        std::ofstream(where, std::ios::binary) << text;
        return "\"" + where.generic_string() + "\"";
    }
};

// What running `line` printed, stream by stream.
struct Printed {
    bool ok = false;
    std::string out;
    std::string err;
};

Printed run(Session& session, const std::string& line)
{
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Printed printed;
    printed.ok = session.run(line);
    printed.out = testing::internal::GetCapturedStdout();
    printed.err = testing::internal::GetCapturedStderr();
    return printed;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// One of the committed Katana customisation files (tests/data/customisation).
// By their text: test_symbols holds four symbols and no rule, test_survey
// eleven rules and no definition.
std::string customisationFile(const char* name)
{
    return (std::filesystem::path(KATANA_CUSTOMISATION_DATA) / name).generic_string();
}

} // namespace

// ---- what a session starts with ---------------------------------------------------------------

TEST(Session, AProgramsSessionStartsWithTheBuiltInOfTheRunAndSaysWhichOnStdout)
{
    const std::string symbols = customisationFile("test_symbols.customisation.json");
    const StartEnvironment environment(symbols.c_str(), nullptr);
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session("katana_cli");
    const std::string out = testing::internal::GetCapturedStdout();
    const std::string err = testing::internal::GetCapturedStderr();
    // The line names what was installed, and holds no word a script takes
    // for a failure.
    EXPECT_EQ(out, "Customisation: test_symbols, 4 linestyles and symbols and 0 survey code "
                   "rules, built in\n");
    EXPECT_EQ(err, "");
    EXPECT_EQ(session.document().styleLibrary().size(), 4U);
    EXPECT_EQ(session.document().customisationState().origin,
              katana::cad::CustomisationOrigin::BuiltIn);
    EXPECT_EQ(session.document().customisationState().name, "test_symbols");

    // The interpreter was handed the host: RESET has a built-in to go back
    // to, and KEEP is refused for the one thing this run was not given - a
    // kept file - naming the variable that would give it one.
    const Printed reset = run(session, "CUSTOMISE RESET");
    EXPECT_TRUE(reset.ok) << reset.err;
    EXPECT_EQ(reset.out, "reset name=test_symbols definitions=4 codes=0 rules=0 kept=yes\n");
    const Printed keep = run(session, "CUSTOMISE KEEP");
    EXPECT_FALSE(keep.ok);
    EXPECT_EQ(keep.out, "");
    EXPECT_TRUE(contains(keep.err, "KATANA_CUSTOMISATION")) << keep.err;
}

TEST(Session, ASessionOfNoProgramStartsEmptyWhateverTheEnvironmentNames)
{
    // Both variables name files that read: a program's session would start
    // with the kept one. A session given no program reads neither, so about
    // thirty suites that construct one see the same empty Document on every
    // machine.
    const std::string symbols = customisationFile("test_symbols.customisation.json");
    const std::string survey = customisationFile("test_survey.customisation.json");
    const StartEnvironment environment(symbols.c_str(), survey.c_str());
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session(nullptr);
    EXPECT_EQ(testing::internal::GetCapturedStdout(), "");
    EXPECT_EQ(testing::internal::GetCapturedStderr(), "");
    EXPECT_TRUE(session.document().styleLibrary().empty());
    EXPECT_EQ(session.document().surveyMap().size(), 0U);
    EXPECT_EQ(session.document().customisationState().origin,
              katana::cad::CustomisationOrigin::None);

    // And it was handed no host: the words that need one are refused by name.
    for (const char* line : {"CUSTOMISE RESET", "CUSTOMISE KEEP", "CUSTOMISE REVERT"}) {
        const Printed refused = run(session, line);
        EXPECT_FALSE(refused.ok) << line;
        EXPECT_EQ(refused.out, "") << line;
        EXPECT_TRUE(refused.err.starts_with(std::string("error: InvalidState: ") + line + " needs "))
            << refused.err;
    }
    // The rest of the family is there all the same.
    const Printed report = run(session, "CUSTOMISE");
    EXPECT_TRUE(report.ok) << report.err;
    EXPECT_TRUE(report.out.starts_with("No customisation is loaded.\n")) << report.out;
}

TEST(Session, AKeptFileNamedByTheVariableStartsTheSessionInTheBuiltInsPlace)
{
    // A copy, so that nothing a session does can touch the committed file.
    const ScratchDirectory scratch("kept");
    const std::filesystem::path kept = scratch.path / "kept.customisation.json";
    std::filesystem::copy_file(customisationFile("test_survey.customisation.json"), kept);
    const std::string symbols = customisationFile("test_symbols.customisation.json");
    const StartEnvironment environment(symbols.c_str(), kept.generic_string().c_str());
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session("katana_cli");
    const std::string out = testing::internal::GetCapturedStdout();
    const std::string err = testing::internal::GetCapturedStderr();
    EXPECT_EQ(out, "Customisation: test_survey, 0 linestyles and symbols and 11 survey code "
                   "rules, kept\n");
    // The file says nothing of what it was made from, so nothing is said of
    // another built-in.
    EXPECT_EQ(err, "");
    EXPECT_EQ(session.document().customisationState().origin,
              katana::cad::CustomisationOrigin::Kept);
    EXPECT_EQ(session.document().surveyMap().size(), 11U);
    EXPECT_TRUE(session.document().styleLibrary().empty());
}

TEST(Session, AKeptCustomisationMadeFromAnotherBuiltInStartsAndSaysSoOnStderr)
{
    // It names what it was made from, and that is not this run's built-in
    // (test_symbols). It is the user's, so it is what starts; the warning is
    // for them to know the program's own has moved on.
    const ScratchDirectory scratch("kept-from-another");
    const std::string kept = scratch.file(
        "kept.customisation.json",
        R"({"format": "katana-customisation", "version": 1, "name": "Mine",
 "basedOn": {"name": "Some Other", "digest": "0123456789abcdef"},
 "codes": [{"key": "MN*", "sets": "feature", "layer": "MINE"}]})");
    const std::string symbols = customisationFile("test_symbols.customisation.json");
    // `file` gives the path quoted for a line; the variable takes it bare.
    const std::string bare = kept.substr(1, kept.size() - 2);
    const StartEnvironment environment(symbols.c_str(), bare.c_str());
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session("katana_cli");
    const std::string out = testing::internal::GetCapturedStdout();
    const std::string err = testing::internal::GetCapturedStderr();
    // ONE rule, so the noun is in the singular: the line said "1 survey code
    // rules", and this expected it, until 2026-10-07.
    EXPECT_EQ(out, "Customisation: Mine, 0 linestyles and symbols and 1 survey code rule, kept\n");
    EXPECT_EQ(err, "warning: the kept customisation was made from another built-in customisation "
                   "than this program has; CUSTOMISE RESET gives this program's\n");
    EXPECT_EQ(session.document().customisationState().name, "Mine");
}

TEST(Session, TheStartUpLineCountsOneDefinitionAndOneRuleInTheSingular)
{
    // The kept file written here holds ONE symbol and ONE rule, and there is
    // no built-in for the run. The line gave every count the plural - "1
    // linestyles and symbols and 1 survey code rules" - where the window says
    // "1 definition (1 symbol) and 1 survey code rule" of the same start. One
    // definition is a linestyle OR a symbol; the line does not say which.
    const ScratchDirectory scratch("kept-one-of-each");
    const std::string kept = scratch.file(
        "kept.customisation.json",
        R"({"format": "katana-customisation", "version": 1, "name": "One",
 "symbols": [{"name": "ONE Mark", "atVertices": true, "strokes": [["move", 0, 0], ["circle", 0.5]]}],
 "codes": [{"key": "MK*", "sets": "symbol", "symbol": {"name": "ONE Mark"}}]})");
    // `file` gives the path quoted for a line; the variable takes it bare.
    const std::string bare = kept.substr(1, kept.size() - 2);
    const StartEnvironment environment("none", bare.c_str());
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session("katana_cli");
    const std::string out = testing::internal::GetCapturedStdout();
    const std::string err = testing::internal::GetCapturedStderr();
    EXPECT_EQ(out, "Customisation: One, 1 linestyle or symbol and 1 survey code rule, kept\n");
    EXPECT_EQ(err, "");
    EXPECT_EQ(session.document().styleLibrary().size(), 1U);
    EXPECT_EQ(session.document().surveyMap().size(), 1U);
}

TEST(Session, AStartUpProblemIsSaidOnStderrAndTheSessionStartsAllTheSame)
{
    // The seam names a file that is not there: there is no built-in for the
    // run, and that is said - one line, where errors go, naming the variable
    // and the file - rather than the session starting with whatever this
    // build compiled in, or failing to start.
    const ScratchDirectory scratch("no-built-in");
    const std::string absent = (scratch.path / "absent.customisation.json").generic_string();
    const StartEnvironment environment(absent.c_str(), nullptr);
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session("katana_cli");
    const std::string out = testing::internal::GetCapturedStdout();
    const std::string err = testing::internal::GetCapturedStderr();
    // Nothing was installed, so no line says anything was.
    EXPECT_EQ(out, "");
    EXPECT_TRUE(err.starts_with("error: ")) << err;
    EXPECT_TRUE(contains(err, katana::cad::kBuiltInCustomisationVariable)) << err;
    EXPECT_TRUE(contains(err, "absent.customisation.json")) << err;
    EXPECT_EQ(std::count(err.begin(), err.end(), '\n'), 1) << err;
    EXPECT_TRUE(session.document().styleLibrary().empty());
    EXPECT_EQ(session.document().customisationState().origin,
              katana::cad::CustomisationOrigin::None);
    EXPECT_TRUE(run(session, "POINT 1,1").ok);
}

// The two variables name FILES, and a file may sit under a folder named in
// any script. They were read with getenv, which on Windows gives the ANSI
// code page's bytes: a name the code page cannot spell arrived as '?', the
// kept file was "not there" - the ordinary case, so nothing was said and the
// built-in started in its place - and CUSTOMISE KEEP then failed to write it.
// The folder here is named in two scripts no one code page holds.

TEST(Session, AKeptFileUnderAFolderNoCodePageSpellsStartsTheSessionAndIsWhereKeepWrites)
{
    const ScratchDirectory scratch("kept-under-two-scripts");
    const std::filesystem::path folder =
        scratch.path / katana::core::pathFromUtf8("kept-" + kTwoScripts);
    std::filesystem::create_directories(folder);
    const std::filesystem::path kept = folder / "k.customisation.json";
    std::filesystem::copy_file(customisationFile("test_survey.customisation.json"), kept);
    const std::string symbols = customisationFile("test_symbols.customisation.json");
    const ScopedVariable builtInVariable(katana::cad::kBuiltInCustomisationVariable,
                                         symbols.c_str());
    const ScopedVariable keptVariable(katana::cad::kKeptCustomisationVariable, kept);

    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session("katana_cli");
    const std::string out = testing::internal::GetCapturedStdout();
    const std::string err = testing::internal::GetCapturedStderr();
    // The kept file starts the session: the survey fixture's eleven rules and
    // no definition, not the built-in's four symbols.
    EXPECT_EQ(out, "Customisation: test_survey, 0 linestyles and symbols and 11 survey code "
                   "rules, kept\n");
    EXPECT_EQ(err, "");
    EXPECT_EQ(session.document().customisationState().origin,
              katana::cad::CustomisationOrigin::Kept);
    EXPECT_EQ(session.document().surveyMap().size(), 11U);

    // And it is where KEEP writes. An edit first, so that the session is no
    // longer the file: coding on a survey import is switched off.
    const Printed set = run(session, "CUSTOMISE SET auto.codes=off");
    EXPECT_TRUE(set.ok) << set.err;
    const Printed keep = run(session, "CUSTOMISE KEEP");
    EXPECT_TRUE(keep.ok) << keep.err;
    EXPECT_EQ(keep.err, "");
    const auto written = katana::cad::readCustomisationFile(kept);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    ASSERT_TRUE(written->customisation.automation.has_value());
    EXPECT_FALSE(written->customisation.automation->codesOnSurveyImport);
    EXPECT_TRUE(written->customisation.automation->lineworkOnSurveyImport);
    EXPECT_EQ(written->customisation.map.size(), 11U);
    // The file that was there stays beside it.
    std::filesystem::path backup = kept;
    backup += ".bak";
    EXPECT_TRUE(std::filesystem::exists(backup));
}

TEST(Session, ABuiltInTheSeamNamesUnderSuchAFolderStartsTheSession)
{
    const ScratchDirectory scratch("built-in-under-two-scripts");
    const std::filesystem::path folder =
        scratch.path / katana::core::pathFromUtf8("built-in-" + kTwoScripts);
    std::filesystem::create_directories(folder);
    const std::filesystem::path builtIn = folder / "b.customisation.json";
    std::filesystem::copy_file(customisationFile("test_symbols.customisation.json"), builtIn);
    const ScopedVariable builtInVariable(katana::cad::kBuiltInCustomisationVariable, builtIn);
    const ScopedVariable keptVariable(katana::cad::kKeptCustomisationVariable, nullptr);

    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session session("katana_cli");
    const std::string out = testing::internal::GetCapturedStdout();
    const std::string err = testing::internal::GetCapturedStderr();
    // Read through the code page the seam named a file that is not there,
    // which is a problem said on stderr, and no built-in.
    EXPECT_EQ(out, "Customisation: test_symbols, 4 linestyles and symbols and 0 survey code "
                   "rules, built in\n");
    EXPECT_EQ(err, "");
    EXPECT_EQ(session.document().styleLibrary().size(), 4U);
}

// ---- the streams ------------------------------------------------------------------------------

TEST(Session, ARefusalThatCarriesAReportPrintsTheReportWhereAReportGoes)
{
    // UTILITY CHECK refuses a schedule with errors against its schema, so that
    // a script stops there, and the refusal carries the whole check. A script
    // that keeps stdout - katana_cli -c "UTILITY CHECK ..." > check.txt - must
    // have the check exactly when it failed, as before the verb moved into the
    // interpreter; the refusal itself is one line on stderr.
    const ScratchDirectory scratch("check");
    const std::string schema = scratch.file("schema.csv", "kind,attribute,value,detail,label\n"
                                                          "schema,Example Utility Schema,0.1,,\n"
                                                          "identifier,AssetId,,,\n"
                                                          "field,AssetId,Alphanumerical,Yes,Asset Id\n"
                                                          "field,Status,Domain List: Status,Yes,Status\n"
                                                          "domain,Status,Live,,\n"
                                                          "domain,Status,Dead,,\n");
    const std::string wrong =
        scratch.file("wrong.csv", "AssetId,Status,easting\nW-1,Alive,1\nW-2,,1\n");
    Session session(nullptr);

    const Printed failed = run(session, "UTILITY CHECK " + wrong + " SCHEMA " + schema);
    EXPECT_FALSE(failed.ok);
    EXPECT_EQ(failed.err, "error: InvalidArgument: the schedule does not meet the schema: 2 errors\n");
    EXPECT_TRUE(failed.out.starts_with("Delivery schema check against Example Utility Schema v0.1\n"))
        << failed.out;
    EXPECT_TRUE(contains(failed.out, "2 rows, 2 assets (AssetId): 2 errors, 0 warnings"));
    EXPECT_TRUE(contains(failed.out, "\"Alive\" is not in the domain")) << failed.out;
    EXPECT_TRUE(contains(failed.out, "Status is empty"));
    EXPECT_TRUE(failed.out.ends_with("\n"));

    // A schedule that passes is a reply, on stdout, as it always was.
    const std::string clean = scratch.file("clean.csv", "AssetId,Status,easting\nW-1,Live,1\n");
    const Printed passed = run(session, "UTILITY CHECK " + clean + " SCHEMA " + schema);
    EXPECT_TRUE(passed.ok);
    EXPECT_EQ(passed.err, "");
    EXPECT_TRUE(contains(passed.out, "1 rows, 1 assets (AssetId): 0 errors, 0 warnings"));

    // A refusal of one line is that line on stderr and nothing on stdout.
    const Printed refused = run(session, "CIRCLE 0,0 -1");
    EXPECT_FALSE(refused.ok);
    EXPECT_EQ(refused.out, "");
    EXPECT_TRUE(refused.err.starts_with("error: ")) << refused.err;
    EXPECT_EQ(refused.err.find('\n'), refused.err.size() - 1) << refused.err;
}

TEST(Session, InfoWithAnEntityIdDescribesTheEntityInEveryBuild)
{
    // With the GIS module every INFO was once read as INFO <file>, before the
    // interpreter could see it, so INFO 1 - and katana_describe_entity, which
    // sends it - answered that the file did not exist. A 10 x 5 rectangle:
    // perimeter 30, area 50.
    Session session(nullptr);
    ASSERT_TRUE(run(session, "RECT 0,0 10,5").ok);
    const Printed info = run(session, "INFO 1");
    EXPECT_TRUE(info.ok) << info.err;
    EXPECT_EQ(info.out, "1  Polyline  layer=0  vertices=4  closed  length=30  area=50\n");
    EXPECT_EQ(info.err, "");
    EXPECT_EQ(run(session, "info #1").out, info.out);

    const Printed missing = run(session, "INFO 2");
    EXPECT_FALSE(missing.ok);
    EXPECT_EQ(missing.err, "error: NotFound: entity does not exist [2]\n");
}

#if defined(KATANA_TEST_WITH_INTEROP)
TEST(Session, InfoOfAFileNamedLikeAnIdStillReadsTheFile)
{
    // The id is the interpreter's only when no file of that name exists: a
    // file called 12 beside the session is described (and, being no GIS
    // file, refused as one), never looked up as entity 12.
    const ScratchDirectory scratch("info-file");
    (void)scratch.file("12", "not a GIS file\n");
    const std::filesystem::path before = std::filesystem::current_path();
    std::filesystem::current_path(scratch.path);
    Session session(nullptr);
    const Printed info = run(session, "INFO 12");
    std::filesystem::current_path(before);
    EXPECT_FALSE(info.ok);
    EXPECT_FALSE(contains(info.err, "entity does not exist")) << info.err;
    EXPECT_TRUE(info.err.starts_with("error: ")) << info.err;
}

TEST(Session, AQuotedImportEndingInLocalMovesTheDataToTheOrigin)
{
    // katana_import sends IMPORT "path" LOCAL. LOCAL was taken off and the
    // quotes left on, so every such import looked for a file named with its
    // quotes. The folder has a blank in its name, so the quotes are needed.
    // samples/gis/parcels.geojson spans (180, 0) to (365, 165), so moved as
    // one piece to put its lower-left corner at 0,0 it spans (0, 0) to
    // (185, 165).
    const ScratchDirectory scratch("import local");
    const std::filesystem::path copy = scratch.path / "site parcels.geojson";
    std::filesystem::copy_file(std::filesystem::path(KATANA_GIS_SAMPLES) / "parcels.geojson", copy);
    Session session(nullptr);
    const Printed imported = run(session, "IMPORT \"" + copy.generic_string() + "\" LOCAL");
    EXPECT_TRUE(imported.ok) << imported.err;
    EXPECT_TRUE(contains(imported.out, " bounds=0,0,185,165 ")) << imported.out;
    const auto bounds = session.document().model().entities.bounds();
    EXPECT_DOUBLE_EQ(bounds.min.x, 0.0);
    EXPECT_DOUBLE_EQ(bounds.min.y, 0.0);
    EXPECT_DOUBLE_EQ(bounds.max.x, 185.0);
    EXPECT_DOUBLE_EQ(bounds.max.y, 165.0);
}

TEST(Session, AlongsideAndAnOffsetPlaceTheDataAndSayTheMove)
{
    // parcels.geojson spans (180, 0) to (365, 165). ALONGSIDE puts that
    // corner on the drawing's, the rectangle's (1000, 2000); OFFSET=10,-20.5
    // moves it to (190, -20.5).
    Session session(nullptr);
    const std::string parcels =
        "\"" + (std::filesystem::path(KATANA_GIS_SAMPLES) / "parcels.geojson").generic_string() +
        "\"";
    ASSERT_TRUE(run(session, "RECT 1000,2000 1010,2005").ok);
    const Printed along = run(session, "IMPORT " + parcels + " ALONGSIDE");
    EXPECT_TRUE(along.ok) << along.err;
    EXPECT_TRUE(contains(along.out, "placed placement=alongside east=820 north=2000 "
                                    "text=\"ALONGSIDE: moved as one piece by 820.000,2000.000"))
        << along.out;
    EXPECT_TRUE(contains(along.out, " bounds=1000,2000,1185,2165 ")) << along.out;

    Session offset(nullptr);
    const Printed moved = run(offset, "IMPORT " + parcels + " offset=10,-20.5");
    EXPECT_TRUE(moved.ok) << moved.err;
    const auto bounds = offset.document().model().entities.bounds();
    EXPECT_DOUBLE_EQ(bounds.min.x, 190.0);
    EXPECT_DOUBLE_EQ(bounds.min.y, -20.5);
    EXPECT_DOUBLE_EQ(bounds.max.x, 375.0);
    EXPECT_DOUBLE_EQ(bounds.max.y, 144.5);
}

TEST(Session, APlacementIsRefusedByNameForRasterAndABadOffsetForAnyFile)
{
    Session session(nullptr);
    const std::string terrain =
        "\"" + (std::filesystem::path(KATANA_GIS_SAMPLES) / "terrain.asc").generic_string() +
        "\"";
    const Printed raster = run(session, "IMPORT " + terrain + " OFFSET=1,2");
    EXPECT_FALSE(raster.ok);
    EXPECT_TRUE(contains(raster.err, "OFFSET is not supported for rasters and point clouds"))
        << raster.err;
    const Printed bad = run(session, "IMPORT " + terrain + " OFFSET=1");
    EXPECT_FALSE(bad.ok);
    EXPECT_TRUE(contains(bad.err, "OFFSET= takes the east and north")) << bad.err;
    EXPECT_TRUE(session.document().model().entities.empty());
}
#endif
