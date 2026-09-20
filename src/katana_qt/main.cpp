#include <QApplication>
#include <QFileInfo>

#include "main_window.hpp"

// Usage: katana_qt_app [project-directory] [data-file...]
//
// The first argument, if it names a directory, is opened as a project. Any
// further arguments are imported by extension, so a session can be set up from
// the command line for testing and for scripted demonstrations.
int main(int argc, char* argv[])
{
    QApplication application(argc, argv);
    application.setApplicationName("Katana");
    application.setOrganizationName("Katana");

    katana::qt::MainWindow window;
    window.show();
    const QStringList arguments = application.arguments();
    for (int i = 1; i < arguments.size(); ++i) {
        const QString argument = arguments.at(i);
        if (QFileInfo(argument).isDir()) {
            window.openProject(argument);
        } else {
            window.importPath(argument);
        }
    }
    return application.exec();
}
