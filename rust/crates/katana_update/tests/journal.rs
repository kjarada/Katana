//! The apply's journal (journal.rs) and the real Windows file system under it (fsops_win.rs). The
//! twin of tests/update/test_journal.cpp.
//!
//! Every expectation here is worked out outside the code under test:
//!
//! * CRC-32 values are the published check values of the algorithm (the "check" value of
//!   CRC-32/ISO-HDLC, 0xCBF43926 for "123456789") and Python's zlib.crc32 of each line body;
//! * encoded lines are written out by hand and their crc taken from zlib;
//! * the Windows error numbers are the documented ones (2 file not found, 32 sharing violation, 80
//!   file exists, 145 directory not empty, 183 already exists).

pub mod test_key;

pub mod apply_harness;

use apply_harness::{
    ForwardingFs, HookFs, Scratch, StdFs, injected, is_present, quiet_simulated_crashes, read_text,
    write_text,
};
use katana_update::fault_fs::{FaultFs, SimulatedCrash};
use katana_update::fsops::RETRYABLE_OS_ERRORS;
use katana_update::journal::{
    Journal, JournalOp, JournalRecord, JournalState, crc32, encode_record, needs_recovery,
    parse_journal, parse_journal_op, to_string as op_name,
};
use std::cell::Cell;
use std::ffi::OsStr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

/// A record of `op` with these arguments.
fn record(op: JournalOp, args: &[&str]) -> JournalRecord {
    JournalRecord {
        op,
        args: args
            .iter()
            .map(|argument| argument.as_bytes().to_vec())
            .collect(),
    }
}

/// The arguments as the parser gives them back: bytes.
fn bytes_of(args: &[&str]) -> Vec<Vec<u8>> {
    args.iter()
        .map(|argument| argument.as_bytes().to_vec())
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

// ---- the format ---------------------------------------------------------------------------------

mod journal_format {
    use super::*;

    #[test]
    fn the_crc_is_the_crc32_of_zlib_and_png() {
        // The check value every CRC-32/ISO-HDLC implementation publishes, and three more taken
        // from zlib.crc32.
        assert_eq!(crc32(""), 0x0000_0000);
        assert_eq!(crc32("a"), 0xE8B7_BE43);
        assert_eq!(crc32("123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32("The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn a_record_is_number_operation_arguments_and_the_crc_of_those_bytes() {
        assert_eq!(
            encode_record(1, &record(JournalOp::Plan, &["t1", "5", "7"])),
            "1 PLAN t1 5 7 a888bad4\n"
        );
        assert_eq!(
            encode_record(4, &record(JournalOp::Committing, &[])),
            "4 COMMITTING 51265d9e\n"
        );
        assert_eq!(
            encode_record(8, &record(JournalOp::CommittedPendingHealth, &[])),
            "8 COMMITTED_PENDING_HEALTH 5f36ea28\n"
        );
        assert_eq!(
            encode_record(9, &record(JournalOp::RollingBack, &[])),
            "9 ROLLING_BACK be72f1f2\n"
        );
        assert_eq!(
            encode_record(10, &record(JournalOp::RolledBack, &[])),
            "10 ROLLED_BACK b37b4c8a\n"
        );
        assert_eq!(
            encode_record(11, &record(JournalOp::Healthy, &[])),
            "11 HEALTHY 7905ab3b\n"
        );
    }

    #[test]
    fn every_byte_outside_the_safe_set_is_percent_encoded_in_upper_case_hex() {
        // The safe set is [A-Za-z0-9._/~-]; a space, a non-ASCII byte, a percent sign and a line
        // break are all escaped, so an argument never holds a space or a line break.
        assert_eq!(
            encode_record(2, &record(JournalOp::Add, &["bin/a b.dll"])),
            "2 ADD bin/a%20b.dll ac554854\n"
        );
        assert_eq!(
            encode_record(3, &record(JournalOp::Replace, &["share/katana/\u{e9}.txt"])),
            "3 REPLACE share/katana/%C3%A9.txt f5b5dfa6\n"
        );
        assert_eq!(
            encode_record(6, &record(JournalOp::Remove, &["a%b\nc"])),
            "6 REMOVE a%25b%0Ac 29770ce9\n"
        );
        assert_eq!(
            encode_record(7, &record(JournalOp::MakeDirectory, &["share/katana/more"])),
            "7 MKDIR share/katana/more 4004c056\n"
        );
    }

    #[test]
    fn an_empty_argument_survives_the_round_trip() {
        // Two spaces in a row: the empty token between them is the argument.
        assert_eq!(
            encode_record(5, &record(JournalOp::MovedOld, &[""])),
            "5 MOVED_OLD  a5d06e93\n"
        );

        let again = parse_journal(encode_record(
            1,
            &record(JournalOp::MovedOld, &["", "x", ""]),
        ));
        assert_eq!(again.records.len(), 1);
        assert_eq!(again.records[0].args, bytes_of(&["", "x", ""]));
    }

    #[test]
    fn arguments_with_spaces_non_ascii_bytes_percent_and_line_breaks_come_back_as_they_went() {
        let awkward: [Vec<u8>; 8] = [
            b"bin/with space.dll".to_vec(),
            "caf\u{e9}/\u{65e5}\u{672c}.txt".as_bytes().to_vec(),
            b"100%".to_vec(),
            b"%41".to_vec(),
            b"a\nb\r\nc".to_vec(),
            b"nul\0byte".to_vec(),
            vec![b'x'; 300],
            b"~tilde-and_dots.v1.2/x".to_vec(),
        ];
        let mut text = String::new();
        for (n, argument) in (1u64..).zip(&awkward) {
            text.push_str(&encode_record(
                n,
                &JournalRecord {
                    op: JournalOp::Add,
                    args: vec![argument.clone()],
                },
            ));
        }
        let parsed = parse_journal(&text);
        assert_eq!(parsed.records.len(), awkward.len());
        assert!(!parsed.torn_tail);
        for (i, argument) in awkward.iter().enumerate() {
            assert_eq!(parsed.records[i].args.len(), 1);
            assert_eq!(&parsed.records[i].args[0], argument, "record {i}");
        }
        // And no encoded line has a raw space inside an argument or a second line break.
        assert_eq!(
            text.bytes().filter(|&byte| byte == b'\n').count(),
            awkward.len()
        );
    }

    #[test]
    fn every_operation_has_its_name_and_parses_back() {
        let all = [
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
        for (op, name) in all {
            assert_eq!(op_name(op), name);
            let parsed = parse_journal_op(name)
                .unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
            assert_eq!(parsed, op);
        }
        assert!(
            parse_journal_op("plan").is_err(),
            "the names are upper case"
        );
        assert!(parse_journal_op("DELETE").is_err());
        assert!(parse_journal_op("").is_err());
    }
}

// ---- reading what a crash left ------------------------------------------------------------------

mod journal_parse {
    use super::*;

    #[test]
    fn an_empty_text_is_an_empty_journal_with_nothing_torn() {
        let parsed = parse_journal("");
        assert!(parsed.records.is_empty());
        assert_eq!(parsed.state, JournalState::Empty);
        assert!(!parsed.torn_tail);
    }

    #[test]
    fn a_last_line_without_its_line_feed_is_torn_and_ignored() {
        let parsed = parse_journal("1 ADD a 3d939a4d\n2 ADD b 9572d16a");
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].args[0], b"a");
        assert!(parsed.torn_tail);
    }

    #[test]
    fn a_line_cut_short_anywhere_leaves_the_good_prefix() {
        let whole = b"1 ADD a 3d939a4d\n2 ADD b 9572d16a\n3 ADD c 4402ea48\n";
        let first_two = "1 ADD a 3d939a4d\n2 ADD b 9572d16a\n";
        for cut in 0..whole.len() {
            let parsed = parse_journal(&whole[..cut]);
            // The records that are complete in the first `cut` bytes: one per line feed.
            let expected = whole[..cut].iter().filter(|&&byte| byte == b'\n').count();
            assert_eq!(parsed.records.len(), expected, "cut at {cut}");
            // Torn exactly when bytes remain after the last line feed.
            let bytes_after = cut > 0 && whole[cut - 1] != b'\n';
            assert_eq!(parsed.torn_tail, bytes_after, "cut at {cut}");
        }
        assert_eq!(parse_journal(first_two).records.len(), 2);
    }

    #[test]
    fn a_wrong_crc_stops_the_read_and_everything_after_it_is_ignored() {
        // The second line's crc is wrong by one digit; the third is fine but follows it.
        let parsed = parse_journal("1 ADD a 3d939a4d\n2 ADD b 9572d16b\n3 ADD c 4402ea48\n");
        assert_eq!(parsed.records.len(), 1);
        assert!(parsed.torn_tail);
    }

    #[test]
    fn an_upper_case_crc_is_not_the_format_and_stops_the_read() {
        let parsed = parse_journal("1 ADD a 3D939A4D\n");
        assert!(parsed.records.is_empty());
        assert!(parsed.torn_tail);
    }

    #[test]
    fn a_gap_or_a_repeat_in_the_numbers_stops_the_read() {
        // 1, 3: the second is not the previous plus one.
        assert_eq!(
            parse_journal("1 ADD a 3d939a4d\n3 ADD c 4402ea48\n")
                .records
                .len(),
            1
        );
        // 1, 1.
        assert_eq!(
            parse_journal("1 ADD a 3d939a4d\n1 ADD a 3d939a4d\n")
                .records
                .len(),
            1
        );
        // Starting at 2.
        assert!(parse_journal("2 ADD b 9572d16a\n").records.is_empty());
        // A leading zero is not how a number is written.
        assert!(parse_journal("01 ADD a 3d939a4d\n").records.is_empty());
    }

    #[test]
    fn an_unknown_operation_a_bad_escape_and_a_missing_part_end_the_read() {
        let crc_of = |body: &str| format!("{body} {:08x}\n", crc32(body));
        // Each line has a correct crc, so only its content is wrong.
        assert!(parse_journal(crc_of("1 DELETE a")).records.is_empty());
        assert!(parse_journal(crc_of("1 ADD a%2")).records.is_empty());
        assert!(parse_journal(crc_of("1 ADD a%zz")).records.is_empty());
        assert!(parse_journal(crc_of("1")).records.is_empty());
        assert!(parse_journal(crc_of("ADD a")).records.is_empty());
        assert!(parse_journal(crc_of("x ADD a")).records.is_empty());
        // And the same line right is read.
        assert_eq!(parse_journal(crc_of("1 ADD a%2F")).records.len(), 1);
        assert_eq!(
            parse_journal(crc_of("1 ADD a%2F")).records[0].args[0],
            b"a/"
        );
    }

    #[test]
    fn a_line_ended_with_carriage_return_and_line_feed_is_not_ours() {
        // A text editor that rewrote the line ends changed the bytes the crc covers.
        let parsed = parse_journal("1 ADD a 3d939a4d\r\n");
        assert!(parsed.records.is_empty());
        assert!(parsed.torn_tail);
    }

    #[test]
    fn garbage_is_a_torn_tail_not_a_failure() {
        let texts: [Vec<u8>; 5] = [
            b"\n".to_vec(),
            b"\n\n\n".to_vec(),
            b"garbage".to_vec(),
            vec![0xFF; 1000],
            b"1 ADD a 3d939a4d\n\n".to_vec(),
        ];
        for text in &texts {
            let parsed = parse_journal(text);
            assert!(parsed.records.len() <= 1);
            assert!(parsed.torn_tail);
        }
    }
}

mod journal_state {
    use super::*;

    fn state_after(ops: &[JournalOp]) -> JournalState {
        let mut text = String::new();
        for (n, &op) in (1u64..).zip(ops) {
            text.push_str(&encode_record(n, &record(op, &[])));
        }
        parse_journal(text).state
    }

    #[test]
    fn each_record_leads_to_the_state_the_header_names() {
        assert_eq!(state_after(&[]), JournalState::Empty);
        assert_eq!(state_after(&[JournalOp::Plan]), JournalState::Planned);
        assert_eq!(
            state_after(&[
                JournalOp::Plan,
                JournalOp::Add,
                JournalOp::Replace,
                JournalOp::Remove,
                JournalOp::MakeDirectory
            ]),
            JournalState::Planned,
            "the operations of the PLAN group do not change the state"
        );
        assert_eq!(
            state_after(&[JournalOp::Plan, JournalOp::Committing]),
            JournalState::Committing
        );
        assert_eq!(
            state_after(&[
                JournalOp::Plan,
                JournalOp::Committing,
                JournalOp::MovedOld,
                JournalOp::MovedNew
            ]),
            JournalState::Committing,
            "progress records do not change the state"
        );
        assert_eq!(
            state_after(&[
                JournalOp::Plan,
                JournalOp::Committing,
                JournalOp::CommittedPendingHealth
            ]),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(
            state_after(&[
                JournalOp::Plan,
                JournalOp::Committing,
                JournalOp::RollingBack
            ]),
            JournalState::RollingBack
        );
        assert_eq!(
            state_after(&[
                JournalOp::Plan,
                JournalOp::Committing,
                JournalOp::RollingBack,
                JournalOp::RolledBack
            ]),
            JournalState::RolledBack
        );
        assert_eq!(
            state_after(&[
                JournalOp::Plan,
                JournalOp::Committing,
                JournalOp::CommittedPendingHealth,
                JournalOp::Healthy
            ]),
            JournalState::Healthy
        );
        // A rollback of a version that was pending health: the last state-bearing record wins.
        assert_eq!(
            state_after(&[
                JournalOp::Plan,
                JournalOp::Committing,
                JournalOp::CommittedPendingHealth,
                JournalOp::RollingBack
            ]),
            JournalState::RollingBack
        );
    }

    #[test]
    fn only_a_journal_a_crash_left_half_done_needs_recovery() {
        assert!(!needs_recovery(JournalState::Empty));
        assert!(needs_recovery(JournalState::Planned));
        assert!(needs_recovery(JournalState::Committing));
        assert!(!needs_recovery(JournalState::CommittedPendingHealth));
        assert!(needs_recovery(JournalState::RollingBack));
        assert!(!needs_recovery(JournalState::RolledBack));
        assert!(!needs_recovery(JournalState::Healthy));
    }
}

// ---- the writer ---------------------------------------------------------------------------------

mod journal_file {
    use super::*;

    #[test]
    fn a_group_is_one_append_and_the_numbers_continue_across_groups_and_objects() {
        let scratch = Scratch::new("journal-group");
        let disk = StdFs;
        let counting = FaultFs::new(&disk);
        let path = scratch.join("tx").join("journal");
        std::fs::create_dir_all(scratch.join("tx")).expect("the folder can be made");
        {
            let mut journal = Journal::new(&counting, path.as_path());
            let first = [
                record(JournalOp::Plan, &["t1", "1", "2"]),
                record(JournalOp::Add, &["bin/a.dll"]),
                record(JournalOp::Replace, &["bin/b.dll"]),
            ];
            assert!(journal.append(&first).is_ok());
            assert_eq!(counting.mutating_operations(), 1, "one flush per group");
            assert!(
                journal
                    .append(&[record(JournalOp::Committing, &[])])
                    .is_ok()
            );
            assert_eq!(counting.mutating_operations(), 2);
        }
        {
            // A new object over the same file learns where the numbers stand.
            let mut again = Journal::new(&disk, path.as_path());
            assert!(
                again
                    .append(&[record(JournalOp::CommittedPendingHealth, &[])])
                    .is_ok()
            );
        }
        let loaded = Journal::new(&disk, path.as_path())
            .load()
            .expect("the journal is read");
        assert_eq!(loaded.records.len(), 5);
        assert_eq!(loaded.records[1].args, bytes_of(&["bin/a.dll"]));
        assert_eq!(loaded.state, JournalState::CommittedPendingHealth);
        assert!(!loaded.torn_tail);
        // The file holds exactly the numbered lines.
        let text = read_text(&path);
        assert!(text.starts_with(b"1 PLAN t1 1 2 "));
        assert!(contains(&text, b"\n4 COMMITTING 51265d9e\n"));
        assert!(contains(&text, b"\n5 COMMITTED_PENDING_HEALTH c796f7dd\n"));
    }

    #[test]
    fn an_empty_group_writes_nothing() {
        let scratch = Scratch::new("journal-empty-group");
        let disk = StdFs;
        let counting = FaultFs::new(&disk);
        let mut journal = Journal::new(&counting, scratch.join("journal"));
        assert!(journal.append(&[]).is_ok());
        assert_eq!(counting.mutating_operations(), 0);
        assert!(!is_present(&scratch.join("journal")));
    }

    #[test]
    fn a_load_of_a_missing_file_is_an_empty_journal() {
        let scratch = Scratch::new("journal-missing");
        let disk = StdFs;
        let loaded = Journal::new(&disk, scratch.join("nope"))
            .load()
            .expect("a missing journal is read as an empty one");
        assert_eq!(loaded.state, JournalState::Empty);
        assert!(loaded.records.is_empty());
    }

    #[test]
    fn records_appended_after_a_torn_line_are_still_readable() {
        // A crash cut the third record in half. Appending ROLLING_BACK straight after those bytes
        // would glue it to the fragment and lose it; the journal is rewritten from its good
        // prefix.
        let scratch = Scratch::new("journal-torn");
        let disk = StdFs;
        let path = scratch.join("journal");
        write_text(&path, "1 ADD a 3d939a4d\n2 ADD b 9572d16a\n3 ADD c 44");
        let mut journal = Journal::new(&disk, path.as_path());
        assert!(
            journal
                .append(&[record(JournalOp::RollingBack, &[])])
                .is_ok()
        );

        let loaded = Journal::new(&disk, path.as_path())
            .load()
            .expect("the journal is read");
        assert_eq!(loaded.records.len(), 3);
        assert!(!loaded.torn_tail);
        assert_eq!(loaded.records[2].op, JournalOp::RollingBack);
        assert_eq!(loaded.state, JournalState::RollingBack);
    }

    #[test]
    fn a_crash_inside_a_group_writes_nothing_of_it() {
        let scratch = Scratch::new("journal-crash");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        let path = scratch.join("journal");
        let mut journal = Journal::new(&faults, path.as_path());
        assert!(
            journal
                .append(&[record(JournalOp::Plan, &["t1", "1", "2"])])
                .is_ok()
        );
        faults.crash_before(2);
        quiet_simulated_crashes();
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            journal.append(&[record(JournalOp::Committing, &[])])
        }));
        assert!(
            crashed.is_err_and(|payload| payload.is::<SimulatedCrash>()),
            "the append did not die of the simulated crash"
        );
        let loaded = Journal::new(&disk, path.as_path())
            .load()
            .expect("the journal is read");
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.state, JournalState::Planned);
    }

    #[test]
    fn a_group_that_failed_to_be_written_leaves_no_gap_in_the_numbers() {
        // Parsing stops at the first number that is not the previous one plus one, so a gap
        // would make every record after it unreadable. The failed group is numbered again.
        let scratch = Scratch::new("journal-failed-append");
        let disk = StdFs;
        let appends = Cell::new(0);
        let hooked = HookFs::new(&disk);
        hooked.set_before_append(|p: &Path, _bytes: &[u8]| {
            appends.set(appends.get() + 1);
            if appends.get() == 2 {
                Some(injected(112, p))
            } else {
                None
            }
        });
        let path = scratch.join("journal");
        let mut journal = Journal::new(&hooked, path.as_path());
        assert!(
            journal
                .append(&[record(JournalOp::Plan, &["t1", "1", "2"])])
                .is_ok()
        );
        assert!(
            journal
                .append(&[record(JournalOp::Committing, &[])])
                .is_err()
        );
        assert!(
            journal
                .append(&[record(JournalOp::MovedOld, &["bin/a.dll"])])
                .is_ok()
        );

        let loaded = Journal::new(&disk, path.as_path())
            .load()
            .expect("the journal is read");
        assert_eq!(loaded.records.len(), 2);
        assert_eq!(loaded.records[0].op, JournalOp::Plan);
        assert_eq!(loaded.records[1].op, JournalOp::MovedOld);
        assert!(!loaded.torn_tail);
    }

    #[test]
    fn a_group_that_was_half_written_before_it_failed_is_replaced_by_the_next_one() {
        let scratch = Scratch::new("journal-half-append");
        let disk = StdFs;
        // The C++ subclasses its forwarding file system to break the append once: half of the
        // COMMITTING group is written, and the append reports a failure.
        let failed = Cell::new(false);
        let half = ForwardingFs::new(&disk);
        half.on_append_durable(|inner, p: &Path, bytes: &[u8]| {
            if !failed.get()
                && p.file_name() == Some(OsStr::new("journal"))
                && contains(bytes, b"COMMITTING")
            {
                failed.set(true);
                let _ = inner.append_durable(p, &bytes[..bytes.len() / 2]);
                return injected(112, p);
            }
            inner.append_durable(p, bytes)
        });
        let path = scratch.join("journal");
        let mut journal = Journal::new(&half, path.as_path());
        assert!(
            journal
                .append(&[record(JournalOp::Plan, &["t1", "1", "2"])])
                .is_ok()
        );
        assert!(
            journal
                .append(&[record(JournalOp::Committing, &[])])
                .is_err()
        );
        assert!(
            parse_journal(read_text(&path)).torn_tail,
            "half a record is in the file"
        );
        assert!(
            journal
                .append(&[record(JournalOp::MovedOld, &["bin/a.dll"])])
                .is_ok()
        );

        let loaded = Journal::new(&disk, path.as_path())
            .load()
            .expect("the journal is read");
        assert_eq!(loaded.records.len(), 2);
        assert_eq!(loaded.records[1].op, JournalOp::MovedOld);
        assert!(!loaded.torn_tail);
    }
}

mod fs_ops_contract {
    use super::*;

    #[test]
    fn the_errors_a_rename_retries_are_access_denied_sharing_and_lock_violations() {
        // What an antivirus scanner or a program that has the file causes (design 5.5). The real
        // file system retries exactly these; the Windows tests below show it for 5 and 32.
        assert_eq!(RETRYABLE_OS_ERRORS, [5, 32, 33]);
    }
}

// ---- the real file system, on Windows -----------------------------------------------------------

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the cases hold files open, make them read-only and start a program through the \
              system's own calls, as the C++ does"
)]
mod windows_fs {
    use super::*;
    use apply_harness::text;
    use katana_core::path_text::{environment_variable, path_from_utf8, path_to_utf8};
    use katana_core::{Error, ErrorCode};
    use katana_update::fsops::{EntryKind, FsOps, RetryPolicy, make_system_fs_ops};
    use katana_update::process::{SpawnRequest, make_system_launcher};
    use std::collections::BTreeSet;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetDiskFreeSpaceExW, GetFileAttributesW, SetFileAttributesW,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
    };

    // The values of the Windows SDK's headers (winnt.h, fileapi.h, winbase.h).
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    // A fast retry policy: the tests that wait for it must not take 30 seconds. The C++'s
    // defaults are 5 and 120.
    fn system_fs(interval_ms: u64, total_ms: u64) -> Box<dyn FsOps> {
        make_system_fs_ops(&RetryPolicy {
            interval: Duration::from_millis(interval_ms),
            total: Duration::from_millis(total_ms),
        })
    }

    fn os_error_of(error: &Error) -> String {
        // The context begins `os_error=<n>`.
        let Some(rest) = error.context.strip_prefix("os_error=") else {
            return String::from("(no os_error)");
        };
        String::from(rest.split(' ').next().unwrap_or_default())
    }

    fn set_attributes(path: &Path, attributes: u32) {
        let name = wide(path);
        // SAFETY: `name` is a NUL-terminated wide path.
        unsafe { SetFileAttributesW(name.as_ptr(), attributes) };
    }

    fn attributes_of(path: &Path) -> u32 {
        let name = wide(path);
        // SAFETY: `name` is a NUL-terminated wide path.
        unsafe { GetFileAttributesW(name.as_ptr()) }
    }

    /// Opens a file the way an antivirus scanner or a running program does, refusing every share
    /// but those given.
    struct HeldOpen {
        handle: HANDLE,
    }

    // SAFETY: a file handle may be used and closed from any thread; the scanner of the cases below
    // lets go of the file from its own.
    unsafe impl Send for HeldOpen {}

    impl HeldOpen {
        fn new(path: &Path, share: u32) -> Self {
            let name = wide(path);
            // SAFETY: `name` is a NUL-terminated wide path; no security attributes and no
            // template.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ,
                    share,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            };
            Self { handle }
        }

        fn ok(&self) -> bool {
            self.handle != INVALID_HANDLE_VALUE
        }

        fn release(&mut self) {
            if self.handle != INVALID_HANDLE_VALUE {
                // SAFETY: the handle is ours and open; it is closed once.
                unsafe { CloseHandle(self.handle) };
                self.handle = INVALID_HANDLE_VALUE;
            }
        }
    }

    impl Drop for HeldOpen {
        fn drop(&mut self) {
            self.release();
        }
    }

    fn system_root() -> PathBuf {
        path_from_utf8(environment_variable(Some("SystemRoot")))
    }

    fn make_junction(link: &Path, target: &Path) -> bool {
        let launcher = make_system_launcher();
        let request = SpawnRequest {
            executable: system_root().join("System32").join("cmd.exe"),
            arguments: vec![
                String::from("/c"),
                String::from("mklink"),
                String::from("/J"),
                text(link),
                text(target),
            ],
            ..SpawnRequest::default()
        };
        matches!(launcher.run(&request), Ok(ran) if ran.exit_code == 0)
    }

    // std::filesystem::space's `available`: the bytes free to this user on the volume.
    fn standard_free_space(path: &Path) -> u64 {
        let name = wide(path);
        let mut free_to_caller = 0u64;
        // SAFETY: `name` is NUL-terminated; the one out-pointer is a u64 of ours, and the two the
        // call may be given null for are null.
        let done = unsafe {
            GetDiskFreeSpaceExW(
                name.as_ptr(),
                &raw mut free_to_caller,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_ne!(done, 0, "the free space of {} is known", text(path));
        free_to_caller
    }

    #[test]
    fn stat_describes_a_file_a_directory_and_an_entry_that_is_not_there() {
        let scratch = Scratch::new("winfs-stat");
        let disk = system_fs(5, 120);
        write_text(&scratch.join("a.txt"), "12345");
        std::fs::create_dir_all(scratch.join("dir")).expect("the folder can be made");

        let file = disk
            .stat(&scratch.join("a.txt"))
            .expect("a file is described");
        assert_eq!(file.kind, EntryKind::File);
        assert_eq!(file.size, 5);
        assert!(!file.reparse_point);

        let directory = disk
            .stat(&scratch.join("dir"))
            .expect("a folder is described");
        assert_eq!(directory.kind, EntryKind::Directory);
        assert!(!directory.reparse_point);

        let missing = disk
            .stat(&scratch.join("nope").join("deeper"))
            .expect("missing is not an error");
        assert_eq!(missing.kind, EntryKind::Missing);
    }

    #[test]
    fn a_junction_is_a_reparse_point_and_resolve_follows_it_where_stat_does_not() {
        let scratch = Scratch::new("winfs-junction");
        let disk = system_fs(5, 120);
        std::fs::create_dir_all(scratch.join("target").join("inner"))
            .expect("the folders can be made");
        write_text(&scratch.join("target").join("inner").join("f.txt"), "x");
        let link = scratch.join("link");
        if !make_junction(&link, &scratch.join("target")) {
            eprintln!("mklink /J is not available here");
            return;
        }
        let info = disk.stat(&link).expect("the junction is described");
        assert_eq!(info.kind, EntryKind::Directory);
        assert!(info.reparse_point, "a junction sends a path somewhere else");

        // Through the link the same folder is reached, and resolve says where it really is.
        let resolved = disk
            .resolve(&link.join("inner").join("not-yet.txt"))
            .expect("the path resolves");
        // std::filesystem::weakly_canonical of the target, which carries no \\?\ prefix.
        let canonical =
            std::fs::canonicalize(scratch.join("target")).expect("the target is canonical");
        let shown = text(&canonical);
        let plain = match shown.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => canonical.clone(),
        };
        let expected = plain.join("inner").join("not-yet.txt");
        assert_eq!(path_to_utf8(&resolved).len(), path_to_utf8(&expected).len());
        assert!(
            text(&resolved).eq_ignore_ascii_case(&text(&expected)),
            "{} vs {}",
            text(&resolved),
            text(&expected)
        );
        // And a folder inside the link is not itself one.
        let inner = disk
            .stat(&link.join("inner"))
            .expect("the folder is described");
        assert!(!inner.reparse_point);
    }

    #[test]
    fn list_directory_gives_full_paths_and_a_not_found_for_a_missing_folder() {
        let scratch = Scratch::new("winfs-list");
        let disk = system_fs(5, 120);
        write_text(&scratch.join("one.txt"), "1");
        std::fs::create_dir_all(scratch.join("two")).expect("the folder can be made");
        let names = disk
            .list_directory(scratch.path())
            .expect("the folder is listed");
        let mut found = BTreeSet::new();
        for name in &names {
            found.insert(
                name.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
            assert_eq!(
                name.parent(),
                Some(scratch.path()),
                "a full path, not a bare name"
            );
        }
        assert_eq!(
            found,
            BTreeSet::from([String::from("one.txt"), String::from("two")])
        );

        let Err(none) = disk.list_directory(&scratch.join("missing")) else {
            panic!("a missing folder was listed");
        };
        assert_eq!(none.code, ErrorCode::NotFound);
        assert_eq!(os_error_of(&none), "3", "{}", none.describe());
    }

    #[test]
    fn read_file_returns_every_byte_and_a_missing_file_is_error_two() {
        let scratch = Scratch::new("winfs-read");
        let disk = system_fs(5, 120);
        // Past one buffer, every byte value, NULs.
        let bytes: Vec<u8> = (0..70000u32).map(|i| (i * 31) as u8).collect();
        write_text(&scratch.join("b.bin"), &bytes);
        let read = disk
            .read_file(&scratch.join("b.bin"))
            .expect("the file is read");
        assert_eq!(read, bytes);

        write_text(&scratch.join("empty"), "");
        let none = disk
            .read_file(&scratch.join("empty"))
            .expect("an empty file is read");
        assert!(none.is_empty());

        let missing = disk
            .read_file(&scratch.join("nope"))
            .expect_err("a missing file is not read");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(os_error_of(&missing), "2");
    }

    #[test]
    fn a_file_too_large_to_be_a_record_is_refused_and_not_read_into_memory() {
        // read_file is for journals, manifests and records. A file past 256 MiB (the limit
        // written in fsops_win.rs) is refused by its size, before a buffer of that size is made.
        let scratch = Scratch::new("winfs-read-large");
        let disk = system_fs(5, 120);
        let big = scratch.join("big.bin");
        write_text(&big, "x");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&big)
            .and_then(|file| file.set_len(256 * 1024 * 1024 + 1))
            .expect("the file can be made that long");
        let read = disk.read_file(&big).expect_err("the file is refused");
        assert_eq!(read.code, ErrorCode::FileImportFailure);
        assert!(read.message.contains("too large"), "{}", read.describe());
    }

    #[test]
    fn make_directories_makes_the_parents_and_is_done_when_it_exists() {
        let scratch = Scratch::new("winfs-mkdir");
        let disk = system_fs(5, 120);
        let deep = scratch.join("a").join("b").join("c");
        assert!(disk.make_directories(&deep).is_ok());
        assert!(deep.is_dir());
        assert!(disk.make_directories(&deep).is_ok(), "again is success");

        write_text(&scratch.join("file"), "x");
        let blocked = disk
            .make_directories(&scratch.join("file").join("sub"))
            .expect_err("a file is where the folder must be");
        assert_eq!(blocked.code, ErrorCode::InvalidState);
    }

    #[test]
    fn copy_file_makes_a_new_file_with_the_same_bytes_and_never_overwrites() {
        let scratch = Scratch::new("winfs-copy");
        let disk = system_fs(5, 120);
        // Three buffers and a bit.
        let mut bytes = vec![b'q'; 3 * 1024 * 1024 + 17];
        bytes[12345] = 0;
        write_text(&scratch.join("from.bin"), &bytes);
        assert!(
            disk.copy_file(&scratch.join("from.bin"), &scratch.join("to.bin"))
                .is_ok()
        );
        assert_eq!(read_text(&scratch.join("to.bin")), bytes);
        assert_eq!(
            read_text(&scratch.join("from.bin")),
            bytes,
            "the source is untouched"
        );

        // The target exists: AlreadyExists, and it is not touched.
        write_text(&scratch.join("taken"), "mine");
        let refused = disk
            .copy_file(&scratch.join("from.bin"), &scratch.join("taken"))
            .expect_err("an existing target is refused");
        assert_eq!(refused.code, ErrorCode::AlreadyExists);
        // ERROR_FILE_EXISTS
        assert_eq!(os_error_of(&refused), "80", "{}", refused.describe());
        assert_eq!(read_text(&scratch.join("taken")), b"mine");

        // The source is missing: NotFound, and no half file.
        let missing = disk
            .copy_file(&scratch.join("absent"), &scratch.join("never"))
            .expect_err("a missing source is refused");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(!is_present(&scratch.join("never")));

        // An empty file copies.
        write_text(&scratch.join("zero"), "");
        assert!(
            disk.copy_file(&scratch.join("zero"), &scratch.join("zero2"))
                .is_ok()
        );
        assert_eq!(
            std::fs::metadata(scratch.join("zero2"))
                .expect("the copy is there")
                .len(),
            0
        );
    }

    #[test]
    fn a_copy_of_a_read_only_file_is_a_file_the_update_can_replace_later() {
        let scratch = Scratch::new("winfs-copy-readonly");
        let disk = system_fs(5, 120);
        write_text(&scratch.join("from.txt"), "x");
        set_attributes(&scratch.join("from.txt"), FILE_ATTRIBUTE_READONLY);
        assert!(
            disk.copy_file(&scratch.join("from.txt"), &scratch.join("to.txt"))
                .is_ok()
        );
        let attributes = attributes_of(&scratch.join("to.txt"));
        assert_eq!(
            attributes & FILE_ATTRIBUTE_READONLY,
            0,
            "the copy takes the install's rules, not the stage's"
        );
        set_attributes(&scratch.join("from.txt"), FILE_ATTRIBUTE_NORMAL);
    }

    #[test]
    fn rename_moves_a_file_and_never_replaces_one() {
        let scratch = Scratch::new("winfs-rename");
        let disk = system_fs(5, 120);
        write_text(&scratch.join("a"), "AAA");
        assert!(
            disk.rename_file(&scratch.join("a"), &scratch.join("b"))
                .is_ok()
        );
        assert!(!is_present(&scratch.join("a")));
        assert_eq!(read_text(&scratch.join("b")), b"AAA");

        write_text(&scratch.join("c"), "CCC");
        let refused = disk
            .rename_file(&scratch.join("c"), &scratch.join("b"))
            .expect_err("an existing target is refused");
        assert_eq!(refused.code, ErrorCode::AlreadyExists);
        assert_eq!(os_error_of(&refused), "183", "{}", refused.describe());
        assert_eq!(
            read_text(&scratch.join("b")),
            b"AAA",
            "the target is untouched"
        );
        assert_eq!(read_text(&scratch.join("c")), b"CCC");

        let missing = disk
            .rename_file(&scratch.join("absent"), &scratch.join("z"))
            .expect_err("a missing source is refused");
        assert_eq!(missing.code, ErrorCode::NotFound);
    }

    #[test]
    fn a_locked_file_is_retried_for_the_whole_policy_and_then_fails_with_error_thirty_two() {
        let scratch = Scratch::new("winfs-locked");
        let disk = system_fs(10, 150);
        write_text(&scratch.join("busy.dll"), "in use");
        // Share mode 0: nobody may move it.
        let held = HeldOpen::new(&scratch.join("busy.dll"), 0);
        assert!(held.ok());

        let start = Instant::now();
        let status = disk.rename_file(&scratch.join("busy.dll"), &scratch.join("moved.dll"));
        let took = start.elapsed();
        let error = status.expect_err("a held file is not moved");
        // ERROR_SHARING_VIOLATION
        assert_eq!(os_error_of(&error), "32", "{}", error.describe());
        assert!(
            took >= Duration::from_millis(140),
            "it retried for the policy's total"
        );
        assert!(took < Duration::from_millis(5000));
        assert!(is_present(&scratch.join("busy.dll")), "nothing moved");
        assert!(!is_present(&scratch.join("moved.dll")));
    }

    #[test]
    fn a_rename_waits_out_a_scanner_that_lets_go_half_way_through() {
        let scratch = Scratch::new("winfs-released");
        let disk = system_fs(10, 3000);
        write_text(&scratch.join("busy.dll"), "in use");
        let held = HeldOpen::new(&scratch.join("busy.dll"), 0);
        assert!(held.ok());
        let status = std::thread::scope(|scope| {
            let scanner = scope.spawn(move || {
                let mut held = held;
                std::thread::sleep(Duration::from_millis(120));
                held.release();
            });
            let status = disk.rename_file(&scratch.join("busy.dll"), &scratch.join("moved.dll"));
            scanner.join().expect("the scanner's thread");
            status
        });
        assert!(
            status.is_ok(),
            "{}",
            status
                .as_ref()
                .err()
                .map(Error::describe)
                .unwrap_or_default()
        );
        assert!(is_present(&scratch.join("moved.dll")));
        assert!(!is_present(&scratch.join("busy.dll")));
    }

    #[test]
    fn a_rename_answered_with_access_denied_is_retried_until_the_holder_lets_go() {
        // Renaming a folder while a file inside it is open is answered with ERROR_ACCESS_DENIED
        // (5) whatever share mode that file was opened with (probed on this Windows) - the other
        // answer, beside 32, that a scanner or a running program causes. The retry is for it too.
        let scratch = Scratch::new("winfs-access-denied");
        write_text(&scratch.join("dir").join("file.txt"), "x");
        let held = HeldOpen::new(
            &scratch.join("dir").join("file.txt"),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        );
        assert!(held.ok());
        {
            let once = system_fs(10, 0);
            let refused = once
                .rename_file(&scratch.join("dir"), &scratch.join("dir2"))
                .expect_err("a folder with an open file is not moved at once");
            assert_eq!(os_error_of(&refused), "5", "{}", refused.describe());
        }
        let disk = system_fs(10, 3000);
        let status = std::thread::scope(|scope| {
            let scanner = scope.spawn(move || {
                let mut held = held;
                std::thread::sleep(Duration::from_millis(120));
                held.release();
            });
            let status = disk.rename_file(&scratch.join("dir"), &scratch.join("dir2"));
            scanner.join().expect("the scanner's thread");
            status
        });
        assert!(
            status.is_ok(),
            "{}",
            status
                .as_ref()
                .err()
                .map(Error::describe)
                .unwrap_or_default()
        );
        assert!(is_present(&scratch.join("dir2").join("file.txt")));
    }

    #[test]
    fn a_no_retry_policy_fails_on_the_first_sharing_violation() {
        let scratch = Scratch::new("winfs-noretry");
        let disk = system_fs(10, 0);
        write_text(&scratch.join("busy.dll"), "in use");
        let held = HeldOpen::new(&scratch.join("busy.dll"), 0);
        assert!(held.ok());
        let start = Instant::now();
        let status = disk.rename_file(&scratch.join("busy.dll"), &scratch.join("moved.dll"));
        assert!(status.is_err());
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn a_running_program_can_be_moved_but_not_deleted() {
        // Why rollback moves a new program out of the way instead of deleting it (apply.rs): a
        // program that is running refuses deletion and allows a rename. cmd.exe, copied, is the
        // running program.
        let scratch = Scratch::new("winfs-running");
        let disk = system_fs(5, 50);
        let copy = scratch.join("prog.exe");
        std::fs::copy(system_root().join("System32").join("cmd.exe"), &copy)
            .expect("cmd.exe can be copied");

        let program = wide(&copy);
        let mut command: Vec<u16> = format!("\"{}\" /c ping -n 4 127.0.0.1 >nul", text(&copy))
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: both structures are plain data, valid when zeroed.
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        // SAFETY: as above.
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the program and the command line are NUL-terminated wide strings, the command
        // line writable as the call may write to it; the structures are ours.
        let started = unsafe {
            CreateProcessW(
                program.as_ptr(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &raw const startup,
                &raw mut process,
            )
        };
        if started == 0 {
            eprintln!("cannot start a program here");
            return;
        }
        std::thread::sleep(Duration::from_millis(200));

        assert!(
            disk.remove_file(&copy).is_err(),
            "a running image cannot be deleted"
        );
        assert!(
            disk.rename_file(&copy, &scratch.join("moved.exe")).is_ok(),
            "but it can be renamed"
        );
        // SAFETY: the handles are the process's and its thread's, ours to end and close once.
        unsafe {
            TerminateProcess(process.hProcess, 0);
            WaitForSingleObject(process.hProcess, 5000);
            CloseHandle(process.hProcess);
            CloseHandle(process.hThread);
        }
    }

    #[test]
    fn remove_file_deletes_is_done_when_gone_and_takes_a_read_only_file() {
        let scratch = Scratch::new("winfs-remove");
        let disk = system_fs(5, 120);
        write_text(&scratch.join("a"), "x");
        assert!(disk.remove_file(&scratch.join("a")).is_ok());
        assert!(!is_present(&scratch.join("a")));
        assert!(
            disk.remove_file(&scratch.join("a")).is_ok(),
            "already gone is success"
        );
        assert!(
            disk.remove_file(&scratch.join("no").join("such").join("file"))
                .is_ok()
        );

        write_text(&scratch.join("ro"), "x");
        set_attributes(&scratch.join("ro"), FILE_ATTRIBUTE_READONLY);
        assert!(disk.remove_file(&scratch.join("ro")).is_ok());
        assert!(!is_present(&scratch.join("ro")));

        std::fs::create_dir_all(scratch.join("dir")).expect("the folder can be made");
        assert!(
            disk.remove_file(&scratch.join("dir")).is_err(),
            "a folder is not a file"
        );
        assert!(scratch.join("dir").is_dir());
    }

    #[test]
    fn remove_empty_directory_removes_only_an_empty_one() {
        let scratch = Scratch::new("winfs-rmdir");
        let disk = system_fs(5, 120);
        std::fs::create_dir_all(scratch.join("empty")).expect("the folder can be made");
        assert!(disk.remove_empty_directory(&scratch.join("empty")).is_ok());
        assert!(!is_present(&scratch.join("empty")));
        assert!(
            disk.remove_empty_directory(&scratch.join("empty")).is_ok(),
            "gone is success"
        );

        write_text(&scratch.join("full").join("f"), "x");
        let refused = disk
            .remove_empty_directory(&scratch.join("full"))
            .expect_err("a folder that holds a file is not removed");
        assert_eq!(refused.code, ErrorCode::InvalidState);
        // ERROR_DIR_NOT_EMPTY
        assert_eq!(os_error_of(&refused), "145");
        assert!(is_present(&scratch.join("full").join("f")));
    }

    #[test]
    fn append_durable_adds_to_the_end_and_creates_the_file() {
        let scratch = Scratch::new("winfs-append");
        let disk = system_fs(5, 120);
        let path = scratch.join("journal");
        assert!(disk.append_durable(&path, b"one\n").is_ok());
        assert!(disk.append_durable(&path, b"two\n").is_ok());
        assert!(disk.append_durable(&path, b"").is_ok());
        assert_eq!(read_text(&path), b"one\ntwo\n");

        // A reader that holds the file open for reading does not stop the append.
        let reader = HeldOpen::new(
            &path,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        );
        assert!(reader.ok());
        assert!(disk.append_durable(&path, b"three\n").is_ok());
        assert_eq!(read_text(&path), b"one\ntwo\nthree\n");
    }

    #[test]
    fn write_durable_creates_and_replaces_whole_and_leaves_no_temporary() {
        let scratch = Scratch::new("winfs-write");
        let disk = system_fs(5, 120);
        let path = scratch.join("installed.json");
        assert!(
            disk.write_durable(&path, b"first version, rather long\n")
                .is_ok()
        );
        assert_eq!(read_text(&path), b"first version, rather long\n");
        assert!(disk.write_durable(&path, b"short").is_ok());
        assert_eq!(
            read_text(&path),
            b"short",
            "replaced, not overwritten in place"
        );
        assert!(disk.write_durable(&path, b"").is_ok());
        assert!(read_text(&path).is_empty());
        let files = std::fs::read_dir(scratch.path())
            .expect("the folder is listed")
            .count();
        assert_eq!(files, 1, "no temporary file left beside it");
    }

    #[test]
    fn the_lock_is_exclusive_until_it_is_destroyed() {
        let scratch = Scratch::new("winfs-lock");
        let disk = system_fs(5, 120);
        std::fs::create_dir_all(scratch.join(".update")).expect("the folder can be made");
        let lock_path = scratch.join(".update").join("lock");

        let first = disk.try_lock(&lock_path);
        assert!(first.is_ok());
        assert!(is_present(&lock_path), "created when absent");

        // A second holder - the same process here, another one in life - is refused, "held".
        let Err(second) = disk.try_lock(&lock_path) else {
            panic!("a second holder was given the lock");
        };
        assert_eq!(second.code, ErrorCode::InvalidState);
        assert_eq!(second.message, "held");

        // And so is another file system object over the same file.
        let other = system_fs(5, 120);
        assert!(other.try_lock(&lock_path).is_err());

        drop(first);
        let third = disk.try_lock(&lock_path);
        assert!(third.is_ok(), "free once the holder is gone");

        // A lock in a folder that is not there is NotFound, not "held".
        let Err(nowhere) = disk.try_lock(&scratch.join("no-such").join("lock")) else {
            panic!("a lock was taken in a folder that is not there");
        };
        assert_ne!(nowhere.message, "held");
    }

    #[test]
    fn free_space_is_the_volumes_and_works_for_a_folder_that_does_not_exist_yet() {
        let scratch = Scratch::new("winfs-space");
        let disk = system_fs(5, 120);
        let here = disk
            .free_space(scratch.path())
            .expect("the free space is known");
        let later = disk
            .free_space(&scratch.join("not").join("yet"))
            .expect("the free space of a folder still to be made is known");
        // The standard library asks the same volume; the two differ only by what happened
        // between.
        let standard = standard_free_space(scratch.path());
        let slack: u64 = 256 * 1024 * 1024;
        assert!(here.abs_diff(standard) < slack);
        assert!(later.abs_diff(standard) < slack);
    }

    #[test]
    fn sync_directory_is_success_on_a_folder_that_exists() {
        let scratch = Scratch::new("winfs-sync");
        let disk = system_fs(5, 120);
        assert!(disk.sync_directory(scratch.path()).is_ok());
    }

    #[test]
    fn a_path_past_two_hundred_and_sixty_characters_works_through_the_long_path_prefix() {
        let scratch = Scratch::new("winfs-long");
        let disk = system_fs(5, 120);
        let mut deep = scratch.path().to_path_buf();
        while deep.as_os_str().encode_wide().count() < 320 {
            deep.push("a_folder_with_a_fairly_long_name");
        }
        assert!(disk.make_directories(&deep).is_ok());
        let file = deep.join("file.dll");
        assert!(disk.write_durable(&file, b"long path bytes").is_ok());
        let read = disk
            .read_file(&file)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read, b"long path bytes");
        assert!(disk.rename_file(&file, &deep.join("moved.dll")).is_ok());
        let info = disk
            .stat(&deep.join("moved.dll"))
            .expect("the moved file is described");
        assert_eq!(info.kind, EntryKind::File);
        assert!(
            disk.copy_file(&deep.join("moved.dll"), &deep.join("copy.dll"))
                .is_ok()
        );
        assert!(disk.remove_file(&deep.join("moved.dll")).is_ok());
        assert!(disk.remove_file(&deep.join("copy.dll")).is_ok());
        // The scratch folder's drop may not remove a path that long by the short form; the
        // system file system can.
        let mut folder = deep.clone();
        while folder.as_path() != scratch.path() {
            assert!(
                disk.remove_empty_directory(&folder).is_ok(),
                "{}",
                text(&folder)
            );
            folder.pop();
        }
    }

    #[test]
    fn a_name_with_a_non_ascii_letter_and_one_ending_in_a_dot_are_kept_as_they_are() {
        let scratch = Scratch::new("winfs-names");
        let disk = system_fs(5, 120);
        let accented = scratch
            .path()
            .join(path_from_utf8("caf\u{e9} \u{65e5}\u{672c}.txt"));
        assert!(disk.write_durable(&accented, b"unicode").is_ok());
        let read = disk.read_file(&accented).expect("the file is read");
        assert_eq!(read, b"unicode");

        // Without the \\?\ prefix Windows drops a trailing dot, so "trail." and "trail" would be
        // one file. The prefix keeps them two.
        assert!(
            disk.write_durable(&scratch.join("trail."), b"dotted")
                .is_ok()
        );
        assert!(disk.write_durable(&scratch.join("trail"), b"plain").is_ok());
        assert_eq!(
            disk.read_file(&scratch.join("trail."))
                .expect("the dotted file is read"),
            b"dotted"
        );
        assert_eq!(
            disk.read_file(&scratch.join("trail"))
                .expect("the plain file is read"),
            b"plain"
        );
        assert!(disk.remove_file(&scratch.join("trail.")).is_ok());
        assert!(is_present(&scratch.join("trail")));
    }
}
