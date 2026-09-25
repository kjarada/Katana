#include "annotation/dimension_style_manager.hpp"

#include <array>
#include <utility>

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QIcon>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>
#include <QSpinBox>
#include <QTableWidget>
#include <QVBoxLayout>

#include "command_word.hpp"
#include "katana/cad/annotation/command_words.hpp"
#include "katana/cad/annotation/dimension_style_verbs.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/dimension_text.hpp"
#include "plan_painter.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace ann = katana::cad::annotation;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::ArrowHead;
using katana::entity::DimensionStyle;

namespace {

// The sample's size, and the length it measures: ten units, the dimension the
// preview line reads out, so the picture and the words agree.
constexpr int kSampleWidth = 300;
constexpr int kSampleHeight = 110;
constexpr double kSampleLength = 10.0;

// The heads, in the order the verb's help lists them, with the words a person
// reads; the combo's data is the name the verb takes.
struct HeadChoice {
    ArrowHead head;
    const char* label;
};
constexpr std::array<HeadChoice, 5> kHeads{{{ArrowHead::None, "None"},
                                            {ArrowHead::Tick, "Tick"},
                                            {ArrowHead::ClosedFilled, "Closed filled"},
                                            {ArrowHead::Open, "Open"},
                                            {ArrowHead::Dot, "Dot"}}};

QLineEdit* lineEdit(QWidget* parent, const char* name, const QString& tip)
{
    auto* line = new QLineEdit(parent);
    line->setObjectName(QString::fromLatin1(name));
    line->setToolTip(tip);
    // Room for a number such as 1000 or 0.625 to show whole.
    line->setMinimumWidth(line->fontMetrics().horizontalAdvance(QStringLiteral("0000.0000")));
    return line;
}

QPushButton* button(QWidget* parent, const char* name, const QString& text, const QString& tip)
{
    auto* push = new QPushButton(text, parent);
    push->setObjectName(QString::fromLatin1(name));
    push->setToolTip(tip);
    push->setAutoDefault(false);
    return push;
}

// A typed number, or the sentence that says which field did not read.
Result<double> numberIn(const QLineEdit* line, const char* what)
{
    const QString text = line->text().trimmed();
    if (const auto value = katana::core::parseFiniteDouble(text.toStdString())) {
        return *value;
    }
    return makeError(ErrorCode::ParseFailure,
                     std::string(what) + " must be a number, not '" + text.toStdString() + "'");
}

QString describe(const katana::core::Error& error)
{
    return QString::fromStdString(error.message +
                                  (error.context.empty() ? std::string() : ": " + error.context));
}

// A dimension of `length` along x, `offset` below its points, drawn with
// `style` into `size` pixels by the plan painter: the look of the style, not
// an imitation of it.
QPixmap drawnDimension(const DimensionStyle& style, double scale, QSize size, double length,
                       double offset, const QString& textOverride, const QColor& ink)
{
    katana::entity::DimensionGeometry dimension;
    dimension.start = katana::geometry::Point2(0.0, 0.0);
    dimension.end = katana::geometry::Point2(length, 0.0);
    dimension.offset = offset;
    dimension.textOverride = textOverride.toStdString();
    QPixmap pixmap(size);
    pixmap.fill(Qt::transparent);
    const katana::cad::DimensionDrawing drawing =
        katana::cad::buildDimension(dimension, style, scale);
    if (drawing.empty() || drawing.extent.empty()) {
        return pixmap;
    }
    PlanFrame frame;
    frame.transform.resize(size.width(), size.height());
    frame.transform.fit(drawing.extent, 0.08);
    PlanPaintOptions options;
    options.annotationScale = scale;
    PlanPaintCache cache;
    QPainter painter(&pixmap);
    painter.setRenderHint(QPainter::Antialiasing);
    painter.setPen(QPen(ink, 1.2));
    paintPlanGeometry(painter, frame, options, cache, dimension, style);
    return pixmap;
}

} // namespace

DimensionStyleManagerDialog::DimensionStyleManagerDialog(katana::cad::Document& document,
                                                         CommandRunner run, QWidget* parent)
    : QDialog(parent), document_(document), run_(std::move(run))
{
    setObjectName(QStringLiteral("dimensionStyleManagerDialog"));
    setWindowTitle(QStringLiteral("Dimension Styles"));

    list_ = new QTableWidget(0, 2, this);
    list_->setObjectName(QStringLiteral("dimStyleList"));
    list_->setHorizontalHeaderLabels({QStringLiteral("Style"), QStringLiteral("Used by")});
    list_->horizontalHeader()->setStretchLastSection(true);
    list_->verticalHeader()->hide();
    list_->setSelectionBehavior(QAbstractItemView::SelectRows);
    list_->setSelectionMode(QAbstractItemView::SingleSelection);
    list_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    newName_ = lineEdit(this, "dimStyleNewName",
                     QStringLiteral("The name New and Duplicate give; blank for the one shown"));

    const QString sizeTip = QStringLiteral(" (model units, or millimetres on paper when the style "
                                           "is paper-sized)");
    text_ = lineEdit(this, "dimStyleText", QStringLiteral("Text height") + sizeTip);
    gap_ = lineEdit(this, "dimStyleGap",
                 QStringLiteral("From the dimension line to its text") + sizeTip);
    prefix_ = lineEdit(this, "dimStylePrefix", QStringLiteral("Written before the number"));
    suffix_ = lineEdit(this, "dimStyleSuffix", QStringLiteral("Written after the number, e.g. mm"));
    extOff_ = lineEdit(this, "dimStyleExtOff",
                    QStringLiteral("The gap between a measured point and its extension line") +
                        sizeTip);
    extBeyond_ = lineEdit(this, "dimStyleExtBeyond",
                       QStringLiteral("How far an extension line runs past the dimension line") +
                           sizeTip);
    arrow_ = lineEdit(this, "dimStyleArrow", QStringLiteral("Arrowhead size") + sizeTip);
    head_ = new QComboBox(this);
    head_->setObjectName(QStringLiteral("dimStyleHead"));
    head_->setToolTip(QStringLiteral("How each end of the dimension line is drawn"));
    head_->setIconSize(QSize(36, 14));
    for (const HeadChoice& choice : kHeads) {
        // Each head's picture drawn by the plan painter, as a dimension with
        // that head and no text is: a line with the head at both ends.
        DimensionStyle look;
        look.arrowHead = choice.head;
        look.arrowSize = 3.0;
        look.extensionOffset = 0.0;
        look.extensionBeyond = 0.0;
        const QPixmap picture = drawnDimension(look, katana::entity::kDefaultAnnotationScale,
                                               QSize(36, 14), 14.0, 0.0, QStringLiteral(" "),
                                               theme::text());
        head_->addItem(QIcon(picture), QString::fromLatin1(choice.label),
                       QString::fromUtf8(katana::entity::toString(choice.head)));
    }
    scale_ = lineEdit(this, "dimStyleScale",
                   QStringLiteral("Multiplies the measurement before it is written: 1000 writes a "
                                  "drawing in metres in millimetres"));
    decimals_ = new QSpinBox(this);
    decimals_->setObjectName(QStringLiteral("dimStyleDecimals"));
    decimals_->setToolTip(QStringLiteral("Places after the decimal point"));
    // The range validate() allows (tables.cpp: twelve is where a double stops
    // having digits to give).
    decimals_->setRange(0, 12);
    round_ = lineEdit(this, "dimStyleRound",
                   QStringLiteral("Rounds the measurement to a multiple of this; 0 for none"));
    trim_ = new QCheckBox(QStringLiteral("Drop trailing zeros"), this);
    trim_->setObjectName(QStringLiteral("dimStyleTrim"));
    paper_ = new QCheckBox(QStringLiteral("Paper-sized (drawn at the annotation scale)"), this);
    paper_->setObjectName(QStringLiteral("dimStylePaper"));
    paper_->setToolTip(QStringLiteral(
        "The sizes are millimetres on paper, drawn at size x scale / 1000 in the view or sheet "
        "showing the dimension, so it reads the same size on every sheet"));
    units_ = new QLabel(this);
    units_->setObjectName(QStringLiteral("dimStyleUnits"));
    units_->setStyleSheet(QStringLiteral("color: %1").arg(theme::textMuted().name()));
    preview_ = new QLabel(this);
    preview_->setObjectName(QStringLiteral("dimStylePreview"));
    sample_ = new QLabel(this);
    sample_->setObjectName(QStringLiteral("dimStyleSample"));
    sample_->setFixedSize(kSampleWidth, kSampleHeight);
    sample_->setStyleSheet(QStringLiteral("background: %1; border: 1px solid %2")
                               .arg(theme::viewport().name(), theme::border().name()));
    problem_ = new QLabel(this);
    problem_->setObjectName(QStringLiteral("dimStyleProblem"));
    problem_->setWordWrap(true);
    problem_->setStyleSheet(QStringLiteral("color: %1").arg(theme::error().name()));

    auto* textGroup = new QGroupBox(QStringLiteral("Text"), this);
    auto* textForm = new QFormLayout(textGroup);
    textForm->addRow(QStringLiteral("Height"), text_);
    textForm->addRow(QStringLiteral("Gap to the line"), gap_);
    textForm->addRow(QStringLiteral("Prefix"), prefix_);
    textForm->addRow(QStringLiteral("Suffix"), suffix_);
    auto* linesGroup = new QGroupBox(QStringLiteral("Lines and arrows"), this);
    auto* linesForm = new QFormLayout(linesGroup);
    linesForm->addRow(QStringLiteral("Extension offset"), extOff_);
    linesForm->addRow(QStringLiteral("Extension beyond"), extBeyond_);
    linesForm->addRow(QStringLiteral("Arrow size"), arrow_);
    linesForm->addRow(QStringLiteral("Arrowhead"), head_);
    auto* unitsGroup = new QGroupBox(QStringLiteral("Units"), this);
    auto* unitsForm = new QFormLayout(unitsGroup);
    unitsForm->addRow(QStringLiteral("Unit scale"), scale_);
    unitsForm->addRow(QStringLiteral("Decimal places"), decimals_);
    unitsForm->addRow(QStringLiteral("Round to"), round_);
    unitsForm->addRow(trim_);

    auto* newButton = button(this, "dimStyleNew", QStringLiteral("New"),
                             QStringLiteral("A new style with the standard sizes (DIMSTYLE NEW)"));
    auto* duplicateButton =
        button(this, "dimStyleDuplicate", QStringLiteral("Duplicate"),
               QStringLiteral("A new style of what the form shows, in one step (DIMSTYLE NEW "
                              "name field value ...)"));
    auto* deleteButton =
        button(this, "dimStyleDelete", QStringLiteral("Delete"),
               QStringLiteral("Delete the style; refused while a layer uses it (DIMSTYLE DELETE)"));
    auto* revertButton =
        button(this, "dimStyleRevert", QStringLiteral("Revert"),
               QStringLiteral("Throw the form's edits away"));
    auto* applyButton =
        button(this, "dimStyleApply", QStringLiteral("Apply"),
               QStringLiteral("Store the fields the form changed, as one undo step "
                              "(DIMSTYLE SET)"));
    connect(newButton, &QPushButton::clicked, this, [this] { (void)newStyle(); });
    connect(duplicateButton, &QPushButton::clicked, this, [this] { (void)duplicateStyle(); });
    connect(deleteButton, &QPushButton::clicked, this, [this] { (void)deleteStyle(); });
    connect(revertButton, &QPushButton::clicked, this, [this] { revert(); });
    connect(applyButton, &QPushButton::clicked, this, [this] { (void)apply(); });

    auto* left = new QVBoxLayout;
    left->addWidget(list_, 1);
    left->addWidget(newName_);
    auto* naming = new QHBoxLayout;
    naming->addWidget(newButton);
    naming->addWidget(duplicateButton);
    naming->addWidget(deleteButton);
    left->addLayout(naming);

    auto* groups = new QHBoxLayout;
    groups->addWidget(textGroup);
    groups->addWidget(linesGroup);
    groups->addWidget(unitsGroup);
    auto* buttons = new QHBoxLayout;
    buttons->addStretch();
    buttons->addWidget(revertButton);
    buttons->addWidget(applyButton);
    auto* right = new QVBoxLayout;
    right->addLayout(groups);
    right->addWidget(paper_);
    right->addWidget(units_);
    right->addWidget(preview_);
    right->addWidget(sample_);
    right->addWidget(problem_);
    right->addStretch();
    right->addLayout(buttons);
    auto* layout = new QHBoxLayout(this);
    layout->addLayout(left, 1);
    layout->addLayout(right, 3);

    // The preview follows the form as it is typed in.
    for (QLineEdit* line :
         {text_, gap_, prefix_, suffix_, extOff_, extBeyond_, arrow_, scale_, round_}) {
        connect(line, &QLineEdit::textChanged, this, [this] { showPreview(); });
    }
    connect(head_, &QComboBox::currentIndexChanged, this, [this] { showPreview(); });
    connect(decimals_, &QSpinBox::valueChanged, this, [this] { showPreview(); });
    connect(trim_, &QCheckBox::toggled, this, [this] { showPreview(); });
    connect(paper_, &QCheckBox::toggled, this, [this] {
        showUnits();
        showPreview();
    });
    connect(list_, &QTableWidget::currentCellChanged, this, [this](int row) {
        if (loading_ || row < 0 || list_->item(row, 0) == nullptr) {
            return;
        }
        select(list_->item(row, 0)->text());
    });
    // Styles and the layers that use them, and the scale a paper-sized
    // sample is drawn at.
    listener_ = document_.addListener([this](const katana::cad::DocumentChange& change) {
        if (change.has(katana::cad::DocumentChange::DimensionStyles |
                       katana::cad::DocumentChange::Layers |
                       katana::cad::DocumentChange::Metadata |
                       katana::cad::DocumentChange::Replaced)) {
            refresh();
        }
    });
    refresh();
}

QString DimensionStyleManagerDialog::nameForNew(const QString& base) const
{
    const QString typed = newName_->text().trimmed();
    if (!typed.isEmpty()) {
        return typed;
    }
    QString name = base;
    for (int n = 2; document_.model().dimensionStyles.contains(name.toStdString()); ++n) {
        name = base + QStringLiteral(" ") + QString::number(n);
    }
    return name;
}

void DimensionStyleManagerDialog::refresh()
{
    const auto& model = document_.model();
    loading_ = true;
    list_->setRowCount(0);
    model.dimensionStyles.forEach([&](const DimensionStyle& style) {
        const int row = list_->rowCount();
        list_->insertRow(row);
        const std::size_t users = ann::layersUsingDimensionStyle(model, style.name).size();
        QString usedBy = users == 0 ? QStringLiteral("no layer")
                                    : users == 1 ? QStringLiteral("1 layer")
                                                 : QStringLiteral("%1 layers").arg(users);
        if (style.name == katana::entity::kDefaultDimensionStyleName) {
            usedBy += QStringLiteral(" (the default)");
        }
        list_->setItem(row, 0, new QTableWidgetItem(QString::fromStdString(style.name)));
        list_->setItem(row, 1, new QTableWidgetItem(usedBy));
    });
    loading_ = false;
    newName_->setPlaceholderText(nameForNew(QStringLiteral("Dimension Style")));

    const QString keep = shown_.isEmpty()
                             ? QString::fromUtf8(katana::entity::kDefaultDimensionStyleName)
                             : shown_;
    const DimensionStyle* stored = model.dimensionStyles.find(keep.toStdString());
    if (stored != nullptr && *stored == loaded_ && !shown_.isEmpty()) {
        // The style is as the form was loaded with it: keep what is being
        // typed, and only put the list's mark back on it.
        loading_ = true;
        for (int row = 0; row < list_->rowCount(); ++row) {
            if (list_->item(row, 0)->text() == keep) {
                list_->setCurrentCell(row, 0);
            }
        }
        loading_ = false;
        showPreview(); // the scale may have moved
        return;
    }
    select(keep);
}

void DimensionStyleManagerDialog::select(const QString& name)
{
    const auto& styles = document_.model().dimensionStyles;
    const QString target = styles.contains(name.toStdString())
                               ? name
                               : QString::fromUtf8(katana::entity::kDefaultDimensionStyleName);
    loading_ = true;
    for (int row = 0; row < list_->rowCount(); ++row) {
        if (list_->item(row, 0)->text() == target) {
            list_->setCurrentCell(row, 0);
        }
    }
    loading_ = false;
    if (const DimensionStyle* style = styles.find(target.toStdString())) {
        showStyle(*style);
    }
}

void DimensionStyleManagerDialog::showStyle(const DimensionStyle& style)
{
    shown_ = QString::fromStdString(style.name);
    loaded_ = style;
    const bool wasLoading = std::exchange(loading_, true);
    text_->setText(exactNumber(style.textHeight));
    gap_->setText(exactNumber(style.textGap));
    prefix_->setText(QString::fromStdString(style.prefix));
    suffix_->setText(QString::fromStdString(style.suffix));
    extOff_->setText(exactNumber(style.extensionOffset));
    extBeyond_->setText(exactNumber(style.extensionBeyond));
    arrow_->setText(exactNumber(style.arrowSize));
    head_->setCurrentIndex(
        head_->findData(QString::fromUtf8(katana::entity::toString(style.arrowHead))));
    scale_->setText(exactNumber(style.unitScale));
    decimals_->setValue(style.decimals);
    round_->setText(exactNumber(style.roundTo));
    trim_->setChecked(style.suppressTrailingZeros);
    paper_->setChecked(style.paperSized);
    loading_ = wasLoading;
    showUnits();
    showPreview();
}

void DimensionStyleManagerDialog::showUnits()
{
    units_->setText(paper_->isChecked()
                        ? QStringLiteral("Sizes are millimetres on paper, drawn at 1:%1 here "
                                         "(the drawing's annotation scale).")
                              .arg(exactNumber(document_.annotationScale()))
                        : QStringLiteral("Sizes are model units: the dimension is drawn at its "
                                         "size on the ground."));
}

void DimensionStyleManagerDialog::showPreview()
{
    if (loading_) {
        return;
    }
    const auto style = formStyle();
    if (!style) {
        preview_->setText(describe(style.error()));
        sample_->setPixmap(QPixmap());
        return;
    }
    if (const auto valid = katana::entity::validate(*style); !valid) {
        preview_->setText(describe(valid.error()));
        sample_->setPixmap(QPixmap());
        return;
    }
    // The formatter DIMSTYLE LIST and INFO read it out with.
    const QString reads =
        QString::fromStdString(katana::entity::formatMeasurement(kSampleLength, *style));
    preview_->setText(QStringLiteral("a dimension of %1 reads as %2")
                          .arg(exactNumber(kSampleLength), reads));
    // The dimension line above its points by two and a half texts: room for
    // the text above the line whatever the sizes.
    const double scale = document_.annotationScale();
    const double textModel = katana::cad::dimensionStyleAtScale(*style, scale).textHeight;
    sample_->setPixmap(drawnDimension(*style, scale, QSize(kSampleWidth, kSampleHeight),
                                      kSampleLength, 2.5 * textModel, QString(), theme::text()));
}

Result<DimensionStyle> DimensionStyleManagerDialog::formStyle() const
{
    DimensionStyle style = loaded_;
    style.name = shown_.toStdString();
    struct Number {
        const QLineEdit* line;
        const char* what;
        double DimensionStyle::* member;
    };
    for (const Number& field : {Number{text_, "the text height", &DimensionStyle::textHeight},
                                Number{gap_, "the gap", &DimensionStyle::textGap},
                                Number{extOff_, "the extension offset",
                                       &DimensionStyle::extensionOffset},
                                Number{extBeyond_, "the extension beyond",
                                       &DimensionStyle::extensionBeyond},
                                Number{arrow_, "the arrow size", &DimensionStyle::arrowSize},
                                Number{scale_, "the unit scale", &DimensionStyle::unitScale},
                                Number{round_, "the rounding", &DimensionStyle::roundTo}}) {
        auto value = numberIn(field.line, field.what);
        if (!value) {
            return value.error();
        }
        style.*field.member = *value;
    }
    style.prefix = prefix_->text().toStdString();
    style.suffix = suffix_->text().toStdString();
    if (auto head =
            katana::entity::arrowHeadFromString(head_->currentData().toString().toStdString())) {
        style.arrowHead = *head;
    }
    style.decimals = decimals_->value();
    style.suppressTrailingZeros = trim_->isChecked();
    style.paperSized = paper_->isChecked();
    return style;
}

Result<QString> DimensionStyleManagerDialog::applyLine() const
{
    const DimensionStyle* stored = document_.model().dimensionStyles.find(shown_.toStdString());
    if (stored == nullptr) {
        return makeError(ErrorCode::NotFound, "choose a dimension style first");
    }
    const auto form = formStyle();
    if (!form) {
        return form.error();
    }
    const auto changes = ann::dimensionStyleChanges(*stored, *form);
    if (!changes) {
        return changes.error();
    }
    if (changes->empty()) {
        return QString();
    }
    const auto name = ann::commandWord(stored->name);
    if (!name) {
        return name.error();
    }
    return QStringLiteral("DIMSTYLE SET %1 %2")
        .arg(QString::fromStdString(*name), QString::fromStdString(*changes));
}

QString DimensionStyleManagerDialog::problem() const
{
    return problem_->text();
}

QString DimensionStyleManagerDialog::preview() const
{
    return preview_->text();
}

bool DimensionStyleManagerDialog::run(const Result<QString>& line)
{
    if (!line) {
        problem_->setText(describe(line.error()));
        return false;
    }
    if (!run_) {
        problem_->setText(QStringLiteral("there is no command line to run the dimension style "
                                         "line on"));
        return false;
    }
    const VerbOutcome outcome = run_(*line);
    problem_->setText(outcome.ok ? QString()
                                 : outcome.error.isEmpty() ? QStringLiteral("refused: ") + *line
                                                           : outcome.error);
    return outcome.ok;
}

bool DimensionStyleManagerDialog::apply()
{
    const auto line = applyLine();
    if (line && line->isEmpty()) {
        problem_->clear();
        return true; // nothing changed: no step
    }
    return run(line);
}

bool DimensionStyleManagerDialog::newStyle()
{
    const QString name = nameForNew(QStringLiteral("Dimension Style"));
    const auto word = ann::commandWord(name.toStdString());
    if (!word) {
        return run(word.error());
    }
    if (!run(QStringLiteral("DIMSTYLE NEW ") + QString::fromStdString(*word))) {
        return false;
    }
    newName_->clear();
    select(name);
    return true;
}

bool DimensionStyleManagerDialog::duplicateStyle()
{
    // What the form shows, edits and all, made a new style in ONE step: NEW
    // with every field that differs from a new style's.
    const QString name = nameForNew(shown_ + QStringLiteral(" copy"));
    const auto form = formStyle();
    if (!form) {
        return run(form.error());
    }
    const auto changes = ann::dimensionStyleChanges(DimensionStyle{}, *form);
    if (!changes) {
        return run(changes.error());
    }
    const auto word = ann::commandWord(name.toStdString());
    if (!word) {
        return run(word.error());
    }
    QString line = QStringLiteral("DIMSTYLE NEW ") + QString::fromStdString(*word);
    if (!changes->empty()) {
        line += QStringLiteral(" ") + QString::fromStdString(*changes);
    }
    if (!run(line)) {
        return false;
    }
    newName_->clear();
    select(name);
    return true;
}

bool DimensionStyleManagerDialog::deleteStyle()
{
    const auto word = ann::commandWord(shown_.toStdString());
    if (!word) {
        return run(word.error());
    }
    if (!run(QStringLiteral("DIMSTYLE DELETE ") + QString::fromStdString(*word))) {
        return false;
    }
    select(QString::fromUtf8(katana::entity::kDefaultDimensionStyleName));
    return true;
}

void DimensionStyleManagerDialog::revert()
{
    problem_->clear();
    select(shown_);
}

} // namespace katana::qt
