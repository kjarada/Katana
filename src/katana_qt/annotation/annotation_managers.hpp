#pragma once

// The Text Style and Label Style managers (Format > Text Styles...,
// Format > Label Styles and Rules...; docs/annotation.md "In the window").
//
// Each is a list of the table's items and a form for the chosen one. They
// change nothing themselves: every button builds the verb line a person would
// type - TEXTSTYLE, LABELSTYLE, AUTOLABEL - and runs it through the window's
// one executor (command_runner.hpp), so an edit is echoed in the command log,
// undone as a typed line is, and refused in the verb's own words in the
// dialog's problem line. Apply sends only the fields the form changed, as one
// line (one undo step), and nothing when none did. The dialogs hold no copy
// of the tables: every refresh reads the document, so an undo or a verb typed
// while one is open shows at once.
//
// Object names, for the headless driver and the tests:
//   textStyleManagerDialog
//     textStyleList, textStyleNewName (blank: the fresh name its placeholder
//     shows), textStyleNew, textStyleDelete, textStyleApply, textStyleProblem;
//     the form: textStyleFont, textStylePaperHeight, textStyleWidthFactor,
//     textStyleOblique, textStyleBold, textStyleItalic, textStyleColour,
//     textStyleMask, textStyleMaskMargin, textStyleReadable,
//     textStyleLineSpacing, textStyleSample
//   labelStyleManagerDialog, its tabs labelStyleTabs
//     Label Styles: labelStyleList, labelStyleNewName, labelStyleNewKind,
//     labelStyleNew, labelStyleDelete, labelStyleDefaults, labelStyleApply;
//     the form: labelStyleKind, labelStyleTemplate, labelTemplateInsert (the
//     values a template of the kind may use, each with its steps: an action
//     "labelValue:VALUE" or "labelValue:VALUE:STEP" puts {VALUE:STEP} at the
//     cursor), labelStyleTemplateCheck, labelStyleTextStyle,
//     labelStylePaperHeight, labelStylePlacement, labelStyleOrientation,
//     labelStyleOffset, labelStyleLeader, labelStyleDisplace,
//     labelStylePriority, labelStyleMarker, labelStyleMarkerSize,
//     labelStyleMinimumLength, labelStyleInterval, labelStyleTickInterval,
//     labelStyleTickLength
//     Auto-Label Rules: labelRuleTable (Rule, Label style, Layer, Code, Type,
//     Labels go on, Enabled - a check box that switches the rule - and
//     Labels, from the last run or preview); the form labelRuleName,
//     labelRuleStyle, labelRuleLayer, labelRuleCode, labelRuleType,
//     labelRuleLabelLayer, labelRuleEnabled; labelRuleAdd, labelRuleUpdate,
//     labelRuleDelete, labelRulePreview, labelRuleRun, labelRuleClear and
//     labelRuleReport. Run, Preview and Clear act on the rules selected in
//     the table, every enabled rule when none is.
//   labelStyleProblem   the last refusal, under both tabs

#include <QDialog>
#include <QMap>
#include <QString>
#include <QStringList>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/annotation.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QListWidget;
class QMenu;
class QPushButton;
class QSpinBox;
class QTableWidget;
class QTabWidget;
class QToolButton;

namespace katana::qt {

class TextStyleManagerDialog final : public QDialog {
  public:
    TextStyleManagerDialog(katana::cad::Document& document, CommandRunner run,
                           QWidget* parent = nullptr);

    // Selects a style by name and shows it in the form.
    void select(const QString& name);
    // The form's style, as Apply would store it.
    [[nodiscard]] katana::entity::TextStyle formStyle() const;
    // The line Apply runs: empty when the form changes nothing.
    [[nodiscard]] katana::core::Result<QString> applyLine() const;
    // What the last refused action said; empty after one that worked.
    [[nodiscard]] QString problem() const;

    // The actions, as the buttons run them. New names the style from
    // textStyleNewName, or the fresh name its placeholder shows.
    bool apply();
    bool addStyle();
    bool deleteStyle();

  private:
    void refresh();
    void showStyle(const katana::entity::TextStyle& style);
    bool run(const katana::core::Result<QString>& line);
    [[nodiscard]] QString current() const;
    [[nodiscard]] QString freshName() const;

    katana::cad::Document& document_;
    CommandRunner run_;
    katana::cad::Document::ListenerHandle listener_;
    QListWidget* list_ = nullptr;
    QLineEdit* newName_ = nullptr;
    QLineEdit* font_ = nullptr;
    QDoubleSpinBox* paper_ = nullptr;
    QDoubleSpinBox* width_ = nullptr;
    QDoubleSpinBox* oblique_ = nullptr;
    QCheckBox* bold_ = nullptr;
    QCheckBox* italic_ = nullptr;
    QLineEdit* colour_ = nullptr;
    QCheckBox* mask_ = nullptr;
    QDoubleSpinBox* margin_ = nullptr;
    QCheckBox* readable_ = nullptr;
    QDoubleSpinBox* spacing_ = nullptr;
    QLabel* sample_ = nullptr;
    QLabel* problem_ = nullptr;
    QString shown_;
};

class LabelStyleManagerDialog final : public QDialog {
  public:
    LabelStyleManagerDialog(katana::cad::Document& document, CommandRunner run,
                            QWidget* parent = nullptr);

    void select(const QString& name);
    [[nodiscard]] katana::entity::LabelStyle formStyle() const;
    [[nodiscard]] katana::core::Result<QString> applyLine() const;
    [[nodiscard]] QString problem() const;
    // What the last rule run, preview or clear did, as AUTOLABEL replies.
    [[nodiscard]] QString runReport() const;
    // The rules selected in the table, in its order; empty for none.
    [[nodiscard]] QStringList chosenRules() const;

    bool apply();
    // New names the style from labelStyleNewName (or the fresh name) and
    // gives it the kind labelStyleNewKind shows.
    bool addStyle();
    bool deleteStyle();
    bool addDefaults();
    // Puts "{field}" - "value" or "value:step" - at the template's cursor, as
    // the Insert Value menu does; "prop.NAME" leaves NAME selected to type
    // over.
    void insertValue(const QString& field);

    // The rules tab: a rule from its form, the form stored over the rule of
    // its name, a rule switched on or off, and the chosen rules (every
    // enabled one when none is chosen) run, previewed or cleared.
    bool addRule();
    bool updateRule();
    bool deleteRule();
    bool setRuleEnabled(const QString& name, bool enabled);
    bool runRules();
    bool previewRules();
    bool clearRules();

  private:
    void refresh();
    void showStyle(const katana::entity::LabelStyle& style);
    void showRule(const QString& name);
    void checkTemplate();
    void fillInsertMenu();
    bool run(const katana::core::Result<QString>& line);
    // RUN, PREVIEW or CLEAR of the chosen rules; the reply's counts go to
    // the report and its "rule=NAME labels=N" lines to the Labels column.
    bool ruleVerb(const QString& action);
    [[nodiscard]] katana::core::Result<QString> ruleOptions(bool everyField) const;
    [[nodiscard]] QString current() const;
    [[nodiscard]] QString freshName() const;

    katana::cad::Document& document_;
    CommandRunner run_;
    katana::cad::Document::ListenerHandle listener_;
    QTabWidget* tabs_ = nullptr;
    QListWidget* list_ = nullptr;
    QLineEdit* newName_ = nullptr;
    QComboBox* newKind_ = nullptr;
    QComboBox* kind_ = nullptr;
    QLineEdit* template_ = nullptr;
    QToolButton* insert_ = nullptr;
    QMenu* insertMenu_ = nullptr;
    QComboBox* textStyle_ = nullptr;
    QDoubleSpinBox* paper_ = nullptr;
    QComboBox* placement_ = nullptr;
    QComboBox* orientation_ = nullptr;
    QDoubleSpinBox* offset_ = nullptr;
    QCheckBox* leader_ = nullptr;
    QCheckBox* displace_ = nullptr;
    QSpinBox* priority_ = nullptr;
    QComboBox* marker_ = nullptr;
    QDoubleSpinBox* markerSize_ = nullptr;
    QDoubleSpinBox* minimumLength_ = nullptr;
    QDoubleSpinBox* interval_ = nullptr;
    QDoubleSpinBox* tick_ = nullptr;
    QDoubleSpinBox* tickLength_ = nullptr;
    QLabel* templateCheck_ = nullptr;
    QLabel* problem_ = nullptr;
    QString shown_;

    QTableWidget* rules_ = nullptr;
    QLineEdit* ruleName_ = nullptr;
    QComboBox* ruleStyle_ = nullptr;
    QLineEdit* ruleLayer_ = nullptr;
    QLineEdit* ruleCode_ = nullptr;
    QComboBox* ruleType_ = nullptr;
    QLineEdit* ruleLabelLayer_ = nullptr;
    QCheckBox* ruleEnabled_ = nullptr;
    QLabel* runReport_ = nullptr;
    // Labels per rule from the last run or preview, for the Labels column.
    QMap<QString, QString> labelCounts_;
    bool filling_ = false;
};

} // namespace katana::qt
