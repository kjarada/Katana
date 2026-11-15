//! Twin of tests/surveyio/test_rinex_compact.cpp: Compact RINEX (Hatanaka
//! compression) - the expansion in src/rinex_compact.rs, and the observation
//! reader on compact files.
//!
//! The expansion's arithmetic is invisible through the reader (it counts
//! observations; it keeps no values), so this file also reaches the reader's
//! private module to test the expansion itself.

// Fixtures in data/rinex/:
//   * rinex305_example1.crx - the specification's own example (Hatanaka 2008,
//     appendix 3: the RINEX 3.00 example of the RINEX specification,
//     compressed) as the published RNX2CRX ver.4.1.0 writes it from
//     rinex305_example1.rnx, up to the epoch at 13:14:12 (whose Doppler values
//     are not F14.3, which the tool refuses). Its lines are the ones the paper
//     prints, and the values below are worked out from them by hand;
//   * test2560.24d and the three .crx beside the .rnx of the same name - the
//     other synthetic fixtures, compressed with the same published tool. What
//     they must read as is what their plain files read as, which rinex.rs
//     works out by hand.

mod instrument_support;

use std::path::PathBuf;

use katana_core::{Error, ErrorCode, Result};
use katana_surveyio::rinex_internal::{CompactExpansion, expand_compact_rinex};
use katana_surveyio::{
    DetectionOutcome, MAX_SURVEY_FILE_BYTES, RINEX_OBSERVATION_FORMAT_ID, ReadOptions, ReadResult,
    SiblingFile, describe, detect_format, format_registry, probe_of, read_survey,
    siblings_in_memory,
};

use instrument_support::{atoi, data_folder};

fn fixture_folder() -> PathBuf {
    data_folder().join("rinex")
}

fn fixture(name: &str) -> Vec<u8> {
    let bytes = std::fs::read(fixture_folder().join(name));
    assert!(bytes.is_ok(), "missing fixture {name}");
    bytes.unwrap_or_default()
}

// The lines of a text, without their terminators: LF ends one, and a CR before
// it is dropped.
fn lines_of(text: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let end = text[at..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(text.len(), |offset| at + offset);
        let mut line = &text[at..end];
        if let Some(rest) = line.strip_suffix(b"\r") {
            line = rest;
        }
        lines.push(String::from_utf8_lossy(line).into_owned());
        at = end + 1;
    }
    lines
}

fn trimmed_right(text: &str) -> String {
    String::from(text.trim_end_matches(' '))
}

fn expand(bytes: &[u8], name: &str) -> CompactExpansion {
    expand_compact_rinex(bytes, name, MAX_SURVEY_FILE_BYTES)
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn read_rinex_with(bytes: &[u8], name: &str, siblings: Vec<SiblingFile>) -> Result<ReadResult> {
    let options = ReadOptions {
        siblings: Some(siblings_in_memory(siblings)),
        ..Default::default()
    };
    read_survey(
        format_registry(),
        RINEX_OBSERVATION_FORMAT_ID,
        bytes,
        name,
        &options,
    )
}

fn read_rinex(bytes: &[u8], name: &str) -> Result<ReadResult> {
    read_rinex_with(bytes, name, Vec::new())
}

fn ok(read: Result<ReadResult>) -> ReadResult {
    read.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn describe_warnings(result: &ReadResult) -> String {
    let mut text = String::new();
    for warning in &result.warnings {
        text.push_str(&describe(warning));
        text.push('\n');
    }
    text
}

// A header record: content in columns 1-60, label in 61-80.
fn header(content: &str, label: &str) -> String {
    let mut line = String::from(content);
    line.truncate(60);
    format!("{line:<60}{label}\n")
}

// The compact header of a RINEX 3.04 GPS file with two observation types. 9
// lines; the first epoch line is line 10.
fn compact_v3_header() -> String {
    [
        header(
            "3.0                 COMPACT RINEX FORMAT",
            "CRINEX VERS   / TYPE",
        ),
        header(
            "hand built                              24-Sep-26 12:00",
            "CRINEX PROG / DATE",
        ),
        header(
            "     3.04           OBSERVATION DATA    G",
            "RINEX VERSION / TYPE",
        ),
        header("katana test", "PGM / RUN BY / DATE"),
        header("M1", "MARKER NAME"),
        header(
            " -4646000.0000  2553000.0000 -3534000.0000",
            "APPROX POSITION XYZ",
        ),
        header(
            "        1.0000        0.0000        0.0000",
            "ANTENNA: DELTA H/E/N",
        ),
        header("G    2 C1C L1C", "SYS / # / OBS TYPES"),
        header("", "END OF HEADER"),
    ]
    .concat()
}

// The compact header of a RINEX 2.11 GPS file with two observation types (C1,
// L1). 9 lines; the first epoch line is line 10.
fn compact_v2_header() -> String {
    [
        header(
            "1.0                 COMPACT RINEX FORMAT",
            "CRINEX VERS   / TYPE",
        ),
        header(
            "RNX2CRX ver.4.1.0                       24-Sep-26 06:30",
            "CRINEX PROG / DATE",
        ),
        header(
            "     2.11           OBSERVATION DATA    G (GPS)",
            "RINEX VERSION / TYPE",
        ),
        header("katana test", "PGM / RUN BY / DATE"),
        header("M2", "MARKER NAME"),
        header(
            " -4646000.0000  2553000.0000 -3534000.0000",
            "APPROX POSITION XYZ",
        ),
        header(
            "        1.0000        0.0000        0.0000",
            "ANTENNA: DELTA H/E/N",
        ),
        header("     2    C1    L1", "# / TYPES OF OBSERV"),
        header("", "END OF HEADER"),
    ]
    .concat()
}

// Section 2.2's text difference, the compressor's side: what the expansion
// undoes.
fn text_difference(before: &str, after: &str) -> String {
    let before = before.as_bytes();
    let after = after.as_bytes();
    let mut difference = String::new();
    for i in 0..before.len().max(after.len()) {
        let old = before.get(i).copied().unwrap_or(b' ');
        let now = after.get(i).copied().unwrap_or(b' ');
        difference.push(if old == now {
            ' '
        } else if now == b' ' {
            '&'
        } else {
            char::from(now)
        });
    }
    trimmed_right(&difference)
}

// A RINEX 3 epoch line as Compact RINEX 3.0 holds it: columns 1-41 and then
// the satellite list.
fn compact_epoch_line(minute: i32, second: i32, count: i32, list: &str) -> String {
    format!(
        "> 2024 09 12 10 {minute:02}{:11.7}  0{count:3}      {list}",
        f64::from(second)
    )
}

// ---- The expansion -----------------------------------------------------------------

mod compact_rinex_expansion {
    use super::*;

    #[test]
    fn the_specifications_example_expands_to_the_values_its_differences_work_out_to() {
        let expanded = expand(&fixture("rinex305_example1.crx"), "rinex305_example1.crx");
        let lines = lines_of(&expanded.text);
        // The RINEX header is lines 3-35 of the compact file and 1-33 of the
        // expansion; epoch 1 is expanded line 34 and its five satellites 35-39.
        assert!(lines.len() >= 47);
        assert_eq!(&lines[0][..9], "     3.05");
        assert_eq!(&lines[32][60..], "END OF HEADER");
        // Epoch 1 is given whole ("3&-123456789012": order 3, -0.123456789012 s).
        assert_eq!(
            lines[33],
            "> 2006 03 24 13 10 36.0000000  0  5       -.123456789012"
        );
        // G06 "3&23629347915 3&300 3&-353 3&23629347158 3&24158 &&&8&4&&&&":
        // flags "&&&8&4&&&&" are LLI/SSI of C1C "  ", L1W " 8", L2W " 4".
        assert_eq!(
            lines[34],
            "G06  23629347.915            .300 8         -.353 4  23629347.158          24.158"
        );
        // Epoch 2: the epoch line's difference changes the seconds 36 -> 54, the
        // count 5 -> 7 and the list G06G09G12E11S20 -> G06G09G12R21R22E11S20; the
        // clock's first difference -198 gives -123456789012 - 198 = -123456789210.
        assert_eq!(
            lines[39],
            "> 2006 03 24 13 10 54.0000000  0  7       -.123456789210"
        );
        // G06 "-10252465 -53875932 -41981022 -10252150 1076", first differences
        // in thousandths:
        //   C1C 23629347.915 - 10252.465 = 23619095.450
        //   L1W       0.300 - 53875.932 =   -53875.632
        //   L2W      -0.353 - 41981.022 =   -41981.375
        //   C1W 23629347.158 - 10252.150 = 23619095.008
        //   S2W      24.158 +     1.076 =       25.234
        // and no flag text: the flags stay " 8" and " 4".
        assert_eq!(
            lines[40],
            "G06  23619095.450      -53875.632 8    -41981.375 4  23619095.008          25.234"
        );
        // G09 "-5458981 -28687907 -22354177 -5469191 4108      7": the flag text
        // after the fifth separator is "     7", so position 6 (L2W's SSI) becomes
        // 7 and the rest stay ("   9 6" -> "   9 7").
        //   C1C 20891534.648 - 5458.981 = 20886075.667
        //   L1W       -0.120 - 28687.907 = -28688.027
        //   L2W       -0.358 - 22354.177 = -22354.535
        //   C1W 20891545.292 - 5469.191 = 20886076.101
        //   S2W       38.123 + 4.108     =     42.231
        assert_eq!(
            lines[41],
            "G09  20886075.667      -28688.027 9    -22354.535 7  20886076.101          42.231"
        );
        // R21 and R22 are new, so start afresh: "3&21345678576 3&12345567 &&&5".
        assert_eq!(lines[43], "R21  21345678.576       12345.567 5");
        // E11 "65431799 48861408  5": 0.324 + 65431.799 = 65432.123 and
        // 0.178 + 48861.408 = 48861.586; flags "&8&7" (" 8 7") with " 5" over
        // them give " 5 7".
        assert_eq!(lines[45], "E11     65432.123 5     48861.586 7");
        // S20 "0 0": unchanged.
        assert_eq!(lines[46], "S20  38137559.506      335849.135 9");
        // The event (flag 2) and its two records as they were.
        assert_eq!(lines[47], "> 2006 03 24 13 11 12.0000000  2  2");
        assert_eq!(
            &lines[48][..42],
            "       *** FROM NOW ON KINEMATIC DATA! ***"
        );

        // Every line maps back to the compact file: compact line 36 is epoch 1,
        // 37 its clock, 38-42 its satellites, 43 epoch 2, 44 its clock, 45 G06.
        assert_eq!(expanded.lines.compact_line(1), 3);
        assert_eq!(expanded.lines.compact_line(33), 35);
        assert_eq!(expanded.lines.compact_line(34), 36);
        assert_eq!(expanded.lines.compact_line(35), 38);
        assert_eq!(expanded.lines.compact_line(40), 43);
        assert_eq!(expanded.lines.compact_line(41), 45);
        assert_eq!(expanded.lines.compact_line(48), 52, "the event line");
        assert_eq!(expanded.compact_lines, 75);
        assert_eq!(expanded.lines_skipped, 0);
        assert!(expanded.warnings.is_empty());
        assert_eq!(
            expanded.description,
            "Compact RINEX 3.0, written by RNX2CRX ver.4.1.0 on 24-Sep-26 05:16"
        );
    }

    #[test]
    fn every_compact_fixture_expands_to_the_plain_file_it_was_compressed_from() {
        // The published compressor's output, expanded here, gives back the plain
        // file line for line - trailing blanks aside, which Compact RINEX never
        // keeps (section 3), and a zero before a decimal point, which it cannot.
        let pairs = [
            ("test2560.24d", "test2560.24o"),
            (
                "TEST00AUS_R_20242561000_01H_30S_MO.crx",
                "TEST00AUS_R_20242561000_01H_30S_MO.rnx",
            ),
            (
                "ROVR00AUS_R_20242561100_01H_01S_GO.crx",
                "ROVR00AUS_R_20242561100_01H_01S_GO.rnx",
            ),
            (
                "BASE00AUS_R_20242561200_01H_30S_MO.crx",
                "BASE00AUS_R_20242561200_01H_30S_MO.rnx",
            ),
            ("rinex305_example1.crx", "rinex305_example1.rnx"),
        ];
        for (compact, plain) in pairs {
            let expanded = lines_of(&expand(&fixture(compact), compact).text);
            let mut original = lines_of(&fixture(plain));
            if compact == "rinex305_example1.crx" {
                // the example up to the epoch the tool refuses (line 70)
                original.resize(69, String::new());
            }
            assert_eq!(expanded.len(), original.len(), "{compact}");
            for (i, (got, want)) in expanded.iter().zip(&original).enumerate() {
                let mut want = trimmed_right(want);
                let mut got = got.clone();
                // " 0.000123456789" and "  .000123456789" are the same clock.
                for line in [&mut want, &mut got] {
                    let mut bytes = std::mem::take(line).into_bytes();
                    let mut at = 0;
                    while let Some(found) = find_from(&bytes, b" 0.", at) {
                        bytes[found + 1] = b' ';
                        at = found + 1;
                    }
                    let mut at = 0;
                    while let Some(found) = find_from(&bytes, b"-0.", at) {
                        bytes[found] = b' ';
                        bytes[found + 1] = b'-';
                        at = found + 1;
                    }
                    *line = String::from_utf8_lossy(&bytes).into_owned();
                }
                assert_eq!(got, want, "{compact} line {}", i + 1);
            }
        }
    }

    // std::string::find from a position.
    fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
        haystack
            .get(from..)?
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|offset| from + offset)
    }

    #[test]
    fn a_third_order_series_is_recovered_by_equation_three_as_it_ramps_up() {
        // One satellite, C1C only: "3&1000" starts a series of order 3 at 1.000;
        // each later field is the difference of the order the series has reached:
        //   epoch 2: 1st difference 10          -> Y1 = 10,           Y = 1010
        //   epoch 3: 2nd difference  1          -> Y1 = 10 + 1 = 11,  Y = 1021
        //   epoch 4: 3rd difference  0 -> Y2 = 1, Y1 = 12,            Y = 1033
        //   epoch 5: 3rd difference  2 -> Y2 = 3, Y1 = 15,            Y = 1048
        // (appendix 1, equation 3). L1C is blank throughout.
        // Epochs 30 s apart; each epoch line is given as its difference from the
        // one before (section 2.2), each has no clock offset (an empty line).
        let epochs = [
            compact_epoch_line(0, 0, 1, "G01"),
            compact_epoch_line(0, 30, 1, "G01"),
            compact_epoch_line(1, 0, 1, "G01"),
            compact_epoch_line(1, 30, 1, "G01"),
            compact_epoch_line(2, 0, 1, "G01"),
        ];
        let fields = ["3&1000", "10", "1", "0", "2"];
        let mut bytes = compact_v3_header();
        for i in 0..epochs.len() {
            let line = if i == 0 {
                epochs[0].clone()
            } else {
                text_difference(&epochs[i - 1], &epochs[i])
            };
            bytes.push_str(&format!("{line}\n\n{}\n", fields[i]));
        }
        let expanded = expand(bytes.as_bytes(), "ramp.crx");
        let lines = lines_of(&expanded.text);
        assert_eq!(
            lines.len(),
            7 + 10,
            "{}",
            String::from_utf8_lossy(&expanded.text)
        );
        assert_eq!(lines[7], "> 2024 09 12 10 00  0.0000000  0  1");
        assert_eq!(lines[8], "G01         1.000");
        assert_eq!(lines[9], "> 2024 09 12 10 00 30.0000000  0  1");
        assert_eq!(lines[10], "G01         1.010");
        assert_eq!(lines[11], "> 2024 09 12 10 01  0.0000000  0  1");
        assert_eq!(lines[12], "G01         1.021");
        assert_eq!(lines[14], "G01         1.033");
        assert_eq!(lines[15], "> 2024 09 12 10 02  0.0000000  0  1");
        assert_eq!(lines[16], "G01         1.048");
        assert!(expanded.warnings.is_empty());
    }

    #[test]
    fn damage_is_warned_once_with_the_lines_it_spoils_and_expansion_resumes_at_a_fresh_epoch() {
        // Lines 1-9 the header. 10-12: epoch 1, G01 starts both series. 13: an
        // optional record ('&'), skipped. 14-16: epoch 2 names G02, which was not
        // in epoch 1, yet gives it differences - nothing to apply them to (line
        // 16). 17-19: epoch 3, a difference of the spoiled epoch line. 20-22: a
        // fresh epoch, expanded again.
        let first = compact_epoch_line(0, 0, 1, "G01");
        let second = compact_epoch_line(0, 30, 1, "G02");
        let third = compact_epoch_line(1, 0, 1, "G02");
        let bytes = format!(
            "{}{first}\n\n3&1000 3&2000\n&reserved\n{}\n\n5 5\n{}\n\n5 5\n{}\n\n3&4000 3&5000\n",
            compact_v3_header(),
            text_difference(&first, &second),
            text_difference(&second, &third),
            compact_epoch_line(1, 30, 1, "G03")
        );
        let expanded = expand(bytes.as_bytes(), "damaged.crx");
        assert_eq!(expanded.warnings.len(), 1);
        assert_eq!(
            expanded.warnings[0].record, 16,
            "the line the damage was found at"
        );
        assert!(
            expanded.warnings[0].message.contains("no earlier value"),
            "{}",
            expanded.warnings[0].message
        );
        assert!(
            expanded.warnings[0]
                .message
                .contains("Lines 14 to 19 are left out"),
            "{}",
            expanded.warnings[0].message
        );
        assert_eq!(expanded.lines_skipped, 6);
        assert_eq!(expanded.optional_lines, 1);
        // The RINEX header is 7 lines; then epoch 1 and G01, epoch 4 and G03.
        let lines = lines_of(&expanded.text);
        assert_eq!(
            lines.len(),
            7 + 4,
            "{}",
            String::from_utf8_lossy(&expanded.text)
        );
        assert_eq!(lines[8], "G01         1.000           2.000");
        assert_eq!(lines[9], "> 2024 09 12 10 01 30.0000000  0  1");
        assert_eq!(lines[10], "G03         4.000           5.000");
        assert_eq!(expanded.lines.compact_line(10), 20);
        assert_eq!(expanded.lines.compact_line(11), 22);
    }

    #[test]
    fn an_epoch_listing_no_satellites_expands_whether_it_opens_the_file_or_follows_an_event() {
        // As the published RNX2CRX ver.4.1.0 writes it (from a plain file holding
        // the lines expected below): an epoch given in full with no satellites
        // loses its trailing blanks, so it is 35 characters, not the 41 before a
        // satellite list; the compressor pads it to 41 before taking the next
        // epoch's difference. Line 10 is the file's first epoch; line 20 an
        // external event (flag 5, no records); line 21 the first epoch after it,
        // given in full again.
        let body = [
            "> 2024 09 12 10 00  0.0000000  0  0\n",             // 10
            "\n",                                                // 11 no clock
            "                   3              2      G01G02\n", // 12
            "\n",                                                // 13
            "3&20000000001 3&105000000456 &7&7\n",               // 14 G01
            "3&21000000001 3&110000000500 &6&6\n",               // 15 G02
            "                 1 &\n",                            // 16
            "\n",                                                // 17
            "1 1\n",                                             // 18
            "2 2\n",                                             // 19
            "> 2024 09 12 10 01 15.0000000  5  0\n",             // 20 event
            "> 2024 09 12 10 01 30.0000000  0  0\n",             // 21
            "\n",                                                // 22
            "                 2 &              2      G01G02\n", // 23
            "\n",                                                // 24
            "3&20000000004 3&105000000460 &7&7\n",               // 25
            "3&21000000004 3&110000000504 &6&6\n",               // 26
            "                   3\n",                            // 27
            "\n",                                                // 28
            "2 3\n",                                             // 29
            "1 2\n",                                             // 30
        ]
        .concat();
        let bytes = compact_v3_header() + &body;
        let expanded = expand(bytes.as_bytes(), "empty.crx");
        assert!(
            expanded.warnings.is_empty(),
            "{}",
            expanded
                .warnings
                .first()
                .map(|warning| warning.message.as_str())
                .unwrap_or_default()
        );
        assert_eq!(expanded.lines_skipped, 0);
        let lines = lines_of(&expanded.text);
        // Worked by hand, column numbers 1-based:
        //   line 12 on line 10 padded to 41: column 20 '3' makes the seconds 30,
        //     column 35 '2' the count 2, columns 42-47 the list;
        //   G01 and G02 start their series at the values written (thousandths);
        //   line 16: column 18 '1' makes the minute 01, column 20 '&' blanks the
        //     '3' (seconds 0); first differences .001 and .001 (G01), .002 and
        //     .002 (G02);
        //   the event as it is; line 21 in full, and line 23 on it padded to 41:
        //     minute 02, seconds 0, count 2, the list; every series afresh;
        //   line 27: seconds 30; first differences .002/.003 and .001/.002.
        let want = [
            "> 2024 09 12 10 00  0.0000000  0  0",
            "> 2024 09 12 10 00 30.0000000  0  2",
            "G01  20000000.001 7 105000000.456 7",
            "G02  21000000.001 6 110000000.500 6",
            "> 2024 09 12 10 01  0.0000000  0  2",
            "G01  20000000.002 7 105000000.457 7",
            "G02  21000000.003 6 110000000.502 6",
            "> 2024 09 12 10 01 15.0000000  5  0",
            "> 2024 09 12 10 01 30.0000000  0  0",
            "> 2024 09 12 10 02  0.0000000  0  2",
            "G01  20000000.004 7 105000000.460 7",
            "G02  21000000.004 6 110000000.504 6",
            "> 2024 09 12 10 02 30.0000000  0  2",
            "G01  20000000.006 7 105000000.463 7",
            "G02  21000000.005 6 110000000.506 6",
        ];
        assert_eq!(
            lines.len(),
            7 + want.len(),
            "{}",
            String::from_utf8_lossy(&expanded.text)
        );
        for (i, want) in want.iter().enumerate() {
            assert_eq!(lines[7 + i], *want, "expanded line {}", 8 + i);
        }

        // Read: six epochs of flag 0 - 10:00:00, 10:00:30, 10:01:00, 10:01:30,
        // 10:02:00 and 10:02:30 - two of them empty, and every compact line read.
        let read = ok(read_rinex(bytes.as_bytes(), "empty.crx"));
        assert_eq!(read.project.gnss_sessions.len(), 1);
        assert_eq!(
            read.project.gnss_sessions[0].epoch_count,
            6,
            "{}",
            describe_warnings(&read)
        );
        assert_eq!(read.records_skipped, 0, "{}", describe_warnings(&read));
        assert_eq!(read.records_read, 30);
    }

    #[test]
    fn in_compact_rinex_one_a_flag_outlives_its_series_restarting_and_is_cleared_by_a_blank_or_a_new_satellite()
     {
        // Compact RINEX 1.0 (RINEX 2) as the published RNX2CRX ver.4.1.0 writes it
        // with "-e 3" (every series starts afresh at every third epoch) from the
        // plain file whose lines are expected below; its CRX2RNX gives that file
        // back byte for byte. What the compressor does with the LLI and signal
        // strength flags, and so what the expansion must undo:
        //   * a series restarting ("3&...") for a phase jump of more than 10^7
        //     leaves the flags alone - they are differenced against the old ones;
        //   * a blank observation blanks its flags on both sides (no '&' for it);
        //   * a satellite the compressor takes as new - every satellite on an
        //     epoch line given in full ('&'), and one whose three characters were
        //     not in the last epoch's list (' 01' is not 'G01') - has its flags
        //     given whole, blanks as blanks.
        let body = [
            "&24  9 12 10  0  0.0000000  0  2G01G02\n", // 10 epoch 1
            "\n",                                       // 11
            "3&20000000123 3&105000000456  7 7\n",      // 12 G01
            "3&21000000123 3&110000000500  6 6\n",      // 13 G02
            "                3\n",                      // 14 epoch 2
            "\n",                                       // 15
            "1000 3&1000456\n",                         // 16 L1 jumps
            "1000 10000\n",                             // 17
            "              1 &\n",                      // 18 epoch 3
            "\n",                                       // 19
            "0 100000   1\n",                           // 20 L1 LLI 1
            "0 0\n",                                    // 21
            "&24  9 12 10  1 30.0000000  0  2G01G02\n", // 22 epoch 4, full
            "\n",                                       // 23
            "3&20000003123 3&1200456  7 7\n",           // 24
            "3&21000003123 3&110000030500  6 6\n",      // 25
            "              2 &\n",                      // 26 epoch 5
            "\n",                                       // 27
            "1000 100000\n",                            // 28
            "1000\n",                                   // 29 G02 L1 blank
            "                3\n",                      // 30 epoch 6
            "\n",                                       // 31
            "0 0\n",                                    // 32
            "0 3&110000050500\n",                       // 33 G02 L1 back
            "&24  9 12 10  3  0.0000000  0  2G01G02\n", // 34 epoch 7, full
            "\n",                                       // 35
            "3&20000006123 3&1500456  717\n",           // 36
            "3&21000006123 3&110000060500  6\n",        // 37
            "                3               &\n",      // 38 epoch 8
            "\n",                                       // 39
            "3&20000007123 3&1600456  7 7\n",           // 40 ' 01'
            "1000 10000\n",                             // 41
        ]
        .concat();
        let bytes = compact_v2_header() + &body;
        let expanded = expand(bytes.as_bytes(), "flags.24d");
        assert!(
            expanded.warnings.is_empty(),
            "{}",
            expanded
                .warnings
                .first()
                .map(|warning| warning.message.as_str())
                .unwrap_or_default()
        );
        let lines = lines_of(&expanded.text);
        // Flags are two characters per type, C1 then L1: LLI, signal strength.
        let want = [
            " 24  9 12 10  0  0.0000000  0  2G01G02",
            "  20000000.123 7 105000000.456 7",
            "  21000000.123 6 110000000.500 6",
            " 24  9 12 10  0 30.0000000  0  2G01G02",
            // L1 restarts at 1000.456 with no flag text: its " 7" stays.
            "  20000001.123 7      1000.456 7",
            "  21000001.123 6 110000010.500 6",
            " 24  9 12 10  1  0.0000000  0  2G01G02",
            // Flag text "  1" (after the separator): L1's LLI becomes 1.
            "  20000002.123 7      1100.45617",
            "  21000002.123 6 110000020.500 6",
            " 24  9 12 10  1 30.0000000  0  2G01G02",
            // A full epoch line: G01 is new, its flags " 7 7" given whole, so
            // L1's LLI is blank again (a blank in the text is a blank here).
            "  20000003.123 7      1200.456 7",
            "  21000003.123 6 110000030.500 6",
            " 24  9 12 10  2  0.0000000  0  2G01G02",
            "  20000004.123 7      1300.456 7",
            // G02's L1 is blank, and with it its flags.
            "  21000004.123 6",
            " 24  9 12 10  2 30.0000000  0  2G01G02",
            "  20000005.123 7      1400.456 7",
            // L1 back with no flag text: the flags the blank cleared stay clear.
            "  21000005.123 6 110000050.500",
            " 24  9 12 10  3  0.0000000  0  2G01G02",
            "  20000006.123 7      1500.45617",
            "  21000006.123 6 110000060.500",
            // '&' in column 33 blanks the 'G': ' 01' is a satellite the
            // compressor had not seen, its flags " 7 7" given whole - not "717"
            // carried over from G01.
            " 24  9 12 10  3 30.0000000  0  2 01G02",
            "  20000007.123 7      1600.456 7",
            "  21000007.123 6 110000070.500",
        ];
        assert_eq!(
            lines.len(),
            7 + want.len(),
            "{}",
            String::from_utf8_lossy(&expanded.text)
        );
        for (i, want) in want.iter().enumerate() {
            assert_eq!(lines[7 + i], *want, "expanded line {}", 8 + i);
        }
    }

    #[test]
    fn damage_warnings_are_listed_up_to_the_readers_limit_and_the_rest_counted() {
        // 600 good one-satellite epochs, each followed by a bare '>' - an epoch
        // line with no event flag, which spoils only itself, since the next epoch
        // is given in full. 600 damaged stretches of one line each: the reader
        // lists 500 warnings (as it does for a plain file) and counts the other
        // 100 in one closing warning, rather than holding one per stretch - a
        // 1 GiB file of 40-byte stretches would otherwise hold millions.
        let mut bytes = compact_v3_header();
        for e in 0..600 {
            bytes.push_str(&format!(
                "> 2024 09 12 10 {:02}{:11.7}  0  1      G01\n\n3&1000 3&2000\n>\n",
                e / 60,
                f64::from(e % 60)
            ));
        }
        let expanded = expand(bytes.as_bytes(), "many.crx");
        assert_eq!(expanded.warnings.len(), 500);
        assert_eq!(expanded.unlisted_warnings, 100);
        assert_eq!(expanded.lines_skipped, 600);

        let read = ok(read_rinex(bytes.as_bytes(), "many.crx"));
        assert_eq!(read.project.gnss_sessions[0].epoch_count, 600);
        assert_eq!(read.warnings.len(), 501);
        assert_eq!(read.warnings[0].record, 13, "the first bare '>'");
        assert_eq!(
            read.warnings[500].message,
            "100 more warnings like these are not listed"
        );
        // 9 header lines and 4 a repetition: 2409, of which the 600 bare lines
        // are skipped.
        assert_eq!(read.records_skipped, 600);
        assert_eq!(read.records_read, 2409 - 600);
    }

    #[test]
    fn compact_headers_that_cannot_be_expanded_are_errors_that_say_why() {
        let error = |bytes: &str| -> Error {
            expand_compact_rinex(bytes.as_bytes(), "x.crx", MAX_SURVEY_FILE_BYTES)
                .expect_err("the header cannot be expanded")
        };
        let mut later = compact_v3_header();
        later.replace_range(0..3, "4.0");
        let version = error(&later);
        assert_eq!(version.code, ErrorCode::Unsupported);
        assert!(
            version.message.contains("version 4.0"),
            "{}",
            version.message
        );

        let mut mismatched = compact_v3_header();
        mismatched.replace_range(0..3, "1.0");
        let mismatch = error(&mismatched);
        assert_eq!(mismatch.code, ErrorCode::FileImportFailure);
        assert!(
            mismatch.message.contains("says RINEX 3.04"),
            "{}",
            mismatch.message
        );

        let mut unended = compact_v3_header();
        let end = unended.find("END OF HEADER").expect("the header ends") - 60;
        unended.truncate(end);
        assert!(error(&unended).message.contains("no END OF HEADER"));

        // The RINEX header expands to 7 lines of 60 columns, a label and a line
        // end: 420 + 121 + 7 = 548 bytes; the one epoch adds 36 + 18. A cap of 560
        // lets the header through and stops at the epoch.
        let tiny = compact_v3_header() + "> 2024 09 12 10 00  0.0000000  0  1      G01\n\n3&1000\n";
        let expanded = expand_compact_rinex(tiny.as_bytes(), "x.crx", 560)
            .expect_err("a cap on the expanded size");
        assert!(expanded.message.contains("expands to more than"));
    }
}

// ---- The reader on compact files ------------------------------------------------------

const COMPACT_PAIRS: [(&str, &str); 4] = [
    ("test2560.24d", "test2560.24o"),
    (
        "TEST00AUS_R_20242561000_01H_30S_MO.crx",
        "TEST00AUS_R_20242561000_01H_30S_MO.rnx",
    ),
    (
        "ROVR00AUS_R_20242561100_01H_01S_GO.crx",
        "ROVR00AUS_R_20242561100_01H_01S_GO.rnx",
    ),
    (
        "BASE00AUS_R_20242561200_01H_30S_MO.crx",
        "BASE00AUS_R_20242561200_01H_30S_MO.rnx",
    ),
];

mod compact_rinex {
    use super::*;

    #[test]
    fn every_compact_fixture_is_identified_as_rinex_observation_with_its_compact_header() {
        for name in [
            "test2560.24d",
            "TEST00AUS_R_20242561000_01H_30S_MO.crx",
            "ROVR00AUS_R_20242561100_01H_01S_GO.crx",
            "BASE00AUS_R_20242561200_01H_30S_MO.crx",
            "rinex305_example1.crx",
        ] {
            let bytes = fixture(name);
            let detection = detect_format(&probe_of(&bytes, name, false));
            assert_eq!(
                detection.outcome(),
                DetectionOutcome::Identified,
                "{name}: {}",
                detection.summary()
            );
            assert_eq!(
                detection.format().expect("identified").id,
                RINEX_OBSERVATION_FORMAT_ID,
                "{name}"
            );
            let evidence = &detection.candidates()[0].evidence;
            assert!(evidence.contains("CRINEX VERS / TYPE"), "{evidence}");
            assert!(evidence.contains("observation data"), "{evidence}");
        }
    }

    #[test]
    fn each_compact_fixture_reads_as_the_plain_file_it_compresses_with_warnings_at_its_own_lines() {
        for (compact_name, plain_name) in COMPACT_PAIRS {
            let compact_bytes = fixture(compact_name);
            let compact = ok(read_rinex(&compact_bytes, compact_name));
            let plain = ok(read_rinex(&fixture(plain_name), plain_name));
            let a = &compact.project;
            let b = &plain.project;
            assert_eq!(
                a.gnss_sessions.len(),
                b.gnss_sessions.len(),
                "{compact_name}"
            );
            for (x, y) in a.gnss_sessions.iter().zip(&b.gnss_sessions) {
                assert_eq!(x.marker_name, y.marker_name, "{compact_name}");
                assert_eq!(x.epoch_count, y.epoch_count, "{compact_name}");
                assert_eq!(x.first_epoch, y.first_epoch, "{compact_name}");
                assert_eq!(x.last_epoch, y.last_epoch, "{compact_name}");
                assert_eq!(x.interval_seconds, y.interval_seconds, "{compact_name}");
                assert_eq!(
                    x.satellites_per_system, y.satellites_per_system,
                    "{compact_name}"
                );
                assert_eq!(x.antenna, y.antenna, "{compact_name}");
                assert_eq!(x.receiver_type, y.receiver_type, "{compact_name}");
                assert_eq!(
                    x.approximate_position, y.approximate_position,
                    "{compact_name}"
                );
                assert_eq!(x.source.file_name, compact_name);
            }
            assert_eq!(a.observations.len(), b.observations.len(), "{compact_name}");
            assert_eq!(
                a.unpositioned_points.len(),
                b.unpositioned_points.len(),
                "{compact_name}"
            );
            let mut metadata = a.metadata.clone();
            assert!(
                metadata
                    .get("compression")
                    .is_some_and(|compression| compression.contains("Compact RINEX"))
            );
            metadata.remove("compression");
            let mut plain_metadata = b.metadata.clone();
            // Neither was read with its navigation file; what was looked for is
            // named after each file.
            metadata.remove("navigation files");
            plain_metadata.remove("navigation files");
            assert_eq!(metadata, plain_metadata, "{compact_name}");
            assert_eq!(compact.not_carried, plain.not_carried, "{compact_name}");
            // The same warnings, each at the compact file's own line.
            assert_eq!(
                compact.warnings.len(),
                plain.warnings.len(),
                "{compact_name}\n{}",
                describe_warnings(&compact)
            );
            let compact_lines = lines_of(&compact_bytes);
            for (x, y) in compact.warnings.iter().zip(&plain.warnings) {
                assert_eq!(x.message, y.message, "{compact_name}");
                assert!(x.record <= compact_lines.len(), "{compact_name}");
            }
            // Every compact line read, none skipped.
            assert_eq!(compact.records_read, compact_lines.len(), "{compact_name}");
            assert_eq!(compact.records_skipped, 0, "{compact_name}");
        }
    }

    #[test]
    fn the_examples_kinematic_warning_names_the_compact_line_of_its_event() {
        // In the plain example the flag 2 event is line 48; in the compact one it
        // is line 52 (35 header lines; epoch 1 = 1 + clock + 5 = 36-42; epoch 2 =
        // 1 + clock + 7 = 43-51).
        let read = ok(read_rinex(
            &fixture("rinex305_example1.crx"),
            "rinex305_example1.crx",
        ));
        let kinematic = read
            .warnings
            .iter()
            .find(|warning| warning.message.contains("antenna moving"))
            .unwrap_or_else(|| panic!("{}", describe_warnings(&read)));
        assert_eq!(kinematic.record, 52);
        assert_eq!(read.project.gnss_sessions.len(), 2);
        assert_eq!(read.project.gnss_sessions[0].epoch_count, 2);
        assert_eq!(read.project.gnss_sessions[1].marker_name, "A 9081");
        // The session's own record is its MARKER NAME: plain line 57, and in the
        // compact file 57 + 2 header lines + the clock lines of the three epochs
        // before it = 62.
        assert_eq!(read.project.gnss_sessions[1].source.record_number, 62);
    }

    #[test]
    fn the_navigation_file_beside_a_compact_file_is_found_by_its_plain_name() {
        // BASE..._MO.crx -> BASE..._MN.rnx (navigation files are never compact),
        // test2560.24d -> test2560.24n.
        let long_name = "BASE00AUS_R_20242561200_01H_30S_MO.crx";
        let nav = "BASE00AUS_R_20242561200_01H_MN.rnx";
        let base = ok(read_rinex_with(
            &fixture(long_name),
            long_name,
            vec![SiblingFile {
                name: String::from(nav),
                bytes: fixture(nav),
            }],
        ));
        let files = &base.project.metadata["navigation files"];
        assert!(files.contains(nav), "{files}");

        let short_name = ok(read_rinex_with(
            &fixture("test2560.24d"),
            "test2560.24d",
            vec![SiblingFile {
                name: String::from("test2560.24n"),
                bytes: fixture("test2560.24n"),
            }],
        ));
        assert!(short_name.project.metadata["navigation files"].contains("test2560.24n"));
    }

    #[test]
    fn damage_in_a_compact_file_is_a_warning_at_its_line_and_the_lines_after_it_are_skipped() {
        // test2560.24d: epoch 2 is its epoch line (36), clock line (37) and five
        // satellites (38-42), then an event at line 43. G02's line (39) replaced
        // by a word spoils the epoch; nothing after it can be expanded until the
        // event restarts every series.
        let mut lines = lines_of(&fixture("test2560.24d"));
        assert_eq!(lines[38], "0 0 0 0 0 0");
        lines[38] = String::from("zero");
        let mut bytes = String::new();
        for line in &lines {
            bytes.push_str(line);
            bytes.push('\n');
        }
        let read = ok(read_rinex(bytes.as_bytes(), "test2560.24d"));
        assert!(!read.warnings.is_empty());
        assert_eq!(read.warnings[0].record, 39, "{}", describe_warnings(&read));
        assert!(
            read.warnings[0].message.contains("'zero' is not a number"),
            "{}",
            read.warnings[0].message
        );
        assert!(
            read.warnings[0]
                .message
                .contains("Lines 36 to 42 are left out"),
            "{}",
            read.warnings[0].message
        );
        assert_eq!(read.records_skipped, 7);
        assert_eq!(read.records_read, lines.len() - 7);
    }
}

// ---- Throughput -------------------------------------------------------------------------

// A synthetic Compact RINEX 3.0 file at 1 Hz: 12 GPS satellites with eight
// observation types, each a third-order series whose third differences
// alternate 1, -1 - about 1.6 kB of RINEX an epoch from about 350 bytes.
fn synthetic_compact(epochs: usize) -> String {
    let mut text = [
        header(
            "3.0                 COMPACT RINEX FORMAT",
            "CRINEX VERS   / TYPE",
        ),
        header(
            "katana test                             24-Sep-26 12:00",
            "CRINEX PROG / DATE",
        ),
        header(
            "     3.04           OBSERVATION DATA    G",
            "RINEX VERSION / TYPE",
        ),
        header("katana test", "PGM / RUN BY / DATE"),
        header("BIG", "MARKER NAME"),
        header(
            " -4646000.0000  2553000.0000 -3534000.0000",
            "APPROX POSITION XYZ",
        ),
        header(
            "        1.0000        0.0000        0.0000",
            "ANTENNA: DELTA H/E/N",
        ),
        header(
            "G    8 C1C L1C D1C S1C C2W L2W D2W S2W",
            "SYS / # / OBS TYPES",
        ),
        header("     1.000", "INTERVAL"),
        header(
            "  2024     9    12     0     0    0.0000000     GPS",
            "TIME OF FIRST OBS",
        ),
        header("", "END OF HEADER"),
    ]
    .concat();
    let mut list = String::new();
    for n in 1..=12 {
        list.push_str(&format!("G{n:02}"));
    }
    text.reserve(epochs * 400);
    let mut previous = String::new();
    for e in 0..epochs {
        let epoch = format!(
            "> 2024 09 12 {:02} {:02}{:11.7}  0 12      {list}",
            (e / 3600) % 24,
            (e / 60) % 60,
            (e % 60) as f64
        );
        text.push_str(&if e == 0 {
            epoch.clone()
        } else {
            text_difference(&previous, &epoch)
        });
        text.push_str("\n\n"); // and no clock offset
        previous = epoch;
        for _ in 0..12 {
            if e == 0 {
                text.push_str(
                    "3&21000000123 3&110000001500 3&-2000250 3&44500 3&21000004123 \
                     3&85000005500 3&-1500250 3&40250\n",
                );
            } else if e == 1 {
                text.push_str("1000 1000 -10 0 1000 1000 -10 0\n");
            } else if e.is_multiple_of(2) {
                text.push_str("1 1 1 0 1 1 1 0\n");
            } else {
                text.push_str("-1 -1 -1 0 -1 -1 -1 0\n");
            }
        }
    }
    text
}

mod compact_rinex_throughput {
    use super::*;

    #[test]
    fn a_large_compact_file_is_expanded_and_counted_at_speed() {
        // 30 000 epochs (8 h 20 min at 1 Hz) is about 11 MB compact, 50 MB
        // expanded. KATANA_RINEX_THROUGHPUT_MB scales the expanded size.
        let megabytes = match std::env::var("KATANA_RINEX_THROUGHPUT_MB") {
            Ok(wanted) => atoi(&wanted).max(1) as usize,
            Err(_) => 50,
        };
        let epochs = (megabytes * 600).min(86_400);
        let bytes = synthetic_compact(epochs);
        let start = std::time::Instant::now();
        let read = read_rinex(bytes.as_bytes(), "BIG00AUS_R_20242560000_01D_01S_MO.crx");
        let elapsed = start.elapsed();
        let read = ok(read);
        assert!(read.warnings.is_empty(), "{}", describe_warnings(&read));
        let session = &read.project.gnss_sessions[0];
        assert_eq!(session.epoch_count, epochs);
        assert_eq!(session.satellites_per_system["GPS"], 12);
        assert_eq!(
            read.project.metadata["satellite records"],
            format!("{}", epochs * 12)
        );
        assert_eq!(read.records_skipped, 0);
        let seconds = elapsed.as_secs_f64().max(1e-9);
        let compact_rate = bytes.len() as f64 / 1e6 / seconds;
        // The expansion alone, to say where the time goes.
        let expand_start = std::time::Instant::now();
        let expanded = expand_compact_rinex(bytes.as_bytes(), "big.crx", MAX_SURVEY_FILE_BYTES);
        let expand_seconds = expand_start.elapsed().as_secs_f64().max(1e-9);
        let expanded = expanded.unwrap_or_else(|error| panic!("{}", error.describe()));
        let expanded_rate = expanded.text.len() as f64 / 1e6 / seconds;
        println!(
            "[ CRINEX   ] read {} MB compact ({} MB of RINEX), {epochs} epochs in {seconds} s: \
             {compact_rate} MB/s compact, {expanded_rate} MB/s of RINEX; the expansion alone \
             {expand_seconds} s",
            bytes.len() as f64 / 1e6,
            expanded.text.len() as f64 / 1e6
        );
        println!(
            "compact_megabytes_per_second={}",
            katana_core::text::format_exact_real(compact_rate)
        );
    }
}
