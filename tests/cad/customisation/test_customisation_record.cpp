// What a project records of the 12d customisation it was drawn with, and what
// opening it says is missing (storage::ProjectMetadata::customisation).
// Hand-built libraries: the names are what matter, not what the files draw.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>

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
