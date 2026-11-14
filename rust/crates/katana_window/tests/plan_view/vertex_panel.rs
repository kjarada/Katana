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
