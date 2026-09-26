# Interactive tools

The drawing tools of the Draw, Modify and Annotate menus: state machines in
`katana_cad` (`include/katana/cad/interactive_tool.hpp`,
`src/katana_cad/tools/`), a catalogue that builds the menus, and the host in
`katana_qt` that runs a tool in a plan view (`src/katana_qt/tools/`). The
window around them is `docs/desktop.md`; the engine the tools' commands run
on is `docs/cad.md`.

## State machines in cad, a catalogue for the menus

The owner asked on 2026-09-23 for more CAD tools, with icons and menus, "to
make it professional CAD software". The tools the application had - Point,
Line, Polyline, Rectangle, Circle, Arc, Move, Copy - were a `switch` inside
`ViewportWidget::acceptPoint`, and the editing verbs (Rotate, Trim, Fillet,
...) existed only on the command line. Nothing about a tool's behaviour could
be tested without clicking, and every new tool meant another case in the view
and another hand-made action in the window.

**A tool is now a state machine in `katana_cad`**
(`include/katana/cad/interactive_tool.hpp`). The plan view tells it what the
user did - a snapped point, a picked entity, a typed value, Enter, Undo - and
the tool answers with the next prompt, a rubber-band preview for the cursor
and, when it completes, ONE command, so one undo removes the whole operation.
A tool never edits the document itself. Each is tested through
`tests/cad/tools/tool_driver.hpp`, which feeds it the inputs a user would and
executes what it returns, against geometry worked out by hand.

**One catalogue feeds everything that names a tool**: the menus, the toolbars,
the command-line aliases and the tooltips are built from `ToolInfo` entries,
so adding a tool is writing it and listing it in its family's file. The
catalogue refuses a duplicate id or alias, a lower-case alias and a
single-letter shortcut (a letter typed into a view goes to the command line);
a refusal is kept, not lost, and a test asserts there are none.

**The families are listed explicitly** (`src/katana_cad/tools/families.hpp`),
not self-registered. `katana_cad` is a static archive, and an object file
nothing references is dropped by the linker - taking its tools with it and
saying nothing; `src/katana_surveyio/CMakeLists.txt` measured that trap. The
source files are globbed so that families written at the same time do not
all edit one list; a family that goes missing is a link error, not a
silently empty menu.

**Typed input has one router**, `routeTypedInput`: text that looks like a
point (a comma, or the `@` of relative input) is a point - `x,y`, `@dx,dy`,
`@distance<angle` from the tool's last point - and anything else is a value,
so "12.5" reaches a Circle's radius rather than being refused as a malformed
point, and a clicked point and a typed one are the same input. The command
interpreter still has its own point parser with the same grammar; folding it
onto `parsePointInput` is a follow-up. `parsePointInput` itself now
delegates to the drawing system's `parsePrecisePoint`, so a DMS angle or a
quadrant bearing may follow the `<` of polar input, and the router has an
overload taking the document's drafting settings and the cursor, which the
tool host uses: `<angle` and `=distance` lock the next points, `x,y,z`
reaches a tool's `InteractiveTool::point3d`, and a number a tool refuses at
a point prompt is direct distance entry (`docs/drawing.md`, "Precision
input").

**Icons live with their family** (`src/katana_qt/tools/icons_<family>.cpp`,
by tool id), drawn to icons.cpp's conventions: a 24-unit grid, a 1.7-unit
stroke, neutral for the object and the accent for what the tool does to it. A
tool with no painter shows a framed initial, visibly a placeholder.
`katana_tool_icon_sheet` renders every catalogue tool's icon at menu, toolbar
and large size with its name and aliases, so an icon is reviewed without
launching the application. `tools::ToolInk` duplicates icons.cpp's private
`Ink` while other work is changing that file; merging them is a follow-up.

### The five families

Thirty-six tools in five families were merged on 2026-09-23, each family one
file (or a few) in `src/katana_cad/tools/`, each tool tested through
`ToolDriver` against geometry worked out by hand, and each with an icon. The
aliases are the ones drafters already type, plus the command interpreter's own
spellings of the same verbs, so a word means one thing whichever reads it:

| Family | Tools (aliases) |
|---|---|
| Draw > Lines (`draw_lines.cpp`) | Point (`POINT`, `PO`), Line (`LINE`, `L`), Polyline (`PLINE`, `PL`, `POLYLINE`), Rectangle (`RECTANG`, `REC`, `RECT`, `RECTANGLE`), Polygon (`POLYGON`, `POL`) |
| Draw > Curves (`draw_curves.cpp`) | Circle (`CIRCLE`, `C`) and its Centre Diameter, 2 Points, 3 Points and Tangent Tangent Radius variants; Arc (`ARC`, `A`, three points) and its Start Centre End, Centre Start End and Start End Radius variants - a variant has no verb, being an option of the general tool, as drafters expect |
| Modify > Transform (`modify_transform.cpp`) | Move (`MOVE`, `M`), Copy (`COPY`, `CO`, `CP`), Rotate (`ROTATE`, `RO`), Scale (`SCALE`, `SC`), Mirror (`MIRROR`, `MI`), Stretch (`STRETCH`, `S`), Rectangular Array (`ARRAYRECT`, `ARRAY`, `AR`), Polar Array (`ARRAYPOLAR`), Erase (`ERASE`, `E`, `DELETE`, `DEL`) |
| Modify > Edit (`modify_edit*.cpp`) | Trim (`TRIM`, `TR`), Extend (`EXTEND`, `EX`), Offset (`OFFSET`, `O`), Fillet (`FILLET`, `F`), Chamfer (`CHAMFER`, `CHA`), Break (`BREAK`, `BR`), Break at Point (`BREAKATPOINT`), Join (`JOIN`, `J`), Explode (`EXPLODE`, `X`) |
| Annotate (`annotate*.cpp`) | Text (`TEXT`, `DTEXT`, `DT`), Multiline Text (`MTEXT`, `MT`), Linear Dimension (`DIMLINEAR`, `DLI`), Aligned Dimension (`DIMALIGNED`, `DAL`), Angular Dimension (`DIMANGULAR`, `DAN`), Radius Dimension (`DIMRADIUS`, `DRA`), Diameter Dimension (`DIMDIAMETER`, `DDI`), Ordinate Dimension (`DIMORDINATE`, `DOR`), Baseline Dimension (`DIMBASELINE`, `DBA`), Continue Dimension (`DIMCONTINUE`, `DCO`), Leader (`LEADER`, `LEAD`, `LE`), Balloon (`BALLOON`), Label Objects (`LABELOBJECTS`, `LBL`, `LABEL`) |

**The Annotate tools and the annotation system** (`docs/annotation.md`,
2026-09-25). The Leader tool makes ONE leader entity (`LeaderGeometry`) -
before, it drew a polyline, an arrowhead and a text per line, which drifted
apart when one was moved - with its arrow head and sizes from the layer's
dimension style: a paper-sized style's millimetres as they are, a model-unit
style's converted to paper at the document's annotation scale, so a leader is
made the size it would have been drawn and then keeps its size on paper. It
keeps the old hook rule as its landing: one arrowhead long, only for a note on
a last segment more than 15 degrees off level. A tip clicked within the pick
aperture of a point, line, arc, circle, polyline or text is put ON it and
follows it (`nearestAnchor`), and the note of such a leader may name the
entity's values in braces - `{prop.size} PVC`, `CH {chainage:.1f}` - which
makes it a smart leader read off the entity (`docs/annotation.md`, "Smart
leaders"). A line with a brace is checked as it is typed, a note that would
say nothing is refused with what the entity lacks, and a tip on nothing keeps
its note as typed, braces and all (`tests/cad/tools/test_smart_leader_tool.cpp`). The Angular, Radius, Diameter
and Ordinate Dimension tools (`annotate_dimension_kinds.cpp`) make the
dimension the `DIM` verb makes, through the same builders
(`include/katana/cad/annotation/dimension_build.hpp`), so a picked pair of
lines, arc or circle is FOLLOWED when it is edited; a dimension given by
clicked points measures those points, unless a point was snapped to an
entity's end, middle, centre or vertex, which it then follows
(`docs/annotation.md`, "Associativity"). The ordinate's datum is the
drawing's origin until Datum gives another (`DIM ORDINATE ... datum=x,y`),
kept for the rest of the run as the datum of the drawing's newest ordinate.
The Linear Dimension tool stores the projected aligned dimension described
in `annotate_dimension.cpp` where that can draw what was asked - every
drawing made before the Linear kind holds it - and the Linear kind `DIM
LINEAR` makes for Rotated (a typed angle or two points), for a line between
the origins' levels (once refused) and for snapped origins, which only the
Linear kind can follow. The export draws both (`export_annotation.hpp`), so
the old reason for the projection, that the DXF writer drew only the aligned
kind, no longer holds. Text takes the `TEXT` verb's style, justification and
paper height at its first prompt (`S`, `J`, `P`), and Multiline Text makes one
text of every line typed, as `MTEXT` does (`docs/annotation.md`, "Text in the
window"). A bare `LEADER`, `DAN` or `DIMRADIUS` typed starts
the tool; with arguments the line is the command interpreter's, as
`LINE 0,0 10,0` is. Label Objects (`annotate_label.cpp`, 2026-09-26) is the
`LABEL` verb by hand, made through the same `annotation::createLabels`; a
bare `LABEL` starts it (`docs/annotation.md`, "In the window").

**Chains, balloons and the leader's options** (2026-09-26). Baseline and
Continue Dimension (`annotate_dimension_chain.cpp`) go on from the drawing's
newest linear or aligned dimension - the one the last dimension tool made,
as AutoCAD's go on from the last - and Select picks another; each origin
clicked is one more dimension, Enter makes them all as one command through
`annotation::dimensionChain` (DIM BASELINE's and DIM CONTINUE's door) and
Enter again ends the tool; Esc keeps those placed. Baseline's Spacing
overrides a text height and a half of the base's style. Balloon
(`annotate_balloon.cpp`) is BALLOON by hand, numbered by
`annotation::nextBalloonNumber` unless Number says otherwise, with Style and
Paper - BALLOON's `style=` and `paper=` - at every prompt, for the balloon
being drawn. The Leader's first prompt takes Arrow, Callout, Style, Paper,
SIze and Landing - LEADER's `arrow=`, `callout=`, `style=`, `paper=`,
`arrowsize=` and `landing=` - for the leader being drawn; SIze is two
letters because S is Style's, and Landing's `Auto` gives back the rule of one
arrowhead where the line slopes. Until 2026-09-26 the tools offered only
some of their verbs' options, so a leader's own arrowhead size and landing,
and a balloon's style and height, could be given only by typing the verb.
The text style is found by `textStyleNamed` (`annotate_common.hpp`), by its
name or ignoring case, the same for both. What a tool
starting again should remember is read from the drawing, not kept by the
program: the newest dimension to go on from, the newest ordinate's datum,
the highest balloon, the newest hand-placed label's style. Each of these
tools is tested against the verb line that makes the same entities.

The sixth family in `families.hpp`, Inquiry (Distance, Area, ID Point, Angle,
List), is listed and empty. The drawing system added Draw > Vertices
(`modify_vertex.cpp`, eighteen tools gathered into a Vertices submenu of
Draw by their "Vertices, <tool>" names) and the professional draw tools
(`draw_professional.cpp`: 3D Polyline, Construction Line, Ray, Double Line,
Freehand Sketch, Revision Cloud, Spline, the Ellipse submenu, Circle Tangent
Tangent Tangent and Arc Start End Direction); `docs/drawing.md` has their
tables. The Survey menu's tools (`docs/survey.md`) are
dialogs, not catalogue tools.

**One command per tool session.** A tool that completes returns ONE command,
so one undo removes the whole operation: a chain of Line segments, every copy
Copy made before Enter, every cut of a Trim. The edit tools need more than
that, because a later pick can land on a piece an earlier pick made - which
has no id until a command runs - and a fillet must keep the side of each line
the user picked. So Trim, Extend, Offset, Fillet, Chamfer, Break and Join
work on an `EditSession` (`modify_edit_support.hpp`, family-internal): a
layer over the document that records what the session has made of each
entity and what it added, with `begin()`/`undo()` per operation for the `U`
inside the tool, and `commit()` turning the whole session into one command
when the tool finishes, as a CAD program's U does after a TRIM. Every piece is
checked as it goes in (`drawable`, the geometry's own `validate`): if one is a
shape the drawing would refuse, the whole operation since `begin()` is
abandoned with a sentence saying why, so the command a session becomes is
always one the drawing accepts.

**What the edit tools remember and refuse.** `EditDefaults` holds the fillet
radius, the two chamfer distances, the offset distance (Through until one is
typed) and a 1 mm join tolerance, one object per program, as CAD programs keep
the last fillet radius, chamfer distances and offset distance - a user who
fillets at 5 expects 5 next time - and nothing of it is in the drawing. Offset refuses an INWARD
offset beyond the polyline's local feature size: `geometry::offset` mitres
each vertex, and past that size the sides pass each other, so an 8 x 8 square
offset inward by 5 came out as a 2 x 2 square drawn the other way round when
no such offset exists (`keepsItsSides`). Offset also refuses an object on a
locked layer, since the copy would land there. Break at Point refuses a
circle ("A circle has no ends, so one point cannot split it; use Break with
two points."). Fillet and Chamfer take lines only, Join always makes a
polyline, and a pending offset cannot itself be picked within the session.

The modify-edit tests reach `modify_edit_support.hpp` through an include
path of their own (`tests/cad/CMakeLists.txt`), as the family's sources do.

### The tool host: how a view runs a tool

The plan view's own `switch` is gone. Every tool the view runs is a catalogue
tool, run by a **`ToolHost`** (`src/katana_qt/tools/tool_host.*`) that sits
between the tool, which knows nothing of Qt, and the view, which knows nothing
of any one tool. It starts a tool with the document's current attributes
(layer and style, D9) and the live selection, hands it what the user did - a
snapped point, a picked entity, typed text, Enter, Esc, Undo - and when the
tool finishes executes its ONE command through the Document. It restarts the
tool when the tool asks (Circle, Point), and gives the tool the view's pick
aperture in model units, so Trim's preview and picks match the zoom. It
reports through hooks and opens nothing, so a test drives it by calling it
(`tests/qt_widgets/tools/`). A generation count, bumped whenever a tool is
made, remade or dropped, is how the host knows that a hook replaced the tool
it was dealing with: a tool started from a hook was once allocated at the
address of the one it replaced.

**Esc keeps collected work.** Esc ends the tool, but a tool holding work
that its Enter only ever COMMITS - a Line or Polyline chain, the cuts of a
Trim or Extend, Offset's copies, a run of Fillets or Chamfers - is sent Enter
first, so Esc keeps that work as drafters expect a LINE's segments to stay
(`tools::escapeKeepsWork`, a list of tool ids). Every other tool is dropped
with nothing done, because its Enter at some step applies a DEFAULT - Move's
"use the first point as the displacement", Join's "join what is selected" -
which Esc must never do; Copy is dropped for that reason although its placed
copies are collected work. A Fillet or Chamfer at a VALUE prompt (its radius,
its distances) is first stepped back out of it: Enter there takes the prompt's
default - at Chamfer's second distance it stores both distances for every
later Chamfer - or, at Fillet's radius, only returns to the lines, and the
corners a Multiple run made would go with the tool. The list stands in for a
`cancel()` the tool interface does not have; a virtual commit-on-cancel on
`InteractiveTool` would replace it.

**A replaced drawing ends the tool.** New and Open call
`ViewWorkspace::resetInteraction`, which ends the running tool WITHOUT
committing anything (`ToolHost::abandon`): its picks and ids belong to the
drawing that is going, and restarting it would read the old drawing's layer
and selection. So does a plan view that is closed or changed into another
kind while its tool runs (`ViewWorkspace::stopToolIn`, called by `closeView`
before it finds the view to erase and by `buildContent`) - the view stops
the tool while it can still say so, which is what un-checks the tool in the
menus.

**Typed input belongs to the running tool.** While a tool runs, what is typed
over the view is kept in the view (`typedInput()`) and shown after the prompt
in a band along the bottom of the view, until Enter or Space sends it
(Space is a space inside a value being typed), Backspace takes a character
back and Esc clears it. With no tool running a printable key is the start of
a command and goes to the window's command line ("type anywhere",
`onTextTyped`), never a Ctrl or Alt chord, which is a shortcut. On the command
line itself (`MainWindow::runCommandLine`):

- while a tool runs, the whole line is the tool's answer
  (`ViewWorkspace::typeIntoTool`) - a point, a distance, an option - so
  Polyline's `C` closes it where on its own `C` would start a Circle. Nothing
  typed is transparent: `ZOOM` typed during a tool goes to the tool too;
- with none running, a single word that is a tool's alias or its catalogue id
  starts it (`tools::toolIdForCommand`: `L`, `line`, `TRIM`, `draw.circle.ttr`,
  aliases case-insensitively), and the tool's action is checked in the menus
  and toolbars as if it had been clicked; with arguments the word is the
  interpreter's (`LINE 0,0 10,0` draws at once);
- an empty line is Enter in the drawing (`ViewWorkspace::pressEnter`),
  delivered as a real Return key to the plan view running a tool - or, with
  none running, to the active plan view, which starts the last tool again -
  so the command line and the view share one Enter path.

`qt_a_tool_started_by_its_alias_draws_from_typed_points_headless` types
`LINE`, two points and two empty Enters, and checks the line and the check
marks.

**Ctrl+Z inside a tool is the tool's.** The view claims the key at
`ShortcutOverride`, so the window's Undo does not run, and acts on it at the
key press, since Qt may ask more than once for one press. It steps back the
tool's last input - the `U` inside LINE - and a tool with nothing to step
back says so rather than undoing the drawing: the drawing's Undo is Esc and
then Ctrl+Z.

**A tool's selection step gathers.** At a step that wants objects, a plain
click or box ADDS what it picks, as a CAD program's "Select objects" prompt does, and one
with Shift or Ctrl held takes it back out; replacing the selection at each
click, as the Select tool does, would leave only the last of several cutting
edges picked. The picks change the DOCUMENT's selection, which the tool reads
when Enter is pressed, so the view keeps what each click or box replaced and
Ctrl+Z (or a typed `U`) at that step takes them back in the order they were
made, together with any input the tool took itself (`All`).

**Enter or Space repeats.** With no tool running, Enter or Space in a plan view
starts the last tool again, the usual CAD repeat of the last command: a run of
circles is a click on Circle and then Enter between them. The workspace keeps
the last tool started in ANY plan view (`onRepeatTool`), so the repeat goes
through `ViewWorkspace::startTool` like every other start.

**One tool per workspace, and Esc to the busy view first.** A tool holds picks
made in one view, so it runs in the active plan view only, and starting one
stops, as Esc stops it, a tool running in any other plan view; it stays in the
view it started in, although some CAD programs carry a command across viewports. Esc
(`ViewWorkspace::cancel`) reaches only the views running a tool or holding
typed input for one; only when none is does every plan view abandon its box
and clear the selection - so the first Esc ends the tool and keeps the
selection it was started on, and the second clears it.

**The menus are the catalogue.** `tools::fillToolMenus`
(`src/katana_qt/tools/tool_menus.*`) builds the Draw, Modify and Annotate
menus and toolbars from `cad::toolCatalog()`: one `QAction` per tool, shared by
its menu and its toolbar and named by the tool's id (`draw.line`), with its
family's icon, its shortcut, its tip as the status tip and a tooltip naming its
aliases ("Line (LINE, L)"). Groups are separated in a fixed order (Lines,
Curves, Transform, Edit, Text, Dimensions, Leaders); tools named "Family,
Variant" ("Circle, 2 Points") are gathered into a submenu by family, and on a
toolbar the family is ONE button that runs its first variant and drops the
rest down. A category no menu takes is still reached by its aliases, and the
window says so in its log at start-up. An action only asks the window to
start its tool (`MainWindow::startTool`), which decides the view. The tool
actions are checkable, in one exclusive group; Select (`toolSelect`, at the
head of the Draw toolbar) is not a tool but the absence of one: it stops what
runs, and is checked while nothing does. What is checked always follows what
runs (`MainWindow::showRunningTool`, from `onActiveToolChanged`), including
when a start is REFUSED - a click checks an action before its handler runs, so
a tool that could not start, for want of a plan view, used to stay checked
beside Select (`qt_a_tool_refused_for_want_of_a_plan_view_is_left_unchecked_headless`).
While a tool runs its prompt is the command line's placeholder text, where the
answer is typed.

Not done: `ViewportWidget` still has the `enum class Tool` of the first eight
tools and `setTool`, used for Select (`stopToolIn` calls it) and by
`test_plan_view_tools.cpp`; the window and the workspace name tools by id.
The picks of an entity step (Trim's edges, the part to cut) are not
highlighted, and `ToolContext` carries no view's layer overrides.
