#include "annotation/annotation_managers.hpp"

#include "annotation/form_widgets.hpp"

#include <string>

#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QSpinBox>
#include <QTabWidget>
#include <QTableWidget>
#include <QVBoxLayout>

#include "katana/cad/annotation/auto_label.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::qt {

namespace cmd = katana::commands;
namespace ann = katana::cad::annotation;
using katana::core::Status;
using namespace annotation_form;

namespace {

// "\n" in a line edit is a line break in the stored template, as on the
// command line.
QString shownTemplate(const std::string& text)
{
    return QString::fromStdString(text).replace(QLatin1Char('\n'), QStringLiteral("\\n"));
}

std::string storedTemplate(const QString& text)
{
    return QString(text).replace(QStringLiteral("\\n"), QStringLiteral("\n")).toStdString();
}

// A unique name: "base", else "base 2", "base 3"...
template <typename Table> QString freshName(const Table& table, const QString& base)
{
    QString name = base;
    for (int n = 2; table.contains(name.toStdString()); ++n) {
        name = base + QStringLiteral(" ") + QString::number(n);
    }
    return name;
}

} // namespace

// ---- text styles ----------------------------------------------------------------------

TextStyleManagerDialog::TextStyleManagerDialog(katana::cad::Document& document, QWidget* parent)
    : QDialog(parent), document_(document)
{
    setObjectName(QStringLiteral("textStyleManagerDialog"));
    setWindowTitle(QStringLiteral("Text Styles"));

    list_ = new QListWidget(this);
    list_->setObjectName(QStringLiteral("textStyleList"));
    font_ = edit(this, "textStyleFont", QStringLiteral("the plain face"));
    paper_ = spin(this, "textStylePaperHeight", 0.0, 100.0, 0.5, 2, QStringLiteral(" mm"));
    width_ = spin(this, "textStyleWidthFactor", 0.05, 100.0, 0.05, 2);
    oblique_ = spin(this, "textStyleOblique", -84.9, 84.9, 1.0, 1, QStringLiteral(" deg"));
    bold_ = check(this, "textStyleBold", QStringLiteral("Bold"));
    italic_ = check(this, "textStyleItalic", QStringLiteral("Italic"));
    colour_ = edit(this, "textStyleColour", QStringLiteral("ByLayer, or #RRGGBB"));
    mask_ = check(this, "textStyleMask", QStringLiteral("Background mask"));
    margin_ = spin(this, "textStyleMaskMargin", 0.0, 20.0, 0.25, 2, QStringLiteral(" mm"));
    readable_ = check(this, "textStyleReadable", QStringLiteral("Keep readable (turn upside-down text)"));
    spacing_ = spin(this, "textStyleLineSpacing", 0.25, 4.0, 0.1, 2);
    sample_ = new QLabel(QStringLiteral("AaBb 123.456"), this);
    sample_->setObjectName(QStringLiteral("textStyleSample"));
    problem_ = problemLine(this, "textStyleProblem");

    auto* form = new QFormLayout;
    form->addRow(QStringLiteral("Font"), font_);
    form->addRow(QStringLiteral("Height on paper (0: the text's own)"), paper_);
    form->addRow(QStringLiteral("Width factor"), width_);
    form->addRow(QStringLiteral("Oblique"), oblique_);
    form->addRow(bold_, italic_);
    form->addRow(QStringLiteral("Colour"), colour_);
    form->addRow(mask_, margin_);
    form->addRow(readable_);
    form->addRow(QStringLiteral("Line spacing"), spacing_);
    form->addRow(sample_);

    auto* newButton = button(this, "textStyleNew", QStringLiteral("New"));
    auto* deleteButton = button(this, "textStyleDelete", QStringLiteral("Delete"));
    auto* applyButton = button(this, "textStyleApply", QStringLiteral("Apply"));
    connect(newButton, &QPushButton::clicked, this, [this] { (void)addStyle(); });
    connect(deleteButton, &QPushButton::clicked, this, [this] { (void)deleteStyle(); });
    connect(applyButton, &QPushButton::clicked, this, [this] { (void)apply(); });

    auto* buttons = new QHBoxLayout;
    buttons->addWidget(newButton);
    buttons->addWidget(deleteButton);
    buttons->addStretch();
    buttons->addWidget(applyButton);
    auto* right = new QVBoxLayout;
    right->addLayout(form);
    right->addWidget(problem_);
    right->addStretch();
    right->addLayout(buttons);
    auto* layout = new QHBoxLayout(this);
    layout->addWidget(list_, 1);
    layout->addLayout(right, 2);

    connect(list_, &QListWidget::currentTextChanged, this, [this](const QString& name) {
        if (const auto* style = document_.model().textStyles.find(name.toStdString())) {
            showStyle(*style);
        }
    });
    listener_ = document_.addListener([this](const katana::cad::DocumentChange& change) {
        if (change.has(katana::cad::DocumentChange::AnnotationStyles |
                       katana::cad::DocumentChange::Replaced)) {
            refresh();
        }
    });
    refresh();
}

QString TextStyleManagerDialog::current() const
{
    return list_->currentItem() != nullptr ? list_->currentItem()->text() : QString();
}

void TextStyleManagerDialog::refresh()
{
    const QString keep = shown_.isEmpty() ? QString::fromUtf8(katana::entity::kDefaultTextStyleName)
                                          : shown_;
    list_->blockSignals(true);
    list_->clear();
    document_.model().textStyles.forEach([&](const katana::entity::TextStyle& style) {
        list_->addItem(QString::fromStdString(style.name));
    });
    list_->blockSignals(false);
    select(keep);
}

void TextStyleManagerDialog::select(const QString& name)
{
    const auto items = list_->findItems(name, Qt::MatchExactly);
    QString target = name;
    if (items.isEmpty()) {
        target = QString::fromUtf8(katana::entity::kDefaultTextStyleName);
    }
    const auto found = list_->findItems(target, Qt::MatchExactly);
    if (!found.isEmpty()) {
        list_->blockSignals(true);
        list_->setCurrentItem(found.front());
        list_->blockSignals(false);
    }
    if (const auto* style = document_.model().textStyles.find(target.toStdString())) {
        showStyle(*style);
    }
}

void TextStyleManagerDialog::showStyle(const katana::entity::TextStyle& style)
{
    shown_ = QString::fromStdString(style.name);
    font_->setText(QString::fromStdString(style.fontFamily));
    paper_->setValue(style.paperHeight);
    width_->setValue(style.widthFactor);
    oblique_->setValue(style.oblique * katana::math::kRadToDeg);
    bold_->setChecked(style.bold);
    italic_->setChecked(style.italic);
    colour_->setText(style.color ? QString::fromStdString(style.color->toHex()) : QString());
    mask_->setChecked(style.mask);
    margin_->setValue(style.maskMargin);
    readable_->setChecked(style.readable);
    spacing_->setValue(style.lineSpacing);
    QFont font = sample_->font();
    if (!style.fontFamily.empty()) {
        font.setFamily(QString::fromStdString(style.fontFamily));
    }
    font.setBold(style.bold);
    font.setItalic(style.italic);
    sample_->setFont(font);
}

katana::entity::TextStyle TextStyleManagerDialog::formStyle() const
{
    katana::entity::TextStyle style;
    if (const auto* stored = document_.model().textStyles.find(shown_.toStdString())) {
        style = *stored;
    }
    style.name = shown_.toStdString();
    style.fontFamily = font_->text().trimmed().toStdString();
    style.paperHeight = paper_->value();
    style.widthFactor = width_->value();
    style.oblique = oblique_->value() * katana::math::kDegToRad;
    style.bold = bold_->isChecked();
    style.italic = italic_->isChecked();
    const QString colour = colour_->text().trimmed();
    style.color.reset();
    if (!colour.isEmpty() && colour.compare(QStringLiteral("bylayer"), Qt::CaseInsensitive) != 0) {
        if (const auto parsed = katana::entity::Color::fromHex(colour.toStdString())) {
            style.color = *parsed;
        }
    }
    style.mask = mask_->isChecked();
    style.maskMargin = margin_->value();
    style.readable = readable_->isChecked();
    style.lineSpacing = spacing_->value();
    return style;
}

QString TextStyleManagerDialog::problem() const
{
    return problem_->text();
}

void TextStyleManagerDialog::report(const Status& status)
{
    problem_->setText(describe(status));
}

bool TextStyleManagerDialog::apply()
{
    const QString colour = colour_->text().trimmed();
    if (!colour.isEmpty() && colour.compare(QStringLiteral("bylayer"), Qt::CaseInsensitive) != 0 &&
        !katana::entity::Color::fromHex(colour.toStdString())) {
        problem_->setText(QStringLiteral("colour must be ByLayer or #RRGGBB: ") + colour);
        return false;
    }
    const auto* stored = document_.model().textStyles.find(shown_.toStdString());
    const katana::entity::TextStyle style = formStyle();
    if (stored != nullptr && *stored == style) {
        report({});
        return true; // unchanged: no step
    }
    const Status status = document_.execute(cmd::updateTextStyle(style));
    report(status);
    return static_cast<bool>(status);
}

bool TextStyleManagerDialog::addStyle()
{
    katana::entity::TextStyle style;
    style.name = freshName(document_.model().textStyles, QStringLiteral("Text Style")).toStdString();
    const Status status = document_.execute(cmd::createTextStyle(style));
    report(status);
    if (status) {
        select(QString::fromStdString(style.name));
    }
    return static_cast<bool>(status);
}

bool TextStyleManagerDialog::deleteStyle()
{
    const Status status = document_.execute(cmd::deleteTextStyle(current().toStdString()));
    report(status);
    if (status) {
        shown_.clear();
        refresh();
    }
    return static_cast<bool>(status);
}

// ---- label styles and rules -------------------------------------------------------------

LabelStyleManagerDialog::LabelStyleManagerDialog(katana::cad::Document& document, QWidget* parent)
    : QDialog(parent), document_(document)
{
    using namespace katana::entity;
    setObjectName(QStringLiteral("labelStyleManagerDialog"));
    setWindowTitle(QStringLiteral("Label Styles and Rules"));
    tabs_ = new QTabWidget(this);
    tabs_->setObjectName(QStringLiteral("labelStyleTabs"));

    // The styles tab.
    auto* styles = new QWidget(tabs_);
    list_ = new QListWidget(styles);
    list_->setObjectName(QStringLiteral("labelStyleList"));
    static constexpr LabelKind kKinds[] = {LabelKind::Point, LabelKind::Segment, LabelKind::Arc,
                                           LabelKind::Area, LabelKind::Chainage};
    static constexpr LabelPlacement kPlacements[] = {
        LabelPlacement::Auto,     LabelPlacement::Above, LabelPlacement::Below,
        LabelPlacement::Along,    LabelPlacement::Centroid, LabelPlacement::Right,
        LabelPlacement::Left};
    static constexpr LabelOrientation kOrientations[] = {LabelOrientation::Aligned,
                                                         LabelOrientation::Horizontal};
    static constexpr LabelMarker kMarkers[] = {LabelMarker::None, LabelMarker::Cross,
                                               LabelMarker::Dot, LabelMarker::Circle};
    kind_ = combo(styles, "labelStyleKind", kKinds);
    kind_->setEnabled(false); // fixed once made; New asks for it
    template_ = edit(styles, "labelStyleTemplate", QStringLiteral("{bearing:dms} {distance:.3f}"));
    textStyle_ = new QComboBox(styles);
    textStyle_->setObjectName(QStringLiteral("labelStyleTextStyle"));
    paper_ = spin(styles, "labelStylePaperHeight", 0.0, 100.0, 0.5, 2, QStringLiteral(" mm"));
    placement_ = combo(styles, "labelStylePlacement", kPlacements);
    orientation_ = combo(styles, "labelStyleOrientation", kOrientations);
    offset_ = spin(styles, "labelStyleOffset", 0.0, 100.0, 0.25, 2, QStringLiteral(" mm"));
    leader_ = check(styles, "labelStyleLeader", QStringLiteral("Leader when displaced"));
    displace_ = check(styles, "labelStyleDisplace", QStringLiteral("May move to make room"));
    priority_ = new QSpinBox(styles);
    priority_->setObjectName(QStringLiteral("labelStylePriority"));
    priority_->setRange(-1000, 1000);
    marker_ = combo(styles, "labelStyleMarker", kMarkers);
    markerSize_ = spin(styles, "labelStyleMarkerSize", 0.0, 50.0, 0.25, 2, QStringLiteral(" mm"));
    minimumLength_ = spin(styles, "labelStyleMinimumLength", 0.0, 500.0, 1.0, 1, QStringLiteral(" mm"));
    interval_ = spin(styles, "labelStyleInterval", 0.001, 1.0e6, 10.0, 3);
    tick_ = spin(styles, "labelStyleTickInterval", 0.0, 1.0e6, 5.0, 3);
    tickLength_ = spin(styles, "labelStyleTickLength", 0.0, 50.0, 0.25, 2, QStringLiteral(" mm"));
    templateCheck_ = new QLabel(styles);
    templateCheck_->setObjectName(QStringLiteral("labelStyleTemplateCheck"));
    templateCheck_->setWordWrap(true);
    connect(template_, &QLineEdit::textChanged, this, [this] { checkTemplate(); });

    auto* form = new QFormLayout;
    form->addRow(QStringLiteral("Kind"), kind_);
    form->addRow(QStringLiteral("Template"), template_);
    form->addRow(templateCheck_);
    form->addRow(QStringLiteral("Text style"), textStyle_);
    form->addRow(QStringLiteral("Height on paper (0: the text style's)"), paper_);
    form->addRow(QStringLiteral("Placement"), placement_);
    form->addRow(QStringLiteral("Orientation"), orientation_);
    form->addRow(QStringLiteral("Offset"), offset_);
    form->addRow(leader_, displace_);
    form->addRow(QStringLiteral("Priority"), priority_);
    form->addRow(QStringLiteral("Marker"), marker_);
    form->addRow(QStringLiteral("Marker size"), markerSize_);
    form->addRow(QStringLiteral("Shortest segment labelled"), minimumLength_);
    form->addRow(QStringLiteral("Chainage label interval"), interval_);
    form->addRow(QStringLiteral("Chainage tick interval"), tick_);
    form->addRow(QStringLiteral("Tick length"), tickLength_);

    auto* newKind = combo(styles, "labelStyleNewKind", kKinds);
    auto* newButton = button(styles, "labelStyleNew", QStringLiteral("New"));
    auto* deleteButton = button(styles, "labelStyleDelete", QStringLiteral("Delete"));
    auto* defaultsButton = button(styles, "labelStyleDefaults", QStringLiteral("Add Standard Set"));
    auto* applyButton = button(styles, "labelStyleApply", QStringLiteral("Apply"));
    connect(newButton, &QPushButton::clicked, this, [this, newKind] {
        choose(kind_, chosen<LabelKind>(newKind));
        (void)addStyle();
    });
    connect(deleteButton, &QPushButton::clicked, this, [this] { (void)deleteStyle(); });
    connect(defaultsButton, &QPushButton::clicked, this, [this] { (void)addDefaults(); });
    connect(applyButton, &QPushButton::clicked, this, [this] { (void)apply(); });
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(newKind);
    buttons->addWidget(newButton);
    buttons->addWidget(deleteButton);
    buttons->addWidget(defaultsButton);
    buttons->addStretch();
    buttons->addWidget(applyButton);
    auto* right = new QVBoxLayout;
    right->addLayout(form);
    right->addStretch();
    right->addLayout(buttons);
    auto* stylesLayout = new QHBoxLayout(styles);
    stylesLayout->addWidget(list_, 1);
    stylesLayout->addLayout(right, 2);
    tabs_->addTab(styles, QStringLiteral("Label Styles"));
    connect(list_, &QListWidget::currentTextChanged, this, [this](const QString& name) {
        if (const auto* style = document_.model().labelStyles.find(name.toStdString())) {
            showStyle(*style);
        }
    });

    // The rules tab.
    auto* rulesPage = new QWidget(tabs_);
    rules_ = new QTableWidget(0, 6, rulesPage);
    rules_->setObjectName(QStringLiteral("labelRuleTable"));
    rules_->setHorizontalHeaderLabels({QStringLiteral("Rule"), QStringLiteral("Label style"),
                                       QStringLiteral("Layer"), QStringLiteral("Code"),
                                       QStringLiteral("Labels go on"), QStringLiteral("Enabled")});
    rules_->horizontalHeader()->setStretchLastSection(true);
    rules_->setSelectionBehavior(QAbstractItemView::SelectRows);
    rules_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    ruleName_ = edit(rulesPage, "labelRuleName", QStringLiteral("name"));
    ruleStyle_ = new QComboBox(rulesPage);
    ruleStyle_->setObjectName(QStringLiteral("labelRuleStyle"));
    ruleLayer_ = edit(rulesPage, "labelRuleLayer", QStringLiteral("layer glob, e.g. cadastre*"));
    ruleCode_ = edit(rulesPage, "labelRuleCode", QStringLiteral("code glob, e.g. BDY*"));
    ruleLabelLayer_ = edit(rulesPage, "labelRuleLabelLayer", QStringLiteral("the target's layer"));
    runReport_ = new QLabel(rulesPage);
    runReport_->setObjectName(QStringLiteral("labelRuleReport"));
    auto* addRuleButton = button(rulesPage, "labelRuleAdd", QStringLiteral("Add Rule"));
    auto* deleteRuleButton = button(rulesPage, "labelRuleDelete", QStringLiteral("Delete Rule"));
    auto* runButton = button(rulesPage, "labelRuleRun", QStringLiteral("Run Rules"));
    auto* clearButton = button(rulesPage, "labelRuleClear", QStringLiteral("Remove Rule Labels"));
    connect(addRuleButton, &QPushButton::clicked, this, [this] { (void)addRule(); });
    connect(deleteRuleButton, &QPushButton::clicked, this, [this] { (void)deleteRule(); });
    connect(runButton, &QPushButton::clicked, this, [this] { (void)runRules(); });
    connect(clearButton, &QPushButton::clicked, this, [this] { (void)clearRules(); });
    auto* ruleForm = new QHBoxLayout;
    for (QWidget* widget : {static_cast<QWidget*>(ruleName_), static_cast<QWidget*>(ruleStyle_),
                            static_cast<QWidget*>(ruleLayer_), static_cast<QWidget*>(ruleCode_),
                            static_cast<QWidget*>(ruleLabelLayer_),
                            static_cast<QWidget*>(addRuleButton)}) {
        ruleForm->addWidget(widget);
    }
    auto* ruleButtons = new QHBoxLayout;
    ruleButtons->addWidget(deleteRuleButton);
    ruleButtons->addStretch();
    ruleButtons->addWidget(clearButton);
    ruleButtons->addWidget(runButton);
    auto* rulesLayout = new QVBoxLayout(rulesPage);
    rulesLayout->addWidget(rules_);
    rulesLayout->addLayout(ruleForm);
    rulesLayout->addWidget(runReport_);
    rulesLayout->addLayout(ruleButtons);
    tabs_->addTab(rulesPage, QStringLiteral("Auto-Label Rules"));

    problem_ = problemLine(this, "labelStyleProblem");
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(tabs_);
    layout->addWidget(problem_);

    listener_ = document_.addListener([this](const katana::cad::DocumentChange& change) {
        if (change.has(katana::cad::DocumentChange::AnnotationStyles |
                       katana::cad::DocumentChange::Replaced)) {
            refresh();
        }
    });
    refresh();
}

QString LabelStyleManagerDialog::current() const
{
    return list_->currentItem() != nullptr ? list_->currentItem()->text() : QString();
}

void LabelStyleManagerDialog::refresh()
{
    const auto& model = document_.model();
    const QString keep = shown_;
    list_->blockSignals(true);
    list_->clear();
    ruleStyle_->clear();
    model.labelStyles.forEach([&](const katana::entity::LabelStyle& style) {
        list_->addItem(QString::fromStdString(style.name));
        ruleStyle_->addItem(QString::fromStdString(style.name));
    });
    list_->blockSignals(false);
    textStyle_->clear();
    model.textStyles.forEach([&](const katana::entity::TextStyle& style) {
        textStyle_->addItem(QString::fromStdString(style.name));
    });
    rules_->setRowCount(0);
    model.labelRules.forEach([&](const katana::entity::LabelRule& rule) {
        const int row = rules_->rowCount();
        rules_->insertRow(row);
        const QString cells[] = {QString::fromStdString(rule.name),
                                 QString::fromStdString(rule.labelStyle),
                                 QString::fromStdString(rule.layer),
                                 QString::fromStdString(rule.code),
                                 QString::fromStdString(rule.labelLayer),
                                 rule.enabled ? QStringLiteral("yes") : QStringLiteral("no")};
        for (int column = 0; column < 6; ++column) {
            rules_->setItem(row, column, new QTableWidgetItem(cells[column]));
        }
    });
    if (!keep.isEmpty()) {
        select(keep);
    } else if (list_->count() > 0) {
        select(list_->item(0)->text());
    }
}

void LabelStyleManagerDialog::select(const QString& name)
{
    const auto found = list_->findItems(name, Qt::MatchExactly);
    if (found.isEmpty()) {
        shown_.clear();
        return;
    }
    list_->blockSignals(true);
    list_->setCurrentItem(found.front());
    list_->blockSignals(false);
    if (const auto* style = document_.model().labelStyles.find(name.toStdString())) {
        showStyle(*style);
    }
}

void LabelStyleManagerDialog::showStyle(const katana::entity::LabelStyle& style)
{
    shown_ = QString::fromStdString(style.name);
    choose(kind_, style.kind);
    template_->setText(shownTemplate(style.text));
    const QString textStyle = style.textStyle.empty()
                                  ? QString::fromUtf8(katana::entity::kDefaultTextStyleName)
                                  : QString::fromStdString(style.textStyle);
    textStyle_->setCurrentIndex(textStyle_->findText(textStyle));
    paper_->setValue(style.paperHeight);
    choose(placement_, style.placement);
    choose(orientation_, style.orientation);
    offset_->setValue(style.offset);
    leader_->setChecked(style.leader);
    displace_->setChecked(style.displace);
    priority_->setValue(style.priority);
    choose(marker_, style.marker);
    markerSize_->setValue(style.markerSize);
    minimumLength_->setValue(style.minimumLength);
    interval_->setValue(style.interval);
    tick_->setValue(style.tickInterval);
    tickLength_->setValue(style.tickLength);
    checkTemplate();
}

void LabelStyleManagerDialog::checkTemplate()
{
    const Status status = katana::entity::checkLabelTemplate(
        storedTemplate(template_->text()), chosen<katana::entity::LabelKind>(kind_));
    templateCheck_->setText(status ? QStringLiteral("template: valid") : describe(status));
}

katana::entity::LabelStyle LabelStyleManagerDialog::formStyle() const
{
    using namespace katana::entity;
    LabelStyle style;
    if (const auto* stored = document_.model().labelStyles.find(shown_.toStdString())) {
        style = *stored;
    }
    style.name = shown_.toStdString();
    style.kind = chosen<LabelKind>(kind_);
    style.text = storedTemplate(template_->text());
    const std::string textStyle = textStyle_->currentText().toStdString();
    style.textStyle = textStyle == kDefaultTextStyleName ? std::string() : textStyle;
    style.paperHeight = paper_->value();
    style.placement = chosen<LabelPlacement>(placement_);
    style.orientation = chosen<LabelOrientation>(orientation_);
    style.offset = offset_->value();
    style.leader = leader_->isChecked();
    style.displace = displace_->isChecked();
    style.priority = priority_->value();
    style.marker = chosen<LabelMarker>(marker_);
    style.markerSize = markerSize_->value();
    style.minimumLength = minimumLength_->value();
    style.interval = interval_->value();
    style.tickInterval = tick_->value();
    style.tickLength = tickLength_->value();
    return style;
}

QString LabelStyleManagerDialog::problem() const
{
    return problem_->text();
}

QString LabelStyleManagerDialog::runReport() const
{
    return runReport_->text();
}

void LabelStyleManagerDialog::report(const Status& status)
{
    problem_->setText(describe(status));
}

bool LabelStyleManagerDialog::apply()
{
    const auto* stored = document_.model().labelStyles.find(shown_.toStdString());
    const katana::entity::LabelStyle style = formStyle();
    if (stored == nullptr || *stored == style) {
        report({});
        return stored != nullptr;
    }
    const Status status = document_.execute(cmd::updateLabelStyle(style));
    report(status);
    return static_cast<bool>(status);
}

bool LabelStyleManagerDialog::addStyle()
{
    katana::entity::LabelStyle style;
    style.kind = chosen<katana::entity::LabelKind>(kind_);
    style.name = freshName(document_.model().labelStyles, QStringLiteral("Label Style")).toStdString();
    // The kind's standard template, from the standard set when it has one.
    for (const katana::entity::LabelStyle& standard : ann::defaultLabelStyles()) {
        if (standard.kind == style.kind) {
            style.text = standard.text;
            style.placement = standard.placement;
            break;
        }
    }
    const Status status = document_.execute(cmd::createLabelStyle(style));
    report(status);
    if (status) {
        refresh();
        select(QString::fromStdString(style.name));
    }
    return static_cast<bool>(status);
}

bool LabelStyleManagerDialog::deleteStyle()
{
    const Status status = document_.execute(cmd::deleteLabelStyle(current().toStdString()));
    report(status);
    if (status) {
        shown_.clear();
        refresh();
    }
    return static_cast<bool>(status);
}

bool LabelStyleManagerDialog::addDefaults()
{
    auto transaction = std::make_unique<cmd::Transaction>("LABELSTYLE_DEFAULTS");
    for (const katana::entity::LabelStyle& style : ann::defaultLabelStyles()) {
        if (!document_.model().labelStyles.contains(style.name)) {
            transaction->add(cmd::createLabelStyle(style));
        }
    }
    if (transaction->size() == 0) {
        report({});
        return true;
    }
    const Status status = document_.execute(std::move(transaction));
    report(status);
    return static_cast<bool>(status);
}

bool LabelStyleManagerDialog::addRule()
{
    katana::entity::LabelRule rule;
    rule.name = ruleName_->text().trimmed().toStdString();
    rule.labelStyle = ruleStyle_->currentText().toStdString();
    rule.layer = ruleLayer_->text().trimmed().toStdString();
    rule.code = ruleCode_->text().trimmed().toStdString();
    rule.labelLayer = ruleLabelLayer_->text().trimmed().toStdString();
    const Status status = document_.execute(cmd::createLabelRule(rule));
    report(status);
    if (status) {
        ruleName_->clear();
    }
    return static_cast<bool>(status);
}

bool LabelStyleManagerDialog::deleteRule()
{
    const int row = rules_->currentRow();
    if (row < 0 || rules_->item(row, 0) == nullptr) {
        return false;
    }
    const Status status =
        document_.execute(cmd::deleteLabelRule(rules_->item(row, 0)->text().toStdString()));
    report(status);
    return static_cast<bool>(status);
}

bool LabelStyleManagerDialog::runRules()
{
    ann::AutoLabelReport result;
    auto built = ann::autoLabel(document_.model(), {}, &result);
    if (!built) {
        report(built.error());
        return false;
    }
    if (*built) {
        const Status status = document_.execute(std::move(*built));
        if (!status) {
            report(status);
            return false;
        }
    }
    report({});
    runReport_->setText(QStringLiteral("created=%1 kept=%2 removed=%3 skipped=%4")
                            .arg(result.created)
                            .arg(result.kept)
                            .arg(result.removed)
                            .arg(result.skipped));
    return true;
}

bool LabelStyleManagerDialog::clearRules()
{
    std::size_t removed = 0;
    auto built = ann::clearAutoLabels(document_.model(), {}, &removed);
    if (!built) {
        report(built.error());
        return false;
    }
    if (*built) {
        const Status status = document_.execute(std::move(*built));
        if (!status) {
            report(status);
            return false;
        }
    }
    report({});
    runReport_->setText(QStringLiteral("removed=%1").arg(removed));
    return true;
}

} // namespace katana::qt
