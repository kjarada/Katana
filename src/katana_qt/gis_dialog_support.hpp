#pragma once

// What the GIS menu's option dialogs share (gis_import_dialogs.hpp,
// gis_export_dialog.hpp): their OK and Cancel row.

#include <QDialog>
#include <QDialogButtonBox>
#include <QObject>
#include <QPushButton>
#include <QString>

namespace katana::qt {

// OK, named for what it does ("Import", "Export"), and Cancel. Their object
// names are the dialog's own without "Dialog", then Run and Cancel
// (vectorImportRun, vectorImportCancel), so a headless run can press them
// (docs/headless.md) as it presses every GIS dialog's Run.
inline QDialogButtonBox* okCancel(QDialog* dialog, const QString& okText)
{
    auto* buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dialog);
    buttons->button(QDialogButtonBox::Ok)->setText(okText);
    QString prefix = dialog->objectName();
    if (prefix.endsWith("Dialog")) {
        prefix.chop(6);
    }
    buttons->button(QDialogButtonBox::Ok)->setObjectName(prefix + "Run");
    buttons->button(QDialogButtonBox::Cancel)->setObjectName(prefix + "Cancel");
    QObject::connect(buttons, &QDialogButtonBox::accepted, dialog, &QDialog::accept);
    QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    return buttons;
}

} // namespace katana::qt
