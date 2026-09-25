#pragma once

// The Format menu and toolbar: the three customisation managers, and what
// goes with them (the owner's requests of 2026-09-24: "enhance the linestyle
// and symbol and survey codes managers" and "make it professional CAD
// software"). Format is where AutoCAD keeps Layer, Linetype and Text Style.
//
// Built like SurveyWorkbench: MainWindow makes the menu and the toolbar,
// hands them over with CustomisationServices and keeps this object alive; the
// window's own pieces arrive through the services, so this never includes
// main_window.hpp - and, having no window in it, it can be built and driven by
// a widget test.
//
// The menu:
//   Layers...                      the window's action (services.layers)
//   Styles and Linetypes...        formatStyles       -> styleManagerDialog
//   Symbol Library...              formatSymbols      -> symbolLibraryDialog
//   Survey Code Manager...         formatSurveyCodes  -> surveyCodeManagerDialog
//   ---
//   Load Customisation...          the window's actions (services.load...,
//   Replace Loaded Customisation...  services.replace...)
//   ---
//   Global Modify...               formatGlobalModify -> globalModifyDialog
//   Purge Unused...                formatPurge
// Each manager's action carries its dialog's object name as its data, which
// is how the headless --dialog switch finds the dialog an action opened.
//
// The workbench owns what the managers share: the picture cache
// (DefinitionThumbnails), the session's linework control codes, and the one
// CustomisationContext they are built from. Each manager is NON-MODAL, made
// the first time it is asked for and kept - hidden, not deleted - between
// uses, so the code manager's unapplied edits survive closing it; asking again
// shows and raises the same one.

#include <functional>
#include <memory>
#include <vector>

#include <QKeySequence>
#include <QPointer>
#include <QString>

#include "command_runner.hpp"
#include "icons.hpp"
#include "katana/cad/linework.hpp"
#include "katana/entity/entity.hpp"

class QAction;
class QMenu;
class QToolBar;
class QWidget;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class DefinitionThumbnails;
class GlobalModifyDialog;
class StyleManagerDialog;
class SurveyCodeManagerDialog;
class SymbolLibraryDialog;
class ViewWorkspace;
struct CustomisationContext;

struct CustomisationServices {
    katana::cad::Document* document = nullptr;
    // For "show me what uses this": the selection is framed in the active
    // plan view. May be null (a test): the managers then only select.
    ViewWorkspace* views = nullptr;
    // The window's action factory, so a Format action looks and reads like
    // every other: icon, status tip, and a tooltip naming its shortcut.
    std::function<QAction*(Icon icon, const QString& text, const QString& tip,
                           const QKeySequence& shortcut, const QString& objectName)>
        makeAction;
    // The command log. isError also flashes the message in the status bar.
    std::function<void(const QString& text, bool isError)> log;
    // True in a session nobody watches (--screenshot, --plot): no modal box,
    // no file dialog. Asked each time, because the window learns it after it
    // is built. Unset: interactive.
    std::function<bool()> headless;
    // Actions the window owns that the Format menu shows too, the same
    // objects, so two menus cannot drift apart. Each may be null.
    QAction* layers = nullptr;
    QAction* loadCustomisation = nullptr;
    QAction* replaceCustomisation = nullptr;
    // The window's one executor, handed on to every manager
    // (CustomisationContext::run). May be empty (a test).
    CommandRunner run;
};

class CustomisationWorkbench {
  public:
    // Fills `menu` and `toolBar`, which the caller has made and placed. The
    // dialogs are made as `window`'s children, so they float over it.
    CustomisationWorkbench(QWidget& window, CustomisationServices services, QMenu& menu,
                           QToolBar& toolBar);
    // Deletes the dialogs. They hold the Document, which the window destroys
    // BEFORE its children (members go before the QWidget base), so they must
    // go with this object - which the window destroys before the Document.
    ~CustomisationWorkbench();

    CustomisationWorkbench(const CustomisationWorkbench&) = delete;
    CustomisationWorkbench& operator=(const CustomisationWorkbench&) = delete;

    // The actions, by what they do: the Survey menu shows the code manager's
    // too (SurveyServices::codeManager).
    [[nodiscard]] QAction* styleManagerAction() const { return stylesAction_; }
    [[nodiscard]] QAction* symbolLibraryAction() const { return symbolsAction_; }
    [[nodiscard]] QAction* codeManagerAction() const { return codesAction_; }
    [[nodiscard]] QAction* purgeAction() const { return purgeAction_; }
    [[nodiscard]] QAction* globalModifyAction() const { return globalModifyAction_; }

    // Shows the manager, making it the first time, and raises it. What each
    // action does.
    StyleManagerDialog& showStyleManager();
    SymbolLibraryDialog& showSymbolLibrary();
    SurveyCodeManagerDialog& showCodeManager();
    // Format > Global Modify (global_modify_dialog.hpp): the selection, a
    // view, layers or the drawing, changed as one undo step. Its View scope
    // offers the workspace's open views (services.views); none without one.
    GlobalModifyDialog& showGlobalModify();
    // The managers made so far; null for one never opened.
    [[nodiscard]] StyleManagerDialog* styleManager() const;
    [[nodiscard]] SymbolLibraryDialog* symbolLibrary() const;
    [[nodiscard]] SurveyCodeManagerDialog* codeManager() const;
    [[nodiscard]] GlobalModifyDialog* globalModify() const;

    // Format > Purge Unused: every style, linetype and hatch pattern nothing
    // uses (cad::purgeCommand), keeping the current style, deleted as ONE
    // undo step and named in the log. An interactive session is asked first
    // (confirm when set, else a question box); a headless one is not. True
    // when something was purged.
    bool purgeUnused();
    // Asked instead of the question box in an interactive session - a test's
    // answer. Unset: QMessageBox::question.
    std::function<bool(const QString& question)> confirm{};

    // Whether the window may close, as far as the managers are concerned:
    // the code manager's unapplied rule edits live only in its buffer, and
    // would go with it unasked. None: true. Interactive: the manager is shown
    // and closed, so its own Apply / Discard / Cancel question is asked over
    // the rules it is about; false on Cancel (or a failed Apply), the
    // manager left open with its edits. Headless: nobody can answer, so -
    // as the window's unsaved-drawing check - refused and said, false.
    [[nodiscard]] bool confirmClose();

    // What every manager is built from, for a caller building one of its
    // own (the window's --style-manager grab).
    [[nodiscard]] CustomisationContext context();
    [[nodiscard]] DefinitionThumbnails& thumbnails() { return *thumbnails_; }
    [[nodiscard]] katana::cad::LineworkCodes& lineworkCodes() { return lineworkCodes_; }

  private:
    [[nodiscard]] bool headless() const;
    // Select `ids` and frame them in the active plan view.
    void selectAndShow(const std::vector<katana::entity::EntityId>& ids);
    void log(const QString& text, bool isError) const;

    QWidget& window_;
    CustomisationServices services_;
    std::unique_ptr<DefinitionThumbnails> thumbnails_;
    katana::cad::LineworkCodes lineworkCodes_{};
    QAction* stylesAction_ = nullptr;
    QAction* symbolsAction_ = nullptr;
    QAction* codesAction_ = nullptr;
    QAction* purgeAction_ = nullptr;
    QAction* globalModifyAction_ = nullptr;
    QPointer<StyleManagerDialog> styles_;
    QPointer<SymbolLibraryDialog> symbols_;
    QPointer<SurveyCodeManagerDialog> codes_;
    QPointer<GlobalModifyDialog> globalModify_;
};

} // namespace katana::qt
