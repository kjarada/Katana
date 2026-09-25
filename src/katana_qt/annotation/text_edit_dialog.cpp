#include "annotation/text_edit_dialog.hpp"

#include <array>
#include <utility>
#include <variant>

#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QVBoxLayout>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/math/numerics.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace ann = katana::cad::annotation;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::TextGeometry;
using katana::entity::TextJustify;

namespace {

const QString kNoStyle = QStringLiteral("(none)");

// The nine, in reading order, by the names the verb takes.
constexpr std::array<TextJustify, 9> kJustifications{
    TextJustify::TopLeft,    TextJustify::TopCentre,    TextJustify::TopRight,
    TextJustify::MiddleLeft, TextJustify::MiddleCentre, TextJustify::MiddleRight,
    TextJustify::BottomLeft, TextJustify::BottomCentre, TextJustify::BottomRight};

// A number as the form shows it: twelve significant figures, which holds a
// survey coordinate to the millimetre and turns a stored 30.000000000000004
// degrees into 30. Only a changed field is sent, so the rounding never
// reaches the drawing.
QString shown(double value)
{
    return QString::number(value, 'g', 12);
}

QLineEdit* lineEdit(QWidget* parent, const char* name, const QString& tip)
{
    auto* line = new QLineEdit(parent);
    line->setObjectName(QString::fromLatin1(name));
    line->setToolTip(tip);
    return line;
}

// `text` read as a number for `what`, refused unless `allowed`.
Result<QString> numberWord(const QString& text, const char* what, bool (*allowed)(double),
                           const char* rule)
{
    const QString trimmed = text.trimmed();
    const auto value = katana::core::parseFiniteDouble(trimmed.toStdString());
    if (!value || !allowed(*value)) {
        return makeError(ErrorCode::ParseFailure,
                         std::string(what) + " must be " + rule + ", not '" +
                             trimmed.toStdString() + "'");
    }
    return trimmed;
}

QString describe(const katana::core::Error& error)
{
    return QString::fromStdString(error.message +
                                  (error.context.empty() ? std::string() : ": " + error.context));
}

} // namespace

TextEditDialog::TextEditDialog(katana::cad::Document& document, CommandRunner run, QWidget* parent)
    : QDialog(parent), document_(document), run_(std::move(run))
{
    setObjectName(QStringLiteral("textEditDialog"));
    setWindowTitle(QStringLiteral("Edit Text"));

    status_ = new QLabel(this);
    status_->setObjectName(QStringLiteral("textEditStatus"));
    status_->setWordWrap(true);
    text_ = new QPlainTextEdit(this);
    text_->setObjectName(QStringLiteral("textEditText"));
    text_->setToolTip(QStringLiteral("The words, a line a line: one text of several lines"));
    text_->setTabChangesFocus(true);
    style_ = new QComboBox(this);
    style_->setObjectName(QStringLiteral("textEditStyle"));
    style_->setToolTip(QStringLiteral("The text style it is set in; (none) for the plain face"));
    paper_ = lineEdit(this, "textEditPaper",
                      QStringLiteral("Height on paper in mm, drawn at the annotation scale; 0 for "
                                     "the style's, or a model height with no style"));
    height_ = lineEdit(this, "textEditHeight",
                       QStringLiteral("Height in model units; a paper-sized text's is worked out "
                                      "for the annotation scale"));
    justify_ = new QComboBox(this);
    justify_->setObjectName(QStringLiteral("textEditJustify"));
    justify_->setToolTip(QStringLiteral(
        "The point the position is: T, M or B for top, middle or bottom (the baseline), then L, C "
        "or R for left, centre or right"));
    for (const TextJustify justify : kJustifications) {
        justify_->addItem(QString::fromUtf8(katana::entity::toString(justify)));
    }
    rotation_ = lineEdit(this, "textEditRotation",
                         QStringLiteral("Degrees counter-clockwise from east"));
    position_ = lineEdit(this, "textEditPosition",
                         QStringLiteral("The justification point, as E,N"));
    problem_ = new QLabel(this);
    problem_->setObjectName(QStringLiteral("textEditProblem"));
    problem_->setWordWrap(true);
    problem_->setStyleSheet(QStringLiteral("color: %1").arg(theme::error().name()));
    apply_ = new QPushButton(QStringLiteral("Apply"), this);
    apply_->setObjectName(QStringLiteral("textEditApply"));
    apply_->setToolTip(QStringLiteral("Store what the form changed, as one undo step (TEXTEDIT)"));
    apply_->setAutoDefault(false);
    revert_ = new QPushButton(QStringLiteral("Revert"), this);
    revert_->setObjectName(QStringLiteral("textEditRevert"));
    revert_->setToolTip(QStringLiteral("Throw the form's edits away"));
    revert_->setAutoDefault(false);
    connect(apply_, &QPushButton::clicked, this, [this] { (void)apply(); });
    connect(revert_, &QPushButton::clicked, this, [this] { revert(); });

    auto* form = new QFormLayout;
    form->addRow(QStringLiteral("Text"), text_);
    form->addRow(QStringLiteral("Style"), style_);
    form->addRow(QStringLiteral("Height on paper (mm)"), paper_);
    form->addRow(QStringLiteral("Model height"), height_);
    form->addRow(QStringLiteral("Justification"), justify_);
    form->addRow(QStringLiteral("Rotation (degrees)"), rotation_);
    form->addRow(QStringLiteral("Position (E,N)"), position_);
    auto* buttons = new QHBoxLayout;
    buttons->addStretch();
    buttons->addWidget(revert_);
    buttons->addWidget(apply_);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(status_);
    layout->addLayout(form);
    layout->addWidget(problem_);
    layout->addLayout(buttons);

    listener_ = document_.addListener([this](const katana::cad::DocumentChange& change) {
        if (change.has(katana::cad::DocumentChange::AnnotationStyles |
                       katana::cad::DocumentChange::Replaced)) {
            fillStyles();
        }
        if (change.has(katana::cad::DocumentChange::Selection |
                       katana::cad::DocumentChange::Entities |
                       katana::cad::DocumentChange::Replaced)) {
            follow();
        }
    });
    fillStyles();
    follow();
}

void TextEditDialog::fillStyles()
{
    const QString keep = style_->currentText();
    style_->clear();
    style_->addItem(kNoStyle);
    document_.model().textStyles.forEach([&](const katana::entity::TextStyle& style) {
        style_->addItem(QString::fromStdString(style.name));
    });
    const int index = style_->findText(keep);
    style_->setCurrentIndex(index >= 0 ? index : 0);
}

void TextEditDialog::follow()
{
    const auto& model = document_.model();
    const std::vector<katana::entity::EntityId> ids = document_.selection().ids();
    const TextGeometry* text = nullptr;
    if (ids.size() == 1) {
        if (const auto* entity = model.entities.find(ids.front())) {
            text = std::get_if<TextGeometry>(&entity->geometry);
        }
    }
    if (text == nullptr) {
        id_ = 0;
        status_->setText(ids.empty()
                             ? QStringLiteral("Select one text in the drawing to edit it.")
                             : QStringLiteral("Select one text in the drawing to edit it: the "
                                              "selection is %1 entit%2%3.")
                                   .arg(ids.size())
                                   .arg(ids.size() == 1 ? QStringLiteral("y")
                                                        : QStringLiteral("ies"))
                                   .arg(ids.size() == 1 ? QStringLiteral(" that is not a text")
                                                        : QString()));
        setEditable(false);
        return;
    }
    // The same text, stored as it was loaded: keep what is being typed.
    if (ids.front() == id_ && *text == loaded_) {
        return;
    }
    load(ids.front(), *text);
}

void TextEditDialog::setEditable(bool editable)
{
    for (QWidget* widget : {static_cast<QWidget*>(text_), static_cast<QWidget*>(style_),
                            static_cast<QWidget*>(paper_), static_cast<QWidget*>(height_),
                            static_cast<QWidget*>(justify_), static_cast<QWidget*>(rotation_),
                            static_cast<QWidget*>(position_), static_cast<QWidget*>(apply_),
                            static_cast<QWidget*>(revert_)}) {
        widget->setEnabled(editable);
    }
}

void TextEditDialog::load(katana::entity::EntityId id, const TextGeometry& text)
{
    id_ = id;
    loaded_ = text;
    status_->setText(QStringLiteral("Text %1").arg(id));
    text_->setPlainText(QString::fromStdString(text.text));
    const int style = style_->findText(QString::fromStdString(text.style));
    style_->setCurrentIndex(text.style.empty() || style < 0 ? 0 : style);
    paper_->setText(shown(text.paperHeight));
    height_->setText(shown(text.height));
    justify_->setCurrentIndex(
        justify_->findText(QString::fromUtf8(katana::entity::toString(text.justify))));
    rotation_->setText(shown(text.rotation * katana::math::kRadToDeg));
    position_->setText(shown(text.position.x) + QStringLiteral(",") + shown(text.position.y));
    shown_ = form();
    problem_->clear();
    setEditable(true);
}

TextEditDialog::Shown TextEditDialog::form() const
{
    return Shown{text_->toPlainText(), style_->currentText(), paper_->text().trimmed(),
                 height_->text().trimmed(), justify_->currentText(), rotation_->text().trimmed(),
                 position_->text().trimmed()};
}

Result<QString> TextEditDialog::applyLine() const
{
    if (id_ == 0) {
        return makeError(ErrorCode::InvalidState, "select one text in the drawing first");
    }
    const Shown now = form();
    QStringList options;
    if (now.text != shown_.text) {
        if (now.text.trimmed().isEmpty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "a text must say something; erase it to remove it");
        }
        const auto word = ann::annotationTextWord(now.text.toStdString());
        if (!word) {
            return word.error();
        }
        options << QStringLiteral("text=") + QString::fromStdString(*word);
    }
    if (now.style != shown_.style) {
        const auto word = ann::commandWord(now.style == kNoStyle ? std::string()
                                                                 : now.style.toStdString());
        if (!word) {
            return word.error();
        }
        options << QStringLiteral("style=") + QString::fromStdString(*word);
    }
    if (now.paper != shown_.paper) {
        auto word = numberWord(now.paper, "the height on paper",
                               [](double v) { return v >= 0.0; }, "0 or more millimetres");
        if (!word) {
            return word.error();
        }
        options << QStringLiteral("paper=") + *word;
    }
    if (now.height != shown_.height) {
        auto word = numberWord(now.height, "the model height", [](double v) { return v > 0.0; },
                               "greater than zero");
        if (!word) {
            return word.error();
        }
        options << QStringLiteral("height=") + *word;
    }
    if (now.justify != shown_.justify) {
        options << QStringLiteral("justify=") + now.justify;
    }
    if (now.rotation != shown_.rotation) {
        auto word = numberWord(now.rotation, "the rotation", [](double) { return true; },
                               "a number of degrees");
        if (!word) {
            return word.error();
        }
        options << QStringLiteral("rotation=") + *word;
    }
    if (now.position != shown_.position) {
        QString at = now.position;
        at.remove(QLatin1Char(' '));
        const QStringList parts = at.split(QLatin1Char(','));
        if (parts.size() != 2 || !katana::core::parseFiniteDouble(parts[0].toStdString()) ||
            !katana::core::parseFiniteDouble(parts[1].toStdString())) {
            return makeError(ErrorCode::ParseFailure,
                             "the position must be E,N, not '" + now.position.toStdString() + "'");
        }
        options << QStringLiteral("at=") + at;
    }
    if (options.isEmpty()) {
        return QString();
    }
    return QStringLiteral("TEXTEDIT %1 %2").arg(id_).arg(options.join(QLatin1Char(' ')));
}

QString TextEditDialog::status() const
{
    return status_->text();
}

QString TextEditDialog::problem() const
{
    return problem_->text();
}

bool TextEditDialog::apply()
{
    const auto line = applyLine();
    if (!line) {
        problem_->setText(describe(line.error()));
        return false;
    }
    if (line->isEmpty()) {
        problem_->clear();
        return true; // nothing changed: no step
    }
    if (!run_) {
        problem_->setText(QStringLiteral("there is no command line to run the TEXTEDIT line on"));
        return false;
    }
    const VerbOutcome outcome = run_(*line);
    problem_->setText(outcome.ok ? QString()
                                 : outcome.error.isEmpty() ? QStringLiteral("refused: ") + *line
                                                           : outcome.error);
    return outcome.ok;
}

void TextEditDialog::revert()
{
    problem_->clear();
    if (id_ != 0) {
        load(id_, loaded_);
    }
}

} // namespace katana::qt
