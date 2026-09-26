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
| `--customise FILE...` | load style libraries and survey code files - every path until the next switch - merged into the built-in customisation before anything is drawn |
| `--action NAME` | trigger the menu item with that object name after the imports, as a click does; repeatable, in order, before the steps |
| `--select-all` | select every entity on an unlocked layer before the actions run |
| `--toggle-layer NAME` | flip the layer's visibility box through the layer panel and refuse to go on if the panel was rebuilt inside its own signal (`desktop.md`, "Panels refresh on the event loop") |
| `--style-manager` | open the styles and linetypes manager and grab it |
| `--layer-manager` | open the Layers dialog and grab it; Format > Layers is non-modal, so `--dialog formatLayers` reaches the same dialog as a step |
| `--attributes [ID]` | open the attribute manager (Edit > Attributes, still modal in a person's session) on entity ID and grab it |
| `--dataset-info FILE`, `--import-options FILE` | build GIS > Dataset Information or the GIS import dialog for FILE and grab it; for a DXF or a .12da the import dialog is File > Import's placement step (`docs/interop.md`, "Placing an import") |
| `--check-shortcuts` | fail the run when a key reaches more than one thing (`desktop.md`, "Every key reaches one thing") |
| `--script FILE` | run a `katana_cli` script (`.kcs`) in the window, a step among the others: the window's `SCRIPT` verb through its one executor, each line its own undo step, stopping at the first refused (`desktop.md`, "Run Script"). A script that stops fails the run, exit 1. Without `--screenshot`, `--plot` or `--plot-sheets` the run is a batch, as `katana_cli`'s is: never shown, over when its steps are, and failed by the first step refused |
| the steps | below |

## The steps: driving dialogs, docks and the command line

The steps run in the order given, with the event loop run between them as it
runs between two things a person does, so what one step sets going has
happened before the next. Everything is found by OBJECT NAME, which is why
every action, field, button and tab gets one. In a test they are one
`-DDRIVE=` list of `tools/check_screenshot.cmake`, a step per sigil:

| Switch | `-DDRIVE` step | What it does |
|---|---|---|
| `--dialog NAME` (first called `--survey-dialog`, still accepted) | `@NAME` | triggers action NAME as a click does and makes the dialog it opened the target: the dialog named by the action's data (the Format managers carry `styleManagerDialog`, `symbolLibraryDialog`, `surveyCodeManagerDialog`, `textStyleManagerDialog`, `labelStyleManagerDialog`, `dimensionStyleManagerDialog`; Annotate > Edit Text carries `textEditDialog`; File > Project Coordinate System carries `projectCrsDialog`; the Annotate menu's `annotateLeaders`, `annotateLeadersForSelection` and `annotateArrangeLeaders` all carry `leaderManagerDialog`; the seven Subsurface Utilities items all carry `utilityDialog`), else NAME + `Dialog` (the Survey dialogs, `formatLayersDialog`); says on stderr what opened, and whether it is modal |
| `--survey-dock ACTION` | `#ACTION` | shows the dock that action shows and makes it the target; at the end its status line is printed |
| `--panel NAME` | `%NAME` | makes the window's own dock, toolbar or menu NAME the target; a menu is opened under its title, so a grab shows what it offers |
| `--fill FIELD=TEXT` | `FIELD=TEXT` | a line or text box (`\n` a line break), a choice by its text (an editable one takes a name it does not list, as typing does), a spin or check box, a tab brought to the front by its text (`managerTabs=Linetypes`), or a list, grid or tree row selected by its text - the whole row where the view selects rows, as a click does |
| `--press BUTTON` | `!BUTTON` | clicks it - a radio button too, which is how one is chosen (`!utilitySourceDrawing`); a disabled button fails the run |
| `--command TEXT` | `>TEXT` | runs TEXT as if typed on the command line - make styles and a selection, or start a tool by its alias and answer its prompts; without `--screenshot` the commands run before `--sheets-json` and the plots, and a refused one fails a run that writes one of them |
| `--enter` | `>` alone | Enter on an empty command line (an empty argument does not survive a CMake list) |
| `--run-line TEXT` | `<TEXT` | runs TEXT through the window's one executor, as a dialog runs the line it built (`MainWindow::runVerbLine`, `desktop.md`, "One executor: the command runner"): never a running tool's answer; prints `--run-line TEXT: ok=yes` or `ok=no`, then a `  reply: ` or `  error: ` line for each line it logged - what the dialog gets back. Without `--screenshot` it runs with the `--command` lines, and a refused one fails a run that writes |
| `--report NAME` | `?NAME` | prints on stderr what the target's widget NAME shows - a label's text, a field's, a list's rows - or, for one of the window's actions, its text and whether it is checked (which tool the menus show running); for one of the window's menus (`formatMenu`), its title and every item with the status tip it shows, without opening it; failing all of those, any of the window's own widgets, so what a dialog did to the window is read with the dialog still the target (`?FrameStatsLabel` after the utilities dialog framed the views, `qt_utility_dialog_headless`) |
| `--trigger NAME` | `*NAME` | triggers menu item NAME in its turn among the steps (`--action` runs before them all) |

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
(`qt_ifc_export_dialog_previews_each_class_and_writes_the_file_headless`). The
IFC dialogs, which run lines, echo each on stderr as "> <line>" before its
reply (`MainWindow::runIfcCommand`), so a test reads which line a press ran.

A headless run echoes its command log to stderr, which is where a test reads
what a command REPORTED.

## check_screenshot.cmake: the test side

Every `qt_*_headless` test in `tests/CMakeLists.txt` runs
`tools/check_screenshot.cmake` (or `tools/check_plot.cmake` for the plot,
`tools/check_sheets_headless.cmake` for the sheets) with `cmake -P`. The script COPIES the project it is given before opening it,
runs `katana` with the switches its variables ask for, and checks the result.
Its variables:

| Variable | Switch or check |
|---|---|
| `-DAPP`, `-DPROJECT`, `-DOUTPUT` | the executable, the project to copy, the PNG to write (required) |
| `-DIMPORT=<file>` | a file to import first |
| `-DCUSTOMISE_DIR=<dir>` | `--customise` with every `.4d` and `.mapfile` in it; absent files are reported and the run goes on |
| `-DTOGGLE_LAYER`, `-DSTYLE_MANAGER=ON`, `-DLAYER_MANAGER=ON`, `-DATTRIBUTES=<id>`, `-DDATASET_INFO`, `-DIMPORT_OPTIONS`, `-DSELECT_ALL=ON`, `-DCHECK_SHORTCUTS=ON` | the switches of the same names |
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
the Format menu and the customisation log name no other program
(`qt_the_format_menu_and_the_customisation_log_name_no_other_program_headless`,
`-DFORBID=12[dD]`). It is matched after the run's own paths - the copy, the
output, the project, the customisation folder, the work folder, the
application's folder and the checkout - are replaced by `<path>`, so where a
checkout or worktree sits cannot fail it; the rest of a line that holds a path
is still read, and a failure prints the text that was checked. Pair it with an
`-DEXPECT`, so that a run which printed nothing cannot pass for one that said
nothing wrong. `docs/survey.md` has the survey import wizard's use of these.

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
- File > Open, Save As (and Save of a drawing with no project), Import and
  Export Vector, and Format > Load and Replace Customisation, open no file
  dialog and name the line that does the same - `OPEN <directory>`,
  `SAVE <directory>`, `IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]`,
  `EXPORT <file>`,
  `CUSTOMISE [REPLACE] <file>...` - since `--trigger` reaches them by their
  object names (`MainWindow::refuseFileDialog`,
  `qt_the_file_items_name_their_verbs_in_a_headless_run_headless`); Export of
  an empty drawing says so in the log, not in a box;
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
