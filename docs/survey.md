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
                 declares a CRS,                                    layers, points       one undo step
                 transforms nothing                                 (+ the finish: codes, styles, lines)
```

The bridge returns a single `CommandPtr`, the same shape
`cad/survey_coding.hpp` already uses. That is what makes an entire import one
undo step, which the brief asks for in its section 19 - not a special case, just
the command pattern the application already has.

The bridge itself makes layers and points and nothing else: every point lands
on the import's layer, ByLayer, with its code in a property. Coding those
points (their rule's layer, style and attributes) and stringing them into
lines is the FINISH, `cad::withSurveyFinish` (`cad/survey_finish.hpp`), which
wraps the bridge's command so that import, codes and lines are still that one
command and one undo step. It is off unless asked for
(`SurveyFinishOptions`, `SurveyJobImport::finish`); `docs/survey_coding.md`,
"One step from field file to finished drawing", has what it does and why it
is composed inside the job command rather than around it.

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

> Leica GSI (GSI-8, GSI-16) - import: yes, export: no, parser 1.2

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
| Leica GSI (`leica-gsi`): GSI-8 and GSI-16, mixed or not; any extension when every line is a GSI block | yes | no | 1.2 | total station words only (a digital level's are counted, not read); declares no coordinate system; a sexagesimal angle whose word writes 60 seconds in their place is read as the next minute; a circle reading past a full circle, and a negative vertical reading, are refused; a shot with no zenith angle read keeps its own horizontal distance and height difference (32, 33) (see "Leica GSI" below) |
| Opcode field file (`opcode-field-file`): `.fld`, tab-separated records opening with a numeric opcode, total-station and GNSS (RTK) jobs | yes | no | 1.2 | opcodes 02, 03, 04, 05, 06, 07, 09, 16, 20, 29, 41, 42, 43, 44, 71, 72, 73, 99, 100, 124, 125, 128, 129, 138, 139 and -2 read; every other opcode skipped with a warning naming it; an RTK position written as 02 with its receiver's "GNSS Solution" is a GNSS position, any other 02 an entered coordinate; a setup's backsight is its first 04 that can orient it; a backsight's stated azimuth kept apart from its circle reading; every measurement of a point keeps its attributes; offsets move the shot they follow; a resected setup's station is computed by the reduction's resection from its 128 ... 129 block, the checks after it checking it (the file states no station); the coordinate system is declared by name from the header comments, never as a guessed EPSG code |
| Sokkia SDR (`sokkia-sdr`): `.sdr`, the SDR33 and SDR2x layouts, a header record `00` naming `SDR33` or `SDR2x` | yes | no | 1.0 | records 00 to 13 read; records marked deleted (`DD`) skipped by name; derived views (09 MC, 11 with distances) and road, template, GPS and levelling records skipped with a warning, a shot with no raw twin said to be lost; units from the header - an undefined angle or distance unit refused, an undefined pressure or temperature unit warned about and not read, a `13DU` followed; coordinates in the header's order (1 N-E-Elev; 2 E-N-Elev and Trimble's 3 east first, each with a warning), a point's latest kept, an observation's POS view (after its 09 or 11) never over a position record; collimation (04) applied per face until another instrument type or job; declares no coordinate system |

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

The owner sent a traverse (GSI-16, 1593 blocks over 52 setups, kept on the
owner's machine and not committed; its name and its values are not copied
here), asking that it be read correctly and completely. It read with 25
warnings, and 24 of them threw an observation away: an angle word, 21 or 22
in unit 4 (sexagesimal), whose seconds were exactly 60 with a tenths digit of
0 - DDD MM 60.0 - refused because minutes and seconds run to 59.

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
  the same minute they are 49" to 62" out. The other 5 are extra face-left
  pointings with no face right in their round: read as the next minute each
  is within 5" of the same face's other readings of its target, and 56" to
  65" out read as 00 seconds. (The file has 857 face-left shots and 684
  face-right.)

So a sexagesimal word whose seconds are 60 with nothing after them is read as
the next minute, carried in whole numbers before the angle becomes a real,
and the file gets ONE warning: the count, every record (up to 100), and the
first word as written and as read - on the hand-built fixture, "at records
4, 5; the first, word 21, 044 59 60.0, read as 045 00 00.0". It says the
reading is "within the writer's rounding of what was measured", not a
figure: this writer rounded to whole seconds, so its 60 stood for anything
from 59.5, and another may round to tenths. The whole-number carry makes the
angle the same double as the word written with its carry; adding 60/3600 of
a degree as a real does not for every angle (4.4e-16 rad, one unit in the
last place, apart at 213 46 60.0; 213 17 60.0 comes out the same both ways),
and `ACarriedAngleIsExactlyTheAngleWrittenWithItsCarryInEitherWidth` fails
when it is done that way.

The carry is made only where the word writes both digits of the seconds in
their place: at its block's full width (8 or 16 characters), or with four
digits or more after a written point. A word of another width is aligned from
the right, and a written point's missing digits are supplied as zeros; either
can make a 60 of digits that were never the seconds. `000000002134600`,
213 46 00.0 with its last digit lost, reads from the right as 021 34 60.0,
and `213.466` is 60 only with the 0 the reader supplies. 1.0 refused both at
their record; carrying them, as the first version of 1.1 did, turned a
damaged word into an angle - 192 degrees wrong in the first example - with
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

**The sign of a reading (second review, 2026-09-30).** GSI ONLINE gives
every measured value a sign (page 6: "+: Positive value", "-: Negative
value"), and the two circles do not use it alike. Word 22 is read as a zenith
angle, and no zenith angle is negative. What writes a negative V is not
known: a damaged word, or an instrument's vertical angle setting reaching the
file. GSI ONLINE lists such a setting - SET 44, "V angle READING: 0 Zenith, 1
Horizontal, 2 Slope in percent" (page 9, the TPS100 series; the document
lists none for the others) - under which a sight below the horizon reads
negative, but nowhere says that word 22 follows it rather than the display
alone. Leica's manuals describe the like settings as ones of the display:
the TPS1200 Technical Reference Manual (version 5.0, page 343) gives
`<V-Display:>`, whose "Elev Angle" is "positive above the horizon and
negative below it", and the FlexLine TS02/TS06/TS09 User Manual (version
1.0, page 45) says that counter-clockwise Hz directions "are displayed but
are saved as clockwise directions"; neither says what a GSI word holds
under a V setting. So the warning gives both causes as possibilities, not
SET 44 as the cause. Either way a negative zenith is none. The circle check
bounded only the magnitude, so V -005 00 00 was wrapped to 355 00 00 and read
as a face-right zenith of 5 degrees: on a hand-built job, a 100 m shot at 45
degrees was drawn 8.7 m out at 225 degrees, 108 m from where a 5-degree
depression puts it, with no warning. A negative word 22 is now refused at its
record, in every unit, as the opcode field file, SDR, GTS-7 and RW5 readers
refuse a zenith reading below 0; -0 is zero, and read. If a setting wrote the
negatives below the horizon, it wrote positives above it, and those read as
zeniths near the vertical with nothing in their words to show it, so the
import's note on the vertical angle setting also says how many were refused
and that the file's other vertical readings may not be zenith angles either
(`ANegativeVerticalReadingIsRefusedAtItsRecordNotWrappedIntoAFaceRightZenith`;
through `katana_cli`,
`cli.survey_read_gsi_refuses_a_negative_vertical_reading_at_its_record` and
`cli.survey_import_gsi_draws_no_point_from_a_negative_vertical_reading`,
whose one radiated shot is worked by hand in `src/katana_app/CMakeLists.txt`).
The note counts every refusal, a negative V in a code block's included,
whose measurement words are not read: each is warned about at its record, and
each is the same sign of what wrote the file. The 60-second note differs on
purpose - it says its words were READ as the next minute, so it counts only
those whose values were kept.

A negative word 21 is still read on the circle, -090 00 00 as 270 00 00, and
GSI ONLINE supports that as far as it speaks to it: every measured value
carries a sign, and none of the horizontal circle's settings it lists gives
the reading another meaning - 171 turns it clockwise or counterclockwise, 178
and 179 switch the Hz compensator and the collimation correction. A negative
direction is the direction it names, counted the other way from the zero; the
other readers wrap it the same way. The document shows no negative angle word
of either kind, so neither is claimed to be common. Rejected:

- reading a negative V as an elevation, a zenith of 90 degrees less it: that
  is the Horizontal setting's value, the percent setting's is another, and
  the file does not say which;
- refusing every V of a file that has one negative: one damaged word would
  cost the job its heights. The note puts the question to the person;
- refusing a negative Hz: it discards a direction whose value is not in
  doubt.

A refused word is now shown with its '-' where it has one - "word 21
'-40000001' is past a full circle (400 gon)" - the sign being part of the
value written. A sexagesimal word that writes no digit, ".", read as 000 00
00.0, the zeros supplied to a short word, with nothing at its record; it is
refused, as the same word already was in gon, degrees and mil
(`ASexagesimalWordOfNothingButAPointIsRefusedNotReadAsZero`).

The full-circle test held only words far past the circle, and passed with
either edge of the check removed: with `degrees > 360` alone, which reads
360 00 00.1 as 0.1" past the zero, and with the magnitude taken without
`std::abs`, which wraps -400.00001 gon onto the circle. It now holds a word
just past each edge - 360 00 00.1, 360 00 01.0, 360 00 60.0 (refused after
its carry, and not counted among the 60-second words), -360 00 00.1, and
-400.00001 gon, -360.00001 degrees and -6400.0001 mil - and -400.00000 gon,
which is the zero. Each of the two mutations now fails it.

The negative V and the word of only a point read differently from parser 1.1,
which was pushed on 2026-09-30 without them, so they are parser 1.2: two
readers calling themselves 1.1 would leave an import's SourceRecords unable
to say which reading it had. (They were first folded into 1.1 when the
remote's reader was still 1.0; main was pushed with 1.1 hours later.) Where
every word 22 is refused, the note on the vertical angle setting now says
that none was read as a zenith angle, where it said "word 22 was read as a
zenith angle" and doubted "the file's other" readings, of which there were
none (`WhenEveryVerticalReadingIsRefusedTheNoteSaysNoneWasReadAsAZenithAngle`).

**A shot's own horizontal distance and height difference (third review,
2026-09-30).** Words 32 and 33 are the horizontal distance and height
difference the instrument computed from its slope distance and vertical
reading. Beside a usable slope distance the reader counted them as derived
and did not carry them, whatever else the block held, and the note said they
stood "beside the measured slope distance and zenith angle". A block whose
zenith angle was refused - negative, or past a full circle - or that recorded
none lost them with that untrue note: a slope distance cannot be reduced
without a zenith angle, the reduction rejected it, and the point was not
drawn, though the file held the horizontal distance that places it. Refusing
a negative V sent a new kind of block down that path. The rule is now its
reason: 32 and 33 are derived, and not carried, only where the block keeps
what the reduction computes them from - a zenith angle and a distance.
Otherwise they are the shot's. A block with no zenith angle read beside its
slope distance gives its own horizontal distance as the shot's one distance,
its height difference as a level difference from the station, and the slope
distance is not carried; the import says once how many slope distances it
passed over, and the first record. A height difference beside a zenith angle
with no distance, which 1.1 also dropped as derived though nothing could
compute it, is carried the same way. The derived note now says "beside a
zenith angle and a distance that were read"
(`AShotWithNoZenithAngleReadKeepsItsOwnHorizontalDistanceAndHeightDifference`;
through `katana_cli`,
`cli.survey_import_gsi_radiates_a_shot_with_no_zenith_angle_by_its_own_horizontal_distance`
on `tests/surveyio/data/leica/derived_distances_gsi8.gsi`, whose three
radiated points are worked by hand in `src/katana_app/CMakeLists.txt` - two
by their own 32, one by its slope distance and zenith angle, 0.4 mm from
where its own 32 would put it). Rejected:

- carrying the slope distance beside the horizontal one: the reduction
  radiates a pointing by its first distance and makes a second a rejected
  shot of its own, so which of the two placed the point would hang on the
  order the reader wrote them;
- reducing the slope distance by the height difference: it redoes the
  instrument's own reduction, which the 32 already is.

The CLI cases that pin the parser version had passed with it wrong: a `;` in
a quoted note split each expression into alternatives, one of which matched
on its own (`docs/testing.md`, "The command line: cli.*"). They are escaped,
and fail on a reader built to say 1.1.

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
- **51 of 52 setups had a backsight.** The traverse's first setup has as
  its reference object a mark that is keyed in only when the instrument
  stands on it, 166 records later. A setup's first shot was its backsight
  only if the point had coordinates EARLIER in the file, so the reduction
  left the first setup unoriented. It has the whole file - it already waits
  for a backsight positioned later - so the rule is now that the file gives
  the point coordinates, before the setup or after it, other than by the
  setup's own shots: target coordinates (81-83) the instrument computed with
  the orientation the backsight is to give would orient the setup on itself
  (`AFirstShotToAPointOnlyItsOwnSetupPositionsNamesNoBacksight`). The note
  says how many backsights came from coordinates later in the file.
- **One distance was measured with another prism constant.** One shot's
  word 51 gives a prism constant where every other shot's is 0, and its
  slope distance is longer than the setup's 14 others to that mark by that
  constant, to within a millimetre. The distance keeps the
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
no distance where it rejected 12, and orients every setup. With the first
setup oriented, its backsight check to its reference object is +39.0 mm in
distance and +34.1 mm in height over 321 m, and the setup on that mark
checks its own backsight at +21.7 mm, where every other setup's distance
check is within 4 mm. Both involve that mark, whose keyed-in coordinates the
reader reads as written: a question for the job's control, not for the
reading. The review round's changes (the width rule, the circle, the
kept-word count, the code words in other blocks) read this file exactly as
before but for the wording of the 60-second note: none of its 24 words is
short, long or pointed. The second review's (the sign, the word of only a
point, the warnings' '-') read it exactly as before, wording and all: it
has no negative word 21 or 22 and no word of only a point - 1593 records, 4
warnings, 4623 observations, and 57 points drawn by `SURVEY IMPORT`, as
with the reader before them. So do the third review's (32 and 33, the note
when every V is refused, parser 1.2), but for the version in the summary
line: every one of its 1541 shots is words 21, 22 and 31, with no word 32 or
33 and no negative word 22.

Tests (`tests/surveyio/test_leica_gsi.cpp`, on the hand-built
`tests/surveyio/data/leica/rounded_seconds_gsi16.gsi` and inline blocks):
`SixtySecondsWithNothingAfterThemAreReadAsTheNextMinute` (086 27 60.0 is
1.509128026557763641 rad, worked in exact rationals),
`TheImportSaysOnceHowManyAngleWordsWroteSixtySecondsAndWhereTheyAre`,
`SecondsPastSixtyAreNoRoundingAndTheirWordIsStillRefusedByRecord`,
`SixtySecondsWithTenthsOrMinutesOfSixtyAreNoRoundingAndAreRefused`,
`SixtySecondsTheWordDoesNotWriteInTheirPlaceAreRefusedNotCarried`,
`ASixtySecondWordWhoseValueIsNotKeptIsNotSaidToBeReadAsTheNextMinute`,
`ACircleReadingPastAFullCircleIsRefusedInEveryUnitAndAFullCircleIsZero`,
`ANegativeVerticalReadingIsRefusedAtItsRecordNotWrappedIntoAFaceRightZenith`,
`WhenEveryVerticalReadingIsRefusedTheNoteSaysNoneWasReadAsAZenithAngle`,
`AShotWithNoZenithAngleReadKeepsItsOwnHorizontalDistanceAndHeightDifference`,
`ASexagesimalWordOfNothingButAPointIsRefusedNotReadAsZero`,
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
`src/katana_app/CMakeLists.txt`, the two on
`tests/surveyio/data/leica/negative_vertical_gsi8.gsi` and the one on
`tests/surveyio/data/leica/derived_distances_gsi8.gsi` named above. The
window's Survey > Import Survey Data reads a GSI file through the same
reader, and `katana_mcp` runs the same verbs.

Not done:

- **The reduction's face pairing.** It pairs a setup's i-th face-left
  pointing to a target with its i-th face-right one
  (`src/katana_survey/reduction.cpp`). Each setup of the traverse has 3 to 9
  more face-left pointings than face-right ones (3 in 41 of the 52), and in
  the setups looked at they come first, so its pairs cross rounds - the
  first setup's first face-pair warning (15.0") pairs a face-left pointing
  of one round with the face-right pointing of the next, where each round on
  its own gives -3" and -1" - and some of the file's face-pair warnings come
  from that, not from the data. Reading the 60-second words moves it both
  ways: a pointing whose zenith was refused before now has a face and pairs
  with a face-right pointing of another round, adding one warning
  (horizontal 19.0"), while another setup's pairs to one target go from
  three to six, all within tolerance. It belongs to the reduction and every
  format that feeds it, not to this reader.
- **A level difference gives a radiated point no height.** A shot with no
  zenith angle read keeps its own height difference (33) as a
  LevelDifferenceObservation, but radiation takes a target's height only
  from its pointing - a zenith angle with a distance
  (`src/katana_survey/reduction.cpp`) - and a level difference, which has no
  pointing, enters only a levelling adjustment. So on
  `tests/surveyio/data/leica/derived_distances_gsi8.gsi` Q1 and Q2 are
  placed by their own 32 and drawn with no elevation ("2 point(s) have no
  height in the source"), though the file gives their height difference. It
  is the reduction's, for every format that writes a level difference beside
  a shot, not this reader's.
- **The backsight check has no tolerance.** `src/katana_survey/reduction.cpp`
  stores each setup's backsight distance and height differences, and the
  report prints them as bare numbers, where face pairs and misclosures are
  marked OUTSIDE TOLERANCE. So the first setup's +39.0 mm, left unoriented
  with a warning before and oriented now, is flagged nowhere. A tolerance
  belongs in the reduction's settings beside the face-pair ones, with a
  source for its value; it would apply to every format, so it is not added
  here.
- **A later block's code information or remark replaces an earlier one's.**
  A point named by two blocks - a target shot in two rounds - that give code
  information (42-49) or a remark (72-79) keeps the later value under its
  key, and the earlier goes without a word; its code (71, or a code block)
  keeps the first instead, and its coordinates keep the first with a warning
  when they differ. Which value is the point's when rounds disagree is a
  choice for all three together - the first, the last, or each block's with
  its record - not one to make for the one kind of word found, so it is
  left as it was. No file seen has shown it: the traverse has no word 42-49
  or 72-79.
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
height; 47 ends the current string before the current point, which "becomes
the first point of a new string" of the same code and number, and 48 ends it
after that point; 29 is a memo; 41, 71, 72 and 73 add
text, an integer, a real and a text attribute to the point just measured, a
blank name making an attribute unnamed; 99 ends the file; 100 gives the
units, of which the format allows one each (decimal degrees, metres,
millimetres of pressure, celsius). 124 and 125 (attribute groups) and 140 (a
GNSS coordinate) "do not exist in the fld file". 44.5 finds the point a
setup, a backsight or a check names by a search that stops at the first
found: the directly entered coordinates before it, then the measurements
before it, and in each its point name among their point names, then its
point name among their point IDs, then its point ID among their point IDs
(steps 4 to 9; steps 1 to 3 search the reducing program's own models, and
step 10 asks the person). A 20 and an offset that give a point description
act on the string its code and number name, else on "the point with that
point ID".

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
  circle, X, the instrument height) blank where the other layout reads a
  whole record - a number there, and a point named - the one sign of which
  layout wrote it: it is then skipped, saying so. Text there, or a reading
  that names no point, is no such sign: a setup with a comment and no
  instrument height, read without the column, has the comment where its
  height would be, or names no point, and it is read as the file's records
  are. A record that does not fit the layout it is read in is skipped with a
  warning, and one blank field past a layout's count is a trailing tab - as
  it is on a 16, a 20 and an offset, which until 2026-09-30 were skipped for
  it (a 16 so skipped took its string, and a bare 20 after it, with it).
  Rejected, as this reader had it until 2026-09-30: any value there as the
  sign, which skipped such a setup and every measurement after it until the
  next. Rejected, as this reader once did: skipping, in a file whose records show
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
  it) stay entered, and a solution after a setup record does not make the 02
  a GNSS position - its own records end at the setup - though the attribute
  is still kept on the current measurement point, the 02's, to which the
  description gives 71 to 73. Rejected too: sigmas from the receiver's quality
  attributes (its quality and positional-uncertainty figures), which are its
  own words for its own statistics. A mark given a GNSS position and an
  entered coordinate that differ holds the entered one, whichever came first,
  and the one it does not hold goes beside it in its metadata: a later GNSS
  position as "coordinates restated at record N", an earlier one as "GNSS
  position of record N". Until 2026-09-30 an entered coordinate after the
  GNSS position was put there as "restated" - the held coordinates, under a
  name that said they were not held - and the GNSS position's values were
  nowhere on the point.
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
  finds the point it names as 44.5 searches, among the coordinates (02) and
  then the shots (07) before it - the name among their names, the name among
  their IDs, the ID among their IDs, in each - and stands on or measures a
  point of its own name, else ID, only where that finds none (step 10, which
  asks the person, has no counterpart in a file read). A 03 that names the ID
  a coordinate was given, alone or with a name of its own, stands on that
  coordinate, and a check whose name is a shot's and whose ID is a
  coordinate's checks the coordinate, as step 6 comes before step 7. What
  such a record calls the point is kept, not made a point of its own: a
  setup's name and ID with the setup ("point name", "point ID"), a
  backsight's or a check's on the point under its record ("record N/point
  name"). A point only a setup, a backsight or a check made is in neither of
  44.5's lists, which hold the directly entered coordinates and the
  measurements, so a record naming it with the ID of a coordinate or a shot
  finds that coordinate or shot; a record whose name is another point than
  the one found is warned of ("the setup names point 'T1' with point ID 5,
  and the format's search for its point (44.5) finds point '5' first").
  Rejected: searching the points those records made first, as if step 10 had
  put each in a model steps 1 to 3 search - the description's program adds
  it to one only where the person says so. A 20 and an offset find the point
  their description's point ID names, as the description's entries for them
  say: the shot's (07) where a shot and a coordinate (02) were both given the
  ID - the description calls a point ID "normally unique", the ID a
  measurement is given, and an offset moves a shot - saying so, and for a 20
  the coordinate's where only its string is open; else, the reader's own
  reading, the point of its name. A point with a name and an ID keeps the
  first ID given it in its metadata, and a different one a later record gives
  under that record. Rejected, each as this reader had it until 2026-09-30:
  searching the ID only for a record that gave no name - a setup, backsight
  or check that gave both a name of its own and a coordinate's ID stood on
  or measured a new point of that name, with no position, and nothing was
  radiated from the setup, or a check made a second point on top of the one
  it checked; looking a 20's or an offset's ID up among the names, which
  skipped both for a named shot, saying no record had made the point; the
  coordinate's before the shot's for a 20 and an offset, as 44.5 has them for
  a setup - an offset of the shot was then kept unapplied, as one of a
  coordinate, and the 20 skipped; keeping what a backsight called a point it
  found by another name as a point of that name, which the reduction then
  reported as not drawn; and the coordinates and the shots as one list, name
  first - 44.5 checks a coordinate's ID before a shot's name.
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
- **The backsight.** A setup's backsight (`SurveyStation::backsightPointId`)
  is the point of its first 04 that can orient it: a 04 that gives a
  horizontal circle to a point with a position the reduction will find -
  coordinates an 02 gives it anywhere in the file, for the reduction places
  every one before any setup, or a direction and a distance measured to it
  from an earlier setup - or to a point a 04 gives an azimuth. Where no 04's
  point can, it is the first point a 04 gives a horizontal circle to, and the
  reduction takes that circle as set to the grid, saying so; where no 04
  gives one, the first 04's point. The reader settles it when the file ends,
  so an 02 after the setup, or after the next one begins, still counts. The
  circle and the azimuth are that point's. `SurveyStation::backsightAzimuth`
  is the circle reading on it, as the model defines it: the horizontal circle
  of the first 04 to it that gives one, as its face-left reading (a
  face-right one less 180 degrees, as the reduction means the pointings to
  the backsight); a later 04 to it changes it no more than a later pointing
  would. The 04's azimuth is `SurveyStation::statedBacksightAzimuth` - the
  first a 04 to it gives; a later one that differs is kept in the setup's
  metadata, with a warning - which the reduction orients the setup on, less
  the reading on the backsight, only where nothing places the backsight: the
  description's "when no coordinate for the backsight point exists". It is
  one field for every reader whose file states an azimuth on its backsight,
  not a second one for this format. A 04 to any other point is read as a
  measurement of that point, with a warning naming the backsight and why it
  was taken, its azimuth kept in the setup's metadata: the reduction
  radiates the point, or, where it has a position, checks it. Where their
  formats leave the choice to them, two other readers also weigh whether a
  candidate can orient the setup: the GSI reader takes a setup's first shot
  for its backsight only where that shot is to a point the file gives
  coordinates, before or after it ("a first shot to an unknown point orients
  nothing and is not named"), and looks at no later shot; the SDR reader
  orients a setup with no backsight record by the first keyed azimuth to a
  point it observes, else the first. The GTS-7 reader takes the first BS
  header's point, whatever it is. Why the first that can orient, and not
  simply the first or the last: with two 04s to different points, one to
  control and one to a point nothing places, with no azimuth, either fixed
  rule orients the setup on the unplaced point's circle taken as a grid
  azimuth in one of the two orders - the last 04 where the control comes
  first, the first 04 where it comes second - turning every shot by the
  difference and reporting the control point as a misclosure of its own
  (51.8 m in the case the tests work); and a first 04 with no horizontal
  circle left the setup with no direction to orient on, its shots not drawn.
  Rejected, each once this reader's: the first 04, whatever it could do; the
  point of the setup's last 04 with the circle and azimuth of its first
  (until 2026-09-30), which oriented the setup on one point's circle less the
  reading on another's and turned every direction by the difference - a
  setup whose first backsight had coordinates and whose second had none drew
  its shots 60 degrees round, with no reader warning; the last 04 naming the
  backsight with its own circle and azimuth; and the 04's azimuth in
  `backsightAzimuth`: the report then said the circle had been taken for a
  grid azimuth, a backsight with coordinates and no circle reading was
  oriented on the azimuth taken for the circle (a shot at 90 degrees drawn on
  bearing 45), and a face-right 04 after it replaced the azimuth with its
  circle, turning every shot by 180 degrees.
  Rejected too: of the 04s that can orient, the one of the best kind -
  coordinates before an azimuth - rather than the first. Where a first 04's
  azimuth and a later one's coordinates disagree, the first orients the
  setup and the later one's coordinates check it, a misclosure in the report;
  taking the later one, nothing would check the azimuth. And a later 04
  re-orienting the measurements after it: the description's 04 entry says
  only that its reducing program shows each 04's "bearing datum difference";
  that the difference holds for the measurements after that 04 is an
  inference from the entry for 50, which says so of its own bearing. The
  model orients a setup once, on one point, so that would split the setup in
  two - as the SDR reader's `splitSetup` does, where its format says a
  backsight record orients what follows it - and each part would still need
  a backsight that can orient it.
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
  after the 16 closes it rather than the shot's first string. A 16 that is
  not read (one value too many, or no feature code) leaves which string is
  current unknown until a measurement or a 16 is read: a bare 20 after it is
  skipped, naming it, where it closed the string of the shot before.
- **Close string (20)** marks the feature closed (`SurveyFeature::closed`) and
  takes it off the open list, so a later point of the same code and number
  starts a new string rather than joining a closed polygon. A description
  closes the string its code and number name, else the string of the point
  its point ID names (above); an offset's description finds its point the
  same way, the last point of that string, else the point of that ID.
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
  skipped, in one warning, and does not vote on the layout: it did, and five
  records without the column after a 99 outvoted four with it before, so the
  file was read without it and every record before the 99 refused. **An
  opcode the description defines only for its
  XML form** (140 GNSS coordinate, 145 GNSS offset ...) has no .fld layout and
  is skipped, saying so; 124 and 125 are read because a real file shows their
  layout and a misread one can only misname an attribute. Every other opcode
  is skipped with the description's name for it ("opcode 18 (circle feature)
  is not one this reader imports"), and one that changes the records after it
  says so: 15, 50, 127 and 131 ("the measurements after it are read without
  it"), the field-template opcodes 51, 53, 54 and 56 to 59 (the measurements
  after them "keep the codes they are written with, which a field template may
  have changed") and 47 and 48 (the points after them "are strung as if the
  string had not ended"; for 47, too, the current point "is not moved into a
  new string of its own").
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
| RTK job, 134 427 lines, Windows-1252 when first read, UTF-8 since the owner saved it again on 2026-09-30 | not recognised; with FORMAT, refused: no record but the version line could be read (134 393 skipped) | 134 394 read, 0 skipped, 2 warnings (two marks given a second GNSS position, 5/32/4 mm and 9/2/11 mm from their first; the Windows-1252 copy had a third, the encoding); 3 482 points placed by 3 484 GNSS positions; 209 strings, 22 closed; the system its header names |
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

The second review's fixes (2026-09-30: the first backsight, 44.5's search by
ID and its order, 20 and the offsets by ID, a record whose other layout
reads no whole record, an unread 16, the votes after a 99, a GNSS position
before an entered coordinate) change nothing in the three files, which have
none of those cases - no setup with 04s to two points, no 03, 04 or 06 that
gives both a name and an ID, no 20 or offset with a description, no 99, and
every 16 whole. Checked, not assumed: a dump of everything the reader made
of each file - every point with its coordinates and metadata, every feature,
observation, setup and warning - is identical before and after, and so is
the whole reply of `SURVEY READ` and of `CRS SET EPSG:7856` then `SURVEY
IMPORT` and `LIST`: 3 482, 862 and 778 points at the same positions.

The third review's fixes (2026-09-30: the backsight is the first 04 that can
orient its setup, a 20 or an offset by an ID a shot and a coordinate share,
a record whose name is another point than 44.5 finds, 16, 20 and the offsets
with a trailing tab) change nothing in the three files either - still no
setup with more than one 04, no 03, 04 or 06 with both a name and an ID, no
20 or offset with a description, and no 16 with a trailing tab, counted on
the copies the owner saved again that day. The whole reply of `SURVEY READ`
and of `CRS SET EPSG:7856`, `SURVEY IMPORT` and `LIST` is the same before
(main's build) and after but for the parser's version, now 1.2: 3 482, 862
and 778 points at the same positions. The version is what a saved job's
report carries to say which reader made it (`FormatDescriptor::parserVersion`),
and these fixes and the second review's change what some files import.

The fixtures are hand-built in the files' layouts, with invented names and
numbers: `tests/surveyio/data/fld/setup.fld` (a setup),
`tests/surveyio/data/fld/gnss.fld` (an RTK job: blank-padded opcodes, time
stamps, each coordinate with its receiver's solution in a group, a keyed-in
coordinate, a close, a mark measured again, a Windows-1252 degree sign),
`tests/surveyio/data/fld/resection.fld` (a resection and its residual
comments in UTF-8, 16, 42, 43, 71, a coded check) and
`tests/surveyio/data/fld/rtk_setup.fld` (a setup on one RTK mark backsighting
another, and two shots with an offset each) and
`tests/surveyio/data/fld/two_backsights.fld` (two setups, each backsighting
an entered mark and a point nothing places, one in each order).
`.gitattributes` stores them byte
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
step (`cad::ImportSurveyJobCommand`); `SETTINGS` and `SET` give it the
wizard's other reduction options and control from the drawing ("The
reduction settings of SURVEY IMPORT", below). The format is detected unless FORMAT
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
`cli.survey_import_rtk_setup_field_file`,
`cli.survey_read_two_backsights_field_file`,
`cli.survey_import_two_backsights_field_file`, `qt_survey_verb_headless`, an agent
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
- A 04 to another point than the setup's backsight does not re-orient the
  measurements after it, as a bearing datum difference of each 04 would, if
  that is what the description means: the model orients a setup once (The
  backsight, above). Two backsights that disagree - a circle reset between
  them - show as a misclosure where the one read as a measurement is to a
  point with a position. Where it is to a point with none, only the reader's
  warning says so, and an azimuth it gives is kept in the setup's metadata
  that nothing checks against the orientation. A point an earlier setup
  measured counts as having a position whether or not that setup is placed
  and oriented - a resected setup's shots have none until the reduction
  computes resections - so a first backsight to such a point is taken over a
  later one to control, and the reduction, finding it unplaced, takes its
  circle as set to the grid.
- A 16's point name and point ID are not searched for: the description adds
  the name to the named points 44.5 searches and records the ID as the point
  ID of its vertex, which 44.5 and the entries for 20 and 42 to 44 search,
  and this reader makes no point of a 16 (it strings the current point into
  the 16's string, above). So a setup, backsight or check that names either
  stands on or measures a point of that name with no position, and a 20 or an
  offset that names the ID finds no point by it.
- A mark first given an entered coordinate with no height, and then a GNSS
  position with one, holds the entered coordinate - the reduction holds
  entered control over a GNSS position - and so has no height; the GNSS
  position's is not used for it.
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
  unless its line sets them (`SET grid_scale=projection`) or a person does in
  Survey > Survey Jobs, so a total-station job imported into a projected
  drawing with the defaults is radiated with ground distances.
- surveyio has eight file-local digit tests (`isDigit` in leica_dbx.cpp,
  leica_gsi.cpp, rinex.cpp, rinex_common.cpp, rinex_compact.cpp,
  topcon_raw_builder.cpp, opcode_field_file.cpp and sokkia_sdr.cpp), and
  `include/katana/core/text.hpp`, which would be their one home, has none.

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
the format, from Trimble's published "SDR33 Observations.xsl" (and the
JobXML schema, 5.72, of the controller field book it writes from), the SDR
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
- **Where a set's `12` stands depends on the layout, and in SDR2x the set's
  `07 SC` decides.** Chapter 2: "The standard SDR33 format [writes] a SET
  record before the raw observations"; from V04-02 on, a job with 4-digit
  point numbers - the SDR2x layout - writes it after them, before the MC
  records ("the SET record precedes the MC records for SDR2x format
  compatibility"), and LISCAD reads that order (a set begun at the field
  book's "13SC Set #" note, completed by its `12SC`). V04-01 wrote the SDR2x
  `12` first, as SDR33 does (chapter 2's V04-01 example: SET, the raw
  observations, a note, the MC records, the BKB SC), and the header cannot
  tell the two apart: Sokkia's 3.2.5 and chapter 5 send every 4-digit job
  as "SDR20 V03-05". So the `07 SC` that closes the set decides: raw
  observations read after the `12` are its set (it opened the set); none -
  only MC records, or nothing - and the set was the raw observations before
  the `12`, those since the setup began, its last `07` or `12`, or a "Set #"
  note, the last of them as many as the `12`'s count where it states one.
  Rejected: taking every `12` as opening its set, the reader's first rule,
  which read chapter 2's garbled sentence as "an SDR2x set collection from
  V04-02 on stores only MC records" - its V04-03 paragraph says the SET
  record precedes the MC records - and, where the circle moved between sets,
  left the second set on the first's orientation, its directions off by
  half the move with only a misleading warning; and deciding at the `12`,
  the second review's rule, which took a side shot before a V04-01 set for
  the set, turned it by the circle's move (76.5 m at 100 m for 45 degrees)
  and said the `07 SC` replaced a `07` "since no observation came between",
  where one had. A `12` whose count exceeds the records before it cannot be
  closing them, but a count cannot settle the rest (a `12` states none, or a
  V04-02 set's record was deleted), and what follows the `12` does.
- **A setup's first `07` orients shots before it only when they read its
  backsight on its circle.** The circle reading the backsight there, every
  time, within a minute of the `07`'s circle reading (a face 2 reading less
  half a circle; with no circle stated, any shot of the backsight) shows the
  circle was not set anew at the `07`, so those shots were read on the
  orientation it gives. Otherwise the `07` orients what follows it (SETX
  8.2): the shots before it were taken on no orientation it gives, so they
  stay a setup of their own - oriented by a keyed azimuth, or read as
  azimuths (below) - and the `07` begins a new setup on the point, with a
  warning that says which: none is of its backsight, they read it on
  another circle, or none gives a horizontal reading of it. The same holds
  for the shots before a set that a first `07 SC` closes. No writer known
  puts the shots first: Trimble's JobXML schema has its BackBearingRecord
  apply "to the following observations", and its writer keeps the field
  book's order, as the owner's file does - each of its 246 back-bearing
  records comes before the round it orients, and 245 hold that round's
  face-mean reading on the backsight (to 0.05"; the other is 0.3" off it),
  where 195 come straight after the backsight shot that ended the round
  before. The rule stands on the readings, not on a writer's order. The
  minute is widened by the horizontal collimation applied to the readings:
  a `07`'s circle may be Trimble's writer's raw face 1 reading, from which a
  corrected reading differs by the correction, or, in Sokkia's own, the
  set's face mean (chapter 2's BKB SC), which the corrected readings match -
  so a 72" collimation had split a setup read on one circle (a reviewer's
  case), where 45 degrees still shows. Rejected: comparing the uncorrected
  readings, which only moves the false split to the face-mean circle.
  Rejected: orienting
  the whole setup, the reader's first rule, which turned such shots by the
  `07`'s circle without a word and let a later `07` override a keyed
  azimuth for the shots it had oriented; any shot of the backsight before
  the `07`, the second review's rule, justified by a claim that Trimble's
  writer puts the backsight's shot first (its schema says otherwise), which
  meaned a shot read on another circle into the orientation and rotated the
  setup's other shots without a word (45 degrees, 76.5 m at 100 m, in a
  reviewer's case); and splitting at every first `07` after shots, which
  would cut a backsight shot read on the `07`'s circle from it.
- **A later `07` on a moved circle takes the backsight shots just before
  it.** Where a `07` naming the setup's backsight begins a new setup (its
  circle moved), the shots of that backsight read just before it - since the
  setup's last `07`, `12` or "Set #" note - on its circle and not on the
  setup's go with it: the same point read on the new circle was read after
  the circle moved. Left in the setup before, one such shot meaned into its
  orientation and moved its other shots by half the move, 39 m at 100 m in
  a reviewer's case of a 45-degree move. A face 2 reading is compared less
  half a circle, so a round ending on face 2 of the backsight stays with
  its own circle even when the next is moved half a circle. Where the
  setup's `07` gives no circle reading, its first reading of the backsight
  stands for its circle; where the new `07` gives none, a shot of the
  backsight off the setup's circle is enough (the fourth review found the
  move did nothing when either `07` lacked a circle, leaving C 39 m off in
  a reviewer's case). Rounds whose `07`s give no circle are one setup with
  nothing to compare, so their readings of the backsight are compared face
  by face (face 2 against face 1 would take an instrument's collimation for
  a move), and a spread past the minute is warned about at the reading.
  Any other shot stays with the orientation it followed (SETX 8.2).
  Rejected: the same for a `07` naming another backsight, its reading
  predicted from the two records' azimuths - a point other than the
  backsight read on the new circle shows only that the azimuths disagree,
  not that the circle moved.
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
  to azimuths and says so. A setup a `07` splits is judged by the
  observations it keeps: shots before a set whose readings of the keyed
  point leave with the set have nothing to orient them by, and the read says
  so at the `11` (it had judged the setup before the set left, and said
  nothing while the reduction dropped the shots). Rejected: leaving such
  setups unoriented, which drops every shot of Sokkia's own example.
- **A bad set is as long as its count.** A `12` whose bad marker is 2 is
  not used, and its raw observations are skipped - as many as the record's
  "count of observations" (chapter 2's example counts 4 for its four `09`
  records), or, where it states none, up to the next set or setup. Rejected:
  always up to the next set or setup, which took the shots after a bad set
  for part of it.
- **The collimation is applied, per face, within its job and instrument
  type.** A `04`'s corrections apply to every raw reading after it as SETX
  29.2.5 applies them: face 1 plus the vertical and horizontal correction,
  face 2 minus, a reading with no vertical angle taken as face 1 (SETX
  29.2.3). They apply until the next `04`, an `01` of another instrument
  type, or another job: SETX 13.1 applies a collimation "until either the
  instrument type is changed or a new collimation record is added", and
  "Collimation is not maintained across all jobs" (the Level 5 manual,
  chapter 13, says both). The instrument type is the `01`'s EDM type, the
  code Sokkia's 3.5 lists instruments by; the same type restated, as some
  files do before every setup, keeps the correction. Each end is warned
  about where it happens. An `01` of another type between two shots of one
  setup ends it there all the same - it is applied reading by reading -
  while the setup keeps its other settings, which the model holds once a
  setup; the warning about the change in the middle of the setup names the
  collimation as the exception (it had said the setup kept everything,
  beside the warning that the collimation ended). A face pair's mean is
  unchanged, so a round on both
  faces reduces as before and its faces now agree; a shot on one face moves
  (17.45 mm at 100 m for a 36" correction). The field book corrects the
  zenith after the instrument and target heights, the reader before them,
  which differs by at most the correction times the height difference over
  the distance (0.01" for 10" and 0.1 m at 100 m). The setup's metadata
  names the record applied. Rejected: keeping it in metadata only, the
  reader's first rule, whose reason - a face pair cancels it - holds only
  where every target is paired, not for the common single-face side shot; a
  collimation field in the model for the reduction to apply, which no other
  reader has a value for; and applying it "until the next `04`", the second
  review's rule, which carried one instrument's correction into another's
  single-face shots, or another job's (17.45 mm at 100 m for 36").
- **Whether the distances carry the atmospheric correction is Unknown.**
  Sokkia's field book applies it as each distance is accepted when the job's
  switch is on (the Level 5 manual, appendix B); Trimble's writer turns the
  switch on whenever it writes weather and writes distances without it. The
  reduction then applies none and says so, and Recompute or Fixed applies
  one from the recorded weather (mmHg and inHg converted by the conventional
  values of NIST SP 811). Where the file's time stamps are in Trimble's
  writer's form ("Time Date MM/DD/YYYY"), what the file did not carry says so
  and that Recompute then applies the correction; the owner's file is such a
  file, and its notes and job flags are in that writer's form too - though
  not all its records are: its back-bearing records hold the face-mean
  reading on the backsight of the round after them, where the 2022 writer
  writes the controller's face 1 reading, so another version of it, or
  another Trimble program, wrote the file. Rejected: Applied
  (Sokkia's rule) or NotApplied (Trimble's), either silently wrong for files
  from the other writer - the time stamps point to a writer, they do not
  prove it. What it is worth on the owner's file: from its weather (981.4 to
  997.7 hPa, 9.9 to 17.0 C, 60 % humidity assumed) the IUGG 1999 group
  refractivity at the reduction's 658 nm gives 4.3 to 12.6 ppm across its 41
  setups, worked independently of Katana's code. Reduced with its first
  station held at an assumed position (N 6 250 000, E 300 000), the far end
  of the traverse moves 16.4 mm between the default and Recompute (another
  station 16.1 mm; measured with the second review's build) - so a person
  whose distances are raw must choose Recompute.
- **The prism constant is in the distances**, the instrument record's value
  in millimetres: the field book applies it on acceptance, and Trimble's
  writer adds the target's constant to each distance and writes 0 there.
  Trimble's writer puts the instrument's serial number in the EDM
  description field; it is kept as that field's text (`instrument: EDM`).
- **Option 45 is the coordinate order, and options 2 and 3 are each warned
  about.** Sokkia's 3.5 gives 1 "N-E-Elev" and 2 "E-N-Elev"; 3.6.2 names
  21-36 the northing and 37-52 the easting, which is option 1's order, and
  SETX 3.5.6 calls the setting "the order in which they are displayed".
  Under 2 the easting is read first, as Trimble's writer writes it, with one
  warning a file, at the first record that states coordinates: no source
  shows the order Sokkia's field book sends under 2. Its own E-N-Elev
  example (chapter 2, V04-04.30) is a printed report - 2.4: "The following
  example SDR files are displayed in Printed output form" - so it shows the
  display order, not the file's; its HORZADJ record, printed with fixed
  labels ("Trans.N", "Trans.E"), only identifies which printed value is the
  easting (the translation printed as "Trans.N" takes its GSTN 1005 to 1.1 mm
  east and 0.8 mm south of its printed POS 1005; the other reading is 5.6 cm
  off in each ordinate, 7.9 cm in all - the third review's text had 0.1 mm
  and 5.5 cm), which a printer that orders values by the option without
  relabelling explains as well as a file stored east first. Both of chapter 4's
  transmitted samples are option 1. Trimble's 3, "Y-X-Z", is east first too
  and read so, with its own warning: Sokkia does not define 3, and SETX 3.5.6
  lists south-west-elevation as the field book's third display order. The
  order read is in the project's metadata (`header: coordinate order`). A
  file that states coordinates under another option is refused. Rejected:
  no warning under 2, the second review's rule, on the printed example's
  evidence, which cannot show a file's order - a Sokkia file under 2 that
  holds the northing first would have every coordinate transposed without a
  word; north first (every Trimble-written file's control transposed);
  telling north from east by magnitude (a guess).
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
- **A point's latest coordinates are its own - but never an observation's
  POS view over a position record.** Coordinates keyed in (KI) are Entered,
  an `08 TP` FieldObserved, `08 AJ`, `TV` and `RS` Calculated, any other Unknown; a
  point given coordinates more than once keeps the latest, the field book's
  own rule. Sokkia's 2.3 in full: "The rule is essentially that the latest
  coordinates are the best(or an observation in POS view) will over-ride an
  observation in OBS view even if the OBS view is stored later"; SETX 6.1
  rule 2 uses "POS, STN, and Pos view records before using OBS records even
  if the OBS record is more recent", and ProLINK 6.1.3.2 searches from the
  end of the field book for a point's POS, STN or POS-view record. A
  traverse adjustment's `08 AJ` follows the positions it adjusts, and a later
  `02` restates the station as the field book held it then. The earlier
  coordinates stay in the point's metadata, each change warned about; the
  same coordinates again say nothing. The shared builder gained the choice
  (`RawProjectBuilder::RestatedCoordinates`); the RW5, GTS-7 and field-file
  readers keep the first, their formats' later positions being checks.
  The exception is the second clause. A field book sending more than one
  view writes "more than one record for each observation record", one after
  another - for its current view and the POS view, "a raw observation record
  followed by a position record" (SETX 27.2) - and an MC or RED record has
  the views a raw one has (SETX chapter 6: "MC and Red records can also be
  stored in Pos view"; SETX 8.5.5's printed examples follow each OBS record,
  the averaged OBS MC among them, with its POS TP). So an `08` other than
  KI, AJ, TV or RS read straight after a `09` F1, F2, MD or MC, or an `11`
  with distances, of the same point is that observation's POS view,
  whatever the reader then does with the observation (an MC or RED is not
  imported, a bad set's raw one is skipped), and the observation stayed in
  its own view: one in OBS, MC or RED view comes after every POS, STN and
  POS-view record (SETX 6.1 rules 2 and 3), and Store OBS "will NOT
  overwrite a previous coordinate if it exists in POS view" (SETX 8.5.2) - a
  check shot onto control. It places a point with no coordinates, or one
  whose coordinates came only from such views (the latest observation counts
  where nothing else does, SETX 6.1 rule 3); it never supersedes the
  coordinates of any other record, which stand, its own going to the point's
  metadata with a warning naming the observation and its view
  (`positionPoint`'s `asideBecause`). Straight after is what pairs them: an
  `08` after an `08` of the same point is a record of its own - SETX 8.5.5
  prints the new averaged position straight after the POS view it averages -
  and so is one with a note or any other record between it and the shot. An
  MC or RED whose observation is lost (below) may still place its point by
  its POS view: the observation is lost to the reduction, the field book's
  position from it is not. Rejected: the latest whatever the record, the
  second review's rule, which let a check shot's POS view move keyed control
  (13 mm in a reviewer's case, the misclose gone), demote it from Entered so
  a later `02` restating it moved the shots from it too, and replace a
  backsight's keyed coordinates; a POS view only straight after a raw `09`,
  the third review's rule, which let the POS view of an MC or RED - a set's
  averaged MC, or a shot stored in MC or RED view - move keyed control by
  the same 13 mm, beside warnings that set the raw shots' POS views aside;
  and keeping the first, the reader's first rule, which imported chapter
  2's traverse with its stations unadjusted beside side shots computed from
  the adjusted ones.
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
  warning and in what the file did not carry - its observation, that is: a
  POS view sent beside it still places its point (above). The twin may come
  before the derived view or after it: the verdict on one whose line has no
  raw observation yet waits for its setup's next `02` or the file's end, and
  "its setup" is all that `02`'s observations, from its point, whatever
  setup a re-orienting `07` put them in - and no earlier `02`'s, whose
  records' views were sent beside them. Rejected: importing them on one
  writer's meaning, which would put another writer's points in the wrong
  place without a word; deciding by whether the setup held any raw
  observation, the rule after the first review, which called an MC of an
  unobserved target "counted twice" and left its shot out of the count of
  lost ones; and deciding as each is read, the second review's rule, which
  called an MC read before its raw twin lost - the same shot counted as
  lost or not by the order of two records.
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

Read again after the third review's changes (2026-09-30), the owner's file
gives the same answer - `read=4206 skipped=14 warnings=14`, `setups=41
observations=6372 unpositioned=42` - and a full dump of the read project,
every value at 17 significant digits, is byte for byte the one before (6 621
lines). None of the changed rules has anything to act on there: each of its
246 back-bearing records comes before its round, its six rounds a setup on
one circle; it has no `04`, `12`, MC or RED with distances, no `08` after a
shot, and its one `08` states no coordinates for the option-2 warning to be
given at. After the fourth review's changes (the same day) it reads and
imports as main's reader did, `SURVEY IMPORT`'s 47 reduction warnings included, and the
full dump is again byte for byte the same: every one of its back-bearing
records gives a circle reading (so none is inferred, and no round goes
unchecked), it has no collimation to widen a circle test, its 42 instrument
records name one EDM type, and its one `08` follows no observation of its
point.

Tests: `tests/surveyio/test_sokkia_sdr.cpp`, on the hand-built
`tests/surveyio/data/sdr/traverse.sdr` (its second setup's circle zeroed on
a backsight whose azimuth is 270) and records written in the test - among
them Sokkia's own chapter 2 examples: the OBS-view job reduced to its
printed positions, the traverse's `POS AJ` records kept over the positions
before them, and its `BKB TP 0003-0002` (azimuth 269-59'50", circle
270-00'00") orienting a shot less the circle, and less a reading 5" off it;
the SDR2x set layout with the circle moved between sets, and with the `12`
first as V04-01 wrote it; each order of shots and first backsight record,
the backsight read before a first `07` on its circle, on another, on face
2, with no horizontal reading, and 72" off it under a 72" collimation, and
a round's backsight shot before a `07` on a moved circle, with a circle in
both `07`s or in only one; rounds whose `07`s give no circle, their
readings of the backsight spread or not; the collimation on each face, and
its end at another instrument type (between setups and in the middle of
one) and another job; a shot's POS view beside control, beside nothing,
twice, and a lone `08 TP`; the POS views of a set's raw shots and its
averaged MC, of an MC alone, of a RED, and of a raw shot sent with its MC;
an `08` that is no view - a note between it and the shot, or SETX 8.5.5's
averaged position after a view; derived views read before their raw twins,
across a re-orientation and past the next `02`, and one after a second `02`
whose line only the first observed. Each test of the third and fourth
reviews was seen to fail with its fix taken out.
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
- The RW5 reader keeps its file's backsight azimuth in metadata
  (`backsight azimuth (radians)`), the GTS-7 reader its bearing (`backsight
  bearing (radians)`), and the JobXML reader only the controller's
  orientation correction (`jxl.orientationCorrection_deg`) - none in
  `statedBacksightAzimuth`, so a setup of theirs whose backsight has no
  coordinates is still oriented on its circle. Filling the field there
  changes those readers' reductions and needs their own formats' checks.
  The field-file reader fills it, as this one does (its `04`'s azimuth; see
  its section).
- The person cannot yet state the coordinate order of an option-2 or
  option-3 file (the read options carry no format-specific setting), so a
  Sokkia file under 2 that holds the northing first is read transposed,
  with the warning.
- Two of the field book's outputs hold an observation stored in its own
  view and one stored in POS view alike. Sent with the POS view alone ("all
  the observation (OBS or MC) and reduced (RED) records are output as POS
  records", SETX 27.2), each is an `08 TP` with no observation record before
  it, taken as a position record: a check shot onto control there replaces
  the control's coordinates (warned, the earlier ones in the point's
  metadata), which the field book itself would not. Sent with an
  observation view and the POS view both on, rather than the current view,
  each is its `09` (or `11`) and its `08`, taken as a check: a shot stored
  in POS view, which the field book lets overwrite, leaves the point's
  earlier coordinates standing (warned, its own in the metadata) - the
  safer of the two mistakes, since control never moves without a word.
- A first `07` that gives no circle reading orients the shots before it
  where any is of its backsight, as the rule was: with no circle there is
  nothing to compare their readings with. Rounds whose `07`s give no circle
  are one setup whatever their circles; a spread in their readings of the
  backsight is warned about, not split, since which round moved is not
  said.
- The latest coordinates are taken record by record. An `08` restating a
  point's northing and easting with no elevation leaves it none, where one
  with the same northing and easting keeps the earlier elevation (the
  builder takes that as a repeat); and an `08` giving an elevation alone for
  a point already placed keeps it in the point's metadata rather than
  replacing the point's height. SETX 6.1 takes a point's coordinates from
  its latest POS or STN record as a whole ("the coordinates are immediately
  available from that record"), which supports the first, but no source
  says whether a blank elevation there means "no height" or "the height
  unchanged", and no file met has either case.
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
  and `survey/angles.hpp` has no mil conversion to share; lifting one there
  is a change to the GSI reader as well, left for one that edits it. The
  reader's calendar is the
  standard library's (`std::chrono::year_month_day`, its month and day
  ranges checked first, since those types keep only a byte); the RINEX
  reader (`isLeapYear`, `daysInMonth`) and the subsurface delivery schema
  still write the leap-year rule out themselves.

## The reduction settings of SURVEY IMPORT, control from the drawing among them

The owner's Sokkia traverse (above) gives no coordinates, so `SURVEY IMPORT`
placed nothing from `katana_cli` or `katana_mcp`, while the wizard's
Reduction step could hold a point of the drawing. Since 2026-09-30 the verb
takes every reduction option that step offers:

```
SURVEY IMPORT <file> [FORMAT <id>] [LAYER <path>] [SETTINGS <file>] [SET <key>=<value> ...]
```

**One grammar, the job's own.** Both options are the stable text form of the
settings (`include/katana/survey/reduction_settings.hpp`) that a survey job
keeps in the project: `SETTINGS` names a file of it, and each `SET` item is
one line of it. Both are read by `survey::parseReductionSettings`, which
checks what it read with `survey::validateReductionSettings`, and a refusal
is in their words. `src/katana_app/survey_verbs.cpp` spells nothing of the
form: its first line and its keys are taken from what
`survey::serialiseReductionSettings` writes, so a setting added there is one
`SET` takes at once. A job's settings written out are a `SETTINGS` file that
imports the same way again
(`SurveyImportSettings.AJobsSettingsWrittenOutImportTheSameWayAgain`).

**Where the settings start, and what changes them.**

- Without `SETTINGS` the import starts where the wizard's step does: the
  defaults, holding the control the file declares (`survey::controlFromFile`).
- `SETTINGS <file>` replaces that start whole, as a job's stored settings are
  whole: a key it does not give keeps its default, and its control lines are
  all the control there is. A file with none holds nothing, not even what the
  survey file declares.
- `SET` then changes the keys its items name. An item
  `control=<id>;<file|drawing>;` followed by a constraint (`fixed`,
  `weighted` or `free`) and a standard deviation in metres for the northing,
  the easting and the elevation holds that point as the wizard's Hold button
  does: in place of the point of that id, or after the others.
- The items run to the next option word or the end of the line; one holding a
  blank is quoted, as a path is. Each option is given once: a second `FORMAT`
  or `LAYER` was taken over the first without a word.

The keys, in the text form's order (the refusal of a key the settings do not
have lists them as the build has them):

| Keys | Values |
|------|--------|
| `atmospheric` | `none`, `auto`, `recompute`, `fixed` |
| `prism_constant.policy` | `auto`, `override`, `none` |
| `faces` | `average`, `face-left-only`, `separate` |
| `height_reduction` | `none`, `ellipsoid`, `geoid` |
| `grid_scale` | `none`, `fixed`, `projection` |
| `adjustment.method` | `none`, `traverse`, `network` |
| `adjustment.traverse_rule` | `bowditch`, `transit`, `least-squares` |
| `adjustment.network` | `horizontal`, `levels`, `horizontal-and-levels` |
| `outliers.test` | `none`, `baarda`, `tau` |
| `atmospheric.fixed_ppm`, `prism_constant.m`, `faces.tolerance.horizontal_rad`, `faces.tolerance.zenith_rad`, `faces.tolerance.distance_m`, `refraction.k`, `earth_radius_m`, `grid_scale.fixed_factor`, `combined_factor.value`, `apriori.direction_rad`, `apriori.zenith_rad`, `apriori.distance_constant_m`, `apriori.distance_ppm`, `apriori.instrument_centring_m`, `apriori.target_centring_m`, `apriori.height_m`, `apriori.levelling_per_sqrt_km_m`, `apriori.gnss_horizontal_m`, `apriori.gnss_vertical_m` | a number, in metres, radians and parts per million as the settings hold them - the wizard shows seconds of arc and millimetres |
| `confidence_level`, `outliers.significance` | a fraction strictly between 0 and 1 (`0.95`, `0.001`) - the wizard shows the confidence level as a percentage |
| `curvature_refraction`, `faces.tolerance.exclude`, `slope_to_horizontal`, `combined_factor.use`, `apriori.use_file_covariances`, `outliers.auto_reject` | `true`, `false` |
| `iterations.max` | a whole number |
| `control` | as above; an id holding `;`, `=` or `%` is percent-encoded |

**Holding a point of the drawing.** The point is one of the drawing's survey
points - a point entity with the import's point-number property - found as
the wizard finds it (`cad::reductionContextFor`), and it is the drawing's:
not drawn again. On a line, `FORWARD` puts a named one there:
`FORWARD 300000,6249999,50 0 1 0 BM1` makes BM1 at 300 000 E, 6 250 000 N,
50 Z, one metre due north of the start and level. Then
`SURVEY IMPORT <file> SET control=BM1;drawing;fixed;0;fixed;0;fixed;0`.
Under the default radiation (`adjustment.method=none`) a held point places
the setups on it, and what they observe, whatever its constraints - `free`
does not release it - since the constraints and standard deviations are
what a traverse or a network adjustment weighs, and radiation weighs
nothing.

The import reduces with `cad::reduceForDrawing`, as the wizard's preview and
Import and the Survey Jobs dialog do, which checks what the settings hold
from the drawing before the reduction takes it:

- An id the drawing has at two places is refused (InvalidArgument), naming
  each place in the drawing's order, to a tenth of a millimetre: "Control
  point CP1 is on the drawing 2 times, at E 500000.0000 N 5000000.0000 Z
  100.0000 and E 600000.0000 N 5000000.0000 Z 100.0000, so which one to hold
  is not known; rename or delete all but one." The reduction holds the
  first point of an id, and which is first is only the order the points
  were drawn in: `FORWARD` twice, or the wizard's "keep both", gives a
  drawing two CP1s, and the job sat on whichever came first - 100 km out in
  the review's case - with nothing in the reply to say so. Points of one id
  closer than `math::tolerance::kCoordinate` (0.1 mm) across and in height
  are one mark, and hold the same job whichever is taken, so they are held;
  one with a height and one without are two.
- An id the file never names changes nothing, and the first reduction
  warning says so: "Control point BM1 is held from the drawing, but the file
  names no point BM1, so holding it changes nothing." A file read by
  surveyio lists every point a setup, an observation or a feature refers to
  among its points or its points without coordinates
  (`survey::validateProject`), so an id in neither is one nothing in it
  measures.
- An id the file gives other coordinates is held where the drawing has it,
  and the first warning gives both: "Control point CP1 is held where the
  drawing has it, E 600000.0000 N 6000000.0000 Z 200.0000, not where the
  file gives it, E 500000.0000 N 5000000.0000 Z 100.0000."

**The reply** gains, before `imported`:

```
settings file=held.txt set=1 differ=2
setting key=faces value=separate
setting key=control value=BM1;drawing;fixed;0;fixed;0;fixed;0
settings_warning text="line 8: setting 'later.setting' is not known to this version and was ignored"
```

`file=` names the `SETTINGS` file, empty without one, and `set=` counts the
items. A `setting` record is each line of the settings' text that is not the
defaults', in the text form's order and words, so each reads back as a `SET`
item; a `settings_warning` is each key of the `SETTINGS` file this version
skipped, and the encoding it was read in where that was a guess.

After `imported`, where each point was held and what the adjustment made of
the data (`katana::app::reductionRecords`):

```
held id=BM1 from=drawing entity=1 northing=6250000 easting=3e+05 height=50
held id=A from=file northing=1000 easting=1000 height=50
held id=B from=file northing=1100 easting=1000 height=50
reduction method="network, horizontal" adjustments=1 rejected=0
adjustment method="network least squares (horizontal)" observations=10 unknowns=4 redundancy=6 variance_factor=0.1697573300133813 global_test=failed flagged=0 rejected=0
```

(the last four from `tests/qt_widgets/survey/data/network_gsi8.gsi` with
`SET adjustment.method=network` and A and B held in plan: the network takes
a setup's directions as angles from its backsight, so its two setups give 4
angles and 6 distances and it has P and Q to find; a variance factor of 0.17
is smaller than the default a-priori precisions expect, which the two-sided
global test calls a failure too)

A `held` record is each point the settings hold: at the drawing's point of
its id, with its entity - the first of the id, as the reduction takes it -
or at the file's own coordinates (`from=file`); `height=none` for a point
with no height. `reduction` gives the settings' method in words, how many
adjustments ran and how many observations were rejected, and an
`adjustment` record is each adjustment run, as its report has it: its
observations, unknowns and redundancy, the a-posteriori variance factor and
the chi-square global test (`none` where it had no redundancy), and how many
outliers were flagged and rejected. A number is the shortest text that reads
back exactly, as every record writes one, so 300 000 is `3e+05`. Until
2026-09-30 the reply said nothing of the adjustment, so an agent could run a
network whose global test failed and not be told: the wizard's words for the
outcome (`adjustmentSummary`), its rejected count (`rejectedObservations`)
and the method's words (`methodText`) moved from the window's code
(`src/katana_qt/survey/survey_job_support.cpp`) to
`include/katana/survey/report_summary.hpp`, so the reply's `rejected=` is
the count the wizard's message gives, taken one way.

**Refused**, each with the drawing as it was
(`SurveyImportSettings.EveryRefusalSaysWhyAndLeavesTheDrawingAsItWas`): a
line with `SET` and no item, an option given twice, `SET` or `SETTINGS` on
`SURVEY READ`; an item with no `=`, a note (`#`), a line break, a key the
settings do not have, a value that does not read or lies outside its range,
a key or a control point given twice; a `SETTINGS` file that cannot be read,
is not a settings text or was written by a newer version (Unsupported); an
id held from the drawing that the drawing has at two places; and the
reduction's own refusals of a control point that is not on the drawing,
that the file gives no coordinates for, or that is held in height and has
none. The settings are refused before the survey file is read: a line
naming a survey file that is not there is refused for its settings first.

**Decisions.**

- An unknown key is refused in `SET` and skipped in a `SETTINGS` file. The
  text form skips a key it does not know because a later Katana may have
  written it; an item typed on the line now is a misspelling, and a setting
  silently not made is the worst answer an import can give.
- No shorthand for control, such as `HOLD BM1 FROM DRAWING`: it would need
  defaults of its own for the three constraints and standard deviations a
  control line states, so it could not map one to one onto a control line,
  and the line is one token an agent writes as easily.
- `SET control=` merges by id rather than replacing the list, as the
  wizard's Hold does, so holding a drawing point keeps what the file
  declares; the whole list is `SETTINGS`' to give. Replacing the list would
  drop the file's control whenever a point was added, which the wizard never
  does.
- Each item is read alone, as a settings text of the version line and the
  item, so a malformed one is refused naming "line 2 of the reduction
  settings" - the parser's own words - with the item in the refusal's
  context: `[SET refraction.k=abc]`.
- The settings are read before the survey file, so a mistyped key costs no
  read of a large file.
- A `SETTINGS` file is decoded as every text file Katana reads is
  (`core::decodeText`): saved with a byte order mark, or as UTF-16 by an
  editor, it is the same settings, where the parser alone called the text
  not settings at all. A file that is not UTF-8 is read as Windows-1252 and a
  `settings_warning` says the guess.
- An id the drawing has at two places is refused, not held at the first
  with a warning: a warning an agent may pass over leaves the job 100 km
  out, while the refusal names both places, so the person renames or
  deletes one. A control line names an id, not an entity, so the wizard -
  whose pick box lists the id twice - cannot say which was meant either;
  holding an entity would change the settings' stored text form.
- The checks are `cad::reduceForDrawing`'s, made before the reduction runs
  and not in it: whether the drawing has an id twice is the drawing's to
  know, and every path that reduces a job against the drawing - the import
  and re-adjustment commands' default, the wizard's and the Survey Jobs
  dialog's previews, `precomputedReduction`'s fall-back - goes through that
  one function, so the window and the line refuse and warn alike
  (`SurveyImportWizard.ItsPreviewRefusesToHoldAPointTheDrawingHasAtTwoPlaces`,
  `SurveyJobsDialog.ItsPreviewRefusesToHoldAPointTheDrawingHasAtTwoPlaces`).
  `survey::reduceAndAdjust` keeps its rule of the first of an id for a
  context handed to it directly.
- An id the file never names is a warning, not a refusal: a `SETTINGS` file
  may list a site's whole control, of which one job's file names some.
- An id the file gives other coordinates is a warning, not a refusal:
  holding a drawing point the file also coordinates is how a job keyed in a
  local frame is put on the drawing's. What is wrong is a job that mixes the
  two - held at the drawing's CP1 and oriented on the file's CP2 - which the
  backsight check should catch (Not done, below).

**The owner's file.** The Sokkia traverse above, through `katana_cli` on
2026-09-30 (a local check; the file is not in the repository): its first
setup's station put on the drawing with `FORWARD` at invented coordinates
(6 250 000 N, 300 000 E, 50 Z) and held fixed in all three components by
`SET`, `SURVEY IMPORT` places 41 points - every point the file names but the
held one - with 6 reduction warnings: the three face pairs outside the
default tolerance, the humidity and the atmospheric correction, and the
first setup oriented on the azimuth the file states. Before: none, with 47
warnings, and `SET` refused as no option of the verb. The reply says where
the station was held (`held ... from=drawing entity=1 northing=6250000
easting=3e+05 height=50`) and that nothing was adjusted or rejected. The
window run headless (`--dialog surveyImport`, the point picked among the
drawing's) imports the same 41 points, `LIST` line for `LIST` line, entity
ids and all, and logs the same counts; so does the line typed on its
command line after an `UNDO`. Adjusted as a network in plan and in height,
face left only, the station weighted (5 mm, 5 mm, 10 mm), the reply gives
what the wizard's message gives: variance factors 0.321 in plan and 0.571
in height, both global tests failed, 3 height differences flagged, and
3 186 observations rejected - the 1 062 face-right pointings, three values
each, that face left only leaves unused. Before, it said nothing of the
adjustment. With the station drawn twice, 10 m apart, the line and the
wizard's preview refuse alike, naming both places.

**A held point is not a point without coordinates.** Found on the way: the
reduction places a point held from the drawing as control, not as a
computed point, so a file that gives it no coordinates leaves it among the
unpositioned ones, and the import counted it in "N point(s) are named in the
source with no coordinates ... reducing the observations is what gives them
a position" - wrong for a point that has one, and said by the wizard too.
`drawableProject` (`src/katana_cad/survey_job.cpp`) now leaves the drawing's
control out of that list as it leaves it out of the points drawn.
`SurveyJobImportCommand.AControlPointTakenFromTheDrawingIsNotCountedAsHavingNoPosition`
failed ("2 point(s)") without the change.

**The wizard does not hand the line over.** A dialog's work should be the
line it builds, run by the window's executor (`MainWindow::runVerbLine`) as
if typed. The wizard's Import still runs `cad::ImportSurveyJobCommand`
itself, because:

1. Import reuses the reduction its preview ran (`precomputedReduction`,
   `src/katana_qt/survey/survey_job_support.hpp`), which for a job of more
   than 20 000 observations and points (`kBackgroundObservations`) ran on a
   pool thread with a busy bar and Cancel, and the wizard reads a file of
   more than 1 MiB (`kBackgroundReadBytes`) off the GUI thread too. A line
   the executor runs is synchronous and cannot be handed an outcome: the
   verb would read the file and reduce it a second time on the GUI thread,
   and a large job would freeze the window where it now shows its progress.
2. The line reads the file from disk again, so a file changed after the
   preview would import other data than the report showed.
3. The wizard's control can drop a point the file declares (Release), which
   `SET` does not say. The line would need a `SETTINGS` file written
   somewhere to name, and a logged line naming a temporary file cannot be
   run again.

Handing it over needs the executor to run `SURVEY IMPORT` as a background
job, as the window runs the geoprocessing verbs
(`src/katana_qt/geo/geo_workbench.hpp`), and a way for the line to take the
outcome the preview made. Until then one mechanism holds the two together
below the line - the same command, the same `cad::reductionContextFor`, the
same settings value - and
`SurveyImportWizard.ItsImportIsTheSurveyImportLinesForTheSameSettings`
imports the file with no coordinates both ways, CP1 held from the drawing
and two settings changed from the defaults, and requires the same job - its
report but for its time - and the same points on the drawing, bit for bit.

Tests: `cli.survey_import_holds_a_point_of_the_drawing` (the file alone
places nothing; held, its three points where they are worked by hand beside
the test), `cli.survey_import_takes_its_settings_from_a_file`,
`cli.survey_import_set_changes_a_key_the_settings_file_leaves` (T1 without
curvature and refraction: 500000 + 150.0015 sin 87 E), eleven runs named
cli.survey_import_refuses_..., each of which must stop the batch before its
`STATUS` (the id the drawing has at two places among them),
`cli.survey_import_says_where_it_held_and_what_it_adjusted` and
`cli.survey_import_says_what_a_network_adjustment_made_of_the_data`; the
`SurveyImportSettings` cases in `tests/app/test_survey_verbs.cpp` (the
heights too, worked by hand to the micrometre; the start, `SETTINGS` taken
whole, the merge by id, the reply's order, the refusals, the settings
refused before a survey file that is not there; the id at two places and at
one mark, the `held` records, the warnings of an id the file never names or
gives other coordinates, the `reduction` and `adjustment` records from a
report built by hand and from the network fixture, held there to the
reduction's own report bit for bit, and a `SETTINGS` file with a byte order
mark, in UTF-16 and in Windows-1252);
`McpServer.AnAgentImportsAFileWithNoCoordinatesHoldingAPointOfTheDrawing`
and `McpServer.TheCommandToolNamesTheSurveyFieldFileVerbs`; the wizard's
equivalence above and its preview's refusal, and the Survey Jobs dialog's;
`SurveyJobDrawingReduction` and
`SurveyJobImportCommand.ItsReductionRefusesAnIdTheDrawingHasAtTwoPlaces`
beside the earlier regression in `tests/cad/test_survey_job.cpp`; and
`ReportSummary` in `tests/survey/test_report_summary.cpp`. The survey file is
`tests/surveyio/data/sdr/traverse_without_coordinates.sdr`, `traverse.sdr`
with its two 08 coordinate records taken out; the settings files are in
`tests/app/data/survey_settings/`.

Not done:

- The wizard's Import hands no line to the executor (above).
- The verb has neither of the wizard's two other import options: what to do
  with an id the drawing already has (`cad::ExistingPointPolicy`; the verb
  refuses the import, as the wizard's default does) and a layer per field
  code. Apply Survey Codes is the window's action; `CODE` does it on a line,
  to the whole drawing.
- The window can neither save the settings to a file nor load them from one,
  so a `SETTINGS` file is written by hand or by a program, in the form above.
- The wizard's Import logs a sentence, not the `SURVEY IMPORT` line it
  equals, so the window's import cannot be run again from the log. The
  settings' lines that are not the defaults' are the reply's `setting`
  records, which a line could carry, but for a point the file declares that
  Release dropped (above).
- The reduction does not warn when a setup's backsight disagrees with its
  coordinates. Held at the drawing's CP1 against a file that keys in CP1
  and CP2 in another frame, a setup is oriented from the one to the other -
  1 000 km away for a measured 100 m in the review's case - and the
  report's setups table has the backsight check
  (`ReportSetup::backsightDistanceDifference`), but no warning names it; the
  new warning of the held point's other coordinates is the only word. A
  backsight check outside a tolerance is the reduction's to warn of
  (`src/katana_survey/reduction.cpp`).
- `survey::rejectedObservations` counts an outlier an adjustment rejects
  twice: once as its rows, which the rejection marks, and once in the
  adjustment's list, so the wizard's "N observation(s) rejected" and the
  reply's `reduction ... rejected=` read 2 for one distance (a scratch copy
  of `network_gsi8.gsi` with one distance 0.3 m long and
  `outliers.auto_reject=true`: the adjustment's `rejected=1`, the
  reduction's `rejected=2`). Counting only the rows would lose an outlier
  that has none - a stated backsight azimuth - so the fix wants the report
  to say which rejected outliers marked rows, in the reduction's report.
- `survey::validateReductionSettings` takes any finite positive factor or
  radius and any prism constant, which `SET` now reaches in one token:
  `grid_scale=fixed grid_scale.fixed_factor=1e300` or `earth_radius_m=1e-300`
  puts points some 1e302 m out, `prism_constant.policy=override
  prism_constant.m=-1e6` 1 000 km, each with no warning. Bounds a surveyor
  would accept belong to that validation
  (`src/katana_survey/reduction_settings.cpp`).

## The reduction's resection

Added 2026-09-30, and reworked the same day after two reviews. A setup on a
point that nothing else positions - a free station, which a field controller
calls a resection (the opcode field file's 128 and 138) - used to be given up
("stands on ..., which has no position: nothing was computed from it"), and
everything measured from it with it: the owner's resection job has twelve such
setups, and they held 4 056 of its 4 834 points. The reduction now computes
such a station from the setup's reduced pointings to points already placed. It
looks at nothing but the reduced pointings, so it serves every format with
such a setup, not the field file alone: `resectSetup` in
`src/katana_survey/reduction_adjust.cpp`, tried from `placeSetups` in
`src/katana_survey/reduction.cpp`.

**When it is tried.** When no setup is left whose station or backsight has
just been placed, the reduction falls back, one setup at a time, on: 0. the
resection of the first setup in the file that has what one needs; 1. the
file's own coordinates for a station nothing positions; 2. the circle as set
on a backsight nothing positions; 3. giving up, saying why. One at a time, so
that a resection's radiation can place the points a later one needs: a second
free station that observes a point only the first radiates is resected from it
too (`ReductionResection.AResectionTakesThePointsAnEarlierResectionPlaced`),
and a setup refused for want of a point is tried again once that point is
placed (`ReductionResection.ARefusedResectionIsTriedAgainWhenAnotherOfItsTargetsIsPlaced`).
A fallback and not a first choice: a station that another setup radiates keeps
that position, as it always did, and where its own setup is oriented on a
backsight, or the circle as set, that setup's pointings to placed points are
checks of it (`ReductionResection.AStationAnotherSetupRadiatesIsNotResected`,
and `ReductionResection.AStationAnotherSetupRadiatedKeepsItWhileItsSetupWaitsForItsBacksight`
for one whose backsight nothing places). A setup that names no backsight is
another matter: nothing orients it but the points it observes. The earlier
versions said its pointings were checks too, and they were not - the setup was
left unoriented and everything measured from it dropped, which is what the
field file's free stations are whenever an earlier setup has shot the next
one's mark. So such a setup, on a station another setup radiated, is resected
from its own block when it is tried, as the field software resected it; the
radiation becomes a check of the resection - a misclosure row "R2 by resection
at setup S2, against its radiation from setup S1", a warning with the
distance, and `radiated_from` and `radiation_offset` in `SURVEY IMPORT`'s
record - and the setup is oriented on its resection and radiates the rest
(`ReductionResection.AFreeStationOnAMarkAnEarlierFreeStationShotIsResectedFromItsOwnBlock`).
With too few placed points by then, it waits for step 0 like a station nothing
positions, and is given up with the reason if it never has enough
(`ReductionResection.AFreeStationOnARadiatedMarkWithTooFewPlacedPointsSaysWhy`).
A station control, an entered or a GNSS point gives is not resected - a
resection must not move it - nor one another resection placed (Not done).
Before the file's own coordinates, because a resection is computed from what
was measured, which is what the reduction is for, where the file's coordinates
of a station are a controller's or a person's and unchecked - the order a
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
is an assumption about the instrument: a setup whose circle was set a degree
wrong on a backsight that a resection then places is oriented on that
backsight's coordinates, the slip found, where the circle taken as a grid
azimuth would radiate everything from it wrongly
(`ReductionResection.AResectionComesBeforeTheCircleAsSet`). A setup is tried
once its targets are placed, whichever setup places them
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
it, others after hundreds of shots - are checks of the resected station, each
a misclosure row, as they were for the field software and as any setup's
pointings to placed points are
(`ReductionResection.ACheckAfterTheFilesResectionBlockChecksTheStationAndDoesNotMoveIt`,
`ReductionResection.ABlockEndThatNamesNoRecordLeavesEveryPointingToTheResection`);
and when a traverse or a network adjustment radiates the side shots again, a
check places nothing, its target held by the resection
(`ReductionResection.ACheckAfterTheBlockMovesNothingWhenATraverseRadiatesAgain`).
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
`ReductionResection.AFreeStationWithNoMarkUnderItHasNoMarksCentringInItsPrecision`,
`ReductionResection.TheHeightIsTheWeightedMeanOfTrigonometricHeightsWithCurvature`,
`ReductionResection.TheHeightIsAWeightedMeanOverSightsOfDifferentLengths`).
The last run of each least squares goes through the network's outlier loop, so
the settings' test - Baarda's w at `outlierSignificance` (critical value 3.29
at the default 0.001) or Pope's tau - flags a resection's residual as it flags
a network's, with a warning, and automatic rejection rejects as there: a
rejected direction takes no part in the orientation either
(`ReductionResection.AGrossReadingIsRejectedAndTheStationOrientationAndShotsComeFromTheRest`).
The orientation is the solution's: `orientAndRadiate` orients a resected setup
on the weighted mean of azimuth less reading over the directions the resection
used - the least-squares orientation for where the station stands
(`ReductionResection.TheOrientationIsTheWeightedMeanOfItsDirectionsAtTheStation`),
whatever backsight the setup names
(`ReductionResection.AResectedSetupIsOrientedByItsResectionNotByTheBacksightItNames`),
and which follows the station when a network adjustment moves it
(`ReductionResection.AResectedSetupFollowsItsStationWhenANetworkMovesIt`) -
and radiates the rest; the pointings it used are not also checks of it.

**Where it starts, and a station with two answers.** Gauss-Newton finds the
solution nearest its start, and a resection can have two. The second review
found one: two marks a few millimetres apart, each with a distance, and a
third mark by direction alone give one distance and one angle - two circles,
which meet twice - and the start the earlier versions took, the rigid fit of
the station's frame onto the close pair, turned by a 5 mm baseline, reached
whichever solution it fell nearer: 50 m from where the readings were made in
up to a quarter of the review's trials, with a precision that said 32 mm at
95 % and nothing flagged. The same start also failed the other way: the closed form of the
first three directions read, where those three were on the station's danger
circle and a fourth mark fixed it, gave a start the least squares could not
solve from, and the station was refused - about half the time, by the errors,
and never with the fourth mark read first. So the least squares now starts
from every point the observations give: where each two of the station's loci
meet - the circle of a distance about its target, the arc from which two
targets are seen under the angle between their readings (the line through
them where that angle is 0 or 180 degrees) - and the rigid fit. Each start is
judged by the weighted squares of the residuals it leaves, the orientation
there the weighted mean of azimuth less reading (which makes them least). The
least squares runs from the best, and then from each other start beyond the
linear reach of the solutions found (below) whose squares are within the
bound of the least found; a run that fails moves on to the next. The arcs are
of each target read with a direction and the next one round the circle, and
the one it makes the widest angle with: every pair of three targets, and of
more a number that grows with the targets, not with their square, every target
on two of them.

The bound is chi-square with two degrees of freedom at the settings'
confidence level - the square of the scale the station's ellipse is drawn at,
5.99 at 95 % - where the likelihood puts the edge of the station's confidence
region: the positions whose weighted squares exceed the least by less than
that are the ones the observations do not exclude. The ellipse stands in for
that region about the solution. Where a second solution lies within the bound
but outside the best one's ellipse, the region has a second part the ellipse
does not show, the observations do not tell the two apart, and the resection
is refused, the two positions and their squares named
(`ReductionResection.TwoPositionsItsObservationsFitAlikeAreRefusedNotEitherPlaced`:
50.023 m apart, their squares 0.057 and 0.625, as a separate script of the
model finds them). Where automatic rejection took an observation out, the
search is made again without it. In the review's Monte Carlo of that geometry
(40 trials a separation, 1.5" a face and 1.5 mm a distance), the round before
put the station 50 m off in 10, 5, 3 and 2 of 40 with the pair 3, 5, 10 and
20 mm apart; now none is placed wrong - 40, 37, 19 and 0 are refused, the rest
placed within millimetres. Four directions whose first three read are on the
danger circle are placed wherever read (20 of 20 in the review's trials, 1 to
3.5 mm from the truth, where 11 were refused;
`ReductionResection.AFourthDirectionFixesAStationWhoseFirstThreeTargetsAreOnItsDangerCircle`),
and a distance to one target breaks the danger circle of the directions in the
tested geometry - a target lies between the station and its reflection in the
diameter through the distance's target, on the arc away from that target - as a
surveyor breaks it (12 of 12, where 6 were refused;
`ReductionResection.ADistanceToOneTargetFixesAStationOnTheDangerCircleOfItsDirections`).
No more is claimed: the third review found the general statement false.
Rejected: a relative one-position rule - marks closer than their pointings
resolve at the range count once - which would need a number of standard
deviations and would still not see two solutions of marks well apart; and
refusing on a second minimum wherever it lies, which would refuse stations
whose observations exclude it.

**What refuses it,** each named in the warning that the setup was not
resected, and in the file's-own-coordinates warning where that follows, and
never a position: too few placed points (saying what it observes and what a
resection needs); a value that is not finite; a horizontal distance that is
not positive (`ReductionResection.ADistanceThatIsNotPositiveIsRefused`);
directions alone, every three of them exactly on one line with the station or
on the circle through it (`ReductionResection.AStationOnOneLineWithItsTargetsIsRefused`,
`ReductionResection.AStationOnTheDangerCircleIsRefused`) - with a distance,
the closed form does not decide, the starts do; a least squares that fails
from every start - rank deficient, not converging - with its reason
(`ReductionResection.ALeastSquaresThatFailsIsRefusedWithItsReason`); two
positions its observations fit alike (above); and a geometry that does not fix
the station. The first version refused the station on one line with its
targets, and on the danger circle, only when the readings were exact: in the
first review's probes, read to an arc second, a station on the danger circle
was placed anywhere on it, 50 to 150 m off, a station on one line with its
marks 25 to 88 m along it, and one with two of its marks 5 mm apart 18 to 156
m off, with nothing flagged. Now the solution is judged by its own a-priori
precision - the station's standard ellipse from the cofactor at the settings'
weights, whatever the residuals say - against the least squares' own
validity. The least squares takes each observation as linear about the
solution; over an offset a from it, a distance of length D changes by up to
a^2 / 2D more than that, and a direction by up to a^2 / 2D^2 radians (the
second-order terms of hypot and atan2). Where the station's ellipse at the
settings' confidence level (x 2.4477 at 95 %) reaches past the offset at which
that exceeds an observation's own standard deviation - sqrt(2 D sigma) for a
distance, D sqrt(2 sigma) for a direction - the model the least squares
solved does not hold over the region the station may be in: its observations
do not fix it. The criterion is the least squares' own and needs no tolerance
from outside. It refuses all 57 of the first review's degenerate probes (20
on the danger circle, 20 on one line, 10 with two of three marks 5 mm apart, 7
mixed) and 9 more with two marks 5, 50 and 500 mm apart at 158 m (placed 0.2
to 30 m off before), and places its four sound ones exactly
(`ReductionResection.AStationOnTheDangerCircleReadWithUnequalErrorsIsRefusedNotPlaced`,
`ReductionResection.AStationOnOneLineWithItsTargetsReadWithErrorsIsRefused`,
`ReductionResection.TwoMarksAFewMillimetresApartDoNotFixAStationWithTheirDistances`,
`ReductionResection.TwoOfThreeTargetsAFewMillimetresApartDoNotFixAStationByDirections`).

A geometric refusal - the two positions, the precision, a least squares rank
deficient from every start - names the shape of a geometry that fixes nothing
which the station's comes nearest, with its numbers at the solution (or, for
the rank deficiency, at the best start): "M and M2 are 5.0 mm apart, 157.750 m
from it", "its sight lines to A, B and C are within 3.0" of one line", "it
stands 0.6 mm from the circle through A, B and C (the danger circle, radius
100.000 m)". The measure that picks it is each shape's own, zero for that
shape exactly (the separation over the distance; the largest sine of an angle
between two sight lines; the distance from the circle over its radius), and
the least is named. The round before named a shape only below a tenth of
that measure, a number with no source that could name the danger circle for
four marks that fixed the station; a fact with its numbers needs no such
number, and whether the geometry fixes the station is the precision's to say.
A least squares that does not converge names no shape: that is not the
geometry's. A refusal leaves nothing of its least squares behind: the
flagged-residual warnings it gave and any observation it rejected are taken
back.

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
their precision says: the first review's station 5 m inside the danger circle,
read 1 to 2" off, 99 mm from the truth against 264 mm at 95 %, and its three
marks within 11 degrees 86 mm against 434 mm. A weak station that had a second
solution within the bound is now refused with it, not placed. Rejected:
refusing the weak ones too. Their positions are right to the precision they
state, and throwing a measurement away is the person's decision
(`ReductionSettings::autoRejectOutliers` is off by default for that reason);
the flag and the warning put it in front of them.

**A network after it.** In a network adjustment the resected station is
adjusted with everything else, its setup's directions as angles from a
reference direction. What the resection's outlier test rejected, the network
takes no more than it did - not a direction as an angle, not a distance, not
a height difference - and a rejected direction is never the reference, which
every angle of the setup is measured from, whether the setup names it as its
backsight or not: a rejected reading takes part in no mean direction
(`directionRejected`, `meanDirection`). The second review found the network
taking a direction the resection had rejected as that reference, putting its
60" into every angle, rejecting the right ones and leaving the station 6.8 mm
off, while the report called the reading rejected
(`ReductionResection.ANetworkTakesNoReadingItsResectionRejected`).

**What it reports.** `ReductionReport::resections`: per resected setup the
station, the points it was computed from, its coordinates and one-sigma
precision, its orientation and that orientation's precision, the a-priori
ellipse and dilution, the checks after its block, the file's own coordinates'
difference, the radiation it replaced and from which setup, and each least
squares as an adjustment is reported - a residual per direction, distance and
height difference with its redundancy number and standardised value. The
precision is the least squares' a-priori one, scaled by its variance factor
only where its global test finds the residuals larger than the a-priori
weights allow, above the test's upper bound
(`ReductionResection.ThePrecisionIsScaledByTheVarianceFactorOnlyWhereTheGlobalTestFailsAbove`);
the mark's centring and height are added where there is a mark. The earlier
versions always scaled it, a posteriori; but on the one to three degrees of
freedom a resection has, the variance factor is itself uncertain - a
chi-square on r has a relative standard deviation of sqrt(2 / r), 141 % on
one, 82 % on three - so one the test accepts says no more than the weights,
and scaling by it shrank or swelled the precision by chance: to 3 nm for the
free-station fixture, whose exact readings fit exactly, and to a sixth of the
a-priori value for one of the owner's heights, whose test failed for fitting
too well. The residual tables keep the variance factor, and the Resections
table its square root, so the a posteriori value can still be read. The report
prints a Resections table - with Dilution and Checks columns, and a Check
column that says which way a global test failed ("plan global test FAILED,
0.000 below 0.216 (fits too well)", or "above" with the upper bound) as the
network's summary gives its bounds, and "radiated from S1 9.7 mm away" where
a radiation was replaced - and each resection's residuals ("Residuals:
resection at S1 (horizontal)"), a row FLAGGED as a network's is; the station's
coordinate row says "resection". A resection with no redundancy is warned
about ("nothing checks the position it gives"), and one with no height too -
naming why, where the marks were read with zenith angles but no distance,
which a trigonometric height needs
(`ReductionResection.AResectionReadWithZenithsButNoDistancesSaysWhyItHasNoHeight`).
Where the settings reduce no distance to the grid and the drawing's projection
gives a point scale factor that would change the longest distance by more
than its standard deviation, the resection has fitted ground distances to grid
coordinates, and a warning says so, naming the setups, with the factor in the
record (`ReductionResection.GroundDistancesFittedToAProjectedGridAreSaid`).
`SURVEY IMPORT` answers one `resection` record each - `dilution`, `weak`,
`checks`, `file_offset`, `radiated_from`, `radiation_offset` and
`unapplied_scale_factor` among its fields - and a `resection_residual` record
for each residual flagged or rejected, so an agent sees which, past the cap on
listed warnings (`docs/mcp.md`). The network adjustment's rank-deficiency
message calls what it cannot resolve unknowns, naming a set's orientation
"orientation(P)", not a coordinate
(`SurveyHorizontalNetwork.APointOnTheCircleThroughItsThreeTargetsIsRankDeficient`).

**Checked.** `tests/survey/test_reduction_resection.cpp`: two placed points
with distances and three directions alone, each recovering the station they
were made from; a perturbed resection whose position, orientation, residuals,
variance factor and precision match a separate script of the textbook model
(normal equations, not Katana's code) to 1e-8 m and 0.000001"; the
orientation's and the height's weights over sights of different lengths, each
against a weighted mean worked apart that an unweighted one misses by 0.13 mm
and 0.44 mm; the distances' three factors; automatic rejection; a network that
moves the station, and one that takes none of what the resection rejected;
the file's block and its check; the file's own coordinates; the unapplied grid
scale; two solutions; the starts; every refusal, exact and read with errors;
the weak flag; the order and the fallbacks; the precision rule. Each proven by
removing its part, the tests rebuilt and run (a scratch script). The round
before: the precision criterion removed, the four noisy-geometry tests fail;
the take-back, the close-marks one; the distances' factors, the four factor
tests; the orientation or the heights unweighted, the test of each; the
instrument's centring and height in each pointing's weight, four; the mark's
centring and height not added, three; a rejected reading left in the
orientation, the rejection test; the block ignored, the two block tests; the
file-coordinates check, its test; the grid-scale notice, its test; the weak
flag, its test; the resection's own pointings radiated as checks, two; only
the coordinate tolerance making one position, its test. This round, 25 parts,
24 caught: without the check for a second solution, with the search stopped at
its first solution, or with its starts unsorted, the two-positions test fails;
with the round before's starts (the rigid fit, else the first three
directions' closed form), four - the fourth direction, the distance on the
danger circle, the retried refusal and the two positions; with the exact
refusal applied where a distance could break it, the danger-circle distance
test; with the network taking a rejected direction, distance or height
difference, or a mean direction taking a rejected reading, the network test;
without the resection of a setup on a radiated station, its two tests, and
without its check reported, one; the review's seven surviving mutants - the
circle as set tried first, every candidate resected at once, no second try
after a refusal, a placed station resected over, a resected setup oriented on
its backsight, a check re-placing its target, every station taken to be over a
mark - one or two tests each; the precision always scaled, four, and never
scaled, one; every station over a mark and the precision always scaled, the
CLI's pinned free-station precision too; no shape for a rank deficiency, one;
the round before's words for no height and for the rank deficiency, their
tests. The one part no test reaches: a start whose least squares fails is
passed over for the next (`continue`, not `break`), and in 400 random
resections through the command line - near-degenerate shapes among them - 201
probe files and the owner's three field files the best start never failed
where a later one then solved.
Two fixtures: `tests/surveyio/data/fld/resection.fld`, whose free station
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
mean of the block's three, 20.00030586, and its precision to the a-priori
sigma_n 1.142 mm and sigma_e 1.153 mm of a separate script, no mark under it).

The owner's resection job, `CRS SET EPSG:7856` and `SURVEY IMPORT` (a local
check; the file is the owner's): before the resection, 778 points and
42 reduction warnings, twelve of them "stands on ..., which has no position";
now all 4 834 points and 71 warnings - the 27 face-pair warnings as before, the
prism and atmospheric notices, one notice that the twelve resections fitted
ground distances to grid coordinates, and 41 residuals the resections flag: 27
distances, every one computed shorter than measured, and 14 directions. Each
resection is of its block, redundancy 3 in plan and 2 in height; the 14 checks
after the blocks are misclosure rows; the dilutions are 0.41 to 0.94, none
weak. The search from every start finds one solution for each of the twelve,
where the rigid fit reached the round before: every drawn position, the
stations' among them, is as the round before gave it, to the last digit
`LIST` prints, and no setup names no backsight on a radiated station. The
import's defaults apply no grid scale factor, and the marks are MGA grid
coordinates where the zone's point scale factor is 0.99980 (200 ppm, 32 mm on
the job's longest resection sight of 162 m): the distance residuals carry it,
every plan global test fails above, and the plan precision is scaled by its
variance factor - 3.1 to 13.4 mm, one sigma, and it says the weights were too
optimistic for ground distances on grid marks; none of the height tests fails
above, and the heights' precision is the a-priori 1.2 to 1.3 mm, where the
a-posteriori one was 0.19 to 2.24 mm. The controller wrote its own residuals
after each block, one line per face per mark: another program's numbers, so
an independent check. Its scale factor is in them: fitting its station,
orientation and scale to where its residual lines put the marks gives
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
weights them its own way. The reduction of the job took 13.1 to 14.7 ms before
the resection and 17.3 to 19.2 ms now (Release, the reduction alone, best of
five in each of five runs), against 17.9 to 18.3 ms the round before: the
search from every start costs nothing measurable, its least squares running
from the best start and then only from starts the bound lets through. The
whole command, drawing and listing 6.2 times the points, was 0.27 s before and
0.39 s the round before; on this round's loaded machine (five tracks building)
main, the round before and this round all took 0.37 to 0.45 s, and nothing
between them can be told apart.

The owner's other files import as they did before (`SURVEY IMPORT`, every
`LIST` position compared): the second field file (862 points, 8 warnings), the
RTK job (3 482, none), the SDR traverse (no point, 47 warnings: none of its
setups observes a placed point) and the GSI traverse (57, 24) give output
identical to main's, byte for byte: none of their setups resects, and none
that names no backsight stands on a station another setup radiated.

Not done:

- The targets are held: their own errors do not reach the station's precision.
  A resected station is not computed again when an adjustment moves its
  targets - the network adjustment adjusts it with the rest, a traverse
  adjustment leaves it.
- A setup whose station is already placed and that has no backsight is not
  oriented on the placed points it observes where that station is control, an
  entered or a GNSS point, or was placed by another resection - a second
  resection block on one station: its targets are not radiated, and its
  warning names the points it could be oriented on
  (`ReductionResection.ASecondBlockOnAStationAlreadyResectedSaysWhatItCouldBeOrientedOn`).
  On a known station that would be an orientation alone, and on a resected
  one a second resection of one station, whose two results would need
  combining; orienting every such setup would change every format's setups
  that name no backsight, which is the reduction's decision to make, not this
  one's.
- A setup that used a free station's radiated position before that station's
  own block resected it - a backsight on it, a resection from it, earlier in
  the same pass - keeps what it used; the network adjustment makes them
  consistent.
- A setup whose station fell back on the file's own coordinates before its
  targets were placed is not resected once they are.
- A resection read with zenith angles but no distances has no height: a
  trigonometric height from the zenith angles over the resected horizontal
  distances is not formed.
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
- A station with two solutions is found only where a start leads to each: the
  starts are the points where two loci meet, and a solution none of them
  descends to within the bound is not looked for.
- Four decisions have no test that fails without them: the re-queue of a
  setup that waits for a resection (`reduction.cpp`), the guard that keeps
  such a setup out of the first pass, the network branch for a resected
  station (`reduction_adjust.cpp`) and the scale applied to ground distances
  all survived the third review's mutants through every survey test and the
  CLI patterns. Their behaviour was checked by hand and by the review's
  probes, not pinned.

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
Linework tab, and the wizard and `SURVEY IMPORT` do not yet ask for the
finish that codes and strings an import (`cad::withSurveyFinish`,
`docs/survey_coding.md`), so both still draw points only; undoing an import
that created a nested layer (`survey/points`) takes that layer away and
leaves the parent it created with it (`survey`), because the layer command's
undo removes only the layer it was given; the Point Manager is
read-only, and its chrome has been seen only in a screenshot; the survey tools
are not in the interactive-tool catalogue (`docs/cad.md`); no headless test
presses Use Selection; under the offscreen platform there is
no monospace font, so report columns look misaligned in the test PNGs; two
headless tests write a template to the user's settings and delete it in the
same run, so a killed point-report run leaves one behind. A name holding one
number followed by coordinates ("P1 500000 7000000 0") is still read as the
label "P1 500000", which cannot be told apart from "CP 1 500000 7000000".
