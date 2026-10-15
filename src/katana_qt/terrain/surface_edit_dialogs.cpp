// Terrain > Edit Surface's dialogs and Combine Surfaces (surface_edit_dialogs.hpp).

#include "terrain/surface_edit_dialogs.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QShowEvent>
#include <QSpinBox>
#include <QStackedWidget>
#include <QVBoxLayout>

#include <utility>

#include "customisation/document_watcher.hpp"
#include "geo/terrain_dialog_support.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

std::optional<double> number(const QString& text)
{
    return katana::core::parseFiniteDouble(text.trimmed().toStdString());
}

// A surface's name as a word of a line, or why it cannot be one.
Result<QString> surfaceWord(const QString& name, const QString& field)
{
    if (name.trimmed().isEmpty()) {
        return invalid(field + ": there is no surface; make one first (Terrain > Surface From)");
    }
    const auto word = lineWord(name.trimmed());
    if (!word) {
        return invalid(field + ": a name with a double quote cannot be written on a line");
    }
    return *word;
}

// The surfaces of the drawing, by name, in their order.
QStringList surfaceNames(const katana::cad::Document* document)
{
    QStringList names;
    if (document != nullptr) {
        for (const auto& surface : document->surfaces().all()) {
            names << QString::fromStdString(surface.name);
        }
    }
    return names;
}

// Refills a combo with `names`, keeping the chosen one when it is still there.
void fillNames(QComboBox& combo, const QStringList& names)
{
    const QString chosen = combo.currentText();
    combo.blockSignals(true);
    combo.clear();
    combo.addItems(names);
    const int at = combo.findText(chosen);
    combo.setCurrentIndex(at >= 0 ? at : (names.isEmpty() ? -1 : 0));
    combo.blockSignals(false);
}

void showOutcome(QPlainTextEdit& reply, const VerbOutcome& outcome)
{
    reply.setPlainText(outcome.ok ? outcome.reply
                                  : (outcome.error.isEmpty() ? outcome.reply : outcome.error));
}

} // namespace

QString surfaceLevelsPrefix(SurfaceLevelsKind kind)
{
    switch (kind) {
    case SurfaceLevelsKind::Heights:
        return "surfaceHeights";
    case SurfaceLevelsKind::Smooth:
        return "surfaceSmooth";
    case SurfaceLevelsKind::Thin:
        return "surfaceThin";
    case SurfaceLevelsKind::DeleteLong:
        return "surfaceDeleteLong";
    }
    return "surfaceEdit";
}

Result<QString> surfaceLevelsLine(SurfaceLevelsKind kind, const SurfaceLevelsForm& form)
{
    auto surface = surfaceWord(form.surface, "Surface");
    if (!surface) {
        return surface.error();
    }
    QString line = "SURFACE EDIT " + *surface;
    const auto value = number(form.value);
    switch (kind) {
    case SurfaceLevelsKind::Heights: {
        static const char* const kWords[] = {"RAISE", "LOWER", "SETLEVEL", "SCALE"};
        if (form.operation < 0 || form.operation > 3) {
            return invalid("Change: choose Raise, Lower, Set Level or Scale");
        }
        if (!value) {
            return invalid(form.operation == 2   ? "Level: type the level, a number"
                           : form.operation == 3 ? "Factor: type the factor, a number"
                                                 : "By: type how far, a number");
        }
        line += QString(" ") + kWords[form.operation] + " " + form.value.trimmed();
        if (form.operation == 3 && !form.base.trimmed().isEmpty()) {
            if (!number(form.base)) {
                return invalid("Base: a level, a number, or blank for the lowest level changed");
            }
            line += " base=" + form.base.trimmed();
        }
        break;
    }
    case SurfaceLevelsKind::Smooth: {
        const auto weight = number(form.weight);
        if (!weight || !(*weight > 0.0 && *weight <= 1.0)) {
            return invalid("Weight: a number above 0 and at most 1");
        }
        if (form.passes < 1 || form.passes > 1000) {
            return invalid("Passes: from 1 to 1000");
        }
        line += " SMOOTH passes=" + QString::number(form.passes) + " weight=" + form.weight.trimmed();
        break;
    }
    case SurfaceLevelsKind::Thin:
        if (!value || *value < 0.0) {
            return invalid("Tolerance: a level difference, 0 or more");
        }
        return line + " THIN " + form.value.trimmed();
    case SurfaceLevelsKind::DeleteLong:
        if (!value || !(*value > 0.0) || (!form.longSides && *value >= 180.0)) {
            return invalid(form.longSides ? "Longer than: a length above 0"
                                          : "Thinner than: an angle in degrees, above 0 and "
                                            "below 180");
        }
        return line + (form.longSides ? " DELETE LONG " : " DELETE THIN ") + form.value.trimmed();
    }
    if (form.inside) {
        if (!form.scopeError.isEmpty()) {
            return invalid("Only inside: " + form.scopeError);
        }
        if (form.scope.trimmed().isEmpty()) {
            return invalid("Only inside: say which polygon");
        }
        line += " INSIDE FROM " + form.scope.trimmed();
    }
    return line;
}

Result<QString> surfaceCombineLine(const SurfaceCombineForm& form)
{
    const auto name = [&form](bool required) -> Result<QString> {
        if (form.name.trimmed().isEmpty()) {
            if (required) {
                return invalid("New surface: type its name");
            }
            return QString();
        }
        const auto word = lineWord(form.name.trimmed());
        if (!word) {
            return invalid("New surface: a name with a double quote cannot be written on a line");
        }
        return " NAME " + *word;
    };
    switch (form.mode) {
    case SurfaceCombineMode::Merge: {
        if (form.merge.size() < 2) {
            return invalid("Surfaces: tick at least two to merge");
        }
        QString line = "SURFACE MERGE";
        for (const QString& surface : form.merge) {
            auto word = surfaceWord(surface, "Surfaces");
            if (!word) {
                return word.error();
            }
            line += " " + *word;
        }
        auto named = name(true);
        if (!named) {
            return named.error();
        }
        return line + *named;
    }
    case SurfaceCombineMode::Paste: {
        auto top = surfaceWord(form.top, "Top");
        if (!top) {
            return top.error();
        }
        auto base = surfaceWord(form.base, "Base");
        if (!base) {
            return base.error();
        }
        if (katana::core::equalsIgnoringCase(form.top.trimmed().toStdString(),
                                             form.base.trimmed().toStdString())) {
            return invalid("Top: choose a surface other than the base");
        }
        auto named = name(false);
        if (!named) {
            return named.error();
        }
        return "SURFACE PASTE " + *top + " INTO " + *base + *named;
    }
    case SurfaceCombineMode::Clip: {
        auto surface = surfaceWord(form.surface, "Surface");
        if (!surface) {
            return surface.error();
        }
        if (!form.scopeError.isEmpty()) {
            return invalid("Polygon: " + form.scopeError);
        }
        if (form.scope.trimmed().isEmpty()) {
            return invalid("Polygon: say which polygon");
        }
        auto named = name(true);
        if (!named) {
            return named.error();
        }
        return "SURFACE CLIP " + *surface + " FROM " + form.scope.trimmed() + *named;
    }
    case SurfaceCombineMode::Offset: {
        auto surface = surfaceWord(form.surface, "Surface");
        if (!surface) {
            return surface.error();
        }
        if (!number(form.dz)) {
            return invalid("Raise by: a number (negative lowers)");
        }
        auto named = name(true);
        if (!named) {
            return named.error();
        }
        return "SURFACE OFFSET " + *surface + " " + form.dz.trimmed() + *named;
    }
    }
    return invalid("Combine: choose Merge, Paste, Clip or Offset");
}

// ---- SurfaceLevelsDialog ---------------------------------------------------------------

SurfaceLevelsDialog::SurfaceLevelsDialog(SurfaceLevelsKind kind, SurfaceEditDialogContext context,
                                         QWidget* parent)
    : QDialog(parent), kind_(kind), context_(std::move(context))
{
    const QString p = surfaceLevelsPrefix(kind);
    setObjectName(p + "Dialog");
    switch (kind) {
    case SurfaceLevelsKind::Heights:
        setWindowTitle("Raise, Lower or Set Level");
        break;
    case SurfaceLevelsKind::Smooth:
        setWindowTitle("Smooth Surface");
        break;
    case SurfaceLevelsKind::Thin:
        setWindowTitle("Thin Surface");
        break;
    case SurfaceLevelsKind::DeleteLong:
        setWindowTitle("Delete Long or Thin Triangles");
        break;
    }
    auto* layout = new QVBoxLayout(this);
    auto* fields = new QFormLayout();
    surface_ = new QComboBox(this);
    surface_->setObjectName(p + "Surface");
    surface_->setToolTip("The surface of the drawing the edit changes, as one undo step");
    fields->addRow("Surface:", surface_);
    value_ = new QLineEdit(this);
    value_->setObjectName(p + "Value");
    switch (kind) {
    case SurfaceLevelsKind::Heights:
        operation_ = new QComboBox(this);
        operation_->setObjectName(p + "Operation");
        operation_->addItems({"Raise", "Lower", "Set Level", "Scale"});
        fields->addRow("Change:", operation_);
        value_->setText("1");
        value_->setToolTip("How far to raise or lower, the level to set, or the factor to scale "
                           "heights by");
        fields->addRow("By / to:", value_);
        base_ = new QLineEdit(this);
        base_->setObjectName(p + "Base");
        base_->setPlaceholderText("lowest level changed");
        base_->setToolTip("Scale only: the level that stays where it is");
        fields->addRow("Scale about:", base_);
        break;
    case SurfaceLevelsKind::Smooth:
        passes_ = new QSpinBox(this);
        passes_->setObjectName(p + "Passes");
        passes_->setRange(1, 1000);
        passes_->setValue(3);
        fields->addRow("Passes:", passes_);
        weight_ = new QLineEdit("0.5", this);
        weight_->setObjectName(p + "Weight");
        weight_->setToolTip("How far each pass moves a vertex towards the mean of its "
                            "neighbours: above 0, at most 1");
        fields->addRow("Weight:", weight_);
        value_->hide();
        break;
    case SurfaceLevelsKind::Thin:
        value_->setText("0.01");
        value_->setToolTip("The most the surface may move, vertically, anywhere");
        fields->addRow("Tolerance:", value_);
        break;
    case SurfaceLevelsKind::DeleteLong:
        measure_ = new QComboBox(this);
        measure_->setObjectName(p + "Measure");
        measure_->addItems({"A side longer than", "An angle smaller than (degrees)"});
        fields->addRow("Delete triangles with:", measure_);
        value_->setText("50");
        fields->addRow("Value:", value_);
        break;
    }
    layout->addLayout(fields);
    if (kind == SurfaceLevelsKind::Heights || kind == SurfaceLevelsKind::Smooth) {
        inside_ = new QCheckBox("Only inside a polygon of the drawing", this);
        inside_->setObjectName(p + "Inside");
        layout->addWidget(inside_);
        scope_ = new ScopeFilterWidget(p, this);
        scope_->views = context_.views;
        scope_->setEnabled(false);
        layout->addWidget(scope_);
        connect(inside_, &QCheckBox::toggled, this, [this](bool on) {
            scope_->setEnabled(on);
            refreshCommand();
        });
        scope_->onChanged = [this] { refreshCommand(); };
    }
    command_ = terrainCommandField(this, p + "Command");
    layout->addWidget(command_);
    auto* buttons = new QHBoxLayout();
    auto* preview = preview_ = new QPushButton("Preview", this);
    preview->setObjectName(p + "Preview");
    preview->setToolTip("Run the line with PREVIEW: what it would change, nothing changed");
    auto* run = run_ = new QPushButton("Run", this);
    run->setObjectName(p + "Run");
    run->setDefault(true);
    auto* close = new QPushButton("Close", this);
    close->setObjectName(p + "Close");
    buttons->addWidget(preview);
    buttons->addStretch(1);
    buttons->addWidget(run);
    buttons->addWidget(close);
    layout->addLayout(buttons);
    reply_ = terrainReplyField(this, p + "Reply");
    layout->addWidget(reply_);

    const auto changed = [this] { refreshCommand(); };
    connect(surface_, &QComboBox::currentIndexChanged, this, changed);
    connect(value_, &QLineEdit::textChanged, this, changed);
    if (operation_ != nullptr) {
        connect(operation_, &QComboBox::currentIndexChanged, this, [this] {
            base_->setEnabled(operation_->currentIndex() == 3);
            refreshCommand();
        });
        base_->setEnabled(false);
        connect(base_, &QLineEdit::textChanged, this, changed);
    }
    if (passes_ != nullptr) {
        connect(passes_, &QSpinBox::valueChanged, this, changed);
        connect(weight_, &QLineEdit::textChanged, this, changed);
    }
    if (measure_ != nullptr) {
        connect(measure_, &QComboBox::currentIndexChanged, this, changed);
    }
    connect(preview, &QPushButton::clicked, this, [this] { runLine(true); });
    connect(run, &QPushButton::clicked, this, [this] { runLine(false); });
    connect(close, &QPushButton::clicked, this, &QDialog::close);
    reload();
    if (context_.document != nullptr) {
        // The dialog is non-modal and kept, so the drawing changes under it: an
        // undo that removes the surface it offers, a typed SURFACE line, New,
        // Open. A DocumentWatcher says so from the event loop, and the lists
        // are read again with what is chosen kept where it is still there.
        watcher_ = std::make_unique<DocumentWatcher>(*context_.document,
                                                     [this](const DocumentChanges& changes) {
                                                         if (changes.model) {
                                                             reload();
                                                         }
                                                     });
    }
}

SurfaceLevelsDialog::~SurfaceLevelsDialog() = default;

void SurfaceLevelsDialog::reload()
{
    fillNames(*surface_, surfaceNames(context_.document));
    if (scope_ != nullptr && context_.document != nullptr) {
        scope_->reload(context_.document->model());
    }
    refreshCommand();
}

SurfaceLevelsForm SurfaceLevelsDialog::form() const
{
    SurfaceLevelsForm form;
    form.surface = surface_->currentText();
    form.value = value_->text();
    if (operation_ != nullptr) {
        form.operation = operation_->currentIndex();
        form.base = base_->text();
    }
    if (passes_ != nullptr) {
        form.passes = passes_->value();
        form.weight = weight_->text();
    }
    if (measure_ != nullptr) {
        form.longSides = measure_->currentIndex() == 0;
    }
    if (inside_ != nullptr && inside_->isChecked()) {
        form.inside = true;
        const auto words = scope_->verbWords();
        if (words) {
            form.scope = *words;
        } else {
            form.scopeError = QString::fromStdString(words.error().message);
        }
    }
    return form;
}

Result<QString> SurfaceLevelsDialog::command() const { return surfaceLevelsLine(kind_, form()); }

void SurfaceLevelsDialog::showEvent(QShowEvent* event)
{
    reload();
    QDialog::showEvent(event);
}

void SurfaceLevelsDialog::refreshCommand()
{
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    // Nothing to run until the fields say a line; the placeholder says why.
    if (run_ != nullptr) {
        run_->setEnabled(line.ok());
        preview_->setEnabled(line.ok());
    }
}

void SurfaceLevelsDialog::runLine(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    if (!context_.run) {
        reply_->setPlainText("There is no window to run the line in.");
        return;
    }
    showOutcome(*reply_, context_.run(preview ? *line + " PREVIEW" : *line));
    if (!preview) {
        reload();
    }
}

// ---- SurfaceCombineDialog ---------------------------------------------------------------

SurfaceCombineDialog::SurfaceCombineDialog(SurfaceEditDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("surfaceCombineDialog");
    setWindowTitle("Combine Surfaces");
    auto* layout = new QVBoxLayout(this);
    auto* top = new QFormLayout();
    mode_ = new QComboBox(this);
    mode_->setObjectName("surfaceCombineMode");
    mode_->addItems({"Merge: later surfaces win", "Paste: a top into a base, with a clean seam",
                     "Clip: a copy of the part inside a polygon", "Offset: a copy raised by dz"});
    top->addRow("Make:", mode_);
    layout->addLayout(top);

    pages_ = new QStackedWidget(this);
    pages_->setObjectName("surfaceCombinePages");
    // Merge.
    auto* mergePage = new QWidget(pages_);
    auto* mergeLayout = new QHBoxLayout(mergePage);
    mergeList_ = new QListWidget(mergePage);
    mergeList_->setObjectName("surfaceCombineMergeList");
    mergeList_->setToolTip("Tick the surfaces to merge; lower in the list wins where they overlap");
    mergeLayout->addWidget(mergeList_);
    auto* order = new QVBoxLayout();
    auto* up = new QPushButton("Up", mergePage);
    up->setObjectName("surfaceCombineUp");
    auto* down = new QPushButton("Down", mergePage);
    down->setObjectName("surfaceCombineDown");
    order->addWidget(up);
    order->addWidget(down);
    order->addStretch(1);
    mergeLayout->addLayout(order);
    pages_->addWidget(mergePage);
    // Paste.
    auto* pastePage = new QWidget(pages_);
    auto* pasteFields = new QFormLayout(pastePage);
    top_ = new QComboBox(pastePage);
    top_->setObjectName("surfaceCombineTop");
    base_ = new QComboBox(pastePage);
    base_->setObjectName("surfaceCombineBase");
    pasteFields->addRow("Top (wins inside its footprint):", top_);
    pasteFields->addRow("Base:", base_);
    pages_->addWidget(pastePage);
    // Clip and Offset share the source.
    auto* sourcePage = new QWidget(pages_);
    auto* sourceLayout = new QVBoxLayout(sourcePage);
    auto* sourceFields = new QFormLayout();
    source_ = new QComboBox(sourcePage);
    source_->setObjectName("surfaceCombineSource");
    sourceFields->addRow("Surface:", source_);
    dz_ = new QLineEdit("1", sourcePage);
    dz_->setObjectName("surfaceCombineDz");
    dz_->setToolTip("How far to raise the copy; negative lowers it");
    sourceFields->addRow("Raise by:", dz_);
    sourceLayout->addLayout(sourceFields);
    scope_ = new ScopeFilterWidget("surfaceCombine", sourcePage);
    scope_->views = context_.views;
    sourceLayout->addWidget(scope_);
    pages_->addWidget(sourcePage);
    layout->addWidget(pages_);

    auto* nameFields = new QFormLayout();
    name_ = new QLineEdit(this);
    name_->setObjectName("surfaceCombineName");
    name_->setPlaceholderText("the new surface's name");
    nameFields->addRow("New surface:", name_);
    layout->addLayout(nameFields);
    command_ = terrainCommandField(this, "surfaceCombineCommand");
    layout->addWidget(command_);
    auto* buttons = new QHBoxLayout();
    auto* preview = preview_ = new QPushButton("Preview", this);
    preview->setObjectName("surfaceCombinePreview");
    preview->setToolTip("Run the line with PREVIEW: what it would make, nothing changed");
    auto* run = run_ = new QPushButton("Run", this);
    run->setObjectName("surfaceCombineRun");
    run->setDefault(true);
    auto* close = new QPushButton("Close", this);
    close->setObjectName("surfaceCombineClose");
    buttons->addWidget(preview);
    buttons->addStretch(1);
    buttons->addWidget(run);
    buttons->addWidget(close);
    layout->addLayout(buttons);
    reply_ = terrainReplyField(this, "surfaceCombineReply");
    layout->addWidget(reply_);

    const auto changed = [this] { refreshCommand(); };
    connect(mode_, &QComboBox::currentIndexChanged, this, [this](int index) {
        setMode(static_cast<SurfaceCombineMode>(index));
    });
    connect(mergeList_, &QListWidget::itemChanged, this, changed);
    connect(up, &QPushButton::clicked, this, [this] { move(-1); });
    connect(down, &QPushButton::clicked, this, [this] { move(1); });
    for (QComboBox* combo : {top_, base_, source_}) {
        connect(combo, &QComboBox::currentIndexChanged, this, changed);
    }
    connect(dz_, &QLineEdit::textChanged, this, changed);
    connect(name_, &QLineEdit::textChanged, this, changed);
    scope_->onChanged = changed;
    connect(preview, &QPushButton::clicked, this, [this] { runLine(true); });
    connect(run, &QPushButton::clicked, this, [this] { runLine(false); });
    connect(close, &QPushButton::clicked, this, &QDialog::close);
    setMode(SurfaceCombineMode::Merge);
    reload();
    if (context_.document != nullptr) {
        // As SurfaceLevelsDialog's: the lists follow the drawing.
        watcher_ = std::make_unique<DocumentWatcher>(*context_.document,
                                                     [this](const DocumentChanges& changes) {
                                                         if (changes.model) {
                                                             reload();
                                                         }
                                                     });
    }
}

SurfaceCombineDialog::~SurfaceCombineDialog() = default;

void SurfaceCombineDialog::setMode(SurfaceCombineMode mode)
{
    const int index = static_cast<int>(mode);
    if (mode_->currentIndex() != index) {
        mode_->setCurrentIndex(index); // comes back here through the signal
        return;
    }
    pages_->setCurrentIndex(mode == SurfaceCombineMode::Merge   ? 0
                            : mode == SurfaceCombineMode::Paste ? 1
                                                                : 2);
    // The stack is as tall as the page shown, not as the tallest: a stacked
    // layout keeps room for every page unless it is Ignored (as Volumes' method
    // tabs are), and Clip's page carries the scope controls, which left Merge a
    // list some 600 px tall.
    for (int page = 0; page < pages_->count(); ++page) {
        pages_->widget(page)->setSizePolicy(
            QSizePolicy::Preferred,
            page == pages_->currentIndex() ? QSizePolicy::Preferred : QSizePolicy::Ignored);
    }
    pages_->updateGeometry();
    dz_->setEnabled(mode == SurfaceCombineMode::Offset);
    scope_->setEnabled(mode == SurfaceCombineMode::Clip);
    name_->setPlaceholderText(mode == SurfaceCombineMode::Paste
                                  ? "blank: paste into the base in place"
                                  : "the new surface's name");
    refreshCommand();
}

void SurfaceCombineDialog::reload()
{
    const QStringList names = surfaceNames(context_.document);
    // The merge list keeps its order and ticks for the names still there.
    QStringList ordered;
    std::vector<bool> ticked;
    for (int i = 0; i < mergeList_->count(); ++i) {
        const QString name = mergeList_->item(i)->text();
        if (names.contains(name)) {
            ordered << name;
            ticked.push_back(mergeList_->item(i)->checkState() == Qt::Checked);
        }
    }
    for (const QString& name : names) {
        if (!ordered.contains(name)) {
            ordered << name;
            ticked.push_back(false);
        }
    }
    mergeList_->blockSignals(true);
    mergeList_->clear();
    for (int i = 0; i < ordered.size(); ++i) {
        auto* item = new QListWidgetItem(ordered[i], mergeList_);
        item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
        item->setCheckState(ticked[static_cast<std::size_t>(i)] ? Qt::Checked : Qt::Unchecked);
    }
    mergeList_->blockSignals(false);
    for (QComboBox* combo : {top_, base_, source_}) {
        fillNames(*combo, names);
    }
    if (base_->currentIndex() == top_->currentIndex() && base_->count() > 1) {
        base_->setCurrentIndex(top_->currentIndex() == 0 ? 1 : 0);
    }
    if (context_.document != nullptr) {
        scope_->reload(context_.document->model());
    }
    refreshCommand();
}

void SurfaceCombineDialog::move(int by)
{
    const int row = mergeList_->currentRow();
    const int to = row + by;
    if (row < 0 || to < 0 || to >= mergeList_->count()) {
        return;
    }
    QListWidgetItem* item = mergeList_->takeItem(row);
    mergeList_->insertItem(to, item);
    mergeList_->setCurrentRow(to);
    refreshCommand();
}

SurfaceCombineForm SurfaceCombineDialog::form() const
{
    SurfaceCombineForm form;
    form.mode = static_cast<SurfaceCombineMode>(mode_->currentIndex());
    for (int i = 0; i < mergeList_->count(); ++i) {
        if (mergeList_->item(i)->checkState() == Qt::Checked) {
            form.merge << mergeList_->item(i)->text();
        }
    }
    form.top = top_->currentText();
    form.base = base_->currentText();
    form.surface = source_->currentText();
    form.dz = dz_->text();
    form.name = name_->text();
    if (form.mode == SurfaceCombineMode::Clip) {
        const auto words = scope_->verbWords();
        if (words) {
            form.scope = *words;
        } else {
            form.scopeError = QString::fromStdString(words.error().message);
        }
    }
    return form;
}

Result<QString> SurfaceCombineDialog::command() const { return surfaceCombineLine(form()); }

void SurfaceCombineDialog::showEvent(QShowEvent* event)
{
    reload();
    QDialog::showEvent(event);
}

void SurfaceCombineDialog::refreshCommand()
{
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : QString::fromStdString(line.error().message));
    // Nothing to run until the fields say a line; the placeholder says why.
    if (run_ != nullptr) {
        run_->setEnabled(line.ok());
        preview_->setEnabled(line.ok());
    }
}

void SurfaceCombineDialog::runLine(bool preview)
{
    const auto line = command();
    if (!line) {
        reply_->setPlainText(QString::fromStdString(line.error().message));
        return;
    }
    if (!context_.run) {
        reply_->setPlainText("There is no window to run the line in.");
        return;
    }
    showOutcome(*reply_, context_.run(preview ? *line + " PREVIEW" : *line));
    if (!preview) {
        reload();
    }
}

} // namespace katana::qt
