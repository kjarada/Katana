#include "annotation/annotation_workbench.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include <QAction>
#include <QComboBox>
#include <QLineEdit>
#include <QMenu>
#include <QToolBar>
#include <QWidget>

#include "annotation/annotation_managers.hpp"
#include "annotation/leader_manager.hpp"
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
    delete leaders_.data();
    delete labelStyles_.data();
    delete textStyles_.data();
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

LeaderManagerDialog& AnnotationWorkbench::showLeaders(int tab)
{
    if (leaders_.isNull()) {
        leaders_ = new LeaderManagerDialog(document_, &window_);
        leaders_->setModal(false);
    }
    leaders_->showTab(static_cast<LeaderManagerDialog::Tab>(std::clamp(tab, 0, 2)));
    raise(*leaders_);
    return *leaders_;
}

void AnnotationWorkbench::addLeaderActions(QMenu& annotateMenu)
{
    // The letters the menu's tools have taken already (tools/tool_menus.cpp
    // gives them theirs), so each of these gets one of its own
    // (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless).
    std::string taken;
    for (const QAction* item : annotateMenu.actions()) {
        if (!item->isSeparator()) {
            (void)tools::withMnemonic(item->text().toStdString(), taken);
        }
    }
    struct Entry {
        const char* name;
        const char* text;
        const char* tip;
        int tab;
    };
    const Entry entries[] = {
        {"annotateLeaders", "Leaders...",
         "The drawing's leaders: a note read off the entity a leader points at - its "
         "attributes, level, length, chainage - the values to put in it, its look, and an "
         "attribute of that entity set through it (LEADER SET, LEADER PROP)",
         0},
        {"annotateLeadersForSelection", "Leaders for Selection...",
         "A leader, or a numbered balloon, to each selected entity, its note read off it "
         "(LEADER FOR, BALLOON FOR)",
         1},
        {"annotateArrangeLeaders", "Arrange Leaders and Balloons...",
         "Line the selected leaders' notes up in a column, and number the balloons again "
         "(LEADER ALIGN, BALLOON RENUMBER)",
         2},
    };
    annotateMenu.addSeparator();
    for (const Entry& entry : entries) {
        auto* action =
            new QAction(QString::fromStdString(tools::withMnemonic(entry.text, taken)), &window_);
        action->setObjectName(QString::fromLatin1(entry.name));
        action->setToolTip(QString::fromLatin1(entry.tip));
        action->setStatusTip(action->toolTip());
        action->setData(QStringLiteral("leaderManagerDialog"));
        const int tab = entry.tab;
        QObject::connect(action, &QAction::triggered, &window_, [this, tab] { showLeaders(tab); });
        annotateMenu.addAction(action);
    }
}

} // namespace katana::qt
