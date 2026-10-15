#pragma once

// Where File > Open and File > Save As start (docs/desktop.md, "The File
// menu").
//
// Qt starts a file dialog in the process's WORKING folder when it is given a
// bare name or none: for a zip copy that is the program's own bin\ (Explorer
// starts a double-clicked exe in its folder), for the installed copy the
// install folder under %LOCALAPPDATA%\Programs - so the first project of a new
// user was proposed as bin\untitled.katana, and deleting the folder, as the
// README says to remove the program, deleted the project with it. The window
// proposes, in this order, the folder the open project is in, the folder the
// last project was opened or saved in (kept between sessions), and the
// person's Documents folder; the first that is there.

#include <QDir>
#include <QString>

namespace katana::qt {

// The first of `projectParent`, `remembered` and `documents` that names a folder
// that exists; the home folder when none does. Each may be empty. Never the
// working folder.
[[nodiscard]] inline QString projectDialogFolder(const QString& projectParent,
                                                 const QString& remembered,
                                                 const QString& documents)
{
    for (const QString& candidate : {projectParent, remembered, documents}) {
        if (!candidate.isEmpty() && QDir(candidate).exists()) {
            return candidate;
        }
    }
    return QDir::homePath();
}

} // namespace katana::qt
