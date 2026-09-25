#pragma once

// Interactive tools: the drawing, editing, annotation and inquiry tools the
// plan view runs (the owner's request of 2026-09-23: "more CAD tools ...
// make it professional CAD software").
//
// A tool is a STATE MACHINE, and it lives here in cad rather than in the view.
// The plan view tells it what the user did - a point picked (already snapped),
// an entity picked, a value typed, Enter, Undo - and the tool answers with the
// next prompt, a rubber-band preview for the cursor and, when it completes,
// ONE command for the document to execute. So every tool is unit tested by
// feeding it a sequence of inputs and comparing the entities its command
// produces with geometry worked out by hand, with no Qt anywhere. The tools
// they replace were a switch inside ViewportWidget::acceptPoint that nothing
// could test without clicking.
//
// Tools are listed in a catalogue (ToolCatalog) that the menus, toolbars,
// command-line aliases and status tips are all generated from, so a tool is
// added by writing it and adding it to its family's list - never by editing
// the window.

#include <cstddef>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/commands/command.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// What the tool wants next. The view uses it to decide what a click means,
// which cursor to show and whether snapping applies.
enum class ToolInput {
    Point,     // a click is a point; snapping applies
    Entity,    // a click picks the entity under the cursor (Trim's edges, Offset's source)
    Selection, // a click selects as the Select tool does; Enter ends the selection
    Value,     // only typed input is meaningful (a count, a text string)
};

[[nodiscard]] const char* toString(ToolInput input);

// The tool's answer to one input.
struct ToolStep {
    enum class Outcome {
        Continue, // input accepted; more is needed (see prompt())
        Done,     // the tool has finished; execute `command` if there is one
        Rejected, // input refused; `message` says why, and the tool is unchanged
    };
    Outcome outcome = Outcome::Continue;
    // Done: what to execute - ONE command, so one undo removes the whole
    // operation. Null for a tool that only reports (Distance, List) or that was
    // finished with nothing to do.
    katana::commands::CommandPtr command;
    // Rejected: why. Otherwise an optional line for the command log - a
    // measurement, or what was made ("3 lines").
    std::string message;
    // Done: start the tool again from its first step, as a CAD user expects
    // of Circle or Text - press Esc to stop. False for tools whose next use
    // needs a fresh selection (Move, Rotate).
    bool restart = false;
    // Done: the selection to leave behind, for a tool whose answer IS a
    // selection (Select Similar, Quick Select). The host puts it in the
    // document's selection set when the step's command, if any, has run;
    // nullopt leaves the selection as it is. A selection is not a command -
    // it is not undone with the drawing - so it travels beside one.
    std::optional<std::vector<katana::entity::EntityId>> selection;

    [[nodiscard]] static ToolStep next(std::string message = {});
    [[nodiscard]] static ToolStep rejected(std::string why);
    [[nodiscard]] static ToolStep done(katana::commands::CommandPtr command,
                                       std::string message = {}, bool restart = false);
};

// The rubber band: what the view draws for the current cursor position, in
// model coordinates, over the drawing and in a distinct pen.
struct ToolFeedback {
    std::vector<katana::entity::Geometry> shapes;
    // Points worth marking: a base point, a polygon's centre, a picked vertex.
    std::vector<katana::geometry::Point2> markers;
};

// What a tool is given when it starts. Everything is by value or const: a tool
// never changes the document itself, it returns a command that does.
struct ToolContext {
    const Document* document = nullptr;
    // For anything the tool creates: the current layer, style and colour.
    katana::commands::EntityAttributes attributes;
    // The selection when the tool started, in ascending id order. Transform and
    // edit tools act on it; when it is empty they begin by asking for one.
    std::vector<katana::entity::EntityId> selection;
    // The view's pick aperture in model units, for a tool that finds geometry
    // near a point itself (the view does the picking for ToolInput::Entity).
    double pickTolerance = 0.0;
};

class InteractiveTool {
  public:
    virtual ~InteractiveTool() = default;

    // The prompt for the NEXT input, as it goes in the command line: what is
    // wanted and any options, "Specify next point or [Close/Undo]".
    [[nodiscard]] virtual std::string prompt() const = 0;
    [[nodiscard]] virtual ToolInput expects() const = 0;

    // The defaults reject the input with a sentence saying what is expected
    // instead, so a tool implements only the inputs it takes.
    [[nodiscard]] virtual ToolStep point(const katana::geometry::Point2& at);
    // A point with a height: typed as x,y,z, or snapped to a point that has
    // one. The default drops the height and takes the point, so only a tool
    // that draws in 3D (Polyline 3D, Point) overrides it.
    [[nodiscard]] virtual ToolStep point3d(const katana::geometry::Point2& at, double z);
    [[nodiscard]] virtual ToolStep entity(katana::entity::EntityId id,
                                          const katana::geometry::Point2& at);
    // Typed text that is not a point (routeTypedInput decides): a distance, an
    // angle in degrees, a count, a text string, or an option keyword from the
    // prompt ("C" for Close).
    [[nodiscard]] virtual ToolStep value(std::string_view text);
    // Enter, Space or a right-click: finish (Polyline), accept the default, or
    // end a selection. The default finishes with nothing to do.
    [[nodiscard]] virtual ToolStep enter();
    // Steps back one input (the U inside LINE). The default rejects.
    [[nodiscard]] virtual ToolStep undo();
    // Esc: the tool is ending, and this says what of its work stays. Done
    // with a command keeps work the user has already placed step by step -
    // a LINE chain's segments, the copies Copy has put down, the cuts a Trim
    // has made - as AutoCAD keeps them; Done with no command drops the lot.
    // It is not enter(): Enter at some steps applies a DEFAULT (Move's "the
    // base point as the displacement", Copy's with nothing placed yet),
    // which Esc must never do. The host ends the tool whatever this answers
    // and ignores `restart`. The default keeps nothing.
    [[nodiscard]] virtual ToolStep cancel();
    // The rubber band for the cursor. The default draws nothing.
    [[nodiscard]] virtual ToolFeedback preview(const katana::geometry::Point2& cursor) const;
    // The last point accepted, which relative input (@dx,dy) and the
    // Perpendicular and Tangent snaps measure from. Nullopt before the first.
    [[nodiscard]] virtual std::optional<katana::geometry::Point2> lastPoint() const;
};

// Parses a typed point. "x,y" is absolute; "@dx,dy" is relative to `last`;
// "@distance<angle" is polar from `last`, the angle in degrees counter-
// clockwise from east (the command line's convention). Fails with
// InvalidState for relative input with no `last`, and ParseFailure otherwise.
// Numbers go through core/text.hpp, so the decimal point is always '.'.
[[nodiscard]] katana::core::Result<katana::geometry::Point2>
parsePointInput(std::string_view text, std::optional<katana::geometry::Point2> last);

// What the view and the command line do with typed text while a tool runs:
// text that parses as a point is a point, anything else is a value. The one
// place that decides, so a click and "10,20" typed are the same input.
[[nodiscard]] ToolStep routeTypedInput(InteractiveTool& tool, std::string_view text);

// The same with the drafting aids (drawing/drafting.hpp), as the plan view
// and the command line route it:
//   <angle         locks the direction of the next points (< alone clears)
//   =distance      locks their distance (= alone clears)
//   x,y,z          a point with a height (InteractiveTool::point3d)
//   a number       the tool's value first (a radius, a count); if the tool
//                  refuses it at a point prompt, DIRECT DISTANCE ENTRY - the
//                  point that far from the last one towards `cursor`
// Points parse with the drafting settings' angle convention, so a bearing
// or a DMS angle can follow the < of polar input.
[[nodiscard]] ToolStep routeTypedInput(InteractiveTool& tool, std::string_view text,
                                       DraftingSettings& drafting,
                                       std::optional<katana::geometry::Point2> cursor);

// ---- The catalogue -----------------------------------------------------------------

struct ToolInfo {
    // Stable identity, "draw.line": menus, toolbars, settings, shortcuts and
    // tests name a tool by it, so it never changes once shipped.
    std::string id;
    std::string name;     // as the menu shows it: "Line"
    std::string category; // the menu it goes in: "Draw", "Modify", "Annotate", "Inquiry"
    std::string group;    // the section within that menu: "Lines", "Curves", "Transform", ...
    int order = 0;        // position within the group, ascending
    // Command-line verbs that start it, upper case: {"LINE", "L"}. Unique
    // across the catalogue - one word, one tool.
    std::vector<std::string> aliases;
    // A QKeySequence string ("Ctrl+Shift+L"), or empty. Single letters are
    // not allowed: a letter typed into a view goes to the command line.
    std::string shortcut;
    std::string tip; // one sentence: what it does and how
    std::function<std::unique_ptr<InteractiveTool>(const ToolContext&)> make;
};

class ToolCatalog {
  public:
    // InvalidArgument for an empty id, name, category or group, no factory, a
    // lower-case or empty alias, or a single-letter shortcut; AlreadyExists for
    // an id or an alias already taken.
    katana::core::Status add(ToolInfo info);
    [[nodiscard]] const ToolInfo* find(std::string_view id) const;
    // By command-line verb, case-insensitively.
    [[nodiscard]] const ToolInfo* findByAlias(std::string_view verb) const;
    // Ordered by category, then group, then order, then id: the menu order,
    // the same on every run.
    [[nodiscard]] std::vector<const ToolInfo*> all() const;
    [[nodiscard]] std::size_t size() const { return tools_.size(); }

  private:
    std::vector<ToolInfo> tools_;
};

// The program's catalogue, built on first use from every tool family's list
// (src/katana_cad/tools/families.hpp). A family whose tool fails to register
// is a programming error; building it throws nothing but logs through the
// returned statuses in toolCatalogProblems(), which a test asserts is empty.
[[nodiscard]] const ToolCatalog& toolCatalog();
[[nodiscard]] const std::vector<std::string>& toolCatalogProblems();

} // namespace katana::cad
