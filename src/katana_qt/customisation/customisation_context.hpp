#pragma once

// What every customisation manager dialog is given by whoever opens it - the
// style and linestyle manager, the symbol library, the survey-code manager -
// so the three are built the same way and none reaches into the main window.
//
// A plain aggregate of borrowed pointers and callbacks. The maker (the main
// window, a test, the headless driver) owns everything it points at and must
// keep it alive for as long as a dialog built from the context is open, with
// one exception the dialogs are written for: the Document may die first
// (docs/cad.md: "a dialog holding Document& must be deletable before the
// Document"), which is why a dialog watches it through DocumentWatcher rather
// than a bare listener.
//
// Nothing here is optional unless its comment says so; a dialog may assume
// `document` and `log` are set.

#include <functional>
#include <string>
#include <vector>

#include <QString>

#include "command_runner.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad {
class Document;
} // namespace katana::cad

// The type cad::LineworkCodes names. It is declared where it is defined, in
// the entity layer: an alias cannot be declared ahead of the header that makes
// it.
namespace katana::entity {
struct LineworkCodes;
} // namespace katana::entity

namespace katana::qt {

class DefinitionThumbnails;

// What a manager asks the definition editor (definition_editor.hpp) to do
// with a library definition. A new one is of the kind the manager that asked
// lists: the symbol library's is a symbol, the Linetypes tab's a linestyle.
enum class DefinitionEdit {
    NewSymbol,
    NewLinestyle,
    Edit,
    Duplicate,
    Delete,
};

struct CustomisationContext {
    // The drawing being customised. Its StyleLibrary and SurveyMap are
    // SESSION data (decision D1): not undoable and not saved in the project;
    // a dialog edits a copy and commits it with setStyleLibrary/setSurveyMap.
    // Model tables (styles, linetypes) change only through Document::execute.
    katana::cad::Document* document = nullptr;

    // Where a dialog reports what it did and what it refused - the main
    // window's log panel, or a test's collector. `isError` is true for a
    // failure (a refused command, an unreadable file), false for information.
    // A dialog never opens a modal box for either: a headless session has no
    // one to close it.
    std::function<void(const QString& message, bool isError)> log{};

    // The picture cache shared by every picker and browser the dialogs open,
    // so a symbol painted for one list is not painted again for the next.
    // Owned by the maker of the dialogs; it keys itself on the library
    // generation, so it needs no clearing when a library is loaded. May be
    // null: a picker then draws no pictures.
    DefinitionThumbnails* thumbnails = nullptr;

    // "Show me what uses this": select these entities and frame them in the
    // active view. The main window supplies it, since only it knows which
    // view is active. May be empty (a test, the headless driver): the dialog
    // then only selects, through document->selection() and
    // notifySelectionChanged(), which is still what a "Select users" button
    // must do.
    std::function<void(const std::vector<katana::entity::EntityId>& ids)> selectAndShow{};

    // The session's survey control codes (start, end, close, arc...) that
    // linework processing reads - configurable, and not survey code file data
    // (cad/linework.hpp). Owned by the maker, which keeps them for the
    // session. May be null: a dialog then shows the defaults and cannot
    // change them.
    katana::entity::LineworkCodes* lineworkCodes = nullptr;

    // The window's one executor (command_runner.hpp): a dialog that changes
    // the drawing builds the verb line a person would type and runs it here,
    // so it is echoed, kept in the history and undone as a typed line. May be
    // empty (a test, a dialog built before the window's command line): the
    // dialog then says it cannot run the line rather than doing the work
    // itself.
    CommandRunner run{};

    // Opens the one definition editor on the library definition `name` - which
    // a New does not read. The maker owns the editor, as it owns the managers,
    // so a manager only asks. May be empty (a test, a manager built on its
    // own): the buttons that would ask are then disabled.
    std::function<void(DefinitionEdit what, const std::string& name)> editDefinition{};

    // The session's customisation is about to be committed by an editor - a
    // definition saved or deleted. An editor calls this BEFORE it commits
    // and, when it is handed a function back, calls that AFTER the commit
    // succeeded (never after one that was refused). It is how a session whose
    // customisation is the kept one stays kept across an editor's own change,
    // where a change typed on the command line lasts the session only: what
    // "before" has to remember, and what "after" then does, is the maker's
    // business, and an editor knows neither. May be empty, and may hand back
    // nothing.
    std::function<std::function<void()>()> beginCommit{};
};

} // namespace katana::qt
