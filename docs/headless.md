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
katana_cli -c "RECT 0,0 30,20" -c LIST
katana_cli --help                         every verb it knows
```

Scripts and `-c` batches stop at the first failing command and exit 1, so the
tool composes with shell pipelines. The `cli.*` tests
(`src/katana_app/CMakeLists.txt`) use it to draw, save, reopen in a SEPARATE
process and check the geometry, to confirm that invalid input fails the
process (`cli.rejects_bad_input`), and to exercise the survey-code verbs on a
code file the tests write themselves.

## katana with --screenshot and --plot

```sh
QT_QPA_PLATFORM=offscreen timeout 120 ./build/release/bin/katana.exe \
    <copy-of-project> [files to import] --screenshot out.png [steps...]
QT_QPA_PLATFORM=offscreen timeout 120 ./build/release/bin/katana.exe \
    <copy-of-project> --plot out.pdf [--fit | --scale N] [--paper A3] [--landscape | --portrait] [--dpi N]
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
the resolution. The dialog and the switch share `plotDrawingToPdf`, so the
`qt_plot_headless` test exercises the code the menu does.

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
| `--plot-sheets PDF` | plot every sheet of the project to one PDF, a page a sheet, and exit; a project with no sheets plots one fitted to the drawing. What the sheet checks find is logged to stderr first (`docs/plotting.md`, "Preflight checks") |
| `--customise FILE...` | load style libraries and survey code files - every path until the next switch - merged into the built-in customisation before anything is drawn |
| `--action NAME` | trigger the menu item with that object name after the imports, as a click does; repeatable, in order, before the steps |
| `--select-all` | select every entity on an unlocked layer before the actions run |
| `--toggle-layer NAME` | flip the layer's visibility box through the layer panel and refuse to go on if the panel was rebuilt inside its own signal (`desktop.md`, "Panels refresh on the event loop") |
| `--style-manager` | open the styles and linetypes manager and grab it |
| `--layer-manager` | open the Layers dialog and grab it; Format > Layers is non-modal, so `--dialog formatLayers` reaches the same dialog as a step |
| `--attributes [ID]` | open the attribute manager (Edit > Attributes, still modal in a person's session) on entity ID and grab it |
| `--dataset-info FILE`, `--import-options FILE` | build GIS > Dataset Information or the GIS import dialog for FILE and grab it |
| `--check-shortcuts` | fail the run when a key reaches more than one thing (`desktop.md`, "Every key reaches one thing") |
| the steps | below |

## The steps: driving dialogs, docks and the command line

The steps run in the order given, with the event loop run between them as it
runs between two things a person does, so what one step sets going has
happened before the next. Everything is found by OBJECT NAME, which is why
every action, field, button and tab gets one. In a test they are one
`-DDRIVE=` list of `tools/check_screenshot.cmake`, a step per sigil:

| Switch | `-DDRIVE` step | What it does |
|---|---|---|
| `--dialog NAME` (first called `--survey-dialog`, still accepted) | `@NAME` | triggers action NAME as a click does and makes the dialog it opened the target: the dialog named by the action's data (the Format managers carry `styleManagerDialog`, `symbolLibraryDialog`, `surveyCodeManagerDialog`), else NAME + `Dialog` (the Survey dialogs, `formatLayersDialog`); says on stderr what opened, and whether it is modal |
| `--survey-dock ACTION` | `#ACTION` | shows the dock that action shows and makes it the target; at the end its status line is printed |
| `--panel NAME` | `%NAME` | makes the window's own dock, toolbar or menu NAME the target; a menu is opened under its title, so a grab shows what it offers |
| `--fill FIELD=TEXT` | `FIELD=TEXT` | a line or text box (`\n` a line break), a choice by its text (an editable one takes a name it does not list, as typing does), a spin or check box, a tab brought to the front by its text (`managerTabs=Linetypes`), or a list, grid or tree row selected by its text - the whole row where the view selects rows, as a click does |
| `--press BUTTON` | `!BUTTON` | clicks it; a disabled button fails the run |
| `--command TEXT` | `>TEXT` | runs TEXT as if typed on the command line - make styles and a selection, or start a tool by its alias and answer its prompts |
| `--enter` | `>` alone | Enter on an empty command line (an empty argument does not survive a CMake list) |
| `--report NAME` | `?NAME` | prints on stderr what the target's widget NAME shows - a label's text, a field's, a list's rows - or, for one of the window's actions, its text and whether it is checked (which tool the menus show running); for one of the window's menus (`formatMenu`), its title and every item with the status tip it shows, without opening it |
| `--trigger NAME` | `*NAME` | triggers menu item NAME in its turn among the steps (`--action` runs before them all) |

What the target is at the end is what `--screenshot` grabs; steps that were
all commands leave the window. A step that cannot be taken - an unknown
dialog or widget, a fill of a choice the dialog does not offer, a press of a
disabled button - ends the run with exit 1 and a sentence on stderr, never a
crash and never a silent no-op. The names a dialog's fields carry are listed
in its header (`survey/survey_dialogs.hpp`, `survey_import_wizard.hpp`,
`survey_points_ui.hpp`, the managers' own).

A headless run echoes its command log to stderr, which is where a test reads
what a command REPORTED.

## check_screenshot.cmake: the test side

Every `qt_*_headless` test in `tests/CMakeLists.txt` runs
`tools/check_screenshot.cmake` (or `tools/check_plot.cmake` for the plot)
with `cmake -P`. The script COPIES the project it is given before opening it,
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
- Edit > Attributes, still `exec()`'d, logs that it is modal and names the
  switch that grabs it (`--attributes`) instead of opening; Format > Layers
  is non-modal and opens as any manager does;
- the Format workbench tells each manager (`CustomisationServices::headless`),
  so the symbol library and the code manager open no file dialog and the code
  manager's close asks nothing, and Purge Unused does not ask.
