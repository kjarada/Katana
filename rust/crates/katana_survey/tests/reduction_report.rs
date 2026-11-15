//! Twin of tests/survey/test_reduction_report.cpp: render_text and
//! render_html: every section, readable, and safe with hostile ids.

mod reduction_support;

use katana_survey::data_model::{Face, SurveyProject};
use katana_survey::reduction::{ReductionContext, ReductionOutcome, reduce_and_adjust};
use katana_survey::reduction_report::{
    AppliedCorrection, CorrectionKind, ReductionReport, ReportObservation, render_html, render_text,
};
use katana_survey::reduction_settings::ReductionSettings;
use reduction_support::{arc_seconds, deg, dms, point, setup, shot, succeeded, unpositioned};

fn reduced(target_id: &str) -> ReductionOutcome {
    let mut project = SurveyProject::default();
    project.points.push(point("A", 1000.0, 1000.0, Some(50.0)));
    project.points.push(point("B", 1100.0, 1000.0, Some(50.0)));
    project.unpositioned_points.push(unpositioned(target_id));
    project.stations.push(setup(
        "S1",
        "A",
        1.5,
        "B",
        &[
            shot("B", 1, Face::Left, Some(0.0), None, None, 0.0),
            shot(
                target_id,
                2,
                Face::Left,
                Some(deg(90.0)),
                Some(deg(90.0)),
                Some(50.0),
                1.5,
            ),
            shot(
                target_id,
                3,
                Face::Right,
                Some(dms(270.0, 0.0, 2.0)),
                Some(deg(90.0)),
                Some(50.001),
                1.5,
            ),
        ],
    ));
    let mut context = ReductionContext {
        created_utc: String::from("2026-09-24T10:00:00Z"),
        ..Default::default()
    };
    context.input.file_name = String::from("job <1>.gsi");
    context.input.format_name = String::from("Leica GSI-16");
    context.input.records_read = 9;
    context
        .input
        .not_carried
        .push(String::from("no atmospheric state"));
    succeeded(reduce_and_adjust(
        &project,
        &ReductionSettings::default(),
        &context,
    ))
}

mod reduction_report_text {
    use super::*;

    #[test]
    fn every_section_is_present_in_the_models_order() {
        let text = render_text(&reduced("P1").report);
        let mut at = 0;
        for title in [
            "Input",
            "Settings used",
            "Warnings",
            "Setups",
            "Face left / face right pairs",
            "Observations: raw value, each correction, reduced value",
            "Misclosures and checks",
            "Coordinates",
        ] {
            let found = text[at..]
                .find(&format!("{title}\n"))
                .map(|offset| at + offset);
            let Some(found) = found else {
                panic!("{title}");
            };
            at = found;
        }
        // A surveyor's units: the face mean of the FR reading shows in seconds,
        // the distance spread in millimetres, the azimuth in DMS.
        assert!(text.contains("90\u{00B0}00'01.0\""));
        assert!(text.contains("+1.0 mm"));
        assert!(text.contains("no atmospheric state"));
    }

    #[test]
    fn columns_line_up_by_displayed_characters_not_bytes() {
        // In the setups table the cell after the three angles (each with a
        // two-byte degree sign) starts under its heading: padding counts
        // displayed characters.
        let text = render_text(&reduced("P1").report);
        let displayed = |s: &str, bytes: usize| {
            s.as_bytes()[..bytes]
                .iter()
                .filter(|byte| (*byte & 0xC0) != 0x80)
                .count()
        };
        let mut header = "";
        let mut row = "";
        for line in text.lines() {
            if line.contains("BS dist check") {
                header = line;
            } else if !header.is_empty() && line.starts_with("  S1 ") {
                row = line;
                break;
            }
        }
        assert!(!header.is_empty());
        assert!(!row.is_empty());
        let last_angle = row.rfind('"').expect("an angle");
        let next = last_angle
            + 1
            + row[last_angle + 1..]
                .find(|character: char| character != ' ')
                .expect("a cell after the angles");
        assert_eq!(
            displayed(row, next),
            displayed(header, header.find("BS dist check").expect("the heading"))
        );
    }

    #[test]
    fn large_corrections_read_in_degrees_and_metres_small_ones_in_seconds_and_millimetres() {
        // An orientation of -100 00 01 is -360 001", which nobody reads; a slope
        // reduction of -12.3456 m is -12 345.6 mm. A face mean of 1.5" and an
        // atmospheric 1.6 mm stay in seconds and millimetres.
        let direction = ReportObservation {
            kind: String::from("horizontal direction"),
            angular: true,
            raw: deg(55.0),
            corrections: vec![
                AppliedCorrection {
                    kind: CorrectionKind::FaceMean,
                    amount: arc_seconds(1.5),
                    factor: None,
                    note: String::new(),
                },
                AppliedCorrection {
                    kind: CorrectionKind::Orientation,
                    amount: -dms(100.0, 0.0, 1.0),
                    factor: None,
                    note: String::new(),
                },
            ],
            reduced: Some(dms(315.0, 0.0, 0.5)),
            ..Default::default()
        };
        let distance = ReportObservation {
            kind: String::from("slope distance"),
            raw: 100.0,
            corrections: vec![
                AppliedCorrection {
                    kind: CorrectionKind::Atmospheric,
                    amount: 0.0016,
                    factor: Some(1.000016),
                    note: String::new(),
                },
                AppliedCorrection {
                    kind: CorrectionKind::SlopeToHorizontal,
                    amount: -12.3456,
                    factor: None,
                    note: String::new(),
                },
            ],
            reduced: Some(87.656),
            ..Default::default()
        };
        let report = ReductionReport {
            observations: vec![direction, distance],
            ..Default::default()
        };
        let text = render_text(&report);
        assert!(text.contains("face left / face right mean +1.5\""));
        assert!(text.contains("orientation -100\u{00B0}00'01.0\""));
        assert!(text.contains("atmospheric +1.6 mm"));
        assert!(text.contains("slope to horizontal -12.3456 m"));
    }
}

mod reduction_report_html {
    use super::*;

    #[test]
    fn is_one_self_contained_document_with_no_script_and_escaped_ids() {
        let html = render_html(&reduced("<script>alert(1)</script>").report);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("</html>"));
        assert!(!html.contains("<script"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("job &lt;1&gt;.gsi"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(html.contains("<h2>Coordinates</h2>"));
    }

    #[test]
    fn a_rejected_observation_is_marked_as_an_alarm() {
        let mut project = SurveyProject::default();
        project.points.push(point("A", 0.0, 0.0, None));
        project.points.push(point("B", 100.0, 0.0, None));
        project.unpositioned_points.push(unpositioned("T"));
        project.stations.push(setup(
            "S",
            "A",
            1.5,
            "B",
            &[
                shot("B", 1, Face::Left, Some(0.0), None, None, 0.0),
                shot(
                    "T",
                    2,
                    Face::Left,
                    Some(deg(45.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    0.0,
                ),
                shot(
                    "T",
                    3,
                    Face::Right,
                    Some(dms(225.0, 0.0, 30.0)),
                    Some(deg(90.0)),
                    Some(100.0),
                    0.0,
                ),
            ],
        ));
        let mut settings = ReductionSettings::default();
        settings.face_tolerances.exclude_outside = true;
        let outcome = succeeded(reduce_and_adjust(
            &project,
            &settings,
            &ReductionContext::default(),
        ));
        let html = render_html(&outcome.report);
        // A long cell, so it also wraps (class note).
        assert!(html.contains("<td class=\"alarm note\">REJECTED: face pair outside tolerance"));
        assert!(render_text(&outcome.report).contains("OUTSIDE TOLERANCE"));
    }
}
