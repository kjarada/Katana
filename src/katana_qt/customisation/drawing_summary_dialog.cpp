#include "customisation/drawing_summary_dialog.hpp"

#include <string>

#include <QApplication>
#include <QClipboard>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTextOption>
#include <QVBoxLayout>

#include "customisation/document_watcher.hpp"
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/document_status.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

QString counted(std::size_t count, const char* one, const char* many)
{
    return QString("%1 %2").arg(count).arg(count == 1 ? one : many);
}

QLabel* valueLabel(const char* name, QWidget* parent)
{
    auto* label = new QLabel(parent);
    label->setObjectName(name);
    label->setWordWrap(true);
    label->setTextInteractionFlags(Qt::TextSelectableByMouse);
    return label;
}

QPushButton* button(const char* name, const QString& text, const QString& tip, QWidget* parent)
{
    auto* made = new QPushButton(text, parent);
    made->setObjectName(name);
    made->setToolTip(tip);
    return made;
}

} // namespace

DrawingSummaryDialog::DrawingSummaryDialog(DrawingSummaryContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("drawingSummaryDialog");
    setWindowTitle("Drawing Summary");
    setModal(false);
    resize(640, 600);

    project_ = valueLabel("drawingSummaryProject", this);
    drawing_ = valueLabel("drawingSummaryDrawing", this);
    current_ = valueLabel("drawingSummaryCurrent", this);
    selection_ = valueLabel("drawingSummarySelection", this);
    history_ = valueLabel("drawingSummaryHistory", this);
    auto* form = new QFormLayout;
    form->addRow("Project:", project_);
    form->addRow("Drawing:", drawing_);
    form->addRow("Current:", current_);
    form->addRow("Selection:", selection_);
    form->addRow("History:", history_);

    customisation_ = new QPlainTextEdit(this);
    customisation_->setObjectName("drawingSummaryCustomisation");
    customisation_->setReadOnly(true);
    customisation_->setFont(katana::qt::theme::monospaceFont());
    // Wrapped at the pane's edge, between words where it can: the reply's
    // records are longer than the pane is wide (the linework record is 143
    // characters), and left unwrapped the end of each lay out of sight
    // behind a scroll bar. A record still begins its own line and starts with
    // its word, so a wrapped one reads as one.
    customisation_->setLineWrapMode(QPlainTextEdit::WidgetWidth);
    customisation_->setWordWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
    customisation_->setMinimumHeight(150);

    auto* unresolvedLabel =
        new QLabel("Names the drawing's styles give that no loaded library defines - click one "
                   "to see its styles among the Missing in Styles and Linetypes:",
                   this);
    unresolvedLabel->setWordWrap(true);
    unresolved_ = new QListWidget(this);
    unresolved_->setObjectName("drawingSummaryUnresolved");
    unresolved_->setMaximumHeight(120);

    auto* showMissing =
        button("drawingSummaryShowMissing", "Show in Styles and Linetypes",
               "Open Format > Styles and Linetypes on its Missing styles, searching for the "
               "chosen name",
               this);
    showMissing->setEnabled(false);
    auto* copy = button("drawingSummaryCopyJson", "Copy as JSON",
                        "Copy the drawing's state as STATUS JSON gives it", this);
    auto* refreshButton =
        button("drawingSummaryRefresh", "Refresh", "Read the drawing's state again", this);
    auto* close = button("drawingSummaryClose", "Close", "Close the summary", this);
    status_ = valueLabel("drawingSummaryStatus", this);
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(copy);
    buttons->addWidget(refreshButton);
    buttons->addStretch(1);
    buttons->addWidget(close);

    auto* layout = new QVBoxLayout(this);
    layout->addLayout(form);
    // Says where the customisation is changed: the summary only shows it.
    auto* customisationLabel =
        new QLabel("Customisation (File > Settings loads, writes or resets it):", this);
    customisationLabel->setObjectName("drawingSummaryCustomisationLabel");
    layout->addWidget(customisationLabel);
    layout->addWidget(customisation_, 1);
    layout->addWidget(unresolvedLabel);
    layout->addWidget(unresolved_);
    auto* missingRow = new QHBoxLayout;
    missingRow->addStretch(1);
    missingRow->addWidget(showMissing);
    layout->addLayout(missingRow);
    layout->addWidget(status_);
    layout->addLayout(buttons);

    connect(copy, &QPushButton::clicked, this, [this] { (void)copyJson(); });
    connect(refreshButton, &QPushButton::clicked, this, [this] {
        refresh();
        status_->setText("Read again.");
    });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    // A double-click on a name, or the button on the chosen one - which is
    // how the headless driver, choosing a row with --fill, reaches it too.
    const auto show = [this](const QListWidgetItem* item) {
        if (item != nullptr && context_.showMissing) {
            context_.showMissing(item->text());
            status_->setText("Styles and Linetypes shows the styles that name " + item->text() +
                             ".");
        }
    };
    connect(unresolved_, &QListWidget::itemActivated, this, show);
    connect(showMissing, &QPushButton::clicked, this,
            [this, show] { show(unresolved_->currentItem()); });
    connect(unresolved_, &QListWidget::currentItemChanged, this,
            [showMissing](const QListWidgetItem* item) { showMissing->setEnabled(item != nullptr); });

    if (context_.document != nullptr) {
        watcher_ = std::make_unique<DocumentWatcher>(*context_.document,
                                                     [this](const DocumentChanges&) { refresh(); });
    }
    refresh();
}

DrawingSummaryDialog::~DrawingSummaryDialog() = default;

void DrawingSummaryDialog::refresh()
{
    if (context_.document == nullptr || (watcher_ != nullptr && !watcher_->documentAlive())) {
        return;
    }
    const katana::cad::Document& document = *context_.document;
    const katana::cad::DocumentStatus status = katana::cad::documentStatus(document);

    project_->setText(
        (status.project ? qs(*status.project) : QString("none - not saved to a project yet")) +
        (status.modified ? QString(", with unsaved changes") : QString(", saved")));
    drawing_->setText(counted(status.entities, "entity", "entities") + ", " +
                      counted(status.layers, "layer", "layers") + ", " +
                      counted(status.alignments, "alignment", "alignments") + ", " +
                      counted(document.sheetSet().sheets.size(), "sheet", "sheets"));
    current_->setText(
        "layer " + qs(status.currentLayer) + ", style " +
        (status.currentStyle.empty() ? QString("ByLayer") : qs(status.currentStyle)) +
        QString(", annotation scale 1:%1").arg(document.annotationScale(), 0, 'g', 10) + ", " +
        (context_.crs ? context_.crs()
                      : document.metadata().coordinateSystem.empty()
                          ? QString("no coordinate system")
                          : qs(document.metadata().coordinateSystem)));
    selection_->setText(status.selected == 0
                            ? QString("nothing selected")
                            : counted(status.selected, "entity", "entities") + " selected");
    const auto next = [](std::string_view name) {
        return name.empty() ? QString() : QString(" (next: %1)").arg(qs(name));
    };
    history_->setText(counted(status.undoSteps, "step", "steps") + " to undo" +
                      next(document.history().undoName()) + ", " +
                      counted(status.redoSteps, "step", "steps") + " to redo" +
                      next(document.history().redoName()));

    // What a bare CUSTOMISE replies, word for word, of the Document's own
    // state: the pane and the command line cannot come to say it differently.
    const katana::cad::CustomisationSummary summary = katana::cad::customisationSummary(document);
    QString report = qs(katana::cad::formatCustomisationReply(summary));
    while (report.endsWith('\n')) {
        report.chop(1);
    }
    customisation_->setPlainText(report);
    unresolved_->clear();
    for (const std::string& name : summary.coverage.unresolved) {
        unresolved_->addItem(qs(name));
    }
}

QString DrawingSummaryDialog::copyJson()
{
    if (!context_.run) {
        status_->setText("Nothing here can run a command.");
        return {};
    }
    // Through the one executor, as typed: what is copied is what an agent
    // reads from STATUS JSON, katana_status or katana://status.
    const VerbOutcome outcome = context_.run("STATUS JSON");
    if (!outcome.ok) {
        status_->setText("STATUS JSON was refused: " + outcome.error);
        return {};
    }
    QApplication::clipboard()->setText(outcome.reply);
    status_->setText("Copied the drawing's state as JSON (STATUS JSON).");
    return outcome.reply;
}

} // namespace katana::qt
