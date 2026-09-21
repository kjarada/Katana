#include <QApplication>
#include <QFileInfo>

#include <cstdio>
#include <optional>

#include "katana/cad/plot.hpp"
#include "main_window.hpp"

// Usage:
//   katana_qt_app [project-directory] [data-file...]
//   katana_qt_app [project-directory] [data-file...] --plot out.pdf
//                 [--fit | --scale N] [--paper A4|A3|A2|A1|A0]
//                 [--landscape | --portrait] [--dpi N]
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
int main(int argc, char* argv[])
{
    QApplication application(argc, argv);
    application.setApplicationName("Katana");
    application.setOrganizationName("Katana");

    std::optional<QString> plotPath;
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
    if (!plotPath) {
        window.show();
    }
    for (const QString& input : inputs) {
        if (QFileInfo(input).isDir()) {
            window.openProject(input);
        } else {
            window.importPath(input);
        }
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
