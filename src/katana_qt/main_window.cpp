#include "main_window.hpp"

#include "theme.hpp"

#include "icons.hpp"
#include "attribute_manager.hpp"
#include "layer_manager.hpp"
#include "style_manager.hpp"

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QCloseEvent>
#include <QColorDialog>
#include <QComboBox>
#include <QDockWidget>
#include <QFileDialog>
#include <QHBoxLayout>
#include <QHeaderView>
#include <array>
#include <optional>

#include <QDialog>
#include <QDialogButtonBox>
#include <QDoubleSpinBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QCheckBox>
#include <QInputDialog>
#include <QPixmap>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QMenuBar>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStatusBar>
#include <QTableWidget>
#include <QTimer>
#include <QTreeWidget>
#include <QTreeWidgetItem>
#include <QToolBar>
#include <QVBoxLayout>

#include <map>
#include <set>

#include "katana/cad/plot.hpp"
#include "katana/cad/corridor.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/archive12d/customisation.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/storage/project_store.hpp"

namespace katana::qt {

namespace cad = katana::cad;
namespace cmd = katana::commands;
namespace interop = katana::interop;
namespace render = katana::render;
using katana::entity::Entity;
using katana::entity::Layer;

namespace {

// The tree shows the LEAF name in the Name column; the full path is carried
// on the item as user data, because that path is the layer's identity
// everywhere else (entities, the database, an export) and reconstructing it by
// walking parent items would be a second, divergent definition of it.
enum LayerColumn { kName = 0, kVisible, kLocked, kColor, kCount, kLayerColumns };
constexpr int kLayerPathRole = Qt::UserRole + 1;

QString fromPath(const std::filesystem::path& path)
{
    return QString::fromStdWString(path.wstring());
}

std::filesystem::path toPath(const QString& text)
{
    return std::filesystem::path(text.toStdWString());
}

QString number(double value)
{
    return QString::number(value, 'f', 4);
}

QString point(const katana::geometry::Point2& p)
{
    return number(p.x) + ", " + number(p.y);
}

// Rows shown in the property panel for one entity.
std::vector<std::pair<QString, QString>> describeGeometry(const katana::entity::Geometry& geometry)
{
    using Rows = std::vector<std::pair<QString, QString>>;
    struct Visitor {
        Rows operator()(const katana::entity::PointGeometry& g) const
        {
            return {{"Position", point(g.position)}};
        }
        Rows operator()(const katana::geometry::Segment2& g) const
        {
            const double degrees = katana::math::normalizeAngle(g.delta().angle()) *
                                   katana::math::kRadToDeg;
            return {{"Start", point(g.start)},
                    {"End", point(g.end)},
                    {"Length", number(g.length())},
                    {"Angle", number(degrees) + " deg"}};
        }
        Rows operator()(const katana::geometry::Arc2& g) const
        {
            return {{"Centre", point(g.center)},
                    {"Radius", number(g.radius)},
                    {"Start angle", number(g.startAngle * katana::math::kRadToDeg) + " deg"},
                    {"Sweep", number(g.sweep * katana::math::kRadToDeg) + " deg"},
                    {"Length", number(g.length())}};
        }
        Rows operator()(const katana::geometry::Polyline2& g) const
        {
            Rows rows = {{"Vertices", QString::number(g.vertices.size())},
                         {"Closed", g.closed ? "yes" : "no"},
                         {"Length", number(g.length())}};
            if (g.closed) {
                rows.push_back({"Area", number(g.area())});
            }
            return rows;
        }
        Rows operator()(const katana::geometry::Circle2& g) const
        {
            return {{"Centre", point(g.center)},
                    {"Radius", number(g.radius)},
                    {"Circumference", number(g.perimeter())},
                    {"Area", number(g.area())}};
        }
        Rows operator()(const katana::entity::TextGeometry& g) const
        {
            return {{"Position", point(g.position)},
                    {"Text", QString::fromStdString(g.text)},
                    {"Height", number(g.height)},
                    {"Rotation", number(g.rotation * katana::math::kRadToDeg) + " deg"}};
        }
        Rows operator()(const katana::entity::DimensionGeometry& g) const
        {
            return {{"Start", point(g.start)},
                    {"End", point(g.end)},
                    {"Measurement", number(g.measurement())},
                    {"Offset", number(g.offset)}};
        }
    };
    return std::visit(Visitor{}, geometry);
}

QString describeProperty(const katana::entity::PropertyValue& value)
{
    // One definition of what a value says, in entity: the panel, the command
    // line and the attribute manager must not disagree about it.
    return QString::fromStdString(katana::entity::toString(value));
}

} // namespace

MainWindow::MainWindow(QWidget* parent) : QMainWindow(parent)
{
    logger_.setMinimumLevel(katana::core::LogLevel::Warning);
    logger_.addSink(katana::core::makeStderrSink());

    resize(1360, 860);
    views_ = new ViewportContainer(document_, this);
    setCentralWidget(views_);

    buildActions();
    buildDocks();
    buildStatusBar();
    views_->setReferenceData(&reference_);

    views_->onPrompt = [this](const QString& prompt) { statusBar()->showMessage(prompt); };
    views_->onError = [this](const QString& error) { logMessage(error, true); };
    views_->onCursorMoved = [this](const katana::geometry::Point2& world,
                                      const std::optional<cad::SnapResult>& snap) {
        coordinateLabel_->setText(point(world));
        snapLabel_->setText(snap ? cad::toString(snap->mode) : "");
    };
    views_->onStatus = [this](const QString& text) { statusBar()->showMessage(text); };
    views_->onActiveChanged = [this] { refreshViewMenu(); };
    views_->onToolChanged = [this](Tool tool) {
        for (QAction* action : toolGroup_->actions()) {
            if (action->data().toInt() == static_cast<int>(tool)) {
                action->setChecked(true);
            }
        }
    };

    views_->setSurfaces(&sceneSurfaces_);
    refreshViewMenu();
    // QUEUED, NOT DIRECT. The listener fires inside Document::execute, and
    // execute is called from the panels' own signal handlers - the layer
    // tree's itemChanged when a layer is switched off, the property table's
    // cellChanged when a value is edited. A refresh there rebuilds the widget
    // that is mid-signal: QTreeWidget::clear() deletes the very item whose
    // setData is still on the stack, and Qt touches it again on the way out.
    // Switching a layer off crashed the application for exactly that reason.
    // Deferring to the event loop also coalesces one refresh per transaction
    // instead of one per command.
    documentListener_ = document_.addListener([this] { scheduleRefresh(); });
    refreshAll();
    views_->setTool(Tool::Select);
    logMessage("Katana ready. Type HELP for the command list.");
}

// ---- construction -----------------------------------------------------------------------

// A QAction is made ONCE and shared by its menu and its toolbar, so the two
// cannot drift: one text, one shortcut, one icon, one enabled state. `tip` is
// the sentence shown in the status bar and, with the shortcut appended, in the
// toolbar tooltip - an icon-only button owes the user its name.
QAction* MainWindow::makeAction(Icon icon, const QString& text, const QString& tip,
                                const QKeySequence& shortcut)
{
    auto* action = new QAction(katana::qt::icon(icon), text, this);
    if (!shortcut.isEmpty()) {
        action->setShortcut(shortcut);
    }
    QString plain = text;
    plain.remove('&').remove("...");
    action->setStatusTip(tip);
    action->setToolTip(shortcut.isEmpty()
                           ? QString("<b>%1</b><br>%2").arg(plain, tip)
                           : QString("<b>%1</b> &nbsp;<span style='color:%4'>%3</span><br>%2")
                                 .arg(plain, tip, shortcut.toString(QKeySequence::NativeText),
                                      theme::textMuted().name()));
    return action;
}

QToolBar* MainWindow::makeToolBar(const QString& title, Qt::ToolBarArea area)
{
    auto* toolbar = new QToolBar(title, this);
    // The object name is what QMainWindow::saveState keys a toolbar's position
    // on; without one, a saved layout cannot be restored.
    toolbar->setObjectName(title + "ToolBar");
    toolbar->setIconSize(QSize(20, 20));
    toolbar->setToolButtonStyle(Qt::ToolButtonIconOnly);
    toolbar->setMovable(true);
    toolbar->setFloatable(false);
    addToolBar(area, toolbar);
    return toolbar;
}

void MainWindow::buildActions()
{
    // ---- File ------------------------------------------------------------------------
    QAction* newAction = makeAction(Icon::New, "&New", "Start a new, empty drawing", QKeySequence::New);
    QAction* openAction = makeAction(Icon::Open, "&Open Project...", "Open a Katana project directory",
                                     QKeySequence::Open);
    QAction* saveAction =
        makeAction(Icon::Save, "&Save", "Save the project", QKeySequence::Save);
    QAction* saveAsAction = makeAction(Icon::SaveAs, "Save &As...",
                                       "Save the project under another name", QKeySequence::SaveAs);
    QAction* importAction =
        makeAction(Icon::Import, "&Import...",
                   "Import a drawing, an image or a point cloud (DXF, SHP, GeoTIFF, LAS ...)",
                   QKeySequence(Qt::CTRL | Qt::Key_I));
    QAction* exportAction = makeAction(Icon::Export, "Export &Vector...",
                                       "Export the drawing (DXF, GeoPackage, GeoJSON, SHP ...)",
                                       QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_E));
    QAction* plotAction = makeAction(Icon::Plot, "&Plot to PDF...",
                                     "Plot the drawing to a sheet at a standard scale",
                                     QKeySequence::Print);
    connect(newAction, &QAction::triggered, this, [this] { newDocument(); });
    connect(openAction, &QAction::triggered, this, [this] { openDocument(); });
    connect(saveAction, &QAction::triggered, this, [this] { saveDocument(); });
    connect(saveAsAction, &QAction::triggered, this, [this] { saveDocumentAs(); });
    connect(importAction, &QAction::triggered, this, [this] { importFile(); });
    connect(exportAction, &QAction::triggered, this, [this] { exportVectorFile(); });
    connect(plotAction, &QAction::triggered, this, [this] { plotToPdf(); });

    QAction* customiseAction =
        makeAction(Icon::Import, "Load 12d &Customisation...",
                   "Load 12d linestyle and symbol libraries (.4d) and mapfiles, so a survey "
                   "code draws what the customisation says it should");
    connect(customiseAction, &QAction::triggered, this, [this] { loadCustomisation(); });

    QAction* codeAction = makeAction(Icon::Import, "Apply Survey &Codes",
                                     "Give every entity carrying a field code the model, style "
                                     "and attributes the loaded mapfile says it should have");
    connect(codeAction, &QAction::triggered, this, [this] { applySurveyCodes(); });

    QMenu* fileMenu = menuBar()->addMenu("&File");
    fileMenu->addActions({newAction, openAction});
    fileMenu->addSeparator();
    fileMenu->addActions({saveAction, saveAsAction});
    fileMenu->addSeparator();
    fileMenu->addActions({importAction, exportAction});
    fileMenu->addSeparator();
    fileMenu->addActions({customiseAction, codeAction});
    fileMenu->addSeparator();
    fileMenu->addAction(plotAction);
    fileMenu->addSeparator();
    fileMenu->addAction("&Quit", QKeySequence::Quit, this, [this] { close(); });

    QToolBar* fileBar = makeToolBar("File", Qt::TopToolBarArea);
    fileBar->addActions({newAction, openAction, saveAction});
    fileBar->addSeparator();
    fileBar->addActions({importAction, exportAction, plotAction});

    // ---- Edit ------------------------------------------------------------------------
    undoAction_ = makeAction(Icon::Undo, "&Undo", "Undo the last command", QKeySequence::Undo);
    redoAction_ = makeAction(Icon::Redo, "&Redo", "Redo the command that was undone",
                             QKeySequence::Redo);
    QAction* selectAllAction = makeAction(Icon::SelectAll, "Select &All",
                                          "Select every entity on an unlocked layer",
                                          QKeySequence::SelectAll);
    QAction* eraseAction = makeAction(Icon::Erase, "&Erase Selection", "Erase the selected entities",
                                      QKeySequence(Qt::Key_Delete));
    connect(undoAction_, &QAction::triggered, this, [this] {
        if (const auto status = document_.undo(); !status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
        }
    });
    connect(redoAction_, &QAction::triggered, this, [this] {
        if (const auto status = document_.redo(); !status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
        }
    });
    connect(selectAllAction, &QAction::triggered, this, [this] { selectAll(); });
    connect(eraseAction, &QAction::triggered, this, [this] {
        commandInput_->setText("ERASE");
        runCommandLine();
    });

    QMenu* editMenu = menuBar()->addMenu("&Edit");
    editMenu->addActions({undoAction_, redoAction_});
    editMenu->addSeparator();
    editMenu->addAction(selectAllAction);
    editMenu->addAction("&Deselect", QKeySequence(Qt::Key_Escape), this, [this] { views_->cancel(); });
    editMenu->addAction(eraseAction);
    editMenu->addSeparator();
    editMenu->addAction("St&yles and Linetypes...", this, [this] {
        auto dialog = makeStyleManager();
        dialog->showFirstRows();
        dialog->exec();
    });
    editMenu->addAction("&Layers...", QKeySequence(Qt::CTRL | Qt::Key_L), this, [this] {
        auto dialog = makeLayerManager();
        dialog->showFirstRow();
        dialog->exec();
        // The dock shows the same layers, so it follows the dialog.
        scheduleRefresh();
    });
    editMenu->addAction("&Attributes...", QKeySequence(Qt::CTRL | Qt::Key_1), this, [this] {
        auto dialog = makeAttributeManager();
        dialog->expandAll();
        dialog->exec();
        // The panel shows the same properties, so it follows the dialog.
        refreshProperties();
    });

    QToolBar* editBar = makeToolBar("Edit", Qt::TopToolBarArea);
    editBar->addActions({undoAction_, redoAction_});
    editBar->addSeparator();
    editBar->addActions({selectAllAction, eraseAction});

    // ---- View ------------------------------------------------------------------------
    QAction* extentsAction = makeAction(Icon::ZoomExtents, "Zoom &Extents",
                                        "Fit the whole drawing in the view",
                                        QKeySequence(Qt::CTRL | Qt::Key_E));
    connect(extentsAction, &QAction::triggered, this, [this] { views_->zoomExtents(); });
    gridAction_ = makeAction(Icon::Grid, "&Grid", "Show or hide the grid", QKeySequence(Qt::Key_F7));
    gridAction_->setCheckable(true);
    gridAction_->setChecked(views_->gridVisible());
    connect(gridAction_, &QAction::toggled, this, [this](bool on) { views_->setGridVisible(on); });
    snapAction_ = makeAction(Icon::Snap, "Object &Snap",
                             "Snap the cursor to endpoints, midpoints, centres and intersections",
                             QKeySequence(Qt::Key_F3));
    snapAction_->setCheckable(true);
    snapAction_->setChecked(views_->snapEnabled());
    connect(snapAction_, &QAction::toggled, this, [this](bool on) { views_->setSnapEnabled(on); });

    viewMenu_ = menuBar()->addMenu("&View");
    viewMenu_->addAction(extentsAction);
    viewMenu_->addActions({gridAction_, snapAction_});

    QMenu* snapMenu = viewMenu_->addMenu("Snap &Modes");
    for (const cad::SnapMode mode :
         {cad::SnapMode::Endpoint, cad::SnapMode::Midpoint, cad::SnapMode::Center,
          cad::SnapMode::Intersection, cad::SnapMode::Perpendicular, cad::SnapMode::Tangent,
          cad::SnapMode::Nearest, cad::SnapMode::Grid}) {
        QAction* action = snapMenu->addAction(cad::toString(mode));
        action->setCheckable(true);
        action->setChecked(cad::hasMode(views_->snapModes(), mode));
        connect(action, &QAction::toggled, this, [this, mode](bool on) {
            const auto bit = static_cast<cad::SnapModes>(mode);
            const cad::SnapModes modes = views_->snapModes();
            views_->setSnapModes(on ? (modes | bit) : (modes & ~bit));
        });
    }

    buildViewMenu(viewMenu_);

    QToolBar* viewBar = makeToolBar("View", Qt::TopToolBarArea);
    viewBar->addAction(extentsAction);
    viewBar->addSeparator();
    viewBar->addActions({gridAction_, snapAction_});

    // ---- Terrain and civil -----------------------------------------------------------
    QAction* cloudSurface = makeAction(Icon::SurfaceFromCloud, "Surface From &Point Cloud...",
                                       "Triangulate a surface from an imported point cloud");
    QAction* rasterSurface = makeAction(Icon::SurfaceFromRaster, "Surface From &Raster...",
                                        "Triangulate a surface from an elevation raster");
    QAction* drawingSurface = makeAction(Icon::SurfaceFromDrawing, "Surface From &Drawing",
                                         "Triangulate a surface from the drawing's points and lines");
    QAction* quantities = makeAction(Icon::CorridorQuantities, "Corridor &Quantities...",
                                     "Cut and fill along an alignment, by average end area",
                                     QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Q));
    QAction* corridor = makeAction(Icon::CorridorSurface, "Corridor &Surface...",
                                   "Build the finished design along an alignment as a surface");
    QAction* alignmentSection = makeAction(Icon::SectionAlignment, "Cut Section Along &Alignment...",
                                           "Long section down a named alignment, with its design profile",
                                           QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_K));
    QAction* selectionSection = makeAction(Icon::Section, "&Cut Section Along Selection",
                                           "Long section along the selected line or polyline",
                                           QKeySequence(Qt::CTRL | Qt::Key_K));
    connect(cloudSurface, &QAction::triggered, this, [this] { buildSurfaceFromPointCloud(); });
    connect(rasterSurface, &QAction::triggered, this, [this] { buildSurfaceFromRaster(); });
    connect(drawingSurface, &QAction::triggered, this, [this] { buildSurfaceFromDrawing(); });
    connect(quantities, &QAction::triggered, this, &MainWindow::corridorQuantities);
    connect(corridor, &QAction::triggered, this, &MainWindow::corridorSurface);
    connect(alignmentSection, &QAction::triggered, this, &MainWindow::cutSectionAlongAlignment);
    connect(selectionSection, &QAction::triggered, this, [this] { cutSectionAlongSelection(); });

    QMenu* terrainMenu = menuBar()->addMenu("&Terrain");
    terrainMenu->addActions({cloudSurface, rasterSurface, drawingSurface});
    terrainMenu->addSeparator();
    terrainMenu->addActions({selectionSection, alignmentSection});
    terrainMenu->addSeparator();
    terrainMenu->addActions({quantities, corridor});

    QToolBar* terrainBar = makeToolBar("Terrain", Qt::TopToolBarArea);
    terrainBar->addActions({cloudSurface, rasterSurface, drawingSurface});
    terrainBar->addSeparator();
    terrainBar->addActions({selectionSection, alignmentSection});
    terrainBar->addSeparator();
    terrainBar->addActions({quantities, corridor});

    // ---- Draw and Modify -------------------------------------------------------------
    // Down the LEFT edge, where every CAD program keeps its drawing tools:
    // they are the ones reached for without looking, and a vertical strip
    // beside the drawing is a shorter mouse journey than a row above it.
    QMenu* drawMenu = menuBar()->addMenu("&Draw");
    QMenu* modifyMenu = menuBar()->addMenu("&Modify");
    QToolBar* drawBar = makeToolBar("Draw", Qt::LeftToolBarArea);
    toolGroup_ = new QActionGroup(this);
    toolGroup_->setExclusive(true);

    struct ToolEntry {
        Tool tool;
        Icon icon;
        const char* tip;
        QKeySequence shortcut;
    };
    const ToolEntry tools[] = {
        {Tool::Select, Icon::Select, "Pick entities, or drag a window or crossing box", {}},
        {Tool::Point, Icon::Point, "Place a point", {}},
        {Tool::Line, Icon::Line, "Draw a line between two points", {}},
        {Tool::Polyline, Icon::Polyline, "Draw a polyline; right-click or Enter to finish", {}},
        {Tool::Rectangle, Icon::Rectangle, "Draw a rectangle by two corners", {}},
        {Tool::Circle, Icon::Circle, "Draw a circle by centre and radius", {}},
        {Tool::Arc, Icon::Arc, "Draw an arc through three points", {}},
        {Tool::Move, Icon::Move, "Move the selection by two points", {}},
        {Tool::Copy, Icon::Copy, "Copy the selection by two points", {}},
    };
    for (const ToolEntry& entry : tools) {
        QAction* action = makeAction(entry.icon, toString(entry.tool), entry.tip, entry.shortcut);
        action->setCheckable(true);
        action->setData(static_cast<int>(entry.tool));
        const Tool tool = entry.tool;
        connect(action, &QAction::triggered, this, [this, tool] { views_->setTool(tool); });
        toolGroup_->addAction(action);
        drawBar->addAction(action);
        if (tool == Tool::Move || tool == Tool::Copy) {
            modifyMenu->addAction(action);
        } else if (tool != Tool::Select) {
            drawMenu->addAction(action);
        }
        if (tool == Tool::Select || tool == Tool::Arc) {
            drawBar->addSeparator();
        }
    }
    modifyMenu->addAction(eraseAction);
    modifyMenu->addSeparator();
    QAction* hint = modifyMenu->addAction(
        "Rotate, Scale, Mirror, Array, Trim, Extend, Offset, Fillet, Chamfer: command line");
    hint->setEnabled(false);

    // ---- Help ------------------------------------------------------------------------
    QAction* reference = makeAction(Icon::Help, "&Command Reference",
                                    "List every command the command line accepts",
                                    QKeySequence::HelpContents);
    QAction* about = makeAction(Icon::About, "&About Katana", "Version and build information");
    connect(reference, &QAction::triggered, this, [this] {
        logMessage(QString::fromStdString(cad::CommandInterpreter::helpText()));
    });
    connect(about, &QAction::triggered, this, [this] {
        QMessageBox box(this);
        box.setWindowTitle("About Katana");
        box.setIconPixmap(QPixmap::fromImage(applicationIconImage(96)));
        box.setText(QString("<h3>Katana %1</h3>"
                            "<p>High-performance survey and CAD platform.</p>"
                            "<p style='color:%2'>Deterministic C++23 engineering core, "
                            "Qt %3 desktop shell.</p>")
                        .arg(KATANA_VERSION, theme::textMuted().name(), qVersion()));
        box.exec();
    });
    QMenu* helpMenu = menuBar()->addMenu("&Help");
    helpMenu->addActions({reference, about});
}

void MainWindow::buildDocks()
{
    // ---- layers ----
    auto* layerDock = new QDockWidget("Layers", this);
    auto* layerPanel = new QWidget(layerDock);
    auto* layerLayout = new QVBoxLayout(layerPanel);
    layerLayout->setContentsMargins(4, 4, 4, 4);
    layerTree_ = new QTreeWidget(layerPanel);
    layerTree_->setColumnCount(kLayerColumns);
    layerTree_->setHeaderLabels({"Layer", "On", "Lock", "Colour", "N"});
    layerTree_->setSelectionMode(QAbstractItemView::SingleSelection);
    layerTree_->setUniformRowHeights(true);
    layerTree_->setExpandsOnDoubleClick(false); // double click sets the current layer
    layerTree_->header()->setSectionResizeMode(kName, QHeaderView::Stretch);
    for (const int column : {kVisible, kLocked, kColor, kCount}) {
        layerTree_->header()->setSectionResizeMode(column, QHeaderView::ResizeToContents);
    }
    layerLayout->addWidget(layerTree_);

    auto* buttons = new QHBoxLayout();
    auto* addButton = new QPushButton("New", layerPanel);
    auto* childButton = new QPushButton("New Child", layerPanel);
    auto* renameButton = new QPushButton("Rename", layerPanel);
    auto* deleteButton = new QPushButton("Delete", layerPanel);
    buttons->addWidget(addButton);
    buttons->addWidget(childButton);
    buttons->addWidget(renameButton);
    buttons->addWidget(deleteButton);
    layerLayout->addLayout(buttons);
    layerDock->setWidget(layerPanel);
    addDockWidget(Qt::LeftDockWidgetArea, layerDock);

    connect(addButton, &QPushButton::clicked, this, [this] { addLayer(); });
    connect(childButton, &QPushButton::clicked, this, [this] { addChildLayer(); });
    connect(renameButton, &QPushButton::clicked, this, [this] { renameSelectedLayer(); });
    connect(deleteButton, &QPushButton::clicked, this, [this] { deleteCurrentLayer(); });
    connect(layerTree_, &QTreeWidget::itemChanged, this,
            [this](QTreeWidgetItem* item, int column) { onLayerItemChanged(item, column); });
    connect(layerTree_, &QTreeWidget::itemDoubleClicked, this,
            [this](QTreeWidgetItem* item, int column) { onLayerItemDoubleClicked(item, column); });

    // ---- properties ----
    auto* propertyDock = new QDockWidget("Properties", this);
    propertyTable_ = new QTableWidget(0, 2, propertyDock);
    propertyTable_->setHorizontalHeaderLabels({"Property", "Value"});
    propertyTable_->verticalHeader()->setVisible(false);
    propertyTable_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    propertyTable_->horizontalHeader()->setSectionResizeMode(0, QHeaderView::ResizeToContents);
    propertyTable_->horizontalHeader()->setSectionResizeMode(1, QHeaderView::Stretch);
    propertyDock->setWidget(propertyTable_);
    addDockWidget(Qt::RightDockWidgetArea, propertyDock);

    // ---- command line ----
    auto* commandDock = new QDockWidget("Command Line", this);
    auto* commandPanel = new QWidget(commandDock);
    auto* commandLayout = new QVBoxLayout(commandPanel);
    commandLayout->setContentsMargins(4, 4, 4, 4);
    commandLog_ = new QPlainTextEdit(commandPanel);
    commandLog_->setReadOnly(true);
    commandLog_->setMaximumBlockCount(2000);
    commandLog_->setFont(QFont("Consolas", 9));
    commandInput_ = new QLineEdit(commandPanel);
    commandInput_->setFont(QFont("Consolas", 10));
    commandInput_->setPlaceholderText(
        "Command:  LINE 0,0 10,0 @0,5   |   CIRCLE 5,5 3   |   SELECT ALL   |   HELP");
    commandInput_->installEventFilter(this);
    commandLayout->addWidget(commandLog_);
    commandLayout->addWidget(commandInput_);
    commandDock->setWidget(commandPanel);
    addDockWidget(Qt::BottomDockWidgetArea, commandDock);
    connect(commandInput_, &QLineEdit::returnPressed, this, [this] { runCommandLine(); });

    buildReferenceDock();

    // Every panel can be closed, so every panel needs a way back. Qt makes the
    // toggle action; it only has to be put somewhere the user will look.
    for (QDockWidget* dock : {layerDock, propertyDock, commandDock}) {
        dock->setObjectName(dock->windowTitle() + "Dock"); // for saveState, as the toolbars
    }
    layerDock->toggleViewAction()->setIcon(katana::qt::icon(Icon::Layers));
    propertyDock->toggleViewAction()->setIcon(katana::qt::icon(Icon::Properties));
    viewMenu_->addSeparator();
    QMenu* panels = viewMenu_->addMenu("&Panels");
    panels->addActions({layerDock->toggleViewAction(), propertyDock->toggleViewAction(),
                        commandDock->toggleViewAction()});
    if (referenceDock_ != nullptr) {
        panels->addAction(referenceDock_->toggleViewAction());
    }

    // Opening sizes. Left to itself Qt gives each dock its size hint, which
    // for a text log is a third of the window - so the drawing, which is the
    // point of the program, opened in the space left over. The drawing gets
    // the room; the panels get what they need to be read.
    resizeDocks({layerDock, propertyDock}, {300, 270}, Qt::Horizontal);
    resizeDocks({commandDock}, {150}, Qt::Vertical);
}

void MainWindow::buildReferenceDock()
{
    auto* dock = new QDockWidget("Reference Data", this);
    dock->setObjectName("ReferenceDataDock");
    referenceDock_ = dock;
    auto* panel = new QWidget(dock);
    auto* layout = new QVBoxLayout(panel);
    layout->setContentsMargins(4, 4, 4, 4);

    referenceTable_ = new QTableWidget(0, 4, panel);
    referenceTable_->setHorizontalHeaderLabels({"Name", "Type", "Detail", "Display"});
    referenceTable_->horizontalHeader()->setStretchLastSection(true);
    referenceTable_->verticalHeader()->setVisible(false);
    referenceTable_->setSelectionBehavior(QAbstractItemView::SelectRows);
    referenceTable_->setSelectionMode(QAbstractItemView::SingleSelection);
    layout->addWidget(referenceTable_);

    auto* buttons = new QHBoxLayout();
    auto* importButton = new QPushButton("Import...", panel);
    auto* zoomButton = new QPushButton("Zoom To", panel);
    auto* removeButton = new QPushButton("Remove", panel);
    buttons->addWidget(importButton);
    buttons->addWidget(zoomButton);
    buttons->addWidget(removeButton);
    buttons->addStretch();
    layout->addLayout(buttons);

    dock->setWidget(panel);
    addDockWidget(Qt::LeftDockWidgetArea, dock);

    connect(importButton, &QPushButton::clicked, this, [this] { importFile(); });
    connect(zoomButton, &QPushButton::clicked, this, [this] { zoomToSelectedReference(); });
    connect(removeButton, &QPushButton::clicked, this, [this] { removeSelectedReference(); });
    connect(referenceTable_, &QTableWidget::cellChanged, this,
            [this](int row, int column) { onReferenceCellChanged(row, column); });
    connect(referenceTable_, &QTableWidget::cellDoubleClicked, this,
            [this](int, int) { zoomToSelectedReference(); });
}

void MainWindow::buildStatusBar()
{
    coordinateLabel_ = new QLabel("0.0000, 0.0000", this);
    coordinateLabel_->setMinimumWidth(200);
    snapLabel_ = new QLabel(this);
    snapLabel_->setMinimumWidth(90);
    layerLabel_ = new QLabel(this);
    statusBar()->addPermanentWidget(layerLabel_);
    statusBar()->addPermanentWidget(snapLabel_);
    statusBar()->addPermanentWidget(coordinateLabel_);
}

// ---- refresh --------------------------------------------------------------------------------

void MainWindow::scheduleRefresh()
{
    if (refreshPending_) {
        return;
    }
    refreshPending_ = true;
    QTimer::singleShot(0, this, [this] {
        refreshPending_ = false;
        refreshAll();
    });
}

void MainWindow::warnUser(const QString& title, const QString& text)
{
    // Already in the command log by the time this is called; the box is for
    // the person at the screen, and in a headless run there is none - a
    // modal box there is a hang. A missing file on a scripted import used to
    // do exactly that.
    if (headless_) {
        logMessage(title + ": " + text.simplified(), true);
        return;
    }
    QMessageBox::warning(this, title, text);
}

katana::core::Status MainWindow::toggleLayerThroughPanel(const QString& layer)
{
    using katana::core::ErrorCode;
    using katana::core::makeError;
    const std::string name = layer.toStdString();
    const Layer* before = document_.model().layers.find(name);
    if (before == nullptr) {
        return makeError(ErrorCode::NotFound, "no such layer", name);
    }
    const bool wasVisible = before->visible;

    // The items as they are now. If the slot rebuilds the tree, these are
    // deleted by the time setCheckState returns - the bug this checks for -
    // and the tree holds different pointers. Only pointer VALUES are compared
    // afterwards; the old items are never dereferenced.
    std::vector<const QTreeWidgetItem*> itemsBefore;
    QTreeWidgetItem* target = nullptr;
    for (QTreeWidgetItemIterator it(layerTree_); *it != nullptr; ++it) {
        itemsBefore.push_back(*it);
        if ((*it)->data(kName, kLayerPathRole).toString() == layer) {
            target = *it;
        }
    }
    if (target == nullptr) {
        return makeError(ErrorCode::NotFound, "the layer is not in the panel", name);
    }
    target->setCheckState(kVisible, wasVisible ? Qt::Unchecked : Qt::Checked);

    std::vector<const QTreeWidgetItem*> itemsAfter;
    for (QTreeWidgetItemIterator it(layerTree_); *it != nullptr; ++it) {
        itemsAfter.push_back(*it);
    }
    if (itemsAfter != itemsBefore) {
        return makeError(ErrorCode::Internal,
                         "the layer panel was rebuilt inside its own itemChanged signal, "
                         "deleting the item Qt was still using");
    }

    // The refresh runs on the event loop; give it that.
    QApplication::processEvents();
    QApplication::processEvents();
    const Layer* after = document_.model().layers.find(name);
    if (after == nullptr || after->visible == wasVisible) {
        return makeError(ErrorCode::Internal, "the document did not record the change", name);
    }
    for (QTreeWidgetItemIterator it(layerTree_); *it != nullptr; ++it) {
        if ((*it)->data(kName, kLayerPathRole).toString() == layer) {
            const bool shown = (*it)->checkState(kVisible) == Qt::Checked;
            if (shown != after->visible) {
                return makeError(ErrorCode::Internal, "the panel does not show the change", name);
            }
            return {};
        }
    }
    return makeError(ErrorCode::Internal, "the layer vanished from the panel", name);
}

void MainWindow::refreshAll()
{
    refreshTitle();
    refreshLayers();
    refreshProperties();
    refreshReferences();
    undoAction_->setEnabled(document_.history().canUndo());
    redoAction_->setEnabled(document_.history().canRedo());
    undoAction_->setText(document_.history().canUndo()
                             ? "&Undo " + QString::fromUtf8(document_.history().undoName().data(),
                                                            static_cast<int>(document_.history().undoName().size()))
                             : "&Undo");
    layerLabel_->setText("Layer: " + QString::fromStdString(document_.currentLayer()));
}

void MainWindow::refreshTitle()
{
    const auto directory = document_.projectDirectory();
    const QString name = directory ? fromPath(directory->filename()) : "Untitled";
    setWindowTitle(name + "[*] - Katana");
    setWindowModified(document_.isModified());
}

void MainWindow::refreshLayers()
{
    refreshingLayers_ = true;

    // Which branches were open, and what was selected, so a rebuild does not
    // collapse the tree under the user every time an entity is drawn.
    std::set<std::string> expanded;
    for (QTreeWidgetItemIterator it(layerTree_); *it != nullptr; ++it) {
        if ((*it)->isExpanded()) {
            expanded.insert((*it)->data(kName, kLayerPathRole).toString().toStdString());
        }
    }
    const std::string selected = selectedLayerPath();

    layerTree_->clear();
    const auto& layers = document_.model().layers;
    // names() is ascending by full path, which IS a pre-order walk of the tree
    // (see layer_path.hpp), so every parent is created before its children and
    // one pass is enough.
    std::map<std::string, QTreeWidgetItem*> items;
    QTreeWidgetItem* toSelect = nullptr;

    for (const std::string& path : layers.names()) {
        const Layer* layer = layers.find(path);
        if (layer == nullptr) {
            continue;
        }
        const auto parentPath = katana::entity::layerParent(path);
        QTreeWidgetItem* parent = nullptr;
        if (!parentPath.empty()) {
            const auto found = items.find(std::string(parentPath));
            if (found != items.end()) {
                parent = found->second;
            }
        }
        auto* item = parent != nullptr ? new QTreeWidgetItem(parent)
                                       : new QTreeWidgetItem(layerTree_);
        item->setText(kName, QString::fromStdString(std::string(katana::entity::layerLeaf(path))));
        item->setData(kName, kLayerPathRole, QString::fromStdString(path));
        item->setToolTip(kName, QString::fromStdString(path));
        item->setFlags(Qt::ItemIsEnabled | Qt::ItemIsSelectable | Qt::ItemIsUserCheckable);
        item->setCheckState(kVisible, layer->visible ? Qt::Checked : Qt::Unchecked);
        item->setCheckState(kLocked, layer->locked ? Qt::Checked : Qt::Unchecked);

        item->setText(kColor, QString::fromStdString(layer->color.toHex()));
        item->setBackground(kColor, QColor(layer->color.r, layer->color.g, layer->color.b));
        item->setForeground(kColor, layer->color.r + layer->color.g + layer->color.b > 380
                                        ? QBrush(Qt::black)
                                        : QBrush(Qt::white));
        item->setText(kCount, QString::number(document_.model().entities.countOnLayer(path)));

        // The current layer is where new geometry lands, so it is called out
        // rather than left to be worked out from the status bar.
        if (path == document_.currentLayer()) {
            QFont bold = layerTree_->font();
            bold.setBold(true);
            item->setFont(kName, bold);
            item->setText(kName, item->text(kName) + "  <");
        }
        // A layer switched off by an ancestor is greyed, because its own tick
        // is still on and would otherwise say it was visible when it is not.
        if (!layers.effectivelyVisible(path)) {
            item->setForeground(kName, QBrush(QColor(130, 130, 130)));
        }
        if (layers.effectivelyLocked(path)) {
            item->setToolTip(kLocked, "Locked here or by a parent layer");
        }

        items.emplace(path, item);
        if (path == selected) {
            toSelect = item;
        }
        item->setExpanded(expanded.empty() ? true : expanded.count(path) != 0);
    }

    if (toSelect != nullptr) {
        layerTree_->setCurrentItem(toSelect);
    }
    refreshingLayers_ = false;
}

std::string MainWindow::selectedLayerPath() const
{
    const QTreeWidgetItem* item = layerTree_ != nullptr ? layerTree_->currentItem() : nullptr;
    return item == nullptr ? std::string{}
                           : item->data(kName, kLayerPathRole).toString().toStdString();
}

void MainWindow::refreshProperties()
{
    std::vector<std::pair<QString, QString>> rows;
    const auto ids = document_.selection().ids();
    if (ids.empty()) {
        rows.push_back({"Selection", "none"});
        rows.push_back({"Entities", QString::number(document_.model().entities.size())});
    } else if (ids.size() == 1) {
        const Entity& entity = *document_.model().entities.find(ids.front());
        rows.push_back({"Id", QString::number(entity.id)});
        rows.push_back({"Type", QString::fromUtf8(toString(entity.type()).data())});
        rows.push_back({"Layer", QString::fromStdString(entity.layer)});
        rows.push_back({"Colour", entity.color ? QString::fromStdString(entity.color->toHex())
                                               : QString("ByLayer")});
        for (auto& row : describeGeometry(entity.geometry)) {
            rows.push_back(std::move(row));
        }
        for (const auto& [key, value] : entity.properties) {
            rows.push_back({QString::fromStdString(key), describeProperty(value)});
        }
    } else {
        rows.push_back({"Selection", QString::number(ids.size()) + " entities"});
        std::map<QString, int> byType;
        for (const auto id : ids) {
            ++byType[QString::fromUtf8(toString(document_.model().entities.find(id)->type()).data())];
        }
        for (const auto& [type, count] : byType) {
            rows.push_back({type, QString::number(count)});
        }
    }
    propertyTable_->setRowCount(static_cast<int>(rows.size()));
    for (int row = 0; row < static_cast<int>(rows.size()); ++row) {
        propertyTable_->setItem(row, 0, new QTableWidgetItem(rows[static_cast<std::size_t>(row)].first));
        propertyTable_->setItem(row, 1, new QTableWidgetItem(rows[static_cast<std::size_t>(row)].second));
    }
}

// ---- file handling ----------------------------------------------------------------------------

bool MainWindow::confirmDiscard()
{
    if (!document_.isModified()) {
        return true;
    }
    const auto choice = QMessageBox::warning(
        this, "Unsaved Changes", "The drawing has unsaved changes. Save them first?",
        QMessageBox::Save | QMessageBox::Discard | QMessageBox::Cancel, QMessageBox::Save);
    if (choice == QMessageBox::Cancel) {
        return false;
    }
    return choice == QMessageBox::Discard || saveDocument();
}

void MainWindow::newDocument()
{
    if (confirmDiscard()) {
        // The view and the interpreter both hold state that belongs to the
        // drawing being discarded - half-picked points, and the last point that
        // relative coordinates measure from. Both must go with it.
        views_->resetInteraction();
        interpreter_.resetPointState();
        document_.newDocument();
        views_->zoomExtents();
        logMessage("New drawing.");
    }
}

void MainWindow::openDocument()
{
    if (!confirmDiscard()) {
        return;
    }
    const QString directory =
        QFileDialog::getExistingDirectory(this, "Open Katana Project (select the .katana folder)");
    if (!directory.isEmpty()) {
        openProject(directory);
    }
}

void MainWindow::selectAll()
{
    logMessage(QString::fromStdString(interpreter_.run("SELECT ALL").valueOr("")));
}

void MainWindow::selectOnly(katana::entity::EntityId id)
{
    logMessage(QString::fromStdString(
        interpreter_.run("SELECT " + std::to_string(id)).valueOr("")));
}

std::unique_ptr<LayerManagerDialog> MainWindow::makeLayerManager()
{
    return std::make_unique<LayerManagerDialog>(
        document_, [this](const QString& message, bool isError) { logMessage(message, isError); },
        this);
}

std::unique_ptr<AttributeManagerDialog> MainWindow::makeAttributeManager()
{
    return std::make_unique<AttributeManagerDialog>(
        document_, [this](const QString& message, bool isError) { logMessage(message, isError); },
        this);
}

std::unique_ptr<StyleManagerDialog> MainWindow::makeStyleManager()
{
    return std::make_unique<StyleManagerDialog>(
        document_, [this](const QString& message, bool isError) { logMessage(message, isError); },
        this);
}

void MainWindow::openProject(const QString& directory)
{
    const std::filesystem::path path = toPath(directory);
    auto status = document_.open(path);
    // Damaged database: offer the newest sound backup. Nothing is deleted. A
    // headless run has nobody to answer and does not restore.
    if (!status && status.error().code == katana::core::ErrorCode::DatabaseFailure && !headless_) {
        const auto answer = QMessageBox::question(
            this, "Project Damaged",
            QString::fromStdString(status.error().describe()) +
                "\n\nRestore the most recent intact backup? The damaged file is kept.");
        if (answer == QMessageBox::Yes) {
            const auto report = katana::storage::ProjectStore::recover(path);
            if (report) {
                logMessage("Restored from " + fromPath(report->restoredFrom.filename()));
                status = document_.open(path);
            } else {
                status = report.error();
            }
        }
    }
    if (!status) {
        warnUser("Open Failed", QString::fromStdString(status.error().describe()));
        return;
    }
    views_->resetInteraction();
    interpreter_.resetPointState();
    views_->zoomExtents();
    logMessage("Opened " + directory + " (" +
               QString::number(document_.model().entities.size()) + " entities).");
}

bool MainWindow::saveDocument()
{
    if (!document_.hasProject()) {
        return saveDocumentAs();
    }
    const auto status = document_.save();
    if (!status) {
        warnUser("Save Failed", QString::fromStdString(status.error().describe()));
        return false;
    }
    logMessage("Saved.");
    return true;
}

bool MainWindow::saveDocumentAs()
{
    QString target = QFileDialog::getSaveFileName(this, "Save Project As", "untitled.katana",
                                                  "Katana project (*.katana)");
    if (target.isEmpty()) {
        return false;
    }
    if (!target.endsWith(".katana", Qt::CaseInsensitive)) {
        target += ".katana";
    }
    const auto status = document_.saveAs(toPath(target));
    if (!status) {
        warnUser("Save Failed", QString::fromStdString(status.error().describe()));
        return false;
    }
    logMessage("Saved to " + target);
    return true;
}

void MainWindow::closeEvent(QCloseEvent* event)
{
    if (confirmDiscard()) {
        event->accept();
    } else {
        event->ignore();
    }
}

// ---- command line -----------------------------------------------------------------------------

void MainWindow::logMessage(const QString& text, bool isError)
{
    if (text.isEmpty()) {
        return;
    }
    commandLog_->appendPlainText(isError ? "! " + text : text);
    if (isError) {
        statusBar()->showMessage(text, 6000);
    }
}

void MainWindow::runCommandLine()
{
    const QString line = commandInput_->text().trimmed();
    commandInput_->clear();
    if (line.isEmpty()) {
        return;
    }
    commandLog_->appendPlainText("> " + line);
    const QStringList words = line.split(' ', Qt::SkipEmptyParts);
    const QString verb = words.front().toUpper();
    const QString argument = words.size() > 1 ? words[1].toUpper() : QString();

    // Commands that concern the view rather than the document.
    if (verb == "ZOOM" || verb == "Z") {
        views_->zoomExtents();
        return;
    }
    if (verb == "GRID") {
        gridAction_->setChecked(argument.isEmpty() ? !gridAction_->isChecked() : argument == "ON");
        views_->setGridVisible(gridAction_->isChecked());
        return;
    }
    if (verb == "SNAP" || verb == "OSNAP") {
        snapAction_->setChecked(argument.isEmpty() ? !snapAction_->isChecked() : argument == "ON");
        views_->setSnapEnabled(snapAction_->isChecked());
        return;
    }
    if (verb == "QUIT" || verb == "EXIT") {
        close();
        return;
    }
    // A bare drawing verb starts the interactive tool, as in any CAD package.
    if (words.size() == 1) {
        static const std::map<QString, Tool> tools = {
            {"POINT", Tool::Point},   {"PO", Tool::Point},      {"LINE", Tool::Line},
            {"L", Tool::Line},        {"PLINE", Tool::Polyline}, {"PL", Tool::Polyline},
            {"RECT", Tool::Rectangle}, {"REC", Tool::Rectangle}, {"CIRCLE", Tool::Circle},
            {"C", Tool::Circle},      {"ARC", Tool::Arc},        {"A", Tool::Arc},
            {"MOVE", Tool::Move},     {"M", Tool::Move},         {"COPY", Tool::Copy},
            {"CO", Tool::Copy},
        };
        if (const auto tool = tools.find(verb); tool != tools.end()) {
            views_->setTool(tool->second);
            views_->setFocus();
            return;
        }
    }

    const bool replacesDocument = verb == "NEW" || verb == "OPEN";
    if (replacesDocument && !confirmDiscard()) {
        return;
    }
    const auto reply = interpreter_.run(line.toStdString());
    if (!reply) {
        logMessage(QString::fromStdString(reply.error().describe()), true);
        return;
    }
    logMessage(QString::fromStdString(*reply));
    if (replacesDocument) {
        views_->zoomExtents();
    }
    historyCursor_ = static_cast<int>(interpreter_.history().size());
}

bool MainWindow::eventFilter(QObject* watched, QEvent* event)
{
    if (watched == commandInput_ && event->type() == QEvent::KeyPress) {
        const auto* key = static_cast<QKeyEvent*>(event);
        const auto& history = interpreter_.history();
        const int size = static_cast<int>(history.size());
        if (key->key() == Qt::Key_Up && size > 0) {
            historyCursor_ = std::max(0, std::min(historyCursor_, size) - 1);
            commandInput_->setText(QString::fromStdString(history[static_cast<std::size_t>(historyCursor_)]));
            return true;
        }
        if (key->key() == Qt::Key_Down && size > 0) {
            historyCursor_ = std::min(size, historyCursor_ + 1);
            commandInput_->setText(historyCursor_ < size
                                       ? QString::fromStdString(history[static_cast<std::size_t>(historyCursor_)])
                                       : QString());
            return true;
        }
        if (key->key() == Qt::Key_Escape) {
            commandInput_->clear();
            views_->cancel();
            return true;
        }
    }
    return QMainWindow::eventFilter(watched, event);
}

// ---- layers ---------------------------------------------------------------------------------------

void MainWindow::addLayer()
{
    bool accepted = false;
    const QString name = QInputDialog::getText(
        this, "New Layer", "Layer name (use / to nest, e.g. design/surface/tin1):",
        QLineEdit::Normal, {}, &accepted);
    if (!accepted || name.trimmed().isEmpty()) {
        return;
    }
    Layer layer;
    layer.name = name.trimmed().toStdString();
    const auto status = document_.execute(cmd::createLayer(std::move(layer)));
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
    }
}

void MainWindow::addChildLayer()
{
    const std::string parent = selectedLayerPath();
    if (parent.empty()) {
        logMessage("Select the layer to nest under first.", true);
        return;
    }
    bool accepted = false;
    const QString leaf = QInputDialog::getText(
        this, "New Nested Layer",
        QString("New layer under '%1':").arg(QString::fromStdString(parent)), QLineEdit::Normal,
        {}, &accepted);
    if (!accepted || leaf.trimmed().isEmpty()) {
        return;
    }
    Layer layer;
    layer.name = katana::entity::joinLayerPath(parent, leaf.trimmed().toStdString());
    const auto status = document_.execute(cmd::createLayer(std::move(layer)));
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
    }
}

void MainWindow::renameSelectedLayer()
{
    const std::string from = selectedLayerPath();
    if (from.empty()) {
        logMessage("Select a layer to rename.", true);
        return;
    }
    bool accepted = false;
    const QString to = QInputDialog::getText(this, "Rename Layer",
                                             "New full path (use / to nest):", QLineEdit::Normal,
                                             QString::fromStdString(from), &accepted);
    if (!accepted || to.trimmed().isEmpty()) {
        return;
    }
    const auto status = document_.execute(cmd::renameLayer(from, to.trimmed().toStdString()));
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
        return;
    }
    logMessage(QString("Renamed '%1' to '%2'.")
                   .arg(QString::fromStdString(from))
                   .arg(to.trimmed()));
}

void MainWindow::deleteCurrentLayer()
{
    const std::string name = selectedLayerPath();
    if (name.empty()) {
        return;
    }
    const auto& layers = document_.model().layers;
    const bool branch = layers.hasChildren(name);
    if (branch) {
        // Deleting a branch takes everything under it. That is a much larger
        // action than deleting one layer, so it is confirmed and says how much.
        const auto under = layers.subtree(name);
        const auto answer = QMessageBox::question(
            this, "Delete Layer Tree",
            QString("'%1' contains %2 nested layers. Delete all of them?")
                .arg(QString::fromStdString(name))
                .arg(under.size() - 1),
            QMessageBox::Yes | QMessageBox::No, QMessageBox::No);
        if (answer != QMessageBox::Yes) {
            return;
        }
    }
    const auto status = document_.execute(branch ? cmd::deleteLayerTree(name)
                                                 : cmd::deleteLayer(name));
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
    }
}

void MainWindow::onLayerItemChanged(QTreeWidgetItem* item, int column)
{
    if (refreshingLayers_ || item == nullptr || (column != kVisible && column != kLocked)) {
        return;
    }
    const std::string name = item->data(kName, kLayerPathRole).toString().toStdString();
    const Layer* current = document_.model().layers.find(name);
    if (current == nullptr) {
        return;
    }
    Layer changed = *current;
    const bool checked = item->checkState(column) == Qt::Checked;
    if (column == kVisible) {
        changed.visible = checked;
    } else {
        changed.locked = checked;
        if (checked && name == document_.currentLayer()) {
            logMessage("The current layer cannot be locked.", true);
            scheduleRefresh(); // puts the box back; never rebuild inside our own signal
            return;
        }
    }
    const auto status = document_.execute(cmd::updateLayer(std::move(changed)));
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
        scheduleRefresh();
        return;
    }
    // On success the document's listener has already scheduled the rebuild,
    // which is what keeps the panel honest about visibility and lock
    // inheriting down the tree.
}

void MainWindow::onLayerItemDoubleClicked(QTreeWidgetItem* item, int column)
{
    if (item == nullptr) {
        return;
    }
    const std::string name = item->data(kName, kLayerPathRole).toString().toStdString();
    if (column == kColor) {
        const Layer* current = document_.model().layers.find(name);
        if (current == nullptr) {
            return;
        }
        const QColor picked = QColorDialog::getColor(
            QColor(current->color.r, current->color.g, current->color.b), this, "Layer Colour");
        if (!picked.isValid()) {
            return;
        }
        Layer changed = *current;
        changed.color = katana::entity::Color{static_cast<std::uint8_t>(picked.red()),
                                              static_cast<std::uint8_t>(picked.green()),
                                              static_cast<std::uint8_t>(picked.blue()), 255};
        const auto status = document_.execute(cmd::updateLayer(std::move(changed)));
        if (!status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
        }
        return;
    }
    if (column == kName) {
        const auto status = document_.setCurrentLayer(name);
        if (!status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
        }
    }
}

// ---- reference data and interoperability (PLAN.MD Phase 20) ------------------------------

namespace {

QString patternsFor(const std::vector<std::string>& extensions)
{
    QStringList patterns;
    for (const std::string& extension : extensions) {
        patterns << ("*." + QString::fromStdString(extension));
    }
    return patterns.join(' ');
}

QString importFilter()
{
    const QString vector = patternsFor(interop::vectorExtensions());
    const QString raster = patternsFor(interop::rasterExtensions());
    const QString cloud = patternsFor(interop::pointCloudExtensions());
    const QString archive = patternsFor(interop::archive12dExtensions());
    return "All supported (" + vector + ' ' + archive + ' ' + raster + ' ' + cloud + ");;" +
           "Vector (" + vector + ");;" + "12d Archive (" + archive + ");;" + "Raster (" + raster +
           ");;" + "Point cloud (" + cloud + ");;" + "All files (*)";
}

QString grouped(std::uint64_t value)
{
    return QLocale().toString(static_cast<qulonglong>(value));
}

enum ReferenceColumn { kRefName = 0, kRefType, kRefDetail, kRefDisplay, kRefColumns };

} // namespace

void MainWindow::importPath(const QString& path)
{
    const std::filesystem::path file = toPath(path);
    switch (interop::kindForPath(file)) {
    case interop::SourceKind::Vector:
        importVectorFile(file);
        return;
    case interop::SourceKind::Raster:
        importRasterFile(file);
        return;
    case interop::SourceKind::PointCloud:
        importPointCloudFile(file);
        return;
    case interop::SourceKind::Archive12d:
        importArchive12dFile(file);
        return;
    case interop::SourceKind::Unknown:
        break;
    }
    logMessage("No importer for " + path, true);
}

// Loading a 12d customisation: the linestyle library, the symbol library and
// the mapfile. Several files at once, because they are useless apart - and
// WHICH is which is decided by looking inside each one, since `.4d` is the
// extension of both a style library and a mapfile (PLAN.MD 20.3).
void MainWindow::loadCustomisation()
{
    const QStringList chosen = QFileDialog::getOpenFileNames(
        this, "Load 12d Customisation", QString(),
        "12d customisation (*.4d *.mapfile);;All files (*)");
    if (chosen.isEmpty()) {
        logMessage("Loading a customisation was cancelled.");
        return;
    }
    std::vector<std::filesystem::path> paths;
    paths.reserve(static_cast<std::size_t>(chosen.size()));
    for (const QString& one : chosen) {
        paths.emplace_back(one.toStdString());
    }
    applyCustomisation(paths);
}

// Applying the loaded mapfile to the drawing. One undoable step, and a
// report - including which property the codes were read from, because "no
// entity carries a code" and "they carry it under another name" are
// different problems and look identical from the outside (PLAN.MD 20.3).
void MainWindow::applySurveyCodes()
{
    if (document_.surveyMap().empty()) {
        warnUser("No mapfile",
                 "Load a 12d customisation first: File > Load 12d Customisation...");
        return;
    }
    katana::cad::SurveyCodingOptions options;
    options.colourOf = [](std::string_view name) {
        return katana::archive12d::standardColour(name);
    };
    if (!document_.selection().empty()) {
        options.ids = document_.selection().ids();
        logMessage("Applying survey codes to the " + grouped(options.ids.size()) + " selected.");
    }
    katana::cad::SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(document_, options, &report);
    if (!command) {
        warnUser("Survey codes failed", QString::fromStdString(command.error().describe()));
        return;
    }
    logMessage(grouped(report.coded) + " entities carry a \"" +
               QString::fromStdString(report.property) + "\", " + grouped(report.matched) +
               " of them codes the mapfile has a rule for.");
    if (!report.unmatchedCodes.empty()) {
        logMessage(grouped(report.unmatchedCodes.size()) + " codes have no rule, starting with \"" +
                   QString::fromStdString(report.unmatchedCodes.front()) + "\"");
    }
    if (!report.missingDefinitions.empty()) {
        logMessage(grouped(report.missingDefinitions.size()) +
                   " linestyles or symbols named but not in the loaded library");
    }
    if (*command == nullptr) {
        logMessage("Nothing to change.");
        return;
    }
    if (const auto status = document_.execute(std::move(*command)); !status) {
        warnUser("Survey codes failed", QString::fromStdString(status.error().describe()));
        return;
    }
    logMessage("Applied: " + grouped(report.layersCreated.size()) + " layers and " +
               grouped(report.stylesCreated.size()) + " styles created. Undo puts it all back.");
}

void MainWindow::applyCustomisation(const std::vector<std::filesystem::path>& paths)
{
    auto loaded = katana::archive12d::readCustomisation(paths);
    if (!loaded) {
        warnUser("Customisation failed", QString::fromStdString(loaded.error().describe()));
        return;
    }
    for (const katana::archive12d::LoadedFile& file : loaded->files) {
        logMessage(fromPath(file.path.filename()) + ": " +
                   QString::fromStdString(katana::archive12d::toString(file.kind)) + ", " +
                   grouped(file.read) +
                   (file.kind == katana::archive12d::CustomisationFile::MapFile ? " rules"
                                                                               : " definitions"));
    }
    for (const std::string& warning : loaded->warnings) {
        logMessage("Warning: " + QString::fromStdString(warning));
    }
    // A customisation need not be self-contained. Naming what is missing is
    // the difference between a symbol that is plainly absent and one that is
    // silently drawn as a plain mark.
    const std::vector<std::string> missing = loaded->unresolvedStyles();
    if (!missing.empty()) {
        logMessage(grouped(missing.size()) +
                   " names the mapfile asks for are in no loaded library, starting with \"" +
                   QString::fromStdString(missing.front()) + "\"");
    }
    logMessage("Customisation: " + grouped(loaded->library.size()) + " definitions (" +
               grouped(katana::entity::vertexStyleNames(loaded->library).size()) +
               " symbols) and " + grouped(loaded->map.size()) + " survey code rules.");
    document_.setStyleLibrary(std::move(loaded->library));
    document_.setSurveyMap(std::move(loaded->map));
    reportCustomisationCoverage();
}

// What the loaded customisation means for THIS drawing. Without it, "the
// linestyles are not showing" is indistinguishable from "this drawing's
// styles are 12d's plain lines" and from "nothing is loaded at all".
void MainWindow::reportCustomisationCoverage()
{
    const katana::cad::CustomisationCoverage coverage =
        katana::cad::customisationCoverage(document_);
    if (coverage.styles == 0) {
        logMessage("This drawing has no styles yet; import a 12d archive to see the "
                   "customisation take effect.");
        return;
    }
    logMessage(grouped(coverage.resolved) + " of this drawing's " + grouped(coverage.styles) +
               " styles are drawn with a loaded definition (" + grouped(coverage.named) +
               " name one; the rest are 12d's plain lines).");
    if (!coverage.unresolved.empty()) {
        logMessage(grouped(coverage.unresolved.size()) +
                   " names are in no loaded library, starting with \"" +
                   QString::fromStdString(coverage.unresolved.front()) + "\"");
    }
}

void MainWindow::importFile()
{
    const QString selected =
        QFileDialog::getOpenFileName(this, "Import", QString(), importFilter());
    if (selected.isEmpty()) {
        return;
    }
    const std::filesystem::path path = toPath(selected);
    switch (interop::kindForPath(path)) {
    case interop::SourceKind::Vector:
        importVectorFile(path);
        return;
    case interop::SourceKind::Raster:
        importRasterFile(path);
        return;
    case interop::SourceKind::PointCloud:
        importPointCloudFile(path);
        return;
    case interop::SourceKind::Archive12d:
        importArchive12dFile(path);
        return;
    case interop::SourceKind::Unknown:
        break;
    }
    warnUser( "Import",
                         "Katana does not recognise the extension of\n" + selected +
                             "\n\nSupported: " + patternsFor(interop::vectorExtensions()) + ' ' +
                             patternsFor(interop::archive12dExtensions()) + ' ' +
                             patternsFor(interop::rasterExtensions()) + ' ' +
                             patternsFor(interop::pointCloudExtensions()));
}

void MainWindow::importVectorFile(const std::filesystem::path& path)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto imported = interop::importVector(path);
    QApplication::restoreOverrideCursor();

    if (!imported.ok()) {
        logMessage(QString::fromStdString(imported.error().describe()), true);
        warnUser( "Import failed",
                             QString::fromStdString(imported.error().describe()));
        return;
    }

    // Survey data in a projected CRS carries coordinates like (255440, 7410850)
    // while a drawing started from scratch sits near the origin. Merging them
    // succeeds and leaves the existing drawing a dot smaller than a pixel, so
    // the choice is put to the user BEFORE anything is added rather than left
    // to be discovered by zooming to extents.
    const auto advice =
        interop::advisePlacement(document_.model().entities.bounds(), imported->bounds);
    if (advice.farApart && headless_) {
        logMessage(QString::fromStdString(advice.message) + " (kept: no one to ask).", true);
    } else if (advice.farApart) {
        QMessageBox box(this);
        box.setIcon(QMessageBox::Question);
        box.setWindowTitle("Far from the current drawing");
        box.setText(QString::fromStdString(advice.message) + ".");
        box.setInformativeText(
            QString("The file covers %1,%2 to %3,%4.\n\n"
                    "Shifting moves the imported data as one piece so it sits beside the "
                    "drawing; its shape and internal dimensions are unchanged.")
                .arg(imported->bounds.min.x, 0, 'f', 2)
                .arg(imported->bounds.min.y, 0, 'f', 2)
                .arg(imported->bounds.max.x, 0, 'f', 2)
                .arg(imported->bounds.max.y, 0, 'f', 2));
        QPushButton* shift = box.addButton("Shift Alongside", QMessageBox::AcceptRole);
        QPushButton* keep = box.addButton("Keep Survey Coordinates", QMessageBox::DestructiveRole);
        QPushButton* cancel = box.addButton(QMessageBox::Cancel);
        box.setDefaultButton(shift);
        box.exec();

        if (box.clickedButton() == cancel) {
            logMessage("Import cancelled.");
            return;
        }
        if (box.clickedButton() == shift) {
            interop::VectorImportOptions options;
            options.originShift = advice.suggestedShift;
            QApplication::setOverrideCursor(Qt::WaitCursor);
            auto shifted = interop::importVector(path, options);
            QApplication::restoreOverrideCursor();
            if (!shifted.ok()) {
                logMessage(QString::fromStdString(shifted.error().describe()), true);
                return;
            }
            imported = std::move(shifted);
            logMessage(QString("Shifted the imported data by %1,%2 to sit beside the drawing.")
                           .arg(advice.suggestedShift.x, 0, 'f', 3)
                           .arg(advice.suggestedShift.y, 0, 'f', 3));
        } else {
            (void)keep;
            logMessage(QString::fromStdString(advice.message) + ".", true);
        }
    }

    // Layers must exist before the entities that reference them, and the whole
    // import has to be ONE undo step - a user who imports a shapefile by
    // mistake expects a single Ctrl+Z to remove it, not one per layer.
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
    for (const std::string& name : imported->layersNeeded) {
        if (!document_.model().layers.contains(name)) {
            Layer layer;
            layer.name = name;
            transaction->add(cmd::createLayer(layer));
        }
    }
    const std::size_t created = imported->entities.size();
    const auto importedBounds = imported->bounds;
    transaction->add(cmd::createEntities(std::move(imported->entities)));

    const auto status = document_.execute(std::move(transaction));
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
        warnUser( "Import failed",
                             QString::fromStdString(status.error().describe()));
        return;
    }

    logMessage("Imported " + grouped(created) + " entities from " + fromPath(path.filename()));
    if (!importedBounds.empty()) {
        logMessage(QString("  extent %1,%2 to %3,%4")
                       .arg(importedBounds.min.x, 0, 'f', 2)
                       .arg(importedBounds.min.y, 0, 'f', 2)
                       .arg(importedBounds.max.x, 0, 'f', 2)
                       .arg(importedBounds.max.y, 0, 'f', 2));
    }
    for (const std::string& warning : imported->warnings) {
        logMessage("  " + QString::fromStdString(warning));
    }
    if (!imported->projectionWkt.empty()) {
        logMessage("  coordinates were imported unchanged; the file declares its own CRS");
    }
    views_->zoomExtents();
}

void MainWindow::importArchive12dFile(const std::filesystem::path& path)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto imported = interop::importArchive12d(path);
    QApplication::restoreOverrideCursor();
    if (!imported.ok()) {
        logMessage(QString::fromStdString(imported.error().describe()), true);
        warnUser( "Import failed",
                             QString::fromStdString(imported.error().describe()));
        return;
    }

    // The same question a vector import asks, for the same reason: a 12da is
    // survey data at survey coordinates. Surfaces and clouds are shifted with
    // the entities - the shift is applied inside the importer, to everything.
    const auto advice =
        interop::advisePlacement(document_.model().entities.bounds(), imported->bounds);
    if (advice.farApart && headless_) {
        logMessage(QString::fromStdString(advice.message) + " (kept: no one to ask).", true);
    } else if (advice.farApart) {
        QMessageBox box(this);
        box.setIcon(QMessageBox::Question);
        box.setWindowTitle("Far from the current drawing");
        box.setText(QString::fromStdString(advice.message) + ".");
        box.setInformativeText(
            "Shifting moves everything in the file as one piece so it sits beside the drawing; "
            "its shape and internal dimensions are unchanged.");
        QPushButton* shift = box.addButton("Shift Alongside", QMessageBox::AcceptRole);
        box.addButton("Keep Survey Coordinates", QMessageBox::DestructiveRole);
        QPushButton* cancel = box.addButton(QMessageBox::Cancel);
        box.setDefaultButton(shift);
        box.exec();
        if (box.clickedButton() == cancel) {
            logMessage("Import cancelled.");
            return;
        }
        if (box.clickedButton() == shift) {
            interop::Archive12dImportOptions options;
            options.originShift = advice.suggestedShift;
            QApplication::setOverrideCursor(Qt::WaitCursor);
            auto shifted = interop::importArchive12d(path, options);
            QApplication::restoreOverrideCursor();
            if (!shifted.ok()) {
                logMessage(QString::fromStdString(shifted.error().describe()), true);
                return;
            }
            imported = std::move(shifted);
            logMessage(QString("Shifted the imported data by %1,%2 to sit beside the drawing.")
                           .arg(advice.suggestedShift.x, 0, 'f', 3)
                           .arg(advice.suggestedShift.y, 0, 'f', 3));
        } else {
            logMessage(QString::fromStdString(advice.message) + ".", true);
        }
    }

    // Layers, entities and alignments: one transaction, one Ctrl+Z.
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
    for (const Layer& layer : imported->layersNeeded) {
        if (!document_.model().layers.contains(layer.name)) {
            transaction->add(cmd::createLayer(layer));
        }
    }
    for (const katana::entity::Style& style : imported->stylesNeeded) {
        if (!document_.model().styles.contains(style.name)) {
            transaction->add(cmd::createStyle(style));
        }
    }
    const std::size_t created = imported->entities.size();
    if (created != 0) {
        transaction->add(cmd::createEntities(std::move(imported->entities)));
    }
    std::size_t alignments = 0;
    for (katana::entity::Alignment& alignment : imported->alignments) {
        // A name the drawing already has would fail the whole transaction.
        const std::string base = alignment.name;
        for (int copy = 2; document_.model().alignments.contains(alignment.name); ++copy) {
            alignment.name = base + " (" + std::to_string(copy) + ")";
        }
        transaction->add(cmd::createAlignment(alignment));
        ++alignments;
    }
    const auto status = document_.execute(std::move(transaction));
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
        warnUser( "Import failed",
                             QString::fromStdString(status.error().describe()));
        return;
    }

    QString summary = "Imported " + grouped(created) + " entities";
    if (alignments != 0) {
        summary += ", " + grouped(alignments) + " alignments";
    }
    if (!imported->surfaces.empty()) {
        summary += ", " + grouped(imported->surfaces.size()) + " surfaces";
    }
    if (!imported->meshes.empty()) {
        summary += ", " + grouped(imported->meshes.size()) + " meshes";
    }
    if (!imported->clouds.empty()) {
        summary += ", " + grouped(imported->clouds.size()) + " point clouds";
    }
    summary += " from " + fromPath(path.filename()) + " (" +
               QString::fromStdString(imported->encoding);
    if (!imported->archiveVersion.empty()) {
        summary += ", 12d archive " + QString::fromStdString(imported->archiveVersion);
    }
    logMessage(summary + ")");
    for (const auto& tally : imported->tally) {
        logMessage(QString("  %1: %2 read, %3 imported")
                       .arg(QString::fromStdString(tally.keyword))
                       .arg(grouped(tally.read))
                       .arg(grouped(tally.imported)),
                   tally.imported < tally.read);
    }
    for (const std::string& warning : imported->warnings) {
        logMessage("  " + QString::fromStdString(warning));
    }

    // Surfaces and clouds are session data, outside undo - see interop/
    // reference_data.hpp for why - and are added after the transaction so that
    // a rejected import leaves nothing behind.
    for (auto& surface : imported->surfaces) {
        addSurface(surface.name, std::move(surface.surface));
    }
    for (auto& mesh : imported->meshes) {
        // A 12d colour Katana has no RGB for leaves the mesh its default
        // clay, which is visible against a surface and against the drawing.
        constexpr katana::render::Rgba kMeshDefault = katana::render::rgba(190, 170, 140);
        const auto toRgba = [](const std::optional<katana::entity::Color>& colour,
                               katana::render::Rgba fallback) {
            return colour ? katana::render::rgba(colour->r, colour->g, colour->b) : fallback;
        };
        const katana::render::Rgba base = toRgba(mesh.color, kMeshDefault);
        std::vector<katana::render::Rgba> faces;
        faces.reserve(mesh.faceColors.size());
        for (const auto& colour : mesh.faceColors) {
            faces.push_back(toRgba(colour, base));
        }
        addMesh(mesh.name, std::move(mesh.mesh), base, std::move(faces));
    }
    // A mesh is a 3D thing: in plan it is only a footprint, so the first
    // import that brings one opens the 3D view, exactly as a surface does.
    // Once, after the loop - not once per mesh, and a real archive brings
    // 1 453 of them.
    if (!imported->meshes.empty() && views_->activeRenderView() == nullptr) {
        views_->setLayoutKind(cad::LayoutKind::SplitVertical);
        views_->layout().setActiveIndex(1);
        views_->setActiveViewKind(cad::ViewKind::Model3D);
        refreshViewMenu();
    }
    if (!imported->clouds.empty()) {
        for (auto& cloud : imported->clouds) {
            reference_.add(std::move(cloud));
        }
        views_->invalidateReferenceCache();
        refreshReferences();
    }
    views_->refreshAll();
    views_->zoomExtents();
}

void MainWindow::importRasterFile(const std::filesystem::path& path)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto raster = interop::importRaster(path);
    QApplication::restoreOverrideCursor();

    if (!raster.ok()) {
        logMessage(QString::fromStdString(raster.error().describe()), true);
        warnUser( "Import failed",
                             QString::fromStdString(raster.error().describe()));
        return;
    }

    const bool georeferenced = raster->hasGeotransform;
    const int pixelWidth = raster->width;
    const int pixelHeight = raster->height;
    reference_.add(std::move(*raster));
    views_->invalidateReferenceCache();
    refreshReferences();

    logMessage("Imported raster " + fromPath(path.filename()) + " (" +
               QString::number(pixelWidth) + " x " + QString::number(pixelHeight) + " px)");
    if (!georeferenced) {
        // Placing it at the origin is a guess, and the user has to know that.
        logMessage("  this file carries no georeferencing; it is placed at the origin at "
                   "one model unit per pixel",
                   true);
    }
    views_->zoomExtents();
}

void MainWindow::importPointCloudFile(const std::filesystem::path& path)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto cloud = interop::importPointCloud(path);
    QApplication::restoreOverrideCursor();

    if (!cloud.ok()) {
        logMessage(QString::fromStdString(cloud.error().describe()), true);
        warnUser( "Import failed",
                             QString::fromStdString(cloud.error().describe()));
        return;
    }

    const std::size_t shown = cloud->points.size();
    const std::uint64_t total = cloud->sourcePointCount;
    const bool decimated = cloud->isDecimated();
    reference_.add(std::move(*cloud));
    views_->invalidateReferenceCache();
    refreshReferences();

    QString message = "Imported point cloud " + fromPath(path.filename()) + " (" +
                      grouped(shown) + " points";
    if (decimated) {
        message += " sampled from " + grouped(total);
    }
    logMessage(message + ")");
    views_->zoomExtents();
}

void MainWindow::exportVectorFile()
{
    if (document_.model().entities.empty() && document_.model().alignments.empty() &&
        sceneSurfaces_.empty()) {
        QMessageBox::information(this, "Export", "The drawing is empty.");
        return;
    }

    QStringList filters;
    for (const interop::FormatChoice& format : interop::vectorExportFormats()) {
        filters << (QString::fromStdString(format.description) + " (*." +
                    QString::fromStdString(format.extension) + ")");
    }
    filters << "12d Archive (*.12da)" << "12d Archive, zipped (*.12daz)";
    QString chosenFilter;
    const QString selected = QFileDialog::getSaveFileName(this, "Export", QString(),
                                                          filters.join(";;"), &chosenFilter);
    if (selected.isEmpty()) {
        return;
    }

    interop::VectorExportOptions options;
    if (!document_.selection().empty()) {
        const auto answer = QMessageBox::question(
            this, "Export",
            "Export only the " + QString::number(document_.selection().size()) +
                " selected entities?\n\nNo exports the whole drawing.",
            QMessageBox::Yes | QMessageBox::No | QMessageBox::Cancel);
        if (answer == QMessageBox::Cancel) {
            return;
        }
        if (answer == QMessageBox::Yes) {
            options.entities = document_.selection().ids();
        }
    }

    const std::filesystem::path path = toPath(selected);
    if (interop::kindForPath(path) == interop::SourceKind::Archive12d) {
        // A 12d archive carries what the other formats cannot: the alignments
        // and the surfaces of the session go with the drawing.
        std::vector<katana::archive12d::ExportSurface> surfaces;
        if (options.entities.empty()) {
            for (const auto& item : sceneSurfaces_) {
                surfaces.push_back({item.name, item.surface});
            }
        }
        interop::Archive12dExportOptions archiveOptions;
        archiveOptions.entities = options.entities;
        QApplication::setOverrideCursor(Qt::WaitCursor);
        auto archive = interop::exportArchive12d(document_.model(), surfaces, path, archiveOptions);
        QApplication::restoreOverrideCursor();
        if (!archive.ok()) {
            logMessage(QString::fromStdString(archive.error().describe()), true);
            warnUser( "Export failed",
                                 QString::fromStdString(archive.error().describe()));
            return;
        }
        logMessage(QString("Exported %1 entities, %2 alignments and %3 surfaces to %4 (12d archive)")
                       .arg(grouped(archive->entitiesWritten))
                       .arg(grouped(archive->alignmentsWritten))
                       .arg(grouped(archive->surfacesWritten))
                       .arg(fromPath(path.filename())));
        for (const std::string& warning : archive->warnings) {
            logMessage("  " + QString::fromStdString(warning));
        }
        return;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto result = interop::exportVector(document_.model(), path, options);
    QApplication::restoreOverrideCursor();

    if (!result.ok()) {
        logMessage(QString::fromStdString(result.error().describe()), true);
        warnUser( "Export failed",
                             QString::fromStdString(result.error().describe()));
        return;
    }

    logMessage("Exported " + grouped(result->featuresWritten) + " features to " +
               fromPath(path.filename()) + " (" + QString::fromStdString(result->driver) + ")");
    for (const std::string& warning : result->warnings) {
        logMessage("  " + QString::fromStdString(warning));
    }
}

void MainWindow::refreshReferences()
{
    if (referenceTable_ == nullptr) {
        return;
    }
    refreshingReferences_ = true;
    referenceTable_->setRowCount(0);

    const auto addRow = [this](katana::interop::ReferenceId id, const QString& name, bool visible,
                               const QString& type, const QString& detail) {
        const int row = referenceTable_->rowCount();
        referenceTable_->insertRow(row);

        auto* nameItem = new QTableWidgetItem(name);
        nameItem->setFlags(nameItem->flags() | Qt::ItemIsUserCheckable);
        nameItem->setCheckState(visible ? Qt::Checked : Qt::Unchecked);
        nameItem->setData(Qt::UserRole, QVariant::fromValue<qulonglong>(id));
        referenceTable_->setItem(row, kRefName, nameItem);

        auto* typeItem = new QTableWidgetItem(type);
        typeItem->setFlags(typeItem->flags() & ~Qt::ItemIsEditable);
        referenceTable_->setItem(row, kRefType, typeItem);

        auto* detailItem = new QTableWidgetItem(detail);
        detailItem->setFlags(detailItem->flags() & ~Qt::ItemIsEditable);
        referenceTable_->setItem(row, kRefDetail, detailItem);
        return row;
    };

    for (const katana::interop::RasterOverlay& raster : reference_.rasters()) {
        QString detail = QString::number(raster.width) + " x " + QString::number(raster.height) +
                         " px";
        if (!raster.hasGeotransform) {
            detail += "  (not georeferenced)";
        }
        const int row = addRow(raster.id, QString::fromStdString(raster.name), raster.visible,
                               "Raster", detail);

        auto* opacity = new QComboBox(referenceTable_);
        opacity->addItems({"100%", "75%", "50%", "25%"});
        const int index = raster.opacity > 0.875   ? 0
                          : raster.opacity > 0.625 ? 1
                          : raster.opacity > 0.375 ? 2
                                                   : 3;
        opacity->setCurrentIndex(index);
        const katana::interop::ReferenceId id = raster.id;
        connect(opacity, &QComboBox::currentIndexChanged, this, [this, id](int chosen) {
            if (auto* target = reference_.findRaster(id)) {
                static constexpr double kValues[] = {1.0, 0.75, 0.5, 0.25};
                target->opacity = kValues[std::clamp(chosen, 0, 3)];
                views_->update();
            }
        });
        referenceTable_->setCellWidget(row, kRefDisplay, opacity);
    }

    for (const katana::interop::PointCloudLayer& cloud : reference_.pointClouds()) {
        QString detail = grouped(cloud.points.size()) + " pts";
        if (cloud.isDecimated()) {
            detail += " of " + grouped(cloud.sourcePointCount);
        }
        const int row = addRow(cloud.id, QString::fromStdString(cloud.name), cloud.visible,
                               "Point cloud", detail);

        auto* mode = new QComboBox(referenceTable_);
        using Mode = katana::interop::PointColorMode;
        for (const Mode value : {Mode::Elevation, Mode::Intensity, Mode::Classification,
                                 Mode::SourceColor, Mode::Flat}) {
            mode->addItem(katana::interop::toString(value), static_cast<int>(value));
        }
        mode->setCurrentIndex(static_cast<int>(cloud.colorMode));
        const katana::interop::ReferenceId id = cloud.id;
        connect(mode, &QComboBox::currentIndexChanged, this, [this, id](int chosen) {
            if (auto* target = reference_.findPointCloud(id)) {
                target->colorMode = static_cast<katana::interop::PointColorMode>(chosen);
                views_->invalidateReferenceCache();
            }
        });
        referenceTable_->setCellWidget(row, kRefDisplay, mode);
    }

    referenceTable_->resizeColumnsToContents();
    refreshingReferences_ = false;
}

void MainWindow::onReferenceCellChanged(int row, int column)
{
    if (refreshingReferences_ || column != kRefName) {
        return;
    }
    const QTableWidgetItem* item = referenceTable_->item(row, kRefName);
    if (item == nullptr) {
        return;
    }
    const auto id = static_cast<katana::interop::ReferenceId>(item->data(Qt::UserRole).toULongLong());
    const bool visible = item->checkState() == Qt::Checked;
    if (auto* raster = reference_.findRaster(id)) {
        raster->visible = visible;
    } else if (auto* cloud = reference_.findPointCloud(id)) {
        cloud->visible = visible;
    }
    views_->update();
}

void MainWindow::removeSelectedReference()
{
    const int row = referenceTable_ != nullptr ? referenceTable_->currentRow() : -1;
    if (row < 0) {
        return;
    }
    const QTableWidgetItem* item = referenceTable_->item(row, kRefName);
    if (item == nullptr) {
        return;
    }
    const auto id = static_cast<katana::interop::ReferenceId>(item->data(Qt::UserRole).toULongLong());
    if (reference_.remove(id)) {
        logMessage("Removed reference layer " + item->text());
        views_->invalidateReferenceCache();
        refreshReferences();
    }
}

void MainWindow::zoomToSelectedReference()
{
    const int row = referenceTable_ != nullptr ? referenceTable_->currentRow() : -1;
    if (row < 0) {
        return;
    }
    const QTableWidgetItem* item = referenceTable_->item(row, kRefName);
    if (item == nullptr) {
        return;
    }
    const auto id = static_cast<katana::interop::ReferenceId>(item->data(Qt::UserRole).toULongLong());
    katana::geometry::Box2 bounds;
    if (const auto* raster = reference_.findRaster(id)) {
        bounds = raster->worldBounds();
    } else if (const auto* cloud = reference_.findPointCloud(id)) {
        bounds = cloud->worldBounds();
    }
    if (bounds.empty()) {
        logMessage("That layer has no extent to zoom to.", true);
        return;
    }
    views_->zoomTo(bounds);
}


// ---- view layout, 3D and sections (PLAN.MD Phases 08, 14, 15, 21) ------------------------

namespace {

// Thinning a point cloud before triangulating it is not an optimisation, it is
// the difference between a surface and a hang: CGAL's constrained Delaunay is
// well behaved but two million points still costs minutes and gigabytes, and a
// ground surface does not carry two million points of information. The cap is
// generous enough that a normal survey is triangulated whole.
constexpr std::size_t kMaximumTinPoints = 400'000;

[[nodiscard]] std::uint32_t thinningFor(std::size_t available)
{
    if (available <= kMaximumTinPoints) {
        return 1;
    }
    // Rounded UP, so the result is at or under the cap rather than just over.
    return static_cast<std::uint32_t>((available + kMaximumTinPoints - 1) / kMaximumTinPoints);
}

} // namespace

void MainWindow::buildViewMenu(QMenu* viewMenu)
{
    viewMenu->addSeparator();

    QMenu* layoutMenu = viewMenu->addMenu("Viewport &Layout");
    for (const cad::LayoutKind kind :
         {cad::LayoutKind::Single, cad::LayoutKind::SplitVertical,
          cad::LayoutKind::SplitHorizontal, cad::LayoutKind::ThreeLeft, cad::LayoutKind::ThreeTop,
          cad::LayoutKind::Quad}) {
        QAction* action = layoutMenu->addAction(cad::toString(kind));
        action->setCheckable(true);
        action->setData(static_cast<int>(kind));
        connect(action, &QAction::triggered, this, [this, kind] {
            views_->setLayoutKind(kind);
            refreshViewMenu();
        });
        layoutActions_.push_back(action);
    }

    QMenu* kindMenu = viewMenu->addMenu("Active Viewport &Shows");
    for (const cad::ViewKind kind : {cad::ViewKind::Plan, cad::ViewKind::Model3D,
                                     cad::ViewKind::Section, cad::ViewKind::Elevation}) {
        QAction* action = kindMenu->addAction(cad::toString(kind));
        action->setCheckable(true);
        action->setData(static_cast<int>(kind));
        connect(action, &QAction::triggered, this, [this, kind] {
            views_->setActiveViewKind(kind);
            refreshViewMenu();
        });
        kindActions_.push_back(action);
    }

    QMenu* standard = viewMenu->addMenu("Standard &3D Views");
    for (const render::StandardView view :
         {render::StandardView::Top, render::StandardView::Bottom, render::StandardView::Front,
          render::StandardView::Back, render::StandardView::Left, render::StandardView::Right,
          render::StandardView::IsoSouthWest, render::StandardView::IsoSouthEast,
          render::StandardView::IsoNorthEast, render::StandardView::IsoNorthWest}) {
        standard->addAction(render::toString(view), this, [this, view] {
            if (RenderViewWidget* renderView = views_->activeRenderView()) {
                renderView->setStandardView(view);
            } else {
                logMessage("No 3D viewport is open. Use View > Viewport Layout.", true);
            }
        });
    }
    viewMenu->addAction("Toggle &Perspective", QKeySequence(Qt::Key_F9), this, [this] {
        RenderViewWidget* renderView = views_->activeRenderView();
        if (renderView == nullptr) {
            logMessage("No 3D viewport is open.", true);
            return;
        }
        const bool wasPerspective =
            renderView->cell()->camera.projection() == render::Projection::Perspective;
        renderView->setProjection(wasPerspective ? render::Projection::Orthographic
                                                 : render::Projection::Perspective);
        logMessage(wasPerspective ? "Orthographic projection." : "Perspective projection.");
    });
    viewMenu->addAction("&Vertical Exaggeration...", this,
                        [this] { setVerticalExaggeration(); });
}

void MainWindow::refreshViewMenu()
{
    for (QAction* action : layoutActions_) {
        action->setChecked(action->data().toInt() == static_cast<int>(views_->layoutKind()));
    }
    for (QAction* action : kindActions_) {
        action->setChecked(action->data().toInt() == static_cast<int>(views_->activeViewKind()));
    }
}

void MainWindow::setVerticalExaggeration()
{
    bool accepted = false;
    const double current = views_->sceneOptions().verticalExaggeration;
    const double factor = QInputDialog::getDouble(this, "Vertical Exaggeration",
                                                  "Multiply elevations by:", current, 0.01, 1000.0,
                                                  2, &accepted);
    if (!accepted) {
        return;
    }
    cad::SceneOptions options = views_->sceneOptions();
    options.verticalExaggeration = factor;
    // About the middle of the data, so exaggerating does not also launch the
    // model off the top of the view.
    cad::SceneOptions flat = options;
    flat.verticalExaggeration = 1.0;
    const auto box = cad::sceneBounds(document_, sceneSurfaces_, flat);
    options.exaggerationDatum = box.empty() ? 0.0 : box.center().z;
    views_->setSceneOptions(options);

    if (SectionViewWidget* section = views_->activeSectionView()) {
        section->setVerticalExaggeration(factor);
    }
    if (RenderViewWidget* renderView = views_->activeRenderView()) {
        renderView->zoomExtents();
    }
    logMessage(QString("Vertical exaggeration x%1.").arg(factor, 0, 'f', 2));
}

void MainWindow::addMesh(std::string name, katana::geometry::TriangleMesh mesh,
                         katana::render::Rgba color,
                         std::vector<katana::render::Rgba> faceColors)
{
    meshStore_.push_back(std::make_unique<katana::geometry::TriangleMesh>(std::move(mesh)));

    cad::SceneMesh item;
    item.name = std::move(name);
    item.mesh = meshStore_.back().get();
    item.flatColor = color;
    item.faceColors = std::move(faceColors);
    sceneMeshes_.push_back(std::move(item));

    // As for surfaces: the views hold a pointer to the VECTOR, and the
    // meshes themselves are behind unique_ptr so this push_back cannot move
    // them out from under it.
    views_->setMeshes(&sceneMeshes_);
}

void MainWindow::addSurface(std::string name, katana::terrain::TinSurface surface)
{
    surfaceStore_.push_back(
        std::make_unique<katana::terrain::TinSurface>(std::move(surface)));

    cad::SceneSurface item;
    item.name = std::move(name);
    item.surface = surfaceStore_.back().get();
    sceneSurfaces_.push_back(item);

    // The views hold a pointer to the VECTOR, not to its elements, so this is a
    // refresh rather than a repair - and the surfaces themselves are behind
    // unique_ptr precisely so that this push_back cannot move them.
    views_->setSurfaces(&sceneSurfaces_);
    views_->refreshAll();

    logMessage(QString("Surface '%1': %2 vertices, %3 triangles, elevation %4 to %5.")
                   .arg(QString::fromStdString(sceneSurfaces_.back().name))
                   .arg(item.surface->vertexCount())
                   .arg(item.surface->triangleCount())
                   .arg(item.surface->minElevation(), 0, 'f', 3)
                   .arg(item.surface->maxElevation(), 0, 'f', 3));

    // Show it. A surface the user cannot see is not obviously a success.
    if (views_->activeRenderView() == nullptr) {
        views_->setLayoutKind(cad::LayoutKind::SplitVertical);
        views_->layout().setActiveIndex(1);
        views_->setActiveViewKind(cad::ViewKind::Model3D);
        refreshViewMenu();
    }
    if (RenderViewWidget* renderView = views_->activeRenderView()) {
        renderView->invalidateScene();
        renderView->zoomExtents();
    }
}

void MainWindow::buildSurfaceFromPointCloud()
{
    if (reference_.pointClouds().empty()) {
        logMessage("No point cloud is loaded. Use File > Import first.", true);
        return;
    }
    QStringList names;
    for (const auto& cloud : reference_.pointClouds()) {
        names << QString::fromStdString(cloud.name);
    }
    bool accepted = false;
    const QString chosen = QInputDialog::getItem(this, "Surface From Point Cloud",
                                                 "Point cloud:", names, 0, false, &accepted);
    if (!accepted) {
        return;
    }
    const auto it = std::find_if(reference_.pointClouds().begin(), reference_.pointClouds().end(),
                                 [&chosen](const katana::interop::PointCloudLayer& cloud) {
                                     return QString::fromStdString(cloud.name) == chosen;
                                 });
    if (it == reference_.pointClouds().end()) {
        return;
    }

    katana::terrain::TinInput input;
    const std::uint32_t step = thinningFor(it->points.size());
    input.points.reserve(it->points.size() / step + 1);
    for (std::size_t i = 0; i < it->points.size(); i += step) {
        const auto& point = it->points[i];
        input.points.push_back(katana::geometry::Point3(point.x, point.y, point.z));
    }
    if (step > 1) {
        logMessage(QString("Thinning %1 points to every %2 for triangulation.")
                       .arg(it->points.size())
                       .arg(step));
    }

    katana::terrain::TinBuildOptions options;
    // Scanned points land on the same ground mark constantly and their
    // elevations differ by millimetres. Failing on that would make a surface
    // from a real scan impossible, so the mean is taken and reported.
    options.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto built = katana::terrain::buildTin(input, options);
    QApplication::restoreOverrideCursor();
    if (!built) {
        logMessage(QString::fromStdString(built.error().describe()), true);
        return;
    }
    if (built->report.duplicatePointCount > 0) {
        logMessage(QString("%1 coincident points merged by averaging their elevations.")
                       .arg(built->report.duplicatePointCount));
    }
    addSurface(chosen.toStdString(), std::move(built->surface));
}

void MainWindow::buildSurfaceFromRaster()
{
    if (reference_.rasters().empty()) {
        logMessage("No raster is loaded. Use File > Import first.", true);
        return;
    }
    QStringList names;
    for (const auto& raster : reference_.rasters()) {
        names << QString::fromStdString(raster.name);
    }
    bool accepted = false;
    const QString chosen = QInputDialog::getItem(this, "Surface From Raster",
                                                 "Elevation raster:", names, 0, false, &accepted);
    if (!accepted) {
        return;
    }
    const auto it = std::find_if(reference_.rasters().begin(), reference_.rasters().end(),
                                 [&chosen](const katana::interop::RasterOverlay& raster) {
                                     return QString::fromStdString(raster.name) == chosen;
                                 });
    if (it == reference_.rasters().end()) {
        return;
    }
    if (!it->hasGeotransform) {
        logMessage("That raster has no geotransform, so its pixels have no ground position.",
                   true);
        return;
    }

    // IMPORTANT, and worth being explicit about: RasterOverlay holds the
    // DISPLAY rgba, which for a DEM is a grey ramp stretched over the band's
    // range - not the elevations themselves. Reconstructing heights from it
    // would be inventing data. Elevation is therefore taken as the grey level
    // scaled between a minimum and a maximum the USER states, which is honest
    // about being an approximation. The real fix - reading the band as doubles
    // through the GDAL adapter - is recorded in PLAN.MD Phase 20.
    bool lowAccepted = false;
    const double low = QInputDialog::getDouble(this, "Surface From Raster",
                                               "Elevation of the darkest pixel:", 0.0, -1e6, 1e6,
                                               3, &lowAccepted);
    if (!lowAccepted) {
        return;
    }
    bool highAccepted = false;
    const double high = QInputDialog::getDouble(this, "Surface From Raster",
                                                "Elevation of the brightest pixel:", 100.0, -1e6,
                                                1e6, 3, &highAccepted);
    if (!highAccepted || high <= low) {
        logMessage("The brightest elevation must be above the darkest.", true);
        return;
    }

    const std::size_t pixels =
        static_cast<std::size_t>(it->width) * static_cast<std::size_t>(it->height);
    const auto stride = static_cast<int>(std::max<std::uint32_t>(
        1, static_cast<std::uint32_t>(
               std::sqrt(static_cast<double>(thinningFor(pixels))))));

    katana::terrain::TinInput input;
    input.points.reserve(pixels / static_cast<std::size_t>(stride * stride) + 1);
    for (int y = 0; y < it->height; y += stride) {
        for (int x = 0; x < it->width; x += stride) {
            const std::size_t index =
                (static_cast<std::size_t>(y) * static_cast<std::size_t>(it->width) +
                 static_cast<std::size_t>(x)) *
                4;
            if (index + 3 >= it->rgba.size() || it->rgba[index + 3] == 0) {
                continue; // transparent: no data
            }
            const double grey = (static_cast<double>(it->rgba[index]) +
                                 static_cast<double>(it->rgba[index + 1]) +
                                 static_cast<double>(it->rgba[index + 2])) /
                                (3.0 * 255.0);
            const auto world = it->pixelToWorld(x + 0.5, y + 0.5);
            input.points.push_back(
                katana::geometry::Point3(world.x, world.y, low + grey * (high - low)));
        }
    }
    if (input.points.size() < 3) {
        logMessage("That raster has too few opaque pixels to triangulate.", true);
        return;
    }

    katana::terrain::TinBuildOptions options;
    options.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto built = katana::terrain::buildTin(input, options);
    QApplication::restoreOverrideCursor();
    if (!built) {
        logMessage(QString::fromStdString(built.error().describe()), true);
        return;
    }
    logMessage(QString("Elevations approximated from the displayed grey levels between %1 and %2.")
                   .arg(low, 0, 'f', 3)
                   .arg(high, 0, 'f', 3));
    addSurface(chosen.toStdString(), std::move(built->surface));
}

void MainWindow::buildSurfaceFromDrawing()
{
    // Points and polyline vertices in the drawing, at the heights their
    // "elevation" property gives them - or, for a 3D string that came from a
    // 12d archive, the per-vertex "elevations" list. A 2D CAD drawing has no Z
    // of its own, so without either everything lands on the datum and the
    // surface is flat - which is reported rather than left to puzzle over.
    //
    // A vertex whose height is NULL is left out, and the breakline is broken
    // there: a null is "not surveyed", and triangulating it at zero would dig a
    // pit to the datum under every unlevelled point.
    katana::terrain::TinInput input;
    std::size_t withElevation = 0;
    std::size_t withoutHeight = 0;

    document_.model().entities.forEach([&](const Entity& entity) {
        if (!entity.visible) {
            return;
        }
        const bool carriesHeights = entity.properties.contains("elevation") ||
                                    entity.properties.contains("elevations");
        withElevation += carriesHeights ? 1 : 0;
        const auto heightAt = [&](const std::vector<std::optional<double>>& heights,
                                  std::size_t index) -> std::optional<double> {
            if (!carriesHeights) {
                return 0.0; // a plain 2D drawing: the datum, as before
            }
            return heights[index];
        };
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry)) {
            const auto heights = katana::archive12d::entityHeights(entity, 1);
            if (const auto z = heightAt(heights, 0)) {
                input.points.push_back(
                    katana::geometry::Point3(point->position.x, point->position.y, *z));
            } else {
                ++withoutHeight;
            }
        } else if (const auto* polyline =
                       std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
            const auto heights =
                katana::archive12d::entityHeights(entity, polyline->vertices.size());
            katana::terrain::Breakline breakline;
            const auto flush = [&] {
                if (breakline.vertices.size() >= 2) {
                    input.breaklines.push_back(breakline);
                }
                breakline.vertices.clear();
            };
            bool whole = true;
            for (std::size_t i = 0; i < polyline->vertices.size(); ++i) {
                const auto& vertex = polyline->vertices[i];
                const auto z = heightAt(heights, i);
                if (!z) {
                    ++withoutHeight;
                    whole = false;
                    flush();
                    continue;
                }
                breakline.vertices.push_back(katana::geometry::Point3(vertex.x, vertex.y, *z));
                input.points.push_back(katana::geometry::Point3(vertex.x, vertex.y, *z));
            }
            // Closing only makes sense for a ring that lost none of its vertices.
            breakline.closed = polyline->closed && whole;
            flush();
        }
    });
    if (withoutHeight != 0) {
        logMessage(QString("%1 vertices have no height and were left out of the surface.")
                       .arg(grouped(withoutHeight)));
    }

    if (input.points.size() < 3) {
        logMessage("The drawing has fewer than three points to triangulate.", true);
        return;
    }
    if (withElevation == 0) {
        logMessage("No entity carries an 'elevation' property; the surface will be flat.", true);
    }

    katana::terrain::TinBuildOptions options;
    options.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
    options.crossingBreaklines = katana::terrain::CrossingBreaklinePolicy::Average;
    auto built = katana::terrain::buildTin(input, options);
    if (!built) {
        logMessage(QString::fromStdString(built.error().describe()), true);
        return;
    }
    addSurface("Drawing", std::move(built->surface));
}

void MainWindow::cutSectionAlongSelection()
{
    const auto selected = document_.selection().ids();
    if (selected.size() != 1) {
        logMessage("Select exactly one line or polyline to cut along.", true);
        return;
    }
    const Entity* entity = document_.model().entities.find(selected.front());
    if (entity == nullptr) {
        logMessage("The selected entity no longer exists.", true);
        return;
    }

    katana::geometry::Polyline2 alignment;
    if (const auto* line = std::get_if<katana::geometry::Segment2>(&entity->geometry)) {
        alignment.vertices = {line->start, line->end};
    } else if (const auto* polyline =
                   std::get_if<katana::geometry::Polyline2>(&entity->geometry)) {
        alignment = *polyline;
    } else {
        logMessage("A section must be cut along a line or a polyline.", true);
        return;
    }

    cutSectionAlong(std::move(alignment), "the selection");
}

void MainWindow::cutSectionAlong(katana::geometry::Polyline2 alignment, const QString& along,
                                 const katana::geometry::SolvedProfile* profile,
                                 const std::string& profileName)
{
    if (sceneSurfaces_.empty()) {
        logMessage("Build a surface first (Terrain > Surface From ...).", true);
        return;
    }
    std::vector<cad::SectionSurfaceInput> inputs;
    inputs.reserve(sceneSurfaces_.size());
    for (const auto& item : sceneSurfaces_) {
        if (item.visible && item.surface != nullptr) {
            inputs.push_back(cad::SectionSurfaceInput{item.name, item.surface});
        }
    }

    cad::SectionOptions options;
    // Two thousand stations across the alignment, floored at 100 mm: fine
    // enough to read off, and the surface breaks are added on top of it so the
    // interval never decides the accuracy of a ridge or a toe.
    options.interval = std::max(alignment.length() / 2000.0, 0.1);
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto section = cad::extractSection(alignment, inputs, &document_.model(), options);
    QApplication::restoreOverrideCursor();
    if (!section) {
        logMessage(QString::fromStdString(section.error().describe()), true);
        return;
    }
    if (profile != nullptr) {
        if (auto status = cad::appendDesignProfile(*section, *profile, profileName); !status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
            return;
        }
    }

    const double length = section->length;
    const std::size_t crossings = section->crossings.size();
    if (!views_->showSection(std::move(*section))) {
        logMessage("No viewport could show the section.", true);
        return;
    }
    refreshViewMenu();
    logMessage(QString("Section cut along %1: length %2, interval %3, %4 crossings.")
                   .arg(along)
                   .arg(length, 0, 'f', 3)
                   .arg(options.interval, 0, 'f', 3)
                   .arg(crossings));
}

void MainWindow::cutSectionAlongAlignment()
{
    const std::vector<std::string> names = document_.model().alignments.names();
    if (names.empty()) {
        logMessage("Define an alignment first: ALIGN NEW name x,y x,y ... in the command line.",
                   true);
        return;
    }
    QStringList items;
    for (const std::string& name : names) {
        items << QString::fromStdString(name);
    }
    bool ok = false;
    const QString chosen = QInputDialog::getItem(this, "Cut Section Along Alignment",
                                                 "Alignment:", items, 0, false, &ok);
    if (!ok || chosen.isEmpty()) {
        return;
    }
    const katana::entity::Alignment* alignment =
        document_.model().alignments.find(chosen.toStdString());
    if (alignment == nullptr) {
        logMessage("The alignment no longer exists.", true);
        return;
    }
    auto solved = katana::geometry::solveAlignment(alignment->horizontal);
    if (!solved) {
        logMessage(QString::fromStdString(solved.error().describe()), true);
        return;
    }
    // The design profile rides along when the alignment has one, so the
    // section shows design against ground - the reason to have a profile.
    std::optional<katana::geometry::SolvedProfile> profile;
    if (alignment->vertical.has_value()) {
        auto solvedProfile = katana::geometry::solveProfile(*alignment->vertical);
        if (!solvedProfile) {
            logMessage(QString::fromStdString(solvedProfile.error().describe()), true);
            return;
        }
        profile = std::move(*solvedProfile);
    }
    // 10 mm chords. The section samples at most every 100 mm along the line,
    // so a finer centreline would cost time and change nothing it reports.
    cutSectionAlong(solved->toPolyline(0.01), QString("alignment %1").arg(chosen),
                    profile ? &*profile : nullptr, "design " + chosen.toStdString());
}

std::optional<MainWindow::CorridorRequest> MainWindow::askCorridor(const QString& title)
{
    // Only an alignment with a design profile has a finished level to measure
    // against or build; the message says what is missing rather than hiding
    // the command.
    QStringList names;
    for (const katana::entity::Alignment& alignment : document_.model().alignments.all()) {
        if (alignment.vertical.has_value()) {
            names << QString::fromStdString(alignment.name);
        }
    }
    if (names.isEmpty()) {
        logMessage("No alignment has a design profile. Define one with ALIGN DESIGN name s,z ...",
                   true);
        return std::nullopt;
    }
    QStringList surfaceNames;
    std::vector<const katana::terrain::TinSurface*> surfaces;
    for (const auto& item : sceneSurfaces_) {
        if (item.visible && item.surface != nullptr) {
            surfaceNames << QString::fromStdString(item.name);
            surfaces.push_back(item.surface);
        }
    }
    if (surfaces.empty()) {
        logMessage("Build a surface first (Terrain > Surface From ...).", true);
        return std::nullopt;
    }

    QDialog dialog(this);
    dialog.setWindowTitle(title);
    auto* form = new QFormLayout(&dialog);
    auto* alignmentBox = new QComboBox(&dialog);
    alignmentBox->addItems(names);
    auto* surfaceBox = new QComboBox(&dialog);
    surfaceBox->addItems(surfaceNames);
    const auto spin = [&](double minimum, double maximum, double value, const QString& suffix) {
        auto* box = new QDoubleSpinBox(&dialog);
        box->setRange(minimum, maximum);
        box->setDecimals(3);
        box->setValue(value);
        box->setSuffix(suffix);
        return box;
    };
    // Defaults are a two-lane rural road: 3.5 m lanes at 2.5% crossfall,
    // 1.5:1 in cut and 2:1 in fill, sectioned every 10 m.
    auto* halfWidth = spin(0.5, 50.0, 3.5, " m");
    auto* crossfall = spin(-20.0, 20.0, 2.5, " %");
    auto* cutBatter = spin(0.1, 10.0, 1.5, " : 1");
    auto* fillBatter = spin(0.1, 10.0, 2.0, " : 1");
    auto* interval = spin(0.1, 1000.0, 10.0, " m");
    form->addRow("Alignment", alignmentBox);
    form->addRow("Ground surface", surfaceBox);
    form->addRow("Half width", halfWidth);
    form->addRow("Crossfall", crossfall);
    form->addRow("Cut batter (run per rise)", cutBatter);
    form->addRow("Fill batter (run per rise)", fillBatter);
    form->addRow("Section interval", interval);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    form->addRow(buttons);
    if (dialog.exec() != QDialog::Accepted) {
        return std::nullopt;
    }

    const katana::entity::Alignment* alignment =
        document_.model().alignments.find(alignmentBox->currentText().toStdString());
    if (alignment == nullptr || !alignment->vertical.has_value()) {
        logMessage("The alignment no longer exists or lost its profile.", true);
        return std::nullopt;
    }
    auto solved = katana::geometry::solveAlignment(alignment->horizontal);
    auto profile = katana::geometry::solveProfile(*alignment->vertical);
    if (!solved || !profile) {
        logMessage(QString::fromStdString((solved ? profile.error() : solved.error()).describe()),
                   true);
        return std::nullopt;
    }

    CorridorRequest request;
    request.alignment = std::move(*solved);
    request.profile = std::move(*profile);
    request.ground = surfaces[static_cast<std::size_t>(surfaceBox->currentIndex())];
    request.alignmentName = alignmentBox->currentText();
    request.surfaceName = surfaceBox->currentText();
    request.assembly.halfWidth = halfWidth->value();
    request.assembly.crossfall = crossfall->value() / 100.0;
    request.assembly.cutBatter = cutBatter->value();
    request.assembly.fillBatter = fillBatter->value();
    request.crossfallPercent = crossfall->value();
    request.interval = interval->value();
    return request;
}

void MainWindow::corridorQuantities()
{
    const auto request = askCorridor("Corridor Quantities");
    if (!request) {
        return;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto quantities = cad::corridorQuantities(request->alignment, request->profile,
                                              request->assembly, *request->ground,
                                              request->interval);
    QApplication::restoreOverrideCursor();
    if (!quantities) {
        logMessage(QString::fromStdString(quantities.error().describe()), true);
        return;
    }

    // The report: one line per section, then the totals - the shape an
    // earthworks schedule takes, so it can be read against one. QString::number
    // is not locale-aware, which here is what is wanted: a decimal point.
    QString report;
    report += QString("Corridor quantities: %1 on %2\n").arg(request->alignmentName,
                                                            request->surfaceName);
    report += QString("Half width %1 m, crossfall %2 %, batters %3:1 cut / %4:1 fill, interval %5 m\n")
                  .arg(request->assembly.halfWidth, 0, 'f', 2)
                  .arg(request->crossfallPercent, 0, 'f', 2)
                  .arg(request->assembly.cutBatter, 0, 'f', 2)
                  .arg(request->assembly.fillBatter, 0, 'f', 2)
                  .arg(request->interval, 0, 'f', 2);
    report += "\n    station     design z     cut area    fill area\n";
    for (const cad::CorridorSection& section : quantities->sections) {
        report += QString("%1  %2  %3  %4%5\n")
                      .arg(section.station, 11, 'f', 3)
                      .arg(section.designElevation, 11, 'f', 3)
                      .arg(section.cutArea, 11, 'f', 3)
                      .arg(section.fillArea, 11, 'f', 3)
                      .arg(section.complete ? "" : "   (no daylight - excluded)");
    }
    report += QString("\nCut %1 m3   Fill %2 m3   Net %3 m3 (%4)\n")
                  .arg(quantities->cut, 0, 'f', 1)
                  .arg(quantities->fill, 0, 'f', 1)
                  .arg(quantities->net, 0, 'f', 1)
                  .arg(quantities->net >= 0.0 ? "import" : "surplus");
    if (quantities->incompleteSections > 0) {
        report += QString("%1 of %2 sections could not reach the ground and contribute NOTHING:"
                          " these totals are not the whole job.\n")
                      .arg(quantities->incompleteSections)
                      .arg(quantities->sections.size());
    }

    QDialog table(this);
    table.setWindowTitle("Corridor Quantities");
    auto* layout = new QVBoxLayout(&table);
    auto* text = new QPlainTextEdit(&table);
    text->setReadOnly(true);
    text->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    text->setPlainText(report);
    layout->addWidget(text);
    auto* close = new QDialogButtonBox(QDialogButtonBox::Close, &table);
    connect(close, &QDialogButtonBox::rejected, &table, &QDialog::reject);
    connect(close, &QDialogButtonBox::accepted, &table, &QDialog::accept);
    layout->addWidget(close);
    table.resize(720, 520);
    table.exec();

    logMessage(QString("Corridor %1: cut %2 m3, fill %3 m3, net %4 m3 over %5 sections%6")
                   .arg(request->alignmentName)
                   .arg(quantities->cut, 0, 'f', 1)
                   .arg(quantities->fill, 0, 'f', 1)
                   .arg(quantities->net, 0, 'f', 1)
                   .arg(quantities->sections.size())
                   .arg(quantities->incompleteSections > 0
                            ? QString(" (%1 incomplete)").arg(quantities->incompleteSections)
                            : QString()));
}

void MainWindow::corridorSurface()
{
    const auto request = askCorridor("Corridor Surface");
    if (!request) {
        return;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto corridor = cad::corridorSurface(request->alignment, request->profile, request->assembly,
                                         *request->ground, request->interval);
    QApplication::restoreOverrideCursor();
    if (!corridor) {
        logMessage(QString::fromStdString(corridor.error().describe()), true);
        return;
    }
    const std::size_t triangles = corridor->surface.triangleCount();
    const std::size_t sections = corridor->sections;
    const std::size_t incomplete = corridor->incompleteSections;
    addSurface("corridor " + request->alignmentName.toStdString(), std::move(corridor->surface));
    logMessage(QString("Corridor surface for %1 added: %2 triangles from %3 sections%4. It is a "
                       "surface like any other - visible in 3D and in sections.")
                   .arg(request->alignmentName)
                   .arg(triangles)
                   .arg(sections)
                   .arg(incomplete > 0 ? QString(", %1 left out for missing daylight (hull-bounded)")
                                             .arg(incomplete)
                                       : QString()));
}

void MainWindow::plotToPdf()
{
    ViewportWidget* view = views_->activePlanView();
    if (view == nullptr) {
        logMessage("Open a plan viewport to plot from.", true);
        return;
    }

    QDialog dialog(this);
    dialog.setWindowTitle("Plot to PDF");
    auto* form = new QFormLayout(&dialog);
    auto* paperBox = new QComboBox(&dialog);
    paperBox->addItems({"A4", "A3", "A2", "A1", "A0"});
    paperBox->setCurrentIndex(1);
    auto* orientationBox = new QComboBox(&dialog);
    orientationBox->addItems({"Landscape", "Portrait"});
    // Fitting is the default because it is what a first plot of any drawing
    // wants, and it picks a scale a scale rule carries.
    auto* fit = new QCheckBox("Fit the drawing to the sheet at a standard scale", &dialog);
    fit->setChecked(true);
    auto* scale = new QDoubleSpinBox(&dialog);
    scale->setRange(1.0, 1000000.0);
    scale->setDecimals(0);
    scale->setValue(1000.0);
    scale->setPrefix("1 : ");
    auto* dpi = new QDoubleSpinBox(&dialog);
    dpi->setRange(72.0, 1200.0);
    dpi->setDecimals(0);
    dpi->setValue(300.0);
    auto* margin = new QDoubleSpinBox(&dialog);
    margin->setRange(0.0, 50.0);
    margin->setDecimals(1);
    margin->setValue(10.0);
    margin->setSuffix(" mm");
    form->addRow("Paper", paperBox);
    form->addRow("Orientation", orientationBox);
    form->addRow(fit);
    form->addRow("Scale, when not fitting", scale);
    form->addRow("Resolution (dpi)", dpi);
    form->addRow("Margin", margin);
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
    connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    form->addRow(buttons);
    if (dialog.exec() != QDialog::Accepted) {
        return;
    }

    cad::PlotSettings settings;
    const std::array<cad::PaperSize, 5> sizes{cad::PaperSize::A4, cad::PaperSize::A3,
                                              cad::PaperSize::A2, cad::PaperSize::A1,
                                              cad::PaperSize::A0};
    settings.paper = sizes[static_cast<std::size_t>(paperBox->currentIndex())];
    settings.landscape = orientationBox->currentIndex() == 0;
    settings.dpi = dpi->value();
    settings.marginMm = margin->value();
    settings.scaleDenominator = scale->value();

    QString path = QFileDialog::getSaveFileName(this, "Plot to PDF", QString(), "PDF (*.pdf)");
    if (path.isEmpty()) {
        return;
    }
    if (!path.endsWith(".pdf", Qt::CaseInsensitive)) {
        path += ".pdf";
    }
    if (const auto status = plotDrawingToPdf(path, settings, fit->isChecked()); !status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
    }
}

namespace {

const char* paperName(cad::PaperSize size)
{
    switch (size) {
    case cad::PaperSize::A0:
        return "A0";
    case cad::PaperSize::A1:
        return "A1";
    case cad::PaperSize::A2:
        return "A2";
    case cad::PaperSize::A3:
        return "A3";
    case cad::PaperSize::A4:
        return "A4";
    }
    return "?";
}

} // namespace

katana::core::Status MainWindow::plotDrawingToPdf(const QString& path, cad::PlotSettings settings,
                                                  bool fitToDrawing)
{
    ViewportWidget* view = views_->activePlanView();
    if (view == nullptr) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidState,
                                       "no plan viewport to plot from");
    }
    if (fitToDrawing) {
        const katana::geometry::Box2 extent = document_.spatialIndex().bounds();
        auto fitted = cad::fitScale(extent, settings);
        if (!fitted) {
            return fitted.error();
        }
        settings.scaleDenominator = *fitted;
        settings.center = katana::geometry::Point2(0.5 * (extent.min.x + extent.max.x),
                                                   0.5 * (extent.min.y + extent.max.y));
    } else {
        settings.center = view->viewTransform().center;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const auto status = view->plotToPdf(path, settings);
    QApplication::restoreOverrideCursor();
    if (!status) {
        return status;
    }
    logMessage(QString("Plotted to %1: %2 %3 at 1 : %4, %5 dpi. Line widths are the layers' "
                       "line weights in millimetres.")
                   .arg(path, paperName(settings.paper), settings.landscape ? "landscape" : "portrait")
                   .arg(settings.scaleDenominator, 0, 'f', 0)
                   .arg(settings.dpi, 0, 'f', 0));
    return {};
}

} // namespace katana::qt
