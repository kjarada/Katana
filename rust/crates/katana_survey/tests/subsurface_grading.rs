//! The twin of tests/survey/test_subsurface_grading.cpp: quality levels,
//! lines and segments, clearance and verification.

mod subsurface_support;

use katana_core::ErrorCode;
use katana_survey::coordinate::Coordinate2;
use katana_survey::subsurface::*;
use subsurface_support::{double_eq, near};

fn evidence(
    method: LocationMethod,
    horizontal: Option<f64>,
    vertical: Option<f64>,
    has_level: bool,
) -> PositionEvidence {
    PositionEvidence {
        method,
        horizontal_uncertainty: horizontal,
        vertical_uncertainty: vertical,
        has_level,
    }
}

fn vertex(
    id: &str,
    northing: f64,
    easting: f64,
    located: PositionEvidence,
    level: Option<f64>,
) -> UtilityVertex {
    UtilityVertex {
        id: String::from(id),
        position: Coordinate2 { northing, easting },
        evidence: located,
        level,
        ..Default::default()
    }
}

// A detection good to 100 mm in plan, and a pothole good to 20 mm in 3D.
const EML: PositionEvidence = PositionEvidence {
    method: LocationMethod::ElectromagneticLocation,
    horizontal_uncertainty: Some(0.10),
    vertical_uncertainty: None,
    has_level: false,
};
const POTHOLE: PositionEvidence = PositionEvidence {
    method: LocationMethod::NonDestructiveExcavation,
    horizontal_uncertainty: Some(0.02),
    vertical_uncertainty: Some(0.02),
    has_level: true,
};

/// `classify` with the default tolerances.
fn grade(located: &PositionEvidence) -> Classification {
    classify(located, &QualityLevelTolerances::default())
}

// ---- quality levels ------------------------------------------------------------

mod subsurface_quality_level {
    use super::*;

    #[test]
    fn parses_the_spellings_deliverables_use() {
        assert_eq!(parse_quality_level("QL-A"), Some(QualityLevel::A));
        assert_eq!(parse_quality_level(" qlb "), Some(QualityLevel::B));
        assert_eq!(parse_quality_level("QL C"), Some(QualityLevel::C));
        assert_eq!(parse_quality_level("d"), Some(QualityLevel::D));
        assert_eq!(parse_quality_level("QL_D"), Some(QualityLevel::D));
        assert!(parse_quality_level("E").is_none());
        assert!(parse_quality_level("").is_none());
        assert!(parse_quality_level("QL-AB").is_none());
        assert_eq!(to_string(QualityLevel::B), "QL-B");
    }

    #[test]
    fn a_better_level_compares_greater() {
        assert!(QualityLevel::A > QualityLevel::B);
        assert!(QualityLevel::B > QualityLevel::C);
        assert!(QualityLevel::C > QualityLevel::D);
        assert_eq!(
            std::cmp::min(QualityLevel::A, QualityLevel::C),
            QualityLevel::C
        );
    }

    #[test]
    fn parses_location_methods_by_name_and_abbreviation() {
        assert_eq!(
            parse_location_method("EML"),
            Some(LocationMethod::ElectromagneticLocation)
        );
        assert_eq!(
            parse_location_method("gpr"),
            Some(LocationMethod::GroundPenetratingRadar)
        );
        assert_eq!(
            parse_location_method("Non-Destructive Excavation"),
            Some(LocationMethod::NonDestructiveExcavation)
        );
        assert_eq!(
            parse_location_method("pothole"),
            Some(LocationMethod::NonDestructiveExcavation)
        );
        assert_eq!(
            parse_location_method("Surface Feature"),
            Some(LocationMethod::SurfaceFeature)
        );
        assert!(parse_location_method("dowsing").is_none());
    }

    #[test]
    fn the_method_caps_the_level() {
        // No uncertainty, however small, lifts records or a radar pick.
        assert_eq!(
            grade(&evidence(LocationMethod::Records, Some(0.001), None, false)).level,
            QualityLevel::D
        );
        assert_eq!(
            grade(&evidence(
                LocationMethod::SurfaceFeature,
                Some(0.001),
                None,
                false
            ))
            .level,
            QualityLevel::C
        );
        assert_eq!(
            grade(&evidence(
                LocationMethod::GroundPenetratingRadar,
                Some(0.001),
                Some(0.001),
                true
            ))
            .level,
            QualityLevel::B
        );
    }

    #[test]
    fn a_detection_within_three_hundred_millimetres_is_ql_b() {
        let plan = grade(&EML);
        assert_eq!(plan.level, QualityLevel::B);
        assert!(!plan.level_qualified); // no level recorded
        assert!(plan.reasons.is_empty());

        let with_depth = grade(&evidence(
            LocationMethod::ElectromagneticLocation,
            Some(0.10),
            Some(0.40),
            true,
        ));
        assert_eq!(with_depth.level, QualityLevel::B);
        assert!(with_depth.level_qualified);
    }

    #[test]
    fn a_depth_outside_tolerance_keeps_the_plan_position_and_disqualifies_the_level() {
        let result = grade(&evidence(
            LocationMethod::ElectromagneticLocation,
            Some(0.10),
            Some(0.60),
            true,
        ));
        assert_eq!(result.level, QualityLevel::B);
        assert!(!result.level_qualified);
        assert_eq!(result.reasons.len(), 1);
        assert!(
            result.reasons[0].contains("600 mm"),
            "{}",
            result.reasons[0]
        );
    }

    #[test]
    fn a_detection_outside_or_without_an_uncertainty_falls_to_ql_c() {
        for horizontal in [Some(0.35), None, Some(-0.1), Some(f64::NAN)] {
            assert_eq!(
                grade(&evidence(
                    LocationMethod::ElectromagneticLocation,
                    horizontal,
                    None,
                    false
                ))
                .level,
                QualityLevel::C,
                "{horizontal:?}"
            );
        }
    }

    #[test]
    fn ql_a_needs_a_three_dimensional_position_within_fifty_millimetres() {
        let exposed = grade(&POTHOLE);
        assert_eq!(exposed.level, QualityLevel::A);
        assert!(exposed.level_qualified);
        assert!(exposed.reasons.is_empty());

        // Exposed, but no level: a plan position, so QL-B.
        let no_level = grade(&evidence(
            LocationMethod::NonDestructiveExcavation,
            Some(0.02),
            None,
            false,
        ));
        assert_eq!(no_level.level, QualityLevel::B);
        assert!(!no_level.reasons.is_empty());
        assert!(no_level.reasons[0].contains("needs a level"));

        assert_eq!(
            grade(&evidence(
                LocationMethod::OpenExcavation,
                Some(0.06),
                Some(0.02),
                true
            ))
            .level,
            QualityLevel::B
        );
        assert_eq!(
            grade(&evidence(
                LocationMethod::OpenExcavation,
                Some(0.02),
                Some(0.06),
                true
            ))
            .level,
            QualityLevel::B
        );
    }

    #[test]
    fn tolerances_are_the_callers_to_tighten() {
        let tight = QualityLevelTolerances {
            b: Tolerance {
                horizontal: Some(0.150),
                vertical: Some(0.300),
            },
            ..Default::default()
        };
        assert_eq!(classify(&EML, &tight).level, QualityLevel::B);
        assert_eq!(
            classify(
                &evidence(
                    LocationMethod::ElectromagneticLocation,
                    Some(0.20),
                    None,
                    false
                ),
                &tight
            )
            .level,
            QualityLevel::C
        );
    }
}

// ---- lines and segments --------------------------------------------------------

// Along the easting axis, northing 0:
//   P1 e=0  EML      P2 e=8  EML      P3 e=20 pothole, level      P4 e=25 pothole, level
// Segments 8 m, 12 m, 5 m.
fn sample_line() -> UtilityLine {
    UtilityLine {
        id: String::from("W1"),
        attributes: UtilityAttributes {
            r#type: UtilityType::Water,
            diameter: 0.2,
            ..Default::default()
        },
        vertices: vec![
            vertex("P1", 0.0, 0.0, EML, None),
            vertex("P2", 0.0, 8.0, EML, None),
            vertex("P3", 0.0, 20.0, POTHOLE, Some(9.0)),
            vertex("P4", 0.0, 25.0, POTHOLE, Some(9.1)),
        ],
        ..Default::default()
    }
}

mod subsurface_utility_line {
    use super::*;

    #[test]
    fn a_segment_is_no_better_than_its_weaker_end_or_its_path() {
        let graded = grade_line(&sample_line(), &GradingSettings::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(graded.segments.len(), 3);

        assert_eq!(graded.segments[0].level, QualityLevel::B);
        double_eq(graded.segments[0].length, 8.0);
        // 12 m is past the default 10 m detected spacing: interpolated, not traced.
        assert_eq!(graded.segments[1].level, QualityLevel::C);
        assert!(
            graded.segments[1]
                .limited_by
                .contains("maximum detected spacing")
        );
        // Between two potholes, but nobody saw the service between them.
        assert_eq!(graded.segments[2].level, QualityLevel::B);
        assert!(graded.segments[2].limited_by.contains("not exposed"));

        double_eq(graded.length_at[QualityLevel::A as usize], 0.0);
        double_eq(graded.length_at[QualityLevel::B as usize], 13.0);
        double_eq(graded.length_at[QualityLevel::C as usize], 12.0);
        double_eq(graded.length(), 25.0);
    }

    #[test]
    fn only_an_exposed_path_keeps_ql_a() {
        let mut line = sample_line();
        line.path_evidence = vec![
            PathEvidence::Detected,
            PathEvidence::Assumed,
            PathEvidence::Exposed,
        ];
        let graded = grade_line(&line, &GradingSettings::default()).unwrap();
        assert_eq!(graded.segments[1].level, QualityLevel::C);
        assert!(graded.segments[1].limited_by.contains("assumed"));
        assert_eq!(graded.segments[2].level, QualityLevel::A);
        double_eq(graded.length_at[QualityLevel::A as usize], 5.0);
    }

    #[test]
    fn the_detected_spacing_is_the_projects_to_set() {
        let settings = GradingSettings {
            maximum_detected_spacing: f64::INFINITY,
            ..Default::default()
        };
        let graded = grade_line(&sample_line(), &settings).unwrap();
        assert_eq!(graded.segments[1].level, QualityLevel::B);
    }

    #[test]
    fn a_claim_better_than_the_evidence_is_reported() {
        let mut line = sample_line();
        line.vertices[0].claimed = Some(QualityLevel::A);
        line.vertices[2].claimed = Some(QualityLevel::A);
        let graded = grade_line(&line, &GradingSettings::default()).unwrap();
        assert_eq!(
            graded.vertices[0].over_claim,
            "claimed QL-A, the electromagnetic location evidence supports QL-B"
        );
        assert!(graded.vertices[2].over_claim.is_empty());
    }

    #[test]
    fn refuses_a_line_it_cannot_grade() {
        let settings = GradingSettings::default();
        let mut line = sample_line();
        line.vertices.truncate(1);
        assert_eq!(
            grade_line(&line, &settings).unwrap_err().code,
            ErrorCode::InvalidArgument
        );

        line = sample_line();
        line.path_evidence = vec![PathEvidence::Detected];
        assert_eq!(
            grade_line(&line, &settings).unwrap_err().code,
            ErrorCode::InvalidArgument
        );

        line = sample_line();
        line.vertices[1].position.easting = f64::INFINITY;
        assert_eq!(
            grade_line(&line, &settings).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_top_of_the_service_from_any_reference() {
        let mut point = vertex("P", 0.0, 0.0, POTHOLE, Some(10.0));
        assert_eq!(top_level(&point, 0.3), Some(10.0));
        point.level_reference = LevelReference::Centre;
        double_eq(top_level(&point, 0.3).unwrap(), 10.15);
        assert!(top_level(&point, 0.0).is_none()); // a centre level needs the diameter
        point.level_reference = LevelReference::Invert;
        double_eq(top_level(&point, 0.3).unwrap(), 10.3);
        point.level = None;
        assert!(top_level(&point, 0.3).is_none());
    }

    #[test]
    fn depth_of_cover_is_surface_minus_top_of_service() {
        let mut line = sample_line();
        // P3: centre level 9.0 on a 200 mm service -> top 9.1; surface 10.0 -> 0.9 cover.
        line.vertices[2].level_reference = LevelReference::Centre;
        line.vertices[2].surface_level = Some(10.0);
        // P4: top 9.1, surface 9.6 -> 0.5 cover, below a 0.6 minimum.
        line.vertices[3].surface_level = Some(9.6);
        // P2: a depth estimate outside QL-B's vertical tolerance.
        line.vertices[1].level = Some(9.4);
        line.vertices[1].evidence.vertical_uncertainty = Some(0.6);
        line.vertices[1].surface_level = Some(10.2);

        let cover = depth_of_cover(&line, Some(0.6), &GradingSettings::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(cover.len(), 4);
        assert!(cover[0].cover.is_none());
        assert_eq!(cover[0].note, "no level or depth of the service");
        assert!(cover[1].cover.is_some());
        near(cover[1].cover.unwrap(), 0.8, 1e-12);
        assert!(cover[1].note.contains("do not rely"));
        near(cover[2].cover.unwrap(), 0.9, 1e-12);
        assert!(!cover[2].below_minimum);
        near(cover[3].cover.unwrap(), 0.5, 1e-12);
        assert!(cover[3].below_minimum);
    }
}

// ---- clearance -----------------------------------------------------------------

// A 200 mm service along the northing axis from (0, 0) to (8, 0).
fn straight_service(located: PositionEvidence) -> UtilityLine {
    UtilityLine {
        id: String::from("S1"),
        attributes: UtilityAttributes {
            diameter: 0.2,
            ..Default::default()
        },
        vertices: vec![
            vertex("A", 0.0, 0.0, located, None),
            vertex("B", 8.0, 0.0, located, None),
        ],
        ..Default::default()
    }
}

fn design_vertex(northing: f64, easting: f64, level: Option<f64>) -> DesignVertex {
    DesignVertex {
        position: Coordinate2 { northing, easting },
        level,
    }
}

// Works parallel to it, at easting `offset`, 200 mm wide.
fn parallel_at(offset: f64) -> DesignAlignment {
    DesignAlignment {
        id: String::from("D"),
        half_width: 0.1,
        vertices: vec![
            design_vertex(-5.0, offset, None),
            design_vertex(25.0, offset, None),
        ],
    }
}

/// `checkClearance` with the default requirement and settings.
fn clearance(
    design: &DesignAlignment,
    services: &[UtilityLine],
) -> katana_core::Result<Vec<ClearanceResult>> {
    check_clearance(
        design,
        services,
        &ClearanceRequirement::default(),
        &GradingSettings::default(),
    )
}

fn only(results: katana_core::Result<Vec<ClearanceResult>>) -> ClearanceResult {
    let mut results = results.unwrap_or_else(|error| panic!("{}", error.describe()));
    assert_eq!(results.len(), 1);
    results.remove(0)
}

mod subsurface_clearance {
    use super::*;

    #[test]
    fn a_ql_b_service_is_widened_by_its_tolerance() {
        // Face to face: offset - 0.1 - 0.1. Required 0.3, tolerance 0.3.
        let services = [straight_service(EML)];

        let far = only(clearance(&parallel_at(1.0), &services));
        near(far.horizontal_gap, 0.8, 1e-12);
        assert_eq!(far.status, ClearanceStatus::Clear); // 0.8 - 0.3 >= 0.3

        let close = only(clearance(&parallel_at(0.6), &services));
        near(close.horizontal_gap, 0.4, 1e-12);
        assert_eq!(close.status, ClearanceStatus::WithinTolerance); // 0.4 >= 0.3, 0.1 < 0.3

        let too_near = only(clearance(&parallel_at(0.45), &services));
        assert_eq!(too_near.status, ClearanceStatus::Conflict);
        assert_eq!(too_near.level, QualityLevel::B);
    }

    #[test]
    fn a_ql_c_service_near_the_works_is_unconfirmed_not_clear() {
        let services = [straight_service(evidence(
            LocationMethod::SurfaceFeature,
            None,
            None,
            false,
        ))];
        let close = only(clearance(&parallel_at(1.2), &services));
        assert_eq!(close.level, QualityLevel::C);
        assert_eq!(close.status, ClearanceStatus::Unconfirmed);
        assert!(close.horizontal_tolerance.is_none());
        assert!(close.note.contains("QL-B or QL-A"));

        // 5.0 - 0.2 = 4.8 >= 0.3 + 2.0.
        assert_eq!(
            only(clearance(&parallel_at(5.0), &services)).status,
            ClearanceStatus::Clear
        );
    }

    #[test]
    fn a_crossing_is_cleared_by_vertical_separation() {
        // Two potholes, top of service 9.0 (centre 8.9), exposed between them.
        let mut exposed = UtilityLine {
            id: String::from("X"),
            attributes: UtilityAttributes {
                diameter: 0.2,
                ..Default::default()
            },
            vertices: vec![
                vertex("A", 0.0, 0.0, POTHOLE, Some(9.0)),
                vertex("B", 8.0, 0.0, POTHOLE, Some(9.0)),
            ],
            path_evidence: vec![PathEvidence::Exposed],
        };

        // Works crossing at northing 4, centre level 9.5, 200 mm wide: face to
        // face |9.5 - 8.9| - 0.1 - 0.1 = 0.4.
        let crossing = DesignAlignment {
            half_width: 0.1,
            vertices: vec![
                design_vertex(4.0, -5.0, Some(9.5)),
                design_vertex(4.0, 5.0, Some(9.5)),
            ],
            ..Default::default()
        };

        let at_a = only(clearance(&crossing, std::slice::from_ref(&exposed)));
        near(at_a.plan_distance, 0.0, 1e-12);
        assert!(at_a.vertical_gap.is_some());
        near(at_a.vertical_gap.unwrap(), 0.4, 1e-12);
        assert_eq!(at_a.status, ClearanceStatus::Clear); // 0.4 - 0.05 >= 0.3
        assert_eq!(at_a.note, "cleared by vertical separation");

        // The same crossing where the path was only detected is QL-B: 0.4 - 0.5 < 0.3.
        exposed.path_evidence = vec![PathEvidence::Detected];
        let detected = only(clearance(&crossing, std::slice::from_ref(&exposed)));
        assert_eq!(detected.status, ClearanceStatus::WithinTolerance);
        assert!(detected.note.contains("not across the tolerance"));
    }

    #[test]
    fn refuses_a_design_it_cannot_measure() {
        let services = [straight_service(EML)];
        let mut design = parallel_at(1.0);
        design.vertices.truncate(1);
        assert_eq!(
            clearance(&design, &services).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        design = parallel_at(1.0);
        design.half_width = -1.0;
        assert_eq!(
            clearance(&design, &services).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
}

// ---- verification --------------------------------------------------------------

mod subsurface_verification {
    use super::*;

    #[test]
    fn exposures_check_the_detections_they_name() {
        let depth_eml = evidence(
            LocationMethod::ElectromagneticLocation,
            Some(0.1),
            Some(0.3),
            true,
        );
        let mut line = UtilityLine {
            id: String::from("G1"),
            vertices: vec![
                vertex("D1", 0.0, 0.0, depth_eml, Some(9.0)),
                vertex("D2", 0.0, 10.0, EML, None),
                vertex("X1", 0.2, 0.1, POTHOLE, Some(9.3)),
                vertex("X2", 0.4, 10.0, POTHOLE, Some(9.0)),
                vertex("X3", 1.0, 1.0, POTHOLE, Some(9.0)),
                vertex("X4", 1.0, 1.0, EML, None),
            ],
            ..Default::default()
        };
        line.vertices[2].verifies = String::from("D1"); // dH = hypot(0.2, 0.1), dV = +0.3: both pass
        line.vertices[3].verifies = String::from("D2"); // 0.4 m in plan: fails; D2 has no level
        line.vertices[4].verifies = String::from("D9"); // names nothing
        line.vertices[5].verifies = String::from("D1"); // not an exposure

        let report = verify_detections(&[line], &GradingSettings::default());
        assert_eq!(report.results.len(), 2);
        near(
            report.results[0].horizontal_deviation,
            0.2_f64.hypot(0.1),
            1e-12,
        );
        assert!(report.results[0].horizontal_within);
        assert!(report.results[0].vertical_deviation.is_some());
        near(report.results[0].vertical_deviation.unwrap(), 0.3, 1e-12);
        assert_eq!(report.results[0].vertical_within, Some(true));
        assert!(!report.results[1].horizontal_within);
        assert!(report.results[1].vertical_deviation.is_none());

        assert_eq!(report.horizontal_passed, 1);
        assert_eq!(report.vertical_checked, 1);
        assert_eq!(report.vertical_passed, 1);
        near(report.maximum_horizontal, 0.4, 1e-12);
        near(
            report.rms_horizontal,
            ((0.05_f64 + 0.16) / 2.0).sqrt(),
            1e-12,
        );

        assert_eq!(report.problems.len(), 2);
        assert!(report.problems[0].contains("no line has"));
        assert!(report.problems[1].contains("not QL-A"));
    }
}
