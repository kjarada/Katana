// renderText and renderHtml: every section, readable, and safe with hostile ids.

#include <gtest/gtest.h>

#include <sstream>
#include <string>

#include "katana/survey/reduction.hpp"
#include "test_reduction_support.hpp"

using namespace katana::survey;
using namespace reduction_test;

namespace {

ReductionOutcome reduced(const std::string& targetId)
{
    SurveyProject project;
    project.points.push_back(point("A", 1000.0, 1000.0, 50.0));
    project.points.push_back(point("B", 1100.0, 1000.0, 50.0));
    project.unpositionedPoints.push_back(unpositioned(targetId));
    project.stations.push_back(
        setup("S1", "A", 1.5, "B",
              {Shot{"B", 1, Face::Left, 0.0, {}, {}},
               Shot{targetId, 2, Face::Left, deg(90), deg(90), 50.0, 1.5},
               Shot{targetId, 3, Face::Right, deg(270, 0, 2), deg(90), 50.001, 1.5}}));
    ReductionContext context;
    context.input.fileName = "job <1>.gsi";
    context.input.formatName = "Leica GSI-16";
    context.input.recordsRead = 9;
    context.input.notCarried.push_back("no atmospheric state");
    context.createdUtc = "2026-09-24T10:00:00Z";
    auto outcome = reduceAndAdjust(project, ReductionSettings{}, context);
    EXPECT_TRUE(outcome.ok());
    return std::move(*outcome);
}

} // namespace

TEST(ReductionReportText, EverySectionIsPresentInTheModelsOrder)
{
    const std::string text = renderText(reduced("P1").report);
    std::size_t at = 0;
    for (const char* title : {"Input", "Settings used", "Warnings", "Setups",
                              "Face left / face right pairs",
                              "Observations: raw value, each correction, reduced value",
                              "Misclosures and checks", "Coordinates"}) {
        const std::size_t found = text.find(std::string(title) + "\n", at);
        ASSERT_NE(found, std::string::npos) << title;
        at = found;
    }
    // A surveyor's units: the face mean of the FR reading shows in seconds,
    // the distance spread in millimetres, the azimuth in DMS.
    EXPECT_NE(text.find("90\xC2\xB0" "00'01.0\""), std::string::npos);
    EXPECT_NE(text.find("+1.0 mm"), std::string::npos);
    EXPECT_NE(text.find("no atmospheric state"), std::string::npos);
}

TEST(ReductionReportText, ColumnsLineUpByDisplayedCharactersNotBytes)
{
    // In the setups table the cell after the three angles (each with a
    // two-byte degree sign) starts under its heading: padding counts
    // displayed characters.
    const std::string text = renderText(reduced("P1").report);
    const auto displayed = [](const std::string& s, std::size_t bytes) {
        std::size_t w = 0;
        for (std::size_t i = 0; i < bytes; ++i) {
            w += (static_cast<unsigned char>(s[i]) & 0xC0) != 0x80 ? 1 : 0;
        }
        return w;
    };
    std::istringstream lines(text);
    std::string line;
    std::string header;
    std::string row;
    while (std::getline(lines, line)) {
        if (line.find("BS dist check") != std::string::npos) {
            header = line;
        } else if (!header.empty() && line.rfind("  S1 ", 0) == 0) {
            row = line;
            break;
        }
    }
    ASSERT_FALSE(header.empty());
    ASSERT_FALSE(row.empty());
    const std::size_t lastAngle = row.rfind('"');
    ASSERT_NE(lastAngle, std::string::npos);
    const std::size_t next = row.find_first_not_of(' ', lastAngle + 1);
    EXPECT_EQ(displayed(row, next), displayed(header, header.find("BS dist check")));
}

TEST(ReductionReportHtml, IsOneSelfContainedDocumentWithNoScriptAndEscapedIds)
{
    const std::string html = renderHtml(reduced("<script>alert(1)</script>").report);
    EXPECT_EQ(html.rfind("<!DOCTYPE html>", 0), 0U);
    EXPECT_NE(html.find("</html>"), std::string::npos);
    EXPECT_EQ(html.find("<script"), std::string::npos);
    EXPECT_NE(html.find("&lt;script&gt;alert(1)&lt;/script&gt;"), std::string::npos);
    EXPECT_NE(html.find("job &lt;1&gt;.gsi"), std::string::npos);
    EXPECT_EQ(html.find("http://"), std::string::npos);
    EXPECT_EQ(html.find("https://"), std::string::npos);
    EXPECT_NE(html.find("<h2>Coordinates</h2>"), std::string::npos);
}

TEST(ReductionReportHtml, ARejectedObservationIsMarkedAsAnAlarm)
{
    SurveyProject project;
    project.points.push_back(point("A", 0.0, 0.0));
    project.points.push_back(point("B", 100.0, 0.0));
    project.unpositionedPoints.push_back(unpositioned("T"));
    project.stations.push_back(
        setup("S", "A", 1.5, "B",
              {Shot{"B", 1, Face::Left, 0.0, {}, {}},
               Shot{"T", 2, Face::Left, deg(45), deg(90), 100.0},
               Shot{"T", 3, Face::Right, deg(225, 0, 30), deg(90), 100.0}}));
    ReductionSettings settings;
    settings.faceTolerances.excludeOutside = true;
    const auto outcome = reduceAndAdjust(project, settings, {});
    ASSERT_TRUE(outcome.ok());
    const std::string html = renderHtml(outcome->report);
    EXPECT_NE(html.find("<td class=\"alarm\">REJECTED: face pair outside tolerance"),
              std::string::npos);
    EXPECT_NE(renderText(outcome->report).find("OUTSIDE TOLERANCE"), std::string::npos);
}
