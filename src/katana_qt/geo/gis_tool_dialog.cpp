// The frame of the GIS analysis and check dialogs (gis_tool_dialog.hpp).

#include "geo/gis_tool_dialog.hpp"

#include <QAction>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QHeaderView>
#include <QTableWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <optional>
#include <string>
#include <utility>

#include "customisation/document_watcher.hpp"
#include "geo/geo_workbench.hpp"
#include "geo/replies.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

// The value of `key` in the reply's first record of `kind`, or empty: read
// by the one record reader (geo::parseRecords), which undoes a quoted
// value's escapes, where a pattern of this file's own did not.
QString field(const QString& reply, const QString& kind, const QString& key)
{
    for (const katana::app::geo::Record& record :
         katana::app::geo::parseRecords(reply.toStdString())) {
        if (record.kind == kind.toStdString()) {
            return QString::fromStdString(record.get(key.toStdString()).value_or(std::string()));
        }
    }
    return {};
}

} // namespace

Result<QString> gisWord(const QString& text, const QString& field)
{
    // The one quoting rule (geo::lineWord): a keyword as a name is quoted too.
    const auto word = katana::app::geo::lineWord(text.toStdString());
    if (!word) {
        return invalid(field + " holds a double quote or a line break, which a command line "
                               "cannot carry");
    }
    return QString::fromStdString(*word);
}

Result<QString> gisScope(const GisScopeWords& scope)
{
    if (!scope.error.isEmpty()) {
        return invalid("the scope: " + scope.error);
    }
    if (scope.words.trimmed().isEmpty()) {
        return invalid("choose what in the drawing to act on first");
    }
    return scope.words.trimmed();
}

Result<QString> gisNumberOption(const QString& key, const QString& text, const QString& field)
{
    const QString number = text.trimmed();
    if (number.isEmpty()) {
        return QString();
    }
    if (!katana::core::parseFiniteDouble(number.toStdString())) {
        return invalid(field + " must be a number, not '" + number + "'");
    }
    return " " + key + "=" + number;
}

Result<QString> gisLayerTarget(const QString& layer)
{
    const QString name = layer.trimmed();
    if (name.isEmpty()) {
        return QString();
    }
    auto word = gisWord(name, "The layer");
    if (!word) {
        return word.error();
    }
    return " TO LAYER " + *word;
}

GisToolDialog::GisToolDialog(const QString& name, const QString& title, GisDialogContext context,
                             QWidget* parent, bool scoped)
    : QDialog(parent), name_(name), context_(std::move(context))
{
    setObjectName(name + "Dialog");
    setWindowTitle(title);
    // Non-modal: kept open beside the drawing its lines change.
    setModal(false);
    resize(scoped ? 980 : 720, 700);

    auto* fieldsBox = new QWidget(this);
    fields_ = new QFormLayout(fieldsBox);
    if (scoped) {
        scope_ = new ScopeFilterWidget(name, this);
        scope_->views = context_.views;
        // What an analysis usually starts from: what is selected, as MODIFY
        // reads a line with no scope word.
        scope_->onChanged = [this] { refreshCommand(); };
    }

    command_ = new QLineEdit(this);
    command_->setObjectName(name + "Command");
    command_->setReadOnly(true);
    command_->setFont(katana::qt::theme::monospaceFont());
    command_->setToolTip("The line Run hands to the command line - type it there, or give it to "
                         "katana_cli or katana_mcp, and it does the same");
    preview_ = new QPushButton("Preview", this);
    preview_->setObjectName(name + "Preview");
    preview_->setAutoDefault(false);
    preview_->setToolTip("Run the line with PREVIEW: what the scope takes, and nothing changed");
    run_ = new QPushButton("Run", this);
    run_->setObjectName(name + "Run");
    run_->setDefault(true);
    status_ = new QLabel(this);
    status_->setObjectName(name + "Status");
    status_->setWordWrap(true);
    status_->setTextFormat(Qt::PlainText); // a refusal's words, never markup
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    reply_ = new QPlainTextEdit(this);
    reply_->setObjectName(name + "Reply");
    reply_->setReadOnly(true);
    reply_->setFont(katana::qt::theme::monospaceFont());
    // Records are one to a line: wrapping would break them.
    reply_->setLineWrapMode(QPlainTextEdit::NoWrap);
    reply_->setPlaceholderText("The reply appears here and in the command log.");
    reply_->setMinimumHeight(140);
    auto* close = new QPushButton("Close", this);
    close->setObjectName(name + "Close");
    close->setAutoDefault(false);

    auto* middle = new QHBoxLayout;
    if (scope_ != nullptr) {
        middle->addWidget(scope_, 2);
    }
    right_ = new QVBoxLayout;
    right_->addWidget(fieldsBox);
    right_->addStretch(1);
    middle->addLayout(right_, 3);
    auto* commandForm = new QFormLayout;
    commandForm->addRow("Command:", command_);
    auto* runRow = new QHBoxLayout;
    runRow->addWidget(status_, 1);
    runRow->addWidget(preview_);
    runRow->addWidget(run_);
    auto* buttons = new QHBoxLayout;
    buttons->addStretch(1);
    buttons->addWidget(close);
    auto* layout = new QVBoxLayout(this);
    layout->addLayout(middle, 3);
    layout->addLayout(commandForm);
    layout->addLayout(runRow);
    layout->addWidget(reply_, 2);
    layout->addLayout(buttons);

    connect(preview_, &QPushButton::clicked, this, [this] { preview(); });
    connect(run_, &QPushButton::clicked, this, [this] { run(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    if (context_.document != nullptr) {
        // The layers follow the drawing: a result adds its layer, an undo
        // takes it away.
        watcher_ = std::make_unique<DocumentWatcher>(*context_.document,
                                                     [this](const DocumentChanges& changes) {
                                                         if (changes.model) {
                                                             reload();
                                                         }
                                                     });
    }
}

GisToolDialog::~GisToolDialog() = default;

bool GisToolDialog::documentAlive() const
{
    return context_.document != nullptr && watcher_ != nullptr && watcher_->documentAlive();
}

void GisToolDialog::showEvent(QShowEvent* event)
{
    // The layers and views there are now, not when the dialog was made.
    reload();
    QDialog::showEvent(event);
}

void GisToolDialog::reload()
{
    if (scope_ != nullptr) {
        if (documentAlive()) {
            scope_->reload(context_.document->model());
        } else {
            scope_->reloadViews();
        }
    }
    refreshCommand();
}

QLineEdit* GisToolDialog::addField(const QString& name, const QString& label,
                                   const QString& placeholder, const QString& tip)
{
    auto* edit = new QLineEdit(this);
    edit->setObjectName(name);
    edit->setPlaceholderText(placeholder);
    edit->setToolTip(tip);
    fields_->addRow(label, edit);
    connect(edit, &QLineEdit::textChanged, this, [this] { refreshCommand(); });
    return edit;
}

GisScopeWords GisToolDialog::scopeWords() const
{
    GisScopeWords words;
    if (scope_ == nullptr) {
        return words;
    }
    if (const auto said = scope_->verbWords()) {
        words.words = *said;
    } else {
        words.error = QString::fromStdString(said.error().message);
    }
    return words;
}

void GisToolDialog::refreshCommand()
{
    if (command_ == nullptr) {
        return; // a field made before the frame's own
    }
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(
        line ? QString() : "nothing to run yet: " + QString::fromStdString(line.error().message));
    run_->setEnabled(line.ok());
    preview_->setEnabled(line.ok());
}

void GisToolDialog::addProblemsTable()
{
    problems_ = new QTableWidget(0, 5, this);
    problems_->setObjectName(name_ + "Problems");
    problems_->setHorizontalHeaderLabels({"Kind", "Entity", "At", "Length", "Reason"});
    problems_->horizontalHeader()->setStretchLastSection(true);
    problems_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    problems_->setSelectionBehavior(QAbstractItemView::SelectRows);
    problems_->setToolTip("The problem records of the last reply, one a row");
    problems_->setMinimumHeight(160);
    // Above the stretch, under the fields.
    right_->insertWidget(right_->count() - 1, problems_, 1);
}

QString GisToolDialog::replyText() const
{
    return reply_->toPlainText();
}

QString GisToolDialog::statusText() const
{
    return status_->text();
}

void GisToolDialog::run()
{
    const auto line = command();
    if (!line) {
        setStatus(QString::fromStdString(line.error().message), true);
        return;
    }
    runLine(*line);
}

void GisToolDialog::preview()
{
    const auto line = command();
    if (!line) {
        setStatus(QString::fromStdString(line.error().message), true);
        return;
    }
    runLine(*line + " PREVIEW");
}

void GisToolDialog::runLine(const QString& line)
{
    if (!context_.run) {
        setStatus("nothing here can run a command", true);
        return;
    }
    pending_ = kNoJob;
    const VerbOutcome outcome = context_.run(line);
    // Interactively the line became a job: its reply comes when it ends.
    if (const std::optional<JobId> started =
            outcome.ok ? startedJob(outcome.reply) : std::nullopt) {
        pending_ = *started;
        reply_->setPlainText(outcome.reply.trimmed());
        setStatus("Running as a background job: the reply appears here when it ends. Cancel "
                  "is in the status bar.",
                  false);
        return;
    }
    showOutcome(outcome);
}

void GisToolDialog::jobFinished(JobId id, const VerbOutcome& outcome)
{
    if (id == kNoJob || id != pending_) {
        return;
    }
    pending_ = kNoJob;
    showOutcome(outcome);
}

void GisToolDialog::showOutcome(const VerbOutcome& outcome)
{
    QString text = outcome.ok ? outcome.reply : outcome.error;
    if (!outcome.ok && !outcome.reply.isEmpty()) {
        text = outcome.reply + "\n" + outcome.error;
    }
    while (text.endsWith('\n')) {
        text.chop(1);
    }
    reply_->setPlainText(text);
    if (problems_ != nullptr) {
        problems_->setRowCount(0);
        for (const auto& record : katana::app::geo::parseRecords(outcome.reply.toStdString())) {
            if (record.kind != "problem") {
                continue;
            }
            const int row = problems_->rowCount();
            problems_->insertRow(row);
            int column = 0;
            for (const char* key : {"kind", "entity", "at", "length", "reason"}) {
                problems_->setItem(row, column++,
                                   new QTableWidgetItem(QString::fromStdString(
                                       record.get(key).value_or(std::string()))));
            }
        }
    }
    if (!outcome.ok) {
        setStatus(outcome.error.section('\n', 0, 0), true);
        replied(QString());
        return;
    }
    setStatus(summary(outcome.reply), false);
    replied(outcome.reply);
}

QString GisToolDialog::summary(const QString& reply) const
{
    if (reply.contains(" preview=yes")) {
        return "Preview: nothing changed. What the scope takes is below.";
    }
    if (reply.contains(" ran=no")) {
        return "The scope took nothing: nothing ran, and nothing changed.";
    }
    const QString created = field(reply, "output", "created");
    const QString updated = field(reply, "output", "updated");
    const QString deleted = field(reply, "output", "deleted");
    if (!created.isEmpty()) {
        const bool changed = created != "0" || updated != "0" || deleted != "0";
        if (!changed) {
            return "Done: nothing to draw, and nothing was added to the undo history.";
        }
        // An in-place reply (target=in-place: REPLACE, REPAIR, COVERAGE
        // CLEAN) names no layer; the status said "... deleted on .".
        const QString layer = field(reply, "output", "layer");
        return QString("Done as one undo step: %1 created, %2 changed, %3 deleted%4.")
            .arg(created, updated, deleted,
                 layer.isEmpty() ? QString(", in place") : " on " + layer);
    }
    return "Done. The reply is below and in the command log.";
}

void GisToolDialog::setStatus(const QString& text, bool isError)
{
    status_->setText(text);
    status_->setStyleSheet(isError ? "color: " + theme::error().name() + ";" : QString());
}

// ---- the menu items ----------------------------------------------------------------------------

void addGisToolAction(GeoMenus& menus, GeoWorkbench& workbench, const QString& section, Icon icon,
                      const QString& text, const QString& tip, const QString& name,
                      GisDialogMaker make)
{
    const GeoServices& services = workbench.services();
    if (!services.makeAction) {
        return;
    }
    QAction* action = services.makeAction(icon, text, tip, QKeySequence(), name);
    // The dialog it opens, by object name: how --dialog finds it.
    action->setData(name + "Dialog");
    menus.addToGis(section, action);
    GeoWorkbench* bench = &workbench;
    QObject::connect(action, &QAction::triggered, action, [action, bench, name, make] {
        auto* window = qobject_cast<QWidget*>(action->parent());
        GisToolDialog* dialog = nullptr;
        if (window != nullptr) {
            dialog = dynamic_cast<GisToolDialog*>(window->findChild<QDialog*>(name + "Dialog"));
        }
        if (dialog == nullptr) {
            const GeoServices& from = bench->services();
            GisDialogContext context;
            context.run = from.run;
            context.document = from.document;
            context.headless = from.headless;
            // The workspace's views, asked afresh each time the scope's
            // View choice is filled, as every workbench of the window does.
            if (from.views != nullptr) {
                ViewWorkspace* views = from.views;
                context.views = [views] { return scopeFilterViews(views->viewSet()); };
            }
            dialog = make(std::move(context), window);
            QPointer<GisToolDialog> guard(dialog);
            (void)bench->addFinishedListener([guard](JobId id, const VerbOutcome& outcome) {
                if (guard != nullptr) {
                    guard->jobFinished(id, outcome);
                }
            });
        }
        dialog->reload();
        dialog->show();
        dialog->raise();
        dialog->activateWindow();
    });
}

} // namespace katana::qt
