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
| Opcode field file (`opcode-field-file`): `.fld`, tab-separated records opening with a numeric opcode, total-station and GNSS (RTK) jobs | yes | no | 1.1 | opcodes 02, 03, 04, 05, 06, 07, 09, 16, 20, 29, 41, 42, 43, 44, 71, 72, 73, 99, 100, 124, 125, 128, 129, 138, 139 and -2 read; every other opcode skipped with a warning naming it; an RTK position written as 02 with its receiver's "GNSS Solution" is a GNSS position, any other 02 an entered coordinate; a backsight's stated azimuth kept apart from its circle reading; every measurement of a point keeps its attributes; offsets move the shot they follow; resected setups are not positioned; the coordinate system is declared by name from the header comments, never as a guessed EPSG code |

That is the one reader on main as of 2026-09-24. The instrument parsers this
file describes above (GSI, the Trimble and Topcon exports, LandXML survey
data) are not in `src/katana_surveyio/`, and the import wizard refuses any
other registered format by name; `chooseFormat` in the wizard is where their
dispatch will go when they land.

## The opcode field file (.fld)

Added 2026-09-29, when the owner asked Katana to read a data collector's
"field file" (`260825kj.fld`, a total-station job with utility attributes,
kept on the owner's machine and not committed), and extended the same day,
when the owner supplied two more - an RTK job and a total-station job with
resections - with the publisher's description of the format, and asked that
every one be read completely. Every record is tab-separated and opens with a
numeric operation code: the format its publisher documents as the "Field File
Format", a reference manual chapter that also defines an XML form of the same
records. The reader follows the edition the owner supplied - sections 44.2
"Structure of the .fld File", 44.3 "Point Description", 44.4 "Measurements
and Named Measurements", 44.5 "Searching for Special Coordinates", 44.6's
time_text, 44.7 and 44.8, one entry per opcode; the June 2025 edition numbers
the same sections 1.2 to 1.8 and has the entries and .fld syntax of 128, 129,
138 and 139, but not their descriptions nor 44.7's resection_measurement
block. The reader is `src/katana_surveyio/opcode_field_file.cpp`, on the
Topcon readers' raw builder (`src/katana_surveyio/topcon_raw_builder.hpp`),
which already turned a journal of setups, backsights and shots into one
`survey::SurveyProject` - a third copy of that bookkeeping would have been the
defect section 2 of the contributors' rules names.

What the format says, and the reader relies on: a record is an opcode
"followed by zero or more tabs and pieces of information", so an opcode alone
on its line is one; most records carry a point description of five
tab-separated values (feature code, string number, point ID, point name,
point comment); 02 is a "directly entered coordinate" (X, Y, Z) of which "No
reduction is needed", 03 a setup with its instrument height, 04 the backsight
(horizontal circle, vertical circle, slope distance, and an azimuth that "may
be specified when no coordinate for the backsight point exists"), 06 a check
measurement and 07 a shot (horizontal circle, vertical circle, slope
distance, decimal degrees); 128 is a setup on an unknown point that a
resection computes, 129 the end of its block; 05 sets the target height for
what follows; 09 is a scale factor for later slope distances; 16 makes a
second measurement point at the current one's position, with its own code and
string number; 20 closes the current string (or the one its description
names); 42 and 43 are radial and tangential offsets of the current point (or
a named one), "from the specified points original position" - radial along
the line from the station, positive away from it, tangential at right angles
to it, negative to the left looking from the station - and 44 adjusts its
height; 47 and 48 end the current string; 29 is a memo; 41, 71, 72 and 73 add
text, an integer, a real and a text attribute to the point just measured, a
blank name making an attribute unnamed; 99 ends the file; 100 gives the
units, of which the format allows one each (decimal degrees, metres,
millimetres of pressure, celsius). 124 and 125 (attribute groups) and 140 (a
GNSS coordinate) "do not exist in the fld file". 44.5 finds the point a
setup, a backsight or a check names by its point name among the names, then
the point IDs, of the coordinates and measurements before it, and by its
point ID among their point IDs.

Decisions that are the reader's own, each stated in the source:

- **The column after the opcode.** The owner's files put one field more than
  the description's syntax at the front of every record with values ("07,
  blank, EB, 01, 101 ..."). In two files it is always empty; in the RTK file
  it holds, on every coordinate, the date and time it was measured (the same
  as the point's own Date and Time attributes) - the counterpart of the time
  the XML form carries in its op_code_properties, which the description gives
  the XML form only. A record whose first field is a date or time has the
  column, for only the column holds one; a record whose first field is filled
  otherwise has not, for that is a feature code. A record whose first field
  is blank cannot say - a record in the column's layout that has lost a value
  has the plain layout's count, and a feature code left empty makes the first
  field empty - so the file decides it: the records whose opcode has a fixed
  number of values (02, 03, 04, 06, 07) vote before any is read, a count that
  fits only the column's layout, with the first field blank or a date or
  time, for it, and a count that fits only the description's, with a filled
  first field, against, and most votes win. Two kinds of record fit both
  layouts and vote only with a date or time first: a 04 of ten fields (the
  column and no azimuth, or the azimuth and no column - the description
  writes the azimuth without the brackets it puts round what is optional, so
  its own 04 has ten fields) and a record one field longer than the
  description's whose last field is blank (a trailing tab, or the column and
  a blank last value). Such a record with a blank first field is read in the
  file's layout, unless that leaves its first measured value (the horizontal
  circle, X, the instrument height) blank where the other layout reads one -
  the one sign of which layout wrote it: it is then skipped, saying so. A
  record that does not fit the layout it is read in is skipped with a
  warning, and one blank field past a layout's count is a trailing tab.
  Rejected, as this reader once did: skipping, in a file whose records show
  both layouts, every record that fits both - in a job with the column, one
  shot written without it then cost every backsight, and so every setup's
  orientation; reading such a record in the file's layout whatever it leaves
  blank - a backsight written with the column in a file without it was read
  one value to the right, its slope distance taken for an azimuth that turned
  the setup's shots, without a word; reading every record in the file's
  layout, which skips a record whose own first field says which it has; and
  taking any text in the column, which would read a plain record with one
  stray value shifted by one. A record of free text follows the same rules; in
  a file without the column - or one no fixed record decided, read as the
  description writes it, as its fixed records are - a record of fixed form
  (05, 09, 100, the offsets, 16, 20, 128) whose first value is blank, and which
  fits only without it, carries the column: an empty target height followed by
  a height is no value, and reading it as one had lost the target height, the
  scale factor or, for 100, the whole file. Rejected: reading an undecided
  file's free records as if they had the column, as this reader once did,
  which named an unnamed attribute by its value. A date or time is known by
  its shape: a time is two numbers joined by ":", a date three joined by one
  of "/", "-" or ".", or a month's name standing apart and a number, or the
  basic ISO form (eight digits, T, four or six); and the words may only be a
  date's or a time's (T,
  Z, AM, PM, UTC, GMT, a month, and after the time a zone's two to five
  capitals) - so "10:49:06", "3 Apr 2026 12:49 PM", the description's
  "2015-09-28T06:42:45Z", "20260403T104906Z" and "10:49:06 AEST" are, and "12",
  "1.5", "1-2" and feature codes such as "MAR1" and "T1-2" are not. It is kept
  as text, "time stamp", on the point or setup its record makes, not turned
  into a `SurveyTimestamp`: which of the first two numbers of a date such as
  03/04/26 is the day the file does not say.
- **Opcodes are numbers.** " 2" (the RTK file right-aligns its opcodes) and
  "7" (the resection blocks write 5 and 7 without their zero) are 02 and 07.
  The probe did not trim, which is why the RTK file was "not recognised". A
  record, a comment or the version line may have blanks or tabs before it:
  the probe read a tab-indented file as a field file and the reader then
  refused every record of it.
- **A line that is no record leaves nothing current.** Its first field is not
  an opcode ("0007", "+3", a word), so what it made is not known: the
  attributes, offsets, codes and closes after it that would describe the
  point it made are skipped, naming the line, rather than given to the point
  before. A number read loosely is taken for the opcode it would be, for that
  alone: "+3" leaves no setup, so the shots after it are not filed under the
  setup before. Rejected, as this reader once did: skipping the line alone,
  after which an offset following "0007" moved the shot before it.
- **An RTK position is a GNSS position.** The description gives a GNSS
  coordinate opcode 140 and says 140 does not exist in a .fld, so a .fld
  writes an RTK position as an 02, its "directly entered coordinate", with
  the receiver's attributes after it. An 02 whose own attributes - those
  after it and before the next record that makes a point or a setup -
  include one named "GNSS Solution" (what the receiver solved) is a
  `survey::GnssPositionObservation` of an unpositioned point, in the file's
  grid, with the a-priori GNSS precision of `ReadOptions` (10 mm and 20 mm by
  default); any other 02 is an entered coordinate. The reduction seeds a GNSS
  position before any setup, as it seeds an entered coordinate, so a
  total-station setup on an RTK mark orients on another; the drawn point is
  calculated and the report's method GNSS; and a mark's second position is a
  misclosure check on its first. `FormatContent::gnss` is true. Its Z is the
  mark's: an 02 needs "no reduction", and the antenna height is only an
  attribute (a point measured on a 2.2 m pole is 0.04 m above a neighbour
  0.62 m away measured on 2.0 m, not the 0.2 m more a Z at the antenna would
  give). An 02 with no Z stays an entered coordinate, with a warning: a GNSS
  position has a height. Rejected, each as the reader once read it: every 02
  entered - the reduction then held 3 482 RTK fixes as control ("Method
  control" in the report, "Control: none chosen" in its settings), drew them
  as entered, and a mark measured twice was a reader warning rather than a
  check; and `Calculated` for an 02 whose group's name held "GPS" - the
  reduction does not seed a file's calculated coordinates, so a setup on an
  RTK mark was oriented on its circle reading and the backsight mark in
  `tests/surveyio/data/fld/rtk_setup.fld` drawn 51.8 m from where the file
  puts it. The one attribute named is the evidence, read only in the 02's own
  records: a total-station job's coordinates (none of the other two files has
  it) stay entered, and a solution after a setup record is not the 02's.
  Rejected too: sigmas from the receiver's quality attributes (its quality
  and positional-uncertainty figures), which are its own words for its own
  statistics.
- **Every measurement of a point keeps its attributes.** The description makes
  every measurement a point of its own, with its own attributes; Katana makes
  one point of a name and keeps every observation of it. So the first record
  that makes a point current (02, 04, 06 or 07) owns the plain names, and the
  time stamp, the comment and the attributes after each later record about
  the same point - the other face of a pair, a mark measured again, a check,
  a backsight - are kept under "record N/", N that record: a face pair's
  second Time is "record 32/Time", and a keyed-in mark measured again by RTK
  keeps the solution's attributes under that measurement rather than as its
  own. A 16 makes no second point here (below), so what follows it is still
  the current record's; where a name comes twice after one record - the
  resection job gives a doubly coded utility shot its attribute set once per
  code - a second value that differs is kept as "Name (record M)", with a
  warning, and an identical one adds nothing. Before, each attribute replaced
  the one of the same name: 44 records in the total-station job and 656 in
  the resection job replaced an earlier one, 11 and 34 of them with another
  value, among them two measured pipe-invert depths, without a word. Rejected:
  keeping under its record only a value that differs, which drops the
  record's identical values and still gave a keyed-in mark an RTK
  measurement's attributes as its own; and the last value as the plain one,
  which moves every earlier value when a later record comes.
- **Attribute groups (124 ... 125) keep their group.** The description
  defines them for the XML form only (a name and a level); the RTK file
  writes 124 with an empty value, the name and the level, and 125 with the
  level. An attribute inside a group is kept as "Group/Name", nested groups
  as "Outer/Inner/Name" - the reference station's "Easting" and "Latitude"
  are not the point's, and "/" is how PROP TREE and the Properties panel
  already show a tree. A group still open when a record makes another point
  or a setup, or when a 124 gives the level of a group still open, is ended
  there, with a warning: nested by the records alone, one missing 125 put
  every later attribute of the file under a stale group. A group nested past
  16 named groups, or whose name would take the groups' part of a name past
  512 characters, is read but not named in its attributes' names, with a
  warning - the bounds Katana gives its other "/" tree name, a layer path
  (`kMaximumLayerDepth` and `kMaximumLayerNameLength`, which surveyio may not
  include): 8 000 groups nested under one point made 8 000 names each holding
  every group's, 1.4 GB for a 342 KB file, and in a test of 2 000 the
  longest name was 10 898 characters; it is now 60. A 124 in the files'
  layout without its level keeps its name. A level that is not the nesting
  depth, an end with no group open and a group never ended are warnings.
- **A point is its name, else its ID**, and a setup, a backsight or a check
  finds the point it names as 44.5 searches: by the name, else by the name
  or the ID among the point IDs the coordinates and shots before it gave. A
  03 that names only the ID a coordinate was given with its name stands on
  that coordinate; before, it stood on a new point with no position, and
  nothing was radiated from it. A point with a name and an ID keeps the first
  ID given it in its metadata, and a different one a later record gives under
  that record.
- **X is the easting.** The description says only "(x, y, z)"; the files put
  six-figure eastings in X and seven-figure MGA northings in Y, and the RTK
  file labels a reference station's Easting and Northing the same way.
- **A record that is not read leaves nothing current.** A skipped 02, 04, 06
  or 07, a record about a point this reader does not read (10, 11, 12, 14,
  140) or an opcode the description does not define leaves no current point,
  and a skipped 03 or 128 no setup: the attributes and shots after it are
  skipped naming it. Before, they went to the point or setup before - in the
  resection job some 21 000 attribute records had landed on control marks and
  731 shots under the wrong setup, silently.
- **The backsight.** `SurveyStation::backsightAzimuth` is the circle reading
  on the backsight, as the model defines it: the horizontal circle of the
  setup's first 04 that gives one, as its face-left reading (a face-right
  one less 180 degrees, as the reduction means the pointings to the
  backsight); a later 04 changes it no more than a later pointing would. The
  04's azimuth is `SurveyStation::statedBacksightAzimuth` - the first a
  setup's 04s give; a later one that differs is kept in the setup's metadata,
  with a warning - which the reduction orients the setup on, less the reading
  on the backsight, only where nothing places the backsight: the
  description's "when no coordinate for the backsight point exists". It is
  one field for every reader whose file states an azimuth on its backsight,
  not a second one for this format. Rejected, as this reader once did: the
  04's azimuth in
  `backsightAzimuth`. The report then said the circle had been taken for a
  grid azimuth; a backsight with coordinates and no circle reading was
  oriented on the azimuth taken for the circle (a shot at 90 degrees drawn on
  bearing 45); and a face-right 04 after it replaced the azimuth with its
  circle, turning every shot by 180 degrees.
- **Resections (128 ... 129; 138 ... 139 for Helmert).** A 128 is a setup on
  the point its description names; the description's .fld syntax gives only
  the height, and the file writes the description first (as the XML form
  carries one) and an empty value after the height; with no point named the
  setup is on "resection at record N". A point ID given with the name is
  kept, as a 03's is. The shots after the 129 are still that setup's: no 03
  follows any of the file's twelve. The file does not state where the
  resected points are and the reduction does not compute a resection, so
  these setups and what was measured from them are not positioned:
  `notCarried` says so, and the reduction report names each setup that
  "stands on" a point "which has no position". Rejected: computing the
  resection in the reader. A resection is an estimate from several pointings
  that needs the reduction's grid scale factor, curvature and refraction, and
  its weights and its report - the reader knows none of them - and the same
  step serves every format with a setup on an unknown point (Not done). An
  offset (below) is not that: a fixed correction to one shot, applied in the
  instrument's frame before the reduction applies any of those.
- **Multiple coding (16)** strings the current measurement point into a second
  string. The description makes a second point at the same position; Katana
  strings the one point into both, and a 16 that names a point of its own
  keeps the name in the metadata, with a warning. The second string is then
  the current string, as the description's new point would make it, so a 20
  after the 16 closes it rather than the shot's first string.
- **Close string (20)** marks the feature closed (`SurveyFeature::closed`) and
  takes it off the open list, so a later point of the same code and number
  starts a new string rather than joining a closed polygon.
- **Offsets (42, 43, 44) move the shot they follow.** An offset applies to the
  one shot (07) of its point from the current setup: the shot's circle
  reading, zenith and slope distance become those of the offset position,
  and what was measured is kept in the point's metadata ("shot record N as
  measured") beside each offset ("offset record M" = "tangential -0.2 m from
  setup S, applied to the shot of record N"). With plan distance h = s sin z
  and rise v = s cos z, and radial r, tangential t and height u, the offset
  position is on circle a + atan2(t, h + r), at plan distance hypot(h + r, t),
  rising v + u. The arithmetic is the builder's (`offsetShot` in
  `topcon_raw_builder.hpp`), one home for every reader on it. A radial, a
  tangential and a height offset of one shot combine, each from the measured
  position; two of one kind leave the shot as measured, since whether the
  second adds to the first or replaces it the description does not say.
  Offsets are applied when the setup ends. The description moves the
  measurement an offset follows, a point of its own there; Katana makes one
  point of every measurement of a name, and moving one of several pointings
  the reduction means would put the point between the moved and the unmoved.
  So an offset of a point measured more than once from its setup is kept and
  not applied, with a warning, as is one of a point not shot from the current
  setup (a coordinate, a shot from an earlier setup) and one that would put
  its point within 0.1 mm of the station (`tolerance::kCoordinate`, the same
  ground mark). The reduction applies its grid scale factor k and its
  curvature to the offset position's plan distance rather than to the
  measured one, so the offset is scaled by k with the rest: (k - 1) of it,
  under a millimetre for a metre's offset anywhere in a six-degree transverse
  Mercator zone (k is k0 (1 + (dl cos phi)^2 / 2), from 0.9996 on the central
  meridian to about 1.001 at a zone's edge, dl up to 3 degrees). In the
  resection job this puts five drawn points where the file says, 0.1 to
  0.4 m to the left of their shots, at right angles to the line from their
  setup; the other 91 offsets are of shots from resected setups. Rejected, as
  this reader first had it: keeping them in metadata unapplied, so that the
  raw model holds only what an instrument measured - the reader already
  multiplies every slope distance by the file's scale factor, and a tree
  drawn at the prism when the file puts its centre 0.95 m away is the larger
  wrong. Rejected for now: a typed offset on the pointing and a reduction
  that applies it (Not done), which needs a model change.
- **A check (06) does not string its target.** The description makes a check
  a one-vertex string of its own; the point's "check measurement" names the
  setup and the check's code ("from setup S1, feature code BS, string number
  11"), under the check's record where an earlier record made the point. With
  every shot read, the resection job's coded checks would have made 9 strings
  of their own, one of them joining five control marks.
- **A setup's code strings nothing.** 44.4 strings a measurement point, and a
  03 or 128 makes none: its code and string number are kept in the setup's
  metadata ("setup coding"), and its comment describes the point it stands
  on. A backsight's (04) code is kept in its setup's metadata too.
- **Attributes after a 04 or a 06 describe its target**, as after a shot. The
  description gives 41 and 71 to 73 to the current measurement point, which
  neither makes, so this is the reader's reading: the files write a check's
  date, time and target height after its 06, and a 04's are taken the same
  way (none of the three files writes any).
- **The header comments are metadata, the later ones notes.** Comments before
  the first record that makes a point or a setup are the header: "Key :
  value" is kept as "header: Key" (a key given again with another value as
  "header: Key (record N)", with a warning, where the second replaced the
  first without one), any other line in "header", and "// Coordinate System:
  Australia/GDA2020" with "// Zone: Zone 56" is the declared system
  "Australia/GDA2020, Zone 56", exactly as written - turning that into
  EPSG:7856 would be a guess from free text; the person states the code in
  the wizard (or `CRS SET` sets the drawing's). A comment after it - a
  resection's residuals, an antenna height change - is a note on the setup it
  follows (the project's before the first), verbatim; a rule of dashes is
  dropped. Before, every comment was parsed as "Key : value", so 71 of the
  resection job's 72 residual lines overwrote one another as "header: ID".
  Rejected: "the header is the comments before the first record", because two
  of the files put a 100 record before their header comments.
- **41 keeps its spaces** ("any spaces from column four onwards will be part
  of the text") and is kept as "additional text N": the description appends
  it to the vertex's text, and the point's description is already its
  comment. **29 is a note on the setup** (the project's before the first): the
  description puts a memo in its check-measurement model, which Katana does
  not have, and a setup's notes are where a person reads a job.
- **09 is the scale factor, and a setup's settings state it.** The
  description's entry for 15 (vertical circle correction) gives its .fld
  syntax as "09 Vertical_circle_in_decimal_degrees", 09 being entry 9's own
  opcode; every other entry's record opens with the entry's number, so that
  line is read as a slip for 15 and every 09 as a scale factor (the three
  files' 26 are all 1). The reader multiplies the slope distances after it by
  the factor, so a setup's `InstrumentSettings` give the factor its every
  distance was multiplied by, `Applied` - once the setup ends, so that a 09
  between its 03 and its first shot counts and one before the next setup does
  not - and none, with a warning, where the factor changed within the setup.
  It said NotApplied, which a reduction reading it would apply a second time.
- **99 stops the read**, as the description says; what follows is counted as
  skipped, in one warning. **An opcode the description defines only for its
  XML form** (140 GNSS coordinate, 145 GNSS offset ...) has no .fld layout and
  is skipped, saying so; 124 and 125 are read because a real file shows their
  layout and a misread one can only misname an attribute. Every other opcode
  is skipped with the description's name for it ("opcode 18 (circle feature)
  is not one this reader imports"), and one that changes the records after it
  says so: 15, 50, 127 and 131 ("the measurements after it are read without
  it"), the field-template opcodes 51, 53, 54 and 56 to 59 (the measurements
  after them "keep the codes they are written with, which a field template may
  have changed") and 47 and 48 (the points after them "are strung as if the
  string had not ended").
- **Lines that are no record say why**: an opcode of more than three digits or
  with a plus, text. An attribute record with neither a name nor a value is
  skipped. "{Version 6.0}", which the description does not mention, is the
  file's version when it is the first record, and no record later; a DOS
  end-of-file byte is not a record.
- **The probe** counts a coordinate, setup or shot only when it has the field
  count the description gives it, in either layout: a list of numbered points
  ("   2", tab, easting, northing, height, code) opens with numbers too, and
  was ranked above the delimited-points reader. It reads every line the
  probe's 64 KiB hold, comments passed over, so a long header of comments
  does not hide the job.

The owner's three files, read before and after (a local check; the files are
the owner's and are not in the repository):

| File | Before | After |
|---|---|---|
| RTK job, 134 427 lines, Windows-1252 | not recognised; with FORMAT, refused: no record but the version line could be read (134 393 skipped) | 134 394 read, 0 skipped, 3 warnings (the encoding; two marks given a second GNSS position, 5/32/4 mm and 9/2/11 mm from their first); 3 482 points placed by 3 484 GNSS positions; 209 strings, 22 closed; the system its header names |
| Total-station job, 13 966 lines | 13 950 read, 0 skipped, 0 warnings; 8 setups, 2 628 observations, 5 points, 857 unpositioned, 134 strings | the same counts; imported after `CRS SET EPSG:7856`, the same 862 points at the same positions, with the same 8 reduction warnings (the whole reply the same but the parser's version); the two marks shot six and seven times now keep every shot's Date, Time, Target height and Prism constant |
| Total-station job with resections, 35 207 lines, UTF-8 | 31 019 read, 4 065 skipped (3 406 shots from resected setups, 659 records of six opcodes unread) | 35 084 read, 0 skipped, 8 warnings (a name given again with another value after one shot, on three doubly coded utility points); 18 setups (12 resections), 14 739 observations, 15 points, 4 819 unpositioned, 583 strings; its 96 offsets applied |

Each count was checked against a census of the file written from the
description, not from the reader: every point's X and Y, coordinate source,
strings and closures, and every one of the 155 515 attribute records and time
stamps (109 983 and 3 484 in the RTK job, 12 988 in the total-station job,
29 060 in the resection job), each on its point under the key the rules above
give it. A first census compared the name and value pairs that survived, and
so could not see the 700 records another had overwritten. Imported with
`SURVEY IMPORT`: the RTK job is 3 482 points where the file puts them, drawn
as calculated, each "GNSS" in the report with its a-priori precision, and the
two marks measured twice are misclosure checks (32.4 mm and 9.2 mm in plan),
whether or not `CRS SET EPSG:7856` came first - nothing transforms them, and
the system the header names is not MGA zone 56: the file's own grid
coordinates for its two reference stations agree with their latitudes and
longitudes projected to zone 56 at one (1 cm) and not at the other (+0.10 m
E, +0.45 m N), so the person must say what system the drawing is in. The
total-station job is 862 points, as before. The resection job was 1 481
points, 731 of them radiated from the wrong setup and one setup oriented 12
degrees out on the shots of another; it is now 778 - the 15 control marks
and the 763 shots from the setups with a backsight - with that setup oriented
to 5.8" and five of its shots moved by their offsets, each at right angles to
the line from it by the offset's amount (checked from the drawn
coordinates). Its backsight distance now checks to 19.3 mm, where it was
1 524.4 mm; 13.4 mm of that is the grid scale factor, which `SURVEY IMPORT`
does not apply (its defaults reduce with none) at an easting where the
combined factor is about 0.99979. The twelve resected setups draw nothing
until the reduction computes a resection.

The fixtures are hand-built in the files' layouts, with invented names and
numbers: `tests/surveyio/data/fld/setup.fld` (a setup),
`tests/surveyio/data/fld/gnss.fld` (an RTK job: blank-padded opcodes, time
stamps, each coordinate with its receiver's solution in a group, a keyed-in
coordinate, a close, a mark measured again, a Windows-1252 degree sign),
`tests/surveyio/data/fld/resection.fld` (a resection and its residual
comments in UTF-8, 16, 42, 43, 71, a coded check) and
`tests/surveyio/data/fld/rtk_setup.fld` (a setup on one RTK mark backsighting
another, and two shots with an offset each). `.gitattributes` stores them byte
for byte, so a Linux checkout reads the same CRLF lines. Every file under
`tests/surveyio/data` is detected by
`OpcodeFieldFile.TheProbeClaimsNoOtherFormatsFixtureOrSample`, which prints
the table a probe change is compared by: only the field files are claimed.

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
the reader's `OpcodeFieldFile` cases in
`tests/surveyio/test_opcode_field_file.cpp`, `cli.survey_read_field_file`,
`cli.survey_import_field_file`, `cli.survey_read_gnss_field_file`,
`cli.survey_import_gnss_field_file`, `cli.survey_read_resection_field_file`,
`cli.survey_import_resection_field_file`,
`cli.survey_read_rtk_setup_field_file`,
`cli.survey_import_rtk_setup_field_file`, `qt_survey_verb_headless`, an agent
through MCP (`McpServer.AnAgentImportsAFieldFileWhoseSetupStandsOnRtkMarks`),
the wizard's content step, which shows the fixtures as read
(`SurveyImportWizard.TheContentStepShowsWhatEachFormatsReaderRead`), and the
reduction's cases for a stated backsight azimuth in
`tests/survey/test_reduction.cpp`.

Not done:

- The format's XML form is not read: no sample of it, and its records are
  the same opcodes written as elements.
- Resections are not computed. The reduction has none, so a setup a 128 or
  138 makes, and everything measured from it, is not positioned: 12 setups
  and 4 056 points of the resection job. The file holds what a resection
  needs - each block measures three known marks on both faces, with
  distances - and in review a plain least-squares resection from those
  pointings (directions, and plan distances times the site's combined
  factor), worked outside Katana, placed all twelve with misfits of 4 to
  14 mm, close to the residuals the controller wrote after each block. It is
  a step of the reduction, which would place a setup on an unknown point from
  its pointings to placed ones for every format, with its own weights and
  report - not a reading of this format.
- Offsets are applied by changing the shot they move; a typed offset on the
  pointing, which RW5's off-centre shots and GTS-7's OFFSET records would use
  too, would keep the measured values in the observation. The GTS-7 reader
  keeps its OFFSET records unapplied (`topcon_gts.cpp`: "moving the point is
  the reduction's work"); applying them would go through the same
  `offsetShot`, once the signs its source gives its offsets are checked. An
  offset of a point measured more than once from its setup, of a coordinate,
  or of a shot from another setup, and two of one kind on a shot, are kept and
  not applied.
- A check (06) is reduced as observations, meaned with the other pointings to
  its target, where the description only reports it: a check to the backsight
  moves the setup's orientation (8.3" at one of the resection job's setups),
  and one to a shot point moves the point (in the total-station job). The
  model has no observation that only checks.
- The reduction does not seed a file's calculated coordinates
  (`seedEntered` in `src/katana_survey/reduction.cpp` seeds entered ones), so
  a backsight on one - RW5's GPS-derived records, a JobXML point a COGO
  routine computed - has no position unless another setup radiates it, and
  its setup is oriented on its circle reading. The field file's RTK marks are
  GNSS positions, which it seeds, and are not affected.
- The RTK receiver's quality attributes are not the positions' sigmas: every
  GNSS position takes the a-priori 10 mm and 20 mm, whatever the receiver
  said of it.
- String numbers are compared as text: the description calls a string number
  a positive integer, so "EP 1" and "EP 01" are one string there and two
  here. The builder keys features by the text for every reader on it; none of
  the three files writes one number two ways.
- A backsight (04) written without the column in a file with it, its feature
  code blank and its azimuth given, is read with the column, so its
  horizontal circle is what the plain layout calls its vertical one. Its ten
  fields cannot tell the two layouts apart, and the one sign there is - a
  first value left blank in one reading - is used where it appears.
- Opcodes -3 (group), -1 (error text), 1 (job data), 10, 11 and 12 (stadia,
  HA HD height, HA HD VD), 14, 15, 17-19, 21-24 (joins), 28, 30, 31, 37-40,
  45-62 (string operations, templates, arcs), 68-70 and 74-79 (string and
  segment attributes) and the other opcodes from 80 up that are not read are
  skipped with a warning naming them; 140 in a .fld is skipped.
- `SURVEY READ`'s `warnings=` counts the warnings listed, which the builder
  caps at 10 000 and one "more" line. `SURVEY IMPORT`'s reply lists the first
  20 reduction warnings, so for the resection job the twelve "stands on ...,
  which has no position" lines are among those it only counts, and the line
  it ends with ("reducing the observations is what gives them a position",
  cad's words for every format) does not say that the setups they were
  measured from have no position.
- `SURVEY IMPORT` reduces with no grid scale factor and no height reduction
  unless a person sets them in Survey > Survey Jobs, so a total-station job
  imported into a projected drawing is radiated with ground distances.
- surveyio has seven file-local digit tests (`isDigit` in leica_dbx.cpp,
  leica_gsi.cpp, rinex.cpp, rinex_common.cpp, rinex_compact.cpp,
  topcon_raw_builder.cpp and opcode_field_file.cpp), and
  `include/katana/core/text.hpp`, which would be their one home, has none.
- The verb has no reduction options - a person changes them in Survey > Survey
  Jobs, which re-adjusts the imported job.

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
