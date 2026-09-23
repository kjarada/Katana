// The survey map's key index gives the answer testing every rule gives.
//
// The index is only a speed-up: match() must return the very rules, in the
// very order, that the original definition does - every rule whose key
// matches, stable-sorted so that an exact key comes first, then longer
// prefixes before shorter, then the bare `*`, ties in read order. That
// definition is written out below independently of the implementation, and
// the two are compared over codes chosen to hit every boundary.

#include <gtest/gtest.h>

#include <algorithm>
#include <limits>

#include "katana/entity/survey_map.hpp"

using katana::entity::SurveyMap;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

// The definition, done the slow way: test every rule, then sort.
std::vector<std::size_t> everyRuleTested(const SurveyMap& map, std::string_view code)
{
    const auto& rules = map.rules();
    const auto specificity = [](const SurveyRule& rule) {
        if (rule.key.back() != '*') {
            return std::numeric_limits<std::size_t>::max();
        }
        return rule.key.size() - 1;
    };
    std::vector<std::size_t> found;
    for (std::size_t i = 0; i < rules.size(); ++i) {
        const std::string& key = rules[i].key;
        const std::string_view prefix = std::string_view(key).substr(0, key.size() - 1);
        const bool matches = key.back() != '*' ? key == code : code.starts_with(prefix);
        if (matches) {
            found.push_back(i);
        }
    }
    std::stable_sort(found.begin(), found.end(), [&](std::size_t a, std::size_t b) {
        return specificity(rules[a]) > specificity(rules[b]);
    });
    return found;
}

SurveyRule rule(std::string key, std::string model)
{
    SurveyRule out;
    out.key = std::move(key);
    out.section = SurveySection::Map;
    out.model = std::move(model);
    return out;
}

// Overlapping keys of every shape, several of them repeated so that ties in
// read order are exercised, and in no particular order.
SurveyMap overlapping()
{
    SurveyMap map;
    for (const auto& [key, model] : std::vector<std::pair<std::string, std::string>>{
             {"WM*", "a"},  {"*", "b"},    {"W*", "c"},     {"WM01", "d"}, {"WM0*", "e"},
             {"WM*", "f"},  {"PABB", "g"}, {"*", "h"},      {"PAB*", "i"}, {"WM01", "j"},
             {"P*", "k"},   {"WM01*", "l"}, {"PABB*", "m"}, {"1*", "n"},   {"10*", "o"},
             {"W*", "p"}}) {
        EXPECT_TRUE(map.add(rule(key, model)).ok());
    }
    return map;
}

const std::vector<std::string>& codes()
{
    static const std::vector<std::string> all = {
        "",     "W",    "WM",    "WM0",  "WM01", "WM012", "WM02", "WN01", "PABB",
        "PAB",  "PABBX", "PA",   "P",    "1",    "10",    "100",  "2",    "wm01",
        "ZZ99", " WM01", "WM01 "};
    return all;
}

void expectIndexAgrees(const SurveyMap& map)
{
    for (const std::string& code : codes()) {
        EXPECT_EQ(map.matchIndices(code), everyRuleTested(map, code)) << "code \"" << code << "\"";
        // match() is the same list as pointers.
        const auto pointers = map.match(code);
        const auto indices = map.matchIndices(code);
        ASSERT_EQ(pointers.size(), indices.size());
        for (std::size_t i = 0; i < indices.size(); ++i) {
            EXPECT_EQ(pointers[i], &map.rules()[indices[i]]);
        }
    }
}

} // namespace

TEST(SurveyMapIndex, TheIndexFindsTheSameRulesInTheSameOrderAsTestingEveryRule)
{
    const SurveyMap map = overlapping();
    expectIndexAgrees(map);

    // Worked by hand for one code, so the reference itself is checked too.
    // WM012 matches WM01* (l, 11), WM0* (e, 4), WM* (a, 0; f, 5), W* (c, 2;
    // p, 15) and * (b, 1; h, 7): longest prefix first, ties in read order.
    EXPECT_EQ(map.matchIndices("WM012"),
              (std::vector<std::size_t>{11, 4, 0, 5, 2, 15, 1, 7}));
    // WM01 matches the exact key twice (d, 3; j, 9), then WM01* (its prefix
    // is the whole code), WM0*, WM*, W* and *.
    EXPECT_EQ(map.matchIndices("WM01"),
              (std::vector<std::size_t>{3, 9, 11, 4, 0, 5, 2, 15, 1, 7}));
}

TEST(SurveyMapIndex, EveryEditKeepsTheIndexInStepWithTheRules)
{
    SurveyMap map = overlapping();
    ASSERT_TRUE(map.remove(0).ok());
    expectIndexAgrees(map);
    ASSERT_TRUE(map.insert(3, rule("WM0*", "x")).ok());
    expectIndexAgrees(map);
    ASSERT_TRUE(map.move(0, 10).ok());
    expectIndexAgrees(map);
    ASSERT_TRUE(map.move(12, 2).ok());
    expectIndexAgrees(map);
    ASSERT_TRUE(map.replace(5, rule("ZZ*", "y")).ok());
    expectIndexAgrees(map);
    EXPECT_EQ(map.lookup("ZZ99").resolved.model, "y");

    // A copy carries its own index, and it answers for the copy.
    SurveyMap copy = map;
    ASSERT_TRUE(copy.remove(0).ok());
    expectIndexAgrees(copy);
    expectIndexAgrees(map);
}
