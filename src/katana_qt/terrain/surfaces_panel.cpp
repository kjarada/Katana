// The Surfaces panel (surfaces_panel.hpp; docs/terrain.md, "Surfaces in the
// drawing").

#include "terrain/surfaces_panel.hpp"

#include <QAction>
#include <QComboBox>
#include <QDockWidget>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QMainWindow>
#include <QMenu>
#include <QSignalBlocker>
#include <QTableWidget>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>

#include <utility>

#include "icons.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

namespace cad = katana::cad;

namespace {

// A name as a line takes it: quoted, so blanks and words such as ALL are
// the name and nothing else.
QString quoted(const std::string& name)
{
    return "\"" + QString::fromStdString(name) + "\"";
}

QToolButton* tool(QWidget* parent, Icon which, const QString& objectName, const QString& name,
                  const QString& tip)
{
    auto* button = new QToolButton(parent);
    button->setObjectName(objectName);
    button->setIcon(katana::qt::icon(which));
    button->setIconSize(QSize(18, 18));
    button->setAutoRaise(true);
    button->setToolTip(QString("<b>%1</b><br>%2").arg(name, tip));
    button->setAccessibleName(name);
    button->setAccessibleDescription(tip);
    return button;
}

} // namespace

SurfacesPanel::SurfacesPanel(cad::Document& document, Services services, QWidget* parent)
    : QWidget(parent), document_(document), services_(std::move(services))
{
    setObjectName("surfacesPanel");
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(4, 4, 4, 4);
    layout->setSpacing(4);

    rename_ = tool(this, Icon::Rename, "surfacesRename", "Rename",
                   "Rename the picked surface (SURFACE RENAME): type the new name in its row.");
    copy_ = tool(this, Icon::Copy, "surfacesCopy", "Copy",
                 "Copy the picked surface under a new name (SURFACE COPY), to edit one and keep "
                 "the other.");
    remove_ = tool(this, Icon::Erase, "surfacesRemove", "Remove",
                   "Remove the picked surface from the drawing (SURFACE REMOVE). Undo puts it "
                   "back.");
    zoomTo_ = tool(this, Icon::ZoomTo, "surfacesZoomTo", "Zoom To",
                   "Frame the picked surface in the plan views.");
    editStyles_ = tool(this, Icon::FormatStyles, "surfacesEditStyles", "Edit Styles",
                       "Make and change the styles a surface is drawn in (Terrain > Surface "
                       "Styles).");
    auto* row = new QHBoxLayout();
    row->setSpacing(1);
    for (QToolButton* button : {rename_, copy_, remove_, zoomTo_, editStyles_}) {
        row->addWidget(button);
    }
    row->addStretch();
    layout->addLayout(row);

    table_ = new QTableWidget(0, kColumns, this);
    table_->setObjectName("surfacesTable");
    table_->setHorizontalHeaderLabels({"Name", "Style", "Source", "Triangles"});
    table_->horizontalHeaderItem(Name)->setIcon(katana::qt::icon(Icon::LayerVisible));
    table_->horizontalHeaderItem(Name)->setToolTip(
        "Ticked: the views draw the surface. Double-click a name to rename it.");
    table_->verticalHeader()->setVisible(false);
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->setSelectionMode(QAbstractItemView::SingleSelection);
    // Only the name is typed into, and only when asked (Rename, or a double
    // click): a click picks a row.
    table_->setEditTriggers(QAbstractItemView::DoubleClicked | QAbstractItemView::EditKeyPressed);
    table_->horizontalHeader()->setSectionResizeMode(Name, QHeaderView::Stretch);
    table_->horizontalHeader()->setSectionResizeMode(Style, QHeaderView::ResizeToContents);
    table_->horizontalHeader()->setSectionResizeMode(Triangles, QHeaderView::ResizeToContents);
    layout->addWidget(table_, 1);

    status_ = new QLabel(this);
    status_->setObjectName("surfacesStatus");
    status_->setWordWrap(true);
    layout->addWidget(status_);

    connect(table_, &QTableWidget::itemChanged, this,
            [this](QTableWidgetItem* item) { onItemChanged(item); });
    connect(table_, &QTableWidget::itemSelectionChanged, this, [this] { updateButtons(); });
    connect(rename_, &QToolButton::clicked, this, [this] { renamePicked(); });
    connect(copy_, &QToolButton::clicked, this, [this] { copyPicked(); });
    connect(remove_, &QToolButton::clicked, this, [this] { removePicked(); });
    connect(zoomTo_, &QToolButton::clicked, this, [this] { zoomToPicked(); });
    connect(editStyles_, &QToolButton::clicked, this, [this] {
        if (services_.editStyles) {
            services_.editStyles();
        }
    });

    // The table is rebuilt from the document, so a change the panel's own
    // handler made - a check box ticked, a style chosen - is read once that
    // handler has returned: rebuilding the row under the widget whose signal
    // is still on the stack deletes it there (the Layers panel's crash,
    // main_window.cpp). Any other change is read at once.
    listener_ = document_.addListener([this](const cad::DocumentChange& change) {
        if (!change.has(cad::DocumentChange::Surfaces | cad::DocumentChange::SurfaceStyles)) {
            return;
        }
        if (!acting_) {
            refresh();
            return;
        }
        if (!refreshPending_) {
            refreshPending_ = true;
            QTimer::singleShot(0, this, [this] {
                refreshPending_ = false;
                refresh();
            });
        }
    });
    refresh();
}

void SurfacesPanel::refresh()
{
    if (shownOnce_ && surfacesSeen_ == document_.surfacesGeneration() &&
        stylesSeen_ == document_.surfaceStylesGeneration()) {
        return;
    }
    shownOnce_ = true;
    surfacesSeen_ = document_.surfacesGeneration();
    stylesSeen_ = document_.surfaceStylesGeneration();
    const std::optional<std::string> picked = pickedSurface();

    filling_ = true;
    const QSignalBlocker block(table_);
    const auto& surfaces = document_.surfaces().all();
    const cad::SurfaceStyleSet& styles = document_.surfaceStyles();
    table_->setRowCount(static_cast<int>(surfaces.size()));
    for (int row = 0; row < static_cast<int>(surfaces.size()); ++row) {
        const katana::terrain::NamedSurface& named = surfaces[static_cast<std::size_t>(row)];

        auto* name = new QTableWidgetItem(QString::fromStdString(named.name));
        name->setFlags(Qt::ItemIsEnabled | Qt::ItemIsSelectable | Qt::ItemIsEditable |
                       Qt::ItemIsUserCheckable);
        name->setCheckState(named.visible ? Qt::Checked : Qt::Unchecked);
        name->setToolTip(named.visible ? "Shown: untick to hide it (SURFACE HIDE)"
                                       : "Hidden: tick to show it (SURFACE SHOW)");
        // The name it had, so an edit knows what it renames.
        name->setData(Qt::UserRole, QString::fromStdString(named.name));
        table_->setItem(row, Name, name);

        // The combo is made again with the row: its first entry is the
        // drawing's default, which a surface with no style of its own is in.
        auto* combo = new QComboBox(table_);
        combo->setObjectName("surfaceStyle");
        combo->setToolTip("The style the surface is drawn in (SURFACE STYLE)");
        const cad::SurfaceDisplayStyle* fallback = styles.find(styles.defaultStyle);
        combo->addItem("Default (" + QString::fromStdString(fallback != nullptr ? fallback->name
                                                                                : styles.defaultStyle) +
                           ")",
                       QString());
        int current = 0;
        for (const cad::SurfaceDisplayStyle& style : styles.styles) {
            combo->addItem(QString::fromStdString(style.name), QString::fromStdString(style.name));
            if (!named.style.empty() && katana::core::equalsIgnoringCase(style.name, named.style)) {
                current = combo->count() - 1;
            }
        }
        if (!named.style.empty() && current == 0) {
            // A style the drawing no longer has (a project's from a newer
            // Katana): said, and drawn in the default meanwhile (styleOf).
            combo->addItem(QString::fromStdString(named.style) + " (missing)",
                           QString::fromStdString(named.style));
            current = combo->count() - 1;
        }
        combo->setCurrentIndex(current);
        connect(combo, &QComboBox::activated, this,
                [this, row](int index) { onStyleChosen(row, index); });
        table_->setCellWidget(row, Style, combo);

        auto* source = new QTableWidgetItem(QString::fromStdString(named.source));
        source->setFlags(Qt::ItemIsEnabled | Qt::ItemIsSelectable);
        source->setToolTip(QString::fromStdString(named.source));
        table_->setItem(row, Source, source);

        auto* triangles =
            new QTableWidgetItem(QString::number(named.surface ? named.surface->triangleCount() : 0));
        triangles->setFlags(Qt::ItemIsEnabled | Qt::ItemIsSelectable);
        triangles->setTextAlignment(Qt::AlignRight | Qt::AlignVCenter);
        table_->setItem(row, Triangles, triangles);
    }
    filling_ = false;
    if (picked) {
        (void)pickSurface(*picked);
    }
    if (surfaces.empty()) {
        status_->setText("No surfaces. Terrain > Surface From makes one; an import may bring "
                         "some.");
    } else {
        status_->setText(QString("%1 surfaces").arg(surfaces.size()));
    }
    updateButtons();
}

std::optional<std::string> SurfacesPanel::pickedSurface() const
{
    const int row = table_->currentRow();
    const QList<QTableWidgetItem*> selected = table_->selectedItems();
    if (row < 0 || selected.isEmpty()) {
        return std::nullopt;
    }
    const QTableWidgetItem* name = table_->item(row, Name);
    if (name == nullptr) {
        return std::nullopt;
    }
    return name->data(Qt::UserRole).toString().toStdString();
}

bool SurfacesPanel::pickSurface(std::string_view name)
{
    for (int row = 0; row < table_->rowCount(); ++row) {
        const QTableWidgetItem* item = table_->item(row, Name);
        if (item != nullptr &&
            katana::core::equalsIgnoringCase(item->data(Qt::UserRole).toString().toStdString(),
                                             name)) {
            table_->setCurrentCell(row, Name);
            table_->selectRow(row);
            return true;
        }
    }
    return false;
}

QString SurfacesPanel::lastReply() const
{
    return lastReply_;
}

void SurfacesPanel::run(const QString& line)
{
    if (!services_.run) {
        return;
    }
    acting_ = true;
    const VerbOutcome outcome = services_.run(line);
    acting_ = false;
    lastReply_ = outcome.ok ? outcome.reply : outcome.error;
    if (!outcome.ok) {
        status_->setText(outcome.error);
    }
}

void SurfacesPanel::onItemChanged(QTableWidgetItem* item)
{
    if (filling_ || item == nullptr) {
        return;
    }
    const QTableWidgetItem* nameItem = table_->item(item->row(), Name);
    if (nameItem == nullptr) {
        return;
    }
    if (item->column() != Name) {
        return;
    }
    const std::string name = nameItem->data(Qt::UserRole).toString().toStdString();
    const katana::terrain::NamedSurface* named = document_.surfaces().find(name);
    const bool ticked = item->checkState() == Qt::Checked;
    const QString typed = item->text().trimmed();
    // One cell says two things: the check box whether it is shown, the text
    // its name. Whichever differs from the drawing is what was changed.
    if (named != nullptr && ticked != named->visible) {
        run((ticked ? "SURFACE SHOW " : "SURFACE HIDE ") + quoted(name));
    } else if (!typed.isEmpty() && typed.toStdString() != name) {
        run("SURFACE RENAME " + quoted(name) + " " + quoted(typed.toStdString()));
    } else {
        // Nothing to do: the cell goes back to what the drawing has.
        const QSignalBlocker block(table_);
        item->setText(QString::fromStdString(name));
        return;
    }
    // A refused line leaves the drawing as it was; the row is made again
    // from it, so the cell shows what the drawing has, not what was typed.
    surfacesSeen_ = 0;
    shownOnce_ = false;
    if (!refreshPending_) {
        refreshPending_ = true;
        QTimer::singleShot(0, this, [this] {
            refreshPending_ = false;
            refresh();
        });
    }
}

void SurfacesPanel::onStyleChosen(int row, int index)
{
    if (filling_) {
        return;
    }
    const QTableWidgetItem* nameItem = table_->item(row, Name);
    auto* combo = qobject_cast<QComboBox*>(table_->cellWidget(row, Style));
    if (nameItem == nullptr || combo == nullptr || index < 0) {
        return;
    }
    const std::string name = nameItem->data(Qt::UserRole).toString().toStdString();
    run("SURFACE STYLE " + quoted(name) + " " + quoted(combo->itemData(index).toString().toStdString()));
}

void SurfacesPanel::renamePicked()
{
    const int row = table_->currentRow();
    if (row < 0 || table_->item(row, Name) == nullptr) {
        return;
    }
    table_->editItem(table_->item(row, Name));
}

void SurfacesPanel::copyPicked()
{
    const std::optional<std::string> picked = pickedSurface();
    if (!picked) {
        return;
    }
    // "ground (2)", as an import names a second of a name.
    const std::string copy = document_.surfaces().uniqueName(*picked);
    run("SURFACE COPY " + quoted(*picked) + " " + quoted(copy));
}

void SurfacesPanel::removePicked()
{
    const std::optional<std::string> picked = pickedSurface();
    if (!picked) {
        return;
    }
    run("SURFACE REMOVE " + quoted(*picked));
}

void SurfacesPanel::zoomToPicked()
{
    const std::optional<std::string> picked = pickedSurface();
    if (!picked || !services_.zoomTo) {
        return;
    }
    if (const katana::terrain::NamedSurface* named = document_.surfaces().find(*picked);
        named != nullptr && named->surface && !named->surface->empty()) {
        services_.zoomTo(named->surface->bounds());
    }
}

void SurfacesPanel::updateButtons()
{
    const bool picked = pickedSurface().has_value();
    const bool runs = static_cast<bool>(services_.run);
    rename_->setEnabled(picked && runs);
    copy_->setEnabled(picked && runs);
    remove_->setEnabled(picked && runs);
    zoomTo_->setEnabled(picked && static_cast<bool>(services_.zoomTo));
    editStyles_->setEnabled(static_cast<bool>(services_.editStyles));
}

// ---- the dock -------------------------------------------------------------------------------

void SurfacesPanelUi::show() const
{
    if (dock != nullptr) {
        dock->show();
        dock->raise();
    }
}

SurfacesPanelUi installSurfacesPanel(QMainWindow& window, cad::Document& document,
                                     SurfacesPanel::Services services, QMenu* panels,
                                     QMenu* terrainMenu, QDockWidget* beside)
{
    SurfacesPanelUi ui;
    ui.dock = new QDockWidget("Surfaces", &window);
    ui.dock->setObjectName("SurfacesDock");
    ui.panel = new SurfacesPanel(document, std::move(services), ui.dock);
    ui.dock->setWidget(ui.panel);
    window.addDockWidget(Qt::LeftDockWidgetArea, ui.dock);
    if (beside != nullptr) {
        window.tabifyDockWidget(beside, ui.dock);
    }
    // Until asked for: the window's layout, and every screenshot of it, is
    // as it was for a drawing that has no use for the panel.
    ui.dock->hide();

    ui.toggle = ui.dock->toggleViewAction();
    ui.toggle->setObjectName("viewSurfacesPanel");
    ui.toggle->setText("&Surfaces");
    ui.toggle->setIcon(katana::qt::icon(Icon::SurfacesPanel));
    ui.toggle->setStatusTip("Show or hide the Surfaces panel: the drawing's surfaces, shown or "
                            "hidden, their styles");
    if (panels != nullptr) {
        panels->addAction(ui.toggle);
    }

    ui.terrainItem = new QAction(katana::qt::icon(Icon::SurfacesPanel), "S&urfaces Panel", &window);
    ui.terrainItem->setObjectName("terrainSurfacesPanel");
    ui.terrainItem->setStatusTip("List the drawing's surfaces: show or hide each, choose its "
                                 "style, rename, copy or remove it");
    ui.terrainItem->setToolTip("<b>Surfaces Panel</b><br>" + ui.terrainItem->statusTip());
    QDockWidget* dock = ui.dock;
    QObject::connect(ui.terrainItem, &QAction::triggered, dock, [dock] {
        dock->show();
        dock->raise();
    });
    if (terrainMenu != nullptr) {
        // After Surface From Drawing, with the other surface items; at the
        // end when the menu has no such item.
        QAction* before = nullptr;
        const QList<QAction*> actions = terrainMenu->actions();
        for (qsizetype i = 0; i < actions.size(); ++i) {
            if (actions[i]->objectName() == "surfaceFromDrawing") {
                before = i + 1 < actions.size() ? actions[i + 1] : nullptr;
                break;
            }
        }
        terrainMenu->insertAction(before, ui.terrainItem);
    }
    return ui;
}

} // namespace katana::qt
