//! The twin of tests/qt_widgets/plotting/test_legend_properties.cpp's scope case: a Legend view's
//! scope chosen in the sheet editor's properties, one step each, and the summary under it saying
//! what it lists. Its summary case is SH1's (tests/sheets/legend_properties.rs).

mod sheet_legend_properties {
    use katana_cad::Document;
    use katana_cad::plotting::sheet_commands::add_sheets;
    use katana_cad::plotting::sheet_set::{LegendScope, Sheet, Viewport, ViewportKind};
    use katana_commands::{create_entities, create_layer};
    use katana_entity::{Entity, Geometry, Layer};
    use katana_geometry::{Box2, Point2, Segment2};

    use crate::support::{Shown, bx, source_with_document};

    fn plan_at(rect: Box2, centre: Point2) -> Viewport {
        Viewport {
            id: String::from("vp1"),
            kind: ViewportKind::Plan,
            rect,
            scale: 500.0,
            centre,
            ..Viewport::default()
        }
    }

    /// Two sheets: a plan of the west end with a legend beside it, and a plan of the east end. A
    /// line on WEST, one on EAST, and one nowhere near either.
    fn two_sheets(document: &mut Document) {
        for name in ["WEST", "EAST", "NOWHERE"] {
            document
                .execute(create_layer(Layer {
                    name: String::from(name),
                    ..Layer::default()
                }))
                .expect("the layer is added");
        }
        let line = |a: Point2, b: Point2, layer: &str| Entity {
            geometry: Geometry::Line(Segment2::new(a, b)),
            layer: String::from(layer),
            ..Entity::default()
        };
        document
            .execute(create_entities(vec![
                line(Point2::new(-100.0, 0.0), Point2::new(-90.0, 0.0), "WEST"),
                line(Point2::new(90.0, 0.0), Point2::new(100.0, 0.0), "EAST"),
                line(Point2::new(0.0, 900.0), Point2::new(1.0, 900.0), "NOWHERE"),
            ]))
            .expect("the lines are added");

        let legend = Viewport {
            id: String::from("vp2"),
            kind: ViewportKind::Legend,
            rect: bx(300.0, 40.0, 400.0, 140.0),
            ..Viewport::default()
        };
        let west = Sheet {
            id: String::from("s1"),
            name: String::from("WEST"),
            viewports: vec![
                plan_at(bx(30.0, 40.0, 230.0, 240.0), Point2::new(-95.0, 0.0)),
                legend,
            ],
            ..Sheet::default()
        };
        let east = Sheet {
            id: String::from("s2"),
            name: String::from("EAST"),
            viewports: vec![plan_at(bx(30.0, 40.0, 230.0, 240.0), Point2::new(95.0, 0.0))],
            ..Sheet::default()
        };
        add_sheets(document, vec![west, east], "ADD_SHEETS").expect("the sheets are added");
    }

    fn legend_of(shown: &Shown) -> Viewport {
        shown.document().sheet_set().sheets[0]
            .viewports
            .iter()
            .find(|viewport| viewport.kind == ViewportKind::Legend)
            .cloned()
            .expect("a legend")
    }

    fn summary_text(shown: &Shown) -> String {
        shown
            .control("sheetLegendSummary")
            .map(|summary| summary.text)
            .unwrap_or_default()
    }

    #[test]
    fn the_scope_choice_is_one_step_and_the_summary_says_what_it_lists() {
        let mut document = Document::new(None);
        two_sheets(&mut document);
        let mut shown = Shown::bare(document, source_with_document());
        let id = legend_of(&shown).id;
        shown.canvas_mut().select(&id);
        shown.draw();

        let scope = shown.control("sheetLegendScope").expect("the scope");
        assert_eq!(scope.items.len(), 3);
        assert_eq!(scope.value, "What this sheet's plans show");
        assert_eq!(summary_text(&shown), "1 entry from this sheet's 1 plan");

        let before = shown.steps();
        shown.fill("sheetLegendScope", "What every sheet's plans show");
        assert_eq!(legend_of(&shown).legend_scope, LegendScope::WholeSet);
        assert_eq!(shown.steps(), before + 1);
        // The properties were built again, from the edited viewport.
        assert_eq!(
            shown.control("sheetLegendScope").expect("the scope").value,
            "What every sheet's plans show"
        );
        assert_eq!(summary_text(&shown), "2 entries from 2 plans of the set");

        shown.fill("sheetLegendScope", "Everything in the drawing");
        assert_eq!(legend_of(&shown).legend_scope, LegendScope::WholeDrawing);
        assert_eq!(summary_text(&shown), "3 entries from the whole drawing");

        shown.undo();
        shown.undo();
        assert_eq!(legend_of(&shown).legend_scope, LegendScope::ThisSheet);
    }
}
