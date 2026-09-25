#include "script_runner.hpp"

#include <QCheckBox>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QVBoxLayout>

#include "katana/cad/command_interpreter.hpp"

namespace katana::qt {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

constexpr const char* kUsage = "usage: SCRIPT <file.kcs> [CONTINUE]";

// QUIT and EXIT as katana_cli reads them (Session::isQuit): the first word,
// whatever follows it.
bool isQuit(const QString& line)
{
    const QString verb = line.section(' ', 0, 0, QString::SectionSkipEmpty).toUpper();
    return verb == "QUIT" || verb == "EXIT";
}

} // namespace

bool isScriptComment(const QString& line)
{
    return line.trimmed().startsWith('#');
}

std::vector<ScriptLine> scriptLines(const QString& text)
{
    QString unified = text;
    unified.replace("\r\n", "\n").replace('\r', '\n');
    std::vector<ScriptLine> lines;
    const QStringList raw = unified.split('\n');
    for (qsizetype at = 0; at < raw.size(); ++at) {
        const QString line = raw[at].trimmed();
        if (line.isEmpty() || isScriptComment(line)) {
            continue;
        }
        lines.push_back({static_cast<int>(at + 1), line});
    }
    return lines;
}

Result<std::vector<ScriptLine>> readScript(const QString& path)
{
    if (!QFileInfo::exists(path)) {
        return makeError(ErrorCode::NotFound, "no such script", path.toStdString());
    }
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        return makeError(ErrorCode::FileImportFailure, "the script cannot be read",
                         path.toStdString() + ": " + file.errorString().toStdString());
    }
    return scriptLines(QString::fromUtf8(file.readAll()));
}

Result<ScriptCommand> parseScriptCommand(const QString& line)
{
    const auto tokens = katana::cad::CommandInterpreter::tokenize(line.toStdString());
    if (!tokens) {
        return tokens.error();
    }
    const std::size_t count = tokens->size();
    if (count < 2 || count > 3 || (*tokens)[1].empty()) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    ScriptCommand command;
    command.path = QString::fromStdString((*tokens)[1]);
    if (count == 3) {
        if (QString::fromStdString((*tokens)[2]).toUpper() != "CONTINUE") {
            return makeError(ErrorCode::InvalidArgument, kUsage, (*tokens)[2]);
        }
        command.continueOnError = true;
    }
    return command;
}

QString scriptCommandLine(const QString& path, bool continueOnError)
{
    return QString("SCRIPT \"%1\"%2")
        .arg(QDir::fromNativeSeparators(path), continueOnError ? " CONTINUE" : "");
}

ScriptReport runScriptLines(std::span<const ScriptLine> lines, const CommandRunner& run,
                            const ScriptOptions& options)
{
    ScriptReport report;
    report.lines = static_cast<int>(lines.size());
    for (const ScriptLine& line : lines) {
        if (options.progress && !options.progress(report.ran, report.lines)) {
            report.cancelled = true;
            report.stoppedAt = line.number;
            return report;
        }
        if (isQuit(line.text)) {
            report.quit = true;
            report.stoppedAt = line.number;
            return report;
        }
        ++report.ran;
        if (!run(line.text).ok) {
            ++report.failed;
            if (!options.continueOnError) {
                report.stoppedAt = line.number;
                return report;
            }
        }
    }
    return report;
}

QString formatScriptReport(const QString& name, const ScriptReport& report)
{
    QString record = QString("script=%1 lines=%2 ran=%3 failed=%4")
                         .arg(name.isEmpty() ? QString("pasted") : "\"" + name + "\"")
                         .arg(report.lines)
                         .arg(report.ran)
                         .arg(report.failed);
    if (report.stoppedAt > 0) {
        const char* key = report.cancelled ? "cancelled_at" : report.quit ? "quit_at" : "stopped_at";
        record += QString(" %1=%2").arg(key).arg(report.stoppedAt);
    }
    return record;
}

// ---- the dialog -----------------------------------------------------------------------------

ScriptRunDialog::ScriptRunDialog(ScriptDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("fileRunScriptDialog");
    setWindowTitle("Run Script");
    setModal(false);
    // Wide enough for a path and the line it runs to be read whole.
    setMinimumWidth(560);

    auto* intro = new QLabel(
        "A script is a file of commands, one a line, as katana_cli runs it: blank lines and "
        "lines starting with # are skipped. Each line is run as if typed and is its own undo "
        "step; the run stops at the first line refused.",
        this);
    intro->setWordWrap(true);

    path_ = new QLineEdit(this);
    path_->setObjectName("scriptPath");
    path_->setPlaceholderText("the script: a .kcs file of commands");
    auto* browseButton = new QPushButton("Browse...", this);
    browseButton->setObjectName("scriptBrowse");
    auto* pathRow = new QHBoxLayout;
    pathRow->addWidget(path_, 1);
    pathRow->addWidget(browseButton);

    preview_ = new QPlainTextEdit(this);
    preview_->setObjectName("scriptPreview");
    preview_->setReadOnly(true);
    preview_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    preview_->setLineWrapMode(QPlainTextEdit::NoWrap);
    preview_->setPlaceholderText("The commands the script holds appear here.");
    preview_->setMinimumHeight(160);

    continueOnError_ = new QCheckBox("Continue after errors", this);
    continueOnError_->setObjectName("scriptContinueOnError");
    continueOnError_->setToolTip("Run every line and count the refused ones (SCRIPT ... CONTINUE), "
                                 "rather than stop at the first");

    command_ = new QLineEdit(this);
    command_->setObjectName("scriptCommand");
    command_->setReadOnly(true);
    command_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    command_->setToolTip("The line Run hands to the command line - type it there and it does the "
                         "same");

    run_ = new QPushButton("Run", this);
    run_->setObjectName("scriptRun");
    run_->setDefault(true);
    status_ = new QLabel(this);
    status_->setObjectName("scriptStatus");
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    auto* close = new QPushButton("Close", this);
    close->setObjectName("scriptClose");
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(status_, 1);
    buttons->addWidget(run_);
    buttons->addWidget(close);

    auto* form = new QFormLayout;
    form->addRow("Script:", pathRow);
    form->addRow("Commands:", preview_);
    form->addRow(QString(), continueOnError_);
    form->addRow("Command:", command_);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(intro);
    layout->addLayout(form, 1);
    layout->addLayout(buttons);

    connect(path_, &QLineEdit::textChanged, this, [this] { refresh(); });
    connect(continueOnError_, &QCheckBox::toggled, this, [this] { refresh(); });
    connect(browseButton, &QPushButton::clicked, this, [this] { browse(); });
    connect(run_, &QPushButton::clicked, this, [this] { run(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    refresh();
}

void ScriptRunDialog::setScriptPath(const QString& path)
{
    path_->setText(QDir::toNativeSeparators(path));
}

Result<QString> ScriptRunDialog::command() const
{
    const QString path = path_->text().trimmed();
    if (path.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "no script is named");
    }
    return scriptCommandLine(path, continueOnError_->isChecked());
}

void ScriptRunDialog::refresh()
{
    const auto line = command();
    command_->setText(line ? *line : QString());
    run_->setEnabled(line.ok());
    // What the file holds, as the run will read it; a file that cannot be
    // read says why here, and Run still runs the line, whose refusal is the
    // verb's to give.
    const QString path = path_->text().trimmed();
    if (path.isEmpty()) {
        preview_->clear();
        return;
    }
    const auto lines = readScript(path);
    if (!lines) {
        preview_->setPlainText(QString::fromStdString(lines.error().describe()));
        return;
    }
    QStringList shown;
    for (const ScriptLine& script : *lines) {
        shown << QString("%1  %2").arg(script.number, 4).arg(script.text);
    }
    preview_->setPlainText(shown.isEmpty() ? "(no commands: every line is blank or a comment)"
                                           : shown.join('\n'));
}

void ScriptRunDialog::run()
{
    const auto line = command();
    if (!line) {
        status_->setText(QString::fromStdString(line.error().message));
        return;
    }
    if (!context_.run) {
        status_->setText("nothing here can run a command");
        return;
    }
    const VerbOutcome outcome = context_.run(*line);
    // The record the run ended with is the reply's last line; what each line
    // said is in the command log.
    const QString last = outcome.reply.section('\n', -1);
    status_->setText(outcome.ok ? "Done: " + last
                                : "Stopped: " + outcome.error.section('\n', -1) +
                                      (last.isEmpty() ? QString() : "\n" + last));
}

void ScriptRunDialog::browse()
{
    if (context_.headless && context_.headless()) {
        status_->setText("A headless session opens no file dialog: fill scriptPath with the "
                         "path instead.");
        return;
    }
    const QString path =
        QFileDialog::getOpenFileName(this, "Run Script", path_->text().trimmed(),
                                     "Katana scripts (*.kcs);;Text files (*.txt);;All files (*)");
    if (!path.isEmpty()) {
        setScriptPath(path);
    }
}

} // namespace katana::qt
