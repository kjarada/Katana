//! tests/geo/test_gis_verbs.cpp: its GisRecords suite, the records read
//! back and as JSON. The file's GisVerbs and GisSession suites drive IMPORT,
//! EXPORT, REFS and COPC through the executor and the session: step 6's wave 2
//! (B).

mod gis_records {
    use katana_app::geo::gis_records::record_json;
    use katana_app::geo::replies::{Record, fixed3, parse_records};
    use katana_core::json::Json;

    /// A field's text, or "<absent>": the C++ file's `field`.
    fn field(record: &Record, key: &str) -> String {
        String::from(record.get(key).unwrap_or("<absent>"))
    }

    fn numbers(items: &[f64]) -> Json {
        Json::array_of(items.iter().map(|item| Json::from(*item)))
    }

    #[test]
    fn a_record_reads_as_json_with_its_numbers_and_its_words() {
        let records = parse_records(
            "reference id=12 kind=raster name=12 width=120 bounds=-5,-5,175,130 visible=yes \
             opacity=0.5 file= role=imagery",
        );
        assert_eq!(records.len(), 1);
        let json = record_json(&records[0]);
        assert_eq!(json["record"], Json::from("reference"));
        assert_eq!(json["id"], Json::from(12_i32));
        // A name, whatever it holds.
        assert_eq!(json["name"], Json::from("12"));
        assert_eq!(json["width"], Json::from(120_i32));
        assert_eq!(json["bounds"], numbers(&[-5.0, -5.0, 175.0, 130.0]));
        assert_eq!(json["visible"], Json::from(true));
        assert_eq!(json["opacity"], Json::from(0.5));
        // A word, empty.
        assert_eq!(json["file"], Json::from(""));
        assert_eq!(json["role"], Json::from("imagery"));
    }

    // A record's kind is every word before its first key=: IFC's "ifc
    // exported file=..." read as the kind "ifc" and a key "exported file".
    // And a comma list of numbers - a cell of 4,3 (RASTER GRID's size=
    // stretches it), a scope's area - is an array, as bounds always was; a
    // list that is not all numbers stays a word.
    #[test]
    fn a_kind_is_every_word_before_the_first_key_and_a_number_list_is_an_array() {
        let records = parse_records(concat!(
            "ifc exported file=\"site plan.ifc\" entities=1\n",
            "grid method=nearest cell=4,3 extent=0,0,40,30 size=10x10 stats=mean,min\n",
            "scope scope=area area=0,4,8,6 matched=1 flagged\n",
            "3 entities"
        ));
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].kind, "ifc exported");
        assert_eq!(field(&records[0], "file"), "site plan.ifc");
        let ifc = record_json(&records[0]);
        assert_eq!(ifc["record"], Json::from("ifc exported"));
        assert!(!ifc.contains("exported file"), "{}", ifc.dump());
        assert_eq!(ifc["entities"], Json::from(1_i32));
        let grid = record_json(&records[1]);
        assert_eq!(grid["cell"], numbers(&[4.0, 3.0]));
        assert_eq!(grid["extent"], numbers(&[0.0, 0.0, 40.0, 30.0]));
        assert_eq!(grid["size"], Json::from("10x10"));
        assert_eq!(grid["stats"], Json::from("mean,min"));
        let scope = record_json(&records[2]);
        assert_eq!(scope["area"], numbers(&[0.0, 4.0, 8.0, 6.0]));
        assert_eq!(scope["matched"], Json::from(1_i32));
        // A bare word, kept.
        assert!(scope.contains("flagged"), "{}", scope.dump());
        assert_eq!(records[3].kind, "3 entities");
    }

    // A value that rounds to nothing at three decimals prints without a
    // sign: RASTER DIFFERENCE of a raster and its own surface gave
    // zmin=-0.000.
    #[test]
    fn a_value_that_rounds_to_zero_has_no_sign() {
        assert_eq!(fixed3(-0.0), "0.000");
        assert_eq!(fixed3(-1e-7), "0.000");
        assert_eq!(fixed3(-0.0004), "0.000");
        assert_eq!(fixed3(-0.0006), "-0.001");
        assert_eq!(fixed3(1.25), "1.250");
    }

    // A level a file may hold and an edit refuses (past 1e9) was "nan" in a
    // record: the formatter's buffer held 64 characters, and 1e61 has 65 with
    // its decimals (1e60, 64, filled it to the last).
    #[test]
    fn a_finite_value_too_long_for_sixty_four_characters_is_its_digits_not_nan() {
        assert_eq!(
            fixed3(1e60),
            "999999999999999949387135297074018866963645011013410073083904.000"
        );
        assert_eq!(
            fixed3(1e61),
            "9999999999999999493871352970740188669636450110134100730839040.000"
        );
        assert_eq!(fixed3(-1e300).len(), 306);
        assert_eq!(fixed3(f64::MAX).len(), 313);
        assert_eq!(fixed3(-1e-300), "0.000");
    }
}
