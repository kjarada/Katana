//! Twin of tests/surveyio/test_detect.cpp.
//!
//! The framework, not any one format. No parser is registered from this file
//! into the process-wide registry: every test that needs formats builds its
//! own FormatRegistry, so the suite says the same thing whichever parsers happen
//! to be in the program's list beside it.

mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};

use katana_core::ErrorCode;
use katana_surveyio::{
    DetectionOutcome, FormatContent, FormatDescriptor, FormatProbe, FormatRegistration,
    FormatRegistry, FormatSignature, Manufacturer, ProbeInput, describe_format, detect_file_with,
    detect_format, detect_format_with, format_registry, looks_like_text, probe_lines, probe_of,
    ruled_out, to_string, without_byte_order_mark,
};

use support::Scratch;

fn descriptor_for(id: &str) -> FormatDescriptor {
    FormatDescriptor {
        id: String::from(id),
        human_name: format!("Test format {id}"),
        manufacturer: Manufacturer::Generic,
        reads: FormatContent {
            points: true,
            ..Default::default()
        },
        can_import: true,
        parser_version: String::from("1.0"),
        ..Default::default()
    }
}

fn never_this_format(_: &ProbeInput<'_>) -> FormatSignature {
    ruled_out()
}

fn signature(confidence: f64, evidence: &str) -> FormatSignature {
    FormatSignature {
        confidence,
        evidence: String::from(evidence),
    }
}

fn very_confident(_: &ProbeInput<'_>) -> FormatSignature {
    signature(0.95, "a signature nothing else has")
}

fn equally_confident(_: &ProbeInput<'_>) -> FormatSignature {
    signature(0.95, "the very same signature")
}

fn nearly_as_confident(_: &ProbeInput<'_>) -> FormatSignature {
    signature(0.85, "most of that signature")
}

fn clearly_behind(_: &ProbeInput<'_>) -> FormatSignature {
    signature(0.75, "a weaker signature")
}

fn half_confident(_: &ProbeInput<'_>) -> FormatSignature {
    signature(0.5, "the extension matches and nothing else does")
}

fn slightly_less_than_half(_: &ProbeInput<'_>) -> FormatSignature {
    signature(0.4, "a record length that could be a coincidence")
}

fn impossibly_confident(_: &ProbeInput<'_>) -> FormatSignature {
    signature(1.5, "a bug in this probe")
}

fn confidence_is_not_a_number(_: &ProbeInput<'_>) -> FormatSignature {
    signature(f64::NAN, "another bug in this probe")
}

// Uses the shared helpers, so that what a real parser would do is exercised too.
fn magic_probe(input: &ProbeInput<'_>) -> FormatSignature {
    if input.extension != "magic" {
        return ruled_out();
    }
    let lines = probe_lines(input, 1);
    if lines.first().is_none_or(|&line| line != b"MAGIC") {
        return ruled_out();
    }
    signature(0.9, "extension .magic and a MAGIC first line")
}

// ---- descriptors ---------------------------------------------------------------

mod survey_format_descriptor {
    use super::*;

    #[test]
    fn a_format_describes_what_it_can_actually_do_rather_than_its_vendor() {
        let mut format = descriptor_for("leica-gsi16");
        format.human_name = String::from("Leica GSI-16");
        format.manufacturer = Manufacturer::Leica;
        format.parser_version = String::from("1.0");

        // descriptor_for leaves a format that imports and does not export,
        // which is what a new parser is: the descriptor says so rather than
        // implying both.
        assert_eq!(
            describe_format(&format),
            "Leica GSI-16, import: yes, export: no, parser: 1.0"
        );
        format.can_import = false;
        format.can_export = true;
        assert_eq!(
            describe_format(&format),
            "Leica GSI-16, import: no, export: yes, parser: 1.0"
        );
        assert_eq!(to_string(Manufacturer::Leica), "Leica");
        assert_eq!(to_string(Manufacturer::OpenStandard), "Open standard");
    }
}

// ---- registration --------------------------------------------------------------

mod survey_format_registry {
    use super::*;

    #[test]
    fn a_descriptor_the_user_interface_could_not_show_is_rejected() {
        let mut registry = FormatRegistry::default();
        let never = Some(never_this_format as FormatProbe);

        let code = |registry: &mut FormatRegistry, descriptor: FormatDescriptor| {
            registry
                .add(descriptor, never, None)
                .expect_err("the descriptor is refused")
                .code
        };
        assert_eq!(
            code(&mut registry, descriptor_for("")),
            ErrorCode::InvalidArgument
        );
        // The id reaches saved settings and every SourceRecord, so it is a slug.
        assert_eq!(
            code(&mut registry, descriptor_for("Leica GSI")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&mut registry, descriptor_for("Leica-Gsi16")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&mut registry, descriptor_for("16-gsi")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(&mut registry, descriptor_for("gsi_16")),
            ErrorCode::InvalidArgument
        );

        assert_eq!(
            registry
                .add(descriptor_for("good-id"), None, None)
                .expect_err("a format with no probe is refused")
                .code,
            ErrorCode::InvalidArgument
        );

        let mut unnamed = descriptor_for("good-id");
        unnamed.human_name.clear();
        assert_eq!(code(&mut registry, unnamed), ErrorCode::InvalidArgument);

        let mut unversioned = descriptor_for("good-id");
        unversioned.parser_version.clear();
        assert_eq!(code(&mut registry, unversioned), ErrorCode::InvalidArgument);

        let mut useless = descriptor_for("good-id");
        useless.can_import = false;
        useless.can_export = false;
        assert_eq!(code(&mut registry, useless), ErrorCode::InvalidArgument);

        let mut dotted = descriptor_for("good-id");
        dotted.extensions = vec![String::from(".gsi")];
        assert_eq!(
            code(&mut registry, dotted.clone()),
            ErrorCode::InvalidArgument
        );
        dotted.extensions = vec![String::from("GSI")];
        assert_eq!(code(&mut registry, dotted), ErrorCode::InvalidArgument);

        assert!(registry.empty()); // nothing above was half-added
    }

    #[test]
    fn two_formats_cannot_answer_to_one_id() {
        let mut registry = FormatRegistry::default();
        assert!(
            registry
                .add(descriptor_for("leica-gsi16"), Some(very_confident), None)
                .is_ok()
        );

        let second = registry.add(descriptor_for("leica-gsi16"), Some(half_confident), None);
        let error = second.expect_err("the second format with the id is refused");
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        assert_eq!(registry.size(), 1);
        assert!(registry.find("leica-gsi16").is_ok());
        assert_eq!(
            registry
                .find("no-such-format")
                .expect_err("there is no such format")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn formats_come_back_in_id_order_whatever_order_they_were_registered_in() {
        let mut registry = FormatRegistry::default();
        for id in ["zulu", "alpha", "mike"] {
            assert!(
                registry
                    .add(descriptor_for(id), Some(never_this_format), None)
                    .is_ok()
            );
        }

        let formats = registry.formats();
        assert_eq!(formats.len(), 3);
        assert_eq!(formats[0].id, "alpha");
        assert_eq!(formats[1].id, "mike");
        assert_eq!(formats[2].id, "zulu");
    }

    #[test]
    fn a_rejected_registration_is_loud_rather_than_a_missing_format() {
        // The program's registry is built where there is no caller to hand a
        // Status to. A rejected registration must not leave the program running
        // with one format quietly absent: it panics, as the C++ throws
        // std::logic_error, with the reason.
        let bad = descriptor_for("Not A Slug");
        let mut registry = FormatRegistry::default();
        let refused = catch_unwind(AssertUnwindSafe(|| {
            FormatRegistration::new(bad, never_this_format, None).register(&mut registry);
        }));
        let payload = refused.expect_err("the registration panics");
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(
            message.starts_with("surveyio: format 'Not A Slug' could not be registered"),
            "{message}"
        );
        assert!(registry.empty());
        assert!(!format_registry().contains("Not A Slug"));
    }

    #[test]
    fn every_format_this_program_registered_satisfies_the_contract() {
        // Whatever parsers are in the program's list, each must have a
        // descriptor the registry would accept and a distinct id. Re-adding
        // them to a fresh registry runs exactly those checks, so a parser that
        // registers a bad descriptor fails here rather than at some customer's
        // import.
        let mut fresh = FormatRegistry::default();
        for format in format_registry().formats() {
            let status = fresh.add(format.clone(), Some(never_this_format), None);
            assert!(
                status.is_ok(),
                "{}: {}",
                describe_format(&format),
                status
                    .err()
                    .map(|error| error.describe())
                    .unwrap_or_default()
            );
            assert!(!describe_format(&format).is_empty());
        }
        assert_eq!(fresh.size(), format_registry().size());
    }
}

// ---- detection -----------------------------------------------------------------

mod survey_format_detection {
    use super::*;

    fn registry_of(formats: &[(&str, FormatProbe)]) -> FormatRegistry {
        let mut registry = FormatRegistry::default();
        for &(id, probe) in formats {
            assert!(registry.add(descriptor_for(id), Some(probe), None).is_ok());
        }
        registry
    }

    #[test]
    fn an_empty_file_is_reported_empty_rather_than_unrecognised() {
        let registry = registry_of(&[("always", very_confident)]);

        let detection = detect_format_with(&probe_of(b"", "job.gsi", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Empty);
        assert!(detection.candidates().is_empty());
        assert!(detection.format().is_err()); // and no format can be had from it
    }

    #[test]
    fn with_nothing_registered_no_file_is_identified() {
        let registry = FormatRegistry::default();
        let detection = detect_format_with(&probe_of(b"some bytes", "job.gsi", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Uncertain);
        assert!(detection.format().is_err());
        assert!(!detection.summary().is_empty());
    }

    #[test]
    fn a_confident_sole_candidate_is_identified_and_says_why() {
        let registry = registry_of(&[
            ("leica-gsi16", very_confident),
            ("trimble-dc", never_this_format),
        ]);

        let detection = detect_format_with(&probe_of(b"some bytes", "job.gsi", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Identified);

        let format = detection.format().expect("the format is identified");
        assert_eq!(format.id, "leica-gsi16");

        // A format that ruled itself out is not a candidate at all.
        assert_eq!(detection.candidates().len(), 1);
        assert_eq!(
            detection.candidates()[0].evidence,
            "a signature nothing else has"
        );
        assert!(detection.summary().contains("a signature nothing else has"));
    }

    #[test]
    fn two_formats_that_both_fit_are_ambiguous_rather_than_a_guess() {
        let registry = registry_of(&[("alpha", very_confident), ("bravo", nearly_as_confident)]);

        // 0.95 and 0.85: both past the bar, 0.10 apart, which is inside the
        // margin.
        let detection = detect_format_with(&probe_of(b"some bytes", "job.dat", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Ambiguous);
        assert_eq!(detection.candidates().len(), 2);

        let error = detection.format().expect_err("no format is identified");
        assert_eq!(error.code, ErrorCode::NotFound);
        // The error carries what was close, so a caller that only propagates it
        // still reports something a person can act on.
        assert!(error.context.contains("alpha"));
        assert!(error.context.contains("bravo"));
    }

    #[test]
    fn a_winner_clear_of_the_runner_up_by_more_than_the_margin_is_identified() {
        let registry = registry_of(&[("alpha", very_confident), ("bravo", clearly_behind)]);

        // 0.95 - 0.75 = 0.20, past IDENTIFICATION_MARGIN of 0.15.
        let detection = detect_format_with(&probe_of(b"some bytes", "job.dat", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Identified);
        assert_eq!(
            detection.format().expect("the format is identified").id,
            "alpha"
        );
        assert_eq!(detection.candidates().len(), 2); // the runner-up is still reported
    }

    #[test]
    fn a_candidate_below_the_bar_is_listed_as_the_closest_but_not_chosen() {
        let registry = registry_of(&[
            ("alpha", half_confident),
            ("bravo", slightly_less_than_half),
        ]);

        let detection = detect_format_with(&probe_of(b"some bytes", "job.dat", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Uncertain);
        assert_eq!(detection.candidates().len(), 2);
        assert_eq!(detection.candidates()[0].format_id, "alpha");
        assert_eq!(detection.candidates()[1].format_id, "bravo");
        assert!(detection.format().is_err());
        assert!(detection.summary().contains("alpha"));
    }

    #[test]
    fn equal_confidences_are_ranked_by_id_so_the_answer_never_depends_on_link_order() {
        let registered = registry_of(&[("zulu", very_confident), ("alpha", equally_confident)]);
        let reversed = registry_of(&[("alpha", equally_confident), ("zulu", very_confident)]);

        let first = detect_format_with(&probe_of(b"some bytes", "job.dat", false), &registered);
        let second = detect_format_with(&probe_of(b"some bytes", "job.dat", false), &reversed);
        assert_eq!(first.candidates().len(), 2);
        assert_eq!(first.candidates()[0].format_id, "alpha");
        assert_eq!(first.candidates(), second.candidates());
    }

    #[test]
    fn a_probe_reporting_an_impossible_confidence_is_ignored_and_said_so() {
        let registry = registry_of(&[
            ("broken", impossibly_confident),
            ("nonsense", confidence_is_not_a_number),
            ("sound", very_confident),
        ]);

        let detection = detect_format_with(&probe_of(b"some bytes", "job.dat", false), &registry);
        // Not clamped into range: a clamp would turn a broken probe into a
        // confident one and run the wrong parser.
        assert_eq!(detection.candidates().len(), 1);
        assert_eq!(detection.candidates()[0].format_id, "sound");
        assert_eq!(detection.outcome(), DetectionOutcome::Identified);
        assert!(detection.summary().contains("broken"));
        assert!(detection.summary().contains("nonsense"));
    }

    #[test]
    fn binary_garbage_that_no_probe_claims_is_uncertain_rather_than_the_first_format() {
        let registry = registry_of(&[("magic", magic_probe)]);

        let garbage: [u8; 8] = [0x00, 0x01, 0xFF, 0xFE, 0x7F, 0x00, 0x13, 0x00];
        let detection = detect_format_with(&probe_of(&garbage, "job.magic", false), &registry);
        assert_eq!(detection.outcome(), DetectionOutcome::Uncertain);
        assert!(detection.candidates().is_empty());
        assert!(detection.format().is_err());
    }

    #[test]
    fn the_default_overload_answers_from_the_process_wide_registry() {
        // Which parsers are in the program's list is not this test's business;
        // the invariant is. A format comes back exactly when one was
        // identified.
        let bytes = b"110001+00000001 81..00+00012345\r\n";
        let detection = detect_format(&probe_of(bytes, "job.gsi", false));
        assert_eq!(
            detection.format().is_ok(),
            detection.outcome() == DetectionOutcome::Identified
        );
        assert!(!detection.summary().is_empty());
    }
}

// ---- detecting a file on disk ---------------------------------------------------

mod survey_format_detection_of_files {
    use super::*;

    #[test]
    fn a_file_that_is_not_there_fails_rather_than_reporting_uncertain() {
        let registry = FormatRegistry::default();
        let scratch = Scratch::new("katana_surveyio_missing_file");
        let missing = scratch.root.join("katana_no_such_survey_file.gsi");

        let error = detect_file_with(&missing, &registry).expect_err("there is no such file");
        assert_eq!(error.code, ErrorCode::FileImportFailure);
    }

    #[test]
    fn an_empty_file_is_not_a_failure_it_is_an_empty_detection() {
        let registry = FormatRegistry::default();
        let scratch = Scratch::new("katana_surveyio_empty_file");
        let path = scratch.write("katana_empty_survey_file.gsi", b"");

        let detection = detect_file_with(&path, &registry).expect("an empty file is detected");
        assert_eq!(detection.outcome(), DetectionOutcome::Empty);
    }

    #[test]
    fn a_file_is_probed_with_its_own_name_and_its_own_bytes() {
        let mut registry = FormatRegistry::default();
        assert!(
            registry
                .add(descriptor_for("magic"), Some(magic_probe), None)
                .is_ok()
        );
        let scratch = Scratch::new("katana_surveyio_probed_file");
        let path = scratch.write("katana_survey_probe.magic", b"MAGIC\r\n1 2 3\r\n");

        let detection = detect_file_with(&path, &registry).expect("the file is read");
        assert_eq!(detection.outcome(), DetectionOutcome::Identified);
        assert_eq!(
            detection.format().expect("the format is identified").id,
            "magic"
        );

        // The same bytes under a name the probe does not accept are not that
        // format.
        let wrong_name = scratch.write("katana_survey_probe.txt", b"MAGIC\r\n1 2 3\r\n");
        let by_name = detect_file_with(&wrong_name, &registry).expect("the file is read");
        assert_eq!(by_name.outcome(), DetectionOutcome::Uncertain);
    }
}

// ---- helpers the probes share ---------------------------------------------------

mod survey_probe_input {
    use super::*;

    #[test]
    fn only_the_name_of_whatever_the_caller_supplied_reaches_a_probe() {
        let bytes = b"x";
        // A probe is never handed something it could open, whatever the caller
        // passes.
        assert_eq!(
            probe_of(bytes, "..\\..\\secret\\job.GSI", false).file_name,
            "job.GSI"
        );
        assert_eq!(probe_of(bytes, "/etc/passwd", false).file_name, "passwd");

        // The extension is lower-cased once, here, so no two probes disagree on
        // it.
        assert_eq!(
            probe_of(bytes, "..\\..\\secret\\job.GSI", false).extension,
            "gsi"
        );
        assert_eq!(probe_of(bytes, "/etc/passwd", false).extension, "");
        assert_eq!(probe_of(bytes, "job.", false).extension, "");
        // A leading dot makes a hidden file, not an extension.
        assert_eq!(probe_of(bytes, ".gsi", false).extension, "");
        assert_eq!(probe_of(bytes, "archive.tar.GZ", false).extension, "gz");
    }
}

mod survey_probe_helpers {
    use super::*;

    #[test]
    fn a_byte_order_mark_is_stepped_over_whichever_one_it_is() {
        assert_eq!(without_byte_order_mark(b"\xEF\xBB\xBFx"), b"x");
        assert_eq!(without_byte_order_mark(b"\xFF\xFEx"), b"x");
        assert_eq!(without_byte_order_mark(b"\xFE\xFFx"), b"x");
        // UTF-32LE begins with the UTF-16LE mark, so the longer mark is tested
        // first; otherwise two NUL bytes would be left at the front of every
        // UTF-32 file.
        assert_eq!(without_byte_order_mark(b"\xFF\xFE\x00\x00x"), b"x");
        assert_eq!(without_byte_order_mark(b"\x00\x00\xFE\xFFx"), b"x");

        assert_eq!(without_byte_order_mark(b"no mark here"), b"no mark here");
        assert_eq!(without_byte_order_mark(b""), b"");
        // Two bytes that merely start like a mark are not one.
        assert_eq!(without_byte_order_mark(b"\xEFx").len(), 2);
    }

    #[test]
    fn every_kind_of_line_ending_ends_a_line() {
        let mixed = b"alpha\r\nbravo\ncharlie\rdelta";
        let lines = probe_lines(&probe_of(mixed, "job.txt", false), 10);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], b"alpha");
        assert_eq!(lines[1], b"bravo");
        assert_eq!(lines[2], b"charlie");
        assert_eq!(lines[3], b"delta");

        assert_eq!(probe_lines(&probe_of(mixed, "job.txt", false), 2).len(), 2);
        assert!(probe_lines(&probe_of(mixed, "job.txt", false), 0).is_empty());
        assert!(probe_lines(&probe_of(b"", "job.txt", false), 5).is_empty());

        // A blank line is a line: a probe counting records must see it.
        let blank_text = b"alpha\n\nbravo\n";
        let blank = probe_lines(&probe_of(blank_text, "job.txt", false), 10);
        assert_eq!(blank.len(), 3);
        assert_eq!(blank[1], b"");

        // The mark belongs to the file, not to the first line.
        let with_mark = b"\xEF\xBB\xBFheader\nbody\n";
        assert_eq!(
            probe_lines(&probe_of(with_mark, "job.txt", false), 1)[0],
            b"header"
        );
    }

    #[test]
    fn a_truncated_probe_drops_its_last_partial_line() {
        // Whether the last line is a record or the place PROBE_BYTES fell is
        // not decidable from the bytes, so a probe must not be shown it.
        let bytes = b"first\nsecond\nthir";
        assert_eq!(probe_lines(&probe_of(bytes, "job.txt", true), 10).len(), 2);
        assert_eq!(probe_lines(&probe_of(bytes, "job.txt", false), 10).len(), 3);

        // A line that DID end is kept either way.
        let terminated = b"first\nsecond\n";
        assert_eq!(
            probe_lines(&probe_of(terminated, "job.txt", true), 10).len(),
            2
        );
    }

    #[test]
    fn text_is_told_from_binary_by_its_control_bytes() {
        assert!(looks_like_text(b"110001+00000001 81..00+00012345\r\n"));
        assert!(looks_like_text(b"one\ttwo\r\n"));

        // Nothing to judge is not evidence of text.
        assert!(!looks_like_text(b""));
        // A NUL settles it: no format read here is UTF-16.
        assert!(!looks_like_text(b"ab\x00c"));
        assert!(!looks_like_text(b"\x01\x02\x03\x04"));

        // One control byte in two hundred is ordinary in instrument output.
        let mut mostly_text = [b'a'; 200];
        mostly_text[100] = 0x0C; // form feed
        assert!(looks_like_text(&mostly_text));
        // Three in two hundred is past the one-in-a-hundred policy.
        mostly_text[101] = 0x0C;
        mostly_text[102] = 0x0C;
        assert!(!looks_like_text(&mostly_text));
    }
}
