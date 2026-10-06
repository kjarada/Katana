// The spellings of the linework control codes, where a customisation keeps
// them (include/katana/entity/linework_codes.hpp). What the controls DO is
// tested with the linework itself, in the cad suite.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/entity/linework_codes.hpp"

using katana::core::ErrorCode;
using katana::entity::LineworkCodeMember;
using katana::entity::lineworkCodeMembers;
using katana::entity::LineworkCodes;

TEST(LineworkCodeSpellings, TheDefaultsAreTheSevenCommonFieldSpellingsAndAreSound)
{
    const LineworkCodes codes;
    EXPECT_EQ(codes.start, "ST");
    EXPECT_EQ(codes.end, "END");
    EXPECT_EQ(codes.close, "CL");
    EXPECT_EQ(codes.arcStart, "BC");
    EXPECT_EQ(codes.arcEnd, "EC");
    EXPECT_EQ(codes.join, "JPN");
    EXPECT_EQ(codes.rectangle, "RECT");
    EXPECT_TRUE(katana::entity::validate(codes).ok());
}

TEST(LineworkCodeSpellings, TheSevenControlsAreListedByNameEachWithItsOwnSpelling)
{
    const auto members = lineworkCodeMembers();
    std::vector<std::string> names;
    for (const LineworkCodeMember& member : members) {
        names.emplace_back(member.name);
    }
    EXPECT_EQ(names, (std::vector<std::string>{"start", "end", "close", "arcStart", "arcEnd",
                                               "join", "rectangle"}));

    // Each name reaches the spelling of that name and no other: written
    // through the list, read through the struct.
    LineworkCodes codes;
    for (std::size_t i = 0; i < members.size(); ++i) {
        codes.*members[i].spelling = "X" + std::to_string(i);
    }
    EXPECT_EQ(codes.start, "X0");
    EXPECT_EQ(codes.end, "X1");
    EXPECT_EQ(codes.close, "X2");
    EXPECT_EQ(codes.arcStart, "X3");
    EXPECT_EQ(codes.arcEnd, "X4");
    EXPECT_EQ(codes.join, "X5");
    EXPECT_EQ(codes.rectangle, "X6");
}

TEST(LineworkCodeSpellings, TwoControlsSpelledAlikeAreRefusedNamingBoth)
{
    LineworkCodes same;
    same.close = "END"; // the end's spelling
    const auto alike = katana::entity::validate(same);
    ASSERT_FALSE(alike.ok());
    EXPECT_EQ(alike.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(alike.error().context, "end and close are \"END\"");

    // Tokens are matched ignoring ASCII case, so the spellings are compared so.
    LineworkCodes folded;
    folded.arcEnd = "bc";
    const auto ignoringCase = katana::entity::validate(folded);
    ASSERT_FALSE(ignoringCase.ok());
    EXPECT_EQ(ignoringCase.error().context, "arcStart and arcEnd are \"BC\"");
}

TEST(LineworkCodeSpellings, ASpellingWithABlankInItIsRefusedNamingItsControl)
{
    LineworkCodes blank;
    blank.rectangle = "RE CT";
    const auto status = katana::entity::validate(blank);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(status.error().context, "rectangle=\"RE CT\"");

    LineworkCodes tab;
    tab.start = "S\tT";
    EXPECT_FALSE(katana::entity::validate(tab).ok()) << "a tab splits a code as a space does";
}

TEST(LineworkCodeSpellings, AnEmptySpellingSwitchesAControlOffAndTwoOffAreNotAClash)
{
    LineworkCodes off;
    off.join.clear();
    off.rectangle.clear();
    EXPECT_TRUE(katana::entity::validate(off).ok());

    LineworkCodes none;
    for (const LineworkCodeMember& member : lineworkCodeMembers()) {
        (none.*member.spelling).clear();
    }
    EXPECT_TRUE(katana::entity::validate(none).ok()) << "every control off is a sound table";
}

TEST(LineworkCodeSpellings, TwoTablesAreEqualWhenEverySpellingIs)
{
    EXPECT_EQ(LineworkCodes{}, LineworkCodes{});
    for (const LineworkCodeMember& member : lineworkCodeMembers()) {
        LineworkCodes changed;
        changed.*member.spelling += "X";
        EXPECT_NE(changed, LineworkCodes{}) << member.name;
    }
}
