#include "annotation/leader_manager.hpp"

#include <algorithm>
#include <initializer_list>
#include <string>
#include <variant>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSignalBlocker>
#include <QSpinBox>
#include <QTabWidget>
#include <QTableWidget>
#include <QTextCursor>
#include <QTextDocument>
#include <QVBoxLayout>

#include "annotation/form_widgets.hpp"
#include "customisation/document_watcher.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/math/numerics.hpp"

namespace katana::qt {

namespace ann = katana::cad::annotation;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using namespace annotation_form;

namespace {

using Kind = ann::LeaderNote::Kind;

constexpr katana::entity::ArrowHead kArrows[] = {
    katana::entity::ArrowHead::ClosedFilled, katana::entity::ArrowHead::Open,
    katana::entity::ArrowHead::Tick, katana::entity::ArrowHead::Dot,
    katana::entity::ArrowHead::None};
constexpr katana::entity::CalloutShape kCallouts[] = {katana::entity::CalloutShape::None,
                                                      katana::entity::CalloutShape::Box,
                                                      katana::entity::CalloutShape::Circle};
// The data of a For Selection combo entry that leaves the choice to the
// leader: an arrow that is a dot inside an outline, a balloon's circle.
constexpr int kAutomatic = -1;
// A leader's sizes: millimetres on paper, to 3 decimals and up to a metre.
// Wider than any drawing needs, so a size typed here is kept as typed; one
// the form cannot hold exactly is kept as it is by not being sent (formChange).
constexpr double kLargestSize = 1000.0;
constexpr int kSizeDecimals = 3;

constexpr const char* kAttachTip =
    "Put the tip on the selected entity, at the place of it nearest the tip: select the leader "
    "and the entity, then press (LEADER ATTACH)";
constexpr const char* kFreezeTip =
    "Keep the words the note says now; the tip still follows (LEADER FREEZE)";
constexpr const char* kDetachTip =
    "Let the tip go of the entity it is on, freezing a smart note first (LEADER DETACH)";

// Text, Template, Label style: what a note is.
QComboBox* noteKindBox(QWidget* parent, const char* name)
{
    auto* box = new QComboBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    box->addItem(QStringLiteral("Text"), static_cast<int>(Kind::Text));
    box->addItem(QStringLiteral("Template (read off the entity)"),
                 static_cast<int>(Kind::Template));
    box->addItem(QStringLiteral("Label style's template"), static_cast<int>(Kind::LabelStyle));
    return box;
}

QPlainTextEdit* noteEdit(QWidget* parent, const char* name, const QString& placeholder)
{
    auto* edit = new QPlainTextEdit(parent);
    edit->setObjectName(QString::fromLatin1(name));
    edit->setPlaceholderText(placeholder);
    edit->setTabChangesFocus(true);
    edit->setMaximumHeight(96);
    return edit;
}

QTableWidget* valueTable(QWidget* parent, const char* name)
{
    auto* table = new QTableWidget(0, 2, parent);
    table->setObjectName(QString::fromLatin1(name));
    table->setHorizontalHeaderLabels({QStringLiteral("Value"), QStringLiteral("Here")});
    table->horizontalHeader()->setStretchLastSection(true);
    table->verticalHeader()->setVisible(false);
    table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table->setSelectionBehavior(QAbstractItemView::SelectRows);
    table->setToolTip(QStringLiteral(
        "What the entity offers where the tip is; double-click one to put it in the note"));
    table->setMinimumHeight(160);
    table->setColumnWidth(0, 120);
    return table;
}

void fillValues(QTableWidget* table, const std::vector<ann::LeaderValueRow>& rows)
{
    table->setRowCount(static_cast<int>(rows.size()));
    for (std::size_t i = 0; i < rows.size(); ++i) {
        table->setItem(static_cast<int>(i), 0,
                       new QTableWidgetItem(QString::fromStdString(rows[i].name)));
        table->setItem(static_cast<int>(i), 1,
                       new QTableWidgetItem(QString::fromStdString(rows[i].text)));
    }
}

// The note as typed, every character kept: toPlainText() would turn a
// non-breaking space into a plain one, which the note then says.
QString plainText(const QPlainTextEdit* edit)
{
    QString text = edit->document()->toRawText();
    text.replace(QChar::ParagraphSeparator, QLatin1Char('\n'));
    text.replace(QChar::LineSeparator, QLatin1Char('\n'));
    return text;
}

// Fills `edit` with `text`, its signals held, and its cursor at the END -
// where a double-clicked value goes unless the user puts it elsewhere.
void setNoteText(QPlainTextEdit* edit, const QString& text)
{
    const QSignalBlocker blocker(edit);
    edit->setPlainText(text);
    QTextCursor cursor = edit->textCursor();
    cursor.movePosition(QTextCursor::End);
    edit->setTextCursor(cursor);
}

// The label styles, by name, into `box`, keeping its choice where it can.
void fillLabelStyles(QComboBox* box, const katana::entity::Model& model)
{
    const QString keep = box->currentText();
    const QSignalBlocker blocker(box);
    box->clear();
    model.labelStyles.forEach([&](const katana::entity::LabelStyle& style) {
        box->addItem(QString::fromStdString(style.name));
    });
    box->setCurrentIndex(std::max(0, box->findText(keep)));
}

// The text styles into `box`: `first` for none (data ""), then each by name.
void fillTextStyles(QComboBox* box, const katana::entity::Model& model, const QString& first)
{
    const QString keep = box->currentData().toString();
    const QSignalBlocker blocker(box);
    box->clear();
    box->addItem(first, QString());
    model.textStyles.forEach([&](const katana::entity::TextStyle& style) {
        box->addItem(QString::fromStdString(style.name), QString::fromStdString(style.name));
    });
    box->setCurrentIndex(std::max(0, box->findData(keep)));
}

// The note a note-kind box and its editor and style box say.
ann::LeaderNote noteFrom(const QComboBox* kind, const QPlainTextEdit* text, const QComboBox* style)
{
    const auto chosenKind = static_cast<Kind>(kind->currentData().toInt());
    if (chosenKind == Kind::LabelStyle) {
        return ann::LeaderNote{chosenKind, style->currentText().toStdString()};
    }
    return ann::LeaderNote{chosenKind, plainText(text).toStdString()};
}

// Puts "{name}" into `edit` at its cursor, and makes `kind` Template.
void insertField(QComboBox* kind, QPlainTextEdit* edit, const QString& name)
{
    if (static_cast<Kind>(kind->currentData().toInt()) != Kind::Template) {
        kind->setCurrentIndex(kind->findData(static_cast<int>(Kind::Template)));
    }
    edit->insertPlainText(QStringLiteral("{") + name + QStringLiteral("}"));
    edit->setFocus();
}

// The first line of what a leader says, for the list.
QString listText(EntityId id, const std::string& says)
{
    QString first = QString::fromStdString(says.substr(0, says.find('\n')));
    if (says.find('\n') != std::string::npos) {
        first += QStringLiteral(" ...");
    }
    return QString::number(id) + QStringLiteral("  ") +
           (first.isEmpty() ? QStringLiteral("(no note)") : first);
}

// Where on its entity a tip is, in words: "50% along segment 2", "inside
// it", "at its end". The command line's terse "along 1 0.5" is
// entity::describe's; this is for reading.
QString placeText(const katana::entity::AnchorRef& ref, bool polyline)
{
    using katana::entity::AnchorPoint;
    switch (ref.point) {
    case AnchorPoint::Position:
        return QStringLiteral("at its position");
    case AnchorPoint::Start:
        return QStringLiteral("at its start");
    case AnchorPoint::End:
        return QStringLiteral("at its end");
    case AnchorPoint::Mid:
        return QStringLiteral("at its middle");
    case AnchorPoint::Centre:
        return QStringLiteral("at its centre");
    case AnchorPoint::Vertex:
        return QStringLiteral("at vertex ") + QString::number(ref.index + 1);
    case AnchorPoint::SegmentMid:
        return QStringLiteral("at the middle of segment ") + QString::number(ref.index + 1);
    case AnchorPoint::Along:
        return QString::number(std::clamp(ref.parameter, 0.0, 1.0) * 100.0, 'f', 1) +
               QStringLiteral("% along") +
               (polyline ? QStringLiteral(" segment ") + QString::number(ref.index + 1)
                         : QStringLiteral(" it"));
    case AnchorPoint::Inside:
        return QStringLiteral("inside it");
    }
    return {};
}

Status runCommand(katana::cad::Document& document, katana::commands::CommandPtr command)
{
    return command ? document.execute(std::move(command)) : Status{};
}

} // namespace

LeaderManagerDialog::LeaderManagerDialog(katana::cad::Document& document, QWidget* parent)
    : QDialog(parent), document_(document)
{
    setObjectName(QStringLiteral("leaderManagerDialog"));
    setWindowTitle(QStringLiteral("Leaders"));
    tabs_ = new QTabWidget(this);
    tabs_->setObjectName(QStringLiteral("leaderTabs"));
    problem_ = problemLine(this, "leaderProblem");

    // ---- Leader ---------------------------------------------------------------------------
    auto* leaderTab = new QWidget(tabs_);
    leaderTab->setObjectName(QStringLiteral("leaderTab"));
    list_ = new QListWidget(leaderTab);
    list_->setObjectName(QStringLiteral("leaderList"));
    list_->setMinimumWidth(200);
    scope_ = new QLabel(leaderTab);
    scope_->setObjectName(QStringLiteral("leaderScope"));
    scope_->setWordWrap(true);
    target_ = new QLabel(leaderTab);
    target_->setObjectName(QStringLiteral("leaderTarget"));
    target_->setWordWrap(true);
    along_ = spin(leaderTab, "leaderAlong", 0.0, 100.0, 5.0, 3, QStringLiteral(" %"));
    along_->setToolTip(QStringLiteral(
        "How far along what it is on the tip is: of the line or the arc from its start, of "
        "the polyline segment, of a turn of the circle (LEADER SET tip=#id@x,y)"));
    noteKind_ = noteKindBox(leaderTab, "leaderNoteKind");
    note_ = noteEdit(leaderTab, "leaderNote", QStringLiteral("IL {prop.invert:.3f}"));
    labelStyle_ = new QComboBox(leaderTab);
    labelStyle_->setObjectName(QStringLiteral("leaderLabelStyle"));
    check_ = new QLabel(leaderTab);
    check_->setObjectName(QStringLiteral("leaderTemplateCheck"));
    check_->setWordWrap(true);
    preview_ = new QLabel(leaderTab);
    preview_->setObjectName(QStringLiteral("leaderPreview"));
    preview_->setWordWrap(true);
    preview_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    values_ = valueTable(leaderTab, "leaderValues");
    arrow_ = combo(leaderTab, "leaderArrow", kArrows);
    callout_ = combo(leaderTab, "leaderCallout", kCallouts);
    textStyle_ = new QComboBox(leaderTab);
    textStyle_->setObjectName(QStringLiteral("leaderTextStyle"));
    paperHeight_ = spin(leaderTab, "leaderPaperHeight", 0.0, kLargestSize, 0.5, kSizeDecimals,
                        QStringLiteral(" mm"));
    paperHeight_->setSpecialValueText(QStringLiteral("the text style's"));
    arrowSize_ = spin(leaderTab, "leaderArrowSize", 0.0, kLargestSize, 0.5, kSizeDecimals,
                      QStringLiteral(" mm"));
    landing_ = spin(leaderTab, "leaderLanding", 0.0, kLargestSize, 0.5, kSizeDecimals,
                    QStringLiteral(" mm"));
    landing_->setSpecialValueText(QStringLiteral("none"));
    attributeName_ = edit(leaderTab, "leaderAttributeName", QStringLiteral("invert"));
    attributeName_->setMinimumWidth(110);
    attributeValue_ = edit(leaderTab, "leaderAttributeValue", QStringLiteral("10.25"));
    attributeValue_->setMinimumWidth(110);
    attributeType_ = new QComboBox(leaderTab);
    attributeType_->setObjectName(QStringLiteral("leaderAttributeType"));
    attributeType_->addItem(QStringLiteral("as typed"), QString());
    for (const char* type : {"text", "integer", "real", "boolean"}) {
        attributeType_->addItem(QString::fromLatin1(type), QString::fromLatin1(type));
    }

    attach_ = button(leaderTab, "leaderAttachSelected", QStringLiteral("Attach to Selected"));
    auto* onRow = new QHBoxLayout;
    onRow->addWidget(target_, 1);
    onRow->addWidget(attach_);
    auto* noteForm = new QFormLayout;
    noteForm->addRow(scope_);
    noteForm->addRow(QStringLiteral("On"), onRow);
    noteForm->addRow(QStringLiteral("Along"), along_);
    noteForm->addRow(QStringLiteral("Note is"), noteKind_);
    noteForm->addRow(QStringLiteral("Note"), note_);
    noteForm->addRow(QStringLiteral("Label style"), labelStyle_);
    noteForm->addRow(check_);
    noteForm->addRow(QStringLiteral("Says"), preview_);
    auto* lookForm = new QFormLayout;
    lookForm->addRow(QStringLiteral("Arrow"), arrow_);
    lookForm->addRow(QStringLiteral("Callout"), callout_);
    lookForm->addRow(QStringLiteral("Text style"), textStyle_);
    lookForm->addRow(QStringLiteral("Text height"), paperHeight_);
    lookForm->addRow(QStringLiteral("Arrow size"), arrowSize_);
    lookForm->addRow(QStringLiteral("Landing"), landing_);
    auto* look = new QGroupBox(QStringLiteral("Look"), leaderTab);
    look->setLayout(lookForm);

    attributeSet_ = button(leaderTab, "leaderAttributeSet", QStringLiteral("Set"));
    attributeSet_->setToolTip(
        QStringLiteral("Set this attribute on the entity the tip is on (LEADER PROP)"));
    attributeRemove_ = button(leaderTab, "leaderAttributeRemove", QStringLiteral("Remove"));
    auto* attributeRow = new QHBoxLayout;
    attributeRow->addWidget(attributeName_, 2);
    attributeRow->addWidget(attributeValue_, 2);
    attributeRow->addWidget(attributeType_, 1);
    attributeRow->addWidget(attributeSet_);
    attributeRow->addWidget(attributeRemove_);
    auto* attribute = new QGroupBox(QStringLiteral("Attribute of the entity it is on"), leaderTab);
    attribute->setLayout(attributeRow);

    freeze_ = button(leaderTab, "leaderFreeze", QStringLiteral("Freeze Note"));
    detach_ = button(leaderTab, "leaderDetach", QStringLiteral("Detach Tip"));
    apply_ = button(leaderTab, "leaderApply", QStringLiteral("Apply"));
    auto* leaderButtons = new QHBoxLayout;
    leaderButtons->addWidget(freeze_);
    leaderButtons->addWidget(detach_);
    leaderButtons->addStretch();
    leaderButtons->addWidget(apply_);

    // The values beside the look: what can go in the note next to how it is
    // drawn.
    auto* middle = new QHBoxLayout;
    middle->addWidget(values_, 3);
    middle->addWidget(look, 2);
    auto* leaderRight = new QVBoxLayout;
    leaderRight->addLayout(noteForm);
    leaderRight->addLayout(middle, 1);
    leaderRight->addWidget(attribute);
    leaderRight->addLayout(leaderButtons);
    auto* leaderLayout = new QHBoxLayout(leaderTab);
    leaderLayout->addWidget(list_, 1);
    leaderLayout->addLayout(leaderRight, 3);
    tabs_->addTab(leaderTab, QStringLiteral("Leader"));

    // ---- For Selection --------------------------------------------------------------------
    auto* forTab = new QWidget(tabs_);
    forTab->setObjectName(QStringLiteral("leaderForTab"));
    forSelection_ = new QLabel(forTab);
    forSelection_->setObjectName(QStringLiteral("leaderForSelection"));
    forNoteKind_ = noteKindBox(forTab, "leaderForNoteKind");
    forNote_ = noteEdit(forTab, "leaderForNote", QStringLiteral("{type} {id}"));
    forLabelStyle_ = new QComboBox(forTab);
    forLabelStyle_->setObjectName(QStringLiteral("leaderForLabelStyle"));
    forCheck_ = new QLabel(forTab);
    forCheck_->setObjectName(QStringLiteral("leaderForCheck"));
    forCheck_->setWordWrap(true);
    forPreview_ = new QLabel(forTab);
    forPreview_->setObjectName(QStringLiteral("leaderForPreview"));
    forPreview_->setWordWrap(true);
    forPreview_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    forValues_ = valueTable(forTab, "leaderForValues");
    forArrow_ = new QComboBox(forTab);
    forArrow_->setObjectName(QStringLiteral("leaderForArrow"));
    forArrow_->addItem(QStringLiteral("Automatic (a dot inside an outline)"), kAutomatic);
    for (const auto head : kArrows) {
        forArrow_->addItem(QString::fromUtf8(katana::entity::toString(head)),
                           static_cast<int>(head));
    }
    forCallout_ = new QComboBox(forTab);
    forCallout_->setObjectName(QStringLiteral("leaderForCallout"));
    forCallout_->addItem(QStringLiteral("Automatic (a circle for a balloon)"), kAutomatic);
    for (const auto shape : kCallouts) {
        forCallout_->addItem(QString::fromUtf8(katana::entity::toString(shape)),
                             static_cast<int>(shape));
    }
    forTextStyle_ = new QComboBox(forTab);
    forTextStyle_->setObjectName(QStringLiteral("leaderForTextStyle"));
    forPaperHeight_ = spin(forTab, "leaderForPaperHeight", 0.0, kLargestSize, 0.5, kSizeDecimals,
                           QStringLiteral(" mm"));
    forPaperHeight_->setSpecialValueText(QStringLiteral("the text style's"));
    // A new leader's own sizes to start from; sent only when changed.
    forArrowSize_ = spin(forTab, "leaderForArrowSize", 0.0, kLargestSize, 0.5, kSizeDecimals,
                         QStringLiteral(" mm"));
    forArrowSize_->setValue(LeaderGeometry{}.arrowSize);
    forLanding_ = spin(forTab, "leaderForLanding", 0.0, kLargestSize, 0.5, kSizeDecimals,
                       QStringLiteral(" mm"));
    forLanding_->setSpecialValueText(QStringLiteral("none"));
    forLanding_->setValue(LeaderGeometry{}.landing);
    forAngle_ = spin(forTab, "leaderForAngle", -360.0, 360.0, 15.0, 1, QStringLiteral(" deg"));
    forAngle_->setValue(45.0);
    forLength_ = spin(forTab, "leaderForLength", 0.5, 500.0, 1.0, 1, QStringLiteral(" mm"));
    forLength_->setValue(10.0);
    forBalloon_ =
        check(forTab, "leaderForBalloon", QStringLiteral("Balloons (numbered when no note)"));
    forReport_ = new QLabel(forTab);
    forReport_->setObjectName(QStringLiteral("leaderForReport"));
    forReport_->setWordWrap(true);
    auto* makeButton = button(forTab, "leaderForMake", QStringLiteral("Make for Selection"));
    auto* forForm = new QFormLayout;
    forForm->addRow(forSelection_);
    forForm->addRow(QStringLiteral("Note is"), forNoteKind_);
    forForm->addRow(QStringLiteral("Note"), forNote_);
    forForm->addRow(QStringLiteral("Label style"), forLabelStyle_);
    forForm->addRow(forCheck_);
    forForm->addRow(QStringLiteral("Says"), forPreview_);
    auto* forLookForm = new QFormLayout;
    forLookForm->addRow(QStringLiteral("Arrow"), forArrow_);
    forLookForm->addRow(QStringLiteral("Callout"), forCallout_);
    forLookForm->addRow(QStringLiteral("Text style"), forTextStyle_);
    forLookForm->addRow(QStringLiteral("Text height"), forPaperHeight_);
    forLookForm->addRow(QStringLiteral("Arrow size"), forArrowSize_);
    forLookForm->addRow(QStringLiteral("Landing"), forLanding_);
    forLookForm->addRow(QStringLiteral("Note direction"), forAngle_);
    forLookForm->addRow(QStringLiteral("Note distance on paper"), forLength_);
    forLookForm->addRow(forBalloon_);
    auto* forLook = new QGroupBox(QStringLiteral("Look"), forTab);
    forLook->setLayout(forLookForm);
    auto* forMiddle = new QHBoxLayout;
    forMiddle->addWidget(forValues_, 3);
    forMiddle->addWidget(forLook, 2);
    auto* forLayout = new QVBoxLayout(forTab);
    forLayout->addLayout(forForm);
    forLayout->addLayout(forMiddle, 1);
    forLayout->addWidget(forReport_);
    forLayout->addWidget(makeButton, 0, Qt::AlignRight);
    tabs_->addTab(forTab, QStringLiteral("For Selection"));

    // ---- Arrange --------------------------------------------------------------------------
    auto* arrangeTab = new QWidget(tabs_);
    arrangeTab->setObjectName(QStringLiteral("leaderArrangeTab"));
    alignUseX_ = check(arrangeTab, "leaderAlignUseX", QStringLiteral("At x"));
    alignX_ = spin(arrangeTab, "leaderAlignX", -1.0e9, 1.0e9, 1.0, 3);
    alignUseSpacing_ = check(arrangeTab, "leaderAlignUseSpacing", QStringLiteral("Stacked"));
    alignSpacing_ =
        spin(arrangeTab, "leaderAlignSpacing", 0.5, 500.0, 1.0, 1, QStringLiteral(" mm apart"));
    alignSpacing_->setValue(8.0);
    auto* alignButton = button(arrangeTab, "leaderAlign", QStringLiteral("Align Selected Leaders"));
    auto* alignForm = new QFormLayout;
    alignForm->addRow(alignUseX_, alignX_);
    alignForm->addRow(alignUseSpacing_, alignSpacing_);
    alignForm->addRow(alignButton);
    auto* alignBox =
        new QGroupBox(QStringLiteral("Line the notes up (the topmost sets x)"), arrangeTab);
    alignBox->setLayout(alignForm);
    renumberStart_ = new QSpinBox(arrangeTab);
    renumberStart_->setObjectName(QStringLiteral("balloonRenumberStart"));
    renumberStart_->setRange(-1000000, 1000000);
    renumberStart_->setValue(1);
    renumberOrder_ = new QComboBox(arrangeTab);
    renumberOrder_->setObjectName(QStringLiteral("balloonRenumberOrder"));
    renumberOrder_->addItem(QStringLiteral("As made"), static_cast<int>(ann::BalloonOrder::Id));
    renumberOrder_->addItem(QStringLiteral("Across, left to right"),
                            static_cast<int>(ann::BalloonOrder::X));
    renumberOrder_->addItem(QStringLiteral("Down, top first"),
                            static_cast<int>(ann::BalloonOrder::Y));
    auto* renumberButton =
        button(arrangeTab, "balloonRenumber", QStringLiteral("Renumber Balloons"));
    auto* renumberForm = new QFormLayout;
    renumberForm->addRow(QStringLiteral("From"), renumberStart_);
    renumberForm->addRow(QStringLiteral("Order"), renumberOrder_);
    renumberForm->addRow(renumberButton);
    auto* renumberBox = new QGroupBox(QStringLiteral("Number the balloons again"), arrangeTab);
    renumberBox->setLayout(renumberForm);
    arrangeReport_ = new QLabel(arrangeTab);
    arrangeReport_->setObjectName(QStringLiteral("leaderArrangeReport"));
    arrangeReport_->setWordWrap(true);
    auto* arrangeLayout = new QVBoxLayout(arrangeTab);
    arrangeLayout->addWidget(alignBox);
    arrangeLayout->addWidget(renumberBox);
    arrangeLayout->addWidget(arrangeReport_);
    arrangeLayout->addStretch();
    tabs_->addTab(arrangeTab, QStringLiteral("Arrange"));

    auto* layout = new QVBoxLayout(this);
    layout->addWidget(tabs_, 1);
    layout->addWidget(problem_);
    resize(960, 680);

    // ---- behaviour ------------------------------------------------------------------------
    // The list and the tables show; they never run a command (docs/desktop.md).
    connect(list_, &QListWidget::currentItemChanged, this,
            [this](QListWidgetItem* item, QListWidgetItem*) {
                if (item != nullptr) {
                    (void)showLeader(item->data(Qt::UserRole).toULongLong());
                }
            });
    connect(values_, &QTableWidget::cellDoubleClicked, this, [this](int row, int) {
        if (const QTableWidgetItem* item = values_->item(row, 0)) {
            insertValue(item->text());
        }
    });
    connect(forValues_, &QTableWidget::cellDoubleClicked, this, [this](int row, int) {
        if (const QTableWidgetItem* item = forValues_->item(row, 0)) {
            insertForValue(item->text());
        }
    });
    // Chosen by the user; showForm holds these signals while it fills the form.
    connect(noteKind_, &QComboBox::currentIndexChanged, this, [this] { noteKindChosen(); });
    connect(labelStyle_, &QComboBox::currentIndexChanged, this, [this] {
        updateNoteKind();
        lendStyleLook();
        updatePreview();
    });
    connect(note_, &QPlainTextEdit::textChanged, this, [this] { updatePreview(); });
    for (QComboBox* box : {arrow_, callout_, textStyle_}) {
        connect(box, &QComboBox::currentIndexChanged, this, [this] { updatePreview(); });
    }
    for (QDoubleSpinBox* box : {along_, paperHeight_, arrowSize_, landing_}) {
        connect(box, &QDoubleSpinBox::valueChanged, this, [this] { updatePreview(); });
    }
    connect(forNoteKind_, &QComboBox::currentIndexChanged, this, [this] {
        updateForNoteKind();
        updateForPreview();
    });
    for (QComboBox* box : {forLabelStyle_, forArrow_, forCallout_, forTextStyle_}) {
        connect(box, &QComboBox::currentIndexChanged, this, [this] { updateForPreview(); });
    }
    connect(forNote_, &QPlainTextEdit::textChanged, this, [this] { updateForPreview(); });
    connect(forAngle_, &QDoubleSpinBox::valueChanged, this, [this] { updateForPreview(); });
    connect(forBalloon_, &QCheckBox::toggled, this, [this] { updateForPreview(); });
    connect(apply_, &QPushButton::clicked, this, [this] { (void)apply(); });
    connect(freeze_, &QPushButton::clicked, this, [this] { (void)freeze(); });
    connect(detach_, &QPushButton::clicked, this, [this] { (void)detach(); });
    connect(attach_, &QPushButton::clicked, this, [this] { (void)attachToSelected(); });
    connect(attributeSet_, &QPushButton::clicked, this, [this] { (void)setAttribute(); });
    connect(attributeRemove_, &QPushButton::clicked, this, [this] { (void)removeAttribute(); });
    connect(makeButton, &QPushButton::clicked, this, [this] { (void)makeForSelection(); });
    connect(alignButton, &QPushButton::clicked, this, [this] { (void)alignSelection(); });
    connect(renumberButton, &QPushButton::clicked, this, [this] { (void)renumberBalloons(); });
    connect(alignUseX_, &QCheckBox::toggled, alignX_, &QWidget::setEnabled);
    connect(alignUseSpacing_, &QCheckBox::toggled, alignSpacing_, &QWidget::setEnabled);
    alignX_->setEnabled(false);
    alignSpacing_->setEnabled(false);

    // Only a flag, never a widget touched: the watcher's delivery does the work.
    replacedListener_ = document_.addListener([this](const katana::cad::DocumentChange& change) {
        if (change.has(katana::cad::DocumentChange::Replaced)) {
            replaced_ = true;
        }
    });
    watcher_ = std::make_unique<DocumentWatcher>(
        document_, [this](const DocumentChanges& changes) { refresh(changes); });
    DocumentChanges everything;
    everything.model = true;
    everything.selection = true;
    refresh(everything);
    updateForNoteKind();
}

LeaderManagerDialog::~LeaderManagerDialog() = default;

void LeaderManagerDialog::showTab(Tab tab)
{
    tabs_->setCurrentIndex(static_cast<int>(tab));
}

const LeaderGeometry* LeaderManagerDialog::leader() const
{
    if (shown_ == 0) {
        return nullptr;
    }
    const katana::entity::Entity* entity = document_.model().entities.find(shown_);
    return entity != nullptr ? std::get_if<LeaderGeometry>(&entity->geometry) : nullptr;
}

std::vector<EntityId> LeaderManagerDialog::selectedLeaders() const
{
    std::vector<EntityId> ids;
    for (const EntityId id : document_.selection().ids()) {
        const katana::entity::Entity* entity = document_.model().entities.find(id);
        if (entity != nullptr && std::holds_alternative<LeaderGeometry>(entity->geometry)) {
            ids.push_back(id);
        }
    }
    return ids;
}

std::vector<EntityId> LeaderManagerDialog::targets() const
{
    if (shown_ == 0) {
        return {};
    }
    auto selected = selectedLeaders();
    if (selected.size() > 1 && std::ranges::find(selected, shown_) != selected.end()) {
        return selected;
    }
    return {shown_};
}

void LeaderManagerDialog::refresh(const DocumentChanges& delivered)
{
    if (watcher_ && !watcher_->documentAlive()) {
        return;
    }
    DocumentChanges changes = delivered;
    if (replaced_) {
        // A new drawing: nothing the form holds is about it.
        replaced_ = false;
        shown_ = 0;
        shownGeometry_.reset();
        loaded_.reset();
        changes.model = true;
        changes.selection = true;
    }
    if (changes.model) {
        fillStyles();
        fillList();
    }
    if (changes.selection) {
        // The form follows the drawing's selection: the shown leader while it
        // is one of those selected, else the first selected.
        const auto selected = selectedLeaders();
        if (!selected.empty() && std::ranges::find(selected, shown_) == selected.end()) {
            (void)showLeader(selected.front());
        }
        const std::size_t count = document_.selection().size();
        forSelection_->setText(count == 0
                                   ? QStringLiteral("Nothing is selected: select the "
                                                    "entities to put leaders on.")
                                   : QString::number(count) +
                                         (count == 1 ? QStringLiteral(" entity selected")
                                                     : QStringLiteral(" entities selected")));
    }
    if (changes.model || changes.selection) {
        const LeaderGeometry* current = leader();
        if (current == nullptr) {
            // Gone (erased, undone): the first leader there is, or none.
            shown_ = 0;
            shownGeometry_.reset();
            if (list_->count() > 0) {
                (void)showLeader(list_->item(0)->data(Qt::UserRole).toULongLong());
            } else {
                showForm(LeaderGeometry{});
            }
        } else if (!shownGeometry_ || !(*shownGeometry_ == *current)) {
            // Changed under the form: show it as it is now.
            showForm(*current);
        } else {
            // The leader is as shown; what its entity and its style say may
            // not be: the style's template, the values, the preview.
            if (static_cast<Kind>(noteKind_->currentData().toInt()) == Kind::LabelStyle) {
                updateNoteKind();
            }
            showValues();
            updatePreview();
        }
        updateForPreview();
    }
}

void LeaderManagerDialog::fillStyles()
{
    fillLabelStyles(labelStyle_, document_.model());
    fillLabelStyles(forLabelStyle_, document_.model());
    fillTextStyles(textStyle_, document_.model(), QStringLiteral("(default: Standard)"));
    fillTextStyles(forTextStyle_, document_.model(), QStringLiteral("(default: Standard)"));
}

void LeaderManagerDialog::fillList()
{
    const QSignalBlocker blocker(list_);
    list_->clear();
    const auto& model = document_.model();
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        if (const auto* shape = std::get_if<LeaderGeometry>(&entity.geometry)) {
            auto* item =
                new QListWidgetItem(listText(entity.id, ann::leaderSays(model, *shape)), list_);
            item->setData(Qt::UserRole, QVariant::fromValue<qulonglong>(entity.id));
            if (entity.id == shown_) {
                list_->setCurrentItem(item);
            }
        }
    });
}

bool LeaderManagerDialog::showLeader(EntityId id)
{
    const katana::entity::Entity* entity = document_.model().entities.find(id);
    const auto* shape =
        entity != nullptr ? std::get_if<LeaderGeometry>(&entity->geometry) : nullptr;
    if (shape == nullptr) {
        return false;
    }
    shown_ = id;
    {
        const QSignalBlocker blocker(list_);
        for (int row = 0; row < list_->count(); ++row) {
            if (list_->item(row)->data(Qt::UserRole).toULongLong() == id) {
                list_->setCurrentRow(row);
                break;
            }
        }
    }
    showForm(*shape);
    return true;
}

void LeaderManagerDialog::showForm(const LeaderGeometry& shape)
{
    const bool any = shown_ != 0;
    shownGeometry_ = any ? std::optional(shape) : std::nullopt;
    stash_.reset();
    lentFrom_.reset();
    for (QWidget* field : std::initializer_list<QWidget*>{
             noteKind_, note_, values_, arrow_, callout_, textStyle_, paperHeight_, arrowSize_,
             landing_, attributeName_, attributeValue_, attributeType_}) {
        field->setEnabled(any);
    }
    const ann::LeaderNote note = ann::noteOf(shape);
    {
        const QSignalBlocker blockers[] = {QSignalBlocker(noteKind_),    QSignalBlocker(note_),
                                           QSignalBlocker(labelStyle_),  QSignalBlocker(arrow_),
                                           QSignalBlocker(callout_),     QSignalBlocker(textStyle_),
                                           QSignalBlocker(paperHeight_), QSignalBlocker(arrowSize_),
                                           QSignalBlocker(landing_),     QSignalBlocker(along_)};
        noteKind_->setCurrentIndex(noteKind_->findData(static_cast<int>(note.kind)));
        if (note.kind == Kind::LabelStyle) {
            labelStyle_->setCurrentIndex(
                std::max(0, labelStyle_->findText(QString::fromStdString(note.text))));
        } else {
            setNoteText(note_, QString::fromStdString(note.text));
        }
        choose(arrow_, shape.arrow);
        choose(callout_, shape.callout);
        textStyle_->setCurrentIndex(
            std::max(0, textStyle_->findData(QString::fromStdString(shape.style))));
        paperHeight_->setValue(shape.paperHeight);
        arrowSize_->setValue(shape.arrowSize);
        landing_->setValue(shape.landing);
        const bool along = shape.tipRef.point == katana::entity::AnchorPoint::Along;
        along_->setValue(along ? shape.tipRef.parameter * 100.0 : 0.0);
    }
    shownKind_ = static_cast<int>(note.kind);
    if (!any) {
        target_->setText(QStringLiteral("No leader: draw one with the Leader tool, or make them "
                                        "on the For Selection tab."));
    } else if (!shape.tipRef.associated()) {
        target_->setText(QStringLiteral("Nothing - a plain leader. Select it and an entity, then "
                                        "Attach to Selected."));
    } else {
        const katana::entity::Entity* on = document_.model().entities.find(shape.tipRef.entity);
        const bool polyline =
            on != nullptr && std::holds_alternative<katana::geometry::Polyline2>(on->geometry);
        target_->setText((on != nullptr ? QString::fromUtf8(katana::entity::toString(on->type()))
                                        : QStringLiteral("a gone entity")) +
                         QStringLiteral(" ") + QString::number(shape.tipRef.entity) +
                         QStringLiteral(", ") + placeText(shape.tipRef, polyline));
    }
    updateNoteKind();
    loaded_ = readForm();
    showValues();
    updatePreview();
}

LeaderManagerDialog::FormValues LeaderManagerDialog::readForm() const
{
    FormValues values;
    values.noteKind = noteKind_->currentData().toInt();
    values.noteText = plainText(note_);
    values.labelStyle = labelStyle_->currentText();
    values.arrow = arrow_->currentData().toInt();
    values.callout = callout_->currentData().toInt();
    values.textStyle = textStyle_->currentData().toString();
    values.paperHeight = paperHeight_->value();
    values.arrowSize = arrowSize_->value();
    values.landing = landing_->value();
    values.along = along_->value();
    return values;
}

void LeaderManagerDialog::showValues()
{
    values_->setRowCount(0);
    const LeaderGeometry* current = leader();
    if (current == nullptr) {
        return;
    }
    if (const auto values = ann::leaderTargetValues(document_.model(), *current)) {
        fillValues(values_, ann::leaderValueRows(*values));
    }
}

void LeaderManagerDialog::updateNoteKind()
{
    const bool styled = static_cast<Kind>(noteKind_->currentData().toInt()) == Kind::LabelStyle;
    labelStyle_->setEnabled(shown_ != 0 && styled);
    note_->setReadOnly(styled);
    if (styled) {
        // The style's template, to read: the note is the style's, live.
        const auto* style =
            document_.model().labelStyles.find(labelStyle_->currentText().toStdString());
        setNoteText(note_, style != nullptr ? QString::fromStdString(style->text) : QString());
    }
}

void LeaderManagerDialog::noteKindChosen()
{
    const int now = noteKind_->currentData().toInt();
    const int styled = static_cast<int>(Kind::LabelStyle);
    if (shownKind_ != styled && now == styled) {
        // The user's words are kept while the style's template stands in.
        stash_ = plainText(note_);
        shownKind_ = now;
        updateNoteKind();
        lendStyleLook();
    } else if (shownKind_ == styled && now != styled) {
        // Back to the user's words and the look before the style lent its
        // own; a leader shown in a style starts from the style's template.
        shownKind_ = now;
        updateNoteKind();
        if (stash_) {
            setNoteText(note_, *stash_);
        }
        if (lentFrom_) {
            textStyle_->setCurrentIndex(std::max(0, textStyle_->findData(lentFrom_->textStyle)));
            paperHeight_->setValue(lentFrom_->paperHeight);
        }
        stash_.reset();
        lentFrom_.reset();
    } else {
        shownKind_ = now;
        updateNoteKind();
    }
    updatePreview();
}

void LeaderManagerDialog::lendStyleLook()
{
    if (static_cast<Kind>(noteKind_->currentData().toInt()) != Kind::LabelStyle) {
        return;
    }
    const auto* style =
        document_.model().labelStyles.find(labelStyle_->currentText().toStdString());
    if (style == nullptr) {
        return;
    }
    if (!lentFrom_) {
        lentFrom_ = Look{textStyle_->currentData().toString(), paperHeight_->value()};
    }
    if (!style->textStyle.empty()) {
        textStyle_->setCurrentIndex(
            std::max(0, textStyle_->findData(QString::fromStdString(style->textStyle))));
    }
    if (style->paperHeight > 0.0) {
        paperHeight_->setValue(style->paperHeight);
    }
}

void LeaderManagerDialog::updateForNoteKind()
{
    const bool styled = static_cast<Kind>(forNoteKind_->currentData().toInt()) == Kind::LabelStyle;
    forLabelStyle_->setEnabled(styled);
    forNote_->setEnabled(!styled);
}

ann::LeaderChange LeaderManagerDialog::formChange() const
{
    ann::LeaderChange change;
    const LeaderGeometry* current = leader();
    if (current == nullptr || !loaded_) {
        return change;
    }
    const FormValues now = readForm();
    const FormValues& was = *loaded_;
    const auto kind = static_cast<Kind>(now.noteKind);
    const bool noteChanged =
        now.noteKind != was.noteKind || (kind == Kind::LabelStyle ? now.labelStyle != was.labelStyle
                                                                  : now.noteText != was.noteText);
    if (noteChanged) {
        change.note = kind == Kind::LabelStyle ? ann::LeaderNote{kind, now.labelStyle.toStdString()}
                                               : ann::LeaderNote{kind, now.noteText.toStdString()};
    }
    if (now.arrow != was.arrow) {
        change.arrow = static_cast<katana::entity::ArrowHead>(now.arrow);
    }
    if (now.callout != was.callout) {
        change.callout = static_cast<katana::entity::CalloutShape>(now.callout);
    }
    if (now.textStyle != was.textStyle) {
        change.textStyle = now.textStyle.toStdString();
    }
    if (now.paperHeight != was.paperHeight) {
        change.paperHeight = now.paperHeight;
    }
    if (now.arrowSize != was.arrowSize) {
        change.arrowSize = now.arrowSize;
    }
    if (now.landing != was.landing) {
        change.landing = now.landing;
    }
    if (now.along != was.along && current->tipRef.point == katana::entity::AnchorPoint::Along) {
        katana::entity::AnchorRef ref = current->tipRef;
        ref.parameter = std::clamp(now.along / 100.0, 0.0, 1.0);
        if (const katana::entity::Entity* on = document_.model().entities.find(ref.entity)) {
            if (const auto point = katana::entity::resolveAnchor(*on, ref)) {
                change.tip = ann::AnchoredPoint{*point, ref};
            }
        }
    }
    return change;
}

void LeaderManagerDialog::updatePreview()
{
    const LeaderGeometry* current = leader();
    if (current == nullptr) {
        check_->clear();
        preview_->clear();
        updateButtons();
        return;
    }
    LeaderGeometry would = *current;
    const Status status = ann::applyLeaderChange(document_.model(), formChange(), would, shown_);
    if (!status) {
        check_->setStyleSheet(QStringLiteral("color: #d9534f"));
        check_->setText(describe(status));
        preview_->clear();
    } else {
        check_->setStyleSheet(QString());
        check_->setText(katana::entity::isSmart(would)
                            ? QStringLiteral("Read off the entity whenever it is drawn.")
                            : QStringLiteral("Plain text: the note says exactly this."));
        preview_->setText(QString::fromStdString(ann::leaderSays(document_.model(), would)));
    }
    updateButtons();
}

void LeaderManagerDialog::updateButtons()
{
    const bool any = shown_ != 0;
    const bool dirty = !formChange().empty();
    const auto ids = targets();
    bool anySmart = false;
    bool anyAttached = false;
    for (const EntityId id : ids) {
        if (const auto* entity = document_.model().entities.find(id)) {
            if (const auto* shape = std::get_if<LeaderGeometry>(&entity->geometry)) {
                anySmart = anySmart || katana::entity::isSmart(*shape);
                anyAttached = anyAttached || shape->tipRef.associated();
            }
        }
    }
    const LeaderGeometry* current = leader();
    const bool attached = current != nullptr && current->tipRef.associated();
    apply_->setEnabled(any);
    freeze_->setEnabled(any && !dirty && anySmart);
    detach_->setEnabled(any && !dirty && anyAttached);
    // Each acts on the leader as drawn, not the form: while the form has
    // changes not applied, their tips say so rather than leave the user
    // guessing why they are greyed.
    const QString applyFirst =
        dirty ? QStringLiteral("\n\nThe form has changes not applied: Apply them first.")
              : QString();
    freeze_->setToolTip(QString::fromLatin1(kFreezeTip) + applyFirst);
    detach_->setToolTip(QString::fromLatin1(kDetachTip) + applyFirst);
    attach_->setToolTip(QString::fromLatin1(kAttachTip) + applyFirst);
    attributeSet_->setEnabled(attached);
    attributeRemove_->setEnabled(attached);
    bool other = false;
    for (const EntityId id : document_.selection().ids()) {
        other = other || id != shown_;
    }
    attach_->setEnabled(any && !dirty && other);
    // Enabled with several selected too, so an Along changed before the
    // others were selected can be set back (apply() refuses it for them).
    along_->setEnabled(any && current != nullptr &&
                       current->tipRef.point == katana::entity::AnchorPoint::Along);
    scope_->setText(ids.size() > 1 ? QStringLiteral("Apply, Freeze and Detach change the ") +
                                         QString::number(ids.size()) +
                                         QStringLiteral(" selected leaders; the form shows ") +
                                         QString::number(shown_) + QStringLiteral(".")
                                   : QString());
    scope_->setVisible(ids.size() > 1);
}

QString LeaderManagerDialog::preview() const
{
    return check_->styleSheet().isEmpty() ? preview_->text() : check_->text();
}

QString LeaderManagerDialog::problem() const
{
    return problem_->text();
}

void LeaderManagerDialog::report(const Status& status)
{
    problem_->setText(describe(status));
}

void LeaderManagerDialog::insertValue(const QString& name)
{
    if (shown_ == 0 || name.isEmpty()) {
        return;
    }
    if (static_cast<Kind>(noteKind_->currentData().toInt()) != Kind::Template) {
        // A field makes the note a template. A label style's template is the
        // starting point (its look, lent, stays); a plain note's words stay.
        const QSignalBlocker blocker(noteKind_);
        noteKind_->setCurrentIndex(noteKind_->findData(static_cast<int>(Kind::Template)));
        shownKind_ = static_cast<int>(Kind::Template);
        stash_.reset();
        lentFrom_.reset();
        updateNoteKind();
    }
    insertField(noteKind_, note_, name);
}

bool LeaderManagerDialog::apply()
{
    if (shown_ == 0) {
        report(makeError(ErrorCode::InvalidState, "no leader is shown"));
        return false;
    }
    const auto ids = targets();
    const ann::LeaderChange change = formChange();
    if (change.tip && ids.size() > 1) {
        // Along is where THIS leader's tip is on THIS leader's entity: sent
        // to the others too, it would move every tip onto that one place.
        report(makeError(ErrorCode::InvalidArgument,
                         "Along moves one leader's tip: select only leader " +
                             std::to_string(shown_) + " to move it, or set Along back to " +
                             QString::number(loaded_->along, 'f', kSizeDecimals).toStdString() +
                             " %"));
        return false;
    }
    auto command = ann::changeLeaders(document_.model(), ids, change);
    if (!command) {
        report(command.error());
        return false;
    }
    const Status status = runCommand(document_, std::move(*command));
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::freeze()
{
    if (shown_ == 0) {
        report(makeError(ErrorCode::InvalidState, "no leader is shown"));
        return false;
    }
    if (!formChange().empty()) {
        // Freezing keeps what the leader says now - not the form's unapplied
        // note, which a re-read form would then throw away.
        report(makeError(ErrorCode::InvalidState,
                         "the form has changes not applied: Apply them first, or show the leader "
                         "again"));
        return false;
    }
    auto release = ann::releaseLeaders(document_.model(), targets(), false);
    if (!release) {
        report(release.error());
        return false;
    }
    const Status status = runCommand(document_, std::move(release->command));
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::detach()
{
    if (shown_ == 0) {
        report(makeError(ErrorCode::InvalidState, "no leader is shown"));
        return false;
    }
    if (!formChange().empty()) {
        report(makeError(ErrorCode::InvalidState,
                         "the form has changes not applied: Apply them first, or show the leader "
                         "again"));
        return false;
    }
    auto release = ann::releaseLeaders(document_.model(), targets(), true);
    if (!release) {
        report(release.error());
        return false;
    }
    const Status status = runCommand(document_, std::move(release->command));
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::attachToSelected()
{
    const LeaderGeometry* current = leader();
    if (current == nullptr) {
        report(makeError(ErrorCode::InvalidState, "no leader is shown"));
        return false;
    }
    if (!formChange().empty()) {
        report(makeError(ErrorCode::InvalidState,
                         "the form has changes not applied: Apply them first, or show the leader "
                         "again"));
        return false;
    }
    // The first selected entity that is not a leader, else any other one.
    const katana::entity::Entity* target = nullptr;
    for (const bool leadersToo : {false, true}) {
        for (const EntityId id : document_.selection().ids()) {
            const katana::entity::Entity* entity = document_.model().entities.find(id);
            if (target == nullptr && entity != nullptr && id != shown_ &&
                (leadersToo || !std::holds_alternative<LeaderGeometry>(entity->geometry))) {
                target = entity;
            }
        }
    }
    if (target == nullptr) {
        report(makeError(ErrorCode::InvalidState,
                         "select the entity to put the tip on, with the leader"));
        return false;
    }
    const auto ref = katana::entity::nearestAnchor(*target, current->vertices.front());
    const auto point = ref ? katana::entity::resolveAnchor(*target, *ref) : std::nullopt;
    if (!point) {
        report(makeError(ErrorCode::InvalidArgument, "that entity has no place on it to attach to",
                         "id=" + std::to_string(target->id)));
        return false;
    }
    auto command = ann::attachLeader(document_.model(), shown_, ann::AnchoredPoint{*point, *ref});
    if (!command) {
        report(command.error());
        return false;
    }
    const Status status = runCommand(document_, std::move(*command));
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::setAttribute()
{
    if (shown_ == 0) {
        report(makeError(ErrorCode::InvalidState, "no leader is shown"));
        return false;
    }
    const std::string type = attributeType_->currentData().toString().toStdString();
    auto value = katana::cad::CommandInterpreter::propertyValue(
        attributeValue_->text().toStdString(), type.empty() ? nullptr : &type);
    if (!value) {
        report(value.error());
        return false;
    }
    auto command = ann::setLeaderTargetProperty(
        document_.model(), shown_, attributeName_->text().trimmed().toStdString(), *value);
    if (!command) {
        report(command.error());
        return false;
    }
    const Status status = runCommand(document_, std::move(*command));
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::removeAttribute()
{
    if (shown_ == 0) {
        report(makeError(ErrorCode::InvalidState, "no leader is shown"));
        return false;
    }
    auto command = ann::setLeaderTargetProperty(
        document_.model(), shown_, attributeName_->text().trimmed().toStdString(), std::nullopt);
    if (!command) {
        report(command.error());
        return false;
    }
    const Status status = runCommand(document_, std::move(*command));
    report(status);
    return static_cast<bool>(status);
}

// ---- For Selection ------------------------------------------------------------------------

ann::LeadersForOptions LeaderManagerDialog::forOptions() const
{
    ann::LeadersForOptions options;
    options.balloon = forBalloon_->isChecked();
    const ann::LeaderNote note = noteFrom(forNoteKind_, forNote_, forLabelStyle_);
    // Nothing typed: a balloon is numbered, a leader is a bare arrow - of
    // whatever kind the box was left at.
    const bool nothing = note.kind != Kind::LabelStyle && note.text.empty();
    if (!nothing) {
        options.change.note = note;
    } else if (!options.balloon) {
        options.change.note = ann::LeaderNote{Kind::Text, std::string()};
    }
    if (const int arrow = forArrow_->currentData().toInt(); arrow != kAutomatic) {
        options.change.arrow = static_cast<katana::entity::ArrowHead>(arrow);
    }
    if (const int callout = forCallout_->currentData().toInt(); callout != kAutomatic) {
        options.change.callout = static_cast<katana::entity::CalloutShape>(callout);
    }
    if (const QString style = forTextStyle_->currentData().toString(); !style.isEmpty()) {
        options.change.textStyle = style.toStdString();
    }
    if (forPaperHeight_->value() > 0.0) {
        options.change.paperHeight = forPaperHeight_->value();
    }
    if (forArrowSize_->value() != LeaderGeometry{}.arrowSize) {
        options.change.arrowSize = forArrowSize_->value();
    }
    if (forLanding_->value() != LeaderGeometry{}.landing) {
        options.change.landing = forLanding_->value();
    }
    options.angle = forAngle_->value() * katana::math::kDegToRad;
    options.length = forLength_->value();
    return options;
}

const katana::entity::Entity* LeaderManagerDialog::forTarget() const
{
    const double angle = forAngle_->value() * katana::math::kDegToRad;
    for (const EntityId id : document_.selection().ids()) {
        const katana::entity::Entity* entity = document_.model().entities.find(id);
        if (entity != nullptr && ann::leaderPlaceOn(*entity, angle)) {
            return entity;
        }
    }
    return nullptr;
}

void LeaderManagerDialog::updateForPreview()
{
    forValues_->setRowCount(0);
    const katana::entity::Entity* target = forTarget();
    if (target == nullptr) {
        forCheck_->setStyleSheet(QString());
        forCheck_->setText(document_.selection().empty()
                               ? QStringLiteral("Select the entities to put leaders on.")
                               : QStringLiteral("Nothing selected offers a place for a leader "
                                                "(a dimension, a label or a leader does not)."));
        forPreview_->clear();
        return;
    }
    const auto options = forOptions();
    const auto place = ann::leaderPlaceOn(*target, options.angle);
    fillValues(forValues_,
               ann::leaderValueRows(katana::entity::anchorValues(
                   *target, place->ref, place->point, katana::cad::codePropertyCandidates())));
    LeaderGeometry would;
    would.vertices = {place->point, place->point + katana::geometry::Vec2(1.0, 1.0)};
    would.tipRef = place->ref;
    if (options.balloon) {
        would.callout = katana::entity::CalloutShape::Circle;
    }
    ann::LeaderChange change = options.change;
    if (options.balloon && !change.note) {
        change.note =
            ann::LeaderNote{Kind::Text, std::to_string(ann::nextBalloonNumber(document_.model()))};
    }
    const QString first = QString::fromUtf8(katana::entity::toString(target->type())) +
                          QStringLiteral(" ") + QString::number(target->id);
    if (const Status status = ann::applyLeaderChange(document_.model(), change, would); !status) {
        forCheck_->setStyleSheet(QStringLiteral("color: #d9534f"));
        forCheck_->setText(first + QStringLiteral(": ") + describe(status));
        forPreview_->clear();
        return;
    }
    forCheck_->setStyleSheet(QString());
    forCheck_->setText(QStringLiteral("For ") + first + QStringLiteral(", the first selected:"));
    const std::string says = ann::leaderSays(document_.model(), would);
    forPreview_->setText(says.empty() ? QStringLiteral("(a bare leader)")
                                      : QString::fromStdString(says));
}

QString LeaderManagerDialog::forPreview() const
{
    return forCheck_->styleSheet().isEmpty() ? forPreview_->text() : forCheck_->text();
}

void LeaderManagerDialog::insertForValue(const QString& name)
{
    if (!name.isEmpty()) {
        insertField(forNoteKind_, forNote_, name);
    }
}

bool LeaderManagerDialog::makeForSelection()
{
    if (document_.selection().empty()) {
        report(makeError(ErrorCode::InvalidState,
                         "nothing is selected: select the entities to put leaders on"));
        forReport_->clear();
        return false;
    }
    auto made = ann::leadersFor(document_.model(), document_.selection().ids(), forOptions(),
                                document_.annotationScale(), document_.currentAttributes());
    if (!made) {
        report(made.error());
        forReport_->clear();
        return false;
    }
    const std::size_t count = made->made;
    const std::size_t skipped = made->skipped;
    const std::string firstSkip = made->firstSkip;
    const Status status = runCommand(document_, std::move(made->command));
    report(status);
    if (!status) {
        return false;
    }
    // What was made is selected, so the Leader tab shows it and Arrange can
    // line it up next.
    document_.selection().set(document_.lastCreatedEntities());
    QString text = QStringLiteral("made=") + QString::number(count) + QStringLiteral(" skipped=") +
                   QString::number(skipped);
    if (skipped > 0) {
        text += QStringLiteral(": ") + QString::fromStdString(firstSkip);
    }
    forReport_->setText(text);
    return true;
}

QString LeaderManagerDialog::forReport() const
{
    return forReport_->text();
}

// ---- Arrange --------------------------------------------------------------------------------

bool LeaderManagerDialog::alignSelection()
{
    const auto ids = selectedLeaders();
    if (ids.empty()) {
        report(makeError(ErrorCode::InvalidState, "no leader is selected: select the leaders to "
                                                  "line up"));
        return false;
    }
    std::optional<double> x;
    if (alignUseX_->isChecked()) {
        x = alignX_->value();
    }
    std::optional<double> spacing;
    if (alignUseSpacing_->isChecked()) {
        spacing = katana::entity::annotationModelSize(alignSpacing_->value(),
                                                      document_.annotationScale());
    }
    auto alignment = ann::alignLeaders(document_.model(), ids, x, spacing);
    if (!alignment) {
        report(alignment.error());
        return false;
    }
    const std::size_t count = alignment->count;
    const Status status = runCommand(document_, std::move(alignment->command));
    report(status);
    if (status) {
        arrangeReport_->setText(QStringLiteral("aligned=") + QString::number(count));
    }
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::renumberBalloons()
{
    auto renumbering = ann::renumberBalloons(
        document_.model(), renumberStart_->value(),
        static_cast<ann::BalloonOrder>(renumberOrder_->currentData().toInt()));
    const Status status = runCommand(document_, std::move(renumbering.command));
    report(status);
    if (status) {
        arrangeReport_->setText(
            QStringLiteral("balloons=") + QString::number(renumbering.balloons) +
            QStringLiteral(" renumbered=") + QString::number(renumbering.renumbered));
    }
    return static_cast<bool>(status);
}

QString LeaderManagerDialog::arrangeReport() const
{
    return arrangeReport_->text();
}

} // namespace katana::qt
