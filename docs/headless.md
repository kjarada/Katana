# Headless runs: katana_cli and the window's driver

Katana runs without a display in two ways: `katana_cli`, the engine with no
GUI at all, and the desktop application `katana` driven by switches, which
builds the real window offscreen, acts on it as a person would and writes a
screenshot or a PDF. The second is how the window, its menus and its dialogs
are tested (`docs/testing.md`) and how a change to the look is reviewed by
someone - or a model - who cannot watch a screen. The window itself is
`docs/desktop.md`.

## katana_cli

`katana_cli` runs the same `CommandInterpreter` as the window
(`docs/cad.md`, "Command interpreter"), plus verbs of its own
(`docs/building.md`, "The command line"):

```
katana_cli                                interactive
katana_cli script.kcs                     run a script, '#' comments
katana --script script.kcs                the same script in the window, headless
katana_cli -c "RECT 0,0 30,20" -c LIST
katana_cli --help                         every verb it knows
```

Scripts and `-c` batches stop at the first failing command and exit 1, so the
tool composes with shell pipelines. A refusal is one line on stderr; a
refusal that carries a report - `UTILITY CHECK` of a schedule with errors -
prints the report after it on stdout, where a reply goes, so output kept from
stdout has it (`tests/app/test_session.cpp`). The `cli.*` tests
(`src/katana_app/CMakeLists.txt`) use it to draw, save, reopen in a SEPARATE
process and check the geometry, to confirm that invalid input fails the
process (`cli.rejects_bad_input`), and to exercise the survey-code verbs on a
code file the tests write themselves. The sheet verbs (`docs/plotting.md`,
"Sheets on the command line") are the interpreter's, so they run here too
(`cli.sheet_verbs_lay_out_edit_and_list_the_sheets`); only `PLOTSHEETS`,
which paints, needs the window. `UTILITY REPORT | VERIFY | CLEARANCE |
CHECK | DRAW | REGRADE | SCHEDULE` grades subsurface utility schedules by
AS 5488 quality level, checks them against a delivery schema, draws them into
the drawing, and does the same to what is drawn
(`docs/subsurface_utilities.md`); it is the interpreter's too, and the
`cli.utility_*` tests run it on `samples/utilities/`. `MODIFY` and the
`UTILITY` verbs take the shared scope words (`docs/cad.md`, "Scope and
filter"; every new verb on drawing data is to take them too), except that
`VIEW` is the window's plan view: headless it is refused, and `AREA
x0,y0,x1,y1` names the window instead
(`cli.utility_view_is_refused_headless_naming_area`). The window itself, run
headless with `--command`, does answer `VIEW` - it has plan views even
offscreen (`qt_modify_view_takes_what_the_plan_view_shows_headless`).

The same session is served to Claude over the Model Context Protocol by
`katana_mcp` (`docs/mcp.md`).

## katana with --screenshot and --plot

```sh
QT_QPA_PLATFORM=offscreen timeout 120 ./build/release/bin/katana.exe \
    <copy-of-project> [files to import] --screenshot out.png [steps...]
QT_QPA_PLATFORM=offscreen timeout 120 ./build/release/bin/katana.exe \
    <copy-of-project> --plot out.pdf [--fit | --scale N] [--paper A3] [--landscape | --portrait] [--dpi N]
QT_QPA_PLATFORM=offscreen timeout 120 ./build/release/bin/katana.exe \
    <copy-of-project> --plot-sheets out.pdf|folder [--sheets 1,3-5] [--format pdf|pdfs|png|tiff] \
    [--plot-style colour|grey|mono] [--dpi N] [--line-weight-scale F]
```

Either switch makes the run HEADLESS (`MainWindow::setHeadless`): the window
is not shown to anyone, and it never opens a modal box ("A headless session
never opens a modal box", below). Always open a COPY of a project: opening
can back up or migrate the project directory in place.

**`--screenshot out.png`** lays the real main window out and grabs it to a
PNG, headlessly. It exists so that the LOOK can be reviewed - in a pull
request, or by a model that cannot watch a screen - the way `--plot` lets its
output be reviewed. The first screenshot found three things no test would
have: alignment station labels printing over one another where key stations
bunch up (a label is now skipped when it would land within 70 px of the last,
while the tick is always drawn - the tick is the information, the label only
names it); Zoom Extents ignoring alignments, which are drawn but are not
entities; and the command log opening at a third of the window's height. The
`qt_screenshot_headless` test builds the whole window - theme, toolbars, every
icon, the sample with its hatch and alignment - and checks a PNG of plausible
size comes out. It protects construction, not appearance: LOOK at the PNG.
A headless window is always the window as built, 1360 x 860 at the
platform's text size: it neither reads nor writes the place, layout, text
size and toolbar choices an interactive session keeps
(`MainWindow::restoreSession`, `docs/desktop.md`, "How the window starts"),
so a screenshot is the same on every machine and a test leaves no settings
behind. The same goes for the customisation: a headless run reads no
per-user place for one ("The customisation a run starts with", below).

**`--plot out.pdf`** plots the drawing and exits (`docs/cad.md`, "Plotting to
PDF"). `--fit` (the default) picks the first standard scale at which the
drawing fits; `--scale N` plots at 1:N; `--paper` is A0 to A4; `--dpi` sets
the resolution. The dialog, the window's `PLOT` verb and the switch share
`plotDrawingToPdf`, so the `qt_plot_headless` test exercises the code the menu
does; it also runs a `PLOT` script in each plot style and weight and checks
the inks each prints (`docs/desktop.md`, "Plot to PDF and view images").

**`--plot-sheets out.pdf`** plots the project's sheets and exits
(`docs/plotting.md`, "Plot styles and output"); a project with no sheets
plots one fitted to the drawing, and what the sheet checks find on the
sheets plotted is logged to stderr first. Each switch not given comes from
the sheet set's page setup, except the format, which is one PDF unless
`--format` says otherwise. `--sheets` chooses the sheets (`1,3-5`, sheet
ids; all by default); `--format pdfs`, `png` or `tiff` writes a file a sheet
into the FOLDER given to `--plot-sheets`, named by the page setup's
file-name pattern; `--plot-style` prints in `colour`, `grey` or `mono`;
`--dpi` is the resolution of a raster and of a PDF's 3D snapshot (300 by
default); `--line-weight-scale` multiplies every line weight (0.1 to 5).
Every file written is printed on stdout, a path a line. The File menu's Plot
dialog and the switch share `plotSheets` in
`src/katana_qt/plotting/plot_output.hpp`. `--plot` takes `--plot-style` and
`--line-weight-scale` too.

**`--sheets-json out.json`** writes the
project's sheets as the JSON the project stores them in, and exits; `-`
writes it to stdout. Given together, the JSON is written first. Without
`--screenshot`, the `--command` lines run BEFORE either is written, and when
one of those two is asked for, a line that is refused fails the run (exit 1,
naming it) and nothing is written. A line that plots with problems (an image
missing, a section with nothing to cut) still counts as carried out.
`--sheets-json` is not written by a `--screenshot` run. So one run can lay the
sheets out with the sheet verbs, keep them, report them and plot them - the
way an agent drives the sheets (`docs/plotting.md`, "Sheets on the command
line"):

```sh
QT_QPA_PLATFORM=offscreen ./build/release/bin/katana.exe <copy-of-project> \
    --command "GENERATE grid scale=500 keyplan=on" --command SAVE \
    --sheets-json sheets.json --plot-sheets sheets.pdf
```

The `qt_sheets_headless` test runs that (`tools/check_sheets_headless.cmake`),
plots one sheet with the window's `PLOTSHEETS` verb, reads the saved copy back
with `--sheets-json -`, and checks that a refused line fails the run.

Offscreen Qt has no monospace font, so a screenshot's command log and any
fixed-pitch text differ from a real display; judge fonts on a real screen.

## The switches

`src/katana_qt/main.cpp` parses them, and the usage comment above its `main`
is the reference. Arguments that are not switches are opened (a directory) or
imported (anything else), in order, before any step runs.

| Switch | What it does |
|---|---|
| `--screenshot PNG` | grab the target at the end - the window, or the dialog, dock or panel the steps made the target |
| `--plot PDF` with `--fit`, `--scale N`, `--paper A0..A4`, `--landscape`, `--portrait`, `--dpi N` | plot the drawing to a PDF and exit |
| `--plot-sheets PDF` | plot every sheet of the project to one PDF, a page a sheet, and exit; a project with no sheets plots one fitted to the drawing (`MainWindow::plotSheetsToPdf`). What the sheet checks find is logged to stderr first (`docs/plotting.md`, "The sheet painter", "Preflight checks") |
| `--plot-sheets` with `--sheets 1,3-5` | plot only those sheets (positions from 1, ranges, sheet ids), in that order |
| `--plot-sheets` with `--format pdf\|pdfs\|png\|tiff` | one PDF (the default), or a PDF, PNG or TIFF a sheet in the FOLDER given to `--plot-sheets`, named by the page setup's pattern; each file written is printed on stdout |
| `--plot-style colour\|grey\|mono`, `--line-weight-scale F` | with `--plot-sheets` or `--plot`: print in colour, greyscale or monochrome, every line weight times F (0.1 to 5) |
| `--dpi N` with `--plot-sheets` | the resolution of a PNG or TIFF and of a PDF's 3D snapshot (the page setup's, 300 by default) |
| `--sheets-json FILE` | write the project's sheets as JSON (`-`: to stdout) and exit; before `--plot-sheets` when both are given |
| `--customise FILE...` | load Katana customisation files (`docs/customisation.md`) - every path until the next switch - before anything is opened: ONE line, `CUSTOMISE "<file>" ...`, through the window's one executor, merged into what the session started with, all or nothing, and answered with the verb's records (`loaded file= ...`). Each path is made absolute first, so a file called as one of the verb's keywords (`reset`, `json`) is still a file. A refused line fails a run that only writes or runs a script batch, as a refused `--command` does, before any step runs (`qt_a_refused_customise_switch_stops_a_batch_run_before_its_script_headless`, `qt_a_refused_customise_switch_fails_a_batch_run_headless`); a screenshot run goes on (`qt_a_refused_customise_switch_leaves_a_screenshot_run_going_headless`). The style libraries and survey code files of another program, which the switch once took, are refused as not a Katana customisation file |
| `--action NAME` | trigger the menu item with that object name after the imports, as a click does; repeatable, in order, before the steps |
| `--select-all` | select every entity on an unlocked layer before the actions run |
| `--toggle-layer NAME` | flip the layer's visibility box through the layer panel and refuse to go on if the panel was rebuilt inside its own signal (`desktop.md`, "Panels refresh on the event loop") |
| `--style-manager` | open the styles and linetypes manager and grab it |
| `--layer-manager` | open the Layers dialog and grab it; Format > Layers is non-modal, so `--dialog formatLayers` reaches the same dialog as a step |
| `--attributes [ID]` | open the attribute manager (Edit > Attributes, still modal in a person's session) on entity ID and grab it |
| `--dataset-info FILE`, `--import-options FILE` | build GIS > Dataset Information or the GIS import dialog for FILE and grab it; for a DXF or a .12da the import dialog is File > Import's placement step (`docs/interop.md`, "Placing an import"). With steps, it is the target they start on, so `--fill vectorImportWhere=...`, `--press vectorImportPreview` and `--report vectorImportMatchCount` drive it (`qt_vector_import_dialog_previews_its_line_headless`). Its Import and Cancel are `<dialog>Run` and `<dialog>Cancel` (`vectorImportRun`, `rasterImportRun`, `pointCloudImportRun`); a pressed Run imports as the menu's accepted dialog does, through `MainWindow::finishImport` (`qt_vector_import_dialog_imports_what_its_filter_takes_headless`) |
| `--check-shortcuts` | fail the run when a key reaches more than one thing (`desktop.md`, "Every key reaches one thing") |
| `--check-menus` | fail the run when a menu item, in any submenu, has no icon or no status tip, listing each (`desktop.md`, "Every menu item has an icon and says what it does") |
| `--check-toolbars` | measure every icon-only button with a menu on a toolbar or in a dock's title bar (the tool families, Undo, Redo, each view's kind switcher) from its own rendering - where it draws its icon, and what its menu adds - print a `toolbar button` or `title bar button` line for each, and fail the run, with a `menu sign clash:` line, when a menu's sign is drawn over the icon or not at all (`tools::menuSignClashes`; `desktop.md`, "Tool families on the toolbars"). It measures once the `--action` switches and the steps have run, so a `--trigger viewToolBarIconsLarge` or a `--command` starting a tool is measured; on a scaled screen it judges in device pixels, and each line says so |
| `--script FILE` | run a `katana_cli` script (`.kcs`) in the window, a step among the others: the window's `SCRIPT` verb through its one executor, each line its own undo step, stopping at the first refused (`desktop.md`, "Run Script"). A script that stops fails the run, exit 1. Without `--screenshot`, `--plot` or `--plot-sheets` the run is a batch, as `katana_cli`'s is: never shown, over when its steps are, and failed by the first step refused |
| the steps | below |

## The customisation a run starts with

The window starts with a customisation - linestyles, symbols and survey codes
- before it opens anything (`MainWindow::loadDefaultCustomisation`,
`cad::startCustomisation`; `docs/desktop.md`, "How the window starts"), and
two environment variables say which. The window reads them once, at start-up.
(The seam is cad's own, `cad::builtInCustomisation`, so every front end that
hands a host over reads it the same way: `katana_cli` and `katana_mcp` do,
through their session's `startCustomisation`.)

| Variable | What it says |
|---|---|
| `KATANA_BUILTIN_CUSTOMISATION` | the seam (`include/katana/cad/customisation_host.hpp`): unset, the built-in is the customisation compiled into the program, when the build has one; `none`, there is no built-in for this run; a path, THAT Katana customisation file is the built-in. One that does not read is said, and the run has no built-in - never the compiled-in one in its place |
| `KATANA_CUSTOMISATION` | the kept customisation file: what `CUSTOMISE KEEP` writes and the next start reads in the place of the built-in. Unset, a headless run has none, and `KEEP` and `REVERT` are refused naming the variable. It is the ONLY way a headless run reads or writes one: it looks in no per-user place, so that it is the same run on every machine. (An interactive window with the variable unset keeps its own in the per-user place: `docs/desktop.md`, "How the window starts") |

The start-up line says what was installed, on stderr with the rest of the
log: `Customisation: <name>, N definitions (M symbols) and K survey code
rules, built in.` or `..., kept.` A run with nothing to install says nothing.

**File > Settings in a headless run.** The dialog is opened by its action and
driven by its fields, as any dialog is: `@fileSettings` (it opens non-modal,
so the run goes on), then
`settingsImportPath=<file>|!settingsImport|?settingsStatus` to load a file -
Browse opens no file dialog and says to fill the field -
`settingsExportPath=<file>|!settingsExport` to write the session,
`settingsAutoCodes=off` to untick a box, which runs its `CUSTOMISE SET` line
as a click does, and `!settingsReset`, `!settingsKeep`, `!settingsRevert`.
Each press echoes nothing on stderr but what its line answers; the status box
(`?settingsStatus`) holds the line and the answer together
(`qt_settings_imports_sets_and_exports_by_the_customise_lines_headless`).
What Settings changes is KEPT only where `KATANA_CUSTOMISATION` names a file:
`qt_an_import_made_in_settings_is_kept_headless` and the three tests after
it run four windows, one after another, on one such file in the test's own
folder (`docs/desktop.md`, "Settings").

**Every program test says what it starts with.** A build on the owner's
machine has a customisation compiled in and a clean checkout has none, and a
test that did not say would be two tests. Two parts, which share nothing:

- **The scan**, at configure time (`tests/CMakeLists.txt`,
  `katana_say_what_every_program_test_starts_with`), gives every
  `qt_..._headless` and `cli.` test, in whichever directory it is
  registered, `KATANA_BUILTIN_CUSTOMISATION=none` and unsets
  `KATANA_CUSTOMISATION` - unless the test sets that variable itself, as the
  tests of the built-in and of the kept customisation do, naming a small
  committed fixture in `tests/data/customisation`
  (`qt_the_window_starts_with_its_built_in_customisation_and_reset_returns_to_it_headless`,
  `qt_a_kept_customisation_is_what_the_next_window_starts_with_headless`).
  It is deferred to the end of the TOP directory, so a directory added after
  `tests/` (`benchmarks`) is reached too. The `cli.` tests carry their
  environment on a `cmake -E env` command line, which the scan cannot read
  and need not: the test property is set on the process that line starts
  from, so a variable named there still wins.
- **The check**, at test time
  (`every_program_test_says_which_built_in_customisation_it_starts_with`,
  `tools/check_program_tests.cmake`), reads what ctest itself will run -
  `ctest --show-only=json-v1`, every directory's tests with the properties
  they really have - and names every test whose COMMAND names `katana`,
  `katana_cli` or `katana_mcp` and which does not give the variable a value:
  in `ENVIRONMENT`, as `set:` in `ENVIRONMENT_MODIFICATION`, or on its own
  `cmake -E env` line. An empty value, `unset:` and `reset:` do not say. It
  goes by the command, not the name, so a program test registered under
  another name is named and fails the suite until it says.
  `the_program_test_check_names_a_test_left_to_the_machines_customisation`
  runs the check on a hand-written listing (`tests/program_test_fixtures`)
  with four tests in it that it must name.

The two were one function, which set the variable on its own list of tests
and then named those of that list without it - so nothing the scan missed
could be named, and the test could not fail. A test of another directory is
reached with `set_property(TEST ... DIRECTORY ...)`, which needs CMake 3.28.
With an older one `tests/CMakeLists.txt` does what it can and says so (it
refused to configure at all until 2026-10-07): the scan is deferred to the
end of `tests/` and reaches that directory's own program tests, the `cli.`
tests and the window checks of other directories are left to the build and
the caller's environment, and the check is not registered - it would name
them at every run, for a reason nobody running the suite can mend. One
`message(STATUS ...)` says all of that as the tree is configured. (The top
`CMakeLists.txt` and the presets declare 3.24. Raising that also turns on
every policy up to the new minimum, C++ module scanning of each source among
them, which is a decision about the build and not about this scan.) There is
no older CMake on the machines this was written on: the degraded path was
run with the scan's own text in a throwaway project that sets
`CMAKE_VERSION` to 3.27 first. Rejected: one line in each test's own
properties - nearly four hundred tests, and the next one added would forget
it. What the check cannot see is a program started from inside a script or a
test executable without being named on the command line; there is none.

The `cli.` tests are held by the same variable: `katana_cli` starts its
session through `cad::startCustomisation` and reads the seam
(`src/katana_app/session.cpp`).

## The steps: driving dialogs, docks and the command line

The steps run in the order given, with the event loop run between them as it
runs between two things a person does, so what one step sets going has
happened before the next. Everything is found by OBJECT NAME, which is why
every action, field, button and tab gets one. In a test they are one
`-DDRIVE=` list of `tools/check_screenshot.cmake`, a step per sigil:

| Switch | `-DDRIVE` step | What it does |
|---|---|---|
| `--dialog NAME` (first called `--survey-dialog`, still accepted) | `@NAME` | triggers action NAME as a click does and makes the dialog it opened the target: the dialog named by the action's data (the Format managers carry `styleManagerDialog`, `symbolLibraryDialog`, `surveyCodeManagerDialog`, `textStyleManagerDialog`, `labelStyleManagerDialog`, `dimensionStyleManagerDialog`; Annotate > Edit Text carries `textEditDialog`; File > Project Coordinate System carries `projectCrsDialog`; the Annotate menu's `annotateLeaders`, `annotateLeadersForSelection` and `annotateArrangeLeaders` all carry `leaderManagerDialog`; the seven Subsurface Utilities items all carry `utilityDialog`; File > Settings carries `settingsDialog`), else NAME + `Dialog` (the Survey dialogs, `formatLayersDialog`); says on stderr what opened, and whether it is modal |
| `--survey-dock ACTION` | `#ACTION` | shows the dock that action shows and makes it the target; at the end its status line is printed |
| `--panel NAME` | `%NAME` | makes the window's own dock, toolbar or menu NAME the target; a menu is opened under its title, so a grab shows what it offers. A popup a step opened - a window of its own, shown - is a panel too: a view bar's Layers popup, whose box is the view's ghost switch (`%View2\|!ViewLayersButton\|%ViewLayersPopup\|!ViewLayersShowSelection`, `qt_the_layers_popups_box_turns_a_views_ghosts_off_headless`); it could not be reached before |
| `--fill FIELD=TEXT` | `FIELD=TEXT` | a line or text box (`\n` a line break), a choice by its text (an editable one takes a name it does not list, as typing does), a spin or check box, a tab brought to the front by its text (`managerTabs=Linetypes`), or a list, grid or tree row selected by its text - the whole row where the view selects rows, as a click does. A disabled field is refused, as `--press` refuses a disabled button: a person cannot type into it and the line does not read it (`qt_fill_of_a_disabled_field_is_refused_headless`) |
| `--press BUTTON` | `!BUTTON` | clicks it - a radio button too, which is how one is chosen (`!utilitySourceDrawing`); a disabled button fails the run, and so does one its owner has hidden - a view bar's tool it had no room for, or a Link its kind has none of - which a press once clicked though nobody could see it (`qt_a_press_of_a_hidden_bar_tool_is_refused_headless`) |
| `--command TEXT` | `>TEXT` | runs TEXT as if typed on the command line - make styles and a selection, or start a tool by its alias and answer its prompts; without `--screenshot` the commands run before `--sheets-json` and the plots, and a refused one fails a run that writes one of them |
| `--enter` | `>` alone | Enter on an empty command line (an empty argument does not survive a CMake list) |
| `--run-line TEXT` | `<TEXT` | runs TEXT through the window's one executor, as a dialog runs the line it built (`MainWindow::runVerbLine`, `desktop.md`, "One executor: the command runner"): never a running tool's answer; prints `--run-line TEXT: ok=yes` or `ok=no`, then a `  reply: ` or `  error: ` line for each line it logged - what the dialog gets back. Without `--screenshot` it runs with the `--command` lines, and a refused one fails a run that writes |
| `--report NAME` | `?NAME` | prints on stderr what the target's widget NAME shows - a label's text, a field's, a list's rows, a button's accessible name and, when it can be checked, whether it is (`%View2\|?ViewLinkButton` prints `ViewLinkButton: Linked, checked`, `qt_linked_plan_views_zoom_together_headless`) - or, for one of the window's actions, its text and whether it is checked (which tool the menus show running); for one of the window's menus (`formatMenu`), its title and every item with the status tip it shows, without opening it - its OWN items: a submenu is one item, by its title, and is asked by its own object name (`?fileImportMenu`, `?fileExportMenu`, the two of File; `qt_file_keeps_every_import_and_export_in_two_submenus_and_settings_above_quit_headless`), and the section titles between items are not listed; failing all of those, any of the window's own widgets, so what a dialog did to the window is read with the dialog still the target (`?FrameStatsLabel` after the utilities dialog framed the views, `qt_utility_dialog_headless`). A plan view is `PlanView<id>`, as its dock is `View<id>`: painted afresh, it prints what it drew - `PlanView2: drawn=1 ghosts=1 grips=0`, the entities, the selection's ghosts on layers it hides, and the grips it offers - which a screenshot shows only to a person (`qt_a_selection_shows_as_a_ghost_where_the_view_hides_its_layer_headless`). A 3D or elevation view is `RenderView<id>`, as its dock is `View<id>`: painted afresh, it prints how many of its pixels are not its background and where its camera is, by the keys a view's record uses - `RenderView2: painted=27324 target=85.375,68.69...,31.85... distance=483.19... azimuth=-135 elevation=35.26... projection=perspective` - which a screenshot shows only to a person |
| `--trigger NAME` | `*NAME` | triggers menu item NAME in its turn among the steps (`--action` runs before them all) |
| `--export-options FILE` | `^FILE` | opens the dialog of File > Export > Export Drawing for FILE, in its turn, and makes it the target - what the menu opens once its file dialog has answered, which a headless run never opens. A step rather than a switch, so the `--command` lines before it have made the drawing whose layers and scope it offers (`qt_vector_export_dialog_writes_what_its_filter_takes_headless`) |
| `--wheel "NAME X,Y N"` | `~NAME X,Y N` | turns the mouse wheel N notches - positive away from the person, which zooms in; negative, out - over widget NAME at X,Y in its logical pixels, NAME found as `--report` finds one. Each notch is a wheel event of its own, sent to the widget under that point (a 3D view's GPU child, where it has one) with the event loop run after it, as a person's wheel reaches the view; so a 3D view's zoom is driven in the real window, `~RenderView2 160,330 10|?RenderView2` (`qt_the_3d_views_wheel_keeps_zooming_into_the_ground_under_the_cursor_headless`). A step that is not three words, no such widget, a point outside it or no notches ends the run |
| `--hover X,Y` | `~X,Y` (a `~` then a widget name is the wheel's) | moves the pointer to model point X,Y in the plan view Enter goes to (the one running a tool, else the active one), as a real mouse move: a tool's preview follows it. Prints the pointer record (below) |
| `--click X,Y[,shift\|,ctrl]` | `+X,Y[,shift\|,ctrl]` | the same, then a left press and release there with the key held: a tool's pick or point, or with no tool a grip picked up (plain) or made hot (`,shift`) as a person's click does. Prints the record as the view shows it after the click |

What the target is at the end is what `--screenshot` grabs; steps that were
all commands leave the window. A dialog that deletes itself when a step
closes it (Generate Sheets and Page Setup in the Sheets editor, opened with
`open()`) leaves the window the target: the run says `<NAME> has closed: the
window is grabbed`, and a later `--fill` or `--press` meant for it is refused
naming it (`qt_a_dialog_that_deletes_itself_on_close_leaves_the_window_the_target_headless`,
`qt_a_step_for_a_dialog_that_has_closed_is_refused_headless`). The target was
once a raw pointer, and such a run crashed at the grab after every step had
passed. A step that cannot be taken - an unknown
dialog or widget, a fill of a choice the dialog does not offer, a press of a
disabled button - ends the run with exit 1 and a sentence on stderr, never a
crash and never a silent no-op. The names a dialog's fields carry are listed
in its header (`survey/survey_dialogs.hpp`, `survey_import_wizard.hpp`,
`survey_points_ui.hpp`, `src/katana_qt/ifc_dialogs.hpp`, the managers' own).
File > Export IFC, for one, is `@fileExportIfc|ifcExportFile=out.ifc|!ifcExportPreview|?ifcExportClasses|!ifcExportExport`:
the preview's table printed, then the file written
(`qt_ifc_export_dialog_previews_each_class_and_writes_the_file_headless`). A
dialog that another dialog's button opened has no action to be named by, and
is a panel once it is shown: the definition editor is
`@formatSymbols|!symbolNew|%definitionEditorDialog|definitionName=TEST Post|...|!definitionSave`,
and `%symbolLibraryDialog` then makes the library the target again
(`qt_a_symbol_made_in_the_definition_editor_is_listed_by_the_symbol_library_headless`;
`docs/desktop.md`, "The definition editor"). The
IFC dialogs, which run lines, echo each on stderr as "> <line>" before its
reply (`MainWindow::runIfcCommand`), so a test reads which line a press ran.

A headless run echoes its command log to stderr, which is where a test reads
what a command REPORTED.

**The pointer.** A headless run has no mouse, and a tool's preview and its
picks follow one: before the pointer steps a run could start Insert Vertex
but never show where a vertex would go, and a pick step (a vertex, a
segment) refused every typed coordinate it was not written to take. `~` and
`+` (`MainWindow::pointerAt`, `ViewWorkspace::pointerAt`,
`ViewportWidget::pointerAt`) paint the view so its transform is the one on
screen, map the model point to a pixel through it, send a real `QMouseEvent`
move - and for `+` a press and a release - and paint again. The view cannot
tell them from a person's mouse, so they drive the same code: snapping, the
grips, the tool's pick. Each prints one line, numbers exact
(`formatExactReal`), what the view then drew of the running tool's preview
by role (`drawing/feedback_painter.hpp`) - `enter` counts where Enter,
not a click, would add:

    pointer: action=hover x=110.3 y=50 tool=draw.vertex.insert expects=point shapes=0 markers=0 target=1 added=1 removed=0 enter=0 focus=4 refused=no caption="new vertex between 1 and 2 · 15.000 from 1" prompt="Insert Vertex: Click on polyline 2 where the new vertex goes"

It is a reply record like every verb's (`docs/cad.md`): the roles' keys are
their names (`cad::toString(FeedbackRole)`), and the caption and the prompt
are written by `core::replyQuoted`, so `core::readReplyRecord` reads the
line back whole. Quoted by hand, Move Vertex's caption - which ends in a
bearing, `0°00'00"` - closed its value early and the line was no record at
all; it is written `0°00'00\"` now
(`qt_pointer_record_quotes_a_bearing_as_a_reply_does_headless`,
`VertexToolHover.ThePointerRecordReadsBackAsOneRecordWithABearingInItsCaption`).

The counts are of what is ON SCREEN: an Added vertex the painter leaves out
beside a Removed one (a fillet a few pixels across) is not counted, nor a
focus square thinned out of a dense string - `added=2` once stood over a
picture with no disc in it. A `+` the tool took is recorded as the view
then shows it: the caption is what the click did, and `refused=no`, until
the pointer moves off (`ToolHost::feedback`).

`tool=none` and zero counts with no tool running. A point outside the view
fails the run (exit 1, "is outside the view ... zoom to it first"), as a
spec that is not `x,y[,shift|ctrl]` does. The pointer's model point comes
back from its pixel within a few units in the last place, so what a click
made from it - a vertex put on a line - is matched to ten decimals, not
exactly (`qt_insert_vertex_puts_it_on_the_line_headless`). A person reads
the PNG for the look; the record is what a test asserts
(`qt_pointer_steps_hover_and_click_the_plan_view_headless`,
`qt_insert_vertex_shows_where_the_vertex_goes_headless`,
`qt_insert_vertex_beside_the_vertex_clicked_first_headless`,
`qt_insert_vertex_beyond_a_chosen_corner_shows_enter_not_a_refusal_headless`,
`qt_insert_vertex_refuses_a_choice_edited_since_headless`,
`qt_insert_vertex_is_answered_with_what_it_did_headless`,
`qt_delete_vertex_takes_the_chosen_vertex_headless`,
`qt_delete_vertex_previews_the_click_not_the_chosen_vertex_headless`).
A headless run's pointer never leaves the view - there is no step for
that - so a tool started after a `~` step previews where the pointer is;
in the window, the pointer leaving the view for a menu takes the preview
away (`PlanViewTools.NoPreviewIsDrawnWhileThePointerIsOutsideTheView`).

The views are reached by their verbs (`docs/cad.md`, "The window's views:
VIEWS and ZOOM"), so a run sets up linked views as a person would and reads
back where each one is: `>VIEWS OPEN plan|>VIEWS LINK 1,2|>ZOOM CENTRE 50,40
SCALE 8 view=1|>VIEWS` prints the ZOOM's record with the line of the view that
followed it, then every view's record; `%View2` then makes that view's dock
the target, so the grab is the view with its bar
(`qt_linked_plan_views_zoom_together_headless`,
`qt_unlinking_one_of_two_views_unlinks_both_headless`). A dock is named
`View` and its id, the id a record's `view=` gives. `ZOOM SELECTION view=2`
frames the selection in view 2 and says what the scope took
(`qt_zoom_to_selection_moves_the_linked_views_headless`); a view that shows
none of it stays where it is and its record says `shown=0 moved=no`, which a
press of its bar's button reads back
(`%View2|!ViewZoomSelectionButton|>VIEWS`,
`qt_a_plan_views_zoom_to_selection_moves_nothing_it_does_not_show_headless`); and View > Zoom
To is driven by its own names - `@viewZoomTo|!zoomToScopeDrawing|zoomToFilterLayer=design|?zoomToLine|!zoomToRun|?zoomToStatus`
(`qt_zoom_to_dialog_runs_its_line_headless`).

A test binary of Qt widgets run by hand needs MSYS2's runtime first on PATH,
as ctest puts it (`KATANA_RUNTIME_BIN`): with `build/release/bin` first,
Qt loads the copy of `Qt6Core.dll` there and looks for its platform plugin
beside the test binary, in `bin/tests/platforms`, which the build does not
fill - and a GUI program with no platform plugin waits on a message box
nobody sees. `katana.exe` itself finds `bin/platforms` beside it.

## check_screenshot.cmake: the test side

Every `qt_*_headless` test in `tests/CMakeLists.txt` runs
`tools/check_screenshot.cmake` (or `tools/check_plot.cmake` for the plot,
`tools/check_sheets_headless.cmake` for the sheets) with `cmake -P`. The script COPIES the project it is given before opening it,
runs `katana` with the switches its variables ask for, and checks the result.
Its variables:

| Variable | Switch or check |
|---|---|
| `-DAPP`, `-DPROJECT`, `-DOUTPUT` | the executable, the project to copy, the PNG to write (required) |
| `-DIMPORT=<file>` | a file to import first. It is put before every switch, `--customise` above all, which takes each path up to the next switch: after it, the file was read as one more customisation file, the one `CUSTOMISE` line loaded nothing, and nothing was imported (`qt_a_customisation_folder_and_an_import_file_are_both_taken_headless`) |
| `-DCUSTOMISE_DIR=<dir>` | `--customise` with every `*.customisation.json` in it, in name order - one `CUSTOMISE` line; a directory that holds none is reported and the run goes on, so give an `-DEXPECT` naming what must have loaded (`qt_customisation_headless`) |
| `-DTOGGLE_LAYER`, `-DSTYLE_MANAGER=ON`, `-DLAYER_MANAGER=ON`, `-DATTRIBUTES=<id>`, `-DDATASET_INFO`, `-DIMPORT_OPTIONS`, `-DSELECT_ALL=ON`, `-DCHECK_SHORTCUTS=ON`, `-DCHECK_MENUS=ON`, `-DCHECK_TOOLBARS=ON` | the switches of the same names |
| `-DACTIONS=a,b` | `--action` for each, commas because a CMake list does not survive `cmake -D` |
| `-DDIALOG=NAME` (or `-DSURVEY_DIALOG`) with `-DFILL=f=t\|f=t` and `-DPRESS=a,b` | one dialog, filled and pressed |
| `-DSCRIPT=<file.kcs>` | `--script`, before the `-DDRIVE` steps, so they can list what it made |
| `-DIMAGE_SIZE=<file>\|<width>\|<height>` | the run must write a PNG of exactly that size (a `SNAPSHOT`'s), read from its header; the file is removed first |
| `-DDRIVE=<step>\|<step>...` | the steps, by sigil, `\|` between them |
| `-DEXPECT=<regex>` | what the run printed must match |
| `-DFORBID=<regex>` | what the run printed must match NOTHING, after the run's own paths are replaced by `<path>` |
| `-DREFUSED=<regex>` | the run must be REFUSED - exit 1, never a crash - and print a match |
| `-DCOMPARE=<reference>\|<file>...` | every file the run writes must hold exactly the reference's text, line ends aside; the files are removed first |

`-DREFUSED` is how a test shows something is NOT offered: pressing a disabled
button, or filling a choice the dialog does not have, fails the run instead
of doing nothing. Give it a regex that names what came before the refusal as
well, so that a run stopped earlier for another reason cannot pass.

`-DFORBID` is how a test holds what the application SAYS to a vocabulary:
the Format menu and the customisation log name no other program, none of its
file suffixes, and neither of the two menu items that loaded them
(`qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless`,
`-DFORBID=12[dD]|...`). It is matched after the run's own paths - the copy, the
output, the project, the customisation folder, the work folder, the
application's folder and the checkout - are replaced by `<path>`, so where a
checkout or worktree sits cannot fail it; the rest of a line that holds a path
is still read, and a failure prints the text that was checked. Pair it with an
`-DEXPECT`, so that a run which printed nothing cannot pass for one that said
nothing wrong. `docs/survey.md` has the survey import wizard's use of these.

Before any of `-DEXPECT`, `-DREFUSED` and `-DFORBID` is matched, the script
drops the whole lines in which Qt's platform layer says the plugin cannot do
a window-management request - "This plugin does not support
propagateSizeHints()", `raise()`, `lower()`, window opacity or masks, keyboard
or mouse grabs. Linux's offscreen plugin prints the first whenever a window's
minimum size changes (a dialog shown, views arranged), Windows' does not, so
on Linux it fell between two report lines an `-DEXPECT` pairs and failed four
checks that pass on Windows. They are Qt's words about the platform, not the
application's, and dropping only those whole lines keeps every check able to
say that two reports are adjacent; loosening the checks to `.*` between the
lines was rejected for that reason. A note that can matter to a check - no
Vulkan instance, no system fonts, no OpenGL context - is not in the list and
is still matched.

A PNG must also be of plausible size (a window that painted only its
background compresses to a few kilobytes), and PDF and PNG headers are
compared as hex: CMake's plain `file(READ ... LIMIT 4)` returned `%PDF` plus a
newline on this platform and failed a good file.

## A headless session never opens a modal box

A modal box in a headless run is a hang until the test's timeout. So
`MainWindow::setHeadless` turns every question into a sentence:

- the "far from the current drawing" question keeps survey coordinates and
  says so, and every "Import failed" box (`warnUser`) becomes a line in the
  log - a scripted import of a missing file used to sit on a warning box until
  the harness killed it;
- a question the window would have to ask - discard unsaved changes, discard
  the code manager's unapplied edits - is REFUSED and said rather than
  answered: a scripted `QUIT` with unapplied code edits, or `NEW` after an
  edit, fails, and the script can Apply or Revert, `SAVE` or `UNDO` first
  (`desktop.md`, "Failure modes");
- File > Open, Save As (and Save of a drawing with no project), Import Any
  File and Export Drawing open no file dialog and name the line that does the
  same - `OPEN <directory>`, `SAVE <directory>`,
  `IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]`, `EXPORT <file>` - since
  `--trigger` reaches them by their object names
  (`MainWindow::refuseFileDialog`,
  `qt_the_file_items_name_their_verbs_in_a_headless_run_headless`); Export of
  an empty drawing says so in the log, not in a box. The GIS menu's Import
  Vector Data, Import Raster, Import Point Cloud and Dataset Information ask
  for a file first as well, and were not guarded until 2026-10-07: each now
  names `IMPORT <file>` with its own kind's words, or `INFO <file>`
  (`qt_the_gis_items_that_ask_for_a_file_name_their_verbs_in_a_headless_run_headless`),
  and its dialog is still reached with `--import-options` or
  `--dataset-info`. Format > Load
  Customisation and Replace Loaded Customisation were two more: they are no
  menu items any more, and a `--trigger` of either fails the run as the name
  of no menu item (`qt_load_customisation_is_no_menu_item_headless`,
  `qt_replace_customisation_is_no_menu_item_headless`) - a customisation file
  is the `CUSTOMISE` line or `--customise`;
- File > Run Script's Browse opens no file dialog and says to fill
  `scriptPath` instead; a script's run shows no progress dialog, and the
  person's Recent Scripts list is left alone;
- GIS > Convert Point Cloud to COPC opens no file dialog and names the verb
  that asks nothing, `COPC <source> <destination.copc.laz>`
  (`qt_copc_typed_on_the_windows_command_line_converts_the_cloud_headless`);
- the Sheets editor (`SheetEditor::setHeadless`, set by File > Sheets) opens
  no file dialog either: an image view's Browse names `VIEW SET id
  file="path"`, and Sheet Set's Save, Load and Append name `SHEETS SAVE`,
  `LOAD` and `APPEND`; Load's "Replace the sheets?" is never asked. Its
  Generate Sheets and Page Setup open without waiting, so
  `*fileSheets|@sheetGenerate` and `@sheetPageSetup` fill them by their
  object names (`qt_generate_sheets_runs_the_line_it_shows_headless`,
  `qt_sheet_set_menu_runs_its_lines_headless`); a dialog goes when it
  closes, so a step after its OK names a new target (`%sheetToolBar`);
- Edit > Attributes, still `exec()`'d, logs that it is modal and names the
  switch that grabs it (`--attributes`) instead of opening; Format > Layers
  is non-modal and opens as any manager does;
- the Format workbench tells each manager (`CustomisationServices::headless`),
  so the symbol library and the code manager open no file dialog and the code
  manager's close asks nothing, and Purge Unused does not ask.

## Not done

- No step presses a key into a view (`&KEY` was proposed for a view's own
  keys: Delete on a hot vertex, Ctrl held over a grip). Enter reaches a view
  through `>` alone; the rest is tested in `tests/qt_widgets` with real key
  events, where Qt's shortcut map is not in the way, so Delete through the
  window's shortcut map is not driven end to end.
- No step drags: `+` is a press and a release at one point. A grip drag is
  a click to pick the grip up and a second click to put it down.
- A `--fill` of a list selects the row, as a click on its text does, but never
  ticks a checkable row: the shared scope widget's "The checked layers" list
  (`vectorExportLayers`, `globalModifyLayers`) cannot be ticked by a step. A
  run reaches the same export through the filter's layer field
  (`vectorExportFilterLayer=other`) or types the `LAYERS` line with
  `--run-line`. A `ROW=on` form of `--fill` for checkable rows would close it.
