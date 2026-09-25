#include "alignment_manager.hpp"

#include <QAbstractItemView>
#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QSplitter>
#include <QTabWidget>
#include <QTableWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "command_word.hpp"
#include "customisation/document_watcher.hpp"
#include "katana/cad/alignment_report.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/math/numerics.hpp"
#include "theme.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
namespace geo = katana::geometry;

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

// A stored number as the editable grids show it: exactly, the shortest text
// that reads back as the same double, so writing an unedited cell back
// changes nothing.
QString exact(double value) { return qs(katana::core::formatExactReal(value)); }

// A computed number, to the millimetre a table reads in.
QString fixed3(double value) { return QString::number(value, 'f', 3); }

QTableWidget* makeTable(QWidget* parent, const QString& objectName, const QStringList& headers,
                        bool editable)
{
    auto* table = new QTableWidget(0, static_cast<int>(headers.size()), parent);
    table->setObjectName(objectName);
    table->setHorizontalHeaderLabels(headers);
    table->verticalHeader()->setVisible(false);
    table->setSelectionBehavior(QAbstractItemView::SelectRows);
    table->setSelectionMode(QAbstractItemView::SingleSelection);
    const QAbstractItemView::EditTriggers typing = QAbstractItemView::DoubleClicked |
                                                   QAbstractItemView::EditKeyPressed |
                                                   QAbstractItemView::AnyKeyPressed;
    table->setEditTriggers(editable ? typing
                                    : QAbstractItemView::EditTriggers(
                                          QAbstractItemView::NoEditTriggers));
    table->horizontalHeader()->setStretchLastSection(true);
    return table;
}

QTableWidgetItem* cell(const QString& text, bool editable)
{
    auto* item = new QTableWidgetItem(text);
    Qt::ItemFlags flags = item->flags();
    item->setFlags(editable ? (flags | Qt::ItemIsEditable) : (flags & ~Qt::ItemIsEditable));
    return item;
}

QPushButton* makeButton(const QString& text, const QString& objectName, const QString& tip,
                        QWidget* parent)
{
    auto* button = new QPushButton(text, parent);
    button->setObjectName(objectName);
    button->setToolTip(tip);
    // Enter in a field belongs to that field, never to whichever button
    // Qt would make the default.
    button->setAutoDefault(false);
    return button;
}

QLineEdit* makeField(const QString& objectName, const QString& placeholder, const QString& tip,
                     QWidget* parent)
{
    auto* field = new QLineEdit(parent);
    field->setObjectName(objectName);
    field->setPlaceholderText(placeholder);
    field->setToolTip(tip);
    return field;
}

QLabel* mutedLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

// The design profile, drawn: the tangent polygon through the PVIs dashed, the
// solved grades and curves over it, and the high and low points ringed. The
// vertical is exaggerated to fill the box, as a long section's is, so that a
// 2% grade can be seen at all.
class ProfilePreview final : public QWidget {
  public:
    explicit ProfilePreview(QWidget* parent) : QWidget(parent)
    {
        setObjectName("alignmentProfilePreview");
        setMinimumHeight(120);
    }

    void setProfile(std::vector<geo::ProfilePVI> pvis, std::optional<geo::SolvedProfile> solved)
    {
        pvis_ = std::move(pvis);
        solved_ = std::move(solved);
        update();
    }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.fillRect(rect(), theme::panel());
        painter.setRenderHint(QPainter::Antialiasing);
        if (pvis_.size() < 2) {
            painter.setPen(theme::textMuted());
            painter.drawText(rect(), Qt::AlignCenter, "No design profile");
            return;
        }
        double s0 = pvis_.front().station;
        double s1 = pvis_.back().station;
        double z0 = pvis_.front().elevation;
        double z1 = z0;
        for (const geo::ProfilePVI& pvi : pvis_) {
            s0 = std::min(s0, pvi.station);
            s1 = std::max(s1, pvi.station);
            z0 = std::min(z0, pvi.elevation);
            z1 = std::max(z1, pvi.elevation);
        }
        // A level profile still gets a height to be drawn in.
        if (z1 - z0 < 1e-3) {
            z0 -= 0.5;
            z1 += 0.5;
        }
        if (s1 - s0 < 1e-9) {
            return;
        }
        const QRectF box = QRectF(rect()).adjusted(10, 10, -10, -10);
        const auto at = [&](double station, double elevation) {
            return QPointF(box.left() + (station - s0) / (s1 - s0) * box.width(),
                           box.bottom() - (elevation - z0) / (z1 - z0) * box.height());
        };
        QPen polygon(theme::textMuted(), 1.0, Qt::DashLine);
        painter.setPen(polygon);
        for (std::size_t i = 1; i < pvis_.size(); ++i) {
            painter.drawLine(at(pvis_[i - 1].station, pvis_[i - 1].elevation),
                             at(pvis_[i].station, pvis_[i].elevation));
        }
        if (!solved_) {
            return;
        }
        QPainterPath path;
        bool first = true;
        for (const geo::ProfileElement& element : solved_->elements()) {
            // A tangent is its two ends; a curve is sampled finely enough to
            // look smooth at any width the dialog is given.
            const int steps = element.kind == geo::ProfileElementKind::Curve ? 32 : 1;
            for (int i = 0; i <= steps; ++i) {
                const double distance = element.length * i / steps;
                const QPointF point =
                    at(element.startStation + distance, element.elevationAt(distance));
                if (first) {
                    path.moveTo(point);
                    first = false;
                } else {
                    path.lineTo(point);
                }
            }
        }
        painter.setPen(QPen(theme::accent(), 2.0));
        painter.drawPath(path);
        painter.setPen(QPen(theme::text(), 1.0));
        for (const geo::ProfileExtremum& point : solved_->highLowPoints()) {
            painter.drawEllipse(at(point.station, point.elevation), 4.0, 4.0);
        }
    }

  private:
    std::vector<geo::ProfilePVI> pvis_;
    std::optional<geo::SolvedProfile> solved_;
};

enum PiColumn { PiIndex, PiEasting, PiNorthing, PiRadius, PiSpiralIn, PiSpiralOut };

} // namespace

// =====================================================================================

struct AlignmentManagerDialog::Impl {
    AlignmentManagerDialog* dialog = nullptr;
    AlignmentManagerContext context;
    // The context every connection is made in: deleted first on the way
    // out, which disconnects them all before anything they read is gone.
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    std::unique_ptr<DocumentWatcher> watcher;

    QTableWidget* list = nullptr;
    QLineEdit* name = nullptr;
    QLineEdit* points = nullptr;
    QTabWidget* tabs = nullptr;
    QTableWidget* piTable = nullptr;
    QLineEdit* startStation = nullptr;
    QTableWidget* pviTable = nullptr;
    QTableWidget* elements = nullptr;
    QLabel* highLow = nullptr;
    ProfilePreview* preview = nullptr;
    QLineEdit* interval = nullptr;
    QLabel* stationsNote = nullptr;
    QTableWidget* stations = nullptr;
    QComboBox* labelStyle = nullptr;
    QLineEdit* labelLayer = nullptr;
    QLabel* status = nullptr;
    std::vector<QPushButton*> needsAlignment;

    // True while a table is being refilled, when its cellChanged is the
    // dialog's own doing and not an edit.
    bool loading = false;
    // Each grid is a buffer: edited in place, applied by one line. `edited`
    // says it holds edits not yet applied, made to what `alignment` held
    // when the grid was filled (`pisFrom`, `pvisFrom`). A reload that did
    // not change that keeps the edits; one that did refills the grid.
    struct Buffer {
        bool edited = false;
        QString alignment;
    };
    Buffer pisBuffer;
    std::vector<geo::AlignmentPI> pisFrom;
    Buffer pvisBuffer;
    std::optional<geo::VerticalAlignment> pvisFrom;
    // The start chainage the field was last filled with, so a reload that
    // did not move it leaves what is being typed there alone.
    QString startFor;
    double startFrom = 0.0;

    [[nodiscard]] bool alive() const { return watcher != nullptr && watcher->documentAlive(); }
    [[nodiscard]] katana::cad::Document& document() const { return *context.document; }

    template <typename Sender, typename Signal, typename Slot>
    void connect(Sender* sender, Signal signal, Slot slot)
    {
        QObject::connect(sender, signal, guard.get(), std::move(slot));
    }

    [[nodiscard]] const katana::entity::Alignment* current() const
    {
        if (!alive()) {
            return nullptr;
        }
        const QString chosen = dialog->currentAlignment();
        return chosen.isEmpty() ? nullptr
                                : document().model().alignments.find(chosen.toStdString());
    }

    void setStatus(const QString& text, bool isError)
    {
        status->setStyleSheet(
            QString("color: %1").arg((isError ? theme::error() : theme::textMuted()).name()));
        status->setText(text);
    }

    // Runs one line through the window's executor and shows what it said.
    // True when it was carried out. The dialog reloads at once, rather than
    // waiting for the watcher, so what the line did is on screen when this
    // returns; the watcher's own delivery then finds nothing new.
    bool run(const QString& line)
    {
        // The drawing closed under the dialog: there is nothing to change.
        if (!alive()) {
            setStatus("The drawing this dialog edited is gone.", true);
            return false;
        }
        if (!context.run) {
            setStatus("Nothing here can run " + line + ": the dialog has no command line.", true);
            return false;
        }
        const VerbOutcome outcome = context.run(line);
        if (outcome.ok) {
            QStringList lines = outcome.reply.split('\n', Qt::SkipEmptyParts);
            setStatus(lines.isEmpty() ? line : lines.back(), false);
        } else {
            setStatus(outcome.error.isEmpty() ? line + " was refused." : outcome.error, true);
        }
        reload();
        return outcome.ok;
    }

    // "`verb` <the chosen alignment> `rest`", or nullopt, said on the status
    // line, when there is no alignment to name.
    std::optional<QString> named(const QString& verb, const QString& rest = {})
    {
        const QString chosen = dialog->currentAlignment();
        if (chosen.isEmpty()) {
            setStatus("Choose an alignment first, or make one with New.", true);
            return std::nullopt;
        }
        const auto quoted = commandWord(chosen, "the alignment's name");
        if (!quoted) {
            setStatus(qs(quoted.error().message), true);
            return std::nullopt;
        }
        return verb + " " + *quoted + (rest.isEmpty() ? QString() : " " + rest);
    }

    void build();
    QWidget* buildList();
    QWidget* buildHorizontal();
    QWidget* buildVertical();
    QWidget* buildSettingOut();

    void reload();
    void loadList(const QString& keep);
    void loadAlignment();
    void loadHorizontal(const katana::entity::Alignment* alignment);
    void loadVertical(const katana::entity::Alignment* alignment);
    void loadSettingOut(const katana::entity::Alignment* alignment);
    void loadLabelStyles();

    void useSelection();
    void createAlignment();
    void renumberPis();
    void applyPis();
    void applyProfile();
    [[nodiscard]] Result<std::vector<katana::cad::SettingOutStation>> settingOut() const;
    void copyStations();
    void saveStations();
    void labelChainages();
};

// ---- building ---------------------------------------------------------------------------

void AlignmentManagerDialog::Impl::build()
{
    auto* layout = new QVBoxLayout(dialog);
    layout->addWidget(mutedLabel(
        "An alignment is defined by its PIs, each with the radius and spirals of the curve that "
        "rounds it; its design profile by its PVIs. Every change runs an ALIGN line on the "
        "command line - it is echoed there and is one undo step.",
        dialog));

    auto* splitter = new QSplitter(Qt::Horizontal, dialog);
    splitter->addWidget(buildList());
    tabs = new QTabWidget(splitter);
    tabs->setObjectName("alignmentTabs");
    tabs->addTab(buildHorizontal(), "Horizontal");
    tabs->addTab(buildVertical(), "Vertical");
    tabs->addTab(buildSettingOut(), "Setting Out");
    splitter->addWidget(tabs);
    splitter->setStretchFactor(0, 2);
    splitter->setStretchFactor(1, 3);
    layout->addWidget(splitter, 1);

    auto* bottom = new QHBoxLayout();
    status = new QLabel(dialog);
    status->setObjectName("alignmentStatus");
    status->setWordWrap(true);
    status->setTextInteractionFlags(Qt::TextSelectableByMouse);
    bottom->addWidget(status, 1);
    auto* close = makeButton("Close", "alignmentClose", {}, dialog);
    bottom->addWidget(close);
    layout->addLayout(bottom);
    connect(close, &QPushButton::clicked, [this] { dialog->hide(); });
}

QWidget* AlignmentManagerDialog::Impl::buildList()
{
    auto* page = new QWidget(dialog);
    auto* layout = new QVBoxLayout(page);
    layout->setContentsMargins(0, 0, 0, 0);
    list = makeTable(page, "alignmentList", {"Name", "PIs", "Start", "End", "Length", "Profile"},
                     false);
    list->horizontalHeader()->setStretchLastSection(false);
    list->horizontalHeader()->setSectionResizeMode(0, QHeaderView::Stretch);
    layout->addWidget(list, 1);

    auto* form = new QFormLayout();
    name = makeField("alignmentName", "name", "The new alignment's name", page);
    points = makeField("alignmentPoints", "x,y x,y ...",
                       "The new alignment's PIs, easting first, at least two", page);
    form->addRow("Name:", name);
    form->addRow("PIs:", points);
    layout->addLayout(form);
    auto* buttons = new QHBoxLayout();
    auto* fromSelection =
        makeButton("From Selection", "alignmentUseSelection",
                   "Take the PIs from the selected line or polyline's vertices", page);
    auto* create = makeButton("New", "alignmentNew", "ALIGN NEW name x,y x,y ...", page);
    auto* remove = makeButton("Delete", "alignmentDelete",
                              "ALIGN DELETE name: the chosen alignment (one undo step)", page);
    buttons->addWidget(fromSelection);
    buttons->addStretch(1);
    buttons->addWidget(create);
    buttons->addWidget(remove);
    layout->addLayout(buttons);
    needsAlignment.push_back(remove);

    connect(list, &QTableWidget::itemSelectionChanged, [this] {
        if (!loading) {
            loadAlignment();
        }
    });
    connect(fromSelection, &QPushButton::clicked, [this] { useSelection(); });
    connect(create, &QPushButton::clicked, [this] { createAlignment(); });
    connect(remove, &QPushButton::clicked, [this] {
        if (const auto line = named("ALIGN DELETE")) {
            run(*line);
        }
    });
    return page;
}

QWidget* AlignmentManagerDialog::Impl::buildHorizontal()
{
    auto* page = new QWidget(tabs);
    page->setObjectName("alignmentHorizontalPage");
    auto* layout = new QVBoxLayout(page);
    layout->addWidget(mutedLabel("The PIs in order, counted from 0: edit the grid, then Apply - "
                                 "every PI is one ALIGN PIS line, one undo step. Radius 0 is a "
                                 "corner with no curve; the first and last PI carry none.",
                                 page));
    piTable = makeTable(page, "alignmentPiTable",
                        {"#", "Easting", "Northing", "Radius", "Spiral in", "Spiral out"}, true);
    layout->addWidget(piTable, 1);
    auto* buttons = new QHBoxLayout();
    auto* addPi = makeButton("Add Row", "alignmentAddPi", "A row for another PI, at the end", page);
    auto* removePi =
        makeButton("Remove Row", "alignmentRemovePi", "Drop the chosen row's PI", page);
    auto* revertPis =
        makeButton("Revert", "alignmentRevertPis", "Put back the PIs the drawing holds", page);
    auto* applyPisButton = makeButton(
        "Apply PIs", "alignmentApplyPis",
        "ALIGN PIS name x,y[,radius[,spiralIn[,spiralOut]]] ...: the grid as the PIs", page);
    buttons->addWidget(addPi);
    buttons->addWidget(removePi);
    buttons->addStretch(1);
    buttons->addWidget(revertPis);
    buttons->addWidget(applyPisButton);
    layout->addLayout(buttons);

    auto* startRow = new QHBoxLayout();
    startStation = makeField("alignmentStartStation", "0",
                             "The chainage the alignment starts at", page);
    auto* start = makeButton("Set Start Chainage", "alignmentStart",
                             "ALIGN START name chainage", page);
    startRow->addWidget(new QLabel("Start chainage:", page));
    startRow->addWidget(startStation, 1);
    startRow->addWidget(start);
    layout->addLayout(startRow);
    needsAlignment.insert(needsAlignment.end(), {addPi, removePi, revertPis, applyPisButton, start});

    // An edit marks the grid and nothing more: a line is run by Apply, never
    // from the table's own signal (docs/desktop.md, "The rules a dialog or
    // panel follows").
    connect(piTable, &QTableWidget::cellChanged, [this](int, int) {
        if (!loading) {
            pisBuffer.edited = true;
        }
    });
    connect(addPi, &QPushButton::clicked, [this] {
        loading = true;
        const int row = piTable->rowCount();
        piTable->insertRow(row);
        piTable->setItem(row, PiIndex, cell({}, false));
        for (int column = PiEasting; column <= PiSpiralOut; ++column) {
            piTable->setItem(row, column, cell({}, true));
        }
        renumberPis();
        loading = false;
        pisBuffer.edited = true;
        piTable->setCurrentCell(row, PiEasting);
    });
    connect(removePi, &QPushButton::clicked, [this] {
        const int row = piTable->currentRow();
        if (row < 0) {
            setStatus("Choose the PI to remove in the grid first.", true);
            return;
        }
        loading = true;
        piTable->removeRow(row);
        renumberPis();
        loading = false;
        pisBuffer.edited = true;
    });
    connect(revertPis, &QPushButton::clicked, [this] {
        pisBuffer.edited = false;
        loadHorizontal(current());
    });
    connect(applyPisButton, &QPushButton::clicked, [this] { applyPis(); });
    connect(start, &QPushButton::clicked, [this] {
        const QString chainage = startStation->text().trimmed();
        if (chainage.isEmpty()) {
            setStatus("Type the chainage the alignment starts at.", true);
            return;
        }
        if (const auto line = named("ALIGN START", chainage)) {
            run(*line);
        }
    });
    return page;
}

QWidget* AlignmentManagerDialog::Impl::buildVertical()
{
    auto* page = new QWidget(tabs);
    page->setObjectName("alignmentVerticalPage");
    auto* layout = new QVBoxLayout(page);
    layout->addWidget(mutedLabel("The PVIs, by chainage: edit the grid, then Apply - the whole "
                                 "profile is one ALIGN DESIGN line, one undo step. The first and "
                                 "last PVI carry no curve.",
                                 page));
    pviTable = makeTable(page, "alignmentPviTable", {"Chainage", "Level", "Curve length"}, true);
    layout->addWidget(pviTable, 2);
    auto* buttons = new QHBoxLayout();
    auto* addPvi = makeButton("Add Row", "alignmentAddPvi", "A row for another PVI", page);
    auto* removePvi = makeButton("Remove Row", "alignmentRemovePvi", "Drop the chosen row", page);
    auto* revert = makeButton("Revert", "alignmentRevertProfile",
                              "Put back the profile the drawing holds", page);
    auto* apply = makeButton("Apply Profile", "alignmentApplyProfile",
                             "ALIGN DESIGN name s,z[,L] ...: the grid as the design profile", page);
    auto* clear = makeButton("Clear Profile", "alignmentClearProfile",
                             "ALIGN CLEARPROFILE name: remove the design profile", page);
    buttons->addWidget(addPvi);
    buttons->addWidget(removePvi);
    buttons->addStretch(1);
    buttons->addWidget(revert);
    buttons->addWidget(apply);
    buttons->addWidget(clear);
    layout->addLayout(buttons);
    needsAlignment.insert(needsAlignment.end(), {addPvi, removePvi, revert, apply, clear});

    elements = makeTable(page, "alignmentProfileElements",
                         {"Element", "From", "To", "Grade in %", "Grade out %", "K"}, false);
    layout->addWidget(elements, 2);
    highLow = new QLabel(page);
    highLow->setObjectName("alignmentHighLow");
    highLow->setWordWrap(true);
    layout->addWidget(highLow);
    preview = new ProfilePreview(page);
    layout->addWidget(preview, 2);

    connect(pviTable, &QTableWidget::cellChanged, [this](int, int) {
        if (!loading) {
            pvisBuffer.edited = true;
        }
    });
    connect(addPvi, &QPushButton::clicked, [this] {
        loading = true;
        const int row = pviTable->rowCount();
        pviTable->insertRow(row);
        for (int column = 0; column < 3; ++column) {
            pviTable->setItem(row, column, cell({}, true));
        }
        loading = false;
        pvisBuffer.edited = true;
        pviTable->setCurrentCell(row, 0);
    });
    connect(removePvi, &QPushButton::clicked, [this] {
        const int row = pviTable->currentRow();
        if (row < 0) {
            setStatus("Choose the row to remove first.", true);
            return;
        }
        pviTable->removeRow(row);
        pvisBuffer.edited = true;
    });
    connect(revert, &QPushButton::clicked, [this] {
        pvisBuffer.edited = false;
        loadVertical(current());
    });
    connect(apply, &QPushButton::clicked, [this] { applyProfile(); });
    connect(clear, &QPushButton::clicked, [this] {
        if (const auto line = named("ALIGN CLEARPROFILE")) {
            // What the grid held goes with the profile.
            pvisBuffer.edited = false;
            run(*line);
        }
    });
    return page;
}

QWidget* AlignmentManagerDialog::Impl::buildSettingOut()
{
    auto* page = new QWidget(tabs);
    page->setObjectName("alignmentSettingOutPage");
    auto* layout = new QVBoxLayout(page);
    layout->addWidget(mutedLabel("Every station on the interval and every key station - TS, SC, "
                                 "CS, ST, TC, CT, a PI with no curve - as ALIGN STATIONS prints "
                                 "them. Azimuths are clockwise from grid north.",
                                 page));
    auto* intervalRow = new QHBoxLayout();
    interval = makeField("alignmentInterval", "20", "The chainage interval, in model units", page);
    interval->setText("20");
    // What the table holds, or why it holds nothing: its own line, so a
    // half-typed interval never overwrites what the last line replied.
    stationsNote = new QLabel(page);
    stationsNote->setObjectName("alignmentStationsNote");
    intervalRow->addWidget(new QLabel("Interval:", page));
    intervalRow->addWidget(interval);
    intervalRow->addWidget(stationsNote, 1);
    layout->addLayout(intervalRow);
    stations = makeTable(page, "alignmentStations",
                         {"Chainage", "Easting", "Northing", "Azimuth", "Radius", "Key"}, false);
    layout->addWidget(stations, 1);
    auto* buttons = new QHBoxLayout();
    auto* copy = makeButton("Copy", "alignmentStationsCopy",
                            "The table to the clipboard, tab separated, for a spreadsheet", page);
    auto* csv = makeButton("Save CSV...", "alignmentStationsCsv", "The table as a CSV file", page);
    buttons->addStretch(1);
    buttons->addWidget(copy);
    buttons->addWidget(csv);
    layout->addLayout(buttons);

    auto* labels = new QHBoxLayout();
    labelStyle = new QComboBox(page);
    labelStyle->setObjectName("alignmentLabelStyle");
    labelStyle->setToolTip("A label style that labels chainages (Format > Label Styles)");
    labelLayer = makeField("alignmentLabelLayer", "layer 0",
                           "The layer the labels go on, which must exist; empty for layer 0, "
                           "as LABEL ALIGN puts them",
                           page);
    auto* label = makeButton("Label Chainages", "alignmentLabelChainages",
                             "LABEL ALIGN name style=...: chainage labels along the alignment, "
                             "one undo step",
                             page);
    labels->addWidget(new QLabel("Label style:", page));
    labels->addWidget(labelStyle, 1);
    labels->addWidget(new QLabel("Layer:", page));
    labels->addWidget(labelLayer, 1);
    labels->addWidget(label);
    layout->addLayout(labels);
    needsAlignment.insert(needsAlignment.end(), {copy, csv, label});

    // As it is typed: the table is cheap to make (settingOutStations caps
    // it), and a headless --fill sets the text without finishing an edit.
    connect(interval, &QLineEdit::textChanged, [this] { loadSettingOut(current()); });
    connect(copy, &QPushButton::clicked, [this] { copyStations(); });
    connect(csv, &QPushButton::clicked, [this] { saveStations(); });
    connect(label, &QPushButton::clicked, [this] { labelChainages(); });
    return page;
}

// ---- loading ----------------------------------------------------------------------------

void AlignmentManagerDialog::Impl::reload()
{
    if (!alive()) {
        return;
    }
    loadList(dialog->currentAlignment());
    loadLabelStyles();
    loadAlignment();
}

void AlignmentManagerDialog::Impl::loadList(const QString& keep)
{
    loading = true;
    list->setRowCount(0);
    int keepRow = -1;
    document().model().alignments.forEach([&](const katana::entity::Alignment& alignment) {
        const int row = list->rowCount();
        list->insertRow(row);
        const QString alignmentName = qs(alignment.name);
        list->setItem(row, 0, cell(alignmentName, false));
        list->setItem(row, 1, cell(QString::number(alignment.horizontal.pis.size()), false));
        // Stored alignments always solve - the model refuses one that does
        // not - so a failure is a defect worth showing, not hiding.
        if (auto solved = geo::solveAlignment(alignment.horizontal)) {
            list->setItem(row, 2, cell(fixed3(solved->startStation()), false));
            list->setItem(row, 3, cell(fixed3(solved->endStation()), false));
            list->setItem(row, 4, cell(fixed3(solved->length()), false));
        } else {
            list->setItem(row, 2, cell("does not solve", false));
        }
        list->setItem(row, 5,
                      cell(alignment.vertical ? QString("%1 PVIs").arg(alignment.vertical->pvis.size())
                                              : QString("none"),
                           false));
        if (alignmentName == keep) {
            keepRow = row;
        }
    });
    if (keepRow < 0 && list->rowCount() > 0) {
        keepRow = 0;
    }
    if (keepRow >= 0) {
        list->selectRow(keepRow);
    }
    list->resizeColumnsToContents();
    loading = false;
}

void AlignmentManagerDialog::Impl::loadAlignment()
{
    const katana::entity::Alignment* alignment = current();
    for (QPushButton* button : needsAlignment) {
        button->setEnabled(alignment != nullptr);
    }
    loadHorizontal(alignment);
    loadVertical(alignment);
    loadSettingOut(alignment);
}

void AlignmentManagerDialog::Impl::loadHorizontal(const katana::entity::Alignment* alignment)
{
    const QString alignmentName = alignment != nullptr ? qs(alignment->name) : QString();
    const std::vector<geo::AlignmentPI> pis =
        alignment != nullptr ? alignment->horizontal.pis : std::vector<geo::AlignmentPI>{};
    const bool keepGrid =
        pisBuffer.edited && alignmentName == pisBuffer.alignment && pis == pisFrom;
    if (!keepGrid) {
        loading = true;
        piTable->setRowCount(0);
        for (std::size_t i = 0; i < pis.size(); ++i) {
            const int row = static_cast<int>(i);
            piTable->insertRow(row);
            piTable->setItem(row, PiIndex, cell({}, false));
            piTable->setItem(row, PiEasting, cell(exact(pis[i].point.x), true));
            piTable->setItem(row, PiNorthing, cell(exact(pis[i].point.y), true));
            piTable->setItem(row, PiRadius, cell(exact(pis[i].radius), true));
            piTable->setItem(row, PiSpiralIn, cell(exact(pis[i].spiralIn), true));
            piTable->setItem(row, PiSpiralOut, cell(exact(pis[i].spiralOut), true));
        }
        renumberPis();
        loading = false;
        pisBuffer = {false, alignmentName};
        pisFrom = pis;
    }
    const double start = alignment != nullptr ? alignment->horizontal.startStation : 0.0;
    if (alignmentName != startFor || start != startFrom) {
        startStation->setText(alignment != nullptr ? exact(start) : QString());
        startFor = alignmentName;
        startFrom = start;
    }
}

void AlignmentManagerDialog::Impl::loadVertical(const katana::entity::Alignment* alignment)
{
    const QString alignmentName = alignment != nullptr ? qs(alignment->name) : QString();
    const std::optional<geo::VerticalAlignment> profile =
        alignment != nullptr ? alignment->vertical : std::nullopt;
    // Unapplied edits survive a reload that did not touch what they were
    // made to: another alignment's edit, a label, an undo elsewhere.
    const bool keepGrid =
        pvisBuffer.edited && alignmentName == pvisBuffer.alignment && profile == pvisFrom;
    if (!keepGrid) {
        loading = true;
        pviTable->setRowCount(0);
        if (profile) {
            for (const geo::ProfilePVI& pvi : profile->pvis) {
                const int row = pviTable->rowCount();
                pviTable->insertRow(row);
                pviTable->setItem(row, 0, cell(exact(pvi.station), true));
                pviTable->setItem(row, 1, cell(exact(pvi.elevation), true));
                pviTable->setItem(row, 2, cell(exact(pvi.curveLength), true));
            }
        }
        loading = false;
        pvisBuffer = {false, alignmentName};
        pvisFrom = profile;
    }

    elements->setRowCount(0);
    std::optional<geo::SolvedProfile> solved;
    if (profile) {
        if (auto result = geo::solveProfile(*profile)) {
            solved = std::move(*result);
        }
    }
    QStringList extremes;
    if (solved) {
        for (const geo::ProfileElement& element : solved->elements()) {
            const int row = elements->rowCount();
            elements->insertRow(row);
            const bool curve = element.kind == geo::ProfileElementKind::Curve;
            elements->setItem(row, 0, cell(curve ? "curve" : "tangent", false));
            elements->setItem(row, 1, cell(fixed3(element.startStation), false));
            elements->setItem(row, 2, cell(fixed3(element.startStation + element.length), false));
            elements->setItem(row, 3, cell(fixed3(element.startGrade * 100.0), false));
            elements->setItem(row, 4, cell(fixed3(element.endGrade * 100.0), false));
            const auto k = katana::cad::curveK(element);
            elements->setItem(row, 5, cell(k ? fixed3(*k) : QString(), false));
        }
        for (const geo::ProfileExtremum& point : solved->highLowPoints()) {
            extremes << QString("%1 point at chainage %2, level %3")
                            .arg(point.high ? "High" : "Low")
                            .arg(fixed3(point.station))
                            .arg(fixed3(point.elevation));
        }
    }
    highLow->setText(!profile            ? QString("No design profile: fill the grid and Apply.")
                     : extremes.isEmpty() ? QString("No high or low point: no curve turns the "
                                                    "grade through level.")
                                          : extremes.join("; ") + ".");
    preview->setProfile(profile ? profile->pvis : std::vector<geo::ProfilePVI>{},
                        std::move(solved));
}

Result<std::vector<katana::cad::SettingOutStation>> AlignmentManagerDialog::Impl::settingOut() const
{
    const katana::entity::Alignment* alignment = current();
    if (alignment == nullptr) {
        return makeError(ErrorCode::InvalidState, "no alignment is chosen");
    }
    const auto spacing = katana::core::parseFiniteDouble(
        katana::core::trimmed(interval->text().toStdString()));
    if (!spacing) {
        return makeError(ErrorCode::InvalidArgument, "the interval is not a number",
                         interval->text().toStdString());
    }
    auto solved = geo::solveAlignment(alignment->horizontal);
    if (!solved) {
        return solved.error();
    }
    return katana::cad::settingOutStations(*solved, *spacing);
}

void AlignmentManagerDialog::Impl::loadSettingOut(const katana::entity::Alignment* alignment)
{
    stations->setRowCount(0);
    stationsNote->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    if (alignment == nullptr) {
        stationsNote->clear();
        return;
    }
    const auto rows = settingOut();
    if (!rows) {
        stationsNote->setStyleSheet(QString("color: %1").arg(theme::error().name()));
        stationsNote->setText(qs(rows.error().describe()));
        return;
    }
    const auto keys = std::count_if(rows->begin(), rows->end(),
                                    [](const auto& row) { return !row.key.empty(); });
    stationsNote->setText(
        QString("%1 stations, %2 of them key stations.").arg(rows->size()).arg(keys));
    stations->setRowCount(static_cast<int>(rows->size()));
    for (std::size_t i = 0; i < rows->size(); ++i) {
        const katana::cad::SettingOutStation& station = (*rows)[i];
        const int row = static_cast<int>(i);
        stations->setItem(row, 0, cell(fixed3(station.station), false));
        stations->setItem(row, 1, cell(fixed3(station.point.x), false));
        stations->setItem(row, 2, cell(fixed3(station.point.y), false));
        stations->setItem(row, 3, cell(qs(katana::cad::formatAzimuth(station.azimuth)), false));
        stations->setItem(
            row, 4,
            cell(station.curvature == 0.0
                     ? QString("straight")
                     : fixed3(1.0 / std::abs(station.curvature)) +
                           (station.curvature > 0.0 ? " L" : " R"),
                 false));
        stations->setItem(row, 5, cell(qs(station.key), false));
    }
}

void AlignmentManagerDialog::Impl::loadLabelStyles()
{
    const QString keep = labelStyle->currentText();
    labelStyle->clear();
    document().model().labelStyles.forEach([&](const katana::entity::LabelStyle& style) {
        if (style.kind == katana::entity::LabelKind::Chainage) {
            labelStyle->addItem(qs(style.name));
        }
    });
    const int found = labelStyle->findText(keep);
    if (found >= 0) {
        labelStyle->setCurrentIndex(found);
    }
}

// ---- acting -----------------------------------------------------------------------------

void AlignmentManagerDialog::Impl::useSelection()
{
    if (!alive()) {
        return;
    }
    const std::vector<katana::entity::EntityId> ids = document().selection().ids();
    const katana::entity::Entity* entity =
        ids.size() == 1 ? document().model().entities.find(ids.front()) : nullptr;
    std::vector<geo::Point2> vertices;
    if (entity != nullptr) {
        if (const auto* line = std::get_if<geo::Segment2>(&entity->geometry)) {
            vertices = {line->start, line->end};
        } else if (const auto* polyline = std::get_if<geo::Polyline2>(&entity->geometry)) {
            vertices = polyline->vertices;
        }
    }
    if (vertices.size() < 2) {
        setStatus(QString("Select one line or polyline to take the PIs from (%1 selected).")
                      .arg(ids.size()),
                  true);
        return;
    }
    QStringList words;
    for (const geo::Point2& vertex : vertices) {
        words << exact(vertex.x) + "," + exact(vertex.y);
    }
    points->setText(words.join(' '));
    setStatus(QString("%1 PIs from the selection's vertices.").arg(vertices.size()), false);
}

void AlignmentManagerDialog::Impl::createAlignment()
{
    const QString typed = name->text().trimmed();
    if (typed.isEmpty()) {
        setStatus("Type a name for the new alignment.", true);
        return;
    }
    const auto quoted = commandWord(typed, "the name");
    if (!quoted) {
        setStatus(qs(quoted.error().message), true);
        return;
    }
    const QString pis = points->text().simplified();
    if (pis.isEmpty()) {
        setStatus("Type the PIs (x,y x,y ...) or take them From Selection.", true);
        return;
    }
    if (run("ALIGN NEW " + *quoted + " " + pis)) {
        dialog->selectAlignment(typed);
        name->clear();
        points->clear();
    }
}

void AlignmentManagerDialog::Impl::renumberPis()
{
    for (int row = 0; row < piTable->rowCount(); ++row) {
        piTable->item(row, PiIndex)->setText(QString::number(row));
    }
}

void AlignmentManagerDialog::Impl::applyPis()
{
    QStringList pis;
    for (int row = 0; row < piTable->rowCount(); ++row) {
        QStringList values;
        for (int column = PiEasting; column <= PiSpiralOut; ++column) {
            const QTableWidgetItem* item = piTable->item(row, column);
            values << (item != nullptr ? item->text().trimmed() : QString());
        }
        if (std::all_of(values.begin(), values.end(), [](const QString& v) { return v.isEmpty(); })) {
            continue; // a row added and never filled
        }
        if (values[0].isEmpty() || values[1].isEmpty()) {
            setStatus(QString("PI %1 needs an easting and a northing.").arg(row), true);
            return;
        }
        // The curve's numbers as far as the last one given, an empty one
        // before it 0; a corner with no curve at all is just x,y.
        QStringList curve = values.mid(2);
        while (!curve.isEmpty() && (curve.back().isEmpty() || curve.back() == "0")) {
            curve.removeLast();
        }
        for (QString& value : curve) {
            if (value.isEmpty()) {
                value = "0";
            }
        }
        pis << values[0] + "," + values[1] + (curve.isEmpty() ? QString() : "," + curve.join(','));
    }
    if (pis.size() < 2) {
        setStatus("An alignment needs at least two PIs.", true);
        return;
    }
    if (const auto line = named("ALIGN PIS", pis.join(' '))) {
        // Applied, the reload finds PIs other than the grid was filled from
        // and refills it from the model; refused, the model is unchanged and
        // the grid is kept for correcting.
        run(*line);
    }
}

void AlignmentManagerDialog::Impl::applyProfile()
{
    QStringList pvis;
    for (int row = 0; row < pviTable->rowCount(); ++row) {
        const auto value = [this, row](int column) {
            const QTableWidgetItem* item = pviTable->item(row, column);
            return item != nullptr ? item->text().trimmed() : QString();
        };
        const QString station = value(0);
        const QString level = value(1);
        const QString length = value(2);
        if (station.isEmpty() && level.isEmpty() && length.isEmpty()) {
            continue; // a row added and never filled
        }
        if (station.isEmpty() || level.isEmpty()) {
            setStatus(QString("Profile row %1 needs a chainage and a level.").arg(row + 1), true);
            return;
        }
        pvis << station + "," + level +
                    (length.isEmpty() || length == "0" ? QString() : "," + length);
    }
    if (pvis.size() < 2) {
        setStatus("A design profile needs at least two PVIs.", true);
        return;
    }
    if (const auto line = named("ALIGN DESIGN", pvis.join(' '))) {
        // Applied, the reload finds a profile other than the one the grid
        // was loaded from and refills it from the model; refused, the model
        // is unchanged and the grid is kept for correcting.
        run(*line);
    }
}

void AlignmentManagerDialog::Impl::copyStations()
{
    QStringList lines;
    QStringList header;
    for (int column = 0; column < stations->columnCount(); ++column) {
        header << stations->horizontalHeaderItem(column)->text();
    }
    lines << header.join('\t');
    for (int row = 0; row < stations->rowCount(); ++row) {
        QStringList cells;
        for (int column = 0; column < stations->columnCount(); ++column) {
            const QTableWidgetItem* item = stations->item(row, column);
            cells << (item != nullptr ? item->text() : QString());
        }
        lines << cells.join('\t');
    }
    QApplication::clipboard()->setText(lines.join('\n') + '\n');
    setStatus(QString("%1 stations copied.").arg(stations->rowCount()), false);
}

void AlignmentManagerDialog::Impl::saveStations()
{
    if (context.headless && context.headless()) {
        setStatus("A headless session opens no file dialog; ALIGN STATIONS name interval gives "
                  "the same table.",
                  true);
        return;
    }
    const QString chosen = dialog->currentAlignment();
    const QString path = QFileDialog::getSaveFileName(
        dialog, "Save Setting-Out Table", chosen + "_setting_out.csv", "CSV (*.csv)");
    if (path.isEmpty()) {
        return;
    }
    if (const auto saved = dialog->saveStationsCsv(path); !saved) {
        setStatus(qs(saved.error().describe()), true);
        return;
    }
    setStatus("Saved to " + QDir::toNativeSeparators(path) + ".", false);
}

void AlignmentManagerDialog::Impl::labelChainages()
{
    const QString style = labelStyle->currentText();
    if (style.isEmpty()) {
        setStatus("No label style labels chainages. Add one in Format > Label Styles "
                  "(LABELSTYLE DEFAULTS adds the standard set).",
                  true);
        return;
    }
    const auto quotedStyle = commandWord(style, "the label style's name");
    if (!quotedStyle) {
        setStatus(qs(quotedStyle.error().message), true);
        return;
    }
    QString options = "style=" + *quotedStyle;
    const QString layer = labelLayer->text().trimmed();
    if (!layer.isEmpty()) {
        const auto quotedLayer = commandWord(layer, "the layer");
        if (!quotedLayer) {
            setStatus(qs(quotedLayer.error().message), true);
            return;
        }
        options += " layer=" + *quotedLayer;
    }
    if (const auto line = named("LABEL ALIGN", options)) {
        run(*line);
    }
}

// =====================================================================================

AlignmentManagerDialog::AlignmentManagerDialog(AlignmentManagerContext context, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>())
{
    Impl& impl = *impl_;
    impl.dialog = this;
    impl.context = std::move(context);
    setObjectName("alignmentManagerDialog");
    setWindowTitle("Alignment Manager");
    // Non-modal: kept open beside the drawing while PIs are picked off it.
    setModal(false);
    impl.build();
    impl.watcher = std::make_unique<DocumentWatcher>(
        *impl.context.document, [&impl](const DocumentChanges& changes) {
            if (changes.model) {
                impl.reload();
            }
        });
    impl.reload();
    resize(1100, 680);
}

AlignmentManagerDialog::~AlignmentManagerDialog()
{
    // The watcher first, then every connection: left to ~QWidget, the
    // children would die after impl_ and could signal into it on the way.
    impl_->watcher.reset();
    impl_->guard.reset();
}

bool AlignmentManagerDialog::selectAlignment(const QString& name)
{
    for (int row = 0; row < impl_->list->rowCount(); ++row) {
        if (impl_->list->item(row, 0)->text() == name) {
            impl_->list->selectRow(row);
            return true;
        }
    }
    return false;
}

QString AlignmentManagerDialog::currentAlignment() const
{
    const QList<QTableWidgetItem*> chosen = impl_->list->selectedItems();
    if (chosen.isEmpty()) {
        return {};
    }
    const QTableWidgetItem* first = impl_->list->item(chosen.front()->row(), 0);
    return first != nullptr ? first->text() : QString();
}

Status AlignmentManagerDialog::saveStationsCsv(const QString& path) const
{
    const auto rows = impl_->settingOut();
    if (!rows) {
        return rows.error();
    }
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    const std::string text = katana::cad::settingOutCsv(*rows);
    if (file.write(text.data(), static_cast<qint64>(text.size())) !=
        static_cast<qint64>(text.size())) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    return {};
}

} // namespace katana::qt
