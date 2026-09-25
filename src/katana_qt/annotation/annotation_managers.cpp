#include "annotation/annotation_managers.hpp"

#include <string>
#include <utility>

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
#include <QMenu>
#include <QPushButton>
#include <QSpinBox>
#include <QTabWidget>
#include <QTableWidget>
#include <QToolButton>
#include <QVBoxLayout>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::qt {

namespace ann = katana::cad::annotation;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// The rule table's columns.
enum RuleColumn { kRuleName, kRuleStyle, kRuleLayer, kRuleCode, kRuleType, kRuleLabelLayer,
                  kRuleEnabled, kRuleLabels, kRuleColumns };

// The steps of the template language (entity/label_text.hpp, and HELP's list):
// the Insert Value menu offers each of them under every value it applies to,
// which checkLabelTemplate decides - so the menu cannot offer a step the
// template would refuse. One decimal of a second for dms.N and three places
// for .Nf stand for the rest.
constexpr const char* kSteps[] = {"m",   "mm",  "km",  "ft",    "m2",  "ha",  "km2", "ac",
                                  "deg", "rad", "gon", "dms",   "dms.1", "dm", "qb",  "ch",
                                  ".0f", ".1f", ".2f", ".3f",   "upper", "lower"};

// The values every kind has, before the kind's own (LABELSTYLE VALUES).
constexpr const char* kCommonValues[] = {"id", "layer", "code", "point", "description"};

// The entity types a rule may name (entity.hpp, EntityType) and an
// alignment, for a chainage style.
constexpr const char* kRuleTypes[] = {"Point",     "Line",   "Arc",   "Polyline", "Circle",
                                      "Text",      "Dimension", "Leader", "Alignment"};
const QString kAnyType = QStringLiteral("Any");

QDoubleSpinBox* spin(QWidget* parent, const char* name, double minimum, double maximum,
                     double step, int decimals, const QString& suffix = {})
{
    auto* box = new QDoubleSpinBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    box->setRange(minimum, maximum);
    box->setSingleStep(step);
    box->setDecimals(decimals);
    box->setSuffix(suffix);
    return box;
}

QCheckBox* check(QWidget* parent, const char* name, const QString& text)
{
    auto* box = new QCheckBox(text, parent);
    box->setObjectName(QString::fromLatin1(name));
    return box;
}

QLineEdit* edit(QWidget* parent, const char* name, const QString& placeholder = {})
{
    auto* line = new QLineEdit(parent);
    line->setObjectName(QString::fromLatin1(name));
    line->setPlaceholderText(placeholder);
    return line;
}

QPushButton* button(QWidget* parent, const char* name, const QString& text)
{
    auto* push = new QPushButton(text, parent);
    push->setObjectName(QString::fromLatin1(name));
    push->setAutoDefault(false);
    return push;
}

QLabel* problemLine(QWidget* parent, const char* name)
{
    auto* label = new QLabel(parent);
    label->setObjectName(QString::fromLatin1(name));
    label->setWordWrap(true);
    label->setStyleSheet(QStringLiteral("color: #d9534f"));
    return label;
}

// An enumeration's names in a combo, by value.
template <typename Enum, std::size_t N>
QComboBox* combo(QWidget* parent, const char* name, const Enum (&values)[N])
{
    auto* box = new QComboBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    for (const Enum value : values) {
        box->addItem(QString::fromUtf8(katana::entity::toString(value)), static_cast<int>(value));
    }
    return box;
}

template <typename Enum> Enum chosen(const QComboBox* box)
{
    return static_cast<Enum>(box->currentData().toInt());
}

template <typename Enum> void choose(QComboBox* box, Enum value)
{
    box->setCurrentIndex(box->findData(static_cast<int>(value)));
}

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
template <typename Table> QString freshNameIn(const Table& table, const QString& base)
{
    QString name = base;
    for (int n = 2; table.contains(name.toStdString()); ++n) {
        name = base + QStringLiteral(" ") + QString::number(n);
    }
    return name;
}

QString describe(const katana::core::Error& error)
{
    return QString::fromStdString(error.message +
                                  (error.context.empty() ? std::string() : ": " + error.context));
}

// A value as a line gives it: exact, so it reads back as stored.
QString exact(double value)
{
    return QString::fromStdString(katana::core::formatExactReal(value));
}

QString onOff(bool on)
{
    return on ? QStringLiteral("on") : QStringLiteral("off");
}

// `text` as one word of a line, or why it cannot be one.
Result<QString> word(const std::string& text)
{
    auto written = ann::commandWord(text);
    if (!written) {
        return written.error();
    }
    return QString::fromStdString(*written);
}

// Runs `line` through `run`; the refusal, or why the line could not be built,
// goes to `problem`. The reply comes back through `reply` when asked for.
bool runLine(const CommandRunner& run, QLabel& problem, const Result<QString>& line,
             QString* reply = nullptr)
{
    if (!line) {
        problem.setText(describe(line.error()));
        return false;
    }
    if (!run) {
        problem.setText(QStringLiteral("there is no command line to run the line on"));
        return false;
    }
    const VerbOutcome outcome = run(*line);
    problem.setText(outcome.ok ? QString()
                               : outcome.error.isEmpty() ? QStringLiteral("refused: ") + *line
                                                         : outcome.error);
    if (reply != nullptr) {
        *reply = outcome.reply;
    }
    return outcome.ok;
}

} // namespace

// ---- text styles ----------------------------------------------------------------------

TextStyleManagerDialog::TextStyleManagerDialog(katana::cad::Document& document, CommandRunner run,
                                               QWidget* parent)
    : QDialog(parent), document_(document), run_(std::move(run))
{
    setObjectName(QStringLiteral("textStyleManagerDialog"));
    setWindowTitle(QStringLiteral("Text Styles"));

    list_ = new QListWidget(this);
    list_->setObjectName(QStringLiteral("textStyleList"));
    newName_ = edit(this, "textStyleNewName");
    newName_->setToolTip(QStringLiteral("The name New gives; blank for the one shown"));
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
    newButton->setToolTip(QStringLiteral("A new text style of the name beside it (TEXTSTYLE NEW)"));
    auto* deleteButton = button(this, "textStyleDelete", QStringLiteral("Delete"));
    auto* applyButton = button(this, "textStyleApply", QStringLiteral("Apply"));
    connect(newButton, &QPushButton::clicked, this, [this] { (void)addStyle(); });
    connect(deleteButton, &QPushButton::clicked, this, [this] { (void)deleteStyle(); });
    connect(applyButton, &QPushButton::clicked, this, [this] { (void)apply(); });

    auto* naming = new QHBoxLayout;
    naming->addWidget(newName_, 1);
    naming->addWidget(newButton);
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(deleteButton);
    buttons->addStretch();
    buttons->addWidget(applyButton);
    auto* left = new QVBoxLayout;
    left->addWidget(list_, 1);
    left->addLayout(naming);
    auto* right = new QVBoxLayout;
    right->addLayout(form);
    right->addWidget(problem_);
    right->addStretch();
    right->addLayout(buttons);
    auto* layout = new QHBoxLayout(this);
    layout->addLayout(left, 1);
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

QString TextStyleManagerDialog::freshName() const
{
    return freshNameIn(document_.model().textStyles, QStringLiteral("Text Style"));
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
    newName_->setPlaceholderText(freshName());
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

Result<QString> TextStyleManagerDialog::applyLine() const
{
    const auto* stored = document_.model().textStyles.find(shown_.toStdString());
    if (stored == nullptr) {
        return makeError(ErrorCode::NotFound, "choose a text style first");
    }
    const QString colour = colour_->text().trimmed();
    if (!colour.isEmpty() && colour.compare(QStringLiteral("bylayer"), Qt::CaseInsensitive) != 0 &&
        !katana::entity::Color::fromHex(colour.toStdString())) {
        return makeError(ErrorCode::ParseFailure, "colour must be ByLayer or #RRGGBB",
                         colour.toStdString());
    }
    const katana::entity::TextStyle form = formStyle();
    // TEXTSTYLE SET's keys (annotation_verbs.cpp, kTextStyleKeys), only for
    // what the form changed.
    QStringList options;
    if (form.fontFamily != stored->fontFamily) {
        auto font = word(form.fontFamily);
        if (!font) {
            return font.error();
        }
        options << QStringLiteral("font=") + *font;
    }
    if (form.paperHeight != stored->paperHeight) {
        options << QStringLiteral("paper=") + exact(form.paperHeight);
    }
    if (form.widthFactor != stored->widthFactor) {
        options << QStringLiteral("width=") + exact(form.widthFactor);
    }
    if (form.oblique != stored->oblique) {
        // In degrees, as the box shows it and the verb takes it.
        options << QStringLiteral("oblique=") + exact(oblique_->value());
    }
    if (form.bold != stored->bold) {
        options << QStringLiteral("bold=") + onOff(form.bold);
    }
    if (form.italic != stored->italic) {
        options << QStringLiteral("italic=") + onOff(form.italic);
    }
    if (form.color != stored->color) {
        options << QStringLiteral("colour=") +
                       (form.color ? QString::fromStdString(form.color->toHex())
                                   : QStringLiteral("bylayer"));
    }
    if (form.mask != stored->mask) {
        options << QStringLiteral("mask=") + onOff(form.mask);
    }
    if (form.maskMargin != stored->maskMargin) {
        options << QStringLiteral("margin=") + exact(form.maskMargin);
    }
    if (form.readable != stored->readable) {
        options << QStringLiteral("readable=") + onOff(form.readable);
    }
    if (form.lineSpacing != stored->lineSpacing) {
        options << QStringLiteral("spacing=") + exact(form.lineSpacing);
    }
    if (options.isEmpty()) {
        return QString();
    }
    auto name = word(stored->name);
    if (!name) {
        return name.error();
    }
    return QStringLiteral("TEXTSTYLE SET ") + *name + QStringLiteral(" ") +
           options.join(QLatin1Char(' '));
}

QString TextStyleManagerDialog::problem() const
{
    return problem_->text();
}

bool TextStyleManagerDialog::run(const Result<QString>& line)
{
    return runLine(run_, *problem_, line);
}

bool TextStyleManagerDialog::apply()
{
    const auto line = applyLine();
    if (line && line->isEmpty()) {
        problem_->clear();
        return true; // unchanged: no step
    }
    return run(line);
}

bool TextStyleManagerDialog::addStyle()
{
    const QString typed = newName_->text().trimmed();
    const QString name = typed.isEmpty() ? freshName() : typed;
    auto written = word(name.toStdString());
    if (!written) {
        return run(written.error());
    }
    if (!run(QStringLiteral("TEXTSTYLE NEW ") + *written)) {
        return false;
    }
    newName_->clear();
    select(name);
    return true;
}

bool TextStyleManagerDialog::deleteStyle()
{
    auto written = word(current().toStdString());
    if (!written) {
        return run(written.error());
    }
    if (!run(QStringLiteral("TEXTSTYLE DELETE ") + *written)) {
        return false;
    }
    shown_.clear();
    refresh();
    return true;
}

// ---- label styles and rules -------------------------------------------------------------

LabelStyleManagerDialog::LabelStyleManagerDialog(katana::cad::Document& document,
                                                 CommandRunner run, QWidget* parent)
    : QDialog(parent), document_(document), run_(std::move(run))
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
    insert_ = new QToolButton(styles);
    insert_->setObjectName(QStringLiteral("labelTemplateInsert"));
    insert_->setText(QStringLiteral("Insert Value"));
    insert_->setToolTip(QStringLiteral(
        "The values a template of this kind may use, each with the steps that apply to it "
        "(LABELSTYLE VALUES); the choice goes in at the cursor"));
    insert_->setPopupMode(QToolButton::InstantPopup);
    insertMenu_ = new QMenu(insert_);
    insertMenu_->setObjectName(QStringLiteral("labelTemplateInsertMenu"));
    insert_->setMenu(insertMenu_);
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

    auto* templateRow = new QHBoxLayout;
    templateRow->addWidget(template_, 1);
    templateRow->addWidget(insert_);
    auto* form = new QFormLayout;
    form->addRow(QStringLiteral("Kind"), kind_);
    form->addRow(QStringLiteral("Template"), templateRow);
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

    newName_ = edit(styles, "labelStyleNewName");
    newName_->setToolTip(QStringLiteral("The name New gives; blank for the one shown"));
    newKind_ = combo(styles, "labelStyleNewKind", kKinds);
    newKind_->setToolTip(QStringLiteral("What the new style labels; fixed once it is made"));
    auto* newButton = button(styles, "labelStyleNew", QStringLiteral("New"));
    newButton->setToolTip(QStringLiteral("A new label style of the name and kind beside it "
                                         "(LABELSTYLE NEW)"));
    auto* deleteButton = button(styles, "labelStyleDelete", QStringLiteral("Delete"));
    auto* defaultsButton = button(styles, "labelStyleDefaults", QStringLiteral("Add Standard Set"));
    auto* applyButton = button(styles, "labelStyleApply", QStringLiteral("Apply"));
    connect(newButton, &QPushButton::clicked, this, [this] { (void)addStyle(); });
    connect(deleteButton, &QPushButton::clicked, this, [this] { (void)deleteStyle(); });
    connect(defaultsButton, &QPushButton::clicked, this, [this] { (void)addDefaults(); });
    connect(applyButton, &QPushButton::clicked, this, [this] { (void)apply(); });
    // The name on a row of its own, so a long one is seen whole.
    auto* naming = new QHBoxLayout;
    naming->addWidget(newKind_, 1);
    naming->addWidget(newButton);
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(deleteButton);
    buttons->addWidget(defaultsButton);
    buttons->addStretch();
    buttons->addWidget(applyButton);
    auto* left = new QVBoxLayout;
    left->addWidget(list_, 1);
    left->addWidget(newName_);
    left->addLayout(naming);
    auto* right = new QVBoxLayout;
    right->addLayout(form);
    right->addStretch();
    right->addLayout(buttons);
    auto* stylesLayout = new QHBoxLayout(styles);
    stylesLayout->addLayout(left, 1);
    stylesLayout->addLayout(right, 2);
    tabs_->addTab(styles, QStringLiteral("Label Styles"));
    connect(list_, &QListWidget::currentTextChanged, this, [this](const QString& name) {
        if (const auto* style = document_.model().labelStyles.find(name.toStdString())) {
            showStyle(*style);
        }
    });

    // The rules tab.
    auto* rulesPage = new QWidget(tabs_);
    rules_ = new QTableWidget(0, kRuleColumns, rulesPage);
    rules_->setObjectName(QStringLiteral("labelRuleTable"));
    rules_->setHorizontalHeaderLabels(
        {QStringLiteral("Rule"), QStringLiteral("Label style"), QStringLiteral("Layer"),
         QStringLiteral("Code"), QStringLiteral("Type"), QStringLiteral("Labels go on"),
         QStringLiteral("Enabled"), QStringLiteral("Labels")});
    rules_->horizontalHeader()->setStretchLastSection(true);
    rules_->setSelectionBehavior(QAbstractItemView::SelectRows);
    // Several rules at once: Run, Preview and Clear act on the ones chosen.
    rules_->setSelectionMode(QAbstractItemView::ExtendedSelection);
    rules_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    rules_->verticalHeader()->hide();
    ruleName_ = edit(rulesPage, "labelRuleName", QStringLiteral("name"));
    ruleStyle_ = new QComboBox(rulesPage);
    ruleStyle_->setObjectName(QStringLiteral("labelRuleStyle"));
    ruleLayer_ = edit(rulesPage, "labelRuleLayer", QStringLiteral("layer glob, e.g. cadastre*"));
    ruleCode_ = edit(rulesPage, "labelRuleCode", QStringLiteral("code glob, e.g. BDY*"));
    ruleType_ = new QComboBox(rulesPage);
    ruleType_->setObjectName(QStringLiteral("labelRuleType"));
    ruleType_->setToolTip(QStringLiteral("The kind of entity labelled - a glob, so Poly* is "
                                         "allowed - or Any for every one the style labels"));
    ruleType_->setEditable(true);
    ruleType_->addItem(kAnyType);
    for (const char* type : kRuleTypes) {
        ruleType_->addItem(QString::fromLatin1(type));
    }
    ruleLabelLayer_ = edit(rulesPage, "labelRuleLabelLayer", QStringLiteral("the target's layer"));
    ruleEnabled_ = check(rulesPage, "labelRuleEnabled", QStringLiteral("Enabled"));
    ruleEnabled_->setChecked(true);
    runReport_ = new QLabel(rulesPage);
    runReport_->setObjectName(QStringLiteral("labelRuleReport"));
    runReport_->setWordWrap(true);
    auto* addRuleButton = button(rulesPage, "labelRuleAdd", QStringLiteral("Add Rule"));
    auto* updateRuleButton = button(rulesPage, "labelRuleUpdate", QStringLiteral("Update Rule"));
    updateRuleButton->setToolTip(QStringLiteral("Store the form over the rule of its name "
                                                "(AUTOLABEL RULE SET)"));
    auto* deleteRuleButton = button(rulesPage, "labelRuleDelete", QStringLiteral("Delete Rule"));
    auto* previewButton = button(rulesPage, "labelRulePreview", QStringLiteral("Preview"));
    previewButton->setToolTip(QStringLiteral("What running the chosen rules would do, changing "
                                             "nothing (AUTOLABEL PREVIEW)"));
    auto* runButton = button(rulesPage, "labelRuleRun", QStringLiteral("Run Rules"));
    runButton->setToolTip(QStringLiteral("Label what the chosen rules match - every enabled "
                                         "rule when none is chosen - as one step (AUTOLABEL RUN)"));
    auto* clearButton = button(rulesPage, "labelRuleClear", QStringLiteral("Remove Rule Labels"));
    clearButton->setToolTip(QStringLiteral("Remove the labels the chosen rules made - every "
                                           "rule's when none is chosen (AUTOLABEL CLEAR)"));
    connect(addRuleButton, &QPushButton::clicked, this, [this] { (void)addRule(); });
    connect(updateRuleButton, &QPushButton::clicked, this, [this] { (void)updateRule(); });
    connect(deleteRuleButton, &QPushButton::clicked, this, [this] { (void)deleteRule(); });
    connect(previewButton, &QPushButton::clicked, this, [this] { (void)previewRules(); });
    connect(runButton, &QPushButton::clicked, this, [this] { (void)runRules(); });
    connect(clearButton, &QPushButton::clicked, this, [this] { (void)clearRules(); });
    auto* ruleForm = new QHBoxLayout;
    for (QWidget* widget : {static_cast<QWidget*>(ruleName_), static_cast<QWidget*>(ruleStyle_),
                            static_cast<QWidget*>(ruleLayer_), static_cast<QWidget*>(ruleCode_),
                            static_cast<QWidget*>(ruleType_),
                            static_cast<QWidget*>(ruleLabelLayer_),
                            static_cast<QWidget*>(ruleEnabled_)}) {
        ruleForm->addWidget(widget);
    }
    auto* ruleEdits = new QHBoxLayout;
    ruleEdits->addWidget(addRuleButton);
    ruleEdits->addWidget(updateRuleButton);
    ruleEdits->addWidget(deleteRuleButton);
    ruleEdits->addStretch();
    auto* ruleButtons = new QHBoxLayout;
    ruleButtons->addStretch();
    ruleButtons->addWidget(clearButton);
    ruleButtons->addWidget(previewButton);
    ruleButtons->addWidget(runButton);
    auto* rulesLayout = new QVBoxLayout(rulesPage);
    rulesLayout->addWidget(rules_);
    rulesLayout->addLayout(ruleForm);
    rulesLayout->addLayout(ruleEdits);
    rulesLayout->addWidget(runReport_);
    rulesLayout->addLayout(ruleButtons);
    tabs_->addTab(rulesPage, QStringLiteral("Auto-Label Rules"));
    // A row chosen shows its rule in the form, to edit and Update.
    connect(rules_, &QTableWidget::currentCellChanged, this, [this](int row) {
        if (!filling_ && row >= 0 && rules_->item(row, kRuleName) != nullptr) {
            showRule(rules_->item(row, kRuleName)->text());
        }
    });
    // The Enabled box switches the rule, as its own line - run once the
    // box's signal has returned: the rule's change rebuilds the table, which
    // would delete the item inside its own signal (docs/desktop.md, "Panels
    // refresh on the event loop").
    connect(rules_, &QTableWidget::itemChanged, this, [this](QTableWidgetItem* item) {
        if (filling_ || item->column() != kRuleEnabled) {
            return;
        }
        const QTableWidgetItem* name = rules_->item(item->row(), kRuleName);
        if (name != nullptr) {
            const QString rule = name->text();
            const bool enabled = item->checkState() == Qt::Checked;
            QMetaObject::invokeMethod(
                this, [this, rule, enabled] { (void)setRuleEnabled(rule, enabled); },
                Qt::QueuedConnection);
        }
    });

    problem_ = problemLine(this, "labelStyleProblem");
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(tabs_);
    layout->addWidget(problem_);
    // Room for the template and for every column of the rules table: at its
    // smallest the table hid Enabled and Labels behind a scroll bar.
    resize(940, 640);

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

QString LabelStyleManagerDialog::freshName() const
{
    return freshNameIn(document_.model().labelStyles, QStringLiteral("Label Style"));
}

void LabelStyleManagerDialog::refresh()
{
    const auto& model = document_.model();
    const QString keep = shown_;
    list_->blockSignals(true);
    list_->clear();
    const QString ruleStyle = ruleStyle_->currentText();
    ruleStyle_->clear();
    model.labelStyles.forEach([&](const katana::entity::LabelStyle& style) {
        list_->addItem(QString::fromStdString(style.name));
        ruleStyle_->addItem(QString::fromStdString(style.name));
    });
    list_->blockSignals(false);
    if (const int index = ruleStyle_->findText(ruleStyle); index >= 0) {
        ruleStyle_->setCurrentIndex(index);
    }
    newName_->setPlaceholderText(freshName());
    textStyle_->clear();
    model.textStyles.forEach([&](const katana::entity::TextStyle& style) {
        textStyle_->addItem(QString::fromStdString(style.name));
    });
    // The chosen rules stay chosen through a rebuild, so a Run after an
    // Enabled click runs what was chosen.
    const QStringList chosen = chosenRules();
    filling_ = true;
    rules_->setRowCount(0);
    model.labelRules.forEach([&](const katana::entity::LabelRule& rule) {
        const int row = rules_->rowCount();
        rules_->insertRow(row);
        const QString name = QString::fromStdString(rule.name);
        const QString cells[kRuleColumns] = {
            name,
            QString::fromStdString(rule.labelStyle),
            QString::fromStdString(rule.layer),
            QString::fromStdString(rule.code),
            rule.entityType.empty() ? kAnyType : QString::fromStdString(rule.entityType),
            QString::fromStdString(rule.labelLayer),
            QString(),
            labelCounts_.value(name)};
        for (int column = 0; column < kRuleColumns; ++column) {
            auto* item = new QTableWidgetItem(cells[column]);
            if (column == kRuleEnabled) {
                item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
                item->setCheckState(rule.enabled ? Qt::Checked : Qt::Unchecked);
                item->setText(rule.enabled ? QStringLiteral("yes") : QStringLiteral("no"));
            }
            rules_->setItem(row, column, item);
        }
        if (chosen.contains(name)) {
            // Added to what is chosen, not in place of it as selectRow's
            // click would be.
            rules_->selectionModel()->select(rules_->model()->index(row, kRuleName),
                                             QItemSelectionModel::Select |
                                                 QItemSelectionModel::Rows);
        }
    });
    filling_ = false;
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
    fillInsertMenu();
}

void LabelStyleManagerDialog::showRule(const QString& name)
{
    const auto* rule = document_.model().labelRules.find(name.toStdString());
    if (rule == nullptr) {
        return;
    }
    ruleName_->setText(QString::fromStdString(rule->name));
    ruleStyle_->setCurrentIndex(ruleStyle_->findText(QString::fromStdString(rule->labelStyle)));
    ruleLayer_->setText(QString::fromStdString(rule->layer));
    ruleCode_->setText(QString::fromStdString(rule->code));
    const QString type = rule->entityType.empty() ? kAnyType
                                                  : QString::fromStdString(rule->entityType);
    if (const int index = ruleType_->findText(type); index >= 0) {
        ruleType_->setCurrentIndex(index);
    } else {
        ruleType_->setEditText(type);
    }
    ruleLabelLayer_->setText(QString::fromStdString(rule->labelLayer));
    ruleEnabled_->setChecked(rule->enabled);
}

void LabelStyleManagerDialog::checkTemplate()
{
    const katana::core::Status status = katana::entity::checkLabelTemplate(
        storedTemplate(template_->text()), chosen<katana::entity::LabelKind>(kind_));
    templateCheck_->setText(status ? QStringLiteral("template: valid") : describe(status.error()));
}

void LabelStyleManagerDialog::fillInsertMenu()
{
    // What the kind may say, each with the steps checkLabelTemplate takes
    // after it: the menu offers only what the template would accept.
    const auto kind = chosen<katana::entity::LabelKind>(kind_);
    const auto valid = [kind](const std::string& field) {
        return static_cast<bool>(katana::entity::checkLabelTemplate("{" + field + "}", kind));
    };
    // The last kind's values go, submenus and all: clear() takes only the
    // menu's own actions, and a submenu left behind would still be found by
    // its actions' names.
    for (QMenu* old : insertMenu_->findChildren<QMenu*>(Qt::FindDirectChildrenOnly)) {
        delete old;
    }
    insertMenu_->clear();
    std::vector<std::string> values(std::begin(kCommonValues), std::end(kCommonValues));
    for (const std::string_view value : katana::entity::labelValueNames(kind)) {
        values.emplace_back(value);
    }
    for (const std::string& value : values) {
        if (!valid(value)) {
            continue;
        }
        const QString name = QString::fromStdString(value);
        QMenu* sub = insertMenu_->addMenu(name);
        QAction* plain = sub->addAction(QStringLiteral("{%1}").arg(name));
        plain->setObjectName(QStringLiteral("labelValue:") + name);
        connect(plain, &QAction::triggered, this, [this, name] { insertValue(name); });
        for (const char* step : kSteps) {
            const std::string field = value + ":" + step;
            if (!valid(field)) {
                continue;
            }
            const QString text = QString::fromStdString(field);
            QAction* stepped = sub->addAction(QStringLiteral("{%1}").arg(text));
            stepped->setObjectName(QStringLiteral("labelValue:") + text);
            connect(stepped, &QAction::triggered, this, [this, text] { insertValue(text); });
        }
    }
    // Any property of the target, by name.
    QAction* property = insertMenu_->addAction(QStringLiteral("{prop.NAME}"));
    property->setObjectName(QStringLiteral("labelValue:prop.NAME"));
    connect(property, &QAction::triggered, this,
            [this] { insertValue(QStringLiteral("prop.NAME")); });
}

void LabelStyleManagerDialog::insertValue(const QString& field)
{
    template_->insert(QStringLiteral("{%1}").arg(field));
    if (field == QStringLiteral("prop.NAME")) {
        // "NAME" is the property's to type: leave it selected.
        const int end = template_->cursorPosition() - 1;
        template_->setSelection(end - 4, 4);
    }
    template_->setFocus(Qt::OtherFocusReason);
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

Result<QString> LabelStyleManagerDialog::applyLine() const
{
    using katana::entity::toString;
    const auto* stored = document_.model().labelStyles.find(shown_.toStdString());
    if (stored == nullptr) {
        return makeError(ErrorCode::NotFound, "choose a label style first");
    }
    const katana::entity::LabelStyle form = formStyle();
    // LABELSTYLE SET's keys (annotation_verbs.cpp, kLabelStyleKeys), only for
    // what the form changed.
    QStringList options;
    if (form.text != stored->text) {
        auto text = ann::annotationTextWord(form.text);
        if (!text) {
            return text.error();
        }
        options << QStringLiteral("text=") + QString::fromStdString(*text);
    }
    if (form.textStyle != stored->textStyle) {
        auto name = word(form.textStyle);
        if (!name) {
            return name.error();
        }
        options << QStringLiteral("textstyle=") + *name;
    }
    const auto number = [&options](const char* key, double before, double after) {
        if (after != before) {
            options << QString::fromLatin1(key) + QStringLiteral("=") + exact(after);
        }
    };
    const auto named = [&options](const char* key, std::string_view before,
                                  std::string_view after) {
        if (after != before) {
            options << QString::fromLatin1(key) + QStringLiteral("=") +
                           QString::fromUtf8(after.data(), static_cast<qsizetype>(after.size()));
        }
    };
    number("paper", stored->paperHeight, form.paperHeight);
    named("placement", toString(stored->placement), toString(form.placement));
    named("orientation", toString(stored->orientation), toString(form.orientation));
    number("offset", stored->offset, form.offset);
    if (form.leader != stored->leader) {
        options << QStringLiteral("leader=") + onOff(form.leader);
    }
    if (form.displace != stored->displace) {
        options << QStringLiteral("displace=") + onOff(form.displace);
    }
    if (form.priority != stored->priority) {
        options << QStringLiteral("priority=%1").arg(form.priority);
    }
    named("marker", toString(stored->marker), toString(form.marker));
    number("markersize", stored->markerSize, form.markerSize);
    number("minlength", stored->minimumLength, form.minimumLength);
    number("interval", stored->interval, form.interval);
    number("tick", stored->tickInterval, form.tickInterval);
    number("ticklength", stored->tickLength, form.tickLength);
    if (options.isEmpty()) {
        return QString();
    }
    auto name = word(stored->name);
    if (!name) {
        return name.error();
    }
    return QStringLiteral("LABELSTYLE SET ") + *name + QStringLiteral(" ") +
           options.join(QLatin1Char(' '));
}

QString LabelStyleManagerDialog::problem() const
{
    return problem_->text();
}

QString LabelStyleManagerDialog::runReport() const
{
    return runReport_->text();
}

QStringList LabelStyleManagerDialog::chosenRules() const
{
    QStringList names;
    if (rules_->selectionModel() == nullptr) {
        return names;
    }
    for (int row = 0; row < rules_->rowCount(); ++row) {
        if (rules_->selectionModel()->isRowSelected(row, QModelIndex()) &&
            rules_->item(row, kRuleName) != nullptr) {
            names << rules_->item(row, kRuleName)->text();
        }
    }
    return names;
}

bool LabelStyleManagerDialog::run(const Result<QString>& line)
{
    return runLine(run_, *problem_, line);
}

bool LabelStyleManagerDialog::apply()
{
    const auto line = applyLine();
    if (line && line->isEmpty()) {
        problem_->clear();
        return true; // unchanged: no step
    }
    return run(line);
}

bool LabelStyleManagerDialog::addStyle()
{
    const QString typed = newName_->text().trimmed();
    const QString name = typed.isEmpty() ? freshName() : typed;
    auto written = word(name.toStdString());
    if (!written) {
        return run(written.error());
    }
    // The kind's standard template comes with it, as LABELSTYLE NEW gives.
    const QString kind = QString::fromUtf8(
        katana::entity::toString(chosen<katana::entity::LabelKind>(newKind_)));
    if (!run(QStringLiteral("LABELSTYLE NEW %1 kind=%2").arg(*written, kind))) {
        return false;
    }
    newName_->clear();
    select(name);
    return true;
}

bool LabelStyleManagerDialog::deleteStyle()
{
    auto written = word(current().toStdString());
    if (!written) {
        return run(written.error());
    }
    if (!run(QStringLiteral("LABELSTYLE DELETE ") + *written)) {
        return false;
    }
    shown_.clear();
    refresh();
    return true;
}

bool LabelStyleManagerDialog::addDefaults()
{
    return run(QStringLiteral("LABELSTYLE DEFAULTS"));
}

Result<QString> LabelStyleManagerDialog::ruleOptions(bool everyField) const
{
    // A filter left blank is "any": sent blank ("") only where it replaces
    // one the rule has (everyField, for RULE SET).
    QStringList options;
    const auto add = [&](const char* key, const QString& value) -> katana::core::Status {
        if (value.isEmpty() && !everyField) {
            return {};
        }
        auto written = word(value.toStdString());
        if (!written) {
            return written.error();
        }
        options << QString::fromLatin1(key) + QStringLiteral("=") + *written;
        return {};
    };
    QString type = ruleType_->currentText().trimmed();
    if (type.compare(kAnyType, Qt::CaseInsensitive) == 0) {
        type.clear();
    }
    for (const auto& [key, value] :
         {std::pair{"style", ruleStyle_->currentText()},
          std::pair{"layer", ruleLayer_->text().trimmed()},
          std::pair{"code", ruleCode_->text().trimmed()}, std::pair{"type", type},
          std::pair{"labellayer", ruleLabelLayer_->text().trimmed()}}) {
        if (auto status = add(key, value); !status) {
            return status.error();
        }
    }
    options << QStringLiteral("enabled=") + onOff(ruleEnabled_->isChecked());
    return options.join(QLatin1Char(' '));
}

bool LabelStyleManagerDialog::addRule()
{
    auto name = word(ruleName_->text().trimmed().toStdString());
    if (!name) {
        return run(name.error());
    }
    const auto options = ruleOptions(false);
    if (!options) {
        return run(options.error());
    }
    if (!run(QStringLiteral("AUTOLABEL RULE ADD %1 %2").arg(*name, *options))) {
        return false;
    }
    ruleName_->clear();
    return true;
}

bool LabelStyleManagerDialog::updateRule()
{
    auto name = word(ruleName_->text().trimmed().toStdString());
    if (!name) {
        return run(name.error());
    }
    const auto options = ruleOptions(true);
    if (!options) {
        return run(options.error());
    }
    return run(QStringLiteral("AUTOLABEL RULE SET %1 %2").arg(*name, *options));
}

bool LabelStyleManagerDialog::deleteRule()
{
    const int row = rules_->currentRow();
    if (row < 0 || rules_->item(row, kRuleName) == nullptr) {
        problem_->setText(QStringLiteral("choose the rule to delete in the table"));
        return false;
    }
    auto name = word(rules_->item(row, kRuleName)->text().toStdString());
    if (!name) {
        return run(name.error());
    }
    return run(QStringLiteral("AUTOLABEL RULE DELETE ") + *name);
}

bool LabelStyleManagerDialog::setRuleEnabled(const QString& name, bool enabled)
{
    auto written = word(name.toStdString());
    if (!written) {
        return run(written.error());
    }
    const bool done =
        run(QStringLiteral("AUTOLABEL RULE SET %1 enabled=%2").arg(*written, onOff(enabled)));
    if (!done) {
        refresh(); // the box shows what the rule still is
    }
    return done;
}

bool LabelStyleManagerDialog::ruleVerb(const QString& action)
{
    QString line = QStringLiteral("AUTOLABEL ") + action;
    for (const QString& rule : chosenRules()) {
        auto written = word(rule.toStdString());
        if (!written) {
            return run(written.error());
        }
        line += QStringLiteral(" ") + *written;
    }
    QString reply;
    if (!runLine(run_, *problem_, line, &reply)) {
        return false;
    }
    // "autolabel created=1 kept=0 removed=0 skipped=0" (or "preview ...", or
    // CLEAR's "removed=1"), then "rule=NAME labels=N" a rule: the counts for
    // the report, the rules' for the Labels column.
    QStringList summary;
    for (const QString& replyLine : reply.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        if (!replyLine.startsWith(QStringLiteral("rule="))) {
            summary << replyLine;
            continue;
        }
        const auto words = katana::cad::CommandInterpreter::tokenize(replyLine.toStdString());
        if (!words) {
            continue;
        }
        QString rule;
        QString labels;
        for (const std::string& field : *words) {
            const QString text = QString::fromStdString(field);
            if (text.startsWith(QStringLiteral("rule="))) {
                rule = text.mid(5);
            } else if (text.startsWith(QStringLiteral("labels="))) {
                labels = text.mid(7);
            }
        }
        if (!rule.isEmpty()) {
            labelCounts_[rule] = labels;
        }
    }
    runReport_->setText(summary.join(QLatin1Char(' ')));
    refresh();
    return true;
}

bool LabelStyleManagerDialog::runRules()
{
    return ruleVerb(QStringLiteral("RUN"));
}

bool LabelStyleManagerDialog::previewRules()
{
    return ruleVerb(QStringLiteral("PREVIEW"));
}

bool LabelStyleManagerDialog::clearRules()
{
    // What CLEAR removes it removes by rule; the Labels column no longer
    // says how many those rules have.
    for (const QString& rule : chosenRules().isEmpty() ? labelCounts_.keys() : chosenRules()) {
        labelCounts_.remove(rule);
    }
    return ruleVerb(QStringLiteral("CLEAR"));
}

} // namespace katana::qt
