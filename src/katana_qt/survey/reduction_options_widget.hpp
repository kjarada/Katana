#pragma once

// The reduction and adjustment options of a survey job, as a person sets
// them: simple by default, complete when opened.
//
// Shown by the import wizard's Reduction & Adjustment step and by Survey
// Jobs' Edit Adjustment, so a job's settings are changed later exactly where
// they were first chosen.
//
//   Basic     the adjustment (radiation, a traverse and its rule, a network
//             and its dimension); the corrections, each with what the
//             INSTRUMENT already did to the numbers shown beside it, read from
//             the file's setups (survey::InstrumentSettings) - "applied by
//             the instrument at 2 of 2 setups (+12.4 ppm)"; and the control:
//             points picked from the file's positioned points or the
//             drawing's survey points, each held Fixed or Weighted,
//             horizontally and vertically.
//   Advanced  folded until opened: face tolerances, the a-priori standard
//             deviations, the refraction coefficient and earth radius, the
//             fixed ppm and prism constant, the scale factor (none, fixed or
//             from the projection; or a combined factor), the height
//             reduction, the confidence level, the outlier test, its
//             significance and auto-reject, and the iterations.
//
// The widget holds NO survey logic: it maps survey::ReductionSettings to
// fields and back, in the units a surveyor reads - seconds of arc,
// millimetres, parts per million - while the settings stay in radians and
// metres. A field left as it was shown gives back the setting's own value,
// bit for bit, so opening a job's options and applying them unchanged
// changes nothing; only a field the person edited is parsed (core/text.hpp,
// never the locale).
//
// Object names (an interface: the headless --fill and --press use them):
//   basic     method traverseRule networkDimension atmospheric atmosphericState
//             prismConstant prismState faces curvatureRefraction
//             curvatureState slopeToHorizontal
//   control   controlFrom controlPick controlHorizontal controlVertical
//             controlSigmaHorizontal controlSigmaVertical addControl
//             removeControl control (the table of chosen control)
//   advanced  advanced (the fold button) advancedOptions (what it folds)
//             toleranceHorizontal toleranceZenith toleranceDistance
//             excludeOutside sigmaDirection sigmaZenith sigmaDistance
//             sigmaPpm sigmaInstrumentCentring sigmaTargetCentring sigmaHeight
//             sigmaLevelling sigmaGnssHorizontal sigmaGnssVertical
//             useFileCovariances refractionK earthRadius fixedPpm
//             prismConstantValue gridScale gridScaleFactor useCombinedFactor
//             combinedFactor heightReduction confidence outlierTest
//             outlierSignificance autoReject iterations

#include <QWidget>

#include <functional>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/survey/reduction_settings.hpp"

class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QSpinBox;
class QTableWidget;
class QToolButton;

namespace katana::qt {

// A point that may be taken as control, and whose coordinates it would be
// held at.
struct ControlCandidate {
    std::string id;
    katana::survey::ControlOrigin origin = katana::survey::ControlOrigin::File;
    bool hasElevation = false;
};

class ReductionOptionsWidget final : public QWidget {
  public:
    explicit ReductionOptionsWidget(QWidget* parent);

    // The fields show `settings`; what the widget gives back until a field is
    // edited.
    void setSettings(const katana::survey::ReductionSettings& settings);
    // The settings as the fields say, checked by
    // survey::validateReductionSettings. ParseFailure naming the field for a
    // number that does not read.
    [[nodiscard]] katana::core::Result<katana::survey::ReductionSettings> settings() const;

    // What the file's setups say the instrument already applied, and the
    // file's positioned points as control candidates.
    void setProject(const katana::survey::SurveyProject& raw);
    // The drawing's survey points as control candidates.
    void setDrawingPoints(const std::vector<katana::survey::SurveyPoint>& points);

    [[nodiscard]] bool advancedShown() const;
    void showAdvanced(bool shown);

    // Called when any setting changes, e.g. so a page can mark a preview as
    // out of date.
    std::function<void()> onChanged;

  private:
    QWidget* buildBasic();
    QWidget* buildControl();
    QWidget* buildAdvanced();
    // A number field showing `value` (in the unit shown), remembered with the
    // value it stands for so an untouched field gives that value back.
    QLineEdit* numberField(const char* name, const QString& tip);
    void showNumber(QLineEdit* field, double shown, double stored);
    [[nodiscard]] katana::core::Result<double> number(const QLineEdit* field, double divisor,
                                                      const char* what) const;
    void fillControlPick();
    void showControl();
    void addControl();
    void removeControl();
    void enableFields();
    void changed();

    katana::survey::ReductionSettings shown_{}; // what setSettings gave
    std::vector<katana::survey::ControlSelection> control_{};
    std::vector<ControlCandidate> fileCandidates_{};
    std::vector<ControlCandidate> drawingCandidates_{};
    bool filling_ = false; // setSettings is writing the fields

    // basic
    QComboBox* method_ = nullptr;
    QComboBox* traverseRule_ = nullptr;
    QComboBox* networkDimension_ = nullptr;
    QComboBox* atmospheric_ = nullptr;
    QLabel* atmosphericState_ = nullptr;
    QComboBox* prism_ = nullptr;
    QLabel* prismState_ = nullptr;
    QComboBox* faces_ = nullptr;
    QCheckBox* curvature_ = nullptr;
    QLabel* curvatureState_ = nullptr;
    QCheckBox* slope_ = nullptr;
    // control
    QComboBox* controlFrom_ = nullptr;
    QComboBox* controlPick_ = nullptr;
    QComboBox* controlHorizontal_ = nullptr;
    QComboBox* controlVertical_ = nullptr;
    QLineEdit* controlSigmaHorizontal_ = nullptr;
    QLineEdit* controlSigmaVertical_ = nullptr;
    QTableWidget* controlTable_ = nullptr;
    // advanced
    QToolButton* advanced_ = nullptr;
    QWidget* advancedOptions_ = nullptr;
    QLineEdit* toleranceHorizontal_ = nullptr;
    QLineEdit* toleranceZenith_ = nullptr;
    QLineEdit* toleranceDistance_ = nullptr;
    QCheckBox* excludeOutside_ = nullptr;
    QLineEdit* sigmaDirection_ = nullptr;
    QLineEdit* sigmaZenith_ = nullptr;
    QLineEdit* sigmaDistance_ = nullptr;
    QLineEdit* sigmaPpm_ = nullptr;
    QLineEdit* sigmaInstrumentCentring_ = nullptr;
    QLineEdit* sigmaTargetCentring_ = nullptr;
    QLineEdit* sigmaHeight_ = nullptr;
    QLineEdit* sigmaLevelling_ = nullptr;
    QLineEdit* sigmaGnssHorizontal_ = nullptr;
    QLineEdit* sigmaGnssVertical_ = nullptr;
    QCheckBox* useFileCovariances_ = nullptr;
    QLineEdit* refractionK_ = nullptr;
    QLineEdit* earthRadius_ = nullptr;
    QLineEdit* fixedPpm_ = nullptr;
    QLineEdit* prismConstantValue_ = nullptr;
    QComboBox* gridScale_ = nullptr;
    QLineEdit* gridScaleFactor_ = nullptr;
    QCheckBox* useCombinedFactor_ = nullptr;
    QLineEdit* combinedFactor_ = nullptr;
    QComboBox* heightReduction_ = nullptr;
    QLineEdit* confidence_ = nullptr;
    QComboBox* outlierTest_ = nullptr;
    QLineEdit* outlierSignificance_ = nullptr;
    QCheckBox* autoReject_ = nullptr;
    QSpinBox* iterations_ = nullptr;
};

// Words for what the instrument did to one correction across the setups of
// a file: "applied by the instrument at 2 of 2 setups (+12.4 ppm)", "not
// stated at 3 setups". Exposed for the tests.
[[nodiscard]] std::string atmosphericStateText(const katana::survey::SurveyProject& raw);
[[nodiscard]] std::string prismStateText(const katana::survey::SurveyProject& raw);
[[nodiscard]] std::string curvatureStateText(const katana::survey::SurveyProject& raw);

} // namespace katana::qt
