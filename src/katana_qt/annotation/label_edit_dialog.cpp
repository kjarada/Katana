#include "annotation/label_edit_dialog.hpp"

#include <optional>
#include <utility>
#include <variant>

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QStringList>
#include <QVBoxLayout>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"

namespace katana::qt {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

QString exact(double value)
{
    return QString::fromStdString(katana::core::formatExactReal(value));
}

// A value as a word of the LABEL SET line, by the rule every dialog writes
// words with (cad/annotation/command_words.hpp); a refusal names the field.
Result<QString> written(Result<std::string> word, const char* field)
{
    if (!word) {
        return makeError(word.error().code, std::string(field) + ": " + word.error().message,
                         word.error().context);
    }
    return QString::fromStdString(*word);
}

std::optional<double> number(const QString& text)
{
    return katana::core::parseFiniteDouble(text.trimmed().toStdString());
}

} // namespace

Result<LabelEditForm> labelEditFormOf(const katana::entity::Model& model,
                                      katana::entity::EntityId id)
{
    const katana::entity::Entity* entity = model.entities.find(id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist", "id=" + std::to_string(id));
    }
    const auto* label = std::get_if<katana::entity::LabelGeometry>(&entity->geometry);
    if (label == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "that entity is not a label",
                         "id=" + std::to_string(id));
    }
    LabelEditForm form;
    form.id = id;
    form.style = QString::fromStdString(label->style);
    form.override = !label->textOverride.empty();
    form.text = QString::fromStdString(label->textOverride);
    form.pinned = label->position.has_value();
    const katana::geometry::Point2 at = label->position.value_or(label->anchor);
    form.easting = exact(at.x);
    form.northing = exact(at.y);
    form.layer = QString::fromStdString(entity->layer);
    return form;
}

Result<QString> labelSetLine(const LabelEditForm& form, const LabelEditForm& current)
{
    if (form.id == 0) {
        return makeError(ErrorCode::InvalidArgument, "no label is chosen: select one label first");
    }
    namespace ann = katana::cad::annotation;
    const QString style = form.style.trimmed();
    const QString layer = form.layer.trimmed();
    if (style.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "Style: choose a label style");
    }
    if (layer.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "Layer: choose the layer the label is on");
    }
    // Every field is written, even one left as it was, so a value no line
    // can carry is refused whichever field changed.
    const auto styleWord = written(ann::commandWord(style.toStdString()), "Style");
    if (!styleWord) {
        return styleWord.error();
    }
    const auto layerWord = written(ann::commandWord(layer.toStdString()), "Layer");
    if (!layerWord) {
        return layerWord.error();
    }

    QStringList words{QStringLiteral("LABEL"), QStringLiteral("SET"), QString::number(form.id)};
    if (style != current.style) {
        words << "style=" + *styleWord;
    }
    if (form.override) {
        if (form.text.isEmpty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "Own text: type the label's words, or untick Own text for its "
                             "style's");
        }
        if (form.text.trimmed().compare(QStringLiteral("none"), Qt::CaseInsensitive) == 0) {
            return makeError(ErrorCode::InvalidArgument,
                             "Own text: the word none on its own means \"no own text\" to LABEL "
                             "SET; untick Own text for the style's words");
        }
        // LABEL SET's text= reads "\n" as a line break, as TEXT's does, so
        // the text is written as the annotation verbs' texts are - and one
        // holding a typed backslash and n, which would come back a break, is
        // refused.
        const auto textWord = written(ann::annotationTextWord(form.text.toStdString()),
                                      "Own text");
        if (!textWord) {
            return textWord.error();
        }
        if (!current.override || form.text != current.text) {
            words << "text=" + *textWord;
        }
    } else if (current.override) {
        words << QStringLiteral("text=none");
    }
    if (form.pinned) {
        const auto x = number(form.easting);
        const auto y = number(form.northing);
        if (!x) {
            return makeError(ErrorCode::InvalidArgument,
                             "Easting: '" + form.easting.toStdString() + "' is not a number");
        }
        if (!y) {
            return makeError(ErrorCode::InvalidArgument,
                             "Northing: '" + form.northing.toStdString() + "' is not a number");
        }
        const auto wasX = number(current.easting);
        const auto wasY = number(current.northing);
        if (!current.pinned || wasX != x || wasY != y) {
            words << "at=" + exact(*x) + "," + exact(*y);
        }
    } else if (current.pinned) {
        words << QStringLiteral("at=none");
    }
    if (layer != current.layer) {
        words << "layer=" + *layerWord;
    }
    if (words.size() == 3) {
        return QString();
    }
    return words.join(' ');
}

LabelEditDialog::LabelEditDialog(katana::cad::Document& document, CommandRunner run,
                                 QWidget* parent)
    : QDialog(parent), document_(document), run_(std::move(run))
{
    setObjectName(QStringLiteral("labelEditDialog"));
    setWindowTitle(QStringLiteral("Edit Label"));

    target_ = new QLabel(this);
    target_->setObjectName(QStringLiteral("labelEditTarget"));
    target_->setWordWrap(true);
    style_ = new QComboBox(this);
    style_->setObjectName(QStringLiteral("labelEditStyle"));
    style_->setToolTip(QStringLiteral("What the label says and where it goes (Format > Label "
                                      "Styles and Rules)"));
    override_ = new QCheckBox(QStringLiteral("&Own text"), this);
    override_->setObjectName(QStringLiteral("labelEditOverride"));
    override_->setToolTip(QStringLiteral("Say these words instead of the style's; unticked, the "
                                         "label says what its style works out from its target"));
    text_ = new QLineEdit(this);
    text_->setObjectName(QStringLiteral("labelEditText"));
    pinned_ = new QCheckBox(QStringLiteral("&Pinned at"), this);
    pinned_->setObjectName(QStringLiteral("labelEditPinned"));
    pinned_->setToolTip(QStringLiteral("Put the text at this point, with a leader back when its "
                                       "style draws one; unticked, the placer finds it room at "
                                       "every scale"));
    easting_ = new QLineEdit(this);
    easting_->setObjectName(QStringLiteral("labelEditEasting"));
    easting_->setPlaceholderText(QStringLiteral("Easting"));
    northing_ = new QLineEdit(this);
    northing_->setObjectName(QStringLiteral("labelEditNorthing"));
    northing_->setPlaceholderText(QStringLiteral("Northing"));
    layer_ = new QComboBox(this);
    layer_->setObjectName(QStringLiteral("labelEditLayer"));
    layer_->setEditable(true);
    command_ = new QLineEdit(this);
    command_->setObjectName(QStringLiteral("labelEditCommand"));
    command_->setReadOnly(true);
    command_->setToolTip(QStringLiteral("The line OK and Apply run, as it would be typed"));
    status_ = new QLabel(this);
    status_->setObjectName(QStringLiteral("labelEditStatus"));
    status_->setWordWrap(true);

    ok_ = new QPushButton(QStringLiteral("OK"), this);
    ok_->setObjectName(QStringLiteral("labelEditOk"));
    ok_->setDefault(true);
    apply_ = new QPushButton(QStringLiteral("&Apply"), this);
    apply_->setObjectName(QStringLiteral("labelEditApply"));
    auto* cancel = new QPushButton(QStringLiteral("Cancel"), this);
    cancel->setObjectName(QStringLiteral("labelEditCancel"));

    auto* place = new QHBoxLayout();
    place->addWidget(pinned_);
    place->addWidget(easting_);
    place->addWidget(northing_);
    auto* fields = new QFormLayout();
    fields->addRow(QStringLiteral("Label"), target_);
    fields->addRow(QStringLiteral("&Style"), style_);
    fields->addRow(override_, text_);
    fields->addRow(place);
    fields->addRow(QStringLiteral("La&yer"), layer_);
    fields->addRow(QStringLiteral("Command"), command_);
    auto* buttons = new QHBoxLayout();
    buttons->addStretch();
    buttons->addWidget(ok_);
    buttons->addWidget(apply_);
    buttons->addWidget(cancel);
    auto* layout = new QVBoxLayout(this);
    layout->addLayout(fields);
    layout->addWidget(status_);
    layout->addLayout(buttons);

    const auto changed = [this] { updateLine(); };
    connect(style_, &QComboBox::currentTextChanged, this, changed);
    connect(layer_, &QComboBox::currentTextChanged, this, changed);
    connect(text_, &QLineEdit::textChanged, this, changed);
    connect(easting_, &QLineEdit::textChanged, this, changed);
    connect(northing_, &QLineEdit::textChanged, this, changed);
    connect(override_, &QCheckBox::toggled, this, [this](bool on) {
        text_->setEnabled(on && current_.id != 0);
        updateLine();
    });
    connect(pinned_, &QCheckBox::toggled, this, [this](bool on) {
        easting_->setEnabled(on && current_.id != 0);
        northing_->setEnabled(on && current_.id != 0);
        updateLine();
    });
    connect(ok_, &QPushButton::clicked, this, [this] {
        if (apply()) {
            accept();
        }
    });
    connect(apply_, &QPushButton::clicked, this, [this] { (void)apply(); });
    connect(cancel, &QPushButton::clicked, this, &QDialog::reject);

    setEnabledFields(false);
}

bool LabelEditDialog::load(katana::entity::EntityId id)
{
    auto loaded = labelEditFormOf(document_.model(), id);
    if (!loaded) {
        current_ = LabelEditForm{};
        showForm(current_);
        target_->setText(QStringLiteral("-"));
        setEnabledFields(false);
        status_->setText(QString::fromStdString(loaded.error().message) + " (" +
                         QString::fromStdString(loaded.error().context) + ")");
        return false;
    }
    current_ = *loaded;
    const auto& model = document_.model();
    const auto& label =
        std::get<katana::entity::LabelGeometry>(model.entities.find(id)->geometry);
    QString what;
    if (label.target != 0) {
        const katana::entity::Entity* target = model.entities.find(label.target);
        what = QStringLiteral("entity %1").arg(label.target);
        if (target != nullptr) {
            what += QStringLiteral(" (%1)").arg(
                QString::fromUtf8(katana::entity::toString(target->type()).data()));
        }
    } else {
        what = QStringLiteral("alignment %1").arg(QString::fromStdString(label.alignment));
    }
    if (label.part >= 0) {
        what += QStringLiteral(", segment %1").arg(label.part);
    }
    target_->setText(QStringLiteral("Label %1 of %2").arg(id).arg(what));
    showForm(current_);
    setEnabledFields(true);
    status_->clear();
    updateLine();
    return true;
}

bool LabelEditDialog::loadSelection()
{
    const auto ids = document_.selection().ids();
    if (ids.size() != 1) {
        current_ = LabelEditForm{};
        showForm(current_);
        target_->setText(QStringLiteral("-"));
        setEnabledFields(false);
        status_->setText(ids.empty() ? QStringLiteral("Select one label, then choose Edit Label.")
                                     : QStringLiteral("%1 entities are selected; select one "
                                                      "label, then choose Edit Label.")
                                           .arg(ids.size()));
        return false;
    }
    return load(ids.front());
}

void LabelEditDialog::showForm(const LabelEditForm& form)
{
    filling_ = true;
    const auto& model = document_.model();
    style_->clear();
    for (const std::string& name : model.labelStyles.names()) {
        style_->addItem(QString::fromStdString(name));
    }
    // A label whose style has gone still shows it, so OK does not change it
    // unasked; LABEL SET refuses nothing for it, the painter draws nothing.
    if (!form.style.isEmpty() && style_->findText(form.style) < 0) {
        style_->addItem(form.style);
    }
    style_->setCurrentIndex(style_->findText(form.style));
    layer_->clear();
    for (const std::string& name : model.layers.names()) {
        layer_->addItem(QString::fromStdString(name));
    }
    const int layer = layer_->findText(form.layer);
    if (layer >= 0) {
        layer_->setCurrentIndex(layer);
    } else {
        layer_->setEditText(form.layer);
    }
    override_->setChecked(form.override);
    text_->setText(form.text);
    pinned_->setChecked(form.pinned);
    easting_->setText(form.easting);
    northing_->setText(form.northing);
    filling_ = false;
}

void LabelEditDialog::setEnabledFields(bool enabled)
{
    for (QWidget* widget :
         std::initializer_list<QWidget*>{style_, override_, pinned_, layer_, ok_, apply_}) {
        widget->setEnabled(enabled);
    }
    text_->setEnabled(enabled && override_->isChecked());
    easting_->setEnabled(enabled && pinned_->isChecked());
    northing_->setEnabled(enabled && pinned_->isChecked());
}

LabelEditForm LabelEditDialog::form() const
{
    LabelEditForm form;
    form.id = current_.id;
    form.style = style_->currentText();
    form.override = override_->isChecked();
    form.text = text_->text();
    form.pinned = pinned_->isChecked();
    form.easting = easting_->text();
    form.northing = northing_->text();
    form.layer = layer_->currentText();
    return form;
}

QString LabelEditDialog::status() const { return status_->text(); }

QString LabelEditDialog::line() const
{
    const auto made = labelSetLine(form(), current_);
    return made ? *made : QString();
}

void LabelEditDialog::updateLine()
{
    if (filling_ || current_.id == 0) {
        command_->clear();
        return;
    }
    const auto made = labelSetLine(form(), current_);
    if (!made) {
        command_->clear();
        status_->setText(QString::fromStdString(made.error().message));
        return;
    }
    command_->setText(*made);
    status_->setText(made->isEmpty() ? QStringLiteral("Nothing to change.") : QString());
}

bool LabelEditDialog::apply()
{
    const auto made = labelSetLine(form(), current_);
    if (!made) {
        status_->setText(QString::fromStdString(made.error().message));
        return false;
    }
    if (made->isEmpty()) {
        status_->setText(QStringLiteral("Nothing to change."));
        return true;
    }
    if (!run_) {
        status_->setText(QStringLiteral("There is no command line to run it on."));
        return false;
    }
    const VerbOutcome outcome = run_(*made);
    if (!outcome.ok) {
        status_->setText(outcome.error.isEmpty() ? QStringLiteral("The line was refused.")
                                                 : outcome.error);
        return false;
    }
    // The label as it now is, so the next Apply compares against it.
    const katana::entity::EntityId id = current_.id;
    (void)load(id);
    status_->setText(outcome.reply.isEmpty() ? QStringLiteral("Done.") : outcome.reply);
    return true;
}

} // namespace katana::qt
