//! Twin of tests/survey/test_reduction_corrections.cpp: each correction of the
//! reduction on its own, against a value worked by hand.
//!
//! The project is one setup on A (N 1000, E 1000, H 500) oriented on B due
//! north, reading 0 on B, and one shot to P due east. Everything is off except
//! the correction under test, so the report's number for it is the whole story.

mod reduction_support;
mod survey_support;

mod reduction_corrections {
    use crate::reduction_support::{
        bare_settings, deg, find_correction, find_point, find_row, point, setup, shot, succeeded,
        unpositioned,
    };
    use crate::survey_support::{double_eq, near};
    use katana_core::ErrorCode;
    use katana_survey::data_model::{
        CorrectionState, Face, InstrumentSettings, Observation, SurveyProject, TargetInfo,
    };
    use katana_survey::reduction::{ReductionContext, ReductionOutcome, reduce_and_adjust};
    use katana_survey::reduction_report::{CorrectionKind, ReportObservation};
    use katana_survey::reduction_settings::{
        AtmosphericCorrection, GridScale, HeightReduction, PrismConstantPolicy, ReductionSettings,
    };

    fn one_shot(
        zenith: f64,
        slope: f64,
        target_height: f64,
        instrument: InstrumentSettings,
        target: TargetInfo,
    ) -> SurveyProject {
        let mut project = SurveyProject::default();
        project.points.push(point("A", 1000.0, 1000.0, Some(500.0)));
        project.points.push(point("B", 1100.0, 1000.0, Some(500.0)));
        project.unpositioned_points.push(unpositioned("P"));
        let mut station = setup(
            "S1",
            "A",
            1.5,
            "B",
            &[
                shot("B", 1, Face::Left, Some(0.0), None, None, 0.0),
                shot(
                    "P",
                    2,
                    Face::Left,
                    Some(deg(90.0)),
                    Some(zenith),
                    Some(slope),
                    target_height,
                ),
            ],
        );
        station.instrument = instrument;
        for observation in &mut station.observations {
            if let Observation::Distance(distance) = observation {
                distance.target = target.clone();
            }
        }
        project.stations.push(station);
        project
    }

    // oneShot(zenith, slope) of the C++: target height 1.5, nothing stated.
    fn plain_shot(zenith: f64, slope: f64) -> SurveyProject {
        one_shot(
            zenith,
            slope,
            1.5,
            InstrumentSettings::default(),
            TargetInfo::default(),
        )
    }

    fn reduce(project: &SurveyProject, settings: &ReductionSettings) -> ReductionOutcome {
        succeeded(reduce_and_adjust(
            project,
            settings,
            &ReductionContext::default(),
        ))
    }

    fn distance_row(outcome: &ReductionOutcome) -> &ReportObservation {
        find_row(&outcome.report, "slope distance", "P", 0).expect("the slope distance's row")
    }

    #[test]
    fn the_atmospheric_correction_at_twenty_degrees_is_eight_ppm_by_the_iugg1999_formula() {
        // Worked with a calculator from the formula in reduction_formulas.rs:
        //   N_gr = 287.6155 + 4.88660 / 0.658^2 + 0.06800 / 0.658^4
        //        = 287.6155 + 11.28639 + 0.36275 = 299.26464
        //   (273.15 / 1013.25) N_gr = 80.675189
        //   reference 12 C, 1013.25 hPa, 60 %:
        //     E = 6.1078 * 10^(7.5*12/249.3) = 14.02477 hPa, e = 8.41486
        //     N_ref = 80.675189 * 1013.25 / 285.15 - 11.27 * 8.41486 / 285.15 = 286.33807
        //   20 C, 1013.25 hPa, 60 %:
        //     E = 6.1078 * 10^(7.5*20/257.3) = 23.38094 hPa, e = 14.02856
        //     N_L = 80.675189 * 1013.25 / 293.15 - 11.27 * 14.02856 / 293.15 = 278.30815
        //   ppm = 286.33807 - 278.30815 = +8.02992
        // Leica's published formula (286.338 - 0.29535 p / (1 + t/273.15) +
        // 4.126e-4 h / (1 + t/273.15) 10^(7.5t/(237.3+t) + 0.7857)) gives 8.0308
        // for the same air; the 0.001 ppm between them is the Magnus constant.
        // NeuralSurvey's atmospheric_ppm gives 7.5830 (no humidity term, and
        // 0.29525 where the refractivity gives 0.29535): 0.45 ppm lower.
        let instrument = InstrumentSettings {
            temperature_celsius: Some(20.0),
            pressure_hectopascals: Some(1013.25),
            relative_humidity_percent: Some(60.0),
            atmospheric_ppm_state: CorrectionState::NotApplied,
            ..Default::default()
        };
        let settings = ReductionSettings {
            atmospheric: AtmosphericCorrection::Auto,
            ..bare_settings()
        };

        let outcome = reduce(
            &one_shot(deg(90.0), 1000.0, 1.5, instrument, TargetInfo::default()),
            &settings,
        );
        let row = distance_row(&outcome);
        let atmospheric =
            find_correction(row, CorrectionKind::Atmospheric).expect("an atmospheric correction");
        near(atmospheric.amount, 1000.0 * 8.02992e-6, 1e-8);
        let factor = atmospheric.factor.expect("a factor");
        near(factor, 1.0 + 8.02992e-6, 1e-11);
        near(factor, 1.0 + 8.0308e-6, 0.002e-6); // Leica's formula
        // Horizontal (zenith 90), nothing else on: the reduced distance is the
        // corrected one.
        near(row.reduced.expect("a reduced value"), 1000.00802992, 1e-7);
    }

    #[test]
    fn an_atmospheric_correction_the_instrument_already_applied_is_not_applied_again() {
        let mut instrument = InstrumentSettings {
            temperature_celsius: Some(20.0),
            pressure_hectopascals: Some(1013.25),
            atmospheric_ppm: Some(8.0),
            atmospheric_ppm_state: CorrectionState::Applied,
            ..Default::default()
        };
        let mut settings = ReductionSettings {
            atmospheric: AtmosphericCorrection::Auto,
            ..bare_settings()
        };
        let outcome = reduce(
            &one_shot(
                deg(90.0),
                1000.0,
                1.5,
                instrument.clone(),
                TargetInfo::default(),
            ),
            &settings,
        );
        assert!(find_correction(distance_row(&outcome), CorrectionKind::Atmospheric).is_none());

        // Recompute takes the instrument's 8 ppm out and puts the computed one
        // in: net factor (1 + 8.02992e-6) / (1 + 8e-6) = 1 + 0.02992e-6.
        settings.atmospheric = AtmosphericCorrection::Recompute;
        instrument.relative_humidity_percent = Some(60.0);
        let recomputed = reduce(
            &one_shot(deg(90.0), 1000.0, 1.5, instrument, TargetInfo::default()),
            &settings,
        );
        let net = find_correction(distance_row(&recomputed), CorrectionKind::Atmospheric)
            .expect("a net correction");
        near(net.amount, 1000.0 * 0.02992e-6, 1e-8);
    }

    #[test]
    fn an_unstated_atmospheric_state_applies_nothing_and_says_so() {
        let instrument = InstrumentSettings {
            temperature_celsius: Some(30.0),
            pressure_hectopascals: Some(950.0),
            ..Default::default()
        };
        let settings = ReductionSettings {
            atmospheric: AtmosphericCorrection::Auto,
            ..bare_settings()
        };
        let outcome = reduce(
            &one_shot(deg(90.0), 1000.0, 1.5, instrument, TargetInfo::default()),
            &settings,
        );
        assert!(find_correction(distance_row(&outcome), CorrectionKind::Atmospheric).is_none());
        let warned = outcome
            .report
            .warnings
            .iter()
            .any(|warning| warning.text.contains("atmospheric"));
        assert!(warned);
    }

    #[test]
    fn a_prism_constant_the_shot_says_is_missing_is_added_as_recorded() {
        // A Leica round prism on a zero-constant setting: -34.4 mm, not applied.
        let mut target = TargetInfo {
            prism_constant: Some(-0.0344),
            prism_constant_state: CorrectionState::NotApplied,
            ..Default::default()
        };
        let mut settings = ReductionSettings {
            prism_constant_policy: PrismConstantPolicy::Auto,
            ..bare_settings()
        };
        let outcome = reduce(
            &one_shot(
                deg(90.0),
                100.0,
                1.5,
                InstrumentSettings::default(),
                target.clone(),
            ),
            &settings,
        );
        let row = distance_row(&outcome);
        let prism = find_correction(row, CorrectionKind::PrismConstant).expect("a prism constant");
        double_eq(prism.amount, -0.0344);
        near(row.reduced.unwrap(), 99.9656, 1e-12);

        // Override with 0 mm replaces a constant the instrument applied: +34.4 mm.
        target.prism_constant_state = CorrectionState::Applied;
        settings.prism_constant_policy = PrismConstantPolicy::Override;
        settings.prism_constant = 0.0;
        let overridden = reduce(
            &one_shot(deg(90.0), 100.0, 1.5, InstrumentSettings::default(), target),
            &settings,
        );
        let replaced = find_correction(distance_row(&overridden), CorrectionKind::PrismConstant)
            .expect("a replaced constant");
        double_eq(replaced.amount, 0.0344);
    }

    #[test]
    fn slope_to_horizontal_with_curvature_and_refraction_matches_the_worked_kilometre_sight() {
        // S = 1000 m at z = 85 deg, k = 0.13, R = 6 371 000 m, HI 1.5, HT 1.8:
        //   X = 1000 cos 85 = 87.155743    Y = 1000 sin 85 = 996.194698
        //   A = (1 - 0.065) / 6371000 = 1.4675875e-7
        //   A X Y = 1.4675875e-7 * 87.155743 * 996.194698 = 0.0127422 m
        //   HD = 996.194698 - 0.012742 = 996.181956
        //   B = 0.87 / 12742000 = 6.8278135e-8, B Y^2 = 0.0677595 m
        //   dH = 87.155743 + 0.067759 + 1.5 - 1.8 = 86.923502
        // Cross-check: the owner's NeuralSurvey engine (corrections.rs, run with
        // cargo on the same inputs) gives height_diff = 86.923502234 (the same
        // height) and slope_to_horizontal = 996.194698092, which is Y: it
        // leaves out the A X Y term, 12.7 mm on this sight.
        let settings = ReductionSettings {
            curvature_and_refraction: true,
            ..bare_settings()
        };
        let outcome = reduce(
            &one_shot(
                deg(85.0),
                1000.0,
                1.8,
                InstrumentSettings::default(),
                TargetInfo::default(),
            ),
            &settings,
        );
        let row = distance_row(&outcome);
        let slope = find_correction(row, CorrectionKind::SlopeToHorizontal).expect("slope");
        let curvature =
            find_correction(row, CorrectionKind::CurvatureRefraction).expect("curvature");
        near(slope.amount, 996.194698 - 1000.0, 1e-6);
        near(curvature.amount, -0.0127422, 1e-7);
        near(row.reduced.unwrap(), 996.181956, 1e-6);

        let height = find_row(&outcome.report, "height difference", "P", 0).expect("height");
        near(height.raw, 87.155743, 1e-6);
        near(
            find_correction(height, CorrectionKind::CurvatureRefraction)
                .unwrap()
                .amount,
            0.0677595,
            1e-7,
        );
        near(
            find_correction(height, CorrectionKind::InstrumentAndTargetHeight)
                .unwrap()
                .amount,
            -0.3,
            1e-12,
        );
        near(height.reduced.unwrap(), 86.923502, 1e-6);
        // P = A + HD east, H = 500 + dH.
        let p = find_point(&outcome, "P").expect("P");
        near(p.easting, 1000.0 + 996.181956, 1e-6);
        near(p.northing, 1000.0, 1e-9);
        near(p.elevation.unwrap(), 586.923502, 1e-6);
    }

    #[test]
    fn height_reduction_to_the_geoid_uses_the_lines_mean_height() {
        // Level 1000 m sight from H 500 with HI 1.5: the line is at 501.5 m.
        //   factor R / (R + h) = 6371000 / 6371501.5; 1000 (factor - 1) = -0.0787099 m
        let settings = ReductionSettings {
            height_reduction: HeightReduction::Geoid,
            ..bare_settings()
        };
        let outcome = reduce(&plain_shot(deg(90.0), 1000.0), &settings);
        let height = find_correction(distance_row(&outcome), CorrectionKind::HeightReduction)
            .expect("a height reduction");
        near(height.amount, -0.0787099, 1e-7);
    }

    #[test]
    fn height_reduction_to_the_ellipsoid_without_a_geoid_is_refused() {
        let settings = ReductionSettings {
            height_reduction: HeightReduction::Ellipsoid,
            ..bare_settings()
        };
        let outcome = reduce_and_adjust(
            &plain_shot(deg(90.0), 1000.0),
            &settings,
            &ReductionContext::default(),
        );
        assert_eq!(
            outcome.expect_err("no geoid").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_fixed_grid_scale_factor_scales_the_horizontal_distance() {
        // 1000 m * (0.9996 - 1) = -0.4 m.
        let settings = ReductionSettings {
            grid_scale: GridScale::Fixed,
            fixed_grid_scale_factor: 0.9996,
            ..bare_settings()
        };
        let outcome = reduce(&plain_shot(deg(90.0), 1000.0), &settings);
        let row = distance_row(&outcome);
        near(
            find_correction(row, CorrectionKind::GridScale)
                .unwrap()
                .amount,
            -0.4,
            1e-12,
        );
        near(row.reduced.unwrap(), 999.6, 1e-12);
    }

    #[test]
    fn the_projection_scale_factor_is_taken_at_the_lines_mid_point() {
        // A made-up projection whose scale grows 1e-7 per metre east of E 1000:
        // at the station it is exactly 1, at the mid-point (E 1500) 1.00005, so
        // +0.05 m on the 1000 m line proves where it was evaluated.
        let settings = ReductionSettings {
            grid_scale: GridScale::FromProjection,
            ..bare_settings()
        };
        let context = ReductionContext {
            grid_scale_factor: Some(Box::new(|_: f64, easting: f64, _: f64| {
                Some(1.0 + 1e-7 * (easting - 1000.0))
            })),
            ..Default::default()
        };
        let outcome = succeeded(reduce_and_adjust(
            &plain_shot(deg(90.0), 1000.0),
            &settings,
            &context,
        ));
        near(
            find_correction(distance_row(&outcome), CorrectionKind::GridScale)
                .unwrap()
                .amount,
            0.05,
            1e-9,
        );

        // No projection on the drawing: refused, naming the setting.
        let refused = reduce_and_adjust(
            &plain_shot(deg(90.0), 1000.0),
            &settings,
            &ReductionContext::default(),
        );
        assert_eq!(
            refused.expect_err("no projection").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_combined_factor_replaces_the_height_and_grid_factors() {
        // 1000 m * (0.9999 - 1) = -0.1 m, and no height or grid correction.
        let settings = ReductionSettings {
            height_reduction: HeightReduction::Geoid,
            grid_scale: GridScale::Fixed,
            fixed_grid_scale_factor: 0.9996,
            use_combined_factor: true,
            combined_factor: 0.9999,
            ..bare_settings()
        };
        let outcome = reduce(&plain_shot(deg(90.0), 1000.0), &settings);
        let row = distance_row(&outcome);
        near(
            find_correction(row, CorrectionKind::CombinedFactor)
                .unwrap()
                .amount,
            -0.1,
            1e-12,
        );
        assert!(find_correction(row, CorrectionKind::HeightReduction).is_none());
        assert!(find_correction(row, CorrectionKind::GridScale).is_none());
    }

    #[test]
    fn the_corrections_are_recorded_in_the_stated_order() {
        let target = TargetInfo {
            prism_constant: Some(0.01),
            prism_constant_state: CorrectionState::NotApplied,
            ..Default::default()
        };
        let instrument = InstrumentSettings {
            atmospheric_ppm: Some(10.0),
            atmospheric_ppm_state: CorrectionState::NotApplied,
            ..Default::default()
        };
        let settings = ReductionSettings {
            height_reduction: HeightReduction::Geoid,
            grid_scale: GridScale::Fixed,
            fixed_grid_scale_factor: 0.9996,
            ..Default::default()
        };
        let outcome = reduce(
            &one_shot(deg(80.0), 500.0, 1.5, instrument, target),
            &settings,
        );
        let row = distance_row(&outcome);
        let kinds: Vec<CorrectionKind> = row
            .corrections
            .iter()
            .map(|correction| correction.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                CorrectionKind::PrismConstant,
                CorrectionKind::Atmospheric,
                CorrectionKind::SlopeToHorizontal,
                CorrectionKind::CurvatureRefraction,
                CorrectionKind::HeightReduction,
                CorrectionKind::GridScale,
            ]
        );
        // The chain adds up: raw + every amount = reduced.
        let mut value = row.raw;
        for correction in &row.corrections {
            value += correction.amount;
        }
        near(value, row.reduced.unwrap(), 1e-9);
    }
}
