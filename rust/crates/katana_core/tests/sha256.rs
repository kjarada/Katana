pub mod support;

const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const BITS_448: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
const BITS_896: &str = "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1";
const MILLION_A: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
const MESSAGE_448: &[u8] = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
const MESSAGE_896: &[u8] = concat!(
    "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnop",
    "qrsmnopqrstnopqrstu"
)
.as_bytes();
const PATTERN_1048593: &str = "0f08b3a8f00fd6361440a1187acb2c83bb3e1f8cb5c395d541bcb11a3b408c4b";
const PATTERN_64_MIB: &str = "35a477c6d711cfbfc35e7857d6d975474c8efb1e8c7fe956bdefc5e7caecc6a2";
const ZEROS_2_POW_29_PLUS_1: &str =
    "7c40fe5ce847740d0f0d0cdde3949d6585804cdec3ae61a15b923165699c8137";

const REPEATED_A: &[(usize, &str)] = &[
    (
        1,
        "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
    ),
    (
        55,
        "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
    ),
    (
        56,
        "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
    ),
    (
        57,
        "f13b2d724659eb3bf47f2dd6af1accc87b81f09f59f2b75e5c0bed6589dfe8c6",
    ),
    (
        63,
        "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
    ),
    (
        64,
        "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
    ),
    (
        65,
        "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
    ),
    (
        119,
        "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb",
    ),
    (
        120,
        "2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c",
    ),
    (
        121,
        "e9615320128cc7a3d6078e9af05603188e5ccbf0d07d8b735d3df5e8e0c1281f",
    ),
    (
        127,
        "c57e9278af78fa3cab38667bef4ce29d783787a2f731d4e12200270f0c32320a",
    ),
    (
        128,
        "6836cf13bac400e9105071cd6af47084dfacad4e5e302c94bfed24e013afb73e",
    ),
    (
        129,
        "c12cb024a2e5551cca0e08fce8f1c5e314555cc3fef6329ee994a3db752166ae",
    ),
];
const PATTERN: &[(usize, &str)] = &[
    (
        0,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        1,
        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
    ),
    (
        55,
        "576a1bf8d4478657e6dc4af9398544765c2a92cde28478b019235cfed315fc09",
    ),
    (
        56,
        "9b20501dfd1d99161c257950f3444f3e49230c351c5c8e0943ef369f85f5205d",
    ),
    (
        63,
        "30b345906b493f06f69444b6521113511c242f30e29840462950035043682f1e",
    ),
    (
        64,
        "d8bc63b4fc1156e5e7d95a418b9bf54cd3174bedbc2db40f74895349b229b3c0",
    ),
    (
        65,
        "1ee23b0fbcaecc1aff4a9e8f1645f35ab2c8e13609cd73b68df8b5e3f63ce073",
    ),
    (
        127,
        "67d79933e3c9aa8e89f481e071cfca1e9a16c09d7263b5efa3bb01f1a9f8d065",
    ),
    (
        128,
        "54c9eb041badfd7064645067b107661fed6113197ce2dd066ba69618abd3732f",
    ),
    (
        129,
        "5b55715b51c54661ed0cbd75ba5b00cf1ffee3b7c113a636eca613d9425116a6",
    ),
    (
        100000,
        "55af394c980c7a7fb68aa904c4afdd93d76e5f826487105fc06f92a25bab8cbe",
    ),
    (
        262144,
        "23295ac6e56186bdc6715065c52588ed68859187befcff701de609c7841ab38f",
    ),
    (
        524288,
        "202f75b6b7bea0f70d6fe412355f10ea2aaf3acd569eb2b55f70b69d4ba4f9f3",
    ),
    (1048593, PATTERN_1048593),
];

fn pattern_bytes(start: u64, count: usize) -> Vec<u8> {
    (0..count)
        .map(|index| {
            let position = start + index as u64;
            ((position * 7 + (position >> 8)) & 0xff) as u8
        })
        .collect()
}

fn hash_in_pieces(message: &[u8], piece: usize) -> String {
    let mut hash = katana_core::sha256::Sha256::new();
    for bytes in message.chunks(piece) {
        hash.update(bytes);
    }
    hash.finish_hex()
}

mod sha256 {
    use super::*;
    use katana_core::ErrorCode;
    use katana_core::path_text::path_from_utf8;
    use katana_core::sha256::*;
    use std::path::Path;
    use support::Scratch;

    #[test]
    fn the_standards_own_messages_give_the_standards_digests() {
        assert_eq!(sha256_hex(""), EMPTY);
        assert_eq!(sha256_hex("abc"), ABC);
        assert_eq!(sha256_hex(MESSAGE_448), BITS_448);
        assert_eq!(sha256_hex(MESSAGE_896), BITS_896);
        assert_eq!(sha256_hex(vec![b'a'; 1000000]), MILLION_A);
    }

    #[test]
    fn the_standards_own_messages_give_the_same_digests_when_streamed() {
        let mut hash = Sha256::new();
        hash.update("a");
        hash.update("");
        hash.update("bc");
        assert_eq!(hash.finish_hex(), ABC);
        let mut million = Sha256::new();
        for _ in 0..1000000 {
            million.update("a");
        }
        assert_eq!(million.finish_hex(), MILLION_A);
        assert_eq!(hash_in_pieces(MESSAGE_896, 17), BITS_896);
    }

    #[test]
    fn messages_either_side_of_the_padding_boundaries_give_hashlibs_digests() {
        for &(length, digest) in REPEATED_A {
            assert_eq!(sha256_hex(vec![b'a'; length]), digest, "{length} x 'a'");
        }
        for &(length, digest) in PATTERN {
            if length <= 100000 {
                assert_eq!(
                    sha256_hex(pattern_bytes(0, length)),
                    digest,
                    "{length} bytes of the pattern"
                );
            }
        }
    }

    #[test]
    fn every_cut_of_a_message_gives_the_digest_of_the_whole_message() {
        let message = pattern_bytes(0, 200);
        let whole = sha256_hex(&message);
        for cut in 0..=message.len() {
            let mut hash = Sha256::new();
            hash.update(&message[..cut]);
            hash.update(&message[cut..]);
            assert_eq!(hash.finish_hex(), whole, "cut at {cut}");
        }
    }

    #[test]
    fn pieces_of_one_seven_sixty_four_and_sixty_five_bytes_give_the_one_shot_digest() {
        for length in 0..=300 {
            let message = pattern_bytes(0, length);
            let whole = sha256_hex(&message);
            for piece in [1, 7, 64, 65] {
                assert_eq!(
                    hash_in_pieces(&message, piece),
                    whole,
                    "{length} bytes in pieces of {piece}"
                );
            }
        }
    }

    #[test]
    fn pieces_of_different_sizes_in_one_message_give_hashlibs_digest() {
        let message = pattern_bytes(0, 1048593);
        let mut hash = Sha256::new();
        let mut at = 0;
        for piece in [1, 7, 64, 65, 1000].into_iter().cycle() {
            if at >= message.len() {
                break;
            }
            let take = piece.min(message.len() - at);
            hash.update(&message[at..at + take]);
            at += take;
        }
        assert_eq!(hash.finish_hex(), PATTERN_1048593);
    }

    #[test]
    fn an_empty_update_and_a_view_of_nothing_change_nothing() {
        let mut hash = Sha256::new();
        hash.update([]);
        hash.update("abc");
        hash.update(&[] as &[u8]);
        assert_eq!(hash.finish_hex(), ABC);
        assert_eq!(Sha256::new().finish_hex(), EMPTY);
    }

    #[test]
    fn the_bytes_are_read_wherever_they_lie_with_no_alignment_required() {
        let message = pattern_bytes(0, 200);
        for offset in 0..8 {
            let mut buffer = vec![b'#'; offset];
            buffer.extend_from_slice(&message);
            assert_eq!(
                sha256_hex(&buffer[offset..]),
                sha256_hex(&message),
                "{offset}"
            );
        }
    }

    #[test]
    fn sixty_four_mebibytes_streamed_give_hashlibs_digest_without_holding_them() {
        let total: u64 = 64 * 1024 * 1024;
        let piece = 1000003;
        let mut hash = Sha256::new();
        for at in (0..total).step_by(piece) {
            let count = (piece as u64).min(total - at) as usize;
            hash.update(pattern_bytes(at, count));
        }
        assert_eq!(hash.finish_hex(), PATTERN_64_MIB);
    }

    #[test]
    fn a_message_of_more_than_four_gibibits_has_its_whole_length_padded() {
        let zeros = vec![0u8; 1 << 20];
        let mut hash = Sha256::new();
        for _ in 0..512 {
            hash.update(&zeros);
        }
        hash.update(&zeros[..1]);
        assert_eq!(hash.finish_hex(), ZEROS_2_POW_29_PLUS_1);
    }

    #[test]
    fn finish_starts_the_next_message_from_scratch() {
        let mut hash = Sha256::new();
        hash.update("abc");
        assert_eq!(hash.finish_hex(), ABC);
        assert_eq!(hash.finish_hex(), EMPTY);
        hash.update(MESSAGE_448);
        assert_eq!(hash.finish_hex(), BITS_448);
    }

    #[test]
    fn reset_forgets_what_was_appended() {
        let mut hash = Sha256::new();
        hash.update(vec![b'x'; 1000]);
        hash.reset();
        hash.update("abc");
        assert_eq!(hash.finish_hex(), ABC);
        let mut untouched = Sha256::new();
        untouched.reset();
        assert_eq!(untouched.finish_hex(), EMPTY);
    }

    #[test]
    fn finish_gives_the_digest_bytes_and_finish_hex_the_same_as_lower_case_text() {
        let mut hash = Sha256::new();
        hash.update("abc");
        let digest = hash.finish();
        assert_eq!(digest[0], 0xba);
        assert_eq!(digest[1], 0x78);
        assert_eq!(digest[31], 0xad);
        assert_eq!(digest, parse_sha256_hex(ABC).unwrap());
        for text in [
            sha256_hex("abc"),
            sha256_hex(""),
            sha256_hex(vec![b'z'; 1000]),
        ] {
            assert_eq!(text.len(), 64);
            assert!(
                text.bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_file_gives_hashlibs_digest_at_every_length_around_the_block_and_the_reading_chunk() {
        let scratch = Scratch::new("katana-core-tests-sha256-lengths");
        for &(length, expected) in PATTERN {
            let file = scratch.root.join(format!("pattern-{length}.bin"));
            std::fs::write(&file, pattern_bytes(0, length)).unwrap();
            assert_eq!(sha256_file_hex(&file).unwrap(), expected, "{length} bytes");
        }
    }

    #[test]
    fn a_file_is_hashed_as_the_bytes_it_holds_not_as_text() {
        let scratch = Scratch::new("katana-core-tests-sha256-binary");
        let file = scratch.root.join("binary.bin");
        std::fs::write(&file, b"a\r\nb\0c\x1a").unwrap();
        let digest = sha256_file_hex(&file).unwrap();
        assert_eq!(digest, sha256_hex(b"a\r\nb\0c\x1a"));
        assert_eq!(
            digest,
            "657e370c762fabff180fa0a764e3cf7fd4548d387480203751a76358c7060974"
        );
    }

    #[test]
    fn a_file_that_is_not_there_is_not_found_and_names_the_path() {
        let scratch = Scratch::new("katana-core-tests-sha256-missing");
        let error = sha256_file_hex(&scratch.root.join("absent.bin")).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(error.context.contains("absent.bin"), "{error}");
    }

    #[test]
    fn a_directory_is_not_found_rather_than_the_digest_of_an_empty_file() {
        let scratch = Scratch::new("katana-core-tests-sha256-directory");
        assert_eq!(
            sha256_file_hex(&scratch.root).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_device_is_not_found_rather_than_the_digest_of_what_it_yields() {
        let device = if cfg!(windows) { "NUL" } else { "/dev/null" };
        assert_eq!(
            sha256_file_hex(Path::new(device)).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_file_under_a_folder_named_in_non_ascii_letters_is_hashed_and_named_in_utf8() {
        let folder_name = "caf\u{00e9} \u{65e5}";
        let scratch = Scratch::new("katana-core-tests-sha256-unicode");
        let folder = scratch.root.join(path_from_utf8(folder_name));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("abc.txt"), b"abc").unwrap();
        assert_eq!(sha256_file_hex(&folder.join("abc.txt")).unwrap(), ABC);
        let missing = sha256_file_hex(&folder.join("absent.txt")).unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(missing.context.contains(folder_name), "{missing}");
        assert!(missing.context.contains("absent.txt"), "{missing}");
    }

    #[cfg(windows)]
    #[test]
    fn a_read_that_fails_part_way_is_a_failure_not_the_digest_of_what_was_read() {
        use std::os::windows::fs::OpenOptionsExt;
        let scratch = Scratch::new("katana-core-tests-sha256-locked");
        let file = scratch.root.join("locked.bin");
        std::fs::write(&file, pattern_bytes(0, 524288)).unwrap();
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(3)
            .open(&file)
            .unwrap();
        let range = super::windows_lock::lock(&holder);
        let locked = sha256_file_hex(&file).unwrap_err();
        assert_eq!(locked.code, ErrorCode::FileImportFailure);
        assert!(locked.context.contains("locked.bin"), "{locked}");
        range.unlock();
        assert_eq!(
            sha256_file_hex(&file).unwrap(),
            "202f75b6b7bea0f70d6fe412355f10ea2aaf3acd569eb2b55f70b69d4ba4f9f3"
        );
    }

    #[test]
    fn parse_reads_sixty_four_lower_case_hex_digits_into_their_bytes() {
        let parsed = parse_sha256_hex(EMPTY).unwrap();
        assert_eq!(parsed[0], 0xe3);
        assert_eq!(parsed[1], 0xb0);
        assert_eq!(parsed[2], 0xc4);
        assert_eq!(parsed[3], 0x42);
        assert_eq!(parsed[28], 0x78);
        assert_eq!(parsed[29], 0x52);
        assert_eq!(parsed[30], 0xb8);
        assert_eq!(parsed[31], 0x55);
        assert_eq!(parse_sha256_hex("0".repeat(64)).unwrap(), [0; 32]);
        assert!(
            parse_sha256_hex("f".repeat(64))
                .unwrap()
                .iter()
                .all(|byte| *byte == 0xff)
        );
    }

    #[test]
    fn parse_refuses_the_wrong_length_upper_case_and_every_other_character() {
        let good = ABC;
        for bad in [
            String::new(),
            good[..63].into(),
            format!("{good}0"),
            "0".repeat(32),
            "0".repeat(128),
            format!(" {}", &good[1..]),
            format!("{}g", &good[..63]),
            format!("{}G", &good[..63]),
            format!("0x{}", &good[2..]),
            format!("{}B{}", &good[..10], &good[11..]),
            format!("{}\0{}", &good[..20], &good[21..]),
            format!("{}\n", &good[..63]),
        ] {
            assert_eq!(
                parse_sha256_hex(&bad).unwrap_err().code,
                ErrorCode::ParseFailure,
                "accepted: {bad}"
            );
        }
    }
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "The Windows oracle forces a real partial-read failure with a byte-range lock."
)]
mod windows_lock {
    use std::ffi::c_void;
    use std::fs::File;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: *mut c_void,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LockFileEx(
            file: *mut c_void,
            flags: u32,
            reserved: u32,
            low: u32,
            high: u32,
            range: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(
            file: *mut c_void,
            reserved: u32,
            low: u32,
            high: u32,
            range: *mut Overlapped,
        ) -> i32;
    }

    pub struct Range<'a> {
        file: &'a File,
        range: Overlapped,
    }
    pub fn lock(file: &File) -> Range<'_> {
        let mut range = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 300000,
            offset_high: 0,
            event: std::ptr::null_mut(),
        };
        assert_ne!(
            unsafe { LockFileEx(file.as_raw_handle(), 3, 0, 4096, 0, &mut range) },
            0
        );
        Range { file, range }
    }
    impl Range<'_> {
        pub fn unlock(mut self) {
            assert_ne!(
                unsafe { UnlockFileEx(self.file.as_raw_handle(), 0, 4096, 0, &mut self.range) },
                0
            );
        }
    }
}
