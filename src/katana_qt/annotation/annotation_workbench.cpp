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
#include "annotation/dimension_style_manager.hpp"
#include "annotation/text_edit_dialog.hpp"
#include "annotation/label_edit_dialog.hpp"
#include "annotation/label_layout_report.hpp"
#include "annotation/leader_manager.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "icons.hpp"
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
    // Menu letters T, E and I: the Format menu's others are L, Y, B, S, D,
    // R, G and P, and a letter that reaches two items reaches neither
    // (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless).
    textStylesAction_ = new QAction(QStringLiteral("&Text Styles..."), &window);
    textStylesAction_->setObjectName(QStringLiteral("formatTextStyles"));
    textStylesAction_->setIcon(katana::qt::icon(Icon::TextStyles));
    textStylesAction_->setToolTip(QStringLiteral(
        "The drawing's text styles: face, height on paper, width, slant, colour, background "
        "mask, readability (TEXTSTYLE)"));
    textStylesAction_->setStatusTip(textStylesAction_->toolTip());
    textStylesAction_->setData(QStringLiteral("textStyleManagerDialog"));
    labelStylesAction_ = new QAction(QStringLiteral("Lab&el Styles and Rules..."), &window);
    labelStylesAction_->setObjectName(QStringLiteral("formatLabelStyles"));
    labelStylesAction_->setIcon(katana::qt::icon(Icon::LabelStyles));
    labelStylesAction_->setToolTip(QStringLiteral(
        "Label styles - what a label says and where it goes - and the rules that label the "
        "drawing by layer, code and kind (LABELSTYLE, AUTOLABEL)"));
    labelStylesAction_->setStatusTip(labelStylesAction_->toolTip());
    labelStylesAction_->setData(QStringLiteral("labelStyleManagerDialog"));
    dimStylesAction_ = new QAction(QStringLiteral("D&imension Styles..."), &window);
    dimStylesAction_->setObjectName(QStringLiteral("formatDimensionStyles"));
    dimStylesAction_->setIcon(katana::qt::icon(Icon::DimensionStyles));
    dimStylesAction_->setToolTip(QStringLiteral(
        "The drawing's dimension styles: text, arrows, extension lines, units and paper sizing, "
        "and the layers that use each (DIMSTYLE)"));
    dimStylesAction_->setStatusTip(dimStylesAction_->toolTip());
    dimStylesAction_->setData(QStringLiteral("dimensionStyleManagerDialog"));
    QObject::connect(textStylesAction_, &QAction::triggered, &window, [this] { showTextStyles(); });
    QObject::connect(labelStylesAction_, &QAction::triggered, &window,
                     [this] { showLabelStyles(); });
    QObject::connect(dimStylesAction_, &QAction::triggered, &window,
                     [this] { showDimensionStyles(); });
    menu.addSection(QStringLiteral("Annotation Styles"));
    menu.addActions({textStylesAction_, labelStylesAction_, dimStylesAction_});

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
    delete textEdit_.data();
    delete dimStyles_.data();
    delete leaders_.data();
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
    const auto make = [&](Icon glyph, const char* text, const char* name, const char* dialog,
                          const QString& tip) {
        auto* action = new QAction(katana::qt::icon(glyph),
                                   QString::fromStdString(tools::withMnemonic(text, taken)),
                                   &window_);
        action->setObjectName(QString::fromLatin1(name));
        action->setToolTip(tip);
        action->setStatusTip(tip);
        // The dialog's object name, as the headless --dialog looks it up.
        action->setData(QString::fromLatin1(dialog));
        return action;
    };
    QAction* editLabel = make(Icon::EditLabel, "Edit Label...", "annotateEditLabel",
                              "labelEditDialog",
                              QStringLiteral("The selected label's style, own text, pinned place "
                                             "and layer (LABEL SET)"));
    QAction* labelLayout =
        make(Icon::LabelLayout, "Label Layout Report...", "annotateLabelLayout",
             "labelLayoutDialog",
             QStringLiteral("How many labels are placed, moved to find room, left without "
                            "room or label nothing at the annotation scale; selects those "
                            "without room (LABEL LAYOUT)"));
    QObject::connect(editLabel, &QAction::triggered, &window_, [this] { showEditLabel(); });
    QObject::connect(labelLayout, &QAction::triggered, &window_, [this] { showLabelLayout(); });
    // Edit Label joins Edit Text's section when the window put that item
    // last (addEditTextAction), so the editors sit together after the tools.
    const QList<QAction*> items = annotateMenu.actions();
    if (items.isEmpty() || items.back()->objectName() != QStringLiteral("annotateEditText")) {
        annotateMenu.addSection(QStringLiteral("Edit Annotation"));
    }
    annotateMenu.addActions({editLabel, labelLayout});
}

LabelEditDialog& AnnotationWorkbench::showEditLabel()
{
    if (editLabel_.isNull()) {
        editLabel_ = new LabelEditDialog(document_, lateRunner(), &window_);
        editLabel_->setModal(false);
    }
    (void)editLabel_->loadSelection();
    raise(*editLabel_);
    return *editLabel_;
}

LabelLayoutReportDialog& AnnotationWorkbench::showLabelLayout()
{
    if (labelLayout_.isNull()) {
        labelLayout_ = new LabelLayoutReportDialog(lateRunner(), &window_);
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
        textStyles_ = new TextStyleManagerDialog(document_, lateRunner(), &window_);
        textStyles_->setModal(false);
    }
    raise(*textStyles_);
    return *textStyles_;
}

DimensionStyleManagerDialog& AnnotationWorkbench::showDimensionStyles()
{
    if (dimStyles_.isNull()) {
        dimStyles_ = new DimensionStyleManagerDialog(document_, lateRunner(), &window_);
        dimStyles_->setModal(false);
    }
    raise(*dimStyles_);
    return *dimStyles_;
}

CommandRunner AnnotationWorkbench::lateRunner()
{
    return [this](const QString& line) {
        return run_ ? run_(line)
                    : VerbOutcome{false, {}, QStringLiteral("no command line to run on")};
    };
}

QAction* AnnotationWorkbench::addEditTextAction(QMenu& annotateMenu)
{
    // Menu letter X: the tools' letters are given as the catalogue fills the
    // menu, the first letters of words first, and X begins none of their
    // names (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless).
    auto* action = new QAction(QStringLiteral("Edit Te&xt..."), &window_);
    action->setObjectName(QStringLiteral("annotateEditText"));
    action->setIcon(katana::qt::icon(Icon::EditText));
    action->setToolTip(QStringLiteral(
        "Edit the selected text: its words over several lines, style, height on paper, "
        "justification, rotation and position (TEXTEDIT)"));
    action->setStatusTip(action->toolTip());
    action->setData(QStringLiteral("textEditDialog"));
    QObject::connect(action, &QAction::triggered, &window_, [this] { showTextEdit(); });
    annotateMenu.addSection(QStringLiteral("Edit Annotation"));
    annotateMenu.addAction(action);
    return action;
}

TextEditDialog& AnnotationWorkbench::showTextEdit()
{
    if (textEdit_.isNull()) {
        textEdit_ = new TextEditDialog(document_, lateRunner(), &window_);
        textEdit_->setModal(false);
    }
    raise(*textEdit_);
    return *textEdit_;
}

LabelStyleManagerDialog& AnnotationWorkbench::showLabelStyles()
{
    if (labelStyles_.isNull()) {
        labelStyles_ = new LabelStyleManagerDialog(document_, lateRunner(), &window_);
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
    // "Leader Manager", not "Leaders": the tools' own Leaders group is above.
    annotateMenu.addSection(QStringLiteral("Leader Manager"));
    for (const Entry& entry : entries) {
        auto* action =
            new QAction(QString::fromStdString(tools::withMnemonic(entry.text, taken)), &window_);
        action->setObjectName(QString::fromLatin1(entry.name));
        action->setIcon(katana::qt::icon(entry.tab == 0   ? Icon::LeaderManager
                                         : entry.tab == 1 ? Icon::LeadersForSelection
                                                          : Icon::ArrangeLeaders));
        action->setToolTip(QString::fromLatin1(entry.tip));
        action->setStatusTip(action->toolTip());
        action->setData(QStringLiteral("leaderManagerDialog"));
        const int tab = entry.tab;
        QObject::connect(action, &QAction::triggered, &window_, [this, tab] { showLeaders(tab); });
        annotateMenu.addAction(action);
    }
}

} // namespace katana::qt
