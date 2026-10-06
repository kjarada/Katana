#include "definition_editor.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <functional>
#include <initializer_list>
#include <limits>
#include <utility>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QSplitter>
#include <QTextBlock>
#include <QTextCursor>
#include <QTextDocument>
#include <QTextEdit>
#include <QVBoxLayout>

#include "command_word.hpp"
#include "document_watcher.hpp"
#include "katana/cad/definition_edit.hpp"
#include "katana/cad/definition_users.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "style_preview.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::entity::LineStyle;
using katana::entity::StyleUnits;

// The kind box's two rows: the two lists of a customisation file.
constexpr int kLinestyleRow = 0;
constexpr int kSymbolRow = 1;

// The units box's rows. The words are the format's own for "units"
// (docs/customisation.md, "The members"), so the box reads as a file does;
// TheUnitsAreChosenByTheWordsAFileHolds checks each against what the format's
// writer writes for it.
struct UnitsRow {
    StyleUnits units;
    const char* word;
    const char* note;
};
constexpr std::array<UnitsRow, 3> kUnitsRows{{
    {StyleUnits::World, "world", "metres on the ground: its size on a plot follows the plot scale"},
    {StyleUnits::Paper, "paper", "millimetres on the plot: the same size at every scale"},
    {StyleUnits::TwoPoint, "twoPoint", "stretched between the two points it is drawn across"},
}};

// The name a form with no name yet is READ under, for the picture alone: the
// format's reader wants one, and what a definition draws does not depend on
// it. Never saved - a form with no name has no definition - and worded so
// that a refusal citing it ("definition "(not named yet)" strokes[2]: ...")
// still reads.
constexpr std::string_view kStandInName = "(not named yet)";

int unitsRow(StyleUnits units)
{
    for (std::size_t row = 0; row < kUnitsRows.size(); ++row) {
        if (kUnitsRows[row].units == units) {
            return static_cast<int>(row);
        }
    }
    return 0;
}

QString text(std::string_view value)
{
    return QString::fromUtf8(value.data(), static_cast<qsizetype>(value.size()));
}

std::string utf8(const QString& value) { return value.toUtf8().toStdString(); }

QString inQuotes(std::string_view value)
{
    return QLatin1Char('"') + text(value) + QLatin1Char('"');
}

// A refusal as the message area shows it: what was refused, then what it was
// refused of. Not Error::describe(), which leads with the code's name.
QString worded(const Error& error)
{
    QString out = text(error.message);
    if (!error.context.empty()) {
        out += QStringLiteral(" - ") + text(error.context);
    }
    return out;
}

Error refusal(const QString& problem)
{
    return katana::core::makeError(ErrorCode::InvalidArgument, utf8(problem));
}

// A number field: its value, `whenEmpty` for an empty one - a member a file
// leaves out - or nothing, said in `problem`, for text that is not a number.
std::optional<double> numberIn(const QLineEdit& field, double whenEmpty, const QString& label,
                               QString& problem)
{
    const std::string typed = utf8(field.text().trimmed());
    if (typed.empty()) {
        return whenEmpty;
    }
    const std::optional<double> value = katana::core::parseFiniteDouble(typed);
    if (!value) {
        problem = QStringLiteral("%1: \"%2\" is not a number.").arg(label, field.text().trimmed());
    }
    return value;
}

// The text of a box like the strokes box, character for character. Not
// toPlainText(): that hands back a no-break space as a blank and a line
// separator as a line break, and both are characters a stroke's text may hold
// like any other. A form opened on such a text would read as edited for ever,
// and Save would write the blank.
QString rawText(const QTextDocument& document)
{
    QString raw = document.toRawText();
    raw.replace(QChar::ParagraphSeparator, QLatin1Char('\n'));
    return raw;
}

// Whether the strokes box holds `strokes` as given, one stroke a line. It
// breaks a line at more than a line feed - at a paragraph separator, which a
// stroke's text may hold - so the question is asked of a document like its
// own rather than of a list of characters kept here.
bool boxHolds(const QString& strokes)
{
    QTextDocument scratch;
    scratch.setPlainText(strokes);
    return rawText(scratch) == strokes;
}

// What names a definition is drawn as once the library has none of that
// name: a sentence for what names it as a linetype and one for what draws it
// as a symbol. The drawing's own linetype of the name, or the built-in shape
// of that name, answers in its place (cad::DefinitionUsers), and only where
// neither does is a line plain and a point a stand-in mark.
QStringList drawnWithoutIt(const katana::cad::DefinitionUsers& users)
{
    QStringList out;
    if (users.namedAsLinetype()) {
        out << (users.drawingLinetype
                    ? QStringLiteral("A line naming it is then drawn with the drawing's own "
                                     "linetype of that name.")
                    : QStringLiteral("A line naming it is then drawn plain."));
    }
    if (users.namedAsSymbol()) {
        out << (users.builtInShape
                    ? QStringLiteral("A point naming it is then drawn as the built-in shape of "
                                     "that name.")
                    : QStringLiteral("A point naming it is then drawn as a stand-in mark."));
    }
    return out;
}

// The same of a definition that is NOT in the library - being made, or
// deleted and still in the form: what its name draws until it is saved, and
// what saving it takes over. `atVertices` is the form's: the resolver draws a
// definition along a line only when it is not at vertices
// (cad/style_resolver.hpp, decision D2).
QStringList drawnUntilSaved(const katana::cad::DefinitionUsers& users, bool atVertices,
                            const QString& name)
{
    QStringList out;
    if (users.drawingLinetype) {
        out << (atVertices
                    ? QStringLiteral("The drawing has a linetype %1 of its own, which goes on "
                                     "drawing the lines that name it: a definition at vertices "
                                     "is not drawn along a line.")
                          .arg(name)
                    : QStringLiteral("The drawing has a linetype %1 of its own, which draws the "
                                     "lines that name it now. Saved, this definition is drawn in "
                                     "its place, and Styles and Linetypes lists the name under "
                                     "Diagnostics as one in both.")
                          .arg(name));
    } else if (users.namedAsLinetype()) {
        out << (atVertices ? QStringLiteral("A line naming it is drawn plain, and stays so: a "
                                            "definition at vertices is not drawn along a line.")
                           : QStringLiteral("Until it is saved a line naming it is drawn plain."));
    }
    if (users.builtInShape) {
        out << QStringLiteral("%1 is a built-in shape, which draws the points that name it now. "
                              "Saved, this definition is drawn in its place.")
                   .arg(name);
    } else if (users.namedAsSymbol()) {
        out << QStringLiteral("Until it is saved a point naming it is drawn as a stand-in mark.");
    }
    return out;
}

QString counted(std::size_t count, const char* one, const char* many)
{
    return QStringLiteral("%1 %2").arg(count).arg(QString::fromLatin1(count == 1 ? one : many));
}

QString kindWord(bool symbol)
{
    return symbol ? QStringLiteral("symbol") : QStringLiteral("linestyle");
}

// A user a line, set in under the sentence that introduces them.
QString listed(const std::vector<std::string>& each)
{
    QStringList out;
    for (const std::string& line : each) {
        out << QStringLiteral("  ") + text(line);
    }
    return out.join(QLatin1Char('\n'));
}

// True when `shown` holds every one of `lines`, and there is one to hold:
// whether the reply on the page already cites each user of a definition as
// CUSTOMISE REMOVE's refusal cites it (cad::DefinitionUsers::cited). Not a
// reading of the reply - the lines are the verb's own, made by the function
// the verb makes them with.
bool citesEvery(const QString& shown, const std::vector<std::string>& lines)
{
    if (lines.empty()) {
        return false;
    }
    for (const std::string& line : lines) {
        if (!shown.contains(text(line))) {
            return false;
        }
    }
    return true;
}

} // namespace

DefinitionEditorDialog::DefinitionEditorDialog(const CustomisationContext& context,
                                               QWidget* parent)
    : QDialog(parent), context_(context)
{
    setObjectName(QStringLiteral("definitionEditorDialog"));
    setWindowTitle(QStringLiteral("Definition Editor"));
    setModal(false);
    buildUi();
    // The theme draws a field that cannot be typed into exactly as one that
    // can - it has a rule for a disabled button and none for a disabled field
    // - and here that difference is how a fixed name, and the anchors of a
    // definition that is not two-point, are told from the rest.
    setStyleSheet(QStringLiteral("QLineEdit:disabled, QComboBox:disabled, "
                                 "QAbstractSpinBox:disabled, QPlainTextEdit:disabled "
                                 "{ color: %1; }")
                      .arg(theme::textDisabled().name()));
    // The context's contract is that `document` is set
    // (customisation_context.hpp), and the previews above are built on it: it
    // is not asked for here as if it might not be. What can happen is that
    // the Document dies FIRST, which is what the watcher is asked about.
    watcher_ = std::make_unique<DocumentWatcher>(
        *context_.document,
        [this](const DocumentChanges& changes) { onDocumentChanged(changes); });
    load(fieldsOf(LineStyle{}), std::nullopt, {}, {});
}

DefinitionEditorDialog::~DefinitionEditorDialog()
{
    // The children outlive this body, and one dying can still signal - the
    // strokes box as its document goes. Cut every connection into this dialog
    // first, so none reaches a lambda whose members no longer exist.
    watcher_.reset();
    for (QObject* child : findChildren<QObject*>()) {
        QObject::disconnect(child, nullptr, this, nullptr);
    }
}

bool DefinitionEditorDialog::alive() const
{
    return watcher_ != nullptr && watcher_->documentAlive();
}

void DefinitionEditorDialog::log(const QString& message, bool isError) const
{
    if (context_.log) {
        context_.log(message, isError);
    }
}

// ---- layout ------------------------------------------------------------------------------------

void DefinitionEditorDialog::buildUi()
{
    auto* outer = new QVBoxLayout(this);

    heading_ = new QLabel(this);
    heading_->setObjectName(QStringLiteral("definitionHeading"));
    QFont bold = heading_->font();
    bold.setBold(true);
    if (bold.pointSizeF() > 0.0) {
        bold.setPointSizeF(bold.pointSizeF() * 1.15);
    }
    heading_->setFont(bold);
    outer->addWidget(heading_);

    auto* splitter = new QSplitter(Qt::Horizontal, this);
    splitter->setObjectName(QStringLiteral("definitionSplitter"));
    splitter->setChildrenCollapsible(false);

    // Left: the form, and the strokes under it.
    auto* left = new QWidget(splitter);
    auto* leftLayout = new QVBoxLayout(left);
    leftLayout->setContentsMargins(0, 0, 0, 0);
    auto* form = new QFormLayout;
    form->setFieldGrowthPolicy(QFormLayout::AllNonFixedFieldsGrow);

    const auto line = [&](QLineEdit*& field, const char* objectName, const QString& tip,
                          const QString& placeholder = {}) {
        field = new QLineEdit(left);
        field->setObjectName(QString::fromLatin1(objectName));
        field->setToolTip(tip);
        field->setPlaceholderText(placeholder);
        return field;
    };
    // Two fields on one row, for a point.
    const auto pair = [&](QLineEdit* x, QLineEdit* y) {
        auto* row = new QWidget(left);
        auto* rowLayout = new QHBoxLayout(row);
        rowLayout->setContentsMargins(0, 0, 0, 0);
        rowLayout->addWidget(x);
        rowLayout->addWidget(y);
        return row;
    };

    line(name_, "definitionName",
         QStringLiteral("What styles and survey codes name it by. Fixed once the definition "
                        "exists: another name would be another definition, and everything "
                        "naming this one left naming nothing. Duplicate makes a copy under a "
                        "new name."));
    form->addRow(QStringLiteral("Name"), name_);
    line(group_, "definitionGroup",
         QStringLiteral("Where a browser lists it: a path with / between its parts, such as "
                        "Survey/Water"));
    form->addRow(QStringLiteral("Group"), group_);

    kind_ = new QComboBox(left);
    kind_->setObjectName(QStringLiteral("definitionKind"));
    kind_->addItem(QStringLiteral("Linestyle"));
    kind_->addItem(QStringLiteral("Symbol"));
    kind_->setToolTip(QStringLiteral(
        "Which of a customisation's two lists holds it. A symbol is offered wherever a "
        "symbol is chosen; a linestyle that is not at vertices, wherever a linetype is."));
    form->addRow(QStringLiteral("Kind"), kind_);

    auto* unitsRowWidget = new QWidget(left);
    auto* unitsLayout = new QHBoxLayout(unitsRowWidget);
    unitsLayout->setContentsMargins(0, 0, 0, 0);
    units_ = new QComboBox(unitsRowWidget);
    units_->setObjectName(QStringLiteral("definitionUnits"));
    for (const UnitsRow& row : kUnitsRows) {
        units_->addItem(QString::fromLatin1(row.word));
    }
    units_->setToolTip(QStringLiteral("What the strokes' coordinates are measured in"));
    unitsNote_ = new QLabel(unitsRowWidget);
    unitsNote_->setObjectName(QStringLiteral("definitionUnitsNote"));
    unitsLayout->addWidget(units_);
    unitsLayout->addWidget(unitsNote_, 1);
    form->addRow(QStringLiteral("Units"), unitsRowWidget);

    atVertices_ = new QCheckBox(QStringLiteral("At vertices"), left);
    atVertices_->setObjectName(QStringLiteral("definitionAtVertices"));
    atVertices_->setToolTip(QStringLiteral(
        "Drawn at each vertex of a string rather than along it; a line whose linetype names "
        "it is then drawn plain"));
    form->addRow(QString(), atVertices_);

    line(length_, "definitionLength",
         QStringLiteral("One repeat of the pattern along a line, in the definition's units. "
                        "Left empty, or 0, the span of the strokes is used."),
         QStringLiteral("0"));
    form->addRow(QStringLiteral("Length"), length_);
    line(factor_, "definitionFactor",
         QStringLiteral("A scale applied to every coordinate; above 0"), QStringLiteral("1"));
    form->addRow(QStringLiteral("Factor"), factor_);
    const QString originTip =
        QStringLiteral("The point of the strokes' coordinates that is put on the point drawn");
    line(originX_, "definitionOriginX", originTip, QStringLiteral("0"));
    line(originY_, "definitionOriginY", originTip, QStringLiteral("0"));
    form->addRow(QStringLiteral("Origin x, y"), pair(originX_, originY_));
    const QString anchorTip = QStringLiteral(
        "For the units twoPoint only: where the two points it is drawn across sit in the "
        "strokes' coordinates");
    line(anchor1X_, "definitionAnchor1X", anchorTip, QStringLiteral("0"));
    line(anchor1Y_, "definitionAnchor1Y", anchorTip, QStringLiteral("0"));
    form->addRow(QStringLiteral("Anchor 1 x, y"), pair(anchor1X_, anchor1Y_));
    line(anchor2X_, "definitionAnchor2X", anchorTip, QStringLiteral("0"));
    line(anchor2Y_, "definitionAnchor2Y", anchorTip, QStringLiteral("0"));
    form->addRow(QStringLiteral("Anchor 2 x, y"), pair(anchor2X_, anchor2Y_));

    const auto whole = [&](QSpinBox*& box, const char* objectName) {
        box = new QSpinBox(left);
        box->setObjectName(QString::fromLatin1(objectName));
        box->setRange(std::numeric_limits<int>::min(), std::numeric_limits<int>::max());
        box->setToolTip(QStringLiteral(
            "For the units twoPoint only: a whole number kept as the customisation gives it"));
        return box;
    };
    auto* modes = new QWidget(left);
    auto* modesLayout = new QHBoxLayout(modes);
    modesLayout->setContentsMargins(0, 0, 0, 0);
    modesLayout->addWidget(whole(stretchMode_, "definitionStretchMode"));
    modesLayout->addWidget(whole(cycleMode_, "definitionCycleMode"));
    form->addRow(QStringLiteral("Stretch, cycle mode"), modes);

    sourceLabel_ = new QLabel(left);
    sourceLabel_->setObjectName(QStringLiteral("definitionSource"));
    sourceLabel_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    form->addRow(QStringLiteral("From"), sourceLabel_);
    leftLayout->addLayout(form);

    auto* help = new QLabel(
        QStringLiteral(
            "Strokes, one a line, as a customisation file holds them:  [\"move\", x, y]   "
            "[\"draw\", x, y]   [\"arc\", radius, start, end]   [\"circle\", radius]   "
            "[\"dot\", radius]   [\"pen\", \"colour name\"]   "
            "[\"text\", {\"text\": \"W\", \"height\": 1.5}].  Coordinates and radii are in the "
            "units above and angles in degrees, counter-clockwise; an arc, a circle, a dot "
            "and a text sit at the point the pen is at."),
        left);
    help->setObjectName(QStringLiteral("definitionStrokesHelp"));
    help->setWordWrap(true);
    leftLayout->addWidget(help);
    strokes_ = new QPlainTextEdit(left);
    strokes_->setObjectName(QStringLiteral("definitionStrokes"));
    strokes_->setLineWrapMode(QPlainTextEdit::NoWrap);
    strokes_->setTabChangesFocus(true);
    strokes_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    strokes_->setPlaceholderText(QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 1, 0]"));
    leftLayout->addWidget(strokes_, 1);

    // Right: what it draws.
    auto* right = new QWidget(splitter);
    auto* rightLayout = new QVBoxLayout(right);
    rightLayout->setContentsMargins(0, 0, 0, 0);
    auto* symbolBox = new QGroupBox(QStringLiteral("As a symbol, on its insertion point"), right);
    auto* symbolLayout = new QVBoxLayout(symbolBox);
    preview_ = new StylePreview(*context_.document, symbolBox);
    preview_->setObjectName(QStringLiteral("definitionPreview"));
    preview_->setMinimumSize(260, 170);
    symbolLayout->addWidget(preview_);
    auto* lineBox = new QGroupBox(QStringLiteral("Along a line"), right);
    lineBox_ = lineBox;
    auto* lineLayout = new QVBoxLayout(lineBox);
    linePreview_ = new StylePreview(*context_.document, lineBox);
    linePreview_->setObjectName(QStringLiteral("definitionLinePreview"));
    linePreview_->setMinimumSize(260, 130);
    lineLayout->addWidget(linePreview_);
    auto* scaleRow = new QHBoxLayout;
    auto* scaleLabel = new QLabel(QStringLiteral("Plot scale"), right);
    plotScale_ = new QComboBox(right);
    plotScale_->setObjectName(QStringLiteral("definitionPlotScale"));
    for (const int denominator : StylePreview::kPlotScales) {
        plotScale_->addItem(QStringLiteral("1:%1").arg(denominator), denominator);
    }
    plotScale_->setCurrentIndex(plotScale_->findData(preview_->scaleDenominator()));
    scaleLabel->setBuddy(plotScale_);
    scaleRow->addWidget(scaleLabel);
    scaleRow->addWidget(plotScale_);
    scaleRow->addStretch(1);
    // Said while the text does not read and the last picture that did stays
    // up. One line, its height kept whether or not it says anything, so the
    // panes do not move as it comes and goes.
    previewNote_ = new QLabel(right);
    previewNote_->setObjectName(QStringLiteral("definitionPreviewNote"));
    previewNote_->setFixedHeight(previewNote_->fontMetrics().lineSpacing() + 2);
    previewNote_->setStyleSheet(QStringLiteral("color: %1;").arg(theme::error().name()));
    rightLayout->addWidget(symbolBox, 3);
    rightLayout->addWidget(lineBox, 2);
    rightLayout->addWidget(previewNote_);
    rightLayout->addLayout(scaleRow);

    splitter->addWidget(left);
    splitter->addWidget(right);
    splitter->setStretchFactor(0, 3);
    splitter->setStretchFactor(1, 2);
    outer->addWidget(splitter, 1);

    // Below: what is wrong, or what names it; then the buttons.
    issues_ = new QPlainTextEdit(this);
    issues_->setObjectName(QStringLiteral("definitionIssues"));
    issues_->setReadOnly(true);
    issues_->setTabChangesFocus(true);
    issues_->setFixedHeight(104);
    outer->addWidget(issues_);

    removePanel_ = new QWidget(this);
    removePanel_->setObjectName(QStringLiteral("definitionDeletePanel"));
    auto* removeLayout = new QHBoxLayout(removePanel_);
    removeLayout->setContentsMargins(0, 0, 0, 0);
    removeAnyway_ = new QPushButton(QStringLiteral("Delete Anyway"), removePanel_);
    removeAnyway_->setObjectName(QStringLiteral("definitionDeleteAnyway"));
    removeAnyway_->setToolTip(QStringLiteral(
        "Delete it although it is used: what names it then names nothing"));
    removeCancel_ = new QPushButton(QStringLiteral("Keep It"), removePanel_);
    removeCancel_->setObjectName(QStringLiteral("definitionDeleteCancel"));
    removeLayout->addWidget(removeAnyway_);
    removeLayout->addWidget(removeCancel_);
    removeLayout->addStretch(1);
    outer->addWidget(removePanel_);

    auto* buttons = new QHBoxLayout;
    const auto button = [&](const QString& label, const char* objectName, const QString& tip) {
        auto* made = new QPushButton(label, this);
        made->setObjectName(QString::fromLatin1(objectName));
        made->setToolTip(tip);
        return made;
    };
    new_ = button(QStringLiteral("New"), "definitionNew",
                  QStringLiteral("An empty definition of the kind shown"));
    duplicate_ = button(QStringLiteral("Duplicate"), "definitionDuplicate",
                        QStringLiteral("A copy of this definition under a new name"));
    delete_ = button(QStringLiteral("Delete"), "definitionDelete",
                     QStringLiteral("Take it out of the library. Refused, listing what uses "
                                    "it, while a survey code, a style or a layer names it"));
    revert_ = button(QStringLiteral("Revert"), "definitionRevert",
                     QStringLiteral("Forget what was changed here"));
    save_ = button(QStringLiteral("Save"), "definitionSave",
                   QStringLiteral("Put the definition into the session's library, in place of "
                                  "the one of its name"));
    auto* close = button(QStringLiteral("Close"), "definitionClose", {});
    for (QPushButton* each : {new_, duplicate_, delete_}) {
        buttons->addWidget(each);
    }
    buttons->addStretch(1);
    for (QPushButton* each : {revert_, save_, close}) {
        buttons->addWidget(each);
    }
    outer->addLayout(buttons);
    resize(1000, 720);

    // ---- wiring: lambdas, no moc. Nothing but a button's clicked commits.
    for (QLineEdit* field : {name_, group_, length_, factor_, originX_, originY_, anchor1X_,
                             anchor1Y_, anchor2X_, anchor2Y_}) {
        // textChanged, not textEdited: a field filled by the headless driver
        // or a test has been edited as much as one typed into.
        QObject::connect(field, &QLineEdit::textChanged, this, [this] { edited(); });
    }
    QObject::connect(kind_, &QComboBox::currentIndexChanged, this, [this] { edited(); });
    QObject::connect(units_, &QComboBox::currentIndexChanged, this, [this] { edited(); });
    QObject::connect(atVertices_, &QCheckBox::toggled, this, [this] { edited(); });
    QObject::connect(stretchMode_, &QSpinBox::valueChanged, this, [this] { edited(); });
    QObject::connect(cycleMode_, &QSpinBox::valueChanged, this, [this] { edited(); });
    QObject::connect(strokes_, &QPlainTextEdit::textChanged, this, [this] { edited(); });
    QObject::connect(plotScale_, &QComboBox::currentIndexChanged, this, [this] {
        const int denominator = plotScale_->currentData().toInt();
        preview_->setScaleDenominator(denominator);
        linePreview_->setScaleDenominator(denominator);
    });
    QObject::connect(new_, &QPushButton::clicked, this,
                     [this] { newDefinition(kind_->currentIndex() == kSymbolRow); });
    QObject::connect(duplicate_, &QPushButton::clicked, this, [this] {
        if (existing_) {
            duplicateDefinition(std::string(*existing_));
        }
    });
    QObject::connect(delete_, &QPushButton::clicked, this, [this] { remove(false); });
    QObject::connect(removeAnyway_, &QPushButton::clicked, this, [this] { remove(true); });
    QObject::connect(removeCancel_, &QPushButton::clicked, this, [this] {
        removeRefused_ = false;
        said_.clear();
        refresh();
    });
    QObject::connect(revert_, &QPushButton::clicked, this, [this] { revert(); });
    QObject::connect(save_, &QPushButton::clicked, this, [this] { save(); });
    QObject::connect(close, &QPushButton::clicked, this, [this] { this->close(); });

    // No button is a default: QDialog makes the first auto-default button its
    // default when shown, and Enter in any field - a name, a length - would
    // press it. That button is New, which would empty the form.
    for (QPushButton* each : findChildren<QPushButton*>()) {
        each->setAutoDefault(false);
        each->setDefault(false);
    }
}

// ---- the form ----------------------------------------------------------------------------------

DefinitionEditorDialog::Form DefinitionEditorDialog::fieldsOf(const LineStyle& definition)
{
    Form form;
    form.name = text(definition.name);
    form.group = text(definition.group);
    form.symbol = definition.symbol;
    form.units = unitsRow(definition.units);
    form.atVertices = definition.atVertices;
    form.length = exactNumber(definition.length);
    form.factor = exactNumber(definition.factor);
    form.originX = exactNumber(definition.origin.x);
    form.originY = exactNumber(definition.origin.y);
    form.anchor1X = exactNumber(definition.anchor1.x);
    form.anchor1Y = exactNumber(definition.anchor1.y);
    form.anchor2X = exactNumber(definition.anchor2.x);
    form.anchor2Y = exactNumber(definition.anchor2.y);
    form.stretchMode = definition.stretchMode;
    form.cycleMode = definition.cycleMode;
    return form;
}

Result<DefinitionEditorDialog::Form> DefinitionEditorDialog::formOf(const LineStyle& definition)
{
    const Result<std::string> strokes = katana::cad::strokeText(definition);
    if (!strokes) {
        return katana::core::makeError(
            strokes.error().code,
            utf8(QStringLiteral("the customisation format cannot write it: %1")
                     .arg(worded(strokes.error()))));
    }
    Form form = fieldsOf(definition);
    form.strokes = text(*strokes);
    if (!boxHolds(form.strokes)) {
        return katana::core::makeError(
            ErrorCode::Unsupported,
            "one of its texts holds a paragraph separator, which the strokes box breaks a line "
            "at: its strokes could not be shown one a line");
    }
    return form;
}

DefinitionEditorDialog::Form DefinitionEditorDialog::shownForm() const
{
    Form form;
    form.name = name_->text();
    form.group = group_->text();
    form.symbol = kind_->currentIndex() == kSymbolRow;
    form.units = units_->currentIndex();
    form.atVertices = atVertices_->isChecked();
    form.length = length_->text();
    form.factor = factor_->text();
    form.originX = originX_->text();
    form.originY = originY_->text();
    form.anchor1X = anchor1X_->text();
    form.anchor1Y = anchor1Y_->text();
    form.anchor2X = anchor2X_->text();
    form.anchor2Y = anchor2Y_->text();
    form.stretchMode = stretchMode_->value();
    form.cycleMode = cycleMode_->value();
    form.strokes = strokesShown();
    return form;
}

QString DefinitionEditorDialog::strokesShown() const { return rawText(*strokes_->document()); }

void DefinitionEditorDialog::showForm(const Form& form)
{
    // The fields' own signals are not a person's doing here.
    const bool wasLoading = loading_;
    loading_ = true;
    name_->setText(form.name);
    group_->setText(form.group);
    kind_->setCurrentIndex(form.symbol ? kSymbolRow : kLinestyleRow);
    units_->setCurrentIndex(form.units);
    atVertices_->setChecked(form.atVertices);
    length_->setText(form.length);
    factor_->setText(form.factor);
    originX_->setText(form.originX);
    originY_->setText(form.originY);
    anchor1X_->setText(form.anchor1X);
    anchor1Y_->setText(form.anchor1Y);
    anchor2X_->setText(form.anchor2X);
    anchor2Y_->setText(form.anchor2Y);
    stretchMode_->setValue(form.stretchMode);
    cycleMode_->setValue(form.cycleMode);
    strokes_->setPlainText(form.strokes);
    loading_ = wasLoading;
}

bool DefinitionEditorDialog::dirty() const { return !(shownForm() == baseline_); }

DefinitionEditorDialog::Reading DefinitionEditorDialog::read() const
{
    Reading reading;
    // Everything wrong, the first of them as the error: a name that is
    // missing does not stop the strokes being read, so a new definition is
    // pictured, and a line of it that does not read is washed, before it has
    // a name.
    QStringList problems;
    const auto wrong = [&](const QString& why, const Error& error) {
        if (problems.isEmpty()) {
            reading.error = error;
        }
        problems << why;
    };

    LineStyle members;
    // A blank at an end of a NEW name is taken off: it cannot be seen where
    // the name is shown, and a rule naming "Valve" would miss "Valve ". The
    // name of a definition that exists is its own, whatever it holds.
    const std::string name = existing_ ? *existing_ : utf8(name_->text().trimmed());
    if (name.empty()) {
        const QString why = QStringLiteral("Type a name: it is what a style or a survey code "
                                           "names the definition by.");
        wrong(why, refusal(why));
    } else if (!existing_) {
        // Nor a name no command line could say: saved, the definition could
        // never be removed again (cad::removeDefinitionLine says which names
        // those are). One that already exists under such a name - out of a
        // file - is still opened and changed.
        if (const Result<std::string> line = katana::cad::removeDefinitionLine(name, false);
            !line) {
            wrong(QStringLiteral("A new definition cannot be named %1: %2.")
                      .arg(inQuotes(name), text(line.error().message)),
                  line.error());
        }
    }
    members.group = utf8(group_->text());
    members.symbol = kind_->currentIndex() == kSymbolRow;
    members.units =
        kUnitsRows[static_cast<std::size_t>(std::max(0, units_->currentIndex()))].units;
    members.atVertices = atVertices_->isChecked();
    // Read whether or not their fields can be typed into: a definition that
    // is not two-point may still carry anchors, and an edit of its length
    // must not drop them.
    QString problem;
    const LineStyle fresh;
    const auto number = [&](const QLineEdit* field, double whenEmpty, const char* label) {
        return problem.isEmpty()
                   ? numberIn(*field, whenEmpty, QString::fromLatin1(label), problem)
                   : std::optional<double>();
    };
    const std::optional<double> length = number(length_, fresh.length, "Length");
    const std::optional<double> factor = number(factor_, fresh.factor, "Factor");
    const std::optional<double> originX = number(originX_, 0.0, "Origin x");
    const std::optional<double> originY = number(originY_, 0.0, "Origin y");
    const std::optional<double> anchor1X = number(anchor1X_, 0.0, "Anchor 1 x");
    const std::optional<double> anchor1Y = number(anchor1Y_, 0.0, "Anchor 1 y");
    const std::optional<double> anchor2X = number(anchor2X_, 0.0, "Anchor 2 x");
    const std::optional<double> anchor2Y = number(anchor2Y_, 0.0, "Anchor 2 y");
    if (!problem.isEmpty()) {
        // With a number that is none there are no members to read strokes
        // under.
        wrong(problem, refusal(problem));
        reading.problem = problems.join(QLatin1Char('\n'));
        return reading;
    }
    members.length = *length;
    members.factor = *factor;
    members.origin = {*originX, *originY};
    members.anchor1 = {*anchor1X, *anchor1Y};
    members.anchor2 = {*anchor2X, *anchor2Y};
    members.stretchMode = stretchMode_->value();
    members.cycleMode = cycleMode_->value();
    members.source = source_;
    members.name = name.empty() ? std::string(kStandInName) : name;

    // The strokes as typed, read under those members by the format's reader
    // (cad::readStrokeText) - which is also where entity::validate refuses a
    // length below 0 or a factor of 0.
    katana::cad::StrokeTextRead strokes =
        katana::cad::readStrokeText(members, utf8(strokesShown()));
    if (strokes.definition) {
        reading.drawable = std::move(*strokes.definition);
    } else {
        reading.line = strokes.line;
        wrong(text(strokes.problem), strokes.error);
    }
    if (problems.isEmpty()) {
        reading.definition = reading.drawable;
    } else {
        reading.problem = problems.join(QLatin1Char('\n'));
    }
    return reading;
}

Result<LineStyle> DefinitionEditorDialog::definition() const
{
    Reading reading = read();
    if (reading.definition) {
        return std::move(*reading.definition);
    }
    return reading.error;
}

// ---- opening -----------------------------------------------------------------------------------

void DefinitionEditorDialog::say(const QString& message, bool isError)
{
    said_ = message;
    saidIsError_ = isError;
    refresh();
}

bool DefinitionEditorDialog::mayReplace(const QString& what)
{
    if (!alive()) {
        refresh();
        return false;
    }
    if (!dirty()) {
        return true;
    }
    const QString held =
        existing_ ? inQuotes(*existing_)
                  : QStringLiteral("a new %1").arg(kindWord(kind_->currentIndex() == kSymbolRow));
    const QString message =
        QStringLiteral("%1: the definition editor holds edits to %2 that are not saved. Save or "
                       "Revert them first.")
            .arg(what, held);
    log(message, true);
    say(message, true);
    return false;
}

void DefinitionEditorDialog::load(const Form& form, std::optional<std::string> existing,
                                  std::string source, const QString& origin)
{
    existing_ = std::move(existing);
    source_ = std::move(source);
    origin_ = origin;
    baseline_ = form;
    note_.clear();
    said_.clear();
    removeRefused_ = false;
    discardAgreed_ = false;
    seen_.reset();
    if (existing_ && alive()) {
        if (const LineStyle* found = context_.document->styleLibrary().find(*existing_)) {
            seen_ = *found;
        }
    }
    // Not the last definition's picture over a form that does not read yet.
    preview_->clear();
    linePreview_->clear();
    pictured_ = false;
    showForm(form);
    refresh();
}

bool DefinitionEditorDialog::newDefinition(bool symbol)
{
    if (!mayReplace(QStringLiteral("New"))) {
        return false;
    }
    Form form = fieldsOf(LineStyle{});
    form.symbol = symbol;
    load(form, std::nullopt, {}, {});
    name_->setFocus();
    return true;
}

bool DefinitionEditorDialog::editDefinition(std::string_view name)
{
    // Already on it: what is typed stays.
    if (existing_ && *existing_ == name && alive()) {
        return true;
    }
    if (!mayReplace(QStringLiteral("Edit %1").arg(inQuotes(name)))) {
        return false;
    }
    const LineStyle* found = context_.document->styleLibrary().find(name);
    if (found == nullptr) {
        const QString message =
            QStringLiteral("Edit: the library has no definition %1.").arg(inQuotes(name));
        log(message, true);
        say(message, true);
        return false;
    }
    const Result<Form> form = formOf(*found);
    if (!form) {
        const QString message = QStringLiteral("%1 cannot be edited here, because %2")
                                    .arg(inQuotes(name), text(form.error().message));
        log(message, true);
        say(message, true);
        return false;
    }
    load(*form, std::string(name), found->source, {});
    return true;
}

bool DefinitionEditorDialog::duplicateDefinition(std::string_view name)
{
    if (!mayReplace(QStringLiteral("Duplicate %1").arg(inQuotes(name)))) {
        return false;
    }
    const LineStyle* found = context_.document->styleLibrary().find(name);
    if (found == nullptr) {
        const QString message =
            QStringLiteral("Duplicate: the library has no definition %1.").arg(inQuotes(name));
        log(message, true);
        say(message, true);
        return false;
    }
    // Equal but for its name - where it came from included: its strokes did.
    LineStyle copy = *found;
    // The first name free in the library AND in the drawing's own linetypes,
    // so the copy does not begin over either (cad::freeDefinitionName).
    copy.name = katana::cad::freeDefinitionName(*context_.document, name);
    const Result<Form> form = formOf(copy);
    if (!form) {
        const QString message = QStringLiteral("%1 cannot be copied here, because %2")
                                    .arg(inQuotes(name), text(form.error().message));
        log(message, true);
        say(message, true);
        return false;
    }
    load(*form, std::nullopt, copy.source, QStringLiteral("a copy of %1").arg(inQuotes(name)));
    name_->setFocus();
    name_->selectAll();
    return true;
}

bool DefinitionEditorDialog::deleteDefinition(std::string_view name)
{
    // Asked here, so the refusal over unsaved edits says what was asked for.
    const bool onIt = existing_ && *existing_ == name;
    if (!onIt && !mayReplace(QStringLiteral("Delete %1").arg(inQuotes(name)))) {
        return false;
    }
    return editDefinition(name) && remove(false);
}

bool DefinitionEditorDialog::request(DefinitionEdit what, const std::string& name)
{
    switch (what) {
    case DefinitionEdit::NewSymbol:
        return newDefinition(true);
    case DefinitionEdit::NewLinestyle:
        return newDefinition(false);
    case DefinitionEdit::Edit:
        return editDefinition(name);
    case DefinitionEdit::Duplicate:
        return duplicateDefinition(name);
    case DefinitionEdit::Delete:
        return deleteDefinition(name);
    }
    return false;
}

// ---- committing --------------------------------------------------------------------------------

bool DefinitionEditorDialog::save()
{
    if (!alive()) {
        refresh();
        return false;
    }
    Reading reading = read();
    katana::cad::Document& document = *context_.document;
    if (!reading.definition ||
        (!existing_ && document.styleLibrary().contains(reading.definition->name))) {
        refresh(); // the message area says which
        return false;
    }
    const LineStyle saved = std::move(*reading.definition);
    // A COPY of the library with the definition in it, installed whole: the
    // library has no command and no undo (Document::styleLibrary), and this
    // is how the Survey Code Manager's Apply commits its map.
    katana::entity::StyleLibrary library = document.styleLibrary();
    const Result<bool> replaced = katana::entity::addOrReplace(library, saved);
    if (!replaced) {
        say(worded(replaced.error()), true);
        return false;
    }
    const std::function<void()> committed =
        context_.beginCommit ? context_.beginCommit() : std::function<void()>();
    document.setStyleLibrary(std::move(library));
    if (committed) {
        committed();
    }

    existing_ = saved.name;
    seen_ = saved;
    origin_.clear();
    note_.clear();
    removeRefused_ = false;
    discardAgreed_ = false;
    // A new name was trimmed on the way in; the field shows what was saved.
    loading_ = true;
    name_->setText(text(saved.name));
    loading_ = false;
    baseline_ = shownForm();
    const QString done =
        QStringLiteral("%1 %2 %3: %4, %5.")
            .arg(saved.symbol ? QStringLiteral("Symbol") : QStringLiteral("Linestyle"),
                 inQuotes(saved.name),
                 *replaced ? QStringLiteral("saved") : QStringLiteral("added to the library"),
                 counted(saved.strokes.size(), "stroke", "strokes"),
                 counted(saved.texts.size(), "text", "texts"));
    log(done, false);
    say(done, false);
    return true;
}

void DefinitionEditorDialog::revert()
{
    said_.clear();
    removeRefused_ = false;
    discardAgreed_ = false;
    // "Changed elsewhere" is answered by showing what the library has; a
    // definition that is no longer there is still not there.
    if (seen_ || !existing_) {
        note_.clear();
    }
    // The picture of what was typed goes with it: what the form goes back to
    // may not read at all.
    preview_->clear();
    linePreview_->clear();
    pictured_ = false;
    showForm(baseline_);
    refresh();
}

bool DefinitionEditorDialog::deleteAnywayOffered() const
{
    return alive() && removeRefused_ && existing_ &&
           context_.document->styleLibrary().contains(*existing_);
}

bool DefinitionEditorDialog::remove(bool anyway)
{
    if (!alive()) {
        refresh();
        return false;
    }
    if (!existing_ || !context_.document->styleLibrary().contains(*existing_)) {
        say(QStringLiteral("Delete: the form holds no definition that is in the library."), true);
        return false;
    }
    // FORCE answers a refusal that is on the page with what uses the
    // definition listed under it. Asked for with none showing - the button
    // pressed while the page was not offering it, by a script or a caller -
    // it is refused: nobody has been shown what it would pass over, and the
    // library has no undo.
    if (anyway && !removeRefused_) {
        say(QStringLiteral("Delete Anyway is offered once Delete has been refused, under the "
                           "list of what uses the definition: press Delete first."),
            true);
        return false;
    }
    const std::string name = *existing_;
    // The line, or why no line can name this definition: a double quote in
    // its name, or a name that is one of the line's own words
    // (cad::removeDefinitionLine).
    const Result<std::string> line = katana::cad::removeDefinitionLine(name, anyway);
    if (!line) {
        say(QStringLiteral("Delete: no command line can name %1 - %2.")
                .arg(inQuotes(name), text(line.error().message)),
            true);
        return false;
    }
    if (!context_.run) {
        say(QStringLiteral("Nothing here can run CUSTOMISE REMOVE: the editor has no command "
                           "line."),
            true);
        return false;
    }
    const std::function<void()> committed =
        context_.beginCommit ? context_.beginCommit() : std::function<void()>();
    const VerbOutcome outcome = context_.run(text(*line));
    if (!alive()) {
        return false;
    }
    if (!outcome.ok) {
        // What the line answered, as it answered it: the editor does not
        // guess why a line failed. Delete Anyway is offered when it was the
        // PLAIN line that failed and something names the definition - which
        // is what the verb refuses for, by the function it asks - and the
        // list goes under the verb's own words, so a line that failed for
        // another reason is read as that. A FORCE that failed has nothing
        // further to offer.
        removeRefused_ =
            !anyway && !katana::cad::definitionUsers(*context_.document, name).empty();
        say(QStringLiteral("%1 was not deleted: %2")
                .arg(inQuotes(name), outcome.error.isEmpty()
                                         ? QStringLiteral("the command was refused.")
                                         : outcome.error),
            true);
        return false;
    }
    if (committed) {
        committed();
    }
    removeRefused_ = false;
    discardAgreed_ = false;
    seen_.reset();
    note_ = QStringLiteral("%1 was deleted from the library. It is still shown here: Save puts "
                           "it back.")
                .arg(inQuotes(name));
    said_.clear();
    refresh();
    return true;
}

void DefinitionEditorDialog::reject()
{
    if (alive() && dirty()) {
        log(QStringLiteral("The definition editor was closed with edits that are not saved; "
                           "they are kept in it."),
            false);
    }
    QDialog::reject();
}

// ---- following the form and the Document -------------------------------------------------------

void DefinitionEditorDialog::edited()
{
    if (loading_) {
        return;
    }
    // What the editor last said - and what a person agreed might be
    // discarded - was about the form as it then was.
    said_.clear();
    removeRefused_ = false;
    discardAgreed_ = false;
    refresh();
}

void DefinitionEditorDialog::onDocumentChanged(const DocumentChanges& changes)
{
    if (!alive()) {
        refresh();
        return;
    }
    if (changes.library && existing_) {
        const LineStyle* found = context_.document->styleLibrary().find(*existing_);
        const std::optional<LineStyle> now =
            found != nullptr ? std::optional<LineStyle>(*found) : std::nullopt;
        if (now != seen_) {
            const bool wasEdited = dirty();
            seen_ = now;
            if (!now) {
                note_ = QStringLiteral("%1 was removed from the library elsewhere. It is still "
                                       "shown here: Save puts it back.")
                            .arg(inQuotes(*existing_));
            } else if (const Result<Form> form = formOf(*now); !form) {
                note_ = QStringLiteral("%1 was changed elsewhere into something that cannot be "
                                       "shown here, because %2. What is typed here is kept.")
                            .arg(inQuotes(*existing_), text(form.error().message));
            } else if (!wasEdited) {
                // Nothing typed is lost by showing what the library has now.
                source_ = now->source;
                baseline_ = *form;
                note_.clear();
                showForm(*form);
            } else {
                // What was typed is kept, and what it is measured against
                // moves to the library's: Revert must show what is there
                // now, not what was there when the form was opened.
                source_ = now->source;
                baseline_ = *form;
                note_ = QStringLiteral("%1 was changed elsewhere since it was opened here. "
                                       "What is typed here is kept: Save puts it in place of "
                                       "the library's, Revert shows the library's.")
                            .arg(inQuotes(*existing_));
            }
        }
    }
    if (changes.library || changes.surveyMap || changes.model) {
        refresh();
    }
}

void DefinitionEditorDialog::markProblemLine()
{
    QList<QTextEdit::ExtraSelection> marks;
    if (problemLine_ > 0) {
        const QTextBlock block = strokes_->document()->findBlockByNumber(problemLine_ - 1);
        if (block.isValid()) {
            QTextEdit::ExtraSelection mark;
            QColor wash = theme::error();
            wash.setAlpha(70);
            mark.format.setBackground(wash);
            mark.format.setProperty(QTextFormat::FullWidthSelection, true);
            mark.cursor = QTextCursor(block);
            marks << mark;
        }
    }
    strokes_->setExtraSelections(marks);
}

void DefinitionEditorDialog::refresh()
{
    if (loading_) {
        return;
    }
    const bool live = alive();
    const Reading reading = read();
    const bool symbol = kind_->currentIndex() == kSymbolRow;
    const bool twoPoint = kUnitsRows[static_cast<std::size_t>(std::max(0, units_->currentIndex()))]
                              .units == StyleUnits::TwoPoint;
    problemLine_ = reading.line;
    markProblemLine();

    // The picture follows text that reads, named or not. While a line is
    // half typed the last definition that did read stays up - a pane that
    // went blank at every keystroke of a stroke would show nothing for most
    // of the time a person types one - and the note under the panes says the
    // picture is not of what is typed now.
    if (reading.drawable) {
        preview_->setDefinition(*reading.drawable, StylePreview::DefinitionAs::Symbol);
        linePreview_->setDefinition(*reading.drawable, StylePreview::DefinitionAs::Linestyle);
        pictured_ = true;
    }
    previewNote_->setText(pictured_ && !reading.drawable
                              ? QStringLiteral("Not what is typed: the picture is of the last "
                                               "text that read.")
                              : QString());
    lineBox_->setVisible(!symbol);

    const katana::entity::StyleLibrary* library =
        live ? &context_.document->styleLibrary() : nullptr;
    const bool inLibrary = live && existing_ && library->contains(*existing_);
    const bool taken =
        live && !existing_ && reading.definition && library->contains(reading.definition->name);
    const bool changed = dirty();
    const bool offered = deleteAnywayOffered();

    QStringList parts;
    bool isError = false;
    if (!live) {
        parts << QStringLiteral("The drawing this editor worked on has closed: nothing here "
                                "acts on anything any more.");
        isError = true;
    } else {
        if (!said_.isEmpty()) {
            parts << said_;
            isError = saidIsError_;
        }
        if (offered) {
            // Of the definition the form is ON, whatever the form reads as
            // now: Delete acts on the library's, and an offer with nothing
            // under it would be FORCE unexplained.
            const katana::cad::DefinitionUsers users =
                katana::cad::definitionUsers(*context_.document, *existing_);
            // ONCE. The verb's refusal, shown above as it came, cites who
            // names the definition, in its own words (DefinitionUsers::cited),
            // and a person read the same users twice when this list followed
            // it. The list goes here only where what is shown above does not
            // cite every one of them: a line that failed for another reason
            // cites nobody, and a refusal that came before something else
            // came to name the definition does not cite that - and FORCE is
            // never offered with nothing saying what it passes over.
            if (!citesEvery(said_, users.cited(*existing_))) {
                parts << QStringLiteral("It is named by:\n%1").arg(listed(users.describe()));
            }
            parts << (QStringList{QStringLiteral(
                          "Delete Anyway runs the line again with FORCE.")} +
                      drawnWithoutIt(users))
                         .join(QLatin1Char(' '));
            isError = true;
        }
        if (!reading.definition) {
            parts << reading.problem;
            isError = true;
        } else {
            const std::string& name = reading.definition->name;
            if (taken) {
                parts << QStringLiteral("%1 is already in the library. A new definition needs "
                                        "a name of its own; Edit changes the one there is.")
                             .arg(inQuotes(name));
                isError = true;
            }
            if (!note_.isEmpty()) {
                parts << note_;
            }
            if (reading.definition->strokes.empty()) {
                // The format holds one, so it is saved; said, since a
                // definition that draws nothing is rarely what was meant.
                parts << QStringLiteral("No strokes yet: a definition is drawn by its strokes, "
                                        "and this one has none.");
            }
            // Not of a name that is taken: what names that, names the
            // definition there is and not the one being made. Nor twice,
            // under a refusal that has just listed them.
            if (!taken && !offered) {
                const katana::cad::DefinitionUsers users =
                    katana::cad::definitionUsers(*context_.document, name);
                const std::vector<std::string> named = users.describe();
                if (inLibrary) {
                    parts << (named.empty()
                                  ? QStringLiteral("No survey code, style or layer names it.")
                                  : QStringLiteral("Named by:\n%1").arg(listed(named)));
                } else {
                    if (!named.empty()) {
                        parts << QStringLiteral("Named already:\n%1").arg(listed(named));
                    }
                    parts << drawnUntilSaved(users, reading.definition->atVertices,
                                             inQuotes(name));
                }
            }
        }
    }
    issues_->setPlainText(parts.join(QStringLiteral("\n")));
    issues_->setStyleSheet(isError ? QStringLiteral("color: %1;").arg(theme::error().name())
                                   : QString());

    // A name is typed only while the definition is being made.
    name_->setEnabled(live && !existing_);
    for (QWidget* each : std::initializer_list<QWidget*>{group_, kind_, units_, atVertices_,
                                                         length_, factor_, originX_, originY_,
                                                         strokes_}) {
        each->setEnabled(live);
    }
    for (QWidget* each : std::initializer_list<QWidget*>{anchor1X_, anchor1Y_, anchor2X_,
                                                         anchor2Y_, stretchMode_, cycleMode_}) {
        each->setEnabled(live && twoPoint);
    }
    unitsNote_->setText(QString::fromLatin1(
        kUnitsRows[static_cast<std::size_t>(std::max(0, units_->currentIndex()))].note));
    sourceLabel_->setText(source_.empty() ? QStringLiteral("Made in this session")
                                          : text(source_));

    // Save has something to do: an edit, a definition not yet in the library,
    // or one that has gone from it.
    save_->setEnabled(live && reading.definition.has_value() && !taken &&
                      (changed || !inLibrary));
    revert_->setEnabled(live && changed);
    new_->setEnabled(live);
    duplicate_->setEnabled(inLibrary);
    delete_->setEnabled(inLibrary);
    // The two buttons THEMSELVES, not only the row they sit in: a button in
    // a hidden row is still a button a script can press (the headless driver
    // refuses one that is itself hidden or disabled, docs/headless.md), and
    // this one deletes what something uses.
    removePanel_->setVisible(offered);
    for (QPushButton* each : {removeAnyway_, removeCancel_}) {
        each->setVisible(offered);
        each->setEnabled(offered);
    }

    QString heading;
    if (existing_) {
        heading = QStringLiteral("%1 %2").arg(
            symbol ? QStringLiteral("Symbol") : QStringLiteral("Linestyle"), inQuotes(*existing_));
        if (live && !inLibrary) {
            heading += QStringLiteral(" - not in the library");
        }
    } else {
        heading = QStringLiteral("New %1").arg(kindWord(symbol));
        if (!origin_.isEmpty()) {
            heading += QStringLiteral(", ") + origin_;
        }
    }
    if (changed) {
        heading += QStringLiteral(" - edited, not saved");
    }
    heading_->setText(heading);
    setWindowTitle(changed ? QStringLiteral("Definition Editor *")
                           : QStringLiteral("Definition Editor"));
}

} // namespace katana::qt
