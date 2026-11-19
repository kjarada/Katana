//! The twin of tests/qt_widgets/drawing/test_vertex_panel.cpp: the Vertices panel
//! (drawing/vertex_panel) follows the selection, shows the polyline's vertices, and each edit
//! typed into it is one undoable step; and the drawing system's part of the window
//! (drawing/drawing_ui), whose toggles follow the document.
//!
//! The C++ panel reads the document again inside a notification; the port's reads it at its next
//! look, so a twin that changes the selection asks the panel to `follow` before it reads the
//! table. The buttons are pressed by their object names through the harness, as the C++ clicks
//! them.

use katana_cad::Document;
use katana_cad::snapping::{SnapMode, has_mode};
use katana_commands::{EntityAttributes, create_polyline};
use katana_entity::EntityId;
use katana_entity::entity::heights_of;
use katana_geometry::{Point2, Polyline2};
use katana_window::drawing::drawing_ui::DrawingUi;
use katana_window::drawing::vertex_panel::VertexPanel;
use katana_window::drive::Harness;

fn add_traverse(document: &mut Document) -> EntityId {
    document
        .execute(create_polyline(
            Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                    Point2::new(10.0, 10.0),
                ],
                false,
            ),
            EntityAttributes::default(),
        ))
        .expect("the traverse is made");
    document.last_created_entities()[0]
}

fn heights(document: &Document, id: EntityId) -> Vec<Option<f64>> {
    heights_of(
        &document
            .model()
            .entities
            .find(id)
            .expect("the traverse is there")
            .properties,
        3,
    )
}

mod vertex_panel_widget {
    use super::*;

    #[test]
    fn follows_the_selection() {
        let mut document = Document::new(None);
        let id = add_traverse(&mut document);
        let mut panel = VertexPanel::new(&document);
        assert!(panel.shown().is_none());
        assert_eq!(panel.row_count(), 0);
        document.selection_mut().add(id);
        document.notify_selection_changed();
        panel.follow(&document);
        assert_eq!(panel.shown(), Some(id));
        assert_eq!(panel.row_count(), 3);
        assert_eq!(panel.cell(1, 1), "10.000");
        document.selection_mut().clear();
        document.notify_selection_changed();
        panel.follow(&document);
        assert_eq!(panel.row_count(), 0);
    }

    #[test]
    fn an_edit_typed_into_the_table_is_one_undo_step() {
        let mut document = Document::new(None);
        let id = add_traverse(&mut document);
        document.selection_mut().add(id);
        document.notify_selection_changed();
        let mut panel = VertexPanel::new(&document);
        let before = document.history().undo_count();
        // Typed as a user types it: the cell's text changes and the panel reacts. Vertex 2's
        // height.
        panel.type_into_cell(&mut document, 2, 3, "104.5");
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(document.history().undo_name(), "VERTEX_SET");
        assert_eq!(heights(&document, id)[2], Some(104.5));
        assert_eq!(
            panel.cell(2, 3),
            "104.500",
            "shown as the drawing now has it"
        );
        // A value that does not parse changes nothing and puts the cell back.
        panel.type_into_cell(&mut document, 1, 1, "east");
        assert_eq!(document.history().undo_count(), before + 1);
        assert_eq!(panel.cell(1, 1), "10.000");
        document.undo().expect("the edit is undone");
        assert!(heights(&document, id)[2].is_none());
    }

    #[test]
    fn insert_and_delete_buttons() {
        let mut document = Document::new(None);
        let id = add_traverse(&mut document);
        document.selection_mut().add(id);
        document.notify_selection_changed();
        let mut panel = VertexPanel::new(&document);
        let mut harness = Harness::new();
        panel.set_current_row(Some(0));
        harness
            .press("vertexInsertButton")
            .run(|ui| panel.ui(ui, &mut document))
            .expect("Insert After is pressed");
        assert_eq!(panel.row_count(), 4);
        assert_eq!(panel.cell(1, 1), "5.000");
        panel.set_current_row(Some(1));
        harness
            .press("vertexDeleteButton")
            .run(|ui| panel.ui(ui, &mut document))
            .expect("Delete is pressed");
        assert_eq!(panel.row_count(), 3);
    }

    #[test]
    fn the_table_shows_whole_columns_at_a_panels_width_and_scrolls_the_rest_in() {
        // The fourth look audit (S\main_vertices.png): in the panel's 225 points the table's view
        // ended in the middle of the Height column's fields. It shows as many whole columns as
        // fit and the rest scroll in: no column is cut at the view's edge.
        let mut document = Document::new(None);
        let id = add_traverse(&mut document);
        document.selection_mut().add(id);
        document.notify_selection_changed();
        let mut panel = VertexPanel::new(&document);
        let mut harness = Harness::new();
        katana_window::theme::apply(harness.context());
        let mut draw = |ui: &mut egui::Ui| {
            ui.allocate_ui(egui::vec2(225.0, 400.0), |ui| {
                panel.ui(ui, &mut document);
            });
        };
        harness.show(&mut draw).expect("the panel is drawn");
        let output = harness.frame(&mut draw);
        let headers = [
            "#", "Easting", "Northing", "Height", "Bulge", "Bearing", "Distance",
        ];
        let mut seen = Vec::new();
        for clipped in &output.shapes {
            let egui::Shape::Text(text) = &clipped.shape else {
                continue;
            };
            let words = text.galley.text();
            if !headers.contains(&words) {
                continue;
            }
            let ink = text.visual_bounding_rect();
            if clipped.clip_rect.contains_rect(ink) {
                seen.push(String::from(words));
            } else {
                assert!(
                    !clipped.clip_rect.intersects(ink.shrink(0.5)),
                    "{words} is cut at the table's edge: {ink:?} in {:?}",
                    clipped.clip_rect
                );
            }
        }
        output.drop_without_applying_deltas();
        assert!(seen.len() >= 3, "the index and the coordinates: {seen:?}");
        assert!(seen.len() < headers.len(), "the premise: the rest scroll in");
        assert_eq!(seen, headers[..seen.len()], "the leading columns, whole");
        // A header out of the view is not painted at all (egui skips what it cannot see), or
        // painted wholly outside it - never cut.
        let table = harness.control("vertexTable").expect("drawn").rect;
        assert!(table.width() > 225.0, "the premise: the table is wider than its panel");
    }

    #[test]
    fn whole_columns_are_the_leading_ones_that_fit_and_the_first_always() {
        use katana_window::drawing::vertex_panel::whole_columns_width;
        let widths = [24.0, 58.0, 68.0, 56.0, 50.0];
        // 24 + 4 + 58 + 4 + 68 = 158; with the next, 218.
        assert_eq!(whole_columns_width(&widths, 4.0, 217.0), 158.0);
        assert_eq!(whole_columns_width(&widths, 4.0, 218.0), 218.0);
        assert_eq!(whole_columns_width(&widths, 4.0, 10_000.0), 272.0);
        assert_eq!(whole_columns_width(&widths, 4.0, 0.0), 24.0);
        assert_eq!(whole_columns_width(&[], 4.0, 100.0), 0.0);
    }
}

mod drawing_ui_install {
    use super::*;

    #[test]
    fn the_dock_and_the_drafting_toggles_follow_the_document() {
        let mut document = Document::new(None);
        let mut ui = DrawingUi::install(&document);
        assert_eq!(ui.dock().name, "VerticesDock");
        assert!(ui.action(&document, "actionOrtho").is_some());
        assert!(ui.trigger(&mut document, "actionOrtho"));
        assert!(document.drafting().ortho);
        // A verb (or anything else) that changes the setting checks the button.
        document.drafting_mut().ortho = false;
        document.notify_drafting_changed();
        let ortho = ui
            .action(&document, "actionOrtho")
            .expect("the Ortho toggle is there");
        assert_eq!(ortho.checked, Some(false));
        assert!(ui.action(&document, "actionSnapQuadrant").is_some());
        assert!(ui.trigger(&mut document, "actionSnapQuadrant"));
        assert!(has_mode(
            document.drafting().snap_modes,
            SnapMode::Quadrant
        ));
        assert!(ui.action(&document, "actionVerticesPanel").is_some());
    }
}
