// BIM > Road Objects... (bim/road_object_manager.hpp).

#include "bim/road_object_manager.hpp"

#include <QAbstractItemView>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QItemSelectionModel>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <optional>
#include <string>
#include <vector>

#include "customisation/document_watcher.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/road_object_scope.hpp"
#include "katana/cad/road/road_object_verbs.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

using katana::cad::road::RoadObject;

namespace {

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

// A real as the table shows it: to the millimetre, which is what a station
// or a quantity is set out to.
QString shown(double value)
{
    return QString::number(value, 'f', 3);
}

enum Column { Id, Kind, Name, Placement, From, To, Offset, Layer, Quantities, Problem, Count };

} // namespace

std::optional<katana::geometry::Box2> roadObjectsFrame(const katana::cad::Document& document,
                                                       const QStringList& ids)
{
    const katana::cad::road::PlacementContext context =
        katana::cad::road::placementContext(document);
    katana::geometry::Box2 box;
    bool any = false;
    for (const QString& id : ids) {
        const RoadObject* object = document.roadObjects().find(id.toStdString());
        if (object == nullptr) {
            continue;
        }
        // The same extent the scope's AREA and VIEW meet (road_object_scope.hpp).
        if (const auto extent = katana::cad::road::planExtent(context, *object)) {
            box.expand(*extent);
            any = true;
        }
    }
    if (!any) {
        return std::nullopt;
    }
    // A tenth more each way, and 10 m across at the least, so what is framed
    // is seen with what is round it.
    const katana::geometry::Point2 centre = (box.min + box.max) / 2.0;
    const double half =
        std::max({(box.max.x - box.min.x) * 0.6, (box.max.y - box.min.y) * 0.6, 5.0});
    return katana::geometry::Box2(centre - katana::geometry::Point2(half, half),
                                  centre + katana::geometry::Point2(half, half));
}

struct RoadObjectManagerDialog::Impl {
    RoadObjectManagerContext context;
    QTableWidget* table = nullptr;
    QLabel* count = nullptr;
    QPlainTextEdit* info = nullptr;
    QLineEdit* keys = nullptr;
    QPushButton* apply = nullptr;
    QPushButton* copy = nullptr;
    QPushButton* remove = nullptr;
    QPushButton* zoom = nullptr;
    QPushButton* place = nullptr;
    ScopeFilterWidget* scope = nullptr;
    QPushButton* selectMatching = nullptr;
    QPushButton* quantities = nullptr;
    QPushButton* deleteMatching = nullptr;
    QLabel* status = nullptr;
    // Last, so it goes first.
    std::unique_ptr<DocumentWatcher> watcher;

    [[nodiscard]] bool documentAlive() const
    {
        return context.document != nullptr && watcher != nullptr && watcher->documentAlive();
    }
};

RoadObjectManagerDialog::RoadObjectManagerDialog(RoadObjectManagerContext context, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>())
{
    Impl& d = *impl_;
    d.context = std::move(context);
    setObjectName("roadObjectManagerDialog");
    setWindowTitle("Road Objects");
    setModal(false);

    d.table = new QTableWidget(0, Column::Count, this);
    d.table->setObjectName("roadObjectTable");
    d.table->setHorizontalHeaderLabels({"Id", "Kind", "Name", "Placement", "From", "To", "Offset",
                                        "Layer", "Quantities", "Problem"});
    // Read-only: a form edits, a table never runs a line from its own signal.
    d.table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    d.table->setSelectionBehavior(QAbstractItemView::SelectRows);
    d.table->setSelectionMode(QAbstractItemView::ExtendedSelection);
    d.table->horizontalHeader()->setStretchLastSection(true);
    d.count = new QLabel(this);
    d.count->setObjectName("roadObjectCount");
    d.info = new QPlainTextEdit(this);
    d.info->setObjectName("roadObjectInfo");
    d.info->setReadOnly(true);
    d.keys = new QLineEdit(this);
    d.keys->setObjectName("roadObjectKeys");
    d.keys->setPlaceholderText("key=value ...   e.g. offset=4.5 height=1.0 (ROADOBJECT KINDS)");
    d.keys->setToolTip("Apply sets these on the chosen object; Copy makes a copy with them "
                       "(station=, dx= and dy= move the copy, and any other key changes it)");
    const auto button = [this](const char* text, const char* name) {
        auto* made = new QPushButton(text, this);
        made->setObjectName(name);
        return made;
    };
    d.apply = button("&Apply", "roadObjectApply");
    d.copy = button("C&opy", "roadObjectCopy");
    d.remove = button("&Delete", "roadObjectDelete");
    d.zoom = button("&Zoom To", "roadObjectZoom");
    d.place = button("&New...", "roadObjectNew");
    auto* close = button("&Close", "roadObjectManagerClose");
    d.selectMatching = button("&Select Matching", "roadObjectSelectMatching");
    d.quantities = button("&Quantities", "roadObjectQuantities");
    d.deleteMatching = button("Delete Matchin&g", "roadObjectDeleteMatching");
    d.selectMatching->setToolTip("Choose the rows of the objects the scope and the filter take "
                                 "(ROADOBJECT LIST <scope>)");
    d.quantities->setToolTip("The quantities of the objects the scope and the filter take, in "
                             "the details (ROADOBJECT QUANTITIES <scope>)");
    d.deleteMatching->setToolTip("Delete the objects the scope and the filter take, as one undo "
                                 "step (ROADOBJECT DELETE <scope>)");
    // The shared scope and filter. A road object is a definition on a layer
    // with a plan extent, so the whole drawing is the natural start; the views
    // are the window's. The controls that are about entities are greyed, with
    // the reason: the verb would refuse them by name.
    d.scope = new ScopeFilterWidget("roadObjectManager", this);
    d.scope->views = d.context.views;
    d.scope->setChoice(ScopeChoice::Drawing);
    const QString noEntity = "A road object is not an entity: it has no type, style or text to "
                             "match. Its kind is a property: key kind, value barrier";
    for (QWidget* each : d.scope->findChildren<QWidget*>()) {
        const QString name = each->objectName();
        if (name.startsWith("roadObjectManagerType") || name == "roadObjectManagerFilterStyle" ||
            name == "roadObjectManagerFilterText") {
            each->setEnabled(false);
            each->setToolTip(noEntity);
        } else if (name == "roadObjectManagerScopeSelection") {
            each->setToolTip("The objects placed along the selected lines; one placed along an "
                             "alignment or at points has no line to select");
        } else if (name == "roadObjectManagerScopeView") {
            each->setToolTip("The objects on a layer the view shows, and (with only what is on "
                             "screen) whose plan extent meets the view's area");
        } else if (name == "roadObjectManagerFilterProperty") {
            each->setToolTip("A key ROADOBJECT SET writes: kind, id, type, offset, station, "
                             "layer ... - exactly, in any case");
        }
    }
    auto* byScope = new QGroupBox("Road objects by scope", this);
    byScope->setObjectName("roadObjectManagerByScope");
    auto* byScopeLayout = new QVBoxLayout(byScope);
    byScopeLayout->addWidget(d.scope, 1);
    auto* scopeButtons = new QHBoxLayout;
    scopeButtons->addWidget(d.selectMatching);
    scopeButtons->addWidget(d.quantities);
    scopeButtons->addWidget(d.deleteMatching);
    byScopeLayout->addLayout(scopeButtons);
    d.status = new QLabel(this);
    d.status->setObjectName("roadObjectManagerStatus");
    d.status->setWordWrap(true);

    auto* edit = new QHBoxLayout;
    edit->addWidget(d.keys, 1);
    edit->addWidget(d.apply);
    auto* buttons = new QHBoxLayout;
    for (QPushButton* each : {d.place, d.copy, d.remove, d.zoom}) {
        buttons->addWidget(each);
    }
    buttons->addStretch(1);
    buttons->addWidget(close);
    auto* layout = new QVBoxLayout;
    layout->addWidget(d.count);
    layout->addWidget(d.table, 3);
    layout->addWidget(d.info, 2);
    layout->addLayout(edit);
    layout->addLayout(buttons);
    layout->addWidget(d.status);
    auto* columns = new QHBoxLayout(this);
    columns->addLayout(layout, 3);
    columns->addWidget(byScope, 0);
    resize(1180, 680);

    // Runs one line through the window's executor and says what it replied;
    // the outcome when the line was accepted.
    const auto execute = [this](const QString& line) -> std::optional<VerbOutcome> {
        Impl& self = *impl_;
        if (!self.context.run) {
            self.status->setText("There is no command line to run it on here.");
            return std::nullopt;
        }
        const VerbOutcome outcome = self.context.run(line);
        self.status->setText(outcome.ok ? outcome.reply : "Refused: " + outcome.error);
        return outcome.ok ? std::optional<VerbOutcome>(outcome) : std::nullopt;
    };
    const auto run = [execute](const QString& line) { return execute(line).has_value(); };
    // The scope the controls say, as the words a line takes; the reason when
    // they say none (no layer ticked, a colour that does not read ...).
    const auto scopeWords = [this]() -> std::optional<QString> {
        Impl& self = *impl_;
        const auto words = self.scope->verbWords();
        if (!words) {
            self.status->setText("Refused: " + QString::fromStdString(words.error().message));
            return std::nullopt;
        }
        return *words;
    };
    const auto showInfo = [this] {
        Impl& self = *impl_;
        const QStringList ids = selectedIds();
        if (ids.size() != 1 || !self.documentAlive()) {
            self.info->clear();
            return;
        }
        // The verb's own INFO - it changes nothing, so it is read here and
        // not run through the executor, which would keep it in the history.
        const auto reply = katana::cad::road::runRoadObjectVerb(
            *self.context.document, {"ROADOBJECT", "INFO", ids.front().toStdString()});
        self.info->setPlainText(reply ? qs(*reply) : qs(reply.error().describe()));
    };
    connect(d.table, &QTableWidget::itemSelectionChanged, this, [this, showInfo] {
        showInfo();
        const bool one = selectedIds().size() == 1;
        const bool some = !selectedIds().isEmpty();
        impl_->apply->setEnabled(one);
        impl_->copy->setEnabled(one);
        impl_->remove->setEnabled(some);
        impl_->zoom->setEnabled(some);
    });
    connect(d.apply, &QPushButton::clicked, this, [this, run] {
        const QStringList ids = selectedIds();
        const QString keys = impl_->keys->text().simplified();
        if (ids.size() != 1 || keys.isEmpty()) {
            impl_->status->setText("Choose one object and give key=value to set.");
            return;
        }
        // The keys are spent once they are set; a refused line keeps them to
        // be put right.
        if (run("ROADOBJECT SET " + ids.front() + " " + keys)) {
            impl_->keys->clear();
        }
    });
    connect(d.copy, &QPushButton::clicked, this, [this, run] {
        const QStringList ids = selectedIds();
        if (ids.size() == 1) {
            // The keys field goes with the copy: station=, dx= and dy= are what
            // move it, and no other control says them.
            const QString keys = impl_->keys->text().simplified();
            if (run("ROADOBJECT COPY " + ids.front() + (keys.isEmpty() ? QString() : " " + keys))) {
                impl_->keys->clear();
            }
        }
    });
    connect(d.remove, &QPushButton::clicked, this, [this, run] {
        const QStringList ids = selectedIds();
        if (!ids.isEmpty()) {
            run("ROADOBJECT DELETE " + ids.join(' '));
        }
    });
    connect(d.zoom, &QPushButton::clicked, this, [this, run] {
        if (!impl_->documentAlive()) {
            return;
        }
        const auto frame = roadObjectsFrame(*impl_->context.document, selectedIds());
        if (!frame) {
            impl_->status->setText("Nothing of those objects is made, so there is nothing to "
                                   "frame: ROADOBJECT PROBLEMS says why.");
            return;
        }
        run(QString("ZOOM WINDOW %1,%2,%3,%4")
                .arg(QString::fromStdString(katana::core::formatExactReal(frame->min.x)),
                     QString::fromStdString(katana::core::formatExactReal(frame->min.y)),
                     QString::fromStdString(katana::core::formatExactReal(frame->max.x)),
                     QString::fromStdString(katana::core::formatExactReal(frame->max.y))));
    });
    connect(d.selectMatching, &QPushButton::clicked, this, [this, execute, scopeWords] {
        const auto words = scopeWords();
        if (!words) {
            return;
        }
        const auto outcome = execute("ROADOBJECT LIST " + *words);
        if (!outcome) {
            return;
        }
        // The records say which: `object id=...`, after the scope's own.
        QStringList ids;
        QString record;
        for (const QString& line : outcome->reply.split('\n')) {
            if (line.startsWith("object id=")) {
                ids << line.mid(10, line.indexOf(' ', 10) - 10);
            } else if (line.startsWith("scope=")) {
                record = line.trimmed();
            }
        }
        QTableWidget* table = impl_->table;
        table->clearSelection();
        QItemSelectionModel* selection = table->selectionModel();
        for (int row = 0; row < table->rowCount(); ++row) {
            if (ids.contains(table->item(row, Column::Id)->text())) {
                selection->select(table->model()->index(row, 0),
                                  QItemSelectionModel::Select | QItemSelectionModel::Rows);
            }
        }
        impl_->status->setText(QString("%1 chosen - %2")
                                   .arg(ids.size())
                                   .arg(record.isEmpty() ? QString("no scope record") : record));
    });
    connect(d.quantities, &QPushButton::clicked, this, [this, execute, scopeWords] {
        const auto words = scopeWords();
        if (!words) {
            return;
        }
        if (const auto outcome = execute("ROADOBJECT QUANTITIES " + *words)) {
            impl_->info->setPlainText(outcome->reply);
            impl_->status->setText(outcome->reply.section('\n', 0, 0));
        }
    });
    connect(d.deleteMatching, &QPushButton::clicked, this, [this, run, scopeWords] {
        if (const auto words = scopeWords()) {
            run("ROADOBJECT DELETE " + *words);
        }
    });
    connect(d.place, &QPushButton::clicked, this, [this] {
        if (!impl_->context.place) {
            return;
        }
        katana::cad::road::RoadObjectKind kind = katana::cad::road::RoadObjectKind::Barrier;
        const QStringList ids = selectedIds();
        if (!ids.isEmpty() && impl_->documentAlive()) {
            if (const RoadObject* object =
                    impl_->context.document->roadObjects().find(ids.front().toStdString())) {
                kind = object->kind;
            }
        }
        impl_->context.place(kind);
    });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });

    if (d.context.document != nullptr) {
        d.watcher = std::make_unique<DocumentWatcher>(
            *d.context.document, [this](const DocumentChanges& changes) {
                if (changes.model) {
                    reload();
                }
            });
    }
    reload();
}

RoadObjectManagerDialog::~RoadObjectManagerDialog() = default;

bool RoadObjectManagerDialog::selectObject(const QString& id)
{
    QTableWidget* table = impl_->table;
    for (int row = 0; row < table->rowCount(); ++row) {
        if (table->item(row, Column::Id)->text().compare(id, Qt::CaseInsensitive) == 0) {
            table->clearSelection();
            table->selectRow(row);
            return true;
        }
    }
    return false;
}

QStringList RoadObjectManagerDialog::selectedIds() const
{
    QStringList ids;
    QTableWidget* table = impl_->table;
    for (int row = 0; row < table->rowCount(); ++row) {
        if (table->item(row, Column::Id)->isSelected()) {
            ids << table->item(row, Column::Id)->text();
        }
    }
    return ids;
}

void RoadObjectManagerDialog::reload()
{
    Impl& d = *impl_;
    const QStringList kept = selectedIds();
    d.table->setRowCount(0);
    if (!d.documentAlive()) {
        d.count->setText("No drawing.");
        d.scope->reloadViews();
        return;
    }
    const katana::cad::Document& document = *d.context.document;
    // The layers and the views to choose from follow the drawing and the window.
    d.scope->reload(document.model());
    const katana::cad::road::PlacementContext context =
        katana::cad::road::placementContext(document);
    std::size_t problems = 0;
    // A set that cannot be read is empty to roadObjects(): without this the
    // manager would say "0 road objects, 0 problems" over a record that is
    // there and kept (a newer Katana's, or damaged).
    QString unreadable;
    if (const auto readable = document.roadObjectsStatus(); !readable) {
        unreadable = qs(readable.error().describe());
        ++problems;
    }
    const auto& objects = document.roadObjects().objects;
    d.table->setRowCount(static_cast<int>(objects.size()));
    for (std::size_t i = 0; i < objects.size(); ++i) {
        const RoadObject& object = objects[i];
        const auto built = katana::cad::road::buildRoadObject(context, object);
        problems += built.problems.size();
        const auto& placement = object.placement;
        QString quantities;
        for (const auto& quantity : katana::cad::road::totalQuantities(built.objects)) {
            quantities += QString("%1 %2 %3  ")
                              .arg(qs(quantity.name), shown(quantity.value), qs(quantity.unit));
        }
        const bool along = placement.kind == katana::cad::road::PlacementKind::Alignment;
        const QString cells[Column::Count] = {
            qs(object.id),
            qs(kindTitle(object.kind)),
            qs(object.name),
            along ? qs(placement.alignment)
                  : QString("%1 (%2 points)")
                        .arg(qs(placementWord(placement.kind)))
                        .arg(placement.vertices.size()),
            along ? shown(placement.station) : QString(),
            along && placement.to ? shown(*placement.to) : QString(),
            along ? shown(placement.offset) : QString(),
            qs(katana::cad::road::layerOf(object)),
            quantities.trimmed(),
            built.problems.empty() ? QString() : qs(built.problems.front().message),
        };
        for (int column = 0; column < Column::Count; ++column) {
            d.table->setItem(static_cast<int>(i), column, new QTableWidgetItem(cells[column]));
        }
    }
    d.table->resizeColumnsToContents();
    d.count->setText(QString("%1 road objects, %2 problems").arg(objects.size()).arg(problems) +
                     (unreadable.isEmpty()
                          ? QString()
                          : QString(" - the drawing's road objects cannot be read, and are kept as "
                                    "they are: %1")
                                .arg(unreadable)));
    // What was chosen stays chosen, all of it: selectRow alone would keep
    // only the last.
    QItemSelectionModel* selection = d.table->selectionModel();
    for (int row = 0; row < d.table->rowCount(); ++row) {
        if (kept.contains(d.table->item(row, Column::Id)->text())) {
            selection->select(d.table->model()->index(row, 0),
                              QItemSelectionModel::Select | QItemSelectionModel::Rows);
        }
    }
    const bool one = selectedIds().size() == 1;
    d.apply->setEnabled(one);
    d.copy->setEnabled(one);
    d.remove->setEnabled(!selectedIds().isEmpty());
    d.zoom->setEnabled(!selectedIds().isEmpty());
}

} // namespace katana::qt
