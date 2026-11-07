//! The apply's journal: R/.update/<tx>/journal, append-only text, one record per line, written so
//! that a crash at any byte leaves something the recovery can read.
//!
//! ```text
//! <n> <OP> <arg>... <crc>\n
//!
//! n    1, 2, 3 ... the record's place in the journal
//! OP   an operation name below
//! arg  zero or more arguments, each percent-encoded: every byte outside
//!      [A-Za-z0-9._/~-] is %XX (upper-case hex), so an argument never
//!      contains a space or a line break
//! crc  CRC-32 (IEEE 802.3, the zlib crc32) of the line's bytes before the
//!      space that precedes it, as 8 lower-case hex digits
//! ```
//!
//! A record is complete only with its LF. Parsing stops at the first line that is not complete,
//! whose crc is wrong, or whose n is not the previous one plus one: the journal is append-only, so
//! a bad line can only be the last (a torn write) and everything from it on is ignored
//! (`torn_tail`).
//!
//! Records are appended in GROUPS with one flush to stable storage per group
//! (FsOps::append_durable), so the PLAN, every operation the transaction will do, is on disk
//! before the first file moves.
//!
//! ```text
//! PLAN <tx> <fromSequence> <toSequence>   header; first record
//! ADD <path>          a file the new version adds (absent before): rollback deletes it
//! REPLACE <path>      a file present with other bytes: the old one moves to old/<path>
//! REMOVE <path>       a file of the old manifest, absent from the new, whose disk
//!                     hash equals the old one's: it moves to old/<path>
//! MKDIR <path>        a directory this transaction creates: rollback removes it when empty
//! COMMITTING          the renames begin; from here the tree is mid-swap
//! MOVED_OLD <path>    informational progress (R/<path> moved to old/<path>)
//! MOVED_NEW <path>    informational progress (new/<path> moved to R/<path>)
//! COMMITTED_PENDING_HEALTH   every file is in place; the new version has not yet proved it starts
//! ROLLING_BACK        rollback begins (from a failure, a timeout, or the recovery)
//! ROLLED_BACK         rollback finished; the tree is the old version's
//! HEALTHY             the new version started and wrote its marker; the transaction is over
//! ```
//!
//! Paths are root-relative with '/', as in the manifest.
//!
//! WHO WRITES WHAT FOLLOWS COMMITTED_PENDING_HEALTH. The helper that made the swap, while it
//! lives: HEALTHY once its health run has passed, or ROLLING_BACK and ROLLED_BACK when it has not.
//! A helper that dies in between leaves the journal at COMMITTED_PENDING_HEALTH, and no further
//! kind of record exists for that: the SAME records are appended later, under the update lock, by
//! whoever settles the transaction (the recovery) - HEALTHY by `katana_updater --recover`, by the
//! window of the new version or by the next swap, each on the marker the health run writes;
//! ROLLING_BACK and ROLLED_BACK by `katana_updater --recover` alone. So a journal still reads the
//! same way whoever finished it. And it stays TRUE of the tree: while it says
//! COMMITTED_PENDING_HEALTH the tree is the whole new version, because a rollback from that state
//! moves no file before its ROLLING_BACK record is on disk, and the folder and the marker are
//! deleted only after HEALTHY is.
//!
//! THE JOURNAL IS NOT AUTHENTICATED. It sits in R/.update, which a process running as the same
//! user can write, and its crc detects a torn write, not a forged line. So what rollback and
//! recovery READ from it is never trusted as a place to act on: every path of an ADD, REPLACE or
//! REMOVE record is checked with check_journal_path (paths.rs: the manifest rules, plus the
//! helper's own three records), and that of an MKDIR record with check_journal_directory (the
//! same, and a bare install folder such as "etc"), before any file is touched, and a record that
//! fails is skipped and reported, not followed; the transaction id in the folder name is checked
//! as a transaction id. A forged journal can therefore at worst make rollback do less, never reach
//! outside R's bin, share, etc and lib. An argument is bytes for the same reason: what a journal
//! holds is whatever was written there, and the path rules, not the reader, refuse it.
//!
//! Rollback does NOT replay the journal backwards. It decides from the disk, so it is idempotent
//! after any crash, including a crash inside itself; the journal says which transaction state to
//! resume (state) and which files and directories the transaction owns (the PLAN group).

use crate::fsops::FsOps;
use katana_core::error::make_error;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{ErrorCode, Result, Status};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum JournalOp {
    #[default]
    Plan,
    Add,
    Replace,
    Remove,
    MakeDirectory,
    Committing,
    MovedOld,
    MovedNew,
    CommittedPendingHealth,
    RollingBack,
    RolledBack,
    Healthy,
}

const OPERATIONS: [(JournalOp, &str); 12] = [
    (JournalOp::Plan, "PLAN"),
    (JournalOp::Add, "ADD"),
    (JournalOp::Replace, "REPLACE"),
    (JournalOp::Remove, "REMOVE"),
    (JournalOp::MakeDirectory, "MKDIR"),
    (JournalOp::Committing, "COMMITTING"),
    (JournalOp::MovedOld, "MOVED_OLD"),
    (JournalOp::MovedNew, "MOVED_NEW"),
    (
        JournalOp::CommittedPendingHealth,
        "COMMITTED_PENDING_HEALTH",
    ),
    (JournalOp::RollingBack, "ROLLING_BACK"),
    (JournalOp::RolledBack, "ROLLED_BACK"),
    (JournalOp::Healthy, "HEALTHY"),
];

/// "PLAN" "ADD" ... "HEALTHY".
pub fn to_string(op: JournalOp) -> &'static str {
    for (value, name) in OPERATIONS {
        if value == op {
            return name;
        }
    }
    "UNKNOWN"
}

/// The operation a name stands for; ParseFailure for any other text (the names are upper case).
pub fn parse_journal_op(text: impl AsRef<[u8]>) -> Result<JournalOp> {
    let text = text.as_ref();
    for (value, name) in OPERATIONS {
        if name.as_bytes() == text {
            return Ok(value);
        }
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        "not a journal operation",
        utf8_or_windows1252(text),
    ))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JournalRecord {
    pub op: JournalOp,
    /// Raw (decoded) arguments.
    pub args: Vec<Vec<u8>>,
}

/// Where the transaction stands, from the last state-bearing record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum JournalState {
    /// No complete record.
    #[default]
    Empty,
    /// PLAN written, COMMITTING not.
    Planned,
    Committing,
    CommittedPendingHealth,
    RollingBack,
    RolledBack,
    Healthy,
}

/// A journal in Planned, Committing or RollingBack is one a crash left behind: the recovery must
/// roll it back. CommittedPendingHealth is NOT one of them: its tree is a whole version and its
/// helper may be alive and waiting, so the recovery leaves it (the recovery module says who
/// settles one whose helper died, and how).
pub fn needs_recovery(state: JournalState) -> bool {
    matches!(
        state,
        JournalState::Planned | JournalState::Committing | JournalState::RollingBack
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedJournal {
    /// Complete, valid, consecutive.
    pub records: Vec<JournalRecord>,
    pub state: JournalState,
    /// Bytes after the last good record were ignored.
    pub torn_tail: bool,
}

// The reflected polynomial of IEEE 802.3, 0xEDB88320: the CRC-32 of zlib, gzip and PNG.
const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if (value & 1) != 0 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = make_crc_table();

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// CRC-32 (IEEE 802.3), as zlib's crc32().
pub fn crc32(bytes: impl AsRef<[u8]>) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes.as_ref() {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn is_safe_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'/' | b'~' | b'-')
}

fn encode_argument(argument: &[u8], out: &mut String) {
    out.reserve(argument.len());
    for &byte in argument {
        if is_safe_byte(byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX_UPPER[usize::from(byte >> 4)]));
            out.push(char::from(HEX_UPPER[usize::from(byte & 0x0F)]));
        }
    }
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'F' => Some(c - b'A' + 10),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

// None for a '%' not followed by two hex digits: such a token was not written by
// encode_argument, so the line is not one of ours.
fn decode_argument(token: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(token.len());
    let mut i = 0;
    while i < token.len() {
        if token[i] != b'%' {
            out.push(token[i]);
            i += 1;
            continue;
        }
        if i + 2 >= token.len() {
            return None;
        }
        let high = hex_value(token[i + 1])?;
        let low = hex_value(token[i + 2])?;
        out.push(high * 16 + low);
        i += 3;
    }
    Some(out)
}

// The crc of a line's body as the line carries it: 8 lower-case hex digits.
fn crc_text(body: &[u8]) -> String {
    format!("{:08x}", crc32(body))
}

fn is_decimal(text: &[u8]) -> bool {
    !text.is_empty() && text.iter().all(u8::is_ascii_digit)
}

// One complete line (without its LF) -> a record, or None when it is not a valid next record of
// the journal.
fn parse_line(line: &[u8], expected_number: u64) -> Option<JournalRecord> {
    let last_space = line.iter().rposition(|&c| c == b' ')?;
    let body = &line[..last_space];
    let crc = &line[last_space + 1..];
    if crc.len() != 8 || crc != crc_text(body).as_bytes() {
        return None;
    }

    // Tokens split on single spaces, empty ones kept: an empty argument is "%"-free nothing
    // between two spaces.
    let tokens: Vec<&[u8]> = body.split(|&c| c == b' ').collect();
    if tokens.len() < 2
        || !is_decimal(tokens[0])
        || tokens[0] != format!("{expected_number}").as_bytes()
    {
        return None;
    }
    let op = parse_journal_op(tokens[1]).ok()?;
    let mut args = Vec::with_capacity(tokens.len() - 2);
    for token in &tokens[2..] {
        args.push(decode_argument(token)?);
    }
    Some(JournalRecord { op, args })
}

fn state_after(current: JournalState, op: JournalOp) -> JournalState {
    match op {
        JournalOp::Plan => JournalState::Planned,
        JournalOp::Committing => JournalState::Committing,
        JournalOp::CommittedPendingHealth => JournalState::CommittedPendingHealth,
        JournalOp::RollingBack => JournalState::RollingBack,
        JournalOp::RolledBack => JournalState::RolledBack,
        JournalOp::Healthy => JournalState::Healthy,
        JournalOp::Add
        | JournalOp::Replace
        | JournalOp::Remove
        | JournalOp::MakeDirectory
        | JournalOp::MovedOld
        | JournalOp::MovedNew => current,
    }
}

/// "<n> <OP> <args> <crc>\n" for one record. Always ASCII: every byte of an argument outside the
/// safe set is written as %XX.
pub fn encode_record(n: u64, record: &JournalRecord) -> String {
    let mut line = format!("{n} {}", to_string(record.op));
    for argument in &record.args {
        line.push(' ');
        encode_argument(argument, &mut line);
    }
    let crc = crc_text(line.as_bytes());
    line.push(' ');
    line.push_str(&crc);
    line.push('\n');
    line
}

/// The journal text -> its good prefix. Never fails: whatever cannot be read is the torn tail.
pub fn parse_journal(text: impl AsRef<[u8]>) -> ParsedJournal {
    let text = text.as_ref();
    let mut parsed = ParsedJournal::default();
    let mut position = 0usize;
    while position < text.len() {
        let Some(length) = text[position..].iter().position(|&c| c == b'\n') else {
            break; // no LF: the write that was torn
        };
        let end = position + length;
        let Some(record) = parse_line(&text[position..end], parsed.records.len() as u64 + 1) else {
            break;
        };
        parsed.state = state_after(parsed.state, record.op);
        parsed.records.push(record);
        position = end + 1;
    }
    parsed.torn_tail = position < text.len();
    parsed
}

/// A journal file, appended through FsOps so the fault loops see every write.
pub struct Journal<'a> {
    fs: &'a dyn FsOps,
    path: PathBuf,
    next: u64, // 0: not yet known
}

impl<'a> Journal<'a> {
    pub fn new(fs: &'a dyn FsOps, path: impl Into<PathBuf>) -> Self {
        Self {
            fs,
            path: path.into(),
            next: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends the records as one group, numbered from the journal's next number, with one
    /// flush. The Journal reads the file once (on the first append) to learn the next number.
    pub fn append(&mut self, group: &[JournalRecord]) -> Status {
        if group.is_empty() {
            return Ok(());
        }
        // Learn where the journal stands the first time, because a crash may have left a torn last
        // line: records written after it would be unreadable (parsing stops at the first bad
        // line), so a journal with a torn tail is rewritten from its good prefix instead.
        let mut rewritten = String::new();
        let mut rewrite = false;
        let mut next = self.next;
        if next == 0 {
            let existing = self.load()?;
            next = existing.records.len() as u64 + 1;
            if existing.torn_tail {
                rewrite = true;
                for (number, record) in (1u64..).zip(&existing.records) {
                    rewritten.push_str(&encode_record(number, record));
                }
            }
        }
        let mut bytes = String::new();
        for record in group {
            bytes.push_str(&encode_record(next, record));
            next += 1;
        }
        let written = if rewrite {
            rewritten.push_str(&bytes);
            self.fs.write_durable(&self.path, rewritten.as_bytes())
        } else {
            self.fs.append_durable(&self.path, bytes.as_bytes())
        };
        // The numbers move on only for a group that was written. One that failed may have left
        // part of its bytes behind, and what the file holds is then learned again by the next
        // append (which rewrites from the good prefix): carrying on from `next` would leave a gap
        // in the numbers, and the read stops at the first gap.
        self.next = if written.is_ok() { next } else { 0 };
        written
    }

    /// Reads and parses the file; an absent file is an Empty journal.
    pub fn load(&self) -> Result<ParsedJournal> {
        match self.fs.read_file(&self.path) {
            Ok(bytes) => Ok(parse_journal(&bytes)),
            Err(error) if error.code == ErrorCode::NotFound => Ok(ParsedJournal::default()),
            Err(error) => Err(error),
        }
    }
}
