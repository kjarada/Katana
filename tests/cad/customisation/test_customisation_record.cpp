// What a project records of the customisation it was drawn with, and what
// opening it says is missing (storage::ProjectMetadata::customisation).
// Hand-built libraries: the names are what matter, not what the files draw.

#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <set>

#include "katana/cad/customisation_record.hpp"

using katana::cad::CustomisationSource;
using katana::entity::LineStyle;
using katana::entity::StyleLibrary;

namespace {

LineStyle definition(const char* name, const char* source)
{
    LineStyle style;
    style.name = name;
    style.source = source;
    style.strokes.push_back({katana::entity::StrokeOp::Draw, katana::geometry::Point2(1.0, 0.0)});
    return style;
}

CustomisationSource library(const char* name) { return {name, true}; }
CustomisationSource mapfile(const char* name) { return {name, false}; }

} // namespace

TEST(CustomisationRecord, TheLoadedFilesAreRecordedInLoadOrder)
{
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("WATR Main", "linestyles.4d")).ok());
    ASSERT_TRUE(styles.add(definition("CULT Bollard", "symbols.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(
        loaded, {library("symbols.4d"), library("linestyles.4d"), mapfile("survey.mapfile")},
        false, false);
    // In the order loaded, not the library's name order ("linestyles" < "symbols").
    EXPECT_EQ(katana::cad::customisationRecord(styles, loaded),
              (std::vector<std::string>{"symbols.4d", "linestyles.4d", "survey.mapfile"}));
}

TEST(CustomisationRecord, ALibraryFileEveryDefinitionOfWhichWasReplacedIsNotRecorded)
{
    // extra.4d defined only "WATR Main", and a later mine.4d redefined it:
    // nothing is drawn with extra.4d any more, so a project does not need it.
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("WATR Main", "mine.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(loaded, {library("extra.4d")}, false, false);
    katana::cad::recordCustomisationLoad(loaded, {library("mine.4d")}, false, false);
    EXPECT_EQ(katana::cad::customisationRecord(styles, loaded),
              (std::vector<std::string>{"mine.4d"}));
}

TEST(CustomisationRecord, AReplaceDropsTheEarlierFilesOfTheKindItBroughtAndNoOther)
{
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "first.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(loaded, {library("first.4d"), mapfile("first.mapfile")},
                                         false, false);
    // A replacing load that brought a mapfile only: the library stays.
    katana::cad::recordCustomisationLoad(loaded, {mapfile("second.mapfile")}, false, true);
    EXPECT_EQ(katana::cad::customisationRecord(styles, loaded),
              (std::vector<std::string>{"first.4d", "second.mapfile"}));
}

TEST(CustomisationRecord, AFileLoadedAgainMovesToWhereItWasLoadedLastAndIsListedOnce)
{
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(loaded, {mapfile("a.mapfile"), mapfile("b.mapfile")},
                                         false, false);
    katana::cad::recordCustomisationLoad(loaded, {mapfile("a.mapfile"), mapfile("a.mapfile")},
                                         false, false);
    EXPECT_EQ(loaded, (std::vector<CustomisationSource>{mapfile("b.mapfile"), mapfile("a.mapfile")}));
}

TEST(CustomisationRecord, ADefinitionFromAFileNoLoadAccountsForIsStillRecorded)
{
    // The library was given some other way; what it is drawn with still
    // counts, after the loaded files and in name order.
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("B", "zeta.4d")).ok());
    ASSERT_TRUE(styles.add(definition("C", "alpha.4d")).ok());
    ASSERT_TRUE(styles.add(definition("D", "")).ok());
    EXPECT_EQ(katana::cad::customisationRecord(styles, {mapfile("m.mapfile")}),
              (std::vector<std::string>{"m.mapfile", "alpha.4d", "zeta.4d"}))
        << "a definition made in a session names no file";
}

TEST(CustomisationRecord, OpeningNamesTheRecordedFilesThatAreNotLoadedInTheProjectsOrder)
{
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "linestyles.4d")).ok());
    const std::vector<CustomisationSource> loaded{library("linestyles.4d")};
    const std::vector<std::string> recorded{"survey.mapfile", "linestyles.4d", "symbols.4d",
                                            "survey.mapfile"};
    EXPECT_EQ(katana::cad::customisationNotLoaded(recorded, styles, loaded),
              (std::vector<std::string>{"survey.mapfile", "symbols.4d"}));
    EXPECT_TRUE(katana::cad::customisationNotLoaded({"linestyles.4d"}, styles, loaded).empty());
}

// The reviewer's case: session 1 saved with test_symbols.4d loaded; session 2
// opens it without the file (warned) and saves unedited; session 3 must still
// be warned. By hand: session 2 has only the built-in's two files loaded, the
// open finds "test_symbols.4d" missing, and nothing loads it before the
// save, so the record keeps it after the session's own files.
TEST(CustomisationRecord, ASaveKeepsTheRecordedFilesTheOpenFoundMissingAndNoLoadBroughtSince)
{
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "linestyles.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(
        loaded, {library("linestyles.4d"), mapfile("survey.mapfile")}, false, false);
    const std::vector<std::string> recorded{"linestyles.4d", "test_symbols.4d",
                                            "survey.mapfile"};
    std::vector<std::string> missing = katana::cad::customisationNotLoaded(recorded, styles, loaded);
    ASSERT_EQ(missing, (std::vector<std::string>{"test_symbols.4d"}));
    EXPECT_EQ(katana::cad::customisationRecordToSave(recorded, missing, styles, loaded),
              (std::vector<std::string>{"linestyles.4d", "survey.mapfile", "test_symbols.4d"}));
    // An unrelated load judges nothing it did not bring.
    katana::cad::noteCustomisationLoaded(missing, {mapfile("other.mapfile")});
    EXPECT_EQ(missing, (std::vector<std::string>{"test_symbols.4d"}));
}

// Loaded after the open and then replaced away, the file was judged by this
// session: the drawing is drawn without it here, and the record lets it go.
// By hand: the open misses "old.4d"; loading it takes it off the missing list;
// a library Replace with "new.4d" drops it from the loaded files; the save
// records only what is loaded - "new.4d" (it still defines "A").
TEST(CustomisationRecord, AFileLoadedSinceTheOpenAndThenReplacedIsNoLongerRecorded)
{
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "new.4d")).ok());
    std::vector<CustomisationSource> loaded;
    const std::vector<std::string> recorded{"old.4d"};
    std::vector<std::string> missing = katana::cad::customisationNotLoaded(recorded, styles, loaded);
    ASSERT_EQ(missing, (std::vector<std::string>{"old.4d"}));
    katana::cad::recordCustomisationLoad(loaded, {library("old.4d")}, false, false);
    katana::cad::noteCustomisationLoaded(missing, {library("old.4d")});
    EXPECT_TRUE(missing.empty());
    katana::cad::recordCustomisationLoad(loaded, {library("new.4d")}, true, false);
    EXPECT_EQ(katana::cad::customisationRecordToSave(recorded, missing, styles, loaded),
              (std::vector<std::string>{"new.4d"}));
}

// A missing list left from an earlier drawing keeps nothing the current one
// never recorded: after NEW the record is empty, whatever the list holds.
TEST(CustomisationRecord, AMissingListFromAnEarlierDrawingAddsNothingToANewOne)
{
    StyleLibrary styles;
    const std::vector<CustomisationSource> loaded{mapfile("survey.mapfile")};
    EXPECT_EQ(katana::cad::customisationRecordToSave({}, {"test_symbols.4d"}, styles, loaded),
              (std::vector<std::string>{"survey.mapfile"}));
}

// A typed SAVE has somewhere to go with one project directory, or with none
// once the drawing has a project - split as the interpreter splits it. By
// hand: "SAVE" is 1 word, "SAVE x" 2, "SAVE \"my dir/x\"" 2 (the quotes
// group), "SAVE a b" 3 (the interpreter's usage error), "SAVE \"x" an
// unclosed quote the interpreter refuses.
TEST(CustomisationRecord, ATypedSaveHasADestinationOnlyWhereTheInterpreterWouldSave)
{
    using katana::cad::typedSaveHasDestination;
    EXPECT_FALSE(typedSaveHasDestination("SAVE", false)) << "untitled: nowhere to save";
    EXPECT_TRUE(typedSaveHasDestination("SAVE", true));
    EXPECT_TRUE(typedSaveHasDestination("  save   ", true));
    EXPECT_TRUE(typedSaveHasDestination("SAVE x.katana", false));
    EXPECT_TRUE(typedSaveHasDestination("SAVE \"C:/my dir/x.katana\"", false));
    EXPECT_TRUE(typedSaveHasDestination("SAVE \"\"", false)) << "\"\" is a word to the interpreter";
    EXPECT_FALSE(typedSaveHasDestination("SAVE a b", true)) << "a usage error saves nothing";
    EXPECT_FALSE(typedSaveHasDestination("SAVE my dir/x.katana", false));
    EXPECT_FALSE(typedSaveHasDestination("SAVE \"x.katana", true)) << "an unclosed quote";
}

// A file named twice in one load is read once, however it is spelt. By hand,
// in a fresh directory d holding x.mapfile and y.mapfile: "d/x.mapfile",
// "d/sub/../x.mapfile" and "d/./x.mapfile" are one file; "d/y.mapfile" is
// another; so two files, and two repeats, as named.
TEST(CustomisationRecord, AFileNamedTwiceInOneLoadIsReadOnce)
{
    const std::filesystem::path dir =
        std::filesystem::temp_directory_path() / "katana-cad-tests-distinct-files";
    std::filesystem::remove_all(dir);
    std::filesystem::create_directories(dir / "sub");
    std::ofstream(dir / "x.mapfile") << "x";
    std::ofstream(dir / "y.mapfile") << "y";
    const std::filesystem::path x = dir / "x.mapfile";
    const std::filesystem::path y = dir / "y.mapfile";
    const std::filesystem::path x2 = dir / "sub" / ".." / "x.mapfile";
    const std::filesystem::path x3 = dir / "." / "x.mapfile";
    const katana::cad::DistinctFiles distinct =
        katana::cad::distinctCustomisationFiles({x, y, x2, x3});
    EXPECT_EQ(distinct.files,
              (std::vector<std::filesystem::path>{std::filesystem::absolute(x).lexically_normal(),
                                                  std::filesystem::absolute(y).lexically_normal()}));
    EXPECT_EQ(distinct.repeats, (std::vector<std::filesystem::path>{x2, x3}));
#if defined(_WIN32)
    // Windows names files without regard to case: X.MAPFILE is x.mapfile.
    const std::filesystem::path upper = dir / "X.MAPFILE";
    EXPECT_EQ(katana::cad::distinctCustomisationFiles({x, upper}).files.size(), 1U);
#endif
    // Two names for files that do not exist are not known to be one.
    EXPECT_EQ(katana::cad::distinctCustomisationFiles({dir / "a.4d", dir / "b.4d"}).files.size(), 2U);
    std::filesystem::remove_all(dir);
}

// ---- a file renamed since the project was saved ----------------------------------------------

// The hash a former name is known by: FNV-1a over the bytes, 64-bit, checked
// against the published test vectors of the algorithm.
TEST(CustomisationRecord, AFormerNameIsKnownByItsFnv1a64BitHash)
{
    EXPECT_EQ(katana::cad::sourceNameHash(""), 0xcbf29ce484222325ULL);
    EXPECT_EQ(katana::cad::sourceNameHash("a"), 0xaf63dc4c8601ec8cULL);
    EXPECT_EQ(katana::cad::sourceNameHash("foobar"), 0x85944171f73967e8ULL);
}

// A project saved before its files were renamed records the old names; the
// same files are loaded under the new ones. By hand: "old_codes.4d" and
// "old_lines.4d" are renamed to the loaded survey_codes.mapfile and
// linestyles.4d (which still defines "A"), so only "mine.4d" - which nothing
// renamed and nothing loaded - is missing; and the save records what is
// loaded, under the new names, and the one file still missing.
TEST(CustomisationRecord, ARecordedFormerNameIsAnsweredByTheFileThatNowHasItsPlace)
{
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "linestyles.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(
        loaded, {library("linestyles.4d"), mapfile("survey_codes.mapfile")}, false, false);
    const std::vector<katana::cad::RenamedSource> renamed{
        {katana::cad::sourceNameHash("old_lines.4d"), "linestyles.4d"},
        {katana::cad::sourceNameHash("old_codes.4d"), "survey_codes.mapfile"},
    };
    const std::vector<std::string> recorded{"old_codes.4d", "old_lines.4d", "mine.4d"};
    const std::vector<std::string> missing =
        katana::cad::customisationNotLoaded(recorded, styles, loaded, renamed);
    EXPECT_EQ(missing, (std::vector<std::string>{"mine.4d"}));
    EXPECT_EQ(katana::cad::customisationRecordToSave(recorded, missing, styles, loaded),
              (std::vector<std::string>{"linestyles.4d", "survey_codes.mapfile", "mine.4d"}));
}

// The rename answers for a file only while the file that has its place is
// loaded: without it, the drawing lacks what the old name brought, and the
// warning names the file as the project recorded it.
TEST(CustomisationRecord, AFormerNameWhoseFileIsNotLoadedNowIsMissingAsRecorded)
{
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "mine.4d")).ok());
    std::vector<CustomisationSource> loaded;
    // linestyles.4d was loaded, and mine.4d then redefined all it brought:
    // nothing is drawn with it now.
    katana::cad::recordCustomisationLoad(loaded, {library("linestyles.4d")}, false, false);
    katana::cad::recordCustomisationLoad(loaded, {library("mine.4d")}, false, false);
    const std::vector<katana::cad::RenamedSource> renamed{
        {katana::cad::sourceNameHash("old_lines.4d"), "linestyles.4d"}};
    EXPECT_EQ(katana::cad::customisationNotLoaded({"old_lines.4d"}, styles, loaded, renamed),
              (std::vector<std::string>{"old_lines.4d"}));
    EXPECT_EQ(katana::cad::customisationNotLoaded({"old_lines.4d"}, styles, {}, renamed),
              (std::vector<std::string>{"old_lines.4d"}));
}

// The built-in customisation's renames: one for each of its files, in load
// order, each an earlier name that is not the name it has now. Nothing here
// can spell the earlier names; that each is answered is the test above.
TEST(CustomisationRecord, TheBuiltInCustomisationsFourFilesEachAnswerForTheNameTheyHadBefore)
{
    const auto renames = katana::cad::builtinRenames();
    std::vector<std::string> now;
    std::set<std::uint64_t> former;
    for (const katana::cad::RenamedSource& rename : renames) {
        now.emplace_back(rename.now);
        former.insert(rename.formerName);
        EXPECT_NE(rename.formerName, katana::cad::sourceNameHash(rename.now)) << rename.now;
        EXPECT_NE(rename.formerName, katana::cad::sourceNameHash("")) << rename.now;
    }
    EXPECT_EQ(now, (std::vector<std::string>{"linestyles.4d", "survey_codes.mapfile",
                                             "survey_codes_names.mapfile", "symbols.4d"}));
    EXPECT_EQ(former.size(), renames.size()) << "no two files had one name";

    // And a project that recorded the names the files have now is missing
    // nothing when they are loaded.
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "linestyles.4d")).ok());
    ASSERT_TRUE(styles.add(definition("B", "symbols.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(
        loaded,
        {library("linestyles.4d"), mapfile("survey_codes.mapfile"),
         mapfile("survey_codes_names.mapfile"), library("symbols.4d")},
        false, false);
    EXPECT_TRUE(katana::cad::customisationNotLoaded(now, styles, loaded).empty());
}

// ---- a plain earlier name, and loading what a warning asked for ------------------------------

namespace {

// A rename table shaped like the built-in one: "branded_lines.4d" is an
// earlier name nobody's own file would have, "common.4d" a plain one
// anybody's might.
std::vector<katana::cad::RenamedSource> renamedSet()
{
    return {{katana::cad::sourceNameHash("branded_lines.4d"), "linestyles.4d"},
            {katana::cad::sourceNameHash("common.4d"), "survey_codes_names.mapfile", false}};
}

// The two files the set is loaded as now, the library defining "A".
std::vector<CustomisationSource> theSetLoaded(StyleLibrary& styles)
{
    EXPECT_TRUE(styles.add(definition("A", "linestyles.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(
        loaded, {library("linestyles.4d"), mapfile("survey_codes_names.mapfile")}, false, false);
    return loaded;
}

} // namespace

// A plain earlier name recorded alone, or beside the names the set has now, is
// a person's own file that happens to share it: missing while it is not
// loaded, warned about, and kept by the save rather than dropped for good.
TEST(CustomisationRecord, APlainFormerNameOutsideTheRenamedSetIsSomeoneElsesFileAndStaysMissing)
{
    StyleLibrary styles;
    const std::vector<CustomisationSource> loaded = theSetLoaded(styles);
    for (const std::vector<std::string>& recorded :
         {std::vector<std::string>{"common.4d"},
          std::vector<std::string>{"linestyles.4d", "survey_codes_names.mapfile", "common.4d"}}) {
        const std::vector<std::string> missing =
            katana::cad::customisationNotLoaded(recorded, styles, loaded, renamedSet());
        EXPECT_EQ(missing, (std::vector<std::string>{"common.4d"}));
        EXPECT_EQ(katana::cad::customisationRecordToSave(recorded, missing, styles, loaded,
                                                         renamedSet()),
                  (std::vector<std::string>{"linestyles.4d", "survey_codes_names.mapfile",
                                            "common.4d"}));
    }
}

// Beside a distinctive earlier name of the same table, the plain one is the
// record of the renamed set, and the file that has its place answers it.
TEST(CustomisationRecord, APlainFormerNameBesideADistinctiveOneIsAnsweredByTheRenamedFile)
{
    StyleLibrary styles;
    const std::vector<CustomisationSource> loaded = theSetLoaded(styles);
    const std::vector<std::string> recorded{"branded_lines.4d", "common.4d"};
    const std::vector<std::string> missing =
        katana::cad::customisationNotLoaded(recorded, styles, loaded, renamedSet());
    EXPECT_TRUE(missing.empty());
    EXPECT_EQ(
        katana::cad::customisationRecordToSave(recorded, missing, styles, loaded, renamedSet()),
        (std::vector<std::string>{"linestyles.4d", "survey_codes_names.mapfile"}));
}

// Opened with nothing loaded, a project that recorded the earlier names is
// told they are missing; loading the files that have their places, as the
// warning asks, clears them, and the save records the new names alone - so
// the next open, anywhere, is about those and is cleared the same way.
TEST(CustomisationRecord, LoadingTheFilesThatReplacedTheFormerNamesClearsWhatTheOpenFoundMissing)
{
    const std::vector<std::string> recorded{"branded_lines.4d", "common.4d"};
    std::vector<std::string> missing =
        katana::cad::customisationNotLoaded(recorded, StyleLibrary{}, {}, renamedSet());
    EXPECT_EQ(missing, recorded);

    StyleLibrary styles;
    const std::vector<CustomisationSource> loaded = theSetLoaded(styles);
    katana::cad::noteCustomisationLoaded(missing, loaded, renamedSet());
    EXPECT_TRUE(missing.empty());
    const std::vector<std::string> saved =
        katana::cad::customisationRecordToSave(recorded, missing, styles, loaded, renamedSet());
    EXPECT_EQ(saved, (std::vector<std::string>{"linestyles.4d", "survey_codes_names.mapfile"}));

    std::vector<std::string> nextOpen =
        katana::cad::customisationNotLoaded(saved, StyleLibrary{}, {}, renamedSet());
    EXPECT_EQ(nextOpen, saved);
    katana::cad::noteCustomisationLoaded(nextOpen, loaded, renamedSet());
    EXPECT_TRUE(nextOpen.empty());

    // A person's own file of the plain name is not what the set's file is:
    // loading the set leaves it missing.
    std::vector<std::string> own{"common.4d"};
    katana::cad::noteCustomisationLoaded(own, loaded, renamedSet());
    EXPECT_EQ(own, (std::vector<std::string>{"common.4d"}));
}

// noteCustomisationLoaded sees only the names still missing. When the open
// found only the plain earlier name missing - the distinctive one was answered
// then - the load cannot tell it from someone's own file; the save, which has
// the whole record, can, and does not keep it once its file is loaded.
TEST(CustomisationRecord, TheSaveJudgesAPlainFormerNameAgainstTheWholeRecord)
{
    const std::vector<std::string> recorded{"branded_lines.4d", "common.4d"};
    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("A", "linestyles.4d")).ok());
    std::vector<CustomisationSource> loaded;
    katana::cad::recordCustomisationLoad(loaded, {library("linestyles.4d")}, false, false);
    std::vector<std::string> missing =
        katana::cad::customisationNotLoaded(recorded, styles, loaded, renamedSet());
    EXPECT_EQ(missing, (std::vector<std::string>{"common.4d"}));

    const std::vector<CustomisationSource> load{mapfile("survey_codes_names.mapfile")};
    katana::cad::recordCustomisationLoad(loaded, load, false, false);
    katana::cad::noteCustomisationLoaded(missing, load, renamedSet());
    EXPECT_EQ(
        katana::cad::customisationRecordToSave(recorded, missing, styles, loaded, renamedSet()),
        (std::vector<std::string>{"linestyles.4d", "survey_codes_names.mapfile"}));
}

// Only the second survey code file's earlier name was a plain one; the other
// three carried the publisher's name and are distinctive. Nothing here can
// spell them: the tests above are of what the flag does.
TEST(CustomisationRecord, OnlyTheBuiltInSecondSurveyCodeFilesFormerNameIsTooPlainToStandAlone)
{
    std::vector<std::string> plain;
    for (const katana::cad::RenamedSource& rename : katana::cad::builtinRenames()) {
        if (!rename.distinctive) {
            plain.emplace_back(rename.now);
        }
    }
    EXPECT_EQ(plain, (std::vector<std::string>{"survey_codes_names.mapfile"}));
}

// ---- a definition renamed since the drawing was saved ----------------------------------------

// A drawing saved before a definition was renamed names it the earlier way. A
// lookup that misses asks for the name it has now, and looks that up instead.
// Names compare exactly, as the library's do.
TEST(CustomisationRecord, ADefinitionLookupThatMissesIsRetriedUnderTheNameItHasNow)
{
    const std::vector<katana::cad::RenamedDefinition> renamed{
        {katana::cad::sourceNameHash("Brand SURVEY - FU"), "SURVEY - FU"}};
    EXPECT_EQ(katana::cad::definitionNameNow("Brand SURVEY - FU", renamed), "SURVEY - FU");
    EXPECT_TRUE(katana::cad::definitionNameNow("brand SURVEY - FU", renamed).empty());
    EXPECT_TRUE(katana::cad::definitionNameNow("SURVEY - FU", renamed).empty());
    EXPECT_TRUE(katana::cad::definitionNameNow("", renamed).empty());

    StyleLibrary styles;
    ASSERT_TRUE(styles.add(definition("SURVEY - FU", "linestyles.4d")).ok());
    EXPECT_EQ(styles.find("Brand SURVEY - FU"), nullptr);
    const LineStyle* found =
        styles.find(katana::cad::definitionNameNow("Brand SURVEY - FU", renamed));
    ASSERT_NE(found, nullptr);
    EXPECT_EQ(found->name, "SURVEY - FU");
}

// The built-in table: 29 definitions, each earlier name known once, none the
// hash of a name given now (a lookup of a name as it is now never needs the
// table), and no name given now renamed again.
TEST(CustomisationRecord, TheBuiltInDefinitionRenamesAreTwentyNineDistinctNamesInByteOrder)
{
    const auto renames = katana::cad::builtinDefinitionRenames();
    ASSERT_EQ(renames.size(), 29U);
    std::set<std::uint64_t> former;
    std::set<std::string_view> now;
    for (const katana::cad::RenamedDefinition& rename : renames) {
        former.insert(rename.formerName);
        now.insert(rename.now);
        EXPECT_FALSE(rename.now.empty());
        EXPECT_TRUE(katana::cad::definitionNameNow(rename.now).empty()) << rename.now;
    }
    EXPECT_EQ(former.size(), renames.size());
    EXPECT_EQ(now.size(), renames.size());
    EXPECT_TRUE(std::is_sorted(renames.begin(), renames.end(),
                               [](const auto& a, const auto& b) { return a.now < b.now; }));
}
