//! Sokkia SDR (.sdr): the SDR electronic field book's Comms output - a job as
//! fixed-width ASCII records, in either of the two layouts the field book
//! writes: SDR33 (point names in a 16-character field; the field book lets a
//! name be 14 characters, [SDR] chapter 2) and SDR2x (4-digit point numbers).
//!
//! The port of src/katana_surveyio/sokkia_sdr.cpp, which has no header of its
//! own: the format reaches the program through its registration alone.

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
//     AZIMUTH is the setup's stated_backsight_azimuth and its horizontal
//     observation - the circle reading on the backsight - its
//     backsight_azimuth, the model's circle. One field for both would drop
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
//     correction applied"): its backsight_azimuth, with no backsight named,
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
//
// Text: the file is decoded to UTF-8 first (decode_text), as the C++ decodes
// it, and read as the bytes of that text. A cut the C++ makes at a byte
// offset (the header's fields, a quoted value cut at 40 bytes) can split a
// character; what such a cut keeps goes into the model with the split
// character replaced (text()), where the C++ keeps the bytes.

#![allow(
    clippy::manual_range_contains,
    reason = "The C++'s two comparisons, which let a NaN through where a range's contains would not."
)]

use std::collections::{BTreeMap, BTreeSet};

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, is_ascii_space, split_lines, trimmed};
use katana_core::text_encoding::{self, TextEncoding, decode_text};
use katana_core::{Error, ErrorCode, Result};
use katana_math::cpp;
use katana_math::numerics::{PI, TWO_PI};
use katana_math::unit_ratio::{UnitRatio, scale_by_ratio, to_metres, units};
use katana_survey::angles::{degrees_to_radians, gon_to_radians};
use katana_survey::data_model::{self, distance_sigma, metres_per, observation_pointing};
use katana_survey::{
    AngularUnit, CoordinateSource, CorrectionState, DeclaredUnits, DistanceKind,
    DistanceObservation, Face, HorizontalDirectionObservation, InstrumentSettings, LinearUnit,
    Observation, Pointing, SourceRecord, SurveyStation, SurveyTimestamp, ZenithAngleObservation,
};

use crate::detect::{looks_like_text, probe_lines, without_byte_order_mark};
use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, ProbeInput,
    ReadOptions, ReadResult, describe, ruled_out,
};
use crate::topcon_raw_builder::{
    RawProjectBuilder, RestatedCoordinates, face_of_zenith_reading, parse_real, wrap_to_circle,
    zenith_in_model_range,
};

const FORMAT_ID: &str = "sokkia-sdr";

// [SDR] 3.3.1: 6400 mils to the circle.
const RADIANS_PER_MIL: f64 = PI / 3200.0;

// Hectopascals in one conventional millimetre of mercury: 13.5951 g/cm3 x
// 9.80665 m/s2 x 1 mm = 133.322387415 Pa, the definition NIST Special
// Publication 811 (2008), appendix B.8, gives as 1.333 224 E+02 Pa. An inch
// of mercury is as many of them as an inch has millimetres (3.386 389 E+03 Pa
// there), the inch being unit_ratio.rs's.
const HECTOPASCALS_PER_MILLIMETRE_OF_MERCURY: f64 = 1.33322387415;
const MILLIMETRES_PER_INCH: f64 = scale_by_ratio(
    1.0,
    units::INTERNATIONAL_INCH.numerator * units::MILLIMETRE.denominator,
    units::INTERNATIONAL_INCH.denominator * units::MILLIMETRE.numerator,
);
const HECTOPASCALS_PER_INCH_OF_MERCURY: f64 =
    MILLIMETRES_PER_INCH * HECTOPASCALS_PER_MILLIMETRE_OF_MERCURY;

// Rounds of one setup whose circle readings on the backsight differ by more
// than this were not observed on one orientation (see the top of this file).
// One minute of arc.
const SAME_CIRCLE: f64 = PI / 10800.0;

// The refraction constants of the job record's option: [SDR] 3.5 ("1 0.14,
// 2 0.20") and, for a header in [NIKON]'s form, [NIKON] page 172 ("0.132 or
// 0.200").
const REFRACTION_OPTION_1: f64 = 0.14;
const NIKON_REFRACTION_OPTION_1: f64 = 0.132;
const REFRACTION_OPTION_2: f64 = 0.20;

// How many lines of the probe's bytes are judged: 200 records of at most 150
// characters ([SDR] 3.6.2's longest, the GSTN record), with their line ends,
// fit inside PROBE_BYTES, so every file long enough is judged on the same
// number.
const PROBED_LINES: usize = 200;

// A setup is current wherever this is said: the record began or found one.
const CURRENT: &str = "a setup is current here";

// Whether two circle readings (radians) are within SAME_CIRCLE of each other,
// round the circle's zero, and `slack` more: the horizontal collimation one of
// them may carry and the other not (see the top of this file).
fn on_same_circle(a: f64, b: f64, slack: f64) -> bool {
    let mut difference = (wrap_to_circle(a) - wrap_to_circle(b)).abs();
    difference = cpp::min(difference, TWO_PI - difference);
    difference <= SAME_CIRCLE + slack
}

// A horizontal reading as face 1 reads that line: face 2 less half a circle
// ([SETX] 29.2.3's faces), a reading of no known face taken as face 1.
fn face_one_reading(direction: &HorizontalDirectionObservation) -> f64 {
    if direction.pointing.face == Face::Right {
        wrap_to_circle(direction.direction - PI)
    } else {
        direction.direction
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Variant {
    Sdr33,
    Sdr2x,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AngleUnit {
    Degrees,
    Gons,
    Mils,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PressureUnit {
    Unknown,
    MillimetresOfMercury,
    InchesOfMercury,
    Millibars,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemperatureUnit {
    Unknown,
    Celsius,
    Fahrenheit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CoordinateOrder {
    Unknown,
    NorthFirst,
    EastFirst,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VerticalReference {
    Zenith,
    Horizon,
    Unknown,
}

// A field as the model holds text: the bytes of the decoded file are UTF-8,
// and only a cut at a byte offset can leave a split character, which is
// replaced (see the top of this file).
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// std::string_view::substr(pos, count): `pos` past the end is the C++'s
// out_of_range, here a panic, which read_survey reports as the C++'s does.
fn substr(bytes: &[u8], pos: usize, count: usize) -> &[u8] {
    let end = pos.saturating_add(count).min(bytes.len());
    &bytes[pos..end]
}

// The first `byte` at or after `from` (from <= bytes.len()).
fn find(bytes: &[u8], byte: u8, from: usize) -> Option<usize> {
    bytes[from..]
        .iter()
        .position(|&c| c == byte)
        .map(|at| at + from)
}

// Where `needle` first stands in `haystack`.
fn find_text(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn signature(confidence: f64, evidence: String) -> FormatSignature {
    FormatSignature {
        confidence,
        evidence,
    }
}

fn is_code_char(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == b' '
}

fn is_control(c: u8) -> bool {
    c < 0x20 || c == 0x7F
}

// Two digits of record type and two characters of derivation code ([SDR]
// chapter 3), the derivation possibly blank ("used in reports", 3.4).
fn is_record(line: &[u8]) -> bool {
    line.len() >= 4
        && line[0].is_ascii_digit()
        && line[1].is_ascii_digit()
        && is_code_char(line[2])
        && is_code_char(line[3])
}

// `line` with its deleted-record mark stepped over, and how many marks that
// was: every leading D, where there are two or more ("DD", "DDDD"). A single D
// is no mark, and `line` comes back whole.
fn without_deletion_marks(line: &[u8]) -> (&[u8], usize) {
    let count = line.iter().take_while(|&&c| c == b'D').count();
    let marks = if count >= 2 { count } else { 0 };
    (&line[marks..], marks)
}

// `line` without the STX ([SDR] chapter 3) a transmission opens with: on a
// line of its own, or glued to the first record by a writer that left out the
// line end after it.
fn without_stx(line: &[u8]) -> &[u8] {
    line.strip_prefix(b"\x02").unwrap_or(line)
}

// A line of DOS end-of-file marks (Ctrl-Z, 0x1A), which a file copied through
// MS-DOS keeps: framing, not a record.
fn is_end_of_file_marks(line: &[u8]) -> bool {
    let line = trimmed(line);
    !line.is_empty() && line.iter().all(|&c| c == 0x1a)
}

// What each record type is, in [SDR] 3.6.2's words, for the warnings that name
// a record - a deleted one, or one of a type this reader does not import;
// empty for a type [SDR] does not list.
fn record_name(kind: &[u8]) -> &'static str {
    const NAMES: [(&[u8], &str); 57] = [
        (b"00", "header"),
        (b"01", "instrument"),
        (b"02", "station"),
        (b"03", "target height"),
        (b"04", "collimation"),
        (b"05", "atmosphere"),
        (b"06", "scale factor"),
        (b"07", "backsight"),
        (b"08", "coordinates"),
        (b"09", "observation"),
        (b"10", "job"),
        (b"11", "reduced observation"),
        (b"12", "set"),
        (b"13", "note"),
        (b"14", "GPS instrument"),
        (b"15", "GPS station"),
        (b"16", "GPS observation"),
        (b"17", "GPS reduced observation"),
        (b"18", "GPS position"),
        (b"19", "GPS projection"),
        (b"21", "GPS vertical adjustment"),
        (b"24", "GPS latitude and longitude"),
        (b"25", "road station"),
        (b"26", "road position"),
        (b"27", "road check"),
        (b"28", "road name"),
        (b"29", "horizontal alignment"),
        (b"30", "horizontal alignment point"),
        (b"31", "horizontal straight"),
        (b"32", "horizontal arc"),
        (b"33", "horizontal spiral"),
        (b"34", "vertical alignment"),
        (b"35", "circular vertical curve"),
        (b"36", "parabolic vertical curve"),
        (b"37", "vertical alignment point"),
        (b"38", "cross section"),
        (b"39", "template"),
        (b"40", "template offset and height difference"),
        (b"41", "template grade and distance"),
        (b"42", "template side slope"),
        (b"44", "apply superelevation"),
        (b"45", "define superelevation"),
        (b"46", "template element"),
        (b"47", "template side slope"),
        (b"50", "GPS horizontal adjustment"),
        (b"57", "antenna height"),
        (b"60", "levelling"),
        (b"61", "levelling"),
        (b"62", "levelling"),
        (b"63", "levelling"),
        (b"64", "levelling"),
        (b"65", "levelling offset"),
        (b"66", "GPS raw observation"),
        (b"96", "latitude and longitude station"),
        (b"97", "transformation"),
        (b"98", "WGS84 latitude and longitude"),
        (b"99", "local latitude and longitude"),
    ];
    NAMES
        .iter()
        .find(|(known, _)| *known == kind)
        .map_or("", |(_, name)| *name)
}

// "01NM (instrument)": a record's type, derivation code and name.
fn record_label(record: &[u8]) -> String {
    let name = record_name(substr(record, 0, 2));
    let label = text(substr(record, 0, 4));
    if name.is_empty() {
        label
    } else {
        format!("{label} ({name})")
    }
}

// `text` as a warning quotes it: each control byte written "\xNN", since a
// byte a person cannot see must not become a blank they cannot tell from one.
fn shown(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(bytes.len());
    for &c in bytes {
        if is_control(c) {
            out.extend_from_slice(b"\\x");
            out.push(HEX[usize::from(c >> 4)]);
            out.push(HEX[usize::from(c & 0x0F)]);
        } else {
            out.push(c);
        }
    }
    text(&out)
}

// A text field as something to keep: control bytes blanked, THEN trimmed, so a
// field of nothing but line noise is empty rather than a blank.
fn cleaned_bytes(field: &[u8]) -> Vec<u8> {
    let blanked: Vec<u8> = field
        .iter()
        .map(|&c| if is_control(c) { b' ' } else { c })
        .collect();
    trimmed(&blanked).to_vec()
}

fn cleaned(field: &[u8]) -> String {
    text(&cleaned_bytes(field))
}

// `text` with every run of blanks made one: "SDR33     V04-03" is "SDR33
// V04-03".
fn collapsed_blanks(field: &[u8]) -> Vec<u8> {
    let mut collapsed: Vec<u8> = Vec::new();
    for &c in trimmed(field) {
        if c != b' ' || collapsed.last() != Some(&b' ') {
            collapsed.push(c);
        }
    }
    collapsed
}

// A header record's version (columns 5-20), its runs of blanks made one - the
// one reading of it, for the metadata and the provenance alike. Empty for a
// record too short to have one.
fn version_of(header: &[u8]) -> String {
    if header.len() > 4 {
        text(&collapsed_blanks(&cleaned_bytes(substr(header, 4, 16))))
    } else {
        String::new()
    }
}

// The fields of one record, read left to right from its fifth character
// ([SDR] chapter 3: characters 1-4 are the type and the derivation code). A
// column is a CHARACTER: the file was decoded to UTF-8 first, so a name typed
// in another code page is more than one byte and the fields after it would
// shift if bytes were counted. A record shortened by dropped trailing blanks
// ([SDR] chapter 5) simply has shorter fields, empty past its end.
struct Fields<'a> {
    record: &'a [u8],
    point_id_width: usize,
    real_width: usize,
    column: usize,
    ascii: bool,
    starts: Vec<usize>,
}

impl<'a> Fields<'a> {
    fn new(record: &'a [u8], point_id_width: usize, real_width: usize) -> Self {
        let ascii = record.iter().all(|&c| c < 0x80);
        let starts = if ascii {
            Vec::new()
        } else {
            record
                .iter()
                .enumerate()
                .filter(|&(_, &c)| (c & 0xC0) != 0x80)
                .map(|(i, _)| i)
                .collect()
        };
        Self {
            record,
            point_id_width,
            real_width,
            column: 4,
            ascii,
            starts,
        }
    }

    fn take(&mut self, width: usize) -> &'a [u8] {
        let count = if self.ascii {
            self.record.len()
        } else {
            self.starts.len()
        };
        let first = self.column.min(count);
        let last = (self.column + width).min(count);
        self.column += width;
        let record = self.record;
        &record[self.offset(first)..self.offset(last)]
    }

    fn point_id(&mut self) -> &'a [u8] {
        self.take(self.point_id_width)
    }

    fn real(&mut self) -> &'a [u8] {
        self.take(self.real_width)
    }

    fn offset(&self, character: usize) -> usize {
        if self.ascii {
            return character;
        }
        if character < self.starts.len() {
            self.starts[character]
        } else {
            self.record.len()
        }
    }
}

// The first word of a description, which is its field code.
fn first_word(description: &str) -> &str {
    match description.find(' ') {
        Some(blank) => &description[..blank],
        None => description,
    }
}

// ---- Time stamps --------------------------------------------------------------

fn digits_value(text: &[u8]) -> Option<i32> {
    if text.is_empty() || text.len() > 4 {
        return None;
    }
    let mut value = 0;
    for &c in text {
        if !c.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i32::from(c - b'0');
    }
    Some(value)
}

fn month_named(text: &[u8]) -> Option<i32> {
    const MONTHS: [&[u8]; 12] = [
        b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
        b"Dec",
    ];
    MONTHS
        .iter()
        .position(|month| text.eq_ignore_ascii_case(month))
        .map(|i| i as i32 + 1)
}

// "HH:MM" or "HH:MM:SS" into `time`.
fn read_clock(text: &[u8], time: &mut SurveyTimestamp) -> bool {
    let Some(first) = find(text, b':', 0) else {
        return false;
    };
    let second = find(text, b':', first + 1);
    let hour = digits_value(&text[..first]);
    let minute = digits_value(match second {
        Some(second) => &text[first + 1..second],
        None => &text[first + 1..],
    });
    let seconds = match second {
        Some(second) => digits_value(&text[second + 1..]),
        None => Some(0),
    };
    let (Some(hour), Some(minute), Some(seconds)) = (hour, minute, seconds) else {
        return false;
    };
    if hour > 23 || minute > 59 || seconds > 60 {
        return false;
    }
    time.hour = hour;
    time.minute = minute;
    time.second = f64::from(seconds);
    true
}

// A day of the proleptic Gregorian calendar, leap years included. The C++
// asks C++20 <chrono> rather than keep another copy of the month lengths; the
// Rust standard library has no calendar, so the rule is written out: a year
// divisible by 4 is a leap year unless it is divisible by 100 and not by 400.
fn plausible_date(year: i32, month: i32, day: i32) -> bool {
    if year <= 0 || month < 1 || month > 12 || day < 1 || day > 31 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= days
}

// A 13TS note's date and time (see the top of this file for the forms).
fn time_stamp_of(text: &[u8]) -> Option<SurveyTimestamp> {
    let mut time = SurveyTimestamp::default();
    let text = trimmed(text);
    // [TA]: "Time Date MM/DD/YYYY Time HH:MM:SS", as "Time Date 03/14/2026
    // Time 08:15:00".
    if let Some(date) = find_text(text, b"Date ") {
        let mut rest = trimmed(&text[date + 5..]);
        let end = find(rest, b' ', 0);
        let day = match end {
            Some(end) => &rest[..end],
            None => rest,
        };
        let slash1 = find(day, b'/', 0)?;
        let slash2 = find(day, b'/', slash1 + 1)?;
        let month = digits_value(&day[..slash1]);
        let day_of_month = digits_value(&day[slash1 + 1..slash2]);
        let year = digits_value(&day[slash2 + 1..]);
        let (Some(month), Some(day_of_month), Some(year)) = (month, day_of_month, year) else {
            return None;
        };
        if day[slash2 + 1..].len() != 4 || !plausible_date(year, month, day_of_month) {
            return None;
        }
        time.year = year;
        time.month = month;
        time.day = day_of_month;
        rest = match end {
            Some(end) => trimmed(&rest[end..]),
            None => &[],
        };
        if rest.starts_with(b"Time ") && !read_clock(trimmed(&rest[5..]), &mut time) {
            return None;
        }
        return Some(time);
    }
    // The field book's own: "18-Jan-80 20:14", and "May-12-97 10:53".
    let blank = find(text, b' ', 0);
    let day = match blank {
        Some(blank) => &text[..blank],
        None => text,
    };
    let dash1 = find(day, b'-', 0)?;
    let dash2 = find(day, b'-', dash1 + 1)?;
    let a = &day[..dash1];
    let b = &day[dash1 + 1..dash2];
    let c = &day[dash2 + 1..];
    let day_first = month_named(b).is_some();
    let month = if day_first {
        month_named(b)
    } else {
        month_named(a)
    };
    let day_of_month = digits_value(if day_first { a } else { b });
    let short_year = if c.len() == 2 { digits_value(c) } else { None };
    let (Some(month), Some(day_of_month), Some(short_year)) = (month, day_of_month, short_year)
    else {
        return None;
    };
    // POSIX strptime %y: 69-99 are 1969-1999, 00-68 are 2000-2068.
    let year = if short_year >= 69 {
        1900 + short_year
    } else {
        2000 + short_year
    };
    if !plausible_date(year, month, day_of_month) {
        return None;
    }
    time.year = year;
    time.month = month;
    time.day = day_of_month;
    if let Some(blank) = blank
        && !read_clock(trimmed(&text[blank..]), &mut time)
    {
        return None;
    }
    Some(time)
}

// ---- Observations as the model holds them -------------------------------------

// The point an observation is TO, for the kinds that have one.
fn target_of(observation: &Observation) -> Option<&str> {
    match observation {
        Observation::Distance(o) => Some(&o.to),
        Observation::HorizontalAngle(o) => Some(&o.to),
        Observation::VerticalAngle(o) => Some(&o.to),
        Observation::ZenithAngle(o) => Some(&o.to),
        Observation::Azimuth(o) => Some(&o.to),
        Observation::GnssBaseline(o) => Some(&o.to),
        Observation::LevelDifference(o) => Some(&o.to),
        Observation::HorizontalDirection(o) => Some(&o.to),
        Observation::GnssGeocentricBaseline(o) => Some(&o.to),
        Observation::GnssPosition(_) | Observation::GnssGlobalPosition(_) => None,
    }
}

// The pointing of an observation, to renumber it, for the kinds that have one
// (observation_pointing's, writable).
fn pointing_of(observation: &mut Observation) -> Option<&mut Pointing> {
    match observation {
        Observation::Distance(o) => Some(&mut o.pointing),
        Observation::HorizontalAngle(o) => Some(&mut o.pointing),
        Observation::VerticalAngle(o) => Some(&mut o.pointing),
        Observation::ZenithAngle(o) => Some(&mut o.pointing),
        Observation::HorizontalDirection(o) => Some(&mut o.pointing),
        _ => None,
    }
}

fn direction_of(observation: &Observation) -> Option<&HorizontalDirectionObservation> {
    match observation {
        Observation::HorizontalDirection(direction) => Some(direction),
        _ => None,
    }
}

// The horizontal reading of `station`'s raw record `record` (an index into
// `record_starts`), or None when it has none.
fn direction_of_record<'s>(
    station: &'s SurveyStation,
    record_starts: &[usize],
    record: usize,
) -> Option<&'s HorizontalDirectionObservation> {
    let begin = record_starts[record];
    let end = record_starts
        .get(record + 1)
        .copied()
        .unwrap_or(station.observations.len());
    station
        .observations
        .iter()
        .take(end)
        .skip(begin)
        .find_map(direction_of)
}

// The metadata of the current setup, or the project's before the first.
fn current_metadata(builder: &mut RawProjectBuilder) -> &mut BTreeMap<String, String> {
    let project = builder.project();
    match project.stations.last_mut() {
        Some(station) => &mut station.metadata,
        None => &mut project.metadata,
    }
}

// ---- The reader -----------------------------------------------------------------

// One setup's backsight, as its 07 records gave it.
#[derive(Clone, Debug, Default)]
struct Orientation {
    set: bool,
    backsight: String,
    // radians
    azimuth: Option<f64>,
    // the first round's circle reading, radians
    reading: Option<f64>,
    rounds: usize,
    record: usize,
}

// An azimuth keyed from the setup's point with no distance (an 11): what
// orients a setup that has no 07.
#[derive(Clone, Debug)]
struct KeyedAzimuth {
    to: String,
    // radians
    azimuth: f64,
    record: usize,
}

// The corrections of the latest 04 ([SETX] 29.2.5), radians; record 0 until
// one is read.
#[derive(Clone, Copy, Debug, Default)]
struct Collimation {
    vertical: f64,
    horizontal: f64,
    record: usize,
}

// What the observations before a setup's first 07 say of its backsight:
// whether any is of it, and how many of their horizontal readings on it are on
// the 07's circle and how many are not.
#[derive(Clone, Copy, Debug, Default)]
struct BacksightReadings {
    observed: bool,
    on_circle: usize,
    off_circle: usize,
}

// A 09 MC or an 11 with distances from the setup's point whose line no raw
// observation has repeated yet: its verdict waits for the setup's end.
#[derive(Clone, Debug)]
struct PendingDerivedView {
    record: usize,
    // an MC; otherwise a RED
    corrected: bool,
    from: String,
    to: String,
}

// The northing, easting and elevation a 02 or 08 states, metres.
#[derive(Clone, Copy, Debug)]
struct StatedCoordinates {
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
}

// An observation record read - a 09 F1, F2, MD or MC, or an 11 with distances
// - whatever the reader then did with it: its record, its target, what a
// warning calls it and the view the field book keeps it in; record 0 for none.
#[derive(Clone, Debug, Default)]
struct ShotRecord {
    record: usize,
    to: String,
    what: &'static str,
    view: &'static str,
}

struct SdrReader {
    builder: RawProjectBuilder,
    fatal: Option<Error>,

    // The header.
    header_read: bool,
    header_record: usize,
    header_options: Vec<u8>,
    variant: Variant,
    // Written as [NIKON] writes its header: no blank before the version.
    nikon_form: bool,
    point_id_width: usize,
    real_width: usize,
    angle_unit: AngleUnit,
    linear_unit: LinearUnit,
    metres: UnitRatio,
    pressure_unit: PressureUnit,
    temperature_unit: TemperatureUnit,
    order: CoordinateOrder,
    order_option: u8,
    // option 2's or 3's doubt, said once
    order_warned: bool,

    // The job.
    job_read: bool,
    record_elevations: bool,
    // "on", "off" or empty when the file does not say
    atmospheric_switch: String,
    // A 13TS note in [TA]'s form was read.
    writer_time_stamps: bool,

    // What applies to the next observation.
    settings: InstrumentSettings,
    instrument_seen: bool,
    instrument_warned: bool,
    vertical: VerticalReference,
    target_height: f64,
    target_height_seen: bool,
    target_height_warned: bool,
    bad_set: bool,
    // the bad set's raw observations still to come
    bad_set_remaining: Option<usize>,
    bad_set_name: String,
    bad_set_record: usize,
    collimation: Collimation,
    // The EDM type option of the latest 01 that gave one: the instrument type
    // a collimation holds for ([SETX] 13.1).
    edm_type: String,
    // The 01 that last ended a collimation by another instrument type, and
    // that collimation's 04: for the warning at a shot of the same setup after
    // it.
    collimation_ended_at: usize,
    ended_collimation: usize,
    // The largest horizontal collimation correction applied to a reading since
    // the latest 02, radians: how far a corrected reading and a 07's circle
    // reading on one line may differ with the circle unmoved.
    applied_collimation: f64,

    // The current setup.
    orientation: Orientation,
    keyed: Vec<KeyedAzimuth>,
    // Its raw (F1, F2, MD) observation records read.
    setup_observations: usize,
    // Where its latest set began among its observations, and whether a 07 has
    // been read since: a 07 SC after a set's shots closes that set.
    set_start: Option<usize>,
    set_start_record: usize,
    backsight_since_set: bool,
    // An SDR2x 12 read after raw records: where the set it would close begins,
    // and how many records that is, for its 07 SC to choose.
    set_before: Option<usize>,
    set_before_records: usize,
    // Where a set the SDR2x layout closes with its 12 could have begun: the
    // setup's start, its last 07 or 12, or a "Set #" note.
    set_boundary: usize,
    // The first observation of each of its raw records, in order.
    record_starts: Vec<usize>,
    // Each 01, 05 or 06 read after the setup's first observation: it belongs
    // to the next setup unless another shot of this one follows.
    pending_changes: Vec<(String, usize)>,
    weather_seen: bool,
    non_ascii: bool,

    // Since the latest 02: the targets observed raw from its point, whatever
    // setup a 07 put them in - what an MC or RED is the reduction of - and the
    // derived views whose line none of them has observed yet.
    occupation_targets: BTreeSet<String>,
    pending_derived: Vec<PendingDerivedView>,
    // Derived views with no raw twin in their setup: the shots are lost, which
    // the file's summary says.
    lost_corrected: usize,
    lost_reduced: usize,

    // The observation record read last, if the record read last was one, and
    // the one before the record being read, if that was one: an 08 of its
    // target straight after it is its POS view.
    last_shot: ShotRecord,
    previous_shot: ShotRecord,
    // Points given coordinates by a position record - a 02, or an 08 that is
    // no observation's POS view - which an observation's POS view never
    // supersedes ([SETX] 6.1 rule 2).
    position_recorded: BTreeSet<String>,
}

// [SDR] 3.3 (i): "an optional minus character, followed by a sequence of at
// least one digit ..., optionally followed by a decimal point ..., optionally
// followed by one or more digits" - no plus, no exponent. A leading point
// (".5") is let through: it is one number however it is read.
fn is_sdr_real(text: &[u8]) -> bool {
    let text = text.strip_prefix(b"-").unwrap_or(text);
    let mut digits = 0;
    let mut point = false;
    for &c in text {
        if c.is_ascii_digit() {
            digits += 1;
        } else if c == b'.' && !point {
            point = true;
        } else {
            return false;
        }
    }
    digits > 0
}

// Trimmed at both ends ([SDR] 3.2 pads alpha fields on the right, and writers
// are met that pad them on the left).
fn point_id_of(field: &[u8]) -> String {
    text(trimmed(field))
}

impl SdrReader {
    fn new(file_name: &str, source: SourceRecord, options: &ReadOptions) -> Self {
        let mut builder = RawProjectBuilder::new(file_name, source, options);
        // [SDR] 2.3: the latest coordinates of a point are the best.
        builder.set_restated_coordinates(RestatedCoordinates::LatestKept);
        Self {
            builder,
            fatal: None,
            header_read: false,
            header_record: 0,
            header_options: Vec::new(),
            variant: Variant::Sdr33,
            nikon_form: false,
            point_id_width: 16,
            real_width: 16,
            angle_unit: AngleUnit::Degrees,
            linear_unit: LinearUnit::Metres,
            metres: units::METRE,
            pressure_unit: PressureUnit::Unknown,
            temperature_unit: TemperatureUnit::Unknown,
            order: CoordinateOrder::Unknown,
            order_option: b' ',
            order_warned: false,
            job_read: false,
            record_elevations: true,
            atmospheric_switch: String::new(),
            writer_time_stamps: false,
            // [SETX] 29.1: curvature and refraction are applied between a raw
            // observation and its corrected (MC) view, so never in an F1, F2
            // or MD reading - whatever the job's switch, which is about that
            // view.
            settings: InstrumentSettings {
                curvature_refraction_state: CorrectionState::NotApplied,
                ..Default::default()
            },
            instrument_seen: false,
            instrument_warned: false,
            vertical: VerticalReference::Zenith,
            target_height: 0.0,
            target_height_seen: false,
            target_height_warned: false,
            bad_set: false,
            bad_set_remaining: None,
            bad_set_name: String::new(),
            bad_set_record: 0,
            collimation: Collimation::default(),
            edm_type: String::new(),
            collimation_ended_at: 0,
            ended_collimation: 0,
            applied_collimation: 0.0,
            orientation: Orientation::default(),
            keyed: Vec::new(),
            setup_observations: 0,
            set_start: None,
            set_start_record: 0,
            backsight_since_set: false,
            set_before: None,
            set_before_records: 0,
            set_boundary: 0,
            record_starts: Vec::new(),
            pending_changes: Vec::new(),
            weather_seen: false,
            non_ascii: false,
            occupation_targets: BTreeSet::new(),
            pending_derived: Vec::new(),
            lost_corrected: 0,
            lost_reduced: 0,
            last_shot: ShotRecord::default(),
            previous_shot: ShotRecord::default(),
            position_recorded: BTreeSet::new(),
        }
    }

    fn refuse(&mut self, n: usize, message: String) {
        if self.fatal.is_none() {
            self.fatal = Some(make_error(
                ErrorCode::FileImportFailure,
                format!("record {n} of {}: {message}", self.builder.file_name()),
                if self.variant == Variant::Sdr33 {
                    "Sokkia SDR33"
                } else {
                    "Sokkia SDR2x"
                },
            ));
        }
    }

    // The observations of the current setup, as many as there are.
    fn current_observation_count(&mut self) -> usize {
        self.builder
            .current_station()
            .map_or(0, |station| station.observations.len())
    }

    fn number(&mut self, field: &[u8], what: &str, n: usize) -> Option<f64> {
        // [SDR] 3.3 (ii): a field of blanks is null, "not measured".
        let value_text = trimmed(field);
        if value_text.is_empty() {
            return None;
        }
        let value = if is_sdr_real(value_text) {
            parse_real(value_text)
        } else {
            None
        };
        if value.is_none() {
            self.builder.warn(
                n,
                format!(
                    "{what} '{}' is not a number; it is read as not measured",
                    shown(substr(value_text, 0, 40))
                ),
            );
        }
        value
    }

    fn angle(&mut self, field: &[u8], what: &str, n: usize) -> Option<f64> {
        let value = self.number(field, what, n)?;
        Some(match self.angle_unit {
            AngleUnit::Degrees => degrees_to_radians(value),
            AngleUnit::Gons => gon_to_radians(value),
            AngleUnit::Mils => value * RADIANS_PER_MIL,
        })
    }

    fn length(&mut self, field: &[u8], what: &str, n: usize) -> Option<f64> {
        let value = self.number(field, what, n)?;
        Some(to_metres(value, self.metres))
    }

    // True, with the record skipped and the byte named, when one of the point
    // id fields holds a control byte.
    fn refused_ids(&mut self, n: usize, fields: &[&[u8]]) -> bool {
        for field in fields {
            if let Some(&c) = field.iter().find(|&&c| is_control(c)) {
                self.builder.skip(
                    n,
                    format!(
                        "a point id holds the control byte {}, which no point name can; the \
                         record is not imported",
                        shown(&[c])
                    ),
                );
                return true;
            }
        }
        false
    }

    // The coordinates a 02 or 08 states for `id`, read in the header's order;
    // None when they cannot place it (a height alone is kept in the point's
    // metadata, one ordinate alone said), or refused.
    fn stated_coordinates(
        &mut self,
        id: &str,
        first: &[u8],
        second: &[u8],
        elevation: &[u8],
        n: usize,
    ) -> Option<StatedCoordinates> {
        let first_stated = !trimmed(first).is_empty();
        let second_stated = !trimmed(second).is_empty();
        // [SETX] 4: with the job's "Record elev" No every point has "the same
        // (indeterminate) elevation" - a placeholder, not a height.
        if !first_stated && !second_stated {
            // A height alone cannot place a point; it is kept, not dropped.
            let height = if self.record_elevations {
                self.length(elevation, "elevation", n)
            } else {
                None
            };
            if let Some(height) = height {
                self.builder.add_point_metadata(
                    id,
                    String::from("height without a position"),
                    format_exact_real(height),
                );
                self.builder.warn(
                    n,
                    format!(
                        "point '{id}' is given an elevation ({} m) and no northing or easting; a \
                         height alone cannot place a point, so it is kept in the point's metadata",
                        format_exact_real(height)
                    ),
                );
            }
            return None;
        }
        if self.order == CoordinateOrder::Unknown {
            let option = shown(&[self.order_option]);
            self.refuse(
                n,
                format!(
                    "the record states coordinates, and the header's coordinate order option is \
                     '{option}'; the format defines 1 (north first) and 2 (east first), and \
                     Katana will not guess which ordinate is the northing"
                ),
            );
            return None;
        }
        let north_first = self.order == CoordinateOrder::NorthFirst;
        // Neither east-first option is settled by the field book's own files
        // (see the top of this file): said once, at the first coordinates read.
        if !self.order_warned && (self.order_option == b'2' || self.order_option == b'3') {
            self.order_warned = true;
            self.builder.warn(
                n,
                String::from(if self.order_option == b'2' {
                    "the header's coordinate order option is 2, east-north-elevation, and the \
                     coordinates are read easting first, as Trimble's published writer writes \
                     them; the format's record layout names the first field the northing, and \
                     no source shows which order Sokkia's field book sends under 2 (its one \
                     example is a printed report), so a file of its may hold the northing first \
                     - check a known point"
                } else {
                    "the header's coordinate order option is 3, which the format's publisher \
                     does not define: Trimble's published writer uses it for Y-X-Z, the easting \
                     first, and the coordinates are read so; Sokkia's field book lists \
                     south-west-elevation as a third display order, so a file set to that would \
                     be read wrongly - check a known point"
                }),
            );
        }
        let a = self.length(first, if north_first { "northing" } else { "easting" }, n);
        let b = self.length(second, if north_first { "easting" } else { "northing" }, n);
        let z = if self.record_elevations {
            self.length(elevation, "elevation", n)
        } else {
            None
        };
        let (Some(a), Some(b)) = (a, b) else {
            if a.is_some() || b.is_some() {
                self.builder.warn(
                    n,
                    format!(
                        "point '{id}' is given only one of its northing and easting; it is \
                         imported without coordinates"
                    ),
                );
            }
            return None;
        };
        Some(StatedCoordinates {
            northing: if north_first { a } else { b },
            easting: if north_first { b } else { a },
            elevation: z,
        })
    }

    fn coordinates(
        &mut self,
        id: &str,
        first: &[u8],
        second: &[u8],
        elevation: &[u8],
        how: CoordinateSource,
        n: usize,
    ) {
        if let Some(stated) = self.stated_coordinates(id, first, second, elevation, n) {
            self.builder.position_point(
                id,
                stated.northing,
                stated.easting,
                stated.elevation,
                how,
                n,
                "",
            );
            // A position record: coordinates the field book uses before any
            // observation's ([SETX] 6.1 rule 2).
            self.position_recorded.insert(String::from(id));
        }
    }

    // The settings a setup begun now starts with, and the current setup's when
    // it has no observation yet (an 01, 05 or 06 read between its 02 and its
    // first shot belongs to it).
    fn settings_changed(&mut self, n: usize, what: &str) {
        let no_observation_yet = self.setup_observations == 0;
        let Some(station) = self.builder.current_station() else {
            return;
        };
        if no_observation_yet {
            let time = station.instrument.time.clone();
            station.instrument = self.settings.clone();
            station.instrument.time = time;
            return;
        }
        // Between one setup's shots and the next 02 is where the field book
        // writes the next setup's instrument and weather; only a shot of the
        // SAME setup after it says the values changed in the middle of one.
        // Every such record is kept, so each is reported.
        self.pending_changes.push((String::from(what), n));
    }

    fn reset_setup(&mut self) {
        self.orientation = Orientation::default();
        self.keyed.clear();
        self.setup_observations = 0;
        self.set_start = None;
        self.set_start_record = 0;
        self.backsight_since_set = false;
        self.set_before = None;
        self.set_before_records = 0;
        self.set_boundary = 0;
        self.record_starts.clear();
        self.pending_changes.clear();
    }

    // The raw records of the current setup before observation `first`.
    fn records_before(&self, first: usize) -> usize {
        self.record_starts.partition_point(|&start| start < first)
    }

    // What the current setup's observations before `first` say of backsight
    // `to`, against the circle reading `reading` a first 07 gives it.
    fn backsight_before(
        &mut self,
        first: usize,
        to: &str,
        reading: Option<f64>,
    ) -> BacksightReadings {
        let mut found = BacksightReadings::default();
        let applied = self.applied_collimation;
        let Some(station) = self.builder.current_station() else {
            return found;
        };
        for observation in station.observations.iter().take(first) {
            if let Some(target) = target_of(observation) {
                found.observed = found.observed || target == to;
            }
            if let (Some(reading), Some(direction)) = (reading, direction_of(observation))
                && direction.to == to
            {
                if on_same_circle(face_one_reading(direction), reading, applied) {
                    found.on_circle += 1;
                } else {
                    found.off_circle += 1;
                }
            }
        }
        found
    }

    // Where a 07 that begins a new setup takes the current setup's
    // observations from: the end, or the first of the shots of its backsight
    // `to` just before it read on its circle `reading` (any, with none) and not
    // on the setup's (see the top of this file).
    fn new_circle_start(&mut self, to: &str, reading: Option<f64>) -> usize {
        let floor = self.records_before(self.set_boundary);
        let applied = self.applied_collimation;
        let Some(station) = self.builder.current_station() else {
            return 0;
        };
        let end = station.observations.len();
        // Only the setup's own backsight has a reading the old circle gave it:
        // read elsewhere - on the new record's circle, or off the old one where
        // the new record gives none - it shows the circle moved. Another point
        // read on the new circle shows only that the two records' azimuths
        // disagree, and [SETX] 8.2 has the old record orient it.
        if to != self.orientation.backsight {
            return end;
        }
        // The old circle: the setup's 07's reading on the backsight or, with
        // none, the setup's first reading of it. A first reading that is one
        // of the shots below is on it, so none of them moves.
        let mut old = self.orientation.reading;
        let mut record = 0;
        while old.is_none() && record < self.record_starts.len() {
            if let Some(direction) = direction_of_record(station, &self.record_starts, record)
                && direction.to == to
            {
                old = Some(face_one_reading(direction));
            }
            record += 1;
        }
        let Some(old) = old else {
            return end;
        };
        let mut record = self.record_starts.len();
        while record > floor {
            let Some(direction) = direction_of_record(station, &self.record_starts, record - 1)
            else {
                break;
            };
            if direction.to != to {
                break;
            }
            let face_one = face_one_reading(direction);
            if reading.is_some_and(|reading| !on_same_circle(face_one, reading, applied))
                || on_same_circle(face_one, old, applied)
            {
                break;
            }
            record -= 1;
        }
        self.record_starts.get(record).copied().unwrap_or(end)
    }

    // For a setup whose 07s give no circle reading: a warning when its readings
    // of its backsight among its first `kept` observations are more than a
    // minute apart on one face, since nothing else can show its circle moved
    // between its rounds.
    fn warn_of_backsight_spread(&mut self, kept: usize) {
        // Face by face: a face 2 reading less half a circle differs from face
        // 1's by twice the instrument's collimation error where no 04 corrects
        // it, which is not the circle moving. The slack is for a 04 read
        // between two of them, correcting one and not the other.
        let applied = self.applied_collimation;
        let warning = {
            let Some(station) = self.builder.current_station() else {
                return;
            };
            let mut first: [Option<&HorizontalDirectionObservation>; 2] = [None, None];
            let mut warning = None;
            for direction in station
                .observations
                .iter()
                .take(kept)
                .filter_map(direction_of)
            {
                if direction.to != self.orientation.backsight {
                    continue;
                }
                let face = usize::from(direction.pointing.face == Face::Right);
                let earlier = first[face];
                match earlier {
                    None => first[face] = Some(direction),
                    Some(earlier)
                        if !on_same_circle(direction.direction, earlier.direction, applied) =>
                    {
                        warning = Some((
                            direction.source.record_number,
                            format!(
                                "setup '{}' reads its backsight '{}' here more than a minute of \
                                 arc from its reading at record {} on the same face, and its \
                                 backsight records give no circle reading to tell whether the \
                                 circle was moved between them: they are read as one setup, \
                                 oriented on the mean of those readings",
                                station.setup.id,
                                self.orientation.backsight,
                                earlier.source.record_number
                            ),
                        ));
                        break;
                    }
                    Some(_) => {}
                }
            }
            warning
        };
        if let Some((record, message)) = warning {
            self.builder.warn(record, message);
        }
    }

    // Skips a derived view with its reason: `repeated`, a raw observation of
    // its line carries its shot; otherwise its shot is lost, and counted so.
    fn skip_derived_view(&mut self, view: &PendingDerivedView, repeated: bool) {
        if repeated {
            self.builder.skip(
                view.record,
                if view.corrected {
                    format!(
                        "a corrected observation (MC) is the field software's reduction of raw \
                         observations this setup holds of '{}' - oriented, and reduced for the \
                         heights, the prism constant and the weather ([SETX] 29.1); it is not \
                         imported, since it would count them twice",
                        view.to
                    )
                } else {
                    format!(
                        "a reduced observation (azimuth, horizontal and vertical distance) is the \
                         field software's reduction of raw observations this setup holds of \
                         '{}', the scale factor and sea level applied ([SETX] 29.1); it is not \
                         imported, since it would count them twice",
                        view.to
                    )
                },
            );
            return;
        }
        if view.corrected {
            self.lost_corrected += 1;
        } else {
            self.lost_reduced += 1;
        }
        self.builder.skip(
            view.record,
            if view.corrected {
                format!(
                    "a corrected observation (MC) from '{}' to '{}', which no raw observation of \
                     its setup repeats, is not imported: writers of the format put different \
                     numbers in one (Sokkia's field book a mark-to-mark vector already oriented \
                     and corrected, Trimble's writer the inverse of computed coordinates, the \
                     Nikon and Spectra manuals the instrument's slope distance and angles), so \
                     this shot is lost",
                    shown(view.from.as_bytes()),
                    shown(view.to.as_bytes())
                )
            } else {
                format!(
                    "a reduced observation (azimuth, horizontal and vertical distance) from '{}' \
                     to '{}', which no raw observation of its setup repeats, is not imported: it \
                     carries the scale factor and the sea-level reduction the reduction would \
                     apply again, and writers of the format differ in what else, so this shot is \
                     lost",
                    view.from, view.to
                )
            },
        );
    }

    // A 09 MC or an 11 with distances: skipped now when it cannot have a raw
    // twin or already has one, otherwise left for settle_derived_views().
    fn read_derived_view(&mut self, view: PendingDerivedView) {
        // Its raw twin is an observation of the same line from the setup's
        // point, before it or after; from anywhere else nothing can carry it.
        let occupied = self
            .builder
            .current_station()
            .map(|station| station.setup.point_id.clone());
        if occupied.as_deref() != Some(view.from.as_str()) {
            self.skip_derived_view(&view, false);
        } else if self.occupation_targets.contains(&view.to) {
            self.skip_derived_view(&view, true);
        } else {
            self.pending_derived.push(view);
        }
    }

    // The verdict on each derived view a setup left waiting: its line observed
    // raw from the setup's point - counted twice - or its shot lost.
    fn settle_derived_views(&mut self) {
        let pending = std::mem::take(&mut self.pending_derived);
        for view in &pending {
            let repeated = self.occupation_targets.contains(&view.to);
            self.skip_derived_view(view, repeated);
        }
    }

    // What a setup's records have not said about its orientation, said before
    // the next one begins: a keyed azimuth, or none at all. `end`: the setup
    // keeps its observations before it, the rest going to the setup a 07
    // begins, so only those are what it observes; None for all of them.
    fn finish_setup(&mut self, end: Option<usize>) {
        let Some(station) = self.builder.current_station() else {
            return;
        };
        // The targets of its directions, gathered once: looked up for each
        // keyed azimuth, a scan of the observations each time would cost the
        // product of the two.
        let kept = end.map_or(station.observations.len(), |end| {
            end.min(station.observations.len())
        });
        let directed: BTreeSet<String> = station
            .observations
            .iter()
            .take(kept)
            .filter_map(direction_of)
            .map(|direction| direction.to.clone())
            .collect();
        let setup_id = station.setup.id.clone();
        let setup_record = station.source.record_number;
        if self.orientation.set {
            if !directed.contains(&self.orientation.backsight) {
                let message = format!(
                    "setup '{setup_id}' has no observation of its backsight '{}'; {}",
                    self.orientation.backsight,
                    if self.orientation.reading.is_some() {
                        "the reduction orients it by the backsight record's azimuth less its \
                         circle reading on the backsight ([SETX] 29.2.6)"
                    } else {
                        "the backsight record gives no circle reading on it either, so only the \
                         backsight's coordinates can orient it"
                    }
                );
                self.builder.warn(self.orientation.record, message);
            } else if self.orientation.reading.is_none() {
                self.warn_of_backsight_spread(kept);
            }
            return;
        }
        if let Some(front) = self.keyed.first() {
            let chosen = self
                .keyed
                .iter()
                .find(|keyed| directed.contains(&keyed.to))
                .unwrap_or(front)
                .clone();
            let station = self.builder.current_station().expect(CURRENT);
            station.backsight_point_id = chosen.to.clone();
            station.stated_backsight_azimuth = Some(chosen.azimuth);
            station.metadata.insert(
                String::from("oriented by"),
                format!(
                    "the azimuth to '{}' keyed at record {}",
                    chosen.to, chosen.record
                ),
            );
            if !directed.contains(&chosen.to) {
                self.builder.warn(
                    chosen.record,
                    format!(
                        "setup '{setup_id}' has no backsight record, and it does not observe \
                         '{}', the point of the azimuth keyed here; the reduction cannot orient \
                         it",
                        chosen.to
                    ),
                );
            }
            return;
        }
        if !directed.is_empty() {
            // [SETX] 8.2.1: with the backsight skipped, "Horizontal angles
            // stored are treated as azimuths". A circle with no backsight named
            // is, to the model, one set to read azimuths; 0 is its reading on
            // north.
            let station = self.builder.current_station().expect(CURRENT);
            station.backsight_azimuth = Some(0.0);
            station.metadata.insert(
                String::from("oriented by"),
                String::from(
                    "no backsight record: its horizontal readings are azimuths ([SETX] 8.2.1)",
                ),
            );
            self.builder.warn(
                setup_record,
                format!(
                    "setup '{setup_id}' has no backsight record and no keyed azimuth; its \
                     horizontal readings are taken as azimuths, as the field book takes them \
                     when the backsight is skipped ([SETX] 8.2.1)"
                ),
            );
        }
    }

    // A setup on the current one's point, with its instrument height and the
    // settings in force now, taking the current one's observations from
    // `first` on, their pointings numbered anew: they were read on the
    // orientation the record at `n` gives.
    fn split_setup(&mut self, first: usize, n: usize) {
        let (mut moved, point, height, previous, set_metadata) = {
            let station = self.builder.current_station().expect(CURRENT);
            let moved = station.observations.split_off(first);
            let mut set_metadata = String::new();
            if first > 0 && self.set_start == Some(first) {
                let key = format!("set at record {}", self.set_start_record);
                if let Some(value) = station.metadata.remove(&key) {
                    set_metadata = value;
                }
            }
            (
                moved,
                station.setup.point_id.clone(),
                station.setup.instrument_height,
                station.setup.id.clone(),
                set_metadata,
            )
        };
        self.builder
            .begin_station(&point, height, &self.settings, n);
        {
            let station = self.builder.current_station().expect(CURRENT);
            station.metadata.insert(
                String::from("begun by"),
                format!(
                    "the backsight record at record {n}, which does not orient the observations \
                     of setup '{previous}' before it"
                ),
            );
            if !set_metadata.is_empty() {
                station.metadata.insert(
                    format!("set at record {}", self.set_start_record),
                    set_metadata,
                );
            }
        }
        // One record was one pointing; its observations keep sharing one.
        let mut renumbered: BTreeMap<usize, usize> = BTreeMap::new();
        for observation in &mut moved {
            if let Some(pointing) = pointing_of(observation)
                && pointing.index != 0
            {
                pointing.index = *renumbered
                    .entry(pointing.index)
                    .or_insert_with(|| self.builder.next_pointing());
            }
        }
        self.builder
            .current_station()
            .expect(CURRENT)
            .observations
            .extend(moved);
        let set_start = if self.set_start == Some(first) {
            Some(0)
        } else {
            None
        };
        let set_record = self.set_start_record;
        let backsight_since_set = self.backsight_since_set;
        self.reset_setup();
        self.setup_observations = renumbered.len();
        self.set_start = set_start;
        self.set_start_record = set_record;
        self.backsight_since_set = backsight_since_set;
        // Its raw records, as the new setup holds them: one record's
        // observations share a pointing and stand together.
        let station = self.builder.current_station().expect(CURRENT);
        let mut last_pointing = 0;
        for (i, observation) in station.observations.iter().enumerate() {
            if let Some(pointing) = observation_pointing(observation)
                && pointing.index != last_pointing
            {
                last_pointing = pointing.index;
                self.record_starts.push(i);
            }
        }
    }

    fn read_header(&mut self, line: &[u8], n: usize) {
        // White space after the options pads the record: [SDR] chapter 5 lets a
        // writer drop trailing blanks, and one that adds them, or a tab, says
        // nothing more.
        let end = line
            .iter()
            .rposition(|&c| !is_ascii_space(c))
            .map_or(0, |last| last + 1);
        let header = &line[..end];
        let version = version_of(header);
        let sdr33 = version.starts_with("SDR33");
        let sdr2x = !sdr33 && version.starts_with("SDR2");
        let variant = if sdr33 {
            Variant::Sdr33
        } else {
            Variant::Sdr2x
        };
        // [SDR] 3.6: 46 characters, the serial number in 21-24, the date and
        // time in 25-40 and the six options in 41-46. [TA]'s 2022 version
        // writes no serial number, so its header is 42 characters with the
        // options in 37-42. Both end with the options, so they are the last
        // six characters; a header of another length is read that way too,
        // and said to be odd.
        let options_last =
            header.len() >= 42 && header[header.len() - 6..].iter().all(u8::is_ascii_digit);
        if self.header_read {
            // A second header: a second transmission appended, or a stray line.
            if !(sdr33 || sdr2x) || !options_last {
                self.builder.skip(
                    n,
                    format!(
                        "a header record that {} follows the file's own (record {}); the units \
                         and layout that one gives hold",
                        if version.is_empty() {
                            "names no version"
                        } else {
                            "is not one this reader can read"
                        },
                        self.header_record
                    ),
                );
                return;
            }
            // The same layout and units read on; any other would change what
            // every number after it means.
            if header[header.len() - 6..] != *self.header_options.as_slice()
                || variant != self.variant
            {
                let message = format!(
                    "a second header record states other units or another layout than the first \
                     (record {})",
                    self.header_record
                );
                self.refuse(n, message);
                return;
            }
            self.builder.count_read();
            return;
        }
        if !(sdr33 || sdr2x) {
            let message = if version.is_empty() {
                String::from(
                    "the header record names no version, so the layout of the file cannot be told",
                )
            } else {
                format!(
                    "the header names the version '{}'; Katana reads the two layouts the \
                     format's publisher documents, SDR33 and SDR2x ([SDR] 3.2.5: 'SDR33 V04-..' \
                     and 'SDR20 V03-05')",
                    shown(substr(version.as_bytes(), 0, 20))
                )
            };
            self.refuse(n, message);
            return;
        }
        if !options_last {
            self.refuse(
                n,
                String::from(
                    "the header record does not end in its six unit and order options ([SDR] \
                     3.6: 46 characters, the options in 41-46), so the units cannot be told",
                ),
            );
            return;
        }
        let options = header[header.len() - 6..].to_vec();
        let (serial, date) = if header.len() == 46 {
            (
                cleaned(substr(header, 20, 4)),
                cleaned(substr(header, 24, 16)),
            )
        } else {
            let date = cleaned(substr(header, 20, header.len() - 26));
            if header.len() != 42 {
                self.builder.warn(
                    n,
                    format!(
                        "the header record is {} characters where the format's is 46; its last \
                         six are read as the options",
                        header.len()
                    ),
                );
            }
            (String::new(), date)
        };
        self.header_read = true;
        self.header_record = n;
        self.header_options = options.clone();
        self.variant = variant;
        // [NIKON]: "SDR33V04-01", "SDR20V03-05" - the version with no blank.
        self.nikon_form = version.len() > 5 && version.as_bytes()[5] == b'V';
        self.point_id_width = if variant == Variant::Sdr33 { 16 } else { 4 };
        self.real_width = if variant == Variant::Sdr33 { 16 } else { 10 };

        let angular = match options[0] {
            b'1' => {
                self.angle_unit = AngleUnit::Degrees;
                AngularUnit::DecimalDegrees
            }
            b'2' => {
                self.angle_unit = AngleUnit::Gons;
                AngularUnit::Gons
            }
            // [SDR] 3.5
            b'3' => {
                self.angle_unit = AngleUnit::Mils;
                AngularUnit::Mils
            }
            // Not [SDR]'s; two writers define it, and the header's form tells
            // them apart (see the top of this file).
            b'4' => {
                if self.nikon_form {
                    self.angle_unit = AngleUnit::Mils;
                    self.builder.warn(
                        n,
                        String::from(
                            "the header's angle unit option is 4, which the format's publisher \
                             does not define; this header is written as the Nikon and Spectra \
                             manuals write theirs (no blank before the version), and they give 4 \
                             as mils, so the angles are read in mils - Trimble's writer uses 4 \
                             for quadrant bearings in degrees",
                        ),
                    );
                    AngularUnit::Mils
                } else {
                    self.angle_unit = AngleUnit::Degrees;
                    self.builder.warn(
                        n,
                        String::from(
                            "the header's angle unit option is 4, which the format's publisher \
                             does not define; Trimble's writer uses it for quadrant bearings, \
                             which are degrees underneath, so the angles are read in degrees - \
                             the Nikon and Spectra manuals give 4 as mils, but in a header \
                             written with no blank before the version, and this one has one",
                        ),
                    );
                    AngularUnit::DecimalDegrees
                }
            }
            other => {
                self.refuse(
                    n,
                    format!(
                        "the header's angle unit option is '{}'; the format defines 1 degrees, 2 \
                         gons and 3 mils, and Katana will not guess what another means",
                        shown(&[other])
                    ),
                );
                return;
            }
        };
        self.linear_unit = match options[1] {
            b'1' => LinearUnit::Metres,
            // [SDR] 3.3.2: "meters and international feet"
            b'2' => LinearUnit::Feet,
            // [TA]: US survey feet
            b'3' => LinearUnit::UsSurveyFeet,
            other => {
                self.refuse(
                    n,
                    format!(
                        "the header's distance unit option is '{}'; the format defines 1 metres \
                         and 2 feet, and Trimble's writer 3 US survey feet, and Katana will not \
                         guess what another means",
                        shown(&[other])
                    ),
                );
                return;
            }
        };
        self.metres = metres_per(self.linear_unit).expect("a unit the header defines has a ratio");
        match options[2] {
            b'1' => self.pressure_unit = PressureUnit::MillimetresOfMercury,
            b'2' => self.pressure_unit = PressureUnit::InchesOfMercury,
            b'3' => self.pressure_unit = PressureUnit::Millibars,
            other => self.builder.warn(
                n,
                format!(
                    "the header's pressure unit option is '{}', which the format does not \
                     define; no pressure is imported",
                    text(&[other])
                ),
            ),
        }
        match options[3] {
            b'1' => self.temperature_unit = TemperatureUnit::Celsius,
            b'2' => self.temperature_unit = TemperatureUnit::Fahrenheit,
            other => self.builder.warn(
                n,
                format!(
                    "the header's temperature unit option is '{}', which the format does not \
                     define; no temperature is imported",
                    text(&[other])
                ),
            ),
        }
        self.order_option = options[4];
        self.order = match options[4] {
            b'1' => CoordinateOrder::NorthFirst,
            b'2' | b'3' => CoordinateOrder::EastFirst,
            _ => CoordinateOrder::Unknown,
        };
        if options[5] != b'1' {
            // [SDR] 3.6.1 "Always '1'"; [TA] "Always Angles Right". Nothing
            // says what another value would do to a circle reading.
            self.refuse(
                n,
                format!(
                    "the header's angles left/right option is '{}'; only 1, angles right, is \
                     documented, and Katana will not guess which way another turns",
                    text(&options[5..6])
                ),
            );
            return;
        }
        let order = self.order;
        let order_option = self.order_option;
        let project = self.builder.project();
        project.units = DeclaredUnits {
            linear: self.linear_unit,
            angular,
        };
        project
            .metadata
            .insert(String::from("header: version"), version);
        if !serial.is_empty() {
            project
                .metadata
                .insert(String::from("header: serial number"), serial);
        }
        if !date.is_empty() {
            project
                .metadata
                .insert(String::from("header: date and time"), date);
        }
        // How the coordinates of the 02 and 08 records were read (see the top
        // of this file), for anyone checking a known point.
        if order != CoordinateOrder::Unknown {
            project.metadata.insert(
                String::from("header: coordinate order"),
                if order == CoordinateOrder::NorthFirst {
                    String::from("north, east, elevation (option 1)")
                } else {
                    format!("east, north, elevation (option {})", text(&[order_option]))
                },
            );
        }
        self.builder.count_read();
    }

    fn read_job(&mut self, fields: &mut Fields<'_>, n: usize) {
        let name = cleaned(fields.take(16));
        if self.job_read {
            // [SDR] chapter 5: a job ends where another begins. One import is
            // one survey, so the next job's records are read into it.
            self.builder.warn(
                n,
                format!(
                    "a second job ('{name}') begins here; its records are read into the same survey"
                ),
            );
            self.builder
                .project()
                .metadata
                .insert(format!("job at record {n}"), name.clone());
            if self.collimation.record != 0 {
                // [SETX] 13.1: "Collimation is not maintained across all jobs."
                self.builder.warn(
                    n,
                    format!(
                        "the collimation of record {} is not applied in job '{name}': the field \
                         book keeps a collimation within its job",
                        self.collimation.record
                    ),
                );
                self.collimation = Collimation::default();
            }
            self.builder.count_read();
            return;
        }
        self.job_read = true;
        if !name.is_empty() {
            self.builder.project().name = name;
        }
        if self.variant == Variant::Sdr33 {
            // [SDR] 3.6.2: point id type, include elevation, atmospheric
            // correction, C & R correction, refraction constant, sea level
            // correction; 3.5: yes/no fields are 1 No, 2 Yes.
            let flags = fields.take(6);
            let flag = |i: usize| flags.get(i).copied().unwrap_or(b' ');
            let yes_no = |c: u8| -> &'static str {
                if c == b'1' {
                    "off"
                } else if c == b'2' {
                    "on"
                } else {
                    ""
                }
            };
            if flag(1) == b'1' {
                self.record_elevations = false;
                self.builder.project().metadata.insert(
                    String::from("job: elevations"),
                    String::from("not recorded"),
                );
            }
            self.atmospheric_switch = String::from(yes_no(flag(2)));
            let curvature = yes_no(flag(3));
            let sea_level = yes_no(flag(5));
            let metadata = &mut self.builder.project().metadata;
            if !self.atmospheric_switch.is_empty() {
                metadata.insert(
                    String::from("job: atmospheric correction"),
                    self.atmospheric_switch.clone(),
                );
            }
            if !curvature.is_empty() {
                metadata.insert(
                    String::from("job: curvature and refraction correction"),
                    String::from(curvature),
                );
            }
            if !sea_level.is_empty() {
                metadata.insert(
                    String::from("job: sea level correction"),
                    String::from(sea_level),
                );
            }
            // [SETX] 4 makes the constant available "only when C and R crn is
            // set to Yes".
            if curvature == "on" && (flag(4) == b'1' || flag(4) == b'2') {
                self.settings.refraction_coefficient = Some(if flag(4) == b'2' {
                    REFRACTION_OPTION_2
                } else if self.nikon_form {
                    NIKON_REFRACTION_OPTION_1
                } else {
                    REFRACTION_OPTION_1
                });
            }
        }
        self.builder.count_read();
    }

    fn read_correction_note(&mut self, note: &[u8]) {
        // SDR2x keeps the job's corrections in 13CP notes, "Atmos crn: N"
        // ([SDR] chapter 4's sample; [SETX] 28 "Note CP").
        let Some(colon) = find(note, b':', 0) else {
            return;
        };
        let key = trimmed(&note[..colon]);
        let value = trimmed(&note[colon + 1..]);
        let state = if value == b"Y" {
            "on"
        } else if value == b"N" {
            "off"
        } else {
            ""
        };
        if state.is_empty() {
            return;
        }
        if key == b"Atmos crn" {
            self.atmospheric_switch = String::from(state);
            self.builder.project().metadata.insert(
                String::from("job: atmospheric correction"),
                String::from(state),
            );
        } else if key == b"C and R crn" {
            self.builder.project().metadata.insert(
                String::from("job: curvature and refraction correction"),
                String::from(state),
            );
        } else if key == b"Sea level crn" {
            self.builder.project().metadata.insert(
                String::from("job: sea level correction"),
                String::from(state),
            );
        }
    }

    fn read_instrument(&mut self, fields: &mut Fields<'_>, n: usize) {
        // [SDR] 3.6: EDM type, EDM description, EDM serial, theodolite
        // description, theodolite serial, mounting, vertical angle option, EDM
        // offset, reflector offset, prism constant (mm).
        let edm_type = cleaned(fields.take(1));
        let edm = cleaned(fields.take(16));
        let edm_serial = cleaned(fields.take(6));
        let theodolite = cleaned(fields.take(16));
        let theodolite_serial = cleaned(fields.take(6));
        let mounting = cleaned(fields.take(1));
        let vertical_option = cleaned(fields.take(1));
        let edm_offset = self.length(fields.real(), "EDM offset", n);
        let reflector_offset = self.length(fields.real(), "reflector offset", n);
        let prism = self.number(fields.real(), "prism constant", n);

        self.instrument_seen = true;
        if !edm_type.is_empty() {
            // [SETX] 13.1: a collimation applies "until either the instrument
            // type is changed or a new collimation record is added"; the EDM
            // type is the code [SDR] 3.5 lists instruments by. The same type
            // restated (every setup of some files) changes nothing.
            if self.collimation.record != 0
                && !self.edm_type.is_empty()
                && edm_type != self.edm_type
            {
                self.builder.warn(
                    n,
                    format!(
                        "the instrument type changes here (EDM type '{}' to '{}'), so the \
                         collimation of record {} is not applied after it: the field book applies \
                         one until the instrument type changes",
                        shown(self.edm_type.as_bytes()),
                        shown(edm_type.as_bytes()),
                        self.collimation.record
                    ),
                );
                self.collimation_ended_at = n;
                self.ended_collimation = self.collimation.record;
                self.collimation = Collimation::default();
            }
            self.edm_type = edm_type.clone();
        }
        if vertical_option == "1" {
            self.vertical = VerticalReference::Zenith;
        } else if vertical_option == "2" {
            self.vertical = VerticalReference::Horizon;
        } else {
            self.vertical = VerticalReference::Unknown;
            self.builder.warn(
                n,
                format!(
                    "the instrument's vertical angle option is '{vertical_option}'; the format \
                     defines 1 (zenith) and 2 (horizon), so the vertical readings after this \
                     record are not imported"
                ),
            );
        }
        // A serial number of zeros is the field book's "none" ([SDR] 2.4's
        // "EDM S/N 000000" for a manual instrument).
        let stated = |serial: &str| !serial.is_empty() && serial.bytes().any(|c| c != b'0');
        self.settings.model = if theodolite.is_empty() {
            edm.clone()
        } else {
            theodolite
        };
        self.settings.serial_number = if stated(theodolite_serial.as_str()) {
            theodolite_serial
        } else if stated(edm_serial.as_str()) {
            edm_serial
        } else {
            String::new()
        };
        if let Some(prism) = prism {
            // [SDR] 3.3.4: millimetres whatever the distance unit.
            self.settings.prism_constant = Some(prism / 1000.0);
            self.settings.prism_constant_state = CorrectionState::Applied;
        } else {
            self.settings.prism_constant = None;
            self.settings.prism_constant_state = CorrectionState::Unknown;
        }
        let model = self.settings.model.clone();
        let metadata = &mut self.builder.project().metadata;
        if !edm_type.is_empty() {
            metadata.insert(String::from("instrument: EDM type option"), edm_type);
        }
        // [TA] writes the instrument's serial number here; it is kept as the
        // field's text, the format's EDM description.
        if !edm.is_empty() && edm != model {
            metadata.insert(String::from("instrument: EDM"), edm);
        }
        if !mounting.is_empty() {
            metadata.insert(String::from("instrument: mounting option"), mounting);
        }
        // An EDM or reflector off the telescope's axis needs [L5] B.4.4's
        // mounting eccentricity correction, which the model has no place for.
        for (offset, name) in [
            (edm_offset, "EDM offset"),
            (reflector_offset, "reflector offset"),
        ] {
            if let Some(offset) = offset
                && offset != 0.0
            {
                self.builder.warn(
                    n,
                    format!(
                        "the instrument record's {name} is {} m, which is not applied to the \
                         distances",
                        format_exact_real(offset)
                    ),
                );
                self.builder
                    .project()
                    .metadata
                    .insert(format!("instrument: {name} (m)"), format_exact_real(offset));
            }
        }
        self.settings_changed(n, "the instrument");
        self.builder.count_read();
    }

    fn read_atmosphere(&mut self, fields: &mut Fields<'_>, n: usize) {
        let pressure = self.number(fields.real(), "pressure", n);
        let temperature = self.number(fields.real(), "temperature", n);
        if pressure.is_none() && temperature.is_none() {
            self.builder.skip(
                n,
                String::from("atmosphere record with neither a pressure nor a temperature"),
            );
            return;
        }
        self.weather_seen = true;
        if let Some(pressure) = pressure {
            match self.pressure_unit {
                PressureUnit::MillimetresOfMercury => {
                    self.settings.pressure_hectopascals =
                        Some(pressure * HECTOPASCALS_PER_MILLIMETRE_OF_MERCURY);
                }
                PressureUnit::InchesOfMercury => {
                    self.settings.pressure_hectopascals =
                        Some(pressure * HECTOPASCALS_PER_INCH_OF_MERCURY);
                }
                // a millibar is a hectopascal
                PressureUnit::Millibars => self.settings.pressure_hectopascals = Some(pressure),
                PressureUnit::Unknown => {}
            }
        }
        if let Some(temperature) = temperature {
            match self.temperature_unit {
                TemperatureUnit::Celsius => self.settings.temperature_celsius = Some(temperature),
                // NIST SP 811 (2008), appendix B: t/C = (t/F - 32) / 1.8.
                TemperatureUnit::Fahrenheit => {
                    self.settings.temperature_celsius = Some((temperature - 32.0) / 1.8);
                }
                TemperatureUnit::Unknown => {}
            }
        }
        self.settings_changed(n, "the weather");
        self.builder.count_read();
    }

    fn read_scale(&mut self, fields: &mut Fields<'_>, n: usize) {
        let factor = self.number(fields.real(), "scale factor", n);
        let Some(factor) = factor.filter(|&factor| factor > 0.0) else {
            self.builder.skip(
                n,
                String::from("scale factor record without a positive factor"),
            );
            return;
        };
        // [SETX] 28: "plane scale factor", applied between an observation's
        // corrected view and its reduced one (29) - never in a raw distance.
        self.settings.scale_factor = Some(factor);
        self.settings.scale_factor_state = CorrectionState::NotApplied;
        self.settings_changed(n, "the scale factor");
        self.builder.count_read();
    }

    fn read_collimation(&mut self, fields: &mut Fields<'_>, n: usize) {
        let vertical = self.angle(fields.real(), "vertical collimation", n);
        let horizontal = self.angle(fields.real(), "horizontal collimation", n);
        if vertical.is_none() && horizontal.is_none() {
            self.builder
                .skip(n, String::from("collimation record with no value"));
            return;
        }
        // Applied to every raw reading from here to the next 04, another
        // instrument type or another job (see the top of this file); a
        // correction the record leaves null is none.
        self.collimation = Collimation {
            vertical: vertical.unwrap_or(0.0),
            horizontal: horizontal.unwrap_or(0.0),
            record: n,
        };
        let metadata = current_metadata(&mut self.builder);
        if let Some(vertical) = vertical {
            metadata.insert(
                String::from("collimation, vertical (radians)"),
                format_exact_real(vertical),
            );
        }
        if let Some(horizontal) = horizontal {
            metadata.insert(
                String::from("collimation, horizontal (radians)"),
                format_exact_real(horizontal),
            );
        }
        self.builder.count_read();
    }

    fn read_station(&mut self, fields: &mut Fields<'_>, derivation: &[u8], n: usize) {
        let id_field = fields.point_id();
        let first = fields.real();
        let second = fields.real();
        let elevation = fields.real();
        let height = self.length(fields.real(), "instrument height", n);
        let description = cleaned(fields.take(16));
        if self.refused_ids(n, &[id_field]) {
            return;
        }
        let id = point_id_of(id_field);
        if id.is_empty() {
            self.builder
                .skip(n, String::from("station record names no point"));
            return;
        }
        self.finish_setup(None);
        self.settle_derived_views();
        self.occupation_targets.clear();
        self.applied_collimation = 0.0;
        if height.is_none() {
            self.builder.warn(
                n,
                String::from("the setup states no instrument height; 0 is used"),
            );
        }
        self.builder
            .begin_station(&id, height.unwrap_or(0.0), &self.settings, n);
        self.reset_setup();
        self.bad_set = false;
        self.bad_set_remaining = None;
        let how = if derivation == b"KI" {
            CoordinateSource::Entered
        } else {
            CoordinateSource::Unknown
        };
        self.coordinates(&id, first, second, elevation, how, n);
        if self.fatal.is_some() {
            return;
        }
        if !description.is_empty() {
            self.builder
                .code_point(&id, first_word(&description), &description, "", n);
        }
        self.builder.count_read();
    }

    fn read_target(&mut self, fields: &mut Fields<'_>, n: usize) {
        let Some(height) = self.length(fields.real(), "target height", n) else {
            self.builder
                .skip(n, String::from("target height record with no height"));
            return;
        };
        self.target_height = height;
        self.target_height_seen = true;
        self.builder.count_read();
    }

    fn read_backsight(&mut self, fields: &mut Fields<'_>, derivation: &[u8], n: usize) {
        let from_field = fields.point_id();
        let to_field = fields.point_id();
        let azimuth = self.angle(fields.real(), "backsight azimuth", n);
        let reading = self.angle(fields.real(), "backsight circle reading", n);
        if self.refused_ids(n, &[from_field, to_field]) {
            return;
        }
        let from = point_id_of(from_field);
        let to = point_id_of(to_field);
        let Some(occupied) = self
            .builder
            .current_station()
            .map(|station| station.setup.point_id.clone())
        else {
            self.builder.skip(
                n,
                String::from("backsight record with no station record before it"),
            );
            return;
        };
        if from != occupied {
            self.builder.skip(
                n,
                format!("backsight record from '{from}' while the setup is on '{occupied}'"),
            );
            return;
        }
        if to.is_empty() || to == from {
            self.builder.skip(
                n,
                if to.is_empty() {
                    String::from("backsight record names no backsight point")
                } else {
                    format!("backsight record from '{from}' to itself")
                },
            );
            return;
        }
        let wrapped_azimuth = azimuth.map(wrap_to_circle);
        let wrapped_reading = reading.map(wrap_to_circle);
        // An SDR2x 12 read after raw records closed them (V04-02 on) or opened
        // the set after it (V04-01), and the set's 07 SC tells which (see the
        // top of this file): raw observations read since the 12 are its set;
        // none, and the set was the records before it.
        let count = self.current_observation_count();
        if derivation == b"SC"
            && !self.backsight_since_set
            && self.set_before.is_some()
            && self.set_start == Some(count)
        {
            self.set_start = self.set_before;
            let key = format!("set at record {}", self.set_start_record);
            let closing = format!(", closing the {} before it", self.set_before_records);
            self.builder
                .current_station()
                .expect(CURRENT)
                .metadata
                .entry(key)
                .or_default()
                .push_str(&closing);
        }
        self.set_before = None;
        // Set Collection's 07 SC read after a set's observations, with no 07
        // since the set began, closes that set: it is what those observations
        // were oriented on. Any other 07 orients what follows ([SETX] 8.2).
        let closes_set = derivation == b"SC"
            && self.set_start.is_some_and(|start| start < count)
            && !self.backsight_since_set;
        self.backsight_since_set = true;
        // The observations this record orients: those of the set it closes,
        // or what follows it.
        let first = match self.set_start {
            Some(start) if closes_set => start,
            _ => count,
        };
        if self.orientation.set {
            let same_circle = match (wrapped_reading, self.orientation.reading) {
                (Some(reading), Some(earlier)) => on_same_circle(reading, earlier, 0.0),
                _ => wrapped_reading.is_some() == self.orientation.reading.is_some(),
            };
            let another_round = to == self.orientation.backsight
                && wrapped_azimuth == self.orientation.azimuth
                && same_circle;
            if another_round {
                self.orientation.rounds += 1;
                let rounds = self.orientation.rounds;
                let station = self.builder.current_station().expect(CURRENT);
                station
                    .metadata
                    .insert(String::from("backsight rounds"), format!("{rounds}"));
                if let Some(reading) = wrapped_reading {
                    let readings = station
                        .metadata
                        .entry(String::from("backsight circle readings (radians)"))
                        .or_default();
                    if !readings.is_empty() {
                        readings.push_str(", ");
                    }
                    readings.push_str(&format_exact_real(reading));
                }
                self.set_boundary = station.observations.len();
                self.builder.count_read();
                return;
            }
            // The circle was oriented anew. Shots of this record's backsight
            // just before it, on its circle and not the setup's, were read
            // after that: they go with it (see the top of this file).
            let moved = if closes_set {
                first
            } else {
                self.new_circle_start(&to, wrapped_reading)
            };
            if moved > 0 {
                // Those observations are read against another zero, so they
                // are a setup of their own ([SETX] 8.2). The instrument height
                // is the setup's: no 02 has moved it. The settings are those in
                // force now, which an 01 or 05 between the rounds may have
                // changed.
                self.finish_setup(Some(moved));
                self.split_setup(moved, n);
            } else {
                let earlier = self.orientation.record;
                let message = if closes_set {
                    format!(
                        "this backsight record, closing the set of record {}, replaces the one \
                         at record {earlier}, since no observation came between that one and the \
                         set",
                        self.set_start_record
                    )
                } else if count == 0 {
                    format!(
                        "a second backsight record before any observation on the first (record \
                         {earlier}) replaces it"
                    )
                } else {
                    format!(
                        "this backsight record replaces the one at record {earlier}: every \
                         observation since that one reads its backsight '{to}' {}, so the circle \
                         was set anew before them",
                        if wrapped_reading.is_some() {
                            "on this record's circle and not on that one's"
                        } else {
                            "off that one's circle"
                        }
                    )
                };
                self.builder.warn(n, message);
                self.builder
                    .current_station()
                    .expect(CURRENT)
                    .metadata
                    .remove("backsight circle readings (radians)");
            }
        } else if first > 0 {
            // The setup's first 07, read after observations. They are read on
            // the orientation it gives only where they read its backsight on
            // its circle, which says the circle was not set anew at it (see the
            // top of this file); and never when they came before the set it
            // closes.
            let before = self.backsight_before(first, &to, wrapped_reading);
            let on_its_circle = if wrapped_reading.is_some() {
                before.on_circle > 0 && before.off_circle == 0
            } else {
                before.observed
            };
            if closes_set || !on_its_circle {
                // They stay a setup of their own, oriented as finish_setup()
                // finds, and this record begins another.
                let previous = self
                    .builder
                    .current_station()
                    .expect(CURRENT)
                    .setup
                    .id
                    .clone();
                let records = self.records_before(first);
                let set_record = self.set_start_record;
                let why = if closes_set {
                    format!(
                        "the set this backsight record closes (record {set_record}), which it \
                         orients and not them"
                    )
                } else if !before.observed {
                    format!(
                        "its first backsight record, and none is of its backsight '{to}': a \
                         backsight record orients what follows it ([SETX] 8.2)"
                    )
                } else if before.off_circle > 0 {
                    format!(
                        "its first backsight record, and they read its backsight '{to}' more \
                         than a minute of arc from this record's circle reading on it: the circle \
                         was set anew, and a backsight record orients what follows it ([SETX] \
                         8.2)"
                    )
                } else {
                    format!(
                        "its first backsight record, and none gives a horizontal reading of its \
                         backsight '{to}', so nothing shows them read on this record's circle: a \
                         backsight record orients what follows it ([SETX] 8.2)"
                    )
                };
                self.finish_setup(Some(first));
                self.split_setup(first, n);
                let begun = self
                    .builder
                    .current_station()
                    .expect(CURRENT)
                    .setup
                    .id
                    .clone();
                self.builder.warn(
                    n,
                    format!(
                        "setup '{previous}' has {records} observation record(s) before {why}; \
                         they stay setup '{previous}', and setup '{begun}' begins here"
                    ),
                );
            }
        }
        self.orientation = Orientation {
            set: true,
            backsight: to.clone(),
            azimuth: wrapped_azimuth,
            reading: wrapped_reading,
            rounds: 1,
            record: n,
        };
        // a backsight record orients the setup; a keyed azimuth no longer does
        self.keyed.clear();
        self.builder.mention_point(&to, n);
        let station = self.builder.current_station().expect(CURRENT);
        station.backsight_point_id = to;
        // the model's circle
        station.backsight_azimuth = wrapped_reading;
        // and the azimuth, beside it
        station.stated_backsight_azimuth = wrapped_azimuth;
        station.metadata.remove("oriented by");
        station
            .metadata
            .insert(String::from("backsight rounds"), String::from("1"));
        if let Some(reading) = wrapped_reading {
            station.metadata.insert(
                String::from("backsight circle readings (radians)"),
                format_exact_real(reading),
            );
        }
        self.set_boundary = station.observations.len();
        self.builder.count_read();
    }

    fn read_position(&mut self, fields: &mut Fields<'_>, derivation: &[u8], n: usize) {
        let id_field = fields.point_id();
        let first = fields.real();
        let second = fields.real();
        let elevation = fields.real();
        let description = cleaned(fields.take(16));
        if self.refused_ids(n, &[id_field]) {
            return;
        }
        let id = point_id_of(id_field);
        if id.is_empty() {
            self.builder
                .skip(n, String::from("coordinate record names no point"));
            return;
        }
        let how = if derivation == b"KI" {
            CoordinateSource::Entered
        } else if derivation == b"TP" {
            CoordinateSource::FieldObserved
        } else if derivation == b"AJ" || derivation == b"TV" || derivation == b"RS" {
            CoordinateSource::Calculated
        } else {
            CoordinateSource::Unknown
        };
        self.builder.mention_point(&id, n);
        // Neither keyed nor computed, and of the point the observation record
        // before it observes: that observation's POS view, sent beside it
        // ([SETX] 27.2).
        let position_view = how != CoordinateSource::Entered
            && how != CoordinateSource::Calculated
            && self.previous_shot.record != 0
            && id == self.previous_shot.to;
        if !position_view {
            self.coordinates(&id, first, second, elevation, how, n);
        } else if let Some(stated) = self.stated_coordinates(&id, first, second, elevation, n) {
            // The observation stayed in its own view in the field book, which
            // places a point that has no other coordinates and never
            // supersedes those of a position record ([SETX] 6.1, 8.5.2).
            let aside = if self.position_recorded.contains(&id) {
                format!(
                    "they are the POS view of the {} at record {}, which the field book keeps in \
                     {} view: an observation in OBS, MC or RED view does not overwrite a point's \
                     coordinates from a position record ([SETX] 6.1, 8.5.2)",
                    self.previous_shot.what, self.previous_shot.record, self.previous_shot.view
                )
            } else {
                String::new()
            };
            self.builder.position_point(
                &id,
                stated.northing,
                stated.easting,
                stated.elevation,
                how,
                n,
                &aside,
            );
        }
        if self.fatal.is_some() {
            return;
        }
        if !description.is_empty() {
            self.builder
                .code_point(&id, first_word(&description), &description, "", n);
        }
        self.builder.count_read();
    }

    fn read_observation(&mut self, fields: &mut Fields<'_>, derivation: &[u8], n: usize) {
        let from_field = fields.point_id();
        let to_field = fields.point_id();
        let slope_field = fields.real();
        let vertical_field = fields.real();
        let horizontal_field = fields.real();
        let description = cleaned(fields.take(16));
        // An observation record, whatever becomes of it below: an 08 of its
        // target straight after it is its POS view (see the top of this file).
        if derivation == b"MC" {
            self.last_shot = ShotRecord {
                record: n,
                to: point_id_of(to_field),
                what: "corrected observation (MC)",
                view: "MC",
            };
            self.read_derived_view(PendingDerivedView {
                record: n,
                corrected: true,
                from: point_id_of(from_field),
                to: point_id_of(to_field),
            });
            return;
        }
        if derivation == b"F1" || derivation == b"F2" || derivation == b"MD" {
            self.last_shot = ShotRecord {
                record: n,
                to: point_id_of(to_field),
                what: "observation",
                view: "OBS",
            };
        } else {
            self.builder.skip(
                n,
                format!(
                    "an observation with derivation code '{}', which the format does not define \
                     for one",
                    shown(derivation)
                ),
            );
            return;
        }
        if self.refused_ids(n, &[from_field, to_field]) {
            return;
        }
        let from = point_id_of(from_field);
        let to = point_id_of(to_field);
        let Some((setup_id, setup_point, hi)) = self.builder.current_station().map(|station| {
            (
                station.setup.id.clone(),
                station.setup.point_id.clone(),
                station.setup.instrument_height,
            )
        }) else {
            self.builder.skip(
                n,
                String::from("observation with no station record before it"),
            );
            return;
        };
        if from != setup_point {
            self.builder.skip(
                n,
                format!("observation from '{from}' while the setup is on '{setup_point}'"),
            );
            return;
        }
        if to.is_empty() || to == from {
            self.builder.skip(
                n,
                if to.is_empty() {
                    String::from("observation names no target point")
                } else {
                    format!("observation from '{from}' to itself")
                },
            );
            return;
        }
        if self.bad_set {
            self.builder.skip(
                n,
                format!(
                    "observation in {} (record {}), which the field book marks bad and does not \
                     use",
                    self.bad_set_name, self.bad_set_record
                ),
            );
            if let Some(remaining) = self.bad_set_remaining.as_mut() {
                *remaining = remaining.wrapping_sub(1);
                if *remaining == 0 {
                    self.bad_set = false;
                    self.bad_set_remaining = None;
                }
            }
            return;
        }
        let slope = self.length(slope_field, "slope distance", n);
        let vertical = self.angle(vertical_field, "vertical reading", n);
        let horizontal = self.angle(horizontal_field, "horizontal reading", n);

        let mut face = if derivation == b"F1" {
            Face::Left
        } else if derivation == b"F2" {
            Face::Right
        } else {
            Face::Unknown
        };
        let mut has_zenith = false;
        let mut zenith = 0.0;
        if let Some(vertical) = vertical
            && self.vertical != VerticalReference::Unknown
        {
            let mut reading = vertical;
            if self.vertical == VerticalReference::Horizon {
                reading = wrap_to_circle(PI / 2.0 - reading);
            }
            if reading < 0.0 || reading >= TWO_PI {
                self.builder.warn(
                    n,
                    String::from(
                        "vertical reading outside a full circle; the zenith is not imported",
                    ),
                );
            } else {
                let reading_face = face_of_zenith_reading(reading);
                if face == Face::Unknown {
                    face = reading_face;
                } else if reading_face != Face::Unknown && reading_face != face {
                    self.builder.warn(
                        n,
                        format!(
                            "{} says {}, but its vertical reading is on {}; the record's face is \
                             kept",
                            text(derivation),
                            data_model::to_string(face),
                            data_model::to_string(reading_face)
                        ),
                    );
                }
                zenith = zenith_in_model_range(reading);
                has_zenith = true;
            }
        }
        let distance_measured = slope.is_some_and(|slope| slope > 0.0);
        if let Some(slope) = slope
            && !distance_measured
        {
            self.builder.warn(
                n,
                format!(
                    "slope distance of {} read as no distance measured",
                    format_exact_real(slope)
                ),
            );
        }
        if horizontal.is_none() && !has_zenith && !distance_measured {
            // Not a shot: it changes nothing and names no point, so a skipped
            // record leaves no target behind that nothing observed.
            self.builder
                .skip(n, String::from("observation with no readable value"));
            return;
        }

        let changes = std::mem::take(&mut self.pending_changes);
        for (what, record) in &changes {
            // One setup holds one set of settings, and the shots before this
            // record were made under the old ones. A collimation the record
            // ended is not one of them: it is applied reading by reading, so
            // it has ended for the shots after the record (see the top of this
            // file), and the warning must not say it was kept.
            let collimation = if *record == self.collimation_ended_at {
                format!("the collimation of record {}", self.ended_collimation)
            } else {
                String::new()
            };
            let exception = if collimation.is_empty() {
                String::new()
            } else {
                format!(
                    " - all but {collimation}, which ends here all the same: it is applied to \
                     each reading, and the field book applies one only until the instrument type \
                     changes"
                )
            };
            self.builder.warn(
                *record,
                format!(
                    "{what} changes in the middle of setup '{setup_id}'; the setup keeps the \
                     values it began with, and these apply from the next setup{exception}"
                ),
            );
            self.builder
                .current_station()
                .expect(CURRENT)
                .metadata
                .insert(
                    format!("{what} changed at record {record}"),
                    if collimation.is_empty() {
                        String::from("not applied to this setup")
                    } else {
                        format!("not applied to this setup, but {collimation} ends at it")
                    },
                );
        }
        if !self.instrument_seen && !self.instrument_warned {
            self.instrument_warned = true;
            self.builder.warn(
                n,
                String::from(
                    "no instrument record comes before the first observation; vertical readings \
                     are read as zenith angles, the format's first option",
                ),
            );
        }
        if !self.target_height_seen && !self.target_height_warned {
            self.target_height_warned = true;
            self.builder.warn(
                n,
                String::from(
                    "no target height record comes before this observation; 0 is used until one \
                     does",
                ),
            );
            self.builder.not_carried(String::from(
                "no target height for the observations before the first target height record (0 \
                 was used)",
            ));
        }
        self.builder.mention_point(&to, n);
        if !description.is_empty() {
            self.builder
                .code_point(&to, first_word(&description), &description, "", n);
        }

        let mut direction = horizontal.map_or(0.0, wrap_to_circle);
        if self.collimation.record != 0 {
            // [SETX] 29.2.5: face 1 plus, face 2 minus; a reading with no
            // vertical angle is face 1 ([SETX] 29.2.3).
            let sign = if face == Face::Right { -1.0 } else { 1.0 };
            direction = wrap_to_circle(direction + sign * self.collimation.horizontal);
            if has_zenith {
                zenith = cpp::clamp(zenith + sign * self.collimation.vertical, 0.0, PI);
            }
            self.builder
                .current_station()
                .expect(CURRENT)
                .metadata
                .entry(format!("collimation of record {}", self.collimation.record))
                .or_insert_with(|| String::from("applied to its readings ([SETX] 29.2.5)"));
            self.applied_collimation =
                cpp::max(self.applied_collimation, self.collimation.horizontal.abs());
        }

        let precision = self.builder.options().precision;
        let pointing = Pointing {
            index: self.builder.next_pointing(),
            face,
        };
        let observation_count = self.current_observation_count();
        self.record_starts.push(observation_count);
        self.occupation_targets.insert(to.clone());
        if horizontal.is_some() {
            let observation = self
                .builder
                .station_observation::<HorizontalDirectionObservation>(n);
            observation.at = setup_point.clone();
            observation.to = to.clone();
            observation.direction = direction;
            observation.sigma = precision.direction;
            observation.pointing = pointing;
        }
        if has_zenith {
            let observation = self
                .builder
                .station_observation::<ZenithAngleObservation>(n);
            observation.from = setup_point.clone();
            observation.to = to.clone();
            observation.angle = zenith;
            observation.sigma = precision.zenith;
            observation.instrument_height = hi;
            observation.target_height = self.target_height;
            observation.pointing = pointing;
        }
        if let Some(slope) = slope
            && distance_measured
        {
            let observation = self.builder.station_observation::<DistanceObservation>(n);
            observation.from = setup_point;
            observation.to = to;
            observation.distance = slope;
            observation.sigma = distance_sigma(&precision, slope);
            observation.kind = DistanceKind::Slope;
            observation.instrument_height = hi;
            observation.target_height = self.target_height;
            observation.pointing = pointing;
        }
        self.setup_observations += 1;
        self.builder.count_read();
    }

    fn read_reduced(&mut self, fields: &mut Fields<'_>, n: usize) {
        let from_field = fields.point_id();
        let to_field = fields.point_id();
        let azimuth = self.angle(fields.real(), "azimuth", n);
        let horizontal_field = fields.real();
        let vertical_field = fields.real();
        let description = cleaned(fields.take(16));
        if self.refused_ids(n, &[from_field, to_field]) {
            return;
        }
        let from = point_id_of(from_field);
        let to = point_id_of(to_field);
        if from.is_empty() || to.is_empty() {
            self.builder
                .skip(n, String::from("reduced observation record names no point"));
            return;
        }
        if !trimmed(horizontal_field).is_empty() || !trimmed(vertical_field).is_empty() {
            // A derived view, as an MC is, and an observation record whose POS
            // view may follow it (see the top of this file).
            self.last_shot = ShotRecord {
                record: n,
                to: to.clone(),
                what: "reduced observation (RED)",
                view: "RED",
            };
            self.read_derived_view(PendingDerivedView {
                record: n,
                corrected: false,
                from,
                to,
            });
            return;
        }
        let Some(azimuth) = azimuth else {
            self.builder
                .skip(n, String::from("reduced observation record with no value"));
            return;
        };
        // An azimuth and nothing else: an orientation keyed for the line.
        self.builder.mention_point(&from, n);
        self.builder.mention_point(&to, n);
        let here = self
            .builder
            .current_station()
            .is_some_and(|station| station.setup.point_id == from);
        let key = if here {
            format!("azimuth to {to}")
        } else {
            format!("azimuth {from} to {to}")
        };
        let metadata = if here {
            &mut self.builder.current_station().expect(CURRENT).metadata
        } else {
            &mut self.builder.project().metadata
        };
        metadata.insert(
            format!("{key} (radians)"),
            format_exact_real(wrap_to_circle(azimuth)),
        );
        if !description.is_empty() {
            metadata.insert(format!("{key}, description"), description);
        }
        if here && !self.orientation.set {
            // What orients the setup if no 07 does (see finish_setup).
            self.keyed.push(KeyedAzimuth {
                to,
                azimuth: wrap_to_circle(azimuth),
                record: n,
            });
        }
        self.builder.count_read();
    }

    fn read_set(&mut self, fields: &mut Fields<'_>, n: usize) {
        let from_field = fields.point_id();
        let count = cleaned(fields.take(3));
        let mut set_number = String::new();
        let mut bad = String::new();
        if self.variant == Variant::Sdr33 {
            // [SDR] 3.6.2: number of the set, bad marker, return sight, order.
            set_number = cleaned(fields.take(3));
            bad = cleaned(fields.take(1));
        }
        if self.refused_ids(n, &[from_field]) {
            return;
        }
        let from = point_id_of(from_field);
        let counted = digits_value(count.as_bytes());
        let marked_bad = bad == "2";
        self.bad_set_remaining = if marked_bad {
            counted.map(|counted| counted as usize)
        } else {
            None
        };
        // A set of none has nothing to skip, and must not take the next shots.
        self.bad_set = marked_bad && self.bad_set_remaining != Some(0);
        self.bad_set_record = n;
        self.bad_set_name = format!(
            "set {} at '{from}'",
            if set_number.is_empty() {
                "?"
            } else {
                set_number.as_str()
            }
        );
        if marked_bad {
            let stated = self.bad_set_remaining.is_some();
            self.builder.warn(
                n,
                format!(
                    "{} is marked bad; its {}observation(s) are not imported{}",
                    self.bad_set_name,
                    if stated {
                        format!("{count} ")
                    } else {
                        String::new()
                    },
                    if stated {
                        ""
                    } else {
                        ", up to the next set or setup, since it states no count of them"
                    }
                ),
            );
        }
        let since = self.record_starts.len() - self.records_before(self.set_boundary);
        if let Some(station) = self.builder.current_station() {
            let here = station.observations.len();
            // The set begins here, as a 12 opens it in SDR33 and SDR2x's V04-01
            // - unless, in the SDR2x layout, this 12 follows raw records of its
            // setup and its set's 07 SC finds no raw observation after it: then
            // it closed the set they make (see the top of this file), the last
            // of them as many as its count, where it states one they hold.
            let mut closable = 0;
            if self.variant == Variant::Sdr2x && since > 0 {
                closable = match counted {
                    Some(counted) if counted > 0 && (counted as usize) < since => counted as usize,
                    _ => since,
                };
            }
            station.metadata.insert(
                format!("set at record {n}"),
                format!(
                    "{}{count} observation(s){}",
                    if set_number.is_empty() {
                        String::new()
                    } else {
                        format!("number {set_number}, ")
                    },
                    if marked_bad { ", marked bad" } else { "" }
                ),
            );
            self.set_start = Some(here);
            self.set_before = if closable > 0 {
                Some(self.record_starts[self.record_starts.len() - closable])
            } else {
                None
            };
            self.set_before_records = closable;
            self.set_start_record = n;
            self.backsight_since_set = false;
            self.set_boundary = here;
        }
        self.builder.count_read();
    }

    fn read_distance_unit_note(&mut self, note: &[u8], n: usize) {
        // [SDR] 3.3.2: "13DU1:Meters:", "13DU2:Feet:", "13DU3:US Feet:"; "The
        // numeric code directly after DU specifies which distance unit ... The
        // text is informational only." It is sent to say US feet where the
        // header can say only feet.
        let code = note.first().copied().unwrap_or(b' ');
        let unit = match code {
            b'1' => LinearUnit::Metres,
            b'2' => LinearUnit::Feet,
            b'3' => LinearUnit::UsSurveyFeet,
            _ => {
                // It says nothing the header has not: the header's unit holds.
                self.builder.skip(
                    n,
                    format!(
                        "the distance unit note gives {}, and the format defines 1 metres, 2 feet \
                         and 3 US feet; the header's unit, {}, holds",
                        if note.is_empty() {
                            String::from("no code")
                        } else {
                            format!("the code '{}'", shown(&[code]))
                        },
                        data_model::to_string(self.linear_unit)
                    ),
                );
                return;
            }
        };
        // [SDR] 3.3.2: "All distances specified after this Note record are
        // displayed in the specified distance units" - the note governs.
        let agrees = unit == self.linear_unit
            || (unit == LinearUnit::UsSurveyFeet && self.linear_unit == LinearUnit::Feet);
        if !agrees {
            self.builder.warn(
                n,
                format!(
                    "the distance unit note says {} where the header (record {}) says {}; the \
                     format has the note govern the distances after it, so they are read in {}",
                    data_model::to_string(unit),
                    self.header_record,
                    data_model::to_string(self.linear_unit),
                    data_model::to_string(unit)
                ),
            );
        }
        self.linear_unit = unit;
        self.metres = metres_per(unit).expect("a unit the note defines has a ratio");
        self.builder.project().units.linear = unit;
        self.builder.count_read();
    }

    fn read_note(&mut self, fields: &mut Fields<'_>, derivation: &[u8], n: usize) {
        let note = cleaned_bytes(fields.take(60));
        if derivation == b"DU" {
            self.read_distance_unit_note(&note, n);
            return;
        }
        if note.is_empty() {
            self.builder.count_read();
            return;
        }
        if derivation == b"CP" {
            self.read_correction_note(&note);
        }
        // [LISCAD]: the field book's "Set #" note begins a set, which in the
        // SDR2x layout its 12 closes afterwards.
        if derivation == b"SC"
            && note.starts_with(b"Set #")
            && let Some(station) = self.builder.current_station()
        {
            self.set_boundary = station.observations.len();
        }
        if derivation == b"TS" {
            if let Some(time) = time_stamp_of(&note) {
                if let Some(station) = self.builder.current_station()
                    && !station.instrument.time.known()
                {
                    station.instrument.time = time;
                }
                // [TA]'s form, which points to distances without the
                // atmospheric correction (see read()).
                self.writer_time_stamps =
                    self.writer_time_stamps || find_text(&note, b"Date ").is_some();
            } else {
                self.builder.warn(
                    n,
                    format!(
                        "time stamp '{}' is not a date and time in a form the format's writers \
                         use; it is kept as a note",
                        text(substr(&note, 0, 60))
                    ),
                );
            }
        }
        let kept = if derivation == b"NM" {
            text(&note)
        } else {
            format!("{}: {}", text(derivation), text(&note))
        };
        self.builder.add_station_note(&kept);
        self.builder.count_read();
    }

    fn dispatch(&mut self, line: &[u8], n: usize) {
        // The observation record before this one, if it was one, whatever this
        // one is: only a record straight after it can be its POS view.
        self.previous_shot = std::mem::take(&mut self.last_shot);
        if !self.header_read {
            // [SDR] chapter 3: "The first data record of each transmission is
            // always of type 00."
            if !line.starts_with(b"00") || !is_record(line) {
                self.refuse(
                    n,
                    String::from("the file does not begin with an SDR header record (type 00)"),
                );
                return;
            }
            self.read_header(line, n);
            return;
        }
        let (live, marks) = without_deletion_marks(line);
        if marks > 0 {
            // The mark as written, where it is short enough to read; a line of
            // D's is quoted by its length, as other warnings cut what they
            // quote.
            const QUOTED_MARKS: usize = 8;
            self.builder.skip(
                n,
                format!(
                    "a deleted record (marked {}){} is not imported",
                    if marks <= QUOTED_MARKS {
                        format!("'{}'", "D".repeat(marks))
                    } else {
                        format!("with a run of {marks} D's")
                    },
                    if is_record(live) {
                        format!(", {},", record_label(live))
                    } else {
                        String::new()
                    }
                ),
            );
            return;
        }
        if !is_record(line) {
            self.builder.skip(
                n,
                format!(
                    "'{}' is not a record: a record opens with a two-digit type and a derivation \
                     code",
                    shown(trimmed(substr(line, 0, 12)))
                ),
            );
            return;
        }
        if !self.non_ascii && line.iter().any(|&c| c >= 0x80) {
            self.non_ascii = true;
            self.builder.warn(
                n,
                String::from(
                    "the record holds characters outside the ASCII the format allows; its fields \
                     are read by character position",
                ),
            );
        }
        let kind = &line[..2];
        let derivation = &line[2..4];
        let mut fields = Fields::new(line, self.point_id_width, self.real_width);
        if kind == b"00" {
            self.read_header(line, n);
        } else if kind == b"01" {
            self.read_instrument(&mut fields, n);
        } else if kind == b"02" {
            self.read_station(&mut fields, derivation, n);
        } else if kind == b"03" {
            self.read_target(&mut fields, n);
        } else if kind == b"04" {
            self.read_collimation(&mut fields, n);
        } else if kind == b"05" {
            self.read_atmosphere(&mut fields, n);
        } else if kind == b"06" {
            self.read_scale(&mut fields, n);
        } else if kind == b"07" {
            self.read_backsight(&mut fields, derivation, n);
        } else if kind == b"08" {
            self.read_position(&mut fields, derivation, n);
        } else if kind == b"09" {
            self.read_observation(&mut fields, derivation, n);
        } else if kind == b"10" {
            self.read_job(&mut fields, n);
        } else if kind == b"11" {
            self.read_reduced(&mut fields, n);
        } else if kind == b"12" {
            self.read_set(&mut fields, n);
        } else if kind == b"13" {
            self.read_note(&mut fields, derivation, n);
        } else {
            self.builder.skip(
                n,
                format!(
                    "record {} is of a type this reader does not import",
                    record_label(line)
                ),
            );
        }
    }

    // `ends_with_line_end`: whether the text's last character ends a line, so
    // its last record was not cut short in the middle.
    fn read(
        mut self,
        lines: &[&[u8]],
        guessed_encoding: bool,
        encoding: TextEncoding,
        ends_with_line_end: bool,
    ) -> Result<ReadResult> {
        if guessed_encoding {
            self.builder.warn(
                0,
                format!(
                    "the file is not UTF-8; it was read as {}, so an accented name may be wrong",
                    text_encoding::to_string(encoding)
                ),
            );
        }
        // Record numbers are line numbers: split_lines keeps a blank line in
        // the middle, and takes CR LF ([SDR] chapter 3), LF or a lone CR alike.
        // The line of the last record dispatched.
        let mut last_record = 0;
        for (i, &line) in lines.iter().enumerate() {
            if self.fatal.is_some() {
                break;
            }
            let n = i + 1;
            if trimmed(line).is_empty() {
                continue;
            }
            // [SDR] chapter 3: a transmission may open with STX and close with
            // ETX and a checksum. Neither is a record; the checksum is kept as
            // written and not checked.
            let line = without_stx(line);
            if trimmed(line).is_empty() {
                continue;
            }
            if line[0] == 0x03 {
                self.builder
                    .project()
                    .metadata
                    .insert(String::from("transmission checksum"), cleaned(&line[1..]));
                continue;
            }
            if is_end_of_file_marks(line) {
                continue;
            }
            self.dispatch(line, n);
            last_record = n;
        }
        if let Some(fatal) = self.fatal.take() {
            return Err(fatal);
        }
        if !self.header_read {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "{} holds no SDR header record, so its units and its layout are unknown",
                    self.builder.file_name()
                ),
                "Sokkia SDR",
            ));
        }
        // A last record with no line end, where [SDR] chapter 3 ends every one
        // with CR LF: a transfer cut inside it leaves a shorter number that is
        // still a number, and only this says so.
        if !ends_with_line_end && last_record != 0 && last_record == lines.len() {
            self.builder.warn(
                last_record,
                String::from(
                    "the file ends inside this record, with no line end after it (the format ends \
                     every record with CR LF), so it may have been cut short: the record's last \
                     field may be incomplete",
                ),
            );
        }
        self.finish_setup(None);
        self.settle_derived_views();
        if self.lost_corrected > 0 {
            self.builder.not_carried(format!(
                "{} corrected (MC) observation(s) that no raw observation of their setup \
                 repeats: writers of the format disagree about what an MC holds, so they are not \
                 imported and their shots are lost",
                self.lost_corrected
            ));
        }
        if self.lost_reduced > 0 {
            self.builder.not_carried(format!(
                "{} reduced (RED) observation(s) with distances that no raw observation of their \
                 setup repeats: they are not imported and their shots are lost",
                self.lost_reduced
            ));
        }
        if !self.builder.project().stations.is_empty() {
            let switch = if self.atmospheric_switch.is_empty() {
                String::new()
            } else {
                format!("; this job's switch is {}", self.atmospheric_switch)
            };
            let writer = if self.writer_time_stamps {
                "; this file's time stamps are written as Trimble's writer writes them (\"Time \
                 Date MM/DD/YYYY\"), so if it wrote the file its distances are without the \
                 correction and Recompute applies it"
            } else {
                ""
            };
            self.builder.not_carried(format!(
                "whether the distances carry the atmospheric correction: Sokkia's field book \
                 applies it as each distance is accepted when the job's switch is on, and \
                 Trimble's published writer turns the switch on whenever it writes weather and \
                 leaves the correction out{switch}{writer}; the reduction applies none unless \
                 told to recompute it"
            ));
            if !self.weather_seen {
                self.builder.not_carried(String::from(
                    "no pressure and temperature to compute an atmospheric correction from",
                ));
            }
            if !self.instrument_seen {
                self.builder.not_carried(String::from(
                    "no instrument record: the prism constant, and whether the distances carry \
                     one, are unknown",
                ));
            }
        }
        let result = self.builder.finish();
        let project = &result.project;
        if project.stations.is_empty()
            && project.points.is_empty()
            && project.unpositioned_points.is_empty()
        {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                format!(
                    "{} holds no setup, observation or point Katana could read",
                    project.source.file_name
                ),
                format!(
                    "{} warning(s){}",
                    result.warnings.len(),
                    match result.warnings.first() {
                        Some(warning) => format!(", the first: {}", describe(warning)),
                        None => String::new(),
                    }
                ),
            ));
        }
        Ok(result)
    }
}

// ---- Registration -----------------------------------------------------------------

// "00" and a derivation code, then "SDR2" or "SDR33" where the version field
// begins ([SDR] 3.2.5; [NIKON] writes "SDR33V04-01" with no blank).
fn is_header(line: &[u8]) -> bool {
    line.len() >= 9
        && line[0] == b'0'
        && line[1] == b'0'
        && is_code_char(line[2])
        && is_code_char(line[3])
        && line[4..].starts_with(b"SDR")
        && (line[7] == b'2' || &line[7..9] == b"33")
}

fn probe(input: &ProbeInput<'_>) -> FormatSignature {
    let bytes = without_byte_order_mark(input.bytes);
    if !looks_like_text(bytes) {
        return ruled_out();
    }
    let mut lines = 0usize;
    let mut shaped = 0usize;
    let mut first: &[u8] = &[];
    for line in probe_lines(input, PROBED_LINES) {
        // The framing the reader passes over: STX, ETX and its checksum, DOS
        // end-of-file marks.
        let line = without_stx(line);
        if trimmed(line).is_empty() || line[0] == 0x03 || is_end_of_file_marks(line) {
            continue;
        }
        if lines == 0 {
            first = line;
        }
        lines += 1;
        if is_record(without_deletion_marks(line).0) {
            shaped += 1;
        }
    }
    if lines == 0 {
        return ruled_out();
    }
    let sdr = input.extension == "sdr";
    let header = is_header(first);
    // A judgement, not a measurement: an SDR file is records and nothing else,
    // and a tenth leaves room for a damaged stretch or a converter's unmarked
    // notes.
    let mostly_records = shaped * 10 >= lines * 9;
    let counted = format!("{shaped} of {lines} lines open with a record type and derivation code");
    if header {
        let version = version_of(first);
        if mostly_records {
            return signature(
                if sdr { 0.98 } else { 0.95 },
                format!(
                    "{}header {version}, {counted}",
                    if sdr { "extension .sdr, " } else { "" }
                ),
            );
        }
        // No other format writes this header (the Survey Controller probe
        // steps aside for it), so it identifies the file even with odd lines
        // in it, which the reader skips, each with a warning - but less surely
        // than a clean one.
        return signature(0.8, format!("header {version} but only {counted}"));
    }
    if sdr && mostly_records {
        return signature(
            0.4,
            format!("extension .sdr and {counted}, but no SDR header record"),
        );
    }
    if sdr {
        return signature(0.1, String::from("extension .sdr only"));
    }
    ruled_out()
}

fn descriptor() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(FORMAT_ID),
        human_name: String::from("Sokkia SDR33 / SDR2x"),
        manufacturer: Manufacturer::Sokkia,
        reads: FormatContent {
            points: true,
            observations: true,
            stations: true,
            features: true,
            coordinate_system: false,
            instrument_settings: true,
            gnss: false,
        },
        can_import: true,
        can_export: false,
        parser_version: String::from("1.0"),
        extensions: vec![String::from("sdr")],
    }
}

fn read_sdr(bytes: &[u8], file_name: &str, options: &ReadOptions) -> Result<ReadResult> {
    let decoded = decode_text(bytes)?;
    let text_bytes = decoded.text.as_bytes();
    let lines = split_lines(text_bytes);
    // The header names the variant, which every record's provenance carries.
    let mut variant = "SDR";
    let mut version = String::new();
    for &line in &lines {
        let line = without_stx(line);
        if trimmed(line).is_empty() || is_end_of_file_marks(line) {
            continue;
        }
        if is_header(line) {
            version = version_of(line);
            variant = if version.starts_with("SDR33") {
                "SDR33"
            } else {
                "SDR2x"
            };
        }
        break;
    }
    let reader = SdrReader::new(
        file_name,
        SourceRecord {
            manufacturer: String::from("Sokkia"),
            format: String::from(variant),
            format_version: version,
            file_name: String::from(file_name),
            record_number: 0,
        },
        options,
    );
    let ends_with_line_end = text_bytes.last().is_some_and(|&c| c == b'\n' || c == b'\r');
    reader.read(
        &lines,
        decoded.guessed,
        decoded.encoding,
        ends_with_line_end,
    )
}

pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(descriptor(), probe, Some(read_sdr))
}
