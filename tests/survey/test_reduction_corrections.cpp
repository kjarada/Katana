// Each correction of the reduction on its own, against a value worked by hand.
//
// The project is one setup on A (N 1000, E 1000, H 500) oriented on B due
// north, reading 0 on B, and one shot to P due east. Everything is off except
// the correction under test, so the report's number for it is the whole story.

#include <gtest/gtest.h>

#include <cmath>
#include <string>

#include "katana/survey/reduction.hpp"
#include "test_reduction_support.hpp"

using namespace katana::survey;
using namespace reduction_test;
using katana::core::ErrorCode;

namespace {

SurveyProject oneShot(double zenith, double slope, double targetHeight = 1.5,
                      InstrumentSettings instrument = {}, TargetInfo target = {})
{
    SurveyProject project;
    project.points.push_back(point("A", 1000.0, 1000.0, 500.0));
    project.points.push_back(point("B", 1100.0, 1000.0, 500.0));
    project.unpositionedPoints.push_back(unpositioned("P"));
    SurveyStation station =
        setup("S1", "A", 1.5, "B",
              {Shot{"B", 1, Face::Left, 0.0, {}, {}},
               Shot{"P", 2, Face::Left, deg(90), zenith, slope, targetHeight}});
    station.instrument = instrument;
    for (Observation& observation : station.observations) {
        if (auto* distance = std::get_if<DistanceObservation>(&observation)) {
            distance->target = target;
        }
    }
    project.stations.push_back(std::move(station));
    return project;
}

const ReportObservation& distanceRow(const ReductionOutcome& outcome)
{
    const ReportObservation* row = findRow(outcome.report, "slope distance", "P");
    EXPECT_NE(row, nullptr);
    return *row;
}

} // namespace

TEST(ReductionCorrections, TheAtmosphericCorrectionAtTwentyDegreesIsEightPpmByTheIugg1999Formula)
{
    // Worked with a calculator from the formula in reduction_formulas.hpp:
    //   N_gr = 287.6155 + 4.88660 / 0.658^2 + 0.06800 / 0.658^4
    //        = 287.6155 + 11.28639 + 0.36275 = 299.26464
    //   (273.15 / 1013.25) N_gr = 80.675189
    //   reference 12 C, 1013.25 hPa, 60 %:
    //     E = 6.1078 * 10^(7.5*12/249.3) = 14.02477 hPa, e = 8.41486
    //     N_ref = 80.675189 * 1013.25 / 285.15 - 11.27 * 8.41486 / 285.15 = 286.33807
    //   20 C, 1013.25 hPa, 60 %:
    //     E = 6.1078 * 10^(7.5*20/257.3) = 23.38094 hPa, e = 14.02856
    //     N_L = 80.675189 * 1013.25 / 293.15 - 11.27 * 14.02856 / 293.15 = 278.30815
    //   ppm = 286.33807 - 278.30815 = +8.02992
    // Leica's published formula (286.338 - 0.29535 p / (1 + t/273.15) +
    // 4.126e-4 h / (1 + t/273.15) 10^(7.5t/(237.3+t) + 0.7857)) gives 8.0308
    // for the same air; the 0.001 ppm between them is the Magnus constant.
    InstrumentSettings instrument;
    instrument.temperatureCelsius = 20.0;
    instrument.pressureHectopascals = 1013.25;
    instrument.relativeHumidityPercent = 60.0;
    instrument.atmosphericPpmState = CorrectionState::NotApplied;
    ReductionSettings settings = bareSettings();
    settings.atmospheric = AtmosphericCorrection::Auto;

    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0, 1.5, instrument), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReportObservation& row = distanceRow(*outcome);
    const AppliedCorrection* atmospheric = findCorrection(row, CorrectionKind::Atmospheric);
    ASSERT_NE(atmospheric, nullptr);
    EXPECT_NEAR(atmospheric->amount, 1000.0 * 8.02992e-6, 1e-8);
    ASSERT_TRUE(atmospheric->factor.has_value());
    EXPECT_NEAR(*atmospheric->factor, 1.0 + 8.02992e-6, 1e-11);
    EXPECT_NEAR(*atmospheric->factor, 1.0 + 8.0308e-6, 0.002e-6); // Leica's formula
    // Horizontal (zenith 90), nothing else on: the reduced distance is the
    // corrected one.
    ASSERT_TRUE(row.reduced.has_value());
    EXPECT_NEAR(*row.reduced, 1000.00802992, 1e-7);
}

TEST(ReductionCorrections, AnAtmosphericCorrectionTheInstrumentAlreadyAppliedIsNotAppliedAgain)
{
    InstrumentSettings instrument;
    instrument.temperatureCelsius = 20.0;
    instrument.pressureHectopascals = 1013.25;
    instrument.atmosphericPpm = 8.0;
    instrument.atmosphericPpmState = CorrectionState::Applied;
    ReductionSettings settings = bareSettings();
    settings.atmospheric = AtmosphericCorrection::Auto;
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0, 1.5, instrument), settings, {});
    ASSERT_TRUE(outcome.ok());
    EXPECT_EQ(findCorrection(distanceRow(*outcome), CorrectionKind::Atmospheric), nullptr);

    // Recompute takes the instrument's 8 ppm out and puts the computed one
    // in: net factor (1 + 8.02992e-6) / (1 + 8e-6) = 1 + 0.02992e-6.
    settings.atmospheric = AtmosphericCorrection::Recompute;
    instrument.relativeHumidityPercent = 60.0;
    const auto recomputed =
        reduceAndAdjust(oneShot(deg(90), 1000.0, 1.5, instrument), settings, {});
    ASSERT_TRUE(recomputed.ok());
    const AppliedCorrection* net =
        findCorrection(distanceRow(*recomputed), CorrectionKind::Atmospheric);
    ASSERT_NE(net, nullptr);
    EXPECT_NEAR(net->amount, 1000.0 * 0.02992e-6, 1e-8);
}

TEST(ReductionCorrections, AnUnstatedAtmosphericStateAppliesNothingAndSaysSo)
{
    InstrumentSettings instrument;
    instrument.temperatureCelsius = 30.0;
    instrument.pressureHectopascals = 950.0;
    ReductionSettings settings = bareSettings();
    settings.atmospheric = AtmosphericCorrection::Auto;
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0, 1.5, instrument), settings, {});
    ASSERT_TRUE(outcome.ok());
    EXPECT_EQ(findCorrection(distanceRow(*outcome), CorrectionKind::Atmospheric), nullptr);
    bool warned = false;
    for (const ReportMessage& warning : outcome->report.warnings) {
        warned = warned || warning.text.find("atmospheric") != std::string::npos;
    }
    EXPECT_TRUE(warned);
}

TEST(ReductionCorrections, APrismConstantTheShotSaysIsMissingIsAddedAsRecorded)
{
    // A Leica round prism on a zero-constant setting: -34.4 mm, not applied.
    TargetInfo target;
    target.prismConstant = -0.0344;
    target.prismConstantState = CorrectionState::NotApplied;
    ReductionSettings settings = bareSettings();
    settings.prismConstantPolicy = PrismConstantPolicy::Auto;
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 100.0, 1.5, {}, target), settings, {});
    ASSERT_TRUE(outcome.ok());
    const ReportObservation& row = distanceRow(*outcome);
    const AppliedCorrection* prism = findCorrection(row, CorrectionKind::PrismConstant);
    ASSERT_NE(prism, nullptr);
    EXPECT_DOUBLE_EQ(prism->amount, -0.0344);
    EXPECT_NEAR(*row.reduced, 99.9656, 1e-12);

    // Override with 0 mm replaces a constant the instrument applied: +34.4 mm.
    target.prismConstantState = CorrectionState::Applied;
    settings.prismConstantPolicy = PrismConstantPolicy::Override;
    settings.prismConstant = 0.0;
    const auto overridden =
        reduceAndAdjust(oneShot(deg(90), 100.0, 1.5, {}, target), settings, {});
    ASSERT_TRUE(overridden.ok());
    const AppliedCorrection* replaced =
        findCorrection(distanceRow(*overridden), CorrectionKind::PrismConstant);
    ASSERT_NE(replaced, nullptr);
    EXPECT_DOUBLE_EQ(replaced->amount, 0.0344);
}

TEST(ReductionCorrections, SlopeToHorizontalWithCurvatureAndRefractionMatchesTheWorkedKilometreSight)
{
    // S = 1000 m at z = 85 deg, k = 0.13, R = 6 371 000 m, HI 1.5, HT 1.8:
    //   X = 1000 cos 85 = 87.155743    Y = 1000 sin 85 = 996.194698
    //   A = (1 - 0.065) / 6371000 = 1.4675875e-7
    //   A X Y = 1.4675875e-7 * 87.155743 * 996.194698 = 0.0127422 m
    //   HD = 996.194698 - 0.012742 = 996.181956
    //   B = 0.87 / 12742000 = 6.8278135e-8, B Y^2 = 0.0677595 m
    //   dH = 87.155743 + 0.067759 + 1.5 - 1.8 = 86.923502
    ReductionSettings settings = bareSettings();
    settings.curvatureAndRefraction = true;
    const auto outcome = reduceAndAdjust(oneShot(deg(85), 1000.0, 1.8), settings, {});
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const ReportObservation& row = distanceRow(*outcome);
    const AppliedCorrection* slope = findCorrection(row, CorrectionKind::SlopeToHorizontal);
    const AppliedCorrection* curvature = findCorrection(row, CorrectionKind::CurvatureRefraction);
    ASSERT_NE(slope, nullptr);
    ASSERT_NE(curvature, nullptr);
    EXPECT_NEAR(slope->amount, 996.194698 - 1000.0, 1e-6);
    EXPECT_NEAR(curvature->amount, -0.0127422, 1e-7);
    EXPECT_NEAR(*row.reduced, 996.181956, 1e-6);

    const ReportObservation* height = findRow(outcome->report, "height difference", "P");
    ASSERT_NE(height, nullptr);
    EXPECT_NEAR(height->raw, 87.155743, 1e-6);
    EXPECT_NEAR(findCorrection(*height, CorrectionKind::CurvatureRefraction)->amount, 0.0677595,
                1e-7);
    EXPECT_NEAR(findCorrection(*height, CorrectionKind::InstrumentAndTargetHeight)->amount, -0.3,
                1e-12);
    EXPECT_NEAR(*height->reduced, 86.923502, 1e-6);
    // P = A + HD east, H = 500 + dH.
    const ComputedPoint* p = findPoint(*outcome, "P");
    ASSERT_NE(p, nullptr);
    EXPECT_NEAR(p->easting, 1000.0 + 996.181956, 1e-6);
    EXPECT_NEAR(p->northing, 1000.0, 1e-9);
    EXPECT_NEAR(*p->elevation, 586.923502, 1e-6);
}

TEST(ReductionCorrections, HeightReductionToTheGeoidUsesTheLinesMeanHeight)
{
    // Level 1000 m sight from H 500 with HI 1.5: the line is at 501.5 m.
    //   factor R / (R + h) = 6371000 / 6371501.5; 1000 (factor - 1) = -0.0787099 m
    ReductionSettings settings = bareSettings();
    settings.heightReduction = HeightReduction::Geoid;
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0), settings, {});
    ASSERT_TRUE(outcome.ok());
    const AppliedCorrection* height =
        findCorrection(distanceRow(*outcome), CorrectionKind::HeightReduction);
    ASSERT_NE(height, nullptr);
    EXPECT_NEAR(height->amount, -0.0787099, 1e-7);
}

TEST(ReductionCorrections, HeightReductionToTheEllipsoidWithoutAGeoidIsRefused)
{
    ReductionSettings settings = bareSettings();
    settings.heightReduction = HeightReduction::Ellipsoid;
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0), settings, {});
    ASSERT_FALSE(outcome.ok());
    EXPECT_EQ(outcome.error().code, ErrorCode::InvalidArgument);
}

TEST(ReductionCorrections, AFixedGridScaleFactorScalesTheHorizontalDistance)
{
    // 1000 m * (0.9996 - 1) = -0.4 m.
    ReductionSettings settings = bareSettings();
    settings.gridScale = GridScale::Fixed;
    settings.fixedGridScaleFactor = 0.9996;
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0), settings, {});
    ASSERT_TRUE(outcome.ok());
    const ReportObservation& row = distanceRow(*outcome);
    EXPECT_NEAR(findCorrection(row, CorrectionKind::GridScale)->amount, -0.4, 1e-12);
    EXPECT_NEAR(*row.reduced, 999.6, 1e-12);
}

TEST(ReductionCorrections, TheProjectionScaleFactorIsTakenAtTheLinesMidPoint)
{
    // A made-up projection whose scale grows 1e-7 per metre east of E 1000:
    // at the station it is exactly 1, at the mid-point (E 1500) 1.00005, so
    // +0.05 m on the 1000 m line proves where it was evaluated.
    ReductionSettings settings = bareSettings();
    settings.gridScale = GridScale::FromProjection;
    ReductionContext context;
    context.gridScaleFactor = [](double, double easting, double) -> std::optional<double> {
        return 1.0 + 1e-7 * (easting - 1000.0);
    };
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0), settings, context);
    ASSERT_TRUE(outcome.ok());
    EXPECT_NEAR(findCorrection(distanceRow(*outcome), CorrectionKind::GridScale)->amount, 0.05,
                1e-9);

    // No projection on the drawing: refused, naming the setting.
    const auto refused = reduceAndAdjust(oneShot(deg(90), 1000.0), settings, {});
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
}

TEST(ReductionCorrections, ACombinedFactorReplacesTheHeightAndGridFactors)
{
    // 1000 m * (0.9999 - 1) = -0.1 m, and no height or grid correction.
    ReductionSettings settings = bareSettings();
    settings.heightReduction = HeightReduction::Geoid;
    settings.gridScale = GridScale::Fixed;
    settings.fixedGridScaleFactor = 0.9996;
    settings.useCombinedFactor = true;
    settings.combinedFactor = 0.9999;
    const auto outcome = reduceAndAdjust(oneShot(deg(90), 1000.0), settings, {});
    ASSERT_TRUE(outcome.ok());
    const ReportObservation& row = distanceRow(*outcome);
    EXPECT_NEAR(findCorrection(row, CorrectionKind::CombinedFactor)->amount, -0.1, 1e-12);
    EXPECT_EQ(findCorrection(row, CorrectionKind::HeightReduction), nullptr);
    EXPECT_EQ(findCorrection(row, CorrectionKind::GridScale), nullptr);
}

TEST(ReductionCorrections, TheCorrectionsAreRecordedInTheStatedOrder)
{
    TargetInfo target;
    target.prismConstant = 0.01;
    target.prismConstantState = CorrectionState::NotApplied;
    InstrumentSettings instrument;
    instrument.atmosphericPpm = 10.0;
    instrument.atmosphericPpmState = CorrectionState::NotApplied;
    ReductionSettings settings;
    settings.heightReduction = HeightReduction::Geoid;
    settings.gridScale = GridScale::Fixed;
    settings.fixedGridScaleFactor = 0.9996;
    const auto outcome =
        reduceAndAdjust(oneShot(deg(80), 500.0, 1.5, instrument, target), settings, {});
    ASSERT_TRUE(outcome.ok());
    const ReportObservation& row = distanceRow(*outcome);
    std::vector<CorrectionKind> kinds;
    for (const AppliedCorrection& correction : row.corrections) {
        kinds.push_back(correction.kind);
    }
    EXPECT_EQ(kinds, (std::vector<CorrectionKind>{
                         CorrectionKind::PrismConstant, CorrectionKind::Atmospheric,
                         CorrectionKind::SlopeToHorizontal, CorrectionKind::CurvatureRefraction,
                         CorrectionKind::HeightReduction, CorrectionKind::GridScale}));
    // The chain adds up: raw + every amount = reduced.
    double value = row.raw;
    for (const AppliedCorrection& correction : row.corrections) {
        value += correction.amount;
    }
    EXPECT_NEAR(value, *row.reduced, 1e-9);
}
