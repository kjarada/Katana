// A section fitted to its viewport, its label steps and its layout
// (include/katana/cad/plotting/section_fit.hpp);
// tests/cad/plotting/test_section_fit.cpp. Every expectation is worked by
// hand from the sheet scale ladder (1:1 ... 1:50 000), the exaggeration
// ladder 1, 2, 2.5, 4, 5, 8, 10, 20 and a fill of 0.9 unless a test says
// otherwise.

use crate::{expect_box, near, rect};
use katana_cad::plotting::section_fit::{
    DEFAULT_SECTION_HALF_WIDTH, GRID_VALUES_CAP, MAXIMUM_SECTION_ROWS, SECTION_EXAGGERATIONS,
    fit_exaggeration, fit_section, grid_values, label_step, next_round_step, round_step_at_least,
    section_drawing_area, section_plot_layout, section_span, step_decimals, step_text,
    viewport_half_width, viewport_stations,
};
use katana_cad::plotting::{SectionFit, SectionFitRequest, SectionLayoutRequest, ViewportSource};
use katana_core::ErrorCode;
use katana_geometry::Box2;

fn request(span_m: f64, depth_m: f64, width_mm: f64, height_mm: f64) -> SectionFitRequest {
    SectionFitRequest {
        span_m,
        depth_m,
        plot_width_mm: width_mm,
        plot_height_mm: height_mm,
        ..SectionFitRequest::default()
    }
}

fn fitted(request: &SectionFitRequest) -> SectionFit {
    fit_section(request).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn refused(request: &SectionFitRequest) -> ErrorCode {
    fit_section(request).expect_err("no fit").code
}

// ---- the fit ---------------------------------------------------------------------------

mod section_fit {
    use super::*;

    #[test]
    fn the_scale_is_the_largest_at_which_the_span_fits_and_the_exaggeration_the_largest_for_the_depth()
     {
        // 200 m across 90% of 300 mm needs 1 : 200000 / 270 = 740.7: 1 : 750.
        // 5 m up 90% of 100 mm at 1 : 750: 5000 e / 750 <= 90 allows e <=
        // 13.5, so 10 (20 would be 133 mm).
        let fit = fitted(&request(200.0, 5.0, 300.0, 100.0));
        assert_eq!(fit.scale, 750.0);
        assert_eq!(fit.exaggeration, 10.0);
    }

    #[test]
    fn a_flat_long_section_takes_the_top_of_the_ladder() {
        // A 1 km road across 360 mm: 1 000 000 / 324 = 3086, 1 : 5000. 12 m
        // of relief at 1 : 5000 is 2.4 mm: 20 times is 48 mm, inside 81 mm.
        let fit = fitted(&request(1000.0, 12.0, 360.0, 90.0));
        assert_eq!(fit.scale, 5000.0);
        assert_eq!(fit.exaggeration, 20.0);
    }

    #[test]
    fn a_deep_narrow_section_is_drawn_smaller_rather_than_out_of_its_plot() {
        // 10 m across 90 mm would be 1 : 111, so 1 : 125; but 30 m of depth at
        // 1 : 125 is 240 mm in a 36 mm high plot. At true scale the depth
        // needs 1 : 30000 / 36 = 833: 1 : 1000, at which even 2 times is 60 mm
        // - so 1.
        let fit = fitted(&request(10.0, 30.0, 100.0, 40.0));
        assert_eq!(fit.scale, 1000.0);
        assert_eq!(fit.exaggeration, 1.0);
    }

    #[test]
    fn no_depth_takes_the_flat_exaggeration() {
        let mut flat = request(40.0, 0.0, 180.0, 60.0);
        let cross = fitted(&flat);
        assert_eq!(cross.scale, 250.0); // 40 000 / 162 = 246.9
        assert_eq!(cross.exaggeration, 1.0);
        flat.flat_exaggeration = 10.0; // a long section's usual H 1:500 V 1:50
        assert_eq!(fitted(&flat).exaggeration, 10.0);
    }

    #[test]
    fn a_span_that_fits_exactly_is_not_pushed_to_the_next_scale() {
        // 50 m across all of 100 mm is exactly 1 : 500; 4 m up all of 40 mm
        // at 1 : 500 allows exactly 5.
        let mut exact = request(50.0, 4.0, 100.0, 40.0);
        exact.fill = 1.0;
        let fit = fitted(&exact);
        assert_eq!(fit.scale, 500.0);
        assert_eq!(fit.exaggeration, 5.0);
    }

    #[test]
    fn beyond_the_ladder_the_scale_is_the_one_needed() {
        // 40 km across 90 mm at 90%: 1 : 40 000 000 / 81, past 1 : 50 000.
        let fit = fitted(&request(40000.0, 0.0, 90.0, 50.0));
        near(fit.scale, 40_000_000.0 / 81.0, 1e-3);
    }

    #[test]
    fn what_cannot_be_fitted_is_refused() {
        for span in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                refused(&request(span, 1.0, 100.0, 50.0)),
                ErrorCode::InvalidArgument,
                "{span}"
            );
        }
        assert_eq!(
            refused(&request(10.0, 1.0, 0.0, 50.0)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(&request(10.0, 1.0, 100.0, -1.0)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(&request(10.0, -1.0, 100.0, 50.0)),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(&request(10.0, f64::NAN, 100.0, 50.0)),
            ErrorCode::InvalidArgument
        );
        let mut overfull = request(10.0, 1.0, 100.0, 50.0);
        overfull.fill = 1.5;
        assert_eq!(refused(&overfull), ErrorCode::InvalidArgument);
        overfull.fill = 0.0;
        assert_eq!(refused(&overfull), ErrorCode::InvalidArgument);
    }

    #[test]
    fn the_exaggeration_is_the_largest_of_the_ladder_that_fits() {
        // The cross-section generator's case: 20 m deep at 1 : 500 in 98.8 mm
        // allows 2.47, so 2.
        assert_eq!(fit_exaggeration(20.0, 500.0, 98.8, 1.0), 2.0);
        // Allowing exactly 2.5 takes 2.5; allowing 25 takes the top, 20.
        assert_eq!(fit_exaggeration(20.0, 500.0, 100.0, 1.0), 2.5);
        assert_eq!(fit_exaggeration(2.0, 500.0, 100.0, 1.0), 20.0);
        // Not even 1 fits: 1, the least there is.
        assert_eq!(fit_exaggeration(200.0, 500.0, 100.0, 1.0), 1.0);
        // No depth: the flat value asked for.
        assert_eq!(fit_exaggeration(0.0, 500.0, 100.0, 1.0), 1.0);
        assert_eq!(fit_exaggeration(0.0, 500.0, 100.0, 10.0), 10.0);
        // 12 m at 1 : 750 in 70 mm allows 4.375: not 4, which would print
        // V 1:187.5, but 2.5, V 1:300. At 1 : 500, 4 is V 1:125 and is taken.
        assert_eq!(fit_exaggeration(12.0, 750.0, 70.0, 1.0), 2.5);
        assert_eq!(fit_exaggeration(12.0, 500.0, 100.0, 1.0), 4.0);
        // A scale off the ladder has no whole vertical: the largest that fits.
        assert_eq!(fit_exaggeration(1.0, 1234.5, 100.0, 1.0), 20.0);
        assert_eq!(SECTION_EXAGGERATIONS.len(), 8);
        assert_eq!(SECTION_EXAGGERATIONS[SECTION_EXAGGERATIONS.len() - 1], 20.0);
    }

    #[test]
    fn the_span_is_measured_around_a_fixed_centre() {
        assert_eq!(section_span(10.0, 20.0, None), 10.0);
        // Centred at 12 the plot must reach 20, 8 away, both ways: 16.
        assert_eq!(section_span(10.0, 20.0, Some(12.0)), 16.0);
        assert_eq!(section_span(10.0, 20.0, Some(25.0)), 30.0);
        assert_eq!(section_span(5.0, 5.0, None), 0.0);
        assert_eq!(section_span(20.0, 10.0, None), 0.0); // empty
    }
}

// ---- label steps ---------------------------------------------------------------------

mod section_label_steps {
    use super::*;

    #[test]
    fn round_steps_are_one_two_and_five_times_a_power_of_ten() {
        assert_eq!(round_step_at_least(0.3), 0.5);
        assert_eq!(round_step_at_least(6.0), 10.0);
        assert_eq!(round_step_at_least(1.0), 1.0);
        assert_eq!(round_step_at_least(2.0), 2.0);
        assert_eq!(round_step_at_least(7.0), 10.0);
        near(round_step_at_least(0.012), 0.02, 1e-15);
        assert_eq!(round_step_at_least(0.0), 1.0);
        assert_eq!(round_step_at_least(-3.0), 1.0);

        assert_eq!(next_round_step(1.0), 2.0);
        assert_eq!(next_round_step(2.0), 5.0);
        assert_eq!(next_round_step(5.0), 10.0);
        assert_eq!(next_round_step(20.0), 50.0);
        near(next_round_step(0.5), 1.0, 1e-15);
        near(next_round_step(0.01), 0.02, 1e-15);
    }

    #[test]
    fn a_value_is_written_with_the_decimals_its_step_needs() {
        assert_eq!(step_decimals(1.0), 0);
        assert_eq!(step_decimals(50.0), 0);
        assert_eq!(step_decimals(0.5), 1);
        assert_eq!(step_decimals(0.2), 1);
        assert_eq!(step_decimals(0.05), 2);
        assert_eq!(step_decimals(0.25), 2);
        assert_eq!(step_decimals(0.001), 3);
        assert_eq!(step_decimals(0.0001), 3); // three at most

        assert_eq!(step_text(120.0, 20.0), "120");
        assert_eq!(step_text(12.5, 0.5), "12.5");
        assert_eq!(step_text(-3.0, 1.0), "-3");
        assert_eq!(step_text(-0.0001, 0.1), "0.0"); // never "-0.0"
        assert_eq!(step_text(-0.4, 1.0), "0");
    }

    #[test]
    fn grid_values_are_whole_steps_inside_the_range() {
        assert_eq!(
            grid_values(-7.0, 13.0, 5.0, GRID_VALUES_CAP),
            vec![-5.0, 0.0, 5.0, 10.0]
        );
        assert_eq!(
            grid_values(0.0, 10.0, 5.0, GRID_VALUES_CAP),
            vec![0.0, 5.0, 10.0]
        );
        let tenths = grid_values(0.1, 0.35, 0.1, GRID_VALUES_CAP);
        assert_eq!(tenths.len(), 3);
        near(tenths[2], 0.3, 1e-15);
        assert!(grid_values(1.0, 0.0, 1.0, GRID_VALUES_CAP).is_empty());
        assert!(grid_values(0.0, 1.0, 0.0, GRID_VALUES_CAP).is_empty());
        assert_eq!(grid_values(0.0, 1000.0, 1.0, 10).len(), 10);
    }

    #[test]
    fn the_step_grows_until_the_widest_label_clears_its_neighbours() {
        // A made-up font: every character 1.2 mm wide.
        let widest = |first: f64, last: f64| {
            move |step: f64| {
                let mut w: f64 = 0.0;
                for v in grid_values(first, last, step, GRID_VALUES_CAP) {
                    let width = 1.2 * step_text(v, step).len() as f64;
                    if w < width {
                        w = width;
                    }
                }
                w
            }
        };
        // 1 : 500 is 2 mm a metre. A 10 mm minimum gives 5 m; "1300" is
        // 4.8 mm, with 2 mm of gap 6.8 mm: 5 m holds.
        assert_eq!(
            label_step(2.0, 10.0, 2.0, Some(&widest(1000.0, 1300.0))),
            5.0
        );
        // Chainages of 10 000 and more are 6 mm and, with the gap, 8 mm; at
        // 1 : 1000 (1 mm a metre) the minimum's 10 m step is 10 mm: holds. At
        // 1 : 2000 a 5 mm minimum gives 10 m, 5 mm: too tight; 20 m is 10 mm.
        assert_eq!(
            label_step(1.0, 10.0, 2.0, Some(&widest(10000.0, 12000.0))),
            10.0
        );
        assert_eq!(
            label_step(0.5, 5.0, 2.0, Some(&widest(10000.0, 12000.0))),
            20.0
        );
        // A label written across the axis passes its height: the minimum rules.
        assert_eq!(label_step(2.0, 10.0, 1.0, Some(&|_: f64| 1.4)), 5.0);
        // Wide labels on a fine grid: 1 : 25 is 40 mm a metre and the minimum
        // gives 0.5 m; "1234.5" is 7.2 mm - 9.2 with the gap - inside 20 mm.
        assert_eq!(
            label_step(40.0, 10.0, 2.0, Some(&widest(1234.0, 1236.0))),
            0.5
        );
        // Labels so wide that 5 mm steps crowd them go to 10 mm steps.
        assert_eq!(label_step(1.0, 5.0, 1.0, Some(&|_: f64| 8.0)), 10.0);
    }
}

// ---- the plot's layout ----------------------------------------------------------------

mod section_layout {
    use super::*;

    #[test]
    fn a_long_section_keeps_its_band_under_the_plot() {
        // A long section across the A3 drawing area, above its title's 7 mm:
        // 23..410 x 167..287. Four band rows are 32 mm; the plot starts 1 mm
        // above them, 22 mm in for the row names, 2 mm short of the top and
        // right.
        let mut r = SectionLayoutRequest {
            area: rect(23.0, 167.0, 410.0, 287.0),
            band_rows: 4,
            level_label_mm: 9.0,
            ..SectionLayoutRequest::default()
        };
        let layout = section_plot_layout(&r);
        assert!(layout.banded);
        assert_eq!(layout.left_mm, 22.0);
        expect_box(&layout.plot, 45.0, 200.0, 408.0, 285.0);

        // Levels wider than the names widen the column: 25 mm and 1.6 of
        // clearance.
        r.level_label_mm = 25.0;
        let wide = section_plot_layout(&r);
        near(wide.left_mm, 26.6, 1e-12);
        near(wide.plot.min.x, 49.6, 1e-12);
    }

    #[test]
    fn a_band_that_would_crush_the_plot_gives_way_to_the_axis_values() {
        // 50 mm high: the band would leave 50 - 33 - 2 = 15 mm, under 30.
        let r = SectionLayoutRequest {
            area: rect(0.0, 0.0, 200.0, 50.0),
            band_rows: 4,
            level_label_mm: 8.0,
            ..SectionLayoutRequest::default()
        };
        let layout = section_plot_layout(&r);
        assert!(!layout.banded);
        near(layout.left_mm, 9.6, 1e-12);
        expect_box(&layout.plot, 9.6, 5.0, 198.0, 48.0);
    }

    #[test]
    fn a_cross_section_leaves_room_for_its_caption_and_its_levels() {
        let mut r = SectionLayoutRequest {
            area: rect(100.0, 100.0, 280.0, 160.0),
            caption: true,
            level_label_mm: 7.4,
            ..SectionLayoutRequest::default()
        };
        let layout = section_plot_layout(&r);
        assert!(!layout.banded);
        // Left 7.4 + 1.6 = 9; below 4 for the values, 4.5 for the caption, 1
        // gap.
        expect_box(&layout.plot, 109.0, 109.5, 278.0, 158.0);
        // Short levels still get 6 mm.
        r.level_label_mm = 1.0;
        near(section_plot_layout(&r).plot.min.x, 106.0, 1e-12);
    }

    #[test]
    fn an_area_too_small_for_a_plot_has_none() {
        let mut r = SectionLayoutRequest {
            area: rect(0.0, 0.0, 20.0, 12.0),
            ..SectionLayoutRequest::default()
        };
        assert!(section_plot_layout(&r).plot.empty());
        r.area = Box2::default();
        assert!(section_plot_layout(&r).plot.empty());
    }
}

// ---- a section viewport ----------------------------------------------------------------

mod section_viewport {
    use super::*;

    #[test]
    fn its_sections_are_drawn_above_the_titles_strip() {
        expect_box(
            &section_drawing_area(&rect(23.0, 35.0, 210.0, 145.0)),
            23.0,
            42.0,
            210.0,
            145.0,
        );
        // A rectangle lower than the strip keeps none of itself for sections.
        assert_eq!(
            section_drawing_area(&rect(0.0, 0.0, 50.0, 5.0)).height(),
            0.0
        );
        assert!(section_drawing_area(&Box2::default()).empty());
    }

    #[test]
    fn cross_sections_are_cut_at_the_stations_else_every_interval_to_the_end() {
        let mut source = ViewportSource {
            stations: vec![60.0, 20.0],
            ..ViewportSource::default()
        };
        assert_eq!(viewport_stations(&source), vec![60.0, 20.0]);

        // Every 0.1 m from 0 to 0.3 lands on 0.3 itself: counted in whole
        // intervals, not added up (0.1 + 0.1 + 0.1 is 0.30000000000000004).
        source.stations.clear();
        source.chainage_from = 0.0;
        source.chainage_to = 0.3;
        source.section_interval = 0.1;
        let tenths = viewport_stations(&source);
        assert_eq!(tenths.len(), 4);
        assert_eq!(tenths[3], 3.0 * 0.1);

        // Every 20 m from 100 to 150: 100, 120, 140 - not past the end.
        source.chainage_from = 100.0;
        source.chainage_to = 150.0;
        source.section_interval = 20.0;
        assert_eq!(viewport_stations(&source), vec![100.0, 120.0, 140.0]);

        // At most MAXIMUM_SECTION_ROWS, and none without an interval or a
        // range.
        source.chainage_to = 1.0e6;
        source.section_interval = 1.0;
        assert_eq!(viewport_stations(&source).len(), MAXIMUM_SECTION_ROWS);
        source.section_interval = 0.0;
        assert!(viewport_stations(&source).is_empty());
    }

    #[test]
    fn a_half_width_not_given_is_the_default() {
        let mut source = ViewportSource::default();
        assert_eq!(viewport_half_width(&source), DEFAULT_SECTION_HALF_WIDTH);
        source.section_half_width = -5.0;
        assert_eq!(viewport_half_width(&source), DEFAULT_SECTION_HALF_WIDTH);
        source.section_half_width = f64::INFINITY;
        assert_eq!(viewport_half_width(&source), DEFAULT_SECTION_HALF_WIDTH);
        source.section_half_width = 12.5;
        assert_eq!(viewport_half_width(&source), 12.5);
    }
}
