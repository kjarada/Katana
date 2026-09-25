#pragma once

// The annotation front end (docs/annotation.md, "In the window"): Text
// Styles... and Label Styles and Rules... in the Format menu, the plan
// view's annotation scale on the Format toolbar, and the Leaders manager in
// the Annotate menu (Leaders..., Leaders for Selection..., Arrange Leaders
// and Balloons...).
//
// Every one of them is a thin front end over katana_cad and katana_commands:
// a text style is changed by updateTextStyle, a rule run by
// annotation::autoLabel, the scale by Document::setAnnotationScale - each ONE
// undoable step, exactly what the command line's TEXTSTYLE, AUTOLABEL and
// ANNOSCALE make - so nothing here decides anything a script or an agent
// could not do the same way. Stable object names throughout, for the
// headless driver and the tests.

#include <memory>

#include <QPointer>

class QAction;
class QComboBox;
class QMenu;
class QToolBar;
class QWidget;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class TextStyleManagerDialog;
class LabelStyleManagerDialog;
class LeaderManagerDialog;

class AnnotationWorkbench {
  public:
    AnnotationWorkbench(QWidget& window, katana::cad::Document& document, QMenu& menu,
                        QToolBar& toolBar);
    ~AnnotationWorkbench();
    AnnotationWorkbench(const AnnotationWorkbench&) = delete;
    AnnotationWorkbench& operator=(const AnnotationWorkbench&) = delete;

    TextStyleManagerDialog& showTextStyles();
    LabelStyleManagerDialog& showLabelStyles();
    // The Leaders manager, on its tab: 0 Leader, 1 For Selection, 2 Arrange
    // (LeaderManagerDialog::Tab).
    LeaderManagerDialog& showLeaders(int tab = 0);

    // Adds the Leaders manager's three entries to the Annotate menu, after
    // its tools: "annotateLeaders", "annotateLeadersForSelection",
    // "annotateArrangeLeaders", each with a menu letter no item there has.
    void addLeaderActions(QMenu& annotateMenu);
    // The Format toolbar's scale box ("annotationScaleCombo").
    [[nodiscard]] QComboBox* scaleBox() const { return scale_; }

  private:
    // The box shows the document's scale; called whenever the document
    // changes, since an undo or an ANNOSCALE typed changes it too.
    void showScale();

    QWidget& window_;
    katana::cad::Document& document_;
    QAction* textStylesAction_ = nullptr;
    QAction* labelStylesAction_ = nullptr;
    QComboBox* scale_ = nullptr;
    QPointer<TextStyleManagerDialog> textStyles_;
    QPointer<LabelStyleManagerDialog> labelStyles_;
    QPointer<LeaderManagerDialog> leaders_;
    struct Listener;
    std::unique_ptr<Listener> listener_;
};

} // namespace katana::qt
