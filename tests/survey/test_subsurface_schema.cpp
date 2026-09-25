#include <gtest/gtest.h>

#include <algorithm>
#include <string>

#include "katana/survey/subsurface/delivery_schema.hpp"

using namespace katana::survey::subsurface;
using katana::core::ErrorCode;

namespace {

// A schema of the tests' own, in the shape tools/utility_schema_domains.py
// writes: the real ones are clients' documents and stay out of the
// repository. Features belong under a type code; sizes are open.
constexpr std::string_view kSchema = R"(kind,attribute,value,detail,label
schema,Example Utility Schema,0.1,,
parent,TypeCode,,,
identifier,AssetId,,,
prefix,AssetId,TypeCode,,
field,AssetId,Alphanumerical,Yes,Asset Id
field,TypeCode,Domain List: Type Code,Yes,Type Code
field,Feature,Domain List: Feature,Yes,Feature
field,Status,Domain List: Status,Yes,Status
field,Size,Domain List: Size,No,Size
field,Surveyed,Date (YYYY/MM/DD),Yes,Date Surveyed
field,Depth,Real Number (metres),Yes,Depth
field,Report,Free Text,Conditional,Report
domain,TypeCode,W,,
domain,TypeCode,E,,
domain,Feature,Valve,W,
domain,Feature,Pit,W,
domain,Feature,Pit,E,
domain,Feature,Other,,
domain,Status,Live,,
domain,Status,Dead,,
open,Size,,,
domain,Size,Unknown,,
)";

DeliverySchema schema()
{
    auto parsed = parseDeliverySchema(kSchema);
    EXPECT_TRUE(parsed.ok()) << parsed.error().describe();
    return std::move(parsed).value();
}

std::vector<std::string> problems(const SchemaCheck& check)
{
    std::vector<std::string> out;
    for (const SchemaFinding& finding : check.findings) {
        out.push_back(std::string(finding.severity == FindingSeverity::Error ? "E " : "W ") +
                      std::to_string(finding.lineNumber) + " " + finding.attribute + " " +
                      finding.problem);
    }
    return out;
}

} // namespace

TEST(SubsurfaceDeliverySchema, ReadsASchemaFile)
{
    const DeliverySchema parsed = schema();
    EXPECT_EQ(parsed.title, "Example Utility Schema");
    EXPECT_EQ(parsed.version, "0.1");
    ASSERT_EQ(parsed.fields.size(), 8u);
    EXPECT_EQ(parsed.fields[5].label, "Date Surveyed");
    EXPECT_EQ(parsed.fields[7].requirement, Requirement::Conditional);
    EXPECT_EQ(parsed.parentAttribute, "TypeCode");
    EXPECT_EQ(parsed.identifierAttribute, "AssetId");
    EXPECT_EQ(parsed.prefixes.at("AssetId"), "TypeCode");
    EXPECT_EQ(parsed.domains.at("Feature").values.size(), 4u);
    EXPECT_TRUE(parsed.domains.at("Size").open);
}

TEST(SubsurfaceDeliverySchema, RefusesASchemaFileItCannotUse)
{
    const auto error = [](std::string_view text) {
        const auto result = parseDeliverySchema(text);
        return result.ok() ? std::string("ok") : result.error().describe();
    };
    EXPECT_NE(error("kind,attribute,value,detail\nrule,X,,\n").find("kind \"rule\""),
              std::string::npos);
    EXPECT_NE(error("kind,attribute,value,detail\nfield,X,Free Text,Yes\ndomain,Y,1,\n")
                  .find("a domain for \"Y\", which is not a field"),
              std::string::npos);
    EXPECT_NE(error("kind,attribute,value,detail\nschema,S,1,\n").find("no fields"),
              std::string::npos);
    EXPECT_NE(error("kind,attribute,value,detail\nfield,X,Free Text,Maybe\n")
                  .find("not Yes, No or Conditional"),
              std::string::npos);
    EXPECT_NE(error("kind,attribute\n").find("\"value\" column"), std::string::npos);
}

TEST(SubsurfaceDeliverySchema, ChecksEveryRowAgainstTheSchema)
{
    // Lines: 1 header, 2 clean, 3..8 one problem each (7 has two).
    const auto check = checkDelivery("AssetId,Type Code,Feature,Status,Size,date surveyed,Depth,"
                                     "Report,easting\n"
                                     "W-1,W,Valve,Live,600,2026/09/20,1.2,,1\n"
                                     "E-1,E,Valve,Live,600,2026/09/20,1.2,,1\n"
                                     "E-2,E,Pit,live,2 x 1,2026/09/20,1.2,,1\n"
                                     "E-3,E,Pit,Live,big,2026/09/20,1.2,,1\n"
                                     "E-4,E,Pit,Live,Unknown,2026/02/30,1.2,,1\n"
                                     "X-5,E,Other,,,2024/02/29,deep,,1\n"
                                     "E-6,E,Pit,Dead,,2026/09/20,0.5,see file,1\n",
                                     schema());
    ASSERT_TRUE(check.ok()) << check.error().describe();
    EXPECT_EQ(check->rows, 7u);
    EXPECT_EQ(check->assets, 7u);
    EXPECT_EQ(check->notInSchema, std::vector<std::string>{"easting"});
    EXPECT_EQ(check->conditionalNotChecked, 1u);
    const std::vector<std::string> expected{
        "E 3 Feature \"Valve\" is not listed under TypeCode \"E\"",
        "W 4 Status \"live\" is listed as \"Live\"; values must be spelt exactly",
        "E 5 Size \"big\" is not in the domain",
        "E 6 Surveyed \"2026/02/30\" is not a date as YYYY/MM/DD",
        "E 7 Status is empty",
        "E 7 Depth \"deep\" is not a number",
        "W 7 AssetId does not start with its TypeCode \"E\"",
    };
    EXPECT_EQ(problems(*check), expected);
    EXPECT_EQ(check->count(FindingSeverity::Error), 5u);
    EXPECT_EQ(check->count(FindingSeverity::Warning), 2u);
}

TEST(SubsurfaceDeliverySchema, AMandatoryAttributeWithNoColumnIsOneFinding)
{
    const auto check = checkDelivery("AssetId,TypeCode,Feature,Status,Surveyed\n"
                                     "W-1,W,Pit,Live,2026/09/20\nW-2,W,Pit,Live,2026/09/20\n",
                                     schema());
    ASSERT_TRUE(check.ok());
    ASSERT_EQ(check->findings.size(), 1u);
    EXPECT_EQ(check->findings[0].attribute, "Depth");
    EXPECT_EQ(check->findings[0].problem, "is mandatory and has no column");
    EXPECT_EQ(check->findings[0].lineNumber, 0u);
}

TEST(SubsurfaceDeliverySchema, TheReportGroupsFindingsAndCountsTheirRows)
{
    const DeliverySchema parsed = schema();
    const auto check = checkDelivery("AssetId,TypeCode,Feature,Status,Surveyed,Depth\n"
                                     "W-1,W,Pit,,2026/09/20,1\nW-2,W,Pit,,2026/09/20,1\n"
                                     "W-3,W,Pit,Live,2026/09/20,1\n",
                                     parsed);
    ASSERT_TRUE(check.ok());
    const std::string text = renderSchemaCheck(*check, parsed);
    EXPECT_NE(text.find("against Example Utility Schema v0.1"), std::string::npos) << text;
    EXPECT_NE(text.find("3 rows, 3 assets (AssetId): 2 errors, 0 warnings"), std::string::npos)
        << text;
    EXPECT_NE(text.find("error    Status is empty (2 rows: lines 2, 3)"), std::string::npos)
        << text;

    const auto clean = checkDelivery(
        "AssetId,TypeCode,Feature,Status,Surveyed,Depth\nW-1,W,Pit,Live,2026/09/20,1\n", parsed);
    ASSERT_TRUE(clean.ok());
    EXPECT_NE(renderSchemaCheck(*clean, parsed).find("The schedule meets the schema."),
              std::string::npos);
}
