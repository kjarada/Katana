//! tests/geo/test_buffer_and_dissolve.cpp: its GeoLineWord suite, the ONE
//! quoting rule of a name written into a line. The file's GisBuffer,
//! GisDissolve and GisBufferSession suites are the vector verbs' (step 6,
//! wave 2, V); its GisBufferContract suite is step 5's (katana_gis).

mod geo_line_word {
    use katana_app::geo::replies::line_word;

    #[test]
    fn a_blank_an_empty_word_and_a_keyword_are_quoted_and_a_quote_is_refused() {
        assert_eq!(
            line_word("gis/easement"),
            Some(String::from("gis/easement"))
        );
        assert_eq!(line_word("set backs"), Some(String::from("\"set backs\"")));
        assert_eq!(line_word(""), Some(String::from("\"\"")));
        for keyword in [
            "preview",
            "PREVIEW",
            "Confirm",
            "to",
            "FROM",
            "overwrite",
            "replace",
            "name",
            "minus",
            "where",
            "reference",
        ] {
            assert_eq!(
                line_word(keyword),
                Some(format!("\"{keyword}\"")),
                "{keyword}"
            );
        }
        assert!(line_word("a\"b").is_none());
        assert!(line_word("a\nb").is_none());
    }
}
