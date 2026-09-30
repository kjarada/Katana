# The Survey module and instrument interoperability

This file is the record of *why* the survey module is shaped the way it is.
The short version: a survey that is read slightly wrong
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

> Leica GSI (GSI-8, GSI-16) - import: yes, export: no, parser 1.1

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

Both are called out here because a test that does not aim at them will pass
straight over the bug.

**GSI encodes the decimal position inside the word.** It is not a decimal point
in the data. Read the data block as a plain integer and the coordinate comes out
wrong by orders of magnitude - and still looks like a survey coordinate. Every
expected metre value in the GSI tests is worked out by hand from the
specification with the arithmetic in a comment, never taken from what the
reader returned.

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
| Leica GSI (`leica-gsi`): GSI-8 and GSI-16, mixed or not; any extension when every line is a GSI block | yes | no | 1.1 | total station words only (a digital level's are counted, not read); declares no coordinate system; a sexagesimal angle whose word writes 60 seconds in their place is read as the next minute, and a circle reading past a full circle is refused (see "Leica GSI" below) |
| Opcode field file (`opcode-field-file`): `.fld`, tab-separated records opening with a numeric opcode, total-station and GNSS (RTK) jobs | yes | no | 1.1 | opcodes 02, 03, 04, 05, 06, 07, 09, 16, 20, 29, 41, 42, 43, 44, 71, 72, 73, 99, 100, 124, 125, 128, 129, 138, 139 and -2 read; every other opcode skipped with a warning naming it; an RTK position written as 02 with its receiver's "GNSS Solution" is a GNSS position, any other 02 an entered coordinate; a backsight's stated azimuth kept apart from its circle reading; every measurement of a point keeps its attributes; offsets move the shot they follow; a resected setup's station is computed by the reduction's resection from its 128 ... 129 block, the checks after it checking it (the file states no station); the coordinate system is declared by name from the header comments, never as a guessed EPSG code |
| Sokkia SDR (`sokkia-sdr`): `.sdr`, the SDR33 and SDR2x layouts, a header record `00` naming `SDR33` or `SDR2x` | yes | no | 1.0 | records 00 to 13 read; records marked deleted (`DD`) skipped by name; derived views (09 MC, 11 with distances) and road, template, GPS and levelling records skipped with a warning, a shot with no raw twin said to be lost; units from the header - an undefined angle or distance unit refused, an undefined pressure or temperature unit warned about and not read, a `13DU` followed; coordinates in the header's order (1 N-E-Elev, 2 E-N-Elev; Trimble's 3 east first, with a warning), a point's latest kept; collimation (04) applied per face; declares no coordinate system |

The matrix lists the formats that have a section in this file.
`src/katana_surveyio/` also registers readers for TDS RW5, Topcon GTS-7
raw, Trimble JobXML and RINEX observation files, and recognises Leica DBX,
Trimble DC and Topcon GTS-6 in order to refuse them by name with the export
to make instead. They reach the wizard, `SURVEY READ` and `SURVEY IMPORT`
through the registry like the rest, but have no row or section here yet;
that is not done. LandXML survey data has no reader. The import wizard
reads every format with a reader of its own through that reader
(`SurveyImportWizard::chooseFormat`), as `SURVEY READ` and `SURVEY IMPORT`
do, and refuses a format without one by name.

## Leica GSI

The reader is `src/katana_surveyio/leica_gsi.cpp`; what an import makes of a
file is set out in `include/katana/surveyio/leica.hpp`. The specification
followed is Leica's "GSI ONLINE for Leica TPS and DNA" (November 2003): the
data word and the word information table on its pages 5-6, the word indices
in the PUT and GET tables. Every expected value in
`tests/surveyio/test_leica_gsi.cpp` is worked by hand from a fixture word and
that table.

### Sixty seconds, and the rest of one traverse (2026-09-29, reviewed 2026-09-30)

The owner sent a traverse, `260713LUNCHTRAV-v2.txt` (GSI-16, 1593 blocks
over 52 setups, kept on the owner's machine and not committed), asking that
it be read correctly and completely. It read with 25 warnings, and 24 of them
threw an observation away: an angle word, 21 or 22 in unit 4 (sexagesimal),
whose seconds were exactly 60 with a tenths digit of 0 - `084 01 60.0` and
the like - refused because minutes and seconds run to 59.

GSI ONLINE names the unit ("4: 360° sexagesimal", page 6), and its GET
examples on page 7, `21.104+12149400` and `22.104+08832420`, read as DDD MM
SS s - 121 49 40.0 and 088 32 42.0. It states the layout no further and says
nothing of 60. What decided the reading:

- **60.0 seconds is exactly the next minute.** D MM 60.0 and D (MM+1) 00.0
  are one angle; there is nothing to guess.
- **The file shows how it came to be.** All 3082 of its angle words have a
  tenths digit of 0: its writer wrote whole seconds. Seconds of "00" occur
  24 times and "60" 24 times; the values 01 to 59 occur 51 times each on
  average, from 19 (32) to 90 (26). Together "00" and "60" are about one
  value's share, as the second either side of the minute split between them
  would be when a writer rounds the seconds on their own and does not carry
  the minute. The counts alone would not single them out - 02 occurs 25
  times and 32 19 times - so the next point is what decides. Leica's Format
  Manager builds a sexagesimal angle from separate integer fields, degrees
  "range [0..359]" and minutes and seconds "range [0..59]" (its Reference
  Guide, V1.0, 8.3 and 10), so one working as documented never writes 60;
  what wrote this "-v2" file is not known.
- **Each word's other readings agree only with the carry.** 19 of the 24
  have an opposite face in their own round (the face-right pointing to the
  target next after a face-left one, with no other face-left to it between).
  Read as the next minute they give 2C of -1" to +8" and 2i of +6" to +11",
  where the file's 663 round pairs without a 60-second word run from 0" to
  +5" and from +4" to +14" (5th to 95th percentile); read as 00 seconds of
  the same minute they are 49" to 62" out. The other 5 (records 73, 576,
  879, 1457 and 1466) are extra face-left pointings with no face right in
  their round: read as the next minute each is within 5" of the same face's
  other readings of its target, and 56" to 65" out read as 00 seconds. (The
  file has 857 face-left shots and 684 face-right.)

So a sexagesimal word whose seconds are 60 with nothing after them is read as
the next minute, carried in whole numbers before the angle becomes a real,
and the file gets ONE warning: the count, every record (up to 100), and the
first word as written and as read - "at records 14, 46, ...; the first, word
21, 354 02 60.0, read as 354 03 00.0". It says the reading is "within the
writer's rounding of what was measured", not a figure: this writer rounded
to whole seconds, so its 60 stood for anything from 59.5, and another may
round to tenths. The whole-number carry makes the angle the same double as
the word written with its carry; adding 60/3600 of a degree as a real does
not (1.8e-15 rad apart at 267 46 60.0), and
`ACarriedAngleIsExactlyTheAngleWrittenWithItsCarryInEitherWidth` fails when
it is done that way.

The carry is made only where the word writes both digits of the seconds in
their place: at its block's full width (8 or 16 characters), or with four
digits or more after a written point. A word of another width is aligned from
the right, and a written point's missing digits are supplied as zeros; either
can make a 60 of digits that were never the seconds. `000000000841600`,
084 16 00.0 with its last digit lost, reads from the right as 008 41 60.0,
and `084.016` is 60 only with the 0 the reader supplies. 1.0 refused both at
their record; carrying them, as the first version of 1.1 did, turned a
damaged word into an angle - 75 degrees wrong in the first example - with
nothing at its record to say so. They are refused there again, with the reason
(`SixtySecondsTheWordDoesNotWriteInTheirPlaceAreRefusedNotCarried`). The note
counts only the words whose values became observations: one in a code block,
whose measurement words are not read, or in a shot at the occupied point is
not said to be read as the next minute
(`ASixtySecondWordWhoseValueIsNotKeptIsNotSaidToBeReadAsTheNextMinute`).

Rejected:

- refusing the word, as 1.0 did: it discards an observation whose value is
  not in doubt, and a lost zenith angle takes its pointing's slope distance
  with it - the reduction rejected 12 of the traverse's distances for want of
  one;
- reading it as 00 seconds of the same minute: a minute wrong;
- carrying anything else past 59: seconds past 60.0 (60.1, 61) and minutes of
  60, which would need a second carry to have been dropped as well. No file
  has shown either, so each is still refused at its record ("the seconds run
  to 59", "the minutes run to 59"), where the evidence can be weighed if one
  appears. That no writer makes them is not claimed: GSI ONLINE's SET 55,
  "Angle rounding [0..10] e.g. n=3: 0.3, 0.6, 0.9" (TPS110C/300/400/700,
  page 19), rounds the last digit in steps, and what such a rounding writes
  at the top of a minute is not stated.

**A circle reading runs to one full circle.** Checking what else the
rewritten check let through found that a word past a full circle was wrapped
onto it without a word: a zenith of 365 00 00 read as 5 degrees, a direction
of 450 00 00 as 90, and 9999 59 60 as 280. The unit digit names the circle
(page 6: "400 gon", "360° decimal", "360° sexagesimal", "6400 mil"), so such
a word is damaged, and it is now refused at its record in every unit. A full
circle itself - 400.00000 gon, 360 00 00.0, or 359 59 60.0 carried - is read
as the zero, which it is exactly, as a rounding up at the top of the circle
writes it
(`ACircleReadingPastAFullCircleIsRefusedInEveryUnitAndAFullCircleIsZero`).
Rejected: refusing 360 degrees itself, as the Format Manager's [0..359] might
suggest; that is the range of its display field, and 360 00 00.0 is a true
reading of the zero. A letter in any field of a sexagesimal word is refused
before the seconds are looked at, so a letter beside a 60 is never carried
(`ALetterInAnyFieldOfASexagesimalWordRefusesItAtItsRecord`).

The other warnings and notes on the traverse, each checked against the data:

- **Word 71 of nothing but zeros was the code "0".** Word 71 is
  `0000000000000000` on all 1541 shots. GSI right-justifies text and pads it
  with '0' (GET 11 and 41), so that is an empty value - the Format Manager
  writes an unset code information word "43....+00000000" (its Annex 2) - but
  the reader took it as the code "0", gave every target that code and ran one
  feature through all of them in shot order. Words 42-49 and 71-79 of nothing
  but zeros are now empty, and the import says how many it read so; a code
  "0" would be written the same way and cannot be told from one. Words 11 and
  41 keep "0": a point needs an id, and a code block exists to give a code
  (`AnAllZeroPointIdOrCodeBlockCodeIsZeroNotEmpty`). Looking at which blocks
  read those words found two that none did, each dropped without a word:
  code information (42-49) in a point's own block, and remark words (71-79)
  in a code block. GSI ONLINE's PUT 41-49 give code information with a code
  block, but "one may create any kind of GSI formats" (the Format Manager's
  Reference Guide, Annex 2), and in a point's block it can be about nothing
  but that point, so it is now in the point's metadata as "code information
  N". A remark word in a code block could be the point's code and remarks or
  more about the block's own code, and nothing tells which, so it is not
  read, and the import now says so once
  (`CodeInformationInAPointsOwnBlockIsKeptAndRemarksInACodeBlockAreSaidNotRead`).
  The traverse has neither.
- **51 of 52 setups had a backsight.** The traverse begins on SS27898 with
  SS27899 as its reference object, and SS27899 is keyed in only when the
  instrument stands on it, 166 records later. A setup's first shot was its
  backsight only if the point had coordinates EARLIER in the file, so the
  reduction left SS27898 unoriented. It has the whole file - it already waits
  for a backsight positioned later - so the rule is now that the file gives
  the point coordinates, before the setup or after it, other than by the
  setup's own shots: target coordinates (81-83) the instrument computed with
  the orientation the backsight is to give would orient the setup on itself
  (`AFirstShotToAPointOnlyItsOwnSetupPositionsNamesNoBacksight`). The note
  says how many backsights came from coordinates later in the file.
- **One distance was measured with another prism constant.** Record 1526's
  word 51 is +23 mm where every other shot's is 0, and its slope distance,
  78.526 m, is 24 mm longer than the setup's 10 others to that mark at
  78.502 m and 23 mm longer than its 4 at 78.503 m. The distance keeps the
  constant it was measured with (`TargetInfo`), as before; the import now
  says how many distances were measured with a constant other than their
  setup's and names the first, as it already said of a changed ppm. A changed
  prism or a wrong setting is for the person to decide.
- **Right as they were:** 1489 blocks record a time the model has no place
  for (it keeps one per setup, and an observation has no time); GSI states no
  coordinate system; word 22 was read as a zenith angle - over the 1474 shots
  to keyed-in stations, HI + SD cos z - HR closes on their height difference
  to a median of -1.8 mm and SD sin z on their distance to +0.4 mm - and word
  21 as clockwise, which all 51 setups with two positioned targets fit
  (median misfit 3.4"; read counterclockwise, 37 degrees).

Read again with parser 1.1: 1593 records, none skipped; 4623 observations
(1541 shots of three), 24 more than before; 52 points and 5 unpositioned; no
feature; 4 warnings, each said once for the file (the 60-second words, with
all 24 records, the zero code words, the prism constant, the times).
`SURVEY IMPORT` on a drawing on EPSG:7856 draws the same 57 points, rejects
no distance where it rejected 12, and orients every setup. With SS27898
oriented, its backsight check to SS27899 is +39.0 mm in distance and +34.1
mm in height over 321 m, and the setup on SS27899 checks its own backsight
at +21.7 mm, where every other setup's distance check is within 4 mm. Both
involve SS27899, whose keyed-in coordinates the reader reads as written: a
question for the job's control, not for the reading. The review round's
changes (the width rule, the circle, the kept-word count, the code words in
other blocks) read this file exactly as before but for the wording of the
60-second note: none of its 24 words is short, long or pointed.

Tests (`tests/surveyio/test_leica_gsi.cpp`, on the hand-built
`tests/surveyio/data/leica/rounded_seconds_gsi16.gsi` and inline blocks):
`SixtySecondsWithNothingAfterThemAreReadAsTheNextMinute` (084 01 60.0 is
1.466658348092568288 rad, worked in exact rationals),
`TheImportSaysOnceHowManyAngleWordsWroteSixtySecondsAndWhereTheyAre`,
`SecondsPastSixtyAreNoRoundingAndTheirWordIsStillRefusedByRecord`,
`SixtySecondsWithTenthsOrMinutesOfSixtyAreNoRoundingAndAreRefused`,
`SixtySecondsTheWordDoesNotWriteInTheirPlaceAreRefusedNotCarried`,
`ASixtySecondWordWhoseValueIsNotKeptIsNotSaidToBeReadAsTheNextMinute`,
`ACircleReadingPastAFullCircleIsRefusedInEveryUnitAndAFullCircleIsZero`,
`ALetterInAnyFieldOfASexagesimalWordRefusesItAtItsRecord`,
`ACarriedAngleIsExactlyTheAngleWrittenWithItsCarryInEitherWidth`,
`AnAllZeroRemarkOrCodeInformationWordIsEmptyNotTheCodeZero`,
`AnAllZeroPointIdOrCodeBlockCodeIsZeroNotEmpty`,
`CodeInformationInAPointsOwnBlockIsKeptAndRemarksInACodeBlockAreSaidNotRead`,
`AFirstShotToAPointTheFileGivesCoordinatesOnlyLaterIsStillTheBacksight`,
`AFirstShotToAPointOnlyItsOwnSetupPositionsNamesNoBacksight` and
`DistancesMeasuredWithAnotherPrismConstantThanTheirSetupsAreCountedAndKeepTheirOwn`;
through `katana_cli`, `cli.survey_read_gsi_reads_sixty_seconds_as_the_next_minute`
and `cli.survey_import_gsi_radiates_the_shots_whose_angles_wrote_sixty_seconds`,
whose two radiated points are worked by hand to the millimetre in
`src/katana_app/CMakeLists.txt`. The window's Survey > Import Survey Data
reads a GSI file through the same reader, and `katana_mcp` runs the same
verbs.

Not done:

- **The reduction's face pairing.** It pairs a setup's i-th face-left
  pointing to a target with its i-th face-right one
  (`src/katana_survey/reduction.cpp`). Each setup of the traverse has 3 to 9
  more face-left pointings than face-right ones (3 in 41 of the 52), and in
  the setups looked at they come first, so its pairs cross rounds -
  "SS27898, 5140: 15.0"" pairs record 10 with record 15, where record 10's
  own round gives -3" and record 15's -1" - and some of the file's face-pair
  warnings come from that, not from the data. Reading the 60-second words
  moves it both ways: record 1457, whose zenith was refused before, now has
  a face and pairs with record 1476 of another round, adding one warning
  (setup 5181 to 5180, horizontal 19.0"), while setup 5170's pairs to 5171
  go from three (pointings 4/7, 14/11, 18/15) to six (4/7, 6/11, 10/15,
  14/19, 18/23, 22/27), all within tolerance. It belongs to the reduction
  and every format that feeds it, not to this reader.
- **The backsight check has no tolerance.** `src/katana_survey/reduction.cpp`
  stores each setup's backsight distance and height differences, and the
  report prints them as bare numbers, where face pairs and misclosures are
  marked OUTSIDE TOLERANCE. So SS27898's +39.0 mm, left unoriented with a
  warning before and oriented now, is flagged nowhere. A tolerance belongs in
  the reduction's settings beside the face-pair ones, with a source for its
  value; it would apply to every format, so it is not added here.
- **Three routines turn degrees, minutes and seconds into radians:** this
  reader's, `packedDegreesToRadians` in
  `src/katana_surveyio/topcon_raw_builder.cpp` (GTS-7 and RW5), and
  `dmsToRadians` in `src/katana_survey/angles.cpp`. Only this one reads 60
  seconds as the next minute; the other two refuse it. Carrying it there
  needs evidence from those formats' writers, not this file's. They should
  become one helper with the carry as a choice its caller makes.

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
  resected points are, so the reader gives none, and the reduction computes
  each resection from the setup's pointings to placed points before the 129
  - the reader marks where its block ends (`kResectionEndMetadata`), so a
  check (06) after it checks the station, as it did in the field ("The
  reduction's resection", below) - then radiates the rest from it. Rejected:
  computing the resection in the reader. A resection is an estimate from
  several pointings that needs the reduction's grid scale factor, curvature
  and refraction, and its weights and its report - the reader knows none of
  them - and the same step serves every format with a setup on an unknown
  point. An
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
combined factor is about 0.99979. The twelve resected setups drew nothing
until the reduction computed a resection (2026-09-30, "The reduction's
resection" below); the job is now all 4 834 of its points.

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
- A Helmert resection (138) is computed as the least squares a 128's is.
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
  20 reduction warnings: for the resection job the face-pair ones, and not
  the residuals its resections flag; those each have a `resection_residual`
  record after their `resection` record, whatever the cap.
- `SURVEY IMPORT` reduces with no grid scale factor and no height reduction
  unless a person sets them in Survey > Survey Jobs, so a total-station job
  imported into a projected drawing is radiated with ground distances.
- surveyio has seven file-local digit tests (`isDigit` in leica_dbx.cpp,
  leica_gsi.cpp, rinex.cpp, rinex_common.cpp, rinex_compact.cpp,
  topcon_raw_builder.cpp and opcode_field_file.cpp), and
  `include/katana/core/text.hpp`, which would be their one home, has none.
- The verb has no reduction options - a person changes them in Survey > Survey
  Jobs, which re-adjusts the imported job.

## The Sokkia SDR file (.sdr)

Added 2026-09-29, when the owner asked that every example survey file be
read. The SDR file among them - a traverse of 41 setups in the SDR33 layout,
written by another program's converter, kept on the owner's machine and not
committed - was not recognised at all: `SURVEY READ` answered "no registered
format recognises this file". The reader is
`src/katana_surveyio/sokkia_sdr.cpp`, on the raw builder the field file
uses (`src/katana_surveyio/topcon_raw_builder.hpp`).

It is read from Sokkia's "Interfacing with the SOKKIA SDR Electronic Field
Book" (software 04-04.xx, October 1999): the example jobs of chapter 2, the
record layouts of section 3.6.2 (SDR33: point names in 16 columns, reals of
16) and 3.6.1 (SDR2x: 4-digit point numbers, reals of 10), the field types
of 3.1 to 3.3, the derivation codes of 3.4, the options of 3.5 and the
sample files of chapter 4. What a record means comes from Sokkia's SDR
Software Reference Manual (SETX), sections 3.5.6, 5, 8.2, 28 and 29, its SDR
Level 5 manual, appendices A and B, and its ProLINK manual, 6.1.3.2 (how its
office program finds a point's coordinates); what other programs write into
the format, from Trimble's published "SDR33 Observations.xsl", the SDR
columns in Nikon's DTM-322 manual and Spectra Precision's Focus 6 guide, and
the LISCAD help page on Sokkia field codes (how a set collection is
bracketed). The SDR2x layout is the SDR33 one with a narrower point id and
real, less some fields (its JOB has no option flags, its SET no set number
or marks), so one reader reads both, choosing by the header's version.

Read: the header's units (degrees, gons or mils; metres, feet or US survey
feet, `13DU` included; mmHg, inHg or mbar; Celsius or Fahrenheit) and its
coordinate order; the job's name and correction switches; the instrument
(model, serial number, prism constant, zenith or horizon vertical
readings); weather and scale factor for the setup they are written before;
setups (02) with their instrument heights; target heights (03); collimation
(04, applied to the readings after it); backsights (07); coordinates (08,
and a 02's); raw face 1, face 2 and multiple-distance observations (09 F1,
F2, MD) as pointings of a direction, a zenith and a slope distance; an
azimuth keyed with no distance (11); sets (12); notes (13), a time stamp
also dating its setup - a time during the setup, since a writer may stamp
any round. Every other record is skipped with a warning naming it and its
type.

**The backsight's two numbers, and the model.** A `07` gives the azimuth to
the backsight and the circle reading on it. `SurveyStation::backsightAzimuth`
was the only field, and it is the CIRCLE (the model's comment says so, and
the RW5, GTS-7, JobXML and field-file readers store the circle there); the
first version of this reader stored the azimuth in it instead, which gave
SDR files the right bearings where the backsight had no coordinates but gave
the field a second meaning, turned the reduction's "the circle, taken as a
grid azimuth" warning false, and still lost the orientation correction when
the backsight was placed but never observed. The model now has
`SurveyStation::statedBacksightAzimuth`, the azimuth the file states, beside
the circle; the reader fills both. The reduction orients a setup on the
backsight's coordinates when it has them, as before (and with no reading on
the backsight, less the circle); where nothing places the backsight it takes
the stated azimuth - waiting, as it does for the circle, in case a later
setup places the backsight - less the setup's own mean reading on the
backsight, or with none less the circle. That is SETX 29.2.6's A = H + BKB
azimuth - BKB h.obs with the setup's readings standing in for the h.obs where
there are any: for one backsight the two differ by the pointing error of its
rounds, which the mean of all of them shares out (and Trimble's writer puts
the face 1 reading in h.obs while its azimuth comes from the face mean, so
the mean is the better of the two there); for a back-bearing record SETX
8.2.2 averages over several backsights, whose h.obs is computed rather than
read, they differ by that averaging - 1.8" in a reviewer's case - since the
reduction orients on the one backsight the record names. Only failing both
does the circle stand in for the grid azimuth. The reduced project keeps both
numbers. Rejected: keeping one field and storing the circle there, which
rotates every bearing of an SDR setup whose circle was zeroed on a backsight
with no coordinates (Sokkia's chapter 4 sample: azimuth 14, circle 0); and
renaming the field, which touches every reader for no change in meaning.

Decisions that are the reader's own, each stated in the source:

- **`DD` marks a deleted record.** No published document describes the
  prefix. After it (or after `DDDD`) every such line in the owner's file is a
  complete record, and a record type is two digits, so a D cannot open a
  live one: every leading D is part of the mark, and a warning quotes it as
  written (a run longer than eight by its length, so a line of two million
  D's is not a two-megabyte reply). The two blocks that carry it are setups
  begun and abandoned, each redone under the next live `02`. A deleted
  record is skipped, named in a warning, and changes nothing after it: a
  deleted `03` sets no target height, a deleted `02` begins no setup.
  Rejected: reading it as live, which restarts a setup the surveyor
  abandoned, and passing over it without a word, which the skipped count
  would then not show.
- **One setup per `02`, its rounds together.** The owner's file observes each
  setup in six rounds, each opened by a `07` to the same backsight at the same
  azimuth; they are one setup, and the reduction means them face pair by face
  pair. A `07` that names another backsight or azimuth, or whose circle
  reading on the backsight is more than a minute of arc from the first
  round's, begins a new setup on the same point: the circle was oriented
  anew (SETX 8.2: a back-bearing record orients what follows until the
  next). The minute: the round-to-round spread in the owner's file is 6" at
  most, and a circle moved between rounds on purpose moves by degrees.
  Merging is exact when every round observes the same targets, as every
  setup of the owner's file does; a target a round missed is oriented on the
  mean of all the rounds, off by at most half their spread. Rejected: a new
  setup at every `07`, as the RW5 reader does for a repeated backsight -
  never wrong, but the reduction then places a target from the first round
  and takes the other five as checks, and a traverse adjustment sees 246
  setups for 41 stations.
- **Set Collection's `07 SC` closes the set before it; any other `07` orients
  what follows.** Sokkia's Set Collection writes each set's back-bearing
  after the set, its circle the mean of the set's readings on the backsight
  (chapter 2's V04-01 example: face 1 0-00'00", face 2 180-00'30", BKB
  0-00'15"). So a `07 SC` read after a set's observations, with no `07`
  between, orients them: when it begins a new setup (the circle was moved
  between sets), the set's observations go with it, their pointings
  numbered anew. Rejected: splitting at the `07`, which left each moved set
  in the setup before it and an empty setup after; and letting any `07`
  close a set, the reader's first rule, which took a plain `07` opening the
  next round (after `12` and its shots) for the set's close and turned the
  set's shots by the circle's move.
- **Where a set's `12` stands depends on the layout.** Chapter 2: "The
  standard SDR33 format [writes] a SET record before the raw observations";
  from V04-02 on, a job with 4-digit point numbers - the SDR2x layout -
  writes it after them, before the MC records ("the SET record precedes the
  MC records for SDR2x format compatibility"), and LISCAD reads that order
  (a set begun at the field book's "13SC Set #" note, completed by its
  `12SC`). So in the SDR2x layout a `12` read after raw observations of its
  setup closes the set they make - those since the setup began, its last
  `07` or `12`, or a "Set #" note, the last of them as many as the `12`'s
  count where it states one - and a `12` with none before it opens its set,
  as V04-01 wrote it. The `07 SC` after the MC records then orients that set.
  Rejected: taking every `12` as opening its set, the reader's first rule,
  which read chapter 2's garbled sentence as "an SDR2x set collection from
  V04-02 on stores only MC records" - its V04-03 paragraph says the SET
  record precedes the MC records - and, where the circle moved between sets,
  left the second set on the first's orientation, its directions off by
  half the move with only a misleading warning.
- **A setup's first `07` orients shots before it only when one of them is of
  its backsight.** That is Trimble's writer's order: it writes the field
  book's records in order, and a back-bearing record holds the face 1 circle
  reading of the backsight's shot, so it follows that shot. Otherwise the
  `07` orients what follows it (SETX 8.2): the shots before it were taken on
  no orientation it gives, so they stay a setup of their own - oriented by a
  keyed azimuth, or read as azimuths (next) - and the `07` begins a new setup
  on the point, with a warning. The same holds for the shots before a set
  that a first `07 SC` closes. Rejected: orienting the whole setup, the
  reader's first rule, which turned such shots by the `07`'s circle without a
  word and let a later `07` override a keyed azimuth for the shots it had
  oriented; and splitting at every first `07` after shots, which would cut
  Trimble's backsight shot from the record made from it.
- **A setup with no `07` is oriented by a keyed azimuth, or reads azimuths.**
  An `11` with an azimuth and no distance from the setup's point is a keyed
  orientation, and a setup with no back-bearing record is oriented by the
  first such to a point it observes: chapter 2's OBS-view example has only
  that, and its printed positions (the same job in POS view) follow A = H +
  the keyed azimuth - the reading on that point, to the printed 0.001 ft
  (`SokkiaSdr.ASetupWithNoBacksightRecordIsOrientedByItsKeyedAzimuth`
  reduces it). A setup with neither takes its readings as azimuths, as the
  field book does when the backsight is skipped (SETX 8.2.1); in the model
  that is a circle with no backsight named, which the reduction takes as set
  to azimuths and says so. Rejected: leaving such setups unoriented, which
  drops every shot of Sokkia's own example.
- **A bad set is as long as its count.** A `12` whose bad marker is 2 is
  not used, and its raw observations are skipped - as many as the record's
  "count of observations" (chapter 2's example counts 4 for its four `09`
  records), or, where it states none, up to the next set or setup. Rejected:
  always up to the next set or setup, which took the shots after a bad set
  for part of it.
- **The collimation is applied, per face.** A `04`'s corrections apply to
  every raw reading after it, until the next `04`, as SETX 29.2.5 applies
  them: face 1 plus the vertical and horizontal correction, face 2 minus, a
  reading with no vertical angle taken as face 1 (SETX 29.2.3). A face
  pair's mean is unchanged, so a round on both faces reduces as before and
  its faces now agree; a shot on one face moves (17.45 mm at 100 m for a 36"
  correction). The field book corrects the zenith after the instrument and
  target heights, the reader before them, which differs by at most the
  correction times the height difference over the distance (0.01" for 10"
  and 0.1 m at 100 m). The setup's metadata names the record applied.
  Rejected: keeping it in metadata only, the reader's first rule, whose
  reason - a face pair cancels it - holds only where every target is paired,
  not for the common single-face side shot; and a collimation field in the
  model for the reduction to apply, which no other reader has a value for.
- **Whether the distances carry the atmospheric correction is Unknown.**
  Sokkia's field book applies it as each distance is accepted when the job's
  switch is on (the Level 5 manual, appendix B); Trimble's writer turns the
  switch on whenever it writes weather and writes distances without it. The
  reduction then applies none and says so, and Recompute or Fixed applies
  one from the recorded weather (mmHg and inHg converted by the conventional
  values of NIST SP 811). Where the file's time stamps are in Trimble's
  writer's form ("Time Date MM/DD/YYYY"), what the file did not carry says so
  and that Recompute then applies the correction; the owner's file is such a
  file, and its notes and job flags are that writer's too. Rejected: Applied
  (Sokkia's rule) or NotApplied (Trimble's), either silently wrong for files
  from the other writer - the time stamps point to a writer, they do not
  prove it. What it is worth on the owner's file: from its weather (981.4 to
  997.7 hPa, 9.9 to 17.0 C, 60 % humidity assumed) the IUGG 1999 group
  refractivity at the reduction's 658 nm gives 4.3 to 12.6 ppm across its 41
  setups, worked independently of Katana's code. Reduced with its first
  station held at an assumed position (N 6 250 000, E 300 000), the far end
  of the traverse, 18236, moves 16.4 mm between the default and Recompute
  (161512 16.1 mm; measured with this round's build) - so a person whose
  distances are raw must choose Recompute.
- **The prism constant is in the distances**, the instrument record's value
  in millimetres: the field book applies it on acceptance, and Trimble's
  writer adds the target's constant to each distance and writes 0 there.
  Trimble's writer puts the instrument's serial number in the EDM
  description field; it is kept as that field's text (`instrument: EDM`).
- **Option 45 is the coordinate order the format defines.** Sokkia's 3.5
  gives 1 "N-E-Elev" and 2 "E-N-Elev"; 3.6.2 names 21-36 the northing and
  37-52 the easting, which is option 1's order. Under 2 the easting comes
  first: Sokkia's own E-N-Elev example (chapter 2, V04-04.30) stores in the
  displayed order - its HORZADJ translation printed as "Trans.N" is the
  easting's, since only that reading takes its GSTN 1005 to its printed POS
  1005, to 0.1 mm where the other is 5.5 cm off - and Trimble's writer puts
  the easting first under 2. Trimble's 3, "Y-X-Z", is east first too and read
  so, with a warning: Sokkia does not define 3, and SETX 3.5.6 lists
  south-west-elevation as the field book's third display order. The order
  read is in the project's metadata (`header: coordinate order`). A file
  that states coordinates under another option is refused. Rejected: a
  warning under option 2 that a Sokkia field book may write the northing
  first, which the reader gave after its first review - Sokkia's own example
  shows it does not; north first (every Trimble-written file's control
  transposed); telling north from east by magnitude (a guess).
- **Angle option 4 is mils in the Nikon form and degrees otherwise.** Sokkia
  defines 1 to 3. Nikon's and Spectra's manuals give 4 as mils, in headers
  written with no blank before the version ("SDR33V04-01"); Trimble's writer
  gives 4 as quadrant bearings, which SETX 3.5.6 says are degrees
  underneath. The header's form tells the writers apart, and either reading
  is warned about, naming the other. Degrees is also the reading whose
  mistake shows: mils read as degrees put every zenith past a full circle,
  which is warned about record by record. A header in the Nikon form also
  takes the refraction constant option 1 as Nikon's 0.132, not Sokkia's
  0.14.
- **A point's latest coordinates are its own.** Coordinates keyed in (KI)
  are Entered, an `08 TP` FieldObserved, `08 AJ`, `TV` and `RS` Calculated,
  any other Unknown; a point given coordinates more than once keeps the
  latest, the field book's own rule (Sokkia's 2.3: "the latest coordinates
  are the best"; ProLINK 6.1.3.2 searches from the end of the field book
  for a point's POS, STN or POS-view record). A traverse adjustment's `08 AJ`
  follows the positions it adjusts, and a later `02` restates the station as
  the field book held it then. The earlier coordinates stay in the point's
  metadata, each change warned about; the same coordinates again say
  nothing. The shared builder gained the choice
  (`RawProjectBuilder::RestatedCoordinates`); the RW5, GTS-7 and field-file
  readers keep the first, their formats' later positions being checks.
  Rejected: keeping the first, the reader's first rule, which imported
  chapter 2's traverse with its stations unadjusted beside side shots
  computed from the adjusted ones.
- **A height with no position is kept.** An `08` or `02` that gives an
  elevation and no northing or easting cannot place its point; the height
  goes to the point's metadata (`height without a position`, as the GSI
  reader keeps one) with a warning. With the job's "Record elev" No an
  elevation is a placeholder (SETX 4) and is not kept.
- **Derived views are not imported, and a shot with no raw twin is said to
  be lost.** A `09 MC` is the field software's reduction of raw observations
  - oriented, and reduced for the heights, the prism constant, the weather
  and curvature and refraction - and an `11` with distances further for
  scale and sea level (SETX 5 and 29.1). Beside a raw observation of the
  same line in its setup they would count it twice. Without one - a field
  book set to send the MC or RED view (2.1), an MC of a target its setup
  never observed raw, a RED from another point, or a Nikon or Spectra
  instrument, whose only observation record is `09MC` - the writers
  disagree about what the numbers are: Sokkia's MC is a mark-to-mark vector
  already oriented and corrected, Trimble's the inverse of computed grid
  coordinates, Nikon's "slope distance, vertical angle, horizontal angle"
  with target heights in `03` records beside them. So they are skipped, each
  with the reason, and a shot with no raw twin is said to be lost, in its
  warning and in what the file did not carry. Rejected: importing them on
  one writer's meaning, which would put another writer's points in the
  wrong place without a word; and deciding by whether the setup held any
  raw observation, the rule after the first review, which called an MC of
  an unobserved target "counted twice" and left its shot out of the count
  of lost ones.
- **A point id holding a control byte is line noise**, not a name: its
  record is skipped, naming the byte (`\x01`). Rejected: blanking it, which
  made two different corrupt ids one point, placed at neither.
- **One stray line does not cost the file.** A second header record with no
  version, or too short to have one, or of a version the reader does not
  read, and a `13DU` with no code or one the format does not define, are
  skipped with a warning - the first header's units hold. A second header
  that states other units is still refused: every number after it would
  mean something else. A `13DU` that disagrees with the header is followed,
  with a warning: Sokkia's 3.3.2 has it govern "all distances specified
  after this Note record" (it was refused at first). Trailing white space
  after the header's options (a tab included) is padding; an STX glued to
  the first record is framing; a line of DOS end-of-file marks (Ctrl-Z) is
  passed over, by the reader and by detection alike; any other control byte
  a warning quotes is written `\xNN`.
- **A last record with no line end is said to be perhaps cut short.**
  Sokkia's chapter 3 ends every record with CR LF. A transfer cut inside
  the last record leaves a real that is still a number (88.7 of
  88.7450058), so the record is read and the read says the file may have
  been cut there.

The owner's file, read on 2026-09-30 (a local check; the file is not in the
repository): 4 206 records read - every line but 101 blank ones and the 14
deleted - and 14 skipped, each warning naming its deleted record; 41 setups
(two points occupied twice), 6 372 observations: 2 124 pointings, 1 062 on
each face, each a direction, a zenith and a slope distance; 42 points, none
with coordinates - the file gives none, so its coordinate order (option 2,
in the metadata) is never used - and one coded feature. After this round's
changes a full dump of the read project is byte for byte the one before,
but for the atmospheric note (which now says the file's time stamps are
Trimble's writer's) and the coordinate order's metadata: the file has no
`04`, no `12`, no MC or RED record, no restated coordinates and no shot
before a setup's first `07`. `SURVEY IMPORT` therefore draws nothing: its
47 warnings are 41 setups standing on points with no position, three face
pairs outside the default 10" horizontal tolerance (11.0", 13.5" and 14.8",
the file's own), two about the atmospheric correction, and the 42 named
points that are not drawn. The file declares no coordinate system, so none
applies until the person states one (wizard step 4, or `CRS SET` for the
drawing); nothing is transformed. Given coordinates for its first station
in a scratch copy, the first setup oriented by its `07`'s azimuth less its
reading on the backsight, the reduction places all 42 points, within 0.057
mm of an independent reduction of the same copy by a separate script that
reads the published columns. The owner's four other files read and import
exactly as before, and so do the other formats' fixtures.

Tests: `tests/surveyio/test_sokkia_sdr.cpp`, on the hand-built
`tests/surveyio/data/sdr/traverse.sdr` (its second setup's circle zeroed on
a backsight whose azimuth is 270) and records written in the test - among
them Sokkia's own chapter 2 examples: the OBS-view job reduced to its
printed positions, the traverse's `POS AJ` records kept over the positions
before them, and its `BKB TP 0003-0002` (azimuth 269-59'50", circle
270-00'00") orienting a shot less the circle, and less a reading 5" off it;
the SDR2x set layout with the circle moved between sets; each order of shots
and first backsight record; the collimation on each face.
`Reduction.AStatedAzimuthIsTakenLessTheSetupsReadingOnTheBacksightNotLessTheCircle`
and `Reduction.WithNoReadingOnTheBacksightTheStatedAzimuthIsTakenLessTheCircleSetOnIt`
pin the orientation with a reading and a circle that are not zero (the
earlier tests subtracted zero, so they could not tell the rules apart),
beside
`Reduction.ASetupWhoseBacksightNothingPlacesIsOrientedOnTheAzimuthTheFileStatesNotTheCircle`
and `Reduction.AStatedAzimuthOnABacksightWithNoPositionYetWaitsForTheBacksightsCoordinates`;
`SurveyProjects.AStatedBacksightAzimuthThatIsNotFiniteIsRejectedAndAFiniteOneIsKeptApart`
validates the field; `cli.survey_read_sokkia_sdr` and
`cli.survey_import_sokkia_sdr`, whose positions are worked by hand in
`src/katana_app/CMakeLists.txt` to the micrometre (with the reduction's
refraction coefficient, 0.13, named); `McpServer.AnAgentReadsAndImportsASokkiaSdrFile`;
and the wizard's content step, which lists the fixture's two setups. The
truncation-and-noise test the raw readers share fails on an `Internal`
error: `readSurvey` turns a reader's exception into one, so it had passed
with a reader that threw (it did, on a `00` line two characters long). The
Survey Controller probe (`src/katana_surveyio/trimble_dc.cpp`) steps aside
for an SDR header with any derivation code, not only `NM`: Sokkia allows
`ED` as well, and such a file named `.dc` was claimed at 0.5. The probe
identifies a file by its header: 0.8 on the header alone, 0.95 (0.98 named
`.sdr`) when at least nine in ten of its first 200 lines are record-shaped
- a judgement, leaving room for a damaged stretch the reader skips line by
line. No other format writes that header, so a file with odd lines in it is
still identified; it was 0.6, and refused without FORMAT.

The wizard's Browse filter is built from the registry
(`katana::qt::surveyFileFilter`) and the few patterns no format names
(`.pnt`, `.xyz`, `.gt6`, `.x01`, RINEX 2's `.??o` and `.??d`); it had been a
hand-written list that lacked `.fld`, `.gts`, `.gts7` and `.dat`.

Not done:

- The transmission checksum is kept, not checked; a truncated file is
  signalled only by a last record with no line end.
- `09 MC`, `11` with distances, and road, template, GPS and levelling
  records are not imported, so a file written in those views, and a Nikon or
  Spectra instrument's file (its only observations are `09MC`), keep only
  their points and what is raw. The header's Nikon form would tell such a
  file apart, but what a Nikon `09MC` holds (a circle reading or an
  oriented one, a zenith with the job's curvature and refraction in it or
  not) is settled by no source here and no real file.
- The stated azimuth of a backsight whose coordinates orient the setup is
  not compared with the azimuth between the coordinates: the comparison
  needs a tolerance with a source, and the field software's azimuth is
  computed from the coordinates of its day (chapter 2's traverse differs by
  10" from its own final ones).
- An averaged back-bearing of several backsights (SETX 8.2.2) orients on the
  named backsight's readings, not the record's computed h.obs.
- The RW5 and field-file readers keep their files' backsight azimuths in
  metadata (`backsight azimuth (radians)`), the GTS-7 reader its bearing
  (`backsight bearing (radians)`), and the JobXML reader only the
  controller's orientation correction (`jxl.orientationCorrection_deg`) -
  none in `statedBacksightAzimuth`, so a setup of theirs whose backsight has
  no coordinates is still oriented on its circle. Filling the field there
  changes those readers' reductions and needs their own formats' checks.
- The person cannot yet state the coordinate order of an option-3 file (the
  read options carry no format-specific setting).
- `SURVEY IMPORT` takes control only from the file, so on the command line
  and through MCP a file with no coordinates, as the owner's is, places
  nothing; the window's reduction options can take control from points on
  the drawing. A control option for the verb is not done.
- The reader sets no limits on what a number may be - a distance of 10^16 m
  is read as one, and the other raw readers set none either, so a check
  belongs in the model or the reduction, for all of them.
- A file padded with NULs, or saved as UTF-16, is not recognised, since
  detection's shared `looksLikeText` takes any NUL for binary (FORMAT
  sokkia-sdr reads both).
- An observation from a point other than the current setup's is skipped
  rather than begun as a setup; the EDM and reflector offsets of a
  non-coaxial instrument (the Level 5 manual's B.4.4) are warned about, not
  applied; notes are kept with their setup, not with the record before or
  after them, since writers of the format disagree which.
- The mil is defined twice, here and in the GSI reader (`kTwoPi / 6400`),
  and `survey/angles.hpp` has no mil conversion to share; lifting it waits
  for the GSI work under way beside this one. The reader's calendar is the
  standard library's (`std::chrono::year_month_day`, its month and day
  ranges checked first, since those types keep only a byte); the RINEX
  reader (`isLeapYear`, `daysInMonth`) and the subsurface delivery schema
  still write the leap-year rule out themselves.

## The reduction's resection

Added 2026-09-30, and reworked the same day after review. A setup on a point
that nothing else positions - a free station, which a field controller calls a
resection (the opcode field file's 128 and 138) - used to be given up ("stands
on ..., which has no position: nothing was computed from it"), and everything
measured from it with it: the owner's resection job has twelve such setups,
and they held 4 056 of its 4 834 points. The reduction now computes such a
station from the setup's reduced pointings to points already placed. It looks
at nothing but the reduced pointings, so it serves every format with such a
setup, not the field file alone: `resectSetup` in
`src/katana_survey/reduction_adjust.cpp`, tried from `placeSetups` in
`src/katana_survey/reduction.cpp`.

**When it is tried.** When no setup is left whose station or backsight has
just been placed, the reduction falls back, one setup at a time, on: 0. the
resection of the first setup in the file that has what one needs; 1. the
file's own coordinates for a station nothing positions; 2. the circle as set
on a backsight nothing positions; 3. giving up, saying why. A fallback and not
a first choice: a station that another setup radiates keeps that position, as
it always did, and its setup's pointings to placed points are checks of it
(`ReductionResection.AStationAnotherSetupRadiatesIsNotResected`). Before the
file's own coordinates, because a resection is computed from what was
measured, which is what the reduction is for, where the file's coordinates of
a station are a controller's or a person's and unchecked - the order a
radiated point already takes over the file's coordinates of a point
(`PositionOrigin::FileOnly`). So a job whose setups stood on the file's own
coordinates before (an RW5 `OC`, a GSI station, a GTS `STN` all arrive with no
source, which only step 1 places) and observe enough placed points is now
resected there instead, and reduces differently: the file's coordinates are
then a check of the resection - a misclosure row "R by resection at setup S1,
against the file's own coordinates", a warning with the distance, and
`file_offset` in `SURVEY IMPORT`'s record - never dropped unseen
(`ReductionResection.AResectionComesBeforeTheFilesOwnCoordinatesForTheStation`).
An invented RW5 job of that kind, a third setup G whose `OC` is 50 mm north of
where its readings put it, went from the file's `OC` for G, with the warning
that it was used, to G resected from P and Q, 50.0 mm from that `OC` - the
warning and the misclosure row say so - and the shot from G moved with it (a
scratch check, main's CLI against this one). Before the circle as set, which
is an assumption about the instrument. A setup is tried once its targets are
placed, whichever setup places them
(`ReductionResection.AResectionWaitsForTargetsALaterSetupPlaces`): the
candidates are counted as points are placed, each placement touching only the
setups that observe that point, so the step adds nothing per placement and
`ReductionPerformance.SetupsPlacedOneAtATimeCostLinearlyInTheirNumber` holds
its ratio. A refusal leaves the setup to the later steps, which name it.

**What it needs.** Two placed points each observed with a direction and a
horizontal distance, or three observed with a direction: the fewest that fix
north, east and the orientation (four observations or three of three
unknowns). Points closer together than sqrt 2 times the settings' target
centring - the standard deviation of the difference of two centred targets,
which no pointing to them resolves - count once
(`ReductionResection.MarksCloserThanTheirCentringCountAsOnePosition`). Any
placed point serves - control, entered, GNSS, radiated, even a station on the
file's own coordinates - and is held as it is. Face pairs are meaned as the
reduction means them, and each reduced pointing is one observation. Where the
file marks the end of the observations its field software resected from (the
field file's 129: its reader sets `kResectionEndMetadata`, "record N", on the
setup, `include/katana/survey/data_model.hpp`), only the pointings before it
enter the resection; a controller's checks after the block - one right after
it, others after hundreds of shots - are checks of the resected station, each a misclosure row, as they were for the field software and as any
setup's pointings to placed points are
(`ReductionResection.ACheckAfterTheFilesResectionBlockChecksTheStationAndDoesNotMoveIt`,
`ReductionResection.ABlockEndThatNamesNoRecordLeavesEveryPointingToTheResection`).
The first version took them into the resection, where they stopped checking;
the owner's stations moved by up to 7.0 mm (heights 3.1 mm) between the two
versions, which also differ in their weights (the review measured 6.9 mm for
the checks alone). A file that marks no block gives every pointing to a placed
point.

**How it is solved.** By the network adjustment itself
(`adjustHorizontalNetwork`): the station free, the targets held, each
pointing's direction one of a set with an orientation unknown, each horizontal
distance with phase B's factors - the combined factor, or the height reduction
and the grid scale - taken at the approximate station
(`ReductionResection.ItsDistancesTakeTheCombinedFactor`,
`ReductionResection.ItsDistancesTakeAFixedGridScaleFactor`,
`ReductionResection.ItsDistancesTakeTheHeightReductionAtTheStationsHeight`);
then by the level adjustment of its trigonometric height differences - the
instrument and target heights, the reduction's curvature and refraction - to
the targets with heights. The weights are the reduction's one policy, the
settings' a-priori precision meaned over the faces in phase A, through the
helpers the network uses (`withCentring`, `distanceSigmaOf`,
`heightDifferenceSigmaOf`), for an instrument that is itself the unknown
(`StationModel::Resected`): each pointing carries its own precision and its
target's centring and height, not the instrument's, which are the same for
every pointing - treated as independent errors of each, they would weight a
short sight as if the instrument moved between pointings. Where the setup
records an instrument height, a mark is under the instrument, and its centring
and the measured height are added to the station's precision after the least
squares; an instrument height of zero is a free station with no mark, the
instrument itself the point
(`ReductionResection.ResidualsAndPrecisionMatchAnIndependentLeastSquares`,
`ReductionResection.TheHeightIsTheWeightedMeanOfTrigonometricHeightsWithCurvature`,
`ReductionResection.TheHeightIsAWeightedMeanOverSightsOfDifferentLengths`).
Both least squares go through the network's outlier loop, so the settings'
test - Baarda's w at `outlierSignificance` (critical value 3.29 at the default
0.001) or Pope's tau - flags a resection's residual as it flags a network's,
with a warning, and automatic rejection rejects as there: a rejected direction
takes no part in the orientation either
(`ReductionResection.AGrossReadingIsRejectedAndTheStationOrientationAndShotsComeFromTheRest`).
Gauss-Newton starts from a closed form: the rigid (Helmert, no scale) fit of
the station's own frame onto the targets with distances, or else the
two-circle construction from three directions. The orientation is the
solution's: `orientAndRadiate` orients a resected setup on the weighted mean
of azimuth less reading over the directions the resection used - the
least-squares orientation for where the station stands
(`ReductionResection.TheOrientationIsTheWeightedMeanOfItsDirectionsAtTheStation`),
which follows the station when a network adjustment moves it
(`ReductionResection.AResectedSetupFollowsItsStationWhenANetworkMovesIt`) -
and radiates the rest; the pointings it used are not also checks of it.

The direction sets are new in the network adjustment
(`SurveyHorizontalNetwork.ThreeDirectionsFixAFreePointAndTheOrientationOfItsSet`,
`SurveyHorizontalNetwork.ADirectionSetWithDistancesIsAdjustedToAnIndependentSolution`):
every direction read at one point is one set. Rejected: the angles from a
reference pointing that the reduction's network builds. They share that
pointing's error, which uncorrelated weights ignore, so a short first sight -
its centring error in every angle - spoils all of them; three of the resection
job's twelve blocks read a mark 3 to 5 m away. Rejected too: a resection
formula (Tienstra's, Collins') for the answer. It takes three directions
exactly, and so uses neither a fourth nor a distance, nor weights, nor says
how well the station is fixed.

**What refuses it,** each named in the warning that the setup was not
resected, and in the file's-own-coordinates warning where that follows, and
never a position: too few placed points (saying what it observes and what a
resection needs); a value that is not finite; a horizontal distance that is
not positive (`ReductionResection.ADistanceThatIsNotPositiveIsRefused`); a
least squares that fails - rank deficient, not converging - with its reason
(`ReductionResection.ALeastSquaresThatFailsIsRefusedWithItsReason`); and a
geometry that does not fix the station. That last is where the first version
failed. It refused the station on one line with its targets, and on the
danger circle, only when the readings were exact: in the review's probes,
read to an arc second, a station on the danger circle was placed anywhere on
it, 50 to 150 m off, a station on one line with its marks 25 to 88 m along it,
and one with two of its marks 5 mm apart 18 to 156 m off, with nothing
flagged. Now:

- Exactly degenerate geometry is still found in the closed form, and named
  (`ReductionResection.AStationOnOneLineWithItsTargetsIsRefused`,
  `ReductionResection.AStationOnTheDangerCircleIsRefused`).
- Otherwise the solution is judged by its own a-priori precision - the
  station's standard ellipse from the cofactor at the settings' weights,
  whatever the residuals say - against the least squares' own validity. The
  least squares takes each observation as linear about the solution; over an
  offset a from it, a distance of length D changes by up to a^2 / 2D more than
  that, and a direction by up to a^2 / 2D^2 radians (the second-order terms of
  hypot and atan2). Where the station's ellipse at the settings' confidence
  level (x 2.4477 at 95 %) reaches past the offset at which that exceeds an
  observation's own standard deviation - sqrt(2 D sigma) for a distance, D
  sqrt(2 sigma) for a direction - the model the least squares solved does not
  hold over the region the station may be in: its observations do not fix it.
  The criterion is the least squares' own and needs no tolerance from outside.
  It refuses all 57 of the review's degenerate probes (20 on the danger
  circle, 20 on one line, 10 with two of three marks 5 mm apart, 7 mixed) and
  9 more with two marks 5, 50 and 500 mm apart at 158 m (placed 0.2 to 30 m
  off before), and places the review's four sound ones exactly; where the
  least squares fails first, the refusal names the geometry just the same
  (`ReductionResection.AStationOnTheDangerCircleReadWithUnequalErrorsIsRefusedNotPlaced`,
  `ReductionResection.AStationOnOneLineWithItsTargetsReadWithErrorsIsRefused`,
  `ReductionResection.TwoMarksAFewMillimetresApartDoNotFixAStationWithTheirDistances`,
  `ReductionResection.TwoOfThreeTargetsAFewMillimetresApartDoNotFixAStationByDirections`).
- The warning names the geometry nearest one that fixes nothing - two targets
  at nearly one place, the station nearly on one line with its targets, or
  near the circle through three of them - by a measure that is zero for each
  exactly (their separation over their distance; the largest sine of an angle
  between two sight lines; the station's distance from the circle over its
  radius), below a tenth, which only chooses the words.
- A refusal leaves nothing of its least squares behind: the flagged-residual
  warnings it gave and any observation it rejected are taken back.

A geometry the criterion passes can still be weak. The resection reports its
dilution: the a-priori semi-major axis over the standard deviation of the
least precise single observation's line of position (for a direction, its
sight length times its standard deviation) - how many times the geometry
magnifies the observations' errors. Above `kWeakResectionDilution`, 3 - the
reduction's allowable multiplier, as a traverse's angular misclosure is
allowed three standard deviations - the station is placed, with the precision
it has, and flagged weak: a warning with its 95 % uncertainty and the
dilution, `weak=1`, and "WEAK GEOMETRY" in the report
(`ReductionResection.AWeakButSoundGeometryIsPlacedAndFlaggedWithItsPrecision`).
Worked by a separate script at the settings' defaults: three marks all round
the station with distances 0.37 to 0.44, two with distances 90 degrees apart
1.0, 30 degrees apart 2.7, 10 degrees apart 8.1; three directions 60 degrees
apart from outside their triangle 2.5, 30 degrees apart 9.1; a station 20 m
inside a 100 m danger circle 10.4, 5 m inside it 48; three marks within 11
degrees 123; the owner's twelve 0.41 to 0.94. The weak ones are placed where
their precision says: the review's station 5 m inside the danger circle, read
1 to 2" off, 99 mm from the truth against 264 mm at 95 %, and its three marks
within 11 degrees 86 mm against 434 mm. Rejected: refusing them too. Their
positions are right to the precision they state, and throwing a measurement
away is the person's decision (`ReductionSettings::autoRejectOutliers` is off
by default for that reason); the flag and the warning put it in front of them.

**What it reports.** `ReductionReport::resections`: per resected setup the
station, the points it was computed from, its coordinates and one-sigma
precision (a posteriori where there is redundancy, a priori where there is
none, the mark's centring and height added where there is a mark), its
orientation and that orientation's precision, the a-priori ellipse and
dilution, the checks after its block, the file's own coordinates' difference,
and each least squares as an adjustment is reported - a residual per
direction, distance and height difference with its redundancy number and
standardised value. The report prints a Resections table - with Dilution and
Checks columns, and a Check column that says which way a global test failed
("plan global test FAILED, 0.000 below 0.216 (fits too well)", or "above"
with the upper bound) as the network's summary gives its bounds - and each
resection's residuals ("Residuals: resection at S1 (horizontal)"), a row
FLAGGED as a network's is; the station's coordinate row says "resection". A
resection with no redundancy is warned about ("nothing checks the position it
gives"), and one with no height too. Where the settings reduce no distance to
the grid and the drawing's projection gives a point scale factor that would
change the longest distance by more than its standard deviation, the
resection has fitted ground distances to grid coordinates, and a warning
says so, naming the setups, with the factor in the record
(`ReductionResection.GroundDistancesFittedToAProjectedGridAreSaid`).
`SURVEY IMPORT` answers one `resection` record each - `dilution`, `weak`,
`checks`, `file_offset` and `unapplied_scale_factor` among its fields - and a
`resection_residual` record for each residual flagged or rejected, so an
agent sees which, past the cap on listed warnings (`docs/mcp.md`).

**Checked.** `tests/survey/test_reduction_resection.cpp`: two placed points
with distances and three directions alone, each recovering the station they
were made from; a perturbed resection whose position, orientation, residuals,
variance factor and precision match a separate script of the textbook model
(normal equations, not Katana's code) to 1e-8 m and 0.0001"; the orientation's
and the height's weights over sights of different lengths, each against a
weighted mean worked apart that an unweighted one misses by 0.13 mm and 0.44
mm; the distances' three factors; automatic rejection; a network that moves
the station; the file's block and its check; the file's own coordinates; the
unapplied grid scale; every refusal, exact and read with errors; the weak
flag; the order; the fallbacks. Each proven by removing its part: without the
precision criterion the four noisy-geometry tests fail; without the take-back,
the close-marks one; without the distances' factors, the four factor tests;
with the orientation or the heights unweighted, the test of each; with the
instrument's centring and height left in each pointing's weight, four; without
the mark's centring and height added to the station, three; with a rejected
reading left in the orientation, the rejection test; without the block, the
two block tests; without the file-coordinates check, its test; without the
grid-scale notice, its test; without the weak flag, its test; with the
resection's own pointings radiated as checks, two; with only the coordinate
tolerance making one position, its test. Two fixtures:
`tests/surveyio/data/fld/resection.fld`, whose free station
`cli.survey_import_resection_field_file` expects where a hand calculation of
its block puts it, the check after the 129 a misclosure (S1 1000.00033 E,
5100.00000 N from K1 and K2, redundancy 1 in plan; the 06 after its 129 a
check of it; the fixture's heights, written for the reader, disagree by 1.1 m
and are flagged, each with a `resection_residual` record), and
`tests/surveyio/data/fld/free_station.fld`, a free station in the structure of
a controller's resections - 128, target heights, three marks on both faces,
129, the residual comments, a check, then shots - with invented names and
numbers (`cli.survey_read_free_station_field_file`,
`cli.survey_import_free_station_field_file`, its height pinned to the weighted
mean of the block's three, 20.00030586).

The owner's resection job, `CRS SET EPSG:7856` and `SURVEY IMPORT` (a local
check; the file is the owner's): before the resection, 778 points and 42
reduction warnings, twelve of them "stands on ..., which has no position"; now
all 4 834 points and 71 warnings - the 27 face-pair warnings as before, the
prism and atmospheric notices, one notice that the twelve resections fitted
ground distances to grid coordinates, and 41 residuals the resections flag: 27
distances, every one computed shorter than measured, and 14 directions. Each
resection is of its block, redundancy 3 in plan and 2 in height; the 14 checks
after the blocks are misclosure rows; the dilutions are 0.41 to 0.94, none
weak. The import's defaults apply no grid scale factor, and the marks are MGA
grid coordinates where the zone's point scale factor is 0.99980 (200 ppm, 32
mm on the job's longest resection sight of 162 m). The controller wrote its
own residuals after each block, one line per face per mark: another program's
numbers, so an independent check. Its scale factor is in them: fitting its
station, orientation and scale to where its residual lines put the marks gives
0.999792 to 0.999800 for the twelve, the projection's factor there. With that
factor applied (a fixed 0.9998, through a scratch harness, since `SURVEY
IMPORT` sets none; Survey > Survey Jobs does), Katana's residuals agree with
the controller's over the 36 marks, against the mean of each mark's two faces,
to 0.95 mm RMS in distance (at most 2.9 mm) and 0.87 mm in height (at most 2.3
mm), and its directions to 1.9" RMS over the 27 its weights put under 10" (at
most 4.9"); the other 9, to marks near enough that the target's centring
weighs them over 10", differ by up to 39"; and its twelve stations are 1.2 mm
RMS from the controller's own (at most 3.2 mm; heights 0.85 mm RMS, at most
2.0 mm). Without the factor - `SURVEY IMPORT`'s default - the stations are 4.5
mm RMS from the controller's (at most 7.5 mm; the first version, which took
the checks in, 6.1 mm RMS and 11.3 mm at most, as the review measured it),
which the grid-scale notice now says, and the distance residuals 14.6 mm RMS
from its (at most 31 mm). What is left differs because the controller adjusts
each face as an observation where the reduction means the pair first, and
weights them its own way. The reduction of the job took 13.6 to 13.7 ms before
the resection and 17.9 to 18.3 ms now (Release, the reduction alone, best of
five in each of three runs) - the twelve least squares, and the 4 056 points
they let it radiate and report; the whole command, drawing and listing 6.2
times the points, 0.27 s before and 0.39 s now (best of five).

The owner's other files import as they did before (`SURVEY IMPORT`, every
`LIST` position compared): the second field file (862 points, 8 warnings), the
RTK job (3 482, none), the SDR traverse (no point, 47 warnings: none of its
setups observes a placed point) and the GSI traverse (57, 24) give output
identical to main's - none of their setups resects.

Not done:

- The targets are held: their own errors do not reach the station's precision.
  A resected station is not computed again when an adjustment moves its
  targets - the network adjustment adjusts it with the rest, a traverse
  adjustment leaves it.
- A setup whose station is already placed and that has no backsight - a second
  resection block on a station an earlier one placed - is not oriented on the
  placed points it observes: its targets are not radiated, and its warning
  names the points it could be oriented on
  (`ReductionResection.ASecondBlockOnAStationAlreadyResectedSaysWhatItCouldBeOrientedOn`).
  Orienting every such setup on them would change every format's setups that
  name no backsight, which is the reduction's decision to make, not this
  one's.
- A setup whose station fell back on the file's own coordinates before its
  targets were placed is not resected once they are.
- The reduction's network still turns a setup's directions into angles from a
  reference pointing: the network adjustment knows no setups, so two setups on
  one point would be one set there.
- The report gives no per-mark coordinate misfit (dE, dN, dZ), which the
  controller prints beside its residuals.
- `SURVEY IMPORT` applies no grid scale factor, so a resection in a projected
  drawing fits ground distances to grid coordinates: it says so, and the
  owner's stations are 0.8 to 6.8 mm from where the projection's scale puts
  them. Whether the import should take the projection's scale where the
  drawing has one is `SURVEY IMPORT`'s decision for every setup, not the
  resection's alone.
- A weak geometry is flagged, not refused, whatever its dilution short of the
  criterion that refuses it.

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
