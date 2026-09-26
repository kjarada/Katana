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

// What is read is kept as it was written too, under the header it was
// written under, so that a deliverable written back out (the IFC export's
// delivery property set) carries the schedule's own values rather than
// Katana's reading of them.
TEST(SubsurfaceUtilityCsv, KeepsEachInterpretedCellAsWrittenUnderItsHeader)
{
    const auto lines = parseUtilityCsv(kSchedule);
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    const auto& written = (*lines)[0].attributes.written;
    EXPECT_EQ(written.at("Line"), "W1");
    EXPECT_EQ(written.at("Type"), "water");
    EXPECT_EQ(written.at("Status"), "live");
    EXPECT_EQ(written.at("Owner"), "Water Co, North");
    EXPECT_EQ(written.at("Diameter_mm"), "150");
    EXPECT_FALSE(written.contains("Description")); // W1 has none

    const auto& pothole = (*lines)[0].vertices[2].written;
    EXPECT_EQ(pothole.at("Point_ID"), "P3");
    EXPECT_EQ(pothole.at("Location Method"), "pothole");
    EXPECT_EQ(pothole.at("RL"), "99.10"); // not 99.1
    EXPECT_EQ(pothole.at("Level_Ref"), "centre");
    EXPECT_FALSE((*lines)[0].vertices[0].written.contains("RL")); // absent stays absent
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

// ---- a schedule in the TfNSW Utility Schema's attribute names ------------------

TEST(SubsurfaceUtilityCsv, ReadsTheTfnswAttributeNames)
{
    const auto lines = parseUtilityCsv(
        "point,easting,northing,surface,AssetIdentifier,AssetTypeCode,AssetFeature,AssetOwner,"
        "AssetStatus,Size,Configuration,DepthLocation,Depth,QualityLevel,LocateMethod,Notes\n"
        "1,0,0,,D-1,D,Culvert,Private,Disused,1200 x 900,Single,Top Row Invert,1.5,"
        "Quality Level A,Potholing,dug 2026/09/20\n"
        "2,0,5,,D-1,D,Culvert,Private,Disused,1200 x 900,Single,Other,1.4,Unknown,Survey,\n");
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    ASSERT_EQ(lines->size(), 1u);
    const UtilityLine& culvert = lines->front();
    EXPECT_EQ(culvert.id, "D-1");
    EXPECT_EQ(culvert.attributes.type, UtilityType::Stormwater);
    EXPECT_EQ(culvert.attributes.status, UtilityStatus::Disused);
    EXPECT_EQ(culvert.attributes.owner, "Private");
    // 1200 x 900: the larger side, as an inside dimension.
    EXPECT_DOUBLE_EQ(culvert.attributes.diameter, 1.2);
    EXPECT_TRUE(culvert.attributes.diameterIsInside);
    EXPECT_EQ(culvert.attributes.fields.at("AssetFeature"), "Culvert");

    const UtilityVertex& first = culvert.vertices[0];
    EXPECT_EQ(first.levelReference, LevelReference::Invert);
    EXPECT_EQ(first.depth, 1.5);
    EXPECT_FALSE(first.level);
    EXPECT_EQ(first.claimed, QualityLevel::A);
    EXPECT_EQ(first.evidence.method, LocationMethod::NonDestructiveExcavation);
    EXPECT_EQ(first.fields.at("Notes"), "dug 2026/09/20");

    const UtilityVertex& second = culvert.vertices[1];
    EXPECT_EQ(second.levelReference, LevelReference::Unknown);
    EXPECT_FALSE(second.claimed); // "Unknown" claims nothing
    EXPECT_EQ(second.evidence.method, LocationMethod::SurfaceFeature);

    // Cover from a depth alone: to the invert 1.5, less the 1.2 m inside
    // height, is 0.3 m to the inside top - and says the wall is not in it.
    // The second vertex's depth is to an unknown point on the culvert.
    const auto cover = depthOfCover(culvert);
    ASSERT_TRUE(cover.ok()) << cover.error().describe();
    ASSERT_TRUE((*cover)[0].cover);
    EXPECT_NEAR(*(*cover)[0].cover, 0.3, 1e-12);
    EXPECT_FALSE((*cover)[1].cover);
    EXPECT_EQ((*cover)[1].note, "the level's place on the service is unknown");
}

TEST(SubsurfaceUtilityCsv, InsideCoverIsSaidToBeToTheInsideTop)
{
    UtilityLine line;
    line.id = "W";
    line.attributes.diameter = 0.3;
    line.attributes.diameterIsInside = true;
    UtilityVertex a;
    a.id = "a";
    a.depth = 1.5;
    a.levelReference = LevelReference::Invert;
    a.evidence = {LocationMethod::NonDestructiveExcavation, 0.02, 0.02, true};
    UtilityVertex b = a;
    b.id = "b";
    b.position = {0, 1};
    line.vertices = {a, b};
    const auto cover = depthOfCover(line);
    ASSERT_TRUE(cover.ok());
    EXPECT_NEAR(*(*cover)[0].cover, 1.2, 1e-12);
    EXPECT_NE((*cover)[0].note.find("inside top"), std::string::npos) << (*cover)[0].note;
}

TEST(SubsurfaceUtilityCsv, TheSchemaNamesOfTypesMethodsAndLevelsParse)
{
    EXPECT_EQ(parseQualityLevel("Quality Level B"), QualityLevel::B);
    EXPECT_FALSE(parseQualityLevel("Unknown"));
    EXPECT_EQ(parseLocationMethod("Electronic Detection"), LocationMethod::ElectromagneticLocation);
    EXPECT_EQ(parseLocationMethod("Archive Drawings and Plans"), LocationMethod::Records);
    EXPECT_EQ(parseLocationMethod("Geographic Information System"), LocationMethod::Records);
    EXPECT_EQ(parseLocationMethod("Unknown"), LocationMethod::Unknown);
    EXPECT_EQ(maximumQualityLevel(LocationMethod::Unknown), QualityLevel::D);
    EXPECT_EQ(parseLevelReference("Top of Concrete Encasement"), LevelReference::Top);
    EXPECT_EQ(parseLevelReference("Plastic Cover Protection Encountered"), LevelReference::Top);
    EXPECT_EQ(parseLevelReference("Obvert"), LevelReference::Top);
    EXPECT_EQ(parseLevelReference("Other"), LevelReference::Unknown);
    EXPECT_EQ(parseUtilityType("Fire Service"), UtilityType::FireService);
    EXPECT_EQ(parseUtilityType("F"), UtilityType::FireService);
    EXPECT_EQ(parseUtilityType("ITS"), UtilityType::IntelligentTransport);
    EXPECT_EQ(parseUtilityType("Petroleum"), UtilityType::Fuel);
    EXPECT_EQ(parseUtilityType("Not Specified"), UtilityType::Unknown);
    EXPECT_FALSE(parseUtilityType("X"));
    EXPECT_EQ(parseUtilityStatus("Disused"), UtilityStatus::Disused);
}

TEST(SubsurfaceUtilityCsv, AServicesSchemaAttributesMustAgreeAcrossItsRows)
{
    const auto lines =
        parseUtilityCsv("point,easting,northing,method,AssetIdentifier,AssetFeature\n"
                        "1,0,0,EML,C-1,Pit\n2,0,5,EML,C-1,Pole\n");
    ASSERT_FALSE(lines.ok());
    EXPECT_NE(lines.error().message.find("AssetFeature \"Pit\" at line 2 and \"Pole\" here"),
              std::string::npos)
        << lines.error().describe();
    const auto size =
        parseUtilityCsv("point,easting,northing,method,line,Size\n1,0,0,EML,C,large\n");
    ASSERT_FALSE(size.ok());
    EXPECT_NE(size.error().message.find("W x H millimetres"), std::string::npos);
}

TEST(SubsurfaceReport, AClaimAlongTheWholeAssetIsTestedSegmentBySegment)
{
    // Both ends claim and are QL-B; 18 m between them is past the 10 m spacing.
    auto lines = parseUtilityCsv("line,point,easting,northing,method,h_unc,ql\n"
                                 "E,1,0,0,GPR,0.2,QL-B\nE,2,18,0,GPR,0.2,QL-B\n");
    ASSERT_TRUE(lines.ok());
    const auto report = renderInvestigationReport(*lines);
    ASSERT_TRUE(report.ok());
    EXPECT_NE(report->find("E: 18.000 m is claimed better than it grades: 1 -> 2 claimed QL-B, "
                           "grades QL-C"),
              std::string::npos)
        << *report;
}

TEST(SubsurfaceReport, TheClashSuggestedForADeliverySchema)
{
    ClearanceResult result;
    result.status = ClearanceStatus::Clear;
    EXPECT_EQ(clashOf(result), Clash::No);
    result.status = ClearanceStatus::Unconfirmed;
    EXPECT_EQ(clashOf(result), Clash::Unknown);
    result.status = ClearanceStatus::WithinTolerance;
    EXPECT_EQ(clashOf(result), Clash::Soft);
    result.status = ClearanceStatus::Conflict;
    result.horizontalGap = 0.1; // apart, but closer than required
    EXPECT_EQ(clashOf(result), Clash::Soft);
    result.horizontalGap = -0.2; // overlapping in plan, no levels to separate them
    EXPECT_EQ(clashOf(result), Clash::Hard);
    result.verticalGap = 0.1; // overlapping in plan, apart in level
    EXPECT_EQ(clashOf(result), Clash::Soft);
    EXPECT_STREQ(toString(Clash::Hard), "Hard");
}
