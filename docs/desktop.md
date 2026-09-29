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
status bar shows a view's running readout, how many entities are selected of
how many, the current layer, the active snap and the cursor coordinates.

### How the window starts

`main` (`src/katana_qt/main.cpp`) makes the `QApplication`, then applies the
theme before any widget exists (`theme::apply`: the style, the palette, the
stylesheet, and the platform's font taken as the one font of the whole
window - below, "Text"), sets the application icon, reads the switches, and
builds the `MainWindow`: its actions and menus, its panels, View's Window
section (`MainWindow::buildWindowMenu`) and the status bar, and records the
layout it was built with for View > Reset Window Layout. The default
customisation is loaded next, so the first drawing is drawn with it. Then an
interactive run - one with no `--screenshot`, `--plot`, `--plot-sheets`,
`--sheets-json` or batch `--script` - puts the window where the last session
left it (`MainWindow::restoreSession`) and shows it, and the project and files
on the command line are opened.

**Where the window was.** Until 2026-09-26 every session opened a 1360 x 860
window, whatever the screen: on a 1366 x 768 laptop it ran off the bottom,
and on a large screen it was a small window in a corner, with the panels and
toolbars wherever Qt put them and nothing a person moved kept. Now an
interactive session keeps, in the application's `QSettings`, the window's
place and size (`saveGeometry`), where the panels and toolbars are
(`saveState`, keyed on the object names every dock and toolbar has), which
panels are minimised to the tray (`DockChrome::minimisedNames`), the text
size, the toolbar icon size and whether the toolbars show their names; they
are written when the window closes and read before it is shown. With nothing
saved - the first run - or a layout from another version (`kLayoutVersion`,
raised when a change to the toolbars or panels would make an old layout put
them wrong), the window fits the screen it is on (`MainWindow::fitToScreen`):
85% of the free area, centred, or maximised on a screen smaller than
1440 x 900, where the drawing needs every pixel. View > Reset Window Layout
puts the panels and toolbars back as a new window has them and leaves the
size and the appearance alone. The views inside the workspace are not kept:
they belong to the drawing being worked on, not to the window. A headless
run neither reads nor writes any of it (`docs/headless.md`), so a screenshot
is the same on every machine.

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
the last tool again; Esc abandons a box and then clears the selection; a
right-click, or the keyboard's menu key, opens the shortcut menu; and a double
click on an entity edits it (both below, "The plan view's shortcut menu").
While a tool runs, Enter, Space and a right-click are the tool's Enter, a
double click is two clicks for it, and Esc ends the tool first. What a tool
does with each is `docs/tools.md`, "The tool host".

## The plan view's shortcut menu

A right-click in a plan view with no tool running opens the shortcut menu
(`PlanContextMenu`, `src/katana_qt/plan_context_menu.hpp`, object name
`planContextMenu`); so does the keyboard's menu key, at the cursor. With
nothing selected, the entity under the cursor is selected first, so the menu
acts on what was clicked; a selection already there is kept, since the click
may land beside what the menu is for (`ViewportWidget::selectForMenu`).

It holds no command of its own. Its items are the window's existing actions,
found by object name, so the menu item, the menu bar's, the toolbar button
and a headless `--trigger` are ONE action and cannot drift: Erase
(`editErase`); Move, Copy, Rotate, Scale, Mirror and Offset (the tools
`modify.move` ... `modify.offset`, which start with the selection as their
objects); Attributes (`editAttributes`); List (`inquiry.list`); Select Similar,
Quick Select and Global Modify (`select.similar`, `select.quick`,
`formatGlobalModify`); Edit Text and Edit Label (`annotateEditText`,
`annotateEditLabel`) for a text or a label in the selection; and Deselect
(`editDeselect`). An action the window does not have is left out, not faked,
as in a test's window of a few actions; the desktop window has them all. With nothing
selected it offers Repeat and the last tool's name (`planContextRepeat`, the
tool's own action), Select All, Select by ID, Quick Select and Zoom Extents.

What it adds is the verb lines a person would otherwise type, run through
the window's one executor ("One executor", below), so the log shows each
line and each is one undo step: Put on Layer (`planContextLayer`) runs
`CHLAYER <layer>`; Style (`planContextStyle`) runs `STYLE APPLY <name>`, or
`STYLE APPLY -` for ByLayer; Colour (`planContextColour`) runs `COLOR BYLAYER`
or, after the colour dialog, `COLOR #RRGGBB`; Entity Information
(`planContextInfo`, for one entity) runs `INFO #<id>`, whose answer is in the
log - `#`, because `INFO 12` is a file when the working directory holds one
called 12, and the item once failed there. An item's status tip is its line,
so a person learns the verb. Put on Layer is the layer tree, not a list: a
layer with layers under it is a submenu headed by the layer itself
(`planContextLayerTree.<path>`), because a drawing brought in from a survey
has hundreds of layers. The layer, style or ByLayer the whole selection
shares is ticked. A name is written by `namedLine`, through `commandWord`
("One rule for a word on the line", below): bare when it can be, quoted when
it is empty or holds a blank (`CHLAYER 0`, `CHLAYER "Site Boundary"`), and a
name holding a double quote or a line break is offered disabled.

`popup`, not `exec`: nothing waits on the menu, and it deletes itself when it
closes. It is not built in a headless run, which has no mouse; everything on
it is a menu action or a verb, so an agent reaches the same through
`--trigger` and `--command`. `tests/qt_widgets/test_plan_context_menu.cpp`
checks what it offers with and without a selection, that Put on Layer is one
undo step, the layer tree, the ticks, and that the view raises it from a view
made before the window set the hook (the workspace's first plan view is).

**Double click to edit.** A double click on an entity with no tool running
(`ViewportWidget::onEntityDoubleClicked`, after its first click selected it)
makes it the whole selection and opens what edits it: Edit Text for a text
and Edit Label for a label where the window has those actions, and the
Properties panel for anything else (`MainWindow::editDoubleClicked`). On empty
space it does nothing; while a tool runs it is two clicks for the tool.

**Select by ID** (Edit > Select by ID..., `editSelectById`;
`src/katana_qt/select_by_id_dialog.hpp`) turns the ids that `LIST`, `INFO`,
`AREA` and a refusal print back into a selection. It builds `SELECT id id
...` from `selectByIdIds` (commas or blanks between them, `#12` as well as
12), with the current selection first when `selectByIdAdd` is ticked, and
runs it through the executor; `SELECT` takes `#12` too, as `INFO` does. A
missing id, or one on a hidden or locked layer, is `SELECT`'s own refusal,
shown in `selectByIdStatus`, and the selection is left as it was. Then, with
`selectByIdZoom` (on by default), the active plan view frames the selection,
and the Properties panel is brought forward to show it. It is non-modal and
kept, like Format > Layers, so several lookups need no reopening, and
`--dialog editSelectById` makes it a headless run's target
(`qt_select_by_id_selects_and_frames_headless`).

**Every Edit and View action has an object name**, so `--trigger` and
`--report` reach it as a person's click does: `editUndo`, `editRedo`,
`editSelectAll`, `editDeselect`, `editSelectById`, `editErase`,
`editAttributes`, `viewZoomExtents`, `viewGrid` and `viewSnap`; the snap
modes `viewSnapModeEndpoint` to `viewSnapModeGrid` (by their menu names:
Endpoint, Midpoint, Center, Intersection, Perpendicular, Tangent, Nearest,
Grid); the viewport layouts `viewLayoutSingle`, `viewLayoutSplitVertical`,
`viewLayoutSplitHorizontal`, `viewLayoutThreeLeft`, `viewLayoutThreeTop` and
`viewLayoutQuad`; `viewShowsPlan` and the other Active Viewport Shows items;
the standard 3D views `viewStandardTop`, `Bottom`, `Front`, `Back`, `Left`,
`Right`, `IsoSouthWest`, `IsoSouthEast`, `IsoNorthEast` and `IsoNorthWest`;
`viewTogglePerspective`; `viewVerticalExaggeration`; and the Panels toggles
`viewPanelLayers`, `viewPanelProperties`, `viewPanelCommandLine` and
`viewPanelReferenceData`. Until 2026-09-26 Undo, Redo, Select All, Erase
Selection, Zoom Extents, Grid and Object Snap had none, and a headless run
could not reach them; the rest of the View menu had none after that either,
while this paragraph said it had.

The values a click cannot carry are typed: `SNAP <mode> [ON|OFF]` sets one
snap mode, as its menu item does, and `EXAGGERATION [factor]` sets the
vertical exaggeration (0.01 to 1000), or alone says it as a record,
`vertical_exaggeration=2`. View > Vertical Exaggeration asks in a box and runs
that line through the one executor; in a headless run it asks nothing and
says to type the line, since `--trigger` now reaches it and the box would
wait for ever.
`GRID` and `SNAP` take `ON`, `OFF` or nothing (a toggle) and refuse any other
word - every other word once meant `OFF`, so `SNAP ENDPOINT OFF` turned
object snap off altogether - and say what they did (`grid=on`, `snap=off`,
`snap_mode=endpoint state=off`)
(`qt_every_view_menu_item_is_named_and_its_values_are_typed_headless`).

Two rendering details are worth noting. Arcs and circles are tessellated in
*model* space with a chord count chosen for a sub-quarter-pixel sagitta, so a
very large radius with only a sliver on screen never hands Qt coordinates in the
millions. Text below three pixels tall is drawn as a baseline stroke rather than
glyphs, so a zoomed-out drawing stays legible instead of dissolving into
unreadable marks.

The Layers panel's narrow columns - shown, locked, colour - are headed by
icons with their words in the tooltips, and a layer's colour is a swatch
with its code in the tooltip: the words "On", "Lock" and "Colour" and a
"#FFD54F" in every row took so much of a 300 px panel that the layer names
were cut to three letters.

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

**Text.** The fonts are tokens too (`theme::uiFont`, `theme::monospaceFont`,
`theme::overlayFont`). The command line named "Consolas", the plan view's
prompt and snap tag "Segoe UI" at fixed sizes, and eighteen dialogs'
reports and command fields `QFontDatabase::systemFont(FixedFont)` - Courier
New on Windows, wider and lighter than the Consolas beside it - while on
Windows the menus, the menu bar, tooltips and message boxes each kept the
font the platform gave that class. Now the whole window is one font, the
platform's UI face (Segoe UI on Windows): `theme::setTextSize` sets it
without a class name, which drops the per-class fonts. The fixed-pitch font
is the first the machine has of Cascadia Mono, Consolas, DejaVu Sans Mono,
Liberation Mono and Menlo, then the platform's own, at the same size as the
chrome, so a column of coordinates in a report lines up and is the size of
the text around it. **View > Text Size** (`viewTextSizeSmall` to
`viewTextSizeExtraLarge`) moves every font together: Small is the platform's
size less a point, Large two points more and Extra Large four - one point
(9 to 10) is a difference few people see. The command line's font, set on
the widget, is set again with the rest. The drawing's own text face
(`PlanPaintOptions::fontFamily`) is content, like the viewport's colours, and
is not a theme font: a plot must print the same whatever size the window's
text is.

**`KatanaStyle` (in `theme.cpp`) - Fusion, with two things it does not do.**
The Survey, Terrain and GIS menus were grouped under titled sections
("Surfaces", "Point Cloud - PDAL") and not one title had ever been seen:
Fusion says it does not support sections (`SH_Menu_SupportsSections`), so
`QMenu` drew each as a plain separator - and the stylesheet drew a menu's
separators with its own parent's code whenever the menu had a background
colour, which draws no text either. The style says it does and draws the
title in small muted capitals, and the `QMenu` rule declares
`-qt-style-features: background-color`, which hands the separators back to
the style while the stylesheet keeps drawing the items
(`qt_widgets.Theme.AMenuSectionShowsItsTitleUnderTheStylesheet`, shown
failing without that line). Every long menu is now in titled sections: File
(Drawing, Import and Export, Scripts, Plot, Project), View (Display,
Viewports, 3D Views, Window), Format (Tables and Libraries, Customisation
Files, Across the Drawing, Annotation Styles), Annotate's editors and Leader
Manager, and the tool menus by their catalogue groups (Lines, Curves,
Transform, Edit, Dimensions ...; `src/katana_qt/tools/tool_menus.hpp`). Second, for a
choice list that cannot be typed into, Fusion drops a popup the height of
every item (`SH_ComboBox_Popup`), ignoring `maxVisibleItems`: a drawing's two
hundred styles ran off the screen. The style asks for the scrolling list
(`qt_widgets.Theme.ALongChoiceListScrollsRatherThanRunningOffTheScreen`).

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

**Every menu item has an icon and says what it does.** Until 2026-09-26
sixty menu items had no icon or no status tip, or neither: the snap modes,
the viewport layouts, the standard 3D views, Deselect, Quit, the annotation
editors and managers, every submenu. `MainWindow::menuGaps` walks the menu
bar and every submenu and lists each item without both; `katana
--check-menus` fails a run that has one, and
`qt_every_menu_item_has_an_icon_and_a_status_tip_headless` runs it. The new
icons keep the one language: a snap mode is the mark the plan view draws for
it, on the geometry it snaps to; a layout is its panes with the main view in
the accent; an orthographic standard view is the object in plan with the face
seen in the accent and the eye's arrow coming at it, and an isometric one the
cube with the arrow from its corner. `qt_widgets.Icons.TheListHoldsEveryIconOnceAndEachPaintsSomething`
holds `allIcons` to every enumerator, in order, each painting at 16 px.

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

**A toolbar says its name.** Two rows of icon-only buttons read as one
undivided strip, and which icons were Survey's and which Terrain's was a
matter of hovering over each. A toolbar along the top or the bottom now
starts with its name, muted (`QLabel#toolBarName`, made by
`MainWindow::makeToolBar`); a vertical one has no room for a word, and the
Properties bar's " Style " label already says what it holds. View >
Toolbars (`viewToolBars`) shows or hides each toolbar (`viewToolBarFile` ...),
turns the names off (`viewToolBarNames`) and sizes the icons: Small 16 px,
Standard 20, Large 28 (`viewToolBarIconsSmall` ...). Words under every icon were
rejected: the two top rows hold some sixty buttons, and with their words they
would not fit in the widest window.

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
aliases are in their tooltips. Help > Keyboard Shortcuts lists the keys for a
person ("The Command Reference and the keyboard shortcuts", below).

`katana --screenshot` grabs the window headlessly so that the look can be
reviewed, and the same run can drive the menus and dialogs step by step:
`docs/headless.md`.

**Not done.** There is no light theme. The dialogs (corridor, plot) are
themed but plain. The views inside the workspace are not kept between
sessions (above, "How the window starts"); the dock chrome's hooks
(`minimisedNames`, `minimiseNamed`) are ready for it. The Text Size is the
window's; a dialog that set a font of its own other than the fixed-pitch one
keeps it. `resources/icon_sheet.png` is regenerated by `katana_make_icons` and
carries every icon, the menu items' of 2026-09-26 included.

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

**Zoom Extents frames the active view only** - and the views linked with it
follow (below, "Linked views"); one view's zoom is not another's business
unless the user has linked them, and the old container did one thing while
its header said another. After New, Open and an import every view is framed
(`zoomExtentsAll`), because all of them are looking at a drawing that has
just changed under them; the link is framed once there, as below.

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
and isolating a tree node hides everything but that node's path. The same
filter has lines - `VIEWS HIDE <id> <layers>`, `VIEWS SHOW <id> <layers>|ALL`
and `VIEWS ISOLATE <id> <layer>` (`docs/cad.md`, "The window's views: VIEWS
and ZOOM") - so an agent, a script and a test set up a design view hiding the
as-built layers beside an as-built view hiding the design ones
(`qt_a_design_view_and_an_as_built_view_hide_each_others_layers_headless`).
Not done: the popup's rows still edit the view's state directly rather than
run those lines, and a view's hidden reference layers have no line, since
cad cannot name a reference layer.

### Linked views: a design view and an as-built view that move together

The owner's request of 2026-09-30: "sync views together so i can zoom in a
design view and as-built view zoom in too", with the control "as toolbar on
the view widget itself". A plan view is linked or not; every linked view
shows the centre and the scale (pixels per unit) of the linked view moved
last, each at its own size and through its own hidden layers - the design
view hiding the as-built layers and the as-built view the design ones, both
looking at the same place. The model is `cad::ViewSet`'s link and
`include/katana/cad/view_link.hpp`; the verbs are `VIEWS LINK`, `VIEWS
UNLINK` and every `ZOOM` (`docs/cad.md`, "The window's views: VIEWS and
ZOOM").

**What moves the link.** A wheel notch, a middle-drag pan, a frame (Zoom
Extents from the bar, the menu or a line, the program's own framing, a
`ZOOM`) and a first paint's frame: each raises the plan view's
`ViewportWidget::onViewMoved`, and the workspace's `viewMoved` has the others
take its centre and scale in the same pass, with one repaint each. The
follow is STAR-SHAPED: a view moved by the link never reports that move -
`ViewportWidget::holdView` raises nothing - and the workspace's guard flag
makes sure of it, so no echo can run between two views. **A linked view
never refits on a resize**: a view framed by Zoom Extents refits its frame
whenever it is resized until the user moves it, which in a linked view would
change its scale behind the link's back, so every member forgets its frame
when the link moves it or it moves the link. Resizing a dock never breaks the
link (`ViewLinks.ALinkedViewKeepsTheLinksViewWhenResized`, shown failing with
`holdView` emptied).

**Who comes to whom when views are linked.** `VIEWS LINK ids TO id` brings
the others to TO; without TO the joining views come to the link (its lowest
id), or with no link yet to the first id listed. The Link button always
sends TO: the view the user panned or zoomed most recently of the one clicked
and the link's members (`ViewSet::linkLeaderFor`, on a clock of the user's
moves, `ViewState::lastMoved`), else a member, else the view clicked.
Clicking the two views' buttons in either order then keeps the zoom just
made (`ViewSet.AJoiningViewComesToTheViewTheUserMovedLast`). Rejected:
"the joining view always follows", which loses that zoom whenever the view
zoomed is clicked second; and one click linking to an implicit "previous"
view, which surprises with three views open. The first click makes a link of
one, which waits for the second ("Linked, waiting"); an unlink, a close or a
change of kind that leaves one member dissolves the link, and none of them
moves a view.

**Zoom Extents on every view** (`zoomExtentsAll`, after New, Open and an
import) and `ViewWorkspace::zoomTo` frame the link ONCE, in the member moved
last (else the lowest id), on the union of what the members draw; the others
follow. Framed one by one, each on what it alone draws, a design view and an
as-built view would come apart the moment a drawing opened
(`ViewLinks.ZoomExtentsAllFramesTheLinkOnceOnTheUnionOfWhatItsViewsDraw`).

**The bar.** A plan view's bar is `[kind] Plan 1 ... [Link][Layers][Zoom
Extents] [_][float][max][x]`. `ViewLinkButton` is checkable and never a menu
(a title bar's menu is a modal loop, which a headless press waits in for
ever): an open chain in the neutral colour while the view moves alone, a
closed chain in the accent inside an accent frame while it is linked, and a
tooltip that names the views it moves with. A click builds `VIEWS LINK <id>
TO <id>` or `VIEWS UNLINK <id>` and runs it through the window's command
runner, so it is logged as the line it is; afterwards every Link button is
set from the link with its signals blocked, so a refused line leaves the
button as it was (`ViewLinks.ARefusedLinkLeavesTheButtonAsItWas`). The
button shows on plan views only and follows the view's kind; each tool
always on the bar costs 23 px of the dock's least width (22 px and the bar's
1 px spacing), which the Link button's test holds to
(`ViewLinks.ThePlanDocksMinimumWidthGrowsByTheLinkButtonAlone`). A checked
title-bar button had no frame at all: the rule for every chrome button came
after `QToolButton:checked` with the same weight and took it away, so
`theme.cpp` now has a `:checked` rule of its own for them. View > Link This
View (`viewLinkActive`, Ctrl+Shift+L; K, as Viewport Layout has L) does the
Link button's line for the active plan view, and View > Unlink All Views
(`viewUnlinkAll`) runs `VIEWS UNLINK ALL`.

**The zoom tools on the bar.** A plan view's bar is `[kind] Plan 1 ...
[Link][Layers][Zoom In][Zoom Out][Zoom to Selection][Zoom Extents]
[_][float][max][x]`; a section's has In, Out and Extents, a 3D or elevation
view's Extents alone (below, "Not done"). Each is a plain button that runs
its ZOOM line through the command runner - `ZOOM IN view=<id>`, `ZOOM OUT
view=<id>`, `ZOOM SELECTION view=<id>`, `ZOOM EXTENTS view=<id>` - so a
click is logged, the views linked with it follow, and Zoom to Selection with
nothing selected says `matched=0` and moves nothing. A button the view's
kind would refuse is not offered: the bar shows what ZOOM takes there, and
follows the view's kind. The View menu has the same four for the active
view (`viewZoomIn`, `viewZoomOut`, `viewZoomSelection`, and Zoom Extents,
which now runs `ZOOM EXTENTS` too) and Zoom To (below).

In, Out and Zoom to Selection are OPTIONAL tools (`DockTitleBar::addTool`
with a priority): a bar shows them, highest priority first, only while they
fit and leave the title `DockTitleBar::kTitleRoom` (56 px, "Plan 1" and a
little over); tools of one priority come and go together. Zoom to Selection
(2) outlasts In and Out (1), which the wheel does as well. They are never in
the bar's least width (`DockTitleBar::minimumSizeHint` leaves them out: Qt
asks a child's hint rather than taking its layout's minimum), so a view can
always be narrowed past them - each tool always shown costs 23 px of the
least width, and three more would have stopped a Quad view's bars from
narrowing to their docks. No overflow menu: every optional tool is in the
View menu too, and a menu opened from a title bar is a modal loop a headless
press would wait in (`ViewChrome.TheTitleBarShowsOptionalToolsOnlyWhereTheyFitAndNoTwoOverlap`,
`ViewChrome.OptionalToolsNeverWidenTheDocksMinimum`).

**Zoom To** (View > Zoom To, `zoomToDialog`) frames what a scope takes - the
shared "Apply to" and "Only those that match" (`ScopeFilterWidget`, prefix
`zoomTo`) - in a chosen view (`zoomToView`: the active view, then every open
view by its title and id). It builds `ZOOM <scope words> [view=<id>]`
(`zoomToLine` shows it as the controls change), runs it through the command
runner (`zoomToRun`) and shows ZOOM's answer (`zoomToStatus`). Non-modal and
kept, as Select by ID is (`qt_zoom_to_dialog_runs_its_line_headless`).

**ZOOM typed while a tool runs** - Z, ZOOM, 'ZOOM, 'Z and whatever follows -
is the view's, not the tool's, as AutoCAD resumes LINE after 'ZOOM: the tool
host hands it to the workspace (`ToolHost::onTransparent`), which runs it as
its ZOOM line through the command runner, and the tool stays at its step
with its prompt (`ViewLinks.ATransparentZoomTypedInsideLineZoomsAndLeavesLineAtItsStep`).
The command line leaves the line's echo to the runner
(`ViewWorkspace::runsTransparently`), so it is logged once
(`qt_dist_typed_at_the_command_line_reports_the_inverse_headless`). PAN and
'P have no verb, so the host goes on refusing them by name. A workspace
built without a window runs the bars' lines through an interpreter of its
own, answered by itself: the same verbs, unlogged.

Rejected: a Zoom to Selection button greyed out while nothing is selected -
it needs a document listener on the workspace, and ZOOM already answers an
empty selection with `matched=0`; a second toolbar row or tools drawn over
the drawing (above).

**Rejected alternatives.** Linking by the visible extent: views of different
shapes fit one box at different scales, and each move would round each
view's scale differently, so they drift over a session. Qt signals from
widget to widget, and document notifications: a notification repaints every
plan view's whole kept drawing, and signals between widgets would be a
second way of saying who follows whom, outside the tested model. A toolbar
drawn over the drawing, or a second row under the title bar: the first
covers geometry and catches picks, the second costs 26 px of height in
every view, while the title bar had room. Link groups (several links, each
with a colour): one link is what design against as-built needs, and the
model changes cheaply when groups are asked for.

**Not done.** Only plan views link: a 3D or elevation view needs its camera
in logical pixels first (the software view counts device pixels, the GPU
view logical ones), a section its pan and zoom in `ViewState`. Grips are
still shown on a selected entity that a view hides (`gripsOfSelection` takes
no view layers). The link is not kept between sessions, as the workspace is
not. A 3D or elevation view's bar has no Zoom In, Out or Selection yet, and
ZOOM IN on one is refused naming its kind: its zoom is to go towards what is
drawn at the centre, through the 3D wheel's own zoom towards the cursor
(built on another branch; once merged, the workspace's `zoomView` hands IN
and OUT to it at the centre pixel, and the verb's rule of which kinds take
which zoom lets them through), and framing what a scope takes in 3D needs
the scene to draw a set of ids.

### The selection in every view

The owner's request of 2026-09-30: "when selecting a feature in 2d it gets
highlighted in other views". Plan views already drew the selection in every
plan view that shows its layer; the rule is now the same for every kind of
view:

- **Selected and drawn in the view: highlighted.** A plan view keeps the 2 px
  dashed #FF9F1C it always drew. A 3D or elevation view draws a selected line
  as a 3 px core of the same orange over a 5 px dark casing (16, 18, 22),
  which shows a pixel either side on the light face of a hill and the dark
  sky alike, and a selected point 9 px over 11 (an unselected one is 5). A
  section draws the selected entity's crossing solid, 2 px, in the orange,
  with a dot at its level, where the other crossings are 1 px red dashes.
- **Selected, on a layer the drawing shows and the view hides: a ghost** -
  drawn faint, and never picked, snapped to or plotted there
  (`ViewLayerRule.AGhostIsNeitherPickedNorSnappedTo`). A feature selected in
  the design view shows where it is in the as-built view that hides the
  design layers. In plan it is DOTS of the orange at 60 % (alpha 153), 2 px
  square and 6 px apart, where a selection is dashed, with no fill, hatch,
  linestyle or symbol; a point is a ring of dots; a text, note, leader or
  dimension the outline of the box it draws; a label is not ghosted
  (`docs/plan_view.md`, "Ghosts of the selection", for how and what it
  costs). In 3D and elevation
  it is one 1 px line of (130, 88, 32) - the orange at 45 % mixed over the
  view's ground beforehand, since the software rasteriser draws no alpha -
  with no casing. In a section it is the crossing, 1 px, dotted, at 60 %.
- **A layer the drawing hides stays hidden everywhere.** A ghost says
  "selected, but hidden in this view", never "selected, though switched
  off".

One colour and every size are in `include/katana/cad/selection_style.hpp`,
each with its reason. The 3D selection was (255, 190, 60), beside the
BOUNDARY layer's (255, 213, 79) and the top of the elevation ramp, and apart
from the plan's; the sheet editor's blue stays, since it selects sheet
items, not entities.

**Each view has its own switch**, on when the view opens
(`ViewState::selectionGhosts`): "Show the selection on hidden layers"
(`ViewLayersShowSelection`) in the view's Layers popup, beside the layer
filter it belongs with, and `VIEWS SET <id> ghosts=on|off`, which the box
runs through the command runner, so a click is logged as its line
(`ViewSelection.TheLayersPopupsBoxRunsViewsSetThroughTheRunner`). VIEWS
reports it in every record, `ghosts=on|off` (`docs/cad.md`, "The window's
views"). Not a bar button: every tool always on the bar costs 23 px of every
view's least width.

**What it costs.** The plan painter draws ghosts in a pass after the
entities, over the selection's ids alone - nothing when the selection is
empty, and never a test in the entity loop every entity passes - into the
view's kept drawing, whose key already held the selection and the view's
hidden layers and now holds the switch. The 3D overlay walks the selection's
ids the same way (it used to walk the whole drawing and keep the selected),
and a section repaints on a document change without cutting again
(`SectionViewWidget::setDocument`).

**The casing in 3D is a draw list of its own** (`SceneLayers::selectionCasing`),
drawn just before the core's and writing no depth, so the core covers it
wherever they overlap. In one list with the core the software view was
right, by the draw lists' depth bias, but the GPU view - which pulls a mark
nearer by its width and reads no bias - drew the wider casing over the core,
and a selected line was a dark line (the control in
`GpuLayers.ASelectionCoreDrawnAfterACasingThatWritesNoDepthKeepsItsColour`).
A frame's depth range takes in the selection's box too, because a ghost lies
where the view draws nothing (`SceneFrame.TheDepthRangeTakesInAGhostOutsideWhatTheViewDraws`);
what a view frames is unchanged.

Rejected: ghosting layers the drawing hides (a switched-off layer would
linger in every view); a halo in plan (the dashed orange already reads
apart from every layer colour); the ghost switch on the view's bar (the width
above); the casing by draw order in one list (the GPU's pull decides, not
the order).

Not done: grips are still drawn, and can be dragged, on a ghost - as on any
selected entity on a layer its view hides, because `gripsOfSelection` takes
no view layers; it is to take the view's `LayerOverrides` with no default,
as `isDrawn` does. A selected line buried under the ground in 3D is hidden
by it, as any line is; an x-ray pass over everything is the next step.

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

## The Properties panel: a tree read a level at a time

The panel was a two-column table, rebuilt whole after every change: Id, Type,
Layer, Colour, what the geometry measures, then every property by its flat
name. A string brought in from a survey archive carries its attributes
flattened into '/' names ("The attribute manager", below), so thirty
attributes on each of a thousand vertices was thirty thousand rows, all made
before the first could be seen, and a line of a thousand heights in one cell.
The owner's request of 2026-09-26: "i want the attributes in the properties
to show as a tree structure, because some lines are very long and contain
lots of attributes in the vertices, so need a smart way to load all these
information".

`PropertyTreePanel` (`src/katana_qt/property_panel.hpp`) is a filter box
above a tree (`propertyFilter`, and `propertyTable`, the name the table had,
so `--report` and DRIVE's `?` read it as before). One entity is its groups:

| Group | Holds | Opened |
|---|---|---|
| General | Id, Type, Layer, Colour (and Visible when hidden) | yes |
| Geometry | what the shape measures, as the table showed it | yes |
| Vertices | each vertex, "Vertex 3 \| x, y, z", its own attributes (`vertex/3/...`) beneath it | no |
| Attributes | the properties as the tree their '/' names make | yes |
| Source | the import's record of the entity (`Entity::metadata`), kept to be written back | no |

**Nothing is read until it is opened.** A group's level is read by
`cad::propertyOutline` - the reading `PROP TREE` replies with, so the panel and
an agent cannot disagree (`docs/cad.md`, "Properties as a tree: PROP TREE") -
when a person opens it, and made a page of 200 rows at a time
(`kPropertyPage`); a "Show 200 more" row at the end of a level makes the next
page when clicked or chosen with Enter. The model (`PropertyTreeModel`)
draws an expander for a level it has not read, so a vertex with thirty
attributes costs one row until it is opened. A vertex is numbered from 1, as
the archive numbers its attributes; its height is shown only where it has one
(absent is not zero), and the `elevations` list in Attributes says "see
Vertices" instead of a thousand numbers. Attributes of a
vertex the string no longer has (one since removed) keep the `vertex` branch
in Attributes, so nothing held is out of sight.

**The filter lists every value that holds the words typed**, however deep -
a vertex's attributes and the source record too - by whole name
(`vertex/3/QL | B`), in natural order, after a pause in the typing; cleared,
the tree is back as it was left. **Rows stay open across a refresh**: the
window rebuilds the panel after every change, and the opened rows are
remembered by the keys of their path and opened again where they still are,
so an undo elsewhere does not fold up the vertex being read; a group a person
closed stays closed from one entity to the next. Several entities are read as
the attribute manager reads them - General counts them by type, Attributes
shows a value where all agree and `<varies>` where they do not (opened by
itself up to 500 entities, read on demand past that). Copy (Ctrl+C, or the
shortcut menu) puts the selected rows on the clipboard, name and value
separated by a tab. The panel edits nothing; the Style row above it and
Edit > Attributes do.

One trap was found on the way and is recorded in the model: `QTreeView`
calls `fetchMore` from inside its own layout of the row it is opening, and a
fetch there that removed the Show more row cleared the view's items under
the layout and wrote past their end (valgrind found it in
`qt_widgets.PropertyPanel.ALongStringIsMadeAPageAtATimeWithAShowMoreRow`). So
`fetchMore` only reads a level the first time, adding rows and removing none,
and the next pages come from the Show more row after the click is over
(`qt_widgets.PropertyPanel.OpeningALongLevelAsAPersonDoesReadsItsFirstPageOnly`,
which fails with the old rule because the view then made every page at once).
The rest is `tests/qt_widgets/test_property_panel.cpp`.

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
  ---
  Text Styles...                     formatTextStyles    -> textStyleManagerDialog
  Label Styles and Rules...          formatLabelStyles   -> labelStyleManagerDialog
  Dimension Styles...                formatDimensionStyles -> dimensionStyleManagerDialog
```

The last three are the annotation workbench's (`docs/annotation.md`, "In
the window"), and like every dialog that changes the drawing they run the
verb lines they build through the one executor below.

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
| Hatch Patterns | `cad::hatchPatternRows`: each hatch pattern, solid or how many families, and who uses it (`entity::tableUsage`) | a family grid (angle in degrees, spacing in model units) or Solid, with a swatch drawn by `cad::hatchSegments`, Save and Revert; New, New Solid, Duplicate, Delete, Purge; New Style Using This; Select Users |
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

**The Hatch Patterns tab** (`HatchPatternsTab`,
`src/katana_qt/customisation/hatch_patterns_tab.hpp`) is where the window got
the `HATCH` verb, which it had only offered by name in pick lists. Unlike the
two tabs before it, it runs nothing itself: every button writes the line -
`HATCH NEW`, `SOLID`, `SET`, `DELETE`, `PURGE HATCHES`, `STYLE NEW name HATCH
pattern` - and hands it to the window's executor (`CustomisationContext::run`),
so it is echoed and one undo step, and an agent types the same. The family
grid is a buffer saved by one `HATCH SET`; its swatch draws the grid as it
stands, with the offsets Save keeps, through the hatcher the plan view uses.
Angles show to twelve significant digits (`cad::hatchAngleDegrees`), the
degrees that were typed: radians cannot hold 30 degrees exactly, and the
exact reading, 29.999999999999996, would be written back by every Save.
`none` - what an unhatched layer resolves to - is neither edited nor deleted.
The window's title and the Format menu item keep their names ("Styles and
Linetypes"); the menu item's tip names the hatch patterns. Renaming them
would have changed a menu item, this heading and a test other work reads,
for a word the tab already says.

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
  the workbenches' verbs (`runWorkbenchLine`: the geoprocessing executor's -
  `GDAL`, `IMPORT`, `EXPORT`, `INFO <file>`, `REFS`, `COPC` - then `ONLINE`,
  `UTILITY`), then to a running tool (`ViewWorkspace::typeIntoTool`), and
  hands whatever is left to `dispatchLine` - the view verbs, the window's own
  (`SCRIPT`, `CUSTOMISE`, `PLOTSHEETS`, `PLOT`, `SNAPSHOT`), a tool's alias,
  and the interpreter. `runVerbLine` echoes the line and runs the same
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

The Sheets editor is handed the runner when File > Sheets first opens it
(`SheetEditor::setCommandRunner`): Generate Sheets, Choose Paper and the view
fields run their `GENERATE`, `SHEET SUGGESTPAPER` and `VIEW SET` lines
through it (`docs/plotting.md`, "The sheet editor"), and the Sheet Set menu
its `SHEETS SAVE`, `LOAD`, `APPEND`, `JSON` and `PAGESETUP` lines ("The Sheet
Set menu and Page Setup"). The window's command line
and the editor's lines get the same sheet-verb context,
`sheetVerbContextFor`, which replaced the window's own copy of it.

**One rule for a word on the line.** A name a dialog writes into its line is
written by `cad::annotation::commandWord`
(`include/katana/cad/annotation/command_words.hpp`): bare when it can be,
double-quoted when it is empty or holds a blank. A name with a double quote or
a line break in it cannot be carried, because the tokenizer has no escape, so it
is refused. Otherwise the line would run on the name cut short at the quote.
One verb reads past the tokenizer: `CRS SET` takes the rest of its line
verbatim, because a WKT is all quotes (`docs/cad.md`, "The project's
coordinate system"), so File > Project Coordinate System writes its text onto
the line as typed.
An annotation text uses `annotationTextWord`, which also writes a line break
as `\n`. `src/katana_qt/command_word.hpp` hands a QString to the same rules
(`commandWord`, `annotationTextWord`) and names the field in the refusal, and
writes a number exactly (`exactNumber`). The Alignment Manager, the Hatch
Patterns tab, Edit Label, the text and label style managers, the Dimension
Styles manager's numbers and the plan view's shortcut menu write their words
with it. The menu lists a layer or style whose name holds a quote (a DXF can
bring one) disabled, with the reason as its tip. The parallel lanes of the
2026-09-26 UI work first wrote four copies of this rule, and the integration
folded them into this one; Edit Label and the style managers still carried
QString wrappers of their own over it, and four files their own exact-number
helper, until these were folded in too.

Not done: these lines still quote a name or a path by hand, always, rather
than through `commandWord` - the Sheets editor (`sheet_editor.cpp` `quoted`),
the Sheet Set menu (`plotting/sheet_set_menu.cpp` `quotedPath`), File >
Import's `IMPORT "<file>"` (`import_placement.cpp`), Plot to PDF's
`PLOT "<file>"` (`plotting/plot_drawing_dialog.cpp`), Export View as Image's
`SNAPSHOT "<file>"` (`plotting/view_image_export.cpp`), Run Script's
`SCRIPT "<file>"` (`script_runner.cpp`), the GIS menu's `COPC "<source>"
"<destination>"` (`main_window.cpp`), and Page Setup's `pattern="..."`, with
its own backslash doubling (`plotting/plot_dialog.cpp`). A Windows path
cannot hold a double quote, and none of these dialogs offers one. Still, each
is a second spelling of the rule and should move onto `commandWord`; doing so
changes the lines those dialogs' tests pin.

### The session's verbs on the window's command line

Until 2026-09-26 the window refused or misread several verbs `katana_cli` had.
Each now reaches the same code on every front end
(`qt_the_session_verbs_run_on_the_windows_command_line_headless`,
`qt_copc_typed_on_the_windows_command_line_converts_the_cloud_headless`,
`qt_import_local_typed_on_the_windows_command_line_moves_the_data_headless`):

- `INFO 12` describes entity 12, unless a file of that name exists, and
  `INFO #12` describes it whatever files there are:
  `CommandInterpreter::isEntityId`. Entity Information and
  `katana_describe_entity` send `INFO #<id>`; run beside a file called `1`
  or `#1` they once failed (`cli.info_hash_id_is_the_entity_beside_a_file_of_that_name`,
  `qt_info_hash_id_is_the_entity_beside_a_file_of_that_name_headless`). The window, and the session under
  `katana_cli` and `katana_mcp`, once took every `INFO` for `INFO <file>`, so
  `katana_describe_entity` answered that the file did not exist.
- `CODE`, `CODE EXPLAIN`, `CODE CENSUS`, `MAPFILE LIST` and `MAPFILE CHECK`
  are the interpreter's (`include/katana/cad/survey_code_verbs.hpp`), given the
  standard colour table by `CommandInterpreter::setColourLookup`.
- `COPC <source> <destination.copc.laz>`, each path one word or quoted, a
  background job whose `converted` record names the file
  (`docs/interop.md`, "IMPORT, EXPORT, INFO, REFS and COPC on every front
  end"). GIS > Convert Point Cloud to COPC offers to import the result once
  the job has converted it. In a headless session the menu item opens no
  file dialog and names this verb instead.
- `CUSTOMISE` alone reports what is loaded, from which files, what the project
  was drawn with that is not loaded, and what it covers in this drawing
  (`cad::customisationReport`, the words `katana_cli` prints); it once
  answered with its usage.
- `IMPORT <file> LOCAL` moves a DXF, vector file or .12da archive as one piece
  so its lower-left corner sits at 0,0, and asks nothing; a raster or a point
  cloud refuses it by name. The path and the `LOCAL` are read by
  `CommandInterpreter::importArgument`, as the session reads them; `LOCAL` was
  once taken for part of the path. `ALONGSIDE` and `OFFSET=dE,dN` work the
  same way, and the import dialogs offer all of them (`docs/interop.md`,
  "Placing an import").

## Run Script: a katana_cli script in the window

A script is what `katana_cli` runs: a `.kcs` file of commands, one a line,
UTF-8, a Windows line end taken off, blank lines and lines whose first
non-blank is `#` skipped. The session under `katana_cli` and `katana_mcp`
once skipped only a line whose first character was `#`, and refused an
indented note that the window skipped; it reads the same rule now
(`cli.an_indented_note_in_a_script_runs_nothing`,
`McpServer.AScriptsIndentedNoteRunsNothing`). The window runs the same files three ways, all
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
points). Nor does a bare tool word in it start the tool, as a typed one does
(`MainWindow::LineSource`): `ERASE` alone is the interpreter's, erasing the
selection as `katana_cli` does, and `MOVE` alone is refused for want of its
arguments there as here. The window once started the Erase tool for it, erased
nothing, and still reported every line run and exited 0
(`qt_a_scripts_bare_tool_word_runs_as_katana_cli_runs_it_headless`). The run stops at the first line refused unless `CONTINUE` (the
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
blanks and run the whole as one line of nonsense. Pasted while a tool waits
for typed text - the lines of a Text or a Multiline Text - they are that text
instead (`MainWindow::typeLinesIntoTool`): each line typed in turn, as though
typed and entered, and a blank line left out, since to Text it is the Enter
that finishes and a paragraph break in what was copied is not. Run as a
script, a label's words were once taken for commands - `RECT 0,0 5,5` pasted
as a text's first line drew a rectangle, and its second line was refused
(`qt_lines_pasted_at_a_text_prompt_are_the_texts_lines_headless`).

Tested in `tests/qt_widgets/test_script_runner.cpp` (reading, the stopping
rules, the record, the dialog) and through the real window by
`qt_script_switch_runs_every_line_of_a_script_headless`,
`qt_script_switch_stops_at_the_first_refused_line_headless`,
`qt_typed_script_continue_runs_every_line_headless`,
`qt_run_script_dialog_runs_the_line_it_shows_headless`,
`qt_several_lines_on_the_command_line_run_as_a_script_headless` and the two
batch runs, `qt_script_batch_run_exits_when_the_script_is_done_headless` and
`qt_script_batch_run_fails_at_a_refused_line_headless`.

## The Command Reference and the keyboard shortcuts

Help > Command Reference (`helpCommandReference`, F1) used to print the
interpreter's help into the log, which left out every verb the window's front
end runs itself, and the sheet verbs' options could be read only by typing
HELP SHEETS. It is now a non-modal, searchable dialog (`commandReferenceDialog`,
`src/katana_qt/command_reference_dialog.hpp`), built from the texts the verbs'
own code keeps, so nothing is written twice:

- **Commands**: `CommandInterpreter::helpText`, the annotation verbs included;
- **Sheets**: `plotting::sheetVerbHelp`, every option (Help > Sheets and
  Plotting Commands, `helpSheetCommands`, opens the reference here);
- **Subsurface utilities**: `utilities::utilityVerbHelp`;
- **Online data**: `interop::onlineUsage`;
- **Window**: `windowHelpText`, the verbs `MainWindow::dispatchLine` and
  `runWorkbenchLine` take before the interpreter (`SCRIPT`, `IMPORT`,
  `EXPORT`, `INFO <file>`, `REFS`, `COPC` - the geoprocessing executor's,
  whose usage the Geoprocessing section gives - `CUSTOMISE`, `PLOTSHEETS`, `PLOT`,
  `SNAPSHOT`, `ZOOM`, `GRID`, `SNAP`, `EXAGGERATION`, `ONLINE`, `UTILITY`,
  `QUIT`, and `HELP`, which adds this section to the interpreter's), and the rule for a bare tool
  word - with the one word that means different things on the two command
  lines: a bare `LS` starts the List tool in the window and is `LABELSTYLE`
  in `katana_cli`;
- **the tools**, a section a menu: each tool's name, aliases, key, tip and id,
  from the tool catalogue.

`referenceEntries` reads a help text into entries by the layout every help
here keeps: an entry starts at a line that does not start with a blank, a
label in the first column (`DimStyle  DIMSTYLE LIST ...`) titles a group, and a
first paragraph ended by a blank line is the section's introduction. The
search wants every word it is given, in any case; a double-click puts the
entry's verb on the command line, to be finished there, and the reference
itself runs nothing. `windowHelpText` lives beside the reference rather than
in `main_window.cpp` so that the widget tests search the real text; a comment
on `dispatchLine` says a verb added there is added to it, and
`CommandReference.TheWindowSectionNamesEveryVerbTheWindowRunsItself` checks
the list. A typed `HELP` (or `?`) alone prints the interpreter's help and then
`windowHelpText`; `HELP SHEETS` and `HELP UTILITY` are unchanged.

The reverse fault is fixed too: the interpreter's help listed `PLOTSHEETS`,
which `katana_cli` refuses (it paints, and only the window can), so
`katana_cli -h` and `katana_mcp`'s `katana_help` advertised a verb their
reader could not run. The line now says it is the window's, and names
`--plot-sheets` for a headless run.

Help > Keyboard Shortcuts (`helpKeyboardShortcuts`, `keyboardShortcutsDialog`,
`src/katana_qt/keyboard_shortcuts_dialog.hpp`) is a searchable table of every
key the window answers to - the key, the command, the menu it is under and its
status tip (`MainWindow::shortcutRows`) - with any key that
`MainWindow::shortcutClashes` finds reaching two commands marked "(clash)" and
listed under the table. The keys are the ones `--check-shortcuts` counts, so
the two cannot disagree.

Tested in `tests/qt_widgets/test_command_reference.cpp` and, through the Help
menu, by `qt_the_help_menu_finds_a_verb_and_lists_every_key_headless`.

## File > Drawing Summary

The window showed only pieces of the drawing's state - the title, the current
layer, the current style - and nowhere the alignment count, the undo depth,
the full project path or the customisation it was drawn with; the
customisation's coverage was logged once, at a load, and scrolled away. File >
Drawing Summary (`fileDrawingSummary`, `drawingSummaryDialog`,
`src/katana_qt/customisation/drawing_summary_dialog.hpp`) keeps it all in view,
non-modal and kept, following the Document through a `DocumentWatcher`
(refreshed once a turn of the event loop however many commands ran):

- the project's full path and whether it has unsaved changes;
- entities, layers, alignments and sheets;
- the current layer, style, annotation scale and coordinate system;
- the selection;
- the undo and redo depth with the next step each way;
- the customisation, in the words a bare `CUSTOMISE` prints
  (`cad::customisationSummary`): the loaded files in load order, the files the
  project was drawn with that are not loaded, the counts and this drawing's
  coverage; and the names the drawing's styles give that no loaded library
  defines (`drawingSummaryUnresolved`). A double-click on one, or Show in Styles
  and Linetypes, opens Format > Styles and Linetypes on its Styles tab with the
  Missing chip and the name in the search, reached by the object names the
  manager's header lists (`MainWindow::showMissingInStyles`), so its styles
  are what the manager shows.

It changes nothing. Copy as JSON runs `STATUS JSON` through the one executor
and copies the reply - exactly what `katana_status` and `katana://status` give
(`docs/cad.md`, "STATUS") - and Load Customisation is the Format menu's item.
The status bar has a permanent `statusSelectionCount` label, "3 selected / 120
entities", refreshed with the panels.

Tested in `tests/qt_widgets/customisation/test_drawing_summary_dialog.cpp`
and, typed and through the menu, by
`qt_drawing_summary_and_status_json_describe_the_drawing_headless`, which
leaves a style's linestyle undefined by replacing the library that defined it
and follows the name into the style manager.

## Plot to PDF and view images

File > Plot to PDF was a modal box that never set the plot's colour mode or
line weight scale - `--plot` took both - and nothing an agent driving the
window could run plotted the drawing; nothing saved or copied a picture of a
view at all but `--screenshot`, which grabs the whole window. Both are verbs
now, and the menu items write their lines:

- **`PLOT <file.pdf> [paper=A0..A4] [landscape|portrait] [fit|scale=N]
  [dpi=N] [style=colour|grey|mono] [lineweight=F] [margin=MM]`**
  (`src/katana_qt/plotting/plot_drawing_dialog.hpp`): the drawing on one
  sheet as the active plan view shows it, through the `plotDrawingToPdf`
  that `--plot` uses; what is not given is `PlotSettings`' default. It logs
  the sentence it always did and a record, `file="..." paper= orientation=
  scale= dpi= style= lineweight=`, with the scale a fitted plot chose. File >
  Plot to PDF (`filePlot`) opens `plotDrawingDialog`, non-modal and kept:
  every field has an object name, the colour mode
  (`plotDrawingColourMode`, Colour / Greyscale / Monochrome) and the line
  weight scale (`plotDrawingLineWeightScale`, 0.10 to 5.00) among them;
  `plotDrawingCommand` shows the line and Plot runs it through the one
  executor. The file is a field, so a headless run fills it; Browse opens a
  file dialog, except headless. The file suggested follows the drawing
  (`MainWindow::suggestPlotFiles`, on a drawing opened, made new or saved):
  kept from the first showing, the dialog once plotted a drawing opened
  afterwards into the first project's folder under its name. A path typed in
  the field is left as it is. Plot asks before writing over a file that is
  there, as the save dialog before it did - the drawing's suggested PDF is the
  sheet set's too (`suggestedPlotFile`) - and a headless run writes it, as the
  verb does (`qt_the_plot_dialogs_file_follows_the_drawing_opened_headless`).
- **`SNAPSHOT <file.png|.jpg|.tif> | CLIPBOARD [width=N] [height=N]
  [scale=F] [bg=theme|white|none] [view=plan|3d]`**
  (`src/katana_qt/plotting/view_image_export.hpp`): a picture of the plan or
  3D view. The plan view is painted afresh at the size asked by the plan
  painter (`ViewportWidget::renderToImage`, read-only: the view's own kept
  drawing is untouched), at the scale that fits what the view shows, without
  the grid, the snap marker or a tool's preview. On `bg=white` it is drawn
  as a plot draws it - white pens black, line weights in millimetres - since
  the screen's white pens would vanish into the ground; `bg=none` is
  transparent, a PNG's or the clipboard's only (a JPEG has no alpha, and the
  TIFF the plot's writer makes is RGB). A 3D view is grabbed as drawn and
  scaled, on its own ground, so `bg=white` or `bg=none` with `view=3d` is
  refused rather than dropped - it once wrote an opaque image for `bg=none`
  and said nothing - and the dialog greys `viewImageBackground` out for the
  3D view. A side is 1 to 10 000 pixels (an A0 sheet at 300 dpi is 9933).
  File > Export View as Image (`fileExportViewImage`, `viewImageDialog`)
  writes the line, its file following the drawing and asking before one is
  written over, as Plot to PDF's does; Edit > Copy View as Image (`editCopyViewImage`) runs
  `SNAPSHOT CLIPBOARD`. The record is `file="..."` or `clipboard=yes`, then
  `view= width= height=`.

Both are the window's verbs, beside `PLOTSHEETS`: the painter is Qt's, which
`katana_cli` and `katana_mcp` do not have, and which refuse the lines by name,
saying where they run (`docs/mcp.md`, "What the server adds to the command
line"), rather than as unknown commands. A headless run has `--plot` with
the same settings, and an agent driving the window types the lines (the
Command Reference's Window section lists them).

Tested in `tests/qt_widgets/plotting/test_plot_drawing_dialog.cpp` and
`tests/qt_widgets/plotting/test_view_image_export.cpp` (the grammars, the
dialogs' lines, the sizes, each file format, and the plan painted into an
image), by `qt_plot_and_view_image_dialogs_run_their_verbs_headless` (both
dialogs driven by object name, Copy View as Image, a typed transparent PNG,
the 3D view grabbed; the dialog's image size read from the PNG's header), and
by `qt_plot_headless`, whose `PLOT` script prints the same sheet in each
style: the colour plot has coloured pixels and the greyscale and monochrome
ones none, the monochrome one fewer mid greys than the greyscale one, and
line weights times 4 and times 0.25 more and less ink than times 1.

## Undo and Redo lists

Edit > Undo and Redo move one step a click; `UNDO n` and `REDO n` have always
moved several, but only for someone who typed them. The Edit toolbar's Undo
and Redo are now split buttons (`editUndoButton`, `editRedoButton`): the
button is one step, as before, and the arrow drops down the history
(`editUndoMenu`, `editRedoMenu`) from `CommandStack::undoNames` and
`CommandStack::redoNames`, the next step first. The k-th entry (`undoStepK`,
`redoStepK`, so `--trigger undoStep3` reaches it) runs `UNDO k` or `REDO k`
through the one executor: one line in the log, and what it did said as the
typed line says it. Its status tip is that line and what it covers: "UNDO 3:
the last 3 steps, down to ..." and "REDO 3: the next 3 steps, down to ..." -
the Redo list's once said "the last" too, which REDO does not do. The lists show 25 steps each way, with a last line saying
how many more there are and that `UNDO n` reaches them; they are refilled with
the panels. The names are the commands' own (`CREATE_POINT`), the words the
Undo item and the Drawing Summary use. Tested in
`tests/commands/test_command_stack_names.cpp` and by
`qt_the_undo_and_redo_lists_step_through_the_history_headless`. Not done: an
Undo History dock with the save point marked is not built.

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

## Geoprocessing jobs: GDAL on the window's command line

The geoprocessing verbs (`docs/geoprocessing.md`) - GDAL, and the families
the geoprocessing packages add - are `GeoWorkbench`
(`src/katana_qt/geo/geo_workbench.hpp`), built as the online and utility
workbenches are. `MainWindow::buildGisActions` hands it the window's
document, interpreter, reference rasters and surfaces. `runWorkbenchLine`
asks it before the online and utility workbenches, so a typed line and a
dialog's line through `runVerbLine` reach the same executor `katana_cli` and
`katana_mcp` run. The window links `katana_app` for it: the window never has
a second implementation of a session verb.

- **Prepared on the GUI thread.** The line is read, its scope resolved and
  what the run needs copied there. What answers at once - LIST, HELP, a
  PREVIEW, a refusal - is logged there.
- **Run as a job.** Anything that runs is a background job (`jobs.hpp`), with
  progress and Cancel in the status bar. The job's Apply runs the executor's
  apply, as one undo step.
- **Headless, the line waits for its job,** so what it did is logged, and
  captured by `runVerbLine`, before the next line.
- **Interactively it logs `job id=<n> title="..." state=started`,** and when
  the job ends it tells `GeoServices::finished` and any listener a dialog
  added (`GeoWorkbench::addFinishedListener`). A cancelled job applies
  nothing, whenever the cancel arrived.

Its menu items come from one table with a block per package
(`src/katana_qt/geo/menu_table.cpp`, `GeoMenus`). The GIS sections and the
Terrain submenus are made only when an item is added, so no empty heading
shows. The GDAL verb's item is GIS > Processing - GDAL > GDAL Toolbox.

**The GDAL Toolbox** (`gdalToolboxDialog`, `src/katana_qt/geo/gdal_toolbox_dialog.hpp`)
is every one of GDAL's algorithms in one window: the catalogue as a searchable
tree, and for the algorithm chosen a form made at run time from the arguments
GDAL declares - its bounds on the spin boxes, its choices in the lists, the
Advanced ones folded away - a picker for each dataset it reads (the drawing's
scope and filter, a reference raster, a surface, a file), where the output
goes, and Confirm for an algorithm that changes existing data. Its Pipeline
tab chains GDAL's pipeline steps - only those that read what the pipeline
makes at that point - and shows the pipeline's text, which may be edited and
is read back into steps. It writes the GDAL line and runs it through the one
executor, as the dialogs below do.

**The geoprocessing dialogs build lines.** Terrain > DEM > Grid Points to DEM
(`gridDemDialog`), Terrain > DEM > DEM Tools (`demToolsDialog`, a tab per
tool) and the dialogs after them do no work of their own: each
writes the line its fields describe into its Command field and hands it to
`GeoServices::run`, the window's one executor. `GeoRunPanel`
(`src/katana_qt/geo/geo_dialog_support.hpp`) is the Command, Preview, Run and
Reply they share: an interactive run answers `job id=<n> ... state=started`,
so the panel shows "running" and takes the reply from the workbench's
finished listeners when the job ends; a headless run's reply is the runner's
own. A dialog is kept once made, a child of the window, so `--dialog` finds it
by the name its action carries. Their scope and filter controls are Global
Modify's (`ScopeFilterWidget`), given the window's views through
`GeoServices::views`.

**The terrain dialogs** (Terrain > Surface From, GIS > Export Surface as DEM,
and the Terrain > Analysis items) share
`src/katana_qt/geo/terrain_dialog_support.hpp`. Each writes its line into a
read-only `<d>Command` field and Run hands it to `runVerbLine`. `TerrainRun`
shows the reply in `<d>Reply`: at once when the line answered at once (a
refusal, a PREVIEW, a headless run), else when the job whose id the reply
named ends. A dialog is a child of the window, which destroys the workbench
before its children, so a dialog never calls into the workbench as it goes:
what a finished listener reaches is held weakly instead of the listener
being taken back. The three Surface From items open one dialog,
`surfaceFromDialog`, on their own source, and GIS > Export Surface as DEM
opens `surfaceRasterDialog`; both are made on first use and kept, as the
Alignment Manager is (`docs/terrain.md`, "Surfaces on every front end").

**Terrain > Analysis** holds the terrain analysis items, each opening its
dialog through `showTerrainDialog` (made on first use under the window, so
`--dialog <item>` finds it by the name the action carries as its data).
The submenu is A&nalysis (A is Cut Section Along Alignment's) and its items
take C, H, S, A, D and V, so each is reached from the keyboard; they had no
letters, and `GeoMenus.EveryItemTheLanesAddHasALetterOfItsOwn` now fails an
item the GIS or Terrain lanes add without one, as
`qt_every_shortcut_and_menu_letter_reaches_one_thing_headless` fails a
letter shared in one menu:

- `terrainContours` "Contours..." opens `contoursDialog`, which writes a
  CONTOUR line: the source (a surface or a reference raster), the interval,
  every how many is major, the level counted from, the parent layer, a
  raster's smoothing, and - with "Keep inside closed shapes of the drawing"
  (`contourClip`) - the shared scope controls (`contourScope`, the closed
  shapes the contours are kept inside; the selection at first)
  (`docs/terrain.md`, "Contours").
- `terrainShading` "Terrain Shading..." opens `terrainShadingDialog`, which
  writes a RASTER SHADE line: the source, the style, the light (for the
  hillshade styles only: the light's controls are off for the others), the
  ramp (a built-in or a colour-map file typed in) and its range, the
  reference raster's name, and a GeoTIFF to save the picture to
  (`docs/terrain.md`, "Shading").
- `terrainSlope` "Slope and Aspect..." opens `slopeAnalysisDialog`, which
  writes a RASTER SLOPE or RASTER ASPECT line: the source, the kind, the
  unit, the class breaks with the layer their areas go under and the least
  area kept (both off until there are classes), the reference raster's
  name, and - with `slopeClip` - the shared scope controls (`slopeScope`,
  the closed shapes the analysis is kept inside) (`docs/terrain.md`, "Slope
  and aspect").
- `terrainZonal` "Statistics by Area..." opens `zonalStatsDialog`, which
  writes a RASTER ZONAL line: the source, the zones (the shared scope
  controls, `zonalScope`; the selection at first), the statistics ticked,
  the property prefix, how cells count, and a CSV file with its Replace box
  (`docs/terrain.md`, "Statistics by area").
- `terrainDrape` "Drape and Sample Heights..." opens `drapeDialog`, two
  tabs: Drape writes a DRAPE line (the ground, how a raster is read between
  cells - off for a surface - and the shared scope controls, `drapeScope`),
  and Sample a RASTER SAMPLE line of the points listed, typed or picked in
  a plan view (`samplePick`, through `GeoServices::pickPoint`: the next
  left click; Esc or a right click cancels) (`docs/terrain.md`, "Sampling
  and drape").
- `terrainViewshed` "Viewshed and Line of Sight..." opens `viewshedDialog`,
  two tabs: Viewshed writes a RASTER VIEWSHED line (the ground, observers
  typed or picked - or, with `viewshedUseScope`, the points the shared
  scope controls take - the eye and target heights, how far to look, the
  curvature, a layer for the visible area and the raster's name), and Line
  of Sight a LOS line (the two ends, each typed or picked, and the heights)
  (`docs/terrain.md`, "Viewshed and line of sight").
- The three are driven by object name through the real window headless,
  filled, run and their replies read (`tests/geo/headless/analysis.cmake`;
  `docs/terrain.md`, "Viewshed and line of sight", "Tests in the window").
  The pick buttons are not: a pick waits for a click in a plan view, which
  a headless drive does not make, so the points are typed.

**One store of surfaces.** The window's surfaces are a
`terrain::SurfaceStore` (`include/katana/terrain/surface_store.hpp`), the
store the headless session has too, so `SURFACE <name>` finds the same
surface on every front end. `MainWindow::syncSceneSurfaces` rebuilds the
views' list from it whenever its revision moves, keeping how each was shown.
Surfaces are shared and immutable, so a job may read one while the views
draw it. The store keeps names unique: a second surface of a name is
"name (2)".

**The GIS menu's option dialogs are one file each** - `gis_import_dialogs`,
`gis_export_dialog`, `surface_raster_dialog`, `dataset_info_dialog`, with
their shared OK/Cancel row in `gis_dialog_support.hpp` - split from one
`gis_dialogs` file without a change in behaviour, since four geoprocessing
packages each extend one of them.

## File > Import IFC and Export IFC

IFC 4.3 both ways (`docs/ifc.md`) has two entries of its own in File,
`fileImportIfc` ("Import IFC...") and `fileExportIfc` ("Export IFC..."), beside
Import and Export Vector rather than inside them: what an IFC exchange chooses
- which of the drawing's objects go, an AS 5488 utility schedule and the
delivery schema it was written to, a project's classification rules - has no
place in the generic dialogs. Each opens a dialog (`IfcImportDialog`,
`IfcExportDialog`, `src/katana_qt/ifc_dialogs.hpp`) that the window makes on
first use and keeps: non-modal, a File field with a Browse button rather than a
file dialog asked up front, and every control named in the header, so that a
headless run drives it (`--dialog fileExportIfc`, `--fill`, `--press`) and
Browse, headless, says to type the path instead of opening a box.

**The dialogs write lines, and do nothing else** - the pattern of the
Subsurface Utilities dialog below. Every choice either offers is a word of the
verb: `EXPORT <file.ifc> ... [NOENTITIES] [SELECTED] [NOALIGNMENTS]
[NOSURFACES] [PREVIEW]` and `IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS]
[NOELEMENTS] [NOSURFACES] [TOLERANCE <m>] [TAKECRS | KEEPCRS]`, with `INFO`
and `IFC RULES` (`docs/ifc.md`, "Using it"). A dialog writes the line its
fields mean (`ifc::formatExportLine`, `ifc::formatImportLine`), shows it as it
is edited in `ifcExportCommand` and `ifcImportCommand`, and hands it to the
window's command line through its context's `runLine`
(`MainWindow::runIfcCommand`, `src/katana_qt/main_window_ifc.cpp`), which
echoes it as typed and runs it by `MainWindow::runIfcLine` - the code a typed
line runs, which reads it with the grammar `katana_cli` reads it with
(`include/katana/ifc/front_end.hpp`). So there is nothing a dialog can do that
a typed line, a script or an agent cannot, and the reply - key=value records
- is the same whoever asked. The alternative, a dialog calling the export
function itself with options no line could say, was what these dialogs first
did; it was rejected because a person could then do what an agent could not,
and a dialog's action would be neither echoed nor logged as a typed one is.

The export dialog's table, `ifcExportClasses`, previews the file: for each
layer, service, alignment and surface, how many objects become which IFC
class, in which system, and why ("rule kerb", "a graded segment: type water",
"a label: labels are not exported"). Preview runs the dialog's own line with
`PREVIEW` added, and fills the table from the reply's object records
(`ifc::readExportObjects`, the one reader of what `ifc::formatExportReply`
writes): the writer's own account (`ifc::IfcExport::tally`), made by writing
the file in memory, never a second guess at the mapping, and the same rows an
agent asking `PREVIEW` reads. A class that is wrong for a project's layers is
put right with a rules file, which Save Default Rules starts from the defaults
(`IFC RULES <file.csv>`). The table is the preview of the choices when
Preview was pressed: changing any of them but the file's name clears it, and
the dialog reads the window's counts again (`IfcExportDialog::refresh`) as the
drawing changes and before each preview and export, so "Selected entities
only" with nothing selected is refused rather than writing the whole drawing -
as `EXPORT ... SELECTED` is.

With the geoprocessing executor in the window, which takes `IMPORT`,
`EXPORT` and `INFO` of every other file, `MainWindow::runWorkbenchLine` asks
`MainWindow::runIfcLine` first, before the geo workbench: a typed line and a
dialog's line through `runVerbLine` reach the workbench before
`dispatchLine`, so without it the executor read an IFC line's options as
part of a path (`qt_ifc_typed_export_and_import_are_the_command_lines_headless`
failed exactly so). The session does the same (`src/katana_app/session.cpp`).

A .ifc reaches the same line from everywhere else a path enters: File > Import
and its IFC filter and a path given to the window run `IMPORT "<file>"`, the
typed `IMPORT`, `EXPORT` and `INFO` are split by the shared grammar before the
window's generic one takes the rest of the line as a path, the GIS imports'
"All files" and GIS > Dataset Information describe it
(`MainWindow::describeIfcFile`, `INFO`'s reply), and `--import-options` builds
the import dialog. Choosing IFC in Export Vector's file dialog opens the
export dialog with the file filled. The Command Reference lists the window's
own verbs after the interpreter's.

An import asks what the DXF import asks - a file far from the drawing is
shifted alongside or kept - weighing everything it brings against everything
the drawing holds, alignments and the session's surfaces too. When the file
names an EPSG code and the project has none, `TAKECRS` takes it and `KEEPCRS`
does not; the import dialog always says one (its "Take the file's coordinate
system"), File > Import and a path given to the window, which say neither,
ask the person there, and a typed `IMPORT` or a headless session asks no one
and says how (`CRS SET`) - `MainWindow::IfcLineFrom` names who ran the line.
The change is its own undoable step, after the import's. Declining the
far-apart question cancels the import (`CommandRejected`), which the dialog
says as a cancel, not a failure. Data shifted or moved to the origin is in no
system, and takes none. Terrain comes in as surfaces of the session
(`MainWindow::addSurface`), which the export also writes out - the one
exchange that carries the 3D view's surfaces both ways besides the .12da
archive.

`qt_widgets.IfcExportDialog.TheLineCarriesEveryChoiceAsTheVerbReadsIt`,
`qt_widgets.IfcExportDialog.PreviewShowsTheWritersAccountClassByClass` and the
rest of `qt_widgets.IfcExportDialog.*` and `qt_widgets.IfcImportDialog.*`
drive the dialogs against a command line that records their lines and answers
as the verb does; the `qt_ifc_*_headless` checks (`tests/CMakeLists.txt`)
drive each way in through the real window - the two dialogs, whose echoed
lines they read, the typed verbs, a path given to the window,
`--import-options`, Dataset Information, a file far from the drawing's
alignment, a missing file, and surfaces out and back.

## Survey > Subsurface Utilities (AS 5488): the same pattern, one verb

The AS 5488 tools (`docs/subsurface_utilities.md`) are `UtilityWorkbench`
(`src/katana_qt/survey/utility_workbench.hpp`), built as the Online Data
workbench is: `MainWindow::buildSurveyActions` makes it after the Survey
workbench and hands it the Survey menu, which it ends with the section
"Subsurface Utilities (AS 5488)" - Draw Utility Schedule, Utility
Investigation Report, Verify Detections Against Exposures, Clearance of
Proposed Works, Check Against a Delivery Schema, Regrade Drawn Utilities,
Export Drawn Utilities as a Schedule (`utilityDraw`, `utilityReport`,
`utilityVerify`, `utilityClearance`, `utilityCheck`, `utilityRegrade`,
`utilityWriteSchedule`; N and X were the letters the Survey menu had left).
`MainWindow::runCommandLine` hands it any line that starts with `UTILITY`,
before a running tool can take the line for an answer - unless that tool is
waiting for typed text (`ViewWorkspace::toolTakesText`: a Text's string, a
count), whose answer the whole line is, as it is for a bare `ZOOM`: a label on
a services plan may well read "Utility pit". `ONLINE` gives way to such a
tool the same way (`qt_utility_line_leaves_a_text_to_the_tool_headless`).

The verb is the `CommandInterpreter`'s, shared with `katana_cli`; the
workbench runs it through the window's interpreter and adds the one thing
only a window has: after a `UTILITY DRAW` or a `UTILITY REGRADE` that worked,
every plan view is framed on the reply's `bounds=` box
(`utilities::drawReplyBounds`, `ViewWorkspace::zoomTo`). A schedule in a real coordinate system lands far
from whatever the view was showing, and without the framing a draw that
worked looked like one that did nothing - the lesson Online Data learnt. The
box is read by the verb's own function, not a second parser in the window,
so the record's format has one reader to keep in step with its writer
(`utilities::formatUtilityDrawing`); the two branches that built this each
had one, and the window's was dropped when they were merged.

All seven items open ONE non-modal dialog (`UtilityToolsDialog`,
`src/katana_qt/survey/utility_dialog.hpp`, object name `utilityDialog`), each
on its own tab; the header lists every control's object name. The detected
spacing and the minimum cover sit above the tabs, shared by Draw, Report and
Regrade and live on those tabs only, because the verb reads `SPACING` and
`MINCOVER` for each: a drawing and a report of one schedule are graded and
flagged alike. The dialog never calls the AS 5488 library.

**Where the services come from** (`docs/cad.md`, "Scope and filter") is chosen above the tabs:
a schedule file (`utilitySourceFile`) or what is drawn
(`utilitySourceDrawing`), taken by Global Modify's own "Apply to" and "Only
those that match" controls - the shared `ScopeFilterWidget` (below), here
named `utility...` - on the left. For Draw, what is drawn is the lines,
polylines and points a survey or an import left there, drawn as services
(`docs/subsurface_utilities.md`, "Services from surveyed and imported
geometry"); for the others, the lines `UTILITY DRAW` drew. The widget's
`verbWords()` are the line's scope and filter words, so Report on the
drawing is `UTILITY REPORT DRAWING`, on a view `UTILITY REPORT VIEW 3` (or
`VIEW 3 EXTENTS` without "Only what is on screen"), and so on. Draw,
Report, Verify, Clearance and Check take either source; Regrade and Schedule
always the drawing, and the controls that do not apply to the tab in front
are disabled rather than hidden, so the choice is still seen. Draw of what is
drawn brings its group "Drawn geometry as services" (`utilityGeometryGroup`)
to life: what the geometry cannot say for itself - type, method, the two
uncertainties, what the heights are, level reference, path, owner,
material, diameter, status, and the Fields that read an import's own
attributes as the schedule's columns. Each choice lists the verb's own
words (`EML`, `pothole`, `service`), so the line reads as the choice does and
the headless `--fill` takes a choice as typed; "not given" leaves the option
out, for what each line and point says of itself. They were added on
2026-09-26, when Draw stopped being a file's alone: the owner's services
are surveyed by GNSS and total station, or come in a `.12da` archive, an IFC
file, a shapefile or a DXF file, and are post-processed in the drawing, not typed into a schedule
first. The scope defaults to the whole drawing, what the utility tools
usually mean. The layers, the views and the alignments follow the drawing
through a `DocumentWatcher`, and the views are read again whenever the dialog
is shown. Clearance's works are a design file (`utilityDesignFile`), a drawn
line or polyline (`utilityDesignEntity`: its `#id`, Use Selected for the one
entity selected, and a `LEVEL`) or one of the drawing's alignments
(`utilityDesignAlignment`, a choice of their names). A schedule measured
against a design file keeps the positional line it always had; every other
works follows `DESIGN`, which also ends a `WHERE` filter plainly. The
Regrade tab says that nothing is added to the undo history when nothing
changed, and the status says which happened; the Schedule tab names the
`.csv` to write and, optionally, the delivery schema whose words to write it
in. The status is read from the reply's first record, never from the tab
alone: a scope that takes no utility line is answered with the scope's
record, and the status then says nothing was regraded or written, rather
than offering an Undo that would take back the person's previous edit
(`UtilityDialog.AScopeThatTakesNoUtilityLineIsSaidSoAndNothingIsSaidRegradedOrWritten`).

`utilityCommandLine` - a pure function of
the fields, tested without a window - writes the `UTILITY` line (a path with
blanks quoted, a blank option left out, a file field left empty, a number
that does not read or a scope the controls cannot say refused with the field
named, and nothing run; a file that
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
`qt_utility_dialog_headless` (Run on Draw),
`qt_utility_draw_typed_frames_the_views_headless` (the same draw typed, as an
agent types it, framed the same),
`qt_utility_dialog_reports_what_is_drawn_and_what_the_view_shows_headless`
(Report on the drawing and on the plan view, on screen),
`qt_utility_dialog_regrades_what_is_drawn_headless` (nothing moved: no step,
framed), `qt_utility_dialog_writes_what_is_drawn_as_a_schedule_headless`
(the Schedule tab's file read back by a typed `REPORT`) and
`qt_utility_dialog_draws_what_an_import_left_headless` (a GIS file imported
by a typed `IMPORT`, then Draw on what is drawn with its Fields: the
sample's hand-worked figures). A Draw whose scope had nothing left to draw
says so in the status, and that no undo step was added
(`UtilityDialog.ADrawOfTheSurveyInTheDrawingRunsTheVerbAndSaysWhenNothingWasLeftToDraw`).

**The window answers `VIEW`.** The scope word `VIEW` (`docs/cad.md`, "Scope
and filter") is the window's: `MainWindow` gives its interpreter
`cad::scopeViewOf(views_->viewSet(), id)` through
`CommandInterpreter::setScopeContext`, next to the sheet context. No id is
the plan view Plot and the Standard Views act on (`ViewSet::mostRecent`); an
id is that open view, of any kind, with its own hidden layers and, for a
plan view, `ViewTransform::visibleWorldBounds` as it is when the line runs. So
`MODIFY VIEW WHERE TYPE=point SET COLOUR=#FF0000` typed in the window acts on
the points the plan view shows, where `katana_cli` refuses `VIEW` in favour
of `AREA` (`qt_modify_view_takes_what_the_plan_view_shows_headless`). The
rule lives in `katana_cad`, not a widget, so it is tested without a window
(`ScopeVerbsTest.AWorkspacesViewSetAnswersViewAsTheWindowDoes`).

## Global Modify

Format > Global Modify... (`GlobalModifyDialog`,
`src/katana_qt/customisation/global_modify_dialog.*`) states the request
`cad::planGlobalModify` takes (`docs/cad.md`, "Global Modify") and decides
nothing of its own. Non-modal, one instance kept by the Format workbench
(`CustomisationWorkbench::showGlobalModify`), as the managers are:

- **Apply to**: the selection, a view, the checked layers (with their
  sublayers or not), or the whole drawing. The views are the workspace's
  open ones, read afresh each time through `GlobalModifyDialog::views`
  (`scopeFilterViews(ViewSet&)`), so a closed view is never pointed at and a
  plan view's "only what is on screen" is its visible area as it is now; the
  dialog itself never sees the workspace, which is what lets a widget test
  give it views of its own.
- **Only those that match**: types, layer and style patterns, colour,
  property and value, text, drawn only.
- These two groups are `ScopeFilterWidget`
  (`src/katana_qt/customisation/scope_filter_widget.*`), the ONE set of
  scope and filter controls every dialog on drawing data shows (`docs/cad.md`,
  "Scope and filter"). Global Modify gives it the prefix `globalModify`, so its controls kept
  their names and its tests pass unchanged. It says what it holds twice from
  the same controls: `scope()` and `filter()`, what `matchEntities` takes and
  Global Modify plans with, and `verbWords()`, the same as the shared
  grammar's words (`cad::formatScopeWords`), for a dialog that builds a verb
  line. A view is named by its id, which the view list shows beside its
  title - "Plan 2 (VIEW 3)" - since the number in a title counts only the
  views of its kind; "Only what is on screen" unticked is
  `VIEW <id> EXTENTS`, a word added to the grammar for it, since without it
  the controls could say what no line could. What a line cannot say - a `:`
  in the property's name, a comma in a layer's - `verbWords()` refuses
  rather than write words that take something else; a value with the
  property left empty is `PROP=:value`, any property's. The fallback "Whole drawing
  view" of a dialog with no workspace is read by `scope()` and refused by
  `verbWords()`: a line can name only an open view.
  `tests/qt_widgets/customisation/test_scope_filter_widget.cpp` shows every
  scope and filter said in words, and those words, read back by the one
  parser and resolved with the window's answer for `VIEW`, taking exactly
  what the controls take.
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

## Terrain > Alignment Manager

The alignments, their PIs, design profiles and setting-out tables, in a
non-modal dialog the window keeps (`MainWindow::showAlignmentManager`,
`src/katana_qt/alignment_manager.hpp`): every edit an `ALIGN` line through
`runVerbLine`, the PI and PVI grids buffers applied by one line each. The
decisions are in `docs/cad.md`, "The Alignment Manager". The Terrain menu
opens with it, in an Alignments section, and the toolbar with its button.

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
