// Editing a survey code map by index, and what a match says about itself.
//
// A rule's only identity is its place in the map, and its place is also its
// precedence, so these tests check both what each edit leaves in the map and
// what the codes it answered for resolve to afterwards.

#include <gtest/gtest.h>

#include "katana/entity/survey_map.hpp"

using katana::entity::SurveyMap;
using katana::entity::SurveyMatchKind;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

SurveyRule mapRule(std::string key, std::string model)
{
    SurveyRule rule;
    rule.key = std::move(key);
    rule.section = SurveySection::Map;
    rule.model = std::move(model);
    return rule;
}

// Four rules whose order can be read back from their models.
SurveyMap abcd()
{
    SurveyMap map;
    EXPECT_TRUE(map.add(mapRule("A*", "A")).ok());
    EXPECT_TRUE(map.add(mapRule("B*", "B")).ok());
    EXPECT_TRUE(map.add(mapRule("C*", "C")).ok());
    EXPECT_TRUE(map.add(mapRule("D*", "D")).ok());
    return map;
}

std::string order(const SurveyMap& map)
{
    std::string out;
    for (const SurveyRule& rule : map.rules()) {
        out += rule.model;
    }
    return out;
}

} // namespace

TEST(SurveyMapEditing, AtGivesACopyOfTheRuleThereAndRefusesAnIndexPastTheEnd)
{
    const SurveyMap map = abcd();
    const auto third = map.at(2);
    ASSERT_TRUE(third.ok()) << third.error().describe();
    EXPECT_EQ(third->key, "C*");
    EXPECT_EQ(third->model, "C");

    const auto past = map.at(4);
    ASSERT_FALSE(past.ok()) << "four rules: indices 0 to 3";
    EXPECT_EQ(past.error().code, katana::core::ErrorCode::NotFound);
}

TEST(SurveyMapEditing, ReplaceChangesOneRuleAndTheCodesItNowAnswersFor)
{
    SurveyMap map = abcd();
    ASSERT_TRUE(map.replace(1, mapRule("WM*", "SURVEY SERVICES")).ok());
    EXPECT_EQ(order(map), "ASURVEY SERVICESCD");
    // The key changed from B* to WM*, so B01 has lost its rule and WM01 has
    // gained one - the lookup follows the edit rather than the old key.
    EXPECT_TRUE(map.lookup("B01").empty());
    EXPECT_EQ(map.lookup("WM01").resolved.model, "SURVEY SERVICES");
}

TEST(SurveyMapEditing, RemoveTakesOutOneRuleAndItsCodesFallToTheNextRuleThatMatches)
{
    SurveyMap map;
    ASSERT_TRUE(map.add(mapRule("WM*", "SURVEY SERVICES")).ok());
    ASSERT_TRUE(map.add(mapRule("W*", "SURVEY WATER")).ok());
    EXPECT_EQ(map.lookup("WM01").resolved.model, "SURVEY SERVICES") << "the longer prefix wins";

    ASSERT_TRUE(map.remove(0).ok());
    EXPECT_EQ(map.size(), 1u);
    EXPECT_EQ(map.lookup("WM01").resolved.model, "SURVEY WATER")
        << "with WM* gone, W* is the most specific rule left";
}

TEST(SurveyMapEditing, InsertGoesBeforeTheRuleAtThatIndexAndAtTheSizeAppends)
{
    SurveyMap map = abcd();
    ASSERT_TRUE(map.insert(1, mapRule("X*", "X")).ok());
    EXPECT_EQ(order(map), "AXBCD");
    ASSERT_TRUE(map.insert(map.size(), mapRule("Y*", "Y")).ok());
    EXPECT_EQ(order(map), "AXBCDY");
    ASSERT_TRUE(map.insert(0, mapRule("Z*", "Z")).ok());
    EXPECT_EQ(order(map), "ZAXBCDY");
    EXPECT_EQ(map.lookup("X1").resolved.model, "X");
}

TEST(SurveyMapEditing, MoveLeavesTheRuleAtTheTargetWithTheOthersInOrder)
{
    SurveyMap forward = abcd();
    ASSERT_TRUE(forward.move(0, 2).ok());
    EXPECT_EQ(order(forward), "BCAD"); // A taken out of ABCD, put back at index 2

    SurveyMap backward = abcd();
    ASSERT_TRUE(backward.move(3, 1).ok());
    EXPECT_EQ(order(backward), "ADBC"); // D taken out of ABCD, put back at index 1

    SurveyMap still = abcd();
    ASSERT_TRUE(still.move(2, 2).ok());
    EXPECT_EQ(order(still), "ABCD");
}

TEST(SurveyMapEditing, MovingOneOfTwoEqualKeysAheadChangesWhichOneWins)
{
    // Equal keys tie on specificity, and the tie goes to the earlier rule, so
    // order is how an editor says which of two mapfiles' opinions stands.
    SurveyMap map;
    ASSERT_TRUE(map.add(mapRule("WM*", "FIRST")).ok());
    ASSERT_TRUE(map.add(mapRule("WM*", "SECOND")).ok());
    EXPECT_EQ(map.lookup("WM01").resolved.model, "FIRST");
    ASSERT_TRUE(map.move(1, 0).ok());
    EXPECT_EQ(map.lookup("WM01").resolved.model, "SECOND");
}

TEST(SurveyMapEditing, AnEditThatFailsLeavesTheMapExactlyAsItWas)
{
    const SurveyMap before = abcd();
    SurveyMap map = before;

    EXPECT_FALSE(map.replace(0, mapRule(" A*", "A")).ok()) << "a key with a leading blank";
    EXPECT_FALSE(map.replace(0, mapRule("A*", "SURVEY//DETAIL")).ok()) << "an empty path segment";
    EXPECT_FALSE(map.replace(4, mapRule("E*", "E")).ok()) << "past the end";
    EXPECT_FALSE(map.insert(0, mapRule("E *x", "E")).ok()) << "a `*` before the end";
    EXPECT_FALSE(map.insert(5, mapRule("E*", "E")).ok()) << "size() is 4, so 5 is past the end";
    EXPECT_FALSE(map.remove(4).ok());
    EXPECT_FALSE(map.move(4, 0).ok());
    EXPECT_FALSE(map.move(0, 4).ok());

    EXPECT_EQ(map, before);
    EXPECT_EQ(map.lookup("A1").resolved.model, "A") << "and the index still answers";
}

TEST(SurveyMapEditing, ValidateRefusesAKeyWithSurroundingWhitespaceAndAModelThatIsNotALayerPath)
{
    EXPECT_TRUE(validate(mapRule("WM*", "SURVEY SERVICES")).ok());
    EXPECT_TRUE(validate(mapRule("PABB", "")).ok()) << "no model is a rule about something else";
    EXPECT_TRUE(validate(mapRule("PABB", "SURVEY/DETAIL")).ok()) << "a nested layer is a path";

    EXPECT_FALSE(validate(mapRule("WM* ", "SURVEY SERVICES")).ok()) << "trailing blank";
    EXPECT_FALSE(validate(mapRule("\tWM*", "SURVEY SERVICES")).ok()) << "leading tab";
    EXPECT_FALSE(validate(mapRule("WM*", "/SURVEY")).ok()) << "a leading separator";
    EXPECT_FALSE(validate(mapRule("WM*", "SURVEY/..")).ok()) << "a parent-directory segment";

    // The same refusal reaches add(), which is what a mapfile reader calls.
    SurveyMap map;
    EXPECT_FALSE(map.add(mapRule("WM* ", "SURVEY SERVICES")).ok());
    EXPECT_TRUE(map.empty());
}

TEST(SurveyMapEditing, TwoMapsAreEqualOnlyWithTheSameRulesInTheSameOrder)
{
    const SurveyMap one = abcd();
    SurveyMap two = abcd();
    EXPECT_EQ(one, two);

    ASSERT_TRUE(two.move(0, 1).ok());
    EXPECT_NE(one, two) << "the same rules in another order resolve codes differently";
    ASSERT_TRUE(two.move(1, 0).ok());
    EXPECT_EQ(one, two);

    SurveyRule changed = mapRule("A*", "A");
    changed.comment = "abutment";
    ASSERT_TRUE(two.replace(0, changed).ok());
    EXPECT_NE(one, two) << "a comment is part of a rule";
}

TEST(SurveyMapMatchKind, TheMostSpecificKeyThatMatchedSaysHowTheCodeWasMet)
{
    SurveyMap map;
    ASSERT_TRUE(map.add(mapRule("PABB", "SURVEY BRIDGE")).ok());
    ASSERT_TRUE(map.add(mapRule("WM*", "SURVEY SERVICES")).ok());
    SurveyRule everything;
    everything.key = "*";
    everything.section = SurveySection::StringAttribute;
    everything.attributes = {{"text", "DepthLocation", "Top of Pipe"}};
    ASSERT_TRUE(map.add(everything).ok());

    EXPECT_EQ(map.lookup("PABB").kind, SurveyMatchKind::Exact);
    EXPECT_EQ(map.lookup("WM01").kind, SurveyMatchKind::Prefix);
    EXPECT_EQ(map.lookup("ZZ99").kind, SurveyMatchKind::FallbackOnly);

    SurveyMap none;
    ASSERT_TRUE(none.add(mapRule("WM*", "SURVEY SERVICES")).ok());
    EXPECT_EQ(none.lookup("ZZ99").kind, SurveyMatchKind::None);
    EXPECT_FALSE(none.lookup("ZZ99").matched());
}

TEST(SurveyMapMatchKind, OnlyTheStarAnsweringWithAttributesIsNotAMatchButWithAModelItIs)
{
    // Decision D5: "matched" means the map says something about THIS code. A
    // `*` that only attaches attributes answers every code alike; a `*` that
    // names a model is an instruction for codes nobody listed.
    SurveyMap attributesOnly;
    SurveyRule everything;
    everything.key = "*";
    everything.section = SurveySection::StringAttribute;
    everything.attributes = {{"text", "DepthLocation", "Top of Pipe"}};
    ASSERT_TRUE(attributesOnly.add(everything).ok());
    EXPECT_FALSE(attributesOnly.lookup("ZZ99").matched());
    EXPECT_FALSE(attributesOnly.lookup("ZZ99").empty()) << "a rule did match; it is just `*`";

    SurveyMap withModel = attributesOnly;
    ASSERT_TRUE(withModel.add(mapRule("*", "SURVEY MISCELLANEOUS")).ok());
    const auto match = withModel.lookup("ZZ99");
    EXPECT_EQ(match.kind, SurveyMatchKind::FallbackOnly);
    EXPECT_TRUE(match.matched());

    SurveyMap exact;
    ASSERT_TRUE(exact.add(everything).ok());
    SurveyRule comment;
    comment.key = "PABB";
    comment.section = SurveySection::StringAttribute;
    comment.attributes = {{"text", "Note", "abutment"}};
    ASSERT_TRUE(exact.add(comment).ok());
    EXPECT_TRUE(exact.lookup("PABB").matched()) << "an exact key is always a match";
}
