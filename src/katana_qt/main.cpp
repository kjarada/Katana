#include <QAbstractButton>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QAction>
#include <QDialog>
#include <QDockWidget>
#include <QFileInfo>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QSpinBox>

#include <cstdio>
#include <optional>

#include "icons.hpp"
#include "attribute_manager.hpp"
#include "gis_dialogs.hpp"
#include "layer_manager.hpp"
#include "style_manager.hpp"
#include "katana/cad/plot.hpp"
#include "theme.hpp"
#include "main_window.hpp"

namespace {

// --survey-dialog: the dialog NAME's action opens, found by the object name
// the workbench gives it (the action's plus "Dialog"). nullptr, said on
// stderr, for an unknown action or one that opens no dialog.
QDialog* openSurveyDialog(katana::qt::MainWindow& window, const QString& name)
{
    if (const auto status = window.triggerAction(name); !status) {
        std::fprintf(stderr, "--survey-dialog %s failed: %s\n", qPrintable(name),
                     status.error().describe().c_str());
        return nullptr;
    }
    QApplication::processEvents();
    auto* dialog = window.findChild<QDialog*>(name + "Dialog");
    if (dialog == nullptr) {
        std::fprintf(stderr, "--survey-dialog: %s opened no dialog named %sDialog\n",
                     qPrintable(name), qPrintable(name));
    }
    return dialog;
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
// have or a value it cannot take - a test that fills nothing must not pass.
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
    if (auto* check = qobject_cast<QCheckBox*>(widget); check != nullptr &&
                                                         (text == "on" || text == "off")) {
        check->setChecked(text == "on");
        return true;
    }
    std::fprintf(stderr, "--fill: the dialog has no field %s that takes '%s'\n",
                 qPrintable(name), qPrintable(text));
    return false;
}

} // namespace

// Usage:
//   katana [project-directory] [data-file...]
//   katana [project-directory] [data-file...] --plot out.pdf
//                 [--fit | --scale N] [--paper A4|A3|A2|A1|A0]
//                 [--landscape | --portrait] [--dpi N]
//   katana [project-directory] [data-file...] --screenshot out.png
//   katana [project-directory] [data-file...] --toggle-layer NAME --screenshot out.png
//   katana [project-directory] [data-file...] --style-manager --screenshot out.png
//   katana [project-directory] [data-file...] --attributes --screenshot out.png
//   katana [project-directory] [data-file...] --layer-manager --screenshot out.png
//   katana [project-directory] [data-file...] --action NAME... --screenshot out.png
//   katana [project-directory] --dataset-info FILE --screenshot out.png
//   katana [project-directory] --import-options FILE --screenshot out.png
//   katana [project-directory] [data-file...] [--select-all] [--action NAME...]
//                 --survey-dialog NAME [--fill FIELD=TEXT...] [--press BUTTON...]
//                 [--survey-dialog NAME ...] [--survey-dock ACTION ...]
//                 --screenshot out.png
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
// --toggle-layer flips a layer's visibility box in the layer panel the way a
// click does, before the screenshot, and fails if the application does not
// come through it cleanly (MainWindow::toggleLayerThroughPanel). It is the
// regression test for a crash on the first click of that box.
//
// --customise loads 12d linestyle and symbol libraries and mapfiles before
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
//
// --select-all selects every entity on an unlocked layer, as Edit > Select
// All does, before the actions run - for a command that acts on the selection
// (surveyArea).
//
// --survey-dialog opens a Survey menu dialog THROUGH ITS ACTION (surveyInverse,
// surveyTraverse, ...), as a click does, and grabs that dialog instead of the
// window. --fill types TEXT into the dialog's field with object name FIELD - a
// line, a text box (where "\n" is a line break, so a field book fits on a
// command line), a choice by its item text, or a check box by on/off - and
// --press clicks the button with object name BUTTON. Fills and presses run in
// the order given, so a paged dialog (the import wizard) can be filled page by
// page between its Next presses. The names are listed in
// survey/survey_dialogs.hpp, survey_import_wizard.hpp and survey_points_ui.hpp.
// What a pressed verb reports goes to the log, and so to stderr, where a test
// reads it: a dialog is driven the way a person drives it, not through a side
// door.
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
    application.setApplicationName("Katana");
    application.setOrganizationName("Katana");
    application.setApplicationVersion(KATANA_VERSION);
    // Before any window exists: a widget created under the default style keeps
    // some of its metrics when the style changes beneath it.
    katana::qt::theme::apply(application);
    application.setWindowIcon(katana::qt::applicationIcon());

    std::optional<QString> plotPath;
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
    // --survey-dialog, --survey-dock, --fill and --press, in the order given.
    std::vector<std::pair<QString, QString>> surveySteps;
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
        } else if (argument == "--select-all") {
            selectEverything = true;
        } else if (argument == "--survey-dialog" || argument == "--survey-dock" ||
                   argument == "--fill" || argument == "--press") {
            surveySteps.emplace_back(argument, value());
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
        } else {
            inputs << argument;
        }
    }
    if (plotPath.has_value() && plotPath->isEmpty()) {
        std::fprintf(stderr, "--plot needs an output path\n");
        return 2;
    }

    katana::qt::MainWindow window;
    window.setHeadless(plotPath.has_value() || screenshotPath.has_value());
    // Before anything is opened, so the first drawing is drawn with it. A
    // --customise on the command line is applied after and wins.
    window.loadDefaultCustomisation();
    if (!plotPath && !screenshotPath) {
        window.show();
    }
    for (const QString& input : inputs) {
        if (QFileInfo(input).isDir()) {
            window.openProject(input);
        } else {
            window.importPath(input);
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
            QWidget* target = nullptr;
            QString targetName;
            std::vector<QDockWidget*> docks;
            for (const auto& [kind, text] : surveySteps) {
                if (kind == "--survey-dialog" || kind == "--survey-dock") {
                    QWidget* opened = nullptr;
                    if (kind == "--survey-dialog") {
                        opened = openSurveyDialog(window, text);
                    } else if (QDockWidget* dock = openSurveyDock(window, text)) {
                        docks.push_back(dock);
                        opened = dock;
                    }
                    if (opened == nullptr) {
                        return 1;
                    }
                    target = opened;
                    targetName = text;
                    continue;
                }
                if (target == nullptr) {
                    std::fprintf(stderr, "%s %s comes before any --survey-dialog\n",
                                 qPrintable(kind), qPrintable(text));
                    return 1;
                }
                if (kind == "--fill") {
                    if (!fillField(*target, text)) {
                        return 1;
                    }
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
            if (!target->grab().save(*screenshotPath, "PNG")) {
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
        if (!customisation.empty()) {
            window.applyCustomisation(customisation);
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

    if (plotPath) {
        const auto status = window.plotDrawingToPdf(*plotPath, settings, fit);
        if (!status) {
            std::fprintf(stderr, "plot failed: %s\n", status.error().describe().c_str());
            return 1;
        }
        return 0;
    }
    return application.exec();
}
