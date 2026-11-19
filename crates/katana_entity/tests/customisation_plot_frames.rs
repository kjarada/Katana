//! The plot frames a customisation offers (docs/customisation.md, "The title
//! frames of a customisation"): the "plotFrames" member of version 2 of the
//! format - read in its order, written as the layout says and only by a
//! version 2 file, refused in a version 1 file, and each name held to the rule
//! a frame's name is held to.
//!
//! Every expectation is written by hand from those rules. The frame names are
//! invented.

pub mod customisation_support;

use katana_core::error::ErrorCode;
use katana_entity::customisation::{
    CUSTOMISATION_PLOT_FRAMES_VERSION, CUSTOMISATION_VERSION, customisation_to_json,
    validate_plot_frame_name,
};
use katana_entity::{Customisation, CustomisationPlotFrame, CustomisationWriteOptions};

use customisation_support::{expect_names, read_customisation, refusal_of, write_customisation};

fn frame(name: &str) -> CustomisationPlotFrame {
    CustomisationPlotFrame {
        name: String::from(name),
    }
}

fn offering(names: &[&str]) -> Customisation {
    Customisation {
        name: String::from("T"),
        plot_frames: names.iter().map(|name| frame(name)).collect(),
        ..Customisation::default()
    }
}

// A version 2 file holding only format, version, name and `members`.
fn version_2_with(members: &str) -> String {
    format!(r#"{{"format": "katana-customisation", "version": 2, "name": "T", {members}}}"#)
}

mod customisation_plot_frames {
    use super::*;

    #[test]
    fn the_version_that_offers_plot_frames_is_the_newest_this_build_reads() {
        assert_eq!(CUSTOMISATION_PLOT_FRAMES_VERSION, 2);
        assert_eq!(CUSTOMISATION_VERSION, 2);
    }

    #[test]
    fn a_version_2_file_offers_its_plot_frames_in_the_order_it_lists_them() {
        let read = read_customisation(version_2_with(
            r#""plotFrames": [{"name": "probe_frame_b"}, {"name": "probe_frame_a"}]"#,
        ));
        assert_eq!(read.plot_frames, vec![frame("probe_frame_b"), frame("probe_frame_a")]);
    }

    #[test]
    fn a_file_without_plot_frames_offers_none_whatever_its_version() {
        for version in [1, 2] {
            let text = format!(
                r#"{{"format": "katana-customisation", "version": {version}, "name": "T"}}"#
            );
            assert!(read_customisation(text).plot_frames.is_empty(), "{version}");
        }
    }

    // The layout of docs/customisation.md: the member after "automation" and
    // before "linestyles", its entries one a line four blanks in, each
    // {"name": ...} on its line, the closing bracket two blanks in.
    #[test]
    fn plot_frames_are_written_one_a_line_in_a_version_2_file() {
        let written = write_customisation(&offering(&["probe_frame", "probe_frame_2"]));
        assert_eq!(
            written,
            concat!(
                "{\n",
                "  \"format\": \"katana-customisation\",\n",
                "  \"version\": 2,\n",
                "  \"name\": \"T\",\n",
                "  \"plotFrames\": [\n",
                "    {\"name\": \"probe_frame\"},\n",
                "    {\"name\": \"probe_frame_2\"}\n",
                "  ]\n",
                "}\n"
            )
        );
        assert!(read_customisation(&written) == offering(&["probe_frame", "probe_frame_2"]));
    }

    // So that an older build still reads what this one writes, unless it
    // carries frames, which it could not read.
    #[test]
    fn a_customisation_that_offers_no_frame_is_still_written_as_version_1() {
        let written = write_customisation(&offering(&[]));
        assert_eq!(
            written,
            "{\n  \"format\": \"katana-customisation\",\n  \"version\": 1,\n  \"name\": \"T\"\n}\n"
        );
    }

    #[test]
    fn the_member_sits_between_automation_and_the_definitions() {
        let mut customisation = offering(&["probe_frame"]);
        customisation.automation = Some(katana_entity::CustomisationAutomation::default());
        let written = write_customisation(&customisation);
        let automation = written.find("\"automation\"").expect("automation");
        let frames = written.find("\"plotFrames\"").expect("plotFrames");
        assert!(automation < frames, "{written}");
        assert!(read_customisation(&written) == customisation);
    }

    #[test]
    fn a_version_1_file_holding_plot_frames_is_refused_as_an_older_build_refuses_it() {
        let error = refusal_of(
            r#"{"format": "katana-customisation", "version": 1, "name": "T", "plotFrames": [{"name": "probe_frame"}]}"#,
        );
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(error.message.contains("\"plotFrames\""), "{}", error.describe());
        assert!(error.message.contains("version 2"), "{}", error.describe());
        assert!(error.message.contains("version 1"), "{}", error.describe());
    }

    #[test]
    fn a_frame_entry_is_an_object_with_a_name_and_nothing_else() {
        let unknown = refusal_of(version_2_with(
            r#""plotFrames": [{"name": "probe_frame", "file": "probe_frame.json"}]"#,
        ));
        assert_eq!(unknown.code, ErrorCode::ParseFailure);
        expect_names(&unknown, "plotFrames[0] \"probe_frame\"", "file");

        let nameless = refusal_of(version_2_with(r#""plotFrames": [{}]"#));
        assert_eq!(nameless.code, ErrorCode::ParseFailure);
        expect_names(&nameless, "plotFrames[0]", "name");

        let not_a_list = refusal_of(version_2_with(r#""plotFrames": {"name": "probe_frame"}"#));
        assert_eq!(not_a_list.code, ErrorCode::ParseFailure);
        assert!(not_a_list.message.contains("must be a list"), "{}", not_a_list.describe());

        let not_an_object = refusal_of(version_2_with(r#""plotFrames": ["probe_frame"]"#));
        assert_eq!(not_an_object.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn a_frame_name_is_a_lower_case_word_of_at_most_64_bytes() {
        let longest = "a".repeat(64);
        for good in ["a", "a3_landscape", "probe_frame_2", longest.as_str()] {
            assert!(validate_plot_frame_name(good).is_ok(), "{good}");
        }
        let too_long = "a".repeat(65);
        for bad in [
            "",
            "A3_landscape",
            "3a_frame",
            "_frame",
            "probe frame",
            "probe-frame",
            "probe/frame",
            "probe.json",
            "fr\u{e4}me",
            too_long.as_str(),
        ] {
            let error = validate_plot_frame_name(bad).expect_err(bad);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{bad}");
        }
    }

    #[test]
    fn a_bad_frame_name_is_refused_by_the_reader_and_by_the_writer_naming_the_entry() {
        let read = refusal_of(version_2_with(r#""plotFrames": [{"name": "Probe Frame"}]"#));
        assert_eq!(read.code, ErrorCode::InvalidArgument);
        expect_names(&read, "plotFrames[0] \"Probe Frame\"", "name");

        let written = customisation_to_json(
            &offering(&["Probe Frame"]),
            &CustomisationWriteOptions::default(),
        )
        .expect_err("a name the reader refuses is not written");
        assert_eq!(written.code, ErrorCode::InvalidArgument);
        expect_names(&written, "plotFrames[0] \"Probe Frame\"", "name");
    }

    // A sheet names its frame by the name alone.
    #[test]
    fn a_frame_offered_twice_is_refused_naming_the_other() {
        let read = refusal_of(version_2_with(
            r#""plotFrames": [{"name": "probe_frame"}, {"name": "other"}, {"name": "probe_frame"}]"#,
        ));
        assert_eq!(read.code, ErrorCode::InvalidArgument);
        assert!(
            read.message.starts_with("plotFrames[2] \"probe_frame\": "),
            "{}",
            read.describe()
        );
        assert!(read.message.contains("plotFrames[0]"), "{}", read.describe());
    }

    #[test]
    fn an_empty_list_reads_as_no_frames_and_is_written_as_none() {
        let read = read_customisation(version_2_with(r#""plotFrames": []"#));
        assert!(read.plot_frames.is_empty());
        // Nothing to say, so nothing of version 2 is written.
        assert!(write_customisation(&read).contains("\"version\": 1"));
    }

    #[test]
    fn two_customisations_differing_only_in_their_frames_are_not_equal() {
        assert!(offering(&["probe_frame"]) != offering(&[]));
        assert!(offering(&["probe_frame", "other"]) != offering(&["other", "probe_frame"]));
        assert!(offering(&["probe_frame"]) == offering(&["probe_frame"]));
    }
}
