#include <gtest/gtest.h>

#include <string>

#include "katana/survey/subsurface/utility_csv.hpp"
#include "katana/survey/subsurface/utility_report.hpp"

using namespace katana::survey::subsurface;
using katana::core::ErrorCode;

namespace {

// Two services. W1 is a 150 mm water main: two detections 8 m apart and a
// pothole 5 m on. G1 is a gas service known only from the records. The
// header is in a mix of letter cases and aliases, as a spreadsheet export is.
constexpr std::string_view kSchedule =
    "\xEF\xBB\xBF"
    "Line,Point_ID,Easting,Northing,Location Method,RL,Level_Ref,Surface,H_Unc,V_Unc,QL,Path,"
    "Type,Owner,Material,Diameter_mm,Status,Description\n"
    "# located 2026-09-20\n"
    "W1,P1,1000.0,5000.0,EML,,,,0.10,,QL-B,,water,\"Water Co, North\",DICL,150,live,\n"
    "W1,P2,1008.0,5000.0,EML,,,,0.10,,QL-B,,water,,,,,\n"
    "G1,R1,990.0,4990.0,records,,,,,,QL-D,assumed,gas,,,,,\"from 1987 plan\"\n"
    "W1,P3,1013.0,5000.0,pothole,99.10,centre,100.00,0.02,0.02,QL-A,,,,,150,,\n"
    "\n"
    "G1,R2,990.0,5010.0,records,,,,,,QL-D,,,,,,,\n";

} // namespace

TEST(SubsurfaceUtilityCsv, ReadsLinesByNameInOrderOfFirstAppearance)
{
    const auto lines = parseUtilityCsv(kSchedule);
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    ASSERT_EQ(lines->size(), 2u);

    const UtilityLine& water = (*lines)[0];
    EXPECT_EQ(water.id, "W1");
    EXPECT_EQ(water.attributes.type, UtilityType::Water);
    EXPECT_EQ(water.attributes.owner, "Water Co, North");
    EXPECT_EQ(water.attributes.material, "DICL");
    EXPECT_DOUBLE_EQ(water.attributes.diameter, 0.150);
    EXPECT_EQ(water.attributes.status, UtilityStatus::InService);
    ASSERT_EQ(water.vertices.size(), 3u);
    EXPECT_EQ(water.vertices[1].id, "P2");
    EXPECT_DOUBLE_EQ(water.vertices[1].position.easting, 1008.0);
    EXPECT_DOUBLE_EQ(water.vertices[1].position.northing, 5000.0);
    EXPECT_EQ(water.vertices[0].evidence.method, LocationMethod::ElectromagneticLocation);
    EXPECT_FALSE(water.vertices[0].level); // absent is not zero
    EXPECT_EQ(water.vertices[2].levelReference, LevelReference::Centre);
    EXPECT_EQ(water.vertices[2].level, 99.10);
    EXPECT_EQ(water.vertices[2].surfaceLevel, 100.0);
    EXPECT_EQ(water.vertices[2].claimed, QualityLevel::A);
    EXPECT_TRUE(water.pathEvidence.empty());

    const UtilityLine& gas = (*lines)[1];
    EXPECT_EQ(gas.attributes.description, "from 1987 plan");
    ASSERT_EQ(gas.pathEvidence.size(), 1u);
    EXPECT_EQ(gas.pathEvidence[0], PathEvidence::Assumed);
}

TEST(SubsurfaceUtilityCsv, RefusesWhatItWouldOtherwiseHaveToGuess)
{
    const auto code = [](std::string_view text) {
        const auto result = parseUtilityCsv(text);
        return result.ok() ? std::string("ok") : result.error().describe();
    };
    const std::string header = "line,point,easting,northing,method";

    // A misspelt column is not read past.
    EXPECT_NE(code(header + ",survace\nW,1,0,0,EML,1\n").find("unknown column \"survace\""),
              std::string::npos);
    // X and Y are not accepted for easting and northing: which is which differs
    // between conventions.
    EXPECT_NE(code("line,point,x,y,method\n").find("unknown column \"x\""), std::string::npos);
    EXPECT_NE(code("line,point,easting,northing\n").find("\"method\" is missing"),
              std::string::npos);
    EXPECT_NE(code(header + "\nW,1,0,0,dowsing\n").find("not a location method"),
              std::string::npos);
    EXPECT_NE(code(header + "\nW,1,0,0\n").find("4 fields where the header has 5"),
              std::string::npos);
    EXPECT_NE(code(header + "\nW,1,0,0,EML\nW,1,1,1,EML\n").find("appears twice"),
              std::string::npos);
    EXPECT_NE(code(header + ",type\nW,1,0,0,EML,water\nW,2,1,1,EML,gas\n")
                  .find("has type \"water\" at line 2 and \"gas\" here [line 3]"),
              std::string::npos);
    EXPECT_NE(code(header + ",diameter_mm\nW,1,0,0,EML,0\n").find("positive number"),
              std::string::npos);
    EXPECT_NE(code(header + "\nW,\"1,0,0,EML\n").find("unbalanced quotes"), std::string::npos);
    EXPECT_NE(code("# nothing\n").find("no header row"), std::string::npos);
}

TEST(SubsurfaceUtilityCsv, ReadsADesignCentreLine)
{
    const auto design = parseDesignCsv("Easting,Northing,Level\n0,0,10\n0,10,\n", "stage 1");
    ASSERT_TRUE(design.ok()) << design.error().describe();
    EXPECT_EQ(design->id, "stage 1");
    ASSERT_EQ(design->vertices.size(), 2u);
    EXPECT_EQ(design->vertices[0].level, 10.0);
    EXPECT_DOUBLE_EQ(design->vertices[1].position.northing, 10.0);
    EXPECT_FALSE(design->vertices[1].level);
}

TEST(SubsurfaceReport, TheInvestigationReportEndsWithWhatToActOn)
{
    const auto lines = parseUtilityCsv(kSchedule);
    ASSERT_TRUE(lines.ok());
    const auto report = renderInvestigationReport(*lines, {}, 0.9);
    ASSERT_TRUE(report.ok()) << report.error().describe();
    const std::string& text = *report;

    EXPECT_NE(text.find("2 lines, 5 vertices"), std::string::npos) << text;
    // P3: centre 99.10 on a 150 mm main -> top 99.175, cover 0.825.
    EXPECT_NE(text.find("cover 0.825"), std::string::npos) << text;
    EXPECT_NE(text.find("W1 / P3: cover 0.825 m is below the minimum 0.900 m"), std::string::npos)
        << text;
    // P2 -> P3 is 5 m between a detection and an exposure: QL-B.
    EXPECT_NE(text.find("P2 -> P3  5.000 m  QL-B"), std::string::npos) << text;
    EXPECT_NE(text.find("G1: not recorded: owner, status, material, size"), std::string::npos)
        << text;
    EXPECT_NE(text.find("water                      0.000      13.000"), std::string::npos) << text;
}

TEST(SubsurfaceReport, ANonGradedClaimIsAFinding)
{
    auto lines = parseUtilityCsv("line,point,easting,northing,method,ql\n"
                                 "W,1,0,0,GPR,QL-A\nW,2,0,5,GPR,QL-B\n");
    ASSERT_TRUE(lines.ok());
    const auto report = renderInvestigationReport(*lines);
    ASSERT_TRUE(report.ok());
    EXPECT_NE(report->find("W / 1: claimed QL-A, the ground penetrating radar evidence "
                           "supports QL-C"),
              std::string::npos)
        << *report; // no uncertainty given: not assumed to be good
}

TEST(SubsurfaceReport, ClearanceAndVerificationReportsNameWhatToDoNext)
{
    const auto lines = parseUtilityCsv(kSchedule);
    ASSERT_TRUE(lines.ok());
    DesignAlignment design;
    design.id = "stage 1";
    design.vertices = {{{4995, 1010}, std::nullopt}, {{5005, 1010}, std::nullopt}};
    const ClearanceRequirement requirement;
    const auto results = checkClearance(design, *lines, requirement);
    ASSERT_TRUE(results.ok()) << results.error().describe();
    const std::string clearance = renderClearanceReport(design, *results, requirement);
    // The works cross W1's P2-P3 in plan with no levels: a conflict. P1-P2
    // ends 2 m short of them (QL-B: 2 - 0.075 - 0.3 >= 0.3), and G1 runs
    // north-south at easting 990, 20 m off: clear even at QL-D's margin.
    EXPECT_NE(clearance.find("2 services, 3 segments"), std::string::npos) << clearance;
    EXPECT_NE(clearance.find("Segments: 1 conflict, 0 unconfirmed, 0 within tolerance, 2 clear"),
              std::string::npos)
        << clearance;
    EXPECT_NE(clearance.find("W1 P2 -> P3"), std::string::npos) << clearance;
    EXPECT_EQ(clearance.find("W1 P1 -> P2"), std::string::npos) << clearance; // clear: not listed

    const std::string verification = renderVerificationReport(verifyDetections(*lines));
    EXPECT_NE(verification.find("nothing was verified"), std::string::npos) << verification;
}
