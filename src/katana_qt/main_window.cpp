#include "main_window.hpp"

#include "katana/core/cpu_features.hpp"
#include "katana/core/text.hpp"
#if defined(KATANA_HAS_GPU)
#include "gpu/renderer_choice.hpp"
#endif

#include "theme.hpp"

#include "icons.hpp"
#include "alignment_manager.hpp"
#include "attribute_manager.hpp"
#include "customisation/drawing_summary_dialog.hpp"
#include "format.hpp"
#include "dataset_info_dialog.hpp"
#include "geo/replies.hpp"
#include "gis_export_dialog.hpp"
#include "gis_import_dialogs.hpp"
#include "surface_raster_dialog.hpp"
#include "jobs.hpp"
#include "layer_manager.hpp"
#include "plan_context_menu.hpp"
#include "plotting/plot_dialog.hpp"
#include "plotting/plot_drawing_dialog.hpp"
#include "plotting/view_image_export.hpp"
#include "plotting/sheet_arrange.hpp"
#include "plotting/sheet_checks.hpp"
#include "plotting/sheet_tables.hpp"
#include "project_crs_dialog.hpp"
#include "render_view_widget.hpp"
#include "style_manager.hpp"

#include <chrono>
#include <QAction>
#include <QAbstractButton>
#include <QActionGroup>
#include <QApplication>
#include <QCloseEvent>
#include <QColorDialog>
#include <QComboBox>
#include <QDockWidget>
#include <QCoreApplication>
#include <QFileDialog>
#include <QHBoxLayout>
#include <QHeaderView>
#include <array>
#include <functional>
#include <optional>

#include <QDialog>
#include <QDialogButtonBox>
#include <QFileInfo>
#include <QDoubleSpinBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QCheckBox>
#include <QClipboard>
#include <QInputDialog>
#include <QPixmap>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QLocale>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QProgressDialog>
#include <QPushButton>
#include <QSettings>
#include <QRegularExpression>
#include <QShortcut>
#include <QSignalBlocker>
#include <QTabWidget>
#include <QStatusBar>
#include <QTableWidget>
#include <QTimer>
#include <QTreeWidget>
#include <QTreeWidgetItem>
#include <QToolBar>
#include <QToolButton>
#include <QVBoxLayout>

#include <algorithm>
#include <cstdio>
#include <map>
#include <set>
#include <utility>

#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/corridor.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/archive12d/customisation.hpp"
#include "katana/cad/customisation_report.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/dataset_info.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/terrain_io.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"
#include "katana/storage/project_store.hpp"

namespace katana::qt {

namespace {

// The vector kernels in force, and why when it is not what the processor
// could run (core::simdSelection).
QString simdDescription()
{
    const katana::core::SimdSelection& simd = katana::core::simdSelection();
    QString text = QString::fromLatin1(katana::core::toString(simd.active));
    if (!simd.note.empty()) {
        text += QString(" (%1)").arg(QString::fromStdString(simd.note));
    }
    return text;
}

// What the renderer rules choose for a 3D view made now, and why
// (gpu::chooseRenderer).
QString rendererDescription()
{
#if defined(KATANA_HAS_GPU)
    const auto decision =
        gpu::chooseRenderer(gpu::currentRendererEnvironment(false, false));
    return QString("%1 - %2")
        .arg(QString::fromLatin1(gpu::toString(decision.kind)),
             QString::fromStdString(decision.reason));
#else
    return QStringLiteral("software - the GPU renderer is not built into this copy");
#endif
}

} // namespace

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

// The reference data panel's columns. The Name cell carries the layer's
// ReferenceId as user data.
enum ReferenceColumn { kRefName = 0, kRefType, kRefDetail, kRefDisplay, kRefColumns };

// The vertical exaggeration a person may choose, the box's range since View >
// Vertical Exaggeration had one: a hundredth flattens a section to a line,
// and beyond a thousand a model of metres is a wall of spikes.
constexpr double kLeastExaggeration = 0.01;
constexpr double kMostExaggeration = 1000.0;

// What the command line shows while nothing is typed and no tool is asking.
constexpr const char* kCommandPlaceholder =
    "Command:  LINE  |  CIRCLE 5,5 3  |  SELECT ALL  |  HELP   (Enter repeats the last tool)";

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

// Rows shown in the property panel for one entity of `model`.
std::vector<std::pair<QString, QString>> describeGeometry(const katana::entity::Model& model,
                                                          const katana::entity::Geometry& geometry)
{
    using Rows = std::vector<std::pair<QString, QString>>;
    struct Visitor {
        // A label's words are worked out from its target, which is in the
        // model, not in the label.
        const katana::entity::Model& model;

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
            Rows rows = {{"Position", point(g.position)},
                         {"Text", QString::fromStdString(g.text)},
                         {"Height", number(g.height)},
                         {"Rotation", number(g.rotation * katana::math::kRadToDeg) + " deg"}};
            if (!g.style.empty()) {
                rows.push_back({"Text style", QString::fromStdString(g.style)});
            }
            if (g.paperHeight > 0.0) {
                rows.push_back({"Paper height", number(g.paperHeight) + " mm"});
            }
            rows.push_back({"Justify", QString::fromUtf8(katana::entity::toString(g.justify))});
            return rows;
        }
        Rows operator()(const katana::entity::DimensionGeometry& g) const
        {
            Rows rows = {{"Kind", QString::fromUtf8(katana::entity::toString(g.kind))},
                         {"Start", point(g.start)},
                         {"End", point(g.end)},
                         {"Measurement", number(g.measurement())},
                         {"Offset", number(g.offset)}};
            if (g.usesVertex()) {
                rows.push_back({"Vertex", point(g.vertex)});
            }
            if (g.startRef.associated() || g.endRef.associated() || g.vertexRef.associated()) {
                rows.push_back({"Associative", "yes"});
            }
            return rows;
        }
        Rows operator()(const katana::entity::LabelGeometry& g) const
        {
            Rows rows = {{"Label style", QString::fromStdString(g.style)}};
            if (g.target != 0) {
                rows.push_back({"Labels", QString::number(g.target)});
            } else {
                rows.push_back({"Alignment", QString::fromStdString(g.alignment)});
            }
            if (g.part >= 0) {
                rows.push_back({"Part", QString::number(g.part)});
            }
            if (!g.rule.empty()) {
                rows.push_back({"Rule", QString::fromStdString(g.rule)});
            }
            // What it says and where, as LABEL LIST replies them: the words
            // through the same resolver, so the two cannot disagree.
            rows.push_back(
                {"Text", QString::fromStdString(katana::cad::annotation::shownLabelText(model, g))});
            rows.push_back({"Position", g.position ? "pinned at " + point(*g.position)
                                                   : QString("automatic")});
            return rows;
        }
        Rows operator()(const katana::entity::LeaderGeometry& g) const
        {
            return {{"Tip", point(g.vertices.front())},
                    {"Vertices", QString::number(g.vertices.size())},
                    {"Text", QString::fromStdString(g.text)},
                    {"Callout", QString::fromUtf8(katana::entity::toString(g.callout))}};
        }
    };
    return std::visit(Visitor{model}, geometry);
}

// How many of the loaded library's definitions are symbols by decision D3 -
// what the Symbol Library lists: a `mode vertex` definition, one a survey rule
// or a style draws as a symbol, one from a symbol file. Counting `mode vertex`
// alone called 157 of the reference library's definitions symbols and left
// out the trees and valves its mapfiles draw as symbols without it.
std::size_t librarySymbolCount(const katana::cad::Document& document)
{
    const std::vector<katana::cad::CatalogueEntry> choices = katana::cad::symbolChoices(document);
    return static_cast<std::size_t>(std::ranges::count_if(choices, [](const auto& entry) {
        return entry.source == katana::cad::DefinitionSource::Library;
    }));
}

QString describeProperty(const katana::entity::PropertyValue& value)
{
    // One definition of what a value says, in entity: the panel, the command
    // line and the attribute manager must not disagree about it.
    return QString::fromStdString(katana::entity::toString(value));
}

// A panel's own tool: an icon, with its name in the tooltip. The text buttons
// these replaced were at least 64 px each (theme.cpp's QPushButton rule) plus
// padding, so four of them held the Layers column at 402 px; four of these
// fit in 120 and the column can be as narrow as its tree is useful.
QToolButton* panelTool(QWidget* parent, Icon which, const QString& objectName,
                       const QString& name, const QString& tip)
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

// A row of panel tools above the list they act on, where a toolbar is looked
// for.
QHBoxLayout* toolRow(std::initializer_list<QToolButton*> tools)
{
    auto* row = new QHBoxLayout();
    row->setSpacing(1);
    for (QToolButton* tool : tools) {
        row->addWidget(tool);
    }
    row->addStretch();
    return row;
}

} // namespace

MainWindow::MainWindow(QWidget* parent) : QMainWindow(parent)
{
    logger_.setMinimumLevel(katana::core::LogLevel::Warning);
    logger_.addSink(katana::core::makeStderrSink());

    resize(1360, 860);
    views_ = new ViewWorkspace(document_, this);
    setCentralWidget(views_);
    // After the workspace: Qt deletes a window's children in the order they
    // were made, so the workspace's views go while the chrome that keeps
    // their records is still there to drop them.
    chrome_ = new DockChrome(*this);
    views_->setChrome(chrome_);

    buildActions();
    buildDocks();
    buildStatusBar();
    // A category the catalogue gains that no menu here takes is still
    // reachable by its aliases and ids on the command line; said, once the
    // log exists, rather than left to vanish from the window.
    for (const std::string& category : toolActions_.unplaced()) {
        logMessage("The tool category " + QString::fromStdString(category) +
                   " has no menu; its tools start from the command line.");
    }
    views_->setReferenceData(&reference_);
    // CODE and MAPFILE CHECK name colours by the standard table, which is
    // archive12d's and so out of the interpreter's reach (survey_code_verbs.hpp).
    interpreter_.setColourLookup(
        [](std::string_view name) { return katana::archive12d::standardColour(name); });
    // GENERATE on the command line lays out what Generate Sheets would: what
    // the plan view draws, and the visible surfaces for the sections; SHEETS
    // CHECK and ARRANGE see what the painter sees. The same context the
    // sheet editor's own lines get (sheetVerbContextFor).
    interpreter_.setSheetContext(
        [this] { return sheetVerbContextFor(document_, [this] { return sheetSource(); }); });
    // VIEW in a verb's scope (cad/scope_verbs.hpp) - MODIFY VIEW, UTILITY
    // REPORT VIEW - is a view of this window's: the plan view in use, or the
    // one with the id given, with its own hidden layers and what it shows.
    interpreter_.setScopeContext([this](std::optional<std::uint32_t> id) {
        return cad::scopeViewOf(views_->viewSet(), id);
    });

    views_->onPrompt = [this](const QString& prompt) {
        statusBar()->showMessage(prompt);
        // A running tool's prompt shows where its answer is typed, as
        // AutoCAD's command line shows it.
        if (!prompt.isEmpty() && !views_->activeToolId().empty()) {
            commandInput_->setPlaceholderText(prompt);
        }
    };
    views_->onError = [this](const QString& error) { logMessage(error, true); };
    views_->onCursorMoved = [this](const katana::geometry::Point2& world,
                                      const std::optional<cad::SnapResult>& snap) {
        coordinateLabel_->setText(point(world));
        snapLabel_->setText(snap ? cad::toString(snap->mode) : "");
    };
    views_->onStatus = [this](const QString& text) { statusBar()->showMessage(text); };
    views_->onFrameStats = [this](const QString& text) { frameStatsLabel_->setText(text); };
    views_->onActiveChanged = [this] { refreshViewMenu(); };
    // The running tool's action checked, and Select while none runs, in the
    // menus and on the toolbars alike: one action per tool, so they agree.
    views_->onActiveToolChanged = [this](const std::string& id) { showRunningTool(id); };
    // "1 line", "3 lines trimmed": what a tool did goes in the log with
    // everything else the drawing was told.
    views_->onToolMessage = [this](const QString& message) { logMessage(message); };
    // Type anywhere: a letter typed over the drawing with no tool running is
    // the start of a command, so it goes to the command line, which keeps the
    // keyboard for the rest of the word.
    views_->onTextTyped = [this](const QString& text) {
        commandInput_->setFocus(Qt::ShortcutFocusReason);
        commandInput_->insert(text);
    };
    // A right-click with no tool running: the selection's verbs
    // (plan_context_menu.hpp). A double click on an entity: what edits it.
    views_->onContextMenu = [this](const QPoint& globalPos) { showPlanContextMenu(globalPos); };
    views_->onEntityDoubleClicked = [this](katana::entity::EntityId id) {
        editDoubleClicked(id);
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
    fileListener_ = document_.addListener([this](const cad::DocumentChange& change) {
        if (change.has(cad::DocumentChange::Replaced | cad::DocumentChange::Saved |
                       cad::DocumentChange::Metadata)) {
            suggestPlotFiles();
        }
    });
    refreshAll();
    views_->stopTool();
    selectAction_->setChecked(true);
    logMessage("Katana ready. Type HELP for the command list.");
}

// ---- construction -----------------------------------------------------------------------

// A QAction is made ONCE and shared by its menu and its toolbar, so the two
// cannot drift: one text, one shortcut, one icon, one enabled state. `tip` is
// the sentence shown in the status bar and, with the shortcut appended, in the
// toolbar tooltip - an icon-only button owes the user its name.
QAction* MainWindow::makeAction(Icon icon, const QString& text, const QString& tip,
                                const QKeySequence& shortcut, const QString& name)
{
    auto* action = new QAction(katana::qt::icon(icon), text, this);
    if (!name.isEmpty()) {
        action->setObjectName(name);
    }
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
    QAction* newAction = makeAction(Icon::New, "&New", "Start a new, empty drawing",
                                    QKeySequence::New, "fileNew");
    // Every one named, as --action and --trigger find a menu item by its
    // object name: these six had none, so an agent driving the window could
    // not reach them.
    QAction* openAction = makeAction(Icon::Open, "&Open Project...", "Open a Katana project directory",
                                     QKeySequence::Open, "fileOpen");
    QAction* saveAction =
        makeAction(Icon::Save, "&Save", "Save the project", QKeySequence::Save, "fileSave");
    QAction* saveAsAction =
        makeAction(Icon::SaveAs, "Save &As...", "Save the project under another name",
                   QKeySequence::SaveAs, "fileSaveAs");
    QAction* importAction =
        makeAction(Icon::Import, "&Import...",
                   "Import a drawing, an image or a point cloud (DXF, SHP, GeoTIFF, LAS ...)",
                   QKeySequence(Qt::CTRL | Qt::Key_I), "fileImport");
    QAction* exportAction = makeAction(Icon::Export, "Export &Vector...",
                                       "Export the drawing (DXF, GeoPackage, GeoJSON, SHP ...)",
                                       QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_E),
                                       "fileExportVector");
    QAction* plotAction = makeAction(Icon::Plot, "&Plot to PDF...",
                                     "Plot the drawing to a sheet at a standard scale",
                                     QKeySequence::Print, "filePlot");
    plotAction->setData("plotDrawingDialog");
    // A picture of a view, and the same on the clipboard from Edit
    // (plotting/view_image_export.hpp): both are the SNAPSHOT verb.
    QAction* exportImageAction =
        makeAction(Icon::Export, "Export View as I&mage...",
                   "Save the plan or 3D view as a PNG, JPEG or TIFF, at the view's size or larger",
                   QKeySequence(), "fileExportViewImage");
    exportImageAction->setData("viewImageDialog");
    connect(exportImageAction, &QAction::triggered, this, [this] { showViewImageExport(); });
    // A katana_cli script run in the window, a line at a time through the
    // one executor (script_runner.hpp); the dialog shows what the file holds
    // and runs the SCRIPT line.
    QAction* runScriptAction =
        makeAction(Icon::CommandLine, "&Run Script...",
                   "Run a script of commands (.kcs), a line at a time, as katana_cli runs it",
                   QKeySequence(), "fileRunScript");
    connect(runScriptAction, &QAction::triggered, this, [this] { showRunScript(); });
    connect(newAction, &QAction::triggered, this, [this] { newDocument(); });
    connect(openAction, &QAction::triggered, this, [this] { openDocument(); });
    connect(saveAction, &QAction::triggered, this, [this] { saveDocument(); });
    connect(saveAsAction, &QAction::triggered, this, [this] { saveDocumentAs(); });
    connect(importAction, &QAction::triggered, this, [this] { importFile(); });
    connect(exportAction, &QAction::triggered, this, [this] { exportVectorFile(); });
    connect(plotAction, &QAction::triggered, this, [this] { plotToPdf(); });
    QAction* sheetsAction =
        makeAction(Icon::Plot, "S&heets...",
                   "Lay the drawing out on sheets with a title block, and plot them",
                   QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_P), "fileSheets");
    connect(sheetsAction, &QAction::triggered, this, [this] { showSheets(); });
    QAction* plotSheetsAction =
        makeAction(Icon::Plot, "Plot Sheets to P&DF...",
                   "Plot every sheet of the project to one PDF", QKeySequence(), "filePlotSheets");
    connect(plotSheetsAction, &QAction::triggered, this, [this] {
        // The Plot dialog (plotting/plot_dialog.hpp) over every sheet; the
        // page setup is kept only for sheets the project has.
        const auto set = sheetsToPlot();
        if (!set) {
            logMessage(QString::fromStdString(set.error().describe()), true);
            return;
        }
        plotInteractively(this, *set, 0, true, suggestedPlotFile(document_),
                          QString::fromStdString(document_.metadata().name),
                          [this] { return sheetSource(); },
                          document_.sheetSet().sheets.empty() ? nullptr : &document_,
                          [this](const QString& text, bool isError) { logMessage(text, isError); });
    });

    QAction* customiseAction =
        makeAction(Icon::Import, "Loa&d Customisation...",
                   "Load linestyle and symbol libraries (.4d) and survey code files (.mapfile) "
                   "on top of what is loaded: a file's definitions and codes take the place of "
                   "the same ones, and everything else is kept",
                   {}, "loadCustomisation");
    connect(customiseAction, &QAction::triggered, this,
            [this] { loadCustomisation(katana::archive12d::LoadMode::Merge); });
    // Replacing is asked for, never the default (D1): a person loading their
    // own symbol file wants it added to the 792 definitions already loaded,
    // not in their place. An action, not a question box, so that a headless
    // run never meets a box nobody can answer.
    QAction* replaceCustomisationAction =
        makeAction(Icon::Import, "&Replace Loaded Customisation...",
                   "Load libraries and survey code files IN PLACE of the loaded ones: a "
                   "library replaces the whole library, a survey code file every survey code; "
                   "a kind the files do not bring is kept",
                   {}, "replaceCustomisation");
    connect(replaceCustomisationAction, &QAction::triggered, this,
            [this] { loadCustomisation(katana::archive12d::LoadMode::Replace); });

    QAction* codeAction = makeAction(Icon::Import, "Appl&y Survey Codes",
                                     "Give every entity carrying a field code the layer, style "
                                     "and attributes the loaded survey codes say it should have",
                                     {}, "applySurveyCodes");
    connect(codeAction, &QAction::triggered, this, [this] { applySurveyCodes(); });

    // The menu bar, in the order a CAD user reads it: the file, editing and
    // viewing it, the three tool menus, Format for how things are drawn,
    // then the survey, civil and GIS work, and Help. Made here, in order,
    // and filled below; each mnemonic letter is its own (--check-shortcuts).
    const auto topMenu = [this](const QString& title, const QString& name) {
        QMenu* menu = menuBar()->addMenu(title);
        menu->setObjectName(name);
        return menu;
    };
    QMenu* fileMenu = topMenu("&File", "fileMenu");
    QMenu* editMenu = topMenu("&Edit", "editMenu");
    viewMenu_ = topMenu("&View", "viewMenu");
    QMenu* drawMenu = topMenu("&Draw", "drawMenu");
    QMenu* modifyMenu = topMenu("&Modify", "modifyMenu");
    QMenu* annotateMenu = topMenu("&Annotate", "annotateMenu");
    QMenu* formatMenu = topMenu("F&ormat", "formatMenu");
    QMenu* surveyMenu = topMenu("&Survey", "surveyMenu");
    QMenu* terrainMenu = topMenu("&Terrain", "terrainMenu");
    QMenu* gisMenu = topMenu("&GIS", "gisMenu");
    QMenu* helpMenu = topMenu("&Help", "helpMenu");

    // File keeps to files: the customisation is loaded from Format (and
    // Survey > Survey Coding), where the managers of what it brings are.
    fileMenu->addActions({newAction, openAction});
    fileMenu->addSeparator();
    fileMenu->addActions({saveAction, saveAsAction});
    fileMenu->addSeparator();
    fileMenu->addActions({importAction, exportAction, exportImageAction});
    fileMenu->addSeparator();
    fileMenu->addAction(runScriptAction);
    recentScriptsMenu_ = fileMenu->addMenu("Rec&ent Scripts");
    recentScriptsMenu_->setObjectName("fileRecentScripts");
    refreshRecentScripts();
    fileMenu->addSeparator();
    fileMenu->addActions({plotAction, sheetsAction, plotSheetsAction});
    fileMenu->addSeparator();
    // The project's coordinate system: what online data, reprojection and the
    // title block need, set in one place (project_crs_dialog.hpp; CRS verb).
    QAction* crsAction = makeAction(Icon::Properties, "Project &Coordinate System...",
                                    "Set the coordinate system the project is in (EPSG code, WKT or "
                                    "PROJ); online data and reprojection need one",
                                    QKeySequence(), "fileProjectCrs");
    crsAction->setData(QStringLiteral("projectCrsDialog")); // for --dialog
    connect(crsAction, &QAction::triggered, this, [this] { showProjectCrs(); });
    fileMenu->addAction(crsAction);
    // The drawing at a glance, kept live beside it: what STATUS reports and
    // the customisation report (customisation/drawing_summary_dialog.hpp).
    QAction* summaryAction =
        makeAction(Icon::Properties, "Drawing S&ummary...",
                   "The drawing at a glance: project, contents, what is current, the history and "
                   "the customisation loaded, and what it leaves undefined",
                   QKeySequence(), "fileDrawingSummary");
    summaryAction->setData("drawingSummaryDialog");
    connect(summaryAction, &QAction::triggered, this, [this] { showDrawingSummary(); });
    fileMenu->addAction(summaryAction);
    fileMenu->addSeparator();
    QAction* quitAction =
        fileMenu->addAction("&Quit", QKeySequence::Quit, this, [this] { close(); });
    quitAction->setObjectName("fileQuit");

    QToolBar* fileBar = makeToolBar("File", Qt::TopToolBarArea);
    fileBar->addActions({newAction, openAction, saveAction});
    fileBar->addSeparator();
    fileBar->addActions({importAction, exportAction, plotAction, sheetsAction});

    // ---- Edit ------------------------------------------------------------------------
    undoAction_ = makeAction(Icon::Undo, "&Undo", "Undo the last command", QKeySequence::Undo,
                             "editUndo");
    redoAction_ = makeAction(Icon::Redo, "&Redo", "Redo the command that was undone",
                             QKeySequence::Redo, "editRedo");
    QAction* selectAllAction = makeAction(Icon::SelectAll, "Select &All",
                                          "Select every entity on an unlocked layer",
                                          QKeySequence::SelectAll, "editSelectAll");
    QAction* eraseAction = makeAction(Icon::Erase, "&Erase Selection", "Erase the selected entities",
                                      QKeySequence(Qt::Key_Delete), "editErase");
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
    // The selection erased at once, as Delete does in every CAD program: the
    // interpreter's ERASE, not the Erase tool a typed ERASE starts.
    connect(eraseAction, &QAction::triggered, this, [this] {
        commandLog_->appendPlainText("> ERASE");
        runInterpreterLine("ERASE", "ERASE");
    });

    editMenu->addActions({undoAction_, redoAction_});
    editMenu->addSeparator();
    editMenu->addAction(selectAllAction);
    QAction* deselectAction = editMenu->addAction("&Deselect", QKeySequence(Qt::Key_Escape), this,
                                                  [this] { views_->cancel(); });
    deselectAction->setObjectName("editDeselect");
    // The ids LIST, INFO and AREA print, back into a selection
    // (select_by_id_dialog.hpp); non-modal and kept, as Format > Layers is.
    QAction* selectByIdAction =
        editMenu->addAction("Select by &ID...", this, [this] { showSelectById(); });
    selectByIdAction->setObjectName("editSelectById");
    selectByIdAction->setStatusTip("Select entities by the ids LIST and INFO print, and find them");
    selectByIdAction->setData(QStringLiteral("selectByIdDialog")); // for --dialog
    editMenu->addAction(eraseAction);
    QAction* copyImageAction = editMenu->addAction(
        "Copy &View as Image", this, [this] { (void)runVerbLine("SNAPSHOT CLIPBOARD"); });
    copyImageAction->setObjectName("editCopyViewImage");
    copyImageAction->setStatusTip(
        "Copy the plan view, as it is on screen, to the clipboard as an image (SNAPSHOT CLIPBOARD)");
    editMenu->addSeparator();
    QAction* attributesAction =
        editMenu->addAction("A&ttributes...", QKeySequence(Qt::CTRL | Qt::Key_1), this, [this] {
            // Modal, like Layers: a headless run is told how to see it.
            if (headless_) {
                logMessage("Attributes: the dialog is modal and a headless run opens no modal "
                           "box; --attributes grabs it.",
                           true);
                return;
            }
            auto dialog = makeAttributeManager();
            dialog->expandAll();
            dialog->exec();
            // The panel shows the same properties, so it follows the dialog.
            refreshProperties();
        });
    attributesAction->setObjectName("editAttributes");

    // Format > Layers: beside the drawing, like the Format menu's other
    // managers. One instance, made the first time and kept, so it reopens on
    // the layer it showed; it asks nothing in boxes and reloads itself from
    // the Document, so it needs no refresh from here and a headless run can
    // open it too. The layers dock follows the Document as it always has.
    QAction* layersAction =
        makeAction(Icon::Layers, "&Layers...",
                   "The drawing's layers: colour, linetype, weight, what is on each",
                   QKeySequence(Qt::CTRL | Qt::Key_L), "formatLayers");
    connect(layersAction, &QAction::triggered, this, [this] {
        if (!layers_) {
            layers_ = makeLayerManager();
            // The action's name plus "Dialog", as the headless --dialog
            // looks a dialog up.
            layers_->setObjectName("formatLayersDialog");
            layers_->showFirstRow();
        }
        layers_->show();
        layers_->raise();
        layers_->activateWindow();
    });

    QToolBar* editBar = makeToolBar("Edit", Qt::TopToolBarArea);
    // Undo and Redo, each with its history dropped down beside it (the
    // button is one step; the k-th entry of the list is UNDO k or REDO k,
    // run through the one executor as one line). The menus are refilled
    // with the panels (refreshHistoryMenus).
    const auto historyButton = [this, editBar](QAction* action, const char* name,
                                               const char* menuName) {
        auto* button = new QToolButton(editBar);
        button->setObjectName(name);
        button->setDefaultAction(action);
        button->setPopupMode(QToolButton::MenuButtonPopup);
        button->setAutoRaise(true);
        button->setIconSize(editBar->iconSize());
        auto* menu = new QMenu(button);
        menu->setObjectName(menuName);
        button->setMenu(menu);
        editBar->addWidget(button);
        return menu;
    };
    undoHistory_ = historyButton(undoAction_, "editUndoButton", "editUndoMenu");
    redoHistory_ = historyButton(redoAction_, "editRedoButton", "editRedoMenu");
    editBar->addSeparator();
    editBar->addActions({selectAllAction, eraseAction});

    // ---- Properties: the style new work is drawn in (decision D9) -----------
    // Where CAD programs keep it: a choice on the properties toolbar beside
    // the drawing, read at a glance and changed without a dialog. ByLayer
    // (no style) is first, then the drawing's styles by name.
    QToolBar* propertiesBar = makeToolBar("Properties", Qt::TopToolBarArea);
    auto* currentStyleLabel = new QLabel(" Style ", propertiesBar);
    currentStyle_ = new QComboBox(propertiesBar);
    currentStyle_->setObjectName("CurrentStyleCombo");
    currentStyle_->setToolTip("<b>Current Style</b><br>The style everything drawn from now on "
                              "is given. ByLayer draws with the layer's linetype and colour.");
    currentStyle_->setAccessibleName("Current style");
    currentStyle_->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
    currentStyle_->setMinimumContentsLength(16);
    propertiesBar->addWidget(currentStyleLabel);
    propertiesBar->addWidget(currentStyle_);
    // currentIndexChanged, so a headless --fill sets it as a person's pick
    // does; the refresh that rebuilds the list is kept out by the flag. Not a
    // command: the current style is session state, like the current layer.
    connect(currentStyle_, &QComboBox::currentIndexChanged, this, [this](int index) {
        if (refreshingStyles_ || index < 0) {
            return;
        }
        const std::string name = currentStyle_->itemData(index).toString().toStdString();
        if (const auto status = document_.setCurrentStyle(name); !status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
            scheduleRefresh(); // show the style that is really current
            return;
        }
        logMessage("New work is drawn in " +
                   (name.empty() ? QString("ByLayer") : QString::fromStdString(name)) + ".");
    });

    // ---- View ------------------------------------------------------------------------
    QAction* extentsAction = makeAction(Icon::ZoomExtents, "Zoom &Extents",
                                        "Fit the whole drawing in the view",
                                        QKeySequence(Qt::CTRL | Qt::Key_E), "viewZoomExtents");
    connect(extentsAction, &QAction::triggered, this, [this] { views_->zoomExtents(); });
    gridAction_ = makeAction(Icon::Grid, "&Grid", "Show or hide the grid", QKeySequence(Qt::Key_F7),
                             "viewGrid");
    gridAction_->setCheckable(true);
    gridAction_->setChecked(views_->gridVisible());
    connect(gridAction_, &QAction::toggled, this, [this](bool on) { views_->setGridVisible(on); });
    snapAction_ = makeAction(Icon::Snap, "Object &Snap",
                             "Snap the cursor to endpoints, midpoints, centres and intersections",
                             QKeySequence(Qt::Key_F3), "viewSnap");
    snapAction_->setCheckable(true);
    snapAction_->setChecked(views_->snapEnabled());
    connect(snapAction_, &QAction::toggled, this, [this](bool on) { views_->setSnapEnabled(on); });

    viewMenu_->addAction(extentsAction);
    viewMenu_->addActions({gridAction_, snapAction_});
    // Plan-view lines as a cosmetic pixel instead of the 1.5 px hairline:
    // measured 5-8x cheaper to stroke (docs/plan_view.md), so on by default,
    // with the heavier look one click away. Plots are unaffected.
    QAction* thinLinesAction = viewMenu_->addAction("&Thin screen lines (faster)");
    thinLinesAction->setObjectName("ThinScreenLinesAction");
    thinLinesAction->setStatusTip(
        "Draw plan-view lines one pixel wide, which repaints large drawings several times faster");
    thinLinesAction->setCheckable(true);
    thinLinesAction->setChecked(ViewportWidget::thinScreenLines());
    connect(thinLinesAction, &QAction::toggled, this, [this](bool on) {
        ViewportWidget::setThinScreenLines(on);
        // Every plan view, docked or floating (a floating dock is still this
        // window's child). By dynamic_cast: the views have no Q_OBJECT.
        for (QWidget* widget : findChildren<QWidget*>()) {
            if (auto* plan = dynamic_cast<ViewportWidget*>(widget)) {
                plan->update();
            }
        }
    });

    QMenu* snapMenu = viewMenu_->addMenu("Snap &Modes");
    for (const cad::SnapMode mode :
         {cad::SnapMode::Endpoint, cad::SnapMode::Midpoint, cad::SnapMode::Center,
          cad::SnapMode::Intersection, cad::SnapMode::Perpendicular, cad::SnapMode::Tangent,
          cad::SnapMode::Nearest, cad::SnapMode::Grid}) {
        QAction* action = snapMenu->addAction(cad::toString(mode));
        // viewSnapModeEndpoint ... viewSnapModeGrid: what --trigger reaches
        // it by, and what SNAP <mode> ON|OFF sets (dispatchLine).
        action->setObjectName(QString("viewSnapMode") + cad::toString(mode));
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

    // ---- Draw, Modify, Annotate ----------------------------------------------------
    buildToolActions(*drawMenu, *modifyMenu, *annotateMenu);

    // ---- Format --------------------------------------------------------------------
    // Before Survey, whose Survey Coding section shows the code manager's
    // action too.
    buildFormatActions(*formatMenu, layersAction, customiseAction, replaceCustomisationAction);
    // Annotate > Edit Text...: the annotation workbench's, which exists only
    // from here, after the tools filled the menu.
    annotation_->addEditTextAction(*annotateMenu);
    // Annotate > Edit Label... and Label Layout Report..., after the
    // catalogue's tools: the annotation workbench's, which Format made.
    annotation_->addLabelActions(*annotateMenu);

    // ---- Survey ------------------------------------------------------------------------
    buildSurveyActions(*surveyMenu, customiseAction, replaceCustomisationAction, codeAction);

    // ---- Terrain and civil -----------------------------------------------------------
    QAction* cloudSurface = makeAction(Icon::SurfaceFromCloud, "Surface From &Point Cloud...",
                                       "Triangulate a surface from an imported point cloud's "
                                       "ground returns",
                                       {}, "surfaceFromPointCloud");
    QAction* rasterSurface = makeAction(Icon::SurfaceFromRaster, "Surface From &Raster...",
                                        "Triangulate a surface from an elevation raster's true "
                                        "values, read through GDAL",
                                        {}, "surfaceFromRaster");
    QAction* drawingSurface = makeAction(Icon::SurfaceFromDrawing, "Surface From &Drawing",
                                         "Triangulate a surface from the drawing's points and lines",
                                         {}, "surfaceFromDrawing");
    QAction* quantities = makeAction(Icon::CorridorQuantities, "Corridor &Quantities...",
                                     "Cut and fill along an alignment, by average end area",
                                     QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Q),
                                     "terrainCorridorQuantities");
    QAction* corridor = makeAction(Icon::CorridorSurface, "Corridor &Surface...",
                                   "Build the finished design along an alignment as a surface",
                                   {}, "terrainCorridorSurface");
    QAction* alignmentSection = makeAction(Icon::SectionAlignment, "Cut Section Along &Alignment...",
                                           "Long section down a named alignment, with its design profile",
                                           QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_K),
                                           "terrainSectionAlongAlignment");
    QAction* selectionSection = makeAction(Icon::Section, "&Cut Section Along Selection",
                                           "Long section along the selected line or polyline",
                                           QKeySequence(Qt::CTRL | Qt::Key_K),
                                           "terrainSectionAlongSelection");
    // The alignments a section and a corridor are cut along, defined and
    // edited in one window (alignment_manager.hpp) rather than only by ALIGN.
    QAction* alignmentManager =
        makeAction(Icon::SectionAlignment, "Alignment &Manager...",
                   "Define and edit alignments: PIs and curves, the design profile, the "
                   "setting-out table and chainage labels",
                   {}, "terrainAlignmentManager");
    // The dialog it shows, by object name: how --dialog finds it.
    alignmentManager->setData(QString("alignmentManagerDialog"));
    connect(alignmentManager, &QAction::triggered, this, [this] { showAlignmentManager(); });
    connect(cloudSurface, &QAction::triggered, this, [this] { buildSurfaceFromPointCloud(); });
    connect(rasterSurface, &QAction::triggered, this, [this] { buildSurfaceFromRaster(); });
    connect(drawingSurface, &QAction::triggered, this, [this] { buildSurfaceFromDrawing(); });
    connect(quantities, &QAction::triggered, this, &MainWindow::corridorQuantities);
    connect(corridor, &QAction::triggered, this, &MainWindow::corridorSurface);
    connect(alignmentSection, &QAction::triggered, this, &MainWindow::cutSectionAlongAlignment);
    connect(selectionSection, &QAction::triggered, this, [this] { cutSectionAlongSelection(); });

    // Sections rather than plain separators, as the Survey and GIS menus
    // have them; alignments first, since sections and corridors need one.
    terrainMenu->addSection("Alignments");
    terrainMenu->addAction(alignmentManager);
    terrainMenu->addSection("Surfaces");
    terrainMenu->addActions({cloudSurface, rasterSurface, drawingSurface});
    terrainMenu->addSection("Sections");
    terrainMenu->addActions({selectionSection, alignmentSection});
    terrainMenu->addSection("Corridors");
    terrainMenu->addActions({quantities, corridor});

    QToolBar* terrainBar = makeToolBar("Terrain", Qt::TopToolBarArea);
    terrainBar->addAction(alignmentManager);
    terrainBar->addSeparator();
    terrainBar->addActions({cloudSurface, rasterSurface, drawingSurface});
    terrainBar->addSeparator();
    terrainBar->addActions({selectionSection, alignmentSection});
    terrainBar->addSeparator();
    terrainBar->addActions({quantities, corridor});

    // ---- GIS ---------------------------------------------------------------------------
    buildGisActions(*gisMenu, exportAction);
    // The geoprocessing packages' items, GIS sections and Terrain submenus,
    // from their one table (geo/menu_table.cpp).
    GeoMenus geoMenus(*gisMenu, *terrainMenu);
    buildGeoMenus(geoMenus, *geo_);

    // ---- Help ------------------------------------------------------------------------
    // The reference and the shortcuts are dialogs of their own, non-modal and
    // kept (command_reference_dialog.hpp, keyboard_shortcuts_dialog.hpp);
    // each action's data names its dialog, as --dialog finds it.
    QAction* reference = makeAction(Icon::Help, "&Command Reference",
                                    "Every command the command line accepts, searchable",
                                    QKeySequence::HelpContents);
    reference->setData("commandReferenceDialog");
    connect(reference, &QAction::triggered, this, [this] { showCommandReference({}); });
    QAction* sheetCommands =
        makeAction(Icon::Help, "&Sheets and Plotting Commands",
                   "The sheet verbs and every option they take (HELP SHEETS)", QKeySequence(),
                   "helpSheetCommands");
    sheetCommands->setData("commandReferenceDialog");
    connect(sheetCommands, &QAction::triggered, this, [this] { showCommandReference("Sheets"); });
    QAction* shortcuts =
        makeAction(Icon::Help, "&Keyboard Shortcuts...",
                   "Every key the window answers to, where its command is and what it does",
                   QKeySequence(), "helpKeyboardShortcuts");
    shortcuts->setData("keyboardShortcutsDialog");
    connect(shortcuts, &QAction::triggered, this, [this] { showKeyboardShortcuts(); });
    QAction* about = makeAction(Icon::About, "&About Katana", "Version and build information");
    connect(about, &QAction::triggered, this, [this] {
        QMessageBox box(this);
        box.setWindowTitle("About Katana");
        box.setIconPixmap(QPixmap::fromImage(applicationIconImage(96)));
        box.setText(QString("<h3>Katana %1</h3>"
                            "<p>High-performance survey and CAD platform.</p>"
                            "<p style='color:%2'>Deterministic C++ engineering core, "
                            "Qt %3 desktop shell.</p>")
                        .arg(KATANA_VERSION, theme::textMuted().name(), qVersion()) +
                    // The two libraries every GIS import and export goes
                    // through, named with their versions: what a bug report
                    // about a file that will not open needs first.
                    QString("<p style='color:%1'>GDAL %2, PDAL %3.</p>")
                        .arg(theme::textMuted().name(),
                             QString::fromStdString(katana::gis::gdalVersion()),
                             QString::fromStdString(katana::pointcloud::pdalVersion())) +
                    // What this copy runs on this machine: the vector kernels
                    // in force (docs/performance.md, "Dispatch") and what
                    // draws a 3D view (docs/gpu.md, "Fallback") - the first
                    // two things to know about a report that something is slow.
                    QString("<p style='color:%1'>Vector kernels: %2. 3D views: %3.</p>")
                        .arg(theme::textMuted().name(), simdDescription(), rendererDescription()));
        box.exec();
    });
    reference->setObjectName("helpCommandReference");
    about->setObjectName("helpAbout");
    helpMenu->addActions({reference, sheetCommands, shortcuts});
    helpMenu->addSeparator();
    helpMenu->addAction(about);
}

// Every GDAL and PDAL capability the program has, in one menu, grouped by the
// library and the kind of data it is for. File > Import stays the quick way
// in - any file, default options, no questions; the imports here are the
// considered way, and describe the file and offer its options first.
void MainWindow::buildGisActions(QMenu& gisMenu, QAction* exportAction)
{
    QAction* importVector = makeAction(
        Icon::ImportVector, "Import Vector Da&ta...",
        "Import a Shapefile, GeoPackage, GeoJSON, KML, GML, DXF or MapInfo file through GDAL, "
        "choosing its layers and where they go",
        {}, "importVectorData");
    QAction* importRaster = makeAction(
        Icon::ImportRaster, "Import &Raster...",
        "Import a GeoTIFF, ASCII grid, IMG or image through GDAL, as a backdrop",
        {}, "importRaster");
    QAction* importCloud = makeAction(
        Icon::ImportPointCloud, "Import &Point Cloud...",
        "Import a LAS, LAZ or COPC point cloud through PDAL, choosing how many points and "
        "which classes",
        {}, "importPointCloud");
    QAction* exportCloud = makeAction(Icon::ExportPointCloud, "Export Point &Cloud...",
                                      "Write an imported point cloud to LAS or LAZ through PDAL",
                                      {}, "exportPointCloud");
    QAction* exportDem = makeAction(
        Icon::ExportDem, "Export Surface as &DEM...",
        "Write a surface to a GeoTIFF, ASCII grid or IMG elevation raster through GDAL",
        {}, "exportSurfaceDem");
    QAction* copc = makeAction(
        Icon::ConvertCopc, "Convert Point Cloud to C&OPC...",
        "Rewrite a LAS or LAZ as a Cloud Optimised Point Cloud, which can be read at any level "
        "of detail",
        {}, "convertCopc");
    QAction* info = makeAction(
        Icon::DatasetInfo, "Dataset &Information...",
        "What a GIS file or point cloud holds - layers, size, extent, coordinate system - "
        "without importing it",
        {}, "datasetInformation");
    connect(importVector, &QAction::triggered, this, [this] { importVectorWithOptions(); });
    connect(importRaster, &QAction::triggered, this, [this] { importRasterWithOptions(); });
    connect(importCloud, &QAction::triggered, this, [this] { importPointCloudWithOptions(); });
    connect(exportCloud, &QAction::triggered, this, [this] { exportPointCloud(); });
    connect(exportDem, &QAction::triggered, this, [this] { exportSurfaceAsDem(); });
    connect(copc, &QAction::triggered, this, [this] { convertPointCloudToCopc(); });
    connect(info, &QAction::triggered, this, [this] { showDatasetInformation(); });

    // Sections rather than plain separators: a style that draws their titles
    // says which library each group goes through, and one that does not draws
    // the separator it would have had anyway.
    gisMenu.addSection("Vector - GDAL");
    gisMenu.addActions({importVector, exportAction});
    gisMenu.addSection("Raster - GDAL");
    gisMenu.addActions({importRaster, exportDem});
    gisMenu.addSection("Point Cloud - PDAL");
    gisMenu.addActions({importCloud, exportCloud, copc});
    gisMenu.addSeparator();
    gisMenu.addAction(info);
    // Online - Web Services: its own section, filled by the workbench.
    OnlineServices online;
    online.document = &document_;
    online.views = views_;
    online.makeAction = [this](Icon icon, const QString& text, const QString& tip,
                               const QKeySequence& shortcut, const QString& name) {
        return makeAction(icon, text, tip, shortcut, name);
    };
    online.log = [this](const QString& text, bool isError) { logMessage(text, isError); };
    online.headless = [this] { return headless_; };
    online.addRaster = [this](interop::RasterOverlay raster) {
        const interop::ReferenceId id = reference_.add(std::move(raster));
        views_->invalidateReferenceCache();
        refreshReferences();
        views_->refreshAll();
        return id;
    };
    online.version = KATANA_VERSION;
    online.chooseProjectCrs = [this](std::optional<std::pair<double, double>> place) {
        // Online Data waits for the answer, to import into the system chosen.
        // A headless run has nobody to answer: it gets the non-modal dialog,
        // to fill by object name, and Online Data reads the system again when
        // it next refreshes.
        if (headless_) {
            showProjectCrs(place);
            return false;
        }
        return chooseProjectCrs(this, document_, place);
    };
    online_ = std::make_unique<OnlineDataWorkbench>(*this, std::move(online), gisMenu);

    // The geoprocessing verbs: the executor's context is this window's
    // drawing, interpreter (VIEW is its plan view), reference rasters and
    // surfaces.
    GeoServices geo;
    geo.document = &document_;
    geo.interpreter = &interpreter_;
    geo.reference = &reference_;
    geo.surfaces = &surfaceStore_;
    geo.scratch = katana::app::geo::defaultScratch();
    geo.log = [this](const QString& text, bool isError) { logMessage(text, isError); };
    geo.headless = [this] { return headless_; };
    geo.frame = [this](const katana::geometry::Box2& box) { views_->zoomTo(box); };
    geo.changed = [this] {
        views_->invalidateReferenceCache();
        refreshReferences();
        syncSceneSurfaces();
        views_->refreshAll();
    };
    geo.run = commandRunner();
    geo.makeAction = [this](Icon icon, const QString& text, const QString& tip,
                            const QKeySequence& shortcut, const QString& name) {
        return makeAction(icon, text, tip, shortcut, name);
    };
    geo_ = std::make_unique<GeoWorkbench>(*this, std::move(geo));
    // What only this window does with an IMPORT line: ask where data that
    // lands far from the drawing goes - nobody is asked headless - and show
    // a 12d archive's meshes, and its surfaces, in a 3D view.
    geo_->context().farApart = [this](const katana::geometry::Box2&,
                                      const katana::geometry::Box2& incoming,
                                      const std::string& advice) {
        using katana::app::geo::FarApartChoice;
        if (headless_) {
            return FarApartChoice::Keep;
        }
        switch (askFarApart(this, QString::fromStdString(advice), incoming)) {
        case FarApartAnswer::ShiftAlongside:
            return FarApartChoice::Alongside;
        case FarApartAnswer::Cancel:
            return FarApartChoice::Cancel;
        case FarApartAnswer::Keep:
            break;
        }
        return FarApartChoice::Keep;
    };
    geo_->context().imported = [this](katana::app::geo::ImportShown&& shown) {
        for (katana::archive12d::ImportedMesh& mesh : shown.meshes) {
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
        // A surface or a mesh is a 3D thing: in plan it is only a footprint,
        // so an import that brings one opens the 3D view - once, not once per
        // mesh (a real archive brings 1 453 of them).
        if (shown.surfaces != 0 || !shown.meshes.empty()) {
            views_->ensureView(cad::ViewKind::Model3D);
            refreshViewMenu();
            views_->refreshAll();
        }
    };

    QToolBar* gisBar = makeToolBar("GIS", Qt::TopToolBarArea);
    gisBar->addActions({importVector, importRaster, importCloud});
    gisBar->addSeparator();
    gisBar->addActions({exportCloud, exportDem, copc});
    gisBar->addSeparator();
    gisBar->addAction(info);
}

void MainWindow::buildSurveyActions(QMenu& surveyMenu, QAction* customiseAction,
                                    QAction* replaceCustomisationAction, QAction* codeAction)
{
    SurveyServices services;
    services.document = &document_;
    services.views = views_;
    services.chrome = chrome_;
    services.makeAction = [this](Icon icon, const QString& text, const QString& tip,
                                 const QKeySequence& shortcut, const QString& name) {
        return makeAction(icon, text, tip, shortcut, name);
    };
    services.log = [this](const QString& text, bool isError) { logMessage(text, isError); };
    services.loadCustomisation = customiseAction;
    services.replaceCustomisation = replaceCustomisationAction;
    services.applySurveyCodes = codeAction;
    services.codeManager = format_->codeManagerAction();
    services.run = commandRunner();
    services.headless = [this] { return headless_; };
    // A second row: the drawing's own toolbars (File to Format) fill the
    // first, and in one row the Survey, Terrain and GIS bars were squeezed
    // to a button each behind their overflow arrows.
    addToolBarBreak(Qt::TopToolBarArea);
    QToolBar* surveyBar = makeToolBar("Survey", Qt::TopToolBarArea);
    survey_ = std::make_unique<SurveyWorkbench>(*this, std::move(services), surveyMenu,
                                                *surveyBar);
    // Subsurface Utilities (AS 5488): its own section after Survey Coding,
    // filled by its workbench. The verb is the interpreter's; the dialog's
    // Run comes back through this window's command line.
    UtilityServices utilities;
    utilities.views = views_;
    utilities.document = &document_;
    utilities.makeAction = [this](Icon icon, const QString& text, const QString& tip,
                                  const QKeySequence& shortcut, const QString& name) {
        return makeAction(icon, text, tip, shortcut, name);
    };
    utilities.log = [this](const QString& text, bool isError) { logMessage(text, isError); };
    utilities.headless = [this] { return headless_; };
    utilities.interpret = [this](const std::string& line) {
        auto reply = interpreter_.run(line);
        historyCursor_ = static_cast<int>(interpreter_.history().size());
        return reply;
    };
    // The window's one executor: echoed as a typed line is and run as a
    // typed UTILITY line is - but never handed to a running tool first: a
    // tool waiting for a text's string would take any typed line for it, and
    // the dialog's line is never a text.
    utilities.run = commandRunner();
    utilities_ = std::make_unique<UtilityWorkbench>(*this, std::move(utilities), surveyMenu);
}

void MainWindow::buildFormatActions(QMenu& formatMenu, QAction* layersAction,
                                    QAction* customiseAction, QAction* replaceCustomisationAction)
{
    CustomisationServices services;
    services.document = &document_;
    services.views = views_;
    services.makeAction = [this](Icon icon, const QString& text, const QString& tip,
                                 const QKeySequence& shortcut, const QString& name) {
        return makeAction(icon, text, tip, shortcut, name);
    };
    services.log = [this](const QString& text, bool isError) { logMessage(text, isError); };
    services.headless = [this] { return headless_; };
    services.layers = layersAction;
    services.loadCustomisation = customiseAction;
    services.replaceCustomisation = replaceCustomisationAction;
    services.run = commandRunner();
    QToolBar* formatBar = makeToolBar("Format", Qt::TopToolBarArea);
    formatBar->addAction(layersAction);
    format_ = std::make_unique<CustomisationWorkbench>(*this, std::move(services), formatMenu,
                                                       *formatBar);
    annotation_ = std::make_unique<AnnotationWorkbench>(*this, document_, formatMenu, *formatBar);
    annotation_->setCommandRunner(commandRunner());
}

void MainWindow::buildToolActions(QMenu& drawMenu, QMenu& modifyMenu, QMenu& annotateMenu)
{
    // Beside the drawing, where AutoCAD's classic layout keeps them - Draw
    // and Annotate down the left edge, Modify down the right: they are the
    // tools reached for without looking, and a strip beside the drawing is a
    // shorter mouse journey than a row above it. Modify has its own edge
    // because one column cannot hold all three without hiding the last
    // tools behind an overflow arrow.
    QToolBar* drawBar = makeToolBar("Draw", Qt::LeftToolBarArea);
    QToolBar* annotateBar = makeToolBar("Annotate", Qt::LeftToolBarArea);
    QToolBar* modifyBar = makeToolBar("Modify", Qt::RightToolBarArea);

    // Select heads the Draw toolbar: not a catalogue tool but the absence
    // of one, so it stops whatever runs. Checked while nothing does.
    selectAction_ = makeAction(Icon::Select, "&Select",
                               "Pick entities, or drag a window or crossing box; stops the "
                               "running tool",
                               {}, "toolSelect");
    selectAction_->setCheckable(true);
    connect(selectAction_, &QAction::triggered, this, [this] {
        views_->stopTool();
        // A click on it while already selecting would untick it otherwise.
        selectAction_->setChecked(true);
    });
    drawBar->addAction(selectAction_);
    drawBar->addSeparator();

    tools::ToolMenuTargets targets;
    targets.menus = {{"Draw", &drawMenu}, {"Modify", &modifyMenu}, {"Annotate", &annotateMenu}};
    targets.toolBars = {{"Draw", drawBar}, {"Modify", modifyBar}, {"Annotate", annotateBar}};
    toolActions_ = tools::fillToolMenus(katana::cad::toolCatalog(), targets, this,
                                        [this](const std::string& id) { startTool(id); });
}

void MainWindow::startTool(const std::string& id)
{
    if (const auto started = views_->startTool(id); !started) {
        logMessage(QString::fromStdString(started.error().describe()), true);
        // A click on a tool's action checks it before this runs (a checkable
        // action in an exclusive group), and a refusal changes no tool, so
        // no onActiveToolChanged follows to put the marks right: without
        // this the refused tool stays checked beside Select, a tool shown
        // running that does not exist.
        showRunningTool(views_->activeToolId());
        return;
    }
    // The picks and the typed values go to the drawing from here on.
    if (ViewportWidget* plan = views_->activePlanView()) {
        plan->setFocus(Qt::OtherFocusReason);
    }
}

void MainWindow::showRunningTool(const std::string& id)
{
    toolActions_.setActive(id);
    selectAction_->setChecked(id.empty());
    // The prompt belongs in the command line, where the answer is typed;
    // with no tool running the line is the command line's own again.
    if (id.empty()) {
        commandInput_->setPlaceholderText(kCommandPlaceholder);
    }
}

katana::core::Status MainWindow::triggerAction(const QString& name)
{
    auto* action = findChild<QAction*>(name);
    if (action == nullptr) {
        return katana::core::makeError(katana::core::ErrorCode::NotFound, "no such menu item",
                                       name.toStdString());
    }
    if (!action->isEnabled()) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidState,
                                       "that menu item is disabled", name.toStdString());
    }
    action->trigger();
    return {};
}

QStringList MainWindow::shortcutClashes(int* sequences) const
{
    // Every key a person can press to reach something here, and what each
    // reaches: the actions' key sequences (all of them - Redo has two), any
    // QShortcut, the Alt letter of each top-level menu, and each item's
    // letter within its menu. A shared QAction is one object, so a menu and a
    // toolbar showing it are not a clash.
    std::map<QString, QStringList> owners;
    const auto nameOf = [](const QObject& object, QString text) {
        return object.objectName().isEmpty() ? text.remove('&') : object.objectName();
    };
    for (const QAction* action : findChildren<QAction*>()) {
        for (const QKeySequence& key : action->shortcuts()) {
            if (!key.isEmpty()) {
                owners[key.toString(QKeySequence::PortableText)] << nameOf(*action, action->text());
            }
        }
    }
    for (const QShortcut* shortcut : findChildren<QShortcut*>()) {
        if (!shortcut->key().isEmpty()) {
            owners[shortcut->key().toString(QKeySequence::PortableText)]
                << nameOf(*shortcut, "a shortcut");
        }
    }
    // The letter after a single '&' ("&&" is a literal ampersand), upper case;
    // empty for a text with none.
    const auto mnemonic = [](const QString& text) {
        for (qsizetype at = text.indexOf('&'); at >= 0 && at + 1 < text.size();
             at = text.indexOf('&', at + 2)) {
            if (text[at + 1] != '&') {
                return text.mid(at + 1, 1).toUpper();
            }
        }
        return QString();
    };
    // Inside an open menu its items' letters are keys too: two items with
    // one letter make the key cycle between them instead of choosing.
    const std::function<void(const QMenu&, const QString&)> letters =
        [&](const QMenu& menu, const QString& path) {
            for (const QAction* item : menu.actions()) {
                if (item->isSeparator() || !item->isVisible()) {
                    continue;
                }
                if (const QString letter = mnemonic(item->text()); !letter.isEmpty()) {
                    owners[path + " > " + letter] << QString(item->text()).remove('&');
                }
                if (const QMenu* sub = item->menu()) {
                    letters(*sub, path + " > " + QString(item->text()).remove('&'));
                }
            }
        };
    for (const QAction* top : menuBar()->actions()) {
        const QString title = QString(top->text()).remove('&');
        if (const QString letter = mnemonic(top->text()); !letter.isEmpty()) {
            owners["Alt+" + letter] << "the " + title + " menu";
        }
        if (const QMenu* menu = top->menu()) {
            letters(*menu, "Alt+" + mnemonic(top->text()) + " (" + title + ")");
        }
    }
    if (sequences != nullptr) {
        *sequences = static_cast<int>(owners.size());
    }
    QStringList clashes;
    for (const auto& [key, names] : owners) {
        if (names.size() > 1) {
            clashes << key + ": " + names.join(", ");
        }
    }
    return clashes;
}

std::vector<ShortcutRow> MainWindow::shortcutRows() const
{
    // The menus first, in their order, each key with the menu path it is
    // found under; then any action no menu shows, and the QShortcuts - the
    // keys shortcutClashes counts, so the table and the check agree.
    std::vector<ShortcutRow> rows;
    std::set<const QAction*> inMenus;
    const auto add = [&rows](const QAction& action, const QString& menu) {
        for (const QKeySequence& key : action.shortcuts()) {
            if (!key.isEmpty()) {
                rows.push_back({key.toString(QKeySequence::PortableText),
                                QString(action.text()).remove('&').remove("..."), menu,
                                action.statusTip()});
            }
        }
    };
    const std::function<void(const QMenu&, const QString&)> walk = [&](const QMenu& menu,
                                                                       const QString& path) {
        for (const QAction* item : menu.actions()) {
            if (item->isSeparator()) {
                continue;
            }
            if (const QMenu* sub = item->menu()) {
                walk(*sub, path + " > " + QString(item->text()).remove('&'));
                continue;
            }
            inMenus.insert(item);
            add(*item, path);
        }
    };
    for (const QAction* top : menuBar()->actions()) {
        if (const QMenu* menu = top->menu()) {
            walk(*menu, QString(top->text()).remove('&'));
        }
    }
    for (const QAction* action : findChildren<QAction*>()) {
        if (!inMenus.contains(action)) {
            add(*action, "(no menu)");
        }
    }
    for (const QShortcut* shortcut : findChildren<QShortcut*>()) {
        if (!shortcut->key().isEmpty()) {
            rows.push_back({shortcut->key().toString(QKeySequence::PortableText),
                            shortcut->objectName(), "(no menu)", shortcut->whatsThis()});
        }
    }
    return rows;
}

void MainWindow::showCommandReference(const QString& section)
{
    if (referenceDialog_ == nullptr) {
        // A double-click puts the verb on the command line, to be finished
        // there: the reference runs nothing.
        referenceDialog_ = new CommandReferenceDialog(
            commandReferenceSections(),
            [this](const QString& verb) {
                commandInput_->setText(verb);
                commandInput_->setFocus(Qt::OtherFocusReason);
            },
            this);
    }
    if (!section.isEmpty()) {
        referenceDialog_->showSection(section);
    }
    referenceDialog_->show();
    referenceDialog_->raise();
    referenceDialog_->activateWindow();
}

void MainWindow::showDrawingSummary()
{
    if (summaryDialog_ == nullptr) {
        DrawingSummaryContext context;
        context.document = &document_;
        context.customisation = [this] {
            return cad::customisationSummary(document_, customisation_,
                                             customisationMissingAtOpen_);
        };
        context.run = commandRunner();
        context.load = [this] { (void)triggerAction("loadCustomisation"); };
        context.showMissing = [this](const QString& name) { showMissingInStyles(name); };
        context.crs = [this] { return projectCrsLabel(document_); };
        summaryDialog_ = new DrawingSummaryDialog(std::move(context), this);
    }
    summaryDialog_->refresh();
    summaryDialog_->show();
    summaryDialog_->raise();
    summaryDialog_->activateWindow();
}

void MainWindow::showMissingInStyles(const QString& name)
{
    // The manager has no call for this, so it is reached as a person reaches
    // it - its Styles tab, its Missing chip, its search - by the object names
    // style_manager.hpp gives them.
    StyleManagerDialog& manager = format_->showStyleManager();
    auto* page = manager.findChild<QWidget*>("stylesPage");
    if (page == nullptr) {
        return;
    }
    if (auto* tabs = manager.findChild<QTabWidget*>("managerTabs")) {
        tabs->setCurrentWidget(page);
    }
    if (auto* missing = page->findChild<QAbstractButton*>("filterMissing")) {
        missing->click();
    }
    if (auto* search = page->findChild<QLineEdit*>("filterText")) {
        search->setText(name);
    }
}

void MainWindow::showKeyboardShortcuts()
{
    if (shortcutsDialog_ == nullptr) {
        shortcutsDialog_ = new KeyboardShortcutsDialog(shortcutRows(), shortcutClashes(), this);
    }
    shortcutsDialog_->show();
    shortcutsDialog_->raise();
    shortcutsDialog_->activateWindow();
}

void MainWindow::buildDocks()
{
    // The side columns run the full height of the window and the command
    // line sits under the drawing only, between them - the arrangement CAD
    // programs share (AutoCAD's command line docks under the drawing between
    // its palettes). The panels at the sides are trees and tables that read
    // downwards and use the height; the command line drives the drawing and
    // reads with it. Qt's default gives both bottom corners to the bottom
    // area, which ran the command line under both columns and cut them short.
    setCorner(Qt::BottomLeftCorner, Qt::LeftDockWidgetArea);
    setCorner(Qt::BottomRightCorner, Qt::RightDockWidgetArea);
    // Nested, so two panels can stand side by side within one column; tabbed,
    // so they can share one place. Not GroupedDragging, for the reason
    // ViewWorkspace gives: a panel dragged out must stay a dock with its own
    // title bar, not become a tab in a window of Qt's.
    setDockOptions(QMainWindow::AnimatedDocks | QMainWindow::AllowNestedDocks |
                   QMainWindow::AllowTabbedDocks);
    setTabPosition(Qt::AllDockWidgetAreas, QTabWidget::North);

    // ---- layers ----
    layerDock_ = new QDockWidget("Layers", this);
    QDockWidget* layerDock = layerDock_;
    auto* layerPanel = new QWidget(layerDock);
    auto* layerLayout = new QVBoxLayout(layerPanel);
    layerLayout->setContentsMargins(4, 4, 4, 4);
    layerLayout->setSpacing(4);
    auto* addButton = panelTool(layerPanel, Icon::LayerNew, "LayerNewButton", "New Layer",
                                "Add a layer at the top of the tree.");
    auto* childButton =
        panelTool(layerPanel, Icon::LayerNewChild, "LayerNewChildButton", "New Child Layer",
                  "Add a layer beneath the selected one.");
    auto* renameButton = panelTool(layerPanel, Icon::Rename, "LayerRenameButton", "Rename Layer",
                                   "Rename the selected layer and everything beneath it.");
    auto* deleteButton = panelTool(layerPanel, Icon::Erase, "LayerDeleteButton", "Delete Layer",
                                   "Delete the selected layer.");
    layerLayout->addLayout(toolRow({addButton, childButton, renameButton, deleteButton}));
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
    layerDock->setWidget(layerPanel);
    addDockWidget(Qt::LeftDockWidgetArea, layerDock);

    connect(addButton, &QToolButton::clicked, this, [this] { addLayer(); });
    connect(childButton, &QToolButton::clicked, this, [this] { addChildLayer(); });
    connect(renameButton, &QToolButton::clicked, this, [this] { renameSelectedLayer(); });
    connect(deleteButton, &QToolButton::clicked, this, [this] { deleteCurrentLayer(); });
    connect(layerTree_, &QTreeWidget::itemChanged, this,
            [this](QTreeWidgetItem* item, int column) { onLayerItemChanged(item, column); });
    connect(layerTree_, &QTreeWidget::itemDoubleClicked, this,
            [this](QTreeWidgetItem* item, int column) { onLayerItemDoubleClicked(item, column); });

    // ---- properties ----
    propertyDock_ = new QDockWidget("Properties", this);
    QDockWidget* propertyDock = propertyDock_;
    auto* propertyPanel = new QWidget(propertyDock);
    auto* propertyLayout = new QVBoxLayout(propertyPanel);
    propertyLayout->setContentsMargins(4, 4, 4, 4);
    propertyLayout->setSpacing(4);
    // The selection's style, as a form row above the read-only table: the
    // table shows, the row edits (docs/cad.md). EDITABLE, so a name the
    // drawing does not have - an imported entity's style that was never
    // defined - is shown as it is rather than replaced by the first choice,
    // and "<varies>" when the selection's entities differ. Applied by its
    // button or Enter, never by the box's own change signal.
    auto* styleRow = new QHBoxLayout();
    styleRow->setSpacing(4);
    auto* styleLabel = new QLabel("Style", propertyPanel);
    propertyStyle_ = new QComboBox(propertyPanel);
    propertyStyle_->setObjectName("PropertyStyle");
    propertyStyle_->setEditable(true);
    propertyStyle_->setInsertPolicy(QComboBox::NoInsert);
    propertyStyle_->setToolTip("<b>Style</b><br>The selection's style. Choose or type one and "
                               "Apply; ByLayer takes the layer's linetype and colour.");
    propertyStyle_->setAccessibleName("Style of the selection");
    styleLabel->setBuddy(propertyStyle_);
    propertyStyleApply_ = new QToolButton(propertyPanel);
    propertyStyleApply_->setObjectName("PropertyStyleApply");
    propertyStyleApply_->setText("Apply");
    propertyStyleApply_->setToolTip("<b>Apply Style</b><br>Give the selected entities this "
                                    "style, in one undoable step.");
    styleRow->addWidget(styleLabel);
    styleRow->addWidget(propertyStyle_, 1);
    styleRow->addWidget(propertyStyleApply_);
    propertyLayout->addLayout(styleRow);
    propertyTable_ = new QTableWidget(0, 2, propertyPanel);
    // Named, so the headless --report can read the rows (a label's Text and
    // Position, qt_labels_are_made_edited_and_reported_in_the_window_headless).
    propertyTable_->setObjectName("propertyTable");
    propertyTable_->setHorizontalHeaderLabels({"Property", "Value"});
    propertyTable_->verticalHeader()->setVisible(false);
    propertyTable_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    propertyTable_->horizontalHeader()->setSectionResizeMode(0, QHeaderView::ResizeToContents);
    propertyTable_->horizontalHeader()->setSectionResizeMode(1, QHeaderView::Stretch);
    propertyLayout->addWidget(propertyTable_);
    propertyDock->setWidget(propertyPanel);
    connect(propertyStyleApply_, &QToolButton::clicked, this, [this] { applyPropertyStyle(); });
    connect(propertyStyle_->lineEdit(), &QLineEdit::returnPressed, this,
            [this] { applyPropertyStyle(); });
    addDockWidget(Qt::RightDockWidgetArea, propertyDock);

    // ---- command line ----
    commandDock_ = new QDockWidget("Command Line", this);
    QDockWidget* commandDock = commandDock_;
    auto* commandPanel = new QWidget(commandDock);
    auto* commandLayout = new QVBoxLayout(commandPanel);
    commandLayout->setContentsMargins(4, 4, 4, 4);
    commandLog_ = new QPlainTextEdit(commandPanel);
    commandLog_->setObjectName("commandLog");
    commandLog_->setReadOnly(true);
    commandLog_->setMaximumBlockCount(2000);
    commandLog_->setFont(QFont("Consolas", 9));
    commandInput_ = new QLineEdit(commandPanel);
    commandInput_->setObjectName("commandInput");
    commandInput_->setFont(QFont("Consolas", 10));
    commandInput_->setPlaceholderText(kCommandPlaceholder);
    commandInput_->installEventFilter(this);
    commandLayout->addWidget(commandLog_);
    commandLayout->addWidget(commandInput_);
    commandDock->setWidget(commandPanel);
    addDockWidget(Qt::BottomDockWidgetArea, commandDock);
    connect(commandInput_, &QLineEdit::returnPressed, this, [this] { runCommandLine(); });

    buildReferenceDock();

    // Literal object names, not built from the titles: a saved layout finds a
    // dock by this name, and a title is prose that may be reworded or
    // translated ("Command LineDock", with its space, was what building them
    // gave).
    layerDock->setObjectName("LayersDock");
    propertyDock->setObjectName("PropertiesDock");
    commandDock->setObjectName("CommandLineDock");
    // The same title bar as every view, with Minimise, Float and Close.
    chrome_->install(layerDock, Icon::Layers, DockRole::Panel);
    chrome_->install(propertyDock, Icon::Properties, DockRole::Panel);
    chrome_->install(commandDock, Icon::CommandLine, DockRole::Panel);
    chrome_->install(referenceDock_, Icon::ReferenceData, DockRole::Panel);

    // Every panel can be closed, so every panel needs a way back. Qt makes the
    // toggle action; it only has to be put somewhere the user will look.
    layerDock->toggleViewAction()->setIcon(katana::qt::icon(Icon::Layers));
    propertyDock->toggleViewAction()->setIcon(katana::qt::icon(Icon::Properties));
    commandDock->toggleViewAction()->setIcon(katana::qt::icon(Icon::CommandLine));
    referenceDock_->toggleViewAction()->setIcon(katana::qt::icon(Icon::ReferenceData));
    // Named, as every View item is, since Qt names none of its toggles.
    layerDock->toggleViewAction()->setObjectName("viewPanelLayers");
    propertyDock->toggleViewAction()->setObjectName("viewPanelProperties");
    commandDock->toggleViewAction()->setObjectName("viewPanelCommandLine");
    referenceDock_->toggleViewAction()->setObjectName("viewPanelReferenceData");
    viewMenu_->addSeparator();
    QMenu* panels = viewMenu_->addMenu("&Panels");
    panels->addActions({layerDock->toggleViewAction(), propertyDock->toggleViewAction(),
                        commandDock->toggleViewAction(), referenceDock_->toggleViewAction()});

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
    layout->setSpacing(4);
    auto* importButton =
        panelTool(panel, Icon::Import, "ReferenceImportButton", "Import",
                  "Import a drawing, an image or a point cloud, as File > Import does.");
    auto* zoomButton = panelTool(panel, Icon::ZoomTo, "ReferenceZoomButton", "Zoom To",
                                 "Frame the selected reference layer in every plan view.");
    auto* infoButton = panelTool(panel, Icon::DatasetInfo, "ReferenceInfoButton", "Information",
                                 "What the selected layer's file holds, as GDAL or PDAL reads it.");
    auto* removeButton = panelTool(panel, Icon::Erase, "ReferenceRemoveButton", "Remove",
                                   "Remove the selected reference layer. The file is not touched.");
    layout->addLayout(toolRow({importButton, zoomButton, infoButton, removeButton}));

    referenceTable_ = new QTableWidget(0, 4, panel);
    referenceTable_->setHorizontalHeaderLabels({"Name", "Type", "Detail", "Display"});
    referenceTable_->horizontalHeader()->setStretchLastSection(true);
    referenceTable_->verticalHeader()->setVisible(false);
    referenceTable_->setSelectionBehavior(QAbstractItemView::SelectRows);
    referenceTable_->setSelectionMode(QAbstractItemView::SingleSelection);
    layout->addWidget(referenceTable_);

    dock->setWidget(panel);
    addDockWidget(Qt::LeftDockWidgetArea, dock);

    connect(importButton, &QToolButton::clicked, this, [this] { importFile(); });
    connect(zoomButton, &QToolButton::clicked, this, [this] { zoomToSelectedReference(); });
    connect(removeButton, &QToolButton::clicked, this, [this] { removeSelectedReference(); });
    // What the selected layer's SOURCE holds - which, for a cloud, is the
    // whole file and not the sample the panel shows.
    connect(infoButton, &QToolButton::clicked, this, [this] {
        const int row = referenceTable_->currentRow();
        const QTableWidgetItem* item = row < 0 ? nullptr : referenceTable_->item(row, kRefName);
        if (item == nullptr) {
            logMessage("Select a reference layer first.", true);
            return;
        }
        const auto id =
            static_cast<katana::interop::ReferenceId>(item->data(Qt::UserRole).toULongLong());
        std::filesystem::path source;
        if (const auto* raster = reference_.findRaster(id)) {
            source = raster->source;
        } else if (const auto* cloud = reference_.findPointCloud(id)) {
            source = cloud->source;
        }
        if (auto dialog = makeDatasetInfo(fromPath(source))) {
            dialog->exec();
        }
    });
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
    frameStatsLabel_ = new QLabel(this);
    frameStatsLabel_->setObjectName("FrameStatsLabel");
    // How many of how many are selected, always in view: the one count a
    // person acting on a selection needs before every command.
    selectionCountLabel_ = new QLabel(this);
    selectionCountLabel_->setObjectName("statusSelectionCount");
    selectionCountLabel_->setToolTip("Entities selected, of all the drawing holds");
    // The project's coordinate system, one click from changing it.
    crsButton_ = new QToolButton(this);
    crsButton_->setObjectName("statusProjectCrs");
    crsButton_->setAutoRaise(true);
    crsButton_->setToolTip("The project's coordinate system: click to change it");
    connect(crsButton_, &QToolButton::clicked, this, [this] { showProjectCrs(); });
    statusBar()->addPermanentWidget(crsButton_);
    statusBar()->addPermanentWidget(frameStatsLabel_);
    statusBar()->addPermanentWidget(selectionCountLabel_);
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
    // A box is not a logged error, but a dialog that ran the line must still
    // be told it failed, and why.
    if (capture_ != nullptr) {
        capture_->errors << title + ": " + text.simplified();
        capture_->failed = true;
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
    // Before the panels: a view's hidden layers that name no layer any more (a
    // rename, a delete, a New) would otherwise hide a later layer reusing the
    // name, and the listener cannot say which layer changed.
    views_->pruneViewLayers();
    refreshLayers();
    refreshProperties();
    refreshStyleChoices();
    refreshReferences();
    undoAction_->setEnabled(document_.history().canUndo());
    redoAction_->setEnabled(document_.history().canRedo());
    undoAction_->setText(document_.history().canUndo()
                             ? "&Undo " + QString::fromUtf8(document_.history().undoName().data(),
                                                            static_cast<int>(document_.history().undoName().size()))
                             : "&Undo");
    refreshHistoryMenus();
    layerLabel_->setText("Layer: " + QString::fromStdString(document_.currentLayer()));
    selectionCountLabel_->setText(QString("%1 selected / %2 entities")
                                      .arg(document_.selection().size())
                                      .arg(document_.model().entities.size()));
    if (crsButton_ != nullptr) {
        crsButton_->setText("CRS: " + projectCrsLabel(document_));
    }
}

void MainWindow::refreshHistoryMenus()
{
    // The steps each way, the next first, as far as a list can be read: a
    // longer history is said in a last line and reached by typing UNDO n.
    constexpr std::size_t kListed = 25;
    // `which` says where the k steps are: the last ones for Undo, the next
    // for Redo - whose list once said "the last k steps" as Undo's does.
    const auto fill = [this](QMenu& menu, const std::vector<std::string_view>& names,
                             const QString& verb, const QString& itemName,
                             const QString& which) {
        menu.clear();
        for (std::size_t k = 0; k < names.size() && k < kListed; ++k) {
            const QString name =
                QString::fromUtf8(names[k].data(), static_cast<qsizetype>(names[k].size()));
            // '&&' so a name's own '&' is shown, not taken for a mnemonic.
            QAction* item = menu.addAction(QString("%1  %2").arg(k + 1).arg(
                QString(name).replace("&", "&&")));
            item->setObjectName(itemName + QString::number(k + 1));
            const QString line = QString("%1 %2").arg(verb).arg(k + 1);
            item->setStatusTip(k == 0 ? QString("%1: %2").arg(line, name)
                                      : QString("%1: the %2 %3 steps, down to %4")
                                            .arg(line, which)
                                            .arg(k + 1)
                                            .arg(name));
            connect(item, &QAction::triggered, this, [this, line] { (void)runVerbLine(line); });
        }
        if (names.size() > kListed) {
            menu.addAction(QString("%1 more: type %2 n").arg(names.size() - kListed).arg(verb))
                ->setEnabled(false);
        }
    };
    fill(*undoHistory_, document_.history().undoNames(), "UNDO", "undoStep", "last");
    fill(*redoHistory_, document_.history().redoNames(), "REDO", "redoStep", "next");
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
        for (auto& row : describeGeometry(document_.model(), entity.geometry)) {
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

void MainWindow::refreshStyleChoices()
{
    refreshingStyles_ = true;
    const std::vector<std::string> names = document_.model().styles.names();

    currentStyle_->clear();
    currentStyle_->addItem("ByLayer", QString());
    for (const std::string& name : names) {
        currentStyle_->addItem(QString::fromStdString(name), QString::fromStdString(name));
    }
    const std::string& current = document_.currentStyle();
    int index = current.empty() ? 0 : currentStyle_->findData(QString::fromStdString(current));
    if (index < 0) {
        // Never dropped for the first choice (the QT-02 lesson): a current
        // style the list does not hold is shown, and marked.
        currentStyle_->addItem(QString::fromStdString(current) + "  (not in the drawing)",
                               QString::fromStdString(current));
        index = currentStyle_->count() - 1;
    }
    currentStyle_->setCurrentIndex(index);

    const QSignalBlocker blocker(propertyStyle_);
    propertyStyle_->clear();
    propertyStyle_->addItem("ByLayer");
    for (const std::string& name : names) {
        propertyStyle_->addItem(QString::fromStdString(name));
    }
    std::set<std::string> styles;
    for (const auto id : document_.selection().ids()) {
        if (const Entity* entity = document_.model().entities.find(id)) {
            styles.insert(entity->style);
        }
    }
    const bool any = !styles.empty();
    propertyStyle_->setEnabled(any);
    propertyStyleApply_->setEnabled(any);
    // The text is set, not an index chosen: an entity's style that is in
    // no list is shown as it is.
    if (styles.size() == 1) {
        const std::string& style = *styles.begin();
        propertyStyle_->setEditText(style.empty() ? QString("ByLayer")
                                                  : QString::fromStdString(style));
    } else {
        propertyStyle_->setEditText(QString());
    }
    propertyStyle_->lineEdit()->setPlaceholderText(styles.size() > 1 ? "<varies>" : QString());
    refreshingStyles_ = false;
}

void MainWindow::applyPropertyStyle()
{
    const auto ids = document_.selection().ids();
    if (ids.empty()) {
        logMessage("Style: nothing is selected.", true);
        return;
    }
    const QString text = propertyStyle_->currentText().trimmed();
    if (text.isEmpty()) {
        logMessage("Style: the selection's styles vary; choose one, or ByLayer, to give them all.",
                   true);
        return;
    }
    // Names are case-sensitive (D3), so only "ByLayer" itself means none.
    const std::string style = text == "ByLayer" ? std::string{} : text.toStdString();
    // Only the entities that change: applying the style already shown is
    // then no undo step at all, rather than one that changes nothing.
    std::vector<katana::entity::EntityId> changing;
    for (const auto id : ids) {
        if (const Entity* entity = document_.model().entities.find(id);
            entity != nullptr && entity->style != style) {
            changing.push_back(id);
        }
    }
    if (changing.empty()) {
        logMessage("Style: the selection is already in " + text + ".");
        return;
    }
    const std::size_t count = changing.size();
    if (const auto status =
            document_.execute(katana::commands::setEntityStyle(std::move(changing), style));
        !status) {
        logMessage("Style: " + QString::fromStdString(status.error().describe()), true);
        return;
    }
    logMessage("Style " + text + " given to " + grouped(count) +
               (count == 1 ? " entity." : " entities."));
}

// ---- file handling ----------------------------------------------------------------------------

bool MainWindow::confirmDiscard()
{
    if (!document_.isModified()) {
        return true;
    }
    // A headless run has nobody to answer the box, which would wait for ever
    // (a scripted NEW after an edit hung to the test's timeout). Refused and
    // said, never discarded unasked: the script can SAVE or UNDO first.
    if (headless_) {
        logMessage("Unsaved Changes: the drawing has unsaved changes, and a headless run has "
                   "nobody to ask whether to discard them; SAVE or UNDO first.",
                   true);
        return false;
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
        clearReferenceData();
        clearSceneData();
        views_->zoomExtentsAll();
        logMessage("New drawing.");
    }
}

bool MainWindow::refuseFileDialog(const QString& verb)
{
    // A file dialog nobody can close is a hang, and --trigger reaches these
    // items by their names: pointed at the verb that asks nothing instead,
    // as the COPC item is.
    if (!headless_) {
        return false;
    }
    logMessage("A headless session opens no file dialog: type " + verb + " instead.", true);
    return true;
}

void MainWindow::openDocument()
{
    if (!confirmDiscard() || refuseFileDialog("OPEN <directory>")) {
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

void MainWindow::showPlanContextMenu(const QPoint& globalPos)
{
    PlanContextMenuContext context;
    context.document = &document_;
    context.actions = this;
    context.run = commandRunner();
    context.lastToolId = views_->lastToolId();
    context.chooseColour = [this](const QColor& initial) -> std::optional<QColor> {
        const QColor chosen = QColorDialog::getColor(initial, this, "Colour");
        return chosen.isValid() ? std::optional<QColor>(chosen) : std::nullopt;
    };
    // popup, not exec: nothing waits on it. It goes when it hides, by
    // deleteLater: a menu hides BEFORE it triggers the item chosen, and the
    // deferred delete waits for the item to finish - even for a colour
    // dialog's own event loop, which does not run it.
    auto* menu = new PlanContextMenu(std::move(context), this);
    connect(menu, &QMenu::aboutToHide, menu, &QObject::deleteLater);
    menu->popup(globalPos);
}

void MainWindow::showProjectCrs(std::optional<std::pair<double, double>> place)
{
    // Made afresh unless it is open: the dialog reads the project's system
    // and the drawing's centre when it is made, and a place asked about is a
    // new question.
    if (!projectCrs_ || !projectCrs_->isVisible() || place) {
        projectCrs_ = std::make_unique<ProjectCrsDialog>(document_, place, this);
    }
    projectCrs_->show();
    projectCrs_->raise();
    projectCrs_->activateWindow();
}

void MainWindow::showSelectById()
{
    if (!selectById_) {
        SelectByIdContext context;
        context.document = &document_;
        context.run = commandRunner();
        context.onSelected = [this](bool zoom) { showSelection(zoom); };
        selectById_ = std::make_unique<SelectByIdDialog>(std::move(context), this);
    }
    selectById_->show();
    selectById_->raise();
    selectById_->activateWindow();
}

void MainWindow::showSelection(bool zoom)
{
    // Framed in the view the person is working in, as the style manager's
    // Select Users frames what a style covers; the other views keep theirs.
    ViewportWidget* plan = views_->activePlanView();
    if (zoom && plan != nullptr) {
        katana::geometry::Box2 bounds;
        for (const katana::entity::EntityId id : document_.selection().ids()) {
            if (const katana::entity::Entity* entity = document_.model().entities.find(id)) {
                bounds.expand(katana::entity::boundingBox(entity->geometry));
            }
        }
        plan->zoomTo(bounds);
    }
    propertyDock_->show();
    propertyDock_->raise();
}

void MainWindow::editDoubleClicked(katana::entity::EntityId id)
{
    const katana::entity::Entity* entity = document_.model().entities.find(id);
    if (entity == nullptr) {
        return;
    }
    // The first click selected it; a Shift or Ctrl first click may have
    // added it to more, and the editors act on what is selected.
    if (document_.selection().size() != 1 || !document_.selection().contains(id)) {
        document_.selection().set({id});
        document_.notifySelectionChanged();
    }
    // A text or a label opens its own editor where the window has one
    // (Annotate > Edit Text, Edit Label); anything else is edited in
    // the Properties panel.
    const char* editor = entity->type() == katana::entity::EntityType::Text    ? "annotateEditText"
                         : entity->type() == katana::entity::EntityType::Label ? "annotateEditLabel"
                                                                               : nullptr;
    if (editor != nullptr) {
        if (auto* action = findChild<QAction*>(QString::fromLatin1(editor));
            action != nullptr && action->isEnabled()) {
            action->trigger();
            return;
        }
    }
    showSelection(false);
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
    // The Format menu's context: its picture cache, and "Select Users"
    // framing what they cover in the active plan view.
    return std::make_unique<StyleManagerDialog>(format_->context(), this);
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
    clearReferenceData();
    clearSceneData();
    views_->zoomExtentsAll();
    logMessage("Opened " + directory + " (" +
               QString::number(document_.model().entities.size()) + " entities).");
    reportMissingCustomisation();
}

bool MainWindow::saveDocument()
{
    if (!document_.hasProject()) {
        return saveDocumentAs();
    }
    recordCustomisation();
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
    if (refuseFileDialog("SAVE <directory>")) {
        return false;
    }
    QString target = QFileDialog::getSaveFileName(this, "Save Project As", "untitled.katana",
                                                  "Katana project (*.katana)");
    if (target.isEmpty()) {
        return false;
    }
    if (!target.endsWith(".katana", Qt::CaseInsensitive)) {
        target += ".katana";
    }
    recordCustomisation();
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
    // The Format managers first: the code manager's Apply puts its rules on
    // the drawing, which the unsaved-drawing question must then see.
    if (!format_->confirmClose()) {
        event->ignore();
        return;
    }
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
    const QString line = isError ? "! " + text : text;
    commandLog_->appendPlainText(line);
    if (capture_ != nullptr) {
        (isError ? capture_->errors : capture_->reply) << text;
    }
    if (isError) {
        ++errorsLogged_;
        statusBar()->showMessage(text, 6000);
    }
    // A headless run has no window to read the log in. Echoed, it is what a
    // script - or a ctest check - can see of what a command reported.
    if (headless_) {
        std::fprintf(stderr, "%s\n", line.toUtf8().constData());
    }
}

bool MainWindow::runCommand(const QString& line)
{
    const int errors = errorsLogged_;
    commandInput_->setText(line);
    runCommandLine();
    return errorsLogged_ == errors;
}

void MainWindow::runCommandLine()
{
    const QString line = commandInput_->text().trimmed();
    commandInput_->clear();
    if (line.isEmpty()) {
        // Enter on an empty command line is Enter in the drawing, as in
        // AutoCAD: the running tool's (end a chain, take a default) or, with
        // none running, the last tool again.
        views_->pressEnter();
        return;
    }
    // Several lines at once - pasted, or a --command holding line breaks -
    // are a script, run a line at a time and stopping at the first refused
    // (runPastedLines). A single-line field shows the breaks as blanks, and
    // the whole was once run as one line of nonsense. But pasted at a prompt
    // for text - a Text's or a Multiline Text's lines - they are that text:
    // run as a script, a label's words became commands and a RECT was drawn.
    if (line.contains('\n') || line.contains('\r')) {
        if (views_->toolTakesText()) {
            typeLinesIntoTool(line);
        } else {
            runPastedLines(line);
        }
        return;
    }
    runTypedLine(line);
}

void MainWindow::typeLinesIntoTool(const QString& text)
{
    static const QRegularExpression kLineBreak(QStringLiteral("\r\n|\r|\n"));
    // A blank line is left out rather than typed: to a Text it is the Enter
    // that finishes it, and a paragraph break in what was copied is not the
    // person pressing Enter. Each other line goes through the typed path, so
    // one the tool no longer wants as text (it took an option's word and
    // asks for a point) is what typing it would have been.
    for (const QString& each : text.split(kLineBreak)) {
        const QString line = each.trimmed();
        if (!line.isEmpty()) {
            runTypedLine(line);
        }
    }
}

void MainWindow::runTypedLine(const QString& line)
{
    // What is typed is echoed - but an ONLINE KEY's value never is.
    commandLog_->appendPlainText("> " + OnlineDataWorkbench::loggedLine(line));
    // A tool waiting for typed text - a Text's string, a count - takes the
    // whole line before any verb below, as a transparent ZOOM gives way to it
    // (tools::isTransparentCommand): "Utility pit" is a label on a services
    // plan, not a UTILITY line to refuse. Otherwise the workbenches' verbs
    // come before a running tool at a point or a pick, which would take the
    // line for an answer - and so does a '#' comment, as in a katana_cli
    // script, which a Text's string ("#3 pit") may well start with.
    if (!views_->toolTakesText() && (isScriptComment(line) || runWorkbenchLine(line))) {
        return;
    }
    // While a tool runs, what is typed is its answer - a point, a distance,
    // an option - before it is anything else: Polyline's C closes it, where
    // on its own C would start a Circle.
    if (views_->typeIntoTool(line)) {
        return;
    }
    dispatchLine(line, LineSource::Typed);
}

bool MainWindow::runWorkbenchLine(const QString& line)
{
    // GDAL and the geoprocessing families after it: the executor katana_cli
    // and katana_mcp share, run here as background jobs.
    if (geo_ != nullptr && geo_->runLine(line)) {
        return true;
    }
    // ONLINE PROVIDERS, LAYERS, INFO, IMPORT, CUSTOM, KEY: the online
    // workbench's, as the interoperability verbs are the window's.
    if (online_ != nullptr && online_->runLine(line)) {
        return true;
    }
    // UTILITY REPORT, VERIFY, CLEARANCE, CHECK, DRAW, REGRADE, SCHEDULE: the
    // interpreter's verb, through the utilities workbench, which frames what a
    // DRAW or a REGRADE drew.
    return utilities_ != nullptr && utilities_->runLine(line);
}

VerbOutcome MainWindow::runVerbLine(const QString& line)
{
    const QString trimmed = line.trimmed();
    if (trimmed.isEmpty()) {
        // An empty TYPED line is Enter in the drawing; a dialog has no Enter
        // to press, so for it an empty line is a line it failed to build.
        return {false, {}, "there is no command to run"};
    }
    commandLog_->appendPlainText("> " + OnlineDataWorkbench::loggedLine(trimmed));
    VerbCapture capture;
    VerbCapture* const outer = std::exchange(capture_, &capture);
    const int errorsBefore = errorsLogged_;
    // Never typeIntoTool: that is the whole difference from a typed line.
    if (!runWorkbenchLine(trimmed)) {
        dispatchLine(trimmed, LineSource::Executor);
    }
    capture_ = outer;
    // A line run inside another's run - a script's, under the SCRIPT line a
    // dialog ran - logged into the outer line's reply too: the dialog that
    // ran the SCRIPT line gets back everything its lines said.
    if (outer != nullptr) {
        outer->reply << capture.reply;
        outer->errors << capture.errors;
        outer->failed = outer->failed || capture.failed;
    }
    VerbOutcome outcome;
    // By the errors COUNTED, not the lines captured: PLOTSHEETS logs the
    // problems of a plot it carried out and takes them off the count, and a
    // --command run judges the same line the same way (runCommand).
    outcome.ok = errorsLogged_ == errorsBefore && !capture.failed;
    outcome.reply = capture.reply.join('\n');
    outcome.error = capture.errors.join('\n');
    return outcome;
}

CommandRunner MainWindow::commandRunner()
{
    return [this](const QString& line) { return runVerbLine(line); };
}

// The verbs taken here before the interpreter are listed for people by
// windowHelpText (command_reference_dialog.cpp): what the typed HELP adds and
// the Command Reference's Window section. A verb added here is added there.
void MainWindow::dispatchLine(const QString& line, LineSource source)
{
    // A note, as a katana_cli script has them: a dialog's or a script's line
    // comes here without passing runCommandLine's check.
    if (isScriptComment(line)) {
        return;
    }
    const QStringList words = line.split(' ', Qt::SkipEmptyParts);
    const QString verb = words.front().toUpper();
    const QString argument = words.size() > 1 ? words[1].toUpper() : QString();

    // Commands that concern the view rather than the document.
    if (verb == "ZOOM" || verb == "Z") {
        views_->zoomExtents();
        return;
    }
    // ON, OFF, or nothing to toggle - and anything else refused: every other
    // word once meant OFF, so SNAP ENDPOINT OFF turned object snap off
    // altogether and said nothing.
    const auto onOff = [](const QString& word, bool now) -> std::optional<bool> {
        if (word.isEmpty()) {
            return !now;
        }
        if (word == "ON" || word == "OFF") {
            return word == "ON";
        }
        return std::nullopt;
    };
    if (verb == "GRID") {
        const auto on =
            words.size() <= 2 ? onOff(argument, gridAction_->isChecked()) : std::nullopt;
        if (!on) {
            logMessage("usage: GRID [ON|OFF]", true);
            return;
        }
        gridAction_->setChecked(*on);
        views_->setGridVisible(*on);
        logMessage(QString("grid=%1").arg(*on ? "on" : "off"));
        return;
    }
    if (verb == "SNAP" || verb == "OSNAP") {
        // SNAP <mode> [ON|OFF]: one of View > Snap Modes, by its name there.
        const QString mode = argument == "CENTRE" ? QString("CENTER") : argument;
        for (QAction* each : findChildren<QAction*>()) {
            if (!mode.isEmpty() && each->objectName().startsWith("viewSnapMode") &&
                each->objectName().mid(12).toUpper() == mode) {
                const auto on = words.size() <= 3
                                    ? onOff(words.size() == 3 ? words[2].toUpper() : QString(),
                                            each->isChecked())
                                    : std::nullopt;
                if (!on) {
                    logMessage("usage: SNAP <mode> [ON|OFF]", true);
                    return;
                }
                each->setChecked(*on); // toggled: the views take the modes
                logMessage(QString("snap_mode=%1 state=%2")
                               .arg(each->objectName().mid(12).toLower(), *on ? "on" : "off"));
                return;
            }
        }
        const auto on =
            words.size() <= 2 ? onOff(argument, snapAction_->isChecked()) : std::nullopt;
        if (!on) {
            logMessage("usage: SNAP [ON|OFF] | SNAP <mode> [ON|OFF]   modes: Endpoint, Midpoint, "
                       "Center, Intersection, Perpendicular, Tangent, Nearest, Grid",
                       true);
            return;
        }
        snapAction_->setChecked(*on);
        views_->setSnapEnabled(*on);
        logMessage(QString("snap=%1").arg(*on ? "on" : "off"));
        return;
    }
    // EXAGGERATION [factor]: View > Vertical Exaggeration's line, and alone
    // the factor now, as a record.
    if (verb == "EXAGGERATION") {
        if (words.size() == 1) {
            logMessage("vertical_exaggeration=" +
                       QString::fromStdString(katana::core::formatExactReal(
                           views_->sceneOptions().verticalExaggeration)));
            return;
        }
        const auto factor = words.size() == 2
                                ? katana::core::parseFiniteDouble(words[1].toStdString())
                                : std::nullopt;
        if (!factor || *factor < kLeastExaggeration || *factor > kMostExaggeration) {
            logMessage("usage: EXAGGERATION [factor]   elevations times 0.01 to 1000", true);
            return;
        }
        setVerticalExaggeration(*factor);
        return;
    }
    if (verb == "QUIT" || verb == "EXIT") {
        close();
        return;
    }
    // SCRIPT <file> [CONTINUE]: a katana_cli script, each line run through
    // the one executor (script_runner.hpp). The window's, as PLOTSHEETS is:
    // katana_cli runs a script given on its command line, and katana_mcp has
    // katana_run_script.
    if (verb == "SCRIPT") {
        const auto command = parseScriptCommand(line);
        if (!command) {
            logMessage(QString::fromStdString(command.error().describe()), true);
            return;
        }
        runScript(command->path, command->continueOnError);
        return;
    }
    // CUSTOMISE [REPLACE] <file>..., as katana_cli has it and for the same
    // reason (katana_cad may not see the library readers): the files are merged
    // into what is loaded, or with REPLACE - an unquoted first word - take
    // the place of the kinds they bring. A quoted path may hold spaces.
    if (verb == "CUSTOMISE" || verb == "CUSTOMIZE") {
        const QString rest = line.mid(words.front().size());
        std::vector<std::filesystem::path> paths;
        bool replace = false;
        qsizetype at = 0;
        while (at < rest.size()) {
            if (rest[at].isSpace()) {
                ++at;
                continue;
            }
            if (rest[at] == '"') {
                const qsizetype end = rest.indexOf('"', at + 1);
                if (end < 0) {
                    logMessage("CUSTOMISE: a quoted path is never closed", true);
                    return;
                }
                paths.push_back(toPath(rest.mid(at + 1, end - at - 1)));
                at = end + 1;
                continue;
            }
            qsizetype end = at;
            while (end < rest.size() && !rest[end].isSpace()) {
                ++end;
            }
            const QString word = rest.mid(at, end - at);
            at = end;
            if (paths.empty() && !replace && word.toUpper() == "REPLACE") {
                replace = true;
                continue;
            }
            paths.push_back(toPath(word));
        }
        if (paths.empty() && replace) {
            logMessage("usage: CUSTOMISE [REPLACE] <file> [<file>...]", true);
            return;
        }
        // Alone, it reports what is loaded and what it covers here, in the
        // words katana_cli's CUSTOMISE says them (cad/customisation_report.hpp).
        if (paths.empty()) {
            logMessage(QString::fromStdString(katana::cad::customisationReport(
                                                  document_, customisation_,
                                                  customisationMissingAtOpen_))
                           .trimmed());
            return;
        }
        applyCustomisation(paths, replace ? katana::archive12d::LoadMode::Replace
                                          : katana::archive12d::LoadMode::Merge);
        return;
    }
    // IMPORT, EXPORT, INFO <file>, REFS and COPC are not here: they are the
    // executor katana_cli and katana_mcp run (src/katana_app/geo), which the
    // geo workbench runs as jobs (runWorkbenchLine). INFO <id> is left to the
    // interpreter by the executor itself (geo::takesInfo).

    // PLOTSHEETS [path] [format=] [style=] [sheets=] [dpi=] [lineweight=]
    // [folder=] [pattern=]: the sheets plotted as the Plot dialog and
    // --plot-sheets plot them, the set's page setup filling in what is not
    // given. Here and not in the interpreter because the painter is Qt; the
    // line is read by the interpreter's own rules (sheet_verbs.hpp,
    // parsePlotSheets). Each file written is logged on a line of its own,
    // file="path", after the summary, for a script to pick up.
    if (verb == "PLOTSHEETS") {
        const auto tokens = cad::CommandInterpreter::tokenize(line.toStdString());
        if (!tokens) {
            logMessage(QString::fromStdString(tokens.error().describe()), true);
            return;
        }
        // A project with no sheets plots one fitted to the drawing, so
        // sheets= is read against that one sheet.
        const auto set = sheetsToPlot();
        if (!set) {
            logMessage(QString::fromStdString(set.error().describe()), true);
            return;
        }
        const auto parsed = cad::plotting::parsePlotSheets(
            *set, std::vector<std::string>(tokens->begin() + 1, tokens->end()));
        if (!parsed) {
            logMessage(QString::fromStdString(parsed.error().describe()), true);
            return;
        }
        const PlotRequest request = plotRequestFor(set->pageSetup, *parsed);
        // What cannot be drawn on a sheet is logged as a problem, and the
        // files are still written: the line was carried out, not refused, so
        // those do not count against it (runCommand), as they do not fail
        // --plot-sheets.
        const int errorsBefore = errorsLogged_;
        const auto result = plotSheets(*set, request);
        if (!result) {
            logMessage(QString::fromStdString(result.error().describe()), true);
            return;
        }
        for (const QString& file : result->files) {
            logMessage(QString("file=\"%1\"").arg(QDir::toNativeSeparators(file)));
        }
        errorsLogged_ = errorsBefore;
        return;
    }
    // PLOT <file.pdf> [paper=] [landscape|portrait] [fit|scale=] [dpi=]
    // [style=] [lineweight=] [margin=]: the drawing on one sheet, as File >
    // Plot to PDF and --plot plot it (plotting/plot_drawing_dialog.hpp). The
    // window's, as PLOTSHEETS is: the painter is Qt.
    if (verb == "PLOT") {
        const auto request = parsePlotDrawing(line);
        if (!request) {
            logMessage(QString::fromStdString(request.error().describe()), true);
            return;
        }
        if (const auto status = plotDrawingToPdf(request->path, request->settings, request->fit);
            !status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
        }
        return;
    }
    // SNAPSHOT <file> | CLIPBOARD [width=] [height=] [scale=] [bg=] [view=]:
    // a picture of the plan or 3D view (plotting/view_image_export.hpp).
    if (verb == "SNAPSHOT") {
        const auto request = parseSnapshot(line);
        if (!request) {
            logMessage(QString::fromStdString(request.error().describe()), true);
            return;
        }
        snapshotView(*request);
        return;
    }
    // HELP (or ?) alone: the interpreter's commands, then the verbs this
    // front end runs itself, which the interpreter cannot know of. With a
    // word, HELP SHEETS and HELP UTILITY stay the interpreter's.
    if ((verb == "HELP" || verb == "?") && words.size() == 1) {
        runInterpreterLine(line, verb);
        logMessage(QString::fromStdString(katana::app::geo::helpText()));
        logMessage(windowHelpText());
        return;
    }
    // A bare tool word typed by a person starts the tool, as in any CAD
    // package: an alias (L, LINE, C, TR) or a catalogue id
    // (draw.circle.ttr). With arguments it stays the interpreter's
    // (LINE 0,0 10,0 draws at once). A script's, a paste's or a dialog's line
    // is the interpreter's however short, as katana_cli runs it: ERASE in a
    // script once started the Erase tool, erased nothing, and the script
    // still reported every line run.
    if (source == LineSource::Typed && words.size() == 1) {
        if (const auto id = tools::toolIdForCommand(verb.toStdString())) {
            startTool(*id);
            return;
        }
    }
    runInterpreterLine(line, verb);
}

void MainWindow::runInterpreterLine(const QString& line, const QString& verb)
{
    const bool replacesDocument = verb == "NEW" || verb == "OPEN";
    if (replacesDocument && !confirmDiscard()) {
        return;
    }
    // As File > Save does - but only for a SAVE that has somewhere to go:
    // writing the record marks the drawing modified, and an untitled SAVE
    // with no directory fails, which left a drawing nobody touched asking to
    // be saved.
    if (verb == "SAVE" &&
        katana::cad::typedSaveHasDestination(line.toStdString(), document_.hasProject())) {
        recordCustomisation();
    }
    const auto reply = interpreter_.run(line.toStdString());
    if (!reply) {
        logMessage(QString::fromStdString(reply.error().describe()), true);
        return;
    }
    logMessage(QString::fromStdString(*reply));
    if (replacesDocument) {
        // The same as File > New and Open: the backdrop and the surfaces went
        // with the drawing.
        clearReferenceData();
        clearSceneData();
        views_->zoomExtentsAll();
    }
    if (verb == "OPEN") {
        reportMissingCustomisation();
    }
    historyCursor_ = static_cast<int>(interpreter_.history().size());
}

// ---- scripts ----------------------------------------------------------------------------------

namespace {

// The Recent Scripts list, newest first, in the settings as the online keys
// are: a person's, not a drawing's.
constexpr const char* kRecentScriptsKey = "scripts/recent";
// As many as a File menu lists of recent files in most programs.
constexpr qsizetype kRecentScripts = 8;

} // namespace

void MainWindow::runScript(const QString& path, bool continueOnError)
{
    // A script that runs itself, directly or through another, would never
    // end: refused by the file, as the file is what a person would fix.
    const QString canonical = QFileInfo(path).canonicalFilePath();
    if (!canonical.isEmpty() && runningScripts_.contains(canonical)) {
        logMessage("SCRIPT: " + QFileInfo(path).fileName() +
                       " is already running; a script may not run itself.",
                   true);
        return;
    }
    const auto lines = readScript(path);
    if (!lines) {
        logMessage(QString::fromStdString(lines.error().describe()), true);
        return;
    }
    runningScripts_ << canonical;
    runLines(*lines, QDir::fromNativeSeparators(path), continueOnError);
    runningScripts_.removeLast();
    // A headless run is a test's or an agent's, and leaves a person's list
    // alone.
    if (!headless_ && !canonical.isEmpty()) {
        QSettings settings;
        QStringList recent = settings.value(kRecentScriptsKey).toStringList();
        recent.removeAll(canonical);
        recent.prepend(canonical);
        settings.setValue(kRecentScriptsKey, recent.mid(0, kRecentScripts));
        refreshRecentScripts();
    }
}

void MainWindow::runPastedLines(const QString& text)
{
    const std::vector<ScriptLine> lines = scriptLines(text);
    if (lines.empty()) {
        return;
    }
    runLines(lines, QString(), false);
}

void MainWindow::runLines(const std::vector<ScriptLine>& lines, const QString& name,
                          bool continueOnError)
{
    ScriptOptions options;
    options.continueOnError = continueOnError;
    // A long script shows how far it has got and can be stopped between two
    // lines; a short one finishes before the dialog would appear. Never in a
    // headless run, which has nobody to press Cancel.
    std::unique_ptr<QProgressDialog> progress;
    if (!headless_ && lines.size() > 1) {
        progress = std::make_unique<QProgressDialog>(
            "Running " + (name.isEmpty() ? QString("the pasted lines") : QFileInfo(name).fileName()) +
                "...",
            "Cancel", 0, static_cast<int>(lines.size()), this);
        progress->setObjectName("scriptProgress");
        progress->setWindowTitle("Run Script");
        progress->setWindowModality(Qt::WindowModal);
        progress->setMinimumDuration(500);
        options.progress = [&progress](int done, int) {
            progress->setValue(done);
            return !progress->wasCanceled();
        };
    }
    const ScriptReport report = runScriptLines(lines, commandRunner(), options);
    progress.reset();
    logMessage(formatScriptReport(name, report));
    const QString shown = name.isEmpty() ? QString("The pasted lines") : QFileInfo(name).fileName();
    const auto textAt = [&lines](int number) {
        const auto found = std::ranges::find(lines, number, &ScriptLine::number);
        return found != lines.end() ? found->text : QString();
    };
    if (report.cancelled) {
        logMessage(QString("%1: cancelled before line %2; the lines from there were not run.")
                       .arg(shown)
                       .arg(report.stoppedAt),
                   true);
    } else if (report.failed > 0 && !continueOnError) {
        logMessage(QString("%1 stopped at line %2, which was refused: %3")
                       .arg(shown)
                       .arg(report.stoppedAt)
                       .arg(textAt(report.stoppedAt)),
                   true);
    } else if (report.failed > 0) {
        logMessage(QString("%1: %2 of the %3 lines run were refused.")
                       .arg(shown)
                       .arg(report.failed)
                       .arg(report.ran),
                   true);
    }
}

void MainWindow::showRunScript()
{
    if (scriptDialog_ == nullptr) {
        ScriptDialogContext context;
        context.run = commandRunner();
        context.headless = [this] { return headless_; };
        scriptDialog_ = new ScriptRunDialog(std::move(context), this);
    }
    scriptDialog_->show();
    scriptDialog_->raise();
    scriptDialog_->activateWindow();
}

void MainWindow::refreshRecentScripts()
{
    recentScriptsMenu_->clear();
    const QStringList recent = QSettings().value(kRecentScriptsKey).toStringList();
    // Named by position, so --trigger recentScript1 runs the newest, as a
    // click on the first item does.
    for (qsizetype at = 0; at < recent.size() && at < kRecentScripts; ++at) {
        const QString path = recent[at];
        QAction* item = recentScriptsMenu_->addAction(
            QString("&%1 %2").arg(at + 1).arg(QFileInfo(path).fileName()), this,
            [this, path] { (void)runVerbLine(scriptCommandLine(path, false)); });
        item->setObjectName(QString("recentScript%1").arg(at + 1));
        item->setStatusTip("Run " + QDir::toNativeSeparators(path));
    }
    if (recent.isEmpty()) {
        recentScriptsMenu_->addAction("(none)")->setEnabled(false);
        return;
    }
    recentScriptsMenu_->addSeparator();
    QAction* clear = recentScriptsMenu_->addAction("&Clear the List", this, [this] {
        QSettings().remove(kRecentScriptsKey);
        refreshRecentScripts();
    });
    clear->setObjectName("recentScriptsClear");
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
        // Several lines pasted run at once, a line at a time, as pasting a
        // script into AutoCAD's command line runs it; one line (with or
        // without its line end) is pasted to be edited, as ever. Whatever
        // was typed before the paste starts the first line.
        if (key->matches(QKeySequence::Paste)) {
            const QString pasted = QApplication::clipboard()->text();
            if (pasted.trimmed().contains('\n') || pasted.trimmed().contains('\r')) {
                commandInput_->insert(pasted);
                runCommandLine();
                return true;
            }
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

} // namespace

void MainWindow::importPath(const QString& path, const cad::ImportPlacement& placement)
{
    // The line a person would type, through the one executor: what it read
    // and where it put it are logged as records, and it is one undo step.
    (void)runVerbLine(importLine(QDir::fromNativeSeparators(path), placement));
}

PlacementDecision MainWindow::placeImport(const cad::ImportPlacement& placement,
                                          const katana::geometry::Box2& incoming)
{
    return decideImportPlacement(this, headless_, placement, document_.model().entities.bounds(),
                                 incoming,
                                 [this](const QString& text, bool isError) {
                                     logMessage(text, isError);
                                 });
}

void MainWindow::importWithPlacement(const QString& path)
{
    ImportPlacementDialog dialog(path, document_.model().entities.bounds(), this);
    if (dialog.exec() != QDialog::Accepted) {
        logMessage("Import cancelled.");
        return;
    }
    (void)runVerbLine(dialog.line()); // echoed, logged and undone as if typed
}

namespace {

// The files of a load by name and kind, for the record a project keeps.
std::vector<katana::cad::CustomisationSource>
sourcesOf(const katana::archive12d::Customisation& loaded)
{
    std::vector<katana::cad::CustomisationSource> sources;
    for (const katana::archive12d::LoadedFile& file : loaded.files) {
        const std::u8string name = file.path.filename().u8string();
        sources.push_back({std::string(reinterpret_cast<const char*>(name.data()), name.size()),
                           file.kind == katana::archive12d::CustomisationFile::StyleLibrary});
    }
    return sources;
}

// Up to `most` names, quoted, and how many more: loading the reference
// library replaces hundreds, and a log line of them all says nothing.
QString sampleOf(const std::vector<std::string>& names, std::size_t most = 8)
{
    QStringList quoted;
    for (std::size_t i = 0; i < names.size() && i < most; ++i) {
        quoted << "\"" + QString::fromStdString(names[i]) + "\"";
    }
    QString text = quoted.join(", ");
    if (names.size() > most) {
        text += " and " + QString::number(names.size() - most) + " more";
    }
    return text;
}

} // namespace

// The customisation that is PART OF THIS BUILD. Nothing is found, loaded or
// configured: its linestyles, symbols and survey codes are compiled in, so a
// survey drawing is drawn with them from the moment it is opened.
//
// A build made without one falls back to looking beside the executable, so a
// checkout that does not carry the customisation - it is third-party material
// under its own licence - can still be given one. Format > Load
// Customisation... adds to either; Format > Replace Loaded Customisation...
// takes the place of the kinds it brings.
void MainWindow::loadDefaultCustomisation()
{
    const katana::archive12d::Customisation& built = katana::archive12d::builtinCustomisation();
    // A file of it that could not be read cost only itself, and is said, once
    // (audit A12-06): unsaid, a damaged file drew every drawing's linestyles
    // as plain lines without a word.
    for (const std::string& error : built.errors) {
        logMessage("The built-in customisation: " + QString::fromStdString(error), true);
    }
    for (const std::string& warning : built.warnings) {
        logMessage("Warning: the built-in customisation: " + QString::fromStdString(warning));
    }
    if (!built.empty()) {
        document_.setStyleLibrary(built.library);
        document_.setSurveyMap(built.map);
        katana::cad::recordCustomisationLoad(customisation_, sourcesOf(built), false, false);
        logMessage("Customisation: " + grouped(built.library.size()) + " definitions (" +
                   grouped(librarySymbolCount(document_)) + " symbols) and " +
                   grouped(built.map.size()) + " survey code rules, built in.");
        return;
    }
    const auto paths = katana::archive12d::findCustomisation(
        std::filesystem::path(QCoreApplication::applicationFilePath().toStdString()));
    if (!paths.empty()) {
        applyCustomisation(paths);
    }
}

// Loading a customisation: the linestyle library, the symbol library and
// the survey code file. Several files at once, because they are useless
// apart - and WHICH is which is decided by looking inside each one, since
// `.4d` is the extension of both a style library and a survey code file
// (docs/survey_coding.md).
void MainWindow::loadCustomisation(katana::archive12d::LoadMode mode)
{
    const bool replace = mode == katana::archive12d::LoadMode::Replace;
    if (refuseFileDialog(replace ? "CUSTOMISE REPLACE <file> [<file>...]"
                                 : "CUSTOMISE <file> [<file>...]")) {
        return;
    }
    const QStringList chosen = QFileDialog::getOpenFileNames(
        this, replace ? "Replace Loaded Customisation" : "Load Customisation", QString(),
        "Customisation files (*.4d *.mapfile);;Style and symbol libraries (*.4d);;"
        "Survey code files (*.mapfile);;All files (*)");
    if (chosen.isEmpty()) {
        logMessage("Loading a customisation was cancelled.");
        return;
    }
    std::vector<std::filesystem::path> paths;
    paths.reserve(static_cast<std::size_t>(chosen.size()));
    for (const QString& one : chosen) {
        paths.emplace_back(toPath(one));
    }
    applyCustomisation(paths, mode);
}

void MainWindow::recordCustomisation()
{
    katana::storage::ProjectMetadata metadata = document_.metadata();
    metadata.customisation = katana::cad::customisationRecordToSave(
        metadata.customisation, customisationMissingAtOpen_, document_.styleLibrary(),
        customisation_);
    document_.setMetadata(std::move(metadata));
}

void MainWindow::reportMissingCustomisation()
{
    const std::vector<std::string> missing = katana::cad::customisationNotLoaded(
        document_.metadata().customisation, document_.styleLibrary(), customisation_);
    // Kept until loaded: saving this drawing must not forget them.
    customisationMissingAtOpen_ = missing;
    if (missing.empty()) {
        return;
    }
    // Not an error box: the drawing opens and draws, but what a missing
    // file defined draws as a plain line until it is loaded.
    logMessage("Warning: this project was drawn with customisation files that are not loaded: " +
               sampleOf(missing, missing.size()) +
               ". Load them with Format > Load Customisation...");
}

// Applying the loaded survey codes to the drawing. One undoable step, and a
// report - including which property the codes were read from, because "no
// entity carries a code" and "they carry it under another name" are
// different problems and look identical from the outside (PLAN.MD 20.3).
void MainWindow::applySurveyCodes()
{
    if (document_.surveyMap().empty()) {
        warnUser("No survey codes",
                 "Load a survey code file first: Format > Load Customisation...");
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
               " of them codes the loaded survey codes have a rule for.");
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

void MainWindow::applyCustomisation(const std::vector<std::filesystem::path>& paths,
                                    katana::archive12d::LoadMode mode)
{
    // Each file once, by the command line's rule: read twice, every rule of
    // a file named twice would sit in the map twice.
    const katana::cad::DistinctFiles distinct = katana::cad::distinctCustomisationFiles(paths);
    for (const std::filesystem::path& repeat : distinct.repeats) {
        logMessage(fromPath(repeat) + " is named twice in this load; it is read once.");
    }
    auto loaded = katana::archive12d::readCustomisation(distinct.files);
    if (!loaded) {
        warnUser("Customisation failed", QString::fromStdString(loaded.error().describe()));
        return;
    }
    for (const std::string& warning : loaded->warnings) {
        logMessage("Warning: " + QString::fromStdString(warning));
    }
    // Into what is loaded, through the one merge the command line's
    // CUSTOMISE uses too (audit QT-21): installing the load's library and
    // map wholesale threw away the other 792 definitions for one symbol
    // file, and every survey rule for a library with no mapfile.
    katana::archive12d::CustomisationMerge merged = katana::archive12d::mergeCustomisation(
        document_.styleLibrary(), document_.surveyMap(), *loaded, mode);
    const bool replace = mode == katana::archive12d::LoadMode::Replace;
    for (const katana::archive12d::FileMerge& file : merged.files) {
        const bool map = file.kind == katana::archive12d::CustomisationFile::MapFile;
        // The window's own words for the two kinds, not the reader's name
        // for its format: this line is read by a person, who knows them as
        // a style library and a survey code file.
        QString line = (file.name.empty() ? QString("(no file)") : QString::fromStdString(file.name)) +
                       ": " + (map ? QString("survey code file") : QString("style library")) +
                       ", " + grouped(file.added.size()) + " added, " +
                       grouped(file.replaced.size()) + " replaced" +
                       (map ? " (codes, once for each section)" : "");
        if (!file.added.empty()) {
            line += "; added " + sampleOf(file.added);
        }
        if (!file.replaced.empty()) {
            line += "; replaced " + sampleOf(file.replaced);
        }
        logMessage(line);
    }
    for (const std::string& problem : merged.problems) {
        logMessage("Not installed: " + QString::fromStdString(problem), true);
    }
    if (!merged.removedDefinitions.empty()) {
        logMessage(grouped(merged.removedDefinitions.size()) +
                   " definitions the load did not bring are gone: " +
                   sampleOf(merged.removedDefinitions));
    }
    if (!merged.removedKeys.empty()) {
        logMessage(grouped(merged.removedKeys.size()) +
                   " codes the load did not bring are gone: " + sampleOf(merged.removedKeys));
    }
    // Both installed unconditionally: a kind the load did not bring comes
    // back as it was (mergeCustomisation), so a symbol file alone keeps the
    // map.
    document_.setStyleLibrary(std::move(merged.library));
    document_.setSurveyMap(std::move(merged.map));
    const std::vector<katana::cad::CustomisationSource> sources = sourcesOf(*loaded);
    katana::cad::recordCustomisationLoad(customisation_, sources,
                                         replace && merged.libraryLoaded,
                                         replace && merged.mapLoaded);
    katana::cad::noteCustomisationLoaded(customisationMissingAtOpen_, sources);
    logMessage("Customisation now: " + grouped(document_.styleLibrary().size()) + " definitions (" +
               grouped(librarySymbolCount(document_)) + " symbols) and " +
               grouped(document_.surveyMap().size()) + " survey code rules.");
    // A customisation need not be self-contained. Naming what is missing is
    // the difference between a symbol that is plainly absent and one that is
    // silently drawn as a plain mark. Judged against everything now loaded,
    // since a mapfile may name what an earlier load defined.
    std::vector<std::string> missing;
    for (const std::string& name : document_.surveyMap().stylesReferenced()) {
        if (!katana::cad::isPlainLinestyle(name) && document_.definitionFor(name) == nullptr &&
            !katana::entity::isBuiltInSymbolName(name)) {
            missing.push_back(name);
        }
    }
    if (!missing.empty()) {
        logMessage(grouped(missing.size()) +
                   " names the survey codes ask for are in no loaded library: " +
                   sampleOf(missing));
    }
    reportCustomisationCoverage();
}

// What the loaded customisation means for THIS drawing. Without it, "the
// linestyles are not showing" is indistinguishable from "this drawing's
// styles are plain continuous lines" and from "nothing is loaded at all".
void MainWindow::reportCustomisationCoverage()
{
    const katana::cad::CustomisationCoverage coverage =
        katana::cad::customisationCoverage(document_);
    if (coverage.styles == 0) {
        logMessage("This drawing has no styles yet; import a drawing or survey that carries "
                   "styles, or make one in Format > Styles and Linetypes, to see the "
                   "customisation take effect.");
        return;
    }
    logMessage(grouped(coverage.resolved) + " of this drawing's " + grouped(coverage.styles) +
               " styles are drawn with a loaded definition (" + grouped(coverage.named) +
               " name one; the rest are plain continuous lines).");
    if (!coverage.unresolved.empty()) {
        logMessage(grouped(coverage.unresolved.size()) +
                   " names are in no loaded library, starting with \"" +
                   QString::fromStdString(coverage.unresolved.front()) + "\"");
    }
    // The other reason a style draws a fallback, with a different fix: the
    // library IS loaded and defines the name - as a symbol, which a line
    // cannot be drawn with, so it is drawn solid (D2). Pick a linestyle.
    if (!coverage.notLinestyles.empty()) {
        logMessage(grouped(coverage.notLinestyles.size()) +
                   " linetype names are symbols, not linestyles, so those lines are drawn "
                   "solid; starting with \"" +
                   QString::fromStdString(coverage.notLinestyles.front()) + "\"");
    }
}

void MainWindow::importFile()
{
    if (refuseFileDialog("IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]")) {
        return;
    }
    const QString selected =
        QFileDialog::getOpenFileName(this, "Import", QString(), importFilter());
    if (selected.isEmpty()) {
        return;
    }
    // The IMPORT line a person would type, run through the one executor. A
    // DXF or a .12da has one choice, where it lands, asked first; everything
    // else keeps the quick path, the GIS menu's imports being the considered
    // one (docs/interop.md, "Placing an import").
    const std::filesystem::path path = toPath(selected);
    const interop::SourceKind kind = interop::kindForPath(path);
    if (katana::dxf::isDxfPath(path) || kind == interop::SourceKind::Archive12d) {
        importWithPlacement(selected);
        return;
    }
    if (kind != interop::SourceKind::Unknown) {
        (void)runVerbLine(importLine(selected, {}));
        return;
    }
    warnUser( "Import",
                         "Katana does not recognise the extension of\n" + selected +
                             "\n\nSupported: " + patternsFor(interop::vectorExtensions()) + ' ' +
                             patternsFor(interop::archive12dExtensions()) + ' ' +
                             patternsFor(interop::rasterExtensions()) + ' ' +
                             patternsFor(interop::pointCloudExtensions()));
}

void MainWindow::importVectorFile(const std::filesystem::path& path,
                                  interop::VectorImportOptions options,
                                  const cad::ImportPlacement& placement)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto imported = interop::importVector(path, options);
    QApplication::restoreOverrideCursor();
    if (!imported.ok()) {
        logMessage(QString::fromStdString(imported.error().describe()), true);
        warnUser( "Import failed",
                             QString::fromStdString(imported.error().describe()));
        return;
    }

    // Where it lands, chosen or asked BEFORE anything is added. A move is
    // read again with the shift rather than made afterwards - the same
    // choices, the layers, the target, the attributes, with the shift added -
    // so the one reader applies the one shift to everything, as katana_cli
    // does.
    const PlacementDecision placed = placeImport(placement, imported->bounds);
    if (placed.cancelled) {
        logMessage("Import cancelled.");
        return;
    }
    if (placed.shift) {
        options.originShift = placed.shift;
        QApplication::setOverrideCursor(Qt::WaitCursor);
        imported = interop::importVector(path, options);
        QApplication::restoreOverrideCursor();
        if (!imported.ok()) {
            logMessage(QString::fromStdString(imported.error().describe()), true);
            return;
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
    if (created != 0) {
        transaction->add(cmd::createEntities(std::move(imported->entities)));
    }

    // A file with nothing the drawing can hold - every feature skipped, say -
    // leaves the transaction empty, and the stack refuses an empty one as a
    // command that changes nothing. That is not a failed import: the file was
    // read, and its warnings below say why nothing came of it.
    if (transaction->size() != 0) {
        const auto status = document_.execute(std::move(transaction));
        if (!status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
            warnUser("Import failed", QString::fromStdString(status.error().describe()));
            return;
        }
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
    views_->zoomExtentsAll();
}

void MainWindow::importRasterFile(const std::filesystem::path& path,
                                  interop::RasterImportOptions options)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto raster = interop::importRaster(path, options);
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
    views_->zoomExtentsAll();
}

void MainWindow::importPointCloudFile(const std::filesystem::path& path,
                                      interop::PointCloudImportOptions options)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto cloud = interop::importPointCloud(path, options);
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
    views_->zoomExtentsAll();
}

void MainWindow::exportVectorFile()
{
    if (document_.model().entities.empty() && document_.model().alignments.empty() &&
        sceneSurfaces_.empty()) {
        if (headless_) {
            logMessage("Export: the drawing is empty.", true);
        } else {
            QMessageBox::information(this, "Export", "The drawing is empty.");
        }
        return;
    }
    if (refuseFileDialog("EXPORT <file>")) {
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

    const std::filesystem::path path = toPath(selected);
    interop::VectorExportOptions options;
    if (interop::kindForPath(path) == interop::SourceKind::Archive12d ||
        katana::dxf::isDxfPath(path)) {
        // A 12d archive has none of the GDAL formats' choices - it keeps arcs
        // as arcs and every property - and nor has the native DXF writer, so
        // the only question is how much.
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
    } else {
        VectorExportDialog dialog(chosenFilter, document_.selection().size(), this);
        if (dialog.exec() != QDialog::Accepted) {
            logMessage("Export cancelled.");
            return;
        }
        dialog.apply(options);
        if (dialog.selectedOnly()) {
            options.entities = document_.selection().ids();
        }
    }
    (void)exportDrawingTo(path, std::move(options));
}

bool MainWindow::exportDrawingTo(const std::filesystem::path& path,
                                 interop::VectorExportOptions options)
{
    if (katana::dxf::isDxfPath(path)) {
        return exportDxfFile(path, options);
    }
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
            warnUser("Export failed", QString::fromStdString(archive.error().describe()));
            return false;
        }
        logMessage(QString("Exported %1 entities, %2 alignments and %3 surfaces to %4 (12d archive)")
                       .arg(grouped(archive->entitiesWritten))
                       .arg(grouped(archive->alignmentsWritten))
                       .arg(grouped(archive->surfacesWritten))
                       .arg(fromPath(path.filename())));
        for (const std::string& warning : archive->warnings) {
            logMessage("  " + QString::fromStdString(warning));
        }
        return true;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto result = interop::exportVector(document_.model(), path, options);
    QApplication::restoreOverrideCursor();

    if (!result.ok()) {
        logMessage(QString::fromStdString(result.error().describe()), true);
        warnUser("Export failed", QString::fromStdString(result.error().describe()));
        return false;
    }

    logMessage("Exported " + grouped(result->featuresWritten) + " features to " +
               fromPath(path.filename()) + " (" + QString::fromStdString(result->driver) + ")");
    for (const std::string& warning : result->warnings) {
        logMessage("  " + QString::fromStdString(warning));
    }
    return true;
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
                views_->repaintViews();
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
    views_->repaintViews();
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

void MainWindow::clearReferenceData()
{
    if (reference_.empty()) {
        return;
    }
    reference_.clear();
    views_->invalidateReferenceCache();
    refreshReferences();
}

void MainWindow::clearSceneData()
{
    // The scene lists first: they point into the stores. The views keep their
    // pointers to the (now empty) lists, which stay where they are.
    sceneSurfaces_.clear();
    sceneMeshes_.clear();
    surfaceStore_.clear();
    sceneSurfacesRevision_ = surfaceStore_.revision();
    meshStore_.clear();
    views_->drawingReplaced();
}

// ---- GIS menu: GDAL and PDAL (PLAN.MD Phases 17 and 20) ------------------------------------

namespace {

// A reference layer to act on: the one selected in the panel if it is of the
// wanted kind, else the only one there is, else one the person picks. Written
// once for rasters and clouds so the two commands cannot choose differently.
template <typename Layer>
const Layer* chooseLayer(const std::vector<Layer>& layers,
                         std::optional<katana::interop::ReferenceId> selected,
                         const QString& title, const QString& kind, bool headless,
                         QWidget* parent, const std::function<void(const QString&)>& refuse)
{
    if (layers.empty()) {
        refuse("No " + kind + " is loaded. Use GIS > Import first.");
        return nullptr;
    }
    if (selected) {
        for (const Layer& layer : layers) {
            if (layer.id == *selected) {
                return &layer;
            }
        }
    }
    if (layers.size() == 1) {
        return &layers.front();
    }
    if (headless) {
        refuse("Several " + kind + "s are loaded and nobody can be asked which; select one "
               "in the Reference Data panel.");
        return nullptr;
    }
    // Labelled with the id as well as the name: two imports of one file have
    // the same name, and the list must still say which is which.
    QStringList labels;
    for (const Layer& layer : layers) {
        labels << QString("%1  (#%2)").arg(QString::fromStdString(layer.name)).arg(layer.id);
    }
    bool accepted = false;
    const QString chosen =
        QInputDialog::getItem(parent, title, kind.left(1).toUpper() + kind.mid(1) + ":", labels,
                              0, false, &accepted);
    if (!accepted) {
        return nullptr;
    }
    return &layers[static_cast<std::size_t>(labels.indexOf(chosen))];
}

} // namespace

const interop::PointCloudLayer* MainWindow::chooseReferenceCloud(const QString& title)
{
    std::optional<katana::interop::ReferenceId> selected;
    if (const int row = referenceTable_->currentRow(); row >= 0) {
        if (const QTableWidgetItem* item = referenceTable_->item(row, kRefName)) {
            selected = static_cast<katana::interop::ReferenceId>(
                item->data(Qt::UserRole).toULongLong());
        }
    }
    return chooseLayer(reference_.pointClouds(), selected, title, "point cloud", headless_, this,
                       [this](const QString& why) { logMessage(why, true); });
}

const interop::RasterOverlay* MainWindow::chooseReferenceRaster(const QString& title)
{
    std::optional<katana::interop::ReferenceId> selected;
    if (const int row = referenceTable_->currentRow(); row >= 0) {
        if (const QTableWidgetItem* item = referenceTable_->item(row, kRefName)) {
            selected = static_cast<katana::interop::ReferenceId>(
                item->data(Qt::UserRole).toULongLong());
        }
    }
    return chooseLayer(reference_.rasters(), selected, title, "raster", headless_, this,
                       [this](const QString& why) { logMessage(why, true); });
}

std::unique_ptr<DatasetInfoDialog> MainWindow::makeDatasetInfo(const QString& path)
{
    // The dialog runs INFO lines through the one executor; it reads nothing
    // itself (dataset_info_dialog.hpp).
    DatasetInfoRunner runner;
    runner.run = commandRunner();
    runner.await = [this](const VerbOutcome& started, std::function<void(const VerbOutcome&)> done) {
        return awaitJob(started, std::move(done));
    };
    auto dialog = std::make_unique<DatasetInfoDialog>(QDir::fromNativeSeparators(path),
                                                      std::move(runner), this);
    // Headless, the lines have answered by now: a file that could not be
    // described has no window to grab, and the run says so.
    if (headless_ && !dialog->described()) {
        return nullptr;
    }
    return dialog;
}

bool MainWindow::awaitJob(const VerbOutcome& started, std::function<void(const VerbOutcome&)> done)
{
    // The job a line started says so in its reply, as a record:
    // job id=<n> title="..." state=started (geo/geo_workbench.hpp).
    std::optional<JobId> job;
    for (const katana::app::geo::Record& record :
         katana::app::geo::parseRecords(started.reply.toStdString())) {
        if (record.kind == "job" && record.get("state") == std::optional<std::string>("started")) {
            if (const auto id = katana::core::parseInteger(record.get("id").value_or(""))) {
                job = static_cast<JobId>(*id);
            }
        }
    }
    if (!job || geo_ == nullptr || !JobRunner::of(*this).isActive(*job)) {
        return false;
    }
    // Told once, then forgotten. The listener is added before the event
    // loop runs again, so the job cannot end unheard in between.
    auto key = std::make_shared<int>(0);
    *key = geo_->addFinishedListener(
        [this, id = *job, key, done = std::move(done)](JobId ended, const VerbOutcome& outcome) {
            if (ended != id) {
                return;
            }
            geo_->removeFinishedListener(*key);
            done(outcome);
        });
    return true;
}

std::unique_ptr<QDialog> MainWindow::makeImportOptions(const QString& path)
{
    // A DXF or a .12da has one option, where it lands; GDAL is not asked.
    const std::filesystem::path file = toPath(path);
    if (katana::dxf::isDxfPath(file) ||
        interop::kindForPath(file) == interop::SourceKind::Archive12d) {
        return std::make_unique<ImportPlacementDialog>(path, document_.model().entities.bounds(),
                                                       this);
    }
    auto description = interop::describeSource(toPath(path));
    if (!description) {
        logMessage(QString::fromStdString(description.error().describe()), true);
        warnUser("Import", QString::fromStdString(description.error().describe()));
        return nullptr;
    }
    switch (description->kind) {
    case interop::SourceKind::Vector:
        return std::make_unique<VectorImportDialog>(*description,
                                                    document_.model().entities.bounds(), this);
    case interop::SourceKind::Raster:
        return std::make_unique<RasterImportDialog>(*description, this);
    case interop::SourceKind::PointCloud:
        return std::make_unique<PointCloudImportDialog>(*description, this);
    case interop::SourceKind::Archive12d:
    case interop::SourceKind::Unknown:
        break;
    }
    warnUser("Import", "Katana has no import options for " + path);
    return nullptr;
}

void MainWindow::importWithOptions(const QString& path)
{
    // None of GDAL's options apply to a DXF or a .12da: only where it lands.
    const std::filesystem::path file = toPath(path);
    if (katana::dxf::isDxfPath(file) ||
        interop::kindForPath(file) == interop::SourceKind::Archive12d) {
        importWithPlacement(path);
        return;
    }
    auto dialog = makeImportOptions(path);
    if (dialog == nullptr) {
        return; // reported
    }
    if (dialog->exec() != QDialog::Accepted) {
        logMessage("Import cancelled.");
        return;
    }
    // Routed by the dialog the file got, which was routed by what the file
    // IS - a .las chosen through Import Vector's "All files" still arrives
    // as a point cloud. The vector dialog's layer and attribute choices have
    // no IMPORT word yet, so it imports itself; the move it makes is logged.
    if (const auto* vector = dynamic_cast<const VectorImportDialog*>(dialog.get())) {
        vector->placementBox().remember();
        importVectorFile(file, vector->options(), vector->placementBox().placement());
    } else if (const auto* raster = dynamic_cast<const RasterImportDialog*>(dialog.get())) {
        importRasterFile(file, raster->options());
    } else if (const auto* cloud = dynamic_cast<const PointCloudImportDialog*>(dialog.get())) {
        importPointCloudFile(file, cloud->options());
    }
}

void MainWindow::importVectorWithOptions()
{
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Import Vector Data", QString(),
        "Vector data (" + patternsFor(interop::vectorExtensions()) + ");;All files (*)");
    if (!chosen.isEmpty()) {
        importWithOptions(chosen);
    }
}

void MainWindow::importRasterWithOptions()
{
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Import Raster", QString(),
        "Raster (" + patternsFor(interop::rasterExtensions()) + ");;All files (*)");
    if (!chosen.isEmpty()) {
        importWithOptions(chosen);
    }
}

void MainWindow::importPointCloudWithOptions()
{
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Import Point Cloud", QString(),
        "Point cloud (" + patternsFor(interop::pointCloudExtensions()) + ");;All files (*)");
    if (!chosen.isEmpty()) {
        importWithOptions(chosen);
    }
}

void MainWindow::showDatasetInformation()
{
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Dataset Information", QString(),
        "GIS data and point clouds (" + patternsFor(interop::vectorExtensions()) + ' ' +
            patternsFor(interop::rasterExtensions()) + ' ' +
            patternsFor(interop::pointCloudExtensions()) + ");;All files (*)");
    if (chosen.isEmpty()) {
        return;
    }
    if (auto dialog = makeDatasetInfo(chosen)) {
        dialog->exec();
    }
}

void MainWindow::exportPointCloud()
{
    const interop::PointCloudLayer* cloud = chooseReferenceCloud("Export Point Cloud");
    if (cloud == nullptr) {
        return; // reported, or the choice was cancelled
    }
    // What is held is a SAMPLE when the import was budgeted, and writing it
    // out as if it were the survey would be a quiet loss of most of the data.
    if (cloud->isDecimated() && !headless_) {
        const auto answer = QMessageBox::question(
            this, "Export Point Cloud",
            QString("'%1' holds %2 of the file's %3 points - the sample imported for display.\n\n"
                    "Export the sample? To rewrite the whole file, use Convert Point Cloud to "
                    "COPC instead.")
                .arg(QString::fromStdString(cloud->name))
                .arg(grouped(cloud->points.size()))
                .arg(grouped(cloud->sourcePointCount)),
            QMessageBox::Yes | QMessageBox::Cancel);
        if (answer != QMessageBox::Yes) {
            return;
        }
    }
    QStringList filters;
    for (const interop::FormatChoice& format : interop::pointCloudExportFormats()) {
        filters << (QString::fromStdString(format.description) + " (*." +
                    QString::fromStdString(format.extension) + ")");
    }
    const QString chosen =
        QFileDialog::getSaveFileName(this, "Export Point Cloud",
                                     QString::fromStdString(cloud->name) + ".laz",
                                     filters.join(";;"));
    if (chosen.isEmpty()) {
        return;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const auto status = interop::exportPointCloud(*cloud, toPath(chosen));
    QApplication::restoreOverrideCursor();
    if (!status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
        warnUser("Export failed", QString::fromStdString(status.error().describe()));
        return;
    }
    logMessage(QString("Exported %1 points of '%2' to %3%4")
                   .arg(grouped(cloud->points.size()))
                   .arg(QString::fromStdString(cloud->name))
                   .arg(fromPath(toPath(chosen).filename()))
                   .arg(cloud->isDecimated() ? QString(" (a sample of %1)")
                                                   .arg(grouped(cloud->sourcePointCount))
                                             : QString()));
}

void MainWindow::exportSurfaceAsDem()
{
    if (sceneSurfaces_.empty()) {
        logMessage("There is no surface to export. Build one from the Terrain menu, or import "
                   "a 12d archive that holds a tin.",
                   true);
        return;
    }
    std::vector<SurfaceChoice> choices;
    choices.reserve(sceneSurfaces_.size());
    for (const cad::SceneSurface& item : sceneSurfaces_) {
        choices.push_back({QString::fromStdString(item.name), item.surface->bounds()});
    }
    SurfaceRasterDialog dialog(std::move(choices), this);
    if (dialog.exec() != QDialog::Accepted || dialog.surfaceIndex() < 0) {
        return;
    }
    const cad::SceneSurface& chosenSurface =
        sceneSurfaces_[static_cast<std::size_t>(dialog.surfaceIndex())];

    QStringList filters;
    for (const interop::FormatChoice& format : interop::rasterExportFormats()) {
        filters << (QString::fromStdString(format.description) + " (*." +
                    QString::fromStdString(format.extension) + ")");
    }
    const QString chosen = QFileDialog::getSaveFileName(
        this, "Export Surface as DEM", QString::fromStdString(chosenSurface.name) + ".tif",
        filters.join(";;"));
    if (chosen.isEmpty()) {
        return;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto written = interop::exportSurfaceRaster(*chosenSurface.surface, toPath(chosen),
                                                dialog.options());
    QApplication::restoreOverrideCursor();
    if (!written) {
        logMessage(QString::fromStdString(written.error().describe()), true);
        warnUser("Export failed", QString::fromStdString(written.error().describe()));
        return;
    }
    logMessage(QString("Wrote '%1' as a %2 x %3 DEM (%4 cells on the surface) to %5 (%6).")
                   .arg(QString::fromStdString(chosenSurface.name))
                   .arg(written->columns)
                   .arg(written->rows)
                   .arg(grouped(written->cellsWithData))
                   .arg(fromPath(toPath(chosen).filename()))
                   .arg(QString::fromStdString(written->driver)));
    // Said, because a DEM with no coordinate system is placed by whatever
    // reads it: a Katana surface does not record the one it was built in.
    logMessage("  the DEM declares no coordinate system; its coordinates are the drawing's");
}

void MainWindow::convertPointCloudToCopc()
{
    // Two file dialogs nobody could close: a headless session is pointed at
    // the verb that asks nothing instead, where --action convertCopc would
    // wait on the first one for ever.
    if (headless_) {
        logMessage("A headless session opens no file dialog: type COPC <source> "
                   "<destination.copc.laz> instead.",
                   true);
        return;
    }
    const QString source = QFileDialog::getOpenFileName(
        this, "Convert Point Cloud to COPC", QString(),
        "Point cloud (" + patternsFor(interop::pointCloudExtensions()) + ");;All files (*)");
    if (source.isEmpty()) {
        return;
    }
    const katana::pointcloud::PointCloudEngine engine;
    if (const auto copc = engine.isCopc(toPath(source)); copc && *copc) {
        logMessage(fromPath(toPath(source).filename()) +
                   " is already a Cloud Optimised Point Cloud; import it with GIS > Import "
                   "Point Cloud to read it at a level of detail.");
        return;
    }
    // The extension is what makes every later read use readers.copc, so it is
    // enforced here rather than left to be discovered as a refusal.
    std::filesystem::path suggested = toPath(source);
    suggested.replace_extension(".copc.laz");
    QString destination = QFileDialog::getSaveFileName(this, "Save COPC As", fromPath(suggested),
                                                       "Cloud Optimised Point Cloud (*.copc.laz)");
    if (destination.isEmpty()) {
        return;
    }
    if (!destination.endsWith(".copc.laz", Qt::CaseInsensitive)) {
        destination += ".copc.laz";
    }
    // The dialogs chose the files; the conversion is the COPC verb's, run as
    // if typed, so it is logged as one and an agent's COPC is the same code.
    const VerbOutcome outcome = runVerbLine(QString("COPC \"%1\" \"%2\"")
                                                .arg(QDir::fromNativeSeparators(source),
                                                     QDir::fromNativeSeparators(destination)));
    if (!outcome.ok) {
        warnUser("Conversion failed", outcome.error);
        return;
    }
    // The conversion is a background job: the offer to import waits for it
    // to end, and is made only when it converted.
    (void)awaitJob(outcome, [this, destination](const VerbOutcome& done) {
        if (!done.ok) {
            warnUser("Conversion failed", done.error);
            return;
        }
        if (QMessageBox::question(this, "Convert Point Cloud to COPC",
                                  "Import the COPC file now?") == QMessageBox::Yes) {
            importWithOptions(destination);
        }
    });
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

    // Every item below has an object name, so --trigger and DRIVE's '*' reach
    // it as a click does; the layouts' and the standard views' are their
    // enumerators', since their menu text ("Two: Vertical", "SW Isometric")
    // is no name.
    QMenu* layoutMenu = viewMenu->addMenu("Viewport &Layout");
    for (const auto& [kind, name] : std::initializer_list<std::pair<cad::LayoutKind, const char*>>{
             {cad::LayoutKind::Single, "Single"},
             {cad::LayoutKind::SplitVertical, "SplitVertical"},
             {cad::LayoutKind::SplitHorizontal, "SplitHorizontal"},
             {cad::LayoutKind::ThreeLeft, "ThreeLeft"},
             {cad::LayoutKind::ThreeTop, "ThreeTop"},
             {cad::LayoutKind::Quad, "Quad"}}) {
        QAction* action = layoutMenu->addAction(cad::toString(kind));
        action->setObjectName(QString("viewLayout") + name);
        action->setData(static_cast<int>(kind));
        connect(action, &QAction::triggered, this, [this, kind] {
            views_->arrange(kind);
            refreshViewMenu();
        });
        layoutActions_.push_back(action);
    }

    QMenu* kindMenu = viewMenu->addMenu("Active Viewport S&hows");
    for (const cad::ViewKind kind : {cad::ViewKind::Plan, cad::ViewKind::Model3D,
                                     cad::ViewKind::Section, cad::ViewKind::Elevation}) {
        QAction* action = kindMenu->addAction(cad::toString(kind));
        // viewShowsPlan, viewShows3D, ...: what --trigger and DRIVE's '*'
        // reach it by.
        action->setObjectName(QString("viewShows") + cad::toString(kind));
        action->setCheckable(true);
        action->setData(static_cast<int>(kind));
        connect(action, &QAction::triggered, this, [this, kind] {
            if (auto status = views_->setViewKind(views_->viewSet().activeId(), kind); !status) {
                logMessage(QString::fromStdString(status.error().describe()), true);
            }
            refreshViewMenu();
        });
        kindActions_.push_back(action);
    }

    QMenu* standard = viewMenu->addMenu("Standard &3D Views");
    for (const auto& [view, name] :
         std::initializer_list<std::pair<render::StandardView, const char*>>{
             {render::StandardView::Top, "Top"},
             {render::StandardView::Bottom, "Bottom"},
             {render::StandardView::Front, "Front"},
             {render::StandardView::Back, "Back"},
             {render::StandardView::Left, "Left"},
             {render::StandardView::Right, "Right"},
             {render::StandardView::IsoSouthWest, "IsoSouthWest"},
             {render::StandardView::IsoSouthEast, "IsoSouthEast"},
             {render::StandardView::IsoNorthEast, "IsoNorthEast"},
             {render::StandardView::IsoNorthWest, "IsoNorthWest"}}) {
        QAction* action = standard->addAction(render::toString(view), this, [this, view] {
            if (RenderViewWidget* renderView = views_->activeRenderView()) {
                renderView->setStandardView(view);
            } else {
                logMessage("No 3D viewport is open. Use View > Viewport Layout.", true);
            }
        });
        action->setObjectName(QString("viewStandard") + name);
    }
    QAction* perspective = viewMenu->addAction("Toggle Pe&rspective", QKeySequence(Qt::Key_F9),
                                               this, [this] {
        RenderViewWidget* renderView = views_->activeRenderView();
        if (renderView == nullptr) {
            logMessage("No 3D viewport is open.", true);
            return;
        }
        const bool wasPerspective =
            renderView->camera().projection() == render::Projection::Perspective;
        renderView->setProjection(wasPerspective ? render::Projection::Orthographic
                                                 : render::Projection::Perspective);
        logMessage(wasPerspective ? "Orthographic projection." : "Perspective projection.");
    });
    perspective->setObjectName("viewTogglePerspective");
    QAction* exaggeration = viewMenu->addAction("&Vertical Exaggeration...", this,
                                                [this] { askVerticalExaggeration(); });
    exaggeration->setObjectName("viewVerticalExaggeration");
}

void MainWindow::refreshViewMenu()
{
    for (QAction* action : kindActions_) {
        action->setChecked(action->data().toInt() == static_cast<int>(views_->activeViewKind()));
    }
}

void MainWindow::askVerticalExaggeration()
{
    // The box would wait for ever in a headless run, which --trigger now
    // reaches it from: pointed at the line instead, as COPC's two file
    // dialogs are.
    if (headless_) {
        logMessage("A headless session opens no dialog: type EXAGGERATION <factor> instead.",
                   true);
        return;
    }
    bool accepted = false;
    const double current = views_->sceneOptions().verticalExaggeration;
    const double factor = QInputDialog::getDouble(
        this, "Vertical Exaggeration", "Multiply elevations by:", current,
        kLeastExaggeration, kMostExaggeration, 2, &accepted);
    if (accepted) {
        (void)runVerbLine("EXAGGERATION " +
                          QString::fromStdString(katana::core::formatExactReal(factor)));
    }
}

void MainWindow::setVerticalExaggeration(double factor)
{
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

void MainWindow::syncSceneSurfaces()
{
    if (sceneSurfacesRevision_ == surfaceStore_.revision()) {
        return;
    }
    std::vector<cad::SceneSurface> next;
    next.reserve(surfaceStore_.all().size());
    for (const katana::terrain::NamedSurface& named : surfaceStore_.all()) {
        cad::SceneSurface item;
        const auto kept = std::ranges::find_if(sceneSurfaces_, [&](const cad::SceneSurface& shown) {
            return shown.surface == named.surface.get();
        });
        if (kept != sceneSurfaces_.end()) {
            item = *kept; // its style, colouring and visibility
        }
        item.name = named.name;
        item.surface = named.surface.get();
        next.push_back(std::move(item));
    }
    sceneSurfaces_ = std::move(next);
    sceneSurfacesRevision_ = surfaceStore_.revision();
    // The views hold a pointer to the VECTOR, not to its elements, so this is a
    // refresh rather than a repair; the surfaces themselves are shared and
    // never move.
    views_->setSurfaces(&sceneSurfaces_);
}

void MainWindow::addSurface(std::string name, katana::terrain::TinSurface surface)
{
    // A second surface of a name is "name (2)", as the session names a
    // second alignment: the store keeps names unique, so SURFACE <name>
    // finds one.
    const std::string unique = surfaceStore_.uniqueName(name);
    if (auto added = surfaceStore_.add(
            {unique, std::make_shared<const katana::terrain::TinSurface>(std::move(surface)), {}});
        !added) {
        logMessage(QString::fromStdString(added.error().describe()), true);
        return;
    }
    syncSceneSurfaces();
    const cad::SceneSurface& item = sceneSurfaces_.back();
    views_->refreshAll();

    logMessage(QString("Surface '%1': %2 vertices, %3 triangles, elevation %4 to %5.")
                   .arg(QString::fromStdString(sceneSurfaces_.back().name))
                   .arg(item.surface->vertexCount())
                   .arg(item.surface->triangleCount())
                   .arg(item.surface->minElevation(), 0, 'f', 3)
                   .arg(item.surface->maxElevation(), 0, 'f', 3));

    // Show it. A surface the user cannot see is not obviously a success.
    views_->ensureView(cad::ViewKind::Model3D);
    refreshViewMenu();
    if (RenderViewWidget* renderView = views_->activeRenderView()) {
        renderView->invalidateScene();
        renderView->zoomExtents();
    }
}

namespace {

// What a Surface From command's job prepares on its own thread: the points
// and breaklines to triangulate. Notes about how they were chosen (a sampling
// stride, no-data pixels left out) go into `notes` as they are found, not
// into the result, because they matter MOST when the preparation fails: "14
// 400 no-data pixels left out" is the reason a raster has fewer than three
// points to triangulate, and a note carried in the result was lost with it.
using SurfaceInput = std::function<katana::core::Result<katana::terrain::TinInput>(
    katana::qt::JobControl&, QStringList& notes)>;

// What every Surface From command shares once its inputs are in hand: the
// triangulation, run as a background job (jobs.hpp) so that the window keeps
// painting and answering while CGAL works - 1.7 s at the 400k-point cap,
// measured, which used to be 1.7 s of a frozen window behind a wait cursor.
//
// `prepare` runs on the job's thread, on inputs the job already OWNS (a
// copied cloud, a raster path): never the document or a widget, which belong
// to the GUI thread. `addSurface` runs on the GUI thread with the result and
// is the only step that touches the window. `log` reports the outcome, which
// is never silent: the notes - whether or not the job succeeded - then a
// failure's message, a cancellation or the time taken.
//
// A headless session waits for the job, pumping events, because what follows
// it (a screenshot, a report) must see the surface; it also logs the longest
// pause of the event loop, the freeze an interactive user would have felt.
void startSurfaceJob(katana::qt::JobRunner& runner, bool headless, const QString& title,
                     SurfaceInput prepare, katana::terrain::TinBuildOptions options,
                     std::function<void(katana::terrain::TinSurface&&)> addSurface,
                     std::function<void(const QString&, bool)> log)
{
    using katana::qt::JobRunner;
    // Written by the job before its completion is posted and read on the GUI
    // thread after it is delivered; the post orders the two.
    auto notes = std::make_shared<QStringList>();
    // For the headless report: how much of the pause was the Apply step.
    auto applySeconds = std::make_shared<double>(0.0);
    if (headless) {
        runner.resetEventLoopPause();
    }
    const katana::qt::JobId id = runner.start(
        title,
        [prepare = std::move(prepare), options, notes, addSurface, log](
            katana::qt::JobControl& control) -> katana::core::Result<JobRunner::Apply> {
            control.setStage("Reading");
            auto input = prepare(control, *notes);
            if (!input) {
                return input.error();
            }
            if (control.stopRequested()) {
                return JobRunner::Apply{}; // cancelled: the runner discards it anyway
            }
            control.setStage("Triangulating");
            auto built = katana::terrain::buildTin(*input, options);
            if (!built) {
                return built.error();
            }
            // Shared, because an Apply is a std::function and must be
            // copyable; the surface itself is moved out exactly once.
            auto result = std::make_shared<katana::terrain::TinBuildResult>(std::move(*built));
            return JobRunner::Apply([result, addSurface, log] {
                if (result->report.duplicatePointCount > 0) {
                    log(QString("%1 coincident points merged by averaging their elevations.")
                            .arg(result->report.duplicatePointCount),
                        false);
                }
                addSurface(std::move(result->surface));
            });
        },
        [title, notes, applySeconds, log](const katana::qt::JobReport& report) {
            *applySeconds = report.applySeconds;
            for (const QString& note : *notes) {
                log(note, false);
            }
            switch (report.outcome) {
            case katana::qt::JobOutcome::Finished:
                log(QString("%1: triangulated in %2 s in the background.")
                        .arg(title)
                        .arg(report.workSeconds, 0, 'f', 2),
                    false);
                break;
            case katana::qt::JobOutcome::Cancelled:
                log(title + " was cancelled; no surface was added.", false);
                break;
            case katana::qt::JobOutcome::Failed:
                log(QString::fromStdString(report.error), true);
                break;
            }
        });
    if (headless) {
        runner.waitFor(id);
        log(QString("%1: the event loop paused for at most %2 ms while it ran (adding the "
                    "result: %3 ms).")
                .arg(title)
                .arg(runner.longestEventLoopPause() * 1000.0, 0, 'f', 0)
                .arg(*applySeconds * 1000.0, 0, 'f', 0),
            false);
    }
}

} // namespace

void MainWindow::buildSurfaceFromPointCloud()
{
    const interop::PointCloudLayer* cloud = chooseReferenceCloud("Surface From Point Cloud");
    if (cloud == nullptr) {
        return; // reported, or the choice was cancelled
    }

    // The ground returns when the cloud is classified, every return when it
    // is not - one policy, interop::surfacePoints, and SAID either way: a
    // surface over trees and roofs presented as ground is the silent failure
    // audit QT-10 found.
    //
    // Copied here, on the GUI thread, because the cloud belongs to the
    // reference data and could be removed while the job runs; the job then
    // owns its copy outright.
    auto source = std::make_shared<interop::CloudSurfacePoints>(interop::surfacePoints(*cloud));
    if (source->groundOnly) {
        logMessage(QString("Using the %1 ground points (ASPRS class 2); %2 other returns left out.")
                       .arg(grouped(source->points.size()))
                       .arg(grouped(source->excluded)));
    } else {
        logMessage("This cloud has no points classified as ground (ASPRS class 2), so every "
                   "return is triangulated: vegetation and buildings are part of the surface.",
                   true);
    }
    const std::uint32_t step = thinningFor(source->points.size());
    if (step > 1) {
        logMessage(QString("Thinning %1 points to every %2 for triangulation.")
                       .arg(grouped(source->points.size()))
                       .arg(step));
    }

    katana::terrain::TinBuildOptions options;
    // Scanned points land on the same ground mark constantly and their
    // elevations differ by millimetres. Failing on that would make a surface
    // from a real scan impossible, so the mean is taken and reported.
    options.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
    const std::string name = cloud->name;
    startSurfaceJob(
        katana::qt::JobRunner::of(*this), headless_, "Surface From Point Cloud",
        [source, step](katana::qt::JobControl&,
                       QStringList&) -> katana::core::Result<katana::terrain::TinInput> {
            katana::terrain::TinInput input;
            input.points.reserve(source->points.size() / step + 1);
            for (std::size_t i = 0; i < source->points.size(); i += step) {
                input.points.push_back(source->points[i]);
            }
            return input;
        },
        options,
        [this, name](katana::terrain::TinSurface&& surface) {
            addSurface(name, std::move(surface));
        },
        [this](const QString& text, bool isError) { logMessage(text, isError); });
}

void MainWindow::buildSurfaceFromRaster()
{
    const interop::RasterOverlay* raster = chooseReferenceRaster("Surface From Raster");
    if (raster == nullptr) {
        return; // reported, or the choice was cancelled
    }

    // The band's TRUE values, read again from the source through GDAL. The
    // overlay holds only the 8-bit display copy - a grey ramp stretched over
    // the band's range - and heights rebuilt from that were 256 terraces
    // between two numbers the user had to type (audit QT-23). The source is
    // sampled on a stride that keeps the whole extent under the triangulation
    // cap (audit QT-24: the old stride could exceed it threefold).
    //
    // Read on the job's thread, from the file: a path is all the job takes,
    // so the overlay can go away meanwhile. GDAL keeps its error handlers per
    // thread, and the reader opens a dataset of its own.
    interop::RasterElevationOptions options;
    options.maxPoints = kMaximumTinPoints;
    const std::filesystem::path path = raster->source;
    const std::string name = raster->name;

    katana::terrain::TinBuildOptions buildOptions;
    buildOptions.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
    startSurfaceJob(
        katana::qt::JobRunner::of(*this), headless_, "Surface From Raster",
        [path, options, name](katana::qt::JobControl&, QStringList& notes)
            -> katana::core::Result<katana::terrain::TinInput> {
            auto elevations = interop::readRasterElevations(path, options);
            if (!elevations) {
                return elevations.error();
            }
            if (elevations->stride > 1) {
                notes << QString("Sampling one pixel in %1 along each row and column of %2: %3 "
                                 "points.")
                             .arg(elevations->stride)
                             .arg(QString::fromStdString(name))
                             .arg(grouped(elevations->points.size()));
            }
            if (elevations->noData != 0) {
                notes << QString("%1 no-data pixels left out.").arg(grouped(elevations->noData));
            }
            if (elevations->points.size() < 3) {
                return katana::core::makeError(
                    katana::core::ErrorCode::InvalidArgument,
                    "that raster has fewer than three pixels with an elevation to triangulate");
            }
            katana::terrain::TinInput input;
            input.points = std::move(elevations->points);
            return input;
        },
        buildOptions,
        [this, name](katana::terrain::TinSurface&& surface) {
            addSurface(name, std::move(surface));
        },
        [this](const QString& text, bool isError) { logMessage(text, isError); });
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
        // What the drawing SHOWS is what is triangulated: a layer switched off
        // is left out, as it is out of every view (audit REN-04).
        // The document rule, not the active view's: a surface is shared by
        // every view and must not depend on which one was clicked last.
        if (!katana::cad::isDrawn(document_.model(), entity, katana::cad::kNoLayerOverrides)) {
            return;
        }
        const bool carriesHeights =
            entity.properties.contains(std::string(katana::entity::kElevationProperty)) ||
            entity.properties.contains(std::string(katana::entity::kElevationsProperty));
        withElevation += carriesHeights ? 1 : 0;
        const auto heightAt = [&](const std::vector<std::optional<double>>& heights,
                                  std::size_t index) -> std::optional<double> {
            if (!carriesHeights) {
                return 0.0; // a plain 2D drawing: the datum, as before
            }
            return heights[index];
        };
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry)) {
            const auto heights = katana::entity::heightsOf(entity.properties, 1);
            if (const auto z = heightAt(heights, 0)) {
                input.points.push_back(
                    katana::geometry::Point3(point->position.x, point->position.y, *z));
            } else {
                ++withoutHeight;
            }
        } else if (const auto* polyline =
                       std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
            const auto heights =
                katana::entity::heightsOf(entity.properties, polyline->vertices.size());
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
    // The drawing was read above, on the GUI thread, because the document is
    // single-threaded; from here the job owns the input.
    auto owned = std::make_shared<katana::terrain::TinInput>(std::move(input));
    startSurfaceJob(
        katana::qt::JobRunner::of(*this), headless_, "Surface From Drawing",
        [owned](katana::qt::JobControl&,
                QStringList&) -> katana::core::Result<katana::terrain::TinInput> {
            return std::move(*owned);
        },
        options,
        [this](katana::terrain::TinSurface&& surface) {
            addSurface("Drawing", std::move(surface));
        },
        [this](const QString& text, bool isError) { logMessage(text, isError); });
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
        logMessage("Define an alignment first: Terrain > Alignment Manager, or ALIGN NEW name "
                   "x,y x,y ... on the command line.",
                   true);
        // Where one is defined, opened for the person; a headless run has
        // nobody to show it to.
        if (!headless_) {
            showAlignmentManager();
        }
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
        logMessage("No alignment has a design profile. Define one in Terrain > Alignment "
                   "Manager (Vertical), or with ALIGN DESIGN name s,z ...",
                   true);
        if (!headless_) {
            showAlignmentManager();
        }
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
    // The dialog writes the PLOT line and runs it through the one executor;
    // it asks nothing modally, so a headless run can drive it too.
    if (plotDialog_ == nullptr) {
        PlotDrawingDialogContext context;
        context.run = commandRunner();
        context.headless = [this] { return headless_; };
        context.suggestedPath = suggestedPlotFile(document_);
        context.confirmReplace = [this](const QString& path) {
            return confirmReplaceFile("Plot to PDF", path);
        };
        plotDialog_ = new PlotDrawingDialog(std::move(context), this);
    }
    plotDialog_->show();
    plotDialog_->raise();
    plotDialog_->activateWindow();
}

namespace {

// Export View as Image's file: the plot's, as a PNG.
QString suggestedImageFile(const katana::cad::Document& document)
{
    const QFileInfo plot(suggestedPlotFile(document));
    return QDir::toNativeSeparators(plot.path() + "/" + plot.completeBaseName() + ".png");
}

} // namespace

void MainWindow::showViewImageExport()
{
    if (imageDialog_ == nullptr) {
        ViewImageDialogContext context;
        context.run = commandRunner();
        context.headless = [this] { return headless_; };
        context.suggestedPath = suggestedImageFile(document_);
        context.confirmReplace = [this](const QString& path) {
            return confirmReplaceFile("Export View as Image", path);
        };
        imageDialog_ = new ViewImageDialog(std::move(context), this);
    }
    imageDialog_->show();
    imageDialog_->raise();
    imageDialog_->activateWindow();
}

void MainWindow::suggestPlotFiles()
{
    if (plotDialog_ != nullptr) {
        plotDialog_->suggestPath(suggestedPlotFile(document_));
    }
    if (imageDialog_ != nullptr) {
        imageDialog_->suggestPath(suggestedImageFile(document_));
    }
}

bool MainWindow::confirmReplaceFile(const QString& title, const QString& path)
{
    if (headless_) {
        return true;
    }
    return QMessageBox::question(this, title,
                                 QDir::toNativeSeparators(path) +
                                     " is already there. Replace it?") == QMessageBox::Yes;
}

void MainWindow::snapshotView(const SnapshotRequest& request)
{
    QImage image;
    QString viewName;
    if (request.view == SnapshotRequest::View::Plan) {
        ViewportWidget* view = views_->activePlanView();
        if (view == nullptr) {
            logMessage("SNAPSHOT: no plan view is open; open one, or ask for view=3d.", true);
            return;
        }
        const QColor background = request.background == SnapshotRequest::Background::Theme
                                      ? theme::viewport()
                                  : request.background == SnapshotRequest::Background::White
                                      ? QColor(Qt::white)
                                      : QColor(Qt::transparent);
        // On white it is drawn as a plot draws it: the screen's white pens
        // would vanish into the ground.
        image = view->renderToImage(snapshotSize(request, view->size()), background,
                                    request.background == SnapshotRequest::Background::White);
        viewName = "plan";
    } else {
        RenderViewWidget* view = views_->activeRenderView();
        if (view == nullptr) {
            logMessage("SNAPSHOT: no 3D view is open; open one, or ask for view=plan.", true);
            return;
        }
        // Grabbed as drawn - the renderer's own frame - and scaled.
        const QSize size = snapshotSize(request, view->size());
        image = view->grab().toImage();
        if (image.size() != size) {
            image = image.scaled(size, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
        }
        viewName = "3d";
    }
    const QString record =
        QString("view=%1 width=%2 height=%3").arg(viewName).arg(image.width()).arg(image.height());
    if (request.clipboard) {
        QApplication::clipboard()->setImage(image);
        logMessage("clipboard=yes " + record);
        return;
    }
    if (const auto status = writeSnapshot(image, request.path); !status) {
        logMessage(QString::fromStdString(status.error().describe()), true);
        return;
    }
    logMessage(QString("file=\"%1\" ").arg(QDir::toNativeSeparators(request.path)) + record);
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
        // Fitted by the view to what it draws, as Zoom Extents frames it
        // (ViewportWidget::fittedPlot, where the widget tests check it).
        auto fitted = view->fittedPlot(settings);
        if (!fitted) {
            return fitted.error();
        }
        settings = *fitted;
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
    // The same as a record, for a script or an agent to read: what PLOT and
    // --plot made, with the scale a fitted plot chose.
    const std::string_view style = cad::toString(settings.colourMode);
    logMessage(QString("file=\"%1\" paper=%2 orientation=%3 scale=%4 dpi=%5 style=%6 lineweight=%7")
                   .arg(QDir::toNativeSeparators(path), paperName(settings.paper),
                        settings.landscape ? "landscape" : "portrait")
                   .arg(settings.scaleDenominator, 0, 'g', 10)
                   .arg(settings.dpi, 0, 'g', 10)
                   .arg(QString::fromUtf8(style.data(), static_cast<qsizetype>(style.size())))
                   .arg(settings.lineWeightScale, 0, 'g', 10));
    return {};
}

SheetSource MainWindow::sheetSource() const
{
    SheetSource source;
    source.plan = planSourceOf(document_);
    source.plan.reference = &reference_;
    source.plan.meshes = &sceneMeshes_;
    source.document = &document_;
    source.surfaces = sceneSurfaces_;
    // The drawing's revision, and which surfaces there are: a section cut
    // before a surface was built is cut again once there is one.
    source.revision = document_.modelRevision();
    for (const auto& surface : sceneSurfaces_) {
        source.revision = source.revision * 1'000'003u +
                          reinterpret_cast<std::uintptr_t>(surface.surface) +
                          (surface.visible ? 1u : 0u);
    }
    const auto today = std::chrono::year_month_day(
        std::chrono::floor<std::chrono::days>(std::chrono::system_clock::now()));
    source.fields = cad::plotting::fieldContextFor(document_, cad::plotting::frameDate(today));
    if (const auto logo = cad::plotting::logoPath(document_)) {
        source.logo.load(QString::fromStdWString(logo->wstring()));
    }
    if (const auto directory = document_.projectDirectory()) {
        source.assets = *directory / "assets";
    }
    return source;
}

void MainWindow::showAlignmentManager()
{
    if (!alignments_) {
        AlignmentManagerContext context;
        context.document = &document_;
        context.run = commandRunner();
        context.headless = [this] { return headless_; };
        alignments_ = std::make_unique<AlignmentManagerDialog>(std::move(context), this);
    }
    alignments_->show();
    alignments_->raise();
    alignments_->activateWindow();
}

void MainWindow::showSheets()
{
    if (!sheets_) {
        sheets_ = std::make_unique<SheetEditor>(document_, [this] { return sheetSource(); }, this);
        sheets_->onMessage = [this](const QString& text, bool isError) { logMessage(text, isError); };
        // Its dialogs' lines run as typed ones do (command_runner.hpp).
        sheets_->setCommandRunner(commandRunner());
        // Generate Sheets' "The current plan view": what the plan view shows.
        sheets_->planViewArea = [this]() -> std::optional<katana::geometry::Box2> {
            const ViewportWidget* view = views_->activePlanView();
            if (view == nullptr) {
                return std::nullopt;
            }
            const cad::ViewTransform& shown = view->viewTransform();
            const katana::geometry::Point2 a = shown.screenToWorld({0.0, 0.0});
            const katana::geometry::Point2 b = shown.screenToWorld({shown.widthPixels, shown.heightPixels});
            return katana::geometry::Box2({std::min(a.x, b.x), std::min(a.y, b.y)},
                                          {std::max(a.x, b.x), std::max(a.y, b.y)});
        };
    }
    sheets_->setHeadless(headless_);
    sheets_->show();
    sheets_->raise();
    sheets_->activateWindow();
}

katana::core::Result<cad::plotting::SheetSet> MainWindow::sheetsToPlot()
{
    cad::plotting::SheetSet set = document_.sheetSet();
    if (set.sheets.empty()) {
        // Nothing laid out yet: one sheet fitted to the drawing, for this plot.
        cad::plotting::LayoutRequest request;
        request.planArea = planDrawnBounds(sheetSource().plan, {}, {});
        auto fitted = cad::plotting::smartLayout(document_.model(), request);
        if (!fitted) {
            return fitted.error();
        }
        cad::plotting::prepareForAppend(set, *fitted);
        set.sheets = std::move(*fitted);
        logMessage("The project has no sheets: plotting one fitted to the drawing.");
    }
    return set;
}

katana::core::Result<PlotReport> MainWindow::plotSheets(const cad::plotting::SheetSet& set,
                                                        const PlotRequest& request)
{
    PlotRequest titled = request;
    if (titled.title.isEmpty()) {
        titled.title = QString::fromStdString(document_.metadata().name);
    }
    // The checks first, on the sheets this plot takes: what they find is
    // logged, and the plot goes ahead - an error there is paper wasted, not
    // a file that cannot be written. A selection that does not parse is
    // left to the plot to refuse, with its own words.
    if (const auto selected = cad::plotting::parseSheetSelection(titled.sheets, set)) {
        for (const QString& line : preflightLog(checkSheetsFor(set, sheetSource(), *selected))) {
            logMessage(line);
        }
    }
    SheetPaintCache cache;
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto result = katana::qt::plotSheets(set, titled, [this] { return sheetSource(); }, cache);
    QApplication::restoreOverrideCursor();
    if (!result) {
        return result.error();
    }
    for (const std::string& problem : result->problems) {
        logMessage(QString::fromStdString(problem), true);
    }
    logMessage(result->summary(titled));
    return result;
}

katana::core::Status MainWindow::plotSheetsToPdf(const QString& path)
{
    const auto set = sheetsToPlot();
    if (!set) {
        return set.error();
    }
    PlotRequest request = plotRequestFor(set->pageSetup, path);
    request.format = PlotFormat::Pdf;
    if (auto result = plotSheets(*set, request); !result) {
        return result.error();
    }
    return {};
}

} // namespace katana::qt
