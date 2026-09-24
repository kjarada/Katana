// The reduction and adjustment options as a surveyor sets them: the
// defaults they start from, the advanced section folded until opened, a
// job's settings round-tripped through the fields bit for bit, numbers typed
// in seconds and millimetres stored in radians and metres, and control
// picked from the file's and the drawing's points.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QTableWidget>
#include <QToolButton>

#include <numbers>

#include "katana/survey/reduction_settings.hpp"
#include "survey/reduction_options_widget.hpp"

namespace survey = katana::survey;
using katana::qt::ReductionOptionsWidget;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

QString text(QWidget& parent, const char* name)
{
    auto* field = child<QLineEdit>(parent, name);
    return field == nullptr ? QString() : field->text();
}

void type(QWidget& parent, const char* name, const QString& value)
{
    auto* field = child<QLineEdit>(parent, name);
    ASSERT_NE(field, nullptr);
    field->setText(value);
}

void choose(QWidget& parent, const char* name, const QString& item)
{
    auto* box = child<QComboBox>(parent, name);
    ASSERT_NE(box, nullptr);
    const int index = box->findText(item);
    ASSERT_GE(index, 0) << name << " has no " << item.toStdString();
    box->setCurrentIndex(index);
}

survey::SurveyPoint point(std::string id, double northing, double easting,
                          std::optional<double> elevation)
{
    survey::SurveyPoint p;
    p.id = std::move(id);
    p.northing = northing;
    p.easting = easting;
    p.elevation = elevation;
    return p;
}

survey::SurveyStation stationWith(std::string id, survey::CorrectionState ppmState,
                                  std::optional<double> ppm)
{
    survey::SurveyStation station;
    station.setup.id = id;
    station.setup.pointId = id;
    station.instrument.atmosphericPpmState = ppmState;
    station.instrument.atmosphericPpm = ppm;
    return station;
}

} // namespace

TEST(ReductionOptions, ANewWidgetGivesTheReductionDefaultsWithTheAdvancedSectionFolded)
{
    ReductionOptionsWidget widget(nullptr);
    widget.show();
    const auto settings = widget.settings();
    ASSERT_TRUE(settings.ok()) << settings.error().describe();
    EXPECT_EQ(*settings, survey::ReductionSettings{});
    EXPECT_FALSE(widget.advancedShown());
    auto* advanced = child<QWidget>(widget, "advancedOptions");
    ASSERT_NE(advanced, nullptr);
    EXPECT_FALSE(advanced->isVisible());
    // The basic choices are on show.
    EXPECT_TRUE(child<QComboBox>(widget, "method")->isVisible());
    EXPECT_TRUE(child<QComboBox>(widget, "atmospheric")->isVisible());
    EXPECT_TRUE(child<QPushButton>(widget, "addControl")->isVisible());
}

TEST(ReductionOptions, TheDefaultsReadInSecondsMillimetresAndPercent)
{
    ReductionOptionsWidget widget(nullptr);
    // FaceTolerances: 10" and 20" (in radians 10 * pi / 648000), 5 mm;
    // ObservationPrecision: 3", 2 mm + 2 ppm, 1 mm centring, 2 mm heights,
    // 1 mm per sqrt km, GNSS 10 mm and 20 mm; confidence 0.95 is 95 %.
    EXPECT_EQ(text(widget, "toleranceHorizontal"), "10");
    EXPECT_EQ(text(widget, "toleranceZenith"), "20");
    EXPECT_EQ(text(widget, "toleranceDistance"), "5");
    EXPECT_EQ(text(widget, "sigmaDirection"), "3");
    EXPECT_EQ(text(widget, "sigmaDistance"), "2");
    EXPECT_EQ(text(widget, "sigmaPpm"), "2");
    EXPECT_EQ(text(widget, "sigmaInstrumentCentring"), "1");
    EXPECT_EQ(text(widget, "sigmaHeight"), "2");
    EXPECT_EQ(text(widget, "sigmaLevelling"), "1");
    EXPECT_EQ(text(widget, "sigmaGnssHorizontal"), "10");
    EXPECT_EQ(text(widget, "sigmaGnssVertical"), "20");
    EXPECT_EQ(text(widget, "refractionK"), "0.13");
    EXPECT_EQ(text(widget, "earthRadius"), "6371000");
    EXPECT_EQ(text(widget, "confidence"), "95");
    EXPECT_EQ(text(widget, "outlierSignificance"), "0.001");
    EXPECT_EQ(child<QSpinBox>(widget, "iterations")->value(), 25);
}

TEST(ReductionOptions, OpeningAdvancedShowsItsFieldsAndClosingItFoldsThemAgain)
{
    ReductionOptionsWidget widget(nullptr);
    widget.show();
    auto* fold = child<QToolButton>(widget, "advanced");
    ASSERT_NE(fold, nullptr);
    fold->click();
    EXPECT_TRUE(widget.advancedShown());
    for (const char* name : {"toleranceHorizontal", "sigmaDirection", "refractionK", "fixedPpm",
                             "gridScaleFactor", "confidence", "outlierSignificance"}) {
        EXPECT_TRUE(child<QWidget>(widget, name)->isVisible()) << name;
    }
    EXPECT_TRUE(child<QComboBox>(widget, "heightReduction")->isVisible());
    EXPECT_TRUE(child<QCheckBox>(widget, "autoReject")->isVisible());
    fold->click();
    EXPECT_FALSE(widget.advancedShown());
    EXPECT_FALSE(child<QWidget>(widget, "toleranceHorizontal")->isVisible());
}

TEST(ReductionOptions, AJobsSettingsComeBackFromTheFieldsExactlyAsTheyWent)
{
    // Every setting away from its default, with numbers that no field shows
    // exactly (7.3" is 3.5391...e-05 rad), so an untouched field must give its
    // setting's own double back, not its rounded text read again.
    survey::ReductionSettings settings;
    settings.atmospheric = survey::AtmosphericCorrection::Fixed;
    settings.fixedPpm = 12.345678901234;
    settings.prismConstantPolicy = survey::PrismConstantPolicy::Override;
    settings.prismConstant = -0.0344;
    settings.faces = survey::FaceHandling::Separate;
    settings.faceTolerances.horizontal = 7.3 * std::numbers::pi / 648000.0;
    settings.faceTolerances.zenith = 1.0e-4 / 3.0;
    settings.faceTolerances.distance = 0.0071;
    settings.faceTolerances.excludeOutside = true;
    settings.curvatureAndRefraction = false;
    settings.refractionCoefficient = 0.142;
    settings.earthRadius = 6'378'137.0;
    settings.slopeToHorizontal = false;
    settings.heightReduction = survey::HeightReduction::Geoid;
    settings.gridScale = survey::GridScale::Fixed;
    settings.fixedGridScaleFactor = 0.99960123456789;
    settings.useCombinedFactor = true;
    settings.combinedFactor = 1.0000123;
    settings.method = survey::AdjustmentMethod::Network;
    settings.traverseRule = survey::TraverseRule::Transit;
    settings.networkDimension = survey::NetworkDimension::HorizontalAndLevels;
    settings.apriori.direction = 1.0 / 3.0 * 1e-5;
    settings.apriori.zenith = 2.2e-5;
    settings.apriori.distanceConstant = 0.0015;
    settings.apriori.distancePpm = 1.5;
    settings.apriori.instrumentCentring = 0.0007;
    settings.apriori.targetCentring = 0.0009;
    settings.apriori.heightMeasurement = 0.0013;
    settings.apriori.levellingPerSqrtKilometre = 0.0025;
    settings.apriori.gnssHorizontal = 0.008;
    settings.apriori.gnssVertical = 0.015;
    settings.useFileCovariances = false;
    survey::ControlSelection fixed{survey::ControlPoint::fixed3d("CP1"),
                                   survey::ControlOrigin::File};
    survey::ControlSelection weighted{survey::ControlPoint::weightedHorizontal("CP;2", 0.004, 0.006),
                                      survey::ControlOrigin::Drawing};
    settings.control = {fixed, weighted};
    settings.confidenceLevel = 0.99;
    settings.outlierTest = survey::OutlierTest::Tau;
    settings.outlierSignificance = 0.0025;
    settings.autoRejectOutliers = true;
    settings.maxIterations = 40;
    ASSERT_TRUE(survey::validateReductionSettings(settings).ok());

    ReductionOptionsWidget widget(nullptr);
    widget.setSettings(settings);
    const auto back = widget.settings();
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(*back, settings);
    // And through the text form a job stores them in, as a save would.
    const auto stored = survey::parseReductionSettings(survey::serialiseReductionSettings(*back));
    ASSERT_TRUE(stored.ok());
    EXPECT_EQ(*stored, settings);
}

TEST(ReductionOptions, NumbersTypedInSecondsMillimetresAndPercentAreStoredInRadiansMetresAndAFraction)
{
    ReductionOptionsWidget widget(nullptr);
    type(widget, "toleranceHorizontal", "5");
    type(widget, "sigmaDistance", "3");
    type(widget, "prismConstantValue", "-34.4");
    type(widget, "confidence", "99");
    const auto settings = widget.settings();
    ASSERT_TRUE(settings.ok()) << settings.error().describe();
    // 5" = 5 / (648000 / pi) rad; 3 mm = 0.003 m; -34.4 mm = -0.0344 m;
    // 99 % = 0.99 - each a division, as the widget does it.
    EXPECT_DOUBLE_EQ(settings->faceTolerances.horizontal, 5.0 * std::numbers::pi / 648000.0);
    EXPECT_EQ(settings->apriori.distanceConstant, 3.0 / 1000.0);
    EXPECT_EQ(settings->prismConstant, -34.4 / 1000.0);
    EXPECT_EQ(settings->confidenceLevel, 0.99);
}

TEST(ReductionOptions, ANumberThatDoesNotReadIsRefusedNamingItsField)
{
    ReductionOptionsWidget widget(nullptr);
    type(widget, "sigmaZenith", "three");
    const auto settings = widget.settings();
    ASSERT_FALSE(settings.ok());
    EXPECT_EQ(settings.error().code, katana::core::ErrorCode::ParseFailure);
    EXPECT_NE(settings.error().message.find("the zenith standard deviation"), std::string::npos)
        << settings.error().message;
}

TEST(ReductionOptions, ControlIsPickedFromTheFileOrTheDrawingAndHeldFixedOrWeighted)
{
    survey::SurveyProject raw;
    raw.points = {point("A", 1000.0, 1000.0, 50.0), point("B", 1100.0, 1000.0, std::nullopt)};
    const survey::SurveyPoint cp = point("CP7", 6'250'000.0, 330'000.0, 12.0);
    ReductionOptionsWidget widget(nullptr);
    widget.setProject(raw);
    widget.setDrawingPoints({cp});

    choose(widget, "controlFrom", "the file");
    choose(widget, "controlPick", "A");
    choose(widget, "controlHorizontal", "fixed");
    choose(widget, "controlVertical", "fixed");
    child<QPushButton>(widget, "addControl")->click();
    choose(widget, "controlFrom", "the drawing");
    choose(widget, "controlPick", "CP7");
    choose(widget, "controlHorizontal", "weighted");
    type(widget, "controlSigmaHorizontal", "5");
    choose(widget, "controlVertical", "free");
    child<QPushButton>(widget, "addControl")->click();

    const auto settings = widget.settings();
    ASSERT_TRUE(settings.ok()) << settings.error().describe();
    ASSERT_EQ(settings->control.size(), 2u);
    EXPECT_EQ(settings->control[0],
              (survey::ControlSelection{survey::ControlPoint::fixed3d("A"),
                                        survey::ControlOrigin::File}));
    // 5 mm is 0.005 m, north and east alike; the elevation left free.
    EXPECT_EQ(settings->control[1],
              (survey::ControlSelection{survey::ControlPoint::weightedHorizontal("CP7", 0.005, 0.005),
                                        survey::ControlOrigin::Drawing}));
    auto* table = child<QTableWidget>(widget, "control");
    ASSERT_EQ(table->rowCount(), 2);
    EXPECT_EQ(table->item(1, 2)->text(), "weighted 5 mm");

    // Chosen in the table and released.
    table->setCurrentCell(0, 0);
    child<QPushButton>(widget, "removeControl")->click();
    const auto after = widget.settings();
    ASSERT_TRUE(after.ok());
    ASSERT_EQ(after->control.size(), 1u);
    EXPECT_EQ(after->control[0].point.pointId, "CP7");
}

TEST(ReductionOptions, WhatTheInstrumentAppliedIsSaidPerSetupWithItsValues)
{
    survey::SurveyProject raw;
    raw.stations = {stationWith("S1", survey::CorrectionState::Applied, 12.4),
                    stationWith("S2", survey::CorrectionState::Applied, 12.4)};
    EXPECT_EQ(katana::qt::atmosphericStateText(raw),
              "applied by the instrument at all 2 setups (+12.4 ppm)");
    raw.stations.push_back(stationWith("S3", survey::CorrectionState::NotApplied, std::nullopt));
    raw.stations.push_back(stationWith("S4", survey::CorrectionState::Unknown, std::nullopt));
    EXPECT_EQ(katana::qt::atmosphericStateText(raw),
              "applied by the instrument at 2 of 4 setups (+12.4 ppm); not applied by the "
              "instrument at 1 of 4 setups; not stated by the file at 1 of 4 setups");
    EXPECT_EQ(katana::qt::atmosphericStateText(survey::SurveyProject{}),
              "the file has no instrument setups");

    ReductionOptionsWidget widget(nullptr);
    widget.setProject(raw);
    auto* state = widget.findChild<QLabel*>("atmosphericState");
    ASSERT_NE(state, nullptr);
    EXPECT_EQ(state->text().toStdString(), katana::qt::atmosphericStateText(raw));
}
