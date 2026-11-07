//! Twin of tests/survey/test_report_summary.cpp: a reduction's outcome in a
//! line (report_summary.rs): the words the wizard and the Survey Jobs dialog
//! show, and the rejected count SURVEY IMPORT's reply shares with them. Every
//! report here is built by hand.

mod report_summary {
    use katana_survey::reduction_report::{
        AdjustmentReport, ReductionReport, ReportGlobalTest, ReportObservation,
    };
    use katana_survey::reduction_settings::{
        AdjustmentMethod, NetworkDimension, ReductionSettings, TraverseRule,
    };
    use katana_survey::report_summary::{adjustment_summary, method_text, rejected_observations};

    fn adjustment(
        method: &str,
        variance_factor: Option<f64>,
        passed: Option<bool>,
    ) -> AdjustmentReport {
        AdjustmentReport {
            method: String::from(method),
            variance_factor,
            global_test: passed.map(|passed| ReportGlobalTest {
                passed,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn each_adjustment_is_a_line_of_its_outcome_and_none_is_said_as_such() {
        let mut report = ReductionReport::default();
        assert_eq!(adjustment_summary(&report), "radiation: nothing adjusted");
        report.settings.method = AdjustmentMethod::Traverse;
        assert_eq!(
            adjustment_summary(&report),
            "traverse, Bowditch: no adjustment was run"
        );

        let mut horizontal = adjustment(
            "network least squares (horizontal)",
            Some(0.3214),
            Some(false),
        );
        horizontal.flagged_outliers = vec![String::from("a"), String::from("b"), String::from("c")];
        report.adjustments = vec![
            horizontal,
            adjustment("network least squares (levels)", Some(1.0), Some(true)),
            adjustment("network least squares (levels)", None, None),
        ];
        assert_eq!(
            adjustment_summary(&report),
            "network least squares (horizontal): variance factor 0.321, global test FAILED, 3 \
             flagged outlier(s); network least squares (levels): variance factor 1.000, global \
             test passed; network least squares (levels): no redundancy, so no variance factor"
        );
    }

    // The rows the report marks rejected (three here) and the outliers each
    // adjustment rejected (one and two): six.
    #[test]
    fn the_rejected_count_is_the_rows_marked_rejected_and_the_outliers_rejected() {
        let mut report = ReductionReport::default();
        assert_eq!(rejected_observations(&report), 0);
        let kept = ReportObservation::default();
        let rejected = ReportObservation {
            rejected: true,
            ..Default::default()
        };
        report.observations = vec![
            kept.clone(),
            rejected.clone(),
            kept,
            rejected.clone(),
            rejected,
        ];
        let first = AdjustmentReport {
            rejected_outliers: vec![String::from("x")],
            ..Default::default()
        };
        let second = AdjustmentReport {
            rejected_outliers: vec![String::from("y"), String::from("z")],
            ..Default::default()
        };
        report.adjustments = vec![first, second];
        assert_eq!(rejected_observations(&report), 6);
    }

    #[test]
    fn the_method_is_the_settings_in_words() {
        let mut settings = ReductionSettings::default();
        assert_eq!(method_text(&settings), "radiation");
        settings.method = AdjustmentMethod::Traverse;
        for (rule, words) in [
            (TraverseRule::Bowditch, "traverse, Bowditch"),
            (TraverseRule::Transit, "traverse, Transit"),
            (TraverseRule::LeastSquares, "traverse, least squares"),
        ] {
            settings.traverse_rule = rule;
            assert_eq!(method_text(&settings), words);
        }
        settings.method = AdjustmentMethod::Network;
        for (dimension, words) in [
            (NetworkDimension::Horizontal, "network, horizontal"),
            (NetworkDimension::Levels, "network, levels"),
            (
                NetworkDimension::HorizontalAndLevels,
                "network, horizontal and levels",
            ),
        ] {
            settings.network_dimension = dimension;
            assert_eq!(method_text(&settings), words);
        }
    }
}
