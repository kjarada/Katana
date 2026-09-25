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
#include <QVBoxLayout>

#include "annotation/form_widgets.hpp"
#include "customisation/document_watcher.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/math/numerics.hpp"

namespace katana::qt {

namespace ann = katana::cad::annotation;
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

// The note a note-kind box and its editor and style box say.
ann::LeaderNote noteFrom(const QComboBox* kind, const QPlainTextEdit* text, const QComboBox* style)
{
    const auto chosenKind = static_cast<Kind>(kind->currentData().toInt());
    if (chosenKind == Kind::LabelStyle) {
        return ann::LeaderNote{chosenKind, style->currentText().toStdString()};
    }
    return ann::LeaderNote{chosenKind, text->toPlainText().toStdString()};
}

// Where on its entity a tip is, in words: "halfway along segment 2",
// "inside it", "at its end". The command line's terse "along 1 0.5" is
// entity::describe's; this is for reading.
QString placeText(const katana::entity::AnchorRef& ref, bool polyline)
{
    using katana::entity::AnchorPoint;
    const auto percent = [](double fraction) {
        return QString::number(std::clamp(fraction, 0.0, 1.0) * 100.0, 'f', 0) +
               QStringLiteral("%");
    };
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
        return percent(ref.parameter) + QStringLiteral(" along") +
               (polyline ? QStringLiteral(" segment ") + QString::number(ref.index + 1)
                         : QStringLiteral(" it"));
    case AnchorPoint::Inside:
        return QStringLiteral("inside it");
    }
    return {};
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
    target_ = new QLabel(leaderTab);
    target_->setObjectName(QStringLiteral("leaderTarget"));
    target_->setWordWrap(true);
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
    values_ = new QTableWidget(0, 2, leaderTab);
    values_->setObjectName(QStringLiteral("leaderValues"));
    values_->setHorizontalHeaderLabels({QStringLiteral("Value"), QStringLiteral("Here")});
    values_->horizontalHeader()->setStretchLastSection(true);
    values_->verticalHeader()->setVisible(false);
    values_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    values_->setSelectionBehavior(QAbstractItemView::SelectRows);
    values_->setToolTip(QStringLiteral(
        "What the entity the tip is on offers here; double-click one to put it in the note"));
    arrow_ = combo(leaderTab, "leaderArrow", kArrows);
    callout_ = combo(leaderTab, "leaderCallout", kCallouts);
    textStyle_ = new QComboBox(leaderTab);
    textStyle_->setObjectName(QStringLiteral("leaderTextStyle"));
    paperHeight_ = spin(leaderTab, "leaderPaperHeight", 0.0, 100.0, 0.5, 2, QStringLiteral(" mm"));
    paperHeight_->setSpecialValueText(QStringLiteral("the text style's"));
    arrowSize_ = spin(leaderTab, "leaderArrowSize", 0.0, 50.0, 0.5, 2, QStringLiteral(" mm"));
    landing_ = spin(leaderTab, "leaderLanding", 0.0, 50.0, 0.5, 2, QStringLiteral(" mm"));
    landing_->setSpecialValueText(QStringLiteral("none"));
    attributeName_ = edit(leaderTab, "leaderAttributeName", QStringLiteral("invert"));
    attributeValue_ = edit(leaderTab, "leaderAttributeValue", QStringLiteral("10.25"));
    attributeType_ = new QComboBox(leaderTab);
    attributeType_->setObjectName(QStringLiteral("leaderAttributeType"));
    attributeType_->addItem(QStringLiteral("as typed"), QString());
    for (const char* type : {"text", "integer", "real", "boolean"}) {
        attributeType_->addItem(QString::fromLatin1(type), QString::fromLatin1(type));
    }

    auto* noteForm = new QFormLayout;
    noteForm->addRow(QStringLiteral("On"), target_);
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

    auto* setAttributeButton = button(leaderTab, "leaderAttributeSet", QStringLiteral("Set"));
    auto* removeAttributeButton =
        button(leaderTab, "leaderAttributeRemove", QStringLiteral("Remove"));
    setAttributeButton->setToolTip(
        QStringLiteral("Set this attribute on the entity the tip is on (LEADER PROP)"));
    auto* attributeRow = new QHBoxLayout;
    attributeRow->addWidget(attributeName_, 2);
    attributeRow->addWidget(attributeValue_, 2);
    attributeRow->addWidget(attributeType_, 1);
    attributeRow->addWidget(setAttributeButton);
    attributeRow->addWidget(removeAttributeButton);
    auto* attribute = new QGroupBox(QStringLiteral("Attribute of the entity it is on"), leaderTab);
    attribute->setLayout(attributeRow);

    auto* freezeButton = button(leaderTab, "leaderFreeze", QStringLiteral("Freeze Note"));
    freezeButton->setToolTip(
        QStringLiteral("Keep the words the note says now; the tip still follows (LEADER FREEZE)"));
    auto* detachButton = button(leaderTab, "leaderDetach", QStringLiteral("Detach Tip"));
    detachButton->setToolTip(QStringLiteral(
        "Let the tip go of the entity it is on, freezing a smart note first (LEADER DETACH)"));
    auto* applyButton = button(leaderTab, "leaderApply", QStringLiteral("Apply"));
    auto* leaderButtons = new QHBoxLayout;
    leaderButtons->addWidget(freezeButton);
    leaderButtons->addWidget(detachButton);
    leaderButtons->addStretch();
    leaderButtons->addWidget(applyButton);

    // The values beside the look: what can go in the note next to how it
    // is drawn, each tall enough to read without scrolling for a point.
    values_->setMinimumHeight(180);
    values_->setColumnWidth(0, 120);
    attributeName_->setMinimumWidth(110);
    attributeValue_->setMinimumWidth(110);
    list_->setMinimumWidth(200);
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
    forForm->addRow(QStringLiteral("Note direction"), forAngle_);
    forForm->addRow(QStringLiteral("Note distance on paper"), forLength_);
    forForm->addRow(forBalloon_);
    auto* forLayout = new QVBoxLayout(forTab);
    forLayout->addLayout(forForm);
    forLayout->addWidget(forReport_);
    forLayout->addStretch();
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
    resize(960, 640);

    // ---- behaviour ------------------------------------------------------------------------
    // The list shows; it never runs a command (docs/desktop.md).
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
    // Chosen by the user (showForm blocks these): a label style lends the
    // form its look, as LEADER labelstyle= lends the leader it.
    connect(noteKind_, &QComboBox::currentIndexChanged, this, [this] {
        updateNoteKind();
        lendStyleLook();
        updatePreview();
    });
    connect(labelStyle_, &QComboBox::currentIndexChanged, this, [this] {
        updateNoteKind();
        lendStyleLook();
        updatePreview();
    });
    for (QComboBox* box : {arrow_, callout_, textStyle_}) {
        connect(box, &QComboBox::currentIndexChanged, this, [this] { updatePreview(); });
    }
    connect(note_, &QPlainTextEdit::textChanged, this, [this] { updatePreview(); });
    connect(forNoteKind_, &QComboBox::currentIndexChanged, this, [this] { updateForNoteKind(); });
    connect(applyButton, &QPushButton::clicked, this, [this] { (void)apply(); });
    connect(freezeButton, &QPushButton::clicked, this, [this] { (void)freeze(); });
    connect(detachButton, &QPushButton::clicked, this, [this] { (void)detach(); });
    connect(setAttributeButton, &QPushButton::clicked, this, [this] { (void)setAttribute(); });
    connect(removeAttributeButton, &QPushButton::clicked, this,
            [this] { (void)removeAttribute(); });
    connect(makeButton, &QPushButton::clicked, this, [this] { (void)makeForSelection(); });
    connect(alignButton, &QPushButton::clicked, this, [this] { (void)alignSelection(); });
    connect(renumberButton, &QPushButton::clicked, this, [this] { (void)renumberBalloons(); });
    connect(alignUseX_, &QCheckBox::toggled, alignX_, &QWidget::setEnabled);
    connect(alignUseSpacing_, &QCheckBox::toggled, alignSpacing_, &QWidget::setEnabled);
    alignX_->setEnabled(false);
    alignSpacing_->setEnabled(false);

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

void LeaderManagerDialog::refresh(const DocumentChanges& changes)
{
    if (watcher_ && !watcher_->documentAlive()) {
        return;
    }
    if (changes.model) {
        fillStyles();
        fillList();
    }
    if (changes.selection) {
        // The form follows the drawing's selection: its first leader.
        const auto selected = selectedLeaders();
        if (!selected.empty() && selected.front() != shown_) {
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
            // The leader is as shown; what its entity says may not be.
            showValues();
            updatePreview();
        }
    }
}

void LeaderManagerDialog::fillStyles()
{
    fillLabelStyles(labelStyle_, document_.model());
    fillLabelStyles(forLabelStyle_, document_.model());
    const QString keep = textStyle_->currentData().toString();
    const QSignalBlocker blocker(textStyle_);
    textStyle_->clear();
    textStyle_->addItem(QStringLiteral("(default: Standard)"), QString());
    document_.model().textStyles.forEach([&](const katana::entity::TextStyle& style) {
        textStyle_->addItem(QString::fromStdString(style.name), QString::fromStdString(style.name));
    });
    textStyle_->setCurrentIndex(std::max(0, textStyle_->findData(keep)));
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
    for (QWidget* field : std::initializer_list<QWidget*>{
             noteKind_, note_, labelStyle_, values_, arrow_, callout_, textStyle_, paperHeight_,
             arrowSize_, landing_, attributeName_, attributeValue_, attributeType_}) {
        field->setEnabled(any);
    }
    {
        const QSignalBlocker kindBlocker(noteKind_);
        const QSignalBlocker noteBlocker(note_);
        const QSignalBlocker styleBlocker(labelStyle_);
        const ann::LeaderNote note = ann::noteOf(shape);
        noteKind_->setCurrentIndex(noteKind_->findData(static_cast<int>(note.kind)));
        if (note.kind == Kind::LabelStyle) {
            labelStyle_->setCurrentIndex(
                std::max(0, labelStyle_->findText(QString::fromStdString(note.text))));
        } else {
            note_->setPlainText(QString::fromStdString(note.text));
        }
        choose(arrow_, shape.arrow);
        choose(callout_, shape.callout);
        const QSignalBlocker textStyleBlocker(textStyle_);
        textStyle_->setCurrentIndex(
            std::max(0, textStyle_->findData(QString::fromStdString(shape.style))));
        paperHeight_->setValue(shape.paperHeight);
        arrowSize_->setValue(shape.arrowSize);
        landing_->setValue(shape.landing);
    }
    if (!any) {
        target_->setText(QStringLiteral("No leader: draw one with the Leader tool, or make them "
                                        "on the For Selection tab."));
    } else if (!shape.tipRef.associated()) {
        target_->setText(QStringLiteral("Nothing - a plain leader (LEADER ATTACH puts its tip on "
                                        "an entity)"));
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
    showValues();
    updatePreview();
}

void LeaderManagerDialog::showValues()
{
    values_->setRowCount(0);
    const LeaderGeometry* current = leader();
    if (current == nullptr) {
        return;
    }
    const auto values = ann::leaderTargetValues(document_.model(), *current);
    if (!values) {
        return;
    }
    const auto rows = ann::leaderValueRows(*values);
    values_->setRowCount(static_cast<int>(rows.size()));
    for (std::size_t i = 0; i < rows.size(); ++i) {
        values_->setItem(static_cast<int>(i), 0,
                         new QTableWidgetItem(QString::fromStdString(rows[i].name)));
        values_->setItem(static_cast<int>(i), 1,
                         new QTableWidgetItem(QString::fromStdString(rows[i].text)));
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
        const QSignalBlocker blocker(note_);
        note_->setPlainText(style != nullptr ? QString::fromStdString(style->text) : QString());
    }
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
    change.note = noteFrom(noteKind_, note_, labelStyle_);
    change.arrow = chosen<katana::entity::ArrowHead>(arrow_);
    change.callout = chosen<katana::entity::CalloutShape>(callout_);
    change.textStyle = textStyle_->currentData().toString().toStdString();
    change.paperHeight = paperHeight_->value();
    change.arrowSize = arrowSize_->value();
    change.landing = landing_->value();
    return change;
}

void LeaderManagerDialog::updatePreview()
{
    const LeaderGeometry* current = leader();
    if (current == nullptr) {
        check_->clear();
        preview_->clear();
        return;
    }
    LeaderGeometry would = *current;
    const Status status = ann::applyLeaderChange(document_.model(), formChange(), would, shown_);
    if (!status) {
        check_->setStyleSheet(QStringLiteral("color: #d9534f"));
        check_->setText(describe(status));
        preview_->clear();
        return;
    }
    check_->setStyleSheet(QString());
    const bool smart = katana::entity::isSmart(would);
    check_->setText(smart ? QStringLiteral("Read off the entity whenever it is drawn.")
                          : QStringLiteral("Plain text: the note says exactly this."));
    preview_->setText(QString::fromStdString(ann::leaderSays(document_.model(), would)));
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
        // A field makes the note a template: a label style's template is the
        // starting point, a plain note's words stay as they are.
        const QSignalBlocker blocker(noteKind_);
        noteKind_->setCurrentIndex(noteKind_->findData(static_cast<int>(Kind::Template)));
        updateNoteKind();
    }
    note_->insertPlainText(QStringLiteral("{") + name + QStringLiteral("}"));
    note_->setFocus();
}

bool LeaderManagerDialog::apply()
{
    if (shown_ == 0) {
        report(
            katana::core::makeError(katana::core::ErrorCode::InvalidState, "no leader is shown"));
        return false;
    }
    auto command = ann::changeLeaders(document_.model(), {shown_}, formChange());
    if (!command) {
        report(command.error());
        return false;
    }
    const Status status = *command ? document_.execute(std::move(*command)) : Status{};
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::freeze()
{
    if (shown_ == 0) {
        return false;
    }
    auto release = ann::releaseLeaders(document_.model(), {shown_}, false);
    if (!release) {
        report(release.error());
        return false;
    }
    const Status status =
        release->command ? document_.execute(std::move(release->command)) : Status{};
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::detach()
{
    if (shown_ == 0) {
        return false;
    }
    auto release = ann::releaseLeaders(document_.model(), {shown_}, true);
    if (!release) {
        report(release.error());
        return false;
    }
    const Status status =
        release->command ? document_.execute(std::move(release->command)) : Status{};
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::setAttribute()
{
    if (shown_ == 0) {
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
    const Status status = document_.execute(std::move(*command));
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::removeAttribute()
{
    if (shown_ == 0) {
        return false;
    }
    auto command = ann::setLeaderTargetProperty(
        document_.model(), shown_, attributeName_->text().trimmed().toStdString(), std::nullopt);
    if (!command) {
        report(command.error());
        return false;
    }
    const Status status = document_.execute(std::move(*command));
    report(status);
    return static_cast<bool>(status);
}

bool LeaderManagerDialog::makeForSelection()
{
    ann::LeadersForOptions options;
    options.balloon = forBalloon_->isChecked();
    const ann::LeaderNote note = noteFrom(forNoteKind_, forNote_, forLabelStyle_);
    // A balloon with nothing typed is numbered; a leader with nothing typed
    // is a bare arrow.
    if (!(options.balloon && note.kind == Kind::Text && note.text.empty())) {
        options.change.note = note;
    }
    options.angle = forAngle_->value() * katana::math::kDegToRad;
    options.length = forLength_->value();
    auto made = ann::leadersFor(document_.model(), document_.selection().ids(), options,
                                document_.annotationScale(), document_.currentAttributes());
    if (!made) {
        report(made.error());
        forReport_->clear();
        return false;
    }
    const std::size_t madeCount = made->made;
    const std::size_t skipped = made->skipped;
    const std::string firstSkip = made->firstSkip;
    const Status status = document_.execute(std::move(made->command));
    report(status);
    if (!status) {
        return false;
    }
    QString text = QStringLiteral("made=") + QString::number(madeCount) +
                   QStringLiteral(" skipped=") + QString::number(skipped);
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

bool LeaderManagerDialog::alignSelection()
{
    const auto ids = selectedLeaders();
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
    const Status status =
        alignment->command ? document_.execute(std::move(alignment->command)) : Status{};
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
    const Status status =
        renumbering.command ? document_.execute(std::move(renumbering.command)) : Status{};
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
