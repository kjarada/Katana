#include <QAbstractButton>
#include <QAbstractItemView>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QAction>
#include <QDialog>
#include <QDir>
#include <QDockWidget>
#include <QDoubleSpinBox>
#include <QFileInfo>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPointer>
#include <QSpinBox>
#include <QTabWidget>
#include <QTextDocumentFragment>
#include <QTextEdit>
#include <QToolBar>
#include <QTreeView>

#include <algorithm>
#include <cstdio>
#include <functional>
#include <optional>
#include <thread>
#include <utility>

#include "icons.hpp"
#include "attribute_manager.hpp"
#include "dataset_info_dialog.hpp"
#include "layer_manager.hpp"
#include "style_manager.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "theme.hpp"
#include "main_window.hpp"
#include "plotting/plot_output.hpp"
#include "script_runner.hpp"
#if defined(KATANA_GPU_D3D11)
#include "gpu/renderer_choice.hpp"
#include "gpu/shader_compiler.hpp"
#endif

namespace {

// --dialog (and --survey-dialog, its first name): the dialog action NAME
// opens, triggered as a click does. The dialog is found by the object name
// the action carries as its data - how the Format menu's managers name theirs
// (CustomisationWorkbench) - or else the action's own plus "Dialog", as the
// Survey workbench names them. What opened is said on stderr, which is where
// a test reads it. nullptr, said, for an unknown action or one that opens no
// dialog.
QDialog* openDialog(katana::qt::MainWindow& window, const QString& name)
{
    if (const auto status = window.triggerAction(name); !status) {
        std::fprintf(stderr, "--dialog %s failed: %s\n", qPrintable(name),
                     status.error().describe().c_str());
        return nullptr;
    }
    QApplication::processEvents();
    const auto* action = window.findChild<QAction*>(name);
    const QString named = action != nullptr && !action->data().toString().isEmpty()
                              ? action->data().toString()
                              : name + "Dialog";
    auto* dialog = window.findChild<QDialog*>(named);
    if (dialog == nullptr) {
        std::fprintf(stderr, "--dialog: %s opened no dialog named %s\n", qPrintable(name),
                     qPrintable(named));
        return nullptr;
    }
    std::fprintf(stderr, "--dialog %s opened %s \"%s\"%s\n", qPrintable(name), qPrintable(named),
                 qPrintable(dialog->windowTitle()), dialog->isModal() ? " (modal)" : "");
    return dialog;
}

// --report NAME: what the widget NAME in the current target shows, on
// stderr as "NAME: text", so a test can see what a dialog SAYS - an
// explanation, a count - and not only that it painted. A label's text
// without its markup, a field's or a text box's text, a list's or a tree's
// rows (their columns joined by " | ", the rows by " ; "; a tree's depth
// first, down the rows it shows open). A NAME that is no
// widget there may be one of the window's actions - a menu item or a tool -
// reported as its text and whether it is checked, so a test can see which
// tool the menus and toolbars show running. One of the window's menus
// (formatMenu), found whatever the target, is its title and its items, each
// with the status tip it shows - the window makes an item's tooltip from the
// two - so a test can read everything a menu says without opening it. A NAME
// that is none of these may be any widget of the window's, so what a dialog
// did to the window - framing the views - is read with the dialog still the
// target. False, said, for none of these.
bool reportWidget(const QWidget& target, const QWidget& window, const QString& name)
{
    const QWidget* widget = target.findChild<QWidget*>(name);
    if (widget == nullptr) {
        widget = window.findChild<QMenu*>(name);
    }
    QString text;
    const auto* action = widget == nullptr ? window.findChild<QAction*>(name) : nullptr;
    // Last, any of the window's own widgets: the status bar's
    // FrameStatsLabel after a dialog framed the views.
    if (widget == nullptr && action == nullptr) {
        widget = window.findChild<QWidget*>(name);
    }
    if (action != nullptr) {
        text = QString(action->text()).remove('&') +
               (action->isCheckable() ? (action->isChecked() ? ", checked" : ", unchecked")
                                      : QString());
    } else if (const auto* menu = qobject_cast<const QMenu*>(widget)) {
        QStringList items;
        for (const QAction* item : menu->actions()) {
            if (item->isSeparator()) {
                continue;
            }
            QString line = QString(item->text()).remove('&');
            if (!item->statusTip().isEmpty()) {
                line += " [" + item->statusTip() + "]";
            }
            items << line;
        }
        text = QString(menu->title()).remove('&') + ": " + items.join(" ; ");
    } else if (const auto* label = qobject_cast<const QLabel*>(widget)) {
        text = QTextDocumentFragment::fromHtml(label->text()).toPlainText();
    } else if (const auto* line = qobject_cast<const QLineEdit*>(widget)) {
        text = line->text();
    } else if (const auto* box = qobject_cast<const QPlainTextEdit*>(widget)) {
        text = box->toPlainText();
    } else if (const auto* rich = qobject_cast<const QTextEdit*>(widget)) {
        text = rich->toPlainText();
    } else if (const auto* view = qobject_cast<const QAbstractItemView*>(widget);
               view != nullptr && view->model() != nullptr) {
        const QAbstractItemModel& model = *view->model();
        // A tree's rows depth first, down every row it shows open: what a
        // person reads in it (the Properties panel's groups).
        const auto* tree = qobject_cast<const QTreeView*>(view);
        QStringList rows;
        const std::function<void(const QModelIndex&)> walk = [&](const QModelIndex& parent) {
            for (int row = 0; row < model.rowCount(parent); ++row) {
                QStringList cells;
                for (int column = 0; column < model.columnCount(parent); ++column) {
                    cells << model.index(row, column, parent).data().toString();
                }
                rows << cells.join(" | ");
                const QModelIndex child = model.index(row, 0, parent);
                if (tree != nullptr && tree->isExpanded(child)) {
                    walk(child);
                }
            }
        };
        walk(QModelIndex());
        text = rows.join(" ; ");
    } else {
        std::fprintf(stderr,
                     "--report: there is no label, field, text, list, action or menu %s\n",
                     qPrintable(name));
        return false;
    }
    std::fprintf(stderr, "%s: %s\n", qPrintable(name), qPrintable(text.simplified()));
    return true;
}

// --survey-dock ACTION: the action that shows a survey dock is triggered
// (unless it is already ticked) and the dock it names in its data - the dock's
// object name - is found. nullptr, said on stderr, when either is missing.
QDockWidget* openSurveyDock(katana::qt::MainWindow& window, const QString& name)
{
    auto* action = window.findChild<QAction*>(name);
    if (action == nullptr || action->data().toString().isEmpty()) {
        std::fprintf(stderr, "--survey-dock: no action %s that names a dock\n", qPrintable(name));
        return nullptr;
    }
    if (!action->isCheckable() || !action->isChecked()) {
        action->trigger();
    }
    // Twice: the dock is shown on the first pass and refreshes itself on the
    // event loop in the second (it never rebuilds inside a signal).
    QApplication::processEvents();
    QApplication::processEvents();
    auto* dock = window.findChild<QDockWidget*>(action->data().toString());
    if (dock == nullptr) {
        std::fprintf(stderr, "--survey-dock: %s showed no dock named %s\n", qPrintable(name),
                     qPrintable(action->data().toString()));
    }
    return dock;
}

// --fill FIELD=TEXT. False, said on stderr, for a field the dialog does not
// have, one that is disabled, or a value it cannot take - a test that fills
// nothing must not pass.
bool fillField(QWidget& dialog, const QString& assignment)
{
    const qsizetype equals = assignment.indexOf('=');
    if (equals <= 0) {
        std::fprintf(stderr, "--fill needs FIELD=TEXT, not %s\n", qPrintable(assignment));
        return false;
    }
    const QString name = assignment.left(equals);
    QString text = assignment.mid(equals + 1);
    auto* widget = dialog.findChild<QWidget*>(name);
    // A person cannot type into a disabled field, and what it holds is not
    // read into the line (gridToSurface was disabled and --fill filled it,
    // silently): refused, as --press refuses a disabled button.
    if (widget != nullptr && !widget->isEnabled()) {
        std::fprintf(stderr, "--fill: %s's field %s is disabled\n", qPrintable(dialog.objectName()),
                     qPrintable(name));
        return false;
    }
    if (auto* line = qobject_cast<QLineEdit*>(widget)) {
        line->setText(text);
        return true;
    }
    if (auto* box = qobject_cast<QPlainTextEdit*>(widget)) {
        box->setPlainText(text.replace("\\n", "\n"));
        return true;
    }
    if (auto* choice = qobject_cast<QComboBox*>(widget)) {
        const int index = choice->findText(text);
        // An editable choice takes a name it does not list, as typing does:
        // that is how a picker keeps a name nothing defines.
        if (index < 0 && choice->isEditable()) {
            choice->setEditText(text);
            return true;
        }
        if (index < 0) {
            std::fprintf(stderr, "--fill: %s has no choice '%s'\n", qPrintable(name),
                         qPrintable(text));
            return false;
        }
        choice->setCurrentIndex(index);
        return true;
    }
    if (auto* spin = qobject_cast<QSpinBox*>(widget)) {
        bool ok = false;
        const int number = text.toInt(&ok);
        if (!ok || number < spin->minimum() || number > spin->maximum()) {
            std::fprintf(stderr, "--fill: %s takes a whole number from %d to %d, not '%s'\n",
                         qPrintable(name), spin->minimum(), spin->maximum(), qPrintable(text));
            return false;
        }
        spin->setValue(number);
        return true;
    }
    // A number with decimals - a plot's scale, a line weight - read as the
    // box itself reads typed text, so "0.7" means what it does in the box.
    if (auto* spin = qobject_cast<QDoubleSpinBox*>(widget)) {
        bool ok = false;
        const double number = text.toDouble(&ok);
        if (!ok || number < spin->minimum() || number > spin->maximum()) {
            std::fprintf(stderr, "--fill: %s takes a number from %g to %g, not '%s'\n",
                         qPrintable(name), spin->minimum(), spin->maximum(), qPrintable(text));
            return false;
        }
        spin->setValue(number);
        return true;
    }
    if (auto* check = qobject_cast<QCheckBox*>(widget); check != nullptr &&
                                                         (text == "on" || text == "off")) {
        check->setChecked(text == "on");
        return true;
    }
    // Tabbed pages: the tab whose text is TEXT brought to the front, as a
    // click on it does (the style manager's managerTabs=Linetypes).
    if (auto* tabs = qobject_cast<QTabWidget*>(widget)) {
        for (int tab = 0; tab < tabs->count(); ++tab) {
            if (QString(tabs->tabText(tab)).remove('&') == text) {
                tabs->setCurrentIndex(tab);
                return true;
            }
        }
        std::fprintf(stderr, "--fill: %s has no tab '%s'\n", qPrintable(name), qPrintable(text));
        return false;
    }
    // A list, a grid or a tree: the row whose text is TEXT made current and
    // selected, as a click on it does (the symbol library's grid).
    if (auto* view = qobject_cast<QAbstractItemView*>(widget);
        view != nullptr && view->model() != nullptr) {
        const QModelIndexList found = view->model()->match(
            view->model()->index(0, 0), Qt::DisplayRole, text, 1,
            Qt::MatchExactly | Qt::MatchCaseSensitive | Qt::MatchRecursive);
        if (found.isEmpty()) {
            std::fprintf(stderr, "--fill: %s lists no '%s'\n", qPrintable(name), qPrintable(text));
            return false;
        }
        // A view that selects whole rows selects the row, as its click does:
        // one cell alone is no selected row to a manager that asks for them
        // (the style manager's linetypeTable), which then shows nothing.
        QItemSelectionModel::SelectionFlags flags = QItemSelectionModel::ClearAndSelect;
        if (view->selectionBehavior() == QAbstractItemView::SelectRows) {
            flags |= QItemSelectionModel::Rows;
        }
        view->selectionModel()->setCurrentIndex(found.front(), flags);
        return true;
    }
    std::fprintf(stderr, "--fill: the dialog has no field %s that takes '%s'\n",
                 qPrintable(name), qPrintable(text));
    return false;
}

// --run-line: `text` through the window's one executor, as a dialog runs a
// line, and the outcome the dialog gets printed back on stderr - the ok, then
// each line of the reply and of the errors - so a test reads what a dialog
// would show. Whether it was ok.
bool runLine(katana::qt::MainWindow& window, const QString& text)
{
    const katana::qt::VerbOutcome outcome = window.runVerbLine(text);
    std::fprintf(stderr, "--run-line %s: ok=%s\n", qPrintable(text), outcome.ok ? "yes" : "no");
    for (const auto& [label, lines] : {std::pair("reply", outcome.reply),
                                       std::pair("error", outcome.error)}) {
        if (lines.isEmpty()) {
            continue;
        }
        for (const QString& line : lines.split('\n')) {
            std::fprintf(stderr, "  %s: %s\n", label, qPrintable(line));
        }
    }
    return outcome.ok;
}

// --script: the file run by the window's SCRIPT verb, through the one
// executor, as File > Run Script runs it; stopping at the first line refused.
// Whether every line ran.
bool runScriptFile(katana::qt::MainWindow& window, const QString& path)
{
    if (window.runVerbLine(katana::qt::scriptCommandLine(path, false)).ok) {
        return true;
    }
    std::fprintf(stderr, "--script %s stopped at a line that was refused\n", qPrintable(path));
    return false;
}

} // namespace

// Usage:
//   katana [project-directory] [data-file...]
//   katana [project-directory] [data-file...] --plot out.pdf
//                 [--fit | --scale N] [--paper A4|A3|A2|A1|A0]
//                 [--landscape | --portrait] [--dpi N]
//                 [--plot-style colour|grey|mono] [--line-weight-scale F]
//   katana [project-directory] [data-file...] [--command TEXT...]
//                 [--sheets-json out.json|-]
//                 [--plot-sheets out.pdf|folder [--sheets 1,3-5]
//                  [--format pdf|pdfs|png|tiff] [--plot-style colour|grey|mono]
//                  [--dpi N] [--line-weight-scale F]]
//   katana [project-directory] [data-file...] --screenshot out.png
//   katana [project-directory] [data-file...] --toggle-layer NAME --screenshot out.png
//   katana [project-directory] [data-file...] --style-manager --screenshot out.png
//   katana [project-directory] [data-file...] --attributes --screenshot out.png
//   katana [project-directory] [data-file...] --layer-manager --screenshot out.png
//   katana [project-directory] [data-file...] --action NAME... --screenshot out.png
//   katana [project-directory] --dataset-info FILE --screenshot out.png
//   katana [project-directory] --import-options FILE [steps...] --screenshot out.png
//   katana [project-directory] [steps...] --export-options FILE [steps...] --screenshot out.png
//   katana [project-directory] [data-file...] [--select-all] [--action NAME...]
//                 --dialog NAME [--fill FIELD=TEXT...] [--press BUTTON...]
//                 [--report WIDGET...] [--dialog NAME ...] [--survey-dock ACTION ...]
//                 [--command TEXT...] [--run-line TEXT...] [--enter] [--trigger NAME...]
//                 [--script FILE...] --screenshot out.png
//   katana [project-directory] [data-file...] --script FILE... [--command TEXT...]
//   katana --check-shortcuts --screenshot out.png
//   katana --check-menus --screenshot out.png
//
// The first argument that names a directory is opened as a project; other
// arguments are imported by extension, so a session can be set up from the
// command line for testing and for scripted demonstrations.
//
// --plot plots the opened drawing to a PDF and exits WITHOUT showing a window.
// That is what lets ctest exercise the PDF writing, which lives inside a Qt
// widget and otherwise needs a person: the qt_plot_headless test runs this
// under QT_QPA_PLATFORM=offscreen and opens the result. --fit is the default;
// --scale N plots at 1 : N about the view centre.
//
// --plot-sheets plots every sheet of the project to one PDF, a page a sheet,
// and exits without showing a window, as --plot does; a project with no
// sheets plots one fitted to the drawing (MainWindow::sheetsToPlot). The
// switches after it make the request MainWindow::plotSheets carries out
// (plotting/plot_output.hpp); each one not given comes from the set's page
// setup, except the format, which is one PDF unless --format says otherwise.
// --sheets chooses the sheets ("1,3-5", sheet ids); --format pdfs, png or
// tiff writes a file a sheet into the FOLDER given to --plot-sheets, named by
// the page setup's pattern; --plot-style prints in colour, greyscale or
// monochrome (colour, grey, mono); --dpi is the resolution of a raster and
// of a PDF's 3D snapshot; --line-weight-scale multiplies every line weight
// (0.1 to 5). Each file written is printed on stdout, a path a line; the
// summary and any problem go to stderr. --plot takes --plot-style and
// --line-weight-scale too.
//
// --sheets-json writes the project's sheets - every sheet, view, title-block
// value and revision - as the JSON the project stores them in
// (docs/plotting.md), and exits without showing a window: the state an agent
// reads before it changes anything with the sheet verbs. "-" writes it to
// stdout. Given with --plot-sheets, the JSON is written first. Without
// --screenshot, the --command lines run before either is written, so
//   katana project --command "GENERATE grid scale=500" --command SAVE
//                  --sheets-json - --plot-sheets out.pdf
// lays out, keeps, reports and plots the sheets in one headless run; a line
// that is refused fails the run.
//
// --toggle-layer flips a layer's visibility box in the layer panel the way a
// click does, before the screenshot, and fails if the application does not
// come through it cleanly (MainWindow::toggleLayerThroughPanel). It is the
// regression test for a crash on the first click of that box.
//
// --customise loads linestyle and symbol libraries and survey code files before
// anything is drawn, so a screenshot shows the drawing as the customisation
// says it should look. It takes every path until the next switch, because a
// customisation is several files and which is which is decided by looking
// inside them rather than by their extension.
//
// --style-manager opens the styles and linetypes manager before the
// screenshot and grabs THAT window instead of the main one, so the dialog -
// its tables, its form and its two previews - is built and painted in a test
// rather than only by a person who opens the menu.
//
// --action triggers the menu item with that object name (surfaceFromRaster,
// exportSurfaceDem, ...) after the imports, as a click does - repeatable, and
// run in order. A command that would ask a question takes its headless
// default or says why it cannot, so a menu command is exercised by a test
// through the QAction a person clicks. The log is echoed to stderr in a
// headless run, which is where the test reads what the command reported.
//
// --dataset-info and --import-options build GIS > Dataset Information and the
// GIS menu's import dialog for FILE, and grab that window, as --style-manager
// does: the description GDAL or PDAL gives of the file is read and painted.
// With steps, either is the target the steps start on, so --fill, --press
// and --report reach its fields by name. --export-options FILE is a step: it
// opens File > Export Vector's dialog for FILE in its turn, after the
// commands before it made the drawing, and makes it the target - a headless
// run cannot reach it through the menu, which opens no file dialog.
//
// --select-all selects every entity on an unlocked layer, as Edit > Select
// All does, before the actions run - for a command that acts on the selection
// (surveyArea).
//
// --dialog opens a dialog THROUGH ITS ACTION (surveyInverse, surveyTraverse,
// formatStyles, formatSymbols, formatSurveyCodes, ...), as a click does, says
// on stderr what opened, and grabs that dialog instead of the window.
// --survey-dialog is its first name and does the same. --fill types TEXT into the dialog's field with object name FIELD - a
// line, a text box (where "\n" is a line break, so a field book fits on a
// command line), a choice by its item text, a number box (whole or with
// decimals) by its number, or a check box by on/off - and
// --press clicks the button with object name BUTTON. Fills and presses run in
// the order given, so a paged dialog (the import wizard) can be filled page by
// page between its Next presses, and the event loop runs after each one, as
// it does between two things a person does: what a fill sets going there (the
// wizard's preview and role boxes) has happened before the next step. The
// names are listed in
// survey/survey_dialogs.hpp, survey_import_wizard.hpp and survey_points_ui.hpp.
// What a pressed verb reports goes to the log, and so to stderr, where a test
// reads it: a dialog is driven the way a person drives it, not through a side
// door.
//
// --panel NAME makes the window's own dock or toolbar with that object name
// (PropertiesDock, PropertiesToolBar) what the fills and presses after it go
// to, and what --screenshot grabs. --command TEXT runs TEXT as if typed on the
// command line, wherever it comes among the steps, so a test can make the
// styles, entities and selection a panel then acts on - or start a tool by
// its alias and answer its prompts; --enter is Enter on an empty command
// line. --run-line TEXT runs TEXT through the window's one executor, as a
// dialog runs the line it built (MainWindow::runVerbLine): never a running
// tool's answer, and what it logged is printed back as the dialog gets it -
// "--run-line TEXT: ok=yes|no", then a "  reply: " or "  error: " line for each
// line it logged. --report WIDGET prints what the target's WIDGET shows (reportWidget).
// --trigger NAME is --action in its turn among these steps, for a menu
// command that acts on what the steps before it made (formatPurge).
//
// --script FILE runs a katana_cli script (.kcs) in the window, a step among the
// others: the window's SCRIPT verb through its one executor, each line echoed
// and run as a dialog's line is, stopping at the first refused
// (script_runner.hpp). A script that stops fails the run, exit 1. Without
// --screenshot, --plot or --plot-sheets, a --script makes the run a batch as
// katana_cli's is: headless, never shown, over when its steps are, and failed
// by the first step refused.
//
// --check-shortcuts lists every key sequence two of the window's actions or
// menus share, and fails the run when there is one: Qt disables an
// ambiguous shortcut for both, so a clash is keys that silently do nothing.
//
// --check-menus lists every menu item with no icon or no status tip
// (MainWindow::menuGaps), and fails the run when there is one.
//
// --survey-dialog may be given again: the next dialog opens and the fills and
// presses after it go to it, so one run can import a file and export it again.
// --survey-dock ACTION shows the dock that action shows (the Point Manager,
// surveyPointManager), after what came before it; fills and presses after it
// go to the dock, and at the end its status line ("12 points, 12 shown") is
// printed to stderr, so a test sees what the dock lists. The last dialog or
// dock is what --screenshot grabs.
//
// --screenshot lays the main window out exactly as it would appear, grabs it
// to a PNG and exits. It exists so that the LOOK of the application can be
// reviewed - by a person in a pull request, or by a model that cannot watch a
// screen - the same way --plot lets its output be reviewed. Under
// QT_QPA_PLATFORM=offscreen nothing is shown anywhere.
int main(int argc, char* argv[])
{
    QApplication application(argc, argv);
#if defined(KATANA_GPU_D3D11)
    // The HLSL compiled on a worker while the window is built, so the first
    // GPU 3D view finds its bytecode ready (docs/gpu.md, "Shaders"; the
    // compile is 110-175 ms) - only when a 3D view would be drawn on the GPU,
    // so a headless run neither pays for it nor waits for it at exit. The
    // library caches it for the process and is safe from any thread. Joined
    // when main returns.
    std::jthread precompile;
    if (katana::qt::gpu::chooseRenderer(katana::qt::gpu::currentRendererEnvironment(false, false))
            .kind == katana::qt::gpu::RendererKind::Gpu) {
        precompile = std::jthread([] { (void)katana::qt::gpu::precompileHlslShaders(); });
    }
#endif
    application.setApplicationName("Katana");
    application.setOrganizationName("Katana");
    application.setApplicationVersion(KATANA_VERSION);
    // Before any window exists: a widget created under the default style keeps
    // some of its metrics when the style changes beneath it.
    katana::qt::theme::apply(application);
    application.setWindowIcon(katana::qt::applicationIcon());

    std::optional<QString> plotPath;
    std::optional<QString> sheetsPath;
    std::optional<QString> sheetsJsonPath;
    // --plot-sheets' request, beyond the page setup; the style is --plot's
    // too.
    std::string sheetsSelection;
    std::optional<katana::qt::PlotFormat> sheetsFormat;
    std::optional<katana::cad::PlotColourMode> plotStyle;
    std::optional<double> lineWeightScale;
    bool dpiGiven = false;
    std::optional<QString> screenshotPath;
    std::optional<QString> toggleLayer;
    std::vector<std::filesystem::path> customisation;
    bool styleManager = false;
    bool attributeManager = false;
    bool layerManager = false;
    QStringList actions;
    std::optional<QString> datasetInfo;
    std::optional<QString> importOptions;
    bool selectEverything = false;
    // --dialog, --survey-dock, --fill, --press, --panel, --command,
    // --run-line, --enter and --report, in the order given.
    std::vector<std::pair<QString, QString>> surveySteps;
    bool checkShortcuts = false;
    bool checkMenus = false;
    long long attributeEntity = 0;
    bool fit = true;
    katana::cad::PlotSettings settings;
    QStringList inputs;
    const QStringList arguments = application.arguments();
    for (int i = 1; i < arguments.size(); ++i) {
        const QString argument = arguments.at(i);
        const auto value = [&]() -> QString {
            return i + 1 < arguments.size() ? arguments.at(++i) : QString();
        };
        if (argument == "--plot") {
            plotPath = value();
        } else if (argument == "--plot-sheets") {
            sheetsPath = value();
        } else if (argument == "--sheets-json") {
            sheetsJsonPath = value();
        } else if (argument == "--sheets") {
            sheetsSelection = value().toStdString();
        } else if (argument == "--format") {
            const QString format = value();
            sheetsFormat = katana::qt::plotFormatFrom(format.toStdString());
            if (!sheetsFormat) {
                std::fprintf(stderr, "--format must be one of pdf, pdfs, png, tiff (not '%s')\n",
                             qPrintable(format));
                return 2;
            }
        } else if (argument == "--plot-style") {
            const QString style = value();
            plotStyle = katana::cad::plotColourModeFrom(style.toStdString());
            if (!plotStyle) {
                std::fprintf(stderr, "--plot-style must be one of colour, grey, mono (not '%s')\n",
                             qPrintable(style));
                return 2;
            }
            settings.colourMode = *plotStyle;
        } else if (argument == "--line-weight-scale") {
            const QString text = value();
            bool number = false;
            const double factor = text.toDouble(&number);
            if (!number || !(factor >= katana::cad::plotting::kMinimumLineWeightScale &&
                             factor <= katana::cad::plotting::kMaximumLineWeightScale)) {
                std::fprintf(stderr, "--line-weight-scale must be a number from %g to %g (not '%s')\n",
                             katana::cad::plotting::kMinimumLineWeightScale,
                             katana::cad::plotting::kMaximumLineWeightScale, qPrintable(text));
                return 2;
            }
            lineWeightScale = factor;
            settings.lineWeightScale = factor;
        } else if (argument == "--screenshot") {
            screenshotPath = value();
        } else if (argument == "--toggle-layer") {
            toggleLayer = value();
        } else if (argument == "--customise") {
            // Every path until the next switch: a customisation is several
            // files and they are useless apart.
            while (i + 1 < arguments.size() && !arguments.at(i + 1).startsWith("--")) {
                customisation.emplace_back(arguments.at(++i).toStdString());
            }
        } else if (argument == "--style-manager") {
            styleManager = true;
        } else if (argument == "--layer-manager") {
            layerManager = true;
        } else if (argument == "--action") {
            actions << value();
        } else if (argument == "--dataset-info") {
            datasetInfo = value();
        } else if (argument == "--import-options") {
            importOptions = value();
        } else if (argument == "--export-options") {
            surveySteps.emplace_back(argument, value());
        } else if (argument == "--select-all") {
            selectEverything = true;
        } else if (argument == "--survey-dialog" || argument == "--dialog") {
            surveySteps.emplace_back("--dialog", value());
        } else if (argument == "--survey-dock" || argument == "--fill" || argument == "--press" ||
                   argument == "--panel" || argument == "--command" || argument == "--report" ||
                   argument == "--trigger" || argument == "--run-line" ||
                   argument == "--script") {
            surveySteps.emplace_back(argument, value());
        } else if (argument == "--enter") {
            // Enter on an empty command line, a step of its own: an empty
            // --command cannot come through a CMake list.
            surveySteps.emplace_back("--command", QString());
        } else if (argument == "--check-shortcuts") {
            checkShortcuts = true;
        } else if (argument == "--check-menus") {
            checkMenus = true;
        } else if (argument == "--attributes") {
            attributeManager = true;
            // An optional entity id: with one entity selected the manager
            // shows values, with several it shows where they differ.
            if (i + 1 < arguments.size() && arguments.at(i + 1).toLongLong() > 0) {
                attributeEntity = arguments.at(++i).toLongLong();
            }
        } else if (argument == "--fit") {
            fit = true;
        } else if (argument == "--scale") {
            fit = false;
            settings.scaleDenominator = value().toDouble(); // 0 on bad input: refused later
        } else if (argument == "--paper") {
            const QString paper = value().toUpper();
            if (paper == "A0") {
                settings.paper = katana::cad::PaperSize::A0;
            } else if (paper == "A1") {
                settings.paper = katana::cad::PaperSize::A1;
            } else if (paper == "A2") {
                settings.paper = katana::cad::PaperSize::A2;
            } else if (paper == "A3") {
                settings.paper = katana::cad::PaperSize::A3;
            } else if (paper == "A4") {
                settings.paper = katana::cad::PaperSize::A4;
            } else {
                std::fprintf(stderr, "--paper must be one of A0, A1, A2, A3, A4\n");
                return 2;
            }
        } else if (argument == "--landscape") {
            settings.landscape = true;
        } else if (argument == "--portrait") {
            settings.landscape = false;
        } else if (argument == "--dpi") {
            settings.dpi = value().toDouble();
            dpiGiven = true;
        } else {
            inputs << argument;
        }
    }
    if (sheetsPath.has_value() && sheetsPath->isEmpty()) {
        std::fprintf(stderr, "--plot-sheets needs an output path\n");
        return 2;
    }
    if (plotPath.has_value() && plotPath->isEmpty()) {
        std::fprintf(stderr, "--plot needs an output path\n");
        return 2;
    }
    if (sheetsJsonPath.has_value() && sheetsJsonPath->isEmpty()) {
        std::fprintf(stderr, "--sheets-json needs an output path, or - for stdout\n");
        return 2;
    }
    const bool writesOnly = plotPath.has_value() || sheetsPath.has_value() ||
                            sheetsJsonPath.has_value();
    // A script with nothing to grab is a batch run, as katana_cli's is.
    const bool scriptBatch =
        !screenshotPath && std::ranges::any_of(surveySteps, [](const auto& step) {
            return step.first == "--script";
        });

    katana::qt::MainWindow window;
    window.setHeadless(writesOnly || scriptBatch || screenshotPath.has_value());
    // Before anything is opened, so the first drawing is drawn with it. A
    // --customise on the command line is merged in next, as Format > Load
    // Customisation would, and so is loaded when a project is opened: its
    // record of what it was drawn with is compared with what is loaded.
    window.loadDefaultCustomisation();
    if (!customisation.empty()) {
        window.applyCustomisation(customisation);
    }
    if (!writesOnly && !screenshotPath && !scriptBatch) {
        // Where the last session left the window, or fitted to the screen:
        // never in a headless run, whose window is the same everywhere.
        window.restoreSession();
        window.show();
    }
    for (const QString& input : inputs) {
        if (QFileInfo(input).isDir()) {
            window.openProject(input);
        } else {
            window.importPath(input);
        }
    }
    // Without --screenshot the --command lines and scripts run here, in
    // order, before anything is written: lay the sheets out with the sheet
    // verbs, then --sheets-json or --plot-sheets what they made, in one run.
    // A headless run stops at the first line that is refused. (With
    // --screenshot they run among its steps, below.)
    if (!screenshotPath) {
        for (const auto& [kind, text] : surveySteps) {
            if (kind != "--command" && kind != "--run-line" && kind != "--script") {
                continue;
            }
            const bool ran = kind == "--command"    ? window.runCommand(text)
                             : kind == "--run-line" ? runLine(window, text)
                                                    : runScriptFile(window, text);
            QApplication::processEvents();
            QApplication::processEvents();
            if (!ran && (writesOnly || scriptBatch)) {
                std::fprintf(stderr, "%s %s was refused\n", qPrintable(kind), qPrintable(text));
                return 1;
            }
        }
    }

    if (screenshotPath) {
        if (screenshotPath->isEmpty()) {
            std::fprintf(stderr, "--screenshot needs an output path\n");
            return 2;
        }
        // Shown so that the layout is real - docks sized, toolbars wrapped -
        // and events pumped twice, because the first pass lays out and the
        // second paints what the layout produced.
        window.show();
        QApplication::processEvents();
        QApplication::processEvents();
        if (checkShortcuts) {
            int sequences = 0;
            const QStringList clashes = window.shortcutClashes(&sequences);
            for (const QString& clash : clashes) {
                std::fprintf(stderr, "shortcut clash: %s\n", qPrintable(clash));
            }
            if (!clashes.isEmpty()) {
                return 1;
            }
            std::fprintf(stderr, "shortcuts: %d key sequences, each reaching one thing\n",
                         sequences);
        }
        if (checkMenus) {
            int items = 0;
            const QStringList gaps = window.menuGaps(&items);
            for (const QString& gap : gaps) {
                std::fprintf(stderr, "menu gap: %s\n", qPrintable(gap));
            }
            if (!gaps.isEmpty()) {
                return 1;
            }
            std::fprintf(stderr, "menus: %d items, each with an icon and a status tip\n", items);
        }
        if (selectEverything) {
            window.selectAll();
        }
        for (const QString& action : actions) {
            const auto status = window.triggerAction(action);
            if (!status) {
                std::fprintf(stderr, "--action %s failed: %s\n", qPrintable(action),
                             status.error().describe().c_str());
                return 1;
            }
            QApplication::processEvents();
        }
        if (!surveySteps.empty()) {
            // Guarded: a dialog opened with open() may delete itself when a
            // step closes it (the Sheets editor's Generate Sheets and Page
            // Setup do), and a raw pointer left to it was a crash at the next
            // step or at the grab. Gone, the window is the target again.
            QPointer<QWidget> target;
            QString targetName;
            std::vector<QDockWidget*> docks;
            // Held here: the steps drive it and the grab reads it.
            std::unique_ptr<QDialog> optionsDialog;
            if (datasetInfo || importOptions) {
                optionsDialog = datasetInfo
                                    ? std::unique_ptr<QDialog>(window.makeDatasetInfo(*datasetInfo))
                                    : window.makeImportOptions(*importOptions);
                if (optionsDialog == nullptr) {
                    std::fprintf(stderr, "no dialog for %s\n",
                                 qPrintable(datasetInfo ? *datasetInfo : *importOptions));
                    return 1;
                }
                optionsDialog->show();
                target = optionsDialog.get();
                targetName = target->objectName();
                // Its Run pressed (vectorImportRun ...), the import runs as
                // the menu's does once its dialog is accepted.
                if (importOptions) {
                    QDialog* accepted = optionsDialog.get();
                    QObject::connect(accepted, &QDialog::accepted, &window,
                                     [&window, accepted] { window.finishImport(*accepted); });
                }
                QApplication::processEvents();
                QApplication::processEvents();
            }
            for (const auto& [kind, text] : surveySteps) {
                if (kind == "--trigger") {
                    // --action, in its turn among the steps: a menu command
                    // that acts on what the steps before it made.
                    if (const auto status = window.triggerAction(text); !status) {
                        std::fprintf(stderr, "--trigger %s failed: %s\n", qPrintable(text),
                                     status.error().describe().c_str());
                        return 1;
                    }
                    QApplication::processEvents();
                    QApplication::processEvents();
                    continue;
                }
                if (kind == "--command") {
                    window.runCommand(text);
                    // Twice: the document's listener defers the panels'
                    // refresh to the event loop, which the next step reads.
                    QApplication::processEvents();
                    QApplication::processEvents();
                    continue;
                }
                if (kind == "--run-line") {
                    (void)runLine(window, text);
                    QApplication::processEvents();
                    QApplication::processEvents();
                    continue;
                }
                if (kind == "--script") {
                    if (!runScriptFile(window, text)) {
                        return 1;
                    }
                    QApplication::processEvents();
                    QApplication::processEvents();
                    continue;
                }
                if (kind == "--panel") {
                    auto* panel = window.findChild<QWidget*>(text);
                    // A menu is opened under its title, as a click on the
                    // menu bar opens it, so a grab shows what it offers.
                    if (auto* menu = qobject_cast<QMenu*>(panel)) {
                        menu->popup(window.mapToGlobal(QPoint(0, 0)));
                        QApplication::processEvents();
                        QApplication::processEvents();
                    } else if (panel == nullptr || (qobject_cast<QDockWidget*>(panel) == nullptr &&
                                                    qobject_cast<QToolBar*>(panel) == nullptr)) {
                        std::fprintf(stderr,
                                     "--panel: the window has no dock, toolbar or menu %s\n",
                                     qPrintable(text));
                        return 1;
                    }
                    target = panel;
                    targetName = text;
                    continue;
                }
                if (kind == "--dialog" || kind == "--survey-dock" || kind == "--export-options") {
                    QWidget* opened = nullptr;
                    if (kind == "--dialog") {
                        opened = openDialog(window, text);
                    } else if (kind == "--export-options") {
                        opened = window.showExportOptions(text);
                        std::fprintf(stderr, "--export-options %s: opened vectorExportDialog\n",
                                     qPrintable(text));
                    } else if (QDockWidget* dock = openSurveyDock(window, text)) {
                        docks.push_back(dock);
                        opened = dock;
                    }
                    if (opened == nullptr) {
                        return 1;
                    }
                    target = opened;
                    targetName = kind == "--export-options" ? opened->objectName() : text;
                    continue;
                }
                if (kind == "--report") {
                    // The window's own widgets and actions before any
                    // dialog: what a command line step changed there.
                    if (!reportWidget(target != nullptr ? *target : window, window, text)) {
                        return 1;
                    }
                    continue;
                }
                if (target == nullptr) {
                    if (!targetName.isEmpty()) {
                        std::fprintf(stderr, "%s %s: %s has closed\n", qPrintable(kind),
                                     qPrintable(text), qPrintable(targetName));
                        return 1;
                    }
                    std::fprintf(stderr, "%s %s comes before any --dialog\n", qPrintable(kind),
                                 qPrintable(text));
                    return 1;
                }
                if (kind == "--fill") {
                    if (!fillField(*target, text)) {
                        return 1;
                    }
                    QApplication::processEvents();
                    continue;
                }
                auto* button = target->findChild<QAbstractButton*>(text);
                if (button == nullptr) {
                    std::fprintf(stderr, "--press: %s has no button %s\n",
                                 qPrintable(targetName), qPrintable(text));
                    return 1;
                }
                // A disabled button ignores a click: a test that pressed
                // nothing must not pass.
                if (!button->isEnabled()) {
                    std::fprintf(stderr, "--press: %s's button %s is disabled\n",
                                 qPrintable(targetName), qPrintable(text));
                    return 1;
                }
                button->click();
                QApplication::processEvents();
            }
            QApplication::processEvents();
            QApplication::processEvents();
            for (const QDockWidget* dock : docks) {
                const auto* status = dock->findChild<QLabel*>("status");
                std::fprintf(stderr, "%s: %s\n", qPrintable(dock->objectName()),
                             status != nullptr ? qPrintable(status->text()) : "(no status)");
            }
            // Steps that were all --command leave no target: the window is
            // what they changed. So is it when the target closed itself.
            if (target == nullptr && !targetName.isEmpty()) {
                std::fprintf(stderr, "%s has closed: the window is grabbed\n",
                             qPrintable(targetName));
            }
            QWidget* shot = target != nullptr ? target.data() : static_cast<QWidget*>(&window);
            if (!shot->grab().save(*screenshotPath, "PNG")) {
                std::fprintf(stderr, "could not write %s\n", qPrintable(*screenshotPath));
                return 1;
            }
            return 0;
        }
        // The two GIS windows: built from the file, shown, grabbed. A file
        // that cannot be described has no window to grab, and fails the run.
        if (datasetInfo || importOptions) {
            const std::unique_ptr<QDialog> dialog =
                datasetInfo ? std::unique_ptr<QDialog>(window.makeDatasetInfo(*datasetInfo))
                            : window.makeImportOptions(*importOptions);
            if (dialog == nullptr) {
                std::fprintf(stderr, "no dialog for %s\n",
                             qPrintable(datasetInfo ? *datasetInfo : *importOptions));
                return 1;
            }
            dialog->show();
            QApplication::processEvents();
            QApplication::processEvents();
            if (!dialog->grab().save(*screenshotPath, "PNG")) {
                std::fprintf(stderr, "could not write %s\n", qPrintable(*screenshotPath));
                return 1;
            }
            return 0;
        }
        if (toggleLayer) {
            const auto status = window.toggleLayerThroughPanel(*toggleLayer);
            if (!status) {
                std::fprintf(stderr, "--toggle-layer failed: %s\n",
                             status.error().describe().c_str());
                return 1;
            }
            QApplication::processEvents();
        }
        if (layerManager) {
            auto dialog = window.makeLayerManager();
            dialog->show();
            dialog->showFirstRow();
            QApplication::processEvents();
            QApplication::processEvents();
            if (!dialog->grab().save(*screenshotPath, "PNG")) {
                std::fprintf(stderr, "could not write %s\n", qPrintable(*screenshotPath));
                return 1;
            }
            return 0;
        }
        if (attributeManager) {
            // Everything selected, because the manager acts on the selection
            // and an empty one would show an empty tree.
            if (attributeEntity > 0) {
                window.selectOnly(static_cast<katana::entity::EntityId>(attributeEntity));
            } else {
                window.selectAll();
            }
            auto dialog = window.makeAttributeManager();
            dialog->show();
            dialog->expandAll();
            QApplication::processEvents();
            QApplication::processEvents();
            if (!dialog->grab().save(*screenshotPath, "PNG")) {
                std::fprintf(stderr, "could not write %s\n", qPrintable(*screenshotPath));
                return 1;
            }
            return 0;
        }
        if (styleManager) {
            auto dialog = window.makeStyleManager();
            dialog->show();
            dialog->showFirstRows();
            QApplication::processEvents();
            QApplication::processEvents();
            if (!dialog->grab().save(*screenshotPath, "PNG")) {
                std::fprintf(stderr, "could not write %s\n", qPrintable(*screenshotPath));
                return 1;
            }
            return 0;
        }
        if (!window.grab().save(*screenshotPath, "PNG")) {
            std::fprintf(stderr, "could not write %s\n", qPrintable(*screenshotPath));
            return 1;
        }
        return 0;
    }

    if (sheetsJsonPath) {
        const katana::cad::Document& document = window.document();
        if (const auto readable = document.sheetSetStatus(); !readable) {
            std::fprintf(stderr, "--sheets-json: %s\n", readable.error().describe().c_str());
            return 1;
        }
        const katana::cad::plotting::SheetSet& set = document.sheetSet();
        if (*sheetsJsonPath == "-") {
            const auto json = katana::cad::plotting::sheetSetToJson(set);
            if (!json) {
                std::fprintf(stderr, "--sheets-json: %s\n", json.error().describe().c_str());
                return 1;
            }
            std::fprintf(stdout, "%s\n", json->c_str());
            std::fflush(stdout);
        } else {
            const auto status = katana::cad::plotting::writeSheetSetFile(
                set, std::filesystem::path(sheetsJsonPath->toStdWString()));
            if (!status) {
                std::fprintf(stderr, "--sheets-json: %s\n", status.error().describe().c_str());
                return 1;
            }
            std::fprintf(stderr, "wrote %zu sheet%s to %s\n", set.sheets.size(),
                         set.sheets.size() == 1 ? "" : "s", qPrintable(*sheetsJsonPath));
        }
        if (!sheetsPath && !plotPath) {
            return 0;
        }
    }
    if (sheetsPath) {
        const auto set = window.sheetsToPlot();
        if (!set) {
            std::fprintf(stderr, "plot failed: %s\n", set.error().describe().c_str());
            return 1;
        }
        katana::qt::PlotRequest request =
            katana::qt::plotRequestFor(set->pageSetup, *sheetsPath, sheetsSelection);
        request.format = sheetsFormat.value_or(katana::qt::PlotFormat::Pdf);
        if (plotStyle) {
            request.colourMode = *plotStyle;
        }
        if (dpiGiven) {
            request.dpi = settings.dpi;
        }
        if (lineWeightScale) {
            request.lineWeightScale = *lineWeightScale;
        }
        const auto result = window.plotSheets(*set, request);
        if (!result) {
            std::fprintf(stderr, "plot failed: %s\n", result.error().describe().c_str());
            return 1;
        }
        // What was written, for a script to pick up: one path a line.
        for (const QString& file : result->files) {
            std::printf("%s\n", QDir::toNativeSeparators(file).toUtf8().constData());
        }
        return 0;
    }
    if (plotPath) {
        const auto status = window.plotDrawingToPdf(*plotPath, settings, fit);
        if (!status) {
            std::fprintf(stderr, "plot failed: %s\n", status.error().describe().c_str());
            return 1;
        }
        return 0;
    }
    if (scriptBatch) {
        return 0;
    }
    return application.exec();
}
