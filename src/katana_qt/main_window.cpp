#include "main_window.hpp"

#include "katana/core/cpu_features.hpp"
#include "katana/core/path_text.hpp"
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
#include "geo/references.hpp"
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
#include "survey_verbs.hpp" // katana_app: SURVEY READ and SURVEY IMPORT
#include "property_panel.hpp"
#include "render_view_widget.hpp"
#include "style_manager.hpp"
#include "tools/flyout_button.hpp"

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
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QCheckBox>
#include <QClipboard>
#include <QInputDialog>
#include <QPainter>
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
#include <QStandardPaths>
#include <QRegularExpression>
#include <QScreen>
#include <QGuiApplication>
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
#include <tuple>
#include <utility>

#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/corridor.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/entity/curve_pieces.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_record.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/cad/view_link.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/ifc/export.hpp"
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

// Where the window keeps itself between sessions (restoreSession), in the
// QSettings the recent scripts are kept in. kLayoutVersion is saveState's
// version: raised when the toolbars or panels change so that an old layout
// would put them wrong, which makes restoreState refuse it and the window
// start as built.
constexpr const char* kGeometryKey = "window/geometry";
constexpr const char* kLayoutKey = "window/layout";
constexpr const char* kMinimisedKey = "window/minimised";
constexpr const char* kTextSizeKey = "appearance/textSize";
constexpr const char* kToolBarIconsKey = "appearance/toolBarIcons";
constexpr const char* kToolBarNamesKey = "appearance/toolBarNames";
constexpr int kLayoutVersion = 1;

// View > Text Size's items by size: viewTextSizeSmall ... ExtraLarge.
QString textSizeActionName(katana::qt::theme::TextSize size)
{
    using katana::qt::theme::TextSize;
    switch (size) {
    case TextSize::Small:
        return QStringLiteral("viewTextSizeSmall");
    case TextSize::Standard:
        return QStringLiteral("viewTextSizeStandard");
    case TextSize::Large:
        return QStringLiteral("viewTextSizeLarge");
    case TextSize::ExtraLarge:
        return QStringLiteral("viewTextSizeExtraLarge");
    }
    return QStringLiteral("viewTextSizeStandard");
}

QString fromView(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

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

// A colour as a rounded square, for a list's colour column.
QIcon colourSwatch(const QColor& colour)
{
    QPixmap pixmap(32, 32);
    pixmap.fill(Qt::transparent);
    QPainter painter(&pixmap);
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setPen(QPen(theme::border(), 2.0));
    painter.setBrush(colour);
    painter.drawRoundedRect(QRectF(3, 3, 26, 26), 6, 6);
    return QIcon(pixmap);
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
    buildWindowMenu();
    buildStatusBar();
    // A category the catalogue gains that no menu here takes is still
    // reachable by its aliases and ids on the command line; said, once the
    // log exists, rather than left to vanish from the window.
    for (const std::string& category : toolActions_.unplaced()) {
        logMessage("The tool category " + QString::fromStdString(category) +
                   " has no menu; its tools start from the command line.");
    }
    // And an item File > Import or Export lists by a name no action has
    // (fillMenuByName): an error, since the item is missing from the menu.
    for (const QString& problem : std::as_const(menuProblems_)) {
        logMessage(problem, true);
    }
    views_->setReferenceData(&reference_);
    // No colour lookup is handed to the interpreter: CODE resolves a colour
    // name through the Document - the customisation's own table, then the
    // standard names (cad/colour_lookup.hpp) - and the window has no names
    // beyond those. (It passed the standard table while cad could not see it.)
    //
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
    // VIEWS and ZOOM (cad/view_verbs.hpp) act on this window's views; the
    // controls on a view's bar build those lines and run them through the
    // one executor, so a click is logged as the line it is.
    interpreter_.setViewHost([this] { return &views_->verbHost(); });
    views_->setCommandRunner(commandRunner());
    views_->onLinksChanged = [this] { refreshViewMenu(); };
    views_->onViewSettingsChanged = [this] { refreshViewMenu(); };

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
        coordinateLabel_->setText(planPoint(world));
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
        if (change.has(cad::DocumentChange::Drafting)) {
            syncSnapActions();
        }
    });
    refreshAll();
    views_->stopTool();
    selectAction_->setChecked(true);
    // What View > Reset Window Layout goes back to: this window as built.
    defaultLayout_ = saveState(kLayoutVersion);
    logMessage("Katana ready. Type HELP for the command list.");
}

// The object snap's items show the document's drafting settings, which the
// drafting toolbar and the SNAP verb set too (docs/drawing.md). Blocked, so
// showing a setting does not set it again from inside the notification.
void MainWindow::syncSnapActions()
{
    if (snapAction_ != nullptr) {
        const QSignalBlocker block(snapAction_);
        snapAction_->setChecked(views_->snapEnabled());
    }
    for (QAction* each : findChildren<QAction*>()) {
        if (!each->objectName().startsWith("viewSnapMode")) {
            continue;
        }
        const auto mode = each->data();
        if (!mode.isValid()) {
            continue;
        }
        const QSignalBlocker block(each);
        each->setChecked((views_->snapModes() & mode.toUInt()) != 0);
    }
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
    toolbar->setIconSize(QSize(toolBarIconSize_, toolBarIconSize_));
    toolbar->setToolButtonStyle(Qt::ToolButtonIconOnly);
    toolbar->setMovable(true);
    toolbar->setFloatable(false);
    // Its name before its buttons, muted, while it lies along the top or the
    // bottom: two rows of icon-only buttons read as one undivided strip, and
    // which icons are Survey's and which Terrain's was a matter of hovering
    // over each. The Properties bar is left out: its " Style " label already
    // says what it holds. A vertical bar has no room for a word.
    QAction* name = nullptr;
    if (title != "Properties") {
        auto* caption = new QLabel(title, toolbar);
        caption->setObjectName("toolBarName");
        caption->setToolTip(QString("<b>%1</b><br>The %1 toolbar. View &gt; Toolbars shows, "
                                    "hides and sizes the toolbars.")
                                .arg(title.toHtmlEscaped()));
        name = toolbar->addWidget(caption);
    }
    toolBars_.emplace_back(toolbar, name);
    QAction* toggle = toolbar->toggleViewAction();
    toggle->setObjectName("viewToolBar" + QString(title).remove(' '));
    toggle->setIcon(katana::qt::icon(Icon::Toolbars));
    toggle->setStatusTip("Show or hide the " + title + " toolbar");
    connect(toolbar, &QToolBar::orientationChanged, this, [this] { refreshToolBarNames(); });
    addToolBar(area, toolbar);
    refreshToolBarNames();
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
    // "Any File", where it was "Import...": it heads a submenu of imports
    // now, and is the one that takes whatever it is given, routed by what the
    // file holds. Its letter is A: I is the submenu's in File and Import
    // Survey Points' inside it.
    QAction* importAction =
        makeAction(Icon::Import, "Import &Any File...",
                   "Import a drawing, an image or a point cloud (DXF, IFC, SHP, GeoTIFF, LAS ...)",
                   QKeySequence(Qt::CTRL | Qt::Key_I), "fileImport");
    // "Drawing", where it was "Vector": it writes DXF, archives and IFC as
    // well as the GIS vector formats, and what it takes is the drawing. Shown
    // in GIS too, the same object.
    QAction* exportAction =
        makeAction(Icon::Export, "Export Dra&wing...",
                   "Export the drawing, or a scope of it, to a file: DXF, an archive, IFC 4.3 or "
                   "a GIS vector format (GeoPackage, GeoJSON, SHP ...)",
                   QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_E), "fileExportVector");
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
    // IFC 4.3 both ways (docs/ifc.md), each a dialog of its own: what an IFC
    // exchange chooses - which objects, a utility schedule and its schema,
    // a project's classification rules - has no place in the generic ones.
    QAction* importIfcAction =
        makeAction(Icon::Import, "Import I&FC...",
                   "Import an IFC file: alignments as PIs and PVIs, elements and annotations with "
                   "their property sets, terrain as surfaces",
                   QKeySequence(), "fileImportIfc");
    QAction* exportIfcAction = makeAction(
        Icon::Export, "Export I&FC...",
        "Export IFC 4.3: alignments, the drawing by class and an AS 5488 utility investigation, "
        "with a preview of the class each object becomes",
        QKeySequence(), "fileExportIfc");
    connect(importIfcAction, &QAction::triggered, this, [this] { showIfcImport(); });
    connect(exportIfcAction, &QAction::triggered, this, [this] { showIfcExport(); });
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

    // Shown under Survey > Survey Coding and on the Survey toolbar. (Load
    // Customisation and Replace Loaded Customisation were made here too, for
    // Format and Survey: they opened a file dialog over another program's
    // style libraries and survey code files, and went with those. A Katana
    // customisation file is loaded in File > Settings, or by the CUSTOMISE
    // line.)
    QAction* codeAction = makeAction(Icon::Import, "Appl&y Survey Codes",
                                     "Give the selected entities that carry a field code - every "
                                     "one in the drawing when nothing is selected - the layer, "
                                     "style and attributes the loaded survey codes say: the CODE "
                                     "line, one undo step",
                                     {}, "applySurveyCodes");
    connect(codeAction, &QAction::triggered, this, [this] { applySurveyCodes(); });
    // Beside it under Survey > Survey Coding: what is done with coded points
    // next. It was one tab of the Survey Code Manager and nothing else.
    QAction* lineworkAction =
        makeAction(Icon::SurveyLinework, "P&rocess Linework",
                   "Join the selected coded points into lines - every coded point in the "
                   "drawing when nothing is selected - by their codes and the linework control "
                   "codes: the LINEWORK line, one undo step",
                   {}, "surveyLinework");
    connect(lineworkAction, &QAction::triggered, this, [this] { surveyLinework(); });

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

    // File is the drawing as a file, what comes into it and goes out of it,
    // and the program's own settings: fifteen items in titled sections, as
    // the long menus all are (a style that draws titles, theme.cpp, says what
    // each group is for).
    //
    // EVERY import and every export is under one of two submenus here,
    // whichever menu also shows it: File's own (any file, IFC, a view's
    // picture), then Survey's and GIS's under their names. A person looking
    // for "how do I get this file in" looks in File first, and imports were
    // in three menus. The items are the SAME QAction objects the Survey and
    // GIS menus show - never a second action that runs the first, which would
    // be a second text, tip and enabled state to drift - and are put in by
    // their object names at the END of this function, once those menus have
    // made them (fillMenuByName).
    //
    // The managers of what a customisation brings stay in Format and Survey >
    // Survey Coding; the customisation as a whole - load a file, write one,
    // reset, keep - is File > Settings, above Quit.
    fileMenu->addSection("Drawing");
    fileMenu->addActions({newAction, openAction, saveAction, saveAsAction});
    fileMenu->addSection("Import and Export");
    QMenu* importMenu = fileMenu->addMenu("&Import");
    importMenu->setObjectName("fileImportMenu");
    importMenu->setIcon(katana::qt::icon(Icon::Import));
    importMenu->menuAction()->setStatusTip(
        "Bring a file into the drawing: any file as it comes, IFC, survey points, or GIS data "
        "with its options");
    QMenu* exportMenu = fileMenu->addMenu("&Export");
    exportMenu->setObjectName("fileExportMenu");
    exportMenu->setIcon(katana::qt::icon(Icon::Export));
    exportMenu->menuAction()->setStatusTip(
        "Write the drawing, a view's picture, survey points, a surface or a point cloud to a "
        "file");
    fileMenu->addSection("Scripts");
    fileMenu->addAction(runScriptAction);
    recentScriptsMenu_ = fileMenu->addMenu("Recen&t Scripts");
    recentScriptsMenu_->setObjectName("fileRecentScripts");
    recentScriptsMenu_->setIcon(katana::qt::icon(Icon::RecentScripts));
    recentScriptsMenu_->menuAction()->setStatusTip("Run one of the last scripts run again");
    refreshRecentScripts();
    fileMenu->addSection("Plot");
    fileMenu->addActions({plotAction, sheetsAction, plotSheetsAction});
    fileMenu->addSection("Project");
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
    quitAction->setIcon(katana::qt::icon(Icon::Quit));
    quitAction->setStatusTip("Close Katana, asking first about anything unsaved");

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
    deselectAction->setIcon(katana::qt::icon(Icon::Deselect));
    deselectAction->setStatusTip("Clear the selection, and end the running tool");
    // The ids LIST, INFO and AREA print, back into a selection
    // (select_by_id_dialog.hpp); non-modal and kept, as Format > Layers is.
    QAction* selectByIdAction =
        editMenu->addAction("Select by &ID...", this, [this] { showSelectById(); });
    selectByIdAction->setObjectName("editSelectById");
    selectByIdAction->setIcon(katana::qt::icon(Icon::SelectById));
    selectByIdAction->setStatusTip("Select entities by the ids LIST and INFO print, and find them");
    selectByIdAction->setData(QStringLiteral("selectByIdDialog")); // for --dialog
    editMenu->addAction(eraseAction);
    QAction* copyImageAction = editMenu->addAction(
        "Copy &View as Image", this, [this] { (void)runVerbLine("SNAPSHOT CLIPBOARD"); });
    copyImageAction->setObjectName("editCopyViewImage");
    copyImageAction->setIcon(katana::qt::icon(Icon::CopyViewImage));
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
    attributesAction->setIcon(katana::qt::icon(Icon::Attributes));
    attributesAction->setStatusTip("The selection's attributes as a tree: add, change, rename "
                                   "and remove them, each one undo step");

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
    const auto historyButton = [editBar](QAction* action, const char* name, const char* menuName) {
        auto* button = new QToolButton(editBar);
        button->setObjectName(name);
        button->setDefaultAction(action);
        button->setPopupMode(QToolButton::MenuButtonPopup);
        button->setAutoRaise(true);
        auto* menu = new QMenu(button);
        menu->setObjectName(menuName);
        button->setMenu(menu);
        editBar->addWidget(button);
        tools::followToolBarIconSize(*button, *editBar);
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
    // The zoom items run their ZOOM lines for the active view through the one
    // executor, as each view's bar runs them for itself (cad/view_verbs.hpp):
    // logged, and followed by the views linked with it.
    QAction* extentsAction = makeAction(Icon::ZoomExtents, "Zoom &Extents",
                                        "Fit the whole drawing in the view (ZOOM EXTENTS)",
                                        QKeySequence(Qt::CTRL | Qt::Key_E), "viewZoomExtents");
    connect(extentsAction, &QAction::triggered, this,
            [this] { (void)runVerbLine("ZOOM EXTENTS"); });
    QAction* zoomInAction = makeAction(Icon::ZoomIn, "Zoom &In",
                                       "Twice as close about the centre of the active view (ZOOM IN)",
                                       {}, "viewZoomIn");
    connect(zoomInAction, &QAction::triggered, this, [this] { (void)runVerbLine("ZOOM IN"); });
    QAction* zoomOutAction = makeAction(Icon::ZoomOut, "Zoom &Out",
                                        "Twice as far about the centre of the active view (ZOOM OUT)",
                                        {}, "viewZoomOut");
    connect(zoomOutAction, &QAction::triggered, this, [this] { (void)runVerbLine("ZOOM OUT"); });
    QAction* zoomSelectionAction =
        makeAction(Icon::ZoomSelection, "Zoom to Sele&ction",
                   "Frame what is selected in the active view (ZOOM SELECTION)", {},
                   "viewZoomSelection");
    connect(zoomSelectionAction, &QAction::triggered, this,
            [this] { (void)runVerbLine("ZOOM SELECTION"); });
    QAction* zoomToAction = makeAction(
        Icon::ZoomTo, "&Zoom To...",
        "Frame what a scope takes - layers, a filter, the selection - in a view (ZOOM <scope>)", {},
        "viewZoomTo");
    // What the headless --dialog step opens it by (main.cpp, openDialog).
    zoomToAction->setData(QStringLiteral("zoomToDialog"));
    connect(zoomToAction, &QAction::triggered, this, [this] { showZoomTo(); });
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

    viewMenu_->addSection("Display");
    viewMenu_->addAction(extentsAction);
    viewMenu_->addActions({zoomInAction, zoomOutAction, zoomSelectionAction, zoomToAction});
    viewMenu_->addActions({gridAction_, snapAction_});
    // Plan-view lines as a cosmetic pixel instead of the 1.5 px hairline:
    // measured 5-8x cheaper to stroke (docs/plan_view.md), so on by default,
    // with the heavier look one click away. Plots are unaffected.
    QAction* thinLinesAction = viewMenu_->addAction("&Thin screen lines (faster)");
    thinLinesAction->setObjectName("ThinScreenLinesAction");
    thinLinesAction->setIcon(katana::qt::icon(Icon::ThinLines));
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

    // Beside Object Snap, which it refines.
    auto* snapMenu = new QMenu("Snap &Modes", viewMenu_);
    snapMenu->setObjectName("viewSnapModes");
    snapMenu->setIcon(katana::qt::icon(Icon::Snap));
    snapMenu->menuAction()->setStatusTip("Which points Object Snap finds: ends, middles, "
                                         "centres, crossings and more");
    viewMenu_->insertMenu(thinLinesAction, snapMenu);
    // Each mode with the mark the plan view shows for it, and what it finds.
    struct SnapItem {
        cad::SnapMode mode;
        Icon icon;
        const char* tip;
    };
    for (const SnapItem& item : {
             SnapItem{cad::SnapMode::Endpoint, Icon::SnapEndpoint,
                      "Snap to the ends of lines, arcs and polyline segments"},
             SnapItem{cad::SnapMode::Midpoint, Icon::SnapMidpoint,
                      "Snap to the middle of a line, an arc or a segment"},
             SnapItem{cad::SnapMode::Center, Icon::SnapCenter,
                      "Snap to the centre of a circle or an arc"},
             SnapItem{cad::SnapMode::Intersection, Icon::SnapIntersection,
                      "Snap to where two entities cross"},
             SnapItem{cad::SnapMode::Perpendicular, Icon::SnapPerpendicular,
                      "Snap to the foot of the perpendicular from the last point"},
             SnapItem{cad::SnapMode::Tangent, Icon::SnapTangent,
                      "Snap to the point where a line from the last point touches a circle"},
             SnapItem{cad::SnapMode::Nearest, Icon::SnapNearest,
                      "Snap to the nearest point on any entity"},
             SnapItem{cad::SnapMode::Grid, Icon::SnapGrid, "Snap to the grid's points"}}) {
        const cad::SnapMode mode = item.mode;
        QAction* action = snapMenu->addAction(katana::qt::icon(item.icon), cad::toString(mode));
        action->setStatusTip(item.tip);
        // viewSnapModeEndpoint ... viewSnapModeGrid: what --trigger reaches
        // it by, and what SNAP <mode> ON|OFF sets (dispatchLine).
        action->setObjectName(QString("viewSnapMode") + cad::toString(mode));
        action->setData(static_cast<uint>(static_cast<cad::SnapModes>(mode)));
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
    buildFormatActions(*formatMenu, layersAction);
    // Annotate > Edit Text...: the annotation workbench's, which exists only
    // from here, after the tools filled the menu.
    annotation_->addEditTextAction(*annotateMenu);
    // Annotate > Edit Label... and Label Layout Report..., after the
    // catalogue's tools: the annotation workbench's, which Format made.
    annotation_->addLabelActions(*annotateMenu);
    // The Leaders manager's entries under the Annotate tools, made by the
    // annotation workbench buildFormatActions has just built.
    annotation_->addLeaderActions(*annotateMenu);

    // ---- Survey ------------------------------------------------------------------------
    buildSurveyActions(*surveyMenu, codeAction, lineworkAction);

    // ---- Terrain and civil -----------------------------------------------------------
    QAction* cloudSurface = makeAction(Icon::SurfaceFromCloud, "Surface From &Point Cloud...",
                                       "Triangulate a surface from an imported point cloud's "
                                       "ground returns",
                                       {}, "surfaceFromPointCloud");
    QAction* rasterSurface = makeAction(Icon::SurfaceFromRaster, "Surface From &Raster...",
                                        "Triangulate a surface from an elevation raster's true "
                                        "values, read through GDAL",
                                        {}, "surfaceFromRaster");
    QAction* drawingSurface = makeAction(Icon::SurfaceFromDrawing, "Surface From &Drawing...",
                                         "Triangulate a surface from the levelled points and lines "
                                         "of the drawing, a scope of it or a filter",
                                         {}, "surfaceFromDrawing");
    // One dialog for the three, on their own source: what --dialog finds.
    for (QAction* surfaceFrom : {cloudSurface, rasterSurface, drawingSurface}) {
        surfaceFrom->setData(QString("surfaceFromDialog"));
    }
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
    connect(cloudSurface, &QAction::triggered, this,
            [this] { showSurfaceFrom(SurfaceFromKind::Cloud); });
    connect(rasterSurface, &QAction::triggered, this,
            [this] { showSurfaceFrom(SurfaceFromKind::Raster); });
    connect(drawingSurface, &QAction::triggered, this,
            [this] { showSurfaceFrom(SurfaceFromKind::Drawing); });
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

    // ---- File, finished ---------------------------------------------------------------
    // Last, when every menu has made its actions. Settings is the Format
    // workbench's (it owns the dialog), shown above Quit; and File's two
    // submenus take File's own items and then Survey's and GIS's by name.
    fileMenu->insertAction(quitAction, format_->settingsAction());
    fillMenuByName(
        *importMenu,
        {{"", {"fileImport", "fileImportIfc"}},
         {"Survey", {"surveyImport"}},
         {"GIS", {"importVectorData", "importRaster", "importPointCloud", "onlineData"}}});
    fillMenuByName(*exportMenu,
                   {{"", {"fileExportVector", "fileExportIfc", "fileExportViewImage"}},
                    {"Survey", {"surveyExport"}},
                    {"GIS", {"exportSurfaceDem", "exportPointCloud"}}});
}

void MainWindow::fillMenuByName(QMenu& menu, const std::vector<MenuPart>& parts)
{
    const QString where = "File > " + QString(menu.title()).remove('&');
    for (const MenuPart& part : parts) {
        if (*part.section != '\0') {
            menu.addSection(QString::fromLatin1(part.section));
        }
        for (const char* name : part.items) {
            auto* action = findChild<QAction*>(QString::fromLatin1(name));
            if (action == nullptr) {
                menuProblems_ << where + ": no menu item is named " + QString::fromLatin1(name) +
                                     ", so it is left out of this menu.";
                continue;
            }
            menu.addAction(action);
        }
    }
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
    exportDem->setData(QString("surfaceRasterDialog"));
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
        return chooseProjectCrs(this, document_, place, commandRunner());
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
    geo.scratch = katana::app::geo::ownScratch();
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
    geo.views = views_;
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

void MainWindow::buildSurveyActions(QMenu& surveyMenu, QAction* codeAction,
                                    QAction* lineworkAction)
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
    services.applySurveyCodes = codeAction;
    services.surveyLinework = lineworkAction;
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

void MainWindow::buildFormatActions(QMenu& formatMenu, QAction* layersAction)
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
    services.run = commandRunner();
    // Asked at each commit of an editor, and when File > Settings is first
    // opened: the host is made after the window is built
    // (loadDefaultCustomisation). The path is shown with '/', as every path
    // the window shows is.
    services.hasKeptFile = [this] { return !keptCustomisationFile_.empty(); };
    services.hasBuiltIn = [this] { return hasBuiltInCustomisation_; };
    services.keptFile = [this] {
        return QDir::fromNativeSeparators(fromPath(keptCustomisationFile_));
    };
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
    if (id == "draw.vertex.edit") {
        drawingUi_.showVertices(); // the polyline it picks is shown there
    }
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

QStringList MainWindow::menuGaps(int* items) const
{
    QStringList gaps;
    int counted = 0;
    const std::function<void(const QMenu&, const QString&)> walk = [&](const QMenu& menu,
                                                                     const QString& path) {
        for (const QAction* action : menu.actions()) {
            if (action->isSeparator() || !action->isVisible()) {
                continue;
            }
            const QString where = path + " > " + QString(action->text()).remove('&');
            ++counted;
            QStringList missing;
            if (action->icon().isNull()) {
                missing << "no icon";
            }
            if (action->statusTip().isEmpty()) {
                missing << "no status tip";
            }
            if (!missing.isEmpty()) {
                gaps << where + ": " + missing.join(", ");
            }
            if (const QMenu* sub = action->menu()) {
                walk(*sub, where);
            }
        }
    };
    for (const QAction* top : menuBar()->actions()) {
        if (const QMenu* menu = top->menu()) {
            walk(*menu, QString(menu->title()).remove('&'));
        }
    }
    if (items != nullptr) {
        *items = counted;
    }
    return gaps;
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
        context.run = commandRunner();
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
    layerTree_->setHeaderLabels({"Layer", "", "", "", "N"});
    // Icons over the narrow columns, their words in the tooltips: "On",
    // "Lock" and "Colour" set those columns' widths, and the layer names
    // beside them were cut to their first three letters in a 300 px panel.
    QTreeWidgetItem* header = layerTree_->headerItem();
    for (const auto& [column, glyph, tip] :
         std::initializer_list<std::tuple<int, Icon, const char*>>{
             {kVisible, Icon::LayerVisible, "On: the layer is shown"},
             {kLocked, Icon::LayerLocked, "Lock: the layer's entities cannot be picked or changed"},
             {kColor, Icon::LayerColour, "Colour: double-click a swatch to change it"}}) {
        header->setIcon(column, katana::qt::icon(glyph));
        header->setToolTip(column, tip);
    }
    header->setToolTip(kCount, "N: how many entities are on the layer");
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
    // The selection as a tree read a level at a time (property_panel.hpp):
    // a surveyed string's thousands of vertex attributes cost nothing until
    // they are opened.
    propertyTree_ = new PropertyTreePanel(propertyPanel);
    propertyLayout->addWidget(propertyTree_, 1);
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
    commandLog_->setFont(theme::monospaceFont());
    commandInput_ = new QLineEdit(commandPanel);
    commandInput_->setObjectName("commandInput");
    // A point larger than the log: it is where the eye is while typing.
    commandInput_->setFont(theme::monospaceFont(1.0));
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
    for (QDockWidget* dock : {layerDock, propertyDock, commandDock, referenceDock_}) {
        dock->toggleViewAction()->setStatusTip("Show or hide the " + dock->windowTitle() +
                                               " panel");
    }
    viewMenu_->addSection("Window");
    QMenu* panels = viewMenu_->addMenu("&Panels");
    panels->setObjectName("viewPanels");
    panels->setIcon(katana::qt::icon(Icon::Panels));
    panels->menuAction()->setStatusTip("Show or hide the panels: Layers, Properties, Command Line, "
                                       "Reference Data and Vertices");
    panels->addActions({layerDock->toggleViewAction(), propertyDock->toggleViewAction(),
                        commandDock->toggleViewAction(), referenceDock_->toggleViewAction()});
    drawingUi_ = drawing::installDrawingUi(*this, document_, panels,
                                           findChild<QMenu*>("drawMenu"));

    // Opening sizes. Left to itself Qt gives each dock its size hint, which
    // for a text log is a third of the window - so the drawing, which is the
    // point of the program, opened in the space left over. The drawing gets
    // the room; the panels get what they need to be read.
    resizeDocks({layerDock, propertyDock}, {300, 300}, Qt::Horizontal);
    resizeDocks({commandDock}, {150}, Qt::Vertical);
}

void MainWindow::buildReferenceDock()
{
    // The Reference Data panel builds REFS lines and runs them through the
    // window's one executor (docs/interop.md, "Reference layers"), as a
    // person or an agent would type them: each is logged, and does what
    // katana_cli's does. Object names: referenceList, referenceShow,
    // referenceHide, referenceRemove, referenceInfo, referenceOverviews,
    // referenceOpacity, referenceColour; ReferenceImportButton and
    // ReferenceZoomButton.
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
    auto* showButton = panelTool(panel, Icon::Layers, "referenceShow", "Show",
                                 "Show the selected reference layer (REFS SHOW).");
    auto* hideButton = panelTool(panel, Icon::ViewLayersFiltered, "referenceHide", "Hide",
                                 "Hide the selected reference layer (REFS HIDE).");
    auto* infoButton = panelTool(panel, Icon::DatasetInfo, "referenceInfo", "Information",
                                 "The selected layer's source and display (REFS INFO), and what its "
                                 "file holds, as GDAL or PDAL reads it.");
    auto* overviewsButton = panelTool(
        panel, Icon::Processing, "referenceOverviews", "Build Overviews",
        "Build overviews for the selected raster, written beside its file as .ovr (REFS "
        "OVERVIEWS ... CONFIRM), so it is read quickly at every scale.");
    auto* removeButton = panelTool(panel, Icon::Erase, "referenceRemove", "Remove",
                                   "Remove the selected reference layer (REFS REMOVE). The file "
                                   "is not touched.");
    layout->addLayout(toolRow({importButton, zoomButton, showButton, hideButton, infoButton,
                               overviewsButton, removeButton}));

    referenceTable_ = new QTableWidget(0, kRefColumns, panel);
    referenceTable_->setObjectName("referenceList");
    referenceTable_->setHorizontalHeaderLabels({"Name", "Type", "Detail", "Display"});
    referenceTable_->horizontalHeader()->setStretchLastSection(true);
    referenceTable_->verticalHeader()->setVisible(false);
    referenceTable_->setSelectionBehavior(QAbstractItemView::SelectRows);
    referenceTable_->setSelectionMode(QAbstractItemView::SingleSelection);
    layout->addWidget(referenceTable_);

    // How the selected layer is drawn: a raster's opacity, a cloud's colour.
    auto* display = new QHBoxLayout();
    referenceOpacity_ = new QComboBox(panel);
    referenceOpacity_->setObjectName("referenceOpacity");
    referenceOpacity_->addItems({"100%", "75%", "50%", "25%"});
    referenceOpacity_->setToolTip("How much of the selected raster shows (REFS OPACITY)");
    referenceColour_ = new QComboBox(panel);
    referenceColour_->setObjectName("referenceColour");
    using Mode = katana::interop::PointColorMode;
    for (const Mode mode : {Mode::Elevation, Mode::Intensity, Mode::Classification,
                            Mode::SourceColor, Mode::Flat}) {
        referenceColour_->addItem(katana::interop::toString(mode),
                                  QString::fromLatin1(katana::interop::toWord(mode)));
    }
    referenceColour_->setToolTip("How the selected point cloud is coloured (REFS COLOR)");
    display->addWidget(new QLabel("Opacity", panel));
    display->addWidget(referenceOpacity_);
    display->addWidget(new QLabel("Colour", panel));
    display->addWidget(referenceColour_, 1);
    layout->addLayout(display);

    dock->setWidget(panel);
    addDockWidget(Qt::LeftDockWidgetArea, dock);

    connect(importButton, &QToolButton::clicked, this, [this] { importFile(); });
    connect(zoomButton, &QToolButton::clicked, this, [this] { zoomToSelectedReference(); });
    connect(showButton, &QToolButton::clicked, this, [this] { runReferenceLine("SHOW"); });
    connect(hideButton, &QToolButton::clicked, this, [this] { runReferenceLine("HIDE"); });
    connect(removeButton, &QToolButton::clicked, this, [this] { removeSelectedReference(); });
    // The layer's own facts, then what its SOURCE holds - which, for a
    // cloud, is the whole file and not the sample the panel shows.
    connect(infoButton, &QToolButton::clicked, this, [this] {
        const std::optional<katana::interop::ReferenceId> id = selectedReference();
        if (!id) {
            logMessage("Select a reference layer first.", true);
            return;
        }
        std::filesystem::path source;
        if (const auto* raster = reference_.findRaster(*id)) {
            source = raster->source;
        } else if (const auto* cloud = reference_.findPointCloud(*id)) {
            source = cloud->source;
        }
        if (!runReferenceLine("INFO").ok) {
            return;
        }
        if (auto dialog = makeDatasetInfo(fromPath(source))) {
            dialog->exec();
        }
    });
    connect(overviewsButton, &QToolButton::clicked, this, [this] {
        const std::optional<katana::interop::ReferenceId> id = selectedReference();
        const auto* raster = id ? reference_.findRaster(*id) : nullptr;
        if (raster == nullptr) {
            logMessage("Select a raster first: overviews are a raster's.", true);
            return;
        }
        // Writing beside a person's file is asked for: CONFIRM is the
        // answer. A headless run has nobody to ask, so its line goes without
        // it, and the verb says what it would write.
        std::filesystem::path ovr = raster->source;
        ovr += ".ovr";
        const bool confirmed =
            !headless_ && QMessageBox::question(this, "Build Overviews",
                                                "Write the overviews of '" +
                                                    QString::fromStdString(raster->name) +
                                                    "' beside its file, as\n" + fromPath(ovr) +
                                                    "?") == QMessageBox::Yes;
        if (!headless_ && !confirmed) {
            return;
        }
        (void)runReferenceLine("OVERVIEWS", confirmed ? QStringLiteral("CONFIRM") : QString());
    });
    connect(referenceOpacity_, &QComboBox::currentIndexChanged, this, [this](int chosen) {
        if (refreshingReferences_) {
            return;
        }
        static constexpr const char* kValues[] = {"1", "0.75", "0.5", "0.25"};
        (void)runReferenceLine("OPACITY", QString::fromLatin1(kValues[std::clamp(chosen, 0, 3)]));
    });
    connect(referenceColour_, &QComboBox::currentIndexChanged, this, [this](int) {
        if (refreshingReferences_) {
            return;
        }
        (void)runReferenceLine("COLOR", referenceColour_->currentData().toString());
    });
    connect(referenceTable_, &QTableWidget::cellChanged, this,
            [this](int row, int column) { onReferenceCellChanged(row, column); });
    connect(referenceTable_, &QTableWidget::cellDoubleClicked, this,
            [this](int, int) { zoomToSelectedReference(); });
    connect(referenceTable_, &QTableWidget::itemSelectionChanged, this,
            [this] { showSelectedReferenceDisplay(); });
    showSelectedReferenceDisplay();
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
    // File > Export IFC, open beside the drawing: its counts and selection
    // follow the drawing, as the panels do.
    if (!ifcExport_.isNull() && ifcExport_->isVisible()) {
        ifcExport_->refresh();
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

        // A swatch, with its code in the tooltip: the code written out in the
        // cell took seventy pixels of a 300 px column, and the layer names
        // beside it were cut to "AN..." and "BO...".
        const QString hex = QString::fromStdString(layer->color.toHex());
        item->setIcon(kColor, colourSwatch(QColor(layer->color.r, layer->color.g, layer->color.b)));
        item->setData(kColor, Qt::AccessibleTextRole, hex);
        item->setToolTip(kColor, QString("<b>%1</b><br>Double-click to change the layer's "
                                         "colour.")
                                     .arg(hex));
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

void MainWindow::refreshProperties() { propertyTree_->showSelection(document_); }

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
    // Opened on a grip, the grip's items: its tools start with it hot, the
    // handle a click on it before the tool would have made
    // (ToolContext::handles).
    if (ViewportWidget* plan = views_->activePlanView()) {
        context.grip = plan->gripAt(QPointF(plan->mapFromGlobal(globalPos)));
        const QPointer<ViewportWidget> view(plan);
        context.startToolOn = [this, view](const std::string& toolId,
                                           const katana::cad::Grip& grip) {
            if (view != nullptr) {
                view->gripController().setHot({grip});
            }
            startTool(toolId);
        };
    }
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
        projectCrs_ =
            std::make_unique<ProjectCrsDialog>(document_, place, this, commandRunner());
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

void MainWindow::showZoomTo()
{
    if (!zoomTo_) {
        ZoomToContext context;
        context.document = &document_;
        context.views = views_;
        context.run = commandRunner();
        zoomTo_ = std::make_unique<ZoomToDialog>(std::move(context), this);
    }
    zoomTo_->refresh();
    zoomTo_->show();
    zoomTo_->raise();
    zoomTo_->activateWindow();
}

void MainWindow::showSelection(bool zoom)
{
    // Framed in the view the person is working in, as the style manager's
    // Select Users frames what a style covers; the other views keep theirs,
    // but for the views linked with it. By the view's Zoom to Selection
    // line, through the one executor, so the log says what moved the view.
    ViewportWidget* plan = views_->activePlanView();
    if (zoom && plan != nullptr) {
        views_->zoomToSelection(plan->state().id);
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
    restoreReferences();
}

bool MainWindow::saveDocument()
{
    if (!document_.hasProject()) {
        return saveDocumentAs();
    }
    recordReferences();
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
    recordReferences();
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
        // Only a window a person used: a headless run leaves no layout behind.
        if (!headless_) {
            saveSession();
        }
    } else {
        event->ignore();
    }
}

// ---- the window between sessions -------------------------------------------------------

void MainWindow::buildWindowMenu()
{
    QMenu* bars = viewMenu_->addMenu("Tool&bars");
    bars->setObjectName("viewToolBars");
    bars->setIcon(katana::qt::icon(Icon::Toolbars));
    bars->menuAction()->setStatusTip(
        "Show or hide each toolbar, choose their icons' size, and whether they show their names");
    bars->addSection("Show");
    for (const auto& [toolbar, name] : toolBars_) {
        bars->addAction(toolbar->toggleViewAction());
    }
    bars->addSection("Buttons");
    QAction* names = bars->addAction(katana::qt::icon(Icon::Rename), "Show Toolbar &Names");
    names->setObjectName("viewToolBarNames");
    names->setCheckable(true);
    names->setChecked(toolBarNamesShown_);
    names->setStatusTip("Show each toolbar's name before its buttons, so the groups are told "
                        "apart without hovering over them");
    connect(names, &QAction::toggled, this, [this](bool on) { setToolBarNamesShown(on); });
    auto* iconGroup = new QActionGroup(this);
    for (const auto& [pixels, text, name] :
         std::initializer_list<std::tuple<int, const char*, const char*>>{
             {16, "&Small Icons", "viewToolBarIconsSmall"},
             {20, "S&tandard Icons", "viewToolBarIconsStandard"},
             {28, "&Large Icons", "viewToolBarIconsLarge"}}) {
        QAction* action = bars->addAction(katana::qt::icon(Icon::Toolbars), text);
        action->setObjectName(name);
        action->setCheckable(true);
        action->setChecked(pixels == toolBarIconSize_);
        action->setData(pixels);
        action->setStatusTip(QString("Toolbar icons %1 pixels square").arg(pixels));
        iconGroup->addAction(action);
        connect(action, &QAction::triggered, this, [this, size = pixels] {
            setToolBarIconSize(size);
        });
    }

    QMenu* text = viewMenu_->addMenu("Te&xt Size");
    text->setObjectName("viewTextSize");
    text->setIcon(katana::qt::icon(Icon::TextSize));
    text->menuAction()->setStatusTip(
        "How large the window's text is: menus, panels, dialogs and the command line");
    auto* textGroup = new QActionGroup(this);
    for (const auto& [size, label] : std::initializer_list<std::pair<theme::TextSize, const char*>>{
             {theme::TextSize::Small, "&Small"},
             {theme::TextSize::Standard, "S&tandard"},
             {theme::TextSize::Large, "&Large"},
             {theme::TextSize::ExtraLarge, "&Extra Large"}}) {
        QAction* action = text->addAction(katana::qt::icon(Icon::TextSize), label);
        action->setObjectName(textSizeActionName(size));
        action->setCheckable(true);
        action->setChecked(size == theme::textSize());
        const int steps = theme::textSizeSteps(size);
        action->setStatusTip(
            steps == 0 ? QString("The platform's own text size")
                       : QString("The platform's text size %1 %2 point%3")
                             .arg(steps > 0 ? "and" : "less")
                             .arg(std::abs(steps))
                             .arg(std::abs(steps) == 1 ? "" : "s"));
        textGroup->addAction(action);
        connect(action, &QAction::triggered, this, [this, chosen = size] { setTextSize(chosen); });
    }

    QAction* reset = viewMenu_->addAction(katana::qt::icon(Icon::ResetLayout),
                                          "Reset &Window Layout");
    reset->setObjectName("viewResetLayout");
    reset->setStatusTip("Put the panels and toolbars back where a new window has them");
    connect(reset, &QAction::triggered, this, [this] { resetWindowLayout(); });
}

void MainWindow::refreshToolBarNames()
{
    for (const auto& [toolbar, name] : toolBars_) {
        if (name != nullptr) {
            name->setVisible(toolBarNamesShown_ && toolbar->orientation() == Qt::Horizontal);
        }
    }
}

void MainWindow::setToolBarNamesShown(bool shown)
{
    toolBarNamesShown_ = shown;
    refreshToolBarNames();
    if (auto* action = findChild<QAction*>("viewToolBarNames"); action != nullptr) {
        const QSignalBlocker quiet(action);
        action->setChecked(shown);
    }
    if (!headless_) {
        QSettings().setValue(kToolBarNamesKey, shown);
    }
}

void MainWindow::setToolBarIconSize(int pixels)
{
    toolBarIconSize_ = std::clamp(pixels, 12, 48);
    // The buttons each bar made for its actions follow it, and so do the
    // ones put on it as widgets - the tool families, Undo and Redo
    // (tools::followToolBarIconSize).
    for (const auto& [toolbar, name] : toolBars_) {
        toolbar->setIconSize(QSize(toolBarIconSize_, toolBarIconSize_));
    }
    for (QAction* action : findChildren<QAction*>()) {
        if (action->objectName().startsWith("viewToolBarIcons")) {
            const QSignalBlocker quiet(action);
            action->setChecked(action->data().toInt() == toolBarIconSize_);
        }
    }
    if (!headless_) {
        QSettings().setValue(kToolBarIconsKey, toolBarIconSize_);
    }
}

void MainWindow::setTextSize(theme::TextSize size)
{
    theme::setTextSize(*qApp, size);
    // The fonts set on widgets of their own, which the application font
    // does not reach: the command line's fixed pitch.
    commandLog_->setFont(theme::monospaceFont());
    commandInput_->setFont(theme::monospaceFont(1.0));
    for (QAction* action : findChildren<QAction*>()) {
        if (action->objectName().startsWith("viewTextSize")) {
            const QSignalBlocker quiet(action);
            action->setChecked(action->objectName() == textSizeActionName(size));
        }
    }
    if (!headless_) {
        QSettings().setValue(kTextSizeKey, fromView(theme::toString(size)));
    }
    logMessage("Text size: " + fromView(theme::toString(size)) + ".");
}

void MainWindow::fitToScreen()
{
    const QScreen* screen = this->screen() != nullptr ? this->screen() : QGuiApplication::primaryScreen();
    if (screen == nullptr) {
        return;
    }
    const QRect free = screen->availableGeometry();
    // Below 1440 x 900 (a 1366 x 768 laptop, a 1280 x 800 one) the fixed
    // 1360 x 860 window ran off the screen: maximised, the drawing gets every
    // pixel there is.
    if (free.width() < 1440 || free.height() < 900) {
        resize(free.size());
        move(free.topLeft());
        setWindowState(windowState() | Qt::WindowMaximized);
        return;
    }
    // 85%: large enough that the drawing is the window, small enough that
    // it is plainly a window a person can move, with the desktop around it.
    const QSize size(static_cast<int>(free.width() * 0.85), static_cast<int>(free.height() * 0.85));
    resize(size);
    move(free.center() - QPoint(size.width() / 2, size.height() / 2));
}

void MainWindow::restoreSession()
{
    QSettings settings;
    // The appearance first: the sizes below are laid out in it.
    if (const auto size =
            theme::textSizeFrom(settings.value(kTextSizeKey).toString().toStdString())) {
        if (*size != theme::textSize()) {
            setTextSize(*size);
        }
    }
    if (const int pixels = settings.value(kToolBarIconsKey, toolBarIconSize_).toInt();
        pixels != toolBarIconSize_) {
        setToolBarIconSize(pixels);
    }
    setToolBarNamesShown(settings.value(kToolBarNamesKey, toolBarNamesShown_).toBool());

    // Qt puts a window that was on a screen no longer there back onto one.
    if (!restoreGeometry(settings.value(kGeometryKey).toByteArray())) {
        fitToScreen();
    }
    // A layout saved by another version, or none, leaves the window as built.
    if (restoreState(settings.value(kLayoutKey).toByteArray(), kLayoutVersion)) {
        const QStringList unknown =
            chrome_->minimiseNamed(settings.value(kMinimisedKey).toStringList());
        (void)unknown; // a view of the last session: views are not kept
        refreshToolBarNames();
    }
}

void MainWindow::saveSession() const
{
    QSettings settings;
    settings.setValue(kGeometryKey, saveGeometry());
    settings.setValue(kLayoutKey, saveState(kLayoutVersion));
    settings.setValue(kMinimisedKey, chrome_->minimisedNames());
}

void MainWindow::resetWindowLayout()
{
    // Minimised panels back first, so their tray buttons go with them.
    for (QDockWidget* dock : {layerDock_, propertyDock_, commandDock_, referenceDock_}) {
        chrome_->restore(dock);
    }
    restoreState(defaultLayout_, kLayoutVersion);
    refreshToolBarNames();
    logMessage("The panels and toolbars are back where a new window has them.");
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

katana::core::Result<std::string> MainWindow::pointerAt(const QString& spec, bool click)
{
    const QStringList parts = spec.split(',');
    const auto coordinate = [&](int i) {
        return i < parts.size() ? katana::core::parseFiniteDouble(parts[i].trimmed().toStdString())
                                : std::nullopt;
    };
    const auto x = coordinate(0);
    const auto y = coordinate(1);
    Qt::KeyboardModifiers modifiers = Qt::NoModifier;
    bool understood = x.has_value() && y.has_value() && parts.size() <= 3;
    if (understood && parts.size() == 3) {
        const QString key = parts[2].trimmed().toLower();
        if (key == "shift") {
            modifiers = Qt::ShiftModifier;
        } else if (key == "ctrl") {
            modifiers = Qt::ControlModifier;
        } else {
            understood = false;
        }
    }
    if (!understood) {
        return katana::core::makeError(katana::core::ErrorCode::ParseFailure,
                                       "a pointer step is x,y or x,y,shift or x,y,ctrl",
                                       spec.toStdString());
    }
    return views_->pointerAt(katana::geometry::Point2(*x, *y), click, modifiers);
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
    // What is typed is echoed - but an ONLINE KEY's value never is, and a
    // ZOOM typed while a tool runs is echoed by the executor that runs it
    // (ViewWorkspace::runsTransparently): echoed here as well, it showed twice.
    if (!views_->runsTransparently(line)) {
        commandLog_->appendPlainText("> " + OnlineDataWorkbench::loggedLine(line));
    }
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
    // SURVEY READ and SURVEY IMPORT: the session's verb (survey_verbs.hpp),
    // so the window, katana_cli and katana_mcp read and import a field file
    // by one function and reply alike.
    if (katana::app::isSurveyLine(line.toStdString())) {
        const auto reply = katana::app::runSurveyLine(document_, line.toStdString());
        if (reply) {
            logMessage(QString::fromStdString(*reply));
        } else {
            logMessage(QString::fromStdString(reply.error().describe()), true);
        }
        return true;
    }
    // A .ifc's IMPORT, EXPORT and INFO, and IFC RULES, are IFC's grammar
    // before anything else, as in the session (session.cpp): the geo
    // executor below takes IMPORT, EXPORT and INFO of every other file, and
    // would read an IFC line's options as a path or GDAL's options.
    if (const QStringList words = line.split(' ', Qt::SkipEmptyParts); !words.isEmpty()) {
        const QString verb = words.front().toUpper();
        if ((verb == "IMPORT" || verb == "EXPORT" || verb == "INFO" || verb == "IFC") &&
            runIfcLine(verb, line.mid(line.indexOf(words.front()) + words.front().size()))
                .has_value()) {
            return true;
        }
    }
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

    // ZOOM (Z) is the interpreter's now (cad/view_verbs.hpp), answered by the
    // workspace: it was taken here, and whatever followed it was ignored.
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
    // SNAP with the drafting verb's options (modes=, add=, remove=) is the
    // interpreter's (docs/drawing.md); the window keeps SNAP [ON|OFF] and
    // SNAP <mode> [ON|OFF] as its shorthand for View > Snap Modes.
    if ((verb == "SNAP" || verb == "OSNAP") &&
        std::any_of(words.begin() + 1, words.end(),
                    [](const QString& word) { return word.contains('='); })) {
        runInterpreterLine(line, verb);
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
                       "Center, Intersection, Perpendicular, Tangent, Nearest, Grid | "
                       "SNAP [on|off] [modes=a,b|all|none] [add=a,b] [remove=a,b] for every "
                       "mode (HELP)",
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
    // CUSTOMISE is not here: it is the interpreter's
    // (cad/customisation_verbs.hpp), and reaches it at the end of this
    // function as CODE does. The window had a tokenizer and a loader of its
    // own for it, beside katana_cli's, and the two had come to differ.
    //
    // IMPORT, EXPORT, INFO <file>, REFS and COPC are not here: they are the
    // executor katana_cli and katana_mcp run (src/katana_app/geo), which the
    // geo workbench runs as jobs (runWorkbenchLine). INFO <id> is left to the
    // interpreter by the executor itself (geo::takesInfo).
    //
    // A .ifc takes the verbs' options exactly as katana_cli reads them
    // (ifc/front_end.hpp), so its line is split by that grammar before the
    // generic one takes the rest as a path; IFC RULES is IFC's alone.
    if ((verb == "IMPORT" || verb == "EXPORT" || verb == "INFO" || verb == "IFC") &&
        runIfcLine(verb, line.mid(words.front().size())).has_value()) {
        return;
    }
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
    // Not an OPEN of polylines (OPEN #12, OPEN SELECTION): that is an edit.
    const bool replacesDocument =
        katana::cad::CommandInterpreter::replacesDocument(line.toStdString());
    if (replacesDocument && !confirmDiscard()) {
        return;
    }
    // As File > Save does - but only for a SAVE that has somewhere to go:
    // writing the reference layers marks the drawing modified, and an
    // untitled SAVE with no directory fails, which left a drawing nobody
    // touched asking to be saved. (The customisation record needs none of
    // this: the save itself writes it, Document::save.)
    if (verb == "SAVE" &&
        katana::cad::typedSaveHasDestination(line.toStdString(), document_.hasProject())) {
        recordReferences();
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
    if (replacesDocument && verb == "OPEN") {
        reportMissingCustomisation();
        restoreReferences();
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
        item->setIcon(katana::qt::icon(Icon::RecentScripts));
        item->setStatusTip("Run " + QDir::toNativeSeparators(path));
    }
    if (recent.isEmpty()) {
        QAction* none = recentScriptsMenu_->addAction(katana::qt::icon(Icon::RecentScripts), "(none)");
        none->setStatusTip("No script has been run yet: File > Run Script runs one");
        none->setEnabled(false);
        return;
    }
    recentScriptsMenu_->addSeparator();
    QAction* clear = recentScriptsMenu_->addAction("&Clear the List", this, [this] {
        QSettings().remove(kRecentScriptsKey);
        refreshRecentScripts();
    });
    clear->setObjectName("recentScriptsClear");
    clear->setIcon(katana::qt::icon(Icon::Erase));
    clear->setStatusTip("Forget the scripts listed here; the files are not touched");
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

// The vector and raster extensions are GDAL's registry's (interop::
// vectorExtensions, docs/interop.md "Formats"), so the filters offer what
// this build opens; a file picked through "All files" is still routed by
// what it holds. An archive (.zip, .tar.gz) is opened by its inside.
QString importFilter()
{
    const QString vector = patternsFor(interop::vectorExtensions());
    const QString raster = patternsFor(interop::rasterExtensions());
    const QString cloud = patternsFor(interop::pointCloudExtensions());
    const QString archive = patternsFor(interop::archive12dExtensions());
    const QString zipped = patternsFor(interop::archiveExtensions());
    return "All supported (" + vector + " *.ifc " + archive + ' ' + raster + ' ' + cloud + ' ' +
           zipped + ");;" + "Vector (" + vector + ");;" + "IFC (*.ifc);;" + "12d Archive (" +
           archive + ");;" + "Raster (" + raster + ");;" + "Point cloud (" + cloud + ");;" +
           "Zipped GIS data (" + zipped + ");;" + "All files (*)";
}

} // namespace

void MainWindow::importPath(const QString& path, const cad::ImportPlacement& placement)
{
    const std::filesystem::path file = toPath(path);
    if (katana::ifc::isIfcPath(file)) {
        // As the line it is: echoed, and answered as a typed one is.
        katana::ifc::ImportArguments arguments;
        arguments.path = path.toStdString();
        if (const auto line = katana::ifc::formatImportLine(arguments)) {
            (void)runIfcCommand(QString::fromStdString(*line), IfcLineFrom::Menu);
        } else {
            logMessage(QString::fromStdString(line.error().describe()), true);
        }
        return;
    }
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

// Every name, quoted: what a project was drawn with that is not loaded is a
// short list, and each of its names is one a person has to go and find.
QString quotedNames(const std::vector<std::string>& names)
{
    QStringList quoted;
    for (const std::string& name : names) {
        quoted << "\"" + QString::fromStdString(name) + "\"";
    }
    return quoted.join(", ");
}

// "1 survey code rule", "11 survey code rules", "1,624 survey code rules":
// the count grouped, and the noun by it. A customisation of one rule was
// said to have "1 survey code rules".
QString countOf(std::size_t count, const char* one, const char* many)
{
    return grouped(count) + ' ' + QLatin1String(count == 1 ? one : many);
}

} // namespace

// What the session starts with (cad/customisation_host.hpp): the
// customisation the user kept, else the one built into the program, else
// nothing - a build with no built-in has none, and then draws plain
// lines until a customisation is loaded. cad chooses and installs; the window
// builds the host and says what was installed.
//
// The built-in is cad's for THIS RUN (builtInCustomisation, which reads the
// seam KATANA_BUILTIN_CUSTOMISATION, so a test starts the same on a machine
// whose build has one and on one whose build has not).
//
// THE KEPT FILE, in order:
//   1. the file the environment variable KATANA_CUSTOMISATION names, whatever
//      kind of session this is;
//   2. else, in an INTERACTIVE session, customisation.json in the per-user
//      place Qt gives the application (QStandardPaths::AppConfigLocation -
//      where the online sources a person adds are kept too, gis_online.cpp);
//   3. else none: a HEADLESS run with the variable unset. It must be the same
//      run on every machine, so it reads and writes no per-user place - as it
//      keeps no window layout - and CUSTOMISE KEEP and REVERT are then
//      refused, naming the variable.
// The file need not exist: nothing is kept until a KEEP writes it, and the
// folder is made then. This is called after setHeadless for rule 3.
void MainWindow::loadDefaultCustomisation()
{
    cad::CustomisationHost host;
    host.builtIn = cad::builtInCustomisation();
    // Read as the session of katana_cli and katana_mcp reads it
    // (core::environmentVariable: wide on Windows, UTF-8 out), so a kept file
    // in a folder whose name is outside the code page is still found - and so
    // that two front ends do not read one variable two ways.
    const std::string kept = katana::core::environmentVariable(cad::kKeptCustomisationVariable);
    if (!kept.empty()) {
        host.keptFile = katana::core::pathFromUtf8(kept);
    } else if (!headless_) {
        // Empty where the platform has no such place: the session then keeps
        // none, as a headless one does.
        const QString place = QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation);
        if (!place.isEmpty()) {
            host.keptFile = toPath(place + "/customisation.json");
        }
    }
    // What the Format workbench asks: before an editor's own commit runs
    // CUSTOMISE KEEP, and for File > Settings to show
    // (CustomisationServices::hasKeptFile, keptFile, hasBuiltIn).
    keptCustomisationFile_ = host.keptFile;
    hasBuiltInCustomisation_ = host.builtIn.customisation != nullptr;
    // The interpreter first: it notes the kept file as it is NOW, which is
    // what CUSTOMISE KEEP later refuses to write over once it has changed
    // (CommandInterpreter::setCustomisationHost).
    interpreter_.setCustomisationHost(host);
    const cad::CustomisationStart start = cad::startCustomisation(document_, host);
    // A built-in that does not parse, a kept file that does not read: said,
    // once, here. Unsaid, a damaged file drew every drawing's linestyles as
    // plain lines without a word (audit A12-06).
    for (const std::string& problem : start.problems) {
        logMessage("Customisation: " + QString::fromStdString(problem), true);
    }
    if (start.installed == cad::CustomisationOrigin::None) {
        return;
    }
    logMessage("Customisation: " + QString::fromStdString(start.name) + ", " +
               countOf(start.definitions, "definition", "definitions") + " (" +
               countOf(start.symbols, "symbol", "symbols") + ") and " +
               countOf(start.rules, "survey code rule", "survey code rules") + ", " +
               (start.installed == cad::CustomisationOrigin::Kept ? "kept." : "built in."));
    if (start.keptFromAnotherBuiltIn) {
        // Installed all the same - it is the user's - but they may want to
        // know that the program's own has moved on since they kept theirs.
        logMessage("The kept customisation was made from another built-in customisation than "
                   "this program has; CUSTOMISE RESET gives this program's.");
    }
}

void MainWindow::reportMissingCustomisation()
{
    // Worked out by the open, and kept by the Document until loaded: saving
    // this drawing must not forget them.
    const std::vector<std::string>& missing = document_.customisationState().missingAtOpen;
    if (missing.empty()) {
        return;
    }
    // Not an error box: the drawing opens and draws, but what a missing
    // customisation defined draws as a plain line until it is loaded. By
    // name, as the project records them - never a path - and with the line
    // that loads one: the menu item this sentence once named is gone.
    logMessage("Warning: this project was drawn with customisations that are not loaded: " +
               quotedNames(missing) + ". CUSTOMISE <file> loads a Katana customisation file.");
}

// Survey > Apply Survey Codes: the CODE line a person would type, through the
// one executor, so it is echoed, kept in the history, one undo step and
// answered in the verb's own words - which say the property the codes were
// read from, because "no entity carries a code" and "they carry it under
// another name" look the same from outside (cad/survey_code_verbs.hpp). On
// the selection when there is one, which is also how the import wizard codes
// only the points it has just made.
void MainWindow::applySurveyCodes()
{
    (void)runVerbLine(document_.selection().empty() ? QStringLiteral("CODE DRAWING")
                                                    : QStringLiteral("CODE SELECTION"));
}

// Survey > Process Linework: the LINEWORK line, the same way and for the same
// reasons - one undo step, answered in the verb's records (what the scope
// took, each line strung, each reason a point is in none;
// cad/linework_verbs.hpp). LINEWORK with no scope word chooses the selection
// or the drawing itself; the item writes the word, so that the echoed line in
// the log says which this click was.
void MainWindow::surveyLinework()
{
    (void)runVerbLine(document_.selection().empty() ? QStringLiteral("LINEWORK DRAWING")
                                                    : QStringLiteral("LINEWORK SELECTION"));
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
    if (katana::ifc::isIfcPath(path)) {
        katana::ifc::ImportArguments arguments;
        arguments.path = selected.toStdString();
        if (const auto line = katana::ifc::formatImportLine(arguments)) {
            (void)runIfcCommand(QString::fromStdString(*line), IfcLineFrom::Menu);
        } else {
            logMessage(QString::fromStdString(line.error().describe()), true);
        }
        return;
    }
    const interop::SourceKind kind = interop::kindForPath(path);
    if (katana::dxf::isDxfPath(path) || kind == interop::SourceKind::Archive12d) {
        importWithPlacement(selected);
        return;
    }
    if (kind != interop::SourceKind::Unknown) {
        (void)runVerbLine(importLine(selected, {}));
        return;
    }
    // Routed by what the file holds, so nothing recognised it: not GDAL, and
    // not by name. The formats are too many to list in a message box; GIS >
    // Formats lists them, as FORMATS does.
    warnUser("Import", "Neither GDAL nor Katana recognises the data in\n" + selected +
                           "\n\nGIS > Formats lists what this build of GDAL reads (FORMATS READ); "
                           "archives (" + patternsFor(interop::archive12dExtensions()) +
                           ") and point clouds (" + patternsFor(interop::pointCloudExtensions()) +
                           ") are read too.");
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

    // Every vector writer of GDAL's registry, the common ones first
    // (interop::vectorExportFormats, docs/interop.md "Formats").
    QStringList filters;
    for (const interop::FormatChoice& format : interop::vectorExportFormats()) {
        filters << (QString::fromStdString(format.description) + " (*." +
                    QString::fromStdString(format.extension) + ")");
    }
    filters << "12d Archive (*.12da)" << "12d Archive, zipped (*.12daz)" << "IFC 4.3 (*.ifc)";
    QString chosenFilter;
    QString selected = QFileDialog::getSaveFileName(this, "Export", QString(),
                                                    filters.join(";;"), &chosenFilter);
    if (selected.isEmpty()) {
        return;
    }
    // A name typed without an extension takes the chosen format's: the
    // extension is what picks the writer, and none is refused.
    if (const qsizetype pattern = chosenFilter.lastIndexOf("(*.");
        pattern >= 0 && chosenFilter.endsWith(')') && QFileInfo(selected).suffix().isEmpty()) {
        selected += '.' + chosenFilter.mid(pattern + 3).chopped(1);
    }
    // IFC has choices of its own - a utility schedule, rules, which objects
    // go - so its dialog takes the file from here.
    if (katana::ifc::isIfcPath(toPath(selected))) {
        showIfcExport(selected);
        return;
    }
    (void)showExportOptions(selected);
}

QDialog* MainWindow::showExportOptions(const QString& selected)
{
    // File > Export > Export Drawing's dialog (gis_export_dialog.hpp): its
    // scope and options are the EXPORT line it shows, and Run hands that line to the
    // one executor, as if typed (docs/interop.md, "Export options"). One per
    // window, made the first time and kept, as every GIS dialog is.
    // Found by its name, as addGisToolAction finds a GIS dialog: the class
    // has no Q_OBJECT for findChild to cast by.
    auto* dialog = dynamic_cast<VectorExportDialog*>(findChild<QDialog*>("vectorExportDialog"));
    if (dialog == nullptr) {
        GisDialogContext context;
        context.run = commandRunner();
        context.document = &document_;
        context.headless = [this] { return headless_; };
        if (views_ != nullptr) {
            context.views = [this] { return scopeFilterViews(views_->viewSet()); };
        }
        dialog = new VectorExportDialog(std::move(context), this);
        if (geo_ != nullptr) {
            QPointer<VectorExportDialog> guard(dialog);
            (void)geo_->addFinishedListener([guard](JobId id, const VerbOutcome& outcome) {
                if (guard != nullptr) {
                    guard->jobFinished(id, outcome);
                }
            });
        }
    }
    dialog->reload();
    dialog->setFile(QDir::fromNativeSeparators(selected));
    // What is selected is what is usually meant, when there is a selection;
    // else the whole drawing.
    if (ScopeFilterWidget* scope = dialog->scopeControls()) {
        scope->setChoice(document_.selection().empty() ? ScopeChoice::Drawing
                                                       : ScopeChoice::Selection);
    }
    dialog->show();
    dialog->raise();
    dialog->activateWindow();
    return dialog;
}

void MainWindow::refreshReferences()
{
    if (referenceTable_ == nullptr) {
        return;
    }
    // Kept across the rebuild, so a line run on the selected layer leaves it
    // selected for the next.
    const std::optional<katana::interop::ReferenceId> kept = selectedReference();
    refreshingReferences_ = true;
    referenceTable_->setRowCount(0);

    const auto addRow = [this](katana::interop::ReferenceId id, const QString& name, bool visible,
                               const QString& type, const QString& detail, const QString& shown) {
        const int row = referenceTable_->rowCount();
        referenceTable_->insertRow(row);

        auto* nameItem = new QTableWidgetItem(name);
        nameItem->setFlags((nameItem->flags() | Qt::ItemIsUserCheckable) & ~Qt::ItemIsEditable);
        nameItem->setCheckState(visible ? Qt::Checked : Qt::Unchecked);
        nameItem->setData(Qt::UserRole, QVariant::fromValue<qulonglong>(id));
        referenceTable_->setItem(row, kRefName, nameItem);

        for (const auto& [column, text] : {std::pair(kRefType, type), std::pair(kRefDetail, detail),
                                           std::pair(kRefDisplay, shown)}) {
            auto* item = new QTableWidgetItem(text);
            item->setFlags(item->flags() & ~Qt::ItemIsEditable);
            referenceTable_->setItem(row, column, item);
        }
    };

    for (const katana::interop::RasterOverlay& raster : reference_.rasters()) {
        QString detail = QString::number(raster.width) + " x " + QString::number(raster.height) +
                         " px";
        if (!raster.hasGeotransform) {
            detail += "  (not georeferenced)";
        }
        addRow(raster.id, QString::fromStdString(raster.name), raster.visible, "Raster", detail,
               QString::number(qRound(raster.opacity * 100.0)) + "%");
    }
    for (const katana::interop::PointCloudLayer& cloud : reference_.pointClouds()) {
        QString detail = grouped(cloud.points.size()) + " pts";
        if (cloud.isDecimated()) {
            detail += " of " + grouped(cloud.sourcePointCount);
        }
        addRow(cloud.id, QString::fromStdString(cloud.name), cloud.visible, "Point cloud", detail,
               katana::interop::toString(cloud.colorMode));
    }
    for (int row = 0; kept && row < referenceTable_->rowCount(); ++row) {
        if (referenceTable_->item(row, kRefName)->data(Qt::UserRole).toULongLong() == *kept) {
            referenceTable_->selectRow(row);
        }
    }

    referenceTable_->resizeColumnsToContents();
    refreshingReferences_ = false;
    showSelectedReferenceDisplay();
}

std::optional<katana::interop::ReferenceId> MainWindow::selectedReference() const
{
    const int row = referenceTable_ != nullptr ? referenceTable_->currentRow() : -1;
    const QTableWidgetItem* item = row < 0 ? nullptr : referenceTable_->item(row, kRefName);
    if (item == nullptr) {
        return std::nullopt;
    }
    return static_cast<katana::interop::ReferenceId>(item->data(Qt::UserRole).toULongLong());
}

void MainWindow::showSelectedReferenceDisplay()
{
    if (referenceOpacity_ == nullptr || referenceColour_ == nullptr) {
        return;
    }
    // Set from the layer, not a choice: no line is run for it.
    const bool wasRefreshing = std::exchange(refreshingReferences_, true);
    const std::optional<katana::interop::ReferenceId> id = selectedReference();
    const auto* raster = id ? reference_.findRaster(*id) : nullptr;
    const auto* cloud = id ? reference_.findPointCloud(*id) : nullptr;
    referenceOpacity_->setEnabled(raster != nullptr);
    referenceColour_->setEnabled(cloud != nullptr);
    if (raster != nullptr) {
        referenceOpacity_->setCurrentIndex(raster->opacity > 0.875   ? 0
                                           : raster->opacity > 0.625 ? 1
                                           : raster->opacity > 0.375 ? 2
                                                                     : 3);
    }
    if (cloud != nullptr) {
        referenceColour_->setCurrentIndex(
            referenceColour_->findData(QString::fromLatin1(katana::interop::toWord(cloud->colorMode))));
    }
    refreshingReferences_ = wasRefreshing;
}

VerbOutcome MainWindow::runReferenceLine(const QString& action, const QString& words)
{
    const std::optional<katana::interop::ReferenceId> id = selectedReference();
    if (!id) {
        logMessage("Select a reference layer first.", true);
        return {false, {}, "no reference layer is selected"};
    }
    return runVerbLine("REFS " + action + " " + QString::number(*id) +
                       (words.isEmpty() ? QString() : " " + words));
}

void MainWindow::recordReferences()
{
    if (geo_ != nullptr) {
        katana::app::geo::recordReferences(geo_->context());
    }
}

void MainWindow::restoreReferences()
{
    // The layers the project records, read again through the one executor
    // (REFS RESTORE), so what was restored, and what was missing, is logged.
    if (geo_ != nullptr && katana::app::geo::recordsReferences(geo_->context())) {
        (void)runVerbLine(QStringLiteral("REFS RESTORE"));
    }
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
    const QString line = QString("REFS %1 %2")
                             .arg(item->checkState() == Qt::Checked ? "SHOW" : "HIDE")
                             .arg(id);
    // Run once the table has finished with the click: the line's reply
    // rebuilds the table, and with it the item this signal came from.
    QMetaObject::invokeMethod(this, [this, line] { (void)runVerbLine(line); },
                              Qt::QueuedConnection);
}

void MainWindow::removeSelectedReference()
{
    (void)runReferenceLine("REMOVE");
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
    // The job a line started says so in its reply, as a record
    // (startedJob, geo/geo_workbench.hpp).
    const std::optional<JobId> job = startedJob(started.reply);
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
    if (katana::ifc::isIfcPath(toPath(path))) {
        // Described first, as the GIS files are: a file that cannot be read
        // has no dialog, and says why.
        auto described = describeIfcFile(toPath(path));
        if (!described) {
            logMessage(QString::fromStdString(described.error().describe()), true);
            warnUser("Import", QString::fromStdString(described.error().describe()));
            return nullptr;
        }
        auto dialog = std::make_unique<IfcImportDialog>(ifcImportContext(), this);
        dialog->setFile(path);
        dialog->setSummary(*described);
        return dialog;
    }
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
    case interop::SourceKind::Vector: {
        // Preview runs its line through the one executor and waits for the
        // job, as every GIS dialog does; the scope offers this drawing's
        // layers and the workspace's views.
        GisDialogContext context;
        context.run = commandRunner();
        context.await = [this](const VerbOutcome& started,
                               std::function<void(const VerbOutcome&)> done) {
            return awaitJob(started, std::move(done));
        };
        context.document = &document_;
        context.headless = [this] { return headless_; };
        if (views_ != nullptr) {
            context.views = [this] { return scopeFilterViews(views_->viewSet()); };
        }
        return std::make_unique<VectorImportDialog>(
            *description, document_.model().entities.bounds(), std::move(context), this);
    }
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
    if (katana::ifc::isIfcPath(toPath(path))) {
        showIfcImport(path); // IFC's own options, non-modally
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
    finishImport(*dialog);
}

void MainWindow::finishImport(const QDialog& dialog)
{
    // Routed by the dialog the file got, which was routed by what the file
    // IS - a .las chosen through Import Vector's "All files" still arrives
    // as a point cloud. Each dialog's choices are the IMPORT line it shows,
    // run through the one executor as if typed: echoed, logged, one undo step
    // (docs/interop.md, "Import options").
    katana::core::Result<QString> line =
        katana::core::makeError(katana::core::ErrorCode::InvalidArgument, "no import line");
    if (const auto* vector = dynamic_cast<const VectorImportDialog*>(&dialog)) {
        vector->placementBox().remember();
        line = vector->command();
    } else if (const auto* raster = dynamic_cast<const RasterImportDialog*>(&dialog)) {
        line = raster->command();
    } else if (const auto* cloud = dynamic_cast<const PointCloudImportDialog*>(&dialog)) {
        line = cloud->command();
    }
    if (!line) {
        logMessage(QString::fromStdString(line.error().describe()), true);
        return;
    }
    (void)runVerbLine(*line);
}

// The three imports and Dataset Information open a file dialog first, and a
// headless session has nobody to close one: --trigger importRaster waited on
// it until the run's timeout. Each is refused there as File's own items are
// (refuseFileDialog), naming the line that does the same and the words of it
// that are this kind of data's (docs/interop.md, "Import options"); the
// dialog itself is reached headless with --import-options or --dataset-info.
void MainWindow::importVectorWithOptions()
{
    if (refuseFileDialog(
            "IMPORT <file> [layers=a,b] [where=\"<filter>\"] [target=<layer>]")) {
        return;
    }
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Import Vector Data", QString(),
        "Vector data (" + patternsFor(interop::vectorExtensions()) + ' ' +
            patternsFor(interop::archiveExtensions()) + ");;All files (*)");
    if (!chosen.isEmpty()) {
        importWithOptions(chosen);
    }
}

void MainWindow::importRasterWithOptions()
{
    if (refuseFileDialog("IMPORT <file> [band=N] [maxpixels=N] [name=<n>]")) {
        return;
    }
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Import Raster", QString(),
        "Raster (" + patternsFor(interop::rasterExtensions()) + ' ' +
            patternsFor(interop::archiveExtensions()) + ");;All files (*)");
    if (!chosen.isEmpty()) {
        importWithOptions(chosen);
    }
}

void MainWindow::importPointCloudWithOptions()
{
    if (refuseFileDialog("IMPORT <file> [budget=N] [class=N] [resolution=<m>]")) {
        return;
    }
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Import Point Cloud", QString(),
        "Point cloud (" + patternsFor(interop::pointCloudExtensions()) + ");;All files (*)");
    if (!chosen.isEmpty()) {
        importWithOptions(chosen);
    }
}

void MainWindow::showDatasetInformation()
{
    if (refuseFileDialog("INFO <file>")) {
        return;
    }
    const QString chosen = QFileDialog::getOpenFileName(
        this, "Dataset Information", QString(),
        "GIS data and point clouds (" + patternsFor(interop::vectorExtensions()) + ' ' +
            patternsFor(interop::rasterExtensions()) + ' ' +
            patternsFor(interop::pointCloudExtensions()) + ' ' +
            patternsFor(interop::archiveExtensions()) + ");;All files (*)");
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
    // A file dialog nobody could close: a headless session is pointed at the
    // verb, which asks nothing, as Convert Point Cloud to COPC is.
    if (headless_) {
        logMessage(QString("A headless session opens no file dialog: type EXPORT <file.las|.laz> "
                           "CLOUD %1 instead.")
                       .arg(cloud->id),
                   true);
        return;
    }
    // What is held is a SAMPLE when the import was budgeted, and writing it
    // out as if it were the survey would be a quiet loss of most of the data.
    // Asked here; the verb says so in its reply, since a line has nobody to
    // ask.
    if (cloud->isDecimated()) {
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
    QString chosen =
        QFileDialog::getSaveFileName(this, "Export Point Cloud",
                                     QString::fromStdString(cloud->name) + ".laz",
                                     filters.join(";;"));
    if (chosen.isEmpty()) {
        return;
    }
    // The extension is what makes the line a cloud's: a name typed without
    // one would be the drawing's export, refused for want of a driver.
    if (!chosen.endsWith(".las", Qt::CaseInsensitive) &&
        !chosen.endsWith(".laz", Qt::CaseInsensitive)) {
        chosen += ".laz";
    }
    // The dialogs chose the cloud and the file; the writing is the EXPORT
    // verb's, run as if typed (docs/interop.md, "The GIS menu"), so it is
    // logged as one and katana_cli's and an agent's EXPORT is the same code.
    // The cloud by its id: a name several layers share would be refused.
    const VerbOutcome outcome = runVerbLine(
        QString("EXPORT \"%1\" CLOUD %2").arg(QDir::fromNativeSeparators(chosen)).arg(cloud->id));
    if (!outcome.ok) {
        warnUser("Export failed", outcome.error);
        return;
    }
    // The write is a background job: a failure is known when it ends.
    (void)awaitJob(outcome, [this](const VerbOutcome& done) {
        if (!done.ok) {
            warnUser("Export failed", done.error);
        }
    });
}

void MainWindow::exportSurfaceAsDem()
{
    // The dialog writes the SURFACE EXPORT line and runs it through
    // runVerbLine; the surface is sampled and written by the verb's job.
    if (surfaceExport_ == nullptr) {
        surfaceExport_ = new SurfaceRasterDialog(terrainDialogContext(*geo_), this);
    }
    surfaceExport_->show();
    surfaceExport_->raise();
    surfaceExport_->activateWindow();
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

void MainWindow::buildViewMenu(QMenu* viewMenu)
{
    viewMenu->addSection("Viewports");

    // Every item below has an object name, so --trigger and DRIVE's '*' reach
    // it as a click does; the layouts' and the standard views' are their
    // enumerators', since their menu text ("Two: Vertical", "SW Isometric")
    // is no name.
    QMenu* layoutMenu = viewMenu->addMenu("Viewport &Layout");
    layoutMenu->setIcon(katana::qt::icon(Icon::LayoutQuad));
    layoutMenu->menuAction()->setStatusTip(
        "Arrange the open views: one, two, three or four, side by side or stacked");
    struct LayoutItem {
        cad::LayoutKind kind;
        const char* name;
        Icon icon;
        const char* tip;
    };
    for (const auto& [kind, name, glyph, tip] : std::initializer_list<LayoutItem>{
             {cad::LayoutKind::Single, "Single", Icon::LayoutSingle, "One view, the whole window"},
             {cad::LayoutKind::SplitVertical, "SplitVertical", Icon::LayoutSplitVertical,
              "Two views side by side"},
             {cad::LayoutKind::SplitHorizontal, "SplitHorizontal", Icon::LayoutSplitHorizontal,
              "Two views, one above the other"},
             {cad::LayoutKind::ThreeLeft, "ThreeLeft", Icon::LayoutThreeLeft,
              "Three views: a large one on the left, two stacked on the right"},
             {cad::LayoutKind::ThreeTop, "ThreeTop", Icon::LayoutThreeTop,
              "Three views: a wide one on top, two side by side below"},
             {cad::LayoutKind::Quad, "Quad", Icon::LayoutQuad, "Four views of equal size"}}) {
        QAction* action = layoutMenu->addAction(katana::qt::icon(glyph), cad::toString(kind));
        action->setStatusTip(tip);
        action->setObjectName(QString("viewLayout") + name);
        action->setData(static_cast<int>(kind));
        connect(action, &QAction::triggered, this, [this, kind] {
            views_->arrange(kind);
            refreshViewMenu();
        });
        layoutActions_.push_back(action);
    }

    QMenu* kindMenu = viewMenu->addMenu("Active Viewport S&hows");
    kindMenu->setIcon(katana::qt::icon(Icon::View3D));
    kindMenu->menuAction()->setStatusTip(
        "What the active view shows: the plan, the model in 3D, a section or an elevation");
    struct KindItem {
        cad::ViewKind kind;
        Icon icon;
        const char* tip;
    };
    for (const auto& [kind, glyph, tip] : std::initializer_list<KindItem>{
             {cad::ViewKind::Plan, Icon::ViewPlan, "Show the plan in the active view"},
             {cad::ViewKind::Model3D, Icon::View3D, "Show the model in 3D in the active view"},
             {cad::ViewKind::Section, Icon::ViewSection,
              "Show the section cut last in the active view"},
             {cad::ViewKind::Elevation, Icon::ViewElevation,
              "Show an elevation of the model in the active view"}}) {
        QAction* action = kindMenu->addAction(katana::qt::icon(glyph), cad::toString(kind));
        action->setStatusTip(tip);
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

    // Linked views (cad/view_link.hpp): the Link button of the active plan
    // view's bar, and every view out of the link at once - the same lines,
    // through the same executor. K, not L: Viewport Layout has L.
    linkAction_ = viewMenu->addAction(katana::qt::icon(Icon::ViewLinked), "Lin&k This View");
    linkAction_->setObjectName("viewLinkActive");
    linkAction_->setCheckable(true);
    linkAction_->setShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_L));
    linkAction_->setStatusTip("Pan and zoom the active plan view together with the other linked "
                              "views, or take it out of the link (VIEWS LINK, VIEWS UNLINK)");
    connect(linkAction_, &QAction::triggered, this, [this] {
        // The ACTIVE view, as the item says, and only a kind that links
        // (refreshViewMenu disables it otherwise): it took the plan view used
        // last while a 3D view was active, and linked a view out of sight.
        const cad::ViewState* active = views_->viewSet().active();
        if (active != nullptr && cad::linkable(active->kind)) {
            views_->toggleLink(active->id);
        } else {
            logMessage("Only a plan view links, and the active view is not one. Click a plan "
                       "view, or VIEWS OPEN plan to open one.",
                       true);
        }
        refreshViewMenu();
    });
    QAction* unlinkAll = viewMenu->addAction(katana::qt::icon(Icon::ViewUnlinked),
                                             "U&nlink All Views");
    unlinkAll->setObjectName("viewUnlinkAll");
    unlinkAll->setStatusTip(
        "Take every view out of the link, so each pans and zooms on its own (VIEWS UNLINK ALL)");
    connect(unlinkAll, &QAction::triggered, this,
            [this] { (void)runVerbLine("VIEWS UNLINK ALL"); });

    // The active view's ghosts of the selection (docs/desktop.md, "The
    // selection in every view"): the box in its Layers popup, by the same
    // line through the same executor. D: every other letter of it is taken.
    ghostsAction_ = viewMenu->addAction(katana::qt::icon(Icon::LayerVisible),
                                        "Show the Selection on Hi&dden Layers");
    ghostsAction_->setObjectName("viewSelectionGhosts");
    ghostsAction_->setCheckable(true);
    ghostsAction_->setStatusTip("Draw what is selected faintly in the active view where it hides "
                                "the layer, or not at all (VIEWS SET <id> ghosts=on|off)");
    connect(ghostsAction_, &QAction::triggered, this, [this](bool on) {
        const cad::ViewId active = views_->viewSet().activeId();
        if (active == cad::kNoView) {
            logMessage("No view is open. VIEWS OPEN plan opens one.", true);
        } else {
            views_->setSelectionGhosts(active, on);
        }
        refreshViewMenu();
    });

    viewMenu->addSection("3D Views");
    QMenu* standard = viewMenu->addMenu("Standard &3D Views");
    standard->setIcon(katana::qt::icon(Icon::ViewIsoSouthWest));
    standard->menuAction()->setStatusTip(
        "Turn the 3D view to look from a side, from above or below, or from a corner");
    struct StandardItem {
        render::StandardView view;
        const char* name;
        Icon icon;
        const char* tip;
    };
    for (const auto& [view, name, glyph, tip] : std::initializer_list<StandardItem>{
             {render::StandardView::Top, "Top", Icon::ViewTop, "Look straight down on the model"},
             {render::StandardView::Bottom, "Bottom", Icon::ViewBottom,
              "Look straight up at the model from below"},
             {render::StandardView::Front, "Front", Icon::ViewFront,
              "Look north at the model, from the south"},
             {render::StandardView::Back, "Back", Icon::ViewBack,
              "Look south at the model, from the north"},
             {render::StandardView::Left, "Left", Icon::ViewLeft,
              "Look east at the model, from the west"},
             {render::StandardView::Right, "Right", Icon::ViewRight,
              "Look west at the model, from the east"},
             {render::StandardView::IsoSouthWest, "IsoSouthWest", Icon::ViewIsoSouthWest,
              "Look at the model from above its south-west corner"},
             {render::StandardView::IsoSouthEast, "IsoSouthEast", Icon::ViewIsoSouthEast,
              "Look at the model from above its south-east corner"},
             {render::StandardView::IsoNorthEast, "IsoNorthEast", Icon::ViewIsoNorthEast,
              "Look at the model from above its north-east corner"},
             {render::StandardView::IsoNorthWest, "IsoNorthWest", Icon::ViewIsoNorthWest,
              "Look at the model from above its north-west corner"}}) {
        QAction* action = standard->addAction(katana::qt::icon(glyph), render::toString(view),
                                              this, [this, view] {
            if (RenderViewWidget* renderView = views_->activeRenderView()) {
                renderView->setStandardView(view);
            } else {
                logMessage("No 3D viewport is open. Use View > Viewport Layout.", true);
            }
        });
        action->setObjectName(QString("viewStandard") + name);
        action->setStatusTip(tip);
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
    perspective->setIcon(katana::qt::icon(Icon::Perspective));
    perspective->setStatusTip(
        "Switch the active 3D view between perspective and a true-scale orthographic view");
    QAction* exaggeration = viewMenu->addAction("&Vertical Exaggeration...", this,
                                                [this] { askVerticalExaggeration(); });
    exaggeration->setObjectName("viewVerticalExaggeration");
    exaggeration->setIcon(katana::qt::icon(Icon::VerticalExaggeration));
    exaggeration->setStatusTip("Stretch heights in the 3D and section views, so a gentle "
                               "grade can be seen (EXAGGERATION)");
}

void MainWindow::refreshViewMenu()
{
    for (QAction* action : kindActions_) {
        action->setChecked(action->data().toInt() == static_cast<int>(views_->activeViewKind()));
    }
    const cad::ViewState* active = views_->viewSet().find(views_->viewSet().activeId());
    if (linkAction_ != nullptr) {
        // The active view's link, as the zoom items below follow its kind.
        const bool linkable = active != nullptr && cad::linkable(active->kind);
        linkAction_->setEnabled(linkable);
        linkAction_->setChecked(linkable && active->linked);
    }
    if (ghostsAction_ != nullptr) {
        ghostsAction_->setEnabled(active != nullptr);
        ghostsAction_->setChecked(active != nullptr && active->selectionGhosts);
    }
    // The zoom items the active view's kind takes, as its bar offers them
    // (cad::zoomTakes, ZOOM's own rule): an item offered that ZOOM would
    // refuse - a section's Zoom to Selection - is disabled, not refused.
    using Request = cad::ZoomRequest::Kind;
    for (const auto& [name, request] :
         {std::pair{"viewZoomIn", Request::In}, std::pair{"viewZoomOut", Request::Out},
          std::pair{"viewZoomSelection", Request::Scope}}) {
        if (QAction* action = findChild<QAction*>(name)) {
            action->setEnabled(active != nullptr && cad::zoomTakes(active->kind, request));
        }
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
    // Whether the store gained a surface the scene has not drawn: SURFACE
    // FROM, TO SURFACE and an import alike.
    const bool gained = std::ranges::any_of(surfaceStore_.all(), [&](const auto& named) {
        return std::ranges::none_of(sceneSurfaces_, [&](const cad::SceneSurface& shown) {
            return shown.surface == named.surface.get();
        });
    });
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
    views_->refreshAll();
    if (!gained) {
        return;
    }
    // Show it. A surface the user cannot see is not obviously a success.
    views_->ensureView(cad::ViewKind::Model3D);
    refreshViewMenu();
    if (RenderViewWidget* renderView = views_->activeRenderView()) {
        renderView->invalidateScene();
        renderView->zoomExtents();
    }
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
    logMessage(QString("Surface '%1': %2 vertices, %3 triangles, elevation %4 to %5.")
                   .arg(QString::fromStdString(sceneSurfaces_.back().name))
                   .arg(item.surface->vertexCount())
                   .arg(item.surface->triangleCount())
                   .arg(item.surface->minElevation(), 0, 'f', 3)
                   .arg(item.surface->maxElevation(), 0, 'f', 3));
}

void MainWindow::showSurfaceFrom(SurfaceFromKind kind)
{
    // The dialog writes the SURFACE FROM line and runs it through
    // runVerbLine: the triangulation is the verb's background job, the one
    // katana_cli and katana_mcp run (docs/terrain.md, "Surfaces on every
    // front end").
    if (surfaceFrom_ == nullptr) {
        surfaceFrom_ = new SurfaceFromDialog(terrainDialogContext(*geo_), this);
    }
    // Reference Data's chosen cloud or raster first, as the old items took it.
    QString chosen;
    if (const int row = referenceTable_ != nullptr ? referenceTable_->currentRow() : -1; row >= 0) {
        if (const QTableWidgetItem* item = referenceTable_->item(row, kRefName)) {
            const auto id =
                static_cast<katana::interop::ReferenceId>(item->data(Qt::UserRole).toULongLong());
            if (kind == SurfaceFromKind::Cloud && reference_.findPointCloud(id) != nullptr) {
                chosen = QString("CLOUD %1").arg(id);
            } else if (kind == SurfaceFromKind::Raster && reference_.findRaster(id) != nullptr) {
                chosen = QString("RASTER %1").arg(id);
            }
        }
    }
    surfaceFrom_->show();
    surfaceFrom_->showKind(kind, chosen);
    surfaceFrom_->raise();
    surfaceFrom_->activateWindow();
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
    } else if (std::holds_alternative<katana::geometry::CurvePolyline2>(entity->geometry) ||
               std::holds_alternative<katana::geometry::Spline2>(entity->geometry)) {
        // Along the curve's chords within a millimetre (entity::linework),
        // which a section's chainage cannot tell from the curve.
        alignment.vertices = katana::entity::linework(entity->geometry);
    } else {
        logMessage("A section must be cut along a line, a polyline or a spline.", true);
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
    text->setFont(katana::qt::theme::monospaceFont());
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
