// Sokkia SDR (.sdr): the SDR electronic field book's Comms output - a job as
// fixed-width ASCII records, in either of the two layouts the field book
// writes: SDR33 (point names in a 16-character field; the field book lets a
// name be 14 characters, [SDR] chapter 2) and SDR2x (4-digit point numbers).
//
// Specifications this reader implements:
//   [SDR]   Sokkia Technology, "Interfacing with the SOKKIA SDR Electronic
//           Field Book", software version 04-04.xx, October 1999. Chapter 2:
//           the views an observation is stored and sent in, and printed
//           example jobs; chapter 3: the transmission and the record
//           structure; 3.1-3.3 the field types (a field of blanks is null,
//           "not measured"); 3.3.2 the 13DU distance-unit note; 3.4 the
//           derivation codes; 3.5 the option values; 3.6.1 and 3.6.2 the
//           SDR2x and SDR33 record layouts; chapter 4, sample files. Chapter
//           5, "General notes on SDR files".
//   [SETX]  Sokkia, "SDR Software Reference Manual (SETX)": 3.5.6 (the
//           units: quadrant bearings are degrees underneath, and the
//           coordinate setting is "the order in which they are displayed"),
//           chapter 5 (the OBS, MC, RED and POS views), 8.2 (the back-bearing
//           record "orients subsequent observations until another
//           back-bearing record is stored"), 8.2.1 (a skipped backsight),
//           8.2.2 (several backsights averaged into one back-bearing record),
//           chapter 28 "SDR Database" (what each record is) and chapter 29
//           "Observational Calculations" (29.1 the corrections between the
//           views, 29.2.3 the faces, 29.2.5 the collimation correction,
//           29.2.6 the orientation correction A = H + BKB azimuth - BKB
//           h.obs).
//   [L5]    Sokkia, "SDR Level 5 Reference Manual" (750-1-0073 rev 1),
//           appendix A.1 (the atmospheric correction is set "either on the
//           instrument or via SDR pressure and temperature entries at the
//           station setup, but NOT both"), appendix B ("The SDR applies the
//           prism constant and atmospheric parts per million (PPM)
//           corrections as soon as the observation is accepted") and B.4.4
//           (the mounting eccentricity correction, for an EDM or a reflector
//           off the telescope's axis).
//   [PROLINK] Sokkia, "ProLINK Reference Manual" (750-1-0006 rev 2),
//           6.1.3.2: Sokkia's office program takes a point's coordinates
//           from its latest POS, STN or POS-view record, searching "starting
//           at the end of the field book".
//   [TA]    Trimble, "SDR33 Observations.xsl" (2022), a published writer of
//           the format: in that version its header has no serial number (42
//           characters - a file met whose notes and job flags are this
//           writer's has a 46-character one, so the length does not tell
//           the writer), it writes 3 in the distance-unit option for US
//           survey feet and in the coordinate-order option for "Y-X-Z", its
//           time stamp note is "Time Date MM/DD/YYYY Time HH:MM:SS", it
//           turns the job's atmospheric switch on whenever it writes
//           weather, its slope distances carry the prism constant and no
//           atmospheric correction, it writes the controller's field book
//           records in their order, its back-bearing record's circle is
//           that record's face 1 reading on the backsight, and it puts the
//           instrument's serial number in the EDM description field. Its
//           notes give the angle-unit option 4 as "Quadrant bearings
//           (stored as degrees ...)", a value it never writes.
//   [JXL]   Trimble, "JobXML Schema" 5.72: the field book those records
//           come from, where "The BackBearingRecord element provides the
//           backsight orientation details that will apply to the following
//           observations" - so [TA] writes a back-bearing record before the
//           shots it orients, as the file met writes each before its round.
//   [NIKON] Nikon, "Total Station DTM-322 Instruction Manual", pages
//           169-172, the SDR2x and SDR33 columns side by side, and the same
//           tables in Spectra Precision's "Focus 6" user guide, pages
//           183-186: a header version written with no blank ("SDR33V04-01",
//           "SDR20V03-05"), 4 in the angle-unit option for mils, the
//           coordinate-order option named as such ("1 NEZ, 2 ENZ"), a
//           refraction constant of 0.132 for option 1, and 09MC as the only
//           observation record.
//   [LISCAD] Listech, "Sokkia Field Operation Codes" (LISCAD help), a
//           program that reads the format: it begins a set collection at
//           the field book's note "13SCSet #: .." and completes it at the
//           "12SC" record.
//
// What those say, and this reader relies on:
//   * Bytes 1-2 are the record type, 3-4 the derivation code. The SDR2x
//     layout is the SDR33 one with a point id of 4 characters rather than
//     16 and a real of 10 rather than 16, less some fields - its JOB has no
//     option flags, its SET no set number or marks - and a few records
//     (GPS and projection ones, none imported here) with 16-character reals.
//     The header's version ("SDR33 ...", "SDR20 ...") says which layout the
//     rest of the file is in ([SDR] 3.2.5 and chapter 5).
//   * The header's options are the units of everything after it: angles in
//     degrees, gons or mils (decimal - [SDR] 3.3.1 gives angles 8 decimal
//     places, and 6400 mils to the circle), distances in metres or feet,
//     pressure in mmHg, inHg or mbar, temperature in Celsius or Fahrenheit.
//     13DU names the distance unit again, can narrow feet to US survey feet,
//     and governs the distances after it ([SDR] 3.3.2: "All distances
//     specified after this Note record are displayed in the specified
//     distance units"), so one that disagrees with the header is followed,
//     and said to. An angle or distance unit no writer defines is refused,
//     since every number after the header depends on it: [SDR] 3.5 defines
//     angles 1 to 3 and distances 1 and 2, and the angle option 4 (below)
//     and [TA]'s distance option 3 are read as their writers define them.
//     An undefined pressure or temperature unit costs only those values,
//     and is warned about.
//   * 01 is the instrument, 02 a setup (point, coordinates, instrument
//     height), 03 the target height for what follows, 04 the collimation
//     corrections, 05 the pressure and temperature, 06 the job's scale
//     factor, 07 the backsight, 08 a point's coordinates, 09 an
//     observation (slope distance, vertical reading, horizontal circle
//     reading), 10 the job, 11 a reduced observation, 12 a set, 13 a note.
//   * 09 F1 and F2 are the two faces; MD, a mean of several distances, is
//     on the face its vertical reading says ([SETX] 29.2.3: 0 to 180
//     degrees is face 1, 180 to 360 face 2).
//   * The vertical reading is a zenith angle or one "measured upwards from
//     horizontal", as the latest instrument record says ([SDR] 3.5); the
//     second is taken to its zenith equivalent, 90 degrees less it
//     ([SETX] 29.2.3: the reading "is converted to an equivalent zenith
//     angle").
//
// Readings that are this reader's own, stated so they can be checked:
//   * A line opening with "DD" is a DELETED record. No published document
//     describes the prefix, but after it every such line is a complete
//     record, and a record type is two digits, so a D cannot open a live
//     one: every leading D is part of the mark ("DD" and "DDDD" in the files
//     met). A deleted record is not imported, is counted as skipped with a
//     warning naming it, and changes nothing: a deleted 03 sets no target
//     height, a deleted 02 begins no setup.
//   * A 07 gives two numbers, and the model has a field for each: its
//     AZIMUTH is the setup's statedBacksightAzimuth and its horizontal
//     observation - the circle reading on the backsight - its
//     backsightAzimuth, the model's circle. One field for both would drop
//     the difference - the ten seconds of [SDR] chapter 2's traverse
//     (azimuth 269 59 50, circle 270 00 00), the 14 degrees of its chapter 4
//     sample (azimuth 14, circle 0). Where the backsight has no coordinates
//     the reduction orients the setup on the azimuth less the setup's own
//     mean reading on the backsight, or with no reading less the 07's
//     circle reading. That is [SETX] 29.2.6's A = H + BKB azimuth - BKB
//     h.obs with the setup's readings standing in for the h.obs where there
//     are any: for one backsight they differ by the pointing error of its
//     rounds, which the mean of all of them shares out; for a back-bearing
//     averaged over several backsights ([SETX] 8.2.2), whose h.obs is
//     computed rather than read, they differ by that averaging, since the
//     reduction orients on the one backsight the record names.
//   * One setup per 02, with every 07 after it that names the same
//     backsight, the same azimuth and a circle reading within one minute of
//     arc of the first as another ROUND of that setup. [SETX] 8.2 has a
//     back-bearing record orient what follows until the next, so rounds on
//     one circle setting share one orientation, and the reduction means
//     them together, face pair by face pair. A 07 that names another
//     backsight or azimuth, or a circle moved further, begins a new setup on
//     the same point, as the TDS RW5 reader does for a repeated backsight.
//     The minute: the face-mean circle readings on one backsight across six
//     rounds spread by 6" at most in the files met, while a circle moved
//     between rounds on purpose moves by degrees. Merging is exact when
//     every round observes the same targets (every setup of the files met
//     does); a target missed by a round is oriented on the mean of all of
//     them, off by at most half their spread.
//   * Sokkia's Set Collection writes a set's back-bearing (07 SC) AFTER the
//     set's observations ([SDR] chapter 2's V04-01 example: "BKB SC ... H.obs
//     0-00'15"", the mean of the set's face 1 and face 2 readings on the
//     backsight). So a 07 SC read after a set's observations, with no 07
//     between, orients the set it closes: when it begins a new setup, the
//     set's observations go with it. A 07 of any other derivation code
//     orients what follows it ([SETX] 8.2), after a set or not.
//   * Where a set's 12 stands depends on the layout. [SDR] chapter 2: "The
//     standard SDR33 format [writes] a SET record before the raw
//     observations", and from V04-02 on a job with 4-digit point numbers -
//     the SDR2x layout - writes it after them, before the MC records ("the
//     SET record precedes the MC records for SDR2x format compatibility");
//     [LISCAD] reads that order too, the set completed by its 12. V04-01
//     wrote the SDR2x 12 first, as SDR33 does, and the header cannot tell
//     the two apart: [SDR] 3.2.5 and chapter 5 send every job with 4-digit
//     point numbers as "SDR20 V03-05". So the set's 07 SC decides, when it
//     closes the set: raw observations read after the 12 are its set (the
//     12 opened it, V04-01's order); none - only its MC records, or nothing
//     - and the set was the raw observations before the 12, those since the
//     setup began, its last 07 or 12, or a 13SC "Set #" note, the last of
//     them as many as the 12's count where it states one.
//   * A setup's first 07 read after some of its observations orients them
//     too only where they show themselves on its circle: they read its
//     backsight, every time, within a minute of the 07's circle reading
//     (face 2 less half a circle; with no circle stated, a shot of the
//     backsight is enough). The circle then was not set anew at the 07,
//     so they were read on the orientation it gives. No writer known puts
//     shots first ([SETX] 8.2 and [JXL] have a back-bearing record orient
//     what follows it), so otherwise the observations before it stay a
//     setup of their own, oriented by a keyed azimuth or read as azimuths
//     (below), and the 07 begins a new setup on the same point - with a
//     warning, since a shot taken before any backsight, or before the
//     circle was set anew, is unusual enough to be a mistake.
//   * A later 07 that names the setup's backsight and begins a new setup
//     (its circle moved) takes with it the shots of that backsight read
//     just before it, since the setup's last 07, 12 or "Set #" note, on its
//     circle and not on the setup's: the same point read on the new circle
//     shows it was read after the circle was set anew. Where the setup's 07
//     gives no circle reading, the setup's first reading of the backsight
//     is its circle; where the new 07 gives none, a shot of the backsight
//     off the setup's circle is enough. Any other shot stays with the
//     orientation it followed ([SETX] 8.2) - a point other than the
//     backsight read on a new 07's circle shows only that the two records'
//     azimuths disagree, not that the circle moved. Rounds whose 07s give
//     no circle reading are one setup with nothing to compare, so their
//     readings of the backsight are compared face by face, and a spread
//     past the minute is warned about.
//   * Those minutes are widened by the horizontal collimation applied to
//     the readings (below). A 07's circle reading may be [TA]'s, a raw face
//     1 reading, from which a corrected reading differs by the correction;
//     or the field book's own, a set's face mean ([SDR] chapter 2's BKB
//     SC), which the corrected readings match.
//   * A setup with no 07 is oriented by a keyed azimuth: an 11 with an
//     azimuth and no distance from the setup's point ([SDR] chapter 2's
//     "RED KI 0100-0101 Azimuth 23-56'15" H.dist <Null> V.Dist <Null> Code
//     BS AZ"), the first to a point the setup observes. That chapter's
//     OBS-view example has no back-bearing record, and its printed
//     positions follow A = H + the keyed azimuth - the reading on that
//     point. A setup with neither takes its readings as azimuths, as the
//     field book does when the backsight is skipped ([SETX] 8.2.1:
//     "Horizontal angles stored are treated as azimuths with no orientation
//     correction applied"): its backsightAzimuth, with no backsight named,
//     is the model's circle set to read azimuths, and the reduction says so.
//   * The atmospheric correction's state is Unknown whatever the job says.
//     [L5] has the field book apply it to each distance as it is accepted
//     when the job turns it on (and the instrument apply it when the job
//     does not); [TA] turns the job's switch on and writes distances
//     WITHOUT it. A file does not say for certain which program wrote it,
//     so the reduction is told it does not know, and says so; where the
//     file's time stamps are in [TA]'s form, what the file did not carry
//     says that too, since it points to the distances being raw.
//   * The prism constant is in the distances (Applied): [L5] appendix B
//     for the field book, and [TA] adds the target's constant to every
//     distance and writes 0 in the instrument record. The instrument
//     record's value (mm, [SDR] 3.3.4) is the constant.
//   * Raw distances carry no scale factor and no curvature and refraction:
//     [SETX] 29.1 applies those between an observation and its corrected
//     (MC) and reduced (RED) views.
//   * Option 45 is the order of the coordinates in 02 and 08: [SDR] 3.5
//     defines 1 "N-E-Elev" and 2 "E-N-Elev". 3.6.2 names 21-36 the northing
//     and 37-52 the easting, which is option 1's order; [SETX] 3.5.6 calls
//     the setting "the order in which they are displayed". Under 2 the
//     easting is read first, as [TA] writes it, with one warning a file:
//     no source shows the order Sokkia's field book sends under 2. Its own
//     E-N-Elev example ([SDR] chapter 2, V04-04.30) is a printed report
//     ([SDR] 2.4: "displayed in Printed output form"), so it shows the
//     display order and not the file's; its HORZADJ record, printed with
//     fixed labels, only identifies its translation's "Trans.N" as the
//     easting (that reading takes its GSTN 1005 to 1.1 mm east and 0.8 mm
//     south of its printed POS 1005; the other, 5.6 cm off in each
//     ordinate, 7.9 cm in all). [TA]'s 3,
//     "Y-X-Z", is east first as well, and read so with its own warning:
//     [SDR] does not define 3, and [SETX] 3.5.6 lists "S-W-Elv" as the
//     field book's third display order. A file that states coordinates
//     under any other option is refused. The order read is in the
//     project's metadata.
//   * The angle-unit option 4, which [SDR] does not define, is mils in a
//     header written as [NIKON] writes its own (no blank between "SDR33" or
//     "SDR20" and the version), whose manuals define 4 so; in any other it
//     is degrees - [TA]'s quadrant bearings, which [SETX] 3.5.6 says are
//     degrees underneath. Either way it is warned about, naming the other
//     reading. A header in [NIKON]'s form also takes the refraction
//     constant option 1 as [NIKON]'s 0.132 rather than [SDR] 3.5's 0.14.
//   * Coordinates keyed in - a 02 or 08 whose derivation code is KI - are
//     Entered; an 08 TP (a shot stored as a position) FieldObserved; an 08
//     AJ, TV or RS (the traverse adjustment, the traverse, the resection)
//     Calculated; any other Unknown. The LATEST coordinates a file gives a
//     point are kept, the field book's own rule ([SDR] 2.3: "the latest
//     coordinates are the best(or an observation in POS view) will
//     over-ride an observation in OBS view even if the OBS view is stored
//     later"; [SETX] 6.1 rule 2; [PROLINK] searches from the end of the
//     field book for a point's POS, STN or POS-view record): the 08 AJ of a
//     traverse adjustment follows the positions it adjusts, and a later 02
//     restates the station as the field book held it then. The earlier
//     ones are kept in the point's metadata, with a warning. (The shared
//     builder keeps the first for the other raw formats, whose later
//     positions are checks.)
//   * Except the POS view of an observation. A field book sending more than
//     one view writes "more than one record for each observation record",
//     one after another - for its current view and the POS view "a raw
//     observation record followed by a position record" ([SETX] 27.2) - and
//     an MC or RED record has the views an observation has ([SETX] chapter
//     6: "MC and Red records can also be stored in Pos view"; 8.5.5's
//     printed examples follow each OBS record, the averaged OBS MC among
//     them, with its POS TP). So an 08 other than KI, AJ, TV or RS read
//     straight after a 09 F1, F2, MD or MC, or an 11 with distances, of the
//     same point is that observation's POS view, whatever the reader does
//     with the observation (an MC or RED is not imported, a bad set's raw
//     one is skipped), and the observation stayed in its own view in the
//     field book: one in OBS, MC or RED view comes after every POS, STN and
//     POS-view record ([SETX] 6.1 rules 2 and 3), and Store OBS "will NOT
//     overwrite a previous coordinate if it exists in POS view" ([SETX]
//     8.5.2) - a check shot onto control. So it places a point that has no
//     coordinates, or whose coordinates came from such a view (the latest
//     observation is used where nothing else is, [SETX] 6.1 rule 3), and
//     never supersedes coordinates from any other record: those stand, and
//     its own go to the point's metadata, with a warning naming the
//     observation. An 08 after an 08 of the same point is a record of its
//     own - 8.5.5's averaged position follows the POS view it averages -
//     and so is one with any other record between it and the observation.
//     Two outputs cannot tell an observation stored in OBS view from one
//     stored in POS view: with the POS view alone ("all the observation (OBS
//     or MC) and reduced (RED) records are output as POS records", [SETX]
//     27.2) each is a lone 08, taken as a position; with the OBS and POS
//     views both on, rather than the current view, each is a 09 and its 08,
//     taken as a check - so a shot stored in POS view, which the field book
//     lets overwrite, is kept aside too, the safer mistake: control then
//     never moves unsaid.
//   * An 08 or 02 that gives an elevation and no northing or easting cannot
//     place its point; the height is kept in the point's metadata ("height
//     without a position", as the GSI reader keeps one), with a warning.
//   * A 09 MC and an 11 with distances are not imported. Each is the field
//     software's reduction of a raw observation - MC oriented and reduced
//     for the heights, the prism constant, the weather, and curvature and
//     refraction, RED further for the scale factor and sea level ([SETX]
//     chapter 5 and 29.1) - so beside a raw observation of the same line in
//     its setup it would count it twice, whether read before that raw
//     observation or after it: the verdict waits for the setup's next 02 or
//     the file's end, and "its setup" is everything observed from that 02,
//     rounds and re-orientations included. Without one - a field book set to
//     send the MC or RED view ([SDR] 2.1), an MC of a target its setup
//     never observed raw, a RED from another point, or a [NIKON]
//     instrument, whose only observation record is 09MC - the writers
//     disagree about what the numbers are: Sokkia's MC is a mark-to-mark
//     vector already oriented and corrected, [TA]'s the inverse of the
//     computed grid coordinates, [NIKON]'s "slope distance, vertical angle,
//     horizontal angle" with target heights in 03 records beside them. So
//     they are skipped with the reason, and the shot is said to be lost, in
//     its warning and in what the file did not carry.
//   * A 04's collimation corrections are applied to every raw reading after
//     it as [SETX] 29.2.5 applies them: face 1 readings plus the vertical
//     (Vc) and horizontal (Hc) correction, face 2 minus, a reading with no
//     vertical angle taken as face 1 ([SETX] 29.2.3). They apply until the
//     next 04, an 01 of another instrument type (its EDM type, the code
//     [SDR] 3.5 lists instruments by), or another job: [SETX] 13.1 applies
//     them "until either the instrument type is changed or a new
//     collimation record is added", and "Collimation is not maintained
//     across all jobs" (the Level 5 manual, chapter 13, the same). An 01 of
//     another type between two shots of one setup ends it there all the
//     same: it is applied reading by reading, where the setup's other
//     settings, which the model holds once a setup, are kept whole. A face
//     pair's mean is unchanged, so a round observed on both faces is
//     reduced as before; a shot on one face is corrected. The field book
//     applies Vc after the instrument and target heights, the reduction
//     here before them, which differs by at most Vc times the height
//     difference over the distance - 0.01" for a 10" correction and 0.1 m
//     of height at 100 m.
//   * A 12 SET whose bad marker is 2 ("Bad set", [SDR] 3.5) is not used "for
//     further averaging" ([SDR] chapter 2): its raw observations are
//     skipped with a warning - as many as the record's count ([SDR] 3.6.2
//     "Count of observations"; chapter 2's example counts 4 for its four 09
//     records), or up to the next set or setup where it states none.
//   * A 13 note is kept in the notes of the setup it falls in (the
//     project's before the first); a 13TS time stamp also dates the setup
//     it is the first of - a time during the setup, which a writer may
//     stamp at any round. Its text is read as [TA]'s "Time Date MM/DD/YYYY
//     Time HH:MM:SS" or as the field book's "DD-Mon-YY HH:MM" ([SDR]
//     chapters 2 and 4) or "Mon-DD-YY HH:MM" (its V04-04.30 example), a
//     two-digit year by the POSIX strptime %y rule (69-99 are 1969-1999,
//     00-68 2000-2068), and a day that is not in its month's Gregorian
//     calendar is not a date.
//   * Blank lines are not records and are passed over; the record numbers
//     in warnings are line numbers, blank lines counted. So is a line of
//     DOS end-of-file marks (0x1A), which a file copied through MS-DOS
//     keeps. An STX that opens a transmission ([SDR] chapter 3) is framing
//     whether or not the writer ended its line. [SDR] chapter 3 ends every
//     record with CR LF, so a last record with no line end is read with a
//     warning that the file may have been cut short inside it: a real cut
//     short is still a number, and nothing else would say so.
//   * Point ids are trimmed at both ends: [SDR] 3.2 pads an alpha field on
//     the right, but its own chapter 4 sample right-justifies the SDR33
//     point ids, as the files met do. An id that holds a control byte is
//     line noise, not a name - blanked, two different ids would become one
//     - so its record is skipped, naming the byte.

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <functional>
#include <initializer_list>
#include <iterator>
#include <map>
#include <numbers>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/math/unit_ratio.hpp"
#include "katana/survey/angles.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "topcon_raw_builder.hpp"

namespace {

using katana::core::ErrorCode;
using katana::core::formatExactReal;
using katana::core::makeError;
using katana::core::Result;
using katana::core::trimmed;
using katana::surveyio::FormatDescriptor;
using katana::surveyio::FormatSignature;
using katana::surveyio::ProbeInput;
using katana::surveyio::ReadOptions;
using katana::surveyio::ReadResult;
using katana::surveyio::topcon::faceOfZenithReading;
using katana::surveyio::topcon::parseReal;
using katana::surveyio::topcon::RawProjectBuilder;
using katana::surveyio::topcon::wrapToCircle;
using katana::surveyio::topcon::zenithInModelRange;
namespace survey = katana::survey;

constexpr std::string_view kFormatId = "sokkia-sdr";
constexpr double kPi = std::numbers::pi;

// [SDR] 3.3.1: 6400 mils to the circle.
constexpr double kRadiansPerMil = kPi / 3200.0;

// Hectopascals in one conventional millimetre of mercury: 13.5951 g/cm3 x
// 9.80665 m/s2 x 1 mm = 133.322387415 Pa, the definition NIST Special
// Publication 811 (2008), appendix B.8, gives as 1.333 224 E+02 Pa. An inch
// of mercury is as many of them as an inch has millimetres (3.386 389 E+03
// Pa there), the inch being unit_ratio.hpp's.
constexpr double kHectopascalsPerMillimetreOfMercury = 1.33322387415;
constexpr double kMillimetresPerInch =
    katana::math::scaleByRatio(1.0,
                               katana::math::units::kInternationalInch.numerator *
                                   katana::math::units::kMillimetre.denominator,
                               katana::math::units::kInternationalInch.denominator *
                                   katana::math::units::kMillimetre.numerator);
constexpr double kHectopascalsPerInchOfMercury =
    kMillimetresPerInch * kHectopascalsPerMillimetreOfMercury;

// Rounds of one setup whose circle readings on the backsight differ by more
// than this were not observed on one orientation (see the top of this file).
constexpr double kSameCircle = kPi / 10800.0; // one minute of arc

// Whether two circle readings (radians) are within kSameCircle of each
// other, round the circle's zero, and `slack` more: the horizontal
// collimation one of them may carry and the other not (see the top of this
// file).
bool onSameCircle(double a, double b, double slack = 0.0)
{
    double difference = std::fabs(wrapToCircle(a) - wrapToCircle(b));
    difference = std::min(difference, 2.0 * kPi - difference);
    return difference <= kSameCircle + slack;
}

// A horizontal reading as face 1 reads that line: face 2 less half a circle
// ([SETX] 29.2.3's faces), a reading of no known face taken as face 1.
double faceOneReading(const survey::HorizontalDirectionObservation& direction)
{
    return direction.pointing.face == survey::Face::Right
               ? wrapToCircle(direction.direction - kPi)
               : direction.direction;
}

// The refraction constants of the job record's option: [SDR] 3.5 ("1 0.14,
// 2 0.20") and, for a header in [NIKON]'s form, [NIKON] page 172 ("0.132 or
// 0.200").
constexpr double kRefractionOption1 = 0.14;
constexpr double kNikonRefractionOption1 = 0.132;
constexpr double kRefractionOption2 = 0.20;

enum class Variant { Sdr33, Sdr2x };
enum class AngleUnit { Degrees, Gons, Mils };
enum class PressureUnit { Unknown, MillimetresOfMercury, InchesOfMercury, Millibars };
enum class TemperatureUnit { Unknown, Celsius, Fahrenheit };
enum class CoordinateOrder { Unknown, NorthFirst, EastFirst };
enum class VerticalReference { Zenith, Horizon, Unknown };

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

bool isCodeChar(char c)
{
    return (c >= 'A' && c <= 'Z') || isDigit(c) || c == ' ';
}

bool isControl(char c)
{
    const auto byte = static_cast<unsigned char>(c);
    return byte < 0x20 || byte == 0x7F;
}

// Two digits of record type and two characters of derivation code ([SDR]
// chapter 3), the derivation possibly blank ("used in reports", 3.4).
bool isRecord(std::string_view line)
{
    return line.size() >= 4 && isDigit(line[0]) && isDigit(line[1]) && isCodeChar(line[2]) &&
           isCodeChar(line[3]);
}

// `line` with its deleted-record mark stepped over: every leading D, where
// there are two or more ("DD", "DDDD"), `marks` of them. A single D is no
// mark, and `line` comes back whole.
std::string_view withoutDeletionMarks(std::string_view line, std::size_t& marks)
{
    std::size_t count = 0;
    while (count < line.size() && line[count] == 'D') {
        ++count;
    }
    marks = count >= 2 ? count : 0;
    return line.substr(marks);
}

// `line` without the STX ([SDR] chapter 3) a transmission opens with: on a
// line of its own, or glued to the first record by a writer that left out
// the line end after it.
std::string_view withoutStx(std::string_view line)
{
    return line.starts_with('\x02') ? line.substr(1) : line;
}

// A line of DOS end-of-file marks (Ctrl-Z, 0x1A), which a file copied
// through MS-DOS keeps: framing, not a record.
bool isEndOfFileMarks(std::string_view line)
{
    const std::string_view text = trimmed(line);
    return !text.empty() && text.find_first_not_of('\x1a') == std::string_view::npos;
}

// What each record type is, in [SDR] 3.6.2's words, for the warnings that
// name a record - a deleted one, or one of a type this reader does not
// import; empty for a type [SDR] does not list.
std::string_view recordName(std::string_view type)
{
    struct Named {
        std::string_view type;
        std::string_view name;
    };
    static constexpr Named kNames[] = {
        {"00", "header"},
        {"01", "instrument"},
        {"02", "station"},
        {"03", "target height"},
        {"04", "collimation"},
        {"05", "atmosphere"},
        {"06", "scale factor"},
        {"07", "backsight"},
        {"08", "coordinates"},
        {"09", "observation"},
        {"10", "job"},
        {"11", "reduced observation"},
        {"12", "set"},
        {"13", "note"},
        {"14", "GPS instrument"},
        {"15", "GPS station"},
        {"16", "GPS observation"},
        {"17", "GPS reduced observation"},
        {"18", "GPS position"},
        {"19", "GPS projection"},
        {"21", "GPS vertical adjustment"},
        {"24", "GPS latitude and longitude"},
        {"25", "road station"},
        {"26", "road position"},
        {"27", "road check"},
        {"28", "road name"},
        {"29", "horizontal alignment"},
        {"30", "horizontal alignment point"},
        {"31", "horizontal straight"},
        {"32", "horizontal arc"},
        {"33", "horizontal spiral"},
        {"34", "vertical alignment"},
        {"35", "circular vertical curve"},
        {"36", "parabolic vertical curve"},
        {"37", "vertical alignment point"},
        {"38", "cross section"},
        {"39", "template"},
        {"40", "template offset and height difference"},
        {"41", "template grade and distance"},
        {"42", "template side slope"},
        {"44", "apply superelevation"},
        {"45", "define superelevation"},
        {"46", "template element"},
        {"47", "template side slope"},
        {"50", "GPS horizontal adjustment"},
        {"57", "antenna height"},
        {"60", "levelling"},
        {"61", "levelling"},
        {"62", "levelling"},
        {"63", "levelling"},
        {"64", "levelling"},
        {"65", "levelling offset"},
        {"66", "GPS raw observation"},
        {"96", "latitude and longitude station"},
        {"97", "transformation"},
        {"98", "WGS84 latitude and longitude"},
        {"99", "local latitude and longitude"},
    };
    for (const Named& named : kNames) {
        if (named.type == type) {
            return named.name;
        }
    }
    return {};
}

// "01NM (instrument)": a record's type, derivation code and name.
std::string recordLabel(std::string_view record)
{
    const std::string_view name = recordName(record.substr(0, 2));
    return std::string(record.substr(0, 4)) +
           (name.empty() ? std::string{} : " (" + std::string(name) + ")");
}

// `text` as a warning quotes it: each control byte written "\xNN", since a
// byte a person cannot see must not become a blank they cannot tell from one.
std::string shown(std::string_view text)
{
    static constexpr std::string_view kHex = "0123456789ABCDEF";
    std::string out;
    for (const char c : text) {
        if (isControl(c)) {
            const auto byte = static_cast<unsigned char>(c);
            out += "\\x";
            out += kHex[byte >> 4];
            out += kHex[byte & 0x0F];
        } else {
            out += c;
        }
    }
    return out;
}

// A text field as something to keep: control bytes blanked, THEN trimmed, so
// a field of nothing but line noise is empty rather than a blank.
std::string cleaned(std::string_view field)
{
    std::string text(field);
    for (char& c : text) {
        if (isControl(c)) {
            c = ' ';
        }
    }
    return std::string(trimmed(text));
}

// `text` with every run of blanks made one: "SDR33     V04-03" is
// "SDR33 V04-03".
std::string collapsedBlanks(std::string_view text)
{
    std::string collapsed;
    for (const char c : trimmed(text)) {
        if (c != ' ' || collapsed.back() != ' ') {
            collapsed += c;
        }
    }
    return collapsed;
}

// A header record's version (columns 5-20), its runs of blanks made one - the
// one reading of it, for the metadata and the provenance alike. Empty for a
// record too short to have one.
std::string versionOf(std::string_view header)
{
    return header.size() > 4 ? collapsedBlanks(cleaned(header.substr(4, 16))) : std::string{};
}

// The fields of one record, read left to right from its fifth character
// ([SDR] chapter 3: characters 1-4 are the type and the derivation code).
// A column is a CHARACTER: the file was decoded to UTF-8 first, so a name
// typed in another code page is more than one byte and the fields after it
// would shift if bytes were counted. A record shortened by dropped trailing
// blanks ([SDR] chapter 5) simply has shorter fields, empty past its end.
class Fields {
  public:
    Fields(std::string_view record, std::size_t pointIdWidth, std::size_t realWidth)
        : record_(record), pointIdWidth_(pointIdWidth), realWidth_(realWidth)
    {
        for (const char c : record) {
            if (static_cast<unsigned char>(c) >= 0x80) {
                ascii_ = false;
                break;
            }
        }
        if (!ascii_) {
            for (std::size_t i = 0; i < record.size(); ++i) {
                if ((static_cast<unsigned char>(record[i]) & 0xC0) != 0x80) {
                    starts_.push_back(i);
                }
            }
        }
    }

    std::string_view take(std::size_t width)
    {
        const std::size_t count = ascii_ ? record_.size() : starts_.size();
        const std::size_t first = std::min(column_, count);
        const std::size_t last = std::min(column_ + width, count);
        column_ += width;
        return record_.substr(offset(first), offset(last) - offset(first));
    }
    std::string_view pointId() { return take(pointIdWidth_); }
    std::string_view real() { return take(realWidth_); }

  private:
    [[nodiscard]] std::size_t offset(std::size_t character) const
    {
        if (ascii_) {
            return character;
        }
        return character < starts_.size() ? starts_[character] : record_.size();
    }

    std::string_view record_;
    std::size_t pointIdWidth_ = 16;
    std::size_t realWidth_ = 16;
    std::size_t column_ = 4;
    bool ascii_ = true;
    std::vector<std::size_t> starts_;
};

// The first word of a description, which is its field code.
std::string_view firstWord(std::string_view description)
{
    const std::size_t blank = description.find(' ');
    return blank == std::string_view::npos ? description : description.substr(0, blank);
}

// ---- Time stamps --------------------------------------------------------------

std::optional<int> digitsValue(std::string_view text)
{
    if (text.empty() || text.size() > 4) {
        return std::nullopt;
    }
    int value = 0;
    for (const char c : text) {
        if (!isDigit(c)) {
            return std::nullopt;
        }
        value = value * 10 + (c - '0');
    }
    return value;
}

std::optional<int> monthNamed(std::string_view text)
{
    static constexpr std::array<std::string_view, 12> kMonths = {
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
    for (std::size_t i = 0; i < kMonths.size(); ++i) {
        if (katana::core::equalsIgnoringCase(text, kMonths[i])) {
            return static_cast<int>(i) + 1;
        }
    }
    return std::nullopt;
}

// "HH:MM" or "HH:MM:SS" into `time`.
bool readClock(std::string_view text, survey::SurveyTimestamp& time)
{
    const std::size_t first = text.find(':');
    if (first == std::string_view::npos) {
        return false;
    }
    const std::size_t second = text.find(':', first + 1);
    const bool withSeconds = second != std::string_view::npos;
    const std::optional<int> hour = digitsValue(text.substr(0, first));
    const std::optional<int> minute = digitsValue(
        text.substr(first + 1, withSeconds ? second - first - 1 : text.npos));
    const std::optional<int> seconds =
        withSeconds ? digitsValue(text.substr(second + 1)) : std::optional<int>(0);
    if (!hour || !minute || !seconds || *hour > 23 || *minute > 59 || *seconds > 60) {
        return false;
    }
    time.hour = *hour;
    time.minute = *minute;
    time.second = *seconds;
    return true;
}

// A day of the Gregorian calendar, by the standard library's own (C++20
// <chrono>'s proleptic Gregorian calendar, leap years included) rather than
// another copy of the month lengths. The callers' values are at most four
// digits each; std::chrono::month and day keep only a byte of what they are
// given (day 271 would be day 15), so the ranges are checked first.
bool plausibleDate(int year, int month, int day)
{
    if (year <= 0 || month < 1 || month > 12 || day < 1 || day > 31) {
        return false;
    }
    return std::chrono::year_month_day{std::chrono::year{year},
                                       std::chrono::month{static_cast<unsigned>(month)},
                                       std::chrono::day{static_cast<unsigned>(day)}}
        .ok();
}

// A 13TS note's date and time (see the top of this file for the forms).
std::optional<survey::SurveyTimestamp> timeStampOf(std::string_view text)
{
    survey::SurveyTimestamp time;
    text = trimmed(text);
    // [TA]: "Time Date MM/DD/YYYY Time HH:MM:SS", as "Time Date 03/14/2026
    // Time 08:15:00".
    if (const std::size_t date = text.find("Date "); date != std::string_view::npos) {
        std::string_view rest = trimmed(text.substr(date + 5));
        const std::size_t end = rest.find(' ');
        const std::string_view day = rest.substr(0, end);
        const std::size_t slash1 = day.find('/');
        const std::size_t slash2 =
            slash1 == std::string_view::npos ? slash1 : day.find('/', slash1 + 1);
        if (slash2 == std::string_view::npos) {
            return std::nullopt;
        }
        const std::optional<int> month = digitsValue(day.substr(0, slash1));
        const std::optional<int> dayOfMonth =
            digitsValue(day.substr(slash1 + 1, slash2 - slash1 - 1));
        const std::optional<int> year = digitsValue(day.substr(slash2 + 1));
        if (!month || !dayOfMonth || !year || day.substr(slash2 + 1).size() != 4 ||
            !plausibleDate(*year, *month, *dayOfMonth)) {
            return std::nullopt;
        }
        time.year = *year;
        time.month = *month;
        time.day = *dayOfMonth;
        rest = end == std::string_view::npos ? std::string_view{} : trimmed(rest.substr(end));
        if (rest.starts_with("Time ")) {
            if (!readClock(trimmed(rest.substr(5)), time)) {
                return std::nullopt;
            }
        }
        return time;
    }
    // The field book's own: "18-Jan-80 20:14", and "May-12-97 10:53".
    const std::size_t blank = text.find(' ');
    const std::string_view day = text.substr(0, blank);
    const std::size_t dash1 = day.find('-');
    const std::size_t dash2 = dash1 == std::string_view::npos ? dash1 : day.find('-', dash1 + 1);
    if (dash2 == std::string_view::npos) {
        return std::nullopt;
    }
    const std::string_view a = day.substr(0, dash1);
    const std::string_view b = day.substr(dash1 + 1, dash2 - dash1 - 1);
    const std::string_view c = day.substr(dash2 + 1);
    const bool dayFirst = monthNamed(b).has_value();
    const std::optional<int> month = dayFirst ? monthNamed(b) : monthNamed(a);
    const std::optional<int> dayOfMonth = digitsValue(dayFirst ? a : b);
    const std::optional<int> shortYear = c.size() == 2 ? digitsValue(c) : std::nullopt;
    if (!month || !dayOfMonth || !shortYear) {
        return std::nullopt;
    }
    // POSIX strptime %y: 69-99 are 1969-1999, 00-68 are 2000-2068.
    const int year = *shortYear >= 69 ? 1900 + *shortYear : 2000 + *shortYear;
    if (!plausibleDate(year, *month, *dayOfMonth)) {
        return std::nullopt;
    }
    time.year = year;
    time.month = *month;
    time.day = *dayOfMonth;
    if (blank != std::string_view::npos && !readClock(trimmed(text.substr(blank)), time)) {
        return std::nullopt;
    }
    return time;
}

// ---- The reader -----------------------------------------------------------------

// One setup's backsight, as its 07 records gave it.
struct Orientation {
    bool set = false;
    std::string backsight;
    std::optional<double> azimuth; // radians
    std::optional<double> reading; // the first round's circle reading, radians
    std::size_t rounds = 0;
    std::size_t record = 0;
};

// An azimuth keyed from the setup's point with no distance (an 11): what
// orients a setup that has no 07.
struct KeyedAzimuth {
    std::string to;
    double azimuth = 0.0; // radians
    std::size_t record = 0;
};

// The corrections of the latest 04 ([SETX] 29.2.5), radians; record 0 until
// one is read.
struct Collimation {
    double vertical = 0.0;
    double horizontal = 0.0;
    std::size_t record = 0;
};

// What the observations before a setup's first 07 say of its backsight:
// whether any is of it, and how many of their horizontal readings on it are
// on the 07's circle and how many are not.
struct BacksightReadings {
    bool observed = false;
    std::size_t onCircle = 0;
    std::size_t offCircle = 0;
};

// A 09 MC or an 11 with distances from the setup's point whose line no raw
// observation has repeated yet: its verdict waits for the setup's end.
struct PendingDerivedView {
    std::size_t record = 0;
    bool corrected = true; // an MC; otherwise a RED
    std::string from;
    std::string to;
};

// The northing, easting and elevation a 02 or 08 states, metres.
struct StatedCoordinates {
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> elevation;
};

// An observation record read - a 09 F1, F2, MD or MC, or an 11 with
// distances - whatever the reader then did with it: its record, its target,
// what a warning calls it and the view the field book keeps it in; record 0
// for none.
struct ShotRecord {
    std::size_t record = 0;
    std::string to;
    std::string_view what;
    std::string_view view;
};

class SdrReader {
  public:
    SdrReader(std::string_view fileName, survey::SourceRecord source, const ReadOptions& options)
        : builder_(fileName, std::move(source), options)
    {
        // [SETX] 29.1: curvature and refraction are applied between a raw
        // observation and its corrected (MC) view, so never in an F1, F2 or
        // MD reading - whatever the job's switch, which is about that view.
        settings_.curvatureRefractionState = survey::CorrectionState::NotApplied;
        // [SDR] 2.3: the latest coordinates of a point are the best.
        builder_.setRestatedCoordinates(RawProjectBuilder::RestatedCoordinates::LatestKept);
    }

    // `endsWithLineEnd`: whether the text's last character ends a line, so
    // its last record was not cut short in the middle.
    Result<ReadResult> read(const std::vector<std::string_view>& lines, bool guessedEncoding,
                            katana::core::TextEncoding encoding, bool endsWithLineEnd);

  private:
    void dispatch(std::string_view line, std::size_t n);
    void readHeader(std::string_view line, std::size_t n);
    void readInstrument(Fields& fields, std::size_t n);
    void readStation(Fields& fields, std::string_view derivation, std::size_t n);
    void readTarget(Fields& fields, std::size_t n);
    void readCollimation(Fields& fields, std::size_t n);
    void readAtmosphere(Fields& fields, std::size_t n);
    void readScale(Fields& fields, std::size_t n);
    void readBacksight(Fields& fields, std::string_view derivation, std::size_t n);
    void readPosition(Fields& fields, std::string_view derivation, std::size_t n);
    void readObservation(Fields& fields, std::string_view derivation, std::size_t n);
    void readJob(Fields& fields, std::size_t n);
    void readReduced(Fields& fields, std::size_t n);
    void readSet(Fields& fields, std::size_t n);
    void readNote(Fields& fields, std::string_view derivation, std::size_t n);
    void readDistanceUnitNote(std::string_view text, std::size_t n);
    void readCorrectionNote(std::string_view text);

    std::optional<double> number(std::string_view field, std::string_view what, std::size_t n);
    std::optional<double> angle(std::string_view field, std::string_view what, std::size_t n);
    std::optional<double> length(std::string_view field, std::string_view what, std::size_t n);
    // True, with the record skipped and the byte named, when one of the
    // point id fields holds a control byte.
    bool refusedIds(std::size_t n, std::initializer_list<std::string_view> fields);
    // The coordinates a 02 or 08 states for `id`, read in the header's
    // order; nullopt when they cannot place it (a height alone is kept in
    // the point's metadata, one ordinate alone said), or refused.
    std::optional<StatedCoordinates> statedCoordinates(std::string_view id,
                                                       std::string_view first,
                                                       std::string_view second,
                                                       std::string_view elevation, std::size_t n);
    void coordinates(std::string_view id, std::string_view first, std::string_view second,
                     std::string_view elevation, survey::CoordinateSource how, std::size_t n);
    // The settings a setup begun now starts with, and the current setup's
    // when it has no observation yet (an 01, 05 or 06 read between its 02
    // and its first shot belongs to it).
    void settingsChanged(std::size_t n, std::string_view what);
    // What a setup's records have not said about its orientation, said
    // before the next one begins: a keyed azimuth, or none at all. `end`:
    // the setup keeps its observations before it, the rest going to the
    // setup a 07 begins, so only those are what it observes.
    void finishSetup(std::size_t end = std::string::npos);
    // A 09 MC or an 11 with distances: skipped now when it cannot have a raw
    // twin or already has one, otherwise left for settleDerivedViews().
    void readDerivedView(PendingDerivedView view);
    // Skips it with its reason: `repeated`, a raw observation of its line
    // carries its shot; otherwise its shot is lost, and counted so.
    void skipDerivedView(const PendingDerivedView& view, bool repeated);
    // The verdict on each derived view a setup left waiting: its line
    // observed raw from the setup's point - counted twice - or its shot lost.
    void settleDerivedViews();
    void resetSetup();
    // A setup on the current one's point, with its instrument height and the
    // settings in force now, taking the current one's observations from
    // `first` on, their pointings numbered anew: they were read on the
    // orientation the record at `n` gives.
    survey::SurveyStation& splitSetup(std::size_t first, std::size_t n);
    // The raw records of the current setup before observation `first`.
    [[nodiscard]] std::size_t recordsBefore(std::size_t first) const;
    // The horizontal reading of the current setup's raw record `record` (an
    // index into recordStarts_), or nullptr when it has none.
    [[nodiscard]] const survey::HorizontalDirectionObservation*
    directionOfRecord(const survey::SurveyStation& station, std::size_t record) const;
    // What the setup's observations before `first` say of backsight `to`,
    // against the circle reading `reading` a first 07 gives it.
    [[nodiscard]] BacksightReadings backsightBefore(const survey::SurveyStation& station,
                                                    std::size_t first, std::string_view to,
                                                    std::optional<double> reading) const;
    // Where a 07 that begins a new setup takes its observations from: the
    // end, or the first of the shots of its backsight `to` just before it
    // read on its circle `reading` (any, with none) and not on the setup's
    // (see the top of this file).
    [[nodiscard]] std::size_t newCircleStart(const survey::SurveyStation& station,
                                             std::string_view to,
                                             std::optional<double> reading) const;
    // For a setup whose 07s give no circle reading: a warning when its
    // readings of its backsight among its first `kept` observations are
    // more than a minute apart on one face, since nothing else can show its
    // circle moved between its rounds.
    void warnOfBacksightSpread(const survey::SurveyStation& station, std::size_t kept);
    void refuse(std::size_t n, std::string message);

    RawProjectBuilder builder_;
    std::optional<katana::core::Error> fatal_;

    // The header.
    bool headerRead_ = false;
    std::size_t headerRecord_ = 0;
    std::string headerOptions_;
    Variant variant_ = Variant::Sdr33;
    // Written as [NIKON] writes its header: no blank before the version.
    bool nikonForm_ = false;
    std::size_t pointIdWidth_ = 16;
    std::size_t realWidth_ = 16;
    AngleUnit angleUnit_ = AngleUnit::Degrees;
    survey::LinearUnit linearUnit_ = survey::LinearUnit::Metres;
    katana::math::UnitRatio metres_ = katana::math::units::kMetre;
    PressureUnit pressureUnit_ = PressureUnit::Unknown;
    TemperatureUnit temperatureUnit_ = TemperatureUnit::Unknown;
    CoordinateOrder order_ = CoordinateOrder::Unknown;
    char orderOption_ = ' ';
    bool orderWarned_ = false; // option 2's or 3's doubt, said once

    // The job.
    bool jobRead_ = false;
    bool recordElevations_ = true;
    std::string atmosphericSwitch_; // "on", "off" or empty when the file does not say
    // A 13TS note in [TA]'s form was read.
    bool writerTimeStamps_ = false;

    // What applies to the next observation.
    survey::InstrumentSettings settings_{};
    bool instrumentSeen_ = false;
    bool instrumentWarned_ = false;
    VerticalReference vertical_ = VerticalReference::Zenith;
    double targetHeight_ = 0.0;
    bool targetHeightSeen_ = false;
    bool targetHeightWarned_ = false;
    bool badSet_ = false;
    std::optional<std::size_t> badSetRemaining_; // the bad set's raw observations still to come
    std::string badSetName_;
    std::size_t badSetRecord_ = 0;
    Collimation collimation_{};
    // The EDM type option of the latest 01 that gave one: the instrument
    // type a collimation holds for ([SETX] 13.1).
    std::string edmType_;
    // The 01 that last ended a collimation by another instrument type, and
    // that collimation's 04: for the warning at a shot of the same setup
    // after it.
    std::size_t collimationEndedAt_ = 0;
    std::size_t endedCollimation_ = 0;
    // The largest horizontal collimation correction applied to a reading
    // since the latest 02, radians: how far a corrected reading and a 07's
    // circle reading on one line may differ with the circle unmoved.
    double appliedCollimation_ = 0.0;

    // The current setup.
    Orientation orientation_{};
    std::vector<KeyedAzimuth> keyed_;
    // Its raw (F1, F2, MD) observation records read.
    std::size_t setupObservations_ = 0;
    // Where its latest set began among its observations, and whether a 07
    // has been read since: a 07 SC after a set's shots closes that set.
    std::optional<std::size_t> setStart_;
    std::size_t setStartRecord_ = 0;
    bool backsightSinceSet_ = false;
    // An SDR2x 12 read after raw records: where the set it would close
    // begins, and how many records that is, for its 07 SC to choose.
    std::optional<std::size_t> setBefore_;
    std::size_t setBeforeRecords_ = 0;
    // Where a set the SDR2x layout closes with its 12 could have begun: the
    // setup's start, its last 07 or 12, or a "Set #" note.
    std::size_t setBoundary_ = 0;
    // The first observation of each of its raw records, in order.
    std::vector<std::size_t> recordStarts_;
    // Each 01, 05 or 06 read after the setup's first observation: it belongs
    // to the next setup unless another shot of this one follows.
    std::vector<std::pair<std::string, std::size_t>> pendingChanges_;
    bool weatherSeen_ = false;
    bool nonAscii_ = false;

    // Since the latest 02: the targets observed raw from its point, whatever
    // setup a 07 put them in - what an MC or RED is the reduction of - and
    // the derived views whose line none of them has observed yet.
    std::set<std::string, std::less<>> occupationTargets_;
    std::vector<PendingDerivedView> pendingDerived_;
    // Derived views with no raw twin in their setup: the shots are lost,
    // which the file's summary says.
    std::size_t lostCorrected_ = 0;
    std::size_t lostReduced_ = 0;

    // The observation record read last, if the record read last was one,
    // and the one before the record being read, if that was one: an 08 of
    // its target straight after it is its POS view.
    ShotRecord lastShot_;
    ShotRecord previousShot_;
    // Points given coordinates by a position record - a 02, or an 08 that is
    // no observation's POS view - which an observation's POS view never
    // supersedes ([SETX] 6.1 rule 2).
    std::set<std::string, std::less<>> positionRecorded_;
};

void SdrReader::refuse(std::size_t n, std::string message)
{
    if (!fatal_) {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() + ": " +
                               std::move(message),
                           variant_ == Variant::Sdr33 ? "Sokkia SDR33" : "Sokkia SDR2x");
    }
}

// [SDR] 3.3 (i): "an optional minus character, followed by a sequence of at
// least one digit ..., optionally followed by a decimal point ..., optionally
// followed by one or more digits" - no plus, no exponent. A leading point
// (".5") is let through: it is one number however it is read.
bool isSdrReal(std::string_view text)
{
    if (text.starts_with('-')) {
        text.remove_prefix(1);
    }
    std::size_t digits = 0;
    bool point = false;
    for (const char c : text) {
        if (isDigit(c)) {
            ++digits;
        } else if (c == '.' && !point) {
            point = true;
        } else {
            return false;
        }
    }
    return digits > 0;
}

std::optional<double> SdrReader::number(std::string_view field, std::string_view what,
                                        std::size_t n)
{
    // [SDR] 3.3 (ii): a field of blanks is null, "not measured".
    const std::string_view text = trimmed(field);
    if (text.empty()) {
        return std::nullopt;
    }
    const std::optional<double> value =
        isSdrReal(text) ? parseReal(text) : std::optional<double>{};
    if (!value) {
        builder_.warn(n, std::string(what) + " '" + shown(text.substr(0, 40)) +
                             "' is not a number; it is read as not measured");
    }
    return value;
}

std::optional<double> SdrReader::angle(std::string_view field, std::string_view what,
                                       std::size_t n)
{
    const std::optional<double> value = number(field, what, n);
    if (!value) {
        return std::nullopt;
    }
    switch (angleUnit_) {
    case AngleUnit::Degrees:
        return survey::degreesToRadians(*value);
    case AngleUnit::Gons:
        return survey::gonToRadians(*value);
    case AngleUnit::Mils:
        return *value * kRadiansPerMil;
    }
    return std::nullopt;
}

std::optional<double> SdrReader::length(std::string_view field, std::string_view what,
                                        std::size_t n)
{
    const std::optional<double> value = number(field, what, n);
    if (!value) {
        return std::nullopt;
    }
    return katana::math::toMetres(*value, metres_);
}

// Trimmed at both ends ([SDR] 3.2 pads alpha fields on the right, and
// writers are met that pad them on the left).
std::string pointIdOf(std::string_view field)
{
    return std::string(trimmed(field));
}

bool SdrReader::refusedIds(std::size_t n, std::initializer_list<std::string_view> fields)
{
    for (const std::string_view field : fields) {
        for (const char c : field) {
            if (isControl(c)) {
                builder_.skip(n, "a point id holds the control byte " + shown(std::string(1, c)) +
                                     ", which no point name can; the record is not imported");
                return true;
            }
        }
    }
    return false;
}

std::optional<StatedCoordinates> SdrReader::statedCoordinates(std::string_view id,
                                                              std::string_view first,
                                                              std::string_view second,
                                                              std::string_view elevation,
                                                              std::size_t n)
{
    const bool firstStated = !trimmed(first).empty();
    const bool secondStated = !trimmed(second).empty();
    // [SETX] 4: with the job's "Record elev" No every point has "the same
    // (indeterminate) elevation" - a placeholder, not a height.
    if (!firstStated && !secondStated) {
        // A height alone cannot place a point; it is kept, not dropped.
        if (const std::optional<double> height =
                recordElevations_ ? length(elevation, "elevation", n) : std::nullopt) {
            builder_.addPointMetadata(id, "height without a position", formatExactReal(*height));
            builder_.warn(n, "point '" + std::string(id) + "' is given an elevation (" +
                                 formatExactReal(*height) +
                                 " m) and no northing or easting; a height alone cannot place "
                                 "a point, so it is kept in the point's metadata");
        }
        return std::nullopt;
    }
    if (order_ == CoordinateOrder::Unknown) {
        refuse(n, "the record states coordinates, and the header's coordinate order option is '" +
                      shown(std::string(1, orderOption_)) +
                      "'; the format defines 1 (north first) and 2 (east first), and Katana "
                      "will not guess which ordinate is the northing");
        return std::nullopt;
    }
    const bool northFirst = order_ == CoordinateOrder::NorthFirst;
    // Neither east-first option is settled by the field book's own files
    // (see the top of this file): said once, at the first coordinates read.
    if (!orderWarned_ && (orderOption_ == '2' || orderOption_ == '3')) {
        orderWarned_ = true;
        builder_.warn(
            n, orderOption_ == '2'
                   ? std::string(
                         "the header's coordinate order option is 2, east-north-elevation, "
                         "and the coordinates are read easting first, as Trimble's published "
                         "writer writes them; the format's record layout names the first field "
                         "the northing, and no source shows which order Sokkia's field book "
                         "sends under 2 (its one example is a printed report), so a file of its "
                         "may hold the northing first - check a known point")
                   : std::string(
                         "the header's coordinate order option is 3, which the format's "
                         "publisher does not define: Trimble's published writer uses it for "
                         "Y-X-Z, the easting first, and the coordinates are read so; Sokkia's "
                         "field book lists south-west-elevation as a third display order, so a "
                         "file set to that would be read wrongly - check a known point"));
    }
    const std::optional<double> a = length(first, northFirst ? "northing" : "easting", n);
    const std::optional<double> b = length(second, northFirst ? "easting" : "northing", n);
    const std::optional<double> z =
        recordElevations_ ? length(elevation, "elevation", n) : std::nullopt;
    if (!a || !b) {
        if (a || b) {
            builder_.warn(n, "point '" + std::string(id) +
                                 "' is given only one of its northing and easting; it is "
                                 "imported without coordinates");
        }
        return std::nullopt;
    }
    return StatedCoordinates{northFirst ? *a : *b, northFirst ? *b : *a, z};
}

void SdrReader::coordinates(std::string_view id, std::string_view first, std::string_view second,
                            std::string_view elevation, survey::CoordinateSource how,
                            std::size_t n)
{
    if (const std::optional<StatedCoordinates> stated =
            statedCoordinates(id, first, second, elevation, n)) {
        builder_.positionPoint(id, stated->northing, stated->easting, stated->elevation, how, n);
        // A position record: coordinates the field book uses before any
        // observation's ([SETX] 6.1 rule 2).
        positionRecorded_.emplace(id);
    }
}

void SdrReader::settingsChanged(std::size_t n, std::string_view what)
{
    survey::SurveyStation* station = builder_.currentStation();
    if (station != nullptr && setupObservations_ == 0) {
        const survey::SurveyTimestamp time = station->instrument.time;
        station->instrument = settings_;
        station->instrument.time = time;
        return;
    }
    // Between one setup's shots and the next 02 is where the field book
    // writes the next setup's instrument and weather; only a shot of the
    // SAME setup after it says the values changed in the middle of one.
    // Every such record is kept, so each is reported.
    if (station != nullptr) {
        pendingChanges_.emplace_back(std::string(what), n);
    }
}

void SdrReader::resetSetup()
{
    orientation_ = Orientation{};
    keyed_.clear();
    setupObservations_ = 0;
    setStart_.reset();
    setStartRecord_ = 0;
    backsightSinceSet_ = false;
    setBefore_.reset();
    setBeforeRecords_ = 0;
    setBoundary_ = 0;
    recordStarts_.clear();
    pendingChanges_.clear();
}

std::size_t SdrReader::recordsBefore(std::size_t first) const
{
    return static_cast<std::size_t>(
        std::lower_bound(recordStarts_.begin(), recordStarts_.end(), first) -
        recordStarts_.begin());
}

const survey::HorizontalDirectionObservation*
SdrReader::directionOfRecord(const survey::SurveyStation& station, std::size_t record) const
{
    const std::size_t begin = recordStarts_[record];
    const std::size_t end =
        record + 1 < recordStarts_.size() ? recordStarts_[record + 1] : station.observations.size();
    for (std::size_t i = begin; i < end; ++i) {
        if (const auto* direction =
                std::get_if<survey::HorizontalDirectionObservation>(&station.observations[i])) {
            return direction;
        }
    }
    return nullptr;
}

BacksightReadings SdrReader::backsightBefore(const survey::SurveyStation& station,
                                             std::size_t first, std::string_view to,
                                             std::optional<double> reading) const
{
    BacksightReadings found;
    for (std::size_t i = 0; i < first; ++i) {
        std::visit(
            [&](const auto& o) {
                if constexpr (requires { o.to; }) {
                    found.observed = found.observed || o.to == to;
                }
            },
            station.observations[i]);
        const auto* direction =
            std::get_if<survey::HorizontalDirectionObservation>(&station.observations[i]);
        if (reading && direction != nullptr && direction->to == to) {
            ++(onSameCircle(faceOneReading(*direction), *reading, appliedCollimation_)
                   ? found.onCircle
                   : found.offCircle);
        }
    }
    return found;
}

std::size_t SdrReader::newCircleStart(const survey::SurveyStation& station, std::string_view to,
                                      std::optional<double> reading) const
{
    const std::size_t end = station.observations.size();
    // Only the setup's own backsight has a reading the old circle gave it:
    // read elsewhere - on the new record's circle, or off the old one where
    // the new record gives none - it shows the circle moved. Another point
    // read on the new circle shows only that the two records' azimuths
    // disagree, and [SETX] 8.2 has the old record orient it.
    if (to != orientation_.backsight) {
        return end;
    }
    // The old circle: the setup's 07's reading on the backsight or, with
    // none, the setup's first reading of it. A first reading that is one of
    // the shots below is on it, so none of them moves.
    std::optional<double> old = orientation_.reading;
    for (std::size_t record = 0; !old && record < recordStarts_.size(); ++record) {
        if (const survey::HorizontalDirectionObservation* direction =
                directionOfRecord(station, record);
            direction != nullptr && direction->to == to) {
            old = faceOneReading(*direction);
        }
    }
    if (!old) {
        return end;
    }
    std::size_t record = recordStarts_.size();
    const std::size_t floor = recordsBefore(setBoundary_);
    while (record > floor) {
        const survey::HorizontalDirectionObservation* direction =
            directionOfRecord(station, record - 1);
        if (direction == nullptr || direction->to != to) {
            break;
        }
        const double faceOne = faceOneReading(*direction);
        if ((reading && !onSameCircle(faceOne, *reading, appliedCollimation_)) ||
            onSameCircle(faceOne, *old, appliedCollimation_)) {
            break;
        }
        --record;
    }
    return record < recordStarts_.size() ? recordStarts_[record] : end;
}

void SdrReader::warnOfBacksightSpread(const survey::SurveyStation& station, std::size_t kept)
{
    // Face by face: a face 2 reading less half a circle differs from face
    // 1's by twice the instrument's collimation error where no 04 corrects
    // it, which is not the circle moving. The slack is for a 04 read
    // between two of them, correcting one and not the other.
    const survey::HorizontalDirectionObservation* first[2] = {nullptr, nullptr};
    for (std::size_t i = 0; i < kept; ++i) {
        const auto* direction =
            std::get_if<survey::HorizontalDirectionObservation>(&station.observations[i]);
        if (direction == nullptr || direction->to != orientation_.backsight) {
            continue;
        }
        const std::size_t face = direction->pointing.face == survey::Face::Right ? 1 : 0;
        if (first[face] == nullptr) {
            first[face] = direction;
        } else if (!onSameCircle(direction->direction, first[face]->direction,
                                 appliedCollimation_)) {
            builder_.warn(direction->source.recordNumber,
                          "setup '" + station.setup.id + "' reads its backsight '" +
                              orientation_.backsight +
                              "' here more than a minute of arc from its reading at record " +
                              std::to_string(first[face]->source.recordNumber) +
                              " on the same face, and its backsight records give no circle "
                              "reading to tell whether the circle was moved between them: they "
                              "are read as one setup, oriented on the mean of those readings");
            return;
        }
    }
}

void SdrReader::skipDerivedView(const PendingDerivedView& view, bool repeated)
{
    if (repeated) {
        builder_.skip(view.record,
                      view.corrected
                          ? "a corrected observation (MC) is the field software's reduction of "
                            "raw observations this setup holds of '" +
                                view.to +
                                "' - oriented, and reduced for the heights, the prism constant "
                                "and the weather ([SETX] 29.1); it is not imported, since it "
                                "would count them twice"
                          : "a reduced observation (azimuth, horizontal and vertical distance) "
                            "is the field software's reduction of raw observations this setup "
                            "holds of '" +
                                view.to +
                                "', the scale factor and sea level applied ([SETX] 29.1); it is "
                                "not imported, since it would count them twice");
        return;
    }
    ++(view.corrected ? lostCorrected_ : lostReduced_);
    builder_.skip(view.record,
                  view.corrected
                      ? "a corrected observation (MC) from '" + shown(view.from) + "' to '" +
                            shown(view.to) +
                            "', which no raw observation of its setup repeats, is not imported: "
                            "writers of the format put different numbers in one (Sokkia's field "
                            "book a mark-to-mark vector already oriented and corrected, "
                            "Trimble's writer the inverse of computed coordinates, the Nikon and "
                            "Spectra manuals the instrument's slope distance and angles), so "
                            "this shot is lost"
                      : "a reduced observation (azimuth, horizontal and vertical distance) from '" +
                            view.from + "' to '" + view.to +
                            "', which no raw observation of its setup repeats, is not imported: "
                            "it carries the scale factor and the sea-level reduction the "
                            "reduction would apply again, and writers of the format differ in "
                            "what else, so this shot is lost");
}

void SdrReader::readDerivedView(PendingDerivedView view)
{
    // Its raw twin is an observation of the same line from the setup's
    // point, before it or after; from anywhere else nothing can carry it.
    const survey::SurveyStation* current = builder_.currentStation();
    if (current == nullptr || view.from != current->setup.pointId) {
        skipDerivedView(view, false);
    } else if (occupationTargets_.contains(view.to)) {
        skipDerivedView(view, true);
    } else {
        pendingDerived_.push_back(std::move(view));
    }
}

void SdrReader::settleDerivedViews()
{
    for (const PendingDerivedView& view : pendingDerived_) {
        skipDerivedView(view, occupationTargets_.contains(view.to));
    }
    pendingDerived_.clear();
}

void SdrReader::finishSetup(std::size_t end)
{
    survey::SurveyStation* station = builder_.currentStation();
    if (station == nullptr) {
        return;
    }
    // The targets of its directions, gathered once: looked up for each keyed
    // azimuth, a scan of the observations each time would cost the product
    // of the two.
    std::set<std::string_view> directed;
    const std::size_t kept = std::min(end, station->observations.size());
    for (std::size_t i = 0; i < kept; ++i) {
        if (const auto* direction =
                std::get_if<survey::HorizontalDirectionObservation>(&station->observations[i])) {
            directed.insert(direction->to);
        }
    }
    const auto directionTo = [&](std::string_view target) { return directed.contains(target); };
    if (orientation_.set) {
        if (!directionTo(orientation_.backsight)) {
            builder_.warn(orientation_.record,
                          "setup '" + station->setup.id + "' has no observation of its backsight '" +
                              orientation_.backsight + "'; " +
                              (orientation_.reading
                                   ? std::string("the reduction orients it by the backsight "
                                                 "record's azimuth less its circle reading on the "
                                                 "backsight ([SETX] 29.2.6)")
                                   : std::string("the backsight record gives no circle reading "
                                                 "on it either, so only the backsight's "
                                                 "coordinates can orient it")));
        } else if (!orientation_.reading) {
            warnOfBacksightSpread(*station, kept);
        }
        return;
    }
    if (!keyed_.empty()) {
        const KeyedAzimuth* chosen = &keyed_.front();
        for (const KeyedAzimuth& keyed : keyed_) {
            if (directionTo(keyed.to)) {
                chosen = &keyed;
                break;
            }
        }
        station->backsightPointId = chosen->to;
        station->statedBacksightAzimuth = chosen->azimuth;
        station->metadata["oriented by"] = "the azimuth to '" + chosen->to + "' keyed at record " +
                                           std::to_string(chosen->record);
        if (!directionTo(chosen->to)) {
            builder_.warn(chosen->record,
                          "setup '" + station->setup.id +
                              "' has no backsight record, and it does not observe '" + chosen->to +
                              "', the point of the azimuth keyed here; the reduction cannot "
                              "orient it");
        }
        return;
    }
    if (!directed.empty()) {
        // [SETX] 8.2.1: with the backsight skipped, "Horizontal angles
        // stored are treated as azimuths". A circle with no backsight named
        // is, to the model, one set to read azimuths; 0 is its reading on
        // north.
        station->backsightAzimuth = 0.0;
        station->metadata["oriented by"] =
            "no backsight record: its horizontal readings are azimuths ([SETX] 8.2.1)";
        builder_.warn(station->source.recordNumber,
                      "setup '" + station->setup.id +
                          "' has no backsight record and no keyed azimuth; its horizontal "
                          "readings are taken as azimuths, as the field book takes them when the "
                          "backsight is skipped ([SETX] 8.2.1)");
    }
}

survey::SurveyStation& SdrReader::splitSetup(std::size_t first, std::size_t n)
{
    std::vector<survey::SurveyStation>& stations = builder_.project().stations;
    const std::size_t index = stations.size() - 1;
    std::vector<survey::Observation> moved(
        std::make_move_iterator(stations[index].observations.begin() +
                                static_cast<std::ptrdiff_t>(first)),
        std::make_move_iterator(stations[index].observations.end()));
    stations[index].observations.erase(stations[index].observations.begin() +
                                           static_cast<std::ptrdiff_t>(first),
                                       stations[index].observations.end());
    const std::string point = stations[index].setup.pointId;
    const double height = stations[index].setup.instrumentHeight;
    const std::string previous = stations[index].setup.id;
    std::string setMetadata;
    if (first > 0 && setStart_ && *setStart_ == first) {
        const std::string key = "set at record " + std::to_string(setStartRecord_);
        if (const auto it = stations[index].metadata.find(key);
            it != stations[index].metadata.end()) {
            setMetadata = it->second;
            stations[index].metadata.erase(it);
        }
    }
    // beginStation may move `stations`: nothing above is used past here.
    survey::SurveyStation& station = builder_.beginStation(point, height, settings_, n);
    station.metadata["begun by"] = "the backsight record at record " + std::to_string(n) +
                                   ", which does not orient the observations of setup '" +
                                   previous + "' before it";
    if (!setMetadata.empty()) {
        station.metadata["set at record " + std::to_string(setStartRecord_)] = setMetadata;
    }
    // One record was one pointing; its observations keep sharing one.
    std::map<std::size_t, std::size_t> renumbered;
    for (survey::Observation& observation : moved) {
        std::visit(
            [&](auto& o) {
                if constexpr (requires { o.pointing.index; }) {
                    if (o.pointing.index != 0) {
                        const auto [it, added] = renumbered.try_emplace(o.pointing.index, 0);
                        if (added) {
                            it->second = builder_.nextPointing();
                        }
                        o.pointing.index = it->second;
                    }
                }
            },
            observation);
        station.observations.push_back(std::move(observation));
    }
    const std::optional<std::size_t> setStart =
        setStart_ && *setStart_ == first ? std::optional<std::size_t>(0) : std::nullopt;
    const std::size_t setRecord = setStartRecord_;
    const bool backsightSinceSet = backsightSinceSet_;
    resetSetup();
    setupObservations_ = renumbered.size();
    setStart_ = setStart;
    setStartRecord_ = setRecord;
    backsightSinceSet_ = backsightSinceSet;
    // Its raw records, as the new setup holds them: one record's
    // observations share a pointing and stand together.
    std::size_t lastPointing = 0;
    for (std::size_t i = 0; i < station.observations.size(); ++i) {
        std::visit(
            [&](const auto& o) {
                if constexpr (requires { o.pointing.index; }) {
                    if (o.pointing.index != lastPointing) {
                        lastPointing = o.pointing.index;
                        recordStarts_.push_back(i);
                    }
                }
            },
            station.observations[i]);
    }
    return station;
}

void SdrReader::readHeader(std::string_view line, std::size_t n)
{
    // White space after the options pads the record: [SDR] chapter 5 lets a
    // writer drop trailing blanks, and one that adds them, or a tab, says
    // nothing more.
    std::string_view text = line;
    while (!text.empty() && katana::core::isAsciiSpace(text.back())) {
        text.remove_suffix(1);
    }
    const std::string version = versionOf(text);
    const bool sdr33 = version.starts_with("SDR33");
    const bool sdr2x = !sdr33 && version.starts_with("SDR2");
    const Variant variant = sdr33 ? Variant::Sdr33 : Variant::Sdr2x;
    // [SDR] 3.6: 46 characters, the serial number in 21-24, the date and
    // time in 25-40 and the six options in 41-46. [TA]'s 2022 version writes
    // no serial number, so its header is 42 characters with the options in
    // 37-42. Both end with the options, so they are the last six characters;
    // a header of another length is read that way too, and said to be odd.
    const bool optionsLast =
        text.size() >= 42 && std::all_of(text.end() - 6, text.end(), isDigit);
    if (headerRead_) {
        // A second header: a second transmission appended, or a stray line.
        if (!(sdr33 || sdr2x) || !optionsLast) {
            builder_.skip(n, std::string("a header record that ") +
                                 (version.empty() ? "names no version"
                                                  : "is not one this reader can read") +
                                 " follows the file's own (record " +
                                 std::to_string(headerRecord_) +
                                 "); the units and layout that one gives hold");
            return;
        }
        // The same layout and units read on; any other would change what
        // every number after it means.
        if (text.substr(text.size() - 6) != headerOptions_ || variant != variant_) {
            refuse(n, "a second header record states other units or another layout than the "
                      "first (record " +
                          std::to_string(headerRecord_) + ")");
            return;
        }
        builder_.countRead();
        return;
    }
    if (!(sdr33 || sdr2x)) {
        refuse(n, version.empty()
                      ? std::string("the header record names no version, so the layout of the "
                                    "file cannot be told")
                      : "the header names the version '" + shown(version.substr(0, 20)) +
                            "'; Katana reads the two layouts the format's publisher documents, "
                            "SDR33 and SDR2x ([SDR] 3.2.5: 'SDR33 V04-..' and 'SDR20 V03-05')");
        return;
    }
    if (!optionsLast) {
        refuse(n, "the header record does not end in its six unit and order options ([SDR] "
                  "3.6: 46 characters, the options in 41-46), so the units cannot be told");
        return;
    }
    const std::string options(text.substr(text.size() - 6));
    std::string serial;
    std::string date;
    if (text.size() == 46) {
        serial = cleaned(text.substr(20, 4));
        date = cleaned(text.substr(24, 16));
    } else {
        date = cleaned(text.substr(20, text.size() - 26));
        if (text.size() != 42) {
            builder_.warn(n, "the header record is " + std::to_string(text.size()) +
                                 " characters where the format's is 46; its last six are read "
                                 "as the options");
        }
    }
    headerRead_ = true;
    headerRecord_ = n;
    headerOptions_ = options;
    variant_ = variant;
    // [NIKON]: "SDR33V04-01", "SDR20V03-05" - the version with no blank.
    nikonForm_ = version.size() > 5 && version[5] == 'V';
    pointIdWidth_ = variant == Variant::Sdr33 ? 16 : 4;
    realWidth_ = variant == Variant::Sdr33 ? 16 : 10;

    survey::AngularUnit angular = survey::AngularUnit::DecimalDegrees;
    switch (options[0]) {
    case '1':
        angleUnit_ = AngleUnit::Degrees;
        angular = survey::AngularUnit::DecimalDegrees;
        break;
    case '2':
        angleUnit_ = AngleUnit::Gons;
        angular = survey::AngularUnit::Gons;
        break;
    case '3': // [SDR] 3.5
        angleUnit_ = AngleUnit::Mils;
        angular = survey::AngularUnit::Mils;
        break;
    case '4':
        // Not [SDR]'s; two writers define it, and the header's form tells
        // them apart (see the top of this file).
        if (nikonForm_) {
            angleUnit_ = AngleUnit::Mils;
            angular = survey::AngularUnit::Mils;
            builder_.warn(n, "the header's angle unit option is 4, which the format's publisher "
                             "does not define; this header is written as the Nikon and Spectra "
                             "manuals write theirs (no blank before the version), and they give "
                             "4 as mils, so the angles are read in mils - Trimble's writer uses 4 "
                             "for quadrant bearings in degrees");
        } else {
            angleUnit_ = AngleUnit::Degrees;
            angular = survey::AngularUnit::DecimalDegrees;
            builder_.warn(n, "the header's angle unit option is 4, which the format's publisher "
                             "does not define; Trimble's writer uses it for quadrant bearings, "
                             "which are degrees underneath, so the angles are read in degrees - "
                             "the Nikon and Spectra manuals give 4 as mils, but in a header "
                             "written with no blank before the version, and this one has one");
        }
        break;
    default:
        refuse(n, "the header's angle unit option is '" + shown(std::string(1, options[0])) +
                      "'; the format defines 1 degrees, 2 gons and 3 mils, and Katana will not "
                      "guess what another means");
        return;
    }
    switch (options[1]) {
    case '1':
        linearUnit_ = survey::LinearUnit::Metres;
        break;
    case '2': // [SDR] 3.3.2: "meters and international feet"
        linearUnit_ = survey::LinearUnit::Feet;
        break;
    case '3': // [TA]: US survey feet
        linearUnit_ = survey::LinearUnit::UsSurveyFeet;
        break;
    default:
        refuse(n, "the header's distance unit option is '" + shown(std::string(1, options[1])) +
                      "'; the format defines 1 metres and 2 feet, and Trimble's writer 3 US "
                      "survey feet, and Katana will not guess what another means");
        return;
    }
    metres_ = survey::metresPer(linearUnit_).value();
    switch (options[2]) {
    case '1':
        pressureUnit_ = PressureUnit::MillimetresOfMercury;
        break;
    case '2':
        pressureUnit_ = PressureUnit::InchesOfMercury;
        break;
    case '3':
        pressureUnit_ = PressureUnit::Millibars;
        break;
    default:
        builder_.warn(n, "the header's pressure unit option is '" + std::string(1, options[2]) +
                             "', which the format does not define; no pressure is imported");
        break;
    }
    switch (options[3]) {
    case '1':
        temperatureUnit_ = TemperatureUnit::Celsius;
        break;
    case '2':
        temperatureUnit_ = TemperatureUnit::Fahrenheit;
        break;
    default:
        builder_.warn(n, "the header's temperature unit option is '" +
                             std::string(1, options[3]) +
                             "', which the format does not define; no temperature is imported");
        break;
    }
    orderOption_ = options[4];
    order_ = options[4] == '1'                         ? CoordinateOrder::NorthFirst
             : (options[4] == '2' || options[4] == '3') ? CoordinateOrder::EastFirst
                                                        : CoordinateOrder::Unknown;
    if (options[5] != '1') {
        // [SDR] 3.6.1 "Always '1'"; [TA] "Always Angles Right". Nothing
        // says what another value would do to a circle reading.
        refuse(n, "the header's angles left/right option is '" + std::string(1, options[5]) +
                      "'; only 1, angles right, is documented, and Katana will not guess which "
                      "way another turns");
        return;
    }
    survey::SurveyProject& project = builder_.project();
    project.units = survey::DeclaredUnits{linearUnit_, angular};
    project.metadata["header: version"] = version;
    if (!serial.empty()) {
        project.metadata["header: serial number"] = serial;
    }
    if (!date.empty()) {
        project.metadata["header: date and time"] = date;
    }
    // How the coordinates of the 02 and 08 records were read (see the top
    // of this file), for anyone checking a known point.
    if (order_ != CoordinateOrder::Unknown) {
        project.metadata["header: coordinate order"] =
            order_ == CoordinateOrder::NorthFirst
                ? "north, east, elevation (option 1)"
                : "east, north, elevation (option " + std::string(1, orderOption_) + ")";
    }
    builder_.countRead();
}

void SdrReader::readJob(Fields& fields, std::size_t n)
{
    const std::string name = cleaned(fields.take(16));
    survey::SurveyProject& project = builder_.project();
    if (jobRead_) {
        // [SDR] chapter 5: a job ends where another begins. One import is
        // one survey, so the next job's records are read into it.
        builder_.warn(n, "a second job ('" + name + "') begins here; its records are read into "
                                                    "the same survey");
        project.metadata["job at record " + std::to_string(n)] = name;
        if (collimation_.record != 0) {
            // [SETX] 13.1: "Collimation is not maintained across all jobs."
            builder_.warn(n, "the collimation of record " + std::to_string(collimation_.record) +
                                 " is not applied in job '" + name +
                                 "': the field book keeps a collimation within its job");
            collimation_ = Collimation{};
        }
        builder_.countRead();
        return;
    }
    jobRead_ = true;
    if (!name.empty()) {
        project.name = name;
    }
    if (variant_ == Variant::Sdr33) {
        // [SDR] 3.6.2: point id type, include elevation, atmospheric
        // correction, C & R correction, refraction constant, sea level
        // correction; 3.5: yes/no fields are 1 No, 2 Yes.
        const std::string_view flags = fields.take(6);
        const auto flag = [&](std::size_t i) { return i < flags.size() ? flags[i] : ' '; };
        const auto yesNo = [](char c) -> std::string {
            return c == '1' ? "off" : (c == '2' ? "on" : "");
        };
        if (flag(1) == '1') {
            recordElevations_ = false;
            project.metadata["job: elevations"] = "not recorded";
        }
        atmosphericSwitch_ = yesNo(flag(2));
        const std::string curvature = yesNo(flag(3));
        const std::string seaLevel = yesNo(flag(5));
        if (!atmosphericSwitch_.empty()) {
            project.metadata["job: atmospheric correction"] = atmosphericSwitch_;
        }
        if (!curvature.empty()) {
            project.metadata["job: curvature and refraction correction"] = curvature;
        }
        if (!seaLevel.empty()) {
            project.metadata["job: sea level correction"] = seaLevel;
        }
        // [SETX] 4 makes the constant available "only when C and R crn is
        // set to Yes".
        if (curvature == "on" && (flag(4) == '1' || flag(4) == '2')) {
            settings_.refractionCoefficient =
                flag(4) == '2' ? kRefractionOption2
                               : (nikonForm_ ? kNikonRefractionOption1 : kRefractionOption1);
        }
    }
    builder_.countRead();
}

void SdrReader::readCorrectionNote(std::string_view text)
{
    // SDR2x keeps the job's corrections in 13CP notes, "Atmos crn: N"
    // ([SDR] chapter 4's sample; [SETX] 28 "Note CP").
    const std::size_t colon = text.find(':');
    if (colon == std::string_view::npos) {
        return;
    }
    const std::string_view key = trimmed(text.substr(0, colon));
    const std::string_view value = trimmed(text.substr(colon + 1));
    const std::string state = value == "Y" ? "on" : (value == "N" ? "off" : "");
    if (state.empty()) {
        return;
    }
    survey::SurveyProject& project = builder_.project();
    if (key == "Atmos crn") {
        atmosphericSwitch_ = state;
        project.metadata["job: atmospheric correction"] = state;
    } else if (key == "C and R crn") {
        project.metadata["job: curvature and refraction correction"] = state;
    } else if (key == "Sea level crn") {
        project.metadata["job: sea level correction"] = state;
    }
}

void SdrReader::readInstrument(Fields& fields, std::size_t n)
{
    // [SDR] 3.6: EDM type, EDM description, EDM serial, theodolite
    // description, theodolite serial, mounting, vertical angle option, EDM
    // offset, reflector offset, prism constant (mm).
    const std::string edmType = cleaned(fields.take(1));
    const std::string edm = cleaned(fields.take(16));
    const std::string edmSerial = cleaned(fields.take(6));
    const std::string theodolite = cleaned(fields.take(16));
    const std::string theodoliteSerial = cleaned(fields.take(6));
    const std::string mounting = cleaned(fields.take(1));
    const std::string verticalOption = cleaned(fields.take(1));
    const std::optional<double> edmOffset = length(fields.real(), "EDM offset", n);
    const std::optional<double> reflectorOffset = length(fields.real(), "reflector offset", n);
    const std::optional<double> prism = number(fields.real(), "prism constant", n);

    instrumentSeen_ = true;
    if (!edmType.empty()) {
        // [SETX] 13.1: a collimation applies "until either the instrument
        // type is changed or a new collimation record is added"; the EDM
        // type is the code [SDR] 3.5 lists instruments by. The same type
        // restated (every setup of some files) changes nothing.
        if (collimation_.record != 0 && !edmType_.empty() && edmType != edmType_) {
            builder_.warn(n, "the instrument type changes here (EDM type '" + shown(edmType_) +
                                 "' to '" + shown(edmType) + "'), so the collimation of record " +
                                 std::to_string(collimation_.record) +
                                 " is not applied after it: the field book applies one until "
                                 "the instrument type changes");
            collimationEndedAt_ = n;
            endedCollimation_ = collimation_.record;
            collimation_ = Collimation{};
        }
        edmType_ = edmType;
    }
    if (verticalOption == "1") {
        vertical_ = VerticalReference::Zenith;
    } else if (verticalOption == "2") {
        vertical_ = VerticalReference::Horizon;
    } else {
        vertical_ = VerticalReference::Unknown;
        builder_.warn(n, "the instrument's vertical angle option is '" + verticalOption +
                             "'; the format defines 1 (zenith) and 2 (horizon), so the vertical "
                             "readings after this record are not imported");
    }
    // A serial number of zeros is the field book's "none" ([SDR] 2.4's
    // "EDM S/N 000000" for a manual instrument).
    const auto stated = [](const std::string& serialText) {
        return !serialText.empty() && serialText.find_first_not_of('0') != std::string::npos;
    };
    settings_.model = theodolite.empty() ? edm : theodolite;
    settings_.serialNumber = stated(theodoliteSerial) ? theodoliteSerial
                                                      : (stated(edmSerial) ? edmSerial : "");
    if (prism) {
        // [SDR] 3.3.4: millimetres whatever the distance unit.
        settings_.prismConstant = *prism / 1000.0;
        settings_.prismConstantState = survey::CorrectionState::Applied;
    } else {
        settings_.prismConstant.reset();
        settings_.prismConstantState = survey::CorrectionState::Unknown;
    }
    survey::SurveyProject& project = builder_.project();
    if (!edmType.empty()) {
        project.metadata["instrument: EDM type option"] = edmType;
    }
    // [TA] writes the instrument's serial number here; it is kept as the
    // field's text, the format's EDM description.
    if (!edm.empty() && edm != settings_.model) {
        project.metadata["instrument: EDM"] = edm;
    }
    if (!mounting.empty()) {
        project.metadata["instrument: mounting option"] = mounting;
    }
    // An EDM or reflector off the telescope's axis needs [L5] B.4.4's
    // mounting eccentricity correction, which the model has no place for.
    for (const auto& [offset, name] : {std::pair{edmOffset, "EDM offset"},
                                       std::pair{reflectorOffset, "reflector offset"}}) {
        if (offset && *offset != 0.0) {
            builder_.warn(n, std::string("the instrument record's ") + name + " is " +
                                 formatExactReal(*offset) +
                                 " m, which is not applied to the distances");
            project.metadata[std::string("instrument: ") + name + " (m)"] =
                formatExactReal(*offset);
        }
    }
    settingsChanged(n, "the instrument");
    builder_.countRead();
}

void SdrReader::readAtmosphere(Fields& fields, std::size_t n)
{
    const std::optional<double> pressure = number(fields.real(), "pressure", n);
    const std::optional<double> temperature = number(fields.real(), "temperature", n);
    if (!pressure && !temperature) {
        builder_.skip(n, "atmosphere record with neither a pressure nor a temperature");
        return;
    }
    weatherSeen_ = true;
    if (pressure) {
        switch (pressureUnit_) {
        case PressureUnit::MillimetresOfMercury:
            settings_.pressureHectopascals = *pressure * kHectopascalsPerMillimetreOfMercury;
            break;
        case PressureUnit::InchesOfMercury:
            settings_.pressureHectopascals = *pressure * kHectopascalsPerInchOfMercury;
            break;
        case PressureUnit::Millibars: // a millibar is a hectopascal
            settings_.pressureHectopascals = *pressure;
            break;
        case PressureUnit::Unknown:
            break;
        }
    }
    if (temperature) {
        switch (temperatureUnit_) {
        case TemperatureUnit::Celsius:
            settings_.temperatureCelsius = *temperature;
            break;
        case TemperatureUnit::Fahrenheit:
            // NIST SP 811 (2008), appendix B: t/C = (t/F - 32) / 1.8.
            settings_.temperatureCelsius = (*temperature - 32.0) / 1.8;
            break;
        case TemperatureUnit::Unknown:
            break;
        }
    }
    settingsChanged(n, "the weather");
    builder_.countRead();
}

void SdrReader::readScale(Fields& fields, std::size_t n)
{
    const std::optional<double> factor = number(fields.real(), "scale factor", n);
    if (!factor || !(*factor > 0.0)) {
        builder_.skip(n, "scale factor record without a positive factor");
        return;
    }
    // [SETX] 28: "plane scale factor", applied between an observation's
    // corrected view and its reduced one (29) - never in a raw distance.
    settings_.scaleFactor = *factor;
    settings_.scaleFactorState = survey::CorrectionState::NotApplied;
    settingsChanged(n, "the scale factor");
    builder_.countRead();
}

void SdrReader::readCollimation(Fields& fields, std::size_t n)
{
    const std::optional<double> vertical = angle(fields.real(), "vertical collimation", n);
    const std::optional<double> horizontal = angle(fields.real(), "horizontal collimation", n);
    if (!vertical && !horizontal) {
        builder_.skip(n, "collimation record with no value");
        return;
    }
    // Applied to every raw reading from here to the next 04, another
    // instrument type or another job (see the top of this file); a
    // correction the record leaves null is none.
    collimation_ = Collimation{vertical.value_or(0.0), horizontal.value_or(0.0), n};
    survey::SurveyStation* station = builder_.currentStation();
    std::map<std::string, std::string>& metadata =
        station != nullptr ? station->metadata : builder_.project().metadata;
    if (vertical) {
        metadata["collimation, vertical (radians)"] = formatExactReal(*vertical);
    }
    if (horizontal) {
        metadata["collimation, horizontal (radians)"] = formatExactReal(*horizontal);
    }
    builder_.countRead();
}

void SdrReader::readStation(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string_view idField = fields.pointId();
    const std::string_view first = fields.real();
    const std::string_view second = fields.real();
    const std::string_view elevation = fields.real();
    const std::optional<double> height = length(fields.real(), "instrument height", n);
    const std::string description = cleaned(fields.take(16));
    if (refusedIds(n, {idField})) {
        return;
    }
    const std::string id = pointIdOf(idField);
    if (id.empty()) {
        builder_.skip(n, "station record names no point");
        return;
    }
    finishSetup();
    settleDerivedViews();
    occupationTargets_.clear();
    appliedCollimation_ = 0.0;
    if (!height) {
        builder_.warn(n, "the setup states no instrument height; 0 is used");
    }
    builder_.beginStation(id, height.value_or(0.0), settings_, n);
    resetSetup();
    badSet_ = false;
    badSetRemaining_.reset();
    coordinates(id, first, second, elevation,
                derivation == "KI" ? survey::CoordinateSource::Entered
                                   : survey::CoordinateSource::Unknown,
                n);
    if (fatal_) {
        return;
    }
    if (!description.empty()) {
        builder_.codePoint(id, firstWord(description), description, {}, n);
    }
    builder_.countRead();
}

void SdrReader::readTarget(Fields& fields, std::size_t n)
{
    const std::optional<double> height = length(fields.real(), "target height", n);
    if (!height) {
        builder_.skip(n, "target height record with no height");
        return;
    }
    targetHeight_ = *height;
    targetHeightSeen_ = true;
    builder_.countRead();
}

void SdrReader::readBacksight(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string_view fromField = fields.pointId();
    const std::string_view toField = fields.pointId();
    const std::optional<double> azimuth = angle(fields.real(), "backsight azimuth", n);
    const std::optional<double> reading = angle(fields.real(), "backsight circle reading", n);
    if (refusedIds(n, {fromField, toField})) {
        return;
    }
    const std::string from = pointIdOf(fromField);
    const std::string to = pointIdOf(toField);
    survey::SurveyStation* station = builder_.currentStation();
    if (station == nullptr) {
        builder_.skip(n, "backsight record with no station record before it");
        return;
    }
    if (from != station->setup.pointId) {
        builder_.skip(n, "backsight record from '" + from + "' while the setup is on '" +
                             station->setup.pointId + "'");
        return;
    }
    if (to.empty() || to == from) {
        builder_.skip(n, to.empty() ? std::string("backsight record names no backsight point")
                                    : "backsight record from '" + from + "' to itself");
        return;
    }
    const std::optional<double> wrappedAzimuth =
        azimuth ? std::optional<double>(wrapToCircle(*azimuth)) : std::nullopt;
    const std::optional<double> wrappedReading =
        reading ? std::optional<double>(wrapToCircle(*reading)) : std::nullopt;
    // An SDR2x 12 read after raw records closed them (V04-02 on) or opened
    // the set after it (V04-01), and the set's 07 SC tells which (see the
    // top of this file): raw observations read since the 12 are its set;
    // none, and the set was the records before it.
    if (derivation == "SC" && !backsightSinceSet_ && setBefore_ && setStart_ &&
        *setStart_ == station->observations.size()) {
        setStart_ = setBefore_;
        station->metadata["set at record " + std::to_string(setStartRecord_)] +=
            ", closing the " + std::to_string(setBeforeRecords_) + " before it";
    }
    setBefore_.reset();
    // Set Collection's 07 SC read after a set's observations, with no 07
    // since the set began, closes that set: it is what those observations
    // were oriented on. Any other 07 orients what follows ([SETX] 8.2).
    const bool closesSet = derivation == "SC" && setStart_ && !backsightSinceSet_ &&
                           *setStart_ < station->observations.size();
    backsightSinceSet_ = true;
    // The observations this record orients: those of the set it closes, or
    // what follows it.
    const std::size_t first = closesSet ? *setStart_ : station->observations.size();
    if (orientation_.set) {
        const bool sameCircle = wrappedReading && orientation_.reading
                                    ? onSameCircle(*wrappedReading, *orientation_.reading)
                                    : wrappedReading.has_value() == orientation_.reading.has_value();
        const bool anotherRound =
            to == orientation_.backsight && wrappedAzimuth == orientation_.azimuth && sameCircle;
        if (anotherRound) {
            ++orientation_.rounds;
            station->metadata["backsight rounds"] = std::to_string(orientation_.rounds);
            if (wrappedReading) {
                std::string& readings = station->metadata["backsight circle readings (radians)"];
                readings += (readings.empty() ? "" : ", ") + formatExactReal(*wrappedReading);
            }
            setBoundary_ = station->observations.size();
            builder_.countRead();
            return;
        }
        // The circle was oriented anew. Shots of this record's backsight
        // just before it, on its circle and not the setup's, were read after
        // that: they go with it (see the top of this file).
        const std::size_t moved =
            closesSet ? first : newCircleStart(*station, to, wrappedReading);
        if (moved > 0) {
            // Those observations are read against another zero, so they are
            // a setup of their own ([SETX] 8.2). The instrument height is
            // the setup's: no 02 has moved it. The settings are those in
            // force now, which an 01 or 05 between the rounds may have
            // changed.
            finishSetup(moved);
            station = &splitSetup(moved, n);
        } else {
            const std::string earlier = std::to_string(orientation_.record);
            builder_.warn(n, closesSet ? "this backsight record, closing the set of record " +
                                             std::to_string(setStartRecord_) +
                                             ", replaces the one at record " + earlier +
                                             ", since no observation came between that one "
                                             "and the set"
                             : station->observations.empty()
                                 ? "a second backsight record before any observation on the "
                                   "first (record " +
                                       earlier + ") replaces it"
                                 : "this backsight record replaces the one at record " + earlier +
                                       ": every observation since that one reads its backsight '" +
                                       to + "' " +
                                       (wrappedReading
                                            ? "on this record's circle and not on that one's"
                                            : "off that one's circle") +
                                       ", so the circle was set anew before them");
            station->metadata.erase("backsight circle readings (radians)");
        }
    } else if (first > 0) {
        // The setup's first 07, read after observations. They are read on the
        // orientation it gives only where they read its backsight on its
        // circle, which says the circle was not set anew at it (see the top
        // of this file); and never when they came before the set it closes.
        const BacksightReadings before = backsightBefore(*station, first, to, wrappedReading);
        const bool onItsCircle =
            wrappedReading ? before.onCircle > 0 && before.offCircle == 0 : before.observed;
        if (closesSet || !onItsCircle) {
            // They stay a setup of their own, oriented as finishSetup()
            // finds, and this record begins another.
            const std::string previous = station->setup.id;
            const std::size_t records = recordsBefore(first);
            const std::size_t setRecord = setStartRecord_;
            std::string why;
            if (closesSet) {
                why = "the set this backsight record closes (record " + std::to_string(setRecord) +
                      "), which it orients and not them";
            } else if (!before.observed) {
                why = "its first backsight record, and none is of its backsight '" + to +
                      "': a backsight record orients what follows it ([SETX] 8.2)";
            } else if (before.offCircle > 0) {
                why = "its first backsight record, and they read its backsight '" + to +
                      "' more than a minute of arc from this record's circle reading on it: the "
                      "circle was set anew, and a backsight record orients what follows it "
                      "([SETX] 8.2)";
            } else {
                why = "its first backsight record, and none gives a horizontal reading of its "
                      "backsight '" +
                      to +
                      "', so nothing shows them read on this record's circle: a backsight "
                      "record orients what follows it ([SETX] 8.2)";
            }
            finishSetup(first);
            station = &splitSetup(first, n);
            builder_.warn(n, "setup '" + previous + "' has " + std::to_string(records) +
                                 " observation record(s) before " + why + "; they stay setup '" +
                                 previous + "', and setup '" + station->setup.id +
                                 "' begins here");
        }
    }
    orientation_ = Orientation{true, to, wrappedAzimuth, wrappedReading, 1, n};
    keyed_.clear(); // a backsight record orients the setup; a keyed azimuth no longer does
    builder_.mentionPoint(to, n);
    station->backsightPointId = to;
    station->backsightAzimuth = wrappedReading;       // the model's circle
    station->statedBacksightAzimuth = wrappedAzimuth; // and the azimuth, beside it
    station->metadata.erase("oriented by");
    station->metadata["backsight rounds"] = "1";
    if (wrappedReading) {
        station->metadata["backsight circle readings (radians)"] = formatExactReal(*wrappedReading);
    }
    setBoundary_ = station->observations.size();
    builder_.countRead();
}

void SdrReader::readPosition(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string_view idField = fields.pointId();
    const std::string_view first = fields.real();
    const std::string_view second = fields.real();
    const std::string_view elevation = fields.real();
    const std::string description = cleaned(fields.take(16));
    if (refusedIds(n, {idField})) {
        return;
    }
    const std::string id = pointIdOf(idField);
    if (id.empty()) {
        builder_.skip(n, "coordinate record names no point");
        return;
    }
    survey::CoordinateSource how = survey::CoordinateSource::Unknown;
    if (derivation == "KI") {
        how = survey::CoordinateSource::Entered;
    } else if (derivation == "TP") {
        how = survey::CoordinateSource::FieldObserved;
    } else if (derivation == "AJ" || derivation == "TV" || derivation == "RS") {
        how = survey::CoordinateSource::Calculated;
    }
    builder_.mentionPoint(id, n);
    // Neither keyed nor computed, and of the point the observation record
    // before it observes: that observation's POS view, sent beside it
    // ([SETX] 27.2).
    const bool positionView = how != survey::CoordinateSource::Entered &&
                              how != survey::CoordinateSource::Calculated &&
                              previousShot_.record != 0 && id == previousShot_.to;
    if (!positionView) {
        coordinates(id, first, second, elevation, how, n);
    } else if (const std::optional<StatedCoordinates> stated =
                   statedCoordinates(id, first, second, elevation, n)) {
        // The observation stayed in its own view in the field book, which
        // places a point that has no other coordinates and never supersedes
        // those of a position record ([SETX] 6.1, 8.5.2).
        builder_.positionPoint(
            id, stated->northing, stated->easting, stated->elevation, how, n,
            positionRecorded_.contains(id)
                ? "they are the POS view of the " + std::string(previousShot_.what) +
                      " at record " + std::to_string(previousShot_.record) +
                      ", which the field book keeps in " + std::string(previousShot_.view) +
                      " view: an observation in OBS, MC or RED view does not overwrite a point's "
                      "coordinates from a position record ([SETX] 6.1, 8.5.2)"
                : std::string{});
    }
    if (fatal_) {
        return;
    }
    if (!description.empty()) {
        builder_.codePoint(id, firstWord(description), description, {}, n);
    }
    builder_.countRead();
}

void SdrReader::readObservation(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string_view fromField = fields.pointId();
    const std::string_view toField = fields.pointId();
    const std::string_view slopeField = fields.real();
    const std::string_view verticalField = fields.real();
    const std::string_view horizontalField = fields.real();
    const std::string description = cleaned(fields.take(16));
    // An observation record, whatever becomes of it below: an 08 of its
    // target straight after it is its POS view (see the top of this file).
    if (derivation == "MC") {
        lastShot_ = ShotRecord{n, pointIdOf(toField), "corrected observation (MC)", "MC"};
        readDerivedView(PendingDerivedView{n, true, pointIdOf(fromField), pointIdOf(toField)});
        return;
    }
    if (derivation == "F1" || derivation == "F2" || derivation == "MD") {
        lastShot_ = ShotRecord{n, pointIdOf(toField), "observation", "OBS"};
    } else {
        builder_.skip(n, "an observation with derivation code '" + shown(derivation) +
                             "', which the format does not define for one");
        return;
    }
    if (refusedIds(n, {fromField, toField})) {
        return;
    }
    const std::string from = pointIdOf(fromField);
    const std::string to = pointIdOf(toField);
    survey::SurveyStation* setup = builder_.currentStation();
    if (setup == nullptr) {
        builder_.skip(n, "observation with no station record before it");
        return;
    }
    if (from != setup->setup.pointId) {
        builder_.skip(n, "observation from '" + from + "' while the setup is on '" +
                             setup->setup.pointId + "'");
        return;
    }
    if (to.empty() || to == from) {
        builder_.skip(n, to.empty() ? std::string("observation names no target point")
                                    : "observation from '" + from + "' to itself");
        return;
    }
    if (badSet_) {
        builder_.skip(n, "observation in " + badSetName_ + " (record " +
                             std::to_string(badSetRecord_) +
                             "), which the field book marks bad and does not use");
        if (badSetRemaining_ && --*badSetRemaining_ == 0) {
            badSet_ = false;
            badSetRemaining_.reset();
        }
        return;
    }
    const std::optional<double> slope = length(slopeField, "slope distance", n);
    const std::optional<double> vertical = angle(verticalField, "vertical reading", n);
    const std::optional<double> horizontal = angle(horizontalField, "horizontal reading", n);

    // A plain double and a flag, not a reassigned optional: GCC 16 at -O3
    // reports the optional's payload "maybe uninitialized" (a false positive
    // that -Werror turns into a build failure).
    survey::Face face = derivation == "F1"   ? survey::Face::Left
                        : derivation == "F2" ? survey::Face::Right
                                             : survey::Face::Unknown;
    bool hasZenith = false;
    double zenith = 0.0;
    if (vertical && vertical_ != VerticalReference::Unknown) {
        double reading = *vertical;
        if (vertical_ == VerticalReference::Horizon) {
            reading = wrapToCircle(kPi / 2.0 - reading);
        }
        if (reading < 0.0 || reading >= 2.0 * kPi) {
            builder_.warn(n, "vertical reading outside a full circle; the zenith is not "
                             "imported");
        } else {
            const survey::Face readingFace = faceOfZenithReading(reading);
            if (face == survey::Face::Unknown) {
                face = readingFace;
            } else if (readingFace != survey::Face::Unknown && readingFace != face) {
                builder_.warn(n, std::string(derivation) + " says " + survey::toString(face) +
                                     ", but its vertical reading is on " +
                                     survey::toString(readingFace) +
                                     "; the record's face is kept");
            }
            zenith = zenithInModelRange(reading);
            hasZenith = true;
        }
    }
    const bool distanceMeasured = slope && *slope > 0.0;
    if (slope && !distanceMeasured) {
        builder_.warn(n, "slope distance of " + formatExactReal(*slope) +
                             " read as no distance measured");
    }
    if (!horizontal && !hasZenith && !distanceMeasured) {
        // Not a shot: it changes nothing and names no point, so a skipped
        // record leaves no target behind that nothing observed.
        builder_.skip(n, "observation with no readable value");
        return;
    }

    for (const auto& [what, record] : pendingChanges_) {
        // One setup holds one set of settings, and the shots before this
        // record were made under the old ones. A collimation the record
        // ended is not one of them: it is applied reading by reading, so it
        // has ended for the shots after the record (see the top of this
        // file), and the warning must not say it was kept.
        const std::string collimation =
            record == collimationEndedAt_
                ? "the collimation of record " + std::to_string(endedCollimation_)
                : std::string{};
        builder_.warn(record, what + " changes in the middle of setup '" + setup->setup.id +
                                  "'; the setup keeps the values it began with, and these apply "
                                  "from the next setup" +
                                  (collimation.empty()
                                       ? std::string{}
                                       : " - all but " + collimation +
                                             ", which ends here all the same: it is applied to "
                                             "each reading, and the field book applies one only "
                                             "until the instrument type changes"));
        setup->metadata[what + " changed at record " + std::to_string(record)] =
            collimation.empty() ? std::string("not applied to this setup")
                                : "not applied to this setup, but " + collimation + " ends at it";
    }
    pendingChanges_.clear();
    if (!instrumentSeen_ && !instrumentWarned_) {
        instrumentWarned_ = true;
        builder_.warn(n, "no instrument record comes before the first observation; vertical "
                         "readings are read as zenith angles, the format's first option");
    }
    if (!targetHeightSeen_ && !targetHeightWarned_) {
        targetHeightWarned_ = true;
        builder_.warn(n, "no target height record comes before this observation; 0 is used "
                         "until one does");
        builder_.notCarried("no target height for the observations before the first target "
                            "height record (0 was used)");
    }
    builder_.mentionPoint(to, n);
    if (!description.empty()) {
        builder_.codePoint(to, firstWord(description), description, {}, n);
    }

    double direction = horizontal ? wrapToCircle(*horizontal) : 0.0;
    if (collimation_.record != 0) {
        // [SETX] 29.2.5: face 1 plus, face 2 minus; a reading with no
        // vertical angle is face 1 ([SETX] 29.2.3).
        const double sign = face == survey::Face::Right ? -1.0 : 1.0;
        direction = wrapToCircle(direction + sign * collimation_.horizontal);
        if (hasZenith) {
            zenith = std::clamp(zenith + sign * collimation_.vertical, 0.0, kPi);
        }
        setup->metadata.try_emplace("collimation of record " +
                                        std::to_string(collimation_.record),
                                    "applied to its readings ([SETX] 29.2.5)");
        appliedCollimation_ = std::max(appliedCollimation_, std::fabs(collimation_.horizontal));
    }

    const survey::ObservationPrecision& precision = builder_.options().precision;
    const double hi = setup->setup.instrumentHeight;
    const survey::Pointing pointing{builder_.nextPointing(), face};
    recordStarts_.push_back(setup->observations.size());
    occupationTargets_.insert(to);
    if (horizontal) {
        auto& observation = builder_.stationObservation<survey::HorizontalDirectionObservation>(n);
        observation.at = setup->setup.pointId;
        observation.to = to;
        observation.direction = direction;
        observation.sigma = precision.direction;
        observation.pointing = pointing;
    }
    if (hasZenith) {
        auto& observation = builder_.stationObservation<survey::ZenithAngleObservation>(n);
        observation.from = setup->setup.pointId;
        observation.to = to;
        observation.angle = zenith;
        observation.sigma = precision.zenith;
        observation.instrumentHeight = hi;
        observation.targetHeight = targetHeight_;
        observation.pointing = pointing;
    }
    if (distanceMeasured) {
        auto& observation = builder_.stationObservation<survey::DistanceObservation>(n);
        observation.from = setup->setup.pointId;
        observation.to = to;
        observation.distance = *slope;
        observation.sigma = survey::distanceSigma(precision, *slope);
        observation.kind = survey::DistanceKind::Slope;
        observation.instrumentHeight = hi;
        observation.targetHeight = targetHeight_;
        observation.pointing = pointing;
    }
    ++setupObservations_;
    builder_.countRead();
}

void SdrReader::readReduced(Fields& fields, std::size_t n)
{
    const std::string_view fromField = fields.pointId();
    const std::string_view toField = fields.pointId();
    const std::optional<double> azimuth = angle(fields.real(), "azimuth", n);
    const std::string_view horizontalField = fields.real();
    const std::string_view verticalField = fields.real();
    const std::string description = cleaned(fields.take(16));
    if (refusedIds(n, {fromField, toField})) {
        return;
    }
    const std::string from = pointIdOf(fromField);
    const std::string to = pointIdOf(toField);
    if (from.empty() || to.empty()) {
        builder_.skip(n, "reduced observation record names no point");
        return;
    }
    if (!trimmed(horizontalField).empty() || !trimmed(verticalField).empty()) {
        // A derived view, as an MC is, and an observation record whose POS
        // view may follow it (see the top of this file).
        lastShot_ = ShotRecord{n, to, "reduced observation (RED)", "RED"};
        readDerivedView(PendingDerivedView{n, false, from, to});
        return;
    }
    if (!azimuth) {
        builder_.skip(n, "reduced observation record with no value");
        return;
    }
    // An azimuth and nothing else: an orientation keyed for the line.
    builder_.mentionPoint(from, n);
    builder_.mentionPoint(to, n);
    survey::SurveyStation* station = builder_.currentStation();
    const bool here = station != nullptr && station->setup.pointId == from;
    std::map<std::string, std::string>& metadata =
        here ? station->metadata : builder_.project().metadata;
    const std::string key = here ? "azimuth to " + to : "azimuth " + from + " to " + to;
    metadata[key + " (radians)"] = formatExactReal(wrapToCircle(*azimuth));
    if (!description.empty()) {
        metadata[key + ", description"] = description;
    }
    if (here && !orientation_.set) {
        // What orients the setup if no 07 does (see finishSetup).
        keyed_.push_back(KeyedAzimuth{to, wrapToCircle(*azimuth), n});
    }
    builder_.countRead();
}

void SdrReader::readSet(Fields& fields, std::size_t n)
{
    const std::string_view fromField = fields.pointId();
    const std::string count = cleaned(fields.take(3));
    std::string setNumber;
    std::string bad;
    if (variant_ == Variant::Sdr33) {
        // [SDR] 3.6.2: number of the set, bad marker, return sight, order.
        setNumber = cleaned(fields.take(3));
        bad = cleaned(fields.take(1));
    }
    if (refusedIds(n, {fromField})) {
        return;
    }
    const std::string from = pointIdOf(fromField);
    const std::optional<int> counted = digitsValue(count);
    const bool markedBad = bad == "2";
    badSetRemaining_ = markedBad && counted
                           ? std::optional<std::size_t>(static_cast<std::size_t>(*counted))
                           : std::nullopt;
    // A set of none has nothing to skip, and must not take the next shots.
    badSet_ = markedBad && badSetRemaining_ != std::optional<std::size_t>(0);
    badSetRecord_ = n;
    badSetName_ =
        "set " + (setNumber.empty() ? std::string("?") : setNumber) + " at '" + from + "'";
    if (markedBad) {
        builder_.warn(n, badSetName_ + " is marked bad; its " +
                             (badSetRemaining_ ? count + " " : std::string{}) +
                             "observation(s) are not imported" +
                             (badSetRemaining_ ? std::string{}
                                               : ", up to the next set or setup, since it "
                                                 "states no count of them"));
    }
    if (survey::SurveyStation* station = builder_.currentStation(); station != nullptr) {
        const std::size_t here = station->observations.size();
        // The set begins here, as a 12 opens it in SDR33 and SDR2x's V04-01
        // - unless, in the SDR2x layout, this 12 follows raw records of its
        // setup and its set's 07 SC finds no raw observation after it: then
        // it closed the set they make (see the top of this file), the last
        // of them as many as its count, where it states one they hold.
        const std::size_t since = recordStarts_.size() - recordsBefore(setBoundary_);
        std::size_t closable = 0;
        if (variant_ == Variant::Sdr2x && since > 0) {
            closable = counted && *counted > 0 && static_cast<std::size_t>(*counted) < since
                           ? static_cast<std::size_t>(*counted)
                           : since;
        }
        station->metadata["set at record " + std::to_string(n)] =
            (setNumber.empty() ? std::string{} : "number " + setNumber + ", ") + count +
            " observation(s)" + (markedBad ? ", marked bad" : "");
        setStart_ = here;
        setBefore_ = closable > 0 ? std::optional<std::size_t>(
                                        recordStarts_[recordStarts_.size() - closable])
                                  : std::nullopt;
        setBeforeRecords_ = closable;
        setStartRecord_ = n;
        backsightSinceSet_ = false;
        setBoundary_ = here;
    }
    builder_.countRead();
}

void SdrReader::readDistanceUnitNote(std::string_view text, std::size_t n)
{
    // [SDR] 3.3.2: "13DU1:Meters:", "13DU2:Feet:", "13DU3:US Feet:"; "The
    // numeric code directly after DU specifies which distance unit ...
    // The text is informational only." It is sent to say US feet where
    // the header can say only feet.
    const char code = text.empty() ? ' ' : text.front();
    survey::LinearUnit unit = survey::LinearUnit::Unknown;
    switch (code) {
    case '1':
        unit = survey::LinearUnit::Metres;
        break;
    case '2':
        unit = survey::LinearUnit::Feet;
        break;
    case '3':
        unit = survey::LinearUnit::UsSurveyFeet;
        break;
    default:
        // It says nothing the header has not: the header's unit holds.
        builder_.skip(n, "the distance unit note gives " +
                             (text.empty() ? std::string("no code")
                                           : "the code '" + shown(std::string(1, code)) + "'") +
                             ", and the format defines 1 metres, 2 feet and 3 US feet; the "
                             "header's unit, " +
                             std::string(survey::toString(linearUnit_)) + ", holds");
        return;
    }
    // [SDR] 3.3.2: "All distances specified after this Note record are
    // displayed in the specified distance units" - the note governs.
    const bool agrees = unit == linearUnit_ || (unit == survey::LinearUnit::UsSurveyFeet &&
                                                linearUnit_ == survey::LinearUnit::Feet);
    if (!agrees) {
        builder_.warn(n, "the distance unit note says " + std::string(survey::toString(unit)) +
                             " where the header (record " + std::to_string(headerRecord_) +
                             ") says " + survey::toString(linearUnit_) +
                             "; the format has the note govern the distances after it, so they "
                             "are read in " +
                             survey::toString(unit));
    }
    linearUnit_ = unit;
    metres_ = survey::metresPer(unit).value();
    builder_.project().units.linear = unit;
    builder_.countRead();
}

void SdrReader::readNote(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string text = cleaned(fields.take(60));
    if (derivation == "DU") {
        readDistanceUnitNote(text, n);
        return;
    }
    if (text.empty()) {
        builder_.countRead();
        return;
    }
    if (derivation == "CP") {
        readCorrectionNote(text);
    }
    // [LISCAD]: the field book's "Set #" note begins a set, which in the
    // SDR2x layout its 12 closes afterwards.
    if (derivation == "SC" && text.starts_with("Set #")) {
        if (const survey::SurveyStation* station = builder_.currentStation(); station != nullptr) {
            setBoundary_ = station->observations.size();
        }
    }
    if (derivation == "TS") {
        if (const std::optional<survey::SurveyTimestamp> time = timeStampOf(text)) {
            survey::SurveyStation* station = builder_.currentStation();
            if (station != nullptr && !station->instrument.time.known()) {
                station->instrument.time = *time;
            }
            // [TA]'s form, which points to distances without the atmospheric
            // correction (see read()).
            writerTimeStamps_ = writerTimeStamps_ || text.find("Date ") != std::string::npos;
        } else {
            builder_.warn(n, "time stamp '" + text.substr(0, 60) +
                                 "' is not a date and time in a form the format's writers use; "
                                 "it is kept as a note");
        }
    }
    builder_.addStationNote(derivation == "NM" ? text : std::string(derivation) + ": " + text);
    builder_.countRead();
}

void SdrReader::dispatch(std::string_view line, std::size_t n)
{
    // The observation record before this one, if it was one, whatever this
    // one is: only a record straight after it can be its POS view.
    previousShot_ = std::exchange(lastShot_, ShotRecord{});
    if (!headerRead_) {
        // [SDR] chapter 3: "The first data record of each transmission is
        // always of type 00."
        if (!line.starts_with("00") || !isRecord(line)) {
            refuse(n, "the file does not begin with an SDR header record (type 00)");
            return;
        }
        readHeader(line, n);
        return;
    }
    std::size_t marks = 0;
    const std::string_view live = withoutDeletionMarks(line, marks);
    if (marks > 0) {
        // The mark as written, where it is short enough to read; a line of
        // D's is quoted by its length, as other warnings cut what they quote.
        constexpr std::size_t kQuotedMarks = 8;
        builder_.skip(n, "a deleted record (marked " +
                             (marks <= kQuotedMarks
                                  ? "'" + std::string(marks, 'D') + "'"
                                  : "with a run of " + std::to_string(marks) + " D's") +
                             ")" +
                             (isRecord(live) ? ", " + recordLabel(live) + "," : std::string{}) +
                             " is not imported");
        return;
    }
    if (!isRecord(line)) {
        builder_.skip(n, "'" + shown(trimmed(line.substr(0, 12))) +
                             "' is not a record: a record opens with a two-digit type and a "
                             "derivation code");
        return;
    }
    if (!nonAscii_) {
        for (const char c : line) {
            if (static_cast<unsigned char>(c) >= 0x80) {
                nonAscii_ = true;
                builder_.warn(n, "the record holds characters outside the ASCII the format "
                                 "allows; its fields are read by character position");
                break;
            }
        }
    }
    const std::string_view type = line.substr(0, 2);
    const std::string_view derivation = line.substr(2, 2);
    Fields fields(line, pointIdWidth_, realWidth_);
    if (type == "00") {
        readHeader(line, n);
    } else if (type == "01") {
        readInstrument(fields, n);
    } else if (type == "02") {
        readStation(fields, derivation, n);
    } else if (type == "03") {
        readTarget(fields, n);
    } else if (type == "04") {
        readCollimation(fields, n);
    } else if (type == "05") {
        readAtmosphere(fields, n);
    } else if (type == "06") {
        readScale(fields, n);
    } else if (type == "07") {
        readBacksight(fields, derivation, n);
    } else if (type == "08") {
        readPosition(fields, derivation, n);
    } else if (type == "09") {
        readObservation(fields, derivation, n);
    } else if (type == "10") {
        readJob(fields, n);
    } else if (type == "11") {
        readReduced(fields, n);
    } else if (type == "12") {
        readSet(fields, n);
    } else if (type == "13") {
        readNote(fields, derivation, n);
    } else {
        builder_.skip(n, "record " + recordLabel(line) + " is of a type this reader does not "
                                                         "import");
    }
}

Result<ReadResult> SdrReader::read(const std::vector<std::string_view>& lines,
                                   bool guessedEncoding, katana::core::TextEncoding encoding,
                                   bool endsWithLineEnd)
{
    if (guessedEncoding) {
        builder_.warn(0, "the file is not UTF-8; it was read as " +
                             std::string(katana::core::toString(encoding)) +
                             ", so an accented name may be wrong");
    }
    // Record numbers are line numbers: splitLines keeps a blank line in the
    // middle, and takes CR LF ([SDR] chapter 3), LF or a lone CR alike.
    std::size_t lastRecord = 0; // the line of the last record dispatched
    for (std::size_t i = 0; i < lines.size() && !fatal_; ++i) {
        const std::size_t n = i + 1;
        std::string_view line = lines[i];
        if (trimmed(line).empty()) {
            continue;
        }
        // [SDR] chapter 3: a transmission may open with STX and close with
        // ETX and a checksum. Neither is a record; the checksum is kept as
        // written and not checked.
        line = withoutStx(line);
        if (trimmed(line).empty()) {
            continue;
        }
        if (line.front() == '\x03') {
            builder_.project().metadata["transmission checksum"] = cleaned(line.substr(1));
            continue;
        }
        if (isEndOfFileMarks(line)) {
            continue;
        }
        dispatch(line, n);
        lastRecord = n;
    }
    if (fatal_) {
        return *fatal_;
    }
    if (!headerRead_) {
        return makeError(ErrorCode::FileImportFailure,
                         builder_.fileName() + " holds no SDR header record, so its units and "
                                               "its layout are unknown",
                         "Sokkia SDR");
    }
    // A last record with no line end, where [SDR] chapter 3 ends every one
    // with CR LF: a transfer cut inside it leaves a shorter number that is
    // still a number, and only this says so.
    if (!endsWithLineEnd && lastRecord != 0 && lastRecord == lines.size()) {
        builder_.warn(lastRecord, "the file ends inside this record, with no line end after it "
                                  "(the format ends every record with CR LF), so it may have been "
                                  "cut short: the record's last field may be incomplete");
    }
    finishSetup();
    settleDerivedViews();
    survey::SurveyProject& project = builder_.project();
    if (lostCorrected_ > 0) {
        builder_.notCarried(std::to_string(lostCorrected_) +
                            " corrected (MC) observation(s) that no raw observation of their "
                            "setup repeats: writers of the format disagree about what an MC "
                            "holds, so they are not imported and their shots are lost");
    }
    if (lostReduced_ > 0) {
        builder_.notCarried(std::to_string(lostReduced_) +
                            " reduced (RED) observation(s) with distances that no raw "
                            "observation of their setup repeats: they are not imported and their "
                            "shots are lost");
    }
    if (!project.stations.empty()) {
        std::string atmospheric =
            "whether the distances carry the atmospheric correction: Sokkia's field book applies "
            "it as each distance is accepted when the job's switch is on, and Trimble's "
            "published writer turns the switch on whenever it writes weather and leaves the "
            "correction out";
        if (!atmosphericSwitch_.empty()) {
            atmospheric += "; this job's switch is " + atmosphericSwitch_;
        }
        if (writerTimeStamps_) {
            atmospheric += "; this file's time stamps are written as Trimble's writer writes "
                           "them (\"Time Date MM/DD/YYYY\"), so if it wrote the file its "
                           "distances are without the correction and Recompute applies it";
        }
        builder_.notCarried(std::move(atmospheric) + "; the reduction applies none unless told "
                                                     "to recompute it");
        if (!weatherSeen_) {
            builder_.notCarried("no pressure and temperature to compute an atmospheric "
                                "correction from");
        }
        if (!instrumentSeen_) {
            builder_.notCarried("no instrument record: the prism constant, and whether the "
                                "distances carry one, are unknown");
        }
    }
    ReadResult result = builder_.finish();
    if (result.project.stations.empty() && result.project.points.empty() &&
        result.project.unpositionedPoints.empty()) {
        return makeError(ErrorCode::FileImportFailure,
                         result.project.source.fileName +
                             " holds no setup, observation or point Katana could read",
                         std::to_string(result.warnings.size()) + " warning(s)" +
                             (result.warnings.empty()
                                  ? std::string{}
                                  : ", the first: " +
                                        katana::surveyio::describe(result.warnings.front())));
    }
    return result;
}

// ---- Registration -----------------------------------------------------------------

// "00" and a derivation code, then "SDR2" or "SDR33" where the version
// field begins ([SDR] 3.2.5; [NIKON] writes "SDR33V04-01" with no blank).
bool isHeader(std::string_view line)
{
    return line.size() >= 9 && line[0] == '0' && line[1] == '0' && isCodeChar(line[2]) &&
           isCodeChar(line[3]) && line.substr(4).starts_with("SDR") &&
           (line[7] == '2' || line.substr(7, 2) == "33");
}

// How many lines of the probe's bytes are judged: 200 records of at most 150
// characters ([SDR] 3.6.2's longest, the GSTN record), with their line ends,
// fit inside kProbeBytes, so every file long enough is judged on the same
// number.
constexpr std::size_t kProbedLines = 200;

FormatSignature probe(const ProbeInput& input)
{
    const std::string_view bytes = katana::surveyio::withoutByteOrderMark(input.bytes);
    if (!katana::surveyio::looksLikeText(bytes)) {
        return katana::surveyio::ruledOut();
    }
    std::size_t lines = 0;
    std::size_t shaped = 0;
    std::string_view first;
    for (std::string_view line : katana::surveyio::probeLines(input, kProbedLines)) {
        // The framing the reader passes over: STX, ETX and its checksum,
        // DOS end-of-file marks.
        line = withoutStx(line);
        if (trimmed(line).empty() || line.front() == '\x03' || isEndOfFileMarks(line)) {
            continue;
        }
        if (lines == 0) {
            first = line;
        }
        ++lines;
        std::size_t marks = 0;
        if (isRecord(withoutDeletionMarks(line, marks))) {
            ++shaped;
        }
    }
    if (lines == 0) {
        return katana::surveyio::ruledOut();
    }
    const bool sdr = input.extension == "sdr";
    const bool header = isHeader(first);
    // A judgement, not a measurement: an SDR file is records and nothing
    // else, and a tenth leaves room for a damaged stretch or a converter's
    // unmarked notes.
    const bool mostlyRecords = shaped * 10 >= lines * 9;
    const std::string counted = std::to_string(shaped) + " of " + std::to_string(lines) +
                                " lines open with a record type and derivation code";
    if (header) {
        const std::string version = versionOf(first);
        if (mostlyRecords) {
            return {sdr ? 0.98 : 0.95, std::string(sdr ? "extension .sdr, " : "") + "header " +
                                           version + ", " + counted};
        }
        // No other format writes this header (the Survey Controller probe
        // steps aside for it), so it identifies the file even with odd lines
        // in it, which the reader skips, each with a warning - but less
        // surely than a clean one.
        return {0.8, "header " + version + " but only " + counted};
    }
    if (sdr && mostlyRecords) {
        return {0.4, "extension .sdr and " + counted + ", but no SDR header record"};
    }
    if (sdr) {
        return {0.1, "extension .sdr only"};
    }
    return katana::surveyio::ruledOut();
}

FormatDescriptor descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kFormatId);
    format.humanName = "Sokkia SDR33 / SDR2x";
    format.manufacturer = katana::surveyio::Manufacturer::Sokkia;
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = false,
                    .instrumentSettings = true,
                    .gnss = false};
    format.canImport = true;
    format.parserVersion = "1.0";
    format.extensions = {"sdr"};
    return format;
}

Result<ReadResult> readSdr(std::string_view bytes, std::string_view fileName,
                           const ReadOptions& options)
{
    Result<katana::core::DecodedText> decoded = katana::core::decodeText(bytes);
    if (!decoded) {
        return decoded.error();
    }
    const std::string_view text = decoded->text;
    const std::vector<std::string_view> lines = katana::core::splitLines(text);
    // The header names the variant, which every record's provenance carries.
    std::string variant = "SDR";
    std::string version;
    for (std::string_view line : lines) {
        line = withoutStx(line);
        if (trimmed(line).empty() || isEndOfFileMarks(line)) {
            continue;
        }
        if (isHeader(line)) {
            version = versionOf(line);
            variant = version.starts_with("SDR33") ? "SDR33" : "SDR2x";
        }
        break;
    }
    SdrReader reader(fileName,
                     survey::SourceRecord{"Sokkia", variant, version, std::string(fileName), 0},
                     options);
    const bool endsWithLineEnd = !text.empty() && (text.back() == '\n' || text.back() == '\r');
    return reader.read(lines, decoded->guessed, decoded->encoding, endsWithLineEnd);
}

const katana::surveyio::FormatRegistration kRegistration{descriptor(), &probe, &readSdr};

} // namespace
