#include "annotation/annotation_workbench.hpp"

#include <cmath>

#include <QAction>
#include <QComboBox>
#include <QLineEdit>
#include <QMenu>
#include <QToolBar>
#include <QWidget>

#include "annotation/annotation_managers.hpp"
#include "annotation/label_edit_dialog.hpp"
#include "annotation/label_layout_report.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/core/text.hpp"
#include "tools/tool_menus.hpp"

namespace katana::qt {

struct AnnotationWorkbench::Listener {
    katana::cad::Document::ListenerHandle handle;
};

namespace {

void raise(QWidget& dialog)
{
    dialog.show();
    dialog.raise();
    dialog.activateWindow();
}

QString scaleText(double scale)
{
    return QStringLiteral("1:") + QString::fromStdString(katana::core::formatExactReal(scale));
}

} // namespace

AnnotationWorkbench::AnnotationWorkbench(QWidget& window, katana::cad::Document& document,
                                         QMenu& menu, QToolBar& toolBar)
    : window_(window), document_(document), listener_(std::make_unique<Listener>())
{
    // Menu letters T and E: the Format menu's others are L, Y, B, S, D, R
    // and P, and a letter that reaches two items reaches neither
    // (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless).
    textStylesAction_ = new QAction(QStringLiteral("&Text Styles..."), &window);
    textStylesAction_->setObjectName(QStringLiteral("formatTextStyles"));
    textStylesAction_->setToolTip(QStringLiteral(
        "The drawing's text styles: face, height on paper, width, slant, colour, background "
        "mask, readability (TEXTSTYLE)"));
    textStylesAction_->setStatusTip(textStylesAction_->toolTip());
    textStylesAction_->setData(QStringLiteral("textStyleManagerDialog"));
    labelStylesAction_ = new QAction(QStringLiteral("Lab&el Styles and Rules..."), &window);
    labelStylesAction_->setObjectName(QStringLiteral("formatLabelStyles"));
    labelStylesAction_->setToolTip(QStringLiteral(
        "Label styles - what a label says and where it goes - and the rules that label the "
        "drawing by layer, code and kind (LABELSTYLE, AUTOLABEL)"));
    labelStylesAction_->setStatusTip(labelStylesAction_->toolTip());
    labelStylesAction_->setData(QStringLiteral("labelStyleManagerDialog"));
    QObject::connect(textStylesAction_, &QAction::triggered, &window, [this] { showTextStyles(); });
    QObject::connect(labelStylesAction_, &QAction::triggered, &window,
                     [this] { showLabelStyles(); });
    menu.addSeparator();
    menu.addActions({textStylesAction_, labelStylesAction_});

    // The plan view's annotation scale, where every drafter looks for it: an
    // editable box of the standard scales on the Format toolbar.
    scale_ = new QComboBox(&toolBar);
    scale_->setObjectName(QStringLiteral("annotationScaleCombo"));
    scale_->setToolTip(QStringLiteral(
        "Annotation scale: text, labels, leaders and paper-sized dimensions are drawn at their "
        "paper size for this scale in the plan view (ANNOSCALE)"));
    scale_->setEditable(true);
    scale_->setInsertPolicy(QComboBox::NoInsert);
    for (const double scale : katana::cad::kStandardScales) {
        scale_->addItem(scaleText(scale), scale);
    }
    toolBar.addWidget(scale_);
    const auto commit = [this] {
        // "1:500" or "500".
        QString text = scale_->currentText().trimmed();
        if (text.startsWith(QStringLiteral("1:"))) {
            text = text.mid(2);
        }
        const auto value = katana::core::parseFiniteDouble(text.toStdString());
        if (!value || !(*value > 0.0) || !document_.setAnnotationScale(*value)) {
            showScale(); // refused: show what the document still has
        }
    };
    QObject::connect(scale_, &QComboBox::activated, &window, commit);
    QObject::connect(scale_->lineEdit(), &QLineEdit::editingFinished, &window, commit);
    listener_->handle = document_.addListener([this](const katana::cad::DocumentChange& change) {
        if (change.has(katana::cad::DocumentChange::History | katana::cad::DocumentChange::Replaced |
                       katana::cad::DocumentChange::Metadata)) {
            showScale();
        }
    });
    showScale();
}

AnnotationWorkbench::~AnnotationWorkbench()
{
    // Here, not left to the window's children: the dialogs hold the
    // Document, which goes when the window's members do - before Qt deletes
    // its children (CustomisationWorkbench's destructor says the same).
    delete labelLayout_.data();
    delete editLabel_.data();
    delete labelStyles_.data();
    delete textStyles_.data();
}

void AnnotationWorkbench::addLabelActions(QMenu& annotateMenu)
{
    // The letters the tools' items took (withMnemonic adds an item's own
    // letter to `taken`), so each of these reaches one item
    // (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless).
    std::string taken;
    for (const QAction* item : annotateMenu.actions()) {
        if (!item->isSeparator()) {
            (void)tools::withMnemonic(item->text().toStdString(), taken);
        }
    }
    const auto make = [&](const char* text, const char* name, const char* dialog,
                          const QString& tip) {
        auto* action =
            new QAction(QString::fromStdString(tools::withMnemonic(text, taken)), &window_);
        action->setObjectName(QString::fromLatin1(name));
        action->setToolTip(tip);
        action->setStatusTip(tip);
        // The dialog's object name, as the headless --dialog looks it up.
        action->setData(QString::fromLatin1(dialog));
        return action;
    };
    QAction* editLabel = make("Edit Label...", "annotateEditLabel", "labelEditDialog",
                              QStringLiteral("The selected label's style, own text, pinned place "
                                             "and layer (LABEL SET)"));
    QAction* labelLayout =
        make("Label Layout Report...", "annotateLabelLayout", "labelLayoutDialog",
             QStringLiteral("How many labels are placed, moved to find room, left without "
                            "room or label nothing at the annotation scale; selects those "
                            "without room (LABEL LAYOUT)"));
    QObject::connect(editLabel, &QAction::triggered, &window_, [this] { showEditLabel(); });
    QObject::connect(labelLayout, &QAction::triggered, &window_, [this] { showLabelLayout(); });
    annotateMenu.addSeparator();
    annotateMenu.addActions({editLabel, labelLayout});
}

CommandRunner AnnotationWorkbench::deferredRunner()
{
    // The runner the window gives, read when a line runs rather than when a
    // dialog is made: a dialog may be made before the window has one.
    return [this](const QString& line) {
        return run_ ? run_(line)
                    : VerbOutcome{false, {},
                                  QStringLiteral("There is no command line to run it on.")};
    };
}

LabelEditDialog& AnnotationWorkbench::showEditLabel()
{
    if (editLabel_.isNull()) {
        editLabel_ = new LabelEditDialog(document_, deferredRunner(), &window_);
        editLabel_->setModal(false);
    }
    (void)editLabel_->loadSelection();
    raise(*editLabel_);
    return *editLabel_;
}

LabelLayoutReportDialog& AnnotationWorkbench::showLabelLayout()
{
    if (labelLayout_.isNull()) {
        labelLayout_ = new LabelLayoutReportDialog(deferredRunner(), &window_);
        labelLayout_->setModal(false);
    }
    raise(*labelLayout_);
    (void)labelLayout_->run();
    return *labelLayout_;
}

void AnnotationWorkbench::showScale()
{
    const QString text = scaleText(document_.annotationScale());
    if (scale_->currentText() != text) {
        const int index = scale_->findText(text);
        if (index >= 0) {
            scale_->setCurrentIndex(index);
        } else {
            scale_->setEditText(text);
        }
    }
}

TextStyleManagerDialog& AnnotationWorkbench::showTextStyles()
{
    if (textStyles_.isNull()) {
        textStyles_ = new TextStyleManagerDialog(document_, &window_);
        textStyles_->setModal(false);
    }
    raise(*textStyles_);
    return *textStyles_;
}

LabelStyleManagerDialog& AnnotationWorkbench::showLabelStyles()
{
    if (labelStyles_.isNull()) {
        labelStyles_ = new LabelStyleManagerDialog(document_, &window_);
        labelStyles_->setModal(false);
    }
    raise(*labelStyles_);
    return *labelStyles_;
}

} // namespace katana::qt
