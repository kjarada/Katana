#include "survey/survey_points_ui.hpp"

#include <QApplication>
#include <QCheckBox>
#include <QClipboard>
#include <QCollator>
#include <QComboBox>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QItemSelectionModel>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSaveFile>
#include <QTableWidget>
#include <QTimer>
#include <QVBoxLayout>

#include <algorithm>
#include <set>
#include <string_view>
#include <utility>

#include "katana/core/text.hpp"
#include "katana/surveyio/delimited_points.hpp"
#include "survey/survey_templates.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

namespace cad = katana::cad;
namespace surveyio = katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

QLabel* mutedLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

const std::vector<survey::LinearUnit>& units()
{
    static const std::vector<survey::LinearUnit> all{
        survey::LinearUnit::Metres, survey::LinearUnit::Feet, survey::LinearUnit::UsSurveyFeet,
        survey::LinearUnit::Links};
    return all;
}

Status writeFile(const QString& path, const std::string& bytes)
{
    // All or nothing: a half-written point file is worse than none, because
    // it reads without an error.
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly) ||
        file.write(bytes.data(), static_cast<qint64>(bytes.size())) !=
            static_cast<qint64>(bytes.size()) ||
        !file.commit()) {
        return makeError(ErrorCode::FileExportFailure, "the file could not be written",
                         path.toStdString() + ": " + file.errorString().toStdString());
    }
    return {};
}

// A number in a table cell that sorts as a number: the text shows the
// report's decimals, the sort uses the value, and a blank (an absent
// elevation) sorts before every number rather than as zero.
class NumberItem final : public QTableWidgetItem {
  public:
    NumberItem(const QString& text, std::optional<double> value)
        : QTableWidgetItem(text), value_(value)
    {
        setTextAlignment(Qt::AlignRight | Qt::AlignVCenter);
    }
    bool operator<(const QTableWidgetItem& other) const override
    {
        const auto* number = dynamic_cast<const NumberItem*>(&other);
        if (number == nullptr) {
            return QTableWidgetItem::operator<(other);
        }
        if (!value_ || !number->value_) {
            return !value_ && number->value_;
        }
        return *value_ < *number->value_;
    }

  private:
    std::optional<double> value_;
};

// A point id that sorts as a person reads a list of them: 2 before 10, and
// "CP2" before "CP10" (a collator's numeric mode), where a plain text sort
// puts 10, 11 and 12 between 1 and 2.
class IdItem final : public QTableWidgetItem {
  public:
    using QTableWidgetItem::QTableWidgetItem;
    bool operator<(const QTableWidgetItem& other) const override
    {
        static const QCollator collator = [] {
            QCollator numeric;
            numeric.setNumericMode(true);
            return numeric;
        }();
        return collator.compare(text(), other.text()) < 0;
    }
};

QString fixed3(double value) { return QString::number(value, 'f', 3); }

// The margin round a point the Point Manager zooms to, in drawing units
// (metres): a view of about 20 m across shows the point and its neighbours
// on a detail survey. Policy, not a measurement.
constexpr double kZoomHalfWidth = 10.0;

} // namespace

// ---- export ----------------------------------------------------------------------------------

SurveyExportDialog::SurveyExportDialog(SurveyDialogContext context, QWidget* parent)
    : SurveyToolDialog(std::move(context), "surveyExportDialog", "Export Survey Points",
                       "Writes the drawing's survey points - point entities carrying a point "
                       "number - to a delimited text file with the layout below. A point with "
                       "no elevation is written with an empty field, never 0. Drawing "
                       "coordinates are taken as metres (what the import writes); another unit "
                       "converts on the way out.",
                       parent)
{
    file_ = makeField("file", "the file to write", "The file to write; it is replaced whole");
    addRow("File:", {file_});
    addFiller("Browse...", "browse", [this]() -> Status {
        const QString path = QFileDialog::getSaveFileName(
            this, "Export Survey Points", file_->text(), "CSV (*.csv);;Text (*.txt);;All (*)");
        if (!path.isEmpty()) {
            file_->setText(path);
        }
        return {};
    });
    scope_ = addChoice("Points:", "scope", {"All survey points", "Selected survey points"},
                       "Which survey points to write");
    template_ = addChoice("Saved template:", "template", {}, "A layout saved earlier");
    // Kept current as templates are saved and deleted - in the import wizard,
    // while this dialog is open or hidden - not filled once here.
    keepTemplateChoiceCurrent(*template_);
    QStringList presets{"(presets)"};
    for (const surveyio::ColumnPreset preset : surveyio::allColumnPresets()) {
        presets << surveyio::toString(preset);
    }
    preset_ = addChoice("Preset:", "preset", presets, "A column order named by its letters");
    columns_ = addField("Columns:", "columns", "P,E,N,Z,D",
                        "One letter per column: P point id, N northing, E easting, "
                        "Z elevation, C code, D description, - an empty column");
    columns_->setText("P,E,N,Z,C,D");
    delimiter_ = addChoice("Delimiter:", "delimiter",
                           {surveyio::toString(surveyio::Delimiter::Comma),
                            surveyio::toString(surveyio::Delimiter::Tab),
                            surveyio::toString(surveyio::Delimiter::Semicolon),
                            surveyio::toString(surveyio::Delimiter::Whitespace)},
                           "What separates the fields");
    header_ = new QCheckBox("A header line naming the columns", this);
    header_->setObjectName("header");
    header_->setChecked(true);
    header_->setToolTip("With it, the import wizard reads the column order back without "
                        "asking");
    form()->addRow("", header_);
    quoting_ = addChoice("Quotes:", "quoting",
                         {surveyio::toString(surveyio::Quoting::DoubleQuote),
                          surveyio::toString(surveyio::Quoting::None)},
                         "Double quotes round a field that needs them (RFC 4180), or none");
    comment_ = addField("Comment prefix:", "comment", "none",
                        "A field that would start a line with it is quoted");
    decimals_ = addField("Decimals:", "decimals", "0 to 12", "Places after the point");
    decimals_->setText("3");
    QStringList unitNames;
    for (const survey::LinearUnit unit : units()) {
        unitNames << survey::toString(unit);
    }
    unit_ = addChoice("Write in:", "unit", unitNames, "The unit the coordinates are written in");

    connect(template_, &QComboBox::currentIndexChanged, this, [this](int index) {
        if (index <= 0) {
            return;
        }
        const auto layout =
            surveyio::parseLayoutTemplate(template_->currentData().toString().toStdString());
        if (!layout) {
            setNote(qs(layout.error().describe()));
            return;
        }
        QStringList letters;
        for (const surveyio::ColumnRole role : layout->columns) {
            letters << QString(QChar(surveyio::templateLetter(role)));
        }
        columns_->setText(letters.join(','));
        delimiter_->setCurrentIndex(
            std::max(0, delimiter_->findText(surveyio::toString(layout->delimiter))));
        header_->setChecked(layout->headerLines != 0);
        quoting_->setCurrentIndex(
            std::max(0, quoting_->findText(surveyio::toString(layout->quoting))));
        comment_->setText(qs(layout->commentPrefix));
    });
    connect(preset_, &QComboBox::currentIndexChanged, this, [this](int index) {
        if (index <= 0) {
            return;
        }
        if (const auto preset = surveyio::columnPresetNamed(preset_->currentText().toStdString())) {
            QStringList letters;
            for (const surveyio::ColumnRole role : surveyio::presetColumns(*preset)) {
                letters << QString(QChar(surveyio::templateLetter(role)));
            }
            columns_->setText(letters.join(','));
        }
    });
    addVerb("Export", "export", [this] { return exportPoints(); });
    resize(620, 560);
}

void SurveyExportDialog::showEvent(QShowEvent* event)
{
    // Saves and deletes in this Katana refill the list as they happen; this
    // catches a template another Katana saved into the settings meanwhile.
    fillTemplateChoice(*template_);
    SurveyToolDialog::showEvent(event);
}

Result<std::string> SurveyExportDialog::exportPoints()
{
    const QString path = file_->text().trimmed();
    if (path.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "choose the file to write");
    }
    // surveyio's own template text, read by its own parser: the export takes
    // exactly the layouts a saved template may hold.
    QString text = columns_->text().trimmed() + ";delimiter=" + delimiter_->currentText() +
                   ";header=" + (header_->isChecked() ? "1" : "0") +
                   ";quote=" + quoting_->currentText();
    if (!comment_->text().trimmed().isEmpty()) {
        text += ";comment=" + comment_->text().trimmed();
    }
    const auto layout = surveyio::parseLayoutTemplate(text.toStdString());
    if (!layout) {
        return layout.error();
    }
    const auto decimals = katana::core::parseInteger(decimals_->text().trimmed().toStdString());
    if (!decimals || *decimals < 0 || *decimals > surveyio::kMaximumExportDecimals) {
        return makeError(ErrorCode::InvalidArgument, "decimals are a whole number from 0 to 12",
                         decimals_->text().toStdString());
    }

    const cad::Document& doc = document();
    std::vector<cad::DrawingSurveyPoint> points;
    std::size_t notPoints = 0;
    const bool selected = scope_->currentIndex() == 1;
    if (selected) {
        const std::vector<katana::entity::EntityId> ids = doc.selection().ids();
        if (ids.empty()) {
            return makeError(ErrorCode::InvalidState,
                             "nothing is selected - select survey points, or write all of them");
        }
        cad::SurveyPointPick pick = cad::surveyPointsAmong(doc, ids);
        points = std::move(pick.points);
        notPoints = pick.notSurveyPoints;
    } else {
        points = cad::drawingSurveyPoints(doc);
    }
    if (points.empty()) {
        return makeError(ErrorCode::InvalidState,
                         selected ? "none of the selection is a survey point (a point entity "
                                    "with a point number)"
                                  : "the drawing has no survey points (point entities with a "
                                    "point number)");
    }
    std::vector<survey::SurveyPoint> surveyPoints;
    surveyPoints.reserve(points.size());
    std::size_t withoutElevation = 0;
    for (const cad::DrawingSurveyPoint& point : points) {
        surveyPoints.push_back(cad::toSurveyPoint(point));
        withoutElevation += point.elevation ? 0 : 1;
    }
    surveyio::DelimitedExportOptions options;
    options.decimals = static_cast<int>(*decimals);
    options.unit = units()[static_cast<std::size_t>(std::max(0, unit_->currentIndex()))];
    const auto bytes = surveyio::writeDelimitedPoints(surveyPoints, *layout, options);
    if (!bytes) {
        return bytes.error();
    }
    if (const auto status = writeFile(path, *bytes); !status) {
        return status.error();
    }
    std::string report = "Exported " + std::to_string(points.size()) + " point(s) to " +
                         QFileInfo(path).fileName().toStdString() + " - layout " +
                         surveyio::layoutTemplate(*layout) + ", " + std::to_string(*decimals) +
                         " decimals, in " + survey::toString(options.unit) + ".";
    if (withoutElevation != 0) {
        report += "\n" + std::to_string(withoutElevation) +
                  " point(s) have no elevation and were written with an empty field.";
    }
    if (notPoints != 0) {
        report += "\n" + std::to_string(notPoints) +
                  " selected entities are not survey points and were not written.";
    }
    return report;
}

// ---- the Point Manager -----------------------------------------------------------------------

SurveyPointsDock::SurveyPointsDock(cad::Document& document, ViewWorkspace* views, QWidget* parent)
    : QDockWidget("Survey Points", parent), document_(document), views_(views)
{
    setObjectName("SurveyPointsDock");
    auto* body = new QWidget(this);
    auto* layout = new QVBoxLayout(body);
    layout->setContentsMargins(4, 4, 4, 4);
    auto* top = new QHBoxLayout();
    filter_ = new QLineEdit(body);
    filter_->setObjectName("filter");
    filter_->setPlaceholderText("Filter by point, code or description");
    filter_->setClearButtonEnabled(true);
    status_ = new QLabel(body);
    status_->setObjectName("status");
    top->addWidget(filter_, 1);
    top->addWidget(status_);
    layout->addLayout(top);
    table_ = new QTableWidget(0, 7, body);
    table_->setObjectName("points");
    table_->setHorizontalHeaderLabels(
        {"Point", "Code", "Easting", "Northing", "Elevation", "Description", "Source file"});
    table_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->setSelectionMode(QAbstractItemView::ExtendedSelection);
    table_->verticalHeader()->setVisible(false);
    table_->horizontalHeader()->setStretchLastSection(true);
    // No column sorted until a header is clicked: the drawing's own order -
    // the order the points were imported in - is the first one shown.
    table_->horizontalHeader()->setSortIndicator(-1, Qt::AscendingOrder);
    table_->setSortingEnabled(true);
    layout->addWidget(table_, 1);
    layout->addWidget(mutedLabel("Read-only: edit a point with the drawing's tools.", body));
    setWidget(body);

    connect(filter_, &QLineEdit::textChanged, this, [this] { applyFilter(); });
    connect(table_, &QTableWidget::itemSelectionChanged, this, [this] { selectionEdited(); });
    connect(table_, &QTableWidget::cellDoubleClicked, this,
            [this](int row, int) { zoomToRow(row); });
    listener_ = document_.addListener([this] { scheduleRefresh(); });
}

QString SurveyPointsDock::status() const { return status_->text(); }

void SurveyPointsDock::showEvent(QShowEvent* event)
{
    QDockWidget::showEvent(event);
    scheduleRefresh();
}

void SurveyPointsDock::scheduleRefresh()
{
    if (refreshPending_) {
        return;
    }
    refreshPending_ = true;
    QTimer::singleShot(0, this, [this] {
        refreshPending_ = false;
        refresh();
    });
}

void SurveyPointsDock::refresh()
{
    // A hidden dock reads nothing; showing it schedules a refresh.
    if (isHidden()) {
        return;
    }
    std::vector<cad::DrawingSurveyPoint> points = cad::drawingSurveyPoints(document_);
    if (listed_ && points == points_) {
        mirrorSelection();
        return;
    }
    points_ = std::move(points);
    listed_ = true;
    mirroring_ = true;
    table_->setSortingEnabled(false);
    table_->clearContents();
    table_->setRowCount(static_cast<int>(points_.size()));
    for (int row = 0; row < static_cast<int>(points_.size()); ++row) {
        const cad::DrawingSurveyPoint& point = points_[static_cast<std::size_t>(row)];
        auto* id = new IdItem(qs(point.id));
        id->setData(Qt::UserRole, QVariant::fromValue<qulonglong>(point.entity));
        table_->setItem(row, 0, id);
        table_->setItem(row, 1, new QTableWidgetItem(qs(point.code)));
        table_->setItem(row, 2, new NumberItem(fixed3(point.easting), point.easting));
        table_->setItem(row, 3, new NumberItem(fixed3(point.northing), point.northing));
        table_->setItem(row, 4,
                        new NumberItem(point.elevation ? fixed3(*point.elevation) : QString(),
                                       point.elevation));
        table_->setItem(row, 5, new QTableWidgetItem(qs(point.description)));
        table_->setItem(row, 6, new QTableWidgetItem(qs(point.sourceFile)));
    }
    table_->setSortingEnabled(true);
    table_->resizeColumnsToContents();
    mirroring_ = false;
    applyFilter();
    mirrorSelection();
}

void SurveyPointsDock::applyFilter()
{
    const QString needle = filter_->text().trimmed();
    int shown = 0;
    for (int row = 0; row < table_->rowCount(); ++row) {
        bool match = needle.isEmpty();
        for (const int column : {0, 1, 5}) {
            const QTableWidgetItem* item = table_->item(row, column);
            if (!match && item != nullptr && item->text().contains(needle, Qt::CaseInsensitive)) {
                match = true;
            }
        }
        table_->setRowHidden(row, !match);
        shown += match ? 1 : 0;
    }
    status_->setText(QString("%1 points, %2 shown").arg(table_->rowCount()).arg(shown));
}

void SurveyPointsDock::mirrorSelection()
{
    mirroring_ = true;
    table_->clearSelection();
    const auto& selection = document_.selection();
    for (int row = 0; row < table_->rowCount(); ++row) {
        const QTableWidgetItem* item = table_->item(row, 0);
        if (item != nullptr &&
            selection.contains(static_cast<katana::entity::EntityId>(
                item->data(Qt::UserRole).toULongLong()))) {
            // Through the model: selectRow() in extended selection mode
            // would replace the rows already selected.
            table_->selectionModel()->select(
                table_->model()->index(row, 0),
                QItemSelectionModel::Select | QItemSelectionModel::Rows);
        }
    }
    mirroring_ = false;
}

void SurveyPointsDock::selectionEdited()
{
    if (mirroring_) {
        return;
    }
    std::set<katana::entity::EntityId> ids;
    for (const QTableWidgetItem* item : table_->selectedItems()) {
        const QTableWidgetItem* first = table_->item(item->row(), 0);
        if (first != nullptr) {
            ids.insert(static_cast<katana::entity::EntityId>(first->data(Qt::UserRole).toULongLong()));
        }
    }
    // The document's listener runs from here and schedules this dock's
    // refresh for later; nothing is rebuilt inside this signal.
    document_.selection().set({ids.begin(), ids.end()});
    document_.notifySelectionChanged();
}

void SurveyPointsDock::zoomToRow(int row)
{
    const QTableWidgetItem* e = table_->item(row, 2);
    const QTableWidgetItem* n = table_->item(row, 3);
    if (views_ == nullptr || e == nullptr || n == nullptr) {
        return;
    }
    const QTableWidgetItem* first = table_->item(row, 0);
    const auto id = static_cast<katana::entity::EntityId>(first->data(Qt::UserRole).toULongLong());
    const auto found = std::ranges::find(points_, id, &cad::DrawingSurveyPoint::entity);
    if (found == points_.end()) {
        return;
    }
    const katana::geometry::Point2 at(found->easting, found->northing);
    views_->zoomTo(katana::geometry::Box2(
        at - katana::geometry::Vec2(kZoomHalfWidth, kZoomHalfWidth),
        at + katana::geometry::Vec2(kZoomHalfWidth, kZoomHalfWidth)));
}

// ---- the Point Report ------------------------------------------------------------------------

SurveyPointReportDialog::SurveyPointReportDialog(SurveyDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("surveyPointReportDialog");
    setWindowTitle("Point Report");
    setModal(false);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(mutedLabel("The selected survey points, or every survey point when none "
                                 "is selected. Coordinates to 3 decimals; a blank elevation is "
                                 "a point with none.",
                                 this));
    scope_ = new QLabel(this);
    scope_->setObjectName("scope");
    layout->addWidget(scope_);
    report_ = new QPlainTextEdit(this);
    report_->setObjectName("report");
    report_->setReadOnly(true);
    report_->setFont(katana::qt::theme::monospaceFont());
    report_->setLineWrapMode(QPlainTextEdit::NoWrap);
    layout->addWidget(report_, 1);
    auto* buttons = new QHBoxLayout();
    auto* refresh = new QPushButton("Refresh", this);
    refresh->setObjectName("refresh");
    auto* copy = new QPushButton("Copy", this);
    copy->setObjectName("copy");
    auto* save = new QPushButton("Save As CSV...", this);
    save->setObjectName("saveCsv");
    auto* close = new QPushButton("Close", this);
    close->setObjectName("close");
    for (QPushButton* button : {refresh, copy, save, close}) {
        button->setAutoDefault(false);
    }
    buttons->addWidget(refresh);
    buttons->addStretch(1);
    buttons->addWidget(copy);
    buttons->addWidget(save);
    buttons->addWidget(close);
    layout->addLayout(buttons);

    connect(refresh, &QPushButton::clicked, this, [this] { this->refresh(); });
    connect(copy, &QPushButton::clicked, this,
            [this] { QApplication::clipboard()->setText(report_->toPlainText()); });
    connect(save, &QPushButton::clicked, this, [this] {
        const QString path =
            QFileDialog::getSaveFileName(this, "Save Point Report", {}, "CSV (*.csv)");
        if (path.isEmpty()) {
            return;
        }
        if (const auto status = writeFile(path, cad::pointReportCsv(points_, 3)); !status) {
            context_.log("Point Report: " + qs(status.error().describe()), true);
            return;
        }
        context_.log(QString("Point Report: %1 point(s) saved to %2")
                         .arg(points_.size())
                         .arg(QFileInfo(path).fileName()),
                     false);
    });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    resize(900, 520);
}

void SurveyPointReportDialog::showEvent(QShowEvent* event)
{
    QDialog::showEvent(event);
    refresh();
}

void SurveyPointReportDialog::refresh()
{
    const cad::Document& document = *context_.document;
    const std::vector<katana::entity::EntityId> ids = document.selection().ids();
    cad::SurveyPointPick pick = cad::surveyPointsAmong(document, ids);
    const bool selected = !pick.points.empty();
    points_ = selected ? std::move(pick.points) : cad::drawingSurveyPoints(document);
    scope_->setText(selected ? QString("The %1 selected survey point(s).").arg(points_.size())
                             : QString("All %1 survey point(s) in the drawing.").arg(points_.size()));
    report_->setPlainText(qs(cad::formatPointReport(points_, 3)));
    context_.log(QString("Point Report: %1 point(s)").arg(points_.size()), false);
}

} // namespace katana::qt
