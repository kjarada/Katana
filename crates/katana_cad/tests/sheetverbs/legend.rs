// The smart legend (include/katana/cad/plotting/legend.hpp): what a sheet's
// plans show, grouped by what prints, labelled and ordered for a reader; its
// scope stored with the viewport; and the panel's flow into columns;
// tests/cad/plotting/test_legend.cpp.
//
// Every window below is worked out by hand from the viewport: a rectangle of
// W x H mm at 1 : S shows W x S / 1000 by H x S / 1000 metres about its
// centre, turned by its rotation.

use crate::{near, ok, refused};
use katana_cad::Document;
use katana_cad::plotting::legend::{
    Legend, LegendKind, LegendMetrics, LegendOptions, compute_legend, fitted_plan_window,
    layout_legend, legend_for, legend_json, set_legend_scope,
};
use katana_cad::plotting::sheet_commands::{ADD_SHEETS, add_sheets};
use katana_cad::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use katana_cad::plotting::sheet_set::legend_scope_from;
use katana_cad::plotting::{LegendScope, PlanWindow, Sheet, SheetSet, Viewport, ViewportKind};
use katana_commands::{create_entities, create_layer};
use katana_core::ErrorCode;
use katana_entity::{
    Color, DimensionGeometry, Entity, EntityId, EnumName, Geometry, HatchPattern, Layer, Model,
    PointGeometry, PropertyValue, Style, SurveyMap, SurveyRule, TextGeometry,
};
use katana_geometry::{Arc2, Box2, Circle2, Point2, Polyline2, Segment2};
use katana_math::{DEG_TO_RAD, HALF_PI, TWO_PI};

const RED: Color = Color {
    r: 220,
    g: 0,
    b: 0,
    a: 255,
};
const BLUE: Color = Color {
    r: 0,
    g: 0,
    b: 200,
    a: 255,
};

fn add_layer(model: &mut Model, name: &str, colour: Color, visible: bool) {
    let layer = Layer {
        name: String::from(name),
        color: colour,
        visible,
        ..Layer::default()
    };
    ok(model.layers.add(layer));
}

fn add_plain_layer(model: &mut Model, name: &str) {
    add_layer(model, name, Color::default(), true);
}

fn add_style(model: &mut Model, style: Style) {
    ok(model.styles.add(style));
}

fn add(model: &mut Model, geometry: Geometry, layer: &str, style: &str) -> EntityId {
    let entity = Entity {
        geometry,
        layer: String::from(layer),
        style: String::from(style),
        ..Entity::default()
    };
    ok(model.entities.add(entity))
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
    Geometry::Line(Segment2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

fn point(x: f64, y: f64) -> Geometry {
    Geometry::Point(PointGeometry {
        position: Point2::new(x, y),
    })
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

// A plan of W x H mm at 1 : scale about `centre`, turned by `degrees`.
fn plan(id: &str, w: f64, h: f64, scale: f64, centre: Point2, degrees: f64) -> Viewport {
    Viewport {
        id: String::from(id),
        kind: ViewportKind::Plan,
        rect: Box2::new(Point2::new(30.0, 40.0), Point2::new(30.0 + w, 40.0 + h)),
        scale,
        centre,
        rotation: degrees * DEG_TO_RAD,
        ..Viewport::default()
    }
}

fn legend_panel(id: &str) -> Viewport {
    Viewport {
        id: String::from(id),
        kind: ViewportKind::Legend,
        rect: Box2::new(Point2::new(300.0, 40.0), Point2::new(400.0, 140.0)),
        ..Viewport::default()
    }
}

fn one_sheet(viewports: Vec<Viewport>) -> SheetSet {
    SheetSet {
        sheets: vec![Sheet {
            id: String::from("s1"),
            viewports,
            ..Sheet::default()
        }],
        ..SheetSet::default()
    }
}

fn labels(legend: &Legend) -> Vec<String> {
    legend
        .entries
        .iter()
        .map(|entry| entry.label.clone())
        .collect()
}

fn legend_of(
    model: &Model,
    set: &SheetSet,
    sheet: usize,
    scope: LegendScope,
    options: &LegendOptions<'_>,
) -> Legend {
    ok(compute_legend(model, set, sheet, scope, options))
}

fn this_sheet(model: &Model, set: &SheetSet) -> Legend {
    legend_of(
        model,
        set,
        0,
        LegendScope::ThisSheet,
        &LegendOptions::default(),
    )
}

fn hatch(name: &str) -> HatchPattern {
    HatchPattern {
        name: String::from(name),
        description: String::new(),
        solid: true,
        families: Vec::new(),
    }
}

fn rule(key: &str, comment: &str) -> SurveyRule {
    SurveyRule {
        key: String::from(key),
        comment: String::from(comment),
        ..SurveyRule::default()
    }
}

mod sheet_legend {
    use super::*;

    // ---- what the plans show --------------------------------------------------------

    #[test]
    fn lists_only_what_the_plans_window_shows() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "ROAD");
        add_plain_layer(&mut model, "FENCE");
        add_plain_layer(&mut model, "FAR");
        // 100 x 50 mm at 1:1000 about the origin: x -50..50, y -25..25.
        add(&mut model, line(-10.0, 0.0, 10.0, 0.0), "ROAD", "");
        // Crosses the right edge at (50, 21).
        add(&mut model, line(40.0, 20.0, 60.0, 22.0), "FENCE", "");
        add(&mut model, line(200.0, 0.0, 300.0, 0.0), "FAR", "");
        // Half a metre above the top edge.
        add(&mut model, line(-20.0, 25.5, 20.0, 25.5), "FAR", "");
        let set = one_sheet(vec![
            plan("vp1", 100.0, 50.0, 1000.0, Point2::new(0.0, 0.0), 0.0),
            legend_panel("vpL"),
        ]);
        let legend = this_sheet(&model, &set);
        assert_eq!(labels(&legend), ["FENCE", "ROAD"]);
        assert_eq!(legend.scope, LegendScope::ThisSheet);
        assert_eq!(legend.windows, 1);
        assert_eq!(legend.scale, 1000.0);
    }

    #[test]
    fn a_turned_viewport_shows_its_rotated_rectangle_not_the_box_around_it() {
        let mut model = Model::new();
        for name in ["ON", "CORNER", "ACROSS", "RING", "CURVE"] {
            add_plain_layer(&mut model, name);
        }
        // A strip 100 x 10 mm at 1:1000 turned 45 degrees about the origin:
        // 100 m along the diagonal y = x and 10 m across it. The box around it
        // reaches (+-38.9, +-38.9).
        let set = one_sheet(vec![
            plan("vp1", 100.0, 10.0, 1000.0, Point2::new(0.0, 0.0), 45.0),
            legend_panel("vpL"),
        ]);
        // On the strip, 42 m along it.
        add(&mut model, point(30.0, 30.0), "ON", "");
        // In the box but 28 m off the strip's middle line: 40 cos 45 along,
        // 40 sin 45 across.
        add(&mut model, point(40.0, 0.0), "CORNER", "");
        // A line whose own box sits inside the strip's box, and which never
        // meets the strip.
        add(&mut model, line(30.0, 0.0, 38.0, 0.0), "CORNER", "");
        // A line square across the strip's middle.
        add(&mut model, line(-20.0, 20.0, 20.0, -20.0), "ACROSS", "");
        // A circle round the strip, and one in the empty corner of its box.
        add(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), 30.0)),
            "RING",
            "",
        );
        add(
            &mut model,
            Geometry::Circle(Circle2::new(Point2::new(30.0, -30.0), 5.0)),
            "CORNER",
            "",
        );
        // An arc through the strip, and one bowing away from it in the
        // corner.
        add(
            &mut model,
            Geometry::Arc(Arc2::new(Point2::new(0.0, 0.0), 20.0, 0.0, HALF_PI)),
            "CURVE",
            "",
        );
        add(
            &mut model,
            Geometry::Arc(Arc2::new(Point2::new(35.0, -35.0), 6.0, 0.0, TWO_PI * 0.9)),
            "CORNER",
            "",
        );
        let legend = this_sheet(&model, &set);
        assert_eq!(labels(&legend), ["ON", "ACROSS", "CURVE", "RING"]);
    }

    #[test]
    fn hidden_layers_and_hidden_entities_are_not_listed() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "SHOWN");
        // Hidden in the document.
        add_layer(&mut model, "OFF", Color::default(), false);
        // Hidden in the viewport.
        add_plain_layer(&mut model, "HIDDEN_HERE");
        // A parent hidden in the viewport ... hides its child.
        add_plain_layer(&mut model, "SURVEY");
        add_plain_layer(&mut model, "SURVEY/TREES");
        add(&mut model, line(0.0, 0.0, 1.0, 0.0), "SHOWN", "");
        add(&mut model, line(0.0, 1.0, 1.0, 1.0), "OFF", "");
        add(&mut model, line(0.0, 2.0, 1.0, 2.0), "HIDDEN_HERE", "");
        add(&mut model, point(0.0, 3.0), "SURVEY/TREES", "");
        let invisible = Entity {
            geometry: line(0.0, 4.0, 1.0, 4.0),
            layer: String::from("SHOWN"),
            visible: false,
            ..Entity::default()
        };
        ok(model.entities.add(invisible.clone()));
        // Its layer has nothing else to list.
        let invisible_only = Entity {
            layer: String::from("SURVEY"),
            ..invisible
        };
        ok(model.entities.add(invisible_only));
        let mut view = plan("vp1", 100.0, 100.0, 500.0, Point2::new(0.0, 0.0), 0.0);
        view.hidden_layers.hide("HIDDEN_HERE");
        view.hidden_layers.hide("SURVEY");
        let legend = this_sheet(&model, &one_sheet(vec![view, legend_panel("vpL")]));
        assert_eq!(labels(&legend), ["SHOWN"]);
        // The invisible one is not counted.
        assert_eq!(legend.entries[0].count, 1);
    }

    #[test]
    fn a_hatched_area_covering_the_whole_window_is_shown() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "PARK");
        add_plain_layer(&mut model, "OUTLINE");
        ok(model.hatch_patterns.add(hatch("SOLIDFILL")));
        let mut park = model
            .layers
            .find("PARK")
            .cloned()
            .unwrap_or_else(|| panic!("no layer PARK"));
        park.hatch_pattern = String::from("SOLIDFILL");
        ok(model.layers.update(&park));
        // Both boundaries lie wholly outside a 50 x 50 m window they surround.
        let around = Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(-100.0, -100.0),
                Point2::new(100.0, -100.0),
                Point2::new(100.0, 100.0),
                Point2::new(-100.0, 100.0),
            ],
            true,
        ));
        add(&mut model, around.clone(), "PARK", "");
        // Not hatched: no ink inside the window.
        add(&mut model, around, "OUTLINE", "");
        let legend = this_sheet(
            &model,
            &one_sheet(vec![plan(
                "vp1",
                100.0,
                100.0,
                500.0,
                Point2::new(0.0, 0.0),
                0.0,
            )]),
        );
        assert_eq!(legend.entries.len(), 1);
        assert_eq!(legend.entries[0].label, "PARK");
        assert_eq!(legend.entries[0].kind, LegendKind::Area);
        assert_eq!(legend.entries[0].hatch_pattern, "SOLIDFILL");
    }

    #[test]
    fn a_symbol_reaches_half_its_size_past_its_point() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "TREES");
        add_plain_layer(&mut model, "PEGS");
        add_style(
            &mut model,
            Style {
                name: String::from("Tree"),
                symbol: String::from("circle"),
                // 3 m either side of its point.
                symbol_size: 6.0,
                ..Style::default()
            },
        );
        // A 100 x 100 m window: the edge is at x = 50.
        // Its circle reaches 49.
        add(&mut model, point(52.0, 0.0), "TREES", "Tree");
        // A plain mark does not.
        add(&mut model, point(52.0, 5.0), "PEGS", "");
        let legend = this_sheet(
            &model,
            &one_sheet(vec![plan(
                "vp1",
                100.0,
                100.0,
                1000.0,
                Point2::new(0.0, 0.0),
                0.0,
            )]),
        );
        assert_eq!(labels(&legend), ["Tree"]);
        assert_eq!(legend.entries[0].kind, LegendKind::Symbol);
        assert_eq!(legend.entries[0].symbol, "circle");
        assert_eq!(legend.entries[0].symbol_size, 6.0);
    }

    // ---- grouped by what prints, in order ------------------------------------------

    #[test]
    fn groups_by_style_else_layer_and_by_kind_of_mark() {
        let mut model = Model::new();
        add_layer(&mut model, "BOUNDARY", BLUE, true);
        add_plain_layer(&mut model, "WEST");
        add_plain_layer(&mut model, "EAST");
        add_plain_layer(&mut model, "ANNOTATION");
        add_style(
            &mut model,
            Style {
                name: String::from("Fence"),
                color: Some(RED),
                line_weight: 0.35,
                ..Style::default()
            },
        );
        add_style(
            &mut model,
            Style {
                name: String::from("Pit"),
                symbol: String::from("square"),
                ..Style::default()
            },
        );
        // One style on two layers is one entry.
        add(&mut model, line(0.0, 0.0, 10.0, 0.0), "WEST", "Fence");
        add(&mut model, line(0.0, 5.0, 10.0, 5.0), "EAST", "Fence");
        // A style naming no style of the drawing prints as its layer.
        add(&mut model, line(0.0, 8.0, 10.0, 8.0), "BOUNDARY", "Gone");
        // A point and a line of one layer print differently.
        add(&mut model, point(1.0, 1.0), "BOUNDARY", "");
        add(&mut model, line(0.0, 9.0, 10.0, 9.0), "BOUNDARY", "");
        add(&mut model, point(2.0, 2.0), "WEST", "Pit");
        add(&mut model, text(3.0, 3.0, "LOT 7", 1.0), "ANNOTATION", "");
        // A dimension explains itself.
        add(
            &mut model,
            Geometry::Dimension(DimensionGeometry {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(5.0, 0.0),
                offset: 1.0,
                ..DimensionGeometry::default()
            }),
            "ANNOTATION",
            "",
        );
        let legend = this_sheet(
            &model,
            &one_sheet(vec![plan(
                "vp1",
                100.0,
                100.0,
                500.0,
                Point2::new(5.0, 5.0),
                0.0,
            )]),
        );
        assert_eq!(legend.entries.len(), 5);

        let symbol = &legend.entries[0];
        assert_eq!(symbol.kind, LegendKind::Symbol);
        assert_eq!(symbol.label, "Pit");
        assert_eq!(symbol.style, "Pit");
        assert!(symbol.layer.is_empty());

        let point_entry = &legend.entries[1];
        assert_eq!(point_entry.kind, LegendKind::Point);
        assert_eq!(point_entry.label, "BOUNDARY");
        assert_eq!(point_entry.layer, "BOUNDARY");
        assert!(point_entry.style.is_empty());
        assert_eq!(point_entry.colour, BLUE);

        let boundary = &legend.entries[2];
        assert_eq!(boundary.kind, LegendKind::Line);
        assert_eq!(boundary.label, "BOUNDARY");
        // Its own line and the one naming a missing style.
        assert_eq!(boundary.count, 2);

        let fence_line = &legend.entries[3];
        assert_eq!(fence_line.kind, LegendKind::Line);
        assert_eq!(fence_line.label, "Fence");
        assert_eq!(fence_line.count, 2);
        assert_eq!(fence_line.colour, RED);
        assert_eq!(fence_line.line_weight, 0.35);

        assert_eq!(legend.entries[4].kind, LegendKind::Text);
        assert_eq!(legend.entries[4].label, "ANNOTATION");
    }

    #[test]
    fn entries_are_sorted_symbols_points_lines_areas_texts_then_by_label() {
        let mut model = Model::new();
        ok(model.hatch_patterns.add(hatch("SOLIDFILL")));
        for name in ["cherry", "Banana", "apple", "Zone"] {
            add_plain_layer(&mut model, name);
        }
        let mut zone = model
            .layers
            .find("Zone")
            .cloned()
            .unwrap_or_else(|| panic!("no layer Zone"));
        zone.hatch_pattern = String::from("SOLIDFILL");
        ok(model.layers.update(&zone));
        let square = Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(4.0, 4.0),
                Point2::new(0.0, 4.0),
            ],
            true,
        ));
        add(&mut model, text(1.0, 1.0, "A", 1.0), "apple", "");
        add(&mut model, square, "Zone", "");
        add(&mut model, line(0.0, 0.0, 1.0, 1.0), "cherry", "");
        add(&mut model, line(0.0, 0.0, 1.0, 1.0), "Banana", "");
        add(&mut model, line(0.0, 0.0, 1.0, 1.0), "apple", "");
        add(&mut model, point(1.0, 1.0), "cherry", "");
        add_style(
            &mut model,
            Style {
                name: String::from("mark"),
                symbol: String::from("cross"),
                ..Style::default()
            },
        );
        add(&mut model, point(2.0, 1.0), "Zone", "mark");
        let legend = this_sheet(
            &model,
            &one_sheet(vec![plan(
                "vp1",
                100.0,
                100.0,
                500.0,
                Point2::new(0.0, 0.0),
                0.0,
            )]),
        );
        let order: Vec<(LegendKind, String)> = legend
            .entries
            .iter()
            .map(|entry| (entry.kind, entry.label.clone()))
            .collect();
        let expected = [
            (LegendKind::Symbol, "mark"),
            (LegendKind::Point, "cherry"),
            (LegendKind::Line, "apple"),
            (LegendKind::Line, "Banana"),
            (LegendKind::Line, "cherry"),
            (LegendKind::Area, "Zone"),
            (LegendKind::Text, "apple"),
        ]
        .map(|(kind, label)| (kind, String::from(label)));
        assert_eq!(order, expected);
    }

    #[test]
    fn an_entry_looks_as_most_of_its_entities_do_a_tie_to_the_first() {
        let mut model = Model::new();
        add_layer(&mut model, "KERB", BLUE, true);
        let red = Entity {
            geometry: line(0.0, 0.0, 1.0, 0.0),
            layer: String::from("KERB"),
            color: Some(RED),
            ..Entity::default()
        };
        let by_layer = Entity {
            color: None,
            ..red.clone()
        };
        ok(model.entities.add(by_layer));
        ok(model.entities.add(red.clone()));
        let set = one_sheet(vec![plan(
            "vp1",
            100.0,
            100.0,
            500.0,
            Point2::new(0.0, 0.0),
            0.0,
        )]);
        // One of each: the first, blue by its layer.
        let legend = this_sheet(&model, &set);
        assert_eq!(legend.entries.len(), 1);
        assert_eq!(legend.entries[0].colour, BLUE);
        // Two red to one blue.
        ok(model.entities.add(red));
        let legend = this_sheet(&model, &set);
        assert_eq!(legend.entries.len(), 1);
        assert_eq!(legend.entries[0].colour, RED);
        assert_eq!(legend.entries[0].count, 3);
    }

    // ---- labels from the survey code library --------------------------------------------

    #[test]
    fn a_coded_style_is_labelled_by_its_codes_description() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "SERVICES");
        add_style(
            &mut model,
            Style {
                name: String::from("WATR Main"),
                ..Style::default()
            },
        );
        add_style(
            &mut model,
            Style {
                name: String::from("Plain"),
                ..Style::default()
            },
        );
        let coded = |model: &mut Model, code: &str, style: &str, y: f64| {
            let mut entity = Entity {
                geometry: line(0.0, y, 10.0, y),
                layer: String::from("SERVICES"),
                style: String::from(style),
                ..Entity::default()
            };
            entity.properties.insert(
                String::from("code"),
                PropertyValue::from(String::from(code)),
            );
            ok(model.entities.add(entity));
        };
        coded(&mut model, "WM01", "WATR Main", 0.0);
        coded(&mut model, "WM02", "WATR Main", 1.0);
        // Uncoded: no vote.
        add(
            &mut model,
            line(0.0, 2.0, 10.0, 2.0),
            "SERVICES",
            "WATR Main",
        );
        // Two codes of different descriptions share one appearance, and so
        // one style.
        coded(&mut model, "FE", "Plain", 3.0);
        coded(&mut model, "WA", "Plain", 4.0);
        let mut map = SurveyMap::new();
        let main = SurveyRule {
            linestyle: String::from("WATR Main"),
            ..rule("WM*", "Water main")
        };
        ok(map.add(main));
        ok(map.add(rule("FE", "Fence")));
        ok(map.add(rule("WA", "Wall")));

        let set = one_sheet(vec![plan(
            "vp1",
            100.0,
            100.0,
            500.0,
            Point2::new(0.0, 0.0),
            0.0,
        )]);
        let options = LegendOptions {
            codes: Some(&map),
            ..LegendOptions::default()
        };
        let legend = legend_of(&model, &set, 0, LegendScope::ThisSheet, &options);
        assert_eq!(legend.entries.len(), 2);
        // The descriptions disagree.
        assert_eq!(legend.entries[0].label, "Plain");
        assert!(legend.entries[0].code.is_empty());
        assert_eq!(legend.entries[1].label, "Water main");
        assert_eq!(legend.entries[1].code, "WM01");
        assert_eq!(legend.entries[1].style, "WATR Main");
        assert_eq!(legend.entries[1].count, 3);

        // Without a library, every label is a name.
        let names = this_sheet(&model, &set);
        assert_eq!(labels(&names), ["Plain", "WATR Main"]);

        // A coded entity whose code the library does not describe breaks the
        // vote.
        coded(&mut model, "ZZ9", "WATR Main", 5.0);
        let broken = legend_of(&model, &set, 0, LegendScope::ThisSheet, &options);
        assert_eq!(labels(&broken), ["Plain", "WATR Main"]);
    }

    #[test]
    fn a_points_code_is_looked_up_by_its_string_name() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "SURVEY");
        add_style(
            &mut model,
            Style {
                name: String::from("Pit"),
                symbol: String::from("square"),
                ..Style::default()
            },
        );
        let mut coded = Entity {
            geometry: point(1.0, 1.0),
            layer: String::from("SURVEY"),
            style: String::from("Pit"),
            ..Entity::default()
        };
        // A string name and a linework control.
        coded.properties.insert(
            String::from("code"),
            PropertyValue::from(String::from("SP1 ST")),
        );
        ok(model.entities.add(coded));
        let mut map = SurveyMap::new();
        ok(map.add(rule("SP*", "Stormwater pit")));
        let options = LegendOptions {
            codes: Some(&map),
            ..LegendOptions::default()
        };
        let legend = legend_of(
            &model,
            &one_sheet(vec![plan(
                "vp1",
                100.0,
                100.0,
                500.0,
                Point2::new(0.0, 0.0),
                0.0,
            )]),
            0,
            LegendScope::ThisSheet,
            &options,
        );
        assert_eq!(legend.entries.len(), 1);
        assert_eq!(legend.entries[0].label, "Stormwater pit");
        assert_eq!(legend.entries[0].code, "SP1");
    }

    // ---- scopes -------------------------------------------------------------------------

    #[test]
    fn scopes_this_sheet_whole_set_and_whole_drawing() {
        let mut model = Model::new();
        for name in ["WEST", "EAST", "NOWHERE"] {
            add_plain_layer(&mut model, name);
        }
        add(&mut model, line(-100.0, 0.0, -90.0, 0.0), "WEST", "");
        add(&mut model, line(90.0, 0.0, 100.0, 0.0), "EAST", "");
        add(&mut model, line(0.0, 500.0, 1.0, 500.0), "NOWHERE", "");
        let west = Sheet {
            id: String::from("s1"),
            viewports: vec![
                plan("vp1", 40.0, 40.0, 1000.0, Point2::new(-95.0, 0.0), 0.0),
                legend_panel("vp2"),
            ],
            ..Sheet::default()
        };
        let east = Sheet {
            id: String::from("s2"),
            viewports: vec![plan("vp3", 40.0, 40.0, 500.0, Point2::new(95.0, 0.0), 0.0)],
            ..Sheet::default()
        };
        let set = SheetSet {
            sheets: vec![west, east],
            ..SheetSet::default()
        };
        let options = LegendOptions::default();

        let own = legend_of(&model, &set, 0, LegendScope::ThisSheet, &options);
        assert_eq!(labels(&own), ["WEST"]);
        assert_eq!(own.windows, 1);

        let all = legend_of(&model, &set, 0, LegendScope::WholeSet, &options);
        assert_eq!(labels(&all), ["EAST", "WEST"]);
        assert_eq!(all.scope, LegendScope::WholeSet);
        assert_eq!(all.windows, 2);
        // The plan beside the legend, not the first of the set's.
        assert_eq!(all.scale, 1000.0);

        let drawing = legend_of(&model, &set, 0, LegendScope::WholeDrawing, &options);
        assert_eq!(labels(&drawing), ["EAST", "NOWHERE", "WEST"]);
        assert_eq!(drawing.scope, LegendScope::WholeDrawing);
        assert_eq!(drawing.windows, 0);
    }

    #[test]
    fn a_sheet_without_a_plan_falls_back_to_the_set_and_a_set_without_one_to_the_drawing() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "SEEN");
        add_plain_layer(&mut model, "UNSEEN");
        add(&mut model, line(0.0, 0.0, 1.0, 0.0), "SEEN", "");
        add(&mut model, line(0.0, 900.0, 1.0, 900.0), "UNSEEN", "");
        // Not on the paper: shows nothing.
        let unplaced = Viewport {
            rect: Box2::default(),
            ..plan("vp9", 10.0, 10.0, 500.0, Point2::new(0.0, 900.0), 0.0)
        };
        let legend_sheet = Sheet {
            id: String::from("s1"),
            viewports: vec![legend_panel("vp1"), unplaced],
            ..Sheet::default()
        };
        let plan_sheet = Sheet {
            id: String::from("s2"),
            viewports: vec![plan("vp2", 100.0, 100.0, 250.0, Point2::new(0.0, 0.0), 0.0)],
            ..Sheet::default()
        };
        let mut set = SheetSet {
            sheets: vec![legend_sheet, plan_sheet],
            ..SheetSet::default()
        };

        let from_set = this_sheet(&model, &set);
        assert_eq!(from_set.scope, LegendScope::WholeSet);
        assert_eq!(labels(&from_set), ["SEEN"]);
        assert_eq!(from_set.scale, 250.0);

        set.sheets.pop();
        let from_drawing = this_sheet(&model, &set);
        assert_eq!(from_drawing.scope, LegendScope::WholeDrawing);
        assert_eq!(labels(&from_drawing), ["SEEN", "UNSEEN"]);
        assert_eq!(from_drawing.scale, 0.0);
    }

    #[test]
    fn an_entity_two_plans_show_is_counted_once() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "ROAD");
        add(&mut model, line(-5.0, 0.0, 5.0, 0.0), "ROAD", "");
        add(&mut model, line(-5.0, 1.0, 5.0, 1.0), "ROAD", "");
        let set = one_sheet(vec![
            plan("vp1", 50.0, 50.0, 500.0, Point2::new(0.0, 0.0), 0.0),
            plan("vp2", 50.0, 50.0, 500.0, Point2::new(1.0, 0.0), 30.0),
        ]);
        let legend = this_sheet(&model, &set);
        assert_eq!(legend.entries.len(), 1);
        assert_eq!(legend.entries[0].count, 2);
        assert_eq!(legend.windows, 2);
    }

    #[test]
    fn an_automatic_plan_is_fitted_or_resolved_by_the_caller() {
        let mut model = Model::new();
        add_plain_layer(&mut model, "SITE");
        add(&mut model, line(1000.0, 1000.0, 1100.0, 1050.0), "SITE", "");
        let mut automatic = plan("vp1", 100.0, 100.0, 500.0, Point2::new(0.0, 0.0), 0.0);
        automatic.auto_scale = true;
        automatic.auto_centre = true;
        let set = one_sheet(vec![automatic.clone()]);
        // Fitted: centred on (1050, 1025); 104 m needs 1:1040, so 1:1250.
        let fitted = fitted_plan_window(&model, &automatic);
        near(fitted.centre.x, 1050.0, 1e-9);
        near(fitted.centre.y, 1025.0, 1e-9);
        assert_eq!(fitted.scale, 1250.0);
        let legend = this_sheet(&model, &set);
        assert_eq!(labels(&legend), ["SITE"]);
        assert_eq!(legend.scale, 1250.0);
        // A caller's window is taken as it is: this one looks at the origin.
        let origin = |_: &Viewport| PlanWindow {
            scale: 200.0,
            centre: Point2::new(0.0, 0.0),
        };
        let options = LegendOptions {
            window: Some(&origin),
            ..LegendOptions::default()
        };
        let elsewhere = legend_of(&model, &set, 0, LegendScope::ThisSheet, &options);
        assert!(elsewhere.entries.is_empty());
        assert_eq!(elsewhere.scale, 200.0);
    }

    #[test]
    fn an_index_past_the_end_is_refused() {
        let model = Model::new();
        let error = refused(compute_legend(
            &model,
            &one_sheet(Vec::new()),
            1,
            LegendScope::WholeDrawing,
            &LegendOptions::default(),
        ));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    // ---- through a document ----------------------------------------------------------------

    #[test]
    fn the_documents_legend_uses_its_index_and_codes_and_gives_the_same_answer() {
        let mut document = Document::new(None);
        ok(document.execute(create_layer(Layer {
            name: String::from("PITS"),
            ..Layer::default()
        })));
        ok(document.execute(create_layer(Layer {
            name: String::from("FAR"),
            ..Layer::default()
        })));
        let mut entities = Vec::new();
        for i in 0..400 {
            let mut near_one = Entity {
                geometry: point(f64::from(i % 20), f64::from(i / 20)),
                layer: String::from("PITS"),
                ..Entity::default()
            };
            near_one.properties.insert(
                String::from("code"),
                PropertyValue::from(String::from("SP1")),
            );
            let far = Entity {
                geometry: point(5000.0 + f64::from(i), 5000.0),
                layer: String::from("FAR"),
                ..near_one.clone()
            };
            entities.push(near_one);
            entities.push(far);
        }
        ok(document.execute(create_entities(entities)));
        let mut map = SurveyMap::new();
        ok(map.add(rule("SP*", "Stormwater pit")));
        document.set_survey_map(map);
        ok(add_sheets(
            &mut document,
            one_sheet(vec![
                plan("vp1", 60.0, 60.0, 500.0, Point2::new(10.0, 10.0), 0.0),
                legend_panel("vp2"),
            ])
            .sheets,
            ADD_SHEETS,
        ));
        let legend_id = document.sheet_set().sheets[0].viewports[1].id.clone();

        let legend = ok(legend_for(&document, &legend_id));
        assert_eq!(legend.entries.len(), 1);
        assert_eq!(legend.entries[0].label, "Stormwater pit");
        assert_eq!(legend.entries[0].count, 400);
        // The index narrows the search and changes nothing.
        let scan = LegendOptions {
            codes: Some(document.survey_map()),
            ..LegendOptions::default()
        };
        let scanned = legend_of(
            document.model(),
            &document.sheet_set(),
            0,
            LegendScope::ThisSheet,
            &scan,
        );
        assert_eq!(legend, scanned);
        // And the same question gives the same answer.
        assert_eq!(ok(legend_for(&document, &legend_id)), legend);

        assert_eq!(
            refused(legend_for(&document, "vp999")).code,
            ErrorCode::NotFound
        );
        let plan_id = document.sheet_set().sheets[0].viewports[0].id.clone();
        assert_eq!(
            refused(legend_for(&document, &plan_id)).code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn setting_the_scope_is_one_undoable_step() {
        let mut document = Document::new(None);
        ok(add_sheets(
            &mut document,
            one_sheet(vec![
                plan("vp1", 60.0, 60.0, 500.0, Point2::new(0.0, 0.0), 0.0),
                legend_panel("vp2"),
            ])
            .sheets,
            ADD_SHEETS,
        ));
        let plan_id = document.sheet_set().sheets[0].viewports[0].id.clone();
        let legend_id = document.sheet_set().sheets[0].viewports[1].id.clone();
        let scope_now =
            |document: &Document| document.sheet_set().sheets[0].viewports[1].legend_scope;

        ok(set_legend_scope(
            &mut document,
            &legend_id,
            LegendScope::WholeSet,
        ));
        assert_eq!(scope_now(&document), LegendScope::WholeSet);
        ok(document.undo());
        assert_eq!(scope_now(&document), LegendScope::ThisSheet);
        ok(document.redo());
        assert_eq!(scope_now(&document), LegendScope::WholeSet);

        assert_eq!(
            refused(set_legend_scope(
                &mut document,
                &plan_id,
                LegendScope::WholeSet
            ))
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(set_legend_scope(
                &mut document,
                "vp404",
                LegendScope::WholeSet
            ))
            .code,
            ErrorCode::NotFound
        );
        assert_eq!(scope_now(&document), LegendScope::WholeSet);
    }

    // ---- stored with the viewport ----------------------------------------------------------

    #[test]
    fn the_scope_is_stored_only_when_not_the_default_and_read_back() {
        for scope in [
            LegendScope::ThisSheet,
            LegendScope::WholeSet,
            LegendScope::WholeDrawing,
        ] {
            assert_eq!(legend_scope_from(scope.name()), Some(scope));
        }
        assert!(legend_scope_from("everything").is_none());

        let mut set = one_sheet(vec![legend_panel("vp1")]);
        let text = ok(sheet_set_to_json(&set));
        assert!(!text.contains("legend_scope"));

        for scope in [LegendScope::WholeSet, LegendScope::WholeDrawing] {
            set.sheets[0].viewports[0].legend_scope = scope;
            let text = ok(sheet_set_to_json(&set));
            assert!(text.contains(&format!("\"legend_scope\":\"{}\"", scope.name())));
            let back = ok(sheet_set_from_json(&text));
            assert_eq!(back, set);
        }
    }

    #[test]
    fn an_unknown_scope_is_refused_when_read() {
        let error = refused(sheet_set_from_json(concat!(
            r#"{"format": "katana-sheets", "version": 1, "sheets": [{"id": "s1", "viewports": "#,
            r#"[{"id": "vp1", "kind": "legend", "legend_scope": "somewhere"}]}]}"#
        )));
        assert_eq!(error.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn the_legend_as_json_names_every_member_of_every_entry() {
        let legend = Legend {
            scope: LegendScope::WholeSet,
            windows: 2,
            scale: 500.0,
            entries: vec![katana_cad::plotting::LegendEntry {
                kind: LegendKind::Symbol,
                label: String::from("Stormwater pit"),
                style: String::from("Pit"),
                code: String::from("SP1"),
                colour: RED,
                symbol: String::from("square"),
                symbol_size: 1.5,
                count: 4,
                ..katana_cad::plotting::LegendEntry::default()
            }],
        };
        assert_eq!(
            legend_json(&legend),
            concat!(
                r##"{"entries":[{"code":"SP1","colour":"#DC0000","count":4,"hatch":"none","##,
                r#""kind":"symbol","label":"Stormwater pit","layer":"","linetype":"continuous","#,
                r#""style":"Pit","symbol":"square","symbol_size":1.5,"weight":0.25}],"#,
                r#""scale":500.0,"scope":"whole_set","windows":2}"#
            )
        );
    }

    // ---- the panel ---------------------------------------------------------------------------

    #[test]
    fn entries_flow_down_columns_as_wide_as_the_widest_label() {
        // 100 x 100 mm: rows from 8.25 mm under the top to 2.5 mm over the
        // bottom, 89.25 mm, hold 19 rows of 4.5 mm.
        let panel = Box2::new(Point2::new(300.0, 40.0), Point2::new(400.0, 140.0));
        let metrics = LegendMetrics::default();
        let layout = layout_legend(&panel, &[20.0; 3], &metrics);
        assert_eq!(layout.cells.len(), 3);
        assert_eq!(layout.columns, 1);
        assert_eq!(layout.rows, 3);
        // 10 of sample, 2 of gap, 20 of label.
        assert_eq!(layout.column_width, 32.0);
        near(layout.cells[0].sample.min.x, 302.5, 1e-9);
        near(layout.cells[0].sample.min.y, 127.7, 1e-9);
        near(layout.cells[0].sample.max.x, 312.5, 1e-9);
        near(layout.cells[0].sample.max.y, 131.3, 1e-9);
        assert_eq!(layout.cells[0].label, Point2::new(314.5, 129.5));
        assert_eq!(layout.cells[2].label, Point2::new(314.5, 120.5));
        assert_eq!(layout.cells[0].label_room, 20.0);
        assert_eq!(layout.more, 0);
        assert!(layout.more_at.is_none());

        // 25 entries need two columns; the rows are shared out, 13 and 12.
        let layout = layout_legend(&panel, &[20.0; 25], &metrics);
        assert_eq!(layout.cells.len(), 25);
        assert_eq!(layout.columns, 2);
        assert_eq!(layout.rows, 13);
        assert_eq!(
            layout.cells[13].label,
            Point2::new(302.5 + 32.0 + 4.0 + 12.0, 129.5)
        );
    }

    #[test]
    fn what_does_not_fit_is_counted_in_the_last_place() {
        let panel = Box2::new(Point2::new(300.0, 40.0), Point2::new(400.0, 140.0));
        let metrics = LegendMetrics::default();
        // Columns of 27 mm (10 + 2 + 15) and gaps of 4: three fit in 95 mm,
        // so 3 x 19 = 57 places, the last saying "+24 more".
        let many = [15.0; 80];
        let layout = layout_legend(&panel, &many, &metrics);
        assert_eq!(layout.columns, 3);
        assert_eq!(layout.rows, 19);
        assert_eq!(layout.cells.len(), 56);
        assert_eq!(layout.more, 24);
        assert_eq!(layout.more_at, Some(Point2::new(302.5 + 2.0 * 31.0, 48.5)));
        // With 32 mm columns only two fit: 38 places.
        let layout = layout_legend(&panel, &[20.0; 80], &metrics);
        assert_eq!(layout.columns, 2);
        assert_eq!(layout.cells.len(), 37);
        assert_eq!(layout.more, 43);

        // A label wider than the panel gets one column and is squeezed to it.
        let layout = layout_legend(&panel, &[500.0; 2], &metrics);
        assert_eq!(layout.columns, 1);
        assert_eq!(layout.column_width, 95.0);
        assert_eq!(layout.cells[0].label_room, 83.0);

        // Too low for a row: nothing placed, everything counted.
        let layout = layout_legend(
            &Box2::new(Point2::new(0.0, 0.0), Point2::new(100.0, 12.0)),
            &many,
            &metrics,
        );
        assert!(layout.cells.is_empty());
        assert_eq!(layout.more, 80);
        assert!(layout.more_at.is_none());
    }
}
