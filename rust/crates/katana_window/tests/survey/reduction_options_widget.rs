//! The twins of tests/qt_widgets/survey/test_reduction_options_widget.cpp: the reduction and
//! adjustment options as a surveyor sets them - the defaults they start from, the advanced
//! section folded until opened, a job's settings round-tripped through the fields bit for bit,
//! numbers typed in seconds and millimetres stored in radians and metres, and control picked from
//! the file's and the drawing's points.
//!
//! The widget is drawn in the harness's window, as the C++ showed it on its own; its controls are
//! driven and read by their object names.

use katana_core::ErrorCode;
use katana_survey::reduction_settings::{
    parse_reduction_settings, serialise_reduction_settings, validate_reduction_settings,
};
use katana_survey::{
    AdjustmentMethod, AtmosphericCorrection, ControlOrigin, ControlPoint, ControlSelection,
    CorrectionState, FaceHandling, FaceTolerances, GridScale, HeightReduction, InstrumentSettings,
    NetworkDimension, ObservationPrecision, OutlierTest, PrismConstantPolicy, ReductionSettings,
    Station, SurveyPoint, SurveyProject, SurveyStation, TraverseRule,
};
use katana_window::drive::Harness;
use katana_window::survey::reduction_options_widget::{
    ReductionOptionsWidget, atmospheric_state_text,
};

use crate::{cells, double_eq};

fn point(id: &str, northing: f64, easting: f64, elevation: Option<f64>) -> SurveyPoint {
    SurveyPoint {
        id: String::from(id),
        northing,
        easting,
        elevation,
        ..SurveyPoint::default()
    }
}

fn station_with(id: &str, state: CorrectionState, ppm: Option<f64>) -> SurveyStation {
    SurveyStation {
        setup: Station {
            id: String::from(id),
            point_id: String::from(id),
            ..Station::default()
        },
        instrument: InstrumentSettings {
            atmospheric_ppm_state: state,
            atmospheric_ppm: ppm,
            ..InstrumentSettings::default()
        },
        ..SurveyStation::default()
    }
}

/// The widget in the harness's window.
struct Shown {
    harness: Harness,
    widget: ReductionOptionsWidget,
}

impl Shown {
    fn new(widget: ReductionOptionsWidget) -> Self {
        let mut shown = Self {
            harness: Harness::new(),
            widget,
        };
        shown.run();
        shown
    }

    fn run(&mut self) {
        let widget = &mut self.widget;
        if let Err(error) = self.harness.run(|ui| {
            widget.ui(ui);
        }) {
            panic!("{error}");
        }
    }

    /// QLineEdit::setText or QComboBox::setCurrentIndex by the item's text, as the C++'s `type`
    /// and `choose`.
    fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.run();
    }

    fn click(&mut self, name: &str) {
        self.harness.press(name);
        self.run();
    }

    fn text(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn visible(&self, name: &str) -> bool {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .visible
    }
}

mod reduction_options {
    use super::*;

    #[test]
    fn a_new_widget_gives_the_reduction_defaults_with_the_advanced_section_folded() {
        let shown = Shown::new(ReductionOptionsWidget::new());
        let settings = shown.widget.settings().expect("the defaults read");
        assert_eq!(settings, ReductionSettings::default());
        assert!(!shown.widget.advanced_shown());
        assert!(!shown.visible("advancedOptions"));
        // The basic choices are on show.
        assert!(shown.visible("method"));
        assert!(shown.visible("atmospheric"));
        assert!(shown.visible("addControl"));
    }

    #[test]
    fn the_defaults_read_in_seconds_millimetres_and_percent() {
        let shown = Shown::new(ReductionOptionsWidget::new());
        // FaceTolerances: 10" and 20" (in radians 10 * pi / 648000), 5 mm; ObservationPrecision:
        // 3", 2 mm + 2 ppm, 1 mm centring, 2 mm heights, 1 mm per sqrt km, GNSS 10 mm and 20 mm;
        // confidence 0.95 is 95 %.
        assert_eq!(shown.text("toleranceHorizontal"), "10");
        assert_eq!(shown.text("toleranceZenith"), "20");
        assert_eq!(shown.text("toleranceDistance"), "5");
        assert_eq!(shown.text("sigmaDirection"), "3");
        assert_eq!(shown.text("sigmaDistance"), "2");
        assert_eq!(shown.text("sigmaPpm"), "2");
        assert_eq!(shown.text("sigmaInstrumentCentring"), "1");
        assert_eq!(shown.text("sigmaHeight"), "2");
        assert_eq!(shown.text("sigmaLevelling"), "1");
        assert_eq!(shown.text("sigmaGnssHorizontal"), "10");
        assert_eq!(shown.text("sigmaGnssVertical"), "20");
        assert_eq!(shown.text("refractionK"), "0.13");
        assert_eq!(shown.text("earthRadius"), "6371000");
        assert_eq!(shown.text("confidence"), "95");
        assert_eq!(shown.text("outlierSignificance"), "0.001");
        assert_eq!(
            shown
                .harness
                .control("iterations")
                .expect("the iterations")
                .value,
            "25"
        );
    }

    #[test]
    fn opening_advanced_shows_its_fields_and_closing_it_folds_them_again() {
        let mut shown = Shown::new(ReductionOptionsWidget::new());
        shown.click("advanced");
        assert!(shown.widget.advanced_shown());
        for name in [
            "toleranceHorizontal",
            "sigmaDirection",
            "refractionK",
            "fixedPpm",
            "gridScaleFactor",
            "confidence",
            "outlierSignificance",
        ] {
            assert!(shown.visible(name), "{name}");
        }
        assert!(shown.visible("heightReduction"));
        assert!(shown.visible("autoReject"));
        shown.click("advanced");
        assert!(!shown.widget.advanced_shown());
        assert!(!shown.visible("toleranceHorizontal"));
    }

    #[test]
    fn a_jobs_settings_come_back_from_the_fields_exactly_as_they_went() {
        // Every setting away from its default, with numbers that no field shows exactly (7.3" is
        // 3.5391...e-05 rad), so an untouched field must give its setting's own double back, not
        // its rounded text read again.
        let settings = ReductionSettings {
            atmospheric: AtmosphericCorrection::Fixed,
            fixed_ppm: 12.345678901234,
            prism_constant_policy: PrismConstantPolicy::Override,
            prism_constant: -0.0344,
            faces: FaceHandling::Separate,
            face_tolerances: FaceTolerances {
                horizontal: 7.3 * std::f64::consts::PI / 648000.0,
                zenith: 1.0e-4 / 3.0,
                distance: 0.0071,
                exclude_outside: true,
            },
            curvature_and_refraction: false,
            refraction_coefficient: 0.142,
            earth_radius: 6_378_137.0,
            slope_to_horizontal: false,
            height_reduction: HeightReduction::Geoid,
            grid_scale: GridScale::Fixed,
            fixed_grid_scale_factor: 0.99960123456789,
            use_combined_factor: true,
            combined_factor: 1.0000123,
            method: AdjustmentMethod::Network,
            traverse_rule: TraverseRule::Transit,
            network_dimension: NetworkDimension::HorizontalAndLevels,
            apriori: ObservationPrecision {
                direction: 1.0 / 3.0 * 1e-5,
                zenith: 2.2e-5,
                distance_constant: 0.0015,
                distance_ppm: 1.5,
                instrument_centring: 0.0007,
                target_centring: 0.0009,
                height_measurement: 0.0013,
                levelling_per_sqrt_kilometre: 0.0025,
                gnss_horizontal: 0.008,
                gnss_vertical: 0.015,
            },
            use_file_covariances: false,
            control: vec![
                ControlSelection {
                    point: ControlPoint::fixed3d("CP1"),
                    origin: ControlOrigin::File,
                },
                ControlSelection {
                    point: ControlPoint::weighted_horizontal("CP;2", 0.004, 0.006),
                    origin: ControlOrigin::Drawing,
                },
            ],
            confidence_level: 0.99,
            outlier_test: OutlierTest::Tau,
            outlier_significance: 0.0025,
            auto_reject_outliers: true,
            max_iterations: 40,
        };
        assert!(validate_reduction_settings(&settings).is_ok());

        let mut widget = ReductionOptionsWidget::new();
        widget.set_settings(&settings);
        let back = widget.settings().expect("the settings read back");
        assert_eq!(back, settings);
        // And through the text form a job stores them in, as a save would.
        let stored = parse_reduction_settings(&serialise_reduction_settings(&back), None)
            .expect("the text reads");
        assert_eq!(stored, settings);
    }

    #[test]
    fn numbers_typed_in_seconds_millimetres_and_percent_are_stored_in_radians_metres_and_a_fraction()
     {
        let mut shown = Shown::new(ReductionOptionsWidget::new());
        shown.fill("toleranceHorizontal", "5");
        shown.fill("sigmaDistance", "3");
        shown.fill("prismConstantValue", "-34.4");
        shown.fill("confidence", "99");
        let settings = shown.widget.settings().expect("the numbers read");
        // 5" = 5 / (648000 / pi) rad; 3 mm = 0.003 m; -34.4 mm = -0.0344 m; 99 % = 0.99 - each a
        // division, as the widget does it.
        assert!(double_eq(
            settings.face_tolerances.horizontal,
            5.0 * std::f64::consts::PI / 648000.0
        ));
        assert_eq!(settings.apriori.distance_constant, 3.0 / 1000.0);
        assert_eq!(settings.prism_constant, -34.4 / 1000.0);
        assert_eq!(settings.confidence_level, 0.99);
    }

    #[test]
    fn a_number_that_does_not_read_is_refused_naming_its_field() {
        let mut shown = Shown::new(ReductionOptionsWidget::new());
        shown.fill("sigmaZenith", "three");
        let error = shown
            .widget
            .settings()
            .expect_err("a number that does not read");
        assert_eq!(error.code, ErrorCode::ParseFailure);
        assert!(
            error.message.contains("the zenith standard deviation"),
            "{}",
            error.message
        );
    }

    #[test]
    fn control_is_picked_from_the_file_or_the_drawing_and_held_fixed_or_weighted() {
        let raw = SurveyProject {
            points: vec![
                point("A", 1000.0, 1000.0, Some(50.0)),
                point("B", 1100.0, 1000.0, None),
            ],
            ..SurveyProject::default()
        };
        let cp = point("CP7", 6_250_000.0, 330_000.0, Some(12.0));
        let mut widget = ReductionOptionsWidget::new();
        widget.set_project(&raw);
        widget.set_drawing_points(&[cp]);
        let mut shown = Shown::new(widget);

        shown.fill("controlFrom", "the file");
        shown.fill("controlPick", "A");
        shown.fill("controlHorizontal", "fixed");
        shown.fill("controlVertical", "fixed");
        shown.click("addControl");
        shown.fill("controlFrom", "the drawing");
        shown.fill("controlPick", "CP7");
        shown.fill("controlHorizontal", "weighted");
        shown.fill("controlSigmaHorizontal", "5");
        shown.fill("controlVertical", "free");
        shown.click("addControl");

        let settings = shown.widget.settings().expect("the control reads");
        assert_eq!(settings.control.len(), 2);
        assert_eq!(
            settings.control[0],
            ControlSelection {
                point: ControlPoint::fixed3d("A"),
                origin: ControlOrigin::File,
            }
        );
        // 5 mm is 0.005 m, north and east alike; the elevation left free.
        assert_eq!(
            settings.control[1],
            ControlSelection {
                point: ControlPoint::weighted_horizontal("CP7", 0.005, 0.005),
                origin: ControlOrigin::Drawing,
            }
        );
        let table = shown.harness.control("control").expect("the control table");
        assert_eq!(table.items.len(), 2);
        assert_eq!(cells(&table.items[1])[2], "weighted 5 mm");

        // Chosen in the table and released.
        shown.fill("control", "A");
        shown.click("removeControl");
        let after = shown.widget.settings().expect("the control reads");
        assert_eq!(after.control.len(), 1);
        assert_eq!(after.control[0].point.point_id, "CP7");
    }

    #[test]
    fn what_the_instrument_applied_is_said_per_setup_with_its_values() {
        let mut raw = SurveyProject {
            stations: vec![
                station_with("S1", CorrectionState::Applied, Some(12.4)),
                station_with("S2", CorrectionState::Applied, Some(12.4)),
            ],
            ..SurveyProject::default()
        };
        assert_eq!(
            atmospheric_state_text(&raw),
            "applied by the instrument at all 2 setups (+12.4 ppm)"
        );
        raw.stations
            .push(station_with("S3", CorrectionState::NotApplied, None));
        raw.stations
            .push(station_with("S4", CorrectionState::Unknown, None));
        assert_eq!(
            atmospheric_state_text(&raw),
            "applied by the instrument at 2 of 4 setups (+12.4 ppm); not applied by the \
             instrument at 1 of 4 setups; not stated by the file at 1 of 4 setups"
        );
        assert_eq!(
            atmospheric_state_text(&SurveyProject::default()),
            "the file has no instrument setups"
        );

        let mut widget = ReductionOptionsWidget::new();
        widget.set_project(&raw);
        let shown = Shown::new(widget);
        assert_eq!(shown.text("atmosphericState"), atmospheric_state_text(&raw));
    }
}
