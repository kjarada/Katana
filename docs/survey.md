# The Survey module and instrument interoperability

`PLAN.MD` section 45 is the programme; this file is the record of *why* it is
shaped the way it is. The short version: a survey that is read slightly wrong
looks exactly like a survey that is read right, so almost every decision here
buys the ability to fail loudly instead of quietly.

## What was already here

Four of the fifteen phases the brief asked for were already built, and this is
the first thing to know before touching any of it:

| Asked for | Already in the repository |
|---|---|
| Survey calculation engine | `katana::survey` - `cogo.hpp`, `traverse.hpp`, `angles.hpp`, `leveling.hpp`, `least_squares.hpp`, `network_adjustment.hpp`, `error_propagation.hpp`, `statistics.hpp` |
| Normalized survey model | `survey/data_model.hpp` - `SurveyPoint` and four observation kinds |
| Coordinate systems | `katana::geodesy` - CRS, transformer, units, ellipsoid, geodesic, grid factors, PROJ behind `src/` |
| XML reading | a strict reader, then private to `archive12d`, now `katana::core::readXml` |

So this programme is an *exchange* subsystem and a *user interface*, not a
second survey library. The conventions it obeys were set by `data_model.hpp` and
are not restated per field: metres, **radians**, azimuths clockwise in
`[0, 2*pi)`, and ordered containers so that no result depends on hashing.

## Where the manufacturer code is allowed to live

`surveyio` is a layer of its own, allowed `core;math;geometry;survey` in
`tools/check_layering.cmake`. Two absences in that list are the design.

**`cad` may not see `surveyio`**, exactly as it may not see `interop` or
`archive12d`. The brief asks that the CAD application not become dependent on
Leica, Trimble or Topcon structures. A comment saying so would be a wish; a
layering rule is a build failure. `app` and `qt` own the parsers, and `cad`
only ever sees `katana::survey` values, which it was already allowed to see.

**`geodesy` is not in the list either**, which is the more interesting one. A
parser records the coordinate system a file *declares* - a name, an EPSG code,
or explicitly unknown - and transforms nothing. The brief says never to
transform coordinates without knowing both systems; making the transformation
unreachable from the parser is stronger than remembering not to call it. The
transformation is an act the user authorises in the wizard, with both systems
named on screen.

A third consequence, inherited from `archive12d`'s reasoning: because `surveyio`
needs no third-party library, it builds with `-DKATANA_BUILD_IO=OFF`. That puts
a hand-written parser of untrusted field data under the sanitizer job, which is
where a parser of untrusted input belongs.

## Shared foundations, and the three things there was briefly more than one of

The first round of parsers was written by five authors at once against a model
that did not yet have what they needed, and each of them filled the gap
privately. Three of those gaps are now closed in the layer below, so that no
parser carries its own answer:

| What | The one place | What it replaced |
|---|---|---|
| Bytes to UTF-8 | `katana::core::decodeText` / `decodeTextAs` (`core/text_encoding.hpp`), moved down from `archive12d` | a second decoder written into the CSV reader because surveyio may not see archive12d |
| Trim, integers, reals, lines | `katana::core` (`core/text.hpp`) | private trims in the XML reader and the 12d reader, and a third in the Trimble parser |
| Metres per foot and link | `katana::math::units` (`math/unit_ratio.hpp`), which `geodesy/units.cpp` is now built from; `survey::metresPer(LinearUnit)` maps a declared unit onto it | ratios repeated in the CSV template and as rounded doubles in the Topcon parser |

Two properties of the text helpers are load bearing. They never consult the C
locale: `std::isspace(0xA0)` is true in a Latin-1 locale and 0xA0 is the second
byte of "à" in UTF-8, so a locale-dependent trim cut characters in half. And a
number must occupy its whole token with at most one sign, so "+-1", "1,5",
"inf" and "1e999" are refusals rather than values.

**Absent is not zero.** `SurveyPoint::elevation` is `std::optional<double>`.
A coordinate list with no height column, a GSI block with no word 83, a
two-ordinate LandXML point: each is a mark whose height nobody measured, and 0.0
in its place is a real height at the datum. The bridge writes no elevation
property for such a point - the surface builder already leaves a point without
one out of the triangulation - and reports how many there were. A level
adjustment refuses a held or weighted benchmark that has no published height,
because starting it at zero levels the whole run onto the wrong datum and
reports a perfect fit; a free point may start anywhere, since the model is
linear. The rejected alternative was a companion `hasElevation` flag: a flag
and a value can disagree, and every reader of a bare double can forget the flag,
where an optional has to be opened.

**Named is not positioned.** A raw observation file names its targets and gives
them no coordinates until something reduces the observations. Two of the first
parsers improvised - one created such a point at the origin with a marker in its
metadata, another at placeholder ordinates - and either reaches the drawing as a
real mark at (0, 0) as soon as one consumer forgets the marker. Such a point is
now a `survey::UnpositionedPoint` in `SurveyProject::unpositionedPoints`: it
shares the id space with `points`, observations, stations and features may refer
to it, and the bridge cannot draw it because it is not in the list the bridge
draws; the report counts them and says why. Rejected: an optional northing and
easting on `SurveyPoint`, which lets the two disagree and makes every horizontal
consumer - the network adjustment, the COGO, the bridge - check for a case most
of them can never meet.

## The pipeline

```
field file ──> surveyio (parse only) ──> survey::SurveyProject ──> cad bridge ──> one Command ──> Document
                 declares a CRS,                                    layers, styles,      one undo step
                 transforms nothing                                 codes
```

The bridge returns a single `CommandPtr`, the same shape
`cad/survey_coding.hpp` already uses. That is what makes an entire import one
undo step, which the brief asks for in its section 19 - not a special case, just
the command pattern the application already has.

## How five parsers were written at once without colliding

Each format registers itself from its own translation unit through
`FormatRegistration` (`include/katana/surveyio/format.hpp`), and
`src/katana_surveyio/` is globbed. Adding a parser is adding a file: no shared
table, no shared CMake list, nothing for two authors to edit at the same time.

That design has one trap, and it was measured rather than guessed. A linker
pulls a member out of a static archive only to satisfy an undefined symbol, and
a self-registering object file defines nothing anyone references - so it is
dropped, the format vanishes from the registry, and the only symptom is a file
being reported as unrecognised months later. The measurements are recorded in
`src/katana_surveyio/CMakeLists.txt`; the outcome is that the module is an
`INTERFACE` target wrapping its archive in `WHOLE_ARCHIVE`, so that no consumer
has to remember the rule. A correctness rule that must be remembered in `app`,
in `qt` and in every future consumer is a rule that will be forgotten in one of
them.

## What "supported" is allowed to mean

The brief's section 23 is the rule and the UI carries it. A `FormatDescriptor`
states the exact variant, what it can carry, whether import and export work, and
the version of *this parser*. So the application says

> Leica GSI-16 - import: yes, export: yes, parser 1.0

and never "all Leica files supported". Three consequences:

- **Nothing is implemented from a guessed binary layout.** Formats whose
  structure is proprietary and undocumented are refused *by name*, with a
  message saying why and what to export instead. Refusing a Trimble `.job`
  clearly is a feature; guessing at it is the thing the brief forbids.
- **Detection returns a ranked list with an explicit uncertain outcome** that a
  caller cannot ignore, so an unknown file is never handed to an incompatible
  parser. A probe sees the first few kilobytes and the file's *name* - never a
  path, and never anything it could open.
- **Every candidate carries its evidence** ("extension .gsi, every record 24
  bytes, word 11 first"), because a detection a person has to second-guess is
  one they cannot check.

## The two ways survey data gets silently corrupted

Both are called out here, and in `PLAN.MD` 45.5, because a test that does not
aim at them will pass straight over the bug.

**GSI encodes the decimal position inside the word.** It is not a decimal point
in the data. Read the data block as a plain integer and the coordinate comes out
wrong by orders of magnitude - and still looks like a survey coordinate. Every
expected metre value in the GSI tests is worked out by hand from the
specification with the arithmetic in a comment, per `CLAUDE.md` section 3.

**Coordinate order differs between families.** LandXML `CgPoint` content and
Trimble exports are northing-first; much else is easting-first. A transposed
survey is the worst kind of defect: it is self-consistent, it plots, and it is
in the wrong place. Where a file is genuinely ambiguous the importer is *told*,
never left to guess, and the fixtures use northing and easting of unmistakably
different magnitudes (northing 5 000 000 against easting 500 000) so that a
transposition cannot pass a test by coincidence.

## Test fixtures

Constructed from specifications, in `tests/surveyio/data/`. No proprietary
sample files are used, which the brief requires and which also means the
fixtures can say what they are testing: a hand-built record exercising one
documented field is worth more than a real file that happens to contain it.

## Compatibility matrix

Filled in as each parser lands, and it states formats and variants rather than
manufacturers. Anything not listed here is not supported, whatever its
extension.

| Format (id) | Import | Export | Parser | Notes |
|---|---|---|---|---|
| Delimited point files (`delimited-points`): CSV, TXT; comma, tab, semicolon or whitespace; any column order a layout states | yes | yes | 1.0 | declares no coordinate system; the unit is stated by the person; the column order is never guessed |
| Opcode field file (`opcode-field-file`): `.fld`, first line `{Version 6.0}`, tab-separated records opening with a numeric opcode | yes | no | 1.0 | opcodes 02, 03, 04, 05, 06, 07, 09, 29, 41, 72, 73, 100 and -2 read; every other opcode skipped with a warning naming it; the coordinate system is declared by name from the header comments, never as a guessed EPSG code |

That is the one reader on main as of 2026-09-24. The instrument parsers this
file describes above (GSI, the Trimble and Topcon exports, LandXML survey
data) are not in `src/katana_surveyio/`, and the import wizard refuses any
other registered format by name; `chooseFormat` in the wizard is where their
dispatch will go when they land.

## The opcode field file (.fld)

Added 2026-09-29, when the owner asked Katana to read a data collector's
"field file" (`260825kj.fld`, a total-station job with utility attributes,
kept on the owner's machine and not committed). Its first line is
`{Version 6.0}`, its comments call it "Field File Version 6", and every
record is tab-separated and opens with a numeric operation code - the format
whose publisher documents it as the "Field File Format" (a reference manual
chapter; the June 2025 edition was read: sections 1.2 "Structure of the .fld
File", 1.3 "Point Description" and 1.8, one entry per opcode). Nothing in
Katana read it before: no probe claimed a `.fld`, and the wizard called the
file unrecognised. The reader is
`src/katana_surveyio/opcode_field_file.cpp`, on the Topcon readers' raw
builder (`src/katana_surveyio/topcon_raw_builder.hpp`), which already turned a
journal of setups, backsights and shots into one `survey::SurveyProject` - a
third copy of that bookkeeping would have been the defect section 2 of the
contributors' rules names.

What the format says, and the reader relies on: most records carry a point
description of five tab-separated values (feature code, string number, point
ID, point name, point comment); 02 is an entered coordinate (X, Y, Z), 03 a
setup with its instrument height, 04 the backsight, 06 a check measurement and
07 a shot (horizontal circle, vertical circle, slope distance, decimal
degrees); 05 sets the target height for what follows; 09 is a scale factor for
later slope distances; 29 a memo; 41, 72 and 73 add text, a real and a text
attribute to the point just measured; 100 gives the units, of which the format
allows one each - decimal degrees and metres - so any other is refused.

Decisions that are the reader's own, each stated in the source:

- **The column after the opcode.** Every record in the owner's file has an
  empty value between the opcode and the description ("07, blank, KJ, 01,
  KJ01 ..."); the manual's syntax lines do not show one. The records whose
  opcode has a fixed number of values (02, 03, 04, 06, 07) vote, before any is
  read, and the file is read in the layout they choose. Rejected: deciding per
  record, because a record in the column's layout that has lost a value has
  the plain layout's count, and a feature code left empty makes the first
  field empty in both - one such record would read a zenith as a slope
  distance. A record that does not fit the file's layout is skipped with a
  warning.
- **X is the easting.** The owner's file puts six-figure eastings in X and
  seven-figure MGA northings in Y, as the map-grid convention does.
- **A point is its name, else its ID** (the manual's 1.5 finds a setup by
  either); a point with both keeps the ID in its metadata.
- **The header comments are metadata, and the coordinate system is declared
  by name.** "// Coordinate System: Australia/GDA2020" and "// Zone: Zone 56"
  become the declared system "Australia/GDA2020, Zone 56", exactly as
  written. Turning that into EPSG:7856 would be a guess from free text; the
  person states the code in the wizard (or `CRS SET` sets the drawing's).
- **72 "Target height" and "Prism constant" are kept as attributes, not
  applied.** 05 is the format's target height, and the prism constant is
  written without a unit.

The owner's file, read on 2026-09-29 (a local check; the file is not in the
repository): 13 950 records read - every line but the 16 comments - none
skipped, no warning; 8 setups; 2 628 observations, 876 pointings of a
direction, a zenith and a slope distance (867 shots, 8 backsights, one check
measurement); the 5 entered control marks; 857 points without coordinates,
each with its utility attributes (QualityLevel, Depth, Material ...); 134
coded strings; declared "Australia/GDA2020, Zone 56". Imported with the
drawing on EPSG:7856 - through `SURVEY IMPORT` and through the wizard with its
defaults, the same result - it is 862 points as one survey job. KJ01, the
second setup, lands at 328904.232 E 6253485.838 N; radiating its first
face-left shot from GB14, oriented on GB15, by hand gives 328904.235 E
6253485.836 N, 3.6 mm away - well inside what meaning its six pointings can
move it, since their face pairs disagree by up to 47" (8.7 mm at 38 m). The
reduction's 8 warnings are the
file's: three face pairs on KJ01 from GB14 differ by 31" to 47" horizontally,
three on KJ02 from GB16 by 80" to 83" in zenith, and the file does not say
whether the atmospheric correction or the prism constant is in its distances.
`tests/surveyio/test_opcode_field_file.cpp` works on a hand-built fixture,
`tests/surveyio/data/fld/setup.fld`, in the same layout.

**`SURVEY READ` and `SURVEY IMPORT`** (`src/katana_app/survey_verbs.hpp`)
bring every format surveyio reads to `katana_cli`, `katana_mcp` and the
window's command line, which until now reached a field file only through the
import wizard. `SURVEY READ <file> [FORMAT <id>]` says what the reader made of
the file and changes nothing; `SURVEY IMPORT <file> [FORMAT <id>] [LAYER
<path>]` does what the wizard's Import does with its defaults - the reduction
with `ReductionSettings`' defaults and the control the file declares, the
points on `survey/points`, the job kept for Survey > Survey Jobs - as one undo
step (`cad::ImportSurveyJobCommand`). The format is detected unless FORMAT
names it, and a detection that is not certain is refused, naming the
candidates. The verb lives in the session and not the interpreter because
`cad` may not see `surveyio`; the window runs the same function
(`MainWindow::runWorkbenchLine`). `surveyio::reportInputFor`, which the
wizard and the Survey Jobs dialog had in the window's code, moved to
`include/katana/surveyio/reader.hpp` so the verb uses the same one. Tests:
`cli.survey_read_field_file`, `cli.survey_import_field_file`,
`qt_survey_verb_headless`.

Not done: opcodes 10, 11 and 12 (stadia, HA HD height, HA HD VD) and the
string operations (joins, closes, arcs) are skipped with a warning; the format's XML
form is not read; the verb has no reduction options - a
person changes them in Survey > Survey Jobs, which re-adjusts the imported job.

## The Survey menu: tools, the import wizard, and the points in the drawing

The owner asked on 2026-09-23 for "survey menus and toolbars" with the
functionality wired. The menu (`src/katana_qt/survey/survey_workbench.*`)
has four sections - Survey Points (Import, Export, Point Manager, Point
Report), Coordinate Geometry (Inverse, Forward Point, Area of Selection, Parcel
Report, the Angle and Bearing Calculator), Traverse and Levelling (Traverse,
Level Book) and Coordinates (the Coordinate Converter) - plus a Survey Coding
section that shows actions the window and the Format workbench own: the
Survey Code Manager,
Load Customisation, Replace Loaded Customisation and Apply Survey Codes
(`docs/survey_coding.md`). The first three are the same `QAction` objects as
on the Format menu (`SurveyServices::codeManager`, `loadCustomisation`,
`replaceCustomisation`), so the two menus cannot drift; Apply Survey Codes
(`applySurveyCodes`) is on this menu and the Survey toolbar only. The menu
ends with Subsurface Utilities (AS 5488), added by a workbench of its own
(`src/katana_qt/survey/utility_workbench.*`, `docs/subsurface_utilities.md`).
`MainWindow` only makes the menu and toolbar - on a second toolbar row, with
Terrain and GIS - and hands them over through `SurveyServices`, so the survey
work never includes `main_window.hpp`.

**Nothing here is new surveying** (`include/katana/cad/survey_tools.hpp`).
Every number comes from `katana::survey` (cogo, traverse, levelling, the
network adjustment, angles) or `katana::geodesy`; the tools move drawing
entities in and out of those libraries and put the answers into words. Each is
a structured result plus ONE formatter, and the dialogs and the command line
(`INVERSE`, `FORWARD`/`RADIATE`, `AREA`) print the same formatter's text, so a
number cannot appear only in a dialog. The dialogs are non-modal and kept by
the workbench between uses, so one can stay open beside the drawing; Use
Selection reads the selected points when pressed, never before. The
conventions are inherited, not invented: drawing x is easting and y northing,
where `survey::Coordinate2` is (northing, easting), and the swap happens in
`survey_tools.cpp` only; angles are radians inside, azimuths clockwise from
grid north; heights are optional and ABSENT IS NOT ZERO, so an inverse to a
point with no height reports no height difference; lengths are in the
project's unit, and hectares are reported only when that is the metre; no
scale factor, convergence or curvature is applied (the converter REPORTS the
grid scale factor and convergence and applies neither). Angle text is what
`survey::parseDms` and `parseBearing` accept - `36d52m11.63s`, `36:52:11.63`,
`36-52-11.63`, `36 52 11.63` in a box of its own, decimal degrees, or a
quadrant bearing `N 36d52m11.63s E` - and there is deliberately no DDD.MMSS
notation, since `36.5211` would then mean two angles depending on who typed
it. In a line of several fields (a traverse leg) blanks separate the fields,
so an angle there has none inside it.

**Parcel Report** (`src/katana_qt/survey/survey_parcel_dialog.hpp`) is the
window's way to `PARCEL`, which had none: a closed polyline's courses (from
corner, quadrant bearing to whole seconds, distance) in a table, its area,
perimeter, centroid and the direction it was drawn, the legal description
with the name typed, and Label Courses. The numbers and the words are
`cad::parcelReport`, `cad::formatParcelReport` and `cad::legalDescription` -
the pane shows exactly what `PARCEL id` prints and the Legal Description tab
what `PARCEL id LEGAL name` does. Label Courses is the line `PARCEL <id> LABEL
<height>` through the window's executor (`SurveyServices::run`), one undo
step, and its reply names the layer the labels went on. Copy and Save CSV
(`cad::parcelCoursesCsv`, RFC 4180, a negative coordinate's sign kept) and the
legal text's Save open no file dialog in a headless run (`SurveyServices::headless`).
Its menu item has no mnemonic: every letter of "Parcel Report" is another
item's in that menu. Tested by `tests/qt_widgets/survey/test_parcel_dialog.cpp`
and, through the window, `qt_the_parcel_report_computes_labels_and_describes_a_lot_headless`. Not done: the report
is of the parcel as it was at Compute, as every survey tool's is, and is not
recomputed when the boundary is edited.

**The points in the drawing** (`include/katana/cad/survey_points.hpp`). A
SURVEY POINT is a point entity carrying the point-number property the import
writes (`point` by default); a point without one is a CAD point, left out of
every list and, where asked for by id, counted as left out. The Point Manager
(a dock, `SurveyPointsDock`, filterable and read-only) and the Point Report
(text, or CSV by `cad::pointReportCsv`, a small RFC 4180 writer, since the
report carries columns a point file does not) both read them through the
import's own keys, so a point reads back as it went in.

**The import wizard** (`survey_import_wizard.*`) is a paged dialog in six
steps - File, Format, Layout, System, Options, Report - and a `QDialog`, not a
`QWizard`, because `QWizard`'s buttons have private names and the headless
driver presses buttons by name. Its rules are the ones this file started
with:

- **The format is never assumed.** Step 2 lists every candidate with its
  evidence and its `FormatDescriptor` record; a detection that is not
  Identified - and a delimited file never is, its probe being weak by design -
  starts on "(choose the format)", and Next refuses that.
- **The column order is never guessed.** Step 3 shows `proposeLayout`'s
  reading of the header, a role box per column of the list as typed (kept
  while a change such as swapping northing and easting passes through a list
  that does not validate yet), the delimiter, header lines, comments,
  quoting, saved layout templates and a preview with the parser's error, line
  and column, inline. When the proposal is Uncertain, Next needs the person to
  tick that they have checked the order of northing and easting.
- **The unit has no default and nothing is transformed silently.** Step 4
  takes the unit the numbers are in, the system the file is in (unknown unless
  stated) and, only when BOTH a source and a target EPSG code are given, a
  horizontal transformation through `katana::geodesy`
  (`cad::transformSurveyProject`, with the unit conversion on both sides;
  heights pass through unchanged and the report says so). The parser still
  transforms nothing. Step 3's preview parses in metres and labels its
  numbers "as written in the file", since the unit comes a step later.
- **Ids the drawing already has** (`cad::ExistingPointPolicy`): Refuse (the
  default - a clash more often means the wrong file than a wanted update),
  Skip, Replace (the drawing's point deleted in the same command, so one undo
  puts it back) or Keep Both (the report says the drawing now has two).
- **One command.** Import makes one undoable command
  (`cad::importSurveyPoints`), frames the views and logs the report; step 5
  can apply the loaded survey codes afterwards with the window's own Apply
  Survey Codes action. After Import the wizard hides and goes back to step 1
  with its fields kept, as a wizard's Finish does.

Saved layout templates live in the user's settings (`survey/templates`), and
every open template list - the wizard's and the Export dialog's - is refilled
at once when one is saved or deleted in the process
(`keepTemplateChoiceCurrent`), and on show for one saved by another Katana.

**Driven headlessly.** Every dialog, field and button has an object name
(listed at the head of each dialog's header), and `katana` takes
`--dialog ACTION` (repeatable; first called `--survey-dialog`, which still
works), `--fill field=text`, `--press button` and `--survey-dock ACTION`,
among the other steps `docs/cad.md` tables (`--command`, `--enter`,
`--report`, `--trigger`, `--panel`); `--fill` runs the event loop after each fill, as
happens between two user actions, and `--press` on a disabled button fails the
run rather than doing nothing. `tools/check_screenshot.cmake` strings these
into `-DDRIVE=@surveyImport|file=...|!next|...|!import|#surveyPointManager|filter=CP`,
checks the log with `-DEXPECT`, the files written with
`-DCOMPARE=reference|file...` (the export round trip writes, re-imports with
Replace and writes again, and both files must equal the reference), and
requires a refusal with `-DREFUSED=<regex>` - exit 1, never a crash, with the
regex naming what came before the refusal so a run stopped earlier cannot
pass. Fourteen `qt_survey_*` tests run this way.

The Point Manager dock wears the window's dock chrome - minimise to the tray,
float, close - through `SurveyServices::chrome`, which the workbench tells to
forget the dock before deleting it.

Not done: Process Linework is reached only through the Survey Code Manager's
Linework tab, Draw Survey Features not at all, and the wizard does not call
`drawSurveyFeatures` (`docs/survey_coding.md`); the Point Manager is
read-only, and its chrome has been seen only in a screenshot; the survey tools
are not in the interactive-tool catalogue (`docs/cad.md`); no headless test
presses Use Selection; under the offscreen platform there is
no monospace font, so report columns look misaligned in the test PNGs; two
headless tests write a template to the user's settings and delete it in the
same run, so a killed point-report run leaves one behind. A name holding one
number followed by coordinates ("P1 500000 7000000 0") is still read as the
label "P1 500000", which cannot be told apart from "CP 1 500000 7000000".
