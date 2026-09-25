# Desktop application

`katana` (the target in `src/katana_qt`): the window, its theme, icons and
toolbars, the workspace of docked views, the panels and the managers, and the
rules every dialog follows. What the window drives - the document, selection,
snapping, the interpreter and the engineering computations - is `katana_cad`,
in `docs/cad.md`; the drawing tools and their host are `docs/tools.md`; the
switches that run the window headlessly are `docs/headless.md`.

## The window

The window (`MainWindow`) is a `QMainWindow` around a workspace of views, with dockable Layers, Properties, Command Line and
Reference Data panels. Its menu bar reads in the order a CAD user reads it -
File, Edit, View, Draw, Modify, Annotate, Format, Survey, Terrain, GIS, Help
(`MainWindow::buildActions`; each menu has an object name, `fileMenu` to
`helpMenu`). File keeps to files: the customisation is loaded from Format
and from Survey > Survey Coding, beside the managers of what it brings. The
status bar shows a view's running readout, the current layer, the active snap
and the cursor coordinates.

The readout (`FrameStatsLabel`) is a 3D view's frame time or a section's
station and elevation under the cursor, forwarded from whichever view raised
it (`ViewWorkspace::onFrameStats`) and kept apart from `onStatus`, which is
for messages, so the status bar's message never hides it. It shows the most
recent view's readout, not only the active view's, and keeps its last text
while a plan view is active. It has no automated test; a screenshot of a 3D
view is what checked it.

The plan viewport draws with `QPainter` and turns mouse input into commands. It
holds no geometry of its own: it paints whatever the model contains (Rule 3).
It is NOT on the `render::DrawList` path: the tile-binned software rasteriser
(`docs/render.md`) draws the 3D and Elevation views, and a GPU backend would
replace that rasteriser, not this painting code.

Interaction: left click picks, or gives the running tool its point or its
entity; dragging left-to-right is a window selection and right-to-left a
crossing selection (shown by a solid or dashed rubber band); middle-drag pans;
the wheel zooms about the cursor; Delete erases the selection. With no tool
running, Shift adds to the selection and Ctrl toggles; Enter or Space starts
the last tool again; Esc abandons a box and then clears the selection; and a
right-click cancels as Esc does, since nothing is wired to the view's context
menu (`ViewportWidget::onContextMenu`). While a tool runs, Enter, Space and a
right-click are the tool's Enter, and Esc ends the tool first. What a tool
does with each is `docs/tools.md`, "The tool host".

Two rendering details are worth noting. Arcs and circles are tessellated in
*model* space with a chord count chosen for a sub-quarter-pixel sagitta, so a
very large radius with only a sliver on screen never hands Qt coordinates in the
millions. Text below three pixels tall is drawn as a baseline stroke rather than
glyphs, so a zoomed-out drawing stays legible instead of dissolving into
unreadable marks.

Layer visibility, locking and colour are edited in the layer panel or the
Layers dialog (Format > Layers..., Ctrl+L); each edit is a command, so it
participates in undo. Choosing the CURRENT layer is
`Document::setCurrentLayer`, deliberately not a command - it changes what the
next drawing tool does, not the drawing (`docs/cad.md`, "Document"). Opening a project whose
database is damaged offers to restore the newest sound backup.

**The current style and the Style row (decision D9).** The Properties
toolbar carries the style new work is drawn in (`CurrentStyleCombo`): ByLayer
first, then the drawing's styles by name. Choosing one is
`Document::setCurrentStyle`, session state like the current layer and so not a
command, and the log says "New work is drawn in ...". It acts on the box's
`currentIndexChanged`, kept out while `MainWindow::refreshStyleChoices`
rebuilds the list, so a headless `--fill` sets it as a person's pick does. A
current style the list does not hold is shown, marked "(not in the drawing)",
never replaced by the first choice - the QT-02 lesson. The Properties panel
has a Style row above its read-only table (`PropertyStyle`, editable, and
`PropertyStyleApply`): it shows the selection's style as it is, even a name
the drawing lacks, or "<varies>" when the selection disagrees, and Apply or
Enter gives the selection the style typed or chosen as one undo step
(`MainWindow::applyPropertyStyle`, `commands::setEntityStyle`). Only the
entities that change are in that step, so applying the style already shown
is no step at all; and nothing is applied from the box's own change signal.
A rename or delete of the current style clears it rather than following it
(`docs/cad.md`, "Document"), so the toolbar then shows ByLayer.
`qt_current_style_and_the_style_row_headless` drives both.

## Failure modes

Every command failure surfaces as a message in the command log and, for errors,
the status bar; the model is untouched. Closing the window asks twice, in this
order (`MainWindow::closeEvent`): first the Survey Code Manager, whose
unapplied rule edits live only in its buffer - it is shown and asks Apply /
Discard / Cancel over the rules it is about (`CustomisationWorkbench::confirmClose`),
first because its Apply changes the drawing the next question is about - and
then, with unsaved changes, save, discard or cancel (`confirmDiscard`). A
headless run has nobody to answer either, so it refuses and says why in the
log, never discarding anything unasked: a scripted `QUIT` with unapplied code
edits, or `NEW` after an edit, is refused, and the script can Apply or Revert
(`applyMap`, `revertMap`), `SAVE` or `UNDO` first. A failed save reports the
error and leaves the modified flag set. A damaged project offers backup
recovery and never deletes the damaged file.

## The look of the application: theme, icons, toolbars

The viewport has been dark (`#1e2329`) from the start, and the window around
it was whatever grey Qt defaults to. A dark drawing in a light frame is most
of why the application looked unfinished. Three files fix that, and each was
shaped by what this toolchain does and does not have.

**`theme.hpp` - every chrome colour is a named token.** `window`, `panel`,
`raised`, `hover`, `border`, `text`, `textMuted`, `accent` and so on, as
functions. The stylesheet is ASSEMBLED from them rather than written with
colour literals, so the tokens are the only place a colour is decided and a
change of theme is a change to one file. Nothing else under `src/katana_qt`
should contain a widget colour literal; the viewports keep their own constants
for DRAWING colours (grid, snap marker, selection), which are content, not
chrome. The style is Fusion, because it is the one built-in style that honours
a palette completely - the native Windows style draws light controls into a
dark palette. The accent is blue, not the red of the logo: in an engineering
program red already means "error", and a checked tool must not read as one.

**`icons.hpp` - the icons are drawn in code.** Each is a vector drawing on a
24-unit grid painted by a `QIconEngine` at whatever size and device-pixel ratio
Qt asks for. There are no image files behind them, for four reasons: they are
crisp at every size because nothing is resampled; they take their colours from
the theme, so a theme change recolours all of them; a function that is wrong
does not compile, where a resource path that is wrong is a blank button; and
this toolchain has no Qt SVG module and no image tools, so an SVG or PNG set
would have been a dependency added for decoration. The language is one rule
applied everywhere: a 1.7-unit round stroke, neutral for the object and ACCENT
for the part the command acts on - the arrow of Import, the new geometry of a
draw tool, the cut line of a section. 1.7 rather than the usual 2.0 because at
16 px the heavier stroke closes up small counters (the label of Save, the
inside of the magnet).

**The application icon comes from the same painter.** `katana_make_icons`
writes `resources/katana.ico` (nine sizes, PNG-compressed entries), a 256 px
PNG, and `icon_sheet.png` - every icon at 96 px and at the 20 px it ships at.
The `.ico` is COMMITTED, not generated in the build: generating it would make
an ordinary build run a Qt program before it could compile a resource file,
which fails wherever that program cannot start, and buys nothing for an icon
that changes once a year. Below 64 px the sword is drawn larger and heavier
within its tile; at true proportions it is a few pixels wide on a taskbar and
disappears. `katana.rc.in` embeds the icon and a `VERSIONINFO` block whose
numbers come from `project(Katana VERSION ...)`, so a version is written in
exactly one place.

**The contact sheet earned its keep at once.** The first application icon had
the handle running UP the blade with the guard at the pommel - a sign error in
the tangent - and the Circle tool read as a minus sign in a ring. Both were
obvious on the sheet and invisible in the code.

**Toolbars share their actions with the menus.** `MainWindow::makeAction`
makes each `QAction` once - text, shortcut, icon, status tip and a tooltip
that names the command and shows its shortcut, since an icon-only button owes
the user its name - and the same object goes into the menu and the toolbar, so
the two cannot drift. File, Edit, Properties (the current style), View and
Format run along the top; Survey, Terrain and GIS have a second row of their
own (`addToolBarBreak`), because in one row they were squeezed to a button
each behind overflow arrows. Draw and Annotate run down the LEFT edge, where
every CAD program keeps its drawing tools, because they are the ones reached
for without looking and a strip beside the drawing is a shorter journey than a
row above it; Modify runs down the RIGHT edge, as in the classic CAD layout,
because one column could not hold all three without hiding the last tools
behind an overflow arrow. Every toolbar and dock has an object name, which is
what `QMainWindow::saveState` keys a layout on. View > Panels brings back any
dock that has been closed.

**Every key reaches one thing.** `MainWindow::shortcutClashes` gathers every
key a person can press - each action's key sequences, any `QShortcut`, each
menu's Alt letter and each item's underlined letter within its own menu (two
items with one letter make the key cycle between them instead of choosing) -
and lists those that reach more than one thing, since Qt disables an
ambiguous shortcut for both. `katana --check-shortcuts` fails a run that has
any, and `qt_every_shortcut_and_menu_letter_reaches_one_thing_headless` runs
it. Ten underlined letters were changed to pass it (A&ttributes, Sym&bol
Library, Loa&d Customisation, In&verse, Toggle Pe&rspective and others).
The tool actions built from the catalogue have no underlined letters; their
aliases are in their tooltips.

`katana --screenshot` grabs the window headlessly so that the look can be
reviewed, and the same run can drive the menus and dialogs step by step:
`docs/headless.md`.

**Not done.** Settings are not persisted: toolbar positions, dock layout and
window geometry are not saved between sessions although every object now has
the name that would allow it (the dock chrome has the hooks, below). There is no
light theme. The dialogs (corridor, plot) are themed but plain. View >
Viewport Layout's actions still have no object names (View > Active Viewport
Shows has them: `viewShowsPlan`, `viewShows3D`, `viewShowsSection`,
`viewShowsElevation`). `resources/icon_sheet.png` is regenerated by
`katana_make_icons` and now carries the Survey icons and the four Format ones
(Purge Unused is a broom, not a second bin).

## The workspace: every view is a dock, and each has its own layers

The user's request of 2026-09-23 was for panels and views that dock, move,
minimise, close and come back from the menus, views that go out onto another
screen, and per-view control of which layers show. Until then the views were
tiles: `ViewportContainer` placed widgets at the rectangles of a fixed split
(`cad::ViewportLayout`), so a view was identified by its POSITION. Every layout
change destroyed and rebuilt every widget, which lost the plan's zoom, every
section but the first and any half-picked clicks - and the application changed
the layout on its own, whenever a surface was built or a section cut.

**Each view is now a `QDockWidget` inside a nested `QMainWindow`**
(`katana_qt/view_workspace.*`), which is the main window's central widget. Qt's
dock machinery gives splitting, tabbing and floating onto any screen for free.
A NESTED window rather than the main window's own dock areas, because an
arrangement ("Four: Equal") must move views and never the Layers panel, and a
panel should not be droppable between two views. Rejected alternatives:

* *Keep the tiles and add pop-out windows.* Two ways of showing a view, and the
  tiled half still rebuilds on every change.
* *Qt Advanced Docking System* (LGPL-2.1+, packaged by MSYS2 for ucrt64 but not
  installed). It would give auto-hide side bars and native floating frames with
  a real minimise button off the shelf. It is a new third-party dependency, a
  change to the bundle and a licence decision, which belong to the owner; the
  plain-Qt design does not preclude moving to it later, because the views and
  their state do not know what hosts them.

**A `QMainWindow` makes itself a top-level window whatever parent it is given**
(its constructor ORs in `Qt::Window`), and a window placed as another main
window's central widget is an EMPTY layout item. The first build of the
workspace was given no space at all and every view vanished while the panels
filled the window. `setWindowFlags(Qt::Widget)` in the constructor is what
makes it a child; the comment there says so.

**A view's state lives in `cad::ViewSet`, not in its widget.** `ViewState`
holds the kind, the 3D camera, the plan zoom, the layers hidden in that view,
the reference layers hidden in it and its section. Views are identified by a
`ViewId` that is monotonic and never reused, like entity ids, so a queued event
for a closed view cannot find another. States are held by `unique_ptr` because
each widget holds a reference to its state; closing a view deletes the dock
(and so the widget) BEFORE dropping the state. Changing a view's kind replaces
only the widget inside the dock, so a 3D view switched to plan and back keeps
its orbit. `ViewSet::mostRecent(kind)` is what Plot, F9 and the Standard Views
act on: the active view when it is that kind, otherwise the one of that kind
the user touched last - so F9 still reaches the 3D view after a click in the
plan. The layout presets survive as arrangements: `cad::dockSplits(kind)` lists
the `splitDockWidget` steps that build each one, and a test asserts that
applying them to rectangles reproduces `layoutRects(kind)` exactly.

**Showing a view never replaces one.** Building a surface or importing meshes
used to split the window and turn a cell into 3D; cutting a section turned the
ACTIVE cell into a section, which was the plan view being drawn in whenever the
user had just clicked there to select the alignment. `ensureView(kind)` raises
the view of that kind used most recently, or opens a new one beside the active
view.

**Zoom Extents frames the active view only**; one view's zoom is not another's
business, and the old container did one thing while its header said another.
After New, Open and an import every view is framed (`zoomExtentsAll`), because
all of them are looking at a drawing that has just changed under them.

**Per-view layers extend THE visibility rule rather than adding a second.**
`cad::isDrawn` and `isSelectable` take a `const LayerOverrides&` - the layers
one view hides - with NO default argument, so every caller has to say whether
it means a view or the document (`kNoLayerOverrides`); a default would let the
next consumer ignore the view it draws in, which is how the 3D view once came
to ignore layer visibility altogether (audit REN-03). The overrides are
SUBTRACTIVE: a view can hide what the document shows, never show what it hides,
so the rule stays a plain conjunction and a layer switched off in the Layers
panel cannot linger in some forgotten view. Hiding a path hides everything
beneath it, the document rule's ancestor semantics (`docs/model.md`, "Layer
visibility and lock"), tested by
walking the path's ancestors as `string_view` slices - no allocation, and an
immediate return for a view that hides nothing, since this is asked once per
entity per frame. The overrides reach snapping (`SnapRequest::view`), picking
and box selection (`SelectionFilter::view`) and the 3D scene
(`SceneOptions::layers`), so a layer hidden in a view can be neither snapped to
nor picked there - otherwise Delete would erase something the user cannot see.
Three consumers use the document rule on purpose, because their result is
shared and must not depend on which view was clicked last: the section cut
(a view is to hide the crossings on its hidden layers when it paints them),
the typed SELECT, and Surface From Drawing.

Per-view layers are view state: not saved in the project, not undoable, and
not a document change - so nothing hears them through the document listener,
and the workspace repaints the one view that changed. Document listeners carry
no payload, so a view cannot follow a layer rename; every view's overrides are
pruned of names that no longer exist after each change, which is what stops a
later layer reusing the name from being born hidden. A view's Layers button
(`view_layers_popup.*`) is where they are set: it filters that view alone,
and isolating a tree node hides everything but that node's path.

### The dock chrome: one title bar for panels and views

The same request asked for panels and views that minimise, float, maximise
and close. `QDockWidget`'s own title bar has two buttons, Float and Close,
drawn by the style - and Fusion drew them light on the dark theme, which is
why `theme.cpp` used to strip them, so no dock could be closed or floated
except by dragging. It has no Minimise, no Maximise and nowhere for a view's
own controls. `DockTitleBar` (`src/katana_qt/dock_chrome.*`) replaces the
whole bar: `[icon] title ... [tools] [_] [float] [max] [x]`, with a view's
kind switcher where the icon is and its Layers button among the tools. Qt then
draws no buttons and gives a floating dock no native frame; it still resizes
one from its edges (4 px, outlined by the chrome, since Fusion's was one
invisible pixel), and dragging the bar still moves and re-docks it -
PROVIDED the bar ignores the mouse events it does not use. It does, and must
go on doing so; a double click is the one it takes, as the Float button.

**One owner.** One `DockChrome` per main window holds the tray and the
bookkeeping, and the panels (`MainWindow`) and the views (`ViewWorkspace`)
share it: a panel and a view minimised into two trays would be two ways of
doing one thing. It knows nothing of views or panels beyond the `DockRole` it
is told (`Panel`: Close hides it and the menus bring it back; `View`: it can
be maximised, and one view is active).

**Minimise is a tray, and why.** A floating dock is a `Qt::Tool` window and
must stay one: Qt relays a Tool window's shortcuts to its parent, so Esc,
Delete, Ctrl+Z and F9 keep working in a view floated onto another screen,
where a `Qt::Window` would lose every one (and Qt resets the flags on every
float anyway). A Tool window has no taskbar button, so the window manager's
minimise would lose it, and a docked panel is not a window at all. So
Minimise HIDES the dock and puts a button for it on `MinimisedToolBar` at the
bottom of the main window, and the button puts it back exactly where it was:
in its area and tab when docked, at its geometry on its screen when floating.
A hidden dock keeps its place in the layout tree, so "where" comes free; its
SIZE does not - the neighbours grow into the space and Qt shares the
shortfall equally when it comes back - so the sizes of the docks around it
are recorded and put back with it. Setting only the returning dock's size is
not enough: when the places of one row add up to more than the row has, Qt
takes the same number of pixels off each, so `DockChrome::applySizes` sets
every dock in both directions.

**Rejected: rolling a dock up to its title bar.** Measured offscreen in the
1360 x 860 window: Properties alone in the right area, rolled up, took the
whole column with it - 270 x 770 became 131 x 26, and the column's space went
to the drawing sideways; Layers rolled up above Reference Data narrowed the
left column from 300 to 135 px, and it stayed narrow after unrolling. Qt sizes
a dock area from its docks' hints, and a dock with its contents hidden hints
at the width of its bar. Only a floating dock behaved. Minimise does the job
for docked and floating docks alike and moves nothing else.

**Maximise (views only).** A docked view hides every other docked view of its
window until it is restored, which gives back the sizes they had; a floating
view fills the available geometry of its screen, one per screen. A maximise
is a temporary state, not a layout: the workspace undoes every docked
maximise before it opens or arranges views (a split made while the others
were hidden would be made against the wrong neighbours), a tray restore of a
docked view ends a maximise in its window, and everything is unmaximised
before a layout is saved.

**Not GroupedDragging**, in either window. With it, dragging a tabbed view
drags its whole tab group out into a `QDockWidgetGroupWindow` - Qt's own
window, with a native frame and none of the chrome - and a view in it can no
longer be floated or docked on its own.

**The side columns own the bottom corners.** `setCorner` gives both bottom
corners to the left and right areas, so the panels run the full height of
the window and the command line sits under the drawing only, between them -
the usual CAD arrangement. Qt's default gives both corners to the bottom area,
which ran the command line under both columns and cut them short.

**The active view is always one on screen.** Zoom Extents and the view
menus act on the active view (and `mostRecent(kind)` prefers it), and its
title bar carries the accent, so an active view nobody can see is a menu
acting on nothing visible. Pressing a
view's title bar or any of its buttons makes it active. Minimising or closing
the active view hands on to a view that is ON SCREEN - floating, or a docked
view inside the workspace's rectangle - not merely one that is not hidden,
because a tab page behind the current one is not hidden: Qt moves it off the
window. The one that takes over is the first on screen in the order the views
were opened; the most recently used would be better, but `cad::ViewSet`
keeps its recency order private (a public `ViewSet::recent()` is the fix).
`openView` also activates the new view when the active one is hidden.

Two Qt 6.11.2 behaviours were measured on the way, and the code relies on
them: a tab group is laid out again AS SOON AS a page is hidden - the new
current page is already inside the window when the minimise hook runs, which
is why the on-screen test, not a `raise()`, is what picks the right view (the
`raise()` stays for a window never laid out, where no geometry means
anything); and `resizeDocks` leaves a hidden dock out of the total it gives
the enclosing row, so a view must be shown before it is sized - a stacked
split of a view already side by side came out 397 and 197 px of 600 until
`openView` showed the new dock first (297 each after).

Tested in `tests/qt_widgets/test_view_chrome.cpp` (`qt_widgets.ViewChrome.*`),
each test shown failing without its fix. Not yet tested outside a
hand-run harness, because `MainWindow` cannot be built into the widget test
target: the four panels' bars, their exact minimise and restore, F9 from a
floating panel, floating maximise across two screens. Not done: a Window
menu; saving and restoring the layout (`minimisedNames` and `minimiseNamed`
are the hooks, and `unmaximiseAll` must run before `saveState`); a floating
active view left minimised during an arrange stays hidden and active, since
only the tray button (`DockChrome::restore`) raises `onRestored`. The Survey
Point Manager dock (`SurveyPointsDock`) wears the chrome now that
`SurveyServices::chrome` hands it over, and the workbench has the chrome
forget the dock before deleting it; its minimise, tray and float have been
seen only in a screenshot, with no automated test.

## The layer manager, and why "move" is "rename"

The dock beside the drawing is the quick view; Format > Layers... (Ctrl+L,
first on the Format menu and toolbar, where CAD programs keep their layer
table) is the whole table, with the fields a dock has no room for and the
operations that need it: move a layer under another parent, and put the
selection on a layer.

`LayerManagerDialog` follows the managers' rules ("The rules a dialog or
panel follows") and can be shown beside the drawing: it hears the Document
through a `DocumentWatcher`, so an undo or a typed command reloads it once,
from the event loop, keeping the selected layer and the form's unsaved edits
when that layer did not change underneath them; New, New Child and Rename or
Move ask for a name in a prompt row inside the dialog; a colour is typed as
`#RRGGBB` (`layerColourText`) or picked in a colour dialog opened with
`open()`, never `exec()`'d; and once its Document is gone it reads and changes
nothing, every control but Close disabled the first time a person reaches for
one. Its constructor is unchanged, and `selectedLayer()` and `selectLayer()`
are public for a caller that opens it on a layer. Since 2026-09-24 the
window keeps ONE instance, made the first time Format > Layers is chosen and
shown, raised and reused after that (`formatLayersDialog`), as the Format
workbench keeps the other managers; it is non-modal, so a headless run opens
it like any manager (`--dialog formatLayers`), and `--layer-manager` still
grabs it on its own. Selecting another row drops unsaved form edits without
asking, as the style manager does.

Moving needed no new command. A layer name is a path ("design/surface/tin1"
- see `layer_path.hpp` for why the tree is derived from the names rather
than stored), so moving a layer under another parent is exactly renaming it,
and `renameLayer` already carries the subtree and the entities on it as one
undo step. A "move" command would have been a second spelling of the same
thing.

Make Current is deliberately NOT a command: the current layer is session
state like the selection, and putting it on the undo stack would make Ctrl+Z
undo where the next line will be drawn - which is not what anyone means by
undo.

## The attribute manager: the attribute tree, and what "varies" protects

A string read from a `.12da` archive carries a tree of attributes; the importer flattens it to
properties keyed "Asset/Dimensions/Size", and per-vertex attributes to
"vertex/3/Name". The flat keys are honest - the model has one property map
per entity and no nesting - but unreadable on a survey string with thirty of
them, so the manager rebuilds the branches for
display and writes back the flat key. Nothing in the model changes: the tree
is a view of the names, exactly as the layer panel is a view of "/"-separated
layer paths (see `layer_path.hpp` for why that tree is derived and not
stored).

The decision worth keeping is `<varies>`. The dialog acts on the whole
selection, and shows a value only where every selected entity agrees on it;
a property only some of them carry counts as a disagreement too. The
alternative - showing the first entity's value - looks tidier and is a trap:
pressing Save would then write that value over the others without anyone
asking for it. Showing `<varies>` means a user who saves has said what they
want all of them to be.

Values are formatted by `entity::toString`, which is in `entity` and not in
the dialog, because the properties panel and the `PROP` command line print
the same values and must not disagree about them. A real is written with
enough digits to read back as the same double: a level shown as 31.2 that is
really 31.249 is a lie in survey work.

## The Format menu: the customisation workbench and its three managers

The owner's requests were to "enhance the linestyle and symbol and survey
codes managers" and to "make it professional CAD software". Format is where
CAD programs keep layers, linetypes and text styles, and it is where the three
managers and what goes with them now live:

```
Format
  Layers...                          formatLayers        (Ctrl+L)
  Styles and Linetypes...            formatStyles        -> styleManagerDialog
  Symbol Library...                  formatSymbols       -> symbolLibraryDialog
  Survey Code Manager...             formatSurveyCodes   -> surveyCodeManagerDialog
  ---
  Load Customisation...              loadCustomisation
  Replace Loaded Customisation...    replaceCustomisation
  ---
  Global Modify...                   formatGlobalModify  -> globalModifyDialog
  Purge Unused...                    formatPurge
```

The Format toolbar carries Layers, the three managers and Global Modify. Survey > Survey
Coding shows the same code manager, Load and Replace actions - the same
`QAction` objects (`SurveyServices::codeManager`), so two menus cannot drift
apart.

**`CustomisationWorkbench`** (`src/katana_qt/customisation/customisation_workbench.*`)
is built like the Survey workbench: `MainWindow::buildFormatActions` makes the
menu and the toolbar and hands them over with `CustomisationServices` - the
Document, the view workspace, the window's action factory and log, whether
the session is headless (asked each time, since the window learns it after it
is built), and the window's own Layers, Load and Replace actions. So the
workbench never includes `main_window.hpp`, and a widget test builds it and
drives it (`tests/qt_widgets/customisation/test_customisation_workbench.cpp`).
It owns what the managers share: the picture cache (`DefinitionThumbnails`),
the session's linework control codes, and the one `CustomisationContext`
each manager is built from.

- **Non-modal, one of each, kept.** A manager is made the first time it is
  asked for and then hidden, not deleted, between uses (`QPointer` slots);
  asking again shows and raises the same one. That is what lets the code
  manager's unapplied edits survive closing it. Each opens on its first entry
  - the style manager's first rows, the symbol library's first symbol, the
  code manager's rule 0 - rather than on an empty pane.
- **Deleted before the Document.** The dialogs hold the Document and paint
  from the workbench's cache, and a window destroys its members BEFORE its
  child widgets, so `~CustomisationWorkbench` deletes them itself - and the
  window destroys the workbench before the Document.
- **"Show me what uses it"** (`CustomisationContext::selectAndShow`) selects
  the entities and frames them in the ACTIVE plan view only; the other views
  keep their zoom.
- **Found by the headless driver.** Each manager's action carries its
  dialog's object name as its data, which is how `--dialog formatStyles`
  finds the dialog it opened.
- **Purge Unused** (`purgeUnused`) deletes every style, linetype and hatch
  pattern nothing uses (`cad::planPurge`, `cad::purgeCommand`) as ONE undo
  step, keeping the current style, and names them in the log. An interactive
  session is asked first (the `confirm` hook, else a question box); a
  headless one is not. `qt_purge_unused_deletes_what_nothing_uses_as_one_undo_step_headless`
  purges, undoes and checks the style is back.
- **Closing the window asks the code manager first** ("Failure modes",
  above: `confirmClose`).

Loading and replacing a customisation are `docs/survey_coding.md` ("Loading a
customisation"): a load merges, Replace is asked for.

### Styles and Linetypes

One dialog for the `Style` and `Linetype` tables and the session's library
linestyles (`src/katana_qt/style_manager.cpp`), because
they are one subject: a style names a linetype. An archive import brings 211
styles from one file - but NOT their linetypes: each style's linetype is a
LIBRARY name, which lives in the session's style library, not in the
model's Linetype table. So almost every real style names something the
drawing's own linetype table does not contain, and the dialog lists both.

What it shows, all from `include/katana/cad/style_manager_rows.hpp` so the
rows, filters and bulk edit are tested below Qt:

| Tab | Shows | Does |
|---|---|---|
| Styles | `cad::styleRows`: each style, how many entities wear it, and whether its linetype or symbol is missing; the chips All / Used / Unused / Missing and a search | a form (linetype and symbol through `NamePicker`, weight, colour or ByLayer, hatch, symbol size, description) with Save and Revert; New, Duplicate, Rename, Merge Into, Delete, Purge; Apply to Selection, Select Users, Make Current |
| Linetypes | `cad::linetypeRows`: the drawing's linetypes and the library's linestyles, by group, a name both hold marked (D2) | a pattern grid that edits a drawing linetype's dashes, gaps and dots in place; New, Duplicate, Rename, Merge Into, Delete, Purge; New Style Using This; Select Users |
| Diagnostics | `cad::styleDiagnostics`: `cad::missingNames`, then the D2 collisions, each with who uses it and what is drawn meanwhile | Select Users |

A `StylePreview` beside each form draws the style, or the linestyle, at a
plot scale on paper or on screen through the shared painter (`docs/cad.md`,
"What a style draws"): lines at their PRINTED size, so a `paperstyle` looks the same
at every scale and a `worldstyle` shrinks as the scale's N grows; a symbol
fitted to the pane on its insertion point, with a scale bar in ground metres.
The dialog's Undo and Redo buttons are the drawing's own stack. A name is
asked for in a prompt row inside the dialog and a purge is checked in a panel,
never in a box, so a headless session drives every action by object name.

**QT-02's rule** (audit QT-02, fixed 2026-09-24). Save used to rewrite the
linetype and symbol of every archive-imported style: the form's boxes were
non-editable combos filled from the model's linetypes and the sixteen
built-in shapes, `setCurrentText` with a library name was a silent no-op on them,
and Save wrote back whatever they still showed. Now:

- a form writes back only the fields a person EDITED (`cad::StyleFields`,
  `applyEdit`) - even for one style, because a spin box cannot show every
  stored value exactly (a weight of 0.1234, a symbol size of 0.03125), so
  writing back what it shows would change a style nobody edited;
- an unedited Save is no command at all (`cad::editStylesCommand`,
  `commands::updateStyleIfChanged`), so Save is enabled whenever a style is
  selected and Revert only when there are edits;
- every name field keeps a name it cannot list - `NamePicker` for linetypes
  and symbols, `kept_name_combo.hpp` for the hatch and dimension-style
  combos;
- selecting several styles shows `<varies>` for the fields they differ in,
  and Save leaves those fields of each style alone unless they were edited.

The Layers dialog got the same fix; commands have no `updateLayerIfChanged`,
so its unchanged Save compares the `Layer`.

Two decisions from the first version still hold:

- **The table is read-only; the form edits.** Editing in the table would run
  a command from inside the table's own `itemChanged` signal - the shape of
  the crash recorded under "Panels refresh on the event loop" below. There is
  no `itemChanged` handler here at all, so the bug cannot be written. The
  dialog's own commands do not reload it either: they record what to select
  and the watcher's deferred reload does it, so there is one reload path.
- **A rename is a move, not an edit of a name field.** A `NamedTable` is
  keyed by name, so `renameStyle` removes, re-adds under the new name and
  repoints every holder (entities for a style; layers and styles for a
  linetype) in one command, and therefore one undo step. It then asks the
  DELETE guard whether anything still names the old item: the guard and the
  repoint are two readings of the same set of references, and a holder the
  repoint missed would leave an entity naming a style that no longer exists.
  Making them check each other costs one line and removes the class of bug
  that a second implementation of "who uses this" invites. A protected item
  ("continuous") is protected from a rename as much as from a delete -
  everything that resolves to it by name would silently change what it draws.
  Linetypes can be renamed too: a rename onto a library name is how a drawing
  linetype ends a D2 collision, since a model linetype cannot be MERGED into
  a library linestyle (`commands` cannot see the library).

Not done: the Linetypes tab's preview draws the STORED linetype, and only the
pattern strip shows unsaved grid edits; the Styles table has a Preview
picture only for a symbol or a library linestyle, not a drawing's dash
linetype; selecting another row drops unsaved form edits without asking (a
reload caused elsewhere keeps them); the Diagnostics summary still calls
every missing name one "that nothing defines", although each row's "drawn
as" says when it is "not a linestyle"; and in the dark theme the paper
preview is a large white pane while nothing is selected, and the Colour row
shows a disabled ByLayer button with a stray drop-down arrow.

### Symbol Library

A block palette, a cell selector and a civil package's symbol chooser in one
window (`src/katana_qt/customisation/symbol_library.*`):

- **left**, a tree of groups: All, Built-in, the library's `/` groups,
  "(ungrouped)", and "Not defined" when a name resolves to nothing;
- **centre**, a grid of 64-pixel pictures from the shared cache, captioned
  with the name and badged with how many entities draw it, under a filter
  bar: a search over name, group and survey code, and the chips All / In
  drawing / Used by codes / Missing / Vertex mode;
- **right**, a `StylePreview` at a plot scale (the insertion point marked,
  and a warning when it lies outside what the symbol draws), the details,
  and the actions.

What is listed is `cad::symbolLibrary`: D3's symbols (a definition is a
symbol when it is `mode vertex`, a survey code draws it as one, a style names
it as one, or its file is a symbol file), the built-in shapes, and every
symbol name a style OR A SURVEY CODE gives that nothing defines, in the
pickers' amber with the shape it is drawn as instead. The codes' names are the
library's own addition: `cad::missingNames` looks only at styles and layers.

**How big it prints.** The details say what a point wearing the symbol
prints: "2 x 2 mm at 1:500, 1 x 1 m on the ground". `cad::symbolPrintSize`
(`include/katana/cad/symbol_assign.hpp`) measures what `cad::symbolDrawing`
draws - its strokes AND the space each of its texts covers, since a symbol
that is a letter, or carries one above its mark, prints the letter too - at
the style's size (a width in model units; 0 for the definition's own), at
1:N. A text's size comes from a `TextExtent`: the dialog measures it with the
preview's own font (`styleTextExtent` in `style_painter`), so the number
agrees with the picture beside it, and without one cad estimates it
(`estimatedTextExtent`: 0.6 of the height per character, the height tall,
descenders not counted) - so the two can differ a little, 2.91 mm with Arial
against 3 mm estimated for the fixture's TEST Valve. A built-in shape, or the
stand-in for a missing name, has no size of its own at size 0: its print size
is the viewport's plain mark, and the pane says so rather than giving a
number.

The actions, each from a button and never from the grid's own selection
signal; each that changes the drawing is ONE undo step through
`Document::execute`:

| Button | What it does |
|---|---|
| Assign to Selected Points | `cad::assignSymbolToPoints`: the points among the selection move into a style that draws the symbol at the size given - one found that draws exactly that, else one made - and the log counts the points moved, already in it, not points, and not found |
| Set on Style | the chosen style's symbol (an inline style picker; `updateStyleIfChanged`) |
| Select Points Using | the points wearing a style that names it, selected and framed |
| Replace in Styles | `cad::replaceSymbolInStyles`: every style naming the current symbol names the one in the inline `NamePicker`; refused while that is empty |
| Load .4d... | `archive12d::readCustomisation`, MERGED into the session's library (D1: never a Replace from here), each file's added and replaced definitions in the log |
| Export Selected to .4d... | `archive12d::writeStyleLibrary` of the selected library definitions |

A headless session opens no file dialog: Load and Export are
`loadLibraryFile` and `exportSelectedTo`, which take a path, and the buttons
say in the log what to call. Not done: Assign makes a style with linetype
ByLayer and weight 0.25, and under D8 a line later put in that style draws
the symbol at its vertices; the grid's pictures are on the dark screen ground
only, though the preview switches; and the dialog reloads whole (two
`tableUsage` passes) on every command the watcher reports, not measured on a
250,000-entity drawing.

### Survey Code Manager

The survey code library a surveyor codes against - what civil packages call
description keys or feature definitions - in five tabs, each
over one cad foundation, so the dialog decides nothing the CLI would say
differently: Code Table (`cad::codeTable` and `cad::explainCode`, with the
rule form), Codes in Drawing (`cad::codeCensus`), Issues
(`cad::lintSurveyMap`), Apply Codes (`cad::applySurveyCodes`, previewed before
it runs) and Linework (`cad::processLinework`, and the session's control
codes). Its edits go to a BUFFER and reach the drawing only on Apply; Revert
takes the drawing's map back; closing with unapplied edits asks. That, and
what each tab shows, is `docs/survey_coding.md` ("The Survey Code Manager").

## Panels refresh on the event loop, never inside their own signal

Switching a layer off in the layer panel crashed the application. The chain
was: the box's `itemChanged` signal -> `execute(updateLayer)` -> the
document's change listener -> `refreshAll()` -> `refreshLayers()` ->
`QTreeWidget::clear()`, all synchronous, all while the item whose `setData`
raised the signal was still on Qt's stack. `clear()` deleted it, and
`QTreeWidgetItem::setData` read its parent pointer on the way out. The
property table had the same shape of bug behind an edited value.

The listener now calls `scheduleRefresh()`, which queues one `refreshAll()`
on the event loop however many times it is asked before that runs. It also
coalesces: a transaction of a hundred commands used to rebuild every panel a
hundred times. **Rejected:** guarding each slot with a re-entrancy flag,
because that leaves the next slot to make the same mistake; the rule is that
a document change never rebuilds a widget synchronously, and one function
holds it.

`katana --toggle-layer NAME --screenshot out.png` flips the box through the
real widget, headlessly, and refuses to continue if the tree was rebuilt
during the signal (it compares the item pointers before and after, without
dereferencing the old ones) or if the document and the panel disagree
afterwards. `qt_toggle_layer_headless` runs it on the sample; with the
listener made synchronous again it fails with "the layer panel was rebuilt
inside its own itemChanged signal", which is how the test earned its place.

A related rule, from the same afternoon: **a headless session never opens a
modal box** - every question the window would ask becomes a line in the log
or a refusal. `docs/headless.md` ("A headless session never opens a modal
box") lists what each one does.

## One executor: the command runner

A dialog that changes the drawing does not do the work itself: it builds the
verb line a person would type and hands it to the window's one executor,
`MainWindow::runVerbLine`, as a `CommandRunner` (`src/katana_qt/command_runner.hpp`).
The workbenches carry it to their dialogs - `SurveyServices::run`,
`CustomisationServices::run` into every manager's `CustomisationContext::run`,
`AnnotationWorkbench::setCommandRunner`, `UtilityServices::run` - so a dialog
never reaches into the window for it.

- **The same dispatcher as a typed line.** `runCommandLine` is Enter on the
  command line: it reads and clears the field, echoes the line, offers it to
  the workbenches' verbs (`runWorkbenchLine`: `ONLINE`, `UTILITY`), then to a
  running tool (`ViewWorkspace::typeIntoTool`), and hands whatever is left to
  `dispatchLine` - the view verbs, the window's own (`SCRIPT`, `CUSTOMISE`,
  `IMPORT`, `EXPORT`, `INFO <file>`, `REFS`, `COPC`, `PLOTSHEETS`), a tool's
  alias, and the interpreter. `runVerbLine` echoes the line and runs the same
  `runWorkbenchLine` and `dispatchLine`, so the two cannot come to differ; the
  line is kept in the interpreter's history and undone exactly as a typed one.
- **Never a running tool's answer.** That step is the only one `runVerbLine`
  leaves out. A tool waiting for a point or a text takes any typed line for
  one; a dialog's `CIRCLE 5,5 2` while Line waits for its first point is drawn,
  and the tool still waits. What is being typed on the command line is left
  alone.
- **The reply comes back.** While the line runs, `logMessage` also collects
  what it logs, and `warnUser` what it shows in a box, into a
  `VerbOutcome`: `ok`, the `reply` lines and the `error` lines. `ok` is judged
  by the errors counted, as `runCommand` judges a `--command` line, so a
  `PLOTSHEETS` that plotted with a missing image is still ok. An empty line is
  not ok: a dialog has no Enter to press.

The Subsurface Utilities dialog was the first to run its line this way; before
the runner it had an echo-and-run callback of its own in the window, a second
executor, which is gone. GIS > Convert Point Cloud to COPC is the second: its two file
dialogs choose the files, and the conversion is the `COPC "<source>"
"<destination>"` line through `runVerbLine`. The headless driver's `--run-line`
step (`docs/headless.md`) runs a line the same way and prints the outcome;
`qt_a_dialogs_line_is_run_never_given_to_a_running_tool_headless` runs one while
Line waits for its first point.

### The session's verbs on the window's command line

Until 2026-09-26 the window refused or misread several verbs `katana_cli` had.
Each now reaches the same code on every front end
(`qt_the_session_verbs_run_on_the_windows_command_line_headless`,
`qt_copc_typed_on_the_windows_command_line_converts_the_cloud_headless`,
`qt_import_local_typed_on_the_windows_command_line_moves_the_data_headless`):

- `INFO 12` (or `INFO #12`) describes entity 12, unless a file of that name
  exists: `CommandInterpreter::isEntityId`. The window, and the session under
  `katana_cli` and `katana_mcp`, once took every `INFO` for `INFO <file>`, so
  `katana_describe_entity` answered that the file did not exist.
- `CODE`, `CODE EXPLAIN`, `CODE CENSUS`, `MAPFILE LIST` and `MAPFILE CHECK`
  are the interpreter's (`include/katana/cad/survey_code_verbs.hpp`), given the
  standard colour table by `CommandInterpreter::setColourLookup`.
- `COPC <source> <destination.copc.laz>`, each path one word or quoted; it
  logs the `IMPORT` line that reads the result. In a headless session the GIS
  menu's item opens no file dialog and names this verb instead.
- `CUSTOMISE` alone reports what is loaded, from which files, what the project
  was drawn with that is not loaded, and what it covers in this drawing
  (`cad::customisationReport`, the words `katana_cli` prints); it once
  answered with its usage.
- `IMPORT <file> LOCAL` moves a DXF, vector file or .12da archive as one piece
  so its lower-left corner sits at 0,0, and asks nothing; a raster or a point
  cloud refuses it by name. The path and the `LOCAL` are read by
  `CommandInterpreter::importArgument`, as the session reads them; `LOCAL` was
  once taken for part of the path. The import dialogs do not offer it yet.

## Run Script: a katana_cli script in the window

A script is what `katana_cli` runs: a `.kcs` file of commands, one a line,
UTF-8, a Windows line end taken off, blank lines and lines whose first
non-blank is `#` skipped. The window runs the same files three ways, all
through one function, `MainWindow::runScript`
(`src/katana_qt/script_runner.hpp`):

- **File > Run Script...** (`fileRunScript`) opens a non-modal dialog
  (`fileRunScriptDialog`) that shows the file's commands with their line
  numbers and the exact line it will run, and runs nothing itself: Run hands
  `SCRIPT "<file>" [CONTINUE]` to the one executor. File > Recent Scripts
  (`fileRecentScripts`, items `recentScript1` to `recentScript8`) runs one
  again. The list is the person's, kept in the settings; a headless session
  neither adds to it nor changes it.
- **`SCRIPT <file> [CONTINUE]`** typed or sent by a dialog. The window's own
  verb, beside `PLOTSHEETS`: `katana_cli` runs a script given on its command
  line and `katana_mcp` has `katana_run_script`, so an interpreter verb would
  add nothing on those two.
- **`--script FILE`** in a headless run (`docs/headless.md`).

Each line is run by `runVerbLine`, as a dialog's line is: echoed, kept in the
history, its own undo step as in `katana_cli`, and never a running tool's
answer - a script names its points (`LINE 0,0 10,0`, not `LINE` and then its
points). The run stops at the first line refused unless `CONTINUE` (the
dialog's `scriptContinueOnError`) is given, and ends with a record:
`script="<file>" lines=N ran=N failed=N`, with `stopped_at=K` (the file's line
number), `quit_at=K` or `cancelled_at=K` when it ended early, and an error
naming the line that stopped it. `QUIT` or `EXIT` in a script ends the script,
as it ends `katana_cli`'s, and never closes the window under the person who
ran it. A script that runs itself, directly or through another, is refused at
that line (`qt_a_script_may_not_run_itself_headless`). A long run shows
`scriptProgress` with Cancel, which stops between two lines; a headless run
shows nothing.

The line a dialog runs gets back everything its script's lines logged: a
line run inside another's `runVerbLine` adds what it logged to the outer
line's capture as well as its own.

**Comments and pasted lines.** A typed line starting with `#` is a note: it
is echoed and runs nothing, as in a script - unless a tool is waiting for
typed text, whose answer ("#3 pit") it may be. Several lines pasted on the
command line (Ctrl+V with line breaks in the clipboard, or a context-menu
paste and then Enter) run at once as a script that stops at the first
refused, `script=pasted` in its record; whatever was typed before the paste
starts the first line. The single-line field used to show the breaks as
blanks and run the whole as one line of nonsense.

Tested in `tests/qt_widgets/test_script_runner.cpp` (reading, the stopping
rules, the record, the dialog) and through the real window by
`qt_script_switch_runs_every_line_of_a_script_headless`,
`qt_script_switch_stops_at_the_first_refused_line_headless`,
`qt_typed_script_continue_runs_every_line_headless`,
`qt_run_script_dialog_runs_the_line_it_shows_headless`,
`qt_several_lines_on_the_command_line_run_as_a_script_headless` and the two
batch runs, `qt_script_batch_run_exits_when_the_script_is_done_headless` and
`qt_script_batch_run_fails_at_a_refused_line_headless`.

## GIS > Online Data: a workbench of its own

The online import (`docs/gis_online.md`) is `OnlineDataWorkbench`
(`src/katana_qt/gis_online.hpp`), built as the Survey and Format workbenches
are: `MainWindow::buildGisActions` hands it the GIS menu and a few callbacks
(the document, the views, the log, whether the session is headless, and one
that adds a raster to the reference data), and `MainWindow::runCommandLine`
hands it any line that starts with `ONLINE`. Nothing else of it is in
`main_window.cpp`. Its dialog (`src/katana_qt/gis_online_dialog.hpp`) is
non-modal, has an object name on every control, and imports only by building
the `ONLINE IMPORT` command the verb would parse and handing it back to the
workbench - one executor, so no path exists in the dialog that an agent cannot
take on the command line. The download runs as a background job with its
progress and Cancel in the status bar; the entities arrive as one command.

## Survey > Subsurface Utilities (AS 5488): the same pattern, one verb

The AS 5488 tools (`docs/subsurface_utilities.md`) are `UtilityWorkbench`
(`src/katana_qt/survey/utility_workbench.hpp`), built as the Online Data
workbench is: `MainWindow::buildSurveyActions` makes it after the Survey
workbench and hands it the Survey menu, which it ends with the section
"Subsurface Utilities (AS 5488)" - Draw Utility Schedule, Utility
Investigation Report, Verify Detections Against Exposures, Clearance of
Proposed Works, Check Against a Delivery Schema (`utilityDraw`,
`utilityReport`, `utilityVerify`, `utilityClearance`, `utilityCheck`).
`MainWindow::runCommandLine` hands it any line that starts with `UTILITY`,
before a running tool can take the line for an answer - unless that tool is
waiting for typed text (`ViewWorkspace::toolTakesText`: a Text's string, a
count), whose answer the whole line is, as it is for a bare `ZOOM`: a label on
a services plan may well read "Utility pit". `ONLINE` gives way to such a
tool the same way (`qt_utility_line_leaves_a_text_to_the_tool_headless`).

The verb is the `CommandInterpreter`'s, shared with `katana_cli`; the
workbench runs it through the window's interpreter and adds the one thing
only a window has: after a `UTILITY DRAW` that worked, every plan view is
framed on the reply's `bounds=` box (`utilities::drawReplyBounds`,
`ViewWorkspace::zoomTo`). A schedule in a real coordinate system lands far
from whatever the view was showing, and without the framing a draw that
worked looked like one that did nothing - the lesson Online Data learnt. The
box is read by the verb's own function, not a second parser in the window,
so the record's format has one reader to keep in step with its writer
(`utilities::formatUtilityDrawing`); the two branches that built this each
had one, and the window's was dropped when they were merged.

All five items open ONE non-modal dialog (`UtilityToolsDialog`,
`src/katana_qt/survey/utility_dialog.hpp`, object name `utilityDialog`), each
on its own tab; the header lists every control's object name. The detected
spacing and the minimum cover sit under the schedule, shared by Draw and
Report and live on those tabs only, because the verb reads `SPACING` and
`MINCOVER` for both: a drawing and a report of one schedule are graded and
flagged alike. The dialog never calls the AS 5488 library. `utilityCommandLine` - a pure function of
the fields, tested without a window - writes the `UTILITY` line (a path with
blanks quoted, a blank option left out, a file field left empty or a number
that does not read refused with the field named, and nothing run; a file that
cannot be read is the verb's refusal, by its path, once the line has run); `utilityCommand`
shows that line as it is edited; Run hands it to the window's one executor
(`UtilityServices::run`, "One executor: the command runner"), so it is echoed, kept in the history and
undone exactly as a typed line - but never offered to a running tool first,
since the dialog's line is never a text - and the reply the workbench got for it comes
back into `utilityOutput`, with Copy and Save As beside it. The reply stays
while another tab is brought forward, and Save As offers the name of the tool
it came from, `utility_draw.txt`, not of the tab in front
(`UtilityToolsDialog::suggestedFileName`). A headless session
opens no file dialog: Browse and Save As say so, and a script fills the path
fields instead. Tested in `tests/qt_widgets/survey/test_utility_dialog.cpp`
and, through the real window, by `qt_utility_dialog_writes_the_line_headless`,
`qt_utility_dialog_headless` (Run on Draw) and
`qt_utility_draw_typed_frames_the_views_headless` (the same draw typed, as an
agent types it, framed the same).

## Global Modify

Format > Global Modify... (`GlobalModifyDialog`,
`src/katana_qt/customisation/global_modify_dialog.*`) states the request
`cad::planGlobalModify` takes (`docs/cad.md`, "Global Modify") and decides
nothing of its own. Non-modal, one instance kept by the Format workbench
(`CustomisationWorkbench::showGlobalModify`), as the managers are:

- **Apply to**: the selection, a view, the checked layers (with their
  sublayers or not), or the whole drawing. The views are the workspace's
  open ones, read afresh each time through `GlobalModifyDialog::views`, so a
  closed view is never pointed at and a plan view's "only what is on screen"
  is its visible area as it is now; the dialog itself never sees the
  workspace, which is what lets a widget test give it views of its own.
- **Only those that match**: types, layer and style patterns, colour,
  property and value, text, drawn only.
- **Modify**: three tabs - Entities, Their Layers, Their Styles. A field
  changes only when its box is ticked, and its editor is disabled until then,
  so an unticked field can never be written by accident (the rule "`<varies>`
  protects" states for the attribute manager).
- **The summary** under the form is the plan's own, refreshed as the form is
  edited (coalesced to one preview per 150 ms) and as the drawing or the
  selection changes. Apply is one undo step and logs the summary; Select
  Matches selects what the scope and filter take and frames it in the active
  plan view. Text that does not read - a colour that is not `#RRGGBB` or
  ByLayer - is said in the summary, in red, and nothing runs.

Tested in `tests/qt_widgets/customisation/test_global_modify_dialog.cpp`.

## The rules a dialog or panel follows

Collected here because each was paid for once, and a new manager is where
they are easiest to break:

- **No moc.** No `Q_OBJECT`, no custom signals: connections are lambdas and
  state is passed through `std::function` members (`DockTitleBar::onPressed`,
  `onMinimised`). A `QAbstractItemModel` or `QSortFilterProxyModel` subclass
  needs no `Q_OBJECT` and is fine.
- **Tables are read-only and a form edits**; a command is never run from a
  list's or table's own change signal ("Styles and Linetypes").
- **A view reacting to the Document defers and coalesces** its reload to the
  event loop, never rebuilding a table inside its own signal ("Panels refresh
  on the event loop"). A manager does it through a `DocumentWatcher`
  (`customisation/document_watcher.*`), declared as its LAST member so it
  goes first: one delivery per turn of the event loop however many
  notifications came, saying what moved - the model (`Document::modelRevision`),
  the library or the map (their generations), the selection, the current
  layer or style.
- **A manager is non-modal and kept** by the workbench that opened it (the
  Format workbench, "The Format menu" above; the Survey workbench's dialogs),
  so it stays open beside the drawing and a buffer of unapplied edits
  survives hiding it.
- **A headless session never opens a modal box**, and every action, menu,
  field, button and tab has an object name, so tests and the headless driver
  can find it.
- **A dialog holding `Document&` must be deletable before the Document**, and
  a registration is owned by a `ListenerHandle` (`docs/cad.md`, "A listener
  lives exactly as long as the thing it notifies").
- **Logic that can be tested below Qt lives in `katana_cad`** -
  `src/katana_cad/customisation/*.cpp` and `include/katana/cad/*.hpp` are
  globbed, with tests in `tests/cad/customisation/` - and the dialog stays
  thin. What must be tested WITH Qt goes in `katana_qt_widget_tests`
  (`tests/qt_widgets/`, run offscreen as `qt_widgets.*`), which compiles
  `src/katana_qt/customisation/*.cpp` and `src/katana_qt/tools/*.cpp` and
  globs its own `customisation/` and `tools/` tests; `widget_harness.hpp`
  drives widgets by object name and asserts on the Document.
