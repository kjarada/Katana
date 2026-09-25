#pragma once

// The annotation front end in the Format menu (docs/annotation.md, "In the
// window"): Text Styles..., Label Styles and Rules..., Dimension Styles...,
// the plan view's annotation scale on the Format toolbar, and Annotate >
// Edit Text....
//
// No dialog here changes the drawing itself: each builds the line a person
// would type - TEXTSTYLE, LABELSTYLE, AUTOLABEL, DIMSTYLE, TEXTEDIT - and
// runs it through the window's one executor (commandRunner), and the scale
// box sets the scale as ANNOSCALE does, by Document::setAnnotationScale - each
// ONE undoable step, so nothing here decides anything a script or an agent
// could not do the same way. Stable object names throughout, for the
// headless driver and the tests.

#include <memory>
#include <utility>

#include <QPointer>

#include "command_runner.hpp"

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
class DimensionStyleManagerDialog;
class TextEditDialog;

class AnnotationWorkbench {
  public:
    AnnotationWorkbench(QWidget& window, katana::cad::Document& document, QMenu& menu,
                        QToolBar& toolBar);
    ~AnnotationWorkbench();
    AnnotationWorkbench(const AnnotationWorkbench&) = delete;
    AnnotationWorkbench& operator=(const AnnotationWorkbench&) = delete;

    TextStyleManagerDialog& showTextStyles();
    LabelStyleManagerDialog& showLabelStyles();
    DimensionStyleManagerDialog& showDimensionStyles();
    // Annotate > Edit Text... ("annotateEditText", its data the dialog's name
    // "textEditDialog"), put at the end of `annotateMenu`: the window fills
    // that menu with the tools before this workbench exists, so it hands the
    // menu over here. The dialog edits the one text selected.
    QAction* addEditTextAction(QMenu& annotateMenu);
    TextEditDialog& showTextEdit();
    // The Format toolbar's scale box ("annotationScaleCombo").
    [[nodiscard]] QComboBox* scaleBox() const { return scale_; }

    // The window's one executor (command_runner.hpp), for the annotation
    // dialogs that change the drawing through a verb line. Set by the window
    // once its command line exists; empty until then, and in a test that
    // gives none.
    void setCommandRunner(CommandRunner run) { run_ = std::move(run); }
    [[nodiscard]] const CommandRunner& commandRunner() const { return run_; }

  private:
    // The box shows the document's scale; called whenever the document
    // changes, since an undo or an ANNOSCALE typed changes it too.
    void showScale();
    // What a dialog made here runs its lines through: the runner as it is
    // when a line is run, not as it was when the dialog was made - the window
    // sets it after building the menus - and a refusal naming the want of
    // one without it.
    [[nodiscard]] CommandRunner lateRunner();

    QWidget& window_;
    katana::cad::Document& document_;
    QAction* textStylesAction_ = nullptr;
    QAction* labelStylesAction_ = nullptr;
    QAction* dimStylesAction_ = nullptr;
    QComboBox* scale_ = nullptr;
    QPointer<TextStyleManagerDialog> textStyles_;
    QPointer<LabelStyleManagerDialog> labelStyles_;
    QPointer<DimensionStyleManagerDialog> dimStyles_;
    QPointer<TextEditDialog> textEdit_;
    CommandRunner run_;
    struct Listener;
    std::unique_ptr<Listener> listener_;
};

} // namespace katana::qt
