#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/survey/reduction.hpp"
#include "katana/survey/reduction_settings.hpp"

using namespace katana::survey;
using katana::core::ErrorCode;

namespace {

// One second of arc in radians: pi / (180 * 3600) = pi / 648000
// = 3.14159265358979 / 648000 = 4.84813681109536e-06.
constexpr double kArcSecond = 4.84813681109536e-06;

// Every setting moved off its default, and control ids that need escaping in
// the text form (';' separates fields, '=' separates the key, '%' is the
// escape itself, and a line break would end the line).
ReductionSettings everythingChanged()
{
    ReductionSettings settings;
    settings.atmospheric = AtmosphericCorrection::Fixed;
    settings.fixedPpm = -12.3;
    settings.prismConstantPolicy = PrismConstantPolicy::Override;
    settings.prismConstant = -0.0344;
    settings.faces = FaceHandling::Separate;
    settings.faceTolerances = FaceTolerances{7.0 * kArcSecond, 13.0 * kArcSecond, 0.0031};
    settings.curvatureAndRefraction = false;
    settings.refractionCoefficient = 0.142;
    settings.earthRadius = 6378137.0;
    settings.slopeToHorizontal = false;
    settings.heightReduction = HeightReduction::Ellipsoid;
    settings.gridScale = GridScale::FromProjection;
    settings.fixedGridScaleFactor = 0.9996;
    settings.useCombinedFactor = true;
    settings.combinedFactor = 0.99987654321;
    settings.method = AdjustmentMethod::Network;
    settings.traverseRule = TraverseRule::LeastSquares;
    settings.networkDimension = NetworkDimension::HorizontalAndLevels;
    settings.apriori.direction = 1.0 * kArcSecond;
    settings.apriori.zenith = 1.5 * kArcSecond;
    settings.apriori.distanceConstant = 0.001;
    settings.apriori.distancePpm = 1.5;
    settings.apriori.instrumentCentring = 0.0005;
    settings.apriori.targetCentring = 0.0007;
    settings.apriori.heightMeasurement = 0.003;
    settings.apriori.levellingPerSqrtKilometre = 0.0003;
    settings.apriori.gnssHorizontal = 0.008;
    settings.apriori.gnssVertical = 0.015;
    settings.useFileCovariances = false;
    ControlSelection first{ControlPoint::fixed3d("CP;1=a%b"), ControlOrigin::Drawing};
    ControlSelection second{ControlPoint::weightedHorizontal("line\nbreak", 0.005, 0.006),
                            ControlOrigin::File};
    second.point.elevation = ControlComponent{ControlConstraint::Weighted, 0.01};
    settings.control = {first, second};
    settings.confidenceLevel = 0.99;
    settings.outlierTest = OutlierTest::Tau;
    settings.outlierSignificance = 0.0005;
    settings.autoRejectOutliers = true;
    settings.maxIterations = 40;
    return settings;
}

} // namespace

TEST(ReductionSettings, DefaultsReduceLikeTheInstrumentAndAdjustNothing)
{
    const ReductionSettings settings;
    EXPECT_EQ(settings.atmospheric, AtmosphericCorrection::Auto);
    EXPECT_EQ(settings.prismConstantPolicy, PrismConstantPolicy::Auto);
    EXPECT_EQ(settings.faces, FaceHandling::Average);
    EXPECT_TRUE(settings.curvatureAndRefraction);
    EXPECT_EQ(settings.refractionCoefficient, 0.13);
    EXPECT_EQ(settings.earthRadius, 6371000.0);
    EXPECT_TRUE(settings.slopeToHorizontal);
    EXPECT_EQ(settings.heightReduction, HeightReduction::None);
    EXPECT_EQ(settings.gridScale, GridScale::None);
    EXPECT_FALSE(settings.useCombinedFactor);
    EXPECT_EQ(settings.method, AdjustmentMethod::None);
    EXPECT_TRUE(settings.control.empty());
    EXPECT_EQ(settings.confidenceLevel, 0.95);
    EXPECT_EQ(settings.outlierTest, OutlierTest::Baarda);
    EXPECT_EQ(settings.outlierSignificance, 0.001);
    EXPECT_FALSE(settings.autoRejectOutliers);
    EXPECT_EQ(settings.maxIterations, 25U);
    EXPECT_TRUE(validateReductionSettings(settings).ok());
}

TEST(ReductionSettings, DefaultPrecisionIsThatOfAThreeSecondInstrumentWithATwoPlusTwoEdm)
{
    const ObservationPrecision precision;
    // 3" = 3 * 4.84813681109536e-06 = 1.454441043328608e-05 rad.
    EXPECT_NEAR(precision.direction, 1.454441043328608e-05, 1e-18);
    EXPECT_NEAR(precision.zenith, 1.454441043328608e-05, 1e-18);
    EXPECT_EQ(precision.distanceConstant, 0.002);
    EXPECT_EQ(precision.distancePpm, 2.0);
    // Face tolerances: 10" = 4.84813681109536e-05 rad horizontal, 20" zenith.
    const FaceTolerances tolerances;
    EXPECT_NEAR(tolerances.horizontal, 4.84813681109536e-05, 1e-17);
    EXPECT_NEAR(tolerances.zenith, 9.69627362219072e-05, 1e-17);
    EXPECT_EQ(tolerances.distance, 0.005);
}

TEST(ReductionSettings, TheDistanceSigmaCombinesTheConstantAndTheProportionalPart)
{
    // 2 mm + 2 ppm at 1000 m: the ppm part is 2e-6 * 1000 = 0.002 m, so
    // sqrt(0.002^2 + 0.002^2) = 0.002 * sqrt(2) = 0.0028284271247461903.
    EXPECT_NEAR(distanceSigma(ObservationPrecision{}, 1000.0), 0.0028284271247461903, 1e-15);
    // At zero length only the constant is left.
    EXPECT_EQ(distanceSigma(ObservationPrecision{}, 0.0), 0.002);
}

TEST(ReductionSettings, TheDefaultTextStartsWithItsVersionAndWritesShortNumbers)
{
    const std::string text = serialiseReductionSettings(ReductionSettings{});
    EXPECT_EQ(text.rfind("katana-reduction-settings=1\n", 0), 0U);
    // The shortest text that reads back as the double 0.13 is "0.13".
    EXPECT_NE(text.find("\nrefraction.k=0.13\n"), std::string::npos);
    EXPECT_NE(text.find("\natmospheric=auto\n"), std::string::npos);
    EXPECT_NE(text.find("\nadjustment.method=none\n"), std::string::npos);
    EXPECT_EQ(text.find("\ncontrol="), std::string::npos);
}

TEST(ReductionSettings, DefaultsSurviveTheTextRoundTrip)
{
    const auto parsed = parseReductionSettings(serialiseReductionSettings(ReductionSettings{}));
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(*parsed, ReductionSettings{});
}

TEST(ReductionSettings, EverySettingSurvivesTheTextRoundTripBitForBit)
{
    const ReductionSettings original = everythingChanged();
    ASSERT_TRUE(validateReductionSettings(original).ok());
    const std::string text = serialiseReductionSettings(original);
    std::vector<std::string> warnings;
    const auto parsed = parseReductionSettings(text, &warnings);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_TRUE(warnings.empty());
    EXPECT_EQ(*parsed, original);
    // And the text of the text is the same text.
    EXPECT_EQ(serialiseReductionSettings(*parsed), text);
}

TEST(ReductionSettings, EveryChoiceOfEveryEnumSurvivesTheRoundTrip)
{
    for (auto value : {AtmosphericCorrection::None, AtmosphericCorrection::Auto,
                       AtmosphericCorrection::Recompute, AtmosphericCorrection::Fixed}) {
        ReductionSettings settings;
        settings.atmospheric = value;
        EXPECT_EQ(parseReductionSettings(serialiseReductionSettings(settings))->atmospheric, value);
    }
    for (auto value : {TraverseRule::Bowditch, TraverseRule::Transit, TraverseRule::LeastSquares}) {
        ReductionSettings settings;
        settings.traverseRule = value;
        EXPECT_EQ(parseReductionSettings(serialiseReductionSettings(settings))->traverseRule, value);
    }
    for (auto value : {NetworkDimension::Horizontal, NetworkDimension::Levels,
                       NetworkDimension::HorizontalAndLevels}) {
        ReductionSettings settings;
        settings.networkDimension = value;
        EXPECT_EQ(parseReductionSettings(serialiseReductionSettings(settings))->networkDimension,
                  value);
    }
    for (auto value : {FaceHandling::Average, FaceHandling::FaceLeftOnly, FaceHandling::Separate}) {
        ReductionSettings settings;
        settings.faces = value;
        EXPECT_EQ(parseReductionSettings(serialiseReductionSettings(settings))->faces, value);
    }
    for (auto value : {GridScale::None, GridScale::Fixed, GridScale::FromProjection}) {
        ReductionSettings settings;
        settings.gridScale = value;
        EXPECT_EQ(parseReductionSettings(serialiseReductionSettings(settings))->gridScale, value);
    }
    for (auto value : {HeightReduction::None, HeightReduction::Ellipsoid, HeightReduction::Geoid}) {
        ReductionSettings settings;
        settings.heightReduction = value;
        EXPECT_EQ(parseReductionSettings(serialiseReductionSettings(settings))->heightReduction,
                  value);
    }
}

TEST(ReductionSettings, AKeyMissingFromOlderTextKeepsItsDefault)
{
    const auto parsed =
        parseReductionSettings("katana-reduction-settings=1\nadjustment.method=network\n");
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    ReductionSettings expected;
    expected.method = AdjustmentMethod::Network;
    EXPECT_EQ(*parsed, expected);
}

TEST(ReductionSettings, WindowsLineEndsCommentsAndBlankLinesAreAccepted)
{
    const auto parsed = parseReductionSettings(
        "# saved by hand\r\nkatana-reduction-settings=1\r\n\r\nrefraction.k=0.2\r\n");
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed->refractionCoefficient, 0.2);
}

TEST(ReductionSettings, TextFromANewerVersionIsRefusedNotHalfRead)
{
    const auto parsed = parseReductionSettings("katana-reduction-settings=2\nrefraction.k=0.2\n");
    ASSERT_FALSE(parsed.ok());
    EXPECT_EQ(parsed.error().code, ErrorCode::Unsupported);
}

TEST(ReductionSettings, AnUnknownKeyIsReportedAndSkippedNotFatal)
{
    std::vector<std::string> warnings;
    const auto parsed = parseReductionSettings(
        "katana-reduction-settings=1\nsomething.new=7\nrefraction.k=0.2\n", &warnings);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed->refractionCoefficient, 0.2);
    ASSERT_EQ(warnings.size(), 1U);
    EXPECT_NE(warnings.front().find("line 2"), std::string::npos);
    EXPECT_NE(warnings.front().find("something.new"), std::string::npos);
}

TEST(ReductionSettings, AMalformedValueIsAParseFailureNamingItsLine)
{
    const auto number = parseReductionSettings("katana-reduction-settings=1\nrefraction.k=abc\n");
    ASSERT_FALSE(number.ok());
    EXPECT_EQ(number.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(number.error().message.find("line 2"), std::string::npos);

    const auto choice = parseReductionSettings("katana-reduction-settings=1\nfaces=sideways\n");
    ASSERT_FALSE(choice.ok());
    EXPECT_EQ(choice.error().code, ErrorCode::ParseFailure);

    const auto control = parseReductionSettings("katana-reduction-settings=1\ncontrol=A;file\n");
    ASSERT_FALSE(control.ok());
    EXPECT_EQ(control.error().code, ErrorCode::ParseFailure);

    const auto twice = parseReductionSettings(
        "katana-reduction-settings=1\nrefraction.k=0.1\nrefraction.k=0.2\n");
    ASSERT_FALSE(twice.ok());
    EXPECT_EQ(twice.error().code, ErrorCode::ParseFailure);
}

TEST(ReductionSettings, TextThatIsNotReductionSettingsIsRefused)
{
    EXPECT_EQ(parseReductionSettings("").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseReductionSettings("refraction.k=0.13\n").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseReductionSettings("katana-reduction-settings=0\n").error().code,
              ErrorCode::ParseFailure);
}

TEST(ReductionSettings, ValuesThatMakeNoSurveyingSenseAreRefusedByName)
{
    ReductionSettings refraction;
    refraction.refractionCoefficient = 1.5;
    EXPECT_EQ(validateReductionSettings(refraction).error().code, ErrorCode::InvalidArgument);

    ReductionSettings weighted;
    weighted.control = {ControlSelection{ControlPoint::weightedVertical("BM1", 0.0)}};
    const auto status = validateReductionSettings(weighted);
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("BM1"), std::string::npos);

    ReductionSettings twice;
    twice.control = {ControlSelection{ControlPoint::fixedHorizontal("CP1")},
                     ControlSelection{ControlPoint::fixedVertical("CP1")}};
    EXPECT_FALSE(validateReductionSettings(twice).ok());

    ReductionSettings confidence;
    confidence.confidenceLevel = 1.0;
    EXPECT_FALSE(validateReductionSettings(confidence).ok());

    ReductionSettings iterations;
    iterations.maxIterations = 0;
    EXPECT_FALSE(validateReductionSettings(iterations).ok());

    // The same refusal comes back through the text form.
    EXPECT_EQ(parseReductionSettings("katana-reduction-settings=1\nrefraction.k=1.5\n").error().code,
              ErrorCode::InvalidArgument);
}

TEST(ReductionSettings, TheFilesControlIsTheStartingSelection)
{
    SurveyProject project;
    project.controlPoints = {ControlPoint::fixed3d("CP1"), ControlPoint::fixedVertical("BM2")};
    const std::vector<ControlSelection> control = controlFromFile(project);
    ASSERT_EQ(control.size(), 2U);
    EXPECT_EQ(control[0].point, ControlPoint::fixed3d("CP1"));
    EXPECT_EQ(control[0].origin, ControlOrigin::File);
    EXPECT_EQ(control[1].point.pointId, "BM2");
}

TEST(ReductionContract, TheReportAndOutcomeStartEmpty)
{
    const ReductionReport report;
    EXPECT_TRUE(report.observations.empty());
    EXPECT_TRUE(report.adjustments.empty());
    EXPECT_EQ(report.settings, ReductionSettings{});
    const ComputedPoint point;
    EXPECT_EQ(point.method, ComputationMethod::Radiation);
    EXPECT_FALSE(point.elevation.has_value());
    const ReductionContext context;
    EXPECT_FALSE(static_cast<bool>(context.gridScaleFactor));
    EXPECT_FALSE(static_cast<bool>(context.geoidSeparation));
    EXPECT_STREQ(toString(CorrectionKind::CurvatureRefraction), "curvature and refraction");
    EXPECT_STREQ(toString(ComputationMethod::TraverseBowditch), "traverse (Bowditch)");
}
