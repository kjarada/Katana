// Where a definition came from (LineStyle::source), and a built-in
// customisation that reports the files it could not read instead of losing
// them (audit A12-06).

#include <gtest/gtest.h>

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <fstream>
#include <set>
#include <string>

#include "katana/archive12d/customisation.hpp"
#include "katana/archive12d/style_library.hpp"

#include "../reference_files.hpp"

namespace a12 = katana::archive12d;
using katana::entity::LineStyle;

namespace {

constexpr const char* kTwoDefinitions = R"(worldstyle "A" { move 0 0 draw 1 0 }
paperstyle "B" { mode vertex move 0 0 circle 1 })";

a12::StyleLibraryRead read(const std::string& text, const std::string& source)
{
    auto result = a12::readStyleLibrary(text, source);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : a12::StyleLibraryRead{};
}

std::string sourceOf(const katana::entity::StyleLibrary& library, const std::string& name)
{
    const LineStyle* style = library.find(name);
    return style == nullptr ? "(missing)" : style->source;
}

} // namespace

TEST(Provenance, EveryDefinitionReadIsStampedWithTheNameOfTheFileItCameFrom)
{
    const auto library = read(kTwoDefinitions, "user_symbols_test.4d");
    ASSERT_EQ(library.library.size(), 2u);
    EXPECT_EQ(sourceOf(library.library, "A"), "user_symbols_test.4d");
    EXPECT_EQ(sourceOf(library.library, "B"), "user_symbols_test.4d");
}

TEST(Provenance, APathGivenAsTheSourceIsCutDownToTheFileName)
{
    // Both separators, whichever platform wrote the path: a library must
    // never carry where on someone's disk it was loaded from.
    EXPECT_EQ(sourceOf(read(kTwoDefinitions, "C:\\Users\\someone\\custom\\mine.4d").library, "A"),
              "mine.4d");
    EXPECT_EQ(sourceOf(read(kTwoDefinitions, "/home/someone/custom/a b.4d").library, "A"), "a b.4d");
    EXPECT_EQ(a12::sourceFileName("C:/mixed\\sep/last.4d"), "last.4d");
    EXPECT_EQ(a12::sourceFileName("plain.4d"), "plain.4d");
}

TEST(Provenance, TextThatCameFromNoFileLeavesTheSourceEmpty)
{
    auto library = a12::readStyleLibrary(kTwoDefinitions);
    ASSERT_TRUE(library.ok());
    EXPECT_EQ(sourceOf(library->library, "A"), "");
}

TEST(Provenance, ReadingIntoALibraryStampsOnlyTheDefinitionsOfTheNewText)
{
    auto first = a12::readStyleLibrary(R"(worldstyle "A" { move 0 0 }
worldstyle "C" { move 0 0 })",
                                       "first.4d");
    ASSERT_TRUE(first.ok());
    // The second file redefines A and adds B; C is only in the first.
    auto second = a12::readStyleLibraryInto(std::move(first->library), R"(worldstyle "A" { move 1 1 }
worldstyle "B" { move 0 0 })",
                                            "second.4d");
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(sourceOf(second->library, "A"), "second.4d") << "the later definition won, and says so";
    EXPECT_EQ(sourceOf(second->library, "B"), "second.4d");
    EXPECT_EQ(sourceOf(second->library, "C"), "first.4d") << "an earlier file's stamp is kept";
}

TEST(Provenance, ALoadedFileStampsItsNameAndNotItsPath)
{
    const std::filesystem::path directory =
        std::filesystem::temp_directory_path() / "katana_provenance_test";
    std::filesystem::create_directories(directory);
    const std::filesystem::path path = directory / "my_symbols.4d";
    {
        std::ofstream file(path, std::ios::binary);
        file << kTwoDefinitions;
    }
    const auto loaded = a12::readCustomisation({path});
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    EXPECT_EQ(sourceOf(loaded->library, "A"), "my_symbols.4d");
    ASSERT_EQ(loaded->files.size(), 1u);
    EXPECT_EQ(loaded->files[0].path, path) << "the report keeps the path the caller gave";
}

// ---- A12-06: one unreadable file costs only itself ----------------------------------------------

TEST(Provenance, AFileThatCannotBeReadCostsOnlyItselfAndIsNamed)
{
    const std::string library = "worldstyle \"Kept\" { move 0 0 draw 1 0 }";
    // An unterminated quote fails the whole read of a library (see
    // StyleLibrary.AQuoteThatIsNeverClosedFailsTheWholeRead).
    const std::string broken = "worldstyle \"Lost { move 0 0 }";
    const std::string mapfile =
        "<xml12d><map_file><map_data><item><key>WM*</key><model>M</model></item></map_data>"
        "</map_file></xml12d>";
    const std::string neither = "just some notes";

    const a12::Customisation built = a12::readEachCustomisationFile({
        {"a_linestyles.4d", library},
        {"b_broken.4d", broken},
        {"c_rules.mapfile", mapfile},
        {"d_notes.txt", neither},
    });

    // The bug this guards against: reading into the customisation being built
    // left its library MOVED-FROM when b_broken.4d failed, so the definition
    // a_linestyles.4d had brought was gone, and nothing said so.
    EXPECT_EQ(built.library.size(), 1u);
    EXPECT_EQ(sourceOf(built.library, "Kept"), "a_linestyles.4d");
    EXPECT_EQ(built.map.size(), 1u) << "a file after the broken one is still read";
    ASSERT_EQ(built.errors.size(), 2u);
    EXPECT_NE(built.errors[0].find("b_broken.4d"), std::string::npos) << built.errors[0];
    EXPECT_NE(built.errors[1].find("d_notes.txt"), std::string::npos) << built.errors[1];
    ASSERT_EQ(built.files.size(), 2u) << "only the files that were read are listed as loaded";
    EXPECT_EQ(built.files[0].path, std::filesystem::path("a_linestyles.4d"));
    EXPECT_EQ(built.files[1].path, std::filesystem::path("c_rules.mapfile"));
}

TEST(Provenance, TheBuiltInCustomisationStampsEachDefinitionWithTheFileItWasEmbeddedFrom)
{
    const a12::Customisation& built = a12::builtinCustomisation();
    if (built.library.empty()) {
        GTEST_SKIP() << "this build has no customisation compiled in";
    }
    EXPECT_TRUE(built.errors.empty()) << built.errors.front();
    std::set<std::string> libraryFiles;
    for (const a12::LoadedFile& file : built.files) {
        if (file.kind == a12::CustomisationFile::StyleLibrary) {
            libraryFiles.insert(file.path.string());
        }
    }
    std::size_t unstamped = 0;
    built.library.forEach([&](const LineStyle& style) {
        if (!libraryFiles.contains(style.source) ||
            style.source.find_first_of("/\\") != std::string::npos) {
            ++unstamped;
        }
    });
    EXPECT_EQ(unstamped, 0u) << "every built-in definition names one of the embedded library files";
}

TEST(Provenance, TheReferenceSymbolFileIsWhereItsSymbolsSayTheyCameFrom)
{
    // The point of the stamp: most symbols the reference mapfiles use are not
    // `mode vertex`, and the file they come from is what says they are
    // symbols. Counted by a script that uses none of Katana's code: the
    // symbol file holds 474 blocks under 474 names; the linestyle file holds
    // 322 blocks under 321 names (one is defined twice), three of which the
    // symbol file - read after it, in name order - defines again and so
    // takes. 474 from the symbol file, 321 - 3 = 318 from the other.
    const auto paths = katana::testing::referencePaths();
    if (paths.size() < 4) {
        GTEST_SKIP() << "the reference customisation is not in this checkout";
    }
    const auto loaded = a12::readCustomisation(paths);
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    std::size_t fromSymbolFile = 0;
    loaded->library.forEach([&](const LineStyle& style) {
        std::string lower = style.source;
        std::transform(lower.begin(), lower.end(), lower.begin(),
                       [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
        fromSymbolFile += lower.find("symbol") != std::string::npos ? 1 : 0;
    });
    EXPECT_EQ(fromSymbolFile, 474u);
    EXPECT_EQ(loaded->library.size() - fromSymbolFile, 318u);
}
