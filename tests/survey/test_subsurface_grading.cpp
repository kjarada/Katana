#include <gtest/gtest.h>

#include <cmath>
#include <limits>

#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/quality_level.hpp"
#include "katana/survey/subsurface/utility_network.hpp"
#include "katana/survey/subsurface/verification.hpp"

using namespace katana::survey;
using namespace katana::survey::subsurface;
using katana::core::ErrorCode;

namespace {

PositionEvidence evidence(LocationMethod method, std::optional<double> horizontal,
                          std::optional<double> vertical = std::nullopt, bool hasLevel = false)
{
    PositionEvidence out;
    out.method = method;
    out.horizontalUncertainty = horizontal;
    out.verticalUncertainty = vertical;
    out.hasLevel = hasLevel;
    return out;
}

UtilityVertex vertex(std::string id, double northing, double easting, PositionEvidence located,
                     std::optional<double> level = std::nullopt)
{
    UtilityVertex out;
    out.id = std::move(id);
    out.position = {northing, easting};
    out.evidence = located;
    out.level = level;
    return out;
}

// A detection good to 100 mm in plan, and a pothole good to 20 mm in 3D.
const PositionEvidence kEml = evidence(LocationMethod::ElectromagneticLocation, 0.10);
const PositionEvidence kPothole =
    evidence(LocationMethod::NonDestructiveExcavation, 0.02, 0.02, true);

} // namespace

// ---- quality levels ------------------------------------------------------------

TEST(SubsurfaceQualityLevel, ParsesTheSpellingsDeliverablesUse)
{
    EXPECT_EQ(parseQualityLevel("QL-A"), QualityLevel::A);
    EXPECT_EQ(parseQualityLevel(" qlb "), QualityLevel::B);
    EXPECT_EQ(parseQualityLevel("QL C"), QualityLevel::C);
    EXPECT_EQ(parseQualityLevel("d"), QualityLevel::D);
    EXPECT_EQ(parseQualityLevel("QL_D"), QualityLevel::D);
    EXPECT_FALSE(parseQualityLevel("E"));
    EXPECT_FALSE(parseQualityLevel(""));
    EXPECT_FALSE(parseQualityLevel("QL-AB"));
    EXPECT_STREQ(toString(QualityLevel::B), "QL-B");
}

TEST(SubsurfaceQualityLevel, ABetterLevelComparesGreater)
{
    EXPECT_GT(QualityLevel::A, QualityLevel::B);
    EXPECT_GT(QualityLevel::B, QualityLevel::C);
    EXPECT_GT(QualityLevel::C, QualityLevel::D);
    EXPECT_EQ(std::min(QualityLevel::A, QualityLevel::C), QualityLevel::C);
}

TEST(SubsurfaceQualityLevel, ParsesLocationMethodsByNameAndAbbreviation)
{
    EXPECT_EQ(parseLocationMethod("EML"), LocationMethod::ElectromagneticLocation);
    EXPECT_EQ(parseLocationMethod("gpr"), LocationMethod::GroundPenetratingRadar);
    EXPECT_EQ(parseLocationMethod("Non-Destructive Excavation"),
              LocationMethod::NonDestructiveExcavation);
    EXPECT_EQ(parseLocationMethod("pothole"), LocationMethod::NonDestructiveExcavation);
    EXPECT_EQ(parseLocationMethod("Surface Feature"), LocationMethod::SurfaceFeature);
    EXPECT_FALSE(parseLocationMethod("dowsing"));
}

TEST(SubsurfaceQualityLevel, TheMethodCapsTheLevel)
{
    // No uncertainty, however small, lifts records or a radar pick.
    EXPECT_EQ(classify(evidence(LocationMethod::Records, 0.001)).level, QualityLevel::D);
    EXPECT_EQ(classify(evidence(LocationMethod::SurfaceFeature, 0.001)).level, QualityLevel::C);
    EXPECT_EQ(classify(evidence(LocationMethod::GroundPenetratingRadar, 0.001, 0.001, true)).level,
              QualityLevel::B);
}

TEST(SubsurfaceQualityLevel, ADetectionWithinThreeHundredMillimetresIsQlB)
{
    const Classification plan = classify(kEml);
    EXPECT_EQ(plan.level, QualityLevel::B);
    EXPECT_FALSE(plan.levelQualified); // no level recorded
    EXPECT_TRUE(plan.reasons.empty());

    const Classification withDepth =
        classify(evidence(LocationMethod::ElectromagneticLocation, 0.10, 0.40, true));
    EXPECT_EQ(withDepth.level, QualityLevel::B);
    EXPECT_TRUE(withDepth.levelQualified);
}

TEST(SubsurfaceQualityLevel, ADepthOutsideToleranceKeepsThePlanPositionAndDisqualifiesTheLevel)
{
    const Classification result =
        classify(evidence(LocationMethod::ElectromagneticLocation, 0.10, 0.60, true));
    EXPECT_EQ(result.level, QualityLevel::B);
    EXPECT_FALSE(result.levelQualified);
    ASSERT_EQ(result.reasons.size(), 1u);
    EXPECT_NE(result.reasons[0].find("600 mm"), std::string::npos) << result.reasons[0];
}

TEST(SubsurfaceQualityLevel, ADetectionOutsideOrWithoutAnUncertaintyFallsToQlC)
{
    EXPECT_EQ(classify(evidence(LocationMethod::ElectromagneticLocation, 0.35)).level,
              QualityLevel::C);
    EXPECT_EQ(classify(evidence(LocationMethod::ElectromagneticLocation, std::nullopt)).level,
              QualityLevel::C);
    EXPECT_EQ(classify(evidence(LocationMethod::ElectromagneticLocation, -0.1)).level,
              QualityLevel::C);
    EXPECT_EQ(classify(evidence(LocationMethod::ElectromagneticLocation,
                                std::numeric_limits<double>::quiet_NaN()))
                  .level,
              QualityLevel::C);
}

TEST(SubsurfaceQualityLevel, QlANeedsAThreeDimensionalPositionWithinFiftyMillimetres)
{
    const Classification exposed = classify(kPothole);
    EXPECT_EQ(exposed.level, QualityLevel::A);
    EXPECT_TRUE(exposed.levelQualified);
    EXPECT_TRUE(exposed.reasons.empty());

    // Exposed, but no level: a plan position, so QL-B.
    const Classification noLevel =
        classify(evidence(LocationMethod::NonDestructiveExcavation, 0.02));
    EXPECT_EQ(noLevel.level, QualityLevel::B);
    ASSERT_FALSE(noLevel.reasons.empty());
    EXPECT_NE(noLevel.reasons[0].find("needs a level"), std::string::npos);

    EXPECT_EQ(classify(evidence(LocationMethod::OpenExcavation, 0.06, 0.02, true)).level,
              QualityLevel::B);
    EXPECT_EQ(classify(evidence(LocationMethod::OpenExcavation, 0.02, 0.06, true)).level,
              QualityLevel::B);
}

TEST(SubsurfaceQualityLevel, TolerancesAreTheCallersToTighten)
{
    QualityLevelTolerances tight;
    tight.b = {0.150, 0.300};
    EXPECT_EQ(classify(kEml, tight).level, QualityLevel::B);
    EXPECT_EQ(classify(evidence(LocationMethod::ElectromagneticLocation, 0.20), tight).level,
              QualityLevel::C);
}

// ---- lines and segments --------------------------------------------------------

namespace {

// Along the easting axis, northing 0:
//   P1 e=0  EML      P2 e=8  EML      P3 e=20 pothole, level      P4 e=25 pothole, level
// Segments 8 m, 12 m, 5 m.
UtilityLine sampleLine()
{
    UtilityLine line;
    line.id = "W1";
    line.attributes.type = UtilityType::Water;
    line.attributes.diameter = 0.2;
    line.vertices = {vertex("P1", 0, 0, kEml), vertex("P2", 0, 8, kEml),
                     vertex("P3", 0, 20, kPothole, 9.0), vertex("P4", 0, 25, kPothole, 9.1)};
    return line;
}

} // namespace

TEST(SubsurfaceUtilityLine, ASegmentIsNoBetterThanItsWeakerEndOrItsPath)
{
    const auto graded = gradeLine(sampleLine());
    ASSERT_TRUE(graded.ok()) << graded.error().describe();
    ASSERT_EQ(graded->segments.size(), 3u);

    EXPECT_EQ(graded->segments[0].level, QualityLevel::B);
    EXPECT_DOUBLE_EQ(graded->segments[0].length, 8.0);
    // 12 m is past the default 10 m detected spacing: interpolated, not traced.
    EXPECT_EQ(graded->segments[1].level, QualityLevel::C);
    EXPECT_NE(graded->segments[1].limitedBy.find("maximum detected spacing"), std::string::npos);
    // Between two potholes, but nobody saw the service between them.
    EXPECT_EQ(graded->segments[2].level, QualityLevel::B);
    EXPECT_NE(graded->segments[2].limitedBy.find("not exposed"), std::string::npos);

    EXPECT_DOUBLE_EQ(graded->lengthAt[static_cast<int>(QualityLevel::A)], 0.0);
    EXPECT_DOUBLE_EQ(graded->lengthAt[static_cast<int>(QualityLevel::B)], 13.0);
    EXPECT_DOUBLE_EQ(graded->lengthAt[static_cast<int>(QualityLevel::C)], 12.0);
    EXPECT_DOUBLE_EQ(graded->length(), 25.0);
}

TEST(SubsurfaceUtilityLine, OnlyAnExposedPathKeepsQlA)
{
    UtilityLine line = sampleLine();
    line.pathEvidence = {PathEvidence::Detected, PathEvidence::Assumed, PathEvidence::Exposed};
    const auto graded = gradeLine(line);
    ASSERT_TRUE(graded.ok());
    EXPECT_EQ(graded->segments[1].level, QualityLevel::C);
    EXPECT_NE(graded->segments[1].limitedBy.find("assumed"), std::string::npos);
    EXPECT_EQ(graded->segments[2].level, QualityLevel::A);
    EXPECT_DOUBLE_EQ(graded->lengthAt[static_cast<int>(QualityLevel::A)], 5.0);
}

TEST(SubsurfaceUtilityLine, TheDetectedSpacingIsTheProjectsToSet)
{
    GradingSettings settings;
    settings.maximumDetectedSpacing = std::numeric_limits<double>::infinity();
    const auto graded = gradeLine(sampleLine(), settings);
    ASSERT_TRUE(graded.ok());
    EXPECT_EQ(graded->segments[1].level, QualityLevel::B);
}

TEST(SubsurfaceUtilityLine, AClaimBetterThanTheEvidenceIsReported)
{
    UtilityLine line = sampleLine();
    line.vertices[0].claimed = QualityLevel::A;
    line.vertices[2].claimed = QualityLevel::A;
    const auto graded = gradeLine(line);
    ASSERT_TRUE(graded.ok());
    EXPECT_EQ(graded->vertices[0].overClaim,
              "claimed QL-A, the electromagnetic location evidence supports QL-B");
    EXPECT_TRUE(graded->vertices[2].overClaim.empty());
}

TEST(SubsurfaceUtilityLine, RefusesALineItCannotGrade)
{
    UtilityLine line = sampleLine();
    line.vertices.resize(1);
    EXPECT_EQ(gradeLine(line).error().code, ErrorCode::InvalidArgument);

    line = sampleLine();
    line.pathEvidence = {PathEvidence::Detected};
    EXPECT_EQ(gradeLine(line).error().code, ErrorCode::InvalidArgument);

    line = sampleLine();
    line.vertices[1].position.easting = std::numeric_limits<double>::infinity();
    EXPECT_EQ(gradeLine(line).error().code, ErrorCode::InvalidArgument);
}

TEST(SubsurfaceUtilityLine, TheTopOfTheServiceFromAnyReference)
{
    UtilityVertex point = vertex("P", 0, 0, kPothole, 10.0);
    EXPECT_EQ(topLevel(point, 0.3), 10.0);
    point.levelReference = LevelReference::Centre;
    EXPECT_DOUBLE_EQ(*topLevel(point, 0.3), 10.15);
    EXPECT_FALSE(topLevel(point, 0.0)); // a centre level needs the diameter
    point.levelReference = LevelReference::Invert;
    EXPECT_DOUBLE_EQ(*topLevel(point, 0.3), 10.3);
    point.level.reset();
    EXPECT_FALSE(topLevel(point, 0.3));
}

TEST(SubsurfaceUtilityLine, DepthOfCoverIsSurfaceMinusTopOfService)
{
    UtilityLine line = sampleLine();
    // P3: centre level 9.0 on a 200 mm service -> top 9.1; surface 10.0 -> 0.9 cover.
    line.vertices[2].levelReference = LevelReference::Centre;
    line.vertices[2].surfaceLevel = 10.0;
    // P4: top 9.1, surface 9.6 -> 0.5 cover, below a 0.6 minimum.
    line.vertices[3].surfaceLevel = 9.6;
    // P2: a depth estimate outside QL-B's vertical tolerance.
    line.vertices[1].level = 9.4;
    line.vertices[1].evidence.verticalUncertainty = 0.6;
    line.vertices[1].surfaceLevel = 10.2;

    const auto cover = depthOfCover(line, 0.6);
    ASSERT_TRUE(cover.ok()) << cover.error().describe();
    ASSERT_EQ(cover->size(), 4u);
    EXPECT_FALSE((*cover)[0].cover);
    EXPECT_EQ((*cover)[0].note, "no level or depth of the service");
    ASSERT_TRUE((*cover)[1].cover);
    EXPECT_NEAR(*(*cover)[1].cover, 0.8, 1e-12);
    EXPECT_NE((*cover)[1].note.find("do not rely"), std::string::npos);
    EXPECT_NEAR(*(*cover)[2].cover, 0.9, 1e-12);
    EXPECT_FALSE((*cover)[2].belowMinimum);
    EXPECT_NEAR(*(*cover)[3].cover, 0.5, 1e-12);
    EXPECT_TRUE((*cover)[3].belowMinimum);
}

// ---- clearance -----------------------------------------------------------------

namespace {

// A 200 mm service along the northing axis from (0, 0) to (8, 0).
UtilityLine straightService(PositionEvidence located)
{
    UtilityLine line;
    line.id = "S1";
    line.attributes.diameter = 0.2;
    line.vertices = {vertex("A", 0, 0, located), vertex("B", 8, 0, located)};
    return line;
}

// Works parallel to it, at easting `offset`, 200 mm wide.
DesignAlignment parallelAt(double offset)
{
    DesignAlignment design;
    design.id = "D";
    design.halfWidth = 0.1;
    design.vertices = {{{-5, offset}, std::nullopt}, {{25, offset}, std::nullopt}};
    return design;
}

ClearanceResult only(const katana::core::Result<std::vector<ClearanceResult>>& results)
{
    EXPECT_TRUE(results.ok());
    EXPECT_EQ(results->size(), 1u);
    return results->front();
}

} // namespace

TEST(SubsurfaceClearance, AQlBServiceIsWidenedByItsTolerance)
{
    // Face to face: offset - 0.1 - 0.1. Required 0.3, tolerance 0.3.
    const std::vector<UtilityLine> services{straightService(kEml)};

    const ClearanceResult far = only(checkClearance(parallelAt(1.0), services));
    EXPECT_NEAR(far.horizontalGap, 0.8, 1e-12);
    EXPECT_EQ(far.status, ClearanceStatus::Clear); // 0.8 - 0.3 >= 0.3

    const ClearanceResult near = only(checkClearance(parallelAt(0.6), services));
    EXPECT_NEAR(near.horizontalGap, 0.4, 1e-12);
    EXPECT_EQ(near.status, ClearanceStatus::WithinTolerance); // 0.4 >= 0.3, 0.1 < 0.3

    const ClearanceResult tooNear = only(checkClearance(parallelAt(0.45), services));
    EXPECT_EQ(tooNear.status, ClearanceStatus::Conflict);
    EXPECT_EQ(tooNear.level, QualityLevel::B);
}

TEST(SubsurfaceClearance, AQlCServiceNearTheWorksIsUnconfirmedNotClear)
{
    const std::vector<UtilityLine> services{
        straightService(evidence(LocationMethod::SurfaceFeature, std::nullopt))};
    const ClearanceResult near = only(checkClearance(parallelAt(1.2), services));
    EXPECT_EQ(near.level, QualityLevel::C);
    EXPECT_EQ(near.status, ClearanceStatus::Unconfirmed);
    EXPECT_FALSE(near.horizontalTolerance);
    EXPECT_NE(near.note.find("QL-B or QL-A"), std::string::npos);

    // 5.0 - 0.2 = 4.8 >= 0.3 + 2.0.
    EXPECT_EQ(only(checkClearance(parallelAt(5.0), services)).status, ClearanceStatus::Clear);
}

TEST(SubsurfaceClearance, ACrossingIsClearedByVerticalSeparation)
{
    // Two potholes, top of service 9.0 (centre 8.9), exposed between them.
    UtilityLine exposed;
    exposed.id = "X";
    exposed.attributes.diameter = 0.2;
    exposed.vertices = {vertex("A", 0, 0, kPothole, 9.0), vertex("B", 8, 0, kPothole, 9.0)};
    exposed.pathEvidence = {PathEvidence::Exposed};

    // Works crossing at northing 4, centre level 9.5, 200 mm wide: face to
    // face |9.5 - 8.9| - 0.1 - 0.1 = 0.4.
    DesignAlignment crossing;
    crossing.halfWidth = 0.1;
    crossing.vertices = {{{4, -5}, 9.5}, {{4, 5}, 9.5}};

    const ClearanceResult atA = only(checkClearance(crossing, {exposed}));
    EXPECT_NEAR(atA.planDistance, 0.0, 1e-12);
    ASSERT_TRUE(atA.verticalGap);
    EXPECT_NEAR(*atA.verticalGap, 0.4, 1e-12);
    EXPECT_EQ(atA.status, ClearanceStatus::Clear); // 0.4 - 0.05 >= 0.3
    EXPECT_EQ(atA.note, "cleared by vertical separation");

    // The same crossing where the path was only detected is QL-B: 0.4 - 0.5 < 0.3.
    exposed.pathEvidence = {PathEvidence::Detected};
    const ClearanceResult detected = only(checkClearance(crossing, {exposed}));
    EXPECT_EQ(detected.status, ClearanceStatus::WithinTolerance);
    EXPECT_NE(detected.note.find("not across the tolerance"), std::string::npos);
}

TEST(SubsurfaceClearance, RefusesADesignItCannotMeasure)
{
    DesignAlignment design = parallelAt(1.0);
    design.vertices.resize(1);
    EXPECT_EQ(checkClearance(design, {straightService(kEml)}).error().code,
              ErrorCode::InvalidArgument);
    design = parallelAt(1.0);
    design.halfWidth = -1.0;
    EXPECT_EQ(checkClearance(design, {straightService(kEml)}).error().code,
              ErrorCode::InvalidArgument);
}

// ---- verification --------------------------------------------------------------

TEST(SubsurfaceVerification, ExposuresCheckTheDetectionsTheyName)
{
    const PositionEvidence depthEml =
        evidence(LocationMethod::ElectromagneticLocation, 0.1, 0.3, true);
    UtilityLine line;
    line.id = "G1";
    line.vertices = {vertex("D1", 0, 0, depthEml, 9.0),     vertex("D2", 0, 10, kEml),
                     vertex("X1", 0.2, 0.1, kPothole, 9.3), vertex("X2", 0.4, 10, kPothole, 9.0),
                     vertex("X3", 1, 1, kPothole, 9.0),     vertex("X4", 1, 1, kEml)};
    line.vertices[2].verifies = "D1"; // dH = hypot(0.2, 0.1), dV = +0.3: both pass
    line.vertices[3].verifies = "D2"; // 0.4 m in plan: fails; D2 has no level
    line.vertices[4].verifies = "D9"; // names nothing
    line.vertices[5].verifies = "D1"; // not an exposure

    const VerificationReport report = verifyDetections({line});
    ASSERT_EQ(report.results.size(), 2u);
    EXPECT_NEAR(report.results[0].horizontalDeviation, std::hypot(0.2, 0.1), 1e-12);
    EXPECT_TRUE(report.results[0].horizontalWithin);
    ASSERT_TRUE(report.results[0].verticalDeviation);
    EXPECT_NEAR(*report.results[0].verticalDeviation, 0.3, 1e-12);
    EXPECT_EQ(report.results[0].verticalWithin, true);
    EXPECT_FALSE(report.results[1].horizontalWithin);
    EXPECT_FALSE(report.results[1].verticalDeviation);

    EXPECT_EQ(report.horizontalPassed, 1u);
    EXPECT_EQ(report.verticalChecked, 1u);
    EXPECT_EQ(report.verticalPassed, 1u);
    EXPECT_NEAR(report.maximumHorizontal, 0.4, 1e-12);
    EXPECT_NEAR(report.rmsHorizontal, std::sqrt((0.05 + 0.16) / 2.0), 1e-12);

    ASSERT_EQ(report.problems.size(), 2u);
    EXPECT_NE(report.problems[0].find("no line has"), std::string::npos);
    EXPECT_NE(report.problems[1].find("not QL-A"), std::string::npos);
}
