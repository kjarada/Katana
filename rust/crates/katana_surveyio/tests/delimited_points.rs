//! Twin of tests/surveyio/test_delimited_points.cpp.
//!
//! Delimited point files: reading, writing and the layout template.
//!
//! Every fixture puts the northing near 5 000 000 and the easting near 500 000,
//! an order of magnitude apart, so that a transposed reading cannot pass any
//! assertion by coincidence (docs/survey.md, "The two ways survey data gets
//! silently corrupted"). The values are chosen exact in binary - quarters,
//! eighths, sixteenths - so that "the file says 5000000.25" and "the model
//! holds 5000000.25" can be compared with ==, and the expected values are the
//! numbers written in the fixture, not anything captured from a run.

mod support;

use katana_core::text_encoding::{TextEncoding, encode_utf16_little_endian};
use katana_core::{Error, ErrorCode, Result};
use katana_survey::data_model::validate_project;
use katana_survey::{AngularUnit, CoordinateSource, LinearUnit, SurveyPoint};
use katana_surveyio::{
    ColumnPreset, ColumnRole, DELIMITED_POINTS_FORMAT_ID, DelimitedExportOptions,
    DelimitedImportOptions, DelimitedLayout, Delimiter, FormatContent, ImportResult,
    MAXIMUM_EXPORT_DECIMALS, Manufacturer, Quoting, all_column_presets, column_preset_named,
    delimited_points_format, describe_format, format_registry, layout_template,
    parse_delimited_points, parse_layout_template, preset_columns, preset_layout, to_string,
    validate_layout, write_delimited_points,
};

use support::contains;

const NORTHING: f64 = 5000000.25;
const EASTING: f64 = 500000.5;
const ELEVATION: f64 = 101.75;

fn in_metres() -> DelimitedImportOptions {
    DelimitedImportOptions {
        unit: LinearUnit::Metres,
        ..Default::default()
    }
}

fn layout_of(text: &str) -> DelimitedLayout {
    parse_layout_template(text).unwrap_or_else(|error| panic!("{}", error.describe()))
}

// The text as bytes: a str fixture, a byte-string one, or encoded bytes.
fn read_text<T: AsRef<[u8]> + ?Sized>(
    text: &T,
    layout_template_text: &str,
) -> Result<ImportResult> {
    read_text_named(text, layout_template_text, "points.csv")
}

fn read_text_named<T: AsRef<[u8]> + ?Sized>(
    text: &T,
    layout_template_text: &str,
    file_name: &str,
) -> Result<ImportResult> {
    parse_delimited_points(
        text.as_ref(),
        &layout_of(layout_template_text),
        file_name,
        &in_metres(),
    )
}

fn any_warning_contains(result: &ImportResult, needle: &str) -> bool {
    result
        .warnings
        .iter()
        .any(|warning| contains(warning, needle))
}

fn error_of(result: &Result<ImportResult>) -> String {
    match result {
        Ok(_) => String::from("(no error)"),
        Err(error) => error.describe(),
    }
}

fn imported(result: Result<ImportResult>) -> ImportResult {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn refused(result: Result<ImportResult>) -> Error {
    result.expect_err("the file is refused")
}

// ---- Presets ---------------------------------------------------------------------

mod delimited_points_presets {
    use super::*;

    #[test]
    fn every_preset_reads_the_coordinates_from_the_columns_its_letters_name() {
        // One point - id 7, N 5000000.25, E 500000.5, Z 101.75, "Kerb" -
        // written by hand in each preset's column order. A preset that swapped
        // northing and easting would put 500000.5 in the northing and fail
        // here.
        struct Case {
            preset: ColumnPreset,
            line: &'static str,
            /// What the point must be called.
            id: &'static str,
            has_elevation: bool,
            description: &'static str,
        }
        let case = |preset, line, id, has_elevation, description| Case {
            preset,
            line,
            id,
            has_elevation,
            description,
        };
        let cases = [
            case(
                ColumnPreset::PNEZD,
                "7,5000000.25,500000.5,101.75,Kerb",
                "7",
                true,
                "Kerb",
            ),
            case(
                ColumnPreset::PENZD,
                "7,500000.5,5000000.25,101.75,Kerb",
                "7",
                true,
                "Kerb",
            ),
            case(
                ColumnPreset::PNEZ,
                "7,5000000.25,500000.5,101.75",
                "7",
                true,
                "",
            ),
            case(
                ColumnPreset::PENZ,
                "7,500000.5,5000000.25,101.75",
                "7",
                true,
                "",
            ),
            // No id column: the point is named by its line, which is line 1.
            case(
                ColumnPreset::NEZ,
                "5000000.25,500000.5,101.75",
                "1",
                true,
                "",
            ),
            case(
                ColumnPreset::ENZ,
                "500000.5,5000000.25,101.75",
                "1",
                true,
                "",
            ),
            case(ColumnPreset::PNE, "7,5000000.25,500000.5", "7", false, ""),
            case(ColumnPreset::PEN, "7,500000.5,5000000.25", "7", false, ""),
        ];
        assert_eq!(cases.len(), all_column_presets().len()); // every preset, no more

        for c in &cases {
            let trace = to_string(c.preset);
            let layout = preset_layout(c.preset, Delimiter::Comma, 0);
            let result =
                parse_delimited_points(c.line.as_bytes(), &layout, "points.csv", &in_metres());
            let result = result.unwrap_or_else(|error| panic!("{trace}: {}", error.describe()));
            assert_eq!(result.project.points.len(), 1, "{trace}");
            let point = &result.project.points[0];
            assert_eq!(point.id, c.id, "{trace}");
            assert_eq!(point.northing, NORTHING, "{trace}");
            assert_eq!(point.easting, EASTING, "{trace}");
            assert_eq!(point.elevation.is_some(), c.has_elevation, "{trace}");
            if c.has_elevation {
                assert_eq!(point.elevation, Some(ELEVATION), "{trace}");
            }
            assert_eq!(point.description, c.description, "{trace}");
        }
    }

    #[test]
    fn a_preset_is_named_by_its_letters_in_any_case_and_nothing_else() {
        for &preset in all_column_presets() {
            let named = column_preset_named(to_string(preset)).expect("the preset is named");
            assert_eq!(named, preset);
        }
        assert_eq!(
            column_preset_named("penzd").expect("the preset is named"),
            ColumnPreset::PENZD
        );
        assert_eq!(
            column_preset_named("PNEZC")
                .expect_err("there is no such preset")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            column_preset_named("")
                .expect_err("there is no such preset")
                .code,
            ErrorCode::NotFound
        );

        let pnezd = preset_columns(ColumnPreset::PNEZD);
        let expected = [
            ColumnRole::PointId,
            ColumnRole::Northing,
            ColumnRole::Easting,
            ColumnRole::Elevation,
            ColumnRole::Description,
        ];
        assert_eq!(pnezd, expected);
    }
}

// ---- Layout validation and templates ------------------------------------------------

mod delimited_points_layout {
    use super::*;

    #[test]
    fn a_layout_without_both_horizontal_coordinates_or_with_a_role_twice_is_refused() {
        let mut layout = DelimitedLayout {
            columns: vec![
                ColumnRole::PointId,
                ColumnRole::Northing,
                ColumnRole::Elevation,
            ],
            ..Default::default()
        };
        assert_eq!(
            validate_layout(&layout)
                .expect_err("there is no easting")
                .code,
            ErrorCode::InvalidArgument
        );

        layout.columns = vec![ColumnRole::Easting, ColumnRole::Elevation];
        assert!(validate_layout(&layout).is_err());

        layout.columns = vec![
            ColumnRole::Northing,
            ColumnRole::Easting,
            ColumnRole::Easting,
        ];
        let twice = validate_layout(&layout).expect_err("the easting is there twice");
        assert!(contains(&twice.message, "two easting columns (2 and 3)"));

        // Ignore may repeat: it is not a role, it is the absence of one.
        layout.columns = vec![
            ColumnRole::Ignore,
            ColumnRole::Northing,
            ColumnRole::Ignore,
            ColumnRole::Easting,
        ];
        assert!(validate_layout(&layout).is_ok());

        layout.columns.clear();
        assert!(validate_layout(&layout).is_err());
    }

    #[test]
    fn a_comment_prefix_that_could_swallow_a_line_of_data_is_refused() {
        let mut layout = preset_layout(ColumnPreset::PNEZ, Delimiter::Comma, 0);
        // Each of these can begin a line of data, which would then vanish as a
        // comment: an id "12", a coordinate "-3.5", "+7", ".5".
        for prefix in ["1", "-", "+", ".", "9x"] {
            layout.comment_prefix = String::from(prefix);
            assert!(validate_layout(&layout).is_err(), "{prefix}");
        }
        // A blank is trimmed away before the prefix is looked for; a quote
        // opens a field; ';' ends a template part; the delimiter makes an empty
        // first field a comment.
        for prefix in ["# x", "\"", ";", ",", "\t#"] {
            layout.comment_prefix = String::from(prefix);
            assert!(validate_layout(&layout).is_err(), "{prefix}");
        }
        for prefix in ["#", "//", "REM", "!"] {
            layout.comment_prefix = String::from(prefix);
            assert!(validate_layout(&layout).is_ok(), "{prefix}");
        }
    }
}

mod delimited_points_template {
    use super::*;

    #[test]
    fn the_briefs_example_template_reads_as_the_layout_it_spells() {
        let layout = parse_layout_template("P,N,E,Z,D;delimiter=comma;header=1")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(layout.columns, preset_columns(ColumnPreset::PNEZD));
        assert_eq!(layout.delimiter, Delimiter::Comma);
        assert_eq!(layout.header_lines, 1);
        // The two optional keys, absent, mean what the header documents.
        assert_eq!(layout.quoting, Quoting::DoubleQuote);
        assert_eq!(layout.comment_prefix, "");

        // Case and blanks round the parts are not meaning.
        let loose = parse_layout_template(" p , n , e , z , d ; Delimiter = COMMA ; header = 1 ")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(loose, layout);
    }

    #[test]
    fn a_layout_survives_the_trip_through_its_template_text() {
        let mut layouts = vec![preset_layout(ColumnPreset::PNEZD, Delimiter::Comma, 1)];
        let mut aligned = preset_layout(ColumnPreset::ENZ, Delimiter::Whitespace, 0);
        aligned.quoting = Quoting::None;
        aligned.comment_prefix = String::from("#");
        layouts.push(aligned.clone());
        let wide = DelimitedLayout {
            columns: vec![
                ColumnRole::PointId,
                ColumnRole::Ignore,
                ColumnRole::Northing,
                ColumnRole::Easting,
                ColumnRole::Elevation,
                ColumnRole::Code,
                ColumnRole::Ignore,
                ColumnRole::Description,
            ],
            delimiter: Delimiter::Tab,
            header_lines: 3,
            comment_prefix: String::from("//"),
            ..Default::default()
        };
        layouts.push(wide.clone());
        layouts.push(preset_layout(ColumnPreset::PEN, Delimiter::Semicolon, 12));

        for layout in &layouts {
            let text = layout_template(layout);
            let back = parse_layout_template(&text)
                .unwrap_or_else(|error| panic!("{text}: {}", error.describe()));
            assert_eq!(&back, layout, "{text}");
        }
        // The canonical text states every key, so a saved template never
        // depends on what "absent" meant in the version of the program that
        // saved it.
        assert_eq!(
            layout_template(&wide),
            "P,-,N,E,Z,C,-,D;delimiter=tab;header=3;quote=double;comment=//"
        );
        assert_eq!(
            layout_template(&aligned),
            "E,N,Z;delimiter=whitespace;header=0;quote=none;comment=#"
        );
    }

    #[test]
    fn a_malformed_template_is_rejected_rather_than_read_as_something_close() {
        let malformed = [
            "",                                                  // nothing at all
            "P,N,E,Z",                                           // no delimiter, no header count
            "P,N,E,Z;delimiter=comma",                           // no header count
            "P,N,E,Z;header=1",                                  // no delimiter
            "PNEZ;delimiter=comma;header=1", // a preset name is not a column list
            "P,N,Q,Z;delimiter=comma;header=1", // no such column letter
            "P,N,X,Z;delimiter=comma;header=1", // X is not "ignore"
            "P,N,,Z;delimiter=comma;header=1", // an empty column
            "P,N,E;delimiter=pipe;header=1", // no such delimiter
            "P,N,E;delimiter=space;header=1", // not "whitespace" by another name
            "P,N,E;delimiter=comma;header=-1", // a negative count
            "P,N,E;delimiter=comma;header=1.5", // not a count
            "P,N,E;delimiter=comma;header=one", // not a count
            "P,N,E;delimiter=comma;header=", // no value
            "P,N,E;delimiter=comma;header=99999999999999999999", // beyond any count
            "P,N,E;delimiter=comma;header=1;header=2", // which one?
            "P,N,E;delimiter=comma;delimiter=comma;header=1", // even when they agree
            "P,N,E;delimiter=comma;header=1;quote=single", // no such quoting
            "P,N,E;delimiter=comma;header=1;decimal=comma", // no such key
            "P,N,E;delimiter=comma;header=1;;", // an empty part
            "P,N,E;delimiter=comma;header=1;", // an empty last part
            "P,N,E;delimiter comma;header=1", // not key=value
            "P,N,E;delimiter=comma;header=1;comment=1", // a prefix that begins a number
            "P,N,E;delimiter=comma;header=1;comment=,", // the delimiter as a prefix
            "P,N,Z;delimiter=comma;header=1", // no easting column
            "P,N,E,N;delimiter=comma;header=1", // two northings
        ];
        for text in malformed {
            match parse_layout_template(text) {
                Ok(layout) => panic!("'{text}' was read as {}", layout_template(&layout)),
                Err(error) => assert_eq!(error.code, ErrorCode::InvalidArgument, "{text}"),
            }
        }
    }
}

// ---- Import ----------------------------------------------------------------------

mod delimited_points_import {
    use super::*;

    #[test]
    fn a_point_file_reads_into_the_model_with_its_provenance() {
        let text = "Point,Northing,Easting,Elevation,Description\r\n\
                    1,5000000.25,500000.5,101.75,Kerb\r\n\
                    2,5000010.125,500020.375,99.5,Fence\r\n";
        let template_text = "P,N,E,Z,D;delimiter=comma;header=1";
        // A path a file (or a careless caller) supplied: only the name may
        // survive.
        let result = imported(read_text_named(
            text,
            template_text,
            "..\\..\\jobs\\site survey.csv",
        ));

        assert_eq!(result.format_id, "delimited-points");
        assert_eq!(result.records_read, 2);
        assert_eq!(result.records_skipped, 0);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);

        let project = &result.project;
        assert_eq!(project.points.len(), 2);
        let second = &project.points[1];
        assert_eq!(second.id, "2");
        assert_eq!(second.northing, 5000010.125);
        assert_eq!(second.easting, 500020.375);
        assert_eq!(second.elevation, Some(99.5));
        assert_eq!(second.description, "Fence");
        assert_eq!(second.code, "");
        // A plain coordinate list does not say how its coordinates came to be.
        assert_eq!(second.coordinate_source, CoordinateSource::Unknown);

        // Provenance: the file NAME, the line, and the layout it was read with -
        // enough to repeat the import exactly.
        assert_eq!(second.source.file_name, "site survey.csv");
        assert_eq!(second.source.record_number, 3); // header on line 1, point 2 on line 3
        assert_eq!(second.source.format, "Delimited text points (CSV, TXT)");
        assert_eq!(
            second.source.format_version,
            layout_template(&layout_of(template_text))
        );
        assert_eq!(second.source.manufacturer, "Generic");
        assert_eq!(project.source.file_name, "site survey.csv");
        assert_eq!(project.source.record_number, 0);
        // format_version holds the layout, so the version of the code that read
        // the file - which the format contract requires an import to record -
        // is in metadata. "1.0" is the version delimited_points states for this
        // parser.
        assert_eq!(
            project.metadata.get("parser version").map(String::as_str),
            Some("1.0")
        );

        // Transforms nothing, declares nothing it was not told.
        assert!(project.coordinate_system.unknown);
        assert_eq!(project.coordinate_system.epsg_code, 0);
        assert_eq!(project.units.linear, LinearUnit::Metres);
        assert_eq!(project.units.angular, AngularUnit::Unknown);
        assert!(validate_project(project).is_ok());
    }

    #[test]
    fn an_empty_elevation_is_absent_and_never_zero() {
        // Line 2's elevation field is empty; line 3 ends before it; line 4 is a
        // real height of zero, which must stay a height.
        let text = "1,5000000.25,500000.5,,Kerb\n\
                    2,5000000.25,500010.5,  ,Kerb\n\
                    3,5000000.25,500020.5\n\
                    4,5000000.25,500030.5,0,Datum\n";
        let result = imported(read_text(text, "P,N,E,Z,D;delimiter=comma;header=0"));
        let points = &result.project.points;
        assert_eq!(points.len(), 4);
        assert!(points[0].elevation.is_none());
        assert!(points[1].elevation.is_none());
        assert!(points[2].elevation.is_none());
        assert_eq!(points[3].elevation, Some(0.0));

        assert!(any_warning_contains(
            &result,
            "3 of 4 point(s) have no elevation"
        ));
        assert!(any_warning_contains(&result, "first is on line 1"));
        assert!(any_warning_contains(
            &result,
            "1 line(s) end before the layout's last column"
        ));

        // In a whitespace file an empty field has to be written "" to exist at
        // all.
        let aligned = imported(read_text(
            "1 5000000.25 500000.5 \"\" Kerb\n",
            "P,N,E,Z,D;delimiter=whitespace;header=0",
        ));
        assert!(aligned.project.points[0].elevation.is_none());
        assert_eq!(aligned.project.points[0].description, "Kerb");
    }

    #[test]
    fn quoted_fields_may_hold_the_delimiter_doubled_quotes_and_line_breaks() {
        let text = "1,5000000.25,500000.5,101.75,\"Kerb, north side\"\r\n\
                    2,5000000.25,500000.5,101.75,\"6\"\" PVC\"\r\n\
                    3,5000000.25,500000.5,101.75,\"two\r\nlines\"\r\n\
                    4,5000000.25,500000.5,101.75,6\" PVC\r\n\
                    \"5\" , 5000000.25,500000.5,101.75,\"  padded  \"\r\n";
        let result = imported(read_text(text, "P,N,E,Z,D;delimiter=comma;header=0"));
        let points = &result.project.points;
        assert_eq!(points.len(), 5);
        assert_eq!(points[0].description, "Kerb, north side");
        assert_eq!(points[1].description, "6\" PVC"); // RFC 4180: "" inside quotes is one quote
        assert_eq!(points[2].description, "two\r\nlines");
        // A quote that does not open a field is a character (the header says
        // why).
        assert_eq!(points[3].description, "6\" PVC");
        // Blanks round a quoted field are padding; blanks inside it are content.
        assert_eq!(points[4].id, "5");
        assert_eq!(points[4].description, "  padded  ");

        // The record that spans lines 3 and 4 is numbered by the line it starts
        // on, and the next record is on line 5, not 4.
        assert_eq!(points[2].source.record_number, 3);
        assert_eq!(points[3].source.record_number, 5);
        assert_eq!(points[4].source.record_number, 6);

        // With quoting off a quote is just a character, even at the start.
        let plain = imported(read_text(
            "\"7\",5000000.25,500000.5\n",
            "P,N,E;delimiter=comma;header=0;quote=none",
        ));
        assert_eq!(plain.project.points[0].id, "\"7\"");
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_first_point_id() {
        let text = b"\xEF\xBB\xBF1,5000000.25,500000.5\r\n";
        let result = imported(read_text(text, "P,N,E;delimiter=comma;header=0"));
        assert_eq!(result.project.points[0].id, "1");
        assert_eq!(
            result
                .project
                .metadata
                .get("text encoding")
                .map(String::as_str),
            Some(katana_core::text_encoding::to_string(
                TextEncoding::Utf8WithBom
            ))
        );
    }

    #[test]
    fn cr_lf_lf_and_lone_cr_each_end_a_line() {
        let text = "1,5000000.25,500000.5\r\n\
                    2,5000000.25,500010.5\n\
                    3,5000000.25,500020.5\r\
                    4,5000000.25,500030.5";
        let result = imported(read_text(text, "P,N,E;delimiter=comma;header=0"));
        assert_eq!(result.project.points.len(), 4);
        for (i, point) in result.project.points.iter().enumerate() {
            assert_eq!(point.source.record_number, i + 1);
            assert_eq!(point.easting, 500000.5 + 10.0 * i as f64);
        }
    }

    #[test]
    fn utf8_names_codes_and_descriptions_arrive_unchanged() {
        let text = "Pünkt-7,5000000.25,500000.5,101.75,Bäume,東京 station\n";
        let result = imported(read_text(text, "P,N,E,Z,C,D;delimiter=comma;header=0"));
        let point = &result.project.points[0];
        assert_eq!(point.id, "Pünkt-7");
        assert_eq!(point.code, "Bäume");
        assert_eq!(point.description, "東京 station");
        assert!(result.warnings.is_empty()); // UTF-8 validates, so nothing was guessed
    }

    #[test]
    fn a_utf16_spreadsheet_export_is_read_through_the_shared_decoder() {
        // Excel's "Unicode text" is UTF-16LE with a mark, tab-delimited.
        let bytes = encode_utf16_little_endian(
            "Point\tNorthing\tEasting\r\nPünkt\t5000000.25\t500000.5\r\n",
        )
        .expect("the text encodes");
        let result = imported(read_text(&bytes, "P,N,E;delimiter=tab;header=1"));
        assert_eq!(result.project.points.len(), 1);
        assert_eq!(result.project.points[0].id, "Pünkt");
        assert_eq!(result.project.points[0].northing, NORTHING);
    }

    #[test]
    fn an_inferred_windows1252_encoding_is_warned_about_and_a_stated_one_is_not() {
        // 0xE9 alone is not UTF-8; in Windows-1252 it is U+00E9, "é".
        let text = b"1,5000000.25,500000.5,,Caf\xE9\n";
        let guessed = imported(read_text(text, "P,N,E,Z,D;delimiter=comma;header=0"));
        assert_eq!(guessed.project.points[0].description, "Café");
        assert!(any_warning_contains(&guessed, "inferred as Windows-1252"));

        let stated = DelimitedImportOptions {
            encoding: Some(TextEncoding::Windows1252),
            ..in_metres()
        };
        let told = imported(parse_delimited_points(
            text,
            &layout_of("P,N,E,Z,D;delimiter=comma;header=0"),
            "points.csv",
            &stated,
        ));
        assert_eq!(told.project.points[0].description, "Café");
        assert!(!any_warning_contains(&told, "inferred"));
    }

    #[test]
    fn feet_are_converted_to_metres_exactly_and_the_unit_is_recorded() {
        // International foot = 381/1250 m exactly (katana_math's unit_ratio).
        // Worked:
        //   1000 ft * 381 / 1250 = 381000 / 1250 = 304.8 m
        //   2000 ft * 381 / 1250 = 762000 / 1250 = 609.6 m
        //     10 ft * 381 / 1250 =   3810 / 1250 = 3.048 m
        // Each is one correctly rounded division of exact integers, so it is
        // the double nearest the true value - the same double the literal
        // denotes.
        let options = DelimitedImportOptions {
            unit: LinearUnit::Feet,
            ..Default::default()
        };
        let result = imported(parse_delimited_points(
            b"1,1000,2000,10\n",
            &layout_of("P,N,E,Z;delimiter=comma;header=0"),
            "feet.csv",
            &options,
        ));
        let point = &result.project.points[0];
        assert_eq!(point.northing, 304.8);
        assert_eq!(point.easting, 609.6);
        assert_eq!(point.elevation, Some(3.048));
        assert_eq!(result.project.units.linear, LinearUnit::Feet);
    }

    #[test]
    fn an_unstated_unit_is_refused_because_none_is_assumed() {
        let error = refused(parse_delimited_points(
            b"1,5000000.25,500000.5\n",
            &layout_of("P,N,E;delimiter=comma;header=0"),
            "points.csv",
            &DelimitedImportOptions::default(),
        ));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn blank_and_comment_lines_are_skipped_and_comments_counted() {
        let text = "# exported from the field\n\
                    \n\
                    1,5000000.25,500000.5\n   \t \n  # a comment may be indented\n\
                    2,5000000.25,500010.5\n";
        let result = imported(read_text(text, "P,N,E;delimiter=comma;header=0;comment=#"));
        assert_eq!(result.project.points.len(), 2);
        assert_eq!(result.project.points[0].source.record_number, 3);
        assert_eq!(result.project.points[1].source.record_number, 6);
        assert!(any_warning_contains(
            &result,
            "2 comment line(s) starting '#' were skipped"
        ));
    }

    #[test]
    fn every_delimiter_splits_its_own_file() {
        let cases = [
            (
                "P,N,E,Z,D;delimiter=tab;header=0",
                "1\t5000000.25\t500000.5\t101.75\tKerb, north\n",
            ),
            (
                "P,N,E,Z,D;delimiter=semicolon;header=0",
                "1;5000000.25;500000.5;101.75;Kerb, north\n",
            ),
            (
                "P,N,E,Z,D;delimiter=comma;header=0",
                "1, 5000000.25 ,500000.5,101.75,Kerb north\n",
            ),
            // A column-aligned listing: runs of blanks, blanks at both ends.
            (
                "P,N,E,Z,D;delimiter=whitespace;header=0",
                "   1    5000000.25\t 500000.5    101.75   \"Kerb, north\"   \n",
            ),
        ];
        for (template_text, text) in cases {
            let result = read_text(text, template_text)
                .unwrap_or_else(|error| panic!("{template_text}: {}", error.describe()));
            let point = &result.project.points[0];
            assert_eq!(point.id, "1", "{template_text}");
            assert_eq!(point.northing, NORTHING, "{template_text}");
            assert_eq!(point.easting, EASTING, "{template_text}");
            assert_eq!(point.elevation, Some(ELEVATION), "{template_text}");
            assert!(
                point.description == "Kerb, north" || point.description == "Kerb north",
                "{template_text}: {}",
                point.description
            );
        }
    }

    #[test]
    fn without_an_id_column_each_point_is_named_by_its_line_and_a_warning_says_so() {
        let text = "Northing,Easting,Elevation\n\
                    5000000.25,500000.5,101.75\n\
                    \n\
                    5000010.125,500020.375,99.5\n";
        let result = imported(read_text(text, "N,E,Z;delimiter=comma;header=1"));
        assert_eq!(result.project.points.len(), 2);
        assert_eq!(result.project.points[0].id, "2");
        assert_eq!(result.project.points[1].id, "4"); // line 3 is blank
        assert!(any_warning_contains(
            &result,
            "named by the number of the line it is on"
        ));
    }

    #[test]
    fn an_ignored_column_is_read_past_and_trailing_empty_fields_are_not_data() {
        let text = "1,2026-09-23,5000000.25,500000.5,,,\n";
        let result = imported(read_text(text, "P,-,N,E;delimiter=comma;header=0"));
        assert_eq!(result.project.points[0].northing, NORTHING);
        assert_eq!(result.project.points[0].easting, EASTING);
    }

    #[test]
    fn a_file_of_only_a_header_is_an_empty_import_and_says_so() {
        let result = imported(read_text(
            "Point,Northing,Easting\r\n",
            "P,N,E;delimiter=comma;header=1",
        ));
        assert!(result.project.points.is_empty());
        assert_eq!(result.records_read, 0);
        assert!(any_warning_contains(&result, "no points"));

        let nothing = imported(read_text("", "P,N,E;delimiter=comma;header=0"));
        assert!(nothing.project.points.is_empty());
    }
}

// ---- Import errors: each names its line and column, and nothing is imported ------

mod delimited_points_import_errors {
    use super::*;

    #[test]
    fn a_malformed_number_names_its_line_and_column() {
        // Line 3's easting has letter Os for zeros.
        let text = "Point,Northing,Easting\n\
                    1,5000000.25,500000.5\n\
                    2,5000000.25,5OOOOO.5\n";
        let result = read_text(text, "P,N,E;delimiter=comma;header=1");
        let error = refused(result); // and so no partial import either
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(
            contains(&error.message, "line 3, column 3 (easting)"),
            "{}",
            error.message
        );
        assert!(contains(&error.message, "'5OOOOO.5' is not a number"));
        assert_eq!(error.context, "points.csv");
    }

    #[test]
    fn numbers_that_are_not_plain_decimals_are_refused_not_read() {
        // A decimal comma, trailing text, a non-finite value, hexadecimal, two
        // signs: each could be read as SOME number, and each would be a guess.
        for bad in [
            "5000000,25",
            "5000000.25m",
            "inf",
            "nan",
            "0x4C4B40",
            "+-5000000",
            "1e999",
        ] {
            let text = format!("1;{bad};500000.5\n");
            let result = read_text(&text, "P,N,E;delimiter=semicolon;header=0");
            assert!(result.is_err(), "{bad}: {}", error_of(&result));
            let error = refused(result);
            assert!(
                contains(&error.message, "line 1, column 2 (northing)"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn an_empty_northing_or_easting_is_an_error() {
        let northing = refused(read_text("1,,500000.5\n", "P,N,E;delimiter=comma;header=0"));
        assert!(
            contains(&northing.message, "line 1, column 2 (northing): empty"),
            "{}",
            northing.message
        );

        let easting = refused(read_text(
            "1,5000000.25, \n",
            "P,N,E;delimiter=comma;header=0",
        ));
        assert!(
            contains(&easting.message, "line 1, column 3 (easting): empty"),
            "{}",
            easting.message
        );
    }

    #[test]
    fn a_line_too_short_to_reach_a_required_column_is_an_error() {
        let error = refused(read_text(
            "1,5000000.25,500000.5,101.75\n2,5000000.25\n",
            "P,N,E,Z;delimiter=comma;header=0",
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(
            contains(&error.message, "line 2, column 3 (easting): missing"),
            "{}",
            error.message
        );
    }

    #[test]
    fn text_past_the_last_column_is_an_error_rather_than_a_cut_description() {
        // An unquoted comma in the description splits it into a sixth field;
        // taking the first five would import "Kerb" and lose " north side"
        // silently.
        let error = refused(read_text(
            "1,5000000.25,500000.5,101.75,Kerb, north side\n",
            "P,N,E,Z,D;delimiter=comma;header=0",
        ));
        assert!(
            contains(&error.message, "line 1: 6 fields where the layout has 5"),
            "{}",
            error.message
        );
        assert!(contains(&error.message, "column 6 holds 'north side'"));
    }

    #[test]
    fn an_empty_point_id_is_an_error_and_no_id_is_invented() {
        let error = refused(read_text(
            "1,5000000.25,500000.5\n\"\",5000000.25,500010.5\n",
            "P,N,E;delimiter=comma;header=0",
        ));
        assert!(
            contains(&error.message, "line 2, column 1 (point id): empty"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_repeated_point_id_is_an_error_naming_both_lines() {
        let text = "Point,Northing,Easting\n\
                    A1,5000000.25,500000.5\n\
                    A2,5000000.25,500010.5\n\
                    A1,5000000.25,500020.5\n";
        let error = refused(read_text(text, "P,N,E;delimiter=comma;header=1"));
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        assert!(
            contains(&error.message, "line 4, column 1 (point id)"),
            "{}",
            error.message
        );
        assert!(contains(&error.message, "the point on line 2"));
    }

    #[test]
    fn a_quote_that_never_closes_names_the_line_it_opens_on() {
        let error = refused(read_text(
            "1,5000000.25,500000.5,\"Kerb\n2,5000000.25,500010.5,Fence\n",
            "P,N,E,D;delimiter=comma;header=0",
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(
            contains(&error.message, "line 1, column 4"),
            "{}",
            error.message
        );
        assert!(contains(&error.message, "never closed"));
    }

    #[test]
    fn text_after_a_closing_quote_is_an_error() {
        let error = refused(read_text(
            "1,5000000.25,500000.5,\"Kerb\" north\n",
            "P,N,E,D;delimiter=comma;header=0",
        ));
        assert!(
            contains(&error.message, "line 1, column 4"),
            "{}",
            error.message
        );
        assert!(contains(&error.message, "follows the closing quote"));
    }

    #[test]
    fn an_unusable_layout_is_refused_before_anything_is_read() {
        let layout = DelimitedLayout {
            columns: vec![ColumnRole::PointId, ColumnRole::Northing],
            ..Default::default()
        };
        let error = refused(parse_delimited_points(
            b"1,5000000.25\n",
            &layout,
            "points.csv",
            &in_metres(),
        ));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn bytes_that_claim_an_encoding_they_are_not_in_are_an_error() {
        // A UTF-8 byte order mark followed by a byte that cannot be UTF-8: a
        // truncated or corrupt file, not something to decode to anything
        // plausible.
        let error = refused(read_text(
            b"\xEF\xBB\xBF1,5000000.25,500000.5,,Caf\xE9\n",
            "P,N,E,Z,D;delimiter=comma;header=0",
        ));
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }
}

// ---- Export ----------------------------------------------------------------------

fn point_of(
    id: &str,
    northing: f64,
    easting: f64,
    elevation: Option<f64>,
    code: &str,
    description: &str,
) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation,
        code: String::from(code),
        description: String::from(description),
        ..Default::default()
    }
}

fn export_options(decimals: i32, unit: LinearUnit) -> DelimitedExportOptions {
    DelimitedExportOptions { decimals, unit }
}

fn written(result: Result<String>) -> String {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod delimited_points_export {
    use super::*;

    #[test]
    fn fixed_decimals_and_an_absent_elevation_written_as_an_empty_field() {
        let points = [
            point_of("1", 5000000.25, 500000.5, Some(101.75), "KB", "Kerb"),
            point_of("2", 5000010.125, 500020.375, None, "", ""),
        ];
        let text = written(write_delimited_points(
            &points,
            &preset_layout(ColumnPreset::PNEZD, Delimiter::Comma, 1),
            &export_options(3, LinearUnit::Metres),
        ));
        // Worked by hand: every value is exact in binary, so three places are
        // just its digits padded with zeros; point 2's elevation is absent -
        // empty, never 0.000.
        assert_eq!(
            text,
            "Point,Northing,Easting,Elevation,Description\r\n\
             1,5000000.250,500000.500,101.750,Kerb\r\n\
             2,5000010.125,500020.375,,\r\n"
        );

        // Whitespace cannot hold an empty field unquoted.
        let aligned = written(write_delimited_points(
            &points,
            &preset_layout(ColumnPreset::PENZ, Delimiter::Whitespace, 0),
            &export_options(2, LinearUnit::Metres),
        ));
        // Two places of 500020.375 and 5000010.125: both are exact in binary,
        // so both are exact ties, and they are rounded as printf rounds a tie
        // under IEEE 754 - to the even last digit: .375 -> .38, .125 -> .12.
        assert_eq!(
            aligned,
            "1 500000.50 5000000.25 101.75\r\n\
             2 500020.38 5000010.12 \"\"\r\n"
        );
    }

    #[test]
    fn a_value_that_rounds_to_zero_is_written_without_a_sign() {
        let points = [point_of("1", -0.0004, 0.0, Some(-0.0), "", "")];
        let text = written(write_delimited_points(
            &points,
            &preset_layout(ColumnPreset::PNEZ, Delimiter::Comma, 0),
            &export_options(3, LinearUnit::Metres),
        ));
        assert_eq!(text, "1,0.000,0.000,0.000\r\n");
    }

    #[test]
    fn feet_are_written_from_metres_exactly() {
        // metres * 1250 / 381 (from_metres with the international foot). The
        // double 304.8 is 304.80000000000001137; times 1250 that is
        // 381000.0000000000142, and half an ulp at 381000 is 2^-35 = 2.9e-11,
        // so the product rounds to 381000 exactly and 381000 / 381 = 1000.
        // Likewise 609.6 is 609.60000000000002274; times 1250,
        // 762000.0000000000284, inside half an ulp (2^-34 = 5.8e-11) of 762000,
        // which / 381 = 2000. So the file says 1000.000 and 2000.000, not
        // 999.999...
        let points = [point_of("1", 304.8, 609.6, None, "", "")];
        let text = written(write_delimited_points(
            &points,
            &preset_layout(ColumnPreset::PNE, Delimiter::Comma, 0),
            &export_options(3, LinearUnit::Feet),
        ));
        assert_eq!(text, "1,1000.000,2000.000\r\n");
    }

    #[test]
    fn fields_that_would_be_misread_are_quoted() {
        let layout = DelimitedLayout {
            columns: vec![
                ColumnRole::PointId,
                ColumnRole::Northing,
                ColumnRole::Easting,
                ColumnRole::Description,
            ],
            comment_prefix: String::from("#"),
            ..Default::default()
        };
        let points = [
            point_of("#1", 1.0, 2.0, None, "", "Kerb, north"),
            point_of("2", 1.0, 2.0, None, "", "6\" PVC"),
            point_of("3", 1.0, 2.0, None, "", " padded"),
            point_of("4", 1.0, 2.0, None, "", "two\nlines"),
        ];
        let text = written(write_delimited_points(
            &points,
            &layout,
            &export_options(0, LinearUnit::Metres),
        ));
        assert_eq!(
            text,
            "\"#1\",1,2,\"Kerb, north\"\r\n\
             2,1,2,\"6\"\" PVC\"\r\n\
             3,1,2,\" padded\"\r\n\
             4,1,2,\"two\nlines\"\r\n"
        );
    }

    #[test]
    fn a_field_after_empty_tab_fields_is_quoted_when_it_would_start_the_line_as_a_comment() {
        // The reader looks for the comment prefix once the line is trimmed of
        // blanks, tabs among them. In a tab file whose first column is written
        // empty, the line therefore starts with the second column: an id "#12"
        // there is where a comment would start, and unquoted, the point would
        // be skipped as one.
        let points = [
            point_of("1", 100.0, 200.0, None, "", ""),
            point_of("#12", 110.0, 210.0, None, "", ""),
        ];
        let layout = layout_of("-,P,N,E;delimiter=tab;header=0;quote=double;comment=#");
        let text = written(write_delimited_points(
            &points,
            &layout,
            &export_options(3, LinearUnit::Metres),
        ));
        // Worked by hand: an empty Ignore field, then the id, then the
        // coordinates to three places; "#12" is the first thing on its line, so
        // it is quoted.
        assert_eq!(
            text,
            "\t1\t100.000\t200.000\r\n\
             \t\"#12\"\t110.000\t210.000\r\n"
        );
        let back = imported(parse_delimited_points(
            text.as_bytes(),
            &layout,
            "export.txt",
            &in_metres(),
        ));
        assert_eq!(back.project.points.len(), 2);
        assert_eq!(back.project.points[1].id, "#12");
        assert!(!any_warning_contains(&back, "comment"));

        // An empty code in front does the same, and with quoting off the point
        // cannot be written at all.
        let layout = layout_of("C,P,N,E;delimiter=tab;header=1;quote=none;comment=#");
        let unquotable =
            write_delimited_points(&points, &layout, &export_options(3, LinearUnit::Metres))
                .expect_err("the point cannot be written");
        assert_eq!(unquotable.code, ErrorCode::InvalidArgument);
        assert!(
            contains(&unquotable.message, "point '#12': its point id"),
            "{}",
            unquotable.message
        );
    }

    #[test]
    fn what_cannot_be_written_is_refused_naming_the_point() {
        let points = [point_of("1", NORTHING, EASTING, Some(ELEVATION), "", "")];
        let pnez = preset_layout(ColumnPreset::PNEZ, Delimiter::Comma, 0);
        let metres = |decimals| export_options(decimals, LinearUnit::Metres);

        // Unset decimals and unit: the surveyor's statement, not this writer's.
        assert_eq!(
            write_delimited_points(&points, &pnez, &DelimitedExportOptions::default())
                .expect_err("nothing is stated")
                .code,
            ErrorCode::InvalidArgument
        );
        assert!(write_delimited_points(&points, &pnez, &metres(-1)).is_err());
        assert!(
            write_delimited_points(&points, &pnez, &metres(MAXIMUM_EXPORT_DECIMALS + 1)).is_err()
        );
        assert!(write_delimited_points(&points, &pnez, &metres(MAXIMUM_EXPORT_DECIMALS)).is_ok());
        assert!(
            write_delimited_points(&points, &pnez, &export_options(3, LinearUnit::Unknown))
                .is_err()
        );

        // Two header lines: this writer has one header line to write.
        assert!(
            write_delimited_points(
                &points,
                &preset_layout(ColumnPreset::PNEZ, Delimiter::Comma, 2),
                &metres(3)
            )
            .is_err()
        );

        // Ids the reader would refuse.
        let repeated = [
            point_of("A", 1.0, 2.0, Some(3.0), "", ""),
            point_of("A", 4.0, 5.0, Some(6.0), "", ""),
        ];
        let twice =
            write_delimited_points(&repeated, &pnez, &metres(3)).expect_err("the id is repeated");
        assert!(contains(
            &twice.message,
            "'A' is used twice (points 1 and 2)"
        ));
        let unnamed = [point_of("", 1.0, 2.0, Some(3.0), "", "")];
        assert!(write_delimited_points(&unnamed, &pnez, &metres(3)).is_err());
        // ...unless the layout writes no id, when neither matters.
        assert!(
            write_delimited_points(
                &repeated,
                &preset_layout(ColumnPreset::NEZ, Delimiter::Comma, 0),
                &metres(3)
            )
            .is_ok()
        );

        let infinite = [point_of("1", f64::INFINITY, 2.0, Some(3.0), "", "")];
        let not_finite = write_delimited_points(&infinite, &pnez, &metres(3))
            .expect_err("the northing is not finite");
        assert!(contains(&not_finite.message, "point '1': its northing"));

        // With quoting off, a field that needs quotes cannot be written at all.
        let mut plain = preset_layout(ColumnPreset::PNEZD, Delimiter::Comma, 0);
        plain.quoting = Quoting::None;
        let comma = [point_of("1", 1.0, 2.0, Some(3.0), "", "Kerb, north")];
        let unquotable = write_delimited_points(&comma, &plain, &metres(3))
            .expect_err("the description cannot be written");
        assert!(
            contains(&unquotable.message, "point '1': its description"),
            "{}",
            unquotable.message
        );
        let mut aligned_plain = preset_layout(ColumnPreset::PNEZ, Delimiter::Whitespace, 0);
        aligned_plain.quoting = Quoting::None;
        let no_height = [point_of("1", 1.0, 2.0, None, "", "")];
        assert!(write_delimited_points(&no_height, &aligned_plain, &metres(3)).is_err());
    }

    #[test]
    fn what_export_writes_import_reads_back_unchanged() {
        // Every awkward value: a comment prefix at the start of an id, blanks
        // inside and round an id, a comma, a quote and a line break in a
        // description, a semicolon in a code, UTF-8, an absent elevation and a
        // real zero one. Coordinates are exact at four places (sixteenths), so
        // rounding to four places changes nothing and the comparison can be
        // exact.
        let points = [
            point_of(
                "1",
                5000000.25,
                500000.5,
                Some(101.75),
                "KB",
                "Kerb, north side",
            ),
            point_of("#2", 5000010.125, 500020.375, None, "", "6\" PVC"),
            point_of(
                "Pünkt 3",
                5000020.5,
                500040.0625,
                Some(-0.5),
                "Bäume",
                "line one\nline two",
            ),
            point_of(" padded ", 5000030.0, 500060.0, Some(0.0), "; semi", ""),
        ];
        let templates = [
            "P,N,E,Z,D;delimiter=comma;header=1",
            "P,E,N,Z,C,D;delimiter=whitespace;header=0",
            "P,N,E,Z,C,D;delimiter=tab;header=1;comment=#",
            "P,-,N,E,Z,C,D;delimiter=semicolon;header=0",
            // Empty fields in front of the id: an Ignore column, and a code that
            // is empty for "#2". The line then starts with tabs, which a reader
            // trims.
            "-,P,N,E,Z,D;delimiter=tab;header=0;comment=#",
            "C,P,N,E,Z,D;delimiter=tab;header=1;comment=#",
        ];
        for template_text in templates {
            let layout = layout_of(template_text);
            let text =
                write_delimited_points(&points, &layout, &export_options(4, LinearUnit::Metres))
                    .unwrap_or_else(|error| panic!("{template_text}: {}", error.describe()));
            let back = parse_delimited_points(text.as_bytes(), &layout, "export.csv", &in_metres())
                .unwrap_or_else(|error| panic!("{template_text}: {}\n{text}", error.describe()));
            assert_eq!(back.project.points.len(), points.len(), "{template_text}");
            let codes = layout.columns.contains(&ColumnRole::Code);
            for (original, read) in points.iter().zip(&back.project.points) {
                assert_eq!(read.id, original.id, "{template_text}");
                assert_eq!(read.northing, original.northing, "{template_text}");
                assert_eq!(read.easting, original.easting, "{template_text}");
                assert_eq!(read.elevation, original.elevation, "{template_text}");
                assert_eq!(read.description, original.description, "{template_text}");
                assert_eq!(
                    read.code,
                    if codes { original.code.as_str() } else { "" },
                    "{template_text}"
                );
            }
        }
    }
}

// ---- The format in the registry -----------------------------------------------------

mod delimited_points_format {
    use super::*;

    #[test]
    fn the_format_is_registered_and_states_that_it_carries_points_only() {
        // In the program's list of formats (format.rs) and so in the
        // process-wide registry.
        let format = format_registry()
            .find(DELIMITED_POINTS_FORMAT_ID)
            .expect("the format is registered");
        assert_eq!(format, delimited_points_format());

        let points_only = FormatContent {
            points: true,
            ..Default::default()
        };
        assert_eq!(format.reads, points_only);
        assert!(format.can_import);
        assert!(format.can_export);
        assert_eq!(format.manufacturer, Manufacturer::Generic);
        assert_eq!(
            describe_format(&format),
            "Delimited text points (CSV, TXT), import: yes, export: yes, parser: 1.0"
        );
    }
}
