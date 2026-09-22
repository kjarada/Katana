#include <QApplication>
#include <QFileInfo>

#include <cstdio>
#include <optional>

#include "icons.hpp"
#include "style_manager.hpp"
#include "katana/cad/plot.hpp"
#include "theme.hpp"
#include "main_window.hpp"

// Usage:
//   katana [project-directory] [data-file...]
//   katana [project-directory] [data-file...] --plot out.pdf
//                 [--fit | --scale N] [--paper A4|A3|A2|A1|A0]
//                 [--landscape | --portrait] [--dpi N]
//   katana [project-directory] [data-file...] --screenshot out.png
//   katana [project-directory] [data-file...] --toggle-layer NAME --screenshot out.png
//   katana [project-directory] [data-file...] --style-manager --screenshot out.png
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
// --style-manager opens the styles and linetypes manager before the
// screenshot and grabs THAT window instead of the main one, so the dialog -
// its tables, its form and its two previews - is built and painted in a test
// rather than only by a person who opens the menu.
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
    bool styleManager = false;
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
        } else if (argument == "--style-manager") {
            styleManager = true;
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
        if (toggleLayer) {
            const auto status = window.toggleLayerThroughPanel(*toggleLayer);
            if (!status) {
                std::fprintf(stderr, "--toggle-layer failed: %s\n",
                             status.error().describe().c_str());
                return 1;
            }
            QApplication::processEvents();
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
