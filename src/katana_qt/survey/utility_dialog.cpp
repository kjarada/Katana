#include "survey/utility_dialog.hpp"

#include <QApplication>
#include <QClipboard>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSizePolicy>
#include <QTabWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <initializer_list>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/utility_network.hpp"
#include "theme.hpp"

namespace katana::qt {
namespace {

namespace sub = katana::survey::subsurface;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// What the dialog refuses to write, said in its words.
katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

// A default of the library's own, as a placeholder shows it: 10, 0.3, 2.
QString defaultOf(double value)
{
    return "default " + QString::number(value, 'g', 12);
}

// A word as the command line reads it: double-quoted when it holds a blank,
// which the interpreter's tokenizer would otherwise split it at.
Result<QString> word(const QString& text, const QString& field)
{
    if (text.contains('"')) {
        return invalid(field + " holds a double quote, which a command line cannot carry");
    }
    const bool blank = std::any_of(text.begin(), text.end(), [](QChar c) { return c.isSpace(); });
    return blank ? "\"" + text + "\"" : text;
}

// A file the tool needs, trimmed; InvalidArgument naming it when blank.
Result<QString> requiredFile(const QString& text, const QString& field)
{
    const QString path = text.trimmed();
    if (path.isEmpty()) {
        return invalid("choose the " + field + " first");
    }
    return word(path, "the " + field + " path");
}

// " KEYWORD <number>" for an option that was given, nothing for a blank one.
// The number goes on the line as typed, once it reads as one.
Result<QString> option(const QString& keyword, const QString& text, const QString& field)
{
    const QString number = text.trimmed();
    if (number.isEmpty()) {
        return QString();
    }
    if (!katana::core::parseFiniteDouble(number.toStdString())) {
        return invalid(field + " must be a number of metres, not '" + number + "'");
    }
    return " " + keyword + " " + number;
}

// `part` after `prefix`, or its error.
Result<QString> prefixed(const QString& prefix, const Result<QString>& part)
{
    if (!part) {
        return part.error();
    }
    return prefix + *part;
}

QLabel* noteLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

} // namespace

const char* utilityVerbWord(UtilityTool tool)
{
    switch (tool) {
    case UtilityTool::Draw:
        return "DRAW";
    case UtilityTool::Report:
        return "REPORT";
    case UtilityTool::Verify:
        return "VERIFY";
    case UtilityTool::Clearance:
        return "CLEARANCE";
    case UtilityTool::Check:
        return "CHECK";
    }
    return "REPORT";
}

Result<QString> utilityCommandLine(const UtilityForm& form)
{
    const auto schedule = requiredFile(form.schedule, "utility schedule");
    if (!schedule) {
        return schedule.error();
    }
    QString line = QString("UTILITY ") + utilityVerbWord(form.tool) + " " + *schedule;
    // The rest in the order the verb lists it, an option only when given; the
    // first part that cannot be written refuses the whole line.
    std::vector<Result<QString>> parts;
    switch (form.tool) {
    case UtilityTool::Draw:
        parts.push_back(option("SPACING", form.spacing, "The detected spacing"));
        if (const QString prefix = form.layerPrefix.trimmed(); !prefix.isEmpty()) {
            parts.push_back(prefixed(" LAYER ", word(prefix, "The layer prefix")));
        }
        break;
    case UtilityTool::Report:
        parts.push_back(option("MINCOVER", form.minCover, "The minimum cover"));
        parts.push_back(option("SPACING", form.spacing, "The detected spacing"));
        break;
    case UtilityTool::Verify:
        break;
    case UtilityTool::Clearance:
        parts.push_back(prefixed(" ", requiredFile(form.design, "design of the proposed works")));
        parts.push_back(option("WIDTH", form.width, "The works' width"));
        parts.push_back(option("H", form.horizontal, "The horizontal clearance"));
        parts.push_back(option("V", form.vertical, "The vertical clearance"));
        parts.push_back(option("MARGIN", form.margin, "The unverified margin"));
        break;
    case UtilityTool::Check:
        parts.push_back(prefixed(" SCHEMA ", requiredFile(form.schema, "delivery schema")));
        break;
    }
    for (const Result<QString>& part : parts) {
        if (!part) {
            return part.error();
        }
        line += *part;
    }
    return line;
}

UtilityToolsDialog::UtilityToolsDialog(UtilityDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("utilityDialog");
    setWindowTitle("Subsurface Utilities (AS 5488)");
    // Non-modal: kept open beside the drawing, which a Draw changes.
    setModal(false);
    resize(900, 660);

    // The placeholders show the library's own defaults, read from its
    // settings, so what they say cannot drift from what a blank option gets.
    const sub::GradingSettings grading;
    const sub::ClearanceRequirement requirement;
    const auto field = [this](const char* name, const QString& placeholder, const QString& tip) {
        auto* edit = new QLineEdit(this);
        edit->setObjectName(name);
        edit->setPlaceholderText(placeholder);
        edit->setToolTip(tip);
        return edit;
    };
    const auto button = [this](const char* name, const QString& text) {
        auto* made = new QPushButton(text, this);
        made->setObjectName(name);
        made->setAutoDefault(false);
        return made;
    };
    const auto row = [](std::initializer_list<QWidget*> widgets) {
        auto* layout = new QHBoxLayout;
        bool first = true;
        for (QWidget* widget : widgets) {
            layout->addWidget(widget, first ? 1 : 0);
            first = false;
        }
        return layout;
    };
    const QString csvFilter = "CSV files (*.csv);;All files (*)";

    // What every tool reads.
    schedule_ = field("utilitySchedule", "the utility schedule: a .csv, one row per located vertex",
                      "The schedule of located services (docs/subsurface_utilities.md, \"The "
                      "schedule format\"): rows with the same line id form one service, in order");
    auto* scheduleBrowse = button("utilityScheduleBrowse", "Browse...");
    spacing_ = field("utilitySpacing", defaultOf(grading.maximumDetectedSpacing) + " m",
                     "SPACING: the longest segment a detected path may span and stay QL-B; a "
                     "longer one was interpolated, not traced, and grades QL-C. The project "
                     "specification's figure, not the standard's");
    auto* common = new QFormLayout;
    common->addRow("Schedule:", row({schedule_, scheduleBrowse}));
    common->addRow("Detected spacing (m):", spacing_);

    // One tab per form of the verb, in the order of the menu.
    tabs_ = new QTabWidget(this);
    tabs_->setObjectName("utilityTabs");
    // As tall as the tallest tab needs and no taller: the room is the reply's.
    tabs_->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum);

    auto* drawPage = new QWidget(tabs_);
    auto* drawLayout = new QFormLayout(drawPage);
    drawLayout->addRow(noteLabel(
        "Grades the schedule and adds it to the drawing as one undo step: a layer for each "
        "type of service and quality level, with a linetype for each level (QL-A continuous, "
        "QL-B dashed, QL-C dash-dot, QL-D dotted), one polyline for each run at one level, and "
        "a point at every located vertex carrying its evidence. A line that cannot be graded "
        "refuses the whole draw.",
        drawPage));
    layerPrefix_ = field("utilityLayerPrefix", "default utilities",
                         "LAYER: the layer the drawn services nest under, as "
                         "<prefix>/<type>/QL-A and <prefix>/<type>/points");
    drawLayout->addRow("Layer prefix:", layerPrefix_);
    tabs_->addTab(drawPage, "Draw");

    auto* reportPage = new QWidget(tabs_);
    auto* reportLayout = new QFormLayout(reportPage);
    reportLayout->addRow(noteLabel(
        "The investigation report: every vertex and segment graded by AS 5488 quality level, "
        "the length of each service at each level, depth of cover, and what the schedule "
        "claims better than its evidence supports. Nothing is added to the drawing.",
        reportPage));
    minCover_ = field("utilityMinCover", "none - cover is reported, not tested",
                      "MINCOVER: flag every vertex whose cover is less than this");
    reportLayout->addRow("Minimum cover (m):", minCover_);
    tabs_->addTab(reportPage, "Report");

    auto* verifyPage = new QWidget(tabs_);
    auto* verifyLayout = new QVBoxLayout(verifyPage);
    verifyLayout->addWidget(noteLabel(
        "The QL-B detections compared with the QL-A exposures that check them (the schedule's "
        "verifies column): how far each detection was from where the service was seen, in plan "
        "and in level, against QL-B's tolerance. Nothing is added to the drawing.",
        verifyPage));
    verifyLayout->addStretch(1);
    tabs_->addTab(verifyPage, "Verify");

    auto* clearancePage = new QWidget(tabs_);
    auto* clearanceLayout = new QFormLayout(clearancePage);
    clearanceLayout->addRow(noteLabel(
        "Clearance of proposed works from every segment of every service, each widened by the "
        "positional tolerance of its quality level: Conflict, Unconfirmed (QL-C or QL-D nearby: "
        "locate it first), Within tolerance, or Clear. The clearances are the asset owners' "
        "rules, not AS 5488's.",
        clearancePage));
    design_ = field("utilityDesign", "the proposed works: a .csv of the centre line",
                    "The design centre line (docs/subsurface_utilities.md): easting, northing and "
                    "an optional level at each vertex");
    auto* designBrowse = button("utilityDesignBrowse", "Browse...");
    clearanceLayout->addRow("Design:", row({design_, designBrowse}));
    width_ = field("utilityWidth", "default 0 m - the centre line itself",
                   "WIDTH: the works' width - a pipe's outside diameter, a trench's width");
    clearanceLayout->addRow("Works width (m):", width_);
    horizontal_ = field("utilityH", defaultOf(requirement.horizontal) + " m",
                        "H: the horizontal clearance required, face to face");
    clearanceLayout->addRow("Horizontal clearance (m):", horizontal_);
    vertical_ = field("utilityV", defaultOf(requirement.vertical) + " m",
                      "V: the vertical clearance required, face to face");
    clearanceLayout->addRow("Vertical clearance (m):", vertical_);
    margin_ = field("utilityMargin", defaultOf(requirement.unverifiedMargin) + " m",
                    "MARGIN: a QL-C or QL-D service this close to the works, beyond the "
                    "horizontal clearance, is Unconfirmed - its drawn position is not a "
                    "measurement");
    clearanceLayout->addRow("Unverified margin (m):", margin_);
    tabs_->addTab(clearancePage, "Clearance");

    auto* checkPage = new QWidget(tabs_);
    auto* checkLayout = new QFormLayout(checkPage);
    checkLayout->addRow(noteLabel(
        "The schedule against a client's delivery schema: every mandatory attribute present, "
        "every value from its list, spelt exactly. A schedule with errors still shows the whole "
        "report, and the line fails so that a script can stop on it.",
        checkPage));
    schema_ = field("utilitySchema", "the delivery schema: a .csv of attributes and value lists",
                    "The schema file (include/katana/survey/subsurface/delivery_schema.hpp); "
                    "tools/utility_schema_domains.py makes one from a TfNSW Utility Schema "
                    "workbook");
    auto* schemaBrowse = button("utilitySchemaBrowse", "Browse...");
    checkLayout->addRow("Schema:", row({schema_, schemaBrowse}));
    tabs_->addTab(checkPage, "Check");

    // The line, as it will run.
    command_ = new QLineEdit(this);
    command_->setObjectName("utilityCommand");
    command_->setReadOnly(true);
    command_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    command_->setToolTip("The line Run hands to the command line - type it there, or give it to "
                         "katana_cli, and it does the same");
    run_ = new QPushButton("Run", this);
    run_->setObjectName("utilityRun");
    run_->setDefault(true);
    status_ = new QLabel(this);
    status_->setObjectName("utilityStatus");
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    auto* runRow = new QHBoxLayout;
    runRow->addWidget(status_, 1);
    runRow->addWidget(run_);

    output_ = new QPlainTextEdit(this);
    output_->setObjectName("utilityOutput");
    output_->setReadOnly(true);
    output_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    // The reports are tables: wrapping would break their columns.
    output_->setLineWrapMode(QPlainTextEdit::NoWrap);
    output_->setPlaceholderText("The reply appears here and in the command log.");
    output_->setMinimumHeight(200);

    copy_ = button("utilityCopy", "Copy");
    save_ = button("utilitySave", "Save As...");
    auto* close = button("utilityClose", "Close");
    copy_->setEnabled(false);
    save_->setEnabled(false);
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(copy_);
    buttons->addWidget(save_);
    buttons->addStretch(1);
    buttons->addWidget(close);

    auto* layout = new QVBoxLayout(this);
    layout->addLayout(common);
    layout->addWidget(tabs_);
    auto* commandForm = new QFormLayout;
    commandForm->addRow("Command:", command_);
    layout->addLayout(commandForm);
    layout->addLayout(runRow);
    layout->addWidget(output_, 1);
    layout->addLayout(buttons);

    for (QLineEdit* edit : {schedule_, spacing_, layerPrefix_, minCover_, design_, width_,
                            horizontal_, vertical_, margin_, schema_}) {
        connect(edit, &QLineEdit::textChanged, this, [this] { refreshCommand(); });
    }
    connect(tabs_, &QTabWidget::currentChanged, this, [this] { refreshCommand(); });
    connect(scheduleBrowse, &QPushButton::clicked, this, [this, csvFilter] {
        browse(*schedule_, "Utility Schedule", csvFilter);
    });
    connect(designBrowse, &QPushButton::clicked, this,
            [this, csvFilter] { browse(*design_, "Proposed Works", csvFilter); });
    connect(schemaBrowse, &QPushButton::clicked, this,
            [this, csvFilter] { browse(*schema_, "Delivery Schema", csvFilter); });
    connect(run_, &QPushButton::clicked, this, [this] { run(); });
    connect(copy_, &QPushButton::clicked, this, [this] {
        QApplication::clipboard()->setText(output_->toPlainText());
        setStatus("Copied.");
    });
    connect(save_, &QPushButton::clicked, this, [this] { saveOutput(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });

    refreshCommand();
}

void UtilityToolsDialog::showTool(UtilityTool tool)
{
    tabs_->setCurrentIndex(static_cast<int>(tool));
}

UtilityTool UtilityToolsDialog::tool() const
{
    return static_cast<UtilityTool>(tabs_->currentIndex());
}

UtilityForm UtilityToolsDialog::form() const
{
    UtilityForm form;
    form.tool = tool();
    form.schedule = schedule_->text();
    form.design = design_->text();
    form.schema = schema_->text();
    form.minCover = minCover_->text();
    form.spacing = spacing_->text();
    form.width = width_->text();
    form.horizontal = horizontal_->text();
    form.vertical = vertical_->text();
    form.margin = margin_->text();
    form.layerPrefix = layerPrefix_->text();
    return form;
}

Result<QString> UtilityToolsDialog::command() const
{
    return utilityCommandLine(form());
}

void UtilityToolsDialog::refreshCommand()
{
    // The detected spacing grades; only Draw and Report grade.
    spacing_->setEnabled(tool() == UtilityTool::Draw || tool() == UtilityTool::Report);
    const auto line = command();
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : "nothing to run yet: " + qs(line.error().message));
}

void UtilityToolsDialog::run()
{
    const auto line = command();
    if (!line) {
        setStatus(qs(line.error().message), true);
        return;
    }
    if (!context_.execute) {
        setStatus("nothing here can run a command", true);
        return;
    }
    const UtilityTool running = tool();
    const auto reply = context_.execute(*line);
    if (!reply) {
        // The whole of it below - a schema check with errors is a whole
        // report - and its first line beside Run.
        showOutput(qs(reply.error().describe()));
        setStatus(qs(reply.error().message).section('\n', 0, 0), true);
        return;
    }
    showOutput(qs(*reply));
    setStatus(running == UtilityTool::Draw
                  ? "Drawn as one undo step - Undo removes it all. The records are below and in "
                    "the command log."
                  : "Done. The report is below and in the command log.");
}

void UtilityToolsDialog::showOutput(const QString& text)
{
    QString trimmed = text;
    while (trimmed.endsWith('\n')) {
        trimmed.chop(1);
    }
    output_->setPlainText(trimmed);
    copy_->setEnabled(!trimmed.isEmpty());
    save_->setEnabled(!trimmed.isEmpty());
}

void UtilityToolsDialog::browse(QLineEdit& field, const QString& title, const QString& filter)
{
    if (context_.headless && context_.headless()) {
        setStatus("A headless session opens no file dialog: fill " + field.objectName() +
                      " with the path instead.",
                  true);
        return;
    }
    const QString path = QFileDialog::getOpenFileName(this, title, field.text().trimmed(), filter);
    if (!path.isEmpty()) {
        field.setText(QDir::toNativeSeparators(path));
    }
}

void UtilityToolsDialog::saveOutput()
{
    if (context_.headless && context_.headless()) {
        setStatus("A headless session opens no file dialog; the reply is in the command log.", true);
        return;
    }
    const QString suggested = QString("utility_%1.txt").arg(QString(utilityVerbWord(tool())).toLower());
    const QString path = QFileDialog::getSaveFileName(this, "Save Utility Output", suggested,
                                                      "Text files (*.txt);;All files (*)");
    if (path.isEmpty()) {
        return;
    }
    if (const auto saved = saveOutputTo(path); !saved) {
        setStatus(qs(saved.error().describe()), true);
        return;
    }
    setStatus("Saved to " + QDir::toNativeSeparators(path) + ".");
}

katana::core::Status UtilityToolsDialog::saveOutputTo(const QString& path)
{
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    const QByteArray bytes = (output_->toPlainText() + "\n").toUtf8();
    if (file.write(bytes) != bytes.size()) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    return {};
}

void UtilityToolsDialog::setStatus(const QString& text, bool isError)
{
    status_->setText(text);
    status_->setStyleSheet(isError ? "color: " + theme::error().name() + ";" : QString());
}

} // namespace katana::qt
