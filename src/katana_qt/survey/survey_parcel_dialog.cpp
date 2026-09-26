#include "survey/survey_parcel_dialog.hpp"

#include <QApplication>
#include <QClipboard>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTabWidget>
#include <QTableWidget>
#include <QVBoxLayout>

#include <string>
#include <utility>
#include <variant>

#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

// The label height PARCEL id LABEL takes when it is given none, so the dialog
// and the verb start from the same text.
constexpr double kDefaultLabelHeight = 2.5;

// An entity id as the command line takes it: 12 or #12.
Result<katana::entity::EntityId> entityId(const QString& typed)
{
    QString text = typed.trimmed();
    if (text.startsWith('#')) {
        text.remove(0, 1);
    }
    const auto number = katana::core::parseInteger(text.toStdString());
    if (!number || *number <= 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "Parcel: type the id of a closed polyline, or Use Selection",
                         typed.trimmed().toStdString());
    }
    return static_cast<katana::entity::EntityId>(*number);
}

Status writeText(const QString& path, const std::string& text)
{
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    if (file.write(text.data(), static_cast<qint64>(text.size())) !=
        static_cast<qint64>(text.size())) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    return {};
}

QPushButton* button(const QString& text, const QString& objectName, QWidget* parent)
{
    auto* made = new QPushButton(text, parent);
    made->setObjectName(objectName);
    made->setAutoDefault(false);
    return made;
}

} // namespace

SurveyParcelDialog::SurveyParcelDialog(SurveyDialogContext context, CommandRunner run,
                                       std::function<bool()> headless, QWidget* parent)
    : SurveyToolDialog(std::move(context), "surveyParcelReportDialog", "Parcel Report",
                       "The courses of a closed polyline as quadrant bearings (whole seconds) "
                       "and horizontal distances, its area, perimeter and centroid, and the "
                       "wording a deed carries - what PARCEL id and PARCEL id LEGAL print. "
                       "Label Courses puts a bearing and distance on every course and the area "
                       "at the centroid, on the current layer, as one undo step.",
                       parent),
      run_(std::move(run)), headless_(std::move(headless))
{
    parcel_ = addField("Parcel:", "parcel", "entity id",
                       "The id of a closed polyline, as INFO and LIST give it");
    labelHeight_ = addField("Label height:", "labelHeight", "2.5",
                            "The labels' text height, in model units");
    labelHeight_->setText(QString::number(kDefaultLabelHeight));

    auto* tabs = new QTabWidget(this);
    tabs->setObjectName("parcelTabs");

    auto* coursesPage = new QWidget(tabs);
    auto* coursesLayout = new QVBoxLayout(coursesPage);
    courses_ = new QTableWidget(0, 5, coursesPage);
    courses_->setObjectName("parcelCourses");
    courses_->setHorizontalHeaderLabels({"Course", "From E", "From N", "Bearing", "Distance"});
    courses_->verticalHeader()->setVisible(false);
    courses_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    courses_->setSelectionBehavior(QAbstractItemView::SelectRows);
    courses_->horizontalHeader()->setStretchLastSection(true);
    coursesLayout->addWidget(courses_, 1);
    summary_ = new QLabel(coursesPage);
    summary_->setObjectName("parcelSummary");
    summary_->setWordWrap(true);
    summary_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    coursesLayout->addWidget(summary_);
    auto* coursesButtons = new QHBoxLayout();
    auto* copy = button("Copy", "parcelCopy", coursesPage);
    auto* csv = button("Save CSV...", "parcelCsv", coursesPage);
    coursesButtons->addStretch(1);
    coursesButtons->addWidget(copy);
    coursesButtons->addWidget(csv);
    coursesLayout->addLayout(coursesButtons);
    tabs->addTab(coursesPage, "Courses");

    auto* legalPage = new QWidget(tabs);
    auto* legalLayout = new QVBoxLayout(legalPage);
    auto* nameRow = new QFormLayout();
    name_ = new QLineEdit(legalPage);
    name_->setObjectName("parcelName");
    name_->setPlaceholderText("Parcel <id>");
    name_->setToolTip("The name the description opens with, as PARCEL id LEGAL name takes it");
    nameRow->addRow("Name:", name_);
    legalLayout->addLayout(nameRow);
    legal_ = new QPlainTextEdit(legalPage);
    legal_->setObjectName("parcelLegalText");
    legal_->setReadOnly(true);
    legalLayout->addWidget(legal_, 1);
    auto* legalButtons = new QHBoxLayout();
    auto* legalSave = button("Save...", "parcelLegalSave", legalPage);
    legalButtons->addStretch(1);
    legalButtons->addWidget(legalSave);
    legalLayout->addLayout(legalButtons);
    tabs->addTab(legalPage, "Legal Description");
    form()->addRow(tabs);

    // Each filler goes in at the left, so the last added is first: Use
    // Selection, then Label Courses.
    addFiller("Label Courses", "label", [this] { return label(); });
    addFiller("Use Selection", "useSelection", [this] { return useSelection(); });
    addVerb("Compute", "compute", [this] { return compute(); });

    connect(name_, &QLineEdit::textChanged, this, [this] { showLegal(); });
    connect(copy, &QPushButton::clicked, this, [this] {
        QStringList lines;
        QStringList header;
        for (int column = 0; column < courses_->columnCount(); ++column) {
            header << courses_->horizontalHeaderItem(column)->text();
        }
        lines << header.join('\t');
        for (int row = 0; row < courses_->rowCount(); ++row) {
            QStringList cells;
            for (int column = 0; column < courses_->columnCount(); ++column) {
                cells << courses_->item(row, column)->text();
            }
            lines << cells.join('\t');
        }
        QApplication::clipboard()->setText(lines.join('\n') + '\n');
        setNote(QString("%1 courses copied.").arg(courses_->rowCount()));
    });
    connect(csv, &QPushButton::clicked, this, [this] {
        save("Save Parcel Courses", QString("parcel_%1_courses.csv").arg(reportOf_),
             "CSV (*.csv)", [this](const QString& path) { return saveCoursesCsv(path); });
    });
    connect(legalSave, &QPushButton::clicked, this, [this] {
        save("Save Legal Description", QString("parcel_%1_legal.txt").arg(reportOf_),
             "Text (*.txt)", [this](const QString& path) { return saveLegalText(path); });
    });
    resize(760, 720);
}

Status SurveyParcelDialog::useSelection()
{
    const katana::cad::Document& doc = document();
    const std::vector<katana::entity::EntityId> ids = doc.selection().ids();
    const katana::entity::Entity* entity =
        ids.size() == 1 ? doc.model().entities.find(ids.front()) : nullptr;
    const auto* boundary =
        entity != nullptr ? std::get_if<katana::geometry::Polyline2>(&entity->geometry) : nullptr;
    if (boundary == nullptr || !boundary->closed) {
        return makeError(ErrorCode::InvalidState, "select the one closed polyline to report",
                         std::to_string(ids.size()) + " selected");
    }
    parcel_->setText(QString::number(ids.front()));
    return {};
}

Result<std::string> SurveyParcelDialog::compute()
{
    const auto id = entityId(parcel_->text());
    if (!id) {
        return id.error();
    }
    const katana::entity::Entity* entity = document().model().entities.find(*id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "no entity with that id", std::to_string(*id));
    }
    // The verb's own refusals, in its own words.
    if (std::holds_alternative<katana::geometry::CurvePolyline2>(entity->geometry)) {
        return makeError(ErrorCode::Unsupported,
                         "a parcel with arc courses is not reported yet; its courses are "
                         "bearings and distances of straight sides",
                         std::to_string(*id));
    }
    const auto* boundary = std::get_if<katana::geometry::Polyline2>(&entity->geometry);
    if (boundary == nullptr) {
        return makeError(ErrorCode::InvalidGeometry, "a parcel must be a closed polyline",
                         std::to_string(*id));
    }
    auto report = katana::cad::parcelReport(*boundary);
    if (!report) {
        return report.error();
    }
    report_ = std::move(*report);
    reportOf_ = *id;

    courses_->setRowCount(static_cast<int>(report_->courses.size()));
    for (std::size_t i = 0; i < report_->courses.size(); ++i) {
        const katana::cad::ParcelCourse& course = report_->courses[i];
        const int row = static_cast<int>(i);
        const QStringList cells = {QString::number(i + 1),
                                   QString::number(course.from.x, 'f', 3),
                                   QString::number(course.from.y, 'f', 3), qs(course.bearing),
                                   QString::number(course.distance, 'f', 3)};
        for (int column = 0; column < cells.size(); ++column) {
            courses_->setItem(row, column, new QTableWidgetItem(cells[column]));
        }
    }
    courses_->resizeColumnsToContents();
    summary_->setText(qs(katana::cad::formatParcelSummary(*report_)));
    name_->setPlaceholderText(QString("Parcel %1").arg(*id));
    showLegal();
    return katana::cad::formatParcelReport(*report_);
}

void SurveyParcelDialog::showLegal()
{
    if (!report_) {
        legal_->clear();
        return;
    }
    // PARCEL id LEGAL's default name when none is typed.
    const QString typed = name_->text().trimmed();
    const QString name = typed.isEmpty() ? QString("Parcel %1").arg(reportOf_) : typed;
    legal_->setPlainText(qs(katana::cad::legalDescription(*report_, name.toStdString())));
}

Status SurveyParcelDialog::label()
{
    const auto id = entityId(parcel_->text());
    if (!id) {
        return id.error();
    }
    const QString height = labelHeight_->text().trimmed();
    if (height.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "type the label height");
    }
    if (!run_) {
        return makeError(ErrorCode::InvalidState,
                         "this dialog has no command line to run PARCEL LABEL on");
    }
    const QString line = QString("PARCEL %1 LABEL %2").arg(*id).arg(height);
    const VerbOutcome outcome = run_(line);
    if (!outcome.ok) {
        return makeError(ErrorCode::InvalidArgument,
                         (outcome.error.isEmpty() ? line + " was refused" : outcome.error)
                             .toStdString());
    }
    const QStringList replies = outcome.reply.split('\n', Qt::SkipEmptyParts);
    setNote(replies.isEmpty() ? line : replies.back());
    frameViews();
    return {};
}

void SurveyParcelDialog::save(const QString& title, const QString& suggested,
                              const QString& filter,
                              const std::function<Status(const QString&)>& write)
{
    if (!report_) {
        showError(makeError(ErrorCode::InvalidState, "compute the report first"));
        return;
    }
    if (headless_ && headless_()) {
        showError(makeError(ErrorCode::InvalidState,
                            "a headless session opens no file dialog; PARCEL id and PARCEL id "
                            "LEGAL give the same text"));
        return;
    }
    const QString path = QFileDialog::getSaveFileName(this, title, suggested, filter);
    if (path.isEmpty()) {
        return;
    }
    if (const Status written = write(path); !written) {
        showError(written.error());
        return;
    }
    setNote("Saved to " + QDir::toNativeSeparators(path) + ".");
}

Status SurveyParcelDialog::saveCoursesCsv(const QString& path) const
{
    if (!report_) {
        return makeError(ErrorCode::InvalidState, "compute the report first");
    }
    return writeText(path, katana::cad::parcelCoursesCsv(*report_));
}

Status SurveyParcelDialog::saveLegalText(const QString& path) const
{
    if (!report_) {
        return makeError(ErrorCode::InvalidState, "compute the report first");
    }
    return writeText(path, legal_->toPlainText().toStdString() + "\n");
}

} // namespace katana::qt
