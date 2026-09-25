#include "plotting/sheet_set_menu.hpp"

#include <QAction>
#include <QClipboard>
#include <QDir>
#include <QFileDialog>
#include <QGuiApplication>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QStandardPaths>
#include <QStatusBar>

#include "katana/cad/document.hpp"
#include "plotting/plot_dialog.hpp"
#include "sheet_editor.hpp"

namespace katana::qt {

namespace {

const QString kSheetSetFiles = QStringLiteral("Sheet sets (*.json)");

// Where the file dialogs start: the project's folder, else the documents.
QString startFolder(const katana::cad::Document& document)
{
    if (const auto directory = document.projectDirectory()) {
        return QString::fromStdWString(directory->wstring());
    }
    return QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
}

// A path as a SHEETS line takes it: in double quotes, for the spaces.
QString quotedPath(const QString& path)
{
    return '"' + QDir::fromNativeSeparators(path) + '"';
}

// A headless session has no one to pick a file or answer a question: the
// item says what to type instead, and does nothing.
bool saidHeadless(SheetEditor& editor, const QString& verb)
{
    if (!editor.headless()) {
        return false;
    }
    editor.report("A headless session opens no file dialog: type " + verb + " \"path\" instead.",
                  true);
    return true;
}

void saveSheetSet(SheetEditor& editor, const katana::cad::Document& document)
{
    if (saidHeadless(editor, QStringLiteral("SHEETS SAVE"))) {
        return;
    }
    QString path = QFileDialog::getSaveFileName(&editor, QStringLiteral("Save Sheet Set As"),
                                                QDir(startFolder(document)).filePath("sheets.json"),
                                                kSheetSetFiles);
    if (path.isEmpty()) {
        return;
    }
    if (!path.endsWith(QStringLiteral(".json"), Qt::CaseInsensitive)) {
        path += QStringLiteral(".json");
    }
    (void)editor.runLine("SHEETS SAVE " + quotedPath(path));
}

void loadSheetSet(SheetEditor& editor, const katana::cad::Document& document)
{
    if (saidHeadless(editor, QStringLiteral("SHEETS LOAD"))) {
        return;
    }
    const QString path = QFileDialog::getOpenFileName(&editor, QStringLiteral("Load Sheet Set"),
                                                      startFolder(document), kSheetSetFiles);
    if (path.isEmpty()) {
        return;
    }
    // Loading replaces every sheet. Undo brings them back, which the
    // question says, so it is no threat; with nothing to replace it is not
    // asked.
    QString replaced;
    if (const auto status = document.sheetSetStatus(); !status) {
        replaced = QStringLiteral("The project's sheets cannot be read (") +
                   QString::fromStdString(status.error().message) +
                   QStringLiteral("). Replace them? Undo brings them back.");
    } else if (const std::size_t count = document.sheetSet().sheets.size(); count > 0) {
        replaced = QString("Replace the %1 sheet%2 there are now? Undo brings them back.")
                       .arg(count)
                       .arg(count == 1 ? "" : "s");
    }
    if (!replaced.isEmpty()) {
        QMessageBox question(QMessageBox::Question, QStringLiteral("Load Sheet Set"), replaced,
                             QMessageBox::Yes | QMessageBox::Cancel, &editor);
        question.setObjectName(QStringLiteral("sheetLoadSetQuestion"));
        question.setDefaultButton(QMessageBox::Yes);
        if (question.exec() != QMessageBox::Yes) {
            return;
        }
    }
    (void)editor.runLine("SHEETS LOAD " + quotedPath(path));
}

void appendSheets(SheetEditor& editor, const katana::cad::Document& document)
{
    if (saidHeadless(editor, QStringLiteral("SHEETS APPEND"))) {
        return;
    }
    const QString path = QFileDialog::getOpenFileName(
        &editor, QStringLiteral("Append Sheets From File"), startFolder(document), kSheetSetFiles);
    if (path.isEmpty()) {
        return;
    }
    const std::size_t first = document.sheetSet().sheets.size();
    if (editor.runLine("SHEETS APPEND " + quotedPath(path)).ok) {
        editor.setCurrentSheet(first);
    }
}

void copySheetSetJson(SheetEditor& editor)
{
    const VerbOutcome outcome = editor.runLine(QStringLiteral("SHEETS JSON"));
    if (outcome.ok) {
        QGuiApplication::clipboard()->setText(outcome.reply);
        editor.statusBar()->showMessage(
            QString("The sheet set's JSON is on the clipboard: %1 characters.").arg(outcome.reply.size()),
            8000);
    }
}

} // namespace

QMenu* addSheetSetMenu(SheetEditor& editor, katana::cad::Document& document, QMenuBar& bar)
{
    QMenu* menu = bar.addMenu(QStringLiteral("Sheet &Set"));
    menu->setObjectName(QStringLiteral("sheetSetMenu"));
    const auto item = [&editor, menu](const QString& text, const char* name, const QString& tip) {
        auto* action = new QAction(text, &editor);
        action->setObjectName(QString::fromLatin1(name));
        action->setToolTip(tip);
        action->setStatusTip(tip);
        menu->addAction(action);
        return action;
    };
    QObject::connect(item(QStringLiteral("&Save Sheet Set As..."), "sheetSaveSet",
                          QStringLiteral("Keep every sheet, its views, the title block and the page "
                                         "setup in a JSON file (SHEETS SAVE)")),
                     &QAction::triggered, &editor, [&editor, &document] { saveSheetSet(editor, document); });
    QObject::connect(item(QStringLiteral("&Load Sheet Set..."), "sheetLoadSet",
                          QStringLiteral("Replace the sheets with a set kept in a JSON file; Undo brings "
                                         "them back (SHEETS LOAD)")),
                     &QAction::triggered, &editor, [&editor, &document] { loadSheetSet(editor, document); });
    QObject::connect(item(QStringLiteral("&Append Sheets From File..."), "sheetAppendSet",
                          QStringLiteral("Put another set's sheets after these, their ids renumbered "
                                         "(SHEETS APPEND)")),
                     &QAction::triggered, &editor, [&editor, &document] { appendSheets(editor, document); });
    QObject::connect(item(QStringLiteral("&Copy Sheet Set as JSON"), "sheetCopySetJson",
                          QStringLiteral("The whole set as the JSON the project keeps, on the clipboard "
                                         "(SHEETS JSON)")),
                     &QAction::triggered, &editor, [&editor] { copySheetSetJson(editor); });
    menu->addSeparator();
    QAction* pageSetup =
        item(QStringLiteral("Page Set&up..."), "sheetPageSetup",
             QStringLiteral("How the set plots - colours, line weights, resolution, file names - kept "
                            "with it without plotting (SHEETS PAGESETUP)"));
    // What a headless session's --dialog finds it by.
    pageSetup->setData(QStringLiteral("pageSetupDialog"));
    QObject::connect(pageSetup, &QAction::triggered, &editor,
                     [&editor, &document] { (void)openPageSetupDialog(editor, document); });
    return menu;
}

PlotDialog* openPageSetupDialog(SheetEditor& editor, katana::cad::Document& document)
{
    auto* dialog = new PlotDialog(document.sheetSet(), editor.currentSheet(), true, QString(), &editor,
                                  PlotDialog::Mode::PageSetup);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    QObject::connect(dialog, &QDialog::accepted, &editor,
                     [&editor, dialog] { (void)editor.runLine(dialog->pageSetupLine()); });
    dialog->open();
    return dialog;
}

} // namespace katana::qt
