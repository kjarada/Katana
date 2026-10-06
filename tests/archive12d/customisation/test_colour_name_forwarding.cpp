// The standard colour names are the entity layer's
// (include/katana/entity/colour_names.hpp); this module keeps the three names
// its import, its export and the front ends have always called them by. What
// those names promise is held here.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/archive12d/domain.hpp"
#include "katana/entity/colour_names.hpp"

namespace a12 = katana::archive12d;

namespace katana::archive12d::colour_name_forwarding_test {

// Called as this module's own sources call it (domain_export.cpp): by its
// plain name, with an entity::Color. That argument makes the call find
// entity::nearestStandardColour as well as this namespace's name for it; were
// the two different functions of one signature, this line would not compile
// in any file that sees both headers ("call of overloaded ... is ambiguous").
[[nodiscard]] inline std::string plainNearest(const katana::entity::Color& colour)
{
    return nearestStandardColour(colour);
}

} // namespace katana::archive12d::colour_name_forwarding_test

TEST(ColourNameForwarding, TheArchiveNamesAreTheEntityFunctionsAndNotCopiesOfThem)
{
    using Colour = std::optional<katana::entity::Color> (*)(std::string_view);
    using Names = std::vector<std::string> (*)();
    using Nearest = std::string (*)(const katana::entity::Color&);
    const Colour colour = &a12::standardColour;
    const Names names = &a12::standardColourNames;
    const Nearest nearest = &a12::nearestStandardColour;
    EXPECT_EQ(colour, static_cast<Colour>(&katana::entity::standardColour));
    EXPECT_EQ(names, static_cast<Names>(&katana::entity::standardColourNames));
    EXPECT_EQ(nearest, static_cast<Nearest>(&katana::entity::nearestStandardColour));
}

TEST(ColourNameForwarding, APlainCallInsideTheArchiveNamespaceWithAColourIsNotAmbiguous)
{
    using katana::archive12d::colour_name_forwarding_test::plainNearest;
    // (250, 5, 5) is 5, 5, 5 from red and 111 from dark red in the first
    // channel alone.
    EXPECT_EQ(plainNearest({250, 5, 5, 255}), "red");
    EXPECT_EQ(plainNearest({0, 0, 255, 255}), "blue");
}

// An archive names its colours, and the names are read by the one fold of
// the entity layer: '_' and '-' as a blank, and no blank left at either end.
// So a name with a separator at its end is the standard colour, where it used
// to be no colour at all (trimmed first, "red_" became "red " and matched
// nothing).
TEST(ColourNameForwarding, AnArchivesColourNameWithASeparatorAtAnEndIsTheStandardColour)
{
    EXPECT_EQ(a12::standardColour("red_"), (katana::entity::Color{255, 0, 0, 255}));
    EXPECT_EQ(a12::standardColour("_Dark-Green_"), (katana::entity::Color{0, 100, 0, 255}));
    EXPECT_FALSE(a12::standardColour("_").has_value());
    EXPECT_FALSE(a12::standardColour("pen_025").has_value());
}
