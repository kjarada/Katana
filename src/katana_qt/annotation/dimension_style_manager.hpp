#pragma once

// The Dimension Styles manager (Format > Dimension Styles...,
// docs/annotation.md "Dimension styles"): the drawing's dimension styles, how
// many layers use each, and a form for the chosen one with a preview of what
// a dimension reads as and a drawn sample.
//
// It changes nothing itself. Every button builds the DIMSTYLE line a person
// would type - with the field words of cad/annotation/dimension_style_verbs.hpp,
// so it cannot write a line the verb reads differently - and runs it through
// the window's one executor (command_runner.hpp): echoed in the command log,
// one undo step, refused in the verb's own words, which the problem line
// shows. Apply sends only the fields the form changed, as ONE line; with none
// changed it sends nothing and makes no step. The dialog keeps no copy of the
// table: it reloads from the document on every change, and keeps a form's
// unapplied edits only while the style they were made to is unchanged.
//
// Object names, for the headless driver and the tests:
//   dimensionStyleManagerDialog   the dialog
//   dimStyleList                  the styles: Style | Used by
//   dimStyleNewName               the name New and Duplicate give (blank: a
//                                 fresh one, shown as the placeholder)
//   dimStyleNew, dimStyleDuplicate, dimStyleDelete, dimStyleRevert, dimStyleApply
//   dimStyleText, dimStyleGap, dimStylePrefix, dimStyleSuffix           Text
//   dimStyleExtOff, dimStyleExtBeyond, dimStyleArrow, dimStyleHead      Lines and arrows
//   dimStyleScale, dimStyleDecimals, dimStyleRound, dimStyleTrim        Units
//   dimStylePaper                 paper-sized (drawn at the annotation scale)
//   dimStyleUnits                 what the sizes are in
//   dimStylePreview               "a dimension of 10 reads as ..."
//   dimStyleSample                the sample, drawn by the plan painter
//   dimStyleProblem               the last refusal, in the verb's words

#include <QDialog>
#include <QString>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/tables.hpp"

class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QSpinBox;
class QTableWidget;

namespace katana::qt {

class DimensionStyleManagerDialog final : public QDialog {
  public:
    DimensionStyleManagerDialog(katana::cad::Document& document, CommandRunner run,
                                QWidget* parent = nullptr);

    // Shows the style `name` (Standard when there is none of that name).
    void select(const QString& name);
    // The style the form shows.
    [[nodiscard]] QString current() const { return shown_; }
    // The form as a style; an error naming the field that does not read.
    [[nodiscard]] katana::core::Result<katana::entity::DimensionStyle> formStyle() const;
    // The line Apply runs: empty when the form changes nothing.
    [[nodiscard]] katana::core::Result<QString> applyLine() const;
    // What the last refused action said; empty after one that worked.
    [[nodiscard]] QString problem() const;
    [[nodiscard]] QString preview() const;

    // The buttons. Each is true when its line ran (or, for Apply, when there
    // was nothing to run).
    bool apply();
    bool newStyle();
    bool duplicateStyle();
    bool deleteStyle();
    // Throws the form's edits away.
    void revert();

  private:
    void refresh();
    void showStyle(const katana::entity::DimensionStyle& style);
    void showPreview();
    void showUnits();
    [[nodiscard]] QString nameForNew(const QString& base) const;
    // Runs `line`; its refusal, or the reason it could not be built, goes to
    // the problem line.
    bool run(const katana::core::Result<QString>& line);

    katana::cad::Document& document_;
    CommandRunner run_;
    katana::cad::Document::ListenerHandle listener_;
    QTableWidget* list_ = nullptr;
    QLineEdit* newName_ = nullptr;
    QLineEdit* text_ = nullptr;
    QLineEdit* gap_ = nullptr;
    QLineEdit* prefix_ = nullptr;
    QLineEdit* suffix_ = nullptr;
    QLineEdit* extOff_ = nullptr;
    QLineEdit* extBeyond_ = nullptr;
    QLineEdit* arrow_ = nullptr;
    QComboBox* head_ = nullptr;
    QLineEdit* scale_ = nullptr;
    QSpinBox* decimals_ = nullptr;
    QLineEdit* round_ = nullptr;
    QCheckBox* trim_ = nullptr;
    QCheckBox* paper_ = nullptr;
    QLabel* units_ = nullptr;
    QLabel* preview_ = nullptr;
    QLabel* sample_ = nullptr;
    QLabel* problem_ = nullptr;
    QString shown_;
    // The style as the form was last loaded with it: a refresh keeps the
    // form's edits while the stored style still equals this.
    katana::entity::DimensionStyle loaded_;
    bool loading_ = false;
};

} // namespace katana::qt
