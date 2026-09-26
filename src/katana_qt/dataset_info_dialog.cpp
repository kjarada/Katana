#include "dataset_info_dialog.hpp"

#include <QApplication>
#include <QClipboard>
#include <QDialogButtonBox>
#include <QFileInfo>
#include <QFontDatabase>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPointer>
#include <QPushButton>
#include <QTabWidget>
#include <QTableWidget>
#include <QVBoxLayout>

#include <string>
#include <utility>
#include <vector>

#include "geo/replies.hpp"

namespace katana::qt {

namespace {

namespace geo = katana::app::geo;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

QTableWidget* table(const QString& name, const QStringList& headings, QWidget* parent)
{
    auto* widget = new QTableWidget(0, static_cast<int>(headings.size()), parent);
    widget->setObjectName(name);
    widget->setHorizontalHeaderLabels(headings);
    widget->setEditTriggers(QAbstractItemView::NoEditTriggers);
    widget->verticalHeader()->setVisible(false);
    widget->horizontalHeader()->setStretchLastSection(true);
    return widget;
}

void addRow(QTableWidget& table, const QStringList& cells)
{
    const int row = table.rowCount();
    table.insertRow(row);
    for (int column = 0; column < cells.size(); ++column) {
        table.setItem(row, column, new QTableWidgetItem(cells[column]));
    }
}

QString field(const geo::Record& record, const char* key)
{
    return qs(record.get(key).value_or(""));
}

// A record as a person reads it: its word, then key=value, unquoted.
QString readable(const geo::Record& record)
{
    QStringList words{qs(record.kind)};
    for (const auto& [key, text] : record.fields) {
        words << qs(key) + "=" + qs(text);
    }
    return words.join(' ');
}

} // namespace

DatasetInfoDialog::DatasetInfoDialog(const QString& path, DatasetInfoRunner runner,
                                     QWidget* parent)
    : QDialog(parent), path_(path), runner_(std::move(runner))
{
    setObjectName(QStringLiteral("datasetInfoDialog"));
    setWindowTitle("Dataset Information - " + QFileInfo(path).fileName());
    resize(760, 520);
    auto* layout = new QVBoxLayout(this);

    const QFont fixed = QFontDatabase::systemFont(QFontDatabase::FixedFont);
    tabs_ = new QTabWidget(this);
    tabs_->setObjectName(QStringLiteral("datasetInfoTabs"));
    summary_ = new QPlainTextEdit(this);
    summary_->setObjectName(QStringLiteral("datasetInfoSummary"));
    summary_->setReadOnly(true);
    summary_->setFont(fixed);
    summary_->setLineWrapMode(QPlainTextEdit::NoWrap);
    fields_ = table(QStringLiteral("datasetInfoFields"), {"Layer", "Field", "Type", "Width"}, this);
    bands_ = table(QStringLiteral("datasetInfoBands"),
                   {"Band", "Type", "No data", "Min", "Max", "Mean", "Std dev", "Overviews"}, this);
    auto* jsonPage = new QWidget(this);
    auto* jsonLayout = new QVBoxLayout(jsonPage);
    json_ = new QPlainTextEdit(jsonPage);
    json_->setObjectName(QStringLiteral("datasetInfoJson"));
    json_->setReadOnly(true);
    json_->setFont(fixed);
    json_->setLineWrapMode(QPlainTextEdit::NoWrap);
    copyJson_ = new QPushButton(QStringLiteral("&Copy as JSON"), jsonPage);
    copyJson_->setObjectName(QStringLiteral("datasetInfoCopyJson"));
    copyJson_->setToolTip(QStringLiteral("GDAL's own description of the file, to the clipboard"));
    copyJson_->setEnabled(false);
    jsonLayout->addWidget(json_);
    jsonLayout->addWidget(copyJson_, 0, Qt::AlignRight);
    tabs_->addTab(summary_, QStringLiteral("Summary"));
    tabs_->addTab(fields_, QStringLiteral("Fields"));
    tabs_->addTab(bands_, QStringLiteral("Bands"));
    tabs_->addTab(jsonPage, QStringLiteral("JSON"));
    layout->addWidget(tabs_, 1);

    command_ = new QLineEdit(this);
    command_->setObjectName(QStringLiteral("datasetInfoCommand"));
    command_->setReadOnly(true);
    command_->setFont(fixed);
    command_->setToolTip(QStringLiteral("The line last run, as it would be typed"));
    layout->addWidget(command_);
    reply_ = new QLabel(this);
    reply_->setObjectName(QStringLiteral("datasetInfoReply"));
    reply_->setWordWrap(true);
    reply_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(reply_);

    auto* buttons = new QDialogButtonBox(this);
    stats_ = buttons->addButton(QStringLiteral("Compute &Statistics"), QDialogButtonBox::ActionRole);
    stats_->setObjectName(QStringLiteral("datasetInfoStats"));
    stats_->setToolTip(QStringLiteral(
        "INFO ... STATS: every band's minimum, maximum, mean and standard deviation, read from "
        "every pixel. Nothing is written beside the file."));
    check_ = buttons->addButton(QStringLiteral("C&heck"), QDialogButtonBox::ActionRole);
    check_->setObjectName(QStringLiteral("datasetInfoCheck"));
    check_->setToolTip(QStringLiteral(
        "INFO ... CHECK: read every value of the file, and say what could not be read"));
    QPushButton* close = buttons->addButton(QDialogButtonBox::Close);
    close->setObjectName(QStringLiteral("datasetInfoClose"));
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    layout->addWidget(buttons);

    connect(copyJson_, &QPushButton::clicked, this,
            [this] { QApplication::clipboard()->setText(json_->toPlainText()); });
    connect(stats_, &QPushButton::clicked, this, [this] {
        runLine(QStringLiteral("STATS"), [this](const VerbOutcome& outcome) { showRecords(outcome); });
    });
    connect(check_, &QPushButton::clicked, this, [this] {
        runLine(QStringLiteral("CHECK"), [this](const VerbOutcome& outcome) { showRecords(outcome); });
    });

    runLine({}, [this](const VerbOutcome& outcome) { showRecords(outcome); });
    runLine(QStringLiteral("JSON"), [this](const VerbOutcome& outcome) { showJson(outcome); });
}

QString DatasetInfoDialog::line(const QString& words) const
{
    return "INFO \"" + path_ + "\"" + (words.isEmpty() ? QString() : " " + words);
}

void DatasetInfoDialog::runLine(const QString& words, std::function<void(const VerbOutcome&)> use)
{
    const QString text = line(words);
    command_->setText(text);
    if (!runner_.run) {
        return;
    }
    const VerbOutcome outcome = runner_.run(text);
    // The dialog may be closed while the job reads: what it says then goes
    // nowhere.
    QPointer<DatasetInfoDialog> guard(this);
    const auto deliver = [guard, use](const VerbOutcome& done) {
        if (guard != nullptr) {
            use(done);
        }
    };
    if (outcome.ok && runner_.await && runner_.await(outcome, deliver)) {
        reply_->setText("Reading " + QFileInfo(path_).fileName() + "...");
        return;
    }
    use(outcome);
}

void DatasetInfoDialog::showRecords(const VerbOutcome& outcome)
{
    if (!outcome.ok) {
        reply_->setText(outcome.error.isEmpty() ? QStringLiteral("The line was refused.")
                                                : outcome.error);
        return;
    }
    described_ = true;
    QStringList summary;
    QStringList checked;
    fields_->setRowCount(0);
    bands_->setRowCount(0);
    for (const geo::Record& record : geo::parseRecords(outcome.reply.toStdString())) {
        if (record.kind == "field") {
            const QString subtype = field(record, "subtype");
            addRow(*fields_, {field(record, "layer"), field(record, "name"),
                              field(record, "type") + (subtype.isEmpty() ? "" : " (" + subtype + ")"),
                              field(record, "width")});
        } else if (record.kind == "band") {
            addRow(*bands_, {field(record, "band"), field(record, "type"), field(record, "nodata"),
                             field(record, "min"), field(record, "max"), field(record, "mean"),
                             field(record, "stddev"), field(record, "overviews")});
        } else if (record.kind == "check" || record.kind == "problem") {
            checked << readable(record);
        } else if (record.kind != "overview") {
            summary << readable(record);
        }
    }
    summary_->setPlainText(summary.join('\n'));
    fields_->resizeColumnsToContents();
    bands_->resizeColumnsToContents();
    reply_->setText(checked.isEmpty() ? QString() : checked.join('\n'));
}

void DatasetInfoDialog::showJson(const VerbOutcome& outcome)
{
    if (!outcome.ok) {
        json_->setPlainText(outcome.error);
        copyJson_->setEnabled(false);
        return;
    }
    // The executor gives it on one line; indented here for a person to read.
    const QJsonDocument document = QJsonDocument::fromJson(outcome.reply.toUtf8());
    json_->setPlainText(document.isNull() ? outcome.reply
                                          : QString::fromUtf8(document.toJson(QJsonDocument::Indented)));
    copyJson_->setEnabled(true);
}

} // namespace katana::qt
