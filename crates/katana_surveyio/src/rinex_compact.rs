//! Compact RINEX (Hatanaka compression) expanded back to the RINEX observation
//! file it stands for, so that the observation reader (rinex.rs) reads it
//! exactly as it reads a plain file and every check it makes still applies.

// Specification followed: Y. Hatanaka, "A Compression Format and Tools for GNSS
// Observation Data", Bulletin of the Geographical Survey Institute, vol. 55,
// March 2008 - section 2 (the rules, and where Compact RINEX 1.0, for RINEX 2,
// differs from 3.0, for RINEX 3 and 4), appendix 1 (the recovering operation,
// equation 3), appendix 2 tables A-1 to A-3 (the records) and appendix 3 (an
// example, which the tests read).
//
// What a compact file holds:
//   * CRINEX VERS / TYPE and CRINEX PROG / DATE, then the RINEX header as it
//     was;
//   * per epoch, an epoch line: the RINEX epoch record's first 41 columns
//     (3.0) or 32 (1.0) followed by the whole satellite list on the one line,
//     TEXT-DIFFERENCED against the previous epoch line - a blank keeps the old
//     character, '&' makes it a blank, anything else replaces it. A line
//     starting '>' (3.0) or '&' (1.0) is given in full instead;
//   * the receiver clock offset on a line of its own (empty when the epoch has
//     none), as an integer in 1e-12 s (3.0) or 1e-9 s (1.0);
//   * one line per satellite: a field per observation type, separated by
//     single blanks - "M&v" starts a series of differencing order M at the
//     integer v, a bare integer is the series' next M-th difference, an empty
//     field is a blank observation - then the LLI and signal-strength flags of
//     every type as one text-differenced string. Observations are integers in
//     thousandths (F14.3 without its point), recovered by equation 3:
//     Y(m-1)_i = Y(m-1)_(i-1) + Y(m)_i, and so on down to Y(0)_i;
//   * an epoch with event flag 2 to 6 as it is, its special records after it
//     unchanged, and every series starting afresh at the next epoch (table 2).
// Where 1.0 differs (section 2.1): a blank observation blanks its LLI and
// signal-strength flags, on both sides and with no '&' written for it; in 3.0
// the flags stand apart from their observation.
//
// Where the paper leaves a detail open, the published RNX2CRX / CRX2RNX
// ver.4.1.0 (by the same author) decide it, since every compact file in
// circulation was written by them:
//   * a series restarting ("M&v", for a new arc or a jump of more than 10^7)
//     leaves its flags alone - the compressor differences them against the old
//     ones;
//   * an epoch line given in full makes every satellite new: its series all
//     start afresh and its flags are given whole, a blank in them a blank;
//   * a satellite is known by its three characters as written, so ' 01' and
//     'G01' in a RINEX 2 file are two satellites.
//
// Why expand to text rather than count straight from the compact records: the
// observation reader's record checks, event handling and warnings then apply
// to a compact file unchanged, and what the two paths report cannot drift
// apart. The price is holding the expanded text while it is read - about three
// times the compact file - which the size cap bounds.
//
// A compact file is differential: once a line is damaged, nothing after it can
// be recovered until every series starts afresh (section 2.4). Damage is
// therefore reported once, with the lines it spoils, and expansion resumes at
// the next epoch that stands on its own.

use katana_core::text::trimmed;
use katana_core::{Error, ErrorCode, Result, Status};

use crate::format::ReadWarning;
use crate::rinex_internal::{
    CompactExpansion, LineCursor, MAX_LISTED_WARNINGS, clean_text, columns, header_label,
    integer_field, is_digit, raw_text, version_record,
};

// Table A-2 allows orders up to 9; the published compressor writes 3.
const MAX_ORDER: usize = 9;
// Satellite slots, one per satellite as written: a system letter or (RINEX 2)
// a blank, a tens digit or a blank, and a units digit.
const SATELLITE_SLOTS: usize = 27 * 11 * 10;

// The slot of a satellite already checked to be a letter or blank, a digit or
// blank, and a digit. By the characters as written rather than the satellite
// they mean: the compressor matches a satellite against the last epoch's list
// by its three characters, so to it ' 01' after 'G01' is a new satellite, whose
// series start afresh and whose flags are given whole.
fn satellite_slot(id: &[u8; 3]) -> usize {
    let system = if id[0] == b' ' {
        26
    } else {
        usize::from(id[0] - b'A')
    };
    let tens = if id[1] == b' ' {
        10
    } else {
        usize::from(id[1] - b'0')
    };
    (system * 11 + tens) * 10 + usize::from(id[2] - b'0')
}

// More observation types than any receiver records (RINEX 3.05 has about 150
// codes across all systems); a bound so a damaged header cannot ask for
// gigabytes of series.
const MOST_OBSERVATION_TYPES: i32 = 200;

fn trimmed_right(text: &[u8]) -> &[u8] {
    let mut end = text.len();
    while end > 0 && text[end - 1] == b' ' {
        end -= 1;
    }
    &text[..end]
}

// Section 2.2: a blank keeps the old character, '&' makes it a blank, anything
// else replaces it; a difference longer than the old text extends it.
fn apply_text_difference(text: &mut Vec<u8>, difference: &[u8]) {
    let common = text.len().min(difference.len());
    for (old, &c) in text.iter_mut().zip(&difference[..common]) {
        if c != b' ' {
            *old = if c == b'&' { b' ' } else { c };
        }
    }
    for &c in &difference[common..] {
        text.push(if c == b'&' { b' ' } else { c });
    }
}

// An integer as a Fortran F field of the field's width with `decimals` places,
// right aligned: 1234567 with 3 decimals is "1234.567", -353 is "-.353" -
// without the zero before the point, as Fortran writes it and as the published
// CRX2RNX restores it, so an expansion matches that tool's byte for byte. It is
// written into `field`, which holds blanks, digits laid down from the right, so
// no text is built and copied. False when it does not fit, which a value that
// came out of an F14.3 field never does. Digits are written two at a time: the
// chain of divisions is most of what writing a value costs, and pairs halve it.
const DIGIT_PAIRS: [u8; 200] = {
    let mut pairs = [0u8; 200];
    let mut i = 0;
    while i < 100 {
        pairs[2 * i] = b'0' + (i / 10) as u8;
        pairs[2 * i + 1] = b'0' + (i % 10) as u8;
        i += 1;
    }
    pairs
};

fn write_pair(field: &mut [u8], at: &mut usize, magnitude: &mut u64) {
    let pair = (*magnitude % 100) as usize;
    *magnitude /= 100;
    *at -= 2;
    field[*at] = DIGIT_PAIRS[2 * pair];
    field[*at + 1] = DIGIT_PAIRS[2 * pair + 1];
}

fn write_fixed(field: &mut [u8], value: i64, decimals: i32) -> bool {
    let negative = value < 0;
    let mut magnitude = if negative {
        0u64.wrapping_sub(value as u64)
    } else {
        value as u64
    };
    let mut at = field.len();
    let mut left = decimals;
    while left >= 2 {
        write_pair(field, &mut at, &mut magnitude);
        left -= 2;
    }
    if left == 1 {
        at -= 1;
        field[at] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
    }
    at -= 1;
    field[at] = b'.';
    while magnitude >= 10 {
        if at < 2 {
            return false;
        }
        write_pair(field, &mut at, &mut magnitude);
    }
    if magnitude != 0 {
        if at == 0 {
            return false;
        }
        at -= 1;
        field[at] = b'0' + magnitude as u8;
    }
    if negative {
        if at == 0 {
            return false;
        }
        at -= 1;
        field[at] = b'-';
    }
    true
}

// One numeric data series (table 1, categories B and C): the state vector of
// appendix 1 - the last value and its differences up to the series' order.
#[derive(Clone, Copy, Debug, Default)]
struct Series {
    state: [i64; MAX_ORDER + 1],
    order: i32,
    /// Values since the series started, saturating at `order`.
    values: i32,
    active: bool,
}

impl Series {
    fn start(&mut self, maximum_order: i32, value: i64) {
        self.state[0] = value;
        self.order = maximum_order;
        self.values = 1;
        self.active = true;
    }

    // Equation 3: this epoch's M-th difference back to its value. False on an
    // overflow, which only a damaged file produces; the sum is kept wrapped, as
    // the C++'s __builtin_add_overflow keeps it.
    fn advance(&mut self, difference: i64) -> bool {
        let m = self.values.min(self.order) as usize;
        self.state[m] = difference;
        for j in (0..m).rev() {
            let (sum, overflow) = self.state[j].overflowing_add(self.state[j + 1]);
            self.state[j] = sum;
            if overflow {
                return false;
            }
        }
        if self.values < self.order {
            self.values += 1;
        }
        true
    }
}

// A numeric field read: blank, a start ("M&v") or a difference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum NumericKind {
    #[default]
    Blank,
    Start,
    Difference,
    Unreadable,
}

#[derive(Clone, Copy, Debug, Default)]
struct NumericField {
    kind: NumericKind,
    order: i32,
    value: i64,
}

// std::from_chars for a 64-bit integer, the whole text: an optional '-' and
// digits, nothing else ('+' is not one of its forms), in range.
fn from_chars_i64(text: &[u8]) -> Option<i64> {
    if text.first() == Some(&b'+') {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse().ok()
}

fn read_numeric_field(field: &[u8]) -> NumericField {
    let mut read = NumericField::default();
    if field.is_empty() {
        return read;
    }
    let mut number = field;
    if field.len() >= 2 && field[1] == b'&' {
        if !is_digit(field[0]) {
            read.kind = NumericKind::Unreadable;
            return read;
        }
        read.order = i32::from(field[0] - b'0');
        number = &field[2..];
        read.kind = NumericKind::Start;
    } else {
        read.kind = NumericKind::Difference;
    }
    match from_chars_i64(number) {
        Some(value) => read.value = value,
        None => read.kind = NumericKind::Unreadable,
    }
    read
}

const CONTINUED: u8 = 0;
const STARTED: u8 = 1;
const BLANK: u8 = 2;

#[derive(Clone, Debug, Default)]
struct SatelliteState {
    /// The epoch it was last expanded in; 0 = never.
    last_epoch: u64,
    series: Vec<Series>,
    /// LLI and signal strength, two characters per type.
    flags: Vec<u8>,
}

// The line-map runs of the epoch being expanded, added to the map only once it
// has expanded whole (its text is cut off again if it does not).
#[derive(Clone, Copy, Debug)]
struct PendingRun {
    expanded: usize,
    compact: usize,
    per_compact: usize,
}

struct Expander<'a> {
    bytes_len: usize,
    file_name: String,
    max_bytes: usize,
    cursor: LineCursor<'a>,
    out: CompactExpansion,

    /// Compact RINEX 1.0: RINEX 2.
    version_one: bool,
    /// 0-based start of the satellite list in an epoch line.
    list_column: usize,
    types_v2: i32,
    types_v3: [i32; 26],

    /// The last epoch line, expanded: what differences apply to.
    epoch_line: Vec<u8>,
    have_epoch_line: bool,
    clock: Series,
    epoch: u64,
    satellites: Vec<SatelliteState>,

    /// Where the epoch being expanded starts in the text.
    epoch_start: usize,
    pending_runs: Vec<PendingRun>,
    pending_lines: usize,
    /// Lines committed.
    expanded: usize,
    /// One satellite's RINEX record, reused.
    satellite_line: Vec<u8>,
    /// An epoch record line, reused.
    epoch_text: Vec<u8>,
    // What each of a satellite's fields was this epoch: CONTINUED, STARTED or
    // BLANK.
    field_kinds: Vec<u8>,

    why: String,
    failed_line: usize,
    /// First line of a damaged stretch, 0 = none.
    damaged_from: usize,
    /// The line the damage was found at.
    damage_line: usize,
    damage_why: String,
}

impl<'a> Expander<'a> {
    fn new(bytes: &'a [u8], file_name: &str, max_bytes: usize) -> Self {
        Self {
            bytes_len: bytes.len(),
            file_name: String::from(file_name),
            max_bytes,
            cursor: LineCursor::new(bytes),
            out: CompactExpansion::default(),
            version_one: false,
            list_column: 0,
            types_v2: 0,
            types_v3: [0; 26],
            epoch_line: Vec::new(),
            have_epoch_line: false,
            clock: Series::default(),
            epoch: 1,
            satellites: vec![SatelliteState::default(); SATELLITE_SLOTS],
            epoch_start: 0,
            pending_runs: Vec::new(),
            pending_lines: 0,
            expanded: 0,
            satellite_line: Vec::new(),
            epoch_text: Vec::new(),
            field_kinds: Vec::new(),
            why: String::new(),
            failed_line: 0,
            damaged_from: 0,
            damage_line: 0,
            damage_why: String::new(),
        }
    }

    // Why the epoch being expanded cannot be, found at the line last read.
    fn fail(&mut self, why: String) -> bool {
        self.why = why;
        self.failed_line = self.cursor.line_number();
        false
    }

    fn restart_all_series(&mut self) {
        // Every satellite is then "not in the previous epoch", which table 2
        // makes a new series; and the clock starts afresh.
        self.epoch += 2;
        self.clock.active = false;
    }

    // An expanded line, written straight into the expansion; an epoch that
    // then fails is cut off again (discard), so only whole epochs remain.
    fn emit_line(&mut self, line: &[u8], compact_record: usize, per_compact: usize) {
        self.pending_lines += 1;
        self.pending_runs.push(PendingRun {
            expanded: self.expanded + self.pending_lines,
            compact: compact_record,
            per_compact,
        });
        self.out.text.extend_from_slice(trimmed_right(line));
        self.out.text.push(b'\n');
    }

    fn commit(&mut self) {
        for run in &self.pending_runs {
            self.out
                .lines
                .add(run.expanded, run.compact, run.per_compact);
        }
        self.expanded += self.pending_lines;
        self.pending_runs.clear();
        self.pending_lines = 0;
        self.epoch_start = self.out.text.len();
    }

    fn discard(&mut self) {
        self.out.text.truncate(self.epoch_start);
        self.pending_runs.clear();
        self.pending_lines = 0;
    }

    fn read_header(&mut self) -> Status {
        let line = self.cursor.next().unwrap_or(b"");
        let crx_version = trimmed(columns(line, 1, 20));
        if header_label(line) != b"CRINEX VERS   / TYPE"
            && header_label(line) != b"CRINEX VERS / TYPE"
        {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} is not Compact RINEX: its first line is not CRINEX VERS / TYPE",
                    self.file_name
                ),
            ));
        }
        if crx_version != b"1.0" && crx_version != b"3.0" {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!(
                    "{} is Compact RINEX version {}; Katana expands versions 1.0 (RINEX 2) and 3.0 \
                     (RINEX 3 and 4). Convert it with CRX2RNX and import the RINEX file it writes.",
                    self.file_name,
                    raw_text(crx_version)
                ),
            ));
        }
        self.version_one = crx_version == b"1.0";
        self.list_column = if self.version_one { 32 } else { 41 };
        let crx = raw_text(crx_version);
        self.out.description = format!("Compact RINEX {crx}");

        let line = match self.cursor.next() {
            Some(line) if header_label(line) == b"CRINEX PROG / DATE" => line,
            _ => {
                return Err(Error::new(
                    ErrorCode::FileImportFailure,
                    format!(
                        "{} is not whole Compact RINEX: its second line is not CRINEX PROG / DATE",
                        self.file_name
                    ),
                ));
            }
        };
        let program = clean_text(columns(line, 1, 40), None);
        let written = clean_text(columns(line, 41, 20), None);
        if !program.is_empty() {
            self.out.description.push_str(", written by ");
            self.out.description.push_str(&program);
        }
        if !written.is_empty() {
            self.out.description.push_str(" on ");
            self.out.description.push_str(&written);
        }

        let Some(mut line) = self.cursor.next() else {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} ends after its Compact RINEX header, before the RINEX header it should hold",
                    self.file_name
                ),
            ));
        };
        let Some(version) = version_record(line) else {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} is Compact RINEX, but its third line is not the RINEX VERSION / TYPE record \
                     of the file it compresses",
                    self.file_name
                ),
            ));
        };
        let matches = if self.version_one {
            version.major == 2
        } else {
            version.major == 3 || version.major == 4
        };
        if !matches {
            return Err(Error::new(
                ErrorCode::FileImportFailure,
                format!(
                    "{} is Compact RINEX {crx}, which holds RINEX {}, but the file it compresses \
                     says RINEX {}",
                    self.file_name,
                    if self.version_one { "2" } else { "3 and 4" },
                    version.version_text
                ),
            ));
        }
        // The observation reader reads the RINEX header from here: line 3 of the
        // compact file is line 1 of the expansion.
        self.out.lines.add(1, 3, 1);
        let mut lines = 0;
        loop {
            lines += 1;
            self.out.text.extend_from_slice(trimmed_right(line));
            self.out.text.push(b'\n');
            self.note_observation_types(line);
            if header_label(line) == b"END OF HEADER" {
                self.expanded = lines;
                self.epoch_start = self.out.text.len();
                return Ok(());
            }
            match self.cursor.next() {
                Some(next) => line = next,
                None => break,
            }
        }
        Err(Error::new(
            ErrorCode::FileImportFailure,
            format!(
                "{} is Compact RINEX, but the RINEX header inside it has no END OF HEADER",
                self.file_name
            ),
        ))
    }

    // How many observation types each system has: a satellite line has one field
    // per type. Read from the header and from event records that redefine them.
    fn note_observation_types(&mut self, line: &[u8]) {
        let label = header_label(line);
        let most = i64::from(MOST_OBSERVATION_TYPES) + 1;
        if self.version_one && label == b"# / TYPES OF OBSERV" {
            if let Some(count) = integer_field(columns(line, 1, 6)) {
                self.types_v2 = count.clamp(0, most) as i32;
            }
        } else if !self.version_one
            && label == b"SYS / # / OBS TYPES"
            && !line.is_empty()
            && line[0] != b' '
        {
            let system = line[0];
            let count = integer_field(columns(line, 4, 3));
            if system.is_ascii_uppercase()
                && let Some(count) = count
            {
                self.types_v3[usize::from(system - b'A')] = count.clamp(0, most) as i32;
            }
        }
    }

    fn append_epoch_line(&mut self, clock: Option<i64>, count: i32, record: usize) {
        let mut line = std::mem::take(&mut self.epoch_text);
        if !self.version_one {
            // RINEX 3 (table A3): columns 1-35, six reserved, the clock in 42-56.
            line.clear();
            line.extend_from_slice(&self.epoch_line[..self.epoch_line.len().min(41)]);
            if let Some(clock) = clock {
                line.resize(41 + 15, b' ');
                write_fixed(&mut line[41..56], clock, 12);
            }
            self.emit_line(&line, record, 1);
            self.epoch_text = line;
            return;
        }
        // RINEX 2 (table A2): columns 1-32, twelve satellites per line (33-68),
        // the clock in 69-80 of the first line, continuation lines of 32 blanks.
        let count = count as usize;
        let lines = if count > 12 { count.div_ceil(12) } else { 1 };
        for i in 0..lines {
            line.clear();
            if i == 0 {
                line.extend_from_slice(&self.epoch_line[..self.epoch_line.len().min(32)]);
            }
            line.resize(32, b' ');
            let first = i * 36;
            let left = count * 3 - first;
            let list = &self.epoch_line[self.list_column..];
            line.extend_from_slice(&list[first..first + left.min(36)]);
            if i == 0
                && let Some(clock) = clock
            {
                line.resize(68 + 12, b' ');
                write_fixed(&mut line[68..80], clock, 9);
            }
            // Every line of the epoch record comes from the one compact line.
            self.emit_line(&line, record, 0);
        }
        self.epoch_text = line;
    }

    fn expand_event(&mut self, flag: i32, count: i32, record: usize) -> bool {
        // Events (flags 2-6) are written as they are (table A-2); a RINEX 2 cycle
        // slip record (flag 6) keeps its satellite list on the epoch line, and
        // each of its satellites one line per five types, as in the RINEX file.
        if flag == 6 && self.version_one {
            if self.epoch_line.len() < self.list_column + count as usize * 3 {
                return self.fail(format!(
                    "the cycle slip record lists {count} satellites but its satellite list is \
                     shorter"
                ));
            }
            self.append_epoch_line(None, count, record);
        } else {
            let line = std::mem::take(&mut self.epoch_line);
            self.emit_line(&line[..line.len().min(self.list_column)], record, 1);
            self.epoch_line = line;
        }
        let per_satellite = if self.version_one && flag == 6 {
            ((self.types_v2 + 4) / 5).max(1)
        } else {
            1
        };
        let records = i64::from(count) * i64::from(per_satellite);
        for _ in 0..records {
            let Some(line) = self.cursor.next() else {
                return self.fail(format!(
                    "the file ends before the {records} records this event (flag {flag}) promises"
                ));
            };
            let number = self.cursor.line_number();
            self.emit_line(line, number, 1);
            self.note_observation_types(line);
        }
        self.restart_all_series();
        true
    }

    fn is_full_epoch(&self, line: &[u8]) -> bool {
        line.first() == Some(if self.version_one { &b'&' } else { &b'>' })
    }

    fn expand_epoch(&mut self, line: &[u8], record: usize) -> bool {
        // The epoch line: given in full, or as its difference from the last one.
        if self.is_full_epoch(line) {
            self.epoch_line.clear();
            self.epoch_line.extend_from_slice(line);
            if self.version_one {
                self.epoch_line[0] = b' ';
            }
            self.have_epoch_line = true;
            // Every satellite is new after a line given in full, as CRX2RNX
            // takes it: the compressor writes one only where it started every
            // series afresh (the first epoch, after an event, or every n epochs
            // with "-e n"), and gives each satellite's flags whole there - a
            // blank in them is a blank, not "unchanged".
            self.epoch += 2;
        } else if !self.have_epoch_line {
            return self.fail(String::from(
                "an epoch line given only as its difference from an earlier one, with no earlier \
                 one to apply it to",
            ));
        } else {
            apply_text_difference(&mut self.epoch_line, line);
        }

        let flag_at = if self.version_one { 28 } else { 31 };
        if self.epoch_line.len() <= flag_at || !is_digit(self.epoch_line[flag_at]) {
            return self.fail(format!(
                "its epoch line has no event flag where one should be (column {})",
                flag_at + 1
            ));
        }
        let flag = i32::from(self.epoch_line[flag_at] - b'0');
        let counted =
            integer_field(&self.epoch_line[flag_at + 1..self.epoch_line.len().min(flag_at + 4)]);
        let Some(counted) = counted.filter(|&counted| counted >= 0) else {
            return self.fail(String::from(
                "its epoch line has no satellite or record count after the event flag",
            ));
        };
        let count = counted.min(999) as i32;
        if flag > 6 {
            return self.fail(format!(
                "event flag {flag} is not one RINEX defines (0 to 6)"
            ));
        }
        if flag >= 2 {
            return self.expand_event(flag, count, record);
        }

        // An epoch line given in full loses its trailing blanks, so one listing no
        // satellites stops at the count (35 columns in 3.0) short of where a list
        // would start. The compressor pads it to that column before differencing
        // the next epoch against it, so the padding is what the next line's
        // difference applies to.
        if self.epoch_line.len() < self.list_column {
            self.epoch_line.resize(self.list_column, b' ');
        }
        if self.epoch_line.len() < self.list_column + count as usize * 3 {
            return self.fail(format!(
                "the epoch lists {count} satellites but its satellite list is shorter"
            ));
        }

        // The clock offset line (table A-2): empty, "M&v" or a difference.
        let Some(clock_line) = self.cursor.next() else {
            return self.fail(String::from(
                "the file ends after an epoch line, before its clock offset line",
            ));
        };
        let offset = read_numeric_field(trimmed_right(clock_line));
        match offset.kind {
            NumericKind::Blank => self.clock.active = false,
            NumericKind::Start => self.clock.start(offset.order, offset.value),
            NumericKind::Difference => {
                if !self.clock.active {
                    return self.fail(String::from(
                        "a receiver clock offset given as a difference, with no earlier value to \
                         apply it to",
                    ));
                }
                if !self.clock.advance(offset.value) {
                    return self.fail(String::from(
                        "the receiver clock offset cannot be recovered: its differences overflow",
                    ));
                }
            }
            NumericKind::Unreadable => {
                return self.fail(String::from(
                    "the receiver clock offset line is not a number",
                ));
            }
        }
        self.epoch += 1;
        let clock = if self.clock.active {
            Some(self.clock.state[0])
        } else {
            None
        };
        self.append_epoch_line(clock, count, record);

        for i in 0..count {
            let mark = self.cursor.mark();
            let Some(satellite_line) = self.cursor.next() else {
                return self.fail(format!(
                    "the file ends inside the epoch: {} of its {count} satellite lines are missing",
                    count - i
                ));
            };
            if self.is_full_epoch(satellite_line) {
                self.cursor.reset(mark);
                return self.fail(format!(
                    "the epoch lists {count} satellites but only {i} lines follow it"
                ));
            }
            let at = self.list_column + i as usize * 3;
            let id = [
                self.epoch_line[at],
                self.epoch_line[at + 1],
                self.epoch_line[at + 2],
            ];
            let number = self.cursor.line_number();
            if !self.expand_satellite(id, satellite_line, number) {
                return false;
            }
        }
        true
    }

    fn expand_satellite(&mut self, id: [u8; 3], text: &[u8], record: usize) -> bool {
        let line = trimmed_right(text);
        // RINEX 2 lets a blank system letter mean GPS (2.11 section 5.1).
        let system = if id[0] == b' ' && self.version_one {
            b'G'
        } else {
            id[0]
        };
        let shown = raw_text(&id);
        if !system.is_ascii_uppercase() || !(is_digit(id[1]) || id[1] == b' ') || !is_digit(id[2]) {
            return self.fail(format!(
                "the satellite list holds '{shown}', which is not a satellite (a system letter and \
                 a number)"
            ));
        }
        let types = if self.version_one {
            self.types_v2
        } else {
            self.types_v3[usize::from(system - b'A')]
        };
        if types <= 0 || types > MOST_OBSERVATION_TYPES {
            return self.fail(format!(
                "satellite {shown}: the header declares {} observation types for its system, so \
                 its line cannot be expanded",
                if types <= 0 { "no" } else { "too many" }
            ));
        }
        let slot = satellite_slot(&id);
        if self.satellites[slot].last_epoch == self.epoch {
            return self.fail(format!("satellite {shown} appears twice in one epoch"));
        }
        let type_count = types as usize;
        let epoch = self.epoch;
        let satellite = &mut self.satellites[slot];
        if satellite.last_epoch + 1 != epoch || satellite.series.len() != type_count {
            // Not in the previous epoch: every series starts afresh (table 2).
            satellite.series.clear();
            satellite.series.resize(type_count, Series::default());
            satellite.flags.clear();
            satellite.flags.resize(type_count * 2, b' ');
        }
        satellite.last_epoch = epoch;

        // The numeric fields: the first `types` blanks separate them (table A-2,
        // note 3); what follows the last is the flags' difference text.
        // The RINEX record is laid out blank - F14.3, I1, I1 per type, after the
        // satellite in RINEX 3 - and each value written into its place.
        self.field_kinds.clear();
        self.field_kinds.resize(type_count, CONTINUED);
        let prefix = if self.version_one { 0 } else { 3 };
        self.satellite_line.clear();
        self.satellite_line.resize(prefix + type_count * 16, b' ');
        if !self.version_one {
            self.satellite_line[..3].copy_from_slice(&id);
        }
        let mut at = 0;
        for k in 0..type_count {
            let field: &[u8] = if at < line.len() {
                let stop = line[at..]
                    .iter()
                    .position(|&c| c == b' ')
                    .map_or(line.len(), |offset| at + offset);
                let field = &line[at..stop];
                at = stop + 1;
                field
            } else {
                at = line.len() + 1;
                &[]
            };
            let read = read_numeric_field(field);
            let series = &mut self.satellites[slot].series[k];
            match read.kind {
                NumericKind::Blank => {
                    series.active = false;
                    self.field_kinds[k] = BLANK;
                    continue;
                }
                NumericKind::Start => {
                    series.start(read.order, read.value);
                    self.field_kinds[k] = STARTED;
                }
                NumericKind::Difference => {
                    if !series.active {
                        return self.fail(format!(
                            "satellite {shown}, observation {}: a difference with no earlier \
                             value to apply it to",
                            k + 1
                        ));
                    }
                    if !series.advance(read.value) {
                        return self.fail(format!(
                            "satellite {shown}, observation {}: its differences overflow",
                            k + 1
                        ));
                    }
                }
                NumericKind::Unreadable => {
                    return self.fail(format!(
                        "satellite {shown}, observation {}: '{}' is not a number",
                        k + 1,
                        raw_text(field)
                    ));
                }
            }
            let value = self.satellites[slot].series[k].state[0];
            let place = prefix + k * 16;
            if !write_fixed(&mut self.satellite_line[place..place + 14], value, 3) {
                return self.fail(format!(
                    "satellite {shown}, observation {}: the recovered value does not fit an F14.3 \
                     field",
                    k + 1
                ));
            }
        }

        // The flags: this epoch's difference applied to the last epoch's, whether
        // or not a series restarted (a restart does not touch them).
        let flags: &[u8] = if at < line.len() { &line[at..] } else { &[] };
        if flags.len() > type_count * 2 {
            return self.fail(format!(
                "satellite {shown}: more flag characters than its {type_count} observation types \
                 have"
            ));
        }
        let satellite = &mut self.satellites[slot];
        apply_text_difference(&mut satellite.flags, flags);

        for k in 0..type_count {
            // 1.0 cannot carry flags without their observation (section 2.1): a
            // blank observation has blank flags, and they stay blank for the next
            // epoch's difference - the compressor blanks its own copy the same way.
            // Blanked after the difference is applied, as CRX2RNX does.
            if self.version_one && self.field_kinds[k] == BLANK {
                satellite.flags[2 * k] = b' ';
                satellite.flags[2 * k + 1] = b' ';
                continue;
            }
            let place = prefix + k * 16;
            self.satellite_line[place + 14] = satellite.flags[2 * k];
            self.satellite_line[place + 15] = satellite.flags[2 * k + 1];
        }
        let whole = std::mem::take(&mut self.satellite_line);
        if !self.version_one {
            self.emit_line(&whole, record, 1);
        } else {
            // RINEX 2: five types (80 columns) per line.
            let lines = type_count.div_ceil(5);
            for l in 0..lines {
                let start = l * 80;
                self.emit_line(&whole[start..whole.len().min(start + 80)], record, lines);
            }
        }
        self.satellite_line = whole;
        true
    }

    fn report_damage(&mut self, last_record: usize) {
        if self.damaged_from == 0 {
            return;
        }
        let lines = if last_record >= self.damaged_from {
            last_record - self.damaged_from + 1
        } else {
            1
        };
        self.out.lines_skipped += lines;
        if self.out.warnings.len() >= MAX_LISTED_WARNINGS {
            self.out.unlisted_warnings += 1;
        } else {
            let mut message = format!(
                "the compact data cannot be expanded here: {}. ",
                self.damage_why
            );
            if lines == 1 {
                message.push_str(&format!("Line {} is left out", self.damaged_from));
            } else {
                message.push_str(&format!(
                    "Lines {} to {} are left out (Compact RINEX is differential: nothing after \
                     damage can be recovered until every series starts afresh)",
                    self.damaged_from,
                    self.damaged_from + lines - 1
                ));
            }
            self.out.warnings.push(ReadWarning {
                file_name: self.file_name.clone(),
                record: self.damage_line,
                message,
            });
        }
        self.damaged_from = 0;
        self.damage_line = 0;
        self.damage_why.clear();
    }

    fn run(mut self) -> Result<CompactExpansion> {
        self.read_header()?;
        // Compact RINEX is typically a third to a sixth of the RINEX it holds.
        // The C++ reports an allocation that fails; Rust would abort, so a
        // reservation that cannot be made is not made.
        let _ = self
            .out
            .text
            .try_reserve(self.max_bytes.min(self.bytes_len.saturating_mul(4)));
        while let Some(line) = self.cursor.next() {
            let record = self.cursor.line_number();
            if line.is_empty() && self.cursor.at_end() {
                continue; // an editor's last blank line
            }
            let full = self.is_full_epoch(line);
            if self.damaged_from != 0 && !full {
                continue; // still inside a damaged stretch: only a full epoch can end it
            }
            if !self.version_one && line.first() == Some(&b'&') {
                // Table A-3: an optional record, reserved for future use; readers
                // are to skip it, and it does not restart anything.
                self.out.optional_lines += 1;
                continue;
            }
            if self.expand_epoch(line, record) {
                self.report_damage(record - 1);
                self.commit();
                if self.out.text.len() > self.max_bytes {
                    return Err(Error::new(
                        ErrorCode::FileImportFailure,
                        format!(
                            "{} expands to more than {} MB of RINEX, more than Katana reads from \
                             one file",
                            self.file_name,
                            self.max_bytes >> 20
                        ),
                    ));
                }
                continue;
            }
            self.discard();
            self.have_epoch_line = false;
            self.restart_all_series();
            if self.damaged_from == 0 {
                self.damaged_from = record;
                self.damage_line = record.max(self.failed_line);
                self.damage_why = std::mem::take(&mut self.why);
            }
        }
        let last = self.cursor.line_number();
        self.report_damage(last);
        self.out.compact_lines = last;
        Ok(self.out)
    }
}

/// Expands `bytes`, which packing_of() found to be Compact RINEX. An error
/// (naming the file) when the compact header itself cannot be read or its
/// version is one this expansion does not know; damage in the body is a
/// warning and the epochs it spoils are left out, the rest kept. Refuses to
/// expand beyond `max_expanded_bytes`.
pub fn expand_compact_rinex(
    bytes: &[u8],
    file_name: &str,
    max_expanded_bytes: usize,
) -> Result<CompactExpansion> {
    Expander::new(bytes, file_name, max_expanded_bytes).run()
}
