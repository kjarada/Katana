// The named surfaces a session keeps (include/katana/terrain/surface_store.hpp).

#include <memory>
#include <string>
#include <utility>

#include <gtest/gtest.h>

#include "katana/core/text_encoding.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::terrain::NamedSurface;
using katana::terrain::SurfaceStore;
using katana::terrain::TinSurface;

std::shared_ptr<const TinSurface> triangle()
{
    auto surface = TinSurface::create({{0, 0, 10}, {10, 0, 11}, {0, 10, 12}}, {{0, 1, 2}});
    EXPECT_TRUE(surface.ok());
    return std::make_shared<const TinSurface>(std::move(surface).value());
}

TEST(SurfaceStore, ASurfaceIsFoundByItsNameInAnyCase)
{
    SurfaceStore store;
    ASSERT_TRUE(store.add({"Ground", triangle(), "drawing"}).ok());
    ASSERT_NE(store.find("ground"), nullptr);
    EXPECT_EQ(store.find("GROUND")->name, "Ground");
    EXPECT_EQ(store.find("GROUND")->source, "drawing");
    EXPECT_EQ(store.find("design"), nullptr);
}

TEST(SurfaceStore, ANameInUseIsRefused)
{
    SurfaceStore store;
    ASSERT_TRUE(store.add({"ground", triangle(), {}}).ok());
    const auto again = store.add({"GROUND", triangle(), {}});
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, katana::core::ErrorCode::AlreadyExists);
    EXPECT_EQ(store.all().size(), 1u);
}

TEST(SurfaceStore, AnEmptyNameOrNoSurfaceIsRefused)
{
    SurfaceStore store;
    EXPECT_EQ(store.add({"", triangle(), {}}).error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(store.add({"ground", nullptr, {}}).error().code,
              katana::core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(store.empty());
}

TEST(SurfaceStore, EveryChangeMovesTheRevision)
{
    SurfaceStore store;
    const auto start = store.revision();
    ASSERT_TRUE(store.add({"a", triangle(), {}}).ok());
    const auto added = store.revision();
    EXPECT_GT(added, start);
    EXPECT_FALSE(store.remove("b"));
    EXPECT_EQ(store.revision(), added); // nothing removed, nothing changed
    EXPECT_TRUE(store.remove("A"));
    EXPECT_GT(store.revision(), added);
    const auto removed = store.revision();
    store.clear();
    EXPECT_GT(store.revision(), removed);
}

TEST(SurfaceStore, AUniqueNameCountsFromTwo)
{
    SurfaceStore store;
    EXPECT_EQ(store.uniqueName("ground"), "ground");
    ASSERT_TRUE(store.add({"ground", triangle(), {}}).ok());
    EXPECT_EQ(store.uniqueName("ground"), "ground (2)");
    ASSERT_TRUE(store.add({"ground (2)", triangle(), {}}).ok());
    EXPECT_EQ(store.uniqueName("Ground"), "Ground (3)");
    EXPECT_EQ(store.uniqueName(""), "surface");
}

// ---- the rule for the text a surface carries --------------------------------------------
//
// A surface's name, source and style reach project storage and the JSON replies, whose
// writer throws on a byte that is not UTF-8, and a name a command line cannot carry (a double
// quote, a line break) is a surface nobody can address again. So the store refuses both where
// they enter, and makes an importer's name nameable.

NamedSurface named(std::string name, std::string source = "test", std::string style = {})
{
    NamedSurface surface;
    surface.name = std::move(name);
    surface.surface = triangle();
    surface.source = std::move(source);
    surface.style = std::move(style);
    return surface;
}

// "cafe" typed in the ANSI code page: Windows-1252 puts e-acute at 0xE9, which is no UTF-8
// (the lead byte of a three-byte sequence with nothing after it).
const std::string kCafeInAnsi = "caf\xE9";

TEST(SurfaceStore, ANameThatIsNotUtf8IsRefusedAndTheMessageDoesNotCarryItsBytes)
{
    SurfaceStore store;
    const auto refused = store.add(named(kCafeInAnsi));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidArgument);
    // The point of the check is the bytes: a message holding them would reach the same JSON
    // writers the name was refused for.
    EXPECT_TRUE(katana::core::isValidUtf8(refused.error().describe()))
        << refused.error().describe();
    EXPECT_NE(refused.error().message.find("UTF-8"), std::string::npos) << refused.error().message;
    EXPECT_TRUE(store.empty());
    EXPECT_EQ(store.revision(), 0u) << "a refused add is no change";
}

TEST(SurfaceStore, AValidUtf8NameWithLettersBeyondAsciiIsKept)
{
    SurfaceStore store;
    // "Grüße 测试" as UTF-8: U+00FC, U+00DF, U+6D4B, U+8BD5.
    const std::string name = "Gr\xC3\xBC\xC3\x9F"
                             "e \xE6\xB5\x8B\xE8\xAF\x95";
    ASSERT_TRUE(store.add(named(name)).ok());
    ASSERT_NE(store.find(name), nullptr);
    EXPECT_EQ(store.find(name)->name, name);
}

TEST(SurfaceStore, ASourceOrAStyleThatIsNotUtf8IsRefused)
{
    SurfaceStore store;
    const auto source = store.add(named("a", "raster " + kCafeInAnsi));
    ASSERT_FALSE(source.ok());
    EXPECT_EQ(source.error().code, katana::core::ErrorCode::InvalidArgument);
    const auto style = store.add(named("a", "test", kCafeInAnsi));
    ASSERT_FALSE(style.ok());
    EXPECT_EQ(style.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(store.empty());

    ASSERT_TRUE(store.add(named("a")).ok());
    const auto restyle = store.setStyle("a", kCafeInAnsi);
    ASSERT_FALSE(restyle.ok());
    EXPECT_EQ(restyle.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(store.find("a")->style.empty()) << "a refused style leaves the surface as it was";
}

TEST(SurfaceStore, ARenameToANameThatIsNotUtf8IsRefusedAndKeepsTheOldName)
{
    SurfaceStore store;
    ASSERT_TRUE(store.add(named("ground")).ok());
    const auto before = store.revision();
    const auto refused = store.rename("ground", "terrain" + std::string("\xE9"));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidArgument);
    ASSERT_NE(store.find("ground"), nullptr);
    EXPECT_EQ(store.all().front().name, "ground");
    EXPECT_EQ(store.revision(), before);
}

TEST(SurfaceStore, ANameACommandLineCannotCarryIsRefusedByAddAndRename)
{
    // The tokenizer has no escape inside quotes, so a double quote cannot be typed in a name and
    // a line break would end the line: such a surface could be listed and never named again.
    SurfaceStore store;
    EXPECT_FALSE(store.add(named("PAD \"A\"")).ok());
    EXPECT_FALSE(store.add(named("two\nlines")).ok());
    EXPECT_FALSE(store.add(named("carriage\rreturn")).ok());
    EXPECT_FALSE(store.add(named("")).ok());
    EXPECT_TRUE(store.empty());

    ASSERT_TRUE(store.add(named("PAD A")).ok());
    EXPECT_FALSE(store.rename("PAD A", "PAD \"A\"").ok());
    EXPECT_FALSE(store.rename("PAD A", "").ok());
    EXPECT_NE(store.find("PAD A"), nullptr);

    // A blank, an apostrophe and a backslash can all be typed (in quotes).
    ASSERT_TRUE(store.rename("PAD A", "Tom's pad C:\\x").ok());
    EXPECT_NE(store.find("Tom's pad C:\\x"), nullptr);
}

TEST(SurfaceStore, CheckNameIsTheOneRuleAndNamesWhatIsWrong)
{
    EXPECT_TRUE(SurfaceStore::checkName("ground").ok());
    EXPECT_TRUE(SurfaceStore::checkName(" a b ").ok());
    const auto empty = SurfaceStore::checkName("");
    ASSERT_FALSE(empty.ok());
    EXPECT_NE(empty.error().message.find("needs a name"), std::string::npos);
    const auto quote = SurfaceStore::checkName("x\"y");
    ASSERT_FALSE(quote.ok());
    EXPECT_NE(quote.error().message.find("double quote"), std::string::npos);
    const auto bytes = SurfaceStore::checkName(kCafeInAnsi);
    ASSERT_FALSE(bytes.ok());
    EXPECT_NE(bytes.error().message.find("UTF-8"), std::string::npos);
}

TEST(SurfaceStore, ANameFromAFileIsMadeNameableAndUniqueByUniqueName)
{
    SurfaceStore store;
    // A quote becomes an apostrophe and a line break a blank; text needing neither is unchanged.
    EXPECT_EQ(SurfaceStore::nameable("PAD \"A\""), "PAD 'A'");
    EXPECT_EQ(SurfaceStore::nameable("two\r\nlines"), "two  lines");
    EXPECT_EQ(SurfaceStore::nameable("ground"), "ground");
    // Bytes that are not UTF-8 are read as Windows-1252: 0xE9 is U+00E9, UTF-8 C3 A9.
    EXPECT_EQ(SurfaceStore::nameable(kCafeInAnsi), "caf\xC3\xA9");

    const std::string first = store.uniqueName("PAD \"A\"");
    EXPECT_EQ(first, "PAD 'A'");
    ASSERT_TRUE(store.add(named(first)).ok());
    const std::string second = store.uniqueName("PAD \"A\"");
    EXPECT_EQ(second, "PAD 'A' (2)");
    ASSERT_TRUE(store.add(named(second)).ok());
    // Every name uniqueName returns is one add accepts: an import never fails for a name a file
    // chose.
    EXPECT_TRUE(SurfaceStore::checkName(store.uniqueName(kCafeInAnsi)).ok());
    EXPECT_EQ(store.uniqueName(""), "surface");
}

} // namespace
