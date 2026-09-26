// What the geoprocessing dialogs share (geo_dialog_support.hpp).

#include "geo/geo_dialog_support.hpp"

#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QVBoxLayout>

#include <algorithm>
#include <utility>

#include "geo/geo_workbench.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

GeoDialogContext geoDialogContext(GeoWorkbench& workbench)
{
    const GeoServices& services = workbench.services();
    GeoDialogContext context;
    context.run = services.run;
    context.headless = services.headless;
    context.document = services.document;
    context.reference = services.reference;
    context.surfaces = services.surfaces;
    if (services.views != nullptr) {
        ViewWorkspace* views = services.views;
        context.views = [views] { return scopeFilterViews(views->viewSet()); };
    }
    GeoWorkbench* bench = &workbench;
    context.listen = [bench](std::function<void(JobId, const VerbOutcome&)> listener) {
        // The listener stays with the workbench, and goes quiet when the
        // token does: the workbench is the window's and may go first, so the
        // token never reaches back into it.
        auto alive = std::make_shared<int>(0);
        std::weak_ptr<int> weak = alive;
        bench->addFinishedListener(
            [weak, listener = std::move(listener)](JobId id, const VerbOutcome& outcome) {
                if (weak.lock()) {
                    listener(id, outcome);
                }
            });
        return std::shared_ptr<void>(std::move(alive));
    };
    return context;
}

Result<QString> lineWord(const QString& text, const QString& field)
{
    if (text.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, (field + " is empty").toStdString());
    }
    if (text.contains('"')) {
        return makeError(ErrorCode::InvalidArgument,
                         (field + " holds a double quote, which a command line cannot carry")
                             .toStdString());
    }
    const bool blank = std::any_of(text.begin(), text.end(), [](QChar c) { return c.isSpace(); });
    return blank ? "\"" + text + "\"" : text;
}

std::optional<JobId> startedJob(const QString& reply)
{
    static const QRegularExpression started(
        QStringLiteral("^job id=(\\d+) title=\".*\" state=started$"),
        QRegularExpression::MultilineOption);
    const QRegularExpressionMatch match = started.match(reply);
    if (!match.hasMatch()) {
        return std::nullopt;
    }
    return match.captured(1).toULongLong();
}

QString firstRecord(const QString& reply, const QString& kind)
{
    for (const QString& record : reply.split('\n')) {
        if (record.startsWith(kind + " ") || record == kind) {
            return record;
        }
    }
    return {};
}

GeoRunPanel::GeoRunPanel(const QString& prefix, GeoDialogContext context, bool preview,
                         QWidget* parent)
    : QWidget(parent), context_(std::move(context))
{
    setObjectName(prefix + "Panel");
    command_ = new QLineEdit(this);
    command_->setObjectName(prefix + "Command");
    command_->setReadOnly(true);
    command_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    command_->setToolTip("The line Run hands to the command line - type it there, give it to "
                         "katana_cli or send it to katana_mcp, and it does the same");
    if (preview) {
        preview_ = new QPushButton("Preview", this);
        preview_->setObjectName(prefix + "Preview");
        preview_->setAutoDefault(false);
        preview_->setToolTip("Run the line with PREVIEW: what it would take, checked by GDAL, "
                             "and nothing changed");
    }
    run_ = new QPushButton("Run", this);
    run_->setObjectName(prefix + "Run");
    run_->setAutoDefault(false);
    status_ = new QLabel(this);
    status_->setObjectName(prefix + "Status");
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    reply_ = new QPlainTextEdit(this);
    reply_->setObjectName(prefix + "Reply");
    reply_->setReadOnly(true);
    reply_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    // Records are one to a line; wrapping would make one look like two.
    reply_->setLineWrapMode(QPlainTextEdit::NoWrap);
    reply_->setPlaceholderText("The reply appears here and in the command log.");
    reply_->setMinimumHeight(110);

    auto* commandRow = new QFormLayout;
    commandRow->addRow("Command:", command_);
    auto* runRow = new QHBoxLayout;
    runRow->addWidget(status_, 1);
    if (preview_ != nullptr) {
        runRow->addWidget(preview_);
    }
    runRow->addWidget(run_);
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->addLayout(commandRow);
    layout->addLayout(runRow);
    layout->addWidget(reply_, 1);

    connect(run_, &QPushButton::clicked, this, [this] { run(false); });
    if (preview_ != nullptr) {
        connect(preview_, &QPushButton::clicked, this, [this] { run(true); });
    }
    if (context_.listen) {
        listening_ = context_.listen(
            [this](JobId id, const VerbOutcome& outcome) { heard(id, outcome); });
    }
}

GeoRunPanel::~GeoRunPanel() = default;

void GeoRunPanel::refresh()
{
    const Result<QString> made = line ? line() : Result<QString>(QString());
    const bool ready = made.ok() && !made->isEmpty();
    command_->setText(ready ? *made : QString());
    command_->setPlaceholderText(
        ready ? QString()
              : "nothing to run yet: " +
                    (made.ok() ? QString("the fields say no line")
                               : QString::fromStdString(made.error().message)));
    run_->setEnabled(ready);
    if (preview_ != nullptr) {
        preview_->setEnabled(ready);
    }
}

void GeoRunPanel::run(bool preview)
{
    const Result<QString> made = line ? line() : Result<QString>(QString());
    if (!made || made->isEmpty()) {
        setStatus(made ? QString("There is nothing to run yet.")
                       : QString::fromStdString(made.error().message),
                  true);
        return;
    }
    if (!context_.run) {
        setStatus("Nothing here can run a command.", true);
        return;
    }
    const QString text = preview ? *made + " PREVIEW" : *made;
    handing_ = true;
    early_.clear();
    const VerbOutcome outcome = context_.run(text);
    handing_ = false;
    const std::optional<JobId> job = outcome.ok ? startedJob(outcome.reply) : std::nullopt;
    if (!job) {
        // Answered at once - a PREVIEW, a refusal - or a headless run, whose
        // job ended inside the call.
        early_.clear();
        finish(outcome);
        return;
    }
    if (const auto heardAlready = early_.find(*job); heardAlready != early_.end()) {
        const VerbOutcome done = heardAlready->second;
        early_.clear();
        finish(done);
        return;
    }
    early_.clear();
    pending_ = *job;
    setStatus(QString("Running as job %1 - its progress and Cancel are in the status bar.")
                  .arg(*job));
}

void GeoRunPanel::heard(JobId id, const VerbOutcome& outcome)
{
    if (handing_) {
        early_[id] = outcome;
        return;
    }
    if (id != kNoJob && id == pending_) {
        pending_ = kNoJob;
        finish(outcome);
    }
}

void GeoRunPanel::finish(const VerbOutcome& outcome)
{
    QString text = outcome.ok ? outcome.reply : outcome.error;
    if (!outcome.ok && !outcome.reply.isEmpty()) {
        text = outcome.reply + "\n" + outcome.error;
    }
    while (text.endsWith('\n')) {
        text.chop(1);
    }
    reply_->setPlainText(text);
    if (!outcome.ok) {
        setStatus(outcome.error.section('\n', 0, 0), true);
    } else if (firstRecord(outcome.reply, "preview").contains("valid=yes")) {
        setStatus("Previewed: GDAL accepts the line, and nothing was changed.");
    } else if (outcome.reply.contains(" ran=no")) {
        setStatus("Nothing ran: the scope took nothing to work on (the reply says what it "
                  "took).");
    } else {
        // Said from the reply's own output record: a raster kept as a
        // reference raster is no step of the drawing's to undo.
        const QString output = firstRecord(outcome.reply, "output");
        if (output.contains(" target=layer ")) {
            setStatus("Done, as one undo step - Undo takes it back. The reply is below and in "
                      "the command log.");
        } else if (output.contains(" target=reference ")) {
            setStatus("Done: the result is a reference raster (REFS lists it). The reply is "
                      "below and in the command log.");
        } else if (output.contains(" target=file ")) {
            setStatus("Written. The reply is below and in the command log.");
        } else {
            setStatus("Done. The reply is below and in the command log.");
        }
    }
    if (onFinished) {
        onFinished(outcome);
    }
}

QString GeoRunPanel::reply() const
{
    return reply_->toPlainText();
}

QString GeoRunPanel::status() const
{
    return status_->text();
}

void GeoRunPanel::setStatus(const QString& text, bool isError)
{
    status_->setText(text);
    status_->setStyleSheet(isError ? "color: " + theme::error().name() + ";" : QString());
}

} // namespace katana::qt
