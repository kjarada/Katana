// The words a rule's section is known by. One table gives them
// (entity::kSurveySectionWords): entity::toString(SurveySection) reads it for
// what a reply and a label show, and the Katana customisation format reads it
// for a rule's "sets" - so what a person reads in the window is what they
// write in a file.
//
// The nine words below are written out by hand from the table "The words a
// rule is shown by" in docs/customisation.md, not taken from the table under
// test.

#include <gtest/gtest.h>

#include <cstddef>
#include <string>
#include <utility>
#include <vector>

#include "katana/entity/customisation.hpp"
#include "katana/entity/survey_map.hpp"

using katana::entity::Customisation;
using katana::entity::kSurveySectionWords;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

// In the enumeration's order.
const std::vector<std::pair<SurveySection, std::string>>& words()
{
    static const std::vector<std::pair<SurveySection, std::string>> all = {
        {SurveySection::Map, "feature"},
        {SurveySection::VertexSymbol, "symbol"},
        {SurveySection::VertexTextStyle, "text"},
        {SurveySection::Pipe, "pipe"},
        {SurveySection::VertexPipe, "vertexPipe"},
        {SurveySection::SegmentPipe, "segmentPipe"},
        {SurveySection::StringAttribute, "attributes"},
        {SurveySection::VertexAttribute, "vertexAttributes"},
        {SurveySection::Tinable, "surface"},
    };
    return all;
}

} // namespace

TEST(SurveySectionWords, EachSectionIsShownByItsKatanaWordAndTheTableHoldsAllNineInOrder)
{
    for (const auto& [section, word] : words()) {
        EXPECT_EQ(std::string(katana::entity::toString(section)), word);
    }
    ASSERT_EQ(kSurveySectionWords.size(), words().size());
    for (std::size_t i = 0; i < words().size(); ++i) {
        EXPECT_EQ(kSurveySectionWords[i].section, words()[i].first) << i;
        EXPECT_EQ(std::string(kSurveySectionWords[i].word), words()[i].second) << i;
    }
}

TEST(SurveySectionWords, AFileHoldsARulesSectionByTheWordItIsShownBy)
{
    for (const auto& [section, word] : words()) {
        Customisation customisation;
        customisation.name = "Words";
        SurveyRule rule;
        rule.key = "A*";
        rule.section = section;
        ASSERT_TRUE(customisation.map.add(rule).ok()) << word;

        const auto written = katana::entity::customisationToJson(customisation);
        ASSERT_TRUE(written.ok()) << written.error().describe();
        EXPECT_NE(written->find("\"sets\": \"" + word + "\""), std::string::npos) << *written;

        const auto read = katana::entity::customisationFromJson(*written);
        ASSERT_TRUE(read.ok()) << read.error().describe();
        ASSERT_EQ(read->map.size(), 1u) << word;
        EXPECT_EQ(read->map.rules()[0].section, section) << word;
    }
}

TEST(SurveySectionWords, ARuleWhoseLayerIsNotALayerPathIsRefusedByTheWordLayer)
{
    // "A//B" has an empty level, which no layer path may. The field is
    // SurveyRule::model, and a person knows it as the rule's layer.
    SurveyRule rule;
    rule.key = "WM*";
    rule.model = "A//B";
    const auto status = katana::entity::validate(rule);
    ASSERT_FALSE(status.ok());
    EXPECT_TRUE(status.error().message.starts_with(
        "the layer of rule \"WM*\" is not a valid layer path: "))
        << status.error().message;
}
