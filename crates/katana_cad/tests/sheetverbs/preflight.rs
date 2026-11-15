// Preflight checks (include/katana/cad/plotting/preflight.hpp): one test per
// check, each with the smallest set that trips it and the nearest set that
// does not, and the order the findings come in;
// tests/cad/plotting/test_preflight.cpp.
//
// Every test starts from a CLEAN set - one A3 sheet with a filled title
// block, a logo, and a plan at 1:500 over a line - that the checker passes
// without a word, and changes one thing. The A3 numbers are the frame's:
// drawing area 23..410 x 35..287 mm, title block 21.825..411.309 x
// 8.945..34.625 mm, tiling area 24..409 x 36..286 mm (docs/plotting.md).

use std::collections::BTreeSet;
use std::f64::consts::PI;

use crate::{near, ok};
use katana_cad::plotting::preflight::{
    Finding, PreflightOptions, PreflightSummary, Severity, check_document_sheets, check_sheets,
    finding_line, findings_from_json, findings_on_sheets, findings_to_json, plan_window,
    plan_window_corners, preflight_checks, severity_from, summarize, summary_text,
};
use katana_cad::plotting::sheet_commands::{ADD_SHEETS, add_sheets, edit_sheet_set};
use katana_cad::plotting::sheet_set::drawing_area;
use katana_cad::plotting::{
    FieldContext, GridStyle, PlanWindow, Revision, Sheet, SheetDefaults, SheetSet, SignOff,
    Viewport, ViewportKind, WorldMark, WorldMarkKind,
};
use katana_cad::{Document, SurfaceProblem, SurfaceStyleProblem};
use katana_commands::create_entities;
use katana_entity::{
    Alignment, DimensionGeometry, Entity, EnumName, Geometry, HatchPattern, Layer, Model,
    PointGeometry, TextGeometry,
};
use katana_geometry::{AlignmentPi, Box2, Circle2, Point2, Polyline2, Segment2};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

fn tiling() -> Box2 {
    rect(24.0, 36.0, 409.0, 286.0)
}

fn add(model: &mut Model, geometry: Geometry, layer: &str) {
    let entity = Entity {
        geometry,
        layer: String::from(layer),
        ..Entity::default()
    };
    ok(model.entities.add(entity));
}

fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
    Geometry::Line(Segment2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

fn text(x: f64, y: f64, words: &str, height: f64) -> Geometry {
    Geometry::Text(TextGeometry {
        position: Point2::new(x, y),
        text: String::from(words),
        height,
        rotation: 0.0,
        ..TextGeometry::default()
    })
}

fn dimension(start: Point2, end: Point2, offset: f64) -> Geometry {
    Geometry::Dimension(DimensionGeometry {
        start,
        end,
        offset,
        ..DimensionGeometry::default()
    })
}

fn pi(x: f64, y: f64) -> AlignmentPi {
    AlignmentPi {
        point: Point2::new(x, y),
        ..AlignmentPi::default()
    }
}

fn road(x0: f64, y0: f64, x1: f64, y1: f64) -> Alignment {
    let mut road = Alignment {
        name: String::from("MC01"),
        ..Alignment::default()
    };
    road.horizontal.pis = vec![pi(x0, y0), pi(x1, y1)];
    road
}

// A line 100 m east from the origin, and a straight 100 m alignment MC01
// along y = 50, CH 0 to 100.
fn drawing() -> Model {
    let mut model = Model::new();
    add(&mut model, segment(0.0, 0.0, 100.0, 0.0), "0");
    ok(model.alignments.add(road(0.0, 50.0, 100.0, 50.0)));
    model
}

fn plan_at(id: &str, rect: Box2, scale: f64, centre: Point2) -> Viewport {
    Viewport {
        id: String::from(id),
        kind: ViewportKind::Plan,
        rect,
        scale,
        centre,
        ..Viewport::default()
    }
}

fn panel(id: &str, kind: ViewportKind, rect: Box2, text: &str) -> Viewport {
    Viewport {
        id: String::from(id),
        kind,
        rect,
        text: String::from(text),
        ..Viewport::default()
    }
}

fn sheet_of(id: &str, name: &str, viewports: Vec<Viewport>) -> Sheet {
    Sheet {
        id: String::from(id),
        name: String::from(name),
        viewports,
        ..Sheet::default()
    }
}

fn sign_off(name: &str) -> SignOff {
    SignOff {
        name: String::from(name),
        date: String::new(),
    }
}

// Every value the frame prints, filled.
fn filled_title_block() -> SheetDefaults {
    SheetDefaults {
        organisation: String::from("ACME SURVEYS"),
        project_lines: vec![String::from("MAIN ROAD"), String::from("DETAIL SURVEY")],
        set_number: String::from("DS-1"),
        height_datum: String::from("AHD"),
        model_name: String::from("DESIGN"),
        locator: sign_off("A. LOCATOR"),
        surveyor: sign_off("B. SURVEYOR"),
        compiler: sign_off("C. COMPILER"),
        reviewer: sign_off("D. REVIEWER"),
        approver: sign_off("E. APPROVER"),
        logo_asset: String::from("logo.png"),
        ..SheetDefaults::default()
    }
}

fn context() -> FieldContext {
    FieldContext {
        project_name: String::from("Main Road"),
        project_description: String::from("Detail survey"),
        coordinate_system: String::from("EPSG:7856"),
        file_name: String::from("main_road"),
        plot_date: String::from("25/09/26"),
    }
}

fn revision(code: &str, date: &str, description: &str, by: &str) -> Revision {
    Revision {
        code: String::from(code),
        date: String::from(date),
        description: String::from(description),
        by: String::from(by),
    }
}

// One sheet, its plan filling the tiling area at 1:500 over the line: the
// window is 192.5 x 125 m about (50, 0).
fn clean_set() -> SheetSet {
    SheetSet {
        defaults: filled_title_block(),
        revisions: vec![revision("A", "25/09/26", "FIRST ISSUE", "BS")],
        sheets: vec![sheet_of(
            "s1",
            "PLAN",
            vec![plan_at("vp1", tiling(), 500.0, Point2::new(50.0, 0.0))],
        )],
        ..SheetSet::default()
    }
}

fn known_logo<'a>() -> PreflightOptions<'a> {
    PreflightOptions {
        logo_readable: Some(true),
        ..PreflightOptions::default()
    }
}

fn check(set: &SheetSet, model: &Model) -> Vec<Finding> {
    check_sheets(set, model, &context(), &known_logo())
}

fn check_with(set: &SheetSet, model: &Model, options: &PreflightOptions<'_>) -> Vec<Finding> {
    check_sheets(set, model, &context(), options)
}

fn with_code(findings: &[Finding], code: &str) -> Vec<Finding> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .cloned()
        .collect()
}

fn codes_of(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn section_of(id: &str, kind: ViewportKind, alignment: &str) -> Viewport {
    let mut viewport = Viewport {
        id: String::from(id),
        kind,
        rect: tiling(),
        scale: 500.0,
        auto_centre: true,
        ..Viewport::default()
    };
    viewport.source.alignment = String::from(alignment);
    viewport.source.stations = vec![50.0];
    viewport
}

// A surface lying in `bounds` whose style "Fine" cannot trace its contours.
fn refused_surface(name: &str, bounds: Box2) -> SurfaceStyleProblem {
    SurfaceStyleProblem {
        problem: SurfaceProblem {
            surface: String::from(name),
            style: String::from("Fine"),
            reason: String::from(
                "contours: an interval of 0.000001 crosses this surface's triangles more than \
                 4000000 times; not drawn - set a larger interval",
            ),
        },
        bounds,
    }
}

fn with_surfaces<'a>(problems: Vec<SurfaceStyleProblem>) -> PreflightOptions<'a> {
    PreflightOptions {
        surface_problems: Some(problems),
        ..known_logo()
    }
}

fn skipping(codes: &[&str]) -> BTreeSet<String> {
    codes.iter().map(|code| String::from(*code)).collect()
}

mod sheet_preflight {
    use super::*;

    #[test]
    fn a_clean_set_passes_without_a_word() {
        let model = drawing();
        let findings = check(&clean_set(), &model);
        assert!(findings.is_empty(), "{}", codes_of(&findings));
    }

    // ---- viewport.outside -------------------------------------------------------------

    #[test]
    fn a_view_under_the_title_block_is_an_error() {
        let model = drawing();
        let mut set = clean_set();
        // Down to 20 mm: 14.625 mm into the title block, whose top is 34.625.
        set.sheets[0].viewports[0].rect = rect(100.0, 20.0, 300.0, 200.0);
        let found = with_code(&check(&set, &model), "viewport.outside");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].sheet_index, Some(0));
        assert_eq!(found[0].sheet_id, "s1");
        assert_eq!(found[0].viewport_id, "vp1");
        assert!(
            found[0].message.contains("14.6 mm under the title block"),
            "{}",
            found[0].message
        );
        assert!(!found[0].fix.is_empty());
    }

    #[test]
    fn a_view_off_the_paper_is_an_error_and_one_in_the_margin_a_warning() {
        let model = drawing();
        let mut set = clean_set();
        // 40 mm past A3's right edge at 420.
        set.sheets[0].viewports[0].rect = rect(300.0, 100.0, 460.0, 200.0);
        let found = with_code(&check(&set, &model), "viewport.outside");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert!(
            found[0].message.contains("40.0 mm off the paper"),
            "{}",
            found[0].message
        );

        set.sheets[0].viewports[0].rect = rect(500.0, 500.0, 600.0, 600.0);
        let found = with_code(&check(&set, &model), "viewport.outside");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].message.contains("is off the paper"),
            "{}",
            found[0].message
        );

        // From 15 mm: 8 mm left of the drawing area at 23, on the paper and
        // clear of the title block.
        set.sheets[0].viewports[0].rect = rect(15.0, 100.0, 200.0, 200.0);
        let found = with_code(&check(&set, &model), "viewport.outside");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert!(
            found[0].message.contains("8.0 mm outside the drawing area"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn a_view_filling_the_drawing_area_exactly_is_inside() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports[0].rect = drawing_area(&set.sheets[0]);
        assert!(with_code(&check(&set, &model), "viewport.outside").is_empty());
        // A frameless sheet's drawing area is the paper less 10 mm.
        set.sheets[0].frame.clear();
        set.sheets[0].viewports[0].rect = rect(10.0, 10.0, 410.0, 287.0);
        assert!(with_code(&check(&set, &model), "viewport.outside").is_empty());
        set.sheets[0].viewports[0].rect = rect(5.0, 10.0, 410.0, 287.0);
        assert_eq!(with_code(&check(&set, &model), "viewport.outside").len(), 1);
    }

    // ---- viewport.overlap -------------------------------------------------------------

    #[test]
    fn overlapping_views_are_reported_on_the_one_in_front() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports = vec![
            plan_at(
                "vp1",
                rect(30.0, 40.0, 220.0, 280.0),
                500.0,
                Point2::new(50.0, 0.0),
            ),
            plan_at(
                "vp2",
                rect(200.0, 40.0, 400.0, 280.0),
                500.0,
                Point2::new(50.0, 0.0),
            ),
        ];
        let found = with_code(&check(&set, &model), "viewport.overlap");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].viewport_id, "vp2");
        assert_eq!(found[0].subject, "vp1");
        assert!(
            found[0].message.contains("20.0 x 240.0 mm"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn views_snapped_edge_to_edge_do_not_overlap() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports = vec![
            plan_at(
                "vp1",
                rect(30.0, 40.0, 215.0, 280.0),
                500.0,
                Point2::new(50.0, 0.0),
            ),
            plan_at(
                "vp2",
                rect(215.0, 40.0, 400.0, 280.0),
                500.0,
                Point2::new(50.0, 0.0),
            ),
        ];
        assert!(with_code(&check(&set, &model), "viewport.overlap").is_empty());
    }

    #[test]
    fn a_legend_set_inside_a_plan_is_an_inset_not_a_mistake() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports.push(panel(
            "vp2",
            ViewportKind::Legend,
            rect(340.0, 40.0, 405.0, 100.0),
            "TEXT",
        ));
        let found = with_code(&check(&set, &model), "viewport.overlap");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Info);
        assert_eq!(found[0].viewport_id, "vp2");
    }

    // ---- viewport.unplaced ------------------------------------------------------------

    #[test]
    fn an_unplaced_view_is_reported_and_checked_no_further() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports.push(plan_at(
            "vp2",
            Box2::default(),
            437.0,
            Point2::new(1e6, 1e6),
        ));
        let findings = check(&set, &model);
        assert_eq!(findings.len(), 1, "{}", codes_of(&findings));
        assert_eq!(findings[0].code, "viewport.unplaced");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].viewport_id, "vp2");
    }

    // ---- plan.empty -------------------------------------------------------------------

    #[test]
    fn a_plan_over_nothing_is_empty() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports[0].centre = Point2::new(5000.0, 5000.0);
        let found = with_code(&check(&set, &model), "plan.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert!(
            found[0].message.contains("192.5 x 125.0 m window at 1:500"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn the_plan_window_turns_with_the_viewport() {
        // A point at (30, 30) and a window 100 x 10 m (100 x 10 mm at 1:1000)
        // about the origin: level, it reaches y = 5 and misses the point;
        // turned 45 degrees it runs along y = x, and the point is 42.4 m along
        // it.
        let mut model = Model::new();
        add(
            &mut model,
            Geometry::Point(PointGeometry {
                position: Point2::new(30.0, 30.0),
            }),
            "0",
        );
        let mut set = clean_set();
        set.sheets[0].viewports = vec![plan_at(
            "vp1",
            rect(100.0, 100.0, 200.0, 110.0),
            1000.0,
            Point2::new(0.0, 0.0),
        )];
        assert_eq!(with_code(&check(&set, &model), "plan.empty").len(), 1);
        set.sheets[0].viewports[0].rotation = PI / 4.0;
        assert!(with_code(&check(&set, &model), "plan.empty").is_empty());
    }

    #[test]
    fn a_line_is_tested_by_itself_not_by_its_box() {
        // The line x + y = 100 from (0, 100) to (100, 0): its box holds the
        // whole 10 x 10 m window about (10, 10), but the line passes 49.5 m
        // from the window's nearest corner.
        let mut model = Model::new();
        add(&mut model, segment(0.0, 100.0, 100.0, 0.0), "0");
        let mut set = clean_set();
        set.sheets[0].viewports = vec![plan_at(
            "vp1",
            rect(100.0, 100.0, 120.0, 120.0),
            500.0,
            Point2::new(10.0, 10.0),
        )];
        assert_eq!(with_code(&check(&set, &model), "plan.empty").len(), 1);
        // A line straight through it, both ends far outside, is seen.
        add(&mut model, segment(-1000.0, 10.0, 1000.0, 10.0), "0");
        assert!(with_code(&check(&set, &model), "plan.empty").is_empty());
    }

    #[test]
    fn a_plan_showing_only_hidden_layers_is_empty() {
        let mut model = Model::new();
        ok(model.layers.add(Layer {
            name: String::from("survey"),
            ..Layer::default()
        }));
        add(&mut model, segment(0.0, 0.0, 100.0, 0.0), "survey");
        let mut set = clean_set();
        assert!(with_code(&check(&set, &model), "plan.empty").is_empty());
        set.sheets[0].viewports[0].hidden_layers.hide("survey");
        assert_eq!(with_code(&check(&set, &model), "plan.empty").len(), 1);
    }

    #[test]
    fn alignments_and_imagery_count_as_shown() {
        let mut model = Model::new();
        let set = clean_set();
        assert_eq!(with_code(&check(&set, &model), "plan.empty").len(), 1);
        // Imagery under the window, from the caller.
        let options = PreflightOptions {
            other_content: vec![rect(0.0, 0.0, 10.0, 10.0)],
            ..known_logo()
        };
        assert!(with_code(&check_with(&set, &model, &options), "plan.empty").is_empty());
        // An alignment through it.
        ok(model.alignments.add(road(-500.0, 20.0, 500.0, 20.0)));
        assert!(with_code(&check(&set, &model), "plan.empty").is_empty());
    }

    #[test]
    fn a_window_inside_a_bare_outline_is_empty_and_inside_a_hatched_one_is_not() {
        // The clean set's window is 192.5 x 125 m about (50, 0). A closed
        // 2 km square and a circle of radius 1 km around it are drawn well
        // outside it: unhatched, the window sees nothing of either.
        let mut model = Model::new();
        let mut boundary = Layer {
            name: String::from("boundary"),
            ..Layer::default()
        };
        ok(model.layers.add(boundary.clone()));
        add(
            &mut model,
            Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(-1000.0, -1000.0),
                    Point2::new(1000.0, -1000.0),
                    Point2::new(1000.0, 1000.0),
                    Point2::new(-1000.0, 1000.0),
                ],
                true,
            )),
            "boundary",
        );
        add(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(50.0, 0.0), 1000.0)),
            "boundary",
        );
        let set = clean_set();
        assert_eq!(with_code(&check(&set, &model), "plan.empty").len(), 1);
        // Hatched, both are fill under the window.
        ok(model.hatch_patterns.add(HatchPattern {
            name: String::from("solid"),
            solid: true,
            ..HatchPattern::default()
        }));
        boundary.hatch_pattern = String::from("solid");
        ok(model.layers.update(&boundary));
        assert!(with_code(&check(&set, &model), "plan.empty").is_empty());
        // A circle whose outline crosses the window is seen, hatched or not.
        let mut ring = Model::new();
        add(
            &mut ring,
            Geometry::Circle(Circle2::new(Point2::new(50.0, 0.0), 80.0)),
            "0",
        );
        assert!(with_code(&check(&set, &ring), "plan.empty").is_empty());
    }

    #[test]
    fn a_dimension_is_seen_by_its_label_not_only_by_what_it_measures() {
        // A dimension of (0, 200) to (50, 200) with its line 10 m below: what
        // it measures is 75 m above the window's top edge (y = 62.5), but its
        // line, arrows and label, drawn about y = 190, are not in it either;
        // one offset 150 m down draws its line at y = 50, inside the window,
        // though the points it measures are not.
        let mut model = Model::new();
        add(
            &mut model,
            dimension(Point2::new(0.0, 200.0), Point2::new(50.0, 200.0), -10.0),
            "0",
        );
        let set = clean_set();
        assert_eq!(with_code(&check(&set, &model), "plan.empty").len(), 1);
        add(
            &mut model,
            dimension(Point2::new(0.0, 200.0), Point2::new(50.0, 200.0), -150.0),
            "0",
        );
        assert!(with_code(&check(&set, &model), "plan.empty").is_empty());
    }

    // ---- text.too-small ---------------------------------------------------------------

    #[test]
    fn text_smaller_than_the_minimum_at_the_plans_scale_is_counted() {
        let mut model = drawing();
        // At 1:500 a 0.5 m text prints 1.0 mm and a 0.8 m one 1.6 mm: both
        // under 1.8 mm. A 1.0 m text prints 2.0 mm. One far outside the window
        // does not count, nor does blank text.
        add(&mut model, text(10.0, 10.0, "A", 0.5), "0");
        add(&mut model, text(20.0, 10.0, "B", 0.8), "0");
        add(&mut model, text(30.0, 10.0, "C", 1.0), "0");
        add(&mut model, text(9000.0, 10.0, "D", 0.1), "0");
        add(&mut model, text(40.0, 10.0, "  ", 0.1), "0");
        let found = with_code(&check(&clean_set(), &model), "text.too-small");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].viewport_id, "vp1");
        assert!(
            found[0]
                .message
                .contains("2 texts printing smaller than 1.8 mm at 1:500, the smallest 1.00 mm"),
            "{}",
            found[0].message
        );
        // 0.5 m reads at 1.8 mm at 1:277.8, so 1:250 is the largest standard
        // scale that holds it; at 1:500 text must be 0.9 m high.
        assert!(found[0].fix.contains("1:250"), "{}", found[0].fix);
        assert!(found[0].fix.contains("0.9 m"), "{}", found[0].fix);
    }

    #[test]
    fn text_that_reads_is_not_counted() {
        let mut model = drawing();
        // 1.8 mm exactly.
        add(&mut model, text(10.0, 10.0, "A", 0.9), "0");
        assert!(with_code(&check(&clean_set(), &model), "text.too-small").is_empty());
        // A smaller minimum lets smaller text through.
        // 1.2 mm.
        add(&mut model, text(10.0, 20.0, "B", 0.6), "0");
        let mut options = known_logo();
        assert_eq!(
            with_code(
                &check_with(&clean_set(), &model, &options),
                "text.too-small"
            )
            .len(),
            1
        );
        options.minimum_text_mm = 1.0;
        assert!(
            with_code(
                &check_with(&clean_set(), &model, &options),
                "text.too-small"
            )
            .is_empty()
        );
    }

    #[test]
    fn dimension_text_is_counted_at_its_styles_height() {
        // The default dimension style's text is 2.5 m high: 1.25 mm at
        // 1:2000.
        let mut model = drawing();
        add(
            &mut model,
            dimension(Point2::new(0.0, 0.0), Point2::new(50.0, 0.0), 5.0),
            "0",
        );
        let mut set = clean_set();
        assert!(with_code(&check(&set, &model), "text.too-small").is_empty());
        set.sheets[0].viewports[0].scale = 2000.0;
        assert_eq!(with_code(&check(&set, &model), "text.too-small").len(), 1);
    }

    // ---- sections ---------------------------------------------------------------------

    #[test]
    fn a_section_of_a_missing_alignment_is_an_error() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports = vec![section_of("vp1", ViewportKind::LongSection, "NOPE")];
        let found = with_code(&check(&set, &model), "section.alignment-missing");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].subject, "NOPE");

        set.sheets[0].viewports = vec![section_of("vp1", ViewportKind::CrossSections, "")];
        let found = with_code(&check(&set, &model), "section.alignment-missing");
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("has no alignment"));

        set.sheets[0].viewports = vec![section_of("vp1", ViewportKind::LongSection, "MC01")];
        let findings = check(&set, &model);
        assert!(findings.is_empty(), "{}", codes_of(&findings));
    }

    #[test]
    fn a_cross_section_off_the_alignment_is_an_error() {
        let model = drawing();
        let mut set = clean_set();
        let mut sections = section_of("vp1", ViewportKind::CrossSections, "MC01");
        sections.source.stations = vec![0.0, 50.0, 100.0, 150.0, -5.0];
        set.sheets[0].viewports = vec![sections];
        let found = with_code(&check(&set, &model), "section.station-outside");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].subject, "MC01");
        assert!(
            found[0].message.contains("2 of its 5 sections"),
            "{}",
            found[0].message
        );
        assert!(
            found[0].message.contains("CH 150.000, CH -5.000"),
            "{}",
            found[0].message
        );
        assert!(
            found[0]
                .message
                .contains("MC01 runs CH 0.000 to CH 100.000"),
            "{}",
            found[0].message
        );

        // Both ends are on it.
        set.sheets[0].viewports[0].source.stations = vec![0.0, 100.0];
        assert!(with_code(&check(&set, &model), "section.station-outside").is_empty());
    }

    #[test]
    fn a_long_section_running_past_the_end_is_a_warning_and_one_wholly_off_an_error() {
        let model = drawing();
        let mut set = clean_set();
        let mut profile = section_of("vp1", ViewportKind::LongSection, "MC01");
        profile.source.chainage_from = 50.0;
        profile.source.chainage_to = 150.0;
        set.sheets[0].viewports = vec![profile];
        let found = with_code(&check(&set, &model), "section.station-outside");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);

        set.sheets[0].viewports[0].source.chainage_from = 200.0;
        set.sheets[0].viewports[0].source.chainage_to = 300.0;
        let found = with_code(&check(&set, &model), "section.station-outside");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);

        set.sheets[0].viewports[0].source.chainage_from = 0.0;
        set.sheets[0].viewports[0].source.chainage_to = 100.0;
        assert!(with_code(&check(&set, &model), "section.station-outside").is_empty());
    }

    #[test]
    fn cross_sections_with_no_chainage_are_an_error() {
        let model = drawing();
        let mut set = clean_set();
        let mut sections = section_of("vp1", ViewportKind::CrossSections, "MC01");
        sections.source.stations.clear();
        set.sheets[0].viewports = vec![sections];
        assert_eq!(
            with_code(&check(&set, &model), "section.no-stations").len(),
            1
        );
        // An interval over a range lists them, as the painter does.
        set.sheets[0].viewports[0].source.section_interval = 20.0;
        set.sheets[0].viewports[0].source.chainage_to = 100.0;
        assert!(with_code(&check(&set, &model), "section.no-stations").is_empty());
    }

    #[test]
    fn a_section_with_no_surface_to_cut_is_an_error_when_the_surfaces_are_known() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports = vec![section_of("vp1", ViewportKind::CrossSections, "MC01")];
        // Not known: not checked.
        assert!(with_code(&check(&set, &model), "section.no-surface").is_empty());
        let mut options = PreflightOptions {
            section_surfaces: Some(0),
            ..known_logo()
        };
        assert_eq!(
            with_code(&check_with(&set, &model, &options), "section.no-surface").len(),
            1
        );
        options.section_surfaces = Some(1);
        assert!(with_code(&check_with(&set, &model, &options), "section.no-surface").is_empty());
    }

    // ---- scales -----------------------------------------------------------------------

    #[test]
    fn a_scale_off_the_ladder_is_a_note() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports[0].scale = 437.0;
        let found = with_code(&check(&set, &model), "scale.non-standard");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Info);
        assert!(found[0].message.contains("1:437"), "{}", found[0].message);
        assert!(found[0].fix.contains("1:500"), "{}", found[0].fix);
        assert!(found[0].fix.contains("1:250"), "{}", found[0].fix);
        // An automatic scale is the painter's to choose.
        set.sheets[0].viewports[0].auto_scale = true;
        assert!(with_code(&check(&set, &model), "scale.non-standard").is_empty());
        // A standard one says nothing.
        set.sheets[0].viewports[0].auto_scale = false;
        set.sheets[0].viewports[0].scale = 1250.0;
        assert!(with_code(&check(&set, &model), "scale.non-standard").is_empty());
    }

    #[test]
    fn a_scale_that_is_not_a_positive_number_is_an_error() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports[0].scale = 0.0;
        assert_eq!(with_code(&check(&set, &model), "scale.invalid").len(), 1);
        // Not also an empty plan: nothing can be said about a window with no
        // size.
        assert!(with_code(&check(&set, &model), "plan.empty").is_empty());
    }

    // ---- the title block --------------------------------------------------------------

    #[test]
    fn a_blank_title_block_value_is_listed_once_for_the_set() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets.push(sheet_of(
            "s2",
            "PLAN 2",
            vec![plan_at("vp2", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        set.sheets.push(sheet_of(
            "s3",
            "PLAN 3",
            vec![plan_at("vp3", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        set.defaults.organisation.clear();
        set.defaults.height_datum = String::from("   ");
        let found = with_code(&check(&set, &model), "field.empty");
        assert_eq!(found.len(), 2);
        // In the Title Block's order: the organisation, then the datum.
        assert_eq!(found[0].subject, "organisation");
        assert_eq!(found[1].subject, "height_datum");
        for finding in &found {
            assert_eq!(finding.severity, Severity::Warning);
            assert!(finding.sheet_index.is_none());
            assert!(finding.sheet_id.is_empty());
            assert!(
                finding.message.contains("every sheet"),
                "{}",
                finding.message
            );
        }
    }

    #[test]
    fn a_sheets_own_value_fills_its_blank_and_the_count_says_so() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets.push(sheet_of(
            "s2",
            "PLAN 2",
            vec![plan_at("vp2", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        set.defaults.surveyor.name.clear();
        set.sheets[0]
            .fields
            .insert(String::from("surveyor_name"), String::from("F. FIELD"));
        let found = with_code(&check(&set, &model), "field.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].subject, "surveyor_name");
        assert!(
            found[0]
                .message
                .contains("Surveyed by is blank in the title block of 1 of 2 sheets"),
            "{}",
            found[0].message
        );
        // A value a sheet sets to nothing is blank on purpose.
        set.sheets[1]
            .fields
            .insert(String::from("surveyor_name"), String::new());
        assert!(with_code(&check(&set, &model), "field.empty").is_empty());
    }

    #[test]
    fn the_project_fills_what_the_set_leaves_and_optional_lines_may_stay_blank() {
        let model = drawing();
        let mut set = clean_set();
        // Project lines 1 and 2 and the coordinate system fall back to the
        // project; lines 3 and 4, the client and the notes may be blank.
        set.defaults.project_lines.clear();
        set.defaults.coordinate_system.clear();
        set.defaults.client.clear();
        set.defaults.notes.clear();
        assert!(with_code(&check(&set, &model), "field.empty").is_empty());
        let bare = FieldContext {
            project_name: String::new(),
            coordinate_system: String::new(),
            file_name: String::new(),
            ..context()
        };
        let found = with_code(
            &check_sheets(&set, &model, &bare, &known_logo()),
            "field.empty",
        );
        let subjects: Vec<&str> = found
            .iter()
            .map(|finding| finding.subject.as_str())
            .collect();
        assert_eq!(
            subjects,
            ["project_line_1", "coordinate_system", "file_name"]
        );
    }

    #[test]
    fn a_frameless_sheet_has_no_title_block_to_leave_blank() {
        let model = drawing();
        let mut set = clean_set();
        set.defaults = SheetDefaults::default();
        set.sheets[0].frame.clear();
        let findings = check(&set, &model);
        assert!(with_code(&findings, "field.empty").is_empty());
        assert!(with_code(&findings, "logo.missing").is_empty());
    }

    #[test]
    fn no_logo_is_a_note_and_an_unreadable_one_a_warning() {
        let model = drawing();
        let mut set = clean_set();
        assert!(with_code(&check(&set, &model), "logo.missing").is_empty());
        set.defaults.logo_asset.clear();
        let found = with_code(&check(&set, &model), "logo.missing");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Info);
        assert!(found[0].sheet_index.is_none());

        set.defaults.logo_asset = String::from("logo.png");
        let mut options = PreflightOptions {
            logo_readable: Some(false),
            ..PreflightOptions::default()
        };
        let found = with_code(&check_with(&set, &model, &options), "logo.unreadable");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].subject, "logo.png");
        // Not known: not reported.
        options.logo_readable = None;
        assert!(check_with(&set, &model, &options).is_empty());
    }

    // ---- sheets -----------------------------------------------------------------------

    #[test]
    fn two_sheets_of_one_name_are_reported_on_the_later() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets.push(sheet_of(
            "s2",
            " plan ",
            vec![plan_at("vp2", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        let found = with_code(&check(&set, &model), "sheet.duplicate-name");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].sheet_index, Some(1));
        assert_eq!(found[0].sheet_id, "s2");
        assert_eq!(found[0].subject, "s1");
        assert!(found[0].viewport_id.is_empty());

        set.sheets[1].name = String::from("PLAN 2");
        assert!(with_code(&check(&set, &model), "sheet.duplicate-name").is_empty());
    }

    #[test]
    fn a_mark_leading_to_a_removed_sheet_dangles() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets.push(sheet_of(
            "s2",
            "PLAN 2",
            vec![plan_at("vp2", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        let to_two = WorldMark {
            kind: WorldMarkKind::MatchLine,
            points: vec![Point2::new(60.0, -10.0), Point2::new(60.0, 10.0)],
            label: String::from("MATCH LINE"),
            sheet: String::from("s2"),
        };
        set.sheets[0].viewports[0].marks = vec![to_two.clone()];
        assert!(with_code(&check(&set, &model), "matchline.dangling").is_empty());

        let to_gone = WorldMark {
            sheet: String::from("s9"),
            ..to_two.clone()
        };
        let outline = WorldMark {
            kind: WorldMarkKind::SheetOutline,
            points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            label: String::new(),
            sheet: String::from("s9"),
        };
        set.sheets[0].viewports[0].marks = vec![to_two, to_gone.clone(), to_gone, outline];
        let found = with_code(&check(&set, &model), "matchline.dangling");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].viewport_id, "vp1");
        assert_eq!(found[0].subject, "s9");
        assert!(
            found[0]
                .message
                .contains("2 match lines and 1 key-plan outline"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn a_portrait_sheet_asking_for_a_frame_is_warned() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].landscape = false;
        set.sheets[0].viewports[0].rect = rect(20.0, 20.0, 280.0, 400.0);
        let found = with_code(&check(&set, &model), "portrait.no-frame");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        // Portrait with no frame asked for is what was meant.
        set.sheets[0].frame.clear();
        assert!(with_code(&check(&set, &model), "portrait.no-frame").is_empty());
    }

    #[test]
    fn a_sheet_with_no_views_is_empty() {
        let model = drawing();
        let mut set = clean_set();
        assert!(with_code(&check(&set, &model), "sheet.empty").is_empty());
        set.sheets.push(sheet_of("s2", "BLANK", Vec::new()));
        let found = with_code(&check(&set, &model), "sheet.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].sheet_id, "s2");
        assert!(found[0].message.contains("empty frame"));
        // A sheet with a view not yet placed has a view: it is unplaced, not
        // empty.
        set.sheets[1].viewports.push(plan_at(
            "vp9",
            Box2::default(),
            500.0,
            Point2::new(50.0, 0.0),
        ));
        let findings = check(&set, &model);
        assert!(
            with_code(&findings, "sheet.empty").is_empty(),
            "{}",
            codes_of(&findings)
        );
        assert_eq!(with_code(&findings, "viewport.unplaced").len(), 1);
    }

    #[test]
    fn an_unknown_frame_is_an_error() {
        let model = drawing();
        let mut set = clean_set();
        assert!(with_code(&check(&set, &model), "frame.unknown").is_empty());
        set.sheets[0].frame = String::from("a1_plan");
        let found = with_code(&check(&set, &model), "frame.unknown");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].subject, "a1_plan");
    }

    #[test]
    fn repeated_ids_are_errors() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets.push(sheet_of(
            "s2",
            "PLAN 2",
            vec![plan_at("vp2", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        assert!(with_code(&check(&set, &model), "sheet.duplicate-id").is_empty());
        assert!(with_code(&check(&set, &model), "viewport.duplicate-id").is_empty());
        set.sheets.pop();
        set.sheets.push(sheet_of(
            "s1",
            "PLAN 2",
            vec![plan_at("vp1", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        let findings = check(&set, &model);
        let sheets = with_code(&findings, "sheet.duplicate-id");
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].sheet_index, Some(1));
        let views = with_code(&findings, "viewport.duplicate-id");
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].sheet_index, Some(1));
        assert_eq!(views[0].subject, "vp1");
    }

    #[test]
    fn a_key_plan_is_checked_as_a_plan_is_but_for_its_text() {
        // A key plan is drawn as a plan: over nothing it is empty, and
        // following an alignment the drawing lacks it is warned. Its outlines
        // are content, and its drawing's small text is expected.
        let mut model = drawing();
        add(&mut model, text(50.0, 10.0, "TINY", 0.1), "0");
        let mut set = clean_set();
        let key = Viewport {
            kind: ViewportKind::KeyPlan,
            ..plan_at(
                "vp9",
                rect(300.0, 200.0, 400.0, 280.0),
                2000.0,
                Point2::new(50.0, 0.0),
            )
        };
        set.sheets.push(sheet_of("s2", "KEY PLAN", vec![key]));
        let findings = check(&set, &model);
        assert!(
            with_code(&findings, "plan.empty").is_empty(),
            "{}",
            codes_of(&findings)
        );
        // The plan on sheet 1 is told about the tiny text; the key plan is
        // not.
        let small = with_code(&findings, "text.too-small");
        assert_eq!(small.len(), 1, "{}", codes_of(&findings));
        assert_eq!(small[0].viewport_id, "vp1");

        // Moved off the drawing it is empty ...
        set.sheets[1].viewports[0].centre = Point2::new(5000.0, 5000.0);
        let found = with_code(&check(&set, &model), "plan.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].viewport_id, "vp9");
        // ... and a sheet outline stored in it does not count: the painter
        // draws the sheets' outlines live and no longer draws those stale
        // copies ...
        set.sheets[1].viewports[0].marks.push(WorldMark {
            kind: WorldMarkKind::SheetOutline,
            sheet: String::from("s1"),
            points: vec![
                Point2::new(4990.0, 4990.0),
                Point2::new(5010.0, 4990.0),
                Point2::new(5010.0, 5010.0),
                Point2::new(4990.0, 5010.0),
            ],
            ..WorldMark::default()
        });
        let on_key = |all: &[Finding]| {
            all.iter()
                .filter(|finding| finding.code == "plan.empty" && finding.viewport_id == "vp9")
                .count()
        };
        assert_eq!(on_key(&check(&set, &model)), 1);
        // ... but the live outline of a sheet's plan there does: it is drawn.
        set.sheets.push(sheet_of(
            "s3",
            "FAR",
            vec![plan_at("vp8", tiling(), 500.0, Point2::new(5000.0, 5000.0))],
        ));
        assert_eq!(on_key(&check(&set, &model)), 0);
        set.sheets.pop();

        set.sheets[1].viewports[0].source.alignment = String::from("GONE");
        let found = with_code(&check(&set, &model), "plan.alignment-missing");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].viewport_id, "vp9");
    }

    #[test]
    fn an_automatic_plan_with_nothing_to_fit_and_no_usable_scale_is_an_error() {
        // Nothing to fit to.
        let model = Model::new();
        let mut set = clean_set();
        set.sheets[0].viewports[0].auto_scale = true;
        assert!(with_code(&check(&set, &model), "scale.invalid").is_empty());
        set.sheets[0].viewports[0].scale = 0.0;
        let found = with_code(&check(&set, &model), "scale.invalid");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].viewport_id, "vp1");
    }

    // ---- panels -----------------------------------------------------------------------

    #[test]
    fn an_image_naming_no_file_or_a_missing_one_is_an_error() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports = vec![panel("vp1", ViewportKind::Image, tiling(), "")];
        assert_eq!(with_code(&check(&set, &model), "image.missing").len(), 1);
        set.sheets[0].viewports[0].text = String::from("site.png");
        // Where the assets are is not known: the name is taken on trust.
        assert!(with_code(&check(&set, &model), "image.missing").is_empty());
        let options = PreflightOptions {
            assets: std::env::temp_dir().join("no-such-assets-folder"),
            ..known_logo()
        };
        assert_eq!(
            with_code(&check_with(&set, &model, &options), "image.missing").len(),
            1
        );
    }

    #[test]
    fn notes_with_no_text_are_a_note() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports = vec![panel("vp1", ViewportKind::Notes, tiling(), " \n")];
        let found = with_code(&check(&set, &model), "notes.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Info);
        set.sheets[0].viewports[0].text = String::from("ALL LEVELS IN METRES");
        assert!(check(&set, &model).is_empty());
    }

    #[test]
    fn a_view_smaller_than_its_kind_reads_at_is_a_note() {
        let model = drawing();
        let mut set = clean_set();
        // A plan reads at 35 x 30 mm at least (layout.rs).
        set.sheets[0].viewports[0].rect = rect(100.0, 100.0, 130.0, 200.0);
        let found = with_code(&check(&set, &model), "viewport.too-small");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Info);
        set.sheets[0].viewports[0].rect = rect(100.0, 100.0, 135.0, 130.0);
        assert!(with_code(&check(&set, &model), "viewport.too-small").is_empty());
    }

    #[test]
    fn a_plan_following_a_missing_alignment_is_warned() {
        let model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports[0].source.alignment = String::from("GONE");
        assert_eq!(
            with_code(&check(&set, &model), "plan.alignment-missing").len(),
            1
        );
        set.sheets[0].viewports[0].source.alignment = String::from("MC01");
        assert!(with_code(&check(&set, &model), "plan.alignment-missing").is_empty());
    }

    // ---- automatic plans --------------------------------------------------------------

    #[test]
    fn an_automatic_plan_is_checked_where_the_painter_would_draw_it() {
        // The line and MC01 span 0..100 x 0..50: on auto, centred on (50, 25),
        // and 100 m with 4% to spare in 385 mm needs 1:270, so 1:500.
        let model = drawing();
        let mut plan = plan_at("vp1", tiling(), 437.0, Point2::new(1e6, 1e6));
        plan.auto_scale = true;
        plan.auto_centre = true;
        let at = plan_window(&plan, &model, &[]);
        assert_eq!(at.scale, 500.0);
        near(at.centre.x, 50.0, 1e-9);
        near(at.centre.y, 25.0, 1e-9);
        let mut set = clean_set();
        set.sheets[0].viewports = vec![plan];
        let findings = check(&set, &model);
        assert!(findings.is_empty(), "{}", codes_of(&findings));
        // The caller's rule wins where it has one (the painter's, in the
        // editor).
        let far_west = |_: &Viewport| PlanWindow {
            scale: 500.0,
            centre: Point2::new(-9000.0, 0.0),
        };
        let options = PreflightOptions {
            resolve_plan: Some(&far_west),
            ..known_logo()
        };
        assert_eq!(
            with_code(&check_with(&set, &model, &options), "plan.empty").len(),
            1
        );
    }

    #[test]
    fn the_windows_corners_turn_about_its_centre() {
        // 100 x 50 mm at 1:1000 is 100 x 50 m; turned 90 degrees, the paper's
        // left-to-right runs north.
        let mut plan = plan_at(
            "vp1",
            rect(0.0, 0.0, 100.0, 50.0),
            1000.0,
            Point2::new(10.0, 20.0),
        );
        plan.rotation = PI / 2.0;
        let corners = plan_window_corners(
            &plan,
            &PlanWindow {
                scale: 1000.0,
                centre: Point2::new(10.0, 20.0),
            },
        );
        // Bottom-left on the paper.
        near(corners[0].x, 35.0, 1e-9);
        near(corners[0].y, -30.0, 1e-9);
        // Top-right.
        near(corners[2].x, -15.0, 1e-9);
        near(corners[2].y, 70.0, 1e-9);
    }

    // ---- order, options, catalogue, JSON ----------------------------------------------

    #[test]
    fn findings_come_in_one_order_every_time() {
        let model = drawing();
        let mut set = clean_set();
        set.defaults.organisation.clear();
        set.defaults.logo_asset.clear();
        set.sheets[0].viewports.push(plan_at(
            "vp2",
            rect(100.0, 20.0, 200.0, 100.0),
            437.0,
            Point2::new(50.0, 0.0),
        ));
        set.sheets.push(sheet_of("s2", "PLAN", Vec::new()));
        let first = check(&set, &model);
        // The set's, then sheet 1's viewports back to front, each viewport's
        // in the catalogue's order, then sheet 2's own.
        assert_eq!(
            codes_of(&first),
            "field.empty logo.missing viewport.outside viewport.overlap scale.non-standard \
             sheet.duplicate-name sheet.empty"
        );
        for _ in 0..3 {
            assert_eq!(check(&set, &model), first);
        }
    }

    #[test]
    fn only_the_sheets_asked_for_are_checked() {
        let model = drawing();
        let mut set = clean_set();
        set.defaults.organisation.clear();
        set.sheets.push(sheet_of("s2", "EMPTY", Vec::new()));
        set.sheets.push(sheet_of("s3", "ALSO EMPTY", Vec::new()));
        let options = PreflightOptions {
            sheets: vec![2, 2, 7],
            ..known_logo()
        };
        let findings = check_with(&set, &model, &options);
        assert_eq!(findings.len(), 2, "{}", codes_of(&findings));
        assert_eq!(findings[0].code, "field.empty");
        assert!(
            findings[0].message.contains("of the sheet"),
            "{}",
            findings[0].message
        );
        assert_eq!(findings[1].code, "sheet.empty");
        assert_eq!(findings[1].sheet_id, "s3");
    }

    #[test]
    fn a_skipped_code_is_not_reported() {
        let model = drawing();
        let mut set = clean_set();
        set.defaults.logo_asset.clear();
        set.sheets[0].viewports[0].scale = 437.0;
        let options = PreflightOptions {
            skip: skipping(&["logo.missing"]),
            ..known_logo()
        };
        assert_eq!(
            codes_of(&check_with(&set, &model, &options)),
            "scale.non-standard"
        );
    }

    // ---- plan.surface-style -----------------------------------------------------------

    #[test]
    fn a_plan_whose_window_reaches_a_surface_its_style_cannot_draw_in_whole_is_warned_and_names_it()
    {
        let model = drawing();
        // The clean plan's window is 192.5 x 125 m about (50, 0): x from
        // -46.25 to 146.25, y from -62.5 to 62.5. The surface lies 0..100 x
        // 0..100, so the window reaches the lower part of it.
        let options = with_surfaces(vec![refused_surface(
            "ground",
            rect(0.0, 0.0, 100.0, 100.0),
        )]);
        let found = with_code(
            &check_with(&clean_set(), &model, &options),
            "plan.surface-style",
        );
        assert_eq!(found.len(), 1, "{}", codes_of(&found));
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].sheet_index, Some(0));
        assert_eq!(found[0].sheet_id, "s1");
        assert_eq!(found[0].viewport_id, "vp1");
        assert_eq!(found[0].subject, "ground");
        // The surface, its style and the reason, as the painter reports them.
        assert!(
            found[0].message.contains(
                "the surface ground (style Fine) without part of its style: contours: an \
                 interval of 0.000001"
            ),
            "{}",
            found[0].message
        );
        // The fix names the two verbs that mend it, with the style and the
        // surface in them.
        assert!(
            found[0].fix.contains("SURFACESTYLE SET \"Fine\" interval="),
            "{}",
            found[0].fix
        );
        assert!(
            found[0].fix.contains("SURFACE STYLE \"ground\" <style>"),
            "{}",
            found[0].fix
        );
    }

    #[test]
    fn a_surface_the_plans_window_does_not_reach_is_not_its_warning_nor_is_anything_when_nothing_is_known()
     {
        let model = drawing();
        // 150 starts 3.75 m past the window's east edge (146.25): not reached.
        let east = with_surfaces(vec![refused_surface("east", rect(150.0, 0.0, 250.0, 50.0))]);
        assert!(
            with_code(
                &check_with(&clean_set(), &model, &east),
                "plan.surface-style"
            )
            .is_empty()
        );
        // 140 starts 6.25 m inside it: reached, if only by a corner.
        let corner = with_surfaces(vec![refused_surface(
            "corner",
            rect(140.0, 0.0, 250.0, 50.0),
        )]);
        assert_eq!(
            with_code(
                &check_with(&clean_set(), &model, &corner),
                "plan.surface-style"
            )
            .len(),
            1
        );
        // No problem known, none found: unknown (not asked) and known empty
        // alike.
        assert!(with_code(&check(&clean_set(), &model), "plan.surface-style").is_empty());
        assert!(
            with_code(
                &check_with(&clean_set(), &model, &with_surfaces(Vec::new())),
                "plan.surface-style"
            )
            .is_empty()
        );
        // And a caller who does not want it can skip the code.
        let skipped = PreflightOptions {
            skip: skipping(&["plan.surface-style"]),
            ..with_surfaces(vec![refused_surface(
                "ground",
                rect(0.0, 0.0, 100.0, 100.0),
            )])
        };
        assert!(
            with_code(
                &check_with(&clean_set(), &model, &skipped),
                "plan.surface-style"
            )
            .is_empty()
        );
    }

    #[test]
    fn each_plan_that_shows_a_refused_surface_is_warned_once_for_each_surface_in_the_order_given() {
        let model = drawing();
        let mut set = clean_set();
        // s2 is a plan far from everything (it also shows nothing: plan.empty,
        // which is not this test's); s3 is a plan over the same ground as s1.
        set.sheets.push(sheet_of(
            "s2",
            "FAR",
            vec![plan_at("vp2", tiling(), 500.0, Point2::new(5000.0, 5000.0))],
        ));
        set.sheets.push(sheet_of(
            "s3",
            "NEAR",
            vec![plan_at("vp3", tiling(), 500.0, Point2::new(50.0, 0.0))],
        ));
        let options = with_surfaces(vec![
            refused_surface("first", rect(0.0, 0.0, 10.0, 10.0)),
            refused_surface("second", rect(20.0, 0.0, 30.0, 10.0)),
        ]);
        let found = with_code(&check_with(&set, &model, &options), "plan.surface-style");
        // Sheet by sheet; on each plan, the surfaces in the order the store
        // has them.
        assert_eq!(found.len(), 4, "{}", codes_of(&found));
        assert_eq!(found[0].viewport_id, "vp1");
        assert_eq!(found[0].subject, "first");
        assert_eq!(found[1].viewport_id, "vp1");
        assert_eq!(found[1].subject, "second");
        assert_eq!(found[2].viewport_id, "vp3");
        assert_eq!(found[2].subject, "first");
        assert_eq!(found[3].viewport_id, "vp3");
        assert_eq!(found[3].subject, "second");
    }

    #[test]
    fn the_catalogue_names_every_code_once_and_their_severities() {
        let mut codes: BTreeSet<&str> = BTreeSet::new();
        for description in preflight_checks() {
            assert!(codes.insert(description.code), "{}", description.code);
            assert!(!description.looks_for.is_empty(), "{}", description.code);
        }
        for code in [
            "viewport.outside",
            "viewport.overlap",
            "plan.empty",
            "section.alignment-missing",
            "section.station-outside",
            "scale.non-standard",
            "field.empty",
            "logo.missing",
            "sheet.duplicate-name",
            "matchline.dangling",
            "text.too-small",
            "portrait.no-frame",
            "viewport.unplaced",
            "sheet.empty",
            "plan.surface-style",
        ] {
            assert!(codes.contains(code), "{code}");
        }
        assert_eq!(Severity::Error.name(), "error");
        assert_eq!(severity_from("info"), Some(Severity::Info));
        assert!(severity_from("fatal").is_none());
    }

    #[test]
    fn the_summary_counts_each_severity() {
        let finding = |severity: Severity, code: &str| Finding {
            severity,
            code: String::from(code),
            ..Finding::default()
        };
        let findings = [
            finding(Severity::Error, "a"),
            finding(Severity::Error, "b"),
            finding(Severity::Warning, "c"),
            finding(Severity::Info, "d"),
        ];
        assert_eq!(
            summarize(&findings),
            PreflightSummary {
                errors: 2,
                warnings: 1,
                infos: 1
            }
        );
        assert_eq!(
            summary_text(&summarize(&findings)),
            "2 errors, 1 warning, 1 note"
        );
        assert_eq!(
            summary_text(&PreflightSummary::default()),
            "no problems found"
        );
    }

    #[test]
    fn a_finding_reads_as_one_line() {
        let finding = Finding {
            severity: Severity::Error,
            code: String::from("viewport.outside"),
            sheet_index: Some(1),
            sheet_id: String::from("s2"),
            viewport_id: String::from("vp3"),
            subject: String::new(),
            message: String::from("PLAN 1:500 (vp3) runs 14.6 mm under the title block"),
            fix: String::from("Drag it back"),
        };
        assert_eq!(
            finding_line(&finding),
            "ERROR viewport.outside [s2/vp3] PLAN 1:500 (vp3) runs 14.6 mm under the title \
             block. Fix: Drag it back."
        );
        let set_wide = Finding {
            severity: Severity::Info,
            code: String::from("logo.missing"),
            message: String::from("No logo"),
            ..Finding::default()
        };
        assert_eq!(finding_line(&set_wide), "INFO logo.missing No logo.");
    }

    #[test]
    fn findings_round_trip_through_json() {
        let model = drawing();
        let mut set = clean_set();
        set.defaults.organisation.clear();
        set.defaults.logo_asset.clear();
        set.sheets[0].viewports.push(plan_at(
            "vp2",
            rect(100.0, 20.0, 200.0, 100.0),
            437.0,
            Point2::new(50.0, 0.0),
        ));
        let findings = check(&set, &model);
        assert!(!findings.is_empty());
        let json = findings_to_json(&findings);
        assert!(json.contains("\"format\":\"katana-sheet-checks\""));
        assert!(
            json.contains("\"summary\":{\"errors\":1,\"info\":2,\"warnings\":2}"),
            "{json}"
        );
        // A set-wide finding has no sheet: the member is left out, not null.
        assert!(!json.contains("null"), "{json}");
        let back = ok(findings_from_json(&json));
        assert_eq!(back, findings);

        assert!(
            findings_from_json(
                "{\"format\":\"katana-sheet-checks\",\"version\":2,\"findings\":[]}"
            )
            .is_err()
        );
        assert!(findings_from_json("[1, 2]").is_err());
        assert!(
            findings_from_json(
                "{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":[{\"severity\":\"fatal\",\"code\":\"x\"}]}"
            )
            .is_err()
        );
        assert!(
            findings_from_json(
                "{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":{}}"
            )
            .is_err()
        );
        // A position is a count: -1 is not read as the largest one there is.
        assert!(
            findings_from_json(
                "{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":[{\"severity\":\"error\",\"code\":\"x\",\"sheet_index\":-1}]}"
            )
            .is_err()
        );
        assert!(
            findings_from_json(
                "{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":[{\"severity\":\"error\",\"code\":\"x\",\"sheet_index\":1.5}]}"
            )
            .is_err()
        );
        let empty = ok(findings_from_json(&findings_to_json(&[])));
        assert!(empty.is_empty());
    }

    #[test]
    fn a_plot_of_some_sheets_is_warned_about_them_and_the_set() {
        let model = drawing();
        let mut set = clean_set();
        set.defaults.logo_asset.clear();
        set.sheets.push(sheet_of("s2", "EMPTY", Vec::new()));
        set.sheets.push(sheet_of("s3", "ALSO EMPTY", Vec::new()));
        let findings = check(&set, &model);
        assert_eq!(codes_of(&findings), "logo.missing sheet.empty sheet.empty");

        let on_third = findings_on_sheets(&findings, &[2]);
        assert_eq!(on_third.len(), 2);
        // The set's: on every sheet.
        assert_eq!(on_third[0].code, "logo.missing");
        assert_eq!(on_third[1].sheet_id, "s3");
        assert_eq!(
            codes_of(&findings_on_sheets(&findings, &[0])),
            "logo.missing"
        );
        assert_eq!(findings_on_sheets(&findings, &[]), findings);
    }

    #[test]
    fn the_spatial_index_finds_what_a_scan_of_the_drawing_finds() {
        // A 1 km square of lines and small texts, and plans over parts of it:
        // level and turned, over lines, over nothing, at scales where the text
        // is too small and where it reads. A plan's window is a few percent of
        // the drawing, so the index answers, and must answer as the scan does.
        let mut document = Document::new(None);
        let mut entities = Vec::new();
        for i in 0..40 {
            let x = f64::from(i) * 25.0;
            entities.push(Entity {
                geometry: segment(x, 0.0, x + 10.0, 1000.0),
                layer: String::from("0"),
                ..Entity::default()
            });
            entities.push(Entity {
                geometry: Geometry::Text(TextGeometry {
                    position: Point2::new(x + 3.0, x),
                    text: String::from("T"),
                    height: 0.2 + 0.05 * f64::from(i),
                    rotation: 0.3,
                    ..TextGeometry::default()
                }),
                layer: String::from("0"),
                ..Entity::default()
            });
        }
        ok(document.execute(create_entities(entities)));
        let mut set = clean_set();
        set.sheets[0].viewports = vec![
            plan_at(
                "vp1",
                rect(30.0, 40.0, 215.0, 280.0),
                500.0,
                Point2::new(300.0, 300.0),
            ),
            plan_at(
                "vp2",
                rect(218.0, 160.0, 405.0, 280.0),
                200.0,
                Point2::new(612.0, 612.0),
            ),
            plan_at(
                "vp3",
                rect(218.0, 40.0, 405.0, 157.0),
                1000.0,
                Point2::new(5000.0, 5000.0),
            ),
        ];
        set.sheets[0].viewports[1].rotation = 0.7;
        let model = document.model();
        let scan = known_logo();
        let indexed = PreflightOptions {
            index: Some(document.spatial_index()),
            ..known_logo()
        };
        let by_index = check_with(&set, model, &indexed);
        assert_eq!(by_index, check_with(&set, model, &scan));
        // And there is something to agree about: the far plan is empty; the
        // first shows 0.75 to 0.85 m texts, under the 0.9 m that reads at
        // 1:500; the turned one shows a 1.4 m text, 7 mm at 1:200.
        let empty = with_code(&by_index, "plan.empty");
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].viewport_id, "vp3");
        let small = with_code(&by_index, "text.too-small");
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].viewport_id, "vp1");
        assert!(small[0].message.contains("3 texts"), "{}", small[0].message);
    }

    #[test]
    fn a_documents_sheets_are_checked_with_its_own_drawing_and_fields() {
        let mut document = Document::new(None);
        let set = clean_set();
        ok(add_sheets(&mut document, set.sheets, ADD_SHEETS));
        ok(edit_sheet_set(
            &mut document,
            |s| {
                s.defaults = filled_title_block();
                s.defaults.logo_asset.clear();
                s.revisions.push(revision("A", "", "", ""));
                Ok(())
            },
            "EDIT_SHEETS",
        ));
        let findings = check_document_sheets(&document, PreflightOptions::default());
        // A new drawing: no coordinate system to fall back on, no logo, and a
        // plan over nothing.
        let codes: String = findings
            .iter()
            .map(|finding| {
                if finding.subject.is_empty() {
                    format!("{} ", finding.code)
                } else {
                    format!("{}:{} ", finding.code, finding.subject)
                }
            })
            .collect();
        assert_eq!(
            codes,
            "field.empty:coordinate_system logo.missing plan.empty "
        );
    }

    // ---- what the other sheet features add --------------------------------------------

    #[test]
    fn a_plans_grid_is_checked_as_the_painter_works_it_out() {
        let model = drawing();
        let mut set = clean_set();
        // Automatic: about 50 mm apart on the paper, lines in the window.
        set.sheets[0].viewports[0].grid_style = GridStyle::Ticks;
        assert!(with_code(&check(&set, &model), "grid.invalid").is_empty());
        assert!(with_code(&check(&set, &model), "grid.empty").is_empty());
        // Every 0.5 m at 1:500 is a line every millimetre: too dense to draw.
        set.sheets[0].viewports[0].grid_interval = 0.5;
        let found = with_code(&check(&set, &model), "grid.invalid");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert_eq!(found[0].viewport_id, "vp1");
        // Every 100 km: no line crosses a 192.5 x 125 m window about
        // (5000, 5000).
        set.sheets[0].viewports[0].grid_interval = 100000.0;
        set.sheets[0].viewports[0].centre = Point2::new(5000.0, 5000.0);
        let found = with_code(&check(&set, &model), "grid.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Info);
    }

    #[test]
    fn a_legend_that_lists_nothing_or_too_much_is_reported() {
        let mut model = drawing();
        let mut set = clean_set();
        set.sheets[0].viewports[0].rect = rect(24.0, 36.0, 300.0, 286.0);
        set.sheets[0].viewports.push(panel(
            "vp2",
            ViewportKind::Legend,
            rect(305.0, 200.0, 405.0, 280.0),
            "",
        ));
        assert!(
            with_code(&check(&set, &model), "legend.empty").is_empty(),
            "the line is listed"
        );
        assert!(with_code(&check(&set, &model), "legend.overflow").is_empty());

        // Many layers, each drawn in the plan, in a legend a row high.
        for i in 0..40 {
            let name = format!("LAYER {i}");
            ok(model.layers.add(Layer {
                name: name.clone(),
                ..Layer::default()
            }));
            let x = f64::from(i);
            add(&mut model, segment(x, 5.0, x, 10.0), &name);
        }
        set.sheets[0].viewports[1].rect = rect(305.0, 260.0, 345.0, 280.0);
        let found = with_code(&check(&set, &model), "legend.overflow");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Warning);
        assert_eq!(found[0].viewport_id, "vp2");

        // Over nothing, its plan shows nothing and it lists nothing.
        set.sheets[0].viewports[0].centre = Point2::new(9000.0, 9000.0);
        let found = with_code(&check(&set, &model), "legend.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].subject, "this_sheet");
    }

    #[test]
    fn tables_that_leave_rows_out_and_an_empty_revision_table_are_reported() {
        let model = drawing();
        let mut set = clean_set();
        let index = panel(
            "vp2",
            ViewportKind::SheetIndex,
            rect(300.0, 240.0, 400.0, 250.0),
            "",
        );
        let revisions = panel(
            "vp3",
            ViewportKind::Revisions,
            rect(300.0, 200.0, 400.0, 230.0),
            "",
        );
        set.sheets[0].viewports[0].rect = rect(24.0, 36.0, 290.0, 286.0);
        set.sheets[0].viewports.push(index);
        set.sheets[0].viewports.push(revisions);
        for i in 2..=30 {
            set.sheets.push(sheet_of(
                &format!("s{i}"),
                &format!("SHEET {i}"),
                Vec::new(),
            ));
        }
        let only = PreflightOptions {
            sheets: vec![0],
            ..known_logo()
        };
        let found = with_code(&check_with(&set, &model, &only), "table.overflow");
        assert_eq!(
            found.len(),
            1,
            "a register 10 mm high cannot list 30 sheets"
        );
        assert_eq!(found[0].viewport_id, "vp2");
        assert!(with_code(&check_with(&set, &model, &only), "revisions.empty").is_empty());

        set.revisions.clear();
        let found = with_code(&check_with(&set, &model, &only), "revisions.empty");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].viewport_id, "vp3");
    }

    #[test]
    fn a_page_setup_the_plot_would_refuse_is_an_error() {
        let model = drawing();
        let mut set = clean_set();
        assert!(with_code(&check(&set, &model), "pagesetup.invalid").is_empty());
        set.page_setup.dpi = 5.0;
        let found = with_code(&check(&set, &model), "pagesetup.invalid");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, Severity::Error);
        assert!(found[0].sheet_index.is_none(), "the set's own");
        set.page_setup.dpi = 300.0;
        set.page_setup.file_name_pattern = String::from("{nope}");
        assert_eq!(
            with_code(&check(&set, &model), "pagesetup.invalid").len(),
            1
        );
    }
}
