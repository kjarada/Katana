#include "terrain/volumes_dialog.hpp"

#include <algorithm>
#include <cmath>
#include <optional>
#include <string>
#include <utility>

#include <QAbstractItemView>
#include <QCheckBox>
#include <QClipboard>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFormLayout>
#include <QGroupBox>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPainterPath>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QTabWidget>
#include <QTableWidget>
#include <QVBoxLayout>

#include "geo/geo_dialog_support.hpp"
#include "icons.hpp"
#include "katana/core/text.hpp"
#include "theme.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// A number as a line takes it back: the shortest text that reads as it.
QString number(double value)
{
    return QString::fromStdString(katana::core::formatExactReal(value));
}

QComboBox* nameBox(QWidget* parent, const char* name, const QString& tip)
{
    auto* box = new QComboBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    box->setToolTip(tip);
    box->setMinimumContentsLength(16);
    return box;
}

QDoubleSpinBox* numberBox(QWidget* parent, const char* name, double minimum, double maximum,
                          double value, int decimals, const QString& suffix, const QString& tip)
{
    auto* box = new QDoubleSpinBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    box->setRange(minimum, maximum);
    box->setDecimals(decimals);
    box->setValue(value);
    box->setSuffix(suffix);
    box->setToolTip(tip);
    return box;
}

QLineEdit* textBox(QWidget* parent, const char* name, const QString& placeholder,
                   const QString& tip)
{
    auto* edit = new QLineEdit(parent);
    edit->setObjectName(QString::fromLatin1(name));
    edit->setPlaceholderText(placeholder);
    edit->setToolTip(tip);
    return edit;
}

// The surface (or alignment) a list has chosen, as one word of a line.
Result<QString> chosen(const QComboBox* box, const QString& field, const QString& missing)
{
    if (box->currentText().isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, (field + ": " + missing).toStdString());
    }
    return lineWord(box->currentText(), field);
}

// Refilled with `names`, keeping the choice where it is still offered; a
// second list of two (the top) starts on the second name, so the two do not
// begin as one surface measured against itself.
void refill(QComboBox* box, const QStringList& names, bool second)
{
    const QString kept = box->currentText();
    const bool blocked = box->blockSignals(true);
    box->clear();
    box->addItems(names);
    const int at = names.indexOf(kept);
    if (at >= 0) {
        box->setCurrentIndex(at);
    } else if (second && names.size() > 1) {
        box->setCurrentIndex(1);
    }
    box->blockSignals(blocked);
}

// "x,y" or "x,y,z": a corner as a line types it.
bool isCorner(const QString& word)
{
    const QStringList parts = word.split(',');
    if (parts.size() != 2 && parts.size() != 3) {
        return false;
    }
    return std::all_of(parts.begin(), parts.end(), [](const QString& part) {
        return katana::core::parseFiniteDouble(part.trimmed().toStdString()).has_value();
    });
}

// key=value of a field left blank for the verb's default, or a number.
Result<std::optional<QString>> optionalNumber(const QLineEdit* edit, const QString& key,
                                              const QString& field)
{
    const QString text = edit->text().trimmed();
    if (text.isEmpty()) {
        return std::optional<QString>{};
    }
    const auto value = katana::core::parseFiniteDouble(text.toStdString());
    if (!value) {
        return makeError(ErrorCode::ParseFailure,
                         (field + " is a number, or blank for the default").toStdString());
    }
    return std::optional<QString>(key + "=" + number(*value));
}

} // namespace

// ---- the results -------------------------------------------------------------------------------

VolumeResultTable volumeResultTable(const QString& reply)
{
    VolumeResultTable table;
    table.columns << QStringLiteral("record");
    std::vector<katana::core::ReplyRecord> records;
    for (const QString& line : reply.split('\n')) {
        const auto record = katana::core::readReplyRecord(line.trimmed().toStdString());
        if (!record || (record->words.empty() && record->fields.empty())) {
            continue;
        }
        for (const auto& [key, value] : record->fields) {
            (void)value;
            const QString column = QString::fromStdString(key);
            if (!table.columns.contains(column)) {
                table.columns << column;
            }
        }
        records.push_back(*record);
    }
    for (const auto& record : records) {
        QStringList row;
        QStringList words;
        for (const std::string& word : record.words) {
            words << QString::fromStdString(word);
        }
        row << words.join(' ');
        for (qsizetype c = 1; c < table.columns.size(); ++c) {
            const auto value = record.value(table.columns[c].toStdString());
            row << (value ? QString::fromStdString(*value) : QString());
        }
        table.rows.push_back(row);
    }
    return table;
}

// ---- the mass haul diagram ---------------------------------------------------------------------

MassHaulChart::MassHaulChart(QWidget* parent) : QWidget(parent)
{
    setObjectName(QStringLiteral("volumesMassHaul"));
    setMinimumHeight(140);
    setToolTip(QStringLiteral("The mass haul diagram: cut less fill summed from the start of "
                              "the range, against station. Rising: a surplus of cut; falling: "
                              "fill to be brought."));
}

void MassHaulChart::setPoints(std::vector<QPointF> points)
{
    points_ = std::move(points);
    update();
}

void MassHaulChart::paintEvent(QPaintEvent* /*event*/)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.fillRect(rect(), palette().base());
    const QRectF frame = QRectF(rect()).adjusted(48, 10, -12, -22);
    painter.setPen(theme::text());
    if (points_.size() < 2 || !(frame.width() > 0.0) || !(frame.height() > 0.0)) {
        painter.drawText(rect(), Qt::AlignCenter,
                         QStringLiteral("Measure along an alignment for its mass haul diagram"));
        return;
    }
    double low = 0.0;
    double high = 0.0;
    for (const QPointF& point : points_) {
        low = std::min(low, point.y());
        high = std::max(high, point.y());
    }
    if (!(high > low)) {
        high = low + 1.0; // all zero: a flat line on the axis
    }
    const double first = points_.front().x();
    const double last = std::max(points_.back().x(), first + 1e-9);
    const auto place = [&](const QPointF& point) {
        return QPointF(frame.left() + (point.x() - first) / (last - first) * frame.width(),
                       frame.bottom() - (point.y() - low) / (high - low) * frame.height());
    };
    // The zero line: where the haul balances.
    QPen zero(theme::text(), 1.0, Qt::DashLine);
    painter.setPen(zero);
    const double zeroY = place(QPointF(first, 0.0)).y();
    painter.drawLine(QPointF(frame.left(), zeroY), QPointF(frame.right(), zeroY));
    QPainterPath curve(place(points_.front()));
    for (std::size_t i = 1; i < points_.size(); ++i) {
        curve.lineTo(place(points_[i]));
    }
    painter.setPen(QPen(theme::accent(), 2.0));
    painter.drawPath(curve);
    painter.setPen(theme::text());
    const auto label = [](double value) { return QString::number(value, 'f', 0); };
    painter.drawText(QRectF(0, frame.top() - 6, 44, 14), Qt::AlignRight, label(high));
    painter.drawText(QRectF(0, frame.bottom() - 8, 44, 14), Qt::AlignRight, label(low));
    painter.drawText(QRectF(frame.left(), frame.bottom() + 4, 80, 16), Qt::AlignLeft,
                     label(first));
    painter.drawText(QRectF(frame.right() - 80, frame.bottom() + 4, 80, 16), Qt::AlignRight,
                     label(last));
}

// ---- the dialog --------------------------------------------------------------------------------

VolumesDialog::VolumesDialog(VolumesDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("volumesDialog"));
    setWindowTitle(QStringLiteral("Volumes"));
    setModal(false);
    auto* layout = new QVBoxLayout(this);

    const QString baseTip = QStringLiteral("The existing surface: cut is where it is above the top");
    const QString topTip = QStringLiteral("The design surface: fill is where it is above the base");
    methods_ = new QTabWidget(this);
    methods_->setObjectName(QStringLiteral("volumesMethods"));

    // Surface to Surface.
    auto* surfacesPage = new QWidget(methods_);
    surfacesPage->setObjectName(QStringLiteral("volumesSurfacesPage"));
    auto* surfacesForm = new QFormLayout(surfacesPage);
    base_ = nameBox(surfacesPage, "volumesBase", baseTip);
    top_ = nameBox(surfacesPage, "volumesTop", topTip);
    surfacesForm->addRow(QStringLiteral("&Base:"), base_);
    surfacesForm->addRow(QStringLiteral("&Top:"), top_);
    methods_->addTab(surfacesPage, QStringLiteral("Surface to Surface"));

    // Surface to Level.
    auto* levelPage = new QWidget(methods_);
    levelPage->setObjectName(QStringLiteral("volumesLevelPage"));
    auto* levelForm = new QFormLayout(levelPage);
    levelSurface_ = nameBox(levelPage, "volumesLevelSurface",
                            QStringLiteral("The surface measured against the level"));
    level_ = numberBox(levelPage, "volumesLevel", -100000.0, 100000.0, 0.0, 3, QStringLiteral(" m"),
                       QStringLiteral("The level: what stands above it is cut, what lies below "
                                      "it is fill"));
    levelForm->addRow(QStringLiteral("&Surface:"), levelSurface_);
    levelForm->addRow(QStringLiteral("&Level:"), level_);
    methods_->addTab(levelPage, QStringLiteral("Surface to Level"));

    // Stockpile.
    auto* stockpilePage = new QWidget(methods_);
    stockpilePage->setObjectName(QStringLiteral("volumesStockpilePage"));
    auto* stockpileForm = new QFormLayout(stockpilePage);
    stockpileSurface_ = nameBox(stockpilePage, "volumesStockpileSurface",
                                QStringLiteral("The surface the pile (or pit) is part of"));
    stockpileBase_ = new QComboBox(stockpilePage);
    stockpileBase_->setObjectName(QStringLiteral("volumesStockpileBase"));
    stockpileBase_->addItem(QStringLiteral("The boundary's heights"), QStringLiteral("boundary"));
    stockpileBase_->addItem(QStringLiteral("The lowest point on the boundary"),
                            QStringLiteral("lowest"));
    stockpileBase_->addItem(QStringLiteral("A level"), QStringLiteral("level"));
    stockpileBase_->setToolTip(QStringLiteral(
        "What the pile is measured above: its toe's own heights (or the surface's on it), "
        "a level plane at the lowest point of the surface along the boundary, or a level"));
    stockpileLevel_ = numberBox(stockpilePage, "volumesStockpileLevel", -100000.0, 100000.0, 0.0,
                                3, QStringLiteral(" m"), QStringLiteral("The base's level"));
    stockpileForm->addRow(QStringLiteral("&Surface:"), stockpileSurface_);
    stockpileForm->addRow(QStringLiteral("&Base:"), stockpileBase_);
    stockpileForm->addRow(QStringLiteral("&Level:"), stockpileLevel_);
    stockpileForm->addRow(new QLabel(
        QStringLiteral("The boundary below is the pile's toe: a polygon is required."),
        stockpilePage));
    methods_->addTab(stockpilePage, QStringLiteral("Stockpile"));

    // Along Alignment.
    auto* sectionsPage = new QWidget(methods_);
    sectionsPage->setObjectName(QStringLiteral("volumesSectionsPage"));
    auto* sectionsLayout = new QVBoxLayout(sectionsPage);
    auto* sectionsForm = new QFormLayout();
    sectionsBase_ = nameBox(sectionsPage, "volumesSectionsBase", baseTip);
    sectionsTop_ = nameBox(sectionsPage, "volumesSectionsTop", topTip);
    alignment_ = nameBox(sectionsPage, "volumesAlignment",
                         QStringLiteral("The alignment the sections are cut square to"));
    interval_ = numberBox(sectionsPage, "volumesInterval", 0.01, 100000.0, 10.0, 3,
                          QStringLiteral(" m"),
                          QStringLiteral("Sections at every whole multiple of this station, "
                                         "and at the alignment's key stations"));
    from_ = textBox(sectionsPage, "volumesFrom", QStringLiteral("the start"),
                    QStringLiteral("The first station; blank for the alignment's start"));
    to_ = textBox(sectionsPage, "volumesTo", QStringLiteral("the end"),
                  QStringLiteral("The last station; blank for the alignment's end"));
    const QString reachTip = QStringLiteral(
        "%1. Blank reaches across all the ground the surfaces share, which is right on a "
        "straight run only: on a curve or a corner give a reach under the curve's radius, "
        "because sections that cross one another count ground twice and the line is refused.");
    left_ = textBox(sectionsPage, "volumesLeft", QStringLiteral("across the common ground"),
                    reachTip.arg(QStringLiteral("How far each section reaches left of the centreline")));
    right_ = textBox(sectionsPage, "volumesRight", QStringLiteral("across the common ground"),
                     reachTip.arg(QStringLiteral("How far each section reaches right of the centreline")));
    sectionMethod_ = new QComboBox(sectionsPage);
    sectionMethod_->setObjectName(QStringLiteral("volumesSectionMethod"));
    sectionMethod_->addItem(QStringLiteral("Average end area"), QStringLiteral("endarea"));
    sectionMethod_->addItem(QStringLiteral("Prismoidal"), QStringLiteral("prismoidal"));
    sectionMethod_->addItem(QStringLiteral("Both, with the prismoidal correction"),
                            QStringLiteral("both"));
    sectionMethod_->setToolTip(QStringLiteral(
        "Average end area: the mean of two sections' areas times the distance between them. "
        "Prismoidal: with a section cut halfway, exact where the area changes as a cubic"));
    sectionsForm->addRow(QStringLiteral("&Base:"), sectionsBase_);
    sectionsForm->addRow(QStringLiteral("&Top:"), sectionsTop_);
    sectionsForm->addRow(QStringLiteral("&Alignment:"), alignment_);
    sectionsForm->addRow(QStringLiteral("&Interval:"), interval_);
    sectionsForm->addRow(QStringLiteral("&From station:"), from_);
    sectionsForm->addRow(QStringLiteral("T&o station:"), to_);
    sectionsForm->addRow(QStringLiteral("&Left:"), left_);
    sectionsForm->addRow(QStringLiteral("Ri&ght:"), right_);
    sectionsForm->addRow(QStringLiteral("Metho&d:"), sectionMethod_);
    sectionsLayout->addLayout(sectionsForm);
    chart_ = new MassHaulChart(sectionsPage);
    sectionsLayout->addWidget(chart_);
    methods_->addTab(sectionsPage, QStringLiteral("Along Alignment"));

    // Grid.
    auto* gridPage = new QWidget(methods_);
    gridPage->setObjectName(QStringLiteral("volumesGridPage"));
    auto* gridForm = new QFormLayout(gridPage);
    gridBase_ = nameBox(gridPage, "volumesGridBase", baseTip);
    gridTop_ = nameBox(gridPage, "volumesGridTop", topTip);
    cell_ = numberBox(gridPage, "volumesCell", 0.001, 10000.0, 1.0, 3, QStringLiteral(" m"),
                      QStringLiteral("The side of a cell; each is counted at its centre"));
    gridForm->addRow(QStringLiteral("&Base:"), gridBase_);
    gridForm->addRow(QStringLiteral("&Top:"), gridTop_);
    gridForm->addRow(QStringLiteral("Ce&ll:"), cell_);
    gridForm->addRow(new QLabel(QStringLiteral("A cross-check of the exact methods: the "
                                               "midpoint rule on square cells."),
                                gridPage));
    methods_->addTab(gridPage, QStringLiteral("Grid"));
    // The tabs are as tall as the page shown, not as the tallest page: a tab
    // widget keeps room for every page, and Along Alignment's mass haul chart
    // made each of the others a tall empty block - the dialog some 1550 px,
    // taller than a laptop's screen. A page not shown is Ignored, which a
    // stacked layout leaves out of its size (fitMethodPage); the height left
    // goes to the results table (stretch 1 below).
    methods_->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum);
    layout->addWidget(methods_);

    // The boundary: none, the shared scope and filter, or typed corners.
    boundary_ = new QGroupBox(QStringLiteral("Inside"), this);
    boundary_->setObjectName(QStringLiteral("volumesBoundaryGroup"));
    auto* boundaryLayout = new QVBoxLayout(boundary_);
    auto* choices = new QHBoxLayout();
    boundaryNone_ = new QRadioButton(QStringLiteral("Everywhere"), boundary_);
    boundaryNone_->setObjectName(QStringLiteral("volumesBoundaryNone"));
    boundaryNone_->setToolTip(QStringLiteral("All the ground the surfaces have in common"));
    boundaryScope_ = new QRadioButton(QStringLiteral("The polygons below"), boundary_);
    boundaryScope_->setObjectName(QStringLiteral("volumesBoundaryScope"));
    boundaryScope_->setToolTip(QStringLiteral(
        "The closed polylines, curve polylines, circles and ellipses the scope takes; lines "
        "and points are counted, not used"));
    boundaryCorners_ = new QRadioButton(QStringLiteral("Typed corners"), boundary_);
    boundaryCorners_->setObjectName(QStringLiteral("volumesBoundaryCorners"));
    boundaryCorners_->setToolTip(QStringLiteral("One polygon, its corners typed"));
    boundaryNone_->setChecked(true);
    choices->addWidget(boundaryNone_);
    choices->addWidget(boundaryScope_);
    choices->addWidget(boundaryCorners_);
    choices->addStretch();
    each_ = new QCheckBox(QStringLiteral("A record for ea&ch polygon"), boundary_);
    each_->setObjectName(QStringLiteral("volumesEach"));
    each_->setToolTip(QStringLiteral(
        "Each polygon measured and reported on its own as well as in the total; without it, "
        "polygons that overlap are refused"));
    choices->addWidget(each_);
    boundaryLayout->addLayout(choices);
    corners_ = textBox(boundary_, "volumesCorners", QStringLiteral("x,y x,y x,y ..."),
                       QStringLiteral("Three or more corners, x,y or x,y,z, separated by blanks"));
    boundaryLayout->addWidget(corners_);
    scope_ = new ScopeFilterWidget(QStringLiteral("volumes"), boundary_);
    if (context_.views) {
        scope_->views = context_.views;
    }
    boundaryLayout->addWidget(scope_);
    layout->addWidget(boundary_);

    // The factors, applied in the reply only.
    auto* factors = new QHBoxLayout();
    bulk_ = numberBox(this, "volumesBulk", 0.01, 10.0, 1.0, 3, QString(),
                      QStringLiteral("Multiplies the cut in the reply (bank to loose, say); "
                                     "never the surfaces"));
    shrink_ = numberBox(this, "volumesShrink", 0.01, 10.0, 1.0, 3, QString(),
                        QStringLiteral("Multiplies the fill in the reply (compacted to bank, "
                                       "say); never the surfaces"));
    auto* bulkLabel = new QLabel(QStringLiteral("B&ulk (cut x):"), this);
    bulkLabel->setBuddy(bulk_);
    auto* shrinkLabel = new QLabel(QStringLiteral("S&hrink (fill x):"), this);
    shrinkLabel->setBuddy(shrink_);
    factors->addWidget(bulkLabel);
    factors->addWidget(bulk_);
    factors->addWidget(shrinkLabel);
    factors->addWidget(shrink_);
    factors->addStretch();
    layout->addLayout(factors);

    // The line, as it will run.
    auto* lineRow = new QHBoxLayout();
    command_ = new QLineEdit(this);
    command_->setObjectName(QStringLiteral("volumesCommand"));
    command_->setReadOnly(true);
    command_->setToolTip(QStringLiteral("The line Measure runs, exactly as typed on the command line"));
    auto* measureButton = new QPushButton(QStringLiteral("&Measure"), this);
    measureButton->setObjectName(QStringLiteral("volumesRun"));
    measureButton->setDefault(true);
    lineRow->addWidget(command_, 1);
    lineRow->addWidget(measureButton);
    layout->addLayout(lineRow);

    results_ = new QTableWidget(this);
    results_->setObjectName(QStringLiteral("volumesResults"));
    results_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    results_->horizontalHeader()->setStretchLastSection(true);
    results_->setMinimumHeight(140);
    layout->addWidget(results_, 1);

    auto* fileRow = new QHBoxLayout();
    auto* copy = new QPushButton(QStringLiteral("Cop&y"), this);
    copy->setObjectName(QStringLiteral("volumesCopy"));
    copy->setToolTip(QStringLiteral("The table on the clipboard, tab-separated with its header"));
    csvPath_ = textBox(this, "volumesCsvPath", QStringLiteral("file.csv"),
                       QStringLiteral("The file Save CSV writes; it is replaced"));
    auto* browse = new QPushButton(QStringLiteral("Bro&wse..."), this);
    browse->setObjectName(QStringLiteral("volumesCsvBrowse"));
    auto* save = new QPushButton(QStringLiteral("Sa&ve CSV"), this);
    save->setObjectName(QStringLiteral("volumesSaveCsv"));
    save->setToolTip(QStringLiteral("The line run again with csv= the file: the records as a table"));
    fileRow->addWidget(copy);
    fileRow->addWidget(csvPath_, 1);
    fileRow->addWidget(browse);
    fileRow->addWidget(save);
    layout->addLayout(fileRow);

    auto* mapRow = new QHBoxLayout();
    mapName_ = textBox(this, "volumesMapName", QStringLiteral("top-base"),
                       QStringLiteral("The cut/fill map's name; blank for top-base"));
    auto* map = new QPushButton(QStringLiteral("Ma&ke Cut/Fill Map"), this);
    map->setObjectName(QStringLiteral("volumesCutFillMap"));
    map->setIcon(icon(Icon::CutFillMap));
    map->setToolTip(QStringLiteral(
        "The top minus the base as a new surface in the Cut Fill style: SURFACE DIFFERENCE"));
    auto* mapLabel = new QLabel(QStringLiteral("Map &name:"), this);
    mapLabel->setBuddy(mapName_);
    mapRow->addWidget(mapLabel);
    mapRow->addWidget(mapName_, 1);
    mapRow->addWidget(map);
    layout->addLayout(mapRow);

    status_ = new QLabel(this);
    status_->setObjectName(QStringLiteral("volumesStatus"));
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(status_);
    reply_ = new QPlainTextEdit(this);
    reply_->setObjectName(QStringLiteral("volumesReply"));
    reply_->setReadOnly(true);
    reply_->setMaximumHeight(90);
    reply_->setFont(theme::monospaceFont());
    layout->addWidget(reply_);

    auto* buttons = new QDialogButtonBox(this);
    QPushButton* close = buttons->addButton(QDialogButtonBox::Close);
    close->setObjectName(QStringLiteral("volumesClose"));
    layout->addWidget(buttons);

    connect(measureButton, &QPushButton::clicked, this, [this] { (void)measure(); });
    connect(copy, &QPushButton::clicked, this,
            [this] { QGuiApplication::clipboard()->setText(resultsText()); });
    connect(save, &QPushButton::clicked, this, [this] { (void)saveCsv(); });
    connect(map, &QPushButton::clicked, this, [this] { (void)makeCutFillMap(); });
    connect(close, &QPushButton::clicked, this, &QDialog::close);
    connect(browse, &QPushButton::clicked, this, [this] {
        if (context_.headless && context_.headless()) {
            setStatus(QStringLiteral("Type the file in the CSV field: a headless run opens no "
                                     "file dialog."),
                      true);
            return;
        }
        const QString file = QFileDialog::getSaveFileName(
            this, QStringLiteral("Save Volumes as CSV"), csvPath_->text(),
            QStringLiteral("CSV files (*.csv)"));
        if (!file.isEmpty()) {
            csvPath_->setText(file);
        }
    });

    // Every edit shows the line it makes.
    for (QComboBox* box : {base_, top_, levelSurface_, stockpileSurface_, stockpileBase_,
                           sectionsBase_, sectionsTop_, alignment_, sectionMethod_, gridBase_,
                           gridTop_}) {
        connect(box, &QComboBox::currentIndexChanged, this, [this] { refreshLine(); });
    }
    for (QDoubleSpinBox* box : {level_, stockpileLevel_, interval_, cell_, bulk_, shrink_}) {
        connect(box, &QDoubleSpinBox::valueChanged, this, [this] { refreshLine(); });
    }
    for (QLineEdit* edit : {from_, to_, left_, right_, corners_, mapName_}) {
        connect(edit, &QLineEdit::textChanged, this, [this] { refreshLine(); });
    }
    for (QRadioButton* radio : {boundaryNone_, boundaryScope_, boundaryCorners_}) {
        connect(radio, &QRadioButton::toggled, this, [this] { refreshLine(); });
    }
    connect(each_, &QCheckBox::toggled, this, [this] { refreshLine(); });
    const auto fitMethodPage = [this] {
        for (int index = 0; index < methods_->count(); ++index) {
            methods_->widget(index)->setSizePolicy(
                QSizePolicy::Preferred,
                index == methods_->currentIndex() ? QSizePolicy::Preferred : QSizePolicy::Ignored);
        }
        methods_->updateGeometry();
    };
    fitMethodPage();
    connect(methods_, &QTabWidget::currentChanged, this, [this, fitMethodPage] {
        fitMethodPage();
        refreshLine();
    });
    scope_->onChanged = [this] { refreshLine(); };

    resize(760, 860);
    reload();
}

void VolumesDialog::reload()
{
    QStringList surfaces;
    QStringList alignments;
    if (context_.document != nullptr) {
        for (const auto& surface : context_.document->surfaces().all()) {
            surfaces << QString::fromStdString(surface.name);
        }
        for (const std::string& name : context_.document->model().alignments.names()) {
            alignments << QString::fromStdString(name);
        }
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
    for (QComboBox* box : {base_, levelSurface_, stockpileSurface_, sectionsBase_, gridBase_}) {
        refill(box, surfaces, false);
    }
    for (QComboBox* box : {top_, sectionsTop_, gridTop_}) {
        refill(box, surfaces, true);
    }
    refill(alignment_, alignments, false);
    refreshLine();
}

void VolumesDialog::setMethod(VolumeMethod method)
{
    methods_->setCurrentIndex(static_cast<int>(method));
}

VolumeMethod VolumesDialog::method() const
{
    return static_cast<VolumeMethod>(std::clamp(methods_->currentIndex(), 0, 4));
}

Result<QString> VolumesDialog::boundaryWords(bool required, bool each) const
{
    QString words;
    if (boundaryNone_->isChecked()) {
        if (required) {
            return makeError(ErrorCode::InvalidArgument,
                             "Inside: a stockpile is measured inside its boundary; choose the "
                             "polygons or type its corners");
        }
        return words;
    }
    if (boundaryScope_->isChecked()) {
        auto scope = scope_->verbWords();
        if (!scope) {
            return scope.error();
        }
        words = QStringLiteral("INSIDE FROM ") + *scope;
    } else {
        const QStringList corners =
            corners_->text().split(QChar(' '), Qt::SkipEmptyParts);
        if (corners.size() < 3 || !std::all_of(corners.begin(), corners.end(), isCorner)) {
            return makeError(ErrorCode::ParseFailure,
                             "Corners: three or more x,y or x,y,z, separated by blanks");
        }
        words = QStringLiteral("INSIDE ") + corners.join(' ');
    }
    if (each && each_->isChecked()) {
        words += QStringLiteral(" EACH");
    }
    return words;
}

QString VolumesDialog::factorWords() const
{
    // A factor of 1 changes nothing, so it is left off the line.
    QString words;
    if (bulk_->value() != 1.0) {
        words += QStringLiteral(" bulk=") + number(bulk_->value());
    }
    if (shrink_->value() != 1.0) {
        words += QStringLiteral(" shrink=") + number(shrink_->value());
    }
    return words;
}

Result<QString> VolumesDialog::line() const
{
    const QString noSurface =
        QStringLiteral("the drawing has no surface yet; make one with Terrain > Surface From");
    QStringList words{QStringLiteral("VOLUME")};
    std::optional<Result<QString>> inside;
    switch (method()) {
    case VolumeMethod::Surfaces: {
        auto base = chosen(base_, QStringLiteral("Base"), noSurface);
        auto top = chosen(top_, QStringLiteral("Top"), noSurface);
        if (!base || !top) {
            return base ? top.error() : base.error();
        }
        words << QStringLiteral("SURFACES") << *base << *top;
        inside = boundaryWords(false, true);
        break;
    }
    case VolumeMethod::Level: {
        auto surface = chosen(levelSurface_, QStringLiteral("Surface"), noSurface);
        if (!surface) {
            return surface.error();
        }
        words << QStringLiteral("DATUM") << *surface << number(level_->value());
        inside = boundaryWords(false, true);
        break;
    }
    case VolumeMethod::Stockpile: {
        auto surface = chosen(stockpileSurface_, QStringLiteral("Surface"), noSurface);
        if (!surface) {
            return surface.error();
        }
        words << QStringLiteral("STOCKPILE") << *surface;
        inside = boundaryWords(true, true);
        break;
    }
    case VolumeMethod::Sections: {
        auto base = chosen(sectionsBase_, QStringLiteral("Base"), noSurface);
        auto top = chosen(sectionsTop_, QStringLiteral("Top"), noSurface);
        if (!base || !top) {
            return base ? top.error() : base.error();
        }
        auto alignment = chosen(alignment_, QStringLiteral("Alignment"),
                                QStringLiteral("the drawing has no alignment yet; define one in "
                                               "Terrain > Alignment Manager"));
        if (!alignment) {
            return alignment.error();
        }
        words << QStringLiteral("SECTIONS") << *base << *top << QStringLiteral("ALIGNMENT")
              << *alignment << QStringLiteral("interval=") + number(interval_->value());
        const std::pair<const QLineEdit*, QString> optional[] = {
            {from_, QStringLiteral("from")},
            {to_, QStringLiteral("to")},
            {left_, QStringLiteral("left")},
            {right_, QStringLiteral("right")}};
        for (const auto& [edit, key] : optional) {
            auto word = optionalNumber(edit, key, key.at(0).toUpper() + key.mid(1));
            if (!word) {
                return word.error();
            }
            if (*word) {
                words << **word;
            }
        }
        const QString method = sectionMethod_->currentData().toString();
        if (method != QStringLiteral("endarea")) {
            words << QStringLiteral("method=") + method;
        }
        break;
    }
    case VolumeMethod::Grid: {
        auto base = chosen(gridBase_, QStringLiteral("Base"), noSurface);
        auto top = chosen(gridTop_, QStringLiteral("Top"), noSurface);
        if (!base || !top) {
            return base ? top.error() : base.error();
        }
        words << QStringLiteral("GRID") << *base << *top
              << QStringLiteral("cell=") + number(cell_->value());
        inside = boundaryWords(false, false);
        break;
    }
    }
    if (inside) {
        if (!*inside) {
            return inside->error();
        }
        if (!(*inside)->isEmpty()) {
            words << **inside;
        }
    }
    if (method() == VolumeMethod::Stockpile) {
        const QString base = stockpileBase_->currentData().toString();
        if (base == QStringLiteral("lowest")) {
            words << QStringLiteral("base=lowest");
        } else if (base == QStringLiteral("level")) {
            words << QStringLiteral("level=") + number(stockpileLevel_->value());
        }
    }
    return words.join(' ') + factorWords();
}

Result<QString> VolumesDialog::cutFillMapLine() const
{
    const QComboBox* base = base_;
    const QComboBox* top = top_;
    if (method() == VolumeMethod::Sections) {
        base = sectionsBase_;
        top = sectionsTop_;
    } else if (method() == VolumeMethod::Grid) {
        base = gridBase_;
        top = gridTop_;
    }
    const QString noSurface =
        QStringLiteral("the drawing has no surface yet; make one with Terrain > Surface From");
    auto baseWord = chosen(base, QStringLiteral("Base"), noSurface);
    auto topWord = chosen(top, QStringLiteral("Top"), noSurface);
    if (!baseWord || !topWord) {
        return baseWord ? topWord.error() : baseWord.error();
    }
    QString text = QStringLiteral("SURFACE DIFFERENCE ") + *baseWord + ' ' + *topWord;
    if (!mapName_->text().trimmed().isEmpty()) {
        auto name = lineWord(mapName_->text().trimmed(), QStringLiteral("Map name"));
        if (!name) {
            return name.error();
        }
        text += QStringLiteral(" NAME ") + *name;
    }
    return text;
}

void VolumesDialog::refreshLine()
{
    updateEnabled();
    const auto made = line();
    command_->setText(made ? *made : QString());
    command_->setToolTip(made ? QStringLiteral("The line Measure runs")
                              : QString::fromStdString(made.error().message));
}

void VolumesDialog::updateEnabled()
{
    const VolumeMethod showing = method();
    boundary_->setEnabled(showing != VolumeMethod::Sections);
    const bool polygons = !boundaryNone_->isChecked();
    each_->setEnabled(polygons && showing != VolumeMethod::Grid);
    corners_->setEnabled(boundaryCorners_->isChecked());
    scope_->setEnabled(boundaryScope_->isChecked());
    stockpileLevel_->setEnabled(stockpileBase_->currentData().toString() ==
                                QStringLiteral("level"));
}

void VolumesDialog::setStatus(const QString& text, bool isError)
{
    status_->setText(text);
    status_->setStyleSheet(isError ? QStringLiteral("color: %1;").arg(theme::error().name())
                                   : QString());
}

void VolumesDialog::showReply(const VerbOutcome& outcome)
{
    QString text = outcome.ok ? outcome.reply : outcome.error;
    if (!outcome.ok && !outcome.reply.isEmpty()) {
        text = outcome.reply + '\n' + outcome.error;
    }
    reply_->setPlainText(text.trimmed());
    if (!outcome.ok) {
        setStatus(outcome.error.trimmed().isEmpty() ? QStringLiteral("The line was refused.")
                                                    : outcome.error.trimmed(),
                  true);
        return;
    }
    table_ = volumeResultTable(outcome.reply);
    results_->clear();
    results_->setColumnCount(static_cast<int>(table_.columns.size()));
    results_->setHorizontalHeaderLabels(table_.columns);
    results_->setRowCount(static_cast<int>(table_.rows.size()));
    std::vector<QPointF> haul;
    const qsizetype stationColumn = table_.columns.indexOf(QStringLiteral("station"));
    const qsizetype massColumn = table_.columns.indexOf(QStringLiteral("mass"));
    for (std::size_t r = 0; r < table_.rows.size(); ++r) {
        const QStringList& row = table_.rows[r];
        for (qsizetype c = 0; c < row.size(); ++c) {
            results_->setItem(static_cast<int>(r), static_cast<int>(c),
                              new QTableWidgetItem(row[c]));
        }
        // The first section has no volume before it, so no ordinate: the
        // diagram starts at 0 there.
        if (row.front() == QStringLiteral("section") && stationColumn > 0) {
            bool stationRead = false;
            const double station = row[stationColumn].toDouble(&stationRead);
            bool massRead = false;
            const double mass = massColumn > 0 ? row[massColumn].toDouble(&massRead) : 0.0;
            if (stationRead) {
                haul.emplace_back(station, massRead ? mass : 0.0);
            }
        }
    }
    chart_->setPoints(std::move(haul));
    // The totals, said as a sentence from the last volume record.
    QString said = QStringLiteral("Measured.");
    for (auto row = table_.rows.rbegin(); row != table_.rows.rend(); ++row) {
        if (row->front() != QStringLiteral("volume")) {
            continue;
        }
        const auto field = [&](const char* key) {
            const qsizetype at = table_.columns.indexOf(QString::fromLatin1(key));
            return at > 0 && at < row->size() ? (*row)[at] : QString();
        };
        if (!field("cut").isEmpty()) {
            said = QStringLiteral("Cut %1 m3, fill %2 m3, net %3 m3 (fill less cut)")
                       .arg(field("cut"), field("fill"), field("net"));
            if (!field("method").isEmpty()) {
                said += QStringLiteral(", by the %1 method").arg(field("method"));
            }
            said += '.';
        } else if (field("ran") == QStringLiteral("no")) {
            said = QStringLiteral("Nothing measured: the scope took no closed polygon.");
        }
        break;
    }
    setStatus(said, false);
}

bool VolumesDialog::measure()
{
    const auto made = line();
    if (!made) {
        setStatus(QString::fromStdString(made.error().message), true);
        return false;
    }
    if (!context_.run) {
        setStatus(QStringLiteral("Nothing here can run a command."), true);
        return false;
    }
    const VerbOutcome outcome = context_.run(*made);
    showReply(outcome);
    return outcome.ok;
}

bool VolumesDialog::saveCsv()
{
    QString path = csvPath_->text().trimmed();
    if (path.isEmpty() && !(context_.headless && context_.headless())) {
        path = QFileDialog::getSaveFileName(this, QStringLiteral("Save Volumes as CSV"), QString(),
                                            QStringLiteral("CSV files (*.csv)"));
        csvPath_->setText(path);
    }
    if (path.isEmpty()) {
        setStatus(QStringLiteral("Give the file to save the table to in the CSV field."), true);
        return false;
    }
    const auto made = line();
    if (!made) {
        setStatus(QString::fromStdString(made.error().message), true);
        return false;
    }
    auto file = lineWord(QStringLiteral("csv=") + path, QStringLiteral("CSV file"));
    if (!file) {
        setStatus(QString::fromStdString(file.error().message), true);
        return false;
    }
    if (!context_.run) {
        setStatus(QStringLiteral("Nothing here can run a command."), true);
        return false;
    }
    // Save replaces the file it names; the file dialog asked before choosing one.
    const VerbOutcome outcome = context_.run(*made + ' ' + *file + QStringLiteral(" OVERWRITE"));
    showReply(outcome);
    if (outcome.ok) {
        setStatus(QStringLiteral("Saved the table to %1.").arg(path), false);
    }
    return outcome.ok;
}

bool VolumesDialog::makeCutFillMap()
{
    const auto made = cutFillMapLine();
    if (!made) {
        setStatus(QString::fromStdString(made.error().message), true);
        return false;
    }
    if (!context_.run) {
        setStatus(QStringLiteral("Nothing here can run a command."), true);
        return false;
    }
    const VerbOutcome outcome = context_.run(*made);
    reply_->setPlainText((outcome.ok ? outcome.reply : outcome.error).trimmed());
    if (!outcome.ok) {
        setStatus(outcome.error.trimmed(), true);
        return false;
    }
    setStatus(QStringLiteral("Made the cut/fill map: a new surface in the Cut Fill style, "
                             "fill above zero and cut below. UNDO removes it."),
              false);
    reload();
    return true;
}

QString VolumesDialog::resultsText() const
{
    QStringList lines;
    lines << table_.columns.join('\t');
    for (const QStringList& row : table_.rows) {
        lines << row.join('\t');
    }
    return lines.join('\n');
}

VolumesDialog& showVolumesDialog(QWidget& window, VolumesDialogContext context)
{
    // No moc, so the kept dialog is found as a QDialog and cast.
    auto* dialog = dynamic_cast<VolumesDialog*>(
        window.findChild<QDialog*>(QStringLiteral("volumesDialog"), Qt::FindDirectChildrenOnly));
    if (dialog == nullptr) {
        dialog = new VolumesDialog(std::move(context), &window);
    }
    dialog->reload();
    dialog->show();
    dialog->raise();
    dialog->activateWindow();
    return *dialog;
}

} // namespace katana::qt
