//! The range merger and, beside it, the text of the Range header and the check of the answer to
//! it (the twin of tests/update/test_ranges.cpp).
//!
//! Every expectation is worked out by hand from the rules the module states - a gap of at most
//! 1 MiB merges, a merged range is at most 32 MiB - at the byte: the boundary, and one past it.
//! The header text and the Content-Range grammar are RFC 9110 (14.1.2 and 14.4): the last byte
//! position is inclusive, so a range of `length` bytes from `offset` ends at offset + length - 1.

#![allow(
    clippy::module_inception,
    reason = "The inner module's name is the C++ suite's, for the parity checker."
)]

mod ranges {
    use katana_core::{ErrorCode, Status};
    use katana_update::ranges::{
        MergedRange, RANGE_MAX_LENGTH, RANGE_MERGE_GAP, RangeMember, check_range_response,
        merge_ranges, range_header_value,
    };

    const MIB: u64 = 1024 * 1024;

    fn member(file_index: usize, offset: u64, length: u64) -> RangeMember {
        RangeMember {
            file_index,
            offset,
            length,
        }
    }

    // merge_ranges with the C++'s default gap and cap.
    fn merge(members: Vec<RangeMember>) -> Vec<MergedRange> {
        merge_ranges(members, RANGE_MERGE_GAP, RANGE_MAX_LENGTH)
    }

    // The indexes of a range's members, in order.
    fn indexes(range: &MergedRange) -> Vec<usize> {
        range.members.iter().map(|m| m.file_index).collect()
    }

    // ---- the constants the module fixes ------------------------------------------------------

    #[test]
    fn the_defaults_are_a_mebibyte_of_gap_and32_mebibytes_of_range() {
        // Measured against the release host on 2026-10-08: ~150 ms a request at ~6 MB/s.
        assert_eq!(RANGE_MERGE_GAP, 1_048_576);
        assert_eq!(RANGE_MAX_LENGTH, 33_554_432);
    }

    // ---- the shape of the answer -------------------------------------------------------------

    #[test]
    fn no_members_are_no_ranges() {
        assert!(merge(Vec::new()).is_empty());
    }

    #[test]
    fn one_member_is_one_range_that_is_exactly_it() {
        let ranges = merge(vec![member(7, 44, 85)]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].offset, 44);
        assert_eq!(ranges[0].length, 85);
        assert_eq!(indexes(&ranges[0]), [7]);
        assert_eq!(ranges[0].members[0].offset, 44);
        assert_eq!(ranges[0].members[0].length, 85);
    }

    #[test]
    fn members_that_touch_merge_into_the_range_that_covers_both() {
        // [0,10) and [10,25): no gap.
        let ranges = merge(vec![member(0, 0, 10), member(1, 10, 15)]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].offset, 0);
        assert_eq!(ranges[0].length, 25);
        assert_eq!(indexes(&ranges[0]), [0, 1]);
    }

    #[test]
    fn the_range_covers_the_holes_between_its_members() {
        // [100,110), hole of 50, [160,170), hole of 30, [200,230): the range is [100,230).
        let ranges = merge(vec![
            member(0, 100, 10),
            member(1, 160, 10),
            member(2, 200, 30),
        ]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].offset, 100);
        assert_eq!(ranges[0].length, 130);
        assert_eq!(indexes(&ranges[0]), [0, 1, 2]);
    }

    // ---- the gap threshold -------------------------------------------------------------------

    #[test]
    fn a_gap_of_exactly_a_mebibyte_merges_and_one_byte_more_does_not() {
        // [0,100) then a hole of exactly 1 MiB, then 100 bytes. "At most" 1 MiB merges (the
        // contract): at 1 MiB the latency of a request and the bytes of the hole cost the same, so
        // the boundary is a tie, and the design's "under 1 MiB apart" is the same rule spoken
        // loosely.
        let second = 100 + MIB;
        let merged = merge(vec![member(0, 0, 100), member(1, second, 100)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].length, second + 100);

        let split = merge(vec![member(0, 0, 100), member(1, second + 1, 100)]);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].offset, 0);
        assert_eq!(split[0].length, 100);
        assert_eq!(split[1].offset, second + 1);
        assert_eq!(split[1].length, 100);
        assert_eq!(indexes(&split[0]), [0]);
        assert_eq!(indexes(&split[1]), [1]);
    }

    #[test]
    fn the_gap_threshold_is_the_callers_when_they_give_one() {
        let members = vec![member(0, 0, 10), member(1, 15, 10)]; // a hole of 5
        assert_eq!(merge_ranges(members.clone(), 5, RANGE_MAX_LENGTH).len(), 1);
        assert_eq!(merge_ranges(members, 4, RANGE_MAX_LENGTH).len(), 2);
        // Zero: only members that touch (or overlap) merge.
        assert_eq!(
            merge_ranges(
                vec![member(0, 0, 10), member(1, 10, 10)],
                0,
                RANGE_MAX_LENGTH
            )
            .len(),
            1
        );
        assert_eq!(
            merge_ranges(
                vec![member(0, 0, 10), member(1, 11, 10)],
                0,
                RANGE_MAX_LENGTH
            )
            .len(),
            2
        );
    }

    // ---- the 32 MiB cap ----------------------------------------------------------------------

    #[test]
    fn two_adjacent_members_filling_exactly_the32_mebibyte_cap_merge() {
        let half = 16 * MIB;
        let ranges = merge(vec![member(0, 0, half), member(1, half, half)]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].length, 32 * MIB);
    }

    #[test]
    fn one_byte_over_the_cap_splits_where_the_member_boundary_is() {
        let half = 16 * MIB;
        let ranges = merge(vec![member(0, 0, half), member(1, half, half + 1)]);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].length, half);
        assert_eq!(ranges[1].offset, half);
        assert_eq!(ranges[1].length, half + 1);
    }

    #[test]
    fn the_hole_counts_toward_the_cap() {
        // 30 MiB, a hole of 1 MiB (within the gap limit), 1 MiB: 32 MiB in all - merges.
        let fits = merge(vec![member(0, 0, 30 * MIB), member(1, 31 * MIB, MIB)]);
        assert_eq!(fits.len(), 1);
        assert_eq!(fits[0].length, 32 * MIB);
        // One byte more of member and the hole is no longer worth it: two requests.
        let over = merge(vec![member(0, 0, 30 * MIB), member(1, 31 * MIB, MIB + 1)]);
        assert_eq!(over.len(), 2);
    }

    #[test]
    fn several_small_members_fill_one_range_then_start_the_next() {
        // Forty members of 1 MiB, touching: 40 MiB. The first 32 fill a range, the other 8 the
        // next.
        let members: Vec<RangeMember> = (0..40).map(|i| member(i, i as u64 * MIB, MIB)).collect();
        let ranges = merge(members);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].length, 32 * MIB);
        assert_eq!(ranges[0].members.len(), 32);
        assert_eq!(ranges[1].offset, 32 * MIB);
        assert_eq!(ranges[1].length, 8 * MIB);
        assert_eq!(ranges[1].members.len(), 8);
    }

    #[test]
    fn a_member_larger_than_the_cap_is_a_range_of_its_own_and_is_not_split() {
        // 1 MiB, then 40 MiB, then 1 MiB, all touching: the big one stands alone, its neighbours
        // do not join it, and they are not joined to each other across it.
        let ranges = merge(vec![
            member(0, 0, MIB),
            member(1, MIB, 40 * MIB),
            member(2, 41 * MIB, MIB),
        ]);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[1].offset, MIB);
        assert_eq!(ranges[1].length, 40 * MIB);
        assert_eq!(indexes(&ranges[1]), [1]);
        assert_eq!(indexes(&ranges[0]), [0]);
        assert_eq!(indexes(&ranges[2]), [2]);
    }

    #[test]
    fn the_cap_is_the_callers_when_they_give_one() {
        let members = vec![member(0, 0, 10), member(1, 10, 10)];
        assert_eq!(merge_ranges(members.clone(), RANGE_MERGE_GAP, 20).len(), 1);
        assert_eq!(merge_ranges(members.clone(), RANGE_MERGE_GAP, 19).len(), 2);
        // A cap of zero: every member is its own range, none is dropped.
        assert_eq!(merge_ranges(members, RANGE_MERGE_GAP, 0).len(), 2);
    }

    // ---- sorting, overlap, zero length -------------------------------------------------------

    // The next permutation in lexicographic order (std::next_permutation); false after the last.
    fn next_permutation(order: &mut [usize]) -> bool {
        let Some(pivot) = (1..order.len()).rev().find(|&i| order[i - 1] < order[i]) else {
            order.reverse();
            return false;
        };
        let pivot = pivot - 1;
        let successor = (pivot + 1..order.len())
            .rev()
            .find(|&i| order[i] > order[pivot])
            .expect("a larger element follows the pivot");
        order.swap(pivot, successor);
        order[pivot + 1..].reverse();
        true
    }

    #[test]
    fn members_in_any_order_give_the_same_ranges() {
        // Four members, 24 orders: one answer. Offsets 0, 50, 2 MiB + 100 (a gap too big),
        // 2 MiB + 150.
        let base = [
            member(10, 0, 50),
            member(11, 50, 40),
            member(12, 2 * MIB + 100, 30),
            member(13, 2 * MIB + 150, 20),
        ];
        let mut order: Vec<usize> = (0..base.len()).collect();
        let mut orders = 0;
        loop {
            let shuffled: Vec<RangeMember> = order.iter().map(|&i| base[i]).collect();
            let ranges = merge(shuffled);
            assert_eq!(ranges.len(), 2);
            assert_eq!(ranges[0].offset, 0);
            assert_eq!(ranges[0].length, 90);
            assert_eq!(indexes(&ranges[0]), [10, 11]);
            assert_eq!(ranges[1].offset, 2 * MIB + 100);
            assert_eq!(ranges[1].length, 70);
            assert_eq!(indexes(&ranges[1]), [12, 13]);
            orders += 1;
            if !next_permutation(&mut order) {
                break;
            }
        }
        assert_eq!(orders, 24);
    }

    #[test]
    fn overlapping_members_are_merged_once_each() {
        // [0,100) and [50,150) overlap; [10,20) lies inside the first.
        let ranges = merge(vec![
            member(0, 0, 100),
            member(1, 50, 100),
            member(2, 10, 10),
        ]);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].offset, 0);
        assert_eq!(ranges[0].length, 150);
        // In offset order; each member appears once.
        assert_eq!(indexes(&ranges[0]), [0, 2, 1]);
    }

    #[test]
    fn a_member_inside_another_never_shortens_the_range() {
        // [0,100) holds [10,20); the third member starts exactly the gap limit past 100, the END
        // of the first - and 1 MiB - 80 bytes past 20, the end of the second.
        let ranges = merge(vec![
            member(0, 0, 100),
            member(1, 10, 10),
            member(2, 100 + MIB, 10),
        ]);
        assert_eq!(
            ranges.len(),
            1,
            "the gap is measured from the furthest end so far, 100"
        );
        assert_eq!(ranges[0].length, 100 + MIB + 10);
    }

    #[test]
    fn the_cap_still_wins_over_an_overlap() {
        // [0,20) and [10,30) MiB overlap and together span 30 MiB, within the cap. A third member,
        // [20,50) MiB, overlaps them too, but joining would make a 50 MiB request: it stands
        // alone.
        let ranges = merge(vec![
            member(0, 0, 20 * MIB),
            member(1, 10 * MIB, 20 * MIB),
            member(2, 20 * MIB, 30 * MIB),
        ]);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].length, 30 * MIB);
        assert_eq!(indexes(&ranges[0]), [0, 1]);
        assert_eq!(indexes(&ranges[1]), [2]);
    }

    #[test]
    fn a_member_with_no_bytes_is_dropped_and_bridges_nothing() {
        // [0,10) and [10 + 2 MiB, ...) are too far apart; a zero-length member in between, at the
        // end of the first, must not make them neighbours, and is in no range.
        let ranges = merge(vec![
            member(0, 0, 10),
            member(1, 10, 0),
            member(2, 12 * MIB, 0),
            member(3, 10 + 2 * MIB, 10),
        ]);
        assert_eq!(ranges.len(), 2);
        for range in &ranges {
            for m in &range.members {
                assert_ne!(m.length, 0);
                assert_ne!(m.file_index, 1);
                assert_ne!(m.file_index, 2);
            }
        }
        assert_eq!(indexes(&ranges[0]), [0]);
        assert_eq!(indexes(&ranges[1]), [3]);
        // Only zero-length members: nothing to ask for.
        assert!(merge(vec![member(0, 5, 0), member(1, 9, 0)]).is_empty());
    }

    #[test]
    fn a_member_whose_end_does_not_fit_sixty_four_bits_is_its_own_range_unchanged() {
        let top = u64::MAX;
        let ranges = merge(vec![member(0, top - 4, 10), member(1, top - 100, 50)]);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].offset, top - 100);
        assert_eq!(ranges[0].length, 50);
        assert_eq!(ranges[1].offset, top - 4);
        assert_eq!(ranges[1].length, 10); // not clamped to fit: nobody can ask for it
        // And no request can be made for it.
        assert!(range_header_value(top - 4, 10).is_err());
    }

    // ---- the properties, over many member sets -----------------------------------------------

    #[test]
    fn every_member_is_in_exactly_one_range_and_the_ranges_obey_the_rules() {
        // A deterministic pseudo-random walk (a 64-bit LCG, Knuth's MMIX constants) lays members
        // end to end with holes chosen from the sizes that matter: 0, exactly the gap limit, one
        // over it, small, large; lengths from tiny to well over the cap.
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = |bound: u64| -> u64 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % bound
        };
        let holes: [u64; 6] = [0, 1, RANGE_MERGE_GAP, RANGE_MERGE_GAP + 1, 4096, 3 * MIB];
        let lengths: [u64; 7] = [1, 100, 4096, MIB, 9 * MIB, 31 * MIB + 7, 40 * MIB];

        let mut sets = 0usize;
        let mut split_by_gap = 0usize;
        let mut split_by_cap = 0usize;
        let mut merged = 0usize;
        for round in 0..400 {
            let count = 1 + next(12) as usize;
            let mut members = Vec::new();
            let mut at = next(1000);
            for i in 0..count {
                at += holes[next(holes.len() as u64) as usize];
                let length = lengths[next(lengths.len() as u64) as usize];
                members.push(member(i, at, length));
                at += length;
            }
            // Present them in a scrambled order; the answer must not care.
            let mut given = members.clone();
            for i in (2..=given.len()).rev() {
                let j = next(i as u64) as usize;
                given.swap(i - 1, j);
            }
            let ranges = merge(given);
            sets += 1;

            // Every member in exactly one range.
            let mut seen: Vec<usize> = Vec::new();
            for range in &ranges {
                assert!(!range.members.is_empty());
                for m in &range.members {
                    seen.push(m.file_index);
                    // The range covers it.
                    assert!(m.offset >= range.offset);
                    assert!(m.offset + m.length <= range.offset + range.length);
                }
                // The range starts at its first member and ends at its furthest end: no slack.
                assert_eq!(range.offset, range.members[0].offset);
                let end = range
                    .members
                    .iter()
                    .map(|m| m.offset + m.length)
                    .max()
                    .unwrap_or(0);
                assert_eq!(range.offset + range.length, end);
                // The cap, unless the range is a single member.
                if range.members.len() > 1 {
                    assert!(range.length <= RANGE_MAX_LENGTH);
                }
            }
            let mut wanted: Vec<usize> = members.iter().map(|m| m.file_index).collect();
            seen.sort_unstable();
            wanted.sort_unstable();
            assert_eq!(seen, wanted, "round {round}");

            // Ranges in offset order; neighbours are neighbours for a reason.
            for pair in ranges.windows(2) {
                let (before, after) = (&pair[0], &pair[1]);
                assert!(
                    after.offset >= before.offset + before.length,
                    "round {round}"
                );
                let gap = after.offset - (before.offset + before.length);
                if gap > RANGE_MERGE_GAP {
                    split_by_gap += 1;
                } else {
                    // Close enough to merge, so it was the cap that kept them apart: the first
                    // member of the next range would have made the request longer than the cap.
                    let first = &after.members[0];
                    assert!(
                        first.offset + first.length - before.offset > RANGE_MAX_LENGTH,
                        "round {round}"
                    );
                    split_by_cap += 1;
                }
            }
            merged += members.len() - ranges.len();
        }
        // The cases the property claims to cover were reached.
        assert_eq!(sets, 400);
        assert!(split_by_gap > 50);
        assert!(split_by_cap > 50);
        assert!(merged > 200);
    }

    // ---- the Range header --------------------------------------------------------------------

    #[test]
    fn the_range_header_names_the_first_and_the_last_byte() {
        // "bytes=first-last", last inclusive: 85 bytes from 44 end at 128; one byte at 0 is 0-0.
        assert_eq!(range_header_value(44, 85).unwrap(), "bytes=44-128");
        assert_eq!(range_header_value(0, 1).unwrap(), "bytes=0-0");
        assert_eq!(range_header_value(0, 32 * MIB).unwrap(), "bytes=0-33554431");
        // Beyond 32 bits: 2^40 + 9 bytes from 2^40 is 2^40 .. 2^40 + 8.
        assert_eq!(
            range_header_value(1_099_511_627_776, 9).unwrap(),
            "bytes=1099511627776-1099511627784"
        );
        // Ten bytes ending at the last position 64 bits can name (2^64 - 2): 2^64 - 11 .. 2^64 - 2.
        let top = u64::MAX;
        assert_eq!(
            range_header_value(top - 10, 10).unwrap(),
            "bytes=18446744073709551605-18446744073709551614"
        );
    }

    #[test]
    fn a_range_header_is_not_made_for_nothing_or_for_an_end_past_sixty_four_bits() {
        assert_eq!(
            range_header_value(10, 0).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        let top = u64::MAX;
        assert_eq!(
            range_header_value(top, 2).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            range_header_value(top - 4, 10).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- the answer to a ranged request ------------------------------------------------------

    // A payload of 3791 bytes (the v2 fixture zip), and a range of its bytes 44 to 128.
    const PAYLOAD: u64 = 3791;
    const OFFSET: u64 = 44;
    const LENGTH: u64 = 85;

    fn answer(status: i32, content_range: &str, body: u64) -> Status {
        check_range_response(status, content_range, body, OFFSET, LENGTH, PAYLOAD)
    }

    fn answer_for(status: i32, content_range: &str, body: u64, offset: u64, length: u64) -> Status {
        check_range_response(status, content_range, body, offset, length, PAYLOAD)
    }

    #[test]
    fn a_partial_answer_that_is_exactly_the_range_asked_for_is_accepted() {
        assert!(answer(206, "bytes 44-128/3791", 85).is_ok());
        // The total may be unknown, the unit is not case-sensitive, blanks around the field are
        // not part of it.
        assert!(answer(206, "bytes 44-128/*", 85).is_ok());
        assert!(answer(206, "Bytes 44-128/3791", 85).is_ok());
        assert!(answer(206, "BYTES 44-128/3791", 85).is_ok());
        assert!(answer(206, "  bytes 44-128/3791 \t", 85).is_ok());
        // The first byte, and the last byte of the file.
        assert!(answer_for(206, "bytes 0-0/3791", 1, 0, 1).is_ok());
        assert!(answer_for(206, "bytes 3790-3790/3791", 1, 3790, 1).is_ok());
    }

    #[test]
    fn a_partial_answer_for_another_part_of_the_file_is_refused() {
        for header in [
            "bytes 45-129/3791",
            "bytes 43-127/3791",
            "bytes 44-127/3791",
            "bytes 44-129/3791",
            "bytes 0-84/3791",
        ] {
            let error = answer(206, header, 85).expect_err(header);
            assert_eq!(error.code, ErrorCode::FileImportFailure, "{header}");
            assert!(error.context.starts_with("status=206"), "{}", error.context);
        }
    }

    #[test]
    fn a_partial_answer_from_a_file_of_another_length_is_refused() {
        // The release names a payload of 3791 bytes; a server holding 3790 or 3792 has another
        // file.
        assert!(answer(206, "bytes 44-128/3790", 85).is_err());
        assert!(answer(206, "bytes 44-128/3792", 85).is_err());
        assert!(answer(206, "bytes 44-128/0", 85).is_err());
    }

    #[test]
    fn a_partial_answer_with_a_body_of_the_wrong_length_is_refused() {
        for body in [0, 84, 86, 3791] {
            let error = answer(206, "bytes 44-128/3791", body).expect_err("a wrong body length");
            assert_eq!(error.code, ErrorCode::FileImportFailure, "{body}");
        }
    }

    #[test]
    fn a_partial_answer_without_a_usable_content_range_is_refused() {
        for header in [
            "",
            "bytes",
            "bytes 44-128",
            "bytes 44-128/",
            "bytes 44/3791",
            "bytes */3791",
            "bytes 128-44/3791",
            "bytes -44/3791",
            "bytes 44-/3791",
            "items 44-128/3791",
            "bytes  44-128/3791",
            "bytes 44 -128/3791",
            "bytes 44-128/3791 extra",
            "bytes 4x-128/3791",
            "bytes 44-128/37 91",
            "bytes +44-128/3791",
            "bytes 44-128/3791,",
            // 21 digits: more than fit 64 bits, so not a number this program can compare
            "bytes 440000000000000000000-440000000000000000084/3791",
            "bytes 44-128/18446744073709551616",
        ] {
            let error = answer(206, header, 85).expect_err(header);
            assert_eq!(error.code, ErrorCode::FileImportFailure, "{header:?}");
        }
    }

    #[test]
    fn a_server_that_ignored_range_is_refused_with_the_reason_the_dialog_shows() {
        // 200, the whole file, for a request for 85 bytes at 44.
        let error = answer(200, "", PAYLOAD).unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(
            error.message,
            "this network does not allow partial downloads"
        );
        // Whatever the body was: a 200 for part of a file is never the part.
        assert!(answer(200, "", LENGTH).is_err());
        assert_eq!(
            answer(200, "", LENGTH).unwrap_err().code,
            ErrorCode::Unsupported
        );
        // A first-bytes range of the file, too: the server did not honour it, however well the
        // body happens to begin.
        assert_eq!(
            answer_for(200, "", PAYLOAD, 0, LENGTH).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn a_whole_file_answer_is_accepted_only_for_a_request_for_the_whole_file() {
        assert!(answer_for(200, "", PAYLOAD, 0, PAYLOAD).is_ok());
        // ... with all of the body.
        assert!(answer_for(200, "", PAYLOAD - 1, 0, PAYLOAD).is_err());
        assert!(answer_for(200, "", PAYLOAD + 1, 0, PAYLOAD).is_err());
    }

    #[test]
    fn other_statuses_are_failures_that_name_the_status() {
        for code in [204, 301, 400, 403, 404, 416, 429, 500, 503] {
            let error = answer(code, "", 0).expect_err("not a 200 or a 206");
            assert_eq!(error.code, ErrorCode::FileImportFailure, "{code}");
            assert!(
                error.context.starts_with(&format!("status={code}")),
                "{}",
                error.context
            );
        }
    }
}
