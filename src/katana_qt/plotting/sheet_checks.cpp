#include "plotting/sheet_checks.hpp"

#include <algorithm>
#include <optional>
#include <string>
#include <utility>

#include <QHeaderView>
#include <QLabel>
#include <QPainter>
#include <QPixmap>
#include <QPolygonF>
#include <QShowEvent>
#include <QStatusBar>
#include <QTimer>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "katana/cad/scene.hpp"
#include "katana/geometry/mesh.hpp"
#include "katana/interop/reference_data.hpp"
#include "sheet_editor.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using plotting::Finding;
using plotting::Severity;

namespace {

// Which finding a row shows: its position in findings_.
constexpr int kFindingRole = Qt::UserRole + 1;

enum Column { SeverityColumn, SheetColumn, ViewColumn, ProblemColumn, FixColumn };

// An icon painted in a 16-unit box, at 16 and 32 pixels.
template <typename Paint>
QIcon paintedIcon(Paint&& paint)
{
    QIcon result;
    for (const int size : {16, 32}) {
        QPixmap pixmap(size, size);
        pixmap.fill(Qt::transparent);
        QPainter painter(&pixmap);
        painter.setRenderHint(QPainter::Antialiasing);
        painter.scale(size / 16.0, size / 16.0);
        paint(painter);
        painter.end();
        result.addPixmap(pixmap);
    }
    return result;
}

QPen stroke(const QColor& colour, double width)
{
    QPen pen(colour, width);
    pen.setCapStyle(Qt::RoundCap);
    pen.setJoinStyle(Qt::RoundJoin);
    return pen;
}

QString severityName(Severity severity)
{
    switch (severity) {
    case Severity::Error: return QStringLiteral("Error");
    case Severity::Warning: return QStringLiteral("Warning");
    case Severity::Info: return QStringLiteral("Note");
    }
    return {};
}

// "No problems found." / "2 errors, 1 warning, 3 notes."
QString sentence(const plotting::PreflightSummary& summary)
{
    QString text = QString::fromStdString(plotting::summaryText(summary));
    if (!text.isEmpty()) {
        text[0] = text[0].toUpper();
    }
    return text + '.';
}

// The sheet a finding is on, as the list names it: its number and name, or
// "All sheets" for the set's own.
QString sheetText(const plotting::SheetSet& set, const Finding& finding)
{
    if (!finding.sheetIndex) {
        return QStringLiteral("All sheets");
    }
    const std::size_t index = *finding.sheetIndex;
    if (index >= set.sheets.size() || set.sheets[index].id != finding.sheetId) {
        return QString::fromStdString(finding.sheetId); // the set changed since
    }
    const std::string number = plotting::formatSheetNumber(set.numbering, index + 1, set.sheets.size(),
                                                           set.defaults.setNumber);
    return QString("%1  %2").arg(QString::fromStdString(number),
                                 QString::fromStdString(set.sheets[index].name));
}

// Whether two findings are about the same thing, however their words changed:
// the row kept current across a check.
bool sameSubject(const Finding& a, const Finding& b)
{
    return a.code == b.code && a.sheetId == b.sheetId && a.viewportId == b.viewportId &&
           a.subject == b.subject;
}

} // namespace

// ---- the checker, given what the painter knows ---------------------------------------------

plotting::PreflightOptions preflightOptionsFor(const SheetSource& source)
{
    plotting::PreflightOptions options;
    // The painter's own rule for an automatic plan, so the window checked is
    // the one drawn.
    options.resolvePlan = [&source](const plotting::Viewport& viewport) {
        const ResolvedViewport at = resolvePlanViewport(viewport, source);
        return plotting::PlanWindow{at.scale, at.centre};
    };
    // What a plan draws besides entities and alignments, as planDrawnBounds
    // counts it: visible imagery and point clouds, and every shown mesh's
    // footprint.
    if (source.plan.reference != nullptr) {
        for (const katana::interop::RasterOverlay& raster : source.plan.reference->rasters()) {
            if (raster.visible) {
                options.otherContent.push_back(raster.worldBounds());
            }
        }
        for (const katana::interop::PointCloudLayer& cloud : source.plan.reference->pointClouds()) {
            if (cloud.visible) {
                options.otherContent.push_back(cloud.worldBounds());
            }
        }
    }
    if (source.plan.meshes != nullptr) {
        for (const katana::cad::SceneMesh& item : *source.plan.meshes) {
            if (!item.visible || item.mesh == nullptr || item.mesh->empty() ||
                item.style == katana::cad::SurfaceStyle::Hidden) {
                continue;
            }
            const katana::math::AABB space = item.mesh->bounds();
            if (!space.empty()) {
                options.otherContent.emplace_back(plotting::Point2(space.min.x, space.min.y),
                                                  plotting::Point2(space.max.x, space.max.y));
            }
        }
    }
    // The painter cuts sections from the visible surfaces only.
    options.sectionSurfaces = static_cast<std::size_t>(
        std::count_if(source.surfaces.begin(), source.surfaces.end(), [](const katana::cad::SceneSurface& s) {
            return s.visible && s.surface != nullptr;
        }));
    // The caller loads the logo once for every page; a null image is a file
    // missing or unreadable.
    options.logoReadable = !source.logo.isNull();
    options.assets = source.assets;
    options.index = source.plan.index;
    return options;
}

std::vector<Finding> checkSheetsFor(const plotting::SheetSet& set, const SheetSource& source,
                                    std::span<const std::size_t> sheets)
{
    static const katana::entity::Model kNothing;
    plotting::PreflightOptions options = preflightOptionsFor(source);
    options.sheets.assign(sheets.begin(), sheets.end());
    const katana::entity::Model& model = source.plan.model != nullptr ? *source.plan.model : kNothing;
    if (source.plan.model == nullptr) {
        options.index = nullptr; // an index of some other drawing
    }
    return plotting::checkSheets(set, model, source.fields, options);
}

QStringList preflightLog(std::span<const Finding> findings)
{
    const plotting::PreflightSummary summary = plotting::summarize(findings);
    if (summary.errors == 0 && summary.warnings == 0) {
        return {};
    }
    QStringList lines;
    lines << QString("Sheet checks: %1. The plot goes ahead; Check Sheets in File > Sheets lists them.")
                 .arg(QString::fromStdString(plotting::summaryText(summary)));
    for (const Finding& finding : findings) {
        if (finding.severity == Severity::Error) {
            lines << QString::fromStdString(plotting::findingLine(finding));
        }
    }
    return lines;
}

QIcon checkSheetsIcon()
{
    return paintedIcon([](QPainter& p) {
        // A sheet with a tick on it.
        p.setPen(stroke(QColor(110, 110, 110), 1.0));
        p.setBrush(Qt::white);
        p.drawRect(QRectF(2.5, 1.5, 11.0, 13.0));
        p.setBrush(Qt::NoBrush);
        p.setPen(stroke(QColor(46, 125, 50), 2.0));
        p.drawPolyline(QPolygonF({QPointF(5.0, 8.5), QPointF(7.3, 11.0), QPointF(11.5, 5.0)}));
    });
}

QIcon severityIcon(Severity severity)
{
    switch (severity) {
    case Severity::Error:
        // A red disc with a white cross.
        return paintedIcon([](QPainter& p) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(211, 47, 47));
            p.drawEllipse(QRectF(1.0, 1.0, 14.0, 14.0));
            p.setPen(stroke(Qt::white, 2.0));
            p.drawLine(QPointF(5.5, 5.5), QPointF(10.5, 10.5));
            p.drawLine(QPointF(10.5, 5.5), QPointF(5.5, 10.5));
        });
    case Severity::Warning:
        // An amber triangle with a dark exclamation mark.
        return paintedIcon([](QPainter& p) {
            p.setPen(stroke(QColor(245, 166, 35), 1.0));
            p.setBrush(QColor(245, 166, 35));
            p.drawPolygon(QPolygonF({QPointF(8.0, 1.5), QPointF(15.0, 14.5), QPointF(1.0, 14.5)}));
            p.setPen(stroke(QColor(60, 40, 0), 2.0));
            p.drawLine(QPointF(8.0, 6.0), QPointF(8.0, 9.8));
            p.drawPoint(QPointF(8.0, 12.3));
        });
    case Severity::Info:
        // A blue disc with a white "i".
        return paintedIcon([](QPainter& p) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(25, 118, 210));
            p.drawEllipse(QRectF(1.0, 1.0, 14.0, 14.0));
            p.setPen(stroke(Qt::white, 2.0));
            p.drawPoint(QPointF(8.0, 4.8));
            p.drawLine(QPointF(8.0, 7.5), QPointF(8.0, 11.5));
        });
    }
    return {};
}

// ---- the dock ------------------------------------------------------------------------------

SheetChecksDock::SheetChecksDock(const katana::cad::Document& document, Checker checker,
                                 QWidget* parent)
    : QDockWidget(QStringLiteral("Checks"), parent), document_(document), checker_(std::move(checker))
{
    setObjectName(QStringLiteral("sheetChecksDock"));
    setFeatures(QDockWidget::DockWidgetMovable | QDockWidget::DockWidgetClosable);
    icons_ = {severityIcon(Severity::Error), severityIcon(Severity::Warning),
              severityIcon(Severity::Info)};

    auto* panel = new QWidget(this);
    panel->setObjectName(QStringLiteral("sheetChecksPanel"));
    auto* layout = new QVBoxLayout(panel);
    layout->setContentsMargins(4, 4, 4, 4);
    summary_ = new QLabel(panel);
    summary_->setObjectName(QStringLiteral("sheetChecksSummary"));
    layout->addWidget(summary_);
    list_ = new QTreeWidget(panel);
    list_->setObjectName(QStringLiteral("sheetChecksList"));
    list_->setRootIsDecorated(false);
    list_->setUniformRowHeights(true);
    list_->setAlternatingRowColors(true);
    list_->setSelectionMode(QAbstractItemView::SingleSelection);
    list_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    list_->setHeaderLabels({QString(), QStringLiteral("Sheet"), QStringLiteral("View"),
                            QStringLiteral("Problem"), QStringLiteral("Fix")});
    QHeaderView* header = list_->header();
    header->setStretchLastSection(true);
    for (const int column : {SeverityColumn, SheetColumn, ViewColumn}) {
        header->setSectionResizeMode(column, QHeaderView::ResizeToContents);
    }
    header->setSectionResizeMode(ProblemColumn, QHeaderView::Interactive);
    header->resizeSection(ProblemColumn, 520);
    layout->addWidget(list_);
    setWidget(panel);

    timer_ = new QTimer(this);
    timer_->setObjectName(QStringLiteral("sheetChecksTimer"));
    timer_->setSingleShot(true);
    timer_->setInterval(kRecheckDelayMs);
    connect(timer_, &QTimer::timeout, this, [this] { (void)checkNow(); });
    // A double-click, or Enter on the current row.
    connect(list_, &QTreeWidget::itemActivated, this, [this](QTreeWidgetItem* item, int) {
        activate(static_cast<std::size_t>(item->data(SeverityColumn, kFindingRole).toULongLong()));
    });
    rebuild();
}

const std::vector<Finding>& SheetChecksDock::checkNow()
{
    timer_->stop();
    std::optional<Finding> current;
    if (const QTreeWidgetItem* item = list_->currentItem()) {
        const auto index = static_cast<std::size_t>(item->data(SeverityColumn, kFindingRole).toULongLong());
        if (index < findings_.size()) {
            current = findings_[index];
        }
    }
    findings_ = checker_ ? checker_() : std::vector<Finding>{};
    ++runs_;
    rebuild();
    // The row that was current, found again by what it is about.
    if (current) {
        for (std::size_t index = 0; index < findings_.size(); ++index) {
            if (sameSubject(findings_[index], *current)) {
                list_->setCurrentItem(list_->topLevelItem(rowOf(index)));
                break;
            }
        }
    }
    return findings_;
}

void SheetChecksDock::schedule()
{
    // The editor lives on while it is closed, and the document keeps
    // changing under it: checking every sheet after each edit nobody can see
    // the result of would only slow the drawing down. Marked out of date,
    // and checked once the editor is shown again.
    if (!window()->isVisible()) {
        stale_ = true;
        timer_->stop();
        return;
    }
    timer_->start();
}

void SheetChecksDock::showEvent(QShowEvent* event)
{
    QDockWidget::showEvent(event);
    if (stale_) {
        stale_ = false;
        timer_->start();
    }
}

bool SheetChecksDock::pending() const { return timer_->isActive(); }

void SheetChecksDock::setDelay(int milliseconds) { timer_->setInterval(std::max(milliseconds, 0)); }

int SheetChecksDock::rowOf(std::size_t finding) const
{
    const auto found = std::find(rows_.begin(), rows_.end(), finding);
    return found == rows_.end() ? -1 : static_cast<int>(found - rows_.begin());
}

void SheetChecksDock::activate(std::size_t finding)
{
    if (finding >= findings_.size()) {
        return;
    }
    if (QTreeWidgetItem* item = list_->topLevelItem(rowOf(finding)); item != list_->currentItem()) {
        list_->setCurrentItem(item);
    }
    if (onActivated) {
        // A copy: going to it may edit the sheets and so check them again.
        const Finding copy = findings_[finding];
        onActivated(copy);
    }
}

void SheetChecksDock::rebuild()
{
    const plotting::SheetSet& set = document_.sheetSet();
    list_->clear();
    // Errors first, then warnings, then notes; the checker's order within
    // each, so the list reads sheet by sheet.
    rows_.clear();
    for (const Severity severity : {Severity::Error, Severity::Warning, Severity::Info}) {
        for (std::size_t index = 0; index < findings_.size(); ++index) {
            if (findings_[index].severity == severity) {
                rows_.push_back(index);
            }
        }
    }
    for (const std::size_t index : rows_) {
        const Finding& finding = findings_[index];
        auto* item = new QTreeWidgetItem(list_);
        item->setIcon(SeverityColumn, icons_[static_cast<std::size_t>(finding.severity)]);
        item->setText(SeverityColumn, severityName(finding.severity));
        item->setData(SeverityColumn, kFindingRole, static_cast<qulonglong>(index));
        item->setText(SheetColumn, sheetText(set, finding));
        item->setText(ViewColumn, QString::fromStdString(finding.viewportId));
        item->setText(ProblemColumn, QString::fromStdString(finding.message));
        item->setText(FixColumn, QString::fromStdString(finding.fix));
        const QString tip = QString("%1\n\nFix: %2\n\n%3")
                                .arg(QString::fromStdString(finding.message),
                                     QString::fromStdString(finding.fix),
                                     QString::fromStdString(finding.code));
        for (int column = SeverityColumn; column <= FixColumn; ++column) {
            item->setToolTip(column, tip);
        }
    }
    const plotting::PreflightSummary summary = plotting::summarize(findings_);
    summary_->setText(runs_ == 0 ? QStringLiteral("Not checked yet.") : sentence(summary));
    setWindowTitle(summary.errors + summary.warnings == 0
                       ? QStringLiteral("Checks")
                       : QString("Checks (%1)").arg(QString::fromStdString(plotting::summaryText(
                             plotting::PreflightSummary{summary.errors, summary.warnings, 0}))));
}

// ---- the editor's side ---------------------------------------------------------------------
// SheetEditor's checks are defined here, beside the dock they drive.

const std::vector<Finding>& SheetEditor::checkSheets()
{
    const std::vector<Finding>& findings = checks_->checkNow();
    checks_->show();
    checks_->raise();
    const std::size_t sheets = document_.sheetSet().sheets.size();
    report(QString("Checked %1 sheet%2: %3")
               .arg(sheets)
               .arg(sheets == 1 ? "" : "s")
               .arg(sentence(plotting::summarize(findings))));
    return findings;
}

void SheetEditor::showFinding(const Finding& finding)
{
    const plotting::SheetSet& set = document_.sheetSet();
    // By its position while that is still the sheet it was found on (two
    // sheets may share an id: that is a finding too), else by its id.
    std::optional<std::size_t> at;
    if (finding.sheetIndex && *finding.sheetIndex < set.sheets.size() &&
        set.sheets[*finding.sheetIndex].id == finding.sheetId) {
        at = finding.sheetIndex;
    } else if (!finding.sheetId.empty()) {
        at = plotting::sheetIndex(set, finding.sheetId);
    }
    if (at) {
        setCurrentSheet(*at);
        canvas_->select(finding.viewportId);
        canvas_->setFocus();
    }
    QString text = QString::fromStdString(finding.message) + '.';
    if (!finding.fix.empty()) {
        text += ' ' + QString::fromStdString(finding.fix) + '.';
    }
    statusBar()->showMessage(text, 12000);
}

} // namespace katana::qt
