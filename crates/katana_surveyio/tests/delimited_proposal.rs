//! Twin of tests/surveyio/test_delimited_proposal.cpp.
//!
//! Proposing a delimited file's layout, and detecting the format at all.
//!
//! The property under test throughout: the order of northing and easting is
//! decided only by a header that names them. Fixtures keep northing near
//! 5 000 000 and easting near 500 000 so that a transposition is visible.

mod support;

use katana_core::ErrorCode;
use katana_core::text_encoding::{decode_text, encode_utf16_little_endian};
use katana_survey::{LinearUnit, SurveyPoint};
use katana_surveyio::{
    ColumnRole, DELIMITED_HEADER_CONFIDENCE, DELIMITED_HEADERLESS_CONFIDENCE,
    DELIMITED_POINTS_FORMAT_ID, DelimitedExportOptions, DelimitedImportOptions, DelimitedLayout,
    Delimiter, DetectionOutcome, FormatContent, FormatDescriptor, FormatRegistry, FormatSignature,
    IDENTIFIED_CONFIDENCE, LayoutProposal, LayoutProposalOutcome, Manufacturer, PROBE_BYTES,
    PROPOSAL_SAMPLE_LINES, ProbeInput, all_column_presets, delimited_points_format, detect_format,
    detect_format_with, parse_delimited_points, preset_layout, probe_delimited_points, probe_of,
    propose_layout, to_string, write_delimited_points,
};

use support::contains;

// The text as bytes: a str fixture, or bytes built or encoded by the test.
fn propose<T: AsRef<[u8]> + ?Sized>(text: &T) -> LayoutProposal {
    propose_sample(text, false)
}

fn propose_sample<T: AsRef<[u8]> + ?Sized>(text: &T, truncated: bool) -> LayoutProposal {
    propose_layout(text.as_ref(), truncated).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn roles(letters: &str) -> Vec<ColumnRole> {
    letters
        .chars()
        .map(|letter| match letter {
            'P' => ColumnRole::PointId,
            'N' => ColumnRole::Northing,
            'E' => ColumnRole::Easting,
            'Z' => ColumnRole::Elevation,
            'C' => ColumnRole::Code,
            'D' => ColumnRole::Description,
            _ => ColumnRole::Ignore,
        })
        .collect()
}

// The layout of a proposal that must be Decided.
fn decided(proposal: &LayoutProposal) -> DelimitedLayout {
    proposal
        .layout()
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn in_metres() -> DelimitedImportOptions {
    DelimitedImportOptions {
        unit: LinearUnit::Metres,
        ..Default::default()
    }
}

fn specific_format() -> FormatDescriptor {
    FormatDescriptor {
        id: String::from("vendor-csv"),
        human_name: String::from("A vendor's CSV export"),
        manufacturer: Manufacturer::Other,
        reads: FormatContent {
            points: true,
            ..Default::default()
        },
        can_import: true,
        parser_version: String::from("1.0"),
        ..Default::default()
    }
}

// The weakest a specific format can be and still be identified: exactly at the
// bar. If the generic probe cannot come within the margin of THIS, it cannot
// come within the margin of any identified format.
fn just_identified(_: &ProbeInput<'_>) -> FormatSignature {
    FormatSignature {
        confidence: IDENTIFIED_CONFIDENCE,
        evidence: String::from("a signature only that vendor writes"),
    }
}

fn delimited_only() -> FormatRegistry {
    let mut registry = FormatRegistry::default();
    assert!(
        registry
            .add(
                delimited_points_format(),
                Some(probe_delimited_points),
                None
            )
            .is_ok()
    );
    registry
}

mod delimited_layout_proposal {
    use super::*;

    // ---- A header decides the order ----------------------------------------------

    #[test]
    fn a_header_naming_easting_first_decides_an_easting_first_layout() {
        let text = "Point,Easting,Northing,Elevation,Code\r\n\
                    1,500000.5,5000000.25,101.75,KB\r\n\
                    2,500020.375,5000010.125,99.5,FL\r\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        let layout = decided(&proposal);
        assert_eq!(layout.columns, roles("PENZC"));
        assert_eq!(layout.delimiter, Delimiter::Comma);
        assert_eq!(layout.header_lines, 1);

        // And reading with it puts each number where the header said.
        let result = parse_delimited_points(text.as_bytes(), &layout, "points.csv", &in_metres())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(result.project.points[0].northing, 5000000.25);
        assert_eq!(result.project.points[0].easting, 500000.5);
        assert_eq!(result.project.points[0].code, "KB");
    }

    #[test]
    fn a_header_naming_northing_first_decides_a_northing_first_layout() {
        let text = "PT   N             E           Z\n\
                    1    5000000.25    500000.5    101.75\n\
                    2    5000010.125   500020.375  99.5\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        assert_eq!(decided(&proposal).columns, roles("PNEZ"));
        assert_eq!(decided(&proposal).delimiter, Delimiter::Whitespace);
        assert_eq!(decided(&proposal).header_lines, 1);
        assert!(
            contains(
                proposal.summary(),
                "northing in column 2, easting in column 3"
            ),
            "{}",
            proposal.summary()
        );
    }

    #[test]
    fn header_words_are_read_without_case_units_or_punctuation() {
        let text = "Point No.;NORTHING (m);easting_[m];R.L.;Feature Code;Remarks\n\
                    1;5000000.25;500000.5;101.75;KB;Kerb\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        assert_eq!(decided(&proposal).columns, roles("PNEZCD"));
        assert_eq!(decided(&proposal).delimiter, Delimiter::Semicolon);
    }

    #[test]
    fn an_unknown_header_word_ignores_its_column_and_says_so() {
        let text = "Point,Date,Northing,Easting\n\
                    1,2026-09-23,5000000.25,500000.5\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        assert_eq!(decided(&proposal).columns, roles("P-NE"));
        assert!(
            contains(proposal.summary(), "column 2 ('Date')"),
            "{}",
            proposal.summary()
        );
    }

    // ---- Nothing else decides it -------------------------------------------------

    #[test]
    fn a_file_with_no_header_is_uncertain_and_offers_both_coordinate_orders() {
        let text = "1,5000000.25,500000.5,101.75,Kerb\n\
                    2,5000010.125,500020.375,99.5,Fence\n";
        let proposal = propose(text);
        assert_eq!(proposal.outcome(), LayoutProposalOutcome::Uncertain);
        // The only way to a usable layout is to choose one.
        assert_eq!(
            proposal.layout().expect_err("no layout was decided").code,
            ErrorCode::NotFound
        );

        // Both five-column orders fit exactly and come first; the rest fit only
        // by ignoring columns. Nothing in "5000000.25" says it is a northing,
        // so the two orders are both offered and neither is preferred on the
        // numbers.
        let candidates = proposal.candidates();
        assert!(candidates.len() >= 2);
        assert_eq!(candidates[0].layout.columns, roles("PNEZD"));
        assert_eq!(candidates[1].layout.columns, roles("PENZD"));
        for candidate in candidates {
            assert_eq!(candidate.layout.header_lines, 0);
            assert_eq!(candidate.layout.delimiter, Delimiter::Comma);
            assert!(
                contains(&candidate.evidence, "no header"),
                "{}",
                candidate.evidence
            );
        }
        assert!(
            contains(proposal.summary(), "northing or easting"),
            "{}",
            proposal.summary()
        );
    }

    #[test]
    fn x_and_y_are_uncertain_with_x_as_easting_ranked_first() {
        let text = "X,Y,Z\n\
                    500000.5,5000000.25,101.75\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            proposal.summary()
        );
        assert_eq!(proposal.candidates().len(), 2);
        assert_eq!(proposal.candidates()[0].layout.columns, roles("ENZ"));
        assert_eq!(proposal.candidates()[1].layout.columns, roles("NEZ"));
        assert_eq!(proposal.candidates()[0].layout.header_lines, 1);
        assert!(
            contains(proposal.summary(), "Gauss-Kruger"),
            "{}",
            proposal.summary()
        );
        assert!(proposal.layout().is_err());
    }

    #[test]
    fn a_header_that_does_not_name_the_coordinates_does_not_decide_them() {
        let text = "Col1,Col2,Col3,Col4\n\
                    1,5000000.25,500000.5,101.75\n";
        let proposal = propose(text);
        assert_eq!(proposal.outcome(), LayoutProposalOutcome::Uncertain);
        assert!(proposal.candidates().len() >= 2);
        assert_eq!(proposal.candidates()[0].layout.columns, roles("PNEZ"));
        assert_eq!(proposal.candidates()[1].layout.columns, roles("PENZ"));
        assert_eq!(proposal.candidates()[0].layout.header_lines, 1); // the header is still skipped

        // One named coordinate is not an order either.
        let half = propose("Point,Northing,Other\n1,5000000.25,500000.5\n");
        assert_eq!(half.outcome(), LayoutProposalOutcome::Uncertain);
    }

    #[test]
    fn rows_that_contradict_the_header_make_it_uncertain() {
        // Line 3's northing is text, so the header cannot be right for every
        // row (or the file is damaged); either way a person should look.
        let text = "Point,Northing,Easting,Elevation\n\
                    1,5000000.25,500000.5,101.75\n\
                    2,abc,500010.5,99.5\n";
        let proposal = propose(text);
        assert_eq!(proposal.outcome(), LayoutProposalOutcome::Uncertain);
        assert_eq!(proposal.candidates().len(), 1);
        assert_eq!(proposal.candidates()[0].layout.columns, roles("PNEZ"));
        assert!(
            contains(proposal.summary(), "line 3, column 2 (northing)"),
            "{}",
            proposal.summary()
        );
    }

    #[test]
    fn values_in_a_column_the_header_does_not_name_make_it_uncertain() {
        // The header names three columns and every row has a fourth - an
        // elevation, by the look of it, that a Decided layout would read past
        // as Ignore and import no point with. The header does not describe
        // these rows.
        let text = "Point,Northing,Easting\n\
                    1,5000000.25,500000.5,101.75\n\
                    2,5000010.125,500020.375,99.5\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            proposal.summary()
        );
        assert_eq!(proposal.candidates().len(), 1);
        // The header's reading is still offered, and it parses: the unnamed
        // column is Ignore rather than text past the last column.
        assert_eq!(proposal.candidates()[0].layout.columns, roles("PNE-"));
        assert!(
            contains(proposal.summary(), "line 2 has a value in column 4"),
            "{}",
            proposal.summary()
        );
        assert!(
            contains(proposal.summary(), "'101.75'"),
            "{}",
            proposal.summary()
        );

        // The same with blanks: a description with a space in it runs one
        // field past the header, and taking the layout would cut "Kerb line"
        // to "Kerb".
        let aligned = propose(
            "Point Northing Easting Description\n\
             1 5000000.25 500000.5 Kerb line\n",
        );
        assert_eq!(
            aligned.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            aligned.summary()
        );
        assert!(
            contains(aligned.summary(), "column 5"),
            "{}",
            aligned.summary()
        );

        // Empty trailing fields are not values - "a,b,c," is how many writers
        // end a row - so they leave the header in charge. The layout is the
        // header's three columns and no more: see
        // a_value_after_the_sample_in_an_empty_trailing_column_is_an_error_not_lost.
        let trailing =
            propose("Point,Northing,Easting\n1,5000000.25,500000.5,\n2,5000010.125,500020.375,,\n");
        assert_eq!(
            trailing.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            trailing.summary()
        );
        assert_eq!(decided(&trailing).columns, roles("PNE"));
    }

    #[test]
    fn a_value_under_a_blank_header_cell_makes_it_uncertain() {
        // A spreadsheet with one column nobody headed writes an empty header
        // cell. Its column is not named, so a Decided layout could only read
        // past it - here line 2's code 'KB' would be lost without a word.
        let trailing_blank = propose(
            "Point,Northing,Easting,Elevation,\n\
             1,5000000.25,500000.5,101.75,KB\n\
             2,5000010.125,500020.375,99.5,FL\n",
        );
        assert_eq!(
            trailing_blank.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            trailing_blank.summary()
        );
        assert!(
            contains(trailing_blank.summary(), "line 2 has a value in column 5"),
            "{}",
            trailing_blank.summary()
        );
        assert!(
            contains(trailing_blank.summary(), "'KB'"),
            "{}",
            trailing_blank.summary()
        );

        // The blank cell in the middle: column 4 holds the elevations.
        let middle_blank = propose(
            "Point,Northing,Easting,,Description\n\
             1,5000000.25,500000.5,101.75,Kerb\n",
        );
        assert_eq!(
            middle_blank.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            middle_blank.summary()
        );
        assert!(
            contains(middle_blank.summary(), "line 2 has a value in column 4"),
            "{}",
            middle_blank.summary()
        );

        // The same with tabs, where the blank cell is the header's last tab.
        let tabbed = propose(
            "Point\tNorthing\tEasting\t\n\
             1\t5000000.25\t500000.5\tKB\n",
        );
        assert_eq!(
            tabbed.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            tabbed.summary()
        );
        assert!(
            contains(tabbed.summary(), "line 2 has a value in column 4"),
            "{}",
            tabbed.summary()
        );

        // A blank header cell over a column that is empty in every row leaves
        // the header in charge: nothing is there to lose.
        let empty_column = propose(
            "Point,Northing,Easting,Elevation,\n\
             1,5000000.25,500000.5,101.75,\n\
             2,5000010.125,500020.375,99.5,\n",
        );
        assert_eq!(
            empty_column.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            empty_column.summary()
        );
        assert_eq!(decided(&empty_column).columns, roles("PNEZ"));
    }

    #[test]
    fn a_value_after_the_sample_in_an_empty_trailing_column_is_an_error_not_lost() {
        // Every sampled row ends with an empty fifth field, and a row after the
        // sample has a description there. A Decided layout with an Ignore fifth
        // column would read that description past without an error or a
        // warning; the header names four columns, so the layout has four, and
        // the parser's own check - text past the last column - names the line.
        let rows = PROPOSAL_SAMPLE_LINES + 50;
        for header in [
            "Point,Northing,Easting,Elevation\n",
            "Point,Northing,Easting,Elevation,\n",
        ] {
            let mut text = String::from(header);
            for i in 1..rows {
                text.push_str(&format!("{i},5000000.25,{},101.75,\n", 500000 + i));
            }
            text.push_str(&format!(
                "{rows},5000000.25,500000.5,101.75,Boundary peg - check\n"
            ));

            let proposal = propose(&text);
            assert_eq!(
                proposal.outcome(),
                LayoutProposalOutcome::Decided,
                "{header}: {}",
                proposal.summary()
            );
            let layout = decided(&proposal);
            assert_eq!(layout.columns, roles("PNEZ"), "{header}");

            let error =
                parse_delimited_points(text.as_bytes(), &layout, "points.csv", &in_metres())
                    .map(|result| result.project.points.len())
                    .expect_err("the value after the sample is refused");
            // The header is line 1 and point i is on line i + 1, so the last
            // point, number `rows`, is on line rows + 1 = 251.
            assert!(
                contains(&error.message, &format!("line {}:", rows + 1)),
                "{header}: {}",
                error.message
            );
            assert!(
                contains(&error.message, "column 5 holds 'Boundary peg - check'"),
                "{header}: {}",
                error.message
            );
        }
    }

    #[test]
    fn a_header_name_with_a_blank_in_it_is_one_column_of_a_whitespace_file() {
        // "Easting (m)" split on its blank is two header words, "Easting" and
        // "(m)", and every header word after it would then sit one column to
        // the right of its data. A word with no letters of its own - the unit -
        // and anything inside a bracket belong to the word before them.
        let northing_first = "Northing (m)   Easting (m)   Height (m)\n\
                              5000000.25     500000.5      101.75\n";
        let proposal = propose(northing_first);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        let layout = decided(&proposal);
        assert_eq!(layout.delimiter, Delimiter::Whitespace);
        assert_eq!(layout.columns, roles("NEZ"));
        let read = parse_delimited_points(
            northing_first.as_bytes(),
            &layout,
            "points.txt",
            &in_metres(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        // The values the fixture's second line writes under each word.
        assert_eq!(read.project.points[0].northing, 5000000.25);
        assert_eq!(read.project.points[0].easting, 500000.5);
        assert_eq!(read.project.points[0].elevation, Some(101.75));

        let easting_first = propose(
            "Pt   Easting (m)   Northing (m)   RL (m)\n\
             1    500000.5      5000000.25     101.75\n",
        );
        assert_eq!(
            easting_first.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            easting_first.summary()
        );
        assert_eq!(decided(&easting_first).columns, roles("PENZ"));

        // A unit with a blank inside its bracket is still one word's unit.
        let spaced_unit = propose(
            "Pt  Northing (US ft)  Easting [ US ft ]\n\
             1   16404199.5        1640419.75\n",
        );
        assert_eq!(
            spaced_unit.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            spaced_unit.summary()
        );
        assert_eq!(decided(&spaced_unit).columns, roles("PNE"));
    }

    #[test]
    fn a_header_with_more_names_than_the_rows_have_fields_is_uncertain() {
        // "Reduced Level" is one column of a whitespace file and two header
        // words: Point, Northing, Easting, Reduced, Level is five names over
        // rows of four fields, so the elevation 101.75 would sit under
        // "Reduced" (not a word this reader knows: Ignore) and "Level" would
        // head a column no row has.
        let proposal = propose(
            "Point  Northing     Easting     Reduced Level\n\
             1      5000000.25   500000.5    101.75\n",
        );
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            proposal.summary()
        );
        assert_eq!(proposal.candidates().len(), 1);
        assert!(
            contains(proposal.summary(), "5 columns"),
            "{}",
            proposal.summary()
        );
        assert!(
            contains(proposal.summary(), "more than 4 fields"),
            "{}",
            proposal.summary()
        );
        assert!(proposal.layout().is_err());

        // The comma form of the same fault: an unquoted comma inside "Height,
        // m" makes six header names over rows of five fields, and the code KB
        // would sit under the unit.
        let comma = propose(
            "Point,Northing,Easting,Height, m,Code\n\
             1,5000000.25,500000.5,101.75,KB\n",
        );
        assert_eq!(
            comma.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            comma.summary()
        );
        assert!(
            contains(comma.summary(), "6 columns"),
            "{}",
            comma.summary()
        );
        assert!(
            contains(comma.summary(), "more than 5 fields"),
            "{}",
            comma.summary()
        );
    }

    #[test]
    fn a_hash_line_that_is_otherwise_a_row_of_numbers_is_not_decided_to_be_a_comment() {
        // Line 3 is a point commented out, or a point whose id is "#2": the
        // text cannot say which, and reading it as a comment drops a point.
        let text = "Point,Northing,Easting\n\
                    1,5000000.25,500000.5\n\
                    #2,5000010.125,500020.375\n\
                    3,5000020.5,500040.0625\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Uncertain,
            "{}",
            proposal.summary()
        );
        assert_eq!(proposal.candidates().len(), 1);
        assert_eq!(proposal.candidates()[0].layout.columns, roles("PNE"));
        assert!(
            contains(proposal.summary(), "line 3 starts with '#'"),
            "{}",
            proposal.summary()
        );

        // With no header the proposal is uncertain anyway, but a '#' row above
        // the first row would otherwise be counted as a header line and skipped
        // with no warning at all; the summary has to say so.
        let headerless = propose("#1,5000000.25,500000.5\n2,5000010.125,500020.375\n");
        assert_eq!(headerless.outcome(), LayoutProposalOutcome::Uncertain);
        assert!(
            contains(headerless.summary(), "line 1 starts with '#'"),
            "{}",
            headerless.summary()
        );

        // A comment that is prose - even prose with numbers in it - is a
        // comment.
        let prose = propose(
            "Point,Northing,Easting\n\
             1,5000000.25,500000.5\n\
             # checked 23 September, 2 points\n\
             2,5000010.125,500020.375\n",
        );
        assert_eq!(
            prose.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            prose.summary()
        );
        assert_eq!(decided(&prose).comment_prefix, "#");
    }

    #[test]
    fn latitude_and_longitude_are_refused_rather_than_read_as_grid_coordinates() {
        let proposal = propose("Point,Latitude,Longitude,Height\n1,-33.865143,151.2099,58.5\n");
        assert_eq!(proposal.outcome(), LayoutProposalOutcome::Uncertain);
        assert!(proposal.candidates().is_empty());
        assert!(
            contains(proposal.summary(), "Latitude"),
            "{}",
            proposal.summary()
        );
    }

    // ---- The rest is inferred ------------------------------------------------------

    #[test]
    fn the_delimiter_is_found_from_the_sample() {
        let cases = [
            ("P\tN\tE\n1\t5000000.25\t500000.5\n", Delimiter::Tab),
            ("P;N;E\n1;5000000.25;500000.5\n", Delimiter::Semicolon),
            ("P,N,E\n1,5000000.25,500000.5\n", Delimiter::Comma),
            ("P N E\n1 5000000.25 500000.5\n", Delimiter::Whitespace),
            // Spaces inside a CSV's descriptions do not make it a whitespace
            // file.
            (
                "P,N,E,D\n1,5000000.25,500000.5,Kerb line\n2,5000000.25,500010.5,Fence line\n",
                Delimiter::Comma,
            ),
            // Nor do commas inside a whitespace file's quoted descriptions.
            (
                "P N E D\n1 5000000.25 500000.5 \"Kerb, north\"\n",
                Delimiter::Whitespace,
            ),
        ];
        for (text, expected) in cases {
            let proposal = propose(text);
            assert_eq!(
                proposal.outcome(),
                LayoutProposalOutcome::Decided,
                "{text}: {}",
                proposal.summary()
            );
            assert_eq!(decided(&proposal).delimiter, expected, "{text}");
        }
    }

    #[test]
    fn decimal_commas_are_reported_rather_than_read_as_two_fields() {
        let proposal = propose(
            "1;5000000,25;500000,5;101,75\n\
             2;5000010,125;500020,375;99,5\n",
        );
        assert_eq!(proposal.outcome(), LayoutProposalOutcome::Uncertain);
        assert!(proposal.candidates().is_empty());
        assert!(
            contains(proposal.summary(), "decimal commas"),
            "{}",
            proposal.summary()
        );
    }

    #[test]
    fn title_and_comment_lines_above_the_header_are_skipped_with_it() {
        let text = "Job: Site A\r\n\
                    # exported 2026-09-23\r\n\
                    \r\n\
                    Point,Northing,Easting\r\n\
                    1,5000000.25,500000.5\r\n\
                    # a note between rows\r\n\
                    2,5000010.125,500020.375\r\n";
        let proposal = propose(text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        let layout = decided(&proposal);
        assert_eq!(layout.header_lines, 4);
        assert_eq!(layout.comment_prefix, "#");

        let result = parse_delimited_points(text.as_bytes(), &layout, "points.csv", &in_metres())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(result.project.points.len(), 2);
    }

    #[test]
    fn a_file_with_no_rows_of_numbers_has_no_candidates() {
        let prose = propose("The quick brown fox\njumps over the lazy dog\n");
        assert_eq!(prose.outcome(), LayoutProposalOutcome::Uncertain);
        assert!(prose.candidates().is_empty());

        // A row of numbers followed by a line that is not one: no delimiter
        // makes every row a row, and the summary names the line that broke it.
        let broken = propose("1,5000000.25,500000.5\n2,5000000.25,500010.5\nTotal: two points\n");
        assert_eq!(broken.outcome(), LayoutProposalOutcome::Uncertain);
        assert!(broken.candidates().is_empty());
        assert!(contains(broken.summary(), "line 3"), "{}", broken.summary());
    }

    #[test]
    fn nothing_to_look_at_is_empty() {
        assert_eq!(propose("").outcome(), LayoutProposalOutcome::Empty);
        assert_eq!(
            propose("\r\n  \r\n").outcome(),
            LayoutProposalOutcome::Empty
        );
        assert_eq!(
            propose("# only a comment\n").outcome(),
            LayoutProposalOutcome::Empty
        );
        assert!(propose("").layout().is_err());
    }

    #[test]
    fn a_truncated_sample_is_read_up_to_its_last_whole_line() {
        // UTF-16LE cut at an odd byte, as a 64 KiB detection sample can be.
        // Read as a whole file that is a corrupt file; read as a sample it is
        // the lines before the cut.
        let encoded = encode_utf16_little_endian(
            "Point\tNorthing\tEasting\r\n1\t5000000.25\t500000.5\r\n2\t5000010.125\t500020.375\r\n",
        )
        .expect("the text encodes");
        let cut = &encoded[..encoded.len() - 5];
        assert_eq!(cut.len() % 2, 1);

        assert!(propose_layout(cut, false).is_err());
        let proposal = propose_sample(cut, true);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        assert_eq!(decided(&proposal).columns, roles("PNE"));

        // A UTF-8 sample cut inside a character is still UTF-8. Read whole, the
        // lone lead byte at the end would make the sample invalid UTF-8 and
        // send it to the Windows-1252 fallback, and the header word "Höhe"
        // would come out mangled. The columns would match either way, so the
        // check is on the word the summary quotes back.
        let mut utf8 = "Point,Northing,Easting,Höhe\n1,5000000.25,500000.5,101.75\n2,5"
            .as_bytes()
            .to_vec();
        utf8.push(0xC3);
        let cut_utf8 = propose_sample(&utf8, true);
        assert_eq!(
            cut_utf8.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            cut_utf8.summary()
        );
        assert_eq!(decided(&cut_utf8).columns, roles("PNE-"));
        assert!(
            contains(cut_utf8.summary(), "'Höhe'"),
            "{}",
            cut_utf8.summary()
        );
    }

    #[test]
    fn only_the_start_of_a_large_file_is_decoded() {
        // A file marked UTF-8 with a byte that cannot be UTF-8 far past the
        // first PROBE_BYTES. Decoded whole it is a corrupt file (the premise,
        // checked first); the proposal decodes only its start and so still
        // proposes - the corrupt byte is the parser's to report, with its line.
        let mut text = b"\xEF\xBB\xBFPoint,Northing,Easting\r\n".to_vec();
        let mut i = 0;
        while text.len() < 3 * PROBE_BYTES {
            text.extend_from_slice(format!("PT{},5000000.25,500000.5\r\n", 100000 + i).as_bytes());
            i += 1;
        }
        text.extend_from_slice(b"PT-bad,5000000.25,500000.5,\xFF\r\n");
        assert!(decode_text(text.as_slice()).is_err());

        let proposal = propose(&text);
        assert_eq!(
            proposal.outcome(),
            LayoutProposalOutcome::Decided,
            "{}",
            proposal.summary()
        );
        assert_eq!(decided(&proposal).columns, roles("PNE"));
    }

    #[test]
    fn an_exported_file_proposes_the_layout_it_was_written_with() {
        let points = [
            SurveyPoint {
                id: String::from("1"),
                northing: 5000000.25,
                easting: 500000.5,
                elevation: Some(101.75),
                description: String::from("Kerb"),
                ..Default::default()
            },
            SurveyPoint {
                id: String::from("2"),
                northing: 5000010.125,
                easting: 500020.375,
                ..Default::default()
            },
        ];
        for &preset in all_column_presets() {
            let trace = to_string(preset);
            let layout = preset_layout(preset, Delimiter::Comma, 1);
            let options = DelimitedExportOptions {
                decimals: 3,
                unit: LinearUnit::Metres,
            };
            let text = write_delimited_points(&points, &layout, &options)
                .unwrap_or_else(|error| panic!("{trace}: {}", error.describe()));
            let proposal = propose(&text);
            assert_eq!(
                proposal.outcome(),
                LayoutProposalOutcome::Decided,
                "{trace}: {}",
                proposal.summary()
            );
            assert_eq!(decided(&proposal), layout, "{trace}");
        }
    }
}

// ---- Detection -----------------------------------------------------------------------

mod delimited_points_detection {
    use super::*;

    #[test]
    fn a_headerless_numeric_file_is_uncertain_rather_than_claimed() {
        let bytes = b"1,5000000.25,500000.5,101.75\n2,5000010.125,500020.375,99.5\n";
        let registry = delimited_only();
        let detection = detect_format_with(&probe_of(bytes, "points.csv", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Uncertain);
        assert_eq!(detection.candidates().len(), 1);
        assert_eq!(
            detection.candidates()[0].format_id,
            DELIMITED_POINTS_FORMAT_ID
        );
        assert_eq!(
            detection.candidates()[0].confidence,
            DELIMITED_HEADERLESS_CONFIDENCE
        );
        assert!(detection.format().is_err());
    }

    #[test]
    fn even_a_header_naming_the_coordinates_only_makes_it_a_candidate() {
        let bytes = b"Point,Northing,Easting\n1,5000000.25,500000.5\n";
        let registry = delimited_only();
        let detection = detect_format_with(&probe_of(bytes, "points.txt", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Uncertain);
        assert_eq!(detection.candidates().len(), 1);
        assert_eq!(
            detection.candidates()[0].confidence,
            DELIMITED_HEADER_CONFIDENCE
        );
        // Stronger evidence than no header, still below the bar: checked where
        // the compiler checks constants, in the const item below.
    }

    const _: () = {
        assert!(DELIMITED_HEADER_CONFIDENCE > DELIMITED_HEADERLESS_CONFIDENCE);
        assert!(DELIMITED_HEADER_CONFIDENCE < IDENTIFIED_CONFIDENCE);
    };

    #[test]
    fn a_specific_format_outranks_the_generic_reading_of_the_same_file() {
        let mut registry = delimited_only();
        assert!(
            registry
                .add(specific_format(), Some(just_identified), None)
                .is_ok()
        );

        // The file this probe likes best - a header naming the coordinates -
        // against a specific format that only just clears the bar: identified,
        // not ambiguous, with the generic reading still listed second.
        let bytes = b"Point,Northing,Easting\n1,5000000.25,500000.5\n";
        let detection = detect_format_with(&probe_of(bytes, "export.csv", false), &registry);
        assert_eq!(
            detection.outcome(),
            DetectionOutcome::Identified,
            "{}",
            detection.summary()
        );
        assert_eq!(
            detection.format().expect("the format is identified").id,
            "vendor-csv"
        );
        assert_eq!(detection.candidates().len(), 2);
        assert_eq!(
            detection.candidates()[1].format_id,
            DELIMITED_POINTS_FORMAT_ID
        );
    }

    #[test]
    fn what_is_not_rows_of_numbers_is_ruled_out() {
        let not_points: [&[u8]; 4] = [
            b"The quick brown fox\njumps over the lazy dog\n",
            // A Leica GSI-16 block: numbers inside words, never a field that is
            // one.
            b"*110001+0000000000000001 81..00+0000000000500000 82..00+0000000005000000\n",
            b"<?xml version=\"1.0\"?>\n<LandXML>\n<CgPoints>\n</CgPoints>\n</LandXML>\n",
            b"\x00\x01\x02\x03\xFF\xFE\x00\x13",
        ];
        for bytes in not_points {
            let signature = probe_delimited_points(&probe_of(bytes, "job.txt", false));
            assert_eq!(
                signature.confidence,
                0.0,
                "{}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn the_process_wide_registry_never_identifies_a_plain_coordinate_list() {
        // Whatever else is in the program's list, a list of numbers is not
        // claimed by this format, and this format is among the candidates
        // offered for it.
        let bytes = b"1 5000000.25 500000.5 101.75\n2 5000010.125 500020.375 99.5\n";
        let detection = detect_format(&probe_of(bytes, "points.txt", false));
        let mut offered = false;
        for candidate in detection.candidates() {
            if candidate.format_id == DELIMITED_POINTS_FORMAT_ID {
                offered = true;
                assert!(candidate.confidence < IDENTIFIED_CONFIDENCE);
            }
        }
        assert!(offered);
        if detection.outcome() == DetectionOutcome::Identified {
            assert_ne!(
                detection.format().expect("the format is identified").id,
                DELIMITED_POINTS_FORMAT_ID
            );
        }
    }
}
