// Where a definition read from a legacy file came from (LineStyle::source).

#include <gtest/gtest.h>

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <fstream>
#include <string>

#include "customisation.hpp"
#include "style_library.hpp"

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

// LineStyle::symbol: "its customisation lists it as a symbol". A style library
// has no such list - it holds one kind a file, and only the file's NAME says
// which - so its reader sets the flag by that name: "symbol" anywhere in it,
// in any case. Of the two definitions read here B is `mode vertex` and A is
// not, and the flag says the same of both, because that is not what it means.
TEST(Provenance, ADefinitionReadFromAFileNamedAsSymbolsIsListedAsASymbol)
{
    for (const char* source : {"user_symbols_test.4d", "SYMBOLS.4D", "My Symbol library.4d",
                               "C:\\lines\\Symbols.4d"}) {
        const auto library = read(kTwoDefinitions, source);
        ASSERT_EQ(library.library.size(), 2u) << source;
        EXPECT_TRUE(library.library.find("A")->symbol) << source;
        EXPECT_TRUE(library.library.find("B")->symbol) << source;
    }
    // A folder called symbols is not the file's name; nor is "sym" the word.
    for (const char* source : {"linestyles.4d", "sym.4d", "C:\\symbols\\lines.4d"}) {
        const auto library = read(kTwoDefinitions, source);
        ASSERT_EQ(library.library.size(), 2u) << source;
        EXPECT_FALSE(library.library.find("A")->symbol) << source;
        EXPECT_FALSE(library.library.find("B")->symbol) << source;
    }
    // Text that came from no file is of unknown kind, and is not listed.
    auto unnamed = a12::readStyleLibrary(kTwoDefinitions);
    ASSERT_TRUE(unnamed.ok());
    EXPECT_FALSE(unnamed->library.find("A")->symbol);
}

TEST(Provenance, ReadingIntoALibraryListsAsSymbolsOnlyWhatTheSymbolFileDefines)
{
    auto first = a12::readStyleLibrary(R"(worldstyle "A" { move 0 0 }
worldstyle "C" { move 0 0 })",
                                       "linestyles.4d");
    ASSERT_TRUE(first.ok());
    // The symbol file redefines A and adds B; C is only in the first.
    auto second = a12::readStyleLibraryInto(std::move(first->library), R"(worldstyle "A" { move 1 1 }
worldstyle "B" { move 0 0 })",
                                            "symbols.4d");
    ASSERT_TRUE(second.ok());
    EXPECT_TRUE(second->library.find("A")->symbol) << "the later definition won, flag and all";
    EXPECT_TRUE(second->library.find("B")->symbol);
    EXPECT_FALSE(second->library.find("C")->symbol) << "an earlier file's definition is as it was";
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
    std::size_t listedAsSymbols = 0;
    loaded->library.forEach([&](const LineStyle& style) {
        std::string lower = style.source;
        std::transform(lower.begin(), lower.end(), lower.begin(),
                       [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
        fromSymbolFile += lower.find("symbol") != std::string::npos ? 1 : 0;
        listedAsSymbols += style.symbol ? 1 : 0;
    });
    EXPECT_EQ(fromSymbolFile, 474u);
    EXPECT_EQ(loaded->library.size() - fromSymbolFile, 318u);
    // The reader says of each definition what its file's name says: the same
    // 474, now as the definition's own flag.
    EXPECT_EQ(listedAsSymbols, 474u);
}
