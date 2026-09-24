// Loading a whole customisation from files (PLAN.MD 20.3, slice 6), and the
// customisation compiled into the build.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <algorithm>
#include <map>
#include <string>

#include "katana/archive12d/customisation.hpp"

#include "reference_files.hpp"

namespace a12 = katana::archive12d;

namespace {

std::filesystem::path writeTemporary(const std::string& name, const std::string& text)
{
    const std::filesystem::path path =
        std::filesystem::temp_directory_path() / ("katana_customisation_" + name);
    std::ofstream file(path, std::ios::binary);
    file << text;
    return path;
}

} // namespace

TEST(Customisation, WhatAFileIsComesFromInsideItAndNotFromItsName)
{
    // The whole reason this exists: `.4d` is BOTH formats in the same folder.
    const auto library = a12::customisationKind("// header\nworldstyle \"S\" { move 0 0 }");
    ASSERT_TRUE(library.ok());
    EXPECT_EQ(*library, a12::CustomisationFile::StyleLibrary);

    const auto map = a12::customisationKind("<xml12d><map_file><map_data/></map_file></xml12d>");
    ASSERT_TRUE(map.ok());
    EXPECT_EQ(*map, a12::CustomisationFile::MapFile);

    EXPECT_FALSE(a12::customisationKind("model \"M\"\nstring super { }").ok())
        << "a 12da archive is neither";
    EXPECT_FALSE(a12::customisationKind("").ok());
}

TEST(Customisation, LibrariesAndMapfilesLoadTogetherWhateverOrderTheyAreGivenIn)
{
    const auto styles = writeTemporary("styles.4d",
                                       "worldstyle \"WATR Main\" { group \"W\" move 0 0 draw 1 0 }");
    const auto rules = writeTemporary(
        "rules.4d",
        "<xml12d><map_file><map_data><item><key>WM*</key><model>SERVICES</model>"
        "<linestyle>WATR Main</linestyle></item></map_data></map_file></xml12d>");

    const auto loaded = a12::readCustomisation({rules, styles});
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    EXPECT_EQ(loaded->library.size(), 1u);
    EXPECT_EQ(loaded->map.size(), 1u);
    ASSERT_EQ(loaded->files.size(), 2u);
    EXPECT_EQ(loaded->files[0].kind, a12::CustomisationFile::MapFile);
    EXPECT_EQ(loaded->files[1].kind, a12::CustomisationFile::StyleLibrary);
    EXPECT_EQ(loaded->files[1].read, 1u);
    EXPECT_TRUE(loaded->warnings.empty());
    EXPECT_TRUE(loaded->unresolvedStyles().empty()) << "the mapfile's linestyle is defined";

    // The mapfile alone leaves its linestyle unresolved, and says so.
    const auto alone = a12::readCustomisation({rules});
    ASSERT_TRUE(alone.ok());
    EXPECT_EQ(alone->unresolvedStyles(), (std::vector<std::string>{"WATR Main"}));

    std::filesystem::remove(styles);
    std::filesystem::remove(rules);
}

TEST(Customisation, AFileThatCannotBeReadFailsTheLoadAndNamesIt)
{
    const auto notEither = writeTemporary("nonsense.4d", "this is not a customisation file at all");
    const auto result = a12::readCustomisation({notEither});
    ASSERT_FALSE(result.ok());
    EXPECT_NE(result.error().describe().find("nonsense.4d"), std::string::npos)
        << result.error().describe();
    std::filesystem::remove(notEither);

    const auto missing = a12::readCustomisation({"no_such_file_anywhere.4d"});
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, katana::core::ErrorCode::NotFound);
}

TEST(Customisation, TheWholeReferenceCustomisationLoadsFromItsFiles)
{
    const std::vector<std::filesystem::path> paths = katana::testing::referencePaths();
    if (paths.size() < 4) {
        GTEST_SKIP() << "the reference customisation is not in this checkout";
    }
    // Every file in the directory, in name order, deliberately mixing the two
    // formats that share the `.4d` extension.
    const auto loaded = a12::readCustomisation(paths);
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();

    std::size_t libraries = 0;
    std::size_t mapfiles = 0;
    for (const a12::LoadedFile& file : loaded->files) {
        (file.kind == a12::CustomisationFile::MapFile ? mapfiles : libraries) += 1;
    }
    EXPECT_EQ(libraries, 2u);
    EXPECT_EQ(mapfiles, 2u) << "one of them shares its extension with the libraries";

    EXPECT_EQ(loaded->library.size(), 792u);
    EXPECT_EQ(loaded->map.size(), 1624u);

    // The chain the whole of PLAN.MD 20.3 exists for, end to end: a survey
    // code, through the mapfile, to a definition in the library.
    const auto water = loaded->map.lookup("WM01");
    ASSERT_FALSE(water.empty());
    EXPECT_EQ(water.resolved.model, "SURVEY SERVICES");
    EXPECT_EQ(water.resolved.linestyle, "WATR Main");
    const katana::entity::LineStyle* main = loaded->library.find(water.resolved.linestyle);
    ASSERT_NE(main, nullptr) << "the water main's linestyle is in the library";
    EXPECT_FALSE(main->strokes.empty());

    // And a code that gets a symbol.
    const auto bollard = loaded->map.lookup("AC01");
    ASSERT_TRUE(bollard.resolved.symbol.has_value());
    const katana::entity::LineStyle* shape = loaded->library.find(bollard.resolved.symbol->style);
    ASSERT_NE(shape, nullptr);
    EXPECT_TRUE(shape->atVertices) << "a symbol is a linestyle drawn at a vertex";

    // A customisation need not be self-contained, and this one is not: five
    // of the 426 names its survey code files reference are defined by neither
    // library - the plain continuous lines "0" and "1", two symbols it expects
    // a standard library to supply, and one linestyle that is simply missing.
    // Reporting them is the point: a name with nothing behind it draws
    // plainly, and a person needs to know which.
    EXPECT_EQ(loaded->unresolvedStyles().size(), 5u);
    const auto missing = loaded->unresolvedStyles();
    EXPECT_NE(std::find(missing.begin(), missing.end(), "0"), missing.end());
    EXPECT_NE(std::find(missing.begin(), missing.end(), "Circle Single"), missing.end());
}

TEST(Customisation, ACustomisationIsFoundBesideTheApplicationWithoutBeingNamed)
{
    // An installed Katana keeps it in share/katana/customisation; a build
    // tree has it in the source. Both are searched, in that order, so that a
    // survey drawing is drawn with its linestyles and symbols without anyone
    // being asked where they are.
    const std::filesystem::path exe = "/opt/katana/bin/katana.exe";
    const auto places = a12::customisationSearchPath(exe);
    ASSERT_GE(places.size(), 2u);
    EXPECT_EQ(places[0], std::filesystem::path("/opt/katana/share/katana/customisation"));

    // Nothing there is not an error: Katana then draws the plain continuous
    // line.
    EXPECT_TRUE(a12::findCustomisation("/no/such/place/katana.exe").empty());
}

TEST(Customisation, WhatIsFoundIsDecidedByLookingInsideEachFileNotByItsName)
{
    const std::filesystem::path directory =
        std::filesystem::temp_directory_path() / "katana_found_customisation" / "bin";
    const std::filesystem::path share =
        directory.parent_path() / "share" / "katana" / "customisation";
    std::filesystem::create_directories(share);
    // Two customisation files and one that is neither, all with extensions
    // that say nothing useful.
    std::ofstream(share / "a.4d") << "worldstyle \"S\" { move 0 0 draw 1 0 }";
    std::ofstream(share / "b.4d")
        << "<xml12d><map_file><map_data><item><key>W*</key><model>M</model></item>"
           "</map_data></map_file></xml12d>";
    std::ofstream(share / "notes.4d") << "just some notes, not a customisation at all";

    const auto found = a12::findCustomisation(directory / "katana.exe");
    ASSERT_EQ(found.size(), 2u) << "the notes are not a customisation file";
    const auto loaded = a12::readCustomisation(found);
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    EXPECT_EQ(loaded->library.size(), 1u);
    EXPECT_EQ(loaded->map.size(), 1u);

    std::filesystem::remove_all(directory.parent_path());
}

// ---- the customisation compiled into the build -------------------------------------------------
//
// These SKIP in a build made without one: it is third-party material under its
// own licence, and the suite must stay green without it.

namespace {

bool builtInCustomisationPresent()
{
    const a12::Customisation& built = a12::builtinCustomisation();
    return !built.files.empty() || !built.errors.empty();
}

} // namespace

// Its files have general names, and are read in this order: the style
// libraries' later file wins a definition both give (the symbol library, after
// the linestyles), and the survey code files' earlier one wins a field both
// give (survey_codes.mapfile, before the names file). Every definition says
// which of the two libraries it came from.
TEST(Customisation, TheBuiltInCustomisationIsFourGenerallyNamedFilesInLoadOrder)
{
    if (!builtInCustomisationPresent()) {
        GTEST_SKIP() << "this build has no customisation compiled in";
    }
    const a12::Customisation& built = a12::builtinCustomisation();
    EXPECT_TRUE(built.errors.empty()) << built.errors.front();
    std::vector<std::string> names;
    std::vector<a12::CustomisationFile> kinds;
    for (const a12::LoadedFile& file : built.files) {
        names.push_back(file.path.filename().string());
        kinds.push_back(file.kind);
    }
    EXPECT_EQ(names, (std::vector<std::string>{"linestyles.4d", "survey_codes.mapfile",
                                               "survey_codes_names.mapfile", "symbols.4d"}));
    EXPECT_EQ(kinds, (std::vector<a12::CustomisationFile>{
                         a12::CustomisationFile::StyleLibrary, a12::CustomisationFile::MapFile,
                         a12::CustomisationFile::MapFile, a12::CustomisationFile::StyleLibrary}));
    std::size_t elsewhere = 0;
    built.library.forEach([&](const katana::entity::LineStyle& style) {
        elsewhere += style.source == "linestyles.4d" || style.source == "symbols.4d" ? 0 : 1;
    });
    EXPECT_EQ(elsewhere, 0u) << "every definition comes from one of the two libraries";
}

// A group path says what a definition is (Survey/DRAIN, Design/LNMK). A word
// put in front of most of them says only whose the customisation was, and is
// not wanted: no first word - up to the first blank - may begin the group path
// of more than half the built-in definitions. Nothing here names such a word;
// the test finds it if there is one.
TEST(Customisation, NoWordBeginsTheGroupPathOfMostBuiltInDefinitions)
{
    if (!builtInCustomisationPresent()) {
        GTEST_SKIP() << "this build has no customisation compiled in";
    }
    const a12::Customisation& built = a12::builtinCustomisation();
    std::map<std::string, std::size_t> firstWords;
    std::size_t grouped = 0;
    built.library.forEach([&](const katana::entity::LineStyle& style) {
        if (style.group.empty()) {
            return;
        }
        ++grouped;
        ++firstWords[style.group.substr(0, style.group.find(' '))];
    });
    ASSERT_GT(grouped, 0u) << "the built-in definitions are grouped";
    for (const auto& [word, count] : firstWords) {
        EXPECT_LE(count * 2, grouped)
            << "\"" << word << "\" begins " << count << " of " << grouped << " group paths";
    }
}
