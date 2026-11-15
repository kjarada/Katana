# Desktop application

`katana` (the program in `crates/katana_window`, drawn with egui since
the Rust port; until 2026-11 it was a Qt program): the window, its theme,
icons and toolbars, the workspace of docked views, the panels and the
managers, and the rules every dialog follows. The window is egui on eframe,
drawn on wgpu, with egui_dock for the workspace of views
(`docs/rust.md`, "The owner's decisions"); its 3D views draw with the
software rasteriser for now, since the window does not yet hand its wgpu
device to them (`docs/gpu.md`, "Status"). Where a passage below tells how
the C++ window met a problem, it is the reason for a rule the Rust keeps;
the Rust's own answer, and why it differs, is in the module comment of the
file named beside it. What the window drives - the document, selection,
snapping, the interpreter and the engineering computations - is `katana_cad`,
in `docs/cad.md`; the drawing tools and their host are `docs/tools.md`; the
switches that run the window headlessly are `docs/headless.md`.

## The window

The window (`MainWindow`, `crates/katana_window/src/main_window/window.rs`) is a workspace of views, with dockable Layers, Properties, Command Line and
Reference Data panels. Its menu bar reads in the order a CAD user reads it -
File, Edit, View, Draw, Modify, Annotate, Format, Survey, Terrain, BIM, GIS,
Help (`katana_window::main_window::actions::compose`; each menu has a control name, `fileMenu` to
`helpMenu`; BIM, `bimMenu`, is the road model's - `docs/road_design.md`). File is the drawing as a file, what comes into it and goes out
of it - every import and every export, under two submenus - and the
program's Settings ("The File menu", below); the managers of what a
customisation brings are in Format and in Survey > Survey Coding ("The Format
menu"). The
status bar shows a view's running readout, how many entities are selected of
how many, the current layer, the active snap and the cursor coordinates.

### How the window starts

`main` (`crates/katana_window/src/bin/katana/main.rs`) reads the switches
through the program driver (`katana_window::drive::run`), builds the
`MainWindow` - its actions and menus, its panels, View's Window section and the
status bar, composed again from the window's state each frame
(`katana_window::main_window::actions::compose`) - and hands it to eframe,
which opens the native window with the application icon. The theme is
applied to the egui context before the first frame is drawn
(`katana_window::theme::style::apply`: the style, the palette and the one font
of the whole window - below, "Text"). The session's
customisation is installed next, so the first drawing is drawn with it ("The
customisation a session starts with", below). Then an
interactive run - one with no `--screenshot`, `--plot`, `--plot-sheets`,
`--sheets-json` or batch `--script` - puts the window where the last session
left it (`MainWindow::restore_session`) and shows it, and the project and files
on the command line are opened. With neither, it shows the start screen too
("The start screen", below), unless that was told not to show.

**The customisation a session starts with.** `MainWindow::load_default_customisation`
builds a `katana_cad::CustomisationHost` - the built-in customisation of this run
(`katana_cad::customisation_host::built_in_customisation`, which reads the seam
`KATANA_BUILTIN_CUSTOMISATION`) and the path of the kept file - hands it to
the interpreter (`CommandInterpreter::set_customisation_host`, FIRST: it notes
the kept file as it is at that moment, which `CUSTOMISE KEEP` later refuses
to write over once it has changed) and calls `katana_cad::customisation_host::start_customisation`, which
installs the kept customisation when the file is there and reads, else the
built-in one, else nothing. The window logs what that reports:

```
Customisation: <name>, N definitions (M symbols) and K survey code rules, built in.
Customisation: <name>, N definitions (M symbols) and K survey code rules, kept.
```

with the name the customisation gives itself (Katana Standard for the default
built-in; NSW when the seam names it, or for a kept copy of it), how many of its definitions it
lists as symbols, and which of the two it is - each noun by its count, so a
customisation of one rule has "1 survey code rule" - then, for a kept one
made from another built-in than this program has, a line saying so and naming
`CUSTOMISE RESET` and `CUSTOMISE BUILTIN`
(`qt_a_kept_customisation_made_from_another_built_in_is_installed_and_said_headless`).
A project opened that was drawn with a built-in the program has but is not the session
(saved while NSW was, opened under Katana Standard) gets the log's warning of the
customisations not loaded with one more sentence, `CUSTOMISE BUILTIN NSW makes the
built-in "NSW" the session`
(`qt_opening_it_under_the_default_names_the_built_in_and_the_verb_headless`).
A problem - a built-in that does not parse, a kept file that does
not read, the built-in then standing in for it - is an error line,
`Customisation: <the problem>`, once. A build with no built-in and no kept
file installs nothing and says nothing: it draws plain lines until a
customisation is loaded, which is a state the program runs in, not a fault
(`docs/customisation.md`; a build whose `KATANA_BUILTIN_CUSTOMISATION` names no
file has none, and a clean checkout has the tracked one).

- **Where the kept file is**, in order:
  1. the file the environment variable `KATANA_CUSTOMISATION` names, whatever
     kind of session this is;
  2. else, in an INTERACTIVE session, `customisation.json` in the per-user
     configuration folder, beside the window's settings file
     (`UserSettings::default_path`: `%APPDATA%\Katana` on Windows, else
     `$XDG_CONFIG_HOME/katana` or `~/.config/katana`), where the online
     sources a person adds are kept too;
  3. else none: a HEADLESS run with the variable unset. It must be the same
     run on every machine, so it reads and writes no per-user place - as it
     keeps no window layout (`docs/headless.md`, "The customisation a run
     starts with") - and `CUSTOMISE KEEP` and `REVERT` are then refused,
     naming the variable.

  That is why `load_default_customisation` is called after
  `MainWindow::set_headless`. The file need not exist: nothing is kept until a
  `KEEP` writes it, and the folder is made then. The variable is read with
  `katana_core::path_text::environment_variable`, as the session of `katana_cli` reads it (wide
  on Windows, UTF-8 out) and never with `getenv`, so a kept file in a folder
  whose name is outside the code page is still found. File > Settings shows
  the path, or that the session keeps none ("Settings", below). *Rejected:
  the per-user place in a headless run too.* Every fixture test would then
  start with whatever the machine's user last kept.
- **`--customise FILE...`** then runs `CUSTOMISE "<file>" ...` through the
  window's one executor (the program driver's `customise` step): merged into what the
  session started with, as the typed line would. A typed, scripted or
  `--customise` load lasts the session; `CUSTOMISE KEEP` is what makes it the
  next start's.
- **What the window does after a `CUSTOMISE` line it does for every change:**
  the panels' pickers, the views and the managers follow the Document's
  notification (the window's own listener, each view's own, a manager's
  `DocumentWatcher`), which an installed customisation raises as
  `StyleLibrary | SurveyMap | Customisation`. No verb tells the window
  anything, so a line run by a script, a dialog or an agent refreshes it as a
  typed one does.
- **Gone: the search beside the executable.** A build with nothing compiled
  in used to look for style libraries and survey code files in the program's
  folder and load what it found. That was a third
  state, decided at run time by what happened to lie in a folder - the same
  build drew differently on two machines - and it read the files of another
  program, which Katana no longer reads at all. Rejected: keeping it for
  Katana customisation files; the kept file is the one place for "my own",
  and it is said, not searched for.

`qt_the_window_starts_with_its_built_in_customisation_and_reset_returns_to_it_headless`
starts a window on a fixture named through the seam, replaces its
definitions, resets, and is refused a `KEEP`;
`qt_a_customisation_is_kept_headless` and
`qt_a_kept_customisation_is_what_the_next_window_starts_with_headless` keep
one in one process and start with it in the next. The per-user place itself
has no test: a headless run never has one, and a test that wrote a real
user's folder would be the thing rule 3 is there to prevent.

**Where the window was.** Until 2026-09-26 every session opened a 1360 x 860
window, whatever the screen: on a 1366 x 768 laptop it ran off the bottom,
and on a large screen it was a small window in a corner, with the panels and
toolbars wherever the toolkit put them and nothing a person moved kept. The
C++ window then kept the window's place and size as well as its layout, and
with nothing saved fitted the window to its screen (85% of the free area,
centred, or maximised on a screen smaller than 1440 x 900). The window keeps
a settings file of its own (`crates/katana_window/src/host/user_settings.rs`,
JSON in the per-user configuration folder, under the key names the C++ used,
`window/layout`, `appearance/textSize` and the rest, so a reader of either
finds the same names; the C++'s settings were the Windows registry's binary
form, which is not read, so nothing the C++ saved is carried over), and in it
the layout - which panels and toolbars are
shown, and the toolbars' rows - the text size, the toolbar icon size and
whether the toolbars show their names, written when the window closes and
read before its first frame (`katana_window::main_window::layout::restore_session`).
A layout from another version (`LAYOUT_VERSION`, raised when a change to the
toolbars or panels would make an old layout put them wrong; the one before
the third toolbar row is carried over, "Fitting the screen") is not applied.
Not done in the Rust: the window's place and size, and the minimised panels,
are not kept, and the native window opens at 1360 x 860 points whatever the
screen, as every session did before 2026-09-26. View > Reset Window Layout
puts the panels and toolbars back as a new window has them and leaves the
size and the appearance alone. The views inside the workspace are not kept:
they belong to the drawing being worked on, not to the window. A headless
run neither reads nor writes any of it (`docs/headless.md`), so a screenshot
is the same on every machine.

The readout (`FrameStatsLabel`) is a 3D view's frame time or a section's
station and elevation under the cursor, forwarded from whichever view raised
it (the workspace's `FrameStats` event) and kept apart from a view's status
message, so the status bar's message never hides it. It shows the most
recent view's readout, not only the active view's, and keeps its last text
while a plan view is active. It has no automated test; a screenshot of a 3D
view is what checked it.

The plan viewport (`PlanView`, `crates/katana_window/src/viewport_widget.rs`)
paints through the window's plan painter (`plan_painter.rs`) and turns mouse
input into commands. It
holds no geometry of its own: it paints whatever the model contains (Rule 3).
It is NOT on the `katana_render::draw_list::DrawList` path: the tile-binned software rasteriser
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

### The File menu: every import and export in two submenus

```
File                                                      fileMenu
  Drawing            New                                  fileNew
                     Open Project...                      fileOpen
                     Save                                 fileSave
                     Save As...                           fileSaveAs
  Import and Export  Import >                             fileImportMenu
                       Import Any File...    Ctrl+I         fileImport
                       Import IFC...                        fileImportIfc
                       Survey  Import Survey Points...      surveyImport
                       GIS     Import Vector Data...        importVectorData
                               Import Raster...             importRaster
                               Import Point Cloud...        importPointCloud
                               Online Data...               onlineData
                     Export >                             fileExportMenu
                       Export Drawing...     Ctrl+Shift+E   fileExportVector
                       Export IFC...                        fileExportIfc
                       Export View as Image...              fileExportViewImage
                       Survey  Export Survey Points...      surveyExport
                       GIS     Export Surface as DEM...     exportSurfaceDem
                               Export Point Cloud...        exportPointCloud
  Scripts            Run Script...                        fileRunScript
                     Recent Scripts >                     fileRecentScripts
  Plot               Plot to PDF...                       filePlot
                     Sheets...                            fileSheets
                     Plot Sheets to PDF...                filePlotSheets
  Project            Project Coordinate System...         fileProjectCrs
                     Drawing Summary...                   fileDrawingSummary
  ---                Settings...                          fileSettings -> settingsDialog
                     Quit                                 fileQuit
```

Importing was offered in three menus - File (two items), GIS (four) and
Survey (one) - and exporting in three, and the place a person looks for "how
do I get this file in" is File. Every import is now under File > Import and
every export under File > Export: File's own first, then Survey's and GIS's
under their names. The Survey and GIS menus still show their own, as before;
File had seventeen top-level items and has fifteen.

- **The items are the same actions** the Survey and GIS menus show,
  so an item has one text, one tip, one icon and one enabled state wherever
  it is shown, and `--trigger importRaster` finds one thing. They are put in
  BY CONTROL NAME once the menus are composed (`fill_by_name` in
  `crates/katana_window/src/main_window/actions.rs`): File is built first, and the Survey and GIS
  items are made later by their workbenches, which need know nothing of File.
  A name no action carries is left out and logged as an error once the log
  exists - `File > Import: no menu item is named <name>, so it is left out of
  this menu.` - because a renamed item must not quietly drop out of a menu.
  *Rejected: proxy actions* - a second action in File whose trigger runs the
  first. They pass the letter check with texts of their own, and that is the
  trouble: two texts, two tips and two enabled states to drift apart, and two
  names for a test to find one command by.
- **Three texts changed, and no control name.** A submenu is its own letter
  space (`--check-shortcuts`, "Every key reaches one thing"), and a shared
  action brings its letter into every menu that shows it. So `fileImport` is
  "Import Any File..." (A: I is the submenu's letter in File, and Import
  Survey Points' inside it) and `fileExportIfc` underlines F (E is Export
  Survey Points'). `fileExportVector` is "Export Drawing...": it writes DXF,
  archives and IFC as well as the GIS vector formats, which "Export Vector"
  did not say, and its tip says what it takes. Tests, scripts and
  `--trigger` find an item by its control name, which is why none moved.
- **Save As offers a folder the person owns.** `MainWindow::save_document_as`
  proposed the bare name `untitled.katana`, which a file dialog resolves
  against the working directory: for a Start menu shortcut or a double-click, the
  program's own folder, `bin/` of the install - read-only in a per-machine
  install, and no place for a project. It now proposes the name beside the
  project already open, else in the person's Documents. Not tested: the
  dialog is a native modal one, which a headless run refuses
  (`refuse_file_dialog`), so the folder it opens in has no seam to be read
  from.
- **No toolbar changed**, so `LAYOUT_VERSION` was not raised and a saved layout
  still fitted: File's toolbar keeps New, Open, Save, Import Any File, Export
  Drawing, Plot and Sheets.
- **Settings is above Quit.** The action is the Format workbench's, which
  owns the dialog ("Settings", below), and is put in when the menus are
  finished. The egui window draws its own menu bar, so on macOS too it stays
  in File, where the C++'s was moved into the application menu.
- **Every item that asks for a file is refused in a headless run**, naming
  the line that does the same. The GIS menu's three imports and Dataset
  Information were not: reached by name they opened a file dialog nobody
  could close, and the run waited until its timeout. They now answer as
  File's own items do (`MainWindow::refuse_file_dialog`), each naming the words
  of `IMPORT` that are its kind of data's, and `INFO <file>`
  (`qt_the_gis_items_that_ask_for_a_file_name_their_verbs_in_a_headless_run_headless`).
- **Open Project and Save As never start in the working folder.** A file
  dialog does when it is given no folder or a bare name (so the `rfd` dialogs
  are always given a folder), and the working folder is the
  program's own `bin\` for a zip copy (Explorer starts a double-clicked exe in
  its folder) and the install folder, hidden under `%LOCALAPPDATA%`, for the
  installed one: the first project was proposed as `bin\untitled.katana`, and
  removing a zip copy, which is deleting its folder, deleted the project with
  it. The dialogs start in the folder the open project is in,
  else the one the last project was opened or saved in (the `files/projectFolder`
  setting, written by a window a person used and never by a headless run), else
  Documents, else the home folder (`katana_window::project_folder::project_dialog_folder`,
  `project_folder::the_open_projects_folder_comes_first_then_the_last_one_then_documents`);
  Plot to PDF and the Sheet Set menu already started in Documents.

`qt_file_keeps_every_import_and_export_in_two_submenus_and_settings_above_quit_headless`
reads the three menus whole, item by item in this order, with Survey's last
section; `qt_every_shortcut_and_menu_letter_reaches_one_thing_headless` and
`qt_every_menu_item_has_an_icon_and_a_status_tip_headless` hold the letters
and the two submenus' own icons and tips.

Not done:

- A menu's headless report (`?fileImportMenu`) lists items, not the section
  titles between them; that Survey and GIS head their groups is seen in a
  screenshot (`%fileImportMenu`) and nowhere asserted.
- The missing-name error has no test of its own: in the real window every
  name exists. A name misspelled by hand logs the error and fails the test
  above.
- The Reference Data panel's Import button and the imports inside dialogs
  (the code manager's, the symbol library's) are where they were.

## The plan view's shortcut menu

A right-click in a plan view with no tool running opens the shortcut menu
(`PlanContextMenu`, `crates/katana_window/src/plan_context_menu.rs`, control name
`planContextMenu`); so does the keyboard's menu key, at the cursor. With
nothing selected, the entity under the cursor is selected first, so the menu
acts on what was clicked; a selection already there is kept, since the click
may land beside what the menu is for (`PlanView::select_for_menu`).

It holds no command of its own. Its items are the window's existing actions,
found by control name, so the menu item, the menu bar's, the toolbar button
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
shares is ticked. A name is written by `named_line`, through `command_word`
("One rule for a word on the line", below): bare when it can be, quoted when
it is empty or holds a blank (`CHLAYER 0`, `CHLAYER "Site Boundary"`), and a
name holding a double quote or a line break is offered disabled.

**An item never reaches back through its menu.** Every item holds its own
copies of what it calls - the runner, the colour chooser, the tool starter -
and never reads the menu after it is triggered
(`crates/katana_window/src/plan_context_menu.rs`). The egui menu is rows
drawn each frame and owns nothing an item calls through; the rule is kept
because the menu may be gone before its item finishes. That was the C++
crash of 2026-11-03, recolouring 774 selected leaders: the menu was deleted
while Colour > Choose waited in its modal colour dialog, and the item then
reached its runner through the deleted menu (any selection crashed the same
way).
`plan_context_menu::choosing_a_colour_runs_color_even_when_the_menu_is_destroyed_while_it_asks`
destroys the menu inside the chooser, with a runner that says when it is
called through a destroyed copy. Rejected: keeping the menu alive until the
item returns: every item would have to remember to do it, where a copy
cannot be forgotten.

**On a grip.** Opened on a selected polyline's grip (`PlanView::grip_at`,
the grip within the pick aperture of the right-click), the menu begins with
that vertex's or segment's own items, under a section naming it ("Vertex 2
of polyline 12"). The edits are verb lines through the same executor: Delete
Vertex (`planContextVertex.Delete`, `VERTEX DELETE 12 2`), Insert Vertex
After (`planContextVertex.InsertAfter`, `VERTEX INSERT 12 #12.s2 after=2`,
the middle of the segment after it), Make It the Start
(`planContextVertex.Start`, `STARTVERTEX 12 2`, a closed polyline's), Add
Vertex at the Middle (`planContextSegment.AddMiddle`) and Make Straight
(`planContextSegment.Line`, `VERTEX SET 12 1 bulge=0`, an arc's only). The
tools - Move Vertex, Set Height, Fillet and Chamfer Corner, Straighten From
Here, Insert Vertex and Make an Arc on a segment - start with the grip hot,
as a click on it before choosing the tool would make it, so the tool takes
it as its handle (`docs/drawing.md`, "What a vertex tool acts on"). An item
that cannot apply is offered disabled with the reason as its status tip:
Delete on a polyline with no vertex to spare, Insert After on an open
polyline's last vertex, Fillet and Chamfer where there is no corner (an end,
beside an arc, in line).

`popup`, not `exec`: nothing waits on the menu, and it deletes itself when it
closes. It is not built in a headless run, which has no mouse; everything on
it is a menu action or a verb, so an agent reaches the same through
`--trigger` and `--command`. `crates/katana_window/tests/plan_view/plan_context_menu.rs`
checks what it offers with and without a selection, that Put on Layer is one
undo step, the layer tree, the ticks, and that the view raises it from a view
made before the window set the hook (the workspace's first plan view is);
and on a grip, the vertex's and the segment's items, their lines, the tool
started with the grip, and what is offered disabled
(`plan_context_menu::a_vertex_grips_items_run_the_vertex_verbs_and_start_the_tools_on_it`,
`plan_context_menu::an_end_vertex_offers_no_corner_and_no_segment_after_it`,
`plan_context_menu::a_segment_grip_offers_make_straight_only_on_an_arc`).

**Double click to edit.** A double click on an entity with no tool running
(`PlanView::mouse_double_click`, after its first click selected it)
makes it the whole selection and opens what edits it: Edit Text for a text
and Edit Label for a label, or the Leaders manager for a leader, where the
window has those actions, and the
Properties panel for anything else (`MainWindow::edit_double_clicked`). On empty
space it does nothing; while a tool runs it is two clicks for the tool.

With leaders selected, the shortcut menu also offers **Edit Leader...** and
**Leader Note Frame > None / Box / Circle**. The latter runs
`LEADER SET SELECTION callout=...`, so it takes only the leaders among the
selection and is one undo step. The existing manager now exposes exact
point coordinates and landing direction beside its note and appearance
controls (`docs/annotation.md`, "Moving and framing existing leaders").

**Select by ID** (Edit > Select by ID..., `editSelectById`;
`crates/katana_window/src/select_by_id_dialog.rs`) turns the ids that `LIST`, `INFO`,
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

**Every Edit and View action has a control name**, so `--trigger` and
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
very large radius with only a sliver on screen never hands the painter
coordinates in the millions. Text below three pixels tall is drawn as a baseline stroke rather than
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
`Document::set_current_layer`, deliberately not a command - it changes what the
next drawing tool does, not the drawing (`docs/cad.md`, "Document"). Opening a project whose
database is damaged offers to restore the newest sound backup.

**The current style and the Style row (decision D9).** The Properties
toolbar carries the style new work is drawn in (`CurrentStyleCombo`): ByLayer
first, then the drawing's styles by name. Choosing one is
`Document::set_current_style`, session state like the current layer and so not a
command, and the log says "New work is drawn in ...". It acts on the choice
made in the box (`current_style_ui`, `crates/katana_window/src/main_window/panels.rs`),
and the list is drawn afresh from the drawing each frame, so a headless
`--fill` sets it as a person's pick does and no rebuild of the list can be
taken for a choice. A
current style the list does not hold is shown, marked "(not in the drawing)",
never replaced by the first choice - the QT-02 lesson. The Properties panel
has a Style row above its read-only table (`PropertyStyle`, editable, and
`PropertyStyleApply`): it shows the selection's style as it is, even a name
the drawing lacks, or "<varies>" when the selection disagrees, and Apply or
Enter gives the selection the style typed or chosen as one undo step
(`MainWindow::apply_property_style`, `katana_commands::set_entity_style`). Only the
entities that change are in that step, so applying the style already shown
is no step at all; and nothing is applied merely because the box's text changed.
A rename or delete of the current style clears it rather than following it
(`docs/cad.md`, "Document"), so the toolbar then shows ByLayer.
`qt_current_style_and_the_style_row_headless` drives both.

## The start screen

What a session starts with, chosen as it starts (`StartDialog`, `startDialog`,
`crates/katana_window/src/start_dialog.rs`):

- **The customisation** (`startCustomisation`). It lists:
  - the session as it is, first, when that is no built-in as installed (a kept
    copy, a file loaded, or nothing at all, as in a run whose seam says
    `none`), and choosing it makes no line;
  - every built-in, from `katana_cad::customisation_host::built_in_choices`, the one answer
    `CUSTOMISE BUILTIN` and Settings' chooser read, with the session's own
    chosen to begin with;
  - "From a customisation file...", with `startCustomisationFile`,
    `startCustomisationBrowse` and `startCustomisationReplace`.

  `startCustomisationKeep` ("Use it at every start") adds `CUSTOMISE KEEP`. It
  is ticked when the session was the kept one, as Settings' Use keeps what was
  kept, and offered only for a choice that changes the session, in a window
  that keeps a customisation.
- **The coordinate system** (`startCrs`): local coordinates, the common
  systems (`katana_cad::project_crs::common_coordinate_systems`), found by any part of their code or
  name as they are typed, or any EPSG code, WKT or PROJ string typed. It is
  checked as it is typed (`startCrsCheck`), in the Project Coordinate System
  dialog's words: both call `crs_check_text`. The drawing's own system is
  offered first; with none, the one chosen last.
- **Start** (`startOk`) runs the lines `startLine` shows, one by one, through
  the window's executor, so each is logged and is an undo step as typed:
  `CUSTOMISE BUILTIN <name>` or `CUSTOMISE [REPLACE] "<file>"`, then
  `CUSTOMISE KEEP`, then `CRS SET <text>` or `CRS CLEAR`. A choice that is what
  the session has already makes no line, so Start on the screen as it opens
  changes nothing. It stops at the first line refused and says why in
  `startStatus`, and stays open. **Skip** (`startSkip`) closes it.
- **When it shows.** An interactive start with no project or file to open shows
  it (`MainWindow::show_start_screen_if_wanted`), until `startShowAtStart` ("Show
  this at start") is cleared. File > Start Screen (`fileStartScreen`) opens it
  at any time, on the drawing as it is then. A project opened brings its own
  system, and its record of what it was drawn with, so a start that opens one
  does not ask.
- **What it remembers.** The window keeps whether to show it
  (`start/showAtStart`) and the system chosen last (`start/coordinateSystem`),
  in its settings, unless the run is headless. The customisation is kept the
  one way a customisation is kept, by `CUSTOMISE KEEP`. The dialog reads and
  writes no settings itself, so it is tested without the registry
  (`crates/katana_window/tests/workspace/start_dialog.rs`). The real window is driven by
  `qt_the_start_screen_sets_the_coordinate_system_through_the_executor_headless`.

Non-modal, as the Project Coordinate System dialog is: a headless run fills it
by its control names (`--dialog fileStartScreen`), and the window behind it can
be used. *Rejected:* a modal dialog before the window shows, which would hold
up a start that only wants to open a file, and which `--dialog` could not
fill. *Rejected:* a second home for the remembered customisation beside the
kept file, which would let the two disagree about what the next start gives.

katana_cli and katana_mcp make the same choices with the same lines: the
customisation with `CUSTOMISE BUILTIN`, `CUSTOMISE "<file>"` and the variables
`KATANA_BUILTIN_CUSTOMISATION` and `KATANA_CUSTOMISATION` (`docs/customisation.md`),
and the system with `CRS SET`. They read no per-user settings, so neither has
a start screen or remembers the system.

## Fitting the screen

A menu or a dialog taller than the screen it is on has its bottom off it: the
last items, the Close button, the results. Until 2026-10-15 nothing kept a
window on its screen but the main window's own first-run size (above) and
the choice lists ("The look of the application"); the
managers and dialogs of the terrain and road work are a thousand pixels tall
and more, and a 1366 x 768 laptop leaves 728 under its task bar. Each piece is
the application's, not each dialog's, because every dialog written to fit one
screen is one more that a smaller screen proves wrong.

* **Menus scroll.** A menu taller than its screen is ONE column as tall as the
  room there is, which scrolls (`crates/katana_window/src/shell/dialog.rs`,
  `popup_menu`). Laid out in columns side by side instead, as the C++'s toolkit
  did until it was told otherwise, the Terrain menu became a wall of three, as
  tall as the screen whatever the menu bar's place on it, so its bottom ran
  under the task bar. A menu that fits is not changed
  (`screen_fit::a_menu_taller_than_the_screen_is_one_scrolling_column`,
  `screen_fit::a_menu_that_fits_is_not_scrolled`).
* **Dialogs are fitted to their screen.** Every dialog of the window is drawn
  inside one dialog frame (`DialogFrame`, `crates/katana_window/src/shell/dialog.rs`),
  which takes the room the dialog's screen has - the available area, less a
  margin all round - and:
  * a dialog whose controls need no more than that is left exactly as it is
    drawn, its size only limited;
  * one whose controls cannot be laid out in that room scrolls, so every
    control is reached by scrolling and the window is as large as the screen
    lets it be. Each control keeps its control name inside the scrolling body:
    whatever finds one by name finds it still;
  * the dialog's own buttons are not scrolled with the rest. Its **footer** -
    its row of buttons and what is said under them, the reply to what they
    did - is kept below the scrolling body, so Close, Run and Add are on the
    dialog as it stands and do not move as the body scrolls, because what the
    buttons answered is read under them and scrolling it out of sight would
    look like the button had done nothing. A footer that needs more than half
    the room is not kept out, because it would leave too little to scroll;
  * a dialog that was moved by hand off the screen is brought back onto it;
    one that was not is centred in the room.
  The frame measures what it drew - what it takes beyond the body, its title
  bar, borders, footer and spacing - and keeps that from one frame to the
  next, so a dialog is fitted at every frame, on whatever screen it is on now.
* **Tests.** `screen_fit::*` (`crates/katana_window/tests/foundation/screen_fit.rs`):
  a dialog that fits is left exactly as it was written; one taller than the
  screen scrolls and every control can be scrolled into view; one wider than
  the screen scrolls across as well; a dialog placed off the screen is brought
  back; the room is inside the screen's available area with a margin round it;
  a dialog's buttons stay below the scrolling body and in sight whatever is
  scrolled, and a button row and the reply under it are the footer together;
  and `volumes_dialog::is_no_taller_than_the_screen_and_still_reaches_its_close_button`
  is the real tallest dialog. `screen_fit_dialogs::*`
  (`crates/katana_window/tests/main_window/screen_fit_dialogs.rs`) gives
  each real dialog a minimum height over the screen's, so that it scrolls
  whatever its own layout needs, and holds its buttons and reply in the
  footer, on the dialog and still as the body scrolls: Surface Edit's Heights
  and Delete Points, Combine Surfaces, Volumes and Road Objects.
* **Decided.** A scroll area for the whole dialog rather than a taller screen's
  worth of smaller pages: no dialog has to be redesigned, and a person with room
  sees nothing different. *Rejected:* each dialog sizing itself (the sizes were
  chosen for the developer's screen, and the next dialog would forget).
* **The footer is declared by the dialog.** An egui dialog is drawn each
  frame inside its frame, so there is no finished layout to read: the dialog
  hands the frame its footer as the second part of what it draws, and
  forgetting it is visible in the dialog's own code, not a rule a reader must
  remember. The C++ fitted a dialog already laid out, once, as it was shown,
  and had to FIND the footer by a rule read off the layout (the last row of
  buttons and everything after it); its cases about finding a footer are
  exempt from the port for that. *Rejected:* letting a dialog that is too tall
  shrink with no scrolling, which clipped or overlapped what no longer fitted.

**The toolbars along the top: three rows, measured.** A toolbar that has not
the width for its buttons hides the last ones behind an extension arrow, so
the arrangement is chosen by what each toolbar needs side by side, which is
the width at which every action shows (its natural width, the strip's
layout in `crates/katana_window/src/tools/flyout_button.rs` with the toolbar's
padding and spacing). Measured alone on the C++ window, in px: Survey 565,
Terrain 379, GIS 312, Surface Edit 692 (13 buttons and the surface list), BIM
384; File to Format together, about 1,180. With a 12 px handle before each, the second row
of Survey, Terrain and GIS needs about 1,290 px and the third, Surface Edit and
BIM, about 1,100, against the 1,366 of a 1366 x 768 laptop: both fit. The five
on one row need some 2,400, which no common screen has, so Surface Edit, BIM
and GIS showed a handle and an arrow only.
`MainWindow::tool_bar_fit` reads the result off the window (a shown toolbar
along the top whose button is not visible is one behind its arrow) and
`katana --check-toolbar-fit 1366x768,1632x918,1920x1080` fails a run that
finds one, printing each row's natural width beside the width it has:
`qt_every_toolbar_along_the_top_shows_all_its_actions_at_laptop_and_desktop_widths_headless`.
1632 x 918 is the 85% of a 1920 x 1080 screen that a first run of the C++
opened at. The test is a run of the real window, as the arrangement is
`MainWindow`'s (`docs/headless.md`); it is the same run as `--check-menus`
and `--check-toolbars`. The Rust's natural widths are 2 to 22 points off the
C++'s, which laid a toolbar out by its toolkit's metrics (its handle, spacing
and each button's size hint); every row still fits, and the test passes
(`docs/rust.md`, "Not done").
* **Decided: a third row.** A row costs a toolbar's height (about 45 px) of a
  768 px laptop, where the drawing needs it, and the alternatives were each
  weighed against that. *Rejected:* the bottom edge, beside the Drafting bar
  (Ortho, Polar, Tracking ...), which has the room and costs no height, but
  puts workflow tools among the drafting aids, away from the menus they
  belong to; Surface Edit hidden until Terrain > Edit Surface asks for it, since
  a toolbar that is not shown fails "every toolbar shows its actions" by
  hiding, and the tool a person looks for is then not there; Terrain and
  Surface Edit together on one row with Survey, BIM and GIS on the other
  (1,100 and 1,300 px: it fits too, and it groups the surface tools, but it
  is no narrower than the second row kept - 1,290 - and Survey, Terrain and GIS
  there is what every saved layout of the older builds already has);
  shrinking Surface Edit's buttons, which would not match its neighbours.
* **A saved layout.** `LAYOUT_VERSION` is 2 and `restore_layout` is the one door
  `restore_session` takes (`crates/katana_window/src/main_window/layout.rs`).
  A layout of version 2 is restored as it is, a hidden toolbar staying hidden.
  A layout of version 1 - every one saved before this change, from builds with
  no Surface Edit and BIM, or with them squeezed on the second row - is
  restored too, with those two seated on a third row (whether the person had
  hidden either is read first and kept), and a toolbar the layout does not
  place at all is put along the top and shown, so a toolbar added later is
  never lost by an older layout. *Rejected:* raising
  the version with no carry-over, which would send every saved layout back to
  the built one - the panels' places too - for the sake of two toolbars (the
  reason the BIM toolbar did not raise it); keeping the version and testing for
  overflow after the restore, which cannot tell a crowded row a person chose
  from one the build made. Checked by the same run: a layout with the two bars
  on the second row (it first proves that layout overflows, so the check can
  fail), one saved before they existed, one of this version with BIM hidden,
  and an older one with Surface Edit hidden. A layout saved by a build of
  2026-10-15, with the bars on the second row, is repaired the same way.
* **A layout is restored before the first frame.** `--check-session-restore`
  (the
  `qt_a_layout_saved_maximised_by_an_older_build_restores_before_the_window_is_shown_headless`
  test) restores a version 1 layout before the window draws, where the
  in-process check above restores into a window already drawn. It was written
  for a C++ crash (2026-10-15 to 2026-10-21: a version 1 layout saved with the
  window maximised crashed every start, through a copy of the layout the
  toolkit kept for a window not yet shown); the Rust's layout is its own JSON,
  applied once with nothing kept, so the order cannot bite it, and the check
  stays as the guard.

Not done: a window narrower
than about 1,290 px (a 1280-px laptop) still overflows the second row, and
larger icons or text widen every toolbar (a button is 41 px at Large Icons
against 33); no setting chooses a density from the screen; a floating dock is
not fitted; the native window opens at 1360 x 860 points whatever the screen,
since its place and size are not kept ("How the window starts").

## Failure modes

Every command failure surfaces as a message in the command log and, for errors,
the status bar; the model is untouched. Closing the window asks twice, in this
order (`MainWindow::close`): first the Survey Code Manager, whose
unapplied rule edits live only in its buffer - it is shown and asks Apply /
Discard / Cancel over the rules it is about (`CustomisationWorkbench::confirm_close`),
first because its Apply changes the drawing the next question is about - and
then, with unsaved changes, save, discard or cancel (`confirm_discard`). A
headless run has nobody to answer either, so it refuses and says why in the
log, never discarding anything unasked: a scripted `QUIT` with unapplied code
edits, or `NEW` after an edit, is refused, and the script can Apply or Revert
(`applyMap`, `revertMap`), `SAVE` or `UNDO` first. The definition editor's
unsaved form is asked about in the same call, before the code manager
(discard, or stay; a headless run is refused and can press `definitionSave`
or `definitionRevert`): "The definition editor", below. A yes there discards
nothing by itself - the questions after it may still keep the window open -
and is remembered until the form next changes. A failed save reports the
error and leaves the modified flag set. A damaged project offers backup
recovery and never deletes the damaged file.

## The look of the application: theme, icons, toolbars

The viewport has been dark (`#1e2329`) from the start, and the window around
it was whatever grey the toolkit defaulted to. A dark drawing in a light
frame is most of why the application looked unfinished. Three files under
`crates/katana_window/src/` fix that: the palette, the style and the icons.

**`palette.rs` - every chrome colour is a named token**
(`crates/katana_window/src/theme/palette.rs`). `window`, `panel`,
`raised`, `hover`, `border`, `text`, `text_muted`, `accent` and so on, as
functions. egui's look is ASSEMBLED from them (`style.rs`'s `visuals`, one
for the dark theme and the light) rather than written with colour literals,
so the tokens are the only place a colour is decided and a change of theme is
a change to one file. Nothing else under `crates/katana_window` should
contain a widget colour literal; the viewports keep their own constants for
DRAWING colours (grid, snap marker, selection), which are content, not
chrome. egui draws its own controls, the same on every platform, so the
system's light or dark preference never changes the look: both of `style.rs`'s
`visuals` come from the tokens. (A platform's native controls were what the
C++ had to avoid - the native Windows style drew light controls into a dark
palette.) The accent is blue, not the red of the logo: in an engineering
program red already means "error", and a checked tool must not read as one.

**Text.** The fonts are tokens too (`katana_window::theme::style::ui_font`, `katana_window::theme::style::monospace_font`,
`katana_window::theme::style::overlay_font`). The whole window is one font:
`katana_window::theme::style::set_text_size` sets every text style of the
context at the chosen size. That was not always so - the command line, the
plan view's prompt and eighteen dialogs' reports each once named a face and
size of their own, and the menus and tooltips kept whatever the platform gave
them - which is why no part of the window names a font now. The face is
egui's embedded sans (Ubuntu-Light, which has no bold face; the C++ took the
platform's UI face, Segoe UI on Windows); which face the window carries is
the owner's to decide (`docs/rust.md`, "Decisions waiting on the owner"), and
changes in `style.rs` only. The sizes are still a step from the platform's
9 points (`PLATFORM_POINTS`). The fixed-pitch font
is the first the machine has of Cascadia Mono, Consolas, DejaVu Sans Mono,
Liberation Mono and Menlo, then the platform's own, at the same size as the
chrome, so a column of coordinates in a report lines up and is the size of
the text around it. **View > Text Size** (`viewTextSizeSmall` to
`viewTextSizeExtraLarge`) moves every font together: Small is the platform's
size less a point, Large two points more and Extra Large four - one point
(9 to 10) is a difference few people see. The command line's font, set on
the widget, is set again with the rest. The drawing's own text face
(`PlanPaintOptions::font_family`) is content, like the viewport's colours, and
is not a theme font: a plot must print the same whatever size the window's
text is.

**`style.rs` - three things a toolkit's defaults got wrong.** The style
(`crates/katana_window/src/theme/style.rs`) holds three rules, each learned
when a default hid something; egui's defaults need them said too, as the
C++'s toolkit did. First, a menu section shows its title: the
Survey, Terrain and GIS menus were grouped under titled sections
("Surfaces", "Point Cloud - PDAL") and for weeks not one title was seen,
because the C++'s style drew each section as a plain separator. `menu_section`
draws the title in small muted capitals
(`theme::a_menu_section_shows_its_title_under_the_stylesheet`). Every long menu is now in titled sections: File
(Drawing, Import and Export, Scripts, Plot, Project), View (Display,
Viewports, 3D Views, Window), Format (Tables and Libraries, Across the
Drawing, Annotation Styles), Annotate's editors and Leader
Manager, and the tool menus by their catalogue groups (Lines, Curves,
Transform, Edit, Dimensions ...; `crates/katana_window/src/tools/tool_menus.rs`). Second, a
choice list shows `MAX_VISIBLE_ITEMS` rows and scrolls the rest
(`theme::a_long_choice_list_scrolls_rather_than_running_off_the_screen`): a
popup the height of every item, as the C++'s style once dropped, ran a
drawing's two hundred styles off the screen. Third, a toolbar's overflow
arrow is two chevrons in the muted text colour, in whole device pixels, and
in the accent while a tool runs whose button it hides
(`paint_overflow_arrow`; "The overflow arrow", below): the toolkit's own was
a small dark image made for light toolbars, and once it was not seen at
all.

**`icons.rs` - the icons are drawn in code.** Each is a vector drawing on a
24-unit grid painted through the window's canvas at whatever size and
device-pixel ratio it is shown at. There are no image files behind them, for
four reasons: they are crisp at every size because nothing is resampled; they
take their colours from the theme, so a theme change recolours all of them; a
function that is wrong does not compile, where a resource path that is wrong
is a blank button; and an SVG or PNG set would need an image decoder or an
SVG renderer, a dependency added for decoration. The language is one rule
applied everywhere: a 1.7-unit round stroke, neutral for the object and ACCENT
for the part the command acts on - the arrow of Import, the new geometry of a
draw tool, the cut line of a section. 1.7 rather than the usual 2.0 because at
16 px the heavier stroke closes up small counters (the label of Save, the
inside of the magnet).

**The application icon comes from the same painter.** `katana_make_icons`
writes `resources/katana.ico` (nine sizes, PNG-compressed entries), a 256 px
PNG, and `icon_sheet.png` - every icon at 96 px and at the 20 px it ships at.
The `.ico` is COMMITTED, not generated in the build: generating it would make
an ordinary build run a program of the build before it could compile a
resource file, which fails wherever that program cannot start, and buys
nothing for an icon that changes once a year. Below 64 px the sword is drawn
larger and heavier within its tile; at true proportions it is a few pixels
wide on a taskbar and disappears. The window takes its icon at run time
(eframe's viewport icon, `crates/katana_window/src/drive/desktop.rs`).
Not done: `katana.exe` carries no Windows resource - no icon and no
`VERSIONINFO` block, both of which the C++ build embedded - so Explorer shows
a generic icon and no version for the file itself; the running window has
its icon.

**Every menu item has an icon and says what it does.** Until 2026-09-26
sixty menu items had no icon or no status tip, or neither: the snap modes,
the viewport layouts, the standard 3D views, Deselect, Quit, the annotation
editors and managers, every submenu. `MainWindow::menu_gaps` walks the menu
bar and every submenu and lists each item without both; `katana
--check-menus` fails a run that has one, and
`qt_every_menu_item_has_an_icon_and_a_status_tip_headless` runs it. The new
icons keep the one language: a snap mode is the mark the plan view draws for
it, on the geometry it snaps to; a layout is its panes with the main view in
the accent; an orthographic standard view is the object in plan with the face
seen in the accent and the eye's arrow coming at it, and an isometric one the
cube with the arrow from its corner. `icons::the_list_holds_every_icon_once_and_each_paints_something`
holds `all_icons` to every enumerator, in order, each painting at 16 px.

**The contact sheet earned its keep at once.** The first application icon had
the handle running UP the blade with the guard at the pommel - a sign error in
the tangent - and the Circle tool read as a minus sign in a ring. Both were
obvious on the sheet and invisible in the code.

**Toolbars share their actions with the menus.** Each action is made once
(`crates/katana_window/src/main_window/actions.rs`) - text, shortcut,
icon, status tip and a tooltip that names the command and shows its
shortcut, since an icon-only button owes the user its name - and the same
action goes into the menu and the toolbar, so the two cannot drift. File,
Edit, Properties (the current style), View and Format run along the top;
Survey, Terrain and GIS have a second row of their own, because in one row
they were squeezed to a button each behind overflow arrows, and Surface Edit
and BIM a third:
Surface Edit (692 px), BIM (384) and the second row's three (1,256) are some
2,400 px of toolbars with their handles, and the 1,366 of a laptop's screen
holds the second row (about 1,290) and the third (about 1,100) but not both in
one. For a day (2026-10-15) the five shared the second row, and Surface Edit,
BIM and GIS showed a handle and an arrow and none of their buttons. "Fitting
the screen" has how it is checked and what was rejected.
Draw and Annotate run down the LEFT edge, where
every CAD program keeps its drawing tools, because they are the ones reached
for without looking and a strip beside the drawing is a shorter journey than a
row above it; Modify runs down the RIGHT edge, as in the classic CAD layout,
because one column could not hold all three without hiding the last tools
behind an overflow arrow. Every toolbar and dock has a control name, which is
what a saved layout keys on. View > Panels brings back any dock that has been
closed.

**A toolbar says its name.** Two rows of icon-only buttons read as one
undivided strip, and which icons were Survey's and which Terrain's was a
matter of hovering over each. A toolbar along the top or the bottom now
starts with its name, muted (the label `toolBarName`); a vertical one has no room for a word, and the
Properties bar's " Style " label already says what it holds. View >
Toolbars (`viewToolBars`) shows or hides each toolbar (`viewToolBarFile` ...),
turns the names off (`viewToolBarNames`) and sizes the icons: Small 16 px,
Standard 20, Large 28 (`viewToolBarIconsSmall` ...). Words under every icon were
rejected: the two top rows hold some sixty buttons, and with their words they
would not fit in the widest window.

**Tool families on the toolbars: a mark in the corner, not a split button.**
A family of tools - Circle's six, Arc's five, Ellipse's three, the eighteen
Vertices tools - is one button on the Draw toolbar
(`crates/katana_window/src/tools/tool_menus.rs`). Until 2026-09-30 it was a
split button, and the split button's drop-down strip was laid over the
button's right 12 px, painted after the icon - solid under the pointer and
while the tool ran, cutting the circle's ring - and took presses there: a
click on the right of the ring opened the menu instead of starting Circle.

**One painter for every tool button.** A family, an action's own button,
Undo and Redo's split buttons, the overflow button and the docks' title bar
buttons are all painted by `paint_tool_button` from a `ButtonPaint`, through
the window's canvas - on the screen as an image kept in the icon cache, and
for the checks by `drawn_parts`, which paints the button against itself - so
what `--check-toolbars` measures is what the screen shows. The geometry is
the one the C++ window settled on, in points: a 1 px border and 4 px padding,
a 5 px mark inset 2 px, a 12 px press zone, a 12 px strip, a 12 px overflow
button (the constants at the head of
`crates/katana_window/src/tools/flyout_button.rs`, each with where it came
from). The pixel measures in this section and the ones after it were taken on
the C++ window; the Rust paints the same geometry, and the tests that pinned
them pin the Rust's.

The family button is a `Flyout` (`crates/katana_window/src/tools/flyout_button.rs`).
Its icon has the whole button, as on every other tool button, and a small
triangle in the bottom-right corner - in the 5 px the centred icon leaves
free at every toolbar icon size, inside the border - says that the button
holds more than one tool, which is how drawing and graphics programs mark a
tool group. It is muted at rest and lighter under the pointer; it takes the
accent while the pointer is on its corner, the part of the button that opens
the family, and while a tool of the family runs. A click anywhere on the icon
runs the family's first tool. The family opens from a press on the
triangle's corner, from a press held for the popup delay (600 ms,
`POPUP_DELAY_SECONDS`, the delay the C++'s held press had) and from a right
click. The corner is 12 px each
way, the width of the strip it replaced, less the icon's square - so less
area than the strip, an L of 119 square pixels at 20 px icons against its
396, the icon keeping its whole square; the held press and the right click,
anywhere on the button, make up for it. The tooltip says how to open the
menu and names the family's other tools, five at most: past five it names
four and says how many more ("Delete Vertex / Move Vertex / Edit Vertices /
Straighten and 13 more"), since all seventeen of the Vertices family's
others made a wall of six lines and the menu lists them anyway, while naming
three of Circle's five and "and 2 more" was hardly shorter than naming them
all. A name's words are held together by non-breaking spaces, so a long
tooltip wraps between names and never inside one ("... 2 Points / 3" then
"Points and 2 more" was Circle's). On the corner that opens the family the
status bar shows the family's own line ("Vertices: Insert Vertex / Delete
Vertex / ..."), and elsewhere on the button the tool's, where it showed the
first tool's alone. The menu is the Draw menu's own submenu, one action per
tool, so a tool is the same object in both.

**A held press opens the menu and the release starts nothing.**
`tool_family_buttons::a_click_on_the_icon_runs_the_tool_while_the_mark_a_hold_or_a_right_click_opens_the_family`
holds a press past the popup delay, then lets go, and names the tools that
started when it fails, so a failure says which tool ran and from where. Its
C++ twin failed once on a loaded release runner (2026-11-04), the release
starting a fourth tool where three were expected, with a cause never found in
the toolkit's press handling; the Rust's press is the button's own, worked
out each frame from the pointer, and the menu once open takes the release.

**A family is framed for its own tool, and its mark lights for any.** A
tool's button is framed in the accent while its tool runs, the toolbar's one
sign that a tool runs, and a
family's button is checked while its default action is - the tool its icon
shows. With another of its tools running (Delete Vertex, Circle 2 Points) the
button's mark takes the accent and its tooltip ends "Running now: Delete
Vertex", in the accent; its menu ticks which. Every frame paints the button
from its family's actions as they are then (`Flyout::family_running`), so
nothing has to tell the button that a tool started; the C++ button had to be
told, through its family's menu, and the bookkeeping is gone with it. Until
2026-09-30 no button showed such a tool running at
all, and then, for a day, the family's button was framed while ANY of its
tools ran - so with Delete Vertex running the Vertices button framed Insert
Vertex's icon, a plus on a polyline: the wrong tool, even the opposite one,
named by the one sign the toolbar has for a tool running. Rejected: that
frame; the running tool's icon on the button while it runs - the icon would
name one tool while a click starts another; and the last tool chosen
becoming the button's own ("Decided", below). The families are behind the
Draw toolbar's overflow arrow at the window's first size, and a tool running
behind that arrow lights it (below).

**The families follow their toolbar's icon size.** A button drawn each frame
is drawn at its bar's icon size, so nothing has to follow a change of it. The
tests below hold it all the same (a bar made at a size and one switched to
it), because the C++'s family buttons once kept the 33 px they were made at
after View > Toolbars > Large Icons, while the other buttons grew to 41.

**The mark is drawn in the screen's own pixels.** The triangle is filled a
row at a time, each a pixel wider, without antialiasing: a filled polygon
lost the pixels on its diagonal, and antialiased at five pixels its edge is
a smudge. The rows are the device's. Rows of the widget's pixels were whole
at 100% and 200%, but at 125%, the owner's display, and at 150% each became
one device row or two, and the staircase came out uneven (rows 1, 2, 3, 3,
5, 6 wide at 125%). Its legs are 5 px times the screen's ratio and its inset
from the button's last device pixel 2 px times it, each taken down to whole
device pixels - 6 px and 2 at 125%, 7 and 3 at 150% - so it is the same on
every button. Snapping the mark's own square to the device, as was done
first, made its size hang on where the button sat in the window: at 150%
Arc's came out 8 px beside the others' 7, and running, two of its rows ran
into the frame's rounded corner. Taken down, not rounded, since the mark
starts the pixel after the icon's square ends. The mark also ends a whole
device pixel before the first device column and row the button's 1 px
border touches. At 125% the border is 1.25 device pixels, and wherever the
button starts off the device grid - in the window, as on the Draw toolbar,
never in a grab - it falls across two columns; set back by the inset alone,
the mark met the first of them along both its sides, and while Circle or Arc
ran it read as a thickened blue corner of the frame rather than a triangle
inside it. Where the inset already leaves that pixel, as it does everywhere
at 100, 150 and 200%, nothing moves; at 125%, and at 175% for some places,
the mark moves a pixel in, into room the icon's square leaves - it then
meets the icon's square, as at 100%, without overlapping it. Rejected: an
inset of three device pixels at 125% wherever the button sits. Where the
button starts on a whole device pixel, as in a grab, the 6 px mark would
then cover the icon's last column, or would have to shrink to 5 px.

**The mark goes out when the pointer leaves.** The pointer leaves the button
over the family's menu to pick from it. The hover is worked out each frame
from where the pointer is, so a mark is lit only while the pointer is on its
corner. (The C++'s was lit by hover events, and a popup's open menu left it
lit in the accent - the colour of a tool running - after the menu closed and
the tool had ended, until the pointer next crossed the button; the test that
caught that drives the pointer onto the menu and holds the Rust to it.)

The alternatives were rendered and measured on 2026-09-30 with a scratch
prototype of the C++ window:
- a strip of its own beside the icon, as Undo and Redo keep - rejected for
  the families: in the vertical Draw toolbar it widens the bar by 10 px and
  moves the family icons 5 px left of the column of the others;
- the arrow as a 7 px corner and nothing else - a 7 x 7 target and no hold or
  right click;
- the arrow below the icon - every family button 7 px taller, 28 px more on
  a Draw toolbar that already overflows the window's first size;
- the menu on any click - two clicks for every circle, and a headless
  `--press` of such a button would wait in the menu.
What the choice costs: a press held is found less readily than an arrow,
which is why the corner and the right click open the menu too and the
tooltip names all three; and a right click on a family button no longer
shows the window's list of toolbars - anywhere else on the toolbar it still
does.

Undo and Redo stay split buttons: the button undoes one step and the arrow
drops the history ("Undo and Redo lists", below). A split button
(`ToolButtonLook::SPLIT`, `split_paint`, `split_button_at`) pads its right
side by 13 px, a pixel more than its 12 px strip (`split_strip`), which is
laid in the button's border rectangle at its right; the 8 px down arrow is
centred in it (`paint_fusion_arrow`, the shape the C++'s Fusion style drew),
so each arrow is 9 px from its own icon's ink, 13 from the next button's and
2 px in from the button's edge (each the columns between the two inks,
measured on the Edit toolbar at 100%). With the 4 px every button has added
to a 10 px strip it was 11 px from its own icon and 12 from the next, and the
row read as four things - undo, arrow, redo, arrow; with a 10 px strip and no
more it was 7 and 12, but the arrow ended a pixel from the button's edge, cut
off against the rounded highlight under the pointer (at 125%, its ink 37..47
of 49 device pixels; now 39..50 of 53). The strip has no hover shade of its
own: the whole button is shaded under the pointer, since a shade on the strip
alone, with the pointer on the icon, read as a band pressed. A 1 px divider
on the strip's left edge, shown under the pointer, was tried in its place and
dropped: the divider takes its pixel from the strip, and the arrow was drawn
differently at rest and under the pointer. Along the top the 13 px each cost
nothing.

**A view's kind switcher has its arrow beside its icon.** Every view's title
bar opens with a button that switches what the view shows (Plan, 3D,
section, elevation), a menu with an arrow beside the icon. The arrow is set
at the right of the button's padding box in a 13 px box (`INDICATOR`) and
the icon is centred in the contents; with only the 2 px every title bar
button has, the contents ran under the arrow and the icon's frame met it,
2 px over at 100% and 3 at 125%. So a title bar button whose menu opens on a
click is padded on the right by that box's 13 px
(`ToolButtonLook::TITLE_BAR_MENU`), and the icon, 4..17 in the 34 px button,
is four pixels clear of the arrow, 22..30. Only that button takes the look:
for a day in the C++ the padding reached every title bar button, and
Minimise, Float, Maximise and a view's Layers and Zoom Extents, fixed at
22 px, had 5 px of contents - their 14 px icons drawn as a dot and slivers -
on every panel and view, at every scale. No title bar button has a
held-press menu, and one that did would draw its arrow over its icon, which
`--check-toolbars` fails; it measures the docks' title bars as well as the
toolbars.
`title_bar_buttons::every_title_bar_button_draws_its_whole_icon_where_close_draws_its_own`
measures every button of a panel's and a view's title bars, menu or none,
against the bar's Close button, which no popup mode rule reaches; the
checks before it measured only buttons with a menu, and passed with every
other title bar button broken.

**The overflow arrow.** A toolbar too short for its buttons shows an
overflow button, 12 px deep (`EXTENSION_EXTENT`; its control name
`qt_toolbar_ext_button`, kept from the C++, whose tests and theme named it).
It has no padding inside its 1 px border (`ToolButtonLook::OVERFLOW`): the
4 px every tool button has left its arrow 2 px, a dot nobody could see, so at
the window's first size (1360 x 860) the Draw toolbar's Ellipse, Divide,
Measure and Vertices tools were behind a button that did not show. The
border stays, so that while the toolbar is expanded the button has the
accent outline every checked tool button has; taking the border too, as was
done first, left it a plain grey pill. The arrow is two chevrons in the
muted text colour, a pixel wide and without antialiasing, painted in the
device pixels they are shown in (`paint_overflow_arrow`), 7 x 8 px in the
button's 10 rows at 100%. It was first two pixmaps, 12 and 24 px,
antialiased: at 125% and 150% the 24 was scaled down, and the chevrons came
out in grey halos (21 colours at 125% against 7 at 100%), and at 100% the two
1.5 px chevrons all but ran into one shape. While a tool runs whose button the arrow hides - a
family with any of its tools running, or a plain tool's button framed - the
chevrons take the accent (`overflow_lit`, `crates/katana_window/src/tools/flyout_button.rs`): at the first size all
eighteen Vertices tools are behind the Draw toolbar's arrow, and with Delete
Vertex running nothing on the toolbars said a tool ran. Only a tool's
button lights it: Select, which heads the Draw toolbar, is checked while no
tool runs. It is worked out each frame (`overflow_lit`) from what the
toolbar's layout hides and what runs then, so a window shown short with a
tool already running has the arrow lit as it appears; expanded, the toolbar
shows every button, and the arrow is not lit.

Tested in `tool_family_buttons::*` - the mark, its press zone and
the icon apart at 16, 20 and 28 px icons, on a bar made at that size and on
one switched to it, and drawn where they say; the mark the same whole
staircase of device pixels at 100, 125, 150, 175 and 200% wherever the
button sits, and there, running, a whole device pixel clear of its frame
and off its icon at 16, 20 and 28 px icons (it met the frame at 125% with
the button 1 to 3 px in, and at 175% 1 px in); a family against a plain
button with the same icon, at rest,
under the pointer, running the tool its icon shows (framed, as the plain
button is) and running another (unframed, as the plain button at rest),
differing only in the mark's square, the mark in the accent while either
runs; the button painted again when any of its tools starts or stops; a
click on the icon, the mark, a held press, a right click and
the click `--press` gives; the mark lit with the
pointer on its corner and out once the pointer has left for the family's
menu, the pointer driven through the platform; the status bar's line on the
corner and off it; the tooltip as it is shown, for a family made by hand and
for the catalogue's, laid out as the tooltip lays it out, word-wrapped,
at every width from 60 to 400 px with no name
split - the test also asserts it reached widths where the names wrap, since
a document not yet laid out has no lines and passed the check with the
names' spaces plain - and naming another tool while it runs - and in
`split_buttons::*` (Undo's strip beside its icon and its arrow in
from the edge, by hand; its hover the button's),
`title_bar_buttons::*` (the kind switcher, by hand, and every
title bar button's whole icon, where its bar's Close draws its own),
`tool_bar_signs::*` (the check fails a split button's arrow and a
kind switcher's drawn over their icons) and `tool_bar_overflow::*`
(the arrow whole, in whole device pixels at 100 to 200%, lit while a tool it
hides runs, also when the window is shown short with the tool already
running, never for a checked button that is no tool, and outlined while
expanded). Every case holds at any screen
ratio: what is drawn is measured in the grab's device pixels, and
`qt_toolbar_signs_at_125_percent.<suite>` runs all five suites at the
owner's 125% (until 2026-09-30 six of these cases held only at 100%). In
the real window, `katana --check-toolbars` measures every icon-only button
with a menu on a toolbar or in a dock's title bar from its own rendering -
with an opaque icon against a clear one for where the icon goes, with its
menu against without for what the menu adds - and fails a run where the two
meet. It judges in device pixels: a family's mark starts the pixel after
its icon's square ends, and at 125% the two boxes taken back to the widget's
pixels, each rounded outwards, met at pixel 26 - four clashes nobody could
see. It measures once the steps have run, so a step sets up what it
measures: `qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_headless`
(the four families, Undo, Redo and the view's kind switcher as the window
starts), `qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_at_125_percent_headless`
and `qt_every_toolbar_menu_sign_is_drawn_clear_of_its_icon_with_large_icons_and_a_tool_running_headless`
(after View > Toolbars > Large Icons, with Delete Vertex running).

Decided, 2026-10-15: a family button does not remember the tool last chosen
from it, as some CAD programs' flyouts do. A click on it runs the family's FIRST
tool, always, so a person who has learned where Circle is has learned what it
does; a button that remembered would run one tool today and another tomorrow,
and its icon - the default action's - would change with it, so the toolbar would
not look the same from one session to the next, which `--check-toolbars` and every
screenshot assume. It is a feature with a decision of the owner's in it (and a
setting to turn it off), not a defect, so it stays as it is. The mark is not
mirrored for a right-to-left layout, which the window does not use.

**Every key reaches one thing.** `MainWindow::shortcut_clashes` gathers every
key a person can press - each action's key sequences, each menu's Alt letter
and each item's underlined letter within its own menu (two items with one
letter make the key cycle between them instead of choosing) - and lists those
that reach more than one thing, since a key that reaches two cannot be meant
for both. `katana --check-shortcuts` fails a run that has
any, and `qt_every_shortcut_and_menu_letter_reaches_one_thing_headless` runs
it. Ten underlined letters were changed to pass it (A&ttributes, Sym&bol
Library, In&verse, Toggle Pe&rspective and others; one of the ten was on
Load Customisation, an item that has since gone).
The tool actions built from the catalogue have no underlined letters; their
aliases are in their tooltips. Help > Keyboard Shortcuts lists the keys for a
person ("The Command Reference and the keyboard shortcuts", below).

`katana --screenshot` grabs the window headlessly so that the look can be
reviewed, and the same run can drive the menus and dialogs step by step:
`docs/headless.md`.

**Not done.** There is no light theme. The dialogs (corridor, plot) are
themed but plain. The views inside the workspace are not kept between
sessions (above, "How the window starts"); the dock chrome's hooks
(`minimised_names`, `minimise_named`) are ready for it. The Text Size is the
window's; a dialog that set a font of its own other than the fixed-pitch one
keeps it. `resources/icon_sheet.png` is regenerated by `katana_make_icons` and
carries every icon, the menu items' of 2026-09-26 included.

## The workspace: every view is a dock, and each has its own layers

The user's request of 2026-09-23 was for panels and views that dock, move,
minimise, close and come back from the menus, views that go out onto another
screen, and per-view control of which layers show. Until then the views were
tiles: the C++'s `ViewportContainer` placed widgets at the rectangles of a fixed split
(`katana_cad::viewport_layout`), so a view was identified by its POSITION. Every layout
change destroyed and rebuilt every widget, which lost the plan's zoom, every
section but the first and any half-picked clicks - and the application changed
the layout on its own, whenever a surface was built or a section cut.

**Each view is now a dock of the workspace's own dock area**
(`crates/katana_window/src/view_workspace.rs`), which fills the middle
of the main window. The area is an egui_dock `DockState`, laid out by
egui_dock's split arithmetic and drawn by the workspace with the window's
dock chrome; a tab strip is drawn only over a group of more than one view.
An
area of its OWN rather than the main window's, because an arrangement ("Four:
Equal") must move views and never the Layers panel, and a panel should not be
droppable between two views. Rejected alternatives:

* *Keep the tiles and add pop-out windows.* Two ways of showing a view, and the
  tiled half still rebuilds on every change.
* *egui_dock's own `DockArea`*: it draws a 24-point tab bar over
  every dock, where a view has only its 26-point title bar, with close and add
  buttons of its own, and its drags would put a view among the main window's
  panels, which the area of its own is there to prevent. The views and their
  state do not know what hosts them, so either could still be changed.

Not done: a view floats as a window inside the main window, and cannot go
out onto another screen, as the C++'s floating docks could; egui_dock floats
a tab inside its own window (`docs/rust.md`, "Decisions waiting on the
owner"). The Float button's tooltip still says it can.

**A view's state lives in `katana_cad::view_set::ViewSet`, not in its widget.** `ViewState`
holds the kind, the 3D camera, the plan zoom, the layers hidden in that view,
the reference layers hidden in it and its section. Views are identified by a
`ViewId` that is monotonic and never reused, like entity ids, so a queued event
for a closed view cannot find another. States are boxed, so taking a closing
view's state hands over the very object the set held. Changing a view's kind replaces
only the widget inside the dock, so a 3D view switched to plan and back keeps
its orbit. `ViewSet::most_recent(kind)` is what Plot, F9 and the Standard Views
act on: the active view when it is that kind, otherwise the one of that kind
the user touched last - so F9 still reaches the 3D view after a click in the
plan. The layout presets survive as arrangements: `katana_cad::viewport_layout::dock_splits(kind)` lists
the split steps that build each one, and a test asserts that
applying them to rectangles reproduces `layout_rects(kind)` exactly.

**Showing a view never replaces one.** Building a surface or importing meshes
used to split the window and turn a cell into 3D; cutting a section turned the
ACTIVE cell into a section, which was the plan view being drawn in whenever the
user had just clicked there to select the alignment. `ViewWorkspace::ensure_view(kind)` raises
the view of that kind used most recently, or opens a new one beside the active
view.

**Zoom Extents frames the active view only** - and the views linked with it
follow (below, "Linked views"); one view's zoom is not another's business
unless the user has linked them. After New, Open and an import every view is framed
(`zoom_extents_all`), because all of them are looking at a drawing that has
just changed under them; the link is framed once there, as below.

**Per-view layers extend THE visibility rule rather than adding a second.**
`katana_cad::selection::is_drawn` and `is_selectable` take a `&LayerOverrides` - the layers
one view hides - with NO default argument, so every caller has to say whether
it means a view or the document (`NO_LAYER_OVERRIDES`); a default would let the
next consumer ignore the view it draws in, which is how the 3D view once came
to ignore layer visibility altogether (audit REN-03). The overrides are
SUBTRACTIVE: a view can hide what the document shows, never show what it hides,
so the rule stays a plain conjunction and a layer switched off in the Layers
panel cannot linger in some forgotten view. Hiding a path hides everything
beneath it, the document rule's ancestor semantics (`docs/model.md`, "Layer
visibility and lock"), tested by
walking the path's ancestors as slices of the name - no allocation, and an
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
(`crates/katana_window/src/view_layers_popup.rs`) is where they are set: it filters that view alone,
and isolating a tree node hides everything but that node's path. The same
filter has lines - `VIEWS HIDE <id> <layers>`, `VIEWS SHOW <id> <layers>|ALL`
and `VIEWS ISOLATE <id> <layer>` (`docs/cad.md`, "The window's views: VIEWS
and ZOOM") - so an agent, a script and a test set up a design view hiding the
as-built layers beside an as-built view hiding the design ones
(`qt_a_design_view_and_an_as_built_view_hide_each_others_layers_headless`).
**The popup runs those lines; it edits nothing itself.** Each box and button
builds the VIEWS line a person would type (`view_layers_line`,
`view_references_line`, pure functions of the layers' names, held in
`view_layers_lines::*`) and hands it to the window's one executor through
`ViewWorkspace::set_view_layers_shown`, `isolate_layer_in_view`, `show_everything_in_view`
and `set_view_reference_shown`, so a click is logged as a typed line is, takes the
verb's own code path and is refused as the verb refuses; the popup then reads
every box from the view, so a refused line leaves each where the view is
(`layers_popup::*`). A plain name is a word of the line; a name with a blank is
quoted; a name with a comma, or one that is the verb's own word (ALL, LAYER,
REFERENCE), goes after `LAYER`, which takes every word after it whole
(`VIEWS HIDE 2 LAYER a,b`); the reference layers are `VIEWS HIDE|SHOW <id>
REFERENCE <id>...`, and `SHOW ... ALL` shows them with the layers. Hide Others is
two lines, VIEWS ISOLATE and then VIEWS HIDE of what the view had hidden
beneath the layer; the view is not a document, so nothing is half-done in a way
undo would have to know. *Rejected:* an `ISOLATE ... KEEP` word, to make Hide
Others one line (a new word for one button, and the two lines each say what they
do); a quote that makes one layer of a comma list (the verb is handed the words
without their quotes, so it could not tell, and a `LAYER` keyword needs no change
to the shared tokenizer); the popup keeping a direct path for the names no line can carry (a
second way, which is what this removed). Not carried by any line: a layer whose
name holds a double quote or a line break, which the tokenizer has no escape
for - the popup then logs why and leaves the box as it was.

### Linked views: a design view and an as-built view that move together

The owner's request of 2026-09-30: "sync views together so i can zoom in a
design view and as-built view zoom in too", with the control "as toolbar on
the view widget itself". A plan view is linked or not; every linked view
shows the centre and the scale (pixels per unit) of the linked view moved
last, each at its own size and through its own hidden layers - the design
view hiding the as-built layers and the as-built view the design ones, both
looking at the same place. The model is `katana_cad::view_set::ViewSet`'s link and
`crates/katana_cad/src/view_link.rs`; the verbs are `VIEWS LINK`, `VIEWS
UNLINK` and every `ZOOM` (`docs/cad.md`, "The window's views: VIEWS and
ZOOM").

**What moves the link.** A wheel notch, a middle-drag pan, a frame (Zoom
Extents from the bar, the menu or a line, the program's own framing, a
`ZOOM`) and a first paint's frame: each raises the plan view's
`PlanViewEvent::ViewMoved`, and the workspace's `view_moved` has the others
take its centre and scale in the same pass, with one repaint each. The
follow is STAR-SHAPED: a view moved by the link never reports that move -
`PlanView::hold_view` raises nothing - and the workspace's guard flag
makes sure of it, so no echo can run between two views. **A linked view
never refits on a resize**: a view framed by Zoom Extents refits its frame
whenever it is resized until the user moves it, which in a linked view would
change its scale behind the link's back, so every member forgets its frame
when the link moves it or it moves the link. Resizing a dock never breaks the
link (`view_links::a_linked_view_keeps_the_links_view_when_resized`, shown failing with
`hold_view` emptied). Each move has its test through the widget, shown failing
with its hook taken out: the wheel's, a middle-drag pan's - the other view
bit for bit where it was worked by hand, and reporting nothing itself
(`view_links::a_pan_in_a_linked_view_moves_the_other_bit_for_bit_and_the_other_never_reports_it`)
- and the first paint's, where a view linked before it was ever seen, and
named the one the others come to, moves nobody until its first paint frames
the drawing, and then leads
(`view_links::a_view_linked_before_it_was_ever_seen_leads_from_its_first_paint`). When
that first paint frames nothing - the view hides every layer with anything
on it - the view has no place of its own to lead to, and takes the link's
place instead: that of the member the user moved last that has framed, else
the first opened (`ViewWorkspace::view_moved`); with none framed yet it waits,
and the first to frame brings it along. It said nothing then, kept the
place every new view starts at, and a script's `VIEWS HIDE 2 design` and
`VIEWS LINK 1,2 TO 2` left the link showing two places while both views said
`linked=yes`
(`view_links::a_link_led_by_a_view_that_frames_nothing_at_its_first_paint_takes_the_others_place`).
Rejected: framing what the link draws there, as its Zoom Extents does - it
would throw away the place the user had put the other views at.

**Who comes to whom when views are linked.** `VIEWS LINK ids TO id` brings
the others to TO; without TO the joining views come to the link (its lowest
id), or with no link yet to the first id listed. The Link button always
sends TO: the view the user panned or zoomed most recently of the one clicked
and the link's members (`ViewSet::link_leader_for`, on a clock of the user's
moves, `ViewState::last_moved`), else a member, else the view clicked.
Clicking the two views' buttons in either order then keeps the zoom just
made (`view_set::a_joining_view_comes_to_the_view_the_user_moved_last`). Rejected:
"the joining view always follows", which loses that zoom whenever the view
zoomed is clicked second; and one click linking to an implicit "previous"
view, which surprises with three views open. The first click makes a link of
one, which waits for the second ("Linked, waiting"), in a DASHED accent
frame: in the solid frame of a working link it looked finished, and the
second click was not made (`theme::a_link_waiting_for_a_second_view_is_framed_dashed_not_solid`);
an unlink, a close or a change of kind that leaves one member dissolves the
link, and none of them moves a view.

**Zoom Extents on every view** (`zoom_extents_all`, after New, Open and an
import) frames the link ONCE, in the member moved last (else the lowest id),
on the union of what the members draw (`ViewWorkspace::link_drawn_bounds`);
the others follow. Framed one by one, each on what it alone draws, a design
view and an as-built view would come apart the moment a drawing opened
(`view_links::zoom_extents_all_frames_the_link_once_on_the_union_of_what_its_views_draw`).
`ViewWorkspace::zoom_to` frames the box it is given, once, in that same
member, and the others follow it. A linked view's own Zoom Extents frames
what it draws; when it draws nothing - the design view hiding the drawing's
only layer - it frames what the link draws instead: Zoom Extents on nothing
goes to the origin at a pixel a unit, as it always has for a view alone,
and a linked view took the as-built view there too, millions of units off
a survey (`view_links::zoom_extents_in_a_linked_view_that_draws_nothing_frames_what_the_link_draws`).
The rule lives in `ViewWorkspace::zoom_extents(id)`, where every Zoom Extents
of one view comes: `ZOOM EXTENTS`, the bar's button, Ctrl+E, and a plan
view's middle double-click, which the widget hands to the workspace
(`PlanView::zoom_extents`, as a GPU 3D view hands its own) - it went
to the widget's own Zoom Extents, around the rule
(`view_links::a_middle_double_click_in_a_linked_view_that_draws_nothing_frames_what_the_link_draws`).

**The Link.** A plan view's Link is the first of its bar's tools, before
Layers and the zoom tools (the whole bar is under "The zoom tools on the
bar" below). `ViewLinkButton` is checkable and never a menu
(a title bar's menu is a modal loop, which a headless press waits in for
ever): an open chain in the neutral colour while the view moves alone, a
closed chain in the accent inside an accent frame while it is linked, and a
tooltip that names the views it moves with. A click builds `VIEWS LINK <id>
TO <id>` or `VIEWS UNLINK <id>` and runs it through the window's command
runner, so it is logged as the line it is; every Link button is drawn from
the link as it is, so a refused line leaves the button as it was
(`view_links::a_refused_link_leaves_the_button_as_it_was`). The
button shows on plan views only and follows the view's kind; each tool
always on the bar costs 23 px of the dock's least width (22 px and the bar's
1 px spacing), which the Link button's test holds to
(`view_links::the_plan_docks_minimum_width_grows_by_the_link_button_alone`). A checked
title-bar button is framed in the accent as a checked toolbar button is, by
the one button painter ("One painter for every tool button"); in the C++ it
once had no frame at all, its theme's rule for chrome buttons taking the
checked one's away. View > Link This
View (`viewLinkActive`, Ctrl+Shift+L; K, as Viewport Layout has L) does the
Link button's line for the ACTIVE view, and is checked from its link; it is
disabled while the active view is of a kind that cannot be linked. It took
the plan view used last instead, so with a 3D view active it linked a view
out of sight and showed itself checked for the 3D view
(`qt_link_this_view_is_disabled_while_a_3d_view_is_active_headless`). View >
Unlink All Views (`viewUnlinkAll`) runs `VIEWS UNLINK ALL`; both items are
run by a test through the window
(`qt_the_view_menus_link_items_link_the_active_view_and_unlink_all_headless`).
Link This View and Show the Selection on Hidden Layers are toggles, and
their check was once never seen: the C++'s theme drew a menu item that had
an icon with no check mark at all, so
they - and Grid, Object Snap and every other toggle with an icon - looked
the same on as off, pixel for pixel, and Link This View on a linked view
unlinked it with nothing on screen to say it was on. A checked item's icon
now wears the accent frame a checked toolbar button wears
(`menu_icon_cell` in `crates/katana_window/src/theme/style.rs`,
`theme::a_checked_menu_item_with_an_icon_is_framed_and_an_unchecked_one_is_not`). Rejected:
a label that changes with the state ("Unlink This View") - a checkable item
keeps one label and shows its state by its check, as Grid does, and a label
that turns into its opposite reads as a second item.

**The zoom tools on the bar.** A plan view's bar is `[kind] Plan 1 ...
[Link][Layers][Zoom In][Zoom Out][Zoom to Selection][Zoom Extents]
[_][float][max][x]`; a 3D or elevation view's the same without the Link,
and a section's has In, Out and Extents. Each is a plain button that runs
its ZOOM line through the command runner - `ZOOM IN view=<id>`, `ZOOM OUT
view=<id>`, `ZOOM SELECTION view=<id>`, `ZOOM EXTENTS view=<id>` - so a
click is logged, the views linked with it follow, and Zoom to Selection with
nothing selected says `matched=0` and moves nothing. A button the view's
kind would refuse is not offered: the bar shows what ZOOM takes there, and
follows the view's kind, by ZOOM's own rule (`katana_cad::view_verbs::zoom_takes`). The View
menu has the same four for the active view (`viewZoomIn`, `viewZoomOut`,
`viewZoomSelection`, and Zoom Extents, which now runs `ZOOM EXTENTS` too),
enabled by the same rule for the active view's kind - a section frames no
scope, so its Zoom to Selection is disabled
(`qt_the_view_menus_zoom_to_selection_is_disabled_for_a_section_headless`) -
and Zoom To (below). The menu items and the bar's buttons are run by a test
through the window, each line in the log and its view moved as asked
(`qt_the_view_menus_zoom_items_and_the_bars_buttons_run_their_lines_headless`,
`view_links::the_bars_zoom_in_and_out_run_their_lines_and_zoom_the_view`). Each tool's tip
says what it does in that kind of view, and only a view that can be linked
says its linked views follow: one tip per tool promised a link to a section
and a 3D view, which cannot have one
(`view_links::a_zoom_tools_tip_says_linked_views_follow_only_where_the_view_can_be_linked`).

**In a 3D or elevation view** Zoom In and Out are its wheel turned over the
middle of it: `RenderView::zoom_by` turns the notches that make the
factor at the wheel's 1.15 a notch, over the pixel the view's axis passes
through (`Camera::ray_through_pixel` samples pixel centres), by
`RenderView::zoom_at_pixel`, the function the wheel itself calls - so a
zoom from the bar can never be a different zoom from the wheel's, and it
goes wherever the wheel goes. Twice as close: the eye half as far from the
target, which stays the middle of the view
(`view_links::a_three_d_views_zoom_in_is_twice_as_close_about_its_middle`). Out no farther
than where the scene is a pixel across - its diagonal D at D (H / 2) /
tan(fov / 2), H the view's height in the camera's pixels - as a plan view's
scale stops at its least: `ZOOM 1e-300` took the eye 4.7e302 units off,
where the camera's arithmetic had no digits left for where it looked, and
the target it keeps in the middle moved from (87.5, 68.69) to (175, 137.38)
(`view_links::an_enormous_zoom_out_of_a_three_d_view_stops_where_the_scene_is_a_pixel_across`).

**Zoom to Selection frames what the view shows of it**, in every kind of
view, by one rule, the verb's (`shown_of` in `view_verbs.rs`): of what the
scope took, the entities the view draws - by its own hidden layers as well
as the drawing's - and, with its ghosts on, the selected ones it ghosts (a
plan view ghosts no label). A plan view frames their box; a 3D or elevation
view the box its scene puts them in, draped and on the datum
(`SceneBuilder::overlay_bounds`, the selection overlay's own emit over the
ids, so a 3D view has no second rule for where an entity stands). The same
for ZOOM on any scope. A plan view framed the box of ALL of the selection:
in the as-built view with its ghosts off, a selected design feature took the
view - and the design view linked with it - to empty ground, where nothing
was drawn or highlighted, while its tip said nothing would move and a 3D
view pressed the same way stayed put
(`view_links::a_plan_views_zoom_to_selection_moves_nothing_where_it_shows_none_of_the_selection`,
`qt_a_plan_views_zoom_to_selection_moves_nothing_it_does_not_show_headless`,
`view_verbs_test::a_view_that_shows_none_of_what_the_scope_took_moves_nothing_and_says_so`,
`view_verbs_test::a_three_d_view_and_a_plan_view_frame_the_selection_by_one_rule`,
`view_links::zoom_to_selection_in_a_three_d_view_frames_what_it_draws_of_the_selection`). The
framed view's record ends `shown=`, how many of what matched it shows; a view
that shows none of it does not move, and its record after the scope's says
`shown=0 moved=no` - the scope's record alone read as though it had framed
something - and Zoom To says "1 matched, but view 2 shows none of them:
nothing moved." Rejected: a tip for the plan view that said what it did -
one button and one verb would then have followed two rules by the kind of
view, and a frame of empty ground shows the person nothing.
Select by ID's zoom and a style manager's Select Users run the active plan
view's Zoom to Selection line (`ViewWorkspace::zoom_to_selection`), where they
framed the view directly, outside the log - each shown by a test that the
old direct zoom fails: the line's reply read from the window's log
(`qt_select_by_id_selects_and_frames_headless`), and the line the window's
executor was handed
(`customisation_workbench::showing_what_uses_a_thing_selects_it_and_frames_it_in_the_active_plan_view`).

**A view not yet painted.** A view frames what it draws at its first paint,
and until then holds the place every new view starts at: a plan view the
origin at 10 px a unit, a 3D view a target at the origin 100 away. A script
runs its lines back to back with no paint between, so `VIEWS OPEN plan` then
`ZOOM IN` zoomed about the origin and marked the view framed - it never
framed the drawing, and a link took its other views there too. A zoom now
does what the first paint would first (`PlanView::frame_if_unframed`;
`RenderView::zoom_by` frames a 3D view)
(`view_links::a_zoom_of_a_view_not_yet_painted_starts_from_what_it_draws`,
`view_links::a_three_d_view_not_yet_painted_frames_the_scene_before_it_zooms`,
`qt_a_script_that_opens_a_view_and_zooms_it_zooms_about_what_it_draws_headless`).
And first the layout still pending is done (`ViewWorkspace::zoom_view`): a
new view's bar fits its tools to the width the split gave it, a tool it
drops queues one more layout, and that moves the split by a pixel or two -
in the headless window, `tests/qt_widgets/data/scripts/open_and_zoom.kcs`
framed the new view at 330 px, scale 5.544, where it is seen at 328 px,
scale 5.5104, so the reply's area was not the one shown
(`view_links::a_zoom_straight_after_a_view_opens_is_framed_at_the_width_the_view_is_seen_at`,
over the window widths at which the tools come and go).
A view's record says `framed=no` until it has framed (`docs/cad.md`): VIEWS
OPEN's record gave the placeholder as though the view looked there.
Rejected: framing a plan view as it opens - a view opened on an empty
drawing would then never frame what is drawn next, which the first paint
does.

In, Out and Zoom to Selection are OPTIONAL tools (`DockTitleBar::add_tool`
with a priority): a bar shows them, highest priority first, only while they
fit and leave the title the room to be read whole - its text's width and
`DockTitleBar::TITLE_GAP` (6 px, the gap before the window buttons), never
more than `TITLE_ROOM` (56 px); tools of one priority come and go together.
Zoom to Selection (2) outlasts In and Out (1), which the wheel does as well.
A flat 56 px left "Plan 1" a 69 px gap at the owner's two-up width, where In
and Out needed 46, so both bars lost them; room by the title's own width
fits all six there. They are never in the bar's least width
(`DockTitleBar::minimum_width` leaves them out), so a view can always be narrowed
past them - each tool always shown costs 23 px of the least width, and three
more would have stopped a Quad view's bars from narrowing to their docks. No
overflow menu: every optional tool is in the View menu too, and a menu opened
from a title bar is a modal loop a headless press would wait in
(`view_chrome::the_title_bar_shows_optional_tools_only_where_they_fit_and_no_two_overlap`,
`view_chrome::optional_tools_never_widen_the_docks_minimum`).

**A bar is measured afresh every time.** The bar keeps no layout to go
stale: `DockTitleBar::fit` works the tools that fit out from the width each
time, and `DockTitleBar::minimum_width` from the tools always shown. A stale
count is what the tests guard against, since the C++'s bar had one - it went
on counting a tool just hidden - and it showed twice. The fit: a 3D view
turned plan showed three tools where a plan opened at that width showed four
(`view_chrome::a_view_turned_into_a_plan_shows_the_tools_a_plan_opened_at_its_width_shows`).
And the least width the workspace lays views out by: a view split just after
its bar dropped tools kept a least width that counted them, 308 px of a
554 px row where half was 277, and the Quad layout took 34 and 33 px from the
side panels for it
(`view_chrome::opening_a_view_splits_it_into_equal_halves_whatever_tools_its_bar_shows`,
`view_chrome::quad_puts_the_views_in_quarters_whatever_tools_their_bars_show`).
Measured after the fix, in the 1360 px window, the Quad layout's side panels
were 11 and 10 px narrower than before the link existed - the Link button's
23 px of the plan dock's least width - where they had been 34 and 33 px
narrower.

**Zoom To** (View > Zoom To, `zoomToDialog`) frames what a scope takes - the
shared "Apply to" and "Only those that match" (`ScopeFilterWidget`, prefix
`zoomTo`) - in a chosen view (`zoomToView`: the active view, then every open
plan, 3D and elevation view by its title and id - the kinds that frame a
scope, by `katana_cad::view_verbs::zoom_takes`; a section was offered and ZOOM refused it). It
builds `ZOOM <scope words> [view=<id>]` (`zoomToLine` shows it as the
controls change), runs it through the command runner (`zoomToRun`) and says
what ZOOM answered as a sentence (`zoomToStatus`: "2 matched, framed in view
1; view 2 followed.", `ZoomToDialog::spoken_reply`), a refusal in the error
colour, as Online Data words its status. The raw records - long reals among
them - wrapped over five lines there, and the dialog, grown only to its
least height, pressed its type grid until the captions lost their
descenders; they are in the command log. Non-modal and kept, as Select by ID
is (`qt_zoom_to_dialog_runs_its_line_headless`,
`qt_zoom_to_frames_the_scope_in_the_view_chosen_headless`, which chooses a
plan view and a 3D view; `zoom_to::the_status_says_what_zoom_answered_as_a_sentence_and_a_refusal_in_the_error_colour`).

**ZOOM typed while a tool runs** - Z, ZOOM, 'ZOOM, 'Z and whatever follows -
is the view's, not the tool's, as AutoCAD resumes LINE after 'ZOOM (and
'ZOOM with no tool running is ZOOM, as at AutoCAD's prompt): the tool
host hands it to the workspace (`ToolHost::on_transparent`), which runs it as
its ZOOM line through the command runner, and the tool stays at its step
with its prompt (`view_links::a_transparent_zoom_typed_inside_line_zooms_and_leaves_line_at_its_step`).
The line names the view the tool runs in, `view=<id>`, whichever view is
active; a `view=` typed is kept. Without it the ACTIVE view was zoomed: a
click on another view's bar or drawing, to look at it, made that one active,
and a Z typed at Line's prompt zoomed it while the view Line picks its next
point in stayed where it was
(`view_links::a_zoom_typed_at_a_tools_prompt_zooms_the_view_the_tool_runs_in_not_the_active_one`).
Which words are ZOOM is the interpreter's reading - its alias table and its
apostrophe rule, `CommandInterpreter::verb_of` - not a list of the window's,
so an alias added there reaches a line typed inside a tool too
(`command_verb::the_verb_of_a_line_is_the_interpreters_own_reading`).
The command line leaves the line's echo to the runner
(`ViewWorkspace::runs_transparently`), so it is logged once
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
view's scale differently, so they drift over a session. Views telling each
other directly, and document notifications: a notification repaints every
plan view's whole kept drawing, and views telling each other would be a
second way of saying who follows whom, outside the tested model. A toolbar
drawn over the drawing, or a second row under the title bar: the first
covers geometry and catches picks, the second costs 26 px of height in
every view, while the title bar had room. Link groups (several links, each
with a colour): one link is what design against as-built needs, and the
model changes cheaply when groups are asked for.

**Not done.** Only plan views link: a 3D or elevation view needs its camera
in logical pixels first (the software view counts device pixels, the GPU
view logical ones), a section its pan and zoom in `ViewState`. The link is
not kept between sessions, as the workspace is not. A checked Link and a
Layers button that filters wear the same accent frame side by side, and the
unlinked chain is the faintest icon on the bar; a look of their own for each
is polish not yet done. The Link is always on a plan view's bar, so a plan
dock's least width is 23 px more than another kind's: six views, four of
them in one row with two plan views, need 1388 px where the window has 1360
(1365 px opened in another order, the section's and the elevation's titles
cut to "Sect..." and "Elev..."), and the Properties panel goes to its least
(five views fit). On a narrow plan bar the title's end can stand 2 to 4 px
from the checked Link's frame: the optional zoom tools keep `TITLE_GAP` (6
px) clear of the title, and the tools always shown do not. Letting the
Link drop off a narrow bar as the zoom tools do was rejected for now: the
checked Link is what says a view is linked, and a linked view would not show
it. Assessed again 2026-10-15 and left: linking a 3D or elevation view is a
change to both renderers' camera units, not a small one. The Layers popup runs
`VIEWS HIDE`, `SHOW` and `ISOLATE` for its rows now ("Per-view layers
extend THE visibility rule", above).

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
  drawn faint, and never picked, snapped to, given grips or plotted there
  (`view_layer_rule::a_ghost_is_neither_picked_nor_snapped_to_nor_given_grips`). A feature selected in
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
  off" - nor an entity made invisible.

**One rule, beside the drawn rule.** `katana_cad::selection::is_ghost(model, entity, view)`
(`selection.rs`) is true where `view` alone hides an entity the document
draws; the plan painter, the 3D scene and a section ask it and nothing else.
A section asks both of its questions of a crossing - does this view hide
it, and how is its selection marked - of the layer its entity is on NOW
(`SectionView::layer_of`): the hidden rule read the layer the cut
found, so the design line moved onto the as-built layer the section hides
was drawn as a plain crossing and as a ghost at one station, and a line
moved off it was counted hidden and drawn selected
(`view_selection::a_section_hides_and_marks_a_crossing_by_the_layer_its_entity_is_on_now`).
It was written three ways - the scene's by `is_drawn`, the painter's inline,
the section's a third way that read the layer the cut found and never the
entity - and the copies had drifted: a selected entity made invisible, or
its layer switched off in the drawing, went on being marked in a section,
as selected or as a ghost, where plan and 3D drew nothing
(`view_selection::a_selected_feature_the_drawing_hides_is_marked_in_no_view`). A
section now reads the crossing's entity as it is now; a crossing whose
entity the drawing hides keeps the plain dashed line every crossing had
before a section knew of the selection. Its surfaces' palette has no orange
any more: the second colour, the usual design surface's, was CIE76 22.9 from
the selection's - nearer than the BOUNDARY yellow that had the 3D selection
recoloured - and a selected crossing's dot sat among that surface's orange
vertex dots; the nearest is now the yellow, 50.9.

**No grip on a ghost.** A view offers grips only on what it draws:
`katana_cad::drawing::grips::grips_of_selection` takes the view's `LayerOverrides`, with no default,
as `is_selectable` does, and the plan view's grip controller follows the
view's layers, which change with no document notification (it compares them
with those it last built from). Before, every grip of a selected entity was
drawn in a view that hid its layer, and could be dragged: in the owner's
case - a design line and an as-built line that cross - a drag from the
crossing in the as-built view moved the design line that view does not show
(`view_selection::a_ghost_offers_no_grip_so_a_drag_where_it_crosses_the_as_built_line_edits_nothing`,
`grip_commands::a_view_offers_no_grip_on_what_it_hides_and_the_others_keep_theirs`). A
headless run reads what a plan view drew, ghosts and grips among it
(`?PlanView2`, `docs/headless.md`), so the check that a view with its ghosts
off shows no trace of the selection looks at the drawing, not only at the
view's record (`qt_a_view_with_its_ghosts_off_shows_no_trace_of_the_selection_headless`).

One colour and every size are in `crates/katana_cad/src/selection_style.rs`,
each with its reason. The 3D selection was (255, 190, 60), beside the
BOUNDARY layer's (255, 213, 79) and the top of the elevation ramp, and apart
from the plan's; the sheet editor's blue stays, since it selects sheet
items, not entities.

**Each view has its own switch**, on when the view opens
(`ViewState::selection_ghosts`): "Show the selection on hidden layers"
(`ViewLayersShowSelection`) in the view's Layers popup, beside the layer
filter it belongs with, and `VIEWS SET <id> ghosts=on|off`, which the box
runs through the command runner, so a click is logged as its line
(`view_selection::the_layers_popups_box_runs_views_set_through_the_runner`,
`qt_the_layers_popups_box_turns_a_views_ghosts_off_headless`). View > Show
the Selection on Hidden Layers (`viewSelectionGhosts`; D, every other letter
of it being taken in the View menu) is the active view's switch by the same
line, checked from the view as the active view or its settings change
(`qt_the_view_menu_turns_the_active_views_ghosts_off_headless`). VIEWS
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
(`SectionView::set_document`).

**The casing in 3D is a draw list of its own** (`SceneLayers::selection_casing`),
drawn just before the core's and writing no depth, so the core covers it
wherever they overlap. In one list with the core the software view was
right, by the draw lists' depth bias, but the GPU view - which pulls a mark
nearer by its width and reads no bias - drew the wider casing over the core,
and a selected line was a dark line (the control in
`gpu_layers::a_selection_core_drawn_after_a_casing_that_writes_no_depth_keeps_its_colour`).
A frame's depth range takes in the selection's box too, because a ghost lies
where the view draws nothing (`scene_frame::the_depth_range_takes_in_a_ghost_outside_what_the_view_draws`);
what a view frames is unchanged.

Rejected: ghosting layers the drawing hides (a switched-off layer would
linger in every view); a halo in plan (the dashed orange already reads
apart from every layer colour); the ghost switch on the view's bar (the width
above); the casing by draw order in one list (the GPU's pull decides, not
the order).

Not done:

- A selected line buried under the ground in 3D is hidden by it, as any line
  is; an x-ray pass over everything is the next step.
- The 3D ghost's (130, 88, 32) was mixed for the empty ground and reads as a
  brown terrain line over the elevation ramp, whose colours it sits among;
  plan and section ghosts are dotted orange. A dotted ghost in 3D needs the
  scene to dash an overlay line, which it does not yet.
- With many entities selected at an overview scale, the 3 px core over the
  5 px casing merges parallel features a few metres apart into one orange
  mass in 3D and elevation; a thinner casing past a count or below a scale
  is not done.
- A section's legend is drawn over a selected crossing with no backing.
- A section draws a crossing whose entity the drawing hides (switched off
  after the cut) as a plain dashed line; only the selection's marks follow
  the drawing's rule, since the crossings are the cut's.
- `VIEWS SET <3d id> ghosts=...` rebuilds the 3D view's terrain as well as
  its overlay (`ViewWorkspace::view_settings_changed` calls
  `RenderView::invalidate_scene`, one scene build, 60 to 115 ms on the
  229k-triangle benchmark surface), though only the overlay reads the switch.
  Narrowing it belongs with the render view's rework on the 3D zoom branch,
  where the change would otherwise conflict.

### The dock chrome: one title bar for panels and views

The same request asked for panels and views that minimise, float, maximise
and close. A plain dock's title bar has no Minimise, no Maximise and nowhere
for a view's own controls (its kind, its layers). `DockTitleBar`
(`crates/katana_window/src/dock_chrome.rs`) is the whole bar of every
dock: `[icon] title ... [tools] [_] [float] [max] [x]`, 26 points tall - one
row of 22-point buttons with 2 above and below - with a view's kind switcher
where the icon is and its Layers button among the tools. The views' dock
area is egui_dock's: the workspace draws each tab through
`DockChrome::dock_ui` and applies what its buttons asked once the area has
been drawn (`DockChrome::apply`), since egui_dock holds its `DockState`
while it draws. A view is dragged by egui_dock's own tab, not by this bar
(where the C++'s was dragged by its bar); a double click on the bar is the
Float button. The four panels are laid out by the main window itself
(`crates/katana_window/src/main_window/layout.rs`), at the C++ window's
geometry, and wear the same bar.

**One owner.** One `DockChrome` per window holds the tray and the
bookkeeping, and the panels (`MainWindow`) and the views (`ViewWorkspace`)
share it: a panel and a view minimised into two trays would be two ways of
doing one thing. It knows nothing of views or panels beyond the `DockRole` it
is told (`Panel`: Close hides it and the menus bring it back; `View`: it can
be maximised, and one view is active), and of a tab nothing but its dock's
control name.

**Minimise is a tray, and why.** A floating dock is a window inside the main
window, with the window's shortcuts and no taskbar button of its own, and a
docked panel is not a window at all. So Minimise HIDES the dock and puts a
button for it on `MinimisedToolBar` at the bottom of the main window (its
buttons `Restore<dock>`), and the button puts it back exactly where it was:
in its place and tab when docked, at its rectangle when floating. Docked,
the place is egui_dock's tree: the layout as it was when the dock went -
every split and its share - is kept with it and given back whole when the
tabs open then are the tabs open now; otherwise the dock is put in the first
place there is.

**Rejected: rolling a dock up to its title bar.** Measured offscreen in the
C++'s 1360 x 860 window: Properties alone in the right area, rolled up, took
the whole column with it - 270 x 770 became 131 x 26, and the column's space
went to the drawing sideways; Layers rolled up above Reference Data narrowed
the left column from 300 to 135 px, and it stayed narrow after unrolling.
Minimise does the job for docked and floating docks alike and moves nothing
else.

**Maximise (views only).** A docked view hides every other docked view of its
window until it is restored, which gives back the sizes they had; a floating
view fills the window it floats in. A maximise is a temporary state, not a
layout: the workspace undoes every docked maximise before it opens or
arranges views (a split made while the others were hidden would be made
against the wrong neighbours), a tray restore of a docked view ends a
maximise in its window, and everything is unmaximised before a layout is
saved.

**The side columns own the bottom corners.** The panels run the full height
of the window and the command line sits under the drawing only, between them
- the usual CAD arrangement (`crates/katana_window/src/main_window/layout.rs`:
the command line at 350,637 660 x 150 under the views, between the Layers
column and Properties).

**The active view is always one on screen.** Zoom Extents and the view
menus act on the active view (and `most_recent(kind)` prefers it), and its
title bar carries the accent, so an active view nobody can see is a menu
acting on nothing visible. Pressing a view's title bar or any of its buttons
makes it active. Minimising or closing the active view hands on to a view
that is ON SCREEN - floating, or a docked view showing in the workspace's
rectangle - not merely one that is not hidden, because a tab page behind the
current one is not hidden. The one that takes over is the first on screen in
the order the views were opened; the most recently used would be better, but
`katana_cad::view_set::ViewSet` keeps its recency order private (a public
`ViewSet::recent()` is the fix). `open_view` also activates the new view
when the active one is hidden.

A split gives the new view and the old equal halves (297 px each of 600 in a
stacked split of a view already side by side, where the C++ once gave 397
and 197), held by `view_chrome::opening_a_view_splits_it_into_equal_halves_whatever_tools_its_bar_shows`.

Tested in `crates/katana_window/tests/workspace/view_chrome.rs` (`view_chrome::*`),
each test shown failing without its fix. The window tests build the real
window (`crates/katana_window/tests/main_window/`), but no case of them is
written yet for the four panels' bars, their exact minimise and restore, F9
from a floating panel and a floating maximise, which the C++ tested only in a
hand-run harness. Not done: a Window menu; keeping the VIEWS between
sessions - the panels' places are kept (`MainWindow::save_session` and
`restore_layout`), and which panels are minimised is not yet
(`minimised_names` and `minimise_named` are ready for it), and the saved
layout does not see the views' docks, which are the workspace's own area's,
so a maximised view needs no `unmaximise_all` before it; a layout that kept
the views would; a floating active view left minimised during an arrange
stays hidden and active, since only the tray button (`DockChrome::restore`)
restores it. The Survey
Point Manager dock (`SurveyPointsDock`) wears the chrome now that
the window's dock chrome hands it over, and the workbench has the chrome
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

`PropertyTreePanel` (`crates/katana_window/src/property_panel.rs`) is a filter box
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
`katana_cad::property_outline::property_outline` - the reading `PROP TREE` replies with, so the panel and
an agent cannot disagree (`docs/cad.md`, "Properties as a tree: PROP TREE") -
when a person opens it, and made a page of 200 rows at a time
(`PROPERTY_PAGE`); a "Show 200 more" row at the end of a level makes the next
page when clicked or chosen with Enter. The panel
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

The model keeps a rule from a trap found in the C++: a level's first read
(`PropertyTreePanel::fetch_more`) only adds rows and removes none, and the
next pages come from the Show more row after the click is over, never from
inside the drawing of the row being opened (in the C++, a fetch there that
removed the Show more row wrote past the view's items, which valgrind found in
`property_panel::a_long_string_is_made_a_page_at_a_time_with_a_show_more_row`).
`property_panel::opening_a_long_level_as_a_person_does_reads_its_first_page_only`
fails with the old rule, because every page was then made at once. A row's
tip is plain text, its name on the first line, since egui's tooltips are not
rich text. The rest is `crates/katana_window/tests/host/property_panel.rs`.

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
one. Its constructor is unchanged, and `selected_layer()` and `select_layer()`
are public for a caller that opens it on a layer. Since 2026-09-24 the
window keeps ONE instance, made the first time Format > Layers is chosen and
shown, raised and reused after that (`formatLayersDialog`), as the Format
workbench keeps the other managers; it is non-modal, so a headless run opens
it like any manager (`--dialog formatLayers`), and `--layer-manager` still
grabs it on its own. Selecting another row drops unsaved form edits without
asking, as the style manager does.

Moving needed no new command. A layer name is a path ("design/surface/tin1"
- see `layer_path.rs` for why the tree is derived from the names rather
than stored), so moving a layer under another parent is exactly renaming it,
and `rename_layer` already carries the subtree and the entities on it as one
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
layer paths (see `layer_path.rs` for why that tree is derived and not
stored).

The decision worth keeping is `<varies>`. The dialog acts on the whole
selection, and shows a value only where every selected entity agrees on it;
a property only some of them carry counts as a disagreement too. The
alternative - showing the first entity's value - looks tidier and is a trap:
pressing Save would then write that value over the others without anyone
asking for it. Showing `<varies>` means a user who saves has said what they
want all of them to be.

Values are formatted by `katana_entity::entity::to_string`, which is in `entity` and not in
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
Coding shows the same code manager - the same action
(`SurveyServices::code_manager`), so two menus cannot drift apart - then the
window's Apply Survey Codes (`applySurveyCodes`), which is also on the Survey
toolbar, and Process Linework (`surveyLinework`): the codes' editor, and the
two things done with them, in the order they are done.

**A customisation file is loaded in File > Settings.** Format had a section
Customisation Files, and Survey > Survey Coding the same two items: Load
Customisation and Replace Loaded Customisation (`loadCustomisation`,
`replaceCustomisation`), each a file dialog over the style libraries and
survey code files of another program. Katana no longer reads those files, and
the two items went with them rather than be kept as doors to a format that
is gone - and, on 2026-10-07, what was left of them: the two members of
`CustomisationServices` that carried the actions, which only a window test
still assigned, and the Survey Code Manager's tip, which said "the loaded
survey code files" and says "the survey codes of the session's customisation"
(`general_texts::no_format_menu_item_or_toolbar_button_names_another_program_in_its_text_or_tips`
holds the menu to both). A Katana customisation file (`docs/customisation.md`) is loaded in
the window by

- File > Settings, Import - a path field, Browse, and Replace instead of
  merging beside it ("Settings", below) - which runs
- the line `CUSTOMISE <file>...` (merge) or `CUSTOMISE REPLACE <file>...`,
  the interpreter's verb: typed, or run by a script or the Run Script dialog;
- `katana --customise <file>...` at start-up.

The managers' own Import and Export buttons work on a Katana customisation file
too, each on its own part of one: the Symbol Library's Import Definitions...
runs `CUSTOMISE DEFINITIONS "<file>"` and its Export Selected... and the
Survey Code Manager's Export Codes... run `CUSTOMISE EXPORT`, so each is in the
command history and the file is written by the verb's one writer; the Survey
Code Manager's Import Codes... merges a file's rules into its buffer, for
review before Apply ("Symbol Library" and "Survey Code Manager", below).

Rejected: an item in Format that opened a file dialog on `CUSTOMISE "<file>"`.
Settings' Import is that, with a path field a headless run can fill and the
Replace choice beside it; and the customisation as a whole - load a file,
write one, reset, keep - is not one of Format's tables.

**Apply Survey Codes is the `CODE` line.** `MainWindow::apply_survey_codes` runs
`CODE SELECTION` when something is selected, else `CODE DRAWING`, through the
window's one executor ("One executor", below): echoed in the log, kept in the
history, one undo step, and answered in the verb's words - what the scope
took, the property the codes were read from, what was matched and created.
It used to call `katana_cad::survey_coding::apply_survey_codes` itself, with sentences of its own, a
box when no codes were loaded, and a lookup of the standard colours only -
so a colour the customisation defines was applied by the typed `CODE` and
not by the menu item. With no codes loaded the item is now refused as the
line is, in the log - `no survey codes are loaded; CUSTOMISE <file> loads a
Katana customisation file` - and no box opens
(`qt_apply_survey_codes_with_no_codes_loaded_is_refused_in_the_verbs_words_headless`;
`qt_apply_survey_codes_runs_the_code_line_on_the_selection_or_the_drawing_headless`
has the two lines). The import wizard triggered this action once, as a second
step, with the points it had just drawn selected for it. It no longer does:
its import codes and strings its own points inside its one command
(`docs/survey.md`, "The import wizard").

**Process Linework is the `LINEWORK` line.** `MainWindow::survey_linework` runs
`LINEWORK SELECTION` when something is selected, else `LINEWORK DRAWING`, the
same way and for the same reasons: echoed, one undo step, and answered in the
verb's records - what the scope took, each line strung, each reason a point
is in none (`docs/survey_coding.md`). Joining coded points into lines was one
tab of the Survey Code Manager and nothing else, so the second half of
coding a survey had no menu item at all. The verb with no scope word makes
the same choice itself; the item writes the word, so that the echoed line
says which this click meant
(`qt_process_linework_runs_the_linework_line_on_the_selection_or_the_drawing_headless`).
It is in the menu and not on the Survey toolbar, which is as it was.

**`CustomisationWorkbench`** (`crates/katana_window/src/customisation/customisation_workbench.rs`)
is built like the Survey workbench: the window makes the Format menu and
toolbar (`crates/katana_window/src/main_window/actions.rs`) and hands them over with `CustomisationServices` - the
Document, the view workspace, the window's action factory and log, whether
the session is headless (asked each time, since the window learns it after it
is built), and the window's own Layers action. So the
workbench never reaches into `main_window`, and a window test builds it and
drives it (`crates/katana_window/tests/workspace/customisation_workbench.rs`).
It owns what the managers share: the picture cache (`DefinitionThumbnails`)
and the one `CustomisationContext` each manager is built from. It owns the definition
editor as well ("The definition editor", below), which has no menu item: a
manager asks for it through the context. And it makes File > Settings' action
and owns that dialog ("Settings", below): the window tells it what its host
offers - whether there is a built-in customisation, and the kept file
(`CustomisationServices::has_built_in`, `kept_file`, `hasKeptFile`).

- **The linework control codes are not the workbench's.** They are the
  customisation's - the Document's (`customisation_state().linework`: what a
  customisation file says and `CUSTOMISE SET linework.*` sets) - and the code
  manager's Linework tab reads them there and sets them with that line
  ("Survey Code Manager", below). Until 2026-10-07 the workbench kept a copy
  for the tab, handed over in the managers' context, and followed the
  Document's codes into it from a `DocumentWatcher`
  (`followLineworkCodes`), filling the tab's seven fields by their control
  names and pressing its Use These Codes: a second home for one value, with
  window code keeping the two in step, and a button that changed the copy
  alone - codes typed there showed in no `CUSTOMISE` reply and reached no
  `KEEP`. The copy, the follower and the watcher are gone.
  `qt_the_linework_tab_strings_with_the_customisations_control_codes_headless`
  strings four points as one line with the default start code and as two
  once the customisation's is the token on the third, and FORBIDs the line
  the follower's press of the button logged.
- **The code manager is handed the views.** Its two action tabs have a View
  scope, so `show_code_manager` gives the dialog the workspace's open views
  (`SurveyCodeManagerDialog::views`, read afresh each time, as Global
  Modify's are).

- **Non-modal, one of each, kept.** A manager is made the first time it is
  asked for and then hidden, not deleted, between uses (the window keeps its
  model between frames);
  asking again shows and raises the same one. That is what lets the code
  manager's unapplied edits survive closing it. Each opens on its first entry
  - the style manager's first rows, the symbol library's first symbol, the
  code manager's rule 0 - rather than on an empty pane.
- **Deleted before the Document.** The dialogs hold the Document and paint
  from the workbench's cache, and a window destroys its members BEFORE its
  child widgets, so `~CustomisationWorkbench` deletes them itself - and the
  window destroys the workbench before the Document.
- **"Show me what uses it"** (`CustomisationContext::select_and_show`) selects
  the entities and frames them in the ACTIVE plan view only; the other views
  keep their zoom.
- **Found by the headless driver.** Each manager's action carries its
  dialog's control name as its data, which is how `--dialog formatStyles`
  finds the dialog it opened.
- **Purge Unused** (`purge_unused`) deletes every style, linetype and hatch
  pattern nothing uses (`katana_cad::purge::plan_purge`, `katana_cad::purge::purge_command`) as ONE undo
  step, keeping the current style, and names them in the log. An interactive
  session is asked first (the `confirm` hook, else a question box); a
  headless one is not. `qt_purge_unused_deletes_what_nothing_uses_as_one_undo_step_headless`
  purges, undoes and checks the style is back.
- **Closing the window asks the definition editor and the code manager
  first** ("Failure modes", above: `confirm_close`).

What a load does - a merge, or a Replace that is asked for - is
`docs/customisation.md`, "The verbs".

Not done:

- Process Linework has no button on the Survey toolbar, where Apply Survey
  Codes has one.

### Styles and Linetypes

One dialog for the `Style` and `Linetype` tables and the session's library
linestyles (`crates/katana_window/src/style_manager.rs`), because
they are one subject: a style names a linetype. An archive import brings 211
styles from one file - but NOT their linetypes: each style's linetype is a
LIBRARY name, which lives in the session's style library, not in the
model's Linetype table. So almost every real style names something the
drawing's own linetype table does not contain, and the dialog lists both.

What it shows, all from `crates/katana_cad/src/style_manager_rows.rs` so the
rows, filters and bulk edit are tested below the window:

| Tab | Shows | Does |
|---|---|---|
| Styles | `katana_cad::style_manager_rows::style_rows`: each style, how many entities wear it, and whether its linetype or symbol is missing; the chips All / Used / Unused / Missing and a search | a form (linetype and symbol through `NamePicker`, weight, colour or ByLayer, hatch, symbol size, description) with Save and Revert; New, Duplicate, Rename, Merge Into, Delete, Purge; Apply to Selection, Select Users, Make Current |
| Linetypes | `katana_cad::style_manager_rows::linetype_rows`: the drawing's linetypes and the library's linestyles, by group, a name both hold marked (D2) | a pattern grid that edits a drawing linetype's dashes, gaps and dots in place; New, Duplicate, Rename, Merge Into, Delete, Purge; New Style Using This; Edit Definition (`linetypeEditDefinition`: a library linestyle, in the definition editor) and New Library Linestyle (`linetypeNewDefinition`: a new one, there); Select Users |
| Hatch Patterns | `katana_cad::hatch_pattern_rows::hatch_pattern_rows`: each hatch pattern, solid or how many families, and who uses it (`katana_entity::table_usage::table_usage`) | a family grid (angle in degrees, spacing in model units) or Solid, with a swatch drawn by `katana_cad::hatching::hatch_segments`, Save and Revert; New, New Solid, Duplicate, Delete, Purge; New Style Using This; Select Users |
| Diagnostics | `katana_cad::style_manager_rows::style_diagnostics`: `katana_cad::style_catalogue::missing_names`, then the D2 collisions, each with who uses it and what is drawn meanwhile | Select Users |

A `StylePreview` beside each form draws the style, or the linestyle, at a
plot scale on paper or on screen through the shared painter (`docs/cad.md`,
"What a style draws"): lines at their PRINTED size, so a `paperstyle` looks the same
at every scale and a `worldstyle` shrinks as the scale's N grows; a symbol
fitted to the pane on its insertion point, with a scale bar in ground metres.
The dialog's Undo and Redo buttons are the drawing's own stack. A name is
asked for in a prompt row inside the dialog and a purge is checked in a panel,
never in a box, so a headless session drives every action by control name.

**QT-02's rule** (audit QT-02, fixed 2026-09-24). Save used to rewrite the
linetype and symbol of every archive-imported style: the form's boxes were
non-editable combos filled from the model's linetypes and the sixteen
built-in shapes, `setCurrentText` with a library name was a silent no-op on them,
and Save wrote back whatever they still showed. Now:

- a form writes back only the fields a person EDITED (`katana_cad::StyleFields`,
  `apply_edit`) - even for one style, because a spin box cannot show every
  stored value exactly (a weight of 0.1234, a symbol size of 0.03125), so
  writing back what it shows would change a style nobody edited;
- an unedited Save is no command at all (`katana_cad::style_manager_rows::edit_styles_command`,
  `katana_commands::table_commands::update_style_if_changed`), so Save is enabled whenever a style is
  selected and Revert only when there are edits;
- every name field keeps a name it cannot list - `NamePicker` for linetypes
  and symbols, `kept_name_combo.rs` for the hatch and dimension-style
  combos;
- selecting several styles shows `<varies>` for the fields they differ in,
  and Save leaves those fields of each style alone unless they were edited.

The Layers dialog got the same fix; there is no command that updates a
layer only if it changed, so its Save compares the `Layer` itself.

Two decisions from the first version still hold:

- **The table is read-only; the form edits.** Editing in the table would run
  a command from inside the table's own change, the shape of the crash
  recorded under "Panels refresh on the event loop" below. The table has no edit of its own at all, so the
  bug cannot be written. The
  dialog's own commands do not reload it either: they record what to select
  and the watcher's deferred reload does it, so there is one reload path.
- **A rename is a move, not an edit of a name field.** A `NamedTable` is
  keyed by name, so `rename_style` removes, re-adds under the new name and
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
`crates/katana_window/src/customisation/hatch_patterns_tab.rs`) is where the window got
the `HATCH` verb, which it had only offered by name in pick lists. Unlike the
two tabs before it, it runs nothing itself: every button writes the line -
`HATCH NEW`, `SOLID`, `SET`, `DELETE`, `PURGE HATCHES`, `STYLE NEW name HATCH
pattern` - and hands it to the window's executor (`CustomisationContext::run`),
so it is echoed and one undo step, and an agent types the same. The family
grid is a buffer saved by one `HATCH SET`; its swatch draws the grid as it
stands, with the offsets Save keeps, through the hatcher the plan view uses.
Angles show to twelve significant digits (`katana_cad::hatch_pattern_rows::hatch_angle_degrees`), the
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
window (`crates/katana_window/src/customisation/symbol_library.rs`):

- **left**, a tree of groups: All, Built-in, the library's `/` groups,
  "(ungrouped)", and "Not defined" when a name resolves to nothing;
- **centre**, a grid of 64-pixel pictures from the shared cache, captioned
  with the name and badged with how many entities draw it, under a filter
  bar: a search over name, group and survey code, and the chips All / In
  drawing / Used by codes / Missing / At vertices;
- **right**, a `StylePreview` at a plot scale (the insertion point marked,
  and a warning when it lies outside what the symbol draws), the details,
  and the actions.

What is listed is `katana_cad::symbol_assign::symbol_library`: D3's symbols (a definition is a
symbol when it is drawn at vertices, a survey code draws it as one, a style
names it as one, or its customisation lists it as a symbol), the built-in
shapes, and every symbol name a style OR A SURVEY CODE gives that nothing
defines, in the pickers' amber with the shape it is drawn as instead. The
codes' names are the library's own addition: `katana_cad::style_catalogue::missing_names` looks only at
styles and layers. The details' Source is the NAME of the customisation a
definition came from (`LineStyle::source`), or "Made in this session".

**How big it prints.** The details say what a point wearing the symbol
prints: "2 x 2 mm at 1:500, 1 x 1 m on the ground". `katana_cad::symbol_assign::symbol_print_size`
(`crates/katana_cad/src/symbol_assign.rs`) measures what `katana_cad::style_drawing::symbol_drawing`
draws - its strokes AND the space each of its texts covers, since a symbol
that is a letter, or carries one above its mark, prints the letter too - at
the style's size (a width in model units; 0 for the definition's own), at
1:N. A text's size comes from a `TextExtent`: the dialog measures it with the
preview's own font (`style_text_extent` in `style_painter`), so the number
agrees with the picture beside it, and without one cad estimates it
(`estimated_text_extent`: 0.6 of the height per character, the height tall,
descenders not counted) - so the two can differ a little, 2.91 mm with Arial
against 3 mm estimated for the fixture's TEST Valve. A built-in shape, or the
stand-in for a missing name, has no size of its own at size 0: its print size
is the viewport's plain mark, and the pane says so rather than giving a
number.

The actions, each from a button and never from a change of the grid's own
selection; each that changes the drawing is ONE undo step through
`Document::execute`:

| Button | What it does |
|---|---|
| Assign to Selected Points | `katana_cad::symbol_assign::assign_symbol_to_points`: the points among the selection move into a style that draws the symbol at the size given - one found that draws exactly that, else one made - and the log counts the points moved, already in it, not points, and not found |
| Set on Style | the chosen style's symbol (an inline style picker; `update_style_if_changed`) |
| Select Points Using | the points wearing a style that names it, selected and framed |
| Replace in Styles | `katana_cad::symbol_assign::replace_symbol_in_styles`: every style naming the current symbol names the one in the inline `NamePicker`; refused while that is empty |
| Import Definitions... (`importDefinitions`) | runs `CUSTOMISE DEFINITIONS "<file>"`: a Katana customisation file's DEFINITIONS and its colours, MERGED into the session's (D1: never a Replace from here); the executor logs the verb's reply, which counts what it added and replaced and what it left |
| Export Selected... (`exportSelected`) | runs `CUSTOMISE EXPORT "<file>" NAME <name> ONLY <the selected names>`: the selected library definitions as a Katana customisation file, with the session's notice, the sources they came from and the colours their pens name |
| New Symbol..., Edit Definition..., Duplicate Definition..., Delete Definition (`symbolNew`, `symbolEdit`, `symbolDuplicate`, `symbolDelete`; along the bottom, since they change the library and not the drawing) | ask the definition editor ("The definition editor", below) for a new symbol, or for the current symbol's definition; the last three need a symbol the library defines, not a built-in shape or a name nothing defines. The buttons do nothing themselves |

**Import and Export are Katana customisation files** (`docs/customisation.md`;
the file dialogs offer "Katana customisation (*.customisation.json *.json)").
They were style library files of another program's format until 2026-10-06.

- **Import** hands the window's executor the line `CUSTOMISE DEFINITIONS
  "<file>"` (`docs/customisation.md`, "The verbs"), so it is the path every
  load takes - the same read, merge and install - and the session's record of
  what went into it gains the file's customisation as a source of
  definitions. The verb's reply is the log: `loaded file=... name=test_symbols
  definitions_added=3 definitions_replaced=1 ...`, and what it does NOT take
  is counted in `left rules=<n> linework=yes|no automation=yes|no`: the
  file's survey code rules, its linework codes and its automation switches. A
  file that holds neither a definition nor a colour is refused, and the log
  says so as an error. *Rejected: taking the rules too*, which is what the
  button did when a file held one kind. A customisation holds everything, and
  rules loaded from a symbol browser would go round the Survey Code Manager's
  buffer, where a person reviews rules before Apply - and silently under any
  edits that buffer holds. A whole customisation is loaded by the `CUSTOMISE`
  line or in File > Settings > Import. A dialog given no executor says it
  cannot run the line and does not merge by itself.

  A file that LISTS its sources - every customisation a session writes does -
  brings them as the session's sources, and of those the import hands on only
  the ones that brought the file definitions, each as a source of definitions
  alone, and a table of colours when the file has colours
  (`katana_cad::customisation_part::definitions_of_file`, `crates/katana_cad/src/customisation_part.rs`). A source that
  brought the file nothing but rules is left out. *Rejected: keeping it with
  its `rules` flag cleared*, which is what the import first did. Every source
  a load lists is taken off the names the open project is missing
  (`Document::install_customisation`) and is recorded by the next save, so a
  project that warned "B is missing" lost the warning - for good - by an
  import of definitions from a file that merely listed B, with not one of B's
  rules in the session
  (`symbol_library::importing_definitions_leaves_out_a_source_that_brought_only_rules`).
- **Export** runs `CUSTOMISE EXPORT "<file>" NAME <the session's name, or the
  file's own while it has none> ONLY <the selected names>`, which writes the
  session's customisation CUT DOWN to the selected definitions through the
  verb's one writer (a file beside the target, then a rename, so that a write
  that fails leaves the file that was there) (`katana_cad::customisation_part::customisation_part`,
  `crates/katana_cad/src/customisation_part.rs`; then
  `katana_entity::customisation::customisation_to_json` with the same
  `CustomisationWriteOptions::only`), under the session's name - or, while
  the session has none, the file's own name without `.customisation.json`.
  It is the ONE rule a part of a customisation is written by: both managers'
  exports and `CUSTOMISE EXPORT` with a kind word or `ONLY`
  (`docs/customisation.md`, "The verbs"), so Export Selected writes the file
  `CUSTOMISE EXPORT <file> ONLY <the selected names>` writes:

  | Of the session | In the file |
  |---|---|
  | name, description, notice | as they are |
  | sources | those that brought what is WRITTEN, each said to have brought that alone, with its own notice: of definitions, a source one of the written definitions came from; of rules, a source that brought rules, when rules are written; a source that brought neither kind (a table of colours) only when the file carries a colour |
  | the notice of a source left out | written with the session's own, after it, each line once |
  | colours | those that what is written NAMES - a definition's pens; a rule's colour, its symbol's and its text's - compared as colour names are |
  | linework codes, automation switches | not written: the file says nothing of them |
  | `basedOn` | not written: a part is not an edition of the built-in |
  | the other kind (rules here; definitions for Export Codes) | not written |

  The notice is there because a customisation's notice is "carried with the
  data and shown to whoever uses it" (`docs/customisation.md`), and two
  symbols out of a customisation are still its author's data: an export that
  wrote them under the session's name with no notice passed them on without
  the terms they came with. The settings are left out because a merge takes
  them from any file that says them, and a file of two symbols for a
  colleague must not reset their control codes. The colours are those NAMED,
  no more: without them a pen of the customisation's own colour would draw in
  the entity's colour wherever the file went, and with all of them a load
  would overwrite colours of the same names that nothing in the file uses.
  *Rejected: the definitions and a name alone* (the first form, which lost
  the notice); *the whole session with the other kind switched off*, which
  carries the control codes and every colour. A definition reads back exactly
  as the session holds it, where it came from and that it is listed as a
  symbol included, which the older file could not hold
  (`symbol_library::export_writes_only_the_selected_library_definitions_and_names_the_rest`,
  `symbol_library::an_export_carries_the_sessions_notice_and_the_sources_that_brought_definitions`).

  The sources are those the SELECTED definitions came from. Until
  2026-10-07 they were every source that had brought the session a
  definition, and the notice of a source left out was dropped with it: the
  managers and the `CUSTOMISE EXPORT` verb were written side by side and cut
  a session down by two rules. *Rejected, with the reason kept*: the wider
  list was there because the session's own entry is the one a later load
  gives the file's notice to (`crates/katana_cad/src/customisation_merge.rs`:
  a notice "goes with the source of its name, or with every source it lists
  when none has its name"), so that a list narrowed to the selection's
  sources hangs the session's notice on somebody else's symbols. It does.
  But a source listed with nothing of it in the file passes for LOADED
  wherever the file goes - which is the fault the import above was changed
  to stop - and a notice shown beside more than its author meant is the
  safe side of the two. The decision, and what each of the two rules had
  right, is `docs/customisation.md` ("One rule for a part, whoever writes
  it"); the rule itself is tested below the window
  (`crates/katana_cad/tests/leaf/customisation_part.rs`).
- **A pen inside a definition is the colour the session resolves its name
  to**: the customisation's own colours, then the standard names. The one
  painter every picture goes through (`paint_style_drawing`) takes the table as
  `StylePaintTarget::colours`, and the grid's pictures, the preview, the plan
  (`PlanSource::colours`) and a sheet's legend all hand it the Document's. A
  name neither knows still draws in the entity's pen, as it always did
  (`symbol_library::a_pen_is_the_colour_the_customisation_gives_its_name_and_the_entitys_pen_when_nothing_does`).
  A changed colour table moves the library's generation, which is what drops
  the pictures and symbol stamps painted with the old colours: a stamp's key
  holds its symbol and the entity's pen, not the colour a pen inside it
  resolved to (`symbol_library::a_symbol_stamped_on_the_plan_is_drawn_again_when_the_customisations_colours_change`,
  which keeps one cache across the change as a view does; the legend's
  wiring is `symbol_library::a_sheets_legend_draws_a_pen_in_the_colour_the_plans_source_gives_it`).

A headless session opens no file dialog: Import and Export are
`importDefinitionsFile` and `export_selected_to`, which take a path, and the
buttons say in the log what to call. Not done: Assign makes a style with
linetype ByLayer and weight 0.25, and under D8 a line later put in that style
draws the symbol at its vertices; the grid's pictures are on the dark screen
ground only, though the preview switches; the dialog reloads whole (two
`table_usage` passes) on every command the watcher reports, not measured on a
250,000-entity drawing; a project's record
is of NAMES, so importing the definitions of a customisation that brought
both kinds clears a warning that it is missing although its rules are still
not here (only a source that brought rules alone is left out). Import and
Export run the lines `CUSTOMISE DEFINITIONS` and `CUSTOMISE EXPORT ... ONLY`,
which cut a part down by the one function, so Export Selected writes what the
verb writes.

An import into a session that was the kept one is kept again: Import
Definitions calls the commit hook round its line - before it, and what
the hook hands back after one that was taken
(`symbol_library::import_calls_the_commit_hook_before_the_install_and_what_it_hands_back_after_one_that_was_taken`)
- and the workbench answers the hook with the `CUSTOMISE KEEP` line ("The
definition editor", below). A linetype picker's picture of a DRAWING linetype
(`model_linetype_image`) is handed the session's colours as every other picture
is; such a linetype is dashes and names no pen, so nothing is coloured by
them yet.

### Survey Code Manager

The survey code library a surveyor codes against - what civil packages call
description keys or feature definitions - in five tabs, each
over one cad foundation, so the dialog decides nothing the CLI would say
differently: Code Table (`katana_cad::code_table::code_table` and `katana_cad::code_table::explain_code`, with the
rule form), Codes in Drawing (`katana_cad::code_table::code_census`), Issues
(`katana_cad::code_table::lint_survey_map`), Apply Codes (the `CODE` line, previewed before it
runs) and Linework (the `LINEWORK` line, and the customisation's control
codes). Its edits go to a BUFFER and reach the drawing only on Apply; Revert
takes the drawing's map back; closing with unapplied edits asks. That, and
what each tab shows, is `docs/survey_coding.md` ("The Survey Code Manager").

**Apply Codes and Linework build a line and run it** (2026-10-07; the rule
every dialog follows, "One executor", below, and for the scope the one
mechanism every tool on drawing data shares, `docs/cad.md`, "Scope and
filter"):

- **Apply to, and Only those that match.** Each tab has the shared scope and
  filter controls (`ScopeFilterWidget`, prefixes `apply` and `linework`:
  `applyScopeDrawing`, `applyScopeSelection`, `applyScopeView`,
  `applyScopeLayers`, `applyFilterLayer` ..., and the same after `linework`),
  starting on the whole drawing. They replace two radio buttons - everything,
  or the selection - that were a scope mechanism of the tabs' own.
- **Execute hands the line to the window's executor**
  (`CustomisationContext::run`): `CODE <scope> [PROPERTY p]`, and `LINEWORK
  <scope> [PROPERTY p] [ORDER entity] [CHORD x]`, echoed in the log, kept in
  the history, one undo step, answered in the verb's words. The line is
  shown as the controls change (`applyCommand`, `lineworkCommand`), with the
  reason in its place where the controls cannot be said on a line (no layer
  ticked, a double quote in a property's name), and Execute then runs
  nothing. A manager built without an executor says it cannot run the line
  and does not do the work some other way. Afterwards the tab shows the
  verb's own answer: the reply in `applyReport`, and in `lineworkSummary`
  how many lines the reply's first record says were drawn.
- **Codes in Drawing has the same controls, prefix `census`**
  (`censusScopeDrawing`, `censusFilterLayer` ...), and builds `CODE CENSUS
  <scope> [WHERE ...] [PROPERTY p]` (`censusCommand`), which Run Line
  (`censusRun`) hands to the executor. The table is counted here, over what
  the scope takes (`katana_cad::code_table::code_census` with the ids `katana_cad::global_modify::match_entities` gave),
  and not read from the line's reply, because it is classed against the rules
  being EDITED and the verb reads the drawing's; the summary begins with what
  the scope took ("3 entities matched the scope; ..."), Select Entities With
  Code acts among those it took, and a scope that cannot be read (no layer
  ticked) leaves the table empty with the reason in the summary
  (`survey_code_manager::the_census_tab_counts_what_its_scope_takes_and_builds_the_code_census_line_it_says`).
- **Preview is a plan made in the manager**, read-only and unlogged, because
  it shows more than a reply does: a row a code with its layer and style,
  every point in no line with its reason, every note. It is planned by what
  the verb plans with - `katana_cad::global_modify::match_entities` over the same scope and filter,
  then `katana_cad::survey_coding::apply_survey_codes`, or `katana_cad::linework_verbs::plan_linework`, which IS the
  `LINEWORK` verb's planning - and begins with what the scope took ("4
  matched."). So the Linework preview leaves out what the verb leaves out,
  and says so: the points their survey job has strung, and lines the drawing
  already holds. Rejected: previewing by running the line with `PREVIEW`. It
  would put a line in the log and the history at every press, and the reply
  counts the points in no line where the tab names each one.
- **Use These Codes is `CUSTOMISE SET linework.*`.** The seven fields show
  the customisation's control codes and follow them as they change, whoever
  changes them; the button runs the line for the codes that differ from the
  customisation's, and for none when none does. What a person is typing
  there is not overwritten by a change of anything else.
- **Controls that went, because no line can say them** and a second, direct
  path kept for them would be the two code paths this ends:
  `applyCreateLayers`, `applyCreateStyles` and `applySetAttributes`, which
  switched parts of the coding off (`CODE` does what the rules say, whole),
  and `lineworkKeepPoints`, off for deleting the points a line replaces
  (`LINEWORK` never removes a point: one taken out from under a survey job
  reads to its next re-adjustment as deleted by hand). The options are still
  in `katana_cad::survey_coding::SurveyCodingOptions` and `katana_cad::linework::LineworkOptions` for a verb word
  that wants them.
- **Controls that stayed, because the verb gained their words**:
  `lineworkProperty` (`PROPERTY <name>`, as `CODE` has it) and
  `lineworkChordTolerance` (`CHORD <length>`; millimetres on the form,
  which divides by 1000 and so assumes a drawing in metres, the line's number
  being in the drawing's units; said only when it is not the 5 mm the verb
  starts on). `docs/survey_coding.md`,
  "`LINEWORK` on the command line".

Tests: `crates/katana_window/tests/codes/code_manager.rs` (and, for the View
scope the workbench hands the manager, `customisation_workbench.rs`) - each tab
building exactly its line as each control changes, through a recording
executor that then runs it
(`survey_code_manager::the_apply_codes_tab_builds_exactly_the_code_line_its_controls_say`,
`survey_code_manager::the_linework_tab_builds_exactly_the_linework_line_its_controls_say`,
`survey_code_manager::the_linework_codes_are_the_customisations_and_use_these_codes_runs_the_set_line`), a
preview changing nothing and running no line, Execute as one undo step, a
second run drawing nothing twice, and a manager with no executor
(`survey_code_manager::with_no_executor_execute_says_it_cannot_run_its_line_and_changes_nothing`).

Not done: the two previews do not mark themselves stale
when the drawing changes under them (Execute plans again before it runs, so
what is run is never the stale plan); and the tab's chord length is in
millimetres, and the line says it in metres (the drawing's unit is assumed to
be the metre).

Import Codes... (`importCodes`) and Export Codes... (`exportCodes`) read and
write Katana customisation files, as the Symbol Library does: Import merges a
file's RULES into the buffer for review and leaves everything else in it
alone, and Export runs `CUSTOMISE EXPORT <file> CODES`, which writes the
DRAWING's rules (so it is refused while the buffer has unapplied edits: Apply
or Revert them first, since no line can name a buffer) as a customisation of
survey codes alone, cut out of the session by the rule the Symbol Library's Export is cut by
("Symbol Library", above; `katana_cad::customisation_part::customisation_part`, which is what
`CUSTOMISE EXPORT <file> CODES` writes by): the session's name and notice,
the sources that brought it rules, the notice of each source left out, the
colours the rules name, and none of its settings. Apply calls the commit
hook round its `set_survey_map`, as the definition editor's Save does
(`survey_code_manager::apply_calls_the_commit_hook_before_the_map_is_set_and_what_it_hands_back_after`). Apply is
not a line, and is not undone: it puts the edited buffer on the session, which
is session data (D1), and a line for it would write the buffer to a file in
order to merge it back (`docs/customisation.md`, "The verbs", "Two window
paths are not lines").
Every colour the manager shows or applies - the swatches of the Code Table,
the three colour fields of the rule form, the explanation, the lint, Apply
Codes and Linework - is resolved by the Document (`katana_cad::colour_lookup::resolve_colour`): the
customisation's own colours first, which the fields list ahead of the
standard names.

### The definition editor

Until 2026-10-06 nothing in the window could make, change, copy or delete a
linestyle or symbol definition: the symbol grid is read-only, the Linetypes
tab's library page was a label, and every library the window installed was a
whole file's. The owner asked to edit the customisation; survey codes had
their manager, and definitions now have theirs
(`crates/katana_window/src/customisation/definition_editor.rs`, `DefinitionEditorDialog`,
control name `definitionEditorDialog`).

**One editor, kept by the workbench, with no menu item.** A definition is
reached from where it is listed, so the editor is asked for by the managers:
the Symbol Library's `symbolNew`, `symbolEdit`, `symbolDuplicate` and
`symbolDelete`, and the Linetypes tab's `linetypeEditDefinition` and
`linetypeNewDefinition`. Each button does nothing itself: it calls
`CustomisationContext::edit_definition`, which the workbench answers with the
one dialog (`CustomisationWorkbench::edit_definition`), made the first time
and kept like the managers. A new definition is of the kind the manager that
asked lists - a symbol from the Symbol Library, a linestyle from the
Linetypes tab, whose button sits with the tab's own rather than on the page
of a library linestyle, so the FIRST linestyle of a library can be begun
there - and its kind can still be changed in the form. The editor has
`definitionNew`, `definitionDuplicate` and `definitionDelete` of its own. A
headless run reaches it as a panel once a step has opened it:
`@formatSymbols|!symbolNew|%definitionEditorDialog|definitionName=...`.
*Rejected: an item of its own on the Format menu.* What it would open is an
empty form: every use of the editor starts from a definition or from the
list a new one belongs in, and Format > Symbol Library and Format > Styles
and Linetypes are those lists. The managers' buttons are its menu items.

**What it edits** is one `katana_entity::LineStyle`:

| Field | Control name | Holds |
|---|---|---|
| Name | `definitionName` | `name`; typed only while the definition is being made |
| Group | `definitionGroup` | `group`, a `/` path |
| Kind | `definitionKind` | `symbol`: which of a customisation's two lists holds it |
| Units | `definitionUnits` | `world`, `paper` or `twoPoint` - the format's words, with what each means beside it (`definitionUnitsNote`) |
| At vertices | `definitionAtVertices` | `atVertices` |
| Length, Factor | `definitionLength`, `definitionFactor` | `length`, `factor` |
| Origin | `definitionOriginX`, `definitionOriginY` | `origin` |
| Anchors | `definitionAnchor1X`, `definitionAnchor1Y`, `definitionAnchor2X`, `definitionAnchor2Y` | `anchor1`, `anchor2`; typed into only while the units are `twoPoint` |
| Modes | `definitionStretchMode`, `definitionCycleMode` | the two whole numbers; `twoPoint` only |
| From | `definitionSource` | `source`, shown and not edited: where the definition came from |
| Strokes | `definitionStrokes` | `strokes` and `texts`, as text |

A number is shown as the shortest text that reads back as itself
(`katana_core::text::format_exact_real`) in a plain field, never a spin box: a spin box
shows 0.30000000000000004 as 0.3, and a Save of a form nobody touched would
then change the definition (QT-02's rule, above, was paid for by exactly
that). An empty number is the value a file's missing member has, and text
that is not a finite number - `inf`, `nan`, `1e999` - is refused as text that
is no number. The anchors and modes of a definition whose units are not
`twoPoint` are READ even though they cannot be typed into, so changing its
length does not drop them
(`definition_editor::an_untouched_form_describes_exactly_the_definition_it_was_opened_on`). The units box,
its note and the tips of the fields it enables all use the format's word,
`twoPoint`, so the form says one thing by one name.

**The strokes are text**, in the Katana customisation format's own form
(`docs/customisation.md`, "The members"): one stroke a line,
`["move", 0, 0]`, `["text", {"text": "W", "height": 1.5}]`. So what a person
learns in the editor is what a customisation file holds, a line copied from
one to the other means the same, and there is no second vocabulary of
strokes to keep in step with the file's. They are read as they are typed by
the format's own reader (`katana_entity::customisation::definition_from_json`); the dialog has no
parser. What joins the lines for the reader, finds the line a refusal is
about and words it is not the dialog's either: `katana_cad::definition_edit::stroke_text` and
`katana_cad::definition_edit::read_stroke_text` (`crates/katana_cad/src/definition_edit.rs`), below the window,
where a refusal's wording is tested without a window
(`crates/katana_cad/tests/customisation_verbs/definition_edit.rs`). *Rejected: a table of strokes with editable cells.* The headless
driver cannot edit a cell, so an agent could not drive it; "tables are
read-only and a form edits" is the house rule ("The rules a dialog or panel
follows"), because an edited cell runs code from inside the table's own
change; and the seven kinds of stroke take different values - two numbers,
three, one, a name, an object of seven members - so a grid's columns would
mean something else on every row.

- *The comma at a line's end may be left off*, and a blank line is passed
  over: a file needs the commas because it is JSON, and a person adding a
  last line should not have to go back and end the one before. That is the
  editor's one leniency, and the strict reader never sees it - the lines are
  joined with commas before they are handed over.
- *A line that does not read is named by its number in the box*
  (`definition_editor::a_stroke_line_with_a_typo_keeps_save_disabled_and_names_the_line`), with the reader's
  own refusal after it - `Line 3: definition "TEST Valve" strokes[2]: "drow"
  is not a kind of stroke ...` - and is washed in the box. The reader names a
  stroke by its place counted from 0, and says nothing of lines. So when the
  strokes do not read, each line is read alone until one fails: that costs a
  reading a line, and only of text that already failed.
- *No refusal cites a place in text nobody typed.* The lines are read under
  a first line of the editor's making, so the reader's "at line 2, column 14"
  is of that text and is left off (`Line 1: ... this stroke has a number too
  large to hold - 1e999`). For text that is not JSON the reason is asked of
  the line by itself: read in place, a line short of its `]` was reported as
  `unexpected '}'`, a brace of the wrapper
  (`definition_edit::a_refusal_never_cites_a_place_in_text_nobody_typed`).
- *The box is read character for character*: egui's text box holds the
  text as typed, so a no-break space and a line separator, both characters a
  stroke's text may hold, are read back as themselves. Read as "plain text",
  as the C++'s box first was, a no-break space came back a blank: a form
  opened on such a text read as edited before anything was typed - every
  other definition then refused, Revert no way out - and Save wrote the blank
  (`definition_editor::a_text_holding_a_no_break_space_opens_unedited_and_is_saved_character_for_character`).
  A definition whose text holds a paragraph separator is not opened, and is
  told why (a rule kept from the C++, whose box broke a line there), rather
  than shown as two lines that are no stroke.
- *The box holds strokes and nothing else.* A line can end the list of
  strokes and go on to give the definition members - `]], "atVertices": true,
  ...` - and read as sound JSON. Only the strokes are taken from what was
  read, and text that would have set anything else is refused
  (`definition_editor::the_strokes_box_holds_strokes_and_nothing_else_of_the_definition`), or a definition
  could be saved that the fields above do not show.

**A name is fixed once its definition exists.** Styles and survey code rules
NAME a definition, and a library has no rename (`crates/katana_entity/src/named_table.rs`),
so another name would be another definition, and everything naming the first
left naming nothing - drawn as a plain line or a stand-in mark, with nothing
said. Duplicate is how a definition comes by a new name: a copy equal but for
its name, under the first free one (`TEST Valve 2`), which can be changed
until it is saved - the first free in the library AND among the drawing's
own linetypes (`katana_cad::definition_edit::free_definition_name`), so a copy does not begin as a name
in both. A blank at an end of a NEW name is taken off, since it cannot be
seen where the name is shown and a rule naming `Valve` would miss `Valve `.
A new definition is not made under a name no command line could name it by -
one holding a double quote, or one that is a word of the `CUSTOMISE REMOVE`
line itself (`CODE`, `FORCE`): saved, it could never be removed again
(`definition_editor::a_definition_is_not_made_under_a_name_no_line_could_remove_it_by`). One that already has
such a name, out of a file, is opened and changed like any other.

**The preview is of what is typed, not of what is saved**: as a symbol on
its insertion point (`definitionPreview`) and, for a linestyle, along the
sample line (`definitionLinePreview`), at the plot scale chosen
(`definitionPlotScale`). `StylePreview` drew only NAMES found in the
Document's library; `StylePreview::set_definition` hands it a definition
instead, which it keeps in a library of its own holding that one alone, so
nothing is read from the Document's library and nothing put into it. The
pane's cache of flattened definitions is emptied at every such call: it is
keyed on a library's generation, and this library is a new one at each
keystroke under the same name. A definition is pictured from its first
stroke, before it has a name: the form is read under a stand-in name for the
picture alone, since what a definition draws does not depend on what it is
called (the pane stayed blank until a name was typed). While a line is half
typed the last picture that read stays up and the message area says why the
text does not - a pane that went blank at every keystroke of a stroke would
be blank for most of the time one is typed - and a line under the panes
(`definitionPreviewNote`) says the picture is not of what is typed now.

**The message area** (`definitionIssues`) says the first thing wrong, in
the reader's or `katana_entity::tables::validate`'s words; a name that is taken; and, while
the form reads, who names the definition - `rule #1 AC* (symbol) draws it as
its symbol`, `style "Marks" draws it as its symbol` - from
`katana_cad::definition_users::definition_users` (`crates/katana_cad/src/definition_users.rs`). That is
said of a NEW name too: a rule that names a symbol nothing defines is
waiting for it. What those users are drawn as meanwhile is said from the same
answer and not assumed: a name the drawing's own Linetype table also holds is
dashed by that linetype, and a name that is a built-in shape is drawn as that
shape - neither is "a plain line or a stand-in mark" - and a definition saved
under such a name is drawn in their place. That is allowed, as decision D2
allows a name in both, and said before Save
(`definition_editor::a_name_the_drawing_also_answers_to_is_said_as_that_and_not_as_naming_nothing`). A
definition with no strokes is saved, since the format holds one, and said to
have none.

**Committing.** The library is session data with no undo
(`docs/survey_coding.md`), so:

- **Save** (`definitionSave`) installs a copy of the session's library with
  the definition added or replaced (`Document::set_style_library`), as the
  Survey Code Manager's Apply installs its map. It is enabled only while the
  form reads as a definition and has something to save, and for a new
  definition only while its name is free
  (`definition_editor::a_name_the_library_holds_cannot_be_made_a_second_time`).
- **Revert** (`definitionRevert`) shows the form as it was opened, or as the
  library has had the definition since.
- **Delete** (`definitionDelete`) runs the line `CUSTOMISE REMOVE "<name>"`
  (`katana_cad::definition_edit::remove_definition_line`) through the window's one executor ("One
  executor", below), so it is echoed and an agent types the same. That verb
  is the interpreter's (`docs/customisation.md`, "The verbs") and refuses a
  definition something names, by the function the editor lists with
  (`katana_cad::definition_users::definition_users`). The window takes no `CUSTOMISE` line itself
  (2026-10-07): while it did, it read `REMOVE` as a file's name, and Delete
  deleted nothing.
  - *What the line answered is shown as it answered it.* The editor does
    not say why a line failed: it once said "is used" of any failure on a
    definition that had users, whatever the line had said
    (`definition_editor::a_line_that_fails_for_another_reason_is_shown_as_it_failed_and_a_failed_force_offers_nothing`).
  - *Who names the definition is said ONCE.* The verb's refusal cites each
    user in its own words (`rule #7 AC* (symbol) names it`;
    `DefinitionUsers::cited`), and until 2026-10-07 the editor put its
    own list of the same users under it (`rule #7 AC* (symbol) draws it as
    its symbol`), so a person read them twice. Decided then: the verb's
    reply, once. The editor's list is added only where what is on the
    page does NOT cite every user - a line that failed for another reason
    cites nobody, and a refusal that came before something else named the
    definition does not cite that - asked of the function the verb makes its
    lines with, not read out of the reply
    (`definition_editor::the_verb_itself_refuses_a_definition_in_use_and_delete_anyway_removes_it`).
    *Rejected: never adding the list*, which would offer `FORCE` over a user
    nothing on the page names.
  - *Delete Anyway* (`definitionDeleteAnyway`; Keep It is
    `definitionDeleteCancel`) is offered when the plain line failed and
    something names the definition, under what names it; it runs the line
    with `FORCE`. A `FORCE` that fails offers nothing further. The users are
    those of the definition the form is ON, whatever the form reads as at
    that moment.
  - *It exists only while that refusal is on the page.* The two buttons are
    themselves hidden and disabled otherwise - not only the row they sit in,
    which left the button pressable by a script, and one press then ran the
    `FORCE` line on a definition in use with nothing listed - and
    `remove(true)` is refused unless the refusal is showing
    (`definition_editor::delete_anyway_is_not_carried_out_unless_its_refusal_is_on_the_page`,
    `qt_delete_anyway_cannot_be_pressed_before_delete_has_been_refused_headless`).
  - Both questions are built into the page: a box would hang a headless run.
    What was deleted stays in the form, and Save puts it back - the only way
    back there is.
  - A name no line can name is refused before any line is run: one holding a
    double quote, which a command line cannot carry, and one that is the
    line's own word - the tokenizer drops the quotes, so
    `CUSTOMISE REMOVE "CODE" FORCE` would be read as "remove the survey code
    FORCE".
- **`CustomisationContext::begin_commit`** is called before each commit, and
  what it hands back is called after one that succeeded - never after one
  that was refused. It is how a session whose customisation is the kept one
  stays kept across an editor's own change while a typed change lasts the
  session only: the maker of the context knows whether the session was kept
  and runs `CUSTOMISE KEEP` afterwards; the editor knows neither and only
  keeps the order (`definition_editor::the_commit_hook_is_called_before_a_save_and_what_it_hands_back_after_it`).
  *Rejected: the editor reading "kept" from the Document itself*, which would
  put the rule for what is kept in every editor that commits. Every editor
  that commits to the session's customisation calls it the same way: this
  one's Save and Delete, the Survey Code Manager's Apply and the Symbol
  Library's Import Definitions.
  - *The workbench answers it* (`CustomisationWorkbench::context`): it reads
    `customisation_state().kept` when asked, and hands back the line
    `CUSTOMISE KEEP`, through the window's one executor, ONLY when the
    session was the kept one and the session has a kept file
    (`CustomisationServices::has_kept_file`, which the window sets from its
    host's kept-file path). A customisation typed, scripted or loaded with
    `--customise` is "not kept", and an editor's change on top of it lasts
    the session as it does. With no kept file nothing is handed back: `KEEP`
    would only be refused, in the log, after every Save. The line is run as
    a typed one, so it is echoed, and what `KEEP` refuses - a kept file
    changed on disk since it was read - is said as it would be to a person
    who typed it. Tested with a host and a kept file of the test's own, each
    of the three commits in turn
    (`customisation_workbench::an_editors_commit_of_a_kept_session_runs_the_keep_line_once_and_it_stays_kept`,
    `customisation_workbench::a_session_that_was_not_the_kept_one_is_not_kept_by_an_editors_commit`,
    `customisation_workbench::with_no_kept_file_an_editors_commit_runs_no_line_and_logs_no_refusal`).
  - *The window's half is two lines* - the window hands the workbench the
    predicate as it makes the Format actions, and `load_default_customisation` sets what it
    answers from the host's kept-file path - and the three tests above cannot
    see either: they supply the predicate themselves. Found in review
    (2026-10-07): with either line deleted every test passed, while the real
    window kept nothing. Two headless checks hold them, each saving a new
    symbol in the editor of a window whose built-in is the symbol fixture. With
    `KATANA_CUSTOMISATION` naming a file the `kept` record comes before the
    editor's own line and the session is `origin=edited kept=yes`; with the
    variable unset no line is run - neither a `kept` record nor `KEEP`'s
    refusal - and the session is `kept=no`
    (`qt_a_save_in_the_definition_editor_keeps_a_kept_session_in_its_kept_file_headless`,
    `qt_a_save_in_the_definition_editor_runs_no_keep_where_the_window_has_no_kept_file_headless`).
    Both lines change when File > Settings gives an interactive session a
    per-user kept file.

**One definition at a time.** Opening another while the form has edits that
are not saved is REFUSED and said, in the message area and the log
(`definition_editor::another_definition_is_not_opened_over_edits_that_are_not_saved`): there is no undo
to get typed strokes back from, so Save or Revert comes first. *Rejected:
dropping the edits*, which is what the style manager's form does on another
row ("Not done" under "Styles and Linetypes") and is survivable there only
because a style is three fields. Closing the dialog hides it and keeps the
form, and says so in the log; closing the WINDOW asks first
(`CustomisationWorkbench::confirm_close`). A yes is only remembered
(`DefinitionEditorDialog::agree_to_discard`), until the form next changes:
the code manager and the unsaved drawing are asked about after it, and a
Cancel at either keeps the window open - with the strokes still in the form.
It once reverted the form on yes, and that Cancel then found them gone
(`definition_editor::the_window_is_not_closed_over_a_form_that_is_not_saved`).

**Changed elsewhere.** The editor watches the Document as the managers do.
A definition another load or command changed is taken up when the form is
untouched; when it holds edits they are kept, the message area says the
definition was changed elsewhere, and Revert shows the library's as it is
now. One removed elsewhere stays in the form, and Save puts it back
(`definition_editor::a_definition_changed_elsewhere_is_taken_up_unedited_and_said_over_edits`).

Tested in `crates/katana_window/tests/codes/definition_editor.rs`, with
a customisation written out in the test and every saved definition compared
with one built by hand; the unsaved-definition preview in
`crates/katana_window/tests/styles/shared_style_preview.rs`; who names a
definition in `crates/katana_cad/tests/custedit/definition_users.rs`; the
strokes as text, the name a copy takes and the remove line in
`crates/katana_cad/tests/customisation_verbs/definition_edit.rs`; and the real window,
headless, in `qt_a_symbol_made_in_the_definition_editor_is_listed_by_the_symbol_library_headless`,
`qt_delete_anyway_cannot_be_pressed_before_delete_has_been_refused_headless`
and `qt_a_definition_a_survey_code_names_is_refused_by_delete_and_removed_by_delete_anyway_headless`.
The window tests give the editor a runner of their own for `CUSTOMISE
REMOVE`, so that what they assert is the editor's; one gives it the verb
itself (`definition_editor::the_verb_itself_refuses_a_definition_in_use_and_delete_anyway_removes_it`), and
that every line the editor can build is read by the verb as that one
definition is `customisation_verbs::the_line_an_editor_builds_to_remove_a_definition_is_read_as_that_definition_alone`
(`crates/katana_cad/tests/customisation_verbs/customisation_verbs.rs`).

Not done:

- **No verb makes or changes ONE definition.** Save is a widget's call on
  the Document, as the code manager's Apply is. On the command line a
  definition is made or changed by writing it in a customisation file and
  loading that; an agent has no shorter way.
- **A session with no kept file keeps nothing.** An editor's commit keeps a
  kept session kept only where the session has a kept file. An interactive
  window has one - `customisation.json` in the per-user place, or the file
  `KATANA_CUSTOMISATION` names (`MainWindow::load_default_customisation`;
  `docs/customisation.md`, "Settings and the kept customisation") - and a
  headless run has none unless that variable is set; there Keep is refused
  by name, and `CUSTOMISE` says `Kept file: none.`
- **A user of a definition is still worded two ways**, by the verb and by
  the editor's own list (`docs/customisation.md`, "The verbs: not done"),
  though no longer both under one refusal.
- **A definition the form cannot show cannot be opened** - one the format
  cannot write (a stroke carrying a member its kind does not use, texts that
  are not its text strokes in order; `docs/customisation.md`, "What it
  refuses"), or one a text of which holds a paragraph separator. The editor
  says which and opens nothing
  (`definition_editor::a_definition_the_form_cannot_show_is_not_opened_or_copied_and_is_said_when_it_arrives`);
  only a library built outside the editor can hold one.
- **The Linetypes tab's library page still opens with library linestyles
  "are read-only here"**, now followed by where one IS edited. The first
  sentence is pinned by `qt_the_style_manager_lists_a_library_group_s_linestyles_headless`
  and was kept: the page itself edits nothing, so it is true, though a page
  that only said where a linestyle is edited would read better.
- **The symbol library does not select what was just saved**, and the
  editor has no list of its own: a definition is found in the manager that
  lists it.
- A text stroke's letters are previewed in the pane's own font, as every
  preview's are.
- **The theme has no rule for a disabled field**, only for a disabled button
  (`crates/katana_window/src/theme`): a line, a choice or a number box that cannot
  be typed into is drawn exactly as one that can. The editor dims its own
  with a rule on the dialog, because a fixed name has to be told from one
  that can still be typed; every other dialog's disabled fields still look
  live. The rule belongs in the theme.

## Panels refresh on the event loop, never inside their own signal

(The title is kept, since other documents cite it; in the egui window the
"event loop" is the next frame and the "signal" a control's own handler.)

**The rule: a document change never rebuilds a panel under the control that
made it.** In the egui window it holds by construction: a panel is drawn
from the document each frame and keeps no items a change could delete under
it, so an edit made from inside the panel's own drawing is shown at the next
frame, and a transaction of a hundred commands draws the panel once. The
rule was learned in the C++ window, where switching a layer off crashed the
application: the box's change handler ran the edit, the document's listener
rebuilt the layer tree at once, and the item whose handler was still running
was deleted under it (the property table had the same bug behind an edited
value). **Rejected** there and here: guarding each handler with a
re-entrancy flag, because that leaves the next handler to make the same
mistake.

`katana --toggle-layer NAME --screenshot out.png` flips the box through the
panel's own handler, headlessly, and refuses to continue if the document and
the panel disagree afterwards (`toggle_layer_through_panel`,
`crates/katana_window/src/main_window/panels.rs`). `qt_toggle_layer_headless`
runs it on the sample; it failed against the C++ with its listener made
synchronous again, which is how the test earned its place.

A related rule, from the same afternoon: **a headless session never opens a
modal box** - every question the window would ask becomes a line in the log
or a refusal. `docs/headless.md` ("A headless session never opens a modal
box") lists what each one does.

## One executor: the command runner

A dialog that changes the drawing does not do the work itself: it builds the
verb line a person would type and hands it to the window's one executor,
`MainWindow::run_verb_line`, as a `CommandRunner` (`crates/katana_window/src/host/command_runner.rs`).
The workbenches carry it to their dialogs - `SurveyServices::run`,
`CustomisationServices::run` into every manager's `CustomisationContext::run`,
`AnnotationWorkbench::set_command_runner`, `UtilityServices::run` - so a dialog
never reaches into the window for it.

- **The same dispatcher as a typed line.** `run_command_line` is Enter on the
  command line: it reads and clears the field, echoes the line, offers it to
  the workbenches' verbs (`run_workbench_line`: the geoprocessing executor's -
  `GDAL`, `IMPORT`, `EXPORT`, `INFO <file>`, `REFS`, `COPC` - then `ONLINE`,
  `UTILITY`), then to a running tool (`ViewWorkspace::type_into_tool`), and
  hands whatever is left to `dispatch_line` - the view verbs, the window's own
  (`SCRIPT`, `PLOTSHEETS`, `PLOT`, `SNAPSHOT`), a tool's alias,
  and the interpreter (`CUSTOMISE` among its verbs, which the window read and
  ran itself until 2026-10-06). `runVerbLine` echoes the line and runs the same
  `run_workbench_line` and `dispatch_line`, so the two cannot come to differ; the
  line is kept in the interpreter's history and undone exactly as a typed one.
- **Never a running tool's answer.** That step is the only one `runVerbLine`
  leaves out. A tool waiting for a point or a text takes any typed line for
  one; a dialog's `CIRCLE 5,5 2` while Line waits for its first point is drawn,
  and the tool still waits. What is being typed on the command line is left
  alone.
- **The reply comes back.** While the line runs, `log_message` also collects
  what it logs, and `warn_user` what it shows in a box, into a
  `VerbOutcome`: `ok`, the `reply` lines and the `error` lines. `ok` is judged
  by the errors counted, as `run_command` judges a `--command` line, so a
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
(`SheetEditor::set_command_runner`): Generate Sheets, Choose Paper and the view
fields run their `GENERATE`, `SHEET SUGGESTPAPER` and `VIEW SET` lines
through it (`docs/plotting.md`, "The sheet editor"), and the Sheet Set menu
its `SHEETS SAVE`, `LOAD`, `APPEND`, `JSON` and `PAGESETUP` lines ("The Sheet
Set menu and Page Setup"). The window's command line
and the editor's lines get the same sheet-verb context,
`sheet_verb_context_for`, which replaced the window's own copy of it.

**One rule for a word on the line.** A name a dialog writes into its line is
written by `katana_cad::annotation::command_words::command_word`
(`crates/katana_cad/src/annotation/command_words.rs`): bare when it can be,
double-quoted when it is empty or holds a blank. A name with a double quote or
a line break in it cannot be carried, because the tokenizer has no escape, so it
is refused. Otherwise the line would run on the name cut short at the quote.
One verb reads past the tokenizer: `CRS SET` takes the rest of its line
verbatim, because a WKT is all quotes (`docs/cad.md`, "The project's
coordinate system"), so File > Project Coordinate System writes its text onto
the line as typed.
An annotation text uses `annotation_text_word`, which also writes a line break
as `\n`. `crates/katana_window/src/command_word.rs` hands a field's text to the same rules
(`command_word`, `annotation_text_word`) and names the field in the refusal, and
writes a number exactly (`exact_number`). The Alignment Manager, the Hatch
Patterns tab, Edit Label, the text and label style managers, the Dimension
Styles manager's numbers and the plan view's shortcut menu write their words
with it. The menu lists a layer or style whose name holds a quote (a DXF can
bring one) disabled, with the reason as its tip. The parallel lanes of the
2026-09-26 UI work first wrote four copies of this rule, and the integration
folded them into this one; Edit Label and the style managers still carried
wrappers of their own over it, and four files their own exact-number
helper, until these were folded in too.

Not done: these lines still quote a name or a path by hand, always, rather
than through `command_word` - the Sheets editor (`sheet_editor.rs` `quoted`),
the Sheet Set menu (`crates/katana_window/src/plotting/sheet_set_menu.rs` `quoted_path`), File >
Import's `IMPORT "<file>"` (the window's `import_placement.rs`), Plot to PDF's
`PLOT "<file>"` (`crates/katana_window/src/plotting/plot_drawing_dialog.rs`), Export View as Image's
`SNAPSHOT "<file>"` (`crates/katana_window/src/plotting/view_image_export.rs`), Run Script's
`SCRIPT "<file>"` (`script_runner.rs`), the GIS menu's `COPC "<source>"
"<destination>"` (`crates/katana_window/src/main_window/window.rs`), and Page Setup's `pattern="..."`, with
its own backslash doubling (`crates/katana_window/src/plotting/plot_dialog.rs`). A Windows path
cannot hold a double quote, and none of these dialogs offers one. Still, each
is a second spelling of the rule and should move onto `command_word`; doing so
changes the lines those dialogs' tests pin.

### The session's verbs on the window's command line

Until 2026-09-26 the window refused or misread several verbs `katana_cli` had.
Each now reaches the same code on every front end
(`qt_the_session_verbs_run_on_the_windows_command_line_headless`,
`qt_copc_typed_on_the_windows_command_line_converts_the_cloud_headless`,
`qt_import_local_typed_on_the_windows_command_line_moves_the_data_headless`):

- `INFO 12` describes entity 12, unless a file of that name exists, and
  `INFO #12` describes it whatever files there are:
  `CommandInterpreter::is_entity_id`. Entity Information and
  `katana_describe_entity` send `INFO #<id>`; run beside a file called `1`
  or `#1` they once failed (`cli.info_hash_id_is_the_entity_beside_a_file_of_that_name`,
  `qt_info_hash_id_is_the_entity_beside_a_file_of_that_name_headless`). The window, and the session under
  `katana_cli` and `katana_mcp`, once took every `INFO` for `INFO <file>`, so
  `katana_describe_entity` answered that the file did not exist.
- `CODE`, `CODE EXPLAIN`, `CODE CENSUS`, `CODE LIST` and `CODE CHECK`
  are the interpreter's (`crates/katana_cad/src/survey_code_verbs.rs`). They
  resolve a colour name through the Document - the customisation's own
  table, then the standard names - and no front end hands the interpreter
  a lookup of its own any more: the window had only the standard names to
  give, which the Document knows, and the interpreter's colour-lookup setter
  went with its last caller. (`CODE LIST`
  and `CODE CHECK` were `MAPFILE LIST` and `MAPFILE CHECK` until 2026-10-06.)
  Survey > Apply Survey Codes runs `CODE SELECTION` or `CODE DRAWING` ("The
  Format menu", above).
- `COPC <source> <destination.copc.laz>`, each path one word or quoted, a
  background job whose `converted` record names the file
  (`docs/interop.md`, "IMPORT, EXPORT, INFO, REFS and COPC on every front
  end"). GIS > Convert Point Cloud to COPC offers to import the result once
  the job has converted it. In a headless session the menu item opens no
  file dialog and names this verb instead.
- `CUSTOMISE`, the whole family, is the interpreter's
  (`crates/katana_cad/src/customisation_verbs.rs`; `docs/customisation.md`,
  "The verbs"): the report, `JSON`, a load (merge or `REPLACE`), `EXPORT`,
  `RESET`, `KEEP`, `REVERT`, `REMOVE` and `SET`, answered in records. The
  window had a tokenizer and a loader of its own for `CUSTOMISE [REPLACE]
  <file>...`, beside `katana_cli`'s, written twice because the readers of
  the older files lived where the interpreter could not see them - and the
  two had come to differ. Both are gone from the window
  (`Executor::dispatch_line` hands the line on), with the prose they
  printed: a load now answers `loaded file=... name=... definitions_added=...`
  a file, the names the rules ask for that nothing defines, and the
  `customisation` record; a file named twice is `repeated file=...`; a file
  of another program is refused as not a Katana customisation file, and
  nothing is loaded. `CUSTOMISE` alone is the two count lines, the records
  (the session's name and origin, its sources, the automation, the linework
  codes, what the open project is missing) and what it covers in this
  drawing.
- `IMPORT <file> LOCAL` moves a DXF, vector file or .12da archive as one piece
  so its lower-left corner sits at 0,0, and asks nothing; a raster or a point
  cloud refuses it by name. The path and the `LOCAL` are read by
  `CommandInterpreter::import_argument`, as the session reads them; `LOCAL` was
  once taken for part of the path. `ALONGSIDE` and `OFFSET=dE,dN` work the
  same way, and the import dialogs offer all of them (`docs/interop.md`,
  "Placing an import").
- `UPDATE`, the whole family, is the session's
  (`crates/katana_app/src/update_verbs.rs`): `CHECK`, `STATUS`, `DOWNLOAD CONFIRM`,
  `APPLY CONFIRM`, `SKIP`, `BLOCKED CLEAR`, `SET AUTO` and `TOKEN`.
  `Executor::run_workbench_line` takes it before every other family and the
  update workbench runs it - the three lines that take time as jobs, the rest
  at once ("Help > Check for Updates", below). A headless window answers
  `CHECK` and `STATUS` when they are typed and has `DOWNLOAD` and `APPLY`
  answered `reason=headless`
  (`qt_a_typed_update_status_says_a_build_tree_is_a_development_copy_headless`,
  `qt_a_typed_update_download_and_apply_say_a_headless_run_does_neither_headless`).

## Run Script: a katana_cli script in the window

A script is what `katana_cli` runs: a `.kcs` file of commands, one a line,
UTF-8, a Windows line end taken off, blank lines and lines whose first
non-blank is `#` skipped. The session under `katana_cli` and `katana_mcp`
once skipped only a line whose first character was `#`, and refused an
indented note that the window skipped; it reads the same rule now
(`cli.an_indented_note_in_a_script_runs_nothing`,
`mcp_server::a_scripts_indented_note_runs_nothing`). The window runs the same files three ways, all
through one function, `Executor::run_script`
(`crates/katana_window/src/script_runner.rs`):

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
that line (`qt_a_script_may_not_run_itself_headless`). Not done: a long run
shows no progress and cannot be cancelled - the script runs inside one call
(`run_lines` in `crates/katana_window/src/main_window/executor.rs`) and no
frame is drawn until it ends, where the C++ showed a progress dialog with a
Cancel that stopped between two lines (`script_runner.rs` takes such a
callback; the window passes none).

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
instead (`Executor::type_lines_into_tool`): each line typed in turn, as though
typed and entered, and a blank line left out, since to Text it is the Enter
that finishes and a paragraph break in what was copied is not. Run as a
script, a label's words were once taken for commands - `RECT 0,0 5,5` pasted
as a text's first line drew a rectangle, and its second line was refused
(`qt_lines_pasted_at_a_text_prompt_are_the_texts_lines_headless`).

Tested in `crates/katana_window/tests/host/script_runner.rs` (reading, the stopping
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
`crates/katana_window/src/command_reference_dialog.rs`), built from the texts the verbs'
own code keeps, so nothing is written twice:

- **Commands**: `CommandInterpreter::help_text`, the annotation verbs included;
- **Sheets**: `katana_cad::plotting::sheet_verbs::sheet_verb_help`, every option (Help > Sheets and
  Plotting Commands, `helpSheetCommands`, opens the reference here);
- **Subsurface utilities**: `katana_cad::utilities::utility_verbs::utility_verb_help`;
- **Online data**: `katana_interop::online_verbs::online_usage`;
- **Updates**: `katana_app::update_verbs::update_help_text`, the `UPDATE` verbs' own help, which
  `HELP UPDATE` and `katana_cli --help` print. One text, so a line added to
  the family is in the reference at once: `UPDATE BLOCKED CLEAR` came with
  the verb and needed no edit here
  (`command_reference::the_updates_section_is_the_update_verbs_own_help` holds every line of the help to
  be in the section, and the section to have no line of its own);
- **Window**: `window_help_text`, the verbs `Executor::dispatch_line` and
  `run_workbench_line` take before the interpreter (`SCRIPT`, `IMPORT`,
  `EXPORT`, `INFO <file>`, `REFS`, `COPC` - the geoprocessing executor's,
  whose usage the Geoprocessing section gives - `PLOTSHEETS`, `PLOT`,
  `SNAPSHOT`, `ZOOM`, `GRID`, `SNAP`, `EXAGGERATION`, `ROADMODEL SHOW`
  (`run_workbench_line`, the road workbench's), `ONLINE`, `UTILITY`,
  `UPDATE` - a row that names the family's lines and sends the reader to the
  Updates section for their usage - `QUIT`, and `HELP`, which adds this
  section to the interpreter's), and the rule for a bare tool
  word - with the one word that means different things on the two command
  lines: a bare `LS` starts the List tool in the window and is `LABELSTYLE`
  in `katana_cli`. `CUSTOMISE` had a block here while the window ran it
  itself; it is under Commands now, with the interpreter's other verbs, and
  `HELP CUSTOMISE` prints every word and every reply;
- **the tools**, a section a menu: each tool's name, aliases, key, tip and id,
  from the tool catalogue.

`reference_entries` reads a help text into entries by the layout every help
here keeps: an entry starts at a line that does not start with a blank, a
label in the first column (`DimStyle  DIMSTYLE LIST ...`) titles a group, and a
first paragraph ended by a blank line is the section's introduction. The
search wants every word it is given, in any case; a double-click puts the
entry's verb on the command line, to be finished there, and the reference
itself runs nothing. `window_help_text` lives beside the reference rather than
in the main window so that the window tests search the real text; a comment
on `dispatch_line` says a verb added there is added to it, and
`command_reference::the_window_section_names_every_verb_the_window_runs_itself` checks
the list. A typed `HELP` (or `?`) alone prints the interpreter's help, the
geoprocessing verbs', the `UPDATE` verbs' and then `window_help_text`; `HELP
SHEETS` and `HELP UTILITY` are unchanged, and `HELP UPDATE` prints that
family alone.

The Updates section and `UPDATE` in the Window list were added on 2026-10-14.
The update verbs had come with their own help and a `HELP UPDATE`, and the
reference - the one place a person searches - had neither: a search for
"update" found nothing, though the command line took every line of the
family. *Rejected: a second description of the lines in `window_help_text`*,
which is how the Window section describes the verbs only the window runs:
`UPDATE` is the session's, its help is the session's, and two texts would
have drifted at the first line added
(`command_reference_dialog::the_update_verbs_are_found_by_searching_and_put_on_the_command_line`).

The reverse fault is fixed too: the interpreter's help listed `PLOTSHEETS`,
which `katana_cli` refuses (it paints, and only the window can), so
`katana_cli -h` and `katana_mcp`'s `katana_help` advertised a verb their
reader could not run. The line now says it is the window's, and names
`--plot-sheets` for a headless run.

Help > Keyboard Shortcuts (`helpKeyboardShortcuts`, `keyboardShortcutsDialog`,
`crates/katana_window/src/keyboard_shortcuts_dialog.rs`) is a searchable table of every
key the window answers to - the key, the command, the menu it is under and its
status tip (`MainWindow::shortcut_rows`) - with any key that
`MainWindow::shortcut_clashes` finds reaching two commands marked "(clash)" and
listed under the table. The keys are the ones `--check-shortcuts` counts, so
the two cannot disagree.

Tested in `crates/katana_window/tests/host/command_reference.rs` and, through the Help
menu, by `qt_the_help_menu_finds_a_verb_and_lists_every_key_headless`.

## Help > Check for Updates

Katana replaces itself with a newer release. The engine that checks,
downloads and swaps the files, the helper program that does the swap and the
release feed are the update system's own (its document is `update.md`); the
verbs are the session's, shared with `katana_cli`
(`crates/katana_app/src/update_verbs.rs`: `UPDATE CHECK`, `STATUS`, `DOWNLOAD
CONFIRM`, `APPLY CONFIRM`, `SKIP`, `BLOCKED CLEAR`, `SET AUTO`, `TOKEN`). This
section is what the WINDOW adds: a menu item, a status-bar button, a dialog, a
page of Settings ("The Updates page", under "Settings"), and the close that
lets the helper run.

**Where it is.** Help > "Check for Updates..." (`helpCheckForUpdates`, the
letter U, the icon `Icon::Download` - a cloud with the accent arrow coming
down out of it; its data names `updateDialog`, which is how `--dialog` finds
the dialog), above About. And a button in the status bar,
`statusUpdateAvailable`, reading "Update 0.3.0": hidden until a newer release
is known, shown while there is one to act on - offered, on its way,
downloaded, or waiting for other programs to close
(`update_status_button_text`; in a build tree, where no update is ever known,
the button is there by its name and has no words:
`qt_a_typed_update_status_says_a_build_tree_is_a_development_copy_headless`).
Both open the dialog.

**Two classes.** `UpdateDialog` (`crates/katana_window/src/update_dialog.rs`) shows and
asks; `UpdateWorkbench` (`crates/katana_window/src/update_workbench.rs`) owns it and
does everything else, built as the geoprocessing and utility workbenches are.
The dialog needs no window and reads no engine: it is given an `UpdateView` -
plain data - and a context of callbacks, so the window tests drive it alone
(`crates/katana_window/tests/update/update_dialog.rs`). A press changes no text in it:
what a line did comes back as the next view, never as a guess of the
dialog's (`update_dialog::a_press_changes_no_sentence_by_itself_the_next_view_does`).

**What the workbench decides is not in the workbench.** It needs the whole
window, so no test can build it; a decision written inside it is run by
nothing. So each one is a function of its arguments beside the dialog
(`update_dialog.rs`, "THE DECISIONS"), in the library the window tests link,
and the workbench only gathers the facts - the disk, the clock, the
environment, the job runner - and does what they say:

| Function | Decides |
|---|---|
| `update_view_of_offer` | which state `UPDATE CHECK`'s answer is: Available, or Ready when that version is already downloaded; Current; NotOffered; CheckFailed; Unsupported |
| `update_quiet_answer` | what the AUTOMATIC check does with its answer: announce it, show it without a word, or drop it |
| `update_effect_of_line` | what a line that was run at once and carried out does to what the dialog shows: `UPDATE SKIP NONE` and `UPDATE BLOCKED CLEAR` take back what their own list set aside, `UPDATE SKIP <version>` the release that was offered |
| `update_staged_to_offer` | whether the release `UPDATE STATUS` says is downloaded is offered as Ready when the window starts |
| `update_staged_after` | whether a downloaded release is still one to install after a check answered |
| `update_view_of_last_result` | how the last apply's result is shown at the next start, and for which endings the view carries the command that puts the install right by hand |
| `update_automatic_check_is_due` | whether the window checks by itself now |
| `update_skipped_of_state`, `update_put_back_of_state` | what Settings > Updates is told of the state file: the skipped version, and whether a release that was put back can still be offered here - and nothing at all for a file that does not read, never "none" ("The Updates page") |
| `update_tells_of_set_aside` | whether the dialog is telling of a release that is set aside, which keeps the page's button on while the dialog points at it |
| `update_start_may_confirm` | whether a window that has come idle is the proof that an update starts: only in a copy with an install record whose installed version is the program's own, which refuses a record that names a version above the program's ("An update that was never said to start", below) |
| `update_failure_is_boxed` | whether an update that could not be started is said in a message box as well as on stderr: only in a run that was given no line to run. It is `main()`'s decision, here with the others so that a test can ask it |
| `update_recovered_transactions` | which helper copies a start may remove: those of this install's updates that a recovery put right and that are over |
| `update_status_button_text` | the status-bar button's words, and that it is hidden |
| `update_reply_value`, `update_staged_of_reply` | a record of a reply, by its key |
| `update_logged_line`, `update_logged_reply` | what the log may show of a line, and of the verbs' answer to it |

Their tests are `update_decisions`, with records written by hand.

**The states.** The first sentence is `updateStateText`, the rest
`updateDetailText`, and exactly these buttons are shown; the others are
hidden, which the headless `--press` refuses.

| State | What it says | Buttons |
|---|---|---|
| Idle | "Katana 0.2.0" / "Not checked for updates yet." | Check now |
| Checking | "Checking for updates..." / "You can keep working." | Cancel |
| Current | "Katana 0.2.0 is up to date." / "Last checked 14:02." | Check now |
| NotOffered | a newer release this copy does not offer: one the person skipped ("Katana 0.3.0 is available, and you chose to skip it." / "Settings > Updates offers it again.") or one that was rolled back here ("Katana 0.3.0 could not be installed here before, and is not offered again." / "A later release will be offered, and Settings > Updates offers this one again.") | Check now |
| Available | "Update Available" / "Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed, 71 MB to download." | Update, Download only, Skip this version, Release notes, Later |
| Downloading | "Downloading 31 of 71 MB (12 of 23 files)." / "You can keep working.", and `updateProgress` | Cancel |
| Ready | "Katana 0.3.0 is ready." / "Katana will close, update and reopen in a few seconds." | Restart and update, Later |
| Blocked | "Close these first: katana_mcp.exe (4412)." / "Then press Retry." | Retry |
| RolledBack | "Katana 0.3.0 could not be installed, so 0.2.0 was restored." / "Reason: ...", and after it, when the reason is that the new version did not start, "A later release will be offered, and Settings > Updates offers this one again." An apply that was refused says "Katana 0.3.0 could not be installed; nothing was changed." with its reason, and one refused because an earlier update is not settled yet adds "Update again; if this is said again, close Katana and run: katana_updater --recover ..." | none: it is read and closed |
| CheckFailed | "Could not check for updates: ...", or for a feed that answers 404 "No update information found." / "If the releases are private, add an access token in Settings > Updates." | Retry |
| Unsupported | "Updates are not available for this copy of Katana: ..." | none |

Idle, Checking and NotOffered are not in the design's list and a window
cannot do without them: something is shown before the first answer and while
one is on its way, and `UPDATE CHECK` has two answers that name a newer
release without offering it. The reason in a sentence is a person's words for
the engine's token (`update_reason_words`: `offline` is "this computer is
offline, or the release server did not answer", `http_503` "the release
server answered with error 503", `html_page` - a hotel's or a site office's
sign-in page where the release server should have answered - "a web page
answered in the place of the release server; this network may want you to
sign in first"; a token it does not know is shown as it came, and
`update_words::every_reason_the_engine_gives_has_words_of_its_own` holds the table to the tokens the
engine's headers list: a check's, the transport's, a download's, the helper's
preflight's and an apply's). A megabyte is a million bytes, rounded to
the nearest, and a download is never said to be 0 MB. The files of
"12 of 23 files" are said only when the download's progress says how many are
staged - a word `files=<done>/<all>` in a progress line - and left out
otherwise ("Downloading 31 of 71 MB."). Three variants: a manifest past its
`expires` adds "The update information is old: a newer release may exist."
and refuses nothing; a copy that cannot replace its own files (the macOS
bundle, and Linux until its apply is written) is told of the release, sent to
its notes, and offered neither Update nor Download only; and an apply that
ended `refused` or `rollback_incomplete` is not called restored - the second
names the command that puts the install right by hand, and so does the first
when it was refused over an earlier update ("A refusal over an earlier update
says what to do", below).

Where a person can do something about a reason, its words say what. A Katana
that was started as administrator installs no update - the helper would run
with those rights from a folder ordinary programs can write (`update.md`) -
and `elevated` reads "Katana was started as administrator; start it the
ordinary way, not as administrator, and update again - the download is kept".
`platform`, where this version installs nothing, reads "this copy of Katana
cannot install an update on this kind of computer; the release notes say
where to download it" (`update_words::a_copy_that_may_not_install_is_told_what_it_can_do_about_it`). The
verbs refuse `UPDATE APPLY CONFIRM` for either as an error, and an error is
shown in the verbs' own sentence, which says the same; the table's words are
for the token wherever it arrives without one. And `interrupted_update` reads
"an earlier update is not settled yet", not that it failed: it is what the
helper answers while an earlier update is neither confirmed nor put back
("An update that was never said to start", below;
`update_words::an_earlier_update_that_is_still_open_is_said_to_be_unsettled_not_failed`).

**A refusal over an earlier update says what to do** (2026-10-14). The helper
names the way out of `interrupted_update` in its own `detail` - "... or run:
katana_updater --recover ..." - which goes to a console nobody sees: the
result it leaves for the next start keeps the reason's token alone
(`katana_update::state::LastResult`). So in an install whose window could not confirm the
earlier update, each Restart and update closed Katana and reopened it on
"Reason: an earlier update is not settled yet." with nothing to do, and the
release was Ready again. `update_view_of_last_result` now gives that ending the
command it gives `rollback_incomplete`, and the sentence after the reason is
"Update again; if this is said again, close Katana and run: katana_updater
--recover "C:\Programs\Katana"" - the install's folder, as the platform
writes it. Update again FIRST, because the start that shows the sentence is
what settles most such updates: every start puts back what a crash
interrupted (`katana_update::recover::recover`), and a window of the installed version
confirms an update that was never said to start. Where the refusal comes
again - a window that may not confirm, a release that does not start - only
the helper's recovery, run by hand with Katana closed, settles it
(`update_decisions::an_update_refused_over_an_earlier_one_that_is_not_settled_names_the_command_that_settles_it`).
The sentence is in the command log too, as every ending is. `UPDATE STATUS`
still gives the token alone (`last=refused:interrupted_update`), on every
surface.

**A release that was set aside says where the way back is.** A release that
is installed, does not prove that it starts and is put back goes on the
state's blocked list, and is neither offered nor applied again. One start too
slow for the helper's health check - a virus scanner reading every new file -
does that to a sound release, and until 2026-10-14 the only way back was to
delete the state file by hand, while the dialog said "A later release will be
offered." and stopped. Both sentences that tell of such a release now end "A
later release will be offered, and Settings > Updates offers this one again.":
a check's `update=blocked`, and the last apply's ending when its reason is
`health_exit` or `health_timeout` - the two after which the helper blocks a
release, and no others. After any other ending nothing was set aside and the
sentence is not said: it would send the person to a button that is off
(`update_dialog::only_a_release_that_was_put_back_for_not_starting_is_said_to_be_offered_again_in_settings`).
The button is Settings > Updates' "Offer the version that was put back
again" ("The Updates page"). Once its line was carried out, a dialog that
still said "not offered again" goes back to "Not checked for updates yet.",
as it does after `UPDATE SKIP NONE`, and the next check offers the release.
Each line takes back only what its own list set aside - `BLOCKED CLEAR`
leaves a skipped release as it is shown, `SKIP NONE` one that was put back -
and a reply that gives a reason (`blocked=none reason=no_state`: no update
folder, so nothing was kept) takes back nothing (`update_effect_of_line`;
`update_decisions::a_line_that_was_carried_out_takes_back_only_what_its_own_list_set_aside`).
*Rejected: a button for it in this dialog.* RolledBack is read and closed,
with nothing to press, so that no single press there installs again a
release that has just failed to start; the way back is a deliberate step
somewhere else.

**What each button does.** It builds the line a person would type and hands
it to the window's one executor ("One executor: the command runner"), or
calls one callback (`update_dialog::each_button_builds_exactly_its_line_or_calls_exactly_its_callback`):

| Button | Control name | Does |
|---|---|---|
| Check now | `updateCheckNow` | `UPDATE CHECK` |
| Retry | `updateRetry` | what failed, again: `UPDATE CHECK` after a failed check, `UPDATE APPLY CONFIRM` for a copy that was blocked |
| Update | `updateInstall` | the one click, below - more than one line, so a callback |
| Download only | `updateDownloadOnly` | `UPDATE DOWNLOAD CONFIRM` |
| Skip this version | `updateSkip` | `UPDATE SKIP 0.3.0` |
| Release notes | `updateNotes` | the notes' address opened in the browser - https only; in a headless run, logged as `notes_url=` |
| Restart and update | `updateRestart` | `UPDATE APPLY CONFIRM` |
| Cancel | `updateCancel` | stops the running check or download; what was downloaded is kept, and the next download starts from it |
| Later | `updateLater` | hides the dialog; a Restart and update that has not closed the window yet is taken back |

A line the executor refused is said under the two sentences, in the error
colour (`updateProblemText`), and so is what the workbench could not do at
the last press ("The download did not finish: this network does not allow
partial downloads."), an Update that was refused before it began included;
both go with the next view. No button is a default.

**The lines that take time are jobs.** `UPDATE CHECK`, `UPDATE DOWNLOAD
CONFIRM` and `UPDATE APPLY CONFIRM` - whose preflight copies the helper and
runs its self-test - are run by `katana_app::update_verbs::run_update_line` on a worker thread
("Geoprocessing jobs", below, for the runner: progress and Cancel in the
status bar), with a COPY of the verbs' context taken on the interface thread,
and their reply is logged on the interface thread when they end, as a typed
line's is. Nothing of the workbench goes to the worker. `STATUS`, `SKIP`,
`BLOCKED CLEAR`, `SET` and `TOKEN` read and write small files and are
answered at once; while
a job runs, every line but `STATUS` is refused, because a check records when
it ran in the state file those lines write. One exception, so that nobody is
refused for a job they never started: a check asked for - typed, Check now,
or Help > Check for Updates with nothing known yet - while the AUTOMATIC
check is on its way takes that one over. It is not started twice; the dialog
says Checking, and its answer is logged and shown as a typed check's is.
`Executor::run_workbench_line`
takes `UPDATE` lines before every other family, as the session does, so a
`DOWNLOAD` or an `APPLY` typed on the command line goes exactly the way the
buttons go - the buttons build those very lines. `HELP UPDATE` prints the
verbs' own help, `HELP` alone includes it, and Help > Command Reference shows
it as its Updates section.

**The context** (`katana_app::UpdateContext`) is made once, at first use -
`katana_app::update_verbs::make_update_context` for the surface the window turned out to be,
`Window` or `HeadlessWindow`, which it knows only after `main()` has said so
- and given this process's id and this program's path: what the helper waits
for before it touches a file, and what it starts again afterwards. It is made
again after a token was saved or cleared, since it carries the token it was
made with.

**The automatic check.** `UpdateWorkbench::start`, called by `main()` once an
interactive window is shown, and then an hourly look at whether one is due.
It runs only when ALL of these hold: the window is one a person uses (never
headless); the copy was installed by packaging (a build tree, which has no
install record, stays offline - so does a `--command` run in one); the
state's automatic flag is on; `katana_update::state::check_is_due` says so (every 24 hours and
up to 2 more, the jitter drawn once per process); and the environment
variable `KATANA_UPDATE_DISABLE` is not set to anything
(`update_automatic_check_is_due`; `update_decisions::the_window_checks_by_itself_only_when_every_condition_holds`
takes each condition away in turn). It is QUIET: when it learns nothing -
offline, a 404, a signature that does not verify - nothing is logged and
nothing shown. A release the person skipped, or one that was rolled back
here, is not offered (`katana_update::check::apply_update_state`) - and if the window still
offered it, as Available or Ready, the offer is taken back without a word,
as it is when the feed no longer has the release at all. When there is news,
the line and its reply are logged as a typed check's are and the status-bar
button appears; the dialog is not opened unasked. A release the window
already offers is no news, and what must stay until it is read - how the
last update ended - is not replaced
(`update_decisions::the_automatic_checks_answer_is_announced_shown_without_a_word_or_dropped`).

**One click.** Update is `UPDATE DOWNLOAD CONFIRM`, then - when its reply says
the release is staged - `UPDATE APPLY CONFIRM`, then the window's ordinary
`close()`. Each line goes through the executor, so the log reads as if both
had been typed. `APPLY CONFIRM` changes nothing in the install: it returns
the request the helper is to be started with. The close is the one File >
Quit makes, so the Format managers' unapplied edits and the unsaved-changes
question are asked as ever:

- **Cancel there**, and the window stays, the request is dropped, and the
  dialog says Ready with the release still downloaded. The copy of the helper
  that `APPLY CONFIRM` made for the request (`run-<transaction>` in the update
  state folder) is listed for the sweep the next start makes; so it is after
  Later, and when the person closes the window themselves while the close
  for the update is waiting. The listing is `katana_app::update_verbs::abandon_apply`
  (`crates/katana_app/src/update_verbs.rs`), the one way a prepared apply is given
  up, which the session and `katana_cli` take too; the workbench had a second
  copy of it until 2026-10-14.
- **A job is running** - a geoprocessing run, say - and the close waits for
  it, since a window that closes stops its jobs: the dialog says "Katana will
  close when the running job has finished, then update and reopen.", and
  Later takes the press back.
- **A modal dialog is open** - a file dialog the person opened while the
  release was downloading - and the close waits for that too: it comes from a
  timer, not from a press, and would close the program around the dialog.
  The update dialog says "Katana will close when the dialog that is open has
  been closed, then update and reopen."
- **The close succeeds**, and only then does
  `MainWindow::take_pending_update` hand `main()` the request, after the event
  loop has ended; `main()` calls `katana_update::apply::launch_apply` as its last act, and
  the helper swaps the files once this process has gone. A window the PERSON
  closes while an update waits installs nothing.
- **The helper cannot be started.** The window is gone by then, so `main()`
  says it: on stderr ALWAYS ("the update could not be started: ..."), and the
  run ends with exit 1, as `katana_cli`'s does; the helper's copy goes to the
  sweep (`katana_app::update_verbs::abandon_apply`) and the release stays downloaded. It is said in
  a message box as well - a GUI program's stderr goes nowhere a person looks -
  but only when the run was given no line to run: no `--command`, `--enter`
  or `--run-line`. A run that was given one is not headless (it is a window a
  person may use, shown, with the automatic check), yet it is driven by
  whoever started it - a script, a test, an agent - and nobody may be there
  to close a box: until 2026-10-14 `katana --command "UPDATE APPLY CONFIRM"`
  whose helper did not start waited on that box for ever. Which runs get the
  box is `update_failure_is_boxed`, asked with the kinds of the steps the run
  was given (`update_decisions::an_update_that_could_not_be_started_is_boxed_only_in_a_window_nobody_drove`).
  *Rejected: making every `--command` run headless.* A headless window is
  never shown, restores no session and starts nothing about updates, and a
  run is started with `--command` precisely to set a window up for a person.

**How the last one ended** is shown once. At the next interactive start the
workbench reads the last apply's result from the update state folder, clears
it, and - unless it was a success, which is one line in the log - opens the
dialog on it: Rolled back, or Blocked with the programs to close and a Retry
that applies the still-downloaded release again. It also removes what an
earlier update could not delete - and with it the helper's copy of each
update of this install that a recovery put right. A recovery writes nothing
per user, so nothing in the update state folder names that copy; the
install's own `recovery.log` does, and a copy is removed only once its
update is over (`update_recovered_transactions`; `update.md`, "Recovery").
And it offers again, as Ready, a release that was downloaded in an earlier
session and not installed - with two exceptions, which are the rule that a
skipped or blocked release is not offered (`update_staged_to_offer`):

- one the person **skipped** since it was downloaded;
- one that was **rolled back here**. A release that was installed and did
  not start is put on the state's blocked list, and its downloaded files are
  KEPT (only a healthy update deletes them), so `UPDATE STATUS` goes on
  naming it as downloaded - by version, which says nothing of which build.
  Offered as Ready, Restart and update would install the release known not
  to start here, and after each rollback it would be offered again. So the
  workbench reads the blocked list from the state file and looks in the
  update folder: while the complete download of a blocked build is there,
  nothing is offered as Ready at a start
  (`update_decisions::a_release_that_was_rolled_back_here_is_not_offered_as_ready_at_the_next_start`). A later
  build of the release, once downloaded, replaces that folder and is offered.

A development copy does none of this: a build run from a build tree shares
the per-user update folder with an installed copy on the same machine, and
would otherwise show - and clear - how THAT copy's last update ended.

**An update that was never said to start.** The helper installs a release and
then waits, up to a minute, for the new version to prove that it starts. A
helper that dies in that wait - power, a shutdown, the process ended - leaves
the release installed with nothing recorded either way: not healthy, not put
back, and no result for the next start to show. Recovery leaves such an
update alone (the tree is a whole version), so three later events settle it
(`crates/katana_update/src/recover.rs`; `update.md`): the next logon's
`katana_updater --recover`, the next update, and - the window's part - **a
window of that very version that has come idle**, which is exactly what the
health run exists to show. On its first idle turn, before the automatic
check, `UpdateWorkbench::start` asks `update_start_may_confirm` and then calls
`katana_update::recover::confirm_started` for the install and the update state folder: with
nothing to settle - every start but that one - it is a listing of one folder
and takes no lock; otherwise the update is recorded healthy and the dead
helper's copy is cleared away. Nothing is shown and nothing logged: the
update did work.

- **Only the version that was installed proves it.** The call is made only
  in an interactive window (it writes per-user state) of an installed copy
  that has an install record and whose installed version is the program's
  own (`update_decisions::a_start_confirms_an_update_only_in_a_window_of_the_version_that_was_installed`).
  The installed version is the HIGHER of two builds (`katana_update::install::read_install`;
  `update.md`, "The installed version"): the one the helper recorded in
  `installed.json` when an update committed, and the one the package names
  of itself in `install.json`. The program that asks is not asked: its
  version stands for the package's only under an `install.json` that says
  its target alone, which no package writes now. What this refuses, then, is
  a helper's record that names a version ABOVE the program's. That is the
  case the rule exists for: a window of the OLD files that was started while
  the files were being swapped. It runs old code over the new tree, the
  record names a version it is not, and its start says nothing about the
  release. A helper's record BELOW the package's refuses nothing: a newer
  copy unpacked by hand over one that had updated itself brings its own
  `install.json`, reads as the version of that package, and its window
  confirms. A program that is above both - one put into the copy by hand,
  without its package - is refused as well: nothing on the disk says that
  the copy is its version. The record is read again for this, not taken from
  the verbs' context, which read it when the window was built - during the
  swap that may have been the old record still.
- **A helper that is alive settles its own update.** It holds the update
  lock, and `confirm_started` then answers that it is busy. The window asks
  again every 5 seconds, 12 times - the minute the helper waits at most - in
  case the helper dies instead, and then leaves it to the next start or the
  next logon.
- **It never puts an update back.** A window that runs is evidence that the
  release starts, never that it does not; putting back is for the logon's
  `--recover`, with no program of the install running.

**`main()`** (`crates/katana_window/src/bin/katana/main.rs`, through the program driver's
`recover_interrupted_update`) does three things for updates: before
any window is made, `katana_update::recover::recover` for the install the program is in, so that
an update a crash interrupted is put right before anything else runs (a
listing of one folder when there is nothing to do); the switch
`--update-health`, by which the helper asks a version it has just put in
place whether it starts (`docs/headless.md`, "Updates in a headless run");
and the helper's start described above, with what it says when the helper
cannot be started.

Decisions, and what was rejected:

- *Rejected: the dialog reading the engine.* It would then need a feed and an
  install to be tested, and its sentences would be a second reading of the
  verbs' replies. It shows what it is handed.
- *Rejected: opening the dialog when the automatic check finds a release.* A
  window that opens over the drawing at start-up, unasked, is an
  interruption; the button in the status bar is there until it is wanted.
- *Rejected: logging every automatic check.* One that found nothing would add
  two lines to the command log at each day's first start. What a check sends
  is said where the check is switched on and off (Settings > Updates).
- *Decided: a development copy is told so without a line.* Help > Check for
  Updates in a build tree shows Unsupported from the install's own record,
  which is what `UPDATE CHECK` would answer; nothing is asked of any server,
  in a headless run either.
- *Decided: the health run exits.* The design had the relaunched window write
  the marker and carry on. The engine as built waits for the EXIT of the copy
  it started with `--update-health`, and stops one that is still running when
  its wait ends, so a window that carried on would be closed under the person
  a minute later. The health run is therefore a window that is built, never
  shown, and gone again; the helper then starts Katana as a person would.

Tested in `crates/katana_window/tests/update/update_dialog.rs` (the dialog, the words
and the decisions) and in the real window by
`tests/update/headless/update_window.cmake`:
`qt_the_update_dialog_opens_by_its_name_and_says_a_build_tree_cannot_update_headless`,
`qt_a_copy_that_cannot_update_is_offered_no_check_in_the_update_dialog_headless`,
`qt_help_has_check_for_updates_with_an_icon_a_tip_and_a_letter_of_its_own_headless`
(`--check-menus` and `--check-shortcuts` with the item in the menu) and
`qt_a_typed_update_check_is_run_as_a_job_and_its_answer_is_what_the_dialog_shows_headless`,
which is the one run of the job's whole way - the line, the worker, the wait,
the reply in the log and the view it becomes - that a build tree allows.

Not done:

- **The workbench's wiring has no test of its own.** What it DECIDES is
  tested (`update_decisions`); what it DOES with a decision is not, beyond the
  one job a build tree allows: it needs the window, so it is not in the
  window tests' library, and a development copy never asks a server,
  downloads or applies. A download's job and its progress, the order of the
  one click, the close that waits and the close that is cancelled, the
  request handed to `main()`, the automatic check starting and being taken
  over, and the last result being cleared are exercised only by the update
  system's end-to-end tests and by a release. So is the confirmation of an
  update that was never said to start: whether a window may confirm is
  tested (`update_start_may_confirm`), and what confirming does is the engine's
  and tested there; the call itself, its fresh read of the install record and
  its twelve askings are run by an installed copy alone. And `main()`'s
  answer to a helper that cannot be started - stderr, exit 1, the box - is
  reached by no test: it takes an installed copy whose helper is there for
  the preflight and gone a moment later. Which runs get the box is tested
  (`update_failure_is_boxed`); that `main()` asks it, and says the rest, is not.
  What a start lists from the install's `recovery.log` and then sweeps is
  reached by neither a development copy nor a headless run, so by no test:
  which copies it may list is tested (`update_recovered_transactions`), and the
  sweep is the engine's and tested there (`katana_update::state::process_cleanup_list`);
  reading the log and listing the copies are run by a release alone.
- **A typed check that fails while a release is Ready** leaves the dialog on
  "Could not check for updates" with Retry; the downloaded release is not
  forgotten, and is Ready again after a check that succeeds or at the next
  start, but nothing in the dialog installs it meanwhile.
- **No "Open Releases" button** for a network that does not allow partial
  downloads: the sentence says why the download did not finish, and Release
  notes, in the Available state, is the way to the page.
- **Windows only installs.** Elsewhere a release is told of and not offered
  to be installed (the design's platform table).
- **A start that confirms an update says nothing.** After an update whose
  helper died, the window that settles it logs no "Katana was updated to
  ...", which a start after an ordinary update does from the helper's
  result: there is no result, and the specification of the confirmation asks
  for none. The version is in Help > About and in the update dialog.
- **Settings does not say WHICH version was put back.** The state keeps the
  build that was set aside, by its sequence, and no version; the page says
  only that one is, and the update dialog names it when a check finds it.
- **Settings can still tell of a build that no longer matters, or miss one
  that does, where only a version would say.** A fresh install has no record
  of its own build, so it is told of every build on the list - one an older
  copy set aside included, though the fresh copy may be a later version. And
  a build made BEFORE the installed one, yet of a higher version, is told of
  only once a check has met it and the dialog says so; until then the page
  says "No version was put back." ("The Updates page"). The list itself is
  shortened by `UPDATE BLOCKED CLEAR` alone.

## File > Drawing Summary

The window showed only pieces of the drawing's state - the title, the current
layer, the current style - and nowhere the alignment count, the undo depth,
the full project path or the customisation it was drawn with; the
customisation's coverage was logged once, at a load, and scrolled away. File >
Drawing Summary (`fileDrawingSummary`, `drawingSummaryDialog`,
`crates/katana_window/src/customisation/drawing_summary_dialog.rs`) keeps it all in view,
non-modal and kept, following the Document through a `DocumentWatcher`
(refreshed once a turn of the event loop however many commands ran):

- the project's full path and whether it has unsaved changes;
- entities, layers, alignments and sheets;
- the current layer, style, annotation scale and coordinate system;
- the selection;
- the undo and redo depth with the next step each way;
- the customisation, word for word what a bare `CUSTOMISE` replies
  (`katana_cad::customisation_report::format_customisation_reply` of `katana_cad::customisation_report::customisation_summary(document)`):
  the counts, then the records - the session's name, origin and whether it
  is kept, its sources in load order, the automation, the linework codes,
  the names the project was drawn with that are not loaded - and this
  drawing's coverage. The pane wraps at its edge: the records are longer
  than it is wide (the linework record is 143 characters), and unwrapped the
  end of each was out of sight behind a scroll bar
  (`drawing_summary_dialog::no_record_of_the_customisation_pane_is_cut_at_the_panes_edge`).
  It reads the Document's own state; the window once
  kept the loaded files and the missing ones in lists of its own and handed
  them over, and the pane printed an older prose of them. Then the names the
  drawing's styles give that no loaded library defines
  (`drawingSummaryUnresolved`). A double-click on one, or Show in Styles
  and Linetypes, opens Format > Styles and Linetypes on its Styles tab with the
  Missing chip and the name in the search, reached by the control names the
  manager's module lists (`MainWindow::show_missing_in_styles`), so its styles
  are what the manager shows.

It changes nothing. Copy as JSON runs `STATUS JSON` through the one executor
and copies the reply - exactly what `katana_status` and `katana://status` give
(`docs/cad.md`, "STATUS"). It had a Load Customisation button
(`drawingSummaryLoad`) that triggered the Format menu's item of that name;
the item is gone ("The Format menu", above), and the button with it, since a
trigger of a name that is no menu item fails without a word. The heading over
its customisation report (`drawingSummaryCustomisationLabel`) says where the
customisation is changed now - "File > Settings loads, writes or resets it"
(`drawing_summary_dialog::it_offers_no_button_for_the_removed_load_customisation_item`).
The status bar has a permanent `statusSelectionCount` label, "3 selected / 120
entities", refreshed with the panels.

Tested in `crates/katana_window/tests/styles/drawing_summary_dialog.rs`
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
  (`crates/katana_window/src/plotting/plot_drawing_dialog.rs`): the drawing on one
  sheet as the active plan view shows it, through the `plot_drawing_to_pdf`
  that `--plot` uses; what is not given is `PlotSettings`' default. It logs
  the sentence it always did and a record, `file="..." paper= orientation=
  scale= dpi= style= lineweight=`, with the scale a fitted plot chose. A plot
  that is without part of a surface's style - the contours of a style whose
  interval is too fine to trace are refused - logs a record for each,
  `warning surface="ground" style="Fine" reason="contours: ..."`, between the
  sentence and the `file=` record, which stays the last line; the plot is
  written all the same (`qt_a_plot_of_a_surface_with_contours_too_fine_says_so_headless`),
  and the dialog shows the records in `plotDrawingWarnings`, hidden when a
  plot had none (`plot_drawing_dialog::the_warning_records_of_the_reply_are_shown_and_the_file_record_stays_the_status`). File >
  Plot to PDF (`filePlot`) opens `plotDrawingDialog`, non-modal and kept:
  every field has a control name, the colour mode
  (`plotDrawingColourMode`, Colour / Greyscale / Monochrome) and the line
  weight scale (`plotDrawingLineWeightScale`, 0.10 to 5.00) among them;
  `plotDrawingCommand` shows the line and Plot runs it through the one
  executor. The file is a field, so a headless run fills it; Browse opens a
  file dialog, except headless. The file suggested follows the drawing
  (`MainWindow::suggest_plot_files`, on a drawing opened, made new or saved):
  kept from the first showing, the dialog once plotted a drawing opened
  afterwards into the first project's folder under its name. A path typed in
  the field is left as it is. Plot asks before writing over a file that is
  there, as the save dialog before it did - the drawing's suggested PDF is the
  sheet set's too (`suggested_plot_file`) - and a headless run writes it, as the
  verb does (`qt_the_plot_dialogs_file_follows_the_drawing_opened_headless`).
- **`SNAPSHOT <file.png|.jpg|.tif> | CLIPBOARD [width=N] [height=N]
  [scale=F] [bg=theme|white|none] [view=plan|3d]`**
  (`crates/katana_window/src/plotting/view_image_export.rs`): a picture of the plan or
  3D view. The plan view is painted afresh at the size asked by the plan
  painter (`PlanView::render_to_image`, read-only: the view's own kept
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

Both are the window's verbs, beside `PLOTSHEETS`: the painter is the window's, which
`katana_cli` and `katana_mcp` do not have, and which refuse the lines by name,
saying where they run (`docs/mcp.md`, "What the server adds to the command
line"), rather than as unknown commands. A headless run has `--plot` with
the same settings, and an agent driving the window types the lines (the
Command Reference's Window section lists them).

Tested in `crates/katana_window/tests/sheets/plot_drawing_dialog.rs` and
`crates/katana_window/tests/sheets/view_image_export.rs` (the grammars, the
dialogs' lines, the sizes, each file format, and the plan painted into an
image), by `qt_plot_and_view_image_dialogs_run_their_verbs_headless` (both
dialogs driven by control name, Copy View as Image, a typed transparent PNG,
the 3D view grabbed; the dialog's image size read from the PNG's header), and
by `qt_plot_headless`, whose `PLOT` script prints the same sheet in each
style: the colour plot has coloured pixels and the greyscale and monochrome
ones none, the monochrome one fewer mid greys than the greyscale one, and
line weights times 4 and times 0.25 more and less ink than times 1.

Not done: printing to a printer. The sheets' Plot dialog offers a Printer
(`plotPrinter`), but the window knows no printers yet, and a print writes the
sheets as a PDF and stops there, saying where the PDF is: handing it to the
operating system's print queue (`hand_to_print_queue` in
`crates/katana_window/src/plotting/plot_output.rs`) answers Unsupported
(`docs/plotting.md`; `docs/rust.md`, "Not done"). Print the PDF from a viewer
meanwhile.

## Undo and Redo lists

Edit > Undo and Redo move one step a click; `UNDO n` and `REDO n` have always
moved several, but only for someone who typed them. The Edit toolbar's Undo
and Redo are now split buttons (`editUndoButton`, `editRedoButton`): the
button is one step, as before, and the arrow - in a strip of its own beside
the icon ("Tool families on the toolbars", above) - drops down the history
(`editUndoMenu`, `editRedoMenu`) from `CommandStack::undo_names` and
`CommandStack::redo_names`, the next step first. The k-th entry (`undoStepK`,
`redoStepK`, so `--trigger undoStep3` reaches it) runs `UNDO k` or `REDO k`
through the one executor: one line in the log, and what it did said as the
typed line says it. Its status tip is that line and what it covers: "UNDO 3:
the last 3 steps, down to ..." and "REDO 3: the next 3 steps, down to ..." -
the Redo list's once said "the last" too, which REDO does not do. The lists show 25 steps each way, with a last line saying
how many more there are and that `UNDO n` reaches them; they are refilled with
the panels. The names are the commands' own (`CREATE_POINT`), the words the
Undo item and the Drawing Summary use. Tested in
`crates/katana_commands/tests/command_stack_names.rs` and by
`qt_the_undo_and_redo_lists_step_through_the_history_headless`. Not done: an
Undo History dock with the save point marked is not built.

## GIS > Online Data: a workbench of its own

The online import (`docs/gis_online.md`) is `OnlineDataWorkbench`
(`crates/katana_window/src/gis_online.rs`), built as the Survey and Format workbenches
are: the window hands it the GIS menu as it makes the GIS actions, and it
reaches the document, the views, the log, whether the session is headless,
and the reference data through the window's host; `MainWindow::run_command_line`
hands it any line that starts with `ONLINE`. Nothing else of it is in the
main window. Its dialog (`crates/katana_window/src/gis_online_dialog.rs`) is
non-modal, has a control name on every control, and imports only by building
the `ONLINE IMPORT` command the verb would parse and handing it back to the
workbench - one executor, so no path exists in the dialog that an agent cannot
take on the command line. The download runs as a background job with its
progress and Cancel in the status bar; the entities arrive as one command.

## Geoprocessing jobs: GDAL on the window's command line

The geoprocessing verbs (`docs/geoprocessing.md`) - GDAL, and the families
the geoprocessing packages add - are `GeoWorkbench`
(`crates/katana_window/src/geo/geo_workbench.rs`), built as the online and utility
workbenches are. The window hands it its document, interpreter, reference
rasters and surfaces through the host. `run_workbench_line`
asks it before the online and utility workbenches, so a typed line and a
dialog's line through `runVerbLine` reach the same executor `katana_cli` and
`katana_mcp` run. The window links `katana_app` for it: the window never has
a second implementation of a session verb.

- **Prepared on the GUI thread.** The line is read, its scope resolved and
  what the run needs copied there. What answers at once - LIST, HELP, a
  PREVIEW, a refusal - is logged there.
- **Run as a job.** Anything that runs is a background job (`jobs.rs`), with
  progress and Cancel in the status bar. The job's Apply runs the executor's
  apply, as one undo step.
- **Headless, the line waits for its job,** so what it did is logged, and
  captured by `runVerbLine`, before the next line.
- **Interactively it logs `job id=<n> title="..." state=started`,** and when
  the job ends it tells the window's geo services' `finished` and any listener a dialog
  added (`GeoWorkbench::add_finished_listener`). A cancelled job applies
  nothing, whenever the cancel arrived.

Its menu items come from one table with a block per package
(`crates/katana_window/src/geo/menu_table.rs`, `GeoMenus`). The GIS sections and the
Terrain submenus are made only when an item is added, so no empty heading
shows. The GDAL verb's item is GIS > Processing - GDAL > GDAL Toolbox.

**The GDAL Toolbox** (`gdalToolboxDialog`, `crates/katana_window/src/geo/gdal_toolbox_dialog.rs`)
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
the window's geo services' `run`, the window's one executor. `GeoRunPanel`
(`crates/katana_window/src/geo/geo_dialog_support.rs`) is the Command, Preview, Run and
Reply they share: an interactive run answers `job id=<n> ... state=started`,
so the panel shows "running" and takes the reply from the workbench's
finished listeners when the job ends; a headless run's reply is the runner's
own. A dialog is kept once made, a child of the window, so `--dialog` finds it
by the name its action carries. Their scope and filter controls are Global
Modify's (`ScopeFilterWidget`), given the window's views through
the window's geo services' `views`.

**The terrain dialogs** (Terrain > Surface From, GIS > Export Surface as DEM,
and the Terrain > Analysis items) share
`crates/katana_window/src/geo/terrain_dialog_support.rs`. Each writes its line into a
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
letters, and `geo_menus::every_item_the_lanes_add_has_a_letter_of_its_own` now fails an
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
  a plan view (`samplePick`, through `PointPick` and the workspace's `ViewAccess::pick_point`: the next
  left click; Esc or a right click cancels) (`docs/terrain.md`, "Sampling
  and drape").
- `terrainViewshed` "Viewshed and Line of Sight..." opens `viewshedDialog`,
  two tabs: Viewshed writes a RASTER VIEWSHED line (the ground, observers
  typed or picked - or, with `viewshedUseScope`, the points the shared
  scope controls take - the eye and target heights, how far to look, the
  curvature, a layer for the visible area and the raster's name), and Line
  of Sight a LOS line (the two ends, each typed or picked, and the heights)
  (`docs/terrain.md`, "Viewshed and line of sight").
- The three are driven by control name through the real window headless,
  filled, run and their replies read (`tests/geo/headless/analysis.cmake`;
  `docs/terrain.md`, "Viewshed and line of sight", "Tests in the window").
  The pick buttons are not: a pick waits for a click in a plan view, which
  a headless drive does not make, so the points are typed.

**One store of surfaces, the drawing's.** The surfaces are the Document's
(`Document::surfaces`; `docs/terrain.md`, "Surfaces in the drawing"), saved
with the project and changed one undo step at a time, so `SURFACE <name>`
finds the same surface on every front end and the window keeps none of its
own. `Executor::sync_scene_surfaces` rebuilds the views' list from the
Document whenever its generation moves - at once, inside the notification,
because the list points at surfaces a step may have replaced - keeping how
each was shown and taking whether it is shown from the drawing. Surfaces
are shared and immutable, so a job may read one while the views draw it.
The store keeps names unique: a second surface of a name is "name (2)".
Terrain > Surfaces Panel (View > Panels > Surfaces) lists them
(`crates/katana_window/src/terrain/surfaces_panel.rs`).

**The GIS menu's option dialogs are one file each** - `gis_import_dialogs`,
`gis_export_dialog`, `surface_raster_dialog`, `dataset_info_dialog`, with
their shared OK/Cancel row in `gis_dialog_support.rs` - split from one
`gis_dialogs` file without a change in behaviour, since four geoprocessing
packages each extend one of them.

## File > Import IFC and Export IFC

IFC 4.3 both ways (`docs/ifc.md`) has two entries of its own in File,
`fileImportIfc` ("Import IFC...") and `fileExportIfc` ("Export IFC..."), beside
Import Any File and Export Drawing in File's two submenus ("The File menu",
above) rather than inside those two: what an IFC exchange chooses
- which of the drawing's objects go, an AS 5488 utility schedule and the
delivery schema it was written to, a project's classification rules - has no
place in the generic dialogs. Each opens a dialog (`IfcImportDialog`,
`IfcExportDialog`, `crates/katana_window/src/ifc_dialogs.rs`) that the window makes on
first use and keeps: non-modal, a File field with a Browse button rather than a
file dialog asked up front, and every control named at the head of its module, so that a
headless run drives it (`--dialog fileExportIfc`, `--fill`, `--press`) and
Browse, headless, says to type the path instead of opening a box.

**The dialogs write lines, and do nothing else** - the pattern of the
Subsurface Utilities dialog below. Every choice either offers is a word of the
verb: `EXPORT <file.ifc> ... [NOENTITIES] [SELECTED] [NOALIGNMENTS]
[NOSURFACES] [NOROADMODEL] [PREVIEW]` and `IMPORT <file.ifc> [LOCAL] [NOALIGNMENTS]
[NOELEMENTS] [NOSURFACES] [TOLERANCE <m>] [TAKECRS | KEEPCRS]`, with `INFO`
and `IFC RULES` (`docs/ifc.md`, "Using it"). A dialog writes the line its
fields mean (`katana_ifc::front_end::format_export_line`, `katana_ifc::front_end::format_import_line`), shows it as it
is edited in `ifcExportCommand` and `ifcImportCommand`, and hands it to the
window's command line through its context's `run_line`
(`MainWindow::run_ifc_command`, `crates/katana_window/src/main_window_ifc.rs`), which
echoes it as typed and runs it by `Executor::run_ifc_line` - the code a typed
line runs, which reads it with the grammar `katana_cli` reads it with
(`crates/katana_ifc/src/front_end.rs`). So there is nothing a dialog can do that
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
(`katana_ifc::front_end::read_export_objects`, the one reader of what `katana_ifc::front_end::format_export_reply`
writes): the writer's own account (`IfcExport::tally`), made by writing
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
`EXPORT` and `INFO` of every other file, `Executor::run_workbench_line` asks
`Executor::run_ifc_line` first, before the geo workbench: a typed line and a
dialog's line through `runVerbLine` reach the workbench before
`dispatch_line`, so without it the executor read an IFC line's options as
part of a path (`qt_ifc_typed_export_and_import_are_the_command_lines_headless`
failed exactly so). The session does the same (`crates/katana_app/src/session.rs`).

A .ifc reaches the same line from everywhere else a path enters: File > Import
and its IFC filter and a path given to the window run `IMPORT "<file>"`, the
typed `IMPORT`, `EXPORT` and `INFO` are split by the shared grammar before the
window's generic one takes the rest of the line as a path, the GIS imports'
"All files" and GIS > Dataset Information describe it
(`MainWindow::describe_ifc_file`, `INFO`'s reply), and `--import-options` builds
the import dialog. Choosing IFC in Export Drawing's file dialog opens the
export dialog with the file filled. The Command Reference lists the window's
own verbs after the interpreter's.

An import asks what the DXF import asks - a file far from the drawing is
shifted alongside or kept - weighing everything it brings against everything
the drawing holds, alignments and its surfaces too. When the file
names an EPSG code and the project has none, `TAKECRS` takes it and `KEEPCRS`
does not; the import dialog always says one (its "Take the file's coordinate
system"), File > Import and a path given to the window, which say neither,
ask the person there, and a typed `IMPORT` or a headless session asks no one
and says how (`CRS SET`) - `MainWindow::IfcLineFrom` names who ran the line.
The change is its own undoable step, after the import's. Declining the
far-apart question cancels the import (`CommandRejected`), which the dialog
says as a cancel, not a failure. Data shifted or moved to the origin is in no
system, and takes none. Terrain comes in as surfaces of the drawing, in the
import's one undo step (`katana_cad::terrain::surface_verbs::import_surfaces`), which the export also writes
out - the one exchange that carries the drawing's surfaces both ways besides
the .12da archive.

`ifc_export_dialog::the_line_carries_every_choice_as_the_verb_reads_it`,
`ifc_export_dialog::preview_shows_the_writers_account_class_by_class` and the
rest of `ifc_export_dialog::*` and `ifc_import_dialog::*`
drive the dialogs against a command line that records their lines and answers
as the verb does; the `qt_ifc_*_headless` checks (`tests/CMakeLists.txt`)
drive each way in through the real window - the two dialogs, whose echoed
lines they read, the typed verbs, a path given to the window,
`--import-options`, Dataset Information, a file far from the drawing's
alignment, a missing file, and surfaces out and back.

## Survey > Subsurface Utilities (AS 5488): the same pattern, one verb

The AS 5488 tools (`docs/subsurface_utilities.md`) are `UtilityWorkbench`
(`crates/katana_window/src/survey/utility_workbench.rs`), built as the Online Data
workbench is: the window makes it after the Survey workbench, as it makes the
Survey actions, and hands it the Survey menu, which it ends with the section
"Subsurface Utilities (AS 5488)" - Draw Utility Schedule, Utility
Investigation Report, Verify Detections Against Exposures, Clearance of
Proposed Works, Check Against a Delivery Schema, Regrade Drawn Utilities,
Export Drawn Utilities as a Schedule (`utilityDraw`, `utilityReport`,
`utilityVerify`, `utilityClearance`, `utilityCheck`, `utilityRegrade`,
`utilityWriteSchedule`; N and X were the letters the Survey menu had left).
`MainWindow::run_command_line` hands it any line that starts with `UTILITY`,
before a running tool can take the line for an answer - unless that tool is
waiting for typed text (`ViewWorkspace::tool_takes_text`: a Text's string, a
count), whose answer the whole line is, as it is for a bare `ZOOM`: a label on
a services plan may well read "Utility pit". `ONLINE` gives way to such a
tool the same way (`qt_utility_line_leaves_a_text_to_the_tool_headless`).

The verb is the `CommandInterpreter`'s, shared with `katana_cli`; the
workbench runs it through the window's interpreter and adds the one thing
only a window has: after a `UTILITY DRAW` or a `UTILITY REGRADE` that worked,
every plan view is framed on the reply's `bounds=` box
(`katana_cad::utilities::utility_verbs::draw_reply_bounds`, `ViewWorkspace::zoom_to`). A schedule in a real coordinate system lands far
from whatever the view was showing, and without the framing a draw that
worked looked like one that did nothing - the lesson Online Data learnt. The
box is read by the verb's own function, not a second parser in the window,
so the record's format has one reader to keep in step with its writer
(`katana_cad::utilities::utility_drawing::format_utility_drawing`); the two branches that built this each
had one, and the window's was dropped when they were merged.

All seven items open ONE non-modal dialog (`UtilityToolsDialog`,
`crates/katana_window/src/survey/utility_dialog.rs`, control name `utilityDialog`), each
on its own tab; the module lists every control's name. The detected
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
`verb_words()` are the line's scope and filter words, so Report on the
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
(`utility_dialog::a_scope_that_takes_no_utility_line_is_said_so_and_nothing_is_said_regraded_or_written`).

`utility_command_line` - a pure function of
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
(`UtilityToolsDialog::suggested_file_name`). A headless session
opens no file dialog: Browse and Save As say so, and a script fills the path
fields instead. Tested in `crates/katana_window/tests/utilities/utility_dialog.rs`
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
(`utility_dialog::a_draw_of_the_survey_in_the_drawing_runs_the_verb_and_says_when_nothing_was_left_to_draw`).

**The window answers `VIEW`.** The scope word `VIEW` (`docs/cad.md`, "Scope
and filter") is the window's: `MainWindow` gives its interpreter
`katana_cad::scope_verbs::scope_view_of` through
`CommandInterpreter::set_scope_context`, next to the sheet context. No id is
the plan view Plot and the Standard Views act on (`ViewSet::most_recent`); an
id is that open view, of any kind, with its own hidden layers and, for a
plan view, `ViewTransform::visible_world_bounds` as it is when the line runs. So
`MODIFY VIEW WHERE TYPE=point SET COLOUR=#FF0000` typed in the window acts on
the points the plan view shows, where `katana_cli` refuses `VIEW` in favour
of `AREA` (`qt_modify_view_takes_what_the_plan_view_shows_headless`). The
rule lives in `katana_cad`, not a widget, so it is tested without a window
(`scope_verbs_test::a_workspaces_view_set_answers_view_as_the_window_does`).

## Global Modify

Format > Global Modify... (`GlobalModifyDialog`,
`crates/katana_window/src/customisation/global_modify_dialog.rs`) states the request
`katana_cad::global_modify::plan_global_modify` takes (`docs/cad.md`, "Global Modify") and decides
nothing of its own. Non-modal, one instance kept by the Format workbench
(`CustomisationWorkbench::show_global_modify`), as the managers are:

- **Apply to**: the selection, a view, the checked layers (with their
  sublayers or not), or the whole drawing. The views are the workspace's
  open ones, read afresh each time through `GlobalModifyDialog::views`
  (`scopeFilterViews(ViewSet&)`), so a closed view is never pointed at and a
  plan view's "only what is on screen" is its visible area as it is now; the
  dialog itself never sees the workspace, which is what lets a window test
  give it views of its own.
- **Only those that match**: types, layer and style patterns, colour,
  property and value, text, drawn only.
- These two groups are `ScopeFilterWidget`
  (`crates/katana_window/src/customisation/scope_filter_widget.rs`), the ONE set of
  scope and filter controls every dialog on drawing data shows (`docs/cad.md`,
  "Scope and filter"). Global Modify gives it the prefix `globalModify`, so its controls kept
  their names and its tests pass unchanged. It says what it holds twice from
  the same controls: `scope()` and `filter()`, what `match_entities` takes and
  Global Modify plans with, and `verb_words()`, the same as the shared
  grammar's words (`katana_cad::scope_verbs::format_scope_words`), for a dialog that builds a verb
  line. A view is named by its id, which the view list shows beside its
  title - "Plan 2 (VIEW 3)" - since the number in a title counts only the
  views of its kind; "Only what is on screen" unticked is
  `VIEW <id> EXTENTS`, a word added to the grammar for it, since without it
  the controls could say what no line could. What a line cannot say - a `:`
  in the property's name, a comma in a layer's - `verb_words()` refuses
  rather than write words that take something else; a value with the
  property left empty is `PROP=:value`, any property's. The fallback "Whole drawing
  view" of a dialog with no workspace is read by `scope()` and refused by
  `verb_words()`: a line can name only an open view.
  `crates/katana_window/tests/foundation/scope_filter_widget.rs` shows every
  scope and filter said in words, and those words, read back by the one
  parser and resolved with the window's answer for `VIEW`, taking exactly
  what the controls take.
- **Modify**: three tabs - Entities, Their Layers, Their Styles. A field
  changes only when its box is ticked, and its editor is disabled until then,
  so an unticked field can never be written by accident (the rule "`<varies>`
  protects" states for the attribute manager). Their Styles' **Scale symbol
  size by** (`globalModifySetStyleSymbolScale`, `globalModifyStyleSymbolScale`,
  2 to start, 0.001 to 1000) multiplies the symbol size of every style the
  scope reaches - `MODIFY ... SET STYLE.SYMBOLSCALE=` (`docs/cad.md`, "Global
  Modify"); ticked with Symbol size, the two are refused together in the
  summary.
- **The summary** under the form is the plan's own, refreshed as the form is
  edited (coalesced to one preview per 150 ms) and as the drawing or the
  selection changes. Apply is one undo step and logs the summary. Text that
  does not read - a colour that is not `#RRGGBB` or ByLayer - is said in the
  summary, in red, and nothing runs.
- **Selecting by kind** (the owner's "select elements by their type such as
  selecting leaders", 2026-11-03):
  - Each type box says how many of its kind the scope and the REST of the
    filter take - "Leader (774)", "Arc (0)" - refreshed with the preview:
    what ticking that box would select. A ticked box does not count itself
    out. The numbers are `katana_cad::scope_verbs::count_by_type`, which `SELECT`'s `types=` uses
    too; the shared widget shows them only when a dialog gives them
    (`ScopeFilterWidget::show_type_counts`), so the other dialogs keep plain
    names.
  - Select Matches runs `SELECT <the scope words>` through the window's
    executor - `SELECT DRAWING WHERE TYPE=leader` - so the line is in the log
    and typeable, and selects by `SELECT`'s rule (`docs/cad.md`, "Select by
    kind"): what is hidden or locked is counted and left out, and the summary
    says how many ("1 entity selected. 2 hidden or on a locked layer left
    out."). It then frames the selection in the active plan view. It used to
    call the matcher itself, selecting locked entities no click could, with
    no line in the log.
  - Opened with nothing selected, "Apply to" is the whole drawing: with the
    selection as the scope, ticking Leader found nothing, which is how the
    type boxes went unnoticed. It is AutoCAD's Quick Select rule, and it is
    applied only as the dialog opens, never to a scope chosen while it is
    open.

Tested in `crates/katana_window/tests/styles/global_modify_dialog.rs`.

Not done: Apply still runs the plan itself (`document().execute`) rather
than a `MODIFY` line through the executor, as a dialog is to ("One
executor: the command runner", above). Every field has a `MODIFY ... SET` key but one: `SET PROP=key:value`
infers the value's type from its text, where the dialog lets it be chosen
(text "12" or the integer 12), so that needs a word of its own first.

## Terrain > Alignment Manager

The alignments, their PIs, design profiles and setting-out tables, in a
non-modal dialog the window keeps (`MainWindow::show_alignment_manager`,
`crates/katana_window/src/alignment_manager.rs`): every edit an `ALIGN` line through
`runVerbLine`, the PI and PVI grids buffers applied by one line each. The
decisions are in `docs/cad.md`, "The Alignment Manager". The Terrain menu
opens with it, in an Alignments section, and the toolbar with its button.

## Terrain > Volumes

Earthwork volumes between the drawing's surfaces in one non-modal dialog the
window keeps (`volumesDialog`, `crates/katana_window/src/terrain/volumes_dialog.rs`,
`showVolumesDialog`): a page per method, the boundary from the shared scope
and filter controls or typed corners, factors, the results table, the mass
haul diagram, Copy, Save CSV and Make Cut/Fill Map. Every one of them is a
`VOLUME` or `SURFACE DIFFERENCE` line through `runVerbLine`; the methods and
the decisions are in `docs/volumes.md`. The Terrain menu has it in a Volumes
section after Corridors (`terrainVolumes`), and the Terrain toolbar its
button.

## Terrain > Surface Styles

The drawing's surface display styles in a non-modal dialog the window keeps
(`MainWindow::show_surface_style_manager`,
`crates/katana_window/src/terrain/surface_style_manager.rs`): every field of a style in
four tabs with a live preview drawn through the plan view's own painter, and
every button a `SURFACESTYLE` or `SURFACE STYLE` line through `runVerbLine`,
the form a buffer applied by one line naming only what changed. It is in the
Terrain menu's Surfaces section (letter Y) with the toolbar button beside the
Surface From items, icon `SurfaceStyles`. The record is
`docs/surface_display.md`, "In the window".

## Settings

What the session's customisation IS, and the few things that are done to it
whole, in one non-modal dialog (`SettingsDialog`, `settingsDialog`,
`crates/katana_window/src/customisation/settings_dialog.rs`). The lines it runs are the
`CUSTOMISE` family's (`docs/customisation.md`, "The verbs"), and what "kept"
means is in "Settings and the kept customisation" there; this is the dialog.

**File > Settings** (`fileSettings`, "Settings...", above Quit: "The File
menu", above). The action and the dialog are the Format workbench's
(`CustomisationWorkbench::show_settings`): the dialog is made the first time
it is asked for and kept, non-modal, and deleted with the workbench before
the Document - as the managers are, and for their reasons. The workbench
hands it a `SettingsContext`:

- the Document, the window's one executor, the log and whether the session
  is headless, as the window gave them to the workbench;
- whether the Survey Code Manager holds unapplied edits, which only the
  workbench knows, asked at each press - a manager not made yet holds none;
- callbacks that open the code manager, the code manager at its Linework tab
  (`CustomisationWorkbench::show_linework`: the tab is brought forward by the
  control names `codeManagerTabs` and `lineworkTab`, as a person reaches it,
  and a tab that cannot be found is said in the log) and the Symbol Library;
- what the session's host offers: whether there is a built-in customisation,
  and the kept file's path as text (`CustomisationServices::has_built_in`,
  `kept_file`; "How the window starts" says where the file is). They are read
  once, when the dialog is first opened - a host does not change while a
  session runs;
- the built-ins the chooser offers (`SettingsContext::built_ins`). The window
  leaves it empty, and the dialog then lists those compiled into the program
  (`katana_cad::customisation_host::built_ins()`); a window test gives its own.

Every member has an empty default, and a button whose callback is empty is
disabled, so a window test builds the dialog on its own
(`crates/katana_window/tests/settings/settings_dialog.rs`) and through the
workbench (`crates/katana_window/tests/workspace/customisation_workbench.rs`).
The action's data names the dialog, which is how `--dialog fileSettings`
finds it in a headless run.

A page list (`settingsPages`) beside a stack (`settingsStack`). Two pages:
Customisation (`settingsCustomisation`), which the table below and the rest
of this section describe, and Updates (`settingsUpdates`; "The Updates page",
at the end of this section). A third is one more `add_page`.

| Part | Control names | What it shows or runs |
|---|---|---|
| What is active | `settingsActiveName`, `settingsActiveOrigin`, `settingsActiveBuiltIn`, `settingsActiveDefinitions`, `settingsActiveSymbols`, `settingsActiveRules`, `settingsActiveKept`, `settingsActiveProblems` | the name; the origin in plain words (None, Built in, Kept by you, Loaded this session, Edited this session); the built-in the session is (`Katana Standard (default)`, `NSW`) or, for a kept or edited copy of one, `Based on NSW`, else `None`; how many definitions, how many of them symbols, and the rules over how many codes; "Kept" or "Not kept" with the kept file's path, or - when the session has no kept file - "Nothing is kept: the next start gives the built-in customisation" (or "no customisation", where the program has none) and the variable that names a file to keep one in; what the START of the session found, in the error colour, hidden when it found nothing |
| The notice | `settingsShowNotice`, `settingsNotice` | the author's notice, read-only and hidden until asked for: the customisation's own, then each source's under `From <name>:`. The toggle is hidden while there is none |
| Import | `settingsImportPath`, `settingsImportBrowse`, `settingsImportReplace`, `settingsImport` | `CUSTOMISE "<file>"`, or with Replace instead of merging ticked `CUSTOMISE REPLACE "<file>"` |
| Export | `settingsExportPath`, `settingsExportBrowse`, `settingsExport` | `CUSTOMISE EXPORT "<file>"` |
| Built-in and kept | `settingsBuiltInRow`, `settingsBuiltIn`, `settingsBuiltInUse`, `settingsReset`, `settingsKeep`, `settingsRevert` | the built-in library: a combo of the built-ins the program has (each item the customisation's name, the default marked ` (default)`, its tip the description, what it holds and the file it was compiled in from) and Use, which runs `CUSTOMISE BUILTIN "<name>"`, followed by `KEEP` like a reset, which puts the kept file away as the `.bak` (the tip says so first; "Use does the same, twice over", below); the row is hidden when there are fewer than two and Use is disabled while the chosen one is the one the session is. Then `CUSTOMISE RESET`, `CUSTOMISE KEEP`, `CUSTOMISE REVERT` |
| When survey data comes in | `settingsAutoCodes`, `settingsAutoLinework` | `CUSTOMISE SET auto.codes=on` or `off`, `CUSTOMISE SET auto.linework=on` or `off`, when the box is toggled |
| Linework control codes | `settingsLinework`, `settingsEditLinework` | the seven spellings, read-only, each under the word its editor labels it with (`Start: ST, End: END, Close: CL, Begin curve: BC, ...`; `(off)` for a control switched off), and a button that opens that editor, the Survey Code Manager at its Linework tab |
| Editors | `settingsOpenCodes`, `settingsOpenSymbols` | open the Survey Code Manager and the Symbol Library |
| Status | `settingsStatus`, `settingsClose` | each line run, as the command log echoes it (`> CUSTOMISE RESET`), then what it answered: the reply, or `Refused:` and the verb's own words, in the error colour. The names a load reports as undefined that the built-in ITSELF leaves undefined (`undefined name=... in_built_in=yes`) are not listed after every load of anything: they are said once, as a count - `3 names undefined (as in the built-in)`, or `3 names undefined, 2 of them as in the built-in` with the load's own named below (`settings_dialog::the_names_the_built_in_itself_leaves_undefined_are_shown_as_a_count_and_the_rest_by_name`). A file of another program's format is refused in the verb's words, `not a Katana customisation file; the older formats are converted with katana_customisation_convert (a developer's tool, not installed; see docs/customisation.md)`, with none of the JSON parser's sentence |

**Every button that changes anything builds a line** and hands it to the one
executor ("One executor: the command runner"), so it is echoed in the command
log and a refusal is the verb's. There is no Apply: a control runs its line
when it is pressed, a box when it is toggled - `toggled`, not `clicked`, so
that a box the headless driver fills (`settingsAutoCodes=off`) runs its line
as a click does.

**The page shows what the Document says**, never what a button hoped for. It
is read again from the Document's change notification, through a
`DocumentWatcher`, when one of three counters has moved:
`Document::customisation_generation`, `Document::library_generation` and
`Document::survey_map_generation`. The first alone is not enough - an editor's
second commit in a session already marked edited moves only the library's or
the map's, and the counts would stand still
(`settings_dialog::the_labels_follow_the_document`). A box whose line was refused, or that nothing
ran, goes back to what the Document has, since no notification comes to say
that nothing changed; reading the boxes from the Document runs no line
(`settings_dialog::refreshing_from_a_document_change_runs_nothing`).

**What the start found stays on the page.** A kept file that did not read,
a built-in that did not parse, a kept customisation made from another
built-in than this program has: the window says each once, at start-up, in a
log that has scrolled away by the time a person opens Settings to ask why
their own customisation is not the one loaded. The Document keeps them
(`CustomisationState::start_problems`, `keptFromAnotherBuiltIn`), and the row
`settingsActiveProblems` shows the sentences as `katana_cad::customisation_host::start_customisation`
wrote them, one a line, then - for the second - the window's own start-up
sentence with Reset to Built-in where that names `CUSTOMISE RESET`. They are
of the start: a load or a reset since does not take them off the page
(`settings_dialog::what_the_start_of_the_session_found_is_shown_and_the_row_is_hidden_when_it_found_nothing`,
`settings_dialog::a_kept_file_that_did_not_read_when_the_session_started_is_said_on_the_page`).

**What is edited in Settings is kept.** A session that was kept before
Import, Reset to Built-in, Use or a toggled box, and is not kept after it, is kept
again: the dialog runs `CUSTOMISE KEEP` behind its own line, and the status
shows both lines and both answers. Only when the host has a kept file, only
when the first line was carried out, and never behind Export, Keep or Revert
to Kept. Whether to keep is asked of the Document after the line, not
inferred from the line: one that was refused, or a reset that left the
session the kept one, is followed by nothing. A load typed on the command
line or run by a script is not followed either, and the page then says "Not
kept" beside an enabled Keep
(`settings_dialog::with_the_verb_itself_an_edit_of_a_kept_session_is_written_to_the_kept_file` runs it with
the verbs themselves and reads the kept file back).

**A reset that is kept sets the kept file aside.** Reset to Built-in on a kept
session is followed by `CUSTOMISE KEEP` like any other edit made here, and
`KEEP` writes no copy of the built-in: it renames the kept file to
`<kept file>.bak` (`docs/customisation.md`, "The host: RESET, KEEP and
REVERT"). Nothing is then kept, so Revert to Kept is refused in the verb's
words - "no customisation is kept" - and is NOT the way back. An earlier
revision of this section said it was. The way back is the file that was set
aside:

- **in Settings, Import it with Replace instead of merging ticked.** Its
  definitions take the place of the whole library and its rules of all the
  codes, each kind it holds, and its control codes and switches are set as it
  says them; the session being the kept one, the import is kept, in a kept
  file written anew
  (`settings_dialog::after_a_reset_of_a_kept_session_revert_has_none_to_read_and_the_file_set_aside_is_the_way_back`).
  A kind the file does not hold stays the built-in's, and so does a colour
  name it does not give;
- **or, outside the program, give the file its own name back** and press
  Revert to Kept, which reads it exactly as it was
  (`settings_dialog::the_file_set_aside_given_its_own_name_back_is_read_by_revert_as_it_was`).

Only until the next `KEEP` that finds a kept file, because ONE earlier file
is kept. The first edit made in Settings after the reset writes a new kept
file and leaves the `.bak`; the second sets that new file aside in the
`.bak`'s place, and the customisation the reset put away is then in neither
file (`settings_dialog::the_file_a_reset_set_aside_lasts_only_until_the_second_edit_kept_after_it`).

No question is asked before the press, so the button says it first. On a kept
session whose host has a kept file, Reset to Built-in's tip names the `.bak`,
says that Revert to Kept will have none to read, and gives the way back
(`settings_dialog::reset_says_before_it_is_pressed_that_a_kept_sessions_file_is_set_aside`). Keep's tip
says the same of a session that is the built-in and not kept - what a reset
typed on the command line leaves - where a press writes nothing and sets the
kept file aside
(`settings_dialog::keep_of_a_session_that_is_the_built_in_writes_nothing_and_sets_the_kept_file_aside`). A
reset that is NOT kept - typed, or made here on a session that was not the
kept one - leaves the kept file where it is, and Revert to Kept then does go
back (`settings_dialog::a_revert_of_a_session_that_was_reset_and_not_kept_reads_the_kept_file_again`).

**Use does the same, twice over.** Use is followed by `CUSTOMISE KEEP` on a kept
session like Reset, and the kept file does not stay: choosing an alternate
writes it over the kept file (what was there goes to the `.bak`), and choosing
the built-in of the run sets it aside as the `.bak` - in both, replacing the
`.bak` that was there. So a kept customisation of the person's own is in neither
file after a second Use (an alternate, then the default), the case a combo makes
natural. Use's tip says it before the press, in Reset's words: four forms, by
the host (a kept file or none), the session (kept or not) and the choice (the
run's own or another)
(`settings_dialog::use_says_before_it_is_pressed_what_keeping_the_choice_does_to_the_kept_file`). The way back
is the same file, Imported with Replace. *Not done: a second backup generation*:
the single `.bak` is every `KEEP`'s contract, as the paragraph above says of a
reset.

**Refused in the dialog**, with the reason in the status and the log, and
nothing run:

- **Import, Reset to Built-in, Use and Revert to Kept while the Survey Code
  Manager holds unapplied edits**, in the words the window refuses to close
  over them with (`CustomisationWorkbench::confirm_close`). The manager keeps
  its buffer when the drawing's rules change under it, and its Apply then
  installs that buffer whole: the load would be undone without a word. Revert
  to Kept is held to it with the other two because it loads rules the same
  way.
- **A file no line can carry** - a double quote or a line break in its path:
  the tokenizer's quoted words have no escape, and the rule is the one every
  dialog writes a word by (`crates/katana_window/src/command_word.rs`) - and no file at
  all.
- **Browse in a headless session**, which opens no file dialog and names the
  line to type and the field to fill. Browse is the only thing here that
  opens one; it offers the filter the two managers offer
  (`customisation_file_filter`), opens where the path in the field points, and
  only fills the field. Export's gives a name that does not end `.json`, in
  any letter case, the ending `.customisation.json`, so that the file is one
  Import's own filter shows
  (`settings_dialog::exports_browse_fills_the_field_and_ends_a_name_with_no_json_ending_as_a_customisation_file`).

Reset to Built-in is disabled, with a tip saying why, when the host has no
built-in; Keep and Revert to Kept when it has no kept file; Keep also while
the session is already kept. No button is a default, Enter in a field presses
nothing, and no path through the dialog opens a modal box.

**A path is written in quotes, with `/`, and a bare name with its directory**
(`keep` becomes `"./keep"`). The family reads a line's whole first word as a
keyword once the tokenizer has taken the quotes off, so a file called `keep`
would run `KEEP`, and `EXPORT` refuses a file called as one of its own words;
a file so called is given with its directory (`docs/customisation.md`, "The
verbs"). Every name with no directory in it is, so the dialog keeps no list
of the family's words to go stale
(`settings_dialog::the_lines_it_builds_are_read_by_the_verb_itself`).

**A path pasted in its quotes is the path inside them.** A file manager
copies a path with double quotes round it (`"C:\data\site codes.json"`), and
they are no part of the file's name: ONE pair round the WHOLE path is taken
off, with the blanks beside it, before the path is read - by Import, by
Export and by Browse alike. Left on, the path was refused for holding a
double quote. A quote anywhere else is still refused, and quotes round
nothing name no file (`settings_dialog::a_path_pasted_in_its_quotes_is_read_as_the_path_it_is`).

Decisions, and what was rejected:

- *Rejected: an Apply button.* The code manager already has one, and a
  second commit step here would be a second buffer that a load could silently
  be undone from. Each control is one line, run at once.
- *Rejected: writing every change to the kept file as it is made.* A line
  typed to try a customisation would then rewrite the user's own for good
  (`docs/customisation.md`, "`KEEP` is said, not implied"). Settings keeps
  only what Settings changed, and only a session that was the kept one.
- *Rejected: editing the linework control codes here too.* They have one
  editor, the Survey Code Manager's Linework tab; a second set of seven
  fields would be a second place for them to differ.
- *Rejected: naming the linework controls as a file does* (`arcStart=BC`).
  Those are the words `CUSTOMISE SET` and the format read, written for a
  script. The page says Start, End, Close, Begin curve, End curve, Join to
  point and Rectangle - the labels of the one editor its button opens - so
  that a control has one name in the window. The words are ONE list,
  `LINEWORK_CONTROL_LABELS` in
  `crates/katana_window/src/customisation/linework_labels.rs`, keyed by the format's
  names and checked against `katana_entity::linework_codes::linework_code_members`
  (`settings_dialog::the_one_list_of_linework_labels_names_every_control_in_the_order_the_format_lists_them`).
  The Linework tab builds its seven fields by looping over
  `katana_entity::linework_codes::linework_code_members` and labels each from that list
  (`linework_control_label`), where it once wrote the seven words out itself;
  `settings_dialog::the_linework_controls_are_called_what_their_one_editor_calls_them` reads the tab's
  labels against what Settings shows.
- *Decided: a reset made in Settings is kept.* Resetting in Settings means
  the built-in is what the next session starts with, so the reset is followed
  by `KEEP` like every other edit made here, and the kept file is set aside
  as `.bak` ("A reset that is kept sets the kept file aside", above).
  Rejected: leaving the kept file to stand. The page would say "Not kept",
  and the next start would bring back, without a word, what the person had
  just reset away from.
- *Rejected: a label for the status.* A load answers with several records
  and a refusal may list every reason; a label grows with its text and would
  push the page about. It is a read-only box of four lines, and the headless
  driver's `?settingsStatus` prints a box's text as it is, where it reads a
  label's as markup and would drop the `<file>` of a refusal that names a
  line.
- *Rejected: asking before Reset or before Export writes over a file.* A
  question is a modal box, which a headless run cannot answer.
  `CUSTOMISE EXPORT` says `replaced=yes` in its answer, and what a reset of a
  kept session costs is said on the button before it is pressed ("A reset
  that is kept sets the kept file aside", above).

**In the real window.** Headless, with the fixtures of
`tests/data/customisation`:
`qt_settings_imports_sets_and_exports_by_the_customise_lines_headless` opens
it by its action (non-modal), imports a file by its path field, unticks a
box and reads the switch back in `CUSTOMISE JSON`, and exports;
`qt_what_settings_exported_loads_again_with_the_same_counts_headless` reads
that file in another window. Four more run one after another on one kept
file that `KATANA_CUSTOMISATION` names:
`qt_an_import_made_in_settings_is_kept_headless` (the file appears),
`qt_what_settings_kept_is_what_the_next_window_starts_with_headless`,
`qt_a_reset_made_in_settings_retires_the_kept_file_and_revert_has_none_to_read_headless`
and
`qt_after_a_reset_made_in_settings_the_next_window_starts_with_the_built_in_headless`,
which loads the `.bak` and finds what the first kept.

Not done:

- **ONE earlier file is kept.** A reset made in Settings sets the kept file
  aside one `.bak` deep, and the second edit kept after it is the end of what
  was there. Keeping more than one earlier file would be `CUSTOMISE KEEP`'s
  to do (`crates/katana_cad/src/customisation_verbs.rs`).
- **The host's two facts are read when the dialog is first opened**, not at
  each press: right while a host cannot change in a running session.
- **Revert to Kept is enabled whenever the host has a kept file**, whether or
  not the file is there: `CUSTOMISE REVERT` refuses in its own words when
  nothing is kept. Disabling it would mean the dialog reading the disk, which
  nothing notifies it of.
- **Export writes the whole session.** The parts (`CODES`, `ONLY`) are the
  two managers' export buttons.

### The Updates page

Settings' second page (`settingsUpdates`, "Updates" in `settingsPages`) is of
the PROGRAM, not of the drawing: how Katana checks for a newer version of
itself ("Help > Check for Updates", above). It needs no Document - it works
in a Settings opened on no drawing and after the drawing has closed
(`settings_updates::the_page_works_with_no_drawing_and_after_one_has_closed`) - and like the first page
it runs lines and changes nothing itself, with one exception that is the
reason it exists.

| Part | Control names | What it shows or runs |
|---|---|---|
| Note | `settingsUpdateNote` | one sentence, hidden when there is nothing to say: that a headless run keeps no update settings, that a development build is never updated, or that `KATANA_UPDATE_DISABLE` is set |
| Checking | `settingsUpdateAuto`, `settingsUpdateSends` | "Check for updates automatically": toggled, `UPDATE SET AUTO ON` or `UPDATE SET AUTO OFF`, and the box then shows what the state has - a line that was refused puts it back, as the automation boxes go back to the Document. Under it, what a check sends: only the user agent (`Katana/0.2.0 (win64)`), nothing about the person, their files or their drawings - and that the server it asks sees the computer's network address, as every website does |
| Access token | `settingsUpdateToken`, `settingsUpdateTokenSave`, `settingsUpdateTokenClear`, `settingsUpdateTokenState` | a token for private releases, typed as dots. **Save writes the credential store directly** (`CredentialStore::write`) and runs no line. Clear runs `UPDATE TOKEN CLEAR`. The label says whether a token is stored and where, never the token |
| Skipped version | `settingsUpdateSkipped`, `settingsUpdateUnskip` | the version `UPDATE SKIP` set aside, and "Offer the skipped version again": `UPDATE SKIP NONE`. Disabled while none is skipped; when nobody can say - a state file that does not read - the label is empty and the button stays on |
| Version that was put back | `settingsUpdateBlocked`, `settingsUpdateUnblock` | whether a version is set aside because it was installed here, did not start and was put back, and can still be offered to this install ("A version that was installed here did not start and was put back: it is not offered." or "No version was put back."), and "Offer the version that was put back again": `UPDATE BLOCKED CLEAR`. Disabled while none is set aside; when nobody can say, the label is empty and the button stays on |

**The version that was put back** (2026-10-14). An update that is installed
and does not prove that it starts is put back, and its release is set aside:
no check offers it and no apply installs it. That is right for a release
that cannot start here, and wrong for a sound one whose first start was slow
- a virus scanner reading every new file is enough - and there was no way
back but deleting the state file by hand. The button is that way back, built
as the skipped version's is and under it: it hands the window's executor the
line `UPDATE BLOCKED CLEAR` (`run_updates_line`, the path every line of the
page takes), the verbs empty the list - all of it, there being one release
to be had from the feed - and the page then shows what the state has
(`settings_updates::offer_the_version_that_was_put_back_again_builds_exactly_its_line_and_nothing_else`:
that one line, no second one, no store written, nothing logged by the
dialog; `settings_updates::offer_the_version_that_was_put_back_again_is_a_button_under_the_skipped_versions_one`
for its name, its words and its place). The next check offers the release
and Update installs it. Whether a version is set aside is read from the
state file by the window (`SettingsUpdates::put_back`), since `UPDATE STATUS`
does not say it; with nobody to say - a headless run - the page says nothing
of it and the button stays on for the verbs to answer
(`settings_updates::the_button_for_a_version_that_was_put_back_is_off_with_none_and_on_when_nobody_says`).

**A state file that does not read is nobody to say, too.** The verbs do not
write over such a file: `UPDATE BLOCKED CLEAR` and `UPDATE SKIP NONE` are
refused, and the refusal is the one thing that names the cause. The window
told the page "none" for it, so the page said "No version was put back." and
switched the button off - a guess, and one that kept a person from that
refusal. Both of the page's questions now answer nothing at all for a file
that did not read (`update_skipped_of_state`, `update_put_back_of_state`:
`update_decisions::a_state_file_that_does_not_read_is_nobody_can_say_and_never_none`; a file that is not
there reads as the defaults, which ARE known: none skipped, none put back),
and the page then leaves the label empty and the button on
(`settings_updates::a_state_that_does_not_read_is_not_guessed_at_and_the_buttons_let_the_verb_answer`: the
line is run, its refusal is shown, and the button is still there to press
once the file is put right). The skipped version's label and button had the
same fault and follow the same rule. The automatic box does not: a state
file that does not read is carried on from as the defaults are, for the
check and so for the box.

**A version that stopped mattering is not told of** (2026-10-14). The list of
builds that were put back is only ever added to, until `UPDATE BLOCKED CLEAR`
empties it, and it is one list for every copy of Katana a person runs. So
once 0.3.0 had been put back, and 0.4.0 installed later and found to start,
the page went on saying "A version that was installed here did not start and
was put back: it is not offered." with its button on - of a release no feed
offers over 0.4.0, and with a button that changed nothing a check could show.
The list keeps a build by its sequence, the time it was made, and no version;
the install's record keeps the sequence of the build an update put there. So
the window tells the page of a build only while it can still be offered to
this install (`update_put_back_of_state`, given what the window knows as
`UpdatePutBackFacts`;
`update_decisions::a_version_that_was_put_back_is_told_of_only_while_it_can_still_be_offered_here`):

- a build made AFTER the one installed here - a sequence above the install
  record's - or any build where the install has no record of its own (a fresh
  install: nothing says when its build was made);
- or any build at all, while the update dialog is telling of a release that
  is set aside (`update_tells_of_set_aside`). A build made before the installed
  one can still be of a higher version - a release line that went back to an
  older version, which was installed, and forward again - and the list cannot
  tell. A check that meets such a release can, and its sentence sends the
  person to this button, which must then be on
  (`update_decisions::the_dialog_tells_of_a_release_that_is_set_aside_exactly_where_it_points_at_settings`).

Otherwise the page says "No version was put back." and the button is off.
Nothing of this changes the state file: the build stays on the list.
*Rejected: dropping the older builds from the list when an update commits
healthy.* The list is one per user and an install's record is its own, so one
copy's update would decide for every other copy; decided where it is read,
each install answers for itself, and a build that a later check does meet is
still refused and still told of.

The update dialog's two sentences about a release that was put back point
here ("Help > Check for Updates", "A release that was set aside says where
the way back is"). *Rejected: offering again only the one release, by its
number.* The state keeps the builds that were put back by sequence, not by
version, so there is no number to show or to ask for; and the feed has one
release to offer, the newest, so emptying the list and offering "the"
release are the same act.

**Why Save is not a line.** Every other button in Settings builds a line so
that it is echoed in the command log and done by the one code path a typed
line takes. A token on a line would be echoed there too, and into a script's
output, and a headless run's stderr. So there is no `UPDATE TOKEN SET`
(`crates/katana_app/src/update_verbs.rs`), and Save is the one thing the dialog does
itself: the store's `write`, then the field emptied. Nothing of the token is
said anywhere - the status says only that one was saved and where, a refusal
shows the store's own message, which names the rule and the place and never
the value, and `settings_updates::save_writes_the_credential_store_and_never_produces_a_line` reads the
status, every label, the log and the lines run for it. A script gives a run a
token with the environment variable `KATANA_UPDATE_TOKEN` instead.

A person may still TYPE a line that carries one, expecting it to work
(`UPDATE TOKEN ghp_...`), and the window echoes a typed line before any verb
refuses it. So the echo shows every word after `UPDATE TOKEN` that is not
`STATUS` or `CLEAR` as `***` (`update_logged_line`, in both of the window's
echoes; `update_words::what_is_typed_where_a_token_was_hoped_to_go_is_never_echoed`), and what the
verbs say back about such a line is logged with those words taken out IN ANY
CASE (`update_logged_reply`): a refusal may quote the line it refuses, and may
have folded its case to find the verb
(`update_decisions::what_the_verbs_say_back_to_a_line_with_a_token_typed_on_it_is_hidden_in_any_case`; in the
real window, `qt_a_token_typed_after_update_token_is_shown_nowhere_in_the_log_headless`).
The line itself still reaches the verbs as typed, and is refused.

**How the page gets what it shows.** The Format workbench makes the Settings
dialog and knows nothing of updates, so the page's services
(`SettingsUpdates`: how to read the automatic flag, the skipped version and
whether a version was put back, the credential store, the user agent, the
note) are handed over by the
window: `SettingsDialog::set_updates`, called each time File > Settings is
triggered, after the workbench has made the dialog, so the page always has
the dialog it fills. The page is filled again
at each opening and each time it is turned to, so a `UPDATE SET AUTO OFF`
typed meanwhile is shown (`settings_updates::turning_to_the_page_reads_the_state_again_and_runs_nothing`).
Given no services - a window test's bare dialog - every control on it is
disabled (`settings_updates::with_nothing_given_the_pages_controls_are_off_and_it_says_why_no_token_is_kept`).
*Rejected: a `CustomisationServices` member for them*, which is the tidier
wiring and one line in the workbench; it was outside the files of the change
that added the page, and is the first thing to do when that workbench is next
edited.

**In a headless run** the page says so and keeps nothing: no state to read,
so the box is off and disabled, and no credential store, so Save is disabled
(`qt_settings_has_an_updates_page_that_says_what_a_check_sends_headless`,
`qt_a_headless_run_saves_no_access_token_headless`). Clear and the two
buttons that offer a version again still build their lines, which the verbs
answer for that surface: the button for a version that was put back is
pressed in the real window by
`qt_settings_offers_again_a_version_that_was_put_back_by_its_line_headless`,
where the command log shows `UPDATE BLOCKED CLEAR` and no other `UPDATE`
line, and the status shows the verbs' refusal, `reason=headless`.

Tested in `crates/katana_window/tests/update/update_dialog.rs` (the `SettingsUpdates`
cases) and, in the real window, by `tests/update/headless/update_window.cmake`.

## The rules a dialog or panel follows

Collected here because each was paid for once, and a new manager is where
they are easiest to break:

- **A dialog is a model and a function that draws it.** It keeps its state
  between frames in plain fields, and what it asks of the window it asks
  through the `Host` it is drawn with or as events the window reads after
  the call (`WorkspaceEvent`, `ChromeRequest`); no callbacks are stored in
  it, so a dialog can be dropped at any time and a test can draw it with a
  host of its own.
- **Tables are read-only and a form edits**; a command is never run from a
  list's or table's own change ("Styles and Linetypes").
- **A view reacting to the Document defers and coalesces** its reload to the
  next frame, never rebuilding a table inside the notification ("Panels
  refresh on the event loop"). A manager does it through a `DocumentWatcher`
  (`crates/katana_window/src/customisation/document_watcher.rs`): one
  delivery per frame however many notifications came, asked for when the
  dialog draws, saying what moved - the model (`Document::model_revision`),
  the library or the map (their generations), the selection, the current
  layer or style.
- **A manager is non-modal and kept** by the workbench that opened it (the
  Format workbench, "The Format menu" above; the Survey workbench's dialogs),
  so it stays open beside the drawing and a buffer of unapplied edits
  survives hiding it.
- **A buffer is kept only while what it was filled from is as it was.** A
  reload that left the stored thing (a template, a corridor, the component's
  courses) equal to the copy the fields were filled from keeps what was typed;
  one that found it changed - an undo, a redo, a typed line, New, Open - shows
  what the drawing holds, and the choice of another thing drops the edits made
  to the last. Comparing a REFERENCE to the thing (`left:lane`, which every
  template with a lane has) is not comparing the thing. The Road Template
  Manager's grids and the Corridors dialog's fields are the models
  (`docs/road_design.md`, "What a dialog shows"). A listing that is the
  reply of a read (Corridor Quantities) is cleared, not left to show a
  drawing that has gone, when the model it read is no longer the drawing's.
- **One Apply is one line, so one undo step.** A grid of edited rows is
  applied as ONE verb line with a group for each row, never a line a row: the
  verb makes all of it or none, and a value it refuses leaves the drawing as it
  was with the edits still in the grid (`ROADTEMPLATE SET <t> <c> key=value
  <c> key=value`). A verb that takes one thing at a time is given a form that
  takes several rather than being run in a loop by the dialog.
- **A headless session never opens a modal box**, and every action, menu,
  field, button and tab has a control name, so tests and the headless driver
  can find it.
- **A dialog keeps no reference to the Document**: it is lent the drawing for
  each frame or call, so it can never outlive it, and a
  registration is owned by a `ListenerHandle` (`docs/cad.md`, "A listener
  lives exactly as long as the thing it notifies").
- **Logic that can be tested below the window lives in `katana_cad`**
  (`crates/katana_cad/src/`, with its tests in
  `crates/katana_cad/tests/`) - and the dialog stays thin. What must be
  tested WITH the window goes in the window's tests
  (`crates/katana_window/tests/`, headless), which drive a dialog by its
  control names through the drive harness
  (`crates/katana_window/src/drive/`) and assert on the Document.
