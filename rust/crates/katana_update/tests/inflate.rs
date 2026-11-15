//! Raw deflate with a cap (the twin of tests/update/test_inflate.cpp).
//!
//! Every expectation is worked out outside the code under test (CLAUDE.md, section 4):
//! - the members of the fixture releases were deflated by Python's zipfile, and their SHA-256
//!   values and sizes are the ones tools/update_make_manifest.py computed with hashlib;
//! - the other streams were made with Python's zlib (compressobj, wbits -15) and checked to
//!   decompress there, or - the hostile ones - written bit by bit from RFC 1951 (3.2.6, the fixed
//!   Huffman code) and checked with Python's zlib.decompress(data, -15), which names the error
//!   each one must raise ("invalid block type", "invalid distance too far back", "invalid stored
//!   block lengths");
//! - sizes and contents of the big outputs are given by a rule (byte i is i % 251; a run of
//!   zeros), not by running the code.

#![allow(
    clippy::module_inception,
    reason = "The inner module's name is the C++ suite's, for the parity checker."
)]

mod inflate {
    use katana_core::error::make_error;
    use katana_core::sha256::sha256_hex;
    use katana_core::{ErrorCode, Status};
    use katana_update::inflate::{inflate_raw, inflate_raw_to_string};
    use std::path::{Path, PathBuf};

    fn from_hex(hex: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        let mut i = 0;
        while i + 1 < hex.len() {
            bytes.push(u8::from_str_radix(&hex[i..i + 2], 16).expect("hex digits"));
            i += 2;
        }
        bytes
    }

    // ---- the fixture releases (tests/update/fixtures/release/<name>) -----------------------

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/update/fixtures"
    );

    struct Member {
        path: String,
        csize: u64,
        offset: u64,
        size: u64,
        sha256: String,
    }

    fn take_number(text: &str) -> Option<(u64, &str)> {
        let end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        if end == 0 {
            return None;
        }
        Some((text[..end].parse().ok()?, &text[end..]))
    }

    // One member as the producer writes it (sorted keys): what the C++ test's regex reads,
    // "csize":N,("exec":true,)?"offset":N,"path":"...","sha256":"<64 hex>","size":N, from just
    // after "csize":.
    fn parse_member(text: &str) -> Option<Member> {
        let (csize, rest) = take_number(text)?;
        let rest = rest.strip_prefix(',')?;
        let rest = rest.strip_prefix("\"exec\":true,").unwrap_or(rest);
        let rest = rest.strip_prefix("\"offset\":")?;
        let (offset, rest) = take_number(rest)?;
        let rest = rest.strip_prefix(",\"path\":\"")?;
        let end = rest.find('"')?;
        if end == 0 {
            return None;
        }
        let path = &rest[..end];
        let rest = rest[end..].strip_prefix("\",\"sha256\":\"")?;
        let sha256 = rest.get(..64)?;
        if !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        let rest = rest[64..].strip_prefix("\",\"size\":")?;
        let (size, _) = take_number(rest)?;
        Some(Member {
            path: path.to_owned(),
            csize,
            offset,
            size,
            sha256: sha256.to_owned(),
        })
    }

    fn members_of(release: &str) -> (Vec<Member>, Vec<u8>) {
        let folder = Path::new(FIXTURES).join("release").join(release);
        let manifest = std::fs::read_to_string(folder.join("Katana-update-win64.json"))
            .unwrap_or_else(|error| panic!("{}: {error}", folder.display()));
        let members = manifest
            .match_indices("\"csize\":")
            .filter_map(|(at, key)| parse_member(&manifest[at + key.len()..]))
            .collect();
        let mut zip = Vec::new();
        for entry in std::fs::read_dir(&folder).expect("the release folder is listed") {
            let path: PathBuf = entry.expect("an entry of the release folder").path();
            if path.extension().is_some_and(|extension| extension == "zip") {
                zip = std::fs::read(&path).expect("the payload is read");
            }
        }
        (members, zip)
    }

    // ---- a deflate stream written by hand ----------------------------------------------------

    // RFC 1951, 3.2.6, the fixed Huffman code: one block holding the literal 0, then `matches`
    // copies of "258 bytes from distance 1", then (when `last` is 3 to 10) one more copy of `last`
    // bytes: 1 + 258 * matches + last zero bytes in 3 + 8 + 13 * matches + 12 + 7 bits. This is
    // how a 64 MB output is a few hundred kilobytes.
    #[derive(Default)]
    struct BitWriter {
        bits: Vec<u8>,
    }

    impl BitWriter {
        fn lsb(&mut self, value: u32, count: u32) {
            for i in 0..count {
                self.bits.push(((value >> i) & 1) as u8);
            }
        }

        fn huffman(&mut self, code: u32, count: u32) {
            for i in (0..count).rev() {
                self.bits.push(((code >> i) & 1) as u8);
            }
        }

        fn bytes(&self) -> Vec<u8> {
            let mut out = vec![0u8; self.bits.len().div_ceil(8)];
            for (i, bit) in self.bits.iter().enumerate() {
                out[i / 8] |= bit << (i % 8);
            }
            out
        }
    }

    fn zero_run(matches: usize, last: u32) -> Vec<u8> {
        let mut w = BitWriter::default();
        w.lsb(1, 1); // BFINAL
        w.lsb(1, 2); // BTYPE 01: fixed Huffman
        w.huffman(0x30, 8); // literal 0: codes 0x30 + value for 0..143
        for _ in 0..matches {
            w.huffman(0xc0 + 5, 8); // length code 285 = 258 bytes (codes 280..287 are 0xC0 + n - 280)
            w.huffman(0, 5); // distance code 0 = 1 back
        }
        if last != 0 {
            w.huffman(last - 3 + 1, 7); // length codes 257..264 are 3..10 bytes: the 7-bit codes 1..8
            w.huffman(0, 5);
        }
        w.huffman(0, 7); // end of block (256)
        w.bytes()
    }

    // ---- streams made with Python's zlib -----------------------------------------------------

    // b"Hello, Katana update!\n" * 40, level 6 (880 bytes in 34).
    fn hello_plain() -> Vec<u8> {
        b"Hello, Katana update!\n".repeat(40)
    }
    fn hello() -> Vec<u8> {
        from_hex("f348cdc9c9d751f04e2c49cc4b54282d48492c4955e4f218151d151d15d5a1be2800")
    }

    // b"" : the empty final block, 03 00.
    fn empty() -> Vec<u8> {
        from_hex("0300")
    }

    // b"stored bytes" at level 0: one stored block (RFC 1951, 3.2.4).
    fn stored() -> Vec<u8> {
        from_hex("010c00f3ff73746f726564206279746573")
    }

    // b"abc", a full flush (an empty stored block), b"def": two blocks, the first not final.
    fn blocks() -> Vec<u8> {
        from_hex("4a4c4a06000000ffff4b494d0300")
    }

    // 502 bytes of text and a run of the printable ASCII range, level 9 (a dynamic-code block);
    // sha256 077b7592ff7244e65e4b7c15c4bf0202ae4682b4a9fa35e1c43fcf4805799a8d (hashlib).
    fn text() -> Vec<u8> {
        from_hex(concat!(
            "0bc94855282ccd4cce56482aca2fcf5348cbaf50c82acd2d2856c82f4b2d5228014ae72456552aa4e4a7eb2984d04c71",
            "4022505d6ea54212505179664986425a66592a50aa2a354f2127b3b034bf08a837bd9804850a8a4aca2aaa6aea1a9a5a",
            "da3aba7afa068646c626a666e6169656d636b676f60e8e4ece2eae6eee1e9e5ede3ebe7efe018141c121a161e1119151",
            "d131b171f1098949c929a969e9199959d939b979f9058545c525a565e5159555d535b57543db7800",
        ))
    }
    const TEXT_SHA256: &str = "077b7592ff7244e65e4b7c15c4bf0202ae4682b4a9fa35e1c43fcf4805799a8d";

    // bytes((i % 251) for i in range(300000)), level 9: 1482 bytes in dynamic blocks.
    fn pattern() -> Vec<u8> {
        from_hex(concat!(
            "edcf539610000000c0cdb6b9d9b66ddbb66dd766dbb66ddbb66d5b5fdda33773830908163c44c850a1c3840d173e42c4",
            "4891a3448d163d46cc58b1e3c48d173f41c244890393244d963c45ca54a9d3a44d973e43c64c99b364cd963d47ce5cb9",
            "f3e4cd97bf40c142858b142d56bc44c952a5cb942d57be42c54a95ab54ad56bd46cd5ab5ebd4ad57bf41c3468d9b346d",
            "d6bc45cb56addbb46dd7be43c74e9dbb74edd6bd47cf5ebdfbf4edd77fc0c04183870c1d16347cc4c851a3c78c1d377e",
            "c2c44993a74c9d367dc6cc59b3e7cc9d377fc1c2458b972c5db67cc5ca55abd7ac5db77ec3c64d9bb76cddb67dc7ce5d",
            "bbf7ecddb7ffc0c143878f1c3d76fcc4c953a7cf9c3d77fec2c54b97af5cbd76fdc6cd5bb7efdcbd77ffc1c3478f9f3c",
            "7df6fcc5cb57afdfbc7df7fec3c74f9fbf7cfdf6fdc7cf5fbffffc0d5057575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757575757",
            "57575757575757575757575757575757575757575757575757575757575757575757575757ffffeaff00",
        ))
    }
    // hashlib.sha256 of the 300000 bytes.
    const PATTERN_SHA256: &str = "3c65ea93424a9c362fec0e3a69ea36031e8a358441479dd665cc6110eabe7b08";

    // What the sink is given.
    #[derive(Default)]
    struct Collected {
        bytes: Vec<u8>,
        blocks: Vec<usize>,
    }

    fn collected(compressed: &[u8], expected_size: u64) -> (Status, Collected) {
        let mut collected = Collected::default();
        let status = inflate_raw(compressed, expected_size, &mut |block: &[u8]| -> Status {
            collected.blocks.push(block.len());
            collected.bytes.extend_from_slice(block);
            Ok(())
        });
        (status, collected)
    }

    fn discarded(compressed: &[u8], expected_size: u64) -> Status {
        inflate_raw(compressed, expected_size, &mut |_: &[u8]| -> Status {
            Ok(())
        })
    }

    fn code_of(status: &Status) -> ErrorCode {
        status.as_ref().expect_err("a refusal").code
    }

    // ---- good input --------------------------------------------------------------------------

    #[test]
    fn every_member_of_the_fixture_releases_inflates_to_the_bytes_the_manifest_hashes() {
        // Two releases, 9 and 10 members, deflated by zipfile; the hashes are hashlib's.
        for (release, count) in [("v1", 9), ("v2", 10)] {
            let (members, zip) = members_of(release);
            assert_eq!(members.len(), count, "{release}");
            assert!(!zip.is_empty());
            for member in &members {
                let start = member.offset as usize;
                let end = (member.offset + member.csize) as usize;
                assert!(end <= zip.len(), "{}", member.path);
                let data =
                    inflate_raw_to_string(&zip[start..end], member.size).unwrap_or_else(|error| {
                        panic!("{release} {}: {}", member.path, error.describe())
                    });
                assert_eq!(data.len() as u64, member.size, "{}", member.path);
                assert_eq!(sha256_hex(&data), member.sha256, "{}", member.path);
            }
        }
    }

    #[test]
    fn an_empty_file_is_the_two_byte_empty_final_block() {
        let data = inflate_raw_to_string(&empty(), 0)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(data.is_empty());
        let (status, collected) = collected(&empty(), 0);
        assert!(status.is_ok());
        assert!(
            collected.blocks.is_empty(),
            "the sink is not called with an empty block"
        );
    }

    #[test]
    fn text_compressed_by_zlib_comes_back_whole() {
        let plain = hello_plain();
        let data = inflate_raw_to_string(&hello(), plain.len() as u64)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(data, plain);
    }

    #[test]
    fn a_stored_block_is_copied_through() {
        let data = inflate_raw_to_string(&stored(), 12)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(data, b"stored bytes");
    }

    #[test]
    fn a_block_that_is_not_final_is_followed_to_the_next() {
        let data = inflate_raw_to_string(&blocks(), 6)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(data, b"abcdef");
    }

    #[test]
    fn a_dynamic_code_block_inflates_to_the_bytes_hashlib_hashed() {
        let data = inflate_raw_to_string(&text(), 502)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(data.len(), 502);
        assert_eq!(sha256_hex(&data), TEXT_SHA256);
    }

    #[test]
    fn an_output_of_several_blocks_is_handed_over_in_order_in_bounded_pieces() {
        let (status, collected) = collected(&pattern(), 300_000);
        assert!(status.is_ok());
        assert_eq!(collected.bytes.len(), 300_000);
        assert_eq!(sha256_hex(&collected.bytes), PATTERN_SHA256);
        assert!(
            collected.blocks.len() > 1,
            "the output is streamed, not held and handed over whole"
        );
        for &size in &collected.blocks {
            assert!(size > 0);
            assert!(size <= 1 << 20, "a block is bounded, whatever the output");
        }
        // Byte i is i % 251 (the rule that built the stream), whatever the block boundaries were.
        let wrong = collected
            .bytes
            .iter()
            .enumerate()
            .filter(|&(i, &byte)| usize::from(byte) != i % 251)
            .count();
        assert_eq!(wrong, 0);
    }

    #[test]
    fn a_long_run_of_zeros_written_by_hand_inflates_to_exactly_that_many_zeros() {
        // 1 + 258 * 2000 bytes, from a stream written in this file from RFC 1951.
        let stream = zero_run(2000, 0);
        assert!(stream.len() < 4000);
        let data = inflate_raw_to_string(&stream, 1 + 258 * 2000)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(data.len(), 516_001);
        assert_eq!(data.iter().filter(|&&byte| byte == 0).count(), 516_001);
    }

    // ---- input that ends early or is not deflate ---------------------------------------------

    #[test]
    fn every_proper_prefix_of_a_stream_is_refused_as_truncated() {
        let cases: [(Vec<u8>, u64); 5] = [
            (hello(), 880),
            (text(), 502),
            (blocks(), 6),
            (stored(), 12),
            (pattern(), 300_000),
        ];
        let mut cuts = 0usize;
        let mut possible = 0usize;
        for (stream, size) in &cases {
            possible += stream.len();
            for length in 0..stream.len() {
                let status = discarded(&stream[..length], *size);
                assert!(
                    status.is_err(),
                    "length {length} of {} accepted",
                    stream.len()
                );
                assert_eq!(
                    code_of(&status),
                    ErrorCode::ParseFailure,
                    "length {length} of {}",
                    stream.len()
                );
                cuts += 1;
            }
        }
        assert_eq!(cuts, possible, "every cut point of every stream was tried");
        assert!(cuts > 1700);
    }

    #[test]
    fn no_input_at_all_is_a_truncated_stream_even_for_an_empty_file() {
        assert_eq!(code_of(&discarded(b"", 0)), ErrorCode::ParseFailure);
        assert_eq!(code_of(&discarded(b"", 5)), ErrorCode::ParseFailure);
    }

    #[test]
    fn data_that_is_not_deflate_is_refused() {
        // Python's zlib.decompress(data, -15) refuses each of these, with the reason in the
        // comment.
        let cases: [(&str, Vec<u8>); 5] = [
            (
                "reserved block type 11 (invalid block type)",
                from_hex("07"),
            ),
            ("all ones (invalid block type)", from_hex("ffffffff")),
            (
                "a stored block whose NLEN is not LEN's complement (invalid stored block lengths)",
                from_hex("01050000006162636465"),
            ),
            (
                "a copy from before the start of the output (invalid distance too far back)",
                from_hex("4b044200"),
            ),
            (
                "text (invalid code lengths set)",
                b"this is not deflate data at all".to_vec(),
            ),
        ];
        for (what, bytes) in &cases {
            let error = inflate_raw_to_string(bytes, 10).expect_err(what);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{what}");
        }
    }

    // ---- the signed size: too long, too short, a lie -----------------------------------------

    #[test]
    fn a_member_that_inflates_to_more_than_its_signed_size_is_cut_off_and_refused() {
        let (status, collected) = collected(&hello(), 879);
        assert_eq!(code_of(&status), ErrorCode::InvalidArgument);
        assert!(
            collected.bytes.len() <= 879,
            "not one byte past the cap reached the sink"
        );
    }

    #[test]
    fn a_member_signed_as_empty_that_holds_data_is_refused_before_any_of_it_is_delivered() {
        let (status, collected) = collected(&hello(), 0);
        assert_eq!(code_of(&status), ErrorCode::InvalidArgument);
        assert!(collected.bytes.is_empty());
    }

    #[test]
    fn a_member_that_inflates_to_less_than_its_signed_size_is_refused() {
        for claimed in [881, 1_000_000, u64::MAX] {
            // The largest claims are refused up front by the collected form, the others by the
            // length check: both are InvalidArgument, and both are before the data is believed.
            let error = inflate_raw_to_string(&hello(), claimed).expect_err("a short member");
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{claimed}");
        }
        // The streaming form takes any claim: it never allocates from it.
        assert_eq!(
            code_of(&discarded(&hello(), u64::MAX)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code_of(&discarded(&empty(), 1)),
            ErrorCode::InvalidArgument,
            "an empty member signed as one byte"
        );
    }

    #[test]
    fn a_stream_that_would_fill_the_disk_is_stopped_at_the_cap() {
        // 1 + 258 * 260000 = 67 MB of zeros from 420 KB of input, against a signed size of 1000.
        let bomb = zero_run(260_000, 0);
        assert!(bomb.len() < 500_000);
        let (status, collected) = collected(&bomb, 1000);
        assert_eq!(code_of(&status), ErrorCode::InvalidArgument);
        assert!(collected.bytes.len() <= 1000);
        // And the collected form, which holds its output, holds no more than the cap either.
        let error = inflate_raw_to_string(&bomb, 1000).expect_err("a bomb");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_cap_exactly_at_the_stream_length_accepts_and_one_less_refuses() {
        let stream = zero_run(300, 0); // 1 + 258 * 300 = 77401 bytes, more than one block
        assert!(inflate_raw_to_string(&stream, 77_401).is_ok());
        assert!(inflate_raw_to_string(&stream, 77_400).is_err());
        assert!(inflate_raw_to_string(&stream, 77_402).is_err());
        // The same where the output ends exactly on, and one past, the 64 KiB (65536) a block of
        // working memory might hold: 1 + 258 * 254 = 65533, plus a copy of 3 or of 4 bytes.
        for last in [3u32, 4] {
            let edge = zero_run(254, last);
            let size = 65_533 + u64::from(last);
            assert!(inflate_raw_to_string(&edge, size).is_ok(), "{size}");
            assert!(inflate_raw_to_string(&edge, size - 1).is_err(), "{size}");
            assert!(inflate_raw_to_string(&edge, size + 1).is_err(), "{size}");
        }
    }

    #[test]
    fn bytes_after_the_end_of_the_stream_are_refused() {
        for tail in [vec![0u8], empty(), b"garbage after".to_vec()] {
            let mut stream = hello();
            stream.extend_from_slice(&tail);
            let error = inflate_raw_to_string(&stream, hello_plain().len() as u64)
                .expect_err("a stream with a tail");
            assert_eq!(error.code, ErrorCode::InvalidArgument);
            assert!(error.message.contains("after the end"), "{}", error.message);
        }
    }

    // ---- the sink ----------------------------------------------------------------------------

    #[test]
    fn a_sink_that_fails_stops_the_inflation_and_its_error_comes_back_unchanged() {
        let mut calls = 0;
        let status = inflate_raw(&pattern(), 300_000, &mut |_: &[u8]| -> Status {
            calls += 1;
            if calls == 2 {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    "disk full",
                    "stage/bin/x",
                ));
            }
            Ok(())
        });
        let error = status.expect_err("the sink refused");
        assert_eq!(error.code, ErrorCode::FileExportFailure);
        assert_eq!(error.message, "disk full");
        assert_eq!(error.context, "stage/bin/x");
        assert_eq!(calls, 2, "nothing is delivered after a refusal");
    }

    #[test]
    fn a_member_larger_than_the_string_form_holds_is_refused_before_any_work() {
        let error = inflate_raw_to_string(&hello(), 1 << 40).expect_err("too large to hold");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}
