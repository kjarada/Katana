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
// The menu, in two titled sections:
//   Tables and Libraries
//     Layers...                    the window's action (services.layers)
//     Styles and Linetypes...      formatStyles       -> styleManagerDialog
//     Symbol Library...            formatSymbols      -> symbolLibraryDialog
//     Survey Code Manager...       formatSurveyCodes  -> surveyCodeManagerDialog
//   Across the Drawing
//     Global Modify...             formatGlobalModify -> globalModifyDialog
//     Purge Unused...              formatPurge
// Each manager's action carries its dialog's object name as its data, which
// is how the headless --dialog switch finds the dialog an action opened.
//
// There was a third section, Customisation Files, with the window's Load
// Customisation and Replace Loaded Customisation: file dialogs over the style
// libraries and survey code files of another program, which Katana no longer
// reads. A customisation file is loaded by the CUSTOMISE line - typed, in a
// script, or with --customise - which is the interpreter's
// (cad/customisation_verbs.hpp); no menu item stands for it here.
//
// The workbench owns what the managers share: the picture cache
// (DefinitionThumbnails) and the one CustomisationContext they are built
// from. Each manager is NON-MODAL, made the first time it is asked for and
// kept - hidden, not deleted - between uses, so the code manager's unapplied
// edits survive closing it; asking again shows and raises the same one.
//
// It owns the definition editor (definition_editor.hpp) the same way. That one
// has no menu item: the Symbol Library's New, Edit, Duplicate and Delete and
// the Linetypes tab's Edit Definition and New Library Linestyle ask for it
// through the context (CustomisationContext::editDefinition), so there is one
// editor whichever manager asked, and a headless run reaches it as
// `%definitionEditorDialog` once a step has opened it.
//
// The linework control codes are NOT among what it owns. It kept a copy of
// them for the code manager's Linework tab, and followed the customisation's
// (the Document's customisationState().linework) into it as they changed;
// the tab now reads the Document and sets them with CUSTOMISE SET
// linework.*, so the copy, its follower and the watcher that drove it went.

#include <functional>
#include <memory>
#include <string>
#include <vector>

#include <QKeySequence>
#include <QPointer>
#include <QString>

#include "command_runner.hpp"
#include "icons.hpp"
#include "katana/entity/entity.hpp"

class QAction;
class QMenu;
class QToolBar;
class QWidget;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class DefinitionEditorDialog;
class DefinitionThumbnails;
class GlobalModifyDialog;
class StyleManagerDialog;
class SurveyCodeManagerDialog;
class SymbolLibraryDialog;
class ViewWorkspace;
struct CustomisationContext;
enum class DefinitionEdit;

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
    // The window's Layers action, which the Format menu shows too: the same
    // object, so the menu and the toolbar cannot drift apart. May be null.
    QAction* layers = nullptr;
    // The window's one executor, handed on to every manager
    // (CustomisationContext::run). May be empty (a test).
    CommandRunner run;
    // True when this session has a kept customisation file - the one
    // CUSTOMISE KEEP writes and the next start reads (the window: its host's
    // kept-file path, cad/customisation_host.hpp). It decides whether an
    // editor's own commit is followed by a CUSTOMISE KEEP line
    // (CustomisationWorkbench::context, beginCommit). Asked each time, because
    // the window makes its host after it is built. Unset, or false: no line is
    // ever run, so a session with no kept file never logs KEEP's refusal.
    std::function<bool()> hasKeptFile;
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
    // The one definition editor, shown and raised - made the first time.
    DefinitionEditorDialog& showDefinitionEditor();
    // A manager's request of it (CustomisationContext::editDefinition): the
    // editor is shown, then asked. False when it refused, which it has said
    // in its own message area and in the log.
    bool editDefinition(DefinitionEdit what, const std::string& name);
    // The managers made so far; null for one never opened.
    [[nodiscard]] StyleManagerDialog* styleManager() const;
    [[nodiscard]] SymbolLibraryDialog* symbolLibrary() const;
    [[nodiscard]] SurveyCodeManagerDialog* codeManager() const;
    [[nodiscard]] GlobalModifyDialog* globalModify() const;
    [[nodiscard]] DefinitionEditorDialog* definitionEditor() const;

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
    //
    // The definition editor's unsaved form is asked about first, the same
    // way: interactive, the editor is shown and `confirm` (else a question
    // box) asks whether to discard it; headless, refused and said. A yes
    // discards nothing here - the later questions may still keep the window
    // open - and is not asked again until the form changes.
    [[nodiscard]] bool confirmClose();

    // What every manager is built from, for a caller building one of its
    // own (the window's --style-manager grab).
    [[nodiscard]] CustomisationContext context();
    [[nodiscard]] DefinitionThumbnails& thumbnails() { return *thumbnails_; }

  private:
    [[nodiscard]] bool headless() const;
    // Select `ids` and frame them in the active plan view.
    void selectAndShow(const std::vector<katana::entity::EntityId>& ids);
    void log(const QString& text, bool isError) const;

    QWidget& window_;
    CustomisationServices services_;
    std::unique_ptr<DefinitionThumbnails> thumbnails_;
    QAction* stylesAction_ = nullptr;
    QAction* symbolsAction_ = nullptr;
    QAction* codesAction_ = nullptr;
    QAction* purgeAction_ = nullptr;
    QAction* globalModifyAction_ = nullptr;
    QPointer<StyleManagerDialog> styles_;
    QPointer<SymbolLibraryDialog> symbols_;
    QPointer<SurveyCodeManagerDialog> codes_;
    QPointer<GlobalModifyDialog> globalModify_;
    QPointer<DefinitionEditorDialog> definitions_;
};

} // namespace katana::qt
