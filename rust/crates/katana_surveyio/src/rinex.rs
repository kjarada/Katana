//! RINEX observation files (versions 2.10/2.11, 3.0x and 4.0x): what an import
//! of one produces, and the reader, its probe and the format's registration.
//! The reader is called through read_survey().
//!
//! A RINEX observation file is imported for what it says about an OCCUPATION,
//! not for its raw measurements: one GnssSession per occupation (normally one
//! per file; a stop-and-go file with "new site occupation" events has one per
//! site) with the receiver, the antenna and its height (vertical, to the
//! antenna reference point, as RINEX defines it), the first and last epoch, the
//! interval, the epoch count and the satellites seen per system; the marker as
//! an UnpositionedPoint named by MARKER NAME; and the header's APPROX POSITION
//! XYZ as a GnssGlobalPositionObservation of that marker, geocentric, with
//! GnssSolution::Autonomous and RINEX_APPROXIMATE_POSITION_SIGMA on each axis
//! stated in its covariance (X/Y/Z, as GnssCovariance3 defines it for a
//! geocentric value). It is an approximate position - most often the
//! receiver's own navigation solution - so a reduction must weight it by that
//! covariance, never by its a-priori sigma for a survey-grade GNSS position: at
//! 10 m it can place the marker on a drawing but cannot pull on a survey-grade
//! observation in an adjustment. The pseudorange, phase, Doppler and
//! signal-strength values are checked and counted, never stored: processing raw
//! GNSS is outside what Katana does.
//!
//! Hatanaka-compressed files (Compact RINEX 1.0 and 3.0, .YYd / .crx) are
//! expanded as they are read and then read like any other; warnings name the
//! compact file's own lines. gzip / Unix-compress / bzip2 / zip packed files are
//! recognised and refused with the step that unpacks them.

// Specifications followed, column for column:
//   * "RINEX: The Receiver Independent Exchange Format, Version 2.11"
//     (W. Gurtner, Astronomical Institute, University of Bern, and L. Estey,
//     UNAVCO; 10 December 2007): tables A1 (observation header) and A2
//     (observation data). 2.10 differs only in details this reader does not
//     depend on.
//   * "RINEX: The Receiver Independent Exchange Format, Version 3.05"
//     (IGS/RTCM RINEX WG, I. Romero ed., 1 December 2020): tables A2
//     (observation header) and A3 (observation data). 3.00 to 3.04 lay out the
//     records this reader uses identically.
//   * "RINEX: The Receiver Independent Exchange Format, Version 4.00"
//     (IGS/RTCM RINEX WG, I. Romero ed., 1 December 2021): observation files
//     are 3.05 plus the optional DOI, LICENSE OF USE and STATION INFORMATION
//     header records; the data records are unchanged.
//
// How the file is read, and why:
//   * One pass over the bytes, a line at a time, fields cut at the specified
//     columns. Epochs are COUNTED, not stored, so a day of 1 Hz multi-system
//     data costs its reading time and a few kilobytes, not a copy of itself.
//   * Every satellite record is still checked - its satellite, its length
//     against the observation types the header declares, and that it holds
//     only what an F14.3/I1/I1 field can - so a damaged file is reported record
//     by record instead of being counted as though it were whole.
//   * Records are found by their structure, never by trusting a count alone:
//     an epoch that promises more satellites than follow is reported and the
//     next epoch is read where it actually starts. RINEX 3 and 4 mark every
//     epoch with '>'; a RINEX 2 epoch line has a decimal point in column 19
//     and blanks in columns 27-28, which no observation line can have (its
//     decimal points are in columns 11, 27, 43, ...).
//   * Event flags (table A2 / A3): 1 (power failure) is an epoch and a
//     warning; 2 (antenna starts moving) ends the occupation, and the epochs
//     until 3 are counted as kinematic; 3 (new site occupation) starts a new
//     session from the MARKER NAME and other header records that follow; 4
//     (header records follow) updates the header, starting a new session when
//     it changes the marker, antenna or position under an occupation that
//     already has epochs; 5 (external event) is counted; 6 (cycle slip
//     records) is read through and counted.
//   * A Hatanaka-compressed file (Compact RINEX) is first expanded to the
//     RINEX it stands for (rinex_compact.rs) and then read exactly as above;
//     its warnings and source records are mapped back to the compact file's
//     own lines.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use katana_core::text::lowered;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_survey::data_model::to_string as survey_text;
use katana_survey::{
    AntennaHeightMethod, GeocentricCoordinate, GnssCovariance3, GnssGlobalPositionObservation,
    GnssSession, LinearUnit, Observation, SourceRecord, SurveyTimestamp, UnpositionedPoint,
};

use crate::format::{
    FormatContent, FormatDescriptor, FormatRegistration, FormatSignature, Manufacturer, Named,
    ProbeInput, ReadOptions, ReadResult, ReadWarning, ruled_out,
};
use crate::reader::MAX_SURVEY_FILE_BYTES;
use crate::rinex_internal::{
    CompactExpansion, LineCursor, MAX_LISTED_WARNINGS, Packing, VersionRecord, clean_text, columns,
    contains, expand_compact_rinex, header_content, header_label, integer_field, is_digit,
    navigation_file_candidates, packing_of, packing_refusal, raw_text, real_field,
    summarise_navigation, system_name, tokens, version_record,
};

/// Stable: saved in survey jobs and source records.
pub const RINEX_OBSERVATION_FORMAT_ID: &str = "rinex-observation";

/// Metres, per ECEF axis, for a header's approximate position. A receiver's
/// autonomous code solution is good to a few metres, and a header position may
/// equally have been typed in roughly by whoever set the receiver up, so ten
/// metres is the honest order of magnitude - and large enough that a
/// millimetre-level observation always wins against it.
pub const RINEX_APPROXIMATE_POSITION_SIGMA: f64 = 10.0;

const HUMAN_NAME: &str = "RINEX observation";
const PARSER_VERSION: &str = "1.0";

// Satellite numbers are two digits (A1,I2 / A1,I2.2).
const SATELLITE_NUMBERS: i32 = 100;

// A header position further than this from the earth's centre, or nearer, is
// not a position on or above the ground: it is zero, in the wrong unit, or
// damaged. The earth's radius runs from 6 356.8 km (poles) to 6 378.1 km
// (equator); the band leaves room for the Dead Sea and for aircraft.
const NEAREST_PLAUSIBLE_RADIUS: f64 = 6_300_000.0;
const FURTHEST_PLAUSIBLE_RADIUS: f64 = 6_500_000.0;

// Two epochs this close in time are the same epoch written twice.
const SAME_EPOCH_SECONDS: f64 = 1e-4;

// ---- Characters an observation field may hold ---------------------------------

// m(F14.3,I1,I1): digits, the sign, the decimal point and blanks. Anything
// else - letters, '*' from a Fortran overflow, a stray control byte - means
// the record is not what the header says it is.
fn only_observation_characters(text: &[u8]) -> bool {
    text.iter()
        .all(|&c| is_digit(c) || c == b' ' || c == b'.' || c == b'-')
}

fn is_blank(text: &[u8]) -> bool {
    text.iter().all(|&c| c == b' ')
}

// std::to_chars(fixed, decimals) into a buffer of 64 characters, "?" when the
// text does not fit (to_chars then fails).
fn format_number(value: f64, decimals: usize) -> String {
    let text = if value.is_nan() {
        String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        })
    } else if value.is_infinite() {
        String::from(if value < 0.0 { "-inf" } else { "inf" })
    } else {
        // Rust's fixed digits are std::to_chars', the sign of a negative zero
        // included.
        format!("{value:.decimals$}")
    };
    if text.len() <= 64 {
        text
    } else {
        String::from("?")
    }
}

// ---- Time -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct EpochTime {
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: f64,
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: i32) -> i32 {
    const DAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if month == 2 && is_leap_year(year) {
        29
    } else {
        DAYS[(month - 1) as usize]
    }
}

// Days from 1970-01-01 in the proleptic Gregorian calendar (H. Hinnant's
// days_from_civil): only differences are used, to find the interval.
fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let year = year - i32::from(month <= 2);
    let shifted = if year >= 0 { year } else { year - 399 };
    let era = i64::from(shifted / 400);
    let year_of_era = i64::from(year) - era * 400;
    let month_shift = if month > 2 { -3 } else { 9 };
    let day_of_year = i64::from((153 * (month + month_shift) + 2) / 5 + day - 1);
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

// Empty when the fields are a time; otherwise what is wrong with them.
fn time_problem(time: &EpochTime) -> String {
    // GPS time starts in 1980 and RINEX in 1989; a year outside this span is a
    // damaged field, not a survey.
    if !(1980..=2200).contains(&time.year) {
        return format!("year {} is not a plausible observation year", time.year);
    }
    if !(1..=12).contains(&time.month) {
        return format!("month {} does not exist", time.month);
    }
    if !(1..=days_in_month(time.year, time.month)).contains(&time.day) {
        return format!("day {} does not exist in that month", time.day);
    }
    if !(0..=23).contains(&time.hour) || !(0..=59).contains(&time.minute) {
        return String::from("the hour or minute is out of range");
    }
    // 60.x is a leap second; RINEX times may carry one.
    if !(0.0..61.0).contains(&time.second) {
        return String::from("the seconds are out of range");
    }
    String::new()
}

fn seconds_of(time: &EpochTime) -> f64 {
    days_from_civil(time.year, time.month, time.day) as f64 * 86400.0
        + f64::from(time.hour) * 3600.0
        + f64::from(time.minute) * 60.0
        + time.second
}

fn timestamp_of(time: &EpochTime, time_system: &str) -> SurveyTimestamp {
    SurveyTimestamp {
        year: time.year,
        month: time.month,
        day: time.day,
        hour: time.hour,
        minute: time.minute,
        second: time.second,
        time_system: String::from(time_system),
    }
}

// ---- Epoch records ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct Epoch {
    /// The date and time fields were filled.
    timed: bool,
    time: EpochTime,
    flag: i32,
    /// Satellites, or special records for flags 2-5.
    count: i32,
}

// Why an epoch line could not be used, or empty when it could.
#[derive(Clone, Debug, Default)]
struct EpochParse {
    /// The line is shaped as an epoch record.
    is_epoch: bool,
    /// Its satellite / special-record count was read.
    count_read: bool,
    /// Shaped as one but unusable: bad time, bad count.
    problem: String,
}

fn read_int(field: &[u8]) -> Option<i32> {
    let read = integer_field(field)?;
    if !(-1_000_000..=1_000_000).contains(&read) {
        return None;
    }
    Some(read as i32)
}

// The time fields shared by both layouts, once cut out. Each field is set as it
// is read, as the C++ sets them.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ readTime takes the six fields, the year's form and the time."
)]
fn read_time(
    year: &[u8],
    month: &[u8],
    day: &[u8],
    hour: &[u8],
    minute: &[u8],
    second: &[u8],
    two_digit_year: bool,
    time: &mut EpochTime,
) -> String {
    let fields = [
        (year, &mut time.year),
        (month, &mut time.month),
        (day, &mut time.day),
        (hour, &mut time.hour),
        (minute, &mut time.minute),
    ];
    for (field, into) in fields {
        match read_int(field) {
            Some(value) => *into = value,
            None => return String::from("its date or time is not a number"),
        }
    }
    let Some(seconds) = real_field(second) else {
        return String::from("its seconds are not a number");
    };
    time.second = seconds;
    if two_digit_year {
        // RINEX 2.11: two-digit years 80-99 are 1980-1999, 00-79 are 2000-2079.
        if !(0..=99).contains(&time.year) {
            return String::from("its two-digit year is not two digits");
        }
        time.year += if time.year >= 80 { 1900 } else { 2000 };
    }
    time_problem(time)
}

// The count of an epoch: blank is 0, else a number, which the epoch keeps even
// when it is negative (as the C++'s readInt leaves it), with the problem.
fn read_count(field: &[u8], epoch: &mut Epoch, problem: &str) -> Option<String> {
    if is_blank(field) {
        epoch.count = 0;
        return None;
    }
    match read_int(field) {
        Some(count) => {
            epoch.count = count;
            (count < 0).then(|| String::from(problem))
        }
        None => Some(String::from(problem)),
    }
}

// RINEX 2 (table A2): 1X,I2.2,4(1X,I2),F11.7,2X,I1,I3,12(A1,I2),F12.9.
fn parse_epoch_v2(line: &[u8], epoch: &mut Epoch) -> EpochParse {
    let mut parse = EpochParse::default();
    if line.len() < 29 || !is_digit(line[28]) || line[26] != b' ' || line[27] != b' ' {
        return parse;
    }
    let time_fields = &line[..26];
    epoch.flag = i32::from(line[28] - b'0');
    epoch.timed = !is_blank(time_fields);
    if epoch.timed
        && (line[0] != b' '
            || line[3] != b' '
            || line[6] != b' '
            || line[9] != b' '
            || line[12] != b' '
            || line[18] != b'.')
    {
        return parse;
    }
    parse.is_epoch = true;
    if let Some(problem) = read_count(
        columns(line, 30, 3),
        epoch,
        "its satellite count is not a number",
    ) {
        parse.problem = problem;
        return parse;
    }
    parse.count_read = true;
    if epoch.timed {
        parse.problem = read_time(
            columns(line, 2, 2),
            columns(line, 5, 2),
            columns(line, 8, 2),
            columns(line, 11, 2),
            columns(line, 14, 2),
            columns(line, 16, 11),
            true,
            &mut epoch.time,
        );
    }
    parse
}

// RINEX 3 and 4 (table A3): A1,1X,I4,4(1X,I2.2),F11.7,2X,I1,I3,6X,F15.12.
fn parse_epoch_v3(line: &[u8], epoch: &mut Epoch) -> EpochParse {
    let mut parse = EpochParse::default();
    if line.first() != Some(&b'>') {
        return parse;
    }
    parse.is_epoch = true;
    if line.len() < 32 || !is_digit(line[31]) {
        parse.problem = String::from("its event flag (column 32) is missing");
        return parse;
    }
    epoch.flag = i32::from(line[31] - b'0');
    if let Some(problem) = read_count(
        columns(line, 33, 3),
        epoch,
        "its satellite count (columns 33-35) is not a number",
    ) {
        parse.problem = problem;
        return parse;
    }
    parse.count_read = true;
    let time_fields = &line[1..31];
    epoch.timed = !is_blank(time_fields);
    if epoch.timed {
        parse.problem = read_time(
            columns(line, 3, 4),
            columns(line, 8, 2),
            columns(line, 11, 2),
            columns(line, 14, 2),
            columns(line, 17, 2),
            columns(line, 19, 11),
            false,
            &mut epoch.time,
        );
    }
    parse
}

// ---- What the header says ----------------------------------------------------------

// The part of the header that describes an occupation. Header records -
// whether in the header or after an event flag - update this; an occupation
// takes a copy of it when it starts.
#[derive(Clone, Debug, Default)]
struct Setup {
    session: GnssSession,
    marker_type: String,
    interval: Option<f64>,
    marker_record: usize,
    position_record: usize,
    /// ANTENNA: DELTA H/E/N seen since the last new site.
    antenna_stated: bool,
}

impl Setup {
    // Whether two setups describe the same occupation: the same marker, the
    // same antenna at the same height, the same receiver and position.
    fn same_occupation(&self, other: &Setup) -> bool {
        self.session.marker_name == other.session.marker_name
            && self.session.marker_number == other.session.marker_number
            && self.session.antenna == other.session.antenna
            && self.session.approximate_position == other.session.approximate_position
            && self.session.receiver_type == other.session.receiver_type
            && self.session.receiver_serial == other.session.receiver_serial
    }
}

// The most common gap between consecutive epochs, in milliseconds: the
// interval when the header does not state one, and the check when it does.
#[derive(Clone, Copy, Debug, Default)]
struct GapCounter {
    slots: [(i64, usize); 8],
    used: usize,
}

impl GapCounter {
    fn add(&mut self, seconds: f64) {
        // std::llround: half away from zero, as f64::round.
        let key = (seconds * 1000.0).round() as i64;
        for slot in &mut self.slots[..self.used] {
            if slot.0 == key {
                slot.1 += 1;
                return;
            }
        }
        // A handful of distinct gaps is all a real file has (the interval
        // and its multiples where epochs are missing); an eighth kind of gap
        // is not going to be the most common one.
        if self.used < self.slots.len() {
            self.slots[self.used] = (key, 1);
            self.used += 1;
        }
    }

    fn most_common(&self) -> Option<f64> {
        if self.used == 0 {
            return None;
        }
        let mut best = 0;
        for i in 1..self.used {
            if self.slots[i].1 > self.slots[best].1 {
                best = i;
            }
        }
        Some(self.slots[best].0 as f64 / 1000.0)
    }
}

#[derive(Clone, Debug, Default)]
struct Occupation {
    setup: Setup,
    start_record: usize,
    have_epoch: bool,
    first: f64,
    last: f64,
    previous: f64,
    first_time: EpochTime,
    last_time: EpochTime,
    epochs: usize,
    /// A bit per satellite number, per system letter A-Z.
    satellites: [u128; 26],
    gaps: GapCounter,
}

// A satellite as a RINEX 2 epoch lists it; number 0 for an unreadable entry.
#[derive(Clone, Copy, Debug)]
struct Satellite {
    system: u8,
    number: i32,
}

const UNREADABLE_SATELLITE: Satellite = Satellite {
    system: b' ',
    number: 0,
};

// The observation types of one system (RINEX 3/4), or of the whole file (2).
#[derive(Clone, Debug, Default)]
struct ObservationTypes {
    /// The count the record gives: what the data records follow.
    declared: i32,
    codes: Vec<String>,
}

// Header record labels kept as metadata, verbatim, under a plainer name.
const METADATA_RECORDS: [(&[u8], &str); 21] = [
    (b"SIGNAL STRENGTH UNIT", "signal strength unit"),
    (b"RCV CLOCK OFFS APPL", "receiver clock offset applied"),
    (b"SYS / DCBS APPLIED", "differential code biases applied"),
    (b"SYS / PCVS APPLIED", "phase centre variations applied"),
    (b"SYS / SCALE FACTOR", "observation scale factors"),
    (b"SYS / PHASE SHIFT", "phase shifts"),
    (b"GLONASS COD/PHS/BIS", "GLONASS code-phase biases"),
    (
        b"GLONASS SLOT / FRQ #",
        "GLONASS slots and frequency numbers",
    ),
    (b"LEAP SECONDS", "leap seconds"),
    (b"# OF SATELLITES", "satellites (header)"),
    (b"PRN / # OF OBS", "observations per satellite (header)"),
    (b"WAVELENGTH FACT L1/2", "wavelength factors"),
    (b"DOI", "DOI"),
    (b"LICENSE OF USE", "licence of use"),
    (b"STATION INFORMATION", "station information"),
    (b"ANTENNA: DELTA X/Y/Z", "antenna delta X/Y/Z (vehicle)"),
    (b"ANTENNA: PHASECENTER", "antenna phase centre"),
    (b"ANTENNA: B.SIGHT XYZ", "antenna boresight"),
    (b"ANTENNA: ZERODIR AZI", "antenna zero direction azimuth"),
    (b"ANTENNA: ZERODIR XYZ", "antenna zero direction"),
    (b"CENTER OF MASS: XYZ", "centre of mass (vehicle)"),
];

// What the satellite records of an epoch are for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordUse {
    /// An observation epoch: its satellites count.
    Counted,
    /// Cycle slips (flag 6): checked, not counted.
    ReadThrough,
    /// An epoch that could not be read: stepped over.
    Skipped,
}

// ---- The reader --------------------------------------------------------------------

struct ObservationReader<'a> {
    file_name: String,
    options: &'a ReadOptions,
    cursor: LineCursor<'a>,
    // A Compact RINEX file, expanded: the cursor then reads its text, and every
    // line number a warning or source record gives is mapped back to the
    // compact file's own line.
    compact: Option<&'a CompactExpansion>,

    version: VersionRecord,
    version_two: bool,
    /// As RINEX writes it: "GPS", "GLO", ...
    time_system: String,
    /// As a SurveyTimestamp says it: GLO is "UTC".
    time_label: String,
    header_first: Option<EpochTime>,
    header_last: Option<EpochTime>,

    types_v2: ObservationTypes,
    types_v3: [ObservationTypes; 26],
    /// The system a blank-led SYS / # / OBS TYPES line continues.
    continuing_system: u8,
    /// One RINEX 2 epoch's list, reused.
    satellites: Vec<Satellite>,

    setup: Setup,
    active: Option<Occupation>,
    finished: Vec<Occupation>,
    moving: bool,
    /// The first event flag 2, for the warning.
    first_moving_record: usize,

    metadata: BTreeMap<String, String>,
    comments: Vec<String>,
    kinematic_epochs: usize,
    power_failures: usize,
    cycle_slip_records: usize,
    satellite_records: usize,
    external_events: Vec<String>,
    external_event_count: usize,
    text_cleaned: bool,

    result: ReadResult,
    suppressed_warnings: usize,
}

impl<'a> ObservationReader<'a> {
    fn new(
        text: &'a [u8],
        file_name: &str,
        options: &'a ReadOptions,
        compact: Option<&'a CompactExpansion>,
    ) -> Self {
        Self {
            file_name: String::from(file_name),
            options,
            cursor: LineCursor::new(text),
            compact,
            version: VersionRecord::default(),
            version_two: false,
            time_system: String::new(),
            time_label: String::new(),
            header_first: None,
            header_last: None,
            types_v2: ObservationTypes::default(),
            types_v3: Default::default(),
            continuing_system: 0,
            satellites: Vec::new(),
            setup: Setup::default(),
            active: None,
            finished: Vec::new(),
            moving: false,
            first_moving_record: 0,
            metadata: BTreeMap::new(),
            comments: Vec::new(),
            kinematic_epochs: 0,
            power_failures: 0,
            cycle_slip_records: 0,
            satellite_records: 0,
            external_events: Vec::new(),
            external_event_count: 0,
            text_cleaned: false,
            result: ReadResult::default(),
            suppressed_warnings: 0,
        }
    }

    fn file_line(&self, record: usize) -> usize {
        match self.compact {
            Some(compact) => compact.lines.compact_line(record),
            None => record,
        }
    }

    fn warn(&mut self, record: usize, message: String) {
        if self.result.warnings.len() < MAX_LISTED_WARNINGS {
            let record = self.file_line(record);
            self.result.warnings.push(ReadWarning {
                file_name: self.file_name.clone(),
                record,
                message,
            });
        } else {
            self.suppressed_warnings += 1;
        }
    }

    fn source_at(&self, record: usize) -> SourceRecord {
        SourceRecord {
            manufacturer: String::from(Manufacturer::OpenStandard.name()),
            format: String::from(HUMAN_NAME),
            format_version: self.version.version_text.clone(),
            file_name: self.file_name.clone(),
            record_number: self.file_line(record),
        }
    }

    fn marker_id_from_file_name(&self) -> String {
        // A short name's first four characters and a long name's first nine are
        // the station; either way it is what comes before the first '.' or '_'.
        let cut = self.file_name.find(['.', '_']);
        let stem = &self.file_name[..cut.unwrap_or(self.file_name.len())];
        if stem.is_empty() {
            String::from("RINEX marker")
        } else {
            String::from(stem)
        }
    }

    fn append_metadata(&mut self, key: &str, value: String) {
        match self.metadata.entry(String::from(key)) {
            Entry::Vacant(slot) => {
                slot.insert(value);
            }
            Entry::Occupied(mut slot) => {
                let text = slot.get_mut();
                text.push_str("; ");
                text.push_str(&value);
            }
        }
    }

    fn clean(&mut self, text: &[u8]) -> String {
        clean_text(text, Some(&mut self.text_cleaned))
    }

    fn three_reals(content: &[u8]) -> Option<[f64; 3]> {
        // 3F14.4, but a hand-edited header is often only roughly in its columns;
        // three numbers separated by blanks mean the same thing.
        let mut values = [0.0; 3];
        let mut fixed = true;
        for (i, slot) in values.iter_mut().enumerate() {
            if !fixed {
                break;
            }
            let value = real_field(columns(content, 1 + 14 * i, 14));
            fixed = value.is_some();
            if let Some(value) = value {
                *slot = value;
            }
        }
        if fixed {
            return Some(values);
        }
        let fields = tokens(content);
        if fields.len() != 3 {
            return None;
        }
        for (slot, field) in values.iter_mut().zip(&fields) {
            *slot = real_field(field)?;
        }
        Some(values)
    }

    // ---- Header ------------------------------------------------------------------------

    fn read_header(&mut self) -> Status {
        while let Some(line) = self.cursor.next() {
            let record = self.cursor.line_number();
            if header_label(line) == b"END OF HEADER" {
                self.result.records_read += 1;
                return Ok(());
            }
            self.apply_header_record(line, record, false);
        }
        Err(Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "{} has no END OF HEADER record: the file is cut short inside its header, or is \
                 not a RINEX observation file",
                self.file_name
            ),
        ))
    }

    fn apply_header_record(&mut self, line: &[u8], record: usize, in_body: bool) {
        let label = header_label(line);
        let content = header_content(line);
        if label.is_empty() {
            self.warn(
                record,
                String::from("not a header record: there is no label in columns 61-80"),
            );
            self.result.records_skipped += 1;
            return;
        }
        self.result.records_read += 1;

        if label == b"COMMENT" {
            let comment = self.clean(content);
            self.comments.push(comment);
        } else if label == b"PGM / RUN BY / DATE" {
            let program = self.clean(columns(content, 1, 20));
            self.append_metadata("program", program);
            let run_by = self.clean(columns(content, 21, 20));
            self.append_metadata("run by", run_by);
            let created = self.clean(columns(content, 41, 20));
            self.append_metadata("file created", created);
        } else if label == b"MARKER NAME" {
            self.setup.session.marker_name = self.clean(content);
            self.setup.marker_record = record;
        } else if label == b"MARKER NUMBER" {
            self.setup.session.marker_number = self.clean(columns(content, 1, 20));
        } else if label == b"MARKER TYPE" {
            self.setup.marker_type = self.clean(columns(content, 1, 20));
        } else if label == b"OBSERVER / AGENCY" {
            self.setup.session.observer = self.clean(columns(content, 1, 20));
            self.setup.session.agency = self.clean(columns(content, 21, 40));
        } else if label == b"REC # / TYPE / VERS" {
            self.setup.session.receiver_serial = self.clean(columns(content, 1, 20));
            self.setup.session.receiver_type = self.clean(columns(content, 21, 20));
            self.setup.session.receiver_firmware = self.clean(columns(content, 41, 20));
        } else if label == b"ANT # / TYPE" {
            self.setup.session.antenna.serial_number = self.clean(columns(content, 1, 20));
            // The IGS antenna name is the model in columns 21-36 and the radome
            // in 37-40; the blanks between them are part of the name.
            self.setup.session.antenna.r#type = self.clean(columns(content, 21, 20));
        } else if label == b"APPROX POSITION XYZ" {
            match Self::three_reals(content) {
                None => {
                    self.warn(
                        record,
                        String::from(
                            "APPROX POSITION XYZ is not three numbers; the marker has no \
                             position from this record",
                        ),
                    );
                    self.setup.session.approximate_position = None;
                }
                Some(xyz) => {
                    self.setup.session.approximate_position = Some(GeocentricCoordinate {
                        x: xyz[0],
                        y: xyz[1],
                        z: xyz[2],
                    });
                    self.setup.position_record = record;
                }
            }
        } else if label == b"ANTENNA: DELTA H/E/N" {
            match Self::three_reals(content) {
                None => self.warn(
                    record,
                    String::from(
                        "ANTENNA: DELTA H/E/N is not three numbers; the antenna height is not \
                         known from this record",
                    ),
                ),
                Some(hen) => {
                    // RINEX 2.11 table A1 and 3.05 table A2: the height of the
                    // antenna reference point above the marker, and its east and
                    // north eccentricity - a vertical height to the ARP.
                    let antenna = &mut self.setup.session.antenna;
                    antenna.height = hen[0];
                    antenna.east_offset = hen[1];
                    antenna.north_offset = hen[2];
                    antenna.method = AntennaHeightMethod::Vertical;
                    antenna.measured_to = String::from("antenna reference point (ARP)");
                    self.setup.antenna_stated = true;
                }
            }
        } else if label == b"# / TYPES OF OBSERV" {
            if !self.version_two {
                let message = format!(
                    "# / TYPES OF OBSERV is a RINEX 2 record in a RINEX {} file; ignored",
                    self.version.version_text
                );
                self.warn(record, message);
            } else {
                self.read_observation_types_v2(content, record, in_body);
            }
        } else if label == b"SYS / # / OBS TYPES" {
            if self.version_two {
                let message = format!(
                    "SYS / # / OBS TYPES is a RINEX 3 record in a RINEX {} file; ignored",
                    self.version.version_text
                );
                self.warn(record, message);
            } else {
                self.read_observation_types_v3(content, record, in_body);
            }
        } else if label == b"INTERVAL" {
            let interval = real_field(columns(content, 1, 10));
            if let Some(interval) = interval.filter(|&interval| interval > 0.0) {
                self.setup.interval = Some(interval);
            } else if interval.is_none_or(|interval| interval < 0.0) {
                // Zero is not an interval (no warning); some writers put it
                // there for "unknown".
                self.warn(
                    record,
                    String::from("INTERVAL is not a positive number of seconds; ignored"),
                );
            }
        } else if label == b"TIME OF FIRST OBS" || label == b"TIME OF LAST OBS" {
            // 5I6,F13.7,5X,A3
            let first = label == b"TIME OF FIRST OBS";
            let mut time = EpochTime::default();
            let problem = read_time(
                columns(content, 1, 6),
                columns(content, 7, 6),
                columns(content, 13, 6),
                columns(content, 19, 6),
                columns(content, 25, 6),
                columns(content, 31, 13),
                false,
                &mut time,
            );
            let name = if first {
                "TIME OF FIRST OBS"
            } else {
                "TIME OF LAST OBS"
            };
            if !problem.is_empty() {
                self.warn(record, format!("{name} is not a time ({problem}); ignored"));
            } else if first {
                self.header_first = Some(time);
                let system = self.clean(columns(content, 49, 3));
                if !system.is_empty() {
                    self.time_system = system;
                }
            } else {
                self.header_last = Some(time);
            }
        } else if label == b"RINEX VERSION / TYPE" || label == b"END OF HEADER" {
            let name = if label == b"END OF HEADER" {
                "END OF HEADER"
            } else {
                "RINEX VERSION / TYPE"
            };
            if in_body {
                self.warn(record, format!("{name} after an event flag; ignored"));
            } else {
                self.warn(record, format!("a second {name} record; ignored"));
            }
        } else if let Some(&(_, key)) = METADATA_RECORDS.iter().find(|(known, _)| *known == label) {
            let value = self.clean(content);
            self.append_metadata(key, value);
        } else {
            let value = self.clean(content);
            let name = self.clean(label);
            self.append_metadata(&format!("header record {name}"), value);
            let message = format!(
                "header record '{name}' is not one RINEX {} defines; kept in the metadata",
                self.version.version_text
            );
            self.warn(record, message);
        }
    }

    fn read_observation_types_v2(&mut self, content: &[u8], record: usize, in_body: bool) {
        // I6, 9(4X,A2), continuation lines 6X,9(4X,A2).
        let count_field = columns(content, 1, 6);
        if !is_blank(count_field) {
            let Some(count) = read_int(count_field).filter(|count| (0..=999).contains(count))
            else {
                self.warn(
                    record,
                    String::from("# / TYPES OF OBSERV does not start with a number of types"),
                );
                return;
            };
            if in_body && count != self.types_v2.declared {
                let message = format!(
                    "the observation types change here from {} to {count}",
                    self.types_v2.declared
                );
                self.warn(record, message);
            }
            self.types_v2 = ObservationTypes {
                declared: count,
                codes: Vec::new(),
            };
        }
        for code in tokens(&content[content.len().min(6)..]) {
            let code = clean_text(code, Some(&mut self.text_cleaned));
            self.types_v2.codes.push(code);
        }
    }

    fn read_observation_types_v3(&mut self, content: &[u8], record: usize, in_body: bool) {
        // A1,2X,I3,13(1X,A3), continuation lines 6X,13(1X,A3).
        if !content.is_empty() && content[0] != b' ' {
            let system = content[0];
            let count_field = columns(content, 4, 3);
            let name = match system_name(system) {
                Some(name) if system != b'T' => name,
                _ => {
                    let message = format!(
                        "SYS / # / OBS TYPES names satellite system '{}', which RINEX does not \
                         define; its records will be reported",
                        raw_text(&[system])
                    );
                    self.warn(record, message);
                    self.continuing_system = 0;
                    return;
                }
            };
            let Some(count) = read_int(count_field).filter(|count| (0..=999).contains(count))
            else {
                self.warn(
                    record,
                    String::from(
                        "SYS / # / OBS TYPES does not give a number of types in columns 4-6",
                    ),
                );
                self.continuing_system = 0;
                return;
            };
            let declared = self.types_v3[usize::from(system - b'A')].declared;
            if in_body && count != declared {
                self.warn(
                    record,
                    format!(
                        "the observation types of {name} change here from {declared} to {count}"
                    ),
                );
            }
            self.types_v3[usize::from(system - b'A')] = ObservationTypes {
                declared: count,
                codes: Vec::new(),
            };
            self.continuing_system = system;
        } else if self.continuing_system == 0 {
            self.warn(
                record,
                String::from("a SYS / # / OBS TYPES continuation line with no system before it"),
            );
            return;
        }
        let letter = usize::from(self.continuing_system - b'A');
        for code in tokens(&content[content.len().min(6)..]) {
            let code = clean_text(code, Some(&mut self.text_cleaned));
            self.types_v3[letter].codes.push(code);
        }
    }

    // ---- Occupations -------------------------------------------------------------------

    fn start_occupation(&mut self, record: usize) {
        self.active = Some(Occupation {
            setup: self.setup.clone(),
            start_record: record,
            ..Default::default()
        });
    }

    fn close_occupation(&mut self) {
        if let Some(occupation) = self.active.take() {
            self.finished.push(occupation);
        }
    }

    fn mark_satellite(&mut self, system: u8, number: i32) {
        if !self.moving
            && system.is_ascii_uppercase()
            && number > 0
            && number < SATELLITE_NUMBERS
            && let Some(active) = self.active.as_mut()
        {
            active.satellites[usize::from(system - b'A')] |= 1u128 << number;
        }
    }

    fn count_epoch(&mut self, epoch: &Epoch, record: usize) {
        if epoch.flag == 1 {
            self.power_failures += 1;
            self.warn(
                record,
                String::from(
                    "event flag 1: the receiver lost power between the previous epoch and this one",
                ),
            );
        }
        if self.moving {
            self.kinematic_epochs += 1;
            return;
        }
        let Some(occupation) = self.active.as_mut() else {
            self.kinematic_epochs += 1;
            return;
        };
        let seconds = seconds_of(&epoch.time);
        let mut problem = None;
        if !occupation.have_epoch {
            occupation.have_epoch = true;
            occupation.first = seconds;
            occupation.last = seconds;
            occupation.first_time = epoch.time;
            occupation.last_time = epoch.time;
        } else {
            let gap = seconds - occupation.previous;
            if gap.abs() < SAME_EPOCH_SECONDS {
                problem = Some("this epoch repeats the time of the epoch before it");
            } else if gap < 0.0 {
                problem = Some("this epoch is earlier than the epoch before it");
            } else {
                occupation.gaps.add(gap);
            }
            if seconds < occupation.first {
                occupation.first = seconds;
                occupation.first_time = epoch.time;
            }
            if seconds > occupation.last {
                occupation.last = seconds;
                occupation.last_time = epoch.time;
            }
        }
        occupation.previous = seconds;
        occupation.epochs += 1;
        if let Some(problem) = problem {
            self.warn(record, String::from(problem));
        }
    }

    // ---- Body --------------------------------------------------------------------------

    fn looks_like_epoch(&self, line: &[u8]) -> bool {
        let mut epoch = Epoch::default();
        if self.version_two {
            parse_epoch_v2(line, &mut epoch).is_epoch
        } else {
            parse_epoch_v3(line, &mut epoch).is_epoch
        }
    }

    fn skip_to_next_epoch(&mut self, first_record: usize, why: &str) {
        let mut skipped = 1;
        while !self.cursor.at_end() {
            let mark = self.cursor.mark();
            let Some(line) = self.cursor.next() else {
                break;
            };
            if self.looks_like_epoch(line) {
                self.cursor.reset(mark);
                break;
            }
            skipped += 1;
        }
        self.result.records_skipped += skipped;
        let last_record = first_record + skipped - 1;
        let mut what = String::from("; skipped it");
        if skipped == 2 {
            what.push_str(" and the next line");
        } else if skipped > 2 {
            what.push_str(&format!(
                " and the next {} lines, to line {last_record}",
                skipped - 1
            ));
        }
        self.warn(first_record, format!("{why}{what}"));
    }

    fn read_body(&mut self) {
        while let Some(line) = self.cursor.next() {
            let record = self.cursor.line_number();
            let mut epoch = Epoch::default();
            let parse = if self.version_two {
                parse_epoch_v2(line, &mut epoch)
            } else {
                parse_epoch_v3(line, &mut epoch)
            };
            if !parse.is_epoch {
                if is_blank(line) && self.cursor.at_end() {
                    continue; // a blank last line: an editor's, not the writer's
                }
                self.skip_to_next_epoch(
                    record,
                    "this line is not an epoch record where one should start",
                );
                continue;
            }
            if !parse.problem.is_empty() && parse.count_read && (2..=5).contains(&epoch.flag) {
                // What an event's header records say does not depend on when it
                // happened: read them, and say the time was not.
                self.warn(
                    record,
                    format!(
                        "the time of this event cannot be read ({}); the header records after \
                         it are still read",
                        parse.problem
                    ),
                );
                epoch.timed = false;
                self.result.records_read += 1;
                self.read_special_records(&epoch, record);
                continue;
            }
            if !parse.problem.is_empty() {
                // The count still says how many records belong to it: read past
                // them, so the next epoch is found where it starts.
                self.warn(
                    record,
                    format!("an epoch record that cannot be read: {}", parse.problem),
                );
                self.result.records_skipped += 1;
                if epoch.flag <= 1 || epoch.flag == 6 {
                    self.read_epoch_records(&epoch, line, record, RecordUse::Skipped);
                }
                continue;
            }
            match epoch.flag {
                0 | 1 => {
                    if !epoch.timed {
                        self.warn(
                            record,
                            String::from(
                                "an observation epoch with no date and time; its records are \
                                 skipped",
                            ),
                        );
                        self.result.records_skipped += 1;
                        self.read_epoch_records(&epoch, line, record, RecordUse::Skipped);
                    } else {
                        self.result.records_read += 1;
                        self.count_epoch(&epoch, record);
                        self.read_epoch_records(&epoch, line, record, RecordUse::Counted);
                    }
                }
                6 => {
                    self.result.records_read += 1;
                    self.cycle_slip_records += epoch.count as usize;
                    self.read_epoch_records(&epoch, line, record, RecordUse::ReadThrough);
                }
                2..=5 => {
                    self.result.records_read += 1;
                    self.read_special_records(&epoch, record);
                }
                _ => {
                    let why = format!(
                        "event flag {} is not one RINEX defines (0 to 6)",
                        epoch.flag
                    );
                    self.skip_to_next_epoch(record, &why);
                }
            }
        }
    }

    fn read_satellite_v3(&mut self, line: &[u8], record: usize, counted: bool) -> bool {
        if line.len() < 3
            || system_name(line[0]).is_none()
            || !(is_digit(line[1]) || line[1] == b' ')
            || !is_digit(line[2])
        {
            self.warn(
                record,
                String::from(
                    "not a satellite record: it does not start with a system letter and a \
                     two-digit satellite number",
                ),
            );
            return false;
        }
        let system = line[0];
        let name = system_name(system).unwrap_or_default();
        // Only built into warnings: the common path must not touch the heap.
        let satellite = || raw_text(&line[..3]);
        let number = (if line[1] == b' ' {
            0
        } else {
            i32::from(line[1] - b'0') * 10
        }) + i32::from(line[2] - b'0');
        let declared = self.types_v3[usize::from(system - b'A')].declared;
        if declared == 0 {
            self.warn(
                record,
                format!(
                    "satellite {}: the header declares no observation types for {name}",
                    satellite()
                ),
            );
            return false;
        }
        let fields = declared as usize * 16;
        let mut values = &line[3..];
        if values.len() > fields {
            if !is_blank(&values[fields..]) {
                self.warn(
                    record,
                    format!(
                        "satellite {} has more than the {declared} observations the header \
                         declares for {name}",
                        satellite()
                    ),
                );
                return false;
            }
            values = &values[..fields];
        }
        if !only_observation_characters(values) {
            self.warn(
                record,
                format!(
                    "satellite {} holds characters an observation field (F14.3, I1, I1) cannot",
                    satellite()
                ),
            );
            return false;
        }
        if number == 0 {
            self.warn(
                record,
                format!("satellite {}: there is no satellite number 0", satellite()),
            );
            return false;
        }
        if counted {
            self.mark_satellite(system, number);
        }
        true
    }

    fn read_satellite_list_v2(
        &mut self,
        list: &[u8],
        record: usize,
        satellites: &mut Vec<Satellite>,
    ) -> bool {
        // 12(A1,I2). A blank system letter is GPS (RINEX 2.11 section 5.1).
        let mut readable = true;
        for entry in list.chunks(3) {
            if is_blank(entry) {
                continue;
            }
            let system = if entry[0] == b' ' { b'G' } else { entry[0] };
            if entry.len() < 3
                || system_name(system).is_none()
                || !(is_digit(entry[1]) || entry[1] == b' ')
                || !is_digit(entry[2])
            {
                readable = false;
                satellites.push(UNREADABLE_SATELLITE);
                continue;
            }
            let number = (if entry[1] == b' ' {
                0
            } else {
                i32::from(entry[1] - b'0') * 10
            }) + i32::from(entry[2] - b'0');
            satellites.push(Satellite { system, number });
        }
        if !readable {
            self.warn(
                record,
                String::from("the satellite list holds an entry that is not a satellite (A1,I2)"),
            );
        }
        readable
    }

    fn ended(&mut self, epoch: &Epoch, epoch_record: usize, missing: i32) {
        self.warn(
            epoch_record,
            format!(
                "the file ends inside this epoch: {missing} of its {} satellite records are \
                 missing",
                epoch.count
            ),
        );
    }

    fn cut_short(&mut self, epoch: &Epoch, epoch_record: usize, found: i32) {
        self.warn(
            epoch_record,
            format!(
                "the epoch lists {} satellites but only {found} records follow it",
                epoch.count
            ),
        );
    }

    fn read_epoch_records(
        &mut self,
        epoch: &Epoch,
        epoch_line: &[u8],
        epoch_record: usize,
        use_: RecordUse,
    ) {
        let counted = use_ == RecordUse::Counted;
        if !self.version_two {
            for i in 0..epoch.count {
                let mark = self.cursor.mark();
                let Some(line) = self.cursor.next() else {
                    self.ended(epoch, epoch_record, epoch.count - i);
                    return;
                };
                if line.first() == Some(&b'>') {
                    self.cursor.reset(mark);
                    self.cut_short(epoch, epoch_record, i);
                    return;
                }
                if use_ == RecordUse::Skipped {
                    self.result.records_skipped += 1;
                    continue;
                }
                let number = self.cursor.line_number();
                if self.read_satellite_v3(line, number, counted) {
                    self.result.records_read += 1;
                    self.satellite_records += usize::from(counted);
                } else {
                    self.result.records_skipped += 1;
                }
            }
            return;
        }
        // The list is the reader's, reused from epoch to epoch.
        let mut satellites = std::mem::take(&mut self.satellites);
        satellites.clear();
        self.read_epoch_records_v2(epoch, epoch_line, epoch_record, use_, &mut satellites);
        self.satellites = satellites;
    }

    // RINEX 2: up to twelve satellites on the epoch line, twelve more on each
    // continuation line (32X,12(A1,I2)); then each satellite has one line per
    // five observation types.
    fn read_epoch_records_v2(
        &mut self,
        epoch: &Epoch,
        epoch_line: &[u8],
        epoch_record: usize,
        use_: RecordUse,
        satellites: &mut Vec<Satellite>,
    ) {
        let counted = use_ == RecordUse::Counted;
        if use_ != RecordUse::Skipped {
            self.read_satellite_list_v2(columns(epoch_line, 33, 36), epoch_record, satellites);
        }
        let continuation_lines = if epoch.count > 12 {
            (epoch.count - 1) / 12
        } else {
            0
        };
        for _ in 0..continuation_lines {
            let mark = self.cursor.mark();
            let Some(line) = self.cursor.next() else {
                self.ended(epoch, epoch_record, epoch.count);
                return;
            };
            if !is_blank(&line[..line.len().min(32)]) || self.looks_like_epoch(line) {
                self.cursor.reset(mark);
                self.warn(
                    epoch_record,
                    format!(
                        "the epoch lists {} satellites but its satellite list is cut short",
                        epoch.count
                    ),
                );
                return;
            }
            if use_ == RecordUse::Skipped {
                self.result.records_skipped += 1;
                continue;
            }
            self.result.records_read += 1;
            let number = self.cursor.line_number();
            self.read_satellite_list_v2(columns(line, 33, 36), number, satellites);
        }
        if use_ != RecordUse::Skipped && satellites.len() != epoch.count as usize {
            self.warn(
                epoch_record,
                format!(
                    "the epoch says {} satellites but lists {}",
                    epoch.count,
                    satellites.len()
                ),
            );
        }
        let lines_per_satellite = (self.types_v2.declared + 4) / 5;
        for i in 0..epoch.count {
            let mut whole = true;
            for _ in 0..lines_per_satellite {
                let mark = self.cursor.mark();
                let Some(line) = self.cursor.next() else {
                    self.ended(epoch, epoch_record, epoch.count - i);
                    return;
                };
                if self.looks_like_epoch(line) {
                    self.cursor.reset(mark);
                    self.cut_short(epoch, epoch_record, i);
                    return;
                }
                if use_ == RecordUse::Skipped {
                    self.result.records_skipped += 1;
                } else if line.len() <= 80 && only_observation_characters(line) {
                    self.result.records_read += 1;
                } else {
                    whole = false;
                    self.result.records_skipped += 1;
                    let number = self.cursor.line_number();
                    self.warn(
                        number,
                        String::from(if line.len() > 80 {
                            "an observation line longer than the 80 columns RINEX 2 allows"
                        } else {
                            "holds characters an observation field (F14.3, I1, I1) cannot"
                        }),
                    );
                }
            }
            if whole
                && counted
                && let Some(satellite) = satellites.get(i as usize)
                && satellite.number > 0
            {
                self.mark_satellite(satellite.system, satellite.number);
                self.satellite_records += 1;
            }
        }
    }

    fn read_special_records(&mut self, epoch: &Epoch, epoch_record: usize) {
        if epoch.flag == 2 {
            if !self.moving {
                self.close_occupation();
                self.moving = true;
                if self.first_moving_record == 0 {
                    self.first_moving_record = epoch_record;
                }
            }
        } else if epoch.flag == 3 {
            // A new site: what identified the old one no longer applies. The
            // antenna and receiver carry on (it is the same equipment moved), and
            // the height is carried with a warning if the new site does not
            // restate it.
            let session = &mut self.setup.session;
            session.marker_name.clear();
            session.marker_number.clear();
            session.approximate_position = None;
            self.setup.marker_type.clear();
            self.setup.marker_record = 0;
            self.setup.position_record = 0;
            self.setup.antenna_stated = false;
        } else if epoch.flag == 5 {
            self.external_event_count += 1;
            // A list, not a count alone: the events are what a person looks for
            // (a camera exposure, a photogrammetry mark). Capped so a file of
            // events cannot grow the metadata without bound.
            if self.external_events.len() < 100 {
                let when = if epoch.timed {
                    survey_text(&timestamp_of(&epoch.time, &self.time_label))
                } else {
                    String::from("(no time)")
                };
                self.external_events.push(when);
            }
        }

        for i in 0..epoch.count {
            let mark = self.cursor.mark();
            let Some(line) = self.cursor.next() else {
                self.warn(
                    epoch_record,
                    format!(
                        "the file ends before the {} header records this event promises",
                        epoch.count
                    ),
                );
                break;
            };
            if header_label(line).is_empty() && self.looks_like_epoch(line) {
                self.cursor.reset(mark);
                self.warn(
                    epoch_record,
                    format!(
                        "the event promises {} header records but only {i} follow",
                        epoch.count
                    ),
                );
                break;
            }
            let number = self.cursor.line_number();
            self.apply_header_record(line, number, true);
        }

        if epoch.flag == 3 {
            self.close_occupation();
            self.moving = false;
            if self.setup.marker_record == 0 {
                self.warn(
                    epoch_record,
                    String::from(
                        "a new site occupation (event flag 3) without a MARKER NAME; the site is \
                         named after the file",
                    ),
                );
            }
            if !self.setup.antenna_stated {
                let message = format!(
                    "the new site occupation does not restate ANTENNA: DELTA H/E/N; the previous \
                     antenna height ({} m) is kept, as RINEX carries header values forward",
                    format_number(self.setup.session.antenna.height, 4)
                );
                self.warn(epoch_record, message);
            }
            self.start_occupation(epoch_record);
            return;
        }
        if !self.moving
            && let Some(active) = &self.active
            && !active.setup.same_occupation(&self.setup)
            && active.epochs > 0
        {
            self.close_occupation();
            self.start_occupation(epoch_record);
            self.warn(
                epoch_record,
                String::from(
                    "the header records after this event change the marker, antenna or \
                     position: a new session starts here",
                ),
            );
            return;
        }
        if !self.moving
            && let Some(active) = self.active.as_mut()
        {
            active.setup = self.setup.clone();
        }
    }

    // ---- The result --------------------------------------------------------------------

    fn finish(&mut self) {
        self.close_occupation();
        let mut project = std::mem::take(&mut self.result.project);
        project.source = self.source_at(0);
        project.units.linear = LinearUnit::Metres;

        // id -> index in unpositioned_points
        let mut markers: BTreeMap<String, usize> = BTreeMap::new();
        let mut positions = 0usize;
        let mut unplaced: Vec<String> = Vec::new();
        let mut last_with_epochs: Option<usize> = None;

        let finished = std::mem::take(&mut self.finished);
        for (index, occupation) in finished.iter().enumerate() {
            let mut session = occupation.setup.session.clone();
            let record = if occupation.setup.marker_record != 0 {
                occupation.setup.marker_record
            } else {
                occupation.start_record
            };
            if session.marker_name.is_empty() {
                let mut id = self.marker_id_from_file_name();
                if index > 0 {
                    id.push_str(&format!(" occupation {}", index + 1));
                }
                if occupation.setup.marker_record == 0 && index == 0 {
                    self.warn(
                        0,
                        format!(
                            "the header gives no MARKER NAME; the marker is named '{id}' after the \
                             file"
                        ),
                    );
                }
                session.marker_name = id;
            }
            session.format_version = self.version.version_text.clone();
            session.source = self.source_at(record);
            session.epoch_count = occupation.epochs;
            if occupation.have_epoch {
                last_with_epochs = Some(index);
                session.first_epoch = timestamp_of(&occupation.first_time, &self.time_label);
                session.last_epoch = timestamp_of(&occupation.last_time, &self.time_label);
            }
            let observed = occupation.gaps.most_common();
            session.interval_seconds = occupation.setup.interval.or(observed);
            if let (Some(interval), Some(observed)) = (occupation.setup.interval, observed)
                && (interval - observed).abs() > 5e-4
            {
                let message = format!(
                    "the header gives an interval of {} s, but the epochs of {} are mostly {} s \
                     apart",
                    format_number(interval, 3),
                    session.marker_name,
                    format_number(observed, 3)
                );
                self.warn(record, message);
            }
            for (letter, bits) in occupation.satellites.iter().enumerate() {
                let seen = bits.count_ones() as usize;
                if seen > 0
                    && let Some(name) = system_name(b'A' + letter as u8)
                {
                    session
                        .satellites_per_system
                        .insert(String::from(name), seen);
                }
            }
            if session.epoch_count == 0 {
                let message = format!(
                    "no observation epochs were recorded at {}",
                    session.marker_name
                );
                self.warn(record, message);
            }

            // The marker, once, however many sessions occupy it.
            if let Entry::Vacant(slot) = markers.entry(session.marker_name.clone()) {
                let mut point = UnpositionedPoint {
                    id: session.marker_name.clone(),
                    description: String::from("GNSS marker (RINEX observation file)"),
                    source: self.source_at(record),
                    ..Default::default()
                };
                if !session.marker_number.is_empty() {
                    point
                        .metadata
                        .insert(String::from("marker number"), session.marker_number.clone());
                }
                if !occupation.setup.marker_type.is_empty() {
                    point.metadata.insert(
                        String::from("marker type"),
                        occupation.setup.marker_type.clone(),
                    );
                }
                slot.insert(project.unpositioned_points.len());
                project.unpositioned_points.push(point);
            }

            // The header's approximate position, if it is one.
            if let Some(xyz) = session.approximate_position {
                let radius = (xyz.x * xyz.x + xyz.y * xyz.y + xyz.z * xyz.z).sqrt();
                let position_record = occupation.setup.position_record;
                if radius == 0.0 {
                    unplaced.push(format!(
                        "{} (its header position is 0, 0, 0)",
                        session.marker_name
                    ));
                    session.approximate_position = None;
                } else if !(NEAREST_PLAUSIBLE_RADIUS..=FURTHEST_PLAUSIBLE_RADIUS).contains(&radius)
                {
                    let message = format!(
                        "APPROX POSITION XYZ is {} km from the earth's centre, which is not on or \
                         near the ground; {} is imported without a position",
                        format_number(radius / 1000.0, 1),
                        session.marker_name
                    );
                    self.warn(position_record, message);
                    unplaced.push(format!(
                        "{} (its header position is implausible)",
                        session.marker_name
                    ));
                    session.approximate_position = None;
                } else {
                    let variance =
                        RINEX_APPROXIMATE_POSITION_SIGMA * RINEX_APPROXIMATE_POSITION_SIGMA;
                    project.observations.push(Observation::GnssGlobalPosition(
                        GnssGlobalPositionObservation {
                            point: session.marker_name.clone(),
                            geocentric: Some(xyz),
                            covariance: GnssCovariance3 {
                                xx: variance,
                                yy: variance,
                                zz: variance,
                                ..Default::default()
                            },
                            antenna: session.antenna.clone(),
                            solution: katana_survey::GnssSolution::Autonomous,
                            source: self.source_at(position_record),
                            ..Default::default()
                        },
                    ));
                    positions += 1;
                    if let Some(&at) = markers.get(&session.marker_name) {
                        project.unpositioned_points[at].metadata.insert(
                            String::from("position"),
                            String::from(
                                "approximate only: the RINEX header's APPROX POSITION XYZ",
                            ),
                        );
                    }
                }
            } else {
                unplaced.push(format!(
                    "{} (the header gives no APPROX POSITION XYZ)",
                    session.marker_name
                ));
            }
            project.gnss_sessions.push(session);
        }
        if let Some(first) = project.gnss_sessions.first() {
            project.name = first.marker_name.clone();
        }

        // The header's own first and last times, checked against the epochs.
        if let Some(header_first) = self.header_first
            && let Some(first) = finished.first()
            && first.have_epoch
            && (seconds_of(&header_first) - first.first).abs() > 5e-4
        {
            let message = format!(
                "TIME OF FIRST OBS in the header ({}) is not the time of the first epoch ({})",
                survey_text(&timestamp_of(&header_first, &self.time_label)),
                survey_text(&project.gnss_sessions[0].first_epoch)
            );
            self.warn(0, message);
        }
        if let Some(header_last) = self.header_last
            && let Some(index) = last_with_epochs
            && (seconds_of(&header_last) - finished[index].last).abs() > 5e-4
        {
            let message = format!(
                "TIME OF LAST OBS in the header ({}) is not the time of the last epoch ({})",
                survey_text(&timestamp_of(&header_last, &self.time_label)),
                survey_text(&timestamp_of(&finished[index].last_time, &self.time_label))
            );
            self.warn(0, message);
        }

        // Metadata: what the header says that the model has no field for.
        let mut metadata = std::mem::take(&mut self.metadata);
        metadata.insert(
            String::from("RINEX version"),
            self.version.version_text.clone(),
        );
        metadata.insert(
            String::from("time system"),
            if self.time_system.is_empty() {
                String::from("not stated")
            } else {
                self.time_system.clone()
            },
        );
        if !self.comments.is_empty() {
            metadata.insert(String::from("comments"), self.comments.join("\n"));
        }
        if self.version_two {
            metadata.insert(
                String::from("observation types"),
                self.types_v2.codes.join(" "),
            );
        } else {
            for (letter, types) in self.types_v3.iter().enumerate() {
                if types.declared > 0
                    && let Some(name) = system_name(b'A' + letter as u8)
                {
                    metadata.insert(format!("observation types {name}"), types.codes.join(" "));
                }
            }
        }
        metadata.insert(
            String::from("satellite records"),
            format!("{}", self.satellite_records),
        );
        if self.power_failures > 0 {
            metadata.insert(
                String::from("power failures (event flag 1)"),
                format!("{}", self.power_failures),
            );
        }
        if self.cycle_slip_records > 0 {
            metadata.insert(
                String::from("cycle slip records (event flag 6)"),
                format!("{}", self.cycle_slip_records),
            );
        }
        if self.external_event_count > 0 {
            let mut events = format!("{}:", self.external_event_count);
            for when in &self.external_events {
                events.push(' ');
                events.push_str(when);
            }
            if self.external_event_count > self.external_events.len() {
                events.push_str(" ...");
            }
            metadata.insert(String::from("external events (event flag 5)"), events);
        }
        project.metadata = metadata;
        if self.kinematic_epochs > 0 {
            project.metadata.insert(
                String::from("kinematic epochs"),
                format!("{}", self.kinematic_epochs),
            );
            let message = format!(
                "{} epochs were recorded with the antenna moving (event flag 2); they are \
                 counted, not imported as positions",
                self.kinematic_epochs
            );
            self.warn(self.first_moving_record, message);
            self.result.not_carried.push(format!(
                "positions for the {} epochs recorded while the antenna was moving",
                self.kinematic_epochs
            ));
        }
        if self.text_cleaned {
            self.warn(
                0,
                String::from(
                    "some header text held control characters or bytes that are not UTF-8; they \
                     were replaced",
                ),
            );
        }

        // What the file does not give an import, in words.
        self.result.not_carried.push(format!(
            "the raw GNSS observations: {} satellite records (pseudorange, carrier phase, \
             Doppler and signal strength) were checked and counted, not imported - Katana does \
             not process raw GNSS data",
            self.satellite_records
        ));
        if positions > 0 {
            self.result.not_carried.push(format!(
                "a survey-grade position: the marker position is the header's approximate one, \
                 weighted with a {} m standard deviation per axis",
                format_number(RINEX_APPROXIMATE_POSITION_SIGMA, 0)
            ));
            self.result.not_carried.push(String::from(
                "the reference frame of the approximate position: the file does not state one \
                 (RINEX 2 calls it WGS 84, RINEX 3 and 4 recommend ITRS)",
            ));
        }
        // One line per marker, however many of its sessions lack a position.
        unplaced.sort();
        unplaced.dedup();
        for marker in unplaced {
            self.result
                .not_carried
                .push(format!("a position for {marker}"));
        }
        self.result.project = project;
    }

    fn read_navigation(&mut self) {
        let candidates = navigation_file_candidates(&self.file_name);
        let Some(siblings) = self.options.siblings.clone() else {
            self.result.project.metadata.insert(
                String::from("navigation files"),
                String::from("not looked for: the file was read on its own"),
            );
            return;
        };
        let mut read: Vec<String> = Vec::new();
        let mut ephemerides: BTreeMap<String, usize> = BTreeMap::new();
        for name in &candidates {
            let bytes = match siblings(name.as_str()) {
                Ok(bytes) => bytes,
                Err(error) => {
                    if error.code != ErrorCode::NotFound {
                        self.warn(
                            0,
                            format!(
                                "the navigation file {name} could not be read: {}",
                                error.message
                            ),
                        );
                    }
                    continue;
                }
            };
            let summary = match summarise_navigation(&bytes, name) {
                Ok(summary) => summary,
                Err(error) => {
                    self.warn(
                        0,
                        format!("the navigation file {name} is not used: {}", error.message),
                    );
                    continue;
                }
            };
            read.push(format!("{name} (RINEX {})", summary.version_text));
            for (system, count) in &summary.ephemerides {
                *ephemerides.entry(system.clone()).or_insert(0) += count;
            }
            for warning in summary.warnings {
                if self.result.warnings.len() < MAX_LISTED_WARNINGS {
                    self.result.warnings.push(warning);
                } else {
                    self.suppressed_warnings += 1;
                }
            }
            if !summary.leap_seconds.is_empty() {
                self.result
                    .project
                    .metadata
                    .entry(String::from("leap seconds"))
                    .or_insert(summary.leap_seconds);
            }
        }
        let metadata = &mut self.result.project.metadata;
        if read.is_empty() {
            metadata.insert(
                String::from("navigation files"),
                format!(
                    "none beside the observation file (looked for {})",
                    candidates.join(", ")
                ),
            );
            return;
        }
        metadata.insert(String::from("navigation files"), read.join("; "));
        for (system, count) in ephemerides {
            metadata.insert(
                format!("broadcast ephemerides {system}"),
                format!("{count}"),
            );
        }
    }

    // A compact file's own account: how it was expanded, what could not be, and
    // its records counted as the lines of the file the person has.
    fn note_compaction(&mut self) {
        let Some(compact) = self.compact else {
            return;
        };
        self.result.project.metadata.insert(
            String::from("compression"),
            format!("{}; expanded to RINEX as it was read", compact.description),
        );
        // The expansion's own warnings come first: they say which epochs never
        // reached the reader at all. The two lists together keep to the one
        // limit; what is past it is counted in the closing warning.
        let mut warnings = compact.warnings.clone();
        warnings.append(&mut self.result.warnings);
        self.result.warnings = warnings;
        self.suppressed_warnings += compact.unlisted_warnings;
        if self.result.warnings.len() > MAX_LISTED_WARNINGS {
            self.suppressed_warnings += self.result.warnings.len() - MAX_LISTED_WARNINGS;
            self.result.warnings.truncate(MAX_LISTED_WARNINGS);
        }
        if compact.optional_lines > 0 {
            self.warn(
                0,
                format!(
                    "{} Compact RINEX optional records ('&' lines, reserved for future use) were \
                     skipped",
                    compact.optional_lines
                ),
            );
        }
        // Every expanded line comes from one compact line, and the clock offset
        // lines are read as part of their epoch, so the compact file's lines are
        // what the expansion skipped plus what the reader skipped, and the rest.
        // (A RINEX 2 satellite whose observations wrap is one compact line but
        // two expanded ones; the reader skipping it counts two - a count that can
        // only overstate the loss.)
        let skipped = compact
            .compact_lines
            .min(compact.lines_skipped + compact.optional_lines + self.result.records_skipped);
        self.result.records_skipped = skipped;
        self.result.records_read = compact.compact_lines - skipped;
    }

    fn read(mut self) -> Result<ReadResult> {
        let line = self.cursor.next().unwrap_or(b"");
        let Some(version) = version_record(line) else {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} is not a RINEX file: its first line is not a RINEX VERSION / TYPE record \
                     (label in columns 61-80)",
                    self.file_name
                ),
            ));
        };
        self.version = version;
        self.version_two = self.version.major == 2;
        let file_type = self.version.file_type;
        if file_type != b'O' {
            let navigation = matches!(file_type, b'N' | b'G' | b'H' | b'L')
                || (self.version_two && file_type == b'E');
            if navigation {
                return Err(Error::new(
                    ErrorCode::FileImportFailure,
                    format!(
                        "{} is a RINEX navigation file: it holds satellite orbits, not an \
                         occupation. Import the observation file (.rnx ending in O, or .YYo) and \
                         keep this one beside it.",
                        self.file_name
                    ),
                ));
            }
            if file_type == b'M' {
                return Err(Error::new(
                    ErrorCode::FileImportFailure,
                    format!(
                        "{} is a RINEX meteorological file: it holds pressure, temperature and \
                         humidity, not an occupation. Import the observation file.",
                        self.file_name
                    ),
                ));
            }
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} is a RINEX '{}' file, not an observation file",
                    self.file_name, self.version.type_text
                ),
            ));
        }
        if !(2..=4).contains(&self.version.major) {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!(
                    "{} is RINEX version {}; Katana reads RINEX observation files of versions 2, \
                     3 and 4",
                    self.file_name, self.version.version_text
                ),
            ));
        }
        self.result.records_read += 1;

        self.read_header()?;
        let any_types = if self.version_two {
            self.types_v2.declared > 0
        } else {
            self.types_v3.iter().any(|types| types.declared > 0)
        };
        if !any_types {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} declares no observation types ({}), so its observation records cannot be \
                     read",
                    self.file_name,
                    if self.version_two {
                        "# / TYPES OF OBSERV"
                    } else {
                        "SYS / # / OBS TYPES"
                    }
                ),
            ));
        }
        let mut miscounted: Vec<String> = Vec::new();
        let check_count = |types: &ObservationTypes, what: &str, out: &mut Vec<String>| {
            if types.declared > 0 && types.codes.len() != types.declared as usize {
                out.push(format!(
                    "{what} declares {} types but lists {}",
                    types.declared,
                    types.codes.len()
                ));
            }
        };
        if self.version_two {
            check_count(&self.types_v2, "# / TYPES OF OBSERV", &mut miscounted);
        } else {
            for (letter, types) in self.types_v3.iter().enumerate() {
                if types.declared > 0 {
                    let name = system_name(b'A' + letter as u8).unwrap_or_default();
                    check_count(
                        types,
                        &format!("SYS / # / OBS TYPES for {name}"),
                        &mut miscounted,
                    );
                }
            }
        }
        for message in miscounted {
            self.warn(0, message);
        }

        // The time system of every epoch (RINEX 3.05 section 5.2.8, 2.11 table
        // A1): stated in TIME OF FIRST OBS, compulsory in mixed files, otherwise
        // that of the file's one system. GLO is UTC.
        if self.time_system.is_empty() {
            let system = match self.version.system {
                b' ' | b'G' | b'S' => Some("GPS"),
                b'R' => Some("GLO"),
                b'E' => Some("GAL"),
                b'J' => Some("QZS"),
                b'C' => Some("BDT"),
                b'I' => Some("IRN"),
                _ => None,
            };
            match system {
                Some(system) => self.time_system = String::from(system),
                None => self.warn(
                    0,
                    String::from(
                        "the file mixes satellite systems but TIME OF FIRST OBS does not say \
                         which time system its epochs are in; they are given without one",
                    ),
                ),
            }
        }
        self.time_label = if self.time_system == "GLO" {
            String::from("UTC")
        } else {
            self.time_system.clone()
        };

        let start = if self.setup.marker_record != 0 {
            self.setup.marker_record
        } else {
            1
        };
        self.start_occupation(start);
        self.read_body();
        self.finish();
        self.read_navigation();
        if self.compact.is_some() {
            self.note_compaction();
        }
        if self.suppressed_warnings > 0 {
            self.result.warnings.push(ReadWarning {
                file_name: self.file_name.clone(),
                record: 0,
                message: format!(
                    "{} more warnings like these are not listed",
                    self.suppressed_warnings
                ),
            });
        }
        Ok(self.result)
    }
}

// ---- Registration ------------------------------------------------------------------

fn read_rinex_observation(
    bytes: &[u8],
    file_name: &str,
    options: &ReadOptions,
) -> Result<ReadResult> {
    if bytes.is_empty() {
        return Err(Error::new(
            ErrorCode::FileImportFailure,
            format!("{file_name} is empty"),
        ));
    }
    // A compact file is expanded first, and the reader then reads the
    // expansion, mapping its lines back to the compact file's own.
    let expansion = match packing_of(bytes) {
        Packing::Hatanaka => Some(expand_compact_rinex(
            bytes,
            file_name,
            MAX_SURVEY_FILE_BYTES,
        )?),
        Packing::Plain => None,
        packing => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                packing_refusal(packing, file_name),
            ));
        }
    };
    let text = expansion
        .as_ref()
        .map_or(bytes, |expansion| expansion.text.as_slice());
    ObservationReader::new(text, file_name, options, expansion.as_ref()).read()
}

// The name inside a packed file's name ("x.rnx.gz" -> "x.rnx"), or the name.
fn without_packing_extension(name: &str) -> &str {
    for suffix in [".gz", ".GZ", ".Z", ".z", ".bz2", ".BZ2", ".zip", ".ZIP"] {
        if name.len() > suffix.len()
            && let Some(inner) = name.strip_suffix(suffix)
        {
            return inner;
        }
    }
    name
}

// True for the names RINEX observation files are given: .rnx / .crx (long
// names, RINEX 3.05 table A1) and .YYo / .YYd (short names; d is Hatanaka).
fn observation_file_name(name: &str) -> bool {
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    let extension = lowered(&name.as_bytes()[dot + 1..]);
    if extension == b"rnx" || extension == b"crx" {
        return true;
    }
    extension.len() == 3
        && is_digit(extension[0])
        && is_digit(extension[1])
        && (extension[2] == b'o' || extension[2] == b'd')
}

fn signature(confidence: f64, evidence: String) -> FormatSignature {
    FormatSignature {
        confidence,
        evidence,
    }
}

fn probe_rinex_observation(input: &ProbeInput<'_>) -> FormatSignature {
    let bytes = input.bytes;
    match packing_of(bytes) {
        Packing::Gzip | Packing::UnixCompress | Packing::Bzip2 | Packing::Zip => {
            // Nothing inside a packed file can be read here, so only its name
            // can say it holds RINEX - and the reader then says to unpack it.
            let inner = without_packing_extension(&input.file_name);
            if inner.len() != input.file_name.len() && observation_file_name(inner) {
                return signature(
                    0.8,
                    format!(
                        "a compressed file named like a RINEX observation file ('{}'); it has to \
                         be decompressed first",
                        input.file_name
                    ),
                );
            }
            return ruled_out();
        }
        Packing::Hatanaka => {
            // The RINEX header it wraps starts on its third line.
            let mut evidence = String::from("Compact RINEX (Hatanaka) header 'CRINEX VERS / TYPE'");
            let mut lines = LineCursor::new(bytes);
            let mut line: &[u8] = b"";
            for _ in 0..3 {
                match lines.next() {
                    Some(next) => line = next,
                    None => break,
                }
            }
            if lines.line_number() == 3
                && let Some(inner) = version_record(line)
            {
                if inner.file_type != b'O' {
                    return signature(
                        0.75,
                        format!(
                            "{evidence} around a RINEX '{}' file, which Compact RINEX does not \
                             hold",
                            inner.type_text
                        ),
                    );
                }
                evidence.push_str(&format!(
                    " around RINEX {} observation data",
                    inner.version_text
                ));
            }
            return signature(0.99, format!("{evidence}; it is expanded as it is read"));
        }
        Packing::Plain => {}
    }
    let end = bytes.iter().position(|&c| c == b'\n');
    let mut first = &bytes[..end.unwrap_or(bytes.len())];
    if let Some(rest) = first.strip_suffix(b"\r") {
        first = rest;
    }
    let Some(version) = version_record(first) else {
        return ruled_out();
    };
    let described = format!(
        "first record RINEX VERSION / TYPE, version {}",
        version.version_text
    );
    if version.file_type != b'O' {
        return signature(
            0.75,
            format!(
                "{described}, type '{}' - RINEX, but not an observation file",
                version.type_text
            ),
        );
    }
    if !(2..=4).contains(&version.major) {
        return signature(
            0.75,
            format!("{described} (observation data) - a version this reader does not take"),
        );
    }
    let header_ends = contains(bytes, b"END OF HEADER");
    if !header_ends && !input.truncated {
        return signature(
            0.8,
            format!("{described} (observation data), but no END OF HEADER"),
        );
    }
    signature(
        0.99,
        format!(
            "{described} (observation data){}",
            if observation_file_name(&input.file_name) {
                ", RINEX file name"
            } else {
                ""
            }
        ),
    )
}

fn rinex_descriptor() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(RINEX_OBSERVATION_FORMAT_ID),
        human_name: String::from(HUMAN_NAME),
        manufacturer: Manufacturer::OpenStandard,
        // GNSS sessions, their markers and an approximate position: no surveyed
        // points, no total-station observations, no coded strings.
        reads: FormatContent {
            gnss: true,
            ..Default::default()
        },
        can_import: true,
        can_export: false,
        parser_version: String::from(PARSER_VERSION),
        // Short names (.24o) cannot be listed as extensions; detection reads the
        // content, so they are recognised all the same.
        extensions: vec![
            String::from("rnx"),
            String::from("obs"),
            String::from("crx"),
        ],
    }
}

pub(crate) fn registration() -> FormatRegistration {
    FormatRegistration::new(
        rinex_descriptor(),
        probe_rinex_observation,
        Some(read_rinex_observation),
    )
}
