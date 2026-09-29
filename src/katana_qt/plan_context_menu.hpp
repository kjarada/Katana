#pragma once

// The plan view's shortcut menu: a right-click with no tool running
// (docs/desktop.md, "The plan view's shortcut menu").
//
// It holds no commands of its own. Its items are the window's existing
// actions, found by object name - Erase is editErase, Move is the tool
// modify.move - so a menu item, its toolbar button and a headless --trigger
// are one action and cannot drift; an action the window does not have (as in
// a test's window of a few actions) is left out rather than faked. What it
// adds is the verb lines a person would otherwise type - CHLAYER, STYLE
// APPLY, COLOR, INFO - which it hands to the window's one executor
// (command_runner.hpp), so each is echoed in the log and is one undo step.
//
// Its own items, by object name:
//   planContextLayer                 Put on Layer, a submenu in the layer tree
//   planContextLayer.<layer path>    CHLAYER <layer path>, quoted when it must be
//   planContextStyle                 Style, a submenu
//   planContextStyle.ByLayer         STYLE APPLY -
//   planContextStyle.<name>          STYLE APPLY <name>, quoted when it must be
//   planContextColour                Colour, a submenu
//   planContextColour.ByLayer        COLOR BYLAYER
//   planContextColour.Choose         COLOR #RRGGBB, the colour asked for
//   planContextInfo                  INFO #<id>, for one entity
//   planContextRepeat                the last tool again, with nothing selected
//
// Opened on a polyline's grip, the grip's own items come first, under a
// section naming it (docs/drawing.md, "Grips"):
//   planContextVertex.Delete         VERTEX DELETE <id> <v>
//   planContextVertex.InsertAfter    VERTEX INSERT <id> #<id>.s<v> after=<v>
//   planContextVertex.Start          STARTVERTEX <id> <v>, a closed polyline's
//   planContextVertex.Move|Height|Fillet|Chamfer|Straighten
//                                    the tool, the vertex its handle
//   planContextSegment.AddMiddle     VERTEX INSERT <id> #<id>.s<s> after=<s>
//   planContextSegment.Insert|Arc    the tool, the segment its handle
//   planContextSegment.Line          VERTEX SET <id> <s> bulge=0, an arc's

#include <functional>
#include <optional>
#include <string>

#include <QColor>
#include <QMenu>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/drawing/grips.hpp"
#include "katana/core/error.hpp"

namespace katana::qt {

struct PlanContextMenuContext {
    const katana::cad::Document* document = nullptr;
    // Where the window's actions are looked up by object name: the window.
    const QObject* actions = nullptr;
    // The window's one executor; a test's own.
    CommandRunner run;
    // The tool Enter at no prompt would run again (ViewWorkspace::lastToolId);
    // "" when none has run, and then there is no Repeat.
    std::string lastToolId;
    // The colour for Colour > Choose, starting from `initial`: the colour
    // dialog in the window, a fixed answer in a test. nullopt when the person
    // cancelled, and nothing is run.
    std::function<std::optional<QColor>(const QColor& initial)> chooseColour;
    // The grip the menu was opened on (ViewportWidget::gripAt), when it was:
    // a polyline's vertex or segment middle gets its own items first.
    std::optional<katana::cad::Grip> grip;
    // Starts catalogue tool `toolId` with `grip` its handle - hot, as a click
    // on the grip before the tool makes it (ToolContext::handles). Unset,
    // the tools are left out.
    std::function<void(const std::string& toolId, const katana::cad::Grip& grip)> startToolOn;
};

class PlanContextMenu final : public QMenu {
  public:
    // Built for the document's selection as it is now: the selection's verbs
    // when something is selected, otherwise selecting and the last tool.
    PlanContextMenu(PlanContextMenuContext context, QWidget* parent = nullptr);

  private:
    // The window's action of that object name, when it has one.
    void addExisting(QMenu& menu, const char* objectName);
    // An item of this menu's own that runs `line` when chosen; offered
    // disabled, saying why, when the line cannot be written (namedLine).
    QAction* addLine(QMenu& menu, const QString& text, const QString& objectName,
                     const katana::core::Result<QString>& line);
    // The layers under `parent` ("" for the roots), `shared` ticked: the
    // layer every selected entity is on, "" when they are on several.
    void addLayerItems(QMenu& menu, const std::string& parent, const std::string& shared);
    // The items of the polyline grip the menu was opened on, when it was.
    void addGripItems();
    // An item that starts `toolId` on the grip; left out without startToolOn.
    QAction* addTool(const QString& text, const QString& objectName, const std::string& toolId);

    PlanContextMenuContext context_;
};

// `verb` followed by `name` as one word of its line, by the one rule every
// dialog writes words with (command_word.hpp): as it is when it is a plain
// word, otherwise in double quotes, which the interpreter's tokenizer takes
// off; a name holding a double quote or a line break, which no line can
// carry, is refused naming `what`.
[[nodiscard]] katana::core::Result<QString> namedLine(const QString& verb, const QString& name,
                                                      const QString& what);

} // namespace katana::qt
