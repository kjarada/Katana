#pragma once

// The Sheets editor's Sheet Set menu (docs/plotting.md, "The Sheet Set menu
// and Page Setup"): the whole set kept in a JSON file and brought back,
// another set's sheets appended, the set copied as JSON, and the page setup
// changed without plotting.
//
// Every item runs the SHEETS line a person would type through the editor
// (SheetEditor::runLine) - in the window, its one executor - so the log shows
// what to type, the history keeps it, and an edit is ONE undoable step:
//
//   Save Sheet Set As...          sheetSaveSet     SHEETS SAVE "path"
//   Load Sheet Set...             sheetLoadSet     SHEETS LOAD "path"      LOAD_SHEETS
//   Append Sheets From File...    sheetAppendSet   SHEETS APPEND "path"    APPEND_SHEETS
//   Copy Sheet Set as JSON        sheetCopySetJson SHEETS JSON, its reply to the clipboard
//   Page Setup...                 sheetPageSetup   SHEETS PAGESETUP ...    PAGE_SETUP
//
// Load asks before it replaces sheets there are (sheetLoadSetQuestion: "Undo
// brings them back"). A headless session opens no file dialog and asks
// nothing: the three file items name their verb instead. Page Setup opens
// without waiting (pageSetupDialog, the Plot dialog's page-setup mode), so a
// headless session fills it by its object names.

class QMenu;
class QMenuBar;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class PlotDialog;
class SheetEditor;

// The Sheet Set menu (sheetSetMenu), added to `bar` where the next menu
// would go: the editor puts it first.
QMenu* addSheetSetMenu(SheetEditor& editor, katana::cad::Document& document, QMenuBar& bar);

// Page Setup for the editor's set, opened and returned without waiting: the
// Plot dialog in its page-setup mode, whose Save (pageSetupSave) runs
// PlotDialog::pageSetupLine through the editor.
PlotDialog* openPageSetupDialog(SheetEditor& editor, katana::cad::Document& document);

} // namespace katana::qt
