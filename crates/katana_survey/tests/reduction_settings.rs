//! Twin of tests/survey/test_reduction_settings.cpp: the reduction settings,
//! their defaults, validation and text form, and the contract's empty values.

#![allow(
    clippy::excessive_precision,
    reason = "The expected values are the C++ tests' literals, digit for digit."
)]

mod survey_support;

use katana_survey::data_model::{
    ControlComponent, ControlConstraint, ControlPoint, ObservationPrecision, SurveyProject,
    distance_sigma, to_string,
};
use katana_survey::reduction::{ComputedPoint, ReductionContext};
use katana_survey::reduction_report::{ComputationMethod, CorrectionKind, ReductionReport};
use katana_survey::reduction_settings::{
    AdjustmentMethod, AtmosphericCorrection, ControlOrigin, ControlSelection, FaceHandling,
    FaceTolerances, GridScale, HeightReduction, NetworkDimension, OutlierTest, PrismConstantPolicy,
    ReductionSettings, TraverseRule, control_from_file, parse_reduction_settings,
    serialise_reduction_settings, validate_reduction_settings,
};

// One second of arc in radians: pi / (180 * 3600) = pi / 648000
// = 3.14159265358979 / 648000 = 4.84813681109536e-06.
const ARC_SECOND: f64 = 4.84813681109536e-06;

// Every setting moved off its default, and control ids that need escaping in
// the text form (';' separates fields, '=' separates the key, '%' is the
// escape itself, and a line break would end the line).
fn everything_changed() -> ReductionSettings {
    let first = ControlSelection {
        point: ControlPoint::fixed3d("CP;1=a%b"),
        origin: ControlOrigin::Drawing,
    };
    let mut second = ControlSelection {
        point: ControlPoint::weighted_horizontal("line\nbreak", 0.005, 0.006),
        origin: ControlOrigin::File,
    };
    second.point.elevation = ControlComponent {
        constraint: ControlConstraint::Weighted,
        sigma: 0.01,
    };
    ReductionSettings {
        atmospheric: AtmosphericCorrection::Fixed,
        fixed_ppm: -12.3,
        prism_constant_policy: PrismConstantPolicy::Override,
        prism_constant: -0.0344,
        faces: FaceHandling::Separate,
        face_tolerances: FaceTolerances {
            horizontal: 7.0 * ARC_SECOND,
            zenith: 13.0 * ARC_SECOND,
            distance: 0.0031,
            exclude_outside: false,
        },
        curvature_and_refraction: false,
        refraction_coefficient: 0.142,
        earth_radius: 6378137.0,
        slope_to_horizontal: false,
        height_reduction: HeightReduction::Ellipsoid,
        grid_scale: GridScale::FromProjection,
        fixed_grid_scale_factor: 0.9996,
        use_combined_factor: true,
        combined_factor: 0.99987654321,
        method: AdjustmentMethod::Network,
        traverse_rule: TraverseRule::LeastSquares,
        network_dimension: NetworkDimension::HorizontalAndLevels,
        apriori: ObservationPrecision {
            direction: 1.0 * ARC_SECOND,
            zenith: 1.5 * ARC_SECOND,
            distance_constant: 0.001,
            distance_ppm: 1.5,
            instrument_centring: 0.0005,
            target_centring: 0.0007,
            height_measurement: 0.003,
            levelling_per_sqrt_kilometre: 0.0003,
            gnss_horizontal: 0.008,
            gnss_vertical: 0.015,
        },
        use_file_covariances: false,
        control: vec![first, second],
        confidence_level: 0.99,
        outlier_test: OutlierTest::Tau,
        outlier_significance: 0.0005,
        auto_reject_outliers: true,
        max_iterations: 40,
    }
}

fn parsed(text: &str) -> ReductionSettings {
    parse_reduction_settings(text, None).unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod reduction_settings {
    use super::*;
    use crate::survey_support::near;
    use katana_core::ErrorCode;

    #[test]
    fn defaults_reduce_like_the_instrument_and_adjust_nothing() {
        let settings = ReductionSettings::default();
        assert_eq!(settings.atmospheric, AtmosphericCorrection::Auto);
        assert_eq!(settings.prism_constant_policy, PrismConstantPolicy::Auto);
        assert_eq!(settings.faces, FaceHandling::Average);
        assert!(settings.curvature_and_refraction);
        assert_eq!(settings.refraction_coefficient, 0.13);
        assert_eq!(settings.earth_radius, 6371000.0);
        assert!(settings.slope_to_horizontal);
        assert_eq!(settings.height_reduction, HeightReduction::None);
        assert_eq!(settings.grid_scale, GridScale::None);
        assert!(!settings.use_combined_factor);
        assert_eq!(settings.method, AdjustmentMethod::None);
        assert!(settings.control.is_empty());
        assert_eq!(settings.confidence_level, 0.95);
        assert_eq!(settings.outlier_test, OutlierTest::Baarda);
        assert_eq!(settings.outlier_significance, 0.001);
        assert!(!settings.auto_reject_outliers);
        assert_eq!(settings.max_iterations, 25);
        assert!(validate_reduction_settings(&settings).is_ok());
    }

    #[test]
    fn default_precision_is_that_of_a_three_second_instrument_with_a_two_plus_two_edm() {
        let precision = ObservationPrecision::default();
        // 3" = 3 * 4.84813681109536e-06 = 1.454441043328608e-05 rad.
        near(precision.direction, 1.454441043328608e-05, 1e-18);
        near(precision.zenith, 1.454441043328608e-05, 1e-18);
        assert_eq!(precision.distance_constant, 0.002);
        assert_eq!(precision.distance_ppm, 2.0);
        // Face tolerances: 10" = 4.84813681109536e-05 rad horizontal, 20" zenith.
        let tolerances = FaceTolerances::default();
        near(tolerances.horizontal, 4.84813681109536e-05, 1e-17);
        near(tolerances.zenith, 9.69627362219072e-05, 1e-17);
        assert_eq!(tolerances.distance, 0.005);
    }

    #[test]
    fn the_distance_sigma_combines_the_constant_and_the_proportional_part() {
        // 2 mm + 2 ppm at 1000 m: the ppm part is 2e-6 * 1000 = 0.002 m, so
        // sqrt(0.002^2 + 0.002^2) = 0.002 * sqrt(2) = 0.0028284271247461903.
        near(
            distance_sigma(&ObservationPrecision::default(), 1000.0),
            0.0028284271247461903,
            1e-15,
        );
        // At zero length only the constant is left.
        assert_eq!(distance_sigma(&ObservationPrecision::default(), 0.0), 0.002);
    }

    #[test]
    fn the_default_text_starts_with_its_version_and_writes_short_numbers() {
        let text = serialise_reduction_settings(&ReductionSettings::default());
        assert!(text.starts_with("katana-reduction-settings=1\n"));
        // The shortest text that reads back as the double 0.13 is "0.13".
        assert!(text.contains("\nrefraction.k=0.13\n"));
        assert!(text.contains("\natmospheric=auto\n"));
        assert!(text.contains("\nadjustment.method=none\n"));
        assert!(!text.contains("\ncontrol="));
    }

    #[test]
    fn defaults_survive_the_text_round_trip() {
        let round_tripped = parsed(&serialise_reduction_settings(&ReductionSettings::default()));
        assert_eq!(round_tripped, ReductionSettings::default());
    }

    #[test]
    fn every_setting_survives_the_text_round_trip_bit_for_bit() {
        let original = everything_changed();
        assert!(validate_reduction_settings(&original).is_ok());
        let text = serialise_reduction_settings(&original);
        let mut warnings = Vec::new();
        let round_tripped = parse_reduction_settings(&text, Some(&mut warnings))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(warnings.is_empty());
        assert_eq!(round_tripped, original);
        // And the text of the text is the same text.
        assert_eq!(serialise_reduction_settings(&round_tripped), text);
    }

    #[test]
    fn every_choice_of_every_enum_survives_the_round_trip() {
        fn round_trip(settings: &ReductionSettings) -> ReductionSettings {
            parsed(&serialise_reduction_settings(settings))
        }
        for value in [
            AtmosphericCorrection::None,
            AtmosphericCorrection::Auto,
            AtmosphericCorrection::Recompute,
            AtmosphericCorrection::Fixed,
        ] {
            let settings = ReductionSettings {
                atmospheric: value,
                ..Default::default()
            };
            assert_eq!(round_trip(&settings).atmospheric, value);
        }
        for value in [
            TraverseRule::Bowditch,
            TraverseRule::Transit,
            TraverseRule::LeastSquares,
        ] {
            let settings = ReductionSettings {
                traverse_rule: value,
                ..Default::default()
            };
            assert_eq!(round_trip(&settings).traverse_rule, value);
        }
        for value in [
            NetworkDimension::Horizontal,
            NetworkDimension::Levels,
            NetworkDimension::HorizontalAndLevels,
        ] {
            let settings = ReductionSettings {
                network_dimension: value,
                ..Default::default()
            };
            assert_eq!(round_trip(&settings).network_dimension, value);
        }
        for value in [
            FaceHandling::Average,
            FaceHandling::FaceLeftOnly,
            FaceHandling::Separate,
        ] {
            let settings = ReductionSettings {
                faces: value,
                ..Default::default()
            };
            assert_eq!(round_trip(&settings).faces, value);
        }
        for value in [GridScale::None, GridScale::Fixed, GridScale::FromProjection] {
            let settings = ReductionSettings {
                grid_scale: value,
                ..Default::default()
            };
            assert_eq!(round_trip(&settings).grid_scale, value);
        }
        for value in [
            HeightReduction::None,
            HeightReduction::Ellipsoid,
            HeightReduction::Geoid,
        ] {
            let settings = ReductionSettings {
                height_reduction: value,
                ..Default::default()
            };
            assert_eq!(round_trip(&settings).height_reduction, value);
        }
    }

    #[test]
    fn a_key_missing_from_older_text_keeps_its_default() {
        let read = parsed("katana-reduction-settings=1\nadjustment.method=network\n");
        let expected = ReductionSettings {
            method: AdjustmentMethod::Network,
            ..Default::default()
        };
        assert_eq!(read, expected);
    }

    #[test]
    fn windows_line_ends_comments_and_blank_lines_are_accepted() {
        let read =
            parsed("# saved by hand\r\nkatana-reduction-settings=1\r\n\r\nrefraction.k=0.2\r\n");
        assert_eq!(read.refraction_coefficient, 0.2);
    }

    #[test]
    fn text_from_a_newer_version_is_refused_not_half_read() {
        let refused =
            parse_reduction_settings("katana-reduction-settings=2\nrefraction.k=0.2\n", None);
        assert_eq!(
            refused.expect_err("a newer version").code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn an_unknown_key_is_reported_and_skipped_not_fatal() {
        let mut warnings = Vec::new();
        let read = parse_reduction_settings(
            "katana-reduction-settings=1\nsomething.new=7\nrefraction.k=0.2\n",
            Some(&mut warnings),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.refraction_coefficient, 0.2);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("line 2"));
        assert!(warnings[0].contains("something.new"));
    }

    #[test]
    fn a_malformed_value_is_a_parse_failure_naming_its_line() {
        let number =
            parse_reduction_settings("katana-reduction-settings=1\nrefraction.k=abc\n", None)
                .expect_err("not a number");
        assert_eq!(number.code, ErrorCode::ParseFailure);
        assert!(number.message.contains("line 2"));

        let choice =
            parse_reduction_settings("katana-reduction-settings=1\nfaces=sideways\n", None)
                .expect_err("not a choice");
        assert_eq!(choice.code, ErrorCode::ParseFailure);

        let control =
            parse_reduction_settings("katana-reduction-settings=1\ncontrol=A;file\n", None)
                .expect_err("a short control line");
        assert_eq!(control.code, ErrorCode::ParseFailure);

        let twice = parse_reduction_settings(
            "katana-reduction-settings=1\nrefraction.k=0.1\nrefraction.k=0.2\n",
            None,
        )
        .expect_err("a key given twice");
        assert_eq!(twice.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn text_that_is_not_reduction_settings_is_refused() {
        for text in ["", "refraction.k=0.13\n", "katana-reduction-settings=0\n"] {
            assert_eq!(
                parse_reduction_settings(text, None)
                    .expect_err("not reduction settings")
                    .code,
                ErrorCode::ParseFailure,
                "{text:?}"
            );
        }
    }

    #[test]
    fn values_that_make_no_surveying_sense_are_refused_by_name() {
        let refraction = ReductionSettings {
            refraction_coefficient: 1.5,
            ..Default::default()
        };
        assert_eq!(
            validate_reduction_settings(&refraction)
                .expect_err("refraction")
                .code,
            ErrorCode::InvalidArgument
        );

        let weighted = ReductionSettings {
            control: vec![ControlSelection {
                point: ControlPoint::weighted_vertical("BM1", 0.0),
                ..Default::default()
            }],
            ..Default::default()
        };
        let status = validate_reduction_settings(&weighted).expect_err("a weighted zero");
        assert!(status.message.contains("BM1"));

        let twice = ReductionSettings {
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed_horizontal("CP1"),
                    ..Default::default()
                },
                ControlSelection {
                    point: ControlPoint::fixed_vertical("CP1"),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(validate_reduction_settings(&twice).is_err());

        let confidence = ReductionSettings {
            confidence_level: 1.0,
            ..Default::default()
        };
        assert!(validate_reduction_settings(&confidence).is_err());

        let iterations = ReductionSettings {
            max_iterations: 0,
            ..Default::default()
        };
        assert!(validate_reduction_settings(&iterations).is_err());

        // The same refusal comes back through the text form.
        assert_eq!(
            parse_reduction_settings("katana-reduction-settings=1\nrefraction.k=1.5\n", None)
                .expect_err("refraction in the text")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn the_files_control_is_the_starting_selection() {
        let project = SurveyProject {
            control_points: vec![
                ControlPoint::fixed3d("CP1"),
                ControlPoint::fixed_vertical("BM2"),
            ],
            ..Default::default()
        };
        let control = control_from_file(&project);
        assert_eq!(control.len(), 2);
        assert_eq!(control[0].point, ControlPoint::fixed3d("CP1"));
        assert_eq!(control[0].origin, ControlOrigin::File);
        assert_eq!(control[1].point.point_id, "BM2");
    }
}

mod reduction_contract {
    use super::*;

    #[test]
    fn the_report_and_outcome_start_empty() {
        let report = ReductionReport::default();
        assert!(report.observations.is_empty());
        assert!(report.adjustments.is_empty());
        assert_eq!(report.settings, ReductionSettings::default());
        let point = ComputedPoint::default();
        assert_eq!(point.method, ComputationMethod::Radiation);
        assert!(point.elevation.is_none());
        let context = ReductionContext::default();
        assert!(context.grid_scale_factor.is_none());
        assert!(context.geoid_separation.is_none());
        assert_eq!(
            to_string(CorrectionKind::CurvatureRefraction),
            "curvature and refraction"
        );
        assert_eq!(
            to_string(ComputationMethod::TraverseBowditch),
            "traverse (Bowditch)"
        );
    }
}
