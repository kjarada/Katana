// What a project records of the 12d customisation it was drawn with, and what
// opening it says is missing (storage::ProjectMetadata::customisation).
// Hand-built libraries: the names are what matter, not what the files draw.

#include <gtest/gtest.h>

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
