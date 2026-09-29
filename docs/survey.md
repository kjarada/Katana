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
| Sokkia SDR (`sokkia-sdr`): `.sdr`, the SDR33 and SDR2x layouts, a header record `00` naming `SDR33` or `SDR2x` | yes | no | 1.0 | records 00 to 13 read; records marked deleted (`DD`) skipped by name; derived views (09 MC, 11 with distances) and road, template, GPS and levelling records skipped with a warning; units from the header, an undefined one refused; declares no coordinate system |

The matrix lists the formats that have a section in this file. The other
readers on main - Leica GSI, TDS RW5, Topcon GTS-7 (GTS-6 recognised only),
Trimble JobXML, RINEX observation, and Leica DBX and the Survey Controller
DC recognised and refused - register the same way and reach the wizard,
`SURVEY READ` and `SURVEY IMPORT` through the registry, but have no row or
section here yet; that is not done.

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

## The Sokkia SDR file (.sdr)

Added 2026-09-29, when the owner asked that every example survey file be
read. The SDR file among them - a traverse of 41 setups in the SDR33 layout,
written by another program's converter, kept on the owner's machine and not
committed - was not recognised at all: `SURVEY READ` answered "no registered
format recognises this file". The reader is
`src/katana_surveyio/sokkia_sdr.cpp`, on the raw builder the field file
uses (`src/katana_surveyio/topcon_raw_builder.hpp`).

It is read from Sokkia's "Interfacing with the SOKKIA SDR Electronic Field
Book" (software 04-04.xx, October 1999): the record layouts of section
3.6.2 (SDR33, 16-character point names and reals) and 3.6.1 (SDR2x, 4-digit
point numbers and 10-character reals), the field types of 3.1 to 3.3, the
derivation codes of 3.4 and the options of 3.5. What a record means comes
from Sokkia's SDR Software Reference Manual (SETX), chapters 8, 28 and 29,
and its SDR Level 5 manual, appendices A and B; what other programs write
into the format, from Trimble's published "SDR33 Observations.xsl" and the
SDR columns in Nikon's DTM-322 manual. The two layouts list the same fields
in the same order - only a point id and a real differ in width - so one
reader reads both, choosing by the header's version.

Read: the header's units (degrees, gons or mils; metres, feet or US survey
feet, `13DU` included; mmHg, inHg or mbar; Celsius or Fahrenheit) and its
coordinate order; the job's name and correction switches; the instrument
(model, serial number, prism constant, zenith or horizon vertical
readings); weather and scale factor for the setup they are written before;
setups (02) with their instrument heights; target heights (03); backsights
(07); coordinates (08, and a 02's); raw face 1, face 2 and multiple-distance
observations (09 F1, F2, MD) as pointings of a direction, a zenith and a
slope distance; an azimuth keyed with no distance (11); sets (12); notes
(13), a time stamp also dating its setup. Every other record is skipped with
a warning naming it and its type.

Decisions that are the reader's own, each stated in the source:

- **`DD` marks a deleted record.** No published document describes the
  prefix. After it (or after `DDDD`) every such line in the owner's file is a
  complete record, and a record type is two digits, so the prefix cannot open
  a live one; the two blocks that carry it are setups begun and abandoned,
  each redone under the next live `02`. A deleted record is skipped, named in
  a warning, and changes nothing after it: a deleted `03` sets no target
  height, a deleted `02` begins no setup. Rejected: reading it as live, which
  restarts a setup the surveyor abandoned, and passing over it without a
  word, which the skipped count would then not show.
- **The 07's azimuth orients the setup.** SETX 29.2.6 orients an observation
  by the back-bearing's azimuth less its horizontal observation; the
  reduction orients a setup by `SurveyStation::backsightAzimuth` less the
  mean of its own readings on the backsight. So the azimuth goes there, and
  the circle readings, one a round, into the setup's metadata. Rejected: the
  circle reading, which the model's comment names and the RW5 and field-file
  readers store for their formats - for an SDR file whose circle was not set
  to the azimuth it drops the orientation correction silently (Sokkia's own
  chapter 4 sample: azimuth 14 degrees, circle 0). Where the circle was set to
  the azimuth, as in every round of the owner's file, the two are equal.
- **One setup per `02`, its rounds together.** The owner's file observes each
  setup in six rounds, each opened by a `07` to the same backsight at the same
  azimuth; they are one setup, and the reduction means them face pair by face
  pair. A `07` that names another backsight or azimuth, or whose circle
  reading on the backsight is more than a minute of arc from the first
  round's, begins a new setup on the same point: the circle was oriented
  anew. The minute: the round-to-round spread in the owner's file is 6" at
  most, and a circle moved between rounds on purpose moves by degrees.
  Rejected: a new setup at every `07`, as the RW5 reader does for a repeated
  backsight - never wrong, but the reduction then places a target from the
  first round and takes the other five as checks, and a traverse adjustment
  sees 246 setups for 41 stations.
- **Whether the distances carry the atmospheric correction is Unknown.**
  Sokkia's field book applies it as each distance is accepted when the job's
  switch is on (the Level 5 manual, appendix B); Trimble's writer turns the
  switch on and writes distances without it. The reduction then applies none
  and says so, and Recompute or Fixed applies one from the recorded weather
  (mmHg and inHg converted by the conventional values of NIST SP 811).
  Rejected: Applied (Sokkia's rule) or NotApplied (Trimble's), either silently
  wrong for files from the other writer.
- **The prism constant is in the distances**, the instrument record's value
  in millimetres: the field book applies it on acceptance, and Trimble's
  writer adds the target's constant to each distance and writes 0 there.
- **Option 45 is the coordinate order** of `02` and `08`: 1 north first, 2
  east first (and Trimble's 3). Nikon's manual calls it the coordinate order
  and Trimble's writer follows it; Sokkia names the fields "Northing" and
  "Easting" but prints East first under E-N-Elev. A file that states
  coordinates under any other option is refused.
- **Coordinates keyed in (KI) are Entered**, an `08 TP` FieldObserved, `08
  AJ`, `TV` and `RS` Calculated, any other Unknown; the first coordinates a
  point is given are kept, the builder's rule, although the field book's is
  that the latest win.
- **Derived views are not imported.** A `09 MC` (oriented, reduced for the
  heights) and an `11` with distances are computed from raw observations;
  beside the raw ones they would count them twice.

The owner's file, read on 2026-09-29 (a local check; the file is not in the
repository): 4 206 records read - every line but 101 blank ones and the 14
deleted - and 14 skipped, each warning naming its deleted record; 41 setups
(two points occupied twice), 6 372 observations: 2 124 pointings, 1 062 on
each face, each a direction, a zenith and a slope distance; 42 points, none
with coordinates - the file gives none - and one coded feature. `SURVEY
IMPORT` therefore draws nothing: its 47 warnings are 41 setups standing on
points with no position, three face pairs outside the default 10"
horizontal tolerance (11.0", 13.5" and 14.8", the file's own), two about the
atmospheric correction, and the 42 named points that are not drawn. The
file declares no coordinate system, so none applies until the person states
one (wizard step 4, or `CRS SET` for the drawing); nothing is transformed.
Given coordinates for its first station in a scratch copy, oriented by the
circle as set, the reduction places all 42 points, within 0.06 mm of an
independent reduction of the same copy by a separate script that reads the
published columns.

Tests: `tests/surveyio/test_sokkia_sdr.cpp`, on the hand-built
`tests/surveyio/data/sdr/traverse.sdr` and records written in the test;
`cli.survey_read_sokkia_sdr` and `cli.survey_import_sokkia_sdr`, whose
positions are worked by hand in `src/katana_app/CMakeLists.txt`;
`McpServer.AnAgentReadsAndImportsASokkiaSdrFile`; and the wizard's content
step, which lists the fixture's two setups. The Survey Controller probe
(`src/katana_surveyio/trimble_dc.cpp`) now steps aside for an SDR header with
any derivation code, not only `NM`: Sokkia allows `ED` as well, and such a
file named `.dc` was claimed at 0.5.

Not done: the transmission checksum is kept, not checked; `09 MC`, `11` with
distances, and road, template, GPS and levelling records are not imported,
so a file written in those views keeps only what it has raw; an observation
from a point other than the current setup's is skipped rather than begun as
a setup; the EDM and reflector offsets of a non-coaxial instrument are warned
about, not applied; notes are kept with their setup, not with the record
before or after them, since writers of the format disagree which; the
wizard's file filter still lists extensions by hand, and has drifted from the
registered ones (it now has `.sdr`, but not `.fld`, `.gts` or `.gts7`).

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
