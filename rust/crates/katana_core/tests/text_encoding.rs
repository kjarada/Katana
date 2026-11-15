fn as_utf16(ascii: &[u8], little_endian: bool) -> Vec<u8> {
    ascii
        .iter()
        .flat_map(|byte| {
            if little_endian {
                [*byte, 0]
            } else {
                [0, *byte]
            }
        })
        .collect()
}

mod text_encoding {
    use super::as_utf16;
    use katana_core::ErrorCode;
    use katana_core::text_encoding::*;

    #[test]
    fn successful_decodes_mark_validity_but_hand_built_text_does_not() {
        assert!(!DecodedText::default().validated_utf8);
        for bytes in [
            b"plain".as_slice(),
            b"\xef\xbb\xbfmarked",
            b"\xff",
            b"\xff\xfea\0",
            b"\xfe\xff\0a",
        ] {
            let decoded = decode_text(bytes).unwrap();
            assert!(decoded.validated_utf8);
            assert!(std::str::from_utf8(decoded.text.as_bytes()).is_ok());
        }
    }

    #[test]
    fn a_byte_order_mark_decides_the_encoding_and_is_removed() {
        let little =
            decode_text([b"\xff\xfe".as_slice(), &as_utf16(b"ab", true)].concat()).unwrap();
        assert_eq!(little.text, "ab");
        assert_eq!(little.encoding, TextEncoding::Utf16LittleEndian);
        assert!(!little.guessed);
        let big = decode_text([b"\xfe\xff".as_slice(), &as_utf16(b"ab", false)].concat()).unwrap();
        assert_eq!(big.text, "ab");
        assert_eq!(big.encoding, TextEncoding::Utf16BigEndian);
        let utf8 = decode_text(b"\xef\xbb\xbfmodel").unwrap();
        assert_eq!(utf8.text, "model");
        assert_eq!(utf8.encoding, TextEncoding::Utf8WithBom);
    }

    #[test]
    fn utf16_without_a_mark_is_recognised_by_where_its_nuls_fall() {
        let source = "model \"Survey Control\"\n";
        let little = decode_text(as_utf16(source.as_bytes(), true)).unwrap();
        assert_eq!(little.text, source);
        assert_eq!(little.encoding, TextEncoding::Utf16LittleEndian);
        assert!(little.guessed);
        let big = decode_text(as_utf16(source.as_bytes(), false)).unwrap();
        assert_eq!(big.text, source);
        assert_eq!(big.encoding, TextEncoding::Utf16BigEndian);
    }

    #[test]
    fn a_character_outside_the_basic_plane_survives_its_surrogate_pair() {
        let decoded = decode_text(b"\xff\xfe\x3d\xd8\x00\xde").unwrap();
        assert_eq!(decoded.text.as_bytes(), b"\xf0\x9f\x98\x80");
    }

    #[test]
    fn bytes_that_are_not_utf8_are_read_as_windows1252() {
        let decoded = decode_text(b"bearing 45\xb0 cost \x805").unwrap();
        assert_eq!(decoded.encoding, TextEncoding::Windows1252);
        assert!(decoded.guessed);
        assert_eq!(
            decoded.text.as_bytes(),
            b"bearing 45\xc2\xb0 cost \xe2\x82\xac5"
        );
    }

    #[test]
    fn plain_ascii_is_not_reported_as_a_guess() {
        let decoded = decode_text(b"string super { }").unwrap();
        assert_eq!(decoded.encoding, TextEncoding::Utf8);
        assert!(!decoded.guessed);
    }

    #[test]
    fn utf8_without_a_mark_is_kept_and_is_not_called_a_guess() {
        let decoded = decode_text(b"name \"caf\xc3\xa9\"").unwrap();
        assert_eq!(decoded.encoding, TextEncoding::Utf8);
        assert!(!decoded.guessed);
        assert_eq!(decoded.text.as_bytes(), b"name \"caf\xc3\xa9\"");
    }

    #[test]
    fn an_odd_number_of_utf16_bytes_is_a_truncated_file() {
        let error = decode_text(b"\xff\xfea\0b").unwrap_err();
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn a_surrogate_without_its_partner_is_refused() {
        assert!(decode_text(b"\xff\xfe\x3d\xd8a\0").is_err());
        assert!(decode_text(b"\xff\xfe\x00\xde").is_err());
        assert!(decode_text(b"\xff\xfe\x3d\xd8").is_err());
    }

    #[test]
    fn the_empty_file_is_empty_text() {
        assert!(decode_text(b"").unwrap().text.is_empty());
    }

    #[test]
    fn encoding_to_utf16_and_decoding_again_returns_the_text() {
        let text = b"name \"caf\xc3\xa9 \xf0\x9f\x98\x80\"\n";
        let encoded = encode_utf16_little_endian(text).unwrap();
        assert_eq!(&encoded[..2], b"\xff\xfe");
        let decoded = decode_text(&encoded).unwrap();
        assert_eq!(decoded.text.as_bytes(), text);
    }

    #[test]
    fn encoding_refuses_bytes_that_are_not_utf8() {
        let error = encode_utf16_little_endian(b"bad \xff byte").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn an_encoding_stated_by_a_person_is_used_without_detection() {
        let latin = decode_text_as(b"45\xb0", TextEncoding::Windows1252).unwrap();
        assert_eq!(latin.text.as_bytes(), b"45\xc2\xb0");
        assert!(!latin.guessed);
        let little = decode_text_as(b"a\0b\0", TextEncoding::Utf16LittleEndian).unwrap();
        assert_eq!(little.text, "ab");
        let big = decode_text_as(b"a\0b\0", TextEncoding::Utf16BigEndian).unwrap();
        assert_eq!(big.text.as_bytes(), b"\xe6\x84\x80\xe6\x88\x80");
    }

    #[test]
    fn a_stated_encoding_steps_over_a_mark_that_agrees_and_refuses_one_that_does_not() {
        let agreed = decode_text_as(b"\xff\xfea\0", TextEncoding::Utf16LittleEndian).unwrap();
        assert_eq!(agreed.text, "a");
        let utf8 = decode_text_as(b"\xef\xbb\xbfx", TextEncoding::Utf8).unwrap();
        assert_eq!(utf8.text, "x");
        assert_eq!(utf8.encoding, TextEncoding::Utf8WithBom);
        let error = decode_text_as(b"\xff\xfea\0", TextEncoding::Utf16BigEndian).unwrap_err();
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(decode_text_as(b"\xef\xbb\xbfx", TextEncoding::Windows1252).is_err());
        assert!(decode_text_as(b"\xfe\xff\0a", TextEncoding::Utf8).is_err());
    }

    #[test]
    fn a_stated_utf8_file_that_is_not_utf8_is_refused_rather_than_repaired() {
        let error = decode_text_as(b"bearing 45\xb0", TextEncoding::Utf8).unwrap_err();
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn utf8_validity_follows_the_unicode_table_of_well_formed_sequences() {
        assert!(is_valid_utf8(b""));
        assert!(is_valid_utf8(b"\x7f"));
        assert!(is_valid_utf8(b"\xc2\x80"));
        assert!(!is_valid_utf8(b"\xc1\xbf"));
        assert!(is_valid_utf8(b"\xe0\xa0\x80"));
        assert!(!is_valid_utf8(b"\xe0\x9f\xbf"));
        assert!(is_valid_utf8(b"\xed\x9f\xbf"));
        assert!(!is_valid_utf8(b"\xed\xa0\x80"));
        assert!(is_valid_utf8(b"\xf0\x90\x80\x80"));
        assert!(!is_valid_utf8(b"\xf0\x8f\xbf\xbf"));
        assert!(is_valid_utf8(b"\xf4\x8f\xbf\xbf"));
        assert!(!is_valid_utf8(b"\xf4\x90\x80\x80"));
        assert!(!is_valid_utf8(b"\xe2\x82"));
        assert!(!is_valid_utf8(b"\x80"));
    }

    #[test]
    fn text_that_is_not_utf8_is_read_as_windows1252_and_text_that_is_is_left_alone() {
        assert_eq!(utf8_or_windows1252(b"Caf\xe9").as_bytes(), b"Caf\xc3\xa9");
        assert_eq!(
            utf8_or_windows1252(b"Pente\xb0").as_bytes(),
            b"Pente\xc2\xb0"
        );
        assert_eq!(
            utf8_or_windows1252(b"\x80 \x96 \x81").as_bytes(),
            b"\xe2\x82\xac \xe2\x80\x93 \xc2\x81"
        );
        assert_eq!(
            utf8_or_windows1252(b"Caf\xc3\xa9").as_bytes(),
            b"Caf\xc3\xa9"
        );
        assert_eq!(utf8_or_windows1252(b""), "");
        assert_eq!(utf8_or_windows1252(b"LAYER NEW ground"), "LAYER NEW ground");
        assert_eq!(
            utf8_or_windows1252(b"\xc3\xa9\xe9").as_bytes(),
            b"\xc3\x83\xc2\xa9\xc3\xa9"
        );
        let every_byte: Vec<u8> = (0..=255).collect();
        assert!(is_valid_utf8(utf8_or_windows1252(&every_byte)));
    }
}
