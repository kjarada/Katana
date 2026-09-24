#include "survey/reduction_options_widget.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QScrollBar>
#include <QSpinBox>
#include <QTableWidget>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <format>
#include <map>
#include <numbers>
#include <string_view>
#include <utility>

#include "katana/core/text.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// Seconds of arc in a radian: 648000 / pi.
constexpr double kSecondsPerRadian = 648000.0 / std::numbers::pi;

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

// A number as a person reads it: up to ten significant figures, no trailing
// zeros - 10" shows as "10", not as the 10.000000000000002 its radians give
// back. Never the locale: a field must read back as it was written.
QString shownNumber(double value)
{
    return qs(std::format("{:.10g}", value));
}

template <class Enum>
struct Choice {
    const char* label;
    Enum value;
};

// The labels a person reads - and what a headless --fill chooses by.
constexpr Choice<survey::AdjustmentMethod> kMethods[] = {
    {"radiation (no adjustment)", survey::AdjustmentMethod::None},
    {"traverse", survey::AdjustmentMethod::Traverse},
    {"network least squares", survey::AdjustmentMethod::Network},
};
constexpr Choice<survey::TraverseRule> kTraverseRules[] = {
    {"Bowditch (compass rule)", survey::TraverseRule::Bowditch},
    {"Transit", survey::TraverseRule::Transit},
    {"least squares", survey::TraverseRule::LeastSquares},
};
constexpr Choice<survey::NetworkDimension> kDimensions[] = {
    {"horizontal", survey::NetworkDimension::Horizontal},
    {"levels", survey::NetworkDimension::Levels},
    {"horizontal and levels", survey::NetworkDimension::HorizontalAndLevels},
};
constexpr Choice<survey::AtmosphericCorrection> kAtmospheric[] = {
    {"auto: only where the instrument did not", survey::AtmosphericCorrection::Auto},
    {"recompute from the recorded weather", survey::AtmosphericCorrection::Recompute},
    {"a fixed ppm (advanced)", survey::AtmosphericCorrection::Fixed},
    {"none", survey::AtmosphericCorrection::None},
};
constexpr Choice<survey::PrismConstantPolicy> kPrism[] = {
    {"auto: only where the instrument did not", survey::PrismConstantPolicy::Auto},
    {"override with one constant (advanced)", survey::PrismConstantPolicy::Override},
    {"none: distances as recorded", survey::PrismConstantPolicy::None},
};
constexpr Choice<survey::FaceHandling> kFaces[] = {
    {"mean of face left and right", survey::FaceHandling::Average},
    {"face left only", survey::FaceHandling::FaceLeftOnly},
    {"every pointing on its own", survey::FaceHandling::Separate},
};
constexpr Choice<survey::GridScale> kGridScale[] = {
    {"none (ground distances)", survey::GridScale::None},
    {"a fixed factor", survey::GridScale::Fixed},
    {"from the drawing's projection", survey::GridScale::FromProjection},
};
constexpr Choice<survey::HeightReduction> kHeights[] = {
    {"none", survey::HeightReduction::None},
    {"to the ellipsoid", survey::HeightReduction::Ellipsoid},
    {"to the geoid (sea level)", survey::HeightReduction::Geoid},
};
constexpr Choice<survey::OutlierTest> kOutlierTests[] = {
    {"Baarda (data snooping)", survey::OutlierTest::Baarda},
    {"Pope's tau", survey::OutlierTest::Tau},
    {"none", survey::OutlierTest::None},
};
constexpr Choice<survey::ControlConstraint> kConstraints[] = {
    {"fixed", survey::ControlConstraint::Fixed},
    {"weighted", survey::ControlConstraint::Weighted},
    {"free", survey::ControlConstraint::Free},
};
constexpr const char* kFromFile = "the file";
constexpr const char* kFromDrawing = "the drawing";

template <class Enum, std::size_t N>
QComboBox* choiceBox(const char* name, const Choice<Enum> (&choices)[N], QWidget* parent)
{
    auto* box = new QComboBox(parent);
    box->setObjectName(name);
    for (const auto& choice : choices) {
        box->addItem(choice.label, static_cast<int>(choice.value));
    }
    return box;
}

template <class Enum>
void choose(QComboBox* box, Enum value)
{
    box->setCurrentIndex(std::max(0, box->findData(static_cast<int>(value))));
}

template <class Enum>
Enum chosen(const QComboBox* box)
{
    return static_cast<Enum>(box->currentData().toInt());
}

QLabel* stateLabel(const char* name, QWidget* parent)
{
    auto* label = new QLabel(parent);
    label->setObjectName(name);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

// Counts of a correction's state across the setups of a file, and the values
// the instrument used where it applied it.
struct StateCounts {
    std::size_t applied = 0;
    std::size_t notApplied = 0;
    std::size_t unknown = 0;
    std::vector<double> values; // where applied and stated
};

template <class Get>
StateCounts countStates(const survey::SurveyProject& raw, Get get)
{
    StateCounts counts;
    for (const survey::SurveyStation& station : raw.stations) {
        const auto [state, value] = get(station.instrument);
        switch (state) {
        case survey::CorrectionState::Applied:
            ++counts.applied;
            if (value) {
                counts.values.push_back(*value);
            }
            break;
        case survey::CorrectionState::NotApplied:
            ++counts.notApplied;
            break;
        case survey::CorrectionState::Unknown:
            ++counts.unknown;
            break;
        }
    }
    return counts;
}

// "applied by the instrument at 2 of 3 setups (+12.4 ppm); not applied at 1"
std::string stateText(const StateCounts& counts, std::size_t setups,
                      const std::function<std::string(double)>& value)
{
    if (setups == 0) {
        return "the file has no instrument setups";
    }
    std::vector<std::string> parts;
    const auto of = [setups](std::size_t n) {
        return n == setups ? (setups == 1 ? std::string("the setup")
                                          : std::format("all {} setups", setups))
                           : std::format("{} of {} setups", n, setups);
    };
    if (counts.applied > 0) {
        std::string part = "applied by the instrument at " + of(counts.applied);
        if (!counts.values.empty()) {
            const auto [low, high] =
                std::minmax_element(counts.values.begin(), counts.values.end());
            part += *low == *high ? " (" + value(*low) + ")"
                                  : " (" + value(*low) + " to " + value(*high) + ")";
        }
        parts.push_back(std::move(part));
    }
    if (counts.notApplied > 0) {
        parts.push_back("not applied by the instrument at " + of(counts.notApplied));
    }
    if (counts.unknown > 0) {
        parts.push_back("not stated by the file at " + of(counts.unknown));
    }
    std::string text;
    for (const std::string& part : parts) {
        text += text.empty() ? part : "; " + part;
    }
    return text;
}

std::string signedFixed(double value, int decimals, std::string_view unit)
{
    return std::format("{:+.{}f} {}", value, decimals, unit);
}

std::string constraintText(const survey::ControlComponent& a, const survey::ControlComponent& b)
{
    // a and b: northing and easting (the widget sets both alike), or the
    // elevation twice.
    const auto one = [](const survey::ControlComponent& c) -> std::string {
        switch (c.constraint) {
        case survey::ControlConstraint::Fixed:
            return "fixed";
        case survey::ControlConstraint::Weighted:
            return std::format("weighted {:.10g} mm", c.sigma * 1000.0);
        case survey::ControlConstraint::Free:
            return "free";
        }
        return "free";
    };
    const std::string first = one(a);
    const std::string second = one(b);
    return first == second ? first : "N " + first + ", E " + second;
}

} // namespace

std::string atmosphericStateText(const survey::SurveyProject& raw)
{
    const auto counts = countStates(raw, [](const survey::InstrumentSettings& instrument) {
        return std::pair{instrument.atmosphericPpmState, instrument.atmosphericPpm};
    });
    return stateText(counts, raw.stations.size(),
                     [](double ppm) { return signedFixed(ppm, 1, "ppm"); });
}

std::string prismStateText(const survey::SurveyProject& raw)
{
    const auto counts = countStates(raw, [](const survey::InstrumentSettings& instrument) {
        return std::pair{instrument.prismConstantState, instrument.prismConstant};
    });
    return stateText(counts, raw.stations.size(),
                     [](double metres) { return signedFixed(metres * 1000.0, 1, "mm"); });
}

std::string curvatureStateText(const survey::SurveyProject& raw)
{
    const auto counts = countStates(raw, [](const survey::InstrumentSettings& instrument) {
        return std::pair{instrument.curvatureRefractionState, instrument.refractionCoefficient};
    });
    return stateText(counts, raw.stations.size(),
                     [](double k) { return std::format("k = {:.3g}", k); });
}

ReductionOptionsWidget::ReductionOptionsWidget(QWidget* parent) : QWidget(parent)
{
    setObjectName("reductionOptions");
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->addWidget(buildBasic());
    layout->addWidget(buildControl());

    advanced_ = new QToolButton(this);
    advanced_->setObjectName("advanced");
    advanced_->setText("Advanced: tolerances, standard deviations, scale, statistics");
    advanced_->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    advanced_->setArrowType(Qt::RightArrow);
    advanced_->setCheckable(true);
    advanced_->setAutoRaise(true);
    layout->addWidget(advanced_);
    advancedOptions_ = buildAdvanced();
    layout->addWidget(advancedOptions_);
    advancedOptions_->setVisible(false);
    connect(advanced_, &QToolButton::toggled, this, [this](bool shown) { showAdvanced(shown); });
    layout->addStretch(1);

    setSettings(survey::ReductionSettings{});
}

QLineEdit* ReductionOptionsWidget::numberField(const char* name, const QString& tip)
{
    auto* field = new QLineEdit(this);
    field->setObjectName(name);
    field->setToolTip(tip);
    field->setMaximumWidth(120);
    connect(field, &QLineEdit::textChanged, this, [this] { changed(); });
    return field;
}

QWidget* ReductionOptionsWidget::buildBasic()
{
    auto* box = new QGroupBox("Adjustment and corrections", this);
    auto* form = new QFormLayout(box);

    auto* methodRow = new QHBoxLayout();
    method_ = choiceBox("method", kMethods, box);
    method_->setToolTip("Radiation computes each point from its setup and adjusts nothing; a "
                        "traverse or a network adjusts, and needs control");
    traverseRule_ = choiceBox("traverseRule", kTraverseRules, box);
    traverseRule_->setToolTip("How a traverse's misclosure is shared out");
    networkDimension_ = choiceBox("networkDimension", kDimensions, box);
    networkDimension_->setToolTip("What the least squares adjusts: there is no combined 3D "
                                  "network, so heights are a level network of their own");
    methodRow->addWidget(method_);
    methodRow->addWidget(traverseRule_);
    methodRow->addWidget(networkDimension_);
    methodRow->addStretch(1);
    form->addRow("Adjustment:", methodRow);

    atmospheric_ = choiceBox("atmospheric", kAtmospheric, box);
    atmosphericState_ = stateLabel("atmosphericState", box);
    auto* atmosphericColumn = new QVBoxLayout();
    atmosphericColumn->addWidget(atmospheric_);
    atmosphericColumn->addWidget(atmosphericState_);
    form->addRow("Atmospheric ppm:", atmosphericColumn);

    prism_ = choiceBox("prismConstant", kPrism, box);
    prismState_ = stateLabel("prismState", box);
    auto* prismColumn = new QVBoxLayout();
    prismColumn->addWidget(prism_);
    prismColumn->addWidget(prismState_);
    form->addRow("Prism constant:", prismColumn);

    faces_ = choiceBox("faces", kFaces, box);
    form->addRow("Faces:", faces_);

    curvature_ = new QCheckBox("Curvature and refraction", box);
    curvature_->setObjectName("curvatureRefraction");
    curvatureState_ = stateLabel("curvatureState", box);
    slope_ = new QCheckBox("Slope distances to horizontal", box);
    slope_->setObjectName("slopeToHorizontal");
    auto* reductionColumn = new QVBoxLayout();
    reductionColumn->addWidget(curvature_);
    reductionColumn->addWidget(curvatureState_);
    reductionColumn->addWidget(slope_);
    form->addRow("Reduction:", reductionColumn);

    for (QComboBox* choice : {method_, traverseRule_, networkDimension_, atmospheric_, prism_,
                              faces_}) {
        connect(choice, &QComboBox::currentIndexChanged, this, [this] { changed(); });
    }
    for (QCheckBox* check : {curvature_, slope_}) {
        connect(check, &QCheckBox::toggled, this, [this] { changed(); });
    }
    return box;
}

QWidget* ReductionOptionsWidget::buildControl()
{
    auto* box = new QGroupBox("Control", this);
    auto* layout = new QVBoxLayout(box);
    auto* row = new QHBoxLayout();
    controlFrom_ = new QComboBox(box);
    controlFrom_->setObjectName("controlFrom");
    controlFrom_->addItems({kFromFile, kFromDrawing});
    controlFrom_->setToolTip("Whose coordinates the point is held at: the file's, or those of "
                             "the survey point of that number already on the drawing");
    controlPick_ = new QComboBox(box);
    controlPick_->setObjectName("controlPick");
    controlPick_->setToolTip("The point to hold");
    controlPick_->setMinimumContentsLength(10);
    controlHorizontal_ = choiceBox("controlHorizontal", kConstraints, box);
    controlHorizontal_->setToolTip("Northing and easting: fixed, weighted by the standard "
                                   "deviation beside it, or free");
    controlSigmaHorizontal_ = new QLineEdit("5", box);
    controlSigmaHorizontal_->setObjectName("controlSigmaHorizontal");
    controlSigmaHorizontal_->setToolTip("Millimetres, for a weighted northing and easting");
    controlSigmaHorizontal_->setMaximumWidth(60);
    controlVertical_ = choiceBox("controlVertical", kConstraints, box);
    controlVertical_->setToolTip("Elevation: fixed, weighted or free");
    controlSigmaVertical_ = new QLineEdit("10", box);
    controlSigmaVertical_->setObjectName("controlSigmaVertical");
    controlSigmaVertical_->setToolTip("Millimetres, for a weighted elevation");
    controlSigmaVertical_->setMaximumWidth(60);
    auto* add = new QPushButton("Hold", box);
    add->setObjectName("addControl");
    add->setAutoDefault(false);
    add->setToolTip("Hold the point as shown, or change how a held point is held");
    auto* remove = new QPushButton("Release", box);
    remove->setObjectName("removeControl");
    remove->setAutoDefault(false);
    remove->setToolTip("Stop holding the point chosen in the table");
    // Two rows, so the options fit the narrow pane of Survey Jobs as well as
    // the wizard's page: which point, then how it is held.
    row->addWidget(new QLabel("Point from", box));
    row->addWidget(controlFrom_);
    row->addWidget(controlPick_, 1);
    row->addWidget(add);
    row->addWidget(remove);
    layout->addLayout(row);
    auto* how = new QHBoxLayout();
    how->addWidget(new QLabel("Northing, easting", box));
    how->addWidget(controlHorizontal_);
    how->addWidget(controlSigmaHorizontal_);
    how->addWidget(new QLabel("mm   Elevation", box));
    how->addWidget(controlVertical_);
    how->addWidget(controlSigmaVertical_);
    how->addWidget(new QLabel("mm", box));
    how->addStretch(1);
    layout->addLayout(how);

    controlTable_ = new QTableWidget(0, 4, box);
    controlTable_->setObjectName("control");
    controlTable_->setHorizontalHeaderLabels({"Point", "From", "Northing, easting", "Elevation"});
    controlTable_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    controlTable_->setSelectionBehavior(QAbstractItemView::SelectRows);
    controlTable_->setSelectionMode(QAbstractItemView::SingleSelection);
    controlTable_->verticalHeader()->setVisible(false);
    controlTable_->horizontalHeader()->setStretchLastSection(true);
    controlTable_->setMaximumHeight(110);
    layout->addWidget(controlTable_);

    connect(controlFrom_, &QComboBox::currentIndexChanged, this, [this] { fillControlPick(); });
    connect(controlHorizontal_, &QComboBox::currentIndexChanged, this,
            [this] { enableFields(); });
    connect(controlVertical_, &QComboBox::currentIndexChanged, this, [this] { enableFields(); });
    connect(add, &QPushButton::clicked, this, [this] { addControl(); });
    connect(remove, &QPushButton::clicked, this, [this] { removeControl(); });
    // Choosing a held point puts it in the editor, to change how it is held.
    connect(controlTable_, &QTableWidget::currentCellChanged, this, [this](int tableRow) {
        if (tableRow < 0 || static_cast<std::size_t>(tableRow) >= control_.size()) {
            return;
        }
        const survey::ControlSelection& selection =
            control_[static_cast<std::size_t>(tableRow)];
        controlFrom_->setCurrentIndex(selection.origin == survey::ControlOrigin::File ? 0 : 1);
        controlPick_->setCurrentIndex(controlPick_->findText(qs(selection.point.pointId)));
        choose(controlHorizontal_, selection.point.northing.constraint);
        choose(controlVertical_, selection.point.elevation.constraint);
        if (selection.point.northing.constraint == survey::ControlConstraint::Weighted) {
            controlSigmaHorizontal_->setText(shownNumber(selection.point.northing.sigma * 1000.0));
        }
        if (selection.point.elevation.constraint == survey::ControlConstraint::Weighted) {
            controlSigmaVertical_->setText(shownNumber(selection.point.elevation.sigma * 1000.0));
        }
    });
    return box;
}

QWidget* ReductionOptionsWidget::buildAdvanced()
{
    auto* page = new QWidget(this);
    page->setObjectName("advancedOptions");
    auto* grid = new QGridLayout(page);
    grid->setContentsMargins(0, 0, 0, 0);

    auto* faces = new QGroupBox("Face tolerances", page);
    auto* facesForm = new QFormLayout(faces);
    toleranceHorizontal_ = numberField("toleranceHorizontal",
                                       "Largest spread of a face-left / face-right pair, seconds");
    toleranceZenith_ = numberField("toleranceZenith",
                                   "Largest zenith spread (twice the index error), seconds");
    toleranceDistance_ = numberField("toleranceDistance", "Largest distance spread, millimetres");
    excludeOutside_ = new QCheckBox("Exclude a pair outside them", faces);
    excludeOutside_->setObjectName("excludeOutside");
    facesForm->addRow("Horizontal (\"):", toleranceHorizontal_);
    facesForm->addRow("Zenith (\"):", toleranceZenith_);
    facesForm->addRow("Distance (mm):", toleranceDistance_);
    facesForm->addRow("", excludeOutside_);
    grid->addWidget(faces, 0, 0);

    auto* sigmas = new QGroupBox("A-priori standard deviations", page);
    auto* sigmasForm = new QFormLayout(sigmas);
    sigmaDirection_ = numberField("sigmaDirection", "A direction, seconds");
    sigmaZenith_ = numberField("sigmaZenith", "A zenith angle, seconds");
    sigmaDistance_ = numberField("sigmaDistance", "The constant part of a distance, millimetres");
    sigmaPpm_ = numberField("sigmaPpm", "The part in proportion to the distance, ppm");
    sigmaInstrumentCentring_ = numberField("sigmaInstrumentCentring", "Millimetres");
    sigmaTargetCentring_ = numberField("sigmaTargetCentring", "Millimetres");
    sigmaHeight_ = numberField("sigmaHeight", "Instrument and target heights, millimetres");
    sigmaLevelling_ = numberField("sigmaLevelling", "Millimetres per square root of a kilometre");
    sigmaGnssHorizontal_ = numberField("sigmaGnssHorizontal",
                                       "Where the file gives no covariance, millimetres");
    sigmaGnssVertical_ = numberField("sigmaGnssVertical",
                                     "Where the file gives no covariance, millimetres");
    useFileCovariances_ = new QCheckBox("Use the covariances a GNSS file states", sigmas);
    useFileCovariances_->setObjectName("useFileCovariances");
    auto* distanceRow = new QHBoxLayout();
    distanceRow->addWidget(sigmaDistance_);
    distanceRow->addWidget(new QLabel("mm +", sigmas));
    distanceRow->addWidget(sigmaPpm_);
    distanceRow->addWidget(new QLabel("ppm", sigmas));
    distanceRow->addStretch(1);
    auto* centringRow = new QHBoxLayout();
    centringRow->addWidget(sigmaInstrumentCentring_);
    centringRow->addWidget(new QLabel("instrument,", sigmas));
    centringRow->addWidget(sigmaTargetCentring_);
    centringRow->addWidget(new QLabel("target", sigmas));
    centringRow->addStretch(1);
    auto* gnssRow = new QHBoxLayout();
    gnssRow->addWidget(sigmaGnssHorizontal_);
    gnssRow->addWidget(new QLabel("horizontal,", sigmas));
    gnssRow->addWidget(sigmaGnssVertical_);
    gnssRow->addWidget(new QLabel("vertical", sigmas));
    gnssRow->addStretch(1);
    sigmasForm->addRow("Direction (\"):", sigmaDirection_);
    sigmasForm->addRow("Zenith (\"):", sigmaZenith_);
    sigmasForm->addRow("Distance:", distanceRow);
    sigmasForm->addRow("Centring (mm):", centringRow);
    sigmasForm->addRow("Heights (mm):", sigmaHeight_);
    sigmasForm->addRow("Levelling (mm/sqrt km):", sigmaLevelling_);
    sigmasForm->addRow("GNSS (mm):", gnssRow);
    sigmasForm->addRow("", useFileCovariances_);
    grid->addWidget(sigmas, 0, 1, 2, 1);

    auto* corrections = new QGroupBox("Corrections and scale", page);
    auto* correctionsForm = new QFormLayout(corrections);
    refractionK_ = numberField("refractionK", "Coefficient of refraction k, 0.13 by day");
    earthRadius_ = numberField("earthRadius", "Metres");
    fixedPpm_ = numberField("fixedPpm", "Applied to every distance the instrument did not "
                                        "correct, when the atmospheric choice is a fixed ppm");
    prismConstantValue_ = numberField("prismConstantValue",
                                      "Millimetres, when the prism constant is overridden");
    gridScale_ = choiceBox("gridScale", kGridScale, corrections);
    gridScaleFactor_ = numberField("gridScaleFactor", "The factor, when it is fixed");
    useCombinedFactor_ = new QCheckBox("A combined factor instead of height and grid", corrections);
    useCombinedFactor_->setObjectName("useCombinedFactor");
    combinedFactor_ = numberField("combinedFactor", "The combined scale factor, e.g. from a "
                                                    "control certificate");
    heightReduction_ = choiceBox("heightReduction", kHeights, corrections);
    auto* gridRow = new QHBoxLayout();
    gridRow->addWidget(gridScale_);
    gridRow->addWidget(gridScaleFactor_);
    gridRow->addStretch(1);
    auto* combinedRow = new QHBoxLayout();
    combinedRow->addWidget(useCombinedFactor_);
    combinedRow->addWidget(combinedFactor_);
    combinedRow->addStretch(1);
    correctionsForm->addRow("Refraction k:", refractionK_);
    correctionsForm->addRow("Earth radius (m):", earthRadius_);
    correctionsForm->addRow("Fixed ppm:", fixedPpm_);
    correctionsForm->addRow("Prism constant (mm):", prismConstantValue_);
    correctionsForm->addRow("Grid scale:", gridRow);
    correctionsForm->addRow("", combinedRow);
    correctionsForm->addRow("Height reduction:", heightReduction_);
    grid->addWidget(corrections, 1, 0);

    auto* statistics = new QGroupBox("Statistics", page);
    auto* statisticsForm = new QFormLayout(statistics);
    confidence_ = numberField("confidence", "Of the error ellipses and the global test, percent");
    outlierTest_ = choiceBox("outlierTest", kOutlierTests, statistics);
    outlierSignificance_ = numberField("outlierSignificance",
                                       "Per observation: 0.001 is Baarda's, critical value 3.29");
    autoReject_ = new QCheckBox("Reject the worst outlier and adjust again, one at a time",
                                statistics);
    autoReject_->setObjectName("autoReject");
    iterations_ = new QSpinBox(statistics);
    iterations_->setObjectName("iterations");
    iterations_->setRange(1, 1000);
    iterations_->setMaximumWidth(80);
    statisticsForm->addRow("Confidence (%):", confidence_);
    statisticsForm->addRow("Outlier test:", outlierTest_);
    statisticsForm->addRow("Significance:", outlierSignificance_);
    statisticsForm->addRow("", autoReject_);
    statisticsForm->addRow("Iterations:", iterations_);
    grid->addWidget(statistics, 2, 0, 1, 2);

    for (QComboBox* choice : {gridScale_, heightReduction_, outlierTest_}) {
        connect(choice, &QComboBox::currentIndexChanged, this, [this] { changed(); });
    }
    for (QCheckBox* check : {excludeOutside_, useFileCovariances_, useCombinedFactor_,
                             autoReject_}) {
        connect(check, &QCheckBox::toggled, this, [this] { changed(); });
    }
    connect(iterations_, &QSpinBox::valueChanged, this, [this] { changed(); });
    return page;
}

bool ReductionOptionsWidget::advancedShown() const
{
    return advanced_->isChecked();
}

void ReductionOptionsWidget::showAdvanced(bool shown)
{
    if (advanced_->isChecked() != shown) {
        advanced_->setChecked(shown); // comes back here through toggled
        return;
    }
    advanced_->setArrowType(shown ? Qt::DownArrow : Qt::RightArrow);
    advancedOptions_->setVisible(shown);
    if (!shown) {
        return;
    }
    // Opened below the fold of a page that scrolls, the section would open
    // out of sight: the page is scrolled to put the fold at the top. On the
    // event loop, once the section has been laid out.
    QTimer::singleShot(0, this, [this] {
        for (QWidget* up = parentWidget(); up != nullptr; up = up->parentWidget()) {
            auto* area = qobject_cast<QScrollArea*>(up);
            if (area != nullptr && area->widget() != nullptr) {
                const int top = advanced_->mapTo(area->widget(), QPoint(0, 0)).y();
                area->verticalScrollBar()->setValue(std::max(0, top - 6));
                return;
            }
        }
    });
}

void ReductionOptionsWidget::showNumber(QLineEdit* field, double shown, double stored)
{
    const QString text = shownNumber(shown);
    field->setText(text);
    field->setProperty("katanaShown", text);
    field->setProperty("katanaStored", stored);
}

Result<double> ReductionOptionsWidget::number(const QLineEdit* field, double divisor,
                                              const char* what) const
{
    const QString text = field->text().trimmed();
    // Untouched: the setting's own value, not its rounded text read back.
    if (text == field->property("katanaShown").toString()) {
        return field->property("katanaStored").toDouble();
    }
    const auto value = katana::core::parseFiniteDouble(text.toStdString());
    if (!value) {
        return makeError(ErrorCode::ParseFailure, std::string(what) + " is not a number",
                         text.toStdString());
    }
    // Divided, not multiplied by a reciprocal: 95 / 100 is 0.95, while
    // 95 * 0.01 is 0.9500000000000001.
    return *value / divisor;
}

void ReductionOptionsWidget::setSettings(const survey::ReductionSettings& settings)
{
    filling_ = true;
    shown_ = settings;
    choose(method_, settings.method);
    choose(traverseRule_, settings.traverseRule);
    choose(networkDimension_, settings.networkDimension);
    choose(atmospheric_, settings.atmospheric);
    choose(prism_, settings.prismConstantPolicy);
    choose(faces_, settings.faces);
    curvature_->setChecked(settings.curvatureAndRefraction);
    slope_->setChecked(settings.slopeToHorizontal);

    showNumber(toleranceHorizontal_, settings.faceTolerances.horizontal * kSecondsPerRadian,
               settings.faceTolerances.horizontal);
    showNumber(toleranceZenith_, settings.faceTolerances.zenith * kSecondsPerRadian,
               settings.faceTolerances.zenith);
    showNumber(toleranceDistance_, settings.faceTolerances.distance * 1000.0,
               settings.faceTolerances.distance);
    excludeOutside_->setChecked(settings.faceTolerances.excludeOutside);
    const survey::ObservationPrecision& p = settings.apriori;
    showNumber(sigmaDirection_, p.direction * kSecondsPerRadian, p.direction);
    showNumber(sigmaZenith_, p.zenith * kSecondsPerRadian, p.zenith);
    showNumber(sigmaDistance_, p.distanceConstant * 1000.0, p.distanceConstant);
    showNumber(sigmaPpm_, p.distancePpm, p.distancePpm);
    showNumber(sigmaInstrumentCentring_, p.instrumentCentring * 1000.0, p.instrumentCentring);
    showNumber(sigmaTargetCentring_, p.targetCentring * 1000.0, p.targetCentring);
    showNumber(sigmaHeight_, p.heightMeasurement * 1000.0, p.heightMeasurement);
    showNumber(sigmaLevelling_, p.levellingPerSqrtKilometre * 1000.0,
               p.levellingPerSqrtKilometre);
    showNumber(sigmaGnssHorizontal_, p.gnssHorizontal * 1000.0, p.gnssHorizontal);
    showNumber(sigmaGnssVertical_, p.gnssVertical * 1000.0, p.gnssVertical);
    useFileCovariances_->setChecked(settings.useFileCovariances);
    showNumber(refractionK_, settings.refractionCoefficient, settings.refractionCoefficient);
    showNumber(earthRadius_, settings.earthRadius, settings.earthRadius);
    showNumber(fixedPpm_, settings.fixedPpm, settings.fixedPpm);
    showNumber(prismConstantValue_, settings.prismConstant * 1000.0, settings.prismConstant);
    choose(gridScale_, settings.gridScale);
    showNumber(gridScaleFactor_, settings.fixedGridScaleFactor, settings.fixedGridScaleFactor);
    useCombinedFactor_->setChecked(settings.useCombinedFactor);
    showNumber(combinedFactor_, settings.combinedFactor, settings.combinedFactor);
    choose(heightReduction_, settings.heightReduction);
    showNumber(confidence_, settings.confidenceLevel * 100.0, settings.confidenceLevel);
    choose(outlierTest_, settings.outlierTest);
    showNumber(outlierSignificance_, settings.outlierSignificance, settings.outlierSignificance);
    autoReject_->setChecked(settings.autoRejectOutliers);
    iterations_->setValue(static_cast<int>(std::clamp<std::size_t>(settings.maxIterations, 1,
                                                                   1000)));
    control_ = settings.control;
    showControl();
    filling_ = false;
    enableFields();
}

Result<survey::ReductionSettings> ReductionOptionsWidget::settings() const
{
    // Everything the fields do not show is kept as it was given.
    survey::ReductionSettings s = shown_;
    s.method = chosen<survey::AdjustmentMethod>(method_);
    s.traverseRule = chosen<survey::TraverseRule>(traverseRule_);
    s.networkDimension = chosen<survey::NetworkDimension>(networkDimension_);
    s.atmospheric = chosen<survey::AtmosphericCorrection>(atmospheric_);
    s.prismConstantPolicy = chosen<survey::PrismConstantPolicy>(prism_);
    s.faces = chosen<survey::FaceHandling>(faces_);
    s.curvatureAndRefraction = curvature_->isChecked();
    s.slopeToHorizontal = slope_->isChecked();

    const double arc = kSecondsPerRadian;
    const struct {
        const QLineEdit* field;
        double divisor; // from the unit shown to the setting's
        const char* what;
        double* target;
    } numbers[] = {
        {toleranceHorizontal_, arc, "the horizontal face tolerance", &s.faceTolerances.horizontal},
        {toleranceZenith_, arc, "the zenith face tolerance", &s.faceTolerances.zenith},
        {toleranceDistance_, 1000.0, "the distance face tolerance", &s.faceTolerances.distance},
        {sigmaDirection_, arc, "the direction standard deviation", &s.apriori.direction},
        {sigmaZenith_, arc, "the zenith standard deviation", &s.apriori.zenith},
        {sigmaDistance_, 1000.0, "the distance standard deviation", &s.apriori.distanceConstant},
        {sigmaPpm_, 1.0, "the distance ppm", &s.apriori.distancePpm},
        {sigmaInstrumentCentring_, 1000.0, "the instrument centring",
         &s.apriori.instrumentCentring},
        {sigmaTargetCentring_, 1000.0, "the target centring", &s.apriori.targetCentring},
        {sigmaHeight_, 1000.0, "the height standard deviation", &s.apriori.heightMeasurement},
        {sigmaLevelling_, 1000.0, "the levelling standard deviation",
         &s.apriori.levellingPerSqrtKilometre},
        {sigmaGnssHorizontal_, 1000.0, "the GNSS horizontal standard deviation",
         &s.apriori.gnssHorizontal},
        {sigmaGnssVertical_, 1000.0, "the GNSS vertical standard deviation",
         &s.apriori.gnssVertical},
        {refractionK_, 1.0, "the refraction coefficient", &s.refractionCoefficient},
        {earthRadius_, 1.0, "the earth radius", &s.earthRadius},
        {fixedPpm_, 1.0, "the fixed ppm", &s.fixedPpm},
        {prismConstantValue_, 1000.0, "the prism constant", &s.prismConstant},
        {gridScaleFactor_, 1.0, "the grid scale factor", &s.fixedGridScaleFactor},
        {combinedFactor_, 1.0, "the combined factor", &s.combinedFactor},
        {confidence_, 100.0, "the confidence level", &s.confidenceLevel},
        {outlierSignificance_, 1.0, "the outlier significance", &s.outlierSignificance},
    };
    for (const auto& entry : numbers) {
        auto value = number(entry.field, entry.divisor, entry.what);
        if (!value) {
            return value.error();
        }
        *entry.target = *value;
    }
    s.faceTolerances.excludeOutside = excludeOutside_->isChecked();
    s.useFileCovariances = useFileCovariances_->isChecked();
    s.gridScale = chosen<survey::GridScale>(gridScale_);
    s.useCombinedFactor = useCombinedFactor_->isChecked();
    s.heightReduction = chosen<survey::HeightReduction>(heightReduction_);
    s.outlierTest = chosen<survey::OutlierTest>(outlierTest_);
    s.autoRejectOutliers = autoReject_->isChecked();
    s.maxIterations = static_cast<std::size_t>(iterations_->value());
    s.control = control_;
    if (auto status = survey::validateReductionSettings(s); !status) {
        return status.error();
    }
    return s;
}

void ReductionOptionsWidget::setProject(const survey::SurveyProject& raw)
{
    atmosphericState_->setText(qs(atmosphericStateText(raw)));
    prismState_->setText(qs(prismStateText(raw)));
    curvatureState_->setText(qs(curvatureStateText(raw)));
    fileCandidates_.clear();
    fileCandidates_.reserve(raw.points.size());
    for (const survey::SurveyPoint& point : raw.points) {
        fileCandidates_.push_back({point.id, survey::ControlOrigin::File,
                                   point.elevation.has_value()});
    }
    fillControlPick();
}

void ReductionOptionsWidget::setDrawingPoints(const std::vector<survey::SurveyPoint>& points)
{
    drawingCandidates_.clear();
    drawingCandidates_.reserve(points.size());
    for (const survey::SurveyPoint& point : points) {
        drawingCandidates_.push_back({point.id, survey::ControlOrigin::Drawing,
                                      point.elevation.has_value()});
    }
    fillControlPick();
}

void ReductionOptionsWidget::fillControlPick()
{
    const auto& candidates = controlFrom_->currentIndex() == 1 ? drawingCandidates_
                                                                : fileCandidates_;
    const QString current = controlPick_->currentText();
    controlPick_->clear();
    for (const ControlCandidate& candidate : candidates) {
        controlPick_->addItem(qs(candidate.id));
    }
    controlPick_->setCurrentIndex(std::max(0, controlPick_->findText(current)));
    controlPick_->setToolTip(candidates.empty()
                                 ? QString("%1 has no positioned points to hold")
                                       .arg(controlFrom_->currentText())
                                 : QString("%1 point(s) of %2")
                                       .arg(candidates.size())
                                       .arg(controlFrom_->currentText()));
}

void ReductionOptionsWidget::showControl()
{
    controlTable_->setRowCount(static_cast<int>(control_.size()));
    for (std::size_t i = 0; i < control_.size(); ++i) {
        const survey::ControlSelection& selection = control_[i];
        const QStringList cells{
            qs(selection.point.pointId),
            selection.origin == survey::ControlOrigin::File ? kFromFile : kFromDrawing,
            qs(constraintText(selection.point.northing, selection.point.easting)),
            qs(constraintText(selection.point.elevation, selection.point.elevation))};
        for (int column = 0; column < cells.size(); ++column) {
            controlTable_->setItem(static_cast<int>(i), column, new QTableWidgetItem(cells[column]));
        }
    }
    controlTable_->resizeColumnsToContents();
}

void ReductionOptionsWidget::addControl()
{
    const QString id = controlPick_->currentText();
    if (id.isEmpty()) {
        controlPick_->setFocus();
        return;
    }
    const auto component = [](const QComboBox* box, const QLineEdit* sigma)
        -> Result<survey::ControlComponent> {
        survey::ControlComponent c;
        c.constraint = chosen<survey::ControlConstraint>(box);
        if (c.constraint == survey::ControlConstraint::Weighted) {
            const auto value = katana::core::parseFiniteDouble(sigma->text().trimmed().toStdString());
            if (!value || *value <= 0.0) {
                return makeError(ErrorCode::InvalidArgument,
                                 "a weighted control component needs a standard deviation "
                                 "above 0 mm",
                                 sigma->text().toStdString());
            }
            c.sigma = *value / 1000.0;
        }
        return c;
    };
    const auto horizontal = component(controlHorizontal_, controlSigmaHorizontal_);
    const auto vertical = component(controlVertical_, controlSigmaVertical_);
    if (!horizontal || !vertical) {
        const auto& error = !horizontal ? horizontal.error() : vertical.error();
        controlTable_->setToolTip(qs(error.describe()));
        controlSigmaHorizontal_->setFocus();
        return;
    }
    survey::ControlSelection selection;
    selection.origin = controlFrom_->currentIndex() == 1 ? survey::ControlOrigin::Drawing
                                                         : survey::ControlOrigin::File;
    selection.point.pointId = id.toStdString();
    selection.point.northing = *horizontal;
    selection.point.easting = *horizontal;
    selection.point.elevation = *vertical;
    const auto same = std::ranges::find(control_, selection.point.pointId,
                                        [](const survey::ControlSelection& s) {
                                            return s.point.pointId;
                                        });
    if (same != control_.end()) {
        *same = std::move(selection);
    } else {
        control_.push_back(std::move(selection));
    }
    showControl();
    changed();
}

void ReductionOptionsWidget::removeControl()
{
    int row = controlTable_->currentRow();
    if (row < 0) {
        // Nothing chosen in the table: the point the editor names.
        const std::string id = controlPick_->currentText().toStdString();
        for (std::size_t i = 0; i < control_.size(); ++i) {
            if (control_[i].point.pointId == id) {
                row = static_cast<int>(i);
            }
        }
    }
    if (row < 0 || static_cast<std::size_t>(row) >= control_.size()) {
        return;
    }
    control_.erase(control_.begin() + row);
    showControl();
    changed();
}

void ReductionOptionsWidget::enableFields()
{
    const auto method = chosen<survey::AdjustmentMethod>(method_);
    traverseRule_->setVisible(method == survey::AdjustmentMethod::Traverse);
    networkDimension_->setVisible(method == survey::AdjustmentMethod::Network);
    refractionK_->setEnabled(curvature_->isChecked());
    fixedPpm_->setEnabled(chosen<survey::AtmosphericCorrection>(atmospheric_) ==
                          survey::AtmosphericCorrection::Fixed);
    prismConstantValue_->setEnabled(chosen<survey::PrismConstantPolicy>(prism_) ==
                                    survey::PrismConstantPolicy::Override);
    const bool combined = useCombinedFactor_->isChecked();
    combinedFactor_->setEnabled(combined);
    gridScale_->setEnabled(!combined);
    heightReduction_->setEnabled(!combined);
    gridScaleFactor_->setEnabled(!combined && chosen<survey::GridScale>(gridScale_) ==
                                                  survey::GridScale::Fixed);
    const bool testing = chosen<survey::OutlierTest>(outlierTest_) != survey::OutlierTest::None;
    outlierSignificance_->setEnabled(testing);
    autoReject_->setEnabled(testing);
    controlSigmaHorizontal_->setEnabled(chosen<survey::ControlConstraint>(controlHorizontal_) ==
                                        survey::ControlConstraint::Weighted);
    controlSigmaVertical_->setEnabled(chosen<survey::ControlConstraint>(controlVertical_) ==
                                      survey::ControlConstraint::Weighted);
}

void ReductionOptionsWidget::changed()
{
    if (filling_) {
        return;
    }
    enableFields();
    if (onChanged) {
        onChanged();
    }
}

} // namespace katana::qt
