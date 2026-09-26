#include "annotation/label_layout_report.hpp"

#include <algorithm>
#include <map>
#include <optional>
#include <utility>

#include <QCheckBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStringList>
#include <QVBoxLayout>

#include "katana/core/text.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

// One reply record's fields: key=value pairs split on blanks, a value in
// double quotes holding blanks, with \" and \\ inside it, as the annotation
// verbs write them (annotation_verbs.cpp, field()).
std::map<QString, QString> fieldsOf(const QString& line)
{
    std::map<QString, QString> fields;
    qsizetype at = 0;
    while (at < line.size()) {
        while (at < line.size() && line[at].isSpace()) {
            ++at;
        }
        const qsizetype equals = line.indexOf('=', at);
        if (at >= line.size() || equals < 0) {
            break;
        }
        // A word with no '=' ("created") is not a field.
        const qsizetype blank = line.indexOf(' ', at);
        if (blank >= 0 && blank < equals) {
            at = blank;
            continue;
        }
        const QString key = line.mid(at, equals - at);
        at = equals + 1;
        QString value;
        if (at < line.size() && line[at] == '"') {
            ++at;
            while (at < line.size() && line[at] != '"') {
                if (line[at] == '\\' && at + 1 < line.size()) {
                    ++at;
                }
                value += line[at];
                ++at;
            }
            ++at; // the closing quote
        } else {
            while (at < line.size() && !line[at].isSpace()) {
                value += line[at];
                ++at;
            }
        }
        fields[key] = value;
    }
    return fields;
}

std::size_t countOf(const std::map<QString, QString>& fields, const QString& key, bool& ok)
{
    const auto found = fields.find(key);
    bool read = false;
    const qulonglong value = found == fields.end() ? 0 : found->second.toULongLong(&read);
    ok = ok && read;
    return static_cast<std::size_t>(value);
}

QLabel* countLabel(const QString& name, QWidget* parent)
{
    auto* label = new QLabel(QStringLiteral("-"), parent);
    label->setObjectName(name);
    return label;
}

} // namespace

QString labelLayoutLine(bool collisions)
{
    return collisions ? QStringLiteral("LABEL LAYOUT") : QStringLiteral("LABEL LAYOUT collisions=off");
}

LabelLayoutSummary parseLabelLayoutReply(const QString& reply)
{
    LabelLayoutSummary summary;
    for (const QString& line : reply.split('\n', Qt::SkipEmptyParts)) {
        const auto fields = fieldsOf(line.trimmed());
        if (fields.contains(QStringLiteral("considered"))) {
            bool ok = true;
            summary.considered = countOf(fields, QStringLiteral("considered"), ok);
            summary.placed = countOf(fields, QStringLiteral("placed"), ok);
            summary.displaced = countOf(fields, QStringLiteral("displaced"), ok);
            summary.suppressed = countOf(fields, QStringLiteral("suppressed"), ok);
            summary.orphaned = countOf(fields, QStringLiteral("orphaned"), ok);
            const auto found = fields.find(QStringLiteral("scale"));
            const auto scale = found == fields.end()
                                   ? std::nullopt
                                   : katana::core::parseFiniteDouble(found->second.toStdString());
            summary.scale = scale.value_or(0.0);
            summary.read = ok && scale.has_value();
            continue;
        }
        const auto suppressed = fields.find(QStringLiteral("suppressed"));
        const auto label = fields.find(QStringLiteral("label"));
        if (suppressed != fields.end() && suppressed->second == QStringLiteral("yes") &&
            label != fields.end()) {
            bool ok = false;
            const katana::entity::EntityId id = label->second.toULongLong(&ok);
            if (ok && std::ranges::find(summary.suppressedLabels, id) ==
                          summary.suppressedLabels.end()) {
                summary.suppressedLabels.push_back(id);
            }
        }
    }
    return summary;
}

QString selectLine(const std::vector<katana::entity::EntityId>& ids)
{
    if (ids.empty()) {
        return {};
    }
    QStringList words{QStringLiteral("SELECT")};
    for (const katana::entity::EntityId id : ids) {
        words << QString::number(id);
    }
    return words.join(' ');
}

LabelLayoutReportDialog::LabelLayoutReportDialog(CommandRunner runner, QWidget* parent)
    : QDialog(parent), run_(std::move(runner))
{
    setObjectName(QStringLiteral("labelLayoutDialog"));
    setWindowTitle(QStringLiteral("Label Layout Report"));

    collisions_ = new QCheckBox(QStringLiteral("&Keep labels clear of each other and of the "
                                               "linework"),
                                this);
    collisions_->setObjectName(QStringLiteral("labelLayoutCollisions"));
    collisions_->setChecked(true);
    collisions_->setToolTip(QStringLiteral("As the plan view places them; unticked, every label "
                                           "is put at its own place however they overlap"));
    command_ = new QLineEdit(this);
    command_->setObjectName(QStringLiteral("labelLayoutCommand"));
    command_->setReadOnly(true);
    command_->setText(labelLayoutLine(true));
    placed_ = countLabel(QStringLiteral("labelLayoutPlaced"), this);
    displaced_ = countLabel(QStringLiteral("labelLayoutDisplaced"), this);
    suppressed_ = countLabel(QStringLiteral("labelLayoutSuppressed"), this);
    orphaned_ = countLabel(QStringLiteral("labelLayoutOrphaned"), this);
    output_ = new QPlainTextEdit(this);
    output_->setObjectName(QStringLiteral("labelLayoutOutput"));
    output_->setReadOnly(true);
    output_->setFont(katana::qt::theme::monospaceFont());
    status_ = new QLabel(this);
    status_->setObjectName(QStringLiteral("labelLayoutStatus"));
    status_->setWordWrap(true);

    auto* runButton = new QPushButton(QStringLiteral("&Run"), this);
    runButton->setObjectName(QStringLiteral("labelLayoutRun"));
    select_ = new QPushButton(QStringLiteral("&Select Suppressed"), this);
    select_->setObjectName(QStringLiteral("labelLayoutSelectSuppressed"));
    select_->setToolTip(QStringLiteral("Select the labels that found no room, to move them "
                                       "(Edit Label) or give them another style"));
    select_->setEnabled(false);
    auto* close = new QPushButton(QStringLiteral("Close"), this);
    close->setObjectName(QStringLiteral("labelLayoutClose"));

    auto* counts = new QFormLayout();
    counts->addRow(QStringLiteral("Placed"), placed_);
    counts->addRow(QStringLiteral("Displaced (moved to find room)"), displaced_);
    counts->addRow(QStringLiteral("Suppressed (no room)"), suppressed_);
    counts->addRow(QStringLiteral("Orphaned (label nothing)"), orphaned_);
    counts->addRow(QStringLiteral("Command"), command_);
    auto* buttons = new QHBoxLayout();
    buttons->addWidget(runButton);
    buttons->addWidget(select_);
    buttons->addStretch();
    buttons->addWidget(close);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(collisions_);
    layout->addLayout(counts);
    layout->addWidget(output_, 1);
    layout->addWidget(status_);
    layout->addLayout(buttons);

    connect(collisions_, &QCheckBox::toggled, this,
            [this](bool on) { command_->setText(labelLayoutLine(on)); });
    connect(runButton, &QPushButton::clicked, this, [this] { (void)run(); });
    connect(select_, &QPushButton::clicked, this, [this] { (void)selectSuppressed(); });
    connect(close, &QPushButton::clicked, this, &QDialog::accept);
}

bool LabelLayoutReportDialog::run()
{
    if (!run_) {
        status_->setText(QStringLiteral("There is no command line to run it on."));
        return false;
    }
    const QString line = labelLayoutLine(collisions_->isChecked());
    command_->setText(line);
    const VerbOutcome outcome = run_(line);
    output_->setPlainText(outcome.ok ? outcome.reply : outcome.error);
    if (!outcome.ok) {
        showSummary(LabelLayoutSummary{});
        status_->setText(outcome.error.isEmpty() ? QStringLiteral("The line was refused.")
                                                 : outcome.error);
        return false;
    }
    const LabelLayoutSummary summary = parseLabelLayoutReply(outcome.reply);
    showSummary(summary);
    if (!summary.read) {
        status_->setText(QStringLiteral("The reply had no counts to read."));
        return false;
    }
    status_->setText(QStringLiteral("%1 of %2 label pieces placed at 1:%3.")
                         .arg(summary.placed)
                         .arg(summary.considered)
                         .arg(QString::fromStdString(katana::core::formatExactReal(summary.scale))));
    return true;
}

bool LabelLayoutReportDialog::selectSuppressed()
{
    const QString line = selectLine(summary_.suppressedLabels);
    if (line.isEmpty()) {
        status_->setText(QStringLiteral("No label was suppressed."));
        return false;
    }
    if (!run_) {
        status_->setText(QStringLiteral("There is no command line to run it on."));
        return false;
    }
    const VerbOutcome outcome = run_(line);
    status_->setText(outcome.ok ? outcome.reply : outcome.error);
    return outcome.ok;
}

QString LabelLayoutReportDialog::status() const { return status_->text(); }

void LabelLayoutReportDialog::showSummary(const LabelLayoutSummary& summary)
{
    summary_ = summary;
    const auto text = [&](std::size_t value) {
        return summary.read ? QString::number(value) : QStringLiteral("-");
    };
    placed_->setText(text(summary.placed));
    displaced_->setText(text(summary.displaced));
    suppressed_->setText(text(summary.suppressed));
    orphaned_->setText(text(summary.orphaned));
    select_->setEnabled(!summary.suppressedLabels.empty());
}

} // namespace katana::qt
