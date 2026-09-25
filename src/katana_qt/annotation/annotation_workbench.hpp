#pragma once

// The annotation front end in the Format menu (docs/annotation.md, "In the
// window"): Text Styles..., Label Styles and Rules..., and the plan view's
// annotation scale on the Format toolbar.
//
// Every one of them is a thin front end over katana_cad and katana_commands:
// a text style is changed by updateTextStyle, a rule run by
// annotation::autoLabel, the scale by Document::setAnnotationScale - each ONE
// undoable step, exactly what the command line's TEXTSTYLE, AUTOLABEL and
// ANNOSCALE make - so nothing here decides anything a script or an agent
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

class AnnotationWorkbench {
  public:
    AnnotationWorkbench(QWidget& window, katana::cad::Document& document, QMenu& menu,
                        QToolBar& toolBar);
    ~AnnotationWorkbench();
    AnnotationWorkbench(const AnnotationWorkbench&) = delete;
    AnnotationWorkbench& operator=(const AnnotationWorkbench&) = delete;

    TextStyleManagerDialog& showTextStyles();
    LabelStyleManagerDialog& showLabelStyles();
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

    QWidget& window_;
    katana::cad::Document& document_;
    QAction* textStylesAction_ = nullptr;
    QAction* labelStylesAction_ = nullptr;
    QComboBox* scale_ = nullptr;
    QPointer<TextStyleManagerDialog> textStyles_;
    QPointer<LabelStyleManagerDialog> labelStyles_;
    CommandRunner run_;
    struct Listener;
    std::unique_ptr<Listener> listener_;
};

} // namespace katana::qt
